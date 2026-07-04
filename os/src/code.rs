//! Code tab — STUB. Owned by a feature agent (see os/PLAN.md). Replace this
//! placeholder with the real tab; expose exactly `pub fn Code() -> Element`.

use dioxus::prelude::*;

#[component]
pub fn Code() -> Element {
    rsx! {
        section { class: "hero",
            p { class: "phase", "Code" }
            p { "Coming soon — building in a worktree agent." }
        }
    }
}
