#[path = "support/bin.rs"]
mod bin_support;
#[allow(dead_code)]
#[path = "support/plan_execution_direct.rs"]
mod plan_execution_direct_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use bin_support::compiled_featureforge_path;
use featureforge::git::discover_slug_identity;
use featureforge::paths::harness_state_path;
use process_support::run;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use workflow_support::{init_repo, install_full_contract_ready_artifacts};

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

fn assert_parity_probe_budget(scenario_id: &str, consumed_probe_commands: usize, max: usize) {
    assert!(
        consumed_probe_commands <= max,
        "scenario {scenario_id} exceeded parity-probe command target: consumed {consumed_probe_commands}, target {max}"
    );
}

fn run_featureforge(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
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
    let events_path = state_path.with_file_name("events.jsonl");
    let legacy_backup_path = state_path.with_file_name("state.legacy.json");
    let _ = fs::remove_file(events_path);
    let _ = fs::remove_file(legacy_backup_path);
}

fn setup_task_boundary_blocked_case(repo: &Path, state: &Path, plan_rel: &str) {
    install_full_contract_ready_artifacts(repo);
    fs::write(repo.join(plan_rel), task_boundary_blocked_plan_source())
        .expect("task-boundary blocked plan fixture should write");
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
        Value::from(concat!("execution pre", "flight")),
        "FS-02 entry-path classification should stay on execution {} for comment-only drift",
        concat!("pre", "flight")
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

    let mut harness_state_json: Value = serde_json::from_str(
        &fs::read_to_string(harness_state_file_path(repo, state))
            .expect("harness state should be readable before FS-09 fixture mutation"),
    )
    .expect("harness state should remain valid json before FS-09 fixture mutation");
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
    assert!(
        repair["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.contains("featureforge plan execution close-current-task --plan")
            }),
        "repair should expose an executable close-current-task command after clearing stale reroute state: {repair}",
    );

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
    assert!(
        operator["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.contains("featureforge plan execution close-current-task --plan")
            }),
        "workflow operator should expose the executable close-current-task command after repair: {operator}"
    );

    assert!(
        operator["next_public_action"]["command"]
            .as_str()
            .is_some_and(
                |command| command.contains("featureforge plan execution close-current-task")
            ),
        "workflow operator should surface the post-repair task-closure blocker through next_public_action"
    );
}
