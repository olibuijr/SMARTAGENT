//! Integration tests: end-to-end store behavior, ANN recall vs brute force,
//! and crash recovery with a real killed writer process.

use std::path::PathBuf;
use std::process::Command;

use semdb::cli;
use semdb::storage::Db;
use semdb::vector;

fn tmp(name: &str) -> PathBuf {
    // In-repo scratch (rule 6: never /tmp). CARGO_MANIFEST_DIR is the crate dir.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join(format!("semdb-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn rand_vec(seed: &mut u64, dim: usize) -> Vec<f32> {
    let mut v = Vec::with_capacity(dim);
    for _ in 0..dim {
        *seed ^= *seed >> 12;
        *seed ^= *seed << 25;
        *seed ^= *seed >> 27;
        let r = (seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32;
        v.push(r / (1u64 << 24) as f32 - 0.5);
    }
    v
}

#[test]
fn cli_roundtrip() {
    let path = tmp("cli");
    let db = path.to_string_lossy().to_string();
    let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    cli::run(&s(&["create", &db])).unwrap();
    cli::run(&s(&["put", &db, "--id", "a", "--vector", "1,0,0", "--meta", r#"{"t":"first"}"#]))
        .unwrap();
    cli::run(&s(&["put", &db, "--id", "b", "--vector", "0,1,0"])).unwrap();

    let got = cli::run(&s(&["get", &db, "--id", "a"])).unwrap();
    assert!(got.contains("first"));

    let hits = cli::run(&s(&["search", &db, "--vector", "0.9,0.1,0", "--k", "1", "--exact"]))
        .unwrap();
    assert!(hits.lines().next().unwrap().contains("\ta\t"));

    cli::run(&s(&["del", &db, "--id", "a"])).unwrap();
    assert!(cli::run(&s(&["get", &db, "--id", "a"])).is_err());
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn ann_recall_vs_brute_force() {
    let path = tmp("recall");
    let mut db = Db::create(&path).unwrap();
    let mut seed = 1234u64;
    let n = 1000;
    let dim = 32;
    for i in 0..n {
        db.put(&format!("v{i}"), "", rand_vec(&mut seed, dim)).unwrap();
    }

    let k = 10;
    let queries = 20;
    let mut total_overlap = 0usize;
    for _ in 0..queries {
        let q = rand_vec(&mut seed, dim);
        let exact: Vec<String> = cli::search(&db, &q, k, true).into_iter().map(|r| r.0).collect();
        let approx: Vec<String> = cli::search(&db, &q, k, false).into_iter().map(|r| r.0).collect();
        total_overlap += approx.iter().filter(|id| exact.contains(id)).count();
    }
    let recall = total_overlap as f64 / (queries * k) as f64;
    println!("HNSW recall@{k} over {queries} queries on {n} vectors: {recall:.3}");
    assert!(recall >= 0.9, "recall {recall:.3} below 0.9");
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn brute_force_matches_manual_cosine() {
    let path = tmp("brute");
    let mut db = Db::create(&path).unwrap();
    db.put("x", "", vec![1.0, 0.0]).unwrap();
    db.put("y", "", vec![0.6, 0.8]).unwrap();
    let res = cli::search(&db, &[1.0, 0.0], 2, true);
    assert_eq!(res[0].0, "x");
    let expect = vector::cosine(&[1.0, 0.0], &[0.6, 0.8]);
    assert!((res[1].1 - expect).abs() < 1e-6);
    std::fs::remove_file(&path).unwrap();
}

/// Real crash: a child process writes puts in a loop and is SIGKILLed mid-run.
/// The database must reopen cleanly with a consistent prefix of the writes.
#[test]
fn kill9_recovery() {
    let path = tmp("kill9");
    {
        Db::create(&path).unwrap();
    }
    let bin = env!("CARGO_BIN_EXE_semdb");
    let db = path.to_string_lossy().to_string();

    // Writer loop in a shell child; kill it hard while writing.
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "i=0; while [ $i -lt 100000 ]; do {bin} put {db} --id k$i --vector '1,2,3' --meta '{{\"big\":\"{}\"}}' >/dev/null; i=$((i+1)); done",
            "z".repeat(8000) // >4KB metadata per record
        ))
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(700));
    // SIGKILL the whole loop (kill -9).
    let _ = Command::new("pkill").args(["-9", "-P", &child.id().to_string()]).status();
    let _ = child.kill();
    let _ = child.wait();

    // Reopen: must not error, and every surviving entry must be intact.
    let db2 = Db::open(&path).unwrap();
    for (id, e) in &db2.index {
        assert!(id.starts_with('k'));
        assert_eq!(e.vector, vec![1.0, 2.0, 3.0]);
        assert!(e.meta.len() > 8000);
    }
    println!("recovered {} entries after kill -9", db2.index.len());

    // And it's writable after recovery.
    let mut db3 = Db::open(&path).unwrap();
    db3.put("post-crash", "", vec![9.0]).unwrap();
    assert!(db3.get("post-crash").is_some());
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn mid_file_corruption_truncates_later_records() {
    // Documented design: on reopen, replay stops at the first bad CRC and the
    // file is truncated there — so a flipped byte in a *middle* record loses
    // that record AND everything after it, even if those later records were
    // themselves valid. This test pins that (lossy) contract so it can't
    // change silently.
    let path = tmp("midcorrupt");
    {
        let mut db = Db::create(&path).unwrap();
        db.put("r1", "one", vec![1.0]).unwrap();
        db.put("r2", "two", vec![2.0]).unwrap();
        db.put("r3", "three", vec![3.0]).unwrap();
    }
    // Flip a byte in the middle of the file (inside record 2's region), past
    // the 8-byte header of record 1.
    let mut bytes = std::fs::read(&path).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    std::fs::write(&path, &bytes).unwrap();

    let db = Db::open(&path).unwrap(); // must not error
    // r1 survives; r2 (corrupted) and r3 (after the boundary) are gone.
    assert!(db.get("r1").is_some(), "record before corruption should survive");
    assert!(db.get("r3").is_none(), "records after a mid-file corruption are truncated away");
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn compact_survives_stale_tmp_and_preserves_live_after_delete() {
    let path = tmp("compact");
    let mut db = Db::create(&path).unwrap();
    db.put("keep", "v", vec![1.0]).unwrap();
    db.put("drop", "v", vec![2.0]).unwrap();
    db.delete("drop").unwrap();

    // A leftover .compact file from an interrupted prior compaction must not
    // block the next one (it is opened create+truncate).
    let stale = path.with_extension("compact");
    std::fs::write(&stale, b"garbage from a crashed compact").unwrap();

    db.compact().unwrap();
    // Still writable, and a put after compact survives a reopen.
    db.put("added", "v", vec![3.0]).unwrap();
    drop(db);

    let db2 = Db::open(&path).unwrap();
    assert!(db2.get("keep").is_some());
    assert!(db2.get("added").is_some());
    assert!(db2.get("drop").is_none(), "deleted entry must not resurrect after compact");
    std::fs::remove_file(&path).unwrap();
}
