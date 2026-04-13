#[path = "support/bin.rs"]
mod bin_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use bin_support::compiled_featureforge_path;
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

#[test]
fn fresh_entry_workflow_status_refresh_routes_directly_without_session_entry_state() {
    let (repo_dir, state_dir) = init_repo("workflow-entry-shell-smoke");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);

    let output = run_featureforge(
        repo,
        state,
        &["workflow", "status", "--refresh"],
        "workflow status refresh from fresh entry shell smoke",
    );
    assert!(
        output.status.success(),
        "fresh entry workflow status should succeed without session-entry state, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("workflow status refresh should emit valid json: {error}"));
    assert_eq!(json["schema_version"], Value::from(3));
    assert!(
        json["status"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
        "fresh entry workflow status should route directly to a concrete workflow status"
    );
    assert!(
        json.get("outcome").is_none(),
        "fresh entry workflow status should not surface session-entry outcome fields"
    );
    assert!(
        json.get("decision_source").is_none(),
        "fresh entry workflow status should not surface session-entry decision metadata"
    );
    assert!(
        !state.join("session-entry").exists(),
        "fresh entry workflow status should not require or create session-entry state"
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
    assert!(
        operator_json["phase_detail"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
        "FS-02 fixture should produce a concrete route after repo-owned plan/evidence drift"
    );
    assert_parity_probe_budget("FS-02", runtime_management_commands, 2);
}
