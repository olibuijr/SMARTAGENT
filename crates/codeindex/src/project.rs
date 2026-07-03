//! Workspace project discovery + per-project persistent file index.
//!
//! Every direct child directory of `workspaces_dir` (config) is a project.
//! A project's index is a semdb table at `<project>/.smartagent/codeindex.semdb`
//! — one row per indexable file (id = relative path, meta = size/lines/ext,
//! placeholder vector) plus one summary row. Structural rows only: no
//! embeddings, so indexing needs no network.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

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
    let tmp_path = temp_index_path(&db_path);
    if tmp_path.exists() {
        std::fs::remove_file(&tmp_path).map_err(|e| e.to_string())?;
    }
    let mut db = Db::create(&tmp_path)?;
    let mut rows: Vec<(String, String, Vec<f32>)> = Vec::with_capacity(files.len() + 1);
    let mut bytes = 0u64;
    for f in &files {
        let rel = f
            .strip_prefix(project)
            .unwrap_or(f)
            .to_string_lossy()
            .to_string();
        let size = std::fs::metadata(f).map(|m| m.len()).unwrap_or(0);
        let lines = if size <= MAX_LINECOUNT_BYTES {
            std::fs::read_to_string(f)
                .map(|s| s.lines().count())
                .unwrap_or(0)
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
    drop(db);
    if std::env::var("SMARTAGENT_CODEINDEX_FAIL_BEFORE_REPLACE").as_deref() == Ok("1") {
        let _ = std::fs::remove_file(&tmp_path);
        return Err("simulated codeindex failure before replace".into());
    }
    std::fs::rename(&tmp_path, &db_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("replace {}: {e}", db_path.display())
    })?;
    Ok(IndexStats {
        files: count,
        bytes,
    })
}

fn temp_index_path(db_path: &Path) -> PathBuf {
    let mut name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("codeindex.semdb")
        .to_string();
    name.push_str(".tmp");
    db_path.with_file_name(name)
}

pub struct Status {
    pub files: usize,
    pub age_secs: u64,
    pub stale: bool,
}

/// Index status for a project: `Some((file_count, age_secs))` if indexed.
pub fn status(project: &Path) -> Option<(usize, u64)> {
    status_detail(project).map(|s| (s.files, s.age_secs))
}

/// Detailed index status including drift: stale when any indexable repo file is
/// newer than the index db. This catches single-file edits immediately without
/// forcing a reindex from the statusline path.
pub fn status_detail(project: &Path) -> Option<Status> {
    let p = project.join(INDEX_REL);
    let db = Db::open(&p).ok()?;
    let index_mtime = std::fs::metadata(&p).ok()?.modified().ok()?;
    let files = db.index.len().saturating_sub(1); // minus summary row
    let age_secs = SystemTime::now()
        .duration_since(index_mtime)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stale = newest_indexable_mtime(project)
        .map(|t| t > index_mtime)
        .unwrap_or(false);
    Some(Status {
        files,
        age_secs,
        stale,
    })
}

fn newest_indexable_mtime(project: &Path) -> Option<SystemTime> {
    let rules = Rules::load(project);
    walk::walk(project, &rules, None)
        .into_iter()
        .filter_map(|p| std::fs::metadata(p).ok()?.modified().ok())
        .max()
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

    // Serializes the two tests that toggle the process-global fail-injection env
    // var, so it can never leak into a concurrent index_project() call (that race
    // made index_status_reindex_roundtrip flaky-panic under parallel test runs).
    static REINDEX_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn failed_reindex_keeps_previous_index_readable() {
        let _env = REINDEX_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let ws = scratch("ci-project-failed-reindex");
        let proj = ws.join("demo");
        std::fs::create_dir_all(proj.join("src")).unwrap();
        std::fs::write(
            proj.join("src/main.rs"),
            "fn main() {}
",
        )
        .unwrap();

        let first = index_project(&proj).unwrap();
        assert_eq!(first.files, 1);
        let before = proj.join(INDEX_REL);
        assert!(Db::open(&before).unwrap().get("src/main.rs").is_some());

        std::fs::write(
            proj.join("src/lib.rs"),
            "pub fn lib() {}
",
        )
        .unwrap();
        std::env::set_var("SMARTAGENT_CODEINDEX_FAIL_BEFORE_REPLACE", "1");
        let failed = match index_project(&proj) {
            Ok(_) => panic!("simulated failure unexpectedly succeeded"),
            Err(e) => e,
        };
        std::env::remove_var("SMARTAGENT_CODEINDEX_FAIL_BEFORE_REPLACE");
        assert!(failed.contains("simulated codeindex failure"), "{failed}");

        let db = Db::open(&before).unwrap();
        assert!(db.get("src/main.rs").is_some());
        assert!(
            db.get("src/lib.rs").is_none(),
            "failed rebuild must not replace old index"
        );
        assert_eq!(status(&proj).unwrap().0, 1);
        assert!(!temp_index_path(&before).exists());
    }

    #[test]
    fn index_status_reindex_roundtrip() {
        let _env = REINDEX_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
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
        assert!(!status_detail(&proj).unwrap().stale);

        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(
            proj.join("src/main.rs"),
            "fn main() { println!(\"hi\"); }\n",
        )
        .unwrap();
        assert!(
            status_detail(&proj).unwrap().stale,
            "edited file newer than index must stale status"
        );

        // Re-index must not swallow its own .smartagent db as a file.
        let s2 = index_project(&proj).unwrap();
        assert_eq!(s2.files, 3);
        assert!(!status_detail(&proj).unwrap().stale);
    }

    // Discovery/resolution tests live with the shared impl: semdb::workspace.
}
