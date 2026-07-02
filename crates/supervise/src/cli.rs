//! CLI: up / down / restart / status / watch.

use std::path::{Path, PathBuf};

use semdb::config::Config;

use crate::proc;
use crate::services::{self, Service};
use crate::state::{Record, Store};

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

struct Ctx {
    repo: PathBuf,
    logs: PathBuf,
    store: Store,
    registry: Vec<Service>,
}

impl Ctx {
    fn load() -> Result<Ctx, String> {
        let cfg = Config::load();
        let repo = cfg.data_dir().parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
        let logs = cfg.workspaces_dir().join("supervise-logs");
        std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
        let chromium = cfg.resolve("chromium_bin", "CHROMIUM_BIN", None).unwrap_or_else(|| "chromium".into());
        let profile = repo.join(".pi/chrome-profile").to_string_lossy().to_string();
        Ok(Ctx {
            store: Store::open(&cfg.data_dir())?,
            registry: services::registry(&chromium, &profile),
            repo,
            logs,
        })
    }

    fn alive(&self, svc: &Service, rec: &Record) -> bool {
        proc::is_alive(rec.pid, svc.needle)
    }

    fn probe_ok(&self, svc: &Service) -> Option<bool> {
        svc.probe.as_ref().map(|url| {
            httpc::Request::new("GET", url).timeout(4).send().map(|r| r.status < 500).unwrap_or(false)
        })
    }

    fn start(&self, svc: &Service) -> Result<Record, String> {
        let log = self.logs.join(format!("{}.log", svc.name));
        let pid = proc::spawn_detached(&svc.argv, &self.repo, &log)?;
        let rec = Record {
            desired_up: true,
            pid,
            cmd: svc.argv.join(" "),
            started_at: now_unix(),
            restarts: self.store.get(svc.name).restarts,
        };
        self.store.put(svc.name, &rec)?;
        Ok(rec)
    }

    fn stop(&self, svc: &Service) -> Result<(), String> {
        let mut rec = self.store.get(svc.name);
        proc::terminate(rec.pid);
        rec.desired_up = false;
        rec.pid = 0;
        self.store.put(svc.name, &rec)
    }
}

pub fn run(args: &[String]) -> Result<String, String> {
    let ctx = Ctx::load()?;
    let cmd = args.first().map(String::as_str).unwrap_or("status");
    let target = args.get(1).map(String::as_str);

    fn selected<'a>(ctx: &'a Ctx, target: Option<&str>) -> Vec<&'a Service> {
        match target {
            Some(n) => ctx.registry.iter().filter(|s| s.name == n).collect(),
            None => ctx.registry.iter().filter(|s| s.enabled).collect(),
        }
    }

    match cmd {
        "up" => {
            let mut out = Vec::new();
            for svc in selected(&ctx, target) {
                let rec = ctx.store.get(svc.name);
                if ctx.alive(svc, &rec) {
                    out.push(format!("{}: already running (pid {})", svc.name, rec.pid));
                } else {
                    let r = ctx.start(svc)?;
                    out.push(format!("{}: started (pid {})", svc.name, r.pid));
                }
            }
            Ok(out.join("\n"))
        }
        "down" => {
            let mut out = Vec::new();
            for svc in selected(&ctx, target) {
                ctx.stop(svc)?;
                out.push(format!("{}: stopped", svc.name));
            }
            Ok(out.join("\n"))
        }
        "restart" => {
            let name = target.ok_or("usage: supervise restart <service>")?;
            let svc = services::find(&ctx.registry, name).ok_or_else(|| format!("unknown service '{name}'"))?;
            ctx.stop(svc)?;
            let r = ctx.start(svc)?;
            Ok(format!("{}: restarted (pid {})", svc.name, r.pid))
        }
        "status" => {
            let mut out = vec!["service    state     pid      restarts health".to_string()];
            for svc in ctx.registry.iter() {
                let rec = ctx.store.get(svc.name);
                let alive = ctx.alive(svc, &rec);
                let health = match ctx.probe_ok(svc) {
                    Some(true) => "ok",
                    Some(false) => "PROBE-FAIL",
                    None => "—", // no HTTP probe for this service; liveness shown in state
                };
                let state = if alive { "running" } else if rec.desired_up { "DOWN(want up)" } else { "stopped" };
                out.push(format!("{:<10} {:<9} {:<8} {:<8} {}", svc.name, state, if alive { rec.pid } else { 0 }, rec.restarts, health));
            }
            Ok(out.join("\n"))
        }
        "logs" => {
            // Tail a service's captured output — was write-only before.
            let name = target.ok_or("usage: supervise logs <service> [--tail N]")?;
            let n: usize = httpc::args::flag(args, "--tail").and_then(|s| s.parse().ok()).unwrap_or(40);
            let path = ctx.logs.join(format!("{name}.log"));
            let text = std::fs::read_to_string(&path).map_err(|_| format!("no log at {}", path.display()))?;
            let lines: Vec<&str> = text.lines().collect();
            let start = lines.len().saturating_sub(n);
            Ok(lines[start..].join("\n"))
        }
        "statusline" => {
            // Compact `level|text` for UI statuslines: worst service state wins
            // the level (ok < warn < err); text is `name:state` per service.
            let mut parts = Vec::new();
            let mut worst = 0u8;
            for svc in ctx.registry.iter() {
                let rec = ctx.store.get(svc.name);
                let alive = ctx.alive(svc, &rec);
                let (mark, sev) = if alive {
                    match ctx.probe_ok(svc) {
                        Some(false) => ("probe-fail", 1),
                        _ => ("up", 0),
                    }
                } else if rec.desired_up {
                    ("DOWN", 2)
                } else {
                    ("off", 1)
                };
                worst = worst.max(sev);
                parts.push(format!("{}:{}", svc.name, mark));
            }
            let level = ["ok", "warn", "err"][worst as usize];
            Ok(format!("{level}|{}", parts.join(" ")))
        }
        "watch" => {
            // Self-healing loop: restart any enabled+desired service that died.
            // This is the "mini-systemd" — the one process that must stay up;
            // if it dies, children keep running (detached), just not restarted.
            eprintln!("[supervise] watch loop started");
            for svc in ctx.registry.iter().filter(|s| s.enabled) {
                let rec = ctx.store.get(svc.name);
                if !ctx.alive(svc, &rec) {
                    let _ = ctx.start(svc);
                }
            }
            // Crash-loop backoff: a service that dies within 60s of starting
            // doubles its restart delay (15s → … → 480s cap) instead of being
            // hammered forever; a service that stays up resets the backoff.
            let mut delay: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();
            let mut next_try: std::collections::HashMap<&str, i64> = std::collections::HashMap::new();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(15));
                for svc in ctx.registry.iter().filter(|s| s.enabled) {
                    let rec = ctx.store.get(svc.name);
                    if ctx.alive(svc, &rec) {
                        if now_unix() - rec.started_at > 60 {
                            delay.insert(svc.name, 15);
                        }
                        continue;
                    }
                    if !(rec.desired_up || !ctx.store.names().contains(&svc.name.to_string())) {
                        continue;
                    }
                    if now_unix() < *next_try.get(svc.name).unwrap_or(&0) {
                        continue; // backing off
                    }
                    let quick_death = rec.started_at > 0 && now_unix() - rec.started_at < 60;
                    if let Ok(r) = ctx.start(svc) {
                        let mut bumped = r;
                        bumped.restarts += 1;
                        let _ = ctx.store.put(svc.name, &bumped);
                        let d = if quick_death { (delay.get(svc.name).copied().unwrap_or(15) * 2).min(480) } else { 15 };
                        delay.insert(svc.name, d);
                        next_try.insert(svc.name, now_unix() + d as i64);
                        eprintln!("[supervise] restarted {} (pid {}, next retry window {d}s)", svc.name, bumped.pid);
                    }
                }
            }
        }
        _ => Ok(HELP.trim().into()),
    }
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn statusline_is_one_compact_line() {
        let out = run(&["statusline".to_string()]).unwrap();
        assert!(!out.contains('\n'));
        let (level, body) = out.split_once('|').expect("level|text");
        assert!(matches!(level, "ok" | "warn" | "err"));
        for part in body.split(' ') {
            let (name, state) = part.split_once(':').expect("name:state token");
            assert!(!name.is_empty());
            assert!(matches!(state, "up" | "probe-fail" | "DOWN" | "off"));
        }
    }
}

const HELP: &str = r#"
supervise — pure-Rust process manager for SMARTAGENT services

USAGE:
  supervise status                 show each service: state, pid, health probe
  supervise statusline             compact one-line status (for UI statuslines)
  supervise up   [service]         start all enabled services (or one)
  supervise down [service]         stop all (or one); clears desired-up
  supervise restart <service>      stop then start one service
  supervise logs <service> [--tail N]   tail a service's captured output
  supervise watch                  foreground self-healing loop (restarts dead
                                    services every 15s) — run at boot via a
                                    single `@reboot` crontab line

Services: scheduler (cron daemon), chromium (headless CDP :9222).
State is a semdb table (data/supervise.semdb); logs in workspaces/supervise-logs/.
"#;
