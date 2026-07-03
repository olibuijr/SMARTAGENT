//! Read the most recent pi session transcript so the evaluator can judge what
//! the working agent actually surfaced. The evaluator only ever sees this text
//! — it cannot run tools — so completion is decided on demonstrated evidence.

use std::fs;
use std::path::{Path, PathBuf};

pub struct Transcript {
    pub text: String,
    /// modelId that did the work — lets the caller pick a DIFFERENT verifier.
    pub worker_model: String,
}

/// Newest transcript for one session id.
pub fn latest_for_session(
    sessions_dir: &Path,
    max_chars: usize,
    session: &str,
) -> Result<Transcript, String> {
    latest_matching(sessions_dir, max_chars, Some(session))
}

fn latest_matching(
    sessions_dir: &Path,
    max_chars: usize,
    session: Option<&str>,
) -> Result<Transcript, String> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(sessions_dir)
        .map_err(|e| format!("no sessions dir {}: {e}", sessions_dir.display()))?
        .flatten()
        .filter(|e| e.path().extension().map(|x| x == "jsonl").unwrap_or(false))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let path = files
        .into_iter()
        .map(|(_, p)| p)
        .find(|p| {
            session
                .map(|s| transcript_has_session(p, s))
                .unwrap_or(true)
        })
        .ok_or_else(|| format!("no session transcripts in {}", sessions_dir.display()))?;
    parse_file(&path, max_chars)
}

fn transcript_has_session(path: &Path, session: &str) -> bool {
    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };
    content.lines().take(8).any(|line| {
        httpc::json::parse(line)
            .ok()
            .and_then(|v| v.get("id").and_then(|x| x.as_str()).map(|id| id == session))
            .unwrap_or(false)
    })
}

/// Parse a specific transcript file (used by tests and `--transcript`).
pub fn parse_file(path: &Path, max_chars: usize) -> Result<Transcript, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut msgs: Vec<String> = Vec::new();
    let mut worker_model = String::new();
    for line in content.lines() {
        let Ok(v) = httpc::json::parse(line) else {
            continue;
        };
        match v.get("type").and_then(|x| x.as_str()) {
            Some("model_change") => {
                if let Some(m) = v.get("modelId").and_then(|x| x.as_str()) {
                    worker_model = m.to_string();
                }
            }
            Some("message") => {
                if let Some(m) = v.get("message") {
                    let role = m.get("role").and_then(|x| x.as_str()).unwrap_or("");
                    let text = message_text(m);
                    if !text.trim().is_empty() {
                        msgs.push(format!("[{}] {}", role.to_uppercase(), text.trim()));
                    }
                }
            }
            _ => {}
        }
    }
    // Keep the most recent messages, oldest-of-the-kept first, within budget.
    let mut kept: Vec<&String> = Vec::new();
    let mut used = 0usize;
    for m in msgs.iter().rev() {
        if used + m.len() + 2 > max_chars && !kept.is_empty() {
            break;
        }
        used += m.len() + 2;
        kept.push(m);
    }
    kept.reverse();
    Ok(Transcript {
        text: kept
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n\n"),
        worker_model,
    })
}

/// Content is either a plain string or an array of `{type:text, text:..}` blocks.
fn message_text(message: &httpc::json::Value) -> String {
    if let Some(s) = message.get("content").and_then(|x| x.as_str()) {
        return s.to_string();
    }
    let Some(arr) = message.get("content").and_then(|x| x.as_arr()) else {
        return String::new();
    };
    arr.iter()
        .filter_map(|b| b.get("text").and_then(|x| x.as_str()).map(str::to_string))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_roles_model_and_respects_budget() {
        let dir = std::env::temp_dir().join("goal-tx-test");
        let _ = fs::create_dir_all(&dir);
        let f = dir.join("s.jsonl");
        fs::write(
            &f,
            concat!(
                "{\"type\":\"session\",\"id\":\"x\"}\n",
                "{\"type\":\"model_change\",\"modelId\":\"codex/gpt-5.4-mini\"}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"do the thing\"}]}}\n",
                "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"all tests pass\"}]}}\n"
            ),
        )
        .unwrap();
        let t = parse_file(&f, 10_000).unwrap();
        assert_eq!(t.worker_model, "codex/gpt-5.4-mini");
        assert!(t.text.contains("[USER] do the thing"));
        assert!(t.text.contains("[ASSISTANT] all tests pass"));
        // Tiny budget keeps only the most recent message.
        let t2 = parse_file(&f, 30).unwrap();
        assert!(t2.text.contains("all tests pass"));
        assert!(!t2.text.contains("do the thing"));
    }
}
