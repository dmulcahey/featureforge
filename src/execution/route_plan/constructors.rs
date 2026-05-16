use crate::execution::command_eligibility::{
    PublicCommandKind, public_advance_late_stage_command_for_phase_detail,
};
use crate::execution::harness::HarnessPhase;
use crate::execution::next_action::{
    NEXT_ACTION_ADVANCE_LATE_STAGE, NEXT_ACTION_CLOSE_CURRENT_TASK,
    NEXT_ACTION_EXECUTION_REENTRY_REQUIRED, NEXT_ACTION_REPAIR_REVIEW_STATE,
    diagnostic_next_action_for_route,
};
use crate::execution::phase;
use crate::execution::public_repair_target_reasons::PublicRepairTargetReason;
use crate::execution::query::{
    ExecutionRoutingExecutionCommandContext, ExecutionRoutingRecordingContext,
};
use crate::execution::reducer::RuntimeState;
use crate::execution::reentry_reconcile::{
    TARGETLESS_STALE_RECONCILE_PHASE_DETAIL, TargetlessStaleReconcile,
};
use crate::execution::review_route_tokens::{
    FOLLOW_UP_ADVANCE_LATE_STAGE, FOLLOW_UP_EXECUTION_REENTRY, FOLLOW_UP_REPAIR_REVIEW_STATE,
    REVIEW_STATE_MISSING_CURRENT_CLOSURE,
};
use crate::execution::state::{PlanExecutionStatus, recommended_execution_source};
use crate::execution::status_assembly::effective_route_review_state_status;

use super::blockers::{
    materialize_blocker_actions, primary_blocker_for_status, targetless_stale_reconcile_blockers,
};
use super::decision::{PublicRouteDecision, RouteDecision};
use super::finalization_facts::PersistedReopenTarget;
use super::public_action::synthesize_next_public_action;
use super::public_commands::{
    close_current_task_public_command, reopen_public_command, repair_review_state_public_command,
};
use super::route_facts::{
    compact_route_reason_codes, public_route_blocking_reason_codes,
    task_closure_recording_reentry_target_source,
};
use super::state_kind::{
    STATE_KIND_ACTIONABLE_PUBLIC_COMMAND, derive_state_kind_from_seed, state_kind_command_marker,
};

pub(crate) fn close_current_task_route_decision(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
    task_number: u32,
) -> RouteDecision {
    let phase_detail = String::from(phase::DETAIL_TASK_CLOSURE_RECORDING_READY);
    let recommended_public_command = Some(close_current_task_public_command(
        &runtime_state.context.plan_rel,
        task_number,
    ));
    let (recommended_command, invocation, template, required_inputs) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let next_public_action = synthesize_next_public_action(
        recommended_public_command.as_ref(),
        &phase_detail,
        &runtime_state.context.plan_rel,
    );
    let state_kind = derive_state_kind_from_seed(
        None,
        HarnessPhase::Executing,
        &phase_detail,
        state_kind_command_marker(recommended_public_command.as_ref()),
    );
    let blockers = materialize_blocker_actions(
        primary_blocker_for_status(status, state_kind.as_str(), next_public_action.as_ref()),
        &runtime_state.context.plan_rel,
    );
    let review_state_status =
        effective_route_review_state_status(status, &phase_detail, &status.review_state_status);
    let blocking_reason_codes = compact_route_reason_codes(
        status,
        &phase_detail,
        &review_state_status,
        Some(task_number),
        None,
    );
    let execution_reentry_target_source =
        task_closure_recording_reentry_target_source(&phase_detail, &blocking_reason_codes);
    let mut decision = RouteDecision {
        state_kind,
        phase: String::from(phase::PHASE_TASK_CLOSURE_PENDING),
        phase_detail,
        review_state_status,
        next_action: String::from(NEXT_ACTION_CLOSE_CURRENT_TASK),
        blocking_reason_codes,
        recommended_command,
        recommended_public_command,
        invocation,
        recommended_public_command_template: template,
        required_inputs,
        required_follow_up: None,
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_reentry_target_source,
        execution_command_context: None,
        recording_context: Some(ExecutionRoutingRecordingContext {
            task_number: Some(task_number),
            dispatch_id: runtime_state.task_review_dispatch_id.clone(),
            branch_closure_id: None,
        }),
        blocking_scope: None,
        blocking_task: None,
        external_wait_state: None,
    };
    decision.apply_public_route_projection(Some(status), false);
    decision
}

pub(crate) fn repair_review_state_route_decision(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
    task_number: Option<u32>,
    reason_code: &str,
    execution_reentry_target_source: Option<String>,
) -> RouteDecision {
    let phase_detail = String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED);
    let recommended_public_command = Some(repair_review_state_public_command(
        &runtime_state.context.plan_rel,
    ));
    let (recommended_command, invocation, template, required_inputs) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let next_public_action = synthesize_next_public_action(
        recommended_public_command.as_ref(),
        &phase_detail,
        &runtime_state.context.plan_rel,
    );
    let review_state_status = status.review_state_status.clone();
    let mut blocking_reason_codes = compact_route_reason_codes(
        status,
        &phase_detail,
        &review_state_status,
        task_number.or(status.blocking_task),
        None,
    );
    if !blocking_reason_codes
        .iter()
        .any(|existing| existing == reason_code)
    {
        blocking_reason_codes.push(reason_code.to_owned());
    }
    let state_kind = derive_state_kind_from_seed(
        None,
        status.harness_phase,
        &phase_detail,
        state_kind_command_marker(recommended_public_command.as_ref()),
    );
    let blockers = materialize_blocker_actions(
        primary_blocker_for_status(status, state_kind.as_str(), next_public_action.as_ref()),
        &runtime_state.context.plan_rel,
    );
    let next_action = diagnostic_next_action_for_route(
        &state_kind,
        &phase_detail,
        invocation.is_some(),
        !required_inputs.is_empty(),
    )
    .unwrap_or_else(|| String::from(NEXT_ACTION_REPAIR_REVIEW_STATE));
    let mut decision = RouteDecision {
        state_kind,
        phase: String::from(phase::PHASE_EXECUTING),
        phase_detail,
        review_state_status,
        next_action,
        blocking_reason_codes,
        recommended_command,
        recommended_public_command,
        invocation,
        recommended_public_command_template: template,
        required_inputs,
        required_follow_up: Some(String::from(FOLLOW_UP_REPAIR_REVIEW_STATE)),
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_reentry_target_source,
        execution_command_context: None,
        recording_context: None,
        blocking_scope: None,
        blocking_task: None,
        external_wait_state: None,
    };
    decision.normalize_diagnostic_next_action();
    decision.apply_public_route_projection(Some(status), false);
    decision
}

pub(crate) fn runtime_reconcile_route_decision(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
    task_number: Option<u32>,
    reason_code: &str,
) -> RouteDecision {
    let phase_detail = String::from(TARGETLESS_STALE_RECONCILE_PHASE_DETAIL);
    let targetless_stale_reconcile =
        TargetlessStaleReconcile::from_reason_code(reason_code).is_some();
    let recommended_public_command = (!targetless_stale_reconcile)
        .then(|| repair_review_state_public_command(&runtime_state.context.plan_rel));
    let (recommended_command, invocation, template, required_inputs) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let next_public_action = synthesize_next_public_action(
        recommended_public_command.as_ref(),
        &phase_detail,
        &runtime_state.context.plan_rel,
    );
    let review_state_status = status.review_state_status.clone();
    let mut blocking_reason_codes = compact_route_reason_codes(
        status,
        &phase_detail,
        &review_state_status,
        task_number.or(status.blocking_task),
        None,
    );
    if targetless_stale_reconcile {
        TargetlessStaleReconcile::ensure_reason_codes(&mut blocking_reason_codes);
    } else if !blocking_reason_codes
        .iter()
        .any(|existing| existing == reason_code)
    {
        blocking_reason_codes.push(reason_code.to_owned());
    }
    let state_kind = derive_state_kind_from_seed(
        None,
        status.harness_phase,
        &phase_detail,
        state_kind_command_marker(recommended_public_command.as_ref()),
    );
    let blockers = if targetless_stale_reconcile {
        targetless_stale_reconcile_blockers(&phase_detail)
    } else {
        materialize_blocker_actions(
            primary_blocker_for_status(status, state_kind.as_str(), next_public_action.as_ref()),
            &runtime_state.context.plan_rel,
        )
    };
    let next_action = diagnostic_next_action_for_route(
        &state_kind,
        &phase_detail,
        invocation.is_some(),
        !required_inputs.is_empty(),
    )
    .unwrap_or_else(|| String::from(NEXT_ACTION_REPAIR_REVIEW_STATE));
    let mut decision = RouteDecision {
        state_kind,
        phase: String::from(phase::PHASE_EXECUTING),
        phase_detail,
        review_state_status,
        next_action,
        blocking_reason_codes,
        recommended_command,
        recommended_public_command,
        invocation,
        recommended_public_command_template: template,
        required_inputs,
        required_follow_up: (!targetless_stale_reconcile)
            .then(|| String::from(FOLLOW_UP_REPAIR_REVIEW_STATE)),
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
    decision.normalize_diagnostic_next_action();
    decision.apply_public_route_projection(Some(status), false);
    decision
}

pub(crate) fn execution_reentry_reopen_route_decision(
    runtime_state: &RuntimeState,
    target: PersistedReopenTarget,
    execution_reentry_target_source: Option<String>,
) -> RouteDecision {
    let task_number = target.task_number;
    let step_number = target.step_number;
    let command = reopen_public_command(
        &runtime_state.context.plan_rel,
        task_number,
        step_number,
        recommended_execution_source(runtime_state.status.execution_mode.as_str()),
        &runtime_state.status.execution_fingerprint,
    );
    let (recommended_command, invocation, template, required_inputs) =
        RouteDecision::command_surfaces(Some(&command));
    let phase_detail = String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED);
    let review_state_status = effective_route_review_state_status(
        &runtime_state.status,
        &phase_detail,
        &runtime_state.status.review_state_status,
    );
    let candidate_reason_codes = compact_route_reason_codes(
        &runtime_state.status,
        &phase_detail,
        &review_state_status,
        Some(task_number),
        None,
    );
    let mut blocking_reason_codes = public_route_blocking_reason_codes(
        &runtime_state.status,
        &phase_detail,
        Some(task_number),
        &candidate_reason_codes,
    );
    if !blocking_reason_codes
        .iter()
        .any(|reason| PublicRepairTargetReason::PersistedExecutionReentryFollowUp.matches(reason))
    {
        blocking_reason_codes
            .push(PublicRepairTargetReason::PersistedExecutionReentryFollowUp.reason_code());
    }
    let mut route_decision = RouteDecision {
        state_kind: String::from(STATE_KIND_ACTIONABLE_PUBLIC_COMMAND),
        phase: String::from(phase::PHASE_EXECUTING),
        phase_detail,
        review_state_status,
        next_action: String::from(NEXT_ACTION_EXECUTION_REENTRY_REQUIRED),
        blocking_reason_codes,
        blocking_scope: Some(String::from("task")),
        blocking_task: Some(task_number),
        external_wait_state: None,
        recommended_command,
        recommended_public_command: Some(command),
        invocation,
        recommended_public_command_template: template,
        required_inputs,
        required_follow_up: Some(String::from(FOLLOW_UP_EXECUTION_REENTRY)),
        next_public_action: None,
        blockers: Vec::new(),
        public_repair_targets: Vec::new(),
        execution_reentry_target_source,
        execution_command_context: Some(ExecutionRoutingExecutionCommandContext {
            command_kind: PublicCommandKind::Reopen.public_mutation_token().to_owned(),
            task_number: Some(task_number),
            step_id: Some(step_number),
        }),
        recording_context: None,
    };
    route_decision.apply_public_route_projection(Some(&runtime_state.status), false);
    route_decision
}

pub(crate) fn branch_closure_recording_route_decision(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
) -> RouteDecision {
    let phase_detail =
        String::from(phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS);
    let recommended_public_command = public_advance_late_stage_command_for_phase_detail(
        &runtime_state.context.plan_rel,
        &phase_detail,
    );
    let (recommended_command, invocation, template, required_inputs) =
        PublicRouteDecision::command_surfaces(recommended_public_command.as_ref());
    let next_public_action = synthesize_next_public_action(
        recommended_public_command.as_ref(),
        &phase_detail,
        &runtime_state.context.plan_rel,
    );
    let blockers = materialize_blocker_actions(
        primary_blocker_for_status(
            status,
            STATE_KIND_ACTIONABLE_PUBLIC_COMMAND,
            next_public_action.as_ref(),
        ),
        &runtime_state.context.plan_rel,
    );
    let review_state_status = effective_route_review_state_status(
        status,
        &phase_detail,
        REVIEW_STATE_MISSING_CURRENT_CLOSURE,
    );
    let mut decision = RouteDecision {
        state_kind: String::from(STATE_KIND_ACTIONABLE_PUBLIC_COMMAND),
        phase: String::from(phase::PHASE_DOCUMENT_RELEASE_PENDING),
        phase_detail,
        review_state_status,
        next_action: String::from(NEXT_ACTION_ADVANCE_LATE_STAGE),
        blocking_reason_codes: vec![String::from(REVIEW_STATE_MISSING_CURRENT_CLOSURE)],
        recommended_command,
        recommended_public_command,
        invocation,
        recommended_public_command_template: template,
        required_inputs,
        required_follow_up: Some(String::from(FOLLOW_UP_ADVANCE_LATE_STAGE)),
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
