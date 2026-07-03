use skills::cli;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("golf")).unwrap();
    std::fs::create_dir_all(d.join("cook")).unwrap();
    std::fs::write(d.join("golf/SKILL.md"), "---\nname: golf-coach\ndescription: help improve golf swing and handicap\n---\n# Golf\nSwing tips.").unwrap();
    std::fs::write(
        d.join("cook/SKILL.md"),
        "---\nname: chef\ndescription: cooking recipes\n---\n# Chef\nRecipes.",
    )
    .unwrap();
    d
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn list_show_search() {
    let d = scratch("skills-it");
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

#[test]
fn match_scores_whole_prompts() {
    let d = scratch("skills-match");
    let root = d.to_string_lossy().to_string();
    // A full sentence: substring `search` would find nothing useful here,
    // token overlap must rank golf-coach first (name hit weighted).
    let m = cli::run(&s(&[
        "match",
        &root,
        "I want to improve my golf handicap this season",
    ]))
    .unwrap();
    let first = m.lines().next().unwrap();
    assert!(first.contains("golf-coach"), "{m}");
    assert!(!first.contains("chef"), "{m}");
    // No overlap → clean miss, not an error.
    let none = cli::run(&s(&["match", &root, "quantum blockchain webinar"])).unwrap();
    assert_eq!(none, "no matching skill");
}
