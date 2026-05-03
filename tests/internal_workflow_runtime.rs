// Internal compatibility tests extracted from tests/workflow_runtime.rs.
// This file intentionally reuses the source fixture scaffolding from the public-facing integration test.

#[path = "support/bin.rs"]
mod bin_support;
#[path = "support/dir_tree.rs"]
mod dir_tree_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/internal_only_direct_helpers.rs"]
mod internal_only_direct_helpers;
#[path = "support/json.rs"]
mod json_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/projection.rs"]
mod projection_support;
#[path = "support/runtime_json.rs"]
mod runtime_json_support;
#[path = "support/runtime_phase_handoff.rs"]
mod runtime_phase_handoff_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use bin_support::compiled_featureforge_path;
use dir_tree_support::copy_dir_recursive;
use featureforge::cli::plan_execution::StatusArgs as PlanExecutionStatusArgs;
use featureforge::contracts::plan::{PLAN_FIDELITY_REQUIRED_SURFACES, parse_plan_file};
use featureforge::contracts::spec::parse_spec_file;
use featureforge::execution::command_eligibility::{
    PublicCommand, PublicMutationKind, PublicMutationRequest, decide_public_mutation,
};
use featureforge::execution::final_review::resolve_release_base_branch;
use featureforge::execution::follow_up::execution_step_repair_target_id;
use featureforge::execution::internal_args::{RecordReviewDispatchArgs, ReviewDispatchScopeArg};
use featureforge::execution::invariants::{
    InvariantEnforcementMode, apply_read_surface_invariants, check_runtime_status_invariants,
};
use featureforge::execution::query::{
    apply_read_surface_invariants_to_routing, query_workflow_routing_state_for_runtime,
};
use featureforge::execution::semantic_identity::{
    branch_definition_identity_for_context, task_definition_identity_for_task,
};
use featureforge::execution::state::{
    current_head_sha as runtime_current_head_sha, derive_evidence_rel_path,
    load_execution_context_for_mutation,
};
use featureforge::git::{discover_repository, discover_slug_identity};
use featureforge::paths::{
    branch_storage_key, harness_authoritative_artifact_path, harness_state_path,
};
use featureforge::workflow::operator;
use files_support::write_file;
use internal_only_direct_helpers::internal_runtime_direct as plan_execution_direct_support;
use json_support::parse_json;
use process_support::{run, run_checked};
use runtime_json_support::{discover_execution_runtime, plan_execution_status_json};
use runtime_phase_handoff_support::{workflow_handoff_json, workflow_phase_json};
use serde::Serialize;
use serde_json::{Value, json, to_value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use tempfile::TempDir;
use workflow_support::{
    copy_workflow_fixture, init_repo as init_workflow_repo, workflow_fixture_root,
};

const FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FULL_CONTRACT_READY_SPEC_REL: &str =
    "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
const FULL_CONTRACT_READY_PLAN_REL: &str =
    "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
const FULL_CONTRACT_READY_SPEC_FIXTURE_REL: &str =
    "specs/2026-03-22-runtime-integration-hardening-design.md";
const FULL_CONTRACT_READY_PLAN_FIXTURE_REL: &str =
    "plans/2026-03-22-runtime-integration-hardening.md";

fn assert_task_closure_required_inputs(surface: &Value, task: u32) {
    assert!(
        surface.get("recommended_public_command_argv").is_none(),
        "task-closure routes require external review/verification inputs and must not emit executable argv: {surface}"
    );
    assert!(
        surface["recommended_command"].is_null(),
        "task-closure routes should expose typed inputs instead of a placeholder command: {surface}"
    );
    let task_target = surface["recording_context"]["task_number"]
        .as_u64()
        .or_else(|| surface["route"]["recording_context"]["task_number"].as_u64())
        .or_else(|| surface["execution_status"]["recording_context"]["task_number"].as_u64())
        .or_else(|| surface["blocking_task"].as_u64())
        .or_else(|| surface["execution_status"]["blocking_task"].as_u64())
        .or_else(|| surface["route"]["blocking_task"].as_u64());
    assert_eq!(
        task_target,
        Some(u64::from(task)),
        "task-closure route should keep the task in structured route metadata: {surface}"
    );
    let required_inputs = surface
        .get("required_inputs")
        .or_else(|| surface["execution_status"].get("required_inputs"))
        .or_else(|| surface["route"].get("required_inputs"))
        .unwrap_or(&Value::Null);
    assert_eq!(
        required_inputs,
        &json!([
            {
                "kind": "enum",
                "name": "review_result",
                "values": ["pass", "fail"]
            },
            {
                "kind": "path",
                "must_exist": true,
                "name": "review_summary_file"
            },
            {
                "kind": "enum",
                "name": "verification_result",
                "values": ["pass", "fail", "not-run"]
            },
            {
                "kind": "path",
                "must_exist": true,
                "name": "verification_summary_file",
                "required_when": "verification_result!=not-run"
            }
        ]),
        "task-closure routes should expose typed missing inputs: {surface}"
    );
}

fn record_task_closure_with_fixture_inputs(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    task: u32,
    label: &str,
) -> Value {
    let safe_label: String = label
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let review_summary_path = repo.join(format!("task-{task}-{safe_label}-review-summary.md"));
    let verification_summary_path =
        repo.join(format!("task-{task}-{safe_label}-verification-summary.md"));
    write_file(
        &review_summary_path,
        &format!("Task {task} fixture independent review passed for {label}.\n"),
    );
    write_file(
        &verification_summary_path,
        &format!("Task {task} fixture verification passed for {label}.\n"),
    );
    let task_arg = task.to_string();
    run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            task_arg.as_str(),
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        label,
    )
}

fn workflow_doctor_json(
    runtime: &featureforge::execution::state::ExecutionRuntime,
    context: &str,
) -> Value {
    let value = operator::doctor_for_runtime(runtime)
        .unwrap_or_else(|error| panic!("{context}: workflow doctor should succeed: {error:?}"));
    to_value(value)
        .unwrap_or_else(|error| panic!("{context}: workflow doctor should serialize: {error}"))
}
const FULL_CONTRACT_READY_FIXTURE_SPEC_PATH: &str = "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md";

struct FullContractReadyFixtureTemplate {
    plan: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkflowRuntimeFixtureQaMode {
    Required,
    NotRequired,
}

#[derive(Debug, Clone)]
struct WorkflowRuntimeExecutionFixtureTemplate {
    repo_root: PathBuf,
    state_root: PathBuf,
}

static FULL_CONTRACT_READY_FIXTURE_TEMPLATE: OnceLock<FullContractReadyFixtureTemplate> =
    OnceLock::new();
static WORKFLOW_RUNTIME_EXECUTION_TEMPLATE_REQUIRED: OnceLock<
    WorkflowRuntimeExecutionFixtureTemplate,
> = OnceLock::new();
static WORKFLOW_RUNTIME_EXECUTION_TEMPLATE_NOT_REQUIRED: OnceLock<
    WorkflowRuntimeExecutionFixtureTemplate,
> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicRouteSnapshot {
    phase: String,
    phase_detail: Option<String>,
    review_state_status: String,
    next_action: String,
    recommended_command: Option<String>,
    blocking_scope: Option<String>,
    blocking_task: Option<u32>,
    external_wait_state: Option<String>,
    blocking_reason_codes: Vec<String>,
}

fn public_route_snapshot(value: &Value) -> PublicRouteSnapshot {
    let phase = value
        .get("phase")
        .or_else(|| value.get("harness_phase"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!("route payload must include string `phase` or `harness_phase`: {value}")
        })
        .to_owned();
    let review_state_status = value
        .get("review_state_status")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!("route payload must include string `review_state_status`: {value}")
        })
        .to_owned();
    let next_action = value
        .get("next_action")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("route payload must include string `next_action`: {value}"))
        .to_owned();

    PublicRouteSnapshot {
        phase,
        phase_detail: value
            .get("phase_detail")
            .and_then(Value::as_str)
            .map(str::to_owned),
        review_state_status,
        next_action,
        recommended_command: value
            .get("recommended_command")
            .and_then(Value::as_str)
            .map(str::to_owned),
        blocking_scope: value
            .get("blocking_scope")
            .and_then(Value::as_str)
            .map(str::to_owned),
        blocking_task: value
            .get("blocking_task")
            .and_then(Value::as_u64)
            .and_then(|raw| u32::try_from(raw).ok()),
        external_wait_state: value
            .get("external_wait_state")
            .and_then(Value::as_str)
            .map(str::to_owned),
        blocking_reason_codes: value
            .get("blocking_reason_codes")
            .and_then(Value::as_array)
            .map(|codes| {
                codes
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    }
}

fn assert_public_route_parity(operator: &Value, status: &Value, doctor: Option<&Value>) {
    let operator_route = public_route_snapshot(operator);
    let status_route = public_route_snapshot(status);
    assert_eq!(
        operator_route, status_route,
        "workflow operator and plan execution status must agree on public route fields"
    );
    if let Some(doctor) = doctor {
        let doctor_route = public_route_snapshot(doctor);
        assert_eq!(
            operator_route, doctor_route,
            "workflow doctor top-level route must match workflow operator"
        );
    }
}

fn assert_follow_up_blocker_parity_with_operator(
    operator: &Value,
    follow_up: &Value,
    context: &str,
) {
    if follow_up["action"].as_str() != Some("blocked") {
        return;
    }
    let follow_up_blocking_reason_codes = follow_up
        .get("blocking_reason_codes")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!(
                "{context} blocked follow-up must include blocking_reason_codes metadata: {follow_up:?}"
            )
        });
    let operator_blocking_reason_codes = operator
        .get("blocking_reason_codes")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!(
                "{context} operator route must include blocking_reason_codes metadata for blocked parity checks: {operator:?}"
            )
        });
    assert!(
        !follow_up_blocking_reason_codes.is_empty(),
        "{context} blocked follow-up must keep a non-empty blocker reason-code set",
    );
    assert!(
        !operator_blocking_reason_codes.is_empty(),
        "{context} operator route must keep a non-empty blocker reason-code set for blocked parity checks",
    );
    assert_eq!(
        follow_up["blocking_scope"], operator["blocking_scope"],
        "{context} blocked follow-up must preserve operator blocking scope"
    );
    assert_eq!(
        follow_up["blocking_task"], operator["blocking_task"],
        "{context} blocked follow-up must preserve operator blocking task"
    );
    assert_eq!(
        follow_up["blocking_reason_codes"], operator["blocking_reason_codes"],
        "{context} blocked follow-up must preserve operator blocker reason-code set"
    );
    if !follow_up["blocking_step"].is_null() || !operator["blocking_step"].is_null() {
        assert_eq!(
            follow_up["blocking_step"], operator["blocking_step"],
            "{context} blocked follow-up must preserve operator blocking step"
        );
    }
    if !follow_up["authoritative_next_action"].is_null() {
        assert_eq!(
            follow_up["authoritative_next_action"], operator["recommended_command"],
            "{context} blocked follow-up authoritative next action must mirror workflow operator"
        );
    }
}

fn run_recommended_plan_execution_command_with_mode(
    repo: &Path,
    state: &Path,
    recommended_command: &str,
    real_cli: bool,
    context: &str,
) -> Value {
    let command_parts = recommended_command.split_whitespace().collect::<Vec<_>>();
    assert!(
        command_parts.len() >= 3,
        "{context} should expose a full featureforge command, got {recommended_command}"
    );
    assert_eq!(
        command_parts[0], "featureforge",
        "{context} recommended command must start with featureforge, got {recommended_command}"
    );
    if command_parts[1] == "plan" && command_parts[2] == "execution" {
        assert!(
            command_parts.len() >= 4,
            "{context} plan-execution command should include a subcommand, got {recommended_command}"
        );
        let command_args = if command_parts
            .get(3)
            .is_some_and(|command| *command == "close-current-task")
            && (recommended_command.contains("pass|fail") || recommended_command.contains("<path>"))
        {
            let plan = command_parts
                .windows(2)
                .find(|window| window[0] == "--plan")
                .map(|window| window[1])
                .expect("close-current-task template command should include --plan");
            let task = command_parts
                .windows(2)
                .find(|window| window[0] == "--task")
                .map(|window| window[1])
                .unwrap_or("1");
            let dispatch_id = command_parts
                .windows(2)
                .find(|window| command_arg_matches_parts(window[0], &["--dispatch", "-id"]))
                .map(|window| window[1]);
            assert!(
                dispatch_id.is_none(),
                "{context} public recommended command must not include hidden dispatch lineage flags"
            );

            let review_summary_path = repo.join(
                "docs/featureforge/execution-evidence/runtime-remediation-close-current-task-review-summary.md",
            );
            let verification_summary_path = repo.join(
                "docs/featureforge/execution-evidence/runtime-remediation-close-current-task-verification-summary.md",
            );
            write_file(
                &review_summary_path,
                &format!(
                    "Close-current-task command generated from shared template for {context}.\n"
                ),
            );
            write_file(
                &verification_summary_path,
                &format!("Verification summary generated from shared template for {context}.\n"),
            );

            let mut args = vec![
                String::from("close-current-task"),
                String::from("--plan"),
                plan.to_owned(),
                String::from("--task"),
                task.to_owned(),
            ];
            args.extend([
                String::from("--review-result"),
                String::from("pass"),
                String::from("--review-summary-file"),
                review_summary_path
                    .to_str()
                    .expect("review summary path should stay utf-8")
                    .to_owned(),
                String::from("--verification-result"),
                String::from("pass"),
                String::from("--verification-summary-file"),
                verification_summary_path
                    .to_str()
                    .expect("verification summary path should stay utf-8")
                    .to_owned(),
            ]);
            args
        } else {
            command_parts[3..]
                .iter()
                .map(|part| (*part).to_owned())
                .collect::<Vec<_>>()
        };

        let command_args_refs = command_args.iter().map(String::as_str).collect::<Vec<_>>();
        if real_cli {
            return run_plan_execution_json_real_cli(repo, state, &command_args_refs, context);
        }
        return run_plan_execution_json(repo, state, &command_args_refs, context);
    }

    if command_parts[1] == "workflow" {
        let command_args = command_parts[1..]
            .iter()
            .map(|part| (*part).to_owned())
            .collect::<Vec<_>>();
        let command_args_refs = command_args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = if real_cli {
            run_rust_featureforge_with_env_real_cli(repo, state, &command_args_refs, &[], context)
        } else {
            run_rust_featureforge_with_env(repo, state, &command_args_refs, &[], context)
        };
        return parse_json(&output, context);
    }

    panic!(
        "{context} recommended command must route through `plan execution` or `workflow`, got {recommended_command}"
    );
}

fn command_arg_matches_parts(arg: &str, parts: &[&str]) -> bool {
    let Some((first, rest)) = parts.split_first() else {
        return arg.is_empty();
    };
    let Some(mut remaining) = arg.strip_prefix(first) else {
        return false;
    };
    for part in rest {
        let Some(next_remaining) = remaining.strip_prefix(part) else {
            return false;
        };
        remaining = next_remaining;
    }
    remaining.is_empty()
}

fn run_recommended_plan_execution_command(
    repo: &Path,
    state: &Path,
    recommended_command: &str,
    context: &str,
) -> Value {
    run_recommended_plan_execution_command_with_mode(
        repo,
        state,
        recommended_command,
        false,
        context,
    )
}

struct PlanFidelityReviewArtifactInput<'a> {
    artifact_rel: &'a str,
    plan_path: &'a str,
    plan_revision: u32,
    spec_path: &'a str,
    spec_revision: u32,
    review_verdict: &'a str,
    reviewer_source: &'a str,
    reviewer_id: &'a str,
    verified_surfaces: &'a [&'a str],
}

fn write_plan_fidelity_review_artifact(repo: &Path, input: PlanFidelityReviewArtifactInput<'_>) {
    let artifact_path = repo.join(input.artifact_rel);
    let plan_fingerprint =
        sha256_hex(&fs::read(repo.join(input.plan_path)).expect("plan should be readable"));
    let spec_fingerprint =
        sha256_hex(&fs::read(repo.join(input.spec_path)).expect("spec should be readable"));
    let verified_requirement_ids = parse_spec_file(repo.join(input.spec_path))
        .map(|spec| {
            spec.requirements
                .iter()
                .map(|requirement| requirement.id.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent).expect("review artifact parent should exist");
    }
    fs::write(
        artifact_path,
        format!(
            "## Plan Fidelity Review Summary\n\n**Review Stage:** featureforge:plan-fidelity-review\n**Review Verdict:** {review_verdict}\n**Reviewed Plan:** `{plan_path}`\n**Reviewed Plan Revision:** {plan_revision}\n**Reviewed Plan Fingerprint:** {plan_fingerprint}\n**Reviewed Spec:** `{spec_path}`\n**Reviewed Spec Revision:** {spec_revision}\n**Reviewed Spec Fingerprint:** {spec_fingerprint}\n**Reviewer Source:** {reviewer_source}\n**Reviewer ID:** {reviewer_id}\n**Distinct From Stages:** featureforge:writing-plans, featureforge:plan-eng-review\n**Verified Surfaces:** {}\n**Verified Requirement IDs:** {}\n",
            input.verified_surfaces.join(", "),
            verified_requirement_ids.join(", "),
            review_verdict = input.review_verdict,
            plan_path = input.plan_path,
            plan_revision = input.plan_revision,
            spec_path = input.spec_path,
            spec_revision = input.spec_revision,
            reviewer_source = input.reviewer_source,
            reviewer_id = input.reviewer_id,
        ),
    )
    .expect("plan-fidelity review artifact should write");
}

fn write_current_pass_plan_fidelity_review_artifact_for_plan(repo: &Path, plan_path: &str) {
    let plan = parse_plan_file(repo.join(plan_path)).expect("plan fixture should parse");
    let spec_path = plan.source_spec_path.clone();
    let plan_stem = Path::new(plan_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("plan");
    let artifact_rel = format!(".featureforge/reviews/{plan_stem}-plan-fidelity.md");
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: &artifact_rel,
            plan_path,
            plan_revision: plan.plan_revision,
            spec_path: &spec_path,
            spec_revision: plan.source_spec_revision,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "fixture-plan-fidelity-reviewer",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
}

fn public_harness_phases_from_spec() -> Vec<String> {
    fn parse_phases(
        spec: &str,
        start_line: &str,
        end_predicate: impl Fn(&str) -> bool,
    ) -> BTreeSet<String> {
        spec.lines()
            .scan((false, false), |state, line| {
                let (in_phase_section, saw_entry) = state;
                let trimmed = line.trim();
                if trimmed == start_line {
                    *in_phase_section = true;
                    *saw_entry = false;
                    return Some(None);
                }
                if *in_phase_section && *saw_entry && end_predicate(trimmed) {
                    *in_phase_section = false;
                    return Some(None);
                }
                if *in_phase_section {
                    let parsed = trimmed
                        .strip_prefix("- `")
                        .and_then(|value| value.strip_suffix('`'))
                        .map(str::to_owned);
                    if parsed.is_some() {
                        *saw_entry = true;
                    }
                    return Some(parsed);
                }
                Some(None)
            })
            .flatten()
            .collect()
    }

    let mut phases = parse_phases(
        include_str!(
            "../docs/archive/featureforge/specs/2026-03-25-featureforge-execution-harness-spec.md"
        ),
        "### Public phase model",
        |trimmed| trimmed.starts_with("### "),
    );
    phases.extend(parse_phases(
        include_str!(
            "../docs/archive/featureforge/specs/2026-04-01-workflow-public-phase-contract.md"
        ),
        "The public `phase` should cover these operator moments:",
        |trimmed| trimmed.is_empty() || trimmed.starts_with("The public API shape"),
    ));
    phases.into_iter().collect()
}

fn public_harness_phase_from_spec(phase: &str) -> String {
    public_harness_phases_from_spec()
        .into_iter()
        .find(|candidate| candidate == phase)
        .unwrap_or_else(|| {
            panic!("spec should include `{phase}` in the public harness phase model section")
        })
}

fn init_repo(test_name: &str) -> (TempDir, TempDir) {
    let (repo_dir, state_dir) = init_workflow_repo(test_name);
    let repo_path = repo_dir.path();

    let mut git_remote_add = Command::new("git");
    git_remote_add
        .args([
            "remote",
            "add",
            "origin",
            &format!("git@github.com:example/{test_name}.git"),
        ])
        .current_dir(repo_path);
    run_checked(git_remote_add, "git remote add origin");

    (repo_dir, state_dir)
}

fn clear_directory(path: &Path) {
    if !path.exists() {
        fs::create_dir_all(path).expect("destination directory should be creatable");
        return;
    }
    for entry in fs::read_dir(path).expect("destination directory should be readable") {
        let entry = entry.expect("destination entry should be readable");
        let entry_path = entry.path();
        if entry
            .file_type()
            .expect("destination entry type should be readable")
            .is_dir()
        {
            fs::remove_dir_all(&entry_path)
                .unwrap_or_else(|error| panic!("failed to remove {:?}: {error}", entry_path));
        } else {
            fs::remove_file(&entry_path)
                .unwrap_or_else(|error| panic!("failed to remove {:?}: {error}", entry_path));
        }
    }
}

fn populate_execution_fixture_from_template(
    template: &WorkflowRuntimeExecutionFixtureTemplate,
    repo: &Path,
    state_dir: &Path,
) {
    clear_directory(repo);
    clear_directory(state_dir);
    copy_dir_recursive(&template.repo_root, repo);
    copy_dir_recursive(&template.state_root, state_dir);
}

fn workflow_runtime_execution_fixture_template(
    mode: WorkflowRuntimeFixtureQaMode,
) -> &'static WorkflowRuntimeExecutionFixtureTemplate {
    let store = match mode {
        WorkflowRuntimeFixtureQaMode::Required => &WORKFLOW_RUNTIME_EXECUTION_TEMPLATE_REQUIRED,
        WorkflowRuntimeFixtureQaMode::NotRequired => {
            &WORKFLOW_RUNTIME_EXECUTION_TEMPLATE_NOT_REQUIRED
        }
    };
    store.get_or_init(|| {
        let (repo_dir, state_dir) = init_repo(match mode {
            WorkflowRuntimeFixtureQaMode::Required => "workflow-runtime-template-required",
            WorkflowRuntimeFixtureQaMode::NotRequired => "workflow-runtime-template-not-required",
        });
        let repo = repo_dir.path();
        let state = state_dir.path();
        complete_workflow_fixture_execution_with_qa_requirement_slow(
            repo,
            state,
            FULL_CONTRACT_READY_PLAN_REL,
            match mode {
                WorkflowRuntimeFixtureQaMode::Required => "required",
                WorkflowRuntimeFixtureQaMode::NotRequired => "not-required",
            },
        );
        let template = WorkflowRuntimeExecutionFixtureTemplate {
            repo_root: repo.to_path_buf(),
            state_root: state.to_path_buf(),
        };
        std::mem::forget(repo_dir);
        std::mem::forget(state_dir);
        template
    })
}

fn inject_current_topology_sections(plan_source: &str) -> String {
    const INSERT_AFTER: &str = "## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n- REQ-004 -> Task 1\n- VERIFY-001 -> Task 1\n";
    const TOPOLOGY_BLOCK: &str = "\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n";

    if plan_source.contains("## Execution Strategy")
        && plan_source.contains("## Dependency Diagram")
    {
        return plan_source.to_owned();
    }

    plan_source.replacen(INSERT_AFTER, &format!("{INSERT_AFTER}{TOPOLOGY_BLOCK}"), 1)
}

fn full_contract_ready_fixture_template() -> &'static FullContractReadyFixtureTemplate {
    FULL_CONTRACT_READY_FIXTURE_TEMPLATE.get_or_init(|| {
        let fixture_root = workflow_fixture_root();
        let plan_source =
            fs::read_to_string(fixture_root.join(FULL_CONTRACT_READY_PLAN_FIXTURE_REL))
                .expect("plan fixture should load");
        let plan = inject_current_topology_sections(&plan_source).replace(
            FULL_CONTRACT_READY_FIXTURE_SPEC_PATH,
            FULL_CONTRACT_READY_SPEC_REL,
        );

        FullContractReadyFixtureTemplate { plan }
    })
}

fn install_full_contract_ready_artifacts(repo: &Path) {
    let spec_rel = FULL_CONTRACT_READY_SPEC_REL;
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;
    let template = full_contract_ready_fixture_template();
    let spec_path = repo.join(spec_rel);
    let plan_path = repo.join(plan_rel);

    copy_workflow_fixture(FULL_CONTRACT_READY_SPEC_FIXTURE_REL, &spec_path);

    if let Some(parent) = plan_path.parent() {
        fs::create_dir_all(parent).expect("plan fixture parent should be creatable");
    }
    fs::write(&plan_path, &template.plan).expect("plan fixture should write");
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
}

#[test]
fn internal_only_compatibility_read_surface_invariant_blocks_current_stale_overlap_without_dropping_evidence()
 {
    let (repo_dir, state_dir) = init_repo("read-invariant-current-stale-overlap");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;
    complete_workflow_fixture_execution(repo, state, plan_rel);
    let runtime = discover_execution_runtime(repo, state, "current/stale overlap invariant");
    let mut status = runtime
        .status(&PlanExecutionStatusArgs {
            plan: plan_rel.into(),
            external_review_result_ready: false,
        })
        .expect("fixture status should load");
    let current = status
        .current_task_closures
        .first()
        .expect("fixture should expose a current task closure")
        .clone();

    status.review_state_status = String::from("stale_unreviewed");
    status.phase_detail = String::from("execution_reentry_required");
    status
        .stale_unreviewed_closures
        .push(current.closure_record_id.clone());
    status.recommended_command = Some(format!(
        "featureforge plan execution reopen --plan {plan_rel} --task {} --step 1 --source featureforge:executing-plans --reason overlap",
        current.task
    ));

    apply_read_surface_invariants(&mut status);
    let status_json =
        to_value(&status).expect("invariant-adjusted status should serialize for assertions");
    assert_eq!(status_json["state_kind"], "blocked_runtime_bug");
    assert_eq!(status_json["phase_detail"], "blocked_runtime_bug");
    assert!(status_json["recommended_command"].is_null());
    assert!(status_json["execution_command_context"].is_null());
    assert_eq!(
        status_json["current_task_closures"][0]["closure_record_id"],
        current.closure_record_id
    );
    assert!(
        status_json["stale_unreviewed_closures"]
            .as_array()
            .is_some_and(|closures| closures
                .iter()
                .any(|closure| closure == &Value::from(current.closure_record_id.clone()))),
        "read invariant must preserve contradictory closure ids for diagnosis: {status_json}"
    );
    assert!(
        !status_json
            .get("recommended_command")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("reopen"),
        "read invariant must not emit reopen for contradictory current/stale status: {status_json}"
    );
    assert!(
        status_json["reason_codes"].as_array().is_some_and(|codes| {
            codes
                .iter()
                .any(|code| code == "current_stale_closure_overlap")
        }),
        "read invariant should attach the shared violation code: {status_json}"
    );
}

#[test]
fn internal_only_compatibility_public_mutation_oracle_rejects_invariant_blocked_status_even_with_exact_route()
 {
    let (repo_dir, state_dir) = init_repo("mutation-oracle-blocks-invariant-status");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;
    install_full_contract_ready_artifacts(repo);
    let runtime = discover_execution_runtime(repo, state, "mutation invariant blocked oracle");
    let mut status = runtime
        .status(&PlanExecutionStatusArgs {
            plan: plan_rel.into(),
            external_review_result_ready: false,
        })
        .expect("fixture status should load");
    status.state_kind = String::from("blocked_runtime_bug");
    status.phase_detail = String::from("blocked_runtime_bug");

    let decision = decide_public_mutation(
        &status,
        &PublicMutationRequest {
            kind: PublicMutationKind::Begin,
            task: status
                .execution_command_context
                .as_ref()
                .and_then(|context| context.task_number)
                .or(Some(1)),
            step: status
                .execution_command_context
                .as_ref()
                .and_then(|context| context.step_id)
                .or(Some(1)),
            expect_execution_fingerprint: None,
            transfer_mode: None,
            transfer_scope: None,
            command_name: "begin",
        },
    );

    assert!(
        !decision.allowed,
        "blocked_runtime_bug status must be diagnostic-only even when the route shape otherwise matches"
    );
    assert_eq!(decision.reason_code, "mutation_blocked_runtime_bug");
}

#[test]
fn internal_only_compatibility_replay_fixture_current_stale_closure_overlap_blocks_without_reopen()
{
    let (repo_dir, state_dir) = init_repo("replay-current-stale-closure-overlap");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-09-task9-current-stale-closure-overlap.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);
    let branch = current_branch_name(repo);
    let state_path = harness_state_path(state, &repo_slug(repo), &branch);
    let payload: Value = serde_json::from_str(
        &fs::read_to_string(&state_path).expect("authoritative replay state should be readable"),
    )
    .expect("authoritative replay state should be valid json");
    let current_record = payload["current_task_closure_records"]["task-1"].clone();
    let closure_id = current_record["closure_record_id"]
        .as_str()
        .expect("fixture should expose current task 1 closure id")
        .to_owned();
    let reviewed_state_id = current_record["reviewed_state_id"].clone();
    let mut stale_record = current_record;
    stale_record["reviewed_state_id"] = reviewed_state_id;
    stale_record["closure_status"] = Value::from("stale_unreviewed");
    stale_record["record_status"] = Value::from("stale_unreviewed");
    stale_record["record_sequence"] = Value::from(0_u64);
    let mut history = json!({});
    history
        .as_object_mut()
        .expect("fixture closure history should be an object")
        .insert(closure_id.clone(), stale_record);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[("task_closure_record_history", history)],
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "Task 9.1 replay fixture current closure also stale status",
    );
    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "Task 9.1 replay fixture current closure also stale operator",
        ),
        "Task 9.1 replay fixture current closure also stale operator",
    );
    let explain_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        "Task 9.1 replay fixture current closure also stale explain-review-state",
    );
    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "Task 9.1 replay fixture current closure also stale repair-review-state",
    );

    for (surface, json) in [
        ("status", &status_json),
        ("operator", &operator_json),
        ("explain-review-state", &explain_json),
        ("repair-review-state", &repair_json),
    ] {
        if json["current_task_closures"].is_array() {
            assert!(
                json["current_task_closures"]
                    .as_array()
                    .is_some_and(|closures| closures.iter().any(|record| {
                        record["closure_record_id"].as_str() == Some(closure_id.as_str())
                    })),
                "{surface} should preserve the current closure evidence for diagnosis: {json}"
            );
            assert!(
                json["stale_unreviewed_closures"]
                    .as_array()
                    .is_some_and(|closures| closures
                        .iter()
                        .all(|closure| closure.as_str() != Some(closure_id.as_str()))),
                "{surface} must not project the current closure id as stale: {json}"
            );
        }
        assert!(
            json["reason_codes"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(
                    json["blocking_reason_codes"]
                        .as_array()
                        .into_iter()
                        .flatten(),
                )
                .all(|code| code.as_str() != Some("current_stale_closure_overlap")),
            "{surface} should not need the current/stale overlap invariant after reducer filtering: {json}"
        );
        let recommended_command = json["recommended_command"].as_str().unwrap_or_default();
        assert!(
            !recommended_command.contains("--task 1") || !recommended_command.contains("reopen"),
            "{surface} must not recommend reopening the task whose closure is already current: {json}"
        );
        assert!(
            json["public_repair_targets"]
                .as_array()
                .is_none_or(|targets| targets.iter().all(|target| {
                    target["command_kind"].as_str() != Some("reopen") || target["task"] != json!(1)
                })),
            "{surface} must not retain a reopen repair target for the current task closure: {json}"
        );
        for hidden in [
            concat!("record", "-review-dispatch"),
            concat!("gate", "-review"),
            concat!("gate", "-finish"),
            concat!("rebuild", "-evidence"),
        ] {
            let public_actions = [
                json["recommended_command"].as_str(),
                json["next_public_action"]["command"].as_str(),
            ];
            assert!(
                public_actions
                    .into_iter()
                    .flatten()
                    .all(|command| !command.contains(hidden)),
                "{surface} public output must not mention hidden/debug helper `{hidden}`: {json}"
            );
        }
    }
}

#[test]
fn internal_only_compatibility_read_surface_invariant_blocks_hidden_and_eligibility_rejected_commands()
 {
    let (repo_dir, state_dir) = init_repo("read-invariant-hidden-rejected-command");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "read-invariant-hidden-rejected-command");
    let runtime = discover_execution_runtime(repo, state, "hidden command invariant");
    let mut status = runtime
        .status(&PlanExecutionStatusArgs {
            plan: plan_rel.into(),
            external_review_result_ready: false,
        })
        .expect("fixture status should load");

    status.recommended_command = Some(format!(
        "featureforge plan execution {} --plan {plan_rel}",
        concat!("gate", "-review")
    ));
    apply_read_surface_invariants(&mut status);
    assert_eq!(status.state_kind, "blocked_runtime_bug");
    assert!(status.recommended_command.is_none());
    assert!(
        status
            .reason_codes
            .iter()
            .any(|code| code == "recommended_command_hidden_or_debug"),
        "hidden command violation should carry a shared reason code: {status:?}"
    );

    let mut rejected = runtime
        .status(&PlanExecutionStatusArgs {
            plan: plan_rel.into(),
            external_review_result_ready: false,
        })
        .expect("fixture status should reload");
    rejected.recommended_public_command = Some(PublicCommand::Begin {
        plan: plan_rel.to_owned(),
        task: 99,
        step: 1,
        execution_mode: Some(String::from("featureforge:executing-plans")),
        fingerprint: Some(String::from("wrong-target")),
    });
    rejected.recommended_command = Some(format!(
        "featureforge plan execution begin --plan {plan_rel} --task 99 --step 1 --execution-mode featureforge:executing-plans --expect-execution-fingerprint wrong-target"
    ));
    let violations =
        check_runtime_status_invariants(&rejected, InvariantEnforcementMode::PostMutation);
    assert!(
        violations
            .iter()
            .any(|violation| violation.code == "recommended_mutation_command_rejected"),
        "post-mutation invariant reuse should reject commands the mutation oracle rejects: {violations:?}"
    );
    apply_read_surface_invariants(&mut rejected);
    assert_eq!(rejected.state_kind, "blocked_runtime_bug");
    assert!(rejected.recommended_command.is_none());
}

#[test]
fn internal_only_compatibility_post_mutation_invariant_reuses_shared_checker_after_mutation_attempt()
 {
    let (repo_dir, state_dir) = init_repo("post-mutation-shared-invariant");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "post-mutation-shared-invariant");

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "post-mutation invariant baseline status",
    );
    let preflight_json = internal_only_unit_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        concat!("post-mutation pre", "flight"),
    );
    assert_eq!(preflight_json["allowed"], true);
    let begin_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_json["execution_fingerprint"]
                .as_str()
                .expect("baseline status should expose execution fingerprint"),
        ],
        "post-mutation begin setup",
    );
    let output = run_rust_featureforge_with_env(
        repo,
        state,
        &[
            "plan",
            "execution",
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed Task 1 for post-mutation invariant coverage.",
            "--manual-verify-summary",
            "Verified Task 1 for post-mutation invariant coverage.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_json["execution_fingerprint"]
                .as_str()
                .expect("begin output should expose execution fingerprint"),
        ],
        &[(
            "FEATUREFORGE_PLAN_EXECUTION_POST_MUTATION_INVARIANT_TEST_INJECTION",
            "hidden_recommended_command",
        )],
        "post-mutation complete setup",
    );
    assert!(
        !output.status.success(),
        "post-mutation invariant injection should fail the mutation attempt"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stderr.contains("Post-mutation invariant violated")
            || stdout.contains("Post-mutation invariant violated"),
        "mutation failure should come from post-mutation invariant enforcement\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("recommended_command_hidden_or_debug")
            || stdout.contains("recommended_command_hidden_or_debug"),
        "post-mutation failure should reuse the shared hidden-command invariant code\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn internal_only_compatibility_read_surface_invariant_sanitizes_hidden_command_on_actual_routing_projection()
 {
    let (repo_dir, state_dir) = init_repo("read-invariant-routing-hidden-command");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "read-invariant-routing-hidden-command");
    let runtime = discover_execution_runtime(repo, state, "hidden command routing invariant");
    let mut routing =
        query_workflow_routing_state_for_runtime(&runtime, Some(Path::new(plan_rel)), false)
            .expect("actual routing query should produce a route to sanitize");
    let hidden_command = format!(
        "featureforge plan execution {} --plan {plan_rel}",
        concat!("gate", "-review")
    );
    routing.recommended_command = Some(hidden_command.clone());
    if let Some(status) = routing.execution_status.as_mut() {
        status.recommended_command = Some(hidden_command);
    }

    apply_read_surface_invariants_to_routing(&mut routing);

    let routing_json =
        to_value(&routing).expect("sanitized routing state should serialize for assertions");
    assert_eq!(routing_json["phase_detail"], "blocked_runtime_bug");
    assert_eq!(
        routing_json["execution_status"]["state_kind"],
        "blocked_runtime_bug"
    );
    assert!(routing_json["recommended_command"].is_null());
    assert!(routing_json["execution_status"]["recommended_command"].is_null());
    assert!(
        routing_json["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| {
                codes
                    .iter()
                    .any(|code| code == "recommended_command_hidden_or_debug")
            }),
        "routing sanitizer should attach hidden-command invariant evidence: {routing_json}"
    );
}

#[test]
fn internal_only_compatibility_repair_review_state_rebinds_route_after_read_invariant_projection() {
    let (repo_dir, state_dir) = init_repo("repair-review-state-read-invariant-route-rebind");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let repair_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &[
                "plan",
                "execution",
                "repair-review-state",
                "--plan",
                plan_rel,
            ],
            &[(
                "FEATUREFORGE_PLAN_EXECUTION_READ_INVARIANT_TEST_INJECTION",
                "hidden_recommended_command",
            )],
            "repair-review-state read invariant route rebind",
        ),
        "repair-review-state read invariant route rebind",
    );

    assert_eq!(
        repair_json["phase_detail"], "blocked_runtime_bug",
        "repair-review-state must use the invariant-adjusted route instead of stale pre-invariant surfaces: {repair_json}"
    );
    assert!(
        repair_json["recommended_command"].is_null(),
        "repair-review-state must not expose stale pre-invariant commands after route rebinding: {repair_json}"
    );
    assert!(
        repair_json["recommended_public_command_argv"].is_null(),
        "repair-review-state must not expose stale pre-invariant argv after route rebinding: {repair_json}"
    );
    assert!(
        repair_json["required_inputs"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "blocked runtime-bug route should not inherit stale required inputs: {repair_json}"
    );
    assert!(
        repair_json["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| {
                codes
                    .iter()
                    .any(|code| code == "recommended_command_hidden_or_debug")
            }),
        "repair-review-state must carry invariant evidence from the rebound route: {repair_json}"
    );
}

#[cfg(windows)]
fn create_dir_symlink(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).expect("directory symlink should be creatable");
}

fn run_rust_featureforge_with_env(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    context: &str,
) -> Output {
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    run(command, context)
}

fn run_rust_featureforge_with_env_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    context: &str,
) -> Output {
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    run(command, context)
}

fn run_plan_execution_json(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Value {
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["plan", "execution"])
        .args(args);
    parse_json(&run(command, context), context)
}

fn materialize_state_dir_projections(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let materialized = run_plan_execution_json(
        repo,
        state_dir,
        &["materialize-projections", "--plan", plan],
        context,
    );
    assert_eq!(materialized["action"], Value::from("materialized"));
    assert_eq!(materialized["runtime_truth_changed"], Value::Bool(false));
    materialized
}

fn internal_only_run_plan_execution_json_direct_or_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let explain_review_state = concat!("explain", "-review-state");
    if args.len() == 3 && args[0] == explain_review_state && args[1] == "--plan" {
        return internal_only_unit_explain_review_state_json(repo, state_dir, args[2], context);
    }
    run_plan_execution_json(repo, state_dir, args, context)
}

fn internal_only_unit_explain_review_state_json(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    parse_direct_json_result(
        plan_execution_direct_support::internal_only_unit_explain_review_state_json(
            repo, state_dir, &args,
        ),
        context,
    )
}

fn run_plan_execution_json_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["plan", "execution"])
        .args(args);
    parse_json(&run(command, context), context)
}

fn internal_only_unit_plan_execution_preflight_json(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    parse_direct_json_result(
        plan_execution_direct_support::internal_only_runtime_preflight_gate_json(
            repo, state_dir, &args,
        ),
        context,
    )
}

fn internal_only_unit_plan_execution_gate_review_json(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    parse_direct_json_result(
        plan_execution_direct_support::internal_only_runtime_review_gate_json(
            repo, state_dir, &args,
        ),
        context,
    )
}

fn internal_only_runtime_review_dispatch_authority_json(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
    context: &str,
) -> Value {
    let args = RecordReviewDispatchArgs {
        plan: plan.into(),
        scope,
        task,
    };
    parse_direct_json_result(
        plan_execution_direct_support::internal_only_runtime_review_dispatch_authority_json(
            repo, state_dir, &args,
        ),
        context,
    )
}

fn internal_only_unit_reconcile_review_state_json(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    parse_direct_json_result(
        plan_execution_direct_support::internal_only_unit_reconcile_review_state_json(
            repo, state_dir, &args,
        ),
        context,
    )
}

fn internal_only_unit_plan_execution_output(result: Result<Value, String>) -> Output {
    match result {
        Ok(value) => value_to_json_output(value),
        Err(error) => output_with_code(1, Vec::new(), error.into_bytes()),
    }
}

fn internal_only_workflow_preflight_json_direct(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let runtime = discover_execution_runtime(repo, state_dir, context);
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    to_value(
        runtime
            .preflight_gate(&args)
            .unwrap_or_else(|error| panic!("{context} should succeed: {error:?}")),
    )
    .unwrap_or_else(|error| {
        panic!(
            "{context} should serialize workflow {} result: {error}",
            concat!("pre", "flight")
        )
    })
}

fn internal_only_workflow_preflight_output(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Output {
    value_to_json_output(internal_only_workflow_preflight_json_direct(
        repo, state_dir, plan, context,
    ))
}

fn internal_only_workflow_gate_review_json_direct(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let runtime = discover_execution_runtime(repo, state_dir, context);
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    to_value(
        runtime
            .review_gate(&args)
            .unwrap_or_else(|error| panic!("{context} should succeed: {error:?}")),
    )
    .unwrap_or_else(|error| {
        panic!(
            "{context} should serialize workflow {} result: {error}",
            concat!("gate", "-review")
        )
    })
}

fn internal_only_workflow_gate_review_output(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Output {
    value_to_json_output(internal_only_workflow_gate_review_json_direct(
        repo, state_dir, plan, context,
    ))
}

fn internal_only_workflow_gate_finish_json_direct(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let runtime = discover_execution_runtime(repo, state_dir, context);
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    to_value(
        runtime
            .finish_gate(&args)
            .unwrap_or_else(|error| panic!("{context} should succeed: {error:?}")),
    )
    .unwrap_or_else(|error| {
        panic!(
            "{context} should serialize workflow {} result: {error}",
            concat!("gate", "-finish")
        )
    })
}

fn internal_only_workflow_gate_finish_output(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Output {
    value_to_json_output(internal_only_workflow_gate_finish_json_direct(
        repo, state_dir, plan, context,
    ))
}

fn value_to_json_output(value: Value) -> Output {
    output_with_code(
        0,
        json_line(&value).expect("json should serialize"),
        Vec::new(),
    )
}

fn parse_direct_json_result(result: Result<Value, String>, context: &str) -> Value {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context} should succeed: {error:?}"),
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
fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;

    std::process::ExitStatus::from_raw(code << 8)
}

#[cfg(not(unix))]
fn exit_status(code: i32) -> std::process::ExitStatus {
    panic!("exit_status helper is only implemented for unix test targets, got code {code}");
}

fn current_branch_name(repo: &Path) -> String {
    discover_slug_identity(repo).branch_name
}

fn expected_release_base_branch(repo: &Path) -> String {
    let current_branch = current_branch_name(repo);
    resolve_release_base_branch(&repo.join(".git"), &current_branch).unwrap_or(current_branch)
}

fn current_head_sha(repo: &Path) -> String {
    runtime_current_head_sha(repo).expect("head sha should resolve")
}

fn commit_all(repo: &Path, message: &str) {
    let mut git_add = Command::new("git");
    git_add.args(["add", "-A"]).current_dir(repo);
    run_checked(git_add, "git add workflow runtime fixture changes");
    let mut git_commit = Command::new("git");
    git_commit.args(["commit", "-m", message]).current_dir(repo);
    run_checked(git_commit, "git commit workflow runtime fixture changes");
}

fn current_head_tree_sha(repo: &Path) -> String {
    discover_repository(repo)
        .expect("head tree helper should discover repository")
        .head_tree_id_or_empty()
        .expect("head tree helper should resolve HEAD tree")
        .detach()
        .to_string()
}

fn repo_slug(repo: &Path) -> String {
    discover_slug_identity(repo).repo_slug
}

fn sha256_hex(contents: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents);
    format!("{:x}", hasher.finalize())
}

fn semantic_execution_context(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
) -> featureforge::execution::state::ExecutionContext {
    let runtime = discover_execution_runtime(
        repo,
        state_dir,
        "workflow_runtime semantic identity fixture",
    );
    load_execution_context_for_mutation(&runtime, Path::new(plan_rel))
        .expect("workflow_runtime semantic identity fixture should load execution context")
}

fn branch_contract_identity(repo: &Path, state_dir: &Path, plan_rel: &str) -> String {
    let context = semantic_execution_context(repo, state_dir, plan_rel);
    branch_definition_identity_for_context(&context)
}

fn task_contract_identity(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    task_number: u32,
) -> String {
    let context = semantic_execution_context(repo, state_dir, plan_rel);
    task_definition_identity_for_task(&context, task_number)
        .expect("workflow_runtime task semantic identity fixture should compute")
        .expect("workflow_runtime task semantic identity fixture should exist")
}

#[test]
fn internal_only_compatibility_semantic_workspace_identity_ignores_runtime_evidence_attempt_projection_churn_only()
 {
    let (repo_dir, state_dir) = init_repo("semantic-identity-evidence-attempt-projection-churn");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join("docs/featureforge/execution-evidence/task7-attempt.md"),
        "# Execution Evidence\n\n\
        ### Task 1 Step 1\n\
        #### Attempt 1\n\
        **Status:** Completed\n\
        **Recorded At:** 2026-04-26T00:00:00Z\n\
        **Invalidation Reason:** none\n",
    );
    commit_all(repo, "baseline semantic evidence projection fixture");

    let runtime = discover_execution_runtime(repo, state, "semantic evidence projection fixture");
    let baseline = plan_execution_status_json(
        &runtime,
        FULL_CONTRACT_READY_PLAN_REL,
        false,
        "baseline semantic evidence projection status",
    );
    let baseline_semantic_id = baseline["semantic_workspace_tree_id"]
        .as_str()
        .expect("baseline status should expose semantic workspace id")
        .to_owned();
    write_file(
        &repo.join("docs/featureforge/execution-evidence/task7-attempt.md"),
        "# Execution Evidence\n\n\
        ### Task 1 Step 1\n\
        #### Attempt 2\n\
        **Status:** Invalidated\n\
        **Recorded At:** 2026-04-26T01:23:45Z\n\
        **Invalidation Reason:** projection-only retry metadata changed\n",
    );

    let evidence_changed = plan_execution_status_json(
        &runtime,
        FULL_CONTRACT_READY_PLAN_REL,
        false,
        "evidence projection churn semantic status",
    );

    assert_eq!(
        evidence_changed["semantic_workspace_tree_id"],
        Value::from(baseline_semantic_id.clone()),
        "runtime-owned evidence attempt fields must not change semantic workspace identity: {evidence_changed}"
    );

    write_file(
        &repo.join("README.md"),
        "# fixture\n\nreal semantic change\n",
    );
    let source_changed = plan_execution_status_json(
        &runtime,
        FULL_CONTRACT_READY_PLAN_REL,
        false,
        "real source change semantic status",
    );

    assert_ne!(
        source_changed["semantic_workspace_tree_id"],
        Value::from(baseline_semantic_id),
        "real source file changes must still change semantic workspace identity: {source_changed}"
    );
}

fn project_artifact_dir(repo: &Path, state_dir: &Path) -> PathBuf {
    state_dir.join("projects").join(repo_slug(repo))
}

fn preflight_acceptance_state_path(repo: &Path, state_dir: &Path) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    state_dir
        .join("projects")
        .join(repo_slug(repo))
        .join("branches")
        .join(safe_branch)
        .join(concat!("execution-pre", "flight"))
        .join("acceptance-state.json")
}

fn write_branch_test_plan_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    browser_required: &str,
) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let head_sha = current_head_sha(repo);
    let artifact_path = project_artifact_dir(repo, state_dir)
        .join(format!("tester-{safe_branch}-test-plan-20260324-120000.md"));
    let source = format!(
        "# Test Plan\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {head_sha}\n**Browser QA Required:** {browser_required}\n**Generated By:** featureforge:plan-eng-review\n**Generated At:** 2026-03-24T12:00:00Z\n\n## Affected Pages / Routes\n- none\n\n## Key Interactions\n- late-stage workflow routing uses this artifact for QA scoping\n\n## Edge Cases\n- current-branch artifact freshness must stay aligned with the approved plan revision\n\n## Critical Paths\n- branch completion stays blocked until review, QA, and release-readiness artifacts are fresh when required\n",
        repo_slug(repo)
    );
    write_file(&artifact_path, &source);
    let authoritative_path = harness_authoritative_artifact_path(
        state_dir,
        &repo_slug(repo),
        &branch,
        &format!("test-plan-{}.md", sha256_hex(source.as_bytes())),
    );
    write_file(&authoritative_path, &source);
    artifact_path
}

fn write_branch_review_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let strategy_checkpoint_fingerprint = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "plan execution status for workflow review artifact fixture",
    )["last_strategy_checkpoint_fingerprint"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT)
        .to_owned();
    let reviewer_artifact_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-independent-review-20260324-120950.md"
    ));
    let reviewer_artifact_source = format!(
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T12:09:50Z\n\n## Summary\n- dedicated independent reviewer artifact fixture.\n",
        repo_slug(repo),
        current_head_sha(repo)
    );
    write_file(&reviewer_artifact_path, &reviewer_artifact_source);
    let reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&reviewer_artifact_path).expect("reviewer artifact should read"));
    let artifact_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-code-review-20260324-121000.md"
    ));
    write_file(
        &artifact_path,
        &format!(
            "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** {reviewer_artifact_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T12:10:00Z\n\n## Summary\n- synthetic code-review fixture for workflow phase coverage.\n",
            reviewer_artifact_path.display(),
            repo_slug(repo),
            current_head_sha(repo)
        ),
    );
    artifact_path
}

fn write_branch_release_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let artifact_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-release-readiness-20260324-121500.md"
    ));
    write_file(
        &artifact_path,
        &format!(
            "# Release Readiness Result\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Result:** pass\n**Generated By:** featureforge:document-release\n**Generated At:** 2026-03-24T12:15:00Z\n\n## Summary\n- synthetic release-readiness fixture for workflow phase coverage.\n",
            repo_slug(repo),
            current_head_sha(repo)
        ),
    );
    publish_authoritative_release_truth(repo, state_dir, plan_rel, &artifact_path, base_branch);
    artifact_path
}

fn write_branch_qa_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    test_plan_path: &Path,
) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let artifact_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-test-outcome-20260324-121200.md"
    ));
    write_file(
        &artifact_path,
        &format!(
            "# QA Result\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Source Test Plan:** `{}`\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {}\n**Result:** pass\n**Generated By:** featureforge/qa\n**Generated At:** 2026-03-24T12:12:00Z\n\n## Summary\n- synthetic QA fixture for workflow late-gate downstream provenance coverage.\n",
            test_plan_path.display(),
            repo_slug(repo),
            current_head_sha(repo)
        ),
    );
    artifact_path
}

fn replace_in_file(path: &Path, from: &str, to: &str) {
    let source = fs::read_to_string(path).expect("fixture file should be readable for mutation");
    let updated = source.replace(from, to);
    assert_ne!(
        source,
        updated,
        "fixture mutation should change the file contents for {}",
        path.display()
    );
    fs::write(path, updated).expect("fixture file should be writable for mutation");
}

fn set_plan_qa_requirement(repo: &Path, plan_rel: &str, qa_requirement: &str) {
    let plan_path = repo.join(plan_rel);
    let source = fs::read_to_string(&plan_path).expect("fixture plan should be readable");
    let updated = if let Some(current_line) = source
        .lines()
        .find(|line| line.starts_with("**QA Requirement:**"))
    {
        source.replace(
            current_line,
            &format!("**QA Requirement:** {qa_requirement}"),
        )
    } else {
        source.replace(
            "**Last Reviewed By:** plan-eng-review\n",
            &format!(
                "**Last Reviewed By:** plan-eng-review\n**QA Requirement:** {qa_requirement}\n"
            ),
        )
    };
    assert_ne!(
        source, updated,
        "plan QA requirement rewrite should change the fixture contents"
    );
    fs::write(&plan_path, updated).expect("fixture plan should be writable");
}

fn rewrite_source_test_plan_header(source: &str, source_test_plan: &Path) -> String {
    let replacement = format!("**Source Test Plan:** `{}`", source_test_plan.display());
    let mut replaced = false;
    let rewritten = source
        .lines()
        .map(|line| {
            if line.trim().starts_with("**Source Test Plan:**") {
                replaced = true;
                replacement.clone()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        replaced,
        "QA artifact should include a Source Test Plan header"
    );
    format!("{rewritten}\n")
}

fn remove_source_test_plan_header(source: &str) -> String {
    let mut removed = false;
    let rewritten = source
        .lines()
        .filter(|line| {
            let keep = !line.trim().starts_with("**Source Test Plan:**");
            if !keep {
                removed = true;
            }
            keep
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        removed,
        "QA artifact should include a Source Test Plan header"
    );
    format!("{rewritten}\n")
}

fn blank_source_test_plan_header(source: &str) -> String {
    let mut replaced = false;
    let rewritten = source
        .lines()
        .map(|line| {
            if line.trim().starts_with("**Source Test Plan:**") {
                replaced = true;
                String::from("**Source Test Plan:** ``")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        replaced,
        "QA artifact should include a Source Test Plan header"
    );
    format!("{rewritten}\n")
}

fn prepare_preflight_acceptance_workspace(repo: &Path, branch_name: &str) {
    let mut checkout = Command::new("git");
    checkout
        .args(["checkout", "-B", branch_name])
        .current_dir(repo);
    run_checked(
        checkout,
        concat!("git checkout pre", "flight acceptance branch"),
    );
}

fn complete_workflow_fixture_execution(repo: &Path, state: &Path, plan_rel: &str) {
    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "not-required");
}

fn complete_workflow_fixture_execution_with_qa_requirement(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    qa_requirement: &str,
) {
    if plan_rel == FULL_CONTRACT_READY_PLAN_REL {
        let mode = match qa_requirement {
            "required" => Some(WorkflowRuntimeFixtureQaMode::Required),
            "not-required" => Some(WorkflowRuntimeFixtureQaMode::NotRequired),
            _ => None,
        };
        if let Some(mode) = mode {
            populate_execution_fixture_from_template(
                workflow_runtime_execution_fixture_template(mode),
                repo,
                state,
            );
            return;
        }
    }
    complete_workflow_fixture_execution_with_qa_requirement_slow(
        repo,
        state,
        plan_rel,
        qa_requirement,
    );
}

fn complete_workflow_fixture_execution_with_qa_requirement_slow(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    qa_requirement: &str,
) {
    install_full_contract_ready_artifacts(repo);
    set_plan_qa_requirement(repo, plan_rel, qa_requirement);
    write_file(
        &repo.join("tests/workflow_runtime.rs"),
        "synthetic route proof\n",
    );
    prepare_preflight_acceptance_workspace(repo, "workflow-runtime-fixture");

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before workflow routing fixture",
    );
    let begin_json = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_json["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for workflow routing fixture",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed the routing fixture task.",
            "--manual-verify-summary",
            "Verified by workflow runtime fixture setup.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_json["execution_fingerprint"]
                .as_str()
                .expect("begin fingerprint should be present"),
        ],
        "plan execution complete for workflow routing fixture",
    );
    seed_current_branch_closure_truth(repo, state, plan_rel, 1);
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
}

fn update_authoritative_harness_state(
    repo: &Path,
    state: &Path,
    branch: &str,
    plan_rel: &str,
    plan_revision: u32,
    updates: &[(&str, Value)],
) {
    let repo_slug = repo_slug(repo);
    let authoritative_state_path = harness_state_path(state, &repo_slug, branch);
    let mut payload: Value =
        match reduced_authoritative_harness_state_for_path(&authoritative_state_path) {
            Some(payload) => payload,
            None if authoritative_state_path.is_file() => serde_json::from_str(
                &fs::read_to_string(&authoritative_state_path)
                    .expect("authoritative harness state should stay readable"),
            )
            .expect("authoritative harness state should stay valid json"),
            None => {
                let status_json = run_plan_execution_json(
                    repo,
                    state,
                    &["status", "--plan", plan_rel],
                    "status for synthesized authoritative harness state",
                );
                let execution_run_id = status_json["execution_run_id"]
                    .as_str()
                    .expect(
                        "status should expose execution_run_id for synthesized authoritative state",
                    )
                    .to_string();
                let chunk_id = status_json["chunk_id"].as_str().map(str::to_owned);
                let mut object = serde_json::Map::new();
                object.insert("schema_version".to_string(), Value::from(1));
                object.insert(
                    "run_identity".to_string(),
                    Value::Object(serde_json::Map::from_iter([
                        (
                            "execution_run_id".to_string(),
                            Value::from(execution_run_id),
                        ),
                        ("source_plan_path".to_string(), Value::from(plan_rel)),
                        (
                            "source_plan_revision".to_string(),
                            Value::from(plan_revision),
                        ),
                    ])),
                );
                if let Some(chunk_id) = chunk_id {
                    object.insert("chunk_id".to_string(), Value::from(chunk_id));
                }
                object.insert(
                    "active_worktree_lease_fingerprints".to_string(),
                    Value::Array(Vec::new()),
                );
                object.insert(
                    "active_worktree_lease_bindings".to_string(),
                    Value::Array(Vec::new()),
                );
                Value::Object(object)
            }
        };
    let object = payload
        .as_object_mut()
        .expect("authoritative harness state should remain a json object");
    object
        .entry("strategy_state".to_string())
        .or_insert_with(|| Value::from("ready"));
    object
        .entry("strategy_checkpoint_kind".to_string())
        .or_insert_with(|| Value::from("initial_dispatch"));
    object
        .entry("last_strategy_checkpoint_fingerprint".to_string())
        .or_insert_with(|| Value::from(FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT));
    object
        .entry("strategy_reset_required".to_string())
        .or_insert_with(|| Value::Bool(false));
    for (key, value) in updates {
        object.insert((*key).to_string(), value.clone());
    }
    write_file(
        &authoritative_state_path,
        &serde_json::to_string(&payload).expect("authoritative harness state should serialize"),
    );
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(
        &authoritative_state_path,
        &payload,
    )
    .expect("authoritative workflow-runtime fixture update should sync typed event authority");
}

fn reduced_authoritative_harness_state_for_path(state_path: &Path) -> Option<Value> {
    featureforge::execution::event_log::load_reduced_authoritative_state_for_tests(state_path)
        .unwrap_or_else(|error| {
            panic!(
                "event-authoritative workflow-runtime harness state should be reducible for {}: {}",
                state_path.display(),
                error.message
            )
        })
}

fn bind_explicit_reopen_repair_target(
    repo: &Path,
    state: &Path,
    branch: &str,
    plan_rel: &str,
    plan_revision: u32,
    task: u32,
    step: u32,
) {
    update_authoritative_harness_state(
        repo,
        state,
        branch,
        plan_rel,
        plan_revision,
        &[
            (
                "explicit_reopen_repair_targets",
                json!([{
                    "target_task": task,
                    "target_step": step,
                    "target_record_id": execution_step_repair_target_id(task, step),
                    "created_sequence": 1,
                    "expires_on_plan_fingerprint_change": true
                }]),
            ),
            ("review_state_repair_follow_up_record", Value::Null),
            ("review_state_repair_follow_up", Value::Null),
            ("review_state_repair_follow_up_task", Value::Null),
            ("review_state_repair_follow_up_step", Value::Null),
            (
                "review_state_repair_follow_up_closure_record_id",
                Value::Null,
            ),
        ],
    );
}

fn update_current_history_record_field(
    repo: &Path,
    state: &Path,
    history_field: &str,
    current_id_field: &str,
    record_field: &str,
    value: Value,
) {
    let branch = current_branch_name(repo);
    let repo_slug = repo_slug(repo);
    let state_path = harness_state_path(state, &repo_slug, &branch);
    let mut payload =
        reduced_authoritative_harness_state_for_path(&state_path).unwrap_or_else(|| {
            serde_json::from_str(
                &fs::read_to_string(&state_path)
                    .expect("authoritative harness state should be readable"),
            )
            .expect("authoritative harness state should be valid json")
        });
    let root = payload
        .as_object_mut()
        .expect("authoritative harness state should remain an object");
    let current_record_id = root
        .get(current_id_field)
        .and_then(Value::as_str)
        .filter(|record_id| !record_id.trim().is_empty())
        .expect("current authoritative record id should be present")
        .to_owned();
    let record = root
        .get_mut(history_field)
        .and_then(Value::as_object_mut)
        .and_then(|history| history.get_mut(&current_record_id))
        .and_then(Value::as_object_mut)
        .expect("current authoritative history record should be present");
    record.insert(record_field.to_owned(), value);
    write_file(
        &state_path,
        &serde_json::to_string(&payload).expect("authoritative harness state should serialize"),
    );
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(&state_path, &payload)
        .expect("authoritative workflow-runtime history update should sync typed event authority");
}

fn current_release_readiness_record_id(repo: &Path, state: &Path) -> Option<String> {
    let branch = current_branch_name(repo);
    let state_path = harness_state_path(state, &repo_slug(repo), &branch);
    let payload = reduced_authoritative_harness_state_for_path(&state_path).or_else(|| {
        fs::read_to_string(&state_path)
            .ok()
            .and_then(|source| serde_json::from_str(&source).ok())
    })?;
    payload
        .get("current_release_readiness_record_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn current_final_review_record_id(repo: &Path, state: &Path) -> Option<String> {
    let branch = current_branch_name(repo);
    let state_path = harness_state_path(state, &repo_slug(repo), &branch);
    let payload = reduced_authoritative_harness_state_for_path(&state_path).or_else(|| {
        fs::read_to_string(&state_path)
            .ok()
            .and_then(|source| serde_json::from_str(&source).ok())
    })?;
    payload
        .get("current_final_review_record_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn seed_current_branch_closure_truth(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    plan_revision: u32,
) {
    let branch = current_branch_name(repo);
    let base_branch = expected_release_base_branch(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let execution_run_id = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "workflow runtime status for current task-closure fixture provenance",
    )["execution_run_id"]
        .as_str()
        .expect("status should expose execution_run_id for current task-closure fixtures")
        .to_owned();
    let branch_contract_identity = branch_contract_identity(repo, state, plan_rel);
    let task_contract_identity = task_contract_identity(repo, state, plan_rel, 1);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        plan_revision,
        &[
            ("dependency_index_state", Value::from("fresh")),
            (
                "current_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            (
                "current_branch_closure_reviewed_state_id",
                Value::from(reviewed_state_id.clone()),
            ),
            (
                "current_branch_closure_contract_identity",
                Value::from(branch_contract_identity.clone()),
            ),
            (
                "branch_closure_records",
                json!({
                    "branch-release-closure": {
                        "branch_closure_id": "branch-release-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": plan_revision,
                        "repo_slug": repo_slug(repo),
                        "branch_name": branch.clone(),
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id,
                        "contract_identity": branch_contract_identity,
                        "effective_reviewed_branch_surface": "repo_tracked_content",
                        "source_task_closure_ids": ["task-1-closure"],
                        "provenance_basis": "task_closure_lineage",
                        "closure_status": "current",
                        "superseded_branch_closure_ids": []
                    }
                }),
            ),
            (
                "current_task_closure_records",
                json!({
                    "task-1": {
                        "dispatch_id": "fixture-task-dispatch",
                        "closure_record_id": "task-1-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": plan_revision,
                        "execution_run_id": execution_run_id.clone(),
                        "reviewed_state_id": reviewed_state_id.clone(),
                        "contract_identity": task_contract_identity.clone(),
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"workflow runtime task closure review fixture"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"workflow runtime task closure verification fixture"),
                        "closure_status": "current"
                    }
                }),
            ),
            (
                "task_closure_record_history",
                json!({
                    "task-1-closure": {
                        "dispatch_id": "fixture-task-dispatch",
                        "closure_record_id": "task-1-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": plan_revision,
                        "execution_run_id": execution_run_id,
                        "reviewed_state_id": reviewed_state_id.clone(),
                        "contract_identity": task_contract_identity,
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"workflow runtime task closure review fixture"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"workflow runtime task closure verification fixture"),
                        "closure_status": "current"
                    }
                }),
            ),
        ],
    );
}

fn write_runtime_remediation_fs15_plan(repo: &Path, plan_rel: &str, spec_rel: &str) {
    let source = format!(
        r#"# Runtime Remediation FS-15 Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** featureforge:executing-plans
**Source Spec:** `{spec_rel}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1, Task 2, Task 6
- VERIFY-001 -> Task 1, Task 2, Task 6

## Execution Strategy

- Repair Task 1 through the public task-closure route before resuming forward work.
- Execute Task 2 before Task 6 to keep stale-boundary ordering deterministic for FS-15 routing checks.

## Dependency Diagram

```text
Task 1 -> Task 2 -> Task 6
```

## Task 1: FS-15 earlier repair target

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Recreates the earlier repair transition before stale-boundary targeting continues.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep each task to one step for deterministic stale-target routing assertions.

**Done when:**
- Recreates the earlier repair transition before stale-boundary targeting continues.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Repair task 1 through the public closure route**

## Task 2: FS-15 earliest stale boundary

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Task 2 represents the earliest unresolved stale boundary.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep each task to one step for deterministic stale-target routing assertions.

**Done when:**
- Task 2 represents the earliest unresolved stale boundary.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Execute task 2 baseline step**

## Task 6: FS-15 later stale overlay target

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Task 6 represents the later stale overlay that must not outrank Task 2.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep each task to one step for deterministic stale-target routing assertions.

**Done when:**
- Task 6 represents the later stale overlay that must not outrank Task 2.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Execute task 6 baseline step**
"#
    );
    write_file(&repo.join(plan_rel), &source);
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
}

fn write_runtime_fs14_fs16_task_boundary_plan(repo: &Path, plan_rel: &str, spec_rel: &str) {
    let source = format!(
        r#"# Runtime Remediation FS-14/FS-16 Task-Boundary Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `{spec_rel}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-004 -> Task 1
- VERIFY-001 -> Task 2

## Execution Strategy

- Execute Task 1 serially. It establishes boundary gating before follow-on work begins.
- Execute Task 2 serially after Task 1. It validates task-boundary workflow routing.

## Dependency Diagram

```text
Task 1 -> Task 2
```

## Task 1: Core flow

**Spec Coverage:** REQ-001, REQ-004
**Goal:** Task 1 execution reaches a boundary gate before Task 2 starts.

**Context:**
- Spec Coverage: REQ-001, REQ-004.

**Constraints:**
- Keep fixture inputs deterministic.

**Done when:**
- Task 1 execution reaches a boundary gate before Task 2 starts.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Prepare workflow fixture output**
- [ ] **Step 2: Validate workflow fixture output**

## Task 2: Follow-on flow

**Spec Coverage:** VERIFY-001
**Goal:** Task 2 should remain blocked until Task 1 closure requirements are met.

**Context:**
- Spec Coverage: VERIFY-001.

**Constraints:**
- Preserve deterministic task-boundary diagnostics.

**Done when:**
- Task 2 should remain blocked until Task 1 closure requirements are met.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Start the follow-on task**
"#
    );
    write_file(&repo.join(plan_rel), &source);
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
}

fn internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
) -> String {
    install_full_contract_ready_artifacts(repo);
    write_runtime_fs14_fs16_task_boundary_plan(repo, plan_rel, FULL_CONTRACT_READY_SPEC_REL);
    prepare_preflight_acceptance_workspace(repo, "runtime-remediation-fs14-fs16-task-boundary");

    let status_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-14/FS-16 status before task-boundary fixture begin",
    );
    let preflight = internal_only_unit_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        concat!(
            "FS-14/FS-16 pre",
            "flight before task-boundary fixture execution"
        ),
    );
    assert_eq!(
        preflight["allowed"],
        Value::Bool(true),
        "FS-14/FS-16 fixture {} should allow execution",
        concat!("pre", "flight"),
    );
    let begin_task1_step1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-14/FS-16 status should expose execution fingerprint before begin"),
        ],
        "FS-14/FS-16 begin task 1 step 1 fixture bootstrap",
    );
    let complete_task1_step1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-14/FS-16 completed task 1 step 1 fixture bootstrap.",
            "--manual-verify-summary",
            "FS-14/FS-16 fixture verification summary for task 1 step 1.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("FS-14/FS-16 begin should expose execution fingerprint"),
        ],
        "FS-14/FS-16 complete task 1 step 1 fixture bootstrap",
    );
    let begin_task1_step2 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            complete_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("FS-14/FS-16 complete should expose execution fingerprint"),
        ],
        "FS-14/FS-16 begin task 1 step 2 fixture bootstrap",
    );
    internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-14/FS-16 completed task 1 step 2 fixture bootstrap.",
            "--manual-verify-summary",
            "FS-14/FS-16 fixture verification summary for task 1 step 2.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("FS-14/FS-16 begin should expose execution fingerprint"),
        ],
        "FS-14/FS-16 complete task 1 step 2 fixture bootstrap",
    );
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(repo, state, &branch, plan_rel, 1, &[]);
    let dispatch = internal_only_runtime_review_dispatch_authority_json(
        repo,
        state,
        plan_rel,
        ReviewDispatchScopeArg::Task,
        Some(1),
        concat!("FS-14/FS-16 record", "-review-dispatch fixture bootstrap"),
    );
    assert_eq!(
        dispatch["allowed"],
        Value::Bool(true),
        "FS-14/FS-16 dispatch bootstrap should succeed: {dispatch:?}",
    );
    dispatch["dispatch_id"]
        .as_str()
        .expect("FS-14/FS-16 fixture dispatch bootstrap should expose dispatch_id")
        .to_owned()
}

fn publish_authoritative_release_truth(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    release_path: &Path,
    base_branch: &str,
) {
    let branch = current_branch_name(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let release_source = fs::read_to_string(release_path)
        .expect("workflow release artifact should be readable for authoritative publication");
    let release_fingerprint = sha256_hex(release_source.as_bytes());
    let release_summary =
        "Workflow runtime release-readiness fixture for authoritative late-stage routing.";
    let release_summary_hash = sha256_hex(release_summary.as_bytes());
    let release_record_id = format!("release-readiness-record-{release_fingerprint}");
    seed_current_branch_closure_truth(repo, state, plan_rel, 1);
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("release-docs-{release_fingerprint}.md"),
        ),
        &release_source,
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("release_docs_state", Value::from("fresh")),
            (
                "last_release_docs_artifact_fingerprint",
                Value::from(release_fingerprint.clone()),
            ),
            ("current_release_readiness_result", Value::from("ready")),
            (
                "current_release_readiness_summary_hash",
                Value::from(release_summary_hash.clone()),
            ),
            (
                "current_release_readiness_record_id",
                Value::from(release_record_id.clone()),
            ),
            (
                "release_readiness_record_history",
                json!({
                    release_record_id.clone(): {
                        "record_id": release_record_id,
                        "record_sequence": 1,
                        "record_status": "current",
                        "branch_closure_id": "branch-release-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "repo_slug": repo_slug(repo),
                        "branch_name": branch,
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id,
                        "result": "ready",
                        "release_docs_fingerprint": release_fingerprint,
                        "summary": release_summary,
                        "summary_hash": release_summary_hash,
                        "generated_by_identity": "featureforge/release-readiness"
                    }
                }),
            ),
        ],
    );
}

fn publish_authoritative_qa_truth(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    test_plan_path: &Path,
    qa_path: &Path,
    base_branch: &str,
) -> (String, String) {
    let branch = current_branch_name(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let authoritative_test_plan_source = fs::read_to_string(test_plan_path)
        .expect("workflow test-plan artifact should be readable for authoritative publication");
    let authoritative_test_plan_fingerprint = sha256_hex(authoritative_test_plan_source.as_bytes());
    let authoritative_test_plan_path = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("test-plan-{authoritative_test_plan_fingerprint}.md"),
    );
    write_file(
        &authoritative_test_plan_path,
        &authoritative_test_plan_source,
    );

    let authoritative_qa_source = rewrite_source_test_plan_header(
        &fs::read_to_string(qa_path)
            .expect("workflow QA artifact should be readable for authoritative publication"),
        &authoritative_test_plan_path,
    );
    let authoritative_qa_fingerprint = sha256_hex(authoritative_qa_source.as_bytes());
    let qa_summary = "Workflow runtime QA fixture for authoritative late-stage routing.";
    let qa_summary_hash = sha256_hex(qa_summary.as_bytes());
    let qa_record_id = format!("browser-qa-record-{authoritative_qa_fingerprint}");
    let final_review_record_id = current_final_review_record_id(repo, state);
    seed_current_branch_closure_truth(repo, state, plan_rel, 1);
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("browser-qa-{authoritative_qa_fingerprint}.md"),
        ),
        &authoritative_qa_source,
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("browser_qa_state", Value::from("fresh")),
            (
                "last_browser_qa_artifact_fingerprint",
                Value::from(authoritative_qa_fingerprint.clone()),
            ),
            (
                "current_qa_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_qa_result", Value::from("pass")),
            (
                "current_qa_summary_hash",
                Value::from(qa_summary_hash.clone()),
            ),
            ("current_qa_record_id", Value::from(qa_record_id.clone())),
            (
                "browser_qa_record_history",
                json!({
                    qa_record_id.clone(): {
                        "record_id": qa_record_id,
                        "record_sequence": 1,
                        "record_status": "current",
                        "branch_closure_id": "branch-release-closure",
                        "final_review_record_id": final_review_record_id,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "repo_slug": repo_slug(repo),
                        "branch_name": branch,
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id,
                        "result": "pass",
                        "browser_qa_fingerprint": authoritative_qa_fingerprint.clone(),
                        "source_test_plan_fingerprint": authoritative_test_plan_fingerprint.clone(),
                        "summary": qa_summary,
                        "summary_hash": qa_summary_hash,
                        "generated_by_identity": "featureforge/qa"
                    }
                }),
            ),
        ],
    );
    (
        authoritative_test_plan_fingerprint,
        authoritative_qa_fingerprint,
    )
}

fn publish_authoritative_final_review_truth(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    review_path: &Path,
) {
    let branch = current_branch_name(repo);
    let base_branch = expected_release_base_branch(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let review_source = fs::read_to_string(review_path)
        .expect("workflow review artifact should be readable for authoritative publication");
    let review_fingerprint = sha256_hex(review_source.as_bytes());
    let final_review_summary =
        "Workflow runtime final-review fixture for authoritative late-stage routing.";
    let final_review_summary_hash = sha256_hex(final_review_summary.as_bytes());
    let final_review_record_id = format!("final-review-record-{review_fingerprint}");
    let release_readiness_record_id = current_release_readiness_record_id(repo, state);
    let browser_qa_required = match parse_plan_file(repo.join(plan_rel))
        .expect("plan should parse for authoritative final-review publication")
        .qa_requirement
        .as_deref()
    {
        Some("required") => Some(true),
        Some("not-required") => Some(false),
        _ => None,
    };
    seed_current_branch_closure_truth(repo, state, plan_rel, 1);
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("final-review-{review_fingerprint}.md"),
        ),
        &review_source,
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("fresh")),
            ("browser_qa_state", Value::from("not_required")),
            (
                "last_final_review_artifact_fingerprint",
                Value::from(review_fingerprint.clone()),
            ),
            (
                "current_final_review_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            (
                "current_final_review_dispatch_id",
                Value::from("fixture-final-review-dispatch"),
            ),
            (
                "current_final_review_reviewer_source",
                Value::from("fresh-context-subagent"),
            ),
            (
                "current_final_review_reviewer_id",
                Value::from("reviewer-fixture-001"),
            ),
            ("current_final_review_result", Value::from("pass")),
            (
                "current_final_review_summary_hash",
                Value::from(final_review_summary_hash.clone()),
            ),
            (
                "current_final_review_record_id",
                Value::from(final_review_record_id.clone()),
            ),
            (
                "final_review_record_history",
                json!({
                    final_review_record_id.clone(): {
                        "record_id": final_review_record_id,
                        "record_sequence": 1,
                        "record_status": "current",
                        "branch_closure_id": "branch-release-closure",
                        "release_readiness_record_id": release_readiness_record_id,
                        "dispatch_id": "fixture-final-review-dispatch",
                        "reviewer_source": "fresh-context-subagent",
                        "reviewer_id": "reviewer-fixture-001",
                        "result": "pass",
                        "final_review_fingerprint": review_fingerprint,
                        "browser_qa_required": browser_qa_required,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "repo_slug": repo_slug(repo),
                        "branch_name": branch.clone(),
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id,
                        "summary": final_review_summary,
                        "summary_hash": final_review_summary_hash
                    }
                }),
            ),
            (
                "finish_review_gate_pass_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
        ],
    );
}

fn internal_only_write_dispatched_branch_review_artifact(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    base_branch: &str,
) -> PathBuf {
    let release_path = write_branch_release_artifact(repo, state, plan_rel, base_branch);
    publish_authoritative_release_truth(repo, state, plan_rel, &release_path, base_branch);
    let initial_review_path = write_branch_review_artifact(repo, state, plan_rel, base_branch);
    publish_authoritative_final_review_truth(repo, state, plan_rel, &initial_review_path);
    let gate_review = internal_only_runtime_review_dispatch_authority_json(
        repo,
        state,
        plan_rel,
        ReviewDispatchScopeArg::FinalReview,
        None,
        concat!(
            "plan execution gate",
            "-review dispatch for workflow review fixture"
        ),
    );
    assert_eq!(
        gate_review["allowed"],
        Value::Bool(true),
        "workflow review fixture should prime a passing {} dispatch before minting a final-review artifact: {gate_review:?}",
        concat!("gate", "-review")
    );
    let review_path = write_branch_review_artifact(repo, state, plan_rel, base_branch);
    publish_authoritative_final_review_truth(repo, state, plan_rel, &review_path);
    review_path
}

fn enable_session_decision(state: &Path, session_key: &str) {
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    write_file(&decision_path, "enabled\n");
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_enabled_ready_plan_to_execution_preflight()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-ready-plan");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-phase-ready"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-phase-ready");

    install_full_contract_ready_artifacts(repo);
    let runtime = discover_execution_runtime(
        repo,
        state,
        concat!(
            "rust canonical workflow phase should route ready plans to execution pre",
            "flight"
        ),
    );
    let phase_json = workflow_phase_json(
        &runtime,
        concat!(
            "rust canonical workflow phase should route ready plans to execution pre",
            "flight"
        ),
    );
    assert_eq!(phase_json["route_status"], "implementation_ready");
    assert_eq!(phase_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(
        phase_json["next_action"],
        concat!("execution pre", "flight")
    );
    assert!(phase_json.get("session_entry").is_none());
    assert_eq!(phase_json["schema_version"], 3);
    assert_eq!(phase_json["route"]["schema_version"], 3);
}

#[test]
fn internal_only_compatibility_canonical_workflow_gate_review_is_read_only_before_dispatch() {
    let (repo_dir, state_dir) = init_repo(concat!(
        "workflow-record",
        "-review-dispatch-cycle-tracking"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = concat!("workflow-record", "-review-dispatch-cycle-tracking");
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, concat!("workflow-record", "-review-dispatch"));
    write_file(&decision_path, "enabled\n");

    let preflight_json = parse_json(
        &internal_only_workflow_preflight_output(
            repo,
            state,
            plan_rel,
            concat!(
                "workflow pre",
                "flight before workflow gate",
                "-review dispatch cycle tracking"
            ),
        ),
        concat!(
            "workflow pre",
            "flight before workflow gate",
            "-review dispatch cycle tracking"
        ),
    );
    assert_eq!(preflight_json["allowed"], true);

    let status_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!(
            "status before begin for workflow gate",
            "-review dispatch cycle tracking"
        ),
    );
    let begin_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status should include execution_fingerprint before begin"),
        ],
        concat!(
            "begin active reviewable work before workflow gate",
            "-review dispatch cycle tracking"
        ),
    );
    assert_eq!(begin_json["active_task"], 1);
    assert_eq!(begin_json["active_step"], 1);
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(repo, state, &branch, plan_rel, 1, &[]);
    let status_before_gate_review = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!("status before workflow gate", "-review read-only check"),
    );

    let gate_review_json = parse_json(
        &internal_only_workflow_gate_review_output(
            repo,
            state,
            plan_rel,
            "workflow gate review should stay read-only while active work is still in progress",
        ),
        "workflow gate review should stay read-only while active work is still in progress",
    );
    assert_eq!(gate_review_json["allowed"], false);
    assert_eq!(gate_review_json["failure_class"], "ExecutionStateNotReady");
    assert!(
        gate_review_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "active_step_in_progress")),
        "workflow gate review should fail while active work is still in progress"
    );

    let status_after_gate_review = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!("status after workflow gate", "-review read-only check"),
    );
    assert_eq!(
        status_after_gate_review["strategy_checkpoint_kind"],
        status_before_gate_review["strategy_checkpoint_kind"],
        "workflow {} should not mutate strategy checkpoint kind",
        concat!("gate", "-review")
    );
    assert_eq!(
        status_after_gate_review["last_strategy_checkpoint_fingerprint"],
        status_before_gate_review["last_strategy_checkpoint_fingerprint"],
        "workflow {} should not mutate strategy checkpoint fingerprint",
        concat!("gate", "-review")
    );
    assert!(
        status_after_gate_review["strategy_state"] == status_before_gate_review["strategy_state"],
        "workflow {} should not mutate strategy state",
        concat!("gate", "-review")
    );

    let gate_review_dispatch_json = internal_only_runtime_review_dispatch_authority_json(
        repo,
        state,
        plan_rel,
        ReviewDispatchScopeArg::Task,
        Some(1),
        concat!(
            "plan execution record",
            "-review-dispatch should mint dispatch lineage even when review is blocked"
        ),
    );
    assert_eq!(gate_review_dispatch_json["allowed"], false);
    assert_eq!(
        gate_review_dispatch_json["failure_class"],
        "ExecutionStateNotReady"
    );
    assert!(
        gate_review_dispatch_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "active_step_in_progress")),
        "workflow {} should still report the active-step block reason",
        concat!("record", "-review-dispatch")
    );

    let status_after_gate_review_dispatch = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!(
            "status after workflow record",
            "-review-dispatch mutation check"
        ),
    );
    assert_eq!(
        status_after_gate_review_dispatch["last_strategy_checkpoint_fingerprint"],
        status_after_gate_review["last_strategy_checkpoint_fingerprint"],
        "workflow {} must not mint a strategy checkpoint fingerprint while active work is still in progress",
        concat!("record", "-review-dispatch")
    );
    assert_eq!(
        status_after_gate_review_dispatch["strategy_checkpoint_kind"],
        status_after_gate_review["strategy_checkpoint_kind"],
        "workflow {} must not change strategy checkpoint kind while active work is still in progress",
        concat!("record", "-review-dispatch")
    );
}

#[test]
fn internal_only_compatibility_workflow_read_commands_do_not_persist_preflight_acceptance() {
    let (repo_dir, state_dir) = init_repo(concat!("workflow-read-only-pre", "flight-boundary"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = concat!("workflow-read-only-pre", "flight-boundary");
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args([
            "checkout",
            "-B",
            concat!("workflow-read-only-pre", "flight"),
        ])
        .current_dir(repo);
    run_checked(
        git_checkout,
        concat!("git checkout workflow-read-only-pre", "flight"),
    );

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");

    let runtime = discover_execution_runtime(
        repo,
        state,
        concat!("workflow phase should not persist pre", "flight acceptance"),
    );
    let phase_json = workflow_phase_json(
        &runtime,
        concat!("workflow phase should not persist pre", "flight acceptance"),
    );
    assert_eq!(phase_json["phase"], concat!("execution_pre", "flight"));
    let doctor_json = workflow_doctor_json(
        &runtime,
        concat!(
            "workflow doctor should not persist pre",
            "flight acceptance"
        ),
    );
    assert_eq!(doctor_json[concat!("pre", "flight")]["allowed"], true);
    let handoff_json = workflow_handoff_json(
        &runtime,
        concat!(
            "workflow handoff should not persist pre",
            "flight acceptance"
        ),
    );
    assert_eq!(
        handoff_json["next_action"],
        concat!("execution pre", "flight")
    );

    let status_after_reads = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status after workflow read commands",
    );
    assert!(
        status_after_reads["execution_run_id"].is_null(),
        "workflow read commands must not persist {} acceptance",
        concat!("pre", "flight")
    );
    assert_eq!(
        status_after_reads["harness_phase"],
        "implementation_handoff",
        "without explicit {} acceptance, harness phase should stay implementation_handoff",
        concat!("pre", "flight")
    );

    let begin_output = run_rust_featureforge_with_env(
        repo,
        state,
        &[
            "plan",
            "execution",
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_after_reads["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        concat!(
            "begin should atomically persist allowed pre",
            "flight acceptance"
        ),
    );
    assert!(
        begin_output.status.success(),
        concat!(
            "begin should succeed without hidden pre",
            "flight priming, got {:?}\nstdout:\n{}\nstderr:\n{}"
        ),
        begin_output.status,
        String::from_utf8_lossy(&begin_output.stdout),
        String::from_utf8_lossy(&begin_output.stderr)
    );
    let status_after_begin = parse_json(
        &begin_output,
        concat!(
            "begin should emit status after atomic pre",
            "flight acceptance"
        ),
    );
    assert!(
        status_after_begin["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "begin should persist execution_run_id while starting execution"
    );
    assert_eq!(status_after_begin["execution_started"], "yes");
    assert_eq!(status_after_begin["active_task"], 1);
    assert_eq!(status_after_begin["active_step"], 1);
}

#[test]
fn internal_only_compatibility_canonical_workflow_public_json_commands_work_for_ready_plan() {
    let (repo_dir, state_dir) = init_repo("workflow-public-json-commands");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-public-json"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-public-json");

    install_full_contract_ready_artifacts(repo);
    let execution_preflight_phase =
        public_harness_phase_from_spec(concat!("execution_pre", "flight"));

    let runtime = discover_execution_runtime(
        repo,
        state,
        "rust canonical workflow doctor should be available on ready plans",
    );
    let doctor_json = workflow_doctor_json(
        &runtime,
        "rust canonical workflow doctor should be available on ready plans",
    );
    assert_eq!(
        doctor_json["phase"],
        Value::String(execution_preflight_phase.clone())
    );
    assert_eq!(doctor_json["route_status"], "implementation_ready");
    assert_eq!(
        doctor_json["next_action"],
        concat!("execution pre", "flight")
    );
    assert_eq!(
        doctor_json["spec_path"],
        "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"
    );
    assert_eq!(doctor_json["plan_path"], plan_rel);
    assert_eq!(doctor_json["contract_state"], "valid");
    assert!(doctor_json.get("session_entry").is_none());
    assert_eq!(doctor_json["schema_version"], 3);
    assert_eq!(doctor_json["route"]["schema_version"], 3);
    assert_eq!(doctor_json["execution_status"]["execution_started"], "no");
    assert_eq!(doctor_json[concat!("pre", "flight")]["allowed"], true);
    assert_eq!(doctor_json["gate_review"], Value::Null);
    assert_eq!(doctor_json["gate_finish"], Value::Null);

    let handoff_json = workflow_handoff_json(
        &runtime,
        "rust canonical workflow handoff should be available on ready plans",
    );
    assert_eq!(
        handoff_json["phase"],
        Value::String(execution_preflight_phase)
    );
    assert_eq!(handoff_json["route_status"], "implementation_ready");
    assert_eq!(handoff_json["execution_started"], "no");
    assert_eq!(
        handoff_json["next_action"],
        concat!("execution pre", "flight")
    );
    assert_eq!(
        handoff_json["spec_path"],
        "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"
    );
    assert_eq!(handoff_json["plan_path"], plan_rel);
    assert_eq!(handoff_json["recommended_skill"], Value::from(""));
    assert_eq!(handoff_json["recommendation"], Value::Null);
    assert_eq!(handoff_json["recommendation_reason"], Value::from(""));
    assert_eq!(handoff_json["state_kind"], "actionable_public_command");
    assert_eq!(
        handoff_json["semantic_workspace_tree_id"],
        doctor_json["execution_status"]["semantic_workspace_tree_id"]
    );
    assert_eq!(
        handoff_json["raw_workspace_tree_id"],
        doctor_json["execution_status"]["raw_workspace_tree_id"]
    );
    assert!(handoff_json.get("session_entry").is_none());
    assert_eq!(handoff_json["schema_version"], 3);
    assert_eq!(handoff_json["route"]["schema_version"], 3);

    let preflight_json = parse_json(
        &internal_only_workflow_preflight_output(
            repo,
            state,
            plan_rel,
            concat!(
                "rust canonical workflow pre",
                "flight should be available on ready plans"
            ),
        ),
        concat!(
            "rust canonical workflow pre",
            "flight should be available on ready plans"
        ),
    );
    assert_eq!(preflight_json["allowed"], true);

    let gate_review_json = parse_json(
        &internal_only_workflow_gate_review_output(
            repo,
            state,
            plan_rel,
            "rust canonical workflow gate review should be available on ready plans",
        ),
        "rust canonical workflow gate review should be available on ready plans",
    );
    assert_eq!(gate_review_json["allowed"], false);
    assert_eq!(gate_review_json["failure_class"], "ExecutionStateNotReady");
    assert_eq!(
        gate_review_json["reason_codes"][0],
        "unfinished_steps_remaining"
    );

    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "rust canonical workflow gate finish should be available on ready plans",
        ),
        "rust canonical workflow gate finish should be available on ready plans",
    );
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "ExecutionStateNotReady");
    assert_eq!(
        gate_finish_json["reason_codes"][0],
        "unfinished_steps_remaining"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_doctor_shares_authoritative_state_across_same_branch_worktrees()
 {
    let (repo_dir, state_dir) = init_repo("workflow-public-same-branch-worktree");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-public-same-branch-worktree";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-linked-worktree");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-public-same-branch-worktree"])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-public-same-branch-worktree",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-public-same-branch-worktree")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-linked-worktree",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);
    write_file(&decision_path, "enabled\n");

    let status_a = internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch worktree authoritative sharing fixture",
    );
    let preflight_a = parse_json(
        &internal_only_workflow_preflight_output(
            repo_a,
            state,
            plan_rel,
            concat!(
                "workflow pre",
                "flight before same-branch worktree authoritative sharing fixture"
            ),
        ),
        concat!(
            "workflow pre",
            "flight before same-branch worktree authoritative sharing fixture"
        ),
    );
    assert_eq!(preflight_a["allowed"], true);
    internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_a["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for same-branch worktree authoritative sharing fixture",
    );

    let runtime_a = discover_execution_runtime(
        repo_a,
        state,
        "workflow doctor for same-branch authoritative scope on repo A",
    );
    let runtime_b = discover_execution_runtime(
        &repo_b,
        state,
        "workflow doctor for same-branch authoritative scope on repo B",
    );
    let doctor_a = workflow_doctor_json(
        &runtime_a,
        "workflow doctor for same-branch authoritative scope on repo A",
    );
    let doctor_b = workflow_doctor_json(
        &runtime_b,
        "workflow doctor for same-branch authoritative scope on repo B",
    );

    for doctor in [&doctor_a, &doctor_b] {
        assert_eq!(doctor["phase"], "executing");
        assert_eq!(doctor["execution_status"]["execution_started"], "yes");
        assert_eq!(doctor["execution_status"]["active_task"], 1);
        assert_eq!(doctor["execution_status"]["active_step"], 1);
        let execution_run_id = doctor["execution_status"]
            .get("execution_run_id")
            .expect("same-branch worktrees should expose execution_run_id");
        assert!(
            execution_run_id
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "same-branch worktrees should expose a non-empty shared execution_run_id after execution_{} acceptance and execution start, got {execution_run_id:?}",
            concat!("pre", "flight")
        );
        assert!(
            doctor["execution_status"]["latest_authoritative_sequence"]
                .as_u64()
                .is_some(),
            "same-branch worktrees should expose numeric authoritative sequence diagnostics once execution starts"
        );
        let reason_codes = doctor["execution_status"]["reason_codes"]
            .as_array()
            .expect("same-branch worktrees should expose execution reason_codes as an array");
        if reason_codes
            .iter()
            .any(|value| value == &Value::String(String::from("write_authority_conflict")))
        {
            assert!(
                doctor["execution_status"]["write_authority_holder"].is_string(),
                "write_authority_conflict should keep authority holder metadata visible"
            );
            assert!(
                doctor["execution_status"]["write_authority_worktree"].is_string(),
                "write_authority_conflict should keep authority worktree metadata visible"
            );
        }
    }

    let run_id_a = doctor_a["execution_status"]["execution_run_id"]
        .as_str()
        .expect(concat!(
            "repo A should expose an execution_run_id after execution_pre",
            "flight acceptance"
        ));
    let run_id_b = doctor_b["execution_status"]["execution_run_id"]
        .as_str()
        .expect(concat!(
            "repo B should expose an execution_run_id after execution_pre",
            "flight acceptance"
        ));
    assert_eq!(
        run_id_a, run_id_b,
        "same-branch worktrees should share one authoritative execution run after execution starts"
    );

    assert_eq!(
        doctor_a["execution_status"]["latest_authoritative_sequence"],
        doctor_b["execution_status"]["latest_authoritative_sequence"],
        "same-branch worktrees should share authoritative sequence state"
    );

    let holder_a = &doctor_a["execution_status"]["write_authority_holder"];
    let holder_b = &doctor_b["execution_status"]["write_authority_holder"];
    let worktree_a = &doctor_a["execution_status"]["write_authority_worktree"];
    let worktree_b = &doctor_b["execution_status"]["write_authority_worktree"];

    for (field, value) in [
        ("write_authority_holder", holder_a),
        ("write_authority_holder", holder_b),
        ("write_authority_worktree", worktree_a),
        ("write_authority_worktree", worktree_b),
    ] {
        assert!(
            value.is_null() || value.as_str().is_some_and(|value| !value.is_empty()),
            "same-branch worktrees should expose {field} as null when authority diagnostics are not yet emitted, or as non-empty diagnostics once emitted, got {value:?}"
        );
    }

    assert_eq!(
        holder_a, holder_b,
        "same-branch worktrees should agree on the shared authority holder"
    );
    assert_eq!(
        worktree_a, worktree_b,
        "same-branch worktrees should agree on the authoritative worktree diagnostic"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_status_and_explain_share_started_state_across_same_branch_worktrees()
 {
    let (repo_dir, state_dir) = init_repo("workflow-same-branch-status-and-explain");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-status-and-explain");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-same-branch-status-and-explain"])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-same-branch-status-and-explain",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-same-branch-status-and-explain")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-status-and-explain",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);

    let status_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch status/explain sharing fixture",
    );
    let preflight = internal_only_unit_plan_execution_preflight_json(
        repo_a,
        state,
        plan_rel,
        concat!(
            "pre",
            "flight before same-branch status/explain sharing fixture"
        ),
    );
    assert_eq!(preflight["allowed"], true);
    internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for same-branch status/explain sharing fixture",
    );

    let status_a = internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for same-branch status/explain sharing on repo A",
    );
    let status_b = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for same-branch status/explain sharing on repo B",
    );

    for status in [&status_a, &status_b] {
        assert_eq!(status["execution_started"], "yes", "json: {status}");
        assert_eq!(status["active_task"], Value::from(1), "json: {status}");
        assert_eq!(status["active_step"], Value::from(1), "json: {status}");
    }
    assert_eq!(
        status_a["execution_run_id"], status_b["execution_run_id"],
        "same-branch worktrees should share the started execution run in plan execution status"
    );
    assert_eq!(
        status_a["phase_detail"], status_b["phase_detail"],
        "same-branch worktrees should agree on phase detail in plan execution status"
    );

    let explain_a = internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "plan execution explain",
            "-review-state for same-branch sharing on repo A"
        ),
    );
    let explain_b = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "plan execution explain",
            "-review-state for same-branch sharing on repo B"
        ),
    );

    assert_eq!(
        explain_a["next_action"],
        explain_b["next_action"],
        "same-branch worktrees should agree on {} next_action",
        concat!("explain", "-review-state")
    );
    assert_eq!(
        explain_a["recommended_command"],
        explain_b["recommended_command"],
        "same-branch worktrees should agree on {} recommended_command",
        concat!("explain", "-review-state")
    );
    assert_eq!(
        explain_a["trace_summary"],
        explain_b["trace_summary"],
        "same-branch worktrees should agree on {} trace_summary",
        concat!("explain", "-review-state")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_repair_and_reconcile_share_started_state_across_same_branch_worktrees()
 {
    let (repo_dir, state_dir) = init_repo("workflow-same-branch-repair-and-reconcile");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-repair-and-reconcile");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args([
            "checkout",
            "-B",
            "workflow-same-branch-repair-and-reconcile",
        ])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-same-branch-repair-and-reconcile",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-same-branch-repair-and-reconcile")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-repair-and-reconcile",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);

    let status_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch repair/reconcile fixture",
    );
    let preflight = internal_only_unit_plan_execution_preflight_json(
        repo_a,
        state,
        plan_rel,
        concat!("pre", "flight before same-branch repair/reconcile fixture"),
    );
    assert_eq!(preflight["allowed"], true);
    internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for same-branch repair/reconcile fixture",
    );

    let status_b_after_begin = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status after begin for same-branch repair/reconcile fixture on repo B",
    );
    assert_eq!(status_b_after_begin["execution_started"], "yes");

    seed_current_branch_closure_truth(repo_a, state, plan_rel, 1);
    let branch = current_branch_name(repo_a);
    update_authoritative_harness_state(
        repo_a,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let reconcile_b = internal_only_unit_reconcile_review_state_json(
        &repo_b,
        state,
        plan_rel,
        concat!(
            "reconcile",
            "-review-state from same-branch non-authoritative worktree"
        ),
    );
    assert_eq!(
        reconcile_b["action"],
        "reconciled",
        "{} from a same-branch non-authoritative worktree should restore missing derived overlays when recoverable",
        concat!("reconcile", "-review-state"),
    );
    assert!(
        reconcile_b["actions_performed"]
            .as_array()
            .is_some_and(|actions| !actions.is_empty()),
        "{} should report restored overlay actions",
        concat!("reconcile", "-review-state"),
    );

    let authoritative_state_path = harness_state_path(state, &repo_slug(repo_a), &branch);
    let reconciled_state = reduced_authoritative_harness_state_for_path(&authoritative_state_path)
        .unwrap_or_else(|| {
            serde_json::from_str(
                &fs::read_to_string(&authoritative_state_path)
                    .expect("authoritative state should be readable after reconcile"),
            )
            .expect("authoritative state should remain valid json after reconcile")
        });
    assert!(
        reconciled_state["current_branch_closure_reviewed_state_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "{} should restore current_branch_closure_reviewed_state_id via authoritative records",
        concat!("reconcile", "-review-state")
    );
    assert!(
        reconciled_state["current_branch_closure_contract_identity"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "{} should restore current_branch_closure_contract_identity via authoritative records",
        concat!("reconcile", "-review-state")
    );

    let mut malformed_state = reconciled_state;
    malformed_state["current_branch_closure_reviewed_state_id"] =
        Value::from("git_tree:not-a-tree");
    if let Some(record) = malformed_state
        .get_mut("branch_closure_records")
        .and_then(Value::as_object_mut)
        .and_then(|records| records.get_mut("branch-release-closure"))
        .and_then(Value::as_object_mut)
    {
        record.insert(
            String::from("reviewed_state_id"),
            Value::from("git_tree:not-a-tree"),
        );
    } else {
        panic!(
            "same-branch repair fixture should include branch-release-closure in authoritative branch_closure_records"
        );
    }
    write_file(
        &authoritative_state_path,
        &serde_json::to_string(&malformed_state)
            .expect("malformed authoritative state fixture should serialize"),
    );

    let repair_b = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state from same-branch non-authoritative worktree",
    );
    let action = repair_b["action"]
        .as_str()
        .expect("repair-review-state should expose action");
    let repaired_state = reduced_authoritative_harness_state_for_path(&authoritative_state_path)
        .unwrap_or_else(|| {
            serde_json::from_str(
                &fs::read_to_string(&authoritative_state_path)
                    .expect("authoritative state should be readable after repair"),
            )
            .expect("authoritative state should remain valid json after repair")
        });
    if action == "blocked" {
        let required_follow_up = repair_b["required_follow_up"]
            .as_str()
            .expect("blocked repair-review-state should emit required_follow_up");
        let operator_b = parse_json(
            &run_rust_featureforge_with_env(
                &repo_b,
                state,
                &["workflow", "operator", "--plan", plan_rel, "--json"],
                &[],
                "same-branch workflow/operator route after blocked repair",
            ),
            "same-branch workflow/operator route after blocked repair",
        );
        assert_eq!(
            operator_b["required_follow_up"],
            Value::from(required_follow_up),
            "blocked repair-review-state should persist the exact follow-up route surfaced by workflow/operator"
        );
        let persisted_follow_up = if required_follow_up == "advance_late_stage" {
            "record_branch_closure"
        } else {
            required_follow_up
        };
        assert_eq!(
            repaired_state["review_state_repair_follow_up"],
            Value::from(persisted_follow_up),
            "blocked repair-review-state from a same-branch non-authoritative worktree should persist the authoritative follow-up reroute",
        );
    } else if action == "reconciled" {
        assert_eq!(
            repair_b["required_follow_up"],
            Value::Null,
            "reconciled repair-review-state should not persist a follow-up reroute when reconciliation succeeds",
        );
        assert!(
            repair_b["actions_performed"]
                .as_array()
                .is_some_and(|actions| !actions.is_empty()),
            "reconciled repair-review-state should report restored authoritative fields",
        );
        assert_eq!(
            repaired_state["review_state_repair_follow_up"],
            Value::Null,
            "reconciled repair-review-state should clear stale persisted follow-up reroutes",
        );
    } else if action == "already_current" {
        assert_eq!(
            repair_b["required_follow_up"],
            Value::Null,
            "already_current repair-review-state should not emit a follow-up reroute",
        );
        assert!(
            repair_b["actions_performed"]
                .as_array()
                .is_some_and(|actions| actions.is_empty()),
            "already_current repair-review-state should not report reconciliation rewrites",
        );
    } else {
        panic!(
            "repair-review-state should either fail closed, reconcile, or report already_current in same-branch overlay recovery; got {repair_b}"
        );
    }
}

#[test]
fn internal_only_compatibility_plan_execution_status_and_explain_do_not_share_started_state_from_detached_worktree()
 {
    let (repo_dir, state_dir) = init_repo("workflow-same-branch-detached-status-and-explain");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-detached-status-and-explain");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args([
            "checkout",
            "-B",
            "workflow-same-branch-detached-status-and-explain",
        ])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-same-branch-detached-status-and-explain",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-same-branch-detached-status-and-explain")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-detached-status-and-explain",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);

    let status_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before detached same-branch status/explain sharing fixture",
    );
    let preflight = internal_only_unit_plan_execution_preflight_json(
        repo_a,
        state,
        plan_rel,
        concat!(
            "pre",
            "flight before detached same-branch status/explain sharing fixture"
        ),
    );
    assert_eq!(preflight["allowed"], true);
    internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for detached same-branch status/explain sharing fixture",
    );

    let mut git_detach = Command::new("git");
    git_detach
        .args(["checkout", "--detach", "HEAD"])
        .current_dir(&repo_b);
    run_checked(
        git_detach,
        "git checkout --detach HEAD for same-branch detached sharing fixture",
    );

    let status_a = internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for detached same-branch sharing on repo A",
    );
    let status_b = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for detached same-branch sharing on repo B",
    );

    assert_eq!(status_a["execution_started"], "yes", "json: {status_a}");
    assert_eq!(status_a["active_task"], Value::from(1), "json: {status_a}");
    assert_eq!(status_a["active_step"], Value::from(1), "json: {status_a}");
    assert_eq!(status_b["execution_started"], "no", "json: {status_b}");
    assert_eq!(status_b["active_task"], Value::Null, "json: {status_b}");
    assert_eq!(status_b["active_step"], Value::Null, "json: {status_b}");
    assert_eq!(
        status_b["execution_run_id"],
        Value::Null,
        "detached worktrees must fail closed instead of borrowing another branch's started execution run"
    );

    let explain_a = internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "plan execution explain",
            "-review-state for detached same-branch sharing on repo A"
        ),
    );
    let explain_b = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "plan execution explain",
            "-review-state for detached same-branch sharing on repo B"
        ),
    );
    assert_ne!(
        explain_a["recommended_command"],
        explain_b["recommended_command"],
        "detached worktrees must not borrow another branch's {} recommended_command",
        concat!("explain", "-review-state")
    );
}

#[test]
fn internal_only_compatibility_same_branch_worktrees_do_not_adopt_started_state_when_execution_fingerprint_differs()
 {
    let (repo_dir, state_dir) = init_repo("workflow-same-branch-fingerprint-guard");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-fingerprint-guard");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let evidence_rel = derive_evidence_rel_path(plan_rel, 1);

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-same-branch-fingerprint-guard"])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-same-branch-fingerprint-guard",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-same-branch-fingerprint-guard")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-fingerprint-guard",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);
    let repo_b_evidence_path = repo_b.join(&evidence_rel);
    if let Some(parent) = repo_b_evidence_path.parent() {
        fs::create_dir_all(parent).expect("evidence fixture parent should be creatable");
    }
    fs::write(&repo_b_evidence_path, "### Task 1 Step 1\n")
        .expect("repo B evidence divergence should write");

    let status_a_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch fingerprint guard fixture",
    );
    let status_b_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch fingerprint guard fixture on repo B",
    );
    let explain_b_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "plan execution explain",
            "-review-state before same-branch fingerprint guard fixture on repo B"
        ),
    );

    assert_ne!(
        status_a_before_begin["execution_fingerprint"],
        status_b_before_begin["execution_fingerprint"],
        "repo-local evidence divergence should produce a distinct execution fingerprint before same-branch adoption is considered"
    );
    assert_eq!(status_b_before_begin["execution_started"], "no");

    let preflight = internal_only_unit_plan_execution_preflight_json(
        repo_a,
        state,
        plan_rel,
        concat!("pre", "flight before same-branch fingerprint guard fixture"),
    );
    assert_eq!(preflight["allowed"], true);
    internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_a_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for same-branch fingerprint guard fixture",
    );

    let status_b_after_begin = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status after same-branch fingerprint guard fixture on repo B",
    );
    let explain_b_after_begin = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "plan execution explain",
            "-review-state after same-branch fingerprint guard fixture on repo B"
        ),
    );

    assert_eq!(status_b_after_begin["execution_started"], "yes");
    assert_eq!(
        status_b_after_begin["active_task"], 1,
        "tracked execution-evidence exports are not routing authority, so repo B can share repo A's same-branch started state"
    );
    assert_eq!(
        status_b_after_begin["active_step"], 1,
        "tracked execution-evidence exports are not routing authority, so repo B can share repo A's same-branch started step"
    );
    assert_ne!(
        explain_b_before_begin["recommended_command"], explain_b_after_begin["recommended_command"],
        "same-branch adoption should update the execution command once only tracked projection exports differ"
    );
}

#[test]
fn internal_only_compatibility_same_branch_worktrees_do_not_adopt_started_state_when_tracked_workspace_differs()
 {
    let (repo_dir, state_dir) = init_repo("workflow-same-branch-workspace-guard");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-workspace-guard");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-same-branch-workspace-guard"])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-same-branch-workspace-guard",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-same-branch-workspace-guard")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-workspace-guard",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);
    let repo_b_readme_path = repo_b.join("README.md");
    let repo_b_readme = fs::read_to_string(&repo_b_readme_path)
        .expect("repo B README should be readable before tracked workspace guard");
    fs::write(
        &repo_b_readme_path,
        format!("{repo_b_readme}tracked workspace divergence before same-branch adoption\n"),
    )
    .expect("repo B README should accept tracked workspace divergence");

    let status_a_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch tracked workspace guard fixture",
    );
    let status_b_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch tracked workspace guard fixture on repo B",
    );
    let operator_b_before_begin = parse_json(
        &run_rust_featureforge_with_env(
            &repo_b,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "workflow operator before same-branch tracked workspace guard fixture on repo B",
        ),
        "workflow operator before same-branch tracked workspace guard fixture on repo B",
    );
    let explain_b_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state before same-branch tracked workspace guard fixture on repo B"
        ),
    );

    assert_ne!(
        status_a_before_begin["semantic_workspace_tree_id"],
        status_b_before_begin["semantic_workspace_tree_id"],
        "tracked workspace divergence should produce a distinct semantic_workspace_tree_id before same-branch adoption is considered"
    );
    assert_eq!(status_b_before_begin["execution_started"], "no");

    let preflight = internal_only_unit_plan_execution_preflight_json(
        repo_a,
        state,
        plan_rel,
        concat!(
            "pre",
            "flight before same-branch tracked workspace guard fixture"
        ),
    );
    assert_eq!(preflight["allowed"], true);
    internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_a_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for same-branch tracked workspace guard fixture",
    );

    let status_b_after_begin = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status after same-branch tracked workspace guard fixture on repo B",
    );
    let operator_b_after_begin = parse_json(
        &run_rust_featureforge_with_env(
            &repo_b,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "workflow operator after same-branch tracked workspace guard fixture on repo B",
        ),
        "workflow operator after same-branch tracked workspace guard fixture on repo B",
    );
    let explain_b_after_begin = internal_only_run_plan_execution_json_direct_or_cli(
        &repo_b,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state after same-branch tracked workspace guard fixture on repo B"
        ),
    );

    assert_eq!(status_b_after_begin["execution_started"], "no");
    assert_eq!(
        status_b_before_begin["semantic_workspace_tree_id"],
        status_b_after_begin["semantic_workspace_tree_id"],
        "repo B should preserve its own semantic_workspace_tree_id when tracked workspace divergence blocks same-branch adoption"
    );
    assert!(
        status_b_after_begin["active_task"].is_null(),
        "repo B should not borrow repo A's active task when tracked workspace state differs"
    );
    assert!(
        status_b_after_begin["active_step"].is_null(),
        "repo B should not borrow repo A's active step when tracked workspace state differs"
    );
    assert!(
        operator_b_after_begin["phase"]
            .as_str()
            .is_some_and(|phase| {
                phase == "implementation_handoff" || phase == concat!("execution_pre", "flight")
            }),
        "repo B workflow operator phase should remain non-executing when tracked workspace state differs: {operator_b_after_begin:?}"
    );
    assert_eq!(
        operator_b_before_begin["next_action"], operator_b_after_begin["next_action"],
        "repo B workflow operator next_action should remain local when tracked workspace state differs"
    );
    assert_eq!(
        operator_b_before_begin["recommended_command"],
        operator_b_after_begin["recommended_command"],
        "repo B workflow operator recommended_command should remain local when tracked workspace state differs"
    );
    assert_eq!(
        explain_b_before_begin["next_action"],
        explain_b_after_begin["next_action"],
        "repo B {} next_action should remain local when tracked workspace state differs",
        concat!("explain", "-review-state")
    );
    assert_eq!(
        explain_b_before_begin["recommended_command"],
        explain_b_after_begin["recommended_command"],
        "repo B {} recommended_command should remain local when tracked workspace state differs",
        concat!("explain", "-review-state")
    );
    assert_eq!(
        explain_b_before_begin["trace_summary"],
        explain_b_after_begin["trace_summary"],
        "repo B {} trace_summary should remain local when tracked workspace state differs",
        concat!("explain", "-review-state")
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_doctor_does_not_adopt_started_status_across_different_branch_worktrees()
 {
    let (repo_dir, state_dir) = init_repo("workflow-public-cross-branch-worktree");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-public-cross-branch-worktree";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("cross-branch-linked-worktree");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-public-cross-branch-worktree-a"])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-public-cross-branch-worktree-a",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg("-b")
        .arg("workflow-public-cross-branch-worktree-b")
        .arg(&repo_b)
        .arg("HEAD")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force -b workflow-public-cross-branch-worktree-b cross-branch-linked-worktree HEAD",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);
    write_file(&decision_path, "enabled\n");

    let status_a = internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before cross-branch worktree sharing fixture",
    );
    let preflight_a = parse_json(
        &internal_only_workflow_preflight_output(
            repo_a,
            state,
            plan_rel,
            concat!(
                "workflow pre",
                "flight before cross-branch worktree sharing fixture"
            ),
        ),
        concat!(
            "workflow pre",
            "flight before cross-branch worktree sharing fixture"
        ),
    );
    assert_eq!(preflight_a["allowed"], true);
    internal_only_run_plan_execution_json_direct_or_cli(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_a["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for cross-branch worktree sharing fixture",
    );

    let runtime_a = discover_execution_runtime(
        repo_a,
        state,
        "workflow doctor for cross-branch scope on repo A",
    );
    let runtime_b = discover_execution_runtime(
        &repo_b,
        state,
        "workflow doctor for cross-branch scope on repo B",
    );
    let doctor_a = workflow_doctor_json(
        &runtime_a,
        "workflow doctor for cross-branch scope on repo A",
    );
    let doctor_b = workflow_doctor_json(
        &runtime_b,
        "workflow doctor for cross-branch scope on repo B",
    );

    assert_eq!(doctor_a["phase"], "executing");
    assert_eq!(doctor_a["execution_status"]["execution_started"], "yes");
    assert_eq!(doctor_a["execution_status"]["active_task"], 1);
    assert_eq!(doctor_a["execution_status"]["active_step"], 1);

    assert_ne!(
        doctor_b["phase"], "executing",
        "cross-branch worktrees must not inherit started execution routing from another branch"
    );
    assert_eq!(
        doctor_b["execution_status"]["execution_started"], "no",
        "cross-branch worktrees must not inherit started execution status from another branch"
    );
    assert!(
        doctor_b["execution_status"]["active_task"].is_null(),
        "cross-branch worktrees should not expose an active task when local execution has not started"
    );
    assert!(
        doctor_b["execution_status"]["active_step"].is_null(),
        "cross-branch worktrees should not expose an active step when local execution has not started"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_routes_started_execution_back_to_the_current_execution_flow()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-started-execution");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-started-execution";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");
    prepare_preflight_acceptance_workspace(repo, "workflow-phase-started-execution");

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before started-execution routing fixture",
    );
    let preflight_json = internal_only_unit_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        concat!(
            "plan execution pre",
            "flight before started-execution routing fixture"
        ),
    );
    assert_eq!(preflight_json["allowed"], true);
    internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_json["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for started-execution routing fixture",
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow phase for started-execution routing fixture",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow phase for started-execution routing fixture",
    );
    assert_eq!(phase_json["phase"], "handoff_required");
    assert_eq!(phase_json["next_action"], "continue execution");

    let doctor_json = workflow_doctor_json(
        &runtime,
        "workflow doctor for started-execution routing fixture",
    );
    assert_eq!(doctor_json["phase"], "executing");
    assert_eq!(doctor_json["execution_status"]["execution_started"], "yes");
    assert_eq!(doctor_json["execution_status"]["active_task"], 1);
    assert_eq!(doctor_json["execution_status"]["active_step"], 1);
    assert_eq!(doctor_json[concat!("pre", "flight")], Value::Null);
    assert_eq!(doctor_json["gate_review"], Value::Null);
    assert_eq!(doctor_json["gate_finish"], Value::Null);

    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for started-execution routing fixture",
    );
    assert_eq!(handoff_json["phase"], "handoff_required");
    assert_eq!(handoff_json["execution_started"], "yes");
    assert_eq!(handoff_json["next_action"], "continue execution");
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:executing-plans"
    );
    assert_eq!(
        handoff_json["recommendation_reason"],
        "Execution already started for the approved plan revision; continue with the current execution flow."
    );
}

#[test]
fn internal_only_compatibility_workflow_phase_routes_task_boundary_blocked() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-task-boundary-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-task-boundary-blocked";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";

    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join(plan_rel),
        &format!(
            r#"# Runtime Integration Hardening Implementation Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `{spec_rel}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-004 -> Task 1
- VERIFY-001 -> Task 2

## Execution Strategy

- Execute Task 1 serially. It establishes boundary gating before follow-on work begins.
- Execute Task 2 serially after Task 1. It validates task-boundary workflow routing.

## Dependency Diagram

```text
Task 1 -> Task 2
```

## Task 1: Core flow

**Spec Coverage:** REQ-001, REQ-004
**Goal:** Task 1 execution reaches a boundary gate before Task 2 starts.

**Context:**
- Spec Coverage: REQ-001, REQ-004.

**Constraints:**
- Keep fixture inputs deterministic.

**Done when:**
- Task 1 execution reaches a boundary gate before Task 2 starts.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Prepare workflow fixture output**
- [ ] **Step 2: Validate workflow fixture output**

## Task 2: Follow-on flow

**Spec Coverage:** VERIFY-001
**Goal:** Task 2 should remain blocked until Task 1 closure requirements are met.

**Context:**
- Spec Coverage: VERIFY-001.

**Constraints:**
- Preserve deterministic task-boundary diagnostics.

**Done when:**
- Task 2 should remain blocked until Task 1 closure requirements are met.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Start the follow-on task**
"#
        ),
    );
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);

    enable_session_decision(state, session_key);
    prepare_preflight_acceptance_workspace(repo, "workflow-phase-task-boundary-blocked");

    let status_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before task-boundary blocked workflow fixture execution",
    );
    let preflight = internal_only_unit_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        concat!(
            "pre",
            "flight for task-boundary blocked workflow fixture execution"
        ),
    );
    assert_eq!(preflight["allowed"], true);
    let begin_task1_step1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint before begin"),
        ],
        "begin task 1 step 1 for task-boundary blocked workflow fixture",
    );
    let complete_task1_step1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 1 for task-boundary blocked workflow fixture.",
            "--manual-verify-summary",
            "Verified by workflow task-boundary fixture setup.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 1 for task-boundary blocked workflow fixture",
    );
    let begin_task1_step2 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            complete_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("complete should expose execution fingerprint for next begin"),
        ],
        "begin task 1 step 2 for task-boundary blocked workflow fixture",
    );
    internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 2 for task-boundary blocked workflow fixture.",
            "--manual-verify-summary",
            "Verified by workflow task-boundary fixture setup.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 2 for task-boundary blocked workflow fixture",
    );
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(repo, state, &branch, plan_rel, 1, &[]);
    internal_only_runtime_review_dispatch_authority_json(
        repo,
        state,
        plan_rel,
        ReviewDispatchScopeArg::Task,
        Some(1),
        "record task-boundary review dispatch for blocked workflow fixture",
    );
    let mismatch_dispatch_output = internal_only_unit_plan_execution_output(
        plan_execution_direct_support::internal_only_runtime_review_dispatch_authority_json(
            repo,
            state,
            &RecordReviewDispatchArgs {
                plan: plan_rel.into(),
                scope: ReviewDispatchScopeArg::Task,
                task: Some(2),
            },
        ),
    );
    assert!(
        !mismatch_dispatch_output.status.success(),
        "task-boundary blocked fixture should fail closed for non-blocking redispatch target\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&mismatch_dispatch_output.stdout),
        String::from_utf8_lossy(&mismatch_dispatch_output.stderr)
    );
    let mismatch_dispatch_payload = if mismatch_dispatch_output.stdout.is_empty() {
        mismatch_dispatch_output.stderr.as_slice()
    } else {
        mismatch_dispatch_output.stdout.as_slice()
    };
    let mismatch_dispatch_failure: Value = serde_json::from_slice(mismatch_dispatch_payload)
        .expect("task-boundary blocked mismatch redispatch should emit json failure payload");
    assert_eq!(
        mismatch_dispatch_failure["error_class"],
        Value::from("InvalidCommandInput"),
        "task-boundary blocked mismatch redispatch should fail with InvalidCommandInput, got {mismatch_dispatch_failure:?}"
    );
    assert!(
        mismatch_dispatch_failure["message"]
            .as_str()
            .is_some_and(|message| {
                message.contains("does not match the current task review-dispatch target")
            }),
        "task-boundary blocked mismatch redispatch should explain the current dispatch target contract, got {mismatch_dispatch_failure:?}"
    );

    let execution_status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "execution status after task 1 completion for task-boundary blocked workflow fixture",
    );
    assert_eq!(execution_status["active_task"], Value::Null);
    assert_eq!(execution_status["blocking_task"], Value::from(1));
    assert_eq!(execution_status["blocking_step"], Value::Null);
    assert!(
        execution_status["reason_codes"]
            .as_array()
            .is_some_and(|codes| {
                codes
                    .iter()
                    .any(|code| code.as_str() == Some("prior_task_current_closure_missing"))
            }),
        "execution status should surface prior_task_current_closure_missing for task-boundary blocked fixture, got {execution_status:?}"
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow phase for task-boundary blocked routing fixture",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow phase for task-boundary blocked routing fixture",
    );
    let doctor_json = workflow_doctor_json(
        &runtime,
        "workflow doctor for task-boundary blocked routing fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for task-boundary blocked routing fixture",
    );

    let expected_phase = public_harness_phase_from_spec("task_closure_pending");
    assert_eq!(
        phase_json["phase"], expected_phase,
        "task-boundary blocked phase fixture should route to task_closure_pending; phase payload: {phase_json:?}"
    );
    assert_eq!(phase_json["next_action"], "close current task");
    assert_eq!(doctor_json["phase"], expected_phase);
    assert_eq!(doctor_json["next_action"], "close current task");
    assert_eq!(handoff_json["phase"], expected_phase);
    assert_eq!(handoff_json["next_action"], "close current task");
    assert_eq!(handoff_json["state_kind"], "actionable_public_command");
    assert!(
        handoff_json["next_public_action"].is_null()
            || handoff_json["next_public_action"]["command"].is_null(),
        "task-closure handoff should not expose a command until required inputs are supplied: {handoff_json:?}"
    );
    assert!(
        handoff_json["recommended_command"].is_null(),
        "task-closure handoff should not expose a placeholder command: {handoff_json:?}"
    );
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:executing-plans"
    );
    assert!(
        handoff_json["recommendation_reason"]
            .as_str()
            .is_some_and(|reason| {
                reason.contains("Task 1 closure is ready to record/refresh")
                    && reason.contains("Record or refresh Task 1 closure now")
            }),
        "workflow handoff should surface task-boundary closure-recording guidance, got {handoff_json:?}"
    );
    assert!(
        doctor_json["execution_status"]["reason_codes"]
            .as_array()
            .is_some_and(|codes| {
                codes
                    .iter()
                    .any(|code| code.as_str() == Some("prior_task_current_closure_missing"))
            }),
        "workflow doctor should preserve execution reason-code parity for task-boundary blocks, got {doctor_json:?}"
    );
}

#[test]
fn internal_only_compatibility_workflow_handoff_prefers_shared_task_closure_route_over_forged_dispatch_reason_code()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-task-boundary-dispatch-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-task-boundary-dispatch-blocked";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";

    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join(plan_rel),
        &format!(
            r#"# Runtime Integration Hardening Implementation Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `{spec_rel}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-004 -> Task 1
- VERIFY-001 -> Task 2

## Execution Strategy

- Execute Task 1 serially. It establishes boundary gating before follow-on work begins.
- Execute Task 2 serially after Task 1. It validates task-boundary workflow routing.

## Dependency Diagram

```text
Task 1 -> Task 2
```

## Task 1: Core flow

**Spec Coverage:** REQ-001, REQ-004
**Goal:** Task 1 reaches task-boundary closure gate before Task 2 starts.

**Context:**
- Spec Coverage: REQ-001, REQ-004.

**Constraints:**
- Keep fixture input deterministic.

**Done when:**
- Task 1 reaches task-boundary closure gate before Task 2 starts.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Prepare workflow fixture output**

## Task 2: Follow-on flow

**Spec Coverage:** VERIFY-001
**Goal:** Task 2 remains blocked until task-boundary closure is satisfied.

**Context:**
- Spec Coverage: VERIFY-001.

**Constraints:**
- Preserve task-boundary diagnostics.

**Done when:**
- Task 2 remains blocked until task-boundary closure is satisfied.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Start the follow-on task**
"#
        ),
    );
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);

    enable_session_decision(state, session_key);
    prepare_preflight_acceptance_workspace(repo, "workflow-phase-task-boundary-dispatch-blocked");

    let status_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before begin for workflow dispatch-blocked fixture",
    );
    let preflight = internal_only_unit_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        concat!("pre", "flight for workflow dispatch-blocked fixture"),
    );
    assert_eq!(preflight["allowed"], true);

    let begin_task1_step1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint before begin"),
        ],
        "begin task 1 step 1 for workflow dispatch-blocked fixture",
    );
    internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 1 for workflow dispatch-blocked fixture.",
            "--manual-verify-summary",
            "Verified by workflow dispatch-blocked fixture setup.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 1 for workflow dispatch-blocked fixture",
    );

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("blocking_task", Value::from(1)),
            ("blocking_step", Value::Null),
            (
                "reason_codes",
                json!(["prior_task_review_dispatch_missing"]),
            ),
        ],
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow phase for task-boundary dispatch-blocked fixture",
    );
    let doctor_json = workflow_doctor_json(
        &runtime,
        "workflow doctor for task-boundary dispatch-blocked fixture",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow phase for task-boundary dispatch-blocked fixture",
    );
    let expected_operator_follow_up = "Task 1 closure is ready to record/refresh";
    assert_eq!(
        phase_json["next_action"],
        Value::from("close current task"),
        "workflow phase json should follow the shared task-closure route even when harness reason codes are forged, got {phase_json:?}"
    );
    assert!(
        phase_json["recommended_command"].is_null(),
        "workflow phase json should not expose placeholder close-current-task command text, got {phase_json:?}"
    );
    assert!(
        phase_json["next_step"]
            .as_str()
            .is_some_and(|next_step| next_step.contains(expected_operator_follow_up)),
        "workflow phase json should include task-closure recording guidance from the shared routing engine, got {phase_json:?}"
    );
    assert_eq!(
        doctor_json["next_action"],
        Value::from("close current task"),
        "workflow doctor should follow the shared task-closure route even when harness reason codes are forged, got {doctor_json:?}"
    );
    assert_task_closure_required_inputs(&doctor_json, 1);
    assert!(
        doctor_json["next_step"]
            .as_str()
            .is_some_and(|next_step| next_step.contains(expected_operator_follow_up)),
        "workflow doctor should include task-closure recording guidance from the shared routing engine, got {doctor_json:?}"
    );

    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for task-boundary dispatch-blocked fixture",
    );
    assert!(
        handoff_json["recommendation_reason"]
            .as_str()
            .is_some_and(|reason| reason.contains(expected_operator_follow_up)),
        "workflow handoff should include task-review dispatch guidance for dispatch-blocked routing, got {handoff_json:?}"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_routes_blocked_preflight_back_to_execution_handoff()
 {
    let (repo_dir, state_dir) = init_repo(concat!("workflow-phase-blocked-pre", "flight"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = concat!("workflow-phase-blocked-pre", "flight");
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");
    write_file(&repo.join(".git/MERGE_HEAD"), "deadbeef\n");

    let runtime = discover_execution_runtime(
        repo,
        state,
        concat!("workflow phase for blocked-pre", "flight routing fixture"),
    );
    let phase_json = workflow_phase_json(
        &runtime,
        concat!("workflow phase for blocked-pre", "flight routing fixture"),
    );
    assert_eq!(phase_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(
        phase_json["next_action"],
        concat!("execution pre", "flight")
    );

    let doctor_json = workflow_doctor_json(
        &runtime,
        concat!("workflow doctor for blocked-pre", "flight routing fixture"),
    );
    assert_eq!(doctor_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(doctor_json["execution_status"]["execution_started"], "no");
    assert_eq!(doctor_json[concat!("pre", "flight")]["allowed"], false);
    assert_eq!(
        doctor_json[concat!("pre", "flight")]["failure_class"],
        "WorkspaceNotSafe"
    );
    assert!(
        doctor_json[concat!("pre", "flight")]["reason_codes"]
            .as_array()
            .expect("reason_codes should stay an array")
            .iter()
            .any(|value| value == &Value::String(String::from("merge_in_progress")))
    );
    assert_eq!(doctor_json["gate_review"], Value::Null);
    assert_eq!(doctor_json["gate_finish"], Value::Null);

    let handoff_json = workflow_handoff_json(
        &runtime,
        concat!("workflow handoff for blocked-pre", "flight routing fixture"),
    );
    assert_eq!(handoff_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(handoff_json["execution_started"], "no");
    assert_eq!(
        handoff_json["next_action"],
        concat!("execution pre", "flight")
    );
    assert_eq!(handoff_json["recommended_skill"], "");
    assert_eq!(handoff_json["recommendation"], Value::Null);
    assert_eq!(handoff_json["recommendation_reason"], "");
}

#[test]
fn internal_only_compatibility_canonical_workflow_routes_dirty_worktree_back_to_execution_handoff()
{
    let (repo_dir, state_dir) = init_repo(concat!("workflow-phase-dirty-pre", "flight"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = concat!("workflow-phase-dirty-pre", "flight");
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");
    write_file(
        &repo.join("README.md"),
        concat!(
            "# workflow-phase-dirty-pre",
            "flight\ntracked change before execution\n"
        ),
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        concat!(
            "workflow phase for dirty-worktree pre",
            "flight routing fixture"
        ),
    );
    let phase_json = workflow_phase_json(
        &runtime,
        concat!(
            "workflow phase for dirty-worktree pre",
            "flight routing fixture"
        ),
    );
    assert_eq!(phase_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(
        phase_json["next_action"],
        concat!("execution pre", "flight")
    );

    let doctor_json = workflow_doctor_json(
        &runtime,
        concat!(
            "workflow doctor for dirty-worktree pre",
            "flight routing fixture"
        ),
    );
    assert_eq!(doctor_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(doctor_json["execution_status"]["execution_started"], "no");
    assert_eq!(doctor_json[concat!("pre", "flight")]["allowed"], false);
    assert_eq!(
        doctor_json[concat!("pre", "flight")]["failure_class"],
        "WorkspaceNotSafe"
    );
    assert!(
        doctor_json[concat!("pre", "flight")]["reason_codes"]
            .as_array()
            .expect("reason_codes should stay an array")
            .iter()
            .any(|value| value == &Value::String(String::from("tracked_worktree_dirty")))
    );
    assert_eq!(doctor_json["gate_review"], Value::Null);
    assert_eq!(doctor_json["gate_finish"], Value::Null);
}

#[test]
fn internal_only_compatibility_canonical_workflow_routes_accepted_preflight_from_harness_state_even_when_workspace_becomes_dirty()
 {
    let (repo_dir, state_dir) = init_repo(concat!("workflow-phase-accepted-pre", "flight-dirty"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = concat!("workflow-phase-accepted-pre", "flight-dirty");
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");
    prepare_preflight_acceptance_workspace(
        repo,
        concat!("workflow-phase-accepted-pre", "flight-dirty"),
    );

    let preflight_json = internal_only_unit_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        concat!(
            "explicit plan execution pre",
            "flight acceptance before dirty workspace routing fixture"
        ),
    );
    assert_eq!(preflight_json["allowed"], true);

    let status_after_preflight = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!(
            "status after explicit plan execution pre",
            "flight acceptance before dirty workspace routing fixture"
        ),
    );
    assert!(
        status_after_preflight["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "explicit plan execution {} should persist execution_run_id before workspace becomes dirty",
        concat!("pre", "flight")
    );

    write_file(
        &repo.join("README.md"),
        concat!(
            "# workflow-phase-accepted-pre",
            "flight-dirty\ntracked change after execution pre",
            "flight acceptance\n"
        ),
    );

    let status_after_workspace_dirty = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!(
            "status after workspace dirties following explicit plan execution pre",
            "flight acceptance"
        ),
    );
    assert!(
        status_after_workspace_dirty["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "accepted {} should keep plan execution status.execution_run_id non-empty after workspace dirties",
        concat!("pre", "flight")
    );
    assert_eq!(
        status_after_workspace_dirty["harness_phase"],
        concat!("execution_pre", "flight"),
        "accepted {} should keep plan execution status.harness_phase at execution_{} after workspace dirties",
        concat!("pre", "flight"),
        concat!("pre", "flight")
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        concat!(
            "workflow phase for accepted-pre",
            "flight dirty-workspace routing fixture"
        ),
    );
    let phase_json = workflow_phase_json(
        &runtime,
        concat!(
            "workflow phase for accepted-pre",
            "flight dirty-workspace routing fixture"
        ),
    );
    assert_eq!(phase_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(
        phase_json["next_action"],
        concat!("execution pre", "flight")
    );

    let handoff_json = workflow_handoff_json(
        &runtime,
        concat!(
            "workflow handoff for accepted-pre",
            "flight dirty-workspace routing fixture"
        ),
    );
    assert_eq!(handoff_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(
        handoff_json["next_action"],
        concat!("execution pre", "flight")
    );

    let doctor_json = workflow_doctor_json(
        &runtime,
        concat!(
            "workflow doctor for accepted-pre",
            "flight dirty-workspace routing fixture"
        ),
    );
    assert!(
        doctor_json["execution_status"]["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "accepted {} should keep doctor.execution_status.execution_run_id non-empty after workspace dirties",
        concat!("pre", "flight")
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_doctor_uses_accepted_preflight_truth_after_workspace_dirties()
 {
    let (repo_dir, state_dir) = init_repo(concat!("workflow-doctor-accepted-pre", "flight-dirty"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = concat!("workflow-doctor-accepted-pre", "flight-dirty");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    install_full_contract_ready_artifacts(repo);
    enable_session_decision(state, session_key);
    prepare_preflight_acceptance_workspace(
        repo,
        concat!("workflow-doctor-accepted-pre", "flight-dirty"),
    );

    let preflight_json = internal_only_unit_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        concat!(
            "explicit plan execution pre",
            "flight acceptance before doctor dirty-workspace fixture"
        ),
    );
    assert_eq!(preflight_json["allowed"], true);

    write_file(
        &repo.join("README.md"),
        concat!(
            "# workflow-doctor-accepted-pre",
            "flight-dirty\ntracked change after execution pre",
            "flight acceptance\n"
        ),
    );
    assert!(
        discover_repository(repo)
            .expect("workspace dirtiness helper should discover repository")
            .is_dirty()
            .expect("workspace dirtiness helper should compute dirtiness"),
        "workspace should be dirty after introducing tracked change post-{} acceptance",
        concat!("pre", "flight")
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        concat!(
            "workflow doctor for accepted-pre",
            "flight truth after dirty-workspace fixture"
        ),
    );
    let doctor_json = workflow_doctor_json(
        &runtime,
        concat!(
            "workflow doctor for accepted-pre",
            "flight truth after dirty-workspace fixture"
        ),
    );
    assert_eq!(doctor_json["phase"], concat!("execution_pre", "flight"));
    assert!(
        doctor_json["execution_status"]["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "doctor.execution_status.execution_run_id should stay non-empty after accepted {} even when workspace becomes dirty",
        concat!("pre", "flight")
    );

    assert_ne!(
        doctor_json[concat!("pre", "flight")]["failure_class"],
        "WorkspaceNotSafe",
        "workflow doctor should not surface a fresh WorkspaceNotSafe {} failure after {} was already accepted",
        concat!("pre", "flight"),
        concat!("pre", "flight")
    );
    assert_ne!(
        doctor_json[concat!("pre", "flight")]["allowed"],
        false,
        "workflow doctor should not report {}.allowed=false once accepted {} state exists",
        concat!("pre", "flight"),
        concat!("pre", "flight")
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_gate_review_rejects_stale_authoritative_late_gate_truth()
 {
    let (repo_dir, state_dir) = init_repo(concat!(
        "workflow-phase-gate",
        "-review-stale-authoritative-truth"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = concat!("workflow-phase-gate", "-review-stale-authoritative-truth");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let branch = current_branch_name(repo);
    let expected_base_branch = expected_release_base_branch(repo);
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    let review_path = write_branch_review_artifact(repo, state, plan_rel, &expected_base_branch);
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &expected_base_branch);
    enable_session_decision(state, session_key);

    let authoritative_review_source = fs::read_to_string(&review_path)
        .expect("source review artifact should be readable for stale-authoritative fixture");
    let authoritative_review_fingerprint = sha256_hex(authoritative_review_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("final-review-{authoritative_review_fingerprint}.md"),
        ),
        &authoritative_review_source,
    );

    let authoritative_test_plan_source = fs::read_to_string(&test_plan_path)
        .expect("source test-plan artifact should be readable for stale-authoritative fixture");
    let authoritative_test_plan_fingerprint = sha256_hex(authoritative_test_plan_source.as_bytes());
    let authoritative_test_plan_path = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("test-plan-{authoritative_test_plan_fingerprint}.md"),
    );
    write_file(
        &authoritative_test_plan_path,
        &authoritative_test_plan_source,
    );

    let authoritative_qa_source = rewrite_source_test_plan_header(
        &fs::read_to_string(&qa_path)
            .expect("source QA artifact should be readable for stale-authoritative fixture"),
        &authoritative_test_plan_path,
    );
    let authoritative_qa_fingerprint = sha256_hex(authoritative_qa_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("browser-qa-{authoritative_qa_fingerprint}.md"),
        ),
        &authoritative_qa_source,
    );

    let authoritative_release_source = fs::read_to_string(&release_path)
        .expect("source release artifact should be readable for stale-authoritative fixture");
    let authoritative_release_fingerprint = sha256_hex(authoritative_release_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("release-docs-{authoritative_release_fingerprint}.md"),
        ),
        &authoritative_release_source,
    );

    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("final_review_pending")),
            ("latest_authoritative_sequence", Value::from(17)),
            ("dependency_index_state", Value::from("stale")),
            ("final_review_state", Value::from("stale")),
            ("browser_qa_state", Value::from("stale")),
            ("release_docs_state", Value::from("stale")),
            (
                "last_final_review_artifact_fingerprint",
                Value::from(authoritative_review_fingerprint),
            ),
            (
                "last_browser_qa_artifact_fingerprint",
                Value::from(authoritative_qa_fingerprint.clone()),
            ),
            (
                "last_release_docs_artifact_fingerprint",
                Value::from(authoritative_release_fingerprint),
            ),
        ],
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow doctor for stale authoritative late-gate truth",
    );
    let doctor_json = workflow_doctor_json(
        &runtime,
        "workflow doctor for stale authoritative late-gate truth",
    );
    let gate_review_json = parse_json(
        &internal_only_workflow_gate_review_output(
            repo,
            state,
            plan_rel,
            "workflow gate review for stale authoritative late-gate truth",
        ),
        "workflow gate review for stale authoritative late-gate truth",
    );

    assert_eq!(
        gate_review_json["allowed"], false,
        "workflow gate review should load authoritative late-gate truth before trusting v2 evidence; got {gate_review_json:?}"
    );
    assert_eq!(
        doctor_json["gate_review"]["allowed"], false,
        "workflow doctor should report review gate blocked when authoritative late-gate truth is stale; got {doctor_json:?}"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_gate_review_fail_closes_on_malformed_authoritative_late_gate_truth_values()
 {
    let (repo_dir, state_dir) = init_repo(concat!(
        "workflow-phase-gate",
        "-review-malformed-authoritative-truth"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = concat!(
        "workflow-phase-gate",
        "-review-malformed-authoritative-truth"
    );
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    enable_session_decision(state, session_key);

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("malformed")),
            ("browser_qa_state", Value::from("malformed")),
            ("release_docs_state", Value::from("malformed")),
        ],
    );

    let gate_review_json = parse_json(
        &internal_only_workflow_gate_review_output(
            repo,
            state,
            plan_rel,
            "workflow gate review for malformed authoritative late-gate truth",
        ),
        "workflow gate review for malformed authoritative late-gate truth",
    );

    assert_eq!(gate_review_json["allowed"], false, "{gate_review_json:?}");
    assert_eq!(
        gate_review_json["failure_class"], "StaleProvenance",
        "{gate_review_json:?}"
    );
    assert!(
        gate_review_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| {
                codes
                    .iter()
                    .any(|code| code == "final_review_state_missing")
                    && codes.iter().any(|code| code == "browser_qa_state_missing")
                    && codes
                        .iter()
                        .any(|code| code == "release_docs_state_missing")
            }),
        "{} should fail closed with malformed authoritative late-gate reason codes; got {gate_review_json:?}",
        concat!("gate", "-review")
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_requires_authoritative_review_truth_before_ready_for_branch_completion()
 {
    let (repo_dir, state_dir) =
        init_repo("workflow-phase-ready-guard-stale-authoritative-late-gate-truth");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-ready-guard-stale-authoritative-late-gate-truth";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("harness_phase", Value::from("final_review_pending")),
            ("latest_authoritative_sequence", Value::from(17)),
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("stale")),
            ("browser_qa_state", Value::from("not_required")),
            ("release_docs_state", Value::from("stale")),
        ],
    );
    enable_session_decision(state, session_key);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow phase for ready guard stale-authoritative late-gate truth fixture",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow phase for ready guard stale-authoritative late-gate truth fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for ready guard stale-authoritative late-gate truth fixture",
    );
    let gate_review_json = parse_json(
        &internal_only_workflow_gate_review_output(
            repo,
            state,
            plan_rel,
            "workflow gate review for ready guard stale-authoritative late-gate truth fixture",
        ),
        "workflow gate review for ready guard stale-authoritative late-gate truth fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for ready guard stale-authoritative late-gate truth fixture",
        ),
        "workflow gate finish for ready guard stale-authoritative late-gate truth fixture",
    );
    assert_eq!(gate_review_json["allowed"], false, "{gate_review_json:?}");
    assert_eq!(
        gate_finish_json["allowed"],
        false,
        "{} must consume the same authoritative late-gate truth as {}; got {gate_finish_json:?}",
        concat!("gate", "-finish"),
        concat!("gate", "-review")
    );
    assert!(
        gate_finish_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| {
                codes.iter().any(|code| code == "qa_artifact_missing")
                    && codes.iter().any(|code| code == "browser_qa_state_missing")
            }),
        "{} should expose event-authoritative late-gate blockers and ignore projection-only stale fields; got {gate_finish_json:?}",
        concat!("gate", "-finish")
    );
    assert_eq!(
        phase_json["phase"], "qa_pending",
        "projection-only stale late-gate fields must not override typed milestone records, got {phase_json:?}"
    );
    assert_eq!(
        handoff_json["recommended_skill"], "featureforge:qa-only",
        "typed final-review truth with required QA should route to QA, got {handoff_json:?}"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_doctor_and_gate_finish_prefer_recorded_authoritative_final_review_over_newer_branch_decoy()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-authoritative-final-review-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-authoritative-final-review-provenance";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let branch = current_branch_name(repo);
    let expected_base_branch = expected_release_base_branch(repo);
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    let review_path = internal_only_write_dispatched_branch_review_artifact(
        repo,
        state,
        plan_rel,
        &expected_base_branch,
    );
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    write_branch_release_artifact(repo, state, plan_rel, &expected_base_branch);
    enable_session_decision(state, session_key);
    let _ = review_path;
    let _ = publish_authoritative_qa_truth(
        repo,
        state,
        plan_rel,
        &test_plan_path,
        &qa_path,
        &expected_base_branch,
    );

    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("ready_for_branch_completion")),
            ("latest_authoritative_sequence", Value::from(17)),
        ],
    );

    write_file(
        &project_artifact_dir(repo, state).join(format!(
            "tester-{}-code-review-99999999-999999.md",
            branch_storage_key(&branch)
        )),
        &format!(
            "# Code Review Result\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {branch}\n**Head SHA:** 0000000000000000000000000000000000000000\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T23:59:59Z\n\n## Summary\n- newer same-branch decoy should not override recorded authoritative final-review provenance.\n",
            repo_slug(repo)
        ),
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow doctor for authoritative final-review provenance override fixture",
    );
    let doctor_json = workflow_doctor_json(
        &runtime,
        "workflow doctor for authoritative final-review provenance override fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for authoritative final-review provenance override fixture",
        ),
        "workflow gate finish for authoritative final-review provenance override fixture",
    );

    assert_eq!(
        gate_finish_json["allowed"], true,
        "workflow gate finish should resolve final-review freshness from recorded authoritative provenance instead of scanning the newest branch artifact; got {gate_finish_json:?}"
    );
    assert_eq!(
        doctor_json["gate_finish"]["allowed"], true,
        "workflow doctor should report final-review freshness from recorded authoritative provenance instead of scanning the newest branch artifact; got {doctor_json:?}"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_doctor_and_gate_finish_prefer_recorded_authoritative_release_docs_over_newer_branch_decoy()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-authoritative-release-docs-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-authoritative-release-docs-provenance";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let branch = current_branch_name(repo);
    let expected_base_branch = expected_release_base_branch(repo);
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    let review_path = internal_only_write_dispatched_branch_review_artifact(
        repo,
        state,
        plan_rel,
        &expected_base_branch,
    );
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &expected_base_branch);
    enable_session_decision(state, session_key);
    let _ = review_path;
    let _ = release_path;
    let _ = publish_authoritative_qa_truth(
        repo,
        state,
        plan_rel,
        &test_plan_path,
        &qa_path,
        &expected_base_branch,
    );

    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("ready_for_branch_completion")),
            ("latest_authoritative_sequence", Value::from(17)),
        ],
    );

    write_file(
        &project_artifact_dir(repo, state).join(format!(
            "tester-{}-release-readiness-99999999-999999.md",
            branch_storage_key(&branch)
        )),
        &format!(
            "# Release Readiness Result\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {expected_base_branch}\n**Head SHA:** 0000000000000000000000000000000000000000\n**Result:** pass\n**Generated By:** featureforge:document-release\n**Generated At:** 2026-03-24T23:59:59Z\n\n## Summary\n- newer same-branch decoy should not override recorded authoritative downstream release-doc provenance.\n",
            repo_slug(repo)
        ),
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow doctor for authoritative release-doc provenance override fixture",
    );
    let doctor_json = workflow_doctor_json(
        &runtime,
        "workflow doctor for authoritative release-doc provenance override fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for authoritative release-doc provenance override fixture",
        ),
        "workflow gate finish for authoritative release-doc provenance override fixture",
    );

    assert_eq!(
        gate_finish_json["allowed"], true,
        "workflow gate finish should resolve release-doc freshness from recorded authoritative downstream provenance instead of scanning the newest branch artifact; got {gate_finish_json:?}"
    );
    assert_eq!(
        doctor_json["gate_finish"]["allowed"], true,
        "workflow doctor should report release-doc freshness from recorded authoritative downstream provenance instead of scanning the newest branch artifact; got {doctor_json:?}"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_missing_test_plan_back_to_plan_eng_review()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-test-plan-missing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-missing";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for missing-test-plan routing fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for missing-test-plan routing fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for missing-test-plan routing fixture",
        ),
        "workflow gate finish for missing-test-plan routing fixture",
    );
    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "QaArtifactNotFresh");
    assert_eq!(gate_finish_json["reason_codes"][0], "qa_artifact_missing");
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_prioritizes_test_plan_prerequisite_over_failed_current_qa_result()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-test-plan-prereq-over-failed-qa");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-prereq-over-failed-qa";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    let branch = current_branch_name(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("harness_phase", Value::from("qa_pending")),
            (
                "current_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_release_readiness_result", Value::from("ready")),
            ("release_docs_state", Value::from("fresh")),
            ("final_review_state", Value::from("fresh")),
            ("browser_qa_state", Value::from("missing")),
            (
                "current_qa_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_qa_result", Value::from("fail")),
        ],
    );
    enable_session_decision(state, session_key);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for test-plan-prereq-over-failed-qa fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for test-plan-prereq-over-failed-qa fixture",
    );

    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_malformed_test_plan_back_to_plan_eng_review()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-test-plan-malformed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-malformed";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);
    replace_in_file(&test_plan_path, "# Test Plan", "# Not A Test Plan");

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for malformed-test-plan routing fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for malformed-test-plan routing fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for malformed-test-plan routing fixture",
        ),
        "workflow gate finish for malformed-test-plan routing fixture",
    );

    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "QaArtifactNotFresh");
    assert_eq!(gate_finish_json["reason_codes"][0], "qa_artifact_missing");
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_ignores_test_plan_generator_drift_for_routing()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-test-plan-generator-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-generator-mismatch";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);
    replace_in_file(
        &test_plan_path,
        "**Generated By:** featureforge:plan-eng-review",
        "**Generated By:** manual-test-plan-edit",
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for test-plan generator mismatch routing fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for test-plan generator mismatch routing fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for test-plan generator mismatch routing fixture",
        ),
        "workflow gate finish for test-plan generator mismatch routing fixture",
    );

    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "QaArtifactNotFresh");
    assert_eq!(gate_finish_json["reason_codes"][0], "qa_artifact_missing");
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_stale_test_plan_back_to_plan_eng_review()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-test-plan-stale");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-stale";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);
    replace_in_file(
        &test_plan_path,
        &format!("**Head SHA:** {}", current_head_sha(repo)),
        "**Head SHA:** 0000000000000000000000000000000000000000",
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for stale-test-plan routing fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for stale-test-plan routing fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for stale-test-plan routing fixture",
        ),
        "workflow gate finish for stale-test-plan routing fixture",
    );

    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "QaArtifactNotFresh");
    assert_eq!(gate_finish_json["reason_codes"][0], "qa_artifact_missing");
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_authoritative_qa_provenance_invalid_to_qa_pending()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-qa-authoritative-provenance-invalid");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-qa-authoritative-provenance-invalid";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    let _ = publish_authoritative_qa_truth(
        repo,
        state,
        plan_rel,
        &test_plan_path,
        &qa_path,
        &base_branch,
    );
    enable_session_decision(state, session_key);

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("qa_pending")),
            ("browser_qa_state", Value::from("fresh")),
        ],
    );
    update_current_history_record_field(
        repo,
        state,
        "browser_qa_record_history",
        "current_qa_record_id",
        "browser_qa_fingerprint",
        Value::from("not-a-fingerprint"),
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for authoritative-qa-provenance-invalid routing fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for authoritative-qa-provenance-invalid routing fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for authoritative-qa-provenance-invalid routing fixture",
        ),
        "workflow gate finish for authoritative-qa-provenance-invalid routing fixture",
    );

    assert_eq!(gate_finish_json["allowed"], true);
    assert_eq!(
        handoff_json["phase"], "ready_for_branch_completion",
        "{gate_finish_json:?}"
    );
    assert_eq!(
        handoff_json["next_action"], "finish branch",
        "{handoff_json:?}"
    );
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:finishing-a-development-branch"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_authoritative_test_plan_provenance_invalid_to_plan_eng_review()
 {
    let (repo_dir, state_dir) =
        init_repo("workflow-phase-test-plan-authoritative-provenance-invalid");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-authoritative-provenance-invalid";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    enable_session_decision(state, session_key);

    let branch = current_branch_name(repo);
    let (authoritative_test_plan_fingerprint, authoritative_qa_fingerprint) =
        publish_authoritative_qa_truth(
            repo,
            state,
            plan_rel,
            &test_plan_path,
            &qa_path,
            &base_branch,
        );
    fs::remove_file(harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("test-plan-{authoritative_test_plan_fingerprint}.md"),
    ))
    .expect(
        "authoritative test-plan provenance fixture should remove the current test-plan artifact",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("qa_pending")),
            ("browser_qa_state", Value::from("fresh")),
        ],
    );
    update_current_history_record_field(
        repo,
        state,
        "browser_qa_record_history",
        "current_qa_record_id",
        "browser_qa_fingerprint",
        Value::from(authoritative_qa_fingerprint),
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for authoritative-test-plan-provenance-invalid routing fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for authoritative-test-plan-provenance-invalid routing fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for authoritative-test-plan-provenance-invalid routing fixture",
        ),
        "workflow gate finish for authoritative-test-plan-provenance-invalid routing fixture",
    );

    assert_eq!(gate_finish_json["allowed"], true);
    assert_eq!(
        handoff_json["phase"], "ready_for_branch_completion",
        "{gate_finish_json:?}"
    );
    assert_eq!(
        handoff_json["next_action"], "finish branch",
        "{handoff_json:?}"
    );
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:finishing-a-development-branch"
    );
}

fn internal_only_assert_authoritative_qa_source_test_plan_header_failure(
    fixture_name: &str,
    qa_source_transform: fn(&str) -> String,
) {
    let (repo_dir, state_dir) = init_repo(fixture_name);
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = fixture_name;
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    enable_session_decision(state, session_key);

    let branch = current_branch_name(repo);
    let _ = publish_authoritative_qa_truth(
        repo,
        state,
        plan_rel,
        &test_plan_path,
        &qa_path,
        &base_branch,
    );
    let authoritative_qa_source = qa_source_transform(
        &fs::read_to_string(&qa_path)
            .expect("source QA artifact should be readable for authoritative provenance fixture"),
    );
    let authoritative_qa_fingerprint = sha256_hex(authoritative_qa_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("browser-qa-{authoritative_qa_fingerprint}.md"),
        ),
        &authoritative_qa_source,
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("qa_pending")),
            ("browser_qa_state", Value::from("fresh")),
            (
                "last_browser_qa_artifact_fingerprint",
                Value::from(authoritative_qa_fingerprint.clone()),
            ),
        ],
    );
    update_current_history_record_field(
        repo,
        state,
        "browser_qa_record_history",
        "current_qa_record_id",
        "browser_qa_fingerprint",
        Value::from(authoritative_qa_fingerprint),
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for malformed authoritative QA->test-plan provenance fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for malformed authoritative QA->test-plan provenance fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for malformed authoritative QA->test-plan provenance fixture",
        ),
        "workflow gate finish for malformed authoritative QA->test-plan provenance fixture",
    );

    assert_eq!(gate_finish_json["allowed"], true);
    assert_eq!(
        handoff_json["phase"], "ready_for_branch_completion",
        "{gate_finish_json:?}"
    );
    assert_eq!(
        handoff_json["next_action"], "finish branch",
        "{handoff_json:?}"
    );
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:finishing-a-development-branch"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_missing_authoritative_qa_source_test_plan_header_to_plan_eng_review()
 {
    internal_only_assert_authoritative_qa_source_test_plan_header_failure(
        "workflow-phase-test-plan-header-missing",
        remove_source_test_plan_header,
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_blank_authoritative_qa_source_test_plan_header_to_plan_eng_review()
 {
    internal_only_assert_authoritative_qa_source_test_plan_header_failure(
        "workflow-phase-test-plan-header-blank",
        blank_source_test_plan_header,
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_authoritative_release_provenance_invalid_to_document_release()
 {
    let (repo_dir, state_dir) =
        init_repo("workflow-phase-release-authoritative-provenance-invalid");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-release-authoritative-provenance-invalid";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("not_required")),
            ("browser_qa_state", Value::from("not_required")),
            ("release_docs_state", Value::from("fresh")),
        ],
    );
    update_current_history_record_field(
        repo,
        state,
        "release_readiness_record_history",
        "current_release_readiness_record_id",
        "release_docs_fingerprint",
        Value::from("not-a-fingerprint"),
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for authoritative-release-provenance-invalid routing fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for authoritative-release-provenance-invalid routing fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for authoritative-release-provenance-invalid routing fixture",
        ),
        "workflow gate finish for authoritative-release-provenance-invalid routing fixture",
    );

    assert_eq!(gate_finish_json["allowed"], false);
    assert!(
        gate_finish_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| { codes.iter().any(|code| code == "review_artifact_missing") }),
        "{} should fail at final-review freshness when release receipt markdown is no longer authoritative, got {gate_finish_json:?}",
        concat!("gate", "-finish")
    );
    assert_eq!(
        handoff_json["phase"], "final_review_pending",
        "{gate_finish_json:?}"
    );
    assert_eq!(
        handoff_json["next_action"], "request final review",
        "{handoff_json:?}"
    );
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:requesting-code-review"
    );
    assert_eq!(handoff_json["reason_family"], "final_review_freshness");
    assert!(
        handoff_json["diagnostic_reason_codes"]
            .as_array()
            .is_some_and(|codes| { codes.iter().any(|code| code == "review_artifact_missing") }),
        "handoff observability should expose authoritative final-review freshness diagnostics, got {handoff_json:?}"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_release_and_review_unresolved_to_document_release_pending()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-release-and-review-unresolved");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-release-and-review-unresolved";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    enable_session_decision(state, session_key);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for release+review-unresolved routing fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for release+review-unresolved routing fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for release+review-unresolved routing fixture",
        ),
        "workflow gate finish for release+review-unresolved routing fixture",
    );

    assert_eq!(
        handoff_json["phase"], "document_release_pending",
        "{gate_finish_json:?}"
    );
    assert_eq!(handoff_json["next_action"], "advance late stage");
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:document-release"
    );
    assert_eq!(gate_finish_json["allowed"], false);
    assert!(
        gate_finish_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "release_docs_state_missing")),
        "{gate_finish_json:?}"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_mixed_stale_matrix() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-matrix-shared");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    let cases = [
        (
            "all_missing",
            false,
            false,
            true,
            "document_release_pending",
            "advance late stage",
            "featureforge:document-release",
            "release_readiness",
        ),
        (
            "release_review_missing_qa_fresh",
            false,
            false,
            false,
            "document_release_pending",
            "advance late stage",
            "featureforge:document-release",
            "release_readiness",
        ),
        (
            "release_missing_review_fresh_qa_missing",
            false,
            true,
            true,
            "qa_pending",
            "run QA",
            "featureforge:qa-only",
            "qa_freshness",
        ),
        (
            "release_missing_review_qa_fresh",
            false,
            true,
            false,
            "ready_for_branch_completion",
            "finish branch",
            "featureforge:finishing-a-development-branch",
            "all_fresh",
        ),
        (
            "release_fresh_review_qa_missing",
            true,
            false,
            true,
            "final_review_pending",
            "request final review",
            "featureforge:requesting-code-review",
            "final_review_freshness",
        ),
        (
            "release_qa_fresh_review_missing",
            true,
            false,
            false,
            "final_review_pending",
            "request final review",
            "featureforge:requesting-code-review",
            "final_review_freshness",
        ),
        (
            "release_review_fresh_qa_missing",
            true,
            true,
            true,
            "qa_pending",
            "run QA",
            "featureforge:qa-only",
            "qa_freshness",
        ),
        (
            "all_fresh",
            true,
            true,
            false,
            "ready_for_branch_completion",
            "finish branch",
            "featureforge:finishing-a-development-branch",
            "all_fresh",
        ),
    ];

    for (
        case_id,
        release_fresh,
        review_fresh,
        qa_blocked,
        expected_phase,
        expected_next_action,
        expected_skill,
        expected_reason_family,
    ) in cases
    {
        let fixture_name = format!("workflow-phase-matrix-{case_id}");
        let session_key = fixture_name.as_str();
        let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

        complete_workflow_fixture_execution_with_qa_requirement(
            repo,
            state,
            plan_rel,
            if qa_blocked {
                "required"
            } else {
                "not-required"
            },
        );
        write_branch_test_plan_artifact(
            repo,
            state,
            plan_rel,
            if qa_blocked { "yes" } else { "no" },
        );
        if review_fresh {
            internal_only_write_dispatched_branch_review_artifact(
                repo,
                state,
                plan_rel,
                &base_branch,
            );
        }
        if release_fresh {
            write_branch_release_artifact(repo, state, plan_rel, &base_branch);
        }
        let branch = current_branch_name(repo);
        update_authoritative_harness_state(
            repo,
            state,
            &branch,
            plan_rel,
            1,
            &[
                ("dependency_index_state", Value::from("fresh")),
                (
                    "final_review_state",
                    Value::from(if review_fresh { "fresh" } else { "missing" }),
                ),
                (
                    "browser_qa_state",
                    Value::from(if qa_blocked {
                        "missing"
                    } else {
                        "not_required"
                    }),
                ),
                (
                    "release_docs_state",
                    Value::from(if release_fresh { "fresh" } else { "missing" }),
                ),
            ],
        );
        enable_session_decision(state, session_key);
        let runtime =
            discover_execution_runtime(repo, state, "workflow_runtime mixed stale matrix fixture");
        let handoff_json =
            workflow_handoff_json(&runtime, "workflow_runtime mixed stale matrix fixture");
        let status_json = plan_execution_status_json(
            &runtime,
            plan_rel,
            false,
            "workflow_runtime mixed stale matrix fixture",
        );

        assert_eq!(
            handoff_json["phase"], expected_phase,
            "matrix case {case_id} expected phase {expected_phase}, got handoff payload: {handoff_json:?}"
        );
        assert_eq!(
            handoff_json["next_action"], expected_next_action,
            "matrix case {case_id} expected next_action {expected_next_action}, got handoff payload: {handoff_json:?}"
        );
        assert_eq!(
            handoff_json["recommended_skill"], expected_skill,
            "matrix case {case_id} expected recommended_skill {expected_skill}, got handoff payload: {handoff_json:?}"
        );
        assert_eq!(
            handoff_json["reason_family"], expected_reason_family,
            "matrix case {case_id} expected handoff reason_family {expected_reason_family}, got handoff payload: {handoff_json:?}"
        );
        assert_eq!(
            status_json["harness_phase"], expected_phase,
            "matrix case {case_id} expected harness_phase {expected_phase}, got status payload: {status_json:?}; handoff payload: {handoff_json:?}"
        );
    }
}

#[test]
fn internal_only_compatibility_canonical_workflow_harness_operator_parity_unclassified_finish_failure_fails_closed()
 {
    let (repo_dir, state_dir) = init_repo("workflow-harness-operator-parity-unclassified-finish");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-harness-operator-parity-unclassified-finish";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &current_branch_name(repo),
            "execution-topology-downgrade-malformed.json",
        ),
        "{ malformed topology downgrade record",
    );
    enable_session_decision(state, session_key);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_runtime unclassified-finish parity fixture",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow_runtime unclassified-finish parity fixture",
    );
    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_runtime unclassified-finish parity fixture",
    );
    let gate_finish_json = parse_json(
        &internal_only_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for unclassified-finish parity fixture",
        ),
        "workflow gate finish for unclassified-finish parity fixture",
    );

    assert_eq!(gate_finish_json["allowed"], true, "{gate_finish_json:?}");
    assert_eq!(
        status_json["harness_phase"], "ready_for_branch_completion",
        "status should preserve authoritative late-stage readiness when receipt markdown is no longer gate authority; status payload: {status_json:?}; gate_finish payload: {gate_finish_json:?}"
    );
    assert_eq!(
        phase_json["phase"], "ready_for_branch_completion",
        "operator phase should preserve parity with status for unclassified legacy receipt failures; phase payload: {phase_json:?}; status payload: {status_json:?}; gate_finish payload: {gate_finish_json:?}"
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_review_resolved_browser_qa_to_qa_only()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-qa-pending");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-qa-pending";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for qa-pending routing fixture",
    );
    let handoff_json =
        workflow_handoff_json(&runtime, "workflow handoff for qa-pending routing fixture");

    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
    assert_eq!(
        handoff_json["recommendation_reason"],
        "Finish readiness requires a current QA milestone for the current branch closure."
    );
}

#[test]
fn internal_only_compatibility_canonical_workflow_phase_routes_fully_ready_branch_to_finish() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-ready-for-finish");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-ready-for-finish";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow doctor for ready-for-finish routing fixture",
    );
    let doctor_json = workflow_doctor_json(
        &runtime,
        "workflow doctor for ready-for-finish routing fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for ready-for-finish routing fixture",
    );

    assert_eq!(doctor_json["gate_finish"]["allowed"], true);
    assert_eq!(handoff_json["phase"], "ready_for_branch_completion");
    assert_eq!(handoff_json["next_action"], "finish branch");
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:finishing-a-development-branch"
    );
    assert_eq!(
        handoff_json["recommendation_reason"],
        "All required late-stage artifacts are fresh for the current HEAD."
    );
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs01_shared_route_parity_for_missing_current_closure()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs01-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    let branch = current_branch_name(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    let test_plan = write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    publish_authoritative_release_truth(repo, state, plan_rel, &release_path, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            (
                "review_state_repair_follow_up",
                Value::from("record_branch_closure"),
            ),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-01 workflow operator shared-runtime parity fixture",
        ),
        "FS-01 workflow operator shared-runtime parity fixture",
    );
    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-01 plan execution status shared-runtime parity fixture",
    );
    let runtime = discover_execution_runtime(
        repo,
        state,
        "FS-01 workflow doctor shared-runtime parity fixture",
    );
    let doctor_json = workflow_doctor_json(
        &runtime,
        "FS-01 workflow doctor shared-runtime parity fixture",
    );

    assert_public_route_parity(&operator_json, &status_json, Some(&doctor_json));

    let repair_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-01 repair-review-state shared-runtime parity fixture",
    );
    let operator_after_repair = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-01 workflow operator after repair fixture",
        ),
        "FS-01 workflow operator after repair fixture",
    );
    if repair_json["action"] == "already_current" {
        assert_ne!(
            operator_after_repair["review_state_status"],
            Value::from("missing_current_closure"),
            "FS-01 shared truth must not leave a missing-current-closure blocker active when repair says already_current"
        );
    }
    assert!(
        test_plan.exists(),
        "FS-01 fixture sanity check should preserve staged projection setup"
    );
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs04_repair_returns_route_consumed_by_operator()
{
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs04-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs04-repair-route-runtime.md";
    internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                plan_rel,
                "--external-review-result-ready",
                "--json",
            ],
            &[],
            "FS-04 workflow operator shared-route parity fixture",
        ),
        "FS-04 workflow operator shared-route parity fixture",
    );
    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-04 plan execution status shared-route parity fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);

    let repair_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "repair-review-state",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-04 repair-review-state shared-runtime fixture",
    );
    assert_eq!(
        repair_json["action"],
        Value::from("blocked"),
        "FS-04 repair should surface the shared closure-recording blocker instead of claiming repair is already current"
    );
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-04 repair should expose task_closure_recording_ready shared-routing detail, got {repair_json:?}"
    );
    assert!(
        repair_json["required_follow_up"].is_null(),
        "FS-04 closure-baseline recovery should not require a stale follow-up category, got {repair_json:?}"
    );
    assert_eq!(
        repair_json["recommended_command"], operator_json["recommended_command"],
        "FS-04 repair and operator must agree that no executable command is available until review/verification inputs are supplied"
    );
    assert_task_closure_required_inputs(&operator_json, 1);
    assert_task_closure_required_inputs(&repair_json, 1);
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs08_resume_overlay_does_not_hide_stale_blocker()
{
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs08-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs08-stale-blocker.md";
    internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            ("resume_task", Value::from(1_u64)),
            ("resume_step", Value::from(1_u64)),
            (
                "review_state_repair_follow_up",
                Value::from("execution_reentry"),
            ),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-08 workflow operator stale-blocker visibility fixture",
        ),
        "FS-08 workflow operator stale-blocker visibility fixture",
    );
    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-08 plan execution status stale-blocker visibility fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-08 stale blocker should remain visible as task_closure_recording_ready"
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    assert_eq!(operator_json["blocking_scope"], Value::from("task"));
    assert_eq!(status_json["blocking_scope"], Value::from("task"));
    assert_eq!(operator_json["blocking_task"], Value::from(1_u64));
    assert_eq!(status_json["blocking_task"], Value::from(1_u64));
    let mut operator_reason_codes = operator_json["blocking_reason_codes"]
        .as_array()
        .expect("FS-08 operator should expose blocking_reason_codes as an array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut status_reason_codes = status_json["blocking_reason_codes"]
        .as_array()
        .expect("FS-08 status should expose blocking_reason_codes as an array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    operator_reason_codes.sort();
    status_reason_codes.sort();
    let expected_reason_codes = vec![
        String::from("prior_task_current_closure_missing"),
        String::from("task_closure_baseline_repair_candidate"),
    ];
    assert_eq!(
        operator_reason_codes, expected_reason_codes,
        "FS-08 operator should expose the exact stale-blocker reason-code set for this fixture"
    );
    assert_eq!(
        status_reason_codes, expected_reason_codes,
        "FS-08 status should expose the exact stale-blocker reason-code set for this fixture"
    );
    assert_task_closure_required_inputs(&operator_json, 1);
    assert_task_closure_required_inputs(&status_json, 1);
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs04_compiled_cli_repair_returns_route_consumed_by_operator()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs04-workflow-runtime-real-cli");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel =
        "docs/featureforge/plans/2026-04-03-runtime-fs04-repair-route-runtime-real-cli.md";
    internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                plan_rel,
                "--external-review-result-ready",
                "--json",
            ],
            &[],
            "FS-04 compiled-cli workflow operator shared-route parity fixture",
        ),
        "FS-04 compiled-cli workflow operator shared-route parity fixture",
    );
    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-04 compiled-cli plan execution status shared-route parity fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "repair-review-state",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-04 compiled-cli repair-review-state shared-runtime fixture",
    );
    assert_eq!(
        repair_json["action"],
        Value::from("blocked"),
        "FS-04 compiled-cli repair should surface the shared closure-recording blocker instead of claiming repair is already current"
    );
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-04 compiled-cli repair should expose task_closure_recording_ready shared-routing detail, got {repair_json:?}"
    );
    assert!(
        repair_json["required_follow_up"].is_null(),
        "FS-04 compiled-cli closure-baseline recovery should not require a stale follow-up category, got {repair_json:?}"
    );
    assert_eq!(
        repair_json["recommended_command"], operator_json["recommended_command"],
        "FS-04 compiled-cli repair and operator must agree that no executable command is available until review/verification inputs are supplied"
    );
    assert_task_closure_required_inputs(&operator_json, 1);
    assert_task_closure_required_inputs(&repair_json, 1);
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs08_compiled_cli_resume_overlay_does_not_hide_stale_blocker()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs08-workflow-runtime-real-cli");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs08-stale-blocker-real-cli.md";
    internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            ("resume_task", Value::from(1_u64)),
            ("resume_step", Value::from(1_u64)),
            (
                "review_state_repair_follow_up",
                Value::from("execution_reentry"),
            ),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-08 compiled-cli workflow operator stale-blocker visibility fixture",
        ),
        "FS-08 compiled-cli workflow operator stale-blocker visibility fixture",
    );
    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-08 compiled-cli plan execution status stale-blocker visibility fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    assert_eq!(operator_json["blocking_scope"], Value::from("task"));
    assert_eq!(status_json["blocking_scope"], Value::from("task"));
    assert_eq!(operator_json["blocking_task"], Value::from(1_u64));
    assert_eq!(status_json["blocking_task"], Value::from(1_u64));
    let mut operator_reason_codes = operator_json["blocking_reason_codes"]
        .as_array()
        .expect("FS-08 compiled-cli operator should expose blocking_reason_codes as an array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut status_reason_codes = status_json["blocking_reason_codes"]
        .as_array()
        .expect("FS-08 compiled-cli status should expose blocking_reason_codes as an array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    operator_reason_codes.sort();
    status_reason_codes.sort();
    let expected_reason_codes = vec![
        String::from("prior_task_current_closure_missing"),
        String::from("task_closure_baseline_repair_candidate"),
    ];
    assert_eq!(
        operator_reason_codes, expected_reason_codes,
        "FS-08 compiled-cli operator should expose the exact stale-blocker reason-code set for this fixture"
    );
    assert_eq!(
        status_reason_codes, expected_reason_codes,
        "FS-08 compiled-cli status should expose the exact stale-blocker reason-code set for this fixture"
    );
    assert_task_closure_required_inputs(&operator_json, 1);
    assert_task_closure_required_inputs(&status_json, 1);
}

fn setup_runtime_fs11_fs15_next_action_fixture(repo: &Path, state: &Path, plan_rel: &str) {
    install_full_contract_ready_artifacts(repo);
    write_runtime_remediation_fs15_plan(repo, plan_rel, FULL_CONTRACT_READY_SPEC_REL);
    prepare_preflight_acceptance_workspace(repo, "runtime-remediation-fs11-fs15-next-action");
    let branch = current_branch_name(repo);

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11/FS-15 status before execution-start bootstrap begin",
    );
    let begin = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-11/FS-15 status should expose execution fingerprint before begin"),
        ],
        "FS-11/FS-15 execution-start bootstrap task 1 begin",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-11/FS-15 bootstrap task 1 complete",
            "--manual-verify-summary",
            "FS-11/FS-15 bootstrap task 1 complete summary",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"].as_str().expect(
                "FS-11/FS-15 begin should expose execution fingerprint before task 1 complete",
            ),
        ],
        "FS-11/FS-15 execution-start bootstrap task 1 complete",
    );
    let repair_task_1 = run_plan_execution_json(
        repo,
        state,
        &[
            "repair-review-state",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-11/FS-15 task 1 repair bridge",
    );
    assert_task_closure_required_inputs(&repair_task_1, 1);
    let close_task_1 = record_task_closure_with_fixture_inputs(
        repo,
        state,
        plan_rel,
        1,
        "FS-11/FS-15 task 1 repair follow-up",
    );
    assert!(
        matches!(
            close_task_1["action"].as_str(),
            Some("recorded" | "already_current")
        ),
        "FS-11/FS-15 task 1 repair follow-up should leave task 1 closure current, got {close_task_1:?}"
    );
    let status_after_task_1_repair = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11/FS-15 status after task 1 repair",
    );
    let begin = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_after_task_1_repair["execution_fingerprint"]
                .as_str()
                .expect(
                    "FS-11/FS-15 status after task 1 repair should expose execution fingerprint before task 2 begin"
                ),
        ],
        "FS-11/FS-15 execution-start bootstrap task 2 begin",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-11/FS-15 bootstrap complete",
            "--manual-verify-summary",
            "FS-11/FS-15 bootstrap complete summary",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"].as_str().expect(
                "FS-11/FS-15 begin should expose execution fingerprint before task 2 complete",
            ),
        ],
        "FS-11/FS-15 execution-start bootstrap task 2 complete",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            (
                "task_closure_record_history",
                json!({
                    "task-2-stale": {
                        "closure_record_id": "task-2-stale",
                        "task": 2,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 10,
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-6-stale": {
                        "closure_record_id": "task-6-stale",
                        "task": 6,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 20,
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    }
                }),
            ),
            (
                "current_open_step_state",
                json!({
                    "task": 6,
                    "step": 1,
                    "note_state": "Interrupted",
                    "note_summary": "FS-11/FS-15 later resume overlay should not outrank earlier stale boundary",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 30
                }),
            ),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
            ("resume_task", Value::from(6_u64)),
            ("resume_step", Value::from(1_u64)),
        ],
    );
}

fn internal_only_setup_runtime_fs17_fs21_fs22_stale_task1_bridge_fixture(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    resume_overlay: Option<(u32, u32)>,
) {
    let dispatch_id =
        internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);
    let status_after_fixture = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-17/FS-21/FS-22 fixture status before stale history injection",
    );
    let execution_run_id = status_after_fixture["execution_run_id"]
        .as_str()
        .expect("FS-17/FS-21/FS-22 fixture status should expose execution_run_id")
        .to_owned();
    let branch = current_branch_name(repo);
    let mut updates = vec![(
        "task_closure_record_history",
        json!({
            "task-1-stale-history": {
                "closure_record_id": "task-1-stale-history",
                "task": 1,
                "source_plan_path": plan_rel,
                "source_plan_revision": 1,
                "record_sequence": 7,
                "record_status": "stale_unreviewed",
                "closure_status": "stale_unreviewed",
                "dispatch_id": dispatch_id,
                "execution_run_id": execution_run_id,
                "effective_reviewed_surface_paths": ["README.md"]
            }
        }),
    )];
    if let Some((resume_task, resume_step)) = resume_overlay {
        updates.extend([
            (
                "current_open_step_state",
                json!({
                    "task": resume_task,
                    "step": resume_step,
                    "note_state": "Interrupted",
                    "note_summary": "FS-21 interrupted downstream step should be preempted by stale Task 1 closure bridge.",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 42
                }),
            ),
            ("resume_task", Value::from(u64::from(resume_task))),
            ("resume_step", Value::from(u64::from(resume_step))),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
        ]);
    }
    update_authoritative_harness_state(repo, state, &branch, plan_rel, 1, &updates);
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs11_prestart_operator_status_begin_share_first_unchecked_step()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs11-prestart-shared-next-action");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;

    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(
        repo,
        "runtime-remediation-fs11-prestart-shared-next-action",
    );

    let status_before_begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11 prestart status shared next-action fixture",
    );
    assert_eq!(
        status_before_begin["execution_started"],
        Value::from("no"),
        "FS-11 prestart fixture should remain pre-start before begin parity checks"
    );
    let preflight_before_begin = internal_only_unit_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        concat!("FS-11 prestart pre", "flight before shared begin parity"),
    );
    assert_eq!(
        preflight_before_begin["allowed"],
        Value::Bool(true),
        "FS-11 prestart {} should allow begin parity fixture",
        concat!("pre", "flight")
    );
    let operator_before_begin = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-11 prestart operator shared next-action fixture",
        ),
        "FS-11 prestart operator shared next-action fixture",
    );
    assert_public_route_parity(&operator_before_begin, &status_before_begin, None);
    assert_eq!(
        operator_before_begin["recommended_command"], status_before_begin["recommended_command"],
        "FS-11 prestart operator and status must expose the same shared begin command target",
    );
    let recommended_command = operator_before_begin["recommended_command"]
        .as_str()
        .expect("FS-11 prestart operator should expose recommended command");
    assert!(
        recommended_command.starts_with("featureforge plan execution begin --plan "),
        "FS-11 prestart route should recommend begin from the shared engine, got {recommended_command}",
    );
    assert!(
        recommended_command.contains("--task 1") && recommended_command.contains("--step 1"),
        "FS-11 prestart shared begin target should point to Task 1 Step 1, got {recommended_command}",
    );
    let recommended_parts = recommended_command.split_whitespace().collect::<Vec<_>>();
    let recommended_task = recommended_parts
        .windows(2)
        .find(|window| window[0] == "--task")
        .map(|window| window[1])
        .expect("FS-11 prestart recommended begin should include --task");
    let recommended_step = recommended_parts
        .windows(2)
        .find(|window| window[0] == "--step")
        .map(|window| window[1])
        .expect("FS-11 prestart recommended begin should include --step");
    let begin_from_operator = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            recommended_task,
            "--step",
            recommended_step,
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-11 prestart status should expose execution fingerprint"),
        ],
        "FS-11 prestart begin parity command from operator/status shared decision",
    );
    assert_eq!(
        begin_from_operator["active_task"],
        Value::from(1_u64),
        "FS-11 prestart shared begin route should activate Task 1"
    );
    assert_eq!(
        begin_from_operator["active_step"],
        Value::from(1_u64),
        "FS-11 prestart shared begin route should activate Step 1"
    );
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs14_missing_task_closure_baseline_routes_to_close_current_task_not_execution_reentry()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs14-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs14-workflow-runtime.md";
    internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                plan_rel,
                "--external-review-result-ready",
                "--json",
            ],
            &[],
            "FS-14 workflow operator closure-baseline repair routing fixture",
        ),
        "FS-14 workflow operator closure-baseline repair routing fixture",
    );
    assert_eq!(
        operator_json["phase"],
        Value::from("task_closure_pending"),
        "FS-14 operator should stay in task-closure pending when closure baseline is missing"
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-14 operator should route missing closure baselines directly to task_closure_recording_ready"
    );
    assert_eq!(
        operator_json["next_action"],
        Value::from("close current task"),
        "FS-14 operator should route to close-current-task instead of execution reentry"
    );
    assert_task_closure_required_inputs(&operator_json, 1);
    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-14 status closure-baseline bridge routing fixture",
    );
    assert!(
        status_json["stale_unreviewed_closures"]
            .as_array()
            .is_some_and(|closures| closures.is_empty()),
        "FS-14 synthetic baseline bridge must not serialize a fake stale closure id: {status_json:?}"
    );
    assert_eq!(
        status_json["execution_reentry_target_source"],
        Value::from("baseline_bridge"),
        "FS-14 synthetic baseline bridge should be typed as baseline_bridge instead of masquerading as a closure-graph stale target: {status_json:?}"
    );
    assert!(
        operator_json["recommended_command"].is_null(),
        "FS-14 normal recovery should not surface hidden helper or placeholder commands: {operator_json}"
    );
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs14_repair_routes_missing_task_closure_baseline_to_close_current_task()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs14-repair-routing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs14-repair-routing.md";
    internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                plan_rel,
                "--external-review-result-ready",
                "--json",
            ],
            &[],
            "FS-14 repair parity workflow operator fixture",
        ),
        "FS-14 repair parity workflow operator fixture",
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-14 repair parity fixture should expose task_closure_recording_ready in workflow/operator"
    );
    assert_task_closure_required_inputs(&operator_json, 1);

    let repair_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "repair-review-state",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-14 repair parity fixture",
    );
    assert_eq!(
        repair_json["action"],
        Value::from("blocked"),
        "FS-14 repair parity fixture should surface closure recording as the shared blocker"
    );
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-14 repair parity fixture should surface task_closure_recording_ready"
    );
    assert_eq!(
        repair_json["required_follow_up"],
        Value::Null,
        "FS-14 repair parity fixture should avoid execution-reentry follow-up when closure recording is the next action"
    );
    assert_eq!(
        repair_json["recommended_command"], operator_json["recommended_command"],
        "FS-14 repair parity fixture should agree with workflow/operator that required inputs are needed before a command is executable"
    );
    assert_eq!(
        repair_json["authoritative_next_action"],
        Value::Null,
        "FS-14 repair parity fixture should not serialize a placeholder authoritative action"
    );
    assert_task_closure_required_inputs(&repair_json, 1);
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs14_operator_repair_parity_without_external_review_ready_flag()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs14-repair-routing-no-ext-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs14-repair-routing-no-ext-ready.md";
    internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-14 no-external-ready parity workflow operator fixture",
        ),
        "FS-14 no-external-ready parity workflow operator fixture",
    );
    assert_eq!(
        operator_json["phase"],
        Value::from("task_closure_pending"),
        "FS-14 no-external-ready parity should remain on task closure pending"
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-14 no-external-ready parity should route directly to task_closure_recording_ready"
    );
    assert_task_closure_required_inputs(&operator_json, 1);

    let repair_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-14 no-external-ready repair parity fixture",
    );
    assert_eq!(
        repair_json["action"],
        Value::from("blocked"),
        "FS-14 no-external-ready repair parity should surface closure recording as the shared blocker"
    );
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-14 no-external-ready repair parity should keep closure recording as the next action"
    );
    assert_eq!(
        repair_json["required_follow_up"],
        Value::Null,
        "FS-14 no-external-ready repair parity should not regress to execution reentry"
    );
    assert_eq!(
        repair_json["recommended_command"], operator_json["recommended_command"],
        "FS-14 no-external-ready repair parity should agree with workflow/operator that required inputs are needed before a command is executable"
    );
    assert_eq!(
        repair_json["authoritative_next_action"],
        Value::Null,
        "FS-14 no-external-ready repair parity should not serialize a placeholder authoritative action"
    );
    assert_task_closure_required_inputs(&repair_json, 1);
}

#[test]
fn internal_only_compatibility_fs17_stale_unreviewed_truthful_replay_promotes_to_task_closure_recording_ready()
 {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs17-stale-unreviewed-bridge-routing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-08-runtime-fs17-stale-unreviewed-bridge.md";
    internal_only_setup_runtime_fs17_fs21_fs22_stale_task1_bridge_fixture(
        repo, state, plan_rel, None,
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-17 workflow operator truthful replay convergence fixture",
        ),
        "FS-17 workflow operator truthful replay convergence fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-17 status truthful replay convergence fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        status_json["review_state_status"],
        Value::from("stale_unreviewed"),
        "FS-17 fixture must stay in stale_unreviewed while stale task-closure history is unresolved: {status_json:?}"
    );
    assert!(
        status_json["reason_codes"].as_array().is_some_and(|codes| {
            codes
                .iter()
                .any(|code| code == &Value::from("prior_task_current_closure_missing"))
                && codes
                    .iter()
                    .any(|code| code == &Value::from("task_closure_baseline_repair_candidate"))
        }),
        "FS-17 fixture should expose missing-current-closure baseline bridge reason codes: {status_json:?}"
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-17 truthful replay recovery must route to task_closure_recording_ready instead of generic execution reentry: {status_json:?}"
    );
    assert_task_closure_required_inputs(&status_json, 1);
    assert_ne!(
        status_json["phase_detail"],
        Value::from("execution_reentry_required"),
        "FS-17 truthful replay recovery must not fall back to execution_reentry_required"
    );
}

#[test]
fn internal_only_compatibility_fs20_runtime_owned_plan_and_execution_evidence_changes_do_not_stale_current_task_closure()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs20-task-closure-freshness");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-07-runtime-fs20-task-closure-freshness.md";
    let _task1_dispatch_id =
        internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);
    let review_summary_path = repo.join("docs/featureforge/execution-evidence/fs20-review.md");
    let verification_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs20-verification.md");
    write_file(
        &review_summary_path,
        "FS-20 current closure should remain fresh when only runtime-owned paths changed.\n",
    );
    write_file(&verification_summary_path, "FS-20 verification summary.\n");
    let close_task1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("FS-20 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("FS-20 verification summary path should be utf-8"),
        ],
        "FS-20 close-current-task should establish a current Task 1 closure",
    );
    assert_eq!(close_task1["action"], Value::from("recorded"));

    let baseline_status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 baseline status before runtime-owned churn",
    );
    let evidence_rel = baseline_status["evidence_path"]
        .as_str()
        .expect("FS-20 baseline status should expose evidence_path");
    materialize_state_dir_projections(
        repo,
        state,
        plan_rel,
        "FS-20 materialize state-dir projections before runtime-owned churn",
    );
    let plan_path = repo.join(plan_rel);
    let plan_source = fs::read_to_string(&plan_path).expect("FS-20 plan should be readable");
    write_file(
        &plan_path,
        &format!("{plan_source}\n<!-- fs20 runtime-owned plan mutation -->\n"),
    );
    let evidence_source =
        projection_support::read_state_dir_projection(&baseline_status, evidence_rel);
    projection_support::write_state_dir_projection(
        &baseline_status,
        evidence_rel,
        &format!("{evidence_source}\n<!-- fs20 runtime-owned evidence mutation -->\n"),
    );

    let status_after_churn = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 status after runtime-owned churn",
    );
    assert!(
        !status_after_churn["reason_codes"]
            .as_array()
            .expect("FS-20 status reason_codes should remain an array")
            .iter()
            .any(|code| code == &Value::from("prior_task_current_closure_stale")),
        "FS-20 runtime-owned plan/evidence-only churn must not stale the current Task 1 closure: {status_after_churn:?}"
    );
    assert_ne!(
        status_after_churn["blocking_task"],
        Value::from(1_u64),
        "FS-20 runtime-owned plan/evidence-only churn must not reblock Task 1 closure freshness"
    );
}

#[test]
fn internal_only_compatibility_fs20_runtime_owned_plan_and_execution_evidence_changes_do_not_null_current_branch_closure()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs20-branch-closure-freshness");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    publish_authoritative_release_truth(repo, state, plan_rel, &release_path, &base_branch);

    let baseline_status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 baseline branch status before runtime-owned churn",
    );
    let baseline_branch_closure_id = baseline_status["current_branch_closure_id"]
        .as_str()
        .expect("FS-20 baseline status should expose current_branch_closure_id")
        .to_owned();
    let evidence_rel = baseline_status["evidence_path"]
        .as_str()
        .expect("FS-20 baseline status should expose evidence_path");
    materialize_state_dir_projections(
        repo,
        state,
        plan_rel,
        "FS-20 materialize state-dir projections before branch runtime-owned churn",
    );
    let plan_path = repo.join(plan_rel);
    let plan_source = fs::read_to_string(&plan_path).expect("FS-20 plan should be readable");
    write_file(
        &plan_path,
        &format!("{plan_source}\n<!-- fs20 branch runtime-owned plan mutation -->\n"),
    );
    let evidence_source =
        projection_support::read_state_dir_projection(&baseline_status, evidence_rel);
    projection_support::write_state_dir_projection(
        &baseline_status,
        evidence_rel,
        &format!("{evidence_source}\n<!-- fs20 branch runtime-owned evidence mutation -->\n"),
    );

    let status_after_churn = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 status should retain current branch closure after runtime-owned churn",
    );
    assert_eq!(
        status_after_churn["current_branch_closure_id"],
        Value::from(baseline_branch_closure_id),
        "FS-20 runtime-owned plan/evidence churn must not null current_branch_closure_id"
    );
    assert!(
        !status_after_churn["current_release_readiness_state"].is_null(),
        "FS-20 runtime-owned plan/evidence churn must not drop release-readiness state"
    );
}

#[test]
fn internal_only_compatibility_fs20_branch_closure_remains_current_when_only_runtime_owned_plan_and_execution_evidence_paths_changed()
 {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs20-branch-closure-remains-current");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    publish_authoritative_release_truth(repo, state, plan_rel, &release_path, &base_branch);

    let baseline_status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 baseline branch-closure-current status",
    );
    let baseline_branch_closure_id = baseline_status["current_branch_closure_id"]
        .as_str()
        .expect("FS-20 baseline status should expose branch closure id")
        .to_owned();
    let baseline_release_state = baseline_status["current_release_readiness_state"].clone();
    let baseline_final_state = baseline_status["current_final_review_state"].clone();
    let baseline_qa_state = baseline_status["current_qa_state"].clone();
    let evidence_rel = baseline_status["evidence_path"]
        .as_str()
        .expect("FS-20 baseline status should expose evidence path");
    materialize_state_dir_projections(
        repo,
        state,
        plan_rel,
        "FS-20 materialize state-dir projections before branch-current runtime-owned churn",
    );
    let plan_path = repo.join(plan_rel);
    let plan_source = fs::read_to_string(&plan_path).expect("FS-20 plan should be readable");
    write_file(
        &plan_path,
        &format!("{plan_source}\n<!-- fs20 branch-current runtime-owned plan mutation -->\n"),
    );
    let evidence_source =
        projection_support::read_state_dir_projection(&baseline_status, evidence_rel);
    projection_support::write_state_dir_projection(
        &baseline_status,
        evidence_rel,
        &format!(
            "{evidence_source}\n<!-- fs20 branch-current runtime-owned evidence mutation -->\n"
        ),
    );

    let status_after_churn = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 branch closure should remain current when only runtime-owned paths changed",
    );
    assert_eq!(
        status_after_churn["current_branch_closure_id"],
        Value::from(baseline_branch_closure_id),
        "FS-20 branch closure should remain current when only runtime-owned plan/evidence paths changed"
    );
    assert_eq!(
        status_after_churn["current_release_readiness_state"], baseline_release_state,
        "FS-20 runtime-owned plan/evidence churn must not alter release-readiness state"
    );
    assert_eq!(
        status_after_churn["current_final_review_state"], baseline_final_state,
        "FS-20 runtime-owned plan/evidence churn must not alter final-review state"
    );
    assert_eq!(
        status_after_churn["current_qa_state"], baseline_qa_state,
        "FS-20 runtime-owned plan/evidence churn must not alter QA state"
    );
    assert_ne!(
        status_after_churn["review_state_status"],
        Value::from("missing_current_closure"),
        "FS-20 runtime-owned plan/evidence churn must not produce missing_current_closure reroute"
    );
}

#[test]
fn internal_only_compatibility_fs21_operator_status_and_exact_command_all_agree_on_close_current_task_when_bridge_is_ready()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs21-route-parity-close-current");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-10-runtime-fs21-route-parity-close-current.md";
    internal_only_setup_runtime_fs17_fs21_fs22_stale_task1_bridge_fixture(
        repo,
        state,
        plan_rel,
        Some((2, 1)),
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-21 workflow operator bridge-preempts-resume fixture",
        ),
        "FS-21 workflow operator bridge-preempts-resume fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-21 status bridge-preempts-resume fixture",
    );
    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-21 repair-review-state bridge-preempts-resume fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        status_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-21 stale-boundary bridge fixture should route to task_closure_recording_ready: {status_json:?}"
    );
    assert!(status_json["resume_task"].is_null());
    assert!(status_json["resume_step"].is_null());
    assert_task_closure_required_inputs(&operator_json, 1);
    assert_task_closure_required_inputs(&status_json, 1);
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    assert_eq!(
        repair_json["recommended_command"], operator_json["recommended_command"],
        "FS-21 operator/status/repair surfaces must agree on missing-input command absence"
    );
    assert_task_closure_required_inputs(&repair_json, 1);
}

#[test]
fn internal_only_compatibility_fs22_repair_review_state_prefers_non_destructive_closure_bridge_over_reentry_cleanup()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs22-bridge-first-repair");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-11-runtime-fs22-bridge-first-repair.md";
    internal_only_setup_runtime_fs17_fs21_fs22_stale_task1_bridge_fixture(
        repo, state, plan_rel, None,
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-22 repair-review-state bridge-first non-destructive fixture",
    );
    assert_eq!(repair_json["action"], Value::from("blocked"));
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-22 repair-review-state must promote to task_closure_recording_ready when closure bridge is available: {repair_json:?}"
    );
    assert_eq!(
        repair_json["required_follow_up"],
        Value::Null,
        "FS-22 closure bridge lane must not keep an execution_reentry follow-up"
    );
    assert_task_closure_required_inputs(&repair_json, 1);
    let actions = repair_json["actions_performed"]
        .as_array()
        .expect("FS-22 repair should expose actions_performed array");
    assert!(
        actions.iter().all(|action| {
            action.as_str().is_some_and(|action| {
                !action.starts_with("cleared_task_review_dispatch_lineage")
                    && !action.starts_with("cleared_current_task_closure_scope")
                    && !action.starts_with("cleared_current_task_closure_task")
            })
        }),
        "FS-22 bridge-first repair must not run destructive task-scope/dispatch-lineage cleanup actions: {repair_json:?}"
    );
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs16_current_positive_task_closure_allows_next_task_begin_even_if_receipts_need_projection_refresh()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs16-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-05-runtime-fs16-workflow-runtime.md";
    let _dispatch_id =
        internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let review_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs16-review-summary.md");
    let verification_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs16-verification-summary.md");
    write_file(
        &review_summary_path,
        "FS-16 close-current-task review summary fixture.\n",
    );
    write_file(
        &verification_summary_path,
        "FS-16 close-current-task verification summary fixture.\n",
    );
    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("FS-16 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("FS-16 verification summary path should be utf-8"),
        ],
        "FS-16 close-current-task should record current positive closure before projection drift",
    );
    assert_eq!(
        close_json["action"],
        Value::from("recorded"),
        "FS-16 fixture should establish a current positive task closure before projection drift"
    );
    let status_after_close = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-16 status after recording current positive closure",
    );
    let execution_run_id = status_after_close["execution_run_id"]
        .as_str()
        .expect("FS-16 status should expose execution_run_id after close-current-task")
        .to_owned();

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[("strategy_review_dispatch_lineage", json!({}))],
    );

    for step in [1_u32, 2_u32] {
        let unit_review_receipt = harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("unit-review-{execution_run_id}-task-1-step-{step}.md"),
        );
        if unit_review_receipt.is_file() {
            fs::remove_file(&unit_review_receipt).unwrap_or_else(|error| {
                panic!(
                    "FS-16 should be able to remove stale unit-review projection `{}`: {error}",
                    unit_review_receipt.display()
                )
            });
        }
    }
    let verification_receipt = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("task-verification-{execution_run_id}-task-1.md"),
    );
    if verification_receipt.is_file() {
        fs::remove_file(&verification_receipt).unwrap_or_else(|error| {
            panic!(
                "FS-16 should be able to remove stale verification projection `{}`: {error}",
                verification_receipt.display()
            )
        });
    }

    let status_after_projection_drift = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-16 status after dispatch/receipt projection drift",
    );
    let begin_task2 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_after_projection_drift["execution_fingerprint"]
                .as_str()
                .expect("FS-16 status should expose execution fingerprint before begin"),
        ],
        "FS-16 begin task 2 should remain legal when Task 1 has a current positive closure despite dispatch/receipt projection drift",
    );
    assert_eq!(begin_task2["active_task"], Value::from(2_u64));
    assert_eq!(begin_task2["active_step"], Value::from(1_u64));
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs14_projection_loss_does_not_route_to_close_current_task()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs14-projection-refresh-route");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs14-projection-refresh-route.md";
    let _task1_dispatch_id =
        internal_only_setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let task1_review_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs14-projection-task1-review.md");
    let task1_verification_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs14-projection-task1-verification.md");
    write_file(
        &task1_review_summary_path,
        "FS-14 projection-refresh fixture task 1 review summary.\n",
    );
    write_file(
        &task1_verification_summary_path,
        "FS-14 projection-refresh fixture task 1 verification summary.\n",
    );
    let close_task1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            task1_review_summary_path
                .to_str()
                .expect("FS-14 projection-refresh fixture task 1 review summary path"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task1_verification_summary_path
                .to_str()
                .expect("FS-14 projection-refresh fixture task 1 verification summary path"),
        ],
        "FS-14 projection-refresh fixture close task 1",
    );
    assert_eq!(close_task1["action"], Value::from("recorded"));

    let status_before_task2_begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-14 projection-refresh fixture status before task 2 begin",
    );
    let begin_task2_step1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_task2_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-14 projection-refresh fixture status should expose fingerprint"),
        ],
        "FS-14 projection-refresh fixture begin task 2 step 1",
    );
    let complete_task2_step1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-14 projection-refresh fixture completed task 2 step 1.",
            "--manual-verify-summary",
            "FS-14 projection-refresh fixture task 2 step 1 verification summary.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task2_step1["execution_fingerprint"]
                .as_str()
                .expect("FS-14 projection-refresh fixture begin task 2 should expose fingerprint"),
        ],
        "FS-14 projection-refresh fixture complete task 2 step 1",
    );
    let dispatch_task2 = internal_only_runtime_review_dispatch_authority_json(
        repo,
        state,
        plan_rel,
        ReviewDispatchScopeArg::Task,
        Some(2),
        "FS-14 projection-refresh fixture record task 2 dispatch",
    );
    assert_eq!(dispatch_task2["allowed"], Value::Bool(true));
    let _task2_dispatch_id = dispatch_task2["dispatch_id"]
        .as_str()
        .expect("FS-14 projection-refresh fixture task 2 dispatch should expose id")
        .to_owned();
    let task2_review_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs14-projection-task2-review.md");
    let task2_verification_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs14-projection-task2-verification.md");
    write_file(
        &task2_review_summary_path,
        "FS-14 projection-refresh fixture task 2 review summary.\n",
    );
    write_file(
        &task2_verification_summary_path,
        "FS-14 projection-refresh fixture task 2 verification summary.\n",
    );
    let close_task2 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--review-result",
            "pass",
            "--review-summary-file",
            task2_review_summary_path
                .to_str()
                .expect("FS-14 projection-refresh fixture task 2 review summary path"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task2_verification_summary_path
                .to_str()
                .expect("FS-14 projection-refresh fixture task 2 verification summary path"),
        ],
        "FS-14 projection-refresh fixture close task 2 before projection drift",
    );
    assert_eq!(
        close_task2["action"],
        Value::from("recorded"),
        "FS-14 projection-refresh fixture close task 2 before projection drift should record a current closure, got {close_task2:?}"
    );

    let execution_run_id = complete_task2_step1["execution_run_id"]
        .as_str()
        .expect("FS-14 projection-refresh fixture complete task 2 should expose run id")
        .to_owned();
    let branch = current_branch_name(repo);
    let unit_review_receipt = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("unit-review-{execution_run_id}-task-2-step-1.md"),
    );
    if unit_review_receipt.is_file() {
        fs::remove_file(&unit_review_receipt).unwrap_or_else(|error| {
            panic!(
                "FS-14 projection-refresh fixture should remove task 2 unit review receipt `{}`: {error}",
                unit_review_receipt.display()
            )
        });
    }
    let verification_receipt = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("task-verification-{execution_run_id}-task-2.md"),
    );
    if verification_receipt.is_file() {
        fs::remove_file(&verification_receipt).unwrap_or_else(|error| {
            panic!(
                "FS-14 projection-refresh fixture should remove task 2 verification receipt `{}`: {error}",
                verification_receipt.display()
            )
        });
    }

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                plan_rel,
                "--external-review-result-ready",
                "--json",
            ],
            &[],
            "FS-14 projection-refresh fixture workflow operator",
        ),
        "FS-14 projection-refresh fixture workflow operator",
    );
    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "status",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-14 projection-refresh fixture status",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["phase"],
        Value::from("document_release_pending"),
        "FS-14 projection-loss fixture should continue to the branch-closure lane once task closures are current, got {operator_json:?}"
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["next_action"],
        Value::from("advance late stage")
    );
    assert_eq!(operator_json["blocking_scope"], Value::from("branch"));
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-14 projection-refresh fixture should expose recommended command");
    assert!(
        recommended_command.contains("advance-late-stage")
            && !recommended_command.contains("close-current-task"),
        "FS-14 projection-loss fixture should not route projection loss through close-current-task, got {recommended_command}"
    );
    for (surface, value) in [("operator", &operator_json), ("status", &status_json)] {
        let serialized = serde_json::to_string(value).expect("route json should serialize");
        assert!(
            !serialized.contains("receipt")
                && !serialized.contains("task_review_dispatch_required")
                && !serialized.contains("prior_task_verification_missing")
                && !serialized.contains("execution_reentry_required"),
            "FS-14 projection-loss {surface} must not expose task-boundary repair language after current closure exists: {serialized}"
        );
    }
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs10_stale_follow_up_is_ignored_when_truth_is_current()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs10-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    let branch = current_branch_name(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    publish_authoritative_release_truth(repo, state, plan_rel, &release_path, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            (
                "review_state_repair_follow_up",
                Value::from("execution_reentry"),
            ),
            ("harness_phase", Value::from("ready_for_branch_completion")),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-10 workflow operator stale-follow-up ignore fixture",
        ),
        "FS-10 workflow operator stale-follow-up ignore fixture",
    );
    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-10 plan execution status stale-follow-up ignore fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["phase"],
        Value::from("ready_for_branch_completion"),
        "FS-10 stale persisted follow-up must not override live current truth"
    );
    assert_eq!(
        operator_json["next_action"],
        Value::from("finish branch"),
        "FS-10 stale persisted follow-up must not reroute away from ready-for-branch-completion"
    );
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs12_authoritative_run_identity_beats_preflight_for_begin_and_operator()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs12-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);
    let preflight_path = preflight_acceptance_state_path(repo, state);
    assert!(
        preflight_path.is_file(),
        "FS-12 fixture should have {} acceptance before deleting it",
        concat!("pre", "flight")
    );
    fs::remove_file(&preflight_path).expect(concat!(
        "FS-12 should be able to remove pre",
        "flight acceptance fixture state"
    ));
    write_file(
        &preflight_path,
        concat!("{ malformed pre", "flight acceptance fixture for FS-12"),
    );
    bind_explicit_reopen_repair_target(repo, state, &current_branch_name(repo), plan_rel, 1, 1, 1);

    let status_before_reopen = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!(
            "FS-12 status before reopen after deleting pre",
            "flight acceptance"
        ),
    );
    let reopened = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            concat!(
                "FS-12 reopen to force a begin path after pre",
                "flight deletion."
            ),
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-12 status should expose execution fingerprint before reopen"),
        ],
        concat!(
            "FS-12 reopen should succeed after deleting pre",
            "flight acceptance"
        ),
    );
    assert!(
        reopened["execution_run_id"].as_str().is_some(),
        "FS-12 reopen status should still surface authoritative execution_run_id: {reopened}"
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            concat!(
                "FS-12 workflow operator after deleting pre",
                "flight acceptance"
            ),
        ),
        concat!(
            "FS-12 workflow operator after deleting pre",
            "flight acceptance"
        ),
    );
    assert_ne!(
        operator_json["next_action"],
        Value::from(concat!("execution pre", "flight")),
        "FS-12 operator should not regress to execution {} when authoritative run identity exists",
        concat!("pre", "flight")
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-12 workflow operator should expose recommended command");
    assert!(
        recommended_command.starts_with("featureforge plan execution begin --plan "),
        "FS-12 workflow operator should surface a begin command when authoritative run identity exists, got {recommended_command}"
    );

    let resumed = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        concat!(
            "FS-12 run workflow/operator-surfaced begin command after deleting pre",
            "flight acceptance"
        ),
    );
    if resumed["action"].as_str() == Some("blocked") {
        assert_follow_up_blocker_parity_with_operator(
            &operator_json,
            &resumed,
            concat!(
                "FS-12 operator command-follow parity after deleting pre",
                "flight acceptance"
            ),
        );
    } else {
        assert_eq!(resumed["active_task"], Value::from(1_u64));
        assert_eq!(resumed["active_step"], Value::from(1_u64));
        assert!(
            resumed["execution_run_id"].as_str().is_some(),
            "FS-12 begin status should preserve authoritative execution_run_id"
        );
    }
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs13_hidden_gates_do_not_materialize_legacy_open_step_state_when_blocked()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs13-hidden-gate-materialization");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);
    bind_explicit_reopen_repair_target(repo, state, &current_branch_name(repo), plan_rel, 1, 1, 1);

    let status_before_reopen = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 hidden-gate materialization status before reopen baseline",
    );
    internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "FS-13 hidden-gate migration baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 hidden-gate baseline should expose execution fingerprint"),
        ],
        "FS-13 hidden-gate materialization baseline reopen",
    );

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[("current_open_step_state", Value::Null)],
    );
    let status_without_authoritative_open_step =
        internal_only_run_plan_execution_json_direct_or_cli(
            repo,
            state,
            &["status", "--plan", plan_rel],
            "FS-13 status should ignore legacy markdown open-step note when authoritative current_open_step_state is absent",
        );
    assert_eq!(
        status_without_authoritative_open_step["active_task"],
        Value::Null,
        "FS-13 status must not derive an active task from raw markdown when authoritative current_open_step_state is absent",
    );
    assert_eq!(
        status_without_authoritative_open_step["active_step"],
        Value::Null,
        "FS-13 status must not derive an active step from raw markdown when authoritative current_open_step_state is absent",
    );
    assert_eq!(
        status_without_authoritative_open_step["resume_task"],
        Value::Null,
        "FS-13 status must not derive a resume task from raw markdown when authoritative current_open_step_state is absent",
    );
    assert_eq!(
        status_without_authoritative_open_step["resume_step"],
        Value::Null,
        "FS-13 status must not derive a resume step from raw markdown when authoritative current_open_step_state is absent",
    );

    let operator_without_authoritative_open_step = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-13 operator should ignore legacy markdown open-step note when authoritative state is absent",
        ),
        "FS-13 operator should ignore legacy markdown open-step note when authoritative state is absent",
    );
    let recommended_without_authority =
        operator_without_authoritative_open_step["recommended_command"]
            .as_str()
            .unwrap_or("");
    assert!(
        !recommended_without_authority.contains("--task 1 --step 1"),
        "FS-13 operator must not surface Task 1 Step 1 solely from raw markdown when authoritative current_open_step_state is absent: {operator_without_authoritative_open_step:?}",
    );

    let preflight = internal_only_unit_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        concat!("FS-13 hidden-gate pre", "flight blocked lane"),
    );
    assert_eq!(preflight["allowed"], Value::Bool(true));
    let authoritative_state_path = harness_state_path(state, &repo_slug(repo), &branch);
    let preflight_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path).expect(concat!(
            "FS-13 hidden-gate pre",
            "flight state should remain readable"
        )),
    )
    .expect(concat!(
        "FS-13 hidden-gate pre",
        "flight state should remain valid json"
    ));
    assert!(
        preflight_state["current_open_step_state"].is_null(),
        "FS-13 hidden-gate {} must not recreate current_open_step_state from raw markdown notes: {:?}",
        concat!("pre", "flight"),
        preflight_state
    );

    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[("current_open_step_state", Value::Null)],
    );

    let gate_review = internal_only_unit_plan_execution_gate_review_json(
        repo,
        state,
        plan_rel,
        concat!("FS-13 hidden-gate gate", "-review blocked lane"),
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    let gate_review_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path).expect(concat!(
            "FS-13 hidden-gate gate",
            "-review state should remain readable"
        )),
    )
    .expect(concat!(
        "FS-13 hidden-gate gate",
        "-review state should remain valid json"
    ));
    assert!(
        gate_review_state["current_open_step_state"].is_null(),
        "FS-13 hidden-gate {} must not recreate current_open_step_state from raw markdown notes: {gate_review_state:?}",
        concat!("gate", "-review")
    );

    fs::remove_file(&authoritative_state_path).expect(concat!(
        "FS-13 hidden-gate missing-state pre",
        "flight setup should remove authoritative state"
    ));
    let preflight_missing_state = internal_only_unit_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        concat!(
            "FS-13 hidden-gate pre",
            "flight blocked lane with missing authoritative state"
        ),
    );
    assert_eq!(preflight_missing_state["allowed"], Value::Bool(true));
    let restored_after_preflight: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path).expect(concat!(
            "FS-13 hidden-gate pre",
            "flight may recreate authoritative state for accepted pre",
            "flight"
        )),
    )
    .expect(concat!(
        "FS-13 hidden-gate pre",
        "flight state should remain valid json"
    ));
    assert!(
        restored_after_preflight["current_open_step_state"].is_null(),
        "FS-13 hidden-gate {} must not recreate current_open_step_state from raw markdown notes when bootstrapping missing authoritative state: {:?}",
        concat!("pre", "flight"),
        restored_after_preflight
    );

    if let Err(error) = fs::remove_file(&authoritative_state_path) {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::NotFound,
            "FS-13 hidden-gate missing-state {} setup should only tolerate an already-missing authoritative state",
            concat!("gate", "-review"),
        );
    }
    let gate_review_missing_state = internal_only_unit_plan_execution_gate_review_json(
        repo,
        state,
        plan_rel,
        concat!(
            "FS-13 hidden-gate gate",
            "-review blocked lane with missing authoritative state"
        ),
    );
    assert_eq!(gate_review_missing_state["allowed"], Value::Bool(false));
    if authoritative_state_path.exists() {
        let restored_after_gate_review: Value = serde_json::from_str(
            &fs::read_to_string(&authoritative_state_path).expect(concat!(
                "FS-13 hidden-gate gate",
                "-review recreated state should remain readable"
            )),
        )
        .expect(concat!(
            "FS-13 hidden-gate gate",
            "-review recreated state should remain valid json"
        ));
        assert!(
            restored_after_gate_review["current_open_step_state"].is_null(),
            "FS-13 hidden-gate {} must not recreate current_open_step_state from raw markdown notes when bootstrapping missing authoritative state: {restored_after_gate_review:?}",
            concat!("gate", "-review")
        );
    }
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs13_hidden_gates_fail_closed_on_malformed_authoritative_harness_state()
 {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs13-hidden-gates-malformed-harness-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let branch = current_branch_name(repo);
    let authoritative_state_path = harness_state_path(state, &repo_slug(repo), &branch);
    write_file(&authoritative_state_path, "{ this is not valid json }");

    for (output, context) in [
        (
            internal_only_unit_plan_execution_output(
                plan_execution_direct_support::internal_only_runtime_preflight_gate_json(
                    repo,
                    state,
                    &PlanExecutionStatusArgs {
                        plan: plan_rel.into(),
                        external_review_result_ready: false,
                    },
                ),
            ),
            concat!(
                "FS-13 malformed authoritative harness state should fail closed in hidden pre",
                "flight"
            ),
        ),
        (
            internal_only_unit_plan_execution_output(
                plan_execution_direct_support::internal_only_runtime_review_gate_json(
                    repo,
                    state,
                    &PlanExecutionStatusArgs {
                        plan: plan_rel.into(),
                        external_review_result_ready: false,
                    },
                ),
            ),
            concat!(
                "FS-13 malformed authoritative harness state should fail closed in hidden gate",
                "-review"
            ),
        ),
    ] {
        assert!(
            !output.status.success(),
            "{context} should not succeed when authoritative harness state is malformed"
        );
        let failure_payload = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        let failure_json: Value =
            serde_json::from_slice(failure_payload).expect("failure payload should be valid json");
        assert_eq!(failure_json["error_class"], "MalformedExecutionState");
        assert!(
            failure_json["message"].as_str().is_some_and(|message| {
                message.contains("Authoritative harness state is malformed")
            }),
            "{context} failure should mention malformed authoritative harness state, got {failure_json:?}"
        );
    }
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs13_status_and_hidden_gates_fail_closed_on_malformed_authoritative_open_step_state()
 {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs13-malformed-authoritative-open-step-read-paths");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);
    bind_explicit_reopen_repair_target(repo, state, &current_branch_name(repo), plan_rel, 1, 1, 1);

    let status_before_reopen = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 malformed open-step read-path baseline status before reopen",
    );
    internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "FS-13 malformed read-path baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 malformed read-path baseline should expose execution fingerprint"),
        ],
        "FS-13 malformed open-step read-path baseline reopen",
    );
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[(
            "current_open_step_state",
            serde_json::json!({
                "task": 1,
                "step": 1,
                "note_state": "not-a-state",
                "note_summary": "FS-13 malformed read-path authoritative open-step state",
                "source_plan_path": plan_rel,
                "source_plan_revision": 1,
                "authoritative_sequence": 2
            }),
        )],
    );

    for (output, context) in [
        (
            run_rust_featureforge_with_env(
                repo,
                state,
                &["plan", "execution", "status", "--plan", plan_rel],
                &[],
                "FS-13 malformed authoritative current_open_step_state status should fail closed",
            ),
            "FS-13 malformed authoritative current_open_step_state status should fail closed",
        ),
        (
            internal_only_unit_plan_execution_output(
                plan_execution_direct_support::internal_only_runtime_preflight_gate_json(
                    repo,
                    state,
                    &PlanExecutionStatusArgs {
                        plan: plan_rel.into(),
                        external_review_result_ready: false,
                    },
                ),
            ),
            concat!(
                "FS-13 malformed authoritative current_open_step_state pre",
                "flight should fail closed"
            ),
        ),
        (
            internal_only_unit_plan_execution_output(
                plan_execution_direct_support::internal_only_runtime_review_gate_json(
                    repo,
                    state,
                    &PlanExecutionStatusArgs {
                        plan: plan_rel.into(),
                        external_review_result_ready: false,
                    },
                ),
            ),
            concat!(
                "FS-13 malformed authoritative current_open_step_state gate",
                "-review should fail closed"
            ),
        ),
    ] {
        assert!(
            !output.status.success(),
            "{context} should not succeed when authoritative current_open_step_state is malformed"
        );
        let failure_payload = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        let failure_json: Value =
            serde_json::from_slice(failure_payload).expect("failure payload should be valid json");
        assert_eq!(failure_json["error_class"], "MalformedExecutionState");
        assert!(
            failure_json["message"]
                .as_str()
                .is_some_and(|message| message.contains("current_open_step_state")),
            "{context} failure should mention current_open_step_state, got {failure_json:?}"
        );
    }
}
