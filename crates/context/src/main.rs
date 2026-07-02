//! context — principal identity/context loader (PAI TELOS pattern).
//! Composes a context/ dir of markdown files into one system-prompt block,
//! trimming to a char budget by dropping lowest-priority files first.

use httpc::args::flag;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod compose;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let dir = flag(args, "--dir").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("context"));
    match cmd {
        "compose" => {
            let budget = flag(args, "--budget").and_then(|s| s.parse().ok()).unwrap_or(4000);
            compose::compose(&dir, budget)
        }
        "validate" => compose::validate(&dir),
        "stat" => compose::stat(&dir),
        _ => Ok(HELP.trim().into()),
    }
}


pub(crate) fn order_path(dir: &Path) -> PathBuf {
    dir.join("ORDER")
}

const HELP: &str = r#"
context — principal identity/context loader (TELOS pattern)

USAGE:
  context compose  --dir context/ [--budget 4000]
  context validate --dir context/
  context stat     --dir context/

Priority order is read from <dir>/ORDER (one filename per line, highest first).
On compose, lowest-priority files are trimmed first to fit the char budget.
"#;
