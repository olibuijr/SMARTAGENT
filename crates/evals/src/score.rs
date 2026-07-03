//! Scoring matchers and run comparison (regression diff).

use crate::store::Trace;

#[derive(Clone, Copy, PartialEq)]
pub enum Matcher {
    Exact,
    Contains,
    /// '*' is a wildcard for any run of characters.
    Wildcard,
}

impl Matcher {
    pub fn parse(s: &str) -> Result<Matcher, String> {
        match s {
            "exact" => Ok(Matcher::Exact),
            "contains" => Ok(Matcher::Contains),
            "regex-lite" | "wildcard" => Ok(Matcher::Wildcard),
            other => Err(format!("unknown matcher '{other}'")),
        }
    }

    pub fn matches(&self, output: &str, expected: &str) -> bool {
        match self {
            Matcher::Exact => output.trim() == expected.trim(),
            Matcher::Contains => output.contains(expected),
            Matcher::Wildcard => wildcard(output, expected),
        }
    }
}

fn wildcard(text: &str, pattern: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return text == pattern;
    }
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match text[pos..].find(part) {
            Some(idx) => {
                if i == 0 && idx != 0 {
                    return false; // no leading '*': must match at start
                }
                pos += idx + part.len();
            }
            None => return false,
        }
    }
    // No trailing '*': last part must reach the end.
    if let Some(last) = parts.last() {
        if !last.is_empty() && !text.ends_with(last) {
            return false;
        }
    }
    true
}

pub struct CaseScore {
    pub case: String,
    pub pass: bool,
}

pub fn score_run(traces: &[Trace], run: &str, matcher: Matcher) -> Vec<CaseScore> {
    let mut latest = std::collections::BTreeMap::<String, &Trace>::new();
    for t in traces.iter().filter(|t| t.run == run && t.expected.is_some()) {
        latest.insert(t.case.clone(), t);
    }
    latest
        .into_iter()
        .map(|(case, t)| CaseScore {
            case,
            pass: matcher.matches(&t.output, t.expected.as_ref().unwrap()),
        })
        .collect()
}

pub fn accuracy(scores: &[CaseScore]) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }
    scores.iter().filter(|s| s.pass).count() as f64 / scores.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matchers() {
        assert!(Matcher::Exact.matches("hi", " hi "));
        assert!(Matcher::Contains.matches("hello world", "lo wo"));
        assert!(Matcher::Wildcard.matches("abcdef", "ab*ef"));
        assert!(!Matcher::Wildcard.matches("abcdef", "ab*xy"));
        assert!(Matcher::Exact.matches("öæð", "öæð"));
    }

    #[test]
    fn wildcard_anchors() {
        assert!(wildcard("golf swing", "golf*"));
        assert!(!wildcard("my golf", "golf*"));
        assert!(wildcard("my golf", "*golf"));
    }

    #[test]
    fn score_run_uses_latest_trace_per_case() {
        let traces = vec![
            Trace { run: "r".into(), case: "c".into(), input: "old".into(), output: "bad".into(), expected: Some("good".into()), latency_ms: None, provenance: None },
            Trace { run: "r".into(), case: "c".into(), input: "new".into(), output: "good".into(), expected: Some("good".into()), latency_ms: None, provenance: None },
        ];
        let scores = score_run(&traces, "r", Matcher::Exact);
        assert_eq!(scores.len(), 1);
        assert_eq!(scores[0].case, "c");
        assert!(scores[0].pass);
    }
}
