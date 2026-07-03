//! CLI: run / fan / list

use httpc::args::flag;
use std::path::PathBuf;
use std::time::Duration;

use crate::spawn::{AgentSpec, Runner};

fn run_id() -> String { procutil::unix_secs().to_string() }

/// Fan-out nesting depth from the environment. Each spawned subagent runs with
/// SMARTAGENT_DEPTH one higher; beyond MAX_DEPTH we refuse — a subagent that
/// itself fans out is a fork bomb (each level multiplies pi processes).
const MAX_DEPTH: u32 = 1;

fn current_depth() -> u32 {
    std::env::var("SMARTAGENT_DEPTH").ok().and_then(|d| d.parse().ok()).unwrap_or(0)
}

fn guard_depth() -> Result<u32, String> {
    let d = current_depth();
    if d >= MAX_DEPTH {
        return Err(format!(
            "orchestrate refused: fan-out depth {d} ≥ max {MAX_DEPTH} (a subagent may not spawn more subagents)"
        ));
    }
    Ok(d + 1)
}

fn workspaces_root() -> PathBuf {
    semdb::config::Config::load().workspaces_dir()
}

fn store_path() -> PathBuf {
    semdb::config::Config::load().data_dir().join("orchestrate.semdb")
}

fn persisted_run_ids() -> Vec<String> {
    let path = store_path();
    let Ok(db) = semdb::storage::Db::open(&path) else { return Vec::new(); };
    let mut ids: Vec<String> = db.index.keys().filter_map(|k| k.split_once(':').map(|(run, _)| run.to_string())).collect();
    ids.sort();
    ids.dedup();
    ids
}

pub fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("run") => {
            let n: usize = flag(args, "--agents").and_then(|s| s.parse().ok()).ok_or("--agents N required")?;
            let template = flag(args, "--prompt").ok_or("--prompt required")?;
            let prompts: Vec<String> = (0..n).map(|i| template.replace("{i}", &i.to_string())).collect();
            fan_out(args, prompts)
        }
        Some("fan") => {
            let file = flag(args, "--prompts-file").ok_or("--prompts-file required")?;
            let text = std::fs::read_to_string(&file).map_err(|e| format!("read {file}: {e}"))?;
            let prompts: Vec<String> = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
            if prompts.is_empty() {
                return Err("no prompts in file".into());
            }
            fan_out(args, prompts)
        }
        Some("statusline") => {
            // `level|text` for UI statuslines: persisted fan-out run count.
            let n = persisted_run_ids().len();
            Ok(if n == 0 { "ok|🤖 idle".to_string() } else { format!("ok|🤖 {n} runs") })
        }
        Some("list") => {
            let runs = persisted_run_ids();
            Ok(if runs.is_empty() { "no runs".into() } else { runs.join("\n") })
        }
        Some("out") => {
            // Collect each subagent's captured output for a run — otherwise the
            // agent must read N workspace files by hand.
            let id = args.get(1).ok_or("usage: orchestrate out <run-id> [--tail N]")?;
            if !persisted_run_ids().iter().any(|r| r == id) {
                return Err(format!("no persisted run '{id}'"));
            }
            let tail = flag(args, "--tail").and_then(|s| s.parse().ok()).unwrap_or(2000usize);
            let run_dir = workspaces_root().join(id);
            let mut agents: Vec<PathBuf> = std::fs::read_dir(&run_dir)
                .map_err(|_| format!("no run '{id}'"))?
                .flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            agents.sort();
            if agents.is_empty() {
                return Err(format!("run '{id}' has no agents"));
            }
            let mut out = Vec::new();
            for a in agents {
                let name = a.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let log = std::fs::read_to_string(a.join("out.log")).unwrap_or_else(|_| "(no output)".into());
                let kept: String = {
                    let chars: Vec<char> = log.chars().collect();
                    let start = chars.len().saturating_sub(tail);
                    chars[start..].iter().collect()
                };
                out.push(format!("── {name} ──\n{}", kept.trim()));
            }
            Ok(out.join("\n"))
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn fan_out(args: &[String], prompts: Vec<String>) -> Result<String, String> {
    let id = run_id();
    let base = workspaces_root().join(&id);
    let child_depth = guard_depth()?;
    let agent_bin = flag(args, "--agent-bin").unwrap_or_else(|| "./pi".into());
    let sh_mode = agent_bin.ends_with("sh") || flag(args, "--agent-bin").map(|b| b == "/bin/echo").unwrap_or(false);
    let timeout = Duration::from_secs(flag(args, "--timeout").and_then(|s| s.parse().ok()).unwrap_or(300));
    let max_parallel = flag(args, "--max-parallel").and_then(|s| s.parse().ok()).unwrap_or(4);
    let retries = flag(args, "--retries").and_then(|s| s.parse().ok()).unwrap_or(0);
    let runner = Runner { agent_bin, timeout, sh_mode, child_depth, max_parallel, retries };

    let specs: Vec<AgentSpec> = prompts
        .into_iter()
        .enumerate()
        .map(|(i, prompt)| AgentSpec { n: i, prompt, workspace: base.join(format!("agent-{i}")) })
        .collect();
    let results = runner.run_all(specs);

    // Persist run + agent results to a semdb table — results were previously
    // stdout-only (convention violation); this unblocks later status queries.
    persist_run(&id, &results);
    let mut out = vec![format!("run {id}: {} agents", results.len())];
    out.push("agent\texit\tsecs\tstatus\tattempts\tworkspace".into());
    for r in &results {
        let status = if r.timed_out { "TIMEOUT" } else if r.exit == 0 { "ok" } else { "fail" };
        out.push(format!("{}\t{}\t{:.1}\t{}\t{}\t{}", r.n, r.exit, r.secs, status, r.attempts, r.workspace.display()));
    }
    Ok(out.join("\n"))
}


/// Best-effort run persistence in data/orchestrate.semdb (one row per agent).
fn persist_run(id: &str, results: &[crate::spawn::AgentResult]) {
    let path = store_path();
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    let db = if path.exists() { semdb::storage::Db::open(&path) } else { semdb::storage::Db::create(&path) };
    if let Ok(mut db) = db {
        for r in results {
            let meta = format!(
                r#"{{"run":"{id}","agent":{},"exit":{},"secs":{:.1},"timed_out":{},"attempts":{}}}"#,
                r.n, r.exit, r.secs, r.timed_out, r.attempts
            );
            let _ = db.put(&format!("{id}:{}", r.n), &meta, vec![0.0]);
        }
    }
}

const HELP: &str = r#"
orchestrate — subagent fan-out (LangGraph send/supervisor concept)

USAGE:
  orchestrate run  --agents N --prompt 'template with {i}' [--timeout 300] [--agent-bin ./pi]
                   [--max-parallel 4] [--retries 0]
  orchestrate fan  --prompts-file FILE [--timeout 300] [--agent-bin ./pi]
                   [--max-parallel 4] [--retries 0]
  orchestrate list
  orchestrate out <run-id> [--tail 2000]
  orchestrate statusline

Each agent is a headless `./pi --thinking low -p '<prompt>' < /dev/null` run in
its own workspaces/<run-id>/agent-<n>/ dir (stdout+stderr → out.log). Runs are
persisted in data/orchestrate.semdb; use `out` to collect subagent logs.
Fan-out depth is guarded by SMARTAGENT_DEPTH; subagents may not fan out again.
"#;

#[cfg(test)]
mod neg_tests {
    use super::*;

    #[test]
    fn rejects_bad_args() {
        let s=|v:&[&str]|v.iter().map(|x|x.to_string()).collect::<Vec<_>>();
        assert!(run(&s(&["run"])).is_err());              // missing --agents
        assert!(run(&s(&["run","--agents","2"])).is_err()); // missing --prompt
        assert!(run(&s(&["fan"])).is_err());              // missing --prompts-file
        assert!(run(&s(&["fan","--prompts-file",".scratch/nonexistent-neg"])).is_err());
    }

}
