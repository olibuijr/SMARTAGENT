//! Command-line interface: create / put / get / del / search / embed / stats / compact.

use httpc::args::flag;
use std::path::Path;

use crate::config::Config;
use crate::hnsw::Hnsw;
use crate::http;
use crate::storage::Db;
use crate::vector;

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
                let (host, port, model) = embed_backend(args)?;
                http::fetch_embedding(&host, port, &model, &text)?
            } else {
                return Err("--vector or --text required".into());
            };
            // --filter key=value keeps only rows whose meta JSON has that field.
            // Over-fetch candidates first so the top-k survives filtering.
            let filter = flag(args, "--filter").and_then(|f| f.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())));
            let fetch_k = if filter.is_some() { (k * 8).max(64) } else { k };
            let mut results = search(&db, &query, fetch_k, exact);
            if let Some((fk, fv)) = &filter {
                results.retain(|(_, _, meta)| {
                    crate::json::parse(meta).ok()
                        .and_then(|v| v.get(fk).and_then(|x| x.as_str().map(str::to_string)))
                        .map(|got| &got == fv)
                        .unwrap_or(false)
                });
                results.truncate(k);
            }
            if results.is_empty() {
                return Ok("no results".into());
            }
            // --ids-only: score+id only (meta often embeds the full source text).
            // --meta-chars N: truncate the meta blob. Defaults to full meta.
            let ids_only = args.iter().any(|a| a == "--ids-only");
            let meta_cap = flag(args, "--meta-chars").and_then(|s| s.parse().ok());
            Ok(results
                .into_iter()
                .map(|(id, score, meta)| {
                    if ids_only {
                        format!("{score:.4}\t{id}")
                    } else if let Some(n) = meta_cap {
                        format!("{score:.4}\t{id}\t{}", truncate_chars(&meta, n))
                    } else {
                        format!("{score:.4}\t{id}\t{meta}")
                    }
                })
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
            let (host, port, model) = embed_backend(args)?;
            let vec = http::fetch_embedding(&host, port, &model, &text)?;
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
        "count" => {
            let db_path = required(args, 1, "db path")?;
            let db = Db::open(Path::new(&db_path))?;
            match flag(args, "--prefix") {
                Some(p) => Ok(db.index.keys().filter(|k| k.starts_with(&p)).count().to_string()),
                None => Ok(db.index.len().to_string()),
            }
        }
        "ids" => {
            let db_path = required(args, 1, "db path")?;
            let db = Db::open(Path::new(&db_path))?;
            let prefix = flag(args, "--prefix").unwrap_or_default();
            let limit = flag(args, "--limit").and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
            let mut ids: Vec<&String> = db.index.keys().filter(|k| k.starts_with(&prefix)).collect();
            ids.sort();
            Ok(ids.into_iter().take(limit).cloned().collect::<Vec<_>>().join("\n"))
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

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max { return s.to_string(); }
    format!("{}…", s.chars().take(max).collect::<String>())
}


/// (host, port, model) for embeddings, honoring --endpoint/--model overrides.
fn embed_backend(args: &[String]) -> Result<(String, u16, String), String> {
    Config::load().embeddings(flag(args, "--endpoint").as_deref(), flag(args, "--model").as_deref())
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

Embedding config: config/smartagent.conf (embeddings_endpoint, embeddings_model)
Override with --endpoint host:port / --model name or $SEMDB_ENDPOINT / $SEMDB_MODEL
"#;
