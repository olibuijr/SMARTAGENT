//! Integration: GET against a real local socket served by the test itself.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use httpc::{get, post_json};

fn serve_once(response: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        read_http_request(&mut sock);
        sock.write_all(response.as_bytes()).unwrap();
    });
    port
}

fn read_http_request(sock: &mut std::net::TcpStream) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = sock.read(&mut tmp).unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..header_end]);
            let len = head
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length:"))
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if buf.len() >= header_end + 4 + len {
                break;
            }
        }
    }
}

#[test]
fn gets_plain_body() {
    let port = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let r = get(&format!("http://127.0.0.1:{port}/x")).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.text().unwrap(), "ok");
}

#[test]
fn gets_chunked_body() {
    let port = serve_once(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n",
    );
    let r = get(&format!("http://127.0.0.1:{port}/")).unwrap();
    assert_eq!(r.text().unwrap(), "Wikipedia");
}

#[test]
fn follows_redirect() {
    let target = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ndone");
    // Redirect server points at target server.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = sock.read(&mut buf);
        let resp = format!(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{target}/final\r\nContent-Length: 0\r\n\r\n"
        );
        sock.write_all(resp.as_bytes()).unwrap();
    });
    let r = get(&format!("http://127.0.0.1:{port}/start")).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.text().unwrap(), "done");
}

#[test]
fn posts_json_and_parses() {
    let port = serve_once(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\n\r\n{\"echo\":\"pong\"}",
    );
    let v = post_json(&format!("http://127.0.0.1:{port}/api"), r#"{"ping":true}"#).unwrap();
    assert_eq!(v.get("echo").unwrap().as_str().unwrap(), "pong");
}
