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

use httpc::json::{self, Value};
use semdb::storage::Db;

use crate::child::Usage;

const PLACEHOLDER_VEC: [f32; 1] = [0.0];
const MEDVITUND_KEEP_ROWS: usize = 5_000;
const MEDVITUND_COMPACT_EVERY_WRITES: u64 = 100;

pub struct Beat {
    repo_root: PathBuf,
    db_path: PathBuf,
    db: Option<Db>,
    writes_since_prune: u64,
    started: Instant,
    pub last_beat: Option<String>,
}

impl Beat {
    pub fn new(repo_root: &Path, data_dir: &Path) -> Beat {
        let db_path = data_dir.join("medvitund.semdb");
        let db = open_or_create_medvitund(&db_path).ok();
        Beat {
            repo_root: repo_root.to_path_buf(),
            db_path,
            db,
            writes_since_prune: 0,
            started: Instant::now(),
            last_beat: None,
        }
    }

    /// Compose the beat and derive autonomous work from the same board output.
    /// This avoids a second `tasks board` subprocess in the heartbeat hot path.
    pub fn compose_with_work(&mut self, busy: bool, agent: &str, role: &str) -> (String, bool) {
        let now = human_now();
        let up = human_dur(self.started.elapsed());
        let board_raw = run_local(&self.repo_root, "tasks", &["board", "--dir", "."], 5);
        let has_work = board_raw
            .as_deref()
            .map(|out| board_has_autonomous_work_for(out, agent, role))
            .unwrap_or(true);
        let board = board_raw
            .as_deref()
            .map(summarize_board)
            .unwrap_or_else(|| "board unavailable".into());
        let wf = run_local(&self.repo_root, "workflow", &["runs", "--dir", "."], 5)
            .map(|out| first_active_line(&out))
            .unwrap_or_default();
        let beat = format_beat(now.clone(), up, busy, &board, &wf);
        self.last_beat = Some(now);
        (beat, has_work)
    }

    /// First task in the DOING column, shortened — "what am I on right now".
    pub fn doing_short(&self) -> String {
        run_local(&self.repo_root, "tasks", &["board", "--dir", "."], 5)
            .and_then(|out| first_doing_for(&out, None))
            .unwrap_or_else(|| "nothing".into())
    }

    /// This agent's own DOING task, shortened. Idle agents must not inherit
    /// some other agent's board slot in the team panel.
    pub fn doing_short_for(&self, owner: &str) -> String {
        run_local(&self.repo_root, "tasks", &["board", "--dir", "."], 5)
            .and_then(|out| first_doing_for(&out, Some(owner)))
            .unwrap_or_else(|| "nothing".into())
    }

    /// Append a row to the medvitund table. Row without meaning-vector (v1).
    pub fn log(&mut self, agent: &str, kind: &str, state: &str, text: &str) {
        self.log_with_usage(agent, kind, state, text, None);
    }

    pub fn log_turn(&mut self, agent: &str, text: &str, usage: Usage) {
        self.log_with_usage(agent, "turn", "idle", text, Some(usage));
    }

    fn log_with_usage(
        &mut self,
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
        if self.db.is_none() {
            self.db = open_or_create_medvitund(&self.db_path).ok();
        }
        if let Some(db) = self.db.as_mut() {
            if db.put(&id, &meta, PLACEHOLDER_VEC.to_vec()).is_ok() {
                self.writes_since_prune += 1;
                if self.writes_since_prune >= MEDVITUND_COMPACT_EVERY_WRITES
                    || db.index.len() > MEDVITUND_KEEP_ROWS
                {
                    prune_medvitund(db, MEDVITUND_KEEP_ROWS);
                    self.writes_since_prune = 0;
                }
            }
        }
    }

    pub fn tokens_today(&self, agent: Option<&str>) -> crate::agents_view::TokenTotals {
        let Some(db) = self.db.as_ref() else {
            return crate::agents_view::TokenTotals::default();
        };
        tokens_today_in_db(db, agent)
    }

    pub fn medvitund_records(&self) -> (usize, u64) {
        self.db
            .as_ref()
            .map(|db| (db.index.len(), db.records))
            .unwrap_or((0, 0))
    }
}

fn open_or_create_medvitund(path: &Path) -> Result<Db, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if path.exists() {
        Db::open(path)
    } else {
        Db::create(path)
    }
}

fn prune_medvitund(db: &mut Db, keep_rows: usize) {
    if db.index.len() <= keep_rows {
        return;
    }
    let mut rows = db
        .index
        .iter()
        .map(|(id, entry)| (medvitund_ts(id, &entry.meta), id.clone()))
        .collect::<Vec<_>>();
    rows.sort_by_key(|(ts, _)| *ts);
    let remove = rows.len().saturating_sub(keep_rows);
    for (_, id) in rows.into_iter().take(remove) {
        let _ = db.delete(&id);
    }
    let _ = db.compact();
}

fn medvitund_ts(id: &str, meta: &str) -> u128 {
    json::parse(meta)
        .ok()
        .and_then(|v| v.get("ts").and_then(Value::as_f64).map(|n| n.max(0.0) as u128))
        .or_else(|| id.strip_prefix("mv-").and_then(|s| s.parse::<u128>().ok()))
        .unwrap_or(0)
}

pub(crate) fn tokens_today_in_db(db: &Db, agent: Option<&str>) -> crate::agents_view::TokenTotals {
    let today = human_day_prefix();
    let mut out = crate::agents_view::TokenTotals::default();
    for entry in db.index.values() {
        let Ok(v) = json::parse(&entry.meta) else {
            continue;
        };
        if v.get("kind").and_then(Value::as_str) != Some("turn") {
            continue;
        }
        if let Some(a) = agent {
            if v.get("agent").and_then(Value::as_str) != Some(a) {
                continue;
            }
        }
        if v.get("day").and_then(Value::as_str) != Some(today.as_str()) {
            continue;
        }
        out.input += meta_u64(&v, "input");
        out.output += meta_u64(&v, "output");
        out.cache_read += meta_u64(&v, "cacheRead");
        out.cache_write += meta_u64(&v, "cacheWrite");
    }
    out
}

fn meta_u64(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as u64
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

fn first_doing_for(board: &str, owner: Option<&str>) -> Option<String> {
    let mut in_doing = false;
    for l in board.lines() {
        if l.starts_with("DOING") {
            in_doing = true;
            continue;
        }
        if in_doing {
            if !l.starts_with(' ') {
                break;
            }
            let trimmed = l.trim();
            if let Some(owner) = owner {
                let marker = format!("@{owner}");
                if !trimmed.split_whitespace().any(|part| part == marker) {
                    continue;
                }
            }
            return Some(trimmed.chars().take(64).collect());
        }
    }
    None
}

fn first_active_line(runs: &str) -> String {
    runs.lines()
        .find(|l| l.contains("step") || l.contains("running"))
        .map(|l| format!("{l}\n"))
        .unwrap_or_default()
}

fn board_has_autonomous_work_for(board: &str, agent: &str, role: &str) -> bool {
    let mine = format!("@{agent}");
    let role_marker = format!("@{role}");
    let mut section = "";
    for l in board.lines() {
        if l.starts_with("DOING") {
            section = "doing";
            continue;
        }
        if l.starts_with("READY") {
            section = "ready";
            continue;
        }
        if l.starts_with("REVIEW") {
            section = "review";
            continue;
        }
        if l.starts_with("BACKLOG") {
            section = "backlog";
            continue;
        }
        if !l.starts_with(' ') {
            section = "";
            continue;
        }
        let row = l.trim();
        if row.is_empty() {
            continue;
        }
        match section {
            // A doing task owned by ANOTHER agent is not work I can pull
            // (one-per-agent WIP) — treating it as "work available" woke every
            // idle agent each cycle to burn a no-op turn concluding "owned by
            // others". Count only my own in-progress task (continue it) or an
            // unowned/orphaned doing row (adoptable).
            "doing" => {
                let owned_by_other = row
                    .split_whitespace()
                    .filter_map(|t| t.strip_prefix('@'))
                    .any(|o| o != agent);
                if !owned_by_other {
                    return true;
                }
            }
            "backlog" => return true,
            "ready" => {
                // Auto-queue after unblock: heartbeat/work-chaser should only
                // spend a turn for READY rows that `tasks next` can actually
                // pull. Blocked rows remain visible on the board but are not
                // pullable until `tasks unblock` clears the ⛔ reason; the next
                // gateway poll then sees this row and prompts an idle agent.
                if !row.contains('⛔') && !row.contains("desktop-agent") {
                    return true;
                }
            }
            "review" => {
                if row.contains(&mine) || (!role.is_empty() && row.contains(&role_marker)) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

pub fn human_day_prefix() -> String {
    human_now().chars().take(10).collect()
}


fn format_beat(now: String, up: String, busy: bool, board: &str, wf: &str) -> String {
    let state = if busy { "working" } else { "idle" };
    format!(
        "⏲ heartbeat {now} | session up {up} | you are {state}\n{board}{wf}\nYou are a persistent agent with continuity across the day. Stay aware of the time and how long you have been on the current task. RULES: (1) one agent does ONE thing at a time — at most one task in doing; finish it (criteria checked, moved to done) before pulling the next single ready task; park extras back to ready. (2) Route every task through `skills match` first and load the winning skill; use the `triage` skill for ready-empty/backlog ordering. (3) Non-trivial tasks (≥2 steps or ≥2 files) run through the workflow engine (`workflow start task-run --task T-n`), advancing only with real evidence. (4) Utilize your sub-agents: independent parallelizable work goes to `orchestrate`; do not serialize what sub-agents can do concurrently. (5) Handoff discipline: when a task needs review or another role, move it to the review column with a note instead of silently finishing — the board IS the handoff protocol. (6) Other agents work this board too: first sweep review for items assigned to you/your role and verify them; otherwise continue only doing tasks owned by you (@owner in the board), never take doing/review tasks owned by someone else, and pull only unclaimed ready/backlog items. (7) NEVER pull or continue any task tagged `desktop-agent` or that edits `desktop-agent/` — that crate is a separate session's WIP and is off-limits to this fleet; skip such tasks in triage even if they are the highest priority."
    )
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


    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch")
            .join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn medvitund_logs_reuse_held_db_and_tokens_read_from_handle() {
        let d = scratch("gateway-medvitund-held-db");
        let mut beat = Beat::new(&d, &d);
        beat.log_turn(
            "ada",
            "turn complete",
            Usage {
                input: 1,
                output: 2,
                cache_read: 3,
                cache_write: 4,
            },
        );
        let before = beat.medvitund_records();
        std::fs::remove_file(d.join("medvitund.semdb")).unwrap();
        beat.log_turn(
            "ada",
            "second turn still uses held handle",
            Usage {
                input: 10,
                output: 20,
                cache_read: 30,
                cache_write: 40,
            },
        );
        let after = beat.medvitund_records();
        assert_eq!(before.0 + 1, after.0);
        assert_eq!(beat.tokens_today(Some("ada")).total(), 110);
    }

    #[test]
    fn medvitund_prune_keeps_latest_rows_and_compacts() {
        let d = scratch("gateway-medvitund-prune");
        let mut db = Db::create(&d.join("medvitund.semdb")).unwrap();
        for i in 0..12 {
            let meta = format!(
                r#"{{"ts":{},"day":"{}","agent":"ada","kind":"beat","state":"idle","text":"row"}}"#,
                i,
                human_day_prefix()
            );
            db.put(&format!("mv-{i}"), &meta, PLACEHOLDER_VEC.to_vec())
                .unwrap();
        }
        prune_medvitund(&mut db, 5);
        assert_eq!(db.index.len(), 5);
        assert!(db.get("mv-6").is_none());
        assert!(db.get("mv-7").is_some());
        assert!(db.records <= 5, "records after compact: {}", db.records);
    }

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
    fn doing_short_filters_by_owner() {
        let board =
            "DOING (2/8)\n  T-1 p2 Alpha @ada [0/1✓]\n  T-2 p2 Beta @turing [0/1✓]\nREADY (0)\n";
        assert_eq!(
            first_doing_for(board, None).unwrap(),
            "T-1 p2 Alpha @ada [0/1✓]"
        );
        assert_eq!(
            first_doing_for(board, Some("turing")).unwrap(),
            "T-2 p2 Beta @turing [0/1✓]"
        );
        assert!(first_doing_for(board, Some("grace")).is_none());
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
    fn autonomous_work_ignores_review_for_other_agents() {
        let other = "BACKLOG (0)\nREADY (0)\nDOING (0/4)\nREVIEW (1/3)\n  T-1 handoff @other\n";
        assert!(!board_has_autonomous_work_for(other, "grace", "QA"));
    }

    #[test]
    fn autonomous_work_sees_assigned_review() {
        let mine = "BACKLOG (0)\nREADY (0)\nDOING (0/4)\nREVIEW (1/3)\n  T-1 handoff @grace\n";
        assert!(board_has_autonomous_work_for(mine, "grace", "QA"));
        let role = "BACKLOG (0)\nREADY (0)\nDOING (0/4)\nREVIEW (1/3)\n  T-2 handoff @QA\n";
        assert!(board_has_autonomous_work_for(role, "grace", "QA"));
    }

    #[test]
    fn others_doing_task_does_not_wake_idle_agent() {
        // The no-op-turn bug: a task in doing owned by ANOTHER agent must NOT
        // count as work for an idle agent (it can't be pulled — one per agent).
        let others =
            "BACKLOG (0)\nREADY (0)\nDOING (2/8)\n  T-1 x @ada\n  T-2 y @dennis\nREVIEW (0/4)\n";
        assert!(!board_has_autonomous_work_for(others, "grace", "QA"));
        // ...but MY own doing task still counts (continue it).
        let mine = "BACKLOG (0)\nREADY (0)\nDOING (1/8)\n  T-3 z @grace\n";
        assert!(board_has_autonomous_work_for(mine, "grace", "QA"));
    }

    #[test]
    fn autonomous_work_sees_backlog_ready_or_doing() {
        assert!(board_has_autonomous_work_for(
            "BACKLOG (1)\n  T-1 x\nREADY (0)\nDOING (0/4)\n",
            "grace",
            "QA"
        ));
        assert!(board_has_autonomous_work_for(
            "BACKLOG (0)\nREADY (1)\n  T-2 y\nDOING (0/4)\n",
            "grace",
            "QA"
        ));
        assert!(board_has_autonomous_work_for(
            "BACKLOG (0)\nREADY (0)\nDOING (1/4)\n  T-3 z\n",
            "grace",
            "QA"
        ));
    }

    #[test]
    fn autonomous_work_ignores_blocked_ready_until_unblocked() {
        let blocked =
            "BACKLOG (0)\nREADY (1)\n  T-1 p2 Waiting ⛔external\nDOING (0/4)\nREVIEW (0/3)\n";
        assert!(!board_has_autonomous_work_for(blocked, "grace", "QA"));
        let unblocked = "BACKLOG (0)\nREADY (1)\n  T-1 p2 Waiting\nDOING (0/4)\nREVIEW (0/3)\n";
        assert!(board_has_autonomous_work_for(unblocked, "grace", "QA"));
    }

    #[test]
    fn autonomous_work_ignores_desktop_ready_rows() {
        let board =
            "BACKLOG (0)\nREADY (1)\n  T-40 p3 desktop-agent task\nDOING (0/4)\nREVIEW (0/3)\n";
        assert!(!board_has_autonomous_work_for(board, "grace", "QA"));
    }
}
