//! ntfy protocol: publishing is an HTTP/HTTPS POST/PUT to <server>/<topic>
//! with the message as body and metadata in headers.

pub struct Message {
    pub server: String,
    pub topic: String,
    pub message: String,
    pub title: Option<String>,
    pub priority: Option<String>,
    pub tags: Option<String>,
    /// URL opened when the notification is tapped (ntfy `Click` header).
    pub click: Option<String>,
    /// Render the body as markdown (ntfy `Markdown` header).
    pub markdown: bool,
    /// Bearer token for protected topics (from env, not argv).
    pub auth: Option<String>,
}

/// Reject CR/LF in header values — a `\r\n` in title/tags/topic would let the
/// caller inject arbitrary HTTP headers into the request.
fn clean(name: &str, v: &str) -> Result<(), String> {
    if v.contains('\r') || v.contains('\n') {
        return Err(format!("{name} must not contain newlines"));
    }
    Ok(())
}

pub fn send(m: &Message) -> Result<String, String> {
    if m.topic.is_empty() || m.topic.contains('/') {
        return Err("topic must be non-empty and contain no '/'".into());
    }
    clean("topic", &m.topic)?;
    if let Some(t) = &m.title {
        clean("title", t)?;
    }
    if let Some(t) = &m.tags {
        clean("tags", t)?;
    }
    let url = format!("{}/{}", m.server.trim_end_matches('/'), m.topic);
    let mut req = httpc::request("POST", &url)
        .body(m.message.as_bytes())
        .timeout(15);
    if let Some(t) = &m.title {
        req = req.header("Title", t);
    }
    if let Some(p) = &m.priority {
        if !matches!(p.as_str(), "1" | "2" | "3" | "4" | "5") {
            return Err("priority must be 1-5".into());
        }
        req = req.header("Priority", p);
    }
    if let Some(t) = &m.tags {
        req = req.header("Tags", t);
    }
    if let Some(c) = &m.click {
        clean("click", c)?;
        if !(c.starts_with("http://") || c.starts_with("https://")) {
            return Err("click must be an http(s) URL".into());
        }
        req = req.header("Click", c);
    }
    if m.markdown {
        req = req.header("Markdown", "yes");
    }
    if let Some(a) = &m.auth {
        clean("auth", a)?;
        req = req.header("Authorization", &format!("Bearer {a}"));
    }
    let resp = req.send()?;
    if resp.ok() {
        Ok(format!("sent to {url}"))
    } else {
        Err(format!(
            "HTTP {}: {}",
            resp.status,
            resp.text().unwrap_or_default()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn rejects_header_injection() {
        for (title, tags) in [
            (Some("x\r\nEvil: 1".to_string()), None),
            (None, Some("a\nb".to_string())),
        ] {
            let err = send(&Message {
                server: "http://127.0.0.1:1".into(),
                topic: "t".into(),
                message: "m".into(),
                title,
                priority: None,
                tags,
                click: None,
                markdown: false,
                auth: None,
            })
            .unwrap_err();
            assert!(err.contains("newlines"), "{err}");
        }
    }

    #[test]
    fn posts_message_with_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            // Drain the full request (headers + body) before responding, else
            // closing with unread data sends RST instead of FIN.
            let mut data = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = sock.read(&mut buf).unwrap();
                data.extend_from_slice(&buf[..n]);
                let req_so_far = String::from_utf8_lossy(&data);
                if req_so_far.contains("hello world") {
                    break;
                }
                if n == 0 {
                    break;
                }
            }
            let req = String::from_utf8_lossy(&data).to_string();
            sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                .unwrap();
            req
        });
        let m = Message {
            server: format!("http://127.0.0.1:{port}"),
            topic: "alerts".into(),
            message: "hello world".into(),
            title: Some("Test".into()),
            priority: Some("4".into()),
            tags: Some("tada".into()),
            click: None,
            markdown: false,
            auth: None,
        };
        let out = send(&m).unwrap();
        assert!(out.contains("/alerts"));
        let req = handle.join().unwrap();
        assert!(req.starts_with("POST /alerts"));
        assert!(req.contains("Title: Test"));
        assert!(req.contains("Priority: 4"));
        assert!(req.contains("hello world"));
    }

    #[test]
    fn rejects_bad_input() {
        let m = Message {
            server: "http://x".into(),
            topic: "a/b".into(),
            message: "m".into(),
            title: None,
            priority: None,
            tags: None,
            click: None,
            markdown: false,
            auth: None,
        };
        assert!(send(&m).is_err());
    }
}
