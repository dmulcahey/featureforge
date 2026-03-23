use std::ffi::OsString;

use clap::Parser;
use cli::Command;

pub mod cli;
pub mod compat;
pub mod contracts;
pub mod diagnostics;
pub mod git;
pub mod instructions;
pub mod output;
pub mod paths;
pub mod workflow;

pub fn run() -> std::process::ExitCode {
    let args = canonicalized_args();
    let cli = cli::Cli::parse_from(args);

    match cli.command {
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
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::from(1)
            }
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

fn emit_json<T>(result: Result<T, diagnostics::DiagnosticError>) -> std::process::ExitCode
where
    T: serde::Serialize,
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
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::from(1)
        }
    }
}
