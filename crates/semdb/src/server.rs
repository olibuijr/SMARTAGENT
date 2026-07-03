//! semdb daemon (`semdb serve`) — one persistent, multi-threaded process that
//! owns every database it touches. Each `.semdb` file is opened ONCE and held
//! in memory behind a per-db `RwLock`; agents connect over a Unix socket and
//! share that in-memory state instead of forking a CLI that re-reads the whole
//! log per operation. Reads take a shared lock (fully concurrent); writes take
//! a brief exclusive lock (O(1) append + index insert). A background thread
//! compacts any db whose log has bloated — safe because the daemon is the sole
//! writer, so no concurrent append can be lost across the compaction swap.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use httpc::json::{self, Value};

use crate::storage::Db;
use crate::wire;

type DbHandle = Arc<RwLock<Db>>;
type OpenSlot = Arc<OnceLock<DbHandle>>;
type Registry = Arc<RwLock<HashMap<PathBuf, OpenSlot>>>;

/// Compaction fires only for genuinely bloated logs: many records relative to
/// live entries AND past a floor, so small/healthy dbs never pay for it.
const COMPACT_MIN_RECORDS: u64 = 4096;
const COMPACT_RATIO: u64 = 4;
const COMPACT_SWEEP_SECS: u64 = 60;

const LIBRARY_RESERVED_BASENAMES: &[&str] = &[
    "medvitund.semdb",
    "tasks.semdb",
    "hooks.semdb",
    "workflow.semdb",
    "goal.semdb",
    "evals.semdb",
    "rag.semdb",
    "codegraph.symbols.semdb",
];

/// Run the daemon: bind the socket, start the compactor, serve forever.
pub fn serve(_args: &[String]) -> Result<String, String> {
    let sock = crate::config::Config::load().semdb_socket();

    if sock.exists() {
        if UnixStream::connect(&sock).is_ok() {
            return Err(format!("semdb daemon already running on {}", sock.display()));
        }
        let _ = std::fs::remove_file(&sock); // stale socket from a dead daemon
    }
    if let Some(dir) = sock.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let listener =
        UnixListener::bind(&sock).map_err(|e| format!("bind {}: {e}", sock.display()))?;

    let reg: Registry = Arc::new(RwLock::new(HashMap::new()));
    {
        let reg = reg.clone();
        std::thread::spawn(move || background_compactor(reg));
    }
    eprintln!("[semdb] daemon up — socket {}", sock.display());

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let reg = reg.clone();
        std::thread::spawn(move || handle_client(stream, reg));
    }
    Ok("semdb daemon stopped".into())
}

/// One connection: read request lines, execute, write a response line each.
fn handle_client(stream: UnixStream, reg: Registry) {
    let reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut write_side = stream;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let resp = exec(&reg, &line);
        if write_side.write_all(resp.as_bytes()).is_err() || write_side.write_all(b"\n").is_err() {
            break;
        }
    }
}

/// Parse and dispatch one request line, returning the JSON response line.
fn exec(reg: &Registry, line: &str) -> String {
    let req = match json::parse(line) {
        Ok(v) => v,
        Err(e) => return wire::err_line(&format!("bad json: {e}")),
    };
    let op = req.get("op").and_then(Value::as_str).unwrap_or("");
    match dispatch(reg, op, &req) {
        Ok(resp) => resp,
        Err(e) => wire::err_line(&e),
    }
}

fn dispatch(reg: &Registry, op: &str, req: &Value) -> Result<String, String> {
    match op {
        "ping" => Ok(wire::ok_text("pong")),
        "create" => {
            let db = wire::field(req, "db")?;
            create_db(reg, &normalize(db))?;
            Ok(wire::ok_text(&format!("created {db}")))
        }
        "put" => {
            let h = handle(reg, wire::field(req, "db")?)?;
            let id = wire::field(req, "id")?.to_string();
            let meta = wire::opt_field(req, "meta").unwrap_or_default().to_string();
            let vector = wire::json_to_vec(req.get("vector"));
            wlock(&h).put(&id, &meta, vector)?;
            Ok(wire::ok_text(&format!("put {id}")))
        }
        "put_many" => {
            let h = handle(reg, wire::field(req, "db")?)?;
            let rows = parse_rows(req)?;
            let n = wlock(&h).put_many(&rows)?;
            Ok(wire::ok_count(n))
        }
        "get" => {
            let h = handle(reg, wire::field(req, "db")?)?;
            let id = wire::field(req, "id")?;
            let db = rlock(&h);
            match db.get(id) {
                Some(e) => Ok(wire::ok_text(&format!(
                    "id: {id}\ndim: {}\nmeta: {}",
                    e.vector.len(),
                    if e.meta.is_empty() { "(none)" } else { &e.meta }
                ))),
                None => Err(format!("id '{id}' not found")),
            }
        }
        "del" => {
            let h = handle(reg, wire::field(req, "db")?)?;
            let mut db = wlock(&h);
            if let Some(prefix) = wire::opt_field(req, "prefix") {
                if prefix.is_empty() {
                    return Err("--prefix must be non-empty".into());
                }
                let ids: Vec<String> = db
                    .index
                    .keys()
                    .filter(|k| k.starts_with(prefix))
                    .cloned()
                    .collect();
                for id in &ids {
                    db.delete(id)?;
                }
                return Ok(wire::ok_text(&format!(
                    "deleted {} rows with prefix '{prefix}'",
                    ids.len()
                )));
            }
            let id = wire::field(req, "id")?;
            if db.delete(id)? {
                Ok(wire::ok_text(&format!("deleted {id}")))
            } else {
                Err(format!("id '{id}' not found"))
            }
        }
        "search" => {
            let h = handle(reg, wire::field(req, "db")?)?;
            let query = wire::json_to_vec(req.get("vector"));
            let k = wire::opt_usize(req, "k").unwrap_or(10);
            let exact = req.get("exact").and_then(|v| match v {
                Value::Bool(b) => Some(*b),
                _ => None,
            }) == Some(true);
            let db = rlock(&h);
            if let Some(d) = db.dim() {
                if query.len() > 1 && query.len() != d {
                    return Err(format!("query dim {} does not match db dim {d}", query.len()));
                }
            }
            let hits = crate::cli::search(&db, &query, k, exact);
            Ok(wire::ok_hits(&hits))
        }
        "stats" => {
            let key = normalize(wire::field(req, "db")?);
            let h = handle(reg, wire::field(req, "db")?)?;
            let db = rlock(&h);
            let bytes = std::fs::metadata(&key).map(|m| m.len()).unwrap_or(0);
            Ok(wire::ok_text(&format!(
                "entries: {}\nrecords: {}\nfile bytes: {bytes}",
                db.index.len(),
                db.records
            )))
        }
        "count" => {
            let h = handle(reg, wire::field(req, "db")?)?;
            let db = rlock(&h);
            let n = match wire::opt_field(req, "prefix") {
                Some(p) => db.index.keys().filter(|k| k.starts_with(p)).count(),
                None => db.index.len(),
            };
            Ok(wire::ok_text(&n.to_string()))
        }
        "ids" => {
            let h = handle(reg, wire::field(req, "db")?)?;
            let prefix = wire::opt_field(req, "prefix").unwrap_or_default();
            let limit = wire::opt_usize(req, "limit").unwrap_or(usize::MAX);
            let db = rlock(&h);
            let mut ids: Vec<&String> =
                db.index.keys().filter(|k| k.starts_with(prefix)).collect();
            ids.sort();
            let out = ids
                .into_iter()
                .take(limit)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            Ok(wire::ok_text(&out))
        }
        "compact" => {
            let h = handle(reg, wire::field(req, "db")?)?;
            let mut db = wlock(&h);
            db.compact()?;
            Ok(wire::ok_text(&format!("compacted, {} live entries", db.index.len())))
        }
        "" => Err("missing op".into()),
        other => Err(format!("unknown op '{other}'")),
    }
}

fn parse_rows(req: &Value) -> Result<Vec<(String, String, Vec<f32>)>, String> {
    let arr = req
        .get("rows")
        .and_then(Value::as_arr)
        .ok_or("missing rows")?;
    let mut rows = Vec::with_capacity(arr.len());
    for row in arr {
        let id = wire::field(row, "id")?.to_string();
        let meta = wire::opt_field(row, "meta").unwrap_or_default().to_string();
        let vector = wire::json_to_vec(row.get("vector"));
        rows.push((id, meta, vector));
    }
    Ok(rows)
}

/// Get an open handle for `db`, opening the file once if it is not cached.
fn handle(reg: &Registry, db: &str) -> Result<DbHandle, String> {
    let key = normalize(db);
    refuse_library_reserved(&key)?;
    let slot = {
        // Only the map lookup/insert is protected by the registry lock. The
        // potentially slow full-file open/replay happens below, outside that
        // global lock, so one cold db cannot stall unrelated db access.
        let mut map = wlock_reg(reg);
        map.entry(key.clone())
            .or_insert_with(|| Arc::new(OnceLock::new()))
            .clone()
    };
    if let Some(h) = slot.get() {
        return Ok(h.clone());
    }
    let db = match Db::open(&key) {
        Ok(db) => db,
        Err(e) => {
            let mut map = wlock_reg(reg);
            if map.get(&key).is_some_and(|current| Arc::ptr_eq(current, &slot)) {
                map.remove(&key);
            }
            return Err(e);
        }
    };
    let h = Arc::new(RwLock::new(db));
    let _ = slot.set(h.clone());
    Ok(h)
}

fn create_db(reg: &Registry, key: &Path) -> Result<(), String> {
    refuse_library_reserved(key)?;
    let mut map = wlock_reg(reg);
    if map.contains_key(key) {
        return Err(format!("{}: already exists", key.display()));
    }
    let db = Db::create(key)?;
    let slot = Arc::new(OnceLock::new());
    let _ = slot.set(Arc::new(RwLock::new(db)));
    map.insert(key.to_path_buf(), slot);
    Ok(())
}

/// Canonical registry key so `data/x.semdb` and its absolute form collapse to
/// one handle (two handles for one file would be two writers → corruption).
/// Must be STABLE across the file's creation: `create` runs before the file
/// exists, `put` after — so we canonicalize the PARENT dir (which exists in
/// both cases) and append the file name, rather than the whole path (which
/// canonicalizes differently pre/post creation). For a regular file this
/// equals `canonicalize(full)`; only a symlinked db path (never used) differs.
fn refuse_library_reserved(path: &Path) -> Result<(), String> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return Ok(());
    };
    if LIBRARY_RESERVED_BASENAMES.contains(&name) {
        return Err(format!(
            "{} is library-reserved; use the owning crate API instead of semdb CLI/daemon direct access",
            path.display()
        ));
    }
    Ok(())
}

fn normalize(path: &str) -> PathBuf {
    let p = Path::new(path);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    };
    match (abs.parent(), abs.file_name()) {
        (Some(parent), Some(name)) => match std::fs::canonicalize(parent) {
            Ok(cp) => cp.join(name),
            Err(_) => abs.clone(),
        },
        _ => abs,
    }
}

fn background_compactor(reg: Registry) {
    loop {
        std::thread::sleep(Duration::from_secs(COMPACT_SWEEP_SECS));
        let handles: Vec<(PathBuf, DbHandle)> = rlock_reg(&reg)
            .iter()
            .filter_map(|(k, slot)| slot.get().map(|h| (k.clone(), h.clone())))
            .collect();
        for (path, h) in handles {
            let mut db = wlock(&h);
            if bloated(&db) {
                let before = db.records;
                match db.compact() {
                    Ok(()) => eprintln!(
                        "[semdb] compacted {} — {} records → {} entries",
                        path.display(),
                        before,
                        db.index.len()
                    ),
                    Err(e) => eprintln!("[semdb] compact {} failed: {e}", path.display()),
                }
            }
        }
    }
}

fn bloated(db: &Db) -> bool {
    db.records > COMPACT_MIN_RECORDS && db.records > COMPACT_RATIO * (db.index.len() as u64).max(1)
}

// Lock helpers that recover from poisoning: a panicked holder must not wedge
// the whole daemon — the in-memory index is still consistent enough to serve.
fn rlock(h: &DbHandle) -> std::sync::RwLockReadGuard<'_, Db> {
    h.read().unwrap_or_else(|e| e.into_inner())
}
fn wlock(h: &DbHandle) -> std::sync::RwLockWriteGuard<'_, Db> {
    h.write().unwrap_or_else(|e| e.into_inner())
}
fn rlock_reg(reg: &Registry) -> std::sync::RwLockReadGuard<'_, HashMap<PathBuf, OpenSlot>> {
    reg.read().unwrap_or_else(|e| e.into_inner())
}
fn wlock_reg(reg: &Registry) -> std::sync::RwLockWriteGuard<'_, HashMap<PathBuf, OpenSlot>> {
    reg.write().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_reserved_basenames_are_refused() {
        for name in LIBRARY_RESERVED_BASENAMES {
            let path = PathBuf::from("data").join(name);
            let err = refuse_library_reserved(&path).unwrap_err();
            assert!(err.contains("library-reserved"), "{name}: {err}");
            assert!(err.contains("owning crate API"), "{name}: {err}");
        }
    }

    #[test]
    fn handle_refuses_to_adopt_library_reserved_db() {
        let reg: Registry = Arc::new(RwLock::new(HashMap::new()));
        let err = match handle(&reg, "data/medvitund.semdb") {
            Ok(_) => panic!("reserved db was adopted"),
            Err(e) => e,
        };
        assert!(err.contains("medvitund.semdb"));
        assert!(err.contains("library-reserved"));
        assert!(rlock_reg(&reg).is_empty());
    }

    #[test]
    fn handle_removes_slot_after_failed_open() {
        let reg: Registry = Arc::new(RwLock::new(HashMap::new()));
        let missing = normalize("target/test-scratch/server-missing.semdb");
        let err = match handle(&reg, missing.to_str().unwrap()) {
            Ok(_) => panic!("missing db opened"),
            Err(e) => e,
        };
        assert!(err.contains("open"), "{err}");

        assert!(
            rlock_reg(&reg).get(&missing).is_none(),
            "failed open slot should be removed so a later create/open can retry"
        );
    }
}
