//! Assistant-UI transcript blocks — Agent B (see os/PLAN.md).
//!
//! Renders a fleet turn as an ordered list of rich blocks instead of one flat
//! text bubble: streaming assistant text (with ```code fences```), a collapsible
//! **thinking** disclosure, live **tool-call cards** (running → green ✓ / red ✗),
//! and **file-edit diffs** (red dels / green adds).
//!
//! ## What chat.rs calls
//!
//! Two public surfaces:
//!
//! - [`parse_events`] — turn the gateway event stream into `Vec<Block>`.
//!   Feed it the events collected for one assistant turn (the same `Ev`s the
//!   streaming loop in `chat.rs` already `match`es on). Text deltas accumulate
//!   into `Block::Text`; each `🛠 <tool> running…` info line opens a Running
//!   tool card that flips to `Ok`/`Fail` on the matching `✓`/`✗` line.
//! - [`BlockView`] — `#[component]` that renders one `Block`. Render a turn with
//!   `for b in parse_events(&evs) { BlockView { block: b } }`, or use the
//!   [`Transcript`] convenience component.
//!
//! ### Adoption sketch for chat.rs
//!
//! ```ignore
//! // collect every Ev for the in-flight turn (Text/Info/Error/Done)…
//! let mut evs = use_signal(Vec::<Ev>::new);
//! // …in the stream loop: evs.with_mut(|v| v.push(ev.clone()));
//! // …then render:
//! Transcript { blocks: parse_events(&evs()) }
//! ```
//!
//! Tool cards only appear once the gateway client forwards `Ev::Info` for the
//! `🛠` lines (net.rs currently swallows them). Thinking and diff blocks have no
//! stream source yet — see the `TODO(orchestrator)` seam in [`parse_events`].
#![allow(dead_code)]

use dioxus::prelude::*;

use crate::net::Ev;

/// Live state of a tool-call card.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolStatus {
    /// The tool is executing (amber, pulsing pill).
    Running,
    /// Finished successfully (green ✓).
    Ok,
    /// Finished with an error (red ✗).
    Fail,
}

/// One line of a file-edit diff. `Ctx` is unchanged context; `Add`/`Del` render
/// green/red with `+`/`-` gutter markers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DiffLine {
    Ctx(String),
    Add(String),
    Del(String),
}

/// One rendered unit of a fleet turn's transcript.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Block {
    /// Assistant text. Newlines are preserved; ```fenced``` spans render as
    /// monospace code blocks.
    Text(String),
    /// Chain-of-thought, shown as a collapsible dim disclosure.
    Thinking(String),
    /// A tool invocation card with a live status pill.
    Tool { name: String, status: ToolStatus },
    /// A file edit, rendered as a red/green unified diff.
    Diff { path: String, hunks: Vec<DiffLine> },
}

/// The hammer-and-wrench marker the gateway prefixes on tool activity info lines.
const TOOL_MARK: char = '🛠';

/// Parse one turn's worth of gateway events into an ordered list of blocks.
///
/// Rules:
/// - Consecutive [`Ev::Text`] deltas accumulate; the run is flushed to a single
///   [`Block::Text`] whenever a non-text event breaks it (so a tool card lands
///   between the text before and after it, preserving order).
/// - An [`Ev::Info`] line of the form `🛠 <tool> running…` opens a
///   [`ToolStatus::Running`] card; `🛠 <tool> ✓` / `🛠 <tool> ✗` flip the most
///   recent matching open card to [`ToolStatus::Ok`] / [`ToolStatus::Fail`].
///   Non-`🛠` info lines are status noise and are ignored.
/// - [`Ev::Error`] is appended to the text stream as a `⚠` line.
/// - [`Ev::Done`] is ignored (turn boundary is the caller's concern).
pub fn parse_events(events: &[Ev]) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut text = String::new();

    let mut thinking = String::new();
    for ev in events {
        // A thinking run and a text run are separate blocks; when one starts,
        // flush the other so order is preserved.
        if !matches!(ev, Ev::Thinking(_)) {
            flush_thinking(&mut thinking, &mut blocks);
        }
        match ev {
            Ev::Text(t) => text.push_str(t),
            Ev::Thinking(t) => {
                flush_text(&mut text, &mut blocks);
                thinking.push_str(t);
            }
            Ev::Info(s) => {
                if let Some((name, status)) = parse_tool_info(s) {
                    flush_text(&mut text, &mut blocks);
                    apply_tool(&mut blocks, name, status);
                }
                // TODO(orchestrator): file-diffs (`Block::Diff`) once the gateway
                // forwards tool I/O.
            }
            Ev::Error(e) => {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str("⚠ ");
                text.push_str(e);
            }
            Ev::Done => {}
        }
    }
    flush_text(&mut text, &mut blocks);
    flush_thinking(&mut thinking, &mut blocks);
    blocks
}

/// Flush an accumulated thinking run into a `Block::Thinking`.
fn flush_thinking(thinking: &mut String, blocks: &mut Vec<Block>) {
    if !thinking.trim().is_empty() {
        blocks.push(Block::Thinking(std::mem::take(thinking)));
    } else {
        thinking.clear();
    }
}

/// Recognise a `🛠` tool info line. Returns `(tool_name, status)` or `None` for
/// any line that isn't tool activity.
fn parse_tool_info(s: &str) -> Option<(String, ToolStatus)> {
    let s = s.trim();
    let rest = s.strip_prefix(TOOL_MARK)?;
    // Drop a trailing emoji variation selector (🛠️) if present.
    let rest = rest.strip_prefix('\u{FE0F}').unwrap_or(rest).trim();

    if let Some(name) = rest.strip_suffix('✓') {
        return Some((name.trim().to_string(), ToolStatus::Ok));
    }
    if let Some(name) = rest.strip_suffix('✗') {
        return Some((name.trim().to_string(), ToolStatus::Fail));
    }
    // Otherwise it's a start line: strip a trailing "running…"/"running..." tag.
    let mut name = rest;
    for suffix in ["running…", "running...", "running"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped;
            break;
        }
    }
    Some((name.trim().to_string(), ToolStatus::Running))
}

/// Apply a parsed tool event: a Running status opens a fresh card; an Ok/Fail
/// flips the most recent still-open card with the same name (or, if none is
/// open, records a standalone terminal card).
fn apply_tool(blocks: &mut Vec<Block>, name: String, status: ToolStatus) {
    if status == ToolStatus::Running {
        blocks.push(Block::Tool { name, status });
        return;
    }
    for b in blocks.iter_mut().rev() {
        if let Block::Tool { name: n, status: st } = b {
            if *n == name && *st == ToolStatus::Running {
                *st = status;
                return;
            }
        }
    }
    blocks.push(Block::Tool { name, status });
}

/// Flush accumulated text into a `Block::Text` (dropping whitespace-only runs).
fn flush_text(buf: &mut String, blocks: &mut Vec<Block>) {
    if buf.trim().is_empty() {
        buf.clear();
    } else {
        blocks.push(Block::Text(std::mem::take(buf)));
    }
}

/// Render a whole parsed turn. Convenience wrapper over [`BlockView`].
#[component]
pub fn Transcript(blocks: Vec<Block>) -> Element {
    rsx! {
        div { class: "blocks",
            for block in blocks {
                BlockView { block }
            }
        }
    }
}

/// Render a single [`Block`].
#[component]
pub fn BlockView(block: Block) -> Element {
    // Disclosure state for `Block::Thinking` (unconditional hook: keeps hook
    // order stable across the match arms below).
    let mut open = use_signal(|| false);

    match block {
        Block::Text(t) => rsx! {
            div { class: "blk blk-text", {rich_text(&t)} }
        },

        Block::Thinking(t) => rsx! {
            div { class: "blk blk-think",
                button {
                    class: "think-head",
                    onclick: move |_| open.set(!open()),
                    span { class: "think-caret", if open() { "▾" } else { "▸" } }
                    span { class: "think-label", "thinking" }
                }
                if open() {
                    div { class: "think-body", "{t}" }
                }
            }
        },

        Block::Tool { name, status } => {
            let (cls, label, sym) = match status {
                ToolStatus::Running => ("running", "running", "●"),
                ToolStatus::Ok => ("ok", "done", "✓"),
                ToolStatus::Fail => ("fail", "failed", "✗"),
            };
            rsx! {
                div { class: "blk blk-tool tool-{cls}",
                    span { class: "tool-ico", "🛠" }
                    span { class: "tool-name", "{name}" }
                    span { class: "tool-pill pill-{cls}",
                        span { class: "pill-sym", "{sym}" }
                        span { class: "pill-txt", "{label}" }
                    }
                }
            }
        }

        Block::Diff { path, hunks } => rsx! {
            div { class: "blk blk-diff",
                div { class: "diff-head", "{path}" }
                div { class: "diff-body",
                    for line in hunks {
                        {diff_line(line)}
                    }
                }
            }
        },
    }
}

/// Render one diff line with its gutter marker + colour class.
fn diff_line(line: DiffLine) -> Element {
    let (cls, gutter, text) = match line {
        DiffLine::Ctx(s) => ("dl-ctx", " ", s),
        DiffLine::Add(s) => ("dl-add", "+", s),
        DiffLine::Del(s) => ("dl-del", "-", s),
    };
    rsx! {
        div { class: "dl {cls}",
            span { class: "dl-gutter", "{gutter}" }
            span { class: "dl-txt", "{text}" }
        }
    }
}

/// Render assistant text, splitting ```fenced``` spans into monospace blocks.
/// Even segments are prose (newlines preserved via CSS); odd segments are code.
fn rich_text(text: &str) -> Element {
    let nodes: Vec<Element> = text_segments(text)
        .into_iter()
        .map(|(is_code, content)| {
            if is_code {
                rsx! { pre { class: "code-fence", "{content}" } }
            } else {
                rsx! { span { class: "seg-text", "{content}" } }
            }
        })
        .collect();
    rsx! { {nodes.into_iter()} }
}

/// Split text on ``` fences into `(is_code, content)` segments, dropping empties.
fn text_segments(text: &str) -> Vec<(bool, String)> {
    text.split("```")
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .map(|(i, s)| {
            let is_code = i % 2 == 1;
            let content = if is_code {
                strip_lang(s)
            } else {
                s.to_string()
            };
            (is_code, content)
        })
        .filter(|(_, s)| !s.is_empty())
        .collect()
}

/// Strip a leading language token line (e.g. `rust\n…`) and the fence's edge
/// newlines from a code segment.
fn strip_lang(seg: &str) -> String {
    if let Some((first, rest)) = seg.split_once('\n') {
        let f = first.trim();
        let is_lang = !f.is_empty()
            && f.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '+' | '-' | '_' | '#'));
        if is_lang {
            return rest.trim_matches('\n').to_string();
        }
    }
    seg.trim_matches('\n').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_start_then_ok() {
        let evs = [
            Ev::Text("searching ".into()),
            Ev::Info("🛠 grep running…".into()),
            Ev::Info("🛠 grep ✓".into()),
            Ev::Text("found it".into()),
        ];
        let blocks = parse_events(&evs);
        assert_eq!(
            blocks,
            vec![
                Block::Text("searching ".into()),
                Block::Tool { name: "grep".into(), status: ToolStatus::Ok },
                Block::Text("found it".into()),
            ]
        );
    }

    #[test]
    fn tool_start_then_fail() {
        let evs = [
            Ev::Info("🛠 build running…".into()),
            Ev::Info("🛠 build ✗".into()),
        ];
        let blocks = parse_events(&evs);
        assert_eq!(
            blocks,
            vec![Block::Tool { name: "build".into(), status: ToolStatus::Fail }]
        );
    }

    #[test]
    fn interleaved_text_and_tools() {
        let evs = [
            Ev::Text("let me look. ".into()),
            Ev::Info("🛠 read running…".into()),
            Ev::Text("meanwhile ".into()), // text arriving during the tool run
            Ev::Info("🛠 read ✓".into()),
            Ev::Info("🛠 edit running…".into()),
            Ev::Info("🛠 edit ✗".into()),
            Ev::Text("done.".into()),
            Ev::Done,
        ];
        let blocks = parse_events(&evs);
        assert_eq!(
            blocks,
            vec![
                Block::Text("let me look. ".into()),
                Block::Tool { name: "read".into(), status: ToolStatus::Ok },
                Block::Text("meanwhile ".into()),
                Block::Tool { name: "edit".into(), status: ToolStatus::Fail },
                Block::Text("done.".into()),
            ]
        );
    }

    #[test]
    fn ok_flips_matching_open_card_by_name() {
        // Out-of-order completion: two tools open, first one finishes.
        let evs = [
            Ev::Info("🛠 alpha running…".into()),
            Ev::Info("🛠 beta running…".into()),
            Ev::Info("🛠 alpha ✓".into()),
        ];
        let blocks = parse_events(&evs);
        assert_eq!(
            blocks,
            vec![
                Block::Tool { name: "alpha".into(), status: ToolStatus::Ok },
                Block::Tool { name: "beta".into(), status: ToolStatus::Running },
            ]
        );
    }

    #[test]
    fn code_fence_becomes_two_segments() {
        let segs = text_segments("before ```rust\nfn x() {}\n``` after");
        assert_eq!(
            segs,
            vec![
                (false, "before ".to_string()),
                (true, "fn x() {}".to_string()),
                (false, " after".to_string()),
            ]
        );
    }

    #[test]
    fn error_event_appended_as_warning_text() {
        let evs = [Ev::Text("hi".into()), Ev::Error("boom".into())];
        let blocks = parse_events(&evs);
        assert_eq!(blocks, vec![Block::Text("hi\n⚠ boom".into())]);
    }
}
