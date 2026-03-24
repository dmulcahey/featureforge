use assert_cmd::cargo::CommandCargoExt;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn session_entry_helper_path() -> PathBuf {
    repo_root().join("bin/superpowers-session-entry")
}

fn config_helper_path() -> PathBuf {
    repo_root().join("bin/superpowers-config")
}

fn slug_helper_path() -> PathBuf {
    repo_root().join("bin/superpowers-slug")
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

fn parse_slug_output(output: &[u8], context: &str) -> (String, String) {
    let mut command = Command::new("bash");
    command
        .arg("-lc")
        .arg("unset SLUG BRANCH; eval \"$ASSIGNMENTS\"; printf '%s\\n%s\\n' \"$SLUG\" \"$BRANCH\"")
        .env("ASSIGNMENTS", String::from_utf8_lossy(output).to_string());
    let parsed = run_checked(command, context);
    let text = String::from_utf8(parsed.stdout).expect("parsed slug output should be utf8");
    let mut lines = text.lines();
    let slug = lines
        .next()
        .expect("parsed slug should include slug line")
        .to_owned();
    let branch = lines
        .next()
        .expect("parsed slug should include branch line")
        .to_owned();
    (slug, branch)
}

fn init_repo(name: &str) -> (TempDir, TempDir) {
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

    (repo_dir, state_dir)
}

fn run_shell_session_entry(state_dir: &Path, args: &[&str], context: &str) -> Output {
    let mut command = Command::new(session_entry_helper_path());
    command.env("SUPERPOWERS_STATE_DIR", state_dir).args(args);
    run(command, context)
}

fn run_shell_config(state_dir: &Path, args: &[&str], context: &str) -> Output {
    let mut command = Command::new(config_helper_path());
    command.env("SUPERPOWERS_STATE_DIR", state_dir).args(args);
    run(command, context)
}

fn run_shell_slug(repo: &Path, context: &str) -> Output {
    let mut command = Command::new(slug_helper_path());
    command.current_dir(repo);
    run(command, context)
}

fn run_rust_superpowers(
    repo: Option<&Path>,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Output {
    let mut command =
        Command::cargo_bin("superpowers").expect("superpowers cargo binary should be available");
    if let Some(repo) = repo {
        command.current_dir(repo);
    }
    command.env("SUPERPOWERS_STATE_DIR", state_dir).args(args);
    run(command, context)
}

fn canonical_session_entry_path(state_dir: &Path, session_key: &str) -> PathBuf {
    state_dir
        .join("session-entry")
        .join("using-superpowers")
        .join(session_key)
}

#[test]
fn canonical_session_entry_missing_decision_matches_helper_semantics() {
    let (_repo_dir, state_dir) = init_repo("session-entry-missing");
    let state = state_dir.path();
    let message_file = state.join("missing-message.txt");
    write_file(&message_file, "Can you help with this task?\n");

    let helper_output = run_shell_session_entry(
        state,
        &[
            "resolve",
            "--message-file",
            message_file.to_str().expect("message file should be utf8"),
            "--session-key",
            "missing-session",
        ],
        "helper session-entry missing decision",
    );
    let helper_json = parse_json(&helper_output, "helper session-entry missing decision");

    let rust_output = run_rust_superpowers(
        None,
        state,
        &[
            "session-entry",
            "resolve",
            "--message-file",
            message_file.to_str().expect("message file should be utf8"),
            "--session-key",
            "missing-session",
        ],
        "canonical session-entry missing decision",
    );
    let rust_json = parse_json(&rust_output, "canonical session-entry missing decision");

    assert_eq!(rust_json["outcome"], helper_json["outcome"]);
    assert_eq!(rust_json["decision_source"], helper_json["decision_source"]);
    assert_eq!(rust_json["persisted"], helper_json["persisted"]);
    assert_eq!(
        rust_json["prompt"]["question"],
        helper_json["prompt"]["question"]
    );
    assert_eq!(
        rust_json["decision_path"].as_str(),
        Some(
            canonical_session_entry_path(state, "missing-session")
                .to_string_lossy()
                .as_ref()
        )
    );
}

#[test]
fn canonical_session_entry_explicit_reentry_migrates_legacy_state_to_canonical_path() {
    let (_repo_dir, state_dir) = init_repo("session-entry-reentry");
    let state = state_dir.path();
    let legacy_path = state
        .join("session-flags")
        .join("using-superpowers")
        .join("explicit-reentry");
    write_file(&legacy_path, "bypassed\n");
    let message_file = state.join("reentry-message.txt");
    write_file(&message_file, "Please use superpowers for this task.\n");

    let rust_output = run_rust_superpowers(
        None,
        state,
        &[
            "session-entry",
            "resolve",
            "--message-file",
            message_file.to_str().expect("message file should be utf8"),
            "--session-key",
            "explicit-reentry",
        ],
        "canonical session-entry explicit reentry",
    );
    let rust_json = parse_json(&rust_output, "canonical session-entry explicit reentry");
    let canonical_path = canonical_session_entry_path(state, "explicit-reentry");

    assert_eq!(rust_json["outcome"], Value::String(String::from("enabled")));
    assert_eq!(
        rust_json["decision_source"],
        Value::String(String::from("explicit_reentry"))
    );
    assert_eq!(
        rust_json["decision_path"].as_str(),
        Some(canonical_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        fs::read_to_string(&canonical_path).expect("canonical session-entry path should exist"),
        "enabled\n"
    );
}

#[test]
fn canonical_session_entry_skill_name_reentry_enables_superpowers_again() {
    let (_repo_dir, state_dir) = init_repo("session-entry-skill-reentry");
    let state = state_dir.path();
    let legacy_path = state
        .join("session-flags")
        .join("using-superpowers")
        .join("skill-reentry");
    write_file(&legacy_path, "bypassed\n");
    let message_file = state.join("skill-reentry-message.txt");
    write_file(&message_file, "Please use brainstorming for this task.\n");

    let rust_output = run_rust_superpowers(
        None,
        state,
        &[
            "session-entry",
            "resolve",
            "--message-file",
            message_file.to_str().expect("message file should be utf8"),
            "--session-key",
            "skill-reentry",
        ],
        "canonical session-entry skill-name reentry",
    );
    let rust_json = parse_json(&rust_output, "canonical session-entry skill-name reentry");
    let canonical_path = canonical_session_entry_path(state, "skill-reentry");

    assert_eq!(rust_json["outcome"], Value::String(String::from("enabled")));
    assert_eq!(
        rust_json["decision_source"],
        Value::String(String::from("explicit_reentry"))
    );
    assert_eq!(
        rust_json["decision_path"].as_str(),
        Some(canonical_path.to_string_lossy().as_ref())
    );
    assert_eq!(
        fs::read_to_string(&canonical_path).expect("canonical session-entry path should exist"),
        "enabled\n"
    );
}

#[test]
fn canonical_config_reads_legacy_yaml_in_read_only_mode_until_install_migrate_runs() {
    let (_repo_dir, state_dir) = init_repo("config-migration");
    let state = state_dir.path();
    let legacy_config = state.join("config.yaml");
    let canonical_config = state.join("config").join("config.yaml");

    write_file(
        &legacy_config,
        "update_check: false\nsuperpowers_contributor: true\n",
    );

    let shell_value = run_shell_config(state, &["get", "update_check"], "helper config get");
    assert_eq!(String::from_utf8_lossy(&shell_value.stdout).trim(), "false");

    let rust_get = run_rust_superpowers(
        None,
        state,
        &["config", "get", "update_check"],
        "canonical config get after migration",
    );
    assert!(
        rust_get.status.success(),
        "canonical config get should succeed"
    );
    assert_eq!(String::from_utf8_lossy(&rust_get.stdout).trim(), "false");
    assert!(
        String::from_utf8_lossy(&rust_get.stderr).contains("PendingMigration"),
        "canonical config get should warn when explicit migration is still pending"
    );
    assert!(
        !canonical_config.exists(),
        "read-only config access should not silently rewrite legacy config state"
    );

    let rust_list = run_rust_superpowers(None, state, &["config", "list"], "canonical config list");
    assert!(
        rust_list.status.success(),
        "canonical config list should succeed"
    );
    let listing = String::from_utf8_lossy(&rust_list.stdout);
    assert!(listing.contains("update_check: false"));
    assert!(listing.contains("superpowers_contributor: true"));
    assert!(
        String::from_utf8_lossy(&rust_list.stderr).contains("PendingMigration"),
        "canonical config list should warn when explicit migration is still pending"
    );
}

#[test]
fn canonical_config_rejects_invalid_yaml_during_migration() {
    let (_repo_dir, state_dir) = init_repo("config-invalid-yaml");
    let state = state_dir.path();
    let legacy_config = state.join("config.yaml");
    write_file(&legacy_config, "update_check:\n  nested: true\n");

    let rust_list = run_rust_superpowers(
        None,
        state,
        &["config", "list"],
        "canonical config invalid yaml",
    );
    assert!(
        !rust_list.status.success(),
        "canonical config command should fail closed on invalid legacy YAML"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&rust_list.stdout),
        String::from_utf8_lossy(&rust_list.stderr)
    );
    assert!(
        combined.contains("InvalidConfigFormat"),
        "canonical config invalid-yaml failure should identify InvalidConfigFormat, got:\n{combined}"
    );
}

#[test]
fn canonical_slug_matches_helper_for_remote_and_detached_head() {
    let (repo_dir, state_dir) = init_repo("slug-remote");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let mut git_remote_add = Command::new("git");
    git_remote_add
        .args([
            "remote",
            "add",
            "origin",
            "https://example.com/acme/slug-helper.git",
        ])
        .current_dir(repo);
    run_checked(git_remote_add, "git remote add origin");

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "feature/$(shell)$branch"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout feature branch");

    let helper_remote = run_shell_slug(repo, "helper remote slug");
    let rust_remote = run_rust_superpowers(
        Some(repo),
        state,
        &["repo", "slug"],
        "canonical remote slug",
    );
    assert!(
        rust_remote.status.success(),
        "canonical remote slug should succeed"
    );
    assert_eq!(
        parse_slug_output(&rust_remote.stdout, "parse canonical remote slug"),
        parse_slug_output(&helper_remote.stdout, "parse helper remote slug")
    );

    let mut git_detach = Command::new("git");
    git_detach
        .args(["checkout", "--detach", "HEAD"])
        .current_dir(repo);
    run_checked(git_detach, "git checkout detached");

    let helper_detached = run_shell_slug(repo, "helper detached slug");
    let rust_detached = run_rust_superpowers(
        Some(repo),
        state,
        &["repo", "slug"],
        "canonical detached slug",
    );
    assert!(
        rust_detached.status.success(),
        "canonical detached slug should succeed"
    );
    assert_eq!(
        parse_slug_output(&rust_detached.stdout, "parse canonical detached slug"),
        parse_slug_output(&helper_detached.stdout, "parse helper detached slug")
    );
}
