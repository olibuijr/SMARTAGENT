//! CLI: list / dispatch / test / audit / statusline.

use std::io::Read;
use std::path::PathBuf;

use httpc::args::flag;
use semdb::storage::Db;

use crate::config;
use crate::dispatch;

fn repo(args: &[String]) -> PathBuf {
    flag(args, "--root").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn conf_path(args: &[String]) -> PathBuf {
    flag(args, "--config").map(PathBuf::from).unwrap_or_else(|| repo(args).join("config/hooks.conf"))
}

pub fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("list") => {
            let hooks = config::load(&conf_path(args))?;
            if hooks.is_empty() {
                return Ok("no hooks configured (config/hooks.conf)".into());
            }
            Ok(hooks
                .iter()
                .map(|h| format!("{}\t{}\t{}\t{} ({}s)", h.name, h.event, h.matcher, h.command, h.timeout_secs))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("dispatch") => {
            // Payload JSON on stdin; prints one JSON decision on stdout.
            let event = flag(args, "--event").ok_or("--event required")?;
            let subject = flag(args, "--subject").unwrap_or_default();
            let mut payload = String::new();
            std::io::stdin().read_to_string(&mut payload).map_err(|e| e.to_string())?;
            let hooks = config::load(&conf_path(args))?;
            let d = dispatch::dispatch(&hooks, &repo(args), &event, &subject, &payload);
            Ok(d.to_json())
        }
        Some("test") => {
            // Dry-run one named hook with a synthetic payload — verify before trusting.
            let name = args.get(1).ok_or("usage: hooks test <name> [--payload '<json>']")?;
            let hooks = config::load(&conf_path(args))?;
            let h = hooks.iter().find(|h| &h.name == name).ok_or_else(|| format!("no hook '{name}'"))?;
            let payload = flag(args, "--payload").unwrap_or_else(|| r#"{"test":true}"#.into());
            let d = dispatch::dispatch(std::slice::from_ref(h), &repo(args), &h.event, &h.matcher.replace('*', "test"), &payload);
            Ok(d.to_json())
        }
        Some("audit") => {
            let path = dispatch::audit_path(&repo(args));
            if !path.exists() {
                return Ok("no hook firings recorded".into());
            }
            let db = Db::open(&path)?;
            let mut rows: Vec<(&String, &str)> = db.index.iter().map(|(k, e)| (k, e.meta.as_str())).collect();
            rows.sort();
            let n = flag(args, "--n").and_then(|s| s.parse().ok()).unwrap_or(20usize);
            Ok(rows.iter().rev().take(n).map(|(_, m)| m.to_string()).collect::<Vec<_>>().join("\n"))
        }
        Some("statusline") => {
            let hooks = config::load(&conf_path(args))?;
            if hooks.is_empty() {
                return Ok("ok|🪝 none".into());
            }
            // err if any configured hook command is missing on disk.
            let root = repo(args);
            let missing: Vec<&str> = hooks.iter().filter(|h| !root.join(&h.command).exists()).map(|h| h.name.as_str()).collect();
            if missing.is_empty() {
                Ok(format!("ok|🪝 {} armed", hooks.len()))
            } else {
                Ok(format!("err|🪝 missing: {}", missing.join(",")))
            }
        }
        _ => Ok(HELP.trim().into()),
    }
}

const HELP: &str = r#"
hooks — user-configurable lifecycle hooks (Claude Code contract, pure Rust)

USAGE:
  hooks list                              parsed config/hooks.conf
  hooks dispatch --event E [--subject S]  payload JSON on stdin → decision JSON
  hooks test <name> [--payload '<json>']  dry-run one hook
  hooks audit [--n 20]                    recent firings (data/hooks.semdb)
  hooks statusline

Events: tool_call | tool_result | user_prompt | session_start | stop
Hook contract: payload JSON on stdin; exit 0 = allow (stdout JSON may carry
{"decision":"block"}, {"updatedInput":…}, {"context":"…"} or plain-text
context), exit 2 = BLOCK (stderr = reason), other = warning. First block wins.
Timeout fails OPEN with a warning. Config blocks are additive.
"#;
