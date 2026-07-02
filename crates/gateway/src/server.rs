//! The gateway daemon: owns pi rpc children, accepts unix-socket clients,
//! broadcasts agent output, runs heartbeats, and writes transcripts.
//!
//! Client wire protocol (line JSON, both directions):
//!   → {"op":"send"|"steer"|"attach"|"status"|"agents"|"stop","agent"?:s,"message"?:s}
//!   ← {"ev":"text","data":s} {"ev":"info","data":s} {"ev":"done"}

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use httpc::json::{self, Value};

use crate::beat::Beat;
use crate::child::{Event, PiChild};

type Clients = Arc<Mutex<Vec<UnixStream>>>;
type Agents = Arc<BTreeMap<String, Arc<AgentRuntime>>>;

struct Flags {
    agents: Vec<String>,
    heartbeat_secs: u64,
    autonomous: bool,
}

struct AgentRuntime {
    name: String,
    child: Arc<Mutex<PiChild>>,
    clients: Clients,
    beat: Arc<Mutex<Beat>>,
    queued_beat: Arc<Mutex<Option<String>>>,
}

fn parse_flags(args: &[String]) -> Flags {
    let cfg = semdb::config::Config::load();
    let mut agents = cfg
        .resolve("gateway_agents", "SMARTAGENT_GATEWAY_AGENTS", None)
        .map(|v| split_agents(&v))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| {
            vec![cfg
                .resolve("gateway_agent", "SMARTAGENT_GATEWAY_AGENT", None)
                .unwrap_or_else(|| "main".into())]
        });
    let mut f = Flags {
        agents: Vec::new(),
        heartbeat_secs: cfg
            .resolve("heartbeat_secs", "SMARTAGENT_HEARTBEAT_SECS", None)
            .and_then(|v| v.parse().ok())
            .unwrap_or(120),
        autonomous: false,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--agent" => {
                if let Some(v) = it.next() {
                    f.agents.push(v.clone());
                }
            }
            "--agents" => {
                if let Some(v) = it.next() {
                    f.agents.extend(split_agents(v));
                }
            }
            "--heartbeat-secs" => {
                f.heartbeat_secs = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(f.heartbeat_secs)
            }
            "--autonomous" => f.autonomous = true,
            _ => {}
        }
    }
    if !f.agents.is_empty() {
        agents = f.agents.clone();
    }
    f.agents = dedupe_agents(agents);
    f
}

fn split_agents(v: &str) -> Vec<String> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

fn dedupe_agents(agents: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for a in agents {
        if !out.iter().any(|x| x == &a) {
            out.push(a);
        }
    }
    if out.is_empty() {
        out.push("main".into());
    }
    out
}

pub fn serve(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args);
    let cfg = semdb::config::Config::load();
    let data_dir = cfg.data_dir();
    let repo_root = data_dir
        .parent()
        .ok_or("cannot resolve repo root from data_dir")?
        .to_path_buf();
    let sock = crate::socket_path();

    if sock.exists() {
        if UnixStream::connect(&sock).is_ok() {
            return Err(format!("gateway already running on {}", sock.display()));
        }
        let _ = std::fs::remove_file(&sock);
    }
    if let Some(dir) = sock.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let listener =
        UnixListener::bind(&sock).map_err(|e| format!("bind {}: {e}", sock.display()))?;

    let mut map = BTreeMap::new();
    for name in &flags.agents {
        map.insert(
            name.clone(),
            Arc::new(spawn_agent(
                name,
                &repo_root,
                &data_dir,
                flags.heartbeat_secs,
                flags.autonomous,
            )?),
        );
    }
    let agents: Agents = Arc::new(map);
    eprintln!(
        "[gateway] agents [{}] up — socket {}, heartbeat {}s{}",
        flags.agents.join(","),
        sock.display(),
        flags.heartbeat_secs,
        if flags.autonomous { ", AUTONOMOUS" } else { "" }
    );

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let agents = agents.clone();
        std::thread::spawn(move || handle_client(stream, agents));
    }
    Ok(())
}

fn spawn_agent(
    name: &str,
    repo_root: &std::path::Path,
    data_dir: &std::path::Path,
    heartbeat_secs: u64,
    autonomous: bool,
) -> Result<AgentRuntime, String> {
    let mut child = PiChild::spawn(repo_root, name)?;
    let clients: Clients = Arc::new(Mutex::new(Vec::new()));
    let beat = Beat::new(repo_root, data_dir);
    let queued_beat = Mutex::new(None);
    let transcript = data_dir.join("gateway").join(format!("{name}.log"));
    if let Some(d) = transcript.parent() {
        let _ = std::fs::create_dir_all(d);
    }

    let events = std::mem::replace(&mut child.events, std::sync::mpsc::channel().1);
    let child = Arc::new(Mutex::new(child));
    let beat = Arc::new(Mutex::new(beat));
    let queued_beat = Arc::new(queued_beat);
    let runtime = AgentRuntime {
        name: name.to_string(),
        child: child.clone(),
        clients: clients.clone(),
        beat: beat.clone(),
        queued_beat: queued_beat.clone(),
    };
    start_event_pump(
        name.to_string(),
        clients,
        data_dir.to_path_buf(),
        transcript,
        events,
        beat.clone(),
        child.clone(),
    );
    let prompt_gate = Arc::new(Mutex::new(PromptGate::default()));
    if autonomous {
        start_work_chaser(name.to_string(), child.clone(), beat.clone(), prompt_gate.clone());
    }
    start_heartbeat(
        name.to_string(),
        child,
        beat,
        queued_beat,
        heartbeat_secs,
        autonomous,
        prompt_gate,
    );
    Ok(runtime)
}

fn start_event_pump(
    agent: String,
    clients: Clients,
    _data_dir: std::path::PathBuf,
    tpath: std::path::PathBuf,
    events: std::sync::mpsc::Receiver<Event>,
    beat: Arc<Mutex<Beat>>,
    child: Arc<Mutex<PiChild>>,
) {
    std::thread::spawn(move || {
        let mut empty_turns = 0u32;
        for ev in events.iter() {
            match ev {
                Event::Text(t) => {
                    append(&tpath, &t);
                    broadcast(
                        &clients,
                        &format!("{{\"ev\":\"text\",\"data\":\"{}\"}}\n", json::escape(&t)),
                    );
                }
                Event::State(b) => {
                    broadcast(
                        &clients,
                        &format!(
                            "{{\"ev\":\"info\",\"data\":\"agent {}\"}}\n",
                            if b { "working…" } else { "idle" }
                        ),
                    );
                    if !b {
                        broadcast(&clients, "{\"ev\":\"done\"}\n");
                    }
                }
                Event::TurnEnd(text, usage) => {
                    append(&tpath, "\n---\n");
                    let action = mute_action(empty_turns, &text, usage);
                    if action == MuteAction::Clear {
                        empty_turns = 0;
                    } else {
                        empty_turns += 1;
                    }
                    beat.lock().unwrap().log_turn(&agent, &text, usage);
                    match action {
                        MuteAction::Clear => {}
                        MuteAction::Observe => {}
                        MuteAction::CompactRetry => {
                            beat.lock().unwrap().log(&agent, "incident", "compact-retry", "mute turn: empty content and zero token usage; compacting session then retrying next beat");
                            let _ = child.lock().unwrap().command("compact", None);
                            broadcast(&clients, "{\"ev\":\"info\",\"data\":\"mute session detected — compact retry\"}\n");
                        }
                        MuteAction::Rotate => {
                            let note = "Continuity note: the previous gateway RPC session went mute (consecutive empty turns with 0 token usage), likely from rejected resume state or context ceiling. Continue the current board task using the live board/workflow state; one task at a time.";
                            beat.lock().unwrap().log(&agent, "incident", "rotate-session", "mute persisted after compact retry; archived old rpc session and started fresh session with continuity note");
                            broadcast(&clients, "{\"ev\":\"info\",\"data\":\"mute session persisted — rotating session\"}\n");
                            let _ = child.lock().unwrap().rotate_fresh(note);
                        }
                    }
                }
                Event::Tool(name) => {
                    append(&tpath, &format!("\n⚙ {name}\n"));
                    broadcast(
                        &clients,
                        &format!(
                            "{{\"ev\":\"info\",\"data\":\"⚙ {}\"}}\n",
                            json::escape(&name)
                        ),
                    );
                }
                Event::Exited(code) => {
                    broadcast(
                        &clients,
                        &format!("{{\"ev\":\"info\",\"data\":\"pi child exited ({code}) — agent stopped\"}}\n"),
                    );
                }
            }
        }
    });
}

fn start_heartbeat(
    agent: String,
    child: Arc<Mutex<PiChild>>,
    beat: Arc<Mutex<Beat>>,
    queued: Arc<Mutex<Option<String>>>,
    heartbeat_secs: u64,
    autonomous: bool,
    prompt_gate: Arc<Mutex<PromptGate>>,
) {
    let period = Duration::from_secs(heartbeat_secs.max(10));
    std::thread::spawn(move || {
        // T-79: startup beat ~10s after serve — a restart must not cost a
        // full period of dead air before in-doing work resumes.
        let mut next = Duration::from_secs(10);
        loop {
            std::thread::sleep(next);
            next = period;
            let busy = child.lock().unwrap().is_busy();
            let text = beat.lock().unwrap().compose(busy);
            beat.lock()
                .unwrap()
                .log(&agent, "beat", if busy { "busy" } else { "idle" }, &text);
            match heartbeat_action(busy, autonomous, beat.lock().unwrap().has_autonomous_work()) {
                HeartbeatAction::Steer => {
                    let _ = child.lock().unwrap().command("steer", Some(&text));
                }
                HeartbeatAction::Prompt => {
                    if prompt_gate.lock().unwrap().allow() {
                        let auto = autonomous_prompt(&text);
                        let _ = child.lock().unwrap().command("prompt", Some(&auto));
                    }
                }
                HeartbeatAction::Queue => *queued.lock().unwrap() = Some(text),
            }
        }
    });
}

fn autonomous_prompt(text: &str) -> String {
    format!(
        "{text}\nAUTONOMOUS MODE — act now, do not end the turn without one of these: (a) a task YOU pulled earlier is in doing → continue it; (b) otherwise, ready has an unclaimed item → pull the highest-priority one; (c) otherwise → load the Triage skill and promote exactly one UNCLAIMED backlog task by p1→p2→p3, oldest, then smallest id, and pull it. Tasks in doing/review that belong to OTHER agents are never a reason to stay idle — skip them and take unclaimed backlog work. If the backlog is truly empty of unclaimed tasks, say so in one sentence — never answer with an empty turn. One task in your doing, maximum."
    )
}

/// Shared cooldown so the heartbeat and the work-chaser never double-prompt:
/// at most one autonomous prompt per 20s per agent.
#[derive(Default)]
struct PromptGate {
    last: Option<std::time::Instant>,
}

impl PromptGate {
    fn allow(&mut self) -> bool {
        let ok = self.last.map(|t| t.elapsed() >= Duration::from_secs(20)).unwrap_or(true);
        if ok {
            self.last = Some(std::time::Instant::now());
        }
        ok
    }
}

/// Work chaser (throughput): the heartbeat alone leaves an idle gap of up to
/// a full period between finishing one task and pulling the next. This thread
/// watches for the busy→idle transition and re-prompts IMMEDIATELY when the
/// board still has unclaimed work — tasks chain back-to-back instead of
/// waiting out the beat.
fn start_work_chaser(
    agent: String,
    child: Arc<Mutex<PiChild>>,
    beat: Arc<Mutex<Beat>>,
    prompt_gate: Arc<Mutex<PromptGate>>,
) {
    std::thread::spawn(move || {
        let mut prev_busy = false;
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let busy = child.lock().unwrap().is_busy();
            let finished_turn = prev_busy && !busy;
            prev_busy = busy;
            if !finished_turn {
                continue;
            }
            let (has_work, text) = {
                let mut b = beat.lock().unwrap();
                let w = b.has_autonomous_work();
                (w, b.compose(false))
            };
            if has_work && prompt_gate.lock().unwrap().allow() {
                beat.lock().unwrap().log(&agent, "chase", "idle", "work-chaser: immediate next-task prompt");
                let auto = autonomous_prompt(&text);
                let _ = child.lock().unwrap().command("prompt", Some(&auto));
            }
        }
    });
}

#[derive(Debug, PartialEq, Eq)]
enum HeartbeatAction {
    Steer,
    Prompt,
    Queue,
}

fn heartbeat_action(busy: bool, autonomous: bool, has_work: bool) -> HeartbeatAction {
    if busy {
        HeartbeatAction::Steer
    } else if autonomous && has_work {
        HeartbeatAction::Prompt
    } else {
        HeartbeatAction::Queue
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MuteAction {
    Clear,
    Observe,
    CompactRetry,
    Rotate,
}

fn mute_action(previous_empty_turns: u32, text: &str, usage: crate::child::Usage) -> MuteAction {
    let empty = text.trim().is_empty()
        && usage.input == 0
        && usage.output == 0
        && usage.cache_read == 0
        && usage.cache_write == 0;
    if !empty {
        return MuteAction::Clear;
    }
    match previous_empty_turns + 1 {
        0 | 1 => MuteAction::Observe,
        2 => MuteAction::CompactRetry,
        _ => MuteAction::Rotate,
    }
}

fn handle_client(stream: UnixStream, agents: Agents) {
    let reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut write_side = stream;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(v) = json::parse(&line) else { continue };
        let op = v.get("op").and_then(Value::as_str).unwrap_or("");
        let msg = v.get("message").and_then(Value::as_str).unwrap_or("");
        let requested = v.get("agent").and_then(Value::as_str).unwrap_or("");
        match op {
            "agents" => write_agents(&mut write_side, &agents),
            "send" | "steer" | "attach" | "status" | "stop" => {
                let Some(agent) = select_agent(&agents, requested) else {
                    write_info_done(&mut write_side, &format!("unknown agent {requested}"));
                    continue;
                };
                handle_agent_op(op, msg, agent, &mut write_side);
            }
            _ => write_info_done(&mut write_side, "unknown op"),
        }
    }
}

fn select_agent(agents: &Agents, requested: &str) -> Option<Arc<AgentRuntime>> {
    if requested == "*" {
        return agents.values().next().cloned();
    }
    if !requested.is_empty() {
        return agents.get(requested).cloned();
    }
    agents.values().next().cloned()
}

fn handle_agent_op(op: &str, msg: &str, agent: Arc<AgentRuntime>, write_side: &mut UnixStream) {
    match op {
        "send" | "steer" => {
            let mut full = String::new();
            if let Some(b) = agent.queued_beat.lock().unwrap().take() {
                full.push_str(&b);
                full.push_str("\n\n");
            }
            full.push_str(msg);
            let sent = if op == "steer" {
                agent
                    .child
                    .lock()
                    .unwrap()
                    .command("steer", Some(&full))
                    .map(|_| "steer")
            } else {
                agent.child.lock().unwrap().send_auto(&full)
            };
            match sent {
                Ok(mode) => {
                    agent
                        .beat
                        .lock()
                        .unwrap()
                        .log(&agent.name, "user", mode, msg);
                    write_info_done(write_side, &format!("{} delivered as {mode}", agent.name));
                }
                Err(e) => write_info_done(write_side, &format!("{} error: {e}", agent.name)),
            }
        }
        "attach" => {
            let _ = write_side.write_all(
                format!(
                    "{{\"ev\":\"info\",\"data\":\"attached to {}\"}}\n",
                    agent.name
                )
                .as_bytes(),
            );
            if let Ok(s) = write_side.try_clone() {
                agent.clients.lock().unwrap().push(s);
            }
        }
        "status" => {
            let busy = agent.child.lock().unwrap().is_busy();
            let (last, doing) = {
                let b = agent.beat.lock().unwrap();
                (
                    b.last_beat.clone().unwrap_or_else(|| "never".into()),
                    b.doing_short(),
                )
            };
            let queued = agent.queued_beat.lock().unwrap().is_some();
            let fleet_tokens = tokens_today(None).total();
            write_info_done(
                write_side,
                &format!(
                    "agent {}: {} | last beat {last} | queued beat: {queued} | doing: {} | tokens today: {fleet_tokens}",
                    agent.name,
                    if busy { "working" } else { "idle" },
                    doing
                ),
            );
        }
        "stop" => {
            write_info_done(write_side, &format!("stopping {}", agent.name));
            agent.child.lock().unwrap().kill();
        }
        _ => write_info_done(write_side, "unknown op"),
    }
}

/// Role by agent name — mirrors MULTIROLE.md's default team.
fn role_of(name: &str) -> &'static str {
    match name {
        "main" => "Coordinator",
        "builder" => "Builder",
        "qa" => "QA",
        "ops" => "Ops",
        _ => "Agent",
    }
}

/// Structured last activity from the agent's own transcript: the recent tool
/// chain ("tasks→memory→codeindex") and its last words (clean word-boundary
/// tail) as separate fields — raw log tails render as garbage in the panel.
fn last_activity(name: &str) -> (String, String) {
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
struct TokenTotals {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

impl TokenTotals {
    fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

fn tokens_today(agent: Option<&str>) -> TokenTotals {
    let path = semdb::config::Config::load()
        .data_dir()
        .join("medvitund.semdb");
    let Ok(db) = semdb::storage::Db::open(&path) else {
        return TokenTotals::default();
    };
    let today = crate::beat::human_day_prefix();
    let mut out = TokenTotals::default();
    for entry in db.index.values() {
        let Ok(v) = json::parse(&entry.meta) else {
            continue;
        };
        if v.get("kind").and_then(Value::as_str) != Some("turn") {
            continue;
        }
        if let Some(a) = agent {
            if v.get("agent").and_then(Value::as_str) != Some(a) {
                continue;
            }
        }
        if v.get("day").and_then(Value::as_str) != Some(today.as_str()) {
            continue;
        }
        out.input += meta_u64(&v, "input");
        out.output += meta_u64(&v, "output");
        out.cache_read += meta_u64(&v, "cacheRead");
        out.cache_write += meta_u64(&v, "cacheWrite");
    }
    out
}

fn meta_u64(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0).max(0.0) as u64
}

fn write_agents(write_side: &mut UnixStream, agents: &Agents) {
    for agent in agents.values() {
        let busy = agent.child.lock().unwrap().is_busy();
        let doing = agent.beat.lock().unwrap().doing_short();
        let state = if busy { "working" } else { "idle" };
        let (tools, words) = last_activity(&agent.name);
        let tokens = tokens_today(Some(&agent.name));
        let line = format!(
            "{{\"ev\":\"info\",\"data\":\"{}\\t{}\\t{}\\t{}\\t{}\\t{}\\t{}\"}}\n",
            json::escape(&agent.name),
            state,
            json::escape(&doing),
            role_of(&agent.name),
            tokens.total(),
            json::escape(&tools),
            json::escape(&words)
        );
        let _ = write_side.write_all(line.as_bytes());
    }
    let _ = write_side.write_all(b"{\"ev\":\"done\"}\n");
}

fn write_info_done(write_side: &mut UnixStream, text: &str) {
    let _ = write_side.write_all(
        format!(
            "{{\"ev\":\"info\",\"data\":\"{}\"}}\n{{\"ev\":\"done\"}}\n",
            json::escape(text)
        )
        .as_bytes(),
    );
}

fn broadcast(clients: &Clients, line: &str) {
    let mut list = clients.lock().unwrap();
    list.retain_mut(|c| c.write_all(line.as_bytes()).is_ok());
}

fn append(path: &std::path::Path, text: &str) {
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = f.write_all(text.as_bytes());
    }
}

/// Shared client-side line classification (used by main.rs).
pub enum LineKind {
    Text(String),
    Info(String),
    Done,
}

pub fn client_line_kind(line: &str) -> LineKind {
    if let Ok(v) = json::parse(line) {
        let data = v
            .get("data")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        match v.get("ev").and_then(Value::as_str) {
            Some("text") => return LineKind::Text(data),
            Some("done") => return LineKind::Done,
            _ => return LineKind::Info(data),
        }
    }
    LineKind::Info(line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_kinds_parse() {
        assert!(
            matches!(client_line_kind("{\"ev\":\"text\",\"data\":\"hi\"}"), LineKind::Text(t) if t == "hi")
        );
        assert!(matches!(
            client_line_kind("{\"ev\":\"done\"}"),
            LineKind::Done
        ));
        assert!(matches!(
            client_line_kind("{\"ev\":\"info\",\"data\":\"x\"}"),
            LineKind::Info(_)
        ));
    }

    #[test]
    fn agent_list_dedupes() {
        assert_eq!(
            dedupe_agents(vec!["main".into(), "qa".into(), "main".into()]),
            vec!["main", "qa"]
        );
    }

    #[test]
    fn mute_detection_uses_zero_tokens_and_escalates() {
        let zero = crate::child::Usage::default();
        assert_eq!(mute_action(0, "", zero), MuteAction::Observe);
        assert_eq!(mute_action(1, "   ", zero), MuteAction::CompactRetry);
        assert_eq!(mute_action(2, "", zero), MuteAction::Rotate);
        assert_eq!(mute_action(2, "real text", zero), MuteAction::Clear);
        assert_eq!(
            mute_action(2, "", crate::child::Usage { input: 1, ..zero }),
            MuteAction::Clear
        );
    }

    #[test]
    fn cross_model_resume_reject_regression_recovers() {
        // A 5.4-mini session resumed under a future/different model can reject
        // old reasoning signatures by producing empty content and zero metered
        // tokens. Recovery is model-agnostic: observe once, compact+retry once,
        // then rotate to a fresh session if silence persists.
        let zero = crate::child::Usage::default();
        let actions = [
            mute_action(0, "", zero),
            mute_action(1, "", zero),
            mute_action(2, "", zero),
        ];
        assert_eq!(
            actions,
            [MuteAction::Observe, MuteAction::CompactRetry, MuteAction::Rotate]
        );
    }
}
