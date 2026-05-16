use crate::execution::repair_route_decision::{
    ExecutionReentryTaskClosureBridgeInputs, execution_reentry_task_closure_bridge_route_task,
};
use crate::execution::review_route_tokens::{
    FOLLOW_UP_REPAIR_REVIEW_STATE, REVIEW_STATE_STALE_UNREVIEWED,
};
use crate::execution::state::{ExecutionContext, PlanExecutionStatus, PublicRepairTarget};

use super::planning_facts::RoutePlanningFacts;
use crate::execution::stale_target_projection::AuthoritativeStaleTarget;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ExecutionReentryTaskClosureBridgeFacts {
    pub(crate) earliest_task_stale_target: Option<AuthoritativeStaleTarget>,
    pub(crate) close_current_task_repair_targets: Vec<PublicRepairTarget>,
    pub(crate) task_review_dispatch_id_present: bool,
    pub(crate) baseline_bridge_route_ready_for_blocking_task: bool,
}

impl ExecutionReentryTaskClosureBridgeFacts {
    fn route_task(
        &self,
        context: &ExecutionContext,
        status: &PlanExecutionStatus,
        phase_detail: &str,
        seed_blocking_task: Option<u32>,
        command_context_task: Option<u32>,
    ) -> Option<u32> {
        execution_reentry_task_closure_bridge_route_task(ExecutionReentryTaskClosureBridgeInputs {
            context,
            status,
            phase_detail,
            seed_blocking_task,
            command_context_task,
            earliest_task_stale_target: self.earliest_task_stale_target.as_ref(),
            close_current_task_repair_targets: &self.close_current_task_repair_targets,
            task_review_dispatch_id_present: self.task_review_dispatch_id_present,
            baseline_bridge_route_ready_for_blocking_task: self
                .baseline_bridge_route_ready_for_blocking_task,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PersistedReopenTarget {
    pub(crate) task_number: u32,
    pub(crate) step_number: u32,
}

impl RoutePlanningFacts {
    pub(crate) fn has_repair_review_state_blocking_record(&self) -> bool {
        self.blocking_records.iter().any(|record| {
            record.record_type == "review_state"
                && record.required_follow_up.as_deref() == Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
        })
    }

    pub(crate) fn repair_review_state_blocking_reason_code(&self) -> Option<&str> {
        self.blocking_records
            .iter()
            .find(|record| {
                record.record_type == "review_state"
                    && record.required_follow_up.as_deref() == Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
            })
            .map(|record| record.code.as_str())
    }

    pub(crate) fn persisted_repair_follow_up(&self) -> Option<&str> {
        self.persisted_repair_follow_up.as_deref()
    }

    pub(crate) fn persisted_repair_follow_up_is(&self, expected: &str) -> bool {
        self.persisted_repair_follow_up() == Some(expected)
    }

    pub(crate) fn targetless_stale_lacks_concrete_public_target(&self) -> bool {
        self.review_state_status == REVIEW_STATE_STALE_UNREVIEWED
            && !self.actionable_stale_reentry_target_bound
            && !self.targetless_stale_has_concrete_public_target
            && (!self.authoritative_stale_target_bound
                || self.stale_task_scope_lacks_concrete_public_target
                || self.stale_resume_begin_route_candidate)
    }

    pub(crate) fn route_execution_reentry_task_closure_bridge(
        &self,
        context: &ExecutionContext,
        status: &PlanExecutionStatus,
        phase_detail: &str,
        seed_blocking_task: Option<u32>,
        command_context_task: Option<u32>,
    ) -> Option<u32> {
        self.execution_reentry_task_closure_bridge_facts.route_task(
            context,
            status,
            phase_detail,
            seed_blocking_task,
            command_context_task,
        )
    }

    pub(crate) fn completed_task_closure_preemption_task(
        &self,
        status: &PlanExecutionStatus,
        preferred_task: Option<u32>,
    ) -> Option<u32> {
        let task_number = preferred_task
            .or(status.active_task)
            .or(status.resume_task)
            .or(status.blocking_task)
            .or(self.fallback_completed_task_closure_preemption_task)?;
        self.completed_task_closure_preemption_tasks
            .contains(&task_number)
            .then_some(task_number)
    }
}
