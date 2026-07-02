//! Minimal PDF text extraction for std-only ingestion.
//!
//! This intentionally targets text-bearing PDFs with literal or hex strings in
//! content streams. Compressed/object-stream PDFs need OCR or a richer parser
//! upstream; this crate remains a pure Rust zero-dependency ingestion stage.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Auto,
    Text,
    Pdf,
}

pub struct Extracted {
    pub text: String,
    pub kind: String,
}

pub fn read_document(path: &Path, kind: Kind) -> Result<Extracted, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let is_pdf = match kind {
        Kind::Pdf => true,
        Kind::Text => false,
        Kind::Auto => {
            bytes.starts_with(b"%PDF")
                || path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.eq_ignore_ascii_case("pdf"))
                    .unwrap_or(false)
        }
    };
    if is_pdf {
        let text = extract_pdf_text(&bytes)?;
        Ok(Extracted {
            text,
            kind: "pdf".into(),
        })
    } else {
        let text = String::from_utf8(bytes)
            .map_err(|_| format!("{} is not UTF-8 text", path.display()))?;
        Ok(Extracted {
            text,
            kind: "text".into(),
        })
    }
}

pub fn extract_pdf_text(bytes: &[u8]) -> Result<String, String> {
    let mut parts = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => {
                if let Some((s, next)) = literal_string(bytes, i + 1) {
                    if useful(&s) {
                        parts.push(s);
                    }
                    i = next;
                } else {
                    i += 1;
                }
            }
            b'<' if bytes.get(i + 1) != Some(&b'<') => {
                if let Some((s, next)) = hex_string(bytes, i + 1) {
                    if useful(&s) {
                        parts.push(s);
                    }
                    i = next;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    let text = normalize_lines(&parts.join("\n"));
    if text.trim().is_empty() {
        Err(
            "no extractable PDF text found (compressed/OCR PDFs need upstream text extraction)"
                .into(),
        )
    } else {
        Ok(text)
    }
}

fn literal_string(bytes: &[u8], mut i: usize) -> Option<(String, usize)> {
    let mut out = Vec::new();
    let mut depth = 1i32;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            i += 1;
            let escaped = *bytes.get(i)?;
            match escaped {
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'b' => out.push(8),
                b'f' => out.push(12),
                b'(' | b')' | b'\\' => out.push(escaped),
                b'\n' => {}
                b'\r' => {
                    if bytes.get(i + 1) == Some(&b'\n') {
                        i += 1;
                    }
                }
                b'0'..=b'7' => {
                    let mut val = (escaped - b'0') as u16;
                    for _ in 0..2 {
                        if let Some(c @ b'0'..=b'7') = bytes.get(i + 1).copied() {
                            i += 1;
                            val = val * 8 + (c - b'0') as u16;
                        }
                    }
                    out.push((val & 0xff) as u8);
                }
                other => out.push(other),
            }
        } else if b == b'(' {
            depth += 1;
            out.push(b);
        } else if b == b')' {
            depth -= 1;
            if depth == 0 {
                return Some((String::from_utf8_lossy(&out).into_owned(), i + 1));
            }
            out.push(b);
        } else {
            out.push(b);
        }
        i += 1;
    }
    None
}

fn hex_string(bytes: &[u8], mut i: usize) -> Option<(String, usize)> {
    let mut nibbles = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'>' {
            if nibbles.len() % 2 == 1 {
                nibbles.push(0);
            }
            let mut out = Vec::new();
            for pair in nibbles.chunks(2) {
                out.push((pair[0] << 4) | pair[1]);
            }
            return Some((String::from_utf8_lossy(&out).into_owned(), i + 1));
        }
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        nibbles.push(hex_val(b)?);
        i += 1;
    }
    None
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn useful(s: &str) -> bool {
    let t = s.trim();
    t.len() > 1 && t.chars().any(|c| c.is_alphanumeric())
}

fn normalize_lines(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_literal_and_hex_pdf_strings() {
        let pdf = b"%PDF-1.1\n1 0 obj\n<<>>stream\nBT (Hello \\(PDF\\)) Tj <776f726c64> Tj ET\nendstream\n%%EOF";
        let text = extract_pdf_text(pdf).unwrap();
        assert!(text.contains("Hello (PDF)"));
        assert!(text.contains("world"));
    }
}
