//! CLI: list / show / start / step / advance / abort / runs / statusline.
//! The engine holds the state machine deterministically; the agent supplies
//! judgment. `advance` refuses trivial evidence — the verification mandate.

use std::path::{Path, PathBuf};
use std::time::Duration;

use httpc::args::flag;

use crate::def::{self, Def};
use crate::drive;
use crate::run::{now, valid_evidence, Run, Store};

fn db_path(args: &[String]) -> Result<PathBuf, String> {
    // --project <name>: run state lives with that workspace repo, matching the
    // per-repo tasks board its runs link to (T-n ids are board-scoped).
    if let Some(p) = flag(args, "--project") {
        return semdb::workspace::data_path(&p, "workflow.semdb");
    }
    Ok(flag(args, "--db")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/workflow.semdb")))
}

fn root(args: &[String]) -> PathBuf {
    flag(args, "--root")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn find_def(root: &Path, name: &str) -> Result<Def, String> {
    def::discover(root)?
        .into_iter()
        .find(|d| d.name == name)
        .ok_or_else(|| format!("no workflow '{name}' (workflow list)"))
}

/// Print one step as agent instructions, including which skill to load —
/// the PAI "different skill per phase" routing, as data.
fn render_step(d: &Def, r: &Run) -> String {
    let s = &d.steps[r.step];
    let mut out = vec![format!(
        "{} · {} — step {}/{}: {}",
        r.id,
        d.name,
        r.step + 1,
        d.steps.len(),
        s.name
    )];
    if !r.task.is_empty() {
        out.push(format!("task: {} (tasks show {})", r.task, r.task));
    }
    // `skill:` values usually name a TOOL (memory, tasks, sandbox, …); only
    // load via the skills tool if a skill by that name actually exists.
    if !s.skill.is_empty() {
        out.push(format!("use: {} — the tool for this step (a skill only if one by this name exists; don't chase missing skills)", s.skill));
    }
    if !s.expect.is_empty() {
        out.push(format!("expect: {}", s.expect));
    }
    if !s.body.is_empty() {
        out.push(s.body.clone());
    }
    out.push(format!(
        "when done: workflow advance --run {} --evidence '<what you verified>'",
        r.id
    ));
    out.join("\n")
}

fn task_owner_state(task: &str) -> Option<(String, String)> {
    if task.is_empty() {
        return None;
    }
    let out = std::process::Command::new("target/release/tasks")
        .args(["show", task])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut col = String::new();
    let mut owner = String::new();
    if let Some(first) = text.lines().next() {
        let parts: Vec<&str> = first.split_whitespace().collect();
        if parts.len() >= 3 {
            col = parts[2].to_string();
        }
    }
    for l in text.lines() {
        if let Some(rest) = l.strip_prefix("owner: ") {
            owner = rest.trim().to_string();
        }
    }
    Some((col, owner))
}

fn current_owner() -> String {
    std::env::var("SMARTAGENT_TASK_OWNER")
        .or_else(|_| std::env::var("SMARTAGENT_GATEWAY_AGENT"))
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "agent".into())
}

fn resolve_run(store: &Store, args: &[String]) -> Result<Run, String> {
    match flag(args, "--run") {
        Some(id) => store.get(&id),
        None => store
            .latest_running()?
            .ok_or_else(|| "no running workflow (workflow start <name>)".into()),
    }
}

fn post_workflow_telegram(event: &str, run: &Run, detail: &str) {
    if std::env::var("SMARTAGENT_WORKFLOW_TELEGRAM_DISABLE").is_ok() {
        return;
    }
    let task = if run.task.is_empty() {
        "no task"
    } else {
        &run.task
    };
    let text = format!(
        "*Workflow {event}*
Run: `{}`
Task: `{}`
Workflow: `{}`
Step: `{}`
{}",
        run.id,
        task,
        run.wf,
        run.step + 1,
        detail
    );
    let _ = std::process::Command::new("target/release/telegram")
        .args(["broadcast", "--kind", "workflow", "--text", &text])
        .output();
}

pub fn run(args: &[String]) -> Result<String, String> {
    let store = Store::open(&db_path(args)?)?;
    match args.first().map(String::as_str) {
        Some("list") => {
            let defs = def::discover(&root(args))?;
            if defs.is_empty() {
                return Ok("no workflows (add workflows/*.md or skills/*/Workflows/*.md)".into());
            }
            Ok(defs
                .iter()
                .map(|d| {
                    format!(
                        "{}\t{} steps\t{}{}",
                        d.name,
                        d.steps.len(),
                        d.description,
                        if d.use_when.is_empty() {
                            String::new()
                        } else {
                            format!(" — USE WHEN {}", d.use_when)
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("show") => {
            let d = find_def(
                &root(args),
                args.get(1).ok_or("usage: workflow show <name>")?,
            )?;
            let mut out = vec![format!(
                "{} — {} ({} steps)",
                d.name,
                d.description,
                d.steps.len()
            )];
            for (i, s) in d.steps.iter().enumerate() {
                out.push(format!(
                    "  {}. {}{}{}",
                    i + 1,
                    s.name,
                    if s.skill.is_empty() {
                        String::new()
                    } else {
                        format!(" [skill: {}]", s.skill)
                    },
                    if s.expect.is_empty() {
                        String::new()
                    } else {
                        format!(" → {}", s.expect)
                    }
                ));
            }
            Ok(out.join("\n"))
        }
        Some("start") => {
            let name = args
                .get(1)
                .ok_or("usage: workflow start <name> [--task T-1]")?;
            let d = find_def(&root(args), name)?;
            // Task lock: one running run per task. Multi-agent fleets race to
            // pull the same top task; the first run claims it, later starts
            // are refused loudly so the losing agent picks other work.
            let task = flag(args, "--task").unwrap_or_default();
            if !task.is_empty() {
                if let Some(existing) = store
                    .all()?
                    .into_iter()
                    .find(|r| r.status == "running" && r.task == task)
                {
                    return Err(format!(
                        "task {task} already has running run {} (wf {}) — another agent owns it; pull a different task",
                        existing.id, existing.wf
                    ));
                }
                if let Some((col, owner)) = task_owner_state(&task) {
                    let me = current_owner();
                    if col != "doing" || (!owner.is_empty() && owner != me) {
                        return Err(format!(
                            "task {task} is {col}{}; start its workflow only after you own it in doing",
                            if owner.is_empty() { String::new() } else { format!(" owned by @{owner}") }
                        ));
                    }
                }
            }
            let r = Run {
                id: store.next_id()?,
                wf: d.name.clone(),
                task,
                step: 0,
                status: "running".into(),
                started: now(),
                updated: now(),
                evidence: Vec::new(),
            };
            store.put(&r)?;
            post_workflow_telegram("started", &r, "Next: observe the first step.");
            Ok(render_step(&d, &r))
        }
        Some("step") => {
            let r = resolve_run(&store, args)?;
            if r.status != "running" {
                return Err(format!("{} is {}", r.id, r.status));
            }
            let d = find_def(&root(args), &r.wf)?;
            Ok(render_step(&d, &r))
        }
        Some("advance") => {
            let mut r = resolve_run(&store, args)?;
            if r.status != "running" {
                return Err(format!("{} is {}", r.id, r.status));
            }
            let d = find_def(&root(args), &r.wf)?;
            // Verification mandate: no advance without real evidence.
            let ev = valid_evidence(&flag(args, "--evidence").unwrap_or_default())?;
            r.evidence.push(format!("{}: {}", d.steps[r.step].name, ev));
            r.updated = now();
            if r.step + 1 >= d.steps.len() {
                r.status = "done".into();
                store.put(&r)?;
                post_workflow_telegram("complete", &r, &format!("Evidence: {ev}"));
                let task_hint = if r.task.is_empty() {
                    String::new()
                } else {
                    format!(" — now: tasks move {} review (or done)", r.task)
                };
                Ok(format!(
                    "{} complete: all {} steps evidenced{}",
                    r.id,
                    d.steps.len(),
                    task_hint
                ))
            } else {
                r.step += 1;
                store.put(&r)?;
                post_workflow_telegram("advanced", &r, &format!("Evidence: {ev}"));
                Ok(render_step(&d, &r))
            }
        }
        Some("drive") => {
            // Engine-drives-pi: the harness owns control flow; each step is a
            // fresh headless pi run, evidence-gated in Rust between steps.
            let name = args.get(1).filter(|a| !a.starts_with('-'))
                .ok_or("usage: workflow drive <name> --task T-n [--project P] [--step-timeout 300] [--retries 1] [--pi ./pi]")?;
            let d = find_def(&root(args), name)?;
            let o = drive::DriveOpts {
                pi_bin: flag(args, "--pi").unwrap_or_else(|| "./pi".into()),
                step_timeout: Duration::from_secs(
                    flag(args, "--step-timeout")
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(300),
                ),
                retries: flag(args, "--retries")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1),
                task: flag(args, "--task").unwrap_or_default(),
                project: flag(args, "--project"),
            };
            drive::drive(&store, &root(args), &d, &o)
        }
        Some("abort") => {
            let mut r = resolve_run(&store, args)?;
            r.status = "aborted".into();
            r.updated = now();
            store.put(&r)?;
            post_workflow_telegram("aborted", &r, "Run was aborted.");
            Ok(format!("{} aborted at step {}", r.id, r.step + 1))
        }
        Some("runs") => {
            let rs = store.all()?;
            if rs.is_empty() {
                return Ok("no runs".into());
            }
            // --live: only runs that are actually being worked — status running
            // AND (no linked task, or the task is in doing/review on the board).
            // Sessions that die mid-run leave "running" rows forever; without
            // this cross-check UIs show phantom activity while the board says
            // nothing is in doing.
            let live_only = args.iter().any(|a| a == "--live");
            let board: Option<Vec<tasks::store::Task>> = if live_only {
                let tdb = flag(args, "--tasks-db").unwrap_or_else(|| "data/tasks.semdb".into());
                tasks::store::Store::open(std::path::Path::new(&tdb))
                    .and_then(|s| s.all())
                    .ok()
            } else {
                None
            };
            let rows: Vec<String> = rs
                .iter()
                .filter(|r| {
                    if !live_only {
                        return true;
                    }
                    if r.status != "running" {
                        return false;
                    }
                    if r.task.is_empty() {
                        return true;
                    }
                    board
                        .as_ref()
                        .map(|b| {
                            b.iter().any(|t| {
                                t.id == r.task && matches!(t.col.as_str(), "doing" | "review")
                            })
                        })
                        .unwrap_or(true)
                })
                .map(|r| {
                    format!(
                        "{}\t{}\t{}\tstep {}{}",
                        r.id,
                        r.wf,
                        r.status,
                        r.step + 1,
                        if r.task.is_empty() {
                            String::new()
                        } else {
                            format!("\t{}", r.task)
                        }
                    )
                })
                .collect();
            if rows.is_empty() {
                return Ok(if live_only {
                    "no live runs".into()
                } else {
                    "no runs".into()
                });
            }
            Ok(rows.join("\n"))
        }
        Some("statusline") => {
            // `level|text`: current run + step; warn when a run has stalled >1d.
            match store.latest_running()? {
                Some(r) => {
                    let steps = find_def(&root(args), &r.wf)
                        .map(|d| d.steps.len())
                        .unwrap_or(0);
                    let stalled = now() - r.updated > 86_400;
                    let level = if stalled { "warn" } else { "ok" };
                    let stale = if stalled { " (stalled)" } else { "" };
                    Ok(format!(
                        "{level}|▶ {} {}/{}{}",
                        r.wf,
                        r.step + 1,
                        steps.max(r.step + 1),
                        stale
                    ))
                }
                None => Ok("ok|▶ idle".into()),
            }
        }
        _ => Ok(HELP.trim().into()),
    }
}

const HELP: &str = r#"
workflow — markdown-defined process engine (PAI pattern: skill per step,
evidence-gated advancement). Definitions: workflows/*.md + skills/*/Workflows/*.md.

USAGE:
  workflow list                          discover workflows (name, steps, USE WHEN)
  workflow show <name>                   step outline with per-step skills
  workflow start <name> [--task T-1]     begin a run (prints step 1 instructions)
  workflow step [--run W-1]              current step (defaults to latest running)
  workflow advance --evidence '<proof>' [--run W-1]   complete step (evidence REQUIRED)
  workflow drive <name> --task T-n [--project P] [--step-timeout 300] [--retries 1]
                                         ENGINE-DRIVEN: each step runs in a fresh
                                         headless pi; the driver validates each
                                         step's final `EVIDENCE:` line and advances
                                         (or aborts) — the model never self-advances
  workflow runs                          all runs + status
  workflow abort [--run W-1]

Run state: data/workflow.semdb; --project <name> scopes runs to that workspace
repo (workspaces/<name>/.smartagent/workflow.semdb, pairs with tasks --project).
A step names the skill/tool to use — load it, do the step, verify, then
advance with what you verified as evidence.
"#;
