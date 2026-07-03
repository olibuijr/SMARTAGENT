//! The gateway daemon: owns pi rpc children, accepts unix-socket clients,
//! broadcasts agent output, runs heartbeats, and writes transcripts.
//!
//! Client wire protocol (line JSON, both directions):
//!   → {"op":"send"|"steer"|"attach"|"status"|"agents"|"stop","agent"?:s,"message"?:s}
//!   ← {"ev":"text","data":s} {"ev":"info","data":s} {"ev":"done"}

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use httpc::json::{self, Value};

use crate::agents_view::{status_line, write_agents};
use crate::beat::Beat;
use crate::child::{Event, PiChild};

type Clients = Arc<Mutex<Vec<UnixStream>>>;
pub(crate) type Agents = Arc<BTreeMap<String, Arc<AgentRuntime>>>;

const AUTONOMOUS_RULES: &str = "AUTONOMOUS MODE — act now, do not end the turn without one of these: (a) a task YOU pulled earlier is in doing → continue it; (b) otherwise, review has an item assigned to you/your role → verify it or move it with a note; (c) otherwise, ready has an unclaimed item → pull the highest-priority one; (d) otherwise → load the Triage skill and promote exactly one UNCLAIMED backlog task by p1→p2→p3, oldest, then smallest id, and pull it. Tasks in doing/review that belong to OTHER agents are never a reason to stay idle — skip them and take unclaimed backlog work. If the backlog is truly empty of unclaimed tasks, say so in one sentence — never answer with an empty turn. One task in your doing, maximum.";

struct Flags {
    agents: Vec<String>,
    chat_agents: Vec<String>,
    heartbeat_secs: u64,
    autonomous: bool,
}

pub(crate) struct AgentRuntime {
    pub(crate) name: String,
    pub(crate) child: Arc<Mutex<PiChild>>,
    pub(crate) clients: Clients,
    pub(crate) beat: Arc<Mutex<Beat>>,
    pub(crate) queued_beat: Arc<Mutex<Option<String>>>,
    pending_prompts: Arc<Mutex<VecDeque<String>>>,
    pub(crate) busy_since: Arc<Mutex<Option<Instant>>>,
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
        chat_agents: Vec::new(),
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
            "--chat" => {
                if let Some(v) = it.next() {
                    f.chat_agents.extend(split_agents(v));
                }
            }
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

    reconcile_board_roster(&repo_root, &flags.agents);

    let mut map = BTreeMap::new();
    // Worker agents (autonomous) + conversational chat agents (never
    // auto-prompted — they idle waiting for a real message, e.g. Telegram).
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
    for name in &flags.chat_agents {
        map.insert(
            name.clone(),
            Arc::new(spawn_agent(
                name,
                &repo_root,
                &data_dir,
                flags.heartbeat_secs,
                false,
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

fn reconcile_board_roster(repo_root: &std::path::Path, live_agents: &[String]) {
    if live_agents.is_empty() {
        return;
    }
    let tasks = repo_root.join("target/release/tasks");
    let _ = std::process::Command::new(&tasks)
        .args(["wip", "--doing", &live_agents.len().to_string()])
        .current_dir(repo_root)
        .output();
    for col in ["doing", "review"] {
        let Ok(out) = std::process::Command::new(&tasks)
            .args(["list", "--col", col])
            .current_dir(repo_root)
            .output()
        else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        for id in orphan_task_ids(&text, live_agents) {
            let _ = std::process::Command::new(&tasks)
                .args(["move", &id, "ready", "--force"])
                .current_dir(repo_root)
                .output();
        }
    }
}

fn orphan_task_ids(list_output: &str, live_agents: &[String]) -> Vec<String> {
    // Humans are never orphans: a task owned by the unix user (interactive
    // session work) must survive gateway restarts — only tasks owned by
    // DEAD AGENT names get requeued to ready.
    let human = std::env::var("USER").unwrap_or_default();
    list_output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let id = parts.next()?;
            let owner = line
                .split_whitespace()
                .find_map(|part| part.strip_prefix('@'))?;
            if live_agents.iter().any(|a| a == owner) || (!human.is_empty() && owner == human) {
                None
            } else {
                Some(id.to_string())
            }
        })
        .collect()
}

fn spawn_agent(
    name: &str,
    repo_root: &std::path::Path,
    data_dir: &std::path::Path,
    heartbeat_secs: u64,
    autonomous: bool,
) -> Result<AgentRuntime, String> {
    // Per-agent identity (full name + specialty) plus the autonomous rules.
    let mut prompt_parts: Vec<String> = Vec::new();
    if let Some(identity) = crate::roster::identity_prompt(name) {
        prompt_parts.push(identity);
    }
    if autonomous {
        prompt_parts.push(AUTONOMOUS_RULES.to_string());
    }
    let static_prompt = if prompt_parts.is_empty() {
        None
    } else {
        Some(prompt_parts.join("\n\n"))
    };
    let model = crate::roster::model_for(&semdb::config::Config::load(), name);
    let mut child = PiChild::spawn(repo_root, name, static_prompt.as_deref(), model)?;
    let clients: Clients = Arc::new(Mutex::new(Vec::new()));
    let beat = Beat::new(repo_root, data_dir);
    let queued_beat = Mutex::new(None);
    let pending_prompts = Arc::new(Mutex::new(VecDeque::new()));
    let busy_since = Arc::new(Mutex::new(None));
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
        pending_prompts: pending_prompts.clone(),
        busy_since: busy_since.clone(),
    };
    start_event_pump(
        name.to_string(),
        clients,
        transcript,
        events,
        beat.clone(),
        child.clone(),
        busy_since.clone(),
    );
    let prompt_gate = Arc::new(Mutex::new(PromptGate {
        last: None,
        cooldown_secs: 20,
    }));
    if autonomous {
        start_work_chaser(
            name.to_string(),
            child.clone(),
            beat.clone(),
            prompt_gate.clone(),
            pending_prompts.clone(),
        );
    }
    start_heartbeat(
        name.to_string(),
        child,
        beat,
        queued_beat,
        heartbeat_secs,
        autonomous,
        prompt_gate,
        pending_prompts,
    );
    Ok(runtime)
}

fn start_event_pump(
    agent: String,
    clients: Clients,
    tpath: std::path::PathBuf,
    events: std::sync::mpsc::Receiver<Event>,
    beat: Arc<Mutex<Beat>>,
    child: Arc<Mutex<PiChild>>,
    busy_since: Arc<Mutex<Option<Instant>>>,
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
                    *busy_since.lock().unwrap() = if b { Some(Instant::now()) } else { None };
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
                    let msg = tool_status_line(&name, None);
                    append(&tpath, &format!("\n{msg}\n"));
                    broadcast(
                        &clients,
                        &format!("{{\"ev\":\"info\",\"data\":\"{}\"}}\n", json::escape(&msg)),
                    );
                }
                Event::ToolEnd(name, ok) => {
                    let msg = tool_status_line(&name, Some(ok));
                    append(&tpath, &format!("\n{msg}\n"));
                    broadcast(
                        &clients,
                        &format!("{{\"ev\":\"info\",\"data\":\"{}\"}}\n", json::escape(&msg)),
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
    pending_prompts: Arc<Mutex<VecDeque<String>>>,
) {
    let period = Duration::from_secs(heartbeat_secs.max(10));
    // Stable per-agent phase offset (0..72s by name hash): beats spread over
    // the period instead of all firing together — a restart or a new task no
    // longer wakes the whole fleet in the same instant.
    let stagger = {
        let h: u32 = agent
            .bytes()
            .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
        Duration::from_secs((h % 8) as u64 * 9)
    };
    std::thread::spawn(move || {
        // T-79: startup beat ~10s after serve — a restart must not cost a
        // full period of dead air before in-doing work resumes.
        let mut next = Duration::from_secs(10) + stagger;
        loop {
            std::thread::sleep(next);
            next = period;
            let busy = child.lock().unwrap().is_busy();
            let (text, has_work) = {
                let mut b = beat.lock().unwrap();
                let role = role_of(&agent);
                b.compose_with_work(busy, &agent, role)
            };
            beat.lock()
                .unwrap()
                .log(&agent, "beat", if busy { "busy" } else { "idle" }, &text);
            let should_check_work = autonomous && !busy;
            match heartbeat_action(busy, autonomous, should_check_work && has_work) {
                HeartbeatAction::Steer => {
                    queue_prompt(&pending_prompts, text);
                }
                HeartbeatAction::Prompt => {
                    if autonomous_allowed(&prompt_gate) {
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
    text.to_string()
}

/// Shared cooldown so the heartbeat and the work-chaser never double-prompt:
/// at most one autonomous prompt per 20s per agent.
#[derive(Default)]
struct PromptGate {
    last: Option<std::time::Instant>,
    cooldown_secs: u64,
}

impl PromptGate {
    fn ready(&self) -> bool {
        self.last
            .map(|t| t.elapsed() >= Duration::from_secs(self.cooldown_secs))
            .unwrap_or(true)
    }

    fn commit(&mut self) {
        self.last = Some(std::time::Instant::now());
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
    pending_prompts: Arc<Mutex<VecDeque<String>>>,
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
            if drain_pending_prompt(&child, &pending_prompts) {
                continue;
            }
            let (text, has_work) = {
                let mut b = beat.lock().unwrap();
                let role = role_of(&agent);
                b.compose_with_work(false, &agent, role)
            };
            if has_work && autonomous_allowed(&prompt_gate) {
                beat.lock().unwrap().log(
                    &agent,
                    "chase",
                    "idle",
                    "work-chaser: immediate next-task prompt",
                );
                let auto = autonomous_prompt(&text);
                let _ = child.lock().unwrap().command("prompt", Some(&auto));
            }
        }
    });
}

fn queue_prompt(queue: &Arc<Mutex<VecDeque<String>>>, text: String) {
    queue.lock().unwrap().push_back(text);
}

fn drain_pending_prompt(child: &Arc<Mutex<PiChild>>, queue: &Arc<Mutex<VecDeque<String>>>) -> bool {
    let Some(next) = queue.lock().unwrap().pop_front() else {
        return false;
    };
    if child
        .lock()
        .unwrap()
        .command("prompt", Some(&next))
        .is_err()
    {
        queue.lock().unwrap().push_front(next);
        return false;
    }
    true
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

/// Fleet-wide autonomy switch: `gateway autonomy off` silences all
/// autonomous prompting (heartbeat pickup + work-chaser) without touching
/// running turns or chat agents. Deliberate work keeps flowing via send/steer.
static AUTONOMY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Fleet-wide dispatch gate: at most ONE autonomous wake-up per 15s across
/// ALL agents. One new task wakes one agent; the next agent's beat sees the
/// task already claimed. Kills the thundering herd without slowing a deep
/// queue much (8 pickups still start within ~2 minutes).
static GLOBAL_DISPATCH: Mutex<PromptGate> = Mutex::new(PromptGate {
    last: None,
    cooldown_secs: 15,
});

fn autonomous_allowed(per_agent: &Arc<Mutex<PromptGate>>) -> bool {
    autonomous_allowed_with_global(&GLOBAL_DISPATCH, per_agent)
}

fn autonomous_allowed_with_global(
    global: &Mutex<PromptGate>,
    per_agent: &Arc<Mutex<PromptGate>>,
) -> bool {
    if !AUTONOMY.load(std::sync::atomic::Ordering::Relaxed) {
        return false;
    }
    let mut per = per_agent.lock().unwrap();
    if !per.ready() {
        return false;
    }
    let mut fleet = global.lock().unwrap();
    if !fleet.ready() {
        return false;
    }
    per.commit();
    fleet.commit();
    true
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
            "autonomy" => {
                use std::sync::atomic::Ordering;
                match msg {
                    "on" => AUTONOMY.store(true, Ordering::Relaxed),
                    "off" => AUTONOMY.store(false, Ordering::Relaxed),
                    _ => {}
                }
                let state = if AUTONOMY.load(Ordering::Relaxed) {
                    "on"
                } else {
                    "off"
                };
                write_info_done(&mut write_side, &format!("fleet autonomy: {state}"));
            }
            "stop" if stop_all_requested(requested) => stop_all_agents(&agents, &mut write_side),
            "send" | "steer" | "attach" | "ask" | "status" | "stop" => {
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

fn stop_all_requested(requested: &str) -> bool {
    requested == "*"
}

fn stop_all_agents(agents: &Agents, write_side: &mut UnixStream) {
    for agent in agents.values() {
        agent.child.lock().unwrap().kill();
    }
    write_info_done(write_side, &format!("stopping {} agents", agents.len()));
}

fn select_agent(agents: &Agents, requested: &str) -> Option<Arc<AgentRuntime>> {
    if !requested.is_empty() {
        return agents.get(requested).cloned();
    }
    agents.values().next().cloned()
}

fn handle_agent_op(op: &str, msg: &str, agent: Arc<AgentRuntime>, write_side: &mut UnixStream) {
    match op {
        "ask" => {
            // Request→reply: register this connection as a broadcast client so
            // it receives the agent's reply turn, THEN deliver the prompt.
            if let Ok(s) = write_side.try_clone() {
                clients_lock(&agent.clients).push(s);
            }
            let busy = agent.child.lock().unwrap().is_busy();
            if busy {
                queue_prompt(&agent.pending_prompts, msg.to_string());
            } else {
                let _ = agent.child.lock().unwrap().command("prompt", Some(msg));
            }
            agent.beat.lock().unwrap().log(
                &agent.name,
                "user",
                if busy { "queued" } else { "ask" },
                msg,
            );
        }
        "send" | "steer" => {
            let mut full = String::new();
            if let Some(b) = agent.queued_beat.lock().unwrap().take() {
                full.push_str(&b);
                full.push_str("\n\n");
            }
            full.push_str(msg);
            let busy = agent.child.lock().unwrap().is_busy();
            let sent = if busy {
                queue_prompt(&agent.pending_prompts, full);
                Ok("queued")
            } else {
                agent
                    .child
                    .lock()
                    .unwrap()
                    .command("prompt", Some(&full))
                    .map(|_| "prompt")
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
                clients_lock(&agent.clients).push(s);
            }
        }
        "status" => {
            let busy = agent.child.lock().unwrap().is_busy();
            let (last, doing, tokens) = {
                let b = agent.beat.lock().unwrap();
                (
                    b.last_beat.clone().unwrap_or_else(|| "never".into()),
                    b.doing_short(),
                    b.tokens_today(Some(&agent.name)).total(),
                )
            };
            let queued = agent.queued_beat.lock().unwrap().is_some();
            write_info_done(
                write_side,
                &status_line(&agent.name, busy, &last, queued, &doing, tokens),
            );
        }
        "stop" => {
            write_info_done(write_side, &format!("stopping {}", agent.name));
            agent.child.lock().unwrap().kill();
        }
        _ => write_info_done(write_side, "unknown op"),
    }
}

pub(crate) use crate::roster::role_of;

/// Structured last activity from the agent's own transcript: the recent tool
/// chain ("tasks→memory→codeindex") and its last words (clean word-boundary
/// tail) as separate fields — raw log tails render as garbage in the panel.
pub(crate) fn tool_status_line(name: &str, done: Option<bool>) -> String {
    let clean = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect::<String>();
    let tool = if clean.is_empty() {
        "tool"
    } else {
        clean.as_str()
    };
    match done {
        None => format!("🛠 {tool} running…"),
        Some(true) => format!("🛠 {tool} ✓"),
        Some(false) => format!("🛠 {tool} ✗"),
    }
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

fn clients_lock(clients: &Clients) -> std::sync::MutexGuard<'_, Vec<UnixStream>> {
    // T-84: attach clients are deliberately disposable (tmux panes and
    // terminal pipes can be SIGKILLed). A panic while touching the client list
    // must not poison the daemon's broadcast path forever; recover the list,
    // then let broadcast prune broken sockets on the next write.
    clients
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn broadcast(clients: &Clients, line: &str) {
    let mut list = clients_lock(clients);
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
    fn tool_status_line_is_concise_and_sanitized() {
        assert_eq!(tool_status_line("memory", None), "🛠 memory running…");
        assert_eq!(tool_status_line("tasks", Some(true)), "🛠 tasks ✓");
        assert_eq!(tool_status_line("secret=abc", Some(false)), "🛠 secretabc ✗");
    }

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
    fn orphan_task_ids_detects_non_roster_owners() {
        let live = vec!["linus".to_string(), "grace".to_string()];
        let rows = "T-1\tdoing\tp2\told owner @builder\nT-2\tdoing\tp2\tlive owner @linus\nT-3\treview\tp2\tother @dev3\n";
        assert_eq!(orphan_task_ids(rows, &live), vec!["T-1", "T-3"]);
    }

    #[test]
    fn agent_list_dedupes() {
        assert_eq!(
            dedupe_agents(vec!["main".into(), "qa".into(), "main".into()]),
            vec!["main", "qa"]
        );
    }

    #[test]
    fn stop_all_uses_explicit_wildcard_path() {
        assert!(stop_all_requested("*"));
        assert!(!stop_all_requested("main"));
        assert!(!stop_all_requested(""));
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
    fn queued_prompts_keep_fifo_order_for_busy_beat_and_send_soak() {
        let q = Arc::new(Mutex::new(VecDeque::new()));
        queue_prompt(&q, "beat-while-busy".to_string());
        for i in 0..5 {
            queue_prompt(&q, format!("send-{i}"));
        }
        let drained: Vec<String> = (0..6)
            .filter_map(|_| q.lock().unwrap().pop_front())
            .collect();
        assert_eq!(
            drained,
            vec![
                "beat-while-busy",
                "send-0",
                "send-1",
                "send-2",
                "send-3",
                "send-4"
            ]
        );
        assert!(q.lock().unwrap().is_empty());
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
            [
                MuteAction::Observe,
                MuteAction::CompactRetry,
                MuteAction::Rotate
            ]
        );
    }

    #[test]
    fn autonomous_prompt_carries_only_dynamic_beat_text() {
        let beat =
            "⏲ heartbeat 2026-07-02 22:44:40Z | session up 2m10s | you are working\nREADY (1)";
        let prompt = autonomous_prompt(beat);
        assert_eq!(prompt, beat);
        assert!(!prompt.contains("AUTONOMOUS MODE"));
        assert!(AUTONOMOUS_RULES.contains("AUTONOMOUS MODE"));
    }

    #[test]
    fn blocked_per_agent_gate_does_not_consume_global_dispatch() {
        let global = Mutex::new(PromptGate {
            last: None,
            cooldown_secs: 15,
        });
        let per = Arc::new(Mutex::new(PromptGate {
            last: Some(Instant::now()),
            cooldown_secs: 20,
        }));

        assert!(!autonomous_allowed_with_global(&global, &per));
        assert!(global.lock().unwrap().last.is_none());

        per.lock().unwrap().last = None;
        assert!(autonomous_allowed_with_global(&global, &per));
        assert!(global.lock().unwrap().last.is_some());
        assert!(per.lock().unwrap().last.is_some());
    }

    #[test]
    fn killed_attach_client_is_pruned_without_poisoning_broadcasts() {
        // Regression for T-84: an attached terminal/tmux client can disappear
        // ungracefully. The daemon must treat that as a dead subscriber, not as
        // a fatal broadcast error or a permanently poisoned client registry.
        let clients: Clients = Arc::new(Mutex::new(Vec::new()));
        let (server_side, client_side) = UnixStream::pair().unwrap();
        clients_lock(&clients).push(server_side);
        drop(client_side);

        broadcast(&clients, "{\"ev\":\"text\",\"data\":\"hi\"}\n");
        assert_eq!(clients_lock(&clients).len(), 0);

        // Simulate a panic while holding the registry: subsequent broadcasts
        // must recover the inner list instead of panicking on PoisonError.
        let poisoned = clients.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoned.lock().unwrap();
            panic!("synthetic attach-list panic");
        })
        .join();
        broadcast(&clients, "{\"ev\":\"done\"}\n");
        assert_eq!(clients_lock(&clients).len(), 0);
    }
}
