use eframe::egui::{self, Align, Layout, Vec2};
use crate::App;
use crate::theme;

pub fn render(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.colored_label(theme::TEXT, egui::RichText::new("Session").size(16.0).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(
                egui::Button::new(egui::RichText::new("✕").size(15.0).color(theme::TEXT_MUTED))
                    .fill(theme::HOVER).corner_radius(6).min_size(Vec2::new(30.0, 30.0)),
            ).clicked() {
                app.right_panel_open = false;
            }
            ui.add_space(12.0);
        });
    });
    ui.add_space(12.0);

    // --- Progress ---
    section(ui, "Progress");
    for step in &app.progress_steps {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            let (icon, color) = if step.done {
                ("✓", theme::GREEN)
            } else {
                ("○", theme::TEXT_FAINT)
            };
            ui.colored_label(color, egui::RichText::new(icon).size(15.0));
            ui.add_space(8.0);
            let fg = if step.done { theme::TEXT_MUTED } else { theme::TEXT_FAINT };
            ui.colored_label(fg, egui::RichText::new(&step.label).size(15.0));
        });
        ui.add_space(3.0);
    }
    ui.add_space(12.0);
    ui.separator();

    // --- Artifacts ---
    section(ui, "Artifacts");
    for art in &app.artifacts {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.colored_label(theme::TEXT_MUTED, egui::RichText::new("📄").size(15.0));
            ui.add_space(8.0);
            ui.colored_label(theme::TEXT, egui::RichText::new(art.as_str()).size(15.0));
        });
        ui.add_space(3.0);
    }
    ui.add_space(12.0);
    ui.separator();

    // --- Context ---
    section(ui, "Context");
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.colored_label(theme::TEXT_MUTED, egui::RichText::new("📁").size(15.0));
        ui.add_space(8.0);
        ui.colored_label(theme::TEXT, egui::RichText::new("SMARTAGENT/").size(15.0));
    });
    ui.add_space(12.0);
    ui.separator();

    // --- Connectors ---
    section(ui, "Connectors");
    let connectors: &[&str] = &["Web search", "File system"];
    for conn in connectors {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.colored_label(theme::GREEN, egui::RichText::new("●").size(13.0));
            ui.add_space(8.0);
            ui.colored_label(theme::TEXT, egui::RichText::new(*conn).size(15.0));
        });
        ui.add_space(3.0);
    }
    ui.add_space(12.0);
    ui.separator();

    // --- Working files ---
    section(ui, "Working files");
    for f in &app.working_files {
        let display = if f.len() > 32 {
            format!("…{}", &f[f.len().saturating_sub(30)..])
        } else {
            f.clone()
        };
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.colored_label(theme::TEXT, egui::RichText::new("▤").size(14.0));
            ui.add_space(8.0);
            ui.colored_label(theme::TEXT_MUTED, egui::RichText::new(&display).size(14.0));
        });
        ui.add_space(3.0);
    }
}

fn section(ui: &mut egui::Ui, label: &str) {
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.colored_label(theme::TEXT_FAINT, egui::RichText::new(label).size(13.0).strong());
    });
    ui.add_space(6.0);
}
