use std::path::Path;
use std::process::{ExitStatus, Output};

use clap::{Parser, Subcommand};
use featureforge::cli::workflow::{
    ArtifactKind, DoctorArgs, ExpectArgs, JsonModeArgs, PhaseArgs, StatusArgs,
    WorkflowPlanFidelityCli, WorkflowPlanFidelityCommand,
};
use featureforge::cli::{Cli, Command as RootCommand};
use featureforge::diagnostics::{FailureClass, JsonFailure};
use featureforge::execution::state::ExecutionRuntime;
use featureforge::workflow::{operator, status};
use serde::Serialize;
enum DirectWorkflowEmission {
    Json(Result<Vec<u8>, JsonFailure>),
    Text(Result<String, JsonFailure>),
}

// These test-only parsers keep semantic coverage for removed workflow compatibility
// commands off the shipped CLI surface. Compiled-CLI rejection stays covered separately.
#[derive(Debug, Parser)]
struct LegacyWorkflowCli {
    #[command(subcommand)]
    command: LegacyWorkflowCommand,
}

#[derive(Debug, Subcommand)]
enum LegacyWorkflowCommand {
    Status(StatusArgs),
    Expect(ExpectArgs),
    Sync(LegacySyncArgs),
    Phase(PhaseArgs),
    Doctor(DoctorArgs),
    Handoff(JsonModeArgs),
    PlanFidelity(WorkflowPlanFidelityCli),
}

#[derive(Debug, Clone, clap::Args)]
struct LegacySyncArgs {
    #[arg(long, value_enum)]
    artifact: ArtifactKind,
    #[arg(long)]
    path: Option<std::path::PathBuf>,
}

pub fn try_run_workflow_output_direct(
    repo: &Path,
    state: &Path,
    args: &[&str],
    context: &str,
    allow_legacy_removed_commands: bool,
) -> Result<Option<Output>, String> {
    Ok(
        match try_run_workflow_emission_direct(
            repo,
            state,
            args,
            context,
            allow_legacy_removed_commands,
        )? {
            Some(DirectWorkflowEmission::Json(result)) => Some(json_output_result(result)),
            Some(DirectWorkflowEmission::Text(result)) => Some(text_output_result(result)),
            None => None,
        },
    )
}

fn try_run_workflow_emission_direct(
    repo: &Path,
    state: &Path,
    args: &[&str],
    _context: &str,
    allow_legacy_removed_commands: bool,
) -> Result<Option<DirectWorkflowEmission>, String> {
    let load_read_only_runtime = || read_only_execution_runtime(repo, state);
    let emission =
        if let Ok(cli) =
            Cli::try_parse_from(std::iter::once("featureforge").chain(args.iter().copied()))
        {
            let Some(RootCommand::Workflow(workflow_cli)) = cli.command else {
                return Ok(None);
            };
            match workflow_cli.command {
                featureforge::cli::workflow::WorkflowCommand::Operator(args) if args.json => {
                    DirectWorkflowEmission::Json(serialize_json(
                        load_read_only_runtime()
                            .and_then(|runtime| operator::operator_for_runtime(&runtime, &args)),
                    ))
                }
                _ => return Ok(None),
            }
        } else {
            if !allow_legacy_removed_commands {
                return Ok(None);
            }
            let legacy = match LegacyWorkflowCli::try_parse_from(
                std::iter::once("featureforge").chain(args.iter().skip(1).copied()),
            ) {
                Ok(cli) => cli,
                Err(_) => return Ok(None),
            };
            match legacy.command {
                LegacyWorkflowCommand::Status(args) => {
                    match status::WorkflowRuntime::discover_for_state_dir(repo, state) {
                        Ok(mut workflow) => {
                            let route = if args.refresh {
                                workflow.status_refresh()
                            } else {
                                workflow.status()
                            };
                            DirectWorkflowEmission::Json(serialize_json(
                                route.map_err(JsonFailure::from),
                            ))
                        }
                        Err(error) => DirectWorkflowEmission::Json(Err(JsonFailure::from(error))),
                    }
                }
                LegacyWorkflowCommand::Expect(args) => {
                    match status::WorkflowRuntime::discover_for_state_dir(repo, state) {
                        Ok(mut workflow) => DirectWorkflowEmission::Json(serialize_json(
                            workflow
                                .expect(args.artifact, &args.path)
                                .map_err(JsonFailure::from),
                        )),
                        Err(error) => DirectWorkflowEmission::Json(Err(JsonFailure::from(error))),
                    }
                }
                LegacyWorkflowCommand::Sync(args) => {
                    match status::WorkflowRuntime::discover_for_state_dir(repo, state) {
                        Ok(mut workflow) => DirectWorkflowEmission::Json(serialize_json(
                            workflow
                                .sync(args.artifact, args.path.as_deref())
                                .map_err(JsonFailure::from),
                        )),
                        Err(error) => DirectWorkflowEmission::Json(Err(JsonFailure::from(error))),
                    }
                }
                LegacyWorkflowCommand::Phase(args) if args.json => {
                    DirectWorkflowEmission::Json(serialize_json(
                        load_read_only_runtime()
                            .and_then(|runtime| operator::phase_for_runtime(&runtime)),
                    ))
                }
                LegacyWorkflowCommand::Phase(_) => DirectWorkflowEmission::Text(
                    load_read_only_runtime()
                        .and_then(|runtime| operator::render_phase_for_runtime(&runtime)),
                ),
                LegacyWorkflowCommand::Doctor(args) if args.json => {
                    DirectWorkflowEmission::Json(serialize_json(load_read_only_runtime().and_then(
                        |runtime| operator::doctor_for_runtime_with_args(&runtime, &args),
                    )))
                }
                LegacyWorkflowCommand::Doctor(args) => {
                    DirectWorkflowEmission::Text(load_read_only_runtime().and_then(|runtime| {
                        operator::render_doctor_for_runtime_with_args(&runtime, &args)
                    }))
                }
                LegacyWorkflowCommand::Handoff(args) if args.json => {
                    DirectWorkflowEmission::Json(serialize_json(
                        load_read_only_runtime()
                            .and_then(|runtime| operator::handoff_for_runtime(&runtime)),
                    ))
                }
                LegacyWorkflowCommand::Handoff(_) => DirectWorkflowEmission::Text(
                    load_read_only_runtime()
                        .and_then(|runtime| operator::render_handoff_for_runtime(&runtime)),
                ),
                LegacyWorkflowCommand::PlanFidelity(cli) => match cli.command {
                    WorkflowPlanFidelityCommand::Record(args) if args.json => {
                        DirectWorkflowEmission::Json(serialize_json(
                            status::record_plan_fidelity_receipt_with_state_dir(repo, state, &args)
                                .map_err(JsonFailure::from),
                        ))
                    }
                    WorkflowPlanFidelityCommand::Record(args) => DirectWorkflowEmission::Text(
                        status::record_plan_fidelity_receipt_with_state_dir(repo, state, &args)
                            .map(status::render_plan_fidelity_record)
                            .map_err(JsonFailure::from),
                    ),
                },
            }
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

fn text_output_result(result: Result<String, JsonFailure>) -> Output {
    match result {
        Ok(text) => output_with_code(0, text.into_bytes(), Vec::new()),
        Err(failure) => output_with_code(
            1,
            Vec::new(),
            format!("{}: {}\n", failure.error_class, failure.message).into_bytes(),
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
