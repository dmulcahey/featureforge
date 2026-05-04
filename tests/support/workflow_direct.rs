//! INTERNAL_RUNTIME_HELPER_TEST: this file intentionally exercises unavailable runtime internals.

use std::path::Path;
use std::process::{ExitStatus, Output};

use clap::Parser;
use featureforge::cli::{Cli, Command as RootCommand};
use featureforge::diagnostics::{FailureClass, JsonFailure};
use featureforge::execution::state::ExecutionRuntime;
use featureforge::workflow::operator;
use serde::Serialize;
enum DirectWorkflowEmission {
    Json(Result<Vec<u8>, JsonFailure>),
}

pub fn internal_only_try_run_workflow_output_direct(
    repo: &Path,
    state: &Path,
    args: &[&str],
    context: &str,
) -> Result<Option<Output>, String> {
    Ok(
        try_run_workflow_emission_direct(repo, state, args, context)?
            .map(|DirectWorkflowEmission::Json(result)| json_output_result(result)),
    )
}

fn try_run_workflow_emission_direct(
    repo: &Path,
    state: &Path,
    args: &[&str],
    _context: &str,
) -> Result<Option<DirectWorkflowEmission>, String> {
    let load_read_only_runtime = || read_only_execution_runtime(repo, state);
    let cli = match Cli::try_parse_from(std::iter::once("featureforge").chain(args.iter().copied()))
    {
        Ok(cli) => cli,
        Err(_) => return Ok(None),
    };
    let Some(RootCommand::Workflow(workflow_cli)) = cli.command else {
        return Ok(None);
    };
    let emission = match workflow_cli.command {
        featureforge::cli::workflow::WorkflowCommand::Operator(args) if args.json => {
            DirectWorkflowEmission::Json(serialize_json(
                load_read_only_runtime()
                    .and_then(|runtime| operator::operator_for_runtime(&runtime, &args)),
            ))
        }
        _ => return Ok(None),
    };

    Ok(Some(emission))
}

fn serialize_json<T: Serialize>(value: Result<T, JsonFailure>) -> Result<Vec<u8>, JsonFailure> {
    value.map(|value| json_line(&value).expect("direct workflow output should serialize to JSON"))
}

fn execution_runtime(repo: &Path, state: &Path) -> Result<ExecutionRuntime, JsonFailure> {
    let mut runtime = ExecutionRuntime::discover(repo)?;
    runtime.state_dir = state.to_path_buf();
    Ok(runtime)
}

fn read_only_execution_runtime(repo: &Path, state: &Path) -> Result<ExecutionRuntime, JsonFailure> {
    execution_runtime(repo, state).map_err(map_read_only_workflow_failure)
}

fn map_read_only_workflow_failure(failure: JsonFailure) -> JsonFailure {
    if failure.error_class == FailureClass::BranchDetectionFailed.as_str() {
        JsonFailure::new(
            FailureClass::RepoContextUnavailable,
            "Read-only workflow resolution requires a git repo.",
        )
    } else {
        failure
    }
}

fn json_output_result(result: Result<Vec<u8>, JsonFailure>) -> Output {
    match result {
        Ok(stdout) => output_with_code(0, stdout, Vec::new()),
        Err(failure) => output_with_code(
            1,
            Vec::new(),
            json_line(&failure).expect("direct workflow json failure should serialize"),
        ),
    }
}

fn output_with_code(code: i32, stdout: Vec<u8>, stderr: Vec<u8>) -> Output {
    Output {
        status: exit_status(code),
        stdout,
        stderr,
    }
}

fn json_line<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    Ok(encoded)
}

#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;

    ExitStatus::from_raw(code << 8)
}

#[cfg(windows)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;

    ExitStatus::from_raw(code as u32)
}
