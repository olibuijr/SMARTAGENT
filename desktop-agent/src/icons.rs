//! Icon font. Embeds JetBrains Mono Nerd Font (Font Awesome glyphs in the PUA)
//! and installs it as a fallback family so the UI's icons render instead of
//! tofu boxes. Named constants map each UI affordance to a Font Awesome glyph.

use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

/// Load the Nerd Font and append it as the last fallback in both families, so
/// normal text keeps the default face and only missing glyphs (our icons)
/// resolve to it. Call once at startup.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "nerd".to_owned(),
        Arc::new(FontData::from_static(include_bytes!("../assets/NerdFont.ttf"))),
    );
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.entry(fam).or_default().push("nerd".to_owned());
    }
    ctx.set_fonts(fonts);
}

// Font Awesome (Nerd Font PUA) codepoints — all within this font's f000-f381 range.
pub const NEW: &str = "\u{f067}"; // plus
pub const SPARKLE: &str = "\u{f0eb}"; // lightbulb — new session
pub const REFRESH: &str = "\u{f021}"; // refresh
pub const CLOSE: &str = "\u{f00d}"; // times
pub const CHECK: &str = "\u{f00c}"; // check
pub const CROSS: &str = "\u{f00d}"; // times (error mark)
pub const ARROW_RIGHT: &str = "\u{f061}"; // arrow-right
pub const SEND: &str = "\u{f1d8}"; // paper-plane
pub const STOP: &str = "\u{f04d}"; // stop
pub const ATTACH: &str = "\u{f0c6}"; // paperclip
pub const DOT: &str = "\u{f111}"; // circle
pub const FOLDER: &str = "\u{f07b}"; // folder
pub const CLOCK: &str = "\u{f017}"; // clock
pub const WARN: &str = "\u{f071}"; // exclamation-triangle

// Session actions.
pub const RENAME: &str = "\u{f040}"; // pencil
pub const COMPACT: &str = "\u{f066}"; // compress
pub const DUPLICATE: &str = "\u{f0c5}"; // copy
pub const EXPORT: &str = "\u{f019}"; // download
pub const FORK: &str = "\u{f126}"; // code-fork

// Tree / disclosure.
pub const CHEVRON_RIGHT: &str = "\u{f054}"; // angle/chevron-right
pub const CHEVRON_DOWN: &str = "\u{f078}"; // chevron-down
pub const BULLET: &str = "\u{f111}"; // circle (small)
pub const HOME: &str = "\u{f015}"; // home
pub const FILE: &str = "\u{f15b}"; // file

// Plan-first checkbox.
pub const CHECK_BOX: &str = "\u{f14a}"; // check-square
pub const EMPTY_BOX: &str = "\u{f0c8}"; // square
