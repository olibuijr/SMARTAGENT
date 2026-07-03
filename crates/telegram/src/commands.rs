use super::*;

pub(crate) struct BotCommand {
    pub(crate) name: String,
    pub(crate) description: String,
}

const SLASH_COMMANDS_TSV: &str = include_str!("../../../config/slash_commands.tsv");
const TELEGRAM_AGENT_CHOICES: &[(&str, &str)] = &[
    ("coordinator", "Coordinator"),
    ("builder", "Builder"),
    ("qa", "QA"),
    ("ops", "Ops"),
];

pub(crate) fn telegram_commands() -> Vec<BotCommand> {
    parse_command_registry()
        .into_iter()
        .filter(|c| c.telegram && c.tui)
        .map(|c| BotCommand {
            name: c.name,
            description: c.description,
        })
        .collect()
}

#[derive(Clone, Debug)]
pub(crate) struct RegistryCommand {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) telegram: bool,
    pub(crate) tui: bool,
    pub(crate) class: String,
}

pub(crate) fn parse_command_registry() -> Vec<RegistryCommand> {
    SLASH_COMMANDS_TSV
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let cols = line.split('\t').collect::<Vec<_>>();
            if cols.len() < 5 {
                return None;
            }
            Some(RegistryCommand {
                name: cols[0].trim().to_string(),
                description: cols[1].trim().to_string(),
                telegram: cols[2].trim() == "true",
                tui: cols[3].trim() == "true",
                class: cols[4].trim().to_string(),
            })
        })
        .collect()
}

pub(crate) fn command_help() -> String {
    let mut out = String::from("SMARTAGENT commands:");
    for c in telegram_commands() {
        out.push_str(&format!("\n• /{} — {}", c.name, c.description));
    }
    out
}

pub(crate) fn command_menu_body() -> String {
    let commands = telegram_commands()
        .iter()
        .map(|c| {
            format!(
                r#"{{"command":"{}","description":"{}"}}"#,
                json::escape(&c.name),
                json::escape(&c.description)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"commands":[{commands}]}}"#)
}

pub(crate) fn slash_command(
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
        "index" => run("codeindex", if arg.is_empty() { vec!["index", "--all"] } else { vec!["index", arg] }.as_slice()),
        "projects" => run("codeindex", &["projects"]),
        "audit" => run("hooks", &["audit", "--n", "10", "--root", "."]),
        "goal" if !arg.is_empty() => {
            let parts = std::iter::once("set").chain(arg.split_whitespace()).collect::<Vec<_>>();
            run("goal", &parts)
        }
        "goal" => run("goal", &["status"]),
        "team" | "assign" => Ok(agent_assignment_menu_text()),
        "sab" => Ok("/sab is available in the TUI visual browser pane. From Telegram, assign this request to the agent team with /assign and choose Browser/Ops.".into()),
        "reset" => reset_context(chat, thread),
        "model" if !arg.is_empty() => set_model_preference(chat, thread, user, arg),
        "model" => Ok(model_menu_text(chat, thread, user)),
        "verbosity" | "verbose" if !arg.is_empty() => set_verbosity(chat, thread, user, arg),
        "verbosity" | "verbose" => Ok(verbosity_status(chat, thread, user)),
        "remember" => remember_context_fact(chat, thread, arg),
        "resolve" => resolve_block_text(arg),
        "stop" => stop_context(chat, thread),
        _ => return None, // unknown slash → treat as normal chat
    })
}

pub(crate) fn slash_name(text: &str) -> Option<&str> {
    let raw = text.trim().split_whitespace().next()?.strip_prefix('/')?;
    Some(raw.split('@').next().unwrap_or(raw))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandClass {
    UserSafe,
    ChatScoped,
    AdminOnly,
}

pub(crate) fn command_class(cmd: &str) -> Option<CommandClass> {
    let cmd = if cmd == "start" { "help" } else { cmd };
    let class = parse_command_registry()
        .into_iter()
        .find(|c| c.name == cmd)?
        .class;
    Some(match class.as_str() {
        "user" => CommandClass::UserSafe,
        "chat" => CommandClass::ChatScoped,
        "admin" => CommandClass::AdminOnly,
        _ => return None,
    })
}

pub(crate) fn authorize_slash_command(cmd: &str, chat: &str, user: &str) -> Result<(), String> {
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

pub(crate) fn authorize_slash_command_with_lists(
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

pub(crate) fn listed(list: &str, item: &str) -> bool {
    !item.is_empty() && list.split(',').map(str::trim).any(|v| v == item)
}

pub(crate) fn safe_denial() -> String {
    "Sorry, this Telegram command is not available for this chat/user.".into()
}

pub(crate) fn context_scope(chat: &str, thread: &str) -> String {
    if thread.is_empty() {
        format!("telegram chat {chat}")
    } else {
        format!("telegram chat {chat} thread {thread}")
    }
}

pub(crate) fn reset_context(chat: &str, thread: &str) -> Result<String, String> {
    let mut db = open_db()?;
    let n = reset_context_in_db(&mut db, chat, thread)?;
    Ok(format!(
        "Reset {} rolling context item(s) for this chat/thread.",
        n
    ))
}

pub(crate) fn reset_context_in_db(db: &mut Db, chat: &str, thread: &str) -> Result<usize, String> {
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

pub(crate) fn stop_context(chat: &str, thread: &str) -> Result<String, String> {
    let mut db = open_db()?;
    let token = cancel_token_from_db(&db, chat, thread)
        .unwrap_or(0)
        .max(unix_secs())
        + 1;
    set_cancel_token_in_db(&mut db, chat, thread, token)?;
    Ok("Stop requested for this chat/thread. I will halt the active streamed reply here; other chats are unaffected.".into())
}

pub(crate) fn cancel_key(chat: &str, thread: &str) -> String {
    format!("cancel:{}", history_scope(chat, thread))
}

pub(crate) fn set_cancel_token_in_db(
    db: &mut Db,
    chat: &str,
    thread: &str,
    token: u64,
) -> Result<(), String> {
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

pub(crate) fn cancel_token_from_db(db: &Db, chat: &str, thread: &str) -> Result<u64, String> {
    Ok(db
        .get(&cancel_key(chat, thread))
        .and_then(|e| json::parse(&e.meta).ok())
        .and_then(|v| u64v(v.get("token")))
        .unwrap_or(0))
}

pub(crate) fn cancel_token(chat: &str, thread: &str) -> Result<u64, String> {
    cancel_token_from_db(&open_db()?, chat, thread)
}

pub(crate) fn stop_requested_in_db(db: &Db, chat: &str, thread: &str, seen: u64) -> bool {
    cancel_token_from_db(db, chat, thread)
        .map(|t| t > seen)
        .unwrap_or(false)
}

pub(crate) fn stop_requested(chat: &str, thread: &str, seen: u64) -> bool {
    open_db()
        .map(|db| stop_requested_in_db(&db, chat, thread, seen))
        .unwrap_or(false)
}

pub(crate) fn remember_context_fact(
    chat: &str,
    thread: &str,
    fact: &str,
) -> Result<String, String> {
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum VerbosityLevel {
    Quiet,
    Normal,
    Verbose,
    Debug,
}

impl VerbosityLevel {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            VerbosityLevel::Quiet => "quiet",
            VerbosityLevel::Normal => "normal",
            VerbosityLevel::Verbose => "verbose",
            VerbosityLevel::Debug => "debug",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "quiet" | "q" => Some(VerbosityLevel::Quiet),
            "normal" | "n" => Some(VerbosityLevel::Normal),
            "verbose" | "v" => Some(VerbosityLevel::Verbose),
            "debug" | "d" => Some(VerbosityLevel::Debug),
            _ => None,
        }
    }
}

pub(crate) fn verbosity_key(chat: &str, thread: &str, user: &str) -> String {
    format!(
        "verbosity:{}:{}",
        history_scope(chat, thread),
        safe_id_part(user)
    )
}

fn scope_verbosity_key(chat: &str, thread: &str) -> String {
    verbosity_key(chat, thread, "*")
}

pub(crate) fn verbosity_from_db(db: &Db, chat: &str, thread: &str, user: &str) -> VerbosityLevel {
    let user_key = verbosity_key(chat, thread, user);
    let scope_key = scope_verbosity_key(chat, thread);
    [user_key, scope_key]
        .iter()
        .find_map(|k| db.get(k))
        .and_then(|e| json::parse(&e.meta).ok())
        .and_then(|v| {
            v.get("level")
                .and_then(Value::as_str)
                .and_then(VerbosityLevel::parse)
        })
        .unwrap_or(VerbosityLevel::Normal)
}

pub(crate) fn verbosity(chat: &str, thread: &str, user: &str) -> VerbosityLevel {
    open_db()
        .map(|db| verbosity_from_db(&db, chat, thread, user))
        .unwrap_or(VerbosityLevel::Normal)
}

pub(crate) fn set_verbosity_in_db(
    db: &mut Db,
    chat: &str,
    thread: &str,
    user: &str,
    level: VerbosityLevel,
) -> Result<(), String> {
    let key = verbosity_key(chat, thread, user);
    let meta = format!(
        r#"{{"kind":"telegram_verbosity","chat":"{}","thread":"{}","user":"{}","level":"{}","ts":{}}}"#,
        json::escape(chat),
        json::escape(thread),
        json::escape(user),
        level.as_str(),
        unix_secs()
    );
    db.put(&key, &meta, VEC0.to_vec())
}

pub(crate) fn set_verbosity(
    chat: &str,
    thread: &str,
    user: &str,
    choice: &str,
) -> Result<String, String> {
    let Some(level) = VerbosityLevel::parse(choice) else {
        return Ok("Usage: /verbosity quiet|normal|verbose|debug (alias: /verbose)".into());
    };
    let mut db = open_db()?;
    set_verbosity_in_db(&mut db, chat, thread, user, level)?;
    Ok(format!(
        "Telegram verbosity set to {} for this chat/thread/user.",
        level.as_str()
    ))
}

pub(crate) fn verbosity_status(chat: &str, thread: &str, user: &str) -> String {
    let level = verbosity(chat, thread, user);
    format!("Current Telegram verbosity: {}. Set with /verbosity quiet|normal|verbose|debug (or /verbose). quiet suppresses progress/task/workflow notifications; normal is default; verbose/debug allow more detail as producers add it.", level.as_str())
}

pub(crate) fn notification_allowed_for_level(level: VerbosityLevel, kind: &str) -> bool {
    match kind {
        "progress" | "task" | "workflow" => level >= VerbosityLevel::Normal,
        "debug" => level >= VerbosityLevel::Debug,
        _ => true,
    }
}

pub(crate) fn notification_allowed_in_db(
    db: &Db,
    chat: &str,
    thread: &str,
    user: &str,
    kind: &str,
) -> bool {
    notification_allowed_for_level(verbosity_from_db(db, chat, thread, user), kind)
}

pub(crate) fn notification_allowed(chat: &str, thread: &str, user: &str, kind: &str) -> bool {
    notification_allowed_for_level(verbosity(chat, thread, user), kind)
}

pub(crate) fn model_pref_key(chat: &str, thread: &str, user: &str) -> String {
    format!(
        "model:{}:{}",
        history_scope(chat, thread),
        safe_id_part(user)
    )
}

pub(crate) fn selected_model(chat: &str, thread: &str, user: &str) -> Option<String> {
    let db = open_db().ok()?;
    selected_model_from_db(&db, chat, thread, user)
}

pub(crate) fn selected_model_from_db(
    db: &Db,
    chat: &str,
    thread: &str,
    user: &str,
) -> Option<String> {
    let key = model_pref_key(chat, thread, user);
    db.get(&key)
        .and_then(|e| json::parse(&e.meta).ok())
        .and_then(|v| v.get("model").and_then(Value::as_str).map(str::to_string))
}

pub(crate) fn normalize_model_choice(choice: &str) -> Option<&'static str> {
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

pub(crate) fn set_model_preference(
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

pub(crate) fn set_model_preference_in_db(
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

pub(crate) fn model_menu_text(chat: &str, thread: &str, user: &str) -> String {
    let current =
        selected_model(chat, thread, user).unwrap_or_else(|| "default gateway model".into());
    let mut out =
        format!("Current model: {current}\nChoose a model with /model <number> or tap a button:");
    for (i, m) in TELEGRAM_MODELS.iter().enumerate() {
        out.push_str(&format!("\n{}. {m}", i + 1));
    }
    out
}

pub(crate) fn callback_model_choice(data: &str, message_date: u64, now: u64) -> Option<&str> {
    if now.saturating_sub(message_date) > CALLBACK_MAX_AGE_SECS {
        return None;
    }
    let model = data.strip_prefix("model:")?;
    normalize_model_choice(model)
}

pub(crate) fn model_menu_markup() -> String {
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

pub(crate) fn send_model_menu(chat: &str, thread: &str, user: &str) -> Result<String, String> {
    let token = bot_token()?;
    let text = model_menu_text(chat, thread, user);
    let mid =
        api::send_message_with_markup(&token, chat, &text, false, Some(&model_menu_markup()))?;
    Ok(format!("message_id={mid} models={}", TELEGRAM_MODELS.len()))
}

pub(crate) fn agent_assignment_menu_text() -> String {
    "Assign this Telegram command/request to the agent team. Choose an agent role:".into()
}

pub(crate) fn agent_assignment_menu_markup() -> String {
    let rows = TELEGRAM_AGENT_CHOICES
        .iter()
        .map(|(id, label)| {
            format!(
                r#"[{{"text":"{}","callback_data":"assign:{}"}}]"#,
                json::escape(label),
                json::escape(id)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(r#"{{"inline_keyboard":[{rows}]}}"#)
}

pub(crate) fn callback_agent_assignment(
    data: &str,
    message_date: u64,
    now: u64,
) -> Option<&'static str> {
    if now.saturating_sub(message_date) > CALLBACK_MAX_AGE_SECS {
        return None;
    }
    let role = data.strip_prefix("assign:")?;
    TELEGRAM_AGENT_CHOICES
        .iter()
        .find(|(id, _)| *id == role)
        .map(|(_, label)| *label)
}

pub(crate) fn send_agent_assignment_menu(chat: &str) -> Result<String, String> {
    let token = bot_token()?;
    let mid = api::send_message_with_markup(
        &token,
        chat,
        &agent_assignment_menu_text(),
        false,
        Some(&agent_assignment_menu_markup()),
    )?;
    Ok(format!(
        "message_id={mid} agents={}",
        TELEGRAM_AGENT_CHOICES.len()
    ))
}

pub(crate) fn bot_token() -> Result<String, String> {
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

pub(crate) fn allow_chat(chat: &str) -> Result<(), String> {
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

pub(crate) fn db_path() -> PathBuf {
    Config::load().data_dir().join("telegram.semdb")
}
pub(crate) fn log_observation(
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

pub(crate) fn log_observation_to_db(
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

pub(crate) fn telegram_status_report(limit: usize) -> String {
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

pub(crate) fn redact_secretish(s: &str) -> String {
    let mut out = Vec::new();
    for part in s.split_whitespace() {
        let lower = part.to_ascii_lowercase();
        let secret_key = [
            "password",
            "passwd",
            "secret",
            "token",
            "authorization",
            "bearer",
            "bot",
            "api_key",
            "apikey",
        ]
        .iter()
        .any(|m| lower.contains(m));
        if secret_key {
            if let Some((k, _)) = part.split_once('=') {
                out.push(format!("{k}=[redacted]"));
            } else if let Some((k, _)) = part.split_once(':') {
                out.push(format!("{k}:[redacted]"));
            } else {
                out.push("[redacted]".to_string());
            }
        } else {
            out.push(part.to_string());
        }
    }
    out.join(" ").chars().take(240).collect()
}

pub(crate) fn chunk_count(s: &str) -> usize {
    final_message_chunks(s).len()
}

pub(crate) fn open_db() -> Result<Db, String> {
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
pub(crate) fn offset() -> Result<u64, String> {
    Ok(open_db()?
        .get(ROW_OFFSET)
        .and_then(|e| json::parse(&e.meta).ok())
        .and_then(|v| u64v(v.get("offset")))
        .unwrap_or(0))
}
pub(crate) fn set_offset(n: u64) -> Result<(), String> {
    let mut db = open_db()?;
    db.put(ROW_OFFSET, &format!(r#"{{"offset":{n}}}"#), VEC0.to_vec())
}

pub(crate) fn u64v(v: Option<&Value>) -> Option<u64> {
    v.and_then(Value::as_f64).map(|x| x.max(0.0) as u64)
}
pub(crate) fn val_s(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        _ => format!("{}", v.as_f64().unwrap_or(0.0) as i64),
    }
}

pub(crate) fn chunks(s: &str, max: usize) -> Vec<String> {
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

pub(crate) const HELP: &str = r#"
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
