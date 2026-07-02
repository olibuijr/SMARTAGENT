//! sandbox CLI: run / clean

use httpc::args::{flag, has};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use sandbox::exec::{self, SandboxSpec};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => { println!("{out}"); ExitCode::SUCCESS }
        Err(e) => { eprintln!("error: {e}"); ExitCode::FAILURE }
    }
}

fn sandbox_root() -> PathBuf {
    semdb::config::Config::load().workspaces_dir().join("sandbox")
}

/// Paths to tmpfs-mask inside the sandbox namespace so untrusted commands can't
/// read them: the secret store and the pi runtime dir (holds the router key).
fn sensitive_paths() -> Vec<PathBuf> {
    let cfg = semdb::config::Config::load();
    let data = cfg.data_dir();
    let repo = data.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    [data.join("secrets"), repo.join(".pi")]
        .into_iter()
        .filter(|p| p.exists())
        .collect()
}

fn run_id() -> String {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0).to_string()
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("run") => {
            // Everything after `--` is the command.
            let split = args.iter().position(|a| a == "--").ok_or("usage: sandbox run [flags] -- <cmd...>")?;
            let cmd: Vec<String> = args[split + 1..].to_vec();
            if cmd.is_empty() {
                return Err("no command after --".into());
            }
            let inputs: Vec<PathBuf> = args.iter().enumerate()
                .filter(|(_, a)| a.as_str() == "--input")
                .filter_map(|(i, _)| args.get(i + 1).map(PathBuf::from))
                .collect();
            let spec = SandboxSpec {
                id: run_id(),
                root: sandbox_root(),
                cmd,
                inputs,
                net: has(args, "--net"),
                // Isolation (namespaces + scrubbed env) is ON by default; a
                // caller must explicitly pass --no-isolate to drop to
                // filesystem-only confinement. Safe default for untrusted cmds.
                isolate: !has(args, "--no-isolate"),
                timeout: Duration::from_secs(flag(args, "--timeout").and_then(|s| s.parse().ok()).unwrap_or(30)),
                // Default 16KB (was 1MB): a chatty command shouldn't be able to
                // flood the agent's context. Raise with --max-output when needed.
                max_output: flag(args, "--max-output").and_then(|s| s.parse().ok()).unwrap_or(16_384),
                tail: has(args, "--tail"),
                masks: sensitive_paths(),
            };
            let res = exec::run(&spec)?;
            let mut out = vec![format!("exit: {}{}", res.exit, if res.timed_out { " (TIMEOUT)" } else { "" })];
            // The isolated/workspace preamble is diagnostics; only the agent
            // debugging isolation needs it, so gate it behind --verbose.
            if has(args, "--verbose") {
                out.push(format!("isolated: {}", if res.isolated { "namespaces" } else { "filesystem-only (unshare not available)" }));
                out.push(format!("workspace: {}", res.workspace.display()));
            }
            if !res.stdout.is_empty() { out.push(format!("--- stdout ---\n{}", res.stdout)); }
            if !res.stderr.is_empty() { out.push(format!("--- stderr ---\n{}", res.stderr)); }
            Ok(out.join("\n"))
        }
        Some("clean") => {
            let hours = flag(args, "--older-than-hours").and_then(|s| s.parse().ok());
            let n = exec::clean(&sandbox_root(), hours)?;
            Ok(format!("removed {n} sandbox workspace(s)"))
        }
        _ => Ok(HELP.trim().into()),
    }
}


const HELP: &str = r#"
sandbox — isolated command execution (Daytona concept, Linux)

USAGE:
  sandbox run [--timeout 30] [--net] [--no-isolate] [--input FILE]... [--max-output N] -- <cmd...>
  sandbox clean [--older-than-hours H]

Runs the command in a fresh workspaces/sandbox/<id>/ dir with a scrubbed
environment (parent secrets removed). Namespace isolation is on by default
when `unshare` works; --no-isolate drops to filesystem-only confinement.
"#;
