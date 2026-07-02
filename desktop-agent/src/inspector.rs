//! Inspector panel: real session stats, context gauge, tool activity, working
//! files, artifacts, statusline widget rows. Placeholders until data exists —
//! never stale demo content (ISC-101).

use eframe::egui::{self, Align, Color32, Layout, Vec2};

use crate::agent::AgentState;
use crate::emit::Emit;
use crate::jsonw::truncate_chars;
use crate::theme;

pub fn render(ui: &mut egui::Ui, state: Option<&AgentState>, emits: &mut Vec<Emit>) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.colored_label(theme::TEXT(), egui::RichText::new("Session").size(15.0).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(10.0);
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("✕").size(14.0).color(theme::TEXT_MUTED()))
                        .fill(theme::HOVER())
                        .corner_radius(6)
                        .min_size(Vec2::new(28.0, 28.0)),
                )
                .clicked()
            {
                emits.push(Emit::ToggleInspector);
            }
        });
    });
    ui.add_space(10.0);

    let Some(state) = state else {
        pad_label(ui, theme::TEXT_FAINT(), "no session yet");
        return;
    };

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        meta(ui, state);
        stats(ui, state);
        statusline(ui, state);
        activity(ui, state);
        files(ui, "Working files", &state.working_files, false, emits);
        files(ui, "Artifacts", &state.artifacts, true, emits);
    });
}

fn section(ui: &mut egui::Ui, label: &str) {
    ui.add_space(8.0);
    pad_label(ui, theme::TEXT_FAINT(), label);
    ui.add_space(4.0);
}

fn pad_label(ui: &mut egui::Ui, color: Color32, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.colored_label(color, egui::RichText::new(text).size(12.5).strong());
    });
}

fn kv(ui: &mut egui::Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new(key).size(12.5));
        ui.colored_label(theme::TEXT_MUTED(), egui::RichText::new(value).size(12.5));
    });
}

fn meta(ui: &mut egui::Ui, s: &AgentState) {
    section(ui, "Info");
    let name = if s.session_name.is_empty() { "(unnamed)" } else { &s.session_name };
    kv(ui, "name", name);
    if !s.session_id.is_empty() {
        kv(ui, "id", &truncate_chars(&s.session_id, 20));
    }
    if !s.session_file.is_empty() {
        let file = s.session_file.rsplit('/').next().unwrap_or("");
        kv(ui, "file", &truncate_chars(file, 30));
    }
    if !s.model.is_empty() {
        kv(ui, "model", &truncate_chars(&s.model, 28));
    }
    ui.add_space(6.0);
    ui.separator();
}

fn stats(ui: &mut egui::Ui, s: &AgentState) {
    section(ui, "Usage");
    if s.stats.input == 0 && s.stats.output == 0 {
        pad_label(ui, theme::TEXT_FAINT(), "no turns yet");
    } else {
        kv(ui, "tokens", &format!("{} in · {} out", s.stats.input, s.stats.output));
        if s.stats.cost > 0.0 {
            kv(ui, "cost", &format!("${:.4}", s.stats.cost));
        }
        // Context gauge (ISC-94).
        let pct = (s.stats.ctx_percent / 100.0).clamp(0.0, 1.0) as f32;
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width() - 24.0, 8.0), egui::Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(rect, 4.0, theme::CODE_BG());
            let mut fill = rect;
            fill.set_width(rect.width() * pct);
            let color = if pct > 0.85 { theme::RED() } else if pct > 0.6 { theme::YELLOW() } else { theme::GREEN() };
            painter.rect_filled(fill, 4.0, color);
        });
        kv(ui, "context", &format!("{:.0}%", s.stats.ctx_percent));
    }
    if !s.queued.is_empty() {
        kv(ui, "queued", &format!("{} message(s)", s.queued.len()));
    }
    ui.add_space(6.0);
    ui.separator();
}

fn statusline(ui: &mut egui::Ui, s: &AgentState) {
    if s.widget_lines.is_empty() && s.tool_status.is_empty() {
        return;
    }
    section(ui, "Status");
    for line in &s.widget_lines {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.colored_label(theme::TEXT_MUTED(), egui::RichText::new(line).monospace().size(11.5));
        });
    }
    for (name, st) in &s.tool_status {
        kv(ui, name, st);
    }
    ui.add_space(6.0);
    ui.separator();
}

fn activity(ui: &mut egui::Ui, s: &AgentState) {
    section(ui, "Tool activity");
    if s.activity.is_empty() {
        pad_label(ui, theme::TEXT_FAINT(), "none yet");
    }
    for a in s.activity.iter().rev().take(12) {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let (icon, color) = if a.is_error { ("✗", theme::RED()) } else { ("✓", theme::GREEN()) };
            ui.colored_label(color, egui::RichText::new(icon).size(12.5));
            ui.colored_label(theme::TEXT_MUTED(), egui::RichText::new(&a.name).size(12.5));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(10.0);
                ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new(format!("{}ms", a.ms)).size(11.5));
            });
        });
    }
    ui.add_space(6.0);
    ui.separator();
}

fn files(ui: &mut egui::Ui, label: &str, list: &[String], copy: bool, _emits: &mut [Emit]) {
    section(ui, label);
    if list.is_empty() {
        pad_label(ui, theme::TEXT_FAINT(), "none yet");
    }
    for f in list.iter().rev().take(12) {
        let display = tail_chars(f, 34);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let resp = ui.add(
                egui::Button::new(egui::RichText::new(display).monospace().size(11.5).color(theme::TEXT_MUTED()))
                    .fill(Color32::TRANSPARENT),
            );
            let resp = if copy { resp.on_hover_text("click to copy path") } else { resp };
            if copy && resp.clicked() {
                ui.ctx().copy_text(f.clone());
            }
        });
    }
    ui.add_space(4.0);
}

/// Keep the tail of a path, unicode-safe (ISC-113).
fn tail_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let skip = count - max;
    format!("…{}", s.chars().skip(skip).collect::<String>())
}
