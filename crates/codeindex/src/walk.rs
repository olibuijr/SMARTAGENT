//! Recursive directory walk respecting .gitignore rules and a file-type filter.

use std::path::{Path, PathBuf};

use crate::gitignore::Rules;

pub fn walk(root: &Path, rules: &Rules, ext_filter: Option<&str>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_inner(root, rules, ext_filter, &mut out);
    out.sort();
    out
}

fn walk_inner(dir: &Path, rules: &Rules, ext: Option<&str>, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for e in entries.flatten() {
        let path = e.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let is_dir = path.is_dir();
        if rules.ignored(&name, is_dir) {
            continue;
        }
        if is_dir {
            walk_inner(&path, rules, ext, out);
        } else if ext.map(|x| path.extension().map(|e| e == x).unwrap_or(false)).unwrap_or(true) {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn respects_gitignore_and_ext() {
        let root = scratch("ci-walk");
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("a.rs"), "fn a() {}").unwrap();
        std::fs::write(root.join("b.md"), "# b").unwrap();
        std::fs::write(root.join("target/skip.rs"), "ignored").unwrap();
        std::fs::write(root.join(".gitignore"), "*.md\n").unwrap();
        let rules = Rules::load(&root);
        let files = walk(&root, &rules, None);
        let names: Vec<String> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert!(names.contains(&"a.rs".to_string()));
        assert!(!names.iter().any(|n| n == "b.md")); // gitignored
        assert!(!names.iter().any(|n| n == "skip.rs")); // in target/
    }
}
