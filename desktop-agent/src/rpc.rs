//! pi RPC-mode subprocess driver.
//!
//! Spawns `<root>/pi --mode rpc` (the repo launcher, so extensions / AGENTS.md /
//! secrets token / session dir all match the TUI), speaks LF-framed JSONL, and
//! forwards every stdout record to the UI thread over an mpsc channel. All child
//! I/O happens on background threads — the UI thread never blocks (ISC-120).

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

use eframe::egui;
use httpc::json::{parse, Value};

use crate::jsonw;

/// A single record read from the child's stdout.
pub enum Incoming {
    /// `{type:"response", id?, command, success, data?, error?}`
    Response(Value),
    /// Any `AgentSessionEvent` (has a `type`, no `id`).
    Event(Value),
    /// `{type:"extension_ui_request", id, method, ...}`
    ExtUi(Value),
}

/// Live connection to one pi RPC child, bound to a fixed cwd.
pub struct RpcClient {
    child: Child,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    rx: Receiver<Incoming>,
    stderr_buf: Arc<Mutex<String>>,
    next_id: AtomicU64,
    alive: bool,
}

impl RpcClient {
    /// Spawn `<pi_path> --mode rpc` with cwd `context_cwd`. `repaint` is pinged on
    /// every incoming record so streams render without user input (ISC-24).
    pub fn spawn(
        pi_path: &Path,
        context_cwd: &Path,
        repaint: egui::Context,
    ) -> std::io::Result<Self> {
        let mut child = Command::new(pi_path)
            .arg("--mode")
            .arg("rpc")
            .current_dir(context_cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let stdin = child.stdin.take().expect("piped stdin");

        let (tx, rx) = channel();
        // Reader thread: one JSON record per line, classified by `type`.
        let rc = repaint.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF: child closed stdout
                    Ok(_) => {
                        let trimmed = line.trim_end_matches(['\n', '\r']);
                        if trimmed.is_empty() {
                            continue;
                        }
                        match parse(trimmed) {
                            Ok(v) => {
                                let kind =
                                    v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                                let msg = match kind {
                                    "response" => Incoming::Response(v),
                                    "extension_ui_request" => Incoming::ExtUi(v),
                                    _ => Incoming::Event(v),
                                };
                                if tx.send(msg).is_err() {
                                    break;
                                }
                                rc.request_repaint();
                            }
                            Err(e) => {
                                // ISC-12: skip malformed line, don't panic.
                                eprintln!("[rpc] skipped malformed stdout line: {e}");
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            rc.request_repaint();
        });

        // stderr drain thread: prevents pipe-full deadlock, keeps text for errors.
        let stderr_buf = Arc::new(Mutex::new(String::new()));
        let sb = stderr_buf.clone();
        let rc2 = repaint;
        thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if let Ok(mut buf) = sb.lock() {
                            buf.push_str(&line);
                            // Keep only the last ~8 KiB so it can't grow unbounded.
                            if buf.len() > 8192 {
                                let start = buf.len() - 8192;
                                *buf = buf[start..].to_string();
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            rc2.request_repaint();
        });

        Ok(Self {
            child,
            stdin: Arc::new(Mutex::new(Some(stdin))),
            rx,
            stderr_buf,
            next_id: AtomicU64::new(1),
            alive: true,
        })
    }

    /// Non-blocking drain of pending records.
    pub fn poll(&self) -> Vec<Incoming> {
        let mut out = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            out.push(msg);
        }
        out
    }

    /// True until the child process has exited.
    pub fn is_alive(&mut self) -> bool {
        if !self.alive {
            return false;
        }
        match self.child.try_wait() {
            Ok(Some(_)) => {
                self.alive = false;
                false
            }
            Ok(None) => true,
            Err(_) => {
                self.alive = false;
                false
            }
        }
    }

    /// Last captured stderr (diagnostic on child death, ISC-132).
    pub fn stderr_tail(&self) -> String {
        self.stderr_buf.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Write one command object (LF-framed). Returns the assigned id.
    fn send(&self, cmd_type: &str, mut fields: Vec<(&str, Value)>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        fields.insert(0, ("type", jsonw::s(cmd_type)));
        fields.push(("id", jsonw::s(&id.to_string())));
        let line = jsonw::write(&jsonw::obj(fields));
        if let Ok(mut guard) = self.stdin.lock() {
            if let Some(w) = guard.as_mut() {
                let _ = w.write_all(line.as_bytes());
                let _ = w.write_all(b"\n");
                let _ = w.flush();
            }
        }
        id
    }

    pub fn prompt(&self, message: &str, streaming: bool) {
        let mut fields = vec![("message", jsonw::s(message))];
        if streaming {
            fields.push(("streamingBehavior", jsonw::s("steer")));
        }
        self.send("prompt", fields);
    }

    pub fn abort(&self) {
        self.send("abort", vec![]);
    }

    pub fn get_state(&self) {
        self.send("get_state", vec![]);
    }

    pub fn get_session_stats(&self) {
        self.send("get_session_stats", vec![]);
    }

    pub fn new_session(&self) {
        self.send("new_session", vec![]);
    }

    pub fn switch_session(&self, session_path: &str) {
        self.send("switch_session", vec![("sessionPath", jsonw::s(session_path))]);
    }

    pub fn get_messages(&self) {
        self.send("get_messages", vec![]);
    }

    pub fn get_commands(&self) {
        self.send("get_commands", vec![]);
    }

    /// Reply to an `extension_ui_request` (ISC-103..110).
    pub fn ext_ui_reply(&self, req_id: &str, fields: Vec<(&str, Value)>) {
        let mut all = vec![
            ("type", jsonw::s("extension_ui_response")),
            ("id", jsonw::s(req_id)),
        ];
        all.extend(fields);
        let line = jsonw::write(&jsonw::obj(all));
        if let Ok(mut guard) = self.stdin.lock() {
            if let Some(w) = guard.as_mut() {
                let _ = w.write_all(line.as_bytes());
                let _ = w.write_all(b"\n");
                let _ = w.flush();
            }
        }
    }

    /// Close stdin so pi shuts down gracefully (ISC-23).
    pub fn shutdown(&mut self) {
        if let Ok(mut guard) = self.stdin.lock() {
            *guard = None; // drop ChildStdin → EOF on child stdin
        }
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        self.shutdown();
        // Give pi a beat to exit on its own, then reap so we leave no orphan
        // (ISC-122). Non-blocking: try_wait a few times, kill as last resort.
        for _ in 0..20 {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => thread::sleep(std::time::Duration::from_millis(25)),
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
