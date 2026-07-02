//! The persistent `./pi --mode rpc` child: spawn, feed commands on stdin,
//! read the event stream on stdout. Busy/idle is tracked from
//! `agent_start`/`agent_end` edges; assistant text deltas are broadcast.
//!
//! Freeze lesson (2026-07-02, httpc): never block unboundedly on another
//! process — all reads happen on a dedicated thread that only ever feeds a
//! channel; consumers use recv_timeout.

use std::io::{BufRead, BufReader, Write};
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
    /// A turn finished; payload is the full assistant text of that turn.
    TurnEnd(String),
    /// A tool call started (payload: tool name) — visibility while "quiet".
    Tool(String),
    /// Child exited.
    Exited(i32),
}

pub struct PiChild {
    child: Child,
    stdin: Arc<Mutex<std::process::ChildStdin>>,
    pub events: Receiver<Event>,
    pub busy: Arc<Mutex<bool>>,
}

impl PiChild {
    /// Spawn `<repo>/pi --mode rpc --session-id gw-<agent>` from the repo root.
    pub fn spawn(repo_root: &std::path::Path, agent: &str) -> Result<PiChild, String> {
        let launcher = repo_root.join("pi");
        let mut child = Command::new(&launcher)
            .args(["--mode", "rpc", "--session-id", &format!("gw-{agent}")])
            .current_dir(repo_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", launcher.display()))?;
        let stdin = child.stdin.take().ok_or("no child stdin")?;
        let stdout = child.stdout.take().ok_or("no child stdout")?;
        let (tx, rx) = std::sync::mpsc::channel();
        let busy = Arc::new(Mutex::new(false));
        let busy_r = busy.clone();
        std::thread::spawn(move || reader_loop(stdout, tx, busy_r));
        Ok(PiChild {
            child,
            stdin: Arc::new(Mutex::new(stdin)),
            events: rx,
            busy,
        })
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
        stdin.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())
    }

    /// Route a user message: `prompt` when idle, `steer` when busy.
    pub fn send_auto(&self, message: &str) -> Result<&'static str, String> {
        let mode = if self.is_busy() { "steer" } else { "prompt" };
        self.command(mode, Some(message))?;
        Ok(mode)
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Parse the child's stdout lines into distilled events.
fn reader_loop(
    stdout: std::process::ChildStdout,
    tx: Sender<Event>,
    busy: Arc<Mutex<bool>>,
) {
    let mut turn_text = String::new();
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
                let _ = tx.send(Event::TurnEnd(std::mem::take(&mut turn_text)));
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
