use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("hooks.d")).unwrap();
    std::fs::create_dir_all(d.join("config")).unwrap();
    d
}

fn write_exec(path: &PathBuf, body: &str) {
    std::fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn block_context_and_rewrite_contract() {
    let root = scratch("hooks-e2e");
    write_exec(&root.join("hooks.d/block-rm.sh"), "#!/bin/sh\ngrep -q 'rm -rf' && { echo 'destructive command refused' >&2; exit 2; }\nexit 0\n");
    write_exec(&root.join("hooks.d/inject.sh"), "#!/bin/sh\ncat >/dev/null\necho 'remember: board first'\n");
    write_exec(&root.join("hooks.d/rewrite.sh"), "#!/bin/sh\ncat >/dev/null\necho '{\"updatedInput\":{\"cmd\":\"ls -la\"}}'\n");
    std::fs::write(
        root.join("config/hooks.conf"),
        "[hook]\nname = no-rm\nevent = tool_call\nmatcher = bash\ncommand = hooks.d/block-rm.sh\n\n[hook]\nname = ctx\nevent = session_start\ncommand = hooks.d/inject.sh\n\n[hook]\nname = rw\nevent = tool_call\nmatcher = bash\ncommand = hooks.d/rewrite.sh\n",
    )
    .unwrap();
    let hooks = hooks::config::load(&root.join("config/hooks.conf")).unwrap();
    assert_eq!(hooks.len(), 3);

    // exit-2 block, stderr as reason, later hooks skipped
    let d = hooks::dispatch::dispatch(&hooks, &root, "tool_call", "bash", r#"{"cmd":"rm -rf /"}"#);
    assert!(d.block);
    assert!(d.reason.contains("destructive"), "{}", d.reason);

    // benign call → no block, rewrite hook's updatedInput captured
    let d = hooks::dispatch::dispatch(&hooks, &root, "tool_call", "bash", r#"{"cmd":"ls"}"#);
    assert!(!d.block);
    assert!(d.updated_input.contains("ls -la"), "{}", d.updated_input);

    // plain-text stdout = context injection
    let d = hooks::dispatch::dispatch(&hooks, &root, "session_start", "startup", "{}");
    assert_eq!(d.context, vec!["remember: board first".to_string()]);

    // non-matching subject → nothing fires
    let d = hooks::dispatch::dispatch(&hooks, &root, "tool_call", "read", r#"{"cmd":"rm -rf /"}"#);
    assert!(!d.block && d.updated_input.is_empty());

    // audit rows landed
    assert!(hooks::dispatch::audit_path(&root).exists());
}

#[test]
fn timeout_fails_open_with_warning() {
    let root = scratch("hooks-timeout");
    // The hook's own sleep is a full 20 minutes — effectively "never" for a
    // test. dispatch()'s poll loop (25ms ticks against a 1s deadline, see
    // run_hook in src/dispatch.rs) enforces the 1s timeout by killing the
    // child; it does NOT wait for the child to exit on its own. What made
    // the old version of this test flaky wasn't the kill/timeout logic
    // itself — it was a *wall-clock upper bound* (previously <5s, then
    // <15s) racing against real scheduling jitter: under heavy parallel CPU
    // load (8 concurrent cargo builds, load ~25) the test thread's
    // thread::sleep(25ms) ticks and the spawn/kill/wait syscalls can each
    // get delayed by seconds, so any bound in the same order of magnitude
    // as the 1s deadline can flake even though the mechanism is working
    // correctly.
    //
    // Fix: keep proving the exact same fail-open behavior (not block, warn
    // "timed out"), but make the wall-clock assertion deterministic by
    // making it a bound relative to the sleep, not an absolute number
    // tuned to "typical" scheduling latency. 20 minutes of headroom between
    // "killed early" (order of seconds, even badly starved) and "ran to
    // completion" (1200s) means a generous 120s ceiling unambiguously
    // distinguishes the two: if the timeout enforcement ever regressed to
    // "wait for the child to exit," this would take ~1200s and fail hard;
    // any real kill, however delayed by scheduling, lands nowhere near that.
    write_exec(&root.join("hooks.d/slow.sh"), "#!/bin/sh\nsleep 1200\n");
    std::fs::write(root.join("config/hooks.conf"), "[hook]\nname = slow\nevent = stop\ncommand = hooks.d/slow.sh\ntimeout = 1\n").unwrap();
    let hooks = hooks::config::load(&root.join("config/hooks.conf")).unwrap();
    let t0 = std::time::Instant::now();
    let d = hooks::dispatch::dispatch(&hooks, &root, "stop", "", "{}");
    assert!(!d.block, "a timed-out hook must fail OPEN, never block");
    assert!(d.warnings.iter().any(|w| w.contains("timed out")), "{:?}", d.warnings);
    let elapsed = t0.elapsed();
    assert!(
        elapsed.as_secs() < 120,
        "timeout enforcement did not cut the hook short (elapsed {elapsed:?} against a 1200s sleep) — \
         the 1s deadline appears not to have been enforced"
    );
}

#[test]
fn spawn_retries_past_transient_text_file_busy() {
    // Found while chasing the flake above: running this file's two tests
    // concurrently (the default `cargo test` behavior — each #[test] gets
    // its own thread) intermittently made `block_context_and_rewrite_contract`
    // fail with "spawn ...: Text file busy" (ETXTBSY) — NOT a timing issue,
    // a real spawn error. Root cause, isolated with a standalone repro: a
    // hook script that was just written+closed (`fs::write` then close) can
    // still transiently look "open for write" to the kernel for a moment,
    // and exec'ing it during that window fails with ETXTBSY — even though
    // the file is a completely ordinary, fully-written, executable script.
    // It always self-clears in milliseconds. dispatch::spawn_hook now
    // retries a bounded number of times on exactly this error kind.
    //
    // This test reproduces the condition deterministically (no timing race
    // needed): ETXTBSY is guaranteed by the kernel while ANY fd has the
    // target script open for writing, so we hold one open, dispatch on a
    // background thread, and close it a beat later from this thread. If
    // spawn_hook's retry didn't exist, dispatch would report a "spawn ...
    // busy" warning and never see the hook run at all.
    let root = scratch("hooks-spawn-busy");
    write_exec(&root.join("hooks.d/echo.sh"), "#!/bin/sh\ncat >/dev/null\necho ran\n");
    std::fs::write(root.join("config/hooks.conf"), "[hook]\nname = echo\nevent = tool_call\nmatcher = bash\ncommand = hooks.d/echo.sh\n").unwrap();
    let hooks = hooks::config::load(&root.join("config/hooks.conf")).unwrap();

    let script = root.join("hooks.d/echo.sh");
    let busy_fd = std::fs::OpenOptions::new().write(true).open(&script).unwrap();

    let root2 = root.clone();
    let hooks2 = hooks.clone();
    let dispatcher = std::thread::spawn(move || hooks::dispatch::dispatch(&hooks2, &root2, "tool_call", "bash", r#"{"cmd":"ls"}"#));

    // Give dispatch a moment to hit spawn while the fd is still held open —
    // this is what forces the ETXTBSY path deterministically — then release
    // it well within spawn_hook's retry budget.
    std::thread::sleep(std::time::Duration::from_millis(30));
    drop(busy_fd);

    let d = dispatcher.join().expect("dispatch thread panicked");
    assert!(
        d.warnings.iter().all(|w| !w.to_lowercase().contains("busy")),
        "spawn did not recover from transient ETXTBSY: {:?}",
        d.warnings
    );
    assert!(d.context.iter().any(|c| c.contains("ran")), "hook never actually ran (context: {:?})", d.context);
}
