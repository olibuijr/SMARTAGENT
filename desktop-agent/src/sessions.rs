use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use httpc::json::{parse, Value};

use crate::jsonw::{truncate_chars, write as write_json};

const LIST_READ_LIMIT: usize = 4 * 1024 * 1024;
const TRANSCRIPT_READ_LIMIT: usize = 16 * 1024 * 1024;
const TOOL_OUTPUT_MAX_CHARS: usize = 4096;

pub struct SessionMeta {
    pub path: PathBuf,
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub mtime_secs: u64,
    pub project: Option<String>,
}

pub fn list_sessions(sessions_dir: &Path, workspaces_dir: &Path) -> Vec<SessionMeta> {
    let entries = match fs::read_dir(sessions_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut sessions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) if metadata.is_file() => metadata,
            _ => continue,
        };
        let mtime_secs = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);

        let text = match read_prefix(&path, LIST_READ_LIMIT) {
            Ok((text, _)) => text,
            Err(_) => continue,
        };
        if text.trim().is_empty() {
            continue;
        }

        if let Some(meta) = parse_session_meta(&path, &text, mtime_secs, workspaces_dir) {
            sessions.push(meta);
        }
    }

    sessions.sort_by(|a, b| {
        b.mtime_secs
            .cmp(&a.mtime_secs)
            .then_with(|| a.path.cmp(&b.path))
    });
    sessions
}

pub fn relative_time(now_secs: u64, mtime_secs: u64) -> String {
    let diff = now_secs.saturating_sub(mtime_secs);
    let minute = 60;
    let hour = 60 * minute;
    let day = 24 * hour;

    if mtime_secs >= now_secs || diff < minute {
        "now".to_string()
    } else if diff < hour {
        format!("{}m ago", diff / minute)
    } else if diff < day {
        format!("{}h ago", diff / hour)
    } else if diff < 2 * day {
        "yesterday".to_string()
    } else if diff < 7 * day {
        format!("{}d ago", diff / day)
    } else {
        let days = (mtime_secs / day) as i64;
        let (year, month, day) = civil_from_days(days);
        format!("{year:04}-{month:02}-{day:02}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReplayItem {
    User(String),
    AssistantText(String),
    Thinking(String),
    Tool {
        id: String,
        name: String,
        args_summary: String,
        args_full: String,
        output: String,
        is_error: bool,
    },
    System(String),
}

pub fn load_transcript(path: &Path) -> Vec<ReplayItem> {
    let (text, truncated) = match read_prefix(path, TRANSCRIPT_READ_LIMIT) {
        Ok(result) => result,
        Err(_) => return Vec::new(),
    };

    let mut items = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let value = match parse(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        match field_str(&value, "type") {
            Some("message") => push_message(&value, &mut items),
            Some("compaction") => items.push(ReplayItem::System("context compacted".to_string())),
            _ => {}
        }
    }

    if truncated {
        items.push(ReplayItem::System("… transcript truncated".to_string()));
    }

    items
}

pub fn args_summary(args: &Value) -> String {
    let keys = [
        "command",
        "path",
        "file_path",
        "filePath",
        "query",
        "url",
        "action",
        "name",
        "message",
        "text",
    ];

    for key in keys {
        if let Some(text) = args.get(key).and_then(Value::as_str) {
            return truncate_chars(text, 80);
        }
    }

    truncate_chars(&write_json(args), 80)
}

fn parse_session_meta(
    path: &Path,
    text: &str,
    mtime_secs: u64,
    workspaces_dir: &Path,
) -> Option<SessionMeta> {
    let file_stem = path.file_stem()?.to_str()?.to_string();
    let mut lines = text.lines();
    let header = parse(lines.next()?.trim()).ok()?;

    if field_str(&header, "type") != Some("session") {
        return None;
    }
    if header.get("version").and_then(Value::as_f64) != Some(3.0) {
        return None;
    }

    let id = field_str(&header, "id").unwrap_or(&file_stem).to_string();
    let cwd = field_str(&header, "cwd").unwrap_or("").to_string();
    let mut last_title = None;
    let mut first_user = None;

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value = match parse(line) {
            Ok(value) => value,
            Err(_) => continue,
        };

        match field_str(&value, "type") {
            Some("session_info") => {
                if let Some(name) = field_str(&value, "name") {
                    last_title = Some(name.to_string());
                }
            }
            Some("message") if first_user.is_none() => {
                if let Some(text) = user_message_text(&value) {
                    first_user = Some(truncate_chars(&text, 60));
                }
            }
            _ => {}
        }
    }

    let title = last_title
        .or(first_user)
        .unwrap_or_else(|| file_stem.clone());
    let project = project_from_cwd(&cwd, workspaces_dir);

    Some(SessionMeta {
        path: path.to_path_buf(),
        id,
        title,
        cwd,
        mtime_secs,
        project,
    })
}

fn push_message(value: &Value, items: &mut Vec<ReplayItem>) {
    let message = match value.get("message") {
        Some(message) => message,
        None => return,
    };

    match field_str(message, "role") {
        Some("user") => {
            let text = text_blocks(message.get("content"));
            if !text.is_empty() {
                items.push(ReplayItem::User(text));
            }
        }
        Some("assistant") => push_assistant_message(message, items),
        Some("toolResult") => push_tool_result(message, items),
        _ => {}
    }
}

fn push_assistant_message(message: &Value, items: &mut Vec<ReplayItem>) {
    let blocks = match message.get("content").and_then(Value::as_arr) {
        Some(blocks) => blocks,
        None => return,
    };

    for block in blocks {
        match field_str(block, "type") {
            Some("text") => {
                if let Some(text) = field_str(block, "text") {
                    items.push(ReplayItem::AssistantText(text.to_string()));
                }
            }
            Some("thinking") => {
                if let Some(thinking) = field_str(block, "thinking") {
                    items.push(ReplayItem::Thinking(thinking.to_string()));
                }
            }
            Some("toolCall") => push_tool_call(block, items),
            _ => {}
        }
    }
}

fn push_tool_call(block: &Value, items: &mut Vec<ReplayItem>) {
    let empty = Value::Obj(Vec::new());
    let args = block.get("arguments").unwrap_or(&empty);
    items.push(ReplayItem::Tool {
        id: field_str(block, "id").unwrap_or("").to_string(),
        name: field_str(block, "name").unwrap_or("").to_string(),
        args_summary: args_summary(args),
        args_full: write_json(args),
        output: String::new(),
        is_error: false,
    });
}

fn push_tool_result(message: &Value, items: &mut Vec<ReplayItem>) {
    let id = field_str(message, "toolCallId").unwrap_or("").to_string();
    let name = field_str(message, "toolName").unwrap_or("").to_string();
    let output = truncate_tool_output(&text_blocks(message.get("content")));
    let is_error = field_bool(message, "isError").unwrap_or(false);

    if let Some(item) = items.iter_mut().rev().find(|item| match item {
        ReplayItem::Tool { id: tool_id, .. } => tool_id == &id,
        _ => false,
    }) {
        if let ReplayItem::Tool {
            name: tool_name,
            output: tool_output,
            is_error: tool_is_error,
            ..
        } = item
        {
            if tool_name.is_empty() {
                *tool_name = name;
            }
            *tool_output = output;
            *tool_is_error = is_error;
        }
        return;
    }

    items.push(ReplayItem::Tool {
        id,
        name,
        args_summary: String::new(),
        args_full: String::new(),
        output,
        is_error,
    });
}

fn user_message_text(value: &Value) -> Option<String> {
    let message = value.get("message")?;
    if field_str(message, "role") != Some("user") {
        return None;
    }
    let text = text_blocks(message.get("content"));
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn text_blocks(content: Option<&Value>) -> String {
    let mut out = String::new();
    let blocks = match content.and_then(Value::as_arr) {
        Some(blocks) => blocks,
        None => return out,
    };

    for block in blocks {
        if field_str(block, "type") == Some("text") {
            if let Some(text) = field_str(block, "text") {
                out.push_str(text);
            }
        }
    }

    out
}

fn truncate_tool_output(text: &str) -> String {
    if text.chars().count() <= TOOL_OUTPUT_MAX_CHARS {
        return text.to_string();
    }

    let mut out = text.chars().take(TOOL_OUTPUT_MAX_CHARS).collect::<String>();
    out.push_str(&format!("… ({} bytes total)", text.len()));
    out
}

fn field_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn field_bool(value: &Value, key: &str) -> Option<bool> {
    match value.get(key) {
        Some(Value::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn project_from_cwd(cwd: &str, workspaces_dir: &Path) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }

    let rel = Path::new(cwd).strip_prefix(workspaces_dir).ok()?;
    for component in rel.components() {
        if let Component::Normal(name) = component {
            return name.to_str().map(|name| name.to_string());
        }
    }

    None
}

fn read_prefix(path: &Path, limit: usize) -> std::io::Result<(String, bool)> {
    let metadata = fs::metadata(path)?;
    let truncated = metadata.len() > limit as u64;
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.by_ref().take(limit as u64).read_to_end(&mut bytes)?;

    if truncated {
        if let Some(pos) = bytes.iter().rposition(|byte| *byte == b'\n') {
            bytes.truncate(pos + 1);
        } else {
            bytes.clear();
        }
    }

    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn scratch_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/test-scratch")
            .join(format!("sessions-{name}-{nonce}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(path: &Path, id: &str, title: &str, cwd: &Path, output: &str) {
        let body = format!(
            "{{\"type\":\"session\",\"version\":3,\"id\":\"{id}\",\"timestamp\":\"2026-07-02T00:00:00Z\",\"cwd\":\"{}\"}}\n\
             {{\"type\":\"session_info\",\"name\":\"old title\"}}\n\
             {{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":\"please run tasks\"}},{{\"type\":\"image\",\"source\":\"skip\"}}]}}}}\n\
             {{\"type\":\"session_info\",\"name\":\"{title}\"}}\n\
             {{\"type\":\"message\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"running\"}},{{\"type\":\"toolCall\",\"id\":\"call_1\",\"name\":\"tasks\",\"arguments\":{{\"command\":\"do it\",\"query\":\"ignore\"}}}}]}}}}\n\
             {{\"type\":\"message\",\"message\":{{\"role\":\"toolResult\",\"toolCallId\":\"call_1\",\"toolName\":\"tasks\",\"content\":[{{\"type\":\"text\",\"text\":\"{output}\"}}],\"isError\":false}}}}\n",
            cwd.display()
        );
        fs::write(path, body).unwrap();
    }

    #[test]
    fn lists_sessions_and_pairs_tool_result() {
        let root = scratch_dir("roundtrip");
        let sessions_dir = root.join("sessions");
        let workspaces_dir = root.join("workspaces");
        fs::create_dir_all(&sessions_dir).unwrap();
        fs::create_dir_all(&workspaces_dir).unwrap();

        let older_cwd = workspaces_dir.join("older-project");
        let newer_cwd = workspaces_dir.join("newer-project").join("repo");
        let older = sessions_dir.join("2026-07-01T00-00-00_old.jsonl");
        let newer = sessions_dir.join("2026-07-02T00-00-00_new.jsonl");

        write_file(&older, "old", "Older title", &older_cwd, "old output");
        std::thread::sleep(Duration::from_millis(1_100));
        write_file(&newer, "new", "Newer title", &newer_cwd, "tool output");

        let listed = list_sessions(&sessions_dir, &workspaces_dir);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "new");
        assert_eq!(listed[0].title, "Newer title");
        assert_eq!(listed[0].cwd, newer_cwd.display().to_string());
        assert_eq!(listed[0].project.as_deref(), Some("newer-project"));

        let transcript = load_transcript(&newer);
        let tool = transcript
            .iter()
            .find_map(|item| match item {
                ReplayItem::Tool {
                    id,
                    name,
                    args_summary,
                    output,
                    is_error,
                    ..
                } if id == "call_1" => Some((name, args_summary, output, is_error)),
                _ => None,
            })
            .unwrap();
        assert_eq!(tool.0, "tasks");
        assert_eq!(tool.1, "do it");
        assert_eq!(tool.2, "tool output");
        assert!(!tool.3);
    }

    #[test]
    fn relative_time_uses_expected_buckets() {
        let now = 10 * 24 * 60 * 60;
        assert_eq!(relative_time(now, now), "now");
        assert_eq!(relative_time(now, now - 5 * 60), "5m ago");
        assert_eq!(relative_time(now, now - 2 * 60 * 60), "2h ago");
        assert_eq!(relative_time(now, now - 25 * 60 * 60), "yesterday");
        assert_eq!(relative_time(now, now - 3 * 24 * 60 * 60), "3d ago");
        assert_eq!(relative_time(now, 0), "1970-01-01");
    }

    #[test]
    fn args_summary_prefers_named_string_keys() {
        let args = Value::Obj(vec![
            ("query".to_string(), Value::Str("later".to_string())),
            ("command".to_string(), Value::Str("first".to_string())),
        ]);
        assert_eq!(args_summary(&args), "first");

        let fallback = Value::Obj(vec![("count".to_string(), Value::Num(3.0))]);
        assert_eq!(args_summary(&fallback), "{\"count\":3}");
    }
}
