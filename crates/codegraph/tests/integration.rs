use std::path::PathBuf;
use codegraph::{graph::Graph, index};

fn scratch() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch/cg-it");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn index_and_query_structural() {
    let d = scratch();
    // Build an in-memory graph from a fixture source (structural half — no network).
    let src = r#"
        fn parse_config() {}
        struct Config;
        fn main() {
            parse_config();
            if true { parse_config(); }
            let s = "parse_config()"; // string — excluded
        }
        impl Clone for Config { fn clone_it() {} }
    "#;
    let mut g = Graph::new();
    index::index_source(&mut g, "lib.rs", src);
    let out = d.join("graph.json");
    g.save(&out).unwrap();

    let g2 = Graph::load(&out).unwrap();
    assert_eq!(g2.defs("parse_config"), vec!["lib.rs:2\tfn"]);
    assert_eq!(g2.callers("parse_config"), vec!["main"]);
    // string occurrence excluded → exactly 2 call edges from main
    let n = g2.edges.iter().filter(|e| e.kind == "calls" && e.to == "parse_config").count();
    assert_eq!(n, 2);
    // impl Clone for Config
    assert!(g2.refs("Clone").iter().any(|r| r.contains("Config")));
}
