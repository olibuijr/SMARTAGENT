//! Persistent chat sessions — owned by Agent A (see os/PLAN.md).
//!
//! Provides an on-device session store (create / auto-name / rename / delete)
//! plus an in-app themed dropdown (`SessionBar`) to switch and manage sessions.
//!
//! ## Persistence
//! Sessions are saved as a single JSON file, `sessions.json`, under an app data
//! directory resolved (in order):
//!   1. `$SMARTAGENT_OS_DATA/` if set,
//!   2. else `$HOME/.smartagent-os/`,
//!   3. else `./.smartagent-os/` (next to the working dir).
//! On mobile/desktop the app runs as a native webview, so `std::fs` to a
//! per-app dir persists across launches. On `wasm32` (web build) persistence is
//! a no-op (in-memory only) — no extra crate deps are pulled in for storage.
//! JSON is hand-encoded/parsed here (zero new deps; the os/ Cargo.toml is not
//! edited by this agent).
//!
//! ## Store API (what `chat.rs` should call)
//! Get the store handle in any component:
//! ```ignore
//! let sessions = crate::sessions::use_sessions();
//! ```
//! Read state (reactive — reads subscribe the calling component):
//! ```ignore
//! let id       = sessions.active_id();          // current session id (u64)
//! let msgs     = sessions.active_messages();    // Vec<StoredMsg { role, text }>
//! let all      = sessions.list();               // Vec<Session>
//! ```
//! Mutate (each call persists to disk on native):
//! ```ignore
//! let new_id = sessions.create();               // new blank session, selected
//! sessions.select(id);
//! sessions.rename(id, "My chat");
//! sessions.delete(id);                          // keeps >=1 session alive
//! sessions.push_message(id, "you", &text);      // appends; auto-names on 1st user msg
//! sessions.push_message(id, "jeeves", &reply);
//! sessions.set_messages(id, msgs);              // replace transcript wholesale
//! ```
//! `StoredMsg { role: String, text: String }` mirrors `chat::Msg` (whose `role`
//! is `&'static str`). Auto-naming truncates the first user message to ~30 chars.
//! `Session::title()` yields the display name ("New chat" when unnamed).
#![allow(dead_code)]

use dioxus::prelude::*;

// ── Model ───────────────────────────────────────────────────────────────────

/// One stored chat message. `role` is "you" | "jeeves" (matches `chat::Msg`).
#[derive(Clone, PartialEq, Debug)]
pub struct StoredMsg {
    pub role: String,
    pub text: String,
}

/// A persisted chat session.
#[derive(Clone, PartialEq, Debug)]
pub struct Session {
    pub id: u64,
    /// Empty string == not yet auto-named; render via [`Session::title`].
    pub name: String,
    pub msgs: Vec<StoredMsg>,
}

impl Session {
    /// Display title — the name, or "New chat" when still unnamed.
    pub fn title(&self) -> String {
        if self.name.is_empty() {
            "New chat".to_string()
        } else {
            self.name.clone()
        }
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
struct Store {
    sessions: Vec<Session>,
    active: u64,
    seq: u64,
}

impl Store {
    /// Load from disk (native) or start fresh, always leaving >=1 valid session
    /// and an `active` id that points at a real session.
    fn load() -> Store {
        let mut store = read_from_disk().unwrap_or_default();
        if store.sessions.is_empty() {
            store.seq += 1;
            let id = store.seq;
            store.sessions.push(Session { id, name: String::new(), msgs: Vec::new() });
            store.active = id;
        }
        if !store.sessions.iter().any(|s| s.id == store.active) {
            store.active = store.sessions[0].id;
        }
        store
    }
}

/// Global reactive store — lazily initialised from disk on first access.
static SESSIONS: GlobalSignal<Store> = Signal::global(Store::load);

/// Apply a mutation and persist the result.
fn mutate(f: impl FnOnce(&mut Store)) {
    let snapshot = {
        let mut w = SESSIONS.write();
        f(&mut w);
        w.clone()
    };
    persist(&snapshot);
}

/// Derive a session name from its first user message (~30 chars, single line).
fn auto_name(text: &str) -> String {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for c in clean.chars() {
        if out.chars().count() >= 30 {
            out.push('…');
            break;
        }
        out.push(c);
    }
    if out.is_empty() {
        "New chat".to_string()
    } else {
        out
    }
}

// ── Store handle ─────────────────────────────────────────────────────────────

/// Lightweight, `Copy` handle over the global session store. Obtain via
/// [`use_sessions`]; all reads are reactive and all writes persist to disk.
#[derive(Clone, Copy, PartialEq)]
pub struct Sessions;

/// Hook: get the shared session store handle (see module docs for the API).
pub fn use_sessions() -> Sessions {
    Sessions
}

impl Sessions {
    /// All sessions, in stored order.
    pub fn list(&self) -> Vec<Session> {
        SESSIONS.read().sessions.clone()
    }

    /// The currently selected session id.
    pub fn active_id(&self) -> u64 {
        SESSIONS.read().active
    }

    /// The currently selected session, if any.
    pub fn active(&self) -> Option<Session> {
        let store = SESSIONS.read();
        store.sessions.iter().find(|s| s.id == store.active).cloned()
    }

    /// Messages of the active session (empty if none).
    pub fn active_messages(&self) -> Vec<StoredMsg> {
        self.active().map(|s| s.msgs).unwrap_or_default()
    }

    /// Messages of a specific session.
    pub fn messages(&self, id: u64) -> Vec<StoredMsg> {
        SESSIONS
            .read()
            .sessions
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.msgs.clone())
            .unwrap_or_default()
    }

    /// Create a new blank session and select it. Returns its id.
    pub fn create(&self) -> u64 {
        let mut id = 0;
        mutate(|st| {
            st.seq += 1;
            id = st.seq;
            st.sessions.push(Session { id, name: String::new(), msgs: Vec::new() });
            st.active = id;
        });
        id
    }

    /// Switch the active session (no-op for unknown ids).
    pub fn select(&self, id: u64) {
        mutate(|st| {
            if st.sessions.iter().any(|s| s.id == id) {
                st.active = id;
            }
        });
    }

    /// Rename a session (trimmed).
    pub fn rename(&self, id: u64, name: &str) {
        let name = name.trim().to_string();
        mutate(|st| {
            if let Some(s) = st.sessions.iter_mut().find(|s| s.id == id) {
                s.name = name;
            }
        });
    }

    /// Delete a session; always keeps at least one alive and a valid `active`.
    pub fn delete(&self, id: u64) {
        mutate(|st| {
            st.sessions.retain(|s| s.id != id);
            if st.sessions.is_empty() {
                st.seq += 1;
                let nid = st.seq;
                st.sessions.push(Session { id: nid, name: String::new(), msgs: Vec::new() });
                st.active = nid;
            } else if st.active == id {
                st.active = st.sessions[0].id;
            }
        });
    }

    /// Append a message; auto-names the session on its first user message.
    pub fn push_message(&self, id: u64, role: &str, text: &str) {
        let role = role.to_string();
        let text = text.to_string();
        mutate(|st| {
            if let Some(s) = st.sessions.iter_mut().find(|s| s.id == id) {
                if s.name.is_empty() && role == "you" {
                    s.name = auto_name(&text);
                }
                s.msgs.push(StoredMsg { role, text });
            }
        });
    }

    /// Replace a session's transcript wholesale; auto-names if still unnamed.
    pub fn set_messages(&self, id: u64, msgs: Vec<StoredMsg>) {
        mutate(|st| {
            if let Some(s) = st.sessions.iter_mut().find(|s| s.id == id) {
                if s.name.is_empty() {
                    if let Some(first) = msgs.iter().find(|m| m.role == "you") {
                        s.name = auto_name(&first.text);
                    }
                }
                s.msgs = msgs;
            }
        });
    }
}

// ── UI: SessionBar dropdown ──────────────────────────────────────────────────

/// In-app session switcher: a themed dropdown showing all sessions with the
/// current one highlighted, a "+ New" action, and per-row rename/delete.
#[component]
pub fn SessionBar() -> Element {
    let sessions = use_sessions();
    let mut open = use_signal(|| false);
    let menu_for = use_signal(|| None::<u64>);
    let renaming = use_signal(|| None::<u64>);
    let rename_buf = use_signal(String::new);

    let list = sessions.list();
    let active = sessions.active_id();
    let current_title = sessions.active().map(|s| s.title()).unwrap_or_else(|| "New chat".to_string());

    rsx! {
        div { class: "sessionbar",
            button {
                class: "session-current",
                onclick: move |_| {
                    let v = !open();
                    open.set(v);
                },
                span { class: "session-dot" }
                span { class: "session-title", "{current_title}" }
                span { class: "session-chevron", if open() { "▴" } else { "▾" } }
            }
            if open() {
                div { class: "session-menu",
                    button {
                        class: "session-new",
                        onclick: move |_| {
                            sessions.create();
                            open.set(false);
                        },
                        span { class: "session-plus", "+" }
                        "New session"
                    }
                    div { class: "session-list",
                        for s in list {
                            SessionRow {
                                key: "{s.id}",
                                session: s,
                                active_id: active,
                                open,
                                menu_for,
                                renaming,
                                rename_buf,
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SessionRow(
    session: Session,
    active_id: u64,
    open: Signal<bool>,
    menu_for: Signal<Option<u64>>,
    renaming: Signal<Option<u64>>,
    rename_buf: Signal<String>,
) -> Element {
    // Signals are Copy; rebind mutably so we can `.set()` inside handlers.
    let mut open = open;
    let mut menu_for = menu_for;
    let mut renaming = renaming;
    let mut rename_buf = rename_buf;

    let sessions = use_sessions();
    let id = session.id;
    let title = session.title();
    let title_for_rename = title.clone();
    let count = session.msgs.len();
    let is_active = id == active_id;
    let is_menu = menu_for() == Some(id);
    let is_rename = renaming() == Some(id);

    rsx! {
        div { class: if is_active { "session-row active" } else { "session-row" },
            if is_rename {
                input {
                    class: "session-rename",
                    value: "{rename_buf}",
                    oninput: move |e| rename_buf.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            sessions.rename(id, &rename_buf());
                            renaming.set(None);
                        } else if e.key() == Key::Escape {
                            renaming.set(None);
                        }
                    },
                }
                button {
                    class: "session-ok",
                    onclick: move |_| {
                        sessions.rename(id, &rename_buf());
                        renaming.set(None);
                    },
                    "✓"
                }
            } else {
                button {
                    class: "session-pick",
                    onclick: move |_| {
                        sessions.select(id);
                        open.set(false);
                        menu_for.set(None);
                    },
                    span { class: "session-row-title", "{title}" }
                    span { class: "session-count", "{count}" }
                }
                button {
                    class: "session-more",
                    onclick: move |_| {
                        let next = if menu_for() == Some(id) { None } else { Some(id) };
                        menu_for.set(next);
                    },
                    "⋯"
                }
            }
            if is_menu && !is_rename {
                div { class: "session-actions",
                    button {
                        class: "session-action",
                        onclick: move |_| {
                            rename_buf.set(title_for_rename.clone());
                            renaming.set(Some(id));
                            menu_for.set(None);
                        },
                        "Rename"
                    }
                    button {
                        class: "session-action danger",
                        onclick: move |_| {
                            sessions.delete(id);
                            menu_for.set(None);
                        },
                        "Delete"
                    }
                }
            }
        }
    }
}

// ── Persistence (native = std::fs JSON; wasm = no-op) ─────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn data_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Ok(dir) = std::env::var("SMARTAGENT_OS_DATA") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("sessions.json"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return Some(PathBuf::from(home).join(".smartagent-os").join("sessions.json"));
        }
    }
    Some(PathBuf::from(".smartagent-os").join("sessions.json"))
}

#[cfg(not(target_arch = "wasm32"))]
fn read_from_disk() -> Option<Store> {
    let path = data_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    decode(&text)
}

#[cfg(not(target_arch = "wasm32"))]
fn persist(store: &Store) {
    if let Some(path) = data_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, encode(store));
    }
}

#[cfg(target_arch = "wasm32")]
fn read_from_disk() -> Option<Store> {
    None
}

#[cfg(target_arch = "wasm32")]
fn persist(_store: &Store) {}

// ── Hand-rolled JSON (encode + minimal parser) ───────────────────────────────

fn encode(store: &Store) -> String {
    let mut o = String::new();
    o.push('{');
    o.push_str("\"active\":");
    o.push_str(&store.active.to_string());
    o.push_str(",\"seq\":");
    o.push_str(&store.seq.to_string());
    o.push_str(",\"sessions\":[");
    for (i, s) in store.sessions.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"id\":");
        o.push_str(&s.id.to_string());
        o.push_str(",\"name\":\"");
        o.push_str(&esc(&s.name));
        o.push_str("\",\"msgs\":[");
        for (j, m) in s.msgs.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            o.push_str("{\"role\":\"");
            o.push_str(&esc(&m.role));
            o.push_str("\",\"text\":\"");
            o.push_str(&esc(&m.text));
            o.push_str("\"}");
        }
        o.push_str("]}");
    }
    o.push_str("]}");
    o
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o
}

/// Minimal parsed-JSON value.
enum J {
    Str(String),
    Num(String),
    Bool(bool),
    Null,
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn get(&self, key: &str) -> Option<&J> {
        if let J::Obj(m) = self {
            m.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        } else {
            None
        }
    }
    fn as_u64(&self) -> Option<u64> {
        if let J::Num(s) = self {
            s.parse::<f64>().ok().map(|f| f as u64)
        } else {
            None
        }
    }
    fn as_string(&self) -> Option<String> {
        if let J::Str(s) = self {
            Some(s.clone())
        } else {
            None
        }
    }
}

fn decode(text: &str) -> Option<Store> {
    let root = Parser::new(text).value()?;
    let active = root.get("active").and_then(J::as_u64).unwrap_or(0);
    let seq = root.get("seq").and_then(J::as_u64).unwrap_or(0);
    let mut sessions = Vec::new();
    if let Some(J::Arr(items)) = root.get("sessions") {
        for it in items {
            let id = it.get("id").and_then(J::as_u64).unwrap_or(0);
            if id == 0 {
                continue;
            }
            let name = it.get("name").and_then(J::as_string).unwrap_or_default();
            let mut msgs = Vec::new();
            if let Some(J::Arr(ms)) = it.get("msgs") {
                for m in ms {
                    let role = m.get("role").and_then(J::as_string).unwrap_or_default();
                    let mtext = m.get("text").and_then(J::as_string).unwrap_or_default();
                    msgs.push(StoredMsg { role, text: mtext });
                }
            }
            sessions.push(Session { id, name, msgs });
        }
    }
    let mut seq = seq;
    let max_id = sessions.iter().map(|s| s.id).max().unwrap_or(0);
    if seq < max_id {
        seq = max_id;
    }
    Some(Store { sessions, active, seq })
}

struct Parser {
    b: Vec<char>,
    i: usize,
}

impl Parser {
    fn new(s: &str) -> Parser {
        Parser { b: s.chars().collect(), i: 0 }
    }
    fn peek(&self) -> Option<char> {
        self.b.get(self.i).copied()
    }
    fn ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.i += 1;
            } else {
                break;
            }
        }
    }
    fn value(&mut self) -> Option<J> {
        self.ws();
        match self.peek()? {
            '{' => self.object(),
            '[' => self.array(),
            '"' => Some(J::Str(self.string()?)),
            't' => {
                self.lit("true")?;
                Some(J::Bool(true))
            }
            'f' => {
                self.lit("false")?;
                Some(J::Bool(false))
            }
            'n' => {
                self.lit("null")?;
                Some(J::Null)
            }
            _ => self.number(),
        }
    }
    fn lit(&mut self, s: &str) -> Option<()> {
        for c in s.chars() {
            if self.peek()? != c {
                return None;
            }
            self.i += 1;
        }
        Some(())
    }
    fn string(&mut self) -> Option<String> {
        if self.peek()? != '"' {
            return None;
        }
        self.i += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '"' => return Some(out),
                '\\' => {
                    let e = self.peek()?;
                    self.i += 1;
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000C}'),
                        'u' => {
                            let mut code: u32 = 0;
                            for _ in 0..4 {
                                let h = self.peek()?;
                                self.i += 1;
                                code = code * 16 + h.to_digit(16)?;
                            }
                            if let Some(ch) = char::from_u32(code) {
                                out.push(ch);
                            }
                        }
                        other => out.push(other),
                    }
                }
                _ => out.push(c),
            }
        }
        None
    }
    fn number(&mut self) -> Option<J> {
        let start = self.i;
        while let Some(c) = self.peek() {
            if c == '-' || c == '+' || c == '.' || c == 'e' || c == 'E' || c.is_ascii_digit() {
                self.i += 1;
            } else {
                break;
            }
        }
        if self.i == start {
            return None;
        }
        Some(J::Num(self.b[start..self.i].iter().collect()))
    }
    fn array(&mut self) -> Option<J> {
        self.i += 1; // '['
        let mut v = Vec::new();
        self.ws();
        if self.peek()? == ']' {
            self.i += 1;
            return Some(J::Arr(v));
        }
        loop {
            let val = self.value()?;
            v.push(val);
            self.ws();
            match self.peek()? {
                ',' => self.i += 1,
                ']' => {
                    self.i += 1;
                    break;
                }
                _ => return None,
            }
        }
        Some(J::Arr(v))
    }
    fn object(&mut self) -> Option<J> {
        self.i += 1; // '{'
        let mut m = Vec::new();
        self.ws();
        if self.peek()? == '}' {
            self.i += 1;
            return Some(J::Obj(m));
        }
        loop {
            self.ws();
            let k = self.string()?;
            self.ws();
            if self.peek()? != ':' {
                return None;
            }
            self.i += 1;
            let val = self.value()?;
            m.push((k, val));
            self.ws();
            match self.peek()? {
                ',' => self.i += 1,
                '}' => {
                    self.i += 1;
                    break;
                }
                _ => return None,
            }
        }
        Some(J::Obj(m))
    }
}
