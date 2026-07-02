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
            // --head N: first N lines only (notes grow via append; don't dump all).
            match flag(args, "--head").and_then(|s| s.parse::<usize>().ok()) {
                Some(n) => {
                    let lines: Vec<&str> = body.lines().collect();
                    let mut out = lines.iter().take(n).cloned().collect::<Vec<_>>().join("\n");
                    if lines.len() > n {
                        out.push_str(&format!("\n…[{n} of {} lines; omit --head for all]", lines.len()));
                    }
                    Ok(out)
                }
                None => Ok(body),
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
            Ok(graph::render(&graph::build(vault)?))
        }
        "search" => {
            let vault = vault.ok_or("usage: vault search <vault> <query>")?;
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
    // Rewrite [[old_name]] → [[new_slug]] in every note.
    let old_link = format!("[[{old_name}]]");
    let new_link = format!("[[{new_slug}]]");
    let mut rewritten = 0;
    for p in note::list(vault)? {
        let body = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        if body.contains(&old_link) {
            rewritten += body.matches(&old_link).count();
            std::fs::write(&p, body.replace(&old_link, &new_link)).map_err(|e| e.to_string())?;
        }
    }
    Ok(rewritten)
}
