#[path = "support/featureforge.rs"]
mod featureforge_support;
#[path = "support/json.rs"]
mod json_support;
#[path = "support/process.rs"]
mod process_support;

use serde_json::Value;
use tempfile::TempDir;

use featureforge_support::{run_rust_featureforge, run_rust_featureforge_with_env_control};
use json_support::parse_json;
use process_support::repo_root;

fn parse_failure_json(output: &std::process::Output, context: &str) -> Value {
    assert!(
        !output.status.success(),
        "{context} should fail, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr)
        .unwrap_or_else(|error| panic!("{context} should emit valid json failure output: {error}"))
}

#[test]
fn runtime_root_helper_resolves_the_repo_local_runtime() {
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let home_dir = TempDir::new().expect("home tempdir should exist");
    let repo = repo_root();

    let output = run_rust_featureforge_with_env_control(
        Some(repo.as_path()),
        Some(state_dir.path()),
        Some(home_dir.path()),
        &["FEATUREFORGE_DIR", "USERPROFILE"],
        &[],
        &["repo", "runtime-root", "--json"],
        "repo runtime-root repo-local success",
    );
    let json = parse_json(&output, "repo runtime-root repo-local success");

    assert_eq!(json["resolved"], Value::Bool(true));
    assert_eq!(
        json["root"],
        Value::String(repo.to_string_lossy().into_owned())
    );
    assert_eq!(json["source"], Value::String(String::from("repo_local")));
    assert_eq!(json["validation"]["has_version"], Value::Bool(true));
    assert_eq!(json["validation"]["has_binary"], Value::Bool(true));
    assert!(
        json["validation"]["upgrade_eligible"].is_boolean(),
        "runtime-root helper should expose upgrade_eligible as a boolean"
    );
}

#[test]
fn runtime_root_helper_reports_unresolved_without_guessing() {
    let outside_repo = TempDir::new().expect("outside repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");

    let output = run_rust_featureforge_with_env_control(
        Some(outside_repo.path()),
        Some(state_dir.path()),
        None,
        &["FEATUREFORGE_DIR", "HOME", "USERPROFILE"],
        &[],
        &["repo", "runtime-root", "--json"],
        "repo runtime-root unresolved",
    );
    let json = parse_json(&output, "repo runtime-root unresolved");

    assert_eq!(json["resolved"], Value::Bool(false));
    assert!(json["root"].is_null(), "unresolved helper root should be null");
    assert!(
        json["source"].is_string(),
        "unresolved helper should still report a source string"
    );
    assert!(
        json["validation"]["has_version"].is_boolean(),
        "unresolved helper should expose has_version as a boolean"
    );
    assert!(
        json["validation"]["has_binary"].is_boolean(),
        "unresolved helper should expose has_binary as a boolean"
    );
}

#[test]
fn runtime_root_helper_rejects_invalid_featureforge_dir_without_fallback() {
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let home_dir = TempDir::new().expect("home tempdir should exist");
    let invalid_dir = TempDir::new().expect("invalid runtime tempdir should exist");
    let repo = repo_root();

    let output = run_rust_featureforge(
        Some(repo.as_path()),
        Some(state_dir.path()),
        Some(home_dir.path()),
        &[(
            "FEATUREFORGE_DIR",
            invalid_dir.path().to_string_lossy().as_ref(),
        )],
        &["repo", "runtime-root", "--json"],
        "repo runtime-root invalid env",
    );
    let json = parse_failure_json(&output, "repo runtime-root invalid env");

    assert_eq!(
        json["error_class"],
        Value::String(String::from("ResolverContractViolation"))
    );
    let message = json["message"]
        .as_str()
        .expect("failure message should be a string");
    assert!(
        message.contains("FEATUREFORGE_DIR"),
        "failure output should name FEATUREFORGE_DIR, got: {message}"
    );
}
