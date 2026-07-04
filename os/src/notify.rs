//! Agent Notify — an in-app feed of the fleet's ntfy notifications (new module).
//!
//! The SMARTAGENT fleet already publishes notifications over **ntfy** (task
//! completion, agent-needs-input, scheduled reminders). This module subscribes
//! to that topic *while the app is open* and shows the items in-app, newest
//! first, with a small settings section (server + topic + enable toggle)
//! persisted on-device exactly like `sessions.rs`.
//!
//! ## Defaults (discovered in `config/smartagent.conf`)
//! - `ntfy_server = http://ntfy.sh`  → [`DEFAULT_SERVER`]
//! - fleet default topic `smartagent` (see `crates/schedule/src/cli.rs`, which
//!   resolves `ntfy_topic` / `NTFY_TOPIC` and falls back to `"smartagent"`, and
//!   the `notify` binary that publishes `--topic`) → [`DEFAULT_TOPIC`]
//!
//! ## Transport (why long-poll/stream over `std::net`, no new deps)
//! ntfy exposes a topic as a newline-delimited JSON stream at
//! `GET <server>/<topic>/json` (and a one-shot `?poll=1` snapshot). We open a
//! plain `std::net::TcpStream`, write a minimal HTTP/1.1 GET, and read the
//! response line-by-line on a worker thread — pushing each parsed message into
//! the UI over a `futures_channel` (the same shape `net.rs`/`chat.rs` use, so
//! the render thread never blocks). HTTP response headers and chunked-transfer
//! size lines are skipped naturally: only lines that start with `{` and look
//! like an ntfy `message` event become items.
//!
//! **TLS caveat:** `std` has no TLS, so only `http://` servers work here. The
//! fleet default (`http://ntfy.sh`) and any self-hosted `http://` ntfy are fine;
//! an `https://` server is reported as unsupported rather than mis-connected.
//!
//! ## Background push — documented follow-up
//! This feed only streams while the app is in the **foreground**. True push when
//! the app is closed needs a native Android foreground/`Firebase`-style service
//! subscribing to ntfy and posting system notifications — out of scope for the
//! webview UI.
//! TODO(orchestrator): native background push
#![allow(dead_code)]

use dioxus::prelude::*;
use futures_util::StreamExt;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

/// Fleet ntfy server (config `ntfy_server`). `std` has no TLS, so `http://` only.
pub const DEFAULT_SERVER: &str = "http://ntfy.sh";
/// Fleet default topic (`crates/schedule/src/cli.rs` falls back to this).
pub const DEFAULT_TOPIC: &str = "smartagent";

/// A cross-platform channel sender for parsed notifications.
type Tx = futures_channel::mpsc::UnboundedSender<Notif>;

// ── Model ────────────────────────────────────────────────────────────────────

/// One received notification (an ntfy `message` event).
#[derive(Clone, PartialEq, Debug)]
pub struct Notif {
    /// ntfy message id (used to de-dupe stream vs. poll overlap). May be empty.
    pub id: String,
    /// Optional title.
    pub title: String,
    /// Body text.
    pub message: String,
    /// Unix epoch seconds (0 = unknown).
    pub time: i64,
    /// ntfy priority 1..=5 (0 = unset, rendered as the default 3).
    pub priority: u8,
}

// ── Settings (persisted on-device, mirrors sessions.rs) ───────────────────────

/// On-device notification settings.
#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    pub server: String,
    pub topic: String,
    pub enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { server: DEFAULT_SERVER.into(), topic: DEFAULT_TOPIC.into(), enabled: true }
    }
}

/// Global reactive settings — lazily loaded from disk on first access.
static SETTINGS: GlobalSignal<Settings> = Signal::global(load_settings);

/// Apply a mutation and persist the result to disk (native).
fn mutate(f: impl FnOnce(&mut Settings)) {
    let snapshot = {
        let mut w = SETTINGS.write();
        f(&mut w);
        w.clone()
    };
    persist(&snapshot);
}

/// Copy handle over the settings store. Obtain via [`use_notify_settings`].
#[derive(Clone, Copy, PartialEq)]
pub struct NotifySettings;

/// Hook: get the shared notification-settings handle.
pub fn use_notify_settings() -> NotifySettings {
    NotifySettings
}

impl NotifySettings {
    /// Current settings (reactive read — subscribes the caller).
    pub fn get(&self) -> Settings {
        SETTINGS.read().clone()
    }
    pub fn set_server(&self, v: &str) {
        let v = v.trim().to_string();
        mutate(|s| s.server = v);
    }
    pub fn set_topic(&self, v: &str) {
        let v = v.trim().to_string();
        mutate(|s| s.topic = v);
    }
    pub fn set_enabled(&self, on: bool) {
        mutate(|s| s.enabled = on);
    }
}

// ── Public seam: one-shot poll (works even if streaming is fragile) ───────────

/// Fetch the latest cached notifications with a single HTTP GET (`?poll=1`).
/// Uses the current on-device settings. Newest first. Empty on wasm/no-TLS/error.
pub fn poll_once() -> Vec<Notif> {
    let s = SETTINGS.peek().clone();
    poll(&s.server, &s.topic)
}

/// One-shot poll of an explicit server/topic. Never blocks the UI thread — call
/// from a worker thread. Returns newest-first.
#[cfg(not(target_arch = "wasm32"))]
pub fn poll(server: &str, topic: &str) -> Vec<Notif> {
    http_poll(server, topic).unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
pub fn poll(_server: &str, _topic: &str) -> Vec<Notif> {
    Vec::new()
}

// ── UI ────────────────────────────────────────────────────────────────────────

/// In-app notification feed + settings. Streams the fleet's ntfy topic while the
/// app is open and lists items newest-first.
#[component]
pub fn Notifications() -> Element {
    let settings = use_notify_settings();
    let cur = settings.get();

    let mut feed = use_signal(Vec::<Notif>::new);
    let mut tx_sig = use_signal(|| None::<Tx>);
    let mut show_settings = use_signal(|| false);

    // A single long-lived channel: worker threads (which come and go as settings
    // change) all send here; this coroutine drains into the feed signal.
    use_coroutine(move |_: UnboundedReceiver<()>| async move {
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<Notif>();
        tx_sig.set(Some(tx));
        while let Some(n) = rx.next().await {
            feed.with_mut(|v| {
                // de-dupe by id (poll seed vs. live stream overlap)
                if !n.id.is_empty() && v.iter().any(|x| x.id == n.id) {
                    return;
                }
                v.insert(0, n);
                if v.len() > 100 {
                    v.truncate(100);
                }
            });
        }
    });

    // (Re)spawn the streaming worker whenever server/topic/enabled changes.
    // Native only — wasm has no threads/sockets; the UI + `poll` seam remain.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut cancel = use_signal(|| Arc::new(AtomicBool::new(false)));
        use_effect(move || {
            let s = SETTINGS.read().clone(); // reactive: re-run on any change
            let maybe_tx = tx_sig.read().clone(); // reactive: run once tx is ready
            let Some(tx) = maybe_tx else { return };
            // Signal the previous worker to stop, then install a fresh flag.
            cancel.peek().store(true, Ordering::Relaxed);
            let flag = Arc::new(AtomicBool::new(false));
            cancel.set(flag.clone());
            if s.enabled && !s.topic.trim().is_empty() {
                spawn_stream(s.server.clone(), s.topic.clone(), tx, flag);
            }
        });
    }

    let refresh = move |_| {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(tx) = tx_sig.read().clone() {
            let s = use_notify_settings().get();
            std::thread::spawn(move || {
                for n in poll(&s.server, &s.topic) {
                    let _ = tx.unbounded_send(n);
                }
            });
        }
    };

    let items = feed();
    let count = items.len();

    rsx! {
        div { class: "notify",
            div { class: "nf-head",
                div { class: "nf-title",
                    span { class: "nf-logo", "◈" }
                    span { "Agent Notify" }
                    if count > 0 {
                        span { class: "nf-count", "{count}" }
                    }
                }
                div { class: "nf-actions",
                    span {
                        class: if cur.enabled { "nf-dot live" } else { "nf-dot off" },
                        title: if cur.enabled { "streaming (foreground)" } else { "disabled" },
                    }
                    button { class: "nf-btn", onclick: refresh, "Refresh" }
                    button {
                        class: "nf-btn",
                        onclick: move |_| feed.set(Vec::new()),
                        "Clear"
                    }
                    button {
                        class: "nf-btn",
                        onclick: move |_| {
                            let v = !show_settings();
                            show_settings.set(v);
                        },
                        "⚙"
                    }
                }
            }

            // Background-push follow-up note — always visible so it's not lost.
            div { class: "nf-note",
                "Live feed streams only while the app is open. Push when the app is "
                "closed needs a native Android background service (documented follow-up)."
            }

            if show_settings() {
                div { class: "nf-settings",
                    label { class: "nf-field",
                        span { class: "nf-label", "ntfy server" }
                        input {
                            class: "nf-input",
                            value: "{cur.server}",
                            placeholder: "{DEFAULT_SERVER}",
                            onchange: move |e| settings.set_server(&e.value()),
                        }
                    }
                    label { class: "nf-field",
                        span { class: "nf-label", "topic" }
                        input {
                            class: "nf-input",
                            value: "{cur.topic}",
                            placeholder: "{DEFAULT_TOPIC}",
                            onchange: move |e| settings.set_topic(&e.value()),
                        }
                    }
                    button {
                        class: if cur.enabled { "nf-toggle on" } else { "nf-toggle" },
                        onclick: move |_| settings.set_enabled(!cur.enabled),
                        span { class: "nf-knob" }
                        span { class: "nf-toggle-txt", if cur.enabled { "Streaming ON" } else { "Streaming OFF" } }
                    }
                    p { class: "nf-hint",
                        "std has no TLS — use an http:// server. Subscribes to "
                        "{cur.server}/{cur.topic}/json"
                    }
                }
            }

            div { class: "nf-list",
                if items.is_empty() {
                    div { class: "nf-empty",
                        p { "No notifications yet." }
                        p { class: "nf-empty-sub", "Fleet alerts will appear here while the app is open." }
                    }
                }
                for (i, n) in items.iter().enumerate() {
                    NotifRow { key: "{i}-{n.id}", notif: n.clone() }
                }
            }
        }
    }
}

#[component]
fn NotifRow(notif: Notif) -> Element {
    let prio = if notif.priority == 0 { 3 } else { notif.priority };
    let (plabel, pclass) = match prio {
        5 => ("MAX", "p-hi"),
        4 => ("HIGH", "p-hi"),
        2 => ("LOW", "p-lo"),
        1 => ("MIN", "p-lo"),
        _ => ("", "p-mid"),
    };
    let when = rel_time(notif.time);
    let title = if notif.title.is_empty() { String::new() } else { notif.title.clone() };

    rsx! {
        div { class: "nf-item",
            div { class: "nf-item-head",
                span { class: format!("nf-prio {pclass}") }
                if !title.is_empty() {
                    span { class: "nf-item-title", "{title}" }
                }
                span { class: "nf-item-time", "{when}" }
                if !plabel.is_empty() {
                    span { class: "nf-item-badge {pclass}", "{plabel}" }
                }
            }
            div { class: "nf-item-msg", "{notif.message}" }
        }
    }
}

/// Human-relative time from an epoch-seconds timestamp (0 = unknown).
fn rel_time(epoch: i64) -> String {
    if epoch <= 0 {
        return String::new();
    }
    let now = now_secs();
    if now <= 0 {
        return String::new();
    }
    let d = now - epoch;
    if d < 0 {
        return "now".into();
    }
    if d < 60 {
        "just now".into()
    } else if d < 3600 {
        format!("{}m ago", d / 60)
    } else if d < 86_400 {
        format!("{}h ago", d / 3600)
    } else {
        format!("{}d ago", d / 86_400)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

// SystemTime::now() panics on wasm32-unknown-unknown; skip relative time there.
#[cfg(target_arch = "wasm32")]
fn now_secs() -> i64 {
    0
}

// ── Networking (native): minimal HTTP over std::net::TcpStream ────────────────

#[cfg(not(target_arch = "wasm32"))]
fn spawn_stream(server: String, topic: String, tx: Tx, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        // Reconnect loop: stream, and on drop/idle back off (checking cancel).
        while !cancel.load(Ordering::Relaxed) {
            let _ = stream_once(&server, &topic, &tx, &cancel);
            for _ in 0..20 {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    });
}

/// Open one streaming connection and forward `message` events until it closes or
/// is cancelled. `since=2h` backfills recent items, then keeps streaming live.
#[cfg(not(target_arch = "wasm32"))]
fn stream_once(server: &str, topic: &str, tx: &Tx, cancel: &Arc<AtomicBool>) -> Option<()> {
    use std::io::{ErrorKind, Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let (https, host, port) = parse_url(server)?;
    if https {
        return None; // std has no TLS — see module note.
    }
    let addr = resolve(&host, port)?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(4)).ok()?;
    // Short read timeout so we can poll the cancel flag between reads.
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut w = stream.try_clone().ok()?;
    let req = http_get(&host, &format!("/{}/json?since=2h", topic_path(topic)), true);
    w.write_all(req.as_bytes()).ok()?;
    w.flush().ok()?;

    let mut s = stream;
    let mut acc: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Some(());
        }
        match s.read(&mut chunk) {
            Ok(0) => return Some(()), // server closed → reconnect
            Ok(n) => {
                acc.extend_from_slice(&chunk[..n]);
                drain_lines(&mut acc, &mut |line| {
                    if let Some(nf) = parse_message_line(line) {
                        let _ = tx.unbounded_send(nf);
                    }
                });
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                continue; // idle tick — loop back to check cancel
            }
            Err(_) => return Some(()),
        }
    }
}

/// One-shot snapshot via `?poll=1`. Returns items newest-first.
#[cfg(not(target_arch = "wasm32"))]
fn http_poll(server: &str, topic: &str) -> Option<Vec<Notif>> {
    use std::io::{ErrorKind, Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let (https, host, port) = parse_url(server)?;
    if https {
        return None;
    }
    let addr = resolve(&host, port)?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(4)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(8))).ok();
    let mut w = s.try_clone().ok()?;
    let req = http_get(&host, &format!("/{}/json?poll=1&since=24h", topic_path(topic)), false);
    w.write_all(req.as_bytes()).ok()?;
    w.flush().ok()?;

    let mut acc: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut out: Vec<Notif> = Vec::new();
    loop {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                acc.extend_from_slice(&chunk[..n]);
                drain_lines(&mut acc, &mut |line| {
                    if let Some(nf) = parse_message_line(line) {
                        out.push(nf);
                    }
                });
            }
            // Connection: close means we read to EOF; a timeout means "done".
            Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => break,
            Err(_) => break,
        }
    }
    out.reverse(); // ntfy returns oldest→newest; we want newest first
    Some(out)
}

/// Build a minimal HTTP/1.1 GET request line + headers.
#[cfg(not(target_arch = "wasm32"))]
fn http_get(host: &str, path: &str, keep_alive: bool) -> String {
    let conn = if keep_alive { "keep-alive" } else { "close" };
    format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: smartagent-os\r\n\
         Accept: application/x-ndjson\r\nConnection: {conn}\r\n\r\n"
    )
}

/// Resolve host:port to a single socket address.
#[cfg(not(target_arch = "wasm32"))]
fn resolve(host: &str, port: u16) -> Option<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;
    (host, port).to_socket_addrs().ok()?.next()
}

/// Sanitise a topic into a single path segment (ntfy topics have no '/').
fn topic_path(topic: &str) -> String {
    topic.trim().split('/').next().unwrap_or("").to_string()
}

/// Split accumulated bytes into complete `\n`-terminated lines, invoking `f` on
/// each trimmed line. Caps the buffer so a pathological giant line can't grow it
/// without bound.
fn drain_lines(acc: &mut Vec<u8>, f: &mut impl FnMut(&str)) {
    while let Some(pos) = acc.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = acc.drain(..=pos).collect();
        let s = String::from_utf8_lossy(&line);
        f(s.trim());
    }
    if acc.len() > (1 << 20) {
        acc.clear();
    }
}

/// Parse one ntfy JSON stream line into a [`Notif`], or `None` if it isn't a
/// `message` event (skips HTTP headers, chunk-size lines, `open`/`keepalive`).
fn parse_message_line(line: &str) -> Option<Notif> {
    if !line.starts_with('{') {
        return None;
    }
    // If an event field is present it must be "message"; absent = treat as one.
    if let Some(ev) = field_str(line, "event") {
        if ev != "message" {
            return None;
        }
    }
    let message = field_str(line, "message")?;
    let id = field_str(line, "id").unwrap_or_default();
    let title = field_str(line, "title").unwrap_or_default();
    let time = field_num(line, "time").unwrap_or(0.0) as i64;
    let priority = field_num(line, "priority").unwrap_or(0.0) as u8;
    Some(Notif { id, title, message, time, priority })
}

/// Extract a top-level JSON string field's (unescaped) value from a flat line.
/// Byte-accumulating so multi-byte UTF-8 in the value survives (see net.rs).
fn field_str(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = line.find(&needle)? + needle.len();
    let bytes = line.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => {
                match bytes[i + 1] {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'"' => out.push(b'"'),
                    b'\\' => out.push(b'\\'),
                    b'/' => out.push(b'/'),
                    other => out.push(other),
                }
                i += 2;
            }
            b'"' => return Some(String::from_utf8_lossy(&out).into_owned()),
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// Extract a top-level numeric JSON field's value from a flat line.
fn field_num(line: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{key}\":");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    // Numbers can't validly follow with a quote; guard against `"key":"..."`.
    if rest.starts_with('"') {
        return None;
    }
    let end = rest.find(|c: char| c == ',' || c == '}' || c == ']').unwrap_or(rest.len());
    rest[..end].trim().parse::<f64>().ok()
}

/// Parse a server URL into `(is_https, host, port)`. Bare host defaults to http.
fn parse_url(server: &str) -> Option<(bool, String, u16)> {
    let (https, rest) = if let Some(r) = server.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = server.strip_prefix("http://") {
        (false, r)
    } else {
        (false, server)
    };
    let hostport = rest.trim_end_matches('/').split('/').next()?;
    if hostport.is_empty() {
        return None;
    }
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None => (hostport.to_string(), if https { 443 } else { 80 }),
    };
    Some((https, host, port))
}

// ── Persistence (native = std::fs JSON; wasm = no-op), mirrors sessions.rs ─────

fn load_settings() -> Settings {
    read_settings_disk().unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn data_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Ok(dir) = std::env::var("SMARTAGENT_OS_DATA") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("notify.json"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return Some(PathBuf::from(home).join(".smartagent-os").join("notify.json"));
        }
    }
    Some(PathBuf::from(".smartagent-os").join("notify.json"))
}

#[cfg(not(target_arch = "wasm32"))]
fn read_settings_disk() -> Option<Settings> {
    let path = data_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    decode_settings(&text)
}

#[cfg(not(target_arch = "wasm32"))]
fn persist(s: &Settings) {
    if let Some(path) = data_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, encode_settings(s));
    }
}

#[cfg(target_arch = "wasm32")]
fn read_settings_disk() -> Option<Settings> {
    None
}

#[cfg(target_arch = "wasm32")]
fn persist(_s: &Settings) {}

fn encode_settings(s: &Settings) -> String {
    format!(
        "{{\"server\":\"{}\",\"topic\":\"{}\",\"enabled\":{}}}",
        jesc(&s.server),
        jesc(&s.topic),
        s.enabled
    )
}

fn decode_settings(text: &str) -> Option<Settings> {
    let server = field_str(text, "server").filter(|x| !x.is_empty()).unwrap_or_else(|| DEFAULT_SERVER.into());
    let topic = field_str(text, "topic").filter(|x| !x.is_empty()).unwrap_or_else(|| DEFAULT_TOPIC.into());
    // Default enabled unless explicitly false.
    let enabled = !text.contains("\"enabled\":false");
    Some(Settings { server, topic, enabled })
}

fn jesc(s: &str) -> String {
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
