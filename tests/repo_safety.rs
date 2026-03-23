use assert_cmd::cargo::CommandCargoExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_safety_helper_path() -> PathBuf {
    repo_root().join("bin/superpowers-repo-safety")
}

fn run(mut command: Command, context: &str) -> Output {
    command
        .output()
        .unwrap_or_else(|error| panic!("{context} should run: {error}"))
}

fn run_checked(command: Command, context: &str) -> Output {
    let output = run(command, context);
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn parse_json(output: &Output, context: &str) -> Value {
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory should be creatable");
    }
    fs::write(path, contents).expect("file should be writable");
}

fn init_repo(name: &str, branch: &str, remote_url: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();

    let mut git_init = Command::new("git");
    git_init.arg("init").current_dir(repo);
    run_checked(git_init, "git init");

    let mut git_config_name = Command::new("git");
    git_config_name
        .args(["config", "user.name", "Superpowers Test"])
        .current_dir(repo);
    run_checked(git_config_name, "git config user.name");

    let mut git_config_email = Command::new("git");
    git_config_email
        .args(["config", "user.email", "superpowers-tests@example.com"])
        .current_dir(repo);
    run_checked(git_config_email, "git config user.email");

    write_file(&repo.join("README.md"), &format!("# {name}\n"));

    let mut git_add = Command::new("git");
    git_add.args(["add", "README.md"]).current_dir(repo);
    run_checked(git_add, "git add README");

    let mut git_commit = Command::new("git");
    git_commit.args(["commit", "-m", "init"]).current_dir(repo);
    run_checked(git_commit, "git commit init");

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", branch])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout branch");

    let mut git_remote_add = Command::new("git");
    git_remote_add
        .args(["remote", "add", "origin", remote_url])
        .current_dir(repo);
    run_checked(git_remote_add, "git remote add origin");

    (repo_dir, state_dir)
}

fn run_shell_repo_safety(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    let mut command = Command::new(repo_safety_helper_path());
    command
        .current_dir(repo)
        .env("SUPERPOWERS_STATE_DIR", state_dir)
        .args(args);
    run(command, context)
}

fn run_rust_superpowers(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    let mut command =
        Command::cargo_bin("superpowers").expect("superpowers cargo binary should be available");
    command
        .current_dir(repo)
        .env("SUPERPOWERS_STATE_DIR", state_dir)
        .args(args);
    run(command, context)
}

fn current_user_name() -> String {
    env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| String::from("user"))
}

fn repo_slug_from_remote(remote_url: &str) -> String {
    remote_url
        .trim_end_matches(".git")
        .rsplit('/')
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("-")
}

fn task_hash(stage: &str, task_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(stage.as_bytes());
    hasher.update(b"\n");
    hasher.update(task_id.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_owned()
}

fn legacy_approval_path(
    state_dir: &Path,
    remote_url: &str,
    branch: &str,
    stage: &str,
    task_id: &str,
) -> PathBuf {
    state_dir
        .join("projects")
        .join(repo_slug_from_remote(remote_url))
        .join(format!("{}-{}-repo-safety", current_user_name(), branch))
        .join(format!("{}.json", task_hash(stage, task_id)))
}

fn canonical_approval_path(
    state_dir: &Path,
    remote_url: &str,
    branch: &str,
    stage: &str,
    task_id: &str,
) -> PathBuf {
    state_dir
        .join("repo-safety")
        .join("approvals")
        .join(repo_slug_from_remote(remote_url))
        .join(format!("{}-{}", current_user_name(), branch))
        .join(format!("{}.json", task_hash(stage, task_id)))
}

#[test]
fn canonical_repo_safety_check_matches_helper_for_protected_branch_block() {
    let remote_url = "https://example.com/acme/repo-safety.git";
    let (repo_dir, state_dir) = init_repo("repo-safety-block", "main", remote_url);
    let repo = repo_dir.path();
    let state = state_dir.path();

    let helper_output = run_shell_repo_safety(
        repo,
        state,
        &[
            "check",
            "--intent",
            "write",
            "--stage",
            "superpowers:brainstorming",
            "--task-id",
            "spec-task",
            "--path",
            "docs/superpowers/specs/new-spec.md",
            "--write-target",
            "spec-artifact-write",
        ],
        "helper protected branch block",
    );
    let helper_json = parse_json(&helper_output, "helper protected branch block");

    let rust_output = run_rust_superpowers(
        repo,
        state,
        &[
            "repo-safety",
            "check",
            "--intent",
            "write",
            "--stage",
            "superpowers:brainstorming",
            "--task-id",
            "spec-task",
            "--path",
            "docs/superpowers/specs/new-spec.md",
            "--write-target",
            "spec-artifact-write",
        ],
        "canonical repo-safety protected branch block",
    );
    let rust_json = parse_json(&rust_output, "canonical repo-safety protected branch block");

    assert_eq!(rust_json["outcome"], helper_json["outcome"]);
    assert_eq!(rust_json["failure_class"], helper_json["failure_class"]);
    assert_eq!(rust_json["protected_by"], helper_json["protected_by"]);
    assert_eq!(
        rust_json["suggested_next_skill"],
        helper_json["suggested_next_skill"]
    );
}

#[test]
fn canonical_repo_safety_migrates_legacy_approval_record_to_canonical_path() {
    let remote_url = "https://example.com/acme/repo-safety.git";
    let (repo_dir, state_dir) = init_repo("repo-safety-migration", "main", remote_url);
    let repo = repo_dir.path();
    let state = state_dir.path();

    let helper_approve = run_shell_repo_safety(
        repo,
        state,
        &[
            "approve",
            "--stage",
            "superpowers:brainstorming",
            "--task-id",
            "spec-task",
            "--reason",
            "User explicitly approved writing the spec on main.",
            "--path",
            "docs/superpowers/specs/new-spec.md",
            "--write-target",
            "spec-artifact-write",
        ],
        "helper approve legacy approval",
    );
    let helper_json = parse_json(&helper_approve, "helper approve legacy approval");
    let legacy_path = legacy_approval_path(
        state,
        remote_url,
        "main",
        "superpowers:brainstorming",
        "spec-task",
    );
    assert_eq!(
        helper_json["approval_path"].as_str(),
        Some(legacy_path.to_string_lossy().as_ref())
    );
    assert!(
        legacy_path.is_file(),
        "helper should write legacy approval record"
    );

    let rust_output = run_rust_superpowers(
        repo,
        state,
        &[
            "repo-safety",
            "check",
            "--intent",
            "write",
            "--stage",
            "superpowers:brainstorming",
            "--task-id",
            "spec-task",
            "--path",
            "docs/superpowers/specs/new-spec.md",
            "--write-target",
            "spec-artifact-write",
        ],
        "canonical repo-safety migrated approval check",
    );
    let rust_json = parse_json(
        &rust_output,
        "canonical repo-safety migrated approval check",
    );
    let canonical_path = canonical_approval_path(
        state,
        remote_url,
        "main",
        "superpowers:brainstorming",
        "spec-task",
    );

    assert_eq!(rust_json["outcome"], Value::String(String::from("allowed")));
    assert_eq!(
        rust_json["approval_path"].as_str(),
        Some(canonical_path.to_string_lossy().as_ref())
    );
    assert!(
        canonical_path.is_file(),
        "canonical repo-safety check should materialize migrated approval state"
    );
}

#[test]
fn canonical_repo_safety_matches_helper_for_instruction_protected_branch_rule() {
    let remote_url = "https://example.com/acme/repo-safety.git";
    let (repo_dir, state_dir) = init_repo("repo-safety-instructions", "release", remote_url);
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_file(
        &repo.join("AGENTS.override.md"),
        "Superpowers protected branches: release\n",
    );

    let helper_output = run_shell_repo_safety(
        repo,
        state,
        &[
            "check",
            "--intent",
            "write",
            "--stage",
            "superpowers:brainstorming",
            "--task-id",
            "release-task",
            "--path",
            "docs/superpowers/specs/release-spec.md",
            "--write-target",
            "spec-artifact-write",
        ],
        "helper instruction protected branch",
    );
    let helper_json = parse_json(&helper_output, "helper instruction protected branch");

    let rust_output = run_rust_superpowers(
        repo,
        state,
        &[
            "repo-safety",
            "check",
            "--intent",
            "write",
            "--stage",
            "superpowers:brainstorming",
            "--task-id",
            "release-task",
            "--path",
            "docs/superpowers/specs/release-spec.md",
            "--write-target",
            "spec-artifact-write",
        ],
        "canonical repo-safety instruction protected branch",
    );
    let rust_json = parse_json(
        &rust_output,
        "canonical repo-safety instruction protected branch",
    );

    assert_eq!(rust_json["outcome"], helper_json["outcome"]);
    assert_eq!(rust_json["failure_class"], helper_json["failure_class"]);
    assert_eq!(rust_json["protected_by"], helper_json["protected_by"]);
}
