use super::*;

pub(crate) fn blocked_scan(args: &[String]) -> Result<String, String> {
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

pub(crate) fn allowed_chats_csv() -> String {
    Config::load()
        .resolve(
            "telegram_allowed_chats",
            "SMARTAGENT_TELEGRAM_ALLOWED_CHATS",
            None,
        )
        .unwrap_or_default()
}

pub(crate) fn blocked_task_ids(list_output: &str) -> Vec<String> {
    list_output
        .lines()
        .filter(|l| !l.trim().is_empty() && *l != "no tasks")
        .filter_map(|l| l.split('\t').next())
        .filter(|id| id.starts_with('T'))
        .map(str::to_string)
        .collect()
}

pub(crate) fn blocked_alert_text(id: &str) -> String {
    let show = Command::new("target/release/tasks")
        .args(["show", id])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_else(|| id.to_string());
    format!("⛔ Blocked task needs resolution\n{}", clip_tg(&show))
}

pub(crate) fn blocked_keyboard(id: &str) -> String {
    format!(
        r#"{{"inline_keyboard":[[{u},{r}],[{d},{t}]]}}"#,
        u = block_button("Unblock", "unblock", id),
        r = block_button("Reassign", "reassign", id),
        d = block_button("Drop", "drop", id),
        t = block_button("Own text", "text", id),
    )
}

pub(crate) fn block_button(label: &str, action: &str, id: &str) -> String {
    format!(
        r#"{{"text":"{}","callback_data":"block:{}:{}"}}"#,
        json::escape(label),
        json::escape(action),
        json::escape(id)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockAction {
    pub(crate) action: String,
    pub(crate) id: String,
}

pub(crate) fn block_callback_action(data: &str) -> Option<BlockAction> {
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

pub(crate) fn handle_block_callback(
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

pub(crate) fn apply_block_action(act: &BlockAction) -> Result<String, String> {
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

pub(crate) fn resolve_block_text(arg: &str) -> Result<String, String> {
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

pub(crate) fn run_tasks_action(args: &[&str]) -> Result<String, String> {
    let out = Command::new("target/release/tasks")
        .args(args)
        .output()
        .map_err(|e| format!("tasks: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub(crate) fn answer_callback(token: &str, callback_id: &str, text: &str) -> Result<(), String> {
    let body = format!(
        r#"{{"callback_query_id":"{}","text":"{}","show_alert":false}}"#,
        json::escape(callback_id),
        json::escape(&clip_tg(text))
    );
    api::call(token, "answerCallbackQuery", &body).map(|_| ())
}
