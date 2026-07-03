//! Hook configuration — `config/hooks.conf`, a flat INI-ish format (the repo's
//! zero-dep style, same family as smartagent.conf):
//!
//! ```text
//! [hook]
//! name    = no-rm-rf
//! event   = tool_call            # pi event: tool_call|user_prompt|session_start|tool_result|stop
//! matcher = bash                 # substring on tool name / '*' = always
//! command = hooks.d/no-rm-rf.sh  # exec'd argv[0] relative to repo root (no sh -c)
//! timeout = 10                   # seconds (default 30)
//! ```
//!
//! Blocks are additive (all matching hooks run, Codex-style). The command gets
//! the event payload as JSON on stdin — Claude Code's contract: exit 0 = allow
//! (stdout JSON may carry a decision), exit 2 = BLOCK (stderr = reason),
//! anything else = non-blocking warning.

use std::path::Path;

#[derive(Default, Clone)]
pub struct Hook {
    pub name: String,
    pub event: String,
    pub matcher: String,
    pub command: String,
    pub timeout_secs: u64,
}

pub const EVENTS: [&str; 5] = ["tool_call", "tool_result", "user_prompt", "session_start", "stop"];

pub fn parse(text: &str) -> Result<Vec<Hook>, String> {
    let mut hooks = Vec::new();
    let mut cur: Option<Hook> = None;
    for (ln, raw) in text.lines().enumerate() {
        let line = strip_inline_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line == "[hook]" {
            if let Some(h) = cur.take() {
                hooks.push(validate(h, ln)?);
            }
            cur = Some(Hook { timeout_secs: 30, matcher: "*".into(), ..Hook::default() });
            continue;
        }
        let Some(h) = cur.as_mut() else {
            return Err(format!("line {}: key outside a [hook] block", ln + 1));
        };
        let (k, v) = line.split_once('=').ok_or_else(|| format!("line {}: expected key = value", ln + 1))?;
        let v = v.trim().to_string();
        match k.trim() {
            "name" => h.name = v,
            "event" => h.event = v,
            "matcher" => h.matcher = v,
            "command" => h.command = v,
            "timeout" => h.timeout_secs = v.parse().map_err(|_| format!("line {}: bad timeout", ln + 1))?,
            other => return Err(format!("line {}: unknown key '{other}'", ln + 1)),
        }
    }
    if let Some(h) = cur.take() {
        hooks.push(validate(h, 0)?);
    }
    Ok(hooks)
}

fn strip_inline_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut prev_ws = true;
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == '#' && prev_ws => return &line[..i],
            None => {}
        }
        prev_ws = c.is_whitespace();
    }
    line
}

fn validate(h: Hook, ln: usize) -> Result<Hook, String> {
    if h.name.is_empty() || h.event.is_empty() || h.command.is_empty() {
        return Err(format!("hook ending near line {}: name, event, command are required", ln + 1));
    }
    if !EVENTS.contains(&h.event.as_str()) {
        return Err(format!("hook '{}': unknown event '{}' ({})", h.name, h.event, EVENTS.join("|")));
    }
    Ok(h)
}

pub fn load(path: &Path) -> Result<Vec<Hook>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    parse(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
}

/// A hook matches when its event equals the fired event and its matcher is
/// `*` or a case-insensitive substring of the subject (tool name, source…).
pub fn matches(h: &Hook, event: &str, subject: &str) -> bool {
    h.event == event && (h.matcher == "*" || subject.to_lowercase().contains(&h.matcher.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_blocks_and_defaults() {
        let text = "# comment\n[hook]\nname = a\nevent = tool_call\nmatcher = bash\ncommand = x.sh\n\n[hook]\nname = b\nevent = session_start\ncommand = y.sh\ntimeout = 5\n";
        let hs = parse(text).unwrap();
        assert_eq!(hs.len(), 2);
        assert_eq!(hs[0].timeout_secs, 30);
        assert_eq!(hs[1].matcher, "*");
        assert_eq!(hs[1].timeout_secs, 5);
    }

    #[test]
    fn rejects_unknown_event_and_missing_fields() {
        assert!(parse("[hook]\nname = a\nevent = nope\ncommand = x\n").is_err());
        assert!(parse("[hook]\nname = a\nevent = stop\n").is_err());
    }

    #[test]
    fn strips_trailing_inline_comments() {
        let text = "[hook]\nname = a # label\nevent = tool_call # event comment\nmatcher = bash#literal\ncommand = hooks.d/a.sh # path comment\ntimeout = 7 # seconds\n";
        let hs = parse(text).unwrap();
        assert_eq!(hs[0].name, "a");
        assert_eq!(hs[0].event, "tool_call");
        assert_eq!(hs[0].matcher, "bash#literal");
        assert_eq!(hs[0].command, "hooks.d/a.sh");
        assert_eq!(hs[0].timeout_secs, 7);
    }

    #[test]
    fn matching_rules() {
        let h = Hook { name: "x".into(), event: "tool_call".into(), matcher: "bash".into(), command: "c".into(), timeout_secs: 1 };
        assert!(matches(&h, "tool_call", "Bash"));
        assert!(!matches(&h, "tool_call", "read"));
        assert!(!matches(&h, "stop", "bash"));
    }
}
