use std::path::PathBuf;
use skills::cli;

fn scratch() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch/skills-it");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("golf")).unwrap();
    std::fs::create_dir_all(d.join("cook")).unwrap();
    std::fs::write(d.join("golf/SKILL.md"), "---\nname: golf-coach\ndescription: help improve golf swing and handicap\n---\n# Golf\nSwing tips.").unwrap();
    std::fs::write(d.join("cook/SKILL.md"), "---\nname: chef\ndescription: cooking recipes\n---\n# Chef\nRecipes.").unwrap();
    d
}

fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

#[test]
fn list_show_search() {
    let d = scratch();
    let root = d.to_string_lossy().to_string();
    let list = cli::run(&s(&["list", &root])).unwrap();
    assert!(list.contains("golf-coach") && list.contains("chef"));
    // list shows description, not body
    assert!(!list.contains("Swing tips"));
    let show = cli::run(&s(&["show", &root, "golf-coach"])).unwrap();
    assert!(show.contains("Swing tips"));
    let search = cli::run(&s(&["search", &root, "golf"])).unwrap();
    assert!(search.contains("golf-coach") && !search.contains("chef"));
}
