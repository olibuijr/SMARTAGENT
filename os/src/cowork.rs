//! Cowork tab — STUB. Owned by a feature agent (see os/PLAN.md). Replace this
//! placeholder with the real tab; expose exactly `pub fn Cowork() -> Element`.

use dioxus::prelude::*;

#[component]
pub fn Cowork() -> Element {
    rsx! {
        section { class: "hero",
            p { class: "phase", "Cowork" }
            p { "Coming soon — building in a worktree agent." }
        }
    }
}
