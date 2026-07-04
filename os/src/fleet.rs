//! Fleet tab — the team roster (parity with the pi TUI `/team` panel). Owned by
//! this module only. Fetches the fleet over the gateway `agents` op via
//! `net::agents_async()` (TSV lines) and renders one card per agent: name,
//! role/specialty, a state pill (idle/working), the current task, and tokens.
//!
//! The "Chat" button is a placeholder that calls the `on_chat` prop with the
//! agent's raw name — the orchestrator wires it to open a chat targeting that
//! agent at merge time.

use dioxus::prelude::*;

const FLEET_CSS: Asset = asset!("/assets/fleet.css");

// ── Roster row (the TSV contract) ───────────────────────────────────────────

/// One agent as parsed from a `net::agents_async()` line. The gateway `agents`
/// op emits tab-separated fields in panel order:
/// `name \t state \t doing \t role \t busy_secs \t tokens \t tools \t words`.
#[derive(Clone, PartialEq)]
struct AgentRow {
    name: String,
    /// "idle" | "working".
    state: String,
    doing: String,
    role: String,
    busy_secs: u64,
    tokens: u64,
    tools: String,
    words: String,
}

impl AgentRow {
    /// Parse one TSV line; `None` for a blank/nameless line so junk is dropped.
    fn parse(line: &str) -> Option<AgentRow> {
        let mut f = line.split('\t');
        let name = f.next()?.trim().to_string();
        if name.is_empty() {
            return None;
        }
        Some(AgentRow {
            name,
            state: f.next().unwrap_or("idle").trim().to_string(),
            doing: f.next().unwrap_or("").trim().to_string(),
            role: f.next().unwrap_or("").trim().to_string(),
            busy_secs: f.next().unwrap_or("0").trim().parse().unwrap_or(0),
            tokens: f.next().unwrap_or("0").trim().parse().unwrap_or(0),
            tools: f.next().unwrap_or("").trim().to_string(),
            words: f.next().unwrap_or("").trim().to_string(),
        })
    }

    fn working(&self) -> bool {
        self.state.eq_ignore_ascii_case("working")
    }
}

// ── Formatting helpers ──────────────────────────────────────────────────────

/// "linus-torvalds" → "Linus Torvalds" for the card title.
fn pretty(name: &str) -> String {
    name.split(|c| c == '-' || c == '_')
        .filter(|s| !s.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// First glyph, uppercased, for the pixel badge.
fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default()
}

/// Compact token count: 1_234 → "1.2k", 2_500_000 → "2.5M".
fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Elapsed-busy as a short "3m 12s" / "45s" string.
fn fmt_busy(secs: u64) -> String {
    if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

/// Cross-platform ~sleep for the auto-refresh loop. Returns `true` if it
/// actually slept (native), `false` on wasm where we have no portable timer —
/// the caller then stops looping and relies on the manual refresh button.
async fn sleep_secs(secs: u64) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = futures_channel::oneshot::channel::<()>();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            let _ = tx.send(());
        });
        rx.await.is_ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = secs;
        false
    }
}

// ── Component ───────────────────────────────────────────────────────────────

/// The Fleet / Team view. `on_chat` fires with the agent's raw name when its
/// "Chat" button is pressed (orchestrator opens a chat targeting that agent).
#[component]
pub fn Fleet(on_chat: EventHandler<String>) -> Element {
    // Resource over the gateway roster; `restart()` refetches (button + timer).
    let mut roster = use_resource(move || async move { crate::net::agents_async().await });

    // Auto-refresh every ~5s on native; the loop self-terminates on wasm.
    use_coroutine(move |_rx: UnboundedReceiver<()>| async move {
        loop {
            if !sleep_secs(5).await {
                break;
            }
            roster.restart();
        }
    });

    // Snapshot the resource into owned parsed rows so no read-guard is held
    // across the render body. `None` = loading; `Some(empty)` = unreachable.
    let rows: Option<Vec<AgentRow>> = roster
        .read()
        .as_ref()
        .map(|lines: &Vec<String>| lines.iter().filter_map(|l| AgentRow::parse(l)).collect());

    let online = rows
        .as_ref()
        .map(|v| v.iter().filter(|a| a.working()).count())
        .unwrap_or(0);
    let total = rows.as_ref().map(|v| v.len()).unwrap_or(0);

    rsx! {
        document::Stylesheet { href: FLEET_CSS }
        div { class: "fleet",

            header { class: "fl-head",
                span { class: "fl-title", "Agent Fleet" }
                if total > 0 {
                    span { class: "fl-sub", "{online} working · {total} agents" }
                }
                button {
                    class: "fl-refresh",
                    title: "Refresh roster",
                    onclick: move |_| roster.restart(),
                    "↻"
                }
            }

            match rows {
                None => rsx! {
                    div { class: "fl-state",
                        div { class: "fl-spinner" }
                        p { "Reaching the fleet…" }
                    }
                },
                Some(list) if list.is_empty() => rsx! {
                    div { class: "fl-state",
                        p { class: "fl-empty-icon", "▚" }
                        p { "No agents online." }
                        button { class: "fl-retry", onclick: move |_| roster.restart(), "Retry" }
                    }
                },
                Some(list) => rsx! {
                    div { class: "fl-grid",
                        for a in list {
                            AgentCard { agent: a, on_chat }
                        }
                    }
                },
            }
        }
    }
}

/// One roster card. Split out so each agent's closures capture only its data.
#[component]
fn AgentCard(agent: AgentRow, on_chat: EventHandler<String>) -> Element {
    let name = agent.name.clone();
    let working = agent.working();
    let doing = if agent.doing.is_empty() {
        "idle — no task".to_string()
    } else {
        agent.doing.clone()
    };
    let pill = if working { "fl-pill working" } else { "fl-pill idle" };
    let pill_label = if working { "working" } else { "idle" };

    rsx! {
        div { class: if working { "fl-card working" } else { "fl-card" },
            div { class: "fl-top",
                div { class: "fl-badge", "{initial(&agent.name)}" }
                div { class: "fl-id",
                    div { class: "fl-name", "{pretty(&agent.name)}" }
                    if !agent.role.is_empty() {
                        div { class: "fl-role", "{agent.role}" }
                    }
                }
                span { class: "{pill}", "{pill_label}" }
            }

            div { class: "fl-task",
                span { class: "fl-tasklabel", "TASK" }
                span { class: "fl-taskbody", "{doing}" }
            }

            div { class: "fl-meta",
                span { class: "fl-metric",
                    span { class: "fl-mk", "tokens" }
                    span { class: "fl-mv", "{fmt_tokens(agent.tokens)}" }
                }
                if working && agent.busy_secs > 0 {
                    span { class: "fl-metric",
                        span { class: "fl-mk", "busy" }
                        span { class: "fl-mv", "{fmt_busy(agent.busy_secs)}" }
                    }
                }
                if !agent.tools.is_empty() {
                    span { class: "fl-metric",
                        span { class: "fl-mk", "tool" }
                        span { class: "fl-mv", "{agent.tools}" }
                    }
                }
            }

            button {
                class: "fl-chat",
                onclick: move |_| on_chat.call(name.clone()),
                "Chat"
            }
        }
    }
}
