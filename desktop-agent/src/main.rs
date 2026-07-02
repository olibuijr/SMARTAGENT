// Claude Desktop clone — dark mode, 3-pane layout

mod theme;
mod sidebar;
mod chat;
mod inspector;

use eframe::egui;

fn main() -> eframe::Result {
    eframe::run_native(
        "Claude",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280.0, 860.0])
                .with_min_inner_size([900.0, 600.0])
                .with_maximized(true),
            ..Default::default()
        },
        Box::new(|_cc| Ok(Box::new(App::default()))),
    )
}

#[derive(Clone, PartialEq)]
pub enum Tab { Chat, Code, Cowork }

#[derive(Clone)]
pub struct Session {
    pub title: String,
    pub pinned: bool,
    pub scheduled: bool,
    pub _time: String,
}

#[derive(Clone)]
pub struct Message {
    pub role: MessageRole,
    pub text: String,
    pub tool_cards: Vec<ToolCard>,
}

#[derive(Clone, PartialEq)]
pub enum MessageRole { User, Claude }

#[derive(Clone)]
pub struct ToolCard {
    pub label: String,
    pub detail: String,
}

#[derive(Clone)]
pub struct ProgressStep {
    pub label: String,
    pub done: bool,
}

pub struct App {
    pub active_tab: Tab,
    pub sessions: Vec<Session>,
    pub selected_session: usize,
    pub messages: Vec<Message>,
    pub chat_input: String,
    pub right_panel_open: bool,
    pub progress_steps: Vec<ProgressStep>,
    pub artifacts: Vec<String>,
    pub working_files: Vec<String>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            active_tab: Tab::Chat,
            sessions: vec![
                Session { title: "Add a dark mode toggle to settings".into(), pinned: true, scheduled: false, _time: "Today".into() },
                Session { title: "Weekly dependency audit".into(), pinned: false, scheduled: true, _time: "Tomorrow".into() },
                Session { title: "Fix the double-charge bug in checkout".into(), pinned: false, scheduled: false, _time: "2h ago".into() },
                Session { title: "Write tests for the payments module".into(), pinned: false, scheduled: false, _time: "Yesterday".into() },
                Session { title: "Explain what this repo does".into(), pinned: false, scheduled: false, _time: "2 days ago".into() },
            ],
            selected_session: 0,
            messages: vec![
                Message {
                    role: MessageRole::User,
                    text: "Add a dark mode toggle to the settings page".into(),
                    tool_cards: vec![],
                },
                Message {
                    role: MessageRole::Claude,
                    text: "I'll add a dark mode toggle to the settings page. Let me first look at the current settings structure.".into(),
                    tool_cards: vec![
                        ToolCard { label: "Read".into(), detail: "src/components/Settings.tsx".into() },
                        ToolCard { label: "Read".into(), detail: "src/theme/ThemeProvider.tsx".into() },
                        ToolCard { label: "Edit".into(), detail: "src/theme/ThemeProvider.tsx  +18 −2".into() },
                    ],
                },
                Message {
                    role: MessageRole::Claude,
                    text: "Done! I added a `useDarkMode` hook and a toggle switch to the settings page. The preference is persisted in localStorage and applies instantly.".into(),
                    tool_cards: vec![],
                },
            ],
            chat_input: String::new(),
            right_panel_open: true,
            progress_steps: vec![
                ProgressStep { label: "Read settings files".into(), done: true },
                ProgressStep { label: "Add dark mode hook".into(), done: true },
                ProgressStep { label: "Create toggle component".into(), done: true },
                ProgressStep { label: "Update components".into(), done: false },
            ],
            artifacts: vec!["dark-mode-toggle.tsx".into()],
            working_files: vec![
                "src/components/Settings.tsx".into(),
                "src/theme/ThemeProvider.tsx".into(),
                "src/hooks/useDarkMode.ts".into(),
            ],
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::apply(ctx);

        egui::SidePanel::left("sidebar")
            .resizable(false)
            .min_width(260.0)
            .max_width(260.0)
            .frame(egui::Frame {
                fill: theme::SIDEBAR,
                inner_margin: egui::Margin::same(0),
                ..egui::Frame::NONE
            })
            .show(ctx, |ui| sidebar::render(self, ui));

        if self.right_panel_open {
            egui::SidePanel::right("inspector")
                .resizable(false)
                .min_width(300.0)
                .max_width(300.0)
                .frame(egui::Frame {
                    fill: theme::SIDEBAR,
                    inner_margin: egui::Margin::same(0),
                    ..egui::Frame::NONE
                })
                .show(ctx, |ui| inspector::render(self, ui));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: theme::PANEL,
                inner_margin: egui::Margin::same(0),
                ..egui::Frame::NONE
            })
            .show(ctx, |ui| chat::render(self, ui));
    }
}
