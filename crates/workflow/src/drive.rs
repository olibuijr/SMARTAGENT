//! Engine-drives-pi: the workflow engine is the EXECUTOR, the model is a
//! step worker. `workflow drive <name>` runs a definition step by step — each
//! step spawns a fresh headless project-pi that receives ONLY that step's
//! instruction + task context, and the evidence gate stays in Rust BETWEEN
//! steps: the step agent cannot advance the run, the driver does, after
//! validating the agent's final `EVIDENCE:` line. Paradigm inversion of
//! `workflow start` (where the model follows the loop voluntarily): here the
//! harness owns control flow and the model only proposes within one step.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::def::Def;
use crate::run::{now, valid_evidence, Run, Store};

pub struct DriveOpts {
    /// Agent binary — the project `./pi` launcher (a fake script in tests).
    pub pi_bin: String,
    pub step_timeout: Duration,
    /// Extra attempts per step when the agent fails, times out, or returns
    /// missing/trivial evidence.
    pub retries: usize,
    /// Linked tasks-board id (T-n) — passed into every step prompt.
    pub task: String,
    /// Workspace project scoping hint for board/workflow calls in steps.
    pub project: Option<String>,
}

pub fn drive(store: &Store, root: &Path, d: &Def, o: &DriveOpts) -> Result<String, String> {
    // Fork-bomb guard: a driven step must never drive workflows itself.
    if std::env::var("SMARTAGENT_DRIVE").map(|v| v == "1").unwrap_or(false) {
        return Err("nested `workflow drive` refused (SMARTAGENT_DRIVE=1 — a driven step cannot drive)".into());
    }
    if d.steps.is_empty() {
        return Err(format!("workflow '{}' has no steps", d.name));
    }
    let mut r = Run {
        id: store.next_id()?,
        wf: d.name.clone(),
        task: o.task.clone(),
        step: 0,
        status: "running".into(),
        started: now(),
        updated: now(),
        evidence: Vec::new(),
    };
    store.put(&r)?;
    let logs = root.join(".scratch").join("workflow-drive").join(&r.id);
    std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let mut report = vec![format!(
        "{} · driving '{}' ({} steps, {}s/step, {} retr{}) — step logs: {}",
        r.id, d.name, d.steps.len(), o.step_timeout.as_secs(), o.retries,
        if o.retries == 1 { "y" } else { "ies" }, logs.display()
    )];

    while r.status == "running" {
        let idx = r.step;
        let s = &d.steps[idx];
        let prompt = step_prompt(d, &r, o);
        let mut verdict: Result<String, String> = Err("no attempt ran".into());
        let mut attempts = 0;
        while attempts <= o.retries {
            attempts += 1;
            let (exit, timed_out, out) = spawn_step(o, &prompt);
            let _ = std::fs::write(logs.join(format!("step{}-{}-attempt{attempts}.log", idx + 1, s.name)), &out);
            verdict = if timed_out {
                Err(format!("timed out after {}s", o.step_timeout.as_secs()))
            } else if exit != 0 {
                Err(format!("step agent exited {exit}"))
            } else {
                extract_evidence(&out).and_then(|e| valid_evidence(&e))
            };
            if verdict.is_ok() {
                break;
            }
        }
        match verdict {
            Ok(ev) => {
                r.evidence.push(format!("{}: {}", s.name, ev));
                r.updated = now();
                if r.step + 1 >= d.steps.len() {
                    r.status = "done".into();
                } else {
                    r.step += 1;
                }
                store.put(&r)?;
                report.push(format!("  {}/{} {} ✓ [{}x] {}", idx + 1, d.steps.len(), s.name, attempts, ev));
            }
            Err(why) => {
                r.status = "aborted".into();
                r.updated = now();
                store.put(&r)?;
                report.push(format!("  {}/{} {} ✗ after {attempts} attempt(s) — {why}", idx + 1, d.steps.len(), s.name));
                report.push(format!("{} ABORTED — fix and re-drive, or continue manually: workflow start {}", r.id, d.name));
                return Err(report.join("\n"));
            }
        }
    }
    let task_hint = if r.task.is_empty() { String::new() } else { format!(" — now: tasks move {} review (or done)", r.task) };
    report.push(format!("{} complete: all {} steps evidenced{task_hint}", r.id, d.steps.len()));
    Ok(report.join("\n"))
}

fn step_prompt(d: &Def, r: &Run, o: &DriveOpts) -> String {
    let s = &d.steps[r.step];
    let proj = o
        .project
        .as_deref()
        .map(|p| format!(" Use project '{p}' on every tasks/board call."))
        .unwrap_or_default();
    let task = if r.task.is_empty() {
        String::new()
    } else {
        format!("Board task: {} (`tasks show {}` for criteria; check them off as they ACTUALLY pass).{proj}\n", r.task, r.task)
    };
    format!(
        "You are executing exactly ONE step of the '{}' workflow (run {}, step {}/{}: {}).\n\
        {}use: {} — the primary tool for this step (a skill only if one by this name exists).\n\
        expect: {}\n\
        {}\n\n\
        Rules: do ONLY this step. Do NOT advance, abort, or drive the workflow run yourself. \
        Do NOT pull or start other tasks.\n\
        When the step is done and verified, end your reply with ONE final line:\n\
        EVIDENCE: <what you verified and how — quote the actual probe result>",
        d.name, r.id, r.step + 1, d.steps.len(), s.name, task, s.skill, s.expect, s.body.trim()
    )
}

fn spawn_step(o: &DriveOpts, prompt: &str) -> (i32, bool, String) {
    let mut cmd = Command::new(&o.pi_bin);
    cmd.arg("--thinking").arg("low").arg("-p").arg(prompt);
    cmd.env("SMARTAGENT_DRIVE", "1");
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (-1, false, format!("spawn failed: {e}")),
    };
    let out = procutil::wait_with_timeout(child, o.step_timeout);
    (out.exit, out.timed_out, out.text)
}

/// The step agent's LAST `EVIDENCE:` line is its claim; everything else is
/// working noise. Missing line = the step did not complete its contract.
fn extract_evidence(out: &str) -> Result<String, String> {
    out.lines()
        .rev()
        .find_map(|l| l.trim_start().strip_prefix("EVIDENCE:"))
        .map(|e| e.trim().to_string())
        .ok_or_else(|| "no `EVIDENCE:` line in step output".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::def::Step;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake_agent(dir: &Path, body: &str) -> String {
        let p = dir.join("fake-pi.sh");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.display().to_string()
    }

    fn mini_def() -> Def {
        Def {
            name: "mini".into(),
            steps: vec![
                Step { name: "observe".into(), skill: "memory".into(), expect: "context".into(), body: "look".into() },
                Step { name: "verify".into(), skill: "evals".into(), expect: "probe".into(), body: "check".into() },
            ],
            ..Def::default()
        }
    }

    // One test fn: scenarios share process env (nested guard) — parallel test
    // threads must not race on SMARTAGENT_DRIVE.
    #[test]
    fn drive_engine_end_to_end() {
        let root = scratch("wf-drive");
        let store = Store::open(&root.join("wf.semdb")).unwrap();

        // 1. Good agent: both steps complete, evidence recorded, run done.
        let good = fake_agent(&root, "echo working…; echo 'EVIDENCE: probe returned expected value 42'");
        let o = DriveOpts { pi_bin: good, step_timeout: Duration::from_secs(10), retries: 0, task: "T-9".into(), project: None };
        let out = drive(&store, &root, &mini_def(), &o).unwrap();
        assert!(out.contains("complete: all 2 steps evidenced") && out.contains("tasks move T-9"), "{out}");
        let r = store.get("W-1").unwrap();
        assert_eq!(r.status, "done");
        assert_eq!(r.evidence.len(), 2);
        assert!(r.evidence[0].contains("value 42"));
        // step logs captured
        assert!(root.join(".scratch/workflow-drive/W-1/step1-observe-attempt1.log").exists());

        // 2. Trivial evidence: retried then aborted, run marked aborted.
        let lazy = fake_agent(&root, "echo 'EVIDENCE: done'");
        let o2 = DriveOpts { pi_bin: lazy, step_timeout: Duration::from_secs(10), retries: 1, task: String::new(), project: None };
        let err = drive(&store, &root, &mini_def(), &o2).unwrap_err();
        assert!(err.contains("ABORTED") && err.contains("after 2 attempt(s)"), "{err}");
        assert_eq!(store.get("W-2").unwrap().status, "aborted");

        // 3. Nested-drive guard. (set_var is unsafe in edition 2024; this is
        // the only test touching env and scenarios run serially in this fn.)
        unsafe { std::env::set_var("SMARTAGENT_DRIVE", "1") };
        let e = drive(&store, &root, &mini_def(), &o2).unwrap_err();
        unsafe { std::env::remove_var("SMARTAGENT_DRIVE") };
        assert!(e.contains("nested"), "{e}");
    }
}
