//! Chat tab — persistent sessions (Agent A) + rich assistant-ui transcript
//! blocks (Agent B) + slash command palette (Agent F), streaming jeeves over
//! the gateway. The orchestrator wires the modules together here.

use dioxus::prelude::*;
use futures_util::StreamExt;

use crate::app::JEEVES;
use crate::blocks::{parse_events, Block, BlockView};
use crate::commands::CommandPalette;
use crate::net::{self, Ev};
use crate::sessions::{use_sessions, SessionBar};

/// Join the text of a parsed block list — what we persist for an assistant turn
/// (thinking/tool cards are live-only; the durable record is the reply text).
fn blocks_to_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for b in blocks {
        if let Block::Text(t) = b {
            out.push_str(t);
        }
    }
    out
}

#[component]
pub fn Chat() -> Element {
    let sess = use_sessions();
    let mut input = use_signal(String::new);
    let mut live = use_signal(Vec::<Ev>::new); // events of the in-flight turn
    let mut busy = use_signal(|| false);

    let sender = use_coroutine(move |mut rx: UnboundedReceiver<String>| async move {
        while let Some(text) = rx.next().await {
            busy.set(true);
            live.set(Vec::new());
            let (tx, mut ev_rx) = futures_channel::mpsc::unbounded::<Ev>();
            net::ask("jeeves", &text, tx);
            while let Some(ev) = ev_rx.next().await {
                match ev {
                    Ev::Done => break,
                    other => live.with_mut(|v| v.push(other)),
                }
            }
            let reply = blocks_to_text(&parse_events(&live()));
            if !reply.trim().is_empty() {
                sess.push_message(sess.active_id(), "jeeves", &reply);
            }
            live.set(Vec::new());
            busy.set(false);
        }
    });

    // Send the current input; `/new` is a local session command, everything
    // else goes to the fleet (a slash command sends its bare word).
    let mut submit = move |raw: String| {
        let text = raw.trim().to_string();
        if text.is_empty() || busy() {
            return;
        }
        if text == "/new" {
            sess.create();
            input.set(String::new());
            return;
        }
        let msg = text.strip_prefix('/').unwrap_or(&text).to_string();
        sess.push_message(sess.active_id(), "you", &text);
        input.set(String::new());
        sender.send(msg);
    };

    let show_palette = input().starts_with('/') && !busy();
    let history = sess.active_messages();

    rsx! {
        div { class: "chat",
            SessionBar {}
            div { class: "transcript",
                if history.is_empty() && !busy() {
                    div { class: "empty",
                        img { class: "empty-ava", src: JEEVES }
                        p { "Ask the fleet anything." }
                    }
                }
                for m in history.iter() {
                    if m.role == "jeeves" {
                        div { class: "bubble jeeves",
                            img { class: "ava", src: JEEVES }
                            div { class: "body",
                                div { class: "who", "jeeves" }
                                for b in parse_events(&[Ev::Text(m.text.clone())]) {
                                    BlockView { block: b }
                                }
                            }
                        }
                    } else {
                        div { class: "bubble you",
                            div { class: "body",
                                div { class: "who", "you" }
                                div { class: "text", "{m.text}" }
                            }
                        }
                    }
                }
                if busy() {
                    div { class: "bubble jeeves",
                        img { class: "ava", src: JEEVES }
                        div { class: "body",
                            div { class: "who", "jeeves" }
                            for b in parse_events(&live()) {
                                BlockView { block: b }
                            }
                            if live().is_empty() {
                                div { class: "text streaming", "▋" }
                            }
                        }
                    }
                }
            }
            if show_palette {
                CommandPalette {
                    query: input(),
                    on_pick: move |name: String| {
                        if name == "new" {
                            sess.create();
                            input.set(String::new());
                        } else {
                            input.set(String::new());
                            submit(format!("/{name}"));
                        }
                    },
                }
            }
            div { class: "composer",
                input {
                    class: "field",
                    placeholder: "Message jeeves…  (/ for commands)",
                    value: "{input}",
                    oninput: move |e| input.set(e.value()),
                    onkeydown: move |e| if e.key() == Key::Enter {
                        let v = input();
                        submit(v);
                    },
                }
                button {
                    class: "send",
                    disabled: busy(),
                    onclick: move |_| {
                        let v = input();
                        submit(v);
                    },
                    if busy() { "…" } else { "Send" }
                }
            }
        }
    }
}
