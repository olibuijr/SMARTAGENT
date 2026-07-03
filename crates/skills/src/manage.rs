//! Write actions for agent-authored skills — the "self-creating skills"
//! capability (ported from Hermes Agent's `skill_manage` tool): create /
//! patch / edit / delete a SKILL.md so agents can save reusable procedures
//! as they work ("procedural memory" — skills are multi-step how-tos loaded
//! when relevant; `memory` is small facts always in context). Read-only
//! discovery stays in registry.rs; this module owns validation + filesystem
//! mutation so cli.rs stays a thin dispatcher.

use std::path::{Path, PathBuf};

use crate::frontmatter;
use crate::registry;

/// Hermes cap: self-authored descriptions stay short enough that `skills
/// list`/`match` output doesn't blow up in the agent's own context.
const MAX_DESC_CHARS: usize = 100;

/// Create a new skill under `root` (or `<root>/<category>` when given).
/// Rejects a name collision — use `edit`/`patch` on an existing skill.
pub fn create(
    root: &Path,
    name: &str,
    category: Option<&str>,
    desc: Option<&str>,
    body: &str,
) -> Result<String, String> {
    validate_name(name)?;
    if let Some(existing) = find(root, name)? {
        return Err(format!(
            "skill '{name}' already exists at {} — use `edit`/`patch` instead",
            existing.display()
        ));
    }
    let content = build_content(body, name, desc, category)?;
    validate_content(&content)?;
    let dir = skill_dir(root, name, category)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("SKILL.md");
    std::fs::write(&path, &content).map_err(|e| e.to_string())?;
    Ok(format!("created '{name}' at {}", path.display()))
}

/// Token-efficient targeted edit: exact-string replace inside an existing
/// skill's SKILL.md. Errors if `old` is absent or not unique (ambiguous).
pub fn patch(root: &Path, name: &str, old: &str, new: &str) -> Result<String, String> {
    if old.is_empty() {
        return Err("--old must not be empty".into());
    }
    let path = find(root, name)?.ok_or_else(|| format!("no skill '{name}'"))?;
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let hits = content.matches(old).count();
    if hits == 0 {
        return Err(format!("old_string not found in '{name}'"));
    }
    if hits > 1 {
        return Err(format!(
            "old_string matches {hits} times in '{name}' — add more context to make it unique"
        ));
    }
    let patched = content.replacen(old, new, 1);
    validate_content(&patched)?;
    std::fs::write(&path, &patched).map_err(|e| e.to_string())?;
    Ok(format!("patched '{name}' ({})", path.display()))
}

/// Full-content rewrite of an existing skill's SKILL.md.
pub fn edit(root: &Path, name: &str, body: &str, desc: Option<&str>) -> Result<String, String> {
    let path = find(root, name)?.ok_or_else(|| format!("no skill '{name}'"))?;
    let content = build_content(body, name, desc, None)?;
    validate_content(&content)?;
    std::fs::write(&path, &content).map_err(|e| e.to_string())?;
    Ok(format!("edited '{name}' ({})", path.display()))
}

/// Remove a skill's SKILL.md and prune the now-empty skill/category dirs.
pub fn delete(root: &Path, name: &str) -> Result<String, String> {
    let path = find(root, name)?.ok_or_else(|| format!("no skill '{name}'"))?;
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        remove_if_empty(dir, root);
    }
    Ok(format!("deleted '{name}'"))
}

/// Locate a skill by its frontmatter `name` under `root`. `root` not existing
/// yet is not an error — a project's first self-created skill has no dir.
fn find(root: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    if !root.exists() {
        return Ok(None);
    }
    Ok(registry::discover(root)?
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.path))
}

fn skill_dir(root: &Path, name: &str, category: Option<&str>) -> Result<PathBuf, String> {
    match category.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => Ok(root.join(sanitize_segment(c)?).join(name)),
        None => Ok(root.join(name)),
    }
}

/// Guard against a category value escaping `root` (mirrors
/// `semdb::workspace::resolve_in`'s traversal guard).
fn sanitize_segment(s: &str) -> Result<String, String> {
    if s.contains('/') || s.contains('\\') || s.contains("..") {
        return Err(format!("invalid category '{s}'"));
    }
    Ok(s.to_string())
}

fn validate_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.ends_with('-')
        && !name.contains("--");
    if ok {
        Ok(())
    } else {
        Err(format!(
            "name '{name}' must be kebab-case: lowercase letters/digits/hyphens, start with a letter, no leading/trailing/double hyphens"
        ))
    }
}

/// Build the final SKILL.md text. If `body` already carries valid
/// frontmatter (non-empty `name` + `description`), it is used as-is;
/// otherwise frontmatter is synthesized from `name`/`desc` — Hermes: "if
/// body is provided without frontmatter, synthesize frontmatter from
/// --name/--desc".
fn build_content(
    body: &str,
    name: &str,
    desc: Option<&str>,
    category: Option<&str>,
) -> Result<String, String> {
    let fm = frontmatter::parse(body);
    let has_frontmatter = body.starts_with("---");
    let existing_name = fm.get("name").map(str::trim).filter(|s| !s.is_empty());
    let existing_desc = fm
        .get("description")
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if has_frontmatter && existing_name.is_some() && existing_desc.is_some() {
        return Ok(normalize_trailing_newline(body));
    }
    let final_name = existing_name.unwrap_or(name);
    let final_desc = desc
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(existing_desc)
        .ok_or("description required: pass --desc or include it in the frontmatter")?;
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("name: {final_name}\n"));
    out.push_str(&format!("description: {final_desc}\n"));
    if let Some(c) = category.map(str::trim).filter(|c| !c.is_empty()) {
        out.push_str(&format!("category: {c}\n"));
    }
    out.push_str("---\n");
    let rest = if has_frontmatter {
        fm.body.as_str()
    } else {
        body
    };
    out.push_str(rest.trim_start_matches('\n'));
    Ok(normalize_trailing_newline(&out))
}

fn normalize_trailing_newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// Spec compliance for self-authored skills: `name` + `description` present,
/// description no longer than `MAX_DESC_CHARS` (Hermes rule — self-created
/// procedures stay terse).
fn validate_content(content: &str) -> Result<(), String> {
    let fm = frontmatter::parse(content);
    let name = fm.get("name").unwrap_or("").trim();
    let desc = fm.get("description").unwrap_or("").trim();
    if name.is_empty() {
        return Err("frontmatter missing 'name'".into());
    }
    if desc.is_empty() {
        return Err("frontmatter missing 'description'".into());
    }
    let len = desc.chars().count();
    if len > MAX_DESC_CHARS {
        return Err(format!(
            "description is {len} chars, must be ≤{MAX_DESC_CHARS} (self-authored skills stay terse)"
        ));
    }
    Ok(())
}

/// Remove `dir`, then walk upward removing now-empty ancestor dirs — stops
/// at (never removes) `root` itself.
fn remove_if_empty(mut dir: &Path, root: &Path) {
    while dir != root {
        let is_empty = std::fs::read_dir(dir)
            .map(|mut e| e.next().is_none())
            .unwrap_or(false);
        if !is_empty || std::fs::remove_dir(dir).is_err() {
            break;
        }
        dir = match dir.parent() {
            Some(p) => p,
            None => break,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch")
            .join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn create_synthesizes_frontmatter_and_rejects_duplicate() {
        let root = scratch("manage-create");
        let msg = create(
            &root,
            "deploy-drill",
            None,
            Some("Roll out a build and verify it landed"),
            "## When to Use\nAfter any deploy.\n",
        )
        .unwrap();
        assert!(msg.contains("created 'deploy-drill'"), "{msg}");
        let written = std::fs::read_to_string(root.join("deploy-drill/SKILL.md")).unwrap();
        assert!(written.starts_with("---\nname: deploy-drill\n"));
        assert!(written.contains("description: Roll out a build and verify it landed"));
        assert!(written.contains("## When to Use"));

        let dup = create(&root, "deploy-drill", None, Some("again"), "body");
        assert!(dup.is_err());
        assert!(dup.unwrap_err().contains("already exists"));
    }

    #[test]
    fn create_honors_existing_frontmatter_and_category_dir() {
        let root = scratch("manage-create-fm");
        let body = "---\nname: golf-swing\ndescription: fix a slice\n---\n## Procedure\ndo it\n";
        create(&root, "golf-swing", Some("sports"), None, body).unwrap();
        let path = root.join("sports/golf-swing/SKILL.md");
        assert!(path.exists(), "expected {}", path.display());
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("## Procedure"));
    }

    #[test]
    fn create_rejects_missing_description_and_bad_name() {
        let root = scratch("manage-create-invalid");
        let no_desc = create(&root, "no-desc", None, None, "body only");
        assert!(no_desc.is_err());

        let bad_name = create(&root, "Not_Kebab", None, Some("d"), "body");
        assert!(bad_name.is_err());

        let too_long = create(&root, "too-long", None, Some(&"x".repeat(101)), "body");
        assert!(too_long.unwrap_err().contains("must be"));
    }

    #[test]
    fn patch_requires_unique_match_then_rewrites() {
        let root = scratch("manage-patch");
        create(
            &root,
            "curl-retry",
            None,
            Some("retry a flaky curl call"),
            "## Procedure\nrun curl once\nrun curl once more\n",
        )
        .unwrap();

        // ambiguous: "run curl once" occurs twice (once, and as a substring
        // of "run curl once more").
        let ambiguous = patch(&root, "curl-retry", "run curl once", "run curl twice");
        assert!(ambiguous.is_err());
        assert!(ambiguous.unwrap_err().contains("unique"));

        let missing = patch(&root, "curl-retry", "does not appear", "x");
        assert!(missing.is_err());

        let ok = patch(
            &root,
            "curl-retry",
            "## Procedure\n",
            "## Procedure\nretry with backoff\n",
        );
        assert!(ok.is_ok(), "{:?}", ok);
        let written = std::fs::read_to_string(root.join("curl-retry/SKILL.md")).unwrap();
        assert!(written.contains("retry with backoff"));
    }

    #[test]
    fn edit_rewrites_full_body_and_delete_prunes_dirs() {
        let root = scratch("manage-edit-delete");
        create(
            &root,
            "release-checklist",
            Some("ops"),
            Some("ship a release safely"),
            "## Procedure\nold steps\n",
        )
        .unwrap();

        edit(
            &root,
            "release-checklist",
            "## Procedure\nnew steps\n## Verification\ncheck it\n",
            Some("ship a release safely, v2"),
        )
        .unwrap();
        let path = root.join("ops/release-checklist/SKILL.md");
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("new steps"));
        assert!(written.contains("v2"));

        delete(&root, "release-checklist").unwrap();
        assert!(!path.exists());
        assert!(
            !path.parent().unwrap().exists(),
            "skill dir should be pruned"
        );
        assert!(
            !root.join("ops").exists(),
            "now-empty category dir should be pruned"
        );
        assert!(root.exists(), "root itself must never be removed");
    }

    #[test]
    fn delete_missing_skill_errors() {
        let root = scratch("manage-delete-missing");
        let err = delete(&root, "ghost").unwrap_err();
        assert!(err.contains("no skill"));
    }
}
