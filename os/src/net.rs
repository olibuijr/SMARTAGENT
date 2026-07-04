//! Network client for the fleet gateway TCP bridge.
//!
//! Native targets (mobile/desktop) open a real `std::net::TcpStream` to the
//! gateway (`gateway_tcp_addr`), send the auth line, then issue an `ask` op and
//! stream `{"ev":"text",...}` deltas back. Events are delivered to the UI over
//! a futures channel so a Dioxus task can `await` them and update signals — no
//! polling, no blocking the render thread. (Web/wasm will use a WebSocket or a
//! server function later; this path targets the phone first.)

use futures_channel::mpsc::UnboundedSender;

/// Dev default — the gateway TCP bridge on the LAN. Made configurable later.
pub const GATEWAY_ADDR: &str = "192.168.1.166:9330";
pub const GATEWAY_TOKEN: &str = "smartagent-os-dev";

/// One streamed reply's events.
#[derive(Clone, Debug)]
pub enum Ev {
    /// A text delta from the agent.
    Text(String),
    /// A thinking/reasoning delta ({"ev":"thinking"}).
    Thinking(String),
    /// The turn finished.
    Done,
    /// Non-fatal info line from the gateway — includes `🛠 <tool> …` tool
    /// status, which `blocks::parse_events` turns into tool cards.
    Info(String),
    /// Connection/protocol error.
    Error(String),
}

#[cfg(not(target_arch = "wasm32"))]
pub fn ask(agent: &str, message: &str, tx: UnboundedSender<Ev>) {
    // agent may be any fleet member (linus/ada/…/jeeves).
    let agent = agent.to_string();
    let message = message.to_string();
    std::thread::spawn(move || {
        if let Err(e) = run_ask(&agent, &message, &tx) {
            let _ = tx.unbounded_send(Ev::Error(e));
        }
        let _ = tx.unbounded_send(Ev::Done);
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn run_ask(agent: &str, message: &str, tx: &UnboundedSender<Ev>) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    let stream =
        TcpStream::connect(GATEWAY_ADDR).map_err(|e| format!("connect {GATEWAY_ADDR}: {e}"))?;
    let mut w = stream.try_clone().map_err(|e| e.to_string())?;
    // auth, then the streaming ask.
    let auth = format!("{{\"token\":\"{}\"}}\n", esc(GATEWAY_TOKEN));
    w.write_all(auth.as_bytes()).map_err(|e| e.to_string())?;
    let ask = format!(
        "{{\"op\":\"ask\",\"agent\":\"{}\",\"message\":\"{}\"}}\n",
        esc(agent),
        esc(message)
    );
    w.write_all(ask.as_bytes()).map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())?;

    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match event_kind(line) {
            Some(("text", data)) => {
                let _ = tx.unbounded_send(Ev::Text(data));
            }
            Some(("thinking", data)) => {
                let _ = tx.unbounded_send(Ev::Thinking(data));
            }
            Some(("info", data)) => {
                // Forward tool-status lines (`🛠 <tool> running…/✓/✗`) so the
                // blocks renderer can build tool cards; swallow the rest (auth
                // ok, agent working/idle) as noise.
                if data.starts_with('🛠') {
                    let _ = tx.unbounded_send(Ev::Info(data));
                }
            }
            Some(("done", _)) => break,
            _ => {}
        }
    }
    Ok(())
}

/// Minimal `{"ev":"<kind>","data":"<...>"}` extractor — avoids a JSON dep on the
/// hot streaming path. Returns (kind, unescaped-data).
fn event_kind(line: &str) -> Option<(&'static str, String)> {
    let ev = field(line, "ev")?;
    let kind = match ev.as_str() {
        "text" => "text",
        "data" => "data",
        "thinking" => "thinking",
        "info" => "info",
        "done" => "done",
        _ => return None,
    };
    let data = field(line, "data").unwrap_or_default();
    Some((kind, data))
}

/// Extract a top-level string field's value (unescaped) from a flat JSON line.
fn field(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = line.find(&needle)? + needle.len();
    let bytes = line.as_bytes();
    // Accumulate BYTES (not chars) so multi-byte UTF-8 in the value survives —
    // pushing `b as char` per byte mangled every non-ASCII glyph (the `…`
    // mojibake). Decode the collected bytes once at the end.
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

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
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

/// Blocking request/collect for a read-only data op (board/mail/projects/tree/
/// file/git). Connects, auths, sends `{"op":op,"path":path}`, returns every
/// `{"ev":"data",...}` line until `done`. Call from a worker thread (see
/// `request_async`), never on the UI thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn request(op: &str, path: &str) -> Vec<String> {
    run_request(op, path).unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn run_request(op: &str, path: &str) -> Option<Vec<String>> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::Duration;
    // Fast-fail connect + read timeout so an unreachable gateway can't wedge a
    // caller (the sync data seams fetch on mount).
    let addr = GATEWAY_ADDR.parse().ok()?;
    let s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(6))).ok();
    let mut w = s.try_clone().ok()?;
    w.write_all(format!("{{\"token\":\"{}\"}}\n", esc(GATEWAY_TOKEN)).as_bytes()).ok()?;
    w.write_all(format!("{{\"op\":\"{}\",\"path\":\"{}\"}}\n", esc(op), esc(path)).as_bytes()).ok()?;
    w.flush().ok()?;
    let mut out = Vec::new();
    for line in BufReader::new(s).lines() {
        let Ok(line) = line else { break };
        match event_kind(line.trim()) {
            Some(("data", d)) => out.push(d),
            Some(("done", _)) => break,
            _ => {}
        }
    }
    Some(out)
}

/// Async wrapper for Dioxus `use_resource`: runs the blocking request on a
/// worker thread and awaits the result, so the render thread never blocks.
#[cfg(not(target_arch = "wasm32"))]
pub async fn request_async(op: &'static str, path: String) -> Vec<String> {
    let (tx, rx) = futures_channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(request(op, &path));
    });
    rx.await.unwrap_or_default()
}

/// Blocking read-only tool invocation via the gateway generic `run` op:
/// `run_tool("memory", &["recall","golf"])` → the tool's stdout lines. Call
/// from a worker thread. Only read-only tool+verb pairs are allowed server-side.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_tool(tool: &str, args: &[&str]) -> Vec<String> {
    run_tool_inner(tool, args).unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn run_tool_inner(tool: &str, args: &[&str]) -> Option<Vec<String>> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::Duration;
    let addr = GATEWAY_ADDR.parse().ok()?;
    let s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut w = s.try_clone().ok()?;
    w.write_all(format!("{{\"token\":\"{}\"}}\n", esc(GATEWAY_TOKEN)).as_bytes()).ok()?;
    let args_json: String = args
        .iter()
        .map(|a| format!("\"{}\"", esc(a)))
        .collect::<Vec<_>>()
        .join(",");
    w.write_all(
        format!("{{\"op\":\"run\",\"tool\":\"{}\",\"args\":[{}]}}\n", esc(tool), args_json).as_bytes(),
    )
    .ok()?;
    w.flush().ok()?;
    let mut out = Vec::new();
    for line in BufReader::new(s).lines() {
        let Ok(line) = line else { break };
        match event_kind(line.trim()) {
            Some(("data", d)) => out.push(d),
            Some(("done", _)) => break,
            _ => {}
        }
    }
    Some(out)
}

/// Async wrapper for `use_resource`.
#[cfg(not(target_arch = "wasm32"))]
pub async fn run_tool_async(tool: &'static str, args: Vec<String>) -> Vec<String> {
    let (tx, rx) = futures_channel::oneshot::channel();
    std::thread::spawn(move || {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let _ = tx.send(run_tool(tool, &refs));
    });
    rx.await.unwrap_or_default()
}

/// Fleet roster + live status via the gateway `agents` op. Each line is a TSV:
/// `name \t state \t doing \t role \t busy_secs \t tokens \t tools \t words`.
#[cfg(not(target_arch = "wasm32"))]
pub fn agents() -> Vec<String> {
    agents_inner().unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn agents_inner() -> Option<Vec<String>> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::Duration;
    let addr = GATEWAY_ADDR.parse().ok()?;
    let s = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(6))).ok();
    let mut w = s.try_clone().ok()?;
    w.write_all(format!("{{\"token\":\"{}\"}}\n", esc(GATEWAY_TOKEN)).as_bytes()).ok()?;
    w.write_all(b"{\"op\":\"agents\"}\n").ok()?;
    w.flush().ok()?;
    let mut out = Vec::new();
    for line in BufReader::new(s).lines() {
        let Ok(line) = line else { break };
        let t = line.trim();
        match event_kind(t) {
            // agents op emits rows as {"ev":"info","data":"<tsv>"} then done.
            Some(("info", d)) if d != "auth ok" && d.contains('\t') => out.push(d),
            Some(("done", _)) => break,
            _ => {}
        }
    }
    Some(out)
}

/// Async wrapper for `use_resource`.
#[cfg(not(target_arch = "wasm32"))]
pub async fn agents_async() -> Vec<String> {
    let (tx, rx) = futures_channel::oneshot::channel();
    std::thread::spawn(move || { let _ = tx.send(agents()); });
    rx.await.unwrap_or_default()
}
