//! Cowork tab: autonomous-work surface. Task composer, live working banner,
//! progress from real tool activity, tasks-kanban strip, session artifacts.

use eframe::egui::{self, Vec2};

use crate::composer;
use crate::conn::AgentConn;
use crate::emit::Emit;
use crate::theme;
use crate::transcript;

pub fn render(
    ui: &mut egui::Ui,
    conn: &mut AgentConn,
    tasks_board: &[String],
    emits: &mut Vec<Emit>,
) {
    let avail = ui.available_size();
    let width = (avail.x * 0.86).clamp(360.0, 860.0);

    ui.vertical_centered(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(width, avail.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                transcript::banners(ui, &conn.state);
                working_banner(ui, conn);
                kanban_strip(ui, tasks_board);

                let scroll_h = avail.y - 240.0;
                egui::ScrollArea::vertical()
                    .max_height(scroll_h.max(120.0))
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if conn.state.items.is_empty() {
                            ui.add_space(30.0);
                            ui.vertical_centered(|ui| {
                                ui.colored_label(
                                    theme::TEXT(),
                                    egui::RichText::new("Describe a task to work on autonomously").size(20.0),
                                );
                                ui.add_space(4.0);
                                ui.colored_label(
                                    theme::TEXT_MUTED(),
                                    egui::RichText::new(
                                        "The agent tracks it on the kanban board and works it end-to-end.",
                                    )
                                    .size(14.0),
                                );
                            });
                        } else {
                            transcript::items(ui, &mut conn.state, width);
                        }
                    });

                ui.add_space(8.0);
                if let Some(e) =
                    composer::render(ui, &conn.state, &mut conn.input, width, "Describe the task…")
                {
                    emits.push(e);
                }
            },
        );
    });
}

/// Elapsed + current tool while a turn runs (ISC-81).
fn working_banner(ui: &mut egui::Ui, conn: &AgentConn) {
    if !conn.state.streaming {
        return;
    }
    let secs = conn
        .state
        .turn_started
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    let tool = conn
        .state
        .current_tool
        .as_deref()
        .map(|t| format!(" · running {t}"))
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(14.0));
        ui.colored_label(
            theme::TEXT_MUTED(),
            egui::RichText::new(format!("working… {}m{:02}s{tool}", secs / 60, secs % 60)).size(13.5),
        );
    });
    ui.add_space(6.0);
}

/// Real `tasks board` output as column chips + rows (ISC-77).
fn kanban_strip(ui: &mut egui::Ui, board: &[String]) {
    if board.is_empty() {
        return;
    }
    egui::Frame {
        fill: theme::TOOL_BG(),
        stroke: egui::Stroke::new(1.0, theme::BORDER()),
        corner_radius: egui::CornerRadius::same(10),
        inner_margin: egui::Margin::symmetric(14, 8),
        ..egui::Frame::NONE
    }
    .show(ui, |ui| {
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new("Board").size(12.5).strong());
        egui::ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
            for line in board {
                let is_col = line.ends_with(')') && line.contains('(');
                let (color, size) = if is_col {
                    (theme::ACCENT(), 13.0)
                } else {
                    (theme::TEXT_MUTED(), 12.5)
                };
                ui.colored_label(color, egui::RichText::new(line).monospace().size(size));
            }
        });
    });
    ui.add_space(8.0);
}
