#![allow(dead_code)]

#[allow(dead_code)]
#[path = "plan_execution_direct.rs"]
mod plan_execution_direct_support;
#[allow(dead_code)]
#[path = "root_direct.rs"]
mod root_direct_support;
#[allow(dead_code)]
#[path = "workflow_direct.rs"]
mod workflow_direct_support;

use std::path::Path;
use std::process::{Command, Output};

use crate::process_support::run;

pub fn run_rust_featureforge(
    repo: Option<&Path>,
    state_dir: Option<&Path>,
    home_dir: Option<&Path>,
    envs: &[(&str, &str)],
    args: &[&str],
    context: &str,
) -> Output {
    run_rust_featureforge_with_env_control(repo, state_dir, home_dir, &[], envs, args, context)
}

pub fn run_rust_featureforge_real_cli(
    repo: Option<&Path>,
    state_dir: Option<&Path>,
    home_dir: Option<&Path>,
    envs: &[(&str, &str)],
    args: &[&str],
    context: &str,
) -> Output {
    run_rust_featureforge_with_env_control_real_cli(
        repo,
        state_dir,
        home_dir,
        &[],
        envs,
        args,
        context,
    )
}

pub fn run_rust_featureforge_with_env_control(
    repo: Option<&Path>,
    state_dir: Option<&Path>,
    home_dir: Option<&Path>,
    env_remove: &[&str],
    envs: &[(&str, &str)],
    args: &[&str],
    context: &str,
) -> Output {
    if let Some(output) =
        try_direct_featureforge_output(repo, state_dir, home_dir, env_remove, envs, args, context)
    {
        return output;
    }

    let mut command = Command::new(env!("CARGO_BIN_EXE_featureforge"));
    if let Some(repo) = repo {
        command.current_dir(repo);
    }
    if let Some(state_dir) = state_dir {
        command.env("FEATUREFORGE_STATE_DIR", state_dir);
    }
    if let Some(home_dir) = home_dir {
        command.env("HOME", home_dir);
    }
    for key in env_remove {
        command.env_remove(key);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    command.args(args);
    run(command, context)
}

pub fn run_rust_featureforge_with_env_control_real_cli(
    repo: Option<&Path>,
    state_dir: Option<&Path>,
    home_dir: Option<&Path>,
    env_remove: &[&str],
    envs: &[(&str, &str)],
    args: &[&str],
    context: &str,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_featureforge"));
    if let Some(repo) = repo {
        command.current_dir(repo);
    }
    if let Some(state_dir) = state_dir {
        command.env("FEATUREFORGE_STATE_DIR", state_dir);
    }
    if let Some(home_dir) = home_dir {
        command.env("HOME", home_dir);
    }
    for key in env_remove {
        command.env_remove(key);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    command.args(args);
    run(command, context)
}

fn try_direct_featureforge_output(
    repo: Option<&Path>,
    state_dir: Option<&Path>,
    home_dir: Option<&Path>,
    env_remove: &[&str],
    envs: &[(&str, &str)],
    args: &[&str],
    context: &str,
) -> Option<Output> {
    if home_dir.is_some() || !env_remove.is_empty() || !envs.is_empty() {
        return None;
    }

    match root_direct_support::try_run_root_output_direct(repo, state_dir, args, context) {
        Ok(Some(output)) => return Some(output),
        Ok(None) => {}
        Err(error) => panic!("{error}"),
    }

    let (Some(repo), Some(state_dir)) = (repo, state_dir) else {
        return None;
    };

    // Boundary tests that depend on process env rewriting, stdout/stderr framing, or
    // root-command shell behavior must keep using the real binary. Everything else
    // should converge on the same in-process runtime path so semantic surfaces don't drift.
    if args.first().copied() == Some("workflow") {
        return match workflow_direct_support::try_run_workflow_output_direct(
            repo, state_dir, args, context,
        ) {
            Ok(Some(output)) => Some(output),
            Ok(None) => None,
            Err(error) => panic!("{error}"),
        };
    }

    if args.starts_with(&["plan", "execution"]) {
        return match plan_execution_direct_support::try_run_plan_execution_output_direct(
            repo,
            state_dir,
            &args[2..],
            context,
        ) {
            Ok(Some(output)) => Some(output),
            Ok(None) => None,
            Err(error) => panic!("{error}"),
        };
    }

    None
}
