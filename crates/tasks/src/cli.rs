//! CLI: add / todo / board / list / show / move / next / crit / block / done /
//! rm / wip / metrics / statusline. Kanban policies (WIP limits, pull-based
//! `next`, criteria-gated done) are enforced HERE, deterministically — not in
//! prompts.

use semdb::storage::Db;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use httpc::args::{flag, has};

use crate::board;
use crate::store::{now, Store, Task, COLUMNS};
use crate::worktree;

fn db_path(args: &[String]) -> Result<PathBuf, String> {
    // --project <name> = that workspace repo's own board
    // (<repo>/.smartagent/tasks.semdb) — tasks never mix between repos.
    if let Some(p) = flag(args, "--project") {
        return semdb::workspace::data_path(&p, "tasks.semdb");
    }
    Ok(flag(args, "--db")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/tasks.semdb")))
}

fn valid_col(c: &str) -> Result<(), String> {
    if COLUMNS.contains(&c) {
        Ok(())
    } else {
        Err(format!("unknown column '{c}' ({})", COLUMNS.join("|")))
    }
}

fn current_owner() -> String {
    std::env::var("SMARTAGENT_TASK_OWNER")
        .or_else(|_| std::env::var("SMARTAGENT_GATEWAY_AGENT"))
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "agent".into())
}

fn is_fleet_off_limits(t: &Task) -> bool {
    t.tags.iter().any(|tag| tag == "desktop-agent") || t.title.contains("desktop-agent")
}

fn task_created_notification_text(t: &Task) -> String {
    let criteria = if t.criteria.is_empty() {
        "none".to_string()
    } else {
        format!("{} item(s)", t.criteria.len())
    };
    let tags = if t.tags.is_empty() {
        "none".to_string()
    } else {
        t.tags.join(", ")
    };
    format!(
        "🗂 *New SMARTAGENT task*\n\n*ID:* `{}`\n*Priority:* `{}`\n*State:* `{}`\n*Title:* {}\n*Tags:* {}\n*Criteria:* {}",
        t.id, t.prio, t.col, t.title, tags, criteria
    )
}

fn notify_task_created(t: &Task) -> String {
    if std::env::var("SMARTAGENT_TASK_NOTIFY_DISABLE").is_ok() {
        return String::new();
    }
    let text = task_created_notification_text(t);
    match Command::new("target/release/telegram")
        .args(["broadcast", "--kind", "task", "--text", &text])
        .output()
    {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            format!(
                "\ntelegram notify: {}",
                if s.is_empty() { "sent" } else { &s }
            )
        }
        Ok(out) => format!(
            "\ntelegram notify failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => format!("\ntelegram notify failed: {e}"),
    }
}

fn task_lifecycle_notification(event: &str, t: &Task, actor: &str, note: Option<&str>) -> String {
    let mut lines = vec![
        format!("🤖 *Agent task update:* {event}"),
        format!("*Agent:* `{actor}`"),
        format!("*Task:* `{}` — {}", t.id, t.title),
        format!("*Priority:* `{}` · *State:* `{}`", t.prio, t.col),
    ];
    if !t.tags.is_empty() {
        lines.push(format!("*Tags:* `{}`", t.tags.join(",")));
    }
    if !t.criteria.is_empty() {
        lines.push(format!(
            "*Criteria:* `{}/{}`",
            t.criteria_done(),
            t.criteria.len()
        ));
    }
    if let Some(note) = note.filter(|s| !s.trim().is_empty()) {
        lines.push(format!("*Note:* {note}"));
    }
    lines.push(
        "*Next:* agents continue from the board; no action needed unless you want to steer.".into(),
    );
    lines.join("\n")
}

fn notify_task_lifecycle(event: &str, t: &Task, actor: &str, note: Option<&str>) {
    if std::env::var("SMARTAGENT_TASK_NOTIFY_DISABLE").is_ok() {
        return;
    }
    let text = task_lifecycle_notification(event, t, actor, note);
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("telegram"));
        }
    }
    candidates.push(PathBuf::from("target/release/telegram"));
    for bin in candidates {
        if bin.exists() {
            let _ = Command::new(bin)
                .arg("broadcast")
                .arg("--text")
                .arg(&text)
                .status();
            break;
        }
    }
}

fn event_db_path() -> PathBuf {
    std::env::var("SMARTAGENT_TASK_EVENTS_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/task-events.semdb"))
}

fn log_task_event(event: &str, t: &Task, actor: &str, from: &str, to: &str, reason: &str) {
    if std::env::var("SMARTAGENT_TASK_EVENT_LOG_DISABLE").is_ok() {
        return;
    }
    let path = event_db_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut db = match if path.exists() {
        Db::open(&path)
    } else {
        Db::create(&path)
    } {
        Ok(db) => db,
        Err(_) => return,
    };
    let ts = now();
    let id = format!("{ts:020}-{}-{}", t.id, event);
    let meta = format!(
        "{{\"ts\":{ts},\"event\":\"{}\",\"task\":\"{}\",\"actor\":\"{}\",\"owner\":\"{}\",\"from\":\"{}\",\"to\":\"{}\",\"reason\":\"{}\",\"title\":\"{}\"}}",
        semdb::json::escape(event), semdb::json::escape(&t.id), semdb::json::escape(actor),
        semdb::json::escape(&t.owner), semdb::json::escape(from), semdb::json::escape(to),
        semdb::json::escape(reason), semdb::json::escape(&t.title)
    );
    let _ = db.put(&id, &meta, vec![0.0]);
}

fn render_task_events(limit: usize) -> String {
    let path = event_db_path();
    if !path.exists() {
        return "no task events".into();
    }
    let db = match Db::open(&path) {
        Ok(db) => db,
        Err(e) => return format!("task events unavailable: {e}"),
    };
    let mut rows: Vec<_> = db.index.iter().collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    let out: Vec<String> = rows
        .into_iter()
        .rev()
        .take(limit)
        .filter_map(|(_, e)| {
            let v = semdb::json::parse(&e.meta).ok()?;
            Some(format!(
                "{}\t{}\t{}\t{}→{}\t@{}\t{}",
                v.get("ts")
                    .and_then(|x| x.as_f64())
                    .map(|n| n as i64)
                    .unwrap_or(0),
                v.get("event").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("task").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("from").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("to").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("actor").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("reason").and_then(|x| x.as_str()).unwrap_or("")
            ))
        })
        .collect();
    if out.is_empty() {
        "no task events".into()
    } else {
        out.join("\n")
    }
}

fn cargo_done_criteria(t: &Task) -> Vec<String> {
    t.criteria
        .iter()
        .filter(|(_, done)| *done)
        .map(|(c, _)| c.trim())
        .filter(|c| {
            let mut parts = c.split_whitespace();
            matches!(parts.next(), Some("cargo"))
                && matches!(parts.next(), Some("test" | "build" | "check"))
        })
        .map(str::to_string)
        .collect()
}

fn done_check_timeout() -> Duration {
    let secs = std::env::var("SMARTAGENT_TASKS_DONE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(120);
    Duration::from_secs(secs)
}

fn run_cargo_done_checks(t: &Task) -> Result<(), String> {
    let checks = cargo_done_criteria(t);
    if checks.is_empty() {
        return Ok(());
    }
    let cwd = if worktree::has_worktree(&t.id) {
        worktree::main_checkout_path()?
    } else {
        std::env::current_dir().map_err(|e| e.to_string())?
    };
    let timeout = done_check_timeout();
    for check in checks {
        run_one_cargo_done_check(&check, &cwd, timeout)?;
    }
    Ok(())
}

fn run_one_cargo_done_check(
    check: &str,
    cwd: &std::path::Path,
    timeout: Duration,
) -> Result<(), String> {
    let mut parts = check.split_whitespace();
    let program = parts.next().ok_or("empty cargo criterion")?;
    let args = parts.collect::<Vec<_>>();
    let mut child = Command::new(program)
        .args(&args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("run done criterion `{check}` in {}: {e}", cwd.display()))?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if status.success() {
                return Ok(());
            }
            let mut stderr = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut e, &mut stderr);
            }
            return Err(format!(
                "done criterion `{check}` failed with {status}{}",
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", stderr.trim())
                }
            ));
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "done criterion `{check}` timed out after {}s",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn run(args: &[String]) -> Result<String, String> {
    let store = Store::open(&db_path(args)?)?;
    match args.first().map(String::as_str) {
        Some("add") | Some("todo") => {
            let quick = args[0] == "todo"; // todo = frictionless capture into backlog
            let title = args.get(1).filter(|t| !t.starts_with("--")).ok_or("usage: tasks add '<title>' [--prio p1|p2|p3] [--col C] [--tags a,b] [--criteria 'a;b']")?;
            let prio =
                flag(args, "--prio")
                    .unwrap_or_else(|| if quick { "p3".into() } else { "p2".into() });
            if !matches!(prio.as_str(), "p1" | "p2" | "p3") {
                return Err("prio must be p1|p2|p3".into());
            }
            // `add` queues to READY by default — the autonomous fleet only
            // auto-pulls from ready, so backlog-by-default stalled real work
            // (Óli 2026-07-02: tasks must be queued + handed off on creation).
            // `todo` stays backlog: frictionless idea capture, triaged later.
            let col = if quick {
                "backlog".to_string()
            } else {
                flag(args, "--col").unwrap_or_else(|| "ready".into())
            };
            valid_col(&col)?;
            let t = Task {
                id: store.next_id()?,
                title: title.clone(),
                col: col.clone(),
                prio,
                tags: flag(args, "--tags")
                    .map(|s| s.split(',').map(str::to_string).collect())
                    .unwrap_or_default(),
                criteria: flag(args, "--criteria")
                    .map(|s| {
                        s.split(';')
                            .map(|c| (c.trim().to_string(), false))
                            .filter(|(c, _)| !c.is_empty())
                            .collect()
                    })
                    .unwrap_or_default(),
                created: now(),
                trans: vec![(col, now())],
                ..Task::default()
            };
            store.put(&t)?;
            let note = notify_task_created(&t);
            Ok(format!("{} added to {}{}", t.id, t.col, note))
        }
        Some("board") => Ok(board::render(&store.all()?, store.wip())),
        Some("list") => {
            let mut ts = store.all()?;
            if let Some(c) = flag(args, "--col") {
                valid_col(&c)?;
                ts.retain(|t| t.col == c);
            }
            if let Some(tag) = flag(args, "--tag") {
                ts.retain(|t| t.tags.iter().any(|x| x == &tag));
            }
            if let Some(owner) = flag(args, "--owner") {
                ts.retain(|t| t.owner == owner);
            }
            if has(args, "--mine") {
                let me = current_owner();
                ts.retain(|t| t.owner == me);
            }
            if has(args, "--others") {
                let me = current_owner();
                ts.retain(|t| t.owner != me && !t.owner.is_empty());
            }
            if has(args, "--blocked") {
                ts.retain(|t| !t.blocked.is_empty());
            }
            if ts.is_empty() {
                return Ok("no tasks".into());
            }
            Ok(ts
                .iter()
                .map(|t| {
                    let owner = if t.owner.is_empty() {
                        String::new()
                    } else {
                        format!(" @{}", t.owner)
                    };
                    format!("{}\t{}\t{}\t{}{}", t.id, t.col, t.prio, t.title, owner)
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("show") => {
            let t = store.get(args.get(1).ok_or("usage: tasks show T-1")?)?;
            let mut out = vec![format!("{} [{}] {} — {}", t.id, t.prio, t.col, t.title)];
            if !t.tags.is_empty() {
                out.push(format!("tags: {}", t.tags.join(",")));
            }
            if !t.owner.is_empty() {
                out.push(format!("owner: {}", t.owner));
            }
            if !t.blocked.is_empty() {
                out.push(format!("BLOCKED: {}", t.blocked));
            }
            for (i, (c, done)) in t.criteria.iter().enumerate() {
                out.push(format!(
                    "  [{}] {}. {}",
                    if *done { "x" } else { " " },
                    i + 1,
                    c
                ));
            }
            for (c, ts) in &t.trans {
                out.push(format!("  → {c} @{ts}"));
            }
            Ok(out.join("\n"))
        }
        Some("move") => {
            let id = args.get(1).ok_or("usage: tasks move T-1 <column>")?;
            let col = args.get(2).ok_or("column required")?;
            valid_col(col)?;
            let mut t = store.get(id)?;
            let actor = current_owner();
            if col == "doing" && !has(args, "--force") {
                if t.col != "ready" {
                    return Err(format!(
                        "{} is already in {}{} — do not double-assign; pull another ready task",
                        t.id,
                        t.col,
                        if t.owner.is_empty() {
                            String::new()
                        } else {
                            format!(" owned by @{}", t.owner)
                        }
                    ));
                }
                if !t.owner.is_empty() && t.owner != actor {
                    return Err(format!("{} is already owned by @{} — do not double-assign; pull another ready task", t.id, t.owner));
                }
            }
            let wip = store.wip();
            let count_in = |c: &str| {
                store
                    .all()
                    .map(|ts| ts.iter().filter(|x| x.col == c && x.id != t.id).count())
                    .unwrap_or(0)
            };
            // WIP limits are HARD unless --force: pull, don't push.
            if !has(args, "--force") {
                if col == "doing" && count_in("doing") >= wip.doing {
                    return Err(format!("WIP limit: doing already has {}/{} — finish or move something out first (--force to override)", count_in("doing"), wip.doing));
                }
                if col == "review" && count_in("review") >= wip.review {
                    return Err(format!(
                        "WIP limit: review already has {}/{} (--force to override)",
                        count_in("review"),
                        wip.review
                    ));
                }
            }
            // Definition of done is the gate: all criteria checked before done.
            if col == "done" && !has(args, "--force") && t.criteria.iter().any(|(_, d)| !d) {
                return Err(format!(
                    "{} has unchecked criteria ({}/{}) — check them (tasks crit check) or --force",
                    t.id,
                    t.criteria_done(),
                    t.criteria.len()
                ));
            }
            // Verification gate: a task worked in an isolated worktree must have
            // produced real changes before it can be done. Checked criteria are
            // self-certified and were being ticked without implementing the fix;
            // an empty worktree is the fingerprint of that false-done. Genuine
            // no-code tasks (investigation, triage) pass --allow-empty.
            if col == "done"
                && !has(args, "--force")
                && !has(args, "--allow-empty")
                && worktree::has_worktree(&t.id)
                && !worktree::changed(&t.id)?
            {
                return Err(format!(
                    "{0}: worktrees/{0} has no changes — the fix was not implemented (criteria were checked but nothing was built). Edit the files in the worktree, or pass --allow-empty if this task genuinely needs no code change.",
                    t.id
                ));
            }
            if col == "done" && !has(args, "--force") {
                run_cargo_done_checks(&t)?;
            }
            let note = if col == "doing" {
                worktree::ensure_for_doing(&t.id)?
            } else if col == "done" {
                worktree::finish_done(&t.id)?
            } else {
                String::new()
            };
            t.col = col.to_string();
            if col == "doing" {
                t.owner = actor.clone();
            } else if t.col != "review" {
                t.owner.clear();
            }
            t.trans.push((col.to_string(), now()));
            if col == "done" {
                t.done_ts = now();
                t.blocked.clear();
            }
            store.put(&t)?;
            match col.as_str() {
                "doing" => notify_task_lifecycle("started work", &t, &actor, None),
                "review" => notify_task_lifecycle("handed off for review", &t, &actor, None),
                "done" => notify_task_lifecycle("completed", &t, &actor, None),
                _ => {}
            }
            Ok(format!("{} → {}{}", t.id, col, note))
        }
        Some("done") => {
            // Sugar for `move <id> done` (same gates).
            let id = args.get(1).ok_or("usage: tasks done T-1")?.clone();
            let mut fwd = vec!["move".to_string(), id, "done".to_string()];
            if has(args, "--force") {
                fwd.push("--force".into());
            }
            if has(args, "--allow-empty") {
                fwd.push("--allow-empty".into());
            }
            if let Some(db) = flag(args, "--db") {
                fwd.push("--db".into());
                fwd.push(db);
            }
            if let Some(p) = flag(args, "--project") {
                fwd.push("--project".into());
                fwd.push(p);
            }
            run(&fwd)
        }
        Some("next") => {
            // Pull-based: only hand out work when there is doing-capacity.
            let ts = store.all()?;
            let wip = store.wip();
            let doing: Vec<&Task> = ts.iter().filter(|t| t.col == "doing").collect();
            if doing.len() >= wip.doing {
                return Ok(format!(
                    "WIP full ({}/{}): finish {} first (stop starting, start finishing)",
                    doing.len(),
                    wip.doing,
                    doing
                        .iter()
                        .map(|t| t.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            let mut ready: Vec<&Task> = ts
                .iter()
                .filter(|t| t.col == "ready" && t.blocked.is_empty() && !is_fleet_off_limits(t))
                .collect();
            ready.sort_by(|a, b| a.prio.cmp(&b.prio).then(a.created.cmp(&b.created)));
            match ready.first() {
                Some(t) => Ok(format!(
                    "pull {}: [{}] {} — move it with: tasks move {} doing",
                    t.id, t.prio, t.title, t.id
                )),
                None => Ok("no ready tasks — triage the backlog (tasks list --col backlog)".into()),
            }
        }
        Some("crit") => {
            let sub = args
                .get(1)
                .map(String::as_str)
                .ok_or("usage: tasks crit add|check|uncheck T-1 …")?;
            let id = args.get(2).ok_or("task id required")?;
            let mut t = store.get(id)?;
            let out = match sub {
                "add" => {
                    let text = args.get(3).ok_or("criterion text required")?;
                    t.criteria.push((text.clone(), false));
                    format!("{}: criterion {} added", t.id, t.criteria.len())
                }
                "check" | "uncheck" => {
                    let n: usize = args
                        .get(3)
                        .ok_or("criterion number required")?
                        .parse()
                        .map_err(|_| "bad number")?;
                    let item = t
                        .criteria
                        .get_mut(n.wrapping_sub(1))
                        .ok_or_else(|| format!("no criterion {n}"))?;
                    item.1 = sub == "check";
                    format!(
                        "{}: criterion {} {} ({}/{}✓)",
                        t.id,
                        n,
                        if sub == "check" { "✓" } else { "unchecked" },
                        t.criteria_done(),
                        t.criteria.len()
                    )
                }
                _ => return Err("crit sub-command must be add|check|uncheck".into()),
            };
            store.put(&t)?;
            Ok(out)
        }
        Some("block") => {
            let id = args.get(1).ok_or("usage: tasks block T-1 'reason'")?;
            let reason = args.get(2).ok_or("a block needs an explicit reason")?;
            let mut t = store.get(id)?;
            t.blocked = reason.clone();
            store.put(&t)?;
            notify_task_lifecycle("blocked", &t, &current_owner(), Some(reason));
            Ok(format!("{} blocked: {reason}", t.id))
        }
        Some("unblock") => {
            let id = args.get(1).ok_or("usage: tasks unblock T-1")?;
            let mut t = store.get(id)?;
            t.blocked.clear();
            store.put(&t)?;
            notify_task_lifecycle("unblocked", &t, &current_owner(), None);
            Ok(format!("{} unblocked", t.id))
        }
        Some("rm") => {
            let id = args.get(1).ok_or("usage: tasks rm T-1")?;
            if store.remove(id)? {
                Ok(format!("{id} removed"))
            } else {
                Err(format!("no task '{id}'"))
            }
        }
        Some("worktrees") => match args.get(1).map(String::as_str) {
            Some("reap") => {
                let age = flag(args, "--age-secs")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(86_400);
                worktree::reap_abandoned(age)
            }
            _ => Err("usage: tasks worktrees reap [--age-secs N]".into()),
        },
        Some("events") => {
            let n = flag(args, "--limit")
                .and_then(|s| s.parse().ok())
                .unwrap_or(20);
            Ok(render_task_events(n))
        }
        Some("wip") => {
            let mut w = store.wip();
            if let Some(d) = flag(args, "--doing").and_then(|s| s.parse().ok()) {
                w.doing = d;
            }
            if let Some(r) = flag(args, "--review").and_then(|s| s.parse().ok()) {
                w.review = r;
            }
            if w.doing == 0 {
                return Err("doing WIP must be ≥1".into());
            }
            store.set_wip(w)?;
            Ok(format!("wip limits: doing {} review {}", w.doing, w.review))
        }
        Some("metrics") => Ok(board::metrics(&store.all()?, now())),
        Some("statusline") => board::statusline(&store),
        _ => Ok(HELP.trim().into()),
    }
}

const HELP: &str = r#"
tasks — kanban board (semdb-backed, policies enforced in Rust)

USAGE:
  tasks add '<title>' [--prio p1|p2|p3] [--col backlog|ready] [--tags a,b] [--criteria 'a;b;c']
  tasks todo '<title>'                quick capture → backlog p3
  tasks board                         render columns + WIP state
  tasks list [--col C] [--tag T] [--owner O|--mine|--others] [--blocked]
  tasks show T-1                      card: owner + criteria checklist + history
  tasks move T-1 <column>             WIP-limited; done requires criteria ✓ and matching cargo checks
  tasks done T-1                      = move done
  tasks next                          pull highest-prio ready task IF doing has capacity
  tasks crit add T-1 '<text>' | check T-1 <n> | uncheck T-1 <n>
  tasks block T-1 '<reason>' | unblock T-1
  tasks events [--limit N]             recent task claim/handoff/spawn events
  tasks wip [--doing N] [--review N]  set WIP limits (default doing 1, review 3)
  tasks metrics                       throughput, cycle time, lead time
  tasks rm T-1
  tasks worktrees reap [--age-secs N]   remove abandoned task worktrees

Columns: backlog → ready → doing → review → done. Default db: data/tasks.semdb.
Every verb accepts --project <name>: that workspace repo's OWN board at
workspaces/<name>/.smartagent/tasks.semdb (boards never mix between repos).
Kanban rules live in skills/Kanban (triage, pull, review, retro workflows).
"#;

#[cfg(test)]
mod tests {
    use super::{task_created_notification_text, task_lifecycle_notification};
    use crate::store::Task;

    #[test]
    fn task_lifecycle_notification_is_structured_and_agent_visible() {
        let t = Task {
            id: "T-189".into(),
            title: "Make agents communicate more proactively in Telegram bot".into(),
            col: "doing".into(),
            prio: "p1".into(),
            tags: vec!["telegram".into(), "agents".into()],
            criteria: vec![("structured".into(), true), ("not spammy".into(), false)],
            ..Task::default()
        };
        let text =
            task_lifecycle_notification("started work", &t, "woz", Some("pulled from ready"));
        assert!(text.contains("🤖 *Agent task update:* started work"));
        assert!(text.contains("*Agent:* `woz`"));
        assert!(text.contains("*Task:* `T-189`"));
        assert!(text.contains("*Priority:* `p1` · *State:* `doing`"));
        assert!(text.contains("*Tags:* `telegram,agents`"));
        assert!(text.contains("*Criteria:* `1/2`"));
        assert!(text.contains("*Note:* pulled from ready"));
        assert!(text.contains("*Next:* agents continue from the board"));
        assert!(text.lines().count() <= 8);
    }

    #[test]
    fn task_created_notification_is_styled_markdown() {
        let t = Task {
            id: "T-999".into(),
            title: "demo task".into(),
            col: "ready".into(),
            prio: "p2".into(),
            tags: vec!["telegram".into(), "polish".into()],
            criteria: vec![("one".into(), false), ("two".into(), false)],
            ..Task::default()
        };
        let msg = task_created_notification_text(&t);
        assert!(msg.starts_with("🗂 *New SMARTAGENT task*"), "{msg}");
        assert!(msg.contains("*ID:* `T-999`"), "{msg}");
        assert!(msg.contains("*Priority:* `p2`"), "{msg}");
        assert!(msg.contains("*State:* `ready`"), "{msg}");
        assert!(msg.contains("*Tags:* telegram, polish"), "{msg}");
        assert!(msg.contains("*Criteria:* 2 item(s)"), "{msg}");
    }
}
