use std::process::Command;
fn bin() -> &'static str { env!("CARGO_BIN_EXE_sandbox") }
fn fails(args: &[&str]) {
    let out = Command::new(bin()).args(args).output().unwrap();
    assert!(!out.status.success(), "expected failure for {args:?}");
}
#[test] fn bad_args() {
    fails(&["run"]);          // no `--` / no cmd
    fails(&["run", "--"]);    // empty cmd after --
}
