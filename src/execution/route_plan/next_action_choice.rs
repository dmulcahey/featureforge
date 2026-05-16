mod execution_ordering;
mod execution_routes;
mod late_stage_public_routes;
mod late_stage_repair_routes;
mod late_stage_routes;
mod types;

use self::execution_ordering::{
    closure_prerequisite_route, completed_execution_missing_branch_closure_route,
    execution_route_facts, first_unchecked_step_route, hard_structural_corruption_route,
    missing_authoritative_stale_reentry_target_route, open_step_route, stale_boundary_route,
};
use self::late_stage_public_routes::{LateStageRouteInputs, late_stage_decision};
use self::late_stage_routes::{branch_rerecording_route, late_stage_milestone_route};
pub use self::types::PUBLIC_NEXT_ACTION_VALUES;
pub(crate) use self::types::{
    NEXT_ACTION_ADVANCE_LATE_STAGE, NEXT_ACTION_CLOSE_CURRENT_TASK,
    NEXT_ACTION_EXECUTION_REENTRY_REQUIRED, NEXT_ACTION_HANDOFF, NEXT_ACTION_PLANNING_REENTRY,
    NEXT_ACTION_REPAIR_REVIEW_STATE, NEXT_ACTION_REQUEST_FINAL_REVIEW,
    NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED, NEXT_ACTION_WAIT_FOR_EXTERNAL_REVIEW_RESULT,
    NextActionDecision, NextActionKind, NextActionRequestInputs, canonical_review_state_status,
    diagnostic_next_action_for_route, public_next_action_text, runtime_route_is_diagnostic,
};
#[cfg(test)]
use crate::execution::current_truth::{
    branch_closure_rerecording_assessment_with_authority,
    resolve_actionable_repair_follow_up_for_status,
};
#[cfg(test)]
use crate::execution::public_repair_targets::public_repair_target_candidates_from_authority;
use crate::execution::reducer::RuntimeState;
#[cfg(test)]
pub(crate) use crate::execution::repair_target_selection::AuthoritativeStaleReentryTarget;
pub(crate) use crate::execution::repair_target_selection::NextActionAuthorityInputs;
use crate::execution::state::{ExecutionContext, PlanExecutionStatus};
#[cfg(test)]
use crate::execution::transitions::load_authoritative_transition_state;

#[cfg(test)]
pub(crate) fn compute_next_action_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
) -> Option<NextActionDecision> {
    compute_next_action_decision_with_inputs(context, status, plan_path, false, None, None, false)
}

#[cfg(test)]
pub(crate) fn next_action_decision_for_tests(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    external_review_result_ready: bool,
    task_review_dispatch_id: Option<&str>,
    final_review_dispatch_id: Option<&str>,
    final_review_dispatch_lineage_present: bool,
) -> Option<NextActionDecision> {
    compute_next_action_decision_with_authority_inputs(
        context,
        status,
        NextActionRequestInputs {
            plan_path,
            external_review_result_ready,
            task_review_dispatch_id,
            final_review_dispatch_id,
            final_review_dispatch_lineage_present,
            final_review_outcome_recorded_for_current_dispatch: false,
        },
        NextActionAuthorityInputs::default(),
    )
}

pub(in crate::execution::route_plan) fn next_action_decision_for_route_plan(
    runtime_state: &RuntimeState,
    external_review_result_ready: bool,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<NextActionDecision> {
    compute_next_action_decision_with_authority_inputs(
        &runtime_state.context,
        &runtime_state.status,
        NextActionRequestInputs {
            plan_path: &runtime_state.context.plan_rel,
            external_review_result_ready,
            task_review_dispatch_id: runtime_state.task_review_dispatch_id.as_deref(),
            final_review_dispatch_id: runtime_state
                .final_review_dispatch_authority
                .dispatch_id
                .as_deref(),
            final_review_dispatch_lineage_present: runtime_state
                .final_review_dispatch_authority
                .lineage_present,
            final_review_outcome_recorded_for_current_dispatch: runtime_state
                .final_review_outcome_recorded_for_current_dispatch,
        },
        authority_inputs,
    )
}

#[cfg(test)]
pub(crate) fn compute_next_action_decision_with_inputs(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    external_review_result_ready: bool,
    task_review_dispatch_id: Option<&str>,
    final_review_dispatch_id: Option<&str>,
    final_review_dispatch_lineage_present: bool,
) -> Option<NextActionDecision> {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    let persisted_repair_follow_up = resolve_actionable_repair_follow_up_for_status(
        context,
        status,
        authoritative_state.as_ref(),
    )
    .map(|record| record.kind.public_token().to_owned());
    let branch_rerecording_assessment =
        branch_closure_rerecording_assessment_with_authority(context, authoritative_state.as_ref())
            .ok();
    let route_repair_target_candidates = public_repair_target_candidates_from_authority(
        context,
        status,
        authoritative_state.as_ref(),
        None,
    );
    compute_next_action_decision_with_authority_inputs(
        context,
        status,
        NextActionRequestInputs {
            plan_path,
            external_review_result_ready,
            task_review_dispatch_id,
            final_review_dispatch_id,
            final_review_dispatch_lineage_present,
            final_review_outcome_recorded_for_current_dispatch: false,
        },
        NextActionAuthorityInputs {
            persisted_repair_follow_up: persisted_repair_follow_up.as_deref(),
            branch_rerecording_assessment: branch_rerecording_assessment.as_ref(),
            route_repair_target_candidates: &route_repair_target_candidates,
            ..NextActionAuthorityInputs::default()
        },
    )
}

fn compute_next_action_decision_with_authority_inputs(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    request_inputs: NextActionRequestInputs<'_>,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<NextActionDecision> {
    let authority_inputs = authority_inputs.with_current_task_closure_branch_route_facts(
        authority_inputs.current_task_closure_branch_route_facts_or_derive(context, status),
    );
    let NextActionRequestInputs {
        plan_path,
        external_review_result_ready,
        task_review_dispatch_id,
        final_review_dispatch_id,
        final_review_dispatch_lineage_present,
        final_review_outcome_recorded_for_current_dispatch,
    } = request_inputs;
    let review_state_status = canonical_review_state_status(status);

    if let Some(decision) = missing_authoritative_stale_reentry_target_route(
        status,
        review_state_status.as_str(),
        authority_inputs,
    ) {
        return Some(decision);
    }
    let execution_facts = execution_route_facts(
        context,
        status,
        plan_path,
        review_state_status.as_str(),
        authority_inputs,
    );
    if let Some(decision) = hard_structural_corruption_route(
        context,
        status,
        plan_path,
        review_state_status.as_str(),
        authority_inputs,
        &execution_facts,
    ) {
        return Some(decision);
    }
    if let Some(decision) = completed_execution_missing_branch_closure_route(
        context,
        status,
        plan_path,
        review_state_status.as_str(),
        authority_inputs,
        &execution_facts,
    ) {
        return Some(decision);
    }
    if let Some(decision) = branch_rerecording_route(
        context,
        status,
        plan_path,
        review_state_status.as_str(),
        authority_inputs,
        &execution_facts,
    ) {
        return Some(decision);
    }

    if let Some(decision) = open_step_route(
        context,
        status,
        plan_path,
        review_state_status.as_str(),
        authority_inputs,
        &execution_facts,
    ) {
        return Some(decision);
    }
    if let Some(decision) = stale_boundary_route(
        context,
        status,
        plan_path,
        review_state_status.as_str(),
        authority_inputs,
        &execution_facts,
    ) {
        return Some(decision);
    }
    if let Some(decision) = closure_prerequisite_route(
        context,
        status,
        plan_path,
        review_state_status.as_str(),
        authority_inputs,
        task_review_dispatch_id,
        external_review_result_ready,
    ) {
        return Some(decision);
    }
    if let Some(decision) = late_stage_milestone_route(
        LateStageRouteInputs {
            context,
            status,
            plan_path,
            external_review_result_ready,
            final_review_dispatch_id,
            final_review_dispatch_lineage_present,
            final_review_outcome_recorded_for_current_dispatch,
            gate_finish: authority_inputs.gate_finish,
            current_task_closure_branch_route_facts: execution_facts
                .current_task_closure_branch_route_facts,
        },
        review_state_status.as_str(),
        authority_inputs,
    ) {
        return Some(decision);
    }
    if let Some(decision) = first_unchecked_step_route(
        context,
        status,
        plan_path,
        review_state_status.as_str(),
        &execution_facts,
    ) {
        return Some(decision);
    }

    Some(late_stage_decision(
        status,
        NextActionKind::PlanningReentry,
        crate::execution::phase::DETAIL_PLANNING_REENTRY_REQUIRED,
        plan_path,
    ))
}

#[cfg(test)]
mod tests;
