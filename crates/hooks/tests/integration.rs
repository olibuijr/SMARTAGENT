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
    write_exec(&root.join("hooks.d/slow.sh"), "#!/bin/sh\nsleep 30\n");
    std::fs::write(root.join("config/hooks.conf"), "[hook]\nname = slow\nevent = stop\ncommand = hooks.d/slow.sh\ntimeout = 1\n").unwrap();
    let hooks = hooks::config::load(&root.join("config/hooks.conf")).unwrap();
    let t0 = std::time::Instant::now();
    let d = hooks::dispatch::dispatch(&hooks, &root, "stop", "", "{}");
    assert!(!d.block);
    assert!(d.warnings.iter().any(|w| w.contains("timed out")), "{:?}", d.warnings);
    // Proves the 30s sleep was cut short at the 1s timeout. Margin is wide
    // (15s) because the full ./build.sh gate runs this under heavy parallel
    // load, where <5s flaked twice on spawn+kill scheduling alone.
    assert!(t0.elapsed().as_secs() < 15);
}
