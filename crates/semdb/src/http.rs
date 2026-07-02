//! Minimal plain-HTTP/1.1 POST client on std::net for the external embeddings
//! endpoint. No TLS by design — https egress routes through a local proxy.
//! (Wave 2 replaces this with the shared `httpc` crate; kept tiny here so
//! semdb stands alone.)

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::json::{self, Value};

/// POST a JSON body, return the parsed JSON response body.
pub fn post_json(host: &str, port: u16, path: &str, body: &str) -> Result<Value, String> {
    let addr = format!("{host}:{port}");
    let mut stream = TcpStream::connect(&addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;

    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("send: {e}"))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("recv: {e}"))?;

    let header_end = find_header_end(&raw).ok_or("no header terminator in response")?;
    let head = std::str::from_utf8(&raw[..header_end]).map_err(|_| "non-utf8 headers")?;
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or("empty response")?;
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("bad status line: {status_line}"))?;

    let mut chunked = false;
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        }
    }

    let body_bytes = &raw[header_end + 4..];
    let body_bytes = if chunked {
        dechunk(body_bytes)?
    } else {
        body_bytes.to_vec()
    };
    let text = String::from_utf8(body_bytes).map_err(|_| "non-utf8 body")?;

    if status != 200 {
        return Err(format!("HTTP {status}: {}", text.chars().take(200).collect::<String>()));
    }
    json::parse(text.trim()).map_err(|e| format!("bad JSON response: {e}"))
}

fn find_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn dechunk(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let line_end = data[pos..]
            .windows(2)
            .position(|w| w == b"\r\n")
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
        pos = start + size + 2; // skip trailing CRLF
    }
}

/// Call an OpenAI-compatible /v1/embeddings endpoint; return the vector.
pub fn fetch_embedding(
    host: &str,
    port: u16,
    model: &str,
    text: &str,
) -> Result<Vec<f32>, String> {
    let body = format!(
        r#"{{"model":"{}","input":"{}"}}"#,
        json::escape(model),
        json::escape(text)
    );
    let resp = post_json(host, port, "/v1/embeddings", &body)?;
    let emb = resp
        .get("data")
        .and_then(|d| d.as_arr())
        .and_then(|a| a.first())
        .and_then(|e| e.get("embedding"))
        .and_then(|e| e.as_arr())
        .ok_or("response missing data[0].embedding")?;
    emb.iter()
        .map(|v| v.as_f64().map(|f| f as f32).ok_or_else(|| "non-number in embedding".to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dechunks() {
        let data = b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        assert_eq!(dechunk(data).unwrap(), b"Wikipedia");
    }

    #[test]
    fn finds_header_boundary() {
        assert_eq!(find_header_end(b"HTTP/1.1 200 OK\r\nA: b\r\n\r\nBODY"), Some(21));
    }
}
