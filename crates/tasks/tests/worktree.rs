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

    // Change-detection powers the done-gate: a fresh worktree has no changes
    // (the false-done fingerprint); an edited or committed one counts as work.
    worktree::ensure_for_doing("T-910").unwrap();
    assert!(worktree::has_worktree("T-910"));
    assert!(!worktree::has_worktree("T-999"), "unknown task has no worktree");
    assert!(!worktree::changed("T-910").unwrap(), "fresh worktree has no changes");
    std::fs::write(d.join("worktrees/T-910/fix.rs"), "// real fix\n").unwrap();
    assert!(worktree::changed("T-910").unwrap(), "uncommitted edit counts as changed");

    worktree::ensure_for_doing("T-911").unwrap();
    assert!(!worktree::changed("T-911").unwrap());
    std::fs::write(d.join("worktrees/T-911/f.txt"), "x\n").unwrap();
    run(&d.join("worktrees/T-911"), &["add", "-A"]);
    run(&d.join("worktrees/T-911"), &["commit", "-m", "T-911 work"]);
    assert!(worktree::changed("T-911").unwrap(), "committed-ahead counts as changed");

    // Conflict-abort: a task branch that conflicts with the base must NOT corrupt
    // the base — finish_done aborts the merge and preserves the worktree/branch.
    std::fs::write(d.join("conflict.txt"), "base line\n").unwrap();
    run(&d, &["add", "-A"]);
    run(&d, &["commit", "-m", "add conflict.txt"]);
    worktree::ensure_for_doing("T-920").unwrap();
    std::fs::write(d.join("worktrees/T-920/conflict.txt"), "worktree version\n").unwrap();
    run(&d.join("worktrees/T-920"), &["add", "-A"]);
    run(&d.join("worktrees/T-920"), &["commit", "-m", "T-920 change"]);
    std::fs::write(d.join("conflict.txt"), "main version\n").unwrap();
    run(&d, &["add", "-A"]);
    run(&d, &["commit", "-m", "main diverges same line"]);
    let out = worktree::finish_done("T-920").unwrap();
    assert!(out.contains("MERGE CONFLICT"), "expected conflict abort, got: {out}");
    assert!(!d.join(".git/MERGE_HEAD").exists(), "base left mid-merge");
    assert_eq!(std::fs::read_to_string(d.join("conflict.txt")).unwrap(), "main version\n", "base corrupted by aborted merge");
    assert!(d.join("worktrees/T-920").exists(), "worktree must be preserved on conflict");

    worktree::ensure_for_doing("T-904").unwrap();
    let out = worktree::reap_abandoned(0).unwrap();
    assert!(out.contains("reaped"), "{out}");
    assert!(!d.join("worktrees/T-904").exists());
}
