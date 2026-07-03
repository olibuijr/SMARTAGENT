//! Parallel subagent execution. Each agent is a headless project-pi run (or a
//! configurable --agent-bin for tests), given its own workspace dir under
//! workspaces/<run-id>/agent-<n>/, with stdout+stderr captured to out.log.
//! Fan-out is std::thread; each agent has a wall-clock timeout (kill on expiry).

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
    pub attempts: usize,
}

pub struct Runner {
    pub agent_bin: String,
    pub timeout: Duration,
    /// Concurrent-agent ceiling per wave (width guard).
    pub max_parallel: usize,
    /// Re-run an agent whose exit≠0 or that timed out, up to N extra attempts.
    pub retries: usize,
    /// When true, pass the prompt via `-c` (sh-style) — used with fake test bins.
    pub sh_mode: bool,
    /// SMARTAGENT_DEPTH to set on each subagent (fork-bomb guard).
    pub child_depth: u32,
}

impl Runner {
    /// Run all specs concurrently in chunks of `max_parallel` (the depth guard
    /// bounds nesting, this bounds WIDTH — 500 prompts must not fork 500 pis),
    /// retrying failed/timed-out agents up to `retries` times.
    pub fn run_all(&self, specs: Vec<AgentSpec>) -> Vec<AgentResult> {
        let mut out: Vec<AgentResult> = Vec::with_capacity(specs.len());
        let chunk = self.max_parallel.max(1);
        let mut queue = specs;
        while !queue.is_empty() {
            let batch: Vec<AgentSpec> = queue.drain(..queue.len().min(chunk)).collect();
            let results = std::thread::scope(|scope| {
                let handles: Vec<_> = batch.into_iter().map(|spec| scope.spawn(move || self.run_with_retries(spec))).collect();
                handles.into_iter().filter_map(|h| h.join().ok()).collect::<Vec<_>>()
            });
            out.extend(results);
        }
        out.sort_by_key(|r| r.n);
        out
    }

    fn run_with_retries(&self, spec: AgentSpec) -> AgentResult {
        let mut attempt = 0;
        loop {
            let spec_n = AgentSpec { n: spec.n, prompt: spec.prompt.clone(), workspace: spec.workspace.clone() };
            let mut r = self.run_one(spec_n);
            r.attempts = attempt + 1;
            if (r.exit == 0 && !r.timed_out) || attempt >= self.retries {
                return r;
            }
            attempt += 1;
        }
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
        cmd.env("SMARTAGENT_DEPTH", self.child_depth.to_string());
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::write(&log_path, format!("spawn failed: {e}"));
                return AgentResult { n: spec.n, exit: -1, secs: 0.0, workspace: spec.workspace, timed_out: false, attempts: 1 };
            }
        };
        let output = procutil::wait_with_timeout(child, self.timeout);
        let _ = std::fs::write(&log_path, output.text);
        AgentResult { n: spec.n, exit: output.exit, secs: start.elapsed().as_secs_f64(), workspace: spec.workspace, timed_out: output.timed_out, attempts: 1 }
    }
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
        let runner = Runner { agent_bin: "/bin/sh".into(), timeout: Duration::from_secs(10), sh_mode: true, child_depth: 1, max_parallel: 4, retries: 0 };
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
        let runner = Runner { agent_bin: "/bin/sh".into(), timeout: Duration::from_secs(1), sh_mode: true, child_depth: 1, max_parallel: 4, retries: 0 };
        let specs = vec![AgentSpec { n: 0, prompt: "sleep 60".into(), workspace: root.join("agent-0") }];
        let start = Instant::now();
        let results = runner.run_all(specs);
        assert!(results[0].timed_out);
        assert!(start.elapsed() < Duration::from_secs(5), "timeout did not fire promptly");
    }
}
