//! Runtime configuration: located in `config/smartagent.conf` at the repo root
//! (walked up from the current directory). No endpoint is ever hardcoded in
//! logic — defaults live in the config file, overridable by flag or env.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Config {
    map: HashMap<String, String>,
}

impl Config {
    /// Load from `config/smartagent.conf`, searching upward from cwd. Missing
    /// file yields an empty config (callers then require a flag/env value).
    pub fn load() -> Config {
        let mut map = HashMap::new();
        if let Some(path) = find() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                map = parse_map(&text);
            }
        }
        Config { map }
    }

    /// Resolution order: explicit override (flag) → env var → config file.
    pub fn resolve(&self, key: &str, env: &str, flag: Option<&str>) -> Option<String> {
        if let Some(f) = flag {
            return Some(f.to_string());
        }
        if let Ok(v) = std::env::var(env) {
            if !v.is_empty() {
                return Some(v);
            }
        }
        self.map.get(key).cloned()
    }

    /// Absolute workspaces root: config `workspaces_dir` resolved against the
    /// repo root (the dir containing config/smartagent.conf). Defaults to
    /// `<repo>/workspaces`. All crates align to this single location.
    pub fn workspaces_dir(&self) -> PathBuf {
        self.repo_relative("workspaces_dir", "workspaces")
    }

    /// Absolute data dir for durable stores; defaults to `<repo>/data`.
    pub fn data_dir(&self) -> PathBuf {
        self.repo_relative("data_dir", "data")
    }

    /// Unix socket the semdb daemon binds; defaults to `<repo>/.pi/semdb.sock`.
    /// Repo-relative so the client and daemon agree regardless of cwd. The env
    /// override (used by tests for an isolated socket) takes an absolute path.
    pub fn semdb_socket(&self) -> PathBuf {
        if let Ok(s) = std::env::var("SMARTAGENT_SEMDB_SOCKET") {
            if !s.is_empty() {
                return PathBuf::from(s);
            }
        }
        self.repo_relative("semdb_socket", ".pi/semdb.sock")
    }

    fn repo_relative(&self, key: &str, default: &str) -> PathBuf {
        let rel = self.map.get(key).map(String::as_str).unwrap_or(default);
        let base = find()
            .and_then(|p| p.parent().and_then(|d| d.parent()).map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(rel)
    }
}

fn parse_map(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = strip_inline_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), unquote(v.trim()).to_string());
        }
    }
    map
}

fn strip_inline_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut prev_ws = true;
    for (i, c) in line.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if c == '#' && prev_ws => return &line[..i],
            None => {}
        }
        prev_ws = c.is_whitespace();
    }
    line
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn find() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join("config").join("smartagent.conf");
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// The default embedding model, in ONE place (was copy-pasted in 4 crates).
pub const DEFAULT_EMBED_MODEL: &str = "embeddinggemma";

impl Config {
    /// Resolve the embeddings backend as `(host, port, model)`. Optional flag
    /// overrides (`--endpoint`, `--model`) take precedence over env/config.
    /// Centralizes the `host:port` parse and the model default so every crate
    /// that embeds agrees.
    pub fn embeddings(
        &self,
        endpoint_flag: Option<&str>,
        model_flag: Option<&str>,
    ) -> Result<(String, u16, String), String> {
        let ep = self
            .resolve("embeddings_endpoint", "SEMDB_ENDPOINT", endpoint_flag)
            .ok_or("no embeddings endpoint: set embeddings_endpoint in config/smartagent.conf, $SEMDB_ENDPOINT, or --endpoint")?;
        let (host, port) = ep.rsplit_once(':').ok_or("endpoint must be host:port")?;
        let port: u16 = port.parse().map_err(|_| "bad port in embeddings_endpoint")?;
        let model = self
            .resolve("embeddings_model", "SEMDB_MODEL", model_flag)
            .unwrap_or_else(|| DEFAULT_EMBED_MODEL.to_string());
        Ok((host.to_string(), port, model))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_beats_env_beats_file() {
        let cfg = Config { map: HashMap::from([("k".into(), "fromfile".into())]) };
        assert_eq!(cfg.resolve("k", "NOPE_ENV", Some("flag")).as_deref(), Some("flag"));
        assert_eq!(cfg.resolve("k", "NOPE_ENV", None).as_deref(), Some("fromfile"));
        assert_eq!(cfg.resolve("missing", "NOPE_ENV", None), None);
    }

    #[test]
    fn strips_inline_comments_without_losing_literal_hashes() {
        let cfg = Config {
            map: parse_map(
                "embeddings_endpoint = http://host/path#frag # comment\nquoted = 'value # kept' # comment\nplain = value # comment\nliteral = value#kept\n",
            ),
        };
        assert_eq!(cfg.map["embeddings_endpoint"], "http://host/path#frag");
        assert_eq!(cfg.map["quoted"], "value # kept");
        assert_eq!(cfg.map["plain"], "value");
        assert_eq!(cfg.map["literal"], "value#kept");
    }
}
