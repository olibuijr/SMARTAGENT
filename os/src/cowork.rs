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
        vec![
            Mail {
                sender: "Skatturinn".into(),
                subject: "Staðfesting á umsókn".into(),
                snippet: "Umsókn þín um endurgreiðslu hefur verið móttekin og er í vinnslu…".into(),
                time: "08:12".into(),
                unread: true,
            },
            Mail {
                sender: "Klara".into(),
                subject: "Depill — dýralæknir kl. 15".into(),
                snippet: "Manstu eftir tímanum í dag? Ég kemst ekki, geturðu farið með hann?".into(),
                time: "07:44".into(),
                unread: true,
            },
            Mail {
                sender: "AkurAI · billing".into(),
                subject: "Invoice #1042 paid".into(),
                snippet: "Your monthly follow-up retainer for June has been settled. Thanks!".into(),
                time: "Yesterday".into(),
                unread: false,
            },
            Mail {
                sender: "GSÍ".into(),
                subject: "Mótaskrá 2027 — forskráning".into(),
                snippet: "Forskráning fyrir keppnistímabilið er opin. Tryggðu þér sæti…".into(),
                time: "Yesterday".into(),
                unread: false,
            },
            Mail {
                sender: "Zophonias".into(),
                subject: "re: helgin".into(),
                snippet: "Hljómar vel pabbi, sjáumst þá á laugardaginn 🎸".into(),
                time: "Mon".into(),
                unread: false,
            },
        ]
    }

    // TODO(orchestrator): back with real gateway op — `schedule` today view +
    // reminders (op:"agenda.today").
    pub fn agenda() -> Vec<AgendaItem> {
        vec![
            AgendaItem {
                at: "09:30".into(),
                title: "AkurAI kickoff — Norðurorka".into(),
                kind: "event",
                done: true,
            },
            AgendaItem {
                at: "12:00".into(),
                title: "Borða hádegismat".into(),
                kind: "reminder",
                done: false,
            },
            AgendaItem {
                at: "14:00".into(),
                title: "Deep-focus: gateway mail op".into(),
                kind: "event",
                done: false,
            },
            AgendaItem {
                at: "15:00".into(),
                title: "Depill — dýralæknir".into(),
                kind: "event",
                done: false,
            },
            AgendaItem {
                at: "17:30".into(),
                title: "Ganga með Depil".into(),
                kind: "reminder",
                done: false,
            },
        ]
    }

    // TODO(orchestrator): back with real gateway op — the `tasks` binary board
    // for the active project (op:"tasks.board").
    pub fn board() -> Vec<Task> {
        let t = |title: &str, stage: Stage| Task {
            title: title.into(),
            stage,
        };
        vec![
            t("Sækja um fjárhagsaðstoð Akureyrarbæ", Stage::Backlog),
            t("Örorku/endurhæfingarlífeyrir umsókn", Stage::Backlog),
            t("Klára RSK skil 2023", Stage::Ready),
            t("Færa localai á Proxmox", Stage::Ready),
            t("Gateway mail op (himalaya)", Stage::Doing),
            t("Cowork tab UI", Stage::Doing),
            t("Telegram streaming svör", Stage::Review),
            t("APK build + verify á síma", Stage::Done),
            t("Pixel-brand tokens", Stage::Done),
        ]
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
