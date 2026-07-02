//! Minimal multipart/form-data body builder for audio upload (STT). Produces
//! the raw body bytes and the boundary string for the Content-Type header.

pub struct Multipart {
    boundary: String,
    body: Vec<u8>,
}

impl Multipart {
    pub fn new(boundary: &str) -> Multipart {
        Multipart { boundary: boundary.to_string(), body: Vec::new() }
    }

    /// A plain text field.
    pub fn field(&mut self, name: &str, value: &str) -> &mut Self {
        self.body.extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        self.body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
        self.body.extend_from_slice(value.as_bytes());
        self.body.extend_from_slice(b"\r\n");
        self
    }

    /// A file part with raw bytes.
    pub fn file(&mut self, name: &str, filename: &str, content_type: &str, data: &[u8]) -> &mut Self {
        self.body.extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        self.body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n").as_bytes(),
        );
        self.body.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
        self.body.extend_from_slice(data);
        self.body.extend_from_slice(b"\r\n");
        self
    }

    /// Finalize: appends the closing boundary and returns (content_type, body).
    pub fn finish(mut self) -> (String, Vec<u8>) {
        self.body.extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        (format!("multipart/form-data; boundary={}", self.boundary), self.body)
    }
}

/// A boundary string derived from time+pid (std-only, no rng crate).
pub fn boundary() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("----smartagent{:x}{:x}", t, std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_framed_body() {
        let mut m = Multipart::new("B");
        m.field("model", "whisper").file("file", "a.wav", "audio/wav", b"RIFFdata");
        let (ct, body) = m.finish();
        assert_eq!(ct, "multipart/form-data; boundary=B");
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("--B\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nwhisper\r\n"));
        assert!(s.contains("filename=\"a.wav\""));
        assert!(s.contains("Content-Type: audio/wav"));
        assert!(s.ends_with("--B--\r\n"));
    }

    #[test]
    fn boundary_is_stable_shape() {
        assert!(boundary().starts_with("----smartagent"));
    }
}
