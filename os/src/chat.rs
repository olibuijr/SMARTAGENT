//! Chat tab — streams jeeves's reply live over the gateway. Agent A layers
//! persistent sessions on top (via `crate::sessions`); Agent B enriches the
//! transcript rendering (via `crate::blocks`). For now it renders plain text.

use dioxus::prelude::*;
use futures_util::StreamExt;

use crate::app::JEEVES;
use crate::net::{self, Ev};

#[derive(Clone, PartialEq)]
pub struct Msg {
    pub role: &'static str, // "you" | "jeeves"
    pub text: String,
}

#[component]
pub fn Chat() -> Element {
    let mut msgs = use_signal(Vec::<Msg>::new);
    let mut input = use_signal(String::new);
    let mut streaming = use_signal(String::new);
    let mut busy = use_signal(|| false);

    let sender = use_coroutine(move |mut rx: UnboundedReceiver<String>| async move {
        while let Some(text) = rx.next().await {
            busy.set(true);
            streaming.set(String::new());
            let (tx, mut ev_rx) = futures_channel::mpsc::unbounded::<Ev>();
            net::ask("jeeves", &text, tx);
            while let Some(ev) = ev_rx.next().await {
                match ev {
                    Ev::Text(t) => streaming.with_mut(|s| s.push_str(&t)),
                    Ev::Error(e) => streaming.set(format!("⚠ {e}")),
                    Ev::Info(_) => {}
                    Ev::Done => break,
                }
            }
            let reply = streaming();
            if !reply.trim().is_empty() {
                msgs.with_mut(|m| m.push(Msg { role: "jeeves", text: reply }));
            }
            streaming.set(String::new());
            busy.set(false);
        }
    });

    let mut send = move || {
        let text = input().trim().to_string();
        if text.is_empty() || busy() {
            return;
        }
        msgs.with_mut(|m| m.push(Msg { role: "you", text: text.clone() }));
        input.set(String::new());
        sender.send(text);
    };

    rsx! {
        div { class: "chat",
            div { class: "transcript",
                if msgs().is_empty() && !busy() {
                    div { class: "empty",
                        img { class: "empty-ava", src: JEEVES }
                        p { "Ask the fleet anything." }
                    }
                }
                for m in msgs() {
                    Bubble { role: m.role, text: m.text }
                }
                if busy() {
                    div { class: "bubble jeeves",
                        img { class: "ava", src: JEEVES }
                        div { class: "body",
                            div { class: "who", "jeeves" }
                            div { class: "text streaming", "{streaming}▋" }
                        }
                    }
                }
            }
            div { class: "composer",
                input {
                    class: "field",
                    placeholder: "Message jeeves…",
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

#[component]
fn Bubble(role: &'static str, text: String) -> Element {
    if role == "jeeves" {
        rsx! {
            div { class: "bubble jeeves",
                img { class: "ava", src: JEEVES }
                div { class: "body",
                    div { class: "who", "jeeves" }
                    div { class: "text", "{text}" }
                }
            }
        }
    } else {
        rsx! {
            div { class: "bubble you",
                div { class: "body",
                    div { class: "who", "you" }
                    div { class: "text", "{text}" }
                }
            }
        }
    }
}
