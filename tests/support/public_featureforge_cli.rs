#![allow(dead_code)]

use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

use crate::process_support::run;

pub fn run_featureforge_real_cli(
    repo: Option<&Path>,
    state_dir: Option<&Path>,
    home_dir: Option<&Path>,
    envs: &[(&str, &str)],
    args: &[&str],
    context: &str,
) -> Output {
    run_featureforge_with_env_control_real_cli(repo, state_dir, home_dir, &[], envs, args, context)
}

pub fn run_public_featureforge_cli_json(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let output = run_featureforge_with_env_control_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        &[],
        args,
        context,
    );
    assert!(
        output.status.success(),
        "public featureforge CLI command should succeed for {context}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "public featureforge CLI command should emit JSON for {context}: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

pub fn run_public_featureforge_cli_failure_json(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let output = run_featureforge_with_env_control_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        &[],
        args,
        context,
    );
    assert!(
        !output.status.success(),
        "public featureforge CLI command should fail for {context}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stderr).unwrap_or_else(|error| {
        panic!(
            "public featureforge CLI command should emit JSON failure for {context}: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

pub fn run_featureforge_with_env_control_real_cli(
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
