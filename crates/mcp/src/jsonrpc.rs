//! JSON-RPC 2.0 message construction + response parsing for MCP. Uses httpc's
//! json module (no serde). Notifications carry no id and expect no response.

use httpc::json::{self, Value};

pub fn request(id: u64, method: &str, params: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#)
}

pub fn notification(method: &str, params: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","method":"{method}","params":{params}}}"#)
}

/// Extract the `result` object from a JSON-RPC response, or an error string.
pub fn result(response: &str) -> Result<Value, String> {
    let v = json::parse(response.trim()).map_err(|e| format!("bad json-rpc: {e}"))?;
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let code = err.get("code").and_then(Value::as_f64).unwrap_or(0.0);
        return Err(format!("json-rpc error {code}: {msg}"));
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

pub fn response_id(response: &str) -> Option<f64> {
    json::parse(response.trim())
        .ok()
        .and_then(|v| v.get("id").and_then(Value::as_f64))
}

/// MCP tools/list result → Vec<(name, description)>.
pub fn parse_tools(result: &Value) -> Vec<(String, String)> {
    result
        .get("tools")
        .and_then(Value::as_arr)
        .map(|arr| {
            arr.iter()
                .map(|t| {
                    (
                        t.get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        t.get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// MCP tools/call result → concatenated text content.
pub fn parse_call(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_arr)
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| "(no text content)".into())
}

pub const INIT_PARAMS: &str = r#"{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"smartagent-mcp","version":"0.1.0"}}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_messages() {
        assert!(request(1, "tools/list", "{}").contains(r#""id":1"#));
        assert!(!notification("notifications/initialized", "{}").contains("\"id\""));
    }

    #[test]
    fn parses_result_and_error() {
        let ok = result(
            r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"a","description":"d"}]}}"#,
        )
        .unwrap();
        let tools = parse_tools(&ok);
        assert_eq!(tools, vec![("a".to_string(), "d".to_string())]);
        let err = result(r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"nope"}}"#);
        assert!(err.unwrap_err().contains("nope"));
    }

    #[test]
    fn parses_call_content() {
        let v = httpc::json::parse(
            r#"{"content":[{"type":"text","text":"hello"},{"type":"text","text":"world"}]}"#,
        )
        .unwrap();
        assert_eq!(parse_call(&v), "hello\nworld");
    }
}
