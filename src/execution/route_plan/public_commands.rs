use crate::execution::command_eligibility::{
    PublicCommand, PublicCommandKind, public_advance_late_stage_command_for_phase_detail,
};
use crate::execution::current_truth::handoff_decision_scope;
use crate::execution::next_action::{NextActionDecision, NextActionKind};
#[cfg(test)]
use crate::execution::state::ExecutionContext;
use crate::execution::state::{PlanExecutionStatus, recommended_execution_source};

use super::execution_targets::ExecutionCommandRouteTarget;
#[cfg(test)]
use super::next_action_choice::compute_next_action_decision;

pub(crate) fn repair_review_state_public_command(plan_path: &str) -> PublicCommand {
    PublicCommand::RepairReviewState {
        plan: plan_path.to_owned(),
    }
}

pub(crate) fn close_current_task_public_command(
    plan_path: &str,
    task_number: u32,
) -> PublicCommand {
    PublicCommand::CloseCurrentTask {
        plan: plan_path.to_owned(),
        task: Some(task_number),
        result_inputs_required: true,
    }
}

pub(crate) fn transfer_handoff_public_command(plan_path: &str, scope: &str) -> PublicCommand {
    PublicCommand::TransferHandoff {
        plan: plan_path.to_owned(),
        scope: scope.to_owned(),
    }
}

fn begin_public_command(
    plan_path: &str,
    task_number: u32,
    step_number: u32,
    execution_mode: Option<&str>,
    fingerprint: &str,
) -> PublicCommand {
    PublicCommand::Begin {
        plan: plan_path.to_owned(),
        task: task_number,
        step: step_number,
        execution_mode: execution_mode.map(str::to_owned),
        fingerprint: Some(fingerprint.to_owned()),
    }
}

fn complete_public_command(
    plan_path: &str,
    task_number: u32,
    step_number: u32,
    source: &str,
    fingerprint: &str,
) -> PublicCommand {
    PublicCommand::Complete {
        plan: plan_path.to_owned(),
        task: task_number,
        step: step_number,
        source: Some(source.to_owned()),
        fingerprint: Some(fingerprint.to_owned()),
    }
}

pub(crate) fn reopen_public_command(
    plan_path: &str,
    task_number: u32,
    step_number: u32,
    source: &str,
    fingerprint: &str,
) -> PublicCommand {
    PublicCommand::Reopen {
        plan: plan_path.to_owned(),
        task: task_number,
        step: step_number,
        source: Some(source.to_owned()),
        reason: Some(runtime_routed_reopen_reason(task_number, step_number)),
        fingerprint: Some(fingerprint.to_owned()),
    }
}

pub(crate) fn reopen_public_command_with_reason(
    plan_path: &str,
    task_number: u32,
    step_number: u32,
    source: &str,
    reason: &str,
    fingerprint: Option<&str>,
) -> PublicCommand {
    PublicCommand::Reopen {
        plan: plan_path.to_owned(),
        task: task_number,
        step: step_number,
        source: Some(source.to_owned()),
        reason: Some(reason.to_owned()),
        fingerprint: fingerprint.map(str::to_owned),
    }
}

fn runtime_routed_reopen_reason(task_number: u32, step_number: u32) -> String {
    format!("runtime-routed-execution-reentry-task-{task_number}-step-{step_number}")
}

pub(crate) fn execution_command_route_target_from_decision(
    status: &PlanExecutionStatus,
    decision: &NextActionDecision,
    plan_path: &str,
) -> Option<ExecutionCommandRouteTarget> {
    let kind = match decision.kind {
        NextActionKind::Begin | NextActionKind::Resume => PublicCommandKind::Begin,
        NextActionKind::Reopen => PublicCommandKind::Reopen,
        NextActionKind::CloseCurrentTask => PublicCommandKind::Complete,
        _ => return None,
    };
    let task_number = decision.task_number?;
    let step_id = decision.step_number;
    if kind != PublicCommandKind::Complete && step_id.is_none() {
        return None;
    }
    if kind == PublicCommandKind::Complete
        && (status.active_task.is_none() || status.active_step.is_none())
    {
        return None;
    }
    public_command_from_decision(status, decision, plan_path)?;
    Some(ExecutionCommandRouteTarget {
        kind,
        task_number,
        step_id,
    })
}

#[cfg(test)]
pub(crate) fn execution_command_route_target_from_status_context(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
) -> Option<ExecutionCommandRouteTarget> {
    let decision = compute_next_action_decision(context, status, plan_path)?;
    execution_command_route_target_from_decision(status, &decision, plan_path)
}

#[cfg(test)]
pub(crate) fn public_command_from_status_context(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
) -> Option<PublicCommand> {
    let decision = compute_next_action_decision(context, status, plan_path)?;
    public_command_from_decision(status, &decision, plan_path)
}

pub(crate) fn public_command_from_decision(
    status: &PlanExecutionStatus,
    decision: &NextActionDecision,
    plan_path: &str,
) -> Option<PublicCommand> {
    let command_kind = match decision.kind {
        NextActionKind::Begin | NextActionKind::Resume => PublicCommandKind::Begin,
        NextActionKind::Reopen => PublicCommandKind::Reopen,
        NextActionKind::CloseCurrentTask => PublicCommandKind::Complete,
        NextActionKind::AdvanceLateStage
        | NextActionKind::RequestFinalReview
        | NextActionKind::RunQa
        | NextActionKind::FinishBranch => {
            return public_advance_late_stage_command_for_phase_detail(
                plan_path,
                &decision.phase_detail,
            );
        }
        NextActionKind::Handoff => {
            let scope = handoff_decision_scope(
                status.active_task,
                status.blocking_task,
                status.resume_task,
                status.handoff_required,
                Some(status.harness_phase),
            )
            .unwrap_or("branch");
            return Some(transfer_handoff_public_command(plan_path, scope));
        }
        NextActionKind::RepairReviewState => {
            return Some(repair_review_state_public_command(plan_path));
        }
        NextActionKind::PlanningReentry => return None,
        NextActionKind::WaitForTaskReviewResult
        | NextActionKind::WaitForFinalReviewResult
        | NextActionKind::RefreshTestPlan => return None,
    };
    synthesized_execution_public_command(
        status,
        plan_path,
        command_kind,
        decision.task_number?,
        decision.step_number,
    )
}

fn synthesized_execution_public_command(
    status: &PlanExecutionStatus,
    plan_path: &str,
    command_kind: PublicCommandKind,
    task_number: u32,
    step_id: Option<u32>,
) -> Option<PublicCommand> {
    let execution_source = recommended_execution_source(status.execution_mode.as_str());
    match command_kind {
        PublicCommandKind::Begin => {
            let step_id = step_id?;
            let execution_mode =
                (status.execution_mode == "none").then_some("featureforge:executing-plans");
            Some(begin_public_command(
                plan_path,
                task_number,
                step_id,
                execution_mode,
                &status.execution_fingerprint,
            ))
        }
        PublicCommandKind::Reopen => {
            let step_id = step_id?;
            Some(reopen_public_command(
                plan_path,
                task_number,
                step_id,
                execution_source,
                &status.execution_fingerprint,
            ))
        }
        PublicCommandKind::Complete => {
            let step_id = status.active_step?;
            Some(complete_public_command(
                plan_path,
                task_number,
                step_id,
                execution_source,
                &status.execution_fingerprint,
            ))
        }
        _ => None,
    }
}
