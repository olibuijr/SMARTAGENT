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
        _ => Ok(HELP.trim().into()),
    }
}

const HELP: &str = r#"
skills — Agent Skills (SKILL.md) loader

USAGE:
  skills list   <root>
  skills show   <root> <name>
  skills search <root> <query>
"#;
