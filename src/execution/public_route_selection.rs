use crate::diagnostics::JsonFailure;
use crate::execution::command_eligibility::PublicAdvanceLateStageMode;
use crate::execution::harness::HarnessPhase;
use crate::execution::next_action::{
    NextActionDecision, NextActionKind, advance_late_stage_public_command,
    close_current_task_public_command, execution_command_route_target_from_decision,
    public_command_from_decision, public_next_action_text, repair_review_state_public_command,
};
use crate::execution::phase;
use crate::execution::query::{
    ExecutionRoutingExecutionCommandContext, ExecutionRoutingRecordingContext,
    WorkflowRoutingDecision, canonical_phase_for_shared_decision, default_phase_for_status,
};
use crate::execution::reducer::RuntimeState;
use crate::execution::state::{ExecutionContext, PlanExecutionStatus};

#[cfg(test)]
pub(crate) fn shared_next_action_seed_from_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    inputs: SharedNextActionRoutingInputs<'_>,
) -> Result<Option<WorkflowRoutingDecision>, JsonFailure> {
    let Some(decision) = crate::execution::router::shared_next_action_decision(
        context,
        status,
        inputs.plan_path,
        inputs.external_review_result_ready,
        inputs.task_review_dispatch_id,
        inputs.final_review_dispatch_id,
        inputs.final_review_dispatch_lineage_present,
    ) else {
        return Ok(None);
    };
    shared_next_action_seed_from_precomputed_decision(context, status, inputs, decision)
}

pub(crate) fn shared_next_action_seed_from_runtime_state(
    runtime_state: &RuntimeState,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<Option<WorkflowRoutingDecision>, JsonFailure> {
    let Some(decision) = crate::execution::router::shared_next_action_decision_from_runtime_state(
        runtime_state,
        external_review_result_ready,
    ) else {
        return Ok(None);
    };
    shared_next_action_seed_from_precomputed_decision(
        &runtime_state.context,
        &runtime_state.status,
        SharedNextActionRoutingInputs {
            plan_path: &runtime_state.context.plan_rel,
            #[cfg(test)]
            external_review_result_ready,
            require_exact_execution_command,
            task_review_dispatch_id: runtime_state.task_review_dispatch_id.as_deref(),
            final_review_dispatch_id: runtime_state
                .final_review_dispatch_authority
                .dispatch_id
                .as_deref(),
            #[cfg(test)]
            final_review_dispatch_lineage_present: runtime_state
                .final_review_dispatch_authority
                .lineage_present,
            current_branch_closure_id: runtime_state.status.current_branch_closure_id.as_deref(),
        },
        decision,
    )
}

pub(crate) struct SharedNextActionRoutingInputs<'a> {
    pub(crate) plan_path: &'a str,
    #[cfg(test)]
    pub(crate) external_review_result_ready: bool,
    pub(crate) require_exact_execution_command: bool,
    pub(crate) task_review_dispatch_id: Option<&'a str>,
    pub(crate) final_review_dispatch_id: Option<&'a str>,
    #[cfg(test)]
    pub(crate) final_review_dispatch_lineage_present: bool,
    pub(crate) current_branch_closure_id: Option<&'a str>,
}

fn shared_next_action_seed_from_precomputed_decision(
    _context: &ExecutionContext,
    status: &PlanExecutionStatus,
    inputs: SharedNextActionRoutingInputs<'_>,
    decision: NextActionDecision,
) -> Result<Option<WorkflowRoutingDecision>, JsonFailure> {
    let default_phase = default_phase_for_shared_seed(status, &decision);
    let mut phase_detail = decision.phase_detail.clone();
    let review_state_status = decision.review_state_status.clone();
    let mut recording_context = None;
    let mut execution_command_context = None;
    let mut next_action = public_next_action_text(&decision);
    let mut recommended_public_command = decision.recommended_public_command.clone();
    let mut blocking_task = decision.blocking_task;
    let task_review_dispatch_id = inputs.task_review_dispatch_id.map(str::to_owned);
    let final_review_dispatch_id = inputs.final_review_dispatch_id.map(str::to_owned);

    let repair_review_state_reentry = decision.kind == NextActionKind::Reopen
        && (next_action == "repair review state / reenter execution"
            || (phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED
                && review_state_status == "missing_current_closure"));
    if repair_review_state_reentry {
        recommended_public_command = Some(repair_review_state_public_command(inputs.plan_path));
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
        let execution_route_target =
            execution_command_route_target_from_decision(status, &decision, inputs.plan_path);
        if decision_requires_exact_execution_command
            && inputs.require_exact_execution_command
            && execution_route_target.is_none()
        {
            return Ok(None);
        }
        if let Some(execution_route_target) = execution_route_target {
            execution_command_context = Some(ExecutionRoutingExecutionCommandContext {
                command_kind: String::from(execution_route_target.command_kind),
                task_number: Some(execution_route_target.task_number),
                step_id: execution_route_target.step_id,
            });
            recommended_public_command =
                public_command_from_decision(status, &decision, inputs.plan_path);
            if decision.kind == NextActionKind::Reopen {
                blocking_task = Some(execution_route_target.task_number);
            }
        }
    }
    if phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && !matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        )
    {
        if decision.kind == NextActionKind::CloseCurrentTask
            && let Some(task_number) = decision.task_number.or(status.blocking_task)
        {
            recording_context = Some(ExecutionRoutingRecordingContext {
                task_number: Some(task_number),
                dispatch_id: task_review_dispatch_id.clone(),
                branch_closure_id: None,
            });
            recommended_public_command = Some(close_current_task_public_command(
                inputs.plan_path,
                task_number,
            ));
            next_action = String::from("close current task");
            blocking_task = Some(task_number);
        } else if review_state_status == "missing_current_closure" {
            recommended_public_command = Some(advance_late_stage_public_command(
                inputs.plan_path,
                PublicAdvanceLateStageMode::Basic,
            ));
            next_action = String::from("advance late stage");
            blocking_task = decision.task_number.or(status.blocking_task);
        } else {
            recommended_public_command = Some(repair_review_state_public_command(inputs.plan_path));
            next_action = String::from("repair review state / reenter execution");
            blocking_task = decision.task_number.or(status.blocking_task);
        }
    } else if phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY {
        if let Some(task_number) = decision.task_number.or(status.blocking_task) {
            recording_context = Some(ExecutionRoutingRecordingContext {
                task_number: Some(task_number),
                dispatch_id: task_review_dispatch_id.clone(),
                branch_closure_id: None,
            });
            recommended_public_command = Some(close_current_task_public_command(
                inputs.plan_path,
                task_number,
            ));
            next_action = String::from("close current task");
            blocking_task = Some(task_number);
        }
    } else if phase_detail == phase::DETAIL_FINAL_REVIEW_RECORDING_READY {
        recording_context = inputs.current_branch_closure_id.map(|branch_closure_id| {
            ExecutionRoutingRecordingContext {
                task_number: None,
                dispatch_id: final_review_dispatch_id.clone(),
                branch_closure_id: Some(branch_closure_id.to_owned()),
            }
        });
        recommended_public_command = Some(advance_late_stage_public_command(
            inputs.plan_path,
            PublicAdvanceLateStageMode::FinalReview,
        ));
        next_action = String::from("advance late stage");
    } else if matches!(
        phase_detail.as_str(),
        phase::DETAIL_RELEASE_READINESS_RECORDING_READY
            | phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
    ) {
        recording_context = inputs.current_branch_closure_id.map(|branch_closure_id| {
            ExecutionRoutingRecordingContext {
                task_number: None,
                dispatch_id: None,
                branch_closure_id: Some(branch_closure_id.to_owned()),
            }
        });
    }
    if phase_detail == phase::DETAIL_TASK_CLOSURE_RECORDING_READY
        && stale_branch_closure_refresh_required(status)
    {
        phase_detail =
            String::from(phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS);
        recording_context = None;
        execution_command_context = None;
        recommended_public_command = Some(advance_late_stage_public_command(
            inputs.plan_path,
            PublicAdvanceLateStageMode::Basic,
        ));
        next_action = String::from("advance late stage");
        blocking_task = None;
    }
    let recommended_command = recommended_public_command
        .as_ref()
        .map(crate::execution::command_eligibility::PublicCommand::to_display_command);

    Ok(Some(WorkflowRoutingDecision {
        phase: canonical_phase_for_shared_decision(default_phase.as_str(), phase_detail.as_str()),
        phase_detail,
        review_state_status,
        recording_context,
        execution_command_context,
        next_action,
        recommended_public_command,
        recommended_command,
        blocking_scope: None,
        blocking_task,
        external_wait_state: None,
        blocking_reason_codes: decision.blocking_reason_codes.clone(),
    }))
}

fn default_phase_for_shared_seed(
    status: &PlanExecutionStatus,
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

fn stale_branch_closure_refresh_required(status: &PlanExecutionStatus) -> bool {
    status.current_branch_closure_id.is_some()
        && status.current_branch_meaningful_drift
        && status.blocking_records.iter().any(|record| {
            record.record_type == "branch_closure"
                && record.review_state_status == "missing_current_closure"
        })
}
