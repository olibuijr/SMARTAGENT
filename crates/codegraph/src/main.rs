//! codegraph CLI: index / defs / refs / callers / search / stats

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use codegraph::graph::Graph;
use codegraph::{index, symdb};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => { println!("{out}"); ExitCode::SUCCESS }
        Err(e) => { eprintln!("error: {e}"); ExitCode::FAILURE }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("index") => {
            let repo = args.get(1).map(PathBuf::from).ok_or("usage: codegraph index <repo-dir> --out <graph> [--embed]")?;
            let out = flag(args, "--out").map(PathBuf::from).ok_or("--out required")?;
            let mut graph = Graph::new();
            let mut files = 0;
            walk_rs(&repo, &repo, &mut graph, &mut files);
            graph.save(&out)?;
            let mut msg = format!("indexed {files} files → {} symbols, {} edges → {}", graph.symbols.len(), graph.edges.len(), out.display());
            if has(args, "--embed") {
                let idx = symbol_index_path(&out);
                let n = symdb::build_index(&graph, &idx)?;
                msg.push_str(&format!("\nembedded {n} symbols → {}", idx.display()));
            }
            Ok(msg)
        }
        Some("defs") => query(args, |g, n| g.defs(n), "not defined")?.pipe(Ok),
        Some("refs") => query(args, |g, n| g.refs(n), "no references")?.pipe(Ok),
        Some("callers") => query(args, |g, n| g.callers(n), "no callers")?.pipe(Ok),
        Some("stats") => {
            let graph = Graph::load(&graph_arg(args)?)?;
            Ok(graph.stats())
        }
        Some("search") => {
            let graph_path = graph_arg(args)?;
            let query = args.get(2).ok_or("usage: codegraph search <graph> <query> [--k N]")?;
            let k = flag(args, "--k").and_then(|s| s.parse().ok()).unwrap_or(5);
            let found = symdb::search(&symbol_index_path(&graph_path), query, k)?;
            if found.is_empty() {
                return Ok("no matches".into());
            }
            Ok(found.iter().map(|f| format!("{:.4}\t{}", f.score, f.meta)).collect::<Vec<_>>().join("\n"))
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn query(args: &[String], f: impl Fn(&Graph, &str) -> Vec<String>, empty: &str) -> Result<String, String> {
    let graph = Graph::load(&graph_arg(args)?)?;
    let name = args.get(2).ok_or("symbol name required")?;
    let res = f(&graph, name);
    Ok(if res.is_empty() { empty.to_string() } else { res.join("\n") })
}

fn graph_arg(args: &[String]) -> Result<PathBuf, String> {
    args.get(1).map(PathBuf::from).ok_or_else(|| "graph file required".into())
}

fn symbol_index_path(graph: &Path) -> PathBuf {
    graph.with_extension("symbols.semdb")
}

fn walk_rs(root: &Path, dir: &Path, graph: &mut Graph, files: &mut usize) {
    let entries = match std::fs::read_dir(dir) { Ok(e) => e, Err(_) => return };
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if matches!(name, "target" | ".git" | ".refrepos" | "node_modules") { continue; }
            walk_rs(root, &p, graph, files);
        } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
            if let Ok(src) = std::fs::read_to_string(&p) {
                let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().to_string();
                index::index_source(graph, &rel, &src);
                *files += 1;
            }
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}
fn has(args: &[String], name: &str) -> bool { args.iter().any(|a| a == name) }

trait Pipe: Sized { fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R { f(self) } }
impl<T> Pipe for T {}

const HELP: &str = r#"
codegraph — Rust code knowledge graph (CodeGraph concept)

USAGE:
  codegraph index <repo-dir> --out <graph.json> [--embed]
  codegraph defs    <graph.json> <name>
  codegraph refs    <graph.json> <name>
  codegraph callers <graph.json> <fn>
  codegraph search  <graph.json> <query> [--k 5]   (needs --embed at index time)
  codegraph stats   <graph.json>

Structural queries (defs/refs/callers) walk the graph; search is semdb-backed
semantic symbol lookup over embeddings.
"#;
