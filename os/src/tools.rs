//! Tools tab — the extensions command-center ("Agent Tools"). A mobile-first
//! launcher over every read-only gateway tool: tap a tile to open a panel that
//! runs the tool over the fleet gateway and streams its stdout lines back.
//!
//! Everything here is READ-ONLY. Each tile is pinned to a `(tool, verb)` pair
//! that the gateway allow-lists server-side (memory recall/list, vault
//! list/search, skills list/match, schedule, evals, hooks audit, supervise
//! status, rag/codegraph/codeindex search, workflow runs, tasks board, secrets
//! list, context show, web search). Panels never mutate anything.
//!
//! DATA SEAM: the only network call is `net::run_tool_async(tool, args)` — an
//! async wrapper (for `use_resource`) that runs a read-only tool over the
//! gateway TCP bridge and returns its output lines. `args` is the verb followed
//! by an optional query (e.g. `"recall neck surgery"` or just `"list"`).

use dioxus::prelude::*;

use crate::net;

const TOOLS_CSS: Asset = asset!("/assets/tools.css");

/// One launcher entry. `args_hint` doubles as the "takes a query" flag: empty
/// means the panel runs `verb` immediately, non-empty means it shows a query
/// input using the hint as placeholder and runs `verb <query>` on submit.
#[derive(Clone, Copy)]
struct ToolDef {
    /// Tool name (the gateway binary), e.g. `"memory"`.
    tool: &'static str,
    /// Default read-only verb, e.g. `"recall"`.
    verb: &'static str,
    /// Human label for the tile / panel header.
    label: &'static str,
    /// One-line description shown on the tile.
    desc: &'static str,
    /// Short pixel-font tag rendered as the tile icon.
    icon: &'static str,
    /// Query-input placeholder; empty = no query input.
    args_hint: &'static str,
}

/// The command-center catalog — one tile per read-only capability. Every
/// `(tool, verb)` here is inside the gateway's server-enforced allow-list.
const CATALOG: &[ToolDef] = &[
    ToolDef {
        tool: "memory",
        verb: "recall",
        label: "Memory",
        desc: "Semantic recall over stored project memory",
        icon: "MEM",
        args_hint: "recall query…",
    },
    ToolDef {
        tool: "vault",
        verb: "list",
        label: "Vault",
        desc: "List notes in the knowledge vault",
        icon: "VLT",
        args_hint: "",
    },
    ToolDef {
        tool: "vault",
        verb: "search",
        label: "Vault Search",
        desc: "Search vault notes by text",
        icon: "V?",
        args_hint: "search notes…",
    },
    ToolDef {
        tool: "skills",
        verb: "list",
        label: "Skills",
        desc: "List available agent skills",
        icon: "SKL",
        args_hint: "",
    },
    ToolDef {
        tool: "skills",
        verb: "match",
        label: "Skill Match",
        desc: "Find the skills that fit a task",
        icon: "SK?",
        args_hint: "describe a task…",
    },
    ToolDef {
        tool: "schedule",
        verb: "list",
        label: "Schedule",
        desc: "Scheduled jobs and routines",
        icon: "SCH",
        args_hint: "",
    },
    ToolDef {
        tool: "evals",
        verb: "list",
        label: "Evals",
        desc: "Evaluation suites",
        icon: "EVL",
        args_hint: "",
    },
    ToolDef {
        tool: "hooks",
        verb: "audit",
        label: "Hooks",
        desc: "Recent hook activity audit log",
        icon: "HK",
        args_hint: "",
    },
    ToolDef {
        tool: "supervise",
        verb: "status",
        label: "Services",
        desc: "Fleet service health status",
        icon: "SUP",
        args_hint: "",
    },
    ToolDef {
        tool: "rag",
        verb: "search",
        label: "RAG",
        desc: "Retrieval over indexed documents",
        icon: "RAG",
        args_hint: "search corpus…",
    },
    ToolDef {
        tool: "codegraph",
        verb: "search",
        label: "Code Graph",
        desc: "Search the symbol graph",
        icon: "CG",
        args_hint: "symbol or text…",
    },
    ToolDef {
        tool: "codeindex",
        verb: "search",
        label: "Code Index",
        desc: "Search indexed source files",
        icon: "CI",
        args_hint: "search code…",
    },
    ToolDef {
        tool: "workflow",
        verb: "runs",
        label: "Workflows",
        desc: "Recent workflow runs",
        icon: "WF",
        args_hint: "",
    },
    ToolDef {
        tool: "tasks",
        verb: "board",
        label: "Tasks",
        desc: "Kanban board across the fleet",
        icon: "TSK",
        args_hint: "",
    },
    ToolDef {
        tool: "secrets",
        verb: "list",
        label: "Secrets",
        desc: "Secret names (values never shown)",
        icon: "SEC",
        args_hint: "",
    },
    ToolDef {
        tool: "context",
        verb: "show",
        label: "Context",
        desc: "Current project context",
        icon: "CTX",
        args_hint: "",
    },
    ToolDef {
        tool: "search",
        verb: "web",
        label: "Web Search",
        desc: "Search the web via the gateway",
        icon: "WEB",
        args_hint: "web query…",
    },
];

/// The Tools tab: a launcher grid, or a single tool panel with a back button.
#[component]
pub fn Tools() -> Element {
    let mut selected = use_signal(|| None::<usize>);

    rsx! {
        document::Stylesheet { href: TOOLS_CSS }
        div { class: "tools",
            if let Some(i) = selected() {
                {
                    let t = CATALOG[i];
                    rsx! {
                        div { class: "tool-panel",
                            header { class: "tool-bar",
                                button {
                                    class: "tool-back",
                                    onclick: move |_| selected.set(None),
                                    "‹ Tools"
                                }
                                div { class: "tool-heading",
                                    span { class: "tool-name", "{t.label}" }
                                    span { class: "tool-cmd", "{t.tool} {t.verb}" }
                                }
                            }
                            ToolPanel {
                                tool: t.tool,
                                verb: t.verb,
                                label: t.label,
                                args_hint: t.args_hint,
                            }
                        }
                    }
                }
            } else {
                div { class: "tool-launch",
                    header { class: "tool-launchhead",
                        h2 { class: "tool-h", "Agent Tools" }
                        p { class: "tool-hsub", "Read-only command center · every extension" }
                    }
                    div { class: "tool-grid",
                        for (i, t) in CATALOG.iter().enumerate() {
                            button {
                                class: "tool-tile",
                                key: "{i}",
                                onclick: move |_| selected.set(Some(i)),
                                span { class: "tool-ico", "{t.icon}" }
                                span { class: "tool-tbody",
                                    span { class: "tool-tlabel", "{t.label}" }
                                    span { class: "tool-tdesc", "{t.desc}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A generic runner panel for one read-only tool. Runs `net::run_tool_async`
/// via `use_resource` and renders the output lines (monospace, scrollable).
/// When `args_hint` is non-empty the panel shows a query input and runs
/// `verb <query>` on submit; otherwise it runs `verb` immediately on mount.
#[component]
fn ToolPanel(
    tool: &'static str,
    verb: &'static str,
    label: &'static str,
    args_hint: &'static str,
) -> Element {
    let _ = label; // header shows the label; the panel keeps the prop for parity.
    let has_query = !args_hint.is_empty();
    let mut input = use_signal(String::new);
    let mut submitted = use_signal(String::new);
    let mut reload = use_signal(|| 0u32);

    // Reactive fetch: reruns when the submitted query or the reload counter
    // changes. Reads happen synchronously (before `async move`) so `use_resource`
    // tracks them as dependencies.
    let res = use_resource(move || {
        let q = submitted();
        let _ = reload();
        async move {
            // A query tool with no query yet does not hit the gateway.
            if has_query && q.trim().is_empty() {
                return Vec::<String>::new();
            }
            let args = if has_query {
                format!("{} {}", verb, q.trim())
            } else {
                verb.to_string()
            };
            net::run_tool_async(tool, args).await
        }
    });

    // Snapshot + drop the guard immediately so rendering can freely read signals.
    let snapshot: Option<Vec<String>> = res.read_unchecked().clone();
    let awaiting_query = has_query && submitted().trim().is_empty();

    rsx! {
        div { class: "tool-run",
            if has_query {
                div { class: "tool-query",
                    input {
                        class: "tool-qinput",
                        placeholder: "{args_hint}",
                        value: "{input}",
                        oninput: move |e| input.set(e.value()),
                        onkeydown: move |e| if e.key() == Key::Enter {
                            submitted.set(input());
                        },
                    }
                    button {
                        class: "tool-qrun",
                        onclick: move |_| submitted.set(input()),
                        "Run"
                    }
                }
            }
            div { class: "tool-outwrap",
                if awaiting_query {
                    div { class: "tool-hint",
                        "Enter a query to run "
                        span { class: "mono", "{tool} {verb}" }
                    }
                } else {
                    match snapshot {
                        None => rsx! {
                            div { class: "tool-loading", "Running {tool} {verb}…" }
                        },
                        Some(lines) if lines.is_empty() => rsx! {
                            div { class: "tool-empty",
                                span { "No output — the tool returned nothing, or the gateway is unreachable." }
                                button {
                                    class: "tool-refresh",
                                    onclick: move |_| reload += 1,
                                    "Retry"
                                }
                            }
                        },
                        Some(lines) => rsx! {
                            div { class: "tool-outhead",
                                span { class: "tool-count", "{lines.len()} lines" }
                                button {
                                    class: "tool-refresh",
                                    onclick: move |_| reload += 1,
                                    "↻ Refresh"
                                }
                            }
                            div { class: "tool-out",
                                for (i, l) in lines.iter().enumerate() {
                                    div { class: "tool-line", key: "{i}", "{l}" }
                                }
                            }
                        },
                    }
                }
            }
        }
    }
}
