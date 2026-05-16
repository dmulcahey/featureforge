use crate::execution::closure_diagnostics::{
    TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING,
    TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE, public_task_boundary_decision,
    task_boundary_projection_diagnostic_reason_code,
};
use crate::execution::command_eligibility::PublicCommandKind;
use crate::execution::phase;
use crate::execution::query::{
    ExecutionRoutingExecutionCommandContext, compact_operator_reason_codes,
};
use crate::execution::reentry_reconcile::TargetlessStaleReconcile;
use crate::execution::repair_route_decision::status_has_current_task_closure_for_task;
use crate::execution::review_route_tokens::REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY;
use crate::execution::state::PlanExecutionStatus;

pub(crate) fn command_context_reopens_current_task_closure(
    status: &PlanExecutionStatus,
    context: Option<&ExecutionRoutingExecutionCommandContext>,
) -> bool {
    if status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY)
    {
        return false;
    }
    let Some(context) = context else {
        return false;
    };
    if !PublicCommandKind::Reopen.matches_public_mutation_token(&context.command_kind) {
        return false;
    }
    let Some(task_number) = context.task_number else {
        return false;
    };
    status_has_current_task_closure_for_task(status, task_number)
}

pub(crate) fn public_route_blocking_reason_codes(
    status: &PlanExecutionStatus,
    phase_detail: &str,
    blocking_task: Option<u32>,
    candidate_blocking_reason_codes: &[String],
) -> Vec<String> {
    if blocking_task.is_some()
        && status.blocking_step.is_none()
        && matches!(
            phase_detail,
            phase::DETAIL_TASK_CLOSURE_RECORDING_READY
                | phase::DETAIL_TASK_REVIEW_RESULT_PENDING
                | phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        )
    {
        return candidate_blocking_reason_codes
            .iter()
            .filter(|reason_code| {
                !task_boundary_projection_diagnostic_reason_code(reason_code)
                    && reason_code.as_str() != phase::DETAIL_TASK_REVIEW_DISPATCH_REQUIRED
            })
            .cloned()
            .collect();
    }
    if status.blocking_task.is_some()
        && status.blocking_step.is_none()
        && phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
    {
        return public_task_boundary_decision(status).public_reason_codes;
    }
    candidate_blocking_reason_codes.to_vec()
}

pub(crate) fn targetless_stale_reconcile_for_phase(
    phase_detail: &str,
    reason_codes: &[String],
) -> bool {
    TargetlessStaleReconcile::from_phase_and_reason_codes(phase_detail, reason_codes).is_some()
}

pub(crate) fn task_closure_recording_reentry_target_source(
    phase_detail: &str,
    blocking_reason_codes: &[String],
) -> Option<String> {
    (phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && blocking_reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
                    | TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE
            )
        }))
    .then(|| String::from("baseline_bridge"))
}

pub(crate) fn compact_route_reason_codes(
    status: &PlanExecutionStatus,
    phase_detail: &str,
    review_state_status: &str,
    blocking_task: Option<u32>,
    blocking_step: Option<u32>,
) -> Vec<String> {
    let mut projected_status = status.clone();
    if blocking_task.is_some() {
        projected_status.blocking_task = blocking_task;
        projected_status.blocking_step = blocking_step;
    }
    compact_operator_reason_codes(Some(&projected_status), phase_detail, review_state_status)
}

pub(crate) fn merge_reason_codes(mut primary: Vec<String>, secondary: Vec<String>) -> Vec<String> {
    for code in secondary {
        if !primary.iter().any(|existing| existing == &code) {
            primary.push(code);
        }
    }
    primary
}
