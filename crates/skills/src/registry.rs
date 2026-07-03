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
        // One unreadable SKILL.md must not hide every other skill.
        let Ok(content) = std::fs::read_to_string(&p) else {
            continue;
        };
        let fm = frontmatter::parse(&content);
        // Fall back to the directory name when frontmatter lacks a name.
        let name = fm.get("name").map(str::to_string).unwrap_or_else(|| {
            p.parent()
                .and_then(|d| d.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        });
        let description = fm
            .get("description")
            .unwrap_or("(no description)")
            .to_string();
        skills.push(Skill {
            name,
            description,
            path: p,
        });
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

/// Merge global + project discovery: a project skill overrides a global
/// skill of the same `name` ("local wins" — the Hermes per-project
/// accumulation rule). `project_root` not existing yet is not an error — a
/// project's first self-created skill has no dir.
pub fn discover_merged(
    global_root: &Path,
    project_root: Option<&Path>,
) -> Result<Vec<Skill>, String> {
    let mut by_name: std::collections::BTreeMap<String, Skill> = std::collections::BTreeMap::new();
    for s in discover(global_root)? {
        by_name.insert(s.name.clone(), s);
    }
    if let Some(p) = project_root {
        if p.exists() {
            for s in discover(p)? {
                by_name.insert(s.name.clone(), s); // project wins on collision
            }
        }
    }
    let mut out: Vec<Skill> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
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
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            // Skip infra + reference clones: a root of `.` must never surface
            // .refrepos/ SKILL.md files (read-only reference material) or
            // workspace repos' own skills as OUR skills.
            if name.starts_with('.') || matches!(name, "target" | "node_modules" | "workspaces") {
                continue;
            }
            walk(&p, out)?;
        } else if name == "SKILL.md" {
            out.push(p);
        }
    }
    Ok(())
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
    fn discovers_recursively() {
        let root = scratch("skills-disc");
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b/c")).unwrap();
        std::fs::write(
            root.join("a/SKILL.md"),
            "---\nname: alpha\ndescription: first\n---\nbody a",
        )
        .unwrap();
        std::fs::write(
            root.join("b/c/SKILL.md"),
            "---\nname: beta\ndescription: second\n---\nbody b",
        )
        .unwrap();
        // Infra + reference dirs must NEVER surface as our skills — a root of
        // `.` used to leak .refrepos/ reference SKILL.md files into matching.
        for skip in [
            ".refrepos/mem0",
            "target/pkg",
            "node_modules/x",
            "workspaces/proj",
            ".hidden",
        ] {
            std::fs::create_dir_all(root.join(skip)).unwrap();
            std::fs::write(
                root.join(skip).join("SKILL.md"),
                "---\nname: leaked\ndescription: nope\n---\n",
            )
            .unwrap();
        }
        let skills = discover(&root).unwrap();
        assert_eq!(skills.len(), 2);
        assert!(skills.iter().all(|s| s.name != "leaked"));
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[1].description, "second");
    }

    #[test]
    fn discover_merged_project_wins_on_collision() {
        let base = scratch("skills-merged");
        let global = base.join("global");
        let project = base.join("project");
        std::fs::create_dir_all(global.join("shared")).unwrap();
        std::fs::create_dir_all(global.join("global-only")).unwrap();
        std::fs::create_dir_all(project.join("shared")).unwrap();
        std::fs::create_dir_all(project.join("project-only")).unwrap();
        std::fs::write(
            global.join("shared/SKILL.md"),
            "---\nname: shared\ndescription: global version\n---\nglobal body",
        )
        .unwrap();
        std::fs::write(
            global.join("global-only/SKILL.md"),
            "---\nname: global-only\ndescription: only in global\n---\n",
        )
        .unwrap();
        std::fs::write(
            project.join("shared/SKILL.md"),
            "---\nname: shared\ndescription: project version\n---\nproject body",
        )
        .unwrap();
        std::fs::write(
            project.join("project-only/SKILL.md"),
            "---\nname: project-only\ndescription: only in project\n---\n",
        )
        .unwrap();

        // A missing project dir must not error — it just contributes nothing.
        let missing_project = base.join("no-such-project-dir");
        let global_only = discover_merged(&global, Some(&missing_project)).unwrap();
        assert_eq!(global_only.len(), 2);

        let merged = discover_merged(&global, Some(&project)).unwrap();
        let names: Vec<&str> = merged.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["global-only", "project-only", "shared"]);
        let shared = merged.iter().find(|s| s.name == "shared").unwrap();
        assert_eq!(
            shared.description, "project version",
            "project skill must win on a name collision"
        );

        // No project root at all: pure global discovery.
        let global_alone = discover_merged(&global, None).unwrap();
        assert_eq!(global_alone.len(), 2);
    }
}
