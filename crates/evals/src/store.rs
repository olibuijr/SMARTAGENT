//! Append-only JSONL trace store. Torn-tail tolerant (skip unparseable lines).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Trace {
    pub run: String,
    pub case: String,
    pub input: String,
    pub output: String,
    pub expected: Option<String>,
    pub latency_ms: Option<i64>,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\t', "\\t")
}

fn field(line: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let start = line.find(&pat)? + pat.len();
    let mut out = String::new();
    let mut chars = line[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                other => out.push(other),
            },
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}

fn num_field(line: &str, key: &str) -> Option<i64> {
    let pat = format!("\"{key}\":");
    let start = line.find(&pat)? + pat.len();
    let end = line[start..].find(|c: char| !c.is_ascii_digit() && c != '-').map(|i| start + i).unwrap_or(line.len());
    line[start..end].parse().ok()
}

pub fn append(path: &Path, t: &Trace) -> Result<(), String> {
    let mut line = format!(
        r#"{{"kind":"trace","run":"{}","case":"{}","input":"{}","output":"{}""#,
        esc(&t.run), esc(&t.case), esc(&t.input), esc(&t.output)
    );
    if let Some(e) = &t.expected {
        line.push_str(&format!(r#","expected":"{}""#, esc(e)));
    }
    if let Some(l) = t.latency_ms {
        line.push_str(&format!(r#","latency_ms":{l}"#));
    }
    line.push('}');
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
}

pub fn load(path: &Path) -> Vec<Trace> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || !line.contains("\"kind\":\"trace\"") {
            continue;
        }
        let (run, case, input, output) = match (field(line, "run"), field(line, "case"), field(line, "input"), field(line, "output")) {
            (Some(r), Some(c), Some(i), Some(o)) => (r, c, i, o),
            _ => continue, // torn/corrupt line
        };
        out.push(Trace {
            run,
            case,
            input,
            output,
            expected: field(line, "expected"),
            latency_ms: num_field(line, "latency_ms"),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(n: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
        std::fs::create_dir_all(&d).unwrap();
        d.join(n)
    }

    #[test]
    fn append_load_roundtrip_and_torn() {
        let p = scratch("evals-store.jsonl");
        let _ = std::fs::remove_file(&p);
        append(&p, &Trace { run: "r1".into(), case: "c1".into(), input: "in".into(), output: "out".into(), expected: Some("out".into()), latency_ms: Some(42) }).unwrap();
        // torn line
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, r#"{{"kind":"trace","run":"r1","ca"#).unwrap();
        drop(f);
        let traces = load(&p);
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].expected.as_deref(), Some("out"));
        assert_eq!(traces[0].latency_ms, Some(42));
    }
}
