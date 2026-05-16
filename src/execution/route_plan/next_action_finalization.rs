use crate::execution::command_eligibility::{
    PublicCommand, public_advance_late_stage_command_for_phase_detail,
};
use crate::execution::next_action::{
    NEXT_ACTION_ADVANCE_LATE_STAGE, NEXT_ACTION_CLOSE_CURRENT_TASK,
    NEXT_ACTION_REPAIR_REVIEW_STATE, NextActionDecision, NextActionKind, public_next_action_text,
};
use crate::execution::phase;
use crate::execution::query::{
    ExecutionRoutingExecutionCommandContext, ExecutionRoutingRecordingContext,
};
use crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED;
use crate::execution::state::PlanExecutionStatus;

use super::execution_targets::ExecutionCommandRouteTarget;
use super::public_commands::{
    close_current_task_public_command, public_command_from_decision,
    repair_review_state_public_command,
};

pub(super) struct RouteFinalization {
    pub(super) phase_detail: String,
    pub(super) review_state_status: String,
    pub(super) recording_context: Option<ExecutionRoutingRecordingContext>,
    pub(super) execution_command_context: Option<ExecutionRoutingExecutionCommandContext>,
    pub(super) next_action: String,
    pub(super) recommended_public_command: Option<PublicCommand>,
    pub(super) blocking_task: Option<u32>,
}

impl RouteFinalization {
    pub(super) fn from_decision(
        status: &PlanExecutionStatus,
        decision: &NextActionDecision,
        plan_path: &str,
    ) -> Self {
        Self {
            phase_detail: decision.phase_detail.clone(),
            review_state_status: decision.review_state_status.clone(),
            recording_context: None,
            execution_command_context: None,
            next_action: public_next_action_text(decision),
            recommended_public_command: public_command_from_decision(status, decision, plan_path),
            blocking_task: decision.blocking_task,
        }
    }

    pub(super) fn bind_repair_review_state_command(&mut self, plan_path: &str) {
        self.recommended_public_command = Some(repair_review_state_public_command(plan_path));
    }

    pub(super) fn bind_task_closure_recording(
        &mut self,
        plan_path: &str,
        task_number: u32,
        task_review_dispatch_id: Option<String>,
    ) {
        self.recording_context = Some(ExecutionRoutingRecordingContext {
            task_number: Some(task_number),
            dispatch_id: task_review_dispatch_id,
            branch_closure_id: None,
        });
        self.recommended_public_command =
            Some(close_current_task_public_command(plan_path, task_number));
        self.next_action = String::from(NEXT_ACTION_CLOSE_CURRENT_TASK);
        self.blocking_task = Some(task_number);
    }

    pub(super) fn bind_execution_reentry_task_closure_bridge(
        &mut self,
        plan_path: &str,
        task_number: u32,
        task_review_dispatch_id: Option<String>,
    ) {
        self.phase_detail = String::from(phase::DETAIL_TASK_CLOSURE_RECORDING_READY);
        self.review_state_status = String::from(REVIEW_STATE_STALE_UNREVIEWED);
        self.bind_task_closure_recording(plan_path, task_number, task_review_dispatch_id);
    }

    pub(super) fn bind_branch_closure_recording(&mut self, plan_path: &str, phase_detail: &str) {
        self.phase_detail = String::from(phase_detail);
        self.recording_context = None;
        self.execution_command_context = None;
        self.recommended_public_command =
            public_advance_late_stage_command_for_phase_detail(plan_path, phase_detail);
        self.next_action = String::from(NEXT_ACTION_ADVANCE_LATE_STAGE);
        self.blocking_task = None;
    }

    pub(super) fn bind_late_stage_command(
        &mut self,
        plan_path: &str,
        phase_detail: &str,
        blocking_task: Option<u32>,
    ) {
        self.recommended_public_command =
            public_advance_late_stage_command_for_phase_detail(plan_path, phase_detail);
        self.next_action = String::from(NEXT_ACTION_ADVANCE_LATE_STAGE);
        self.blocking_task = blocking_task;
    }

    pub(super) fn bind_repair_review_state_route(
        &mut self,
        plan_path: &str,
        blocking_task: Option<u32>,
    ) {
        self.recommended_public_command = Some(repair_review_state_public_command(plan_path));
        self.next_action = String::from(NEXT_ACTION_REPAIR_REVIEW_STATE);
        self.blocking_task = blocking_task;
    }

    pub(super) fn bind_exact_execution_context(
        &mut self,
        status: &PlanExecutionStatus,
        decision: &NextActionDecision,
        plan_path: &str,
        route_target: ExecutionCommandRouteTarget,
    ) {
        self.execution_command_context = Some(ExecutionRoutingExecutionCommandContext {
            command_kind: String::from(route_target.command_kind()),
            task_number: Some(route_target.task_number),
            step_id: route_target.step_id,
        });
        self.recommended_public_command = public_command_from_decision(status, decision, plan_path);
        if decision.kind == NextActionKind::Reopen {
            self.blocking_task = Some(route_target.task_number);
        }
    }

    pub(super) fn bind_final_review_recording(
        &mut self,
        plan_path: &str,
        final_review_dispatch_id: Option<String>,
        branch_closure_id: Option<String>,
    ) {
        self.recording_context =
            branch_closure_id.map(|branch_closure_id| ExecutionRoutingRecordingContext {
                task_number: None,
                dispatch_id: final_review_dispatch_id,
                branch_closure_id: Some(branch_closure_id),
            });
        self.recommended_public_command = public_advance_late_stage_command_for_phase_detail(
            plan_path,
            phase::DETAIL_FINAL_REVIEW_RECORDING_READY,
        );
        self.next_action = String::from(NEXT_ACTION_ADVANCE_LATE_STAGE);
    }

    pub(super) fn bind_branch_stage_recording_context(
        &mut self,
        branch_closure_id: Option<String>,
    ) {
        self.recording_context =
            branch_closure_id.map(|branch_closure_id| ExecutionRoutingRecordingContext {
                task_number: None,
                dispatch_id: None,
                branch_closure_id: Some(branch_closure_id),
            });
    }

    pub(super) fn bind_follow_up_command(&mut self, command: PublicCommand) {
        self.recommended_public_command = Some(command);
    }
}
