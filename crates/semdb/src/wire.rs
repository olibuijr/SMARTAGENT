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
        // {} on f32 emits the shortest decimal that round-trips through f32 —
        // but NaN/±inf stringify as bare tokens that are NOT valid JSON and
        // would make the whole request unparseable. Real embeddings are always
        // finite; coerce any non-finite value to 0 to keep the line valid JSON.
        if x.is_finite() {
            out.push_str(&x.to_string());
        } else {
            out.push('0');
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors_always_serialize_to_valid_json() {
        // Non-finite floats must not produce bare NaN/inf tokens that break the
        // JSON line protocol; they coerce to 0 and stay parseable + round-trip.
        let v = vec![0.5f32, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.25];
        let line = format!("{{\"vector\":{}}}", vec_to_json(&v));
        let parsed = json::parse(&line).expect("must be valid JSON");
        let back = json_to_vec(parsed.get("vector"));
        assert_eq!(back, vec![0.5, 0.0, 0.0, 0.0, -1.25]);
    }

    #[test]
    fn finite_vectors_round_trip_exactly() {
        let v = vec![0.1f32, 0.2, 0.30000001, -0.99999994];
        let line = format!("{{\"vector\":{}}}", vec_to_json(&v));
        let parsed = json::parse(&line).unwrap();
        assert_eq!(json_to_vec(parsed.get("vector")), v);
    }
}
