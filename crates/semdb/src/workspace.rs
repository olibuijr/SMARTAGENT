//! Workspace project resolution — THE shared mechanism for per-repo scoping.
//!
//! Every direct child directory of `workspaces_dir` (config) is a project.
//! Per-project tool state lives at `<project>/.smartagent/<file>` (the
//! memory-policy convention). Crates add a `--project <name>` flag and resolve
//! it here, so boards/graphs/corpora/memories never mix between repos.

use std::path::{Path, PathBuf};

use crate::config::Config;

/// Dir under each project root holding per-project tool state.
pub const STATE_DIR: &str = ".smartagent";

pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub is_repo: bool,
}

/// Absolute workspaces root from config (`workspaces_dir`).
pub fn root() -> Result<PathBuf, String> {
    let root = Config::load().workspaces_dir();
    if root.is_dir() {
        Ok(root)
    } else {
        Err(format!("workspaces dir not found: {}", root.display()))
    }
}

/// All project candidates under `root`: direct child dirs, sorted, hidden skipped.
pub fn list(root: &Path) -> Vec<Project> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let path = e.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if !path.is_dir() || name.starts_with('.') {
                continue;
            }
            let is_repo = path.join(".git").exists();
            out.push(Project { name, path, is_repo });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Resolve a project name to its directory under `root`. Rejects path
/// traversal so `--project` can never escape the workspaces root.
pub fn resolve_in(root: &Path, name: &str) -> Result<PathBuf, String> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!("invalid project name: {name}"));
    }
    let p = root.join(name);
    if p.is_dir() {
        Ok(p)
    } else {
        Err(format!("no such project under workspaces: {name}"))
    }
}

/// Resolve a project name against the configured workspaces root.
pub fn resolve(name: &str) -> Result<PathBuf, String> {
    resolve_in(&root()?, name)
}

/// Per-project state path: `<project>/.smartagent/<file>`. Creates the
/// `.smartagent` dir so callers can open/create the file directly.
pub fn data_path(project: &str, file: &str) -> Result<PathBuf, String> {
    let dir = resolve(project)?.join(STATE_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch")
            .join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn list_marks_repos_and_resolve_guards_traversal() {
        let ws = scratch("semdb-workspace");
        std::fs::create_dir_all(ws.join("repo/.git")).unwrap();
        std::fs::create_dir_all(ws.join("plain")).unwrap();
        std::fs::create_dir_all(ws.join(".hidden")).unwrap();
        let ps = list(&ws);
        let names: Vec<&str> = ps.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["plain", "repo"]);
        assert!(ps.iter().find(|p| p.name == "repo").unwrap().is_repo);
        assert!(!ps.iter().find(|p| p.name == "plain").unwrap().is_repo);
        assert!(resolve_in(&ws, "repo").is_ok());
        assert!(resolve_in(&ws, "../escape").is_err());
        assert!(resolve_in(&ws, "a/b").is_err());
        assert!(resolve_in(&ws, "missing").is_err());
    }
}
