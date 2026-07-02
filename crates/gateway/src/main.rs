//! gateway — persistent agent service (meðvitund).
//!
//! Owns a long-running `./pi --mode rpc` child so the agent survives client
//! disconnects; clients attach/detach over a unix socket. A heartbeat keeps
//! the agent aware of time, its board, and the plan ahead, and every beat and
//! turn lands in the `medvitund` semdb table (the agent's interviewable
//! self-history). Design: Plans/MEDVITUND.md.

mod beat;
mod child;
mod server;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

fn usage() -> ! {
    eprintln!(
        "gateway — persistent agent service (meðvitund)

USAGE:
  gateway serve [--agent NAME] [--heartbeat-secs N] [--autonomous]
  gateway send <message...>       one-shot message; prints the reply
  gateway steer <message...>      inject while the agent is working
  gateway attach                  stream output; stdin lines are sent (Ctrl-D detaches)
  gateway status                  agent state, uptime, last beat
  gateway stop                    graceful shutdown (session is preserved)

Socket: config gateway_socket (default .pi/gateway.sock). Config keys:
gateway_agent, heartbeat_secs (default 120)."
    );
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();
    let result = match cmd {
        "serve" => server::serve(&rest),
        "send" => client_send("send", &rest.join(" ")),
        "steer" => client_send("steer", &rest.join(" ")),
        "attach" => client_attach(),
        "status" => client_send("status", ""),
        "agents" => client_send("agents", ""),
        "statusline" => statusline(),
        "stop" => client_send("stop", ""),
        _ => usage(),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
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
        // resolve against the repo root (dir holding config/), like data_dir
        cfg.data_dir().parent().map(|r| r.join(&rel)).unwrap_or(p)
    }
}

fn connect() -> Result<UnixStream, String> {
    let path = socket_path();
    UnixStream::connect(&path)
        .map_err(|e| format!("gateway not running at {}: {e} (start: gateway serve)", path.display()))
}

/// One-shot op: send a line, print response lines until `done` marker.
fn client_send(op: &str, message: &str) -> Result<(), String> {
    let mut stream = connect()?;
    let line = format!(
        "{{\"op\":\"{}\",\"message\":\"{}\"}}\n",
        op,
        httpc::json::escape(message)
    );
    stream.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    for l in reader.lines() {
        let l = l.map_err(|e| e.to_string())?;
        match server::client_line_kind(&l) {
            server::LineKind::Text(t) => print!("{t}"),
            // one-shot ops: info lines (status payload, delivery mode) are the
            // useful output — stdout, so pipelines and monitors can read them
            server::LineKind::Info(t) => println!("{t}"),
            server::LineKind::Done => {
                println!();
                return Ok(());
            }
        }
    }
    Ok(())
}

/// One-line `level|icon text` for the TUI statusline: agent state, current
/// task, last beat — the real-time meðvitund pulse. Never blocks the TUI.
fn statusline() -> Result<(), String> {
    let Ok(mut stream) = UnixStream::connect(socket_path()) else {
        println!("warn|⏲ gateway down");
        return Ok(());
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    if stream.write_all(b"{\"op\":\"status\"}\n").is_err() {
        println!("warn|⏲ gateway unreachable");
        return Ok(());
    }
    let reader = BufReader::new(stream);
    for l in reader.lines() {
        let Ok(l) = l else { break };
        if let server::LineKind::Info(t) = server::client_line_kind(&l) {
            // "agent main: working | last beat 2026-07-02 18:36:47Z | queued beat: false | doing: T-45 p2 …"
            // Compact hard: the TUI grid pads every line's column to this
            // segment's width, so long text here skews the whole statusline.
            let mut state = "?".to_string();
            let mut beat = "—".to_string();
            let mut doing = String::new();
            for part in t.split(" | ") {
                if let Some(rest) = part.strip_prefix("agent ") {
                    // keep the full agent name visible: "main working"
                    state = rest.replace(": ", " ");
                } else if let Some(rest) = part.strip_prefix("last beat ") {
                    if let Some(time) = rest.split(' ').nth(1) {
                        beat = time.chars().take(5).collect();
                    }
                } else if let Some(rest) = part.strip_prefix("doing: ") {
                    doing = rest.split(' ').next().unwrap_or("").to_string();
                }
            }
            if doing.is_empty() || doing == "nothing" {
                println!("ok|⏲ {state} · {beat}");
            } else {
                println!("ok|⏲ {state} · {beat} · {doing}");
            }
            return Ok(());
        }
    }
    println!("warn|⏲ gateway no answer");
    Ok(())
}

/// Interactive attach: stream events; forward stdin lines (steer when busy).
fn client_attach() -> Result<(), String> {
    let mut stream = connect()?;
    stream
        .write_all(b"{\"op\":\"attach\"}\n")
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
    eprintln!("[gateway] attached — type to talk, Ctrl-D to detach (agent keeps running)");
    let stdin = std::io::stdin();
    for l in stdin.lock().lines() {
        let Ok(l) = l else { break };
        if l.trim().is_empty() {
            continue;
        }
        let line = format!("{{\"op\":\"send\",\"message\":\"{}\"}}\n", httpc::json::escape(&l));
        if stream.write_all(line.as_bytes()).is_err() {
            break;
        }
    }
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = printer.join();
    Ok(())
}
