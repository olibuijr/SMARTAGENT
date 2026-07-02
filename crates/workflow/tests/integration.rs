use std::path::PathBuf;
use workflow::cli;

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("workflows")).unwrap();
    d
}

fn s(v: &[&str], root: &PathBuf) -> Vec<String> {
    let mut a: Vec<String> = v.iter().map(|x| x.to_string()).collect();
    a.extend(["--root".into(), root.display().to_string(), "--db".into(), root.join("workflow.semdb").display().to_string()]);
    a
}

const WF: &str = "---\nname: mini\ndescription: two-step demo\nuse_when: testing\n---\n## observe\nskill: memory\nexpect: context summary\nRecall what matters.\n\n## verify\nskill: evals\nProve it worked.\n";

#[test]
fn evidence_gated_engine_end_to_end() {
    let root = scratch("wf-e2e");
    std::fs::write(root.join("workflows/mini.md"), WF).unwrap();
    // discovery from workflows/ AND skills/*/Workflows/
    std::fs::create_dir_all(root.join("skills/Kanban/Workflows")).unwrap();
    std::fs::write(root.join("skills/Kanban/Workflows/Triage.md"), "## sort\nGo through backlog.\n").unwrap();
    let list = cli::run(&s(&["list"], &root)).unwrap();
    assert!(list.contains("mini") && list.contains("triage"), "{list}");
    // start prints step 1 with skill routing
    let step1 = cli::run(&s(&["start", "mini", "--task", "T-7"], &root)).unwrap();
    assert!(step1.contains("step 1/2: observe") && step1.contains("use: memory") && step1.contains("T-7"), "{step1}");
    // trivial evidence rejected
    let e = cli::run(&s(&["advance", "--evidence", "done"], &root)).unwrap_err();
    assert!(e.contains("evidence required"), "{e}");
    // real evidence advances to step 2
    let step2 = cli::run(&s(&["advance", "--evidence", "recalled 3 memories about the task"], &root)).unwrap();
    assert!(step2.contains("step 2/2: verify") && step2.contains("use: evals"), "{step2}");
    // finishing the last step completes the run and points back at the board
    let fin = cli::run(&s(&["advance", "--evidence", "probe returned expected output x=42"], &root)).unwrap();
    assert!(fin.contains("W-1 complete") && fin.contains("tasks move T-7"), "{fin}");
    let runs = cli::run(&s(&["runs"], &root)).unwrap();
    assert!(runs.contains("done"), "{runs}");
    // statusline idle again
    let sl = cli::run(&s(&["statusline"], &root)).unwrap();
    assert_eq!(sl, "ok|▶ idle");
}
