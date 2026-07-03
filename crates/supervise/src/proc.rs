//! Process primitives on std only (Linux): spawn detached, liveness via /proc,
//! terminate via the `kill` coreutil. No libc dependency.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};

/// Scan /proc for the lowest live pid matching this service in this repo
/// instance, excluding this process. Lets the supervisor ADOPT an
/// already-running service instead of spawning a duplicate, but never adopts a
/// same-named service from another SMARTAGENT checkout.
pub fn pid_by_service(needle: &str, argv: &[String], workdir: &Path) -> Option<u32> {
    if needle.is_empty() || argv.is_empty() {
        return None;
    }
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
        if service_process_matches(pid, needle, argv, workdir) {
            found.push(pid);
        }
    }
    found.into_iter().min()
}

fn service_process_matches(pid: u32, needle: &str, argv: &[String], workdir: &Path) -> bool {
    let Ok(bytes) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let mut actual: Vec<String> = bytes
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    // Some programs (chromium) rewrite their cmdline as ONE space-separated
    // string (process title) — the NUL split then yields a single field and
    // argv[0] comparison can never match. Re-split on whitespace.
    if actual.len() == 1 && actual[0].contains(' ') {
        actual = actual[0].split_whitespace().map(str::to_string).collect();
    }
    let Some(actual_arg0) = actual.first() else {
        return false;
    };
    if !actual.join(" ").contains(needle) {
        return false;
    }
    let expected_arg0 = resolve_program(&argv[0], workdir);
    let actual_arg0 = resolve_program(actual_arg0, workdir);
    // Exact path match, or basename match: launcher wrappers exec the real
    // binary under a different path (Arch: /usr/bin/chromium is a script that
    // execs /usr/lib/chromium/chromium), which made the watch declare a
    // healthy service dead and spawn doomed duplicates forever. The needle
    // and cwd checks still scope the match to OUR instance.
    if actual_arg0 != expected_arg0 && actual_arg0.file_name() != expected_arg0.file_name() {
        return false;
    }
    let Ok(cwd) = std::fs::read_link(format!("/proc/{pid}/cwd")) else {
        return false;
    };
    same_path(&cwd, workdir)
}

fn resolve_program(program: &str, workdir: &Path) -> PathBuf {
    let path = Path::new(program);
    let full = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workdir.join(path)
    };
    std::fs::canonicalize(&full).unwrap_or(full)
}

fn same_path(left: &Path, right: &Path) -> bool {
    let l = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let r = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    l == r
}

static DETACHED_CHILDREN: OnceLock<Mutex<Vec<Child>>> = OnceLock::new();

fn detached_children() -> &'static Mutex<Vec<Child>> {
    DETACHED_CHILDREN.get_or_init(|| Mutex::new(Vec::new()))
}

/// Reap any exited children spawned by this supervisor process. Long-running
/// `supervise watch` must call this opportunistically; otherwise quick-crashing
/// services become zombies because std's `Child` only reaps on `wait`/`try_wait`.
pub fn reap_detached_children() -> usize {
    let mut reaped = 0usize;
    let mut children = detached_children()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut live = Vec::with_capacity(children.len());
    for mut child in children.drain(..) {
        match child.try_wait() {
            Ok(Some(_)) => reaped += 1,
            Ok(None) | Err(_) => live.push(child),
        }
    }
    *children = live;
    reaped
}

/// Spawn a detached child: stdout/stderr appended to `log`, stdin from
/// /dev/null. Appending preserves crash-loop forensics across restarts. The
/// Child handle is retained in this process and reaped by `reap_detached_children`
/// so long-running watch loops do not leak zombies; if this supervisor exits,
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
    let pid = child.id();
    detached_children()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(child);
    Ok(pid)
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
pub fn is_alive(pid: u32, needle: &str, argv: &[String], workdir: &Path) -> bool {
    if pid == 0 {
        return false;
    }
    if needle.is_empty() {
        return Path::new(&format!("/proc/{pid}")).exists();
    }
    service_process_matches(pid, needle, argv, workdir)
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
        assert!(is_alive(me, "", &[], Path::new(".")));
    }

    #[test]
    fn spawn_detached_appends_repeated_startup_failures() {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch/supervise-append-log");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let script = scratch.join("fail.sh");
        std::fs::write(&script, "#!/bin/sh\necho out-$1\necho err-$1 >&2\nexit 7\n").unwrap();
        let _ = Command::new("chmod").arg("+x").arg(&script).status();
        let log = scratch.join("svc.log");
        for i in 1..=3 {
            let argv = vec![script.to_string_lossy().to_string(), i.to_string()];
            let _pid = spawn_detached(&argv, &scratch, &log).unwrap();
        }
        let mut reaped = 0;
        for _ in 0..20 {
            reaped += reap_detached_children();
            if reaped >= 3 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            reaped >= 3,
            "spawned quick-exit children must be reaped, got {reaped}"
        );
        assert_eq!(
            reap_detached_children(),
            0,
            "reaping should drain exited child handles"
        );
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
        assert!(!is_alive(0, "", &[], Path::new(".")));
        // A pid that (almost certainly) does not exist.
        assert!(!is_alive(4_000_000_000, "", &[], Path::new(".")));
    }

    #[test]
    fn is_alive_needle_guards_against_reuse() {
        // Live pid, but a needle that won't appear in this process's cmdline.
        let me = std::process::id();
        assert!(!is_alive(
            me,
            "definitely-not-in-this-cmdline-xyzzy",
            &["definitely-not-in-this-cmdline-xyzzy".into()],
            Path::new(".")
        ));
    }

    #[test]
    fn pid_by_service_scopes_same_binary_to_workdir() {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch/supervise-instance-scope");
        let one = scratch.join("one");
        let two = scratch.join("two");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let mut child_one = Command::new("/bin/sleep")
            .arg("77")
            .current_dir(&one)
            .spawn()
            .unwrap();
        let mut child_two = Command::new("/bin/sleep")
            .arg("77")
            .current_dir(&two)
            .spawn()
            .unwrap();
        let argv = vec!["/bin/sleep".to_string(), "77".to_string()];
        let found_one = pid_by_service("sleep 77", &argv, &one);
        let found_two = pid_by_service("sleep 77", &argv, &two);
        let _ = child_one.kill();
        let _ = child_two.kill();
        let _ = child_one.wait();
        let _ = child_two.wait();
        assert_eq!(found_one, Some(child_one.id()));
        assert_eq!(found_two, Some(child_two.id()));
    }

    #[test]
    fn terminate_pid_zero_is_noop() {
        terminate(0); // must not panic or signal anything
    }
}
