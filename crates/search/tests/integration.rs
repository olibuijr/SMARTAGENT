//! Mock SearXNG server via local TcpListener.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use search::searx::{self, Query};

#[test]
fn queries_mock_instance() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = sock.read(&mut buf);
        let body = r#"{"results":[{"title":"Rust","url":"https://rust-lang.org","content":"systems lang"}]}"#;
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
        sock.write_all(resp.as_bytes()).unwrap();
    });
    let instance = format!("http://127.0.0.1:{port}");
    let q = Query { instance: &instance, terms: "rust lang", engines: None, category: None, time_range: None, limit: 10 };
    let results = searx::search(&q).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Rust");
    assert_eq!(results[0].url, "https://rust-lang.org");
}
