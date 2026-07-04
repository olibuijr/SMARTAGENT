//! Settings screen — edit the gateway **host:port** + **token**, test the
//! connection, and view read-only app info. Owned by the Settings agent
//! (see os/PLAN.md); exposes a small store API the rest of the app reads.
//!
//! ## Persistence
//! Settings are saved as a single JSON file, `settings.json`, under an app data
//! directory resolved (in order) — the SAME strategy as `sessions.rs`:
//!   1. `$SMARTAGENT_OS_DATA/` if set,
//!   2. else `$HOME/.smartagent-os/`,
//!   3. else `./.smartagent-os/` (next to the working dir).
//! On mobile/desktop the app runs as a native webview so `std::fs` persists
//! across launches. On `wasm32` (web build) persistence is a no-op (in-memory
//! only). JSON is hand-encoded/parsed here — zero new deps (Cargo.toml is not
//! touched by this agent).
//!
//! ## Store API
//! Two plain getters that return the persisted value (or the hardcoded default
//! when unset). These are backed by a thread-safe cache — **safe to call from
//! `net.rs`'s background worker threads**, independent of the Dioxus runtime:
//! ```ignore
//! let addr  = crate::settings::gateway_addr();   // "192.168.1.166:9330" default
//! let token = crate::settings::gateway_token();  // "smartagent-os-dev" default
//! ```
//! Reactive access inside a component (subscribes the caller):
//! ```ignore
//! let store = crate::settings::use_settings();
//! let addr  = store.addr();     // reactive read
//! store.save("10.0.0.2:9330", "my-token");  // updates signal + cache + disk
//! ```
#![allow(dead_code)]

use dioxus::prelude::*;

use crate::net;

/// Dev defaults — mirror `net::GATEWAY_ADDR` / `net::GATEWAY_TOKEN`. Used when no
/// value has been persisted yet.
pub const DEFAULT_ADDR: &str = "192.168.1.166:9330";
pub const DEFAULT_TOKEN: &str = "smartagent-os-dev";

// ── Model ─────────────────────────────────────────────────────────────────────

/// Persisted connection settings.
#[derive(Clone, PartialEq, Debug)]
pub struct GatewayConfig {
    /// Gateway TCP bridge address, `host:port`.
    pub addr: String,
    /// Auth token sent on the first line of the connection.
    pub token: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        GatewayConfig { addr: DEFAULT_ADDR.to_string(), token: DEFAULT_TOKEN.to_string() }
    }
}

impl GatewayConfig {
    /// Load from disk (native) or fall back to defaults, never leaving an empty
    /// addr/token (an empty field would break the gateway connection).
    fn load() -> GatewayConfig {
        let mut s = read_from_disk().unwrap_or_default();
        if s.addr.trim().is_empty() {
            s.addr = DEFAULT_ADDR.to_string();
        }
        if s.token.trim().is_empty() {
            s.token = DEFAULT_TOKEN.to_string();
        }
        s
    }

    /// Normalise user input into a stored value, applying defaults on blanks.
    fn from_input(addr: &str, token: &str) -> GatewayConfig {
        let addr = addr.trim();
        let token = token.trim();
        GatewayConfig {
            addr: if addr.is_empty() { DEFAULT_ADDR.to_string() } else { addr.to_string() },
            token: if token.is_empty() { DEFAULT_TOKEN.to_string() } else { token.to_string() },
        }
    }
}

// ── Thread-safe cache (backs the plain getters) ───────────────────────────────
//
// `gateway_addr()` / `gateway_token()` must be readable from `net.rs`'s spawned
// worker threads, which are outside the Dioxus reactive runtime. A `GlobalSignal`
// cannot be read there, so the durable value lives in a `RwLock` cache; the
// reactive signal (below) mirrors it for the UI.

use std::sync::{OnceLock, RwLock};

fn cache() -> &'static RwLock<GatewayConfig> {
    static CACHE: OnceLock<RwLock<GatewayConfig>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(GatewayConfig::load()))
}

/// The persisted gateway address (or [`DEFAULT_ADDR`] if unset). Thread-safe.
pub fn gateway_addr() -> String {
    cache().read().map(|s| s.addr.clone()).unwrap_or_else(|_| DEFAULT_ADDR.to_string())
}

/// The persisted gateway token (or [`DEFAULT_TOKEN`] if unset). Thread-safe.
pub fn gateway_token() -> String {
    cache().read().map(|s| s.token.clone()).unwrap_or_else(|_| DEFAULT_TOKEN.to_string())
}

// ── Reactive store (drives the UI) ────────────────────────────────────────────

/// Global reactive settings — seeded from the same cache so signal + getters
/// start in agreement.
static SETTINGS: GlobalSignal<GatewayConfig> =
    Signal::global(|| cache().read().map(|s| s.clone()).unwrap_or_default());

/// Lightweight `Copy` handle over the settings store. Obtain via [`use_settings`].
#[derive(Clone, Copy, PartialEq)]
pub struct SettingsStore;

/// Hook: get the shared settings store handle.
pub fn use_settings() -> SettingsStore {
    SettingsStore
}

impl SettingsStore {
    /// Current settings (reactive read).
    pub fn get(&self) -> GatewayConfig {
        SETTINGS.read().clone()
    }
    /// Current gateway address (reactive read).
    pub fn addr(&self) -> String {
        SETTINGS.read().addr.clone()
    }
    /// Current gateway token (reactive read).
    pub fn token(&self) -> String {
        SETTINGS.read().token.clone()
    }
    /// Persist new values: updates the reactive signal, the thread-safe cache,
    /// and the on-device JSON file. Blank fields fall back to the defaults.
    pub fn save(&self, addr: &str, token: &str) {
        let next = GatewayConfig::from_input(addr, token);
        *SETTINGS.write() = next.clone();
        if let Ok(mut c) = cache().write() {
            *c = next.clone();
        }
        persist(&next);
    }
}

// ── UI: Settings screen ───────────────────────────────────────────────────────

/// Outcome of a "Test connection" run.
#[derive(Clone, PartialEq, Debug)]
enum TestState {
    Idle,
    Testing,
    Ok(usize),
    Fail,
}

/// Settings form: edit gateway host:port + token, test the connection, and view
/// read-only app info (version + live connection target).
#[component]
pub fn Settings() -> Element {
    let store = use_settings();
    let current = store.get();

    let mut addr = use_signal(|| current.addr.clone());
    let mut token = use_signal(|| current.token.clone());
    let mut show_token = use_signal(|| false);
    let mut saved = use_signal(|| false);
    let mut test = use_signal(|| TestState::Idle);

    // Persist the form and reflect it as the live target.
    let mut do_save = move || {
        store.save(&addr(), &token());
        saved.set(true);
        test.set(TestState::Idle);
    };

    // Save first (so the getters/net target reflect the entered values), then
    // probe the gateway with a read-only `projects` op on a worker thread.
    let run_test = move |_| {
        store.save(&addr(), &token());
        saved.set(true);
        test.set(TestState::Testing);
        spawn(async move {
            let rows = net::request_async("projects", String::new()).await;
            test.set(if rows.is_empty() { TestState::Fail } else { TestState::Ok(rows.len()) });
        });
    };

    let token_type = if show_token() { "text" } else { "password" };
    let live_target = gateway_addr();
    let version = env!("CARGO_PKG_VERSION");

    rsx! {
        document::Stylesheet { href: asset!("/assets/settings.css") }
        div { class: "settings",
            div { class: "settings-inner",
                header { class: "set-head",
                    div { class: "set-dot" }
                    h1 { class: "set-title", "Agent Settings" }
                }
                p { class: "set-sub", "Point the app at your fleet gateway bridge." }

                section { class: "set-card",
                    h2 { class: "set-section", "Gateway" }

                    label { class: "set-field",
                        span { class: "set-label", "Host : Port" }
                        input {
                            class: "set-input",
                            r#type: "text",
                            spellcheck: "false",
                            autocapitalize: "none",
                            autocomplete: "off",
                            placeholder: "{DEFAULT_ADDR}",
                            value: "{addr}",
                            oninput: move |e| {
                                addr.set(e.value());
                                saved.set(false);
                                test.set(TestState::Idle);
                            },
                        }
                        span { class: "set-hint", "TCP bridge address, e.g. 192.168.1.166:9330" }
                    }

                    label { class: "set-field",
                        span { class: "set-label", "Token" }
                        div { class: "set-token",
                            input {
                                class: "set-input",
                                r#type: "{token_type}",
                                spellcheck: "false",
                                autocapitalize: "none",
                                autocomplete: "off",
                                placeholder: "{DEFAULT_TOKEN}",
                                value: "{token}",
                                oninput: move |e| {
                                    token.set(e.value());
                                    saved.set(false);
                                    test.set(TestState::Idle);
                                },
                            }
                            button {
                                class: "set-eye",
                                r#type: "button",
                                onclick: move |_| {
                                    let v = !show_token();
                                    show_token.set(v);
                                },
                                if show_token() { "Hide" } else { "Show" }
                            }
                        }
                        span { class: "set-hint", "Sent as the auth line on connect." }
                    }

                    div { class: "set-actions",
                        button {
                            class: "set-btn primary",
                            onclick: move |_| do_save(),
                            if saved() { "Saved ✓" } else { "Save" }
                        }
                        button {
                            class: "set-btn",
                            disabled: test() == TestState::Testing,
                            onclick: run_test,
                            match test() {
                                TestState::Testing => "Testing…",
                                _ => "Test connection",
                            }
                        }
                        match test() {
                            TestState::Ok(n) => rsx! {
                                span { class: "set-status ok", "Connected · {n} project(s)" }
                            },
                            TestState::Fail => rsx! {
                                span { class: "set-status fail", "Failed — check host & token" }
                            },
                            TestState::Testing => rsx! {
                                span { class: "set-status pending", "Contacting gateway…" }
                            },
                            TestState::Idle => rsx! {},
                        }
                    }
                }

                section { class: "set-card info",
                    h2 { class: "set-section", "About" }
                    div { class: "set-row",
                        span { class: "set-key", "App" }
                        span { class: "set-val", "SMARTAGENT OS" }
                    }
                    div { class: "set-row",
                        span { class: "set-key", "Version" }
                        span { class: "set-val", "v{version}" }
                    }
                    div { class: "set-row",
                        span { class: "set-key", "Connection target" }
                        span { class: "set-val mono", "{live_target}" }
                    }
                }
            }
        }
    }
}

// ── Persistence (native = std::fs JSON; wasm = no-op) ─────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn data_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Ok(dir) = std::env::var("SMARTAGENT_OS_DATA") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("settings.json"));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return Some(PathBuf::from(home).join(".smartagent-os").join("settings.json"));
        }
    }
    Some(PathBuf::from(".smartagent-os").join("settings.json"))
}

#[cfg(not(target_arch = "wasm32"))]
fn read_from_disk() -> Option<GatewayConfig> {
    let path = data_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    decode(&text)
}

#[cfg(not(target_arch = "wasm32"))]
fn persist(s: &GatewayConfig) {
    if let Some(path) = data_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, encode(s));
    }
}

#[cfg(target_arch = "wasm32")]
fn read_from_disk() -> Option<GatewayConfig> {
    None
}

#[cfg(target_arch = "wasm32")]
fn persist(_s: &GatewayConfig) {}

// ── Hand-rolled JSON (encode + minimal string-field parser) ───────────────────

fn encode(s: &GatewayConfig) -> String {
    let mut o = String::new();
    o.push('{');
    o.push_str("\"addr\":\"");
    o.push_str(&esc(&s.addr));
    o.push_str("\",\"token\":\"");
    o.push_str(&esc(&s.token));
    o.push_str("\"}");
    o
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o
}

/// Decode `{"addr":"…","token":"…"}` — only the two string fields we persist.
fn decode(text: &str) -> Option<GatewayConfig> {
    let addr = json_str_field(text, "addr")?;
    let token = json_str_field(text, "token")?;
    Some(GatewayConfig { addr, token })
}

/// Extract a top-level `"key":"value"` string (unescaped) from a flat JSON blob.
fn json_str_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut i = text.find(&needle)? + needle.len();
    let b: Vec<char> = text.chars().collect();
    // skip whitespace + ':'
    while i < b.len() && (b[i].is_whitespace() || b[i] == ':') {
        i += 1;
    }
    if i >= b.len() || b[i] != '"' {
        return None;
    }
    i += 1;
    let mut out = String::new();
    while i < b.len() {
        match b[i] {
            '"' => return Some(out),
            '\\' if i + 1 < b.len() => {
                i += 1;
                match b[i] {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    other => out.push(other),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    None
}
