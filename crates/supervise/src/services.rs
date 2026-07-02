//! The service registry: the long-running processes supervise manages. Kept
//! small and declarative; argv is resolved against the repo root at run time.

pub struct Service {
    pub name: &'static str,
    /// Command argv. Relative paths resolve against the repo root (workdir).
    pub argv: Vec<String>,
    /// Substring that must appear in the live process's /proc cmdline —
    /// guards against PID reuse.
    pub needle: &'static str,
    /// Optional HTTP health probe; liveness alone is used when None.
    pub probe: Option<String>,
    /// Started by `supervise up` / `watch` unless explicitly disabled.
    pub enabled: bool,
}

fn s(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

/// The built-in services. `chromium_bin` comes from config/env so the path is
/// not hardcoded; everything else is repo-relative.
pub fn registry(chromium_bin: &str, chrome_profile: &str) -> Vec<Service> {
    vec![
        Service {
            name: "scheduler",
            argv: s(&[
                "target/release/schedule",
                "run",
                "--journal",
                "data/schedule.semdb",
            ]),
            needle: "schedule run",
            probe: None,
            enabled: true,
        },
        Service {
            name: "gateway",
            argv: s(&[
                "target/release/gateway",
                "serve",
                "--agents",
                "linus,ada,grace,ken,dennis,margaret,turing,woz",
                "--autonomous",
            ]),
            needle: "gateway serve",
            probe: None,
            enabled: true,
        },
        Service {
            name: "telegram",
            argv: s(&[
                "target/release/telegram",
                "listen",
                "--gateway",
                "linus",
            ]),
            needle: "telegram listen",
            probe: None,
            enabled: false,
        },
        Service {
            name: "chromium",
            argv: vec![
                chromium_bin.to_string(),
                "--headless=new".into(),
                "--remote-debugging-port=9222".into(),
                format!("--user-data-dir={chrome_profile}"),
                "--no-first-run".into(),
                "--no-default-browser-check".into(),
                "--disable-gpu".into(),
            ],
            needle: "remote-debugging-port=9222",
            probe: Some("http://127.0.0.1:9222/json/version".into()),
            enabled: true,
        },
    ]
}

pub fn find<'a>(reg: &'a [Service], name: &str) -> Option<&'a Service> {
    reg.iter().find(|svc| svc.name == name)
}
