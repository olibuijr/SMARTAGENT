//! CLI: add / todo / board / list / show / move / next / crit / block / done /
//! rm / wip / metrics / statusline. Kanban policies (WIP limits, pull-based
//! `next`, criteria-gated done) are enforced HERE, deterministically — not in
//! prompts.

use std::path::PathBuf;

use httpc::args::{flag, has};

use crate::board;
use crate::store::{now, Store, Task, COLUMNS};

fn db_path(args: &[String]) -> PathBuf {
    flag(args, "--db").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data/tasks.semdb"))
}

fn valid_col(c: &str) -> Result<(), String> {
    if COLUMNS.contains(&c) { Ok(()) } else { Err(format!("unknown column '{c}' ({})", COLUMNS.join("|"))) }
}

pub fn run(args: &[String]) -> Result<String, String> {
    let store = Store::open(&db_path(args))?;
    match args.first().map(String::as_str) {
        Some("add") | Some("todo") => {
            let quick = args[0] == "todo"; // todo = frictionless capture into backlog
            let title = args.get(1).filter(|t| !t.starts_with("--")).ok_or("usage: tasks add '<title>' [--prio p1|p2|p3] [--col C] [--tags a,b] [--criteria 'a;b']")?;
            let prio = flag(args, "--prio").unwrap_or_else(|| if quick { "p3".into() } else { "p2".into() });
            if !matches!(prio.as_str(), "p1" | "p2" | "p3") {
                return Err("prio must be p1|p2|p3".into());
            }
            let col = if quick { "backlog".to_string() } else { flag(args, "--col").unwrap_or_else(|| "backlog".into()) };
            valid_col(&col)?;
            let t = Task {
                id: store.next_id()?,
                title: title.clone(),
                col: col.clone(),
                prio,
                tags: flag(args, "--tags").map(|s| s.split(',').map(str::to_string).collect()).unwrap_or_default(),
                criteria: flag(args, "--criteria")
                    .map(|s| s.split(';').map(|c| (c.trim().to_string(), false)).filter(|(c, _)| !c.is_empty()).collect())
                    .unwrap_or_default(),
                created: now(),
                trans: vec![(col, now())],
                ..Task::default()
            };
            store.put(&t)?;
            Ok(format!("{} added to {}", t.id, t.col))
        }
        Some("board") => Ok(board::render(&store.all()?, store.wip())),
        Some("list") => {
            let mut ts = store.all()?;
            if let Some(c) = flag(args, "--col") { valid_col(&c)?; ts.retain(|t| t.col == c); }
            if let Some(tag) = flag(args, "--tag") { ts.retain(|t| t.tags.iter().any(|x| x == &tag)); }
            if has(args, "--blocked") { ts.retain(|t| !t.blocked.is_empty()); }
            if ts.is_empty() { return Ok("no tasks".into()); }
            Ok(ts.iter().map(|t| format!("{}\t{}\t{}\t{}", t.id, t.col, t.prio, t.title)).collect::<Vec<_>>().join("\n"))
        }
        Some("show") => {
            let t = store.get(args.get(1).ok_or("usage: tasks show T-1")?)?;
            let mut out = vec![format!("{} [{}] {} — {}", t.id, t.prio, t.col, t.title)];
            if !t.tags.is_empty() { out.push(format!("tags: {}", t.tags.join(","))); }
            if !t.blocked.is_empty() { out.push(format!("BLOCKED: {}", t.blocked)); }
            for (i, (c, done)) in t.criteria.iter().enumerate() {
                out.push(format!("  [{}] {}. {}", if *done { "x" } else { " " }, i + 1, c));
            }
            for (c, ts) in &t.trans { out.push(format!("  → {c} @{ts}")); }
            Ok(out.join("\n"))
        }
        Some("move") => {
            let id = args.get(1).ok_or("usage: tasks move T-1 <column>")?;
            let col = args.get(2).ok_or("column required")?;
            valid_col(col)?;
            let mut t = store.get(id)?;
            let wip = store.wip();
            let count_in = |c: &str| store.all().map(|ts| ts.iter().filter(|x| x.col == c && x.id != t.id).count()).unwrap_or(0);
            // WIP limits are HARD unless --force: pull, don't push.
            if !has(args, "--force") {
                if col == "doing" && count_in("doing") >= wip.doing {
                    return Err(format!("WIP limit: doing already has {}/{} — finish or move something out first (--force to override)", count_in("doing"), wip.doing));
                }
                if col == "review" && count_in("review") >= wip.review {
                    return Err(format!("WIP limit: review already has {}/{} (--force to override)", count_in("review"), wip.review));
                }
            }
            // Definition of done is the gate: all criteria checked before done.
            if col == "done" && !has(args, "--force") && t.criteria.iter().any(|(_, d)| !d) {
                return Err(format!(
                    "{} has unchecked criteria ({}/{}) — check them (tasks crit check) or --force",
                    t.id, t.criteria_done(), t.criteria.len()
                ));
            }
            t.col = col.to_string();
            t.trans.push((col.to_string(), now()));
            if col == "done" { t.done_ts = now(); t.blocked.clear(); }
            store.put(&t)?;
            Ok(format!("{} → {}", t.id, col))
        }
        Some("done") => {
            // Sugar for `move <id> done` (same gates).
            let id = args.get(1).ok_or("usage: tasks done T-1")?.clone();
            let mut fwd = vec!["move".to_string(), id, "done".to_string()];
            if has(args, "--force") { fwd.push("--force".into()); }
            if let Some(db) = flag(args, "--db") { fwd.push("--db".into()); fwd.push(db); }
            run(&fwd)
        }
        Some("next") => {
            // Pull-based: only hand out work when there is doing-capacity.
            let ts = store.all()?;
            let wip = store.wip();
            let doing: Vec<&Task> = ts.iter().filter(|t| t.col == "doing").collect();
            if doing.len() >= wip.doing {
                return Ok(format!(
                    "WIP full ({}/{}): finish {} first (stop starting, start finishing)",
                    doing.len(),
                    wip.doing,
                    doing.iter().map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }
            let mut ready: Vec<&Task> = ts.iter().filter(|t| t.col == "ready" && t.blocked.is_empty()).collect();
            ready.sort_by(|a, b| a.prio.cmp(&b.prio).then(a.created.cmp(&b.created)));
            match ready.first() {
                Some(t) => Ok(format!("pull {}: [{}] {} — move it with: tasks move {} doing", t.id, t.prio, t.title, t.id)),
                None => Ok("no ready tasks — triage the backlog (tasks list --col backlog)".into()),
            }
        }
        Some("crit") => {
            let sub = args.get(1).map(String::as_str).ok_or("usage: tasks crit add|check|uncheck T-1 …")?;
            let id = args.get(2).ok_or("task id required")?;
            let mut t = store.get(id)?;
            let out = match sub {
                "add" => {
                    let text = args.get(3).ok_or("criterion text required")?;
                    t.criteria.push((text.clone(), false));
                    format!("{}: criterion {} added", t.id, t.criteria.len())
                }
                "check" | "uncheck" => {
                    let n: usize = args.get(3).ok_or("criterion number required")?.parse().map_err(|_| "bad number")?;
                    let item = t.criteria.get_mut(n.wrapping_sub(1)).ok_or_else(|| format!("no criterion {n}"))?;
                    item.1 = sub == "check";
                    format!("{}: criterion {} {} ({}/{}✓)", t.id, n, if sub == "check" { "✓" } else { "unchecked" }, t.criteria_done(), t.criteria.len())
                }
                _ => return Err("crit sub-command must be add|check|uncheck".into()),
            };
            store.put(&t)?;
            Ok(out)
        }
        Some("block") => {
            let id = args.get(1).ok_or("usage: tasks block T-1 'reason'")?;
            let reason = args.get(2).ok_or("a block needs an explicit reason")?;
            let mut t = store.get(id)?;
            t.blocked = reason.clone();
            store.put(&t)?;
            Ok(format!("{} blocked: {reason}", t.id))
        }
        Some("unblock") => {
            let id = args.get(1).ok_or("usage: tasks unblock T-1")?;
            let mut t = store.get(id)?;
            t.blocked.clear();
            store.put(&t)?;
            Ok(format!("{} unblocked", t.id))
        }
        Some("rm") => {
            let id = args.get(1).ok_or("usage: tasks rm T-1")?;
            if store.remove(id)? { Ok(format!("{id} removed")) } else { Err(format!("no task '{id}'")) }
        }
        Some("wip") => {
            let mut w = store.wip();
            if let Some(d) = flag(args, "--doing").and_then(|s| s.parse().ok()) { w.doing = d; }
            if let Some(r) = flag(args, "--review").and_then(|s| s.parse().ok()) { w.review = r; }
            if w.doing == 0 { return Err("doing WIP must be ≥1".into()); }
            store.set_wip(w)?;
            Ok(format!("wip limits: doing {} review {}", w.doing, w.review))
        }
        Some("metrics") => Ok(board::metrics(&store.all()?, now())),
        Some("statusline") => board::statusline(&store),
        _ => Ok(HELP.trim().into()),
    }
}

const HELP: &str = r#"
tasks — kanban board (semdb-backed, policies enforced in Rust)

USAGE:
  tasks add '<title>' [--prio p1|p2|p3] [--col backlog|ready] [--tags a,b] [--criteria 'a;b;c']
  tasks todo '<title>'                quick capture → backlog p3
  tasks board                         render columns + WIP state
  tasks list [--col C] [--tag T] [--blocked]
  tasks show T-1                      card: criteria checklist + history
  tasks move T-1 <column>             WIP-limited; done requires criteria ✓ (--force overrides)
  tasks done T-1                      = move done
  tasks next                          pull highest-prio ready task IF doing has capacity
  tasks crit add T-1 '<text>' | check T-1 <n> | uncheck T-1 <n>
  tasks block T-1 '<reason>' | unblock T-1
  tasks wip [--doing N] [--review N]  set WIP limits (default doing 1, review 3)
  tasks metrics                       throughput, cycle time, lead time
  tasks rm T-1

Columns: backlog → ready → doing → review → done. Default db: data/tasks.semdb.
Kanban rules live in skills/Kanban (triage, pull, review, retro workflows).
"#;
