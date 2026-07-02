//! Chat tab: Claude-Desktop-style conversation — greeting empty state, streamed
//! transcript, shared composer.

use eframe::egui::{self, Vec2};

use crate::composer;
use crate::conn::AgentConn;
use crate::emit::Emit;
use crate::theme;
use crate::transcript;

pub fn render(ui: &mut egui::Ui, conn: &mut AgentConn, username: &str, emits: &mut Vec<Emit>) {
    let avail = ui.available_size();
    let width = (avail.x * 0.82).clamp(360.0, 780.0);

    ui.vertical_centered(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(width, avail.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                transcript::banners(ui, &conn.state);

                if conn.state.items.is_empty() {
                    ui.add_space(avail.y * 0.28);
                    ui.vertical_centered(|ui| {
                        ui.colored_label(
                            theme::TEXT(),
                            egui::RichText::new(format!("Hi {username}")).size(30.0),
                        );
                        ui.add_space(6.0);
                        ui.colored_label(
                            theme::TEXT_MUTED(),
                            egui::RichText::new("What are we working on?").size(16.0),
                        );
                    });
                    ui.add_space(24.0);
                } else {
                    let scroll_h = avail.y - 130.0;
                    egui::ScrollArea::vertical()
                        .max_height(scroll_h)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            transcript::items(ui, &mut conn.state, width);
                        });
                    ui.add_space(10.0);
                }

                composer::render(ui, &conn.state, &mut conn.input, width, "Reply to the agent…", emits);
            },
        );
    });
}
