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
            Ok(hits.iter().map(|h| format!("{:.4}\t[{}]\t{}\t{}", h.score, h.tier, h.id, h.text)).collect::<Vec<_>>().join("\n"))
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
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let slug: String = text.chars().take(16).map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    format!("{ts}-{slug}")
}


const HELP: &str = r#"
memory — 3-tier persistent agent memory (Mem0 role)

USAGE:
  memory remember --dir D --tier working|episodic|semantic --text '..' [--id X]
  memory recall   --dir D --text '..' [--k 5] [--tier all|working|episodic|semantic]
  memory forget   --dir D --tier T --id X
  memory promote  --dir D --id X --from T1 --to T2
  memory stats    --dir D
"#;
