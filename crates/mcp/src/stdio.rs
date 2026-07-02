//! stdio transport: spawn an MCP server process and speak line-delimited
//! JSON-RPC over its stdin/stdout.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

use httpc::json::Value;

use crate::jsonrpc;

pub struct StdioClient {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl StdioClient {
    /// Spawn `cmd` (a shell command string) and run the initialize handshake.
    pub fn start(cmd: &str) -> Result<StdioClient, String> {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn '{cmd}': {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let reader = BufReader::new(stdout);
        let mut c = StdioClient { child, stdin, reader, next_id: 1 };
        c.call("initialize", jsonrpc::INIT_PARAMS)?;
        c.notify("notifications/initialized", "{}")?;
        Ok(c)
    }

    pub fn call(&mut self, method: &str, params: &str) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_line(&jsonrpc::request(id, method, params))?;
        // Read lines until the matching id response (skip notifications/events).
        for _ in 0..500 {
            let line = self.read_line()?;
            if line.trim().is_empty() {
                continue;
            }
            if jsonrpc::response_id(&line) == Some(id as f64) {
                return jsonrpc::result(&line);
            }
        }
        Err("no response for stdio call".into())
    }

    pub fn notify(&mut self, method: &str, params: &str) -> Result<(), String> {
        self.write_line(&jsonrpc::notification(method, params))
    }

    fn write_line(&mut self, msg: &str) -> Result<(), String> {
        self.stdin.write_all(msg.as_bytes()).map_err(|e| e.to_string())?;
        self.stdin.write_all(b"\n").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())
    }

    fn read_line(&mut self) -> Result<String, String> {
        let mut line = String::new();
        let n = self.reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("server closed stdout".into());
        }
        Ok(line)
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
