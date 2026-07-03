//! `menu` — std-only, zero-dependency interactive terminal menus.
//!
//! Primitives: [`select`], [`multiselect`], [`input`], [`password`],
//! [`confirm`]. Each renders a live, fuzzy-filterable UI to `/dev/tty` in raw
//! mode (so stdout stays clean for a captured result) and degrades to a
//! numbered / line prompt when there is no controlling terminal.
//!
//! Reused by the `sa` front door and callable as the `menu` binary from shell
//! (installer / scripts).

mod fuzzy;
mod key;
mod prompt;
mod render;
mod select;
mod tty;

use std::io;

/// Single-select. Returns the chosen item's original index, or None if cancelled.
pub fn select(title: &str, items: &[String]) -> io::Result<Option<usize>> {
    Ok(select::select(title, items, false)?.and_then(|v| v.into_iter().next()))
}

/// Multi-select. Returns chosen original indices (may be empty only via cancel→None).
pub fn multiselect(title: &str, items: &[String]) -> io::Result<Option<Vec<usize>>> {
    select::select(title, items, true)
}

/// Free-text line input. Blank submit returns `default` when given.
pub fn input(title: &str, default: Option<&str>) -> io::Result<Option<String>> {
    prompt::input(title, default, false)
}

/// Masked secret input (no echo).
pub fn password(title: &str) -> io::Result<Option<String>> {
    prompt::input(title, None, true)
}

/// Yes/no confirm. Returns None if cancelled.
pub fn confirm(title: &str, default_yes: bool) -> io::Result<Option<bool>> {
    prompt::confirm(title, default_yes)
}
