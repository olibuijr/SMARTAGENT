//! gateway — persistent agent service (meðvitund).
//!
//! Owns long-running `./pi --mode rpc` children so agents survive client
//! disconnects; clients attach/detach over a unix socket. A heartbeat keeps
//! each agent aware of time, its board, and the plan ahead, and every beat and
//! turn lands in the `medvitund` semdb table. Design: Plans/MEDVITUND.md.

mod beat;
mod child;
mod server;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

fn usage() -> ! {
    eprintln!(
        "gateway — persistent agent service (meðvitund)

USAGE:
  gateway serve [--agent NAME ...] [--agents a,b,c] [--heartbeat-secs N] [--autonomous]
  gateway send [--agent NAME] <message...>       one-shot message; prints delivery/reply
  gateway steer [--agent NAME] <message...>      inject while the agent is working
  gateway attach [--agent NAME]                  stream output; stdin lines are sent
  gateway status [--agent NAME]                  agent state, uptime, last beat
  gateway agents                                list hosted agents
  gateway stop [--agent NAME|--all]              stop one agent or the daemon

Socket: config gateway_socket (default .pi/gateway.sock). Config keys:
gateway_agent, gateway_agents, heartbeat_secs (default 120)."
    );
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();
    let result = match cmd {
        "serve" => server::serve(&rest),
        "send" => {
            let (agent, msg) = parse_client_args(&rest);
            client_send("send", &agent, &msg)
        }
        "steer" => {
            let (agent, msg) = parse_client_args(&rest);
            client_send("steer", &agent, &msg)
        }
        "attach" => {
            let (agent, _) = parse_client_args(&rest);
            client_attach(&agent)
        }
        "status" => {
            let (agent, _) = parse_client_args(&rest);
            client_send("status", &agent, "")
        }
        "agents" => client_send("agents", "", ""),
        "statusline" => statusline(),
        "stop" => {
            let (agent, _) = parse_client_args(&rest);
            client_send("stop", &agent, "")
        }
        _ => usage(),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn parse_client_args(args: &[String]) -> (String, String) {
    let mut agent = default_agent();
    let mut words = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--agent" => agent = it.next().cloned().unwrap_or(agent),
            "--all" => agent = "*".into(),
            _ => words.push(a.clone()),
        }
    }
    (agent, words.join(" "))
}

fn default_agent() -> String {
    let cfg = semdb::config::Config::load();
    cfg.resolve("gateway_agent", "SMARTAGENT_GATEWAY_AGENT", None)
        .unwrap_or_else(|| "main".into())
}

pub fn socket_path() -> std::path::PathBuf {
    let cfg = semdb::config::Config::load();
    let rel = cfg
        .resolve("gateway_socket", "SMARTAGENT_GATEWAY_SOCKET", None)
        .unwrap_or_else(|| ".pi/gateway.sock".into());
    let p = std::path::PathBuf::from(&rel);
    if p.is_absolute() {
        p
    } else {
        cfg.data_dir().parent().map(|r| r.join(&rel)).unwrap_or(p)
    }
}

fn connect() -> Result<UnixStream, String> {
    let path = socket_path();
    UnixStream::connect(&path).map_err(|e| {
        format!(
            "gateway not running at {}: {e} (start: gateway serve)",
            path.display()
        )
    })
}

/// One-shot op: send a line, print response lines until `done` marker.
fn client_send(op: &str, agent: &str, message: &str) -> Result<(), String> {
    let mut stream = connect()?;
    let line = format!(
        "{{\"op\":\"{}\",\"agent\":\"{}\",\"message\":\"{}\"}}\n",
        op,
        httpc::json::escape(agent),
        httpc::json::escape(message)
    );
    stream
        .write_all(line.as_bytes())
        .map_err(|e| e.to_string())?;
    let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    for l in reader.lines() {
        let l = l.map_err(|e| e.to_string())?;
        match server::client_line_kind(&l) {
            server::LineKind::Text(t) => print!("{t}"),
            server::LineKind::Info(t) => println!("{t}"),
            server::LineKind::Done => {
                println!();
                return Ok(());
            }
        }
    }
    Ok(())
}

/// One-line statusline; compactly prefers main, or all agents via `agents`.
fn statusline() -> Result<(), String> {
    let Ok(mut stream) = UnixStream::connect(socket_path()) else {
        println!("warn|⏲ gateway down");
        return Ok(());
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    let agent = default_agent();
    let line = format!(
        "{{\"op\":\"status\",\"agent\":\"{}\"}}\n",
        httpc::json::escape(&agent)
    );
    if stream.write_all(line.as_bytes()).is_err() {
        println!("warn|⏲ gateway unreachable");
        return Ok(());
    }
    let reader = BufReader::new(stream);
    for l in reader.lines() {
        let Ok(l) = l else { break };
        if let server::LineKind::Info(t) = server::client_line_kind(&l) {
            let mut state = "?".to_string();
            let mut beat = "—".to_string();
            let mut doing = String::new();
            let mut tokens = String::new();
            for part in t.split(" | ") {
                if let Some(rest) = part.strip_prefix("agent ") {
                    state = rest.replace(": ", " ");
                } else if let Some(rest) = part.strip_prefix("last beat ") {
                    if let Some(time) = rest.split(' ').nth(1) {
                        beat = time.chars().take(5).collect();
                    }
                } else if let Some(rest) = part.strip_prefix("doing: ") {
                    doing = rest.split(' ').next().unwrap_or("").to_string();
                } else if let Some(rest) = part.strip_prefix("tokens today: ") {
                    tokens = rest.to_string();
                }
            }
            let tok = if tokens.is_empty() {
                String::new()
            } else {
                format!(" · {tokens}tok")
            };
            if doing.is_empty() || doing == "nothing" {
                println!("ok|⏲ {state} · {beat}{tok}");
            } else {
                println!("ok|⏲ {state} · {beat} · {doing}{tok}");
            }
            return Ok(());
        }
    }
    println!("warn|⏲ gateway no answer");
    Ok(())
}

/// Interactive attach: stream events; forward stdin lines.
fn client_attach(agent: &str) -> Result<(), String> {
    let mut stream = connect()?;
    let line = format!(
        "{{\"op\":\"attach\",\"agent\":\"{}\"}}\n",
        httpc::json::escape(agent)
    );
    stream
        .write_all(line.as_bytes())
        .map_err(|e| e.to_string())?;
    let read_side = stream.try_clone().map_err(|e| e.to_string())?;
    let printer = std::thread::spawn(move || {
        for l in BufReader::new(read_side).lines() {
            let Ok(l) = l else { break };
            match server::client_line_kind(&l) {
                server::LineKind::Text(t) => {
                    print!("{t}");
                    let _ = std::io::stdout().flush();
                }
                server::LineKind::Info(t) => eprintln!("\n[gateway] {t}"),
                server::LineKind::Done => {}
            }
        }
        eprintln!("\n[gateway] disconnected");
    });
    eprintln!("[gateway] attached to {agent} — type to talk, Ctrl-D detaches");
    let stdin = std::io::stdin();
    for l in stdin.lock().lines() {
        let Ok(l) = l else { break };
        if l.trim().is_empty() {
            continue;
        }
        let line = format!(
            "{{\"op\":\"send\",\"agent\":\"{}\",\"message\":\"{}\"}}\n",
            httpc::json::escape(agent),
            httpc::json::escape(&l)
        );
        if stream.write_all(line.as_bytes()).is_err() {
            break;
        }
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = printer.join();
    Ok(())
}
