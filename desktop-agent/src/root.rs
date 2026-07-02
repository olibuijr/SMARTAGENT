//! Repo-root autodetection and derived paths (ISC-8). No hardcoded absolute path.

use std::path::{Path, PathBuf};

/// Resolved layout of the SMARTAGENT repo the GUI drives.
pub struct Paths {
    pub root: PathBuf,
    pub pi: PathBuf,
    pub sessions_dir: PathBuf,
    pub workspaces_dir: PathBuf,
    pub tasks_bin: PathBuf,
}

impl Paths {
    /// Walk up from the executable dir and cwd until a dir containing both the
    /// `pi` launcher and a `.pi/` dir is found. Returns None if not located.
    pub fn discover() -> Option<Self> {
        let mut starts: Vec<PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(p) = exe.parent() {
                starts.push(p.to_path_buf());
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            starts.push(cwd);
        }
        for start in starts {
            if let Some(root) = climb(&start) {
                return Some(Self::from_root(root));
            }
        }
        None
    }

    fn from_root(root: PathBuf) -> Self {
        let pi = root.join("pi");
        let sessions_dir = root.join(".pi").join("sessions");
        let workspaces_dir = root.join("workspaces");
        let tasks_bin = root.join("target").join("release").join("tasks");
        Self { root, pi, sessions_dir, workspaces_dir, tasks_bin }
    }
}

fn climb(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join("pi").is_file() && dir.join(".pi").is_dir() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

/// Current username for the greeting (ISC-59). Falls back to "there".
pub fn username() -> String {
    std::env::var("USER")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "there".to_string())
}

/// Seconds since the Unix epoch (for relative-time labels).
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
