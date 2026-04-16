use std::path::Path;
use std::process::{ExitStatus, Output};

use clap::Parser;
use featureforge::cli::workflow::{WorkflowCommand, WorkflowGateCommand};
use featureforge::cli::{Cli, Command as RootCommand};
use featureforge::diagnostics::{FailureClass, JsonFailure};
use featureforge::execution::state::ExecutionRuntime;
use featureforge::workflow::{operator, status};
use serde::Serialize;
use serde_json::{Value, json};

enum DirectWorkflowEmission {
    Json(Result<Vec<u8>, JsonFailure>),
    Text(Result<String, JsonFailure>),
    WorkflowResolve(Box<Result<status::WorkflowRoute, JsonFailure>>),
}

pub fn try_run_workflow_output_direct(
    repo: &Path,
    state: &Path,
    args: &[&str],
    context: &str,
) -> Result<Option<Output>, String> {
    Ok(
        match try_run_workflow_emission_direct(repo, state, args, context)? {
            Some(DirectWorkflowEmission::Json(result)) => Some(json_output_result(result)),
            Some(DirectWorkflowEmission::Text(result)) => Some(text_output_result(result)),
            Some(DirectWorkflowEmission::WorkflowResolve(result)) => {
                Some(workflow_resolve_output_result(*result))
            }
            None => None,
        },
    )
}

fn try_run_workflow_emission_direct(
    repo: &Path,
    state: &Path,
    args: &[&str],
    _context: &str,
) -> Result<Option<DirectWorkflowEmission>, String> {
    let argv = std::iter::once("featureforge").chain(args.iter().copied());
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(_) => return Ok(None),
    };
    let Some(RootCommand::Workflow(workflow_cli)) = cli.command else {
        return Ok(None);
    };

    let load_runtime = || execution_runtime(repo, state);
    let load_read_only_runtime = || read_only_execution_runtime(repo, state);
    let emission = match workflow_cli.command {
        WorkflowCommand::Status(args) if !args.summary => {
            match status::WorkflowRuntime::discover_for_state_dir(repo, state) {
                Ok(mut workflow) => {
                    if args.refresh {
                        DirectWorkflowEmission::Json(serialize_json(
                            workflow.status_refresh().map_err(JsonFailure::from),
                        ))
                    } else {
                        DirectWorkflowEmission::Json(serialize_json(
                            workflow.status().map_err(JsonFailure::from),
                        ))
                    }
                }
                Err(_) => return Ok(None),
            }
        }
        WorkflowCommand::Status(args) => {
            match status::WorkflowRuntime::discover_for_state_dir(repo, state) {
                Ok(mut workflow) => {
                    let route = if args.refresh {
                        workflow.status_refresh().map_err(JsonFailure::from)
                    } else {
                        workflow.status().map_err(JsonFailure::from)
                    };
                    DirectWorkflowEmission::Text(route.map(render_workflow_status_summary))
                }
                // Non-repo discovery errors include platform-specific path rendering from the
                // real binary, so keep that boundary on the subprocess path.
                Err(_) => return Ok(None),
            }
        }
        WorkflowCommand::Resolve => {
            match status::WorkflowRuntime::discover_read_only_for_state_dir(repo, state) {
                Ok(workflow) => {
                    let route = workflow
                        .resolve()
                        .map_err(JsonFailure::from)
                        .map_err(map_read_only_workflow_failure);
                    DirectWorkflowEmission::WorkflowResolve(Box::new(route))
                }
                Err(error) => DirectWorkflowEmission::WorkflowResolve(Box::new(Err(
                    map_read_only_workflow_failure(JsonFailure::from(error)),
                ))),
            }
        }
        WorkflowCommand::Expect(args) => {
            match status::WorkflowRuntime::discover_for_state_dir(repo, state) {
                Ok(mut workflow) => DirectWorkflowEmission::Json(serialize_json(
                    workflow
                        .expect(args.artifact, &args.path)
                        .map_err(JsonFailure::from),
                )),
                Err(error) => DirectWorkflowEmission::Json(Err(JsonFailure::from(error))),
            }
        }
        WorkflowCommand::Sync(args) => {
            match status::WorkflowRuntime::discover_for_state_dir(repo, state) {
                Ok(mut workflow) => DirectWorkflowEmission::Json(serialize_json(
                    workflow
                        .sync(args.artifact, args.path.as_deref())
                        .map_err(JsonFailure::from),
                )),
                Err(error) => DirectWorkflowEmission::Json(Err(JsonFailure::from(error))),
            }
        }
        // Plan-fidelity still exercises a workflow-owned shell boundary around state-dir,
        // CLI-shaped JSON failures, and manifest discovery, so tests that cover that contract
        // intentionally keep the real subprocess path.
        WorkflowCommand::PlanFidelity(_) => return Ok(None),
        WorkflowCommand::Next => DirectWorkflowEmission::Text(
            load_read_only_runtime()
                .and_then(|runtime| operator::render_next_for_runtime(&runtime)),
        ),
        WorkflowCommand::Artifacts => DirectWorkflowEmission::Text(
            load_read_only_runtime()
                .and_then(|runtime| operator::render_artifacts_for_runtime(&runtime)),
        ),
        WorkflowCommand::Explain => DirectWorkflowEmission::Text(
            load_read_only_runtime()
                .and_then(|runtime| operator::render_explain_for_runtime(&runtime)),
        ),
        WorkflowCommand::Phase(args) if args.json => DirectWorkflowEmission::Json(serialize_json(
            load_read_only_runtime().and_then(|runtime| operator::phase_for_runtime(&runtime)),
        )),
        WorkflowCommand::Phase(_) => DirectWorkflowEmission::Text(
            load_read_only_runtime()
                .and_then(|runtime| operator::render_phase_for_runtime(&runtime)),
        ),
        WorkflowCommand::Doctor(args) if args.json => DirectWorkflowEmission::Json(serialize_json(
            load_read_only_runtime()
                .and_then(|runtime| operator::doctor_for_runtime_with_args(&runtime, &args)),
        )),
        WorkflowCommand::Doctor(args) => DirectWorkflowEmission::Text(
            load_read_only_runtime()
                .and_then(|runtime| operator::render_doctor_for_runtime_with_args(&runtime, &args)),
        ),
        WorkflowCommand::Handoff(args) if args.json => {
            DirectWorkflowEmission::Json(serialize_json(
                load_read_only_runtime()
                    .and_then(|runtime| operator::handoff_for_runtime(&runtime)),
            ))
        }
        WorkflowCommand::Handoff(_) => DirectWorkflowEmission::Text(
            load_read_only_runtime()
                .and_then(|runtime| operator::render_handoff_for_runtime(&runtime)),
        ),
        WorkflowCommand::Operator(args) if args.json => {
            DirectWorkflowEmission::Json(serialize_json(
                load_read_only_runtime()
                    .and_then(|runtime| operator::operator_for_runtime(&runtime, &args)),
            ))
        }
        WorkflowCommand::RecordPivot(args) if args.json => {
            DirectWorkflowEmission::Json(serialize_json(load_runtime().and_then(|runtime| {
                featureforge::workflow::pivot::record_pivot_for_runtime(&runtime, &args)
                    .map_err(JsonFailure::from)
            })))
        }
        WorkflowCommand::RecordPivot(_) => return Ok(None),
        WorkflowCommand::Preflight(args) if args.json => {
            DirectWorkflowEmission::Json(serialize_json(
                load_runtime().and_then(|runtime| operator::preflight_for_runtime(&runtime, &args)),
            ))
        }
        WorkflowCommand::Gate(gate_cli) => match gate_cli.command {
            WorkflowGateCommand::Review(args) if args.json => {
                DirectWorkflowEmission::Json(serialize_json(
                    load_runtime()
                        .and_then(|runtime| operator::gate_review_for_runtime(&runtime, &args)),
                ))
            }
            WorkflowGateCommand::Finish(args) if args.json => {
                DirectWorkflowEmission::Json(serialize_json(
                    load_runtime()
                        .and_then(|runtime| operator::gate_finish_for_runtime(&runtime, &args)),
                ))
            }
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };

    Ok(Some(emission))
}

fn serialize_resolved_route(route: status::WorkflowRoute) -> Vec<u8> {
    let manifest_source_path = route.manifest_path.clone();
    let mut value =
        serde_json::to_value(route).expect("workflow resolve route should serialize to JSON");
    let object = value
        .as_object_mut()
        .expect("workflow resolve route should serialize as a JSON object");
    object.insert(String::from("outcome"), Value::from("resolved"));
    object.insert(
        String::from("manifest_source_path"),
        Value::from(manifest_source_path),
    );
    json_line(&value).expect("workflow resolve route should serialize to JSON")
}

fn serialize_json<T: Serialize>(value: Result<T, JsonFailure>) -> Result<Vec<u8>, JsonFailure> {
    value.map(|value| json_line(&value).expect("direct workflow output should serialize to JSON"))
}

fn render_workflow_status_summary(route: status::WorkflowRoute) -> String {
    let next = if route.status == "implementation_ready" {
        "execution_preflight"
    } else {
        route.next_skill.as_str()
    };
    format!(
        "status={} next={} spec={} plan={} reason={}\n",
        route.status, next, route.spec_path, route.plan_path, route.reason
    )
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

fn workflow_resolve_output_result(result: Result<status::WorkflowRoute, JsonFailure>) -> Output {
    match result {
        Ok(route) => output_with_code(0, serialize_resolved_route(route), Vec::new()),
        Err(failure) => output_with_code(
            1,
            Vec::new(),
            json_line(&json!({
                "outcome": "runtime_failure",
                "failure_class": failure.error_class,
                "message": failure.message,
            }))
            .expect("direct workflow resolve failure should serialize"),
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
