//! Link graph over the vault: outgoing links + backlinks, adjacency dump.

use std::collections::BTreeMap;
use std::path::Path;

use crate::note::{self, note_name};
use crate::wikilink;

/// name -> set of outgoing target names (resolved by slug/name match).
pub type Adjacency = BTreeMap<String, Vec<String>>;

pub fn build(vault: &Path) -> Result<Adjacency, String> {
    let mut adj: Adjacency = BTreeMap::new();
    for path in note::list(vault)? {
        let name = note_name(&path);
        let body = note::strip_frontmatter(&note::read(&path)?).to_string();
        let mut targets: Vec<String> = wikilink::extract(&body)
            .into_iter()
            .map(|l| l.target)
            .collect();
        targets.sort();
        targets.dedup();
        adj.insert(name, targets);
    }
    Ok(adj)
}

/// Outgoing links and backlinks for one note name.
pub fn links(vault: &Path, name: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let adj = build(vault)?;
    let target_key = resolve_key(&adj, name).unwrap_or_else(|| name.to_string());
    let outgoing = adj.get(&target_key).cloned().unwrap_or_default();
    let mut backlinks = Vec::new();
    for (src, tgts) in &adj {
        if tgts.iter().any(|t| matches(t, &target_key)) {
            backlinks.push(src.clone());
        }
    }
    backlinks.sort();
    Ok((outgoing, backlinks))
}

fn resolve_key(adj: &Adjacency, name: &str) -> Option<String> {
    if adj.contains_key(name) {
        return Some(name.to_string());
    }
    let slug = note::slugify(name);
    adj.keys().find(|k| note::slugify(k) == slug).cloned()
}

fn matches(link_target: &str, note_key: &str) -> bool {
    link_target == note_key || note::slugify(link_target) == note::slugify(note_key)
}

pub fn render(adj: &Adjacency) -> String {
    let mut out = String::new();
    for (src, tgts) in adj {
        out.push_str(&format!("{src} -> {}\n", if tgts.is_empty() { "(none)".into() } else { tgts.join(", ") }));
    }
    out.trim_end().to_string()
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
    fn backlinks_and_alias() {
        let v = scratch("vault-graph");
        std::fs::write(v.join("a.md"), "links to [[b]] and [[c|see-c]]").unwrap();
        std::fs::write(v.join("b.md"), "b points [[a]]").unwrap();
        std::fs::write(v.join("c.md"), "c is alone").unwrap();
        let (out_a, back_a) = links(&v, "a").unwrap();
        assert_eq!(out_a, vec!["b", "c"]);
        assert_eq!(back_a, vec!["b"]);
        let (_, back_c) = links(&v, "c").unwrap();
        assert_eq!(back_c, vec!["a"]); // alias link still counts
    }
}
