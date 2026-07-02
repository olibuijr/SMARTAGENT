//! Trace store backed by a semdb table (rule 7: all data in semdb tables, not
//! bespoke JSONL). Each trace is one row with a monotonic id; `load` reads all
//! rows back in append order. semdb's CRC-framed log gives crash safety.

use std::path::{Path, PathBuf};

use semdb::json;
use semdb::storage::Db;

const PLACEHOLDER_VEC: [f32; 1] = [0.0]; // non-semantic rows

#[derive(Debug, Clone)]
pub struct Trace {
    pub run: String,
    pub case: String,
    pub input: String,
    pub output: String,
    pub expected: Option<String>,
    pub latency_ms: Option<i64>,
}

/// Normalize a legacy `*.jsonl` path to the `*.semdb` table.
fn table(path: &Path) -> PathBuf {
    let mut p = path.to_path_buf();
    if p.extension().map(|e| e == "jsonl").unwrap_or(false) {
        p.set_extension("semdb");
    }
    p
}

fn to_meta(t: &Trace) -> String {
    let mut s = format!(
        r#"{{"run":"{}","case":"{}","input":"{}","output":"{}""#,
        json::escape(&t.run), json::escape(&t.case), json::escape(&t.input), json::escape(&t.output)
    );
    if let Some(e) = &t.expected {
        s.push_str(&format!(r#","expected":"{}""#, json::escape(e)));
    }
    if let Some(l) = t.latency_ms {
        s.push_str(&format!(r#","latency_ms":{l}"#));
    }
    s.push('}');
    s
}

fn from_meta(meta: &str) -> Option<Trace> {
    let v = json::parse(meta).ok()?;
    Some(Trace {
        run: v.get("run")?.as_str()?.to_string(),
        case: v.get("case")?.as_str()?.to_string(),
        input: v.get("input")?.as_str()?.to_string(),
        output: v.get("output")?.as_str()?.to_string(),
        expected: v.get("expected").and_then(|x| x.as_str()).map(str::to_string),
        latency_ms: v.get("latency_ms").and_then(|x| x.as_f64()).map(|f| f as i64),
    })
}

pub fn append(path: &Path, t: &Trace) -> Result<(), String> {
    let p = table(path);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut db = if p.exists() { Db::open(&p)? } else { Db::create(&p)? };
    // Monotonic, unique id: timestamp + current row count.
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let id = format!("{ts:039}-{}", db.index.len());
    db.put(&id, &to_meta(t), PLACEHOLDER_VEC.to_vec())
}

/// All traces, in append order (ids are timestamp+count, so sorting == order).
pub fn load(path: &Path) -> Vec<Trace> {
    let p = table(path);
    if !p.exists() {
        return Vec::new();
    }
    let db = match Db::open(&p) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut rows: Vec<(&String, &String)> = db.index.iter().map(|(k, e)| (k, &e.meta)).collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    rows.into_iter().filter_map(|(_, m)| from_meta(m)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(n: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join(n);
        let _ = std::fs::remove_file(p.with_extension("semdb"));
        p
    }

    #[test]
    fn append_load_roundtrip() {
        let p = scratch("evals-store.jsonl");
        append(&p, &Trace { run: "r1".into(), case: "c1".into(), input: "in".into(), output: "out".into(), expected: Some("out".into()), latency_ms: Some(42) }).unwrap();
        append(&p, &Trace { run: "r1".into(), case: "c2".into(), input: "in2".into(), output: "bad".into(), expected: Some("good".into()), latency_ms: None }).unwrap();
        let traces = load(&p);
        assert_eq!(traces.len(), 2);
        assert_eq!(traces[0].case, "c1");
        assert_eq!(traces[0].expected.as_deref(), Some("out"));
        assert_eq!(traces[0].latency_ms, Some(42));
        assert_eq!(traces[1].latency_ms, None);
    }
}
