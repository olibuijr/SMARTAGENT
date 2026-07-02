//! Secret store backed by a semdb table. Values are obfuscated at rest by
//! XOR against a keystream derived (hash-chain) from a random per-store key
//! file. NOTE: this is OBFUSCATION, not strong cryptography — it stops casual
//! disk inspection, not a determined attacker. Strong crypto (a vetted AEAD)
//! is a later addition once we allow a reviewed dependency.

use std::path::{Path, PathBuf};

use semdb::storage::Db;

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
        let obf = hex(&xor_stream(value.as_bytes(), &self.key, name));
        let mut db = self.db()?;
        db.put(name, &obf, PLACEHOLDER_VEC.to_vec())?;
        // The log is append-only, so an overwritten secret's previous
        // obfuscated value would linger on disk. Compact after every write
        // (the table is tiny) so rotated/removed values don't persist.
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
                let bytes = unhex(&e.meta)?;
                let plain = xor_stream(&bytes, &self.key, name);
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

/// Keystream = repeated hash-chain of key ++ salt; XOR into the data.
fn xor_stream(data: &[u8], key: &[u8], salt: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut state = fnv_seed(key, salt);
    for (i, b) in data.iter().enumerate() {
        if i % 8 == 0 {
            state = fnv_step(state);
        }
        let ks = (state >> ((i % 8) * 8)) as u8;
        out.push(b ^ ks);
    }
    out
}

fn fnv_seed(key: &[u8], salt: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in key.iter().chain(salt.as_bytes()) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

fn fnv_step(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h
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
