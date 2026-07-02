//! Minimal WebSocket client (RFC 6455) over std::net — just enough to speak
//! the Chrome DevTools Protocol. ws:// only (CDP is localhost); no TLS.
//! Supports the opening handshake, masked client text frames, and reading
//! server text frames (handling fragmentation, ping/pong, and close).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub struct WebSocket {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl WebSocket {
    /// Connect to a ws:// URL (ws://host:port/path).
    pub fn connect(url: &str) -> Result<WebSocket, String> {
        let rest = url.strip_prefix("ws://").ok_or("only ws:// supported")?;
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| "bad port")?),
            None => (authority.to_string(), 80),
        };
        let mut stream = TcpStream::connect((host.as_str(), port)).map_err(|e| format!("connect: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(30))).ok();

        let key = ws_key();
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;

        // Read until end of handshake headers.
        let mut buf = Vec::new();
        let mut tmp = [0u8; 1024];
        loop {
            let n = stream.read(&mut tmp).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("connection closed during handshake".into());
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = find_crlf2(&buf) {
                let head = String::from_utf8_lossy(&buf[..pos]);
                if !head.contains("101") {
                    return Err(format!("handshake failed: {}", head.lines().next().unwrap_or("")));
                }
                let leftover = buf[pos + 4..].to_vec();
                return Ok(WebSocket { stream, buf: leftover });
            }
        }
    }

    pub fn send_text(&mut self, text: &str) -> Result<(), String> {
        let payload = text.as_bytes();
        let mut frame = Vec::with_capacity(payload.len() + 8);
        frame.push(0x81); // FIN + text opcode
        let mask_bit = 0x80;
        let len = payload.len();
        if len < 126 {
            frame.push(mask_bit | len as u8);
        } else if len < 65536 {
            frame.push(mask_bit | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(mask_bit | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
        let mask = mask_key();
        frame.extend_from_slice(&mask);
        for (i, b) in payload.iter().enumerate() {
            frame.push(b ^ mask[i % 4]);
        }
        self.stream.write_all(&frame).map_err(|e| e.to_string())
    }

    /// Read the next text message (handling control frames + fragmentation).
    pub fn recv_text(&mut self) -> Result<String, String> {
        let mut message = Vec::new();
        loop {
            let (fin, opcode, payload) = self.read_frame()?;
            match opcode {
                0x1 | 0x0 => {
                    message.extend_from_slice(&payload);
                    if fin {
                        return String::from_utf8(message).map_err(|_| "non-utf8 text".into());
                    }
                }
                0x8 => return Err("websocket closed by server".into()),
                0x9 => self.send_pong(&payload)?, // ping → pong
                0xA => {} // pong, ignore
                other => return Err(format!("unexpected opcode {other}")),
            }
        }
    }

    fn send_pong(&mut self, payload: &[u8]) -> Result<(), String> {
        let mask = mask_key();
        let mut frame = vec![0x8A, 0x80 | payload.len() as u8];
        frame.extend_from_slice(&mask);
        for (i, b) in payload.iter().enumerate() {
            frame.push(b ^ mask[i % 4]);
        }
        self.stream.write_all(&frame).map_err(|e| e.to_string())
    }

    fn read_frame(&mut self) -> Result<(bool, u8, Vec<u8>), String> {
        let b0 = self.read_byte()?;
        let fin = b0 & 0x80 != 0;
        let opcode = b0 & 0x0F;
        let b1 = self.read_byte()?;
        let masked = b1 & 0x80 != 0;
        let mut len = (b1 & 0x7F) as usize;
        if len == 126 {
            let hi = self.read_byte()? as usize;
            let lo = self.read_byte()? as usize;
            len = (hi << 8) | lo;
        } else if len == 127 {
            let mut l = 0usize;
            for _ in 0..8 {
                l = (l << 8) | self.read_byte()? as usize;
            }
            len = l;
        }
        let mask = if masked {
            [self.read_byte()?, self.read_byte()?, self.read_byte()?, self.read_byte()?]
        } else {
            [0; 4]
        };
        let mut payload = Vec::with_capacity(len);
        for i in 0..len {
            let b = self.read_byte()?;
            payload.push(if masked { b ^ mask[i % 4] } else { b });
        }
        Ok((fin, opcode, payload))
    }

    fn read_byte(&mut self) -> Result<u8, String> {
        if self.buf.is_empty() {
            let mut tmp = [0u8; 4096];
            let n = self.stream.read(&mut tmp).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("connection closed".into());
            }
            self.buf.extend_from_slice(&tmp[..n]);
        }
        Ok(self.buf.remove(0))
    }
}

fn find_crlf2(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn ws_key() -> String {
    // 16 random bytes, base64-encoded (server only echoes a hash; we don't verify).
    let mut seed = entropy();
    let mut bytes = [0u8; 16];
    for b in bytes.iter_mut() {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        *b = (seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as u8;
    }
    base64(&bytes)
}

fn mask_key() -> [u8; 4] {
    let mut seed = entropy();
    let mut m = [0u8; 4];
    for b in m.iter_mut() {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        *b = (seed >> 40) as u8;
    }
    m
}

fn entropy() -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    t ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

fn base64(input: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        out.push(A[(b[0] >> 2) as usize] as char);
        out.push(A[(((b[0] & 0x3) << 4) | (b[1] >> 4)) as usize] as char);
        if chunk.len() > 1 { out.push(A[(((b[1] & 0xf) << 2) | (b[2] >> 6)) as usize] as char); } else { out.push('='); }
        if chunk.len() > 2 { out.push(A[(b[2] & 0x3f) as usize] as char); } else { out.push('='); }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_known_vectors() {
        assert_eq!(base64(b"man"), "bWFu");
        assert_eq!(base64(b"ma"), "bWE=");
        assert_eq!(base64(b"m"), "bQ==");
    }

    #[test]
    fn ws_key_is_24_chars() {
        // 16 bytes base64 → 24 chars with padding.
        assert_eq!(ws_key().len(), 24);
    }

    #[test]
    fn crlf2_boundary() {
        assert_eq!(find_crlf2(b"HTTP/1.1 101\r\nA: b\r\n\r\nrest"), Some(18));
    }
}
