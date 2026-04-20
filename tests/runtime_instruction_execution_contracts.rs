//! Runtime instruction execution contracts integration/benchmark crate.
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn assert_file_contains(path: &Path, needle: &str) {
    let source = fs::read_to_string(path).unwrap_or_else(|error| {
        featureforge::abort!("{} should be readable: {error}", path.display())
    });
    assert!(
        source.to_lowercase().contains(&needle.to_lowercase()),
        "{} should contain {:?}",
        path.display(),
        needle
    );
}

fn assert_file_not_contains(path: &Path, needle: &str) {
    let source = fs::read_to_string(path).unwrap_or_else(|error| {
        featureforge::abort!("{} should be readable: {error}", path.display())
    });
    assert!(
        !source.to_lowercase().contains(&needle.to_lowercase()),
        "{} should not contain {:?}",
        path.display(),
        needle
    );
}

#[test]
fn execution_skill_docs_describe_worktree_backed_parallel_dispatch() {
    let root = repo_root();
    let executing = root.join("skills/executing-plans/SKILL.md");
    let subagent = root.join("skills/subagent-driven-development/SKILL.md");

    assert_file_contains(&executing, "runtime-selected topology");
    assert_file_contains(&executing, "worktree-first orchestration");
    assert_file_contains(&subagent, "runtime-selected topology");
    assert_file_contains(
        &root.join("skills/using-git-worktrees/SKILL.md"),
        "worktree-backed parallel",
    );
    assert_file_contains(
        &root.join("skills/dispatching-parallel-agents/SKILL.md"),
        "runtime-selected topology",
    );
    assert_file_not_contains(
        &executing,
        "Do not auto-clean the workspace and do not auto-create a worktree.",
    );
    assert_file_not_contains(
        &executing,
        "Workspace preparation is the user's responsibility; `featureforge:using-git-worktrees` is optional, not automatic",
    );
    assert_file_not_contains(&root.join("skills/executing-plans/SKILL.md"), "repairing");
    assert_file_not_contains(
        &subagent,
        "Do not auto-clean the workspace and do not auto-create a worktree.",
    );
    assert_file_not_contains(
        &subagent,
        "Dispatch multiple implementation subagents in parallel (conflicts)",
    );
    assert_file_not_contains(
        &root.join("skills/subagent-driven-development/SKILL.md"),
        "repairing",
    );
}
