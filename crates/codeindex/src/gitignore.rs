//! Minimal .gitignore support: literal names, `*` globs, `dir/` rules,
//! `!` negation. Always-ignored: .git, target, node_modules, .refrepos, .pi,
//! .scratch, workspaces.

use std::path::Path;

pub struct Rules {
    /// (pattern, negated, dir_only)
    patterns: Vec<(String, bool, bool)>,
}

const ALWAYS: &[&str] = &[".git", "target", "node_modules", ".refrepos", ".pi", ".scratch", ".smartagent", "workspaces"];

impl Rules {
    pub fn load(root: &Path) -> Rules {
        let mut patterns = Vec::new();
        if let Ok(text) = std::fs::read_to_string(root.join(".gitignore")) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let negated = line.starts_with('!');
                let pat = line.trim_start_matches('!');
                let dir_only = pat.ends_with('/');
                patterns.push((pat.trim_matches('/').to_string(), negated, dir_only));
            }
        }
        Rules { patterns }
    }

    /// Should this path component / relative name be ignored?
    pub fn ignored(&self, name: &str, is_dir: bool) -> bool {
        if ALWAYS.contains(&name) {
            return true;
        }
        let mut ignored = false;
        for (pat, negated, dir_only) in &self.patterns {
            if *dir_only && !is_dir {
                continue;
            }
            if glob_match(pat, name) {
                ignored = !negated;
            }
        }
        ignored
    }
}

/// Simple glob: '*' matches any run within a single path component.
fn glob_match(pattern: &str, name: &str) -> bool {
    if !pattern.contains('*') {
        return pattern == name;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match name[pos..].find(part) {
            Some(idx) => {
                if i == 0 && idx != 0 {
                    return false;
                }
                pos += idx + part.len();
            }
            None => return false,
        }
    }
    if let Some(last) = parts.last() {
        if !last.is_empty() && !name.ends_with(last) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_ignored() {
        let r = Rules { patterns: vec![] };
        assert!(r.ignored("target", true));
        assert!(r.ignored(".git", true));
        assert!(!r.ignored("src", true));
    }

    #[test]
    fn globs_and_negation() {
        let r = Rules { patterns: vec![
            ("*.log".into(), false, false),
            ("keep.log".into(), true, false),
        ]};
        assert!(r.ignored("debug.log", false));
        assert!(!r.ignored("keep.log", false));
        assert!(!r.ignored("main.rs", false));
    }
}
