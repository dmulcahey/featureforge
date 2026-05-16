#![allow(dead_code)]

use std::path::Path;
use std::process::{ExitStatus, Output};

use clap::Parser;
use featureforge::cli::plan_execution::PlanExecutionCommand;
use featureforge::cli::workflow::WorkflowCommand;
use featureforge::cli::{Cli, Command as RootCommand, PlanCommand};
use featureforge::diagnostics::JsonFailure;
use featureforge::execution::commands::repair_review_state::repair_review_state_command;
use featureforge::execution::mutate;
use featureforge::execution::state::ExecutionRuntime;
use featureforge::workflow::{operator, status};
use serde::Serialize;

/// Focused contract runner for tests whose boundary is public argv parsing plus
/// production runtime semantics, not OS process startup.
pub fn run_public_runtime_contract_in_process<'a>(
    repo: &Path,
    state_dir: &Path,
    args: impl IntoIterator<Item = &'a str>,
    context: &str,
) -> Output {
    match try_run_public_runtime_contract_in_process(repo, state_dir, args, context) {
        Ok(output) => output,
        Err(message) => panic!("{context}: {message}"),
    }
}

fn try_run_public_runtime_contract_in_process<'a>(
    repo: &Path,
    state_dir: &Path,
    args: impl IntoIterator<Item = &'a str>,
    _context: &str,
) -> Result<Output, String> {
    let args = args.into_iter().collect::<Vec<_>>();
    let cli = Cli::try_parse_from(std::iter::once("featureforge").chain(args.iter().copied()))
        .map_err(|error| error.to_string())?;

    match cli.command {
        Some(RootCommand::Plan(plan_cli)) => {
            let PlanCommand::Execution(plan_execution_cli) = plan_cli.command else {
                return Err(String::from(
                    "public runtime contract runner only supports plan execution commands",
                ));
            };
            let mut runtime = ExecutionRuntime::discover(repo).map_err(|error| error.message)?;
            runtime.state_dir = state_dir.to_path_buf();
            plan_execution_output(&runtime, plan_execution_cli.command)
        }
        Some(RootCommand::Workflow(workflow_cli)) => {
            workflow_output(repo, state_dir, workflow_cli.command)
        }
        Some(_) | None => Err(String::from(
            "public runtime contract runner only supports workflow and plan execution commands",
        )),
    }
}

fn workflow_output(
    repo: &Path,
    state_dir: &Path,
    command: WorkflowCommand,
) -> Result<Output, String> {
    match command {
        WorkflowCommand::Status(args) if args.json => {
            let result = status::WorkflowRuntime::discover_read_only_for_state_dir(repo, state_dir)
                .and_then(|runtime| runtime.status())
                .map_err(JsonFailure::from);
            json_output(result)
        }
        WorkflowCommand::Operator(args) if args.json => {
            let mut runtime = ExecutionRuntime::discover(repo).map_err(|error| error.message)?;
            runtime.state_dir = state_dir.to_path_buf();
            json_output(operator::operator_for_runtime(&runtime, &args))
        }
        _ => Err(String::from(
            "public runtime contract runner only supports workflow status --json and workflow operator --json",
        )),
    }
}

fn plan_execution_output(
    runtime: &ExecutionRuntime,
    command: PlanExecutionCommand,
) -> Result<Output, String> {
    match command {
        PlanExecutionCommand::Status(args) => json_output(runtime.status(&args)),
        PlanExecutionCommand::RepairReviewState(args) => {
            json_output(repair_review_state_command(runtime, &args))
        }
        PlanExecutionCommand::CloseCurrentTask(args) => {
            json_output(mutate::close_current_task(runtime, &args))
        }
        PlanExecutionCommand::AdvanceLateStage(args) => {
            json_output(mutate::advance_late_stage(runtime, &args))
        }
        PlanExecutionCommand::Begin(args) => json_output(mutate::begin(runtime, &args)),
        PlanExecutionCommand::Complete(args) => json_output(mutate::complete(runtime, &args)),
        PlanExecutionCommand::Reopen(args) => json_output(mutate::reopen(runtime, &args)),
        PlanExecutionCommand::Transfer(args) => json_output(mutate::transfer(runtime, &args)),
        PlanExecutionCommand::MaterializeProjections(args) => {
            json_output(mutate::materialize_projections(runtime, &args))
        }
    }
}

fn json_output<T: Serialize>(result: Result<T, JsonFailure>) -> Result<Output, String> {
    Ok(match result {
        Ok(value) => output_with_code(
            0,
            json_line(&value).map_err(|error| error.to_string())?,
            Vec::new(),
        ),
        Err(failure) => output_with_code(
            1,
            Vec::new(),
            json_line(&failure).map_err(|error| error.to_string())?,
        ),
    })
}

fn json_line<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    Ok(encoded)
}

fn output_with_code(code: i32, stdout: Vec<u8>, stderr: Vec<u8>) -> Output {
    Output {
        status: exit_status(code),
        stdout,
        stderr,
    }
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
