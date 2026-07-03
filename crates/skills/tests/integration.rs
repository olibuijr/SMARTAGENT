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

fn empty_root(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Writes `body` to a scratch file and returns its path as a string — used to
/// feed create/edit via `--file` so tests never fall through to the stdin
/// read path (blocking on the test process's real stdin would hang the
/// suite, the same class of bug as the documented `./pi -p` stdin gotcha).
fn body_file(dir: &std::path::Path, name: &str, body: &str) -> String {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p.to_string_lossy().to_string()
}

/// Self-creating skills: create → list/match/show find it → patch → edit →
/// delete, all through the CLI dispatcher (not just the `manage` internals).
#[test]
fn create_patch_edit_delete_round_trip_through_cli() {
    let root = empty_root("skills-cli-manage");
    let root_s = root.to_string_lossy().to_string();
    let bodies = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch/skills-cli-manage-bodies");
    let _ = std::fs::remove_dir_all(&bodies);

    let initial_body = body_file(
        &bodies,
        "initial.md",
        "## When to Use\nA curl call keeps failing intermittently.\n## Procedure\nrun curl once\n",
    );
    let created = cli::run(&s(&[
        "create",
        &root_s,
        "--name",
        "curl-retry-drill",
        "--desc",
        "retry a flaky curl call with backoff",
        "--file",
        &initial_body,
    ]))
    .unwrap();
    assert!(created.contains("created 'curl-retry-drill'"), "{created}");

    // Second create of the same name must be rejected, not silently overwrite.
    let dup = cli::run(&s(&[
        "create",
        &root_s,
        "--name",
        "curl-retry-drill",
        "--desc",
        "again",
        "--file",
        &initial_body,
    ]));
    assert!(dup.is_err());

    let listed = cli::run(&s(&["list", &root_s])).unwrap();
    assert!(listed.contains("curl-retry-drill"), "{listed}");

    let matched = cli::run(&s(&[
        "match",
        &root_s,
        "my curl request keeps failing, need a retry",
    ]))
    .unwrap();
    assert!(matched.contains("curl-retry-drill"), "{matched}");

    let shown = cli::run(&s(&["show", &root_s, "curl-retry-drill"])).unwrap();
    assert!(
        shown.contains("## Procedure") && shown.contains("run curl once"),
        "{shown}"
    );

    let patched = cli::run(&s(&[
        "patch",
        &root_s,
        "--name",
        "curl-retry-drill",
        "--old",
        "run curl once\n",
        "--new",
        "retry with exponential backoff\n",
    ]))
    .unwrap();
    assert!(patched.contains("patched"), "{patched}");
    let raw = std::fs::read_to_string(root.join("curl-retry-drill/SKILL.md")).unwrap();
    assert!(raw.contains("exponential backoff"), "{raw}");
    assert!(!raw.contains("run curl once"), "{raw}");

    // Ambiguous patch (matches more than once) must error, not guess.
    let ambiguous = cli::run(&s(&[
        "patch",
        &root_s,
        "--name",
        "curl-retry-drill",
        "--old",
        "curl",
        "--new",
        "wget",
    ]));
    assert!(ambiguous.is_err());

    let rewritten_body = body_file(
        &bodies,
        "rewritten.md",
        "## Procedure\nrewritten steps with backoff\n## Verification\ncheck the exit code\n",
    );
    let edited = cli::run(&s(&[
        "edit",
        &root_s,
        "--name",
        "curl-retry-drill",
        "--desc",
        "retry a flaky curl call, v2",
        "--file",
        &rewritten_body,
    ]))
    .unwrap();
    assert!(edited.contains("edited"), "{edited}");
    let after_edit = std::fs::read_to_string(root.join("curl-retry-drill/SKILL.md")).unwrap();
    assert!(after_edit.contains("v2"), "{after_edit}");
    assert!(
        after_edit.contains("rewritten steps with backoff"),
        "{after_edit}"
    );

    let deleted = cli::run(&s(&["delete", &root_s, "--name", "curl-retry-drill"])).unwrap();
    assert!(deleted.contains("deleted"), "{deleted}");
    assert!(!root.join("curl-retry-drill").exists());

    let gone = cli::run(&s(&["show", &root_s, "curl-retry-drill"]));
    assert!(gone.is_err());
}

/// `create` with a category places the skill under `<root>/<category>/<name>`
/// and `list`/`match` still discover it via the same root.
#[test]
fn create_with_category_nests_under_root() {
    let root = empty_root("skills-cli-category");
    let root_s = root.to_string_lossy().to_string();
    let bodies = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch/skills-cli-category-bodies");
    let _ = std::fs::remove_dir_all(&bodies);
    let body = body_file(
        &bodies,
        "deploy.md",
        "## When to Use\nAfter any deploy.\n## Procedure\ncurl the healthcheck endpoint\n",
    );
    cli::run(&s(&[
        "create",
        &root_s,
        "--name",
        "deploy-smoke-check",
        "--category",
        "ops",
        "--desc",
        "smoke-check a deploy before declaring it done",
        "--file",
        &body,
    ]))
    .unwrap();
    assert!(root.join("ops/deploy-smoke-check/SKILL.md").exists());
    let listed = cli::run(&s(&["list", &root_s])).unwrap();
    assert!(listed.contains("deploy-smoke-check"), "{listed}");
}

/// Frontmatter validation rejection at the CLI layer: missing description,
/// description over the 100-char self-authored cap, and a non-kebab name.
#[test]
fn create_rejects_invalid_frontmatter_at_cli_layer() {
    let root = empty_root("skills-cli-validate");
    std::fs::create_dir_all(&root).unwrap();
    let root_s = root.to_string_lossy().to_string();
    let bodies = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch/skills-cli-validate-bodies");
    let _ = std::fs::remove_dir_all(&bodies);
    let body = body_file(&bodies, "b.md", "## Procedure\ndo the thing\n");

    let no_desc = cli::run(&s(&[
        "create",
        &root_s,
        "--name",
        "no-desc-skill",
        "--file",
        &body,
    ]));
    assert!(no_desc.is_err());

    let long_desc = "x".repeat(150);
    let too_long = cli::run(&s(&[
        "create",
        &root_s,
        "--name",
        "too-long-skill",
        "--desc",
        &long_desc,
        "--file",
        &body,
    ]));
    assert!(too_long.unwrap_err().contains("must be"));

    let bad_name = cli::run(&s(&[
        "create",
        &root_s,
        "--name",
        "Not_Kebab_Case",
        "--desc",
        "fine",
        "--file",
        &body,
    ]));
    assert!(bad_name.is_err());

    // None of the rejected attempts should have left a skill behind.
    assert_eq!(cli::run(&s(&["list", &root_s])).unwrap(), "no skills found");
}
