use featureforge::execution::state::ExecutionRuntime;
use serde_json::Value;
use std::process::Command;

use crate::process_support::run;

pub fn workflow_phase_json(runtime: &ExecutionRuntime, context: &str) -> Value {
    workflow_doctor_json(runtime, &format!("{context}: workflow phase projection"))
}

pub fn workflow_handoff_json(runtime: &ExecutionRuntime, context: &str) -> Value {
    workflow_doctor_json(runtime, &format!("{context}: workflow handoff projection"))
}

fn workflow_doctor_json(runtime: &ExecutionRuntime, context: &str) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_featureforge"));
    command
        .current_dir(&runtime.repo_root)
        .env("FEATUREFORGE_STATE_DIR", &runtime.state_dir)
        .args(["workflow", "doctor", "--json"]);
    let output = run(command, context);
    assert!(
        output.status.success(),
        "{context} should succeed through the compiled featureforge CLI\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{context} should emit workflow doctor JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}
