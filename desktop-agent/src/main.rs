//! SMARTAGENT Desktop — Claude-Desktop-style GUI over the real `./pi` agent
//! (RPC mode). Module map in desktop-agent/COMPONENTS.md.

mod agent;
mod chat;
mod code;
mod composer;
mod conn;
mod cowork;
mod data;
mod dialogs;
mod emit;
mod inspector;
mod jsonw;
mod root;
mod rpc;
mod sessions;
mod sidebar;
mod theme;
mod transcript;

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui;

use conn::AgentConn;
use emit::Emit;
use root::Paths;
use sessions::SessionMeta;

fn main() -> eframe::Result {
    eframe::run_native(
        "SMARTAGENT",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1280.0, 860.0])
                .with_min_inner_size([900.0, 600.0]),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    )
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Chat,
    Cowork,
    Code,
}

pub struct App {
    pub paths: Paths,
    pub active_tab: Tab,
    pub chat: AgentConn,
    pub cowork: AgentConn,
    pub code: Option<AgentConn>,
    pub sessions: Vec<SessionMeta>,
    pub projects: Vec<PathBuf>,
    pub selected_project: Option<usize>,
    pub expanded_dirs: HashSet<PathBuf>,
    pub open_file: Option<(PathBuf, String)>,
    pub git_status: Vec<String>,
    pub git_is_repo: bool,
    pub tasks_board: Vec<String>,
    pub inspector_open: bool,
    startup_error: Option<String>,
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let (paths, startup_error) = match Paths::discover() {
            Some(p) => (p, None),
            None => {
                // Unusable-but-alive fallback (ISC-7): current dir, error banner.
                let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let msg = "SMARTAGENT repo not found (need ./pi and .pi/ in an ancestor directory)";
                (
                    Paths_fallback(cwd),
                    Some(msg.to_string()),
                )
            }
        };
        let root_dir = paths.root.clone();
        let mut app = Self {
            paths,
            active_tab: Tab::Chat,
            chat: AgentConn::new(root_dir.clone()),
            cowork: AgentConn::new(root_dir),
            code: None,
            sessions: Vec::new(),
            projects: Vec::new(),
            selected_project: None,
            expanded_dirs: HashSet::new(),
            open_file: None,
            git_status: Vec::new(),
            git_is_repo: false,
            tasks_board: Vec::new(),
            inspector_open: true,
            startup_error,
        };
        theme::start_watcher(ctx.clone());
        if app.startup_error.is_none() {
            app.refresh_sessions();
            app.refresh_projects();
            app.refresh_tasks();
            // Boot the Chat agent immediately in the background (ISC-131):
            // window appears now, child connects when ready.
            app.chat.ensure(&app.paths.pi.clone(), ctx);
        } else {
            app.chat.state.error_banner = app.startup_error.clone();
        }
        app
    }

    fn pump_all(&mut self, ctx: &egui::Context) {
        let chat = self.chat.pump();
        let cowork = self.cowork.pump();
        let code = self.code.as_mut().map(|c| c.pump());

        // After any turn ends: refresh sessions (titles/new files), board, git.
        let turn_ended =
            chat.turn_ended || cowork.turn_ended || code.as_ref().map(|p| p.turn_ended).unwrap_or(false);
        if turn_ended {
            self.refresh_sessions();
            self.refresh_tasks();
            if self.active_tab == Tab::Code {
                self.refresh_git();
            }
        }

        // Window title follows session name / extension setTitle (ISC-108/115).
        if let Some(conn) = self.active_conn() {
            let t = conn
                .state
                .window_title
                .clone()
                .or_else(|| {
                    if conn.state.session_name.is_empty() {
                        None
                    } else {
                        Some(conn.state.session_name.clone())
                    }
                })
                .map(|s| format!("{s} — SMARTAGENT"))
                .unwrap_or_else(|| "SMARTAGENT".to_string());
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(t));
        }

        // Streaming: keep elapsed timers/spinners moving (ISC-114: ≤4Hz, only
        // while streaming; idle app repaints on events only).
        let streaming = [Some(&self.chat), Some(&self.cowork), self.code.as_ref()]
            .into_iter()
            .flatten()
            .any(|c| c.state.streaming);
        if streaming {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}

/// Build a Paths value for the fallback case without repo discovery.
#[allow(non_snake_case)]
fn Paths_fallback(cwd: PathBuf) -> Paths {
    Paths {
        pi: cwd.join("pi"),
        sessions_dir: cwd.join(".pi").join("sessions"),
        workspaces_dir: cwd.join("workspaces"),
        tasks_bin: cwd.join("target").join("release").join("tasks"),
        root: cwd,
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::apply(ctx);
        self.pump_all(ctx);

        let mut emits: Vec<Emit> = Vec::new();

        egui::SidePanel::left("sidebar")
            .resizable(false)
            .min_width(250.0)
            .max_width(250.0)
            .frame(egui::Frame {
                fill: theme::SIDEBAR(),
                inner_margin: egui::Margin::same(0),
                ..egui::Frame::NONE
            })
            .show(ctx, |ui| sidebar::render(ui, self, &mut emits));

        if self.inspector_open {
            egui::SidePanel::right("inspector")
                .resizable(false)
                .min_width(290.0)
                .max_width(290.0)
                .frame(egui::Frame {
                    fill: theme::SIDEBAR(),
                    inner_margin: egui::Margin::same(0),
                    ..egui::Frame::NONE
                })
                .show(ctx, |ui| {
                    let state = self.active_conn().map(|c| &c.state);
                    inspector::render(ui, state, &mut emits);
                });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: theme::PANEL(),
                inner_margin: egui::Margin::same(0),
                ..egui::Frame::NONE
            })
            .show(ctx, |ui| {
                if !self.inspector_open {
                    reopen_inspector_button(ui, &mut emits);
                }
                match self.active_tab {
                    Tab::Chat => {
                        let user = root::username();
                        chat::render(ui, &mut self.chat, &user, &mut emits);
                    }
                    Tab::Cowork => {
                        let board = self.tasks_board.clone();
                        cowork::render(ui, &mut self.cowork, &board, &mut emits);
                    }
                    Tab::Code => code::render(ui, self, &mut emits),
                }
            });

        // Modal dialogs float above everything (active tab's agent).
        if let Some(conn) = self.active_conn_mut() {
            dialogs::render(ctx, &mut conn.state, &mut emits);
        }

        for e in emits {
            self.execute(e, ctx);
        }
    }
}

fn reopen_inspector_button(ui: &mut egui::Ui, emits: &mut Vec<Emit>) {
    egui::Area::new(egui::Id::new("reopen-inspector"))
        .anchor(egui::Align2::RIGHT_TOP, egui::Vec2::new(-10.0, 10.0))
        .show(ui.ctx(), |ui| {
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("☰").size(15.0).color(theme::TEXT_MUTED()))
                        .fill(theme::CARD())
                        .corner_radius(6),
                )
                .on_hover_text("Show session panel")
                .clicked()
            {
                emits.push(Emit::ToggleInspector);
            }
        });
}
