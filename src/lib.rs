use std::ffi::OsString;
use std::path::PathBuf;

use clap::Parser;
use cli::{Command, InstallCommand, PlanCommand, RepoCommand};
use diagnostics::{DiagnosticError, JsonFailure};

pub mod cli;
pub mod compat;
pub mod config;
pub mod contracts;
pub mod diagnostics;
pub mod execution;
pub mod git;
pub mod install;
pub mod instructions;
pub mod output;
pub mod paths;
pub mod repo_safety;
pub mod session_entry;
pub mod update_check;
pub mod workflow;

pub fn run() -> std::process::ExitCode {
    let args = canonicalized_args();
    let cli = cli::Cli::parse_from(args);

    match cli.command {
        Some(Command::Config(config_cli)) => match config_cli.command {
            cli::config::ConfigCommand::Get(args) => emit_text(config::get(&args)),
            cli::config::ConfigCommand::Set(args) => emit_text(config::set(&args)),
            cli::config::ConfigCommand::List => emit_text(config::list()),
        },
        Some(Command::Install(install_cli)) => match install_cli.command {
            InstallCommand::Migrate(args) => emit_text(install::migrate(&args)),
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
                        cli::plan_execution::PlanExecutionCommand::Recommend(args) => {
                            emit_json(runtime.recommend(&args))
                        }
                        cli::plan_execution::PlanExecutionCommand::Preflight(args) => {
                            emit_json(runtime.preflight(&args))
                        }
                        cli::plan_execution::PlanExecutionCommand::GateReview(args) => {
                            emit_json(runtime.gate_review(&args))
                        }
                        cli::plan_execution::PlanExecutionCommand::GateFinish(args) => {
                            emit_json(runtime.gate_finish(&args))
                        }
                        cli::plan_execution::PlanExecutionCommand::Begin(args) => {
                            emit_json(execution::mutate::begin(&runtime, &args))
                        }
                        cli::plan_execution::PlanExecutionCommand::Complete(args) => {
                            emit_json(execution::mutate::complete(&runtime, &args))
                        }
                    },
                    Err(error) => emit_json::<serde_json::Value, _>(Err(error)),
                }
            }
        },
        Some(Command::Repo(repo_cli)) => match repo_cli.command {
            RepoCommand::Slug(_) => emit_text(render_slug_output(
                &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            )),
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
                Err(error) => emit_json::<serde_json::Value, JsonFailure>(Err(error.into())),
            }
        }
        Some(Command::SessionEntry(session_entry_cli)) => match session_entry_cli.command {
            cli::session_entry::SessionEntryCommand::Resolve(args) => {
                emit_json(session_entry::resolve(&args))
            }
            cli::session_entry::SessionEntryCommand::Record(args) => {
                emit_json(session_entry::record(&args))
            }
        },
        Some(Command::UpdateCheck(args)) => emit_text(update_check::check(&args)),
        Some(Command::Workflow(workflow_cli)) => match workflow::status::WorkflowRuntime::discover(
            &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        ) {
            Ok(mut runtime) => match workflow_cli.command {
                cli::workflow::WorkflowCommand::Status(_) => emit_json(runtime.status()),
                cli::workflow::WorkflowCommand::Resolve => emit_json(runtime.resolve()),
                cli::workflow::WorkflowCommand::Expect(args) => {
                    emit_json(runtime.expect(args.artifact, &args.path))
                }
                cli::workflow::WorkflowCommand::Sync(args) => {
                    emit_json(runtime.sync(args.artifact, args.path.as_deref()))
                }
                cli::workflow::WorkflowCommand::Phase(_) => emit_json(runtime.phase()),
            },
            Err(error) => emit_json::<serde_json::Value, JsonFailure>(Err(error.into())),
        },
        None => std::process::ExitCode::SUCCESS,
    }
}

fn canonicalized_args() -> Vec<OsString> {
    let mut args = std::env::args_os();
    let argv0 = args.next().unwrap_or_else(|| OsString::from("superpowers"));
    let mut canonicalized = vec![argv0.clone()];
    canonicalized.extend(
        compat::argv0::canonical_command_from_argv0(&argv0.to_string_lossy())
            .iter()
            .map(OsString::from),
    );
    canonicalized.extend(args);
    canonicalized
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

fn emit_text(result: Result<String, DiagnosticError>) -> std::process::ExitCode {
    match result {
        Ok(text) => {
            if !text.is_empty() {
                print!("{text}");
            }
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{}: {}", error.failure_class(), error.message());
            std::process::ExitCode::from(1)
        }
    }
}

fn render_slug_output(current_dir: &std::path::Path) -> Result<String, DiagnosticError> {
    let identity = git::discover_slug_identity(current_dir);
    Ok(format!(
        "SLUG={}\nBRANCH={}\n",
        shell_quote(&identity.repo_slug),
        shell_quote(&identity.safe_branch)
    ))
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
