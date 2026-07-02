//! codeindex CLI: search / files

use std::path::PathBuf;
use std::process::ExitCode;

use codeindex::gitignore::Rules;
use codeindex::search::{self, Opts};
use codeindex::walk;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => { println!("{out}"); ExitCode::SUCCESS }
        Err(e) => { eprintln!("error: {e}"); ExitCode::FAILURE }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("search") => {
            let pattern = args.get(1).ok_or("usage: codeindex search <pattern> [dir] [flags]")?;
            let dir = positional_dir(args);
            let ext = flag(args, "-t");
            let rules = Rules::load(&dir);
            let files = walk::walk(&dir, &rules, ext.as_deref());
            let o = Opts {
                regex: has(args, "-e") || has(args, "--regex"),
                ignore_case: has(args, "-i"),
                before: flag(args, "-B").and_then(|s| s.parse().ok()).unwrap_or(0),
                after: flag(args, "-A").and_then(|s| s.parse().ok()).unwrap_or(0),
            };
            let hits = search::search(&files, pattern, &o)?;
            if hits.is_empty() {
                return Ok("no matches".into());
            }
            let mut out = String::new();
            for h in &hits {
                for (k, b) in h.before.iter().enumerate() {
                    out.push_str(&format!("{}:{}-{}\n", h.file.display(), h.line_no - h.before.len() + k, b));
                }
                out.push_str(&format!("{}:{}:{}\n", h.file.display(), h.line_no, h.text));
                for (k, a) in h.after.iter().enumerate() {
                    out.push_str(&format!("{}:{}-{}\n", h.file.display(), h.line_no + 1 + k, a));
                }
            }
            Ok(out.trim_end().to_string())
        }
        Some("files") => {
            let dir = positional_dir(args);
            let rules = Rules::load(&dir);
            let files = walk::walk(&dir, &rules, flag(args, "-t").as_deref());
            Ok(files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join("\n"))
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn positional_dir(args: &[String]) -> PathBuf {
    // First non-flag arg after the subcommand and pattern.
    let skip = if args.first().map(String::as_str) == Some("search") { 2 } else { 1 };
    args.iter().skip(skip).find(|a| !a.starts_with('-')).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn has(args: &[String], name: &str) -> bool { args.iter().any(|a| a == name) }

const HELP: &str = r#"
codeindex — fast code search (ripgrep concept)

USAGE:
  codeindex search <pattern> [dir] [-i] [-e|--regex] [-A N] [-B N] [-t ext]
  codeindex files  [dir] [-t ext]

Respects .gitignore; always skips .git target node_modules .refrepos .pi .scratch workspaces.
"#;
