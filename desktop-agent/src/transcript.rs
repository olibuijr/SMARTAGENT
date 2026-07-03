//! Shared transcript renderer used by every tab, so streaming fidelity is
//! identical across Chat / Cowork / Code (ISC-80). Renders bubbles, assistant
//! text with code fences, collapsible thinking, and live tool cards.

use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Layout, Margin, Stroke};

use crate::agent::{AgentState, Assistant, Item, ToolCard};
use crate::icons;
use crate::theme;

/// Transient banners (error / retry / compaction notice) above the transcript.
pub fn banners(ui: &mut egui::Ui, state: &AgentState) {
    if let Some(err) = &state.error_banner {
        card(ui, theme::RED(), icons::WARN, err);
    }
    if let Some(notice) = &state.notice {
        card(ui, theme::YELLOW(), crate::icons::DOT, notice);
    }
    if !state.connected && state.error_banner.is_none() && state.pending_prompt.is_some() {
        card(ui, theme::TEXT_MUTED(), "…", "starting agent…");
    }
}

fn card(ui: &mut egui::Ui, color: Color32, icon: &str, text: &str) {
    Frame {
        fill: theme::TOOL_BG(),
        stroke: Stroke::new(1.0, color),
        corner_radius: CornerRadius::same(8),
        inner_margin: Margin::symmetric(14, 8),
        ..Frame::NONE
    }
    .show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(color, icon);
            ui.colored_label(theme::TEXT(), text);
        });
    });
    ui.add_space(6.0);
}

/// Render the whole transcript. `width` caps line length like Claude Desktop.
pub fn items(ui: &mut egui::Ui, state: &mut AgentState, width: f32) {
    ui.set_max_width(width);
    for item in state.items.iter_mut() {
        match item {
            Item::User(text) => user_bubble(ui, text),
            Item::Assistant(a) => assistant(ui, a),
            Item::System(text) => system(ui, text),
            Item::Error(text) => card(ui, theme::RED(), icons::WARN, text),
        }
        ui.add_space(14.0);
    }
}

fn user_bubble(ui: &mut egui::Ui, text: &str) {
    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
        let max_w = ui.available_width() * 0.8;
        Frame {
            fill: theme::USER_BUBBLE(),
            corner_radius: CornerRadius::same(14),
            inner_margin: Margin::symmetric(16, 12),
            ..Frame::NONE
        }
        .show(ui, |ui| {
            ui.set_max_width(max_w);
            ui.colored_label(theme::TEXT(), egui::RichText::new(text).size(15.0));
        });
    });
}

fn system(ui: &mut egui::Ui, text: &str) {
    ui.vertical_centered(|ui| {
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new(text).size(13.0).italics());
    });
}

fn assistant(ui: &mut egui::Ui, a: &mut Assistant) {
    if !a.thinking.trim().is_empty() {
        thinking(ui, a);
    }
    if !a.text.is_empty() {
        rich_text(ui, &a.text);
    }
    for card in a.tools.iter_mut() {
        ui.add_space(6.0);
        tool_card(ui, card);
    }
}

fn thinking(ui: &mut egui::Ui, a: &mut Assistant) {
    let label = if a.thinking_open {
        format!("{} thinking", icons::CHEVRON_DOWN)
    } else {
        format!("{} thinking", icons::CHEVRON_RIGHT)
    };
    if ui
        .add(egui::Button::new(egui::RichText::new(label).size(13.0).color(theme::TEXT_MUTED())).fill(Color32::TRANSPARENT))
        .clicked()
    {
        a.thinking_open = !a.thinking_open;
    }
    if a.thinking_open {
        Frame {
            fill: theme::CODE_BG(),
            corner_radius: CornerRadius::same(8),
            inner_margin: Margin::symmetric(12, 8),
            ..Frame::NONE
        }
        .show(ui, |ui| {
            ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new(a.thinking.trim()).size(13.0));
        });
    }
}

/// Render assistant text as light markdown: fenced code blocks in monospace,
/// headings, bullet lists, and inline **bold** / `code` (ISC-70).
fn rich_text(ui: &mut egui::Ui, text: &str) {
    let mut in_fence = false;
    let mut fence_buf = String::new();

    let flush_fence = |ui: &mut egui::Ui, buf: &mut String| {
        if buf.is_empty() {
            return;
        }
        Frame {
            fill: theme::CODE_BG(),
            corner_radius: CornerRadius::same(8),
            inner_margin: Margin::symmetric(12, 8),
            ..Frame::NONE
        }
        .show(ui, |ui| {
            ui.colored_label(theme::GREEN(), egui::RichText::new(buf.trim_end()).monospace().size(13.5));
        });
        buf.clear();
    };

    for line in text.split('\n') {
        if line.trim_start().starts_with("```") {
            if in_fence {
                flush_fence(ui, &mut fence_buf);
            }
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            fence_buf.push_str(line);
            fence_buf.push('\n');
        } else {
            md_line(ui, line);
        }
    }
    if in_fence {
        flush_fence(ui, &mut fence_buf);
    }
}

/// Render one non-fenced markdown line (heading / bullet / paragraph).
fn md_line(ui: &mut egui::Ui, line: &str) {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        ui.add_space(6.0);
        return;
    }
    if let Some(rest) = trimmed.strip_prefix("### ") {
        ui.colored_label(theme::TEXT(), egui::RichText::new(rest).size(16.0).strong());
    } else if let Some(rest) = trimmed.strip_prefix("## ") {
        ui.colored_label(theme::TEXT(), egui::RichText::new(rest).size(18.0).strong());
    } else if let Some(rest) = trimmed.strip_prefix("# ") {
        ui.colored_label(theme::TEXT(), egui::RichText::new(rest).size(20.0).strong());
    } else if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(8.0);
            ui.colored_label(theme::ACCENT(), "•");
            inline(ui, rest);
        });
    } else {
        ui.horizontal_wrapped(|ui| inline(ui, trimmed));
    }
}

/// Inline markdown: **bold** and `code` spans built into a LayoutJob.
fn inline(ui: &mut egui::Ui, text: &str) {
    use egui::text::{LayoutJob, TextFormat};
    let base = egui::FontId::proportional(15.0);
    let mono = egui::FontId::monospace(13.5);
    let mut job = LayoutJob::default();
    let mut rest = text;
    let push = |job: &mut LayoutJob, s: &str, font: egui::FontId, color: Color32| {
        job.append(s, 0.0, TextFormat { font_id: font, color, ..Default::default() });
    };
    while !rest.is_empty() {
        if let Some(i) = rest.find("**") {
            if let Some(j) = rest[i + 2..].find("**") {
                push(&mut job, &rest[..i], base.clone(), theme::TEXT());
                push(&mut job, &rest[i + 2..i + 2 + j], base.clone(), theme::TEXT()); // bold ~ same color, heavier not trivial in LayoutJob; keep readable
                rest = &rest[i + 2 + j + 2..];
                continue;
            }
        }
        if let Some(i) = rest.find('`') {
            if let Some(j) = rest[i + 1..].find('`') {
                push(&mut job, &rest[..i], base.clone(), theme::TEXT());
                push(&mut job, &rest[i + 1..i + 1 + j], mono.clone(), theme::GREEN());
                rest = &rest[i + 1 + j + 1..];
                continue;
            }
        }
        push(&mut job, rest, base.clone(), theme::TEXT());
        break;
    }
    ui.label(job);
}

fn tool_card(ui: &mut egui::Ui, c: &mut ToolCard) {
    Frame {
        fill: theme::TOOL_BG(),
        stroke: Stroke::new(1.0, theme::BORDER()),
        corner_radius: CornerRadius::same(10),
        inner_margin: Margin::symmetric(14, 10),
        ..Frame::NONE
    }
    .show(ui, |ui| {
        let resp = ui
            .horizontal(|ui| {
                ui.colored_label(tool_color(&c.name), egui::RichText::new(&c.name).size(14.0).strong());
                ui.add_space(8.0);
                ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new(&c.args_summary).size(13.5));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| status(ui, c));
            })
            .response;
        if resp.interact(egui::Sense::click()).clicked() {
            c.expanded = !c.expanded;
        }
        if c.expanded {
            ui.add_space(6.0);
            if !c.args_full.is_empty() {
                ui.colored_label(theme::TEXT_MUTED(), egui::RichText::new(&c.args_full).monospace().size(12.5));
            }
            if !c.output.is_empty() {
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    ui.colored_label(theme::TEXT(), egui::RichText::new(&c.output).monospace().size(12.5));
                });
            }
        }
    });
}

fn status(ui: &mut egui::Ui, c: &ToolCard) {
    if c.running {
        ui.add(egui::Spinner::new().size(14.0));
    } else if let Some(ms) = c.ms {
        let (icon, color) = if c.is_error { (icons::CROSS, theme::RED()) } else { (icons::CHECK, theme::GREEN()) };
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new(format!("{ms}ms")).size(12.5));
        ui.add_space(4.0);
        ui.colored_label(color, egui::RichText::new(icon).size(14.0));
    } else if c.is_error {
        ui.colored_label(theme::RED(), egui::RichText::new(icons::CROSS).size(14.0));
    } else {
        ui.colored_label(theme::GREEN(), egui::RichText::new(icons::CHECK).size(14.0));
    }
}

fn tool_color(name: &str) -> Color32 {
    match name {
        "read" | "codeindex" | "codegraph" => Color32::from_rgb(120, 160, 220),
        "write" | "edit" | "create" | "apply_patch" => theme::GREEN(),
        "bash" | "sandbox" => theme::YELLOW(),
        _ => theme::ACCENT(),
    }
}
