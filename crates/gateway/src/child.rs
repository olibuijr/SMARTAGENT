//! The persistent `./pi --mode rpc` child: spawn, feed commands on stdin,
//! read the event stream on stdout. Busy/idle is tracked from
//! `agent_start`/`agent_end` edges; assistant text deltas are broadcast.
//!
//! Freeze lesson (2026-07-02, httpc): never block unboundedly on another
//! process — all reads happen on a dedicated thread that only ever feeds a
//! channel; consumers use recv_timeout.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use httpc::json::{self, Value};

/// What the reader thread distills from the raw pi event stream.
pub enum Event {
    /// Agent flipped busy (true) / idle (false).
    State(bool),
    /// Streamed assistant text fragment.
    Text(String),
    /// A turn finished; payload is full assistant text plus token usage.
    TurnEnd(String, Usage),
    /// A tool call started (payload: tool name) — visibility while "quiet".
    Tool(String),
    /// Child exited.
    Exited(i32),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

pub struct PiChild {
    child: Child,
    stdin: Arc<Mutex<std::process::ChildStdin>>,
    tx: Sender<Event>,
    repo_root: PathBuf,
    agent: String,
    generation: u64,
    pub events: Receiver<Event>,
    pub busy: Arc<Mutex<bool>>,
}

impl PiChild {
    /// Spawn `<repo>/pi --mode rpc --session-id gw-<agent>` from the repo root.
    pub fn spawn(
        repo_root: &std::path::Path,
        agent: &str,
        static_prompt: Option<&str>,
    ) -> Result<PiChild, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let busy = Arc::new(Mutex::new(false));
        let (child, stdin) = spawn_process(repo_root, agent, 0, static_prompt, &tx, busy.clone())?;
        Ok(PiChild {
            child,
            stdin: Arc::new(Mutex::new(stdin)),
            tx,
            repo_root: repo_root.to_path_buf(),
            agent: agent.to_string(),
            generation: 0,
            events: rx,
            busy,
        })
    }

    /// Archive the wedged session by leaving its session id behind, then start
    /// a fresh gw-<agent>-recovered-* session with a continuity note.
    pub fn rotate_fresh(&mut self, note: &str) -> Result<(), String> {
        self.kill();
        self.generation = now_millis();
        let (child, stdin) = spawn_process(
            &self.repo_root,
            &self.agent,
            self.generation,
            None,
            &self.tx,
            self.busy.clone(),
        )?;
        self.child = child;
        self.stdin = Arc::new(Mutex::new(stdin));
        self.command("prompt", Some(note))
    }

    pub fn is_busy(&self) -> bool {
        *self.busy.lock().unwrap()
    }

    /// Write one RPC command line to the child.
    pub fn command(&self, cmd_type: &str, message: Option<&str>) -> Result<(), String> {
        let line = match message {
            Some(m) => format!(
                "{{\"type\":\"{}\",\"message\":\"{}\"}}\n",
                cmd_type,
                json::escape(m)
            ),
            None => format!("{{\"type\":\"{}\"}}\n", cmd_type),
        };
        let mut stdin = self.stdin.lock().unwrap();
        stdin
            .write_all(line.as_bytes())
            .map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_process(
    repo_root: &std::path::Path,
    agent: &str,
    generation: u64,
    static_prompt: Option<&str>,
    tx: &Sender<Event>,
    busy: Arc<Mutex<bool>>,
) -> Result<(Child, std::process::ChildStdin), String> {
    let launcher = repo_root.join("pi");
    let session = if generation == 0 {
        format!("gw-{agent}")
    } else {
        format!("gw-{agent}-recovered-{generation}")
    };
    let args = spawn_args(&session, static_prompt);
    let mut child = Command::new(&launcher)
        .args(&args)
        // Board ownership: tasks current_owner() reads this, so pulls made by
        // this agent are tagged @<agent> — per-agent attribution on the board
        // and in the sidebar instead of everything showing the unix user.
        .env("SMARTAGENT_GATEWAY_AGENT", agent)
        .current_dir(repo_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", launcher.display()))?;
    let stdin = child.stdin.take().ok_or("no child stdin")?;
    let stdout = child.stdout.take().ok_or("no child stdout")?;
    let tx = tx.clone();
    std::thread::spawn(move || reader_loop(stdout, tx, busy));
    Ok((child, stdin))
}

fn spawn_args(session: &str, static_prompt: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--mode".to_string(),
        "rpc".to_string(),
        "--session-id".to_string(),
        session.to_string(),
    ];
    if let Some(prompt) = static_prompt {
        args.push("--append-system-prompt".to_string());
        args.push(prompt.to_string());
    }
    args
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Parse the child's stdout lines into distilled events.
fn reader_loop(stdout: std::process::ChildStdout, tx: Sender<Event>, busy: Arc<Mutex<bool>>) {
    let mut turn_text = String::new();
    let mut usage = Usage::default();
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(v) = json::parse(&line) else { continue };
        let typ = v.get("type").and_then(Value::as_str).unwrap_or("");
        match typ {
            "agent_start" => {
                *busy.lock().unwrap() = true;
                let _ = tx.send(Event::State(true));
            }
            "agent_end" => {
                *busy.lock().unwrap() = false;
                let _ = tx.send(Event::State(false));
                let _ = tx.send(Event::TurnEnd(std::mem::take(&mut turn_text), usage));
                usage = Usage::default();
            }
            "message_end" | "turn_end" => {
                if let Some(u) = usage_from_event(&v) {
                    usage = u;
                }
            }
            "tool_execution_start" => {
                let name = v
                    .get("toolName")
                    .or_else(|| v.get("tool"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool");
                let _ = tx.send(Event::Tool(name.to_string()));
            }
            "message_update" => {
                if let Some(ev) = v.get("assistantMessageEvent") {
                    if ev.get("type").and_then(Value::as_str) == Some("text_delta") {
                        if let Some(d) = ev.get("delta").and_then(Value::as_str) {
                            turn_text.push_str(d);
                            let _ = tx.send(Event::Text(d.to_string()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let _ = tx.send(Event::Exited(-1));
}

fn usage_from_event(v: &Value) -> Option<Usage> {
    let msg = v.get("message")?;
    let u = msg.get("usage")?;
    Some(Usage {
        input: num(u, "input"),
        output: num(u, "output"),
        cache_read: num(u, "cacheRead"),
        cache_write: num(u, "cacheWrite"),
    })
}

fn num(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_args_append_static_prompt_once() {
        let args = spawn_args("gw-main", Some("static autonomous rules"));
        assert_eq!(
            args,
            vec![
                "--mode",
                "rpc",
                "--session-id",
                "gw-main",
                "--append-system-prompt",
                "static autonomous rules"
            ]
        );
        assert_eq!(
            spawn_args("gw-main", None),
            vec!["--mode", "rpc", "--session-id", "gw-main"]
        );
    }

    #[test]
    fn usage_from_message_end() {
        let v = json::parse(r#"{"type":"message_end","message":{"role":"assistant","usage":{"input":10,"output":4,"cacheRead":20,"cacheWrite":2}}}"#).unwrap();
        let u = usage_from_event(&v).unwrap();
        assert_eq!(u.input, 10);
        assert_eq!(u.output, 4);
        assert_eq!(u.cache_read, 20);
        assert_eq!(u.cache_write, 2);
    }

    #[test]
    fn zero_usage_total_marks_mute_signal() {
        let u = Usage::default();
        assert_eq!(u.input + u.output + u.cache_read + u.cache_write, 0);
    }
}
