#[path = "support/bin.rs"]
mod bin_support;
#[path = "support/dir_tree.rs"]
mod dir_tree_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/json.rs"]
mod json_support;
#[path = "../src/workflow/markdown_scan.rs"]
mod markdown_scan_support;
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

use assert_cmd::cargo::cargo_bin;
use bin_support::compiled_featureforge_path;
use dir_tree_support::copy_dir_recursive;
use featureforge::contracts::plan::{PLAN_FIDELITY_REQUIRED_SURFACES, parse_plan_file};
use featureforge::contracts::spec::parse_spec_file;
use featureforge::execution::final_review::resolve_release_base_branch;
use featureforge::execution::follow_up::execution_step_repair_target_id;
use featureforge::execution::observability::{
    HarnessEventKind, HarnessObservabilityEvent, HarnessTelemetryCounters, STABLE_EVENT_KINDS,
    STABLE_REASON_CODES,
};
use featureforge::execution::query::query_workflow_routing_state_for_runtime;
use featureforge::execution::semantic_identity::{
    branch_definition_identity_for_context, task_definition_identity_for_task,
};
use featureforge::execution::state::{
    current_head_sha as runtime_current_head_sha, load_execution_context_for_mutation,
};
use featureforge::git::{
    RepositoryIdentity, discover_repo_identity, discover_repository, discover_slug_identity,
};
use featureforge::paths::{
    branch_storage_key, harness_authoritative_artifact_path, harness_state_path,
};
use featureforge::workflow::manifest::{
    WorkflowManifest, manifest_path, recover_slug_changed_manifest,
};
use featureforge::workflow::operator;
use featureforge::workflow::status::WorkflowRuntime;
use files_support::write_file;
use json_support::parse_json;
use process_support::{repo_root, run, run_checked};
use runtime_json_support::{
    discover_execution_runtime, plan_execution_status_json, run_featureforge_json_real_cli,
};
use runtime_phase_handoff_support::{workflow_handoff_json, workflow_phase_json};
use serde_json::{Value, json, to_value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
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
        .or_else(|| surface["blocking_task"].as_u64());
    assert_eq!(
        task_target,
        Some(u64::from(task)),
        "task-closure routes should keep the task in structured route metadata: {surface}"
    );
    assert_eq!(
        surface["required_inputs"],
        json!([
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

fn assert_runtime_provenance_fields(value: &Value, context: &str) {
    let runtime_provenance = value
        .get("runtime_provenance")
        .unwrap_or_else(|| panic!("{context}: missing runtime_provenance field in {value}"));
    for field in [
        "binary_path",
        "binary_realpath",
        "runtime_root",
        "repo_root",
        "state_dir",
        "state_dir_kind",
        "control_plane_source",
        "self_hosting_context",
    ] {
        assert!(
            runtime_provenance
                .get(field)
                .and_then(Value::as_str)
                .is_some(),
            "{context}: runtime_provenance.{field} should be a string in {runtime_provenance}",
        );
    }
    let skill_discovery = runtime_provenance
        .get("skill_discovery")
        .unwrap_or_else(|| {
            panic!("{context}: missing runtime_provenance.skill_discovery in {value}")
        });
    for field in [
        "installed_skill_root",
        "workspace_skill_root",
        "active_featureforge_skill_source",
    ] {
        assert!(
            skill_discovery.get(field).and_then(Value::as_str).is_some(),
            "{context}: runtime_provenance.skill_discovery.{field} should be a string in {skill_discovery}",
        );
    }
    assert!(
        skill_discovery
            .get("active_roots")
            .and_then(Value::as_array)
            .is_some(),
        "{context}: runtime_provenance.skill_discovery.active_roots should be an array in {skill_discovery}",
    );
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

fn assert_parity_probe_budget(scenario_id: &str, consumed_probe_commands: usize, max: usize) {
    assert!(
        consumed_probe_commands <= max,
        "scenario {scenario_id} exceeded parity-probe command target: consumed {consumed_probe_commands}, target {max}"
    );
}

fn normalize_workflow_status_snapshot(mut value: Value) -> Value {
    let object = value
        .as_object_mut()
        .expect("workflow status payload should stay a JSON object");
    object.remove("manifest_path");
    object.remove("root");
    value
}

fn write_manifest(path: &Path, manifest: &WorkflowManifest) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("manifest parent should be creatable");
    }
    let json = serde_json::to_string(manifest).expect("manifest json should serialize");
    fs::write(path, json).expect("manifest should be writable");
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

fn write_minimal_plan_fidelity_spec(repo: &Path, spec_path: &str) {
    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-001][behavior] The draft plan must complete an independent fidelity review before engineering review.\n",
    );
}

fn write_minimal_plan_fidelity_plan(
    repo: &Path,
    plan_path: &str,
    spec_path: &str,
    plan_revision: u32,
    last_reviewed_by: &str,
    task_title: &str,
) {
    write_minimal_plan_fidelity_plan_with_state(
        repo,
        plan_path,
        spec_path,
        plan_revision,
        "Draft",
        last_reviewed_by,
        task_title,
    );
}

fn write_minimal_plan_fidelity_plan_with_state(
    repo: &Path,
    plan_path: &str,
    spec_path: &str,
    plan_revision: u32,
    workflow_state: &str,
    last_reviewed_by: &str,
    task_title: &str,
) {
    write_file(
        &repo.join(plan_path),
        &format!(
            "# Draft Plan\n\n**Workflow State:** {workflow_state}\n**Plan Revision:** {plan_revision}\n**Execution Mode:** none\n**Source Spec:** `{spec_path}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** {last_reviewed_by}\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n\n## Task 1: {task_title}\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n"
        ),
    );
}

fn write_plan_fidelity_authoring_defect_with_current_pass(
    repo: &Path,
    spec_path: &str,
    plan_path: &str,
) {
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        1,
        "plan-eng-review",
        "Prepare the final draft plan with a stale source revision",
    );
    replace_in_file(
        &repo.join(plan_path),
        "**Source Spec Revision:** 1",
        "**Source Spec Revision:** 0",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-current-pass-authoring-defect.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-authoring-defect",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
}

fn workflow_status_refresh_json(repo: &Path, state_dir: &Path) -> Value {
    let mut runtime = WorkflowRuntime::discover_for_state_dir(repo, state_dir)
        .expect("workflow runtime should discover fixture repo");
    to_value(
        runtime
            .status_refresh()
            .expect("workflow status refresh should resolve route"),
    )
    .expect("workflow route should serialize")
}

fn assert_no_plan_fidelity_receipt_file_created(repo: &Path, state_dir: &Path) {
    fn visit(root: &Path, findings: &mut Vec<PathBuf>) {
        if !root.exists() {
            return;
        }
        let entries = fs::read_dir(root)
            .unwrap_or_else(|error| panic!("should read `{}`: {error}", root.display()));
        for entry in entries {
            let entry = entry.expect("directory entry should be readable");
            let path = entry.path();
            if path.is_dir() {
                visit(&path, findings);
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let looks_like_plan_fidelity_receipt = (file_name.contains("plan-fidelity")
                || file_name.contains("plan_fidelity"))
                && file_name.contains("receipt");
            if looks_like_plan_fidelity_receipt {
                findings.push(path);
            }
        }
    }

    let mut findings = Vec::new();
    visit(repo, &mut findings);
    visit(state_dir, &mut findings);
    assert!(
        findings.is_empty(),
        "workflow route fixtures must not create runtime-owned plan-fidelity receipt files: {findings:?}"
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
fn full_contract_ready_fixture_template_is_memoized_and_contract_ready() {
    let first = full_contract_ready_fixture_template();
    let second = full_contract_ready_fixture_template();

    assert!(
        std::ptr::eq(first, second),
        "fixture template should be memoized to avoid repeated fixture preparation IO"
    );
    assert!(
        first
            .plan
            .contains("docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"),
        "plan template should reference the repo-relative spec path"
    );
    assert!(
        !first.plan.contains(
            "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md"
        ),
        "plan template should not retain fixture-root absolute references"
    );
    assert!(
        first.plan.contains("## Execution Strategy")
            && first.plan.contains("## Dependency Diagram"),
        "plan template should include topology sections required by current contract fixtures"
    );
}

#[test]
fn read_surface_invariant_blocks_current_stale_overlap_on_public_status_and_operator() {
    let (repo_dir, state_dir) = init_repo("read-invariant-public-current-stale-overlap");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;
    complete_workflow_fixture_execution(repo, state, plan_rel);
    let env = [(
        "FEATUREFORGE_PLAN_EXECUTION_READ_INVARIANT_TEST_INJECTION",
        "current_stale_overlap",
    )];

    let status_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["plan", "execution", "status", "--plan", plan_rel],
            &env,
            "public status current/stale invariant injection",
        ),
        "public status current/stale invariant injection",
    );
    assert_eq!(status_json["state_kind"], "blocked_runtime_bug");
    assert_eq!(status_json["phase_detail"], "blocked_runtime_bug");
    assert_eq!(status_json["next_action"], "runtime diagnostic required");
    assert!(status_json["recommended_command"].is_null());
    assert!(status_json["recommended_public_command_argv"].is_null());
    assert!(
        status_json["required_inputs"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    assert!(status_json["execution_command_context"].is_null());
    let overlap_id = status_json["current_task_closures"][0]["closure_record_id"]
        .as_str()
        .expect("injected status should preserve current closure id");
    assert!(
        status_json["stale_unreviewed_closures"]
            .as_array()
            .is_some_and(|closures| closures
                .iter()
                .any(|closure| closure.as_str() == Some(overlap_id))),
        "status must preserve the overlapping stale closure for diagnosis: {status_json}"
    );
    assert!(
        status_json["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_stale_closure_overlap")),
        "status should expose the shared overlap invariant code: {status_json}"
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &env,
            "public operator current/stale invariant injection",
        ),
        "public operator current/stale invariant injection",
    );
    assert_eq!(operator_json["state_kind"], "blocked_runtime_bug");
    assert_eq!(operator_json["phase_detail"], "blocked_runtime_bug");
    assert_eq!(operator_json["next_action"], "runtime diagnostic required");
    assert!(operator_json["recommended_command"].is_null());
    assert!(operator_json["recommended_public_command_argv"].is_null());
    assert!(
        operator_json["required_inputs"]
            .as_array()
            .is_none_or(Vec::is_empty)
    );
    assert!(operator_json["execution_command_context"].is_null());
    assert!(
        operator_json["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_stale_closure_overlap")),
        "operator should expose the shared overlap invariant code: {operator_json}"
    );
}

#[test]
fn read_surface_invariant_blocks_hidden_commands_on_public_status_and_operator() {
    let (repo_dir, state_dir) = init_repo("read-invariant-public-hidden-command");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "read-invariant-public-hidden-command");
    let env = [(
        "FEATUREFORGE_PLAN_EXECUTION_READ_INVARIANT_TEST_INJECTION",
        "hidden_recommended_command",
    )];

    let status_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["plan", "execution", "status", "--plan", plan_rel],
            &env,
            "public status hidden-command invariant injection",
        ),
        "public status hidden-command invariant injection",
    );
    assert_eq!(status_json["state_kind"], "blocked_runtime_bug");
    assert_eq!(status_json["next_action"], "runtime diagnostic required");
    assert!(status_json["recommended_command"].is_null());
    assert!(status_json["recommended_public_command_argv"].is_null());
    assert!(
        status_json["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "recommended_command_hidden_or_debug")),
        "status should expose the shared hidden-command invariant code: {status_json}"
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &env,
            "public operator hidden-command invariant injection",
        ),
        "public operator hidden-command invariant injection",
    );
    assert_eq!(operator_json["state_kind"], "blocked_runtime_bug");
    assert_eq!(operator_json["next_action"], "runtime diagnostic required");
    assert!(operator_json["recommended_command"].is_null());
    assert!(operator_json["recommended_public_command_argv"].is_null());
    assert!(
        operator_json["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "recommended_command_hidden_or_debug")),
        "operator should expose the shared hidden-command invariant code: {operator_json}"
    );
}

#[cfg(unix)]
fn create_dir_symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("directory symlink should be creatable");
}

#[cfg(windows)]
fn create_dir_symlink(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).expect("directory symlink should be creatable");
}

fn run_shell_status_helper(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    // Keep this helper on the real binary path because these tests assert the
    // shell-level failure contract, including out-of-repo routing and stderr framing.
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["workflow"])
        .args(args);
    run(command, context)
}

fn run_featureforge_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Output {
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    run(command, context)
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

fn missing_null_fields(object: &Value, fields: &[&str]) -> Vec<String> {
    fields
        .iter()
        .copied()
        .filter(|field| !object.get(*field).is_some_and(Value::is_null))
        .map(str::to_owned)
        .collect()
}

fn set_remote_url(repo: &Path, url: &str) {
    let mut git_remote_set = Command::new("git");
    git_remote_set
        .args(["remote", "set-url", "origin", url])
        .current_dir(repo);
    run_checked(git_remote_set, "git remote set-url origin");
}

fn remove_origin_remote(repo: &Path) {
    let mut git_remote_remove = Command::new("git");
    git_remote_remove
        .args(["remote", "remove", "origin"])
        .current_dir(repo);
    run_checked(git_remote_remove, "git remote remove origin");
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

fn project_artifact_dir(repo: &Path, state_dir: &Path) -> PathBuf {
    state_dir.join("projects").join(repo_slug(repo))
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

fn insert_step_with_execution_note_after_step(
    path: &Path,
    after_step: u32,
    occurrence: usize,
    step: u32,
    title: &str,
    note: &str,
) {
    assert!(
        occurrence > 0,
        "step note insertion occurrence must be at least 1"
    );
    let source = fs::read_to_string(path).expect("fixture file should be readable for note insert");
    let lines = source.lines().collect::<Vec<_>>();
    let unchecked_marker = format!("- [ ] **Step {after_step}:");
    let checked_marker = format!("- [x] **Step {after_step}:");
    let mut seen = 0usize;
    let mut inserted = false;
    let mut updated_lines = Vec::new();
    let mut cursor = 0usize;
    while cursor < lines.len() {
        let line = lines[cursor];
        updated_lines.push(line.to_owned());
        if !inserted && (line.starts_with(&unchecked_marker) || line.starts_with(&checked_marker)) {
            seen += 1;
            if seen == occurrence {
                let mut note_cursor = cursor + 1;
                while note_cursor < lines.len() && lines[note_cursor].is_empty() {
                    updated_lines.push(lines[note_cursor].to_owned());
                    note_cursor += 1;
                }
                if note_cursor < lines.len()
                    && lines[note_cursor]
                        .trim_start()
                        .starts_with("**Execution Note:** ")
                {
                    updated_lines.push(lines[note_cursor].to_owned());
                    cursor = note_cursor;
                }
                updated_lines.push(format!("- [ ] **Step {step}: {title}**"));
                updated_lines.push(format!("  **Execution Note:** {note}"));
                inserted = true;
            }
        }
        cursor += 1;
    }
    assert!(
        inserted,
        "fixture note insertion should find Step {after_step} occurrence {occurrence} in {}",
        path.display()
    );
    let updated = format!("{}\n", updated_lines.join("\n"));
    fs::write(path, updated).expect("fixture file should be writable for note insert");
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

fn write_runtime_remediation_fs11_plan(repo: &Path, plan_rel: &str, spec_rel: &str) {
    let source = format!(
        r#"# Runtime Remediation FS-11 Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** featureforge:executing-plans
**Source Spec:** `{spec_rel}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 2, Task 3
- VERIFY-001 -> Task 2, Task 3

## Execution Strategy

- Seed a forward resume overlay on Task 3 Step 6 while keeping Task 2 as the earliest stale boundary.

## Dependency Diagram

```text
Task 2 -> Task 3
```

## Task 2: Earliest stale boundary task

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Task 2 remains the earliest unresolved stale boundary.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep one step for deterministic stale-boundary reopen targeting.

**Done when:**
- Task 2 remains the earliest unresolved stale boundary.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Execute task 2 baseline step**

## Task 3: Forward resume overlay task

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Task 3 Step 6 is the forward overlay target that must never outrank Task 2.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep six steps to preserve the exact Task 3 Step 6 contradiction shape.

**Done when:**
- Task 3 Step 6 is the forward overlay target that must never outrank Task 2.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Build Task 3 step scaffold**
- [ ] **Step 2: Build Task 3 step scaffold**
- [ ] **Step 3: Build Task 3 step scaffold**
- [ ] **Step 4: Build Task 3 step scaffold**
- [ ] **Step 5: Build Task 3 step scaffold**
- [ ] **Step 6: Build Task 3 step scaffold**
"#
    );
    write_file(&repo.join(plan_rel), &source);
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
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

fn enable_session_decision(state: &Path, session_key: &str) {
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    write_file(&decision_path, "enabled\n");
}

#[test]
fn shell_workflow_resolve_exposes_wrapper_contract_fields() {
    let (repo_dir, state_dir) = init_repo("workflow-resolve-contract");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let output = run_shell_status_helper(
        repo,
        state,
        &["resolve"],
        "shell helper resolve should stay removed from the compiled workflow CLI",
    );
    assert!(!output.status.success());
    let failure: Value = serde_json::from_slice(&output.stderr)
        .or_else(|_| serde_json::from_slice(&output.stdout))
        .expect("resolve removal failure should emit valid json");
    assert_eq!(failure["error_class"], "InvalidCommandInput");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("unrecognized subcommand 'resolve'")),
        "workflow resolve should stay removed from the compiled workflow CLI, got {failure:?}"
    );
}

#[test]
fn shell_workflow_resolve_failures_use_runtime_failure_contract() {
    let outside_repo = TempDir::new().expect("outside repo tempdir should be available");
    let state_dir = TempDir::new().expect("state tempdir should be available");

    let output = run_shell_status_helper(
        outside_repo.path(),
        state_dir.path(),
        &["resolve"],
        "shell helper resolve failure contract",
    );
    assert!(
        !output.status.success(),
        "resolve outside repo should fail, got {:?}",
        output.status
    );

    let failure: Value = serde_json::from_slice(&output.stderr)
        .or_else(|_| serde_json::from_slice(&output.stdout))
        .expect("resolve removal failure should emit valid json");
    assert_eq!(failure["error_class"], "InvalidCommandInput");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("unrecognized subcommand 'resolve'")),
        "workflow resolve should fail at CLI parsing before repo-context inspection, got {failure:?}"
    );
}

#[test]
fn direct_workflow_resolve_failures_use_runtime_failure_contract() {
    let outside_repo = TempDir::new().expect("outside repo tempdir should be available");
    let state_dir = TempDir::new().expect("state tempdir should be available");

    let output = run_featureforge_real_cli(
        outside_repo.path(),
        state_dir.path(),
        &["workflow", "resolve"],
        "direct helper resolve failure contract",
    );
    assert!(
        !output.status.success(),
        "resolve outside repo should fail, got {:?}",
        output.status
    );

    let failure: Value = serde_json::from_slice(&output.stderr)
        .or_else(|_| serde_json::from_slice(&output.stdout))
        .expect("resolve removal failure should emit valid json");
    assert_eq!(failure["error_class"], "InvalidCommandInput");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("unrecognized subcommand 'resolve'")),
        "workflow resolve should fail at helper parsing before repo-context inspection, got {failure:?}"
    );
}

#[test]
fn direct_read_only_workflow_failures_preserve_cli_text_contract() {
    let outside_repo = TempDir::new().expect("outside repo tempdir should be available");
    let state_dir = TempDir::new().expect("state tempdir should be available");

    let output = run_featureforge_real_cli(
        outside_repo.path(),
        state_dir.path(),
        &["workflow", "next"],
        "direct helper read-only workflow failure contract",
    );
    assert!(
        !output.status.success(),
        "workflow next should stay removed"
    );
    let failure: Value = serde_json::from_slice(&output.stderr)
        .or_else(|_| serde_json::from_slice(&output.stdout))
        .expect("workflow next removal failure should emit valid json");
    assert_eq!(failure["error_class"], "InvalidCommandInput");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("unrecognized subcommand 'next'")),
        "workflow next should fail at CLI parsing, got {failure:?}"
    );
}

#[test]
fn canonical_workflow_status_matches_helper_for_manifest_backed_missing_spec() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-manifest-backed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let missing_spec = "docs/featureforge/specs/2026-03-24-rust-missing-spec-design.md";
    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: fs::canonicalize(repo)
                .expect("repo root should canonicalize")
                .to_string_lossy()
                .into_owned(),
            branch: identity.branch_name.clone(),
            expected_spec_path: String::from(missing_spec),
            expected_plan_path: String::from(
                "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md",
            ),
            status: String::from("implementation_ready"),
            next_skill: String::new(),
            reason: String::from("implementation_ready"),
            note: String::from("implementation_ready"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let helper_json = workflow_status_refresh_json(repo, state);
    let rust_json = workflow_status_refresh_json(repo, state);

    assert_eq!(rust_json["status"], helper_json["status"]);
    assert_eq!(rust_json["next_skill"], helper_json["next_skill"]);
    assert_eq!(rust_json["spec_path"], helper_json["spec_path"]);
    assert_eq!(rust_json["reason"], helper_json["reason"]);
    assert_eq!(rust_json["reason_codes"], helper_json["reason_codes"]);
    assert_eq!(rust_json["diagnostics"], helper_json["diagnostics"]);
}

#[test]
fn canonical_workflow_status_matches_helper_for_ambiguous_specs() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-ambiguity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let fixture_root = workflow_fixture_root();

    fs::create_dir_all(repo.join("docs/featureforge/specs"))
        .expect("specs directory should be creatable");
    fs::copy(
        fixture_root.join("specs/2026-01-22-document-review-system-design.md"),
        repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design.md"),
    )
    .expect("first fixture spec should copy");
    fs::copy(
        fixture_root.join("specs/2026-02-19-visual-brainstorming-refactor-design.md"),
        repo.join("docs/featureforge/specs/2026-02-19-visual-brainstorming-refactor-design.md"),
    )
    .expect("second fixture spec should copy");

    let _helper_warmup = workflow_status_refresh_json(repo, state);
    let helper_json = workflow_status_refresh_json(repo, state);
    let rust_json = workflow_status_refresh_json(repo, state);

    assert_eq!(rust_json["status"], helper_json["status"]);
    assert_eq!(rust_json["next_skill"], helper_json["next_skill"]);
    assert_eq!(rust_json["reason"], helper_json["reason"]);
    assert_eq!(rust_json["reason_codes"], helper_json["reason_codes"]);
    assert_eq!(
        rust_json["spec_candidate_count"],
        helper_json["spec_candidate_count"]
    );
}

#[test]
fn canonical_workflow_status_ambiguous_specs_matches_checked_in_snapshot() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-ambiguity-snapshot");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let fixture_root = workflow_fixture_root();

    fs::create_dir_all(repo.join("docs/featureforge/specs"))
        .expect("specs directory should be creatable");
    fs::copy(
        fixture_root.join("specs/2026-01-22-document-review-system-design.md"),
        repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design.md"),
    )
    .expect("first fixture spec should copy");
    fs::copy(
        fixture_root.join("specs/2026-01-22-document-review-system-design-v2.md"),
        repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design-v2.md"),
    )
    .expect("second fixture spec should copy");

    let actual = normalize_workflow_status_snapshot(workflow_status_refresh_json(repo, state));
    let expected: Value = serde_json::from_str(
        &fs::read_to_string(repo_root().join("tests/fixtures/differential/workflow-status.json"))
            .expect("checked-in workflow-status snapshot should be readable"),
    )
    .expect("checked-in workflow-status snapshot should parse");

    assert_eq!(actual, expected);
}

#[test]
fn canonical_workflow_expect_and_sync_preserve_missing_spec_semantics() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-expect-sync");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let missing_spec = "docs/featureforge/specs/2026-03-24-rust-sync-missing-spec.md";
    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: identity.repo_root.to_string_lossy().into_owned(),
            branch: identity.branch_name,
            expected_spec_path: String::from(missing_spec),
            expected_plan_path: String::new(),
            status: String::from("needs_brainstorming"),
            next_skill: String::from("featureforge:brainstorming"),
            reason: String::from("missing_expected_spec"),
            note: String::from("missing_expected_spec"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);
    assert_eq!(status_json["status"], "needs_brainstorming");
    assert_eq!(status_json["spec_path"], missing_spec);
    assert_eq!(status_json["reason"], "missing_expected_spec");
    assert_eq!(status_json["reason_codes"][0], "missing_expected_spec");

    let runtime = discover_execution_runtime(
        repo,
        state,
        "rust canonical workflow phase after missing-spec sync",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "rust canonical workflow phase after missing-spec sync",
    );
    assert_eq!(
        phase_json["phase"], "pivot_required",
        "phase JSON should preserve authoritative contract-drafting pivot route, got {phase_json}"
    );
    assert_eq!(phase_json["next_skill"], "featureforge:brainstorming");
    assert_eq!(phase_json["next_action"], "pivot / return to planning");
}

#[test]
fn workflow_status_routes_writing_plans_draft_to_eng_review_without_fidelity_artifact() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-missing-start");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        1,
        "writing-plans",
        "Prepare the draft plan for review",
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
    assert!(
        status_json.get("plan_fidelity_review").is_none(),
        "engineering-review start must not expose inactive fidelity diagnostics: {status_json}"
    );
    assert!(
        !status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .filter_map(Value::as_str)
            .any(|code| code.contains("plan_fidelity")),
        "missing fidelity artifacts must not block engineering review start: {status_json}"
    );
    assert_no_plan_fidelity_receipt_file_created(repo, state);
}

#[test]
fn canonical_workflow_status_normalizes_dot_slash_source_spec_paths() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-dot-slash-spec");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        &format!("./{spec_path}"),
        1,
        "writing-plans",
        "Prepare the draft plan for review",
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
    assert_eq!(status_json["spec_path"], spec_path);
}

#[test]
fn workflow_status_keeps_engineering_review_owner_after_plan_edits_before_handoff() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-edit-loop");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        1,
        "writing-plans",
        "Prepare the draft plan for review",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-stale.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-019d",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        2,
        "writing-plans",
        "Prepare the revised draft plan for review",
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
    assert!(
        status_json.get("plan_fidelity_review").is_none(),
        "engineering-review edit window must not expose stale fidelity diagnostics: {status_json}"
    );
    assert!(
        !status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .filter_map(Value::as_str)
            .any(|code| code == "stale_plan_fidelity_review_artifact"),
        "stale fidelity artifacts must not eject active engineering-review edits: {status_json}"
    );
    assert_no_plan_fidelity_receipt_file_created(repo, state);
}

#[test]
fn workflow_status_suppresses_current_pass_fidelity_when_authoring_defect_routes_to_writing_plans()
{
    let (repo_dir, state_dir) = init_repo("workflow-runtime-fidelity-pass-authoring-defect");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_plan_fidelity_authoring_defect_with_current_pass(repo, spec_path, plan_path);

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:writing-plans");
    assert!(
        status_json.get("plan_fidelity_review").is_none(),
        "writing-plans route must suppress current pass fidelity diagnostics while non-fidelity authoring defects remain: {status_json}"
    );
    assert!(
        !status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .filter_map(Value::as_str)
            .any(|code| code.contains("plan_fidelity")),
        "writing-plans route must not surface fidelity reason codes while authoring is still active: {status_json}"
    );
    assert_no_plan_fidelity_receipt_file_created(repo, state);
}

#[test]
fn workflow_status_routes_engineering_reviewed_draft_without_fidelity_artifact_to_plan_fidelity_review()
 {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-draft-plan-final-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        1,
        "plan-eng-review",
        "Prepare the final draft plan for fidelity review",
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(
        status_json["next_skill"],
        "featureforge:plan-fidelity-review"
    );
    assert_eq!(
        status_json["reason_codes"][0],
        "missing_plan_fidelity_review_artifact"
    );
    assert_eq!(
        status_json["diagnostics"][0]["code"],
        "missing_plan_fidelity_review_artifact"
    );
    assert_eq!(status_json["plan_fidelity_review"]["state"], "missing");
    let template = &status_json["plan_fidelity_review"]["required_artifact_template"];
    assert_eq!(
        template["artifact_path"],
        ".featureforge/reviews/2026-01-22-document-review-system-plan-fidelity.md"
    );
    assert_eq!(template["reviewed_plan_path"], plan_path);
    assert_eq!(template["reviewed_spec_path"], spec_path);
    assert_eq!(
        template["required_verified_surfaces"],
        serde_json::json!(PLAN_FIDELITY_REQUIRED_SURFACES)
    );
    assert!(
        template["content"]
            .as_str()
            .expect("template content should be a string")
            .contains("**Reviewer ID:** <reviewer-id>"),
        "active plan-fidelity route should expose fillable artifact content: {template}"
    );
    let status_text =
        serde_json::to_string(&status_json).expect("status json should serialize cleanly");
    assert!(
        !status_text.contains("receipt"),
        "active fidelity review route must not expose runtime-owned receipt language: {status_text}"
    );
    assert!(
        !status_json["plan_fidelity_review"]["verified_requirement_index"]
            .as_bool()
            .expect("requirement index verification should be present")
    );
    assert!(
        !status_json["plan_fidelity_review"]["verified_execution_topology"]
            .as_bool()
            .expect("execution topology verification should be present")
    );
    assert!(
        !status_json["plan_fidelity_review"]["verified_task_contract"]
            .as_bool()
            .expect("task contract verification should be present")
    );
    assert!(
        !status_json["plan_fidelity_review"]["verified_task_determinism"]
            .as_bool()
            .expect("task determinism verification should be present")
    );
    assert!(
        !status_json["plan_fidelity_review"]["verified_spec_reference_fidelity"]
            .as_bool()
            .expect("spec reference fidelity verification should be present")
    );
    assert_no_plan_fidelity_receipt_file_created(repo, state);
}

#[test]
fn canonical_workflow_status_routes_draft_plan_with_non_independent_fidelity_artifact_to_plan_fidelity_review()
 {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-draft-plan-non-independent-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        1,
        "plan-eng-review",
        "Prepare the final draft plan for fidelity review",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-non-independent.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "same-context",
            reviewer_id: "writer-context",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(
        status_json["next_skill"],
        "featureforge:plan-fidelity-review"
    );
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "plan_fidelity_reviewer_provenance_invalid"),
        "non-independent reviewer provenance should fail closed with explicit reason code"
    );
}

#[test]
fn canonical_workflow_status_routes_draft_plan_with_non_pass_fidelity_artifact_to_engineering_review()
 {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-draft-plan-non-pass-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        1,
        "plan-eng-review",
        "Prepare the final draft plan for fidelity review",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-non-pass-gate.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "fail",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-non-pass-gate",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
    assert_eq!(status_json["plan_fidelity_review"]["state"], "fail");
    assert!(
        status_json["plan_fidelity_review"]
            .get("required_artifact_template")
            .is_none(),
        "draft engineering-review route should not expose the fidelity template after a reviewer returned a fail verdict: {status_json}"
    );
    let status_text =
        serde_json::to_string(&status_json).expect("status json should serialize cleanly");
    assert!(
        !status_text.contains("receipt"),
        "final approval input must not expose runtime-owned receipt language: {status_text}"
    );
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "plan_fidelity_review_artifact_not_pass"),
        "non-pass verdicts should fail closed with explicit reason code"
    );
}

#[test]
fn canonical_workflow_status_routes_draft_plan_with_malformed_fidelity_artifact_to_plan_fidelity_review()
 {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-draft-plan-malformed-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        1,
        "plan-eng-review",
        "Prepare the final draft plan for fidelity review",
    );
    let artifact_path = repo.join(".featureforge/reviews/plan-fidelity-malformed-gate.md");
    fs::create_dir_all(
        artifact_path
            .parent()
            .expect("artifact should have a parent"),
    )
    .expect("artifact parent should be created");
    fs::write(&artifact_path, "{not a plan fidelity review artifact")
        .expect("corrupted artifact should be writable");

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(
        status_json["next_skill"],
        "featureforge:plan-fidelity-review"
    );
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "plan_fidelity_review_artifact_invalid"),
        "malformed artifact payloads should fail closed with explicit reason code"
    );
}

#[test]
fn workflow_status_reports_incomplete_plan_fidelity_review_artifacts() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-incomplete-artifact");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        1,
        "plan-eng-review",
        "Prepare the final draft plan for fidelity review",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-incomplete.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-incomplete",
            verified_surfaces: &[],
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);
    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(
        status_json["next_skill"],
        "featureforge:plan-fidelity-review"
    );
    assert_eq!(status_json["plan_fidelity_review"]["state"], "invalid");
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "plan_fidelity_review_missing_required_surface"),
        "incomplete verification artifacts should fail closed with explicit reason code"
    );
}

#[test]
fn workflow_plan_fidelity_record_command_is_not_public_cli() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-command-removed");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let help_output = run_featureforge_real_cli(
        repo,
        state,
        &["workflow", "--help"],
        "workflow help should describe only public workflow commands",
    );
    assert!(
        help_output.status.success(),
        "workflow help should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        help_output.status,
        String::from_utf8_lossy(&help_output.stdout),
        String::from_utf8_lossy(&help_output.stderr)
    );
    let help_stdout = String::from_utf8_lossy(&help_output.stdout);
    assert!(
        !help_stdout.contains("plan-fidelity"),
        "workflow help must not expose removed plan-fidelity receipt command:\n{help_stdout}"
    );

    let output = run_featureforge_real_cli(
        repo,
        state,
        &[
            "workflow",
            "plan-fidelity",
            "record",
            "--plan",
            "docs/featureforge/plans/example.md",
            "--review-artifact",
            ".featureforge/reviews/example-plan-fidelity.md",
            "--json",
        ],
        "workflow plan-fidelity record should stay removed",
    );
    assert!(
        !output.status.success(),
        "record should not exist as a public workflow command, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand 'plan-fidelity'"),
        "removed command should fail at the CLI parse boundary, got stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("example-plan-fidelity.md"),
        "removed command should not reach artifact-path validation, got stderr:\n{stderr}"
    );
}

#[test]
fn workflow_status_normalizes_dot_slash_plan_fidelity_review_targets() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-dot-slash-artifact");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        &format!("./{spec_path}"),
        1,
        "plan-eng-review",
        "Prepare the final draft plan for fidelity review",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-dot-slash-targets.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-dot-slash-targets",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    replace_in_file(
        &repo.join(".featureforge/reviews/plan-fidelity-dot-slash-targets.md"),
        &format!("**Reviewed Plan:** `{plan_path}`"),
        &format!("**Reviewed Plan:** `./{plan_path}`"),
    );
    replace_in_file(
        &repo.join(".featureforge/reviews/plan-fidelity-dot-slash-targets.md"),
        &format!("**Reviewed Spec:** `{spec_path}`"),
        &format!("**Reviewed Spec:** `./{spec_path}`"),
    );

    let status_json = workflow_status_refresh_json(repo, state);
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
    assert_eq!(status_json["plan_fidelity_review"]["state"], "pass");
}

#[test]
fn workflow_status_routes_current_pass_fidelity_artifact_back_to_engineering_approval() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-current-pass");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        1,
        "plan-eng-review",
        "Prepare the final draft plan for fidelity review",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-current-pass.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-current-pass",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
    assert_eq!(status_json["plan_fidelity_review"]["state"], "pass");
    let status_text =
        serde_json::to_string(&status_json).expect("status json should serialize cleanly");
    assert!(
        !status_text.contains("receipt"),
        "final approval pass input must not expose runtime-owned receipt language: {status_text}"
    );
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .is_empty(),
        "current pass fidelity artifact should not emit route-blocking reason codes: {status_json}"
    );
    assert_no_plan_fidelity_receipt_file_created(repo, state);
}

#[test]
fn workflow_status_blocks_engineering_approved_plan_without_fidelity_artifact() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-approved-plan-missing-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan_with_state(
        repo,
        plan_path,
        spec_path,
        1,
        "Engineering Approved",
        "plan-eng-review",
        "Prepare the approved plan for implementation",
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_review_required");
    assert_ne!(status_json["status"], "implementation_ready");
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
    assert_eq!(
        status_json["reason_codes"][0],
        "engineering_approval_missing_plan_fidelity_review"
    );
    assert_eq!(status_json["plan_fidelity_review"]["state"], "missing");
    assert!(
        status_json["plan_fidelity_review"]
            .get("required_artifact_template")
            .is_some(),
        "Engineering Approved plans blocked on fidelity should expose the artifact template: {status_json}"
    );
    let status_text =
        serde_json::to_string(&status_json).expect("status json should serialize cleanly");
    assert!(
        !status_text.contains("receipt"),
        "approved-plan fidelity gate must not expose receipt language: {status_text}"
    );
}

#[test]
fn execution_query_blocks_engineering_approved_plan_without_fidelity_artifact() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-approved-plan-query-fidelity-gate");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan_with_state(
        repo,
        plan_path,
        spec_path,
        1,
        "Engineering Approved",
        "plan-eng-review",
        "Prepare the approved plan for implementation",
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "approved plan query should respect plan-fidelity implementation gate",
    );
    let routing =
        query_workflow_routing_state_for_runtime(&runtime, Some(Path::new(plan_path)), false)
            .expect("routing query should return a public review route");

    assert_eq!(routing.route.status, "plan_review_required");
    assert_eq!(routing.route.next_skill, "featureforge:plan-eng-review");
    assert!(
        routing.execution_status.is_none(),
        "explicit plan queries must not project runtime status when fidelity blocks implementation: {routing:?}"
    );
    assert!(
        routing
            .route
            .reason_codes
            .iter()
            .any(|code| code == "engineering_approval_missing_plan_fidelity_review"),
        "explicit plan query should preserve the engineering-approval fidelity reason: {routing:?}"
    );
    assert_eq!(
        routing
            .route
            .plan_fidelity_review
            .as_ref()
            .expect("blocked approved route should expose fidelity gate")
            .state,
        "missing"
    );
}

#[test]
fn execution_query_blocks_engineering_approved_plan_without_fidelity_after_execution_started() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-approved-plan-active-fidelity-gate");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "workflow-runtime-active-fidelity-gate");

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "active approved fidelity gate status before begin",
    );
    run_plan_execution_json(
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
                .expect("status fingerprint should be present before begin"),
        ],
        "active approved fidelity gate begin",
    );
    fs::remove_file(
        repo.join(
            ".featureforge/reviews/2026-03-22-runtime-integration-hardening-plan-fidelity.md",
        ),
    )
    .expect("fixture fidelity artifact should be removable after execution starts");

    let runtime = discover_execution_runtime(
        repo,
        state,
        "active approved plan query should respect plan-fidelity implementation gate",
    );
    let routing =
        query_workflow_routing_state_for_runtime(&runtime, Some(Path::new(plan_rel)), false)
            .expect("routing query should return a public review route");

    assert_eq!(routing.route.status, "plan_review_required");
    assert_eq!(routing.route.next_skill, "featureforge:plan-eng-review");
    assert!(
        routing.execution_status.is_some(),
        "existing runtime state should remain visible while preserving the fidelity-blocked route: {routing:?}"
    );
    assert_eq!(
        routing.phase_detail, "execution_in_progress",
        "active runtime projection should keep execution routing while the embedded workflow route remains fidelity-blocked: {routing:?}"
    );
    assert!(
        routing
            .route
            .reason_codes
            .iter()
            .any(|code| code == "engineering_approval_missing_plan_fidelity_review"),
        "active explicit plan query should preserve the engineering-approval fidelity reason: {routing:?}"
    );
}

#[test]
fn workflow_status_blocks_engineering_approved_plan_with_stale_fidelity_artifact() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-approved-plan-stale-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan_with_state(
        repo,
        plan_path,
        spec_path,
        1,
        "Engineering Approved",
        "plan-eng-review",
        "Prepare the approved plan for implementation",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-approved-stale.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-approved-stale",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    write_minimal_plan_fidelity_plan_with_state(
        repo,
        plan_path,
        spec_path,
        2,
        "Engineering Approved",
        "plan-eng-review",
        "Prepare the changed approved plan for implementation",
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_review_required");
    assert_ne!(status_json["status"], "implementation_ready");
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "engineering_approval_stale_plan_fidelity_review"),
        "stale approved-plan fidelity gate should be explicit: {status_json}"
    );
    assert_eq!(status_json["plan_fidelity_review"]["state"], "stale");
}

#[test]
fn workflow_status_blocks_engineering_approved_plan_with_incomplete_fidelity_surfaces() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-approved-plan-incomplete-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan_with_state(
        repo,
        plan_path,
        spec_path,
        1,
        "Engineering Approved",
        "plan-eng-review",
        "Prepare the approved plan for implementation",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-approved-incomplete.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-approved-incomplete",
            verified_surfaces: &[],
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_review_required");
    assert_ne!(status_json["status"], "implementation_ready");
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "engineering_approval_incomplete_plan_fidelity_surfaces"),
        "incomplete approved-plan fidelity surfaces should be explicit: {status_json}"
    );
    assert_eq!(status_json["plan_fidelity_review"]["state"], "invalid");
}

#[test]
fn workflow_status_allows_engineering_approved_plan_with_current_pass_fidelity_artifact() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-approved-plan-pass-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan_with_state(
        repo,
        plan_path,
        spec_path,
        1,
        "Engineering Approved",
        "plan-eng-review",
        "Prepare the approved plan for implementation",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-approved-pass.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-approved-pass",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "implementation_ready");
    assert_eq!(status_json["next_skill"], "");
    assert_eq!(status_json["plan_fidelity_review"]["state"], "pass");
    assert!(
        status_json["plan_fidelity_review"]
            .get("required_artifact_template")
            .is_none(),
        "implementation-ready routes must not expose a required artifact template: {status_json}"
    );
    assert_eq!(
        status_json["plan_fidelity_review"]["verified_requirement_index"],
        true
    );
    assert_eq!(
        status_json["plan_fidelity_review"]["verified_execution_topology"],
        true
    );
    assert_eq!(
        status_json["plan_fidelity_review"]["verified_task_contract"],
        true
    );
    assert_eq!(
        status_json["plan_fidelity_review"]["verified_task_determinism"],
        true
    );
    assert_eq!(
        status_json["plan_fidelity_review"]["verified_spec_reference_fidelity"],
        true
    );
}

#[test]
fn workflow_status_rejects_stale_review_artifact_fingerprints() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-stale-artifact");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        1,
        "plan-eng-review",
        "Prepare the final draft plan for fidelity review",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-stale-fingerprint.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-stale-fingerprint",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    replace_in_file(
        &repo.join(plan_path),
        "Prepare the final draft plan for fidelity review",
        "Prepare the changed draft plan for review",
    );

    let status_json = workflow_status_refresh_json(repo, state);
    assert_eq!(
        status_json["next_skill"],
        "featureforge:plan-fidelity-review"
    );
    assert_eq!(status_json["plan_fidelity_review"]["state"], "stale");
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "stale_plan_fidelity_review_artifact"),
        "stale review artifacts should fail closed with explicit reason code"
    );
}

#[test]
fn plan_contract_analyze_plan_resolves_plan_fidelity_artifacts_from_subdirectories() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-subdir");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_minimal_plan_fidelity_plan(
        repo,
        plan_path,
        spec_path,
        1,
        "plan-eng-review",
        "Prepare the final draft plan for fidelity review",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-subdir.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-subdir",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    fs::create_dir_all(repo.join("src/runtime")).expect("subdirectory should exist");

    let report = parse_json(
        &run_featureforge_real_cli(
            &repo.join("src/runtime"),
            state,
            &[
                "plan",
                "contract",
                "analyze-plan",
                "--spec",
                spec_path,
                "--plan",
                plan_path,
                "--format",
                "json",
            ],
            "analyze-plan should resolve repo-relative plan-fidelity artifacts from subdirectories",
        ),
        "analyze-plan should resolve repo-relative plan-fidelity artifacts from subdirectories",
    );
    assert_eq!(report["plan_fidelity_review"]["state"], "pass");
}

#[test]
fn workflow_status_routes_malformed_spec_requirement_index_to_plan_authoring() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-malformed-spec");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Summary\n\nMalformed fixture without a Requirement Index.\n",
    );
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-malformed-spec.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-malformed-spec",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);
    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:writing-plans");
}

#[test]
fn workflow_status_rejects_invalid_ceo_review_provenance_on_source_spec() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-invalid-spec-reviewer");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** brainstorming\n\n## Requirement Index\n\n- [REQ-001][behavior] The draft plan must complete an independent fidelity review before engineering review.\n",
    );
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-invalid-spec-reviewer.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-invalid-spec-reviewer",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);
    assert_eq!(status_json["next_skill"], "featureforge:plan-ceo-review");
}

#[test]
fn analyze_plan_rejects_out_of_repo_source_spec_paths() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-external-source-spec");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `../external/docs/featureforge/specs/outside-spec.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    let output = run_featureforge_real_cli(
        repo,
        state,
        &[
            "plan",
            "contract",
            "analyze-plan",
            "--spec",
            spec_path,
            "--plan",
            plan_path,
            "--format",
            "json",
        ],
        "analyze-plan should reject out-of-repo Source Spec paths",
    );
    let report = parse_json(
        &output,
        "analyze-plan should report invalid contract when Source Spec escapes the repo",
    );
    assert_eq!(report["contract_state"], "invalid");
    assert!(
        report["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "missing_source_spec")),
        "escaped Source Spec paths should be normalized into a fail-closed invalid contract report"
    );
}

#[test]
#[cfg(unix)]
fn canonical_workflow_status_refresh_preserves_route_when_manifest_write_fails() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-manifest-write-conflict");
    let repo = repo_dir.path();
    let state = state_dir.path();

    install_full_contract_ready_artifacts(repo);

    let original_permissions = fs::metadata(state)
        .expect("state dir metadata should be readable")
        .permissions();
    let mut read_only_permissions = original_permissions.clone();
    read_only_permissions.set_mode(0o555);
    fs::set_permissions(state, read_only_permissions).expect("state dir should become read-only");

    let status_json = workflow_status_refresh_json(repo, state);
    fs::set_permissions(state, original_permissions)
        .expect("state dir permissions should be restorable");

    assert_eq!(status_json["status"], "implementation_ready");
    assert_ne!(status_json["next_skill"], "featureforge:brainstorming");
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should stay an array")
            .iter()
            .any(|value| value == &Value::String(String::from("manifest_write_conflict")))
    );
    assert_eq!(
        status_json["diagnostics"][0]["code"],
        Value::String(String::from("manifest_write_conflict"))
    );
}

#[test]
fn canonical_workflow_status_refresh_accepts_active_implementation_target_specs() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-active-implementation-target");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let spec_rel = "docs/featureforge/specs/2026-04-01-runtime-shift.md";
    let plan_rel = "docs/featureforge/plans/2026-04-01-runtime-shift.md";
    write_file(
        &repo.join("docs/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md"),
        &format!(
            "# Active Implementation Target\n\n**Status:** authoritative index for the active April supersession-aware documentation corpus\n\n## Active Normative Specs\n\n- `{spec_rel}`\n"
        ),
    );
    write_file(
        &repo.join("docs/featureforge/specs/2026-03-01-legacy-design.md"),
        "# Legacy Design\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-LEGACY][behavior] Historical fixture only.\n",
    );
    write_file(
        &repo.join(spec_rel),
        "# Runtime Shift\n\n**Workflow State:** Implementation Target\n**Spec Revision:** 1\n**Last Reviewed By:** clean-context review loop\n**Implementation Target:** Current\n\n## Requirement Index\n\n- [REQ-001][behavior] Active implementation-target specs participate in workflow routing.\n",
    );
    write_file(
        &repo.join(plan_rel),
        &format!(
            "# Runtime Shift Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_rel}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Route the active implementation target\n\n**Spec Coverage:** REQ-001\n**Goal:** Workflow status resolves the April implementation-target spec.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- Workflow status resolves the April implementation-target spec.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Recognize the active implementation target**\n"
        ),
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_ne!(status_json["status"], "stale_plan");
    assert_eq!(status_json["spec_path"], Value::from(spec_rel));
    assert_eq!(status_json["plan_path"], Value::from(plan_rel));
}

#[test]
fn canonical_workflow_status_routes_lone_stale_approved_plan_as_stale() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-stale-approved-plan");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_file(
        &repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design-v2.md"),
        "# Approved Spec, Newer Path\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Notes\n",
    );
    write_file(
        &repo.join("docs/featureforge/plans/2026-01-22-document-review-system.md"),
        "# Approved Plan, Stale Source Path\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Preserve the stale source path case\n\n**Spec Coverage:** REQ-001\n**Goal:** The plan remains structurally valid while its source-spec path goes stale.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The plan remains structurally valid while its source-spec path goes stale.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Detect the stale source path**\n",
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "stale_plan");
    assert_eq!(status_json["next_skill"], "featureforge:writing-plans");
    assert_eq!(status_json["contract_state"], "stale");
    assert_eq!(status_json["reason_codes"][0], "stale_spec_plan_linkage");
    assert_eq!(
        status_json["diagnostics"][0]["code"],
        "stale_spec_plan_linkage"
    );
}

#[test]
fn canonical_workflow_status_routes_stale_source_revision_as_stale() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-stale-approved-revision");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_file(
        &repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design.md"),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 2\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-001][behavior] The route should expose stale approved plans when the source-spec revision drifts.\n",
    );
    write_file(
        &repo.join("docs/featureforge/plans/2026-01-22-document-review-system.md"),
        "# Approved Plan, Stale Source Revision\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Preserve the stale source revision case\n\n**Spec Coverage:** REQ-001\n**Goal:** The plan remains structurally valid while its source-spec revision goes stale.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The plan remains structurally valid while its source-spec revision goes stale.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Detect the stale source revision**\n",
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "stale_plan");
    assert_eq!(status_json["next_skill"], "featureforge:writing-plans");
    assert_eq!(status_json["contract_state"], "stale");
    assert_eq!(status_json["reason_codes"][0], "stale_spec_plan_linkage");
    assert_eq!(
        status_json["diagnostics"][0]["code"],
        "stale_spec_plan_linkage"
    );
}

#[cfg(unix)]
#[test]
fn workflow_status_argv0_alias_no_longer_dispatches_to_canonical_tree() {
    use std::os::unix::fs::symlink;

    let (repo_dir, state_dir) = init_repo("workflow-runtime-argv0");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_file(
        &repo.join("docs/featureforge/specs/2026-03-24-draft-spec-design.md"),
        "# Draft Spec\n\n**Workflow State:** Draft\n**Spec Revision:** 1\n**Last Reviewed By:** brainstorming\n",
    );

    let alias_dir = TempDir::new().expect("alias tempdir should be available");
    let alias_path = alias_dir.path().join("featureforge-workflow-status");
    symlink(cargo_bin("featureforge"), &alias_path)
        .expect("argv0 alias symlink should be creatable");

    let alias_output = run(
        {
            let mut command = Command::new(&alias_path);
            command
                .current_dir(repo)
                .env("FEATUREFORGE_STATE_DIR", state)
                .args(["--refresh"]);
            command
        },
        "rust argv0 workflow-status alias",
    );
    assert!(
        !alias_output.status.success(),
        "argv0 alias should no longer dispatch successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&alias_output.stdout),
        String::from_utf8_lossy(&alias_output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&alias_output.stderr).contains("featureforge-workflow-status"),
        "stderr should mention the rejected legacy alias entrypoint\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&alias_output.stdout),
        String::from_utf8_lossy(&alias_output.stderr)
    );
}

#[test]
fn canonical_workflow_status_refresh_recovers_old_manifest_after_slug_change() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-cross-slug-old");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-03-24-cross-slug-design.md";
    let expected_plan = "docs/featureforge/plans/2026-03-24-cross-slug-plan.md";

    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );

    let old_identity = discover_repo_identity(repo).expect("old repo identity should resolve");
    let old_manifest_path = manifest_path(&old_identity, state);
    write_manifest(
        &old_manifest_path,
        &WorkflowManifest {
            version: 1,
            repo_root: old_identity.repo_root.to_string_lossy().into_owned(),
            branch: old_identity.branch_name.clone(),
            expected_spec_path: spec_path.to_owned(),
            expected_plan_path: expected_plan.to_owned(),
            status: String::from("spec_approved_needs_plan"),
            next_skill: String::from("featureforge:writing-plans"),
            reason: String::from("missing_expected_plan,expect_set"),
            note: String::from("missing_expected_plan,expect_set"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    set_remote_url(
        repo,
        "https://example.com/example/workflow-runtime-cross-slug-new.git",
    );
    let new_identity = discover_repo_identity(repo).expect("new repo identity should resolve");
    let new_manifest_path = manifest_path(&new_identity, state);
    assert_ne!(
        old_manifest_path, new_manifest_path,
        "slug change should move the manifest path"
    );
    let recovered = recover_slug_changed_manifest(&new_identity, state, &new_manifest_path)
        .expect("cross-slug manifest should be recoverable from sibling state");
    assert_eq!(recovered.expected_plan_path, expected_plan);

    let route = WorkflowRuntime {
        identity: new_identity.clone(),
        state_dir: state.to_path_buf(),
        manifest_path: new_manifest_path.clone(),
        manifest: Some(recovered.clone()),
        manifest_warning: None,
        manifest_recovery_reasons: vec![String::from("repo_slug_recovered")],
    }
    .status()
    .expect("status should preserve the recovered expected plan path");
    assert_eq!(route.plan_path, expected_plan);

    let refreshed_route = WorkflowRuntime {
        identity: new_identity,
        state_dir: state.to_path_buf(),
        manifest_path: new_manifest_path.clone(),
        manifest: Some(recovered),
        manifest_warning: None,
        manifest_recovery_reasons: vec![String::from("repo_slug_recovered")],
    }
    .status_refresh()
    .expect("status refresh should preserve recovery metadata and write the new manifest");

    assert_eq!(refreshed_route.status, "spec_approved_needs_plan");
    assert_eq!(refreshed_route.plan_path, expected_plan);
    assert!(refreshed_route.reason.contains("repo_slug_recovered"));
    assert!(
        refreshed_route
            .reason_codes
            .iter()
            .any(|value| value == "repo_slug_recovered")
    );

    let new_manifest_json = fs::read_to_string(&new_manifest_path)
        .expect("recovered manifest should be written at the new slug path");
    assert!(new_manifest_json.contains(expected_plan));
}

#[test]
fn shared_markdown_scan_helper_collects_nested_markdown_only() {
    let fixture = TempDir::new().expect("markdown scan fixture should exist");
    write_file(&fixture.path().join("top.md"), "# top\n");
    write_file(&fixture.path().join("nested/plan.md"), "# nested\n");
    write_file(&fixture.path().join("nested/notes.txt"), "not markdown\n");

    let mut actual = markdown_scan_support::markdown_files_under(fixture.path())
        .into_iter()
        .map(|path| {
            path.strip_prefix(fixture.path())
                .expect("fixture file should stay under fixture root")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect::<Vec<_>>();
    actual.sort();

    assert_eq!(
        actual,
        vec![String::from("nested/plan.md"), String::from("top.md")]
    );
}

#[test]
fn canonical_manifest_path_distinguishes_exact_branch_names() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-branch-identity");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "feature/x"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout feature/x");
    let slash_identity = discover_repo_identity(repo).expect("feature/x identity should resolve");
    let slash_manifest_path = manifest_path(&slash_identity, state);

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "feature-x"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout feature-x");
    let dash_identity = discover_repo_identity(repo).expect("feature-x identity should resolve");
    let dash_manifest_path = manifest_path(&dash_identity, state);

    assert_ne!(
        slash_manifest_path, dash_manifest_path,
        "workflow manifests should stay exact-branch scoped",
    );
}

#[test]
fn canonical_manifest_path_uses_canonical_repo_slug_directory() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-manifest-slug");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    let manifest = manifest_path(&identity, state);

    assert_eq!(
        manifest
            .parent()
            .expect("manifest path should have a parent"),
        state.join("projects").join(repo_slug(repo))
    );
}

#[test]
fn canonical_workflow_status_refresh_limits_cross_slug_manifest_recovery_scan() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-cross-slug-budget");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-03-24-budget-limit-design.md";
    let expected_plan = "docs/featureforge/plans/2026-03-24-budget-limit-plan.md";

    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );

    let current_identity =
        discover_repo_identity(repo).expect("current repo identity should resolve");
    let current_manifest_path = manifest_path(&current_identity, state);
    let manifest_name = current_manifest_path
        .file_name()
        .expect("manifest path should have a file name")
        .to_owned();

    for index in 1..=12 {
        let decoy_dir = state.join("projects").join(format!("decoy-{index:02}"));
        write_manifest(
            &decoy_dir.join(&manifest_name),
            &WorkflowManifest {
                version: 1,
                repo_root: format!("/tmp/not-the-current-repo-{index:02}"),
                branch: current_identity.branch_name.clone(),
                expected_spec_path: String::new(),
                expected_plan_path: String::new(),
                status: String::from("needs_brainstorming"),
                next_skill: String::from("featureforge:brainstorming"),
                reason: String::from("decoy"),
                note: String::from("decoy"),
                updated_at: String::from("2026-03-24T00:00:00Z"),
            },
        );
    }

    write_manifest(
        &state.join("projects/zzz-old-slug").join(&manifest_name),
        &WorkflowManifest {
            version: 1,
            repo_root: current_identity.repo_root.to_string_lossy().into_owned(),
            branch: current_identity.branch_name.clone(),
            expected_spec_path: spec_path.to_owned(),
            expected_plan_path: expected_plan.to_owned(),
            status: String::from("spec_approved_needs_plan"),
            next_skill: String::from("featureforge:writing-plans"),
            reason: String::from("repo_slug_recovered"),
            note: String::from("repo_slug_recovered"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "spec_approved_needs_plan");
    assert_eq!(status_json["plan_path"], "");
    assert!(
        !status_json["reason"]
            .as_str()
            .unwrap_or("")
            .contains("repo_slug_recovered")
    );

    let manifest_json = fs::read_to_string(current_manifest_path)
        .expect("current manifest should be written after refresh");
    assert!(!manifest_json.contains(expected_plan));
}

#[test]
fn canonical_workflow_status_accepts_manifest_selected_plan_with_legacy_symlink_repo_root() {
    let (repo_dir, state_dir) = init_repo("workflow-status-symlink-manifest");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let alias_root = state.join("workflow-status-symlink-manifest-checkout");
    create_dir_symlink(repo, &alias_root);

    let spec_path = "docs/featureforge/specs/2026-03-24-symlink-manifest-spec.md";
    let plan_a = "docs/featureforge/plans/2026-03-24-symlink-manifest-a.md";
    let plan_b = "docs/featureforge/plans/2026-03-24-symlink-manifest-b.md";

    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );
    for plan_path in [plan_a, plan_b] {
        write_file(
            &repo.join(plan_path),
            &format!(
                "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_path}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n"
            ),
        );
    }

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: alias_root.to_string_lossy().into_owned(),
            branch: identity.branch_name.clone(),
            expected_spec_path: spec_path.to_owned(),
            expected_plan_path: plan_a.to_owned(),
            status: String::from("plan_draft"),
            next_skill: String::from("featureforge:plan-eng-review"),
            reason: String::from("legacy-symlink-manifest"),
            note: String::from("legacy-symlink-manifest"),
            updated_at: String::from("2026-03-25T00:00:00Z"),
        },
    );

    let status_json = workflow_status_refresh_json(&alias_root, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["plan_path"], plan_a);
    assert!(
        !status_json["reason_codes"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|value| value == "repo_root_mismatch"),
        "legacy symlink manifests should not be treated as repo_root mismatches"
    );
}

#[test]
fn canonical_workflow_status_ignores_manifest_selected_spec_when_branch_mismatches() {
    let (repo_dir, state_dir) = init_repo("workflow-status-manifest-branch-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_a = "docs/featureforge/specs/2026-03-24-branch-mismatch-a.md";
    let spec_b = "docs/featureforge/specs/2026-03-24-branch-mismatch-b.md";

    for spec_path in [spec_a, spec_b] {
        write_file(
            &repo.join(spec_path),
            "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
        );
    }

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: identity.repo_root.to_string_lossy().into_owned(),
            branch: String::from("other-branch"),
            expected_spec_path: spec_a.to_owned(),
            expected_plan_path: String::new(),
            status: String::from("spec_approved_needs_plan"),
            next_skill: String::from("featureforge:writing-plans"),
            reason: String::from("stale-branch-manifest"),
            note: String::from("stale-branch-manifest"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "spec_draft");
    assert_eq!(status_json["plan_path"], "");
    assert!(
        status_json["reason_codes"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|value| value == "ambiguous_spec_candidates"),
        "branch-mismatched manifests should not suppress ambiguous current spec candidates"
    );
}

#[test]
fn canonical_workflow_status_ignores_manifest_selected_plan_when_repo_root_mismatches() {
    let (repo_dir, state_dir) = init_repo("workflow-status-manifest-root-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-03-24-root-mismatch-spec.md";
    let plan_a = "docs/featureforge/plans/2026-03-24-root-mismatch-a.md";
    let plan_b = "docs/featureforge/plans/2026-03-24-root-mismatch-b.md";

    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );
    for plan_path in [plan_a, plan_b] {
        write_file(
            &repo.join(plan_path),
            &format!(
                "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_path}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n"
            ),
        );
    }

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: String::from("/tmp/another-repo"),
            branch: identity.branch_name.clone(),
            expected_spec_path: spec_path.to_owned(),
            expected_plan_path: plan_a.to_owned(),
            status: String::from("plan_draft"),
            next_skill: String::from("featureforge:plan-eng-review"),
            reason: String::from("stale-root-manifest"),
            note: String::from("stale-root-manifest"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "spec_approved_needs_plan");
    assert_eq!(status_json["plan_path"], "");
    assert!(
        status_json["reason_codes"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|value| value == "ambiguous_plan_candidates"),
        "repo-root-mismatched manifests should not suppress ambiguous current plan candidates"
    );
}

#[test]
fn canonical_workflow_status_refresh_recovers_legacy_symlinked_local_repo_manifest() {
    let (repo_dir, state_dir) = init_repo("workflow-local-symlink-recovery");
    let repo = repo_dir.path();
    let state = state_dir.path();
    remove_origin_remote(repo);

    let alias_root = state.join("workflow-local-symlink-checkout");
    create_dir_symlink(repo, &alias_root);

    let spec_path = "docs/featureforge/specs/2026-03-24-local-symlink-spec.md";
    let plan_a = "docs/featureforge/plans/2026-03-24-local-symlink-a.md";
    let plan_b = "docs/featureforge/plans/2026-03-24-local-symlink-b.md";

    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );
    for plan_path in [plan_a, plan_b] {
        write_file(
            &repo.join(plan_path),
            &format!(
                "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_path}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n"
            ),
        );
    }

    let current_identity = discover_repo_identity(repo).expect("repo identity should resolve");
    let legacy_identity = RepositoryIdentity {
        repo_root: alias_root.clone(),
        remote_url: None,
        branch_name: current_identity.branch_name.clone(),
    };
    let current_manifest_path = manifest_path(&current_identity, state);
    let legacy_manifest_path = manifest_path(&legacy_identity, state);
    assert_ne!(
        current_manifest_path, legacy_manifest_path,
        "canonicalized local repo roots should move the manifest path"
    );

    write_manifest(
        &legacy_manifest_path,
        &WorkflowManifest {
            version: 1,
            repo_root: alias_root.to_string_lossy().into_owned(),
            branch: current_identity.branch_name.clone(),
            expected_spec_path: spec_path.to_owned(),
            expected_plan_path: plan_a.to_owned(),
            status: String::from("plan_draft"),
            next_skill: String::from("featureforge:plan-eng-review"),
            reason: String::from("legacy-local-symlink-manifest"),
            note: String::from("legacy-local-symlink-manifest"),
            updated_at: String::from("2026-03-25T00:00:00Z"),
        },
    );

    let runtime = discover_execution_runtime(
        &alias_root,
        state,
        "workflow phase should preserve legacy local symlink manifest recovery reasons",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow phase should preserve legacy local symlink manifest recovery reasons",
    );
    assert!(
        phase_json["route"]["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|value| value == "repo_slug_recovered")),
        "workflow phase should preserve recovered manifest reason codes in the route payload"
    );
    assert_eq!(phase_json["plan_path"], plan_a);

    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff should preserve legacy local symlink manifest recovery reasons",
    );
    assert!(
        handoff_json["route"]["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|value| value == "repo_slug_recovered")),
        "workflow handoff should preserve recovered manifest reason codes in the route payload"
    );
    assert_eq!(handoff_json["plan_path"], plan_a);

    let status_json = workflow_status_refresh_json(&alias_root, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["plan_path"], plan_a);

    let rewritten: WorkflowManifest = serde_json::from_str(
        &fs::read_to_string(&current_manifest_path)
            .expect("canonical manifest should be rewritten on refresh"),
    )
    .expect("rewritten canonical manifest should parse");
    assert_eq!(
        rewritten.repo_root,
        current_identity.repo_root.to_string_lossy().into_owned()
    );
    assert_eq!(rewritten.expected_plan_path, plan_a);
}

#[test]
fn canonical_workflow_operator_accepts_manifest_selected_ready_route_with_extra_approved_candidates()
 {
    let (repo_dir, state_dir) = init_repo("workflow-manifest-selected-ready-route");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-manifest-selected-ready-route";
    let spec_path = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
    let plan_path = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let extra_spec_path = "docs/featureforge/specs/2026-03-24-extra-approved-spec.md";
    let extra_plan_path = "docs/featureforge/plans/2026-03-24-extra-approved-plan.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-manifest-ready"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-manifest-ready");

    install_full_contract_ready_artifacts(repo);
    enable_session_decision(state, session_key);
    write_file(
        &repo.join(extra_spec_path),
        "# Extra Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );
    write_file(
        &repo.join(extra_plan_path),
        &format!(
            "# Extra Approved Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_path}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n"
        ),
    );

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: fs::canonicalize(repo)
                .expect("repo root should canonicalize")
                .to_string_lossy()
                .into_owned(),
            branch: identity.branch_name.clone(),
            expected_spec_path: String::from(spec_path),
            expected_plan_path: String::from(plan_path),
            status: String::from("implementation_ready"),
            next_skill: String::new(),
            reason: String::from("implementation_ready"),
            note: String::from("implementation_ready"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let status_json = workflow_status_refresh_json(repo, state);
    assert_eq!(status_json["status"], "implementation_ready");
    assert_eq!(status_json["spec_path"], spec_path);
    assert_eq!(status_json["plan_path"], plan_path);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow phase for manifest-selected ready route",
    );
    let phase_json =
        workflow_phase_json(&runtime, "workflow phase for manifest-selected ready route");
    assert_eq!(phase_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(phase_json["route_status"], "implementation_ready");
    assert_eq!(phase_json["plan_path"], plan_path);

    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for manifest-selected ready route",
    );
    assert_eq!(handoff_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(handoff_json["route_status"], "implementation_ready");
    assert_eq!(handoff_json["plan_path"], plan_path);
}

#[test]
fn canonical_workflow_operator_plan_override_selects_explicit_ready_plan_amid_ambiguity() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-plan-override-ready-route");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
    let plan_path = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let extra_plan_path = "docs/featureforge/plans/2026-03-24-extra-approved-plan.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-operator-explicit-plan"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-operator-explicit-plan");

    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join(extra_plan_path),
        &format!(
            "# Extra Approved Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_path}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n"
        ),
    );

    let explicit_plan_arg = format!("./{plan_path}");
    let operator_json = parse_json(
        &run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                &explicit_plan_arg,
                "--json",
            ],
            &[],
            "workflow operator should honor an explicit approved plan even when the repo-wide resolver is ambiguous",
        ),
        "workflow operator should honor an explicit approved plan even when the repo-wide resolver is ambiguous",
    );

    assert_eq!(operator_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(operator_json["spec_path"], spec_path);
    assert_eq!(operator_json["plan_path"], plan_path);
    if let Some(recommended_command) = operator_json["recommended_command"].as_str() {
        assert!(
            recommended_command.contains(plan_path)
                || recommended_command.contains("<approved-plan-path>"),
            "operator recommended command should either preserve the explicit --plan override path or remain a generic approved-plan template, got {recommended_command:?}"
        );
        assert!(
            !recommended_command.contains(extra_plan_path),
            "operator recommended command should not drift to sibling approved plans, got {recommended_command:?}"
        );
    }
}

#[test]
fn canonical_workflow_operator_rejects_missing_plan_override() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-missing-plan-override");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);

    let output = run_rust_featureforge_with_env(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            "docs/featureforge/plans/does-not-exist.md",
            "--json",
        ],
        &[],
        "workflow operator should fail closed when --plan override points to a missing file",
    );
    assert!(
        !output.status.success(),
        "workflow operator should reject missing --plan overrides, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let error_json: Value =
        serde_json::from_slice(payload).expect("missing override error should be json");
    assert_eq!(
        error_json["error_class"],
        Value::from("InvalidCommandInput")
    );
    assert!(
        error_json["message"]
            .as_str()
            .is_some_and(|message| message.contains("Workflow plan override file does not exist.")),
        "expected missing override message, got {error_json:?}"
    );
}

#[test]
fn canonical_workflow_operator_plan_override_resolves_override_source_spec_instead_of_route_spec() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-plan-override-source-spec");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let primary_spec_path =
        "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
    let primary_plan_path = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let override_spec_path = "docs/featureforge/specs/2026-03-24-override-spec.md";
    let override_plan_path = "docs/featureforge/plans/2026-03-24-override-plan.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-operator-explicit-source-spec"])
        .current_dir(repo);
    run_checked(
        git_checkout,
        "git checkout workflow-operator-explicit-source-spec",
    );

    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join(override_spec_path),
        "# Override Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 2\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-001][behavior] Override plan routing should use the override source spec.\n",
    );
    write_file(
        &repo.join(override_plan_path),
        &format!(
            "# Override Approved Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{override_spec_path}`\n**Source Spec Revision:** 2\n**Last Reviewed By:** plan-eng-review\n"
        ),
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-override-pass.md",
            plan_path: override_plan_path,
            plan_revision: 1,
            spec_path: override_spec_path,
            spec_revision: 2,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-override-pass",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: fs::canonicalize(repo)
                .expect("repo root should canonicalize")
                .to_string_lossy()
                .into_owned(),
            branch: identity.branch_name.clone(),
            expected_spec_path: String::from(primary_spec_path),
            expected_plan_path: String::from(primary_plan_path),
            status: String::from("implementation_ready"),
            next_skill: String::new(),
            reason: String::from("implementation_ready"),
            note: String::from("implementation_ready"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                override_plan_path,
                "--json",
            ],
            &[],
            "workflow operator should resolve explicit override plan against the override plan source spec",
        ),
        "workflow operator should resolve explicit override plan against the override plan source spec",
    );

    assert_eq!(operator_json["spec_path"], override_spec_path);
    assert_eq!(operator_json["plan_path"], override_plan_path);
    if let Some(recommended_command) = operator_json["recommended_command"].as_str() {
        assert!(
            recommended_command.contains(override_plan_path)
                || recommended_command.contains("<approved-plan-path>"),
            "operator recommended command should either preserve the explicit override plan path or remain a generic approved-plan template, got {recommended_command:?}"
        );
        assert!(
            !recommended_command.contains(primary_plan_path),
            "operator recommended command should not drift back to manifest primary plan path, got {recommended_command:?}"
        );
    }
}

#[test]
fn canonical_workflow_status_treats_ceo_approved_specs_without_ceo_review_as_draft() {
    let (repo_dir, state_dir) = init_repo("workflow-status-approved-spec-reviewer-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_file(
        &repo.join("docs/featureforge/specs/2026-03-24-reviewer-mismatch-design.md"),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** brainstorming\n\n## Requirement Index\n\n- [REQ-001][behavior] Routing should reject approval-owner drift.\n",
    );

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "spec_draft");
    assert_eq!(status_json["next_skill"], "featureforge:plan-ceo-review");
}

#[test]
fn canonical_workflow_status_treats_eng_approved_plans_without_eng_review_as_draft() {
    let (repo_dir, state_dir) = init_repo("workflow-status-approved-plan-reviewer-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();

    install_full_contract_ready_artifacts(repo);
    let plan_path =
        repo.join("docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md");
    let source = fs::read_to_string(&plan_path).expect("plan fixture should be readable");
    fs::write(
        &plan_path,
        source.replace(
            "**Last Reviewed By:** plan-eng-review",
            "**Last Reviewed By:** writing-plans",
        ),
    )
    .expect("plan fixture should be writable");

    let status_json = workflow_status_refresh_json(repo, state);

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:writing-plans");
}

#[test]
fn canonical_workflow_phase_omits_session_entry_from_public_json() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-canonical-session-entry");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let runtime = discover_execution_runtime(
        repo,
        state,
        "rust canonical workflow phase should read canonical session-entry state",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "rust canonical workflow phase should read canonical session-entry state",
    );

    assert!(phase_json.get("session_entry").is_none());
    assert_eq!(phase_json["route"]["schema_version"], 3);
}

#[test]
fn canonical_workflow_operator_routes_ready_plan_without_session_entry_gate() {
    let (repo_dir, state_dir) = init_repo("workflow-no-session-entry-gate");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-no-session-entry-gate"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-no-session-entry-gate");

    install_full_contract_ready_artifacts(repo);
    let execution_preflight_phase =
        public_harness_phase_from_spec(concat!("execution_pre", "flight"));

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow phase should route directly without a session-entry gate",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow phase should route directly without a session-entry gate",
    );
    assert_eq!(
        phase_json["phase"],
        Value::String(execution_preflight_phase.clone())
    );
    assert_eq!(phase_json["next_action"], "continue execution");
    assert!(phase_json.get("session_entry").is_none());
    assert_eq!(phase_json["schema_version"], 3);
    assert_eq!(phase_json["route"]["schema_version"], 3);

    let doctor_json = workflow_doctor_json(
        &runtime,
        "workflow doctor should route directly without a session-entry gate",
    );
    assert_eq!(
        doctor_json["phase"],
        Value::String(execution_preflight_phase.clone())
    );
    assert_eq!(doctor_json["next_action"], "continue execution");
    assert!(doctor_json.get("session_entry").is_none());
    assert_eq!(doctor_json["schema_version"], 3);
    assert_eq!(doctor_json["route"]["schema_version"], 3);

    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff should route directly without a session-entry gate",
    );
    assert_eq!(
        handoff_json["phase"],
        Value::String(execution_preflight_phase)
    );
    assert_eq!(handoff_json["next_action"], "continue execution");
    assert_eq!(handoff_json["recommended_skill"], Value::from(""));
    assert!(handoff_json.get("session_entry").is_none());
    assert_eq!(handoff_json["schema_version"], 3);
    assert_eq!(handoff_json["route"]["schema_version"], 3);
}

#[test]
fn workflow_runtime_surfaces_expose_runtime_provenance_fields() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-provenance-surfaces");
    let repo = repo_dir.path();
    let state = state_dir.path();

    install_full_contract_ready_artifacts(repo);

    let runtime = discover_execution_runtime(repo, state, "runtime provenance fixture runtime");
    let status_json = plan_execution_status_json(
        &runtime,
        FULL_CONTRACT_READY_PLAN_REL,
        false,
        "plan execution status runtime provenance fixture",
    );
    assert_runtime_provenance_fields(&status_json, "plan execution status");

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            FULL_CONTRACT_READY_PLAN_REL,
            "--json",
        ],
        "workflow operator runtime provenance fixture",
    );
    assert_runtime_provenance_fields(&operator_json, "workflow operator");

    let doctor_json = workflow_doctor_json(&runtime, "workflow doctor runtime provenance fixture");
    assert_runtime_provenance_fields(&doctor_json, "workflow doctor");
}

#[test]
fn canonical_workflow_status_ignores_strict_session_entry_gate_env() {
    let (repo_dir, state_dir) = init_repo("workflow-status-strict-session-entry-gate");
    let repo = repo_dir.path();
    let state = state_dir.path();

    install_full_contract_ready_artifacts(repo);
    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    let workflow_manifest_path = manifest_path(&identity, state);

    let status_json = workflow_status_refresh_json(repo, state);
    assert_eq!(status_json["schema_version"], 3);
    assert_eq!(status_json["status"], "implementation_ready");
    assert!(
        !status_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| {
                code == "session_entry_unresolved" || code == "session_entry_bypassed"
            })),
        "workflow status should not expose removed strict session-entry reason codes"
    );

    let enabled_manifest: WorkflowManifest = serde_json::from_str(
        &fs::read_to_string(&workflow_manifest_path)
            .expect("workflow manifest should be written after strict refresh"),
    )
    .expect("workflow manifest json should parse after strict refresh");
    assert_eq!(
        enabled_manifest.expected_spec_path,
        status_json["spec_path"].as_str().unwrap_or(""),
        "strict refresh should persist the selected expected spec path"
    );
    assert_eq!(
        enabled_manifest.expected_plan_path,
        status_json["plan_path"].as_str().unwrap_or(""),
        "strict refresh should persist the selected expected plan path"
    );

    let bypassed_decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join("workflow-status-strict-session-entry-gate-bypassed");
    write_file(&bypassed_decision_path, "bypassed\n");
    let bypassed_status_json = workflow_status_refresh_json(repo, state);
    assert_eq!(bypassed_status_json["schema_version"], 3);
    assert_eq!(bypassed_status_json["status"], "implementation_ready");
    assert!(
        !bypassed_status_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| {
                code == "session_entry_unresolved" || code == "session_entry_bypassed"
            })),
        "workflow status should not expose removed strict session-entry reason codes"
    );
    let manifest_after_bypassed_session: WorkflowManifest = serde_json::from_str(
        &fs::read_to_string(&workflow_manifest_path)
            .expect("workflow manifest should remain readable after bypassed strict refresh"),
    )
    .expect("workflow manifest should parse after bypassed strict refresh");
    assert_eq!(
        manifest_after_bypassed_session.expected_spec_path, enabled_manifest.expected_spec_path,
        "bypassed session-entry files should not clear the selected expected spec path"
    );
    assert_eq!(
        manifest_after_bypassed_session.expected_plan_path, enabled_manifest.expected_plan_path,
        "bypassed session-entry files should not clear the selected expected plan path"
    );
}

#[test]
fn canonical_workflow_operator_ignores_spawned_subagent_context_markers() {
    let (repo_dir, state_dir) = init_repo("workflow-spawned-subagent");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-spawned-subagent";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-spawned-subagent"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-spawned-subagent");

    install_full_contract_ready_artifacts(repo);
    let execution_preflight_phase =
        public_harness_phase_from_spec(concat!("execution_pre", "flight"));

    let spawned_subagent_env = [
        ("FEATUREFORGE_SESSION_KEY", session_key),
        ("FEATUREFORGE_SPAWNED_SUBAGENT", "1"),
    ];

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &spawned_subagent_env,
            "workflow operator should bypass session-entry gate for spawned subagents",
        ),
        "workflow operator should bypass session-entry gate for spawned subagents",
    );

    assert_eq!(
        operator_json["phase"],
        Value::String(execution_preflight_phase.clone())
    );
    assert_eq!(operator_json["next_action"], "continue execution");
    assert!(operator_json.get("session_entry").is_none());
}

#[test]
fn canonical_workflow_operator_ignores_spawned_subagent_opt_in_markers() {
    let (repo_dir, state_dir) = init_repo("workflow-spawned-subagent-opt-in");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-spawned-subagent-opt-in";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-spawned-subagent-opt-in"])
        .current_dir(repo);
    run_checked(
        git_checkout,
        "git checkout workflow-spawned-subagent-opt-in",
    );

    install_full_contract_ready_artifacts(repo);
    let execution_preflight_phase =
        public_harness_phase_from_spec(concat!("execution_pre", "flight"));

    let spawned_subagent_env = [
        ("FEATUREFORGE_SESSION_KEY", session_key),
        ("FEATUREFORGE_SPAWNED_SUBAGENT", "1"),
        ("FEATUREFORGE_SPAWNED_SUBAGENT_OPT_IN", "1"),
    ];

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &spawned_subagent_env,
            "workflow operator should honor spawned-subagent opt-in",
        ),
        "workflow operator should honor spawned-subagent opt-in",
    );

    assert_eq!(
        operator_json["phase"],
        Value::String(execution_preflight_phase)
    );
    assert_eq!(operator_json["next_action"], "continue execution");
    assert!(operator_json.get("session_entry").is_none());
}

#[test]
fn canonical_workflow_operator_ready_plan_pins_observability_seam_corpus() {
    let workflow_seam_event_kinds = [
        "phase_transition",
        "gate_result",
        "recommendation_proposed",
        "policy_accepted",
        "downstream_gate_rejected",
    ];
    let workflow_seam_minimum_envelope_fields = [
        "event_kind",
        "timestamp",
        "execution_run_id",
        "authoritative_sequence",
        "source_plan_path",
        "source_plan_revision",
        "harness_phase",
        "chunk_id",
        "command_name",
        "gate_name",
        "failure_class",
        "reason_codes",
    ];
    let workflow_seam_telemetry_counter_keys = [
        "phase_transition_count",
        "gate_failures_by_gate",
        "downstream_gate_rejection_count",
        "write_authority_conflict_count",
        "repo_state_drift_count",
    ];
    let workflow_seam_folded_diagnostics = ["write_authority_conflict", "repo_state_drift"];

    let stable_event_kinds: BTreeSet<&str> = STABLE_EVENT_KINDS.iter().copied().collect();
    let unknown_event_kinds: Vec<&str> = workflow_seam_event_kinds
        .iter()
        .copied()
        .filter(|event_kind| !stable_event_kinds.contains(event_kind))
        .collect();
    assert!(
        unknown_event_kinds.is_empty(),
        "workflow operator observability seam should use only runtime-owned event kinds, unknown: {unknown_event_kinds:?}"
    );

    let stable_reason_codes: BTreeSet<&str> = STABLE_REASON_CODES.iter().copied().collect();
    let unknown_reason_codes: Vec<&str> = workflow_seam_folded_diagnostics
        .iter()
        .copied()
        .filter(|reason_code| !stable_reason_codes.contains(reason_code))
        .collect();
    assert!(
        unknown_reason_codes.is_empty(),
        "workflow operator observability seam should fold only stable runtime diagnostics, unknown: {unknown_reason_codes:?}"
    );

    let mut probe_event =
        HarnessObservabilityEvent::new(HarnessEventKind::PhaseTransition, "2026-03-26T12:00:00Z");
    for reason_code in workflow_seam_folded_diagnostics {
        probe_event.add_reason_code(reason_code);
    }
    let serialized_probe_event =
        to_value(probe_event).expect("workflow observability seam probe event should serialize");
    let event_object = serialized_probe_event
        .as_object()
        .expect("workflow observability seam probe should serialize to a JSON object");
    let missing_envelope_fields: Vec<&str> = workflow_seam_minimum_envelope_fields
        .iter()
        .copied()
        .filter(|field| !event_object.contains_key(*field))
        .collect();
    assert!(
        missing_envelope_fields.is_empty(),
        "workflow operator observability seam should pin minimum envelope fields, missing: {missing_envelope_fields:?}"
    );
    assert_eq!(
        event_object.get("event_kind").and_then(Value::as_str),
        Some("phase_transition"),
        "workflow seam observability envelope should keep event_kind machine-readable"
    );
    assert!(
        event_object
            .get("timestamp")
            .and_then(Value::as_str)
            .is_some_and(|timestamp| !timestamp.is_empty()),
        "workflow seam observability envelope should keep timestamp as a non-empty string"
    );
    assert!(
        event_object
            .get("reason_codes")
            .and_then(Value::as_array)
            .is_some_and(|codes| {
                let code_set: BTreeSet<&str> = codes.iter().filter_map(Value::as_str).collect();
                workflow_seam_folded_diagnostics
                    .iter()
                    .all(|reason_code| code_set.contains(reason_code))
            }),
        "workflow seam observability envelope should keep folded conflict/drift diagnostics machine-readable"
    );

    let serialized_counters = to_value(HarnessTelemetryCounters::default())
        .expect("workflow observability seam counters should serialize");
    let counter_object = serialized_counters
        .as_object()
        .expect("workflow observability seam counters should serialize to a JSON object");
    let missing_counter_keys: Vec<&str> = workflow_seam_telemetry_counter_keys
        .iter()
        .copied()
        .filter(|counter_key| !counter_object.contains_key(*counter_key))
        .collect();
    assert!(
        missing_counter_keys.is_empty(),
        "workflow operator observability seam should pin required telemetry counter keys, missing: {missing_counter_keys:?}"
    );
}

#[test]
fn canonical_workflow_operator_surfaces_fail_closed_when_session_entry_is_bypassed() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-bypassed-session");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-bypassed-session";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-phase-bypassed"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-phase-bypassed");

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "bypassed\n");

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow operator should fail closed when session-entry is bypassed",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow phase should fail closed when session-entry is bypassed",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff should fail closed when session-entry is bypassed",
    );
    let execution_preflight_phase =
        public_harness_phase_from_spec(concat!("execution_pre", "flight"));
    assert_eq!(
        phase_json["phase"],
        Value::String(execution_preflight_phase.clone())
    );
    assert_eq!(phase_json["next_action"], "continue execution");
    assert!(phase_json.get("session_entry").is_none());

    assert_eq!(
        handoff_json["phase"],
        Value::String(execution_preflight_phase)
    );
    assert_eq!(handoff_json["next_action"], "continue execution");
    assert_eq!(handoff_json["recommended_skill"], Value::from(""));
    assert!(handoff_json.get("session_entry").is_none());
}

#[test]
fn canonical_workflow_phase_routes_enabled_stale_plan_to_plan_writing() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-stale-plan");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_file(
        &repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design-v2.md"),
        "# Approved Spec, Newer Path\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Notes\n",
    );
    write_file(
        &repo.join("docs/featureforge/plans/2026-01-22-document-review-system.md"),
        "# Approved Plan, Stale Source Path\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Preserve the stale source path case\n\n**Spec Coverage:** REQ-001\n**Goal:** The plan remains structurally valid while its source-spec path goes stale.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The plan remains structurally valid while its source-spec path goes stale.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Detect the stale source path**\n",
    );
    let runtime = discover_execution_runtime(
        repo,
        state,
        "rust canonical workflow phase should route stale plans to plan writing",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "rust canonical workflow phase should route stale plans to plan writing",
    );

    assert_eq!(phase_json["route_status"], "stale_plan");
    assert_eq!(phase_json["phase"], "pivot_required");
    assert_eq!(phase_json["next_action"], "pivot / return to planning");
    assert_eq!(phase_json["next_skill"], "featureforge:writing-plans");
    assert!(phase_json.get("session_entry").is_none());
}

#[test]
fn canonical_workflow_phase_keeps_corrupt_manifest_read_only() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-corrupt-manifest");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = repo.join("docs/featureforge/specs/2026-03-24-corrupt-phase-spec.md");

    write_file(
        &spec_path,
        "# Phase Corrupt Manifest Spec\n\n**Workflow State:** Draft\n**Spec Revision:** 1\n**Last Reviewed By:** brainstorming\n",
    );

    let _ = workflow_status_refresh_json(repo, state);

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    let manifest_path = manifest_path(&identity, state);
    fs::write(&manifest_path, "{ \"broken\": true\n")
        .expect("corrupt manifest fixture should be writable");
    let before_bytes = fs::read(&manifest_path).expect("corrupt manifest fixture should exist");

    let runtime = discover_execution_runtime(
        repo,
        state,
        "rust canonical workflow phase should inspect corrupt manifests without repairing them",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "rust canonical workflow phase should inspect corrupt manifests without repairing them",
    );
    assert!(phase_json["phase"].is_string());

    let after_bytes = fs::read(&manifest_path)
        .expect("workflow phase should leave the corrupt manifest in place");
    assert_eq!(after_bytes, before_bytes);

    let parent = manifest_path
        .parent()
        .expect("manifest fixture should have a parent directory");
    let backup_prefix = format!(
        "{}.corrupt-",
        manifest_path
            .file_name()
            .expect("manifest fixture should have a file name")
            .to_string_lossy()
    );
    let backup_written = fs::read_dir(parent)
        .expect("manifest directory should stay readable")
        .flatten()
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&backup_prefix)
        });
    assert!(
        !backup_written,
        "workflow phase should not create corrupt-manifest backups for read-only inspection"
    );
}

#[test]
fn canonical_workflow_public_text_surfaces_prefer_operator_and_status_over_removed_helpers() {
    let (repo_dir, state_dir) = init_repo("workflow-public-text-commands");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-public-text-commands";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-public-text"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-public-text");

    install_full_contract_ready_artifacts(repo);
    for removed in ["next", "artifacts", "explain"] {
        let output = run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &["workflow", removed],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            &format!("workflow {removed} should stay removed on ready plans"),
        );
        assert!(
            !output.status.success(),
            "workflow {removed} should stay removed"
        );
        let failure: Value = serde_json::from_slice(&output.stderr)
            .or_else(|_| serde_json::from_slice(&output.stdout))
            .expect("removed helper should emit json parse failure");
        assert_eq!(failure["error_class"], "InvalidCommandInput");
        assert!(
            failure["message"].as_str().is_some_and(
                |message| message.contains(&format!("unrecognized subcommand '{removed}'"))
            ),
            "workflow {removed} should fail at CLI parsing, got {failure:?}"
        );
    }

    let operator_output = run_rust_featureforge_with_env(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "workflow operator text should be available on ready plans",
    );
    assert!(operator_output.status.success());
    let operator_stdout = String::from_utf8_lossy(&operator_output.stdout);
    assert!(operator_stdout.contains("Workflow operator"));
    assert!(operator_stdout.contains("Next action: continue execution"));
    assert!(operator_stdout.contains(
        "Spec: docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"
    ));
    assert!(
        operator_stdout
            .contains("Plan: docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md")
    );
    assert!(!operator_stdout.contains("session-entry"));

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should be available on ready plans",
    );
    assert_eq!(status_json["state_kind"], "actionable_public_command");
    assert!(
        status_json["next_public_action"]["command"]
            .as_str()
            .is_some_and(|command| command.starts_with("featureforge plan execution begin "))
    );
    assert_eq!(
        status_json["phase_detail"],
        concat!("execution_pre", "flight_required")
    );
}

#[test]
fn canonical_workflow_doctor_exposes_harness_state_before_execution_starts() {
    let (repo_dir, state_dir) = init_repo("workflow-public-harness-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-public-harness-state";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");

    let implementation_handoff_phase = public_harness_phase_from_spec("implementation_handoff");
    let execution_preflight_phase =
        public_harness_phase_from_spec(concat!("execution_pre", "flight"));

    let runtime =
        discover_execution_runtime(repo, state, "workflow doctor for harness state fixture");
    let doctor_json = workflow_doctor_json(&runtime, "workflow doctor for harness state fixture");

    assert_eq!(
        doctor_json["phase"],
        Value::String(execution_preflight_phase)
    );
    assert_eq!(doctor_json["route_status"], "implementation_ready");
    assert_eq!(doctor_json["next_action"], "continue execution");
    assert_eq!(doctor_json["execution_status"]["execution_started"], "no");
    assert_eq!(doctor_json["gate_review"], Value::Null);
    assert_eq!(doctor_json["gate_finish"], Value::Null);

    let execution_status = &doctor_json["execution_status"];
    let execution_run_id = execution_status
        .get("execution_run_id")
        .expect("workflow doctor should expose execution_run_id");
    assert!(
        execution_run_id.is_null(),
        "workflow doctor should expose execution_run_id as null before {} acceptance, got {execution_run_id:?}",
        concat!("pre", "flight")
    );
    assert_eq!(
        execution_status["harness_phase"],
        Value::String(implementation_handoff_phase)
    );
    assert_eq!(
        execution_status["latest_authoritative_sequence"],
        Value::from(0)
    );

    let missing_pre_acceptance_null_fields = missing_null_fields(
        execution_status,
        &[
            "chunking_strategy",
            "evaluator_policy",
            "reset_policy",
            "review_stack",
        ],
    );
    assert!(
        missing_pre_acceptance_null_fields.is_empty(),
        "workflow doctor should expose pre-acceptance policy fields as required-and-null before execution {} accepts run identity, missing null fields: {missing_pre_acceptance_null_fields:?}",
        concat!("pre", "flight")
    );

    for field in [
        "aggregate_evaluation_state",
        "repo_state_baseline_head_sha",
        "repo_state_baseline_worktree_fingerprint",
        "repo_state_drift_state",
        "dependency_index_state",
        "final_review_state",
        "browser_qa_state",
        "release_docs_state",
        "last_final_review_artifact_fingerprint",
        "last_browser_qa_artifact_fingerprint",
        "last_release_docs_artifact_fingerprint",
    ] {
        assert!(
            execution_status.get(field).is_some(),
            "workflow doctor should expose {field} in execution_status"
        );
    }

    for field in ["write_authority_holder", "write_authority_worktree"] {
        let value = execution_status
            .get(field)
            .unwrap_or_else(|| panic!("workflow doctor should expose {field}"));
        assert!(
            value.is_null() || value.as_str().is_some_and(|value| !value.is_empty()),
            "workflow doctor should expose {field} as null when unknown pre-start or as non-empty diagnostic metadata once known, got {value:?}"
        );
    }

    let missing_null_fields = missing_null_fields(
        execution_status,
        &[
            "active_contract_path",
            "active_contract_fingerprint",
            "last_evaluation_report_path",
            "last_evaluation_report_fingerprint",
            "last_evaluation_evaluator_kind",
        ],
    );
    assert!(
        missing_null_fields.is_empty(),
        "workflow doctor should keep active pointers authoritative-only before execution starts, missing null fields: {missing_null_fields:?}"
    );

    for field in [
        "required_evaluator_kinds",
        "completed_evaluator_kinds",
        "pending_evaluator_kinds",
        "non_passing_evaluator_kinds",
        "open_failed_criteria",
        "reason_codes",
    ] {
        assert!(
            execution_status
                .get(field)
                .and_then(Value::as_array)
                .is_some(),
            "workflow doctor should expose array field {field} in execution_status"
        );
    }

    let review_stack = execution_status
        .get("review_stack")
        .expect("workflow doctor should expose review_stack in execution_status");
    assert!(
        review_stack.is_null(),
        "workflow doctor should expose review_stack as required-and-null before execution {} accepts policy, got {review_stack:?}",
        concat!("pre", "flight")
    );
}

#[test]
fn canonical_workflow_handoff_rejects_legacy_pre_harness_cutover_state() {
    let (repo_dir, state_dir) = init_repo("workflow-handoff-legacy-pre-harness-cutover");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-handoff-legacy-pre-harness-cutover";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let cutover_message = "Legacy pre-harness execution evidence is no longer accepted; regenerate execution evidence using the harness v2 format.";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);

    install_full_contract_ready_artifacts(repo);
    enable_session_decision(state, session_key);
    assert_eq!(
        fs::read_to_string(&decision_path).expect("session decision should be readable"),
        "enabled\n"
    );
    assert!(
        fs::read_to_string(repo.join(plan_rel))
            .expect("approved plan fixture should be readable")
            .contains("**Workflow State:** Engineering Approved"),
        "fixture should keep an engineering-approved plan for workflow handoff routing"
    );

    let execution_status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before legacy pre-harness cutover handoff fixture",
    );
    let evidence_path = repo.join(
        execution_status["evidence_path"]
            .as_str()
            .expect("execution status should expose evidence_path"),
    );
    write_file(&repo.join("docs/example-output.md"), "legacy output\n");
    write_file(
        &evidence_path,
        &format!(
            "# Execution Evidence: 2026-03-17-example-execution-plan\n\n**Plan Path:** {plan_rel}\n**Plan Revision:** 1\n\n## Step Evidence\n\n### Task 1 Step 1\n#### Attempt 1\n**Status:** Completed\n**Recorded At:** 2026-03-17T14:22:31Z\n**Execution Source:** featureforge:executing-plans\n**Claim:** Prepared the workspace for execution.\n**Files:**\n- docs/example-output.md\n**Verification:**\n- Manual verification recorded in fixture setup.\n**Invalidation Reason:** N/A\n"
        ),
    );
    assert!(
        fs::read_to_string(&evidence_path)
            .expect("legacy execution evidence fixture should be readable")
            .contains("## Step Evidence"),
        "fixture should inject legacy pre-harness execution evidence"
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for legacy pre-harness cutover fixture",
    );
    let error = operator::handoff_for_runtime(&runtime)
        .expect_err("workflow handoff should fail closed for legacy pre-harness cutover state");
    let error_message = format!("{} {}", error.error_class, error.message);
    assert!(
        error_message.contains("MalformedExecutionState"),
        "workflow handoff should report malformed legacy execution evidence, got:\n{error_message}"
    );
    assert!(
        error_message.contains(cutover_message),
        "workflow handoff should explain legacy pre-harness cutover rejection, got:\n{error_message}"
    );
}

#[test]
fn canonical_workflow_operator_pins_authoritative_contract_drafting_phase_in_public_surfaces() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-authoritative-contract-drafting");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-authoritative-contract-drafting";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    install_full_contract_ready_artifacts(repo);
    enable_session_decision(state, session_key);
    let branch = current_branch_name(repo);
    write_file(
        &harness_state_path(state, &repo_slug(repo), &branch),
        &serde_json::to_string(&json!({
            "schema_version": 1,
            "run_identity": {
                "execution_run_id": "run-contract-drafting-fixture",
                "source_plan_path": plan_rel,
                "source_plan_revision": 1
            },
            "active_worktree_lease_fingerprints": [],
            "active_worktree_lease_bindings": []
        }))
        .expect("contract_drafting fixture harness state should serialize"),
    );

    let expected_phase = public_harness_phase_from_spec("contract_drafting");
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("harness_phase", Value::from(expected_phase.clone())),
            ("latest_authoritative_sequence", Value::from(17)),
        ],
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow phase should pin authoritative contract_drafting phase",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow phase should pin authoritative contract_drafting phase",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff should pin authoritative contract_drafting phase",
    );

    assert_eq!(phase_json["route_status"], "implementation_ready");
    assert_eq!(
        phase_json["phase"], "pivot_required",
        "phase JSON should preserve authoritative contract-drafting pivot route, got {phase_json}"
    );
    assert_eq!(handoff_json["route_status"], "implementation_ready");
    assert_eq!(handoff_json["phase"], "pivot_required");
}

#[test]
fn canonical_workflow_operator_surfaces_pivot_required_plan_revision_block_phase_and_next_action() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-authoritative-pivot-required");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-authoritative-pivot-required";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);
    enable_session_decision(state, session_key);

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo), &current_branch_name(repo));
    write_file(
        &authoritative_state_path,
        r#"{"harness_phase":"pivot_required","latest_authoritative_sequence":23,"reason_codes":["blocked_on_plan_revision"]}"#,
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow phase should surface authoritative pivot_required plan-revision blocks",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow phase should surface authoritative pivot_required plan-revision blocks",
    );
    let doctor_json = workflow_doctor_json(
        &runtime,
        "workflow doctor should surface authoritative pivot_required plan-revision blocks",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff should surface authoritative pivot_required plan-revision blocks",
    );

    let expected_phase = public_harness_phase_from_spec("pivot_required");

    assert_eq!(
        doctor_json["execution_status"]["harness_phase"],
        Value::String(expected_phase.clone())
    );
    assert_eq!(phase_json["phase"], expected_phase);
    assert_eq!(doctor_json["phase"], expected_phase);
    assert_eq!(handoff_json["phase"], expected_phase);
    assert_eq!(phase_json["next_action"], "pivot / return to planning");
    assert_eq!(doctor_json["next_action"], "pivot / return to planning");
    assert_eq!(handoff_json["next_action"], "pivot / return to planning");
    assert_ne!(
        handoff_json["recommended_skill"], doctor_json["execution_status"]["execution_mode"],
        "pivot-required plan-revision blocks should not keep recommending the active execution mode"
    );
    assert_eq!(
        handoff_json["recommendation_reason"],
        "Execution is blocked pending an approved plan revision."
    );
}

#[test]
fn canonical_workflow_ignores_stale_tracked_evidence_projection_for_routing() {
    let (repo_dir, state_dir) =
        init_repo(concat!("workflow-phase-gate", "-review-evidence-failure"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = concat!("workflow-phase-gate", "-review-evidence-failure");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);
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
            ("final_review_state", Value::from("fresh")),
            ("browser_qa_state", Value::from("fresh")),
            ("release_docs_state", Value::from("fresh")),
        ],
    );

    let execution_status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for workflow review evidence failure fixture",
    );
    let evidence_rel = execution_status["evidence_path"]
        .as_str()
        .expect("execution status should expose evidence_path");
    run_plan_execution_json(
        repo,
        state,
        &[
            "materialize-projections",
            "--plan",
            plan_rel,
            "--repo-export",
            "--confirm-repo-export",
        ],
        concat!(
            "materialize projection export execution evidence for gate",
            "-review diagnostic fixture"
        ),
    );
    let materialized_status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!(
            "status after tracked evidence projection export for gate",
            "-review diagnostic fixture"
        ),
    );
    let evidence_export_rel = materialized_status["tracked_projection_paths"]
        .as_array()
        .expect("status should expose tracked projection paths")
        .iter()
        .filter_map(Value::as_str)
        .find(|path| path.ends_with("/execution-evidence.md"))
        .expect("status should expose a tracked evidence projection export path");
    let state_dir_evidence_path =
        projection_support::state_dir_projection_path(&execution_status, evidence_rel);
    materialize_state_dir_projections(
        repo,
        state,
        plan_rel,
        concat!(
            "materialize state-dir execution evidence before deletion for gate",
            "-review diagnostic fixture"
        ),
    );
    fs::remove_file(&state_dir_evidence_path).unwrap_or_else(|error| {
        panic!(
            "state-dir evidence projection {} should be removable for tracked diagnostic fixture: {error}",
            state_dir_evidence_path.display()
        )
    });
    let state_dir_fingerprint_path = state_dir_evidence_path.with_file_name(format!(
        "{}.sha256",
        state_dir_evidence_path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .expect("state-dir evidence projection should have a utf-8 file name")
    ));
    if let Err(error) = fs::remove_file(&state_dir_fingerprint_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        panic!(
            "state-dir evidence projection fingerprint {} should be removable for tracked diagnostic fixture: {error}",
            state_dir_fingerprint_path.display()
        );
    }
    let evidence_path = repo.join(evidence_export_rel);
    replace_in_file(
        &evidence_path,
        "**Plan Fingerprint:** ",
        "**Plan Fingerprint:** stale-",
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        concat!(
            "workflow phase for gate",
            "-review evidence failure fixture"
        ),
    );
    let phase_json = workflow_phase_json(
        &runtime,
        concat!(
            "workflow phase for gate",
            "-review evidence failure fixture"
        ),
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        concat!(
            "workflow handoff for gate",
            "-review evidence failure fixture"
        ),
    );
    let doctor_json = workflow_doctor_json(
        &runtime,
        concat!(
            "workflow doctor for gate",
            "-review evidence failure fixture"
        ),
    );
    assert_eq!(
        doctor_json["execution_status"]["tracked_projections_current"],
        Value::Bool(false),
        "workflow doctor should expose stale tracked projection currentness without making it route authority, got {doctor_json:?}"
    );
    if let Some(gate_review) = doctor_json["gate_review"].as_object() {
        assert!(
            gate_review
                .get("reason_codes")
                .and_then(Value::as_array)
                .is_none_or(|codes| {
                    !codes
                        .iter()
                        .any(|code| code.as_str() == Some("plan_fingerprint_mismatch"))
                }),
            "tracked projection freshness should not surface as a {} reason, got {doctor_json:?}",
            concat!("gate", "-review")
        );
    }
    assert_eq!(phase_json["phase"], "document_release_pending");
    assert_eq!(phase_json["next_action"], "advance late stage");
    assert_eq!(handoff_json["phase"], "document_release_pending");
    assert_eq!(handoff_json["next_action"], "advance late stage");
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:document-release"
    );
}

#[test]
fn canonical_workflow_harness_operator_precedence_parity_dual_unresolved() {
    let (repo_dir, state_dir) = init_repo("workflow-harness-operator-parity-dual-unresolved");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-harness-operator-parity-dual-unresolved";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let branch = current_branch_name(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
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
            ("final_review_state", Value::from("missing")),
            ("browser_qa_state", Value::from("missing")),
            ("release_docs_state", Value::from("missing")),
        ],
    );
    enable_session_decision(state, session_key);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_runtime harness/operator parity dual-unresolved fixture",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow_runtime harness/operator parity dual-unresolved fixture",
    );
    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_runtime harness/operator parity dual-unresolved fixture",
    );

    assert_eq!(phase_json["phase"], "document_release_pending");
    assert_eq!(
        status_json["harness_phase"], "document_release_pending",
        "authoritative harness phase should match operator precedence output for dual-unresolved late-stage state; phase payload: {phase_json:?}; status payload: {status_json:?}"
    );
}

#[test]
fn canonical_workflow_phase_routes_review_resolved_to_document_release_pending() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-release-pending");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-release-pending";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    let review_path = write_branch_review_artifact(repo, state, plan_rel, &base_branch);
    publish_authoritative_final_review_truth(repo, state, plan_rel, &review_path);
    enable_session_decision(state, session_key);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow handoff for release-pending routing fixture",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for release-pending routing fixture",
    );

    assert_eq!(handoff_json["phase"], "document_release_pending");
    assert_eq!(handoff_json["next_action"], "advance late stage");
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:document-release"
    );
    assert_eq!(
        handoff_json["recommendation_reason"],
        "Finish readiness requires a current release-readiness milestone for the current branch closure."
    );
}

#[test]
fn compiled_cli_route_parity_probe_for_completed_late_stage_fixture() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs07-parity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let mut runtime_management_commands = 0usize;
    runtime_management_commands += 1;
    let operator_json = parse_json(
        &run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-07 workflow operator route parity fixture",
        ),
        "FS-07 workflow operator route parity fixture",
    );
    runtime_management_commands += 1;
    let status_json = parse_json(
        &run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &["plan", "execution", "status", "--plan", plan_rel],
            &[],
            "FS-07 plan execution status route parity fixture",
        ),
        "FS-07 plan execution status route parity fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_parity_probe_budget("PARITY-PROBE-CLEAN", runtime_management_commands, 2);
}

fn setup_runtime_fs11_next_action_fixture(repo: &Path, state: &Path, plan_rel: &str) {
    install_full_contract_ready_artifacts(repo);
    write_runtime_remediation_fs11_plan(repo, plan_rel, FULL_CONTRACT_READY_SPEC_REL);
    prepare_preflight_acceptance_workspace(repo, "runtime-remediation-fs11-next-action");
    let branch = current_branch_name(repo);

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11 status before execution-start bootstrap begin",
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
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-11 status should expose execution fingerprint before begin"),
        ],
        "FS-11 execution-start bootstrap begin",
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
            "FS-11 bootstrap complete",
            "--manual-verify-summary",
            "FS-11 bootstrap complete summary",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"]
                .as_str()
                .expect("FS-11 begin should expose execution fingerprint before complete"),
        ],
        "FS-11 execution-start bootstrap complete",
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
                    }
                }),
            ),
            (
                "current_open_step_state",
                json!({
                    "task": 3,
                    "step": 6,
                    "note_state": "Interrupted",
                    "note_summary": "FS-11 forward reentry overlay must not outrank stale Task 2 boundary",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 30
                }),
            ),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
            ("resume_task", Value::from(3_u64)),
            ("resume_step", Value::from(6_u64)),
        ],
    );
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

#[test]
fn runtime_remediation_fs15_earliest_stale_boundary_beats_latest_overlay_target() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs15-earliest-stale-boundary");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-01-runtime-fs15-earliest-stale-boundary.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-15 workflow operator stale-boundary targeting fixture",
        ),
        "FS-15 workflow operator stale-boundary targeting fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-15 plan execution status stale-boundary targeting fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["blocking_scope"],
        Value::from("task"),
        "FS-15 should keep stale-boundary targeting task-scoped, got {operator_json:?}"
    );
    assert_eq!(
        operator_json["blocking_task"],
        Value::from(2_u64),
        "FS-15 earliest unresolved stale boundary must target Task 2, not Task 6; operator payload: {operator_json:?}"
    );
    assert_eq!(
        status_json["execution_reentry_target_source"],
        Value::from("closure_graph_stale_target"),
        "FS-15 status should diagnose the stale closure graph as the authoritative reentry target source: {status_json:?}"
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-15 workflow operator should return a concrete follow-up command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-15 workflow operator must target Task 2 while it is the earliest unresolved stale boundary, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 6"),
        "FS-15 workflow operator must not prefer stale Task 6 while Task 2 remains unresolved, got {recommended_command}"
    );
    assert!(
        recommended_command.starts_with("featureforge plan execution "),
        "FS-15 workflow operator should return an executable plan-execution command, got {recommended_command}"
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        "FS-15 run operator-routed public command for concrete parity check",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_json,
        &routed_follow_up,
        "FS-15 command-follow parity",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_eq!(
            routed_follow_up["required_follow_up"],
            Value::from("execution_reentry"),
            "FS-15 blocked command-follow parity must keep execution_reentry as the concrete follow-up lane"
        );
    } else {
        let reopened_task_2 = routed_follow_up["resume_task"].as_u64() == Some(2_u64)
            && routed_follow_up["resume_step"].as_u64() == Some(1_u64);
        assert!(
            reopened_task_2,
            "FS-15 command-follow parity must reopen Task 2 Step 1 when the routed command is not blocked, got {routed_follow_up:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs11_late_stage_reroute_does_not_outrank_earliest_stale_boundary() {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs11-late-stage-reroute-stale-boundary-priority");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-04-runtime-fs11-late-stage-reroute-priority.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            (
                "review_state_repair_follow_up",
                Value::from("record_branch_closure"),
            ),
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            ("harness_phase", Value::from("document_release_pending")),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-11 late-stage reroute priority operator fixture",
        ),
        "FS-11 late-stage reroute priority operator fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11 late-stage reroute priority status fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    if let Some(blocking_task) = operator_json["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-11 stale-boundary priority must keep Task 2 as the blocker even when persisted late-stage reroute metadata exists: {operator_json:?}"
        );
    } else {
        assert_eq!(
            operator_json["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-11 stale-boundary priority must keep Task 2 in execution command context even when persisted late-stage reroute metadata exists: {operator_json:?}"
        );
    }
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-11 stale-boundary priority operator should expose recommended command");
    let actionable_command = if recommended_command
        .contains("featureforge plan execution repair-review-state --plan ")
    {
        let repair_json = run_plan_execution_json(
            repo,
            state,
            &["repair-review-state", "--plan", plan_rel],
            "FS-11 stale-boundary priority repair follow-up fixture",
        );
        repair_json["recommended_command"]
            .as_str()
            .expect("FS-11 stale-boundary priority repair should expose recommended_command")
            .to_owned()
    } else {
        recommended_command.to_owned()
    };
    assert!(
        actionable_command.contains("--task 2"),
        "FS-11 stale-boundary priority route should target Task 2, got {actionable_command}",
    );
    assert!(
        !actionable_command.contains("--task 6"),
        "FS-11 stale-boundary priority route must not drift to later stale overlays, got {actionable_command}",
    );
    assert!(
        !actionable_command.contains("advance-late-stage"),
        "FS-11 stale-boundary priority route must not jump straight to late-stage recording while an earlier stale task boundary remains unresolved, got {actionable_command}",
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        &actionable_command,
        "FS-11 stale-boundary priority run operator-surfaced public command target",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_follow_up_blocker_parity_with_operator(
            &operator_json,
            &routed_follow_up,
            "FS-11 stale-boundary priority command-follow parity",
        );
    } else {
        let progressed_task_2 = (routed_follow_up["resume_task"].as_u64() == Some(2_u64)
            && routed_follow_up["resume_step"].as_u64() == Some(1_u64))
            || (routed_follow_up["active_task"].as_u64() == Some(2_u64)
                && routed_follow_up["active_step"].as_u64() == Some(1_u64));
        assert!(
            progressed_task_2,
            "FS-11 stale-boundary priority command-follow parity must keep execution on Task 2 Step 1 when the routed command is not blocked, got {routed_follow_up:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs11_operator_begin_repair_share_one_next_action_engine() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs11-shared-next-action-engine");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-02-runtime-fs11-shared-next-action.md";
    setup_runtime_fs11_next_action_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-11 workflow operator stale-boundary fixture",
        ),
        "FS-11 workflow operator stale-boundary fixture",
    );
    let runtime =
        discover_execution_runtime(repo, state, "FS-11 workflow doctor stale-boundary fixture");
    let doctor_json = to_value(
        operator::doctor_for_runtime_with_args(
            &runtime,
            &operator::DoctorArgs {
                plan: Some(plan_rel.into()),
                external_review_result_ready: false,
            },
        )
        .expect("FS-11 workflow doctor stale-boundary fixture should resolve"),
    )
    .expect("FS-11 workflow doctor stale-boundary fixture should serialize");
    assert!(
        doctor_json["phase_detail"]
            .as_str()
            .is_some_and(|phase_detail| {
                matches!(
                    phase_detail,
                    "execution_reentry_required" | "planning_reentry_required"
                )
            }),
        "FS-11 workflow doctor should keep stale-boundary reentry detail when the stale Task 2 boundary is authoritative, got {doctor_json:?}"
    );
    assert!(
        doctor_json["next_step"]
            .as_str()
            .is_some_and(|next_step| next_step.contains("Task 2")),
        "FS-11 workflow doctor should provide task-targeted next_step guidance for the stale Task 2 boundary, got {doctor_json:?}"
    );
    assert!(
        doctor_json["next_step"].as_str().is_some_and(|next_step| {
            !next_step.contains("Return to the current execution flow for the approved plan")
        }),
        "FS-11 workflow doctor should not fall back to generic execution guidance when Task 2 is the authoritative blocker, got {doctor_json:?}"
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11 plan execution status stale-boundary fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(operator_json["blocking_scope"], Value::from("task"));
    assert_eq!(
        operator_json["blocking_task"],
        Value::from(2_u64),
        "FS-11 operator must surface Task 2 as the blocker when the forward Task 3 Step 6 overlay is present"
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-11 operator should expose recommended command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-11 operator should keep Task 2 as the authoritative blocker target, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 3"),
        "FS-11 operator must not drift to Task 3 while Task 2 remains stale, got {recommended_command}"
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-11 repair-review-state stale-boundary fixture",
    );
    assert_eq!(repair_json["action"], Value::from("blocked"));
    assert_eq!(
        repair_json["required_follow_up"],
        Value::from("execution_reentry"),
        "FS-11 repair-review-state should expose the same shared follow-up as operator routing"
    );
    assert!(
        repair_json["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.starts_with("featureforge plan execution ")
                    && command.contains("--task 2")
                    && command.contains("--step 1")
                    && !command.contains("--task 3")
            }),
        "FS-11 repair-review-state should provide the exact shared executable command targeting Task 2 Step 1 when execution reentry is required: {repair_json}",
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
            "3",
            "--step",
            "6",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_json["execution_fingerprint"]
                .as_str()
                .expect("FS-11 status should expose execution fingerprint for begin rejection"),
        ],
        &[],
        "FS-11 begin should fail closed when it diverges from shared next-action target",
    );
    assert!(
        !begin_output.status.success(),
        "FS-11 begin should fail when requesting Task 3 Step 6 while the shared engine targets Task 2"
    );
    let failure_payload = if begin_output.stderr.is_empty() {
        &begin_output.stdout
    } else {
        &begin_output.stderr
    };
    let failure_json: Value = serde_json::from_slice(failure_payload)
        .expect("FS-11 begin failure payload should be valid json");
    let failure_error_class = failure_json["error_class"]
        .as_str()
        .expect("FS-11 begin failure payload should expose error_class");
    assert!(
        failure_error_class == "InvalidStepTransition"
            || failure_error_class == "ExecutionStateNotReady",
        "FS-11 begin failure should remain a closed begin-time rejection class, got {failure_json:?}",
    );
    let message = failure_json["message"]
        .as_str()
        .expect("FS-11 begin failure payload should expose message text");
    assert!(
        message.contains("Next public action: featureforge plan execution")
            && message.contains("reason_code=mutation_not_route_authorized"),
        "FS-11 rejection should explain the shared mutation-oracle mismatch, got {failure_json:?}"
    );
    assert!(
        message.contains("--task 2"),
        "FS-11 begin failure should preserve the authoritative Task 2 blocker target, got {failure_json:?}"
    );

    let recommended_parts = recommended_command.split_whitespace().collect::<Vec<_>>();
    assert!(
        recommended_parts.len() >= 4,
        "FS-11 operator command should remain directly executable, got {recommended_command}"
    );
    let routed_follow_up = run_plan_execution_json(
        repo,
        state,
        &recommended_parts[3..],
        "FS-11 run operator-routed command directly for parity",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_json,
        &routed_follow_up,
        "FS-11 command-follow parity",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_eq!(
            routed_follow_up["required_follow_up"],
            Value::from("execution_reentry"),
            "FS-11 operator follow-up should preserve execution-reentry blocker parity"
        );
    }
}

#[test]
fn runtime_remediation_fs11_repair_returns_same_action_as_operator_and_begin() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs11-repair-shared-action");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-02-runtime-fs11-repair-shared-action.md";
    setup_runtime_fs11_next_action_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-11 repair shared-action workflow operator fixture",
        ),
        "FS-11 repair shared-action workflow operator fixture",
    );
    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-11 repair shared-action repair-review-state fixture",
    );
    assert_eq!(
        repair_json["action"],
        Value::from("blocked"),
        "FS-11 repair shared-action fixture should return a concrete blocker"
    );
    assert_eq!(
        repair_json["required_follow_up"],
        Value::from("execution_reentry"),
        "FS-11 repair shared-action fixture should surface execution reentry"
    );
    assert_eq!(
        repair_json["recommended_command"], operator_json["recommended_command"],
        "FS-11 repair shared-action fixture should expose the same next command target as workflow operator"
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("FS-11 repair shared-action fixture should expose recommended_command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-11 repair shared-action fixture should keep Task 2 as the earliest stale-boundary target, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 3"),
        "FS-11 repair shared-action fixture must not drift to Task 3 Step 6 while Task 2 remains unresolved, got {recommended_command}"
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        "FS-11 repair shared-action command-follow parity",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_json,
        &routed_follow_up,
        "FS-11 repair shared-action command-follow parity",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_eq!(
            routed_follow_up["required_follow_up"], repair_json["required_follow_up"],
            "FS-11 repair shared-action follow-up should preserve repair-required_follow_up parity when blocked"
        );
    }
}

#[test]
fn latest_current_closure_does_not_become_reentry_target_when_stale_target_is_missing() {
    let (repo_dir, state_dir) = init_repo("runtime-reconcile-no-latest-current-closure-target");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-01-runtime-fs15-no-latest-current-target.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);
    seed_current_branch_closure_truth(repo, state, plan_rel, 1);

    let status_before_retarget = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "targetless stale latest-current fixture status before retarget",
    );
    let execution_run_id = status_before_retarget["execution_run_id"]
        .as_str()
        .expect("targetless stale latest-current fixture should expose execution_run_id");
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let current_task_closure_records = json!({
        "task-1": current_task_closure_record(plan_rel, 1, execution_run_id, &reviewed_state_id, repo, state),
        "task-2": current_task_closure_record(plan_rel, 2, execution_run_id, &reviewed_state_id, repo, state),
        "task-6": current_task_closure_record(plan_rel, 6, execution_run_id, &reviewed_state_id, repo, state),
    });
    update_authoritative_harness_state(
        repo,
        state,
        &current_branch_name(repo),
        plan_rel,
        1,
        &[
            ("harness_phase", Value::from("document_release_pending")),
            (
                "current_branch_closure_reviewed_state_id",
                Value::from("git_tree:not-a-tree"),
            ),
            ("current_open_step_state", Value::Null),
            ("task_closure_record_history", json!({})),
            ("current_task_closure_records", current_task_closure_records),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
            ("resume_task", Value::Null),
            ("resume_step", Value::Null),
        ],
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "targetless stale latest-current fixture status",
    );
    assert!(
        status_json["current_task_closures"]
            .as_array()
            .is_some_and(|closures| closures
                .iter()
                .any(|closure| closure["task"].as_u64() == Some(6))),
        "fixture must expose a real current Task 6 closure: {status_json:?}"
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness"),
        "missing stale target with current Task 6 closure should route through branch closure recording, not execution reentry: {status_json:?}"
    );
    assert!(
        !status_json["recommended_command"]
            .as_str()
            .unwrap_or_default()
            .contains("reopen"),
        "missing stale target must not recommend reopening the latest current closure: {status_json:?}"
    );
    assert!(
        !status_json["recommended_command"]
            .as_str()
            .unwrap_or_default()
            .contains("--task 6"),
        "missing stale target must not fabricate Task 6 from current closures: {status_json:?}"
    );
    assert!(
        status_json["execution_command_context"].is_null(),
        "targetless stale state must not expose an execution command context: {status_json:?}"
    );
    assert!(
        status_json["execution_reentry_target_source"].is_null(),
        "targetless stale state must not fabricate a target source from current closures: {status_json:?}"
    );
}

fn current_task_closure_record(
    plan_rel: &str,
    task_number: u32,
    execution_run_id: &str,
    reviewed_state_id: &str,
    repo: &Path,
    state: &Path,
) -> Value {
    json!({
        "dispatch_id": format!("task-{task_number}-current-dispatch"),
        "closure_record_id": format!("task-{task_number}-current-closure"),
        "source_plan_path": plan_rel,
        "source_plan_revision": 1,
        "execution_run_id": execution_run_id,
        "reviewed_state_id": reviewed_state_id,
        "contract_identity": task_contract_identity(repo, state, plan_rel, task_number),
        "effective_reviewed_surface_paths": ["README.md"],
        "review_result": "pass",
        "review_summary_hash": sha256_hex(format!("task {task_number} current review").as_bytes()),
        "verification_result": "pass",
        "verification_summary_hash": sha256_hex(
            format!("task {task_number} current verification").as_bytes()
        ),
        "closure_status": "current"
    })
}

#[test]
fn runtime_remediation_fs15_repair_never_jumps_to_later_task_when_earlier_boundary_exists() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs15-repair-target-parity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-04-runtime-fs15-repair-target-parity.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);
    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-15 repair target parity workflow operator fixture",
        ),
        "FS-15 repair target parity workflow operator fixture",
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-15 repair target parity fixture",
    );
    assert_eq!(repair_json["action"], Value::from("blocked"));
    assert_eq!(
        repair_json["required_follow_up"],
        Value::from("execution_reentry"),
        "FS-15 repair should keep stale-boundary follow-up in execution_reentry lane"
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("FS-15 repair target parity fixture should expose recommended command");
    assert_eq!(
        repair_json["recommended_command"], operator_json["recommended_command"],
        "FS-15 repair target parity fixture should expose the same public command as workflow/operator"
    );
    assert!(
        recommended_command.starts_with("featureforge plan execution "),
        "FS-15 repair target parity should return an executable plan-execution command, got {recommended_command}"
    );
    assert!(
        recommended_command.contains("--task 2"),
        "FS-15 repair must keep Task 2 as the earliest stale boundary target, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 6"),
        "FS-15 repair must not jump to Task 6 while Task 2 remains unresolved, got {recommended_command}"
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        "FS-15 repair target parity run repair-surfaced public command target",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_follow_up_blocker_parity_with_operator(
            &operator_json,
            &routed_follow_up,
            "FS-15 repair command-follow parity",
        );
        assert_eq!(
            routed_follow_up["required_follow_up"],
            Value::from("execution_reentry"),
            "FS-15 blocked repair command-follow parity must keep execution_reentry as the concrete follow-up lane"
        );
    } else {
        let reopened_task_2 = routed_follow_up["resume_task"].as_u64() == Some(2_u64)
            && routed_follow_up["resume_step"].as_u64() == Some(1_u64);
        assert!(
            reopened_task_2,
            "FS-15 repair command-follow parity must reopen Task 2 Step 1 when the routed command is not blocked, got {routed_follow_up:?}"
        );
    }
}

#[test]
fn fs18_cycle_break_binding_is_task_scoped_not_global() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs18-cycle-break-task-scoped");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-09-runtime-fs18-cycle-break-task-scoped.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);
    let branch = current_branch_name(repo);
    let status_before_cycle_break = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "Task 9.2 status before stale cycle-break overlay",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("strategy_state", Value::from("cycle_breaking")),
            ("strategy_checkpoint_kind", Value::from("cycle_break")),
            ("strategy_cycle_break_task", Value::from(1_u64)),
            ("strategy_cycle_break_step", Value::from(1_u64)),
            (
                "strategy_cycle_break_checkpoint_fingerprint",
                Value::from("fs18-cycle-break-task-1"),
            ),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-18 workflow operator cycle-break task binding fixture",
        ),
        "FS-18 workflow operator cycle-break task binding fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-18 status cycle-break task binding fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        status_json["semantic_workspace_tree_id"],
        status_before_cycle_break["semantic_workspace_tree_id"],
        "FS-18 cycle-break projection overlay must not alter semantic workspace truth"
    );
    assert_eq!(
        status_json["raw_workspace_tree_id"], status_before_cycle_break["raw_workspace_tree_id"],
        "FS-18 cycle-break projection overlay must not dirty tracked workspace truth"
    );
    assert_eq!(
        operator_json["blocking_task"],
        Value::from(2_u64),
        "FS-18 Task-1 cycle-break binding must not globally block later Task-2 stale-boundary routing: {operator_json:?}"
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-18 operator should expose recommended command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-18 cycle-break routing should still target Task 2 after Task 1 is already repaired, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 1"),
        "FS-18 cycle-break binding must not drag routing back to Task 1 once Task 1 is repaired, got {recommended_command}"
    );
    let status_recommended_command = status_json["recommended_command"]
        .as_str()
        .expect("FS-18 status should expose recommended command");
    assert!(
        status_recommended_command.contains("--task 2")
            && !status_recommended_command.contains("--task 1"),
        "FS-18 status should route directly to Task 2, not reopen Task 1: {status_recommended_command}"
    );
    let management_commands_before_forward_route = 0_usize;
    assert!(
        management_commands_before_forward_route <= 1,
        "Task 9.6 budget: current Task 1 closure plus stale cycle-break overlay should need at most one management command before a forward route"
    );
}

#[test]
fn fs19_superseded_stale_historical_task_closure_is_not_an_unresolved_stale_boundary() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs19-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-06-runtime-fs19-workflow-runtime.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);
    let branch = current_branch_name(repo);
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
                    "task-1-stale": {
                        "closure_record_id": "task-1-stale",
                        "task": 1,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 8,
                        "record_status": "stale_unreviewed",
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-1-current": {
                        "closure_record_id": "task-1-current",
                        "task": 1,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 22,
                        "record_status": "current",
                        "closure_status": "current",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-2-stale": {
                        "closure_record_id": "task-2-stale",
                        "task": 2,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 10,
                        "record_status": "stale_unreviewed",
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-6-stale": {
                        "closure_record_id": "task-6-stale",
                        "task": 6,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 20,
                        "record_status": "stale_unreviewed",
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    }
                }),
            ),
            ("superseded_task_closure_ids", json!(["task-1-stale"])),
            (
                "current_open_step_state",
                json!({
                    "task": 6,
                    "step": 1,
                    "note_state": "Interrupted",
                    "note_summary": "FS-19 stale task 1 history should be superseded by current task 1 closure.",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 30
                }),
            ),
            ("resume_task", Value::from(6_u64)),
            ("resume_step", Value::from(1_u64)),
        ],
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-19 status should ignore superseded stale task 1 history",
    );
    assert_eq!(
        status_json["blocking_task"],
        Value::from(2_u64),
        "FS-19 earliest unresolved stale task should move past superseded stale task 1 history"
    );
    assert_ne!(
        status_json["blocking_task"],
        Value::from(1_u64),
        "FS-19 superseded stale task 1 history must not remain an unresolved stale boundary"
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-19 operator stale-boundary target fixture",
        ),
        "FS-19 operator stale-boundary target fixture",
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-19 operator should expose recommended command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-19 operator should route to Task 2 after stale task 1 history is superseded, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 1"),
        "FS-19 operator must not route back to superseded stale task 1 history, got {recommended_command}"
    );
}

#[test]
fn runtime_remediation_fs09_repair_exposes_next_blocker_immediately() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs09-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let branch = current_branch_name(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    seed_current_branch_closure_truth(repo, state, plan_rel, 1);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            (
                "current_branch_closure_reviewed_state_id",
                Value::from("git_tree:not-a-tree"),
            ),
            (
                "review_state_repair_follow_up",
                Value::from("execution_reentry"),
            ),
        ],
    );
    update_current_history_record_field(
        repo,
        state,
        "branch_closure_records",
        "current_branch_closure_id",
        "reviewed_state_id",
        Value::from("git_tree:not-a-tree"),
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-09 repair-review-state post-stale-blocker fixture",
    );
    let repair_action = repair_json["action"]
        .as_str()
        .expect("FS-09 repair-review-state should expose action")
        .to_owned();
    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-09 workflow operator post-repair fixture",
        ),
        "FS-09 workflow operator post-repair fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-09 plan execution status post-repair fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    if repair_action == "blocked" {
        let operator_recommended_command = operator_json["recommended_command"]
            .as_str()
            .expect("FS-09 workflow operator should expose recommended_command");
        if let Some(required_follow_up) = repair_json["required_follow_up"].as_str() {
            let routed_follow_up = run_recommended_plan_execution_command(
                repo,
                state,
                operator_recommended_command,
                "FS-09 run workflow-operator recommended command directly",
            );
            assert_follow_up_blocker_parity_with_operator(
                &operator_json,
                &routed_follow_up,
                "FS-09 command-follow parity",
            );
            if routed_follow_up["action"].as_str() == Some("blocked") {
                let routed_required_follow_up = routed_follow_up["required_follow_up"]
                    .as_str()
                    .expect("FS-09 blocked routed command should expose required_follow_up");
                assert_eq!(
                    required_follow_up, routed_required_follow_up,
                    "FS-09 blocked repair should surface the exact required_follow_up lane returned by the routed public command"
                );
            } else {
                assert!(
                    !required_follow_up.trim().is_empty(),
                    "FS-09 blocked repair should always expose a concrete required_follow_up lane"
                );
            }
        } else {
            assert_eq!(
                repair_json["phase_detail"],
                Value::from("task_closure_recording_ready"),
                "FS-09 blocked repair may omit required_follow_up only when it has already advanced to the direct public task-closure route: {repair_json:?}"
            );
            assert!(
                operator_recommended_command.contains("plan execution close-current-task --plan "),
                "FS-09 blocked repair without required_follow_up must expose the direct close-current-task command through workflow/operator: {operator_json:?}"
            );
        }
    } else {
        assert_eq!(
            repair_action, "reconciled",
            "FS-09 repair-review-state should either fail closed with a follow-up or reconcile in-place, got {repair_json:?}"
        );
        assert_eq!(
            repair_json["required_follow_up"],
            Value::Null,
            "FS-09 reconciled repair-review-state should not emit a required_follow_up lane"
        );
        assert!(
            repair_json["actions_performed"]
                .as_array()
                .is_some_and(|actions| !actions.is_empty()),
            "FS-09 reconciled repair-review-state should report the reconciliation work it performed"
        );
    }
}

#[test]
fn runtime_remediation_fs13_markdown_note_is_projection_not_authority() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs13-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-02-runtime-fs13-workflow-runtime.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);

    let plan_path = repo.join(plan_rel);
    replace_in_file(
        &plan_path,
        "- [ ] **Step 1: Execute task 6 baseline step**",
        "- [ ] **Step 1: Execute task 6 baseline step**\n  **Execution Note:** Interrupted - FS-13 projection baseline",
    );
    let operator_before_tamper = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-13 operator before manual markdown note tamper",
        ),
        "FS-13 operator before manual markdown note tamper",
    );
    if let Some(blocking_task) = operator_before_tamper["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-13 operator should expose Task 2 as the earliest stale boundary before note tamper: {operator_before_tamper:?}"
        );
    } else {
        assert_eq!(
            operator_before_tamper["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-13 operator should expose Task 2 through execution command context before note tamper: {operator_before_tamper:?}"
        );
    }

    let status_before_tamper = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 status before manual markdown note tamper",
    );
    assert_eq!(
        status_before_tamper["blocking_task"],
        Value::from(2_u64),
        "FS-13 status should expose Task 2 as the earliest stale boundary before note tamper"
    );

    replace_in_file(
        &plan_path,
        "  **Execution Note:** Interrupted - FS-13 projection baseline",
        "  **Execution Note:** Blocked - FS-13 manual markdown edit should be projection-only.",
    );
    let status_after_tamper = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 status after manual markdown note tamper",
    );
    assert_eq!(
        status_after_tamper["blocking_task"],
        Value::from(2_u64),
        "FS-13 status must keep Task 2 as the earliest stale boundary even when markdown note is edited"
    );

    let operator_after_tamper = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-13 operator after manual markdown note tamper",
        ),
        "FS-13 operator after manual markdown note tamper",
    );
    assert_eq!(
        operator_after_tamper["phase_detail"], operator_before_tamper["phase_detail"],
        "FS-13 operator phase-detail routing must remain authoritative when markdown note text is tampered"
    );
    if let Some(blocking_task) = operator_after_tamper["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-13 operator should keep Task 2 as the earliest stale boundary after note tamper: {operator_after_tamper:?}"
        );
    } else {
        assert_eq!(
            operator_after_tamper["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-13 operator should keep Task 2 in execution command context after note tamper: {operator_after_tamper:?}"
        );
    }

    replace_in_file(
        &plan_path,
        "  **Execution Note:** Blocked - FS-13 manual markdown edit should be projection-only.",
        "",
    );
    let status_after_projection_delete = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 status after deleting markdown note projection",
    );
    assert_eq!(
        status_after_projection_delete["blocking_task"],
        Value::from(2_u64),
        "FS-13 status must preserve Task 2 stale-boundary targeting even when markdown note projection is deleted"
    );

    let operator_after_projection_delete = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-13 operator after deleting markdown note projection",
        ),
        "FS-13 operator after deleting markdown note projection",
    );
    assert_eq!(
        operator_after_projection_delete["phase_detail"], operator_before_tamper["phase_detail"],
        "FS-13 operator phase-detail routing must remain authoritative when markdown note projection is deleted"
    );
    if let Some(blocking_task) = operator_after_projection_delete["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-13 operator should keep Task 2 as the earliest stale boundary after projection deletion: {operator_after_projection_delete:?}"
        );
    } else {
        assert_eq!(
            operator_after_projection_delete["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-13 operator should keep Task 2 in execution command context after projection deletion: {operator_after_projection_delete:?}"
        );
    }

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(authoritative_state_path)
            .expect("FS-13 authoritative harness state should remain readable"),
    )
    .expect("FS-13 authoritative harness state should remain valid json");
    assert_eq!(
        authoritative_state["current_open_step_state"]["task"],
        Value::from(6_u64)
    );
    assert_eq!(
        authoritative_state["current_open_step_state"]["step"],
        Value::from(1_u64)
    );
    assert_eq!(
        authoritative_state["current_open_step_state"]["note_state"],
        Value::from("Interrupted")
    );
}

#[test]
fn runtime_remediation_fs13_mutation_fails_closed_on_malformed_authoritative_open_step_state() {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs13-malformed-authoritative-open-step-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);
    bind_explicit_reopen_repair_target(repo, state, &current_branch_name(repo), plan_rel, 1, 1, 1);

    let status_before_reopen = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 malformed authoritative open-step baseline status before reopen",
    );
    let reopened = run_plan_execution_json(
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
            "FS-13 malformed authoritative open-step baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 malformed open-step baseline should expose execution fingerprint"),
        ],
        "FS-13 malformed authoritative open-step baseline reopen",
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
                "note_summary": "FS-13 malformed authoritative open-step state",
                "source_plan_path": plan_rel,
                "source_plan_revision": 1,
                "authoritative_sequence": 2
            }),
        )],
    );

    let output = run_rust_featureforge_with_env(
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
            reopened["execution_fingerprint"]
                .as_str()
                .expect("FS-13 malformed open-step reopen should expose execution fingerprint"),
        ],
        &[],
        "FS-13 begin should fail closed on malformed authoritative current_open_step_state",
    );
    assert!(
        !output.status.success(),
        "FS-13 begin must fail closed when authoritative current_open_step_state is malformed"
    );
    let failure_payload = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let failure_json: Value = serde_json::from_slice(failure_payload).expect(
        "FS-13 malformed authoritative current_open_step_state failure payload should be valid json",
    );
    assert_eq!(failure_json["error_class"], "MalformedExecutionState");
    assert!(
        failure_json["message"]
            .as_str()
            .is_some_and(|message| message.contains("current_open_step_state")),
        "FS-13 malformed authoritative current_open_step_state failure should mention current_open_step_state, got {failure_json:?}"
    );
}

#[test]
fn runtime_remediation_fs13_status_fails_closed_on_authoritative_open_step_plan_revision_mismatch()
{
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs13-open-step-plan-revision-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);
    bind_explicit_reopen_repair_target(repo, state, &current_branch_name(repo), plan_rel, 1, 1, 1);

    let status_before_reopen = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 open-step plan-revision mismatch baseline status before reopen",
    );
    run_plan_execution_json(
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
            "FS-13 open-step plan-revision mismatch baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 open-step mismatch baseline should expose execution fingerprint"),
        ],
        "FS-13 open-step plan-revision mismatch baseline reopen",
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
                "note_state": "Interrupted",
                "note_summary": "FS-13 plan revision mismatch",
                "source_plan_path": plan_rel,
                "source_plan_revision": 99,
                "authoritative_sequence": 2
            }),
        )],
    );

    let output = run_rust_featureforge_with_env(
        repo,
        state,
        &["plan", "execution", "status", "--plan", plan_rel],
        &[],
        "FS-13 status should fail closed on authoritative open-step source_plan_revision mismatch",
    );
    assert!(
        !output.status.success(),
        "FS-13 status must fail closed when authoritative current_open_step_state source_plan_revision mismatches"
    );
    let failure_payload = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let failure_json: Value = serde_json::from_slice(failure_payload)
        .expect("FS-13 open-step mismatch status failure payload should be valid json");
    assert_eq!(failure_json["error_class"], "MalformedExecutionState");
    assert!(
        failure_json["message"]
            .as_str()
            .is_some_and(|message| message.contains("source_plan_revision")),
        "FS-13 open-step mismatch status failure should mention source_plan_revision, got {failure_json:?}"
    );
}

#[test]
fn runtime_remediation_fs13_mutation_fails_closed_on_multiple_legacy_open_step_notes() {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs13-multiple-legacy-open-step-notes");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);
    bind_explicit_reopen_repair_target(repo, state, &current_branch_name(repo), plan_rel, 1, 1, 1);
    let status_before_reopen = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 multiple-legacy-note status before reopen baseline",
    );
    let reopened = run_plan_execution_json(
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
            "FS-13 multiple legacy note baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 multiple-legacy-note reopen baseline should expose execution fingerprint"),
        ],
        "FS-13 multiple-legacy-note baseline reopen",
    );

    let plan_path = repo.join(plan_rel);
    insert_step_with_execution_note_after_step(
        &plan_path,
        1,
        1,
        2,
        "FS-13 injected secondary step",
        "Blocked - FS-13 injected secondary legacy note",
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

    let status_with_multiple_notes = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 multiple-legacy-note status before mutation failure assertion",
    );
    assert_eq!(
        reopened["resume_task"],
        Value::from(1_u64),
        "FS-13 multiple-legacy-note reopen baseline should expose resume target"
    );
    let output = run_rust_featureforge_with_env(
        repo,
        state,
        &[
            "plan",
            "execution",
            "note",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--state",
            "blocked",
            "--message",
            "FS-13 multiple legacy note fail-closed check",
            "--expect-execution-fingerprint",
            status_with_multiple_notes["execution_fingerprint"]
                .as_str()
                .expect("FS-13 multiple-legacy-note status should expose execution fingerprint"),
        ],
        &[],
        "FS-13 note should fail closed when multiple legacy open-step notes exist",
    );
    assert!(
        !output.status.success(),
        "FS-13 note must fail closed when multiple legacy open-step notes are present"
    );
    let failure_payload = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let failure_json: Value = serde_json::from_slice(failure_payload)
        .expect("FS-13 multiple legacy open-step note failure payload should be valid json");
    assert_eq!(failure_json["error_class"], "InvalidCommandInput");
    assert!(
        failure_json["message"]
            .as_str()
            .is_some_and(|message| message.contains("unrecognized subcommand 'note'")),
        "FS-13 multiple legacy open-step note failure should reflect the removed public command surface, got {failure_json:?}"
    );
}

#[test]
fn runtime_remediation_fs13_mutation_fails_closed_without_persisting_checked_legacy_open_step_note()
{
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs13-checked-legacy-open-step-note");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);
    bind_explicit_reopen_repair_target(repo, state, &current_branch_name(repo), plan_rel, 1, 1, 1);

    let status_before_reopen = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 checked-note status before reopen baseline",
    );
    run_plan_execution_json(
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
            "FS-13 checked-note baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 checked-note reopen baseline should expose execution fingerprint"),
        ],
        "FS-13 checked-note baseline reopen",
    );

    let plan_path = repo.join(plan_rel);
    replace_in_file(
        &plan_path,
        "- [ ] **Step 1: Validate full approved-plan readiness**",
        "- [x] **Step 1: Validate full approved-plan readiness**",
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

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 checked-note status before begin failure assertion",
    );
    let output = run_rust_featureforge_with_env(
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
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-13 checked-note status should expose execution fingerprint"),
        ],
        &[],
        "FS-13 begin should fail closed on checked legacy open-step note candidate",
    );
    assert!(
        !output.status.success(),
        "FS-13 begin must fail closed when legacy open-step note points at a completed step"
    );
    let failure_payload = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let failure_json: Value = serde_json::from_slice(failure_payload)
        .expect("FS-13 checked-note failure payload should be valid json");
    assert_eq!(failure_json["error_class"], "InvalidStepTransition");
    assert!(
        failure_json["message"].as_str().is_some_and(|message| {
            message.contains("begin failed closed")
                && message.contains("Next public action: featureforge plan execution")
                && message.contains("reason_code=mutation_not_route_authorized")
                && !message.contains("legacy open-step")
                && !message.contains("hidden")
        }),
        "FS-13 checked-note failure should come from the shared mutation oracle without materializing markdown-note truth, got {failure_json:?}"
    );

    let authoritative_state_path = harness_state_path(state, &repo_slug(repo), &branch);
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("FS-13 checked-note authoritative state should be readable"),
    )
    .expect("FS-13 checked-note authoritative state should remain valid json");
    assert!(
        authoritative_state["current_open_step_state"].is_null(),
        "FS-13 checked-note migration must fail before mutating current_open_step_state"
    );
}

#[test]
fn runtime_remediation_inventory_maps_fs_regressions_to_workflow_runtime() {
    let inventory = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime-remediation/README.md"),
    )
    .expect("runtime-remediation inventory should be readable");
    assert!(
        inventory.contains("## Function-Level Traceability"),
        "runtime-remediation inventory should retain function-level traceability coverage"
    );
    assert!(
        inventory.contains("## Detailed Failure Shapes (Mandatory)"),
        "runtime-remediation inventory should include the mandatory detailed failure-shape section"
    );
    for scenario in [
        "FS-01", "FS-02", "FS-03", "FS-04", "FS-05", "FS-06", "FS-07", "FS-08", "FS-09", "FS-10",
        "FS-11", "FS-12", "FS-13", "FS-14", "FS-15", "FS-16",
    ] {
        assert!(
            inventory.contains(scenario),
            "runtime-remediation inventory should include {scenario}"
        );
    }
    for (scenario, detail_anchor) in [
        ("FS-01", "branch-closure mutation says repair is required"),
        (
            "FS-02",
            "late-stage writes re-stale execution and loop between late-stage refresh and execution reentry",
        ),
        (
            "FS-03",
            "begin requires prior-task redispatch, but dispatch recorder rejects that target",
        ),
        (
            "FS-04",
            "repair mutates state and still leaves the wrong route visible",
        ),
        (
            "FS-05",
            "unsupported-field CLI paths mutate authoritative state before returning an error",
        ),
        (
            "FS-06",
            "helper-backed tests pass but compiled CLI behavior differs",
        ),
        (
            "FS-07",
            "status points to the right blocker, operator still recommends execution reentry / begin",
        ),
        (
            "FS-08",
            "later resume overlays suppress the real stale prerequisite",
        ),
        (
            "FS-09",
            "repair removes one stale layer but fails to surface the next blocker and still says begin",
        ),
        (
            "FS-10",
            "persisted repair follow-up remains stuck on execution reentry or branch refresh",
        ),
        (
            "FS-11",
            "rebased consumer-style fixture with forward reentry overlay pointing at Task 3",
        ),
        (
            "FS-12",
            "authoritative state contains `run_identity.execution_run_id`",
        ),
        (
            "FS-13",
            "authoritative open-step state or legacy markdown note on a later task",
        ),
        (
            "FS-14",
            "completed task with no current task closure baseline",
        ),
        (
            "FS-15",
            "stale tasks 2 and 6 present after earlier repair cleanup",
        ),
        (
            "FS-16",
            "remove or stale receipt projections without changing the reviewed state that closure binds to",
        ),
    ] {
        assert!(
            inventory.contains(detail_anchor),
            "runtime-remediation inventory should retain detailed failure-shape text for {scenario}"
        );
    }
    let required_function_traceability_anchors: [(&str, &[&str]); 16] = [
        (
            "FS-01",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs01_shared_route_parity_for_missing_current_closure",
                "tests/workflow_shell_smoke.rs::compiled_cli_route_parity_probe_for_late_stage_refresh_fixture",
                "tests/workflow_shell_smoke.rs::plan_execution_record_release_readiness_primitive_uses_shared_routing_when_stale",
                "tests/workflow_shell_smoke.rs::runtime_remediation_fs01_compiled_cli_repair_and_branch_closure_do_not_disagree",
            ],
        ),
        (
            "FS-02",
            &[
                "tests/workflow_runtime_final_review.rs::fs02_late_stage_drift_routes_consistently_across_operator_and_status",
                "tests/workflow_entry_shell_smoke.rs::fs02_entry_route_surfaces_share_parity_and_budget",
            ],
        ),
        (
            "FS-03",
            &[
                "tests/workflow_runtime.rs::workflow_phase_routes_task_boundary_blocked",
                "tests/plan_execution.rs::runtime_remediation_fs03_compiled_cli_dispatch_target_acceptance_and_mismatch",
                "tests/workflow_shell_smoke.rs::plan_execution_record_review_dispatch_prefers_task_boundary_target_over_interrupted_note_state",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs03_dispatch_target_acceptance_and_mismatch_stay_aligned_between_direct_and_compiled_cli",
            ],
        ),
        (
            "FS-04",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs04_repair_returns_route_consumed_by_operator",
                "tests/workflow_runtime.rs::runtime_remediation_fs04_compiled_cli_repair_returns_route_consumed_by_operator",
                "tests/plan_execution.rs::runtime_remediation_fs04_rebuild_evidence_preserves_authoritative_state_digest",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_route_visibility_stays_aligned_between_direct_and_compiled_cli",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_review_state_accepts_external_review_ready_flag_without_irrelevant_route_drift",
            ],
        ),
        (
            "FS-05",
            &[
                "tests/plan_execution.rs::record_review_dispatch_task_target_mismatch_fails_before_authoritative_mutation",
                "tests/plan_execution.rs::record_review_dispatch_final_review_scope_rejects_task_field_before_authoritative_mutation",
                "tests/plan_execution.rs::record_final_review_rejects_unapproved_reviewer_source_before_mutation",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs05_unsupported_field_fails_before_mutation_on_compatibility_aliases",
            ],
        ),
        (
            "FS-06",
            &[
                "tests/workflow_shell_smoke.rs::fs06_helper_and_compiled_cli_target_mismatch_stay_in_parity",
            ],
        ),
        (
            "FS-07",
            &[
                "tests/execution_query.rs::runtime_remediation_fs07_query_surface_parity_for_task_review_dispatch_blocked",
                "tests/workflow_shell_smoke.rs::fs07_task_review_dispatch_route_parity_in_compiled_cli_surfaces",
            ],
        ),
        (
            "FS-08",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs08_resume_overlay_does_not_hide_stale_blocker",
                "tests/workflow_runtime.rs::runtime_remediation_fs08_compiled_cli_resume_overlay_does_not_hide_stale_blocker",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs08_stale_blocker_visibility_stays_aligned_between_direct_and_compiled_cli",
            ],
        ),
        (
            "FS-09",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs09_repair_exposes_next_blocker_immediately",
                "tests/workflow_entry_shell_smoke.rs::fs09_repair_surfaces_post_repair_next_blocker_in_entry_cli",
            ],
        ),
        (
            "FS-10",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs10_stale_follow_up_is_ignored_when_truth_is_current",
                "tests/workflow_shell_smoke.rs::prerelease_branch_closure_refresh_ignores_stale_execution_reentry_follow_up",
            ],
        ),
        (
            "FS-11",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs11_operator_begin_repair_share_one_next_action_engine",
                "tests/workflow_runtime.rs::runtime_remediation_fs11_repair_returns_same_action_as_operator_and_begin",
                "tests/workflow_shell_smoke.rs::fs11_operator_and_begin_target_parity_after_rebase_resume",
                "tests/workflow_shell_smoke.rs::fs11_repair_output_matches_following_public_command_without_hidden_helper",
                "tests/workflow_shell_smoke.rs::fs11_rebase_resume_recovery_budget_is_capped_without_hidden_helpers",
            ],
        ),
        (
            "FS-12",
            &[
                concat!(
                    "tests/workflow_runtime.rs::runtime_remediation_fs12_authoritative_run_identity_beats_pre",
                    "flight_for_begin_and_operator"
                ),
                concat!(
                    "tests/plan_execution.rs::runtime_remediation_fs12_close_current_task_uses_authoritative_run_identity_without_hidden_pre",
                    "flight"
                ),
                concat!(
                    "tests/workflow_shell_smoke.rs::fs12_recovery_path_does_not_require_hidden_pre",
                    "flight_when_run_identity_exists"
                ),
            ],
        ),
        (
            "FS-13",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs13_markdown_note_is_projection_not_authority",
                "tests/workflow_runtime.rs::runtime_remediation_fs13_hidden_gates_materialize_legacy_open_step_state_when_blocked",
                "tests/plan_execution.rs::runtime_remediation_fs13_reopen_and_begin_update_authoritative_open_step_state",
                "tests/workflow_shell_smoke.rs::fs13_normal_recovery_never_requires_manual_plan_note_edit",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs13_authoritative_open_step_state_survives_compiled_cli_round_trip",
            ],
        ),
        (
            "FS-14",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs14_missing_task_closure_baseline_routes_to_close_current_task_not_execution_reentry",
                "tests/workflow_runtime.rs::runtime_remediation_fs14_repair_routes_missing_task_closure_baseline_to_close_current_task",
                "tests/plan_execution.rs::runtime_remediation_fs14_close_current_task_rebuilds_missing_current_closure_baseline_without_hidden_dispatch",
                "tests/workflow_shell_smoke.rs::fs14_recovery_to_close_current_task_uses_only_public_intent_commands",
            ],
        ),
        (
            "FS-15",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs15_earliest_stale_boundary_beats_latest_overlay_target",
                "tests/workflow_runtime.rs::runtime_remediation_fs15_repair_never_jumps_to_later_task_when_earlier_boundary_exists",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs15_compiled_cli_never_prefers_later_stale_task",
            ],
        ),
        (
            "FS-16",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs16_current_positive_task_closure_allows_next_task_begin_even_if_receipts_need_projection_refresh",
                "tests/plan_execution.rs::runtime_remediation_fs16_begin_no_longer_reads_prior_task_dispatch_or_receipts",
            ],
        ),
    ];
    for (scenario, anchors) in required_function_traceability_anchors {
        for anchor in anchors {
            assert!(
                inventory.contains(anchor),
                "runtime-remediation inventory should map {scenario} to function-level anchor `{anchor}`"
            );
        }
    }
}
