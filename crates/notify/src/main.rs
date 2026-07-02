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
                auth: std::env::var("NTFY_TOKEN").ok(),
            };
            ntfy::send(&msg)
        }
        _ => Ok(HELP.trim().into()),
    }
}

const HELP: &str = r#"
notify — push notifications (ntfy protocol, HTTP/HTTPS)

USAGE:
  notify send --topic T --message M [--title X] [--priority 1-5]
              [--tags tag1,tag2] [--click URL] [--markdown]
              [--server http(s)://host:port]   (bearer auth via $NTFY_TOKEN)
"#;
