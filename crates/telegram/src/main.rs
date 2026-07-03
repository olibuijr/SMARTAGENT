mod api;
mod blocked;
mod commands;
mod inbound;

use blocked::*;
use commands::*;
use inbound::*;

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use httpc::args::flag;
use httpc::json::{self, Value};
use semdb::config::Config;
use semdb::storage::Db;

const ROW_OFFSET: &str = "!offset";
const VEC0: [f32; 1] = [0.0];
const HISTORY_KEEP_PER_SCOPE: usize = 50;
const CALLBACK_MAX_AGE_SECS: u64 = 86_400;
const GATEWAY_STREAM_INFO_PREFIX: &str = "__SMARTAGENT_INFO__";
const GATEWAY_STREAM_THINKING_PREFIX: &str = "__SMARTAGENT_THINKING__";
const TELEGRAM_MODELS: &[&str] = &[
    "codex/gpt-5.5",
    "codex/gpt-5.4-mini",
    "claude/sonnet-4.5",
    "opencode-go/qwen3-coder",
];

fn main() {
    match run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(s) => {
            if !s.is_empty() {
                println!("{s}");
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str).unwrap_or("help") {
        "send" => send(args),
        "poll" => poll(args),
        "listen" => listen(args),
        "commands" => register_commands(),
        "broadcast" => broadcast(args),
        "blocked-scan" => blocked_scan(args),
        "status" => telegram_status(args),
        "statusline" => Ok("ok|telegram".into()),
        _ => Ok(HELP.trim().into()),
    }
}

fn telegram_status(args: &[String]) -> Result<String, String> {
    let limit = flag(args, "--limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(8usize);
    Ok(telegram_status_report(limit))
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
    let kind = flag(args, "--kind").unwrap_or_else(|| "normal".into());
    let token = bot_token()?;
    let chats = Config::load()
        .resolve("telegram_allowed_chats", "SMARTAGENT_TELEGRAM_CHATS", None)
        .unwrap_or_default();
    let mut n = 0;
    for chat in chats.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        if !notification_allowed(chat, "", "*", &kind) {
            continue;
        }
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
    Ok(format!("registered {} Telegram command(s). If your Telegram client still shows the old menu, close and reopen the chat (or restart the app) to refresh its cached commands.", telegram_commands().len()))
}

fn poll(args: &[String]) -> Result<String, String> {
    let token = bot_token()?;
    let timeout = flag(args, "--timeout").unwrap_or_else(|| "25".into());
    let offset = offset()?;
    let bot_username = api::get_me_username(&token).ok();
    let result = api::get_updates(&token, offset, timeout.parse::<u64>().unwrap_or(0))?;
    let mut max_id = offset.saturating_sub(1);
    let mut out = Vec::new();
    if let Some(items) = result.as_arr() {
        for it in items {
            let uid = u64v(it.get("update_id")).unwrap_or(0);
            max_id = max_id.max(uid);
            let msg = if let Some(msg) = it.get("message") {
                msg
            } else if let Some(cb) = it.get("callback_query") {
                let Some(msg) = cb.get("message") else {
                    continue;
                };
                let chat = msg
                    .get("chat")
                    .and_then(|c| c.get("id"))
                    .map(val_s)
                    .unwrap_or_default();
                if allow_chat(&chat).is_err() {
                    continue;
                }
                let data = cb.get("data").and_then(Value::as_str).unwrap_or("");
                let date = u64v(msg.get("date")).unwrap_or_else(unix_secs);
                if let Some(action) = block_callback_action(data) {
                    let callback_id = cb.get("id").and_then(Value::as_str).unwrap_or("");
                    let message_id = msg.get("message_id").map(val_s).unwrap_or_default();
                    let _ = handle_block_callback(&token, callback_id, &chat, &message_id, action);
                    continue;
                }
                let now = unix_secs();
                if let Some(role) = callback_agent_assignment(data, date, now) {
                    let callback_id = cb.get("id").and_then(Value::as_str).unwrap_or("");
                    let _ = api::call(
                        &token,
                        "answerCallbackQuery",
                        &format!(
                            r#"{{"callback_query_id":"{}","text":"Assigned to {}"}}"#,
                            json::escape(callback_id),
                            json::escape(role)
                        ),
                    );
                    let message_id = msg.get("message_id").map(val_s).unwrap_or_default();
                    let _ = api::edit_message(
                        &token,
                        &chat,
                        message_id.parse::<i64>().unwrap_or(0),
                        &format!("Assigned to {role}. A coordinator can now pull this from the Telegram context."),
                        false,
                    );
                    continue;
                }
                let Some(model) = callback_model_choice(data, date, now) else {
                    continue;
                };
                let from = cb
                    .get("from")
                    .and_then(|f| f.get("username"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let user = cb
                    .get("from")
                    .and_then(|f| f.get("id"))
                    .map(val_s)
                    .unwrap_or_default();
                let message_id = msg.get("message_id").map(val_s).unwrap_or_default();
                let thread = msg.get("message_thread_id").map(val_s).unwrap_or_default();
                out.push(format!(r#"{{"update_id":{uid},"chat":"{}","thread":"{}","message_id":"{}","user":"{}","from":"{}","date":{},"text":"/model {}"}}"#, json::escape(&chat), json::escape(&thread), json::escape(&message_id), json::escape(&user), json::escape(from), date, json::escape(model)));
                continue;
            } else {
                continue;
            };
            let chat = msg
                .get("chat")
                .and_then(|c| c.get("id"))
                .map(val_s)
                .unwrap_or_default();
            let raw_text = msg.get("text").and_then(Value::as_str).unwrap_or("");
            let chat_type = msg
                .get("chat")
                .and_then(|c| c.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let mention_ok = bot_username
                .as_deref()
                .map(|name| is_group_mention(chat_type, raw_text, name))
                .unwrap_or(false);
            if allow_chat(&chat).is_err() && !mention_ok {
                continue;
            }
            let text = if mention_ok {
                bot_username
                    .as_deref()
                    .map(|name| strip_bot_mention(raw_text, name))
                    .unwrap_or_else(|| raw_text.to_string())
            } else {
                raw_text.to_string()
            };
            let from = msg
                .get("from")
                .and_then(|f| f.get("username"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let user = msg
                .get("from")
                .and_then(|f| f.get("id"))
                .map(val_s)
                .unwrap_or_default();
            let message_id = msg.get("message_id").map(val_s).unwrap_or_default();
            let thread = msg.get("message_thread_id").map(val_s).unwrap_or_default();
            let date = u64v(msg.get("date")).unwrap_or_else(unix_secs);
            out.push(format!(r#"{{"update_id":{uid},"chat":"{}","thread":"{}","message_id":"{}","user":"{}","from":"{}","date":{},"text":"{}"}}"#, json::escape(&chat), json::escape(&thread), json::escape(&message_id), json::escape(&user), json::escape(from), date, json::escape(&text)));
        }
    }
    set_offset(max_id + 1)?;
    Ok(out.join("\n"))
}

fn is_group_mention(chat_type: &str, text: &str, bot_username: &str) -> bool {
    matches!(chat_type, "group" | "supergroup") && contains_bot_mention(text, bot_username)
}

fn contains_bot_mention(text: &str, bot_username: &str) -> bool {
    let needle = format!("@{}", bot_username.trim_start_matches('@')).to_ascii_lowercase();
    text.split_whitespace().any(|part| {
        part.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '@')
            .eq_ignore_ascii_case(&needle)
    })
}

fn strip_bot_mention(text: &str, bot_username: &str) -> String {
    let needle = format!("@{}", bot_username.trim_start_matches('@')).to_ascii_lowercase();
    let stripped = text
        .split_whitespace()
        .filter(|part| {
            !part
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '@')
                .eq_ignore_ascii_case(&needle)
        })
        .collect::<Vec<_>>()
        .join(" ");
    if stripped.trim().is_empty() {
        text.trim().to_string()
    } else {
        stripped
    }
}

#[cfg(test)]
mod tests;
