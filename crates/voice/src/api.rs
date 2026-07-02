//! STT/TTS over OpenAI-compatible endpoints (Pipecat pipeline concept — thin
//! client; the models live externally). STT = multipart POST to
//! /v1/audio/transcriptions; TTS = JSON POST to /v1/audio/speech → binary audio.

use httpc::json::{self, Value};

use crate::multipart::{self, Multipart};

/// Transcribe a WAV file's bytes → text.
pub fn transcribe(url: &str, model: &str, wav: &[u8]) -> Result<String, String> {
    let mut m = Multipart::new(&multipart::boundary());
    m.field("model", model).file("file", "audio.wav", "audio/wav", wav);
    let (content_type, body) = m.finish();
    let resp = httpc::request("POST", url)
        .header("Content-Type", &content_type)
        .body(&body)
        .timeout(120)
        .send()?;
    if !resp.ok() {
        return Err(format!("STT HTTP {}: {}", resp.status, resp.text().unwrap_or_default().chars().take(200).collect::<String>()));
    }
    let v = json::parse(resp.text()?.trim()).map_err(|e| format!("bad STT json: {e}"))?;
    Ok(v.get("text").and_then(Value::as_str).unwrap_or("").to_string())
}

/// Synthesize speech for `text` → raw audio bytes.
pub fn synthesize(url: &str, model: &str, voice: &str, text: &str) -> Result<Vec<u8>, String> {
    let body = format!(
        r#"{{"model":"{}","voice":"{}","input":"{}"}}"#,
        json::escape(model), json::escape(voice), json::escape(text)
    );
    let resp = httpc::request("POST", url)
        .header("Content-Type", "application/json")
        .body(body.as_bytes())
        .timeout(120)
        .send()?;
    if !resp.ok() {
        return Err(format!("TTS HTTP {}", resp.status));
    }
    Ok(resp.body.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn mock(response: &'static [u8]) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            // drain request fully (avoid RST)
            let mut buf = Vec::new();
            let mut tmp = [0u8; 2048];
            loop {
                let n = s.read(&mut tmp).unwrap();
                if n == 0 { break; }
                buf.extend_from_slice(&tmp[..n]);
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..p]);
                    let len: usize = head.lines().find_map(|l| l.strip_prefix("Content-Length:")).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
                    if buf.len() >= p + 4 + len { break; }
                }
            }
            s.write_all(response).unwrap();
        });
        port
    }

    #[test]
    fn stt_parses_text() {
        let port = mock(b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\n\r\n{\"text\":\"hello\"}");
        let out = transcribe(&format!("http://127.0.0.1:{port}/v1/audio/transcriptions"), "whisper", b"RIFF").unwrap();
        assert_eq!(out, "hello");
    }

    #[test]
    fn tts_returns_bytes() {
        let port = mock(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n\x01\x02\x03\x04");
        let audio = synthesize(&format!("http://127.0.0.1:{port}/v1/audio/speech"), "tts", "alloy", "hi").unwrap();
        assert_eq!(audio, vec![1, 2, 3, 4]);
    }
}
