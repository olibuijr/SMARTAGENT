//! SMARTAGENT OS — one Dioxus fullstack frontend across web, desktop, and
//! mobile. Phase (a): prove the single-codebase pipeline — the shell renders
//! on web and on the phone, no gateway wiring yet. Feature parity with the egui
//! `desktop-agent` (sidebar / chat / cowork-board / inspector) ports in next,
//! as Dioxus components, with the fleet gateway as the backend.

use dioxus::prelude::*;

const MAIN_CSS: Asset = asset!("/assets/main.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Stylesheet { href: MAIN_CSS }
        Shell {}
    }
}

/// The app shell: sidebar rail + main pane. A skeleton of the desktop-agent
/// layout so the port has a home to grow into.
#[component]
fn Shell() -> Element {
    let mut taps = use_signal(|| 0u32);
    rsx! {
        div { class: "app",
            aside { class: "rail",
                div { class: "logo", "◆" }
                nav {
                    button { class: "tab active", "Chat" }
                    button { class: "tab", "Cowork" }
                    button { class: "tab", "Code" }
                }
                div { class: "rail-foot", "fleet" }
            }
            main { class: "pane",
                header { class: "topbar",
                    h1 { "SMARTAGENT" }
                    span { class: "sub", "OS — one codebase: web · desktop · mobile" }
                }
                section { class: "hero",
                    p { class: "phase", "Phase (a): pipeline proof." }
                    p { "Dioxus fullstack shell. Gateway wiring next." }
                    button {
                        class: "cta",
                        onclick: move |_| taps += 1,
                        "Tap to confirm input"
                    }
                    p { class: "count", "taps: {taps}" }
                }
            }
        }
    }
}
