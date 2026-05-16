use crate::diagnostics::JsonFailure;
use crate::execution::current_truth::branch_closure_refresh_missing_current_closure;
use crate::execution::harness::HarnessPhase;
use crate::execution::next_action::{
    NEXT_ACTION_REPAIR_REVIEW_STATE, NextActionDecision, NextActionKind,
};
use crate::execution::phase;
use crate::execution::reducer::RuntimeState;
use crate::execution::reentry_reconcile::TargetlessStaleReconcile;
use crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE;
use crate::execution::status_assembly::effective_route_review_state_status;

use super::final_review_dispatch::final_review_dispatch_route_for_repaired_late_stage_drift;
use super::next_action_finalization::RouteFinalization;
use super::planning_facts::RoutePlanningFacts;
use super::public_action::synthesize_next_public_action;
use super::route_semantics::{
    canonical_phase_for_shared_decision, default_phase_for_status,
    external_wait_state_for_phase_detail,
};
use super::{
    PublicRouteDecision, RouteDecision, close_current_task_or_branch_closure_route_decision,
    command_context_reopens_current_task_closure, compact_route_reason_codes,
    derive_required_follow_up, derive_state_kind_from_seed,
    execution_command_route_target_from_decision, materialize_blocker_actions, merge_reason_codes,
    primary_blocker_for_status, public_command_for_required_follow_up,
    public_route_blocking_reason_codes, repair_review_state_route_decision,
    state_kind_command_marker, targetless_stale_reconcile_blockers,
    targetless_stale_reconcile_for_phase, task_closure_recording_reentry_target_source,
};

enum RouteFinalizationOverride {
    CloseCurrentTaskOrBranchClosure {
        task_number: u32,
    },
    RepairReviewState {
        blocking_task: Option<u32>,
        reason_code: &'static str,
        execution_reentry_target_source: Option<String>,
    },
}

pub(super) fn route_decision_from_next_action_choice(
    runtime_state: &RuntimeState,
    route_facts: &RoutePlanningFacts,
    decision: Option<NextActionDecision>,
    handoff_route_active: bool,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<Option<RouteDecision>, JsonFailure> {
    let Some(decision) = decision else {
        return Ok(None);
    };
    route_decision_from_next_action_decision(
        runtime_state,
        route_facts,
        handoff_route_active,
        external_review_result_ready,
        require_exact_execution_command,
        decision,
    )
}

fn route_decision_for_finalization_override(
    runtime_state: &RuntimeState,
    status: &crate::execution::state::PlanExecutionStatus,
    route_facts: &RoutePlanningFacts,
    override_case: RouteFinalizationOverride,
) -> RouteDecision {
    match override_case {
        RouteFinalizationOverride::CloseCurrentTaskOrBranchClosure { task_number } => {
            close_current_task_or_branch_closure_route_decision(
                runtime_state,
                status,
                route_facts,
                task_number,
            )
        }
        RouteFinalizationOverride::RepairReviewState {
            blocking_task,
            reason_code,
            execution_reentry_target_source,
        } => repair_review_state_route_decision(
            runtime_state,
            status,
            blocking_task,
            reason_code,
            execution_reentry_target_source,
        ),
    }
}

fn route_decision_from_next_action_decision(
    runtime_state: &RuntimeState,
    route_facts: &RoutePlanningFacts,
    handoff_route_active: bool,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
    decision: NextActionDecision,
) -> Result<Option<RouteDecision>, JsonFailure> {
    let status = &runtime_state.status;
    let default_phase = default_phase_for_shared_decision(status, &decision);
    let mut finalization =
        RouteFinalization::from_decision(status, &decision, &runtime_state.context.plan_rel);
    let task_review_dispatch_id = runtime_state.task_review_dispatch_id.clone();
    let final_review_dispatch_id = runtime_state
        .final_review_dispatch_authority
        .dispatch_id
        .clone();

    let repair_review_state_reentry = decision.kind == NextActionKind::Reopen
        && (finalization.next_action == NEXT_ACTION_REPAIR_REVIEW_STATE
            || (finalization.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
                && finalization.review_state_status == REVIEW_STATE_MISSING_CURRENT_CLOSURE));
    if repair_review_state_reentry {
        finalization.bind_repair_review_state_command(&runtime_state.context.plan_rel);
    }
    if finalization.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && finalization.review_state_status == REVIEW_STATE_MISSING_CURRENT_CLOSURE
        && let Some(task_number) = route_facts.baseline_bridge_execution_reentry_task
    {
        finalization.bind_execution_reentry_task_closure_bridge(
            &runtime_state.context.plan_rel,
            task_number,
            task_review_dispatch_id.clone(),
        );
    }

    let decision_requires_exact_execution_command = matches!(
        decision.kind,
        NextActionKind::Begin | NextActionKind::Resume
    ) || (decision.kind == NextActionKind::Reopen
        && !repair_review_state_reentry)
        || (decision.kind == NextActionKind::CloseCurrentTask
            && status.active_task.is_some()
            && status.active_step.is_some());
    if decision_requires_exact_execution_command {
        let execution_route_target = execution_command_route_target_from_decision(
            status,
            &decision,
            &runtime_state.context.plan_rel,
        );
        if require_exact_execution_command && execution_route_target.is_none() {
            return Ok(None);
        }
        if let Some(execution_route_target) = execution_route_target {
            finalization.bind_exact_execution_context(
                status,
                &decision,
                &runtime_state.context.plan_rel,
                execution_route_target,
            );
        }
    }

    if finalization.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && !matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        )
    {
        if decision.kind == NextActionKind::CloseCurrentTask
            && let Some(task_number) = decision.task_number.or(status.blocking_task)
        {
            finalization.bind_task_closure_recording(
                &runtime_state.context.plan_rel,
                task_number,
                task_review_dispatch_id.clone(),
            );
        } else if finalization.review_state_status == REVIEW_STATE_MISSING_CURRENT_CLOSURE {
            finalization.bind_late_stage_command(
                &runtime_state.context.plan_rel,
                phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
                decision.task_number.or(status.blocking_task),
            );
        } else {
            finalization.bind_repair_review_state_route(
                &runtime_state.context.plan_rel,
                decision.task_number.or(status.blocking_task),
            );
        }
    } else if finalization.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY {
        if let Some(task_number) = decision.task_number.or(status.blocking_task) {
            finalization.bind_task_closure_recording(
                &runtime_state.context.plan_rel,
                task_number,
                task_review_dispatch_id.clone(),
            );
        }
    } else if finalization.phase_detail == phase::DETAIL_FINAL_REVIEW_RECORDING_READY {
        finalization.bind_final_review_recording(
            &runtime_state.context.plan_rel,
            final_review_dispatch_id.clone(),
            status.current_branch_closure_id.clone(),
        );
    } else if matches!(
        finalization.phase_detail.as_str(),
        phase::DETAIL_RELEASE_READINESS_RECORDING_READY
            | phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
    ) {
        finalization.bind_branch_stage_recording_context(status.current_branch_closure_id.clone());
    }

    if finalization.phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && branch_closure_refresh_missing_current_closure(status)
    {
        finalization.bind_branch_closure_recording(
            &runtime_state.context.plan_rel,
            phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
        );
    }

    if finalization.phase_detail == phase::DETAIL_PLANNING_REENTRY_REQUIRED
        && status.execution_started == "yes"
        && status.current_branch_closure_id.is_none()
        && !status.reason_codes.iter().any(|code| {
            code == crate::execution::observability::REASON_CODE_BLOCKED_ON_PLAN_REVISION
        })
        && let Some(task_number) = status
            .blocking_task
            .or_else(|| runtime_state.context.tasks_by_number.keys().copied().max())
    {
        return Ok(Some(route_decision_for_finalization_override(
            runtime_state,
            status,
            route_facts,
            RouteFinalizationOverride::CloseCurrentTaskOrBranchClosure { task_number },
        )));
    }

    if let Some(route_decision) = final_review_dispatch_route_for_repaired_late_stage_drift(
        runtime_state,
        route_facts,
        &finalization.phase_detail,
        external_review_result_ready,
    ) {
        return Ok(Some(route_decision));
    }

    if finalization.recommended_public_command.is_none()
        && !phase::RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS
            .contains(&finalization.phase_detail.as_str())
        && TargetlessStaleReconcile::from_phase_and_reason_codes(
            &finalization.phase_detail,
            &decision.blocking_reason_codes,
        )
        .is_none()
        && let Some(follow_up_command) = route_facts.blocking_records.first().and_then(|record| {
            public_command_for_required_follow_up(
                record.required_follow_up.as_deref(),
                &runtime_state.context.plan_rel,
                &finalization.phase_detail,
                Some(record.record_type.as_str()),
            )
        })
    {
        finalization.bind_follow_up_command(follow_up_command);
    }

    let (recommended_command, invocation, template, required_inputs) =
        PublicRouteDecision::command_surfaces(finalization.recommended_public_command.as_ref());
    if !handoff_route_active
        && finalization.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
        && let Some(task_number) = route_facts.completed_task_closure_preemption_task(
            status,
            finalization
                .execution_command_context
                .as_ref()
                .and_then(|context| context.task_number),
        )
    {
        return Ok(Some(route_decision_for_finalization_override(
            runtime_state,
            status,
            route_facts,
            RouteFinalizationOverride::CloseCurrentTaskOrBranchClosure { task_number },
        )));
    }

    let next_public_action = synthesize_next_public_action(
        finalization.recommended_public_command.as_ref(),
        &finalization.phase_detail,
        &runtime_state.context.plan_rel,
    );
    let review_state_status = effective_route_review_state_status(
        status,
        &finalization.phase_detail,
        &finalization.review_state_status,
    );
    let blocking_reason_codes = merge_reason_codes(
        public_route_blocking_reason_codes(
            status,
            &finalization.phase_detail,
            finalization.blocking_task,
            &decision.blocking_reason_codes,
        ),
        compact_route_reason_codes(
            status,
            &finalization.phase_detail,
            &review_state_status,
            finalization.blocking_task,
            None,
        ),
    );
    if command_context_reopens_current_task_closure(
        status,
        finalization.execution_command_context.as_ref(),
    ) {
        return Ok(Some(route_decision_for_finalization_override(
            runtime_state,
            status,
            route_facts,
            RouteFinalizationOverride::RepairReviewState {
                blocking_task: finalization.blocking_task.or_else(|| {
                    finalization
                        .execution_command_context
                        .as_ref()
                        .and_then(|context| context.task_number)
                }),
                reason_code: crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE,
                execution_reentry_target_source: route_facts.execution_reentry_target_source.clone(),
            },
        )));
    }
    let external_wait_state = external_wait_state_for_phase_detail(
        &finalization.phase_detail,
        &blocking_reason_codes,
        external_review_result_ready,
    )
    .or_else(|| status.external_wait_state.clone());
    let state_kind = derive_state_kind_from_seed(
        external_wait_state.as_deref(),
        status.harness_phase,
        &finalization.phase_detail,
        state_kind_command_marker(finalization.recommended_public_command.as_ref()),
    );
    let blockers =
        if targetless_stale_reconcile_for_phase(&finalization.phase_detail, &blocking_reason_codes)
        {
            targetless_stale_reconcile_blockers(&finalization.phase_detail)
        } else {
            let blockers = primary_blocker_for_status(
                status,
                state_kind.as_str(),
                next_public_action.as_ref(),
            );
            materialize_blocker_actions(blockers, &runtime_state.context.plan_rel)
        };
    let required_follow_up = derive_required_follow_up(
        status,
        &finalization.phase_detail,
        &review_state_status,
        blocking_reason_codes.iter().map(String::as_str),
        finalization.execution_command_context.as_ref(),
    );
    let execution_reentry_target_source = (finalization.phase_detail
        == phase::DETAIL_EXECUTION_REENTRY_REQUIRED)
        .then(|| route_facts.execution_reentry_target_source.clone())
        .flatten()
        .or_else(|| {
            task_closure_recording_reentry_target_source(
                &finalization.phase_detail,
                &blocking_reason_codes,
            )
        });
    if let Some(task_number) = route_facts.route_execution_reentry_task_closure_bridge(
        &runtime_state.context,
        status,
        &finalization.phase_detail,
        finalization.blocking_task,
        finalization
            .execution_command_context
            .as_ref()
            .and_then(|context| context.task_number),
    ) {
        return Ok(Some(route_decision_for_finalization_override(
            runtime_state,
            status,
            route_facts,
            RouteFinalizationOverride::CloseCurrentTaskOrBranchClosure { task_number },
        )));
    }

    let mut route_decision = RouteDecision {
        state_kind,
        phase: canonical_phase_for_shared_decision(
            default_phase.as_str(),
            finalization.phase_detail.as_str(),
        ),
        phase_detail: finalization.phase_detail,
        review_state_status,
        next_action: finalization.next_action,
        blocking_reason_codes,
        recommended_command,
        recommended_public_command: finalization.recommended_public_command,
        invocation,
        recommended_public_command_template: template,
        required_inputs,
        required_follow_up,
        next_public_action,
        blockers,
        public_repair_targets: Vec::new(),
        execution_reentry_target_source,
        execution_command_context: finalization.execution_command_context,
        recording_context: finalization.recording_context,
        blocking_scope: None,
        blocking_task: None,
        external_wait_state: None,
    };
    route_decision.apply_public_route_projection(Some(status), external_review_result_ready);
    Ok(Some(route_decision))
}

fn default_phase_for_shared_decision(
    status: &crate::execution::state::PlanExecutionStatus,
    decision: &NextActionDecision,
) -> String {
    if matches!(
        status.harness_phase,
        HarnessPhase::ContractDrafting
            | HarnessPhase::PivotRequired
            | HarnessPhase::HandoffRequired
    ) {
        default_phase_for_status(status)
    } else {
        decision.phase.clone()
    }
}
