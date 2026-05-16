use crate::execution::closure_diagnostics::apply_task_boundary_projection_diagnostics;
use crate::execution::harness::HarnessPhase;
use crate::execution::phase;
use crate::execution::reentry_reconcile::TargetlessStaleReconcile;
use crate::execution::state::{
    PlanExecutionStatus, PublicExecutionCommandContext, PublicRecordingContext,
};

use super::decision::RouteDecision;

pub(crate) struct RouteStatusProjectionInput<'a> {
    pub(crate) status: &'a mut PlanExecutionStatus,
    pub(crate) route_decision: &'a RouteDecision,
}

pub(crate) fn apply_common_route_status_projection(input: RouteStatusProjectionInput<'_>) {
    let status = input.status;
    let route_decision = input.route_decision;

    status.phase = Some(route_decision.phase.clone());
    status.harness_phase = harness_phase_for_route_status(status, &route_decision.phase);
    status.phase_detail = route_decision.phase_detail.clone();
    status.review_state_status = route_decision.review_state_status.clone();
    status.recording_context =
        route_decision
            .recording_context
            .as_ref()
            .map(|context| PublicRecordingContext {
                task_number: context.task_number,
                dispatch_id: context.dispatch_id.clone(),
                branch_closure_id: context.branch_closure_id.clone(),
            });
    status.execution_command_context =
        route_decision
            .execution_command_context
            .as_ref()
            .map(|context| PublicExecutionCommandContext {
                command_kind: context.command_kind.clone(),
                task_number: context.task_number,
                step_id: context.step_id,
            });
    status.next_action = route_decision.next_action.clone();
    status.state_kind = route_decision.state_kind.clone();
    status.next_public_action = route_decision.next_public_action.clone();
    status.blockers.clone_from(&route_decision.blockers);
    status.execution_reentry_target_source = route_decision.execution_reentry_target_source.clone();
    status
        .public_repair_targets
        .clone_from(&route_decision.public_repair_targets);
    status.recommended_public_command = route_decision.recommended_public_command.clone();
    status.recommended_public_command_argv = route_decision.public_command_argv();
    status.recommended_public_command_template = route_decision.public_command_template();
    status.required_inputs = route_decision.required_inputs.clone();
    status.recommended_command = route_decision.recommended_command.clone();
    status.blocking_task = route_decision.blocking_task;
    status
        .blocking_scope
        .clone_from(&route_decision.blocking_scope);
    status
        .external_wait_state
        .clone_from(&route_decision.external_wait_state);
    status
        .blocking_reason_codes
        .clone_from(&route_decision.blocking_reason_codes);
}

pub(crate) fn apply_route_status_projection_diagnostics(status: &mut PlanExecutionStatus) {
    if TargetlessStaleReconcile::from_phase_and_reason_codes(
        &status.phase_detail,
        &status.blocking_reason_codes,
    )
    .is_some()
    {
        TargetlessStaleReconcile::ensure_status_diagnostic(status);
    } else {
        TargetlessStaleReconcile::clear_status_diagnostic(status);
    }
    apply_task_boundary_projection_diagnostics(status);
}

fn harness_phase_for_route_status(status: &PlanExecutionStatus, route_phase: &str) -> HarnessPhase {
    if status.execution_started == "no"
        && matches!(status.harness_phase, HarnessPhase::ImplementationHandoff)
    {
        return status.harness_phase;
    }
    if route_phase == phase::PHASE_TASK_CLOSURE_PENDING {
        return HarnessPhase::Executing;
    }
    route_status_harness_phase(route_phase).unwrap_or(status.harness_phase)
}

fn route_status_harness_phase(route_phase: &str) -> Option<HarnessPhase> {
    let phase = HarnessPhase::parse(route_phase)?;
    // Route projection only overwrites the persisted harness phase for runtime
    // phases that are route-authoritative. Pre-execution and contract phases
    // remain owned by the authoritative harness state.
    match phase {
        HarnessPhase::DocumentReleasePending
        | HarnessPhase::FinalReviewPending
        | HarnessPhase::QaPending
        | HarnessPhase::ReadyForBranchCompletion
        | HarnessPhase::PivotRequired
        | HarnessPhase::HandoffRequired
        | HarnessPhase::Executing => Some(phase),
        _ => None,
    }
}
