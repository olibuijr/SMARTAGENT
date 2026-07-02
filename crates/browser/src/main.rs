//! browser CLI: open / probe

use std::process::ExitCode;
use browser::cdp::Cdp;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => { println!("{out}"); ExitCode::SUCCESS }
        Err(e) => { eprintln!("error: {e}"); ExitCode::FAILURE }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    let base = devtools_base(args);
    match args.first().map(String::as_str) {
        Some("open") => {
            let url = args.get(1).ok_or("usage: browser open <url> [--devtools http://127.0.0.1:9222]")?;
            let mut cdp = Cdp::connect(&base)?;
            cdp.navigate(url)?;
            cdp.snapshot()
        }
        Some("probe") => {
            let v = httpc::get(&format!("{}/json/version", base.trim_end_matches('/')))
                .map_err(|e| format!("devtools unreachable at {base}: {e}"))?;
            if v.ok() { Ok(format!("Chrome DevTools OK at {base}\n{}", v.text().unwrap_or_default())) }
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
  browser open  <url> [--devtools http://127.0.0.1:9222]
  browser probe [--devtools http://127.0.0.1:9222]

Requires Chrome/Chromium launched with --remote-debugging-port=9222.
Returns a compact snapshot (title, visible text, links) for the agent.
"#;
