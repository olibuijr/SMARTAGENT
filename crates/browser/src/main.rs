//! browser CLI: open / probe

use std::process::ExitCode;
use browser::cdp::Cdp;
use httpc::args::{flag, has};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => { println!("{out}"); ExitCode::SUCCESS }
        Err(e) => { eprintln!("error: {e}"); ExitCode::FAILURE }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    let base = devtools_base(args);
    // Snapshot size caps (fewer tokens); `--quiet` on click/type returns only
    // the status line (no page dump) — right for intermediate steps of a flow.
    let mt = flag(args, "--max-text").and_then(|s| s.parse().ok()).unwrap_or(4000usize);
    let ml = flag(args, "--max-links").and_then(|s| s.parse().ok()).unwrap_or(40usize);
    let quiet = has(args, "--quiet");
    match args.first().map(String::as_str) {
        Some("open") => {
            let url = args.get(1).ok_or("usage: browser open <url> [--devtools http://127.0.0.1:9222]")?;
            let mut cdp = Cdp::connect(&base)?;
            cdp.navigate(url)?;
            cdp.snapshot_capped(mt, ml)
        }
        Some("click") => {
            let sel = args.get(1).ok_or("usage: browser click <css-selector>")?;
            let mut cdp = Cdp::connect(&base)?;
            let status = cdp.click(sel)?;
            if quiet { Ok(format!("[{status}]")) } else { Ok(format!("[{status}]\n{}", cdp.snapshot_capped(mt, ml)?)) }
        }
        Some("type") => {
            let sel = args.get(1).ok_or("usage: browser type <css-selector> <text> [--enter]")?;
            let text = args.get(2).ok_or("usage: browser type <css-selector> <text> [--enter]")?;
            let mut cdp = Cdp::connect(&base)?;
            let mut status = cdp.type_text(sel, text)?;
            if has(args, "--enter") {
                status = format!("{status}+{}", cdp.press_enter(sel)?);
            }
            if quiet { Ok(format!("[{status}]")) } else { Ok(format!("[{status}]\n{}", cdp.snapshot_capped(mt, ml)?)) }
        }
        Some("wait") => {
            let sel = args.get(1).ok_or("usage: browser wait <css-selector> [--timeout-ms 10000]")?;
            let ms = flag(args, "--timeout-ms").and_then(|s| s.parse().ok()).unwrap_or(10000u64);
            let mut cdp = Cdp::connect(&base)?;
            let status = cdp.wait_for(sel, ms)?;
            if quiet { Ok(format!("[{status}]")) } else { Ok(format!("[{status}]\n{}", cdp.snapshot_capped(mt, ml)?)) }
        }
        Some("scroll") => {
            let sel = flag(args, "--to").unwrap_or_default();
            let by = flag(args, "--by").and_then(|s| s.parse().ok()).unwrap_or(0i64);
            let mut cdp = Cdp::connect(&base)?;
            cdp.scroll(&sel, by)?;
            cdp.snapshot_capped(mt, ml)
        }
        Some("attr") => {
            let sel = args.get(1).ok_or("usage: browser attr <css-selector> <name|text|value>")?;
            let name = args.get(2).ok_or("usage: browser attr <css-selector> <name|text|value>")?;
            let mut cdp = Cdp::connect(&base)?;
            Ok(cdp.attr(sel, name)?)
        }
        Some("back") => {
            let mut cdp = Cdp::connect(&base)?;
            cdp.history(-1)?;
            cdp.snapshot_capped(mt, ml)
        }
        Some("statusline") => {
            // `level|text` for UI statuslines: CDP endpoint reachability.
            let v = httpc::request("GET", &format!("{}/json/version", base.trim_end_matches('/'))).timeout(4).send();
            Ok(match v {
                Ok(r) if r.ok() => "ok|🌐 chrome✓".to_string(),
                Ok(r) => format!("err|🌐 chrome HTTP {}", r.status),
                Err(_) => "err|🌐 chrome down".to_string(),
            })
        }
        Some("probe") => {
            let v = httpc::get(&format!("{}/json/version", base.trim_end_matches('/')))
                .map_err(|e| format!("devtools unreachable at {base}: {e}"))?;
            // Terse: one line, not the whole /json/version body.
            if v.ok() { Ok(format!("Chrome DevTools OK at {base}")) }
            else { Err(format!("devtools returned HTTP {}", v.status)) }
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn devtools_base(args: &[String]) -> String {
    args.iter().position(|a| a == "--devtools").and_then(|i| args.get(i + 1).cloned())
        .or_else(|| std::env::var("BROWSER_DEVTOOLS").ok())
        .unwrap_or_else(|| {
            semdb::config::Config::load().resolve("browser_devtools", "BROWSER_DEVTOOLS", None)
                .unwrap_or_else(|| "http://127.0.0.1:9222".into())
        })
}

const HELP: &str = r#"
browser — Browser Use port (pure-Rust CDP client)

USAGE:
  browser open  <url>                         [--devtools http://127.0.0.1:9222]
  browser click <css-selector>                [--quiet] [--max-text N] [--max-links N]
  browser type  <css-selector> <text>         [--enter] [--quiet] [--max-text N] [--max-links N]
  browser wait  <css-selector>                [--timeout-ms N] [--quiet]
  browser scroll [--by PX] [--to <selector>]  [--max-text N] [--max-links N]
  browser attr  <css-selector> <name|text|value>
  browser back                                [--max-text N] [--max-links N]
  browser probe
  browser statusline

Requires Chrome/Chromium launched with --remote-debugging-port=9222.
Navigation actions return a compact snapshot (title, visible text, links);
--quiet returns only the status line for intermediate click/type/wait steps.
"#;
