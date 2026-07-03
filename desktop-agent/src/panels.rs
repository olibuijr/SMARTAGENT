//! Tool panels — a command-center surface over the repo's Rust tool binaries.
//! Each panel runs a real binary (`workflow`, `memory`, `vault`, `schedule`,
//! `supervise`, `hooks`, `evals`), shows its output, and offers a few actions.
//! All execution goes through `Emit::PanelExec { bin, args }` (App runs it).

use eframe::egui::{self, Color32, Vec2};

use crate::emit::Emit;
use crate::icons;
use crate::theme;

#[derive(Clone, Copy, PartialEq)]
pub enum Panel {
    Workflow,
    Memory,
    Vault,
    Schedule,
    Services,
    Hooks,
    Evals,
    Orchestrate,
    Mcp,
}

impl Panel {
    pub fn title(self) -> &'static str {
        match self {
            Panel::Workflow => "Workflow runs",
            Panel::Memory => "Memory",
            Panel::Vault => "Vault notes",
            Panel::Schedule => "Scheduled tasks",
            Panel::Services => "Services",
            Panel::Hooks => "Hooks audit",
            Panel::Evals => "Evals / traces",
            Panel::Orchestrate => "Subagent runs",
            Panel::Mcp => "MCP servers",
        }
    }

    pub const ALL: [Panel; 9] = [
        Panel::Workflow,
        Panel::Memory,
        Panel::Vault,
        Panel::Schedule,
        Panel::Services,
        Panel::Hooks,
        Panel::Evals,
        Panel::Orchestrate,
        Panel::Mcp,
    ];

    /// The command run when the panel opens / refreshes.
    pub fn default_cmd(self) -> (&'static str, Vec<String>) {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        match self {
            Panel::Workflow => ("workflow", a(&["runs"])),
            Panel::Memory => ("memory", a(&["recent", "--dir", "data/memory", "--tier", "episodic", "--n", "15"])),
            Panel::Vault => ("vault", a(&["list", "notes"])),
            Panel::Schedule => ("schedule", a(&["list"])),
            Panel::Services => ("supervise", a(&["status"])),
            Panel::Hooks => ("hooks", a(&["audit", "--n", "20"])),
            Panel::Evals => ("evals", a(&["runs"])),
            Panel::Orchestrate => ("orchestrate", a(&["list"])),
            Panel::Mcp => ("mcp", a(&["tools", "--names-only"])),
        }
    }
}

pub fn render(ui: &mut egui::Ui, panel: Panel, out: &[String], input: &mut String, emits: &mut Vec<Emit>) {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.colored_label(theme::TEXT(), egui::RichText::new(panel.title()).size(18.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(14.0);
            if btn(ui, &format!("{} Close", icons::CLOSE)).clicked() {
                emits.push(Emit::ClosePanel);
            }
            if btn(ui, &format!("{} Refresh", icons::REFRESH)).clicked() {
                let (bin, args) = panel.default_cmd();
                emits.push(Emit::PanelExec { bin: bin.to_string(), args });
            }
        });
    });
    ui.add_space(8.0);

    actions(ui, panel, input, emits);
    ui.separator();

    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(6.0);
        if out.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.colored_label(theme::TEXT_FAINT(), egui::RichText::new("(no output)").italics().size(13.0));
            });
        }
        for line in out {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.colored_label(theme::TEXT_MUTED(), egui::RichText::new(line).monospace().size(12.5));
            });
        }
    });
}

/// Per-panel action row (inputs + buttons that run tool verbs).
fn actions(ui: &mut egui::Ui, panel: Panel, input: &mut String, emits: &mut Vec<Emit>) {
    let run = |emits: &mut Vec<Emit>, bin: &str, args: Vec<&str>| {
        emits.push(Emit::PanelExec { bin: bin.to_string(), args: args.iter().map(|s| s.to_string()).collect() });
    };
    ui.horizontal_wrapped(|ui| {
        ui.add_space(14.0);
        match panel {
            Panel::Memory => {
                ui.colored_label(theme::TEXT_FAINT(), "recall:");
                let resp = ui.add(egui::TextEdit::singleline(input).desired_width(180.0).hint_text("query…"));
                let go = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (btn(ui, "Search").clicked() || go) && !input.trim().is_empty() {
                    run(emits, "memory", vec!["recall", input.trim(), "--dir", "data/memory"]);
                }
                if btn(ui, "working").clicked() { run(emits, "memory", vec!["recent","--dir","data/memory","--tier","working","--n","15"]); }
                if btn(ui, "semantic").clicked() { run(emits, "memory", vec!["recent","--dir","data/memory","--tier","semantic","--n","15"]); }
            }
            Panel::Vault => {
                ui.colored_label(theme::TEXT_FAINT(), "search:");
                let resp = ui.add(egui::TextEdit::singleline(input).desired_width(180.0).hint_text("keyword…"));
                let go = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (btn(ui, "Search").clicked() || go) && !input.trim().is_empty() {
                    run(emits, "vault", vec!["search", "notes", input.trim()]);
                }
            }
            Panel::Schedule => {
                ui.colored_label(theme::TEXT_FAINT(), "job:");
                ui.add(egui::TextEdit::singleline(input).desired_width(120.0).hint_text("name"));
                if btn(ui, "Pause").clicked() && !input.trim().is_empty() { run(emits, "schedule", vec!["pause", input.trim()]); }
                if btn(ui, "Resume").clicked() && !input.trim().is_empty() { run(emits, "schedule", vec!["resume", input.trim()]); }
                if btn(ui, "Remove").clicked() && !input.trim().is_empty() { run(emits, "schedule", vec!["rm", input.trim()]); }
            }
            Panel::Services => {
                for svc in ["scheduler", "gateway", "chromium"] {
                    ui.colored_label(theme::TEXT_MUTED(), svc);
                    if btn(ui, "up").clicked() { run(emits, "supervise", vec!["up", svc]); }
                    if btn(ui, "down").clicked() { run(emits, "supervise", vec!["down", svc]); }
                    if btn(ui, "restart").clicked() { run(emits, "supervise", vec!["restart", svc]); }
                    ui.add_space(8.0);
                }
            }
            Panel::Workflow => {
                if btn(ui, "definitions").clicked() { run(emits, "workflow", vec!["list"]); }
            }
            _ => {}
        }
    });
    ui.add_space(6.0);
}

fn btn(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).size(12.5).color(theme::TEXT_MUTED()))
            .fill(theme::TOOL_BG())
            .corner_radius(6)
            .min_size(Vec2::new(0.0, 26.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Sidebar launcher list.
pub fn launcher(ui: &mut egui::Ui, width: f32, active: Option<Panel>, emits: &mut Vec<Emit>) {
    for p in Panel::ALL {
        let sel = active == Some(p);
        let bg = if sel { theme::SELECTED() } else { Color32::TRANSPARENT };
        let resp = ui.add(
            egui::Button::new(egui::RichText::new(p.title()).size(13.0).color(if sel { theme::TEXT() } else { theme::TEXT_MUTED() }))
                .fill(bg)
                .corner_radius(6)
                .min_size(Vec2::new(width - 24.0, 28.0)),
        );
        if resp.clicked() {
            emits.push(Emit::OpenPanel(p));
        }
    }
}
