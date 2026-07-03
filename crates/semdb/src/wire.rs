//! Wire format shared by the semdb daemon (`server`) and its thin `client`.
//! One JSON object per line over a Unix socket. Vectors travel as JSON number
//! arrays; f32 → shortest-round-trip decimal → parsed back as f64 → cast to
//! f32 is lossless (f32 is a subset of f64), so embeddings survive the trip.

use httpc::json::{self, Value};

/// Serialize a float vector as a compact JSON array: `[0.1,0.2,...]`.
pub fn vec_to_json(v: &[f32]) -> String {
    let mut out = String::with_capacity(v.len() * 8 + 2);
    out.push('[');
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        // {} on f32 emits the shortest decimal that round-trips through f32.
        out.push_str(&x.to_string());
    }
    out.push(']');
    out
}

/// Parse a JSON array value back into a float vector (missing/!array → empty).
pub fn json_to_vec(v: Option<&Value>) -> Vec<f32> {
    match v.and_then(Value::as_arr) {
        Some(arr) => arr
            .iter()
            .map(|e| e.as_f64().unwrap_or(0.0) as f32)
            .collect(),
        None => Vec::new(),
    }
}

/// Required string field.
pub fn field<'a>(req: &'a Value, key: &str) -> Result<&'a str, String> {
    req.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing field '{key}'"))
}

/// Optional string field.
pub fn opt_field<'a>(req: &'a Value, key: &str) -> Option<&'a str> {
    req.get(key).and_then(Value::as_str)
}

/// Optional usize field (JSON numbers arrive as f64).
pub fn opt_usize(req: &Value, key: &str) -> Option<usize> {
    req.get(key).and_then(Value::as_f64).map(|n| n as usize)
}

/// Success response carrying a preformatted text line.
pub fn ok_text(text: &str) -> String {
    format!("{{\"ok\":true,\"text\":\"{}\"}}", json::escape(text))
}

/// Success response carrying search hits: `[[score,id,meta],...]`.
pub fn ok_hits(hits: &[(String, f32, String)]) -> String {
    let mut out = String::from("{\"ok\":true,\"hits\":[");
    for (i, (id, score, meta)) in hits.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "[{},\"{}\",\"{}\"]",
            score,
            json::escape(id),
            json::escape(meta)
        ));
    }
    out.push_str("]}");
    out
}

/// Success response carrying an integer count.
pub fn ok_count(n: usize) -> String {
    format!("{{\"ok\":true,\"count\":{n}}}")
}

/// Error response.
pub fn err_line(msg: &str) -> String {
    format!("{{\"ok\":false,\"error\":\"{}\"}}", json::escape(msg))
}
