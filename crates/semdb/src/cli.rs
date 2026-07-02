//! Command-line interface: create / put / get / del / search / embed / stats / compact.

use std::path::Path;

use crate::hnsw::Hnsw;
use crate::http;
use crate::storage::Db;
use crate::vector;

const DEFAULT_ENDPOINT: &str = "100.88.0.2:8081";
const DEFAULT_MODEL: &str = "embeddinggemma";

pub fn run(args: &[String]) -> Result<String, String> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    match cmd {
        "create" => {
            let db_path = required(args, 1, "db path")?;
            Db::create(Path::new(&db_path))?;
            Ok(format!("created {db_path}"))
        }
        "put" => {
            let db_path = required(args, 1, "db path")?;
            let id = flag(args, "--id").ok_or("--id required")?;
            let meta = flag(args, "--meta").unwrap_or_default();
            let vec_str = flag(args, "--vector").ok_or("--vector required")?;
            let vec = vector::parse_vec(&vec_str)?;
            let mut db = Db::open(Path::new(&db_path))?;
            db.put(&id, &meta, vec)?;
            Ok(format!("put {id}"))
        }
        "get" => {
            let db_path = required(args, 1, "db path")?;
            let id = flag(args, "--id").ok_or("--id required")?;
            let db = Db::open(Path::new(&db_path))?;
            match db.get(&id) {
                Some(e) => Ok(format!(
                    "id: {id}\ndim: {}\nmeta: {}",
                    e.vector.len(),
                    if e.meta.is_empty() { "(none)" } else { &e.meta }
                )),
                None => Err(format!("id '{id}' not found")),
            }
        }
        "del" => {
            let db_path = required(args, 1, "db path")?;
            let id = flag(args, "--id").ok_or("--id required")?;
            let mut db = Db::open(Path::new(&db_path))?;
            if db.delete(&id)? {
                Ok(format!("deleted {id}"))
            } else {
                Err(format!("id '{id}' not found"))
            }
        }
        "search" => {
            let db_path = required(args, 1, "db path")?;
            let k: usize = flag(args, "--k").and_then(|s| s.parse().ok()).unwrap_or(10);
            let exact = args.iter().any(|a| a == "--exact");
            let db = Db::open(Path::new(&db_path))?;
            let query = if let Some(vs) = flag(args, "--vector") {
                vector::parse_vec(&vs)?
            } else if let Some(text) = flag(args, "--text") {
                let (host, port) = endpoint(args)?;
                http::fetch_embedding(&host, port, &model(args), &text)?
            } else {
                return Err("--vector or --text required".into());
            };
            let results = search(&db, &query, k, exact);
            if results.is_empty() {
                return Ok("no results".into());
            }
            Ok(results
                .into_iter()
                .map(|(id, score, meta)| format!("{score:.4}\t{id}\t{meta}"))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "embed" => {
            let db_path = required(args, 1, "db path")?;
            let id = flag(args, "--id").ok_or("--id required")?;
            let text = flag(args, "--text").ok_or("--text required")?;
            let meta = flag(args, "--meta").unwrap_or_else(|| {
                format!(r#"{{"text":"{}"}}"#, crate::json::escape(&text))
            });
            let (host, port) = endpoint(args)?;
            let vec = http::fetch_embedding(&host, port, &model(args), &text)?;
            let dim = vec.len();
            let mut db = Db::open(Path::new(&db_path))?;
            db.put(&id, &meta, vec)?;
            Ok(format!("embedded {id} (dim {dim})"))
        }
        "stats" => {
            let db_path = required(args, 1, "db path")?;
            let db = Db::open(Path::new(&db_path))?;
            let bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
            Ok(format!(
                "entries: {}\nrecords: {}\nfile bytes: {bytes}",
                db.index.len(),
                db.records
            ))
        }
        "compact" => {
            let db_path = required(args, 1, "db path")?;
            let mut db = Db::open(Path::new(&db_path))?;
            db.compact()?;
            Ok(format!("compacted, {} live entries", db.index.len()))
        }
        _ => Ok(HELP.trim().to_string()),
    }
}

/// Shared search core: brute-force cosine or HNSW. Returns (id, score, meta).
pub fn search(db: &Db, query: &[f32], k: usize, exact: bool) -> Vec<(String, f32, String)> {
    if exact {
        let mut scored: Vec<(String, f32, String)> = db
            .index
            .iter()
            .map(|(id, e)| (id.clone(), vector::cosine(query, &e.vector), e.meta.clone()))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(k);
        return scored;
    }
    let mut hnsw = Hnsw::new();
    let mut ids = Vec::with_capacity(db.index.len());
    for (id, e) in &db.index {
        hnsw.insert(vector::normalized(&e.vector));
        ids.push(id.clone());
    }
    let q = vector::normalized(query);
    hnsw.search(&q, k, (k * 4).max(64))
        .into_iter()
        .map(|(node, score)| {
            let id = &ids[node];
            let meta = db.index.get(id).map(|e| e.meta.clone()).unwrap_or_default();
            (id.clone(), score, meta)
        })
        .collect()
}

fn required(args: &[String], idx: usize, what: &str) -> Result<String, String> {
    args.get(idx).cloned().ok_or_else(|| format!("{what} required"))
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn endpoint(args: &[String]) -> Result<(String, u16), String> {
    let ep = flag(args, "--endpoint").unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
    let (host, port) = ep.rsplit_once(':').ok_or("endpoint must be host:port")?;
    Ok((host.to_string(), port.parse().map_err(|_| "bad port")?))
}

fn model(args: &[String]) -> String {
    flag(args, "--model").unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

const HELP: &str = r#"
semdb — pure-Rust semantic database

USAGE:
  semdb create  <db>
  semdb put     <db> --id X --vector '0.1,0.2,...' [--meta '<json>']
  semdb get     <db> --id X
  semdb del     <db> --id X
  semdb search  <db> (--vector '...' | --text '...') [--k 10] [--exact]
  semdb embed   <db> --id X --text '...' [--meta '<json>']
  semdb stats   <db>
  semdb compact <db>

Embedding flags: [--endpoint host:port] (default 100.88.0.2:8081)
                 [--model name]         (default embeddinggemma)
"#;
