//! notify — ntfy-style push notifications over plain HTTP (via httpc).
//! `notify send --topic T --message M [--title X] [--priority 1-5] [--tags a,b]
//!         [--server http://host:port]`

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
            let msg = ntfy::Message {
                server: flag(args, "--server").unwrap_or_else(|| "http://ntfy.sh".into()),
                topic: flag(args, "--topic").ok_or("--topic required")?,
                message: flag(args, "--message").ok_or("--message required")?,
                title: flag(args, "--title"),
                priority: flag(args, "--priority"),
                tags: flag(args, "--tags"),
            };
            ntfy::send(&msg)
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

const HELP: &str = r#"
notify — push notifications (ntfy protocol, plain HTTP)

USAGE:
  notify send --topic T --message M [--title X] [--priority 1-5]
              [--tags tag1,tag2] [--server http://host:port]
"#;
