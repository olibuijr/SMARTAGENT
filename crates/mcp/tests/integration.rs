//! Fake stdio MCP server (a shell script) + HTTP mock exercising the client.
use mcp::{http::HttpClient, jsonrpc, stdio::StdioClient};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

/// Drain a full HTTP request (headers + Content-Length body) so closing the
/// socket sends FIN, not RST, avoiding a client-side connection reset.
fn read_full_request(sock: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = sock.read(&mut tmp).unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..pos]);
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("Content-Length:"))
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0);
            if buf.len() >= pos + 4 + len {
                break;
            }
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}

fn scratch() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn stdio_handshake_and_tools() {
    // A tiny awk-based server: reads JSON-RPC lines, replies by id.
    let script = scratch().join("fake-mcp.sh");
    std::fs::write(&script, r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*) echo '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26"}}' ;;
    *'"method":"tools/list"'*) echo '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"ping","description":"say pong"}]}}' ;;
    *'"method":"tools/call"'*) echo '{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"pong"}]}}' ;;
    *) : ;;  # notifications: no reply
  esac
done
"#).unwrap();
    let mut c = StdioClient::start(&format!("sh {}", script.display())).unwrap();
    let tools = jsonrpc::parse_tools(&c.call("tools/list", "{}").unwrap());
    assert_eq!(tools, vec![("ping".to_string(), "say pong".to_string())]);
    let out = jsonrpc::parse_call(
        &c.call("tools/call", r#"{"name":"ping","arguments":{}}"#)
            .unwrap(),
    );
    assert_eq!(out, "pong");
}

#[test]
fn http_notification_accepts_202() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for _ in 0..3 {
            let (mut sock, _) = listener.accept().unwrap();
            let req = read_full_request(&mut sock);
            let resp = if req.contains("\"id\"") {
                // request → JSON result
                let body = if req.contains("initialize") {
                    r#"{"jsonrpc":"2.0","id":1,"result":{}}"#
                } else {
                    r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"t","description":"d"}]}}"#
                };
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                )
            } else {
                // notification → 202 empty
                "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n".to_string()
            };
            sock.write_all(resp.as_bytes()).unwrap();
        }
    });
    let mut c = HttpClient::start(&format!("http://127.0.0.1:{port}/mcp")).unwrap();
    let tools = jsonrpc::parse_tools(&c.call("tools/list", "{}").unwrap());
    assert_eq!(tools[0].0, "t");
}

#[test]
fn http_start_times_out_when_server_stalls() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let _ = read_full_request(&mut sock);
        thread::sleep(Duration::from_secs(4));
    });

    let started = Instant::now();
    let err = match HttpClient::start_with_timeout(&format!("http://127.0.0.1:{port}/mcp"), 1) {
        Ok(_) => panic!("stalled server unexpectedly completed MCP handshake"),
        Err(e) => e,
    };
    assert!(started.elapsed() < Duration::from_secs(3), "timeout took too long: {err}");
    assert!(
        err.contains("timed out") || err.contains("WouldBlock") || err.contains("Resource temporarily unavailable"),
        "unexpected timeout error: {err}"
    );
}
