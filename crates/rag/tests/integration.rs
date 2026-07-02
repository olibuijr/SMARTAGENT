use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn embedding_server(requests: usize) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for _ in 0..requests {
            let (mut sock, _) = listener.accept().unwrap();
            let req = read_http_request(&mut sock);
            let vec = if req.to_ascii_lowercase().contains("golf")
                || req.to_ascii_lowercase().contains("sports")
            {
                "[1.0,0.0]"
            } else {
                "[0.0,1.0]"
            };
            let body = format!(r#"{{"data":[{{"embedding":{vec}}}]}}"#);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            sock.write_all(resp.as_bytes()).unwrap();
        }
    });
    port
}

fn read_http_request(sock: &mut std::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = sock.read(&mut tmp).unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(header_end) = find_header_end(&buf) {
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
    String::from_utf8_lossy(&buf).into_owned()
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

#[test]
fn ingests_and_retrieves_cited_chunks() {
    let root = scratch("rag-e2e");
    let db = root.join("docs.semdb");
    let doc = root.join("guide.txt");
    std::fs::write(
        &doc,
        "Golf swing tempo helps putting accuracy.\nPasta sauce uses basil tomatoes.",
    )
    .unwrap();
    let port = embedding_server(3);
    let endpoint = format!("127.0.0.1:{port}");

    let out = rag::cli::run(&vec![
        "ingest".into(),
        db.display().to_string(),
        doc.display().to_string(),
        "--doc-id".into(),
        "guide".into(),
        "--chunk-tokens".into(),
        "6".into(),
        "--endpoint".into(),
        endpoint.clone(),
    ])
    .unwrap();
    assert!(out.contains("ingested 2 chunks"), "{out}");

    let got = rag::cli::run(&vec![
        "retrieve".into(),
        db.display().to_string(),
        "--text".into(),
        "sports".into(),
        "--k".into(),
        "1".into(),
        "--exact".into(),
        "--endpoint".into(),
        endpoint,
    ])
    .unwrap();
    assert!(got.contains("[ID:guide:000000]"), "{got}");
    assert!(got.contains("Golf swing tempo"), "{got}");
}

#[test]
fn chunks_pdf_text() {
    let root = scratch("rag-pdf");
    let pdf = root.join("plain.pdf");
    std::fs::write(
        &pdf,
        b"%PDF-1.1\n1 0 obj\n<<>>stream\nBT (Alpha PDF text) Tj <42657461> Tj ET\nendstream\n%%EOF",
    )
    .unwrap();
    let out = rag::cli::run(&vec![
        "chunk".into(),
        pdf.display().to_string(),
        "--doc-id".into(),
        "pdfdoc".into(),
        "--kind".into(),
        "pdf".into(),
    ])
    .unwrap();
    assert!(out.contains("Alpha PDF text"), "{out}");
    assert!(out.contains("Beta"), "{out}");
}
