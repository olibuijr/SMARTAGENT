//! CLI: remember / recall / forget / promote / stats

use httpc::args::flag;
use std::path::PathBuf;

use crate::tiers::{resolve_tiers, Memory};

fn dir(args: &[String]) -> Result<PathBuf, String> {
    // --project <name> = that workspace repo's own memory at
    // <repo>/.smartagent/memory — per the project-facts memory policy.
    if let Some(p) = flag(args, "--project") {
        return semdb::workspace::data_path(&p, "memory");
    }
    flag(args, "--dir").map(PathBuf::from).ok_or_else(|| "--dir or --project required".into())
}

pub fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("remember") => {
            let m = Memory::new(&dir(args)?);
            let tier = flag(args, "--tier").ok_or("--tier required")?;
            let text = flag(args, "--text").ok_or("--text required")?;
            let id = flag(args, "--id").unwrap_or_else(|| default_id(&text));
            m.remember(&tier, &id, &text)?;
            Ok(format!("remembered {tier}/{id}"))
        }
        Some("recall") => {
            let m = Memory::new(&dir(args)?);
            let text = flag(args, "--text").ok_or("--text required")?;
            let k = flag(args, "--k").and_then(|s| s.parse().ok()).unwrap_or(5);
            let tiers = resolve_tiers(&flag(args, "--tier").unwrap_or_else(|| "all".into()))?;
            let hits = m.recall_text(&text, k, &tiers)?;
            if hits.is_empty() {
                return Ok("no memories".into());
            }
            // Per-row char cap (rows can embed board dumps/heartbeats): 500
            // default, --chars N overrides, 0 = uncapped.
            let cap = flag(args, "--chars").and_then(|s| s.parse::<usize>().ok()).unwrap_or(500);
            let trim = |t: &str| {
                if cap > 0 && t.chars().count() > cap {
                    format!("{}…", t.chars().take(cap).collect::<String>())
                } else {
                    t.to_string()
                }
            };
            Ok(hits.iter().map(|h| format!("{:.4}\t[{}]\t{}\t{}", h.score, h.tier, h.id, trim(&h.text))).collect::<Vec<_>>().join("\n"))
        }
        Some("forget") => {
            let m = Memory::new(&dir(args)?);
            let tier = flag(args, "--tier").ok_or("--tier required")?;
            let id = flag(args, "--id").ok_or("--id required")?;
            if m.forget(&tier, &id)? { Ok(format!("forgot {tier}/{id}")) } else { Err(format!("id '{id}' not found in {tier}")) }
        }
        Some("promote") => {
            let m = Memory::new(&dir(args)?);
            let id = flag(args, "--id").ok_or("--id required")?;
            let from = flag(args, "--from").ok_or("--from required")?;
            let to = flag(args, "--to").ok_or("--to required")?;
            m.promote(&id, &from, &to)?;
            Ok(format!("promoted {id}: {from} → {to}"))
        }
        Some("recent") => {
            let m = Memory::new(&dir(args)?);
            let tier = flag(args, "--tier").unwrap_or_else(|| "episodic".into());
            let n = flag(args, "--n").and_then(|s| s.parse().ok()).unwrap_or(5);
            let rows = m.recent(&tier, n)?;
            if rows.is_empty() {
                return Ok("no memories".into());
            }
            Ok(rows.iter().map(|(_, t)| format!("- {t}")).collect::<Vec<_>>().join("\n"))
        }
        Some("medvitund") => medvitund(args),
        Some("stats") => {
            let m = Memory::new(&dir(args)?);
            Ok(m.stats()?.iter().map(|(t, n)| format!("{t}\t{n}")).collect::<Vec<_>>().join("\n"))
        }
        Some("statusline") => {
            // `level|text` for UI statuslines: tier fill; warn near working-cap (50).
            let m = Memory::new(&dir(args)?);
            let stats = m.stats()?;
            let get = |t: &str| stats.iter().find(|(n, _)| n == t).map(|(_, c)| *c).unwrap_or(0);
            let (w, e, s) = (get("working"), get("episodic"), get("semantic"));
            let level = if w >= 45 { "warn" } else { "ok" };
            Ok(format!("{level}|🧠 w:{w} e:{e} s:{s}"))
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn default_id(text: &str) -> String {
    // Timestamp-prefixed so working-tier eviction drops oldest first.
    let ts = procutil::unix_secs();
    let slug: String = text.chars().take(16).map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    format!("{ts}-{slug}")
}


const HELP: &str = r#"
memory — 3-tier persistent agent memory (Mem0 role)

USAGE:
  memory remember (--dir D|--project P) --tier working|episodic|semantic --text '..' [--id X]
  memory recall   (--dir D|--project P) --text '..' [--k 5] [--tier all|working|episodic|semantic] [--chars N]
  memory recent   (--dir D|--project P) [--tier working|episodic|semantic] [--n 5]
  memory forget   (--dir D|--project P) --tier T --id X
  memory promote  (--dir D|--project P) --id X --from T1 --to T2
  memory medvitund [--since YYYY-MM-DD|unix] [--agent A] [--query TEXT] [--n 10]
  memory stats    (--dir D|--project P)
  memory statusline (--dir D|--project P)

--project P stores memory under workspaces/<P>/.smartagent/memory; use it for repo facts.
"#;

fn medvitund(args: &[String]) -> Result<String, String> {
    let path = semdb::config::Config::load().data_dir().join("medvitund.semdb");
    if !path.exists() {
        return Ok("no medvitund rows".into());
    }
    let db = semdb::storage::Db::open(&path)?;
    let since = flag(args, "--since");
    let since_num = since.as_deref().and_then(parse_since);
    let agent = flag(args, "--agent");
    let query = flag(args, "--query").map(|s| s.to_lowercase());
    let n = flag(args, "--n").and_then(|s| s.parse().ok()).unwrap_or(10);
    let mut rows = Vec::new();
    for (id, entry) in db.index.iter() {
        let Ok(v) = semdb::json::parse(&entry.meta) else { continue; };
        let ts = v.get("ts").and_then(|x| x.as_f64()).unwrap_or(0.0) as u128;
        if let Some(min) = since_num {
            if ts < min { continue; }
        } else if let Some(day) = since.as_deref() {
            if v.get("day").and_then(|x| x.as_str()) != Some(day) { continue; }
        }
        let a = v.get("agent").and_then(|x| x.as_str()).unwrap_or("");
        if agent.as_deref().is_some_and(|want| want != a) { continue; }
        let kind = v.get("kind").and_then(|x| x.as_str()).unwrap_or("");
        let state = v.get("state").and_then(|x| x.as_str()).unwrap_or("");
        let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("");
        if let Some(q) = &query {
            let hay = format!("{a} {kind} {state} {text}").to_lowercase();
            if !hay.contains(q) { continue; }
        }
        rows.push((ts, id.clone(), a.to_string(), kind.to_string(), state.to_string(), text.to_string()));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    if rows.is_empty() { return Ok("no medvitund rows".into()); }
    Ok(rows.into_iter().take(n).map(|(ts, _, a, k, s, t)| {
        format!("{ts}\t{a}\t{k}\t{s}\t{}", one_line(&t, 500))
    }).collect::<Vec<_>>().join("\n"))
}

fn parse_since(s: &str) -> Option<u128> {
    if let Ok(n) = s.parse::<u128>() {
        return Some(if n < 10_000_000_000 { n * 1_000_000_000 } else { n });
    }
    None
}

fn one_line(s: &str, cap: usize) -> String {
    let flat = s.replace('\n', " ");
    if flat.chars().count() > cap {
        format!("{}…", flat.chars().take(cap).collect::<String>())
    } else {
        flat
    }
}
