//! Caller authentication tokens. `get --as CALLER` used to trust the caller
//! string outright, making the deny-by-default policy decorative — any process
//! could claim a granted identity. Now each caller has a random token, issued
//! by an admin and stored 0600 under <store>/tokens/. The agent's launch path
//! exports it as SMARTAGENT_CALLER_TOKEN; the sandbox scrubs env and
//! tmpfs-masks the store dir, so a sandboxed/injected command can neither
//! inherit nor read it.

use std::io::Read;
use std::path::{Path, PathBuf};

fn token_path(dir: &Path, caller: &str) -> Result<PathBuf, String> {
    if caller.is_empty() || !caller.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("caller must be [A-Za-z0-9_-]+".into());
    }
    Ok(dir.join("tokens").join(format!("{caller}.token")))
}

/// Issue (or rotate) a caller's token: 32 random bytes from /dev/urandom, hex.
pub fn issue(dir: &Path, caller: &str) -> Result<String, String> {
    let path = token_path(dir, caller)?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .map_err(|e| format!("urandom: {e}"))?;
    let token: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(&path, &token).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(token)
}

/// Verify a presented token against the caller's stored token, constant-time.
/// Fail-closed: a caller with no issued token is always rejected.
pub fn verify(dir: &Path, caller: &str, presented: Option<&str>) -> Result<(), String> {
    let path = token_path(dir, caller)?;
    let stored = std::fs::read_to_string(&path)
        .map_err(|_| format!("no token issued for caller '{caller}' (admin: secrets issue-token --caller {caller})"))?;
    let presented = presented.ok_or("caller token required: set SMARTAGENT_CALLER_TOKEN or pass --token")?;
    if ct_eq(stored.trim().as_bytes(), presented.trim().as_bytes()) {
        Ok(())
    } else {
        Err(format!("invalid token for caller '{caller}'"))
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn issue_verify_reject() {
        let d = scratch("sec-token");
        let t = issue(&d, "pi").unwrap();
        assert_eq!(t.len(), 64);
        assert!(verify(&d, "pi", Some(&t)).is_ok());
        assert!(verify(&d, "pi", Some("wrong")).is_err());
        assert!(verify(&d, "pi", None).is_err());
        // no token issued → fail closed
        assert!(verify(&d, "ghost", Some(&t)).is_err());
        // rotation invalidates the old token
        let t2 = issue(&d, "pi").unwrap();
        assert!(verify(&d, "pi", Some(&t)).is_err());
        assert!(verify(&d, "pi", Some(&t2)).is_ok());
    }

    #[test]
    fn rejects_path_traversal_caller() {
        let d = scratch("sec-token-trav");
        assert!(issue(&d, "../evil").is_err());
        assert!(issue(&d, "").is_err());
    }
}
