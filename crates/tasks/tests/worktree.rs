use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use tasks::worktree;

/// `worktree::finish_done` mutates process-global env vars
/// (`SMARTAGENT_WORKTREE_ROOT`/`_DISABLE`) to pick a repo root, and
/// `cargo test` runs #[test] fns in this binary on parallel threads by
/// default. Serialize the whole-test bodies below so they don't stomp on
/// each other's env var. (Doesn't affect the intra-test concurrency test,
/// which spawns its OWN worker threads inside one already-serialized test.)
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_guard() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn run(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn porcelain_empty(dir: &Path) -> bool {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().is_empty()
}

fn repo(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&d)
        .output()
        .unwrap();
    run(&d, &["config", "user.email", "test@example.invalid"]);
    run(&d, &["config", "user.name", "Test"]);
    std::fs::write(d.join("README.md"), "base\n").unwrap();
    // Mirror the real repo's convention: worktrees/ (task dirs and this
    // module's throwaway merge-scratch dirs both live under it) is
    // gitignored, so `git status --porcelain` on the main checkout is not
    // polluted by in-flight or preserved task worktrees.
    std::fs::write(d.join(".gitignore"), "worktrees/\n").unwrap();
    run(&d, &["add", "."]);
    run(&d, &["commit", "-m", "base"]);
    d
}

#[test]
fn worktree_lifecycle_create_merge_isolate_and_reap() {
    let _guard = env_guard();
    let d = repo("tasks-worktree-lifecycle");
    std::env::remove_var("SMARTAGENT_WORKTREE_DISABLE");
    std::env::set_var("SMARTAGENT_WORKTREE_ROOT", &d);

    let out = worktree::ensure_for_doing("T-900").unwrap();
    assert!(out.contains("worktrees/T-900"), "{out}");
    assert!(d.join("worktrees/T-900/.git").exists());
    let branches = Command::new("git")
        .arg("-C")
        .arg(&d)
        .args(["branch", "--list", "task/T-900"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&branches.stdout).contains("task/T-900"));

    // (a) Clean merge: main checkout ends up clean and fast-forwarded onto
    // the merge computed in the isolated throwaway worktree — no residue.
    worktree::ensure_for_doing("T-901").unwrap();
    std::fs::write(d.join("worktrees/T-901/result.txt"), "isolated\n").unwrap();
    let out = worktree::finish_done("T-901").unwrap();
    assert!(out.contains("merged task/T-901"), "{out}");
    assert!(d.join("result.txt").exists());
    assert!(!d.join("worktrees/T-901").exists());
    assert!(
        porcelain_empty(&d),
        "main checkout left dirty after clean merge"
    );
    assert!(
        !d.join(".git/MERGE_HEAD").exists(),
        "main checkout left mid-merge"
    );
    // The root's own operation was a pure fast-forward (moved the ref, no
    // in-place merge command run there) — the reflog says so.
    let reflog = Command::new("git")
        .arg("-C")
        .arg(&d)
        .args(["reflog", "-1"])
        .output()
        .unwrap();
    let reflog_line = String::from_utf8_lossy(&reflog.stdout).to_lowercase();
    assert!(
        reflog_line.contains("fast-forward") || reflog_line.contains("merge"),
        "{reflog_line}"
    );
    // No merge-scratch state left behind under worktrees/ either.
    let leftover = std::fs::read_dir(d.join("worktrees"))
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().starts_with(".merge-"));
    assert!(!leftover, "isolated merge scratch worktree not cleaned up");

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
    assert!(
        !worktree::has_worktree("T-999"),
        "unknown task has no worktree"
    );
    assert!(
        !worktree::changed("T-910").unwrap(),
        "fresh worktree has no changes"
    );
    std::fs::write(d.join("worktrees/T-910/fix.rs"), "// real fix\n").unwrap();
    assert!(
        worktree::changed("T-910").unwrap(),
        "uncommitted edit counts as changed"
    );

    worktree::ensure_for_doing("T-911").unwrap();
    assert!(!worktree::changed("T-911").unwrap());
    std::fs::write(d.join("worktrees/T-911/f.txt"), "x\n").unwrap();
    run(&d.join("worktrees/T-911"), &["add", "-A"]);
    run(&d.join("worktrees/T-911"), &["commit", "-m", "T-911 work"]);
    assert!(
        worktree::changed("T-911").unwrap(),
        "committed-ahead counts as changed"
    );

    // (b) Conflict-abort: a task branch that conflicts with the base must
    // NOT corrupt the base. The conflict is discovered and aborted in the
    // isolated throwaway worktree — the MAIN checkout is never touched at
    // all, so it stays perfectly clean (no UU/M, empty porcelain).
    std::fs::write(d.join("conflict.txt"), "base line\n").unwrap();
    run(&d, &["add", "-A"]);
    run(&d, &["commit", "-m", "add conflict.txt"]);
    worktree::ensure_for_doing("T-920").unwrap();
    std::fs::write(d.join("worktrees/T-920/conflict.txt"), "worktree version\n").unwrap();
    run(&d.join("worktrees/T-920"), &["add", "-A"]);
    run(
        &d.join("worktrees/T-920"),
        &["commit", "-m", "T-920 change"],
    );
    std::fs::write(d.join("conflict.txt"), "main version\n").unwrap();
    run(&d, &["add", "-A"]);
    run(&d, &["commit", "-m", "main diverges same line"]);
    let out = worktree::finish_done("T-920").unwrap_err();
    assert!(
        out.contains("MERGE CONFLICT"),
        "expected conflict abort, got: {out}"
    );
    assert!(!d.join(".git/MERGE_HEAD").exists(), "base left mid-merge");
    assert_eq!(
        std::fs::read_to_string(d.join("conflict.txt")).unwrap(),
        "main version\n",
        "base corrupted by aborted merge"
    );
    assert!(
        d.join("worktrees/T-920").exists(),
        "worktree must be preserved on conflict"
    );
    assert!(
        porcelain_empty(&d),
        "main checkout left dirty (UU/M) by aborted conflict merge"
    );
    let leftover = std::fs::read_dir(d.join("worktrees"))
        .unwrap()
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().starts_with(".merge-"));
    assert!(
        !leftover,
        "isolated merge scratch worktree not cleaned up after conflict"
    );

    worktree::ensure_for_doing("T-930").unwrap();
    std::fs::write(d.join("worktrees/T-930/unmerged.txt"), "branch-only\n").unwrap();
    run(&d.join("worktrees/T-930"), &["add", "-A"]);
    run(
        &d.join("worktrees/T-930"),
        &["commit", "-m", "T-930 branch-only"],
    );
    let out = worktree::finish_done("T-930").unwrap();
    assert!(out.contains("merged task/T-930"), "{out}");
    let ancestor = Command::new("git")
        .arg("-C")
        .arg(&d)
        .args(["merge-base", "--is-ancestor", "HEAD", "main"])
        .status()
        .unwrap();
    assert!(
        ancestor.success(),
        "main must contain the merge commit after finish_done reports success"
    );
    assert!(
        d.join("unmerged.txt").exists(),
        "main checkout must contain task branch changes before done succeeds"
    );

    worktree::ensure_for_doing("T-904").unwrap();
    let out = worktree::reap_abandoned(0).unwrap();
    assert!(out.contains("reaped"), "{out}");
    assert!(!d.join("worktrees/T-904").exists());
}

#[test]
fn sequential_merges_leave_no_residue() {
    // (c, sequential half) Two `tasks done` merges attempted one after the
    // other must both land cleanly — no leftover state from the first
    // merge (lockfile, scratch worktree, dirty index) can block the second.
    let _guard = env_guard();
    let d = repo("tasks-worktree-sequential");
    std::env::remove_var("SMARTAGENT_WORKTREE_DISABLE");
    std::env::set_var("SMARTAGENT_WORKTREE_ROOT", &d);

    worktree::ensure_for_doing("T-960").unwrap();
    std::fs::write(d.join("worktrees/T-960/one.txt"), "one\n").unwrap();
    let out1 = worktree::finish_done("T-960").unwrap();
    assert!(out1.contains("merged task/T-960"), "{out1}");
    assert!(porcelain_empty(&d), "residue after first merge");
    assert!(
        !d.join(".git/tasks-merge.lock").exists(),
        "lock not released after first merge"
    );

    worktree::ensure_for_doing("T-961").unwrap();
    std::fs::write(d.join("worktrees/T-961/two.txt"), "two\n").unwrap();
    let out2 = worktree::finish_done("T-961").unwrap();
    assert!(out2.contains("merged task/T-961"), "{out2}");
    assert!(porcelain_empty(&d), "residue after second merge");
    assert!(
        !d.join(".git/tasks-merge.lock").exists(),
        "lock not released after second merge"
    );

    assert!(d.join("one.txt").exists());
    assert!(d.join("two.txt").exists());
    assert!(!d.join("worktrees/T-960").exists());
    assert!(!d.join("worktrees/T-961").exists());
}

#[test]
fn concurrent_merges_are_serialized_without_residue() {
    // (c, concurrent half) Reproduces the original live bug directly: two
    // fleet agents calling `tasks done` at the same moment. The merge lock
    // must serialize them so the shared main checkout only ever sees clean
    // fast-forwards — never an interleaved `git merge` race.
    let _guard = env_guard();
    let d = repo("tasks-worktree-concurrent");
    std::env::remove_var("SMARTAGENT_WORKTREE_DISABLE");
    std::env::set_var("SMARTAGENT_WORKTREE_ROOT", &d);

    worktree::ensure_for_doing("T-970").unwrap();
    worktree::ensure_for_doing("T-971").unwrap();
    std::fs::write(d.join("worktrees/T-970/left.txt"), "left\n").unwrap();
    std::fs::write(d.join("worktrees/T-971/right.txt"), "right\n").unwrap();

    let t1 = std::thread::spawn(|| worktree::finish_done("T-970"));
    let t2 = std::thread::spawn(|| worktree::finish_done("T-971"));
    let r1 = t1.join().expect("T-970 merge thread panicked");
    let r2 = t2.join().expect("T-971 merge thread panicked");

    assert!(
        r1.as_deref().unwrap_or("").contains("merged task/T-970"),
        "{r1:?}"
    );
    assert!(
        r2.as_deref().unwrap_or("").contains("merged task/T-971"),
        "{r2:?}"
    );
    assert!(d.join("left.txt").exists());
    assert!(d.join("right.txt").exists());
    assert!(!d.join("worktrees/T-970").exists());
    assert!(!d.join("worktrees/T-971").exists());
    assert!(
        porcelain_empty(&d),
        "residue left on main after concurrent merges"
    );
    assert!(
        !d.join(".git/MERGE_HEAD").exists(),
        "main left mid-merge by a racing merge"
    );
    assert!(
        !d.join(".git/tasks-merge.lock").exists(),
        "lock not released after concurrent merges"
    );
}
