//! Deny-by-default access policy, stored in a semdb table. A grant is a row
//! keyed `caller::secret`; presence = allowed. Wildcard `*` secret grants a
//! caller access to everything.

use std::path::{Path, PathBuf};

use semdb::storage::Db;

const PLACEHOLDER_VEC: [f32; 1] = [0.0];

pub struct Policy {
    dir: PathBuf,
}

impl Policy {
    pub fn open(dir: &Path) -> Result<Policy, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        Ok(Policy { dir: dir.to_path_buf() })
    }

    fn table(&self) -> PathBuf {
        self.dir.join("policy.semdb")
    }

    fn db(&self) -> Result<Db, String> {
        let p = self.table();
        if p.exists() { Db::open(&p) } else { Db::create(&p) }
    }

    fn key(caller: &str, secret: &str) -> String {
        format!("{caller}::{secret}")
    }

    pub fn allow(&self, caller: &str, secret: &str) -> Result<(), String> {
        let mut db = self.db()?;
        db.put(&Self::key(caller, secret), "grant", PLACEHOLDER_VEC.to_vec())
    }

    /// Deny by default: only true when a specific or wildcard grant exists.
    pub fn permitted(&self, caller: &str, secret: &str) -> Result<bool, String> {
        let p = self.table();
        if !p.exists() {
            return Ok(false);
        }
        let db = Db::open(&p)?;
        Ok(db.get(&Self::key(caller, secret)).is_some() || db.get(&Self::key(caller, "*")).is_some())
    }

    pub fn grants(&self) -> Result<Vec<String>, String> {
        let p = self.table();
        if !p.exists() {
            return Ok(Vec::new());
        }
        let db = Db::open(&p)?;
        let mut g: Vec<String> = db.index.keys().cloned().collect();
        g.sort();
        Ok(g)
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
    fn deny_by_default_then_grant() {
        let pol = Policy::open(&scratch("sec-pol")).unwrap();
        assert!(!pol.permitted("agentA", "api").unwrap());
        pol.allow("agentA", "api").unwrap();
        assert!(pol.permitted("agentA", "api").unwrap());
        assert!(!pol.permitted("agentA", "other").unwrap());
        assert!(!pol.permitted("agentB", "api").unwrap());
    }

    #[test]
    fn wildcard_grant() {
        let pol = Policy::open(&scratch("sec-wild")).unwrap();
        pol.allow("trusted", "*").unwrap();
        assert!(pol.permitted("trusted", "anything").unwrap());
    }
}
