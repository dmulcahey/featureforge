use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use clap::{CommandFactory, Parser};
use cli::runtime_root::RuntimeRootFieldCli;
use cli::{Command, PlanCommand, RepoCommand};
use diagnostics::{DiagnosticError, FailureClass, JsonFailure};
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
        Some(Command::Plan(plan_cli)) => match plan_cli.command {
            PlanCommand::Contract(plan_contract_cli) => match plan_contract_cli.command {
                cli::plan_contract::PlanContractCommand::Lint(args) => {
                    contracts::runtime::run_lint(&args)
                }
                cli::plan_contract::PlanContractCommand::AnalyzePlan(args) => {
                    contracts::runtime::run_analyze_plan(&args)
                }
                cli::plan_contract::PlanContractCommand::BuildTaskPacket(args) => {
                    contracts::runtime::run_build_task_packet(&args)
                }
            },
            PlanCommand::Execution(plan_execution_cli) => {
                match execution::state::ExecutionRuntime::discover(
                    &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                ) {
                    Ok(runtime) => match plan_execution_cli.command {
                        cli::plan_execution::PlanExecutionCommand::Status(args) => {
                            emit_json(runtime.status(&args))
                        }
                        cli::plan_execution::PlanExecutionCommand::RepairReviewState(args) => {
                            emit_json(execution::review_state::repair_review_state_command(
                                &runtime, &args,
                            ))
                        }
                        cli::plan_execution::PlanExecutionCommand::CloseCurrentTask(args) => {
                            emit_json(execution::mutate::close_current_task(&runtime, &args))
                        }
                        cli::plan_execution::PlanExecutionCommand::AdvanceLateStage(args) => {
                            emit_json(execution::mutate::advance_late_stage(&runtime, &args))
                        }
                        cli::plan_execution::PlanExecutionCommand::Begin(args) => {
                            emit_json(execution::mutate::begin(&runtime, &args))
                        }
                        cli::plan_execution::PlanExecutionCommand::Complete(args) => {
                            emit_json(execution::mutate::complete(&runtime, &args))
                        }
                        cli::plan_execution::PlanExecutionCommand::Reopen(args) => {
                            emit_json(execution::mutate::reopen(&runtime, &args))
                        }
                        cli::plan_execution::PlanExecutionCommand::Transfer(args) => {
                            emit_json(execution::mutate::transfer(&runtime, &args))
                        }
                        cli::plan_execution::PlanExecutionCommand::MaterializeProjections(args) => {
                            emit_json(execution::mutate::materialize_projections(&runtime, &args))
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
                        emit_json(runtime.approve(&args))
                    }
                },
                Err(error) => emit_json::<Value, JsonFailure>(Err(error.into())),
            }
        }
        Some(Command::UpdateCheck(args)) => emit_text(update_check::check(&args)),
        Some(Command::Workflow(workflow_cli)) => {
            let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            match workflow_cli.command {
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
