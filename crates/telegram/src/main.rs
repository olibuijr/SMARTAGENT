use std::path::PathBuf;
use std::process::Command;

use httpc::args::flag;
use httpc::json::{self, Value};
use semdb::config::Config;
use semdb::storage::Db;

const ROW_OFFSET: &str = "!offset";
const VEC0: [f32; 1] = [0.0];

fn main() {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(s) => { if !s.is_empty() { println!("{s}"); } }
        Err(e) => { eprintln!("error: {e}"); std::process::exit(1); }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "send" => send(args),
        "poll" => poll(args),
        "listen" => listen(args),
        "statusline" => Ok("ok|telegram".into()),
        _ => Ok(HELP.trim().into()),
    }
}

fn send(args: &[String]) -> Result<String, String> {
    let chat = flag(args, "--chat").ok_or("--chat required")?;
    allow_chat(&chat)?;
    let text = flag(args, "--text").ok_or("--text required")?;
    let token = bot_token()?;
    let chunks = chunks(&text, 4096);
    for c in &chunks {
        let body = format!(r#"{{"chat_id":"{}","text":"{}"}}"#, json::escape(&chat), json::escape(c));
        let url = format!("https://api.telegram.org/bot{token}/sendMessage");
        let v = httpc::post_json(&url, &body)?;
        if !is_ok(&v) { return Err(format!("sendMessage failed: {v:?}")); }
    }
    Ok(format!("sent {} chunk(s)", chunks.len()))
}

fn poll(args: &[String]) -> Result<String, String> {
    let token = bot_token()?;
    let timeout = flag(args, "--timeout").unwrap_or_else(|| "25".into());
    let offset = offset()?;
    let url = format!("https://api.telegram.org/bot{token}/getUpdates?offset={offset}&timeout={timeout}");
    let v = httpc::request("GET", &url).timeout(timeout.parse::<u64>().unwrap_or(25) + 10).send()?.json()?;
    if !is_ok(&v) { return Err(format!("getUpdates failed: {v:?}")); }
    let mut max_id = offset.saturating_sub(1);
    let mut out = Vec::new();
    if let Some(items) = v.get("result").and_then(Value::as_arr) {
        for it in items {
            let uid = u64v(it.get("update_id")).unwrap_or(0);
            max_id = max_id.max(uid);
            let Some(msg) = it.get("message") else { continue };
            let chat = msg.get("chat").and_then(|c| c.get("id")).map(val_s).unwrap_or_default();
            if allow_chat(&chat).is_err() { continue; }
            let text = msg.get("text").and_then(Value::as_str).unwrap_or("");
            let from = msg.get("from").and_then(|f| f.get("username")).and_then(Value::as_str).unwrap_or("");
            out.push(format!(r#"{{"update_id":{uid},"chat":"{}","from":"{}","text":"{}"}}"#, json::escape(&chat), json::escape(from), json::escape(text)));
        }
    }
    set_offset(max_id + 1)?;
    Ok(out.join("\n"))
}

fn listen(args: &[String]) -> Result<String, String> {
    let sleep: u64 = flag(args, "--sleep").and_then(|s| s.parse().ok()).unwrap_or(2);
    let gateway_agent = flag(args, "--gateway");
    loop {
        let out = poll(&["poll".into(), "--timeout".into(), "25".into()])?;
        for line in out.lines().filter(|l| !l.trim().is_empty()) {
            if let Some(agent) = gateway_agent.as_deref() {
                gateway_send(agent, line)?;
            } else {
                println!("{line}");
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(sleep));
    }
}

fn gateway_send(agent: &str, msg: &str) -> Result<(), String> {
    let st = Command::new("target/release/gateway")
        .args(["send", "--agent", agent, msg])
        .status().map_err(|e| format!("run gateway send: {e}"))?;
    if st.success() { Ok(()) } else { Err(format!("gateway send failed with status {st}")) }
}

fn bot_token() -> Result<String, String> {
    let out = Command::new("target/release/secrets")
        .args(["get", "--store", "data/secrets", "--name", "telegram_bot_token", "--as", "pi"])
        .output().map_err(|e| format!("run secrets get: {e}"))?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).trim().to_string()); }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn allow_chat(chat: &str) -> Result<(), String> {
    let cfg = Config::load();
    let allowed = cfg.resolve("telegram_allowed_chats", "SMARTAGENT_TELEGRAM_ALLOWED_CHATS", None).unwrap_or_default();
    if allowed.split(',').map(str::trim).any(|c| c == chat) { Ok(()) } else { Err(format!("chat {chat} not allowed")) }
}

fn db_path() -> PathBuf { Config::load().data_dir().join("telegram.semdb") }
fn open_db() -> Result<Db, String> { let p = db_path(); if let Some(d)=p.parent(){let _=std::fs::create_dir_all(d);} if p.exists() { Db::open(&p) } else { Db::create(&p) } }
fn offset() -> Result<u64, String> { Ok(open_db()?.get(ROW_OFFSET).and_then(|e| json::parse(&e.meta).ok()).and_then(|v| u64v(v.get("offset"))).unwrap_or(0)) }
fn set_offset(n: u64) -> Result<(), String> { let mut db = open_db()?; db.put(ROW_OFFSET, &format!(r#"{{"offset":{n}}}"#), VEC0.to_vec()) }

fn u64v(v: Option<&Value>) -> Option<u64> { v.and_then(Value::as_f64).map(|x| x.max(0.0) as u64) }
fn is_ok(v: &Value) -> bool { matches!(v.get("ok"), Some(Value::Bool(true))) }
fn val_s(v: &Value) -> String { match v { Value::Str(s) => s.clone(), _ => format!("{}", v.as_f64().unwrap_or(0.0) as i64) } }

fn chunks(s: &str, max: usize) -> Vec<String> {
    if s.is_empty() { return vec![String::new()]; }
    let mut out = Vec::new();
    let mut rest = s;
    while rest.chars().count() > max {
        let mut cut = rest.char_indices().nth(max).map(|(i, _)| i).unwrap_or(rest.len());
        if let Some(nl) = rest[..cut].rfind('\n') { if nl > 0 { cut = nl + 1; } }
        out.push(rest[..cut].to_string());
        rest = &rest[cut..];
    }
    out.push(rest.to_string());
    out
}

const HELP: &str = r#"
telegram — Telegram Bot API bridge

USAGE:
  telegram send --chat ID --text TEXT
  telegram poll [--timeout 25]
  telegram listen [--sleep 2] [--gateway AGENT]

Token is read only via secrets get name=telegram_bot_token as caller pi.
Allowed chats come from config/smartagent.conf telegram_allowed_chats.
"#;

#[cfg(test)]
mod tests {
    use super::chunks;

    #[test]
    fn chunks_long_messages() {
        let s = "x".repeat(9000);
        let c = chunks(&s, 4096);
        assert_eq!(c.len(), 3);
        assert!(c.iter().all(|x| x.chars().count() <= 4096));
    }
}
