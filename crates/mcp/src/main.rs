//! mcp CLI: tools / call  (--cmd for stdio, --url for HTTP)

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
            let tools = jsonrpc::parse_tools(&res);
            if tools.is_empty() { return Ok("no tools".into()); }
            Ok(tools.iter().map(|(n, d)| format!("{n}\t{d}")).collect::<Vec<_>>().join("\n"))
        }
        Some("call") => {
            let tool = flag(args, "--tool").ok_or("--tool required")?;
            let call_args = flag(args, "--args").unwrap_or_else(|| "{}".into());
            let params = format!(r#"{{"name":"{}","arguments":{}}}"#, tool, call_args);
            let mut t = Transport::open(args)?;
            let res = t.call("tools/call", &params)?;
            Ok(jsonrpc::parse_call(&res))
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

const HELP: &str = r#"
mcp — Model Context Protocol client (stdio + HTTP)

USAGE:
  mcp tools (--cmd '<server command>' | --url URL)
  mcp call  (--cmd '...' | --url URL) --tool NAME --args '<json>'
"#;
