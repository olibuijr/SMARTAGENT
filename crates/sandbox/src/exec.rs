//! Isolated command execution (Daytona concept, Linux).
//!
//! Isolation layers:
//!   1. Filesystem (always) — the command's cwd is a fresh scratch dir under
//!      workspaces/sandbox/<id>/; relative writes land there, not in the repo.
//!      Requested inputs are copied in. stdout/stderr are redirected to files
//!      in the workspace (so a timeout-kill never blocks on a pipe held open
//!      by a lingering grandchild).
//!   2. Namespaces (opt-in via `isolate`) — if requested AND the `unshare`
//!      binary works, the command is wrapped in
//!      `unshare --user --map-root-user [--net] --pid --fork`. Otherwise we
//!      fall back to filesystem confinement and mark isolated=false.
//!   3. Resource caps — wall-clock timeout (kills the child) and an output
//!      byte cap (applied when reading the captured files back).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct SandboxSpec {
    pub id: String,
    pub root: PathBuf, // workspaces/sandbox
    pub cmd: Vec<String>,
    pub inputs: Vec<PathBuf>,
    pub net: bool,
    pub isolate: bool,
    pub timeout: Duration,
    pub max_output: usize,
}

pub struct SandboxResult {
    pub exit: i32,
    pub timed_out: bool,
    pub workspace: PathBuf,
    pub stdout: String,
    pub stderr: String,
    pub isolated: bool,
}

pub fn unshare_available() -> bool {
    Command::new("unshare")
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn run(spec: &SandboxSpec) -> Result<SandboxResult, String> {
    if spec.cmd.is_empty() {
        return Err("no command to run".into());
    }
    let workspace = spec.root.join(&spec.id);
    std::fs::create_dir_all(&workspace).map_err(|e| e.to_string())?;
    for input in &spec.inputs {
        if let Some(name) = input.file_name() {
            std::fs::copy(input, workspace.join(name)).map_err(|e| format!("copy input {}: {e}", input.display()))?;
        }
    }

    let use_ns = spec.isolate && unshare_available();
    let out_path = workspace.join(".stdout");
    let err_path = workspace.join(".stderr");

    let mut command = build_command(spec, use_ns);
    command
        .current_dir(&workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::from(File::create(&out_path).map_err(|e| e.to_string())?))
        .stderr(Stdio::from(File::create(&err_path).map_err(|e| e.to_string())?));

    let mut child = command.spawn().map_err(|e| format!("spawn: {e}"))?;
    let start = Instant::now();
    let mut timed_out = false;
    let exit = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {
                if start.elapsed() >= spec.timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break -1;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(_) => break -1,
        }
    };

    let stdout = read_capped(&out_path, spec.max_output);
    let stderr = read_capped(&err_path, spec.max_output);
    Ok(SandboxResult { exit, timed_out, workspace, stdout, stderr, isolated: use_ns })
}

fn build_command(spec: &SandboxSpec, use_ns: bool) -> Command {
    if use_ns {
        let mut c = Command::new("unshare");
        c.arg("--user").arg("--map-root-user").arg("--pid").arg("--fork");
        if !spec.net {
            c.arg("--net");
        }
        c.arg("--").args(&spec.cmd);
        c
    } else {
        let mut c = Command::new(&spec.cmd[0]);
        c.args(&spec.cmd[1..]);
        c
    }
}

fn read_capped(path: &Path, cap: usize) -> String {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut buf = Vec::new();
    let _ = f.by_ref().take(cap as u64 + 1).read_to_end(&mut buf);
    let truncated = buf.len() > cap;
    buf.truncate(cap);
    let mut s = String::from_utf8_lossy(&buf).to_string();
    if truncated {
        s.push_str("\n…[output truncated]");
    }
    s
}

pub fn clean(root: &Path, older_than_hours: Option<u64>) -> Result<usize, String> {
    let mut removed = 0;
    let now = std::time::SystemTime::now();
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let old = match older_than_hours {
            None => true,
            Some(h) => e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| now.duration_since(t).ok())
                .map(|age| age.as_secs() >= h * 3600)
                .unwrap_or(false),
        };
        if old {
            let _ = std::fs::remove_dir_all(&p);
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn spec(root: PathBuf, cmd: &[&str], timeout: u64) -> SandboxSpec {
        SandboxSpec {
            id: "run1".into(),
            root,
            cmd: cmd.iter().map(|s| s.to_string()).collect(),
            inputs: vec![],
            net: false,
            isolate: false, // filesystem confinement — deterministic in CI
            timeout: Duration::from_secs(timeout),
            max_output: 1_000_000,
        }
    }

    #[test]
    fn writes_confined_to_workspace() {
        let r = root("sb-confine");
        let res = run(&spec(r.clone(), &["sh", "-c", "echo hi > escaped.txt"], 10)).unwrap();
        assert_eq!(res.exit, 0);
        assert!(res.workspace.join("escaped.txt").exists());
        // Nothing leaked to the crate dir.
        assert!(!PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("escaped.txt").exists());
    }

    #[test]
    fn timeout_kills() {
        let r = root("sb-timeout");
        let start = Instant::now();
        let res = run(&spec(r, &["sh", "-c", "sleep 60"], 1)).unwrap();
        assert!(res.timed_out);
        assert!(start.elapsed() < Duration::from_secs(6));
    }

    #[test]
    fn output_truncated() {
        let r = root("sb-trunc");
        let mut s = spec(r, &["sh", "-c", "yes x | head -c 100000"], 10);
        s.max_output = 1000;
        let res = run(&s).unwrap();
        assert!(res.stdout.len() <= 1000 + 40);
        assert!(res.stdout.contains("truncated"));
    }

    #[test]
    fn clean_removes_workspaces() {
        let r = root("sb-clean");
        run(&spec(r.clone(), &["true"], 5)).unwrap();
        assert!(clean(&r, None).unwrap() >= 1);
    }
}
