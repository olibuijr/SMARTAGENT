use std::process::Command;

const ALLOWED_SECRET: &str = "mail_olibuijr_password";
const TOKEN_PATH: &str = "data/secrets/tokens/pi.token";
const WORKTREE_TOKEN_PATH: &str = "../../data/secrets/tokens/pi.token";

fn main() {
    if let Err(e) = run(std::env::args().skip(1).collect()) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return Ok(());
    }
    let name = args.first().map(String::as_str).unwrap_or(ALLOWED_SECRET);
    if name != ALLOWED_SECRET {
        return Err(format!("secret {name:?} is not allowed by this mail bridge"));
    }
    let value = get_secret(name)?;
    print!("{value}");
    Ok(())
}

fn get_secret(name: &str) -> Result<String, String> {
    let store = if std::path::Path::new("data/secrets").exists() {
        "data/secrets"
    } else {
        "../../data/secrets"
    };
    let mut cmd = Command::new("target/release/secrets");
    cmd.args(["get", "--store", store, "--name", name, "--as", "pi"]);
    if std::env::var("SMARTAGENT_CALLER_TOKEN").is_err() {
        let token_path = if std::path::Path::new(TOKEN_PATH).exists() { TOKEN_PATH } else { WORKTREE_TOKEN_PATH };
        if let Ok(tok) = std::fs::read_to_string(token_path) {
            cmd.env("SMARTAGENT_CALLER_TOKEN", tok.trim());
        }
    }
    let out = cmd.output().map_err(|e| format!("run secrets get: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(stderr.trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
}

fn print_help() {
    println!("mail-secret — policy-gated Himalaya password bridge");
    println!();
    println!("USAGE:");
    println!("  mail-secret [mail_olibuijr_password]");
    println!();
    println!("Reads only the approved mail secret through target/release/secrets as caller pi.");
    println!("If SMARTAGENT_CALLER_TOKEN is absent, a trusted repo/worktree subprocess may use the pi token file.");
    println!("The secret value is written to stdout for Himalaya password command use; do not run manually in logs.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_other_secret_names() {
        let err = run(vec!["telegram_bot_token".into()]).unwrap_err();
        assert!(err.contains("not allowed"));
    }
}
