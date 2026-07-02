//! Kanban task store — one semdb row per task (non-semantic, placeholder
//! vector), plus a `_board` row for WIP limits. Meta is a small JSON object;
//! column transitions are recorded in-row for cycle-time metrics.

use std::path::{Path, PathBuf};

use semdb::json::{self, Value};
use semdb::storage::Db;

const PLACEHOLDER_VEC: [f32; 1] = [0.0];

/// Kanban columns, in flow order. Blocked is a flag, not a column.
pub const COLUMNS: [&str; 5] = ["backlog", "ready", "doing", "review", "done"];

#[derive(Default, Clone)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub col: String,
    pub prio: String, // p1 (highest) | p2 | p3
    pub tags: Vec<String>,
    pub blocked: String, // reason, empty = not blocked
    pub owner: String, // agent/user that pulled it to doing; empty = unowned
    pub criteria: Vec<(String, bool)>,
    pub created: i64,
    pub done_ts: i64,
    /// (column, unix-ts) history, oldest first.
    pub trans: Vec<(String, i64)>,
}

impl Task {
    /// First time the task entered `doing` (cycle-time start).
    pub fn first_doing(&self) -> Option<i64> {
        self.trans.iter().find(|(c, _)| c == "doing").map(|(_, t)| *t)
    }

    pub fn criteria_done(&self) -> usize {
        self.criteria.iter().filter(|(_, d)| *d).count()
    }
}

#[derive(Clone, Copy)]
pub struct Wip {
    pub doing: usize,
    pub review: usize,
}

impl Default for Wip {
    // Doing=1 is deliberate: one thing at a time is the whole methodology.
    fn default() -> Wip {
        Wip { doing: 1, review: 3 }
    }
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
        Ok(Store { path: db_path.to_path_buf() })
    }

    fn db(&self) -> Result<Db, String> {
        if self.path.exists() { Db::open(&self.path) } else { Db::create(&self.path) }
    }

    pub fn next_id(&self) -> Result<String, String> {
        let db = self.db()?;
        let max = db
            .index
            .keys()
            .filter_map(|k| k.strip_prefix("T-").and_then(|n| n.parse::<u64>().ok()))
            .max()
            .unwrap_or(0);
        Ok(format!("T-{}", max + 1))
    }

    pub fn put(&self, t: &Task) -> Result<(), String> {
        let mut db = self.db()?;
        db.put(&t.id, &encode(t), PLACEHOLDER_VEC.to_vec())
    }

    pub fn get(&self, id: &str) -> Result<Task, String> {
        let db = self.db()?;
        let e = db.get(id).ok_or_else(|| format!("no task '{id}'"))?;
        Ok(decode(id, &e.meta))
    }

    pub fn remove(&self, id: &str) -> Result<bool, String> {
        let mut db = self.db()?;
        db.delete(id)
    }

    pub fn all(&self) -> Result<Vec<Task>, String> {
        let db = self.db()?;
        let mut ts: Vec<Task> = db
            .index
            .iter()
            .filter(|(k, _)| k.starts_with("T-"))
            .map(|(k, e)| decode(k, &e.meta))
            .collect();
        ts.sort_by(|a, b| {
            let n = |t: &Task| t.id.strip_prefix("T-").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
            n(a).cmp(&n(b))
        });
        Ok(ts)
    }

    /// Open-count trend for the statusline: 1 rising, -1 falling, 0 unknown.
    /// Persists the last seen open count in a reserved `_trend` row (not a
    /// `T-` key — invisible to `all()`); the direction of the LAST CHANGE is
    /// kept until the count moves again, so frequent probes don't flatten it.
    pub fn trend_dir(&self, open_now: usize) -> i64 {
        let mut db = match self.db() {
            Ok(d) => d,
            Err(_) => return 0,
        };
        let stored = db.get("_trend").and_then(|e| json::parse(&e.meta).ok());
        let prev_open = stored.as_ref().and_then(|v| v.get("open").and_then(Value::as_f64)).map(|f| f as i64);
        let prev_dir = stored.as_ref().and_then(|v| v.get("dir").and_then(Value::as_f64)).map(|f| f as i64).unwrap_or(0);
        match prev_open {
            Some(p) if p == open_now as i64 => prev_dir,
            None => {
                let _ = db.put("_trend", &format!(r#"{{"open":{open_now},"dir":0}}"#), PLACEHOLDER_VEC.to_vec());
                0
            }
            Some(p) => {
                let dir = if (open_now as i64) > p { 1 } else { -1 };
                let _ = db.put("_trend", &format!(r#"{{"open":{open_now},"dir":{dir}}}"#), PLACEHOLDER_VEC.to_vec());
                dir
            }
        }
    }

    pub fn wip(&self) -> Wip {
        let db = match self.db() {
            Ok(d) => d,
            Err(_) => return Wip::default(),
        };
        let Some(e) = db.get("_board") else { return Wip::default() };
        let v = match json::parse(&e.meta) {
            Ok(v) => v,
            Err(_) => return Wip::default(),
        };
        let get = |k: &str, d: usize| v.get(k).and_then(|x| x.as_f64()).map(|f| f as usize).unwrap_or(d);
        Wip { doing: get("doing", 1), review: get("review", 3) }
    }

    pub fn set_wip(&self, w: Wip) -> Result<(), String> {
        let mut db = self.db()?;
        db.put("_board", &format!(r#"{{"doing":{},"review":{}}}"#, w.doing, w.review), PLACEHOLDER_VEC.to_vec())
    }
}

fn encode(t: &Task) -> String {
    let tags = t.tags.iter().map(|s| format!("\"{}\"", json::escape(s))).collect::<Vec<_>>().join(",");
    let crit = t
        .criteria
        .iter()
        .map(|(c, d)| format!(r#"{{"text":"{}","done":{}}}"#, json::escape(c), d))
        .collect::<Vec<_>>()
        .join(",");
    let trans = t
        .trans
        .iter()
        .map(|(c, ts)| format!(r#"["{}",{}]"#, json::escape(c), ts))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"title":"{}","col":"{}","prio":"{}","tags":[{}],"blocked":"{}","owner":"{}","criteria":[{}],"created":{},"done_ts":{},"trans":[{}]}}"#,
        json::escape(&t.title),
        json::escape(&t.col),
        json::escape(&t.prio),
        tags,
        json::escape(&t.blocked),
        json::escape(&t.owner),
        crit,
        t.created,
        t.done_ts,
        trans
    )
}

fn decode(id: &str, meta: &str) -> Task {
    let v = match json::parse(meta) {
        Ok(v) => v,
        Err(_) => return Task { id: id.to_string(), ..Task::default() },
    };
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let n = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as i64;
    let arr = |k: &str| -> Vec<Value> { v.get(k).and_then(|x| x.as_arr()).map(|a| a.to_vec()).unwrap_or_default() };
    Task {
        id: id.to_string(),
        title: s("title"),
        col: s("col"),
        prio: s("prio"),
        blocked: s("blocked"),
        owner: s("owner"),
        created: n("created"),
        done_ts: n("done_ts"),
        tags: arr("tags").iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        criteria: arr("criteria")
            .iter()
            .map(|c| {
                (
                    c.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    matches!(c.get("done"), Some(Value::Bool(true))),
                )
            })
            .collect(),
        trans: arr("trans")
            .iter()
            .filter_map(|p| {
                let a = p.as_arr()?;
                Some((a.first()?.as_str()?.to_string(), a.get(1)?.as_f64()? as i64))
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trend_dir_tracks_last_change_direction() {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("trend.semdb");
        let _ = std::fs::remove_file(&p);
        let s = Store::open(&p).unwrap();
        assert_eq!(s.trend_dir(10), 0); // first observation: unknown
        assert_eq!(s.trend_dir(10), 0); // flat: stays unknown
        assert_eq!(s.trend_dir(12), 1); // rose
        assert_eq!(s.trend_dir(12), 1); // flat: keeps last direction
        assert_eq!(s.trend_dir(9), -1); // fell
        assert_eq!(s.trend_dir(9), -1); // flat: keeps falling marker
        // the reserved row is invisible to task listings
        assert!(s.all().unwrap().is_empty());
    }

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&d);
        d.join("tasks.semdb")
    }

    #[test]
    fn roundtrip_and_ids() {
        let s = Store::open(&scratch("tasks-rt")).unwrap();
        assert_eq!(s.next_id().unwrap(), "T-1");
        let t = Task {
            id: "T-1".into(),
            title: "fix \"quoted\" thing\nwith newline".into(),
            col: "ready".into(),
            prio: "p1".into(),
            tags: vec!["golf".into()],
            blocked: String::new(),
            owner: "builder".into(),
            criteria: vec![("compiles".into(), true), ("tested".into(), false)],
            created: 100,
            done_ts: 0,
            trans: vec![("backlog".into(), 100), ("ready".into(), 110)],
        };
        s.put(&t).unwrap();
        assert_eq!(s.next_id().unwrap(), "T-2");
        let got = s.get("T-1").unwrap();
        assert_eq!(got.title, t.title);
        assert_eq!(got.owner, "builder");
        assert_eq!(got.criteria.len(), 2);
        assert_eq!(got.criteria_done(), 1);
        assert_eq!(got.trans.len(), 2);
        assert_eq!(got.first_doing(), None);
    }

    #[test]
    fn wip_defaults_and_set() {
        let s = Store::open(&scratch("tasks-wip")).unwrap();
        assert_eq!(s.wip().doing, 1);
        s.set_wip(Wip { doing: 2, review: 4 }).unwrap();
        assert_eq!(s.wip().doing, 2);
        assert_eq!(s.wip().review, 4);
    }
}
