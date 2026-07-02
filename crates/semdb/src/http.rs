//! Embeddings client for the external OpenAI-compatible endpoint. The HTTP
//! transport is the shared `httpc` crate (one client, one set of parsing/redirect
//! fixes) — this module is just the embeddings-specific request/response shape.

use httpc::json::{self, Value};

/// POST a JSON body to `http://host:port{path}`, return the parsed JSON body.
pub fn post_json(host: &str, port: u16, path: &str, body: &str) -> Result<Value, String> {
    let url = format!("http://{host}:{port}{path}");
    let resp = httpc::Request::new("POST", &url)
        .header("Content-Type", "application/json")
        .body(body.as_bytes())
        .timeout(60)
        .send()?;
    let text = resp.text()?;
    if resp.status != 200 {
        return Err(format!("HTTP {}: {}", resp.status, text.chars().take(200).collect::<String>()));
    }
    json::parse(text.trim()).map_err(|e| format!("bad JSON response: {e}"))
}

/// Call an OpenAI-compatible /v1/embeddings endpoint; return the vector.
pub fn fetch_embedding(host: &str, port: u16, model: &str, text: &str) -> Result<Vec<f32>, String> {
    let body = format!(
        r#"{{"model":"{}","input":"{}"}}"#,
        json::escape(model),
        json::escape(text)
    );
    let resp = post_json(host, port, "/v1/embeddings", &body)?;
    let emb = resp
        .get("data")
        .and_then(|d| d.as_arr())
        .and_then(|a| a.first())
        .and_then(|e| e.get("embedding"))
        .and_then(|e| e.as_arr())
        .ok_or("response missing data[0].embedding")?;
    emb.iter()
        .map(|v| v.as_f64().map(|f| f as f32).ok_or_else(|| "non-number in embedding".to_string()))
        .collect()
}
