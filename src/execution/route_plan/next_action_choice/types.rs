use crate::execution::current_truth::task_review_result_requires_verification_reason_codes;
use crate::execution::state::PlanExecutionStatus;
use crate::execution::status_assembly::effective_review_state_status;

use super::super::state_kind::state_kind_or_phase_is_runtime_diagnostic;

pub(crate) const NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED: &str = "runtime diagnostic required";
pub(crate) const NEXT_ACTION_REPAIR_REVIEW_STATE: &str = "repair review state";
pub(crate) const NEXT_ACTION_CONTINUE_EXECUTION: &str = "continue execution";
pub(crate) const NEXT_ACTION_EXECUTION_REENTRY_REQUIRED: &str = "execution reentry required";
pub(crate) const NEXT_ACTION_CLOSE_CURRENT_TASK: &str = "close current task";
pub(crate) const NEXT_ACTION_RUN_VERIFICATION: &str = "run verification";
pub(crate) const NEXT_ACTION_WAIT_FOR_EXTERNAL_REVIEW_RESULT: &str =
    "wait for external review result";
pub(crate) const NEXT_ACTION_RESOLVE_RELEASE_BLOCKER: &str = "resolve release blocker";
pub(crate) const NEXT_ACTION_ADVANCE_LATE_STAGE: &str = "advance late stage";
pub(crate) const NEXT_ACTION_REQUEST_FINAL_REVIEW: &str = "request final review";
pub(crate) const NEXT_ACTION_REFRESH_TEST_PLAN: &str = "refresh test plan";
pub(crate) const NEXT_ACTION_RUN_QA: &str = "run QA";
pub(crate) const NEXT_ACTION_FINISH_BRANCH: &str = "finish branch";
pub(crate) const NEXT_ACTION_PLANNING_REENTRY: &str = "pivot / return to planning";
pub(crate) const NEXT_ACTION_HANDOFF: &str = "hand off";

pub const PUBLIC_NEXT_ACTION_VALUES: &[&str] = &[
    NEXT_ACTION_ADVANCE_LATE_STAGE,
    NEXT_ACTION_FINISH_BRANCH,
    NEXT_ACTION_CLOSE_CURRENT_TASK,
    NEXT_ACTION_CONTINUE_EXECUTION,
    NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED,
    NEXT_ACTION_REQUEST_FINAL_REVIEW,
    NEXT_ACTION_EXECUTION_REENTRY_REQUIRED,
    NEXT_ACTION_HANDOFF,
    NEXT_ACTION_PLANNING_REENTRY,
    NEXT_ACTION_REFRESH_TEST_PLAN,
    NEXT_ACTION_REPAIR_REVIEW_STATE,
    NEXT_ACTION_RESOLVE_RELEASE_BLOCKER,
    NEXT_ACTION_RUN_QA,
    NEXT_ACTION_RUN_VERIFICATION,
    NEXT_ACTION_WAIT_FOR_EXTERNAL_REVIEW_RESULT,
];

pub(crate) fn diagnostic_next_action_for_route(
    state_kind: &str,
    phase_detail: &str,
    has_public_invocation: bool,
    has_required_inputs: bool,
) -> Option<String> {
    if runtime_route_is_diagnostic(state_kind, phase_detail)
        && !has_public_invocation
        && !has_required_inputs
    {
        Some(String::from(NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED))
    } else {
        None
    }
}

pub(crate) fn runtime_route_is_diagnostic(state_kind: &str, phase_detail: &str) -> bool {
    state_kind_or_phase_is_runtime_diagnostic(state_kind, phase_detail)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NextActionKind {
    Begin,
    Resume,
    Reopen,
    CloseCurrentTask,
    WaitForTaskReviewResult,
    AdvanceLateStage,
    RequestFinalReview,
    WaitForFinalReviewResult,
    RefreshTestPlan,
    RunQa,
    FinishBranch,
    RepairReviewState,
    PlanningReentry,
    Handoff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NextActionDecision {
    pub kind: NextActionKind,
    pub phase: String,
    pub phase_detail: String,
    pub review_state_status: String,
    pub task_number: Option<u32>,
    pub step_number: Option<u32>,
    pub blocking_task: Option<u32>,
    pub blocking_reason_codes: Vec<String>,
}

#[derive(Clone, Copy)]
pub(crate) struct NextActionRequestInputs<'a> {
    pub(crate) plan_path: &'a str,
    pub(crate) external_review_result_ready: bool,
    pub(crate) task_review_dispatch_id: Option<&'a str>,
    pub(crate) final_review_dispatch_id: Option<&'a str>,
    pub(crate) final_review_dispatch_lineage_present: bool,
    pub(crate) final_review_outcome_recorded_for_current_dispatch: bool,
}

pub(crate) fn public_next_action_text(decision: &NextActionDecision) -> String {
    match decision.kind {
        NextActionKind::Begin | NextActionKind::Resume | NextActionKind::Reopen => {
            let negative_result_reentry = decision
                .blocking_reason_codes
                .iter()
                .any(|reason_code| reason_code == crate::execution::review_route_tokens::REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY);
            let structural_task_repair_lane =
                decision.blocking_reason_codes.iter().any(|reason_code| {
                    matches!(
                        reason_code.as_str(),
                        crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID
                            | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED
                    ) || (reason_code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE
                        && !negative_result_reentry)
                });
            if decision.phase == crate::execution::phase::PHASE_EXECUTION_PREFLIGHT
                || decision.phase_detail
                    == crate::execution::phase::DETAIL_EXECUTION_PREFLIGHT_REQUIRED
            {
                String::from(NEXT_ACTION_CONTINUE_EXECUTION)
            } else if decision.phase_detail
                == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
                && structural_task_repair_lane
            {
                String::from(NEXT_ACTION_REPAIR_REVIEW_STATE)
            } else if decision.phase_detail
                == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
            {
                String::from(NEXT_ACTION_EXECUTION_REENTRY_REQUIRED)
            } else {
                String::from(NEXT_ACTION_CONTINUE_EXECUTION)
            }
        }
        NextActionKind::CloseCurrentTask => {
            if decision.phase_detail == crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY
            {
                String::from(NEXT_ACTION_CLOSE_CURRENT_TASK)
            } else {
                String::from(NEXT_ACTION_CONTINUE_EXECUTION)
            }
        }
        NextActionKind::WaitForTaskReviewResult => {
            if task_review_result_requires_verification_reason_codes(
                decision.blocking_reason_codes.iter().map(String::as_str),
            ) {
                String::from(NEXT_ACTION_RUN_VERIFICATION)
            } else {
                String::from(NEXT_ACTION_WAIT_FOR_EXTERNAL_REVIEW_RESULT)
            }
        }
        NextActionKind::AdvanceLateStage => {
            if decision.phase_detail
                == crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED
            {
                String::from(NEXT_ACTION_RESOLVE_RELEASE_BLOCKER)
            } else {
                String::from(NEXT_ACTION_ADVANCE_LATE_STAGE)
            }
        }
        NextActionKind::RequestFinalReview => String::from(NEXT_ACTION_REQUEST_FINAL_REVIEW),
        NextActionKind::WaitForFinalReviewResult => {
            String::from(NEXT_ACTION_WAIT_FOR_EXTERNAL_REVIEW_RESULT)
        }
        NextActionKind::RefreshTestPlan => String::from(NEXT_ACTION_REFRESH_TEST_PLAN),
        NextActionKind::RunQa => String::from(NEXT_ACTION_RUN_QA),
        NextActionKind::FinishBranch => String::from(NEXT_ACTION_FINISH_BRANCH),
        NextActionKind::RepairReviewState => String::from(NEXT_ACTION_REPAIR_REVIEW_STATE),
        NextActionKind::PlanningReentry => String::from(NEXT_ACTION_PLANNING_REENTRY),
        NextActionKind::Handoff => String::from(NEXT_ACTION_HANDOFF),
    }
}

pub(crate) fn canonical_review_state_status(status: &PlanExecutionStatus) -> String {
    effective_review_state_status(status, status.review_state_status.as_str())
}
