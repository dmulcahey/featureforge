use assert_cmd::cargo::{CommandCargoExt, cargo_bin};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn workflow_fixture_root() -> PathBuf {
    repo_root().join("tests/codex-runtime/fixtures/workflow-artifacts")
}

fn workflow_status_helper_path() -> PathBuf {
    repo_root().join("bin/superpowers-workflow-status")
}

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory should be creatable");
    }
    fs::write(path, contents).expect("file should be writable");
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

fn init_repo(test_name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should be available");
    let state_dir = TempDir::new().expect("state tempdir should be available");
    let repo_path = repo_dir.path();

    let mut git_init = Command::new("git");
    git_init.arg("init").current_dir(repo_path);
    run_checked(git_init, "git init");

    let mut git_config_name = Command::new("git");
    git_config_name
        .args(["config", "user.name", "Superpowers Test"])
        .current_dir(repo_path);
    run_checked(git_config_name, "git config user.name");

    let mut git_config_email = Command::new("git");
    git_config_email
        .args(["config", "user.email", "superpowers-tests@example.com"])
        .current_dir(repo_path);
    run_checked(git_config_email, "git config user.email");

    write_file(&repo_path.join("README.md"), "# workflow runtime fixture\n");

    let mut git_add = Command::new("git");
    git_add.args(["add", "README.md"]).current_dir(repo_path);
    run_checked(git_add, "git add readme");

    let mut git_commit = Command::new("git");
    git_commit
        .args(["commit", "-m", "init"])
        .current_dir(repo_path);
    run_checked(git_commit, "git commit init");

    let mut git_remote_add = Command::new("git");
    git_remote_add
        .args([
            "remote",
            "add",
            "origin",
            &format!("git@github.com:example/{test_name}.git"),
        ])
        .current_dir(repo_path);
    run_checked(git_remote_add, "git remote add origin");

    (repo_dir, state_dir)
}

fn run_shell_status_helper(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    let mut command = Command::new(workflow_status_helper_path());
    command
        .current_dir(repo)
        .env("SUPERPOWERS_STATE_DIR", state_dir)
        .args(args);
    run(command, context)
}

fn run_shell_status_json(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Value {
    let output = run_shell_status_helper(repo, state_dir, args, context);
    parse_json(&output, context)
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

#[test]
fn canonical_workflow_status_matches_helper_for_manifest_backed_missing_spec() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-manifest-backed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let missing_spec = "docs/superpowers/specs/2026-03-24-rust-missing-spec-design.md";

    let helper_expect = run_shell_status_helper(
        repo,
        state,
        &["expect", "--artifact", "spec", "--path", missing_spec],
        "shell helper expect for missing spec",
    );
    assert!(
        helper_expect.status.success(),
        "shell helper expect should succeed, got {:?}",
        helper_expect.status
    );

    let helper_json = run_shell_status_json(
        repo,
        state,
        &["status", "--refresh"],
        "shell helper status refresh for missing spec",
    );
    let rust_output = run_rust_superpowers(
        repo,
        state,
        &["workflow", "status", "--refresh"],
        "rust canonical workflow status refresh for missing spec",
    );
    let rust_json = parse_json(&rust_output, "rust canonical workflow status refresh for missing spec");

    assert_eq!(rust_json["status"], helper_json["status"]);
    assert_eq!(rust_json["next_skill"], helper_json["next_skill"]);
    assert_eq!(rust_json["spec_path"], helper_json["spec_path"]);
    assert_eq!(rust_json["reason"], helper_json["reason"]);
    assert_eq!(rust_json["reason_codes"], helper_json["reason_codes"]);
    assert_eq!(rust_json["diagnostics"], helper_json["diagnostics"]);
}

#[test]
fn canonical_workflow_status_matches_helper_for_ambiguous_specs() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-ambiguity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let fixture_root = workflow_fixture_root();

    fs::create_dir_all(repo.join("docs/superpowers/specs"))
        .expect("specs directory should be creatable");
    fs::copy(
        fixture_root.join("specs/2026-01-22-document-review-system-design.md"),
        repo.join("docs/superpowers/specs/2026-01-22-document-review-system-design.md"),
    )
    .expect("first fixture spec should copy");
    fs::copy(
        fixture_root.join("specs/2026-02-19-visual-brainstorming-refactor-design.md"),
        repo.join("docs/superpowers/specs/2026-02-19-visual-brainstorming-refactor-design.md"),
    )
    .expect("second fixture spec should copy");

    let helper_json = run_shell_status_json(
        repo,
        state,
        &["status", "--refresh"],
        "shell helper status refresh for ambiguous specs",
    );
    let rust_output = run_rust_superpowers(
        repo,
        state,
        &["workflow", "status", "--refresh"],
        "rust canonical workflow status refresh for ambiguous specs",
    );
    let rust_json = parse_json(&rust_output, "rust canonical workflow status refresh for ambiguous specs");

    assert_eq!(rust_json["status"], helper_json["status"]);
    assert_eq!(rust_json["next_skill"], helper_json["next_skill"]);
    assert_eq!(rust_json["reason"], helper_json["reason"]);
    assert_eq!(rust_json["reason_codes"], helper_json["reason_codes"]);
    assert_eq!(rust_json["spec_candidate_count"], helper_json["spec_candidate_count"]);
}

#[test]
fn canonical_workflow_expect_and_sync_preserve_missing_spec_semantics() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-expect-sync");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let missing_spec = "docs/superpowers/specs/2026-03-24-rust-sync-missing-spec.md";

    let expect_output = run_rust_superpowers(
        repo,
        state,
        &["workflow", "expect", "--artifact", "spec", "--path", missing_spec],
        "rust canonical workflow expect missing spec",
    );
    assert!(
        expect_output.status.success(),
        "rust canonical workflow expect should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        expect_output.status,
        String::from_utf8_lossy(&expect_output.stdout),
        String::from_utf8_lossy(&expect_output.stderr)
    );

    let sync_output = run_rust_superpowers(
        repo,
        state,
        &["workflow", "sync", "--artifact", "spec"],
        "rust canonical workflow sync missing spec",
    );
    assert!(
        sync_output.status.success(),
        "rust canonical workflow sync should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        sync_output.status,
        String::from_utf8_lossy(&sync_output.stdout),
        String::from_utf8_lossy(&sync_output.stderr)
    );
    let sync_stdout =
        String::from_utf8(sync_output.stdout).expect("sync output should be valid utf-8");
    assert!(sync_stdout.contains("missing_artifact"));
    assert!(sync_stdout.contains(missing_spec));
    assert!(sync_stdout.contains("superpowers:brainstorming"));

    let status_json = parse_json(
        &run_rust_superpowers(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "rust canonical workflow status refresh after sync",
        ),
        "rust canonical workflow status refresh after sync",
    );
    assert_eq!(status_json["status"], "needs_brainstorming");
    assert_eq!(status_json["spec_path"], missing_spec);
    assert_eq!(status_json["reason"], "missing_expected_spec");
    assert_eq!(status_json["reason_codes"][0], "missing_expected_spec");
}

#[cfg(unix)]
#[test]
fn workflow_status_argv0_alias_dispatches_to_canonical_tree() {
    use std::os::unix::fs::symlink;

    let (repo_dir, state_dir) = init_repo("workflow-runtime-argv0");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_file(
        &repo.join("docs/superpowers/specs/2026-03-24-draft-spec-design.md"),
        "# Draft Spec\n\n**Workflow State:** Draft\n**Spec Revision:** 1\n**Last Reviewed By:** brainstorming\n",
    );

    let helper_json = run_shell_status_json(
        repo,
        state,
        &["status", "--refresh"],
        "shell helper status refresh for argv0 alias parity",
    );

    let alias_dir = TempDir::new().expect("alias tempdir should be available");
    let alias_path = alias_dir.path().join("superpowers-workflow-status");
    symlink(cargo_bin("superpowers"), &alias_path).expect("argv0 alias symlink should be creatable");

    let alias_output = run(
        {
            let mut command = Command::new(&alias_path);
            command
                .current_dir(repo)
                .env("SUPERPOWERS_STATE_DIR", state)
                .args(["status", "--refresh"]);
            command
        },
        "rust argv0 workflow-status alias",
    );
    let alias_json = parse_json(&alias_output, "rust argv0 workflow-status alias");

    assert_eq!(alias_json["status"], helper_json["status"]);
    assert_eq!(alias_json["next_skill"], helper_json["next_skill"]);
    assert_eq!(alias_json["reason"], helper_json["reason"]);
    assert_eq!(alias_json["reason_codes"], helper_json["reason_codes"]);
}
