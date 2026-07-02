//! codeindex CLI: search / files

use httpc::args::{flag, has};
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
            // -c count-only: just "<N> matches in <M> files". -l files-only:
            // distinct file list. Otherwise lines, capped by -m (default 50) so
            // a loose pattern can't flood the agent's context.
            if has(args, "-c") || has(args, "--count") {
                let files_n = hits.iter().map(|h| &h.file).collect::<std::collections::BTreeSet<_>>().len();
                return Ok(format!("{} matches in {} files", hits.len(), files_n));
            }
            if has(args, "-l") || has(args, "--files-with-matches") {
                let mut fs: Vec<String> = hits.iter().map(|h| h.file.display().to_string()).collect();
                fs.sort(); fs.dedup();
                return Ok(fs.join("\n"));
            }
            let max = flag(args, "-m").or_else(|| flag(args, "--max-count")).and_then(|s| s.parse().ok()).unwrap_or(50usize);
            let shown = hits.len().min(max);
            let mut out = String::new();
            for h in hits.iter().take(max) {
                for (k, b) in h.before.iter().enumerate() {
                    out.push_str(&format!("{}:{}-{}\n", h.file.display(), h.line_no - h.before.len() + k, b));
                }
                out.push_str(&format!("{}:{}:{}\n", h.file.display(), h.line_no, h.text));
                for (k, a) in h.after.iter().enumerate() {
                    out.push_str(&format!("{}:{}-{}\n", h.file.display(), h.line_no + 1 + k, a));
                }
            }
            if hits.len() > max {
                out.push_str(&format!("…[{} of {} matches shown; raise -m or use -l/-c]\n", shown, hits.len()));
            }
            Ok(out.trim_end().to_string())
        }
        Some("files") => {
            let dir = positional_dir(args);
            let rules = Rules::load(&dir);
            let files = walk::walk(&dir, &rules, flag(args, "-t").as_deref());
            let max = flag(args, "--limit").and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
            Ok(files.iter().take(max).map(|f| f.display().to_string()).collect::<Vec<_>>().join("\n"))
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn positional_dir(args: &[String]) -> PathBuf {
    // First non-flag arg after the subcommand and pattern.
    let skip = if args.first().map(String::as_str) == Some("search") { 2 } else { 1 };
    args.iter().skip(skip).find(|a| !a.starts_with('-')).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}



const HELP: &str = r#"
codeindex — fast code search (ripgrep concept)

USAGE:
  codeindex search <pattern> [dir] [-i] [-e|--regex] [-A N] [-B N] [-t ext]
  codeindex files  [dir] [-t ext]

Respects .gitignore; always skips .git target node_modules .refrepos .pi .scratch workspaces.
"#;
