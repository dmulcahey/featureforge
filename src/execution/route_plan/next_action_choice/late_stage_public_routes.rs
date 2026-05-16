use crate::execution::current_truth::{
    final_review_result_branch_mismatch, final_review_result_recorded_for_current_branch,
    finish_review_gate_passed_for_current_branch, reason_code_requires_test_plan_refresh,
};
use crate::execution::harness::{DownstreamFreshnessState, HarnessPhase};
use crate::execution::phase;
use crate::execution::review_route_tokens::is_final_review_refresh_reason;
use crate::execution::state::{
    CurrentTaskClosureBranchRouteFacts, ExecutionContext, GateResult, PlanExecutionStatus,
};
use crate::execution::status_support::qa_pending_requires_test_plan_refresh;

use super::super::route_semantics::canonical_phase_for_shared_decision;
use super::{NextActionDecision, NextActionKind, canonical_review_state_status};

pub(super) struct LateStageRouteInputs<'a> {
    pub(super) context: &'a ExecutionContext,
    pub(super) status: &'a PlanExecutionStatus,
    pub(super) plan_path: &'a str,
    pub(super) external_review_result_ready: bool,
    pub(super) final_review_dispatch_id: Option<&'a str>,
    pub(super) final_review_dispatch_lineage_present: bool,
    pub(super) final_review_outcome_recorded_for_current_dispatch: bool,
    pub(super) gate_finish: Option<&'a GateResult>,
    pub(super) current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts,
}

pub(super) fn select_late_stage_public_route(
    inputs: LateStageRouteInputs<'_>,
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
    match status.harness_phase {
        HarnessPhase::DocumentReleasePending => {
            let phase_detail = document_release_pending_route_phase_detail(status);
            let kind = if phase_detail == phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED {
                NextActionKind::RequestFinalReview
            } else {
                NextActionKind::AdvanceLateStage
            };
            Some(late_stage_decision(status, kind, phase_detail, plan_path))
        }
        HarnessPhase::FinalReviewPending => {
            if current_task_closure_branch_route_facts.missing_branch_closure() {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
                    plan_path,
                ));
            }
            if status.current_release_readiness_state.as_deref() != Some("ready") {
                let phase_detail =
                    if status.current_release_readiness_state.as_deref() == Some("blocked") {
                        phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
                    } else {
                        phase::DETAIL_RELEASE_READINESS_RECORDING_READY
                    };
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    phase_detail,
                    plan_path,
                ));
            }
            if final_review_outcome_recorded_for_current_dispatch
                && status.current_final_review_result.is_none()
                && status.current_branch_meaningful_drift
            {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
                    plan_path,
                ));
            }
            let dispatch_lineage_present =
                final_review_dispatch_lineage_present || final_review_dispatch_id.is_some();
            let phase_requires_dispatch = status.phase_detail
                == phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED
                && (!dispatch_lineage_present || status.current_final_review_result.is_some());
            let refresh_requires_dispatch = final_review_dispatch_requires_refresh(status);
            if phase_requires_dispatch
                || refresh_requires_dispatch
                || (!dispatch_lineage_present && status.current_final_review_result.is_none())
            {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::RequestFinalReview,
                    phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
                    plan_path,
                ));
            }
            if status.phase_detail == phase::DETAIL_FINAL_REVIEW_RECORDING_READY
                || final_review_result_recorded_for_current_branch(status)
            {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    phase::DETAIL_FINAL_REVIEW_RECORDING_READY,
                    plan_path,
                ));
            }
            if external_review_result_ready {
                return Some(late_stage_decision(
                    status,
                    NextActionKind::AdvanceLateStage,
                    phase::DETAIL_FINAL_REVIEW_RECORDING_READY,
                    plan_path,
                ));
            }
            Some(late_stage_decision(
                status,
                NextActionKind::WaitForFinalReviewResult,
                phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING,
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
                    phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED,
                    plan_path,
                ));
            }
            Some(late_stage_decision(
                status,
                NextActionKind::RunQa,
                phase::DETAIL_QA_RECORDING_REQUIRED,
                plan_path,
            ))
        }
        HarnessPhase::ReadyForBranchCompletion => {
            let phase_detail = if finish_review_gate_passed_for_current_branch(status) {
                phase::DETAIL_FINISH_COMPLETION_GATE_READY
            } else {
                phase::DETAIL_FINISH_REVIEW_GATE_READY
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
            phase::DETAIL_HANDOFF_RECORDING_REQUIRED,
            plan_path,
        )),
        _ => None,
    }
}

fn document_release_pending_route_phase_detail(status: &PlanExecutionStatus) -> &'static str {
    match (
        status.current_release_readiness_state.as_deref(),
        status.release_docs_state,
    ) {
        (Some("blocked"), _) => phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED,
        (_, DownstreamFreshnessState::Fresh) => phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
        _ => phase::DETAIL_RELEASE_READINESS_RECORDING_READY,
    }
}

pub(super) fn late_stage_decision(
    status: &PlanExecutionStatus,
    kind: NextActionKind,
    phase_detail: &str,
    _plan_path: &str,
) -> NextActionDecision {
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
    }
}

fn final_review_dispatch_requires_refresh(status: &PlanExecutionStatus) -> bool {
    status
        .reason_codes
        .iter()
        .any(|reason_code| is_final_review_refresh_reason(reason_code))
        || final_review_result_branch_mismatch(status)
}
