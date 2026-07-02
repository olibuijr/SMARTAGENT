use std::process::Command;
fn bin() -> &'static str { env!("CARGO_BIN_EXE_voice") }
fn fails(args: &[&str]) {
    let out = Command::new(bin()).args(args).output().unwrap();
    assert!(!out.status.success(), "expected failure for {args:?}");
}
#[test] fn bad_args() {
    fails(&["stt"]);          // missing --file
    fails(&["tts"]);          // missing --text
}
