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
    /// The turn finished.
    Done,
    /// Non-fatal info line from the gateway.
    Info(String),
    /// Connection/protocol error.
    Error(String),
}

#[cfg(not(target_arch = "wasm32"))]
pub fn ask(agent: &str, message: &str, tx: UnboundedSender<Ev>) {
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
            Some(("info", _)) => {
                // Status noise (auth ok, agent working/idle) — never shown in
                // the chat transcript.
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
