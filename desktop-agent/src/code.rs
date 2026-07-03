//! Code tab: workspace project picker, lazy file tree, read-only file viewer,
//! git changed-files pane, and a project-rooted agent chat.

use eframe::egui::{self, Color32, Vec2};

use crate::composer;
use crate::conn::AgentConn;
use crate::emit::Emit;
use crate::theme;
use crate::icons;
use crate::transcript;
use crate::App;

pub fn render(ui: &mut egui::Ui, app: &mut App, emits: &mut Vec<Emit>) {
    if app.selected_project.is_none() {
        picker(ui, app, emits);
        return;
    }

    // Left: tree + git. Right: viewer or chat.
    egui::SidePanel::left("code-tree")
        .resizable(true)
        .default_width(260.0)
        .frame(egui::Frame {
            fill: theme::SIDEBAR(),
            inner_margin: egui::Margin::same(8),
            ..egui::Frame::NONE
        })
        .show_inside(ui, |ui| {
            header(ui, app, emits);
            ui.add_space(6.0);
            tree(ui, app, emits);
            ui.add_space(8.0);
            git_pane(ui, app);
        });

    egui::CentralPanel::default()
        .frame(egui::Frame {
            fill: theme::PANEL(),
            inner_margin: egui::Margin::same(0),
            ..egui::Frame::NONE
        })
        .show_inside(ui, |ui| {
            if let Some((path, body)) = &app.open_file {
                viewer(ui, path.display().to_string(), body, emits);
            } else if let Some(conn) = app.code.as_mut() {
                chat_pane(ui, conn, emits);
            }
        });
}

fn picker(ui: &mut egui::Ui, app: &App, emits: &mut Vec<Emit>) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.18);
        ui.colored_label(theme::TEXT(), egui::RichText::new("Pick a project").size(26.0));
        ui.add_space(4.0);
        ui.colored_label(
            theme::TEXT_MUTED(),
            egui::RichText::new("Repo root and workspaces/ projects — the agent runs in that directory.").size(14.0),
        );
        ui.add_space(18.0);
        egui::ScrollArea::vertical().max_height(ui.available_height() - 40.0).show(ui, |ui| {
            for p in &app.projects {
                let name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("(root)")
                    .to_string();
                let is_root = p == &app.paths.root;
                let label = if is_root { format!("{} {name} (repo root)", icons::HOME) } else { format!("{} {name}", icons::FOLDER) };
                let btn = egui::Button::new(egui::RichText::new(label).size(15.0).color(theme::TEXT()))
                    .fill(theme::CARD())
                    .corner_radius(8)
                    .min_size(Vec2::new(420.0, 40.0));
                if ui.add(btn).clicked() {
                    emits.push(Emit::SelectProject(p.clone()));
                }
                ui.add_space(6.0);
            }
        });
    });
}

fn header(ui: &mut egui::Ui, app: &App, emits: &mut Vec<Emit>) {
    let name = app
        .selected_project
        .and_then(|i| app.projects.get(i))
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();
    ui.horizontal(|ui| {
        ui.colored_label(theme::TEXT(), egui::RichText::new(name).size(15.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(egui::RichText::new(icons::REFRESH).size(13.0).color(theme::TEXT_MUTED())).fill(Color32::TRANSPARENT))
                .on_hover_text("Change project")
                .clicked()
            {
                emits.push(Emit::SetTab(crate::Tab::Code));
                emits.push(Emit::SelectProjectNone);
            }
        });
    });
}

fn tree(ui: &mut egui::Ui, app: &App, emits: &mut Vec<Emit>) {
    let rows = app.file_tree_rows();
    let h = (ui.available_height() - 160.0).max(120.0);
    egui::ScrollArea::vertical().id_salt("tree").max_height(h).show(ui, |ui| {
        for row in rows {
            ui.horizontal(|ui| {
                ui.add_space(6.0 + row.depth as f32 * 14.0);
                let icon = if row.is_dir {
                    if app.expanded_dirs.contains(&row.path) { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT }
                } else {
                    " "
                };
                let text = format!("{icon} {}", row.name);
                let color = if row.is_dir { theme::TEXT() } else { theme::TEXT_MUTED() };
                let btn = egui::Button::new(egui::RichText::new(text).size(13.5).color(color))
                    .fill(Color32::TRANSPARENT);
                if ui.add(btn).clicked() {
                    if row.is_dir {
                        emits.push(Emit::ToggleDir(row.path.clone()));
                    } else {
                        emits.push(Emit::OpenFile(row.path.clone()));
                    }
                }
            });
        }
    });
}

fn git_pane(ui: &mut egui::Ui, app: &App) {
    ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new("Changes").size(12.5).strong());
    ui.add_space(4.0);
    if !app.git_is_repo {
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new("not a git repo").size(12.5).italics());
        return;
    }
    if app.git_status.is_empty() {
        ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new("working tree clean").size(12.5));
        return;
    }
    egui::ScrollArea::vertical().id_salt("git").max_height(120.0).show(ui, |ui| {
        for line in &app.git_status {
            let color = match line.trim_start().chars().next() {
                Some('M') => theme::YELLOW(),
                Some('A') | Some('?') => theme::GREEN(),
                Some('D') => theme::RED(),
                _ => theme::TEXT_MUTED(),
            };
            ui.colored_label(color, egui::RichText::new(line).monospace().size(12.0));
        }
    });
}

fn viewer(ui: &mut egui::Ui, title: String, body: &str, emits: &mut Vec<Emit>) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.colored_label(theme::TEXT_MUTED(), egui::RichText::new(&title).monospace().size(13.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(12.0);
            if ui
                .add(egui::Button::new(egui::RichText::new(format!("{} close", icons::CLOSE)).size(12.5).color(theme::TEXT_MUTED())).fill(Color32::TRANSPARENT))
                .clicked()
            {
                emits.push(Emit::CloseFile);
            }
        });
    });
    ui.separator();
    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame {
            fill: theme::CODE_BG(),
            inner_margin: egui::Margin::same(12),
            ..egui::Frame::NONE
        }
        .show(ui, |ui| {
            ui.colored_label(theme::TEXT(), egui::RichText::new(body).monospace().size(13.0));
        });
    });
}

fn chat_pane(ui: &mut egui::Ui, conn: &mut AgentConn, emits: &mut Vec<Emit>) {
    let avail = ui.available_size();
    let width = (avail.x * 0.9).clamp(320.0, 760.0);
    ui.vertical_centered(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(width, avail.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.add_space(8.0);
                transcript::banners(ui, &conn.state);
                let h = avail.y - 140.0;
                egui::ScrollArea::vertical()
                    .id_salt("code-chat")
                    .max_height(h.max(120.0))
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if conn.state.items.is_empty() {
                            ui.add_space(24.0);
                            ui.vertical_centered(|ui| {
                                ui.colored_label(
                                    theme::TEXT_MUTED(),
                                    egui::RichText::new("Ask the agent to work in this project.").size(14.5),
                                );
                            });
                        } else {
                            transcript::items(ui, &mut conn.state, width);
                        }
                    });
                ui.add_space(8.0);
                composer::render(ui, &conn.state, &mut conn.input, width, "Code in this project…", emits);
            },
        );
    });
}
