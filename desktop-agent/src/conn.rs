//! `AgentConn` — one pi RPC child bound to a fixed cwd, plus its reduced state
//! and composer input. Owns spawn / pump / send so `main` stays thin.

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::agent::AgentState;
use crate::rpc::RpcClient;

pub struct AgentConn {
    pub state: AgentState,
    pub input: String,
    pub cwd: PathBuf,
    client: Option<RpcClient>,
    was_streaming: bool,
}

/// What one pump cycle observed, so the App can react (stats/git refresh).
pub struct Pumped {
    pub turn_ended: bool,
    pub died: bool,
}

impl AgentConn {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            state: AgentState::fresh(),
            input: String::new(),
            cwd,
            client: None,
            was_streaming: false,
        }
    }

    pub fn is_spawned(&self) -> bool {
        self.client.is_some()
    }

    /// Spawn the child if not already running. Errors surface as a banner (ISC-7).
    pub fn ensure(&mut self, pi: &Path, ctx: &egui::Context) {
        if self.client.is_some() {
            return;
        }
        match RpcClient::spawn(pi, &self.cwd, ctx.clone()) {
            Ok(c) => {
                c.get_state();
                self.client = Some(c);
                self.state.connected = false;
                self.state.error_banner = None;
            }
            Err(e) => {
                self.state.error_banner =
                    Some(format!("could not start agent ({}): {e}", pi.display()));
            }
        }
    }

    /// Drain records, detect death, flush a queued prompt, request end-of-turn stats.
    pub fn pump(&mut self) -> Pumped {
        let mut out = Pumped { turn_ended: false, died: false };
        let Self { state, client, .. } = self;
        let Some(c) = client.as_mut() else { return out };

        if !c.is_alive() {
            if state.connected || state.error_banner.is_none() {
                let tail = c.stderr_tail();
                let tail = tail.trim();
                state.error_banner = Some(if tail.is_empty() {
                    "agent process exited".to_string()
                } else {
                    format!("agent exited: {}", last_line(tail))
                });
            }
            state.connected = false;
            state.streaming = false;
            *client = None;
            out.died = true;
            return out;
        }

        for inc in c.poll() {
            state.apply(inc, c);
        }
        state.prune_dialogs();

        // Flush a prompt queued before the child was ready (ISC-131).
        if state.connected {
            if let Some(text) = state.pending_prompt.take() {
                state.echo_user(&text);
                c.prompt(&text, state.streaming);
                state.streaming = true;
            }
        }

        if self_streaming_ended(self.was_streaming, state.streaming) {
            c.get_session_stats();
            out.turn_ended = true;
        }
        self.was_streaming = state.streaming;
        out
    }

    /// Send a prompt (queues if the child is still booting; steers if streaming).
    pub fn send(&mut self, pi: &Path, ctx: &egui::Context, text: String) {
        self.ensure(pi, ctx);
        let Some(c) = self.client.as_ref() else { return };
        if self.state.connected {
            self.state.echo_user(&text);
            c.prompt(&text, self.state.streaming);
            self.state.streaming = true;
        } else {
            self.state.pending_prompt = Some(text);
        }
    }

    pub fn abort(&mut self) {
        if let Some(c) = &self.client {
            c.abort();
        }
    }

    pub fn new_session(&mut self, pi: &Path, ctx: &egui::Context) {
        self.ensure(pi, ctx);
        if let Some(c) = &self.client {
            c.new_session();
            c.get_state();
        }
        self.state.items.clear();
        self.state.working_files.clear();
        self.state.artifacts.clear();
        self.state.streaming = false;
    }

    /// Resume a session: instant file replay, then bind the RPC child to it.
    pub fn switch(&mut self, pi: &Path, ctx: &egui::Context, path: &Path) {
        self.ensure(pi, ctx);
        self.state.load_from_file(path);
        if let Some(c) = &self.client {
            c.switch_session(&path.to_string_lossy());
            c.get_state();
            c.get_messages();
        }
    }

    /// Fire the reply for a dialog the App resolved.
    pub fn dialog_reply(&self, id: &str, fields: Vec<(&str, httpc::json::Value)>) {
        if let Some(c) = &self.client {
            c.ext_ui_reply(id, fields);
        }
    }
}

fn self_streaming_ended(was: bool, now: bool) -> bool {
    was && !now
}

fn last_line(s: &str) -> String {
    s.lines().last().unwrap_or(s).to_string()
}
