//! Small process helpers shared by SMARTAGENT tools.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct Output {
    pub exit: i32,
    pub timed_out: bool,
    pub text: String,
}

pub fn unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn test_scratch(manifest_dir: &str, name: &str) -> PathBuf {
    let d = Path::new(manifest_dir).join("../../target/test-scratch").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Wait for a child with stdout/stderr drained concurrently.
///
/// This avoids the classic pipe-fill deadlock: a child that writes more than
/// the OS pipe buffer can block before exiting if the parent waits first and
/// reads output only afterward.
pub fn wait_with_timeout(mut child: Child, timeout: Duration) -> Output {
    let stdout = child.stdout.take().map(|mut o| std::thread::spawn(move || {
        let mut s = String::new();
        let _ = o.read_to_string(&mut s);
        s
    }));
    let stderr = child.stderr.take().map(|mut e| std::thread::spawn(move || {
        let mut s = String::new();
        let _ = e.read_to_string(&mut s);
        s
    }));

    let start = Instant::now();
    let (exit, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code().unwrap_or(-1), false),
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break (-1, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break (-1, false),
        }
    };

    Output { exit, timed_out, text: join_output(stdout, stderr) }
}

fn join_output(stdout: Option<JoinHandle<String>>, stderr: Option<JoinHandle<String>>) -> String {
    let mut out = stdout.and_then(|h| h.join().ok()).unwrap_or_default();
    let err = stderr.and_then(|h| h.join().ok()).unwrap_or_default();
    if !err.is_empty() {
        out.push_str("\n[stderr]\n");
        out.push_str(&err);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn child_emitting_more_than_pipe_buffer_completes() {
        let child = Command::new("/bin/sh")
            .arg("-c")
            .arg("i=0; while [ $i -lt 2000 ]; do printf '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\\n'; i=$((i+1)); done")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let out = wait_with_timeout(child, Duration::from_secs(5));
        assert!(!out.timed_out, "large-output child timed out");
        assert_eq!(out.exit, 0);
        assert!(out.text.len() > 64 * 1024, "captured {} bytes", out.text.len());
    }
}
