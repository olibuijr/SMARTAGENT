use eframe::egui::{self, Align, Align2, Color32, Layout, Vec2};
use crate::{App, Tab};
use crate::theme;

pub fn render(app: &mut App, ui: &mut egui::Ui) {
    let w = ui.available_width();

    // Logo + product name
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.add_space(18.0);
        let (resp, painter) = ui.allocate_painter(Vec2::splat(24.0), egui::Sense::hover());
        painter.circle_filled(resp.rect.center(), 10.0, theme::ACCENT);
        ui.add_space(6.0);
        ui.colored_label(theme::TEXT, egui::RichText::new("Claude").size(17.0).strong());
    });
    ui.add_space(14.0);

    // Tab pills
    ui.horizontal(|ui| {
        ui.add_space(18.0);
        for tab in [Tab::Chat, Tab::Code, Tab::Cowork] {
            let label = match tab { Tab::Chat => "Chat", Tab::Code => "Code", Tab::Cowork => "Cowork" };
            let sel = app.active_tab == tab;
            let bg = if sel { theme::SELECTED } else { Color32::TRANSPARENT };
            let fg = if sel { theme::TEXT } else { theme::TEXT_MUTED };
            let resp = ui.add(
                egui::Button::new(egui::RichText::new(label).size(15.0).color(fg))
                    .fill(bg).corner_radius(8).min_size(Vec2::new(68.0, 34.0)),
            );
            if resp.clicked() { app.active_tab = tab; }
            ui.add_space(6.0);
        }
    });
    ui.add_space(16.0);
    ui.separator();

    // New session
    row(ui, w, "✦  New session", theme::TEXT, false);
    ui.add_space(6.0);

    // Nav items
    row(ui, w, "⏱  Routines", theme::TEXT_MUTED, false);
    row(ui, w, "⚙  Customize", theme::TEXT_MUTED, false);
    row(ui, w, "…  More", theme::TEXT_MUTED, false);
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(8.0);

    // Session list
    let list_h = ui.available_height() - 68.0;
    egui::ScrollArea::vertical().max_height(list_h).show(ui, |ui| {
        section(ui, "Pinned");
        for (i, s) in app.sessions.iter().enumerate() {
            if !s.pinned { continue; }
            let sel = i == app.selected_session;
            let resp = row(ui, w, &s.title, if sel { theme::TEXT } else { theme::TEXT_MUTED }, sel);
            if resp.clicked() { app.selected_session = i; }
        }
        ui.add_space(6.0);
        section(ui, "Scheduled");
        for (i, s) in app.sessions.iter().enumerate() {
            if !s.scheduled { continue; }
            let sel = i == app.selected_session;
            row(ui, w, &s.title, if sel { theme::TEXT } else { theme::TEXT_MUTED }, sel);
        }
        ui.add_space(6.0);
        section(ui, "Recents");
        for (i, s) in app.sessions.iter().enumerate() {
            if s.pinned || s.scheduled { continue; }
            let sel = i == app.selected_session;
            let resp = row(ui, w, &s.title, if sel { theme::TEXT } else { theme::TEXT_MUTED }, sel);
            if resp.clicked() { app.selected_session = i; }
        }
    });

    // Account area
    ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            let (resp, painter) = ui.allocate_painter(Vec2::splat(36.0), egui::Sense::hover());
            painter.circle_filled(resp.rect.center(), 15.0, Color32::from_rgb(100, 130, 200));
            painter.text(resp.rect.center(), Align2::CENTER_CENTER, "O", egui::FontId::proportional(16.0), theme::TEXT);
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.colored_label(theme::TEXT, egui::RichText::new("Ólafur").size(15.0));
                ui.colored_label(theme::TEXT_MUTED, egui::RichText::new("Max").size(13.0));
            });
        });
        ui.add_space(10.0);
    });
}

fn section(ui: &mut egui::Ui, label: &str) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(18.0);
        ui.colored_label(theme::TEXT_FAINT, egui::RichText::new(label).size(13.0).strong());
    });
    ui.add_space(4.0);
}

fn row(ui: &mut egui::Ui, width: f32, label: &str, color: Color32, selected: bool) -> egui::Response {
    let bg = if selected { theme::SELECTED } else { Color32::TRANSPARENT };
    let trunc = if label.len() > 36 { format!("{}…", &label[..33]) } else { label.to_string() };
    let r = ui.allocate_ui_with_layout(
        Vec2::new(width, 44.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add_space(18.0);
            ui.add(
                egui::Button::new(egui::RichText::new(&trunc).size(15.0).color(color))
                    .fill(bg).corner_radius(6).min_size(Vec2::new(width - 24.0, 38.0)),
            )
        },
    );
    r.inner
}
