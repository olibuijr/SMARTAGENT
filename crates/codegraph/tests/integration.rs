use codegraph::{graph::Graph, index};
use std::path::PathBuf;

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
    let n = g2
        .edges
        .iter()
        .filter(|e| e.kind == "calls" && e.to == "parse_config")
        .count();
    assert_eq!(n, 2);
    // impl Clone for Config
    assert!(g2.refs("Clone").iter().any(|r| r.contains("Config")));
}

#[test]
fn indexes_multiple_language_fixtures() {
    let mut g = Graph::new();
    index::index_source(
        &mut g,
        "app.py",
        "class App:\n def run(self): helper()\ndef helper(): pass\n",
    );
    index::index_source(
        &mut g,
        "app.ts",
        "export function boot(){ render(); } class View {}",
    );
    index::index_source(
        &mut g,
        "server.go",
        "package main\nfunc Serve(){ route() }\ntype Handler struct{}\n",
    );
    index::index_source(&mut g, "App.java", "class App { void start() { help(); } }");

    assert!(g
        .defs("App")
        .iter()
        .any(|d| d.contains("app.py") && d.ends_with("class")));
    assert!(g
        .defs("boot")
        .iter()
        .any(|d| d.contains("app.ts") && d.ends_with("function")));
    assert!(g
        .defs("Serve")
        .iter()
        .any(|d| d.contains("server.go") && d.ends_with("fn")));
    assert!(g
        .defs("start")
        .iter()
        .any(|d| d.contains("App.java") && d.ends_with("fn")));
    assert!(g.callers("helper").contains(&"run".to_string()));
}
