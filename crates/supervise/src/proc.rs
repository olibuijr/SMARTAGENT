//! Process primitives on std only (Linux): spawn detached, liveness via /proc,
//! terminate via the `kill` coreutil. No libc dependency.

use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};

/// Spawn a detached child: stdout/stderr → `log`, stdin from /dev/null. The
/// handle is dropped so we never wait on it; if this supervisor later exits,
/// the child is reparented to init and keeps running (we track it by pid).
/// Returns the child pid.
pub fn spawn_detached(argv: &[String], workdir: &Path, log: &Path) -> Result<u32, String> {
    let program = argv.first().ok_or("empty command")?;
    let out = File::create(log).map_err(|e| format!("open log {}: {e}", log.display()))?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    let child = Command::new(program)
        .args(&argv[1..])
        .current_dir(workdir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| format!("spawn '{program}': {e}"))?;
    Ok(child.id())
}

/// Is `pid` a live process whose command line still contains `needle`? The
/// needle guard defeats PID reuse — a recycled pid running something else
/// won't match the service's recorded command.
pub fn is_alive(pid: u32, needle: &str) -> bool {
    if pid == 0 {
        return false;
    }
    let cmdline = match std::fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(b) => b,
        Err(_) => return false, // no /proc entry → dead
    };
    // cmdline is NUL-separated argv; join with spaces for a substring check.
    let joined: String = cmdline
        .split(|&b| b == 0)
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    needle.is_empty() || joined.contains(needle)
}

/// Terminate a pid (SIGTERM, then SIGKILL after a grace period). Uses the
/// `kill` binary — std has no signal API and we take no libc dep.
pub fn terminate(pid: u32) {
    if pid == 0 || !Path::new(&format!("/proc/{pid}")).exists() {
        return;
    }
    let _ = Command::new("kill")
        .arg(pid.to_string())
        .stderr(Stdio::null())
        .status();
    // Grace, then hard kill if still alive.
    std::thread::sleep(std::time::Duration::from_millis(600));
    if std::path::Path::new(&format!("/proc/{pid}")).exists() {
        let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_alive_true_for_self() {
        // The test process itself is alive; its cmdline contains the test binary
        // name. An empty needle just checks liveness.
        let me = std::process::id();
        assert!(is_alive(me, ""));
    }

    #[test]
    fn is_alive_false_for_dead_and_pid_zero() {
        assert!(!is_alive(0, ""));
        // A pid that (almost certainly) does not exist.
        assert!(!is_alive(4_000_000_000, ""));
    }

    #[test]
    fn is_alive_needle_guards_against_reuse() {
        // Live pid, but a needle that won't appear in this process's cmdline.
        let me = std::process::id();
        assert!(!is_alive(me, "definitely-not-in-this-cmdline-xyzzy"));
    }

    #[test]
    fn terminate_pid_zero_is_noop() {
        terminate(0); // must not panic or signal anything
    }
}
