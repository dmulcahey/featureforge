#[path = "support/featureforge.rs"]
mod featureforge_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use featureforge::cli::plan_execution::StatusArgs;
use featureforge::execution::query::{
    ExecutionRoutingState, query_review_state, query_workflow_execution_state,
    query_workflow_routing_state_for_runtime,
};
use runtime_support::execution_runtime;
use serde_json::Value;
use workflow_support::{init_repo, install_full_contract_ready_artifacts};

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
const TASK_BOUNDARY_BLOCKED_PLAN_SOURCE: &str = r#"# Runtime Integration Hardening Implementation Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
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
**Task Outcome:** Task 1 execution reaches a boundary gate before Task 2 starts.
**Plan Constraints:**
- Keep fixture inputs deterministic.
**Open Questions:** none

**Files:**
- Modify: `tests/execution_query.rs`

- [ ] **Step 1: Prepare workflow fixture output**
- [ ] **Step 2: Validate workflow fixture output**

## Task 2: Follow-on flow

**Spec Coverage:** VERIFY-001
**Task Outcome:** Task 2 should remain blocked until Task 1 closure requirements are met.
**Plan Constraints:**
- Preserve deterministic task-boundary diagnostics.
**Open Questions:** none

**Files:**
- Modify: `tests/execution_query.rs`

- [ ] **Step 1: Start the follow-on task**
"#;

fn run_plan_execution_json(repo: &Path, state: &Path, args: &[&str], context: &str) -> Value {
    let mut command_args = vec!["plan", "execution"];
    command_args.extend_from_slice(args);
    let output = featureforge_support::run_rust_featureforge(
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
    let output = featureforge_support::run_rust_featureforge(
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

fn prepare_preflight_acceptance_workspace(repo: &Path, branch_name: &str) {
    let mut checkout = Command::new("git");
    checkout
        .args(["checkout", "-B", branch_name])
        .current_dir(repo);
    process_support::run_checked(checkout, "git checkout fixture branch");
}

fn assert_routing_parity_with_operator_json(routing: &ExecutionRoutingState, operator: &Value) {
    assert_eq!(
        operator["phase"],
        Value::from(routing.phase.clone()),
        "routing phase should match workflow/operator"
    );
    assert_eq!(
        operator["phase_detail"],
        Value::from(routing.phase_detail.clone()),
        "routing phase detail should match workflow/operator",
    );
    assert_eq!(
        operator["review_state_status"],
        Value::from(routing.review_state_status.clone()),
        "routing review-state status should match workflow/operator",
    );
    assert_eq!(
        operator.get("qa_requirement").and_then(Value::as_str),
        routing.qa_requirement.as_deref(),
        "routing QA requirement should match workflow/operator",
    );
    assert_eq!(
        operator["follow_up_override"],
        Value::from(routing.follow_up_override.clone()),
        "routing follow-up override should match workflow/operator",
    );
    assert_eq!(
        operator
            .get("finish_review_gate_pass_branch_closure_id")
            .and_then(Value::as_str),
        routing.finish_review_gate_pass_branch_closure_id.as_deref(),
        "routing finish-review gate pass identity should match workflow/operator",
    );
    assert_eq!(
        operator["next_action"],
        Value::from(routing.next_action.clone()),
        "routing next action should match workflow/operator",
    );
    assert_eq!(
        operator.get("recommended_command").and_then(Value::as_str),
        routing.recommended_command.as_deref(),
        "routing recommended command should match workflow/operator",
    );
    assert_eq!(
        operator.get("blocking_scope").and_then(Value::as_str),
        routing.blocking_scope.as_deref(),
        "routing blocking scope should match workflow/operator",
    );
    assert_eq!(
        operator
            .get("blocking_task")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        routing.blocking_task,
        "routing blocking task should match workflow/operator",
    );
    assert_eq!(
        operator.get("external_wait_state").and_then(Value::as_str),
        routing.external_wait_state.as_deref(),
        "routing external wait state should match workflow/operator",
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
        operator_blocking_reason_codes, routing.blocking_reason_codes,
        "routing compact blocking reason codes should match workflow/operator",
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
        )),
        "routing recording context payload should match workflow/operator",
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
        )),
        "routing execution command context payload should match workflow/operator",
    );
}

fn setup_execution_in_progress(repo: &Path, state: &Path) {
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "execution-query-active-context");
    fs::write(repo.join(PLAN_REL), TASK_BOUNDARY_BLOCKED_PLAN_SOURCE)
        .expect("execution-query active-context plan should be writable");
    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status before active-context fixture begin",
    );
    let preflight = run_plan_execution_json(
        repo,
        state,
        &["preflight", "--plan", PLAN_REL],
        "preflight for active-context fixture",
    );
    assert_eq!(
        preflight["allowed"],
        Value::Bool(true),
        "preflight should allow active-context fixture"
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
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution_fingerprint"),
        ],
        "begin for active-context fixture",
    );
}

fn setup_task_boundary_blocked_case(repo: &Path, state: &Path) {
    install_full_contract_ready_artifacts(repo);
    fs::write(repo.join(PLAN_REL), TASK_BOUNDARY_BLOCKED_PLAN_SOURCE)
        .expect("task-boundary blocked plan fixture should write");
    prepare_preflight_acceptance_workspace(repo, "execution-query-task-boundary");

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status before task-boundary fixture execution",
    );
    let preflight = run_plan_execution_json(
        repo,
        state,
        &["preflight", "--plan", PLAN_REL],
        "preflight for task-boundary fixture execution",
    );
    assert_eq!(
        preflight["allowed"],
        Value::Bool(true),
        "preflight should allow task-boundary fixture"
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
            "Completed task 1 step 1 for execution-query task-boundary fixture.",
            "--manual-verify-summary",
            "Verified by execution-query task-boundary fixture setup.",
            "--file",
            "tests/execution_query.rs",
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
            "Completed task 1 step 2 for execution-query task-boundary fixture.",
            "--manual-verify-summary",
            "Verified by execution-query task-boundary fixture setup.",
            "--file",
            "tests/execution_query.rs",
            "--expect-execution-fingerprint",
            begin_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 2 for task-boundary fixture",
    );
}

#[test]
fn query_boundary_reports_empty_review_state_before_execution_starts() {
    let (repo_dir, state_dir) = init_repo("execution-query-empty-review-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);

    let runtime = execution_runtime(repo, state);
    let plan = PathBuf::from(PLAN_REL);
    let status_args = StatusArgs {
        plan: plan.clone(),
        external_review_result_ready: false,
    };

    let review_state = query_review_state(&runtime, &status_args)
        .expect("review-state query should succeed before execution starts");
    assert!(
        review_state.current_task_closures.is_empty(),
        "fresh approved plans should not expose current task closures before execution starts",
    );
    assert!(
        review_state.current_branch_closure.is_none(),
        "fresh approved plans should not expose a current branch closure before execution starts",
    );
    assert!(
        review_state.superseded_closures.is_empty(),
        "fresh approved plans should not expose superseded closures before execution starts",
    );
    assert!(
        review_state.stale_unreviewed_closures.is_empty(),
        "fresh approved plans should not expose stale-unreviewed closures before execution starts",
    );

    let workflow_state = query_workflow_execution_state(&runtime, PLAN_REL)
        .expect("workflow query should succeed before execution starts");
    assert_eq!(
        workflow_state
            .execution_status
            .as_ref()
            .map(|status| status.execution_started.as_str()),
        Some("no"),
        "workflow query should carry the current execution status snapshot",
    );
    assert!(
        workflow_state.preflight.is_some(),
        "workflow query should surface preflight state before execution starts",
    );
    assert!(
        workflow_state.gate_review.is_none(),
        "workflow query should not expose review-gate state before execution starts",
    );
    assert!(
        workflow_state.gate_finish.is_none(),
        "workflow query should not expose finish-gate state before execution starts",
    );
    assert_eq!(
        workflow_state.task_review_dispatch_id, None,
        "fresh approved plans should not expose task review dispatch lineage before execution starts",
    );
    assert_eq!(
        workflow_state.final_review_dispatch_id, None,
        "fresh approved plans should not expose final-review dispatch lineage before execution starts",
    );
    assert_eq!(
        workflow_state.current_branch_closure_id, None,
        "fresh approved plans should not expose a branch closure before execution starts",
    );
    assert_eq!(
        workflow_state.finish_review_gate_pass_branch_closure_id, None,
        "fresh approved plans should not expose a finish-review gate pass branch closure before execution starts",
    );
    assert_eq!(
        workflow_state.current_release_readiness_result, None,
        "fresh approved plans should not expose release-readiness state before execution starts",
    );
}

#[test]
fn routing_snapshot_matches_workflow_operator_output_before_execution_starts() {
    let (repo_dir, state_dir) = init_repo("execution-query-routing-snapshot");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);

    let plan = PathBuf::from(PLAN_REL);
    let runtime = execution_runtime(repo, state);
    let routing = query_workflow_routing_state_for_runtime(&runtime, Some(&plan), false)
        .expect("routing query should succeed before execution starts");
    let operator = run_workflow_operator_json(
        repo,
        state,
        PLAN_REL,
        false,
        "workflow operator json before execution starts",
    );

    assert_routing_parity_with_operator_json(&routing, &operator);
}

#[test]
fn routing_snapshot_matches_workflow_operator_execution_command_context_payload() {
    let (repo_dir, state_dir) = init_repo("execution-query-active-command-context");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_execution_in_progress(repo, state);

    let plan = PathBuf::from(PLAN_REL);
    let runtime = execution_runtime(repo, state);
    let routing = query_workflow_routing_state_for_runtime(&runtime, Some(&plan), false)
        .expect("routing query should succeed after execution has started");
    let operator = run_workflow_operator_json(
        repo,
        state,
        PLAN_REL,
        false,
        "workflow operator json for active execution routing",
    );

    assert!(
        routing.execution_command_context.is_some(),
        "active execution should expose execution command context through routing state: {routing:?}",
    );
    assert_routing_parity_with_operator_json(&routing, &operator);
}

#[test]
fn routing_snapshot_matches_workflow_operator_recording_context_payload() {
    let (repo_dir, state_dir) = init_repo("execution-query-task-closure-recording-context");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            PLAN_REL,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "execution query routing recording-context fixture dispatch",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));

    let plan = PathBuf::from(PLAN_REL);
    let runtime = execution_runtime(repo, state);
    let routing = query_workflow_routing_state_for_runtime(&runtime, Some(&plan), true)
        .expect("routing query should succeed for intent-level task-closure recording-ready state");
    let operator = run_workflow_operator_json(
        repo,
        state,
        PLAN_REL,
        true,
        "workflow operator json for task-closure recording-ready state",
    );

    assert_eq!(
        routing.phase_detail, "task_closure_recording_ready",
        "fixture should route to task_closure_recording_ready",
    );
    let routing_recording_context = routing
        .recording_context
        .as_ref()
        .expect("task_closure_recording_ready should include recording_context");
    assert_eq!(
        routing_recording_context.task_number,
        Some(1),
        "task_closure_recording_ready should carry task_number=1",
    );
    assert!(
        routing_recording_context
            .dispatch_id
            .as_deref()
            .is_some_and(|dispatch_id| !dispatch_id.trim().is_empty()),
        "task_closure_recording_ready should carry a non-empty dispatch_id",
    );
    assert_routing_parity_with_operator_json(&routing, &operator);
}

#[test]
fn runtime_remediation_fs07_query_surface_parity_for_task_review_dispatch_blocked() {
    let (repo_dir, state_dir) = init_repo("execution-query-fs07-task-review-dispatch-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);

    let plan = PathBuf::from(PLAN_REL);
    let runtime = execution_runtime(repo, state);
    let routing = query_workflow_routing_state_for_runtime(&runtime, Some(&plan), false)
        .expect("routing query should succeed for task-review-dispatch blocked fixture");
    let operator = run_workflow_operator_json(
        repo,
        state,
        PLAN_REL,
        false,
        "workflow operator json for FS-07 task-review-dispatch blocked fixture",
    );
    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "plan execution status for FS-07 task-review-dispatch blocked fixture",
    );
    let review_state = query_review_state(
        &runtime,
        &StatusArgs {
            plan: plan.clone(),
            external_review_result_ready: false,
        },
    )
    .expect("review-state query should succeed for FS-07 task-review-dispatch blocked fixture");

    assert_eq!(routing.phase_detail, "task_closure_recording_ready");
    assert_routing_parity_with_operator_json(&routing, &operator);
    assert_eq!(
        status["phase_detail"], operator["phase_detail"],
        "status and operator should agree on FS-07 task-review-dispatch routing detail"
    );
    assert_eq!(
        status["review_state_status"], operator["review_state_status"],
        "status and operator should agree on FS-07 review-state routing status"
    );
    assert!(
        review_state.current_branch_closure.is_none(),
        "FS-07 blocked-task fixture should not expose a current branch closure in query review-state snapshot"
    );
}

#[test]
fn routing_external_review_ready_without_dispatch_lineage_routes_to_close_current_task() {
    let (repo_dir, state_dir) = init_repo("execution-query-task-dispatch-lineage-required");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);

    let plan = PathBuf::from(PLAN_REL);
    let runtime = execution_runtime(repo, state);
    let routing = query_workflow_routing_state_for_runtime(&runtime, Some(&plan), true)
        .expect("routing query should succeed for missing task-dispatch-lineage fixture");
    let operator = run_workflow_operator_json(
        repo,
        state,
        PLAN_REL,
        true,
        "workflow operator json for missing task-dispatch-lineage fixture",
    );

    assert_eq!(routing.phase_detail, "task_closure_recording_ready");
    assert_eq!(routing.next_action, "close current task");
    assert!(
        routing.recommended_command.as_deref().is_some_and(
            |command| command.contains("featureforge plan execution close-current-task --plan")
        ),
        "external-review-ready routing without dispatch lineage should keep a runnable close-current-task command",
    );
    assert!(
        routing
            .blocking_reason_codes
            .iter()
            .any(|code| code == "prior_task_current_closure_missing"),
        "task-closure-recording-ready routing should preserve the task-boundary closure-missing reason code",
    );
    assert!(
        routing
            .recommended_command
            .as_deref()
            .is_some_and(|command| !command.contains("record-review-dispatch")),
        "external-review-ready task-boundary routing should not require hidden dispatch helpers",
    );
    assert_routing_parity_with_operator_json(&routing, &operator);
}
