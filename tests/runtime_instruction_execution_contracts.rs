use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn assert_file_contains(path: PathBuf, needle: &str) {
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
    assert!(
        source.to_lowercase().contains(&needle.to_lowercase()),
        "{} should contain {:?}",
        path.display(),
        needle
    );
}

#[test]
fn execution_skill_docs_describe_worktree_backed_parallel_dispatch() {
    let root = repo_root();

    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "runtime-selected topology",
    );
    assert_file_contains(
        root.join("skills/executing-plans/SKILL.md"),
        "worktree-first orchestration",
    );
    assert_file_contains(
        root.join("skills/subagent-driven-development/SKILL.md"),
        "runtime-selected topology",
    );
    assert_file_contains(
        root.join("skills/using-git-worktrees/SKILL.md"),
        "worktree-backed parallel",
    );
    assert_file_contains(
        root.join("skills/dispatching-parallel-agents/SKILL.md"),
        "runtime-selected topology",
    );
}
