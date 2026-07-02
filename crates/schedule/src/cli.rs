//! CLI: add / list / rm / next / run [--once]

use httpc::args::flag;
use std::path::PathBuf;

use crate::cron::{Civil, Cron};
use crate::journal::{Event, Journal};
use crate::runner;

/// Build a fixed, safe shell command for a reminder: invoke the notify binary
/// with the message single-quote-escaped so it cannot break out into other
/// shell commands. This is the only job form the agent can create unprivileged.
fn build_notify_cmd(msg: &str) -> String {
    let topic = semdb::config::Config::load()
        .resolve("ntfy_topic", "NTFY_TOPIC", None)
        .unwrap_or_else(|| "smartagent".into());
    let esc = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    format!(
        "target/release/notify send --topic {} --message {}",
        esc(&topic),
        esc(msg)
    )
}

/// Turn `YYYY-MM-DDTHH:MM` (or `YYYY-MM-DD HH:MM`) into a cron expression
/// `MIN HOUR DAY MONTH *` for a one-shot job. The job is marked `once` so the
/// runner removes it after the single fire.
fn cron_from_at(at: &str) -> Result<String, String> {
    let (date, time) = at.split_once(['T', ' ']).ok_or("--at must be YYYY-MM-DDTHH:MM")?;
    let d: Vec<&str> = date.split('-').collect();
    let t: Vec<&str> = time.split(':').collect();
    if d.len() != 3 || t.len() < 2 {
        return Err("--at must be YYYY-MM-DDTHH:MM".into());
    }
    let month: u32 = d[1].parse().map_err(|_| "bad month")?;
    let day: u32 = d[2].parse().map_err(|_| "bad day")?;
    let hour: u32 = t[0].parse().map_err(|_| "bad hour")?;
    let min: u32 = t[1].parse().map_err(|_| "bad minute")?;
    if month == 0 || month > 12 || day == 0 || day > 31 || hour > 23 || min > 59 {
        return Err("--at has an out-of-range field".into());
    }
    Ok(format!("{min} {hour} {day} {month} *"))
}

fn journal_path(args: &[String]) -> PathBuf {
    flag(args, "--journal")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/schedule.jsonl"))
}

pub fn run(args: &[String]) -> Result<String, String> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let jpath = journal_path(args);
    if let Some(parent) = jpath.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let journal = Journal::new(&jpath);

    match cmd {
        "add" => {
            // --at 'YYYY-MM-DDTHH:MM' schedules a one-shot at that local time
            // (self-removes after firing); otherwise --cron for a recurring job.
            let (cron_expr, once) = match flag(args, "--at") {
                Some(at) => (cron_from_at(&at)?, true),
                None => (flag(args, "--cron").ok_or("--cron or --at required")?, false),
            };
            // A scheduled job runs a shell command on a recurring timer — the
            // highest-value persistence primitive for a prompt-injected agent.
            // The safe, agent-facing path is `--notify <msg>`, which can only
            // fire a notification. Arbitrary `--cmd` is admin-only (out-of-band
            // SMARTAGENT_SCHEDULE_ADMIN=1), so injection can't plant a backdoor.
            let cmd_str = match flag(args, "--notify") {
                Some(msg) => build_notify_cmd(&msg),
                None => {
                    let raw = flag(args, "--cmd").ok_or("--cmd or --notify required")?;
                    if std::env::var("SMARTAGENT_SCHEDULE_ADMIN").as_deref() != Ok("1") {
                        return Err("arbitrary --cmd is admin-only (set SMARTAGENT_SCHEDULE_ADMIN=1); use --notify <message> for reminders".into());
                    }
                    raw
                }
            };
            Cron::parse(&cron_expr)?; // validate before journaling
            let id = flag(args, "--id").unwrap_or_else(|| {
                // Deterministic id from content + count.
                let n = journal.replay().map(|j| j.len()).unwrap_or(0);
                format!("job{}", n + 1)
            });
            journal.append(&Event::Scheduled { id: id.clone(), cron: cron_expr, cmd: cmd_str, once })?;
            Ok(format!("added {id}{}", if once { " (one-shot)" } else { "" }))
        }
        "pause" | "resume" => {
            let id = flag(args, "--id").or_else(|| args.get(1).cloned()).ok_or("--id required")?;
            if !journal.replay()?.contains_key(&id) {
                return Err(format!("no job '{id}'"));
            }
            journal.append(&Event::SetEnabled { id: id.clone(), enabled: cmd == "resume" })?;
            Ok(format!("{}d {id}", cmd))
        }
        "rm" => {
            let id = flag(args, "--id").or_else(|| args.get(1).cloned()).ok_or("--id required")?;
            if !journal.replay()?.contains_key(&id) {
                return Err(format!("no job '{id}'"));
            }
            journal.append(&Event::Removed { id: id.clone() })?;
            Ok(format!("removed {id}"))
        }
        "list" => {
            let jobs = journal.replay()?;
            if jobs.is_empty() {
                return Ok("no jobs".into());
            }
            let mut out: Vec<String> = jobs
                .values()
                .map(|j| {
                    format!(
                        "{}\t{}\t{}\tlast: {}",
                        j.id,
                        j.cron,
                        j.cmd,
                        j.last_fire.map_or("never".into(), fmt_time)
                    )
                })
                .collect();
            out.sort();
            Ok(out.join("\n"))
        }
        "next" => {
            let jobs = journal.replay()?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let mut out = Vec::new();
            for j in jobs.values() {
                let cron = Cron::parse(&j.cron)?;
                let next = cron.next_after(now).map_or("never".into(), fmt_time);
                out.push(format!("{}\t{}", j.id, next));
            }
            out.sort();
            Ok(out.join("\n"))
        }
        "run" => {
            if args.iter().any(|a| a == "--once") {
                let results = runner::tick(&journal)?;
                if results.is_empty() {
                    return Ok("nothing due".into());
                }
                Ok(results
                    .into_iter()
                    .map(|(id, exit)| format!("ran {id} exit {exit}"))
                    .collect::<Vec<_>>()
                    .join("\n"))
            } else {
                eprintln!("[schedule] daemon started, journal {}", jpath.display());
                runner::daemon(&journal)?;
                Ok(String::new())
            }
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn fmt_time(t: i64) -> String {
    let c = Civil::from_unix(t);
    format!("{:04}-{:02}-{:02} {:02}:{:02}", c.year, c.month, c.day, c.hour, c.minute)
}


const HELP: &str = r#"
schedule — durable cron scheduler (journal + replay)

USAGE:
  schedule add  --cron '*/5 * * * *' --cmd '<shell>' [--id X] [--journal FILE]
  schedule list [--journal FILE]
  schedule next [--journal FILE]
  schedule rm   --id X [--journal FILE]
  schedule run [--once] [--journal FILE]

Journal default: data/schedule.jsonl (append-only JSONL, replayed on start).
"#;
