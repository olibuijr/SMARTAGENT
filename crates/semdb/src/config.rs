//! Runtime configuration: located in `config/smartagent.conf` at the repo root
//! (walked up from the current directory). No endpoint is ever hardcoded in
//! logic — defaults live in the config file, overridable by flag or env.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Config {
    map: HashMap<String, String>,
}

impl Config {
    /// Load from `config/smartagent.conf`, searching upward from cwd. Missing
    /// file yields an empty config (callers then require a flag/env value).
    pub fn load() -> Config {
        let mut map = HashMap::new();
        if let Some(path) = find() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((k, v)) = line.split_once('=') {
                        map.insert(k.trim().to_string(), v.trim().to_string());
                    }
                }
            }
        }
        Config { map }
    }

    /// Resolution order: explicit override (flag) → env var → config file.
    pub fn resolve(&self, key: &str, env: &str, flag: Option<&str>) -> Option<String> {
        if let Some(f) = flag {
            return Some(f.to_string());
        }
        if let Ok(v) = std::env::var(env) {
            if !v.is_empty() {
                return Some(v);
            }
        }
        self.map.get(key).cloned()
    }

    /// Absolute workspaces root: config `workspaces_dir` resolved against the
    /// repo root (the dir containing config/smartagent.conf). Defaults to
    /// `<repo>/workspaces`. All crates align to this single location.
    pub fn workspaces_dir(&self) -> PathBuf {
        self.repo_relative("workspaces_dir", "workspaces")
    }

    /// Absolute data dir for durable stores; defaults to `<repo>/data`.
    pub fn data_dir(&self) -> PathBuf {
        self.repo_relative("data_dir", "data")
    }

    fn repo_relative(&self, key: &str, default: &str) -> PathBuf {
        let rel = self.map.get(key).map(String::as_str).unwrap_or(default);
        let base = find()
            .and_then(|p| p.parent().and_then(|d| d.parent()).map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(rel)
    }
}

fn find() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join("config").join("smartagent.conf");
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_beats_env_beats_file() {
        let cfg = Config { map: HashMap::from([("k".into(), "fromfile".into())]) };
        assert_eq!(cfg.resolve("k", "NOPE_ENV", Some("flag")).as_deref(), Some("flag"));
        assert_eq!(cfg.resolve("k", "NOPE_ENV", None).as_deref(), Some("fromfile"));
        assert_eq!(cfg.resolve("missing", "NOPE_ENV", None), None);
    }
}
