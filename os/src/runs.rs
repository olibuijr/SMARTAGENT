//! Runs tab — a workflow **Runs** view with TUI `/runs` parity. Fetches the
//! gateway `workflow runs` output and renders each run as a card: run id /
//! definition name, current step, status (running amber / done green /
//! failed red), and the linked task if present. Newest first, scrollable,
//! with a refresh button and loading / empty / error states.
//!
//! DATA SEAM: the only source is `net::run_tool_async("workflow", ["runs"])`,
//! which returns the `workflow runs` stdout lines. The CLI format is
//! tab-separated — `{id}\t{def}\t{status}\tstep {n}[\t{task}]` — but we parse
//! DEFENSIVELY: anything we can't structure is surfaced as its raw line so no
//! run is ever lost if the format shifts.

use dioxus::prelude::*;

use crate::net;

const RUNS_CSS: Asset = asset!("/assets/runs.css");

// ── Parsed run ──────────────────────────────────────────────────────────────

/// One workflow run. `structured` is false when the line didn't match the
/// expected shape — then only `raw` is meaningful and we show it verbatim.
#[derive(Clone, PartialEq)]
struct Run {
    id: String,
    def: String,
    status: String,
    step: String,
    task: Option<String>,
    raw: String,
    structured: bool,
}

/// Parse one `workflow runs` line. Splits on tabs first (the CLI's real
/// format), then falls back to whitespace; if neither yields the four
/// expected fields we keep the raw line so nothing is dropped.
fn parse_run(line: &str) -> Run {
    let raw = line.trim_end().to_string();
    // Primary: tab-separated `{id}\t{def}\t{status}\tstep {n}[\t{task}]`.
    let tab: Vec<&str> = raw.split('\t').map(str::trim).filter(|s| !s.is_empty()).collect();
    if tab.len() >= 4 {
        return Run {
            id: tab[0].to_string(),
            def: tab[1].to_string(),
            status: tab[2].to_string(),
            step: tab[3].to_string(),
            task: tab.get(4).map(|s| s.to_string()),
            raw,
            structured: true,
        };
    }
    // Fallback: whitespace-separated, tolerating a `step N` two-token tail.
    let ws: Vec<&str> = raw.split_whitespace().collect();
    if ws.len() >= 4 {
        // Find a `step` token; everything after it (a number) is the step.
        let step = ws
            .iter()
            .position(|t| t.eq_ignore_ascii_case("step"))
            .map(|i| ws[i..].join(" "))
            .unwrap_or_else(|| ws[3].to_string());
        return Run {
            id: ws[0].to_string(),
            def: ws[1].to_string(),
            status: ws[2].to_string(),
            step,
            task: ws.iter().find(|t| t.starts_with("T-")).map(|s| s.to_string()),
            raw,
            structured: true,
        };
    }
    // Couldn't structure it — surface the raw line untouched.
    Run {
        id: String::new(),
        def: String::new(),
        status: String::new(),
        step: String::new(),
        task: None,
        raw,
        structured: false,
    }
}

/// Map a status word to a stable CSS class: running → amber, done → green,
/// failed/aborted → red, anything else → neutral.
fn status_class(status: &str) -> &'static str {
    match status.to_lowercase().as_str() {
        "running" | "active" | "live" => "running",
        "done" | "completed" | "complete" | "finished" | "ok" => "done",
        "failed" | "fail" | "error" | "aborted" | "abort" | "cancelled" => "failed",
        _ => "other",
    }
}

/// Flatten the gateway lines: a single data event may carry the whole
/// newline-joined output, so split on '\n' and drop blanks.
fn flatten(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .flat_map(|l| l.split('\n'))
        .map(|s| s.trim_end().to_string())
        .filter(|s| !s.trim().is_empty())
        .collect()
}

// ── Component ───────────────────────────────────────────────────────────────

#[component]
pub fn Runs() -> Element {
    // Fetch on mount; the refresh button re-runs the resource.
    let mut runs = use_resource(move || async move {
        net::run_tool_async("workflow", vec!["runs".into()]).await
    });

    // Snapshot the resource: None = still loading.
    let snapshot: Option<Vec<String>> = runs.read().clone();

    let body = match snapshot {
        None => rsx! {
            div { class: "runs-state runs-loading", "Loading workflow runs…" }
        },
        Some(lines) => {
            let flat = flatten(&lines);
            if flat.is_empty() {
                // The CLI always prints at least "no runs"; a truly empty
                // response means the gateway couldn't be reached.
                rsx! {
                    div { class: "runs-state runs-error",
                        div { class: "runs-state-title", "Couldn't reach the gateway" }
                        div { class: "runs-state-hint", "No response from `workflow runs`. Tap refresh to retry." }
                    }
                }
            } else if flat.len() == 1 && flat[0].trim().eq_ignore_ascii_case("no runs") {
                rsx! {
                    div { class: "runs-state runs-empty", "no workflow runs" }
                }
            } else {
                // Parse and show newest first (runs are appended chronologically).
                let mut parsed: Vec<Run> = flat.iter().map(|l| parse_run(l)).collect();
                parsed.reverse();
                rsx! {
                    div { class: "runs-list",
                        for run in parsed {
                            if run.structured {
                                div { class: "run-card",
                                    div { class: "run-top",
                                        span { class: "run-def", "{run.def}" }
                                        span { class: "run-status run-status-{status_class(&run.status)}", "{run.status}" }
                                    }
                                    div { class: "run-meta",
                                        span { class: "run-id", "{run.id}" }
                                        span { class: "run-step", "{run.step}" }
                                        if let Some(task) = run.task.clone() {
                                            span { class: "run-task", "{task}" }
                                        }
                                    }
                                }
                            } else {
                                div { class: "run-card run-card-raw",
                                    span { class: "run-raw", "{run.raw}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        document::Stylesheet { href: RUNS_CSS }
        div { class: "runs",
            header { class: "runs-head",
                span { class: "runs-title", "Agent Runs" }
                button {
                    class: "runs-refresh",
                    onclick: move |_| runs.restart(),
                    "↻ Refresh"
                }
            }
            div { class: "runs-body",
                {body}
            }
        }
    }
}
