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
    // no token issued → fail closed even before policy
    let denied = cli::run(&s(&["get","--store",&store,"--name","api","--as","agentA"]));
    assert!(denied.is_err());
    // issue-token and policy-allow are admin-only: refused without the signal.
    unsafe { std::env::remove_var("SMARTAGENT_SECRETS_ADMIN") };
    assert!(cli::run(&s(&["policy-allow","--store",&store,"--caller","agentA","--name","api"])).is_err());
    assert!(cli::run(&s(&["issue-token","--store",&store,"--caller","agentA"])).is_err());
    // grant + issue token as admin
    unsafe { std::env::set_var("SMARTAGENT_SECRETS_ADMIN", "1") };
    cli::run(&s(&["policy-allow","--store",&store,"--caller","agentA","--name","api"])).unwrap();
    let token = cli::run(&s(&["issue-token","--store",&store,"--caller","agentA"])).unwrap();
    unsafe { std::env::remove_var("SMARTAGENT_SECRETS_ADMIN") };
    // authenticated + granted → allowed
    let ok = cli::run(&s(&["get","--store",&store,"--name","api","--as","agentA","--token",&token])).unwrap();
    assert_eq!(ok, "sekret");
    // valid grant but wrong token → denied (caller identity can't be claimed)
    assert!(cli::run(&s(&["get","--store",&store,"--name","api","--as","agentA","--token","forged"])).is_err());
    // granted caller's identity can't be borrowed by another caller either
    assert!(cli::run(&s(&["get","--store",&store,"--name","api","--as","agentB","--token",&token])).is_err());
    // authenticated but ungranted name → policy deny (recorded in audit)
    assert!(cli::run(&s(&["get","--store",&store,"--name","other","--as","agentA","--token",&token])).is_err());
    // audit has a deny then an allow
    let audit = cli::run(&s(&["audit","--store",&store])).unwrap();
    assert!(audit.contains("\"decision\":\"deny\"") && audit.contains("\"decision\":\"allow\""));
}
