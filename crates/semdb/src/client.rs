//! Thin client for the semdb daemon. Every db-touching CLI verb routes through
//! here. If the daemon is not running the call ERRORS (no direct-file fallback,
//! by design): the daemon is the single owner of the db, so a process reaching
//! around it could clobber the daemon's writes. Start it with `supervise up
//! semdb` (or `semdb serve`).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use httpc::json::{self, Value};

use crate::wire;

fn socket() -> std::path::PathBuf {
    crate::config::Config::load().semdb_socket()
}

/// True if a daemon is accepting connections on the socket.
pub fn is_running() -> bool {
    UnixStream::connect(socket()).is_ok()
}

fn connect() -> Result<UnixStream, String> {
    let path = socket();
    UnixStream::connect(&path).map_err(|e| {
        format!(
            "semdb daemon not running at {} ({e}) — start it: supervise up semdb",
            path.display()
        )
    })
}

/// Send one request line, read one response line, validate `ok`, return the
/// full response object.
fn roundtrip(req: String) -> Result<Value, String> {
    let mut stream = connect()?;
    stream
        .write_all(req.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|e| format!("semdb write: {e}"))?;
    let mut line = String::new();
    BufReader::new(&stream)
        .read_line(&mut line)
        .map_err(|e| format!("semdb read: {e}"))?;
    if line.trim().is_empty() {
        return Err("semdb daemon closed the connection".into());
    }
    let resp = json::parse(line.trim()).map_err(|e| format!("semdb bad response: {e}"))?;
    let ok = resp.get("ok").map(|v| v == &Value::Bool(true)).unwrap_or(false);
    if !ok {
        return Err(resp
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("semdb error")
            .to_string());
    }
    Ok(resp)
}

fn text(req: String) -> Result<String, String> {
    let resp = roundtrip(req)?;
    Ok(resp
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

pub fn create(db: &str) -> Result<String, String> {
    text(format!("{{\"op\":\"create\",\"db\":\"{}\"}}", json::escape(db)))
}

pub fn put(db: &str, id: &str, meta: &str, vector: &[f32]) -> Result<String, String> {
    text(format!(
        "{{\"op\":\"put\",\"db\":\"{}\",\"id\":\"{}\",\"meta\":\"{}\",\"vector\":{}}}",
        json::escape(db),
        json::escape(id),
        json::escape(meta),
        wire::vec_to_json(vector)
    ))
}

pub fn put_many(db: &str, rows: &[(String, String, Vec<f32>)]) -> Result<usize, String> {
    let mut body = format!("{{\"op\":\"put_many\",\"db\":\"{}\",\"rows\":[", json::escape(db));
    for (i, (id, meta, vector)) in rows.iter().enumerate() {
        if i > 0 {
            body.push(',');
        }
        body.push_str(&format!(
            "{{\"id\":\"{}\",\"meta\":\"{}\",\"vector\":{}}}",
            json::escape(id),
            json::escape(meta),
            wire::vec_to_json(vector)
        ));
    }
    body.push_str("]}");
    let resp = roundtrip(body)?;
    Ok(resp.get("count").and_then(Value::as_f64).unwrap_or(0.0) as usize)
}

pub fn get(db: &str, id: &str) -> Result<String, String> {
    text(format!(
        "{{\"op\":\"get\",\"db\":\"{}\",\"id\":\"{}\"}}",
        json::escape(db),
        json::escape(id)
    ))
}

pub fn del_id(db: &str, id: &str) -> Result<String, String> {
    text(format!(
        "{{\"op\":\"del\",\"db\":\"{}\",\"id\":\"{}\"}}",
        json::escape(db),
        json::escape(id)
    ))
}

pub fn del_prefix(db: &str, prefix: &str) -> Result<String, String> {
    text(format!(
        "{{\"op\":\"del\",\"db\":\"{}\",\"prefix\":\"{}\"}}",
        json::escape(db),
        json::escape(prefix)
    ))
}

pub fn stats(db: &str) -> Result<String, String> {
    text(format!("{{\"op\":\"stats\",\"db\":\"{}\"}}", json::escape(db)))
}

pub fn count(db: &str, prefix: Option<&str>) -> Result<String, String> {
    match prefix {
        Some(p) => text(format!(
            "{{\"op\":\"count\",\"db\":\"{}\",\"prefix\":\"{}\"}}",
            json::escape(db),
            json::escape(p)
        )),
        None => text(format!("{{\"op\":\"count\",\"db\":\"{}\"}}", json::escape(db))),
    }
}

pub fn ids(db: &str, prefix: &str, limit: usize) -> Result<String, String> {
    text(format!(
        "{{\"op\":\"ids\",\"db\":\"{}\",\"prefix\":\"{}\",\"limit\":{limit}}}",
        json::escape(db),
        json::escape(prefix)
    ))
}

pub fn compact(db: &str) -> Result<String, String> {
    text(format!("{{\"op\":\"compact\",\"db\":\"{}\"}}", json::escape(db)))
}

/// Vector search. Returns `(id, score, meta)` tuples to match `cli::search`.
pub fn search(db: &str, vector: &[f32], k: usize, exact: bool) -> Result<Vec<(String, f32, String)>, String> {
    let resp = roundtrip(format!(
        "{{\"op\":\"search\",\"db\":\"{}\",\"vector\":{},\"k\":{k},\"exact\":{exact}}}",
        json::escape(db),
        wire::vec_to_json(vector)
    ))?;
    let hits = resp.get("hits").and_then(Value::as_arr).ok_or("semdb: no hits field")?;
    let mut out = Vec::with_capacity(hits.len());
    for hit in hits {
        let Some(cols) = hit.as_arr() else { continue };
        let score = cols.first().and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let id = cols.get(1).and_then(Value::as_str).unwrap_or_default().to_string();
        let meta = cols.get(2).and_then(Value::as_str).unwrap_or_default().to_string();
        out.push((id, score, meta));
    }
    Ok(out)
}
