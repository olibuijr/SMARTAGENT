//! notify — ntfy-style push notifications over HTTP/HTTPS (via httpc).
//! `notify send --topic T --message M [--title X] [--priority 1-5] [--tags a,b]
//!         [--server http://host:port]`

use httpc::args::flag;
use std::process::ExitCode;

mod ntfy;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    match args.first().map(String::as_str) {
        Some("send") => {
            let server = semdb::config::Config::load()
                .resolve("ntfy_server", "NTFY_SERVER", flag(args, "--server").as_deref())
                .ok_or("no ntfy server: set ntfy_server in config/smartagent.conf, $NTFY_SERVER, or --server")?;
            let msg = ntfy::Message {
                server,
                topic: flag(args, "--topic").ok_or("--topic required")?,
                message: flag(args, "--message").ok_or("--message required")?,
                title: flag(args, "--title"),
                priority: flag(args, "--priority"),
                tags: flag(args, "--tags"),
                click: flag(args, "--click"),
                markdown: args.iter().any(|a| a == "--markdown"),
                auth: ntfy_token_from_secrets()?,
            };
            ntfy::send(&msg)
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn ntfy_token_from_secrets() -> Result<Option<String>, String> {
    let out = std::process::Command::new("target/release/secrets")
        .args(["get", "--store", "data/secrets", "--name", "ntfy_token", "--as", "pi"])
        .output()
        .map_err(|e| format!("run secrets get: {e}"))?;
    if out.status.success() {
        let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
        return Ok((!token.is_empty()).then_some(token));
    }
    let err = String::from_utf8_lossy(&out.stderr);
    if err.contains("DENIED") || err.contains("not found") || err.contains("no such") {
        Ok(None)
    } else {
        Err(err.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_points_to_policy_gated_secret_not_env_token() {
        assert!(HELP.contains("policy-gated secret `ntfy_token`"), "{HELP}");
        assert!(!HELP.contains("NTFY_TOKEN"), "{HELP}");
    }
}

const HELP: &str = r#"
notify — push notifications (ntfy protocol, HTTP/HTTPS)

USAGE:
  notify send --topic T --message M [--title X] [--priority 1-5]
              [--tags tag1,tag2] [--click URL] [--markdown]
              [--server http(s)://host:port]

Bearer auth, when configured, is read from policy-gated secret `ntfy_token`.
"#;
