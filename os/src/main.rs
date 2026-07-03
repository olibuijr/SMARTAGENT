//! SMARTAGENT OS — one Dioxus fullstack frontend across web, desktop, and
//! mobile. Chat streams the fleet's `jeeves` over the gateway TCP bridge, in
//! the SMARTAGENT pixel-art brand (logo, avatar, pixel tab icons, pixel font).

mod net;

use dioxus::prelude::*;
use futures_util::StreamExt;
use net::Ev;

const MAIN_CSS: Asset = asset!("/assets/main.css");
const LOGO: Asset = asset!("/assets/logo.png");
const JEEVES: Asset = asset!("/assets/jeeves.png");

fn main() {
    dioxus::launch(App);
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Chat,
    Cowork,
    Code,
}

#[derive(Clone, PartialEq)]
struct Msg {
    role: &'static str, // "you" | "jeeves"
    text: String,
}

#[component]
fn App() -> Element {
    let tab = use_signal(|| Tab::Chat);
    rsx! {
        document::Stylesheet { href: MAIN_CSS }
        div { class: "app",
            Rail { tab }
            main { class: "pane",
                header { class: "topbar",
                    img { class: "logo-img", src: LOGO, alt: "SMARTAGENT" }
                }
                match tab() {
                    Tab::Chat => rsx! { Chat {} },
                    Tab::Cowork => rsx! { Placeholder { name: "Cowork" } },
                    Tab::Code => rsx! { Placeholder { name: "Code" } },
                }
            }
        }
    }
}

/// Pixel-art tab glyph — hand-drawn inline SVG, crisp-edged, themed via CSS
/// `currentColor` so active/inactive coloring is a CSS concern.
fn tab_icon(tab: Tab) -> Element {
    let body = match tab {
        // Speech bubble
        Tab::Chat => rsx! {
            rect { x: 1, y: 2, width: 14, height: 9, fill: "currentColor" }
            rect { x: 3, y: 11, width: 3, height: 3, fill: "currentColor" }
        },
        // Three kanban columns with a card each
        Tab::Cowork => rsx! {
            rect { x: 1, y: 2, width: 4, height: 12, fill: "currentColor" }
            rect { x: 6, y: 2, width: 4, height: 12, fill: "currentColor" }
            rect { x: 11, y: 2, width: 4, height: 12, fill: "currentColor" }
        },
        // </> chevrons
        Tab::Code => rsx! {
            rect { x: 2, y: 6, width: 2, height: 2, fill: "currentColor" }
            rect { x: 4, y: 4, width: 2, height: 2, fill: "currentColor" }
            rect { x: 4, y: 8, width: 2, height: 2, fill: "currentColor" }
            rect { x: 12, y: 6, width: 2, height: 2, fill: "currentColor" }
            rect { x: 10, y: 4, width: 2, height: 2, fill: "currentColor" }
            rect { x: 10, y: 8, width: 2, height: 2, fill: "currentColor" }
        },
    };
    rsx! {
        svg {
            class: "tab-ico",
            width: "22",
            height: "22",
            view_box: "0 0 16 16",
            shape_rendering: "crispEdges",
            {body}
        }
    }
}

#[component]
fn Rail(tab: Signal<Tab>) -> Element {
    let item = |t: Tab, label: &'static str| {
        let active = tab() == t;
        rsx! {
            button {
                class: if active { "tab active" } else { "tab" },
                onclick: move |_| tab.set(t),
                {tab_icon(t)}
                span { class: "tab-label", "{label}" }
            }
        }
    };
    rsx! {
        aside { class: "rail",
            div { class: "logo", "◆" }
            nav {
                {item(Tab::Chat, "Chat")}
                {item(Tab::Cowork, "Cowork")}
                {item(Tab::Code, "Code")}
            }
            div { class: "rail-foot", "fleet" }
        }
    }
}

#[component]
fn Placeholder(name: String) -> Element {
    rsx! {
        section { class: "hero",
            p { class: "phase", "{name}" }
            p { "Porting from desktop-agent next." }
        }
    }
}

/// Chat tab: type a message, stream jeeves's reply live over the gateway.
#[component]
fn Chat() -> Element {
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
                    if m.role == "jeeves" {
                        div { class: "bubble jeeves",
                            img { class: "ava", src: JEEVES }
                            div { class: "body",
                                div { class: "who", "jeeves" }
                                div { class: "text", "{m.text}" }
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
