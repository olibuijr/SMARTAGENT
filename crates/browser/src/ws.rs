//! Minimal WebSocket client (RFC 6455) over std::net — just enough to speak
//! the Chrome DevTools Protocol. ws:// only (CDP is localhost); no TLS.
//! Supports the opening handshake, masked client text frames, and reading
//! server text frames (handling fragmentation, ping/pong, and close).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Reject frames larger than this to prevent a corrupt/hostile length field
/// from triggering a multi-GB allocation. CDP frames are well under this.
const MAX_FRAME: usize = 64 * 1024 * 1024;

pub struct WebSocket {
    stream: TcpStream,
    buf: Vec<u8>,
    pos: usize, // read cursor into buf; consumed prefix is compacted lazily
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
        let mut stream =
            TcpStream::connect((host.as_str(), port)).map_err(|e| format!("connect: {e}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(30))).ok();

        let key = ws_key();
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(req.as_bytes())
            .map_err(|e| e.to_string())?;

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
                validate_handshake(&head, &key)?;
                let leftover = buf[pos + 4..].to_vec();
                return Ok(WebSocket {
                    stream,
                    buf: leftover,
                    pos: 0,
                });
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
                0xA => {}                         // pong, ignore
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
        if len > MAX_FRAME {
            return Err(format!("frame too large: {len} bytes (max {MAX_FRAME})"));
        }
        let mask = if masked {
            [
                self.read_byte()?,
                self.read_byte()?,
                self.read_byte()?,
                self.read_byte()?,
            ]
        } else {
            [0; 4]
        };
        // Bulk-read the whole payload at once, then unmask in place — O(n),
        // not the O(n²) that byte-by-byte Vec::remove(0) produced on large
        // (hundreds-of-KB) CDP frames.
        let mut payload = self.read_exact(len)?;
        if masked {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        Ok((fin, opcode, payload))
    }

    /// Ensure at least one buffered byte, reading from the socket if needed.
    fn fill(&mut self) -> Result<(), String> {
        if self.pos >= self.buf.len() {
            self.buf.clear();
            self.pos = 0;
            let mut tmp = [0u8; 8192];
            let n = self.stream.read(&mut tmp).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("connection closed".into());
            }
            self.buf.extend_from_slice(&tmp[..n]);
        }
        Ok(())
    }

    fn read_byte(&mut self) -> Result<u8, String> {
        self.fill()?;
        let b = self.buf[self.pos];
        self.pos += 1;
        Ok(b)
    }

    /// Read exactly `n` bytes, copying buffered runs in bulk.
    fn read_exact(&mut self, n: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            self.fill()?;
            let want = n - out.len();
            let avail = self.buf.len() - self.pos;
            let take = want.min(avail);
            out.extend_from_slice(&self.buf[self.pos..self.pos + take]);
            self.pos += take;
        }
        Ok(out)
    }
}

fn find_crlf2(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn validate_handshake(head: &str, key: &str) -> Result<(), String> {
    let status = head.lines().next().unwrap_or("");
    if !status.starts_with("HTTP/1.1 101 ") && status != "HTTP/1.1 101" {
        return Err(format!("handshake failed: {status}"));
    }
    let got = header_value(head, "sec-websocket-accept")
        .ok_or("handshake failed: missing Sec-WebSocket-Accept")?;
    let want = websocket_accept(key);
    if got.trim() != want {
        return Err("handshake failed: bad Sec-WebSocket-Accept".into());
    }
    Ok(())
}

fn header_value<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    for line in head.lines().skip(1) {
        let (k, v) = line.split_once(':')?;
        if k.trim().eq_ignore_ascii_case(name) {
            return Some(v.trim());
        }
    }
    None
}

fn websocket_accept(key: &str) -> String {
    let mut bytes = Vec::from(key.as_bytes());
    bytes.extend_from_slice(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64(&sha1(&bytes))
}

fn sha1(input: &[u8]) -> [u8; 20] {
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;

    let bit_len = (input.len() as u64) * 8;
    let mut msg = input.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().take(16).enumerate() {
            let j = i * 4;
            *word = u32::from_be_bytes([chunk[j], chunk[j + 1], chunk[j + 2], chunk[j + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, h) in [h0, h1, h2, h3, h4].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&h.to_be_bytes());
    }
    out
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
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1);
    t ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

fn base64(input: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        out.push(A[(b[0] >> 2) as usize] as char);
        out.push(A[(((b[0] & 0x3) << 4) | (b[1] >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(A[(((b[1] & 0xf) << 2) | (b[2] >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(A[(b[2] & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
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

    #[test]
    fn websocket_accept_known_vector() {
        assert_eq!(
            websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn validates_sec_websocket_accept() {
        let key = "dGhlIHNhbXBsZSBub25jZQ==";
        let good = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n";
        assert!(validate_handshake(good, key).is_ok());

        let missing = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n";
        assert!(validate_handshake(missing, key).is_err());

        let wrong = "HTTP/1.1 101 Switching Protocols\r\nSec-WebSocket-Accept: wrong\r\n";
        assert!(validate_handshake(wrong, key).is_err());
    }
}
