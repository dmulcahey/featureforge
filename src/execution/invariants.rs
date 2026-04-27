use std::collections::BTreeSet;

use crate::execution::reentry_reconcile::{
    TARGETLESS_STALE_RECONCILE_REASON_CODE, TargetlessStaleReconcile,
};
use crate::execution::state::PlanExecutionStatus;
use crate::execution::{
    command_eligibility::{command_invokes_hidden_lane, command_is_legal_public_command},
    router::router_allows_public_recommended_mutation,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeInvariantSeverity {
    RuntimeBug,
    ReconcileRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInvariantViolation {
    pub code: &'static str,
    pub severity: RuntimeInvariantSeverity,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvariantEnforcementMode {
    ReadSurface,
    PostMutation,
}

pub fn check_runtime_status_invariants(
    status: &PlanExecutionStatus,
    mode: InvariantEnforcementMode,
) -> Vec<RuntimeInvariantViolation> {
    let mut violations = Vec::new();
    check_current_and_stale_closures_are_disjoint(status, &mut violations);
    check_execution_reentry_has_concrete_target(status, &mut violations);
    check_execution_command_context_kind(status, &mut violations);
    check_public_commands(status, mode, &mut violations);
    check_targetless_stale_unreviewed_routes_to_reconcile(status, &mut violations);
    check_terminal_states_do_not_recommend_mutations(status, &mut violations);
    check_waiting_external_input_does_not_recommend_local_mutation(status, &mut violations);
    check_recommended_command_matches_mutation_eligibility(status, &mut violations);
    violations
}

pub fn apply_read_surface_invariants(status: &mut PlanExecutionStatus) {
    let violations = check_runtime_status_invariants(status, InvariantEnforcementMode::ReadSurface);
    if violations.is_empty() {
        return;
    }
    convert_status_to_runtime_reconcile_or_bug(status, &violations);
}

pub(crate) fn inject_read_surface_invariant_test_violation(
    status: &mut PlanExecutionStatus,
) -> bool {
    inject_status_invariant_test_violation_from_env(
        status,
        "FEATUREFORGE_PLAN_EXECUTION_READ_INVARIANT_TEST_INJECTION",
    )
}

pub(crate) fn inject_post_mutation_invariant_test_violation(
    status: &mut PlanExecutionStatus,
) -> bool {
    inject_status_invariant_test_violation_from_env(
        status,
        "FEATUREFORGE_PLAN_EXECUTION_POST_MUTATION_INVARIANT_TEST_INJECTION",
    )
}

fn inject_status_invariant_test_violation_from_env(
    status: &mut PlanExecutionStatus,
    env_key: &str,
) -> bool {
    let Ok(injection) = std::env::var(env_key) else {
        return false;
    };
    match injection.as_str() {
        "current_stale_overlap" => inject_current_stale_overlap(status),
        "hidden_recommended_command" => {
            status.recommended_command = Some(String::from(
                "featureforge plan execution gate-review --plan injected",
            ));
        }
        "rejected_recommended_command" => {
            status.recommended_command = Some(String::from(
                "featureforge plan execution begin --plan injected --task 999 --step 1 --expect-execution-fingerprint injected",
            ));
        }
        _ => return false,
    }
    true
}

pub fn read_surface_invariant_projection_active(status: &PlanExecutionStatus) -> bool {
    status.state_kind == "blocked_runtime_bug"
        || status.phase_detail == "blocked_runtime_bug"
        || status
            .reason_codes
            .iter()
            .chain(status.blocking_reason_codes.iter())
            .any(|code| RUNTIME_INVARIANT_CODES.contains(&code.as_str()))
}

const RUNTIME_INVARIANT_CODES: &[&str] = &[
    "current_stale_closure_overlap",
    "execution_reentry_target_missing",
    "illegal_execution_command_context",
    "recommended_command_hidden_or_debug",
    "recommended_command_illegal_public_shape",
    "next_public_action_hidden_or_debug",
    "next_public_action_illegal_public_shape",
    "recommended_command_next_action_mismatch",
    TARGETLESS_STALE_RECONCILE_REASON_CODE,
    "terminal_recommended_command",
    "waiting_external_input_local_mutation",
    "recommended_mutation_command_rejected",
];

fn check_current_and_stale_closures_are_disjoint(
    status: &PlanExecutionStatus,
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    let current_ids = status
        .current_task_closures
        .iter()
        .map(|closure| closure.closure_record_id.as_str())
        .collect::<BTreeSet<_>>();
    let overlapping_ids = status
        .stale_unreviewed_closures
        .iter()
        .filter(|closure_id| current_ids.contains(closure_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if overlapping_ids.is_empty() {
        return;
    }
    violations.push(RuntimeInvariantViolation {
        code: "current_stale_closure_overlap",
        severity: RuntimeInvariantSeverity::RuntimeBug,
        detail: format!(
            "current and stale task-closure sets must be disjoint. overlapping_ids={overlapping_ids:?}"
        ),
    });
}

fn check_execution_reentry_has_concrete_target(
    status: &PlanExecutionStatus,
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    if status.phase_detail != "execution_reentry_required"
        || status.state_kind == "blocked_runtime_bug"
        || status.phase_detail == "runtime_reconcile_required"
        || !status_exposes_public_execution_mutation(status)
    {
        return;
    }
    if execution_reentry_has_concrete_target(status) {
        return;
    }
    violations.push(RuntimeInvariantViolation {
        code: "execution_reentry_target_missing",
        severity: RuntimeInvariantSeverity::ReconcileRequired,
        detail: String::from(
            "execution_reentry_required must include a concrete execution command target.",
        ),
    });
}

fn check_execution_command_context_kind(
    status: &PlanExecutionStatus,
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    let Some(context) = status.execution_command_context.as_ref() else {
        return;
    };
    if execution_command_kind_is_legal_public_token(&context.command_kind) {
        return;
    }
    violations.push(RuntimeInvariantViolation {
        code: "illegal_execution_command_context",
        severity: RuntimeInvariantSeverity::RuntimeBug,
        detail: format!(
            "execution command context command_kind `{}` is not a legal public execution token.",
            context.command_kind
        ),
    });
}

fn check_public_commands(
    status: &PlanExecutionStatus,
    mode: InvariantEnforcementMode,
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    if let Some(recommended_command) = status.recommended_command.as_deref() {
        check_public_command_shape(
            "recommended command",
            recommended_command,
            "recommended_command_hidden_or_debug",
            "recommended_command_illegal_public_shape",
            violations,
        );
    }
    if let Some(next_public_action) = status.next_public_action.as_ref() {
        check_public_command_shape(
            "next public action",
            next_public_action.command.as_str(),
            "next_public_action_hidden_or_debug",
            "next_public_action_illegal_public_shape",
            violations,
        );
    }
    if matches!(mode, InvariantEnforcementMode::PostMutation)
        && let (Some(recommended_command), Some(next_public_action)) = (
            status.recommended_command.as_deref(),
            status.next_public_action.as_ref(),
        )
        && recommended_command != next_public_action.command
    {
        violations.push(RuntimeInvariantViolation {
            code: "recommended_command_next_action_mismatch",
            severity: RuntimeInvariantSeverity::RuntimeBug,
            detail: format!(
                "recommended command `{recommended_command}` must match router next public action `{}`.",
                next_public_action.command
            ),
        });
    }
}

fn check_public_command_shape(
    label: &str,
    command: &str,
    hidden_code: &'static str,
    illegal_code: &'static str,
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    if command_invokes_hidden_lane(command) {
        violations.push(RuntimeInvariantViolation {
            code: hidden_code,
            severity: RuntimeInvariantSeverity::RuntimeBug,
            detail: format!("{label} must not expose hidden/debug command `{command}`."),
        });
        return;
    }
    if !command_is_legal_public_command(command) {
        violations.push(RuntimeInvariantViolation {
            code: illegal_code,
            severity: RuntimeInvariantSeverity::RuntimeBug,
            detail: format!("{label} must be one legal public command shape, got `{command}`."),
        });
    }
}

fn check_targetless_stale_unreviewed_routes_to_reconcile(
    status: &PlanExecutionStatus,
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    if !TargetlessStaleReconcile::status_needs_marker_for_status(status)
        || targetless_stale_reconcile_diagnostic(status)
    {
        return;
    }
    violations.push(RuntimeInvariantViolation {
        code: TARGETLESS_STALE_RECONCILE_REASON_CODE,
        severity: RuntimeInvariantSeverity::ReconcileRequired,
        detail: String::from("stale_unreviewed state must include concrete stale targets."),
    });
}

fn check_terminal_states_do_not_recommend_mutations(
    status: &PlanExecutionStatus,
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    if status.state_kind != "terminal" || status.recommended_command.is_none() {
        return;
    }
    violations.push(RuntimeInvariantViolation {
        code: "terminal_recommended_command",
        severity: RuntimeInvariantSeverity::RuntimeBug,
        detail: String::from("terminal states must not emit a recommended command."),
    });
}

fn check_waiting_external_input_does_not_recommend_local_mutation(
    status: &PlanExecutionStatus,
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    if status.state_kind != "waiting_external_input" {
        return;
    }
    let Some(command) = status.recommended_command.as_deref() else {
        return;
    };
    if !command_recommends_local_mutation(command)
        || command_is_external_acknowledgment_path(command)
    {
        return;
    }
    violations.push(RuntimeInvariantViolation {
        code: "waiting_external_input_local_mutation",
        severity: RuntimeInvariantSeverity::RuntimeBug,
        detail: format!(
            "waiting_external_input states must not recommend local mutation command `{command}`."
        ),
    });
}

fn check_recommended_command_matches_mutation_eligibility(
    status: &PlanExecutionStatus,
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    let Some(command) = status.recommended_command.as_deref() else {
        return;
    };
    if command_invokes_hidden_lane(command) || !command_is_legal_public_command(command) {
        return;
    }
    if command_recommends_local_mutation(command)
        && !router_allows_public_recommended_mutation(status, command)
    {
        violations.push(RuntimeInvariantViolation {
            code: "recommended_mutation_command_rejected",
            severity: RuntimeInvariantSeverity::RuntimeBug,
            detail: format!(
                "recommended command `{command}` is not accepted by the mutation eligibility oracle."
            ),
        });
    }
}

fn convert_status_to_runtime_reconcile_or_bug(
    status: &mut PlanExecutionStatus,
    violations: &[RuntimeInvariantViolation],
) {
    let targetless_stale_reconcile = violations
        .iter()
        .any(|violation| TargetlessStaleReconcile::from_reason_code(violation.code).is_some());
    let has_runtime_bug = violations
        .iter()
        .any(|violation| violation.severity == RuntimeInvariantSeverity::RuntimeBug);
    if has_runtime_bug {
        status.phase = Some(String::from("blocked"));
        status.phase_detail = String::from("blocked_runtime_bug");
        status.state_kind = String::from("blocked_runtime_bug");
    } else {
        status.phase_detail = String::from("runtime_reconcile_required");
    }
    status.next_action = String::from("repair review state / reenter execution");
    status.recommended_command = None;
    status.execution_command_context = None;
    status.execution_reentry_target_source = None;
    status.public_repair_targets.clear();
    status.next_public_action = None;
    status.blockers.clear();
    for violation in violations {
        if TargetlessStaleReconcile::from_reason_code(violation.code).is_some() {
            TargetlessStaleReconcile::ensure_status_diagnostic(status);
        } else {
            push_code_once(&mut status.reason_codes, violation.code);
            push_code_once(&mut status.blocking_reason_codes, violation.code);
        }
    }
    if targetless_stale_reconcile {
        status.blocking_records = TargetlessStaleReconcile::status_blocking_record(status)
            .into_iter()
            .collect();
    }
}

fn push_code_once(codes: &mut Vec<String>, code: &str) {
    if codes.iter().any(|existing| existing == code) {
        return;
    }
    codes.push(code.to_owned());
}

fn inject_current_stale_overlap(status: &mut PlanExecutionStatus) {
    let Some(current) = status.current_task_closures.first() else {
        return;
    };
    if !status
        .stale_unreviewed_closures
        .iter()
        .any(|closure_id| closure_id == &current.closure_record_id)
    {
        status
            .stale_unreviewed_closures
            .push(current.closure_record_id.clone());
    }
    status.review_state_status = String::from("stale_unreviewed");
    status.phase_detail = String::from("execution_reentry_required");
    status.recommended_command = Some(format!(
        "featureforge plan execution reopen --plan injected --task {} --step 1 --source featureforge:executing-plans --reason injected",
        current.task
    ));
}

fn targetless_stale_reconcile_diagnostic(status: &PlanExecutionStatus) -> bool {
    TargetlessStaleReconcile::status_has_diagnostic(status)
}

fn execution_command_kind_is_legal_public_token(command_kind: &str) -> bool {
    matches!(command_kind, "begin" | "complete" | "reopen")
}

fn execution_reentry_has_concrete_target(status: &PlanExecutionStatus) -> bool {
    status
        .execution_command_context
        .as_ref()
        .is_some_and(|context| {
            context.task_number.is_some()
                && match context.command_kind.as_str() {
                    "begin" | "complete" | "reopen" => context.step_id.is_some(),
                    _ => false,
                }
        })
}

fn status_exposes_public_execution_mutation(status: &PlanExecutionStatus) -> bool {
    status
        .recommended_command
        .as_deref()
        .is_some_and(command_recommends_execution_mutation)
        || status
            .next_public_action
            .as_ref()
            .is_some_and(|action| command_recommends_execution_mutation(&action.command))
}

fn command_recommends_execution_mutation(command: &str) -> bool {
    command.starts_with("featureforge plan execution begin --plan ")
        || command.starts_with("featureforge plan execution complete --plan ")
        || command.starts_with("featureforge plan execution reopen --plan ")
}

fn command_recommends_local_mutation(command: &str) -> bool {
    command.starts_with("featureforge plan execution begin --plan ")
        || command.starts_with("featureforge plan execution complete --plan ")
        || command.starts_with("featureforge plan execution reopen --plan ")
        || command.starts_with("featureforge plan execution transfer --plan ")
        || command.starts_with("featureforge plan execution close-current-task --plan ")
        || command.starts_with("featureforge plan execution advance-late-stage --plan ")
        || command.starts_with("featureforge plan execution repair-review-state --plan ")
}

fn command_is_external_acknowledgment_path(command: &str) -> bool {
    command.starts_with("featureforge workflow operator --plan ")
        && command.contains("--external-review-result-ready")
}
