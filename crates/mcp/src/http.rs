//! Streamable-HTTP transport: POST JSON-RPC to a URL via httpc. Single-JSON
//! responses only (SSE streaming is out of scope v1). A notification (no id)
//! must accept HTTP 202 with an empty body and NOT block on a response.

use httpc::json::Value;

use crate::jsonrpc;

const MCP_HTTP_TIMEOUT_SECS: u64 = 10;

pub struct HttpClient {
    url: String,
    auth: Option<String>,
    next_id: u64,
    timeout_secs: u64,
}

impl HttpClient {
    pub fn start(url: &str) -> Result<HttpClient, String> {
        Self::start_with_auth(url, None)
    }

    /// `auth` becomes an `Authorization: Bearer …` header — most hosted MCP
    /// servers are unreachable without it.
    pub fn start_with_auth(url: &str, auth: Option<String>) -> Result<HttpClient, String> {
        Self::start_with_auth_timeout(url, auth, MCP_HTTP_TIMEOUT_SECS)
    }

    pub fn start_with_timeout(url: &str, timeout_secs: u64) -> Result<HttpClient, String> {
        Self::start_with_auth_timeout(url, None, timeout_secs)
    }

    pub fn start_with_auth_timeout(url: &str, auth: Option<String>, timeout_secs: u64) -> Result<HttpClient, String> {
        let mut c = HttpClient {
            url: url.to_string(),
            auth,
            next_id: 1,
            timeout_secs: timeout_secs.max(1),
        };
        c.call("initialize", jsonrpc::INIT_PARAMS)?;
        c.notify("notifications/initialized", "{}")?;
        Ok(c)
    }

    pub fn call(&mut self, method: &str, params: &str) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let body = jsonrpc::request(id, method, params);
        let mut req = httpc::request("POST", &self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");
        if let Some(a) = &self.auth {
            req = req.header("Authorization", &format!("Bearer {a}"));
        }
        let resp = req.timeout(self.timeout_secs).body(body.as_bytes()).send()?;
        if !resp.ok() {
            return Err(format!("HTTP {}", resp.status));
        }
        jsonrpc::result(&resp.text()?)
    }

    /// Notifications expect no response body; 202 with empty body is valid.
    pub fn notify(&mut self, method: &str, params: &str) -> Result<(), String> {
        let body = jsonrpc::notification(method, params);
        let mut req = httpc::request("POST", &self.url).header("Content-Type", "application/json");
        if let Some(a) = &self.auth {
            req = req.header("Authorization", &format!("Bearer {a}"));
        }
        let resp = req.timeout(self.timeout_secs).body(body.as_bytes()).send()?;
        // 200 or 202 (empty) both fine; anything else is an error.
        if resp.status == 202 || resp.ok() {
            Ok(())
        } else {
            Err(format!("notification HTTP {}", resp.status))
        }
    }
}
