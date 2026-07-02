//! Heartbeat (meðvitund pulse): every `heartbeat_secs` the gateway composes a
//! beat from LOCAL state (no model call) — wall-clock time, session age, the
//! board's doing/ready lines, the active workflow — and delivers it:
//! busy agent → `steer` (lands between turns); idle agent → queued at zero
//! token cost and prefixed to the next message (or, with --autonomous, sent
//! as a prompt telling the agent to continue the plan). Every beat and every
//! turn-end is appended to the `medvitund` semdb table — the agent's
//! interviewable self-history.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use httpc::json;
use semdb::storage::Db;

use crate::child::Usage;

const PLACEHOLDER_VEC: [f32; 1] = [0.0];

pub struct Beat {
    repo_root: PathBuf,
    db_path: PathBuf,
    started: Instant,
    pub last_beat: Option<String>,
}

impl Beat {
    pub fn new(repo_root: &Path, data_dir: &Path) -> Beat {
        Beat {
            repo_root: repo_root.to_path_buf(),
            db_path: data_dir.join("medvitund.semdb"),
            started: Instant::now(),
            last_beat: None,
        }
    }

    /// Compose the beat text from local state. Cheap, deterministic, no LLM.
    pub fn compose(&mut self, busy: bool) -> String {
        let now = human_now();
        let up = human_dur(self.started.elapsed());
        let board = run_local(&self.repo_root, "tasks", &["board", "--dir", "."], 5)
            .map(|out| summarize_board(&out))
            .unwrap_or_else(|| "board unavailable".into());
        let wf = run_local(&self.repo_root, "workflow", &["runs", "--dir", "."], 5)
            .map(|out| first_active_line(&out))
            .unwrap_or_default();
        let state = if busy { "working" } else { "idle" };
        let beat = format!(
            "⏲ heartbeat {now} | session up {up} | you are {state}\n{board}{wf}\nYou are a persistent agent with continuity across the day. Stay aware of the time and how long you have been on the current task. RULES: (1) one agent does ONE thing at a time — at most one task in doing; finish it (criteria checked, moved to done) before pulling the next single ready task; park extras back to ready. (2) Route every task through `skills match` first and load the winning skill; use the `triage` skill for ready-empty/backlog ordering. (3) Non-trivial tasks (≥2 steps or ≥2 files) run through the workflow engine (`workflow start task-run --task T-n`), advancing only with real evidence. (4) Utilize your sub-agents: independent parallelizable work goes to `orchestrate`; do not serialize what sub-agents can do concurrently. (5) Handoff discipline: when a task needs review or another role, move it to the review column with a note instead of silently finishing — the board IS the handoff protocol. (6) Other agents work this board too: continue only doing tasks owned by you (@owner in the board); never take doing/review tasks owned by someone else; pull only unclaimed ready/backlog items. (7) NEVER pull or continue any task tagged `desktop-agent` or that edits `desktop-agent/` — that crate is a separate session's WIP and is off-limits to this fleet; skip such tasks in triage even if they are the highest priority."
        );
        self.last_beat = Some(now);
        beat
    }

    /// True when an autonomous idle beat should spend a model turn.
    ///
    /// Work exists when the root board has anything in DOING (resume), READY
    /// (pull), or BACKLOG (triage/promote). REVIEW is a handoff queue and does
    /// not justify prompting an idle agent unless addressed out-of-band.
    pub fn has_autonomous_work(&self) -> bool {
        run_local(&self.repo_root, "tasks", &["board", "--dir", "."], 5)
            .map(|out| board_has_autonomous_work(&out))
            .unwrap_or(true)
    }

    /// First task in the DOING column, shortened — "what am I on right now".
    pub fn doing_short(&self) -> String {
        run_local(&self.repo_root, "tasks", &["board", "--dir", "."], 5)
            .and_then(|out| {
                let mut in_doing = false;
                for l in out.lines() {
                    if l.starts_with("DOING") {
                        in_doing = true;
                        continue;
                    }
                    if in_doing {
                        if !l.starts_with(' ') {
                            break;
                        }
                        return Some(l.trim().chars().take(40).collect());
                    }
                }
                None
            })
            .unwrap_or_else(|| "nothing".into())
    }

    /// Append a row to the medvitund table. Row without meaning-vector (v1).
    pub fn log(&self, agent: &str, kind: &str, state: &str, text: &str) {
        self.log_with_usage(agent, kind, state, text, None);
    }

    pub fn log_turn(&self, agent: &str, text: &str, usage: Usage) {
        self.log_with_usage(agent, "turn", "idle", text, Some(usage));
    }

    fn log_with_usage(
        &self,
        agent: &str,
        kind: &str,
        state: &str,
        text: &str,
        usage: Option<Usage>,
    ) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let excerpt: String = text.chars().take(400).collect();
        let tokens = usage
            .map(|u| {
                format!(
                    ",\"input\":{},\"output\":{},\"cacheRead\":{},\"cacheWrite\":{}",
                    u.input, u.output, u.cache_read, u.cache_write
                )
            })
            .unwrap_or_default();
        let day = human_day_prefix();
        let meta = format!(
            "{{\"ts\":{ts},\"day\":\"{day}\",\"agent\":\"{}\",\"kind\":\"{}\",\"state\":\"{}\",\"text\":\"{}\"{tokens}}}",
            json::escape(agent),
            json::escape(kind),
            json::escape(state),
            json::escape(&excerpt)
        );
        let id = format!("mv-{ts}");
        let db = if self.db_path.exists() {
            Db::open(&self.db_path)
        } else {
            Db::create(&self.db_path)
        };
        if let Ok(mut db) = db {
            let _ = db.put(&id, &meta, PLACEHOLDER_VEC.to_vec());
        }
    }
}

/// Run a workspace tool binary with a hard deadline; None on any failure.
fn run_local(repo_root: &Path, bin: &str, args: &[&str], deadline_secs: u64) -> Option<String> {
    let path = repo_root.join("target/release").join(bin);
    let mut child = std::process::Command::new(path)
        .args(args)
        .current_dir(repo_root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(deadline_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                use std::io::Read;
                child.stdout.take()?.read_to_string(&mut out).ok()?;
                return if status.success() { Some(out) } else { None };
            }
            Ok(None) if Instant::now() > deadline => {
                let _ = child.kill();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return None,
        }
    }
}

/// Keep only the DOING and READY sections, indented, max 8 lines.
fn summarize_board(board: &str) -> String {
    let mut out = String::new();
    let mut keep = false;
    let mut lines = 0;
    for l in board.lines() {
        if l.starts_with("DOING") || l.starts_with("READY") {
            keep = true;
        } else if !l.starts_with(' ') {
            keep = false;
        }
        if keep && lines < 8 {
            out.push_str(l);
            out.push('\n');
            lines += 1;
        }
    }
    if out.is_empty() {
        "board: nothing in doing/ready\n".into()
    } else {
        out
    }
}

fn first_active_line(runs: &str) -> String {
    runs.lines()
        .find(|l| l.contains("step") || l.contains("running"))
        .map(|l| format!("{l}\n"))
        .unwrap_or_default()
}

fn board_has_autonomous_work(board: &str) -> bool {
    board.lines().any(|l| {
        (l.starts_with("DOING") || l.starts_with("READY") || l.starts_with("BACKLOG"))
            && !(l.contains("(0)") || l.contains("(0/"))
    })
}

pub fn human_day_prefix() -> String {
    human_now().chars().take(10).collect()
}

fn human_now() -> String {
    // UTC wall clock from the epoch, no external crates.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let (h, m, s) = ((secs % 86400) / 3600, (secs % 3600) / 60, secs % 60);
    // civil date from days-since-epoch (Howard Hinnant's algorithm)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}Z")
}

fn human_dur(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m{}s", s / 60, s % 60)
    } else {
        format!("{}h{}m", s / 3600, (s % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_summary_keeps_doing_ready_only() {
        let b = "BACKLOG (2)\n  T-1 x\nDOING (1/3)\n  T-2 y\nREVIEW (0)\nREADY (1)\n  T-3 z\n";
        let s = summarize_board(b);
        assert!(s.contains("DOING"));
        assert!(s.contains("T-2 y"));
        assert!(s.contains("READY"));
        assert!(!s.contains("BACKLOG"));
        assert!(!s.contains("T-1 x"));
    }

    #[test]
    fn civil_date_sane() {
        let now = human_now();
        assert!(now.starts_with("20"), "got {now}");
        assert!(now.ends_with('Z'));
    }

    #[test]
    fn durations_humanize() {
        assert_eq!(human_dur(Duration::from_secs(45)), "45s");
        assert_eq!(human_dur(Duration::from_secs(125)), "2m5s");
        assert_eq!(human_dur(Duration::from_secs(7300)), "2h1m");
    }

    #[test]
    fn autonomous_work_ignores_empty_board_and_review() {
        let empty = "BACKLOG (0)\nREADY (0)\nDOING (0/4)\nREVIEW (1/3)\n  T-1 handoff\n";
        assert!(!board_has_autonomous_work(empty));
    }

    #[test]
    fn autonomous_work_sees_backlog_ready_or_doing() {
        assert!(board_has_autonomous_work(
            "BACKLOG (1)\n  T-1 x\nREADY (0)\nDOING (0/4)\n"
        ));
        assert!(board_has_autonomous_work(
            "BACKLOG (0)\nREADY (1)\n  T-2 y\nDOING (0/4)\n"
        ));
        assert!(board_has_autonomous_work(
            "BACKLOG (0)\nREADY (0)\nDOING (1/4)\n  T-3 z\n"
        ));
    }
}
