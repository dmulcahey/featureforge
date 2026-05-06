use crate::execution::command_eligibility::public_advance_late_stage_mode_for_phase_detail;
use crate::execution::current_truth::{
    handoff_decision_scope as shared_handoff_decision_scope, reason_code_requires_test_plan_refresh,
};
use crate::execution::harness::HarnessPhase;
use crate::execution::next_action::{
    NextActionDecision, NextActionKind, advance_late_stage_public_command,
    canonical_review_state_status, transfer_handoff_public_command,
};
use crate::execution::query::canonical_phase_for_shared_decision;
use crate::execution::read_model::document_release_pending_phase_detail;
use crate::execution::read_model_support::qa_pending_requires_test_plan_refresh;
use crate::execution::state::{ExecutionContext, GateResult, PlanExecutionStatus};

pub(crate) struct LateStageRouteInputs<'a> {
    pub(crate) context: &'a ExecutionContext,
    pub(crate) status: &'a PlanExecutionStatus,
    pub(crate) plan_path: &'a str,
    pub(crate) external_review_result_ready: bool,
    pub(crate) final_review_dispatch_id: Option<&'a str>,
    pub(crate) final_review_dispatch_lineage_present: bool,
    pub(crate) gate_finish: Option<&'a GateResult>,
}

pub(crate) fn select_late_stage_public_route(
    inputs: LateStageRouteInputs<'_>,
) -> Option<NextActionDecision> {
    let LateStageRouteInputs {
        context,
        status,
        plan_path,
        external_review_result_ready,
        final_review_dispatch_id,
        final_review_dispatch_lineage_present,
        gate_finish,
    } = inputs;
    match status.harness_phase {
        HarnessPhase::DocumentReleasePending => {
            let phase_detail = document_release_pending_phase_detail(status);
            let kind =
                if phase_detail == crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED {
                    NextActionKind::RequestFinalReview
                } else {
                    NextActionKind::AdvanceLateStage
                };
            Some(late_stage_decision(status, kind, phase_detail, plan_path))
        }
        HarnessPhase::FinalReviewPending => {
            if status.current_branch_closure_id.is_none() {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
                    plan_path,
                ));
            }
            if status.current_release_readiness_state.as_deref() != Some("ready") {
                let phase_detail =
                    if status.current_release_readiness_state.as_deref() == Some("blocked") {
                        crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
                    } else {
                        crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY
                    };
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    phase_detail,
                    plan_path,
                ));
            }
            let dispatch_lineage_present =
                final_review_dispatch_lineage_present || final_review_dispatch_id.is_some();
            let phase_requires_dispatch = status.phase_detail
                == crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
                && (!dispatch_lineage_present || status.current_final_review_result.is_some());
            let refresh_requires_dispatch = final_review_dispatch_requires_refresh(status);
            if phase_requires_dispatch
                || refresh_requires_dispatch
                || (!dispatch_lineage_present && status.current_final_review_result.is_none())
            {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::RequestFinalReview,
                    crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
                    plan_path,
                ));
            }
            if status.phase_detail == crate::execution::phase::DETAIL_FINAL_REVIEW_RECORDING_READY
                || status.current_final_review_result.is_some()
                    && status.current_final_review_branch_closure_id.as_deref()
                        == status.current_branch_closure_id.as_deref()
            {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    crate::execution::phase::DETAIL_FINAL_REVIEW_RECORDING_READY,
                    plan_path,
                ));
            }
            if external_review_result_ready {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    crate::execution::phase::DETAIL_FINAL_REVIEW_RECORDING_READY,
                    plan_path,
                ));
            }
            Some(late_stage_decision(
                status,
                NextActionKind::WaitForFinalReviewResult,
                crate::execution::phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING,
                plan_path,
            ))
        }
        HarnessPhase::QaPending => {
            if qa_pending_requires_test_plan_refresh(context, gate_finish)
                || status
                    .reason_codes
                    .iter()
                    .any(|reason_code| reason_code_requires_test_plan_refresh(reason_code))
            {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::RefreshTestPlan,
                    crate::execution::phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED,
                    plan_path,
                ));
            }
            Some(late_stage_decision(
                status,
                NextActionKind::RunQa,
                crate::execution::phase::DETAIL_QA_RECORDING_REQUIRED,
                plan_path,
            ))
        }
        HarnessPhase::ReadyForBranchCompletion => {
            let phase_detail = if status
                .finish_review_gate_pass_branch_closure_id
                .as_deref()
                .zip(status.current_branch_closure_id.as_deref())
                .is_some_and(|(checkpoint, current)| checkpoint == current)
            {
                crate::execution::phase::DETAIL_FINISH_COMPLETION_GATE_READY
            } else {
                crate::execution::phase::DETAIL_FINISH_REVIEW_GATE_READY
            };
            Some(late_stage_decision(
                status,
                NextActionKind::FinishBranch,
                phase_detail,
                plan_path,
            ))
        }
        HarnessPhase::HandoffRequired => Some(late_stage_decision(
            status,
            NextActionKind::Handoff,
            crate::execution::phase::DETAIL_HANDOFF_RECORDING_REQUIRED,
            plan_path,
        )),
        _ => None,
    }
}

pub(crate) fn late_stage_decision(
    status: &PlanExecutionStatus,
    kind: NextActionKind,
    phase_detail: &str,
    plan_path: &str,
) -> NextActionDecision {
    let recommended_public_command = match phase_detail {
        crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS
        | crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY
        | crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
        | crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
        | crate::execution::phase::DETAIL_FINAL_REVIEW_RECORDING_READY
        | crate::execution::phase::DETAIL_QA_RECORDING_REQUIRED
        | crate::execution::phase::DETAIL_FINISH_REVIEW_GATE_READY
        | crate::execution::phase::DETAIL_FINISH_COMPLETION_GATE_READY => {
            public_advance_late_stage_mode_for_phase_detail(phase_detail)
                .map(|mode| advance_late_stage_public_command(plan_path, mode))
        }
        crate::execution::phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING
        | crate::execution::phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED => None,
        crate::execution::phase::DETAIL_HANDOFF_RECORDING_REQUIRED => {
            let scope = shared_handoff_decision_scope(
                status.active_task,
                status.blocking_task,
                status.resume_task,
                status.handoff_required,
                Some(status.harness_phase),
            )
            .unwrap_or("branch");
            Some(transfer_handoff_public_command(plan_path, scope))
        }
        _ => status.recommended_public_command.clone(),
    };
    NextActionDecision {
        kind,
        phase: canonical_phase_for_shared_decision(status.harness_phase.as_str(), phase_detail),
        phase_detail: String::from(phase_detail),
        review_state_status: canonical_review_state_status(status),
        task_number: status
            .blocking_task
            .or(status.resume_task)
            .or(status.active_task),
        step_number: status
            .blocking_step
            .or(status.resume_step)
            .or(status.active_step),
        blocking_task: status.blocking_task,
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_public_command,
    }
}

fn final_review_dispatch_requires_refresh(status: &PlanExecutionStatus) -> bool {
    status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "final_review_state_stale" | "final_review_state_not_fresh"
        )
    }) || (status.current_final_review_result.is_some()
        && status.current_final_review_branch_closure_id.as_deref()
            != status.current_branch_closure_id.as_deref())
}
