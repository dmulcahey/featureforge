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

use featureforge::cli::plan_execution::{
    GateContractArgs, GateEvaluatorArgs, GateHandoffArgs, RebuildEvidenceArgs, RecommendArgs,
    RecordBranchClosureArgs, RecordContractArgs, RecordEvaluationArgs, RecordFinalReviewArgs,
    RecordHandoffArgs, RecordQaArgs, RecordReleaseReadinessArgs, RecordReviewDispatchArgs,
    StatusArgs,
};
use serde_json::Value;

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
    if let Some(output) = try_direct_featureforge_output_with_env_control(
        repo, state_dir, home_dir, env_remove, envs, args, context,
    ) {
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

pub fn run_runtime_preflight_gate_json(
    repo: &Path,
    state_dir: &Path,
    args: &StatusArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_runtime_preflight_gate_json(repo, state_dir, args)
}

pub fn run_runtime_topology_recommendation_json(
    repo: &Path,
    state_dir: &Path,
    args: &RecommendArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_runtime_topology_recommendation_json(repo, state_dir, args)
}

pub fn run_runtime_review_gate_json(
    repo: &Path,
    state_dir: &Path,
    args: &StatusArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_runtime_review_gate_json(repo, state_dir, args)
}

pub fn run_internal_gate_contract_json(
    repo: &Path,
    state_dir: &Path,
    args: &GateContractArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_gate_contract_json(repo, state_dir, args)
}

pub fn run_internal_record_contract_json(
    repo: &Path,
    state_dir: &Path,
    args: &RecordContractArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_record_contract_json(repo, state_dir, args)
}

pub fn run_internal_gate_evaluator_json(
    repo: &Path,
    state_dir: &Path,
    args: &GateEvaluatorArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_gate_evaluator_json(repo, state_dir, args)
}

pub fn run_internal_record_evaluation_json(
    repo: &Path,
    state_dir: &Path,
    args: &RecordEvaluationArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_record_evaluation_json(repo, state_dir, args)
}

pub fn run_internal_gate_handoff_json(
    repo: &Path,
    state_dir: &Path,
    args: &GateHandoffArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_gate_handoff_json(repo, state_dir, args)
}

pub fn run_internal_record_handoff_json(
    repo: &Path,
    state_dir: &Path,
    args: &RecordHandoffArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_record_handoff_json(repo, state_dir, args)
}

pub fn run_runtime_finish_gate_json(
    repo: &Path,
    state_dir: &Path,
    args: &StatusArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_runtime_finish_gate_json(repo, state_dir, args)
}

pub fn run_runtime_review_dispatch_authority_json(
    repo: &Path,
    state_dir: &Path,
    args: &RecordReviewDispatchArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_runtime_review_dispatch_authority_json(repo, state_dir, args)
}

pub fn run_internal_rebuild_evidence_json(
    repo: &Path,
    state_dir: &Path,
    args: &RebuildEvidenceArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_rebuild_evidence_json(repo, state_dir, args)
}

pub fn run_internal_record_branch_closure_json(
    repo: &Path,
    state_dir: &Path,
    args: &RecordBranchClosureArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_record_branch_closure_json(repo, state_dir, args)
}

pub fn run_internal_record_release_readiness_json(
    repo: &Path,
    state_dir: &Path,
    args: &RecordReleaseReadinessArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_record_release_readiness_json(repo, state_dir, args)
}

pub fn run_internal_record_final_review_json(
    repo: &Path,
    state_dir: &Path,
    args: &RecordFinalReviewArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_record_final_review_json(repo, state_dir, args)
}

pub fn run_internal_record_qa_json(
    repo: &Path,
    state_dir: &Path,
    args: &RecordQaArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_record_qa_json(repo, state_dir, args)
}

pub fn run_internal_explain_review_state_json(
    repo: &Path,
    state_dir: &Path,
    args: &StatusArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_explain_review_state_json(repo, state_dir, args)
}

pub fn run_internal_reconcile_review_state_json(
    repo: &Path,
    state_dir: &Path,
    args: &StatusArgs,
) -> Result<Value, String> {
    plan_execution_direct_support::run_internal_reconcile_review_state_json(repo, state_dir, args)
}

fn direct_helper_compatible_env_control(
    home_dir: Option<&Path>,
    env_remove: &[&str],
    envs: &[(&str, &str)],
) -> bool {
    home_dir.is_none()
        && env_remove
            .iter()
            .all(|key| *key == "FEATUREFORGE_SESSION_KEY")
        && envs
            .iter()
            .all(|(key, _)| *key == "FEATUREFORGE_SESSION_KEY")
}

fn try_direct_featureforge_output_with_env_control(
    repo: Option<&Path>,
    state_dir: Option<&Path>,
    home_dir: Option<&Path>,
    env_remove: &[&str],
    envs: &[(&str, &str)],
    args: &[&str],
    context: &str,
) -> Option<Output> {
    if !direct_helper_compatible_env_control(home_dir, env_remove, envs) {
        return None;
    }
    // Session-entry env selection was removed from the active runtime. Allowing this
    // specific env through the direct helper keeps semantic tests on the in-process path
    // without introducing process-global env mutation. If runtime behavior becomes env-bound
    // again, these callers must fall back to the real CLI boundary instead.
    try_direct_featureforge_output(repo, state_dir, home_dir, env_remove, envs, args, context)
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
            repo, state_dir, args, context, false,
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
