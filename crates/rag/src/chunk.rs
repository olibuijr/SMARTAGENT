//! Token-budget chunking with stable byte offsets.

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkConfig {
    pub max_tokens: usize,
    pub overlap_tokens: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        ChunkConfig {
            max_tokens: 512,
            overlap_tokens: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub id: String,
    pub doc_id: String,
    pub source: String,
    pub order: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub kind: String,
}

#[derive(Debug, Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
}

pub fn default_doc_id(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("doc");
    let mut slug = String::new();
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    let prefix = if slug.is_empty() { "doc" } else { slug };
    format!("{prefix}-{:016x}", fnv1a(path.to_string_lossy().as_bytes()))
}

pub fn chunk_text(
    text: &str,
    doc_id: &str,
    source: &str,
    kind: &str,
    cfg: &ChunkConfig,
) -> Vec<Chunk> {
    let spans = token_spans(text);
    if spans.is_empty() {
        return Vec::new();
    }
    let max = cfg.max_tokens.max(1);
    let overlap = cfg.overlap_tokens.min(max.saturating_sub(1));
    let mut chunks = Vec::new();
    let mut start_token = 0usize;
    while start_token < spans.len() {
        let end_token = (start_token + max).min(spans.len());
        let raw_start = spans[start_token].start;
        let raw_end = spans[end_token - 1].end;
        let (start, end) = trim_bounds(text, raw_start, raw_end);
        if start < end {
            let order = chunks.len();
            let id = format!("{doc_id}:{order:06}");
            chunks.push(Chunk {
                id,
                doc_id: doc_id.to_string(),
                source: source.to_string(),
                order,
                start,
                end,
                text: text[start..end].to_string(),
                kind: kind.to_string(),
            });
        }
        if end_token == spans.len() {
            break;
        }
        let next = end_token.saturating_sub(overlap);
        start_token = if next <= start_token { end_token } else { next };
    }
    chunks
}

fn token_spans(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                out.push(Span { start: s, end: i });
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(Span {
            start: s,
            end: text.len(),
        });
    }
    out
}

fn trim_bounds(text: &str, mut start: usize, mut end: usize) -> (usize, usize) {
    while start < end {
        let c = text[start..].chars().next().unwrap();
        if !c.is_whitespace() {
            break;
        }
        start += c.len_utf8();
    }
    while end > start {
        let c = text[..end].chars().next_back().unwrap();
        if !c.is_whitespace() {
            break;
        }
        end -= c.len_utf8();
    }
    (start, end)
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_with_overlap_and_offsets() {
        let cfg = ChunkConfig {
            max_tokens: 3,
            overlap_tokens: 1,
        };
        let chunks = chunk_text("one two three four five", "doc", "s.txt", "text", &cfg);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "one two three");
        assert_eq!(chunks[1].text, "three four five");
        assert_eq!(
            &"one two three four five"[chunks[1].start..chunks[1].end],
            chunks[1].text
        );
    }

    #[test]
    fn default_doc_id_is_stable_and_safe() {
        let id = default_doc_id(Path::new("Notes/My File.md"));
        assert!(id.starts_with("my-file-"));
        assert!(id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
    }
}
