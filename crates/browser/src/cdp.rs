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
        self.wait_ready(8000);
        Ok(())
    }

    /// Poll `document.readyState` until the document is complete (or the
    /// deadline passes — SPAs may never signal, so this never errors). Replaces
    /// the fixed sleeps that raced slow pages and wasted time on fast ones.
    fn wait_ready(&mut self, max_ms: u64) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(max_ms);
        // Small settle first: a just-triggered navigation may still report the
        // OLD document as complete.
        std::thread::sleep(std::time::Duration::from_millis(120));
        loop {
            match self.eval("document.readyState") {
                Ok(s) if s == "complete" || s == "interactive" => return,
                _ if std::time::Instant::now() >= deadline => return,
                _ => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
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

    /// Compact snapshot: page title, visible text (trimmed to `max_text` chars),
    /// and up to `max_links` links. Smaller caps = fewer tokens.
    pub fn snapshot_capped(&mut self, max_text: usize, max_links: usize) -> Result<String, String> {
        self.call("Runtime.enable", "{}")?;
        let js = SNAPSHOT_JS
            .replace("__MAXTEXT__", &max_text.to_string())
            .replace("__MAXLINKS__", &max_links.to_string());
        self.eval(&js)
    }

    /// Default snapshot (4000 chars / 40 links).
    pub fn snapshot(&mut self) -> Result<String, String> {
        self.snapshot_capped(4000, 40)
    }

    /// Click the first element matching a CSS selector. Returns a status line.
    pub fn click(&mut self, selector: &str) -> Result<String, String> {
        self.call("Runtime.enable", "{}")?;
        let js = format!(
            r#"(function(){{var e=document.querySelector("{}");if(!e)return "not found: {}";e.scrollIntoView();e.click();return "clicked";}})()"#,
            json::escape(selector),
            json::escape(selector)
        );
        let r = self.eval(&js)?;
        self.wait_ready(3000);
        Ok(r)
    }

    /// Type text into an input/textarea matching a selector, firing input +
    /// change events so frameworks (React etc.) register the value.
    pub fn type_text(&mut self, selector: &str, text: &str) -> Result<String, String> {
        self.call("Runtime.enable", "{}")?;
        let js = format!(
            r#"(function(){{var e=document.querySelector("{}");if(!e)return "not found: {}";e.focus();e.value="{}";e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));return "typed";}})()"#,
            json::escape(selector),
            json::escape(selector),
            json::escape(text)
        );
        self.eval(&js)
    }

    /// Poll until a selector appears (or timeout). Returns "ready" or errors.
    pub fn wait_for(&mut self, selector: &str, timeout_ms: u64) -> Result<String, String> {
        self.call("Runtime.enable", "{}")?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        let js = format!("(function(){{return document.querySelector(\"{}\")?\"1\":\"\";}})()", json::escape(selector));
        loop {
            if self.eval(&js)? == "1" {
                return Ok("ready".into());
            }
            if std::time::Instant::now() >= deadline {
                return Err(format!("timeout: '{selector}' did not appear in {timeout_ms}ms"));
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    }

    /// Scroll to an element (selector) or by a pixel delta. Empty selector +
    /// px=0 scrolls to the bottom (reveal lazy-loaded content).
    pub fn scroll(&mut self, selector: &str, by_px: i64) -> Result<String, String> {
        self.call("Runtime.enable", "{}")?;
        let js = if !selector.is_empty() {
            format!("(function(){{var e=document.querySelector(\"{}\");if(!e)return \"not found\";e.scrollIntoView();return \"ok\";}})()", json::escape(selector))
        } else if by_px != 0 {
            format!("(function(){{window.scrollBy(0,{by_px});return \"ok\";}})()")
        } else {
            "(function(){window.scrollTo(0,document.body.scrollHeight);return \"ok\";})()".to_string()
        };
        let r = self.eval(&js)?;
        std::thread::sleep(std::time::Duration::from_millis(400));
        Ok(r)
    }

    /// Read an attribute (or 'text'/'value') of the first matching element.
    pub fn attr(&mut self, selector: &str, name: &str) -> Result<String, String> {
        self.call("Runtime.enable", "{}")?;
        let getter = match name {
            "text" => "e.innerText".to_string(),
            "value" => "e.value".to_string(),
            n => format!("e.getAttribute(\"{}\")", json::escape(n)),
        };
        let js = format!("(function(){{var e=document.querySelector(\"{}\");if(!e)return \"\";var v={getter};return v==null?\"\":String(v);}})()", json::escape(selector));
        self.eval(&js)
    }

    /// Press Enter in the focused/selected element (submit forms after `type`).
    pub fn press_enter(&mut self, selector: &str) -> Result<String, String> {
        self.call("Runtime.enable", "{}")?;
        let js = format!(
            "(function(){{var e=document.querySelector(\"{}\");if(!e)return \"not found\";var f=e.form;if(f&&f.requestSubmit){{f.requestSubmit();return \"submitted\";}}e.dispatchEvent(new KeyboardEvent('keydown',{{key:'Enter',keyCode:13,bubbles:true}}));return \"enter\";}})()",
            json::escape(selector)
        );
        let r = self.eval(&js)?;
        self.wait_ready(3000);
        Ok(r)
    }

    /// Navigate history back/forward and let the page settle.
    pub fn history(&mut self, delta: i32) -> Result<String, String> {
        self.call("Runtime.enable", "{}")?;
        let r = self.eval(&format!("(function(){{history.go({delta});return 'ok';}})()"))?;
        self.wait_ready(5000);
        Ok(r)
    }

    /// Capture the visible viewport as PNG bytes (Page.captureScreenshot).
    pub fn screenshot_png(&mut self) -> Result<Vec<u8>, String> {
        let result = self.call("Page.captureScreenshot", r#"{"format":"png"}"#)?;
        let data = result
            .get("data")
            .and_then(Value::as_str)
            .ok_or("captureScreenshot returned no data")?;
        base64_decode(data)
    }

    /// Current page status for address bars: `url \t title \t readyState`.
    pub fn page_status(&mut self) -> Result<String, String> {
        self.call("Runtime.enable", "{}")?;
        self.eval("location.href + '\\t' + (document.title||'') + '\\t' + document.readyState")
    }
}

/// Decode standard base64 (with or without padding). CDP screenshot payloads.
pub fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Result<u32, String> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
            b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("bad base64 byte {c}")),
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| !b" \r\n\t".contains(b)).collect();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let mut acc = 0u32;
        let mut n = 0;
        for &c in chunk.iter().take(chunk.len() - pad) {
            acc = (acc << 6) | val(c)?;
            n += 1;
        }
        acc <<= 6 * (4 - n);
        if n >= 2 { out.push((acc >> 16) as u8); }
        if n >= 3 { out.push((acc >> 8) as u8); }
        if n == 4 { out.push(acc as u8); }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::base64_decode;

    #[test]
    fn base64_decode_known_vectors() {
        assert_eq!(base64_decode("bWFu").unwrap(), b"man");
        assert_eq!(base64_decode("bWE=").unwrap(), b"ma");
        assert_eq!(base64_decode("bQ==").unwrap(), b"m");
        assert_eq!(base64_decode("").unwrap(), b"");
    }

    #[test]
    fn base64_decode_rejects_garbage() {
        assert!(base64_decode("b?==").is_err());
    }
}

/// JS that builds a compact, token-frugal snapshot (Browser Use style).
const SNAPSHOT_JS: &str = r#"(function(){
  var title = document.title || '';
  var text = (document.body ? document.body.innerText : '').replace(/\s+/g,' ').trim().slice(0, __MAXTEXT__);
  var links = [];
  var as = document.querySelectorAll('a[href]');
  for (var i=0; i<as.length && links.length<__MAXLINKS__; i++){
    var t = as[i].innerText.replace(/\s+/g,' ').trim();
    if (t) links.push('- ' + t + ' -> ' + as[i].href);
  }
  return 'TITLE: ' + title + '\n\n' + text + '\n\nLINKS:\n' + links.join('\n');
})()"#;
