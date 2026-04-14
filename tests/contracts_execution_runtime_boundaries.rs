#[path = "support/featureforge.rs"]
mod featureforge_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/runtime.rs"]
mod runtime_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use featureforge::execution::query::{
    ExecutionRoutingState, query_workflow_routing_state_for_runtime,
};
use featureforge::paths::harness_state_path;
use runtime_support::execution_runtime;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
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
- Modify: `tests/contracts_execution_runtime_boundaries.rs`

- [ ] **Step 1: Prepare workflow fixture output**
- [ ] **Step 2: Validate workflow fixture output**

## Task 2: Follow-on flow

**Spec Coverage:** VERIFY-001
**Task Outcome:** Task 2 should remain blocked until Task 1 closure requirements are met.
**Plan Constraints:**
- Preserve deterministic task-boundary diagnostics.
**Open Questions:** none

**Files:**
- Modify: `tests/contracts_execution_runtime_boundaries.rs`

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
    let output = featureforge_support::run_rust_featureforge_real_cli(
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
    real_cli: bool,
    context: &str,
) -> Output {
    if real_cli {
        featureforge_support::run_rust_featureforge_real_cli(
            Some(repo),
            Some(state),
            None,
            &[],
            args,
            context,
        )
    } else {
        featureforge_support::run_rust_featureforge(
            Some(repo),
            Some(state),
            None,
            &[],
            args,
            context,
        )
    }
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

fn authoritative_harness_state_path(repo: &Path, state: &Path) -> PathBuf {
    let runtime = execution_runtime(repo, state);
    harness_state_path(state, &runtime.repo_slug, &runtime.branch_name)
}

fn update_authoritative_harness_state(repo: &Path, state: &Path, updates: &[(&str, Value)]) {
    let state_path = authoritative_harness_state_path(repo, state);
    let source = fs::read_to_string(&state_path)
        .expect("authoritative harness state should be readable for fixture mutation");
    let mut payload: Value = serde_json::from_str(&source)
        .expect("authoritative harness state should remain valid json");
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
}

fn prepare_preflight_acceptance_workspace(repo: &Path, branch_name: &str) {
    let mut checkout = Command::new("git");
    checkout
        .args(["checkout", "-B", branch_name])
        .current_dir(repo);
    process_support::run_checked(checkout, "git checkout boundary fixture branch");
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
    assert_eq!(
        operator["follow_up_override"],
        Value::from(routing.follow_up_override.clone())
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
    let preflight = run_plan_execution_json(
        repo,
        state,
        &["preflight", "--plan", PLAN_REL],
        "preflight before boundary active-context begin",
    );
    assert_eq!(preflight["allowed"], Value::Bool(true));
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
fn execution_query_recording_ready_states_surface_required_recording_context_ids() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-recording-context");
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
        "record-review-dispatch for task_closure_recording_ready fixture",
    );
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

    let query_source = fs::read_to_string(repo_root().join("src/execution/query.rs"))
        .expect("execution query source should be readable");

    assert!(
        query_source.contains("String::from(\"task_closure_recording_ready\")")
            && query_source.contains("task_number: Some(task_number)")
            && query_source.contains("dispatch_id: task_review_dispatch_id.clone()"),
        "task_closure_recording_ready should expose task_number and may surface dispatch_id in recording_context",
    );
    assert!(
        query_source.contains("\"release_readiness_recording_ready\"")
            && query_source.contains("\"release_blocker_resolution_required\"")
            && query_source.contains("branch_closure_id: Some(branch_closure_id.clone())"),
        "release-readiness recording-ready states should expose branch_closure_id recording_context ids",
    );
    assert!(
        query_source.contains("String::from(\"final_review_recording_ready\")")
            && query_source.contains("dispatch_id: final_review_dispatch_id.clone()")
            && query_source.contains("branch_closure_id: Some(branch_closure_id.clone())"),
        "final_review_recording_ready should expose branch_closure_id and may surface dispatch_id in the routing constructor",
    );
}

#[test]
fn workflow_direct_and_real_cli_read_surfaces_stay_semantically_aligned() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-workflow-direct-real-cli-alignment");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_execution_in_progress(repo, state);

    for command in [
        ["workflow", "next"],
        ["workflow", "artifacts"],
        ["workflow", "explain"],
    ] {
        let direct = run_featureforge_output(repo, state, &command, false, "direct workflow text");
        let real = run_featureforge_output(repo, state, &command, true, "real-cli workflow text");
        assert!(
            direct.status.success() && real.status.success(),
            "workflow text command should succeed for direct and real-cli paths\ncommand: {:?}\ndirect status: {:?}\nreal status: {:?}\ndirect stderr:\n{}\nreal stderr:\n{}",
            command,
            direct.status,
            real.status,
            String::from_utf8_lossy(&direct.stderr),
            String::from_utf8_lossy(&real.stderr)
        );
        assert_eq!(
            direct.stdout, real.stdout,
            "workflow text command output must stay aligned between direct and real-cli paths for command {:?}",
            command
        );
    }

    for command in [
        ["workflow", "phase", "--json"].as_slice(),
        ["workflow", "doctor", "--json"].as_slice(),
        ["workflow", "handoff", "--json"].as_slice(),
        ["workflow", "operator", "--plan", PLAN_REL, "--json"].as_slice(),
    ] {
        let direct = run_featureforge_json(repo, state, command, false, "direct workflow json");
        let real = run_featureforge_json(repo, state, command, true, "real-cli workflow json");
        assert_eq!(
            direct, real,
            "workflow json command output must stay aligned between direct and real-cli paths for command {:?}",
            command
        );
    }
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
        ["workflow", "next"],
        ["workflow", "artifacts"],
        ["workflow", "explain"],
    ] {
        let direct = run_featureforge_output(
            repo,
            state,
            &command,
            false,
            "direct workflow text blocked fixture",
        );
        let real = run_featureforge_output(
            repo,
            state,
            &command,
            true,
            "real-cli workflow text blocked fixture",
        );
        assert!(
            direct.status.success() && real.status.success(),
            "workflow text command should succeed for blocked fixture direct and real-cli paths\ncommand: {:?}\ndirect status: {:?}\nreal status: {:?}\ndirect stderr:\n{}\nreal stderr:\n{}",
            command,
            direct.status,
            real.status,
            String::from_utf8_lossy(&direct.stderr),
            String::from_utf8_lossy(&real.stderr)
        );
        assert_eq!(
            direct.stdout, real.stdout,
            "workflow text command output must stay aligned between blocked fixture direct and real-cli paths for command {:?}",
            command
        );
    }

    for command in [
        ["workflow", "phase", "--json"].as_slice(),
        ["workflow", "doctor", "--json"].as_slice(),
        ["workflow", "handoff", "--json"].as_slice(),
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
fn workflow_status_summary_failure_stays_aligned_with_real_cli() {
    let repo_dir = TempDir::new().expect("non-repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let direct = run_featureforge_output(
        repo,
        state,
        &["workflow", "status", "--summary"],
        false,
        "direct workflow status summary failure",
    );
    let real = run_featureforge_output(
        repo,
        state,
        &["workflow", "status", "--summary"],
        true,
        "real-cli workflow status summary failure",
    );

    assert!(
        !direct.status.success() && !real.status.success(),
        "workflow status --summary should fail outside a git repo for both direct and real-cli paths",
    );
    assert_eq!(
        direct.stdout, real.stdout,
        "workflow status --summary failure stdout must stay aligned between direct and real-cli paths",
    );
    assert_eq!(
        direct.stderr, real.stderr,
        "workflow status --summary failure stderr must stay aligned between direct and real-cli paths",
    );
}

#[test]
fn mutate_and_review_state_use_recording_boundary_for_transition_writes() {
    let mutate_source = fs::read_to_string(repo_root().join("src/execution/mutate.rs"))
        .expect("execution mutate source should be readable");
    let review_state_source = fs::read_to_string(repo_root().join("src/execution/review_state.rs"))
        .expect("execution review_state source should be readable");

    assert!(
        mutate_source.contains("crate::execution::recording::"),
        "mutate.rs should consume the recording boundary for closure writes",
    );
    assert!(
        mutate_source.contains("crate::execution::command_eligibility::"),
        "mutate.rs should consume shared command eligibility helpers instead of re-deriving follow-up routing",
    );
    assert!(
        mutate_source.contains("query_workflow_routing_state"),
        "mutate.rs should consume the execution-owned routing boundary",
    );
    assert!(
        !mutate_source.contains("crate::workflow::operator"),
        "mutate.rs should not import or call workflow/operator directly",
    );
    for forbidden in [
        "fn blocked_follow_up_for_operator(",
        "fn close_current_task_required_follow_up(",
        "fn late_stage_required_follow_up(",
        "record_task_closure_result(",
        "record_task_closure_negative_result(",
        "remove_current_task_closure_results(",
        "append_superseded_task_closure_ids(",
        "append_superseded_branch_closure_ids(",
        "set_current_branch_closure_id(",
        "record_final_review_result(",
        "record_release_readiness_result(",
        "record_browser_qa_result(",
    ] {
        assert!(
            !mutate_source.contains(forbidden),
            "mutate.rs should not call transition write primitive `{forbidden}` directly",
        );
    }

    assert!(
        review_state_source.contains("crate::execution::recording::"),
        "review_state.rs should consume the recording boundary for overlay restoration",
    );
    assert!(
        !review_state_source.contains("load_authoritative_transition_state("),
        "review_state.rs should not load transition state directly for overlay restoration",
    );
    assert!(
        !review_state_source.contains("set_current_branch_closure_id("),
        "review_state.rs should not call transition write primitives directly",
    );
    assert!(
        !review_state_source.contains("parse_artifact_document("),
        "review_state.rs should not parse rendered artifacts directly when reconciling authoritative state",
    );
}

#[test]
fn explicit_mutation_paths_keep_strict_authoritative_state_validation() {
    let state_source = fs::read_to_string(repo_root().join("src/execution/state.rs"))
        .expect("execution state source should be readable");
    let dispatch_start = state_source
        .find("fn record_review_dispatch_strategy_checkpoint(")
        .expect("state.rs should keep record_review_dispatch_strategy_checkpoint");
    let dispatch_end = state_source[dispatch_start..]
        .find("fn ensure_review_dispatch_authoritative_bootstrap(")
        .map(|offset| dispatch_start + offset)
        .expect("state.rs should keep ensure_review_dispatch_authoritative_bootstrap");
    let checkpoint_start = state_source
        .find("fn persist_finish_review_gate_pass_checkpoint(")
        .expect("state.rs should keep persist_finish_review_gate_pass_checkpoint");
    let checkpoint_end = state_source[checkpoint_start..]
        .find("fn gate_review_from_context_internal(")
        .map(|offset| checkpoint_start + offset)
        .expect("state.rs should keep gate_review_from_context_internal");
    let dispatch_source = &state_source[dispatch_start..dispatch_end];
    let checkpoint_source = &state_source[checkpoint_start..checkpoint_end];

    assert!(
        dispatch_source.contains("load_authoritative_transition_state("),
        "record-review-dispatch mutation should validate authoritative active-contract truth through the strict transition-state loader",
    );
    assert!(
        !dispatch_source.contains("load_authoritative_transition_state_relaxed("),
        "record-review-dispatch mutation must not bypass active-contract validation with the relaxed transition-state loader",
    );
    assert!(
        checkpoint_source.contains("load_authoritative_transition_state("),
        "gate-review checkpoint mutation should validate authoritative active-contract truth through the strict transition-state loader",
    );
    assert!(
        !checkpoint_source.contains("load_authoritative_transition_state_relaxed("),
        "gate-review checkpoint mutation must not bypass active-contract validation with the relaxed transition-state loader",
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
        inventory.contains("tests/contracts_execution_runtime_boundaries.rs"),
        "runtime-remediation inventory should map boundary coverage to tests/contracts_execution_runtime_boundaries.rs"
    );
}

#[test]
fn runtime_remediation_fs03_dispatch_target_acceptance_and_mismatch_stay_aligned_between_direct_and_compiled_cli()
 {
    let command_success = [
        "plan",
        "execution",
        "record-review-dispatch",
        "--plan",
        PLAN_REL,
        "--scope",
        "task",
        "--task",
        "1",
    ];
    let command_mismatch = [
        "plan",
        "execution",
        "record-review-dispatch",
        "--plan",
        PLAN_REL,
        "--scope",
        "task",
        "--task",
        "2",
    ];

    for (label, real_cli) in [("direct", false), ("compiled-cli", true)] {
        let (repo_dir, state_dir) = init_repo(&format!(
            "contracts-boundary-runtime-remediation-fs03-{label}"
        ));
        let repo = repo_dir.path();
        let state = state_dir.path();
        setup_task_boundary_blocked_case(repo, state);

        let baseline = state_tree_digest(state);
        let accepted = run_featureforge_json(
            repo,
            state,
            &command_success,
            real_cli,
            &format!("FS-03 {label} accepted task-boundary dispatch target"),
        );
        assert_eq!(
            accepted["allowed"],
            Value::Bool(true),
            "FS-03 {label} accepted path should remain allowed"
        );
        let digest_after_accept = state_tree_digest(state);
        assert_ne!(
            digest_after_accept, baseline,
            "FS-03 {label} accepted path should record dispatch lineage"
        );

        let rejected = run_featureforge_output(
            repo,
            state,
            &command_mismatch,
            real_cli,
            &format!("FS-03 {label} rejected mismatched task target"),
        );
        assert!(
            !rejected.status.success(),
            "FS-03 {label} mismatched task target should fail before mutation"
        );
        let rejected_json = parse_json_output(&rejected, &format!("FS-03 {label} rejected output"));
        assert_eq!(
            rejected_json
                .get("failure_class")
                .or_else(|| rejected_json.get("error_class"))
                .cloned()
                .unwrap_or(Value::Null),
            Value::from("InvalidCommandInput"),
            "FS-03 {label} mismatched task target should fail with InvalidCommandInput"
        );
        assert_eq!(
            state_tree_digest(state),
            digest_after_accept,
            "FS-03 {label} mismatched task target must not mutate runtime state after failing"
        );
    }
}

#[test]
fn runtime_remediation_fs05_unsupported_field_fails_before_mutation_on_compatibility_aliases() {
    let (repo_dir, state_dir) = init_repo("contracts-boundary-runtime-remediation-fs05");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state);

    let baseline = state_tree_digest(state);
    let command = [
        "plan",
        "execution",
        "record-review-dispatch",
        "--plan",
        PLAN_REL,
        "--scope",
        "final-review",
        "--task",
        "1",
    ];

    for (label, real_cli) in [("direct", false), ("compiled-cli", true)] {
        let output = run_featureforge_output(repo, state, &command, real_cli, label);
        assert!(
            !output.status.success(),
            "FS-05 {label} path should fail unsupported final-review task field request"
        );
        let failure = parse_json_output(&output, &format!("FS-05 {label} failure"));
        assert_eq!(
            failure
                .get("failure_class")
                .or_else(|| failure.get("error_class"))
                .cloned()
                .unwrap_or(Value::Null),
            Value::from("InvalidCommandInput"),
            "FS-05 {label} path should reject unsupported fields before mutation"
        );
        assert_eq!(
            state_tree_digest(state),
            baseline,
            "FS-05 {label} path must not mutate runtime state files on unsupported fields"
        );
    }
}

#[test]
fn runtime_remediation_fs04_repair_route_visibility_stays_aligned_between_direct_and_compiled_cli()
{
    let run_case = |label: &str, real_cli: bool| -> Value {
        let (repo_dir, state_dir) = init_repo(&format!(
            "contracts-boundary-runtime-remediation-fs04-{label}"
        ));
        let repo = repo_dir.path();
        let state = state_dir.path();
        setup_task_boundary_blocked_case(repo, state);
        run_featureforge_json(
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
        )
    };

    let repair_direct = run_case("direct", false);
    let repair_real = run_case("compiled-cli", true);
    assert_eq!(repair_direct["action"], repair_real["action"]);
    assert_eq!(
        repair_direct["required_follow_up"],
        repair_real["required_follow_up"]
    );
    assert_eq!(repair_real["action"], Value::from("blocked"));
    assert!(
        repair_real["required_follow_up"]
            .as_str()
            .is_some_and(|follow_up| matches!(
                follow_up,
                "advance_late_stage" | "execution_reentry" | "request_external_review"
            )),
        "FS-04 repair-review-state should expose one authoritative blocker follow-up"
    );
}

#[test]
fn runtime_remediation_fs08_stale_blocker_visibility_stays_aligned_between_direct_and_compiled_cli()
{
    let (repo_dir, state_dir) = init_repo("contracts-boundary-runtime-remediation-fs08");
    let repo = repo_dir.path();
    let state = state_dir.path();
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
        Value::from("task_review_dispatch_required"),
        "FS-08 stale blocker should remain visible as task_review_dispatch_required"
    );
    assert_eq!(operator_real["blocking_scope"], Value::from("task"));
    assert_eq!(operator_real["blocking_task"], Value::from(1_u64));
    assert!(
        operator_real["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| {
                codes
                    .iter()
                    .any(|code| code == "prior_task_review_dispatch_missing")
            }),
        "FS-08 stale blocker should preserve prior_task_review_dispatch_missing reason code"
    );
}
