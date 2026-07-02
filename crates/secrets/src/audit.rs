//! Access audit log, stored append-only in a semdb table (one row per access).

use std::path::{Path, PathBuf};

use semdb::storage::Db;

const PLACEHOLDER_VEC: [f32; 1] = [0.0];

pub struct Audit {
    dir: PathBuf,
}

impl Audit {
    pub fn open(dir: &Path) -> Result<Audit, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        Ok(Audit { dir: dir.to_path_buf() })
    }

    fn table(&self) -> PathBuf {
        self.dir.join("audit.semdb")
    }

    fn db(&self) -> Result<Db, String> {
        let p = self.table();
        if p.exists() { Db::open(&p) } else { Db::create(&p) }
    }

    pub fn record(&self, caller: &str, secret: &str, allowed: bool) -> Result<(), String> {
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let decision = if allowed { "allow" } else { "deny" };
        let meta = format!(r#"{{"ts":{ts},"caller":"{}","secret":"{}","decision":"{decision}"}}"#, esc(caller), esc(secret));
        let mut db = self.db()?;
        // Unique row id: timestamp + count keeps append-only ordering.
        let id = format!("{ts:039}-{}", db.index.len());
        db.put(&id, &meta, PLACEHOLDER_VEC.to_vec())
    }

    /// Record a non-read event (set / list / policy-allow) so mutations and
    /// enumerations are auditable, not just reads.
    pub fn event(&self, action: &str, target: &str) -> Result<(), String> {
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let meta = format!(r#"{{"ts":{ts},"action":"{}","target":"{}"}}"#, esc(action), esc(target));
        let mut db = self.db()?;
        let id = format!("{ts:039}-{}", db.index.len());
        db.put(&id, &meta, PLACEHOLDER_VEC.to_vec())
    }

    pub fn entries(&self) -> Result<Vec<String>, String> {
        let p = self.table();
        if !p.exists() {
            return Ok(Vec::new());
        }
        let db = Db::open(&p)?;
        let mut rows: Vec<(String, String)> = db.index.iter().map(|(k, e)| (k.clone(), e.meta.clone())).collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(rows.into_iter().map(|(_, m)| m).collect())
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
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
    fn records_and_orders() {
        let a = Audit::open(&scratch("sec-audit")).unwrap();
        a.record("x", "api", false).unwrap();
        a.record("x", "api", true).unwrap();
        let e = a.entries().unwrap();
        assert_eq!(e.len(), 2);
        assert!(e[0].contains("\"decision\":\"deny\""));
        assert!(e[1].contains("\"decision\":\"allow\""));
    }
}
