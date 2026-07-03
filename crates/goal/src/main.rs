//! goal — keep the agent working until an INDEPENDENTLY-verified condition holds.
//!
//! Mirrors Claude Code's `/goal`: set a completion condition, and after every
//! turn a small fast model (NOT the one doing the work) reads the transcript and
//! decides met / not-met. `check` emits the hooks stop-decision — `block` with
//! the reason keeps the agent going; a met goal clears and lets the turn end.
//!
//!   goal set "<condition>"                 set/replace the active goal
//!   goal status                            show condition, turns, last reason
//!   goal clear|stop|off|reset|none|cancel  drop the active goal
//!   goal check [--sessions DIR] [--transcript F] [--model M] [--max-chars N]
//!                                          evaluate (stop-hook entry point)
//!   goal run "<prompt>"                    spawn headless ./pi turns until check passes
//! All verbs accept --session ID (or PI_SESSION_ID) so gateway agents do not
//! clobber each other's active goals.

use httpc::args::flag;
use std::process::{Command, ExitCode, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

mod eval;
mod store;
mod transcript;

use store::Goal;

const CLEAR_ALIASES: [&str; 6] = ["clear", "stop", "off", "reset", "none", "cancel"];
const DEFAULT_EVAL_MODEL: &str = "claude/claude-haiku-4-5-20251001";
const DEFAULT_SESSIONS_DIR: &str = ".pi/sessions";
const DEFAULT_MAX_CHARS: usize = 24_000;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("help") | Some("--help") | Some("-h") => Ok(HELP.into()),
        Some("set") => {
            let condition = rest(args, 1);
            if condition.trim().is_empty() {
                return Err("usage: goal set \"<condition>\"".into());
            }
            if condition.chars().count() > 4000 {
                return Err("condition too long (max 4000 chars)".into());
            }
            let session = session_key(args);
            let g = Goal::new(condition.trim(), now());
            store::save_for(&session, &g)?;
            Ok(format!(
                "◎ goal set — working until: {}\n(a fresh evaluator checks after each turn; `goal clear` to stop)",
                g.condition
            ))
        }
        Some("status") | None => match store::load_for(&session_key(args))? {
            None => Ok("no goal set".into()),
            Some(g) => Ok(render_status(&g)),
        },
        Some(a) if CLEAR_ALIASES.contains(&a) => {
            let session = session_key(args);
            match store::load_for(&session)? {
                Some(mut g) if g.is_active() => {
                    g.status = "cleared".into();
                    store::save_for(&session, &g)?;
                    Ok("◎ goal cleared".into())
                }
                _ => Ok("no active goal to clear".into()),
            }
        }
        Some("check") => check(args),
        Some("run") => run_driver(args),
        Some(other) => Err(format!(
            "unknown verb '{other}' — try: set|status|clear|check|run|help"
        )),
    }
}

/// Stop-hook entry point. Prints a hooks decision on stdout (exit 0):
///   not met  → {"decision":"block","reason":"..."}   (agent keeps working)
///   met/none → nothing                                 (turn ends normally)
fn check(args: &[String]) -> Result<String, String> {
    let session = session_key(args);
    let Some(mut g) = store::load_for(&session)? else {
        return Ok(String::new()); // no goal ever set
    };
    if !g.is_active() {
        return Ok(String::new()); // achieved or cleared — do not block
    }

    let max_chars = flag(args, "--max-chars")
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_CHARS);
    let tr = match flag(args, "--transcript") {
        Some(f) => transcript::parse_file(std::path::Path::new(&f), max_chars)?,
        None => {
            let dir = flag(args, "--sessions").unwrap_or_else(|| DEFAULT_SESSIONS_DIR.to_string());
            transcript::latest_for_session(std::path::Path::new(&dir), max_chars, &session)?
        }
    };

    let model = eval_model(args, &tr.worker_model);
    let verdict = eval::evaluate(&g, &tr, &model)?;

    g.turns += 1;
    g.last_reason = verdict.reason.clone();
    if verdict.met {
        g.status = "achieved".into();
        store::save_for(&session, &g)?;
        // Turn ends normally; announce achievement on stderr (non-blocking note).
        eprintln!(
            "◎ goal achieved after {} turn(s): {}",
            g.turns, verdict.reason
        );
        Ok(String::new())
    } else {
        store::save_for(&session, &g)?;
        Ok(format!(
            r#"{{"decision":"block","reason":"◎ GOAL NOT MET (turn {}): {}. Keep working toward the goal: {}"}}"#,
            g.turns,
            httpc::json::escape(verdict.reason.trim_end_matches(['.', ' ', '!'])),
            httpc::json::escape(&g.condition)
        ))
    }
}

fn run_driver(args: &[String]) -> Result<String, String> {
    let prompt = rest(args, 1);
    if prompt.trim().is_empty() {
        return Err("usage: goal run \"<prompt>\"".into());
    }
    let session = session_key(args);
    let max_turns = flag(args, "--max-turns")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(10)
        .clamp(1, 100);
    let mut last = String::new();
    for turn in 1..=max_turns {
        let status = Command::new("./pi")
            .arg("-p")
            .arg(&prompt)
            .env("PI_SESSION_ID", &session)
            .stdin(Stdio::null())
            .status()
            .map_err(|e| format!("spawn ./pi failed: {e}"))?;
        if !status.success() {
            return Err(format!("./pi turn {turn} exited with {status}"));
        }
        last = check(args)?;
        if last.trim().is_empty() {
            return Ok(format!("◎ goal run complete after {turn} turn(s)"));
        }
    }
    Ok(format!(
        "◎ goal run stopped after {max_turns} turn(s); last check: {last}"
    ))
}

/// Evaluator model: flag > config `goal_eval_model` > default. Verifier
/// independence is best when it differs from the worker model.
fn eval_model(args: &[String], worker: &str) -> String {
    let chosen = semdb::config::Config::load()
        .resolve(
            "goal_eval_model",
            "GOAL_EVAL_MODEL",
            flag(args, "--model").as_deref(),
        )
        .unwrap_or_else(|| DEFAULT_EVAL_MODEL.to_string());
    if !worker.is_empty() && chosen == worker {
        eprintln!("note: goal evaluator uses the same model as the worker ({worker}); set goal_eval_model to a different model for independent verification");
    }
    chosen
}

fn session_key(args: &[String]) -> String {
    flag(args, "--session")
        .or_else(|| std::env::var("PI_SESSION_ID").ok())
        .unwrap_or_else(|| "current".to_string())
}

fn render_status(g: &Goal) -> String {
    let secs = (now() - g.created_at).max(0);
    let dur = format!("{}m{}s", secs / 60, secs % 60);
    let head = match g.status.as_str() {
        "active" => format!("◎ goal ACTIVE ({dur}, {} turn(s) evaluated)", g.turns),
        "achieved" => format!("◎ goal ACHIEVED after {} turn(s)", g.turns),
        s => format!("◎ goal {} ({} turn(s))", s.to_uppercase(), g.turns),
    };
    let reason = if g.last_reason.is_empty() {
        String::new()
    } else {
        format!("\nlast check: {}", g.last_reason)
    };
    format!("{head}\ncondition: {}{reason}", g.condition)
}

fn rest(args: &[String], from: usize) -> String {
    let mut out = Vec::new();
    let mut skip_next = false;
    for a in args.iter().skip(from) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if matches!(
            a.as_str(),
            "--session" | "--max-turns" | "--sessions" | "--transcript" | "--model" | "--max-chars"
        ) {
            skip_next = true;
            continue;
        }
        if a.starts_with("--") {
            continue;
        }
        out.push(a.clone());
    }
    out.join(" ")
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

const HELP: &str = r#"goal — work until an independently-verified condition holds

  goal set "<condition>"     start a goal (replaces any active one)
  goal status                condition, turns evaluated, last reason
  goal clear                 drop the active goal (aliases: stop off reset none cancel)
  goal check                 evaluate now; prints a hooks block-decision if unmet
  goal run "<prompt>"        run repeated `./pi -p` turns until check passes
    --sessions DIR           session transcripts dir (default .pi/sessions)
    --transcript FILE        evaluate a specific transcript instead
    --model M                evaluator model (default: config goal_eval_model)
    --max-chars N            transcript budget fed to the evaluator (default 24000)
    --session ID             isolate state for one gateway/pi session
    --max-turns N            run driver turn limit (default 10)

The evaluator is a small model that only reads the transcript — it never grades
its own work. Write conditions the transcript can prove, e.g. "cargo test passes
(exit 0) and no file over 1000 lines". Bound a run with "... or stop after N turns".

Interactive TUI caveat: pi's agent_end hook is notification-only. It can surface
an unmet-goal reason, but it cannot itself start the next interactive turn; use
`goal run` or the gateway driver for fully unattended continuation."#;
