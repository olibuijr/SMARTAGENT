//! Event → state reduction. Turns the pi RPC event stream into the display
//! model the views render. No blocking, no I/O — pure folding of `Incoming`
//! records into `AgentState`.

use std::time::Instant;

use httpc::json::Value;

use crate::jsonw;
use crate::rpc::{Incoming, RpcClient};
use crate::sessions::{self, ReplayItem};

/// One rendered transcript entry.
pub enum Item {
    User(String),
    Assistant(Assistant),
    System(String),
    Error(String),
}

#[derive(Default)]
pub struct Assistant {
    pub text: String,
    pub thinking: String,
    pub thinking_open: bool,
    pub tools: Vec<ToolCard>,
    pub streaming: bool,
}

pub struct ToolCard {
    pub id: String,
    pub name: String,
    pub args_summary: String,
    pub args_full: String,
    pub output: String,
    pub running: bool,
    pub is_error: bool,
    pub ms: Option<u64>,
    pub started: Option<Instant>,
    pub expanded: bool,
}

#[derive(Default, Clone)]
pub struct Stats {
    pub input: u64,
    pub output: u64,
    pub cost: f64,
    pub ctx_tokens: u64,
    pub ctx_window: u64,
    pub ctx_percent: f64,
}

pub struct Activity {
    pub name: String,
    pub ms: u64,
    pub is_error: bool,
}

/// A pending extension-UI dialog (ISC-103..110).
pub struct Dialog {
    pub id: String,
    pub method: String, // select | confirm | input | editor
    pub prompt: String,
    pub options: Vec<String>,
    pub input: String,
    pub selected: usize,
    /// Local deadline: when the agent carries a `timeout`, drop the modal on
    /// our side too so it can't linger after the agent auto-resolves (ISC-109).
    pub deadline: Option<Instant>,
}

#[derive(Default)]
pub struct AgentState {
    pub items: Vec<Item>,
    pub streaming: bool,
    pub connected: bool,
    pub model: String,
    pub thinking_level: String,
    /// (provider, id, name) from get_available_models — model picker source.
    pub available_models: Vec<(String, String, String)>,
    /// (name, description) from get_commands — slash-palette source.
    pub commands: Vec<(String, String)>,
    pub session_file: String,
    pub session_id: String,
    pub session_name: String,
    pub stats: Stats,
    pub widget_lines: Vec<String>,
    pub tool_status: Vec<(String, String)>,
    pub activity: Vec<Activity>,
    pub queued: Vec<String>,
    pub working_files: Vec<String>,
    pub artifacts: Vec<String>,
    pub notice: Option<String>,
    pub error_banner: Option<String>,
    pub dialogs: Vec<Dialog>,
    /// (entryId, message text) fork points from get_fork_messages.
    pub fork_points: Vec<(String, String)>,
    pub window_title: Option<String>,
    pub turn_started: Option<Instant>,
    pub current_tool: Option<String>,
    /// Prompt queued before the child finished booting (ISC-131).
    pub pending_prompt: Option<String>,
    /// Composer prefill requested by an extension (set_editor_text).
    pub pending_editor_text: Option<String>,
    pub stick_to_bottom: bool,
}

impl AgentState {
    pub fn fresh() -> Self {
        Self { stick_to_bottom: true, ..Self::default() }
    }

    /// Replace the transcript with items replayed from a session file on disk.
    /// Instant history render on resume before the RPC switch settles (ISC-45/73).
    pub fn load_from_file(&mut self, path: &std::path::Path) {
        self.items.clear();
        for r in sessions::load_transcript(path) {
            self.push_replay(r);
        }
        self.stick_to_bottom = true;
    }

    fn push_replay(&mut self, r: ReplayItem) {
        match r {
            ReplayItem::User(t) => self.items.push(Item::User(t)),
            ReplayItem::AssistantText(t) => {
                self.ensure_assistant();
                if let Some(Item::Assistant(a)) = self.items.last_mut() {
                    if !a.text.is_empty() {
                        a.text.push('\n');
                    }
                    a.text.push_str(&t);
                }
            }
            ReplayItem::Thinking(t) => {
                self.ensure_assistant();
                if let Some(Item::Assistant(a)) = self.items.last_mut() {
                    a.thinking.push_str(&t);
                }
            }
            ReplayItem::Tool { id, name, args_summary, args_full, output, is_error } => {
                self.ensure_assistant();
                if let Some(Item::Assistant(a)) = self.items.last_mut() {
                    a.tools.push(ToolCard {
                        id,
                        name,
                        args_summary,
                        args_full,
                        output,
                        running: false,
                        is_error,
                        ms: None,
                        started: None,
                        expanded: false,
                    });
                }
            }
            ReplayItem::System(t) => self.items.push(Item::System(t)),
        }
    }

    fn ensure_assistant(&mut self) {
        let need = !matches!(self.items.last(), Some(Item::Assistant(_)));
        if need {
            self.items.push(Item::Assistant(Assistant::default()));
        }
    }

    fn new_assistant_stream(&mut self) {
        self.items.push(Item::Assistant(Assistant {
            streaming: true,
            ..Assistant::default()
        }));
        self.stick_to_bottom = true;
    }

    fn last_assistant(&mut self) -> Option<&mut Assistant> {
        for it in self.items.iter_mut().rev() {
            if let Item::Assistant(a) = it {
                return Some(a);
            }
        }
        None
    }

    fn find_card(&mut self, id: &str) -> Option<&mut ToolCard> {
        for it in self.items.iter_mut().rev() {
            if let Item::Assistant(a) = it {
                for c in a.tools.iter_mut() {
                    if c.id == id {
                        return Some(c);
                    }
                }
            }
        }
        None
    }

    /// Drop dialogs whose local timeout elapsed — the agent auto-resolves its
    /// side, so the stale modal must not linger (ISC-109). Returns true if any
    /// dialog was dropped (caller repaints).
    pub fn prune_dialogs(&mut self) -> bool {
        let now = Instant::now();
        let before = self.dialogs.len();
        self.dialogs.retain(|d| d.deadline.map(|dl| now < dl).unwrap_or(true));
        before != self.dialogs.len()
    }

    /// Echo the user's message immediately, before the first event (ISC-74).
    pub fn echo_user(&mut self, text: &str) {
        self.items.push(Item::User(text.to_string()));
        self.stick_to_bottom = true;
    }

    /// Fold one incoming record into state.
    pub fn apply(&mut self, inc: Incoming, rpc: &RpcClient) {
        match inc {
            Incoming::Response(v) => self.apply_response(&v),
            Incoming::Event(v) => self.apply_event(&v),
            Incoming::ExtUi(v) => {
                // Unknown fire-and-forget methods are acked immediately so the
                // agent never hangs (ISC-106); known methods are handled in-state.
                if let Some(ack_id) = self.apply_ext_ui(&v) {
                    rpc.ext_ui_reply(&ack_id, vec![("value", Value::Null)]);
                }
            }
        }
    }

    fn apply_response(&mut self, v: &Value) {
        self.connected = true;
        let cmd = v.get("command").and_then(|c| c.as_str()).unwrap_or("");
        let data = v.get("data");
        match cmd {
            "get_state" => {
                if let Some(d) = data {
                    self.model = model_label(d.get("model"));
                    self.thinking_level =
                        d.get("thinkingLevel").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    self.session_file =
                        d.get("sessionFile").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    self.session_id =
                        d.get("sessionId").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    self.session_name =
                        d.get("sessionName").and_then(|t| t.as_str()).unwrap_or("").to_string();
                    if let Some(s) = d.get("isStreaming").and_then(as_bool) {
                        self.streaming = s;
                    }
                }
            }
            "get_session_stats" => {
                if let Some(d) = data {
                    // Token totals are nested under `tokens`; tolerate flat too.
                    let t = d.get("tokens").unwrap_or(d);
                    self.stats.input = t.get("input").and_then(|x| x.as_f64()).unwrap_or(0.0) as u64;
                    self.stats.output =
                        t.get("output").and_then(|x| x.as_f64()).unwrap_or(0.0) as u64;
                    self.stats.cost = d
                        .get("cost")
                        .and_then(|x| x.as_f64().or_else(|| x.get("total").and_then(|y| y.as_f64())))
                        .unwrap_or(0.0);
                    if let Some(cu) = d.get("contextUsage") {
                        self.stats.ctx_tokens =
                            cu.get("tokens").and_then(|x| x.as_f64()).unwrap_or(0.0) as u64;
                        self.stats.ctx_window =
                            cu.get("contextWindow").and_then(|x| x.as_f64()).unwrap_or(0.0) as u64;
                        self.stats.ctx_percent =
                            cu.get("percent").and_then(|x| x.as_f64()).unwrap_or(0.0);
                    }
                }
            }
            "get_messages" => {
                if let Some(arr) = data.and_then(|d| d.as_arr()) {
                    self.rebuild_from_messages(arr);
                }
            }
            "get_available_models" => {
                if let Some(arr) = data.and_then(|d| d.get("models")).and_then(|m| m.as_arr()) {
                    self.available_models = arr
                        .iter()
                        .filter_map(|m| {
                            let id = m.get("id").and_then(|x| x.as_str())?;
                            let provider = m.get("provider").and_then(|x| x.as_str()).unwrap_or("");
                            let name = m.get("name").and_then(|x| x.as_str()).unwrap_or(id);
                            Some((provider.to_string(), id.to_string(), name.to_string()))
                        })
                        .collect();
                }
            }
            "get_fork_messages" => {
                let arr = data
                    .and_then(|d| d.get("messages").or_else(|| d.get("forkPoints")).or(Some(d)))
                    .and_then(|m| m.as_arr());
                if let Some(arr) = arr {
                    self.fork_points = arr
                        .iter()
                        .filter_map(|m| {
                            let id = m.get("entryId").and_then(|x| x.as_str())?;
                            let text = m.get("text").and_then(|x| x.as_str()).unwrap_or("");
                            Some((id.to_string(), jsonw::truncate_chars(text, 60)))
                        })
                        .collect();
                }
            }
            "get_commands" => {
                if let Some(arr) = data.and_then(|d| d.get("commands")).and_then(|c| c.as_arr()) {
                    self.commands = arr
                        .iter()
                        .filter_map(|c| {
                            let name = c.get("name").and_then(|x| x.as_str())?;
                            let desc = c.get("description").and_then(|x| x.as_str()).unwrap_or("");
                            Some((name.to_string(), desc.to_string()))
                        })
                        .collect();
                }
            }
            _ => {}
        }
    }

    fn rebuild_from_messages(&mut self, arr: &[Value]) {
        self.items.clear();
        for m in arr {
            let msg = m.get("message").unwrap_or(m);
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            match role {
                "user" => {
                    let t = text_of(msg.get("content"));
                    if !t.is_empty() {
                        self.items.push(Item::User(t));
                    }
                }
                "assistant" => {
                    let mut a = Assistant::default();
                    if let Some(blocks) = msg.get("content").and_then(|c| c.as_arr()) {
                        for b in blocks {
                            match b.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                                "text" => {
                                    a.text.push_str(
                                        b.get("text").and_then(|t| t.as_str()).unwrap_or(""),
                                    );
                                }
                                "thinking" => {
                                    a.thinking.push_str(
                                        b.get("thinking").and_then(|t| t.as_str()).unwrap_or(""),
                                    );
                                }
                                "toolCall" => {
                                    let id = b.get("id").and_then(|t| t.as_str()).unwrap_or("");
                                    let name = b.get("name").and_then(|t| t.as_str()).unwrap_or("");
                                    let args = b.get("arguments");
                                    a.tools.push(ToolCard {
                                        id: id.to_string(),
                                        name: name.to_string(),
                                        args_summary: args
                                            .map(sessions::args_summary)
                                            .unwrap_or_default(),
                                        args_full: args.map(jsonw::write).unwrap_or_default(),
                                        output: String::new(),
                                        running: false,
                                        is_error: false,
                                        ms: None,
                                        started: None,
                                        expanded: false,
                                    });
                                }
                                _ => {}
                            }
                        }
                    }
                    self.items.push(Item::Assistant(a));
                }
                "toolResult" => {
                    let id = msg.get("toolCallId").and_then(|t| t.as_str()).unwrap_or("");
                    let out = text_of(msg.get("content"));
                    let is_err = msg.get("isError").and_then(as_bool).unwrap_or(false);
                    if let Some(c) = self.find_card(id) {
                        c.output = out;
                        c.is_error = is_err;
                    }
                }
                _ => {}
            }
        }
        self.stick_to_bottom = true;
    }

    fn apply_event(&mut self, v: &Value) {
        self.connected = true;
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match ty {
            "message_start" => {
                let role =
                    v.get("message").and_then(|m| m.get("role")).and_then(|r| r.as_str()).unwrap_or("");
                if role == "assistant" {
                    self.new_assistant_stream();
                }
                // user message_start is already echoed locally (ISC-74).
            }
            "message_update" => {
                let ev = v.get("assistantMessageEvent");
                let et = ev.and_then(|e| e.get("type")).and_then(|t| t.as_str()).unwrap_or("");
                let delta = ev.and_then(|e| e.get("delta")).and_then(|d| d.as_str()).unwrap_or("");
                match et {
                    "text_delta" => {
                        self.stick_to_bottom = true;
                        if let Some(a) = self.last_assistant() {
                            a.text.push_str(delta);
                        }
                    }
                    "thinking_delta" => {
                        if let Some(a) = self.last_assistant() {
                            a.thinking.push_str(delta);
                        }
                    }
                    "error" => {
                        // Wire shape: {type:"error", reason, error: AssistantMessage}.
                        let reason = ev.and_then(|e| e.get("reason")).and_then(|r| r.as_str()).unwrap_or("error");
                        let detail = text_of(ev.and_then(|e| e.get("error")).and_then(|m| m.get("content")));
                        let msg = if detail.trim().is_empty() {
                            format!("stream error ({reason})")
                        } else {
                            detail
                        };
                        self.items.push(Item::Error(msg));
                    }
                    _ => {}
                }
            }
            "message_end" => {
                let role =
                    v.get("message").and_then(|m| m.get("role")).and_then(|r| r.as_str()).unwrap_or("");
                if role == "assistant" {
                    // Correct any delta drift from the authoritative final message.
                    let final_text =
                        text_of_typed(v.get("message").and_then(|m| m.get("content")), "text");
                    if let Some(a) = self.last_assistant() {
                        if !final_text.is_empty() {
                            a.text = final_text;
                        }
                        a.streaming = false;
                    }
                }
            }
            "tool_execution_start" => {
                let id = v.get("toolCallId").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let name = v.get("toolName").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let args = v.get("args");
                let summary = args.map(sessions::args_summary).unwrap_or_default();
                let full = args.map(jsonw::write).unwrap_or_default();
                self.current_tool = Some(name.clone());
                self.record_files(&name, args);
                if let Some(c) = self.find_card(&id) {
                    c.name = name;
                    c.args_summary = summary;
                    c.args_full = full;
                    c.running = true;
                    c.started = Some(Instant::now());
                } else {
                    self.ensure_assistant();
                    if let Some(Item::Assistant(a)) = self.items.last_mut() {
                        a.tools.push(ToolCard {
                            id,
                            name,
                            args_summary: summary,
                            args_full: full,
                            output: String::new(),
                            running: true,
                            is_error: false,
                            ms: None,
                            started: Some(Instant::now()),
                            expanded: false,
                        });
                    }
                }
            }
            "tool_execution_update" => {
                let id = v.get("toolCallId").and_then(|t| t.as_str()).unwrap_or("");
                let partial = text_of(v.get("partialResult").and_then(|p| p.get("content")));
                if let Some(c) = self.find_card(id) {
                    c.output = jsonw::truncate_chars(&partial, 8192);
                }
            }
            "tool_execution_end" => {
                let id = v.get("toolCallId").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let name = v.get("toolName").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let is_err = v.get("isError").and_then(as_bool).unwrap_or(false);
                let out = text_of(v.get("result").and_then(|r| r.get("content")));
                let out = clamp_output(&out);
                let mut ms = 0u64;
                if let Some(c) = self.find_card(&id) {
                    c.running = false;
                    c.is_error = is_err;
                    c.output = out;
                    if let Some(st) = c.started {
                        ms = st.elapsed().as_millis() as u64;
                        c.ms = Some(ms);
                    }
                }
                self.activity.push(Activity { name, ms, is_error: is_err });
                if self.activity.len() > 50 {
                    self.activity.remove(0);
                }
                self.current_tool = None;
            }
            "agent_end" => {
                self.streaming = false;
                self.turn_started = None;
                self.current_tool = None;
                if let Some(a) = self.last_assistant() {
                    a.streaming = false;
                }
            }
            "turn_start" => {
                if self.turn_started.is_none() {
                    self.turn_started = Some(Instant::now());
                }
            }
            "turn_end" => {
                self.turn_started = None;
            }
            // Agent-side state changes (rename, model/thinking switch from another
            // client or a slash command) — reflect them live.
            "session_info_changed" => {
                if let Some(n) = v.get("name").or_else(|| v.get("sessionName")).and_then(|x| x.as_str()) {
                    self.session_name = n.to_string();
                }
            }
            "thinking_level_changed" => {
                if let Some(l) = v.get("thinkingLevel").or_else(|| v.get("level")).and_then(|x| x.as_str()) {
                    self.thinking_level = l.to_string();
                }
            }
            "model_select" => {
                if let Some(m) = v.get("model") {
                    self.model = model_label(Some(m));
                }
            }
            "auto_retry_start" => {
                let attempt = v.get("attempt").and_then(|x| x.as_f64()).unwrap_or(0.0) as u64;
                let max = v.get("maxAttempts").and_then(|x| x.as_f64()).unwrap_or(0.0) as u64;
                self.notice = Some(format!("retrying (attempt {attempt}/{max})…"));
            }
            "auto_retry_end" => {
                self.notice = None;
            }
            "compaction_start" => {
                self.items.push(Item::System("compacting context…".into()));
            }
            "compaction_end" => {
                self.items.push(Item::System("context compacted".into()));
            }
            "queue_update" => {
                self.queued.clear();
                for key in ["steering", "followUp"] {
                    if let Some(arr) = v.get(key).and_then(|a| a.as_arr()) {
                        for m in arr {
                            let t = m.get("message").and_then(|x| x.as_str()).or_else(|| m.as_str());
                            if let Some(t) = t {
                                self.queued.push(t.to_string());
                            }
                        }
                    }
                }
            }
            "extension_error" => {
                let e = v.get("error").and_then(|x| x.as_str()).unwrap_or("extension error");
                self.error_banner = Some(e.to_string());
            }
            _ => {}
        }
    }

    /// Fold an extension_ui_request. Returns `Some(id)` when the method is
    /// unhandled and the caller must send a default ack.
    fn apply_ext_ui(&mut self, v: &Value) -> Option<String> {
        let id = v.get("id").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let method = v.get("method").and_then(|t| t.as_str()).unwrap_or("").to_string();
        match method.as_str() {
            "select" | "confirm" | "input" | "editor" => {
                let prompt = v
                    .get("prompt")
                    .or_else(|| v.get("message"))
                    .or_else(|| v.get("title"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let mut options = Vec::new();
                if let Some(arr) = v.get("options").and_then(|o| o.as_arr()) {
                    for o in arr {
                        let label = o
                            .as_str()
                            .map(|s| s.to_string())
                            .or_else(|| o.get("label").and_then(|l| l.as_str()).map(|s| s.to_string()))
                            .unwrap_or_default();
                        options.push(label);
                    }
                }
                // editor prefill field is `prefill`; input/select have no prefill.
                let default = v
                    .get("prefill")
                    .or_else(|| v.get("default"))
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                let deadline = v
                    .get("timeout")
                    .and_then(|t| t.as_f64())
                    .filter(|ms| *ms > 0.0)
                    .map(|ms| Instant::now() + std::time::Duration::from_millis(ms as u64));
                self.dialogs.push(Dialog {
                    id,
                    method,
                    prompt,
                    options,
                    input: default,
                    selected: 0,
                    deadline,
                });
                None
            }
            "notify" => {
                let m = v.get("message").and_then(|t| t.as_str()).unwrap_or("").to_string();
                self.notice = Some(m);
                None
            }
            "setStatus" => {
                // Wire fields: statusKey / statusText.
                let key = v.get("statusKey").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let text = v.get("statusText").and_then(|t| t.as_str()).map(strip_ansi);
                self.tool_status.retain(|(n, _)| n != &key);
                if let Some(st) = text {
                    if !st.is_empty() {
                        self.tool_status.push((key, st));
                    }
                }
                None
            }
            "setWidget" => {
                // Wire fields: widgetKey / widgetLines (array of ANSI-colored strings).
                self.widget_lines.clear();
                if let Some(arr) = v.get("widgetLines").and_then(|l| l.as_arr()) {
                    for l in arr {
                        if let Some(s) = l.as_str() {
                            let clean = strip_ansi(s);
                            if !clean.trim().is_empty() {
                                self.widget_lines.push(clean);
                            }
                        }
                    }
                }
                None
            }
            "setTitle" => {
                let t = v.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
                self.window_title = Some(t);
                None
            }
            "set_editor_text" => {
                // Extension prefills the composer.
                let t = v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                self.pending_editor_text = Some(t);
                None
            }
            // Unknown fire-and-forget method: caller acks so the agent never hangs.
            _ => Some(id),
        }
    }

    fn record_files(&mut self, name: &str, args: Option<&Value>) {
        let Some(args) = args else { return };
        let path = ["path", "file_path", "filePath", "file"]
            .iter()
            .find_map(|k| args.get(k).and_then(|x| x.as_str()));
        if let Some(p) = path {
            let p = p.to_string();
            if !self.working_files.contains(&p) {
                self.working_files.push(p.clone());
            }
            let writer = matches!(name, "write" | "edit" | "create" | "str_replace" | "str_replace_editor" | "apply_patch");
            if writer && !self.artifacts.contains(&p) {
                self.artifacts.push(p);
            }
        }
    }
}

/// Strip ANSI SGR escape sequences (`\x1b[…m`) — pi's statusline widget lines
/// are terminal-colored; egui renders the raw escapes as garbage otherwise.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for e in chars.by_ref() {
                    if e.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn clamp_output(s: &str) -> String {
    const CAP: usize = 4096;
    let count = s.chars().count();
    if count <= CAP {
        return s.to_string();
    }
    let head: String = s.chars().take(CAP).collect();
    format!("{head}… ({} bytes total)", s.len())
}

fn model_label(m: Option<&Value>) -> String {
    let Some(m) = m else { return String::new() };
    if let Some(s) = m.as_str() {
        return s.to_string();
    }
    let id = m.get("id").and_then(|t| t.as_str());
    let name = m.get("name").and_then(|t| t.as_str());
    name.or(id).unwrap_or("").to_string()
}

fn as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

/// Concatenate all text blocks of a content array.
fn text_of(content: Option<&Value>) -> String {
    text_of_typed(content, "text")
}

fn text_of_typed(content: Option<&Value>, field: &str) -> String {
    let Some(arr) = content.and_then(|c| c.as_arr()) else {
        // Some content is a bare string.
        return content.and_then(|c| c.as_str()).unwrap_or("").to_string();
    };
    let mut out = String::new();
    for b in arr {
        if b.get("type").and_then(|t| t.as_str()) == Some(field) {
            if let Some(t) = b.get(field).and_then(|t| t.as_str()) {
                out.push_str(t);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpc::json::parse;

    fn ev(s: &mut AgentState, line: &str) {
        s.apply_event(&parse(line).unwrap());
    }

    #[test]
    fn streams_text_deltas_into_live_assistant() {
        let mut s = AgentState::fresh();
        ev(&mut s, r#"{"type":"message_start","message":{"role":"assistant"}}"#);
        ev(&mut s, r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"He"}}"#);
        ev(&mut s, r#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"llo"}}"#);
        match s.items.last().unwrap() {
            Item::Assistant(a) => assert_eq!(a.text, "Hello"),
            _ => panic!("expected assistant item"),
        }
    }

    #[test]
    fn tool_card_lifecycle_by_call_id() {
        let mut s = AgentState::fresh();
        ev(&mut s, r#"{"type":"message_start","message":{"role":"assistant"}}"#);
        ev(&mut s, r#"{"type":"tool_execution_start","toolCallId":"c1","toolName":"tasks","args":{"action":"board"}}"#);
        // running card exists with an args summary drawn from `action`.
        let card = s.find_card("c1").unwrap();
        assert!(card.running);
        assert_eq!(card.name, "tasks");
        assert_eq!(card.args_summary, "board");
        ev(&mut s, r#"{"type":"tool_execution_end","toolCallId":"c1","toolName":"tasks","isError":false,"result":{"content":[{"type":"text","text":"BACKLOG (0)"}]}}"#);
        let card = s.find_card("c1").unwrap();
        assert!(!card.running);
        assert_eq!(card.output, "BACKLOG (0)");
        assert_eq!(s.activity.len(), 1);
        assert!(!s.activity[0].is_error);
    }

    #[test]
    fn agent_end_clears_streaming() {
        let mut s = AgentState::fresh();
        s.streaming = true;
        ev(&mut s, r#"{"type":"agent_end","messages":[]}"#);
        assert!(!s.streaming);
    }

    #[test]
    fn stats_reads_nested_tokens() {
        let mut s = AgentState::fresh();
        let resp = parse(r#"{"type":"response","command":"get_session_stats","success":true,"data":{"tokens":{"input":9236,"output":18},"cost":0,"contextUsage":{"tokens":13862,"contextWindow":400000,"percent":3.46}}}"#).unwrap();
        s.apply_response(&resp);
        assert_eq!(s.stats.input, 9236);
        assert_eq!(s.stats.output, 18);
        assert!((s.stats.ctx_percent - 3.46).abs() < 1e-6);
    }

    #[test]
    fn ext_ui_select_becomes_dialog_known_methods_need_no_ack() {
        let mut s = AgentState::fresh();
        let ack = s.apply_ext_ui(
            &parse(r#"{"type":"extension_ui_request","id":"d1","method":"select","prompt":"Pick","options":["a","b"]}"#).unwrap(),
        );
        assert!(ack.is_none(), "known method must not need a default ack");
        assert_eq!(s.dialogs.len(), 1);
        assert_eq!(s.dialogs[0].method, "select");
        assert_eq!(s.dialogs[0].options, vec!["a", "b"]);
    }

    #[test]
    fn dialog_timeout_prunes_locally() {
        let mut s = AgentState::fresh();
        s.apply_ext_ui(&parse(r#"{"type":"extension_ui_request","id":"d","method":"confirm","prompt":"ok?","timeout":1}"#).unwrap());
        assert_eq!(s.dialogs.len(), 1);
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(s.prune_dialogs());
        assert!(s.dialogs.is_empty());
    }

    #[test]
    fn ext_ui_setwidget_and_unknown_ack() {
        // Real wire fields: widgetKey / widgetLines (regression guard — earlier
        // code read `lines`/`content` and silently no-oped against real pi).
        let mut s = AgentState::fresh();
        s.apply_ext_ui(&parse(r#"{"type":"extension_ui_request","id":"w","method":"setWidget","widgetKey":"k","widgetLines":["info ✓","data ▦"]}"#).unwrap());
        assert_eq!(s.widget_lines, vec!["info ✓", "data ▦"]);
        let ack = s.apply_ext_ui(&parse(r#"{"type":"extension_ui_request","id":"z","method":"mysteryMethod"}"#).unwrap());
        assert_eq!(ack.as_deref(), Some("z"), "unknown method must be acked by id");
    }

    #[test]
    fn ext_ui_setstatus_real_fields() {
        let mut s = AgentState::fresh();
        s.apply_ext_ui(&parse(r#"{"type":"extension_ui_request","id":"s","method":"setStatus","statusKey":"my-ext","statusText":"Turn 3 running…"}"#).unwrap());
        assert_eq!(s.tool_status, vec![("my-ext".to_string(), "Turn 3 running…".to_string())]);
    }

    #[test]
    fn ext_ui_editor_prefill_and_set_editor_text() {
        let mut s = AgentState::fresh();
        s.apply_ext_ui(&parse(r#"{"type":"extension_ui_request","id":"e","method":"editor","title":"Edit","prefill":"Line 1\nLine 2"}"#).unwrap());
        assert_eq!(s.dialogs[0].input, "Line 1\nLine 2");
        s.apply_ext_ui(&parse(r#"{"type":"extension_ui_request","id":"t","method":"set_editor_text","text":"hello"}"#).unwrap());
        assert_eq!(s.pending_editor_text.as_deref(), Some("hello"));
    }

    #[test]
    fn stream_error_reads_object_reason() {
        let mut s = AgentState::fresh();
        ev(&mut s, r#"{"type":"message_start","message":{"role":"assistant"}}"#);
        ev(&mut s, r#"{"type":"message_update","assistantMessageEvent":{"type":"error","reason":"error","error":{"role":"assistant","content":[{"type":"text","text":"rate limited"}]}}}"#);
        let has_err = s.items.iter().any(|i| matches!(i, Item::Error(t) if t == "rate limited"));
        assert!(has_err, "error text must come from the AssistantMessage object");
    }

    #[test]
    fn session_info_and_thinking_change_live() {
        let mut s = AgentState::fresh();
        ev(&mut s, r#"{"type":"session_info_changed","name":"Renamed Session"}"#);
        assert_eq!(s.session_name, "Renamed Session");
        ev(&mut s, r#"{"type":"thinking_level_changed","thinkingLevel":"high"}"#);
        assert_eq!(s.thinking_level, "high");
    }

    #[test]
    fn queue_update_collects_steer_and_followup() {
        let mut s = AgentState::fresh();
        ev(&mut s, r#"{"type":"queue_update","steering":[{"message":"steer me"}],"followUp":[{"message":"later"}]}"#);
        assert_eq!(s.queued, vec!["steer me", "later"]);
    }

    #[test]
    fn auto_retry_sets_and_clears_notice() {
        let mut s = AgentState::fresh();
        ev(&mut s, r#"{"type":"auto_retry_start","attempt":2,"maxAttempts":5}"#);
        assert_eq!(s.notice.as_deref(), Some("retrying (attempt 2/5)…"));
        ev(&mut s, r#"{"type":"auto_retry_end","success":true}"#);
        assert!(s.notice.is_none());
    }

    #[test]
    fn compaction_pushes_system_notices() {
        let mut s = AgentState::fresh();
        ev(&mut s, r#"{"type":"compaction_start","reason":"threshold"}"#);
        ev(&mut s, r#"{"type":"compaction_end","aborted":false}"#);
        let sys: Vec<_> = s
            .items
            .iter()
            .filter_map(|i| match i {
                Item::System(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(sys, vec!["compacting context…", "context compacted"]);
    }
}
