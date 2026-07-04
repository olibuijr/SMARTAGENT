//! Code tab (Agent D) — browse repos under `workspaces/` and talk to the fleet
//! about the selected project. This is a *viewer* + chat: the agent does the
//! editing, this surfaces a project picker, a lazy/expandable file tree, a
//! read-only file viewer (with line numbers), a git-status strip, and a
//! project-rooted chat composer that streams the fleet's reply over the gateway.
//!
//! ── DATA SEAM ────────────────────────────────────────────────────────────────
//! The gateway does NOT yet expose workspace project/file/git data over the
//! network. The `data` module below returns realistic placeholder data behind
//! typed structs so the whole UI is built and testable now. Each function is
//! marked `TODO(orchestrator): back with real gateway op` — swap the body for a
//! gateway RPC (streaming/awaited) at merge time; the UI contract stays fixed.
//! The chat composer already uses the real `net::ask`.

use dioxus::prelude::*;
use futures_util::StreamExt;

use crate::app::JEEVES;
use crate::net::{self, Ev};

const CODE_CSS: Asset = asset!("/assets/code.css");

// ── Typed seam structs ───────────────────────────────────────────────────────

/// A repo under `workspaces/`.
#[derive(Clone, PartialEq)]
pub struct Project {
    pub name: String,
    /// Repo-root path (informational; the agent resolves the real root).
    pub path: String,
    /// Current git branch.
    pub branch: String,
    /// Count of changed (uncommitted) files.
    pub changes: usize,
}

/// One node in a project's file tree. `path` is repo-relative and
/// slash-separated (e.g. `src/main.rs`); `depth` is its slash count.
#[derive(Clone, PartialEq)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub depth: usize,
}

/// One entry in `git status` for the project.
#[derive(Clone, PartialEq)]
pub struct GitChange {
    /// Porcelain-ish code: `M` modified, `A` added, `D` deleted, `?` untracked.
    pub status: String,
    pub path: String,
}

// ── Placeholder data (to be backed by real gateway ops) ──────────────────────

mod data {
    use super::{FileNode, GitChange, Project};

    /// Real repos under `workspaces/` (gateway `projects` op), enriched with
    /// branch + change count (gateway `git` op). Empty if gateway unreachable.
    pub fn projects() -> Vec<Project> {
        // One request only — per-project git status is fetched lazily in the
        // Browser, not here (N+1 blocking round-trips would freeze mount).
        crate::net::request("projects", "")
            .into_iter()
            .filter(|n| !n.is_empty() && !n.starts_with("error:"))
            .map(|name| Project {
                path: format!("workspaces/{name}"),
                name,
                branch: String::new(),
                changes: 0,
            })
            .collect()
    }

    /// Flat depth-annotated tree from `git ls-files` (gateway `tree` op). The UI
    /// synthesizes directory nodes from the file paths.
    pub fn tree(project: &str) -> Vec<FileNode> {
        let files = crate::net::request("tree", project);
        let mut seen_dirs = std::collections::BTreeSet::new();
        let mut nodes: Vec<FileNode> = Vec::new();
        for path in files {
            if path.is_empty() || path.starts_with("error:") { continue; }
            let parts: Vec<&str> = path.split('/').collect();
            // synthesize ancestor dirs
            for d in 1..parts.len() {
                let dir = parts[..d].join("/");
                if seen_dirs.insert(dir.clone()) {
                    nodes.push(FileNode { name: parts[d - 1].to_string(), path: dir, is_dir: true, depth: d - 1 });
                }
            }
            nodes.push(FileNode {
                name: parts.last().unwrap().to_string(),
                path: path.clone(),
                is_dir: false,
                depth: parts.len() - 1,
            });
        }
        nodes.sort_by(|a, b| a.path.cmp(&b.path));
        nodes
    }

    /// File contents (gateway `file` op) — path is `project/relative`.
    pub fn file(project: &str, path: &str) -> String {
        crate::net::request("file", &format!("{project}/{path}")).join("\n")
    }

    /// `git status --porcelain` mapped to change entries (gateway `git` op).
    pub fn git(project: &str) -> Vec<GitChange> {
        crate::net::request("git", project)
            .into_iter()
            .filter(|l| l.len() >= 3 && !l.starts_with("error:"))
            .map(|l| {
                let code = l.trim_start().chars().next().unwrap_or('?');
                let status = match code { 'M' => "M", 'A' => "A", 'D' => "D", 'R' => "R", _ => "?" }.to_string();
                let path = l.get(3..).unwrap_or("").trim().to_string();
                GitChange { status, path }
            })
            .collect()
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Ancestor directory paths of a repo-relative path (`a/b/c.rs` → `["a","a/b"]`).
/// A node is visible only when every ancestor dir is expanded.
fn ancestors(path: &str) -> Vec<String> {
    let parts: Vec<&str> = path.split('/').collect();
    (1..parts.len()).map(|d| parts[..d].join("/")).collect()
}

// ── Component ────────────────────────────────────────────────────────────────

#[component]
pub fn Code() -> Element {
    // Selected project (None = show the picker).
    let selected = use_signal(|| None::<String>);

    rsx! {
        document::Stylesheet { href: CODE_CSS }
        div { class: "code",
            match selected() {
                None => rsx! { Picker { selected } },
                Some(project) => rsx! { Browser { project, selected } },
            }
        }
    }
}

#[component]
fn Picker(selected: Signal<Option<String>>) -> Element {
    let projects = data::projects();
    rsx! {
        div { class: "code-picker",
            div { class: "code-picker-head",
                p { class: "phase", "Code" }
                p { class: "code-sub", "Pick a repo under workspaces/" }
            }
            div { class: "proj-list",
                for p in projects {
                    button {
                        class: "proj",
                        onclick: {
                            let name = p.name.clone();
                            move |_| selected.set(Some(name.clone()))
                        },
                        div { class: "proj-main",
                            span { class: "proj-name", "{p.name}" }
                            span { class: "proj-branch", "⑂ {p.branch}" }
                        }
                        div { class: "proj-meta",
                            if p.changes > 0 {
                                span { class: "proj-changes", "{p.changes} ✎" }
                            } else {
                                span { class: "proj-clean", "clean" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Browser(project: String, selected: Signal<Option<String>>) -> Element {
    // Expanded dir paths (client-side lazy reveal over the flat tree).
    let mut expanded = use_signal(Vec::<String>::new);
    // Currently open file (repo-relative path).
    let mut open_file = use_signal(|| None::<String>);

    let proj_for_tree = project.clone();
    let nodes = use_memo(move || data::tree(&proj_for_tree));
    let proj_for_git = project.clone();
    let changes = use_memo(move || data::git(&proj_for_git));

    // File body loads whenever the open file changes.
    let proj_for_file = project.clone();
    let body = use_memo(move || open_file().map(|p| data::file(&proj_for_file, &p)));

    let toggle = move |path: String| {
        expanded.with_mut(|v| {
            if let Some(i) = v.iter().position(|x| x == &path) {
                v.remove(i);
            } else {
                v.push(path);
            }
        });
    };

    let head_project = project.clone();
    rsx! {
        div { class: "code-browser",
            // Project header + back to picker.
            div { class: "code-head",
                button { class: "back", onclick: move |_| selected.set(None), "‹ repos" }
                span { class: "code-proj", "{head_project}" }
            }

            div { class: "code-body",
                // Left: file tree.
                div { class: "tree",
                    for node in nodes() {
                        {
                            let visible = ancestors(&node.path)
                                .iter()
                                .all(|a| expanded().iter().any(|e| e == a));
                            if !visible {
                                rsx! {}
                            } else if node.is_dir {
                                let is_open = expanded().iter().any(|e| e == &node.path);
                                let path = node.path.clone();
                                let mut toggle = toggle.clone();
                                rsx! {
                                    button {
                                        class: "row dir",
                                        style: "padding-left: {8 + node.depth * 14}px",
                                        onclick: move |_| toggle(path.clone()),
                                        span { class: "tw", if is_open { "▾" } else { "▸" } }
                                        span { class: "nm", "{node.name}/" }
                                    }
                                }
                            } else {
                                let is_open = open_file().as_deref() == Some(node.path.as_str());
                                let path = node.path.clone();
                                rsx! {
                                    button {
                                        class: if is_open { "row file open" } else { "row file" },
                                        style: "padding-left: {8 + node.depth * 14}px",
                                        onclick: move |_| open_file.set(Some(path.clone())),
                                        span { class: "tw", "·" }
                                        span { class: "nm", "{node.name}" }
                                    }
                                }
                            }
                        }
                    }
                }

                // Right: read-only file viewer.
                div { class: "viewer",
                    match (open_file(), body()) {
                        (Some(path), Some(text)) => rsx! {
                            div { class: "viewer-bar", "{path}" }
                            div { class: "viewer-scroll",
                                pre { class: "src",
                                    for (i, line) in text.lines().enumerate() {
                                        div { class: "ln",
                                            span { class: "gutter", "{i + 1}" }
                                            span { class: "code-line", "{line}" }
                                        }
                                    }
                                }
                            }
                        },
                        _ => rsx! {
                            div { class: "viewer-empty", "Select a file to view." }
                        },
                    }
                }
            }

            // Git status strip.
            div { class: "git-strip",
                if changes().is_empty() {
                    span { class: "git-clean", "✓ working tree clean" }
                } else {
                    span { class: "git-title", "changes" }
                    for c in changes() {
                        span { class: "git-item",
                            span { class: "gs gs-{c.status}", "{c.status}" }
                            span { class: "gp", "{c.path}" }
                        }
                    }
                }
            }

            // Project-rooted chat composer (real gateway stream).
            ProjectChat { project }
        }
    }
}

/// Chat rooted at the selected project — reuses the streaming pattern from
/// `chat.rs` but prefixes each message with project context so the fleet scopes
/// its work to that repo.
#[component]
fn ProjectChat(project: String) -> Element {
    let mut msgs = use_signal(Vec::<(&'static str, String)>::new);
    let mut input = use_signal(String::new);
    let mut streaming = use_signal(String::new);
    let mut busy = use_signal(|| false);

    let proj = project.clone();
    let sender = use_coroutine(move |mut rx: UnboundedReceiver<String>| {
        let proj = proj.clone();
        async move {
            while let Some(text) = rx.next().await {
                busy.set(true);
                streaming.set(String::new());
                let scoped = format!("[project: {proj} (workspaces/{proj})]\n{text}");
                let (tx, mut ev_rx) = futures_channel::mpsc::unbounded::<Ev>();
                net::ask("jeeves", &scoped, tx);
                while let Some(ev) = ev_rx.next().await {
                    match ev {
                        Ev::Text(t) => streaming.with_mut(|s| s.push_str(&t)),
                        Ev::Error(e) => streaming.set(format!("⚠ {e}")),
                        Ev::Thinking(_) | Ev::Info(_) => {}
                        Ev::Done => break,
                    }
                }
                let reply = streaming();
                if !reply.trim().is_empty() {
                    msgs.with_mut(|m| m.push(("jeeves", reply)));
                }
                streaming.set(String::new());
                busy.set(false);
            }
        }
    });

    let mut send = move || {
        let text = input().trim().to_string();
        if text.is_empty() || busy() {
            return;
        }
        msgs.with_mut(|m| m.push(("you", text.clone())));
        input.set(String::new());
        sender.send(text);
    };

    rsx! {
        div { class: "code-chat",
            if !msgs().is_empty() || busy() {
                div { class: "code-transcript",
                    for (role, text) in msgs() {
                        div { class: "cbubble {role}",
                            if role == "jeeves" {
                                img { class: "cava", src: JEEVES }
                            }
                            div { class: "ctext", "{text}" }
                        }
                    }
                    if busy() {
                        div { class: "cbubble jeeves",
                            img { class: "cava", src: JEEVES }
                            div { class: "ctext streaming", "{streaming}▋" }
                        }
                    }
                }
            }
            div { class: "composer",
                input {
                    class: "field",
                    placeholder: "Ask the fleet about {project}…",
                    value: "{input}",
                    oninput: move |e| input.set(e.value()),
                    onkeydown: move |e| if e.key() == Key::Enter { send(); },
                }
                button {
                    class: "send",
                    disabled: busy(),
                    onclick: move |_| send(),
                    if busy() { "…" } else { "Send" }
                }
            }
        }
    }
}
