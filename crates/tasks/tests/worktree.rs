use std::path::{Path, PathBuf};
use std::process::Command;
use tasks::worktree;

fn run(dir: &Path, args: &[&str]) {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
}

fn repo(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Command::new("git").args(["init", "-b", "main"]).arg(&d).output().unwrap();
    run(&d, &["config", "user.email", "test@example.invalid"]);
    run(&d, &["config", "user.name", "Test"]);
    std::fs::write(d.join("README.md"), "base\n").unwrap();
    run(&d, &["add", "."]);
    run(&d, &["commit", "-m", "base"]);
    d
}

#[test]
fn worktree_lifecycle_create_merge_isolate_and_reap() {
    let d = repo("tasks-worktree-lifecycle");
    std::env::remove_var("SMARTAGENT_WORKTREE_DISABLE");
    std::env::set_var("SMARTAGENT_WORKTREE_ROOT", &d);

    let out = worktree::ensure_for_doing("T-900").unwrap();
    assert!(out.contains("worktrees/T-900"), "{out}");
    assert!(d.join("worktrees/T-900/.git").exists());
    let branches = Command::new("git").arg("-C").arg(&d).args(["branch", "--list", "task/T-900"]).output().unwrap();
    assert!(String::from_utf8_lossy(&branches.stdout).contains("task/T-900"));

    worktree::ensure_for_doing("T-901").unwrap();
    std::fs::write(d.join("worktrees/T-901/result.txt"), "isolated\n").unwrap();
    let out = worktree::finish_done("T-901").unwrap();
    assert!(out.contains("merged task/T-901"), "{out}");
    assert!(d.join("result.txt").exists());
    assert!(!d.join("worktrees/T-901").exists());

    worktree::ensure_for_doing("T-902").unwrap();
    worktree::ensure_for_doing("T-903").unwrap();
    let a = d.join("worktrees/T-902/crates/demo/src/lib.rs");
    let b = d.join("worktrees/T-903/crates/demo/src/lib.rs");
    assert!(worktree::path_allowed_for_task("T-902", &a).unwrap());
    assert!(!worktree::path_allowed_for_task("T-902", &b).unwrap());

    worktree::ensure_for_doing("T-904").unwrap();
    let out = worktree::reap_abandoned(0).unwrap();
    assert!(out.contains("reaped"), "{out}");
    assert!(!d.join("worktrees/T-904").exists());
}
