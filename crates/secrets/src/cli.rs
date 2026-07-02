//! CLI: set / get / list / audit / policy-allow

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
            Ok(format!("set {name}"))
        }
        Some("get") => {
            let name = flag(args, "--name").ok_or("--name required")?;
            let caller = flag(args, "--as").ok_or("--as CALLER required (policy-gated)")?;
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
            let names = Store::open(&dir)?.list()?;
            Ok(if names.is_empty() { "no secrets".into() } else { names.join("\n") })
        }
        Some("audit") => {
            let entries = Audit::open(&dir)?.entries()?;
            Ok(if entries.is_empty() { "no audit entries".into() } else { entries.join("\n") })
        }
        Some("policy-allow") => {
            let caller = flag(args, "--caller").ok_or("--caller required")?;
            let name = flag(args, "--name").ok_or("--name required")?;
            Policy::open(&dir)?.allow(&caller, &name)?;
            Ok(format!("granted {caller} → {name}"))
        }
        _ => Ok(HELP.trim().into()),
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

const HELP: &str = r#"
secrets — policy-gated, audited secret store (deny by default)

USAGE:
  secrets set          --store DIR --name N --value V
  secrets get          --store DIR --name N --as CALLER   (policy-checked + audited)
  secrets list         --store DIR
  secrets audit        --store DIR
  secrets policy-allow --store DIR --caller C --name N   (use N=* for all)

Values are obfuscated at rest (not strong crypto). All data in semdb tables.
"#;
