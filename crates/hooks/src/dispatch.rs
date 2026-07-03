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

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use semdb::json::{self, Value};
use semdb::storage::Db;

use crate::config::Hook;

const SEMDB_MAGIC: &[u8; 8] = b"SEMDB01\n";
const OP_PUT: u8 = 1;
const HOOK_AUDIT_RETAIN_MAX_DEFAULT: usize = 10_000;
const HOOK_AUDIT_RETAIN_DAYS_DEFAULT: u64 = 14;


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

const SPAWN_BUSY_RETRIES: u32 = 10;
const SPAWN_BUSY_BACKOFF_MS: u64 = 5;

/// Spawn a hook's process, retrying past a transient `ExecutableFileBusy`
/// (ETXTBSY, "Text file busy"). A hook script is often written to disk
/// (`fs::write` → close) an instant before it is exec'd. Under concurrent
/// hook dispatch (multiple fleet agents firing hooks at once, or a hook
/// re-materialized just before each run) the kernel can briefly still
/// consider the just-closed file "busy" from the writer's fd, and exec
/// spuriously fails with ETXTBSY even though the write is already done and
/// the script is otherwise perfectly runnable. That's the exact fingerprint
/// of this error — it always self-clears within milliseconds once the
/// writer's fd is gone, so a short bounded retry is the correct fix (not a
/// real, permanent spawn failure to surface to the caller).
fn spawn_hook(cmd_path: &Path, repo: &Path) -> std::io::Result<std::process::Child> {
    let mut attempt = 0u32;
    loop {
        match Command::new(cmd_path).current_dir(repo).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Ok(child) => return Ok(child),
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < SPAWN_BUSY_RETRIES => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(SPAWN_BUSY_BACKOFF_MS * attempt as u64));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Run one hook. Returns (exit_code, stdout, stderr, timed_out).
fn run_hook(h: &Hook, repo: &Path, payload: &str) -> Result<(i32, String, String, bool), String> {
    let cmd_path = repo.join(&h.command);
    let mut child =
        spawn_hook(&cmd_path, repo).map_err(|e| format!("hook '{}': spawn {}: {e}", h.name, cmd_path.display()))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.as_bytes());
        // stdin drops here → EOF, so a hook that reads to end can proceed.
    }
    // Drain stdout/stderr on dedicated threads WHILE waiting. Previously output
    // was only read after exit via wait_with_output(); a hook writing more than
    // the ~64KB pipe buffer blocked on write, never exited, hit the timeout, and
    // returned timed_out=true — which FAILS OPEN, silently defeating a blocking
    // hook. Concurrent readers keep the pipe drained so the hook can finish.
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let out_t = std::thread::spawn(move || drain(out_pipe));
    let err_t = std::thread::spawn(move || drain(err_pipe));
    let deadline = Instant::now() + Duration::from_secs(h.timeout_secs.max(1));
    let (code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code().unwrap_or(-1), false),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill(); // closing the pipes unblocks the readers
                let _ = child.wait();
                break (-1, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(format!("hook '{}': wait: {e}", h.name)),
        }
    };
    if timed_out {
        // Do NOT join the reader threads here: a killed hook may have left an
        // orphaned grandchild still holding the stdout/stderr pipe open (finding
        // #36), so read_to_end would never see EOF and join() would hang forever
        // — reintroducing the very wedge we're avoiding. Detach them; they exit
        // when the grandchild dies. Output is discarded on timeout anyway.
        return Ok((
            -1,
            String::new(),
            format!("hook '{}' timed out after {}s", h.name, h.timeout_secs),
            true,
        ));
    }
    // Clean exit → the child closed its pipes, so the readers have hit EOF.
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();
    Ok((code, stdout, stderr, false))
}

/// Read a child pipe to EOF (on its own thread) and lossy-decode it.
fn drain(pipe: Option<impl std::io::Read>) -> String {
    let mut buf = Vec::new();
    if let Some(mut p) = pipe {
        let _ = p.read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).into_owned()
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
/// never break dispatch). This hot path must not call `Db::open`: opening a
/// semdb replays the full log to rebuild the index, which made every hook
/// firing O(file) and a long-running session O(n²). We only need append-only
/// audit durability here, so write one valid semdb PUT frame directly and let
/// readers (`hooks audit`, status probes) pay the replay cost when they ask.
fn audit(repo: &Path, name: &str, event: &str, subject: &str, exit: i32, ms: i64) {
    let path = audit_path(repo);
    let _ = std::fs::create_dir_all(repo.join("data"));
    let ts = unix_nanos();
    let meta = format!(
        r#"{{"hook":"{}","event":"{}","subject":"{}","exit":{exit},"ms":{ms},"ts":{ts}}}"#,
        json::escape(name),
        json::escape(event),
        json::escape(subject)
    );
    let id = format!("H-{ts}");
    if append_audit_frame(&path, &id, &meta).is_ok() {
        maybe_prune_audit(&path);
    }
}

fn unix_nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

fn append_audit_frame(path: &Path, id: &str, meta: &str) -> Result<(), String> {
    let new_file = !path.exists();
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    if new_file {
        f.write_all(SEMDB_MAGIC).map_err(|e| e.to_string())?;
    }
    let mut body = vec![OP_PUT];
    put_bytes16(&mut body, id.as_bytes());
    put_bytes32(&mut body, meta.as_bytes());
    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(&0.0f32.to_le_bytes());
    write_framed(&mut f, &body)?;
    f.flush().map_err(|e| e.to_string())
}

fn maybe_prune_audit(path: &Path) {
    let max_rows = audit_retain_max();
    if max_rows == 0 {
        return;
    }
    // Cheap size gate: skip the expensive replay until the log is plausibly
    // larger than the retained live set. The exact cap is enforced when prune
    // runs; between prunes the append path remains O(1).
    let prune_after_bytes = (max_rows as u64).saturating_mul(200).max(32 * 1024);
    if std::fs::metadata(path).map(|m| m.len() <= prune_after_bytes).unwrap_or(true) {
        return;
    }
    let _ = prune_audit(path, max_rows, audit_retain_days());
}

fn prune_audit(path: &Path, max_rows: usize, retain_days: u64) -> Result<(), String> {
    let db = Db::open(path)?;
    let min_ts = unix_nanos().saturating_sub((retain_days as u128).saturating_mul(24 * 60 * 60 * 1_000_000_000));
    let mut rows: Vec<(u128, String, String)> = db
        .index
        .iter()
        .filter_map(|(id, e)| {
            let ts = json::parse(&e.meta).ok().and_then(|v| v.get("ts").and_then(|x| x.as_f64())).unwrap_or(0.0) as u128;
            (ts >= min_ts).then(|| (ts, id.clone(), e.meta.clone()))
        })
        .collect();
    rows.sort_by_key(|(ts, _, _)| *ts);
    if rows.len() > max_rows {
        rows.drain(0..rows.len() - max_rows);
    }
    rewrite_audit(path, &rows)
}

fn rewrite_audit(path: &Path, rows: &[(u128, String, String)]) -> Result<(), String> {
    let tmp = path.with_extension("semdb.prune");
    {
        let mut f = File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(SEMDB_MAGIC).map_err(|e| e.to_string())?;
        for (_, id, meta) in rows {
            let mut body = vec![OP_PUT];
            put_bytes16(&mut body, id.as_bytes());
            put_bytes32(&mut body, meta.as_bytes());
            body.extend_from_slice(&1u32.to_le_bytes());
            body.extend_from_slice(&0.0f32.to_le_bytes());
            write_framed(&mut f, &body)?;
        }
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(tmp, path).map_err(|e| e.to_string())
}

fn audit_retain_max() -> usize {
    std::env::var("SMARTAGENT_HOOK_AUDIT_RETAIN_MAX").ok().and_then(|s| s.parse().ok()).unwrap_or(HOOK_AUDIT_RETAIN_MAX_DEFAULT)
}

fn audit_retain_days() -> u64 {
    std::env::var("SMARTAGENT_HOOK_AUDIT_RETAIN_DAYS").ok().and_then(|s| s.parse().ok()).unwrap_or(HOOK_AUDIT_RETAIN_DAYS_DEFAULT)
}

fn write_framed(f: &mut File, body: &[u8]) -> Result<(), String> {
    f.write_all(&(body.len() as u32).to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(&semdb::storage::crc32(body).to_le_bytes()).map_err(|e| e.to_string())?;
    f.write_all(body).map_err(|e| e.to_string())
}

fn put_bytes16(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
}

fn put_bytes32(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

pub fn audit_path(repo: &Path) -> PathBuf {
    repo.join("data/hooks.semdb")
}

#[cfg(test)]
mod audit_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("data")).unwrap();
        d
    }

    #[test]
    fn audit_fast_path_writes_readable_semdb_without_replay_per_append() {
        let root = scratch("hooks-audit-fast-path");
        let start = Instant::now();
        for i in 0..10_000 {
            audit(&root, "fast", "tool_call", &format!("bash-{i}"), 0, 1);
        }
        let elapsed = start.elapsed();
        let db = Db::open(&audit_path(&root)).unwrap();
        assert_eq!(db.index.len(), 10_000);
        assert!(elapsed.as_secs() < 10, "10k append-only audit firings took {:?}", elapsed);
    }

    #[test]
    fn prune_audit_caps_rows_to_last_k() {
        let root = scratch("hooks-audit-prune");
        let path = audit_path(&root);
        for i in 0..25 {
            append_audit_frame(&path, &format!("H-{i:04}"), &format!(r#"{{"hook":"h","event":"e","subject":"s","exit":0,"ms":1,"ts":{i}}}"#)).unwrap();
        }
        prune_audit(&path, 10, 365 * 100).unwrap();
        let db = Db::open(&path).unwrap();
        assert_eq!(db.index.len(), 10);
        assert!(db.get("H-0024").is_some());
        assert!(db.get("H-0000").is_none());
    }
}
