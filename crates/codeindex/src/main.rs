//! codeindex CLI: search / files / projects / index

use httpc::args::{flag, has};
use std::path::PathBuf;
use std::process::ExitCode;

use codeindex::gitignore::Rules;
use codeindex::project;
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
            let dir = target_dir(args)?;
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
            let dir = target_dir(args)?;
            let rules = Rules::load(&dir);
            let files = walk::walk(&dir, &rules, flag(args, "-t").as_deref());
            let max = flag(args, "--limit").and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
            Ok(files.iter().take(max).map(|f| f.display().to_string()).collect::<Vec<_>>().join("\n"))
        }
        Some("projects") => {
            let root = project::workspaces_root()?;
            let ps = project::list(&root);
            if ps.is_empty() {
                return Ok("no projects under workspaces".into());
            }
            let rows: Vec<String> = ps.iter().map(|p| {
                let kind = if p.is_repo { "repo" } else { "dir" };
                let st = match project::status(&p.path) {
                    Some((files, age)) => format!("indexed {} files ({} ago)", files, project::human_age(age)),
                    None => "not indexed".into(),
                };
                format!("{}\t[{}]\t{}", p.name, kind, st)
            }).collect();
            Ok(rows.join("\n"))
        }
        Some("index") => {
            let root = project::workspaces_root()?;
            let mut skipped: Vec<String> = Vec::new();
            let targets: Vec<project::Project> = if has(args, "--all") {
                // Only git repos: numeric orchestrate run-dirs and infra dirs
                // (sandbox, supervise-logs) under workspaces/ are not projects.
                // Skips are reported, never silent — `index <name>` still works
                // on a non-repo dir explicitly.
                let (repos, rest): (Vec<_>, Vec<_>) = project::list(&root).into_iter().partition(|p| p.is_repo);
                skipped = rest.into_iter().map(|p| p.name).collect();
                repos
            } else {
                let name = args.get(1).filter(|a| !a.starts_with('-'))
                    .ok_or("usage: codeindex index <project>|--all")?;
                let path = project::resolve(&root, name)?;
                vec![project::Project { name: name.clone(), path, is_repo: true }]
            };
            if targets.is_empty() {
                return Ok("no repo projects under workspaces".into());
            }
            let (mut ok, mut failed) = (0, 0);
            let mut out = Vec::new();
            for t in &targets {
                match project::index_project(&t.path) {
                    Ok(s) => { ok += 1; out.push(format!("{}: OK — {} files, {} KB", t.name, s.files, s.bytes / 1024)); }
                    Err(e) => { failed += 1; out.push(format!("{}: FAIL — {e}", t.name)); }
                }
            }
            out.push(format!("indexed {} project(s): {ok} ok, {failed} failed", targets.len()));
            if !skipped.is_empty() {
                out.push(format!("skipped {} non-repo dir(s): {}", skipped.len(), skipped.join(", ")));
            }
            Ok(out.join("\n"))
        }
        _ => Ok(HELP.trim().into()),
    }
}

/// Search/files target: `--project <name>` scopes to a workspace project;
/// otherwise the first positional dir (flag values are NOT positionals).
fn target_dir(args: &[String]) -> Result<PathBuf, String> {
    if let Some(name) = flag(args, "--project") {
        return project::resolve(&project::workspaces_root()?, &name);
    }
    // Flags that consume a value — their value must not be read as the dir.
    const VALUE_FLAGS: &[&str] = &["-t", "-B", "-A", "-m", "--max-count", "--limit", "--project"];
    let skip = if args.first().map(String::as_str) == Some("search") { 2 } else { 1 };
    let mut iter = args.iter().skip(skip);
    while let Some(a) = iter.next() {
        if a.starts_with('-') {
            if VALUE_FLAGS.contains(&a.as_str()) { iter.next(); }
            continue;
        }
        return Ok(PathBuf::from(a));
    }
    Ok(PathBuf::from("."))
}

const HELP: &str = r#"
codeindex — fast code search + per-project workspace index (ripgrep concept)

USAGE:
  codeindex search <pattern> [dir] [--project <name>] [-i] [-e|--regex] [-A N] [-B N] [-t ext]
  codeindex files  [dir] [--project <name>] [-t ext]
  codeindex projects                       list workspace projects + index status
  codeindex index <project>|--all          build per-project file index
                                           (<project>/.smartagent/codeindex.semdb)

Respects the target's .gitignore; always skips .git target node_modules
.refrepos .pi .scratch .smartagent workspaces. --project targets a repo
under workspaces/ (which root-level walks deliberately skip).
"#;
