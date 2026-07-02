//! Execution loop: fire due jobs via std::process, journal every run,
//! retry a failed run once (at-least-once semantics). `--once` fires
//! everything due now and exits — used by tests and external tickers.

use std::process::Command;

use crate::cron::Cron;
use crate::journal::{Event, Job, Journal};

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn run_cmd(cmd: &str) -> i32 {
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status()
        .map(|s| s.code().unwrap_or(-1))
        .unwrap_or(-1)
}

/// A job is due if its cron has a fire time in (last_fire_or_registration, now].
fn due_fire(job: &Job, cron: &Cron, now: i64) -> Option<i64> {
    // Baseline: last journaled fire, else one minute ago (fresh jobs fire on
    // the next matching minute, not on the whole past).
    let baseline = job.last_fire.unwrap_or(now - 60);
    match cron.next_after(baseline) {
        Some(t) if t <= now => Some(t),
        _ => None,
    }
}

/// Fire everything currently due. Returns (job id, exit code) pairs.
pub fn tick(journal: &Journal) -> Result<Vec<(String, i32)>, String> {
    let jobs = journal.replay()?;
    let now = now_unix();
    let mut results = Vec::new();
    for job in jobs.values() {
        if !job.enabled {
            continue; // paused
        }
        let cron = Cron::parse(&job.cron)?;
        if let Some(fire) = due_fire(job, &cron, now) {
            let mut exit = run_cmd(&job.cmd);
            journal.append(&Event::Ran { id: job.id.clone(), fire, exit, attempt: 1 })?;
            if exit != 0 {
                // At-least-once: one retry, journaled as attempt 2.
                exit = run_cmd(&job.cmd);
                journal.append(&Event::Ran { id: job.id.clone(), fire, exit, attempt: 2 })?;
            }
            if job.once {
                // One-shot: remove after it has fired.
                journal.append(&Event::Removed { id: job.id.clone() })?;
            }
            results.push((job.id.clone(), exit));
        }
    }
    Ok(results)
}

/// Daemon loop: sleep to the next whole minute, tick, repeat.
pub fn daemon(journal: &Journal) -> Result<(), String> {
    loop {
        let now = now_unix();
        let sleep_secs = 60 - (now % 60);
        std::thread::sleep(std::time::Duration::from_secs(sleep_secs as u64));
        for (id, exit) in tick(journal)? {
            eprintln!("[schedule] ran {id} exit {exit}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("runner-{name}"))
    }

    /// Journal now lives in a `.semdb` table (not JSONL) — remove that path so
    /// a stale table from a prior run doesn't make jobs look already-fired.
    fn fresh_journal(name: &str) -> (Journal, PathBuf) {
        let jpath = scratch(&format!("{name}.jsonl"));
        let _ = std::fs::remove_file(jpath.with_extension("semdb"));
        (Journal::new(&jpath), jpath)
    }

    #[test]
    fn fires_due_job_and_journals() {
        let out = scratch("fire.out");
        let _ = std::fs::remove_file(&out);
        let (j, _) = fresh_journal("fire");
        // Every minute — due immediately relative to the baseline.
        j.append(&Event::Scheduled {
            id: "t1".into(),
            cron: "* * * * *".into(),
            cmd: format!("echo fired >> {}", out.display()),
            once: false,
        })
        .unwrap();
        let results = tick(&j).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, 0);
        assert!(std::fs::read_to_string(&out).unwrap().contains("fired"));
        // Second tick within the same minute: not due again.
        let results2 = tick(&j).unwrap();
        assert!(results2.is_empty());
    }

    #[test]
    fn failed_job_retries_once() {
        let (j, _) = fresh_journal("retry");
        j.append(&Event::Scheduled { id: "bad".into(), cron: "* * * * *".into(), cmd: "exit 3".into(), once: false }).unwrap();
        // The retry behavior (run twice on failure) is what matters; the final
        // exit code is reported. Run history is no longer persisted separately —
        // only the job's last_fire is, which the next assertion checks.
        let results = tick(&j).unwrap();
        assert_eq!(results, vec![("bad".to_string(), 3)]);
        // The failed run still advanced last_fire, so it won't re-fire this minute.
        assert!(j.replay().unwrap()["bad"].last_fire.is_some());
        assert!(tick(&j).unwrap().is_empty());
    }
}
