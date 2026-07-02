//! Board rendering + kanban flow metrics (cycle time, lead time, throughput).

use crate::store::{Store, Task, Wip, COLUMNS};

/// Render the board as compact columns. Terse by design — this lands in agent
/// context.
pub fn render(tasks: &[Task], wip: Wip) -> String {
    let mut out = Vec::new();
    for col in COLUMNS {
        let in_col: Vec<&Task> = tasks.iter().filter(|t| t.col == col).collect();
        let limit = match col {
            "doing" => Some(wip.doing),
            "review" => Some(wip.review),
            _ => None,
        };
        let head = match limit {
            Some(l) => {
                let over = if in_col.len() > l { " OVER-WIP!" } else { "" };
                format!("{} ({}/{}{})", col.to_uppercase(), in_col.len(), l, over)
            }
            None => format!("{} ({})", col.to_uppercase(), in_col.len()),
        };
        out.push(head);
        for t in in_col {
            let blocked = if t.blocked.is_empty() { String::new() } else { format!(" ⛔{}", t.blocked) };
            let crit = if t.criteria.is_empty() {
                String::new()
            } else {
                format!(" [{}/{}✓]", t.criteria_done(), t.criteria.len())
            };
            let mut title = t.title.clone();
            if title.chars().count() > 60 {
                title = title.chars().take(59).collect::<String>() + "…";
            }
            let owner = if t.owner.is_empty() { String::new() } else { format!(" @{}", t.owner) };
            out.push(format!("  {} {} {}{}{}{}", t.id, t.prio, title, owner, crit, blocked));
        }
    }
    out.join("\n")
}

/// Flow metrics over done tasks + current board state.
pub fn metrics(tasks: &[Task], now: i64) -> String {
    let done: Vec<&Task> = tasks.iter().filter(|t| t.col == "done" && t.done_ts > 0).collect();
    let week = 7 * 86_400;
    let throughput = done.iter().filter(|t| now - t.done_ts <= week).count();
    let cycles: Vec<i64> = done
        .iter()
        .filter_map(|t| t.first_doing().map(|d| (t.done_ts - d).max(0)))
        .collect();
    let leads: Vec<i64> = done.iter().map(|t| (t.done_ts - t.created).max(0)).collect();
    let avg = |v: &[i64]| if v.is_empty() { 0 } else { v.iter().sum::<i64>() / v.len() as i64 };
    let fmt = |secs: i64| {
        if secs >= 86_400 { format!("{:.1}d", secs as f64 / 86_400.0) }
        else if secs >= 3_600 { format!("{:.1}h", secs as f64 / 3_600.0) }
        else { format!("{}m", secs / 60) }
    };
    let wip_now = tasks.iter().filter(|t| t.col == "doing" || t.col == "review").count();
    let blocked = tasks.iter().filter(|t| !t.blocked.is_empty() && t.col != "done").count();
    format!(
        "throughput: {throughput} done in last 7d\ncycle time (doing→done): avg {} over {} tasks\nlead time (created→done): avg {} over {} tasks\nwip now (doing+review): {wip_now}\nblocked: {blocked}",
        fmt(avg(&cycles)),
        cycles.len(),
        fmt(avg(&leads)),
        leads.len()
    )
}

/// `level|icon text` for the UI statusline.
pub fn statusline(store: &Store) -> Result<String, String> {
    let tasks = store.all()?;
    let wip = store.wip();
    let doing = tasks.iter().filter(|t| t.col == "doing").count();
    let ready = tasks.iter().filter(|t| t.col == "ready").count();
    let review = tasks.iter().filter(|t| t.col == "review").count();
    let blocked = tasks.iter().filter(|t| !t.blocked.is_empty() && t.col != "done").count();
    let mut text = format!("▣ {doing}/{} doing · {ready} ready", wip.doing);
    if review > 0 { text.push_str(&format!(" · {review} review")); }
    if blocked > 0 { text.push_str(&format!(" · {blocked} blocked")); }
    let level = if doing > wip.doing || review > wip.review {
        "err" // WIP breached — the board is being pushed, not pulled
    } else if blocked > 0 || doing == wip.doing {
        "warn"
    } else {
        "ok"
    };
    Ok(format!("{level}|{text}"))
}
