//! CLI: set / get / list / audit / policy-allow

use httpc::args::flag;
use std::path::PathBuf;

use crate::audit::Audit;
use crate::policy::Policy;
use crate::store::Store;

fn store_dir(args: &[String]) -> Result<PathBuf, String> {
    flag(args, "--store").map(PathBuf::from).ok_or_else(|| "--store required".into())
}

pub fn run(args: &[String]) -> Result<String, String> {
    let dir = store_dir(args)?;
    match args.first().map(String::as_str) {
        Some("set") => {
            let name = flag(args, "--name").ok_or("--name required")?;
            let value = flag(args, "--value").ok_or("--value required")?;
            Store::open(&dir)?.set(&name, &value)?;
            Audit::open(&dir)?.event("set", &name)?;
            Ok(format!("set {name}"))
        }
        Some("get") => {
            let name = flag(args, "--name").ok_or("--name required")?;
            let caller = flag(args, "--as").ok_or("--as CALLER required (policy-gated)")?;
            // Authenticate the caller BEFORE the policy check — the caller
            // string is self-asserted; without a token the whole gate is
            // decorative. Token comes from env (injected by the launch path,
            // scrubbed inside the sandbox) or --token.
            let presented = flag(args, "--token").or_else(|| std::env::var("SMARTAGENT_CALLER_TOKEN").ok());
            if let Err(e) = crate::token::verify(&dir, &caller, presented.as_deref()) {
                Audit::open(&dir)?.event("auth-fail", &format!("{caller}->{name}"))?;
                return Err(format!("DENIED: {e}"));
            }
            let permitted = Policy::open(&dir)?.permitted(&caller, &name)?;
            Audit::open(&dir)?.record(&caller, &name, permitted)?;
            if !permitted {
                return Err(format!("DENIED: '{caller}' may not read '{name}' (grant with policy-allow)"));
            }
            match Store::open(&dir)?.get(&name)? {
                Some(v) => Ok(v),
                None => Err(format!("secret '{name}' not found")),
            }
        }
        Some("list") => {
            Audit::open(&dir)?.event("list", "*")?;
            let names = Store::open(&dir)?.list()?;
            Ok(if names.is_empty() { "no secrets".into() } else { names.join("\n") })
        }
        Some("audit") => {
            let entries = Audit::open(&dir)?.entries()?;
            Ok(if entries.is_empty() { "no audit entries".into() } else { entries.join("\n") })
        }
        Some("policy-allow") => {
            // Granting access is an ADMIN operation, deliberately kept off the
            // agent's tool surface (secrets.ts exposes only get/list/audit).
            // A self-granting agent is the whole threat, so require an
            // out-of-band signal the agent's normal launch path does not carry.
            if std::env::var("SMARTAGENT_SECRETS_ADMIN").as_deref() != Ok("1") {
                return Err("policy-allow is admin-only: rerun with SMARTAGENT_SECRETS_ADMIN=1".into());
            }
            let caller = flag(args, "--caller").ok_or("--caller required")?;
            let name = flag(args, "--name").ok_or("--name required")?;
            Policy::open(&dir)?.allow(&caller, &name)?;
            Audit::open(&dir)?.event("policy-allow", &format!("{caller}->{name}"))?;
            Ok(format!("granted {caller} → {name}"))
        }
        Some("issue-token") => {
            // Admin-only, like policy-allow: issuing an identity IS a grant.
            if std::env::var("SMARTAGENT_SECRETS_ADMIN").as_deref() != Ok("1") {
                return Err("issue-token is admin-only: rerun with SMARTAGENT_SECRETS_ADMIN=1".into());
            }
            let caller = flag(args, "--caller").ok_or("--caller required")?;
            let token = crate::token::issue(&dir, &caller)?;
            Audit::open(&dir)?.event("issue-token", &caller)?;
            Ok(token)
        }
        _ => Ok(HELP.trim().into()),
    }
}


const HELP: &str = r#"
secrets — policy-gated, audited secret store (deny by default)

USAGE:
  secrets set          --store DIR --name N --value V
  secrets get          --store DIR --name N --as CALLER   (token-authenticated + policy-checked + audited;
                        token via SMARTAGENT_CALLER_TOKEN env or --token)
  secrets issue-token  --store DIR --caller C   (ADMIN: mint/rotate a caller token, prints it once)
  secrets list         --store DIR
  secrets audit        --store DIR
  secrets policy-allow --store DIR --caller C --name N   (use N=* for all)

Values are obfuscated at rest (not strong crypto). All data in semdb tables.
"#;
