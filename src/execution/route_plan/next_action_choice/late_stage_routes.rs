use crate::execution::harness::HarnessPhase;
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::phase;
use crate::execution::repair_target_selection::NextActionAuthorityInputs;
use crate::execution::state::{ExecutionContext, PlanExecutionStatus};
use crate::execution::status_assembly::effective_route_review_state_status;

use super::execution_ordering::ExecutionRouteFacts;
use super::execution_routes::{
    ExecutionReentryDecisionInputs, execution_reentry_blocking_task,
    execution_reentry_decision_for_task, execution_repair_decision,
};
use super::late_stage_public_routes::{
    LateStageRouteInputs, late_stage_decision, select_late_stage_public_route,
};
use super::late_stage_repair_routes::{
    late_stage_missing_current_closure_decision_from_assessment,
    late_stage_planning_reentry_decision, persisted_late_stage_reroute_missing_current_closure,
    stale_late_stage_repair_decision,
};
use super::{NextActionDecision, NextActionKind};

pub(super) fn branch_rerecording_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
    facts: &ExecutionRouteFacts,
) -> Option<NextActionDecision> {
    if facts.legal_begin_route_active {
        return None;
    }
    let persisted_follow_up = authority_inputs
        .persisted_repair_follow_up
        .map(str::to_owned);
    let persisted_branch_rerecording_follow_up = facts
        .current_task_closure_branch_route_facts
        .branch_closure_recorded()
        && persisted_follow_up.as_deref()
            == Some(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE);
    if (review_state_status != crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        || persisted_follow_up.as_deref()
            == Some(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE))
        && (status.current_branch_meaningful_drift || persisted_branch_rerecording_follow_up)
        && let Some(assessment) = authority_inputs.branch_rerecording_assessment
    {
        let branch_review_state_status = effective_route_review_state_status(
            status,
            phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            review_state_status,
        );
        return Some(late_stage_missing_current_closure_decision_from_assessment(
            context,
            status,
            plan_path,
            branch_review_state_status.as_str(),
            assessment,
            authority_inputs,
        ));
    }
    None
}

pub(super) fn late_stage_milestone_route(
    inputs: LateStageRouteInputs<'_>,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<NextActionDecision> {
    let LateStageRouteInputs {
        context,
        status,
        plan_path,
        external_review_result_ready,
        final_review_dispatch_id,
        final_review_dispatch_lineage_present,
        final_review_outcome_recorded_for_current_dispatch,
        gate_finish,
        current_task_closure_branch_route_facts,
    } = inputs;
    if task_scope_handoff_override_active(status) {
        return Some(task_scope_handoff_decision(
            status,
            plan_path,
            review_state_status,
        ));
    }
    if persisted_late_stage_reroute_missing_current_closure(
        context,
        status,
        authority_inputs,
        review_state_status,
    ) {
        return Some(late_stage_decision(
            status,
            NextActionKind::AdvanceLateStage,
            phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            plan_path,
        ));
    }
    if task_scope_pivot_override_active(status, review_state_status) {
        return Some(late_stage_planning_reentry_decision(
            status,
            review_state_status,
        ));
    }
    if let Some(decision) = missing_current_closure_late_stage_route(
        context,
        status,
        plan_path,
        review_state_status,
        authority_inputs,
    ) {
        return Some(decision);
    }
    if review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED {
        return Some(stale_late_stage_repair_decision(
            context,
            status,
            plan_path,
            authority_inputs,
        ));
    }
    select_late_stage_public_route(LateStageRouteInputs {
        context,
        status,
        plan_path,
        external_review_result_ready,
        final_review_dispatch_id,
        final_review_dispatch_lineage_present,
        final_review_outcome_recorded_for_current_dispatch,
        gate_finish,
        current_task_closure_branch_route_facts,
    })
}

fn missing_current_closure_late_stage_route(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<NextActionDecision> {
    let current_task_closure_branch_route_facts =
        authority_inputs.precomputed_current_task_closure_branch_route_facts();
    if review_state_status
        != crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
    {
        return None;
    }
    if status.phase_detail == phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
        || status.harness_phase == HarnessPhase::DocumentReleasePending
    {
        if current_task_closure_branch_route_facts.missing_branch_closure() {
            if let Some(assessment) = authority_inputs.branch_rerecording_assessment {
                return Some(late_stage_missing_current_closure_decision_from_assessment(
                    context,
                    status,
                    plan_path,
                    review_state_status,
                    assessment,
                    authority_inputs,
                ));
            }
            return Some(execution_repair_decision(
                context,
                status,
                plan_path,
                review_state_status,
                authority_inputs,
            ));
        }
        return Some(late_stage_decision(
            status,
            NextActionKind::AdvanceLateStage,
            phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            plan_path,
        ));
    }
    if current_task_closure_branch_route_facts.missing_branch_closure()
        && status.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
    {
        if let Some(assessment) = authority_inputs.branch_rerecording_assessment {
            return Some(late_stage_missing_current_closure_decision_from_assessment(
                context,
                status,
                plan_path,
                review_state_status,
                assessment,
                authority_inputs,
            ));
        }
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status,
            authority_inputs,
        ));
    }
    if let Some(task_number) = execution_reentry_blocking_task(context, status, authority_inputs) {
        return Some(execution_reentry_decision_for_task(
            context,
            status,
            plan_path,
            review_state_status,
            task_number,
            ExecutionReentryDecisionInputs {
                current_task_closure_branch_route_facts,
                authority_inputs,
                stale_boundary_route: false,
            },
        ));
    }
    let task_scope_structural_blocker = status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID
                | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED
                | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE
        )
    });
    if task_scope_structural_blocker {
        return Some(execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status,
            authority_inputs,
        ));
    }
    if let Some(assessment) = authority_inputs.branch_rerecording_assessment {
        return Some(late_stage_missing_current_closure_decision_from_assessment(
            context,
            status,
            plan_path,
            review_state_status,
            assessment,
            authority_inputs,
        ));
    }
    Some(execution_repair_decision(
        context,
        status,
        plan_path,
        review_state_status,
        authority_inputs,
    ))
}

pub(super) fn task_scope_handoff_override_active(status: &PlanExecutionStatus) -> bool {
    status.harness_phase == HarnessPhase::HandoffRequired
}

pub(super) fn task_scope_pivot_override_active(
    status: &PlanExecutionStatus,
    review_state_status: &str,
) -> bool {
    let blocker_free_prestart_contract_phase = status.execution_started != "yes"
        && matches!(
            status.harness_phase,
            HarnessPhase::ContractDrafting
                | HarnessPhase::ContractPendingApproval
                | HarnessPhase::ContractApproved
                | HarnessPhase::Evaluating
        )
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && status.blocking_task.is_none()
        && status.blocking_step.is_none()
        && status.reason_codes.is_empty();
    if blocker_free_prestart_contract_phase {
        return false;
    }
    if status.execution_started != "yes"
        && !matches!(
            status.harness_phase,
            HarnessPhase::PivotRequired
                | HarnessPhase::ContractDrafting
                | HarnessPhase::ContractPendingApproval
                | HarnessPhase::ContractApproved
                | HarnessPhase::Evaluating
        )
    {
        return false;
    }
    if review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED {
        return false;
    }
    if status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
    {
        return false;
    }
    matches!(
        status.harness_phase,
        HarnessPhase::PivotRequired
            | HarnessPhase::ContractDrafting
            | HarnessPhase::ContractPendingApproval
            | HarnessPhase::ContractApproved
            | HarnessPhase::Evaluating
    )
}

pub(super) fn task_scope_handoff_decision(
    status: &PlanExecutionStatus,
    _plan_path: &str,
    review_state_status: &str,
) -> NextActionDecision {
    let task_scoped_handoff = status.blocking_task.is_some()
        || status.active_task.is_some()
        || status.resume_task.is_some();
    NextActionDecision {
        kind: NextActionKind::Handoff,
        phase: if task_scoped_handoff {
            String::from(phase::PHASE_EXECUTING)
        } else {
            String::from(phase::PHASE_HANDOFF_REQUIRED)
        },
        phase_detail: String::from(phase::DETAIL_HANDOFF_RECORDING_REQUIRED),
        review_state_status: review_state_status.to_owned(),
        task_number: status
            .blocking_task
            .or(status.resume_task)
            .or(status.active_task),
        step_number: status
            .blocking_step
            .or(status.resume_step)
            .or(status.active_step),
        blocking_task: status
            .blocking_task
            .or(status.resume_task)
            .or(status.active_task),
        blocking_reason_codes: status.reason_codes.clone(),
    }
}
