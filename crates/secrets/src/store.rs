//! Secret store backed by a semdb table. Values are encrypted at rest with
//! ChaCha20-Poly1305 AEAD (see aead.rs, verified against RFC 8439 vectors):
//! a per-secret random 96-bit nonce, the 256-bit key from `secrets.key`
//! (0600, /dev/urandom), and the secret NAME as associated data so a value
//! can't be moved to another name. Tampering fails the tag on read.

use std::io::Read;
use std::path::{Path, PathBuf};

use semdb::storage::Db;

use crate::aead;

const PLACEHOLDER_VEC: [f32; 1] = [0.0]; // non-semantic table → zero vector

pub struct Store {
    dir: PathBuf,
    key: Vec<u8>,
}

impl Store {
    pub fn open(dir: &Path) -> Result<Store, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let key = load_or_make_key(dir)?;
        Ok(Store { dir: dir.to_path_buf(), key })
    }

    fn table(&self) -> PathBuf {
        self.dir.join("secrets.semdb")
    }

    fn db(&self) -> Result<Db, String> {
        let p = self.table();
        if p.exists() { Db::open(&p) } else { Db::create(&p) }
    }

    pub fn set(&self, name: &str, value: &str) -> Result<(), String> {
        let mut key = [0u8; 32];
        key.copy_from_slice(&self.key[..32]);
        let nonce = random_nonce()?;
        let sealed = aead::seal(&key, &nonce, name.as_bytes(), value.as_bytes());
        let mut db = self.db()?;
        db.put(name, &hex(&sealed), PLACEHOLDER_VEC.to_vec())?;
        // The log is append-only, so an overwritten secret's previous
        // ciphertext would linger on disk. Compact after every write (the
        // table is tiny) so rotated/removed values don't persist.
        db.compact()
    }

    pub fn get(&self, name: &str) -> Result<Option<String>, String> {
        let p = self.table();
        if !p.exists() {
            return Ok(None);
        }
        let db = Db::open(&p)?;
        match db.get(name) {
            Some(e) => {
                let mut key = [0u8; 32];
                key.copy_from_slice(&self.key[..32]);
                let sealed = unhex(&e.meta)?;
                let plain = aead::open(&key, name.as_bytes(), &sealed)
                    .ok_or("secret failed authentication (tampered, wrong key, or corrupt)")?;
                Ok(Some(String::from_utf8(plain).map_err(|_| "corrupt value")?))
            }
            None => Ok(None),
        }
    }

    pub fn list(&self) -> Result<Vec<String>, String> {
        let p = self.table();
        if !p.exists() {
            return Ok(Vec::new());
        }
        let db = Db::open(&p)?;
        let mut names: Vec<String> = db.index.keys().cloned().collect();
        names.sort();
        Ok(names)
    }
}

fn load_or_make_key(dir: &Path) -> Result<Vec<u8>, String> {
    let path = dir.join("secrets.key");
    if let Ok(k) = std::fs::read(&path) {
        if k.len() >= 32 {
            return Ok(k);
        }
    }
    // Read 32 bytes of real entropy from the OS CSPRNG. A time^pid xorshift
    // seed (the old approach) lives in a tiny, searchable space; /dev/urandom
    // does not. At-rest protection still rests on the 0600 key file, not the
    // XOR cipher — but the key itself must not be guessable.
    // read_exact (not fs::read) — /dev/urandom is an endless stream with no EOF.
    let mut key = vec![0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| {
            use std::io::Read;
            f.read_exact(&mut key)
        })
        .map_err(|e| format!("read /dev/urandom: {e}"))?;
    std::fs::write(&path, &key).map_err(|e| e.to_string())?;
    // Best-effort 0600 perms.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

/// 96-bit AEAD nonce from the OS CSPRNG. A fresh nonce per `set` is required —
/// reusing a (key, nonce) pair breaks ChaCha20-Poly1305.
fn random_nonce() -> Result<[u8; 12], String> {
    let mut n = [0u8; 12];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut n))
        .map_err(|e| format!("read /dev/urandom: {e}"))?;
    Ok(n)
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn unhex(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("odd hex length".into());
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "bad hex".to_string())).collect()
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
    fn roundtrip_including_large_and_unicode() {
        let s = Store::open(&scratch("sec-store")).unwrap();
        s.set("api", "hunter2").unwrap();
        s.set("big", &"x".repeat(9000)).unwrap();
        s.set("ice", "lykilorð-öæð").unwrap();
        assert_eq!(s.get("api").unwrap().as_deref(), Some("hunter2"));
        assert_eq!(s.get("big").unwrap().unwrap().len(), 9000);
        assert_eq!(s.get("ice").unwrap().as_deref(), Some("lykilorð-öæð"));
        assert_eq!(s.get("missing").unwrap(), None);
    }

    #[test]
    fn value_is_obfuscated_on_disk() {
        let dir = scratch("sec-obf");
        let s = Store::open(&dir).unwrap();
        s.set("token", "PLAINTEXT_SECRET").unwrap();
        let raw = std::fs::read(dir.join("secrets.semdb")).unwrap();
        let hay = String::from_utf8_lossy(&raw);
        assert!(!hay.contains("PLAINTEXT_SECRET"));
    }
}
