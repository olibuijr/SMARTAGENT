//! Palette that follows the system theme (dark/light) at runtime. On Wayland
//! winit doesn't deliver theme events, so a watcher thread polls the XDG
//! desktop portal (`org.freedesktop.appearance color-scheme`: 1=dark, 2=light)
//! and repaints on change; `apply()` re-skins every view from the flag.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use eframe::egui::{self, Color32};

static DARK: AtomicBool = AtomicBool::new(true);
static WATCHER: OnceLock<()> = OnceLock::new();

/// Start the portal theme watcher (once). Sets the initial value synchronously
/// so the first frame already has the right palette.
pub fn start_watcher(ctx: egui::Context) {
    WATCHER.get_or_init(|| {
        if let Some(dark) = portal_prefers_dark() {
            DARK.store(dark, Ordering::Relaxed);
        }
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if let Some(dark) = portal_prefers_dark() {
                if DARK.swap(dark, Ordering::Relaxed) != dark {
                    ctx.request_repaint();
                }
            }
        });
    });
}

/// Ask the XDG desktop portal for the color-scheme preference.
/// Returns None if the portal is unavailable (then winit/default decides).
fn portal_prefers_dark() -> Option<bool> {
    let out = std::process::Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
            "--method",
            "org.freedesktop.portal.Settings.ReadOne",
            "org.freedesktop.appearance",
            "color-scheme",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // Shape: "(<uint32 2>,)" — 1 = prefer dark, 2 = prefer light, 0 = no pref.
    let digit = text.chars().rev().find(|c| c.is_ascii_digit())?;
    match digit {
        '1' => Some(true),
        '2' => Some(false),
        _ => Some(true), // no preference → keep dark default
    }
}

fn dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

macro_rules! color {
    ($name:ident, $dark:expr, $light:expr) => {
        #[allow(non_snake_case)]
        pub fn $name() -> Color32 {
            if dark() {
                $dark
            } else {
                $light
            }
        }
    };
}

// name, dark palette (Claude Desktop dark), light palette (Claude Desktop light)
color!(BG, Color32::from_rgb(22, 22, 28), Color32::from_rgb(245, 244, 239));
color!(SIDEBAR, Color32::from_rgb(26, 26, 32), Color32::from_rgb(238, 236, 230));
color!(PANEL, Color32::from_rgb(18, 18, 22), Color32::from_rgb(250, 249, 245));
color!(CARD, Color32::from_rgb(32, 32, 40), Color32::from_rgb(255, 255, 255));
color!(BORDER, Color32::from_rgb(48, 48, 56), Color32::from_rgb(214, 211, 202));
color!(HOVER, Color32::from_rgb(42, 42, 50), Color32::from_rgb(228, 226, 219));
color!(SELECTED, Color32::from_rgb(50, 50, 60), Color32::from_rgb(220, 217, 209));
color!(TEXT, Color32::from_rgb(228, 228, 238), Color32::from_rgb(41, 39, 34));
color!(TEXT_MUTED, Color32::from_rgb(140, 140, 155), Color32::from_rgb(110, 106, 98));
color!(TEXT_FAINT, Color32::from_rgb(100, 100, 115), Color32::from_rgb(152, 148, 138));
color!(ACCENT, Color32::from_rgb(232, 123, 66), Color32::from_rgb(217, 104, 51));
color!(ACCENT_DIM, Color32::from_rgb(180, 90, 45), Color32::from_rgb(233, 168, 133));
color!(USER_BUBBLE, Color32::from_rgb(50, 50, 62), Color32::from_rgb(233, 230, 222));
color!(TOOL_BG, Color32::from_rgb(28, 30, 38), Color32::from_rgb(242, 240, 234));
color!(GREEN, Color32::from_rgb(100, 200, 130), Color32::from_rgb(46, 140, 80));
color!(RED, Color32::from_rgb(220, 100, 100), Color32::from_rgb(190, 60, 60));
color!(YELLOW, Color32::from_rgb(220, 180, 100), Color32::from_rgb(176, 130, 40));
color!(CODE_BG, Color32::from_rgb(14, 14, 18), Color32::from_rgb(235, 233, 226));

/// Apply the palette for this frame, following the OS theme when it changes.
pub fn apply(ctx: &egui::Context) {
    // winit delivers the theme on some backends (X11/portal-integrated); when
    // it does, it wins. Otherwise the portal watcher owns the flag.
    if let Some(t) = ctx.input(|i| i.raw.system_theme) {
        DARK.store(t == egui::Theme::Dark, Ordering::Relaxed);
    }
    let system_dark = dark();

    let mut style = (*ctx.style()).clone();
    style.visuals = if system_dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    style.visuals.window_fill = BG();
    style.visuals.panel_fill = PANEL();
    style.visuals.faint_bg_color = SIDEBAR();
    style.visuals.extreme_bg_color = if system_dark {
        Color32::from_rgb(12, 12, 16)
    } else {
        Color32::from_rgb(255, 255, 255)
    };
    style.visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.hovered.bg_fill = HOVER();
    style.visuals.widgets.active.bg_fill = SELECTED();
    style.visuals.selection.bg_fill = ACCENT_DIM();
    style.visuals.hyperlink_color = ACCENT();
    style.visuals.window_shadow = egui::Shadow::NONE;
    style.visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.hovered.weak_bg_fill = HOVER();
    style.visuals.override_text_color = None;
    ctx.set_style(style);
}
