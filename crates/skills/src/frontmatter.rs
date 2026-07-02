//! Minimal YAML-frontmatter parser for SKILL.md files. Handles the subset the
//! Agent Skills spec uses: `key: value` string scalars, quoted values, and
//! folded/continued multi-line values (subsequent more-indented lines).

pub struct Frontmatter {
    pub fields: Vec<(String, String)>,
    /// Everything after the closing `---`.
    pub body: String,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

pub fn parse(content: &str) -> Frontmatter {
    let Some(rest) = content.strip_prefix("---\n").or_else(|| content.strip_prefix("---\r\n")) else {
        return Frontmatter { fields: Vec::new(), body: content.to_string() };
    };
    // Find the closing delimiter line.
    let mut fm_lines = Vec::new();
    let mut body_start = None;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            body_start = Some(offset + line.len());
            break;
        }
        fm_lines.push(trimmed.to_string());
        offset += line.len();
    }
    let body = match body_start {
        Some(b) => rest[b..].trim_start_matches(['\n', '\r']).to_string(),
        None => return Frontmatter { fields: Vec::new(), body: content.to_string() },
    };
    Frontmatter { fields: parse_fields(&fm_lines), body }
}

fn parse_fields(lines: &[String]) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        // Continuation: indented line with no top-level `key:` → append to last.
        let is_indented = line.starts_with(' ') || line.starts_with('\t');
        if let Some((k, v)) = split_kv(line) {
            if is_indented && !fields.is_empty() {
                // Indented but looks like key: — treat as folded continuation text.
                if let Some(last) = fields.last_mut() {
                    last.1.push(' ');
                    last.1.push_str(line.trim());
                    continue;
                }
            }
            fields.push((k, v));
        } else if is_indented {
            if let Some(last) = fields.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        }
    }
    fields
}

fn split_kv(line: &str) -> Option<(String, String)> {
    let (k, v) = line.split_once(':')?;
    let key = k.trim();
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    Some((key.to_string(), unquote(v.trim())))
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\''))) {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_fields_and_body() {
        let c = "---\nname: my-skill\ndescription: does a thing\n---\n# Title\nbody line";
        let fm = parse(c);
        assert_eq!(fm.get("name"), Some("my-skill"));
        assert_eq!(fm.get("description"), Some("does a thing"));
        assert!(fm.body.starts_with("# Title"));
    }

    #[test]
    fn quoted_and_folded() {
        let c = "---\nname: \"quoted name\"\ndescription: line one\n  continued here\n---\nbody";
        let fm = parse(c);
        assert_eq!(fm.get("name"), Some("quoted name"));
        assert_eq!(fm.get("description"), Some("line one continued here"));
    }

    #[test]
    fn missing_frontmatter_tolerated() {
        let fm = parse("no frontmatter here");
        assert!(fm.fields.is_empty());
        assert_eq!(fm.body, "no frontmatter here");
    }
}
