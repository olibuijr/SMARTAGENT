//! Note file model: path <-> title mapping, frontmatter, read/create/append.

use std::fs;
use std::path::{Path, PathBuf};

pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in title.trim().chars() {
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

pub fn note_path(vault: &Path, name: &str) -> PathBuf {
    // Accept either a bare name or a name.md; resolve by slug match too.
    let direct = vault.join(format!("{name}.md"));
    if direct.exists() {
        return direct;
    }
    let slug = slugify(name);
    let by_slug = vault.join(format!("{slug}.md"));
    if by_slug.exists() {
        return by_slug;
    }
    direct
}

/// The note "name" used for link matching = filename without .md.
pub fn note_name(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

pub fn list(vault: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    walk(vault, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))?;
    for e in entries {
        let e = e.map_err(|x| x.to_string())?;
        let p = e.path();
        if p.is_dir() {
            walk(&p, out)?;
        } else if p.extension().map(|x| x == "md").unwrap_or(false) {
            out.push(p);
        }
    }
    Ok(())
}

pub fn create(vault: &Path, title: &str, date: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(vault).map_err(|e| e.to_string())?;
    let slug = slugify(title);
    if slug.is_empty() {
        return Err("title produced empty slug".into());
    }
    let path = vault.join(format!("{slug}.md"));
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    let body = format!("---\ntitle: {title}\ncreated: {date}\n---\n\n# {title}\n");
    fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))
}

pub fn append(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    writeln!(f, "\n{text}").map_err(|e| e.to_string())
}

/// Strip a leading `---` frontmatter block, returning the body only.
pub fn strip_frontmatter(content: &str) -> &str {
    if let Some(rest) = content.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n") {
            return &rest[end + 5..];
        }
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slugify("Hello World!"), "hello-world");
        assert_eq!(slugify("  A_B  c "), "a-b-c");
        assert_eq!(slugify("Óli's Note"), "óli-s-note");
    }

    #[test]
    fn frontmatter_strip() {
        let c = "---\ntitle: X\n---\nbody here";
        assert_eq!(strip_frontmatter(c), "body here");
        assert_eq!(strip_frontmatter("no fm"), "no fm");
    }
}
