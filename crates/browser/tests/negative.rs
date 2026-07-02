//! Negative CLI paths: bad/missing args must exit non-zero with an error, not panic.
use std::process::Command;
fn bin() -> &'static str { env!("CARGO_BIN_EXE_browser") }
fn fails(args: &[&str]) {
    let out = Command::new(bin()).args(args).output().unwrap();
    assert!(!out.status.success(), "expected failure for {args:?}");
    assert!(String::from_utf8_lossy(&out.stderr).to_lowercase().contains("error") ||
            String::from_utf8_lossy(&out.stderr).to_lowercase().contains("usage"), "no error msg for {args:?}");
}
#[test] fn bad_args() {
    fails(&["click"]);            // missing selector
    fails(&["type", "sel"]);      // missing text
    fails(&["open", "--devtools", "http://127.0.0.1:1", "http://x"]); // unreachable devtools
}
