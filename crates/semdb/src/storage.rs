//! Log-structured single-file store with CRC32-framed records.
//! Crash-safe by construction: a torn/partial tail record fails its CRC and is
//! truncated on open. Values of any size (metadata >4KB included) are fine —
//! records are length-prefixed, not page-bound.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"SEMDB01\n";

// Inter-process advisory locking via inline `flock(2)` — std has no wrapper and
// the tree is zero-dep, so we declare the one extern we need (PLATFORM_SUPPORT
// pattern). Direct openers (gateway beat, tasks, workflow, supervise, telegram)
// share a file with the daemon and each other; without this, concurrent
// appends land at stale offsets and clobber each other's frames, and a compact
// from one writer wipes another's rows. LOCK_EX serializes the whole
// read-end→write / replay→rewrite critical section across processes.
#[cfg(unix)]
mod flock {
    use std::os::unix::io::RawFd;
    const LOCK_EX: i32 = 2;
    const LOCK_UN: i32 = 8;
    extern "C" {
        fn flock(fd: RawFd, operation: i32) -> i32;
    }
    /// RAII exclusive lock on an fd; released on drop. Blocks until acquired.
    pub struct Guard(RawFd);
    impl Guard {
        pub fn exclusive(fd: RawFd) -> Result<Guard, String> {
            // Retry on EINTR so a signal during the blocking wait doesn't lose the lock.
            loop {
                let rc = unsafe { flock(fd, LOCK_EX) };
                if rc == 0 {
                    return Ok(Guard(fd));
                }
                let err = std::io::Error::last_os_error();
                if err.kind() != std::io::ErrorKind::Interrupted {
                    return Err(format!("flock: {err}"));
                }
            }
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { flock(self.0, LOCK_UN) };
        }
    }
}
const OP_PUT: u8 = 1;
const OP_DEL: u8 = 2;

const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            c = if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

const CRC_TABLE: [u32; 256] = make_crc_table();

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub meta: String,
    pub vector: Vec<f32>,
}

pub struct Db {
    path: PathBuf,
    file: File,
    pub index: HashMap<String, Entry>,
    /// Number of valid records replayed or appended (puts + deletes).
    pub records: u64,
}

impl Db {
    /// Create a new empty database file. Fails if it already exists.
    pub fn create(path: &Path) -> Result<Db, String> {
        if path.exists() {
            return Err(format!("{} already exists", path.display()));
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| format!("create {}: {e}", path.display()))?;
        file.write_all(MAGIC).map_err(|e| e.to_string())?;
        file.sync_data().map_err(|e| e.to_string())?;
        Ok(Db {
            path: path.to_path_buf(),
            file,
            index: HashMap::new(),
            records: 0,
        })
    }

    /// Open an existing database, replaying the log. A corrupt or partial tail
    /// (torn write from a crash) is detected via CRC and truncated away.
    pub fn open(path: &Path) -> Result<Db, String> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| format!("open {}: {e}", path.display()))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        if buf.len() < MAGIC.len() || &buf[..MAGIC.len()] != MAGIC {
            return Err(format!("{}: not a semdb file", path.display()));
        }

        let (index, records, valid_end) = replay_log(&buf);

        if valid_end < buf.len() {
            // Recover: drop the torn tail so future appends start clean.
            file.set_len(valid_end as u64).map_err(|e| e.to_string())?;
            file.sync_data().map_err(|e| e.to_string())?;
        }
        file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        Ok(Db {
            path: path.to_path_buf(),
            file,
            index,
            records,
        })
    }

    /// Embedding dimension of this db: the length of the first real
    /// (non-placeholder) vector, or None if only placeholders exist.
    pub fn dim(&self) -> Option<usize> {
        self.index.values().map(|e| e.vector.len()).find(|&l| l > 1)
    }

    pub fn put(&mut self, id: &str, meta: &str, vector: Vec<f32>) -> Result<(), String> {
        // Guard the on-disk length fields (id is u16, meta is u32). An oversized
        // value would silently wrap and write a record that fails to decode on
        // reopen, which open() treats as the corruption boundary and truncates
        // everything after it — poisoning the whole log. Reject up front.
        if id.len() > u16::MAX as usize {
            return Err(format!("id too long: {} bytes (max {})", id.len(), u16::MAX));
        }
        if meta.len() > u32::MAX as usize {
            return Err("meta too long (max u32)".into());
        }
        // Dimension guard: cosine over mixed-dim vectors zips to the shorter
        // length and silently mis-scores. Placeholder vectors (len ≤ 1, the
        // non-semantic-row convention) are exempt.
        if vector.len() > 1 {
            if let Some(d) = self.dim() {
                if vector.len() != d {
                    return Err(format!("vector dim {} does not match db dim {d}", vector.len()));
                }
            }
        }
        let body = encode_put(id, meta, &vector);
        self.append(&body)?;
        self.index.insert(
            id.to_string(),
            Entry {
                meta: meta.to_string(),
                vector,
            },
        );
        Ok(())
    }

    /// Bulk insert: appends every row, then fsyncs ONCE. `put` syncs per record,
    /// which costs one fsync per row — minutes for a many-thousand-row code
    /// index. Same length/dimension guards as `put`.
    pub fn put_many(&mut self, rows: &[(String, String, Vec<f32>)]) -> Result<usize, String> {
        let mut dim = self.dim();
        for (id, meta, vector) in rows {
            if id.len() > u16::MAX as usize {
                return Err(format!("id too long: {} bytes (max {})", id.len(), u16::MAX));
            }
            if meta.len() > u32::MAX as usize {
                return Err("meta too long (max u32)".into());
            }
            if vector.len() > 1 {
                match dim {
                    Some(d) if vector.len() != d => {
                        return Err(format!("vector dim {} does not match db dim {d}", vector.len()));
                    }
                    None => dim = Some(vector.len()),
                    _ => {}
                }
            }
        }
        for (id, meta, vector) in rows {
            let body = encode_put(id, meta, vector);
            write_framed(&mut self.file, &body)?;
            self.records += 1;
            self.index.insert(id.clone(), Entry { meta: meta.clone(), vector: vector.clone() });
        }
        self.file.sync_data().map_err(|e| e.to_string())?;
        Ok(rows.len())
    }

    pub fn delete(&mut self, id: &str) -> Result<bool, String> {
        if id.len() > u16::MAX as usize {
            return Err(format!("id too long: {} bytes (max {})", id.len(), u16::MAX));
        }
        if !self.index.contains_key(id) {
            return Ok(false);
        }
        let mut body = vec![OP_DEL];
        put_bytes16(&mut body, id.as_bytes());
        self.append(&body)?;
        self.index.remove(id);
        Ok(true)
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.index.get(id)
    }

    /// Rewrite the log with only live entries (drops tombstoned history).
    pub fn compact(&mut self) -> Result<(), String> {
        // Hold LOCK_EX for the whole rewrite so no writer appends into the old
        // file after we snapshot it and before the rename swaps it out (that
        // append would be silently discarded). Under the lock, re-replay the
        // ON-DISK log so we compact the COMPLETE current state, not this
        // handle's possibly-stale in-memory index (other processes may have
        // appended rows we never saw) — otherwise compaction deletes them.
        let _lock = flock::Guard::exclusive(self.file.as_raw_fd())?;
        let mut buf = Vec::new();
        self.file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        self.file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        let (index, _records, _end) = replay_log(&buf);
        // Unique tmp per pid so two concurrent compactors never share a scratch file.
        let tmp = self
            .path
            .with_extension(format!("compact.{}", std::process::id()));
        {
            let mut f = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&tmp)
                .map_err(|e| e.to_string())?;
            f.write_all(MAGIC).map_err(|e| e.to_string())?;
            for (id, e) in &index {
                let body = encode_put(id, &e.meta, &e.vector);
                write_framed(&mut f, &body)?;
            }
            f.sync_all().map_err(|e| e.to_string())?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        self.file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&self.path)
            .map_err(|e| e.to_string())?;
        self.file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        self.records = index.len() as u64;
        self.index = index;
        Ok(())
    }

    fn append(&mut self, body: &[u8]) -> Result<(), String> {
        // Hold LOCK_EX across seek-end → write → sync so a concurrent writer
        // (another process or another Db handle) cannot land a frame at the
        // same offset. Re-seek under the lock picks up appends made since our
        // last write, so our frame always goes after the true end.
        let _lock = flock::Guard::exclusive(self.file.as_raw_fd())?;
        self.file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        write_framed(&mut self.file, body)?;
        self.file.sync_data().map_err(|e| e.to_string())?;
        self.records += 1;
        Ok(())
    }
}

/// Replay a raw log buffer into (index, valid-record-count, byte offset of the
/// first torn/corrupt record). Shared by open() and compact() so a compaction
/// never rewrites from a stale in-memory index.
fn replay_log(buf: &[u8]) -> (HashMap<String, Entry>, u64, usize) {
    let mut index = HashMap::new();
    let mut records = 0u64;
    let mut pos = MAGIC.len();
    let valid_end = loop {
        if pos == buf.len() {
            break pos; // clean end
        }
        if pos + 8 > buf.len() {
            break pos; // torn length/crc header
        }
        let len = u32::from_le_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
        let crc = u32::from_le_bytes(buf[pos + 4..pos + 8].try_into().unwrap());
        let body_start = pos + 8;
        if body_start + len > buf.len() {
            break pos; // torn body
        }
        let body = &buf[body_start..body_start + len];
        if crc32(body) != crc {
            break pos; // corrupt record — stop replay here
        }
        match decode_record(body) {
            Some((OP_PUT, id, entry)) => {
                index.insert(id, entry.expect("put has entry"));
            }
            Some((OP_DEL, id, _)) => {
                index.remove(&id);
            }
            _ => break pos, // unknown op — treat as corruption boundary
        }
        records += 1;
        pos = body_start + len;
    };
    (index, records, valid_end)
}

fn write_framed(f: &mut File, body: &[u8]) -> Result<(), String> {
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&crc32(body).to_le_bytes());
    frame.extend_from_slice(body);
    f.write_all(&frame).map_err(|e| e.to_string())
}

fn encode_put(id: &str, meta: &str, vector: &[f32]) -> Vec<u8> {
    let mut body = vec![OP_PUT];
    put_bytes16(&mut body, id.as_bytes());
    put_bytes32(&mut body, meta.as_bytes());
    body.extend_from_slice(&(vector.len() as u32).to_le_bytes());
    for v in vector {
        body.extend_from_slice(&v.to_le_bytes());
    }
    body
}

fn put_bytes16(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
}

fn put_bytes32(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

fn decode_record(body: &[u8]) -> Option<(u8, String, Option<Entry>)> {
    let op = *body.first()?;
    let mut pos = 1;
    let id_len = u16::from_le_bytes(body.get(pos..pos + 2)?.try_into().ok()?) as usize;
    pos += 2;
    let id = String::from_utf8(body.get(pos..pos + id_len)?.to_vec()).ok()?;
    pos += id_len;
    if op == OP_DEL {
        return Some((op, id, None));
    }
    if op != OP_PUT {
        return None;
    }
    let meta_len = u32::from_le_bytes(body.get(pos..pos + 4)?.try_into().ok()?) as usize;
    pos += 4;
    let meta = String::from_utf8(body.get(pos..pos + meta_len)?.to_vec()).ok()?;
    pos += meta_len;
    let dim = u32::from_le_bytes(body.get(pos..pos + 4)?.try_into().ok()?) as usize;
    pos += 4;
    let vector_bytes = dim.checked_mul(4)?;
    let vector_end = pos.checked_add(vector_bytes)?;
    if vector_end > body.len() {
        return None;
    }
    let mut vector = Vec::with_capacity(dim);
    for _ in 0..dim {
        vector.push(f32::from_le_bytes(body.get(pos..pos + 4)?.try_into().ok()?));
        pos += 4;
    }
    if pos != body.len() {
        return None;
    }
    Some((op, id, Some(Entry { meta, vector })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-scratch")
            .join(format!("semdb-storage-{name}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn decode_record_rejects_oversized_vector_dim_before_allocation() {
        let mut body = vec![OP_PUT];
        put_bytes16(&mut body, b"id");
        put_bytes32(&mut body, b"meta");
        body.extend_from_slice(&u32::MAX.to_le_bytes());

        assert!(decode_record(&body).is_none());
    }

    #[test]
    fn put_get_delete_reopen() {
        let path = tmp("basic");
        let mut db = Db::create(&path).unwrap();
        db.put("a", r#"{"k":1}"#, vec![1.0, 2.0]).unwrap();
        db.put("b", "", vec![0.5, 0.25]).unwrap();
        db.delete("a").unwrap();
        drop(db);
        let db = Db::open(&path).unwrap();
        assert!(db.get("a").is_none());
        assert_eq!(db.get("b").unwrap().vector.len(), 2);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn large_meta_survives() {
        let path = tmp("large");
        let big = "x".repeat(64 * 1024); // 64KB >> 4KB
        let mut db = Db::create(&path).unwrap();
        db.put("big", &big, vec![1.0]).unwrap();
        drop(db);
        let db = Db::open(&path).unwrap();
        assert_eq!(db.get("big").unwrap().meta.len(), big.len());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn torn_tail_is_recovered() {
        let path = tmp("torn");
        let mut db = Db::create(&path).unwrap();
        db.put("keep", "", vec![1.0, 0.0]).unwrap();
        drop(db);
        // Simulate a crash mid-write: append garbage half-record.
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&[7, 0, 0, 0, 0xAA, 0xBB]).unwrap(); // truncated frame
        drop(f);
        let mut db = Db::open(&path).unwrap();
        assert!(db.get("keep").is_some());
        // And the file is clean again: new puts land fine and reopen sees them.
        db.put("after", "", vec![0.0, 1.0]).unwrap();
        drop(db);
        let db = Db::open(&path).unwrap();
        assert!(db.get("keep").is_some() && db.get("after").is_some());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn put_many_validation_failure_writes_no_rows() {
        let path = tmp("put-many-atomic");
        let mut db = Db::create(&path).unwrap();
        let rows = vec![
            ("first".to_string(), "".to_string(), vec![1.0, 0.0]),
            ("bad".to_string(), "".to_string(), vec![1.0, 0.0, 0.0]),
        ];
        let before_records = db.records;
        let before_len = std::fs::metadata(&path).unwrap().len();
        let err = db.put_many(&rows).unwrap_err();
        assert!(err.contains("vector dim 3 does not match db dim 2"));
        assert_eq!(db.records, before_records);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), before_len);
        assert!(db.get("first").is_none());
        assert!(db.get("bad").is_none());
        drop(db);
        let db = Db::open(&path).unwrap();
        assert!(db.get("first").is_none());
        assert!(db.get("bad").is_none());
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn compact_drops_history() {
        let path = tmp("compact");
        let mut db = Db::create(&path).unwrap();
        for i in 0..50 {
            db.put("same", &format!("v{i}"), vec![i as f32]).unwrap();
        }
        let before = std::fs::metadata(&path).unwrap().len();
        db.compact().unwrap();
        let after = std::fs::metadata(&path).unwrap().len();
        assert!(after < before);
        drop(db);
        let db = Db::open(&path).unwrap();
        assert_eq!(db.get("same").unwrap().meta, "v49");
        std::fs::remove_file(&path).unwrap();
    }
}
