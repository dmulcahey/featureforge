mod blockers;
mod constructors;
mod decision;
mod decision_support;
mod execution_target_authority;
mod execution_targets;
mod final_review_dispatch;
mod finalization_facts;
mod follow_up;
pub(in crate::execution) mod next_action_choice;
mod next_action_finalization;
mod next_action_route;
mod planning_facts;
mod public_action;
mod public_commands;
mod repair_follow_up_binding;
mod route_facts;
mod route_semantics;
mod stale_repair_target;
mod state_kind;
mod status_application;
mod status_projection;
#[cfg(test)]
mod unit_tests;
use std::collections::BTreeSet;

pub(crate) use self::blockers::{
    materialize_blocker_actions, primary_blocker_for_status, targetless_stale_reconcile_blockers,
};
pub(crate) use self::constructors::{
    branch_closure_recording_route_decision, close_current_task_route_decision,
    execution_reentry_reopen_route_decision, repair_review_state_route_decision,
    runtime_reconcile_route_decision,
};
pub(crate) use self::decision::{Blocker, NextPublicAction, PublicRouteDecision, RouteDecision};
pub(crate) use self::decision_support::{
    diagnostic_route_decision_from_status, route_decision_for_unroutable_runtime_state,
    route_decision_from_non_runtime_workflow_routing,
};
pub(crate) use self::execution_target_authority::execution_command_route_target_has_authority;
pub(crate) use self::execution_targets::resolve_execution_command_route_target;
use self::finalization_facts::{ExecutionReentryTaskClosureBridgeFacts, PersistedReopenTarget};
pub(crate) use self::follow_up::{
    derive_required_follow_up, execution_reentry_target_source_for_route,
    public_command_for_required_follow_up, required_follow_up_from_route_decision,
};
use self::next_action_route::route_decision_from_next_action_choice;
use self::planning_facts::{RoutePlanningFactInputs, RoutePlanningFacts, legal_resume_begin_route};
#[cfg(test)]
pub(crate) use self::public_commands::execution_command_route_target_from_status_context;
#[cfg(test)]
pub(crate) use self::public_commands::public_command_from_status_context;
pub(crate) use self::public_commands::{
    execution_command_route_target_from_decision, reopen_public_command_with_reason,
    transfer_handoff_public_command,
};
use self::repair_follow_up_binding::bind_repair_follow_up_to_source_route;
pub(crate) use self::route_facts::{
    command_context_reopens_current_task_closure, compact_route_reason_codes, merge_reason_codes,
    public_route_blocking_reason_codes, targetless_stale_reconcile_for_phase,
    task_closure_recording_reentry_target_source,
};
pub use self::state_kind::PUBLIC_STATE_KIND_VALUES;
#[cfg(test)]
pub(crate) use self::state_kind::classify_state_kind;
pub(crate) use self::state_kind::{
    STATE_KIND_ACTIONABLE_PUBLIC_COMMAND, STATE_KIND_PLANNING_REENTRY_REQUIRED,
    STATE_KIND_TERMINAL, STATE_KIND_WAITING_EXTERNAL_INPUT, derive_state_kind_from_seed,
    external_wait_state_is_external_wait, state_kind_blocks_local_mutation,
    state_kind_command_marker, state_kind_is_blocked_runtime_bug, state_kind_is_external_wait,
    state_kind_is_planning_reentry_required, state_kind_is_runtime_diagnostic,
    state_kind_is_runtime_reconcile_required, state_kind_is_terminal,
    state_kind_or_phase_is_runtime_diagnostic,
};
use self::status_projection::{
    finalize_route_decision_for_route_plan, status_for_route_plan_projection,
};
use crate::diagnostics::JsonFailure;
use crate::execution::command_eligibility::{PublicCommand, PublicCommandKind};
use crate::execution::current_task_closure_selection::current_task_closure_route_target;
use crate::execution::harness::HarnessPhase;
use crate::execution::phase;
use crate::execution::public_repair_target_reasons::PublicRepairTargetReason;
use crate::execution::reducer::RuntimeState;
use crate::execution::reentry_reconcile::TARGETLESS_STALE_RECONCILE_REASON_CODE;
use crate::execution::repair_route_decision::{
    NextActionAuthorityReadScope, next_action_authority_inputs_from_gate_snapshot,
    task_closure_baseline_bridge_persisted_close_current_task_route_task,
    task_closure_baseline_bridge_repair_review_state_route,
    task_closure_baseline_bridge_route_ready_for_status, task_closure_baseline_bridge_route_task,
};
use crate::execution::repair_target_selection::{
    NextActionAuthorityInputs, completed_task_closure_preempts_execution_reentry,
};
use crate::execution::review_route_tokens::{
    FOLLOW_UP_EXECUTION_REENTRY, REVIEW_STATE_MISSING_CURRENT_CLOSURE,
};
use crate::execution::state::PlanExecutionStatus;
use crate::execution::status_assembly::effective_review_state_status;
use crate::execution::status_support::current_task_closure_branch_route_facts_from_status;
use crate::execution::transitions::AuthoritativeTransitionState;
pub(crate) struct RuntimeRoutePlanInputs<'a> {
    pub(crate) authoritative_state: Option<&'a AuthoritativeTransitionState>,
    pub(crate) external_review_result_ready: bool,
    pub(crate) require_exact_execution_command: bool,
}

pub(crate) fn plan_runtime_route(
    runtime_state: &mut RuntimeState,
    inputs: RuntimeRoutePlanInputs<'_>,
) -> Result<(RouteDecision, PlanExecutionStatus), JsonFailure> {
    bind_repair_follow_up_to_source_route(
        runtime_state,
        inputs.authoritative_state,
        inputs.external_review_result_ready,
        inputs.require_exact_execution_command,
    )?;
    let (route_decision, status_projection) =
        route_decision_and_status_from_runtime_state_with_authority(
            runtime_state,
            inputs.authoritative_state,
            inputs.external_review_result_ready,
            inputs.require_exact_execution_command,
        )?;
    Ok((route_decision, status_projection))
}

fn close_current_task_or_branch_closure_route_decision(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
    route_facts: &RoutePlanningFacts,
    task_number: u32,
) -> RouteDecision {
    if route_facts
        .current_task_closure_branch_route_facts
        .task_should_route_to_branch_closure(status, task_number)
    {
        return branch_closure_recording_route_decision(runtime_state, status);
    }
    close_current_task_route_decision(runtime_state, status, task_number)
}

fn persisted_close_current_task_bridge_task(
    runtime_state: &RuntimeState,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    persisted_repair_follow_up: Option<&str>,
) -> Option<u32> {
    task_closure_baseline_bridge_persisted_close_current_task_route_task(
        &runtime_state.context,
        status,
        authority_inputs,
        persisted_repair_follow_up,
    )
}

fn next_action_authority_inputs_for_route_plan<'a>(
    runtime_state: &'a RuntimeState,
    status: &'a PlanExecutionStatus,
    authoritative_state: Option<&'a AuthoritativeTransitionState>,
) -> NextActionAuthorityInputs<'a> {
    let current_task_closure_branch_route_facts =
        current_task_closure_branch_route_facts_from_status(&runtime_state.context, status);
    next_action_authority_inputs_from_gate_snapshot(
        status,
        &runtime_state.gate_snapshot,
        NextActionAuthorityReadScope {
            overlay: runtime_state.overlay.as_ref(),
            authoritative_state,
            persisted_repair_follow_up: runtime_state.persisted_repair_follow_up.as_deref(),
            branch_rerecording_assessment: runtime_state.branch_rerecording_assessment.as_ref(),
            route_repair_target_candidates: &runtime_state.route_repair_target_candidates,
            ..NextActionAuthorityReadScope::default()
        },
    )
    .with_current_task_closure_branch_route_facts(current_task_closure_branch_route_facts)
}

fn route_planning_authority_for_status<'a>(
    runtime_state: &'a RuntimeState,
    status: &'a PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'a>,
) -> RoutePlanningFacts {
    let review_state_status =
        effective_review_state_status(status, status.review_state_status.as_str());
    let baseline_bridge_repair_review_state_route =
        task_closure_baseline_bridge_repair_review_state_route(
            &runtime_state.context,
            status,
            authority_inputs,
            review_state_status.as_str(),
        );
    let baseline_bridge_close_current_task_candidate =
        baseline_bridge_repair_review_state_route.and_then(|route| route.target_task);
    let baseline_bridge_execution_reentry_task =
        task_closure_baseline_bridge_route_task(&runtime_state.context, status, authority_inputs)
            .ok()
            .flatten();
    let baseline_bridge_route_ready_for_blocking_task =
        task_closure_baseline_bridge_route_ready_for_status(
            &runtime_state.context,
            status,
            authority_inputs,
            runtime_state
                .gate_snapshot
                .earliest_task_stale_target_details(),
        );
    let execution_reentry_task_closure_bridge_facts = ExecutionReentryTaskClosureBridgeFacts {
        earliest_task_stale_target: runtime_state
            .gate_snapshot
            .earliest_task_stale_target_details()
            .cloned(),
        close_current_task_repair_targets: runtime_state.route_repair_target_candidates.clone(),
        task_review_dispatch_id_present: runtime_state.task_review_dispatch_id.is_some(),
        baseline_bridge_route_ready_for_blocking_task,
    };
    let execution_reentry_target_source = execution_reentry_target_source_for_route(
        runtime_state,
        status,
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
        authority_inputs,
    );
    let completed_task_closure_preemption_tasks = runtime_state
        .context
        .tasks_by_number
        .keys()
        .copied()
        .filter(|task_number| {
            completed_task_closure_preempts_execution_reentry(
                &runtime_state.context,
                status,
                authority_inputs,
                review_state_status.as_str(),
                *task_number,
            )
        })
        .collect::<BTreeSet<_>>();
    let fallback_completed_task_closure_preemption_task =
        runtime_state.context.tasks_by_number.keys().copied().max();
    let persisted_close_current_task_bridge_task = persisted_close_current_task_bridge_task(
        runtime_state,
        status,
        authority_inputs,
        runtime_state.persisted_repair_follow_up.as_deref(),
    );
    let persisted_reopen_target = runtime_state
        .route_repair_target_candidates
        .iter()
        .find_map(|candidate| {
            if !PublicCommandKind::Reopen.matches_public_mutation_token(&candidate.command_kind)
                || !PublicRepairTargetReason::PersistedExecutionReentryFollowUp
                    .matches(candidate.reason_code.as_str())
            {
                return None;
            }
            Some(PersistedReopenTarget {
                task_number: candidate.task?,
                step_number: candidate.step?,
            })
        });
    let plan_rel = &runtime_state.context.plan_rel;
    let repair_targets = &runtime_state.route_repair_target_candidates;
    RoutePlanningFacts::from_inputs(RoutePlanningFactInputs {
        status,
        review_state_status,
        earliest_stale_task_target: runtime_state.gate_snapshot.earliest_task_stale_target(),
        legal_resume_begin_route: legal_resume_begin_route(status, plan_rel, repair_targets),
        authoritative_stale_target_bound: authority_inputs.has_authoritative_stale_target,
        actionable_stale_reentry_target_bound: authority_inputs
            .authoritative_stale_target
            .is_some(),
        baseline_bridge_repair_review_state_ready: baseline_bridge_repair_review_state_route
            .is_some(),
        baseline_bridge_close_current_task_candidate,
        baseline_bridge_execution_reentry_task,
        execution_reentry_task_closure_bridge_facts,
        execution_reentry_target_source,
        completed_task_closure_preemption_tasks,
        fallback_completed_task_closure_preemption_task,
        persisted_close_current_task_bridge_task,
        persisted_reopen_target,
        persisted_repair_follow_up: runtime_state.persisted_repair_follow_up.as_deref(),
        current_task_closure_branch_route_facts: authority_inputs
            .current_task_closure_branch_route_facts_or_derive(&runtime_state.context, status),
    })
}

#[cfg(test)]
pub(crate) fn route_decision_from_runtime_state_with_inputs(
    runtime_state: &RuntimeState,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<RouteDecision, JsonFailure> {
    Ok(route_decision_and_status_from_runtime_state_with_authority(
        runtime_state,
        None,
        external_review_result_ready,
        require_exact_execution_command,
    )?
    .0)
}

pub(crate) fn route_decision_from_runtime_state_with_authority(
    runtime_state: &RuntimeState,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<RouteDecision, JsonFailure> {
    Ok(route_decision_and_status_from_runtime_state_with_authority(
        runtime_state,
        authoritative_state,
        external_review_result_ready,
        require_exact_execution_command,
    )?
    .0)
}

fn route_decision_and_status_from_runtime_state_with_authority(
    runtime_state: &RuntimeState,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<(RouteDecision, PlanExecutionStatus), JsonFailure> {
    let authority_inputs = next_action_authority_inputs_for_route_plan(
        runtime_state,
        &runtime_state.status,
        authoritative_state,
    );
    let route_facts =
        route_planning_authority_for_status(runtime_state, &runtime_state.status, authority_inputs);
    let mut route_decision = select_runtime_route_decision(
        runtime_state,
        &route_facts,
        authority_inputs,
        external_review_result_ready,
        require_exact_execution_command,
    );
    apply_route_planning_fact_adjustments(&mut route_decision, &route_facts);
    let route_status =
        status_for_route_plan_projection(runtime_state, &route_decision, authoritative_state)?;
    let route_decision = finalize_route_decision_for_route_plan(
        route_decision,
        &route_status,
        runtime_state,
        external_review_result_ready,
    );
    let status_projection =
        status_for_route_plan_projection(runtime_state, &route_decision, authoritative_state)?;
    Ok((route_decision, status_projection))
}

fn apply_route_planning_fact_adjustments(
    route_decision: &mut RouteDecision,
    route_facts: &RoutePlanningFacts,
) {
    if let Some(task) = route_facts.exact_resume_stale_task {
        route_decision.blocking_scope = Some(String::from("task"));
        route_decision.blocking_task = Some(task);
    }
}

fn select_route_planning_candidate(
    primary_route: RouteDecision,
    runtime_state: &RuntimeState,
    route_facts: &RoutePlanningFacts,
) -> RouteDecision {
    if primary_route.phase_detail == phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
        && route_facts.baseline_bridge_repair_review_state_ready
    {
        return repair_review_state_route_decision(
            runtime_state,
            &runtime_state.status,
            route_facts.baseline_bridge_close_current_task_candidate,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE,
            route_facts.execution_reentry_target_source.clone(),
        );
    }
    if route_preempts_persisted_execution_reentry(&primary_route) {
        return primary_route;
    }
    if let Some(persisted_route) =
        persisted_execution_reentry_selection_candidate(runtime_state, route_facts)
    {
        return persisted_route;
    }
    primary_route
}

fn persisted_execution_reentry_selection_candidate(
    runtime_state: &RuntimeState,
    route_facts: &RoutePlanningFacts,
) -> Option<RouteDecision> {
    if !route_facts.persisted_repair_follow_up_is(FOLLOW_UP_EXECUTION_REENTRY) {
        return None;
    }
    let target = route_facts.persisted_reopen_target?;
    Some(execution_reentry_reopen_route_decision(
        runtime_state,
        target,
        route_facts.execution_reentry_target_source.clone(),
    ))
}

fn route_preempts_persisted_execution_reentry(route_decision: &RouteDecision) -> bool {
    public_command_is_begin_or_reopen(route_decision.recommended_public_command.as_ref())
}

fn public_command_is_begin_or_reopen(command: Option<&PublicCommand>) -> bool {
    command
        .map(PublicCommand::kind)
        .is_some_and(|kind| matches!(kind, PublicCommandKind::Begin | PublicCommandKind::Reopen))
}

fn select_runtime_route_decision(
    runtime_state: &RuntimeState,
    route_facts: &RoutePlanningFacts,
    authority_inputs: NextActionAuthorityInputs<'_>,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> RouteDecision {
    let primary_route = select_primary_runtime_route_decision(
        runtime_state,
        route_facts,
        authority_inputs,
        external_review_result_ready,
        require_exact_execution_command,
    );
    select_route_planning_candidate(primary_route, runtime_state, route_facts)
}

fn select_primary_runtime_route_decision(
    runtime_state: &RuntimeState,
    route_facts: &RoutePlanningFacts,
    authority_inputs: NextActionAuthorityInputs<'_>,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> RouteDecision {
    let status = &runtime_state.status;
    let handoff_route_active = status.handoff_required
        || status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == phase::PHASE_HANDOFF_REQUIRED);
    let next_action_decision = next_action_choice::next_action_decision_for_route_plan(
        runtime_state,
        external_review_result_ready,
        authority_inputs,
    );
    if route_facts.review_state_status == REVIEW_STATE_MISSING_CURRENT_CLOSURE
        && route_facts
            .current_task_closure_branch_route_facts
            .set_should_route_to_branch_closure()
        && runtime_state
            .branch_rerecording_assessment
            .as_ref()
            .is_some_and(|assessment| assessment.supported)
        && matches!(
            status.harness_phase,
            HarnessPhase::DocumentReleasePending
                | HarnessPhase::FinalReviewPending
                | HarnessPhase::QaPending
                | HarnessPhase::ReadyForBranchCompletion
                | HarnessPhase::Executing
        )
    {
        return branch_closure_recording_route_decision(runtime_state, status);
    }
    if route_facts.stale_resume_begin_route_candidate
        && let Ok(Some(route_decision)) = route_decision_from_next_action_choice(
            runtime_state,
            route_facts,
            next_action_decision.clone(),
            handoff_route_active,
            external_review_result_ready,
            require_exact_execution_command,
        )
    {
        return route_decision;
    }
    if route_facts.targetless_stale_reconcile_required {
        return runtime_reconcile_route_decision(
            runtime_state,
            status,
            route_facts.projected_stale_repair_task,
            TARGETLESS_STALE_RECONCILE_REASON_CODE,
        );
    }
    if let Some(reason_code) = route_facts
        .task_scope_structural_review_state_reason
        .as_deref()
    {
        return repair_review_state_route_decision(
            runtime_state,
            status,
            route_facts
                .projected_stale_repair_task
                .or_else(|| current_task_closure_route_target(status).map(|target| target.task)),
            reason_code,
            route_facts.execution_reentry_target_source.clone(),
        );
    }
    if let Some(task_number) = route_facts.persisted_close_current_task_bridge_task {
        return close_current_task_or_branch_closure_route_decision(
            runtime_state,
            status,
            route_facts,
            task_number,
        );
    }
    if route_facts.targetless_stale_lacks_concrete_public_target()
        && !route_facts.actionable_stale_reentry_target_bound
        && !route_facts.baseline_bridge_repair_review_state_ready
    {
        return runtime_reconcile_route_decision(
            runtime_state,
            status,
            route_facts.projected_stale_repair_task,
            TARGETLESS_STALE_RECONCILE_REASON_CODE,
        );
    }
    if route_facts.has_repair_review_state_blocking_record()
        && !route_facts.baseline_bridge_repair_review_state_ready
        && !route_facts.actionable_stale_reentry_target_bound
        && !route_facts.negative_result_requires_execution_reentry
    {
        let Some(reason_code) = route_facts.repair_review_state_blocking_reason_code() else {
            return route_decision_for_unroutable_runtime_state(status);
        };
        return repair_review_state_route_decision(
            runtime_state,
            status,
            route_facts.projected_stale_repair_task,
            reason_code,
            route_facts.execution_reentry_target_source.clone(),
        );
    }
    if let Ok(Some(route_decision)) = route_decision_from_next_action_choice(
        runtime_state,
        route_facts,
        next_action_decision,
        handoff_route_active,
        external_review_result_ready,
        require_exact_execution_command,
    ) {
        return route_decision;
    }
    route_decision_for_unroutable_runtime_state(status)
}
