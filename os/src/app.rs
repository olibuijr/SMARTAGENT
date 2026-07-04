//! App shell: head/meta, logo header, tab rail, and screen routing. Owned by
//! the orchestrator — feature modules expose components and are wired here.

use dioxus::prelude::*;

const MAIN_CSS: Asset = asset!("/assets/main.css");
const SESSIONS_CSS: Asset = asset!("/assets/sessions.css");
const BLOCKS_CSS: Asset = asset!("/assets/blocks.css");
const COMMANDS_CSS: Asset = asset!("/assets/commands.css");
pub const LOGO: Asset = asset!("/assets/logo.png");
pub const JEEVES: Asset = asset!("/assets/jeeves.png");

/// Which fleet member the Chat tab talks to (set from the Fleet view).
pub static CHAT_TARGET: GlobalSignal<String> = Signal::global(|| "jeeves".to_string());

#[derive(Clone, PartialEq)]
pub enum Screen {
    Chat,
    Cowork,
    Code,
    More,
    Fleet,
    Tools,
    Runs,
    Notifications,
    Settings,
    Inspector(String),
}

impl Screen {
    /// The primary bottom-nav tab this screen belongs under.
    fn tab(&self) -> u8 {
        match self {
            Screen::Chat => 0,
            Screen::Cowork => 1,
            Screen::Code => 2,
            _ => 3, // everything else lives under "More"
        }
    }
}

#[component]
pub fn App() -> Element {
    let screen = use_signal(|| Screen::Chat);
    rsx! {
        document::Stylesheet { href: MAIN_CSS }
        document::Stylesheet { href: SESSIONS_CSS }
        document::Stylesheet { href: BLOCKS_CSS }
        document::Stylesheet { href: COMMANDS_CSS }
        crate::theme::ThemeMeta {}
        document::Meta {
            name: "viewport",
            content: "width=device-width, initial-scale=1, viewport-fit=cover",
        }
        div { class: "app",
            Rail { screen }
            main { class: "pane",
                header { class: "topbar",
                    img { class: "logo-img", src: LOGO, alt: "SMARTAGENT" }
                }
                {render_screen(screen)}
            }
        }
    }
}

fn render_screen(mut screen: Signal<Screen>) -> Element {
    match screen() {
        Screen::Chat => rsx! { crate::chat::Chat {} },
        Screen::Cowork => rsx! { crate::cowork::Cowork {} },
        Screen::Code => rsx! { crate::code::Code {} },
        Screen::More => rsx! { More { screen } },
        Screen::Fleet => rsx! {
            crate::fleet::Fleet {
                on_chat: move |name: String| {
                    *CHAT_TARGET.write() = name;
                    screen.set(Screen::Chat);
                }
            }
        },
        Screen::Tools => rsx! { crate::tools::Tools {} },
        Screen::Runs => rsx! { crate::runs::Runs {} },
        Screen::Notifications => rsx! { crate::notify::Notifications {} },
        Screen::Settings => rsx! { crate::settings::Settings {} },
        Screen::Inspector(agent) => rsx! { crate::inspector::Inspector { agent } },
    }
}

/// The "More" hub — links to every secondary surface.
#[component]
fn More(mut screen: Signal<Screen>) -> Element {
    let items = [
        (Screen::Fleet, "Fleet", "The 8-agent team — live status"),
        (Screen::Tools, "Tools", "Every extension: memory, vault, rag…"),
        (Screen::Runs, "Runs", "Workflow runs + steps"),
        (Screen::Notifications, "Notifications", "ntfy fleet alerts"),
        (Screen::Inspector("jeeves".into()), "Inspector", "Agent context + tool activity"),
        (Screen::Settings, "Settings", "Gateway host + token"),
    ];
    rsx! {
        div { class: "more",
            for (target, title, desc) in items {
                button {
                    class: "more-item",
                    onclick: move |_| screen.set(target.clone()),
                    div { class: "more-main",
                        span { class: "more-title", "{title}" }
                        span { class: "more-desc", "{desc}" }
                    }
                    span { class: "more-arrow", "›" }
                }
            }
        }
    }
}

fn tab_icon(tab: u8) -> Element {
    let body = match tab {
        0 => rsx! {
            rect { x: 1, y: 2, width: 14, height: 9, fill: "currentColor" }
            rect { x: 3, y: 11, width: 3, height: 3, fill: "currentColor" }
        },
        1 => rsx! {
            rect { x: 1, y: 2, width: 4, height: 12, fill: "currentColor" }
            rect { x: 6, y: 2, width: 4, height: 12, fill: "currentColor" }
            rect { x: 11, y: 2, width: 4, height: 12, fill: "currentColor" }
        },
        2 => rsx! {
            rect { x: 2, y: 6, width: 2, height: 2, fill: "currentColor" }
            rect { x: 4, y: 4, width: 2, height: 2, fill: "currentColor" }
            rect { x: 4, y: 8, width: 2, height: 2, fill: "currentColor" }
            rect { x: 12, y: 6, width: 2, height: 2, fill: "currentColor" }
            rect { x: 10, y: 4, width: 2, height: 2, fill: "currentColor" }
            rect { x: 10, y: 8, width: 2, height: 2, fill: "currentColor" }
        },
        _ => rsx! {
            rect { x: 2, y: 3, width: 3, height: 3, fill: "currentColor" }
            rect { x: 7, y: 3, width: 3, height: 3, fill: "currentColor" }
            rect { x: 12, y: 3, width: 2, height: 3, fill: "currentColor" }
            rect { x: 2, y: 10, width: 3, height: 3, fill: "currentColor" }
            rect { x: 7, y: 10, width: 3, height: 3, fill: "currentColor" }
            rect { x: 12, y: 10, width: 2, height: 3, fill: "currentColor" }
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
fn Rail(screen: Signal<Screen>) -> Element {
    let cur = screen().tab();
    let item = |t: u8, label: &'static str, target: Screen| {
        let active = cur == t;
        rsx! {
            button {
                class: if active { "tab active" } else { "tab" },
                onclick: move |_| screen.set(target.clone()),
                {tab_icon(t)}
                span { class: "tab-label", "{label}" }
            }
        }
    };
    rsx! {
        aside { class: "rail",
            div { class: "logo", "◆" }
            nav {
                {item(0, "Chat", Screen::Chat)}
                {item(1, "Cowork", Screen::Cowork)}
                {item(2, "Code", Screen::Code)}
                {item(3, "More", Screen::More)}
            }
            div { class: "rail-foot", "fleet" }
        }
    }
}
