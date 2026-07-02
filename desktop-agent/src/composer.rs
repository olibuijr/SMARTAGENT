//! Shared composer: multi-line input, Enter-to-send, Shift+Enter newline,
//! Stop while streaming. Emits Send/Abort; never touches the RPC client.

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, Stroke, Vec2};

use crate::agent::AgentState;
use crate::emit::Emit;
use crate::theme;

/// Render the composer. `hint` is the placeholder; returns an Emit if the user
/// acted this frame.
pub fn render(
    ui: &mut egui::Ui,
    state: &AgentState,
    input: &mut String,
    width: f32,
    hint: &str,
) -> Option<Emit> {
    let mut emit = None;

    // Queued steering / follow-up messages (ISC-37).
    if !state.queued.is_empty() {
        for q in &state.queued {
            ui.horizontal(|ui| {
                ui.colored_label(theme::TEXT_FAINT(), "⏳");
                ui.colored_label(theme::TEXT_MUTED(), egui::RichText::new(q).size(13.0));
            });
        }
        ui.add_space(4.0);
    }

    Frame {
        fill: theme::CARD(),
        stroke: Stroke::new(1.0, theme::BORDER()),
        corner_radius: CornerRadius::same(14),
        inner_margin: Margin::symmetric(16, 12),
        ..Frame::NONE
    }
    .show(ui, |ui| {
        ui.set_max_width(width);
        ui.horizontal(|ui| {
            let editor = egui::TextEdit::multiline(input)
                .hint_text(hint)
                .font(egui::TextStyle::Body)
                .frame(false)
                .desired_width(ui.available_width() - 120.0)
                .desired_rows(1);
            let resp = ui.add(editor);

            // Multiline TextEdit consumes Enter as a newline and never drops
            // focus — detect submit while focused instead; the stray trailing
            // newline is trimmed on send (ISC-61).
            let submit = resp.has_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if state.streaming {
                    let stop = egui::Button::new(egui::RichText::new("◼").size(18.0).color(Color32::WHITE))
                        .fill(theme::RED())
                        .corner_radius(10)
                        .min_size(Vec2::new(40.0, 40.0));
                    if ui.add(stop).on_hover_text("Stop").clicked() {
                        emit = Some(Emit::Abort);
                    }
                } else {
                    let send = egui::Button::new(egui::RichText::new("↑").size(22.0).color(Color32::WHITE))
                        .fill(theme::ACCENT())
                        .corner_radius(10)
                        .min_size(Vec2::new(40.0, 40.0));
                    if ui.add(send).clicked() && !input.trim().is_empty() {
                        emit = Some(Emit::Send(std::mem::take(input)));
                    }
                }
            });

            if submit && !input.trim().is_empty() {
                let text = input.trim().to_string();
                input.clear();
                emit = Some(Emit::Send(text));
            }
        });

        ui.add_space(4.0);
        let footer = if !state.connected {
            "Agent not connected — sending will start it.".to_string()
        } else {
            let model = if state.model.is_empty() { "model".to_string() } else { state.model.clone() };
            let tl = if state.thinking_level.is_empty() { String::new() } else { format!(" · {}", state.thinking_level) };
            format!("{model}{tl}")
        };
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new(footer).size(12.5));
    });

    emit
}
