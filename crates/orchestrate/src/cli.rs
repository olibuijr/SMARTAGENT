//! CLI: run / fan / list

use httpc::args::flag;
use std::path::PathBuf;
use std::time::Duration;

use crate::spawn::{AgentSpec, Runner};

fn run_id() -> String {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).to_string()
}

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
        Some("list") => {
            let root = workspaces_root();
            let mut runs = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&root) {
                for e in entries.flatten() {
                    if e.path().is_dir() {
                        runs.push(e.file_name().to_string_lossy().to_string());
                    }
                }
            }
            runs.sort();
            Ok(if runs.is_empty() { "no runs".into() } else { runs.join("\n") })
        }
        Some("out") => {
            // Collect each subagent's captured output for a run — otherwise the
            // agent must read N workspace files by hand.
            let id = args.get(1).ok_or("usage: orchestrate out <run-id> [--tail N]")?;
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
    let runner = Runner { agent_bin, timeout, sh_mode, child_depth };

    let specs: Vec<AgentSpec> = prompts
        .into_iter()
        .enumerate()
        .map(|(i, prompt)| AgentSpec { n: i, prompt, workspace: base.join(format!("agent-{i}")) })
        .collect();
    let results = runner.run_all(specs);

    let mut out = vec![format!("run {id}: {} agents", results.len())];
    out.push("agent\texit\tsecs\tstatus\tworkspace".into());
    for r in &results {
        let status = if r.timed_out { "TIMEOUT" } else if r.exit == 0 { "ok" } else { "fail" };
        out.push(format!("{}\t{}\t{:.1}\t{}\t{}", r.n, r.exit, r.secs, status, r.workspace.display()));
    }
    Ok(out.join("\n"))
}


const HELP: &str = r#"
orchestrate — subagent fan-out (LangGraph send/supervisor concept)

USAGE:
  orchestrate run  --agents N --prompt 'template with {i}' [--timeout 300] [--agent-bin ./pi]
  orchestrate fan  --prompts-file FILE [--timeout 300] [--agent-bin ./pi]
  orchestrate list

Each agent is a headless `./pi --thinking low -p '<prompt>' < /dev/null` run in
its own workspaces/<run-id>/agent-<n>/ dir (stdout+stderr → out.log).
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
