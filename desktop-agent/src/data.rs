//! App-side data refresh: session list, workspace projects, file tree, git
//! status, tasks board. All read real repo state — never mock (ISC-119).

use std::path::{Path, PathBuf};
use std::process::Command;

use eframe::egui;

use crate::conn::AgentConn;
use crate::sessions;
use crate::{App, Tab};

/// A single file-tree row for the Code tab.
pub struct TreeRow {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
}

const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".smartagent", ".pi"];

impl App {
    pub fn active_conn_mut(&mut self) -> Option<&mut AgentConn> {
        match self.active_tab {
            Tab::Chat => Some(&mut self.chat),
            Tab::Cowork => Some(&mut self.cowork),
            Tab::Code => self.code.as_mut(),
        }
    }

    pub fn active_conn(&self) -> Option<&AgentConn> {
        match self.active_tab {
            Tab::Chat => Some(&self.chat),
            Tab::Cowork => Some(&self.cowork),
            Tab::Code => self.code.as_ref(),
        }
    }

    /// Spawn the active tab's child if the tab needs one but hasn't got one.
    pub fn ensure_active(&mut self, ctx: &egui::Context) {
        let pi = self.paths.pi.clone();
        if let Some(conn) = self.active_conn_mut() {
            conn.ensure(&pi, ctx);
        }
    }

    pub fn refresh_sessions(&mut self) {
        self.sessions = sessions::list_sessions(&self.paths.sessions_dir, &self.paths.workspaces_dir);
    }

    pub fn refresh_projects(&mut self) {
        let mut projects = Vec::new();
        // Repo root itself is always a valid coding context.
        projects.push(self.paths.root.clone());
        if let Ok(rd) = std::fs::read_dir(&self.paths.workspaces_dir) {
            let mut subs: Vec<PathBuf> =
                rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            subs.sort();
            projects.extend(subs);
        }
        self.projects = projects;
    }

    pub fn select_project(&mut self, path: PathBuf, ctx: &egui::Context) {
        let pi = self.paths.pi.clone();
        self.selected_project = self.projects.iter().position(|p| p == &path);
        self.expanded_dirs.clear();
        self.open_file = None;
        let mut conn = AgentConn::new(path.clone());
        conn.ensure(&pi, ctx);
        self.code = Some(conn);
        self.active_tab = Tab::Code;
        self.refresh_git();
    }

    pub fn open_file(&mut self, path: PathBuf) {
        const CAP: usize = 200 * 1024;
        match std::fs::read(&path) {
            Ok(bytes) => {
                let shown = &bytes[..bytes.len().min(CAP)];
                match std::str::from_utf8(shown) {
                    Ok(s) => {
                        let mut body = s.to_string();
                        if bytes.len() > CAP {
                            body.push_str("\n… (file truncated at 200 KB)");
                        }
                        self.open_file = Some((path, body));
                    }
                    Err(_) => {
                        self.open_file = Some((path, "(binary file — not shown)".to_string()));
                    }
                }
            }
            Err(e) => {
                self.open_file = Some((path, format!("(could not read: {e})")));
            }
        }
    }

    /// `git status --porcelain` in the selected project (ISC-87/92).
    pub fn refresh_git(&mut self) {
        let Some(idx) = self.selected_project else {
            self.git_status = Vec::new();
            self.git_is_repo = false;
            return;
        };
        let cwd = self.projects[idx].clone();
        if !cwd.join(".git").exists() {
            self.git_is_repo = false;
            self.git_status = Vec::new();
            return;
        }
        self.git_is_repo = true;
        let out = Command::new("git")
            .arg("status")
            .arg("--porcelain")
            .current_dir(&cwd)
            .output();
        self.git_status = match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.to_string())
                .collect(),
            Err(_) => Vec::new(),
        };
    }

    /// `schedule list` output for the Cowork Scheduled section (ISC-143).
    pub fn refresh_scheduled(&mut self) {
        let bin = self.paths.root.join("target").join("release").join("schedule");
        if !bin.exists() {
            self.scheduled_jobs = Vec::new();
            return;
        }
        let out = Command::new(&bin).arg("list").current_dir(&self.paths.root).output();
        self.scheduled_jobs = match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .take(20)
                .collect(),
            Err(_) => Vec::new(),
        };
    }

    /// Run a tool binary at repo root and store its output for a Tools panel.
    pub fn panel_exec(&mut self, bin: &str, args: &[&str]) {
        let path = self.paths.root.join("target").join("release").join(bin);
        if !path.exists() {
            self.panel_out = vec![format!("{bin} binary not built — run ./build.sh")];
            return;
        }
        let out = Command::new(&path).args(args).current_dir(&self.paths.root).output();
        self.panel_out = match out {
            Ok(o) => {
                let mut text = String::from_utf8_lossy(&o.stdout).to_string();
                let err = String::from_utf8_lossy(&o.stderr);
                if !err.trim().is_empty() {
                    text.push_str(&err);
                }
                if text.trim().is_empty() {
                    vec!["(no output)".to_string()]
                } else {
                    text.lines().map(crate::agent::strip_ansi).take(500).collect()
                }
            }
            Err(e) => vec![format!("error: {e}")],
        };
    }

    /// Run a `tasks` subcommand at repo root, then refresh the board (ISC-156).
    pub fn run_tasks(&mut self, args: &[&str]) {
        if self.paths.tasks_bin.exists() {
            let _ = Command::new(&self.paths.tasks_bin)
                .args(args)
                .current_dir(&self.paths.root)
                .output();
        }
        self.refresh_tasks();
    }

    /// `tasks board` output for the Cowork kanban strip (ISC-77/82).
    pub fn refresh_tasks(&mut self) {
        if !self.paths.tasks_bin.exists() {
            self.tasks_board = vec!["tasks binary not built — run ./build.sh".to_string()];
            return;
        }
        let out = Command::new(&self.paths.tasks_bin)
            .arg("board")
            .current_dir(&self.paths.root)
            .output();
        self.tasks_board = match out {
            Ok(o) => {
                let text = String::from_utf8_lossy(&o.stdout);
                text.lines().map(|l| l.to_string()).take(40).collect()
            }
            Err(_) => vec!["(tasks board unavailable)".to_string()],
        };
    }

    /// Lazy file tree of the selected project: only expanded dirs are walked.
    pub fn file_tree_rows(&self) -> Vec<TreeRow> {
        let Some(idx) = self.selected_project else { return Vec::new() };
        let root = self.projects[idx].clone();
        let mut rows = Vec::new();
        walk(&root, 0, &self.expanded_dirs, &mut rows);
        rows
    }
}

fn walk(dir: &Path, depth: usize, expanded: &std::collections::HashSet<PathBuf>, out: &mut Vec<TreeRow>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort_by(|a, b| {
        let ad = a.is_dir();
        let bd = b.is_dir();
        bd.cmp(&ad).then_with(|| a.file_name().cmp(&b.file_name()))
    });
    for path in entries {
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let is_dir = path.is_dir();
        if is_dir && SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        if name.starts_with('.') && is_dir {
            continue;
        }
        out.push(TreeRow { path: path.clone(), name, is_dir, depth });
        if is_dir && expanded.contains(&path) && depth < 12 {
            walk(&path, depth + 1, expanded, out);
        }
    }
}
