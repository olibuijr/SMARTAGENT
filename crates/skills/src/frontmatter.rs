//! Minimal YAML-frontmatter parser for SKILL.md files. Handles the subset the
//! Agent Skills spec uses: `key: value` string scalars, quoted values, and
//! folded/continued multi-line values (subsequent more-indented lines). Also
//! understands one shallow nesting pattern: a `key:` line with an EMPTY value
//! opens a nested mapping (e.g. `metadata:` then `smartagent:`), and leaves
//! under it surface as dotted keys (`metadata.smartagent.role`). Outside an
//! explicitly-opened mapping, indented text (including things that happen to
//! look like `key: value`) still folds onto the previous field, unchanged
//! from before — this keeps wrapped/block-scalar descriptions working.

pub struct Frontmatter {
    pub fields: Vec<(String, String)>,
    /// Everything after the closing `---`.
    pub body: String,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

pub fn parse(content: &str) -> Frontmatter {
    let Some(rest) = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))
    else {
        return Frontmatter {
            fields: Vec::new(),
            body: content.to_string(),
        };
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
        None => {
            return Frontmatter {
                fields: Vec::new(),
                body: content.to_string(),
            }
        }
    };
    Frontmatter {
        fields: parse_fields(&fm_lines),
        body,
    }
}

fn parse_fields(lines: &[String]) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    // Stack of (indent, key) for an explicitly-opened nested mapping (a
    // `key:` line with an empty value). Empty whenever no mapping is open,
    // in which case behavior is identical to before this nesting support was
    // added. Kept shallow on purpose — SKILL.md frontmatter has no need for
    // deep nesting beyond `metadata.smartagent.*`.
    let mut stack: Vec<(usize, String)> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let is_indented = line.starts_with(' ') || line.starts_with('\t');
        let indent = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
        // Close mapping levels we've dedented out of.
        while stack.last().is_some_and(|(i, _)| indent <= *i) {
            stack.pop();
        }
        if let Some((k, v)) = split_kv(line) {
            if !stack.is_empty() {
                // Inside an explicitly-opened mapping: build a dotted path
                // (`metadata.smartagent.role`) instead of folding.
                if v.is_empty() {
                    stack.push((indent, k));
                } else {
                    let prefix: Vec<&str> = stack.iter().map(|(_, s)| s.as_str()).collect();
                    fields.push((format!("{}.{k}", prefix.join(".")), v));
                }
                continue;
            }
            if is_indented && !fields.is_empty() {
                // Indented but looks like key: and no mapping is open —
                // treat as folded continuation text (original behavior).
                if let Some(last) = fields.last_mut() {
                    last.1.push(' ');
                    last.1.push_str(line.trim());
                }
                continue;
            }
            if !is_indented && v.is_empty() {
                // Top-level `key:` with no inline value opens a nested
                // mapping (`metadata:`) rather than an empty field.
                stack.push((indent, k));
                continue;
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
    if v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')))
    {
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

    #[test]
    fn nested_metadata_mapping_becomes_dotted_keys() {
        let c = "---\nname: golf-swing\ndescription: fix a slice\nmetadata:\n  smartagent:\n    role: Builder\n    task: golf-onboarding\nlicense: MIT\n---\nbody";
        let fm = parse(c);
        assert_eq!(fm.get("name"), Some("golf-swing"));
        assert_eq!(fm.get("metadata.smartagent.role"), Some("Builder"));
        assert_eq!(fm.get("metadata.smartagent.task"), Some("golf-onboarding"));
        // A top-level field after a dedent back to indent 0 must parse
        // normally — the mapping must have been closed, not left open.
        assert_eq!(fm.get("license"), Some("MIT"));
        assert!(fm.body.starts_with("body"));
    }

    #[test]
    fn skills_without_metadata_are_unaffected() {
        let c = "---\nname: plain-skill\ndescription: no metadata block here\n---\nbody";
        let fm = parse(c);
        assert_eq!(fm.get("name"), Some("plain-skill"));
        assert_eq!(fm.get("metadata.smartagent.role"), None);
    }
}
