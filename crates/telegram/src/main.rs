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
    let token = bot_token()?;
    let chats = Config::load()
        .resolve("telegram_allowed_chats", "SMARTAGENT_TELEGRAM_CHATS", None)
        .unwrap_or_default();
    let mut n = 0;
    for chat in chats.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        for c in chunks(&text, 4096) {
            let _ = api::send_message(&token, chat, &c, true);
        }
        n += 1;
    }
    Ok(format!("broadcast to {n} chat(s)"))
}

fn blocked_scan(args: &[String]) -> Result<String, String> {
    let out = Command::new("target/release/tasks")
        .args(["list", "--blocked"])
        .output()
        .map_err(|e| format!("tasks list --blocked: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let ids = blocked_task_ids(&text);
    if args.iter().any(|a| a == "--dry-run") {
        let sample = ids
            .first()
            .map(|id| blocked_keyboard(id))
            .unwrap_or_else(|| blocked_keyboard("T-0"));
        return Ok(format!(
            "blocked alerts dry-run: {} task(s); sample_markup={sample}",
            ids.len()
        ));
    }
    let token = bot_token()?;
    let chats = flag(args, "--chat").unwrap_or_else(allowed_chats_csv);
    let mut sent = 0usize;
    for chat in chats.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        allow_chat(chat)?;
        for id in &ids {
            let body = blocked_alert_text(id);
            api::send_message_with_markup(&token, chat, &body, false, Some(&blocked_keyboard(id)))?;
            sent += 1;
        }
    }
    Ok(format!("blocked alerts sent: {sent}"))
}

fn allowed_chats_csv() -> String {
    Config::load()
        .resolve(
            "telegram_allowed_chats",
            "SMARTAGENT_TELEGRAM_ALLOWED_CHATS",
            None,
        )
        .unwrap_or_default()
}

fn blocked_task_ids(list_output: &str) -> Vec<String> {
    list_output
        .lines()
        .filter(|l| !l.trim().is_empty() && *l != "no tasks")
        .filter_map(|l| l.split('\t').next())
        .filter(|id| id.starts_with('T'))
        .map(str::to_string)
        .collect()
}

fn blocked_alert_text(id: &str) -> String {
    let show = Command::new("target/release/tasks")
        .args(["show", id])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_else(|| id.to_string());
    format!("⛔ Blocked task needs resolution\n{}", clip_tg(&show))
}

fn blocked_keyboard(id: &str) -> String {
    format!(
        r#"{{"inline_keyboard":[[{u},{r}],[{d},{t}]]}}"#,
        u = block_button("Unblock", "unblock", id),
        r = block_button("Reassign", "reassign", id),
        d = block_button("Drop", "drop", id),
        t = block_button("Own text", "text", id),
    )
}

fn block_button(label: &str, action: &str, id: &str) -> String {
    format!(
        r#"{{"text":"{}","callback_data":"block:{}:{}"}}"#,
        json::escape(label),
        json::escape(action),
        json::escape(id)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BlockAction {
    action: String,
    id: String,
}

fn block_callback_action(data: &str) -> Option<BlockAction> {
    let rest = data.strip_prefix("block:")?;
    let mut parts = rest.splitn(2, ':');
    let action = parts.next()?.to_string();
    let id = parts.next()?.to_string();
    if id.starts_with('T') {
        Some(BlockAction { action, id })
    } else {
        None
    }
}

fn handle_block_callback(
    token: &str,
    callback_id: &str,
    chat: &str,
    message_id: &str,
    act: BlockAction,
) -> Result<(), String> {
    let outcome = apply_block_action(&act)?;
    if !callback_id.is_empty() {
        let _ = answer_callback(token, callback_id, &outcome);
    }
    if let Ok(mid) = message_id.parse::<i64>() {
        let _ = edit_message_retry(token, chat, mid, &format!("✅ {}", outcome), false);
    }
    Ok(())
}

fn apply_block_action(act: &BlockAction) -> Result<String, String> {
    match act.action.as_str() {
        "unblock" => run_tasks_action(&["unblock", &act.id]),
        "drop" => run_tasks_action(&["move", &act.id, "done"]),
        "reassign" => run_tasks_action(&["move", &act.id, "ready"]),
        "text" => Ok(format!(
            "Reply with /resolve {} <your resolution text>",
            act.id
        )),
        _ => Err(format!("unknown block action {}", act.action)),
    }
}

fn resolve_block_text(arg: &str) -> Result<String, String> {
    let mut parts = arg.trim().splitn(2, char::is_whitespace);
    let id = parts
        .next()
        .ok_or("Usage: /resolve T-123 resolution text")?;
    let text = parts.next().unwrap_or("").trim();
    if !id.starts_with('T') || text.is_empty() {
        return Ok("Usage: /resolve T-123 resolution text".into());
    }
    run_tasks_action(&["unblock", id])
        .map(|r| format!("Custom resolution accepted for {id}: {text}\n{r}"))
}

fn run_tasks_action(args: &[&str]) -> Result<String, String> {
    let out = Command::new("target/release/tasks")
        .args(args)
        .output()
        .map_err(|e| format!("tasks: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn answer_callback(token: &str, callback_id: &str, text: &str) -> Result<(), String> {
    let body = format!(
        r#"{{"callback_query_id":"{}","text":"{}","show_alert":false}}"#,
        json::escape(callback_id),
        json::escape(&clip_tg(text))
    );
    api::call(token, "answerCallbackQuery", &body).map(|_| ())
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
                let Some(model) = callback_model_choice(data, date, unix_secs()) else {
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
            if allow_chat(&chat).is_err() {
                continue;
            }
            let text = msg.get("text").and_then(Value::as_str).unwrap_or("");
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
            out.push(format!(r#"{{"update_id":{uid},"chat":"{}","thread":"{}","message_id":"{}","user":"{}","from":"{}","date":{},"text":"{}"}}"#, json::escape(&chat), json::escape(&thread), json::escape(&message_id), json::escape(&user), json::escape(from), date, json::escape(text)));
        }
    }
    set_offset(max_id + 1)?;
    Ok(out.join("\n"))
}

fn listen(args: &[String]) -> Result<String, String> {
    let sleep: u64 = flag(args, "--sleep")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let gateway_agent = flag(args, "--gateway");
    eprintln!(
        "[tg] listen up (gateway={:?}, poll=short, sleep={sleep}s)",
        gateway_agent
    );
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
    let v = match json::parse(line) {
        Ok(v) => v,
        Err(_) => return,
    };
    let chat = v
        .get("chat")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let thread = v
        .get("thread")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let from = v
        .get("from")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string();
    let user = v
        .get("user")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let message_id = v
        .get("message_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let update_id = u64v(v.get("update_id")).unwrap_or(0);
    let date = u64v(v.get("date")).unwrap_or_else(unix_secs);
    let text = v
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    eprintln!(
        "[tg] inbound from @{from} ({chat}): {}",
        &text.chars().take(60).collect::<String>()
    );
    let _ = log_history(&HistoryEvent {
        direction: "in",
        chat: &chat,
        thread: &thread,
        user: &user,
        from: &from,
        update_id,
        message_id: &message_id,
        reply_to_update: 0,
        ts: date,
        text: &text,
    });
    let thread = v
        .get("thread")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if slash_name(&text) == Some("model") && text.split_whitespace().nth(1).is_none() {
        match send_model_menu(&chat, &thread, &user) {
            Ok(r) => eprintln!("[tg] model menu sent to {chat}: {r}"),
            Err(e) => eprintln!("[tg] model menu FAILED for {chat}: {e}"),
        }
        return;
    }
    if let Some(result) = slash_command(&text, &chat, &thread, &user) {
        let reply = result.unwrap_or_else(|e| format!("command error: {e}"));
        let cmd = slash_name(&text).unwrap_or("?");
        eprintln!(
            "[tg] slash /{cmd} chat={} thread={}: {} chars",
            chat,
            thread,
            reply.len()
        );
        let started = std::time::Instant::now();
        match stream_text_response(&chat, &thread, update_id, &format!("/{cmd}"), &reply) {
            Ok(r) => {
                let ms = started.elapsed().as_millis();
                let chunks = chunk_count(&reply);
                let _ = log_observation("command", &chat, &thread, cmd, "ok", ms, chunks, "", &r);
                eprintln!("[tg] slash streamed cmd=/{cmd} scope={chat}/{thread} duration_ms={ms} chunks={chunks} status=ok {r}");
            }
            Err(e) => {
                let ms = started.elapsed().as_millis();
                let _ = log_observation("command", &chat, &thread, cmd, "error", ms, 0, "", &e);
                eprintln!("[tg] slash stream FAILED cmd=/{cmd} scope={chat}/{thread} duration_ms={ms} status=error error={e}");
            }
        }
        return;
    }
    let Some(agent) = gateway_agent else {
        println!("{line}");
        return;
    };
    let agent = agent.to_string();
    std::thread::spawn(move || {
        if let Err(e) = stream_reply(&agent, &from, &user, &text, &chat, &thread, update_id) {
            eprintln!("[tg] stream FAILED: {e}");
            let _ = send(&[
                "send".into(),
                "--chat".into(),
                chat,
                "--text".into(),
                format!("⚠ agent unavailable: {e}"),
            ]);
        }
    });
}

/// Stream a precomputed slash-command response through the same Telegram UX
/// channel as LLM replies: send a placeholder, edit to an in-progress state,
/// then final-edit the completed text and record outbound history.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResponseKind {
    AgentAnswer,
    Board,
    TaskList,
    Status,
    WorkflowRuns,
    Memory,
    Confirmation,
    Blocker,
    Generic,
}

impl ResponseKind {
    fn from_label(label: &str) -> Self {
        match label.trim_start_matches('/') {
            "board" => ResponseKind::Board,
            "tasks" => ResponseKind::TaskList,
            "status" | "agents" => ResponseKind::Status,
            "runs" => ResponseKind::WorkflowRuns,
            "memory" => ResponseKind::Memory,
            "reset" | "remember" | "model" | "stop" => ResponseKind::Confirmation,
            _ => ResponseKind::Generic,
        }
    }
}

fn format_telegram_response(kind: ResponseKind, text: &str) -> String {
    let body = normalize_response_body(text);
    match kind {
        ResponseKind::AgentAnswer => titled_response("💬 Answer", &body, None),
        ResponseKind::Board => titled_response(
            "📋 Board",
            &body,
            Some("Next: use /tasks for the ready-only view."),
        ),
        ResponseKind::TaskList => titled_response(
            "🧾 Ready tasks",
            &body,
            Some("Next: use /board for WIP and blockers."),
        ),
        ResponseKind::Status => titled_response("🩺 Status", &body, None),
        ResponseKind::WorkflowRuns => titled_response("🔁 Workflow runs", &body, None),
        ResponseKind::Memory => titled_response("🧠 Memory", &body, None),
        ResponseKind::Confirmation => titled_response("✅ Done", &body, None),
        ResponseKind::Blocker => titled_response(
            "⛔ Blocked",
            &body,
            Some("Next: ask an admin or use an allowed chat."),
        ),
        ResponseKind::Generic => titled_response("SMARTAGENT", &body, None),
    }
}

fn titled_response(title: &str, body: &str, next: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str(title);
    out.push('\n');
    out.push_str(body);
    if let Some(n) = next {
        if !body.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(n);
    }
    clip_tg(&out)
}

fn normalize_response_body(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "• Result: no output".into();
    }
    if looks_structured(trimmed) {
        trimmed.to_string()
    } else {
        trimmed
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| format!("• {}", l.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn looks_structured(text: &str) -> bool {
    text.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with('•')
            || t.starts_with("- ")
            || t.starts_with("*")
            || t.starts_with("```")
            || t.starts_with('|')
            || t.ends_with(':')
    })
}

fn escape_markdown(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if matches!(ch, '_' | '*' | '[' | ']' | '(' | ')' | '`') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn stream_text_response(
    chat: &str,
    thread: &str,
    reply_to_update: u64,
    label: &str,
    text: &str,
) -> Result<String, String> {
    let token = bot_token()?;
    let mid = send_message_retry(&token, chat, &format!("💭 {label} …"), false)?;
    let preview = streaming_preview(label, text);
    let _ = edit_message_retry(&token, chat, mid, &preview, false);
    let final_text = if text.trim().is_empty() {
        "(no output)"
    } else {
        text
    };
    let kind = if final_text == safe_denial() || final_text.starts_with("command error:") {
        ResponseKind::Blocker
    } else {
        ResponseKind::from_label(label)
    };
    let formatted = format_telegram_response(kind, final_text);
    finish_streamed_message(&token, chat, mid, &formatted, false)?;
    let _ = log_history(&HistoryEvent {
        direction: "out",
        chat,
        thread,
        user: "agent",
        from: "agent",
        update_id: 0,
        message_id: &mid.to_string(),
        reply_to_update,
        ts: unix_secs(),
        text: &formatted,
    });
    Ok(format!("message_id={mid} chars={}", formatted.len()))
}

fn streaming_preview(label: &str, text: &str) -> String {
    let mut out = format!("💭 {label}\n");
    let first_line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("working…");
    out.push_str(&first_line.chars().take(240).collect::<String>());
    if text.lines().count() > 1 || text.chars().count() > 240 {
        out.push_str("\n…");
    }
    clip_tg(&out)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProgressEvent {
    Planning,
    ToolUse,
    Waiting,
    Verifying,
    FinalAnswer,
}

impl ProgressEvent {
    fn telegram_message(self) -> &'static str {
        match self {
            ProgressEvent::Planning => "🧭 Planning the next step…",
            ProgressEvent::ToolUse => "🔧 Using tools…",
            ProgressEvent::Waiting => "⏳ Waiting for a result…",
            ProgressEvent::Verifying => "✅ Verifying before replying…",
            ProgressEvent::FinalAnswer => "💬 Final answer ready.",
        }
    }
}

fn progress_scope_key(chat: &str, thread: &str) -> String {
    format!("tgprogress:{}", history_scope(chat, thread))
}

fn should_emit_progress(last_ms: Option<u128>, now_ms: u128) -> bool {
    const MIN_PROGRESS_EDIT_MS: u128 = 1_500;
    last_ms
        .map(|last| now_ms.saturating_sub(last) >= MIN_PROGRESS_EDIT_MS)
        .unwrap_or(true)
}

fn progress_event_for_stream_line(line: &str) -> ProgressEvent {
    let l = line.to_ascii_lowercase();
    if l.contains("tool") || l.contains("call") {
        ProgressEvent::ToolUse
    } else if l.contains("wait") || l.contains("queued") {
        ProgressEvent::Waiting
    } else if l.contains("verify") || l.contains("test") {
        ProgressEvent::Verifying
    } else if l.contains("plan") || l.contains("think") {
        ProgressEvent::Planning
    } else {
        ProgressEvent::FinalAnswer
    }
}

fn progress_frame(event: ProgressEvent, body: &str) -> String {
    if event == ProgressEvent::FinalAnswer {
        clip_tg(body)
    } else {
        clip_tg(&format!("{}\n\n{}", event.telegram_message(), body))
    }
}

/// Streaming reply (hermes-style): send a placeholder, then live-edit it as
/// the agent generates. `gateway ask --stream` prints growing snapshots; we
/// throttle editMessageText to ~1 edit / 1.5s to respect Telegram rate limits.
fn stream_reply(
    agent: &str,
    from: &str,
    user: &str,
    text: &str,
    chat: &str,
    thread: &str,
    update_id: u64,
) -> Result<(), String> {
    let token = bot_token()?;
    let prompt = build_gateway_prompt(from, user, text, chat, thread);
    let mid = send_message_retry(
        &token,
        chat,
        ProgressEvent::Planning.telegram_message(),
        false,
    )?;
    let stream_started = std::time::Instant::now();
    let mut tool_markers = 0usize;
    let progress_scope = progress_scope_key(chat, thread);
    eprintln!("[tg] progress scope {progress_scope}: planning");
    let cancel_seen = cancel_token(chat, thread).unwrap_or(0);
    let mut child = Command::new("target/release/gateway")
        .args([
            "ask",
            "--agent",
            agent,
            "--timeout",
            "90",
            "--stream",
            &prompt,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn gateway ask: {e}"))?;
    let out = child.stdout.take().ok_or("no gateway stdout")?;
    let mut last_edit = std::time::Instant::now();
    let mut latest = String::new();
    let mut shown = String::new();
    for line in BufReader::new(out).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if stop_requested(chat, thread, cancel_seen) {
            let _ = child.kill();
            let stopped = "⏹ Stopped this Telegram chat/thread response.";
            let _ = edit_message_retry(&token, chat, mid, stopped, false);
            let _ = log_history(&HistoryEvent {
                direction: "out",
                chat,
                thread,
                user: "agent",
                from: "agent",
                update_id: 0,
                message_id: &mid.to_string(),
                reply_to_update: update_id,
                ts: unix_secs(),
                text: stopped,
            });
            eprintln!("[tg] stopped scoped reply for {chat}/{thread}");
            return Ok(());
        }
        if let Some(info) = line.strip_prefix(GATEWAY_STREAM_INFO_PREFIX) {
            if let Some(status) = telegram_tool_status(info) {
                tool_markers += 1;
                let _ = log_observation(
                    "tool",
                    chat,
                    thread,
                    "stream",
                    "marker",
                    stream_started.elapsed().as_millis(),
                    0,
                    &status,
                    "",
                );
                eprintln!("[tg] tool marker scope={chat}/{thread} marker={status}");
                let preview = stream_preview(&latest, &status);
                let _ = edit_message_retry(&token, chat, mid, &clip_tg(&preview), false);
                shown = preview;
                last_edit = std::time::Instant::now();
            }
            continue;
        }
        if line.starts_with(GATEWAY_STREAM_THINKING_PREFIX) {
            let frame = thinking_fallback().to_string();
            if frame != shown && should_emit_progress(Some(0), last_edit.elapsed().as_millis()) {
                let _ = api::edit_message(&token, chat, mid, &frame, false);
                shown = frame;
                last_edit = std::time::Instant::now();
            }
            continue;
        }
        latest = line.replace("\\n", "\n"); // un-escape the streamed snapshot
        let event = progress_event_for_stream_line(&latest);
        let frame = progress_frame(event, &latest);
        // Throttle: edit at most every 1.5s, only when the text grew.
        if frame != shown && should_emit_progress(Some(0), last_edit.elapsed().as_millis()) {
            let _ = edit_message_retry(&token, chat, mid, &frame, false);
            shown = frame;
            last_edit = std::time::Instant::now();
        }
    }
    let _ = child.wait();
    // Final edit with the complete reply (Markdown for the finished message).
    let final_text = if latest.trim().is_empty() {
        "(the agent had no reply)".to_string()
    } else {
        latest
    };
    let formatted = format_telegram_response(ResponseKind::AgentAnswer, &final_text);
    finish_streamed_message(&token, chat, mid, &formatted, false)?;
    let duration_ms = stream_started.elapsed().as_millis();
    let chunks_sent = final_message_chunks(&formatted).len();
    let _ = log_observation(
        "stream",
        chat,
        thread,
        "agent",
        "ok",
        duration_ms,
        chunks_sent,
        &format!("tool_markers={tool_markers}"),
        "",
    );
    let _ = log_history(&HistoryEvent {
        direction: "out",
        chat,
        thread,
        user: "agent",
        from: "agent",
        update_id: 0,
        message_id: &mid.to_string(),
        reply_to_update: update_id,
        ts: unix_secs(),
        text: &formatted,
    });
    eprintln!("[tg] streamed reply scope={chat}/{thread} duration_ms={duration_ms} chunks={chunks_sent} tool_markers={tool_markers} status=ok chars={}", formatted.len());
    Ok(())
}

fn telegram_tool_status(info: &str) -> Option<String> {
    let s = info.trim();
    if !s.starts_with('🛠') {
        return None;
    }
    let safe = s
        .chars()
        .filter(|c| {
            c.is_ascii_alphanumeric()
                || c.is_whitespace()
                || matches!(*c, '🛠' | '✓' | '✗' | '…' | '-' | '_')
        })
        .take(80)
        .collect::<String>();
    if safe.trim().is_empty() {
        None
    } else {
        Some(safe)
    }
}

fn stream_preview(reply: &str, status: &str) -> String {
    let base = if reply.trim().is_empty() {
        "💭 …"
    } else {
        reply.trim_end()
    };
    format!("{base}\n\n{status}")
}

fn thinking_fallback() -> &'static str {
    "💭 Thinking tokens are not available from this model; streaming visible output instead."
}

#[cfg(test)]
fn simulate_stream_frames(lines: &[&str]) -> Vec<String> {
    let mut latest = String::new();
    let mut frames = Vec::new();
    for line in lines {
        if let Some(info) = line.strip_prefix(GATEWAY_STREAM_INFO_PREFIX) {
            if let Some(status) = telegram_tool_status(info) {
                frames.push(stream_preview(&latest, &status));
            }
        } else if line.starts_with(GATEWAY_STREAM_THINKING_PREFIX) {
            frames.push(thinking_fallback().to_string());
        } else {
            latest = line.replace("\\n", "\n");
            frames.push(progress_frame(
                progress_event_for_stream_line(&latest),
                &latest,
            ));
        }
    }
    frames
}

/// Telegram single-message cap is 4096 chars — keep a margin.
fn build_gateway_prompt(from: &str, user: &str, text: &str, chat: &str, thread: &str) -> String {
    let mut prompt = String::new();
    let scope = if thread.is_empty() {
        chat.to_string()
    } else {
        format!("{chat}/{thread}")
    };
    prompt.push_str(&format!(
        "Telegram message from @{from} in chat/thread {scope}: {text}\n"
    ));
    if let Some(model) = selected_model(chat, thread, user) {
        prompt.push_str(&format!(
            "Selected Telegram model preference for this scope/user: {model}\n"
        ));
    }
    if let Some(ctx) = scoped_context(chat, thread, text) {
        prompt.push_str(
            "\nScoped Telegram context (use only for this chat/thread; do not mix chats):\n",
        );
        prompt.push_str(&ctx);
        prompt.push('\n');
    }
    prompt.push_str("\nReply concisely — your reply is streamed straight to Telegram.");
    prompt
}

fn scoped_context(chat: &str, thread: &str, text: &str) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(h) = scoped_history(chat, thread) {
        parts.push(format!("Recent chat/thread history:\n{h}"));
    }
    if let Some(m) = scoped_memories(chat, thread, text) {
        parts.push(format!("Relevant chat/thread memories:\n{m}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn scoped_history(chat: &str, thread: &str) -> Option<String> {
    let db = open_db().ok()?;
    scoped_history_from_db(&db, chat, thread)
}

fn scoped_history_from_db(db: &Db, chat: &str, thread: &str) -> Option<String> {
    let mut rows = Vec::new();
    for (id, entry) in &db.index {
        let Ok(v) = json::parse(&entry.meta) else {
            continue;
        };
        let row_chat = v.get("chat").and_then(Value::as_str).unwrap_or("");
        let row_thread = v.get("thread").and_then(Value::as_str).unwrap_or("");
        if row_chat != chat || row_thread != thread {
            continue;
        }
        let text = v
            .get("text")
            .and_then(Value::as_str)
            .or_else(|| v.get("reply").and_then(Value::as_str))
            .unwrap_or("");
        if text.trim().is_empty() {
            continue;
        }
        let dir = v
            .get("direction")
            .and_then(Value::as_str)
            .unwrap_or("message");
        let who = v.get("from").and_then(Value::as_str).unwrap_or(dir);
        rows.push((id.clone(), format!("- {who}: {}", text.replace('\n', " "))));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let lines: Vec<String> = rows
        .into_iter()
        .rev()
        .take(8)
        .map(|(_, line)| line)
        .collect();
    if lines.is_empty() {
        None
    } else {
        Some(lines.into_iter().rev().collect::<Vec<_>>().join("\n"))
    }
}

fn scoped_memories(chat: &str, thread: &str, text: &str) -> Option<String> {
    let scope = if thread.is_empty() {
        chat.to_string()
    } else {
        format!("{chat}/{thread}")
    };
    let query = format!("telegram chat/thread {scope}: {text}");
    let out = Command::new("target/release/memory")
        .args([
            "recall",
            "--dir",
            "data/memory",
            "--text",
            &query,
            "--k",
            "3",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn send_message_retry(token: &str, chat: &str, text: &str, markdown: bool) -> Result<i64, String> {
    match retry_telegram(|| api::send_message(token, chat, text, markdown)) {
        Ok(v) => Ok(v),
        Err(e) => {
            let _ = log_observation(
                "send",
                chat,
                "",
                "sendMessage",
                "error",
                0,
                chunk_count(text),
                "retry_failed",
                &e,
            );
            Err(e)
        }
    }
}

fn edit_message_retry(
    token: &str,
    chat: &str,
    message_id: i64,
    text: &str,
    markdown: bool,
) -> Result<(), String> {
    match retry_telegram(|| api::edit_message(token, chat, message_id, text, markdown)) {
        Ok(v) => Ok(v),
        Err(e) => {
            let _ = log_observation(
                "edit",
                chat,
                "",
                "editMessageText",
                "error",
                0,
                chunk_count(text),
                "retry_failed",
                &e,
            );
            Err(e)
        }
    }
}

fn retry_telegram<T, F>(mut f: F) -> Result<T, String>
where
    F: FnMut() -> Result<T, String>,
{
    let mut last = String::new();
    for delay_ms in [0_u64, 700, 1_500] {
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                last = e;
                if !is_retryable_telegram_error(&last) {
                    break;
                }
            }
        }
    }
    Err(last)
}

fn is_retryable_telegram_error(e: &str) -> bool {
    let l = e.to_ascii_lowercase();
    l.contains("too many requests")
        || l.contains("retry after")
        || l.contains("timed out")
        || l.contains("timeout")
        || l.contains("temporarily")
        || l.contains("connection")
        || l.contains("502")
        || l.contains("503")
        || l.contains("504")
}

fn finish_streamed_message(
    token: &str,
    chat: &str,
    message_id: i64,
    final_text: &str,
    markdown: bool,
) -> Result<(), String> {
    let chunks = final_message_chunks(final_text);
    let Some(first) = chunks.first() else {
        return Ok(());
    };
    edit_message_retry(token, chat, message_id, first, markdown)
        .or_else(|_| edit_message_retry(token, chat, message_id, first, false))?;
    for chunk in chunks.iter().skip(1) {
        send_message_retry(token, chat, chunk, false)?;
    }
    Ok(())
}

fn final_message_chunks(final_text: &str) -> Vec<String> {
    chunks(
        if final_text.is_empty() {
            "(no output)"
        } else {
            final_text
        },
        4000,
    )
}

fn clip_tg(s: &str) -> String {
    if s.chars().count() > 4000 {
        format!("{}…", s.chars().take(4000).collect::<String>())
    } else {
        s.to_string()
    }
}

struct HistoryEvent<'a> {
    direction: &'a str,
    chat: &'a str,
    thread: &'a str,
    user: &'a str,
    from: &'a str,
    update_id: u64,
    message_id: &'a str,
    reply_to_update: u64,
    ts: u64,
    text: &'a str,
}

fn log_history(ev: &HistoryEvent<'_>) -> Result<(), String> {
    let mut db = open_db()?;
    log_history_to_db(&mut db, ev)
}

fn log_history_to_db(db: &mut Db, ev: &HistoryEvent<'_>) -> Result<(), String> {
    let scope = history_scope(ev.chat, ev.thread);
    let id = format!(
        "hist:{scope}:{:020}:{}:{}",
        ev.ts, ev.update_id, ev.direction
    );
    let meta = format!(
        r#"{{"kind":"telegram_history","direction":"{}","chat":"{}","thread":"{}","user":"{}","from":"{}","update_id":{},"message_id":"{}","reply_to_update":{},"ts":{},"text":"{}"}}"#,
        json::escape(ev.direction),
        json::escape(ev.chat),
        json::escape(ev.thread),
        json::escape(ev.user),
        json::escape(ev.from),
        ev.update_id,
        json::escape(ev.message_id),
        ev.reply_to_update,
        ev.ts,
        json::escape(ev.text)
    );
    db.put(&id, &meta, VEC0.to_vec())?;
    prune_history(db, &scope, HISTORY_KEEP_PER_SCOPE)
}

fn history_scope(chat: &str, thread: &str) -> String {
    format!(
        "{}:{}",
        safe_id_part(chat),
        safe_id_part(if thread.is_empty() { "main" } else { thread })
    )
}

fn safe_id_part(s: &str) -> String {
    let out = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    if out.is_empty() {
        "unknown".into()
    } else {
        out
    }
}

fn prune_history(db: &mut Db, scope: &str, keep: usize) -> Result<(), String> {
    let prefix = format!("hist:{scope}:");
    let mut ids = db
        .index
        .keys()
        .filter(|id| id.starts_with(&prefix))
        .cloned()
        .collect::<Vec<_>>();
    ids.sort();
    let extra = ids.len().saturating_sub(keep);
    for id in ids.into_iter().take(extra) {
        db.delete(&id)?;
    }
    Ok(())
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Telegram slash commands run the platform binaries directly (no LLM turn) and
/// relay their output — same set as the TUI slash commands.
struct BotCommand {
    name: &'static str,
    description: &'static str,
}

const TELEGRAM_COMMANDS: &[BotCommand] = &[
    BotCommand {
        name: "help",
        description: "Show SMARTAGENT Telegram commands",
    },
    BotCommand {
        name: "commands",
        description: "Show SMARTAGENT Telegram commands",
    },
    BotCommand {
        name: "board",
        description: "Show the kanban board",
    },
    BotCommand {
        name: "tasks",
        description: "List ready tasks",
    },
    BotCommand {
        name: "status",
        description: "Show supervised service status",
    },
    BotCommand {
        name: "skills",
        description: "List skills or match a query",
    },
    BotCommand {
        name: "agents",
        description: "Show gateway fleet state",
    },
    BotCommand {
        name: "runs",
        description: "Show active workflows",
    },
    BotCommand {
        name: "memory",
        description: "Recall SMARTAGENT memory",
    },
    BotCommand {
        name: "model",
        description: "Choose this chat's reply model",
    },
    BotCommand {
        name: "reset",
        description: "Clear this chat/thread rolling context",
    },
    BotCommand {
        name: "remember",
        description: "Remember a fact for this chat/channel",
    },
    BotCommand {
        name: "stop",
        description: "Stop the active reply in this chat/thread",
    },
    BotCommand {
        name: "resolve",
        description: "Add custom resolution text for a blocked task",
    },
];

fn command_help() -> String {
    let mut out = String::from("SMARTAGENT Telegram commands:");
    for c in TELEGRAM_COMMANDS {
        out.push_str(&format!("\n• /{} — {}", c.name, c.description));
    }
    out
}

fn command_menu_body() -> String {
    let commands = TELEGRAM_COMMANDS
        .iter()
        .map(|c| {
            format!(
                r#"{{"command":"{}","description":"{}"}}"#,
                json::escape(c.name),
                json::escape(c.description)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"commands":[{commands}]}}"#)
}

fn slash_command(
    text: &str,
    chat: &str,
    thread: &str,
    user: &str,
) -> Option<Result<String, String>> {
    let mut parts = text.trim().splitn(2, char::is_whitespace);
    let cmd = slash_name(text)?;
    let _ = parts.next();
    let arg = parts.next().unwrap_or("").trim();
    if cmd == "remember" && arg.is_empty() {
        return Some(Ok("Usage: /remember fact to store for this chat".into()));
    }
    if let Err(denial) = authorize_slash_command(cmd, chat, user) {
        return Some(Ok(denial));
    }
    let run = |bin: &str, args: &[&str]| -> Result<String, String> {
        let out = Command::new(format!("target/release/{bin}"))
            .args(args)
            .output()
            .map_err(|e| format!("{bin}: {e}"))?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(if s.is_empty() {
            "(no output)".into()
        } else {
            s
        })
    };
    Some(match cmd {
        "start" | "help" | "commands" => Ok(command_help()),
        "board" => run("tasks", &["board", "--db", "data/tasks.semdb"]),
        "tasks" => run(
            "tasks",
            &["list", "--col", "ready", "--db", "data/tasks.semdb"],
        ),
        "status" => run("supervise", &["status"]),
        "agents" => run("gateway", &["agents"]),
        "runs" => run(
            "workflow",
            &["runs", "--live", "--db", "data/workflow.semdb"],
        ),
        "skills" => run(
            "skills",
            if arg.is_empty() {
                vec!["list"]
            } else {
                vec!["match", arg]
            }
            .as_slice(),
        ),
        "memory" if !arg.is_empty() => {
            run("memory", &["recall", "--dir", "data/memory", "--text", arg])
        }
        "reset" => reset_context(chat, thread),
        "model" if !arg.is_empty() => set_model_preference(chat, thread, user, arg),
        "model" => Ok(model_menu_text(chat, thread, user)),
        "remember" => remember_context_fact(chat, thread, arg),
        "resolve" => resolve_block_text(arg),
        "stop" => stop_context(chat, thread),
        _ => return None, // unknown slash → treat as normal chat
    })
}

fn slash_name(text: &str) -> Option<&str> {
    let raw = text.trim().split_whitespace().next()?.strip_prefix('/')?;
    Some(raw.split('@').next().unwrap_or(raw))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommandClass {
    UserSafe,
    ChatScoped,
    AdminOnly,
}

fn command_class(cmd: &str) -> Option<CommandClass> {
    Some(match cmd {
        "start" | "help" | "commands" => CommandClass::UserSafe,
        "memory" | "reset" | "remember" | "resolve" => CommandClass::ChatScoped,
        "board" | "tasks" | "status" | "agents" | "runs" | "skills" | "model" | "stop" => {
            CommandClass::AdminOnly
        }
        _ => return None,
    })
}

fn authorize_slash_command(cmd: &str, chat: &str, user: &str) -> Result<(), String> {
    let cfg = Config::load();
    let allowed_chats = cfg
        .resolve(
            "telegram_allowed_chats",
            "SMARTAGENT_TELEGRAM_ALLOWED_CHATS",
            None,
        )
        .unwrap_or_default();
    let admin_users = cfg
        .resolve(
            "telegram_admin_users",
            "SMARTAGENT_TELEGRAM_ADMIN_USERS",
            None,
        )
        .unwrap_or_default();
    let admin_chats = cfg
        .resolve(
            "telegram_admin_chats",
            "SMARTAGENT_TELEGRAM_ADMIN_CHATS",
            None,
        )
        .unwrap_or_default();
    authorize_slash_command_with_lists(cmd, chat, user, &allowed_chats, &admin_users, &admin_chats)
}

fn authorize_slash_command_with_lists(
    cmd: &str,
    chat: &str,
    user: &str,
    allowed_chats: &str,
    admin_users: &str,
    admin_chats: &str,
) -> Result<(), String> {
    match command_class(cmd) {
        Some(CommandClass::UserSafe) => Ok(()),
        Some(CommandClass::ChatScoped) => {
            if listed(allowed_chats, chat) {
                Ok(())
            } else {
                Err(safe_denial())
            }
        }
        Some(CommandClass::AdminOnly) => {
            if !listed(allowed_chats, chat) {
                return Err(safe_denial());
            }
            let explicit_admins = !admin_users.trim().is_empty() || !admin_chats.trim().is_empty();
            let is_admin = listed(admin_users, user) || listed(admin_chats, chat);
            if is_admin || (!explicit_admins && listed(allowed_chats, chat)) {
                Ok(())
            } else {
                Err(safe_denial())
            }
        }
        None => Ok(()),
    }
}

fn listed(list: &str, item: &str) -> bool {
    !item.is_empty() && list.split(',').map(str::trim).any(|v| v == item)
}

fn safe_denial() -> String {
    "Sorry, this Telegram command is not available for this chat/user.".into()
}

fn context_scope(chat: &str, thread: &str) -> String {
    if thread.is_empty() {
        format!("telegram chat {chat}")
    } else {
        format!("telegram chat {chat} thread {thread}")
    }
}

fn reset_context(chat: &str, thread: &str) -> Result<String, String> {
    let mut db = open_db()?;
    let n = reset_context_in_db(&mut db, chat, thread)?;
    Ok(format!(
        "Reset {} rolling context item(s) for this chat/thread.",
        n
    ))
}

fn reset_context_in_db(db: &mut Db, chat: &str, thread: &str) -> Result<usize, String> {
    let prefix = format!("hist:{}:", history_scope(chat, thread));
    let ids = db
        .index
        .keys()
        .filter(|id| id.starts_with(&prefix))
        .cloned()
        .collect::<Vec<_>>();
    for id in &ids {
        db.delete(id)?;
    }
    Ok(ids.len())
}

fn stop_context(chat: &str, thread: &str) -> Result<String, String> {
    let mut db = open_db()?;
    let token = cancel_token_from_db(&db, chat, thread)
        .unwrap_or(0)
        .max(unix_secs())
        + 1;
    set_cancel_token_in_db(&mut db, chat, thread, token)?;
    Ok("Stop requested for this chat/thread. I will halt the active streamed reply here; other chats are unaffected.".into())
}

fn cancel_key(chat: &str, thread: &str) -> String {
    format!("cancel:{}", history_scope(chat, thread))
}

fn set_cancel_token_in_db(db: &mut Db, chat: &str, thread: &str, token: u64) -> Result<(), String> {
    let scope = history_scope(chat, thread);
    db.put(
        &cancel_key(chat, thread),
        &format!(
            r#"{{"kind":"telegram_cancel","scope":"{}","token":{}}}"#,
            json::escape(&scope),
            token
        ),
        VEC0.to_vec(),
    )
}

fn cancel_token_from_db(db: &Db, chat: &str, thread: &str) -> Result<u64, String> {
    Ok(db
        .get(&cancel_key(chat, thread))
        .and_then(|e| json::parse(&e.meta).ok())
        .and_then(|v| u64v(v.get("token")))
        .unwrap_or(0))
}

fn cancel_token(chat: &str, thread: &str) -> Result<u64, String> {
    cancel_token_from_db(&open_db()?, chat, thread)
}

fn stop_requested_in_db(db: &Db, chat: &str, thread: &str, seen: u64) -> bool {
    cancel_token_from_db(db, chat, thread)
        .map(|t| t > seen)
        .unwrap_or(false)
}

fn stop_requested(chat: &str, thread: &str, seen: u64) -> bool {
    open_db()
        .map(|db| stop_requested_in_db(&db, chat, thread, seen))
        .unwrap_or(false)
}

fn remember_context_fact(chat: &str, thread: &str, fact: &str) -> Result<String, String> {
    let fact = fact.trim();
    if fact.is_empty() {
        return Ok("Usage: /remember fact to store for this chat".into());
    }
    let scoped_fact = format!("{}: {}", context_scope(chat, thread), fact);
    let out = Command::new("target/release/memory")
        .args([
            "remember",
            "--dir",
            "data/memory",
            "--tier",
            "semantic",
            "--text",
            &scoped_fact,
        ])
        .output()
        .map_err(|e| format!("memory remember: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok("Remembered for this chat.".into())
}

fn model_pref_key(chat: &str, thread: &str, user: &str) -> String {
    format!(
        "model:{}:{}",
        history_scope(chat, thread),
        safe_id_part(user)
    )
}

fn selected_model(chat: &str, thread: &str, user: &str) -> Option<String> {
    let db = open_db().ok()?;
    selected_model_from_db(&db, chat, thread, user)
}

fn selected_model_from_db(db: &Db, chat: &str, thread: &str, user: &str) -> Option<String> {
    let key = model_pref_key(chat, thread, user);
    db.get(&key)
        .and_then(|e| json::parse(&e.meta).ok())
        .and_then(|v| v.get("model").and_then(Value::as_str).map(str::to_string))
}

fn normalize_model_choice(choice: &str) -> Option<&'static str> {
    let c = choice.trim();
    if c.is_empty() {
        return None;
    }
    if let Ok(n) = c.parse::<usize>() {
        if (1..=TELEGRAM_MODELS.len()).contains(&n) {
            return Some(TELEGRAM_MODELS[n - 1]);
        }
    }
    TELEGRAM_MODELS.iter().copied().find(|m| *m == c)
}

fn set_model_preference(
    chat: &str,
    thread: &str,
    user: &str,
    choice: &str,
) -> Result<String, String> {
    let Some(model) = normalize_model_choice(choice) else {
        return Ok(model_menu_text(chat, thread, user));
    };
    let mut db = open_db()?;
    set_model_preference_in_db(&mut db, chat, thread, user, model)?;
    Ok(format!(
        "Model preference set to {model} for this chat/thread/user."
    ))
}

fn set_model_preference_in_db(
    db: &mut Db,
    chat: &str,
    thread: &str,
    user: &str,
    model: &str,
) -> Result<(), String> {
    let key = model_pref_key(chat, thread, user);
    let meta = format!(
        r#"{{"kind":"telegram_model_pref","chat":"{}","thread":"{}","user":"{}","model":"{}","ts":{}}}"#,
        json::escape(chat),
        json::escape(thread),
        json::escape(user),
        json::escape(model),
        unix_secs()
    );
    db.put(&key, &meta, VEC0.to_vec())
}

fn model_menu_text(chat: &str, thread: &str, user: &str) -> String {
    let current =
        selected_model(chat, thread, user).unwrap_or_else(|| "default gateway model".into());
    let mut out =
        format!("Current model: {current}\nChoose a model with /model <number> or tap a button:");
    for (i, m) in TELEGRAM_MODELS.iter().enumerate() {
        out.push_str(&format!("\n{}. {m}", i + 1));
    }
    out
}

fn callback_model_choice(data: &str, message_date: u64, now: u64) -> Option<&str> {
    if now.saturating_sub(message_date) > CALLBACK_MAX_AGE_SECS {
        return None;
    }
    let model = data.strip_prefix("model:")?;
    normalize_model_choice(model)
}

fn model_menu_markup() -> String {
    let rows = TELEGRAM_MODELS
        .iter()
        .map(|m| {
            format!(
                r#"[{{"text":"{}","callback_data":"model:{}"}}]"#,
                json::escape(m),
                json::escape(m)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"inline_keyboard":[{rows}]}}"#)
}

fn send_model_menu(chat: &str, thread: &str, user: &str) -> Result<String, String> {
    let token = bot_token()?;
    let text = model_menu_text(chat, thread, user);
    let mid =
        api::send_message_with_markup(&token, chat, &text, false, Some(&model_menu_markup()))?;
    Ok(format!("message_id={mid} models={}", TELEGRAM_MODELS.len()))
}

fn bot_token() -> Result<String, String> {
    // Caller auth: the ./pi launcher injects SMARTAGENT_CALLER_TOKEN, but the
    // supervised listener is spawned by supervise WITHOUT it — fall back to
    // the token file (same trust domain; tmpfs-masked inside the sandbox, so
    // sandboxed commands still can't present it).
    let mut cmd = Command::new("target/release/secrets");
    cmd.args([
        "get",
        "--store",
        "data/secrets",
        "--name",
        "telegram_bot_token",
        "--as",
        "pi",
    ]);
    if std::env::var("SMARTAGENT_CALLER_TOKEN").is_err() {
        if let Ok(tok) = std::fs::read_to_string("data/secrets/tokens/pi.token") {
            cmd.env("SMARTAGENT_CALLER_TOKEN", tok.trim());
        }
    }
    let out = cmd.output().map_err(|e| format!("run secrets get: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn allow_chat(chat: &str) -> Result<(), String> {
    let cfg = Config::load();
    let allowed = cfg
        .resolve(
            "telegram_allowed_chats",
            "SMARTAGENT_TELEGRAM_ALLOWED_CHATS",
            None,
        )
        .unwrap_or_default();
    if allowed.split(',').map(str::trim).any(|c| c == chat) {
        Ok(())
    } else {
        Err(format!("chat {chat} not allowed"))
    }
}

fn db_path() -> PathBuf {
    Config::load().data_dir().join("telegram.semdb")
}
fn log_observation(
    kind: &str,
    chat: &str,
    thread: &str,
    command: &str,
    status: &str,
    duration_ms: u128,
    chunks: usize,
    detail: &str,
    error: &str,
) -> Result<(), String> {
    let mut db = open_db()?;
    log_observation_to_db(
        &mut db,
        kind,
        chat,
        thread,
        command,
        status,
        duration_ms,
        chunks,
        detail,
        error,
    )
}

fn log_observation_to_db(
    db: &mut Db,
    kind: &str,
    chat: &str,
    thread: &str,
    command: &str,
    status: &str,
    duration_ms: u128,
    chunks: usize,
    detail: &str,
    error: &str,
) -> Result<(), String> {
    let ts = unix_secs();
    let id = format!("obs:{:020}:{}:{}", ts, safe_id_part(kind), db.index.len());
    let safe_error = redact_secretish(error);
    let meta = format!(
        r#"{{"kind":"telegram_observation","event":"{}","chat":"{}","thread":"{}","command":"{}","status":"{}","duration_ms":{},"chunks":{},"detail":"{}","error":"{}","ts":{}}}"#,
        json::escape(kind),
        json::escape(chat),
        json::escape(thread),
        json::escape(command),
        json::escape(status),
        duration_ms,
        chunks,
        json::escape(detail),
        json::escape(&safe_error),
        ts
    );
    db.put(&id, &meta, VEC0.to_vec())
}

fn telegram_status_report(limit: usize) -> String {
    let Ok(db) = open_db() else {
        return "Telegram status: no database".into();
    };
    let mut rows = db
        .index
        .iter()
        .filter_map(|(id, e)| json::parse(&e.meta).ok().map(|v| (id.clone(), v)))
        .filter(|(_, v)| v.get("kind").and_then(Value::as_str) == Some("telegram_observation"))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let failures = rows
        .iter()
        .filter(|(_, v)| v.get("status").and_then(Value::as_str).unwrap_or("") == "error")
        .count();
    let mut out = format!(
        "Telegram status: {} recent observation(s), {} failure(s)",
        rows.len(),
        failures
    );
    for (_, v) in rows.into_iter().rev().take(limit) {
        let event = v.get("event").and_then(Value::as_str).unwrap_or("?");
        let cmd = v.get("command").and_then(Value::as_str).unwrap_or("?");
        let status = v.get("status").and_then(Value::as_str).unwrap_or("?");
        let chunks = v.get("chunks").and_then(Value::as_f64).unwrap_or(0.0) as usize;
        let dur = v.get("duration_ms").and_then(Value::as_f64).unwrap_or(0.0) as u128;
        let err = v.get("error").and_then(Value::as_str).unwrap_or("");
        out.push_str(&format!(
            "\n- {event}/{cmd}: status={status} duration_ms={dur} chunks={chunks}"
        ));
        if !err.is_empty() {
            out.push_str(&format!(" error={}", redact_secretish(err)));
        }
    }
    out
}

fn redact_secretish(s: &str) -> String {
    let mut out = s.to_string();
    for marker in ["bot", "token", "Authorization", "Bearer"] {
        if out
            .to_ascii_lowercase()
            .contains(&marker.to_ascii_lowercase())
        {
            out = out.replace(marker, "[redacted]");
        }
    }
    out.chars().take(240).collect()
}

fn chunk_count(s: &str) -> usize {
    final_message_chunks(s).len()
}

fn open_db() -> Result<Db, String> {
    let p = db_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if p.exists() {
        Db::open(&p)
    } else {
        Db::create(&p)
    }
}
fn offset() -> Result<u64, String> {
    Ok(open_db()?
        .get(ROW_OFFSET)
        .and_then(|e| json::parse(&e.meta).ok())
        .and_then(|v| u64v(v.get("offset")))
        .unwrap_or(0))
}
fn set_offset(n: u64) -> Result<(), String> {
    let mut db = open_db()?;
    db.put(ROW_OFFSET, &format!(r#"{{"offset":{n}}}"#), VEC0.to_vec())
}

fn u64v(v: Option<&Value>) -> Option<u64> {
    v.and_then(Value::as_f64).map(|x| x.max(0.0) as u64)
}
fn val_s(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        _ => format!("{}", v.as_f64().unwrap_or(0.0) as i64),
    }
}

fn chunks(s: &str, max: usize) -> Vec<String> {
    if s.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut rest = s;
    while rest.chars().count() > max {
        let mut cut = rest
            .char_indices()
            .nth(max)
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        if let Some(nl) = rest[..cut].rfind('\n') {
            if nl > 0 {
                cut = nl + 1;
            }
        }
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
    use super::{
        build_gateway_prompt, chunks, command_help, command_menu_body,
        progress_event_for_stream_line, progress_frame, progress_scope_key, should_emit_progress,
        simulate_stream_frames, slash_command, slash_name, streaming_preview, ProgressEvent,
        TELEGRAM_COMMANDS,
    };

    #[test]
    fn command_menu_lists_supported_slashes() {
        let body = command_menu_body();
        let help = command_help();
        for c in TELEGRAM_COMMANDS {
            assert!(
                body.contains(&format!("\"command\":\"{}\"", c.name)),
                "{body}"
            );
            assert!(
                help.contains(&format!("• /{} — {}", c.name, c.description)),
                "{help}"
            );
        }
        assert!(body.contains("\"commands\""));
    }

    #[test]
    fn help_catalog_matches_registered_menu_exactly() {
        let help = command_help();
        let lines = help.lines().skip(1).collect::<Vec<_>>();
        assert_eq!(lines.len(), TELEGRAM_COMMANDS.len(), "{help}");
        for (line, c) in lines.iter().zip(TELEGRAM_COMMANDS) {
            assert_eq!(*line, format!("• /{} — {}", c.name, c.description));
        }
        assert!(help.chars().count() < 1200, "{help}");
    }

    #[test]
    fn formatter_wraps_slash_and_agent_outputs() {
        let board = super::format_telegram_response(
            super::ResponseKind::Board,
            "READY (1)\nT-1 p1 Demo [0/1✓]",
        );
        assert!(board.starts_with("📋 Board"), "{board}");
        assert!(board.contains("READY (1)"), "{board}");
        assert!(board.contains("Next: use /tasks"), "{board}");

        let answer = super::format_telegram_response(
            super::ResponseKind::AgentAnswer,
            "First line\nSecond line",
        );
        assert!(answer.starts_with("💬 Answer"), "{answer}");
        assert!(answer.contains("• First line"), "{answer}");
        assert!(answer.contains("• Second line"), "{answer}");
    }

    #[test]
    fn formatter_preserves_bullets_code_and_tables() {
        let body = "• bullet\n```\ncode_with_*_chars\n```\n| A | B |";
        let out = super::format_telegram_response(super::ResponseKind::Memory, body);
        assert!(out.starts_with("🧠 Memory"), "{out}");
        assert!(out.contains("• bullet"), "{out}");
        assert!(out.contains("```\ncode_with_*_chars\n```"), "{out}");
        assert!(out.contains("| A | B |"), "{out}");
    }

    #[test]
    fn formatter_snapshots_common_response_categories() {
        let cases = [
            (
                super::ResponseKind::AgentAnswer,
                "Done\nEvidence captured",
                "💬 Answer\n• Done\n• Evidence captured",
            ),
            (
                super::ResponseKind::TaskList,
                "T-1 p1 — Fix thing\nT-2 p2 — Test thing",
                "🧾 Ready tasks\n• T-1 p1 — Fix thing\n• T-2 p2 — Test thing\nNext: use /board for WIP and blockers.",
            ),
            (
                super::ResponseKind::Status,
                "scheduler: OK\ngateway: OK",
                "🩺 Status\n• scheduler: OK\n• gateway: OK",
            ),
            (
                super::ResponseKind::Confirmation,
                "Remembered for this chat.",
                "✅ Done\n• Remembered for this chat.",
            ),
            (
                super::ResponseKind::Blocker,
                "Command unavailable",
                "⛔ Blocked\n• Command unavailable\nNext: ask an admin or use an allowed chat.",
            ),
        ];
        for (kind, input, expected) in cases {
            assert_eq!(super::format_telegram_response(kind, input), expected);
        }
    }

    #[test]
    fn formatter_preserves_small_tables_for_task_blocker_and_status_views() {
        let table = "| Item | State |\n|---|---|\n| T-1 | blocked |";
        for kind in [
            super::ResponseKind::TaskList,
            super::ResponseKind::Blocker,
            super::ResponseKind::Status,
        ] {
            let out = super::format_telegram_response(kind, table);
            assert!(out.contains("| Item | State |"), "{out}");
            assert!(out.contains("|---|---|"), "{out}");
            assert!(out.contains("| T-1 | blocked |"), "{out}");
        }
    }

    #[test]
    fn formatted_outputs_are_concise_and_chunk_safe() {
        let out = super::format_telegram_response(
            super::ResponseKind::Board,
            &format!("READY\n{}", "T-1 demo\n".repeat(420)),
        );
        assert!(out.chars().count() <= 4001, "{}", out.chars().count());
        let chunks = super::final_message_chunks(&out);
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|c| c.chars().count() <= 4000));
    }

    #[test]
    fn markdown_escape_covers_dynamic_control_chars() {
        let escaped = super::escape_markdown("a_b *c* [d](e) `f`");
        assert_eq!(escaped, "a\\_b \\*c\\* \\[d\\]\\(e\\) \\`f\\`");
    }

    #[test]
    fn blocker_formatter_uses_safe_sections() {
        let out =
            super::format_telegram_response(super::ResponseKind::Blocker, &super::safe_denial());
        assert!(out.starts_with("⛔ Blocked"), "{out}");
        assert!(out.contains("• Sorry,"), "{out}");
        assert!(out.contains("Next:"), "{out}");
    }

    #[test]
    fn slash_commands_accept_bot_suffix() {
        let out = slash_command("/help@smartagent_bot", "1", "", "u1")
            .expect("recognized")
            .unwrap();
        assert!(out.contains("/board"), "{out}");
        assert_eq!(slash_name("/memory@smartagent_bot status"), Some("memory"));
        assert!(slash_command("/unknown@smartagent_bot", "1", "", "u1").is_none());
    }

    #[test]
    fn slash_help_output_is_concise_and_streamable() {
        let out = slash_command("/help", "chat-a", "main", "u1")
            .expect("recognized")
            .unwrap();
        assert!(out.starts_with("SMARTAGENT Telegram commands:"), "{out}");
        assert!(
            out.contains("• /help — Show SMARTAGENT Telegram commands"),
            "{out}"
        );
        let preview = streaming_preview("/help", &out);
        assert!(
            preview.starts_with("💭 /help\nSMARTAGENT Telegram commands:"),
            "{preview}"
        );
        assert!(preview.chars().count() <= 4000, "{preview}");
    }

    #[test]
    fn slash_response_preview_is_streaming_shaped() {
        let preview = streaming_preview("/board", "READY\nT-1 demo");
        assert!(preview.starts_with("💭 /board\nREADY"), "{preview}");
        assert!(preview.ends_with('\u{2026}'), "{preview}");
    }

    #[test]
    fn progress_events_map_to_telegram_safe_messages() {
        let cases = [
            (ProgressEvent::Planning, "🧭 Planning the next step…"),
            (ProgressEvent::ToolUse, "🔧 Using tools…"),
            (ProgressEvent::Waiting, "⏳ Waiting for a result…"),
            (ProgressEvent::Verifying, "✅ Verifying before replying…"),
            (ProgressEvent::FinalAnswer, "💬 Final answer ready."),
        ];
        for (event, msg) in cases {
            assert_eq!(event.telegram_message(), msg);
            assert!(msg.chars().count() < 80, "{msg}");
            assert!(!msg.contains('\n'), "{msg}");
        }
    }

    #[test]
    fn progress_stream_lines_select_events_and_frames() {
        assert_eq!(
            progress_event_for_stream_line("calling tool browser"),
            ProgressEvent::ToolUse
        );
        assert_eq!(
            progress_event_for_stream_line("waiting for result"),
            ProgressEvent::Waiting
        );
        assert_eq!(
            progress_event_for_stream_line("verify tests"),
            ProgressEvent::Verifying
        );
        let frame = progress_frame(ProgressEvent::ToolUse, "running cargo test");
        assert!(frame.starts_with("🔧 Using tools…"), "{frame}");
        assert!(frame.contains("running cargo test"), "{frame}");
    }

    #[test]
    fn progress_scope_and_rate_limit_are_chat_thread_local() {
        assert_ne!(
            progress_scope_key("chat-a", "main"),
            progress_scope_key("chat-b", "main")
        );
        assert_ne!(
            progress_scope_key("chat-a", "main"),
            progress_scope_key("chat-a", "topic-2")
        );
        assert!(should_emit_progress(None, 0));
        assert!(!should_emit_progress(Some(1_000), 2_000));
        assert!(should_emit_progress(Some(1_000), 2_500));
    }

    #[test]
    fn final_message_chunks_preserve_order_without_duplicates() {
        let text = format!("A{}\nB{}", "a".repeat(3998), "b".repeat(100));
        let out = super::final_message_chunks(&text);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[0].starts_with('A'), "{out:?}");
        assert!(out[1].starts_with('B'), "{out:?}");
        assert_eq!(out.join(""), text);
        assert!(out.iter().all(|c| c.chars().count() <= 4000));
    }

    #[test]
    fn telegram_retry_classifier_is_narrow() {
        assert!(super::is_retryable_telegram_error(
            "Too Many Requests: retry after 3"
        ));
        assert!(super::is_retryable_telegram_error("curl timed out"));
        assert!(super::is_retryable_telegram_error(
            "HTTP 503 temporarily unavailable"
        ));
        assert!(!super::is_retryable_telegram_error(
            "Bad Request: chat not found"
        ));
    }

    #[test]
    fn observation_rows_capture_failures_without_secrets() {
        let mut db = test_db("telegram-observability");
        super::log_observation_to_db(
            &mut db,
            "command",
            "chat-a",
            "thread-1",
            "board",
            "ok",
            42,
            2,
            "tool_markers=1",
            "",
        )
        .unwrap();
        super::log_observation_to_db(
            &mut db,
            "send",
            "chat-a",
            "thread-1",
            "sendMessage",
            "error",
            7,
            1,
            "retry_failed",
            "Bearer token bot123 failed",
        )
        .unwrap();
        let rows = db
            .index
            .values()
            .map(|e| e.meta.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rows.contains("\"status\":\"ok\""), "{rows}");
        assert!(rows.contains("\"chunks\":2"), "{rows}");
        assert!(rows.contains("retry_failed"), "{rows}");
        assert!(!rows.contains("bot123"), "{rows}");
        assert!(rows.contains("[redacted]"), "{rows}");
    }

    #[test]
    fn blocked_resolution_keyboard_and_callbacks_are_parseable() {
        let kb = super::blocked_keyboard("T-123");
        assert!(kb.contains("inline_keyboard"), "{kb}");
        assert!(kb.contains("block:unblock:T-123"), "{kb}");
        assert!(kb.contains("block:reassign:T-123"), "{kb}");
        assert!(kb.contains("block:drop:T-123"), "{kb}");
        assert!(kb.contains("block:text:T-123"), "{kb}");
        let act = super::block_callback_action("block:text:T-123").unwrap();
        assert_eq!(act.action, "text");
        assert_eq!(act.id, "T-123");
        assert!(super::block_callback_action("model:codex/gpt-5.4-mini").is_none());
    }

    #[test]
    fn blocked_task_id_parser_ignores_no_tasks() {
        let ids = super::blocked_task_ids("T-1\tready\tp1\tDemo\nno tasks\nT-2\tdoing\tp2\tOther");
        assert_eq!(ids, vec!["T-1".to_string(), "T-2".to_string()]);
    }

    #[test]
    fn free_text_resolution_usage_is_safe() {
        let out = super::resolve_block_text("T-123").unwrap();
        assert!(out.contains("Usage: /resolve T-123"), "{out}");
    }

    #[test]
    fn telegram_tool_status_is_sanitized() {
        assert_eq!(
            super::telegram_tool_status("🛠 memory running…").unwrap(),
            "🛠 memory running…"
        );
        assert_eq!(super::telegram_tool_status("⚙ memory"), None);
        let preview = super::stream_preview("partial reply", "🛠 tasks ✓");
        assert!(preview.contains("partial reply"));
        assert!(preview.contains("🛠 tasks ✓"));
    }

    #[test]
    fn simulated_stream_frames_show_incremental_answer() {
        let frames = simulate_stream_frames(&["Hel", "Hello\\nworld"]);
        assert_eq!(frames[0], "Hel");
        assert_eq!(frames[1], "Hello\nworld");
    }

    #[test]
    fn simulated_stream_frames_show_tool_wrench_before_final() {
        let frames = simulate_stream_frames(&[
            "Starting",
            "__SMARTAGENT_INFO__🛠 memory running…",
            "Final answer",
        ]);
        assert!(frames[1].contains("Starting"), "{:?}", frames);
        assert!(frames[1].contains("🛠 memory running…"), "{:?}", frames);
        assert_eq!(frames[2], "Final answer");
    }

    #[test]
    fn simulated_stream_frames_fallback_when_thinking_tokens_unsupported() {
        let frames = simulate_stream_frames(&["__SMARTAGENT_THINKING__hidden chain"]);
        assert_eq!(frames[0], super::thinking_fallback());
        assert!(!frames[0].contains("hidden chain"), "{:?}", frames);
    }

    #[test]
    fn simulated_stream_frames_do_not_leak_between_chats() {
        let chat_a = simulate_stream_frames(&["chat A", "__SMARTAGENT_INFO__🛠 tasks ✓"]);
        let chat_b = simulate_stream_frames(&["chat B"]);
        assert!(chat_a.last().unwrap().contains("🛠 tasks ✓"), "{:?}", chat_a);
        assert_eq!(chat_b.last().unwrap(), "chat B");
        assert!(!chat_b.last().unwrap().contains("tasks"), "{:?}", chat_b);
    }

    #[test]
    fn history_scope_separates_chats_and_threads() {
        assert_ne!(
            super::history_scope("1", "main"),
            super::history_scope("2", "main")
        );
        assert_ne!(
            super::history_scope("1", "10"),
            super::history_scope("1", "11")
        );
        assert_eq!(super::history_scope("chat:1", ""), "chat_1:main");
    }

    fn test_db(name: &str) -> semdb::storage::Db {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch")
            .join(name);
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("telegram.semdb");
        let _ = std::fs::remove_file(&path);
        semdb::storage::Db::create(&path).unwrap()
    }

    #[test]
    fn history_prune_is_scope_local() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch/telegram-history");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("history-prune.semdb");
        let _ = std::fs::remove_file(&path);
        let mut db = semdb::storage::Db::create(&path).unwrap();
        for i in 0..4 {
            db.put(
                &format!("hist:chat_a:main:{i:020}:0:in"),
                "{}",
                super::VEC0.to_vec(),
            )
            .unwrap();
        }
        db.put(
            "hist:chat_b:main:00000000000000000000:0:in",
            "{}",
            super::VEC0.to_vec(),
        )
        .unwrap();
        super::prune_history(&mut db, "chat_a:main", 2).unwrap();
        assert_eq!(
            db.index
                .keys()
                .filter(|id| id.starts_with("hist:chat_a:main:"))
                .count(),
            2
        );
        assert!(db
            .index
            .contains_key("hist:chat_b:main:00000000000000000000:0:in"));
    }

    #[test]
    fn inbound_and_reply_history_round_trip_in_scope() {
        let mut db = test_db("telegram-history-roundtrip");
        super::log_history_to_db(
            &mut db,
            &super::HistoryEvent {
                direction: "in",
                chat: "chat-a",
                thread: "main",
                user: "u1",
                from: "oli",
                update_id: 1,
                message_id: "m1",
                reply_to_update: 0,
                ts: 10,
                text: "remember the blue key",
            },
        )
        .unwrap();
        super::log_history_to_db(
            &mut db,
            &super::HistoryEvent {
                direction: "out",
                chat: "chat-a",
                thread: "main",
                user: "bot",
                from: "assistant",
                update_id: 2,
                message_id: "m2",
                reply_to_update: 1,
                ts: 11,
                text: "blue key noted",
            },
        )
        .unwrap();
        let h = super::scoped_history_from_db(&db, "chat-a", "main").unwrap();
        assert!(h.contains("- oli: remember the blue key"), "{h}");
        assert!(h.contains("- assistant: blue key noted"), "{h}");
    }

    #[test]
    fn scoped_history_does_not_leak_between_chats() {
        let mut db = test_db("telegram-history-isolation");
        for (chat, text) in [("chat-a", "alpha secret"), ("chat-b", "beta secret")] {
            super::log_history_to_db(
                &mut db,
                &super::HistoryEvent {
                    direction: "in",
                    chat,
                    thread: "main",
                    user: "u1",
                    from: "user",
                    update_id: if chat == "chat-a" { 1 } else { 2 },
                    message_id: "m",
                    reply_to_update: 0,
                    ts: if chat == "chat-a" { 10 } else { 11 },
                    text,
                },
            )
            .unwrap();
        }
        let a = super::scoped_history_from_db(&db, "chat-a", "main").unwrap();
        let b = super::scoped_history_from_db(&db, "chat-b", "main").unwrap();
        assert!(
            a.contains("alpha secret") && !a.contains("beta secret"),
            "{a}"
        );
        assert!(
            b.contains("beta secret") && !b.contains("alpha secret"),
            "{b}"
        );
    }

    #[test]
    fn headless_telegram_simulation_context_persists_across_turns() {
        let mut db = test_db("telegram-headless-sim");
        super::log_history_to_db(
            &mut db,
            &super::HistoryEvent {
                direction: "in",
                chat: "sim-chat",
                thread: "main",
                user: "u1",
                from: "tester",
                update_id: 1,
                message_id: "m1",
                reply_to_update: 0,
                ts: 10,
                text: "My project codename is aurora",
            },
        )
        .unwrap();
        super::log_history_to_db(
            &mut db,
            &super::HistoryEvent {
                direction: "out",
                chat: "sim-chat",
                thread: "main",
                user: "agent",
                from: "agent",
                update_id: 2,
                message_id: "m2",
                reply_to_update: 1,
                ts: 11,
                text: "I will remember aurora in this chat context.",
            },
        )
        .unwrap();

        let second_turn_context = super::scoped_history_from_db(&db, "sim-chat", "main").unwrap();
        assert!(
            second_turn_context.contains("aurora"),
            "{second_turn_context}"
        );
        assert!(
            second_turn_context.contains("tester"),
            "{second_turn_context}"
        );
        assert!(
            second_turn_context.contains("agent"),
            "{second_turn_context}"
        );
    }

    #[test]
    fn reset_clears_only_current_chat_thread_history() {
        let mut db = test_db("telegram-history-reset");
        for (chat, thread, text, ts) in [
            ("chat-a", "main", "delete me", 10),
            ("chat-a", "topic-2", "keep thread", 11),
            ("chat-b", "main", "keep chat", 12),
        ] {
            super::log_history_to_db(
                &mut db,
                &super::HistoryEvent {
                    direction: "in",
                    chat,
                    thread,
                    user: "u1",
                    from: "user",
                    update_id: ts,
                    message_id: "m",
                    reply_to_update: 0,
                    ts,
                    text,
                },
            )
            .unwrap();
        }
        assert_eq!(
            super::reset_context_in_db(&mut db, "chat-a", "main").unwrap(),
            1
        );
        assert!(super::scoped_history_from_db(&db, "chat-a", "main").is_none());
        assert!(super::scoped_history_from_db(&db, "chat-a", "topic-2")
            .unwrap()
            .contains("keep thread"));
        assert!(super::scoped_history_from_db(&db, "chat-b", "main")
            .unwrap()
            .contains("keep chat"));
    }

    #[test]
    fn stop_cancel_token_is_scope_local() {
        let mut db = test_db("telegram-stop-scope");
        super::set_cancel_token_in_db(&mut db, "chat-a", "main", 10).unwrap();
        assert_eq!(
            super::cancel_token_from_db(&db, "chat-a", "main").unwrap(),
            10
        );
        assert_eq!(
            super::cancel_token_from_db(&db, "chat-a", "topic-2").unwrap(),
            0
        );
        assert_eq!(
            super::cancel_token_from_db(&db, "chat-b", "main").unwrap(),
            0
        );
        assert!(super::stop_requested_in_db(&db, "chat-a", "main", 9));
        assert!(!super::stop_requested_in_db(&db, "chat-a", "main", 10));
        assert!(!super::stop_requested_in_db(&db, "chat-b", "main", 9));
    }

    #[test]
    fn stop_command_is_listed_and_visible() {
        let body = command_menu_body();
        let help = command_help();
        assert!(body.contains("\"command\":\"stop\""), "{body}");
        assert!(help.contains("/stop"), "{help}");
        assert_eq!(slash_name("/stop@smartagent_bot"), Some("stop"));
    }

    #[test]
    fn remember_command_is_listed_and_usable() {
        let body = command_menu_body();
        let help = command_help();
        assert!(body.contains("\"command\":\"remember\""), "{body}");
        assert!(body.contains("Remember a fact"), "{body}");
        assert!(help.contains("/remember"), "{help}");
        assert_eq!(
            slash_name("/remember@smartagent_bot fact"),
            Some("remember")
        );
        let usage = slash_command("/remember", "50020485", "", "u1")
            .expect("recognized")
            .unwrap();
        assert!(usage.contains("Usage: /remember"), "{usage}");
    }

    #[test]
    fn command_permissions_are_classified() {
        assert_eq!(
            super::command_class("help"),
            Some(super::CommandClass::UserSafe)
        );
        assert_eq!(
            super::command_class("remember"),
            Some(super::CommandClass::ChatScoped)
        );
        assert_eq!(
            super::command_class("model"),
            Some(super::CommandClass::AdminOnly)
        );
        assert_eq!(
            super::command_class("status"),
            Some(super::CommandClass::AdminOnly)
        );
    }

    #[test]
    fn unauthorized_chat_gets_safe_denial_without_state() {
        let out = slash_command("/board", "not-allowed", "", "u1")
            .expect("recognized")
            .unwrap();
        assert_eq!(out, super::safe_denial());
        assert!(!out.contains("READY"), "{out}");
        assert!(!out.contains("T-"), "{out}");
    }

    #[test]
    fn admin_only_commands_require_admin_when_admins_configured() {
        assert!(super::authorize_slash_command_with_lists(
            "model",
            "chat-a",
            "admin-user",
            "chat-a",
            "admin-user",
            ""
        )
        .is_ok());
        assert!(super::authorize_slash_command_with_lists(
            "model",
            "chat-a",
            "normal-user",
            "chat-a",
            "admin-user",
            ""
        )
        .is_err());
        assert!(super::authorize_slash_command_with_lists(
            "remember",
            "chat-a",
            "normal-user",
            "chat-a",
            "admin-user",
            ""
        )
        .is_ok());
    }

    #[test]
    fn model_command_lists_keyboard_and_stores_scope_local_preference() {
        let body = command_menu_body();
        let help = command_help();
        assert!(body.contains("\"command\":\"model\""), "{body}");
        assert!(help.contains("/model"), "{help}");
        assert_eq!(
            super::normalize_model_choice("1"),
            Some(super::TELEGRAM_MODELS[0])
        );
        assert_eq!(
            super::normalize_model_choice(super::TELEGRAM_MODELS[1]),
            Some(super::TELEGRAM_MODELS[1])
        );
        let markup = super::model_menu_markup();
        assert!(markup.contains("inline_keyboard"), "{markup}");
        assert!(markup.contains("model:"), "{markup}");

        let mut db = test_db("telegram-model-pref");
        super::set_model_preference_in_db(
            &mut db,
            "chat-a",
            "main",
            "u1",
            super::TELEGRAM_MODELS[1],
        )
        .unwrap();
        assert_eq!(
            super::selected_model_from_db(&db, "chat-a", "main", "u1").as_deref(),
            Some(super::TELEGRAM_MODELS[1])
        );
        assert!(super::selected_model_from_db(&db, "chat-a", "main", "u2").is_none());
        assert!(super::selected_model_from_db(&db, "chat-b", "main", "u1").is_none());
    }

    #[test]
    fn model_callback_is_scoped_and_stale_protected() {
        let now = 100_000;
        assert_eq!(
            super::callback_model_choice(&format!("model:{}", super::TELEGRAM_MODELS[0]), now, now),
            Some(super::TELEGRAM_MODELS[0])
        );
        assert_eq!(super::callback_model_choice("other:data", now, now), None);
        assert_eq!(
            super::callback_model_choice(
                &format!("model:{}", super::TELEGRAM_MODELS[0]),
                now - super::CALLBACK_MAX_AGE_SECS - 1,
                now
            ),
            None
        );
    }

    #[test]
    fn model_callback_selection_returns_confirmation() {
        let ok = super::set_model_preference_in_db;
        let mut db = test_db("telegram-model-callback-confirm");
        ok(
            &mut db,
            "chat-a",
            "topic-1",
            "u1",
            super::TELEGRAM_MODELS[2],
        )
        .unwrap();
        assert_eq!(
            super::selected_model_from_db(&db, "chat-a", "topic-1", "u1").as_deref(),
            Some(super::TELEGRAM_MODELS[2])
        );
        assert!(
            super::set_model_preference("chat-a", "topic-1", "u1", "999")
                .unwrap()
                .contains("Choose a model")
        );
    }

    #[test]
    fn chunks_long_messages() {
        let s = "x".repeat(9000);
        let c = chunks(&s, 4096);
        assert_eq!(c.len(), 3);
        assert!(c.iter().all(|x| x.chars().count() <= 4096));
    }

    #[test]
    fn gateway_prompt_names_current_chat_scope() {
        let p1 = build_gateway_prompt("oli", "u1", "hello", "chat-a", "thread-1");
        let p2 = build_gateway_prompt("oli", "u1", "hello", "chat-b", "thread-2");
        assert!(p1.contains("chat/thread chat-a/thread-1"), "{p1}");
        assert!(p2.contains("chat/thread chat-b/thread-2"), "{p2}");
        assert!(!p1.contains("chat-b"), "{p1}");
        assert!(!p2.contains("chat-a"), "{p2}");
    }
}
