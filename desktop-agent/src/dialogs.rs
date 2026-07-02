//! Extension-UI dialog modals (select / confirm / input / editor) and toasts.
//! Dialogs are answered or cancelled — the agent must never hang (ISC-106).

use eframe::egui::{self, Align2, Color32, Vec2};

use crate::agent::AgentState;
use crate::emit::{DialogOut, Emit};
use crate::theme;

/// Render the front dialog (if any) as a centered modal. Sequential handling:
/// one at a time, in arrival order (ISC-110).
pub fn render(ctx: &egui::Context, state: &mut AgentState, emits: &mut Vec<Emit>) {
    let Some(d) = state.dialogs.first_mut() else { return };

    // Dim the background.
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("dialog-dim"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.painter()
                .rect_filled(screen, 0.0, Color32::from_black_alpha(140));
        });

    egui::Window::new("agent request")
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(egui::Frame {
            fill: theme::CARD(),
            stroke: egui::Stroke::new(1.0, theme::BORDER()),
            corner_radius: egui::CornerRadius::same(12),
            inner_margin: egui::Margin::same(18),
            ..egui::Frame::NONE
        })
        .show(ctx, |ui| {
            ui.set_min_width(360.0);
            ui.colored_label(theme::TEXT(), egui::RichText::new(&d.prompt).size(15.0));
            ui.add_space(12.0);

            match d.method.as_str() {
                "select" => {
                    for (i, opt) in d.options.iter().enumerate() {
                        let btn = egui::Button::new(egui::RichText::new(opt).size(14.0).color(theme::TEXT()))
                            .fill(if i == d.selected { theme::SELECTED() } else { theme::TOOL_BG() })
                            .corner_radius(8)
                            .min_size(Vec2::new(340.0, 34.0));
                        if ui.add(btn).clicked() {
                            emits.push(Emit::Dialog {
                                id: d.id.clone(),
                                out: DialogOut::Select(opt.clone()),
                            });
                        }
                        ui.add_space(4.0);
                    }
                    cancel_row(ui, &d.id, emits);
                }
                "confirm" => {
                    ui.horizontal(|ui| {
                        if primary(ui, "Confirm").clicked() {
                            emits.push(Emit::Dialog { id: d.id.clone(), out: DialogOut::Confirm(true) });
                        }
                        ui.add_space(8.0);
                        if secondary(ui, "Cancel").clicked() {
                            emits.push(Emit::Dialog { id: d.id.clone(), out: DialogOut::Confirm(false) });
                        }
                    });
                }
                // input | editor
                _ => {
                    let rows = if d.method == "editor" { 8 } else { 1 };
                    ui.add(
                        egui::TextEdit::multiline(&mut d.input)
                            .desired_rows(rows)
                            .desired_width(340.0),
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if primary(ui, "Send").clicked() {
                            emits.push(Emit::Dialog {
                                id: d.id.clone(),
                                out: DialogOut::Input(d.input.clone()),
                            });
                        }
                        ui.add_space(8.0);
                        if secondary(ui, "Cancel").clicked() {
                            emits.push(Emit::Dialog { id: d.id.clone(), out: DialogOut::Cancel });
                        }
                    });
                }
            }

            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                emits.push(Emit::Dialog { id: d.id.clone(), out: DialogOut::Cancel });
            }
        });
}

fn cancel_row(ui: &mut egui::Ui, id: &str, emits: &mut Vec<Emit>) {
    ui.add_space(6.0);
    if secondary(ui, "Cancel").clicked() {
        emits.push(Emit::Dialog { id: id.to_string(), out: DialogOut::Cancel });
    }
}

fn primary(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(14.0).color(Color32::WHITE))
            .fill(theme::ACCENT())
            .corner_radius(8)
            .min_size(Vec2::new(110.0, 34.0)),
    )
}

fn secondary(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(14.0).color(theme::TEXT_MUTED()))
            .fill(theme::TOOL_BG())
            .corner_radius(8)
            .min_size(Vec2::new(110.0, 34.0)),
    )
}
