use super::*;

pub(crate) fn listen(args: &[String]) -> Result<String, String> {
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
pub(crate) fn handle_inbound(gateway_agent: Option<&str>, line: &str) {
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
    if slash_name(&text) == Some("assign")
        || (slash_name(&text).is_some() && text.contains(" assign"))
    {
        match send_agent_assignment_menu(&chat) {
            Ok(r) => eprintln!("[tg] assign menu sent to {chat}: {r}"),
            Err(e) => eprintln!("[tg] assign menu FAILED for {chat}: {e}"),
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
pub(crate) enum ResponseKind {
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

pub(crate) fn format_telegram_response(kind: ResponseKind, text: &str) -> String {
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

pub(crate) fn titled_response(title: &str, body: &str, next: Option<&str>) -> String {
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

pub(crate) fn normalize_response_body(text: &str) -> String {
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

pub(crate) fn looks_structured(text: &str) -> bool {
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

pub(crate) fn escape_markdown(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if matches!(ch, '_' | '*' | '[' | ']' | '(' | ')' | '`') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

pub(crate) fn stream_text_response(
    chat: &str,
    thread: &str,
    reply_to_update: u64,
    label: &str,
    text: &str,
) -> Result<String, String> {
    let token = bot_token()?;
    let mid = send_message_retry(&token, chat, thread, &format!("💭 {label} …"), false)?;
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

pub(crate) fn streaming_preview(label: &str, text: &str) -> String {
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
pub(crate) enum ProgressEvent {
    Planning,
    ToolUse,
    Waiting,
    Verifying,
    Error,
    FinalAnswer,
}

impl ProgressEvent {
    pub(crate) fn telegram_message(self) -> &'static str {
        match self {
            ProgressEvent::Planning => "🧭 Planning the next step…",
            ProgressEvent::ToolUse => "🔧 Using tools…",
            ProgressEvent::Waiting => "⏳ Waiting for a result…",
            ProgressEvent::Verifying => "✅ Verifying before replying…",
            ProgressEvent::Error => "⚠️ Error while working…",
            ProgressEvent::FinalAnswer => "💬 Final answer ready.",
        }
    }
}

pub(crate) fn progress_scope_key(chat: &str, thread: &str) -> String {
    format!("tgprogress:{}", history_scope(chat, thread))
}

pub(crate) fn should_emit_progress(last_ms: Option<u128>, now_ms: u128) -> bool {
    should_emit_progress_for_verbosity(VerbosityLevel::Verbose, last_ms, now_ms)
}

pub(crate) fn should_emit_progress_for_verbosity(
    level: VerbosityLevel,
    last_ms: Option<u128>,
    now_ms: u128,
) -> bool {
    if level == VerbosityLevel::Quiet || level == VerbosityLevel::Normal {
        return false;
    }
    const MIN_PROGRESS_EDIT_MS: u128 = 1_500;
    last_ms
        .map(|last| now_ms.saturating_sub(last) >= MIN_PROGRESS_EDIT_MS)
        .unwrap_or(true)
}

pub(crate) fn progress_event_for_stream_line(line: &str) -> ProgressEvent {
    let l = line.to_ascii_lowercase();
    if l.contains("tool") || l.contains("call") {
        ProgressEvent::ToolUse
    } else if l.contains("wait") || l.contains("queued") {
        ProgressEvent::Waiting
    } else if l.contains("verify") || l.contains("test") {
        ProgressEvent::Verifying
    } else if l.contains("error") || l.contains("failed") || l.contains("panic") {
        ProgressEvent::Error
    } else if l.contains("plan") || l.contains("think") {
        ProgressEvent::Planning
    } else {
        ProgressEvent::FinalAnswer
    }
}

pub(crate) fn progress_frame(event: ProgressEvent, body: &str) -> String {
    progress_frame_for_verbosity(VerbosityLevel::Debug, event, body)
}

pub(crate) fn progress_frame_for_verbosity(
    level: VerbosityLevel,
    event: ProgressEvent,
    body: &str,
) -> String {
    if event == ProgressEvent::FinalAnswer {
        return clip_tg(body);
    }
    match level {
        VerbosityLevel::Quiet | VerbosityLevel::Normal => String::new(),
        VerbosityLevel::Verbose => clip_tg(event.telegram_message()),
        VerbosityLevel::Debug => {
            let safe = sanitize_progress_body(body);
            if safe.is_empty() {
                clip_tg(event.telegram_message())
            } else {
                clip_tg(&format!("{}\n\n{}", event.telegram_message(), safe))
            }
        }
    }
}

pub(crate) fn sanitize_progress_body(body: &str) -> String {
    let redacted = redact_secretish(body);
    let first = redacted.lines().next().unwrap_or("").trim();
    first
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(240)
        .collect::<String>()
}

/// Streaming reply (hermes-style): send a placeholder, then live-edit it as
/// the agent generates. `gateway ask --stream` prints growing snapshots; we
/// throttle editMessageText to ~1 edit / 1.5s to respect Telegram rate limits.
pub(crate) fn stream_reply(
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
    let verbosity = verbosity(chat, thread, user);
    let show_progress = verbosity >= VerbosityLevel::Normal;
    let mid = send_message_retry(
        &token,
        chat,
        thread,
        if show_progress {
            ProgressEvent::Planning.telegram_message()
        } else {
            "Working…"
        },
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
                if verbosity >= VerbosityLevel::Verbose {
                    let preview = stream_preview(&latest, &status);
                    let _ = edit_message_retry(&token, chat, mid, &clip_tg(&preview), false);
                    shown = preview;
                    last_edit = std::time::Instant::now();
                }
            }
            continue;
        }
        if line.starts_with(GATEWAY_STREAM_THINKING_PREFIX) {
            let frame = thinking_fallback().to_string();
            if frame != shown
                && should_emit_progress_for_verbosity(
                    verbosity,
                    Some(0),
                    last_edit.elapsed().as_millis(),
                )
            {
                let _ = api::edit_message(&token, chat, mid, &frame, false);
                shown = frame;
                last_edit = std::time::Instant::now();
            }
            continue;
        }
        latest = line.replace("\\n", "\n"); // un-escape the streamed snapshot
        let event = progress_event_for_stream_line(&latest);
        let frame = progress_frame_for_verbosity(verbosity, event, &latest);
        // Throttle: edit at most every 1.5s, only when the text grew.
        if !frame.is_empty()
            && frame != shown
            && should_emit_progress_for_verbosity(
                verbosity,
                Some(0),
                last_edit.elapsed().as_millis(),
            )
        {
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

pub(crate) fn telegram_tool_status(info: &str) -> Option<String> {
    let s = info.trim();
    if !s.starts_with('🛠') {
        return None;
    }
    let redacted = redact_secretish(s);
    let safe = redacted
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

pub(crate) fn stream_preview(reply: &str, status: &str) -> String {
    let base = if reply.trim().is_empty() {
        "💭 …"
    } else {
        reply.trim_end()
    };
    format!("{base}\n\n{status}")
}

pub(crate) fn thinking_fallback() -> &'static str {
    "💭 Thinking tokens are not available from this model; streaming visible output instead."
}

#[cfg(test)]
pub(crate) fn simulate_stream_frames(lines: &[&str]) -> Vec<String> {
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
pub(crate) fn build_gateway_prompt(
    from: &str,
    user: &str,
    text: &str,
    chat: &str,
    thread: &str,
) -> String {
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

pub(crate) fn scoped_context(chat: &str, thread: &str, text: &str) -> Option<String> {
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

pub(crate) fn scoped_history(chat: &str, thread: &str) -> Option<String> {
    let db = open_db().ok()?;
    scoped_history_from_db(&db, chat, thread)
}

pub(crate) fn scoped_history_from_db(db: &Db, chat: &str, thread: &str) -> Option<String> {
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

pub(crate) fn scoped_memories(chat: &str, thread: &str, text: &str) -> Option<String> {
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

pub(crate) fn send_message_retry(
    token: &str,
    chat: &str,
    thread: &str,
    text: &str,
    markdown: bool,
) -> Result<i64, String> {
    match retry_telegram(|| api::send_message_in_thread(token, chat, thread, text, markdown)) {
        Ok(v) => Ok(v),
        Err(e) => {
            let _ = log_observation(
                "send",
                chat,
                thread,
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

pub(crate) fn edit_message_retry(
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

pub(crate) fn retry_telegram<T, F>(mut f: F) -> Result<T, String>
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

pub(crate) fn is_retryable_telegram_error(e: &str) -> bool {
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

pub(crate) fn finish_streamed_message(
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
        send_message_retry(token, chat, "", chunk, false)?;
    }
    Ok(())
}

pub(crate) fn final_message_chunks(final_text: &str) -> Vec<String> {
    chunks(
        if final_text.is_empty() {
            "(no output)"
        } else {
            final_text
        },
        4000,
    )
}

pub(crate) fn clip_tg(s: &str) -> String {
    if s.chars().count() > 4000 {
        format!("{}…", s.chars().take(4000).collect::<String>())
    } else {
        s.to_string()
    }
}

pub(crate) struct HistoryEvent<'a> {
    pub(crate) direction: &'a str,
    pub(crate) chat: &'a str,
    pub(crate) thread: &'a str,
    pub(crate) user: &'a str,
    pub(crate) from: &'a str,
    pub(crate) update_id: u64,
    pub(crate) message_id: &'a str,
    pub(crate) reply_to_update: u64,
    pub(crate) ts: u64,
    pub(crate) text: &'a str,
}

pub(crate) fn log_history(ev: &HistoryEvent<'_>) -> Result<(), String> {
    let mut db = open_db()?;
    log_history_to_db(&mut db, ev)
}

pub(crate) fn log_history_to_db(db: &mut Db, ev: &HistoryEvent<'_>) -> Result<(), String> {
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

pub(crate) fn history_scope(chat: &str, thread: &str) -> String {
    format!(
        "{}:{}",
        safe_id_part(chat),
        safe_id_part(if thread.is_empty() { "main" } else { thread })
    )
}

pub(crate) fn safe_id_part(s: &str) -> String {
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

pub(crate) fn prune_history(db: &mut Db, scope: &str, keep: usize) -> Result<(), String> {
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

pub(crate) fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
