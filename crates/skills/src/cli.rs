//! CLI: list / show / search / create / patch / edit / delete
use httpc::args::flag;

use std::path::{Path, PathBuf};

use crate::manage;
use crate::registry;

/// `<root>/.smartagent/skills` under a workspace project — the same
/// `.smartagent/<name>` convention `tasks`/`memory`/`rag` use for per-repo
/// state (see `semdb::workspace`).
fn project_root(name: &str) -> Result<PathBuf, String> {
    semdb::workspace::data_path(name, "skills")
}

/// Write target for the mutating verbs: `--project P` writes into that
/// project's own skills dir; otherwise the positional `<root>` (global by
/// convention — the extension defaults it to `./skills`).
fn effective_root(root: &Path, args: &[String]) -> Result<PathBuf, String> {
    match flag(args, "--project") {
        Some(p) => project_root(&p),
        None => Ok(root.to_path_buf()),
    }
}

/// Discovery for the read verbs: GLOBAL `<root>` plus, when `--project P` is
/// given, that project's own skills — a project skill wins on a name
/// collision (Hermes "local wins").
fn discover_scoped(root: &Path, args: &[String]) -> Result<Vec<registry::Skill>, String> {
    match flag(args, "--project") {
        Some(p) => registry::discover_merged(root, Some(&project_root(&p)?)),
        None => registry::discover(root),
    }
}

/// SKILL.md body for create/edit: `--file <path>` if given, else the whole
/// of stdin (empty when the caller supplies none — e.g. the pi extension
/// pipes `content` via stdin and closes it, so this never blocks).
fn read_body(args: &[String]) -> Result<String, String> {
    if let Some(path) = flag(args, "--file") {
        return std::fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"));
    }
    let mut s = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|e| e.to_string())?;
    Ok(s)
}

pub fn run(args: &[String]) -> Result<String, String> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let root = args.get(1).map(Path::new);
    match cmd {
        "list" => {
            let root = root.ok_or("usage: skills list <root>")?;
            let skills = discover_scoped(root, args)?;
            if skills.is_empty() {
                return Ok("no skills found".into());
            }
            Ok(skills
                .iter()
                .map(|s| format!("{}\t{}", s.name, s.description))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "show" => {
            let root = root.ok_or("usage: skills show <root> <name>")?;
            let name = args.get(2).ok_or("name required")?;
            let skills = discover_scoped(root, args)?;
            let s = skills
                .iter()
                .find(|s| &s.name == name)
                .ok_or_else(|| format!("no skill '{name}'"))?;
            let body = registry::load_body(&s.path)?;
            // --head N: first N lines (progressive disclosure of a long SKILL.md).
            match flag(args, "--head").and_then(|s| s.parse::<usize>().ok()) {
                Some(n) => {
                    let lines: Vec<&str> = body.lines().collect();
                    let mut out = lines.iter().take(n).cloned().collect::<Vec<_>>().join("\n");
                    if lines.len() > n {
                        out.push_str(&format!("\n…[{n} of {} lines]", lines.len()));
                    }
                    Ok(out)
                }
                None => Ok(body),
            }
        }
        "search" => {
            let root = root.ok_or("usage: skills search <root> <query>")?;
            let query = args.get(2).ok_or("query required")?.to_lowercase();
            let skills = discover_scoped(root, args)?;
            let mut ranked: Vec<(usize, &registry::Skill)> = skills
                .iter()
                .filter_map(|s| {
                    let hay = format!("{} {}", s.name, s.description).to_lowercase();
                    let hits = hay.matches(&query).count();
                    if hits > 0 {
                        Some((hits, s))
                    } else {
                        None
                    }
                })
                .collect();
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
            if ranked.is_empty() {
                return Ok("no matches".into());
            }
            Ok(ranked
                .iter()
                .map(|(_, s)| format!("{}\t{}", s.name, s.description))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "validate" => {
            // Spec compliance: every SKILL.md needs non-empty name + description.
            let root = root.ok_or("usage: skills validate <root>")?;
            let skills = registry::discover(root)?;
            if skills.is_empty() {
                return Ok("no skills found".into());
            }
            let mut problems = Vec::new();
            let mut names = std::collections::BTreeSet::new();
            for s in &skills {
                if s.name.is_empty() {
                    problems.push(format!("{}: missing name", s.path.display()));
                }
                if s.description.trim().is_empty() {
                    problems.push(format!("{}: missing/empty description", s.path.display()));
                }
                if s.description.chars().count() > 4096 {
                    problems.push(format!("{}: description over 4096 chars", s.name));
                }
                if !names.insert(s.name.clone()) {
                    problems.push(format!("duplicate skill name '{}'", s.name));
                }
            }
            if problems.is_empty() {
                Ok(format!("ok: {} skills valid", skills.len()))
            } else {
                Err(problems.join("\n"))
            }
        }
        "match" => {
            // Auto-trigger: score skills against a whole prompt/task sentence
            // (word-boundary token overlap, name hits weighted 3×) — unlike
            // `search`, which is a single-substring count. Use to pick which
            // skill to load for a step or an incoming task.
            let root = root.ok_or("usage: skills match <root> '<prompt text>'")?;
            let query = args.get(2).ok_or("prompt text required")?;
            let qtokens: Vec<String> = tokens(query);
            if qtokens.is_empty() {
                return Err("no usable words in query".into());
            }
            let skills = discover_scoped(root, args)?;
            let mut ranked: Vec<(usize, &registry::Skill)> = skills
                .iter()
                .filter_map(|s| {
                    if is_ponytail_skill(&s.name) && !has_ponytail_trigger(&qtokens) {
                        return None;
                    }
                    let name_t = tokens(&s.name);
                    let desc_t = tokens(&s.description);
                    let negative_t = do_not_use_tokens(&s.description);
                    let raw: isize = qtokens
                        .iter()
                        .map(|q| {
                            let n = if name_t.contains(q) { 3 } else { 0 };
                            let d = if desc_t.contains(q) { 1 } else { 0 };
                            let no = if negative_t.contains(q) { 3 } else { 0 };
                            n + d - no
                        })
                        .sum();
                    let score = raw.max(0) as usize;
                    if score > 0 {
                        Some((score, s))
                    } else {
                        None
                    }
                })
                .collect();
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
            if ranked.is_empty() {
                return Ok("no matching skill".into());
            }
            Ok(ranked
                .iter()
                .take(5)
                .map(|(score, s)| format!("{score}\t{}\t{}", s.name, s.description))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        // Self-creating skills: an agent that just worked out a non-trivial,
        // reusable procedure saves it here — procedural memory, ported from
        // Hermes Agent's `skill_manage` tool.
        "create" => {
            let root = root.ok_or(
                "usage: skills create <root> --name <kebab> [--category C] [--desc \"...\"] [--project P] (body via --file <path> or stdin)",
            )?;
            let target = effective_root(root, args)?;
            let name = flag(args, "--name").ok_or("--name required")?;
            let body = read_body(args)?;
            manage::create(
                &target,
                &name,
                flag(args, "--category").as_deref(),
                flag(args, "--desc").as_deref(),
                &body,
            )
        }
        "patch" => {
            let root = root.ok_or(
                "usage: skills patch <root> --name X --old '<str>' --new '<str>' [--project P]",
            )?;
            let target = effective_root(root, args)?;
            let name = flag(args, "--name").ok_or("--name required")?;
            let old = flag(args, "--old").ok_or("--old required")?;
            let new = flag(args, "--new").unwrap_or_default();
            manage::patch(&target, &name, &old, &new)
        }
        "edit" => {
            let root = root.ok_or(
                "usage: skills edit <root> --name X [--desc \"...\"] [--project P] (body via --file <path> or stdin)",
            )?;
            let target = effective_root(root, args)?;
            let name = flag(args, "--name").ok_or("--name required")?;
            let body = read_body(args)?;
            manage::edit(&target, &name, &body, flag(args, "--desc").as_deref())
        }
        "delete" => {
            let root = root.ok_or("usage: skills delete <root> --name X [--project P]")?;
            let target = effective_root(root, args)?;
            let name = flag(args, "--name").ok_or("--name required")?;
            manage::delete(&target, &name)
        }
        _ => Ok(HELP.trim().into()),
    }
}

/// Lowercased word tokens, stopwords dropped — shared by `match` scoring.
fn is_ponytail_skill(name: &str) -> bool {
    name == "ponytail"
}

fn has_ponytail_trigger(tokens: &[String]) -> bool {
    tokens.iter().any(|t| {
        matches!(
            t.as_str(),
            "ponytail" | "lazy" | "yagni" | "simplest" | "minimal" | "shortest"
        )
    })
}

fn tokens(text: &str) -> Vec<String> {
    const STOP: [&str; 12] = [
        "the", "a", "an", "to", "of", "and", "or", "for", "in", "on", "with", "use",
    ];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1 && !STOP.contains(w))
        .map(str::to_string)
        .collect()
}

fn do_not_use_tokens(description: &str) -> Vec<String> {
    let lower = description.to_lowercase();
    let Some((_, tail)) = lower.split_once("do not use") else {
        return Vec::new();
    };
    tokens(tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_ponytail_requires_explicit_trigger() {
        let q = tokens("bug-hunt harness diagnostics for tool registration");
        assert!(!has_ponytail_trigger(&q));
        let q = tokens("ponytail audit this codebase for bloat");
        assert!(has_ponytail_trigger(&q));
        assert!(is_ponytail_skill("ponytail"));
        assert!(!is_ponytail_skill("ponytail-audit"));
    }
}

const HELP: &str = r#"
skills — Agent Skills (SKILL.md) loader + self-authoring (procedural memory)

USAGE:
  skills list    <root> [--project P]
  skills show    <root> <name> [--head N] [--project P]
  skills search  <root> <query> [--project P]        substring rank (single term)
  skills validate <root>                              frontmatter compliance check
  skills match   <root> '<prompt>' [--project P]      auto-trigger: score against a whole sentence

  skills create <root> --name <kebab> [--category C] [--desc "..."] [--project P]
                                       body via --file <path> or stdin; rejects a
                                       name collision (use edit/patch instead)
  skills patch  <root> --name X --old '<str>' --new '<str>' [--project P]
                                       exact-string replace, errors if not unique
  skills edit   <root> --name X [--desc "..."] [--project P]
                                       full-body rewrite via --file <path> or stdin
  skills delete <root> --name X [--project P]

<root> is the global skills dir (extension default ./skills). --project P scopes
to workspaces/P/.smartagent/skills: read verbs MERGE global+project (project wins
on a name collision); the write verbs (create/patch/edit/delete) target the
project dir instead of <root> when --project is given, else <root>.
"#;
