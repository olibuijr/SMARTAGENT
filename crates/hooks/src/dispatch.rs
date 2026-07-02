//! Hook dispatch — run every matching hook command with the event payload on
//! stdin, fold their verdicts into one decision, audit to a semdb table.
//!
//! Contract per hook process (Claude Code convention):
//!   exit 0 → allow; stdout MAY be JSON `{"decision":"block","reason":...}`,
//!            `{"updatedInput": {...}}`, or `{"context": "..."}`
//!   exit 2 → BLOCK; stderr is the reason
//!   other  → non-blocking warning
//! First block wins; updatedInput/context from later hooks still apply if no
//! block occurred.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use semdb::json::{self, Value};
use semdb::storage::Db;

use crate::config::Hook;

const PLACEHOLDER_VEC: [f32; 1] = [0.0];

#[derive(Default)]
pub struct Decision {
    pub block: bool,
    pub reason: String,
    /// Replacement tool input (JSON text) if a hook rewrote it.
    pub updated_input: String,
    /// Context lines to inject into the agent.
    pub context: Vec<String>,
    pub warnings: Vec<String>,
}

impl Decision {
    pub fn to_json(&self) -> String {
        let ctx = self.context.iter().map(|c| format!("\"{}\"", json::escape(c))).collect::<Vec<_>>().join(",");
        let warn = self.warnings.iter().map(|c| format!("\"{}\"", json::escape(c))).collect::<Vec<_>>().join(",");
        format!(
            r#"{{"block":{},"reason":"{}","updatedInput":{},"context":[{}],"warnings":[{}]}}"#,
            self.block,
            json::escape(&self.reason),
            if self.updated_input.is_empty() { "null".to_string() } else { self.updated_input.clone() },
            ctx,
            warn
        )
    }
}

/// Run one hook. Returns (exit_code, stdout, stderr, timed_out).
fn run_hook(h: &Hook, repo: &Path, payload: &str) -> Result<(i32, String, String, bool), String> {
    let cmd_path = repo.join(&h.command);
    let mut child = Command::new(&cmd_path)
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("hook '{}': spawn {}: {e}", h.name, cmd_path.display()))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.as_bytes());
    }
    let deadline = Instant::now() + Duration::from_secs(h.timeout_secs.max(1));
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                return Ok((
                    status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&out.stdout).to_string(),
                    String::from_utf8_lossy(&out.stderr).to_string(),
                    false,
                ));
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok((-1, String::new(), format!("hook '{}' timed out after {}s", h.name, h.timeout_secs), true));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(format!("hook '{}': wait: {e}", h.name)),
        }
    }
}

/// Dispatch an event to all matching hooks; fold into one Decision.
pub fn dispatch(hooks: &[Hook], repo: &Path, event: &str, subject: &str, payload: &str) -> Decision {
    let mut d = Decision::default();
    for h in hooks.iter().filter(|h| crate::config::matches(h, event, subject)) {
        let started = Instant::now();
        let (code, stdout, stderr, timed_out) = match run_hook(h, repo, payload) {
            Ok(r) => r,
            Err(e) => {
                d.warnings.push(e);
                continue;
            }
        };
        audit(repo, &h.name, event, subject, code, started.elapsed().as_millis() as i64);
        if timed_out {
            // Fail-open on timeout (a hung hook must not wedge the agent),
            // but loudly.
            d.warnings.push(stderr);
            continue;
        }
        match code {
            2 => {
                d.block = true;
                d.reason = if stderr.trim().is_empty() { format!("blocked by hook '{}'", h.name) } else { stderr.trim().to_string() };
                return d; // first block wins, later hooks skipped
            }
            0 => {
                if let Ok(v) = json::parse(stdout.trim()) {
                    if v.get("decision").and_then(|x| x.as_str()) == Some("block") {
                        d.block = true;
                        d.reason = v.get("reason").and_then(|x| x.as_str()).unwrap_or("blocked").to_string();
                        return d;
                    }
                    if let Some(ui) = v.get("updatedInput") {
                        d.updated_input = to_text(ui);
                    }
                    if let Some(c) = v.get("context").and_then(|x| x.as_str()) {
                        d.context.push(c.to_string());
                    }
                } else if !stdout.trim().is_empty() {
                    // Plain-text stdout = context injection (Claude Code style).
                    d.context.push(stdout.trim().to_string());
                }
            }
            _ => d.warnings.push(format!("hook '{}' exit {code}: {}", h.name, stderr.trim())),
        }
    }
    d
}

fn to_text(v: &Value) -> String {
    // Re-serialize the subset we support.
    match v {
        Value::Str(s) => format!("\"{}\"", json::escape(s)),
        Value::Num(n) => format!("{n}"),
        Value::Bool(b) => format!("{b}"),
        Value::Null => "null".into(),
        Value::Arr(a) => format!("[{}]", a.iter().map(to_text).collect::<Vec<_>>().join(",")),
        Value::Obj(o) => format!(
            "{{{}}}",
            o.iter().map(|(k, x)| format!("\"{}\":{}", json::escape(k), to_text(x))).collect::<Vec<_>>().join(",")
        ),
    }
}

/// Append a firing record to data/hooks.semdb (best-effort — auditing must
/// never break dispatch).
fn audit(repo: &Path, name: &str, event: &str, subject: &str, exit: i32, ms: i64) {
    let path = repo.join("data/hooks.semdb");
    let _ = std::fs::create_dir_all(repo.join("data"));
    let db = if path.exists() { Db::open(&path) } else { Db::create(&path) };
    if let Ok(mut db) = db {
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let meta = format!(
            r#"{{"hook":"{}","event":"{}","subject":"{}","exit":{exit},"ms":{ms},"ts":{ts}}}"#,
            json::escape(name),
            json::escape(event),
            json::escape(subject)
        );
        let _ = db.put(&format!("H-{ts}"), &meta, PLACEHOLDER_VEC.to_vec());
    }
}

pub fn audit_path(repo: &Path) -> PathBuf {
    repo.join("data/hooks.semdb")
}
