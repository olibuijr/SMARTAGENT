use std::process::Command;
fn bin() -> &'static str { env!("CARGO_BIN_EXE_codegraph") }
fn fails(args: &[&str]) {
    let out = Command::new(bin()).args(args).output().unwrap();
    assert!(!out.status.success(), "expected failure for {args:?}");
}
#[test] fn bad_args() {
    fails(&["defs"]);                 // missing graph file
    fails(&["index"]);                // missing repo dir + --out
    fails(&["search", ".scratch/nope.graph", "q"]); // missing graph file
}
