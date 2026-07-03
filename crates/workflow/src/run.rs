//! Workflow run state — one semdb row per run. The engine is the state
//! machine; the agent does the thinking. Advancing a step REQUIRES evidence
//! (the PAI inline-verification mandate, enforced deterministically).

use std::path::{Path, PathBuf};

use semdb::json;
use semdb::storage::Db;

const PLACEHOLDER_VEC: [f32; 1] = [0.0];
const COMPACT_MIN_RECORDS: u64 = 4096;
const COMPACT_RATIO: u64 = 4;

#[derive(Default, Clone)]
pub struct Run {
    pub id: String,
    pub wf: String,
    pub task: String,   // linked tasks-board id (T-n), may be empty
    pub step: usize,    // 0-based index of the CURRENT (not yet done) step
    pub status: String, // running | done | aborted
    pub started: i64,
    pub updated: i64,
    pub evidence: Vec<String>, // evidence per completed step, in order
}

/// Verification mandate shared by `advance` and the drive engine: evidence
/// must describe what was verified — trivial acknowledgments are refused.
pub fn valid_evidence(ev: &str) -> Result<String, String> {
    let t = ev.trim();
    let trivial = ["done", "ok", "works", "finished", "complete"];
    if t.chars().count() < 10 || trivial.contains(&t.to_lowercase().as_str()) {
        return Err("evidence required: describe WHAT you verified and HOW (≥10 chars; 'done'/'ok' rejected)".into());
    }
    Ok(t.to_string())
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn open(db_path: &Path) -> Result<Store, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        Ok(Store {
            path: db_path.to_path_buf(),
        })
    }

    fn db(&self) -> Result<Db, String> {
        if self.path.exists() {
            Db::open(&self.path)
        } else {
            Db::create(&self.path)
        }
    }

    pub fn next_id(&self) -> Result<String, String> {
        let db = self.db()?;
        let max = db
            .index
            .keys()
            .filter_map(|k| k.strip_prefix("W-").and_then(|n| n.parse::<u64>().ok()))
            .max()
            .unwrap_or(0);
        Ok(format!("W-{}", max + 1))
    }

    pub fn put(&self, r: &Run) -> Result<(), String> {
        let mut db = self.db()?;
        db.put(&r.id, &encode(r), PLACEHOLDER_VEC.to_vec())?;
        compact_if_bloated(&mut db)
    }

    pub fn get(&self, id: &str) -> Result<Run, String> {
        let db = self.db()?;
        let e = db.get(id).ok_or_else(|| format!("no run '{id}'"))?;
        Ok(decode(id, &e.meta))
    }

    pub fn all(&self) -> Result<Vec<Run>, String> {
        let db = self.db()?;
        let mut rs: Vec<Run> = db
            .index
            .iter()
            .filter(|(k, _)| k.starts_with("W-"))
            .map(|(k, e)| decode(k, &e.meta))
            .collect();
        rs.sort_by_key(|r| {
            r.id.strip_prefix("W-")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        });
        Ok(rs)
    }

    /// Most recent run still in `running` state.
    pub fn latest_running(&self) -> Result<Option<Run>, String> {
        Ok(self
            .all()?
            .into_iter()
            .rev()
            .find(|r| r.status == "running"))
    }
}

fn compact_min_records() -> u64 {
    std::env::var("SMARTAGENT_STORE_COMPACT_MIN_RECORDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(COMPACT_MIN_RECORDS)
}

fn compact_if_bloated(db: &mut Db) -> Result<(), String> {
    if db.records > compact_min_records()
        && db.records > COMPACT_RATIO * (db.index.len() as u64).max(1)
    {
        db.compact()?;
    }
    Ok(())
}

fn encode(r: &Run) -> String {
    let ev = r
        .evidence
        .iter()
        .map(|e| format!("\"{}\"", json::escape(e)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"wf":"{}","task":"{}","step":{},"status":"{}","started":{},"updated":{},"evidence":[{}]}}"#,
        json::escape(&r.wf),
        json::escape(&r.task),
        r.step,
        json::escape(&r.status),
        r.started,
        r.updated,
        ev
    )
}

fn decode(id: &str, meta: &str) -> Run {
    let v = match json::parse(meta) {
        Ok(v) => v,
        Err(_) => {
            return Run {
                id: id.to_string(),
                ..Run::default()
            }
        }
    };
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let n = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as i64;
    Run {
        id: id.to_string(),
        wf: s("wf"),
        task: s("task"),
        step: n("step") as usize,
        status: s("status"),
        started: n("started"),
        updated: n("updated"),
        evidence: v
            .get("evidence")
            .and_then(|x| x.as_arr())
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opportunistic_compaction_bounds_overwrite_bloat() {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch/wf-run-compact");
        let _ = std::fs::remove_dir_all(&d);
        let s = Store::open(&d.join("workflow.semdb")).unwrap();
        let mut r = Run {
            id: "W-1".into(),
            wf: "demo".into(),
            task: "T-1".into(),
            status: "running".into(),
            started: 1,
            updated: 1,
            ..Run::default()
        };
        std::env::set_var("SMARTAGENT_STORE_COMPACT_MIN_RECORDS", "16");
        for i in 0..21 {
            r.updated = i;
            s.put(&r).unwrap();
        }
        let db = s.db().unwrap();
        assert!(
            db.records <= (db.index.len() as u64) + 8,
            "records={} entries={}",
            db.records,
            db.index.len()
        );
        std::env::remove_var("SMARTAGENT_STORE_COMPACT_MIN_RECORDS");
        assert_eq!(s.get("W-1").unwrap().updated, 20);
    }

    #[test]
    fn roundtrip_and_latest_running() {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch/wf-run");
        let _ = std::fs::remove_dir_all(&d);
        let s = Store::open(&d.join("workflow.semdb")).unwrap();
        assert_eq!(s.next_id().unwrap(), "W-1");
        let r = Run {
            id: "W-1".into(),
            wf: "demo".into(),
            task: "T-9".into(),
            step: 1,
            status: "running".into(),
            started: 5,
            updated: 6,
            evidence: vec!["step one \"done\"".into()],
        };
        s.put(&r).unwrap();
        let got = s.get("W-1").unwrap();
        assert_eq!(got.wf, "demo");
        assert_eq!(got.task, "T-9");
        assert_eq!(got.evidence.len(), 1);
        assert_eq!(s.latest_running().unwrap().unwrap().id, "W-1");
    }
}
