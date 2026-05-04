// Internal compatibility tests extracted from tests/contracts_execution_runtime_boundaries.rs.
// This file intentionally reuses the source fixture scaffolding from the public-facing integration test.

#[path = "support/internal_only_direct_helpers.rs"]
mod internal_only_direct_helpers;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/projection.rs"]
mod projection_support;
#[path = "support/public_featureforge_cli.rs"]
mod public_featureforge_cli;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/rust_source_scan.rs"]
mod rust_source_scan;
#[path = "support/workflow.rs"]
mod workflow_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use featureforge::execution::final_review::resolve_release_base_branch;
use featureforge::execution::query::{
    ExecutionRoutingState, query_workflow_routing_state_for_runtime,
};
use featureforge::execution::semantic_identity::{
    branch_definition_identity_for_context, task_definition_identity_for_task,
};
use featureforge::execution::state::load_execution_context;
use featureforge::git::{discover_repository, discover_slug_identity};
use featureforge::paths::harness_state_path;
use internal_only_direct_helpers::internal_runtime_direct;
use runtime_support::execution_runtime;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use workflow_support::{init_repo, install_full_contract_ready_artifacts};

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
const TASK_BOUNDARY_BLOCKED_PLAN_SOURCE: &str = r#"# Runtime Integration Hardening Implementation Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** featureforge:executing-plans
**Source Spec:** `docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md`
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
- Modify: `tests/contracts_execution_runtime_boundaries.rs`

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
- Modify: `tests/contracts_execution_runtime_boundaries.rs`

- [ ] **Step 1: Start the follow-on task**
"#;
const TASK_BOUNDARY_FS15_PLAN_SOURCE: &str = r#"# Runtime Remediation FS-15 Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1, Task 2, Task 3, Task 4, Task 5, Task 6
- VERIFY-001 -> Task 1, Task 2, Task 3, Task 4, Task 5, Task 6

## Execution Strategy

- Execute tasks serially from Task 1 through Task 6 so stale-boundary targeting order stays deterministic.

## Dependency Diagram

```text
Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6
```

## Task 1: FS-15 task 1

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Establishes the earliest completed baseline.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep one step per task for deterministic reopen targeting.

**Done when:**
- Establishes the earliest completed baseline.

**Files:**
- Modify: `tests/contracts_execution_runtime_boundaries.rs`

- [ ] **Step 1: Execute task 1 baseline step**

## Task 2: FS-15 task 2

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Represents the earliest unresolved stale boundary.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep one step per task for deterministic reopen targeting.

**Done when:**
- Represents the earliest unresolved stale boundary.

**Files:**
- Modify: `tests/contracts_execution_runtime_boundaries.rs`

- [ ] **Step 1: Execute task 2 baseline step**

## Task 3: FS-15 task 3

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Provides intermediate task numbering for stale-boundary ordering.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep one step per task for deterministic reopen targeting.

**Done when:**
- Provides intermediate task numbering for stale-boundary ordering.

**Files:**
- Modify: `tests/contracts_execution_runtime_boundaries.rs`

- [ ] **Step 1: Execute task 3 baseline step**

## Task 4: FS-15 task 4

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Provides intermediate task numbering for stale-boundary ordering.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep one step per task for deterministic reopen targeting.

**Done when:**
- Provides intermediate task numbering for stale-boundary ordering.

**Files:**
- Modify: `tests/contracts_execution_runtime_boundaries.rs`

- [ ] **Step 1: Execute task 4 baseline step**

## Task 5: FS-15 task 5

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Provides intermediate task numbering for stale-boundary ordering.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep one step per task for deterministic reopen targeting.

**Done when:**
- Provides intermediate task numbering for stale-boundary ordering.

**Files:**
- Modify: `tests/contracts_execution_runtime_boundaries.rs`

- [ ] **Step 1: Execute task 5 baseline step**

## Task 6: FS-15 task 6

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Represents the later stale overlay target that must not outrank Task 2.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep one step per task for deterministic reopen targeting.

**Done when:**
- Represents the later stale overlay target that must not outrank Task 2.

**Files:**
- Modify: `tests/contracts_execution_runtime_boundaries.rs`

- [ ] **Step 1: Execute task 6 baseline step**
"#;

fn assert_task_closure_required_inputs(surface: &Value, task: u32) {
    assert!(surface["recommended_command"].is_null(), "{surface}");
    assert!(
        surface.get("recommended_public_command_argv").is_none(),
        "{surface}"
    );
    let task_target = surface["recording_context"]["task_number"]
        .as_u64()
        .or_else(|| surface["blocking_task"].as_u64());
    assert_eq!(task_target, Some(u64::from(task)), "{surface}");
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
        "{surface}"
    );
}

fn run_plan_execution_json(repo: &Path, state: &Path, args: &[&str], context: &str) -> Value {
    let mut command_args = vec!["plan", "execution"];
    command_args.extend_from_slice(args);
    let output = public_featureforge_cli::run_featureforge_real_cli(
        Some(repo),
        Some(state),
        None,
        &[],
        &command_args,
        context,
    );
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn record_task_closure_with_fixture_inputs(
    repo: &Path,
    state: &Path,
    task: u32,
    context: &str,
) -> Value {
    let review_summary_path = repo.join(format!("boundary-task-{task}-review-summary.md"));
    let verification_summary_path =
        repo.join(format!("boundary-task-{task}-verification-summary.md"));
    let task_arg = task.to_string();
    fs::write(
        &review_summary_path,
        format!("Review summary supplied as concrete input for {context}.\n"),
    )
    .expect("task closure review summary fixture should be writable");
    fs::write(
        &verification_summary_path,
        format!("Verification summary supplied as concrete input for {context}.\n"),
    )
    .expect("task closure verification summary fixture should be writable");
    run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            PLAN_REL,
            "--task",
            &task_arg,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should stay utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should stay utf-8"),
        ],
        context,
    )
}

fn materialize_state_dir_projections(
    repo: &Path,
    state: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let materialized = run_plan_execution_json(
        repo,
        state,
        &["materialize-projections", "--plan", plan],
        context,
    );
    assert_eq!(materialized["action"], Value::from("materialized"));
    assert_eq!(materialized["runtime_truth_changed"], Value::Bool(false));
    materialized
}

fn run_workflow_operator_json(
    repo: &Path,
    state: &Path,
    plan: &str,
    external_review_result_ready: bool,
    context: &str,
) -> Value {
    let mut command_args = vec!["workflow", "operator", "--plan", plan];
    if external_review_result_ready {
        command_args.push("--external-review-result-ready");
    }
    command_args.push("--json");
    let output = public_featureforge_cli::run_featureforge_real_cli(
        Some(repo),
        Some(state),
        None,
        &[],
        &command_args,
        context,
    );
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn run_featureforge_output(
    repo: &Path,
    state: &Path,
    args: &[&str],
    _real_cli: bool,
    context: &str,
) -> Output {
    public_featureforge_cli::run_featureforge_real_cli(
        Some(repo),
        Some(state),
        None,
        &[],
        args,
        context,
    )
}

fn run_featureforge_json(
    repo: &Path,
    state: &Path,
    args: &[&str],
    real_cli: bool,
    context: &str,
) -> Value {
    let output = run_featureforge_output(repo, state, args, real_cli, context);
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn run_recommended_plan_execution_command(
    repo: &Path,
    state: &Path,
    recommended_command: &str,
    real_cli: bool,
    context: &str,
) -> Value {
    let command_parts = recommended_command.split_whitespace().collect::<Vec<_>>();
    assert!(
        command_parts.len() >= 4,
        "{context} should expose a full featureforge plan-execution command, got {recommended_command}"
    );
    assert_eq!(
        command_parts[0], "featureforge",
        "{context} recommended command must start with featureforge, got {recommended_command}"
    );
    assert_eq!(
        command_parts[1], "plan",
        "{context} recommended command must route through `plan execution`, got {recommended_command}"
    );
    assert_eq!(
        command_parts[2], "execution",
        "{context} recommended command must route through `plan execution`, got {recommended_command}"
    );
    assert!(
        command_parts.iter().all(|part| !["<", ">", "|", "[", "]"]
            .iter()
            .any(|token| part.contains(token))),
        "{context} recommended command must be executable as emitted, got {recommended_command}"
    );
    let command_args = command_parts[1..]
        .iter()
        .map(|part| (*part).to_owned())
        .collect::<Vec<_>>();
    let command_args_refs = command_args.iter().map(String::as_str).collect::<Vec<_>>();
    run_featureforge_json(repo, state, &command_args_refs, real_cli, context)
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

fn authoritative_harness_state_path(repo: &Path, state: &Path) -> PathBuf {
    let runtime = execution_runtime(repo, state);
    harness_state_path(state, &runtime.repo_slug, &runtime.branch_name)
}

fn authoritative_harness_state(repo: &Path, state: &Path) -> Value {
    let state_path = authoritative_harness_state_path(repo, state);
    featureforge::execution::event_log::load_reduced_authoritative_state_for_tests(&state_path)
        .unwrap_or_else(|error| {
            panic!(
                "event-authoritative boundary harness state should reduce for {}: {}",
                state_path.display(),
                error.message
            )
        })
        .unwrap_or_else(|| {
            let source = fs::read_to_string(&state_path)
                .expect("authoritative harness state should be readable");
            serde_json::from_str(&source)
                .expect("authoritative harness state should remain valid json")
        })
}

fn update_authoritative_harness_state(repo: &Path, state: &Path, updates: &[(&str, Value)]) {
    let state_path = authoritative_harness_state_path(repo, state);
    let mut payload = authoritative_harness_state(repo, state);
    let object = payload
        .as_object_mut()
        .expect("authoritative harness state should remain a json object");
    for (key, value) in updates {
        object.insert((*key).to_owned(), value.clone());
    }
    fs::write(
        &state_path,
        serde_json::to_string(&payload).expect("authoritative harness state should serialize"),
    )
    .expect("authoritative harness state should remain writable");
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(&state_path, &payload)
        .expect("boundary harness state fixture should sync typed event authority");
}

fn prepare_preflight_acceptance_workspace(repo: &Path, branch_name: &str) {
    let mut checkout = Command::new("git");
    checkout
        .args(["checkout", "-B", branch_name])
        .current_dir(repo);
    process_support::run_checked(checkout, "git checkout boundary fixture branch");
}

fn current_branch_name(repo: &Path) -> String {
    discover_slug_identity(repo).branch_name
}

fn expected_release_base_branch(repo: &Path) -> String {
    let current_branch = current_branch_name(repo);
    resolve_release_base_branch(&repo.join(".git"), &current_branch).unwrap_or(current_branch)
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

fn branch_contract_identity(repo: &Path, state_dir: &Path, plan_rel: &str) -> String {
    let runtime = execution_runtime(repo, state_dir);
    let context = load_execution_context(&runtime, Path::new(plan_rel))
        .expect("runtime boundary semantic branch identity fixture should load execution context");
    branch_definition_identity_for_context(&context)
}

fn task_contract_identity(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    task_number: u32,
) -> String {
    let runtime = execution_runtime(repo, state_dir);
    let context = load_execution_context(&runtime, Path::new(plan_rel))
        .expect("runtime boundary semantic task identity fixture should load execution context");
    task_definition_identity_for_task(&context, task_number)
        .expect("runtime boundary semantic task identity fixture should compute")
        .expect("runtime boundary semantic task identity fixture should exist")
}

fn seed_branch_chain_truth_for_runtime_owned_churn_fixture(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
) {
    let branch_name = current_branch_name(repo);
    let base_branch = expected_release_base_branch(repo);
    let slug = repo_slug(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let branch_contract_identity = branch_contract_identity(repo, state, plan_rel);
    let execution_run_id = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 status should expose execution_run_id for branch-chain fixture seeding",
    )["execution_run_id"]
        .as_str()
        .expect("FS-20 status should expose execution_run_id")
        .to_owned();
    let task_contract_identity = task_contract_identity(repo, state, plan_rel, 1);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("dependency_index_state", Value::from("fresh")),
            (
                "current_task_closure_records",
                json!({
                    "task-1": {
                        "dispatch_id": "fixture-task-dispatch",
                        "closure_record_id": "task-1-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "execution_run_id": execution_run_id.clone(),
                        "reviewed_state_id": reviewed_state_id.clone(),
                        "contract_identity": task_contract_identity.clone(),
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"contracts fs20 task closure review fixture"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"contracts fs20 task closure verification fixture"),
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
                        "source_plan_revision": 1,
                        "execution_run_id": execution_run_id,
                        "reviewed_state_id": reviewed_state_id.clone(),
                        "contract_identity": task_contract_identity,
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"contracts fs20 task closure review fixture"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"contracts fs20 task closure verification fixture"),
                        "closure_status": "current"
                    }
                }),
            ),
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
                        "source_plan_revision": 1,
                        "repo_slug": slug,
                        "branch_name": branch_name,
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
            ("release_docs_state", Value::from("fresh")),
            ("final_review_state", Value::from("fresh")),
            ("browser_qa_state", Value::from("fresh")),
            ("current_release_readiness_result", Value::from("ready")),
            (
                "current_final_review_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_final_review_result", Value::from("pass")),
            (
                "current_qa_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_qa_result", Value::from("pass")),
            (
                "finish_review_gate_pass_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
        ],
    );
}

fn assert_routing_parity_with_operator_json(routing: &ExecutionRoutingState, operator: &Value) {
    assert_eq!(operator["phase"], Value::from(routing.phase.clone()));
    assert_eq!(
        operator["phase_detail"],
        Value::from(routing.phase_detail.clone())
    );
    assert_eq!(
        operator["review_state_status"],
        Value::from(routing.review_state_status.clone())
    );
    assert_eq!(
        operator.get("qa_requirement").and_then(Value::as_str),
        routing.qa_requirement.as_deref()
    );
    assert!(
        operator.get("follow_up_override").is_none(),
        "operator output must not expose legacy follow_up_override"
    );
    assert_eq!(
        operator
            .get("finish_review_gate_pass_branch_closure_id")
            .and_then(Value::as_str),
        routing.finish_review_gate_pass_branch_closure_id.as_deref()
    );
    assert_eq!(
        operator["next_action"],
        Value::from(routing.next_action.clone())
    );
    assert_eq!(
        operator.get("recommended_command").and_then(Value::as_str),
        routing.recommended_command.as_deref()
    );
    assert_eq!(
        operator.get("blocking_scope").and_then(Value::as_str),
        routing.blocking_scope.as_deref()
    );
    assert_eq!(
        operator
            .get("blocking_task")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        routing.blocking_task
    );
    assert_eq!(
        operator.get("external_wait_state").and_then(Value::as_str),
        routing.external_wait_state.as_deref()
    );
    let operator_blocking_reason_codes = operator
        .get("blocking_reason_codes")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert_eq!(
        operator_blocking_reason_codes,
        routing.blocking_reason_codes
    );
    assert_eq!(
        routing.recording_context.as_ref().map(|context| (
            context.task_number,
            context.dispatch_id.as_deref(),
            context.branch_closure_id.as_deref(),
        )),
        operator.get("recording_context").map(|context| (
            context
                .get("task_number")
                .and_then(Value::as_u64)
                .map(|value| value as u32),
            context.get("dispatch_id").and_then(Value::as_str),
            context.get("branch_closure_id").and_then(Value::as_str),
        ))
    );
    assert_eq!(
        routing.execution_command_context.as_ref().map(|context| (
            context.command_kind.as_str(),
            context.task_number,
            context.step_id,
        )),
        operator.get("execution_command_context").map(|context| (
            context
                .get("command_kind")
                .and_then(Value::as_str)
                .expect("operator execution command context should expose command_kind"),
            context
                .get("task_number")
                .and_then(Value::as_u64)
                .map(|value| value as u32),
            context
                .get("step_id")
                .and_then(Value::as_u64)
                .map(|value| value as u32),
        ))
    );
}

fn setup_task_boundary_blocked_case(repo: &Path, state: &Path) {
    install_full_contract_ready_artifacts(repo);
    fs::write(repo.join(PLAN_REL), TASK_BOUNDARY_BLOCKED_PLAN_SOURCE)
        .expect("task-boundary blocked plan fixture should write");
    prepare_preflight_acceptance_workspace(repo, "boundary-task-closure-recording-ready");

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status before task-boundary fixture execution",
    );
    let begin_task1_step1 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            PLAN_REL,
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
        "begin task 1 step 1 for task-boundary fixture",
    );
    let complete_task1_step1 = run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 1 for boundary task-boundary fixture.",
            "--manual-verify-summary",
            "Verified by boundary task-boundary fixture setup.",
            "--file",
            "tests/contracts_execution_runtime_boundaries.rs",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 1 for task-boundary fixture",
    );
    let begin_task1_step2 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            PLAN_REL,
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
        "begin task 1 step 2 for task-boundary fixture",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "2",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 2 for boundary task-boundary fixture.",
            "--manual-verify-summary",
            "Verified by boundary task-boundary fixture setup.",
            "--file",
            "tests/contracts_execution_runtime_boundaries.rs",
            "--expect-execution-fingerprint",
            begin_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 2 for task-boundary fixture",
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn state_tree_digest(root: &Path) -> String {
    fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files(&path, files);
            } else {
                files.push(path);
            }
        }
    }

    let mut files = Vec::new();
    collect_files(root, &mut files);
    files.sort();
    let mut digest = Sha256::new();
    for file in files {
        let relative = file.strip_prefix(root).unwrap_or(file.as_path());
        digest.update(relative.to_string_lossy().as_bytes());
        digest.update([0]);
        let bytes = fs::read(&file).unwrap_or_default();
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    format!("{:x}", digest.finalize())
}

#[test]
fn internal_only_compatibility_execution_query_recording_ready_states_surface_required_recording_context_ids()
 {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-recording-context");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);
    let dispatch = internal_runtime_direct::internal_only_runtime_review_dispatch_authority_json(
        repo,
        state,
        &featureforge::execution::internal_args::RecordReviewDispatchArgs {
            plan: PLAN_REL.into(),
            scope: featureforge::execution::internal_args::ReviewDispatchScopeArg::Task,
            task: Some(1),
        },
    )
    .expect(concat!(
        "internal record",
        "-review-dispatch helper should succeed for task-boundary fixture"
    ));
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let runtime = execution_runtime(repo, state);
    let plan = PathBuf::from(PLAN_REL);
    let routing = query_workflow_routing_state_for_runtime(&runtime, Some(&plan), true)
        .expect("routing query should succeed for task_closure_recording_ready fixture");
    let operator = run_workflow_operator_json(
        repo,
        state,
        PLAN_REL,
        true,
        "workflow operator json for task_closure_recording_ready fixture",
    );
    assert_eq!(routing.phase_detail, "task_closure_recording_ready");
    let recording_context = routing
        .recording_context
        .as_ref()
        .expect("task_closure_recording_ready should expose recording_context");
    assert_eq!(recording_context.task_number, Some(1));
    assert!(
        recording_context
            .dispatch_id
            .as_deref()
            .is_none_or(|dispatch_id| !dispatch_id.trim().is_empty()),
        "task_closure_recording_ready dispatch_id should be omitted or non-empty",
    );
    assert_routing_parity_with_operator_json(&routing, &operator);

    let public_route_source =
        fs::read_to_string(repo_root().join("src/execution/public_route_selection.rs"))
            .expect("execution public route selection source should be readable");

    assert!(
        public_route_source.contains("phase::DETAIL_TASK_CLOSURE_RECORDING_READY")
            && public_route_source.contains("task_number: Some(task_number)")
            && public_route_source.contains("dispatch_id: task_review_dispatch_id.clone()"),
        "task_closure_recording_ready should expose task_number and may surface dispatch_id in recording_context",
    );
    assert!(
        public_route_source.contains("phase::DETAIL_RELEASE_READINESS_RECORDING_READY")
            && public_route_source.contains("phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED")
            && public_route_source
                .contains("branch_closure_id: Some(branch_closure_id.to_owned())"),
        "release-readiness recording-ready states should expose branch_closure_id recording_context ids",
    );
    assert!(
        public_route_source.contains("phase::DETAIL_FINAL_REVIEW_RECORDING_READY")
            && public_route_source.contains("dispatch_id: final_review_dispatch_id.clone()")
            && public_route_source
                .contains("branch_closure_id: Some(branch_closure_id.to_owned())"),
        "final_review_recording_ready should expose branch_closure_id and may surface dispatch_id in the routing constructor",
    );
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs03_internal_dispatch_target_acceptance_and_mismatch_preserve_mutation_contract()
 {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-runtime-remediation-fs03-internal");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);

    let baseline = state_tree_digest(state);
    let accepted = internal_runtime_direct::internal_only_runtime_review_dispatch_authority_json(
        repo,
        state,
        &featureforge::execution::internal_args::RecordReviewDispatchArgs {
            plan: PLAN_REL.into(),
            scope: featureforge::execution::internal_args::ReviewDispatchScopeArg::Task,
            task: Some(1),
        },
    )
    .expect("FS-03 accepted task-boundary dispatch target should remain allowed");
    assert_eq!(
        accepted["allowed"],
        Value::Bool(true),
        "FS-03 accepted path should remain allowed"
    );
    let digest_after_accept = state_tree_digest(state);
    assert_ne!(
        digest_after_accept, baseline,
        "FS-03 accepted path should record dispatch lineage"
    );

    let rejected_json: Value = serde_json::from_str(
        &internal_runtime_direct::internal_only_runtime_review_dispatch_authority_json(
            repo,
            state,
            &featureforge::execution::internal_args::RecordReviewDispatchArgs {
                plan: PLAN_REL.into(),
                scope: featureforge::execution::internal_args::ReviewDispatchScopeArg::Task,
                task: Some(2),
            },
        )
        .expect_err("FS-03 mismatched task target should fail before mutation"),
    )
    .expect("FS-03 internal mismatch failure should serialize as json");
    assert_eq!(
        rejected_json["error_class"],
        Value::from("InvalidCommandInput"),
        "FS-03 mismatched task target should fail with InvalidCommandInput"
    );
    assert_eq!(
        state_tree_digest(state),
        digest_after_accept,
        "FS-03 mismatched task target must not mutate runtime state after failing"
    );
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs05_internal_unsupported_field_fails_before_mutation_on_dispatch_contract()
 {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-runtime-remediation-fs05");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);

    let baseline = state_tree_digest(state);
    let failure: Value = serde_json::from_str(
        &internal_runtime_direct::internal_only_runtime_review_dispatch_authority_json(
            repo,
            state,
            &featureforge::execution::internal_args::RecordReviewDispatchArgs {
                plan: PLAN_REL.into(),
                scope: featureforge::execution::internal_args::ReviewDispatchScopeArg::FinalReview,
                task: Some(1),
            },
        )
        .expect_err("FS-05 unsupported final-review task field should fail before mutation"),
    )
    .expect("FS-05 internal failure should serialize as json");
    assert_eq!(
        failure["error_class"],
        Value::from("InvalidCommandInput"),
        "FS-05 path should reject unsupported fields before mutation"
    );
    assert_eq!(
        state_tree_digest(state),
        baseline,
        "FS-05 path must not mutate runtime state files on unsupported fields"
    );
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs15_compiled_cli_never_prefers_later_stale_task()
 {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-runtime-remediation-fs15");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    fs::write(repo.join(PLAN_REL), TASK_BOUNDARY_FS15_PLAN_SOURCE)
        .expect("FS-15 stale-boundary fixture plan should be writable");
    prepare_preflight_acceptance_workspace(repo, "boundary-runtime-remediation-fs15");

    let preflight = internal_runtime_direct::internal_only_runtime_preflight_gate_json(
        repo,
        state,
        &featureforge::cli::plan_execution::StatusArgs {
            plan: PLAN_REL.into(),
            external_review_result_ready: false,
        },
    )
    .expect(concat!(
        "internal pre",
        "flight helper should succeed for FS-15 stale-boundary fixture"
    ));
    assert_eq!(
        preflight["allowed"],
        Value::Bool(true),
        "FS-15 {} should allow stale-boundary fixture execution",
        concat!("pre", "flight"),
    );
    let status_before_task1 = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "FS-15 status before bootstrap task 1 begin",
    );
    let begin_task1 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_task1["execution_fingerprint"]
                .as_str()
                .expect("FS-15 status should expose execution fingerprint before bootstrap task 1 begin"),
        ],
        "FS-15 bootstrap task 1 begin",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-15 bootstrap task 1 complete",
            "--manual-verify-summary",
            "FS-15 bootstrap task 1 complete summary",
            "--file",
            "tests/contracts_execution_runtime_boundaries.rs",
            "--expect-execution-fingerprint",
            begin_task1["execution_fingerprint"]
                .as_str()
                .expect("FS-15 begin task 1 should expose execution fingerprint before complete"),
        ],
        "FS-15 bootstrap task 1 complete",
    );
    let repair_task1 = run_plan_execution_json(
        repo,
        state,
        &[
            "repair-review-state",
            "--plan",
            PLAN_REL,
            "--external-review-result-ready",
        ],
        "FS-15 bootstrap task 1 repair-review-state",
    );
    assert_eq!(
        repair_task1["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-15 bootstrap task 1 repair-review-state should surface the public closure-recording repair bridge"
    );
    assert_task_closure_required_inputs(&repair_task1, 1);
    let close_task1 = record_task_closure_with_fixture_inputs(
        repo,
        state,
        1,
        "FS-15 bootstrap task 1 concrete task-closure follow-up",
    );
    assert_eq!(
        close_task1["action"],
        Value::from("recorded"),
        "FS-15 bootstrap task 1 repair follow-up should record a task closure"
    );
    let status_after_close_task1 = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "FS-15 status after bootstrap task 1 close-current-task",
    );
    let begin_task2 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            PLAN_REL,
            "--task",
            "2",
            "--step",
            "1",
            "--expect-execution-fingerprint",
            status_after_close_task1["execution_fingerprint"]
                .as_str()
                .expect(
                    "FS-15 status after close-current-task should expose execution fingerprint before task 2 begin",
                ),
        ],
        "FS-15 bootstrap task 2 begin",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            PLAN_REL,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-15 bootstrap task 2 complete",
            "--manual-verify-summary",
            "FS-15 bootstrap task 2 complete summary",
            "--file",
            "tests/contracts_execution_runtime_boundaries.rs",
            "--expect-execution-fingerprint",
            begin_task2["execution_fingerprint"]
                .as_str()
                .expect("FS-15 begin task 2 should expose execution fingerprint before complete"),
        ],
        "FS-15 bootstrap task 2 complete",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "task_closure_record_history",
                serde_json::json!({
                    "task-1-current": {
                        "closure_record_id": "task-1-current",
                        "task": 1,
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "record_sequence": 5,
                        "closure_status": "current",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-2-stale": {
                        "closure_record_id": "task-2-stale",
                        "task": 2,
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "record_sequence": 10,
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-6-stale": {
                        "closure_record_id": "task-6-stale",
                        "task": 6,
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "record_sequence": 20,
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    }
                }),
            ),
            (
                "current_open_step_state",
                serde_json::json!({
                    "task": 6,
                    "step": 1,
                    "note_state": "Interrupted",
                    "note_summary": "FS-15 later interrupted overlay",
                    "source_plan_path": PLAN_REL,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 30
                }),
            ),
            ("resume_task", Value::from(6_u64)),
            ("resume_step", Value::from(1_u64)),
        ],
    );

    let operator_direct = run_featureforge_json(
        repo,
        state,
        &["workflow", "operator", "--plan", PLAN_REL, "--json"],
        false,
        "FS-15 direct operator stale-boundary targeting",
    );
    let operator_real = run_featureforge_json(
        repo,
        state,
        &["workflow", "operator", "--plan", PLAN_REL, "--json"],
        true,
        "FS-15 compiled-cli operator stale-boundary targeting",
    );
    assert_eq!(
        operator_direct, operator_real,
        "FS-15 direct and compiled-cli operator outputs must stay semantically aligned",
    );
    assert_eq!(
        operator_real["blocking_task"],
        Value::from(2_u64),
        "FS-15 should always target the earliest unresolved stale boundary (Task 2)"
    );
    if let Some(recommended_command) = operator_real["recommended_command"].as_str() {
        assert!(
            recommended_command.contains("--task 2"),
            "FS-15 compiled-cli operator should route to Task 2 while it is the earliest stale boundary, got {recommended_command}",
        );
        assert!(
            !recommended_command.contains("--task 6"),
            "FS-15 compiled-cli operator must not route to Task 6 while Task 2 is stale, got {recommended_command}",
        );
        assert_eq!(
            operator_real["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-15 executable command context should target Task 2"
        );
        if operator_real["execution_command_context"]["command_kind"].as_str() == Some("reopen") {
            assert_eq!(
                operator_real["execution_command_context"]["step_id"],
                Value::from(1_u64),
                "FS-15 reopen command context should target Step 1"
            );
            assert!(
                recommended_command.contains("--step 1"),
                "FS-15 reopen routing should keep Step 1 targeted, got {recommended_command}"
            );
        }
        let operator_follow_up = run_recommended_plan_execution_command(
            repo,
            state,
            recommended_command,
            true,
            "FS-15 compiled-cli operator recommended command follow-up parity",
        );
        assert_follow_up_blocker_parity_with_operator(
            &operator_real,
            &operator_follow_up,
            "FS-15 command-follow parity",
        );
    } else {
        assert_eq!(
            operator_real["execution_command_context"]["command_kind"],
            Value::from("close_current_task"),
            "FS-15 missing-input route should stay in closure-recording lane for Task 2"
        );
        assert_task_closure_required_inputs(&operator_real, 2);
    }
}

#[test]
fn internal_only_compatibility_fs19_compiled_cli_ignores_superseded_stale_history_when_selecting_blocking_task()
 {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-runtime-remediation-fs19");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    fs::write(repo.join(PLAN_REL), TASK_BOUNDARY_FS15_PLAN_SOURCE)
        .expect("FS-19 stale-history fixture plan should be writable");
    prepare_preflight_acceptance_workspace(repo, "boundary-runtime-remediation-fs19");

    let preflight = internal_runtime_direct::internal_only_runtime_preflight_gate_json(
        repo,
        state,
        &featureforge::cli::plan_execution::StatusArgs {
            plan: PLAN_REL.into(),
            external_review_result_ready: false,
        },
    )
    .expect(concat!(
        "internal pre",
        "flight helper should succeed for FS-19 stale-history fixture"
    ));
    assert_eq!(
        preflight["allowed"],
        Value::Bool(true),
        "FS-19 {} should allow fixture execution",
        concat!("pre", "flight"),
    );
    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "FS-19 status before bootstrap begin",
    );
    let begin = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-19 status should expose execution fingerprint before begin"),
        ],
        "FS-19 bootstrap task 1 begin",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-19 bootstrap complete",
            "--manual-verify-summary",
            "FS-19 bootstrap complete summary",
            "--file",
            "tests/contracts_execution_runtime_boundaries.rs",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"]
                .as_str()
                .expect("FS-19 begin should expose execution fingerprint before complete"),
        ],
        "FS-19 bootstrap task 1 complete",
    );

    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "task_closure_record_history",
                serde_json::json!({
                    "task-1-stale": {
                        "closure_record_id": "task-1-stale",
                        "task": 1,
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "record_sequence": 8,
                        "record_status": "stale_unreviewed",
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-1-current": {
                        "closure_record_id": "task-1-current",
                        "task": 1,
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "record_sequence": 24,
                        "record_status": "current",
                        "closure_status": "current",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-2-stale": {
                        "closure_record_id": "task-2-stale",
                        "task": 2,
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "record_sequence": 10,
                        "record_status": "stale_unreviewed",
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-6-stale": {
                        "closure_record_id": "task-6-stale",
                        "task": 6,
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "record_sequence": 20,
                        "record_status": "stale_unreviewed",
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    }
                }),
            ),
            (
                "superseded_task_closure_ids",
                serde_json::json!(["task-1-stale"]),
            ),
            (
                "current_open_step_state",
                serde_json::json!({
                    "task": 6,
                    "step": 1,
                    "note_state": "Interrupted",
                    "note_summary": "FS-19 stale task 1 history should be ignored once superseded",
                    "source_plan_path": PLAN_REL,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 30
                }),
            ),
            ("resume_task", Value::from(6_u64)),
            ("resume_step", Value::from(1_u64)),
        ],
    );

    let operator_real = run_featureforge_json(
        repo,
        state,
        &["workflow", "operator", "--plan", PLAN_REL, "--json"],
        true,
        "FS-19 compiled-cli stale-history routing",
    );
    assert_eq!(
        operator_real["execution_command_context"]["task_number"],
        Value::from(2_u64),
        "FS-19 compiled-cli should target Task 2 after superseding stale task 1 history",
    );
    let recommended_command = operator_real["recommended_command"]
        .as_str()
        .expect("FS-19 compiled-cli operator should expose recommended command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-19 compiled-cli operator should route to Task 2, got {recommended_command}",
    );
    assert!(
        !recommended_command.contains("--task 1"),
        "FS-19 compiled-cli operator must not route to superseded stale task 1 history, got {recommended_command}",
    );
}

#[test]
fn internal_only_compatibility_fs20_runtime_owned_control_plane_churn_does_not_flip_query_or_operator_to_branch_scope()
 {
    let (repo_dir, state_dir) =
        init_repo("contracts-boundary-runtime-remediation-fs20-control-plane-branch-scope");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);
    seed_branch_chain_truth_for_runtime_owned_churn_fixture(repo, state, PLAN_REL);

    let baseline_status = run_featureforge_json(
        repo,
        state,
        &["plan", "execution", "status", "--plan", PLAN_REL],
        true,
        "FS-20 baseline status before runtime-owned control-plane churn",
    );
    let baseline_branch_closure_id = baseline_status["current_branch_closure_id"]
        .as_str()
        .expect("FS-20 baseline status should expose current_branch_closure_id")
        .to_owned();
    let baseline_release_state = baseline_status["current_release_readiness_state"].clone();
    let baseline_final_state = baseline_status["current_final_review_state"].clone();
    let baseline_qa_state = baseline_status["current_qa_state"].clone();
    let baseline_operator = run_featureforge_json(
        repo,
        state,
        &["workflow", "operator", "--plan", PLAN_REL, "--json"],
        true,
        "FS-20 baseline operator before runtime-owned control-plane churn",
    );

    let evidence_rel = baseline_status["evidence_path"]
        .as_str()
        .expect("FS-20 baseline status should expose evidence_path");
    materialize_state_dir_projections(
        repo,
        state,
        PLAN_REL,
        "FS-20 materialize state-dir projections before runtime-owned control-plane churn",
    );
    let plan_path = repo.join(PLAN_REL);
    let plan_source = fs::read_to_string(&plan_path).expect("FS-20 plan should be readable");
    fs::write(
        &plan_path,
        format!("{plan_source}\n<!-- fs20 contracts runtime-owned plan mutation -->\n"),
    )
    .expect("FS-20 should mutate only the approved plan path");
    let evidence_source =
        projection_support::read_state_dir_projection(&baseline_status, evidence_rel);
    projection_support::write_state_dir_projection(
        &baseline_status,
        evidence_rel,
        &format!("{evidence_source}\n<!-- fs20 contracts runtime-owned evidence mutation -->\n"),
    );

    let status_after_churn = run_featureforge_json(
        repo,
        state,
        &["plan", "execution", "status", "--plan", PLAN_REL],
        true,
        "FS-20 status after runtime-owned control-plane churn",
    );
    let operator_after_churn = run_featureforge_json(
        repo,
        state,
        &["workflow", "operator", "--plan", PLAN_REL, "--json"],
        true,
        "FS-20 operator after runtime-owned control-plane churn",
    );
    let runtime = execution_runtime(repo, state);
    let routing_after_churn =
        query_workflow_routing_state_for_runtime(&runtime, Some(&PathBuf::from(PLAN_REL)), false)
            .expect("FS-20 routing query should succeed after runtime-owned control-plane churn");

    assert_eq!(
        status_after_churn["current_branch_closure_id"],
        Value::from(baseline_branch_closure_id),
        "FS-20 status should preserve current_branch_closure_id after runtime-owned control-plane churn"
    );
    assert_eq!(
        status_after_churn["current_release_readiness_state"], baseline_release_state,
        "FS-20 status should preserve release-readiness state after runtime-owned control-plane churn"
    );
    assert_eq!(
        status_after_churn["current_final_review_state"], baseline_final_state,
        "FS-20 status should preserve final-review state after runtime-owned control-plane churn"
    );
    assert_eq!(
        status_after_churn["current_qa_state"], baseline_qa_state,
        "FS-20 status should preserve QA state after runtime-owned control-plane churn"
    );

    for (label, payload) in [
        ("baseline status", &baseline_status),
        ("baseline operator", &baseline_operator),
        ("status after churn", &status_after_churn),
        ("operator after churn", &operator_after_churn),
    ] {
        assert_ne!(
            payload["phase_detail"],
            Value::from("branch_closure_recording_required_for_release_readiness"),
            "FS-20 {label} must not route to branch_closure_recording_required_for_release_readiness from control-plane-only churn"
        );
        assert!(
            !payload["recommended_command"]
                .as_str()
                .is_some_and(|command| command.contains(concat!("record", "-branch-closure"))),
            "FS-20 {} must not recommend {} from control-plane-only churn",
            label,
            concat!("record", "-branch-closure")
        );
    }

    assert_ne!(
        routing_after_churn.phase_detail, "branch_closure_recording_required_for_release_readiness",
        "FS-20 routing query must not reroute to branch_closure_recording_required_for_release_readiness from control-plane-only churn"
    );
    assert!(
        !routing_after_churn
            .recommended_command
            .as_deref()
            .is_some_and(|command| command.contains(concat!("record", "-branch-closure"))),
        "FS-20 routing query must not recommend {} from control-plane-only churn",
        concat!("record", "-branch-closure")
    );
    assert_routing_parity_with_operator_json(&routing_after_churn, &operator_after_churn);
}
