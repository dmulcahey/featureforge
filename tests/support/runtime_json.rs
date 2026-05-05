use std::path::Path;
use std::process::Command;

use featureforge::execution::state::ExecutionRuntime;
use serde_json::Value;

use crate::process_support::{assert_workspace_runtime_uses_temp_state, run};

pub fn discover_execution_runtime(
    repo: &Path,
    state_dir: &Path,
    context: &str,
) -> ExecutionRuntime {
    let mut runtime = ExecutionRuntime::discover(repo).unwrap_or_else(|error| {
        panic!("{context}: failed to discover execution runtime: {error:?}")
    });
    runtime.state_dir = state_dir.to_path_buf();
    runtime
}

pub fn plan_execution_status_json(
    runtime: &ExecutionRuntime,
    plan: &str,
    external_review_result_ready: bool,
    context: &str,
) -> Value {
    let mut args = vec!["plan", "execution", "status", "--plan", plan];
    if external_review_result_ready {
        args.push("--external-review-result-ready");
    }
    run_featureforge_json_real_cli(&runtime.repo_root, &runtime.state_dir, &args, context)
}

pub fn run_featureforge_json_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    assert_workspace_runtime_uses_temp_state(Some(repo), Some(state_dir), None, false, context);
    let mut command = Command::new(env!("CARGO_BIN_EXE_featureforge"));
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    let output = run(command, context);
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{context} should emit valid json: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}
