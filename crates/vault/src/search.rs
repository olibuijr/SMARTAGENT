//! Keyword search across the vault: case-insensitive, ranked by hit count.

use std::path::Path;

use crate::note::{self, note_name};

pub struct Hit {
    pub name: String,
    pub count: usize,
}

pub fn search(vault: &Path, query: &str) -> Result<Vec<Hit>, String> {
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return Err("empty query".into());
    }
    let mut hits = Vec::new();
    for path in note::list(vault)? {
        let body = note::read(&path)?.to_lowercase();
        let count = count_occurrences(&body, &needle);
        if count > 0 {
            hits.push(Hit { name: note_name(&path), count });
        }
    }
    hits.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    Ok(hits)
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        count += 1;
        start += pos + needle.len();
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn ranks_by_hits() {
        let v = scratch("vault-search");
        std::fs::write(v.join("many.md"), "golf golf GOLF swing").unwrap();
        std::fs::write(v.join("few.md"), "one Golf here").unwrap();
        std::fs::write(v.join("none.md"), "tennis").unwrap();
        let hits = search(&v, "golf").unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].name, "many");
        assert_eq!(hits[0].count, 3);
        assert_eq!(hits[1].name, "few");
    }
}
