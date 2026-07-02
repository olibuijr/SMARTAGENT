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
            let mut out = vec!["service    state     pid      health".to_string()];
            for svc in ctx.registry.iter() {
                let rec = ctx.store.get(svc.name);
                let alive = ctx.alive(svc, &rec);
                let health = match ctx.probe_ok(svc) {
                    Some(true) => "ok",
                    Some(false) => "PROBE-FAIL",
                    None => "—", // no HTTP probe for this service; liveness shown in state
                };
                let state = if alive { "running" } else if rec.desired_up { "DOWN(want up)" } else { "stopped" };
                out.push(format!("{:<10} {:<9} {:<8} {}", svc.name, state, if alive { rec.pid } else { 0 }, health));
            }
            Ok(out.join("\n"))
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
            loop {
                std::thread::sleep(std::time::Duration::from_secs(15));
                for svc in ctx.registry.iter().filter(|s| s.enabled) {
                    let rec = ctx.store.get(svc.name);
                    if (rec.desired_up || !ctx.store.names().contains(&svc.name.to_string()))
                        && !ctx.alive(svc, &rec) {
                            if let Ok(r) = ctx.start(svc) {
                                let mut bumped = r;
                                bumped.restarts += 1;
                                let _ = ctx.store.put(svc.name, &bumped);
                                eprintln!("[supervise] restarted {} (pid {})", svc.name, bumped.pid);
                            }
                        }
                }
            }
        }
        _ => Ok(HELP.trim().into()),
    }
}

const HELP: &str = r#"
supervise — pure-Rust process manager for SMARTAGENT services

USAGE:
  supervise status                 show each service: state, pid, health probe
  supervise up   [service]         start all enabled services (or one)
  supervise down [service]         stop all (or one); clears desired-up
  supervise restart <service>      stop then start one service
  supervise watch                  foreground self-healing loop (restarts dead
                                    services every 15s) — run at boot via a
                                    single `@reboot` crontab line

Services: scheduler (cron daemon), chromium (headless CDP :9222).
State is a semdb table (data/supervise.semdb); logs in workspaces/supervise-logs/.
"#;
