use crate::execution::current_truth::{
    BranchRerecordingAssessment, BranchRerecordingUnsupportedReason,
    late_stage_surface_not_declared_reason_code as shared_late_stage_surface_not_declared_reason_code,
    negative_result_requires_execution_reentry_for_status,
    stale_provenance_after_authoritative_closure_is_diagnostic,
    worktree_drift_escapes_late_stage_surface,
};
use crate::execution::harness::HarnessPhase;
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::phase;
use crate::execution::repair_route_decision::{
    task_closure_baseline_bridge_late_stage_missing_current_closure_route_task,
    task_closure_baseline_bridge_missing_baseline_unsupported_route_task,
};
use crate::execution::repair_target_selection::{
    NextActionAuthorityInputs, execution_reentry_target,
};
use crate::execution::state::{
    CurrentTaskClosureBranchRouteFacts, ExecutionContext, PlanExecutionStatus,
    task_scope_structural_review_state_reason,
};

use super::execution_routes::{
    ExecutionReentryDecisionInputs, execution_reentry_decision_for_task, execution_repair_decision,
    execution_repair_decision_for_task, missing_execution_reentry_target_decision,
    task_closure_recording_ready_decision,
};
use super::late_stage_public_routes::late_stage_decision;
use super::{NextActionDecision, NextActionKind, canonical_review_state_status};

pub(super) fn stale_late_stage_repair_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> NextActionDecision {
    let current_task_closure_branch_route_facts =
        authority_inputs.precomputed_current_task_closure_branch_route_facts();
    if current_task_closure_branch_route_facts.missing_branch_closure()
        && let Some(assessment) = authority_inputs.branch_rerecording_assessment
        && assessment.supported
    {
        return late_stage_missing_current_closure_decision_from_assessment(
            context,
            status,
            plan_path,
            crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            assessment,
            authority_inputs,
        );
    }
    let target = execution_reentry_target(context, status, plan_path, authority_inputs);
    if let Some(target) = target.as_ref()
        && status
            .resume_task
            .or(status.active_task)
            .is_none_or(|open_task| target.task < open_task)
        && status.blocking_step.is_none()
    {
        let task_number = target.task;
        let mut reentry_decision = execution_reentry_decision_for_task(
            context,
            status,
            plan_path,
            crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            task_number,
            ExecutionReentryDecisionInputs {
                current_task_closure_branch_route_facts,
                authority_inputs,
                stale_boundary_route: false,
            },
        );
        if !reentry_decision.blocking_reason_codes.iter().any(|reason| {
            reason == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        }) {
            reentry_decision.blocking_reason_codes.push(String::from(
                crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            ));
        }
        return reentry_decision;
    }
    let mut blocking_reason_codes = status.reason_codes.clone();
    if !blocking_reason_codes.iter().any(|reason| {
        reason == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
    }) {
        blocking_reason_codes.push(String::from(
            crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
        ));
    }
    if let Some(target) = target {
        let mut repair_decision = execution_repair_decision_for_task(
            status,
            plan_path,
            crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            target.task,
        );
        if !repair_decision.blocking_reason_codes.iter().any(|reason| {
            reason == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
        }) {
            repair_decision.blocking_reason_codes.push(String::from(
                crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            ));
        }
        return repair_decision;
    }
    if authority_inputs.has_authoritative_stale_target {
        NextActionDecision {
            kind: NextActionKind::RepairReviewState,
            phase: String::from(phase::PHASE_EXECUTING),
            phase_detail: String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
            review_state_status: String::from(
                crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            ),
            task_number: None,
            step_number: None,
            blocking_task: None,
            blocking_reason_codes,
        }
    } else {
        crate::execution::reentry_reconcile::TargetlessStaleReconcile::ensure_reason_codes(
            &mut blocking_reason_codes,
        );
        NextActionDecision {
            kind: NextActionKind::RepairReviewState,
            phase: String::from(phase::PHASE_EXECUTING),
            phase_detail: String::from(
                crate::execution::reentry_reconcile::TARGETLESS_STALE_RECONCILE_PHASE_DETAIL,
            ),
            review_state_status: String::from(
                crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            ),
            task_number: None,
            step_number: None,
            blocking_task: None,
            blocking_reason_codes,
        }
    }
}

pub(super) fn late_stage_planning_reentry_decision(
    status: &PlanExecutionStatus,
    review_state_status: &str,
) -> NextActionDecision {
    NextActionDecision {
        kind: NextActionKind::PlanningReentry,
        phase: String::from(phase::PHASE_PIVOT_REQUIRED),
        phase_detail: String::from(phase::DETAIL_PLANNING_REENTRY_REQUIRED),
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

pub(super) fn late_stage_missing_current_closure_decision_from_assessment(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    assessment: &BranchRerecordingAssessment,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> NextActionDecision {
    let current_task_closure_branch_route_facts =
        authority_inputs.precomputed_current_task_closure_branch_route_facts();
    if task_scope_structural_review_state_reason(status).is_some() {
        let structural_review_state_status = canonical_review_state_status(status);
        return execution_repair_decision(
            context,
            status,
            plan_path,
            structural_review_state_status.as_str(),
            authority_inputs,
        );
    }
    if let Some(task_number) =
        task_closure_baseline_bridge_late_stage_missing_current_closure_route_task(
            context,
            status,
            authority_inputs,
            review_state_status,
        )
    {
        return task_closure_recording_ready_decision(
            status,
            plan_path,
            current_task_closure_branch_route_facts,
            task_number,
        );
    }
    if current_task_closure_branch_route_facts.set_is_missing_for_late_stage_reentry() {
        return execution_reentry_target(context, status, plan_path, authority_inputs)
            .map(|target| {
                execution_reentry_decision_for_task(
                    context,
                    status,
                    plan_path,
                    review_state_status,
                    target.task,
                    ExecutionReentryDecisionInputs {
                        current_task_closure_branch_route_facts,
                        authority_inputs,
                        stale_boundary_route: false,
                    },
                )
            })
            .unwrap_or_else(|| {
                execution_repair_decision(
                    context,
                    status,
                    plan_path,
                    review_state_status,
                    authority_inputs,
                )
            });
    }
    if current_task_closure_branch_route_facts
        .set_has_non_branch_contributing_closure_without_branch()
    {
        return execution_repair_decision(
            context,
            status,
            plan_path,
            review_state_status,
            authority_inputs,
        );
    }
    if assessment.supported {
        return late_stage_decision(
            status,
            NextActionKind::AdvanceLateStage,
            phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            plan_path,
        );
    }
    match assessment.unsupported_reason {
        Some(BranchRerecordingUnsupportedReason::LateStageSurfaceNotDeclared) => {
            late_stage_planning_reentry_decision(status, review_state_status)
        }
        Some(BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline) => {
            if authority_inputs.persisted_repair_follow_up
                == Some(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE)
            {
                return late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
                    plan_path,
                );
            }
            if current_task_closure_branch_route_facts.missing_branch_closure() {
                if current_task_closure_branch_route_facts.set_is_non_branch_contributing() {
                    return execution_repair_decision(
                        context,
                        status,
                        plan_path,
                        review_state_status,
                        authority_inputs,
                    );
                }
                if let Some(task_number) =
                    task_closure_baseline_bridge_missing_baseline_unsupported_route_task(
                        context,
                        status,
                        authority_inputs,
                        assessment,
                    )
                {
                    return task_closure_recording_ready_decision(
                        status,
                        plan_path,
                        current_task_closure_branch_route_facts,
                        task_number,
                    );
                }
                return execution_reentry_target(context, status, plan_path, authority_inputs)
                    .map(|target| {
                        execution_reentry_decision_for_task(
                            context,
                            status,
                            plan_path,
                            review_state_status,
                            target.task,
                            ExecutionReentryDecisionInputs {
                                current_task_closure_branch_route_facts,
                                authority_inputs,
                                stale_boundary_route: false,
                            },
                        )
                    })
                    .unwrap_or_else(|| {
                        execution_repair_decision(
                            context,
                            status,
                            plan_path,
                            review_state_status,
                            authority_inputs,
                        )
                    });
            }
            execution_repair_decision(
                context,
                status,
                plan_path,
                review_state_status,
                authority_inputs,
            )
        }
        Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface) | None => {
            execution_reentry_target(context, status, plan_path, authority_inputs)
                .map(|target| {
                    execution_reentry_decision_for_task(
                        context,
                        status,
                        plan_path,
                        review_state_status,
                        target.task,
                        ExecutionReentryDecisionInputs {
                            current_task_closure_branch_route_facts,
                            authority_inputs,
                            stale_boundary_route: false,
                        },
                    )
                })
                .unwrap_or_else(|| {
                    late_stage_planning_reentry_decision(status, review_state_status)
                })
        }
    }
}

pub(super) fn late_stage_execution_reentry_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    review_state_status: &str,
    authority_inputs: NextActionAuthorityInputs<'_>,
) -> Option<NextActionDecision> {
    let current_task_closure_branch_route_facts =
        authority_inputs.precomputed_current_task_closure_branch_route_facts();
    let stale_provenance_present = status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE);
    let negative_result_reroute = late_stage_negative_result_reroute(
        status,
        review_state_status,
        current_task_closure_branch_route_facts,
    );
    let stale_provenance_reroute = matches!(
        status.harness_phase,
        HarnessPhase::Executing | HarnessPhase::FinalReviewPending
    ) && review_state_status == "clean"
        && current_task_closure_branch_route_facts.branch_closure_recorded()
        && status.blocking_step.is_none()
        && stale_provenance_present;
    let stale_provenance_reroute = stale_provenance_reroute
        && !stale_provenance_after_authoritative_closure_is_diagnostic(status);
    if !(negative_result_reroute || stale_provenance_reroute) {
        return None;
    }
    let reentry_target = execution_reentry_target(
        context,
        status,
        plan_path,
        authority_inputs.with_derived_negative_result_reentry(negative_result_reroute),
    );
    Some(
        reentry_target
            .map(|target| {
                execution_reentry_decision_for_task(
                    context,
                    status,
                    plan_path,
                    review_state_status,
                    target.task,
                    ExecutionReentryDecisionInputs {
                        current_task_closure_branch_route_facts,
                        authority_inputs,
                        stale_boundary_route: negative_result_reroute,
                    },
                )
            })
            .unwrap_or_else(|| {
                missing_execution_reentry_target_decision(status, review_state_status)
            }),
    )
}

pub(super) fn late_stage_negative_result_reroute(
    status: &PlanExecutionStatus,
    review_state_status: &str,
    current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts,
) -> bool {
    review_state_status == "clean"
        && current_task_closure_branch_route_facts.branch_closure_recorded()
        && status.blocking_step.is_none()
        && (status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == crate::execution::review_route_tokens::REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY)
            || negative_result_requires_execution_reentry_for_status(
                false,
                phase::PHASE_EXECUTING,
                status,
            ))
}

pub(super) fn persisted_late_stage_reroute_missing_current_closure(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authority_inputs: NextActionAuthorityInputs<'_>,
    review_state_status: &str,
) -> bool {
    let current_task_closure_branch_route_facts =
        authority_inputs.precomputed_current_task_closure_branch_route_facts();
    review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
        && current_task_closure_branch_route_facts.missing_current_closure_can_reroute_to_late_stage()
        && status.blocking_step.is_none()
        && authority_inputs.persisted_repair_follow_up == Some(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE)
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING)
        && !worktree_drift_escapes_late_stage_surface(context).unwrap_or(false)
        && !status
            .reason_codes
            .iter()
            .any(|reason_code| shared_late_stage_surface_not_declared_reason_code(reason_code))
}
