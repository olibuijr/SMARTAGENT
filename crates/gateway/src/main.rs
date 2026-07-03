//! gateway — persistent agent service (meðvitund).
//!
//! Owns long-running `./pi --mode rpc` children so agents survive client
//! disconnects; clients attach/detach over a unix socket. A heartbeat keeps
//! each agent aware of time, its board, and the plan ahead, and every beat and
//! turn lands in the `medvitund` semdb table. Design: Plans/MEDVITUND.md.

mod agents_view;
mod beat;
mod child;
mod roster;
mod server;

use std::io::ErrorKind;
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
  gateway autonomy [on|off]                     fleet-wide autonomous pickup switch
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
        "ask" => {
            let (agent, msg) = parse_client_args(&rest);
            let secs = httpc::args::flag(&rest, "--timeout")
                .and_then(|s| s.parse().ok())
                .unwrap_or(90);
            let stream = rest.iter().any(|a| a == "--stream");
            client_ask(&agent, &msg, secs, stream)
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
        "autonomy" => client_send(
            "autonomy",
            "",
            rest.first().map(String::as_str).unwrap_or(""),
        ),
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

/// Request→reply: send a message and print the agent's reply turn (the Text
/// broadcast), giving up after `timeout_secs` of silence. Used by the Telegram
/// bridge — a chat needs an answer, not a delivery receipt.
const STREAM_INFO_PREFIX: &str = "__SMARTAGENT_INFO__";

fn stream_info_line(text: &str) -> String {
    format!("{STREAM_INFO_PREFIX}{}", text.replace('\n', " "))
}

fn client_ask(
    agent: &str,
    message: &str,
    timeout_secs: u64,
    stream_mode: bool,
) -> Result<(), String> {
    let mut stream = connect()?;
    let line = format!(
        "{{\"op\":\"ask\",\"agent\":\"{}\",\"message\":\"{}\"}}\n",
        httpc::json::escape(agent),
        httpc::json::escape(message)
    );
    stream
        .write_all(line.as_bytes())
        .map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(timeout_secs)))
        .ok();
    let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut reply = String::new();
    for l in reader.lines() {
        let Ok(l) = l else { break }; // read timeout → stop waiting
        match server::client_line_kind(&l) {
            server::LineKind::Text(t) => {
                reply.push_str(&t);
                // --stream: emit the growing reply as newline-delimited
                // snapshots so the caller (Telegram bridge) can live-edit the
                // message as tokens arrive.
                if stream_mode {
                    println!("{}", reply.replace('\n', "\\n"));
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                }
            }
            // "done" fires at each turn end; return once we have some reply.
            server::LineKind::Done => {
                if !reply.trim().is_empty() {
                    break;
                }
            }
            server::LineKind::Info(t) => {
                if stream_mode && t.starts_with('🛠') {
                    println!("{}", stream_info_line(&t));
                    use std::io::Write as _;
                    let _ = std::io::stdout().flush();
                }
            }
        }
    }
    if !stream_mode {
        print!("{}", reply.trim());
    }
    Ok(())
}

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
            server::LineKind::Text(t) => {
                if !write_stdout(&t)? {
                    return Ok(());
                }
            }
            server::LineKind::Info(t) => {
                if !write_stdout_line(&t)? {
                    return Ok(());
                }
            }
            server::LineKind::Done => {
                let _ = write_stdout_line("")?;
                return Ok(());
            }
        }
    }
    Ok(())
}

/// One-line statusline; compactly prefers main, or all agents via `agents`.
fn statusline() -> Result<(), String> {
    let Ok(mut stream) = UnixStream::connect(socket_path()) else {
        let _ = write_stdout_line("warn|⏲ gateway down")?;
        return Ok(());
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    let agent = default_agent();
    let line = format!(
        "{{\"op\":\"status\",\"agent\":\"{}\"}}\n",
        httpc::json::escape(&agent)
    );
    if stream.write_all(line.as_bytes()).is_err() {
        let _ = write_stdout_line("warn|⏲ gateway unreachable")?;
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
                format!(" · {}", human_tokens(&tokens))
            };
            if doing.is_empty() || doing == "nothing" {
                let _ = write_stdout_line(&format!("ok|⏲ {state} · {beat}{tok}"))?;
            } else {
                let _ = write_stdout_line(&format!("ok|⏲ {state} · {beat} · {doing}{tok}"))?;
            }
            return Ok(());
        }
    }
    let _ = write_stdout_line("warn|⏲ gateway no answer")?;
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
                    if !write_stdout(&t).unwrap_or(false) {
                        break;
                    }
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

fn human_tokens(raw: &str) -> String {
    let Ok(n) = raw.trim().parse::<u64>() else {
        return format!("{raw}tok");
    };
    if n >= 1_000_000 {
        format!("{:.1}m", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("{n}tok")
    }
}

fn write_stdout(text: &str) -> Result<bool, String> {
    let mut out = std::io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|_| out.flush()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

fn write_stdout_line(text: &str) -> Result<bool, String> {
    write_stdout(&format!("{text}\n"))
}

#[cfg(test)]
mod tests {
    use super::{human_tokens, stream_info_line};

    #[test]
    fn stream_info_line_has_marker_and_no_newlines() {
        assert_eq!(
            stream_info_line("🛠 memory running…\nraw"),
            "__SMARTAGENT_INFO__🛠 memory running… raw"
        );
    }

    #[test]
    fn statusline_humanizes_token_counts() {
        assert_eq!(human_tokens("999"), "999tok");
        assert_eq!(human_tokens("6257633"), "6.3m");
        assert_eq!(human_tokens("12500"), "12.5k");
    }
}
