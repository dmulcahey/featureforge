use std::collections::BTreeSet;

use crate::execution::resume_stale_precedence::{
    ResumeStalePrecedence, ResumeStalePrecedenceInputs,
};
use crate::execution::review_route_tokens::REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY;
use crate::execution::state::{
    CurrentTaskClosureBranchRouteFacts, PlanExecutionStatus, PublicRepairTarget,
    StatusBlockingRecord, task_scope_structural_review_state_reason,
};

use super::execution_target_authority::legal_execution_begin_route;
use super::finalization_facts::{ExecutionReentryTaskClosureBridgeFacts, PersistedReopenTarget};
use super::stale_repair_target::{
    projected_stale_repair_task, stale_task_scope_lacks_concrete_public_target,
    targetless_stale_has_concrete_public_target,
};

pub(crate) struct RoutePlanningFactInputs<'a> {
    pub(crate) status: &'a PlanExecutionStatus,
    pub(crate) review_state_status: String,
    pub(crate) earliest_stale_task_target: Option<u32>,
    pub(crate) legal_resume_begin_route: bool,
    pub(crate) authoritative_stale_target_bound: bool,
    pub(crate) actionable_stale_reentry_target_bound: bool,
    pub(crate) baseline_bridge_repair_review_state_ready: bool,
    pub(crate) baseline_bridge_close_current_task_candidate: Option<u32>,
    pub(crate) baseline_bridge_execution_reentry_task: Option<u32>,
    pub(crate) execution_reentry_task_closure_bridge_facts: ExecutionReentryTaskClosureBridgeFacts,
    pub(crate) execution_reentry_target_source: Option<String>,
    pub(crate) completed_task_closure_preemption_tasks: BTreeSet<u32>,
    pub(crate) fallback_completed_task_closure_preemption_task: Option<u32>,
    pub(crate) persisted_close_current_task_bridge_task: Option<u32>,
    pub(crate) persisted_reopen_target: Option<PersistedReopenTarget>,
    pub(crate) persisted_repair_follow_up: Option<&'a str>,
    pub(crate) current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoutePlanningFacts {
    pub(crate) earliest_stale_task_target: Option<u32>,
    pub(crate) targetless_stale_reconcile_required: bool,
    pub(crate) baseline_bridge_repair_review_state_ready: bool,
    pub(crate) baseline_bridge_close_current_task_candidate: Option<u32>,
    pub(crate) baseline_bridge_execution_reentry_task: Option<u32>,
    pub(crate) execution_reentry_task_closure_bridge_facts: ExecutionReentryTaskClosureBridgeFacts,
    pub(crate) execution_reentry_target_source: Option<String>,
    pub(crate) completed_task_closure_preemption_tasks: BTreeSet<u32>,
    pub(crate) fallback_completed_task_closure_preemption_task: Option<u32>,
    pub(crate) persisted_close_current_task_bridge_task: Option<u32>,
    pub(crate) persisted_reopen_target: Option<PersistedReopenTarget>,
    pub(crate) exact_resume_stale_task: Option<u32>,
    pub(crate) blocking_records: Vec<StatusBlockingRecord>,
    pub(crate) review_state_status: String,
    pub(crate) persisted_repair_follow_up: Option<String>,
    pub(crate) projected_stale_repair_task: Option<u32>,
    pub(crate) authoritative_stale_target_bound: bool,
    pub(crate) actionable_stale_reentry_target_bound: bool,
    pub(crate) targetless_stale_has_concrete_public_target: bool,
    pub(crate) stale_task_scope_lacks_concrete_public_target: bool,
    pub(crate) stale_resume_begin_route_candidate: bool,
    pub(crate) negative_result_requires_execution_reentry: bool,
    pub(crate) task_scope_structural_review_state_reason: Option<String>,
    pub(crate) current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts,
}

impl RoutePlanningFacts {
    pub(crate) fn from_inputs(inputs: RoutePlanningFactInputs<'_>) -> Self {
        let projected_stale_repair_task = projected_stale_repair_task(inputs.status);
        let targetless_stale_has_concrete_public_target =
            targetless_stale_has_concrete_public_target(
                inputs.status,
                inputs.authoritative_stale_target_bound,
                inputs.actionable_stale_reentry_target_bound,
            );
        let stale_task_scope_lacks_concrete_public_target =
            stale_task_scope_lacks_concrete_public_target(
                inputs.status,
                &inputs.review_state_status,
                inputs.authoritative_stale_target_bound,
                inputs.actionable_stale_reentry_target_bound,
            );
        let resume_stale_precedence =
            ResumeStalePrecedence::from_inputs(ResumeStalePrecedenceInputs {
                status: inputs.status,
                review_state_status: &inputs.review_state_status,
                open_step_task: None,
                authoritative_stale_boundary: None,
                baseline_stale_boundary_task: None,
                exact_resume_stale_task_target: inputs.earliest_stale_task_target,
                stale_preemption_target: None,
                legal_resume_begin_route: inputs.legal_resume_begin_route,
                targetless_stale_has_concrete_public_target,
            });
        let exact_resume_stale_task = resume_stale_precedence.exact_resume_stale_task;
        let stale_resume_begin_route_candidate =
            resume_stale_precedence.stale_resume_begin_route_candidate;
        let targetless_stale_reconcile_required =
            resume_stale_precedence.targetless_stale_reconcile_required;
        let negative_result_requires_execution_reentry = inputs
            .status
            .reason_codes
            .iter()
            .any(|code| code == REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY);
        let task_scope_structural_review_state_reason =
            task_scope_structural_review_state_reason(inputs.status).map(str::to_owned);

        Self {
            earliest_stale_task_target: inputs.earliest_stale_task_target,
            targetless_stale_reconcile_required,
            baseline_bridge_repair_review_state_ready: inputs
                .baseline_bridge_repair_review_state_ready,
            baseline_bridge_close_current_task_candidate: inputs
                .baseline_bridge_close_current_task_candidate,
            baseline_bridge_execution_reentry_task: inputs.baseline_bridge_execution_reentry_task,
            execution_reentry_task_closure_bridge_facts: inputs
                .execution_reentry_task_closure_bridge_facts,
            execution_reentry_target_source: inputs.execution_reentry_target_source,
            completed_task_closure_preemption_tasks: inputs.completed_task_closure_preemption_tasks,
            fallback_completed_task_closure_preemption_task: inputs
                .fallback_completed_task_closure_preemption_task,
            persisted_close_current_task_bridge_task: inputs
                .persisted_close_current_task_bridge_task,
            persisted_reopen_target: inputs.persisted_reopen_target,
            exact_resume_stale_task,
            blocking_records: inputs.status.blocking_records.clone(),
            review_state_status: inputs.review_state_status,
            persisted_repair_follow_up: inputs.persisted_repair_follow_up.map(str::to_owned),
            projected_stale_repair_task,
            authoritative_stale_target_bound: inputs.authoritative_stale_target_bound,
            actionable_stale_reentry_target_bound: inputs.actionable_stale_reentry_target_bound,
            targetless_stale_has_concrete_public_target,
            stale_task_scope_lacks_concrete_public_target,
            stale_resume_begin_route_candidate,
            negative_result_requires_execution_reentry,
            task_scope_structural_review_state_reason,
            current_task_closure_branch_route_facts: inputs.current_task_closure_branch_route_facts,
        }
    }
}

pub(crate) fn legal_resume_begin_route(
    status: &PlanExecutionStatus,
    plan_path: &str,
    route_repair_target_candidates: &[PublicRepairTarget],
) -> bool {
    legal_execution_begin_route(status, plan_path, route_repair_target_candidates)
}
