//! Three-tier memory (Mem0 concept), each tier a semdb file under a root dir.
//!
//!   working  — recent context, capped (oldest evicted past the cap)
//!   episodic — timestamped events, unbounded
//!   semantic — distilled durable facts
//!
//! Recall is semantic vector search over the requested tiers, results tagged
//! by their tier. Embeddings come from the external endpoint via semdb::http.

use std::path::{Path, PathBuf};

use semdb::cli;
use semdb::config::Config;
use semdb::http;
use semdb::storage::Db;

pub const TIERS: [&str; 3] = ["working", "episodic", "semantic"];
const WORKING_CAP: usize = 50;

pub struct Memory {
    dir: PathBuf,
}

pub struct Recalled {
    pub tier: String,
    pub id: String,
    pub score: f32,
    pub text: String,
}

impl Memory {
    pub fn new(dir: &Path) -> Memory {
        Memory { dir: dir.to_path_buf() }
    }

    fn tier_path(&self, tier: &str) -> PathBuf {
        self.dir.join(format!("{tier}.semdb"))
    }

    fn open_or_create(&self, tier: &str) -> Result<Db, String> {
        validate_tier(tier)?;
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = self.tier_path(tier);
        if path.exists() {
            Db::open(&path)
        } else {
            Db::create(&path)
        }
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let cfg = Config::load();
        let ep = cfg
            .resolve("embeddings_endpoint", "SEMDB_ENDPOINT", None)
            .ok_or("no embeddings_endpoint in config/smartagent.conf")?;
        let (host, port) = ep.rsplit_once(':').ok_or("endpoint must be host:port")?;
        let port: u16 = port.parse().map_err(|_| "bad port")?;
        let model = cfg.resolve("embeddings_model", "SEMDB_MODEL", None).unwrap_or_else(|| "embeddinggemma".into());
        http::fetch_embedding(host, port, &model, text)
    }

    /// Store text in a tier with an auto-embedded vector.
    pub fn remember(&self, tier: &str, id: &str, text: &str) -> Result<(), String> {
        let vector = self.embed(text)?;
        self.remember_vec(tier, id, text, vector)
    }

    /// Store with an explicit vector (used by tests to avoid the network).
    pub fn remember_vec(&self, tier: &str, id: &str, text: &str, vector: Vec<f32>) -> Result<(), String> {
        let mut db = self.open_or_create(tier)?;
        let meta = format!(r#"{{"text":"{}"}}"#, esc(text));
        db.put(id, &meta, vector)?;
        if tier == "working" {
            evict_over_cap(&mut db)?;
        }
        Ok(())
    }

    pub fn forget(&self, tier: &str, id: &str) -> Result<bool, String> {
        let path = self.tier_path(tier);
        if !path.exists() {
            return Ok(false);
        }
        let mut db = Db::open(&path)?;
        db.delete(id)
    }

    /// Promote an entry from one tier to another (copy, then remove source).
    pub fn promote(&self, id: &str, from: &str, to: &str) -> Result<(), String> {
        let from_path = self.tier_path(from);
        let db = Db::open(&from_path)?;
        let entry = db.get(id).ok_or_else(|| format!("id '{id}' not in {from}"))?.clone();
        drop(db);
        {
            let mut dst = self.open_or_create(to)?;
            dst.put(id, &entry.meta, entry.vector)?;
        }
        self.forget(from, id)?;
        Ok(())
    }

    /// Semantic recall across the requested tiers (or all), tagged by tier.
    pub fn recall(&self, query_vec: &[f32], k: usize, tiers: &[String]) -> Result<Vec<Recalled>, String> {
        let mut out = Vec::new();
        for tier in tiers {
            let path = self.tier_path(tier);
            if !path.exists() {
                continue;
            }
            let db = Db::open(&path)?;
            for (id, score, meta) in cli::search(&db, query_vec, k, false) {
                out.push(Recalled { tier: tier.clone(), id, score, text: text_from_meta(&meta) });
            }
        }
        out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(k);
        Ok(out)
    }

    pub fn recall_text(&self, query: &str, k: usize, tiers: &[String]) -> Result<Vec<Recalled>, String> {
        let qv = self.embed(query)?;
        self.recall(&qv, k, tiers)
    }

    /// The N most-recent entries in a tier (ids are timestamp-prefixed, so
    /// lexicographically-highest == newest). Returns (id, text), newest first.
    pub fn recent(&self, tier: &str, n: usize) -> Result<Vec<(String, String)>, String> {
        validate_tier(tier)?;
        let path = self.tier_path(tier);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let db = Db::open(&path)?;
        let mut ids: Vec<String> = db.index.keys().cloned().collect();
        ids.sort();
        ids.reverse();
        let mut out = Vec::new();
        for id in ids.into_iter().take(n) {
            let text = db
                .get(&id)
                .and_then(|e| semdb::json::parse(&e.meta).ok())
                .and_then(|v| v.get("text").and_then(|t| t.as_str().map(str::to_string)))
                .unwrap_or_default();
            out.push((id, text));
        }
        Ok(out)
    }

    pub fn stats(&self) -> Result<Vec<(String, usize)>, String> {
        let mut out = Vec::new();
        for tier in TIERS {
            let path = self.tier_path(tier);
            let n = if path.exists() { Db::open(&path)?.index.len() } else { 0 };
            out.push((tier.to_string(), n));
        }
        Ok(out)
    }
}

pub fn resolve_tiers(spec: &str) -> Result<Vec<String>, String> {
    if spec == "all" {
        return Ok(TIERS.iter().map(|s| s.to_string()).collect());
    }
    validate_tier(spec)?;
    Ok(vec![spec.to_string()])
}

fn validate_tier(tier: &str) -> Result<(), String> {
    if TIERS.contains(&tier) {
        Ok(())
    } else {
        Err(format!("unknown tier '{tier}' (working|episodic|semantic)"))
    }
}

fn evict_over_cap(db: &mut Db) -> Result<(), String> {
    // Keep it simple: if over cap, drop lexicographically-lowest ids until at cap.
    // Callers use timestamp-prefixed ids so lowest == oldest.
    let mut ids: Vec<String> = db.index.keys().cloned().collect();
    if ids.len() <= WORKING_CAP {
        return Ok(());
    }
    ids.sort();
    let excess = ids.len() - WORKING_CAP;
    for id in ids.into_iter().take(excess) {
        db.delete(&id)?;
    }
    Ok(())
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

fn text_from_meta(meta: &str) -> String {
    // meta is {"text":"..."} — pull the value out with the semdb json parser.
    semdb::json::parse(meta)
        .ok()
        .and_then(|v| v.get("text").and_then(|t| t.as_str().map(String::from)))
        .unwrap_or_else(|| meta.to_string())
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
    fn remember_recall_promote() {
        let m = Memory::new(&scratch("mem-basic"));
        // explicit vectors, no network
        m.remember_vec("working", "w1", "golf handicap is 3.5", vec![1.0, 0.0, 0.0]).unwrap();
        m.remember_vec("semantic", "s1", "lives in Akureyri", vec![0.0, 1.0, 0.0]).unwrap();
        // recall closest to the working vector
        let hits = m.recall(&[0.9, 0.1, 0.0], 5, &vec!["working".into(), "semantic".into()]).unwrap();
        assert_eq!(hits[0].id, "w1");
        assert_eq!(hits[0].tier, "working");
        assert!(hits[0].text.contains("golf"));
        // promote w1 → semantic
        m.promote("w1", "working", "semantic").unwrap();
        let stats = m.stats().unwrap();
        assert_eq!(stats.iter().find(|(t, _)| t == "working").unwrap().1, 0);
        assert_eq!(stats.iter().find(|(t, _)| t == "semantic").unwrap().1, 2);
    }

    #[test]
    fn working_cap_evicts_oldest() {
        let m = Memory::new(&scratch("mem-cap"));
        for i in 0..(WORKING_CAP + 5) {
            m.remember_vec("working", &format!("{i:04}"), "x", vec![i as f32]).unwrap();
        }
        let n = m.stats().unwrap().iter().find(|(t, _)| t == "working").unwrap().1;
        assert_eq!(n, WORKING_CAP);
        // oldest (0000) evicted
        let hits = m.recall(&[0.0], WORKING_CAP, &vec!["working".into()]).unwrap();
        assert!(hits.iter().all(|h| h.id != "0000"));
    }
}
