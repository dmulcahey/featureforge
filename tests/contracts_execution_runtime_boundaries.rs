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

use featureforge::execution::query::{
    ExecutionRoutingState, query_workflow_routing_state_for_runtime,
};
use featureforge::paths::harness_state_path;
use runtime_support::execution_runtime;
use serde_json::{Value, json};
use tempfile::TempDir;
use workflow_support::{init_repo, install_full_contract_ready_artifacts};

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

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
    let closure_dispatch_source =
        fs::read_to_string(repo_root().join("src/execution/closure_dispatch.rs"))
            .expect("execution closure dispatch source should be readable");
    let review_gate_source =
        fs::read_to_string(repo_root().join("src/execution/state/review_gate.rs"))
            .expect("execution review gate source should be readable");
    let dispatch_start = closure_dispatch_source
        .find("fn record_review_dispatch_strategy_checkpoint(")
        .expect("closure_dispatch.rs should keep record_review_dispatch_strategy_checkpoint");
    let dispatch_end = closure_dispatch_source[dispatch_start..]
        .find("pub(crate) fn existing_task_dispatch_reviewed_state_status(")
        .map(|offset| dispatch_start + offset)
        .expect("closure_dispatch.rs should keep existing_task_dispatch_reviewed_state_status");
    let checkpoint_start = review_gate_source
        .find("fn persist_finish_review_gate_pass_checkpoint(")
        .expect("review_gate.rs should keep persist_finish_review_gate_pass_checkpoint");
    let checkpoint_end = review_gate_source[checkpoint_start..]
        .find("fn gate_review_from_context_internal(")
        .map(|offset| checkpoint_start + offset)
        .expect("review_gate.rs should keep gate_review_from_context_internal");
    let dispatch_source = &closure_dispatch_source[dispatch_start..dispatch_end];
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
    assert!(
        routing.recommended_command.is_none(),
        "FS-04 shared-action contract should omit executable command text until review/verification inputs are supplied"
    );
    assert_task_closure_required_inputs(&operator, 1);
    assert_routing_parity_with_operator_json(&routing, &operator);

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
        .find("struct SpecificGateRecommendation")
        .map(|offset| explicit_start + offset)
        .expect("runtime_methods.rs should render direct gate surfaces through SpecificGateRecommendation");
    let explicit_source = &runtime_methods_source[explicit_start..explicit_end];
    assert!(
        !explicit_source.contains("reason_code_indicates_stale_unreviewed"),
        "gate follow-up compatibility fallback must not re-derive branch-closure routing from stale_unreviewed reason-code heuristics",
    );
    assert!(
        !explicit_source.contains("current_branch_closure_id_missing"),
        "gate follow-up compatibility fallback must not hardcode current_branch_closure_id_missing into a direct branch-closure recommendation",
    );
    let direct_recommendation_start = runtime_methods_source
        .find("fn specific_gate_direct_recommendation(")
        .expect("runtime_methods.rs should keep specific_gate_direct_recommendation");
    let direct_recommendation_end = runtime_methods_source[direct_recommendation_start..]
        .find("fn set_gate_public_command(")
        .map(|offset| direct_recommendation_start + offset)
        .expect("runtime_methods.rs should keep set_gate_public_command");
    let direct_recommendation_source =
        &runtime_methods_source[direct_recommendation_start..direct_recommendation_end];
    assert!(
        direct_recommendation_source.contains("SpecificGateRecommendation::from_route_decision")
            && !direct_recommendation_source.contains("materialized_follow_up_kind_command"),
        "gate follow-up output must preserve router-owned route surfaces instead of synthesizing fallback commands"
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
        assert_task_closure_required_inputs(&operator, 1);
        assert_task_closure_required_inputs(&repair, 1);
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
        "FS-04 compiled-cli repair output should agree with operator on missing-input command absence"
    );
    assert_eq!(
        operator_direct["recommended_command"], repair_direct["recommended_command"],
        "FS-04 direct repair output should agree with operator on missing-input command absence"
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
            "FS-04 {label} repair and operator should agree on missing-input command absence"
        );
        assert_task_closure_required_inputs(&operator, 1);
        assert_task_closure_required_inputs(&routed, 1);
        results.push((label, routed));
    }

    assert_eq!(results[0].1["action"], results[1].1["action"]);
    assert_eq!(
        results[0].1["required_follow_up"],
        results[1].1["required_follow_up"]
    );
    assert_eq!(
        results[0].1["recommended_command"], results[1].1["recommended_command"],
        "FS-04 direct and compiled-cli routes should stay aligned on missing-input command absence"
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
    assert_task_closure_required_inputs(&operator_real, 1);
    assert_task_closure_required_inputs(&operator_direct, 1);

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
    assert_task_closure_required_inputs(&status_real, 1);
    assert_task_closure_required_inputs(&status_direct, 1);
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
            assert!(
                !command.contains("<approved-plan-path>"),
                "next_public_action command is route authority and must bind the concrete plan path: {command}"
            );
            if let Some(args_template) = next_public_action
                .get("args_template")
                .and_then(Value::as_str)
            {
                assert!(
                    !args_template.contains("<approved-plan-path>"),
                    "next_public_action args_template is route authority and must bind the concrete plan path: {args_template}"
                );
            }
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
