//! Supporting-file management — the `scripts/`/`references/`/`assets/` half
//! of the Agent Skills standard. `SKILL.md` (manage.rs) documents WHEN/HOW;
//! these dirs hold the actual reusable "CLI tool" (`scripts/` — the standard
//! treats a bundled script AS the tool, there is no separate tool/recipe
//! concept) and reference material (`references/`, `assets/`). Write actions
//! mirror manage.rs's validate-then-mutate shape; the path guard mirrors
//! `semdb::workspace::resolve_in`'s traversal guard, extended to a
//! multi-segment relative path.

use std::path::{Path, PathBuf};

use crate::manage;

/// The only subdirectories a skill may hold, per the Agent Skills standard.
const ALLOWED_SUBDIRS: [&str; 3] = ["scripts", "references", "assets"];

/// Resolve `name`'s own directory under `root` (the already-scoped
/// global-or-project skills root). Distinct from `manage::skill_dir`, which
/// computes a *new* skill's target dir from `--category`; this looks up an
/// *existing* skill's real dir via discovery, so it works regardless of
/// whether the skill lives at `<root>/<name>` or `<root>/<category>/<name>`.
pub fn find_skill_dir(root: &Path, name: &str) -> Result<PathBuf, String> {
    let skill_md = manage::find(root, name)?.ok_or_else(|| format!("no skill '{name}'"))?;
    skill_md
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("skill '{name}' has no parent directory"))
}

/// Write `content` to `<skill_dir>/<rel_path>`, creating parent dirs as
/// needed. Scripts (first segment `scripts`) are made executable (0755);
/// everything else is 0644. Returns the full path written.
pub fn write_to(skill_dir: &Path, rel_path: &str, content: &[u8]) -> Result<PathBuf, String> {
    let segments = sanitize_rel_path(rel_path)?;
    let full = join_segments(skill_dir, &segments);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&full, content).map_err(|e| e.to_string())?;
    set_mode(&full, segments[0] == "scripts")?;
    Ok(full)
}

/// Remove `<skill_dir>/<rel_path>`, then prune now-empty subdirs — bounded
/// at `skill_dir` itself (never removed, mirrors `manage::delete`'s pruning
/// which is bounded at the skills root).
pub fn remove_from(skill_dir: &Path, rel_path: &str) -> Result<(), String> {
    let segments = sanitize_rel_path(rel_path)?;
    let full = join_segments(skill_dir, &segments);
    if !full.is_file() {
        return Err(format!("no file '{rel_path}' in this skill"));
    }
    std::fs::remove_file(&full).map_err(|e| e.to_string())?;
    if let Some(parent) = full.parent() {
        manage::remove_if_empty(parent, skill_dir);
    }
    Ok(())
}

/// Read `<skill_dir>/<rel_path>` as text — Level-2 progressive disclosure
/// (`skills show <root> <name> --path scripts/foo.sh`), the standard's
/// `skill_view(name, path)`.
pub fn read_from(skill_dir: &Path, rel_path: &str) -> Result<String, String> {
    let segments = sanitize_rel_path(rel_path)?;
    let full = join_segments(skill_dir, &segments);
    std::fs::read_to_string(&full).map_err(|e| format!("read {}: {e}", full.display()))
}

/// List a skill's supporting files (relative path + byte size) across
/// `scripts/`, `references/`, `assets/` — so an agent can see what's already
/// bundled before re-deriving a command.
pub fn list_in(skill_dir: &Path) -> String {
    let mut out: Vec<(String, u64)> = Vec::new();
    for sub in ALLOWED_SUBDIRS {
        walk_files(&skill_dir.join(sub), sub, &mut out);
    }
    if out.is_empty() {
        return "no supporting files".to_string();
    }
    out.sort();
    out.into_iter()
        .map(|(rel, size)| format!("{rel}\t{size}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn walk_files(dir: &Path, prefix: &str, out: &mut Vec<(String, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let rel = format!("{prefix}/{name}");
        if path.is_dir() {
            walk_files(&path, &rel, out);
        } else if let Ok(meta) = e.metadata() {
            out.push((rel, meta.len()));
        }
    }
}

/// Validate a supporting-file path: relative, forward-slash separated, no
/// empty/`.`/`..` segments, first segment one of `scripts`/`references`/
/// `assets`, and at least a subdir + filename (not the bare subdir). This is
/// the only place a caller-supplied path is turned into a filesystem path —
/// every segment is checked before it ever touches `Path::join`, so the
/// result can never escape `skill_dir`.
fn sanitize_rel_path(rel_path: &str) -> Result<Vec<&str>, String> {
    if rel_path.is_empty() {
        return Err("path must not be empty".into());
    }
    if rel_path.starts_with('/') || rel_path.contains('\\') || rel_path.contains('\0') {
        return Err(format!(
            "path '{rel_path}' must be relative and forward-slash separated"
        ));
    }
    let segments: Vec<&str> = rel_path.split('/').collect();
    if segments.iter().any(|s| s.is_empty() || *s == "." || *s == "..") {
        return Err(format!(
            "path '{rel_path}' must not contain empty, '.', or '..' segments"
        ));
    }
    if segments.len() < 2 {
        return Err(format!(
            "path '{rel_path}' must be a file inside scripts/, references/, or assets/ (e.g. scripts/foo.sh)"
        ));
    }
    if !ALLOWED_SUBDIRS.contains(&segments[0]) {
        return Err(format!(
            "path '{rel_path}' must start with scripts/, references/, or assets/ — got '{}'",
            segments[0]
        ));
    }
    Ok(segments)
}

fn join_segments(base: &Path, segments: &[&str]) -> PathBuf {
    let mut p = base.to_path_buf();
    for s in segments {
        p.push(s);
    }
    p
}

#[cfg(unix)]
fn set_mode(path: &Path, executable: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if executable { 0o755 } else { 0o644 };
    let mut perms = std::fs::metadata(path)
        .map_err(|e| e.to_string())?
        .permissions();
    perms.set_mode(mode);
    std::fs::set_permissions(path, perms).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _executable: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manage;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch")
            .join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_skill(root: &Path, name: &str) {
        manage::create(
            root,
            name,
            None,
            Some("test skill for supporting files"),
            None,
            None,
            "## When to Use\ntesting\n",
        )
        .unwrap();
    }

    #[test]
    fn write_creates_executable_script_and_read_round_trips() {
        let root = scratch("files-write-script");
        make_skill(&root, "curl-retry");
        let dir = find_skill_dir(&root, "curl-retry").unwrap();

        let full = write_to(&dir, "scripts/retry.sh", b"#!/bin/sh\ncurl --retry 3 \"$1\"\n").unwrap();
        assert!(full.ends_with("curl-retry/scripts/retry.sh"), "{full:?}");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&full).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "scripts/ files must be executable");
        }

        let read_back = read_from(&dir, "scripts/retry.sh").unwrap();
        assert!(read_back.contains("curl --retry 3"));
    }

    #[test]
    fn write_reference_file_is_not_executable() {
        let root = scratch("files-write-reference");
        make_skill(&root, "golf-notes");
        let dir = find_skill_dir(&root, "golf-notes").unwrap();
        let full = write_to(&dir, "references/swing-plane.md", b"keep the plane flat").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&full).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o644, "references/ files must not be executable");
        }
    }

    #[test]
    fn rejects_traversal_and_disallowed_subdirs() {
        let root = scratch("files-traversal");
        make_skill(&root, "guarded-skill");
        let dir = find_skill_dir(&root, "guarded-skill").unwrap();

        for bad in [
            "../escape.sh",
            "scripts/../../etc/passwd",
            "/etc/passwd",
            "bin/tool.sh",   // not an allowed subdir
            "scripts",       // no filename, just the subdir itself
            "scripts/../x",  // traversal inside an allowed subdir
        ] {
            let err = write_to(&dir, bad, b"x");
            assert!(err.is_err(), "expected '{bad}' to be rejected, got {err:?}");
        }

        // None of the rejected writes should have escaped or landed anywhere.
        assert!(!root.parent().unwrap().join("escape.sh").exists());
    }

    #[test]
    fn remove_deletes_file_and_prunes_empty_subdir_but_not_skill_dir() {
        let root = scratch("files-remove");
        make_skill(&root, "prune-check");
        let dir = find_skill_dir(&root, "prune-check").unwrap();
        write_to(&dir, "scripts/only.sh", b"#!/bin/sh\necho hi\n").unwrap();
        assert!(dir.join("scripts/only.sh").exists());

        remove_from(&dir, "scripts/only.sh").unwrap();
        assert!(!dir.join("scripts/only.sh").exists());
        assert!(!dir.join("scripts").exists(), "empty scripts/ should be pruned");
        assert!(dir.exists(), "skill dir itself must never be removed");

        let missing = remove_from(&dir, "scripts/only.sh");
        assert!(missing.is_err());
    }

    #[test]
    fn list_in_reports_relative_paths_and_sizes() {
        let root = scratch("files-list");
        make_skill(&root, "listed-skill");
        let dir = find_skill_dir(&root, "listed-skill").unwrap();
        assert_eq!(list_in(&dir), "no supporting files");

        write_to(&dir, "scripts/run.sh", b"#!/bin/sh\ntrue\n").unwrap();
        write_to(&dir, "references/notes.md", b"some notes").unwrap();
        let listing = list_in(&dir);
        assert!(listing.contains("scripts/run.sh\t"), "{listing}");
        assert!(listing.contains("references/notes.md\t"), "{listing}");
    }
}
