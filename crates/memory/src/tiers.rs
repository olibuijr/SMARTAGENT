//! Three-tier memory (Mem0 concept), each tier a semdb file under a root dir.
//!
//!   working  — recent context, capped (oldest evicted past the cap)
//!   episodic — timestamped events, text capped at store-time
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
const EPISODIC_TEXT_CAP: usize = 500;

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
        let (host, port, model) = Config::load().embeddings(None, None)?;
        http::fetch_embedding(&host, port, &model, text)
    }

    /// Store text in a tier with an auto-embedded vector.
    pub fn remember(&self, tier: &str, id: &str, text: &str) -> Result<(), String> {
        let text = cap_tier_text(tier, text);
        let vector = self.embed(&text)?;
        self.remember_vec(tier, id, &text, vector)
    }

    /// Store with an explicit vector (used by tests to avoid the network).
    /// Dedup-on-write: if a near-identical memory already exists in the tier
    /// (cosine ≥ 0.97), UPDATE that row instead of accumulating a duplicate/
    /// contradiction — the Mem0 consolidation behavior, deterministic.
    pub fn remember_vec(&self, tier: &str, id: &str, text: &str, vector: Vec<f32>) -> Result<(), String> {
        let text = cap_tier_text(tier, text);
        let mut db = self.open_or_create(tier)?;
        let ts = procutil::unix_secs();
        let meta = format!(r#"{{"text":"{}","ts":{ts},"hits":0}}"#, esc(&text));
        if vector.len() > 1 {
            let top = cli::search(&db, &vector, 1, true);
            if let Some((existing, score, _)) = top.first() {
                if *score >= 0.97 && existing != id {
                    let existing = existing.clone();
                    db.put(&existing, &meta, vector)?;
                    return Ok(());
                }
            }
        }
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
        // Sort by the explicit `ts` field (custom/user ids broke the old
        // id-prefix ordering); legacy rows without ts fall back to id order.
        let mut keyed: Vec<(i64, String)> = db
            .index
            .iter()
            .map(|(id, e)| {
                let ts = semdb::json::parse(&e.meta)
                    .ok()
                    .and_then(|v| v.get("ts").and_then(|x| x.as_f64()))
                    .unwrap_or(0.0) as i64;
                (ts, id.clone())
            })
            .collect();
        keyed.sort();
        keyed.reverse();
        let ids: Vec<String> = keyed.into_iter().map(|(_, id)| id).collect();
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
    if db.index.len() <= WORKING_CAP {
        return Ok(());
    }
    // Relevance-aware eviction: score = recall hits weighted, plus recency
    // (explicit `ts` in meta; falls back to 0 for legacy rows so they age out
    // first). Lowest score goes first — not blind FIFO.
    let mut scored: Vec<(i64, String)> = db
        .index
        .iter()
        .map(|(id, e)| {
            let v = semdb::json::parse(&e.meta).ok();
            let ts = v.as_ref().and_then(|v| v.get("ts").and_then(|x| x.as_f64())).unwrap_or(0.0) as i64;
            let hits = v.as_ref().and_then(|v| v.get("hits").and_then(|x| x.as_f64())).unwrap_or(0.0) as i64;
            (ts + hits * 3600, id.clone()) // one recall ≈ an hour of freshness
        })
        .collect();
    scored.sort();
    let excess = db.index.len() - WORKING_CAP;
    for (_, id) in scored.into_iter().take(excess) {
        db.delete(&id)?;
    }
    Ok(())
}

fn cap_tier_text(tier: &str, text: &str) -> String {
    if tier != "episodic" || text.chars().count() <= EPISODIC_TEXT_CAP {
        return text.to_string();
    }
    text.chars().take(EPISODIC_TEXT_CAP).collect()
}

fn esc(s: &str) -> String {
    // Full JSON escaping (\r, \t, control chars) via the shared serializer —
    // the hand-rolled version broke meta on control characters.
    semdb::json::escape(s)
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
    fn rejects_unknown_tier_and_recent_empty() {
        assert!(resolve_tiers("bogus").is_err());
        assert!(resolve_tiers("episodic").is_ok());
        // recent() on a never-written tier is empty, not an error.
        let m = Memory::new(&scratch("mem-recent-empty"));
        assert!(m.recent("episodic", 5).unwrap().is_empty());
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

    #[test]
    fn episodic_text_is_capped_at_store_time() {
        let m = Memory::new(&scratch("mem-episodic-cap"));
        let long = "x".repeat(EPISODIC_TEXT_CAP + 200);
        m.remember_vec("episodic", "e1", &long, vec![1.0]).unwrap();
        let db = Db::open(&m.tier_path("episodic")).unwrap();
        let stored = text_from_meta(&db.get("e1").unwrap().meta);
        assert_eq!(stored.chars().count(), EPISODIC_TEXT_CAP);
    }
}

#[cfg(test)]
mod dedup_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn near_duplicate_updates_instead_of_inserting() {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch/mem-dedup");
        let _ = std::fs::remove_dir_all(&d);
        let m = Memory::new(&d);
        m.remember_vec("semantic", "a", "golf handicap is 3.5", vec![1.0, 0.0, 0.0]).unwrap();
        // near-identical vector → update row "a", not a second row
        m.remember_vec("semantic", "b", "golf handicap is 4.0", vec![0.999, 0.01, 0.0]).unwrap();
        let db = Db::open(&d.join("semantic.semdb")).unwrap();
        assert_eq!(db.index.len(), 1, "duplicate accumulated instead of consolidating");
        assert!(db.get("a").unwrap().meta.contains("4.0"));
        // orthogonal vector → genuinely new memory
        m.remember_vec("semantic", "c", "depill needs a walk", vec![0.0, 1.0, 0.0]).unwrap();
        let db = Db::open(&d.join("semantic.semdb")).unwrap();
        assert_eq!(db.index.len(), 2);
    }
}
