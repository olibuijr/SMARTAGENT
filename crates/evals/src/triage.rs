//! triage: eval failures → board tasks. The self-heal loop's ingestion point —
//! a failing run becomes ONE criteria-gated task the autonomous fleet can pull.
//!
//! Loop-safety guards (from the causal-loop analysis — structural, not advisory):
//! - DEDUPE: one open task per failing run, keyed by exact title prefix —
//!   re-running triage (every stop hook) never spams the board.
//! - TRACKED: a run named after an open board task (e.g. `T-87-…`) is that
//!   task's own eval — skip; the work is already owned.
//! - ESCALATE, don't retry: if a run's fix task was completed and the run
//!   fails AGAIN, create one p1 `eval-fix (recurring)` task instead of a
//!   fresh p2 — a recurring red is a design problem, not a retry candidate.
//! - NO-WEAKENING criterion on every generated task: the fix must land in
//!   code/config; editing the eval's expectations to go green fails review.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::score::{self, Matcher};
use crate::store;
use tasks::store::{now, Store, Task};

/// Extract an embedded board-task id (`T-<digits>`) from a run name.
fn embedded_task(run: &str) -> Option<String> {
    let b = run.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'T' && b[i + 1] == b'-' {
            let start = i + 2;
            let mut end = start;
            while end < b.len() && b[end].is_ascii_digit() {
                end += 1;
            }
            if end > start {
                return Some(format!("T-{}", &run[start..end]));
            }
        }
        i += 1;
    }
    None
}

/// Sweep the evals db; create board tasks for failing runs. Idempotent —
/// safe to fire from the stop hook after every agent session.
///
/// Incremental by default: only runs with traces logged since the last sweep
/// are considered (cursor row in the evals db), so append-only history never
/// causes a task burst. First-ever sweep initializes the cursor and skips
/// history. `sweep_all` (CLI `--all`) considers every run regardless.
///
/// Scoring is latest-per-case: a case's most recent trace wins, so re-logging
/// a passing trace after a fix flips the case (and eventually the run) green.
pub fn triage(
    evals_db: &Path,
    tasks_db: &Path,
    dry_run: bool,
    sweep_all: bool,
) -> Result<String, String> {
    // Kill switch: `touch <evals-db-dir>/eval-triage.off` disables the loop
    // instantly (a self-modifying loop needs an off-switch that doesn't wait
    // for a rebuild or service restart).
    let off = evals_db
        .parent()
        .unwrap_or(Path::new("."))
        .join("eval-triage.off");
    if off.exists() {
        return Ok(
            "evals triage: disabled by eval-triage.off — remove the file to re-enable".into(),
        );
    }
    // Sweep lock: concurrent stop hooks (two agents ending together) would race
    // the cursor read→create→set window and duplicate tasks. First writer wins;
    // losers skip — sweeps are frequent and idempotent, skipping is safe.
    // Stale locks (crashed sweep) expire after 120s.
    let lock = evals_db
        .parent()
        .unwrap_or(Path::new("."))
        .join(".eval-triage.lock");
    if let Ok(meta) = std::fs::metadata(&lock) {
        let fresh = meta
            .modified()
            .ok()
            .and_then(|m| m.elapsed().ok())
            .map(|e| e.as_secs() < 120)
            .unwrap_or(true);
        if fresh {
            return Ok("evals triage: another sweep in progress — skipped".into());
        }
        let _ = std::fs::remove_file(&lock);
    }
    if !dry_run
        && std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .is_err()
    {
        return Ok("evals triage: another sweep in progress — skipped".into());
    }
    let result = triage_locked(evals_db, tasks_db, dry_run, sweep_all);
    if !dry_run {
        let _ = std::fs::remove_file(&lock);
    }
    result
}

fn triage_locked(
    evals_db: &Path,
    tasks_db: &Path,
    dry_run: bool,
    sweep_all: bool,
) -> Result<String, String> {
    let traces = store::load(evals_db);
    let cursor = if sweep_all {
        0
    } else {
        store::triage_cursor(evals_db)
    };
    if !sweep_all && cursor == 0 && !traces.is_empty() {
        if !dry_run {
            store::set_triage_cursor(evals_db, traces.len())?;
        }
        return Ok(format!(
            "evals triage: cursor initialized — {} historical trace(s) skipped (use --all to sweep history)",
            traces.len()
        ));
    }
    let fresh = &traces[cursor.min(traces.len())..];
    let mut runs_seen: HashSet<String> = HashSet::new();
    let mut runs: Vec<String> = Vec::new();
    for t in fresh {
        if runs_seen.insert(t.run.clone()) {
            runs.push(t.run.clone());
        }
    }
    let mut latest_by_run: HashMap<String, Vec<crate::store::Trace>> = HashMap::new();
    let mut latest_by_case: HashMap<(String, String), crate::store::Trace> = HashMap::new();
    for t in traces.iter().filter(|t| t.expected.is_some()) {
        latest_by_case.insert((t.run.clone(), t.case.clone()), t.clone());
    }
    for ((run, _case), trace) in latest_by_case {
        latest_by_run.entry(run).or_default().push(trace);
    }
    let tstore = Store::open(tasks_db)?;
    let all = tstore.all()?;
    let mut out: Vec<String> = Vec::new();
    let mut created = 0usize;

    for run in runs {
        // Latest trace per case wins — precomputed once for all runs above.
        let latest = latest_by_run.get(&run).cloned().unwrap_or_default();
        let scores = score::score_run(&latest, &run, Matcher::Exact);
        if scores.is_empty() {
            continue; // nothing scorable (no expected values)
        }
        let failing: Vec<String> = scores
            .iter()
            .filter(|s| !s.pass)
            .map(|s| s.case.clone())
            .collect();
        if failing.is_empty() {
            continue;
        }
        // Owned by an open board task already?
        if let Some(tn) = embedded_task(&run) {
            if all.iter().any(|t| t.id == tn && t.col != "done") {
                out.push(format!("skip {run}: tracked by open {tn}"));
                continue;
            }
        }
        // Title prefixes include the separator so `run` can't prefix-collide
        // with `run-2` (dedupe key = exact run).
        let fix_prefix = format!("eval-fix: {run} —");
        let esc_prefix = format!("eval-fix (recurring): {run} —");
        if all.iter().any(|t| {
            (t.title.starts_with(&fix_prefix) || t.title.starts_with(&esc_prefix))
                && t.col != "done"
        }) {
            out.push(format!("skip {run}: open eval-fix task exists"));
            continue;
        }
        let recurred = all
            .iter()
            .any(|t| t.title.starts_with(&fix_prefix) && t.col == "done");
        if recurred
            && all
                .iter()
                .any(|t| t.title.starts_with(&esc_prefix) && t.col == "done")
        {
            // Escalation was also completed and it STILL fails — a human call.
            out.push(format!(
                "skip {run}: escalation already exhausted — needs orchestrator"
            ));
            continue;
        }
        let case_note = if failing.len() == 1 {
            failing[0].clone()
        } else {
            format!("{} (+{} more)", failing[0], failing.len() - 1)
        };
        let (title, prio, tag) = if recurred {
            (format!("{esc_prefix} {case_note}"), "p1", "escalate")
        } else {
            (format!("{fix_prefix} {case_note}"), "p2", "auto")
        };
        if dry_run {
            out.push(format!("would create ({prio}): {title}"));
            continue;
        }
        let t = Task {
            id: tstore.next_id()?,
            title,
            col: "ready".into(),
            prio: prio.into(),
            tags: vec!["eval-fix".into(), tag.into()],
            criteria: vec![
                (format!("`evals score --run {run}` reports 0 failing cases"), false),
                ("fixed at root cause in code/config — eval expectations NOT weakened/removed, and the green trace comes from re-running the real probe, never hand-logged".into(), false),
            ],
            created: now(),
            trans: vec![("ready".into(), now())],
            ..Task::default()
        };
        tstore.put(&t)?;
        created += 1;
        out.push(format!("created {} ({prio}): {}", t.id, t.title));
    }

    if !dry_run {
        store::set_triage_cursor(evals_db, traces.len())?;
    }
    if out.is_empty() {
        return Ok("evals triage: all fresh runs green — nothing to do".into());
    }
    out.push(format!("evals triage: {created} task(s) created"));
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Trace;
    use std::path::PathBuf;

    /// Per-test directory: the sweep lock and kill-switch files are
    /// directory-scoped, so parallel tests need isolated dirs.
    fn pair(n: &str) -> (PathBuf, PathBuf) {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch/triage")
            .join(n);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        (d.join("evals.semdb"), d.join("tasks.semdb"))
    }

    fn fail_trace(run: &str, case: &str) -> Trace {
        Trace {
            run: run.into(),
            case: case.into(),
            input: "in".into(),
            output: "actual".into(),
            expected: Some("wanted".into()),
            latency_ms: None,
            provenance: None,
        }
    }

    fn pass_trace(run: &str, case: &str) -> Trace {
        Trace {
            run: run.into(),
            case: case.into(),
            input: "in".into(),
            output: "same".into(),
            expected: Some("same".into()),
            latency_ms: None,
            provenance: None,
        }
    }

    #[test]
    fn failing_run_creates_one_task_idempotently() {
        let (ev, tk) = pair("t1");
        store::append(&ev, &fail_trace("r1", "c1")).unwrap();
        store::append(&ev, &fail_trace("r1", "c2")).unwrap();
        let r = triage(&ev, &tk, false, true).unwrap();
        assert!(r.contains("created T-1 (p2): eval-fix: r1 —"), "got: {r}");
        assert!(r.contains("(+1 more)"), "got: {r}");
        let t = Store::open(&tk).unwrap().get("T-1").unwrap();
        assert_eq!(t.col, "ready");
        assert_eq!(t.criteria.len(), 2);
        // Second sweep: dedupe, no new task.
        let r2 = triage(&ev, &tk, false, true).unwrap();
        assert!(
            r2.contains("skip r1: open eval-fix task exists"),
            "got: {r2}"
        );
        assert!(Store::open(&tk).unwrap().get("T-2").is_err());
    }

    #[test]
    fn green_run_creates_nothing() {
        let (ev, tk) = pair("t2");
        store::append(&ev, &pass_trace("g1", "c1")).unwrap();
        let r = triage(&ev, &tk, false, true).unwrap();
        assert!(r.contains("all fresh runs green"), "got: {r}");
    }

    #[test]
    fn incremental_bootstrap_skips_history_then_catches_new_failures() {
        let (ev, tk) = pair("t6");
        store::append(&ev, &fail_trace("old-red", "c1")).unwrap();
        // First incremental sweep: initialize cursor, create NOTHING.
        let r = triage(&ev, &tk, false, false).unwrap();
        assert!(r.contains("cursor initialized"), "got: {r}");
        assert!(Store::open(&tk).unwrap().all().unwrap().is_empty());
        // Old red stays quiet on the next sweep too.
        let r2 = triage(&ev, &tk, false, false).unwrap();
        assert!(r2.contains("all fresh runs green"), "got: {r2}");
        // A NEW failing trace after the cursor generates exactly one task.
        store::append(&ev, &fail_trace("new-red", "c1")).unwrap();
        let r3 = triage(&ev, &tk, false, false).unwrap();
        assert!(
            r3.contains("created T-1 (p2): eval-fix: new-red —"),
            "got: {r3}"
        );
        let titles: Vec<String> = Store::open(&tk)
            .unwrap()
            .all()
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(
            titles.len(),
            1,
            "old-red must not create a task: {titles:?}"
        );
    }

    #[test]
    fn latest_pass_supersedes_earlier_fail() {
        let (ev, tk) = pair("t7");
        store::append(&ev, &fail_trace("flip", "c1")).unwrap();
        store::append(&ev, &pass_trace("flip", "c1")).unwrap();
        let r = triage(&ev, &tk, false, true).unwrap();
        assert!(r.contains("all fresh runs green"), "got: {r}");
        assert!(Store::open(&tk).unwrap().all().unwrap().is_empty());
    }

    #[test]
    fn run_named_after_open_task_is_skipped() {
        let (ev, tk) = pair("t3");
        store::append(&ev, &fail_trace("T-9-self-heal", "c1")).unwrap();
        let s = Store::open(&tk).unwrap();
        s.put(&Task {
            id: "T-9".into(),
            title: "self heal".into(),
            col: "doing".into(),
            prio: "p1".into(),
            created: now(),
            trans: vec![("doing".into(), now())],
            ..Task::default()
        })
        .unwrap();
        let r = triage(&ev, &tk, false, true).unwrap();
        assert!(
            r.contains("skip T-9-self-heal: tracked by open T-9"),
            "got: {r}"
        );
    }

    #[test]
    fn refailure_after_done_escalates_once_to_p1() {
        let (ev, tk) = pair("t4");
        store::append(&ev, &fail_trace("r4", "c1")).unwrap();
        let s = Store::open(&tk).unwrap();
        s.put(&Task {
            id: "T-1".into(),
            title: "eval-fix: r4 — c1".into(),
            col: "done".into(),
            prio: "p2".into(),
            created: now(),
            trans: vec![],
            ..Task::default()
        })
        .unwrap();
        let r = triage(&ev, &tk, false, true).unwrap();
        assert!(
            r.contains("(p1): eval-fix (recurring): r4 — c1"),
            "got: {r}"
        );
        let esc = s
            .all()
            .unwrap()
            .into_iter()
            .find(|t| t.title.starts_with("eval-fix (recurring): r4 —"))
            .unwrap();
        assert_eq!(esc.prio, "p1");
        assert!(esc.tags.contains(&"escalate".to_string()));
        // Escalation open → skip; escalation done + still failing → human.
        let r2 = triage(&ev, &tk, false, true).unwrap();
        assert!(
            r2.contains("skip r4: open eval-fix task exists"),
            "got: {r2}"
        );
    }

    #[test]
    fn dry_run_creates_nothing() {
        let (ev, tk) = pair("t5");
        store::append(&ev, &fail_trace("r5", "c1")).unwrap();
        let r = triage(&ev, &tk, true, true).unwrap();
        assert!(r.contains("would create (p2)"), "got: {r}");
        assert!(Store::open(&tk).unwrap().all().unwrap().is_empty());
    }

    #[test]
    fn kill_switch_and_sweep_lock() {
        let (ev, tk) = pair("t8");
        store::append(&ev, &fail_trace("r8", "c1")).unwrap();
        let dir = ev.parent().unwrap();
        // Kill switch wins over everything.
        std::fs::write(dir.join("eval-triage.off"), "").unwrap();
        let r = triage(&ev, &tk, false, true).unwrap();
        assert!(r.contains("disabled by eval-triage.off"), "got: {r}");
        std::fs::remove_file(dir.join("eval-triage.off")).unwrap();
        // A fresh lock makes a second sweep skip.
        std::fs::write(dir.join(".eval-triage.lock"), "").unwrap();
        let r2 = triage(&ev, &tk, false, true).unwrap();
        assert!(r2.contains("another sweep in progress"), "got: {r2}");
        std::fs::remove_file(dir.join(".eval-triage.lock")).unwrap();
        // Lock released → sweep proceeds and cleans up its own lock.
        let r3 = triage(&ev, &tk, false, true).unwrap();
        assert!(r3.contains("created T-1"), "got: {r3}");
        assert!(!dir.join(".eval-triage.lock").exists());
    }

    #[test]
    fn embedded_task_extraction() {
        assert_eq!(embedded_task("T-87-gateway-mute"), Some("T-87".into()));
        assert_eq!(embedded_task("prefix-T-5"), Some("T-5".into()));
        assert_eq!(embedded_task("no-task-here"), None);
        assert_eq!(embedded_task("T-x"), None);
    }
}
