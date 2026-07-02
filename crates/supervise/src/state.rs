//! Service state persisted in a semdb table (one row per service). Non-semantic,
//! so rows carry a placeholder vector. Row meta is a small JSON object.

use std::path::{Path, PathBuf};

use semdb::json;
use semdb::storage::Db;

const PLACEHOLDER_VEC: [f32; 1] = [0.0];

#[derive(Default, Clone)]
pub struct Record {
    pub desired_up: bool,
    pub pid: u32,
    pub cmd: String,
    pub started_at: i64,
    pub restarts: u64,
}

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn open(data_dir: &Path) -> Result<Store, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        Ok(Store { path: data_dir.join("supervise.semdb") })
    }

    fn db(&self) -> Result<Db, String> {
        if self.path.exists() { Db::open(&self.path) } else { Db::create(&self.path) }
    }

    pub fn get(&self, name: &str) -> Record {
        let db = match self.db() {
            Ok(d) => d,
            Err(_) => return Record::default(),
        };
        match db.get(name) {
            Some(e) => parse(&e.meta),
            None => Record::default(),
        }
    }

    pub fn put(&self, name: &str, r: &Record) -> Result<(), String> {
        let meta = format!(
            r#"{{"desired_up":{},"pid":{},"cmd":"{}","started_at":{},"restarts":{}}}"#,
            r.desired_up,
            r.pid,
            json::escape(&r.cmd),
            r.started_at,
            r.restarts
        );
        let mut db = self.db()?;
        db.put(name, &meta, PLACEHOLDER_VEC.to_vec())?;
        db.compact() // tiny table; keep it from growing per update
    }

    pub fn names(&self) -> Vec<String> {
        match self.db() {
            Ok(db) => {
                let mut v: Vec<String> = db.index.keys().cloned().collect();
                v.sort();
                v
            }
            Err(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn record_roundtrip_and_default() {
        let s = Store::open(&scratch("sup-state")).unwrap();
        assert_eq!(s.get("nope").pid, 0); // default for missing
        let r = Record { desired_up: true, pid: 4242, cmd: "target/release/schedule run".into(), started_at: 1000, restarts: 3 };
        s.put("scheduler", &r).unwrap();
        let got = s.get("scheduler");
        assert!(got.desired_up);
        assert_eq!(got.pid, 4242);
        assert_eq!(got.cmd, "target/release/schedule run");
        assert_eq!(got.restarts, 3);
        assert_eq!(s.names(), vec!["scheduler".to_string()]);
    }

    #[test]
    fn desired_up_false_roundtrips() {
        let s = Store::open(&scratch("sup-down")).unwrap();
        s.put("chromium", &Record { desired_up: false, pid: 0, cmd: "chromium".into(), started_at: 0, restarts: 0 }).unwrap();
        assert!(!s.get("chromium").desired_up);
    }
}

fn parse(meta: &str) -> Record {
    let v = match json::parse(meta) {
        Ok(v) => v,
        Err(_) => return Record::default(),
    };
    Record {
        desired_up: matches!(v.get("desired_up"), Some(json::Value::Bool(true))),
        pid: v.get("pid").and_then(|x| x.as_f64()).unwrap_or(0.0) as u32,
        cmd: v.get("cmd").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        started_at: v.get("started_at").and_then(|x| x.as_f64()).unwrap_or(0.0) as i64,
        restarts: v.get("restarts").and_then(|x| x.as_f64()).unwrap_or(0.0) as u64,
    }
}
