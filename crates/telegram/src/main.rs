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
        "diag" => diag(args),
        "chat" => chat_info(args),
        "member" => member_info(args),
        "updates" => updates(args),
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
        .resolve(
            "telegram_allowed_chats",
            "SMARTAGENT_TELEGRAM_ALLOWED_CHATS",
            None,
        )
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

fn diag(args: &[String]) -> Result<String, String> {
    let token = bot_token()?;
    let me = api::call(&token, "getMe", "{}")?;
    let username = me.get("username").and_then(Value::as_str).unwrap_or("?");
    let allowed = Config::load()
        .resolve(
            "telegram_allowed_chats",
            "SMARTAGENT_TELEGRAM_ALLOWED_CHATS",
            None,
        )
        .unwrap_or_default();
    let chat = flag(args, "--chat");
    let mut out = Vec::new();
    out.push(format!(
        "bot=@{username} id={} can_join_groups={} can_read_all_group_messages={}",
        val_s(me.get("id").unwrap_or(&Value::Null)),
        bool_s(me.get("can_join_groups")),
        bool_s(me.get("can_read_all_group_messages"))
    ));
    out.push(format!(
        "allowed_chats={}",
        if allowed.trim().is_empty() {
            "(none)"
        } else {
            allowed.trim()
        }
    ));
    if let Some(chat_id) = chat {
        match api::call(
            &token,
            "getChat",
            &format!(r#"{{"chat_id":"{}"}}"#, json::escape(&chat_id)),
        ) {
            Ok(c) => out.push(format_chat_summary("chat", &c)),
            Err(e) => out.push(format!("chat error: {e}")),
        }
        let bot_id = val_s(me.get("id").unwrap_or(&Value::Null));
        match api::call(
            &token,
            "getChatMember",
            &format!(
                r#"{{"chat_id":"{}","user_id":{}}}"#,
                json::escape(&chat_id),
                bot_id
            ),
        ) {
            Ok(m) => out.push(format_member_summary("bot_member", &m)),
            Err(e) => out.push(format!("bot_member error: {e}")),
        }
    } else {
        out.push("pass --chat <id-or-@username> to check getChat/getChatMember".into());
    }
    out.push("If group/channel mentions do not arrive, send /board@<botusername> there, then run `telegram updates --limit 10 --no-advance` to discover the real negative chat id. Invite links (t.me/+...) are not Bot API chat ids.".into());
    Ok(out.join("\n"))
}

fn chat_info(args: &[String]) -> Result<String, String> {
    let chat = flag(args, "--chat").ok_or("--chat required")?;
    let token = bot_token()?;
    let v = api::call(
        &token,
        "getChat",
        &format!(r#"{{"chat_id":"{}"}}"#, json::escape(&chat)),
    )?;
    Ok(format_chat_summary("chat", &v))
}

fn member_info(args: &[String]) -> Result<String, String> {
    let chat = flag(args, "--chat").ok_or("--chat required")?;
    let user = flag(args, "--user").unwrap_or_else(|| "me".into());
    let token = bot_token()?;
    let user_id = if user == "me" {
        val_s(
            api::call(&token, "getMe", "{}")?
                .get("id")
                .unwrap_or(&Value::Null),
        )
    } else {
        user
    };
    let v = api::call(
        &token,
        "getChatMember",
        &format!(
            r#"{{"chat_id":"{}","user_id":{}}}"#,
            json::escape(&chat),
            user_id
        ),
    )?;
    Ok(format_member_summary("member", &v))
}

fn updates(args: &[String]) -> Result<String, String> {
    let token = bot_token()?;
    let limit = flag(args, "--limit")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10usize);
    let no_advance = args.iter().any(|a| a == "--no-advance");
    let offset_v = if no_advance { 0 } else { offset()? };
    let result = api::get_updates(&token, offset_v, 0)?;
    let mut lines = Vec::new();
    let mut max_id = offset_v.saturating_sub(1);
    if let Some(items) = result.as_arr() {
        for it in items.iter().rev().take(limit).rev() {
            let uid = u64v(it.get("update_id")).unwrap_or(0);
            max_id = max_id.max(uid);
            lines.push(format_update_summary(it));
        }
    }
    if !no_advance && max_id >= offset_v {
        set_offset(max_id + 1)?;
    }
    if lines.is_empty() {
        Ok("no pending updates".into())
    } else {
        Ok(lines.join("\n"))
    }
}

fn poll(args: &[String]) -> Result<String, String> {
    let token = bot_token()?;
    let timeout = flag(args, "--timeout").unwrap_or_else(|| "25".into());
    let offset = offset()?;
    let me = api::call(&token, "getMe", "{}").ok();
    let bot_username = me
        .as_ref()
        .and_then(|v| v.get("username"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let bot_id = me
        .as_ref()
        .and_then(|v| v.get("id"))
        .map(val_s)
        .unwrap_or_default();
    let result = api::get_updates(&token, offset, timeout.parse::<u64>().unwrap_or(0))?;
    let mut max_id = offset.saturating_sub(1);
    let mut out = Vec::new();
    if let Some(items) = result.as_arr() {
        for it in items {
            let uid = u64v(it.get("update_id")).unwrap_or(0);
            max_id = max_id.max(uid);
            let msg = if let Some(msg) = it.get("message") {
                msg
            } else if let Some(msg) = it.get("channel_post") {
                msg
            } else if let Some(msg) = it.get("edited_channel_post") {
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
            let reply_to_bot = is_reply_to_bot(msg, &bot_id, bot_username.as_deref().unwrap_or(""));
            // Hard allow-list gate FIRST: an @-mention is a noise filter within
            // an allowed chat, never a way in. Previously `is_err() && !mention_ok`
            // let ANY untrusted group grant full agent access just by @-mentioning
            // the bot. Now a non-allow-listed chat is always rejected; within an
            // allowed group we additionally require the bot be addressed.
            if allow_chat(&chat).is_err() {
                let title = msg
                    .get("chat")
                    .and_then(|c| c.get("title"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                eprintln!(
                    "[tg] ignored unallowed chat={} type={} title={} text={}",
                    chat,
                    chat_type,
                    title,
                    raw_text.chars().take(80).collect::<String>()
                );
                continue;
            }
            let is_group = matches!(chat_type, "group" | "supergroup" | "channel");
            if is_group && !mention_ok && !reply_to_bot {
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

fn bool_s(v: Option<&Value>) -> &'static str {
    match v {
        Some(Value::Bool(true)) => "true",
        Some(Value::Bool(false)) => "false",
        _ => "?",
    }
}

fn format_chat_summary(label: &str, v: &Value) -> String {
    let title = v
        .get("title")
        .and_then(Value::as_str)
        .or_else(|| v.get("username").and_then(Value::as_str))
        .unwrap_or("");
    format!(
        "{label}: id={} type={} title={} username=@{} forum={}",
        val_s(v.get("id").unwrap_or(&Value::Null)),
        v.get("type").and_then(Value::as_str).unwrap_or("?"),
        title,
        v.get("username").and_then(Value::as_str).unwrap_or(""),
        bool_s(v.get("is_forum"))
    )
}

fn format_member_summary(label: &str, v: &Value) -> String {
    let status = v.get("status").and_then(Value::as_str).unwrap_or("?");
    format!(
        "{label}: status={} can_post_messages={} can_manage_chat={} can_delete_messages={}",
        status,
        bool_s(v.get("can_post_messages")),
        bool_s(v.get("can_manage_chat")),
        bool_s(v.get("can_delete_messages"))
    )
}

fn format_update_summary(it: &Value) -> String {
    let uid = u64v(it.get("update_id")).unwrap_or(0);
    for key in [
        "message",
        "edited_message",
        "channel_post",
        "edited_channel_post",
        "my_chat_member",
    ] {
        if let Some(v) = it.get(key) {
            let msg = if key == "my_chat_member" { v } else { v };
            let chat = msg.get("chat");
            let chat_id = chat
                .and_then(|c| c.get("id"))
                .map(val_s)
                .unwrap_or_default();
            let chat_type = chat
                .and_then(|c| c.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let title = chat
                .and_then(|c| c.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let thread = msg.get("message_thread_id").map(val_s).unwrap_or_default();
            let text = msg.get("text").and_then(Value::as_str).unwrap_or("");
            return format!("update={uid} kind={key} chat={chat_id} type={chat_type} thread={thread} title={title} text={}", text.chars().take(80).collect::<String>());
        }
    }
    format!("update={uid} kind=other")
}

fn is_reply_to_bot(msg: &Value, bot_id: &str, bot_username: &str) -> bool {
    let Some(reply) = msg.get("reply_to_message") else {
        return false;
    };
    let Some(from) = reply.get("from") else {
        return false;
    };
    let is_bot = matches!(from.get("is_bot"), Some(Value::Bool(true)));
    if !is_bot {
        return false;
    }
    let reply_id = from.get("id").map(val_s).unwrap_or_default();
    if !bot_id.is_empty() && reply_id == bot_id {
        return true;
    }
    let reply_username = from.get("username").and_then(Value::as_str).unwrap_or("");
    !bot_username.is_empty() && reply_username.eq_ignore_ascii_case(bot_username)
}

fn is_group_mention(chat_type: &str, text: &str, bot_username: &str) -> bool {
    matches!(chat_type, "group" | "supergroup" | "channel")
        && contains_bot_mention(text, bot_username)
}

fn contains_bot_mention(text: &str, bot_username: &str) -> bool {
    let username = bot_username.trim_start_matches('@').to_ascii_lowercase();
    let needle = format!("@{}", username);
    text.split_whitespace().any(|part| {
        let cleaned = part
            .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '@' && c != '/')
            .to_ascii_lowercase();
        cleaned.eq_ignore_ascii_case(&needle) || cleaned.ends_with(&needle)
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
pub(crate) fn test_db(name: &str) -> semdb::storage::Db {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch")
        .join(name);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("telegram.semdb");
    let _ = std::fs::remove_file(&path);
    semdb::storage::Db::create(&path).unwrap()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_history;
