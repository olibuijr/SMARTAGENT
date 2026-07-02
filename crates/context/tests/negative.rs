use std::process::Command;
fn bin() -> &'static str { env!("CARGO_BIN_EXE_context") }
fn fails(args: &[&str]) {
    let out = Command::new(bin()).args(args).output().unwrap();
    assert!(!out.status.success(), "expected failure for {args:?}");
}
#[test] fn bad_args() {
    fails(&["validate", "--dir", ".scratch/nonexistent-ctx-dir"]);
    fails(&["stat", "--dir", ".scratch/nonexistent-ctx-dir"]);
}
