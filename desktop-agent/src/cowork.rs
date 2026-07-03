//! Cowork tab — modelled on Anthropic's Cowork "Tasks" surface: describe a task,
//! the agent plans then runs it. Task-suggestion chips, a real Scheduled-tasks
//! section (from `schedule list`), a live working banner, the kanban board, and
//! session artifacts.

use eframe::egui::{self, Color32, Vec2};

use crate::composer;
use crate::conn::AgentConn;
use crate::emit::Emit;
use crate::icons;
use crate::theme;
use crate::transcript;

const SUGGESTIONS: &[&str] = &[
    "Organize files in a folder",
    "Crunch data from a spreadsheet",
    "Draft a document",
    "Research a topic and summarize",
    "Plan & schedule a task",
];

#[allow(clippy::too_many_arguments)]
pub fn render(
    ui: &mut egui::Ui,
    conn: &mut AgentConn,
    tasks_board: &[String],
    scheduled: &[String],
    board_input: &mut String,
    plan_first: bool,
    folder_input: &mut String,
    emits: &mut Vec<Emit>,
) {
    let avail = ui.available_size();
    let width = (avail.x * 0.86).clamp(360.0, 860.0);

    ui.vertical_centered(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(width, avail.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                header(ui, plan_first, folder_input, emits);
                transcript::banners(ui, &conn.state);
                working_banner(ui, conn);

                let scroll_h = avail.y - 230.0;
                egui::ScrollArea::vertical()
                    .max_height(scroll_h.max(120.0))
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if conn.state.items.is_empty() {
                            empty_state(ui, conn, scheduled, emits);
                        } else {
                            transcript::items(ui, &mut conn.state, width);
                        }
                    });

                ui.add_space(6.0);
                crate::board::render(ui, tasks_board, board_input, emits);
                ui.add_space(8.0);
                composer::render(ui, &conn.state, &mut conn.input, width, "Describe the task…", emits);
            },
        );
    });
}

fn header(ui: &mut egui::Ui, plan_first: bool, folder_input: &mut String, emits: &mut Vec<Emit>) {
    ui.horizontal(|ui| {
        ui.colored_label(theme::TEXT(), egui::RichText::new("Tasks").size(16.0).strong());

        // Plan-before-act toggle (real Cowork's defining safety flow).
        let plan_label = if plan_first {
            format!("{} Plan first", icons::CHECK_BOX)
        } else {
            format!("{} Plan first", icons::EMPTY_BOX)
        };
        if ui
            .add(egui::Button::new(egui::RichText::new(plan_label).size(12.5).color(if plan_first { theme::ACCENT() } else { theme::TEXT_MUTED() })).fill(Color32::TRANSPARENT))
            .on_hover_text("Ask for a plan and approval before the agent acts")
            .clicked()
        {
            emits.push(Emit::TogglePlanFirst);
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("+ New task").size(13.0).color(theme::TEXT()))
                        .fill(theme::CARD())
                        .corner_radius(8)
                        .min_size(Vec2::new(96.0, 30.0)),
                )
                .clicked()
            {
                emits.push(Emit::NewSession);
            }
        });
    });
    // Work-in-a-folder: point the agent at any directory.
    ui.horizontal(|ui| {
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new(icons::FOLDER).size(13.0));
        let resp = ui.add(egui::TextEdit::singleline(folder_input).desired_width(280.0).hint_text("work in folder (path)…"));
        let go = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if go && !folder_input.trim().is_empty() {
            let p = std::path::PathBuf::from(folder_input.trim());
            if p.is_dir() {
                emits.push(Emit::WorkInFolder(p));
                folder_input.clear();
            }
        }
    });
    ui.add_space(6.0);
}

fn empty_state(ui: &mut egui::Ui, conn: &mut AgentConn, scheduled: &[String], emits: &mut Vec<Emit>) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.colored_label(theme::TEXT(), egui::RichText::new("What should I work on?").size(22.0));
        ui.add_space(4.0);
        ui.colored_label(
            theme::TEXT_MUTED(),
            egui::RichText::new("Describe a task — I'll plan it, then run it end-to-end.").size(14.0),
        );
    });
    ui.add_space(16.0);

    // Suggestion chips (ISC-142) — clicking seeds the composer.
    ui.horizontal_wrapped(|ui| {
        for s in SUGGESTIONS {
            if ui
                .add(
                    egui::Button::new(egui::RichText::new(*s).size(13.0).color(theme::TEXT()))
                        .fill(theme::TOOL_BG())
                        .stroke(egui::Stroke::new(1.0, theme::BORDER()))
                        .corner_radius(16)
                        .min_size(Vec2::new(0.0, 32.0)),
                )
                .clicked()
            {
                conn.input = s.to_string();
            }
        }
    });

    // Scheduled tasks (ISC-143).
    ui.add_space(20.0);
    scheduled_section(ui, scheduled, emits);
}

fn scheduled_section(ui: &mut egui::Ui, scheduled: &[String], emits: &mut Vec<Emit>) {
    ui.horizontal(|ui| {
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new(format!("{} Scheduled", icons::CLOCK)).size(13.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(egui::RichText::new("/schedule").size(12.0).color(theme::ACCENT())).fill(Color32::TRANSPARENT))
                .on_hover_text("Insert the schedule command")
                .clicked()
            {
                emits.push(Emit::Send("/status".to_string()));
            }
        });
    });
    ui.add_space(4.0);
    if scheduled.is_empty() {
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new("no scheduled tasks").size(12.5).italics());
        return;
    }
    for job in scheduled {
        egui::Frame {
            fill: theme::TOOL_BG(),
            corner_radius: egui::CornerRadius::same(8),
            inner_margin: egui::Margin::symmetric(12, 6),
            ..egui::Frame::NONE
        }
        .show(ui, |ui| {
            ui.colored_label(theme::TEXT_MUTED(), egui::RichText::new(job).monospace().size(12.0));
        });
        ui.add_space(3.0);
    }
}

/// Elapsed + current tool while a turn runs (ISC-81).
fn working_banner(ui: &mut egui::Ui, conn: &AgentConn) {
    if !conn.state.streaming {
        return;
    }
    let secs = conn.state.turn_started.map(|t| t.elapsed().as_secs()).unwrap_or(0);
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

