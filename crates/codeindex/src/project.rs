//! Workspace project discovery + per-project persistent file index.
//!
//! Every direct child directory of `workspaces_dir` (config) is a project.
//! A project's index is a semdb table at `<project>/.smartagent/codeindex.semdb`
//! — one row per indexable file (id = relative path, meta = size/lines/ext,
//! placeholder vector) plus one summary row. Structural rows only: no
//! embeddings, so indexing needs no network.

use std::path::Path;

use semdb::storage::Db;
// Project discovery/resolution is the shared semdb::workspace mechanism; this
// module keeps only what is codeindex-specific (building/reading the index).
pub use semdb::workspace::{list, resolve_in as resolve, root as workspaces_root, Project};

use crate::gitignore::Rules;
use crate::walk;

pub const INDEX_REL: &str = ".smartagent/codeindex.semdb";
const SUMMARY_ID: &str = "__codeindex_summary__";
/// Files larger than this get size-only rows (no line count) — reading huge
/// blobs for a line count wastes index time on artifacts.
const MAX_LINECOUNT_BYTES: u64 = 2_000_000;

pub struct IndexStats {
    pub files: usize,
    pub bytes: u64,
}

/// (Re)build the file index for one project. Fresh rebuild each run: the old
/// table is replaced, so deleted files never linger as stale rows.
pub fn index_project(project: &Path) -> Result<IndexStats, String> {
    let rules = Rules::load(project);
    let files = walk::walk(project, &rules, None);
    if files.is_empty() {
        return Err("no indexable files (empty or fully ignored)".into());
    }
    let db_path = project.join(INDEX_REL);
    if let Some(dir) = db_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if db_path.exists() {
        std::fs::remove_file(&db_path).map_err(|e| e.to_string())?;
    }
    let mut db = Db::create(&db_path)?;
    let mut rows: Vec<(String, String, Vec<f32>)> = Vec::with_capacity(files.len() + 1);
    let mut bytes = 0u64;
    for f in &files {
        let rel = f.strip_prefix(project).unwrap_or(f).to_string_lossy().to_string();
        let size = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        let lines = if size <= MAX_LINECOUNT_BYTES {
            std::fs::read_to_string(f).map(|s| s.lines().count()).unwrap_or(0)
        } else {
            0
        };
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("");
        let meta = format!(
            "{{\"size\":{size},\"lines\":{lines},\"ext\":\"{}\"}}",
            semdb::json::escape(ext)
        );
        rows.push((rel, meta, vec![0.0]));
        bytes += size;
    }
    let count = rows.len();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rows.push((
        SUMMARY_ID.to_string(),
        format!("{{\"files\":{count},\"bytes\":{bytes},\"indexed_at\":{now}}}"),
        vec![0.0],
    ));
    db.put_many(&rows)?;
    Ok(IndexStats { files: count, bytes })
}

/// Index status for a project: `Some((file_count, age_secs))` if indexed.
pub fn status(project: &Path) -> Option<(usize, u64)> {
    let p = project.join(INDEX_REL);
    let db = Db::open(&p).ok()?;
    let files = db.index.len().saturating_sub(1); // minus summary row
    let age = std::fs::metadata(&p)
        .ok()?
        .modified()
        .ok()
        .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((files, age))
}

pub fn human_age(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86_399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch")
            .join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn index_status_reindex_roundtrip() {
        let ws = scratch("ci-project");
        let proj = ws.join("demo");
        std::fs::create_dir_all(proj.join("src")).unwrap();
        std::fs::create_dir_all(proj.join("dist")).unwrap();
        std::fs::write(proj.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(proj.join("README.md"), "# demo\n").unwrap();
        std::fs::write(proj.join("dist/out.js"), "junk").unwrap();
        std::fs::write(proj.join(".gitignore"), "dist/\n").unwrap();

        let s = index_project(&proj).unwrap();
        assert_eq!(s.files, 3); // main.rs + README.md + .gitignore; dist/ ignored
        let (files, _age) = status(&proj).unwrap();
        assert_eq!(files, 3);

        // Re-index must not swallow its own .smartagent db as a file.
        let s2 = index_project(&proj).unwrap();
        assert_eq!(s2.files, 3);
    }

    // Discovery/resolution tests live with the shared impl: semdb::workspace.
}
