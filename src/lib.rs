//! `FeatureForge` runtime crate.
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use clap::{CommandFactory, Parser};
use cli::runtime_root::RuntimeRootFieldCli;
use cli::{Command, PlanCommand, RepoCommand};
use diagnostics::{FailureClass, JsonFailure};
use serde_json::{Value, json};

/// Runtime module.
pub mod benchmarking;
/// Runtime module.
pub mod bool_flag;
/// Runtime module.
pub mod cli;
/// Runtime module.
pub mod config;
/// Runtime module.
pub mod contracts;
/// Runtime module.
pub mod diagnostics;
pub mod execution;
/// Runtime module.
pub mod expect_ext;
/// Runtime module.
pub mod git;
/// Runtime module.
pub mod instructions;
/// Runtime module.
pub mod output;
/// Runtime module.
pub mod paths;
/// Runtime module.
pub mod repo_safety;
/// Runtime module.
pub mod runtime_root;
#[cfg(test)]
/// Runtime module.
pub mod test_support;
/// Runtime module.
pub mod update_check;
/// Runtime module.
pub mod workflow;

#[macro_export]
/// Runtime item.
macro_rules! abort {
    ($($arg:tt)*) => {{
        $crate::expect_ext::abort_with_message(&format!($($arg)*))
    }};
}

trait ExitCodeJson {
    fn exit_code(&self) -> u8;
}

impl ExitCodeJson for execution::state::RebuildEvidenceOutput {
    fn exit_code(&self) -> u8 {
        self.exit_code()
    }
}

#[must_use]
/// Runtime function.
pub fn run() -> std::process::ExitCode {
    let args = match canonicalized_args() {
        Ok(args) => args,
        Err(error) => return emit_json::<Value, JsonFailure>(Err(error)),
    };
    let cli = match parse_cli(args) {
        Ok(cli) => cli,
        Err(exit_code) => return exit_code,
    };
    run_command(cli.command)
}

fn parse_cli(args: Vec<OsString>) -> Result<cli::Cli, std::process::ExitCode> {
    match cli::Cli::try_parse_from(args) {
        Ok(cli) => Ok(cli),
        Err(error) => match error.kind() {
            clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                print!("{error}");
                Err(std::process::ExitCode::SUCCESS)
            }
            _ => Err(emit_json::<Value, JsonFailure>(Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                error.to_string(),
            )))),
        },
    }
}

fn run_command(command: Option<Command>) -> std::process::ExitCode {
    match command {
        Some(Command::Config(config_cli)) => run_config_command(config_cli),
        Some(Command::Plan(plan_cli)) => run_plan_command(plan_cli),
        Some(Command::Repo(repo_cli)) => run_repo_command(repo_cli),
        Some(Command::RepoSafety(repo_safety_cli)) => run_repo_safety_command(repo_safety_cli),
        Some(Command::UpdateCheck(args)) => emit_text(update_check::check(&args)),
        Some(Command::Workflow(workflow_cli)) => run_workflow_command(workflow_cli),
        None => render_help_and_exit(),
    }
}

fn run_config_command(config_cli: cli::config::ConfigCli) -> std::process::ExitCode {
    match config_cli.command {
        cli::config::ConfigCommand::Get(args) => emit_text(config::get(&args)),
        cli::config::ConfigCommand::Set(args) => emit_text(config::set(&args)),
        cli::config::ConfigCommand::List => emit_text(config::list()),
    }
}

fn run_plan_command(plan_cli: cli::PlanCli) -> std::process::ExitCode {
    match plan_cli.command {
        PlanCommand::Contract(plan_contract_cli) => run_plan_contract_command(plan_contract_cli),
        PlanCommand::Execution(plan_execution_cli) => {
            run_plan_execution_command(plan_execution_cli)
        }
    }
}

fn run_plan_contract_command(
    plan_contract_cli: cli::plan_contract::PlanContractCli,
) -> std::process::ExitCode {
    match plan_contract_cli.command {
        cli::plan_contract::PlanContractCommand::Lint(args) => contracts::runtime::run_lint(&args),
        cli::plan_contract::PlanContractCommand::AnalyzePlan(args) => {
            contracts::runtime::run_analyze_plan(&args)
        }
        cli::plan_contract::PlanContractCommand::BuildTaskPacket(args) => {
            contracts::runtime::run_build_task_packet(&args)
        }
    }
}

fn run_plan_execution_command(
    plan_execution_cli: cli::plan_execution::PlanExecutionCli,
) -> std::process::ExitCode {
    let current_dir = current_dir_or_dot();
    match execution::state::ExecutionRuntime::discover(&current_dir) {
        Ok(runtime) => run_plan_execution_with_runtime(&runtime, plan_execution_cli.command),
        Err(error) => emit_json::<Value, _>(Err(error)),
    }
}

fn run_plan_execution_with_runtime(
    runtime: &execution::state::ExecutionRuntime,
    command: cli::plan_execution::PlanExecutionCommand,
) -> std::process::ExitCode {
    use cli::plan_execution::PlanExecutionCommand;

    match command {
        PlanExecutionCommand::Status(args) => emit_json(runtime.status(&args)),
        PlanExecutionCommand::Recommend(args) => emit_json(runtime.recommend(&args)),
        PlanExecutionCommand::Preflight(args) => emit_json(runtime.preflight(&args)),
        PlanExecutionCommand::Internal(internal) => {
            run_internal_plan_execution_command(runtime, internal)
        }
        PlanExecutionCommand::RebuildEvidence(args) => run_rebuild_evidence_command(runtime, &args),
        PlanExecutionCommand::GateContract(args) => emit_json(runtime.gate_contract(&args)),
        PlanExecutionCommand::RecordContract(args) => emit_json(runtime.record_contract(&args)),
        PlanExecutionCommand::GateEvaluator(args) => emit_json(runtime.gate_evaluator(&args)),
        PlanExecutionCommand::RecordEvaluation(args) => emit_json(runtime.record_evaluation(&args)),
        PlanExecutionCommand::GateHandoff(args) => emit_json(runtime.gate_handoff(&args)),
        PlanExecutionCommand::RecordHandoff(args) => emit_json(runtime.record_handoff(&args)),
        PlanExecutionCommand::GateReview(args) => emit_json(runtime.gate_review(&args)),
        PlanExecutionCommand::RecordReviewDispatch(args) => {
            emit_json(runtime.record_review_dispatch(&args))
        }
        PlanExecutionCommand::RepairReviewState(args) => emit_json(
            execution::review_state::repair_review_state_command(runtime, &args),
        ),
        PlanExecutionCommand::ExplainReviewState(args) => emit_json(
            execution::review_state::explain_review_state(runtime, &args),
        ),
        PlanExecutionCommand::GateFinish(args) => emit_json(runtime.gate_finish(&args)),
        PlanExecutionCommand::CloseCurrentTask(args) => {
            emit_json(execution::mutate::close_current_task(runtime, &args))
        }
        PlanExecutionCommand::RecordBranchClosure(args) => {
            emit_json(execution::mutate::record_branch_closure(runtime, &args))
        }
        PlanExecutionCommand::RecordReleaseReadiness(args) => {
            emit_json(execution::mutate::record_release_readiness(runtime, &args))
        }
        PlanExecutionCommand::AdvanceLateStage(args) => {
            emit_json(execution::mutate::advance_late_stage(runtime, &args))
        }
        PlanExecutionCommand::RecordFinalReview(args) => {
            emit_json(execution::mutate::record_final_review(runtime, &args))
        }
        PlanExecutionCommand::RecordQa(args) => {
            emit_json(execution::mutate::record_qa(runtime, &args))
        }
        PlanExecutionCommand::Begin(args) => emit_json(execution::mutate::begin(runtime, &args)),
        PlanExecutionCommand::Note(args) => emit_json(execution::mutate::note(runtime, &args)),
        PlanExecutionCommand::Complete(args) => {
            emit_json(execution::mutate::complete(runtime, &args))
        }
        PlanExecutionCommand::Reopen(args) => emit_json(execution::mutate::reopen(runtime, &args)),
        PlanExecutionCommand::Transfer(args) => {
            emit_json(execution::mutate::transfer(runtime, &args))
        }
    }
}

fn run_internal_plan_execution_command(
    runtime: &execution::state::ExecutionRuntime,
    internal: cli::plan_execution::InternalPlanExecutionCli,
) -> std::process::ExitCode {
    match internal.command {
        cli::plan_execution::InternalPlanExecutionCommand::ReconcileReviewState(args) => emit_json(
            execution::review_state::reconcile_review_state(runtime, &args),
        ),
    }
}

fn run_rebuild_evidence_command(
    runtime: &execution::state::ExecutionRuntime,
    args: &cli::plan_execution::RebuildEvidenceArgs,
) -> std::process::ExitCode {
    let result = execution::mutate::rebuild_evidence(runtime, args);
    if args.json() {
        return emit_json_with_exit(result);
    }
    match result {
        Ok(output) => {
            print!("{}", output.render_text());
            std::process::ExitCode::from(output.exit_code())
        }
        Err(error) => {
            let failure: JsonFailure = error;
            eprintln!(
                "error error_class={} message={}",
                serialize_or_placeholder(&failure.error_class),
                serialize_or_placeholder(&failure.message),
            );
            std::process::ExitCode::from(1)
        }
    }
}

fn run_repo_command(repo_cli: cli::RepoCli) -> std::process::ExitCode {
    match repo_cli.command {
        RepoCommand::Slug(_) => {
            emit_text::<JsonFailure>(Ok(render_slug_output(&current_dir_or_dot())))
        }
        RepoCommand::RuntimeRoot(args) => run_runtime_root_command(&args),
    }
}

fn run_runtime_root_command(args: &cli::runtime_root::RuntimeRootCli) -> std::process::ExitCode {
    if args.json {
        return emit_json(runtime_root::resolve_current_output());
    }
    if args.path {
        return emit_text(runtime_root::resolve_current_path_output());
    }
    match args.field {
        Some(RuntimeRootFieldCli::UpgradeEligible) => {
            emit_text(runtime_root::resolve_current_field_output(
                runtime_root::RuntimeRootField::UpgradeEligible,
            ))
        }
        None => emit_json::<Value, JsonFailure>(Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "repo runtime-root requires either --json, --path, or --field.",
        ))),
    }
}

fn run_repo_safety_command(
    repo_safety_cli: cli::repo_safety::RepoSafetyCli,
) -> std::process::ExitCode {
    let current_dir = current_dir_or_dot();
    match repo_safety::RepoSafetyRuntime::discover(&current_dir) {
        Ok(runtime) => match repo_safety_cli.command {
            cli::repo_safety::RepoSafetyCommand::Check(args) => emit_json(runtime.check(&args)),
            cli::repo_safety::RepoSafetyCommand::Approve(args) => emit_json(runtime.approve(&args)),
        },
        Err(error) => emit_json::<Value, JsonFailure>(Err(error.into())),
    }
}

fn run_workflow_command(workflow_cli: cli::workflow::WorkflowCli) -> std::process::ExitCode {
    let current_dir = current_dir_or_dot();
    match workflow_cli.command {
        cli::workflow::WorkflowCommand::Status(args) => {
            run_workflow_status_command(&current_dir, &args)
        }
        cli::workflow::WorkflowCommand::Resolve => run_workflow_resolve_command(&current_dir),
        cli::workflow::WorkflowCommand::Expect(args) => {
            run_workflow_expect_command(&current_dir, &args)
        }
        cli::workflow::WorkflowCommand::Sync(args) => {
            run_workflow_sync_command(&current_dir, &args)
        }
        cli::workflow::WorkflowCommand::PlanFidelity(plan_fidelity_cli) => {
            run_workflow_plan_fidelity_command(&current_dir, plan_fidelity_cli)
        }
        cli::workflow::WorkflowCommand::Next => emit_text(
            workflow::operator::render_next(&current_dir).map_err(map_read_only_workflow_failure),
        ),
        cli::workflow::WorkflowCommand::Artifacts => emit_text(
            workflow::operator::render_artifacts(&current_dir)
                .map_err(map_read_only_workflow_failure),
        ),
        cli::workflow::WorkflowCommand::Explain => emit_text(
            workflow::operator::render_explain(&current_dir)
                .map_err(map_read_only_workflow_failure),
        ),
        cli::workflow::WorkflowCommand::Phase(args) => {
            run_workflow_phase_command(&current_dir, &args)
        }
        cli::workflow::WorkflowCommand::Doctor(args) => {
            run_workflow_doctor_command(&current_dir, &args)
        }
        cli::workflow::WorkflowCommand::Handoff(args) => {
            run_workflow_handoff_command(&current_dir, &args)
        }
        cli::workflow::WorkflowCommand::Operator(args) => {
            run_workflow_operator_command(&current_dir, &args)
        }
        cli::workflow::WorkflowCommand::RecordPivot(args) => {
            run_workflow_record_pivot_command(&current_dir, &args)
        }
        cli::workflow::WorkflowCommand::Preflight(args) => {
            run_workflow_preflight_command(&current_dir, &args)
        }
        cli::workflow::WorkflowCommand::Gate(gate_cli) => {
            run_workflow_gate_command(&current_dir, gate_cli)
        }
    }
}

fn run_workflow_status_command(
    current_dir: &Path,
    args: &cli::workflow::StatusArgs,
) -> std::process::ExitCode {
    match workflow::status::WorkflowRuntime::discover(current_dir) {
        Ok(mut runtime) => {
            let route = if args.refresh {
                runtime.status_refresh()
            } else {
                runtime.status()
            };
            if args.summary {
                emit_text(route.map(render_workflow_status_summary))
            } else {
                emit_json(route)
            }
        }
        Err(error) => emit_json::<Value, JsonFailure>(Err(error.into())),
    }
}

fn run_workflow_resolve_command(current_dir: &Path) -> std::process::ExitCode {
    match workflow::status::WorkflowRuntime::discover_read_only(current_dir) {
        Ok(runtime) => emit_workflow_resolve_json(runtime.resolve().map_err(JsonFailure::from)),
        Err(error) => emit_workflow_resolve_json(Err(map_read_only_workflow_failure(error.into()))),
    }
}

fn run_workflow_expect_command(
    current_dir: &Path,
    args: &cli::workflow::ExpectArgs,
) -> std::process::ExitCode {
    match workflow::status::WorkflowRuntime::discover(current_dir) {
        Ok(mut runtime) => emit_json(runtime.expect(args.artifact, &args.path)),
        Err(error) => emit_json::<Value, JsonFailure>(Err(error.into())),
    }
}

fn run_workflow_sync_command(
    current_dir: &Path,
    args: &cli::workflow::SyncArgs,
) -> std::process::ExitCode {
    match workflow::status::WorkflowRuntime::discover(current_dir) {
        Ok(mut runtime) => emit_json(runtime.sync(args.artifact, args.path.as_deref())),
        Err(error) => emit_json::<Value, JsonFailure>(Err(error.into())),
    }
}

fn run_workflow_plan_fidelity_command(
    current_dir: &Path,
    plan_fidelity_cli: cli::workflow::WorkflowPlanFidelityCli,
) -> std::process::ExitCode {
    match plan_fidelity_cli.command {
        cli::workflow::WorkflowPlanFidelityCommand::Record(args) => {
            let result = workflow::status::record_plan_fidelity_receipt(current_dir, &args);
            if args.json {
                emit_json(result)
            } else {
                emit_text(result.map(workflow::status::render_plan_fidelity_record))
            }
        }
    }
}

fn run_workflow_phase_command(
    current_dir: &Path,
    args: &cli::workflow::PhaseArgs,
) -> std::process::ExitCode {
    if args.json {
        emit_json(workflow::operator::phase(current_dir).map_err(map_read_only_workflow_failure))
    } else {
        emit_text(
            workflow::operator::render_phase(current_dir).map_err(map_read_only_workflow_failure),
        )
    }
}

fn run_workflow_doctor_command(
    current_dir: &Path,
    args: &cli::workflow::DoctorArgs,
) -> std::process::ExitCode {
    if args.json {
        emit_json(
            workflow::operator::doctor_with_args(current_dir, args)
                .map_err(map_read_only_workflow_failure),
        )
    } else {
        emit_text(
            workflow::operator::render_doctor_with_args(current_dir, args)
                .map_err(map_read_only_workflow_failure),
        )
    }
}

fn run_workflow_handoff_command(
    current_dir: &Path,
    args: &cli::workflow::JsonModeArgs,
) -> std::process::ExitCode {
    if args.json {
        emit_json(workflow::operator::handoff(current_dir).map_err(map_read_only_workflow_failure))
    } else {
        emit_text(
            workflow::operator::render_handoff(current_dir).map_err(map_read_only_workflow_failure),
        )
    }
}

fn run_workflow_operator_command(
    current_dir: &Path,
    args: &cli::workflow::OperatorArgs,
) -> std::process::ExitCode {
    let result = workflow::operator::operator(current_dir, args);
    if args.json {
        emit_json(result.map_err(map_read_only_workflow_failure))
    } else {
        emit_text(result.map(workflow::operator::render_operator))
    }
}

fn run_workflow_record_pivot_command(
    current_dir: &Path,
    args: &cli::workflow::RecordPivotArgs,
) -> std::process::ExitCode {
    let result = workflow::pivot::record_pivot(current_dir, args);
    if args.json {
        emit_json(result)
    } else {
        emit_text(result.map(workflow::pivot::render_pivot_record))
    }
}

fn run_workflow_preflight_command(
    current_dir: &Path,
    args: &cli::workflow::PlanArgs,
) -> std::process::ExitCode {
    let result = workflow::operator::preflight(current_dir, args);
    if args.json {
        emit_json(result)
    } else {
        emit_text(result.map(|gate| workflow::operator::render_gate("Execution preflight", &gate)))
    }
}

fn run_workflow_gate_command(
    current_dir: &Path,
    gate_cli: cli::workflow::WorkflowGateCli,
) -> std::process::ExitCode {
    match gate_cli.command {
        cli::workflow::WorkflowGateCommand::Review(args) => run_workflow_named_gate(
            current_dir,
            &args,
            "Review gate",
            workflow::operator::gate_review,
        ),
        cli::workflow::WorkflowGateCommand::Finish(args) => run_workflow_named_gate(
            current_dir,
            &args,
            "Finish gate",
            workflow::operator::gate_finish,
        ),
    }
}

fn run_workflow_named_gate<F>(
    current_dir: &Path,
    args: &cli::workflow::PlanArgs,
    title: &str,
    gate_fn: F,
) -> std::process::ExitCode
where
    F: Fn(&Path, &cli::workflow::PlanArgs) -> Result<execution::state::GateResult, JsonFailure>,
{
    let result = gate_fn(current_dir, args);
    if args.json {
        emit_json(result)
    } else {
        emit_text(result.map(|gate| workflow::operator::render_gate(title, &gate)))
    }
}

fn render_help_and_exit() -> std::process::ExitCode {
    let mut command = cli::Cli::command();
    print!("{}", command.render_help());
    println!();
    std::process::ExitCode::SUCCESS
}

fn current_dir_or_dot() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn serialize_or_placeholder<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| String::from("\"<serialization-error>\""))
}

fn canonicalized_args() -> Result<Vec<OsString>, JsonFailure> {
    let args = std::env::args_os().collect::<Vec<_>>();
    let argv0 = args
        .first()
        .cloned()
        .unwrap_or_else(|| OsString::from("featureforge"));
    let file_name = Path::new(&argv0)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("featureforge");
    let normalized = file_name.strip_suffix(".exe").unwrap_or(file_name);
    if normalized.starts_with("featureforge-") && normalized != "featureforge" {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!(
                "legacy argv0 alias `{normalized}` is not supported; invoke `featureforge <subcommand>` instead."
            ),
        ));
    }
    Ok(args)
}

fn emit_json<T, E>(result: Result<T, E>) -> std::process::ExitCode
where
    T: serde::Serialize,
    E: Into<JsonFailure>,
{
    match result {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(json) => {
                println!("{json}");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("Could not serialize workflow output: {error}");
                std::process::ExitCode::from(1)
            }
        },
        Err(error) => match serde_json::to_string(&error.into()) {
            Ok(json) => {
                eprintln!("{json}");
                std::process::ExitCode::from(1)
            }
            Err(serialize_error) => {
                eprintln!("Could not serialize error output: {serialize_error}");
                std::process::ExitCode::from(1)
            }
        },
    }
}

fn emit_json_with_exit<T, E>(result: Result<T, E>) -> std::process::ExitCode
where
    T: serde::Serialize + ExitCodeJson,
    E: Into<JsonFailure>,
{
    match result {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(json) => {
                println!("{json}");
                std::process::ExitCode::from(value.exit_code())
            }
            Err(error) => {
                eprintln!("Could not serialize workflow output: {error}");
                std::process::ExitCode::from(1)
            }
        },
        Err(error) => match serde_json::to_string(&error.into()) {
            Ok(json) => {
                eprintln!("{json}");
                std::process::ExitCode::from(1)
            }
            Err(serialize_error) => {
                eprintln!("Could not serialize error output: {serialize_error}");
                std::process::ExitCode::from(1)
            }
        },
    }
}

fn emit_text<E>(result: Result<String, E>) -> std::process::ExitCode
where
    E: Into<JsonFailure>,
{
    match result {
        Ok(text) => {
            if !text.is_empty() {
                print!("{text}");
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            let failure = error.into();
            eprintln!("{}: {}", failure.error_class, failure.message);
            std::process::ExitCode::from(1)
        }
    }
}

fn emit_workflow_resolve_json(
    result: Result<workflow::status::WorkflowRoute, JsonFailure>,
) -> std::process::ExitCode {
    match result {
        Ok(route) => {
            let manifest_source_path = route.manifest_path.clone();
            match serde_json::to_value(route) {
                Ok(Value::Object(mut object)) => {
                    object.insert(
                        String::from("outcome"),
                        Value::String(String::from("resolved")),
                    );
                    object.insert(
                        String::from("manifest_source_path"),
                        Value::String(manifest_source_path),
                    );
                    match serde_json::to_string(&Value::Object(object)) {
                        Ok(json) => {
                            println!("{json}");
                            std::process::ExitCode::SUCCESS
                        }
                        Err(error) => {
                            eprintln!("Could not serialize workflow resolve output: {error}");
                            std::process::ExitCode::from(1)
                        }
                    }
                }
                Ok(_) => {
                    eprintln!("Could not serialize workflow resolve output: expected object");
                    std::process::ExitCode::from(1)
                }
                Err(error) => {
                    eprintln!("Could not serialize workflow resolve output: {error}");
                    std::process::ExitCode::from(1)
                }
            }
        }
        Err(failure) => match serde_json::to_string(&json!({
            "outcome": "runtime_failure",
            "failure_class": failure.error_class,
            "message": failure.message,
        })) {
            Ok(json) => {
                eprintln!("{json}");
                std::process::ExitCode::from(1)
            }
            Err(error) => {
                eprintln!("Could not serialize workflow resolve failure: {error}");
                std::process::ExitCode::from(1)
            }
        },
    }
}

fn render_slug_output(current_dir: &Path) -> String {
    let identity = git::discover_slug_identity(current_dir);
    format!(
        "SLUG={}\nBRANCH={}\n",
        shell_quote(&identity.repo_slug),
        shell_quote(&identity.safe_branch)
    )
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

fn render_workflow_status_summary(route: workflow::status::WorkflowRoute) -> String {
    let workflow::status::WorkflowRoute {
        status,
        next_skill,
        spec_path,
        plan_path,
        reason,
        ..
    } = route;
    let next = if status == "implementation_ready" {
        String::from("execution_preflight")
    } else {
        next_skill
    };
    format!("status={status} next={next} spec={spec_path} plan={plan_path} reason={reason}\n")
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}
