//! Append-only JSONL event journal — the durability layer (Temporal concept:
//! state = replay of the event history). One JSON object per line; a torn
//! tail line (no trailing newline) is ignored on replay.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Job registered: id, cron expression, command.
    Scheduled { id: String, cron: String, cmd: String },
    /// Job removed.
    Removed { id: String },
    /// A run finished: id, fire time (unix), exit code, attempt (1 or 2).
    Ran { id: String, fire: i64, exit: i32, attempt: u8 },
}

impl Event {
    fn to_line(&self) -> String {
        // Hand-built JSON; ids/cmds are escaped.
        match self {
            Event::Scheduled { id, cron, cmd } => format!(
                r#"{{"ev":"scheduled","id":"{}","cron":"{}","cmd":"{}"}}"#,
                esc(id),
                esc(cron),
                esc(cmd)
            ),
            Event::Removed { id } => format!(r#"{{"ev":"removed","id":"{}"}}"#, esc(id)),
            Event::Ran { id, fire, exit, attempt } => format!(
                r#"{{"ev":"ran","id":"{}","fire":{fire},"exit":{exit},"attempt":{attempt}}}"#,
                esc(id)
            ),
        }
    }

    fn from_line(line: &str) -> Option<Event> {
        let get_str = |key: &str| -> Option<String> {
            let pat = format!("\"{key}\":\"");
            let start = line.find(&pat)? + pat.len();
            let mut out = String::new();
            let mut chars = line[start..].chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => match chars.next()? {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        other => out.push(other),
                    },
                    '"' => return Some(out),
                    c => out.push(c),
                }
            }
            None
        };
        let get_num = |key: &str| -> Option<i64> {
            let pat = format!("\"{key}\":");
            let start = line.find(&pat)? + pat.len();
            let end = line[start..]
                .find(|c: char| !c.is_ascii_digit() && c != '-')
                .map(|i| start + i)
                .unwrap_or(line.len());
            line[start..end].parse().ok()
        };
        match get_str("ev")?.as_str() {
            "scheduled" => Some(Event::Scheduled {
                id: get_str("id")?,
                cron: get_str("cron")?,
                cmd: get_str("cmd")?,
            }),
            "removed" => Some(Event::Removed { id: get_str("id")? }),
            "ran" => Some(Event::Ran {
                id: get_str("id")?,
                fire: get_num("fire")?,
                exit: get_num("exit")? as i32,
                attempt: get_num("attempt")? as u8,
            }),
            _ => None,
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\t', "\\t")
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub cron: String,
    pub cmd: String,
    /// Last fire time journaled as completed (any exit).
    pub last_fire: Option<i64>,
}

pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn new(path: &Path) -> Journal {
        Journal { path: path.to_path_buf() }
    }

    pub fn append(&self, ev: &Event) -> Result<(), String> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("open journal: {e}"))?;
        f.write_all(format!("{}\n", ev.to_line()).as_bytes())
            .map_err(|e| e.to_string())?;
        f.sync_data().map_err(|e| e.to_string())
    }

    /// Replay the full history into current state (live jobs + last runs).
    pub fn replay(&self) -> Result<HashMap<String, Job>, String> {
        let mut jobs: HashMap<String, Job> = HashMap::new();
        let mut content = String::new();
        match std::fs::File::open(&self.path) {
            Ok(mut f) => {
                f.read_to_string(&mut content).map_err(|e| e.to_string())?;
            }
            Err(_) => return Ok(jobs), // no journal yet
        }
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match Event::from_line(line) {
                Some(Event::Scheduled { id, cron, cmd }) => {
                    jobs.insert(id.clone(), Job { id, cron, cmd, last_fire: None });
                }
                Some(Event::Removed { id }) => {
                    jobs.remove(&id);
                }
                Some(Event::Ran { id, fire, .. }) => {
                    if let Some(j) = jobs.get_mut(&id) {
                        j.last_fire = Some(j.last_fire.map_or(fire, |p| p.max(fire)));
                    }
                }
                None => continue, // torn/corrupt line — skip (crash tolerance)
            }
        }
        Ok(jobs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        // In-repo scratch (never /tmp): <workspace>/target/test-scratch/
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("journal-{name}.jsonl"))
    }

    #[test]
    fn roundtrip_and_replay() {
        let p = scratch("rt");
        let _ = std::fs::remove_file(&p);
        let j = Journal::new(&p);
        j.append(&Event::Scheduled { id: "a".into(), cron: "* * * * *".into(), cmd: "echo \"hi\"".into() }).unwrap();
        j.append(&Event::Ran { id: "a".into(), fire: 100, exit: 0, attempt: 1 }).unwrap();
        j.append(&Event::Scheduled { id: "b".into(), cron: "0 0 * * *".into(), cmd: "true".into() }).unwrap();
        j.append(&Event::Removed { id: "b".into() }).unwrap();
        let state = j.replay().unwrap();
        assert_eq!(state.len(), 1);
        let a = &state["a"];
        assert_eq!(a.cmd, "echo \"hi\"");
        assert_eq!(a.last_fire, Some(100));
    }

    #[test]
    fn torn_tail_ignored() {
        let p = scratch("torn");
        let _ = std::fs::remove_file(&p);
        let j = Journal::new(&p);
        j.append(&Event::Scheduled { id: "a".into(), cron: "* * * * *".into(), cmd: "true".into() }).unwrap();
        // Torn write: partial line, no newline.
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(br#"{"ev":"ran","id":"a","fi"#).unwrap();
        drop(f);
        let state = j.replay().unwrap();
        assert_eq!(state.len(), 1);
        assert_eq!(state["a"].last_fire, None);
    }
}
