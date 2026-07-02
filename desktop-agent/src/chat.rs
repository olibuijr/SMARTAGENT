use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Layout, Margin, Stroke, Vec2};
use crate::{App, Message, MessageRole};
use crate::theme;

pub fn render(app: &mut App, ui: &mut egui::Ui) {
    let available = ui.available_size();
    let chat_w = (available.x * 0.75).min(780.0).max(460.0);
    let chat_h = available.y;

    ui.allocate_ui_with_layout(
        Vec2::new(available.x, chat_h),
        Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            // Empty greeting
            if app.messages.is_empty() {
                ui.add_space(chat_h * 0.22);
                ui.horizontal(|ui| {
                    let (resp, painter) = ui.allocate_painter(Vec2::splat(42.0), egui::Sense::hover());
                    painter.circle_filled(resp.rect.center(), 16.0, theme::ACCENT);
                    ui.add_space(6.0);
                    ui.colored_label(theme::TEXT, egui::RichText::new("Good afternoon, Ólafur").size(36.0));
                });
                ui.add_space(28.0);
            }

            let msg_area_h = if app.messages.is_empty() { 260.0 } else { chat_h - 130.0 };
            egui::ScrollArea::vertical().max_height(msg_area_h).show(ui, |ui| {
                ui.set_max_width(chat_w);
                for msg in app.messages.clone() {
                    match msg.role {
                        MessageRole::User => user_bubble(ui, &msg.text),
                        MessageRole::Claude => claude_message(ui, &msg),
                    }
                    ui.add_space(16.0);
                }
            });

            ui.add_space(12.0);
            composer(app, ui, chat_w);
        },
    );
}

fn user_bubble(ui: &mut egui::Ui, text: &str) {
    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
        let max_w = ui.available_width() * 0.75;
        Frame {
            fill: theme::USER_BUBBLE,
            corner_radius: CornerRadius::same(14),
            inner_margin: Margin::symmetric(18, 14),
            ..Frame::NONE
        }
        .show(ui, |ui| {
            ui.set_max_width(max_w);
            ui.colored_label(theme::TEXT, egui::RichText::new(text).size(16.0));
        });
    });
    ui.add_space(6.0);
}

fn claude_message(ui: &mut egui::Ui, msg: &Message) {
    ui.colored_label(theme::TEXT, egui::RichText::new(&msg.text).size(16.0).line_height(Some(1.5)));
    ui.add_space(8.0);

    for card in &msg.tool_cards {
        Frame {
            fill: theme::TOOL_BG,
            stroke: Stroke::new(1.0, theme::BORDER),
            corner_radius: CornerRadius::same(10),
            inner_margin: Margin::symmetric(16, 10),
            ..Frame::NONE
        }
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let icon_color = match card.label.as_str() {
                    "Read" => Color32::from_rgb(120, 160, 220),
                    "Edit" => theme::GREEN,
                    "Run" => Color32::from_rgb(220, 180, 100),
                    _ => theme::TEXT_MUTED,
                };
                ui.colored_label(icon_color, egui::RichText::new(&card.label).size(14.0).strong());
                ui.add_space(10.0);
                ui.colored_label(theme::TEXT_FAINT, egui::RichText::new(&card.detail).size(14.0));
            });
        });
        ui.add_space(6.0);
    }
}

fn composer(app: &mut App, ui: &mut egui::Ui, width: f32) {
    Frame {
        fill: theme::CARD,
        stroke: Stroke::new(1.0, theme::BORDER),
        corner_radius: CornerRadius::same(14),
        inner_margin: Margin::symmetric(18, 14),
        ..Frame::NONE
    }
    .show(ui, |ui| {
        ui.set_max_width(width);
        ui.horizontal(|ui| {
            if ui.add(
                egui::Button::new(egui::RichText::new("+").size(20.0).color(theme::TEXT_MUTED))
                    .fill(Color32::TRANSPARENT).min_size(Vec2::new(36.0, 36.0)),
            ).on_hover_text("Attach files").clicked() {}
            ui.add_space(6.0);
            if ui.add(
                egui::Button::new(egui::RichText::new("⚙").size(18.0).color(theme::TEXT_MUTED))
                    .fill(Color32::TRANSPARENT).min_size(Vec2::new(36.0, 36.0)),
            ).on_hover_text("Tools").clicked() {}

            ui.add_space(12.0);
            let edit = egui::TextEdit::multiline(&mut app.chat_input)
                .hint_text("Reply to Claude…")
                .font(egui::TextStyle::Body)
                .desired_width(ui.available_width() - 200.0)
                .desired_rows(1);
            let resp = ui.add(edit);

            ui.add_space(12.0);
            ui.colored_label(theme::TEXT_MUTED, egui::RichText::new("Sonnet 4").size(15.0));

            ui.add_space(10.0);
            let send_btn = egui::Button::new(egui::RichText::new("↑").size(22.0).color(Color32::WHITE))
                .fill(theme::ACCENT).corner_radius(10).min_size(Vec2::new(40.0, 40.0));

            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let clicked = ui.add(send_btn).clicked();
            if (clicked || (resp.lost_focus() && enter)) && !app.chat_input.trim().is_empty() {
                app.messages.push(Message {
                    role: MessageRole::User,
                    text: app.chat_input.trim().into(),
                    tool_cards: vec![],
                });
                app.messages.push(Message {
                    role: MessageRole::Claude,
                    text: "I'd be happy to help with that! Let me look into it…".into(),
                    tool_cards: vec![],
                });
                app.chat_input.clear();
            }
        });

        ui.add_space(6.0);
        ui.colored_label(
            theme::TEXT_FAINT,
            egui::RichText::new("Claude can make mistakes. Please double-check responses.").size(13.0),
        );
    });
}
