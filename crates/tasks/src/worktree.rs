//! Git worktree lifecycle for fleet task isolation.
//!
//! Best-effort by default: if cwd is not a git repo, lifecycle is skipped so
//! task boards remain usable in tests/scratch repos. Set
//! `SMARTAGENT_WORKTREE_STRICT=1` to turn lifecycle failures into task errors.
//!
//! Task merges (`finish_done`) never run `git merge` in the shared main
//! checkout. Concurrent fleet agents calling `tasks done` at the same time
//! used to race on `git merge` there, leaving `UU`/`M` residue with no
//! MERGE_HEAD that then blocked every subsequent merge. Instead the merge is
//! computed in an isolated, throwaway `git worktree` and the main checkout is
//! only ever fast-forwarded onto the resulting commit — a fast-forward can
//! neither conflict nor leave residue. See `merge_lock`/`merge_isolated`.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn strict() -> bool { std::env::var("SMARTAGENT_WORKTREE_STRICT").is_ok() }
fn disabled() -> bool { std::env::var("SMARTAGENT_WORKTREE_DISABLE").is_ok() }

fn repo_root() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("SMARTAGENT_WORKTREE_ROOT") { return Ok(PathBuf::from(p)); }
    let out = Command::new("git").args(["rev-parse", "--show-toplevel"]).output().map_err(|e| e.to_string())?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).trim().to_string()); }
    Ok(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().map_err(|e| e.to_string())?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).trim().to_string()); }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn task_branch(id: &str) -> String { format!("task/{id}") }
fn task_dir(root: &Path, id: &str) -> PathBuf { root.join("worktrees").join(id) }

pub fn ensure_for_doing(id: &str) -> Result<String, String> {
    if disabled() { return Ok(String::new()); }
    let root = match repo_root() { Ok(r) => r, Err(e) if strict() => return Err(e), Err(_) => return Ok(String::new()) };
    let dir = task_dir(&root, id);
    if dir.exists() { return Ok(format!("\nworktree: {}", dir.display())); }
    fs::create_dir_all(root.join("worktrees")).map_err(|e| e.to_string())?;
    let branch = task_branch(id);
    match run(&root, &["worktree", "add", "-B", &branch, dir.to_str().unwrap_or(""), "HEAD"]) {
        Ok(_) => Ok(format!("\nworktree: {} on {branch}", dir.display())),
        Err(e) if strict() => Err(format!("worktree create failed: {e}")),
        Err(e) => Ok(format!("\nworktree warning: {e}")),
    }
}

/// True if a live worktree exists for this task (isolation on + dir present).
/// The done-gate uses this so non-isolated setups skip the change check.
pub fn has_worktree(id: &str) -> bool {
    if disabled() { return false; }
    match repo_root() {
        Ok(root) => task_dir(&root, id).exists(),
        Err(_) => false,
    }
}

/// True if the task's worktree holds real work: uncommitted edits in its
/// working tree, or commits on task/<id> ahead of the base branch. The
/// done-gate rejects "done" when this is false — that is the exact signature
/// of a false-done (criteria checked, but the fix was never built).
pub fn changed(id: &str) -> Result<bool, String> {
    let root = repo_root()?;
    let dir = task_dir(&root, id);
    if !dir.exists() { return Ok(false); }
    // Uncommitted edits in the worktree working directory.
    if run(&dir, &["status", "--porcelain"]).map(|s| !s.trim().is_empty()).unwrap_or(false) {
        return Ok(true);
    }
    // Commits on the task branch beyond the base (checked-out) branch.
    let base = run(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "main".into());
    let branch = task_branch(id);
    let ahead = run(&root, &["rev-list", "--count", &format!("{base}..{branch}")]).ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0);
    Ok(ahead > 0)
}

pub fn finish_done(id: &str) -> Result<String, String> {
    if disabled() { return Ok(String::new()); }
    let root = match repo_root() { Ok(r) => r, Err(e) if strict() => return Err(e), Err(_) => return Ok(String::new()) };
    let dir = task_dir(&root, id);
    if !dir.exists() { return Ok(String::new()); }
    let branch = task_branch(id);
    let base = run(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "main".into());
    let _ = run(&dir, &["add", "-A"]);
    if run(&dir, &["diff", "--cached", "--quiet"]).is_err() {
        let _ = run(&dir, &["commit", "-m", &format!("{id}: task changes")]);
    }

    // Serialize merges across concurrent fleet agents. Held for the whole
    // compute-then-fast-forward sequence below so no two merges can ever
    // interleave against the shared main checkout.
    let _lock = match MergeLock::acquire(&root) {
        Ok(l) => l,
        Err(e) => {
            let msg = format!("\nworktree: merge lock unavailable for {branch} — worktree/branch preserved for retry ({e})");
            return if strict() { Err(msg) } else { Ok(msg) };
        }
    };

    // Compute the merge in an isolated throwaway worktree — never in {root}.
    // A CONFLICT there never touches the base branch at all: abort it,
    // preserve the task worktree/branch for a manual merge.
    match merge_isolated(&root, &base, &branch) {
        Ok(MergeOutcome::Merged(sha)) => {
            // {sha} is a descendant of {base}'s current tip (we computed it
            // from {base} under the lock, so nothing else could have moved
            // {base} meanwhile), so this can only ever fast-forward — it
            // cannot conflict and cannot leave residue.
            if let Err(e) = run(&root, &["merge", "--ff-only", &sha]) {
                let msg = format!(
                    "\nworktree: fast-forward of {base} to merged {branch} ({sha}) failed — base left untouched, worktree/branch preserved for manual merge ({e})"
                );
                return if strict() { Err(msg) } else { Ok(msg) };
            }
            let _ = run(&root, &["worktree", "remove", "--force", dir.to_str().unwrap_or("")]);
            let _ = run(&root, &["branch", "-D", &branch]);
            Ok(format!("\nworktree: merged {branch} and removed {}", dir.display()))
        }
        Ok(MergeOutcome::Conflict(e)) => {
            let msg = format!(
                "\nworktree: MERGE CONFLICT for {branch} — aborted to protect {base}; worktree/branch preserved for manual merge ({e})"
            );
            if strict() { Err(msg) } else { Ok(msg) }
        }
        Err(e) => {
            let msg = format!("\nworktree: merge of {branch} failed — worktree/branch preserved for manual merge ({e})");
            if strict() { Err(msg) } else { Ok(msg) }
        }
    }
}

/// Outcome of computing a task branch's merge against `base` in isolation.
enum MergeOutcome {
    /// Merge succeeded; carries the resulting commit SHA (a descendant of
    /// `base`'s tip, safe to fast-forward onto).
    Merged(String),
    /// Merge conflicted; the throwaway worktree was already aborted/removed.
    /// Carries the git error text for diagnostics.
    Conflict(String),
}

/// Compute `git merge --no-ff --no-edit {branch}` against `{base}` in a
/// throwaway worktree detached at `base`'s current tip, never in `root`
/// itself. The throwaway worktree is always removed before returning,
/// success or failure.
fn merge_isolated(root: &Path, base: &str, branch: &str) -> Result<MergeOutcome, String> {
    fs::create_dir_all(root.join("worktrees")).map_err(|e| e.to_string())?;
    let tmp = merge_tmp_dir(root, branch);
    let _ = fs::remove_dir_all(&tmp); // clear any residue from a prior crash
    let tmp_str = tmp.to_str().ok_or("bad merge tmp path")?.to_string();

    let outcome = run(root, &["worktree", "add", "--detach", &tmp_str, base]).and_then(|_| {
        match run(&tmp, &["merge", "--no-ff", "--no-edit", branch]) {
            Ok(_) => run(&tmp, &["rev-parse", "HEAD"]).map(MergeOutcome::Merged),
            Err(e) => {
                // Never leave the throwaway worktree mid-merge either.
                let _ = run(&tmp, &["merge", "--abort"]);
                Ok(MergeOutcome::Conflict(e))
            }
        }
    });

    // Always try to remove the throwaway worktree, whether or not it was
    // fully set up — merge scratch state must never linger.
    let _ = run(root, &["worktree", "remove", "--force", &tmp_str]);
    let _ = fs::remove_dir_all(&tmp);
    outcome
}

fn merge_tmp_dir(root: &Path, branch: &str) -> PathBuf {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let safe = branch.replace('/', "_");
    root.join("worktrees").join(format!(".merge-{safe}-{ts}"))
}

const MERGE_LOCK_STALE_SECS: u64 = 120;
const MERGE_LOCK_WAIT_SECS: u64 = 180;
const MERGE_LOCK_POLL_MS: u64 = 100;

/// O_EXCL lockfile serializing all task merges against one repo root. Held
/// for the lifetime of the guard; released (best-effort) on drop, covering
/// every exit path including early returns via `?`.
struct MergeLock(PathBuf);

impl MergeLock {
    fn acquire(root: &Path) -> Result<Self, String> {
        let path = root.join(".git").join("tasks-merge.lock");
        let deadline = Instant::now() + Duration::from_secs(MERGE_LOCK_WAIT_SECS);
        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    let _ = write!(f, "{}", std::process::id());
                    return Ok(MergeLock(path));
                }
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    if lock_is_stale(&path) {
                        // Prior holder almost certainly died mid-merge; reclaim.
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(format!("held by another process after {MERGE_LOCK_WAIT_SECS}s wait"));
                    }
                    std::thread::sleep(Duration::from_millis(MERGE_LOCK_POLL_MS));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}

impl Drop for MergeLock {
    fn drop(&mut self) { let _ = fs::remove_file(&self.0); }
}

fn lock_is_stale(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|age| age.as_secs() > MERGE_LOCK_STALE_SECS)
        .unwrap_or(false)
}

pub fn current_task_path(id: &str) -> Result<PathBuf, String> {
    Ok(task_dir(&repo_root()?, id))
}

pub fn path_allowed_for_task(id: &str, path: &Path) -> Result<bool, String> {
    let dir = current_task_path(id)?;
    if path.starts_with(&dir) { return Ok(true); }
    let canon_dir = dir.canonicalize().unwrap_or(dir);
    Ok(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).starts_with(canon_dir))
}

pub fn reap_abandoned(max_age_secs: u64) -> Result<String, String> {
    let root = repo_root()?;
    let dir = root.join("worktrees");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let mut reaped = 0usize;
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            let stale = e.metadata().and_then(|m| m.modified()).ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| now.saturating_sub(d.as_secs()) >= max_age_secs)
                .unwrap_or(false);
            if stale {
                let _ = run(&root, &["worktree", "remove", "--force", p.to_str().unwrap_or("")]);
                reaped += 1;
            }
        }
    }
    Ok(format!("reaped {reaped} abandoned worktree(s)"))
}
