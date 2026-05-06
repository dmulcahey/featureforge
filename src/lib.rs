use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use clap::{CommandFactory, Parser};
use cli::runtime_root::RuntimeRootFieldCli;
use cli::{Command, PlanCommand, RepoCommand};
use diagnostics::{DiagnosticError, FailureClass, JsonFailure};
use execution::live_mutation_guard::{
    deny_workspace_runtime_live_mutation,
    deny_workspace_runtime_live_mutation_for_execution_runtime,
};
use execution::runtime_provenance::RuntimeProvenance;
use serde_json::Value;

pub mod benchmarking;
pub mod cli;
pub mod config;
pub mod contracts;
pub mod diagnostics;
pub mod execution;
pub mod git;
pub mod instructions;
pub mod output;
pub mod paths;
pub mod repo_safety;
pub mod runtime_root;
pub mod self_hosting;
#[cfg(test)]
pub mod test_support;
pub mod update_check;
pub mod workflow;

pub fn run() -> std::process::ExitCode {
    let args = match canonicalized_args() {
        Ok(args) => args,
        Err(error) => return emit_json::<Value, JsonFailure>(Err(error)),
    };
    let cli = match cli::Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => match error.kind() {
            clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                print!("{error}");
                return std::process::ExitCode::SUCCESS;
            }
            _ => {
                return emit_json::<Value, JsonFailure>(Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    error.to_string(),
                )));
            }
        },
    };

    match cli.command {
        Some(Command::Config(config_cli)) => match config_cli.command {
            cli::config::ConfigCommand::Get(args) => emit_text(config::get(&args)),
            cli::config::ConfigCommand::Set(args) => emit_text(config::set(&args)),
            cli::config::ConfigCommand::List => emit_text(config::list()),
        },
        Some(Command::Doctor(doctor_cli)) => match doctor_cli.command {
            cli::doctor::DoctorCommand::SelfHosting(args) => {
                let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                let diagnostic = self_hosting::diagnose_self_hosting(&current_dir);
                if args.json {
                    emit_json::<_, JsonFailure>(Ok(diagnostic))
                } else {
                    emit_text::<JsonFailure>(Ok(self_hosting::render_self_hosting_diagnostic(
                        &diagnostic,
                    )))
                }
            }
        },
        Some(Command::Plan(plan_cli)) => match plan_cli.command {
            PlanCommand::Contract(plan_contract_cli) => match plan_contract_cli.command {
                cli::plan_contract::PlanContractCommand::Lint(args) => {
                    contracts::runtime::run_lint(&args)
                }
                cli::plan_contract::PlanContractCommand::AnalyzePlan(args) => {
                    contracts::runtime::run_analyze_plan(&args)
                }
                cli::plan_contract::PlanContractCommand::BuildTaskPacket(args) => {
                    let guard = if args.persist == cli::plan_contract::PersistMode::Yes {
                        let current_dir =
                            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                        let provenance =
                            contracts::runtime::build_task_packet_runtime_provenance(&current_dir);
                        match deny_workspace_runtime_live_mutation(
                            &provenance,
                            "plan contract build-task-packet",
                        ) {
                            Ok(guard) => guard,
                            Err(error) => return emit_json::<Value, _>(Err(error)),
                        }
                    } else {
                        execution::live_mutation_guard::WorkspaceRuntimeLiveMutationGuardOutcome {
                            override_warning: None,
                        }
                    };
                    contracts::runtime::run_build_task_packet_with_workspace_runtime_warning(
                        &args,
                        guard.override_warning.as_deref(),
                    )
                }
            },
            PlanCommand::Execution(plan_execution_cli) => {
                match execution::state::ExecutionRuntime::discover(
                    &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                ) {
                    Ok(runtime) => match plan_execution_cli.command {
                        cli::plan_execution::PlanExecutionCommand::Status(args) => {
                            let runtime_provenance = runtime.runtime_provenance();
                            emit_json_with_runtime_metadata(
                                runtime.status(&args),
                                Some(&runtime_provenance),
                                None,
                            )
                        }
                        cli::plan_execution::PlanExecutionCommand::RepairReviewState(args) => {
                            let guard =
                                match deny_workspace_runtime_live_mutation_for_execution_runtime(
                                    &runtime,
                                    "plan execution repair-review-state",
                                ) {
                                    Ok(guard) => guard,
                                    Err(error) => return emit_json::<Value, _>(Err(error)),
                                };
                            emit_json_with_workspace_runtime_warning(
                                execution::commands::repair_review_state::repair_review_state(
                                    &runtime, &args,
                                ),
                                guard.override_warning.as_deref(),
                            )
                        }
                        cli::plan_execution::PlanExecutionCommand::CloseCurrentTask(args) => {
                            let guard =
                                match deny_workspace_runtime_live_mutation_for_execution_runtime(
                                    &runtime,
                                    "plan execution close-current-task",
                                ) {
                                    Ok(guard) => guard,
                                    Err(error) => return emit_json::<Value, _>(Err(error)),
                                };
                            emit_json_with_workspace_runtime_warning(
                                execution::commands::close_current_task::close_current_task(
                                    &runtime, &args,
                                ),
                                guard.override_warning.as_deref(),
                            )
                        }
                        cli::plan_execution::PlanExecutionCommand::AdvanceLateStage(args) => {
                            let guard =
                                match deny_workspace_runtime_live_mutation_for_execution_runtime(
                                    &runtime,
                                    "plan execution advance-late-stage",
                                ) {
                                    Ok(guard) => guard,
                                    Err(error) => return emit_json::<Value, _>(Err(error)),
                                };
                            emit_json_with_workspace_runtime_warning(
                                execution::commands::advance_late_stage::advance_late_stage(
                                    &runtime, &args,
                                ),
                                guard.override_warning.as_deref(),
                            )
                        }
                        cli::plan_execution::PlanExecutionCommand::Begin(args) => {
                            let guard =
                                match deny_workspace_runtime_live_mutation_for_execution_runtime(
                                    &runtime,
                                    "plan execution begin",
                                ) {
                                    Ok(guard) => guard,
                                    Err(error) => return emit_json::<Value, _>(Err(error)),
                                };
                            emit_json_with_workspace_runtime_warning(
                                execution::commands::begin::begin(&runtime, &args),
                                guard.override_warning.as_deref(),
                            )
                        }
                        cli::plan_execution::PlanExecutionCommand::Complete(args) => {
                            let guard =
                                match deny_workspace_runtime_live_mutation_for_execution_runtime(
                                    &runtime,
                                    "plan execution complete",
                                ) {
                                    Ok(guard) => guard,
                                    Err(error) => return emit_json::<Value, _>(Err(error)),
                                };
                            emit_json_with_workspace_runtime_warning(
                                execution::commands::complete::complete(&runtime, &args),
                                guard.override_warning.as_deref(),
                            )
                        }
                        cli::plan_execution::PlanExecutionCommand::Reopen(args) => {
                            let guard =
                                match deny_workspace_runtime_live_mutation_for_execution_runtime(
                                    &runtime,
                                    "plan execution reopen",
                                ) {
                                    Ok(guard) => guard,
                                    Err(error) => return emit_json::<Value, _>(Err(error)),
                                };
                            emit_json_with_workspace_runtime_warning(
                                execution::commands::reopen::reopen(&runtime, &args),
                                guard.override_warning.as_deref(),
                            )
                        }
                        cli::plan_execution::PlanExecutionCommand::Transfer(args) => {
                            let guard =
                                match deny_workspace_runtime_live_mutation_for_execution_runtime(
                                    &runtime,
                                    "plan execution transfer",
                                ) {
                                    Ok(guard) => guard,
                                    Err(error) => return emit_json::<Value, _>(Err(error)),
                                };
                            emit_json_with_workspace_runtime_warning(
                                execution::commands::transfer::transfer(&runtime, &args),
                                guard.override_warning.as_deref(),
                            )
                        }
                        cli::plan_execution::PlanExecutionCommand::MaterializeProjections(args) => {
                            let guard =
                                match deny_workspace_runtime_live_mutation_for_execution_runtime(
                                    &runtime,
                                    "plan execution materialize-projections",
                                ) {
                                    Ok(guard) => guard,
                                    Err(error) => return emit_json::<Value, _>(Err(error)),
                                };
                            emit_json_with_workspace_runtime_warning(
                                execution::commands::materialize_projections::materialize_projections(
                                    &runtime, &args,
                                ),
                                guard.override_warning.as_deref(),
                            )
                        }
                    },
                    Err(error) => emit_json::<Value, _>(Err(error)),
                }
            }
        },
        Some(Command::Repo(repo_cli)) => match repo_cli.command {
            RepoCommand::Slug(_) => emit_text(render_slug_output(
                &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            )),
            RepoCommand::RuntimeRoot(args) => {
                if args.json {
                    emit_json(runtime_root::resolve_current_output())
                } else if args.path {
                    emit_text(runtime_root::resolve_current_path_output())
                } else if let Some(field) = args.field {
                    let field = match field {
                        RuntimeRootFieldCli::UpgradeEligible => {
                            runtime_root::RuntimeRootField::UpgradeEligible
                        }
                    };
                    emit_text(runtime_root::resolve_current_field_output(field))
                } else {
                    emit_json::<Value, JsonFailure>(Err(JsonFailure::new(
                        FailureClass::InvalidCommandInput,
                        "repo runtime-root requires either --json, --path, or --field.",
                    )))
                }
            }
        },
        Some(Command::RepoSafety(repo_safety_cli)) => {
            match repo_safety::RepoSafetyRuntime::discover(
                &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            ) {
                Ok(runtime) => match repo_safety_cli.command {
                    cli::repo_safety::RepoSafetyCommand::Check(args) => {
                        emit_json(runtime.check(&args))
                    }
                    cli::repo_safety::RepoSafetyCommand::Approve(args) => {
                        let guard = match deny_workspace_runtime_live_mutation(
                            &runtime.runtime_provenance(),
                            "repo-safety approve",
                        ) {
                            Ok(guard) => guard,
                            Err(error) => return emit_json::<Value, _>(Err(error)),
                        };
                        emit_json_with_workspace_runtime_warning(
                            runtime.approve(&args),
                            guard.override_warning.as_deref(),
                        )
                    }
                },
                Err(error) => emit_json::<Value, JsonFailure>(Err(error.into())),
            }
        }
        Some(Command::UpdateCheck(args)) => emit_text(update_check::check(&args)),
        Some(Command::Workflow(workflow_cli)) => {
            let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            match workflow_cli.command {
                cli::workflow::WorkflowCommand::Status(args) => {
                    let result =
                        workflow::status::WorkflowRuntime::discover_read_only(&current_dir)
                            .and_then(|runtime| runtime.status())
                            .map_err(JsonFailure::from);
                    if args.json {
                        emit_json(result)
                    } else {
                        emit_text(result.map(render_workflow_status))
                    }
                }
                cli::workflow::WorkflowCommand::Doctor(args) => {
                    let doctor_args = workflow::operator::DoctorArgs {
                        plan: Some(args.plan),
                        external_review_result_ready: args.external_review_result_ready,
                    };
                    if args.json {
                        emit_json(
                            workflow::operator::doctor_with_args(&current_dir, &doctor_args)
                                .map_err(map_read_only_workflow_failure),
                        )
                    } else {
                        emit_text(
                            workflow::operator::render_doctor_with_args(&current_dir, &doctor_args)
                                .map_err(map_read_only_workflow_failure),
                        )
                    }
                }
                cli::workflow::WorkflowCommand::Operator(args) => {
                    let result = workflow::operator::operator(&current_dir, &args);
                    if args.json {
                        emit_json(result.map_err(map_read_only_workflow_failure))
                    } else {
                        emit_text(result.map(workflow::operator::render_operator))
                    }
                }
            }
        }
        None => {
            let mut command = cli::Cli::command();
            print!("{}", command.render_help());
            println!();
            std::process::ExitCode::SUCCESS
        }
    }
}

fn render_workflow_status(route: workflow::status::WorkflowRoute) -> String {
    format!(
        "status: {}\nnext_skill: {}\nspec_path: {}\nplan_path: {}\ncontract_state: {}\nreason_codes: {}\n",
        route.status,
        route.next_skill,
        route.spec_path,
        route.plan_path,
        route.contract_state,
        if route.reason_codes.is_empty() {
            String::from("none")
        } else {
            route.reason_codes.join(",")
        }
    )
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

fn emit_json_with_workspace_runtime_warning<T, E>(
    result: Result<T, E>,
    workspace_runtime_warning: Option<&str>,
) -> std::process::ExitCode
where
    T: serde::Serialize,
    E: Into<JsonFailure>,
{
    emit_json_with_runtime_metadata(result, None, workspace_runtime_warning)
}

fn emit_json_with_runtime_metadata<T, E>(
    result: Result<T, E>,
    runtime_provenance: Option<&RuntimeProvenance>,
    workspace_runtime_warning: Option<&str>,
) -> std::process::ExitCode
where
    T: serde::Serialize,
    E: Into<JsonFailure>,
{
    match result {
        Ok(value) => match serde_json::to_value(value)
            .map_err(|error| format!("Could not serialize workflow output: {error}"))
            .and_then(|value| {
                serde_json::to_string(&inject_runtime_metadata(
                    value,
                    runtime_provenance,
                    workspace_runtime_warning,
                ))
                .map_err(|error| format!("Could not serialize workflow output: {error}"))
            }) {
            Ok(json) => {
                if let Some(warning) = workspace_runtime_warning {
                    eprintln!("{warning}");
                }
                println!("{json}");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::from(1)
            }
        },
        Err(error) => {
            let failure: JsonFailure = error.into();
            let value = inject_runtime_metadata(
                serde_json::to_value(&failure)
                    .unwrap_or_else(|_| serde_json::json!({"error_class": failure.error_class, "message": failure.message})),
                runtime_provenance,
                workspace_runtime_warning,
            );
            if let Some(warning) = workspace_runtime_warning {
                eprintln!("{warning}");
            }
            match serde_json::to_string(&value) {
                Ok(json) => {
                    eprintln!("{json}");
                    std::process::ExitCode::from(1)
                }
                Err(serialize_error) => {
                    eprintln!("Could not serialize error output: {serialize_error}");
                    std::process::ExitCode::from(1)
                }
            }
        }
    }
}

fn inject_runtime_metadata(
    mut value: Value,
    runtime_provenance: Option<&RuntimeProvenance>,
    workspace_runtime_warning: Option<&str>,
) -> Value {
    let runtime_provenance_value =
        runtime_provenance.and_then(|value| serde_json::to_value(value).ok());
    let warning_value = workspace_runtime_warning.map(|value| Value::String(value.to_owned()));
    if runtime_provenance_value.is_none() && warning_value.is_none() {
        return value;
    }
    match value {
        Value::Object(ref mut object) => {
            if let Some(runtime_provenance_value) = runtime_provenance_value {
                object.insert(String::from("runtime_provenance"), runtime_provenance_value);
            }
            if let Some(warning_value) = warning_value {
                object.insert(
                    String::from("workspace_runtime_live_mutation_warning"),
                    warning_value,
                );
            }
            value
        }
        other => {
            let mut wrapped = serde_json::Map::new();
            wrapped.insert(String::from("value"), other);
            if let Some(runtime_provenance_value) = runtime_provenance_value {
                wrapped.insert(String::from("runtime_provenance"), runtime_provenance_value);
            }
            if let Some(workspace_runtime_warning) = workspace_runtime_warning {
                wrapped.insert(
                    String::from("workspace_runtime_live_mutation_warning"),
                    Value::String(workspace_runtime_warning.to_owned()),
                );
            }
            Value::Object(wrapped)
        }
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

fn render_slug_output(current_dir: &Path) -> Result<String, DiagnosticError> {
    let identity = git::discover_slug_identity(current_dir);
    Ok(format!(
        "SLUG={}\nBRANCH={}\n",
        shell_quote(&identity.repo_slug),
        shell_quote(&identity.safe_branch)
    ))
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
