//! CLI: new / read / append / list / links / graph / search

use std::path::Path;

use httpc::args::flag;
use crate::{graph, note, search};

fn today() -> String {
    // Days since epoch → civil date (UTC), std-only, no chrono.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs / 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}")
}

pub fn run(args: &[String]) -> Result<String, String> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let vault = args.get(1).map(Path::new);
    match cmd {
        "new" => {
            let vault = vault.ok_or("usage: vault new <vault> <title>")?;
            let title = args.get(2).ok_or("title required")?;
            let path = note::create(vault, title, &today())?;
            Ok(format!("created {}", path.display()))
        }
        "read" => {
            let vault = vault.ok_or("usage: vault read <vault> <note>")?;
            let name = args.get(2).ok_or("note required")?;
            let body = note::read(&note::note_path(vault, name))?;
            // Head-capped BY DEFAULT (150 lines) — notes grow via append and a
            // full dump can flood agent context. --head 0 = full note.
            let n = flag(args, "--head").and_then(|s| s.parse::<usize>().ok()).unwrap_or(150);
            match n {
                n if n > 0 => {
                    let lines: Vec<&str> = body.lines().collect();
                    let mut out = lines.iter().take(n).cloned().collect::<Vec<_>>().join("\n");
                    if lines.len() > n {
                        out.push_str(&format!("\n…[{n} of {} lines; --head 0 for all]", lines.len()));
                    }
                    Ok(out)
                }
                _ => Ok(body),
            }
        }
        "append" => {
            let vault = vault.ok_or("usage: vault append <vault> <note> <text>")?;
            let name = args.get(2).ok_or("note required")?;
            let text = args.get(3).ok_or("text required")?;
            note::append(&note::note_path(vault, name), text)?;
            Ok(format!("appended to {name}"))
        }
        "list" => {
            let vault = vault.ok_or("usage: vault list <vault>")?;
            let names: Vec<String> = note::list(vault)?.iter().map(|p| note::note_name(p)).collect();
            Ok(if names.is_empty() { "empty vault".into() } else { names.join("\n") })
        }
        "rm" => {
            let vault = vault.ok_or("usage: vault rm <vault> <note>")?;
            let name = args.get(2).ok_or("note required")?;
            let path = note::note_path(vault, name);
            if !path.exists() {
                return Err(format!("no note '{name}'"));
            }
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            Ok(format!("removed {name}"))
        }
        "mv" => {
            let vault = vault.ok_or("usage: vault mv <vault> <old> <new>")?;
            let old = args.get(2).ok_or("old note required")?;
            let new = args.get(3).ok_or("new title required")?;
            let n = rename_note(vault, old, new)?;
            Ok(format!("renamed {old} → {new} ({n} link(s) rewritten)"))
        }
        "links" => {
            let vault = vault.ok_or("usage: vault links <vault> <note>")?;
            let name = args.get(2).ok_or("note required")?;
            let (out, back) = graph::links(vault, name)?;
            Ok(format!(
                "outgoing: {}\nbacklinks: {}",
                if out.is_empty() { "(none)".into() } else { out.join(", ") },
                if back.is_empty() { "(none)".into() } else { back.join(", ") }
            ))
        }
        "graph" => {
            let vault = vault.ok_or("usage: vault graph <vault>")?;
            let adj = graph::build(vault)?;
            // --note X --depth N: scope to the neighborhood of one note.
            match flag(args, "--note") {
                Some(root) => {
                    let depth = flag(args, "--depth").and_then(|s| s.parse().ok()).unwrap_or(1usize);
                    Ok(graph::render_scoped(&adj, &root, depth))
                }
                None => Ok(graph::render(&adj)),
            }
        }
        "orphans" => {
            // Second-brain hygiene: notes with zero backlinks + [[targets]]
            // that resolve to no note (dead links). Reuses the adjacency.
            let vault = vault.ok_or("usage: vault orphans <vault>")?;
            let adj = graph::build(vault)?;
            use std::collections::BTreeSet;
            let notes: BTreeSet<&String> = adj.keys().collect();
            let mut linked: BTreeSet<&String> = BTreeSet::new();
            let mut dead: BTreeSet<String> = BTreeSet::new();
            for (_, targets) in adj.iter() {
                for t in targets {
                    if notes.contains(t) { linked.insert(t); } else { dead.insert(t.clone()); }
                }
            }
            let orphans: Vec<&str> = adj.keys().filter(|n| !linked.contains(*n)).map(|s| s.as_str()).collect();
            let mut out = Vec::new();
            out.push(format!("orphans ({}): {}", orphans.len(), if orphans.is_empty() { "(none)".into() } else { orphans.join(", ") }));
            out.push(format!("dead links ({}): {}", dead.len(), if dead.is_empty() { "(none)".into() } else { dead.iter().cloned().collect::<Vec<_>>().join(", ") }));
            Ok(out.join("\n"))
        }
        "tags" => {
            // List all tags (frontmatter `tags:` + inline #tag) with counts.
            let vault = vault.ok_or("usage: vault tags <vault>")?;
            let counts = collect_tags(vault)?;
            if counts.is_empty() { return Ok("no tags".into()); }
            Ok(counts.iter().map(|(t, n)| format!("{t}\t{n}")).collect::<Vec<_>>().join("\n"))
        }
        "search" => {
            let vault = vault.ok_or("usage: vault search <vault> <query>")?;
            // --tag T: list notes carrying a tag instead of keyword search.
            if let Some(tag) = flag(args, "--tag") {
                let notes = notes_with_tag(vault, &tag)?;
                return Ok(if notes.is_empty() { format!("no notes tagged #{tag}") } else { notes.join("\n") });
            }
            let query = args.get(2).ok_or("query required")?;
            let hits = search::search(vault, query)?;
            if hits.is_empty() {
                return Ok("no matches".into());
            }
            Ok(hits.iter().map(|h| format!("{}\t{}", h.count, h.name)).collect::<Vec<_>>().join("\n"))
        }
        _ => Ok(HELP.trim().into()),
    }
}

const HELP: &str = r#"
vault — markdown second brain (Obsidian pattern)

USAGE:
  vault new    <vault> <title>
  vault read   <vault> <note>
  vault append <vault> <note> <text>
  vault rm     <vault> <note>
  vault mv     <vault> <old> <new>          (rewrites [[old]] links)
  vault list   <vault>
  vault links  <vault> <note>
  vault graph  <vault>
  vault search <vault> <query>
"#;

#[cfg(test)]
mod neg_tests {
    use super::*;

    #[test]
    fn rejects_bad_args() {
        let s=|v:&[&str]|v.iter().map(|x|x.to_string()).collect::<Vec<_>>();
        assert!(run(&s(&["new"])).is_err());          // missing vault
        assert!(run(&s(&["new",".scratch/negtest-vault"])).is_err()); // missing title
        assert!(run(&s(&["read",".scratch/negtest-vault"])).is_err()); // missing note
        assert!(run(&s(&["append",".scratch/negtest-vault","n"])).is_err()); // missing text
    }

}

/// Rename a note file and rewrite `[[old]]` wikilinks across the vault to the
/// new name. Returns the count of links rewritten.
fn rename_note(vault: &Path, old: &str, new_title: &str) -> Result<usize, String> {
    let old_path = note::note_path(vault, old);
    if !old_path.exists() {
        return Err(format!("no note '{old}'"));
    }
    let old_name = note::note_name(&old_path);
    let new_slug = note::slugify(new_title);
    if new_slug.is_empty() {
        return Err("new title produced empty slug".into());
    }
    let new_path = vault.join(format!("{new_slug}.md"));
    if new_path.exists() {
        return Err(format!("{} already exists", new_path.display()));
    }
    std::fs::rename(&old_path, &new_path).map_err(|e| e.to_string())?;
    // Rewrite every wikilink form referencing old_name: [[old]], [[old|alias]],
    // [[old#anchor]], and embeds ![[old]] — the bare-form-only rewrite silently
    // broke aliased/anchored links.
    let mut rewritten = 0;
    for p in note::list(vault)? {
        let body = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let (new_body, n) = rewrite_links(&body, &old_name, &new_slug);
        if n > 0 {
            rewritten += n;
            std::fs::write(&p, new_body).map_err(|e| e.to_string())?;
        }
    }
    Ok(rewritten)
}

/// Replace `[[old...]]` targets with `new`, preserving `|alias` and `#anchor`
/// suffixes and the embed `!` prefix. Returns (rewritten body, count).
fn rewrite_links(body: &str, old: &str, new: &str) -> (String, usize) {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    let mut count = 0;
    while let Some(start) = rest.find("[[") {
        let (head, tail) = rest.split_at(start);
        out.push_str(head);
        let Some(end) = tail.find("]]") else { out.push_str(tail); return (out, count) };
        let inner = &tail[2..end];
        // target = up to first '|' or '#'
        let cut = inner.find(['|', '#']).unwrap_or(inner.len());
        let (target, suffix) = inner.split_at(cut);
        if link_target_matches(target, old) {
            out.push_str(&format!("[[{new}{suffix}]]"));
            count += 1;
        } else {
            out.push_str(&format!("[[{inner}]]"));
        }
        rest = &tail[end + 2..];
    }
    out.push_str(rest);
    (out, count)
}

fn link_target_matches(target: &str, old: &str) -> bool {
    let target = target.trim();
    target == old || note::slugify(target) == note::slugify(old)
}

#[cfg(test)]
mod rename_tests {
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
    fn rewrite_links_matches_slug_case_punctuation_and_preserves_suffixes() {
        let body = "[[My Note]] [[my-note]] [[MY note|Alias]] [[my note#part]] [[other]]";
        let (rewritten, n) = rewrite_links(body, "my-note", "new-note");
        assert_eq!(n, 4);
        assert_eq!(
            rewritten,
            "[[new-note]] [[new-note]] [[new-note|Alias]] [[new-note#part]] [[other]]"
        );
    }

    #[test]
    fn rename_note_rewrites_slug_equivalent_wikilinks() {
        let v = scratch("vault-rename-slug-links");
        std::fs::write(v.join("my-note.md"), "body").unwrap();
        std::fs::write(
            v.join("index.md"),
            "A [[My Note]] B [[my-note|Alias]] C [[my note#anchor]] D [[other]]",
        )
        .unwrap();

        let n = rename_note(&v, "my-note", "New Note").unwrap();
        assert_eq!(n, 3);
        assert!(v.join("new-note.md").exists());
        let body = std::fs::read_to_string(v.join("index.md")).unwrap();
        assert_eq!(
            body,
            "A [[new-note]] B [[new-note|Alias]] C [[new-note#anchor]] D [[other]]"
        );
    }
}

/// Tags = frontmatter `tags:` (comma/space separated) + inline `#tag` tokens.
fn collect_tags(vault: &Path) -> Result<Vec<(String, usize)>, String> {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for p in note::list(vault)? {
        for tag in note_tags(&std::fs::read_to_string(&p).map_err(|e| e.to_string())?) {
            *counts.entry(tag).or_default() += 1;
        }
    }
    Ok(counts.into_iter().collect())
}

fn notes_with_tag(vault: &Path, tag: &str) -> Result<Vec<String>, String> {
    let want = tag.trim_start_matches('#').to_lowercase();
    let mut out = Vec::new();
    for p in note::list(vault)? {
        let body = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        if note_tags(&body).contains(&want) {
            out.push(note::note_name(&p));
        }
    }
    Ok(out)
}

fn note_tags(content: &str) -> Vec<String> {
    let mut tags = Vec::new();
    // frontmatter `tags: a, b c`
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            for line in rest[..end].lines() {
                if let Some(v) = line.strip_prefix("tags:") {
                    for t in v.split([',', ' ']).map(str::trim).filter(|t| !t.is_empty()) {
                        tags.push(t.trim_start_matches('#').to_lowercase());
                    }
                }
            }
        }
    }
    // inline #tag (word chars/dash), skipping code fences and md headings
    let mut in_fence = false;
    for line in note::strip_frontmatter(content).lines() {
        let lt = line.trim_start();
        if lt.starts_with("```") || lt.starts_with("~~~") { in_fence = !in_fence; continue; }
        if in_fence || lt.starts_with('#') && lt.chars().take_while(|c| *c == '#').count() >= 1 && lt.contains(' ') && lt.starts_with('#') && lt.split_whitespace().next().map(|w| w.chars().all(|c| c == '#')).unwrap_or(false) { continue; }
        let chars = line.char_indices().peekable();
        for (i, c) in chars {
            if c == '#' && (i == 0 || !line[..i].ends_with(|p: char| p.is_alphanumeric())) {
                let tag: String = line[i + 1..].chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
                if tag.len() > 1 && tag.chars().any(|c| c.is_alphabetic()) {
                    tags.push(tag.to_lowercase());
                }
            }
        }
    }
    tags.sort();
    tags.dedup();
    tags
}
