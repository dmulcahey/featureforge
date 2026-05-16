use crate::contracts::workflow::WorkflowRoute;
use crate::execution::next_action::{
    NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED, diagnostic_next_action_for_route,
};
use crate::execution::phase;
use crate::execution::query::{ExecutionRoutingState, compact_operator_reason_codes};
use crate::execution::state::{PlanExecutionStatus, StatusBlockingRecord};

use super::blockers::{
    materialize_blocker_actions, primary_blocker_for_route, primary_blocker_for_status,
    targetless_stale_reconcile_blockers,
};
use super::decision::{PublicRouteDecision, RouteDecision};
use super::follow_up::derive_required_follow_up_from_optional_status;
use super::public_action::synthesize_next_public_action;
use super::route_facts::targetless_stale_reconcile_for_phase;
use super::route_semantics::{
    blocking_scope_for_phase_detail, canonical_phase_for_shared_decision, default_phase_for_status,
    external_wait_state_for_phase_detail,
};
use super::state_kind::derive_state_kind;

pub(crate) fn route_decision_from_non_runtime_workflow_routing(
    routing: &ExecutionRoutingState,
    blocking_records: &[StatusBlockingRecord],
    external_review_result_ready: bool,
) -> RouteDecision {
    let mut routing = routing.clone();
    if routing.blocking_scope.is_none() {
        routing.blocking_scope = non_runtime_blocking_scope(&routing.route, &routing.phase_detail);
    }
    routing.external_wait_state = external_wait_state_for_phase_detail(
        &routing.phase_detail,
        &routing.blocking_reason_codes,
        external_review_result_ready,
    )
    .or(routing.external_wait_state);
    let state_kind = derive_state_kind(&routing);
    let recommended_public_command = routing.recommended_public_command.clone();
    let (recommended_command, invocation, template, required_inputs) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let next_public_action = synthesize_next_public_action(
        recommended_public_command.as_ref(),
        &routing.phase_detail,
        &routing.route.plan_path,
    );
    let blockers = if targetless_stale_reconcile_for_phase(
        &routing.phase_detail,
        &routing.blocking_reason_codes,
    ) {
        targetless_stale_reconcile_blockers(&routing.phase_detail)
    } else {
        let blockers = primary_blocker_for_route(
            &routing,
            blocking_records,
            state_kind.as_str(),
            next_public_action.as_ref(),
        );
        materialize_blocker_actions(blockers, &routing.route.plan_path)
    };
    let route_next_action = diagnostic_next_action_for_route(
        &state_kind,
        &routing.phase_detail,
        invocation.is_some(),
        !required_inputs.is_empty(),
    )
    .unwrap_or_else(|| routing.next_action.clone());
    let diagnostic_without_local_action =
        route_next_action == NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED;
    let route_required_follow_up = (!diagnostic_without_local_action)
        .then(|| {
            derive_required_follow_up_from_optional_status(
                routing.execution_status.as_ref(),
                &routing.phase_detail,
                &routing.review_state_status,
                routing.blocking_reason_codes.iter().map(String::as_str),
                routing.execution_command_context.as_ref(),
            )
        })
        .flatten();
    let mut decision = RouteDecision {
        state_kind,
        phase: canonical_phase_for_shared_decision(&routing.phase, &routing.phase_detail),
        phase_detail: routing.phase_detail.clone(),
        review_state_status: routing.review_state_status.clone(),
        next_action: route_next_action,
        blocking_reason_codes: routing.blocking_reason_codes.clone(),
        blocking_scope: routing.blocking_scope.clone(),
        blocking_task: routing.blocking_task,
        external_wait_state: routing.external_wait_state.clone(),
        recommended_command,
        recommended_public_command,
        invocation,
        recommended_public_command_template: template,
        required_inputs,
        required_follow_up: route_required_follow_up,
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_reentry_target_source: None,
        execution_command_context: routing.execution_command_context.clone(),
        recording_context: routing.recording_context.clone(),
    };
    decision.normalize_diagnostic_next_action();
    decision
}

fn non_runtime_blocking_scope(route: &WorkflowRoute, phase_detail: &str) -> Option<String> {
    if route.is_engineering_approval_fidelity_blocked() {
        return Some(String::from("workflow"));
    }
    blocking_scope_for_phase_detail(phase_detail, None, None, "clean")
}

pub(crate) fn diagnostic_route_decision_from_status(
    status: &PlanExecutionStatus,
) -> Option<RouteDecision> {
    let next_action =
        diagnostic_next_action_for_route(&status.state_kind, &status.phase_detail, false, false)?;
    let phase = status
        .phase
        .clone()
        .unwrap_or_else(|| default_phase_for_status(status));
    let mut decision = RouteDecision {
        state_kind: status.state_kind.clone(),
        phase: canonical_phase_for_shared_decision(&phase, &status.phase_detail),
        phase_detail: status.phase_detail.clone(),
        review_state_status: status.review_state_status.clone(),
        next_action,
        blocking_reason_codes: status.blocking_reason_codes.clone(),
        blocking_scope: status.blocking_scope.clone(),
        blocking_task: status.blocking_task,
        external_wait_state: status.external_wait_state.clone(),
        recommended_command: None,
        recommended_public_command: None,
        invocation: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        required_follow_up: None,
        next_public_action: None,
        blockers: status.blockers.clone(),
        public_repair_targets: status.public_repair_targets.clone(),
        execution_reentry_target_source: None,
        execution_command_context: None,
        recording_context: None,
    };
    decision.apply_public_route_projection(Some(status), false);
    Some(decision)
}

pub(crate) fn route_decision_for_unroutable_runtime_state(
    status: &PlanExecutionStatus,
) -> RouteDecision {
    let recommended_command = None;
    let next_public_action = None;
    let blockers = primary_blocker_for_status(
        status,
        phase::DETAIL_BLOCKED_RUNTIME_BUG,
        next_public_action.as_ref(),
    );
    let mut decision = RouteDecision {
        state_kind: String::from(phase::DETAIL_BLOCKED_RUNTIME_BUG),
        phase: canonical_phase_for_shared_decision(
            &default_phase_for_status(status),
            "runtime_route_unavailable",
        ),
        phase_detail: status.phase_detail.clone(),
        review_state_status: status.review_state_status.clone(),
        next_action: String::from(NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED),
        blocking_reason_codes: compact_operator_reason_codes(
            Some(status),
            &status.phase_detail,
            &status.review_state_status,
        ),
        recommended_command,
        recommended_public_command: None,
        invocation: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        required_follow_up: None,
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_reentry_target_source: None,
        execution_command_context: None,
        recording_context: None,
        blocking_scope: None,
        blocking_task: None,
        external_wait_state: None,
    };
    decision.apply_public_route_projection(Some(status), false);
    decision
}
