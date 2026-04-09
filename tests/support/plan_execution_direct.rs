use std::path::Path;

use clap::Parser;
use featureforge::cli::plan_execution::PlanExecutionCommand;
use featureforge::cli::{Cli, Command as RootCommand, PlanCommand};
use featureforge::diagnostics::JsonFailure;
use featureforge::execution::mutate;
use featureforge::execution::review_state;
use featureforge::execution::state::ExecutionRuntime;
use serde_json::Value;

pub enum DirectPlanExecutionRun {
    Json(Value),
    Unsupported,
}

pub fn try_run_plan_execution_json_direct(
    repo: &Path,
    state: &Path,
    args: &[&str],
    context: &str,
) -> Result<DirectPlanExecutionRun, String> {
    let normalized_args = normalize_path_args(repo, args);
    let argv = std::iter::once("featureforge")
        .chain(["plan", "execution"])
        .chain(normalized_args.iter().map(String::as_str));
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(_) => return Ok(DirectPlanExecutionRun::Unsupported),
    };
    let Some(RootCommand::Plan(plan_cli)) = cli.command else {
        return Ok(DirectPlanExecutionRun::Unsupported);
    };
    let PlanCommand::Execution(plan_execution_cli) = plan_cli.command else {
        return Ok(DirectPlanExecutionRun::Unsupported);
    };
    let command = plan_execution_cli.command;
    if matches!(
        &command,
        PlanExecutionCommand::RebuildEvidence(args) if !args.json
    ) {
        return Ok(DirectPlanExecutionRun::Unsupported);
    }
    let runtime = execution_runtime(repo, state).map_err(|error| {
        format!("{context} should discover runtime for direct execution: {error}")
    })?;
    let value = execute_plan_execution_command_json(&runtime, command)
        .map_err(|failure| format_direct_failure(context, &failure))?;
    Ok(DirectPlanExecutionRun::Json(value))
}

fn normalize_path_args(repo: &Path, args: &[&str]) -> Vec<String> {
    let mut normalized = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        let arg = args[index];
        if let Some((flag, value)) = arg.split_once('=')
            && path_like_flag(flag)
        {
            normalized.push(format!("{flag}={}", normalize_path_value(repo, value)));
            index += 1;
            continue;
        }
        normalized.push(arg.to_owned());
        if path_like_flag(arg)
            && let Some(value) = args.get(index + 1)
        {
            normalized.push(normalize_path_value(repo, value));
            index += 2;
            continue;
        }
        index += 1;
    }
    normalized
}

fn path_like_flag(flag: &str) -> bool {
    matches!(
        flag,
        "--summary-file" | "--review-summary-file" | "--verification-summary-file"
    )
}

fn normalize_path_value(repo: &Path, value: &str) -> String {
    let path = Path::new(value);
    if path.is_absolute() {
        value.to_owned()
    } else {
        repo.join(path).display().to_string()
    }
}

fn execution_runtime(repo: &Path, state: &Path) -> Result<ExecutionRuntime, String> {
    let git_repo = gix::discover(repo).map_err(|error| {
        format!("git repo should be discoverable for direct command helper: {error}")
    })?;
    let identity = featureforge::git::discover_slug_identity(repo);
    Ok(ExecutionRuntime {
        repo_root: identity.repo_root,
        git_dir: git_repo.path().to_path_buf(),
        branch_name: identity.branch_name,
        repo_slug: identity.repo_slug,
        safe_branch: identity.safe_branch,
        state_dir: state.to_path_buf(),
    })
}

fn execute_plan_execution_command_json(
    runtime: &ExecutionRuntime,
    command: PlanExecutionCommand,
) -> Result<Value, JsonFailure> {
    macro_rules! to_json {
        ($expr:expr) => {
            serde_json::to_value($expr).expect("plan execution command output should serialize")
        };
    }

    match command {
        PlanExecutionCommand::Status(args) => Ok(to_json!(runtime.status(&args)?)),
        PlanExecutionCommand::Recommend(args) => Ok(to_json!(runtime.recommend(&args)?)),
        PlanExecutionCommand::Preflight(args) => Ok(to_json!(runtime.preflight(&args)?)),
        PlanExecutionCommand::RebuildEvidence(args) => {
            Ok(to_json!(mutate::rebuild_evidence(runtime, &args)?))
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
        PlanExecutionCommand::ReconcileReviewState(args) => Ok(to_json!(
            review_state::reconcile_review_state(runtime, &args)?
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

fn format_direct_failure(context: &str, failure: &JsonFailure) -> String {
    format!(
        "{context} should succeed, got runtime failure_class={} message={}",
        failure.error_class, failure.message
    )
}
