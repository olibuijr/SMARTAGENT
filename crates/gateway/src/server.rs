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
                f.heartbeat_secs = it.next().and_then(|v| v.parse().ok()).unwrap_or(f.heartbeat_secs)
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
    let listener = UnixListener::bind(&sock).map_err(|e| format!("bind {}: {e}", sock.display()))?;

    let mut map = BTreeMap::new();
    for name in &flags.agents {
        map.insert(
            name.clone(),
            Arc::new(spawn_agent(name, &repo_root, &data_dir, flags.heartbeat_secs, flags.autonomous)?),
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
    start_event_pump(name.to_string(), clients, data_dir.to_path_buf(), transcript, events, beat.clone());
    start_heartbeat(name.to_string(), child, beat, queued_beat, heartbeat_secs, autonomous);
    Ok(runtime)
}

fn start_event_pump(
    agent: String,
    clients: Clients,
    _data_dir: std::path::PathBuf,
    tpath: std::path::PathBuf,
    events: std::sync::mpsc::Receiver<Event>,
    beat: Arc<Mutex<Beat>>,
) {
    std::thread::spawn(move || {
        for ev in events.iter() {
            match ev {
                Event::Text(t) => {
                    append(&tpath, &t);
                    broadcast(&clients, &format!("{{\"ev\":\"text\",\"data\":\"{}\"}}\n", json::escape(&t)));
                }
                Event::State(b) => {
                    broadcast(
                        &clients,
                        &format!("{{\"ev\":\"info\",\"data\":\"agent {}\"}}\n", if b { "working…" } else { "idle" }),
                    );
                    if !b {
                        broadcast(&clients, "{\"ev\":\"done\"}\n");
                    }
                }
                Event::TurnEnd(text) => {
                    append(&tpath, "\n---\n");
                    beat.lock().unwrap().log(&agent, "turn", "idle", &text);
                }
                Event::Tool(name) => {
                    append(&tpath, &format!("\n⚙ {name}\n"));
                    broadcast(&clients, &format!("{{\"ev\":\"info\",\"data\":\"⚙ {}\"}}\n", json::escape(&name)));
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
) {
    let period = Duration::from_secs(heartbeat_secs.max(10));
    std::thread::spawn(move || loop {
        std::thread::sleep(period);
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
                let auto = format!(
                    "{text}\nAUTONOMOUS MODE — act now, do not end the turn without one of these: (a) a task is in doing → continue it; (b) doing empty, ready has items → pull the highest-priority one; (c) doing AND ready empty → load the Triage skill and promote exactly one backlog task by p1→p2→p3, oldest, then smallest id before pulling it. The backlog must never starve the loop. If you truly cannot act, say why in one sentence — never answer with an empty turn. One task in doing, maximum."
                );
                let _ = child.lock().unwrap().command("prompt", Some(&auto));
            }
            HeartbeatAction::Queue => *queued.lock().unwrap() = Some(text),
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
                agent.child.lock().unwrap().command("steer", Some(&full)).map(|_| "steer")
            } else {
                agent.child.lock().unwrap().send_auto(&full)
            };
            match sent {
                Ok(mode) => {
                    agent.beat.lock().unwrap().log(&agent.name, "user", mode, msg);
                    write_info_done(write_side, &format!("{} delivered as {mode}", agent.name));
                }
                Err(e) => write_info_done(write_side, &format!("{} error: {e}", agent.name)),
            }
        }
        "attach" => {
            let _ = write_side.write_all(format!("{{\"ev\":\"info\",\"data\":\"attached to {}\"}}\n", agent.name).as_bytes());
            if let Ok(s) = write_side.try_clone() {
                agent.clients.lock().unwrap().push(s);
            }
        }
        "status" => {
            let busy = agent.child.lock().unwrap().is_busy();
            let (last, doing) = {
                let b = agent.beat.lock().unwrap();
                (b.last_beat.clone().unwrap_or_else(|| "never".into()), b.doing_short())
            };
            let queued = agent.queued_beat.lock().unwrap().is_some();
            write_info_done(
                write_side,
                &format!(
                    "agent {}: {} | last beat {last} | queued beat: {queued} | doing: {}",
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

/// Last visible activity from the agent's own transcript — the honest
/// per-agent line until board ownership (T-77) exists. Tools show as ⚙name;
/// otherwise the tail of its last words.
fn last_activity(name: &str) -> String {
    let path = semdb::config::Config::load()
        .data_dir()
        .join("gateway")
        .join(format!("{name}.log"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return String::new();
    };
    let tail: String = text.chars().rev().take(220).collect::<String>().chars().rev().collect();
    let flat = tail.replace('\n', " ").trim().to_string();
    let snip: String = flat.chars().rev().take(70).collect::<String>().chars().rev().collect();
    snip.trim_start_matches(['-', ' ']).to_string()
}

fn write_agents(write_side: &mut UnixStream, agents: &Agents) {
    for agent in agents.values() {
        let busy = agent.child.lock().unwrap().is_busy();
        let doing = agent.beat.lock().unwrap().doing_short();
        let state = if busy { "working" } else { "idle" };
        let line = format!(
            "{{\"ev\":\"info\",\"data\":\"{}\\t{}\\t{}\\t{}\\t{}\"}}\n",
            json::escape(&agent.name),
            state,
            json::escape(&doing),
            role_of(&agent.name),
            json::escape(&last_activity(&agent.name))
        );
        let _ = write_side.write_all(line.as_bytes());
    }
    let _ = write_side.write_all(b"{\"ev\":\"done\"}\n");
}

fn write_info_done(write_side: &mut UnixStream, text: &str) {
    let _ = write_side.write_all(
        format!("{{\"ev\":\"info\",\"data\":\"{}\"}}\n{{\"ev\":\"done\"}}\n", json::escape(text)).as_bytes(),
    );
}

fn broadcast(clients: &Clients, line: &str) {
    let mut list = clients.lock().unwrap();
    list.retain_mut(|c| c.write_all(line.as_bytes()).is_ok());
}

fn append(path: &std::path::Path, text: &str) {
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
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
        let data = v.get("data").and_then(Value::as_str).unwrap_or("").to_string();
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
        assert!(matches!(client_line_kind("{\"ev\":\"text\",\"data\":\"hi\"}"), LineKind::Text(t) if t == "hi"));
        assert!(matches!(client_line_kind("{\"ev\":\"done\"}"), LineKind::Done));
        assert!(matches!(client_line_kind("{\"ev\":\"info\",\"data\":\"x\"}"), LineKind::Info(_)));
    }

    #[test]
    fn agent_list_dedupes() {
        assert_eq!(dedupe_agents(vec!["main".into(), "qa".into(), "main".into()]), vec!["main", "qa"]);
    }
}
