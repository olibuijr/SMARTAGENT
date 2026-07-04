//! Cowork tab — a day-to-day cockpit (like Claude Cowork / Hermes): mail inbox,
//! today's agenda + reminders, and a compact tasks kanban. Owned by Agent C.
//!
//! DATA SEAM: the gateway does not yet expose mail / agenda / board data over
//! the network, so the whole UI is built against the `data` module below. Those
//! three functions are the ONLY source of data — the orchestrator swaps in real
//! gateway ops by reimplementing `data::inbox()`, `data::agenda()`,
//! `data::board()` (himalaya for mail, `schedule` for agenda, the `tasks` binary
//! for the board) and the UI keeps working unchanged.

use dioxus::prelude::*;

const COWORK_CSS: Asset = asset!("/assets/cowork.css");

// ── Types (the seam contract) ───────────────────────────────────────────────

/// One inbox message.
#[derive(Clone, PartialEq)]
pub struct Mail {
    pub sender: String,
    pub subject: String,
    pub snippet: String,
    pub time: String,
    pub unread: bool,
}

/// A scheduled item or reminder for today.
#[derive(Clone, PartialEq)]
pub struct AgendaItem {
    pub at: String,
    pub title: String,
    /// "event" | "reminder"
    pub kind: &'static str,
    pub done: bool,
}

/// A kanban card.
#[derive(Clone, PartialEq)]
pub struct Task {
    pub title: String,
    pub stage: Stage,
}

/// The five board columns, in flow order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Backlog,
    Ready,
    Doing,
    Review,
    Done,
}

impl Stage {
    pub const ALL: [Stage; 5] = [
        Stage::Backlog,
        Stage::Ready,
        Stage::Doing,
        Stage::Review,
        Stage::Done,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Stage::Backlog => "Backlog",
            Stage::Ready => "Ready",
            Stage::Doing => "Doing",
            Stage::Review => "Review",
            Stage::Done => "Done",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Stage::Backlog => "backlog",
            Stage::Ready => "ready",
            Stage::Doing => "doing",
            Stage::Review => "review",
            Stage::Done => "done",
        }
    }

    /// Advance one column toward Done (Done stays put).
    pub fn next(self) -> Stage {
        match self {
            Stage::Backlog => Stage::Ready,
            Stage::Ready => Stage::Doing,
            Stage::Doing => Stage::Review,
            Stage::Review => Stage::Done,
            Stage::Done => Stage::Done,
        }
    }
}

// ── Data seam ───────────────────────────────────────────────────────────────
// The ONLY source of data for this tab. Realistic placeholders today; the
// orchestrator backs each function with a real gateway op later.

mod data {
    use super::{AgendaItem, Mail, Stage, Task};

    // TODO(orchestrator): back with real gateway op — himalaya inbox over the
    // gateway TCP bridge (op:"mail.inbox").
    pub fn inbox() -> Vec<Mail> {
        // Real inbox via the gateway `mail` op (himalaya envelope list — a
        // markdown table). Parse rows `| ID | FLAGS | SUBJECT | FROM | DATE |`.
        crate::net::request("mail", "")
            .into_iter()
            .filter_map(|line| {
                if !line.starts_with('|') { return None; }
                let cols: Vec<String> = line.split('|').map(|c| c.trim().to_string()).collect();
                // cols: ["", id, flags, subject, from, date, ""]
                if cols.len() < 6 || cols[1] == "ID" || cols[1].starts_with('-') { return None; }
                let unread = cols[2].contains('*');
                let date = cols[5].split(' ').next().unwrap_or("").to_string();
                Some(Mail {
                    sender: cols[4].clone(),
                    subject: cols[3].clone(),
                    snippet: String::new(),
                    time: date,
                    unread,
                })
            })
            .take(25)
            .collect()
    }

    // TODO(orchestrator): back with real gateway op — `schedule` today view +
    // reminders (op:"agenda.today").
    pub fn agenda() -> Vec<AgendaItem> {
        // Real scheduled jobs via the gateway `run` op (`schedule list`):
        // each line is `name 	 cron 	 cmd 	 last: <date>`.
        crate::net::run_tool("schedule", &["list"])
            .into_iter()
            .filter_map(|line| {
                if line.trim().is_empty() || line.starts_with("error:") { return None; }
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.is_empty() { return None; }
                let title = parts[0].trim().to_string();
                if title.is_empty() { return None; }
                let at = parts.get(1).map(|c| c.trim().to_string()).unwrap_or_default();
                Some(AgendaItem { at, title, kind: "reminder", done: false })
            })
            .collect()
    }

    /// Real fleet board over the gateway `board` op. Parses `tasks board`:
    /// column headers (`BACKLOG (n)` …) set the current stage; indented `  T-… `
    /// rows are cards under it. Falls back to empty if the gateway is unreachable.
    pub fn board() -> Vec<Task> {
        let lines = crate::net::request("board", "");
        let mut out = Vec::new();
        let mut stage = Stage::Backlog;
        for line in lines {
            let up = line.trim_start();
            let set = |s: &str| Stage::ALL.iter().copied().find(|st| up.starts_with(&s.to_uppercase()));
            if let Some(st) = ["backlog", "ready", "doing", "review", "done"]
                .iter()
                .find_map(|k| if up.to_lowercase().starts_with(k) { set(k) } else { None })
            {
                stage = st;
                continue;
            }
            // Card row: "  T-123 p2 Title… @owner [n/m]" — strip id/prio and tags.
            if let Some(rest) = up.strip_prefix("T-").and_then(|r| r.split_once(' ')) {
                let title = rest
                    .1
                    .trim_start_matches(|c: char| c == 'p' || c.is_ascii_digit() || c == ' ')
                    .split(" @")
                    .next()
                    .unwrap_or("")
                    .split(" [")
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !title.is_empty() {
                    out.push(Task { title, stage });
                }
            }
        }
        out
    }
}

// ── Component ───────────────────────────────────────────────────────────────

#[component]
pub fn Cowork() -> Element {
    // Signals seeded from the seam. The seam is the only data source; local
    // signals hold UI state (added tasks, dismissed cards) layered on top.
    let mail = use_signal(data::inbox);
    let agenda = use_signal(data::agenda);
    let mut tasks = use_signal(data::board);
    let mut new_task = use_signal(String::new);

    let unread = mail().iter().filter(|m| m.unread).count();

    let mut add_task = move || {
        let title = new_task().trim().to_string();
        if title.is_empty() {
            return;
        }
        tasks.with_mut(|t| {
            t.push(Task {
                title,
                stage: Stage::Backlog,
            })
        });
        new_task.set(String::new());
    };

    rsx! {
        document::Stylesheet { href: COWORK_CSS }
        div { class: "cowork",

            // 1 ── Mail ──────────────────────────────────────────────────────
            section { class: "cw-card",
                header { class: "cw-head",
                    span { class: "cw-title", "Mail" }
                    if unread > 0 {
                        span { class: "cw-badge", "{unread} new" }
                    }
                }
                ul { class: "cw-mail",
                    for m in mail() {
                        li { class: if m.unread { "cw-mailrow unread" } else { "cw-mailrow" },
                            span { class: "cw-dot" }
                            div { class: "cw-mailbody",
                                div { class: "cw-mailtop",
                                    span { class: "cw-sender", "{m.sender}" }
                                    span { class: "cw-time", "{m.time}" }
                                }
                                div { class: "cw-subject", "{m.subject}" }
                                div { class: "cw-snippet", "{m.snippet}" }
                            }
                        }
                    }
                }
            }

            // 2 ── Agenda / today ────────────────────────────────────────────
            section { class: "cw-card",
                header { class: "cw-head",
                    span { class: "cw-title", "Today" }
                    span { class: "cw-sub", "agenda · reminders" }
                }
                ul { class: "cw-agenda",
                    for a in agenda() {
                        li { class: if a.done { "cw-agrow done" } else { "cw-agrow" },
                            span { class: "cw-at", "{a.at}" }
                            span { class: "cw-kind cw-kind-{a.kind}",
                                if a.kind == "reminder" { "◔" } else { "▶" }
                            }
                            span { class: "cw-agtitle", "{a.title}" }
                        }
                    }
                }
            }

            // 3 ── Tasks board ───────────────────────────────────────────────
            section { class: "cw-card",
                header { class: "cw-head",
                    span { class: "cw-title", "Board" }
                    span { class: "cw-sub", "tap a card to advance" }
                }
                div { class: "cw-add",
                    input {
                        class: "cw-input",
                        placeholder: "Add a task to Backlog…",
                        value: "{new_task}",
                        oninput: move |e| new_task.set(e.value()),
                        onkeydown: move |e| if e.key() == Key::Enter { add_task(); },
                    }
                    button { class: "cw-addbtn", onclick: move |_| add_task(), "+" }
                }
                div { class: "cw-board",
                    for stage in Stage::ALL {
                        {
                            let cards: Vec<(usize, Task)> = tasks()
                                .into_iter()
                                .enumerate()
                                .filter(|(_, t)| t.stage == stage)
                                .collect();
                            let count = cards.len();
                            rsx! {
                                div { class: "cw-col cw-col-{stage.slug()}",
                                    div { class: "cw-colhead",
                                        span { class: "cw-colname", "{stage.label()}" }
                                        span { class: "cw-count", "{count}" }
                                    }
                                    div { class: "cw-cards",
                                        for (idx, card) in cards.into_iter().take(3) {
                                            button {
                                                class: "cw-taskcard",
                                                onclick: move |_| {
                                                    tasks.with_mut(|t| {
                                                        if let Some(item) = t.get_mut(idx) {
                                                            item.stage = item.stage.next();
                                                        }
                                                    });
                                                },
                                                "{card.title}"
                                            }
                                        }
                                        if count > 3 {
                                            span { class: "cw-more", "+{count - 3} more" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
