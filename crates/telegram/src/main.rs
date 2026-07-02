mod api;

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

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
        "commands" => register_commands(),
        "broadcast" => broadcast(args),
        "statusline" => Ok("ok|telegram".into()),
        _ => Ok(HELP.trim().into()),
    }
}

fn send(args: &[String]) -> Result<String, String> {
    let chat = flag(args, "--chat").ok_or("--chat required")?;
    allow_chat(&chat)?;
    let text = flag(args, "--text").ok_or("--text required")?;
    let md = args.iter().any(|a| a == "--markdown");
    let token = bot_token()?;
    let chunks = chunks(&text, 4096);
    for c in &chunks {
        api::send_message(&token, &chat, c, md)?;
    }
    Ok(format!("sent {} chunk(s)", chunks.len()))
}

/// Broadcast a styled report to the allowed chats (Markdown). Used by the
/// task-completion hook: `telegram broadcast --text "..."`.
fn broadcast(args: &[String]) -> Result<String, String> {
    let text = flag(args, "--text").ok_or("--text required")?;
    let token = bot_token()?;
    let chats = Config::load().resolve("telegram_allowed_chats", "SMARTAGENT_TELEGRAM_CHATS", None).unwrap_or_default();
    let mut n = 0;
    for chat in chats.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        for c in chunks(&text, 4096) {
            let _ = api::send_message(&token, chat, &c, true);
        }
        n += 1;
    }
    Ok(format!("broadcast to {n} chat(s)"))
}

fn register_commands() -> Result<String, String> {
    let token = bot_token()?;
    let body = command_menu_body();
    api::call(&token, "setMyCommands", &body)?;
    Ok(format!("registered {} Telegram command(s). If your Telegram client still shows the old menu, close and reopen the chat (or restart the app) to refresh its cached commands.", TELEGRAM_COMMANDS.len()))
}

fn poll(args: &[String]) -> Result<String, String> {
    let token = bot_token()?;
    let timeout = flag(args, "--timeout").unwrap_or_else(|| "25".into());
    let offset = offset()?;
    let result = api::get_updates(&token, offset, timeout.parse::<u64>().unwrap_or(0))?;
    let mut max_id = offset.saturating_sub(1);
    let mut out = Vec::new();
    if let Some(items) = result.as_arr() {
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
    eprintln!("[tg] listen up (gateway={:?}, poll=short, sleep={sleep}s)", gateway_agent);
    let mut backoff = sleep;
    loop {
        // Long-poll (25s) over curl — reliable, and holds the connection so
        // updates arrive promptly. A bridge must SURVIVE transient errors:
        // log + backoff, never exit (exiting is what showed as DOWN).
        match poll(&["poll".into(), "--timeout".into(), "25".into()]) {
            Ok(out) => {
                backoff = sleep;
                for line in out.lines().filter(|l| !l.trim().is_empty()) {
                    handle_inbound(gateway_agent.as_deref(), line);
                }
            }
            Err(e) => {
                eprintln!("[tg] poll error: {e} — retrying in {backoff}s");
                std::thread::sleep(std::time::Duration::from_secs(backoff));
                backoff = (backoff * 2).min(30);
                continue;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(sleep));
    }
}

/// One inbound update: relay to the gateway agent, send its reply back to the
/// chat. Every step logged to stderr → `supervise logs telegram`.
fn handle_inbound(gateway_agent: Option<&str>, line: &str) {
    let v = match json::parse(line) { Ok(v) => v, Err(_) => return };
    let chat = v.get("chat").and_then(Value::as_str).unwrap_or("").to_string();
    let from = v.get("from").and_then(Value::as_str).unwrap_or("?").to_string();
    let text = v.get("text").and_then(Value::as_str).unwrap_or("").to_string();
    eprintln!("[tg] inbound from @{from} ({chat}): {}", &text.chars().take(60).collect::<String>());
    if let Some(result) = slash_command(&text) {
        let reply = result.unwrap_or_else(|e| format!("command error: {e}"));
        eprintln!("[tg] slash /{}: {} chars", text.split_whitespace().next().unwrap_or(""), reply.len());
        match send(&["send".into(), "--chat".into(), chat.clone(), "--text".into(), reply]) {
            Ok(r) => eprintln!("[tg] slash relayed to {chat}: {r}"),
            Err(e) => eprintln!("[tg] slash sendMessage FAILED for {chat}: {e}"),
        }
        return;
    }
    let Some(agent) = gateway_agent else { println!("{line}"); return };
    if let Err(e) = stream_reply(agent, &from, &text, &chat) {
        eprintln!("[tg] stream FAILED: {e}");
        let _ = send(&["send".into(), "--chat".into(), chat, "--text".into(), format!("⚠ agent unavailable: {e}")]);
    }
}

/// Streaming reply (hermes-style): send a placeholder, then live-edit it as
/// the agent generates. `gateway ask --stream` prints growing snapshots; we
/// throttle editMessageText to ~1 edit / 1.5s to respect Telegram rate limits.
fn stream_reply(agent: &str, from: &str, text: &str, chat: &str) -> Result<(), String> {
    let token = bot_token()?;
    let prompt = format!(
        "Telegram message from @{from}: {text}\n(Reply concisely — your reply is streamed straight to Telegram.)"
    );
    let mid = api::send_message(&token, chat, "💭 …", false)?;
    let mut child = Command::new("target/release/gateway")
        .args(["ask", "--agent", agent, "--timeout", "90", "--stream", &prompt])
        .stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn().map_err(|e| format!("spawn gateway ask: {e}"))?;
    let out = child.stdout.take().ok_or("no gateway stdout")?;
    let mut last_edit = std::time::Instant::now();
    let mut latest = String::new();
    let mut shown = String::new();
    for line in BufReader::new(out).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() { continue; }
        latest = line.replace("\\n", "\n"); // un-escape the streamed snapshot
        // Throttle: edit at most every 1.5s, only when the text grew.
        if latest != shown && last_edit.elapsed().as_millis() >= 1500 {
            let _ = api::edit_message(&token, chat, mid, &clip_tg(&latest), false);
            shown = latest.clone();
            last_edit = std::time::Instant::now();
        }
    }
    let _ = child.wait();
    // Final edit with the complete reply (Markdown for the finished message).
    let final_text = if latest.trim().is_empty() { "(the agent had no reply)".to_string() } else { latest };
    api::edit_message(&token, chat, mid, &clip_tg(&final_text), true)
        .or_else(|_| api::edit_message(&token, chat, mid, &clip_tg(&final_text), false))?;
    eprintln!("[tg] streamed reply to {chat}: {} chars", final_text.len());
    Ok(())
}

/// Telegram single-message cap is 4096 chars — keep a margin.
fn clip_tg(s: &str) -> String {
    if s.chars().count() > 4000 {
        format!("{}…", s.chars().take(4000).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Telegram slash commands run the platform binaries directly (no LLM turn) and
/// relay their output — same set as the TUI slash commands.
struct BotCommand {
    name: &'static str,
    description: &'static str,
}

const TELEGRAM_COMMANDS: &[BotCommand] = &[
    BotCommand { name: "help", description: "Show SMARTAGENT Telegram commands" },
    BotCommand { name: "commands", description: "Show SMARTAGENT Telegram commands" },
    BotCommand { name: "board", description: "Show the kanban board" },
    BotCommand { name: "tasks", description: "List ready tasks" },
    BotCommand { name: "status", description: "Show supervised service status" },
    BotCommand { name: "skills", description: "List skills or match a query" },
    BotCommand { name: "agents", description: "Show gateway fleet state" },
    BotCommand { name: "runs", description: "Show active workflows" },
    BotCommand { name: "memory", description: "Recall SMARTAGENT memory" },
];

fn command_help() -> String {
    let mut out = String::from("SMARTAGENT bot. Talk to me normally, or use:");
    for c in TELEGRAM_COMMANDS {
        out.push_str(&format!("\n/{} — {}", c.name, c.description));
    }
    out
}

fn command_menu_body() -> String {
    let commands = TELEGRAM_COMMANDS
        .iter()
        .map(|c| format!(r#"{{"command":"{}","description":"{}"}}"#, json::escape(c.name), json::escape(c.description)))
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"commands":[{commands}]}}"#)
}

fn slash_command(text: &str) -> Option<Result<String, String>> {
    let mut parts = text.trim().splitn(2, char::is_whitespace);
    let raw_cmd = parts.next()?.strip_prefix('/')?;
    let cmd = raw_cmd.split('@').next().unwrap_or(raw_cmd);
    let arg = parts.next().unwrap_or("").trim();
    let run = |bin: &str, args: &[&str]| -> Result<String, String> {
        let out = Command::new(format!("target/release/{bin}"))
            .args(args).output().map_err(|e| format!("{bin}: {e}"))?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(if s.is_empty() { "(no output)".into() } else { s })
    };
    Some(match cmd {
        "start" | "help" | "commands" => Ok(command_help()),
        "board" => run("tasks", &["board", "--db", "data/tasks.semdb"]),
        "tasks" => run("tasks", &["list", "--col", "ready", "--db", "data/tasks.semdb"]),
        "status" => run("supervise", &["status"]),
        "agents" => run("gateway", &["agents"]),
        "runs" => run("workflow", &["runs", "--live", "--db", "data/workflow.semdb"]),
        "skills" => run("skills", if arg.is_empty() { vec!["list"] } else { vec!["match", arg] }.as_slice()),
        "memory" if !arg.is_empty() => run("memory", &["recall", "--dir", "data/memory", "--text", arg]),
        _ => return None, // unknown slash → treat as normal chat
    })
}

fn bot_token() -> Result<String, String> {
    // Caller auth: the ./pi launcher injects SMARTAGENT_CALLER_TOKEN, but the
    // supervised listener is spawned by supervise WITHOUT it — fall back to
    // the token file (same trust domain; tmpfs-masked inside the sandbox, so
    // sandboxed commands still can't present it).
    let mut cmd = Command::new("target/release/secrets");
    cmd.args(["get", "--store", "data/secrets", "--name", "telegram_bot_token", "--as", "pi"]);
    if std::env::var("SMARTAGENT_CALLER_TOKEN").is_err() {
        if let Ok(tok) = std::fs::read_to_string("data/secrets/tokens/pi.token") {
            cmd.env("SMARTAGENT_CALLER_TOKEN", tok.trim());
        }
    }
    let out = cmd.output().map_err(|e| format!("run secrets get: {e}"))?;
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
  telegram commands                 register Telegram slash-command menu

Token is read only via secrets get name=telegram_bot_token as caller pi.
Allowed chats come from config/smartagent.conf telegram_allowed_chats.

Command menu refresh: `telegram commands` calls Bot API setMyCommands for the
supported Telegram slash commands. Telegram clients can cache the old command
menu; if changes do not appear immediately, close/reopen the bot chat or restart
the Telegram app.
"#;

#[cfg(test)]
mod tests {
    use super::{chunks, command_help, command_menu_body, slash_command, TELEGRAM_COMMANDS};

    #[test]
    fn command_menu_lists_supported_slashes() {
        let body = command_menu_body();
        let help = command_help();
        for c in TELEGRAM_COMMANDS {
            assert!(body.contains(&format!("\"command\":\"{}\"", c.name)), "{body}");
            assert!(help.contains(&format!("/{}", c.name)), "{help}");
        }
        assert!(body.contains("\"commands\""));
    }

    #[test]
    fn slash_commands_accept_bot_suffix() {
        let out = slash_command("/help@smartagent_bot").expect("recognized").unwrap();
        assert!(out.contains("/board"), "{out}");
        assert!(slash_command("/unknown@smartagent_bot").is_none());
    }

    #[test]
    fn chunks_long_messages() {
        let s = "x".repeat(9000);
        let c = chunks(&s, 4096);
        assert_eq!(c.len(), 3);
        assert!(c.iter().all(|x| x.chars().count() <= 4096));
    }
}
