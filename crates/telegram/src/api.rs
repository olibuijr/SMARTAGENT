//! Telegram Bot API over the system `curl` (reliable HTTPS — the httpc
//! `openssl s_client` path intermittently hangs on api.telegram.org and hit
//! the timeout wrapper mid-reply). curl is the same "shell to a system TLS
//! tool" category httpc already uses, and it gives us the fast repeated
//! editMessageText calls that streaming needs.

use std::process::Command;

use httpc::json::{self, Value};

/// POST a JSON body to a Bot API method; returns the parsed `result`.
pub fn call(token: &str, method: &str, body: &str) -> Result<Value, String> {
    let url = format!("https://api.telegram.org/bot{token}/{method}");
    let out = Command::new("curl")
        .args([
            "-s",
            "--max-time",
            "20",
            "-X",
            "POST",
            &url,
            "-H",
            "Content-Type: application/json",
            "-d",
            body,
        ])
        .output()
        .map_err(|e| format!("run curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "curl {method} exit {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let v = json::parse(&String::from_utf8_lossy(&out.stdout))
        .map_err(|e| format!("bad {method} json: {e}"))?;
    if !matches!(v.get("ok"), Some(Value::Bool(true))) {
        return Err(format!(
            "{method} not ok: {}",
            v.get("description").and_then(Value::as_str).unwrap_or("?")
        ));
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

/// getMe → returns the bot username, used to recognize @mentions in groups.
pub fn get_me_username(token: &str) -> Result<String, String> {
    let r = call(token, "getMe", "{}")?;
    r.get("username")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "getMe: no username".into())
}

/// getUpdates (short-poll by default) — returns the raw `result` array value.
pub fn get_updates(token: &str, offset: u64, timeout: u64) -> Result<Value, String> {
    let url =
        format!("https://api.telegram.org/bot{token}/getUpdates?offset={offset}&timeout={timeout}");
    let out = Command::new("curl")
        .args(["-s", "--max-time", &(timeout + 10).to_string(), &url])
        .output()
        .map_err(|e| format!("run curl: {e}"))?;
    let v = json::parse(&String::from_utf8_lossy(&out.stdout))
        .map_err(|e| format!("bad getUpdates json: {e}"))?;
    if !matches!(v.get("ok"), Some(Value::Bool(true))) {
        return Err(format!(
            "getUpdates not ok: {}",
            v.get("description").and_then(Value::as_str).unwrap_or("?")
        ));
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

/// sendMessage → returns the new message_id (for later edits).
pub fn send_message(token: &str, chat: &str, text: &str, markdown: bool) -> Result<i64, String> {
    send_message_in_thread(token, chat, "", text, markdown)
}

pub fn send_message_in_thread(
    token: &str,
    chat: &str,
    thread: &str,
    text: &str,
    markdown: bool,
) -> Result<i64, String> {
    send_message_with_markup_in_thread(token, chat, thread, text, markdown, None)
}

/// sendMessage with optional raw JSON reply_markup (for inline keyboards).
pub fn send_message_with_markup(
    token: &str,
    chat: &str,
    text: &str,
    markdown: bool,
    reply_markup: Option<&str>,
) -> Result<i64, String> {
    send_message_with_markup_in_thread(token, chat, "", text, markdown, reply_markup)
}

pub fn send_message_with_markup_in_thread(
    token: &str,
    chat: &str,
    thread: &str,
    text: &str,
    markdown: bool,
    reply_markup: Option<&str>,
) -> Result<i64, String> {
    let pm = if markdown {
        r#","parse_mode":"Markdown""#
    } else {
        ""
    };
    let mt = if thread.trim().is_empty() {
        String::new()
    } else {
        format!(r#","message_thread_id":{}"#, thread.trim())
    };
    let rm = reply_markup
        .map(|m| format!(r#","reply_markup":{}"#, m))
        .unwrap_or_default();
    let body = format!(
        r#"{{"chat_id":"{}","text":"{}"{}{}{}}}"#,
        json::escape(chat),
        json::escape(text),
        mt,
        pm,
        rm
    );
    let r = call(token, "sendMessage", &body)?;
    r.get("message_id")
        .and_then(Value::as_f64)
        .map(|f| f as i64)
        .ok_or_else(|| "sendMessage: no message_id".into())
}

/// editMessageText — used repeatedly for streaming (throttle at the caller).
pub fn edit_message(
    token: &str,
    chat: &str,
    message_id: i64,
    text: &str,
    markdown: bool,
) -> Result<(), String> {
    let pm = if markdown {
        r#","parse_mode":"Markdown""#
    } else {
        ""
    };
    let body = format!(
        r#"{{"chat_id":"{}","message_id":{},"text":"{}"{}}}"#,
        json::escape(chat),
        message_id,
        json::escape(text),
        pm
    );
    // Telegram rejects an edit whose text is byte-identical to the current
    // message ("message is not modified") — treat that as success.
    match call(token, "editMessageText", &body) {
        Ok(_) => Ok(()),
        Err(e) if e.contains("not modified") => Ok(()),
        Err(e) => Err(e),
    }
}
