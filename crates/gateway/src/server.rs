//! The gateway daemon: owns the pi rpc child, accepts unix-socket clients,
//! broadcasts the agent's output, runs the heartbeat, writes the transcript.
//!
//! Client wire protocol (line JSON, both directions):
//!   → {"op":"send"|"steer"|"attach"|"status"|"stop","message"?:string}
//!   ← {"ev":"text","data":s} {"ev":"info","data":s} {"ev":"done"}

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use httpc::json::{self, Value};

use crate::beat::Beat;
use crate::child::{Event, PiChild};

type Clients = Arc<Mutex<Vec<UnixStream>>>;

struct Flags {
    agent: String,
    heartbeat_secs: u64,
    autonomous: bool,
}

fn parse_flags(args: &[String]) -> Flags {
    let cfg = semdb::config::Config::load();
    let mut f = Flags {
        agent: cfg
            .resolve("gateway_agent", "SMARTAGENT_GATEWAY_AGENT", None)
            .unwrap_or_else(|| "main".into()),
        heartbeat_secs: cfg
            .resolve("heartbeat_secs", "SMARTAGENT_HEARTBEAT_SECS", None)
            .and_then(|v| v.parse().ok())
            .unwrap_or(120),
        autonomous: false,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--agent" => f.agent = it.next().cloned().unwrap_or(f.agent),
            "--heartbeat-secs" => {
                f.heartbeat_secs = it.next().and_then(|v| v.parse().ok()).unwrap_or(f.heartbeat_secs)
            }
            "--autonomous" => f.autonomous = true,
            _ => {}
        }
    }
    f
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

    // Stale-socket takeover: if nothing answers, remove and rebind.
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

    let mut child = PiChild::spawn(&repo_root, &flags.agent)?;
    let clients: Clients = Arc::new(Mutex::new(Vec::new()));
    let beat = Arc::new(Mutex::new(Beat::new(&repo_root, &data_dir)));
    let queued_beat: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let transcript = data_dir.join("gateway").join(format!("{}.log", flags.agent));
    if let Some(d) = transcript.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    eprintln!(
        "[gateway] agent '{}' up — socket {}, heartbeat {}s{}",
        flags.agent,
        sock.display(),
        flags.heartbeat_secs,
        if flags.autonomous { ", AUTONOMOUS" } else { "" }
    );

    // Event pump: child events → transcript + broadcast + medvitund turn log.
    {
        let clients = clients.clone();
        let beat = beat.clone();
        let agent = flags.agent.clone();
        let events = std::mem::replace(&mut child.events, std::sync::mpsc::channel().1);
        let tpath = transcript.clone();
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
                            // lets one-shot `gateway send` clients return;
                            // attach clients ignore `done` and keep streaming
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
                            &format!("{{\"ev\":\"info\",\"data\":\"pi child exited ({code}) — gateway stopping\"}}\n"),
                        );
                        std::process::exit(1);
                    }
                }
            }
        });
    }

    let child = Arc::new(Mutex::new(child));

    // Heartbeat timer.
    {
        let child = child.clone();
        let beat = beat.clone();
        let queued = queued_beat.clone();
        let agent = flags.agent.clone();
        let autonomous = flags.autonomous;
        let period = Duration::from_secs(flags.heartbeat_secs.max(10));
        std::thread::spawn(move || loop {
            std::thread::sleep(period);
            let busy = child.lock().unwrap().is_busy();
            let text = beat.lock().unwrap().compose(busy);
            beat.lock().unwrap().log(&agent, "beat", if busy { "busy" } else { "idle" }, &text);
            if busy {
                let _ = child.lock().unwrap().command("steer", Some(&text));
            } else if autonomous {
                let auto = format!(
                    "{text}\nAUTONOMOUS MODE: if a task is in doing, continue it now. If doing is empty, pull the ONE highest-priority ready task to doing and work it end-to-end with real evidence, then close it. Never more than one task in doing."
                );
                let _ = child.lock().unwrap().command("prompt", Some(&auto));
            } else {
                *queued.lock().unwrap() = Some(text); // zero-cost until next send
            }
        });
    }

    // Accept loop.
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let child = child.clone();
        let clients = clients.clone();
        let queued = queued_beat.clone();
        let beat = beat.clone();
        let agent = flags.agent.clone();
        std::thread::spawn(move || handle_client(stream, child, clients, queued, beat, agent));
    }
    Ok(())
}

fn handle_client(
    stream: UnixStream,
    child: Arc<Mutex<PiChild>>,
    clients: Clients,
    queued_beat: Arc<Mutex<Option<String>>>,
    beat: Arc<Mutex<Beat>>,
    agent: String,
) {
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
        match op {
            "send" | "steer" => {
                // Deliver any queued idle beat first, so the agent regains
                // time-awareness in the same turn (one inference, not two).
                let mut full = String::new();
                if let Some(b) = queued_beat.lock().unwrap().take() {
                    full.push_str(&b);
                    full.push_str("\n\n");
                }
                full.push_str(msg);
                let sent = if op == "steer" {
                    child.lock().unwrap().command("steer", Some(&full)).map(|_| "steer")
                } else {
                    child.lock().unwrap().send_auto(&full)
                };
                match sent {
                    Ok(mode) => {
                        beat.lock().unwrap().log(&agent, "user", mode, msg);
                        // one-shot senders become listeners until turn end
                        let _ = write_side.write_all(
                            format!("{{\"ev\":\"info\",\"data\":\"delivered as {mode}\"}}\n").as_bytes(),
                        );
                        clients.lock().unwrap().push(match write_side.try_clone() {
                            Ok(s) => s,
                            Err(_) => return,
                        });
                    }
                    Err(e) => {
                        let _ = write_side.write_all(
                            format!("{{\"ev\":\"info\",\"data\":\"error: {}\"}}\n", json::escape(&e)).as_bytes(),
                        );
                    }
                }
            }
            "attach" => {
                let _ = write_side.write_all(b"{\"ev\":\"info\",\"data\":\"attached\"}\n");
                clients.lock().unwrap().push(match write_side.try_clone() {
                    Ok(s) => s,
                    Err(_) => return,
                });
            }
            "status" => {
                let busy = child.lock().unwrap().is_busy();
                let (last, doing) = {
                    let b = beat.lock().unwrap();
                    (b.last_beat.clone().unwrap_or_else(|| "never".into()), b.doing_short())
                };
                let queued = queued_beat.lock().unwrap().is_some();
                let _ = write_side.write_all(
                    format!(
                        "{{\"ev\":\"info\",\"data\":\"agent {agent}: {} | last beat {last} | queued beat: {queued} | doing: {}\"}}\n{{\"ev\":\"done\"}}\n",
                        if busy { "working" } else { "idle" },
                        json::escape(&doing)
                    )
                    .as_bytes(),
                );
            }
            "stop" => {
                let _ = write_side.write_all(b"{\"ev\":\"info\",\"data\":\"stopping\"}\n{\"ev\":\"done\"}\n");
                child.lock().unwrap().kill();
                let _ = std::fs::remove_file(crate::socket_path());
                std::process::exit(0);
            }
            _ => {
                let _ = write_side.write_all(b"{\"ev\":\"info\",\"data\":\"unknown op\"}\n");
            }
        }
    }
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
}
