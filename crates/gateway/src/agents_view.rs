//! Agent-view rendering: the `gateway agents` TSV (name, state, doing, role,
//! tokens, tool-chain, last-words) and today's token totals from the medvitund
//! table. Split out of server.rs to keep every file under 1000 lines.

use std::io::Write;
use std::os::unix::net::UnixStream;

use httpc::json;

use crate::roster::display_role;
use crate::server::Agents;

pub(crate) fn last_activity(name: &str) -> (String, String) {
    let path = semdb::config::Config::load()
        .data_dir()
        .join("gateway")
        .join(format!("{name}.log"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (String::new(), String::new());
    };
    let tail: String = text
        .chars()
        .rev()
        .take(1500)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let tools: Vec<&str> = tail
        .lines()
        .filter_map(|l| l.trim().strip_prefix("⚙ "))
        .collect();
    let chain = tools
        .iter()
        .rev()
        .take(3)
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join("→");
    let words_raw = tail
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("⚙ ") && *l != "---")
        .last()
        .unwrap_or("");
    // keep the freshest words: take the LAST ≤56 chars, then trim to a word start
    let w: Vec<&str> = words_raw.split_whitespace().collect();
    let mut words = String::new();
    for word in w.iter().rev() {
        if words.chars().count() + word.chars().count() + 1 > 56 {
            break;
        }
        if words.is_empty() {
            words = (*word).to_string();
        } else {
            words = format!("{word} {words}");
        }
    }
    (chain, words)
}

#[derive(Default)]
pub(crate) struct TokenTotals {
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) cache_read: u64,
    pub(crate) cache_write: u64,
}

impl TokenTotals {
    pub(crate) fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

pub(crate) fn status_line(
    agent: &str,
    busy: bool,
    last: &str,
    queued: bool,
    doing: &str,
    tokens: u64,
) -> String {
    format!(
        "agent {agent}: {} | last beat {last} | queued beat: {queued} | doing: {doing} | tokens today: {tokens}",
        if busy { "working" } else { "idle" }
    )
}

fn agent_tsv_line(
    name: &str,
    state: &str,
    own_task: &str,
    role: &str,
    busy_secs: u64,
    tokens: u64,
    tools: &str,
    words: &str,
) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        name, state, own_task, role, busy_secs, tokens, tools, words
    )
}

pub(crate) fn write_agents(write_side: &mut UnixStream, agents: &Agents) {
    for agent in agents.values() {
        let busy = agent.child.lock().unwrap().is_busy();
        let own_task = agent.beat.lock().unwrap().doing_short_for(&agent.name);
        let state = if busy { "working" } else { "idle" };
        let busy_secs = agent
            .busy_since
            .lock()
            .unwrap()
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        let (tools, words) = last_activity(&agent.name);
        let tokens = agent.beat.lock().unwrap().tokens_today(Some(&agent.name));
        let data = agent_tsv_line(
            &json::escape(&agent.name),
            state,
            &json::escape(&own_task),
            &display_role(&agent.name),
            busy_secs,
            tokens.total(),
            &json::escape(&tools),
            &json::escape(&words),
        );
        let line = format!("{{\"ev\":\"info\",\"data\":\"{data}\"}}\n");
        let _ = write_side.write_all(line.as_bytes());
    }
    let _ = write_side.write_all(b"{\"ev\":\"done\"}\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_line_reports_selected_agent_tokens() {
        let line = status_line(
            "builder",
            false,
            "2026-07-02 23:00:00Z",
            true,
            "T-9 build",
            123,
        );
        assert!(line.contains("agent builder: idle"));
        assert!(line.contains("queued beat: true"));
        assert!(line.contains("doing: T-9 build"));
        assert!(line.contains("tokens today: 123"));
        assert!(!line.contains("2889493"));
    }
    #[test]
    fn token_totals_sum_per_agent_to_fleet_total() {
        let ada = TokenTotals {
            input: 1,
            output: 2,
            cache_read: 3,
            cache_write: 4,
        };
        let builder = TokenTotals {
            input: 10,
            output: 20,
            cache_read: 30,
            cache_write: 40,
        };
        let fleet = TokenTotals {
            input: ada.input + builder.input,
            output: ada.output + builder.output,
            cache_read: ada.cache_read + builder.cache_read,
            cache_write: ada.cache_write + builder.cache_write,
        };
        assert_eq!(ada.total() + builder.total(), fleet.total());
    }
    #[test]
    fn agent_tsv_line_has_eight_fields_in_panel_order() {
        let line = agent_tsv_line(
            "ada",
            "working",
            "T-1 build @ada",
            "Builder",
            42,
            99,
            "tasks→bash",
            "last words",
        );
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 8);
        assert_eq!(
            fields,
            vec![
                "ada",
                "working",
                "T-1 build @ada",
                "Builder",
                "42",
                "99",
                "tasks→bash",
                "last words"
            ]
        );
    }
}
