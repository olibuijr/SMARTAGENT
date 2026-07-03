//! Git worktree lifecycle for fleet task isolation.
//!
//! Best-effort by default: if cwd is not a git repo, lifecycle is skipped so
//! task boards remain usable in tests/scratch repos. Set
//! `SMARTAGENT_WORKTREE_STRICT=1` to turn lifecycle failures into task errors.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
    let merged = run(&root, &["merge", "--ff-only", &branch]).or_else(|_| run(&root, &["merge", "--no-edit", &branch]));
    if let Err(e) = merged { if strict() { return Err(format!("worktree merge failed from {branch} into {base}: {e}")); } }
    let _ = run(&root, &["worktree", "remove", "--force", dir.to_str().unwrap_or("")]);
    let _ = run(&root, &["branch", "-D", &branch]);
    Ok(format!("\nworktree: merged {branch} and removed {}", dir.display()))
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
