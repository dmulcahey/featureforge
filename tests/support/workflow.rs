use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn compiled_superpowers_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_superpowers"))
}

pub fn workflow_fixture_root() -> PathBuf {
    repo_root().join("tests/codex-runtime/fixtures/workflow-artifacts")
}

pub fn run(mut command: Command, context: &str) -> Output {
    command
        .output()
        .unwrap_or_else(|error| panic!("{context} should run: {error}"))
}

pub fn run_checked(command: Command, context: &str) -> Output {
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

pub fn parse_json(output: &Output, context: &str) -> Value {
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

pub fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory should be creatable");
    }
    fs::write(path, contents).expect("file should be writable");
}

pub fn init_repo(name: &str) -> (TempDir, TempDir) {
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

    fs::write(repo.join("README.md"), format!("# {name}\n")).expect("README should be writable");

    let mut git_add = Command::new("git");
    git_add.args(["add", "README.md"]).current_dir(repo);
    run_checked(git_add, "git add README");

    let mut git_commit = Command::new("git");
    git_commit.args(["commit", "-m", "init"]).current_dir(repo);
    run_checked(git_commit, "git commit init");

    (repo_dir, state_dir)
}

pub fn install_full_contract_ready_artifacts(repo: &Path) {
    let fixture_root = workflow_fixture_root();
    let spec_rel = "docs/superpowers/specs/2026-03-22-runtime-integration-hardening-design.md";
    let plan_rel = "docs/superpowers/plans/2026-03-22-runtime-integration-hardening.md";
    let spec_path = repo.join(spec_rel);
    let plan_path = repo.join(plan_rel);

    fs::create_dir_all(
        spec_path
            .parent()
            .expect("spec fixture should have a parent directory"),
    )
    .expect("spec directory should be creatable");
    fs::create_dir_all(
        plan_path
            .parent()
            .expect("plan fixture should have a parent directory"),
    )
    .expect("plan directory should be creatable");

    fs::copy(
        fixture_root.join("specs/2026-03-22-runtime-integration-hardening-design.md"),
        &spec_path,
    )
    .expect("full-contract ready spec fixture should copy");

    let plan_source =
        fs::read_to_string(fixture_root.join("plans/2026-03-22-runtime-integration-hardening.md"))
            .expect("full-contract ready plan fixture should load");
    let adjusted_plan = plan_source.replace(
        "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md",
        spec_rel,
    );
    fs::write(&plan_path, adjusted_plan).expect("full-contract ready plan fixture should write");
}
