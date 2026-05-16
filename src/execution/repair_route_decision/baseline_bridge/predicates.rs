//! Private predicate helpers for task-closure baseline bridge routing.
//!
//! These helpers are intentionally scoped under `baseline_bridge`: they support
//! one route family and should not become a second public decision surface.

use super::precedence::baseline_bridge_authority_precedence;
use crate::execution::command_eligibility::PublicCommandKind;
use crate::execution::harness::HarnessPhase;
use crate::execution::repair_target_selection::{
    NextActionAuthorityInputs, missing_current_closure_allows_task_closure_baseline_route,
};
use crate::execution::stale_target_projection::{
    AuthoritativeStaleTarget, authoritative_stale_target_allows_task_closure_bridge,
};
use crate::execution::state::{ExecutionContext, PlanExecutionStatus, PublicRepairTarget};
use crate::execution::status_support::task_closure_baseline_repair_candidate_with_stale_target_and_authority;

pub(super) fn task_closure_baseline_bridge_allows_task_review_pending_route_impl(
    status: &PlanExecutionStatus,
) -> bool {
    closure_baseline_routing_reason_codes_compatible(status)
        && baseline_bridge_reason_present(status)
}

pub(super) fn task_closure_baseline_bridge_blocking_task_route_ready(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
    task_number: u32,
) -> bool {
    status.blocking_step.is_none_or(|_| {
        task_closure_baseline_bridge_allows_blocking_step_route_impl(status, task_number)
    }) && task_closure_recording_harness(status)
        && task_closure_baseline_bridge_allows_stale_provenance_route_impl(
            context,
            status,
            authority_inputs,
            review_state_status,
        )
        && (review_state_status
            != crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
            || super::task_closure_baseline_bridge_ready_for_task_impl(
                context,
                status,
                authority_inputs,
                review_state_status,
                task_number,
            ))
}

pub(super) fn task_closure_baseline_bridge_allows_stale_provenance_route_impl(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
) -> bool {
    missing_current_closure_allows_task_closure_baseline_route(
        context,
        status,
        authority_inputs,
        review_state_status,
    )
}

pub(super) fn task_closure_baseline_bridge_allows_blocking_step_route_impl(
    status: &PlanExecutionStatus,
    task_number: u32,
) -> bool {
    status.blocking_task == Some(task_number)
        && status.reason_codes.iter().any(|reason_code| {
            reason_code
                == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
        })
        && status.review_state_status == "clean"
}

pub(super) fn baseline_bridge_reason_present(status: &PlanExecutionStatus) -> bool {
    status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
                | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE
        )
    })
}

pub(super) fn baseline_bridge_missing_and_candidate_reasons_present(
    status: &PlanExecutionStatus,
) -> bool {
    prior_missing_reason_present(status) && baseline_bridge_candidate_reason_present(status)
}

pub(super) fn task_closure_baseline_bridge_missing_current_task_recording_ready(
    status: &PlanExecutionStatus,
    review_state_status: &str,
) -> bool {
    review_state_status != crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        && current_closure_scope_empty(status)
        && prior_missing_reason_present(status)
}

pub(super) fn prior_missing_reason_present(status: &PlanExecutionStatus) -> bool {
    status.reason_codes.iter().any(|reason_code| {
        reason_code
            == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
    })
}

pub(super) fn prior_stale_reason_present(status: &PlanExecutionStatus) -> bool {
    status.reason_codes.iter().any(|reason_code| {
        reason_code
            == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE
    })
}

fn baseline_bridge_candidate_reason_present(status: &PlanExecutionStatus) -> bool {
    status.reason_codes.iter().any(|reason_code| {
        reason_code
            == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE
    })
}

pub(super) fn baseline_bridge_candidate_route_reason_present(status: &PlanExecutionStatus) -> bool {
    status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
                | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE
                | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_CURRENT_TASK_CLOSURE_OVERLAY_RESTORE_REQUIRED
                | "task_closure_negative_result_overlay_restore_required"
        )
    })
}

pub(super) fn baseline_bridge_candidate_present(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    task_number: u32,
) -> bool {
    let Some(assessment) = authority_inputs.branch_rerecording_assessment else {
        return false;
    };
    task_closure_baseline_repair_candidate_with_stale_target_and_authority(
        context,
        status,
        task_number,
        baseline_bridge_authority_precedence(status, authority_inputs).earliest_stale_task,
        authority_inputs.overlay,
        authority_inputs.authoritative_state,
        assessment,
    )
    .ok()
    .flatten()
    .is_some()
}

pub(super) fn task_closure_recording_harness(status: &PlanExecutionStatus) -> bool {
    matches!(
        status.harness_phase,
        HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
    )
}

pub(super) fn current_closure_scope_empty(status: &PlanExecutionStatus) -> bool {
    status.current_branch_closure_id.is_none() && status.current_task_closures.is_empty()
}

pub(super) fn task_closure_baseline_bridge_external_review_ready_promotes_closure_recording(
    status: &PlanExecutionStatus,
) -> bool {
    closure_baseline_routing_reason_codes_compatible(status)
        && !status.reason_codes.iter().any(|reason_code| {
            crate::execution::closure_diagnostics::task_boundary_pending_review_projection_reason_code(
                reason_code,
            )
        })
}

pub(super) fn execution_reentry_task_closure_bridge_route_ready(
    context: &ExecutionContext,
    earliest_task_stale_target: Option<&AuthoritativeStaleTarget>,
    close_current_task_repair_targets: &[PublicRepairTarget],
    task_review_dispatch_id_present: bool,
    baseline_bridge_route_ready_for_blocking_task: bool,
    task_number: u32,
) -> bool {
    baseline_bridge_route_ready_for_blocking_task
        || authoritative_stale_target_allows_task_closure_bridge(
            earliest_task_stale_target,
            task_number,
        ) && close_current_task_public_repair_target_candidate_present(
            close_current_task_repair_targets,
            task_number,
        )
        || reducer_dispatch_bridge_ready(
            context,
            earliest_task_stale_target,
            task_review_dispatch_id_present,
            task_number,
        )
}

fn close_current_task_public_repair_target_candidate_present(
    close_current_task_repair_targets: &[PublicRepairTarget],
    task_number: u32,
) -> bool {
    close_current_task_repair_targets.iter().any(|target| {
        PublicCommandKind::CloseCurrentTask.matches_public_mutation_token(&target.command_kind)
            && target.task == Some(task_number)
    })
}

fn reducer_dispatch_bridge_ready(
    context: &ExecutionContext,
    earliest_task_stale_target: Option<&AuthoritativeStaleTarget>,
    task_review_dispatch_id_present: bool,
    task_number: u32,
) -> bool {
    earliest_task_stale_target.and_then(|target| target.task) == Some(task_number)
        && authoritative_stale_target_allows_task_closure_bridge(
            earliest_task_stale_target,
            task_number,
        )
        && task_review_dispatch_id_present
        && context
            .steps
            .iter()
            .filter(|step| step.task_number == task_number)
            .all(|step| step.checked)
}

pub(super) fn prior_task_closure_progress_edge_required(status: &PlanExecutionStatus) -> bool {
    status.reason_codes.iter().any(|code| {
        crate::execution::closure_diagnostics::task_boundary_progress_edge_reason_code(code)
    }) && !status.reason_codes.iter().any(|code| {
        crate::execution::closure_diagnostics::task_boundary_current_closure_structural_reason_code(
            code,
        )
    })
}

pub(super) fn closure_baseline_routing_reason_codes_compatible(
    status: &PlanExecutionStatus,
) -> bool {
    !status.reason_codes.iter().any(|reason_code| {
        crate::execution::closure_diagnostics::task_closure_recording_blocking_reason_code(
            reason_code,
        ) || crate::execution::closure_diagnostics::task_boundary_current_closure_structural_reason_code(
            reason_code,
        ) || crate::execution::closure_diagnostics::task_boundary_cycle_break_reason_code(
            reason_code,
        )
    })
}
