//! semdb-backed semantic symbol search. At index time each symbol is embedded
//! (via the external endpoint, using semdb's http client) and stored in a semdb
//! file beside the graph JSON. `search` embeds a query and recalls the most
//! semantically similar symbols — the "semdb-backed queries" the architecture
//! calls for, complementing the exact-name structural graph.

use std::path::Path;

use semdb::cli;
use semdb::config::Config;
use semdb::http;
use semdb::storage::Db;

use crate::graph::{Graph, Symbol};

fn endpoint() -> Result<(String, u16, String), String> {
    Config::load().embeddings(None, None)
}

/// A short natural-language description of a symbol, so semantic search works
/// on meaning ("config parser") rather than the raw identifier alone.
fn describe(s: &Symbol) -> String {
    format!("{} {} defined in {}", s.kind, s.name, s.file)
}

/// Build the semdb symbol index next to the graph. Overwrites any prior index.
pub fn build_index(graph: &Graph, path: &Path) -> Result<usize, String> {
    let (host, port, model) = endpoint()?;
    let _ = std::fs::remove_file(path);
    let mut db = Db::create(path)?;
    let mut n = 0;
    for (i, s) in graph.symbols.iter().enumerate() {
        if s.file.is_empty() {
            continue; // impl placeholder symbols carry no location
        }
        let desc = describe(s);
        let vector = http::fetch_embedding(&host, port, &model, &desc)?;
        let meta = format!(
            r#"{{"name":"{}","kind":"{}","file":"{}","line":{}}}"#,
            semdb::json::escape(&s.name), semdb::json::escape(&s.kind), semdb::json::escape(&s.file), s.line
        );
        db.put(&format!("sym{i}"), &meta, vector)?;
        n += 1;
    }
    Ok(n)
}

pub struct Found {
    pub score: f32,
    pub meta: String,
}

/// Semantic search over the symbol index.
pub fn search(path: &Path, query: &str, k: usize) -> Result<Vec<Found>, String> {
    if !path.exists() {
        return Err("no symbol index — run `codegraph index ... --embed` first".into());
    }
    let (host, port, model) = endpoint()?;
    let qv = http::fetch_embedding(&host, port, &model, query)?;
    let db = Db::open(path)?;
    Ok(cli::search(&db, &qv, k, false)
        .into_iter()
        .map(|(_, score, meta)| Found { score, meta })
        .collect())
}
