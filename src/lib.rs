use std::ffi::OsString;

use clap::Parser;
use cli::{Command, PlanCommand};
use diagnostics::JsonFailure;

pub mod cli;
pub mod compat;
pub mod contracts;
pub mod diagnostics;
pub mod execution;
pub mod git;
pub mod instructions;
pub mod output;
pub mod paths;
pub mod workflow;

pub fn run() -> std::process::ExitCode {
    let args = canonicalized_args();
    let cli = cli::Cli::parse_from(args);

    match cli.command {
        Some(Command::Plan(plan_cli)) => match plan_cli.command {
            PlanCommand::Execution(plan_execution_cli) => {
                match execution::state::ExecutionRuntime::discover(
                    &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
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
        Some(Command::Workflow(workflow_cli)) => match workflow::status::WorkflowRuntime::discover(
            &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
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
                println!("{json}");
                std::process::ExitCode::from(1)
            }
            Err(serialize_error) => {
                eprintln!("Could not serialize error output: {serialize_error}");
                std::process::ExitCode::from(1)
            }
        },
    }
}
