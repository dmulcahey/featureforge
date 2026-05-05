#[path = "support/bin.rs"]
mod bin_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/public_featureforge_cli.rs"]
mod public_featureforge_cli;
#[path = "support/workflow.rs"]
mod workflow_support;

use bin_support::compiled_featureforge_path;
use featureforge::git::discover_slug_identity;
use featureforge::paths::harness_state_path;
use process_support::{assert_workspace_runtime_uses_temp_state, run};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use workflow_support::{
    init_repo, install_full_contract_ready_artifacts,
    write_current_pass_plan_fidelity_review_artifact_for_plan,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicRouteSnapshot {
    phase: String,
    phase_detail: Option<String>,
    review_state_status: String,
    next_action: String,
    recommended_command: Option<String>,
    blocking_task: Option<u32>,
    blocking_scope: Option<String>,
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
        blocking_task: value
            .get("blocking_task")
            .and_then(Value::as_u64)
            .and_then(|raw| u32::try_from(raw).ok()),
        blocking_scope: value
            .get("blocking_scope")
            .and_then(Value::as_str)
            .map(str::to_owned),
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

fn assert_doctor_resolution(
    doctor: &Value,
    expected_kind: &str,
    expected_command_available: bool,
    expected_stop_reasons: &[&str],
) {
    let resolution = doctor
        .get("resolution")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("workflow doctor should expose resolution object: {doctor}"));
    assert_eq!(
        resolution.get("kind").and_then(Value::as_str),
        Some(expected_kind),
        "workflow doctor resolution kind should be deterministic: {doctor}"
    );
    assert_eq!(
        resolution.get("command_available").and_then(Value::as_bool),
        Some(expected_command_available),
        "workflow doctor resolution command availability should match argv presence: {doctor}"
    );
    let stop_reasons = resolution
        .get("stop_reasons")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!("workflow doctor should expose resolution stop_reasons: {doctor}")
        })
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("stop_reasons entries should be strings: {doctor}"))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        stop_reasons, expected_stop_reasons,
        "workflow doctor resolution stop reasons should preserve canonical order: {doctor}"
    );
}

fn assert_doctor_json_does_not_route_to_recover(doctor: &Value, context: &str) {
    assert!(
        !value_contains_recover_route(doctor),
        "{context} workflow doctor JSON must not route to nonexistent `plan execution recover`: {doctor}"
    );
}

fn assert_doctor_text_does_not_route_to_recover(text: &str, context: &str) {
    assert!(
        !text.contains("plan execution recover"),
        "{context} workflow doctor text must not route to nonexistent `plan execution recover`:\n{text}"
    );
}

fn value_contains_recover_route(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("plan execution recover"),
        Value::Array(values) => {
            values.iter().take(4).map(Value::as_str).collect::<Vec<_>>()
                == [
                    Some("featureforge"),
                    Some("plan"),
                    Some("execution"),
                    Some("recover"),
                ]
                || values.iter().any(value_contains_recover_route)
        }
        Value::Object(map) => map.values().any(value_contains_recover_route),
        _ => false,
    }
}

fn assert_parity_probe_budget(scenario_id: &str, consumed_probe_commands: usize, max: usize) {
    assert!(
        consumed_probe_commands <= max,
        "scenario {scenario_id} exceeded parity-probe command target: consumed {consumed_probe_commands}, target {max}"
    );
}

fn assert_task_closure_required_inputs(surface: &Value, context: &str) {
    let required_inputs = json!([
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
    ]);
    assert_eq!(
        surface["recommended_command"],
        Value::Null,
        "{context} should not expose a placeholder command: {surface}"
    );
    assert!(
        surface.get("recommended_public_command_argv").is_none(),
        "{context} should not expose machine argv while task-closure input is missing: {surface}"
    );
    assert_eq!(
        surface["required_inputs"], required_inputs,
        "{context} should expose typed task-closure inputs: {surface}"
    );
    if surface.get("next_public_action").is_some() {
        assert_eq!(
            surface["next_public_action"]["command"],
            Value::Null,
            "{context} next_public_action should not expose a placeholder command: {surface}"
        );
        assert_eq!(
            surface["next_public_action"]["required_inputs"], required_inputs,
            "{context} next_public_action should carry typed task-closure inputs: {surface}"
        );
    }
}

fn run_featureforge(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    assert_workspace_runtime_uses_temp_state(Some(repo), Some(state_dir), None, false, context);
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    run(command, context)
}

fn run_featureforge_with_env(
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

fn run_featureforge_json(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Value {
    let output = run_featureforge(repo, state_dir, args, context);
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

fn run_featureforge_json_with_env(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    context: &str,
) -> Value {
    let output = run_featureforge_with_env(repo, state_dir, args, extra_env, context);
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

fn run_plan_execution_json(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Value {
    let mut full_args = Vec::with_capacity(args.len() + 2);
    full_args.extend(["plan", "execution"]);
    full_args.extend_from_slice(args);
    run_featureforge_json(repo, state_dir, &full_args, context)
}

fn harness_state_file_path(repo: &Path, state: &Path) -> std::path::PathBuf {
    let identity = discover_slug_identity(repo);
    harness_state_path(state, &identity.repo_slug, &identity.branch_name)
}

fn read_authoritative_harness_state(repo: &Path, state: &Path, purpose: &str) -> Value {
    let state_path = harness_state_file_path(repo, state);
    featureforge::execution::event_log::load_reduced_authoritative_state_for_tests(&state_path)
        .unwrap_or_else(|error| {
            panic!(
                "event-authoritative workflow-entry harness state should reduce for {purpose} at {}: {}",
                state_path.display(),
                error.message
            )
        })
        .unwrap_or_else(|| {
            serde_json::from_str(
                &fs::read_to_string(&state_path)
                    .unwrap_or_else(|error| panic!("harness state should read for {purpose}: {error}")),
            )
            .unwrap_or_else(|error| panic!("harness state should parse for {purpose}: {error}"))
        })
}

fn write_harness_state_payload(repo: &Path, state: &Path, payload: &Value) {
    let state_path = harness_state_file_path(repo, state);
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent).expect("harness state parent should be creatable");
    }
    fs::write(
        &state_path,
        serde_json::to_string_pretty(payload).expect("harness state should serialize"),
    )
    .expect("harness state should be writable");
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(&state_path, payload)
        .expect("workflow-entry harness fixture should sync typed event authority");
}

fn rust_function_body<'a>(source: &'a str, signature: &str) -> &'a str {
    let signature_start = source
        .find(signature)
        .unwrap_or_else(|| panic!("source should contain function signature `{signature}`"));
    let after_signature = &source[signature_start..];
    let body_open_offset = after_signature
        .find('{')
        .unwrap_or_else(|| panic!("function `{signature}` should contain an opening brace"));
    let body_start = signature_start + body_open_offset;
    let mut depth = 0usize;
    for (offset, character) in source[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth = depth
                    .checked_sub(1)
                    .unwrap_or_else(|| panic!("function `{signature}` has unbalanced braces"));
                if depth == 0 {
                    return &source[body_start + 1..body_start + offset];
                }
            }
            _ => {}
        }
    }
    panic!("function `{signature}` body should close");
}

fn setup_task_boundary_blocked_case(repo: &Path, state: &Path, plan_rel: &str) {
    install_full_contract_ready_artifacts(repo);
    fs::write(repo.join(plan_rel), task_boundary_blocked_plan_source())
        .expect("task-boundary blocked plan fixture should write");
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
    prepare_preflight_acceptance_workspace(repo, "workflow-entry-task-boundary-blocked");

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before task-boundary blocked entry fixture execution",
    );
    let begin_task1_step1 = run_plan_execution_json(
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
        "begin task 1 step 1 for task-boundary blocked entry fixture",
    );
    let complete_task1_step1 = run_plan_execution_json(
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
            "Completed task 1 step 1 for task-boundary blocked entry fixture.",
            "--manual-verify-summary",
            "Verified by entry-shell task-boundary fixture setup.",
            "--file",
            "tests/workflow_entry_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 1 for task-boundary blocked entry fixture",
    );
    let begin_task1_step2 = run_plan_execution_json(
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
        "begin task 1 step 2 for task-boundary blocked entry fixture",
    );
    let _ = run_plan_execution_json(
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
            "Completed task 1 step 2 for task-boundary blocked entry fixture.",
            "--manual-verify-summary",
            "Verified by entry-shell task-boundary fixture setup.",
            "--file",
            "tests/workflow_entry_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 2 for task-boundary blocked entry fixture",
    );
}

fn prepare_preflight_acceptance_workspace(repo: &Path, branch_name: &str) {
    let mut checkout = Command::new("git");
    checkout
        .args(["checkout", "-B", branch_name])
        .current_dir(repo);
    let output = run(
        checkout,
        concat!("git checkout pre", "flight acceptance branch"),
    );
    assert!(
        output.status.success(),
        concat!(
            "pre",
            "flight acceptance branch checkout should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}"
        ),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn task_boundary_blocked_plan_source() -> &'static str {
    r#"# Runtime Integration Hardening Implementation Plan

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
**Goal:** Task 1 execution reaches a boundary gate before Task 2 starts.

**Context:**
- Spec Coverage: REQ-001, REQ-004.

**Constraints:**
- Keep fixture inputs deterministic.

**Done when:**
- Task 1 execution reaches a boundary gate before Task 2 starts.

**Files:**
- Modify: `tests/workflow_entry_shell_smoke.rs`

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
- Modify: `tests/workflow_entry_shell_smoke.rs`

- [ ] **Step 1: Start the follow-on task**
"#
}

#[test]
fn fresh_entry_workflow_operator_routes_directly_without_session_entry_state() {
    let (repo_dir, state_dir) = init_repo("workflow-entry-shell-smoke");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);

    let output = run_featureforge(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md",
            "--json",
        ],
        "workflow operator from fresh entry shell smoke",
    );
    assert!(
        output.status.success(),
        "fresh entry workflow operator should succeed without session-entry state, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("workflow operator should emit valid json: {error}"));
    assert_eq!(json["schema_version"], Value::from(3));
    assert!(
        json["phase"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
        "fresh entry workflow operator should route directly to a concrete phase"
    );
    assert!(
        json.get("outcome").is_none(),
        "fresh entry workflow operator should not surface session-entry outcome fields"
    );
    assert!(
        json.get("decision_source").is_none(),
        "fresh entry workflow operator should not surface session-entry decision metadata"
    );
    assert!(
        !state.join("session-entry").exists(),
        "fresh entry workflow operator should not require or create session-entry state"
    );
}

#[test]
fn fs02_entry_route_surfaces_share_parity_and_budget() {
    let (repo_dir, state_dir) = init_repo("workflow-entry-runtime-remediation-fs02");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    install_full_contract_ready_artifacts(repo);
    let plan_path = repo.join(plan_rel);
    let plan_source = fs::read_to_string(&plan_path)
        .expect("FS-02 fixture plan should be readable before late-stage drift setup");
    fs::write(
        &plan_path,
        format!(
            "{plan_source}\n<!-- FS-02 fixture: repo-owned plan/evidence drift after baseline -->\n"
        ),
    )
    .expect("FS-02 fixture plan should be writable for late-stage drift setup");
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);

    let mut runtime_management_commands = 0usize;
    runtime_management_commands += 1;
    let operator = run_featureforge(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-02 entry operator route parity",
    );
    runtime_management_commands += 1;
    let status = run_featureforge(
        repo,
        state,
        &["plan", "execution", "status", "--plan", plan_rel],
        "FS-02 entry plan execution status parity",
    );
    let operator_json: Value = serde_json::from_slice(&operator.stdout)
        .unwrap_or_else(|error| panic!("workflow operator should emit valid json: {error}"));
    let status_json: Value = serde_json::from_slice(&status.stdout)
        .unwrap_or_else(|error| panic!("plan execution status should emit valid json: {error}"));

    assert_public_route_parity(&operator_json, &status_json, None);
    let phase_detail = operator_json["phase_detail"]
        .as_str()
        .expect("FS-02 operator route should include phase_detail");
    assert_eq!(
        phase_detail,
        concat!("execution_pre", "flight_required"),
        "FS-02 fixture should keep comment-only entry drift on the execution-{} lane under semantic identity routing, got {}",
        concat!("pre", "flight"),
        operator_json
    );
    assert_eq!(
        operator_json["next_action"],
        Value::from("continue execution"),
        "FS-02 entry-path classification should stay on the executable begin lane for comment-only drift"
    );
    assert_parity_probe_budget("FS-02", runtime_management_commands, 2);
}

#[test]
fn fs09_repair_surfaces_post_repair_next_blocker_in_entry_cli() {
    let (repo_dir, state_dir) = init_repo("workflow-entry-runtime-remediation-fs09");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    setup_task_boundary_blocked_case(repo, state, plan_rel);

    let mut harness_state_json = read_authoritative_harness_state(repo, state, "FS-09 mutation");
    harness_state_json["current_task_closure_records"] = serde_json::json!({});
    harness_state_json["strategy_review_dispatch_lineage"] = serde_json::json!({});
    harness_state_json["review_state_repair_follow_up"] = Value::from("execution_reentry");
    write_harness_state_payload(repo, state, &harness_state_json);

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-09 repair-review-state should expose the post-repair task-closure recording blocker",
    );
    assert_eq!(repair["action"], Value::from("blocked"), "json: {repair}");
    assert_eq!(repair["required_follow_up"], Value::Null, "json: {repair}");
    assert_task_closure_required_inputs(&repair, "FS-09 repair after stale reroute cleanup");

    let operator = run_featureforge_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-09 workflow operator should expose task-closure recording as the next blocker after repair",
    );
    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-09 plan execution status should expose task-closure recording as the next blocker after repair",
    );
    assert_public_route_parity(&operator, &status, None);
    assert_eq!(operator["phase"], Value::from("task_closure_pending"));
    assert_eq!(
        operator["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    assert_eq!(operator["next_action"], Value::from("close current task"));
    assert_task_closure_required_inputs(
        &operator,
        "FS-09 workflow operator after stale reroute cleanup",
    );

    let doctor = run_featureforge_json(
        repo,
        state,
        &["workflow", "doctor", "--plan", plan_rel, "--json"],
        "FS-09 workflow doctor should expose task-closure resolution after repair",
    );
    assert_public_route_parity(&operator, &status, Some(&doctor));
    assert_task_closure_required_inputs(
        &doctor,
        "FS-09 workflow doctor after stale reroute cleanup",
    );
    assert_doctor_resolution(&doctor, "actionable_public_command", false, &[]);
    assert_doctor_json_does_not_route_to_recover(&doctor, "FS-09 task-closure recording route");

    let doctor_external_ready = run_featureforge_json(
        repo,
        state,
        &[
            "workflow",
            "doctor",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "FS-09 workflow doctor external-ready task-closure route should not recover",
    );
    assert_eq!(
        doctor_external_ready["phase"],
        Value::from("task_closure_pending"),
        "FS-09 external-ready doctor should remain on the task-closure recording route: {doctor_external_ready}"
    );
    assert_doctor_json_does_not_route_to_recover(
        &doctor_external_ready,
        "FS-09 external-ready task-closure recording route",
    );

    let doctor_external_ready_text = run_featureforge(
        repo,
        state,
        &[
            "workflow",
            "doctor",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-09 workflow doctor external-ready task-closure text should not recover",
    );
    assert!(
        doctor_external_ready_text.status.success(),
        "FS-09 external-ready workflow doctor text should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        doctor_external_ready_text.status,
        String::from_utf8_lossy(&doctor_external_ready_text.stdout),
        String::from_utf8_lossy(&doctor_external_ready_text.stderr)
    );
    assert_doctor_text_does_not_route_to_recover(
        &String::from_utf8_lossy(&doctor_external_ready_text.stdout),
        "FS-09 external-ready task-closure recording route",
    );
}

#[test]
fn fs15_doctor_resolution_marks_non_actionable_runtime_diagnostic_stop_reasons() {
    let (repo_dir, state_dir) = init_repo("workflow-entry-runtime-remediation-fs15");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "workflow-entry-runtime-remediation-fs15");
    let env = [(
        "FEATUREFORGE_PLAN_EXECUTION_READ_INVARIANT_TEST_INJECTION",
        "hidden_recommended_command",
    )];

    let status = run_featureforge_json_with_env(
        repo,
        state,
        &["plan", "execution", "status", "--plan", plan_rel],
        &env,
        "FS-15 plan execution status hidden-command invariant",
    );
    let operator = run_featureforge_json_with_env(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &env,
        "FS-15 workflow operator hidden-command invariant",
    );
    let doctor = run_featureforge_json_with_env(
        repo,
        state,
        &["workflow", "doctor", "--plan", plan_rel, "--json"],
        &env,
        "FS-15 workflow doctor hidden-command invariant",
    );

    assert_public_route_parity(&operator, &status, Some(&doctor));
    for (label, surface) in [
        ("status", &status),
        ("operator", &operator),
        ("doctor", &doctor),
    ] {
        let state_kind = if label == "doctor" {
            &surface["execution_status"]["state_kind"]
        } else {
            &surface["state_kind"]
        };
        assert_eq!(
            state_kind,
            &Value::from("blocked_runtime_bug"),
            "{label} should classify the invariant as a runtime diagnostic: {surface}"
        );
        assert_eq!(
            surface["next_action"],
            Value::from("runtime diagnostic required"),
            "{label} should fail closed on the diagnostic route: {surface}"
        );
        assert!(
            surface["recommended_command"].is_null(),
            "{label} must not expose a display command for diagnostic-only states: {surface}"
        );
        assert!(
            surface
                .get("recommended_public_command_argv")
                .is_none_or(Value::is_null),
            "{label} must not expose executable argv for diagnostic-only states: {surface}"
        );
        assert!(
            surface["required_inputs"]
                .as_array()
                .is_none_or(Vec::is_empty),
            "{label} must not convert diagnostic-only states into input prompts: {surface}"
        );
    }
    assert_eq!(
        doctor["blocking_reason_codes"],
        json!(["recommended_command_hidden_or_debug"]),
        "doctor blocker reason-code array should stay deterministic: {doctor}"
    );
    assert_doctor_resolution(
        &doctor,
        "runtime_diagnostic_required",
        false,
        &["recommended_command_hidden_or_debug"],
    );
    assert_doctor_json_does_not_route_to_recover(&doctor, "FS-15 runtime diagnostic route");

    let doctor_text = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "doctor", "--plan", plan_rel],
        &env,
        "FS-15 workflow doctor text hidden-command invariant",
    );
    assert!(
        doctor_text.status.success(),
        "FS-15 workflow doctor text should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        doctor_text.status,
        String::from_utf8_lossy(&doctor_text.stdout),
        String::from_utf8_lossy(&doctor_text.stderr)
    );
    let text = String::from_utf8_lossy(&doctor_text.stdout);
    assert!(
        text.contains("Resolution kind: runtime_diagnostic_required"),
        "doctor text should expose the runtime diagnostic classification marker, got:\n{text}"
    );
    assert!(
        text.contains("Command available: no"),
        "doctor text should expose the non-actionable command marker, got:\n{text}"
    );
    assert!(
        text.contains("recommended_command_hidden_or_debug - "),
        "doctor text blockers should include canonical stop reason plus action text, got:\n{text}"
    );
    assert_doctor_text_does_not_route_to_recover(&text, "FS-15 runtime diagnostic route");
}

#[test]
fn fs16_doctor_text_sanitizes_control_payload_plan_path_without_mutating_json() {
    let (repo_dir, state_dir) = init_repo("workflow-entry-runtime-remediation-fs16");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let poisoned_plan_rel = "docs/featureforge/plans/doctor-\u{1b}[31mred\u{7}-plan.md";
    install_full_contract_ready_artifacts(repo);
    fs::copy(repo.join(plan_rel), repo.join(poisoned_plan_rel))
        .expect("poisoned plan-path fixture should copy from canonical plan");

    let doctor_json = run_featureforge_json(
        repo,
        state,
        &["workflow", "doctor", "--plan", poisoned_plan_rel, "--json"],
        "FS-16 workflow doctor json for poisoned plan path",
    );
    assert_eq!(
        doctor_json["plan_path"],
        Value::from(poisoned_plan_rel),
        "doctor JSON should preserve authoritative raw plan path: {doctor_json}"
    );
    assert!(
        doctor_json["plan_path"]
            .as_str()
            .is_some_and(|path| path.contains('\u{1b}') && path.contains('\u{7}')),
        "doctor JSON should keep raw control payloads for machine consumers: {doctor_json}"
    );

    let doctor_text = run_featureforge(
        repo,
        state,
        &["workflow", "doctor", "--plan", poisoned_plan_rel],
        "FS-16 workflow doctor text for poisoned plan path",
    );
    assert!(
        doctor_text.status.success(),
        "FS-16 workflow doctor text should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        doctor_text.status,
        String::from_utf8_lossy(&doctor_text.stdout),
        String::from_utf8_lossy(&doctor_text.stderr)
    );
    let text = String::from_utf8_lossy(&doctor_text.stdout);
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{7}'),
        "doctor text should render control payloads inert, got:\n{text}"
    );
    assert!(
        text.contains("doctor-red -plan.md"),
        "doctor text should preserve readable sanitized path semantics, got:\n{text}"
    );
}

#[test]
fn fs17_doctor_public_entrypoints_keep_single_context_build_path() {
    let source_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/workflow/operator.rs");
    let source = fs::read_to_string(&source_path)
        .unwrap_or_else(|error| panic!("operator source should be readable: {error}"));

    let doctor_with_args = rust_function_body(&source, "pub fn doctor_with_args(");
    assert_eq!(
        doctor_with_args.matches("build_context_with_plan(").count(),
        1,
        "doctor_with_args should build one route context for one invocation:\n{doctor_with_args}"
    );
    for forbidden in [
        "operator_with_args(",
        "workflow_operator_requery",
        "render_operator",
        "query_workflow_routing_state(",
    ] {
        assert!(
            !doctor_with_args.contains(forbidden),
            "doctor_with_args must not requery through `{forbidden}`:\n{doctor_with_args}"
        );
    }

    let render_doctor_with_args = rust_function_body(&source, "pub fn render_doctor_with_args(");
    assert_eq!(
        render_doctor_with_args.matches("doctor_with_args(").count(),
        1,
        "render_doctor_with_args should render from one prebuilt doctor DTO:\n{render_doctor_with_args}"
    );
    assert!(
        !render_doctor_with_args.contains("build_context_with_plan"),
        "render_doctor_with_args must not rebuild route context during text emission:\n{render_doctor_with_args}"
    );

    let doctor_for_runtime_with_args =
        rust_function_body(&source, "pub fn doctor_for_runtime_with_args(");
    assert_eq!(
        doctor_for_runtime_with_args
            .matches("build_context_with_plan_for_runtime(")
            .count(),
        1,
        "doctor_for_runtime_with_args should build one runtime route context:\n{doctor_for_runtime_with_args}"
    );

    let doctor_from_context = rust_function_body(&source, "fn doctor_from_context(");
    for forbidden in [
        "build_context",
        "query_workflow_routing_state",
        "operator_with_args(",
        "workflow_operator_requery",
    ] {
        assert!(
            !doctor_from_context.contains(forbidden),
            "doctor_from_context must stay projection-only and not call `{forbidden}`:\n{doctor_from_context}"
        );
    }
}
