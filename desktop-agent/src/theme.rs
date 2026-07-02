use eframe::egui::{self, Color32};

// Claude Desktop dark-mode palette
pub const BG: Color32 = Color32::from_rgb(22, 22, 28);
pub const SIDEBAR: Color32 = Color32::from_rgb(26, 26, 32);
pub const PANEL: Color32 = Color32::from_rgb(18, 18, 22);
pub const CARD: Color32 = Color32::from_rgb(32, 32, 40);
pub const BORDER: Color32 = Color32::from_rgb(48, 48, 56);
pub const HOVER: Color32 = Color32::from_rgb(42, 42, 50);
pub const SELECTED: Color32 = Color32::from_rgb(50, 50, 60);
pub const TEXT: Color32 = Color32::from_rgb(228, 228, 238);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(140, 140, 155);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(100, 100, 115);
pub const ACCENT: Color32 = Color32::from_rgb(232, 123, 66);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(180, 90, 45);
pub const USER_BUBBLE: Color32 = Color32::from_rgb(50, 50, 62);
pub const TOOL_BG: Color32 = Color32::from_rgb(28, 30, 38);
pub const GREEN: Color32 = Color32::from_rgb(100, 200, 130);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.window_fill = BG;
    style.visuals.panel_fill = PANEL;
    style.visuals.faint_bg_color = SIDEBAR;
    style.visuals.extreme_bg_color = Color32::from_rgb(12, 12, 16);
    style.visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.hovered.bg_fill = HOVER;
    style.visuals.widgets.active.bg_fill = SELECTED;
    style.visuals.selection.bg_fill = ACCENT_DIM;
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.window_shadow = egui::Shadow::NONE;
    style.visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.hovered.weak_bg_fill = HOVER;
    ctx.set_style(style);
}
