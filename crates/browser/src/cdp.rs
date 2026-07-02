//! Chrome DevTools Protocol client: JSON-RPC over the WebSocket. Discovers a
//! page target via the DevTools HTTP endpoint (httpc), opens its debugger
//! socket, and issues Page.navigate + Runtime.evaluate to extract a compact
//! snapshot — the Browser Use mechanism, in pure Rust.

use httpc::json::{self, Value};

use crate::ws::WebSocket;

pub struct Cdp {
    ws: WebSocket,
    next_id: u64,
}

impl Cdp {
    /// Connect to Chrome DevTools at `base` (e.g. http://127.0.0.1:9222),
    /// pick the first page target, and open its debugger WebSocket.
    pub fn connect(base: &str) -> Result<Cdp, String> {
        let list = httpc::get(&format!("{}/json", base.trim_end_matches('/')))
            .map_err(|e| format!("devtools /json: {e} (is Chrome running with --remote-debugging-port?)"))?;
        let targets = list.json().map_err(|e| format!("bad /json: {e}"))?;
        let arr = targets.as_arr().ok_or("devtools /json not an array")?;
        let ws_url = arr
            .iter()
            .find(|t| t.get("type").and_then(Value::as_str) == Some("page"))
            .and_then(|t| t.get("webSocketDebuggerUrl"))
            .and_then(Value::as_str)
            .ok_or("no page target with a debugger url")?;
        let ws = WebSocket::connect(ws_url)?;
        Ok(Cdp { ws, next_id: 1 })
    }

    fn call(&mut self, method: &str, params: &str) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = format!(r#"{{"id":{id},"method":"{method}","params":{params}}}"#);
        self.ws.send_text(&msg)?;
        // Read messages until the matching id response (skip events).
        for _ in 0..200 {
            let text = self.ws.recv_text()?;
            let v = json::parse(&text).map_err(|e| format!("bad cdp json: {e}"))?;
            if v.get("id").and_then(Value::as_f64) == Some(id as f64) {
                if let Some(err) = v.get("error") {
                    return Err(format!("cdp error: {}", err.get("message").and_then(Value::as_str).unwrap_or("?")));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
        }
        Err("no cdp response for call".into())
    }

    pub fn navigate(&mut self, url: &str) -> Result<(), String> {
        self.call("Page.enable", "{}")?;
        self.call("Page.navigate", &format!(r#"{{"url":"{}"}}"#, json::escape(url)))?;
        // Give the page a beat to load, then continue (event-wait is out of scope v1).
        std::thread::sleep(std::time::Duration::from_millis(800));
        Ok(())
    }

    /// Evaluate JS in the page and return the string result.
    pub fn eval(&mut self, expr: &str) -> Result<String, String> {
        let params = format!(r#"{{"expression":"{}","returnByValue":true}}"#, json::escape(expr));
        let result = self.call("Runtime.evaluate", &params)?;
        Ok(result
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string())
    }

    /// Compact snapshot: page title, visible text (trimmed), and links.
    pub fn snapshot(&mut self) -> Result<String, String> {
        self.call("Runtime.enable", "{}")?;
        self.eval(SNAPSHOT_JS)
    }
}

/// JS that builds a compact, token-frugal snapshot (Browser Use style).
const SNAPSHOT_JS: &str = r#"(function(){
  var title = document.title || '';
  var text = (document.body ? document.body.innerText : '').replace(/\s+/g,' ').trim().slice(0, 4000);
  var links = [];
  var as = document.querySelectorAll('a[href]');
  for (var i=0; i<as.length && links.length<40; i++){
    var t = as[i].innerText.replace(/\s+/g,' ').trim();
    if (t) links.push('- ' + t + ' -> ' + as[i].href);
  }
  return 'TITLE: ' + title + '\n\n' + text + '\n\nLINKS:\n' + links.join('\n');
})()"#;
