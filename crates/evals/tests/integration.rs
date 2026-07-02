use std::path::PathBuf;
use evals::cli;

fn dbpath() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch");
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("evals-it.jsonl");
    let _ = std::fs::remove_file(&p);
    p
}
fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

#[test]
fn log_score_diff() {
    let p = dbpath();
    let db = p.to_string_lossy().to_string();
    // run A: 3/3 pass
    for c in ["c1","c2","c3"] {
        cli::run(&s(&["log","--db",&db,"--run","A","--case",c,"--output","ok","--expected","ok"])).unwrap();
    }
    // run B: c2 regresses
    cli::run(&s(&["log","--db",&db,"--run","B","--case","c1","--output","ok","--expected","ok"])).unwrap();
    cli::run(&s(&["log","--db",&db,"--run","B","--case","c2","--output","BAD","--expected","ok"])).unwrap();
    cli::run(&s(&["log","--db",&db,"--run","B","--case","c3","--output","ok","--expected","ok"])).unwrap();

    let score_a = cli::run(&s(&["score","--db",&db,"--run","A"])).unwrap();
    assert!(score_a.contains("3/3"));
    let diff = cli::run(&s(&["diff","--db",&db,"--run-a","A","--run-b","B"])).unwrap();
    assert!(diff.contains("regressions (1)") && diff.contains("c2"));
}
