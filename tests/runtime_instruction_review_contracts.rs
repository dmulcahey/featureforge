use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn assert_file_contains(path: PathBuf, needle: &str) {
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
    assert!(
        source.contains(needle),
        "{} should contain {:?}",
        path.display(),
        needle
    );
}

#[test]
fn review_skill_docs_keep_final_review_dedicated_and_gate_aware() {
    let root = repo_root();

    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "final cross-task review gate",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/SKILL.md"),
        "featureforge:requesting-code-review",
    );
    assert_file_contains(
        root.join("skills/requesting-code-review/code-reviewer.md"),
        "approved plan",
    );
}
