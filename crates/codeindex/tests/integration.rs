use std::path::PathBuf;
use codeindex::{gitignore::Rules, search::{self, Opts}, walk};

fn fixture_named(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("sub")).unwrap();
    std::fs::create_dir_all(d.join("target")).unwrap();
    std::fs::write(d.join(".gitignore"), "*.tmp\n").unwrap();
    std::fs::write(d.join("main.rs"), "fn main() {\n    let handicap = 3;\n}").unwrap();
    std::fs::write(d.join("sub/lib.rs"), "pub fn helper() {}").unwrap();
    std::fs::write(d.join("skip.tmp"), "handicap").unwrap();
    std::fs::write(d.join("target/x.rs"), "handicap").unwrap();
    d
}

#[test]
fn end_to_end_search() {
    let d = fixture_named("ci-it");
    let rules = Rules::load(&d);
    let files = walk::walk(&d, &rules, Some("rs"));
    // gitignored .tmp and the inner target/ dir excluded
    assert!(files.iter().all(|f| !f.ends_with("skip.tmp")));
    assert!(files.iter().all(|f| !f.ends_with("x.rs"))); // was in inner target/
    let o = Opts { regex: false, ignore_case: false, before: 0, after: 0 };
    let hits = search::search(&files, "handicap", &o).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].file.to_string_lossy().contains("main.rs"));
}

#[test]
fn ignore_case_regex_with_uppercase_pattern_matches() {
    let d = fixture_named("ci-it-icase");
    let rules = Rules::load(&d);
    let files = walk::walk(&d, &rules, Some("rs"));
    let o = Opts { regex: true, ignore_case: true, before: 0, after: 0 };
    let hits = search::search(&files, "HANDICAP = .", &o).unwrap();
    assert_eq!(hits.len(), 1);
}
