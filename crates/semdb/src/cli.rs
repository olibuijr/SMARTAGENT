//! Command-line interface: create / put / get / del / search / embed / stats /
//! compact. Every db-touching verb is a THIN CLIENT of the `semdb serve`
//! daemon (see `client`) — the daemon is the single owner of each db, so there
//! is no direct-file fallback: if it is not running, the verb errors. `serve`
//! itself and pure embedding math (fetching vectors) run in-process.

use httpc::args::flag;

use crate::client;
use crate::config::Config;
use crate::hnsw::Hnsw;
use crate::http;
use crate::storage::Db;
use crate::vector;

pub fn run(args: &[String]) -> Result<String, String> {
    if args.is_empty()
        || args.iter().any(|a| a == "--help" || a == "-h")
        || args.first().is_some_and(|a| a == "help")
    {
        return Ok(HELP.trim().to_string());
    }
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    match cmd {
        "serve" => crate::server::serve(args),
        "create" => {
            let db_path = required(args, 1, "db path")?;
            client::create(&db_path)
        }
        "put" => {
            let db_path = required(args, 1, "db path")?;
            let id = flag(args, "--id").ok_or("--id required")?;
            let meta = flag(args, "--meta").unwrap_or_default();
            let vec_str = flag(args, "--vector").ok_or("--vector required")?;
            let vec = vector::parse_vec(&vec_str)?;
            client::put(&db_path, &id, &meta, &vec)
        }
        "get" => {
            let db_path = required(args, 1, "db path")?;
            let id = flag(args, "--id").ok_or("--id required")?;
            client::get(&db_path, &id)
        }
        "del" => {
            let db_path = required(args, 1, "db path")?;
            // --prefix deletes a whole family of rows in one call (doc chunks,
            // a table's namespace) — the review's delete-by-filter gap.
            if let Some(prefix) = flag(args, "--prefix") {
                if prefix.is_empty() {
                    return Err("--prefix must be non-empty".into());
                }
                return client::del_prefix(&db_path, &prefix);
            }
            let id = flag(args, "--id").ok_or("--id or --prefix required")?;
            client::del_id(&db_path, &id)
        }
        "embed-batch" => {
            // Bulk ingest: TSV lines `id<TAB>text` from --file, ONE embeddings
            // POST and one bulk put for the whole set.
            let db_path = required(args, 1, "db path")?;
            let file = flag(args, "--file").ok_or("--file with id<TAB>text lines required")?;
            let content =
                std::fs::read_to_string(&file).map_err(|e| format!("read {file}: {e}"))?;
            let mut ids = Vec::new();
            let mut texts = Vec::new();
            for (i, line) in content.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                let (id, text) = line
                    .split_once('\t')
                    .ok_or_else(|| format!("line {}: expected id<TAB>text", i + 1))?;
                ids.push(id.to_string());
                texts.push(text.to_string());
            }
            if ids.is_empty() {
                return Ok("nothing to embed".into());
            }
            let (host, port, model) = embed_backend(args)?;
            let vecs = http::fetch_embeddings(&host, port, &model, &texts)?;
            let rows: Vec<(String, String, Vec<f32>)> = ids
                .iter()
                .zip(&texts)
                .zip(vecs)
                .map(|((id, text), vec)| {
                    (
                        id.clone(),
                        format!(r#"{{"text":"{}"}}"#, crate::json::escape(text)),
                        vec,
                    )
                })
                .collect();
            let n = client::put_many(&db_path, &rows)?;
            Ok(format!("embedded {n} rows (one request)"))
        }
        "search" => {
            let db_path = required(args, 1, "db path")?;
            let k: usize = flag(args, "--k").and_then(|s| s.parse().ok()).unwrap_or(10);
            let exact = args.iter().any(|a| a == "--exact");
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
            let filter = flag(args, "--filter").and_then(|f| {
                f.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            });
            let fetch_k = if filter.is_some() { (k * 8).max(64) } else { k };
            let mut results = client::search(&db_path, &query, fetch_k, exact)?;
            if let Some((fk, fv)) = &filter {
                results.retain(|(_, _, meta)| {
                    crate::json::parse(meta)
                        .ok()
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
            let meta = flag(args, "--meta")
                .unwrap_or_else(|| format!(r#"{{"text":"{}"}}"#, crate::json::escape(&text)));
            let (host, port, model) = embed_backend(args)?;
            let vec = http::fetch_embedding(&host, port, &model, &text)?;
            let dim = vec.len();
            client::put(&db_path, &id, &meta, &vec)?;
            Ok(format!("embedded {id} (dim {dim})"))
        }
        "stats" => {
            let db_path = required(args, 1, "db path")?;
            client::stats(&db_path)
        }
        "count" => {
            let db_path = required(args, 1, "db path")?;
            client::count(&db_path, flag(args, "--prefix").as_deref())
        }
        "ids" => {
            let db_path = required(args, 1, "db path")?;
            let prefix = flag(args, "--prefix").unwrap_or_default();
            let limit = flag(args, "--limit")
                .and_then(|s| s.parse().ok())
                .unwrap_or(usize::MAX);
            client::ids(&db_path, &prefix, limit)
        }
        "compact" => {
            let db_path = required(args, 1, "db path")?;
            client::compact(&db_path)
        }
        _ => Ok(HELP.trim().to_string()),
    }
}

/// Shared search core: brute-force cosine or HNSW. Returns (id, score, meta).
/// Runs inside the daemon against the in-memory index (see `server`).
pub fn search(db: &Db, query: &[f32], k: usize, exact: bool) -> Vec<(String, f32, String)> {
    // Rebuilding HNSW from scratch per query costs MORE than brute force
    // until the graph is persisted — auto-exact below 10k rows makes the
    // common case both faster and exactly correct.
    let exact = exact || db.index.len() < 10_000;
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
    args.get(idx)
        .cloned()
        .ok_or_else(|| format!("{what} required"))
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}…", s.chars().take(max).collect::<String>())
}

/// (host, port, model) for embeddings, honoring --endpoint/--model overrides.
fn embed_backend(args: &[String]) -> Result<(String, u16, String), String> {
    Config::load().embeddings(
        flag(args, "--endpoint").as_deref(),
        flag(args, "--model").as_deref(),
    )
}

const HELP: &str = r#"
semdb — pure-Rust semantic database (client/daemon)

USAGE:
  semdb serve                                 run the persistent daemon (owns every db)
  semdb create  <db>
  semdb put     <db> --id X --vector '0.1,0.2,...' [--meta '<json>']
  semdb get     <db> --id X
  semdb del     <db> (--id X | --prefix P)
  semdb search  <db> (--vector '...' | --text '...') [--k 10] [--exact]  (auto-exact <10k rows)
  semdb embed   <db> --id X --text '...' [--meta '<json>']
  semdb embed-batch <db> --file lines.tsv    (id<TAB>text per line, ONE request)
  semdb stats   <db>
  semdb compact <db>

All db verbs route through the daemon (start it: supervise up semdb). No fallback.
Embedding config: config/smartagent.conf (embeddings_endpoint, embeddings_model)
Override with --endpoint host:port / --model name or $SEMDB_ENDPOINT / $SEMDB_MODEL
"#;
