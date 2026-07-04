//! Slash commands + multiple-choice dialogs — Agent F (see os/PLAN.md).
//!
//! Two independent surfaces the orchestrator wires into `chat.rs` at merge:
//!
//! 1. **Slash-command palette.** When the composer input starts with `/`, the
//!    chat view renders [`CommandPalette`] above the field. It filters the
//!    [`commands`] registry on whatever follows the leading slash and calls
//!    `on_pick(name)` when a row is tapped/clicked. The orchestrator decides
//!    what a pick does (send it as an op, replace the input, etc.).
//!
//! 2. **Multiple-choice questions.** When the agent needs the user to choose,
//!    it emits a `{"ev":"choice",…}` line that [`parse_choice`] turns into a
//!    [`Choice`]; the chat loop then shows [`ChoiceDialog`] as a modal and
//!    forwards the picked option (or free-text "Other…") back to the fleet via
//!    `on_answer(option)`.
//!
//! ## What chat.rs calls
//!
//! ```ignore
//! // Slash palette — show while the input starts with '/':
//! if input().starts_with('/') {
//!     CommandPalette {
//!         query: input(),
//!         on_pick: move |name: String| input.set(format!("/{name} ")),
//!     }
//! }
//!
//! // Choice modal — when a parsed Choice is pending:
//! if let Some(c) = pending_choice() {
//!     ChoiceDialog {
//!         choice: c,
//!         on_answer: move |ans: String| { /* send ans, clear pending */ },
//!     }
//! }
//! ```
#![allow(dead_code)]

use dioxus::prelude::*;

// ─────────────────────────────────────────────────────────────────────────────
// Slash commands
// ─────────────────────────────────────────────────────────────────────────────

/// One entry in the slash-command palette.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cmd {
    /// The command name **without** the leading slash (e.g. `"board"`).
    pub name: &'static str,
    /// One-line description shown next to the name.
    pub desc: &'static str,
}

/// The TUI/desktop-parity slash-command set. Order is the display order.
pub fn commands() -> Vec<Cmd> {
    vec![
        Cmd { name: "board", desc: "Show the kanban board" },
        Cmd { name: "status", desc: "Fleet + infra health snapshot" },
        Cmd { name: "team", desc: "Agent fleet panel — cards, context, tasks" },
        Cmd { name: "tasks", desc: "List tasks (add / advance / done)" },
        Cmd { name: "runs", desc: "Workflow runs and step logs" },
        Cmd { name: "skills", desc: "Browse and manage skills" },
        Cmd { name: "memory", desc: "Search project memory" },
        Cmd { name: "projects", desc: "Workspaces under workspaces/" },
        Cmd { name: "new", desc: "Start a new chat session" },
        Cmd { name: "rename", desc: "Rename the current session" },
        Cmd { name: "help", desc: "List available commands" },
    ]
}

/// Filter [`commands`] by a raw composer `query`. A single leading `/` is
/// stripped; matching is case-insensitive and hits either the name or the
/// description. An empty query (just `/`) returns the whole set.
pub fn filter_commands(query: &str) -> Vec<Cmd> {
    let q = query.strip_prefix('/').unwrap_or(query).trim().to_lowercase();
    if q.is_empty() {
        return commands();
    }
    commands()
        .into_iter()
        .filter(|c| c.name.to_lowercase().contains(&q) || c.desc.to_lowercase().contains(&q))
        .collect()
}

/// The filterable slash-command menu. Render it above the composer whenever the
/// input starts with `/`. Each row is a tappable button that calls
/// `on_pick(name)` with the bare command name (no slash).
#[component]
pub fn CommandPalette(query: String, on_pick: EventHandler<String>) -> Element {
    let matches = filter_commands(&query);

    rsx! {
        div { class: "cmd-palette", role: "listbox", "aria-label": "Slash commands",
            if matches.is_empty() {
                div { class: "cmd-empty", "No commands match" }
            } else {
                for c in matches {
                    button {
                        class: "cmd-row",
                        role: "option",
                        key: "{c.name}",
                        onclick: move |_| on_pick.call(c.name.to_string()),
                        span { class: "cmd-name", "/{c.name}" }
                        span { class: "cmd-desc", "{c.desc}" }
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Multiple-choice questions
// ─────────────────────────────────────────────────────────────────────────────

/// An agent-posed multiple-choice question.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Choice {
    /// The prompt shown to the user.
    pub question: String,
    /// The offered options. A free-text "Other…" is always added by the dialog.
    pub options: Vec<String>,
}

/// Themed modal card asking the user to pick one of `choice.options`. Each
/// option is a big tappable button; picking one calls `on_answer(option)`. An
/// "Other…" button reveals a free-text field whose submitted value is passed
/// through `on_answer` verbatim.
#[component]
pub fn ChoiceDialog(choice: Choice, on_answer: EventHandler<String>) -> Element {
    let mut other_open = use_signal(|| false);
    let mut other_text = use_signal(String::new);

    let submit_other = move || {
        let t = other_text().trim().to_string();
        if !t.is_empty() {
            on_answer.call(t);
        }
    };

    rsx! {
        div { class: "choice-overlay",
            div { class: "choice-card", role: "dialog", "aria-modal": "true",
                div { class: "choice-q", "{choice.question}" }
                div { class: "choice-opts",
                    for opt in choice.options.clone() {
                        button {
                            class: "choice-opt",
                            key: "{opt}",
                            onclick: move |_| on_answer.call(opt.clone()),
                            "{opt}"
                        }
                    }

                    if other_open() {
                        div { class: "choice-other",
                            input {
                                class: "choice-input",
                                placeholder: "Type your answer…",
                                autofocus: true,
                                value: "{other_text}",
                                oninput: move |e| other_text.set(e.value()),
                                onkeydown: move |e| if e.key() == Key::Enter { submit_other(); },
                            }
                            button {
                                class: "choice-send",
                                disabled: other_text().trim().is_empty(),
                                onclick: move |_| submit_other(),
                                "Send"
                            }
                        }
                    } else {
                        button {
                            class: "choice-opt choice-opt-other",
                            onclick: move |_| other_open.set(true),
                            "Other…"
                        }
                    }
                }
            }
        }
    }
}

/// Recognise an agent-emitted choice event and decode it into a [`Choice`].
///
/// Expected shape (flat, one line):
/// `{"ev":"choice","question":"…","options":["a","b"]}`
///
/// Returns `None` for any line that isn't a choice event or that carries no
/// options. Uses a dependency-free scan (matching net.rs's hot-path style)
/// rather than a JSON crate.
//
// TODO(orchestrator): confirm final event shape. If the gateway wraps the
// payload (e.g. `{"ev":"choice","data":{…}}`) or renames fields, adjust the
// field keys below; the parser is intentionally the only place that knows the
// wire format.
pub fn parse_choice(json_line: &str) -> Option<Choice> {
    let line = json_line.trim();
    if json_string_field(line, "ev")?.as_str() != "choice" {
        return None;
    }
    let question = json_string_field(line, "question")?;
    let options = json_string_array(line, "options")?;
    if options.is_empty() {
        return None;
    }
    Some(Choice { question, options })
}

/// Extract a top-level `"key":"value"` string (JSON-unescaped) from a flat line.
fn json_string_field(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = line.find(&needle)? + needle.len();
    let (value, _end) = scan_json_string(&line[start..])?;
    Some(value)
}

/// Extract a top-level `"key":["a","b",…]` array of strings.
fn json_string_array(line: &str, key: &str) -> Option<Vec<String>> {
    let needle = format!("\"{key}\":[");
    let start = line.find(&needle)? + needle.len();
    let bytes = line.as_bytes();
    let mut i = start;
    let mut out: Vec<String> = Vec::new();
    loop {
        // Skip whitespace/separators up to the next `"` or the closing `]`.
        while i < bytes.len() && bytes[i] != b'"' && bytes[i] != b']' {
            i += 1;
        }
        match bytes.get(i)? {
            b']' => return Some(out),
            b'"' => {
                let (value, consumed) = scan_json_string(&line[i + 1..])?;
                out.push(value);
                i += 1 + consumed; // opening quote + string body + closing quote
            }
            _ => return Some(out),
        }
    }
}

/// Decode a JSON string body starting just after the opening quote. Returns the
/// unescaped value and the number of bytes consumed up to and including the
/// closing quote.
fn scan_json_string(s: &str) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => {
                match bytes[i + 1] {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'"' => out.push(b'"'),
                    b'\\' => out.push(b'\\'),
                    b'/' => out.push(b'/'),
                    other => out.push(other),
                }
                i += 2;
            }
            b'"' => return Some((String::from_utf8_lossy(&out).into_owned(), i + 1)),
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_all_expected_commands() {
        let names: Vec<&str> = commands().iter().map(|c| c.name).collect();
        for want in [
            "board", "status", "team", "tasks", "runs", "skills", "memory", "projects", "new",
            "rename", "help",
        ] {
            assert!(names.contains(&want), "missing /{want}");
        }
    }

    #[test]
    fn filter_strips_leading_slash_and_matches_name() {
        let hits = filter_commands("/bo");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "board");
    }

    #[test]
    fn filter_empty_returns_all() {
        assert_eq!(filter_commands("/").len(), commands().len());
    }

    #[test]
    fn filter_matches_description() {
        let hits = filter_commands("/kanban");
        assert!(hits.iter().any(|c| c.name == "board"));
    }

    #[test]
    fn parse_choice_basic() {
        let line = r#"{"ev":"choice","question":"Deploy now?","options":["Yes","No"]}"#;
        let c = parse_choice(line).expect("should parse");
        assert_eq!(c.question, "Deploy now?");
        assert_eq!(c.options, vec!["Yes".to_string(), "No".to_string()]);
    }

    #[test]
    fn parse_choice_unescapes_and_handles_spacing() {
        let line = r#"{"ev":"choice","question":"Pick a \"lane\"","options":[ "left" , "right" ]}"#;
        let c = parse_choice(line).expect("should parse");
        assert_eq!(c.question, "Pick a \"lane\"");
        assert_eq!(c.options, vec!["left".to_string(), "right".to_string()]);
    }

    #[test]
    fn parse_choice_rejects_other_events() {
        assert!(parse_choice(r#"{"ev":"text","data":"hi"}"#).is_none());
    }

    #[test]
    fn parse_choice_rejects_empty_options() {
        assert!(parse_choice(r#"{"ev":"choice","question":"q","options":[]}"#).is_none());
    }
}
