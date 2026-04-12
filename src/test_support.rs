#![cfg(test)]

use std::fs;
use std::path::Path;
use std::process::Command;

pub(crate) fn init_committed_test_repo(repo_root: &Path, readme_contents: &str, context: &str) {
    run_git(
        repo_root,
        &["init", "-b", "main"],
        &format!("{context}: git init"),
    );
    fs::write(repo_root.join("README.md"), readme_contents)
        .unwrap_or_else(|error| panic!("{context}: README should write: {error}"));
    run_git(
        repo_root,
        &["add", "README.md"],
        &format!("{context}: git add README"),
    );
    let output = Command::new("git")
        .current_dir(repo_root)
        .args([
            "-c",
            "user.name=FeatureForge Test",
            "-c",
            "user.email=featureforge-tests@example.com",
            "commit",
            "-m",
            "init",
        ])
        .output()
        .unwrap_or_else(|error| panic!("{context}: git commit should launch: {error}"));
    assert!(
        output.status.success(),
        "{context}: git commit should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_git(repo_root: &Path, args: &[&str], context: &str) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .unwrap_or_else(|error| panic!("{context} should launch git: {error}"));
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
