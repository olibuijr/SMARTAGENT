//! Interactive kanban parsed from `tasks board`. Add a task, advance a task to
//! the next column, or mark it done — each shells to the real `tasks` binary.

use eframe::egui::{self, Color32, Vec2};

use crate::emit::Emit;
use crate::icons;
use crate::theme;

const COLUMNS: &[&str] = &["backlog", "ready", "doing", "review", "done"];

pub struct Task {
    pub id: String,
    pub prio: String,
    pub title: String,
    pub col: String,
}

/// Parse `tasks board` text lines into (column, tasks).
pub fn parse(lines: &[String]) -> Vec<(String, Vec<Task>)> {
    let mut cols: Vec<(String, Vec<Task>)> = Vec::new();
    let mut cur = String::new();
    for line in lines {
        let trimmed = line.trim_start();
        if !line.starts_with(char::is_whitespace) && trimmed.contains('(') && line.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            // Column header, e.g. "BACKLOG (9)" / "DOING (1/1)".
            let name = trimmed.split_whitespace().next().unwrap_or("").to_lowercase();
            cur = name.clone();
            cols.push((name, Vec::new()));
        } else if let Some(t) = parse_task(trimmed, &cur) {
            if let Some((_, v)) = cols.last_mut() {
                v.push(t);
            }
        }
    }
    cols
}

fn parse_task(s: &str, col: &str) -> Option<Task> {
    let rest = s.strip_prefix("T-")?;
    let mut it = rest.splitn(3, ' ');
    let num = it.next()?;
    let prio = it.next().unwrap_or("");
    let title = it.next().unwrap_or("");
    Some(Task {
        id: format!("T-{num}"),
        prio: prio.to_string(),
        title: title.to_string(),
        col: col.to_string(),
    })
}

fn next_col(col: &str) -> Option<&'static str> {
    let i = COLUMNS.iter().position(|c| *c == col)?;
    COLUMNS.get(i + 1).copied()
}

pub fn render(ui: &mut egui::Ui, lines: &[String], input: &mut String, emits: &mut Vec<Emit>) {
    let cols = parse(lines);
    egui::Frame {
        fill: theme::TOOL_BG(),
        stroke: egui::Stroke::new(1.0, theme::BORDER()),
        corner_radius: egui::CornerRadius::same(10),
        inner_margin: egui::Margin::symmetric(14, 10),
        ..egui::Frame::NONE
    }
    .show(ui, |ui| {
        // Add-task row.
        ui.horizontal(|ui| {
            ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new("Board").size(12.5).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let add = ui.add(
                    egui::Button::new(egui::RichText::new("+ Add").size(12.0).color(theme::ACCENT()))
                        .fill(Color32::TRANSPARENT),
                );
                let resp = ui.add(
                    egui::TextEdit::singleline(input).desired_width(160.0).hint_text("new task…"),
                );
                let go = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (add.clicked() || go) && !input.trim().is_empty() {
                    emits.push(Emit::TaskAdd(input.trim().to_string()));
                    input.clear();
                }
            });
        });
        ui.add_space(4.0);

        egui::ScrollArea::vertical().id_salt("kanban").max_height(200.0).show(ui, |ui| {
            for (col, tasks) in &cols {
                if tasks.is_empty() {
                    continue;
                }
                ui.colored_label(theme::ACCENT(), egui::RichText::new(format!("{} ({})", col.to_uppercase(), tasks.len())).size(12.5).strong());
                for t in tasks {
                    task_row(ui, t, emits);
                }
                ui.add_space(2.0);
            }
        });
    });
}

fn task_row(ui: &mut egui::Ui, t: &Task, emits: &mut Vec<Emit>) {
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let prio_color = match t.prio.as_str() {
            "p1" => theme::RED(),
            "p2" => theme::YELLOW(),
            _ => theme::TEXT_FAINT(),
        };
        ui.colored_label(prio_color, egui::RichText::new(&t.prio).monospace().size(11.5));
        ui.colored_label(theme::TEXT_MUTED(), egui::RichText::new(format!("{} {}", t.id, crate::jsonw::truncate_chars(&t.title, 40))).size(12.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if t.col != "done" {
                if mini(ui, icons::CHECK).on_hover_text("Mark done").clicked() {
                    emits.push(Emit::TaskDone(t.id.clone()));
                }
                if let Some(nc) = next_col(&t.col) {
                    if mini(ui, icons::ARROW_RIGHT).on_hover_text(format!("Move to {nc}")).clicked() {
                        emits.push(Emit::TaskMove { id: t.id.clone(), col: nc.to_string() });
                    }
                }
            }
        });
    });
}

fn mini(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(12.0).color(theme::TEXT_MUTED()))
            .fill(Color32::TRANSPARENT)
            .min_size(Vec2::new(22.0, 20.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_board_columns_and_tasks() {
        let lines: Vec<String> = [
            "BACKLOG (2)",
            "  T-5 p3 Refresh stale --help text [0/1✓]",
            "  T-10 p1 Triage root board over-WIP",
            "DOING (1/1)",
            "  T-16 p2 QA defect runtime",
            "DONE (1)",
            "  T-1 p1 verify tasks live [2/2✓]",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let cols = parse(&lines);
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[0].0, "backlog");
        assert_eq!(cols[0].1.len(), 2);
        assert_eq!(cols[0].1[0].id, "T-5");
        assert_eq!(cols[0].1[0].prio, "p3");
        assert_eq!(cols[1].0, "doing");
        assert_eq!(cols[1].1[0].id, "T-16");
        assert_eq!(cols[1].1[0].col, "doing");
    }

    #[test]
    fn advance_follows_column_order() {
        assert_eq!(next_col("backlog"), Some("ready"));
        assert_eq!(next_col("review"), Some("done"));
        assert_eq!(next_col("done"), None);
    }
}
