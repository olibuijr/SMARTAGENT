//! Workflow definitions — markdown files with frontmatter + `## step` sections,
//! the PAI skill-workflow pattern (skills/<Skill>/Workflows/*.md) as data.
//!
//! Format:
//! ```md
//! ---
//! name: task-run
//! description: one-liner
//! use_when: trigger hints
//! ---
//! # intro prose (ignored by the engine)
//! ## observe
//! skill: memory
//! expect: what the step should produce
//! Free-form step instructions…
//! ```
//! Discovery scans `workflows/*.md` and `skills/*/Workflows/*.md` under the
//! root, so workflows can live beside the skill that owns them (PAI layout).

use std::path::{Path, PathBuf};

#[derive(Default, Clone)]
pub struct Step {
    pub name: String,
    /// Which skill/tool the agent should load for this step (PAI routing).
    pub skill: String,
    pub expect: String,
    pub body: String,
}

#[derive(Default, Clone)]
pub struct Def {
    pub name: String,
    pub description: String,
    pub use_when: String,
    pub path: PathBuf,
    pub steps: Vec<Step>,
}

pub fn parse(path: &Path, text: &str) -> Def {
    let mut def = Def { path: path.to_path_buf(), ..Def::default() };
    let mut body = text;
    // Tiny frontmatter reader (flat key: value lines only).
    if let Some(rest) = text.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            for line in rest[..end].lines() {
                if let Some((k, v)) = line.split_once(':') {
                    let v = v.trim().trim_matches('"').to_string();
                    match k.trim() {
                        "name" => def.name = v,
                        "description" => def.description = v,
                        "use_when" => def.use_when = v,
                        _ => {}
                    }
                }
            }
            body = &rest[end + 4..];
        }
    }
    if def.name.is_empty() {
        def.name = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    }
    // Steps: `## name`, then leading `skill:`/`expect:` directives, then prose.
    let mut current: Option<Step> = None;
    for line in body.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            if let Some(s) = current.take() {
                def.steps.push(s);
            }
            current = Some(Step { name: h.trim().to_string(), ..Step::default() });
            continue;
        }
        let Some(step) = current.as_mut() else { continue };
        if step.body.trim().is_empty() {
            if let Some(v) = line.strip_prefix("skill:") {
                step.skill = v.trim().to_string();
                continue;
            }
            if let Some(v) = line.strip_prefix("expect:") {
                step.expect = v.trim().to_string();
                continue;
            }
        }
        step.body.push_str(line);
        step.body.push('\n');
    }
    if let Some(s) = current.take() {
        def.steps.push(s);
    }
    for s in &mut def.steps {
        s.body = s.body.trim().to_string();
    }
    def
}

pub fn discover(root: &Path) -> Result<Vec<Def>, String> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_md(&root.join("workflows"), &mut files);
    if let Ok(skills) = std::fs::read_dir(root.join("skills")) {
        for e in skills.flatten() {
            collect_md(&e.path().join("Workflows"), &mut files);
        }
    }
    files.sort();
    let mut defs = Vec::new();
    for f in files {
        // One unreadable file must not hide the rest.
        if let Ok(text) = std::fs::read_to_string(&f) {
            let d = parse(&f, &text);
            if !d.steps.is_empty() {
                defs.push(d);
            }
        }
    }
    Ok(defs)
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "md").unwrap_or(false) {
                out.push(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_directives_and_steps() {
        let text = "---\nname: demo\ndescription: d\nuse_when: testing\n---\n# intro\n\n## observe\nskill: memory\nexpect: a summary\nRead everything.\nThen think.\n\n## act\nskill: tasks\nDo the thing.\n";
        let d = parse(Path::new("demo.md"), text);
        assert_eq!(d.name, "demo");
        assert_eq!(d.use_when, "testing");
        assert_eq!(d.steps.len(), 2);
        assert_eq!(d.steps[0].skill, "memory");
        assert_eq!(d.steps[0].expect, "a summary");
        assert!(d.steps[0].body.contains("Then think."));
        assert_eq!(d.steps[1].name, "act");
        assert_eq!(d.steps[1].skill, "tasks");
    }

    #[test]
    fn name_falls_back_to_filename() {
        let d = parse(Path::new("My-Flow.md"), "## only\ndo it\n");
        assert_eq!(d.name, "my-flow");
        assert_eq!(d.steps.len(), 1);
    }
}
