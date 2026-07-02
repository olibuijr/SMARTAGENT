//! UI intents. Views are borrow-free: they push `Emit`s into a queue and the
//! App executes them against the right connection after rendering.

use std::path::PathBuf;

use eframe::egui;

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
    Dialog { id: String, out: DialogOut },
}

impl App {
    pub fn execute(&mut self, emit: Emit, ctx: &egui::Context) {
        let pi = self.paths.pi.clone();
        match emit {
            Emit::Send(text) => {
                if let Some(conn) = self.active_conn_mut() {
                    conn.send(&pi, ctx, text);
                }
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
                self.ensure_active(ctx);
            }
            Emit::ToggleInspector => self.inspector_open = !self.inspector_open,
            Emit::RefreshSessions => self.refresh_sessions(),
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
