//! Parallel subagent execution. Each agent is a headless project-pi run (or a
//! configurable --agent-bin for tests), given its own workspace dir under
//! workspaces/<run-id>/agent-<n>/, with stdout+stderr captured to out.log.
//! Fan-out is std::thread; each agent has a wall-clock timeout (kill on expiry).

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct AgentSpec {
    pub n: usize,
    pub prompt: String,
    pub workspace: PathBuf,
}

pub struct AgentResult {
    pub n: usize,
    pub exit: i32,
    pub secs: f64,
    pub workspace: PathBuf,
    pub timed_out: bool,
}

pub struct Runner {
    pub agent_bin: String,
    pub timeout: Duration,
    /// When true, pass the prompt via `-c` (sh-style) — used with fake test bins.
    pub sh_mode: bool,
}

impl Runner {
    /// Run all specs concurrently, one thread each; collect results in order.
    pub fn run_all(&self, specs: Vec<AgentSpec>) -> Vec<AgentResult> {
        std::thread::scope(|scope| {
            let handles: Vec<_> = specs
                .into_iter()
                .map(|spec| scope.spawn(move || self.run_one(spec)))
                .collect();
            let mut out: Vec<AgentResult> = handles.into_iter().filter_map(|h| h.join().ok()).collect();
            out.sort_by_key(|r| r.n);
            out
        })
    }

    fn run_one(&self, spec: AgentSpec) -> AgentResult {
        let _ = std::fs::create_dir_all(&spec.workspace);
        let log_path = spec.workspace.join("out.log");
        let start = Instant::now();

        let mut cmd = if self.sh_mode {
            let mut c = Command::new(&self.agent_bin);
            c.arg("-c").arg(&spec.prompt);
            c
        } else {
            // Headless project pi: ./pi --thinking low -p "<prompt>" < /dev/null
            let mut c = Command::new(&self.agent_bin);
            c.arg("--thinking").arg("low").arg("-p").arg(&spec.prompt);
            c
        };
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::write(&log_path, format!("spawn failed: {e}"));
                return AgentResult { n: spec.n, exit: -1, secs: 0.0, workspace: spec.workspace, timed_out: false };
            }
        };
        let (exit, timed_out, output) = wait_with_timeout(child, self.timeout);
        let _ = std::fs::write(&log_path, output);
        AgentResult { n: spec.n, exit, secs: start.elapsed().as_secs_f64(), workspace: spec.workspace, timed_out }
    }
}

/// Poll for completion up to `timeout`; kill and reap if it expires.
fn wait_with_timeout(mut child: std::process::Child, timeout: Duration) -> (i32, bool, String) {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = drain(&mut child);
                return (status.code().unwrap_or(-1), false, out);
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let out = drain(&mut child);
                    return (-1, true, out);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return (-1, false, String::new()),
        }
    }
}

fn drain(child: &mut std::process::Child) -> String {
    let mut s = String::new();
    if let Some(mut o) = child.stdout.take() {
        let _ = o.read_to_string(&mut s);
    }
    if let Some(mut e) = child.stderr.take() {
        let mut es = String::new();
        let _ = e.read_to_string(&mut es);
        if !es.is_empty() {
            s.push_str("\n[stderr]\n");
            s.push_str(&es);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn parallel_agents_capture_logs() {
        let root = scratch("orch-par");
        let runner = Runner { agent_bin: "/bin/sh".into(), timeout: Duration::from_secs(10), sh_mode: true };
        let specs: Vec<AgentSpec> = (0..5)
            .map(|i| AgentSpec { n: i, prompt: format!("echo agent {i} ran"), workspace: root.join(format!("agent-{i}")) })
            .collect();
        let results = runner.run_all(specs);
        assert_eq!(results.len(), 5);
        for (i, r) in results.iter().enumerate() {
            assert_eq!(r.n, i);
            assert_eq!(r.exit, 0);
            let log = std::fs::read_to_string(r.workspace.join("out.log")).unwrap();
            assert!(log.contains(&format!("agent {i} ran")));
        }
    }

    #[test]
    fn timeout_kills_slow_agent() {
        let root = scratch("orch-timeout");
        let runner = Runner { agent_bin: "/bin/sh".into(), timeout: Duration::from_secs(1), sh_mode: true };
        let specs = vec![AgentSpec { n: 0, prompt: "sleep 60".into(), workspace: root.join("agent-0") }];
        let start = Instant::now();
        let results = runner.run_all(specs);
        assert!(results[0].timed_out);
        assert!(start.elapsed() < Duration::from_secs(5), "timeout did not fire promptly");
    }
}
