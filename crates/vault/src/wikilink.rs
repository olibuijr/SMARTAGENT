//! Wikilink extraction: [[name]] and [[name|alias]], ignoring links that
//! appear inside fenced code blocks (```) or inline code spans (`...`).

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub target: String,
    pub alias: Option<String>,
}

/// Extract all wikilinks from markdown body, skipping code regions.
pub fn extract(md: &str) -> Vec<Link> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for raw in md.lines() {
        let trimmed = raw.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        extract_line(raw, &mut out);
    }
    out
}

fn extract_line(line: &str, out: &mut Vec<Link>) {
    let bytes: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut in_code = false; // inline `code`
    while i < bytes.len() {
        let c = bytes[i];
        if c == '`' {
            in_code = !in_code;
            i += 1;
            continue;
        }
        if !in_code && c == '[' && i + 1 < bytes.len() && bytes[i + 1] == '[' {
            // Find closing ]]
            if let Some(end) = find_close(&bytes, i + 2) {
                let inner: String = bytes[i + 2..end].iter().collect();
                if let Some(link) = parse_inner(&inner) {
                    out.push(link);
                }
                i = end + 2;
                continue;
            }
        }
        i += 1;
    }
}

fn find_close(bytes: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < bytes.len() {
        if bytes[i] == ']' && bytes[i + 1] == ']' {
            return Some(i);
        }
        // A nested '[' breaks a wikilink — bail.
        if bytes[i] == '[' {
            return None;
        }
        i += 1;
    }
    None
}

fn parse_inner(inner: &str) -> Option<Link> {
    let inner = inner.trim();
    if inner.is_empty() {
        return None;
    }
    match inner.split_once('|') {
        Some((t, a)) => {
            let t = t.trim();
            if t.is_empty() {
                return None;
            }
            Some(Link {
                target: t.to_string(),
                alias: Some(a.trim().to_string()),
            })
        }
        None => Some(Link {
            target: inner.to_string(),
            alias: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_and_alias() {
        let links = extract("See [[Note A]] and [[note-b|Bee]].");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "Note A");
        assert_eq!(links[1].target, "note-b");
        assert_eq!(links[1].alias.as_deref(), Some("Bee"));
    }

    #[test]
    fn ignores_fenced_code() {
        let md = "real [[Live]]\n```\ncode [[NotThis]]\n```\nafter [[Also]]";
        let links = extract(md);
        let targets: Vec<&str> = links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(targets, vec!["Live", "Also"]);
    }

    #[test]
    fn ignores_inline_code() {
        let links = extract("text `[[Nope]]` but [[Yes]]");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "Yes");
    }
}
