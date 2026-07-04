//! System theme + safe area — Agent E (see os/PLAN.md).
//!
//! The color palette + safe-area insets live in `assets/theme.css`. This module
//! injects that sheet into the document head and declares `color-scheme` so the
//! page follows the OS light/dark preference on every target.
//!
//! Native (Android system WebView) follows the OS only when the app also ships
//! a DayNight theme + `setAlgorithmicDarkeningAllowed(true)` — configured in
//! `Dioxus.toml` and `android/MainActivity.kt`. The web target follows the OS
//! directly via `prefers-color-scheme`, so no Rust-side signal is required; the
//! whole thing is CSS-driven. A detector could be added behind `#[cfg]` later if
//! a component ever needs to branch on the active scheme in Rust.
#![allow(dead_code)]

use dioxus::prelude::*;

const THEME_CSS: Asset = asset!("/assets/theme.css");

/// Head nodes: link the theme stylesheet and advertise dual scheme support.
fn theme_head() -> Element {
    rsx! {
        document::Stylesheet { href: THEME_CSS }
        document::Meta { name: "color-scheme", content: "light dark" }
    }
}

/// Theme head component. Wire once near the top of the shell: `ThemeMeta {}`.
/// Owns `color-scheme` for the app — the orchestrator can drop the duplicate
/// `document::Meta { name: "color-scheme", ... }` in `app.rs` once this is wired.
#[component]
pub fn ThemeMeta() -> Element {
    theme_head()
}

/// Function-call alias for callers that prefer `apply_theme()` over a component.
/// Returns the same head nodes as [`ThemeMeta`]; use inside an `rsx!` tree.
pub fn apply_theme() -> Element {
    theme_head()
}
