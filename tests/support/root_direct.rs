//! INTERNAL_RUNTIME_HELPER_TEST: this file intentionally exercises unavailable runtime internals.

use std::path::Path;
use std::process::{ExitStatus, Output};

use clap::Parser;
use featureforge::cli::config::ConfigCommand;
use featureforge::cli::repo_safety::RepoSafetyCommand;
use featureforge::cli::{Cli, Command as RootCommand, RepoCommand};
use featureforge::diagnostics::JsonFailure;
use featureforge::git::discover_slug_identity;
use serde::Serialize;

enum DirectRootEmission {
    Json(Result<Vec<u8>, JsonFailure>),
    Text(Result<String, JsonFailure>),
}

pub fn internal_only_try_run_root_output_direct(
    repo: Option<&Path>,
    state_dir: Option<&Path>,
    args: &[&str],
    _context: &str,
) -> Result<Option<Output>, String> {
    let argv = std::iter::once("featureforge").chain(args.iter().copied());
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(_) => return Ok(None),
    };

    let emission = match cli.command {
        Some(RootCommand::Config(config_cli)) => {
            let Some(state_dir) = state_dir else {
                return Ok(None);
            };
            match config_cli.command {
                ConfigCommand::Get(args) => DirectRootEmission::Text(
                    featureforge::config::get_for_state_dir(state_dir, &args)
                        .map_err(JsonFailure::from),
                ),
                ConfigCommand::Set(args) => DirectRootEmission::Text(
                    featureforge::config::set_for_state_dir(state_dir, &args)
                        .map_err(JsonFailure::from),
                ),
                ConfigCommand::List => DirectRootEmission::Text(
                    featureforge::config::list_for_state_dir(state_dir).map_err(JsonFailure::from),
                ),
            }
        }
        Some(RootCommand::Repo(repo_cli)) => {
            let Some(repo) = repo else {
                return Ok(None);
            };
            match repo_cli.command {
                RepoCommand::Slug(_) => {
                    let identity = discover_slug_identity(repo);
                    DirectRootEmission::Text(Ok(format!(
                        "SLUG={}\nBRANCH={}\n",
                        shell_quote(&identity.repo_slug),
                        shell_quote(&identity.safe_branch)
                    )))
                }
                RepoCommand::RuntimeRoot(_) => return Ok(None),
            }
        }
        Some(RootCommand::RepoSafety(repo_safety_cli)) => {
            let (Some(repo), Some(state_dir)) = (repo, state_dir) else {
                return Ok(None);
            };
            let runtime = featureforge::repo_safety::RepoSafetyRuntime::discover_for_state_dir(
                repo, state_dir,
            )
            .map_err(JsonFailure::from);
            match repo_safety_cli.command {
                RepoSafetyCommand::Check(args) => DirectRootEmission::Json(serialize_json(
                    runtime.and_then(|runtime| runtime.check(&args).map_err(JsonFailure::from)),
                )),
                RepoSafetyCommand::Approve(args) => DirectRootEmission::Json(serialize_json(
                    runtime.and_then(|runtime| runtime.approve(&args).map_err(JsonFailure::from)),
                )),
            }
        }
        Some(RootCommand::Doctor(_))
        | Some(RootCommand::Workflow(_))
        | Some(RootCommand::Plan(_))
        | Some(RootCommand::UpdateCheck(_))
        | None => return Ok(None),
    };

    Ok(Some(match emission {
        DirectRootEmission::Json(result) => json_output_result(result),
        DirectRootEmission::Text(result) => text_output_result(result),
    }))
}

fn serialize_json<T: Serialize>(value: Result<T, JsonFailure>) -> Result<Vec<u8>, JsonFailure> {
    value.map(|value| json_line(&value).expect("direct root output should serialize to JSON"))
}

fn json_output_result(result: Result<Vec<u8>, JsonFailure>) -> Output {
    match result {
        Ok(stdout) => output_with_code(0, stdout, Vec::new()),
        Err(failure) => output_with_code(
            1,
            Vec::new(),
            json_line(&failure).expect("direct root json failure should serialize"),
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
