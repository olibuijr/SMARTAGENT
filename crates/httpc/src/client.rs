//! HTTP/1.1 client over std::net::TcpStream: GET/POST/PUT, headers, timeouts,
//! Content-Length and chunked bodies, redirects (limit 5). Plain HTTP only.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::json::{self, Value};
use crate::url::Url;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub timeout_secs: u64,
    pub follow_redirects: bool,
}

impl Request {
    pub fn new(method: &str, url: &str) -> Request {
        Request {
            method: method.to_uppercase(),
            url: url.to_string(),
            headers: Vec::new(),
            body: Vec::new(),
            timeout_secs: 60,
            follow_redirects: true,
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Request {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn body(mut self, data: &[u8]) -> Request {
        self.body = data.to_vec();
        self
    }

    pub fn timeout(mut self, secs: u64) -> Request {
        self.timeout_secs = secs;
        self
    }

    pub fn send(&self) -> Result<Response, String> {
        let mut url = self.url.clone();
        for _ in 0..=5 {
            let resp = send_once(self, &url)?;
            if self.follow_redirects && matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
                if let Some(loc) = resp.header("location") {
                    url = if loc.starts_with("http") {
                        loc.to_string()
                    } else {
                        // Relative redirect: keep host, replace path.
                        let u = Url::parse(&url)?;
                        format!("http://{}:{}{}", u.host, u.port, loc)
                    };
                    continue;
                }
            }
            return Ok(resp);
        }
        Err("too many redirects (limit 5)".into())
    }
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        let lower = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == lower)
            .map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> Result<String, String> {
        String::from_utf8(self.body.clone()).map_err(|_| "non-utf8 body".into())
    }

    pub fn json(&self) -> Result<Value, String> {
        json::parse(self.text()?.trim()).map_err(|e| format!("bad JSON: {e}"))
    }

    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

fn send_once(req: &Request, url: &str) -> Result<Response, String> {
    let u = Url::parse(url)?;
    if u.scheme != "http" {
        return Err("https not supported directly — route via local proxy".into());
    }
    let addr = format!("{}:{}", u.host, u.port);
    let mut stream = TcpStream::connect(&addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(req.timeout_secs)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(req.timeout_secs.min(30))))
        .map_err(|e| e.to_string())?;

    let mut head = format!("{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n", req.method, u.path, u.host);
    let mut has_len = false;
    for (k, v) in &req.headers {
        if k.to_ascii_lowercase() == "content-length" {
            has_len = true;
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if !req.body.is_empty() && !has_len {
        head.push_str(&format!("Content-Length: {}\r\n", req.body.len()));
    }
    head.push_str("\r\n");

    stream.write_all(head.as_bytes()).map_err(|e| format!("send: {e}"))?;
    if !req.body.is_empty() {
        stream.write_all(&req.body).map_err(|e| format!("send body: {e}"))?;
    }

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| format!("recv: {e}"))?;
    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> Result<Response, String> {
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no header terminator")?;
    let head = std::str::from_utf8(&raw[..header_end]).map_err(|_| "non-utf8 headers")?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or("empty response")?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("bad status line: {status_line}"))?;

    let mut headers = Vec::new();
    let mut chunked = false;
    let mut content_length: Option<usize> = None;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_string();
            let v = v.trim().to_string();
            let kl = k.to_ascii_lowercase();
            if kl == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
                chunked = true;
            }
            if kl == "content-length" {
                content_length = v.parse().ok();
            }
            headers.push((k, v));
        }
    }

    let raw_body = &raw[header_end + 4..];
    let body = if chunked {
        dechunk(raw_body)?
    } else if let Some(len) = content_length {
        raw_body.get(..len).ok_or("truncated body")?.to_vec()
    } else {
        raw_body.to_vec()
    };
    Ok(Response { status, headers, body })
}

pub fn dechunk(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let line_end = data
            .get(pos..)
            .and_then(|d| d.windows(2).position(|w| w == b"\r\n"))
            .ok_or("bad chunk header")?
            + pos;
        let size_str = std::str::from_utf8(&data[pos..line_end]).map_err(|_| "bad chunk size")?;
        let size = usize::from_str_radix(size_str.trim().split(';').next().unwrap_or(""), 16)
            .map_err(|_| format!("bad chunk size '{size_str}'"))?;
        if size == 0 {
            return Ok(out);
        }
        let start = line_end + 2;
        if start + size > data.len() {
            return Err("truncated chunk".into());
        }
        out.extend_from_slice(&data[start..start + size]);
        pos = start + size + 2;
    }
}

/// Convenience: GET a URL.
pub fn get(url: &str) -> Result<Response, String> {
    Request::new("GET", url).send()
}

/// Convenience: POST a JSON string, returns parsed JSON.
pub fn post_json(url: &str, body: &str) -> Result<Value, String> {
    let resp = Request::new("POST", url)
        .header("Content-Type", "application/json")
        .body(body.as_bytes())
        .send()?;
    if !resp.ok() {
        return Err(format!(
            "HTTP {}: {}",
            resp.status,
            resp.text().unwrap_or_default().chars().take(200).collect::<String>()
        ));
    }
    resp.json()
}

/// Convenience: build a request (fluent).
pub fn request(method: &str, url: &str) -> Request {
    Request::new(method, url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dechunks() {
        assert_eq!(dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap(), b"Wikipedia");
    }

    #[test]
    fn parses_response_with_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX-A: b\r\n\r\nhelloEXTRA";
        let r = parse_response(raw).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"hello");
        assert_eq!(r.header("x-a"), Some("b"));
    }

    #[test]
    fn rejects_https_direct() {
        let e = Request::new("GET", "https://example.com").send().unwrap_err();
        assert!(e.contains("proxy"), "{e}");
    }
}
