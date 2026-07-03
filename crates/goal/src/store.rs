//! Goal state — one active goal per pi session, persisted as semdb rows
//! (all data lives in database tables, per the storage rule). Rows survive
//! across turns; `status` transitions active → achieved | cleared.

use semdb::storage::Db;
use std::path::PathBuf;

const DEFAULT_ROW_ID: &str = "current";
const PLACEHOLDER_VEC: [f32; 1] = [0.0];

#[derive(Clone, Debug)]
pub struct Goal {
    pub condition: String,
    pub created_at: i64,
    pub turns: u32,
    pub last_reason: String,
    pub status: String, // active | achieved | cleared
}

impl Goal {
    pub fn new(condition: &str, now: i64) -> Goal {
        Goal {
            condition: condition.to_string(),
            created_at: now,
            turns: 0,
            last_reason: String::new(),
            status: "active".to_string(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == "active"
    }

    fn to_meta(&self) -> String {
        format!(
            r#"{{"condition":"{}","created_at":{},"turns":{},"last_reason":"{}","status":"{}"}}"#,
            esc(&self.condition),
            self.created_at,
            self.turns,
            esc(&self.last_reason),
            esc(&self.status)
        )
    }

    fn from_meta(meta: &str) -> Option<Goal> {
        let v = httpc::json::parse(meta).ok()?;
        Some(Goal {
            condition: v.get("condition")?.as_str()?.to_string(),
            created_at: v.get("created_at").and_then(|x| x.as_f64()).unwrap_or(0.0) as i64,
            turns: v.get("turns").and_then(|x| x.as_f64()).unwrap_or(0.0) as u32,
            last_reason: v
                .get("last_reason")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            status: v
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("active")
                .to_string(),
        })
    }
}

fn db_path() -> PathBuf {
    semdb::config::Config::load().data_dir().join("goal.semdb")
}

fn open() -> Result<Db, String> {
    let p = db_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    if p.exists() {
        Db::open(&p)
    } else {
        Db::create(&p)
    }
}

/// The current goal for one pi session, or None if none was ever set.
pub fn load_for(session: &str) -> Result<Option<Goal>, String> {
    let db = open()?;
    Ok(db
        .get(&row_id(session))
        .and_then(|e| Goal::from_meta(&e.meta)))
}

pub fn save_for(session: &str, g: &Goal) -> Result<(), String> {
    let mut db = open()?;
    db.put(&row_id(session), &g.to_meta(), PLACEHOLDER_VEC.to_vec())
}

fn row_id(session: &str) -> String {
    let s = session.trim();
    if s.is_empty() || s == DEFAULT_ROW_ID {
        DEFAULT_ROW_ID.to_string()
    } else {
        format!(
            "session:{}",
            s.chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                .collect::<String>()
        )
    }
}

fn esc(s: &str) -> String {
    httpc::json::escape(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_roundtrip_survives_quotes_and_newlines() {
        let g = Goal {
            condition: "tests pass and \"lint\" is clean\nno regressions".into(),
            created_at: 1234,
            turns: 3,
            last_reason: "still 2 failing".into(),
            status: "active".into(),
        };
        let back = Goal::from_meta(&g.to_meta()).unwrap();
        assert_eq!(back.condition, g.condition);
        assert_eq!(back.turns, 3);
        assert_eq!(back.status, "active");
        assert_eq!(back.last_reason, "still 2 failing");
    }
}
