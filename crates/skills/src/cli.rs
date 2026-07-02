//! CLI: list / show / search
use httpc::args::flag;

use std::path::Path;

use crate::registry;

pub fn run(args: &[String]) -> Result<String, String> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let root = args.get(1).map(Path::new);
    match cmd {
        "list" => {
            let root = root.ok_or("usage: skills list <root>")?;
            let skills = registry::discover(root)?;
            if skills.is_empty() {
                return Ok("no skills found".into());
            }
            Ok(skills.iter().map(|s| format!("{}\t{}", s.name, s.description)).collect::<Vec<_>>().join("\n"))
        }
        "show" => {
            let root = root.ok_or("usage: skills show <root> <name>")?;
            let name = args.get(2).ok_or("name required")?;
            let skills = registry::discover(root)?;
            let s = skills.iter().find(|s| &s.name == name).ok_or_else(|| format!("no skill '{name}'"))?;
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
            let skills = registry::discover(root)?;
            let mut ranked: Vec<(usize, &registry::Skill)> = skills
                .iter()
                .filter_map(|s| {
                    let hay = format!("{} {}", s.name, s.description).to_lowercase();
                    let hits = hay.matches(&query).count();
                    if hits > 0 { Some((hits, s)) } else { None }
                })
                .collect();
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
            if ranked.is_empty() {
                return Ok("no matches".into());
            }
            Ok(ranked.iter().map(|(_, s)| format!("{}\t{}", s.name, s.description)).collect::<Vec<_>>().join("\n"))
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
            let skills = registry::discover(root)?;
            let mut ranked: Vec<(usize, &registry::Skill)> = skills
                .iter()
                .filter_map(|s| {
                    let name_t = tokens(&s.name);
                    let desc_t = tokens(&s.description);
                    let score: usize = qtokens
                        .iter()
                        .map(|q| {
                            let n = if name_t.contains(q) { 3 } else { 0 };
                            let d = if desc_t.contains(q) { 1 } else { 0 };
                            n + d
                        })
                        .sum();
                    if score > 0 { Some((score, s)) } else { None }
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
        _ => Ok(HELP.trim().into()),
    }
}

/// Lowercased word tokens, stopwords dropped — shared by `match` scoring.
fn tokens(text: &str) -> Vec<String> {
    const STOP: [&str; 12] = ["the", "a", "an", "to", "of", "and", "or", "for", "in", "on", "with", "use"];
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 1 && !STOP.contains(w))
        .map(str::to_string)
        .collect()
}

const HELP: &str = r#"
skills — Agent Skills (SKILL.md) loader

USAGE:
  skills list   <root>
  skills show   <root> <name>
  skills search <root> <query>      substring rank (single term)
  skills match  <root> '<prompt>'   auto-trigger: score skills against a whole sentence
"#;
