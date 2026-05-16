use super::blockers::{primary_blocker_for_status, targetless_stale_reconcile_blockers};
use super::decision::RouteDecision;
use super::follow_up::derive_required_follow_up;
use super::route_facts::targetless_stale_reconcile_for_phase;
use super::status_application::{
    RouteStatusProjectionInput, apply_common_route_status_projection,
    apply_route_status_projection_diagnostics,
};
use crate::diagnostics::JsonFailure;
use crate::execution::next_action::{
    NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED, runtime_route_is_diagnostic,
};
use crate::execution::phase;
use crate::execution::public_repair_targets::public_repair_targets_for_route_decision;
use crate::execution::reducer::RuntimeState;
use crate::execution::stale_target_projection::project_stale_unreviewed_closures;
use crate::execution::state::PlanExecutionStatus;
use crate::execution::status::GateState;
use crate::execution::status_assembly::compute_status_blocking_records;
use crate::execution::transitions::AuthoritativeTransitionState;

pub(in crate::execution) fn status_for_route_plan_projection(
    runtime_state: &RuntimeState,
    route_decision: &RouteDecision,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let mut status = runtime_state.status.clone();
    apply_common_route_status_projection(RouteStatusProjectionInput {
        status: &mut status,
        route_decision,
    });
    project_stale_unreviewed_closures(&mut status, &runtime_state.gate_snapshot);
    let fallback_gate_finish;
    let gate_finish = match runtime_state.gate_snapshot.gate_finish.as_ref() {
        Some(gate_finish) => gate_finish,
        None => {
            fallback_gate_finish = GateState::default().finish();
            &fallback_gate_finish
        }
    };
    let status_blocking_records = compute_status_blocking_records(
        &runtime_state.context,
        &status,
        gate_finish,
        Some(&runtime_state.gate_snapshot.stale_targets),
        authoritative_state,
    )?;
    status.blocking_records = status_blocking_records;
    apply_route_status_projection_diagnostics(&mut status);
    Ok(status)
}

pub(in crate::execution::route_plan) fn finalize_route_decision_for_route_plan(
    mut route_decision: RouteDecision,
    status: &PlanExecutionStatus,
    runtime_state: &RuntimeState,
    external_review_result_ready: bool,
) -> RouteDecision {
    let targetless_reconcile = targetless_stale_reconcile_for_phase(
        &route_decision.phase_detail,
        &route_decision.blocking_reason_codes,
    );
    if targetless_reconcile {
        normalize_targetless_stale_reconcile_route_decision(&mut route_decision);
    } else {
        route_decision.blockers = primary_blocker_for_status(
            status,
            route_decision.state_kind.as_str(),
            route_decision.next_public_action.as_ref(),
        );
        route_decision.required_follow_up = derive_required_follow_up(
            status,
            &route_decision.phase_detail,
            &route_decision.review_state_status,
            route_decision
                .blocking_reason_codes
                .iter()
                .map(String::as_str),
            route_decision.execution_command_context.as_ref(),
        );
    }
    route_decision.public_repair_targets = public_repair_targets_for_route_decision(
        status,
        &route_decision,
        &runtime_state.route_repair_target_candidates,
    );
    normalize_diagnostic_route_decision(&mut route_decision);
    route_decision.apply_public_route_projection(Some(status), external_review_result_ready);
    route_decision
}

pub(crate) fn normalize_diagnostic_route_decision(route_decision: &mut RouteDecision) {
    if runtime_route_is_diagnostic(&route_decision.state_kind, &route_decision.phase_detail) {
        route_decision.next_action = String::from(NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED);
        route_decision.required_follow_up = None;
        route_decision.next_public_action = None;
        route_decision.blockers.clear();
        route_decision.public_repair_targets.clear();
        route_decision.recommended_command = None;
        route_decision.recommended_public_command = None;
        route_decision.invocation = None;
        route_decision.recommended_public_command_template = None;
        route_decision.required_inputs.clear();
        route_decision.execution_reentry_target_source = None;
        route_decision.execution_command_context = None;
        route_decision.recording_context = None;
    }
}

fn normalize_targetless_stale_reconcile_route_decision(route_decision: &mut RouteDecision) {
    route_decision.state_kind = String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
    route_decision.next_action = String::from(NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED);
    route_decision.blockers = targetless_stale_reconcile_blockers(&route_decision.phase_detail);
    route_decision.required_follow_up = None;
    route_decision.next_public_action = None;
    route_decision.public_repair_targets.clear();
    route_decision.recommended_command = None;
    route_decision.recommended_public_command = None;
    route_decision.invocation = None;
    route_decision.recommended_public_command_template = None;
    route_decision.required_inputs.clear();
    route_decision.execution_reentry_target_source = None;
    route_decision.execution_command_context = None;
}
