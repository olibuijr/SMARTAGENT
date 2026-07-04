//! Agent Inspector — desktop-agent right-panel parity for the fleet. Given a
//! fleet agent name it shows role/specialty, live state, a context-usage gauge
//! (tokens vs a 400k ceiling), token count today, the current task, and a
//! recent tool-activity timeline. A reusable `ContextGauge` bar is exported for
//! other screens (green → amber → red as it fills).
//!
//! ── DATA SEAM ────────────────────────────────────────────────────────────────
//! The fleet roster is fetched over the gateway `agents` op (shared read-only
//! `net::request_async`), which returns one TSV row per agent:
//!   `name \t state \t doing \t role \t busy_secs \t tokens \t tools \t words`
//! where `tools` is a `→`-joined tool chain. Loading / empty / error are all
//! handled; an unreachable gateway comes back empty and shows a retry state.

use dioxus::prelude::*;

use crate::app::JEEVES;
use crate::net;

const INSPECTOR_CSS: Asset = asset!("/assets/inspector.css");

/// Context window ceiling used for the usage gauge (per-agent budget).
const CONTEXT_CEILING: u64 = 400_000;

// ── Roster row (the seam contract) ───────────────────────────────────────────

/// One parsed fleet roster row.
#[derive(Clone, PartialEq)]
struct AgentRow {
    name: String,
    state: String,
    /// Current task summary (may be empty / "-" when idle).
    doing: String,
    /// Role / specialty (e.g. "Backend", "QA Lead").
    role: String,
    /// Seconds the agent has been busy on the current turn.
    busy_secs: u64,
    /// Context tokens consumed today.
    tokens: u64,
    /// `→`-joined tool chain (most recent activity).
    tools: String,
    /// Words produced.
    words: u64,
}

/// Parse a single `\t`-separated roster line; `None` for blanks / error lines.
fn parse_row(line: &str) -> Option<AgentRow> {
    if line.trim().is_empty() || line.starts_with("error:") {
        return None;
    }
    let f: Vec<&str> = line.split('\t').collect();
    let name = f.first().copied().unwrap_or("").trim().to_string();
    if name.is_empty() {
        return None;
    }
    let num = |i: usize| f.get(i).and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
    let text = |i: usize| f.get(i).copied().unwrap_or("").trim().to_string();
    Some(AgentRow {
        name,
        state: text(1),
        doing: text(2),
        role: text(3),
        busy_secs: num(4),
        tokens: num(5),
        tools: text(6),
        words: num(7),
    })
}

// ── Format helpers ───────────────────────────────────────────────────────────

/// Compact token count: `128000` → `128.0k`, small values stay plain.
fn fmt_tokens(n: u64) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// Human duration from seconds: `0` → `—`, `72` → `1m 12s`.
fn fmt_dur(secs: u64) -> String {
    if secs == 0 {
        return "—".to_string();
    }
    let (m, s) = (secs / 60, secs % 60);
    if m == 0 {
        format!("{s}s")
    } else {
        format!("{m}m {s}s")
    }
}

/// CSS state class for the live status badge.
fn state_class(state: &str) -> &'static str {
    match state.to_lowercase().as_str() {
        "working" | "busy" | "running" | "thinking" | "active" => "st-working",
        "idle" | "ready" | "waiting" => "st-idle",
        "error" | "failed" | "stopped" | "blocked" => "st-error",
        _ => "st-other",
    }
}

// ── Context gauge (reusable) ─────────────────────────────────────────────────

/// A horizontal context-usage bar that fills green → amber → red as `used`
/// approaches `ceiling`. Reusable across screens.
#[component]
pub fn ContextGauge(used: u64, ceiling: u64) -> Element {
    let ceiling = ceiling.max(1);
    let pct = ((used as f64 / ceiling as f64) * 100.0).clamp(0.0, 100.0);
    let level = if pct >= 85.0 {
        "crit"
    } else if pct >= 60.0 {
        "warn"
    } else {
        "ok"
    };
    let width = format!("{pct:.1}%");
    rsx! {
        div { class: "ctx-gauge",
            div { class: "ctx-track",
                div { class: "ctx-fill ctx-{level}", style: "width: {width}" }
            }
            div { class: "ctx-legend",
                span { class: "ctx-used", "{fmt_tokens(used)} / {fmt_tokens(ceiling)}" }
                span { class: "ctx-pct ctx-{level}-t", "{pct:.0}%" }
            }
        }
    }
}

// ── Agent detail (rendered once a matching row is found) ──────────────────────

fn detail(row: &AgentRow) -> Element {
    let doing_empty = row.doing.trim().is_empty() || row.doing.trim() == "-";
    let doing = if doing_empty {
        "— idle · no active task".to_string()
    } else {
        row.doing.clone()
    };
    let role = if row.role.trim().is_empty() {
        "—".to_string()
    } else {
        row.role.clone()
    };
    let tools: Vec<String> = row
        .tools
        .split('→')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    let task_class = if doing_empty { "insp-task muted" } else { "insp-task" };
    let n_tools = tools.len();

    rsx! {
        div { class: "insp-body",

            // Role / live state.
            div { class: "insp-meta",
                div { class: "insp-field",
                    span { class: "insp-key", "ROLE" }
                    span { class: "insp-val", "{role}" }
                }
                div { class: "insp-field",
                    span { class: "insp-key", "STATE" }
                    span { class: "insp-badge {state_class(&row.state)}",
                        span { class: "st-dot" }
                        span { class: "st-txt", "{row.state}" }
                    }
                }
            }

            // Context-usage gauge.
            div { class: "insp-block",
                div { class: "insp-block-head",
                    span { class: "insp-block-title", "Context usage" }
                    span { class: "insp-block-note", "{fmt_tokens(row.tokens)} today" }
                }
                ContextGauge { used: row.tokens, ceiling: CONTEXT_CEILING }
            }

            // Quick stats.
            div { class: "insp-stats",
                div { class: "insp-chip",
                    span { class: "insp-chip-n", "{fmt_tokens(row.tokens)}" }
                    span { class: "insp-chip-l", "tokens" }
                }
                div { class: "insp-chip",
                    span { class: "insp-chip-n", "{fmt_dur(row.busy_secs)}" }
                    span { class: "insp-chip-l", "active" }
                }
                div { class: "insp-chip",
                    span { class: "insp-chip-n", "{row.words}" }
                    span { class: "insp-chip-l", "words" }
                }
            }

            // Current task.
            div { class: "insp-block",
                div { class: "insp-block-head",
                    span { class: "insp-block-title", "Current task" }
                }
                div { class: "{task_class}", "{doing}" }
            }

            // Recent tool activity — the `→`-chain as a timeline.
            div { class: "insp-block",
                div { class: "insp-block-head",
                    span { class: "insp-block-title", "Recent tool activity" }
                    if n_tools > 0 {
                        span { class: "insp-block-note", "{n_tools} steps" }
                    }
                }
                if tools.is_empty() {
                    div { class: "insp-empty-line", "No tool calls yet." }
                } else {
                    ol { class: "insp-timeline",
                        for (i, t) in tools.iter().enumerate() {
                            li { class: if i + 1 == n_tools { "tl-item tl-last" } else { "tl-item" },
                                span { class: "tl-node" }
                                span { class: "tl-tool", "{t}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Inspector ────────────────────────────────────────────────────────────────

/// Agent/session detail sheet for a single fleet agent. Fetches the roster over
/// the gateway `agents` op, picks the matching row, and renders its detail with
/// a context gauge and tool-activity timeline. Refreshes on demand.
#[component]
pub fn Inspector(agent: String) -> Element {
    // Roster fetch; `.restart()` re-fetches on the refresh button.
    let mut roster = use_resource(move || async move {
        net::agents_async().await
    });

    rsx! {
        document::Stylesheet { href: INSPECTOR_CSS }
        section { class: "inspector",

            header { class: "insp-top",
                img { class: "insp-ava", src: JEEVES, alt: "" }
                div { class: "insp-id",
                    div { class: "insp-name", "{agent}" }
                    div { class: "insp-sub", "agent inspector" }
                }
                button {
                    class: "insp-refresh",
                    title: "Refresh",
                    onclick: move |_| roster.restart(),
                    "⟳"
                }
            }

            {
                let guard = roster.read_unchecked();
                match &*guard {
                    // Still loading the first fetch.
                    None => rsx! {
                        div { class: "insp-state insp-loading",
                            span { class: "insp-spinner", "▋" }
                            span { "Loading fleet…" }
                        }
                    },
                    Some(lines) => {
                        let errored = lines.iter().any(|l| l.starts_with("error:"));
                        let rows: Vec<AgentRow> = lines.iter().filter_map(|l| parse_row(l)).collect();
                        match rows.iter().find(|r| r.name.eq_ignore_ascii_case(agent.trim())) {
                            Some(row) => detail(row),
                            // Gateway unreachable / no data → offer a retry.
                            None if errored || rows.is_empty() => rsx! {
                                div { class: "insp-state insp-error",
                                    div { class: "insp-error-glyph", "⚠" }
                                    div { class: "insp-error-msg", "Fleet gateway unreachable." }
                                    div { class: "insp-error-sub", "Tap ⟳ to retry." }
                                }
                            },
                            // Roster loaded but no such agent.
                            None => rsx! {
                                div { class: "insp-state insp-empty",
                                    div { class: "insp-empty-glyph", "∅" }
                                    div { "No fleet agent named " span { class: "insp-hl", "“{agent}”" } "." }
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}
