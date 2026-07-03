use std::path::PathBuf;
use std::sync::Mutex;
use tasks::cli;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn db(name: &str) -> String {
    std::env::set_var("SMARTAGENT_WORKTREE_DISABLE", "1");
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    d.join("tasks.semdb").display().to_string()
}

fn s(v: &[&str], db: &str) -> Vec<String> {
    let mut a: Vec<String> = v.iter().map(|x| x.to_string()).collect();
    a.push("--db".into());
    a.push(db.into());
    a
}

#[test]
fn kanban_flow_wip_and_done_gates() {
    let db = db("tasks-flow");
    // capture + promote
    cli::run(&s(
        &[
            "add",
            "ship feature",
            "--prio",
            "p1",
            "--col",
            "ready",
            "--criteria",
            "compiles;tested",
        ],
        &db,
    ))
    .unwrap();
    cli::run(&s(&["todo", "someday thing"], &db)).unwrap();
    // pull respects capacity (doing WIP default 1)
    let pull = cli::run(&s(&["next"], &db)).unwrap();
    assert!(pull.contains("pull T-1"), "{pull}");
    cli::run(&s(&["move", "T-1", "doing"], &db)).unwrap();
    // WIP limit blocks a second doing task
    cli::run(&s(&["add", "second", "--col", "ready"], &db)).unwrap();
    let err = cli::run(&s(&["move", "T-3", "doing"], &db)).unwrap_err();
    assert!(err.contains("WIP limit"), "{err}");
    // pull explains instead of over-committing
    let full = cli::run(&s(&["next"], &db)).unwrap();
    assert!(full.contains("WIP full"), "{full}");
    // done is criteria-gated
    let gate = cli::run(&s(&["done", "T-1"], &db)).unwrap_err();
    assert!(gate.contains("unchecked criteria"), "{gate}");
    cli::run(&s(&["crit", "check", "T-1", "1"], &db)).unwrap();
    cli::run(&s(&["crit", "check", "T-1", "2"], &db)).unwrap();
    let done = cli::run(&s(&["done", "T-1"], &db)).unwrap();
    assert!(done.contains("→ done"), "{done}");
    // board + metrics render
    let board = cli::run(&s(&["board"], &db)).unwrap();
    assert!(
        board.contains("DONE (1)") && board.contains("BACKLOG"),
        "{board}"
    );
    let m = cli::run(&s(&["metrics"], &db)).unwrap();
    assert!(m.contains("throughput: 1 done"), "{m}");
}

#[test]
fn desktop_agent_tasks_are_not_pullable_by_fleet() {
    let db = db("tasks-desktop-agent-skip");
    cli::run(&s(
        &[
            "add",
            "desktop-agent: separate session work",
            "--prio",
            "p1",
            "--col",
            "ready",
            "--tags",
            "desktop-agent",
        ],
        &db,
    ))
    .unwrap();
    cli::run(&s(
        &["add", "normal fleet work", "--prio", "p2", "--col", "ready"],
        &db,
    ))
    .unwrap();
    let pull = cli::run(&s(&["next"], &db)).unwrap();
    assert!(pull.contains("pull T-2"), "{pull}");
    assert!(!pull.contains("T-1"), "{pull}");
}

#[test]
fn unblock_makes_ready_task_pullable_again() {
    let db = db("tasks-unblock-pullable");
    cli::run(&s(
        &[
            "add",
            "blocked then clear",
            "--prio",
            "p1",
            "--col",
            "ready",
        ],
        &db,
    ))
    .unwrap();
    cli::run(&s(&["block", "T-1", "waiting on external"], &db)).unwrap();
    let blocked = cli::run(&s(&["next"], &db)).unwrap();
    assert!(blocked.contains("no ready tasks"), "{blocked}");
    cli::run(&s(&["unblock", "T-1"], &db)).unwrap();
    let pull = cli::run(&s(&["next"], &db)).unwrap();
    assert!(pull.contains("pull T-1"), "{pull}");
}

#[test]
fn block_and_statusline_levels() {
    let db = db("tasks-status");
    let ok = cli::run(&s(&["statusline"], &db)).unwrap();
    assert!(ok.starts_with("ok|▣"), "{ok}");
    cli::run(&s(&["add", "x", "--col", "ready"], &db)).unwrap();
    cli::run(&s(&["block", "T-1", "waiting on titan"], &db)).unwrap();
    let warn = cli::run(&s(&["statusline"], &db)).unwrap();
    assert!(
        warn.starts_with("warn|") && warn.contains("1 blocked"),
        "{warn}"
    );
    // forced over-WIP shows err
    cli::run(&s(&["unblock", "T-1"], &db)).unwrap();
    cli::run(&s(&["move", "T-1", "doing"], &db)).unwrap();
    cli::run(&s(&["add", "y", "--col", "ready"], &db)).unwrap();
    cli::run(&s(&["move", "T-2", "doing", "--force"], &db)).unwrap();
    let err = cli::run(&s(&["statusline"], &db)).unwrap();
    assert!(err.starts_with("err|"), "{err}");
    let board = cli::run(&s(&["board"], &db)).unwrap();
    assert!(board.contains("OVER-WIP"), "{board}");
}

#[test]
fn done_runs_checked_cargo_criteria() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::remove_var("SMARTAGENT_TASKS_DONE_TIMEOUT_SECS");
    let db = db("tasks-done-cargo-check");
    cli::run(&s(
        &[
            "add",
            "cargo criterion",
            "--col",
            "ready",
            "--criteria",
            "cargo --version;cargo check -p tasks",
        ],
        &db,
    ))
    .unwrap();
    cli::run(&s(&["move", "T-1", "doing"], &db)).unwrap();
    cli::run(&s(&["crit", "check", "T-1", "1"], &db)).unwrap();
    cli::run(&s(&["crit", "check", "T-1", "2"], &db)).unwrap();
    let done = cli::run(&s(&["done", "T-1"], &db)).unwrap();
    assert!(done.contains("→ done"), "{done}");
}

#[test]
fn done_cargo_criteria_are_timeout_guarded() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch")
        .join("tasks-done-cargo-timeout");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let fake = scratch.join("cargo");
    std::fs::write(&fake, "#!/bin/sh\nsleep 5\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&fake).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake, perms).unwrap();
    }
    let old_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{old_path}", scratch.display()));
    std::env::set_var("SMARTAGENT_TASKS_DONE_TIMEOUT_SECS", "1");

    let db = scratch.join("tasks.semdb").display().to_string();
    cli::run(&s(
        &[
            "add",
            "slow cargo criterion",
            "--col",
            "ready",
            "--criteria",
            "cargo test --version",
        ],
        &db,
    ))
    .unwrap();
    cli::run(&s(&["move", "T-1", "doing"], &db)).unwrap();
    cli::run(&s(&["crit", "check", "T-1", "1"], &db)).unwrap();
    let err = cli::run(&s(&["done", "T-1"], &db)).unwrap_err();
    std::env::set_var("PATH", old_path);
    std::env::remove_var("SMARTAGENT_TASKS_DONE_TIMEOUT_SECS");
    assert!(err.contains("timed out"), "{err}");
}
