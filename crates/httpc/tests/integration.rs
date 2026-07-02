//! Integration: GET against a real local socket served by the test itself.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

#[test]
fn gets_https_body_with_custom_ca() {
    let scratch = test_scratch("https-custom-ca");
    let cert = scratch.join("cert.pem");
    let key = scratch.join("key.pem");
    let status = Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=127.0.0.1",
            "-addext",
            "subjectAltName=IP:127.0.0.1,DNS:localhost",
            "-days",
            "1",
            "-keyout",
            key.to_str().unwrap(),
            "-out",
            cert.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("openssl req must be available for HTTPS regression test");
    assert!(status.success(), "openssl req failed");

    let port = unused_local_port();
    let mut server = Command::new("openssl")
        .args([
            "s_server",
            "-quiet",
            "-accept",
            &format!("127.0.0.1:{port}"),
            "-cert",
            cert.to_str().unwrap(),
            "-key",
            key.to_str().unwrap(),
            "-www",
            "-naccept",
            "1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("openssl s_server must start for HTTPS regression test");

    let old_ca = std::env::var_os("SMARTAGENT_HTTPC_CA_FILE");
    std::env::set_var("SMARTAGENT_HTTPC_CA_FILE", &cert);
    let url = format!("https://127.0.0.1:{port}/");
    let mut result = Err("server did not become ready".to_string());
    for _ in 0..20 {
        match httpc::get(&url) {
            Ok(resp) => {
                result = Ok(resp);
                break;
            }
            Err(e) if e.contains("connect") || e.contains("unreachable") => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    match old_ca {
        Some(value) => std::env::set_var("SMARTAGENT_HTTPC_CA_FILE", value),
        None => std::env::remove_var("SMARTAGENT_HTTPC_CA_FILE"),
    }
    let _ = server.kill();
    let _ = server.wait();

    let resp = result.unwrap();
    assert_eq!(resp.status, 200);
    assert!(resp.text().unwrap().contains("s_server"));
}

fn unused_local_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn test_scratch(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch")
        .join(format!("{name}-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
