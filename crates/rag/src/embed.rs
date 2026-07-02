//! OpenAI-compatible embedding calls over the shared std-only HTTP client.

use httpc::json::{escape, Value};

pub struct Embedder {
    endpoint: String,
    model: String,
}

impl Embedder {
    pub fn new(endpoint: String, model: String) -> Embedder {
        Embedder { endpoint, model }
    }

    pub fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let body = format!(r#"{{"model":"{}","input":"{}"}}"#, escape(&self.model), escape(text));
        let value = httpc::post_json(&self.url(), &body)?;
        parse_embedding(&value)
    }

    fn url(&self) -> String {
        let ep = self.endpoint.trim_end_matches('/');
        if ep.starts_with("http://") {
            format!("{ep}/v1/embeddings")
        } else {
            format!("http://{ep}/v1/embeddings")
        }
    }
}

pub fn parse_embedding(value: &Value) -> Result<Vec<f32>, String> {
    let data = value.get("data").and_then(Value::as_arr).ok_or("embedding response missing data[]")?;
    let first = data.first().ok_or("embedding response has empty data[]")?;
    let emb = first
        .get("embedding")
        .and_then(Value::as_arr)
        .ok_or("embedding response missing data[0].embedding[]")?;
    let mut out = Vec::with_capacity(emb.len());
    for v in emb {
        out.push(v.as_f64().ok_or("embedding contains non-number")? as f32);
    }
    if out.is_empty() {
        Err("empty embedding".into())
    } else {
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_embedding_response() {
        let v = httpc::json::parse(r#"{"data":[{"embedding":[1,-0.5,2e-1]}]}"#).unwrap();
        assert_eq!(parse_embedding(&v).unwrap(), vec![1.0, -0.5, 0.2]);
    }
}
