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
    let base = browser::devtools_base(args);
    let mt = flag(args, "--max-text").and_then(|s| s.parse().ok()).unwrap_or(4000usize);
    let ml = flag(args, "--max-links").and_then(|s| s.parse().ok()).unwrap_or(40usize);
    match args.first().map(String::as_str) {
        Some("open") => {
            let url = normalize_url(args.get(1).ok_or("usage: sa-browser open <url>")?);
            let mut cdp = Cdp::connect(&base)?;
            cdp.navigate(&url)?;
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
            let cols = flag(args, "--cols").and_then(|s| s.parse().ok()).unwrap_or(80usize).max(10);
            let rows = flag(args, "--rows").and_then(|s| s.parse().ok()).unwrap_or(24usize).max(4);
            let device = flag(args, "--device").unwrap_or_else(|| "tablet".into());
            let mode = art::Mode::parse(&flag(args, "--pixels").unwrap_or_else(|| "sextant".into()));
            let mut cdp = Cdp::connect(&base)?;
            // Default: tablet emulation (768 CSS px wide → responsive tablet
            // layout, dsf 2 for crisp minification) with the emulated height
            // derived from the pane's cell grid (1 cell = 1×2 px) so the
            // screenshot aspect matches the pane exactly and the art fills it
            // to the bottom row.
            if device != "none" {
                let width = 768u32;
                let height = ((width as usize * rows * 2 / cols) as u32).clamp(320, 4096);
                cdp.emulate_device(width, height, 2.0, true)?;
            } else {
                cdp.clear_device_emulation()?;
            }
            // Viewport changes apply on the next frame; let Chrome relayout
            // before capturing or the screenshot shows the old metrics.
            std::thread::sleep(std::time::Duration::from_millis(200));
            if let Some(url) = flag(args, "--url") {
                cdp.navigate(&normalize_url(&url))?;
            }
            let status = cdp.page_status()?;
            let png_bytes = cdp.screenshot_png()?;
            let img = png::decode(&png_bytes)?;
            let (lines, _, _) = art::render(&img, cols, rows, mode);
            Ok(format!("{status}\n{}", lines.join("\n")))
        }
        Some("art") => {
            // Render a PNG file (debug/testing path — no Chrome needed).
            let path = args.get(1).ok_or("usage: sa-browser art <file.png> [--cols N --rows N] [--pixels half|quad|sextant]")?;
            let cols = flag(args, "--cols").and_then(|s| s.parse().ok()).unwrap_or(80usize);
            let rows = flag(args, "--rows").and_then(|s| s.parse().ok()).unwrap_or(24usize);
            let mode = art::Mode::parse(&flag(args, "--pixels").unwrap_or_else(|| "sextant".into()));
            let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
            let img = png::decode(&bytes)?;
            let (lines, w, h) = art::render(&img, cols, rows, mode);
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

/// Bare hostnames become https URLs: `visir.is` / `www.visir.is` →
/// `https://…` — Chrome follows the site's own www/apex redirects from there.
fn normalize_url(url: &str) -> String {
    let u = url.trim();
    if u.is_empty() || u.contains("://") || u.starts_with("about:") || u.starts_with("data:") {
        u.to_string()
    } else {
        format!("https://{u}")
    }
}


const HELP: &str = r#"
sa-browser — visual browser: DOM snapshots + pages as high-DPI terminal art

USAGE:
  sa-browser open <url>       navigate, return compact DOM snapshot
                              (bare hosts OK: visir.is → https://visir.is)
  sa-browser snapshot         DOM snapshot of the current page (no navigation)
  sa-browser status           url \t title \t readyState (address-bar line)
  sa-browser pane [--url u] [--cols N --rows N] [--device tablet|none]
                  [--pixels sextant|quad|half]
                              status line + truecolor page render; default
                              emulates a tablet (768px, dsf 2) sized to fill
                              the pane grid, sextant pixels (2x3 px/cell)
  sa-browser art <file.png>   render a PNG file (debug, no Chrome)
  sa-browser probe            check the DevTools connection

Requires Chrome/Chromium with --remote-debugging-port (config: browser_devtools).
"#;

#[cfg(test)]
mod tests {
    use super::normalize_url;

    #[test]
    fn normalize_url_adds_https_to_bare_hosts() {
        assert_eq!(normalize_url("visir.is"), "https://visir.is");
        assert_eq!(normalize_url("www.visir.is"), "https://www.visir.is");
        assert_eq!(normalize_url("visir.is/frettir"), "https://visir.is/frettir");
        assert_eq!(normalize_url("  ruv.is "), "https://ruv.is");
    }

    #[test]
    fn normalize_url_keeps_explicit_schemes() {
        assert_eq!(normalize_url("https://visir.is"), "https://visir.is");
        assert_eq!(normalize_url("http://localhost:3001"), "http://localhost:3001");
        assert_eq!(normalize_url("about:blank"), "about:blank");
        assert_eq!(normalize_url("data:text/html,x"), "data:text/html,x");
        assert_eq!(normalize_url(""), "");
    }
}
