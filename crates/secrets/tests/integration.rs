use std::path::PathBuf;
use secrets::cli;

fn dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch/sec-it");
    let _ = std::fs::remove_dir_all(&d);
    d
}
fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

#[test]
fn gated_get_flow() {
    let d = dir(); let store = d.to_string_lossy().to_string();
    cli::run(&s(&["set","--store",&store,"--name","api","--value","sekret"])).unwrap();
    // deny by default
    let denied = cli::run(&s(&["get","--store",&store,"--name","api","--as","agentA"]));
    assert!(denied.is_err());
    // policy-allow is admin-only: without the out-of-band signal it is refused.
    unsafe { std::env::remove_var("SMARTAGENT_SECRETS_ADMIN") };
    assert!(cli::run(&s(&["policy-allow","--store",&store,"--caller","agentA","--name","api"])).is_err());
    // grant as admin, then allowed
    unsafe { std::env::set_var("SMARTAGENT_SECRETS_ADMIN", "1") };
    cli::run(&s(&["policy-allow","--store",&store,"--caller","agentA","--name","api"])).unwrap();
    unsafe { std::env::remove_var("SMARTAGENT_SECRETS_ADMIN") };
    let ok = cli::run(&s(&["get","--store",&store,"--name","api","--as","agentA"])).unwrap();
    assert_eq!(ok, "sekret");
    // audit has a deny then an allow
    let audit = cli::run(&s(&["audit","--store",&store])).unwrap();
    assert!(audit.contains("\"decision\":\"deny\"") && audit.contains("\"decision\":\"allow\""));
}
