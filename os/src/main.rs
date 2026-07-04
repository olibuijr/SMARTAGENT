//! SMARTAGENT OS — one Dioxus fullstack frontend (web · desktop · mobile).
//! The shell + tab routing live in `app`; each tab and cross-cutting concern is
//! its own module so features can be built in parallel and merged cleanly.

mod app;
mod blocks; // assistant-ui transcript blocks (Agent B)
mod chat; // Chat tab
mod code; // Code tab (Agent D)
mod cowork; // Cowork tab (Agent C)
mod net; // gateway client (shared, read-only for feature agents)
mod sessions; // persistent chat sessions (Agent A)
mod theme; // system theme + safe area (Agent E)

fn main() {
    dioxus::launch(app::App);
}
