//! Sidebar: tab pills, new session, real session list from .pi/sessions,
//! connection status, model footer. No demo rows (ISC-53).

use eframe::egui::{self, Align, Color32, Layout, Vec2};

use crate::emit::Emit;
use crate::icons;
use crate::jsonw::truncate_chars;
use crate::root;
use crate::theme;
use crate::{App, Tab};

pub fn render(ui: &mut egui::Ui, app: &App, emits: &mut Vec<Emit>) {
    let w = ui.available_width();

    logo(ui, app);
    tabs(ui, app, emits);
    ui.add_space(12.0);
    ui.separator();

    if row(ui, w, &format!("{}  New session", icons::NEW), theme::TEXT(), false).clicked() {
        emits.push(Emit::NewSession);
    }
    ui.add_space(6.0);
    ui.separator();

    // Tools launcher.
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new("Tools").size(12.5).strong());
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.vertical(|ui| crate::panels::launcher(ui, w, app.active_panel, emits));
    });
    ui.add_space(6.0);
    ui.separator();

    session_list(ui, app, w, emits);
    footer(ui, app);
}

fn logo(ui: &mut egui::Ui, app: &App) {
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        let (resp, painter) = ui.allocate_painter(Vec2::splat(22.0), egui::Sense::hover());
        painter.circle_filled(resp.rect.center(), 9.0, theme::ACCENT());
        ui.add_space(6.0);
        ui.colored_label(theme::TEXT(), egui::RichText::new("SMARTAGENT").size(16.0).strong());
        // Connection dot (ISC-58).
        let connected = app.active_conn().map(|c| c.state.connected).unwrap_or(false);
        let color = if connected { theme::GREEN() } else { theme::RED() };
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(12.0);
            ui.colored_label(color, egui::RichText::new("●").size(11.0));
        });
    });
    ui.add_space(12.0);
}

fn tabs(ui: &mut egui::Ui, app: &App, emits: &mut Vec<Emit>) {
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        for tab in [Tab::Chat, Tab::Cowork, Tab::Code] {
            let label = match tab {
                Tab::Chat => "Chat",
                Tab::Cowork => "Cowork",
                Tab::Code => "Code",
            };
            let sel = app.active_tab == tab;
            let bg = if sel { theme::SELECTED() } else { Color32::TRANSPARENT };
            let fg = if sel { theme::TEXT() } else { theme::TEXT_MUTED() };
            let resp = ui.add(
                egui::Button::new(egui::RichText::new(label).size(14.0).color(fg))
                    .fill(bg)
                    .corner_radius(8)
                    .min_size(Vec2::new(70.0, 32.0)),
            );
            if resp.clicked() {
                emits.push(Emit::SetTab(tab));
            }
            ui.add_space(4.0);
        }
    });
}

fn session_list(ui: &mut egui::Ui, app: &App, w: f32, emits: &mut Vec<Emit>) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new("Recents").size(12.5).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(10.0);
            if ui
                .add(egui::Button::new(egui::RichText::new(icons::REFRESH).size(12.5).color(theme::TEXT_FAINT())).fill(Color32::TRANSPARENT))
                .on_hover_text("Refresh sessions")
                .clicked()
            {
                emits.push(Emit::RefreshSessions);
            }
        });
    });
    ui.add_space(4.0);

    let active_file = app.active_conn().map(|c| c.state.session_file.clone()).unwrap_or_default();
    let now = root::now_secs();
    let list_h = ui.available_height() - 70.0;

    egui::ScrollArea::vertical().max_height(list_h).show(ui, |ui| {
        if app.sessions.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new("no sessions yet").size(13.0).italics());
            });
        }
        for s in &app.sessions {
            let sel = !active_file.is_empty() && s.path.to_string_lossy() == active_file;
            let mut title = truncate_chars(&s.title, 34);
            if let Some(p) = &s.project {
                title = format!("▸{p} · {title}");
                title = truncate_chars(&title, 36);
            }
            let color = if sel { theme::TEXT() } else { theme::TEXT_MUTED() };
            ui.horizontal(|ui| {
                let resp = row2(ui, w - 30.0, &title, &crate::sessions::relative_time(now, s.mtime_secs), color, sel);
                if resp.clicked() {
                    emits.push(Emit::SwitchSession(s.path.clone()));
                }
                if !sel
                    && ui
                        .add(egui::Button::new(egui::RichText::new("✕").size(11.0).color(theme::TEXT_FAINT())).fill(Color32::TRANSPARENT))
                        .on_hover_text("Delete session")
                        .clicked()
                {
                    emits.push(Emit::DeleteSession(s.path.clone()));
                }
            });
        }
    });
}

fn footer(ui: &mut egui::Ui, app: &App) {
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            let user = root::username();
            let model = app
                .active_conn()
                .map(|c| c.state.model.clone())
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| "—".to_string());
            ui.vertical(|ui| {
                ui.colored_label(theme::TEXT(), egui::RichText::new(user).size(13.5));
                ui.colored_label(theme::TEXT_MUTED(), egui::RichText::new(truncate_chars(&model, 30)).size(12.0));
            });
        });
        ui.add_space(6.0);
        ui.separator();
    });
}

fn row(ui: &mut egui::Ui, width: f32, label: &str, color: Color32, selected: bool) -> egui::Response {
    let bg = if selected { theme::SELECTED() } else { Color32::TRANSPARENT };
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.add(
            egui::Button::new(egui::RichText::new(label).size(14.0).color(color))
                .fill(bg)
                .corner_radius(6)
                .min_size(Vec2::new(width - 24.0, 36.0)),
        )
    })
    .inner
}

fn row2(
    ui: &mut egui::Ui,
    width: f32,
    title: &str,
    time: &str,
    color: Color32,
    selected: bool,
) -> egui::Response {
    let desired = Vec2::new(width, 44.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let bg = if selected {
            theme::SELECTED()
        } else if response.hovered() {
            theme::HOVER()
        } else {
            Color32::TRANSPARENT
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(6), bg);

        let galley = ui.painter().layout(
            format!("{title}\n{time}"),
            egui::FontId::proportional(13.0),
            color,
            rect.width() - 20.0,
        );
        let text_pos = egui::pos2(rect.left() + 12.0, rect.top() + 5.0);
        ui.painter().galley(text_pos, galley, Color32::TRANSPARENT);
    }

    response
}
