//! mcp CLI: tools / call  (--cmd for stdio, --url for HTTP)

use httpc::args::{flag, has};
use std::process::ExitCode;

use mcp::{http::HttpClient, jsonrpc, stdio::StdioClient};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => { println!("{out}"); ExitCode::SUCCESS }
        Err(e) => { eprintln!("error: {e}"); ExitCode::FAILURE }
    }
}

enum Transport { Stdio(StdioClient), Http(HttpClient) }

impl Transport {
    fn open(args: &[String]) -> Result<Transport, String> {
        if let Some(cmd) = flag(args, "--cmd") {
            Ok(Transport::Stdio(StdioClient::start(&cmd)?))
        } else if let Some(url) = flag(args, "--url") {
            Ok(Transport::Http(HttpClient::start(&url)?))
        } else {
            Err("need --cmd '<server>' (stdio) or --url URL (http)".into())
        }
    }
    fn call(&mut self, method: &str, params: &str) -> Result<httpc::json::Value, String> {
        match self {
            Transport::Stdio(c) => c.call(method, params),
            Transport::Http(c) => c.call(method, params),
        }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("tools") => {
            let mut t = Transport::open(args)?;
            let res = t.call("tools/list", "{}")?;
            let mut tools = jsonrpc::parse_tools(&res);
            if let Some(f) = flag(args, "--filter") {
                let fl = f.to_ascii_lowercase();
                tools.retain(|(n, d)| n.to_ascii_lowercase().contains(&fl) || d.to_ascii_lowercase().contains(&fl));
            }
            if tools.is_empty() { return Ok("no tools".into()); }
            // --names-only: just tool names (skip the full schema descriptions).
            if has(args, "--names-only") {
                return Ok(tools.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>().join("\n"));
            }
            Ok(tools.iter().map(|(n, d)| format!("{n}\t{d}")).collect::<Vec<_>>().join("\n"))
        }
        Some("call") => {
            let tool = flag(args, "--tool").ok_or("--tool required")?;
            let call_args = flag(args, "--args").unwrap_or_else(|| "{}".into());
            let params = format!(r#"{{"name":"{}","arguments":{}}}"#, tool, call_args);
            let mut t = Transport::open(args)?;
            let res = t.call("tools/call", &params)?;
            let out = jsonrpc::parse_call(&res);
            // --head N: cap the response length (a chatty tool can flood context).
            match flag(args, "--head").and_then(|s| s.parse::<usize>().ok()) {
                Some(n) if out.chars().count() > n => {
                    Ok(format!("{}…[truncated]", out.chars().take(n).collect::<String>()))
                }
                _ => Ok(out),
            }
        }
        _ => Ok(HELP.trim().into()),
    }
}


const HELP: &str = r#"
mcp — Model Context Protocol client (stdio + HTTP)

USAGE:
  mcp tools (--cmd '<server command>' | --url URL)
  mcp call  (--cmd '...' | --url URL) --tool NAME --args '<json>'
"#;
