//! Durable job store backed by a semdb table (rule 7: all data in semdb tables,
//! not bespoke JSONL). The public API stays event-shaped — `append(Event)` +
//! `replay() -> jobs` — so the CLI and runner are unchanged, but each event is
//! applied to the job-state table immediately (put/delete/read-modify-write)
//! and the table is the source of truth. semdb's CRC-framed log gives crash
//! safety for free (a torn tail is truncated on open).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use semdb::json;
use semdb::storage::Db;

const PLACEHOLDER_VEC: [f32; 1] = [0.0]; // non-semantic rows

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Job registered: id, cron expression, command, one-shot flag.
    Scheduled { id: String, cron: String, cmd: String, once: bool },
    /// Job removed.
    Removed { id: String },
    /// A run finished: id, fire time (unix), exit code, attempt (1 or 2).
    Ran { id: String, fire: i64, exit: i32, attempt: u8 },
    /// Pause/resume a job (skip it in the runner without removing it).
    SetEnabled { id: String, enabled: bool },
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub cron: String,
    pub cmd: String,
    /// Last fire time recorded as completed (any exit).
    pub last_fire: Option<i64>,
    /// One-shot: removed by the runner after it fires once.
    pub once: bool,
    /// Paused jobs are skipped by the runner (resume to re-enable).
    pub enabled: bool,
}

pub struct Journal {
    path: PathBuf,
}

impl Journal {
    /// `path` may be given as the legacy `*.jsonl` name; it is normalized to a
    /// `*.semdb` table so old CLI invocations keep working.
    pub fn new(path: &Path) -> Journal {
        let mut p = path.to_path_buf();
        if p.extension().map(|e| e == "jsonl").unwrap_or(false) {
            p.set_extension("semdb");
        }
        Journal { path: p }
    }

    fn db(&self) -> Result<Db, String> {
        if self.path.exists() { Db::open(&self.path) } else { Db::create(&self.path) }
    }

    fn row(job: &Job) -> String {
        format!(
            r#"{{"cron":"{}","cmd":"{}","last_fire":{},"once":{},"enabled":{}}}"#,
            json::escape(&job.cron),
            json::escape(&job.cmd),
            job.last_fire.unwrap_or(-1),
            job.once,
            job.enabled
        )
    }

    fn parse_row(id: &str, meta: &str) -> Option<Job> {
        let v = json::parse(meta).ok()?;
        let cron = v.get("cron")?.as_str()?.to_string();
        let cmd = v.get("cmd")?.as_str()?.to_string();
        let lf = v.get("last_fire").and_then(|x| x.as_f64()).unwrap_or(-1.0) as i64;
        let once = matches!(v.get("once"), Some(json::Value::Bool(true)));
        // Default enabled=true for rows written before this field existed.
        let enabled = !matches!(v.get("enabled"), Some(json::Value::Bool(false)));
        Some(Job { id: id.to_string(), cron, cmd, last_fire: if lf < 0 { None } else { Some(lf) }, once, enabled })
    }

    pub fn append(&self, ev: &Event) -> Result<(), String> {
        let mut db = self.db()?;
        match ev {
            Event::Scheduled { id, cron, cmd, once } => {
                // Preserve last_fire/enabled if the job is being re-registered.
                let prev = db.get(id).and_then(|e| Self::parse_row(id, &e.meta));
                let job = Job {
                    id: id.clone(),
                    cron: cron.clone(),
                    cmd: cmd.clone(),
                    last_fire: prev.as_ref().and_then(|j| j.last_fire),
                    once: *once,
                    enabled: prev.map(|j| j.enabled).unwrap_or(true),
                };
                db.put(id, &Self::row(&job), PLACEHOLDER_VEC.to_vec())?;
            }
            Event::Removed { id } => {
                db.delete(id)?;
                db.compact()?; // keep the table from growing with tombstones
            }
            Event::Ran { id, fire, .. } => {
                if let Some(mut job) = db.get(id).and_then(|e| Self::parse_row(id, &e.meta)) {
                    job.last_fire = Some(job.last_fire.map_or(*fire, |p| p.max(*fire)));
                    db.put(id, &Self::row(&job), PLACEHOLDER_VEC.to_vec())?;
                }
            }
            Event::SetEnabled { id, enabled } => {
                if let Some(mut job) = db.get(id).and_then(|e| Self::parse_row(id, &e.meta)) {
                    job.enabled = *enabled;
                    db.put(id, &Self::row(&job), PLACEHOLDER_VEC.to_vec())?;
                }
            }
        }
        Ok(())
    }

    /// Current live jobs (id → Job).
    pub fn replay(&self) -> Result<HashMap<String, Job>, String> {
        let mut jobs = HashMap::new();
        if !self.path.exists() {
            return Ok(jobs);
        }
        let db = Db::open(&self.path)?;
        for (id, entry) in db.index.iter() {
            if let Some(job) = Self::parse_row(id, &entry.meta) {
                jobs.insert(id.clone(), job);
            }
        }
        Ok(jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("journal-{name}.semdb"));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn roundtrip_and_replay() {
        let j = Journal::new(&scratch("rt"));
        j.append(&Event::Scheduled { id: "a".into(), cron: "* * * * *".into(), cmd: "echo \"hi\"".into(), once: false }).unwrap();
        j.append(&Event::Ran { id: "a".into(), fire: 100, exit: 0, attempt: 1 }).unwrap();
        j.append(&Event::Scheduled { id: "b".into(), cron: "0 0 * * *".into(), cmd: "true".into(), once: false }).unwrap();
        j.append(&Event::Removed { id: "b".into() }).unwrap();
        let state = j.replay().unwrap();
        assert_eq!(state.len(), 1);
        let a = &state["a"];
        assert_eq!(a.cmd, "echo \"hi\"");
        assert_eq!(a.last_fire, Some(100));
    }

    #[test]
    fn ran_keeps_latest_fire() {
        let j = Journal::new(&scratch("fire"));
        j.append(&Event::Scheduled { id: "a".into(), cron: "* * * * *".into(), cmd: "true".into(), once: false }).unwrap();
        j.append(&Event::Ran { id: "a".into(), fire: 200, exit: 0, attempt: 1 }).unwrap();
        j.append(&Event::Ran { id: "a".into(), fire: 100, exit: 0, attempt: 1 }).unwrap();
        assert_eq!(j.replay().unwrap()["a"].last_fire, Some(200));
    }

    #[test]
    fn legacy_jsonl_path_maps_to_semdb() {
        let p = scratch("legacy");
        let jsonl = p.with_extension("jsonl");
        let j = Journal::new(&jsonl);
        j.append(&Event::Scheduled { id: "a".into(), cron: "* * * * *".into(), cmd: "true".into(), once: false }).unwrap();
        assert!(p.exists(), "should have written the .semdb table");
        assert_eq!(j.replay().unwrap().len(), 1);
    }
}
