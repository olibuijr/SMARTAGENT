//! Independent goal evaluator. A small, fast model reads the transcript and
//! decides whether the goal condition holds. It is deliberately NOT the model
//! that did the work — the whole point of /goal is that completion is judged by
//! a fresh evaluator, not the agent grading its own homework. It cannot call
//! tools; it judges only what the transcript demonstrates.

use crate::store::Goal;
use crate::transcript::Transcript;

const ROUTER_BASE: &str = "https://akurai-router.olibuijr.com/v1";

pub struct Verdict {
    pub met: bool,
    pub reason: String,
}

const SYSTEM: &str = "You are an INDEPENDENT goal verifier. You did NOT do the work. Your only job is to decide whether the stated goal condition is demonstrably met, judging ONLY the conversation transcript below. You cannot run tools or read files, so judge strictly on the evidence the transcript surfaces. Reply with exactly ONE JSON object and nothing else: {\"met\": true|false, \"reason\": \"<one short sentence>\"}. Set met=true ONLY when the transcript clearly demonstrates the condition holds; if the evidence is absent, partial, or you are unsure, set met=false.";

pub fn evaluate(goal: &Goal, tr: &Transcript, model: &str) -> Result<Verdict, String> {
    let key = router_key()?;
    let user = format!(
        "GOAL CONDITION:\n{}\n\nCONVERSATION TRANSCRIPT (most recent turns):\n{}\n\nHas the goal condition been met? Reply with the JSON object only.",
        goal.condition,
        if tr.text.is_empty() { "(transcript is empty)" } else { &tr.text }
    );
    let body = format!(
        r#"{{"model":"{}","temperature":0,"messages":[{{"role":"system","content":"{}"}},{{"role":"user","content":"{}"}}]}}"#,
        model,
        httpc::json::escape(SYSTEM),
        httpc::json::escape(&user)
    );
    let resp = httpc::client::Request::new("POST", &format!("{ROUTER_BASE}/chat/completions"))
        .header("Authorization", &format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .body(body.as_bytes())
        .timeout(90)
        .send()?;
    if resp.status != 200 {
        return Err(format!(
            "evaluator http {}: {}",
            resp.status,
            resp.text().unwrap_or_default()
        ));
    }
    let v = httpc::json::parse(&resp.text()?)?;
    let content = v
        .get("choices")
        .and_then(|c| c.as_arr())
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|x| x.as_str())
        .ok_or("evaluator returned no message content")?;
    parse_verdict(content)
}

/// Pull the `{met, reason}` object out of the model reply, tolerating prose or
/// code fences around it.
pub fn parse_verdict(content: &str) -> Result<Verdict, String> {
    let start = content.find('{').ok_or("evaluator reply had no JSON object")?;
    let end = content.rfind('}').ok_or("evaluator reply had no JSON object")?;
    if end < start {
        return Err("evaluator reply had malformed JSON".into());
    }
    let obj = &content[start..=end];
    let v = httpc::json::parse(obj)?;
    let reason = v
        .get("reason")
        .and_then(|x| x.as_str())
        .unwrap_or("no reason given")
        .trim()
        .to_string();
    Ok(Verdict {
        met: met_is_true(obj),
        reason,
    })
}

/// Robustly read the `met` value without depending on a JSON bool accessor:
/// find the `met` key and the first true/false token after it.
fn met_is_true(obj: &str) -> bool {
    let lower = obj.to_lowercase();
    let Some(k) = lower.find("\"met\"") else {
        return false;
    };
    let rest = &lower[k + 5..];
    let t = rest.find("true");
    let f = rest.find("false");
    match (t, f) {
        (Some(ti), Some(fi)) => ti < fi,
        (Some(_), None) => true,
        _ => false,
    }
}

fn router_key() -> Result<String, String> {
    let dir = std::env::var("PI_CODING_AGENT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| home().join(".pi").join("agent"));
    let p = dir.join("akurai-router.key");
    std::fs::read_to_string(&p)
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("no router key at {}: {e}", p.display()))
}

fn home() -> std::path::PathBuf {
    std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_met_true_false_and_fenced() {
        assert!(parse_verdict("{\"met\": true, \"reason\": \"all green\"}").unwrap().met);
        assert!(!parse_verdict("{\"met\": false, \"reason\": \"2 failing\"}").unwrap().met);
        // Prose and code fences around the object are tolerated.
        let v = parse_verdict("Sure:\n```json\n{\"met\": true, \"reason\":\"done\"}\n```").unwrap();
        assert!(v.met);
        assert_eq!(v.reason, "done");
    }

    #[test]
    fn defaults_to_not_met_on_garbage() {
        assert!(parse_verdict("no json here").is_err());
        assert!(!met_is_true("{\"reason\":\"?\"}"));
    }
}
