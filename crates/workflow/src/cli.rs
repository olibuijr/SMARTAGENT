//! CLI: list / show / start / step / advance / abort / runs / statusline.
//! The engine holds the state machine deterministically; the agent supplies
//! judgment. `advance` refuses trivial evidence — the verification mandate.

use std::path::{Path, PathBuf};

use httpc::args::flag;

use crate::def::{self, Def};
use crate::run::{now, Run, Store};

fn db_path(args: &[String]) -> Result<PathBuf, String> {
    // --project <name>: run state lives with that workspace repo, matching the
    // per-repo tasks board its runs link to (T-n ids are board-scoped).
    if let Some(p) = flag(args, "--project") {
        return semdb::workspace::data_path(&p, "workflow.semdb");
    }
    Ok(flag(args, "--db").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data/workflow.semdb")))
}

fn root(args: &[String]) -> PathBuf {
    flag(args, "--root").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
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
    let mut out = vec![format!("{} · {} — step {}/{}: {}", r.id, d.name, r.step + 1, d.steps.len(), s.name)];
    if !r.task.is_empty() { out.push(format!("task: {} (tasks show {})", r.task, r.task)); }
    if !s.skill.is_empty() { out.push(format!("skill: {} — use this tool/skill for this step", s.skill)); }
    if !s.expect.is_empty() { out.push(format!("expect: {}", s.expect)); }
    if !s.body.is_empty() { out.push(s.body.clone()); }
    out.push(format!("when done: workflow advance --run {} --evidence '<what you verified>'", r.id));
    out.join("\n")
}

fn resolve_run(store: &Store, args: &[String]) -> Result<Run, String> {
    match flag(args, "--run") {
        Some(id) => store.get(&id),
        None => store.latest_running()?.ok_or_else(|| "no running workflow (workflow start <name>)".into()),
    }
}

pub fn run(args: &[String]) -> Result<String, String> {
    let store = Store::open(&db_path(args)?)?;
    match args.first().map(String::as_str) {
        Some("list") => {
            let defs = def::discover(&root(args))?;
            if defs.is_empty() { return Ok("no workflows (add workflows/*.md or skills/*/Workflows/*.md)".into()); }
            Ok(defs
                .iter()
                .map(|d| format!("{}\t{} steps\t{}{}", d.name, d.steps.len(), d.description, if d.use_when.is_empty() { String::new() } else { format!(" — USE WHEN {}", d.use_when) }))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("show") => {
            let d = find_def(&root(args), args.get(1).ok_or("usage: workflow show <name>")?)?;
            let mut out = vec![format!("{} — {} ({} steps)", d.name, d.description, d.steps.len())];
            for (i, s) in d.steps.iter().enumerate() {
                out.push(format!("  {}. {}{}{}", i + 1, s.name,
                    if s.skill.is_empty() { String::new() } else { format!(" [skill: {}]", s.skill) },
                    if s.expect.is_empty() { String::new() } else { format!(" → {}", s.expect) }));
            }
            Ok(out.join("\n"))
        }
        Some("start") => {
            let name = args.get(1).ok_or("usage: workflow start <name> [--task T-1]")?;
            let d = find_def(&root(args), name)?;
            let r = Run {
                id: store.next_id()?,
                wf: d.name.clone(),
                task: flag(args, "--task").unwrap_or_default(),
                step: 0,
                status: "running".into(),
                started: now(),
                updated: now(),
                evidence: Vec::new(),
            };
            store.put(&r)?;
            Ok(render_step(&d, &r))
        }
        Some("step") => {
            let r = resolve_run(&store, args)?;
            if r.status != "running" { return Err(format!("{} is {}", r.id, r.status)); }
            let d = find_def(&root(args), &r.wf)?;
            Ok(render_step(&d, &r))
        }
        Some("advance") => {
            let mut r = resolve_run(&store, args)?;
            if r.status != "running" { return Err(format!("{} is {}", r.id, r.status)); }
            let d = find_def(&root(args), &r.wf)?;
            // Verification mandate: no advance without real evidence.
            let ev = flag(args, "--evidence").unwrap_or_default();
            let trivial = ["done", "ok", "works", "finished", "complete"];
            if ev.trim().chars().count() < 10 || trivial.contains(&ev.trim().to_lowercase().as_str()) {
                return Err("evidence required: describe WHAT you verified and HOW (≥10 chars; 'done'/'ok' rejected)".into());
            }
            r.evidence.push(format!("{}: {}", d.steps[r.step].name, ev.trim()));
            r.updated = now();
            if r.step + 1 >= d.steps.len() {
                r.status = "done".into();
                store.put(&r)?;
                let task_hint = if r.task.is_empty() { String::new() } else { format!(" — now: tasks move {} review (or done)", r.task) };
                Ok(format!("{} complete: all {} steps evidenced{}", r.id, d.steps.len(), task_hint))
            } else {
                r.step += 1;
                store.put(&r)?;
                Ok(render_step(&d, &r))
            }
        }
        Some("abort") => {
            let mut r = resolve_run(&store, args)?;
            r.status = "aborted".into();
            r.updated = now();
            store.put(&r)?;
            Ok(format!("{} aborted at step {}", r.id, r.step + 1))
        }
        Some("runs") => {
            let rs = store.all()?;
            if rs.is_empty() { return Ok("no runs".into()); }
            Ok(rs
                .iter()
                .map(|r| format!("{}\t{}\t{}\tstep {}{}", r.id, r.wf, r.status, r.step + 1, if r.task.is_empty() { String::new() } else { format!("\t{}", r.task) }))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Some("statusline") => {
            // `level|text`: current run + step; warn when a run has stalled >1d.
            match store.latest_running()? {
                Some(r) => {
                    let steps = find_def(&root(args), &r.wf).map(|d| d.steps.len()).unwrap_or(0);
                    let stalled = now() - r.updated > 86_400;
                    let level = if stalled { "warn" } else { "ok" };
                    let stale = if stalled { " (stalled)" } else { "" };
                    Ok(format!("{level}|▶ {} {}/{}{}", r.wf, r.step + 1, steps.max(r.step + 1), stale))
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
  workflow runs                          all runs + status
  workflow abort [--run W-1]

Run state: data/workflow.semdb; --project <name> scopes runs to that workspace
repo (workspaces/<name>/.smartagent/workflow.semdb, pairs with tasks --project).
A step names the skill/tool to use — load it, do the step, verify, then
advance with what you verified as evidence.
"#;
