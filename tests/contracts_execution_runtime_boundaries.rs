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
#[cfg(unix)]
use std::os::unix::fs::symlink;
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
use tempfile::TempDir;
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

fn parse_json_output(output: &Output, context: &str) -> Value {
    serde_json::from_slice(&output.stdout)
        .or_else(|_| serde_json::from_slice(&output.stderr))
        .unwrap_or_else(|error| {
            panic!(
                "{context} should emit json output: {error}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
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
    let command_args = if command_parts
        .get(3)
        .is_some_and(|command| *command == "close-current-task")
        && (recommended_command.contains("pass|fail") || recommended_command.contains("<path>"))
    {
        let plan = command_parts
            .windows(2)
            .find(|window| window[0] == "--plan")
            .map(|window| window[1])
            .unwrap_or(PLAN_REL);
        let task = command_parts
            .windows(2)
            .find(|window| window[0] == "--task")
            .map(|window| window[1])
            .unwrap_or("1");
        let summary_path = repo.join("boundary-close-current-task-review-summary.md");
        let verification_summary_path =
            repo.join("boundary-close-current-task-verification-summary.md");
        fs::write(
            &summary_path,
            format!("Close-current-task command generated from shared template for {context}.\n"),
        )
        .expect("close-current-task template follow-up should write deterministic review summary");
        fs::write(
            &verification_summary_path,
            format!("Verification summary generated from shared template for {context}.\n"),
        )
        .expect(
            "close-current-task template follow-up should write deterministic verification summary",
        );
        vec![
            String::from("plan"),
            String::from("execution"),
            String::from("close-current-task"),
            String::from("--plan"),
            plan.to_owned(),
            String::from("--task"),
            task.to_owned(),
            String::from("--review-result"),
            String::from("pass"),
            String::from("--review-summary-file"),
            summary_path
                .to_str()
                .expect("summary path should stay utf-8")
                .to_owned(),
            String::from("--verification-result"),
            String::from("pass"),
            String::from("--verification-summary-file"),
            verification_summary_path
                .to_str()
                .expect("verification summary path should stay utf-8")
                .to_owned(),
        ]
    } else {
        command_parts[1..]
            .iter()
            .map(|part| (*part).to_owned())
            .collect::<Vec<_>>()
    };
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

fn setup_execution_in_progress(repo: &Path, state: &Path) {
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "boundary-operator-query-active");
    fs::write(repo.join(PLAN_REL), TASK_BOUNDARY_BLOCKED_PLAN_SOURCE)
        .expect("boundary active-context plan should be writable");
    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status before boundary active-context begin",
    );
    run_plan_execution_json(
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
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution_fingerprint"),
        ],
        "begin for boundary active-context fixture",
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

fn setup_fs08_stale_blocker_fixture(repo: &Path, state: &Path) {
    setup_task_boundary_blocked_case(repo, state);
    update_authoritative_harness_state(
        repo,
        state,
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
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn execution_command_source_files() -> Vec<(String, String)> {
    fn collect_rust_source_files(dir: &Path, files: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", dir.display()))
        {
            let entry = entry.expect("execution command entry should be readable");
            let path = entry.path();
            if path.is_dir() {
                collect_rust_source_files(&path, files);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }

    let mut paths = Vec::new();
    collect_rust_source_files(&repo_root().join("src/execution/commands"), &mut paths);
    paths.sort();

    paths
        .into_iter()
        .filter(|path| {
            path.strip_prefix(repo_root())
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
                != "src/execution/commands/common/unit_tests.rs"
        })
        .map(|path| {
            let rel = path
                .strip_prefix(repo_root())
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let source =
                fs::read_to_string(&path).expect("execution command source should be readable");
            (rel, source)
        })
        .collect()
}

fn execution_command_dependency_paths() -> Vec<String> {
    let mut paths = execution_command_source_files()
        .into_iter()
        .flat_map(|(rel, source)| rust_source_scan::normalized_dependency_paths(&rel, &source))
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn execution_command_call_paths() -> Vec<String> {
    let mut paths = execution_command_source_files()
        .into_iter()
        .flat_map(|(rel, source)| rust_source_scan::normalized_call_paths(&rel, &source, &[]))
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn execution_command_function_names() -> Vec<String> {
    let mut names = execution_command_source_files()
        .into_iter()
        .flat_map(|(rel, source)| rust_source_scan::function_names(&rel, &source))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn paths_contain_dependency(paths: &[String], dependency: &str) -> bool {
    paths
        .iter()
        .any(|path| path == dependency || path.starts_with(&format!("{dependency}::")))
}

fn paths_contain_leaf(paths: &[String], leaf: &str) -> bool {
    paths
        .iter()
        .any(|path| path.rsplit("::").next() == Some(leaf))
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

fn assert_trust_boundary_failure(output: &Output, context: &str, message_fragment: &str) {
    assert!(
        !output.status.success(),
        "{context} must fail closed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let failure = parse_json_output(output, context);
    let failure_class = failure
        .get("error_class")
        .or_else(|| failure.get("failure_class"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        failure_class == "MalformedExecutionState" || failure_class == "InstructionParseFailed",
        "{context} should classify trust-boundary failures as MalformedExecutionState or wrapped InstructionParseFailed, got {failure_class}"
    );
    let message = failure
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        message.contains(message_fragment),
        "{context} should surface trust-boundary details containing `{message_fragment}`, got {message}"
    );
}

fn assert_event_or_state_loader_rejection_across_public_surfaces(
    repo: &Path,
    state: &Path,
    scenario: &str,
    message_fragment: &str,
) {
    let status_command = ["plan", "execution", "status", "--plan", PLAN_REL];
    let repair_command = [
        "plan",
        "execution",
        "repair-review-state",
        "--plan",
        PLAN_REL,
    ];
    let operator_command = ["workflow", "operator", "--plan", PLAN_REL, "--json"];
    let commands = [
        ("status", status_command.as_slice()),
        ("repair", repair_command.as_slice()),
        ("operator", operator_command.as_slice()),
    ];

    for (surface, command) in commands {
        let context = format!("{scenario} {surface} trust-boundary rejection");
        let output = run_featureforge_output(repo, state, command, true, &context);
        assert_trust_boundary_failure(&output, &context, message_fragment);
    }
}

#[cfg(unix)]
#[test]
fn symlinked_authoritative_state_is_rejected_across_status_repair_operator_handoff() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-symlinked-authoritative-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);

    let state_path = authoritative_harness_state_path(repo, state);
    let events_path = state_path.with_file_name("events.jsonl");
    if let Err(error) = fs::remove_file(&events_path) {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::NotFound,
            "event log should be removable before symlink state fixture"
        );
    }
    fs::remove_file(&state_path).expect("authoritative state fixture should be removable");
    symlink("missing-authoritative-state.json", &state_path)
        .expect("dangling authoritative state symlink should be creatable");

    assert_event_or_state_loader_rejection_across_public_surfaces(
        repo,
        state,
        "symlinked authoritative state",
        "Authoritative harness state path must not be a symlink",
    );
}

#[cfg(unix)]
#[test]
fn symlinked_event_log_is_rejected_across_status_repair_operator_handoff() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-symlinked-authoritative-events");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);

    let state_path = authoritative_harness_state_path(repo, state);
    let events_path = state_path.with_file_name("events.jsonl");
    fs::remove_file(&events_path).expect("authoritative event log fixture should be removable");
    symlink("missing-authoritative-events.jsonl", &events_path)
        .expect("dangling authoritative event log symlink should be creatable");

    assert_event_or_state_loader_rejection_across_public_surfaces(
        repo,
        state,
        "symlinked authoritative event log",
        "Authoritative event log path must not be a symlink",
    );
}

#[test]
fn non_file_event_log_is_rejected_across_status_repair_operator_handoff() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-directory-authoritative-events");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);

    let state_path = authoritative_harness_state_path(repo, state);
    let events_path = state_path.with_file_name("events.jsonl");
    fs::remove_file(&events_path).expect("authoritative event log fixture should be removable");
    fs::create_dir_all(&events_path)
        .expect("directory fixture for authoritative events should create");

    assert_event_or_state_loader_rejection_across_public_surfaces(
        repo,
        state,
        "non-file authoritative event log",
        "Authoritative event log must be a regular file",
    );
}

#[test]
fn execution_module_exports_query_boundary() {
    let execution_mod = fs::read_to_string(repo_root().join("src/execution/mod.rs"))
        .expect("execution mod should be readable");
    let query_source = fs::read_to_string(repo_root().join("src/execution/query.rs"))
        .expect("execution query source should be readable");
    let recording_source = fs::read_to_string(repo_root().join("src/execution/recording.rs"))
        .expect("execution recording source should be readable");
    let review_state_source = fs::read_to_string(repo_root().join("src/execution/review_state.rs"))
        .expect("execution review-state source should be readable");
    let operator_source = fs::read_to_string(repo_root().join("src/workflow/operator.rs"))
        .expect("workflow operator source should be readable");

    assert!(
        execution_mod.contains("pub mod query;"),
        "execution module should expose a dedicated query boundary for workflow and review-state consumers",
    );
    assert!(
        execution_mod.contains("pub mod recording;"),
        "execution module should expose a dedicated recording boundary for closure and milestone writers",
    );
    assert!(
        execution_mod.contains("query owns the authoritative review-state read model")
            && query_source.contains("workflow consumes this module as a read-only client")
            && recording_source
                .contains("intent adapters should delegate authoritative writes here")
            && review_state_source.contains(
                "reconcile/explain commands stay thin over query and recording boundaries"
            )
            && operator_source
                .contains("Workflow routing consumes the execution-owned query surface"),
        "U12 ownership boundaries should be documented in the owning modules",
    );
}

#[test]
fn workflow_operator_uses_execution_query_boundary_instead_of_raw_execution_internals() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-operator-query-parity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_execution_in_progress(repo, state);

    let runtime = execution_runtime(repo, state);
    let plan = PathBuf::from(PLAN_REL);
    let routing = query_workflow_routing_state_for_runtime(&runtime, Some(&plan), false)
        .expect("routing query should succeed for active execution boundary fixture");
    let operator = run_workflow_operator_json(
        repo,
        state,
        PLAN_REL,
        false,
        "workflow operator json for active execution boundary fixture",
    );

    assert!(
        routing.execution_command_context.is_some(),
        "active execution boundary fixture should expose execution_command_context from execution/query",
    );
    assert_routing_parity_with_operator_json(&routing, &operator);
}

#[test]
fn missing_state_projection_keeps_status_and_operator_event_authoritative() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-missing-state-projection");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_execution_in_progress(repo, state);

    let state_path = authoritative_harness_state_path(repo, state);
    fs::remove_file(&state_path).expect("state projection should be removable for event-only read");

    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "plan execution status should route from events when state projection is missing",
    );
    let operator = run_workflow_operator_json(
        repo,
        state,
        PLAN_REL,
        false,
        "workflow operator should route from events when state projection is missing",
    );

    assert_eq!(
        operator["recommended_command"], status["recommended_command"],
        "status and operator should preserve shared route truth when only events.jsonl remains authoritative"
    );
    assert_eq!(
        operator["phase_detail"], status["phase_detail"],
        "phase routing should remain event-authoritative when state.json is absent"
    );
    assert!(
        !state_path.exists(),
        "read surfaces must not regenerate state.json just because it is missing"
    );
}

#[cfg(unix)]
#[test]
fn malformed_state_projection_is_ignored_when_event_log_is_authoritative() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-malformed-state-projection-ignored");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_execution_in_progress(repo, state);

    let state_path = authoritative_harness_state_path(repo, state);
    fs::remove_file(&state_path).expect("state projection should be removable for symlink fixture");
    symlink("missing-projection-state.json", &state_path)
        .expect("dangling projection state symlink should be creatable");

    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "plan execution status should ignore malformed projection when events are authoritative",
    );
    let operator = run_workflow_operator_json(
        repo,
        state,
        PLAN_REL,
        false,
        "workflow operator should ignore malformed projection when events are authoritative",
    );

    assert_eq!(
        operator["phase_detail"], status["phase_detail"],
        "projection-only state.json shape must not change event-authoritative route truth"
    );
}

#[test]
fn execution_query_boundary_stays_execution_owned() {
    let query_source = fs::read_to_string(repo_root().join("src/execution/query.rs"))
        .expect("execution query source should be readable");

    assert!(
        !query_source.contains("workflow::operator"),
        "execution query boundary should not depend on workflow/operator to derive review-state truth",
    );
    assert!(
        query_source.contains("pub struct ExecutionRoutingState")
            && query_source.contains("pub fn query_workflow_routing_state("),
        "execution query boundary should expose an execution-owned routing snapshot",
    );
    assert!(
        query_source.contains("discover_slug_identity_and_head"),
        "execution query boundary should resolve fallback repository slug+head through shared git helpers instead of re-deriving identity fields independently",
    );
    assert!(
        query_source.contains("pub finish_review_gate_pass_branch_closure_id: Option<String>"),
        "execution query boundary should expose finish-review gate pass branch closure identity",
    );
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

    let router_source = fs::read_to_string(repo_root().join("src/execution/router.rs"))
        .expect("execution router source should be readable");

    assert!(
        router_source.contains("phase::DETAIL_TASK_CLOSURE_RECORDING_READY")
            && router_source.contains("task_number: Some(task_number)")
            && router_source.contains("dispatch_id: task_review_dispatch_id.clone()"),
        "task_closure_recording_ready should expose task_number and may surface dispatch_id in recording_context",
    );
    assert!(
        router_source.contains("phase::DETAIL_RELEASE_READINESS_RECORDING_READY")
            && router_source.contains("phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED")
            && router_source.contains("branch_closure_id: Some(branch_closure_id.clone())"),
        "release-readiness recording-ready states should expose branch_closure_id recording_context ids",
    );
    assert!(
        router_source.contains("phase::DETAIL_FINAL_REVIEW_RECORDING_READY")
            && router_source.contains("dispatch_id: final_review_dispatch_id.clone()")
            && router_source.contains("branch_closure_id: Some(branch_closure_id.clone())"),
        "final_review_recording_ready should expose branch_closure_id and may surface dispatch_id in the routing constructor",
    );
}

#[test]
fn workflow_direct_and_real_cli_read_surfaces_stay_semantically_aligned() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-workflow-direct-real-cli-alignment");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_execution_in_progress(repo, state);

    let command = ["workflow", "operator", "--plan", PLAN_REL, "--json"];
    let direct = run_featureforge_json(
        repo,
        state,
        command.as_slice(),
        false,
        "direct workflow json",
    );
    let real = run_featureforge_json(
        repo,
        state,
        command.as_slice(),
        true,
        "real-cli workflow json",
    );
    assert_eq!(
        direct, real,
        "workflow json command output must stay aligned between direct and real-cli paths for command {:?}",
        command
    );
}

#[test]
fn workflow_direct_and_real_cli_read_surfaces_stay_semantically_aligned_for_task_boundary_blocked_fixture()
 {
    let (repo_dir, state_dir) =
        init_repo("contracts-boundary-workflow-direct-real-cli-alignment-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);

    for command in [
        ["workflow", "operator", "--plan", PLAN_REL, "--json"].as_slice(),
        [
            "workflow",
            "operator",
            "--plan",
            PLAN_REL,
            "--external-review-result-ready",
            "--json",
        ]
        .as_slice(),
    ] {
        let direct = run_featureforge_json(
            repo,
            state,
            command,
            false,
            "direct workflow json blocked fixture",
        );
        let real = run_featureforge_json(
            repo,
            state,
            command,
            true,
            "real-cli workflow json blocked fixture",
        );
        assert_eq!(
            direct, real,
            "workflow json command output must stay aligned between blocked fixture direct and real-cli paths for command {:?}",
            command
        );
    }
}

#[test]
fn workflow_status_legacy_summary_flag_failure_stays_aligned_with_real_cli() {
    let repo_dir = TempDir::new().expect("non-repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let direct = run_featureforge_output(
        repo,
        state,
        &["workflow", "status", "--summary"],
        false,
        "direct workflow status legacy summary failure",
    );
    let real = run_featureforge_output(
        repo,
        state,
        &["workflow", "status", "--summary"],
        true,
        "real-cli workflow status legacy summary failure",
    );

    assert!(
        !direct.status.success() && !real.status.success(),
        "workflow status --summary should fail for both direct and real-cli paths",
    );
    assert_eq!(
        direct.stdout, real.stdout,
        "workflow status legacy flag failure stdout must stay aligned between direct and real-cli paths",
    );
    assert_eq!(
        direct.stderr, real.stderr,
        "workflow status legacy flag failure stderr must stay aligned between direct and real-cli paths",
    );
    let failure = parse_json_output(&direct, "workflow status legacy flag failure");
    assert_eq!(failure["error_class"], "InvalidCommandInput");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("unexpected argument '--summary'")),
        "workflow status legacy flag should fail at CLI parsing, got {failure:?}"
    );
}

#[test]
fn mutation_commands_and_review_state_use_recording_boundary_for_transition_writes() {
    let mutate_source = fs::read_to_string(repo_root().join("src/execution/mutate.rs"))
        .expect("execution mutate source should be readable");
    let command_dependency_paths = execution_command_dependency_paths();
    let command_call_paths = execution_command_call_paths();
    let command_function_names = execution_command_function_names();
    let review_state_source = fs::read_to_string(repo_root().join("src/execution/review_state.rs"))
        .expect("execution review_state source should be readable");
    let review_state_dependency_paths = rust_source_scan::normalized_dependency_paths(
        "src/execution/review_state.rs",
        &review_state_source,
    );
    let review_state_call_paths = rust_source_scan::normalized_call_paths(
        "src/execution/review_state.rs",
        &review_state_source,
        &[],
    );

    assert!(
        mutate_source.contains("crate::execution::commands::"),
        "mutate.rs should remain a compatibility facade over execution command modules",
    );
    assert!(
        !mutate_source.contains("pub fn "),
        "mutate.rs should not regain public command implementation bodies after command extraction",
    );
    assert!(
        paths_contain_dependency(&command_dependency_paths, "crate::execution::recording"),
        "execution commands should consume the recording boundary for closure writes",
    );
    assert!(
        paths_contain_dependency(
            &command_dependency_paths,
            "crate::execution::command_eligibility"
        ),
        "execution commands should consume shared command eligibility helpers instead of re-deriving follow-up routing",
    );
    assert!(
        paths_contain_leaf(
            &command_call_paths,
            "project_runtime_routing_state_with_reduced_state"
        ),
        "execution commands should consume the execution-owned routing projection boundary",
    );
    assert!(
        !paths_contain_dependency(&command_dependency_paths, "crate::workflow::operator"),
        "execution commands should not import or call workflow/operator directly",
    );
    for forbidden in [
        "blocked_follow_up_for_operator",
        "close_current_task_required_follow_up",
        "late_stage_required_follow_up",
    ] {
        assert!(
            !command_function_names.iter().any(|name| name == forbidden),
            "execution commands should not redefine shared command eligibility helper `{forbidden}` directly",
        );
    }
    for forbidden in [
        "record_task_closure_result",
        "record_task_closure_negative_result",
        "remove_current_task_closure_results",
        "append_superseded_task_closure_ids",
        "append_superseded_branch_closure_ids",
        "set_current_branch_closure_id",
        "record_final_review_result",
        "record_release_readiness_result",
        "record_browser_qa_result",
    ] {
        assert!(
            !paths_contain_leaf(&command_call_paths, forbidden),
            "execution commands should not call transition write primitive `{forbidden}` directly",
        );
    }

    assert!(
        paths_contain_dependency(
            &review_state_dependency_paths,
            "crate::execution::recording"
        ),
        "review_state.rs should consume the recording boundary for overlay restoration",
    );
    assert!(
        !paths_contain_leaf(
            &review_state_call_paths,
            "load_authoritative_transition_state"
        ),
        "review_state.rs should not load transition state directly for overlay restoration",
    );
    assert!(
        !paths_contain_leaf(&review_state_call_paths, "set_current_branch_closure_id"),
        "review_state.rs should not call transition write primitives directly",
    );
    assert!(
        !paths_contain_leaf(&review_state_call_paths, "parse_artifact_document"),
        "review_state.rs should not parse rendered artifacts directly when reconciling authoritative state",
    );
}

#[test]
fn normal_command_modules_do_not_write_projection_read_models_or_persist_transition_state_directly()
{
    let command_dir = repo_root().join("src/execution/commands");
    let allowed_projection_writers = ["materialize_projections.rs"];
    let allowed_direct_persist = [
        "common.rs", // shared authoritative persist/event-append boundary
    ];

    for entry in fs::read_dir(&command_dir).expect("execution command directory should be readable")
    {
        let path = entry
            .expect("execution command entry should be readable")
            .path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .expect("execution command module should have a utf-8 file name");
        let source =
            fs::read_to_string(&path).expect("execution command source should be readable");
        if !allowed_projection_writers.contains(&file_name) {
            let projection_write_violations = rust_source_scan::forbidden_call_violations(
                &format!("src/execution/commands/{file_name}"),
                &source,
                &[
                    "materialize_late_stage_projection_artifacts",
                    "materialize_authoritative_transition_state_projection",
                    "regenerate_projection_artifacts_from_authoritative_state",
                    "write_execution_projection_read_models",
                    "write_authoritative_unit_review_receipt_artifact",
                    "write_project_artifact",
                    "write_project_artifact_at_path",
                ],
                &[],
            );
            assert!(
                projection_write_violations.is_empty(),
                "{file_name} must not write projection read models directly; projection writes belong only to explicit materialization command behavior:\n{}",
                projection_write_violations.join("\n")
            );
        }
        if !allowed_direct_persist.contains(&file_name) {
            let persist_violations = rust_source_scan::forbidden_call_violations(
                &format!("src/execution/commands/{file_name}"),
                &source,
                &["persist_if_dirty_with_failpoint"],
                &[],
            );
            assert!(
                persist_violations.is_empty(),
                "{file_name} must append authoritative transition state through commands/common.rs helpers:\n{}",
                persist_violations.join("\n")
            );
        }
    }
    let common_source = fs::read_to_string(command_dir.join("common.rs"))
        .expect("execution command common source should be readable");
    let common_production_source = common_source
        .split("#[cfg(test)]")
        .next()
        .unwrap_or(common_source.as_str());
    for forbidden in [
        "transfer_repair_step",
        "record_workflow_transfer",
        "record_runtime_handoff_checkpoint",
        "unit-review-",
        "task-verification-",
    ] {
        if forbidden.contains('-') {
            assert!(
                !common_production_source.contains(forbidden),
                "command-specific flow and projection materialization should not live in commands/common.rs: found `{forbidden}`"
            );
        } else {
            let common_violations = rust_source_scan::forbidden_call_violations(
                "src/execution/commands/common.rs",
                common_production_source,
                &[forbidden],
                &[],
            );
            assert!(
                common_violations.is_empty(),
                "command-specific flow and projection materialization should not live in commands/common.rs: found `{forbidden}`:\n{}",
                common_violations.join("\n")
            );
        }
    }
    let transitions_source = fs::read_to_string(repo_root().join("src/execution/transitions.rs"))
        .expect("execution transition source should be readable");
    assert!(
        !transitions_source.contains("write_atomic_file(&self.state_path"),
        "authoritative transition persistence must append events only; state.json projection writes belong to explicit materialize-projections behavior"
    );
}

#[test]
fn reconcile_review_state_threads_external_review_ready_through_routing_requeries() {
    let review_state_source = fs::read_to_string(repo_root().join("src/execution/review_state.rs"))
        .expect("execution review_state source should be readable");
    assert!(
        !review_state_source
            .contains("query_workflow_routing_state_for_runtime(runtime, Some(&args.plan), false)"),
        "{} should not hardcode external_review_result_ready=false when requerying authoritative routing",
        concat!("reconcile", "-review-state"),
    );
}

#[test]
fn explicit_mutation_paths_keep_strict_authoritative_state_validation() {
    let runtime_methods_source =
        fs::read_to_string(repo_root().join("src/execution/state/runtime_methods.rs"))
            .expect("execution runtime methods source should be readable");
    let review_gate_source =
        fs::read_to_string(repo_root().join("src/execution/state/review_gate.rs"))
            .expect("execution review gate source should be readable");
    let dispatch_start = runtime_methods_source
        .find("fn record_review_dispatch_strategy_checkpoint(")
        .expect("runtime_methods.rs should keep record_review_dispatch_strategy_checkpoint");
    let dispatch_end = runtime_methods_source[dispatch_start..]
        .find("fn ensure_review_dispatch_authoritative_bootstrap(")
        .map(|offset| dispatch_start + offset)
        .expect("runtime_methods.rs should keep ensure_review_dispatch_authoritative_bootstrap");
    let checkpoint_start = review_gate_source
        .find("fn persist_finish_review_gate_pass_checkpoint(")
        .expect("review_gate.rs should keep persist_finish_review_gate_pass_checkpoint");
    let checkpoint_end = review_gate_source[checkpoint_start..]
        .find("fn gate_review_from_context_internal(")
        .map(|offset| checkpoint_start + offset)
        .expect("review_gate.rs should keep gate_review_from_context_internal");
    let dispatch_source = &runtime_methods_source[dispatch_start..dispatch_end];
    let checkpoint_source = &review_gate_source[checkpoint_start..checkpoint_end];

    assert!(
        dispatch_source.contains("load_authoritative_transition_state("),
        "{} mutation should validate authoritative active-contract truth through the strict transition-state loader",
        concat!("record", "-review-dispatch"),
    );
    assert!(
        !dispatch_source.contains("load_authoritative_transition_state_relaxed("),
        "{} mutation must not bypass active-contract validation with the relaxed transition-state loader",
        concat!("record", "-review-dispatch"),
    );
    assert!(
        checkpoint_source.contains("load_authoritative_transition_state("),
        "{} checkpoint mutation should validate authoritative active-contract truth through the strict transition-state loader",
        concat!("gate", "-review"),
    );
    assert!(
        !checkpoint_source.contains("load_authoritative_transition_state_relaxed("),
        "{} checkpoint mutation must not bypass active-contract validation with the relaxed transition-state loader",
        concat!("gate", "-review"),
    );
}

#[test]
fn gate_follow_up_contract_uses_exact_shared_fs04_action() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-fs04-shared-action");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);
    let runtime = execution_runtime(repo, state);
    let plan = PathBuf::from(PLAN_REL);
    let routing = query_workflow_routing_state_for_runtime(&runtime, Some(&plan), true)
        .expect("FS-04 shared-action routing query should succeed");
    let operator = run_workflow_operator_json(
        repo,
        state,
        PLAN_REL,
        true,
        "FS-04 shared-action workflow operator parity",
    );

    assert_eq!(
        routing.phase_detail, "task_closure_recording_ready",
        "FS-04 shared-action contract should expose the exact closure-recording phase detail"
    );
    assert_eq!(
        routing.next_action, "close current task",
        "FS-04 shared-action contract should expose the exact shared next action"
    );
    let recommended_command = routing
        .recommended_command
        .as_deref()
        .expect("FS-04 shared-action contract should expose the exact recommended command");
    assert!(
        recommended_command.starts_with("featureforge plan execution close-current-task --plan "),
        "FS-04 shared-action contract should route through close-current-task, got {recommended_command}"
    );
    assert!(
        recommended_command.contains("--task 1"),
        "FS-04 shared-action contract should stay pinned to Task 1, got {recommended_command}"
    );
    assert_routing_parity_with_operator_json(&routing, &operator);
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        true,
        "FS-04 shared-action routed-command parity",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator,
        &routed_follow_up,
        "FS-04 shared-action routed-command parity",
    );

    let query_source = fs::read_to_string(repo_root().join("src/execution/query.rs"))
        .expect("execution query source should be readable");
    let query_follow_up_start = query_source
        .find("pub(crate) fn required_follow_up_from_routing(")
        .expect("query.rs should keep required_follow_up_from_routing");
    let query_follow_up_end = query_source[query_follow_up_start..]
        .find("pub(crate) fn normalize_public_follow_up_alias(")
        .map(|offset| query_follow_up_start + offset)
        .expect("query.rs should keep normalize_public_follow_up_alias after follow-up helper");
    let query_follow_up_source = &query_source[query_follow_up_start..query_follow_up_end];
    assert!(
        query_follow_up_source.contains("required_follow_up_from_route_decision")
            && query_follow_up_source.contains("route_decision_from_routing"),
        "query follow-up projection must delegate to the shared RouteDecision authority"
    );

    let router_source = fs::read_to_string(repo_root().join("src/execution/router.rs"))
        .expect("execution router source should be readable");
    let follow_up_start = router_source
        .find("fn derive_required_follow_up_from_optional_status")
        .expect("router.rs should keep shared derive_required_follow_up_from_optional_status");
    let follow_up_end = router_source[follow_up_start..]
        .find("fn route_requires_review_state_repair(")
        .map(|offset| follow_up_start + offset)
        .expect("router.rs should keep route_requires_review_state_repair");
    let follow_up_source = &router_source[follow_up_start..follow_up_end];
    let repair_index = follow_up_source
        .find("route_requires_review_state_repair(")
        .expect("shared required-follow-up derivation should consult repair routing");
    let late_stage_index = follow_up_source
        .find("follow_up_from_phase_detail(phase_detail")
        .expect("shared required-follow-up derivation should keep phase-detail fallback");
    assert!(
        repair_index < late_stage_index,
        "shared required-follow-up derivation must prefer repair routing before phase-detail fallback"
    );
    assert!(
        follow_up_source.contains("normalize_public_routing_follow_up_token")
            && follow_up_source.contains("follow_up_from_phase_detail"),
        "router required-follow-up derivation must delegate alias and phase-detail truth to execution::follow_up"
    );

    let follow_up_helper_source =
        fs::read_to_string(repo_root().join("src/execution/follow_up.rs"))
            .expect("execution follow-up helper source should be readable");
    assert!(
        follow_up_helper_source.contains("pub(crate) enum FollowUpKind")
            && follow_up_helper_source.contains("pub(crate) enum FollowUpAliasContext")
            && follow_up_helper_source
                .contains("pub(crate) fn normalize_public_routing_follow_up_token")
            && follow_up_helper_source
                .contains("pub(crate) fn normalize_persisted_repair_follow_up_token")
            && follow_up_helper_source.contains("pub(crate) fn follow_up_from_phase_detail"),
        "required-follow-up taxonomy, aliasing, and phase-detail mapping must stay centralized in execution::follow_up"
    );

    let runtime_methods_source =
        fs::read_to_string(repo_root().join("src/execution/state/runtime_methods.rs"))
            .expect("execution runtime methods source should be readable");
    let explicit_start = runtime_methods_source
        .find("fn specific_gate_reason_is_explicit_direct_follow_up(")
        .expect("runtime_methods.rs should keep specific_gate_reason_is_explicit_direct_follow_up");
    let explicit_end = runtime_methods_source[explicit_start..]
        .find("fn specific_gate_reason_is_direct_follow_up(")
        .map(|offset| explicit_start + offset)
        .expect("runtime_methods.rs should keep specific_gate_reason_is_direct_follow_up");
    let explicit_source = &runtime_methods_source[explicit_start..explicit_end];
    assert!(
        !explicit_source.contains("reason_code_indicates_stale_unreviewed"),
        "gate follow-up compatibility fallback must not re-derive branch-closure routing from stale_unreviewed reason-code heuristics",
    );
    assert!(
        !explicit_source.contains("current_branch_closure_id_missing"),
        "gate follow-up compatibility fallback must not hardcode current_branch_closure_id_missing into a direct branch-closure recommendation",
    );
}

#[test]
fn runtime_remediation_inventory_includes_boundary_contract_regressions() {
    let inventory = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime-remediation/README.md"),
    )
    .expect("runtime-remediation inventory should be readable");
    assert!(
        inventory.contains("FS-03"),
        "runtime-remediation inventory should include FS-03 compiled-cli target-coherence coverage"
    );
    assert!(
        inventory.contains("FS-04"),
        "runtime-remediation inventory should include FS-04 repair-route parity coverage"
    );
    assert!(
        inventory.contains("FS-05"),
        "runtime-remediation inventory should include FS-05 mutation-before-validation coverage"
    );
    assert!(
        inventory.contains("FS-06"),
        "runtime-remediation inventory should include FS-06 compiled-cli parity coverage"
    );
    assert!(
        inventory.contains("FS-08"),
        "runtime-remediation inventory should include FS-08 stale-blocker visibility coverage"
    );
    assert!(
        inventory.contains("FS-13"),
        "runtime-remediation inventory should include FS-13 authoritative open-step runtime-state coverage"
    );
    assert!(
        inventory.contains("FS-14"),
        "runtime-remediation inventory should include FS-14 closure-baseline repair routing coverage"
    );
    assert!(
        inventory.contains("FS-15"),
        "runtime-remediation inventory should include FS-15 earliest-stale-boundary targeting coverage"
    );
    assert!(
        inventory.contains("FS-16"),
        "runtime-remediation inventory should include FS-16 begin-time closure-authority coverage"
    );
    assert!(
        inventory.contains(
            "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs15_compiled_cli_never_prefers_later_stale_task"
        ),
        "runtime-remediation inventory should map FS-15 parity coverage to the compiled-cli boundary contract test"
    );
    assert!(
        inventory.contains("tests/contracts_execution_runtime_boundaries.rs"),
        "runtime-remediation inventory should map boundary coverage to tests/contracts_execution_runtime_boundaries.rs"
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
fn runtime_remediation_fs04_repair_route_visibility_stays_aligned_between_direct_and_compiled_cli()
{
    let run_case = |label: &str, real_cli: bool| -> (Value, Value) {
        let (repo_dir, state_dir) = init_repo(&format!(
            "contracts-boundary-runtime-remediation-fs04-{label}"
        ));
        let repo = repo_dir.path();
        let state = state_dir.path();
        setup_task_boundary_blocked_case(repo, state);
        let operator = run_featureforge_json(
            repo,
            state,
            &["workflow", "operator", "--plan", PLAN_REL, "--json"],
            real_cli,
            &format!("FS-04 {label} workflow operator shared-route parity"),
        );
        let repair = run_featureforge_json(
            repo,
            state,
            &[
                "plan",
                "execution",
                "repair-review-state",
                "--plan",
                PLAN_REL,
            ],
            real_cli,
            &format!("FS-04 {label} repair-review-state route visibility"),
        );
        let recommended_command = repair["recommended_command"]
            .as_str()
            .expect("FS-04 repair output should expose recommended command");
        assert!(
            recommended_command
                .starts_with("featureforge plan execution close-current-task --plan "),
            "FS-04 closure-baseline route should keep repair/operator on close-current-task guidance, got {recommended_command}"
        );
        let routed_follow_up = run_recommended_plan_execution_command(
            repo,
            state,
            recommended_command,
            real_cli,
            &format!("FS-04 {label} routed-command parity"),
        );
        assert_follow_up_blocker_parity_with_operator(
            &operator,
            &routed_follow_up,
            &format!("FS-04 {label} routed-command parity"),
        );
        assert!(
            matches!(
                routed_follow_up["action"].as_str(),
                Some("recorded" | "already_current")
            ),
            "FS-04 {label} routed command must be immediately runnable when repair reports already_current, got {routed_follow_up:?}"
        );
        (operator, repair)
    };

    let (mut operator_direct, repair_direct) = run_case("direct", false);
    let (mut operator_real, repair_real) = run_case("compiled-cli", true);
    projection_support::normalize_state_dir_projection_paths_for_parity(&mut operator_direct);
    projection_support::normalize_state_dir_projection_paths_for_parity(&mut operator_real);
    assert_eq!(
        operator_direct, operator_real,
        "FS-04 direct and compiled-cli operator outputs must stay semantically aligned"
    );
    assert_eq!(
        repair_direct["action"], repair_real["action"],
        "FS-04 direct and compiled-cli repair actions must stay aligned"
    );
    assert_eq!(
        repair_direct["phase_detail"], repair_real["phase_detail"],
        "FS-04 direct and compiled-cli repair phase_detail must stay aligned"
    );
    assert_eq!(
        repair_direct["required_follow_up"], repair_real["required_follow_up"],
        "FS-04 direct and compiled-cli repair follow-up classification must stay aligned"
    );
    assert_eq!(
        operator_real["recommended_command"], repair_real["recommended_command"],
        "FS-04 compiled-cli repair output should keep operator and repair on the exact same shared command target"
    );
    assert_eq!(
        operator_direct["recommended_command"], repair_direct["recommended_command"],
        "FS-04 direct repair output should keep operator and repair on the exact same shared command target"
    );
    assert_eq!(
        repair_real["action"],
        Value::from("blocked"),
        "FS-04 closure-baseline repair should surface the shared closure-recording blocker instead of claiming repair is already current"
    );
    assert_eq!(
        repair_real["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-04 repair-review-state should expose the exact shared next-action phase detail for closure-baseline recovery, got {repair_real:?}"
    );
    assert!(
        repair_real["required_follow_up"].is_null(),
        "FS-04 closure-baseline recovery should not force a stale follow-up category once shared routing is closure-recording-ready, got {repair_real:?}"
    );
}

#[test]
fn runtime_remediation_fs04_repair_review_state_accepts_external_review_ready_flag_without_irrelevant_route_drift()
 {
    let command = [
        "plan",
        "execution",
        "repair-review-state",
        "--plan",
        PLAN_REL,
        "--external-review-result-ready",
    ];

    let mut results = Vec::new();
    for (label, real_cli) in [("direct", false), ("compiled-cli", true)] {
        let (repo_dir, state_dir) = init_repo(&format!(
            "contracts-boundary-runtime-remediation-fs04-repair-flag-{label}"
        ));
        let repo = repo_dir.path();
        let state = state_dir.path();
        setup_task_boundary_blocked_case(repo, state);
        let output = run_featureforge_output(repo, state, &command, real_cli, label);
        assert!(
            output.status.success(),
            "FS-04 {label} path should accept external-review-result-ready and return a routed repair result\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let routed = parse_json_output(
            &output,
            &format!("FS-04 {label} repair-review-state flag acceptance"),
        );
        let operator = run_featureforge_json(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                PLAN_REL,
                "--external-review-result-ready",
                "--json",
            ],
            real_cli,
            &format!("FS-04 {label} operator parity after repair-review-state flag acceptance"),
        );
        assert_eq!(
            routed["recommended_command"], operator["recommended_command"],
            "FS-04 {label} repair and operator should expose the same command target before command-follow parity checks"
        );
        let recommended_command = routed["recommended_command"]
            .as_str()
            .expect("FS-04 routed output should expose recommended_command text");
        let routed_follow_up = run_recommended_plan_execution_command(
            repo,
            state,
            recommended_command,
            real_cli,
            &format!("FS-04 {label} routed-command parity"),
        );
        assert_follow_up_blocker_parity_with_operator(
            &operator,
            &routed_follow_up,
            &format!("FS-04 {label} routed-command parity"),
        );
        results.push((label, routed, routed_follow_up));
    }

    assert_eq!(results[0].1["action"], results[1].1["action"]);
    assert_eq!(
        results[0].1["required_follow_up"],
        results[1].1["required_follow_up"]
    );
    assert_eq!(
        results[0].2["action"], results[1].2["action"],
        "FS-04 direct and compiled-cli routed-command actions should stay aligned"
    );
    let direct_recommended = results[0].1["recommended_command"]
        .as_str()
        .expect("FS-04 direct route should expose recommended_command text");
    let compiled_recommended = results[1].1["recommended_command"]
        .as_str()
        .expect("FS-04 compiled-cli route should expose recommended_command text");
    let normalized_recommended_command = |command: &str| {
        command
            .split(" --expect-execution-fingerprint ")
            .next()
            .unwrap_or(command)
            .to_owned()
    };
    assert_eq!(
        normalized_recommended_command(direct_recommended),
        normalized_recommended_command(compiled_recommended),
        "FS-04 direct and compiled-cli routes should stay command-shape aligned even when per-fixture execution fingerprints differ"
    );
}

#[test]
fn runtime_remediation_fs08_stale_blocker_visibility_stays_aligned_between_direct_and_compiled_cli()
{
    let (repo_dir, state_dir) = init_repo("contracts-boundary-runtime-remediation-fs08");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_fs08_stale_blocker_fixture(repo, state);

    let operator_direct = run_featureforge_json(
        repo,
        state,
        &["workflow", "operator", "--plan", PLAN_REL, "--json"],
        false,
        "FS-08 direct operator stale-blocker visibility",
    );
    let operator_real = run_featureforge_json(
        repo,
        state,
        &["workflow", "operator", "--plan", PLAN_REL, "--json"],
        true,
        "FS-08 compiled-cli operator stale-blocker visibility",
    );
    assert_eq!(
        operator_direct, operator_real,
        "FS-08 direct and compiled-cli operator outputs must stay semantically aligned"
    );
    assert_eq!(
        operator_real["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-08 stale blocker should remain visible as task_closure_recording_ready"
    );
    assert_eq!(operator_real["blocking_scope"], Value::from("task"));
    assert_eq!(operator_real["blocking_task"], Value::from(1_u64));
    let mut operator_reason_codes = operator_real["blocking_reason_codes"]
        .as_array()
        .expect("FS-08 operator should expose blocking_reason_codes as an array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    operator_reason_codes.sort();
    let expected_reason_codes = vec![
        String::from("prior_task_current_closure_missing"),
        String::from("task_closure_baseline_repair_candidate"),
    ];
    assert_eq!(
        operator_reason_codes, expected_reason_codes,
        "FS-08 operator should expose the exact stale-blocker reason-code set for this fixture"
    );
    let operator_recommended = operator_real["recommended_command"]
        .as_str()
        .expect("FS-08 operator should expose a recommended command");

    let status_direct = run_featureforge_json(
        repo,
        state,
        &["plan", "execution", "status", "--plan", PLAN_REL],
        false,
        "FS-08 direct status stale-blocker visibility",
    );
    let status_real = run_featureforge_json(
        repo,
        state,
        &["plan", "execution", "status", "--plan", PLAN_REL],
        true,
        "FS-08 compiled-cli status stale-blocker visibility",
    );
    assert_eq!(
        status_direct, status_real,
        "FS-08 direct and compiled-cli status outputs must stay semantically aligned"
    );
    assert_eq!(
        status_real["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-08 status should keep stale blocker visible as task_closure_recording_ready"
    );
    assert_eq!(status_real["blocking_scope"], Value::from("task"));
    assert_eq!(status_real["blocking_task"], Value::from(1_u64));
    let mut status_reason_codes = status_real["blocking_reason_codes"]
        .as_array()
        .expect("FS-08 status should expose blocking_reason_codes as an array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    status_reason_codes.sort();
    assert_eq!(
        status_reason_codes, expected_reason_codes,
        "FS-08 status should expose the exact stale-blocker reason-code set for this fixture"
    );
    let (direct_repo_dir, direct_state_dir) =
        init_repo("contracts-boundary-runtime-remediation-fs08-direct-follow-up");
    let direct_repo = direct_repo_dir.path();
    let direct_state = direct_state_dir.path();
    setup_fs08_stale_blocker_fixture(direct_repo, direct_state);
    let direct_operator_follow_up = run_recommended_plan_execution_command(
        direct_repo,
        direct_state,
        operator_recommended,
        false,
        "FS-08 direct operator recommended command follow-up parity",
    );
    assert!(
        direct_operator_follow_up["action"].as_str().is_some(),
        "FS-08 direct follow-up command should return an action payload"
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_direct,
        &direct_operator_follow_up,
        "FS-08 direct command-follow parity",
    );
    let (compiled_repo_dir, compiled_state_dir) =
        init_repo("contracts-boundary-runtime-remediation-fs08-compiled-follow-up");
    let compiled_repo = compiled_repo_dir.path();
    let compiled_state = compiled_state_dir.path();
    setup_fs08_stale_blocker_fixture(compiled_repo, compiled_state);
    let operator_follow_up = run_recommended_plan_execution_command(
        compiled_repo,
        compiled_state,
        operator_recommended,
        true,
        "FS-08 compiled-cli operator recommended command follow-up parity",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_real,
        &operator_follow_up,
        "FS-08 command-follow parity",
    );
    assert!(
        operator_follow_up["action"].as_str().is_some(),
        "FS-08 follow-up command should return an action payload"
    );
    assert!(
        operator_recommended.starts_with("featureforge plan execution close-current-task --plan "),
        "FS-08 closure-baseline route should recommend close-current-task in direct and compiled-cli surfaces, got {operator_recommended}"
    );
    assert!(
        operator_recommended.contains("--task 1"),
        "FS-08 closure-baseline route should remain pinned to Task 1, got {operator_recommended}"
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
    let repair_task1_command = repair_task1["recommended_command"]
        .as_str()
        .expect("FS-15 bootstrap task 1 repair-review-state should expose a follow-up command");
    assert!(
        repair_task1_command.contains("close-current-task"),
        "FS-15 bootstrap task 1 repair-review-state should route through close-current-task, got {repair_task1_command}"
    );
    let close_task1 = run_recommended_plan_execution_command(
        repo,
        state,
        repair_task1_command,
        true,
        "FS-15 bootstrap task 1 repair-review-state follow-up",
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
    let recommended_command = operator_real["recommended_command"]
        .as_str()
        .expect("FS-15 compiled-cli operator should expose a routed command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-15 compiled-cli operator should route to Task 2 while it is the earliest stale boundary, got {recommended_command}",
    );
    assert!(
        !recommended_command.contains("--task 6"),
        "FS-15 compiled-cli operator must not route to Task 6 while Task 2 is stale, got {recommended_command}",
    );
    if operator_real["execution_command_context"]["command_kind"].as_str() == Some("reopen") {
        assert_eq!(
            operator_real["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-15 reopen command context should target Task 2"
        );
        assert_eq!(
            operator_real["execution_command_context"]["step_id"],
            Value::from(1_u64),
            "FS-15 reopen command context should target Step 1"
        );
        assert!(
            recommended_command.contains("--step 1"),
            "FS-15 reopen routing should keep Step 1 targeted, got {recommended_command}"
        );
    } else {
        assert!(
            recommended_command.contains("close-current-task"),
            "FS-15 non-reopen route should stay in closure-recording lane for Task 2, got {recommended_command}"
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

#[test]
fn runtime_remediation_fs13_authoritative_open_step_state_survives_compiled_cli_round_trip() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-runtime-remediation-fs13");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_execution_in_progress(repo, state);

    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_open_step_state"]["task"],
        Value::from(1_u64)
    );
    assert_eq!(
        authoritative_state["current_open_step_state"]["step"],
        Value::from(1_u64)
    );
    assert_eq!(
        authoritative_state["current_open_step_state"]["note_state"],
        Value::from("Active")
    );

    let status_real = run_featureforge_json(
        repo,
        state,
        &["plan", "execution", "status", "--plan", PLAN_REL],
        true,
        "FS-13 compiled-cli status before markdown note tamper",
    );
    assert_eq!(status_real["active_task"], Value::from(1_u64));
    assert_eq!(status_real["active_step"], Value::from(1_u64));

    let plan_path = repo.join(PLAN_REL);
    let plan_source =
        fs::read_to_string(&plan_path).expect("FS-13 plan source should be readable for tamper");
    let tampered = plan_source.replace(
        "- [ ] **Step 2: Validate workflow fixture output**",
        "- [ ] **Step 2: Validate workflow fixture output**\n\n  **Execution Note:** Interrupted - FS-13 tampered markdown note should be projection-only.",
    );
    fs::write(&plan_path, tampered).expect("FS-13 tampered plan source should be writable");

    let status_real_after_tamper = run_featureforge_json(
        repo,
        state,
        &["plan", "execution", "status", "--plan", PLAN_REL],
        true,
        "FS-13 compiled-cli status after markdown note tamper",
    );
    let status_direct_after_tamper = run_featureforge_json(
        repo,
        state,
        &["plan", "execution", "status", "--plan", PLAN_REL],
        false,
        "FS-13 direct status after markdown note tamper",
    );
    assert_eq!(
        status_real_after_tamper["active_task"],
        Value::from(1_u64),
        "FS-13 compiled-cli status must keep authoritative active open-step target despite markdown tamper"
    );
    assert_eq!(status_real_after_tamper["active_step"], Value::from(1_u64));
    assert_eq!(status_real_after_tamper["resume_task"], Value::Null);
    assert_eq!(
        status_direct_after_tamper["active_task"],
        status_real_after_tamper["active_task"]
    );
    assert_eq!(
        status_direct_after_tamper["active_step"],
        status_real_after_tamper["active_step"]
    );
    assert_eq!(
        status_direct_after_tamper["resume_task"],
        status_real_after_tamper["resume_task"]
    );

    update_authoritative_harness_state(repo, state, &[("current_open_step_state", Value::Null)]);

    let status_real_without_authority = run_featureforge_json(
        repo,
        state,
        &["plan", "execution", "status", "--plan", PLAN_REL],
        true,
        "FS-13 compiled-cli status should ignore legacy markdown note when authoritative open-step state is absent",
    );
    let status_direct_without_authority = run_featureforge_json(
        repo,
        state,
        &["plan", "execution", "status", "--plan", PLAN_REL],
        false,
        "FS-13 direct status should ignore legacy markdown note when authoritative open-step state is absent",
    );

    for payload in [
        &status_real_without_authority,
        &status_direct_without_authority,
    ] {
        assert_eq!(
            payload["active_task"],
            Value::Null,
            "FS-13 status must not derive active_task from markdown-only open-step notes when authoritative state is absent: {payload:?}",
        );
        assert_eq!(
            payload["active_step"],
            Value::Null,
            "FS-13 status must not derive active_step from markdown-only open-step notes when authoritative state is absent: {payload:?}",
        );
        assert_eq!(
            payload["resume_task"],
            Value::Null,
            "FS-13 status must not derive resume_task from markdown-only open-step notes when authoritative state is absent: {payload:?}",
        );
        assert_eq!(
            payload["resume_step"],
            Value::Null,
            "FS-13 status must not derive resume_step from markdown-only open-step notes when authoritative state is absent: {payload:?}",
        );
    }

    let operator_real_without_authority = run_featureforge_json(
        repo,
        state,
        &["workflow", "operator", "--plan", PLAN_REL, "--json"],
        true,
        "FS-13 compiled-cli operator should ignore legacy markdown note when authoritative open-step state is absent",
    );
    let operator_direct_without_authority = run_featureforge_json(
        repo,
        state,
        &["workflow", "operator", "--plan", PLAN_REL, "--json"],
        false,
        "FS-13 direct operator should ignore legacy markdown note when authoritative open-step state is absent",
    );

    for payload in [
        &operator_real_without_authority,
        &operator_direct_without_authority,
    ] {
        let recommended = payload["recommended_command"].as_str().unwrap_or("");
        assert!(
            !recommended.contains("--task 1 --step 1"),
            "FS-13 operator must not route to Task 1 Step 1 from markdown-only note text when authoritative open-step state is absent: {payload:?}",
        );
    }
}

#[test]
fn status_and_operator_share_state_taxonomy_and_semantic_identity_fields() {
    let (repo_dir, state_dir) = init_repo("boundary-state-taxonomy-semantic-identity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);

    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status taxonomy/semantic identity contract",
    );
    let operator = run_workflow_operator_json(
        repo,
        state,
        PLAN_REL,
        false,
        "operator taxonomy/semantic identity contract",
    );

    let state_kind = status["state_kind"]
        .as_str()
        .expect("status should expose state_kind");
    assert!(
        matches!(
            state_kind,
            "actionable_public_command"
                | "waiting_external_input"
                | "terminal"
                | "blocked_runtime_bug"
        ),
        "status state_kind should use the public taxonomy: {state_kind}"
    );
    assert_eq!(
        operator["state_kind"], status["state_kind"],
        "workflow operator and plan execution status must share the same routed state_kind"
    );

    let semantic_tree = status["semantic_workspace_tree_id"]
        .as_str()
        .expect("status should expose semantic_workspace_tree_id");
    assert!(
        !semantic_tree.trim().is_empty(),
        "status semantic_workspace_tree_id should be non-empty"
    );
    assert_eq!(
        operator["semantic_workspace_tree_id"], status["semantic_workspace_tree_id"],
        "workflow operator and plan execution status should converge on semantic workspace identity"
    );

    assert!(
        status
            .get("blockers")
            .is_none_or(|value| value.is_array() || value.is_null()),
        "status blockers should be optional and, when present, an array"
    );
    assert!(
        operator
            .get("blockers")
            .is_none_or(|value| value.is_array() || value.is_null()),
        "operator blockers should be optional and, when present, an array"
    );

    let status_next_public = status
        .get("next_public_action")
        .cloned()
        .unwrap_or(Value::Null);
    let operator_next_public = operator
        .get("next_public_action")
        .cloned()
        .unwrap_or(Value::Null);
    for next_public_action in [&status_next_public, &operator_next_public] {
        if let Some(next_public_action) = next_public_action.as_object() {
            let command = next_public_action
                .get("command")
                .and_then(Value::as_str)
                .expect("next_public_action command should be present when object is emitted");
            assert!(
                command.starts_with("featureforge "),
                "next_public_action command should be a public featureforge command: {command}"
            );
        }
    }

    assert!(
        status
            .get("raw_workspace_tree_id")
            .is_none_or(|value| value.is_string() || value.is_null()),
        "status raw_workspace_tree_id should remain debug-only optional string"
    );
    assert!(
        operator
            .get("raw_workspace_tree_id")
            .is_none_or(|value| value.is_string() || value.is_null()),
        "operator raw_workspace_tree_id should remain debug-only optional string"
    );
}
