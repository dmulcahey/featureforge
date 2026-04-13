use std::path::Path;
use std::process::{ExitStatus, Output};

use clap::Parser;
use featureforge::cli::plan_execution::{InternalPlanExecutionCommand, PlanExecutionCommand};
use featureforge::cli::{Cli, Command as RootCommand, PlanCommand};
use featureforge::diagnostics::JsonFailure;
use featureforge::execution::mutate;
use featureforge::execution::review_state;
use featureforge::execution::state::ExecutionRuntime;

struct DirectPlanExecutionSuccess {
    stdout: Vec<u8>,
    exit_code: u8,
}

pub fn try_run_plan_execution_output_direct(
    repo: &Path,
    state: &Path,
    args: &[&str],
    context: &str,
) -> Result<Option<Output>, String> {
    Ok(
        match try_run_plan_execution_result_direct(repo, state, args, context)? {
            Some(Ok(output)) => Some(output_with_code(
                i32::from(output.exit_code),
                output.stdout,
                Vec::new(),
            )),
            Some(Err(failure)) => Some(output_with_code(
                1,
                Vec::new(),
                json_line(&failure).expect("direct plan execution failure should serialize"),
            )),
            None => None,
        },
    )
}

fn try_run_plan_execution_result_direct(
    repo: &Path,
    state: &Path,
    args: &[&str],
    _context: &str,
) -> Result<Option<Result<DirectPlanExecutionSuccess, JsonFailure>>, String> {
    if has_relative_summary_path_arg(args) {
        // Relative summary-file paths are resolved by the real CLI against the subprocess cwd.
        // The direct helper runs in-process, so it must defer these cases to the real binary
        // instead of rewriting argv and changing user-visible path semantics.
        return Ok(None);
    }
    let argv = std::iter::once("featureforge")
        .chain(["plan", "execution"])
        .chain(args.iter().copied());
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(_) => return Ok(None),
    };
    let Some(RootCommand::Plan(plan_cli)) = cli.command else {
        return Ok(None);
    };
    let PlanCommand::Execution(plan_execution_cli) = plan_cli.command else {
        return Ok(None);
    };
    let command = plan_execution_cli.command;
    if matches!(
        &command,
        PlanExecutionCommand::RebuildEvidence(args) if !args.json
    ) {
        return Ok(None);
    }
    let runtime = match execution_runtime(repo, state) {
        Ok(runtime) => runtime,
        Err(_) => {
            // When runtime discovery is unavailable for this fixture, defer to the real CLI
            // boundary instead of failing the in-process helper path.
            return Ok(None);
        }
    };
    Ok(Some(execute_plan_execution_command_json(&runtime, command)))
}

fn has_relative_summary_path_arg(args: &[&str]) -> bool {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index];
        if let Some((flag, value)) = arg.split_once('=')
            && path_like_flag(flag)
            && Path::new(value).is_relative()
        {
            return true;
        }
        if path_like_flag(arg)
            && let Some(value) = args.get(index + 1)
            && Path::new(value).is_relative()
        {
            return true;
        }
        index += 1;
    }
    false
}

fn path_like_flag(flag: &str) -> bool {
    matches!(
        flag,
        "--summary-file" | "--review-summary-file" | "--verification-summary-file"
    )
}

fn execution_runtime(repo: &Path, state: &Path) -> Result<ExecutionRuntime, String> {
    let mut runtime = ExecutionRuntime::discover(repo).map_err(|error| {
        format!(
            "git repo should be discoverable for direct command helper: {}",
            error.message
        )
    })?;
    runtime.state_dir = state.to_path_buf();
    Ok(runtime)
}

fn execute_plan_execution_command_json(
    runtime: &ExecutionRuntime,
    command: PlanExecutionCommand,
) -> Result<DirectPlanExecutionSuccess, JsonFailure> {
    macro_rules! to_json {
        ($expr:expr) => {
            DirectPlanExecutionSuccess {
                stdout: json_line(&$expr).expect("plan execution command output should serialize"),
                exit_code: 0,
            }
        };
    }

    match command {
        PlanExecutionCommand::Status(args) => Ok(to_json!(runtime.status(&args)?)),
        PlanExecutionCommand::Recommend(args) => Ok(to_json!(runtime.recommend(&args)?)),
        PlanExecutionCommand::Preflight(args) => Ok(to_json!(runtime.preflight(&args)?)),
        PlanExecutionCommand::Internal(internal) => match internal.command {
            InternalPlanExecutionCommand::ReconcileReviewState(args) => {
                Ok(to_json!(review_state::reconcile_review_state(runtime, &args)?))
            }
        },
        PlanExecutionCommand::RebuildEvidence(args) => {
            let output = mutate::rebuild_evidence(runtime, &args)?;
            Ok(DirectPlanExecutionSuccess {
                exit_code: output.exit_code(),
                stdout: json_line(&output).expect("plan execution command output should serialize"),
            })
        }
        PlanExecutionCommand::GateContract(args) => Ok(to_json!(runtime.gate_contract(&args)?)),
        PlanExecutionCommand::RecordContract(args) => Ok(to_json!(runtime.record_contract(&args)?)),
        PlanExecutionCommand::GateEvaluator(args) => Ok(to_json!(runtime.gate_evaluator(&args)?)),
        PlanExecutionCommand::RecordEvaluation(args) => {
            Ok(to_json!(runtime.record_evaluation(&args)?))
        }
        PlanExecutionCommand::GateHandoff(args) => Ok(to_json!(runtime.gate_handoff(&args)?)),
        PlanExecutionCommand::RecordHandoff(args) => Ok(to_json!(runtime.record_handoff(&args)?)),
        PlanExecutionCommand::GateReview(args) => Ok(to_json!(runtime.gate_review(&args)?)),
        PlanExecutionCommand::RecordReviewDispatch(args) => {
            Ok(to_json!(runtime.record_review_dispatch(&args)?))
        }
        PlanExecutionCommand::RepairReviewState(args) => {
            Ok(to_json!(review_state::repair_review_state(runtime, &args)?))
        }
        PlanExecutionCommand::ExplainReviewState(args) => Ok(to_json!(
            review_state::explain_review_state(runtime, &args)?
        )),
        PlanExecutionCommand::GateFinish(args) => Ok(to_json!(runtime.gate_finish(&args)?)),
        PlanExecutionCommand::CloseCurrentTask(args) => {
            Ok(to_json!(mutate::close_current_task(runtime, &args)?))
        }
        PlanExecutionCommand::RecordBranchClosure(args) => {
            Ok(to_json!(mutate::record_branch_closure(runtime, &args)?))
        }
        PlanExecutionCommand::RecordReleaseReadiness(args) => {
            Ok(to_json!(mutate::record_release_readiness(runtime, &args)?))
        }
        PlanExecutionCommand::AdvanceLateStage(args) => {
            Ok(to_json!(mutate::advance_late_stage(runtime, &args)?))
        }
        PlanExecutionCommand::RecordFinalReview(args) => {
            Ok(to_json!(mutate::record_final_review(runtime, &args)?))
        }
        PlanExecutionCommand::RecordQa(args) => Ok(to_json!(mutate::record_qa(runtime, &args)?)),
        PlanExecutionCommand::Begin(args) => Ok(to_json!(mutate::begin(runtime, &args)?)),
        PlanExecutionCommand::Note(args) => Ok(to_json!(mutate::note(runtime, &args)?)),
        PlanExecutionCommand::Complete(args) => Ok(to_json!(mutate::complete(runtime, &args)?)),
        PlanExecutionCommand::Reopen(args) => Ok(to_json!(mutate::reopen(runtime, &args)?)),
        PlanExecutionCommand::Transfer(args) => Ok(to_json!(mutate::transfer(runtime, &args)?)),
    }
}
fn output_with_code(code: i32, stdout: Vec<u8>, stderr: Vec<u8>) -> Output {
    Output {
        status: exit_status(code),
        stdout,
        stderr,
    }
}

fn json_line<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
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
