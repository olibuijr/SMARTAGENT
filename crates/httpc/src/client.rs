//! HTTP/1.1 client over std::net::TcpStream for HTTP and `openssl s_client` for
//! HTTPS: GET/POST/PUT, headers, timeouts, Content-Length and chunked bodies,
//! redirects (limit 5).

use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Duration;

use crate::json::{self, Value};
use crate::url::Url;

/// Hard ceiling on a single response (headers + body). Protects against
/// memory-exhaustion from a hostile or misconfigured server.
const MAX_RESPONSE_BYTES: usize = 256 * 1024 * 1024;


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
        // Work on a mutable copy so redirects can rewrite method/body/headers
        // per RFC 9110 without mutating the caller's Request.
        let mut req = self.clone();
        for _ in 0..=5 {
            let resp = send_once(&req, &req.url)?;
            if self.follow_redirects && matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
                if let Some(loc) = resp.header("location") {
                    let prev_host = Url::parse(&req.url).map(|u| u.host).unwrap_or_default();
                    let next_url = if loc.starts_with("http://") || loc.starts_with("https://") {
                        loc.to_string()
                    } else {
                        let u = Url::parse(&req.url)?;
                        let port = if u.port == default_port(&u.scheme) {
                            String::new()
                        } else {
                            format!(":{}", u.port)
                        };
                        format!("{}://{}{}{}", u.scheme, u.host, port, loc)
                    };
                    // 301/302/303 on a non-GET/HEAD become a bodyless GET
                    // (browser + RFC 9110 §15.4 behavior). 307/308 preserve
                    // method and body. Either way, drop Authorization when the
                    // host changes so credentials never leak cross-origin.
                    if matches!(resp.status, 301..=303)
                        && !matches!(req.method.as_str(), "GET" | "HEAD")
                    {
                        req.method = "GET".into();
                        req.body.clear();
                        req.headers.retain(|(k, _)| {
                            let k = k.to_ascii_lowercase();
                            k != "content-length" && k != "content-type"
                        });
                    }
                    let next_host = Url::parse(&next_url).map(|u| u.host).unwrap_or_default();
                    if next_host != prev_host {
                        req.headers
                            .retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
                    }
                    req.url = next_url;
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
    if u.scheme == "https" {
        return send_https(req, &u);
    }
    if u.scheme != "http" {
        return Err(format!("unsupported scheme '{}'", u.scheme));
    }
    let addr = format!("{}:{}", u.host, u.port);
    // connect_timeout: a blackholed address (e.g. VPN peer down) must fail fast,
    // not block the tool indefinitely — the 10s budget is shared across ALL
    // resolved addresses (wall-clock bound), not granted per address.
    let connect_budget = Duration::from_secs(req.timeout_secs.clamp(1, 10));
    let sock_addrs: Vec<std::net::SocketAddr> = (u.host.as_str(), u.port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve {addr}: {e}"))?
        .collect();
    let started = std::time::Instant::now();
    let mut stream = None;
    for sa in &sock_addrs {
        let remaining = connect_budget.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        if let Ok(s) = TcpStream::connect_timeout(sa, remaining) {
            stream = Some(s);
            break;
        }
    }
    let mut stream = stream.ok_or_else(|| {
        format!(
            "connect {addr}: unreachable within {}s",
            connect_budget.as_secs()
        )
    })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(req.timeout_secs)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(req.timeout_secs.min(30))))
        .map_err(|e| e.to_string())?;

    let wire = build_request_bytes(req, &u);
    stream.write_all(&wire).map_err(|e| format!("send: {e}"))?;

    let raw = read_response_bytes(&mut stream)?;
    parse_response(&raw)
}

fn send_https(req: &Request, u: &Url) -> Result<Response, String> {
    let addr = format!("{}:{}", u.host, u.port);
    let wire = build_request_bytes(req, u);
    let mut cmd = Command::new("timeout");
    cmd.arg(format!("{}s", req.timeout_secs.max(1)))
        .arg("openssl")
        .arg("s_client")
        .arg("-connect")
        .arg(&addr)
        .arg("-verify_return_error")
        .arg("-verify_quiet")
        .arg("-quiet")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(ca_file) = std::env::var_os("SMARTAGENT_HTTPC_CA_FILE") {
        if !ca_file.is_empty() {
            cmd.arg("-CAfile").arg(ca_file);
        }
    }
    if u.host.parse::<IpAddr>().is_ok() {
        cmd.arg("-verify_ip").arg(&u.host);
    } else {
        cmd.arg("-servername")
            .arg(&u.host)
            .arg("-verify_hostname")
            .arg(&u.host);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn timeout/openssl for https {addr}: {e}"))?;
    {
        let stdin = child.stdin.as_mut().ok_or("openssl stdin unavailable")?;
        stdin
            .write_all(&wire)
            .map_err(|e| format!("send tls request: {e}"))?;
    }
    drop(child.stdin.take());

    let mut stdout = child.stdout.take().ok_or("openssl stdout unavailable")?;
    let mut stderr = child.stderr.take();
    let raw_result = read_response_bytes(&mut stdout);

    // If framing told us the response is complete before the TLS process exits,
    // stop the helper so tests and tools never leak long-lived s_client children.
    let _ = child.kill();
    let status = child.wait().ok();
    let mut err = String::new();
    if let Some(mut stderr) = stderr.take() {
        let _ = stderr.read_to_string(&mut err);
    }

    let raw = raw_result.map_err(|e| format!("recv tls response from {addr}: {e}"))?;
    parse_response(&raw).map_err(|e| {
        format!(
            "tls response from {addr}: {e}{}",
            tls_error_suffix(status, &err)
        )
    })
}

fn default_port(scheme: &str) -> u16 {
    if scheme == "https" {
        443
    } else {
        80
    }
}

fn build_request_bytes(req: &Request, u: &Url) -> Vec<u8> {
    let host_hdr = if u.port == default_port(&u.scheme) {
        u.host.clone()
    } else {
        format!("{}:{}", u.host, u.port)
    };
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        req.method, u.path, host_hdr
    );
    let mut has_len = false;
    for (k, v) in &req.headers {
        let kl = k.to_ascii_lowercase();
        // Host and Connection are already written; skip caller copies so the
        // request can't carry duplicate headers.
        if kl == "host" || kl == "connection" {
            continue;
        }
        if kl == "content-length" {
            has_len = true;
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if !req.body.is_empty() && !has_len {
        head.push_str(&format!("Content-Length: {}\r\n", req.body.len()));
    }
    head.push_str("\r\n");

    let mut wire = head.into_bytes();
    wire.extend_from_slice(&req.body);
    wire
}

fn tls_error_suffix(status: Option<ExitStatus>, stderr: &str) -> String {
    let detail = stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("; ");
    match (status, detail.is_empty()) {
        (Some(status), false) => format!(" (openssl status: {status}; {detail})"),
        (Some(status), true) => format!(" (openssl status: {status})"),
        (None, false) => format!(" ({detail})"),
        (None, true) => String::new(),
    }
}

/// Read a response without relying on the server closing the socket
/// (Chrome's DevTools HTTP endpoint ignores `Connection: close`): stop as
/// soon as Content-Length is satisfied or the chunked body terminates.
fn read_response_bytes<R: Read>(stream: &mut R) -> Result<Vec<u8>, String> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    // Cache the header boundary + framing once found, so we don't re-scan the
    // whole (growing) buffer on every socket read.
    let mut framing: Option<(usize, Framing)> = None;
    loop {
        let n = stream.read(&mut buf).map_err(|e| format!("recv: {e}"))?;
        if n == 0 {
            return Ok(raw); // EOF — server closed
        }
        raw.extend_from_slice(&buf[..n]);
        // Total-size ceiling: a hostile server advertising a huge Content-Length,
        // an endless chunked stream, or an UntilClose body that never closes
        // would otherwise grow `raw` until the process OOMs.
        if raw.len() > MAX_RESPONSE_BYTES {
            return Err(format!(
                "response exceeds {}MB cap",
                MAX_RESPONSE_BYTES / (1024 * 1024)
            ));
        }

        if framing.is_none() {
            if let Some(he) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = std::str::from_utf8(&raw[..he]).unwrap_or("");
                framing = Some((he, detect_framing(head)));
            } else if raw.len() > 64 * 1024 {
                // Header block never terminated — a hostile server could
                // otherwise grow memory unboundedly before framing kicks in.
                return Err("response headers exceed 64KB".into());
            }
        }
        let Some((header_end, ref f)) = framing else {
            continue;
        };
        let body = &raw[header_end + 4..];
        match f {
            // Chunked: done when the terminating 0-size chunk has arrived.
            // Only attempt the (O(body)) dechunk parse when the tail can
            // plausibly hold the terminator — a full parse per socket read
            // made large chunked bodies O(n²).
            Framing::Chunked => {
                if body.ends_with(b"\r\n\r\n") && dechunk(body).is_ok() {
                    return Ok(raw);
                }
            }
            Framing::Length(len) => {
                if body.len() >= *len {
                    return Ok(raw);
                }
            }
            // No length info: read until the server closes (EOF above).
            Framing::UntilClose => {}
        }
    }
}

enum Framing {
    Chunked,
    Length(usize),
    UntilClose,
}

/// Determine body framing from a parsed header block, matching per-header (not a
/// substring over the whole block, which could mix a `Transfer-Encoding` header
/// with the word "chunked" appearing in some other header's value).
fn detect_framing(head: &str) -> Framing {
    for line in head.split("\r\n").skip(1) {
        if let Some((k, v)) = line.split_once(':') {
            let kl = k.trim().to_ascii_lowercase();
            if kl == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
                return Framing::Chunked;
            }
            if kl == "content-length" {
                if let Ok(n) = v.trim().parse::<usize>() {
                    return Framing::Length(n);
                }
            }
        }
    }
    Framing::UntilClose
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
    Ok(Response {
        status,
        headers,
        body,
    })
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
        // Checked arithmetic: `size` is attacker-controlled hex (up to
        // usize::MAX). `start + size + 2` could overflow, wrap to a small value,
        // pass a naive bounds check, then panic on the out-of-range slice below.
        let end = start
            .checked_add(size)
            .and_then(|v| v.checked_add(2))
            .ok_or("chunk size overflow")?;
        if end > data.len() {
            return Err("truncated chunk".into());
        }
        out.extend_from_slice(&data[start..start + size]);
        // A chunk's data MUST be followed by CRLF; verify rather than skip
        // blindly, so a misframed stream errors instead of silently misaligning.
        if &data[start + size..start + size + 2] != b"\r\n" {
            return Err("missing CRLF after chunk data".into());
        }
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
            resp.text()
                .unwrap_or_default()
                .chars()
                .take(200)
                .collect::<String>()
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
        assert_eq!(
            dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap(),
            b"Wikipedia"
        );
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
    fn dechunk_rejects_missing_crlf_after_data() {
        // Chunk claims 4 bytes but data is not followed by CRLF.
        assert!(dechunk(b"4\r\nWikiXX5\r\npedia\r\n0\r\n\r\n").is_err());
    }

    #[test]
    fn dechunk_rejects_truncated_chunk() {
        assert!(dechunk(b"9\r\nWiki\r\n0\r\n\r\n").is_err());
    }

    #[test]
    fn dechunk_rejects_bad_size() {
        assert!(dechunk(b"zz\r\ndata\r\n0\r\n\r\n").is_err());
    }

    #[test]
    fn parse_rejects_garbage_status_line() {
        assert!(parse_response(b"NOT-HTTP\r\n\r\nbody").is_err());
    }

    #[test]
    fn parse_rejects_missing_header_terminator() {
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 5").is_err());
    }

    #[test]
    fn detect_framing_matches_per_header_not_substring() {
        // "chunked" appears in a non-Transfer-Encoding header value; must NOT
        // be treated as chunked framing.
        let head = "HTTP/1.1 200 OK\r\nX-Note: this is chunked data\r\nContent-Length: 3";
        assert!(matches!(detect_framing(head), Framing::Length(3)));
        let head2 = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked";
        assert!(matches!(detect_framing(head2), Framing::Chunked));
    }
}
