use std::collections::BTreeSet;

use crate::execution::command_eligibility::{
    PublicCommand, PublicCommandKind, command_invokes_hidden_lane, decide_public_mutation,
    hidden_command_tokens, public_argv_has_template_tokens, recommended_public_command_argv,
    recommended_public_command_template, required_inputs_for_public_command,
};
use crate::execution::current_task_closure_selection::current_task_closure_route_target;
use crate::execution::next_action::{
    NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED, diagnostic_next_action_for_route,
};
use crate::execution::reentry_reconcile::{
    TARGETLESS_STALE_RECONCILE_PHASE_DETAIL, TARGETLESS_STALE_RECONCILE_REASON_CODE,
    TargetlessStaleAuthority, TargetlessStaleReconcile,
};
use crate::execution::route_plan::{
    reopen_public_command_with_reason, state_kind_is_blocked_runtime_bug,
    state_kind_is_external_wait, state_kind_is_terminal,
};
use crate::execution::state::PlanExecutionStatus;

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
    check_runtime_status_invariants_with_targetless_authority(status, mode, None)
}

pub fn check_runtime_status_invariants_with_targetless_authority(
    status: &PlanExecutionStatus,
    mode: InvariantEnforcementMode,
    targetless_stale_authority: Option<TargetlessStaleAuthority>,
) -> Vec<RuntimeInvariantViolation> {
    let mut violations = Vec::new();
    check_current_and_stale_closures_are_disjoint(status, &mut violations);
    check_execution_reentry_has_concrete_target(status, &mut violations);
    check_execution_command_context_kind(status, &mut violations);
    check_public_commands(status, mode, &mut violations);
    check_targetless_stale_unreviewed_routes_to_reconcile(
        status,
        targetless_stale_authority,
        &mut violations,
    );
    check_terminal_states_do_not_recommend_mutations(status, &mut violations);
    check_waiting_external_input_does_not_recommend_local_mutation(status, &mut violations);
    check_recommended_command_matches_mutation_eligibility(status, &mut violations);
    violations
}

pub fn apply_read_surface_invariants(status: &mut PlanExecutionStatus) {
    apply_read_surface_invariants_with_targetless_authority(status, None);
}

pub fn apply_read_surface_invariants_with_targetless_authority(
    status: &mut PlanExecutionStatus,
    targetless_stale_authority: Option<TargetlessStaleAuthority>,
) {
    let violations = check_runtime_status_invariants_with_targetless_authority(
        status,
        InvariantEnforcementMode::ReadSurface,
        targetless_stale_authority,
    );
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
        "targetless_stale_unreviewed" => inject_targetless_stale_unreviewed(status),
        "raw_targetless_stale_unreviewed" => inject_raw_targetless_stale_unreviewed(status),
        "hidden_recommended_command" => {
            let hidden_command = hidden_command_tokens()
                .iter()
                .map(String::as_str)
                .find(|token| token.split('-').eq(["gate", "review"]))
                .unwrap_or("hidden-test-command");
            status.recommended_command = Some(format!(
                "featureforge plan execution {hidden_command} --plan injected"
            ));
        }
        "rejected_recommended_command" => {
            status.recommended_public_command = Some(PublicCommand::Begin {
                plan: String::from("injected"),
                task: 999,
                step: 1,
                execution_mode: None,
                fingerprint: Some(String::from("injected")),
            });
            status.recommended_public_command_argv =
                recommended_public_command_argv(status.recommended_public_command.as_ref());
            status.recommended_public_command_template =
                recommended_public_command_template(status.recommended_public_command.as_ref());
            status.required_inputs =
                required_inputs_for_public_command(status.recommended_public_command.as_ref());
            status.recommended_command = Some(String::from(
                "featureforge plan execution begin --plan injected --task 999 --step 1 --expect-execution-fingerprint injected",
            ));
        }
        _ => return false,
    }
    true
}

pub fn read_surface_invariant_projection_active(status: &PlanExecutionStatus) -> bool {
    state_kind_is_blocked_runtime_bug(&status.state_kind)
        || status.phase_detail == crate::execution::phase::DETAIL_BLOCKED_RUNTIME_BUG
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
    "recommended_public_command_argv_template_tokens",
    "next_public_action_hidden_or_debug",
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
    if status.phase_detail != crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        || state_kind_is_blocked_runtime_bug(&status.state_kind)
        || status.phase_detail == crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
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
            violations,
        );
    }
    if let Some(next_public_action) = status.next_public_action.as_ref() {
        check_public_command_shape(
            "next public action",
            next_public_action.command.as_str(),
            "next_public_action_hidden_or_debug",
            violations,
        );
    }
    if let Some(argv) = status.recommended_public_command_argv.as_ref()
        && public_argv_has_template_tokens(argv)
    {
        violations.push(RuntimeInvariantViolation {
            code: "recommended_public_command_argv_template_tokens",
            severity: RuntimeInvariantSeverity::RuntimeBug,
            detail: format!(
                "recommended_public_command_argv must be executable-or-absent; got {argv:?}."
            ),
        });
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
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    if command_invokes_hidden_lane(command) {
        violations.push(RuntimeInvariantViolation {
            code: hidden_code,
            severity: RuntimeInvariantSeverity::RuntimeBug,
            detail: format!("{label} must not expose hidden/debug command `{command}`."),
        });
    }
}

fn check_targetless_stale_unreviewed_routes_to_reconcile(
    status: &PlanExecutionStatus,
    targetless_stale_authority: Option<TargetlessStaleAuthority>,
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    let Some(authority) = targetless_stale_authority else {
        return;
    };
    if TargetlessStaleReconcile::status_has_diagnostic(status)
        || !TargetlessStaleReconcile::status_needs_marker_for_authority(status, authority)
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
    if !state_kind_is_terminal(&status.state_kind) || status.recommended_command.is_none() {
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
    if !state_kind_is_external_wait(&status.state_kind) {
        return;
    }
    let Some(command) = status.recommended_public_command.as_ref() else {
        return;
    };
    if !public_command_recommends_local_mutation(command)
        || command.kind() == PublicCommandKind::WorkflowOperator
    {
        return;
    }
    let display = command.to_display_command();
    violations.push(RuntimeInvariantViolation {
        code: "waiting_external_input_local_mutation",
        severity: RuntimeInvariantSeverity::RuntimeBug,
        detail: format!(
            "waiting_external_input states must not recommend local mutation command `{display}`."
        ),
    });
}

fn check_recommended_command_matches_mutation_eligibility(
    status: &PlanExecutionStatus,
    violations: &mut Vec<RuntimeInvariantViolation>,
) {
    let Some(command) = status.recommended_public_command.as_ref() else {
        return;
    };
    if !public_command_recommends_local_mutation(command) {
        return;
    }
    let Some(request) = command.to_mutation_request() else {
        return;
    };
    if !decide_public_mutation(status, &request).allowed {
        let display = command.to_display_command();
        violations.push(RuntimeInvariantViolation {
            code: "recommended_mutation_command_rejected",
            severity: RuntimeInvariantSeverity::RuntimeBug,
            detail: format!(
                "recommended command `{display}` is not accepted by the mutation eligibility oracle."
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
        status.phase_detail = String::from(crate::execution::phase::DETAIL_BLOCKED_RUNTIME_BUG);
        status.state_kind = String::from(crate::execution::phase::DETAIL_BLOCKED_RUNTIME_BUG);
    } else {
        status.phase_detail =
            String::from(crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
        status.state_kind =
            String::from(crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
    }
    status.recommended_public_command = None;
    status.recommended_public_command_argv = None;
    status.recommended_public_command_template = None;
    status.required_inputs.clear();
    status.next_action = diagnostic_next_action_for_route(
        &status.state_kind,
        &status.phase_detail,
        status.recommended_public_command_argv.is_some(),
        !status.required_inputs.is_empty(),
    )
    .unwrap_or_else(|| String::from(NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED));
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
    let Some(current) = current_task_closure_route_target(status) else {
        return;
    };
    let Some(closure_record_id) = current.closure_record_id.as_ref() else {
        return;
    };
    if !status
        .stale_unreviewed_closures
        .iter()
        .any(|closure_id| closure_id == closure_record_id)
    {
        status
            .stale_unreviewed_closures
            .push(closure_record_id.clone());
    }
    status.review_state_status =
        String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED);
    status.phase_detail = String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED);
    status.recommended_public_command = Some(reopen_public_command_with_reason(
        "injected",
        current.task,
        1,
        "featureforge:executing-plans",
        "injected",
        None,
    ));
    status.recommended_public_command_argv =
        recommended_public_command_argv(status.recommended_public_command.as_ref());
    status.recommended_public_command_template =
        recommended_public_command_template(status.recommended_public_command.as_ref());
    status.required_inputs =
        required_inputs_for_public_command(status.recommended_public_command.as_ref());
    status.recommended_command = Some(format!(
        "featureforge plan execution reopen --plan injected --task {} --step 1 --source featureforge:executing-plans --reason injected",
        current.task
    ));
}

fn inject_targetless_stale_unreviewed(status: &mut PlanExecutionStatus) {
    inject_raw_targetless_stale_unreviewed(status);
    TargetlessStaleReconcile::ensure_status_diagnostic(status);
    status.blocking_records = TargetlessStaleReconcile::status_blocking_record(status)
        .into_iter()
        .collect();
}

fn inject_raw_targetless_stale_unreviewed(status: &mut PlanExecutionStatus) {
    status.review_state_status =
        String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED);
    status.phase = Some(String::from(crate::execution::phase::PHASE_EXECUTING));
    status.harness_phase = crate::execution::harness::HarnessPhase::Executing;
    status.phase_detail = String::from(TARGETLESS_STALE_RECONCILE_PHASE_DETAIL);
    status.state_kind = String::from(TARGETLESS_STALE_RECONCILE_PHASE_DETAIL);
    status.current_branch_closure_id = None;
    status.finish_review_gate_pass_branch_closure_id = None;
    status.current_final_review_branch_closure_id = None;
    status.current_qa_branch_closure_id = None;
    status.stale_unreviewed_closures.clear();
    status.current_task_closures.clear();
    status.recording_context = None;
    status.execution_command_context = None;
    status.execution_reentry_target_source = None;
    status.public_repair_targets.clear();
    status.recommended_public_command = None;
    status.recommended_public_command_argv = None;
    status.recommended_public_command_template = None;
    status.required_inputs.clear();
    status.next_action = diagnostic_next_action_for_route(
        &status.state_kind,
        &status.phase_detail,
        status.recommended_public_command_argv.is_some(),
        !status.required_inputs.is_empty(),
    )
    .unwrap_or_else(|| String::from(NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED));
    status.recommended_command = None;
    status.next_public_action = None;
    status.blockers.clear();
    status.blocking_scope = None;
    status.blocking_task = None;
    status.blocking_step = None;
    status.external_wait_state = None;
    status
        .reason_codes
        .retain(|code| code != crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE);
    status
        .blocking_reason_codes
        .retain(|code| code != crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE);
}

fn execution_command_kind_is_legal_public_token(command_kind: &str) -> bool {
    PublicCommandKind::from_execution_mutation_name(command_kind).is_some()
}

fn execution_reentry_has_concrete_target(status: &PlanExecutionStatus) -> bool {
    status
        .execution_command_context
        .as_ref()
        .is_some_and(|context| {
            context.task_number.is_some()
                && context.step_id.is_some()
                && PublicCommandKind::from_execution_mutation_name(&context.command_kind).is_some()
        })
}

fn status_exposes_public_execution_mutation(status: &PlanExecutionStatus) -> bool {
    status
        .recommended_public_command
        .as_ref()
        .is_some_and(public_command_recommends_execution_mutation)
}

fn public_command_recommends_execution_mutation(command: &PublicCommand) -> bool {
    command.kind().is_execution_mutation()
}

fn public_command_recommends_local_mutation(command: &PublicCommand) -> bool {
    command.kind().public_mutation_name().is_some()
}
