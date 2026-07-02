//! sa-browser CLI: visual browser for the pi TUI. DOM snapshots + CDP
//! screenshots rendered as half-block truecolor art. Composes the `browser`
//! crate's CDP client — the migration seam for eventually absorbing the
//! legacy `browser` tool (port verbs here, retire it, rename sa-browser).

mod art;
mod inflate;
mod png;

use std::process::ExitCode;

use browser::cdp::Cdp;
use httpc::args::flag;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    let base = devtools_base(args);
    let mt = flag(args, "--max-text").and_then(|s| s.parse().ok()).unwrap_or(4000usize);
    let ml = flag(args, "--max-links").and_then(|s| s.parse().ok()).unwrap_or(40usize);
    match args.first().map(String::as_str) {
        Some("open") => {
            let url = args.get(1).ok_or("usage: sa-browser open <url>")?;
            let mut cdp = Cdp::connect(&base)?;
            cdp.navigate(url)?;
            cdp.snapshot_capped(mt, ml)
        }
        Some("snapshot") => {
            let mut cdp = Cdp::connect(&base)?;
            cdp.snapshot_capped(mt, ml)
        }
        Some("status") => {
            let mut cdp = Cdp::connect(&base)?;
            cdp.page_status()
        }
        Some("pane") => {
            // Line 1: url \t title \t readyState (address bar + loading status),
            // then the art lines. One process call per repaint.
            let cols = flag(args, "--cols").and_then(|s| s.parse().ok()).unwrap_or(80usize);
            let rows = flag(args, "--rows").and_then(|s| s.parse().ok()).unwrap_or(24usize);
            let mut cdp = Cdp::connect(&base)?;
            if let Some(url) = flag(args, "--url") {
                cdp.navigate(&url)?;
            }
            let status = cdp.page_status()?;
            let png_bytes = cdp.screenshot_png()?;
            let img = png::decode(&png_bytes)?;
            let (lines, _, _) = art::render(&img, cols, rows);
            Ok(format!("{status}\n{}", lines.join("\n")))
        }
        Some("art") => {
            // Render a PNG file (debug/testing path — no Chrome needed).
            let path = args.get(1).ok_or("usage: sa-browser art <file.png> [--cols N --rows N]")?;
            let cols = flag(args, "--cols").and_then(|s| s.parse().ok()).unwrap_or(80usize);
            let rows = flag(args, "--rows").and_then(|s| s.parse().ok()).unwrap_or(24usize);
            let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
            let img = png::decode(&bytes)?;
            let (lines, w, h) = art::render(&img, cols, rows);
            Ok(format!("{}x{} cells ({}x{} px)\n{}", w, h, img.width, img.height, lines.join("\n")))
        }
        Some("probe") => {
            let v = httpc::get(&format!("{}/json/version", base.trim_end_matches('/')))
                .map_err(|e| format!("devtools unreachable at {base}: {e}"))?;
            if v.ok() {
                Ok(format!("Chrome DevTools OK at {base}"))
            } else {
                Err(format!("devtools returned HTTP {}", v.status))
            }
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn devtools_base(args: &[String]) -> String {
    args.iter()
        .position(|a| a == "--devtools")
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| std::env::var("BROWSER_DEVTOOLS").ok())
        .unwrap_or_else(|| {
            semdb::config::Config::load()
                .resolve("browser_devtools", "BROWSER_DEVTOOLS", None)
                .unwrap_or_else(|| "http://127.0.0.1:9222".into())
        })
}

const HELP: &str = r#"
sa-browser — visual browser: DOM snapshots + pages as high-DPI terminal art

USAGE:
  sa-browser open <url>       navigate, return compact DOM snapshot
  sa-browser snapshot         DOM snapshot of the current page (no navigation)
  sa-browser status           url \t title \t readyState (address-bar line)
  sa-browser pane [--url u] [--cols N --rows N]
                              status line + half-block truecolor page render
  sa-browser art <file.png>   render a PNG file (debug, no Chrome)
  sa-browser probe            check the DevTools connection

Requires Chrome/Chromium with --remote-debugging-port (config: browser_devtools).
"#;
