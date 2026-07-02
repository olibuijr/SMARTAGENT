//! CLI: chunk / ingest / retrieve / get / delete-doc / stats.

use httpc::args::flag;
use std::path::{Path, PathBuf};

use semdb::config::Config;
use semdb::vector;

use crate::chunk::{chunk_text, default_doc_id, ChunkConfig};
use crate::embed::Embedder;
use crate::pdftext::{self, Kind};
use crate::store::{self, RetrievedChunk};

pub fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("chunk") => {
            let path = path_arg(args, 1, "file")?;
            let extracted = pdftext::read_document(&path, kind(args)?)?;
            let doc_id = flag(args, "--doc-id").unwrap_or_else(|| default_doc_id(&path));
            let cfg = chunk_config(args);
            let chunks = chunk_text(
                &extracted.text,
                &doc_id,
                &path.display().to_string(),
                &extracted.kind,
                &cfg,
            );
            if chunks.is_empty() {
                return Ok("no chunks".into());
            }
            Ok(chunks
                .iter()
                .map(|c| {
                    format!(
                        "{}\t{}:{}..{}\t{}",
                        c.id,
                        c.source,
                        c.start,
                        c.end,
                        one_line(&c.text)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("ingest") => {
            let db = path_arg(args, 1, "db")?;
            // --url fetches page text over plain HTTP (via the shared httpc);
            // otherwise ingest a local file (text or PDF-text).
            let (text, source, kind_str, default_id) = if let Some(url) = flag(args, "--url") {
                let body = httpc::get(&url).map_err(|e| format!("fetch {url}: {e}"))?.text()?;
                (strip_html(&body), url.clone(), "url".to_string(), url)
            } else {
                let path = path_arg(args, 2, "file")?;
                let extracted = pdftext::read_document(&path, kind(args)?)?;
                (extracted.text, path.display().to_string(), extracted.kind, default_doc_id(&path))
            };
            let doc_id = flag(args, "--doc-id").unwrap_or(default_id);
            let cfg = chunk_config(args);
            let chunks = chunk_text(&text, &doc_id, &source, &kind_str, &cfg);
            if chunks.is_empty() {
                return Ok("no chunks".into());
            }
            let vectors = if let Some(v) = flag(args, "--vector") {
                let vec = vector::parse_vec(&v)?;
                vec![vec; chunks.len()]
            } else {
                let embedder = embedder(args)?;
                let mut vectors = Vec::with_capacity(chunks.len());
                for chunk in &chunks {
                    vectors.push(embedder.embed(&chunk.text)?);
                }
                vectors
            };
            // Re-ingest safety: drop any existing chunks for this doc first,
            // or an edited doc with fewer/reordered chunks leaves stale orphans.
            let removed = store::delete_doc(&db, &doc_id).unwrap_or(0);
            let n = store::put_chunks(&db, &chunks, &vectors)?;
            if removed > 0 {
                Ok(format!("re-ingested {n} chunks from {source} as {doc_id} (replaced {removed})"))
            } else {
                Ok(format!("ingested {n} chunks from {source} as {doc_id}"))
            }
        }
        Some("retrieve") | Some("search") => {
            let db = path_arg(args, 1, "db")?;
            let k = flag(args, "--k").and_then(|s| s.parse().ok()).unwrap_or(5);
            let exact = args.iter().any(|a| a == "--exact");
            let qv = if let Some(v) = flag(args, "--vector") {
                vector::parse_vec(&v)?
            } else if let Some(text) = flag(args, "--text") {
                embedder(args)?.embed(&text)?
            } else {
                return Err("--text or --vector required".into());
            };
            let hits = store::retrieve(&db, &qv, k, exact, flag(args, "--doc-id").as_deref())?;
            if args.iter().any(|a| a == "--ids-only") {
                // Just citations + scores — the agent fetches full text via `get`
                // for the chunks it actually wants.
                return Ok(hits.iter().map(|h| format!("{:.4}\t{}", h.score, h.citation())).collect::<Vec<_>>().join("\n"));
            }
            let snip = flag(args, "--snippet-chars").and_then(|s| s.parse().ok()).unwrap_or(240usize);
            Ok(format_hits(&hits, snip))
        }
        Some("get") => {
            let db = path_arg(args, 1, "db")?;
            let id = flag(args, "--id").ok_or("--id required")?;
            // `get` returns the full chunk text (that's the point of fetching one).
            Ok(format_hit(&store::get(&db, &id)?, usize::MAX))
        }
        Some("delete-doc") => {
            let db = path_arg(args, 1, "db")?;
            let doc_id = flag(args, "--doc-id").ok_or("--doc-id required")?;
            let n = store::delete_doc(&db, &doc_id)?;
            Ok(format!("deleted {n} chunks for {doc_id}"))
        }
        Some("stats") => {
            let db = path_arg(args, 1, "db")?;
            let (chunks, docs, records) = store::stats(&db)?;
            Ok(format!(
                "chunks: {chunks}\ndocuments: {docs}\nrecords: {records}"
            ))
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn embedder(args: &[String]) -> Result<Embedder, String> {
    let cfg = Config::load();
    let endpoint = cfg
        .resolve("embeddings_endpoint", "SEMDB_ENDPOINT", flag(args, "--endpoint").as_deref())
        .ok_or("no embeddings endpoint: set embeddings_endpoint in config/smartagent.conf, $SEMDB_ENDPOINT, or --endpoint")?;
    let model = cfg
        .resolve(
            "embeddings_model",
            "SEMDB_MODEL",
            flag(args, "--model").as_deref(),
        )
        .unwrap_or_else(|| semdb::config::DEFAULT_EMBED_MODEL.to_string());
    Ok(Embedder::new(endpoint, model))
}

fn kind(args: &[String]) -> Result<Kind, String> {
    match flag(args, "--kind").as_deref().unwrap_or("auto") {
        "auto" => Ok(Kind::Auto),
        "text" => Ok(Kind::Text),
        "pdf" => Ok(Kind::Pdf),
        other => Err(format!("unknown --kind '{other}' (auto|text|pdf)")),
    }
}

fn chunk_config(args: &[String]) -> ChunkConfig {
    ChunkConfig {
        max_tokens: flag(args, "--chunk-tokens")
            .and_then(|s| s.parse().ok())
            .unwrap_or(512),
        overlap_tokens: flag(args, "--overlap")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
    }
}

fn path_arg(args: &[String], idx: usize, what: &str) -> Result<PathBuf, String> {
    args.get(idx)
        .map(Path::new)
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("{what} required"))
}


fn format_hits(hits: &[RetrievedChunk], snippet_chars: usize) -> String {
    if hits.is_empty() {
        "no chunks".into()
    } else {
        hits.iter().map(|h| format_hit(h, snippet_chars)).collect::<Vec<_>>().join("\n")
    }
}

fn format_hit(h: &RetrievedChunk, snippet_chars: usize) -> String {
    // Drop the redundant doc_id column (the citation already carries the id);
    // truncate the chunk body to snippet_chars so retrieval doesn't dump full
    // 512-token chunks × k into context.
    format!(
        "{:.4}\t{}\t{}:{}..{}\t{}",
        h.score,
        h.citation(),
        h.source,
        h.start,
        h.end,
        truncate_chars(&one_line(&h.text), snippet_chars)
    )
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

const HELP: &str = r#"
rag — text/PDF-text ingestion into semdb with cited retrieval

USAGE:
  rag chunk      <file> [--kind auto|text|pdf] [--doc-id ID]
                 [--chunk-tokens 512] [--overlap 0]
  rag ingest     <db> <file> [--kind auto|text|pdf] [--doc-id ID]
                 [--chunk-tokens 512] [--overlap 0]
                 [--endpoint host:port] [--model name] [--vector '0.1,0.2']
  rag retrieve   <db> (--text QUERY | --vector '0.1,0.2') [--k 5]
                 [--endpoint host:port] [--model name] [--exact] [--doc-id ID]
  rag get        <db> --id CHUNK_ID
  rag delete-doc <db> --doc-id ID
  rag stats      <db>

Embedding config: config/smartagent.conf (embeddings_endpoint, embeddings_model)
"#;

/// Crude HTML→text: drop <script>/<style>, strip tags, decode a few entities,
/// collapse whitespace. Enough to ingest a page's readable text over HTTP.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let bytes = html.as_bytes();
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    let mut in_tag = false;
    let mut skip_to: Option<&str> = None;
    while i < bytes.len() {
        if let Some(end) = skip_to {
            if lower[i..].starts_with(end) {
                i += end.len();
                skip_to = None;
            } else {
                i += 1;
            }
            continue;
        }
        let c = bytes[i] as char;
        if c == '<' {
            if lower[i..].starts_with("<script") { skip_to = Some("</script>"); i += 1; continue; }
            if lower[i..].starts_with("<style") { skip_to = Some("</style>"); i += 1; continue; }
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
            out.push(' ');
        } else if !in_tag {
            out.push(c);
        }
        i += 1;
    }
    let decoded = out
        .replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
        .replace("&quot;", "\"").replace("&#39;", "'").replace("&nbsp;", " ");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}
