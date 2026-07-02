//! UI intents. Views are borrow-free: they push `Emit`s into a queue and the
//! App executes them against the right connection after rendering.

use std::path::PathBuf;

use eframe::egui;

use crate::conn::AgentConn;
use crate::{App, Tab};

/// Resolution of an extension-UI dialog (ISC-103..106).
pub enum DialogOut {
    Select(String),
    Confirm(bool),
    Input(String),
    Cancel,
}

pub enum Emit {
    Send(String),
    Abort,
    NewSession,
    SwitchSession(PathBuf),
    SelectProject(PathBuf),
    SelectProjectNone,
    ToggleDir(PathBuf),
    OpenFile(PathBuf),
    CloseFile,
    SetTab(Tab),
    ToggleInspector,
    RefreshSessions,
    SetModel { provider: String, id: String },
    SetThinking(String),
    Compact,
    ExportHtml,
    CloneSession,
    Rename(String),
    RunBash(String),
    TaskAdd(String),
    TaskDone(String),
    TaskMove { id: String, col: String },
    OpenPanel(crate::panels::Panel),
    ClosePanel,
    PanelExec { bin: String, args: Vec<String> },
    TogglePlanFirst,
    WorkInFolder(PathBuf),
    DeleteSession(PathBuf),
    ListForkPoints,
    Fork(String),
    Dialog { id: String, out: DialogOut },
}

impl App {
    pub fn execute(&mut self, emit: Emit, ctx: &egui::Context) {
        let pi = self.paths.pi.clone();
        match emit {
            Emit::Send(text) => {
                // Cowork plan-first gate: ask for a plan + approval before tools.
                let text = if self.active_tab == Tab::Cowork && self.plan_first {
                    format!(
                        "Before doing anything, give me a short numbered plan and wait for my \
                         approval before running any tools or making changes.\n\n{text}"
                    )
                } else {
                    text
                };
                if let Some(conn) = self.active_conn_mut() {
                    conn.send(&pi, ctx, text);
                }
            }
            Emit::TogglePlanFirst => self.plan_first = !self.plan_first,
            Emit::WorkInFolder(path) => {
                let mut conn = AgentConn::new(path);
                conn.ensure(&pi, ctx);
                self.cowork = conn;
                self.active_tab = Tab::Cowork;
                self.active_panel = None;
            }
            Emit::DeleteSession(path) => {
                let active = self
                    .active_conn()
                    .map(|c| c.state.session_file.clone())
                    .unwrap_or_default();
                if path.to_string_lossy() != active {
                    let _ = std::fs::remove_file(&path);
                    self.refresh_sessions();
                }
            }
            Emit::ListForkPoints => {
                if let Some(conn) = self.active_conn() {
                    conn.list_fork_points();
                }
            }
            Emit::Fork(entry_id) => {
                if let Some(conn) = self.active_conn_mut() {
                    conn.fork(&entry_id);
                }
                self.refresh_sessions();
            }
            Emit::Abort => {
                if let Some(conn) = self.active_conn_mut() {
                    conn.abort();
                }
            }
            Emit::NewSession => {
                if let Some(conn) = self.active_conn_mut() {
                    conn.new_session(&pi, ctx);
                }
                self.refresh_sessions();
            }
            Emit::SwitchSession(path) => {
                self.active_tab = Tab::Chat;
                self.chat.switch(&pi, ctx, &path);
            }
            Emit::SelectProject(path) => self.select_project(path, ctx),
            Emit::SelectProjectNone => {
                self.selected_project = None;
                self.open_file = None;
            }
            Emit::CloseFile => self.open_file = None,
            Emit::ToggleDir(path) => {
                if self.expanded_dirs.contains(&path) {
                    self.expanded_dirs.remove(&path);
                } else {
                    self.expanded_dirs.insert(path);
                }
            }
            Emit::OpenFile(path) => self.open_file(path),
            Emit::SetTab(tab) => {
                self.active_tab = tab;
                self.active_panel = None;
                self.ensure_active(ctx);
            }
            Emit::ToggleInspector => self.inspector_open = !self.inspector_open,
            Emit::RefreshSessions => self.refresh_sessions(),
            Emit::SetModel { provider, id } => {
                if let Some(conn) = self.active_conn_mut() {
                    conn.set_model(&provider, &id);
                }
            }
            Emit::SetThinking(level) => {
                if let Some(conn) = self.active_conn_mut() {
                    conn.set_thinking(&level);
                }
            }
            Emit::Compact => {
                if let Some(conn) = self.active_conn_mut() {
                    conn.compact();
                }
            }
            Emit::RunBash(cmd) => {
                if let Some(conn) = self.active_conn_mut() {
                    conn.run_bash(&cmd);
                }
            }
            Emit::CloneSession => {
                if let Some(conn) = self.active_conn_mut() {
                    conn.clone_session();
                }
                self.refresh_sessions();
            }
            Emit::Rename(name) => {
                if let Some(conn) = self.active_conn_mut() {
                    conn.rename(&name);
                }
                self.refresh_sessions();
            }
            Emit::ExportHtml => {
                let dir = self.paths.root.join(".scratch");
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join("session-export.html");
                let p = path.to_string_lossy().to_string();
                if let Some(conn) = self.active_conn_mut() {
                    conn.export_html(&p);
                    conn.state.notice = Some(format!("exported → {p}"));
                }
            }
            Emit::TaskAdd(title) => {
                self.run_tasks(&["add", &title]);
            }
            Emit::TaskDone(id) => {
                self.run_tasks(&["done", &id]);
            }
            Emit::TaskMove { id, col } => {
                self.run_tasks(&["move", &id, &col]);
            }
            Emit::OpenPanel(p) => {
                self.active_panel = Some(p);
                let (bin, args) = p.default_cmd();
                self.panel_exec(bin, &args.iter().map(|s| s.as_str()).collect::<Vec<_>>());
            }
            Emit::ClosePanel => self.active_panel = None,
            Emit::PanelExec { bin, args } => {
                self.panel_exec(&bin, &args.iter().map(|s| s.as_str()).collect::<Vec<_>>());
            }
            Emit::Dialog { id, out } => self.resolve_dialog(&id, out),
        }
    }

    fn resolve_dialog(&mut self, id: &str, out: DialogOut) {
        let Some(conn) = self.active_conn_mut() else { return };
        conn.state.dialogs.retain(|d| d.id != id);
        let fields: Vec<(&str, httpc::json::Value)> = match out {
            DialogOut::Select(v) => vec![("value", crate::jsonw::s(&v))],
            DialogOut::Confirm(b) => vec![("confirmed", crate::jsonw::b(b))],
            DialogOut::Input(v) => vec![("value", crate::jsonw::s(&v))],
            DialogOut::Cancel => vec![("cancelled", crate::jsonw::b(true))],
        };
        conn.dialog_reply(id, fields);
    }
}
