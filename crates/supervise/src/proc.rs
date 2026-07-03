//! Process primitives on std only (Linux): spawn detached, liveness via /proc,
//! terminate via the `kill` coreutil. No libc dependency.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Scan /proc for the lowest live pid whose argv contains `needle`, excluding
/// this process. Lets the supervisor ADOPT an already-running service instead
/// of spawning a duplicate that would exit on the service's own single-instance
/// guard (the gateway's "already running" socket check) — the churn source:
/// the duplicate exits, the store tracks its dead pid, watch respawns, loop.
pub fn pid_by_needle(needle: &str) -> Option<u32> {
    if needle.is_empty() {
        return None;
    }
    // The needle is `<binary> <subcommand>` (e.g. "gateway serve"). Match ONLY
    // real service processes: argv[0]'s basename must equal the binary word.
    // Without this, ANY process whose cmdline merely CONTAINS "gateway serve"
    // — a shell, a grep, this supervisor's own probe — falsely matches, so
    // `alive()` adopts a bogus pid and the real service never starts.
    let binary = needle.split_whitespace().next().unwrap_or(needle);
    let me = std::process::id();
    let mut found: Vec<u32> = Vec::new();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        if let Ok(bytes) = std::fs::read(format!("/proc/{pid}/cmdline")) {
            let argv: Vec<String> = bytes
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect();
            let Some(arg0) = argv.first() else { continue };
            let base = arg0.rsplit('/').next().unwrap_or(arg0);
            if base == binary && argv.join(" ").contains(needle) {
                found.push(pid);
            }
        }
    }
    found.into_iter().min()
}

/// Spawn a detached child: stdout/stderr appended to `log`, stdin from
/// /dev/null. Appending preserves crash-loop forensics across restarts. The
/// handle is dropped so we never wait on it; if this supervisor later exits,
/// the child is reparented to init and keeps running (we track it by pid).
/// Returns the child pid.
pub fn spawn_detached(argv: &[String], workdir: &Path, log: &Path) -> Result<u32, String> {
    let program = argv.first().ok_or("empty command")?;
    let mut out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| format!("open log {}: {e}", log.display()))?;
    let _ = writeln!(
        out,
        "\n[supervise] spawn at {} argv={}",
        unix_secs(),
        argv.join(" ")
    );
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

pub fn unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
    fn spawn_detached_appends_repeated_startup_failures() {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch/supervise-append-log");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let script = scratch.join("fail.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho out-$1\necho err-$1 >&2\nexit 7\n",
        )
        .unwrap();
        let _ = Command::new("chmod").arg("+x").arg(&script).status();
        let log = scratch.join("svc.log");
        for i in 1..=3 {
            let argv = vec![script.to_string_lossy().to_string(), i.to_string()];
            let pid = spawn_detached(&argv, &scratch, &log).unwrap();
            for _ in 0..20 {
                if !Path::new(&format!("/proc/{pid}")).exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("out-1"), "{text}");
        assert!(text.contains("err-1"), "{text}");
        assert!(text.contains("out-2"), "{text}");
        assert!(text.contains("err-2"), "{text}");
        assert!(text.contains("out-3"), "{text}");
        assert!(text.contains("err-3"), "{text}");
        assert!(text.matches("[supervise] spawn at").count() >= 3, "{text}");
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
