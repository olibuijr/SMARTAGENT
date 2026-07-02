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
}

#[derive(Default)]
pub struct AgentState {
    pub items: Vec<Item>,
    pub streaming: bool,
    pub connected: bool,
    pub model: String,
    pub thinking_level: String,
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
    pub window_title: Option<String>,
    pub turn_started: Option<Instant>,
    pub current_tool: Option<String>,
    /// Prompt queued before the child finished booting (ISC-131).
    pub pending_prompt: Option<String>,
    pub stick_to_bottom: bool,
}

impl AgentState {
    pub fn fresh() -> Self {
        let mut s = Self::default();
        s.stick_to_bottom = true;
        s
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
            Incoming::ExtUi(v) => self.apply_ext_ui(&v, rpc),
        }
    }

    fn apply_response(&mut self, v: &Value) {
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
                    self.stats.input = d.get("input").and_then(|x| x.as_f64()).unwrap_or(0.0) as u64;
                    self.stats.output =
                        d.get("output").and_then(|x| x.as_f64()).unwrap_or(0.0) as u64;
                    self.stats.cost = d.get("cost").and_then(|x| x.as_f64()).unwrap_or(0.0);
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
                        let msg = ev
                            .and_then(|e| e.get("error"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("stream error")
                            .to_string();
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

    fn apply_ext_ui(&mut self, v: &Value, rpc: &RpcClient) {
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
                let default = v.get("default").and_then(|d| d.as_str()).unwrap_or("").to_string();
                self.dialogs.push(Dialog {
                    id,
                    method,
                    prompt,
                    options,
                    input: default,
                    selected: 0,
                });
            }
            "notify" => {
                let m = v.get("message").and_then(|t| t.as_str()).unwrap_or("").to_string();
                self.notice = Some(m);
            }
            "setStatus" => {
                let name = v.get("name").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let status = v.get("status").and_then(|t| t.as_str());
                self.tool_status.retain(|(n, _)| n != &name);
                if let Some(st) = status {
                    if !st.is_empty() {
                        self.tool_status.push((name, st.to_string()));
                    }
                }
            }
            "setWidget" => {
                self.widget_lines.clear();
                if let Some(arr) = v.get("lines").or_else(|| v.get("content")).and_then(|l| l.as_arr()) {
                    for l in arr {
                        if let Some(s) = l.as_str() {
                            self.widget_lines.push(s.to_string());
                        }
                    }
                } else if let Some(s) = v.get("content").and_then(|c| c.as_str()) {
                    self.widget_lines.push(s.to_string());
                }
            }
            "setTitle" => {
                let t = v.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
                self.window_title = Some(t);
            }
            _ => {
                // Unknown fire-and-forget method: acknowledge so agent never hangs.
                rpc.ext_ui_reply(&id, vec![("value", jsonw::Value::Null)]);
            }
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
