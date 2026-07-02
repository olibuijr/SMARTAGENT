//! browser — Browser Use port: pure-Rust CDP client driving a real Chrome.
pub mod cdp;
pub mod ws;

/// Resolve the DevTools base URL: --devtools flag → BROWSER_DEVTOOLS env →
/// config `browser_devtools` → localhost default. Shared by browser and
/// sa-browser CLIs (was duplicated byte-identically in both).
pub fn devtools_base(args: &[String]) -> String {
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
