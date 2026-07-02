//! stdio transport: spawn an MCP server process and speak line-delimited
//! JSON-RPC over its stdin/stdout.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};

use httpc::json::Value;

use crate::jsonrpc;

pub struct StdioClient {
    child: Child,
    stdin: ChildStdin,
    /// Lines from a reader thread — lets read_line time out instead of
    /// hanging forever on a silent server.
    lines: Receiver<String>,
    stderr_buf: std::sync::Arc<std::sync::Mutex<String>>,
    next_id: u64,
    pub read_timeout_secs: u64,
}

impl StdioClient {
    /// Spawn the MCP server and run the initialize handshake. `cmd` is tokenized
    /// into argv and exec'd directly — NO shell — so a server command string can
    /// never smuggle `;`, `|`, `$(...)` or other shell metacharacters into
    /// arbitrary command execution.
    pub fn start(cmd: &str) -> Result<StdioClient, String> {
        let argv = tokenize(cmd)?;
        let (program, rest) = argv.split_first().ok_or("empty server command")?;
        let mut child = Command::new(program)
            .args(rest)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn '{cmd}': {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        // Reader thread → channel, so calls can time out. Stderr captured to a
        // buffer (was /dev/null — silent failures were undiagnosable).
        let (tx, lines): (Sender<String>, Receiver<String>) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let stderr_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        if let Some(mut se) = child.stderr.take() {
            let buf = stderr_buf.clone();
            std::thread::spawn(move || {
                let mut s = String::new();
                let _ = se.read_to_string(&mut s);
                if let Ok(mut b) = buf.lock() {
                    b.push_str(&s);
                }
            });
        }
        let mut c = StdioClient {
            child,
            stdin,
            lines,
            stderr_buf,
            next_id: 1,
            read_timeout_secs: 30,
        };
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
        self.stdin
            .write_all(msg.as_bytes())
            .map_err(|e| e.to_string())?;
        self.stdin.write_all(b"\n").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())
    }

    fn read_line(&mut self) -> Result<String, String> {
        match self
            .lines
            .recv_timeout(std::time::Duration::from_secs(self.read_timeout_secs))
        {
            Ok(line) => Ok(line),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(format!(
                "server silent for {}s{}",
                self.read_timeout_secs,
                self.stderr_tail()
            )),
            Err(_) => Err(format!("server closed stdout{}", self.stderr_tail())),
        }
    }

    /// Last captured stderr (diagnostics for silent/dead servers).
    fn stderr_tail(&self) -> String {
        let b = self
            .stderr_buf
            .lock()
            .map(|b| b.clone())
            .unwrap_or_default();
        let t = b.trim();
        if t.is_empty() {
            String::new()
        } else {
            format!("; server stderr: {}", &t[t.len().saturating_sub(400)..])
        }
    }
}

/// Split a command string into argv, honoring single/double quotes so paths
/// with spaces work. Deliberately does NOT interpret shell metacharacters —
/// `;`, `|`, `$`, backticks are literal text, not operators.
fn tokenize(cmd: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut in_token = false;
    for c in cmd.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                in_token = true;
            }
            None if c.is_whitespace() => {
                if in_token {
                    out.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            None => {
                cur.push(c);
                in_token = true;
            }
        }
    }
    if quote.is_some() {
        return Err("unterminated quote in server command".into());
    }
    if in_token {
        out.push(cur);
    }
    if out.is_empty() {
        return Err("empty server command".into());
    }
    Ok(out)
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
