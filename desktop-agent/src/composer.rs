//! Shared composer: multi-line input, Enter-to-send, Shift+Enter newline,
//! Stop while streaming, a `/` slash-command palette, and model/thinking
//! pickers — all driven by the agent's real capability lists.

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, Stroke, Vec2};

use crate::agent::AgentState;
use crate::emit::Emit;
use crate::theme;

const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh"];
/// Commands that take arguments — fill the input instead of firing immediately.
const NEEDS_ARGS: &[&str] = &["skills", "index", "memory", "board"];

/// Render the composer. `hint` is the placeholder; pushes intents into `emits`.
pub fn render(
    ui: &mut egui::Ui,
    state: &AgentState,
    input: &mut String,
    width: f32,
    hint: &str,
    emits: &mut Vec<Emit>,
) {
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

    // Slash-command palette (ISC-135/136): shows while the input is a bare `/query`.
    slash_palette(ui, state, input, width, emits);

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
            // Attach: pi resolves @path file mentions in the prompt.
            if ui
                .add(egui::Button::new(egui::RichText::new("📎").size(15.0).color(theme::TEXT_MUTED())).fill(Color32::TRANSPARENT).min_size(Vec2::new(28.0, 28.0)))
                .on_hover_text("Reference a file — type a path after @")
                .clicked()
            {
                if !input.is_empty() && !input.ends_with(' ') {
                    input.push(' ');
                }
                input.push('@');
            }
            let editor = egui::TextEdit::multiline(input)
                .hint_text(hint)
                .font(egui::TextStyle::Body)
                .frame(false)
                .desired_width(ui.available_width() - 120.0)
                .desired_rows(1);
            let resp = ui.add(editor);

            // Multiline TextEdit keeps focus and swallows Enter — detect submit
            // while focused instead (ISC-61).
            let submit = resp.has_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if state.streaming {
                    let stop = egui::Button::new(egui::RichText::new("Stop").size(14.0).color(Color32::WHITE))
                        .fill(theme::RED())
                        .corner_radius(10)
                        .min_size(Vec2::new(56.0, 36.0));
                    if ui.add(stop).clicked() {
                        emits.push(Emit::Abort);
                    }
                } else {
                    let send = egui::Button::new(egui::RichText::new("Send").size(14.0).color(Color32::WHITE))
                        .fill(theme::ACCENT())
                        .corner_radius(10)
                        .min_size(Vec2::new(56.0, 36.0));
                    if ui.add(send).clicked() && !input.trim().is_empty() {
                        emits.push(Emit::Send(std::mem::take(input)));
                    }
                }
            });

            if submit && !input.trim().is_empty() {
                let text = input.trim().to_string();
                input.clear();
                emits.push(Emit::Send(text));
            }
        });

        ui.add_space(6.0);
        footer_pickers(ui, state, emits);
    });
}

/// Model + thinking-level pickers, sourced from the agent's real lists.
fn footer_pickers(ui: &mut egui::Ui, state: &AgentState, emits: &mut Vec<Emit>) {
    ui.horizontal(|ui| {
        if !state.connected {
            ui.colored_label(
                theme::TEXT_FAINT(),
                egui::RichText::new("Agent not connected — sending will start it.").size(12.5),
            );
            return;
        }

        // Model picker (ISC-139/148).
        let model_label = if state.model.is_empty() { "model".to_string() } else { state.model.clone() };
        ui.menu_button(egui::RichText::new(model_label).size(12.5).color(theme::TEXT_MUTED()), |ui| {
            ui.set_max_height(320.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                if state.available_models.is_empty() {
                    ui.label(egui::RichText::new("loading models…").size(12.5));
                }
                for (provider, id, name) in &state.available_models {
                    let current = state.model.contains(id.as_str());
                    let label = if current { format!("● {name}") } else { format!("   {name}") };
                    if ui.button(egui::RichText::new(label).size(12.5)).clicked() {
                        emits.push(Emit::SetModel { provider: provider.clone(), id: id.clone() });
                        ui.close_menu();
                    }
                }
            });
        });

        ui.colored_label(theme::TEXT_FAINT(), "·");

        // Thinking picker (ISC-140).
        let tl = if state.thinking_level.is_empty() { "thinking".to_string() } else { state.thinking_level.clone() };
        ui.menu_button(egui::RichText::new(tl).size(12.5).color(theme::TEXT_MUTED()), |ui| {
            for level in THINKING_LEVELS {
                let current = state.thinking_level == *level;
                let label = if current { format!("● {level}") } else { format!("   {level}") };
                if ui.button(egui::RichText::new(label).size(12.5)).clicked() {
                    emits.push(Emit::SetThinking(level.to_string()));
                    ui.close_menu();
                }
            }
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new("/ for commands").size(12.0));
        });
    });
}

fn slash_palette(
    ui: &mut egui::Ui,
    state: &AgentState,
    input: &mut String,
    width: f32,
    emits: &mut Vec<Emit>,
) {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') || trimmed.contains(char::is_whitespace) || state.commands.is_empty() {
        return;
    }
    let query = &trimmed[1..];
    let matches: Vec<&(String, String)> =
        state.commands.iter().filter(|(n, _)| n.starts_with(query)).collect();
    if matches.is_empty() {
        return;
    }

    Frame {
        fill: theme::CARD(),
        stroke: Stroke::new(1.0, theme::BORDER()),
        corner_radius: CornerRadius::same(12),
        inner_margin: Margin::symmetric(8, 8),
        ..Frame::NONE
    }
    .show(ui, |ui| {
        ui.set_max_width(width);
        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            for (name, desc) in matches {
                let resp = ui.add(
                    egui::Button::new(egui::RichText::new(format!("/{name}")).size(13.5).color(theme::ACCENT()))
                        .fill(Color32::TRANSPARENT)
                        .min_size(Vec2::new(width - 40.0, 26.0)),
                );
                ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new(desc).size(12.0));
                ui.add_space(2.0);
                if resp.clicked() {
                    if NEEDS_ARGS.contains(&name.as_str()) {
                        *input = format!("/{name} ");
                    } else {
                        emits.push(Emit::Send(format!("/{name}")));
                        input.clear();
                    }
                }
            }
        });
    });
    ui.add_space(6.0);
}
