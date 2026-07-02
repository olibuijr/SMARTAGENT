//! Discover SKILL.md files under a root and index their frontmatter.

use std::path::{Path, PathBuf};

use crate::frontmatter::{self, Frontmatter};

pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

pub fn discover(root: &Path) -> Result<Vec<Skill>, String> {
    let mut paths = Vec::new();
    walk(root, &mut paths)?;
    let mut skills = Vec::new();
    for p in paths {
        let content = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let fm = frontmatter::parse(&content);
        // Fall back to the directory name when frontmatter lacks a name.
        let name = fm.get("name").map(str::to_string).unwrap_or_else(|| {
            p.parent()
                .and_then(|d| d.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        });
        let description = fm.get("description").unwrap_or("(no description)").to_string();
        skills.push(Skill { name, description, path: p });
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

pub fn load_body(path: &Path) -> Result<String, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(parse_body(&content))
}

pub fn parse_body(content: &str) -> String {
    let fm: Frontmatter = frontmatter::parse(content);
    fm.body
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => return Err(format!("read {}: {e}", dir.display())),
    };
    for e in entries {
        let e = e.map_err(|x| x.to_string())?;
        let p = e.path();
        if p.is_dir() {
            walk(&p, out)?;
        } else if p.file_name().map(|n| n == "SKILL.md").unwrap_or(false) {
            out.push(p);
        }
    }
    Ok(())
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
    fn discovers_recursively() {
        let root = scratch("skills-disc");
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b/c")).unwrap();
        std::fs::write(root.join("a/SKILL.md"), "---\nname: alpha\ndescription: first\n---\nbody a").unwrap();
        std::fs::write(root.join("b/c/SKILL.md"), "---\nname: beta\ndescription: second\n---\nbody b").unwrap();
        let skills = discover(&root).unwrap();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[1].description, "second");
    }
}
