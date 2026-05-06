#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicAdvanceLateStageMode {
    Basic,
    ReleaseReadiness,
    FinalReviewDispatch,
    Qa,
    FinalReview,
    FinishReview,
    FinishCompletion,
}

pub(crate) fn public_advance_late_stage_mode_for_phase_detail(
    phase_detail: &str,
) -> Option<PublicAdvanceLateStageMode> {
    match phase_detail {
        crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS => {
            Some(PublicAdvanceLateStageMode::Basic)
        }
        crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY
        | crate::execution::phase::DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED => {
            Some(PublicAdvanceLateStageMode::ReleaseReadiness)
        }
        crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED => {
            Some(PublicAdvanceLateStageMode::FinalReviewDispatch)
        }
        crate::execution::phase::DETAIL_FINAL_REVIEW_RECORDING_READY => {
            Some(PublicAdvanceLateStageMode::FinalReview)
        }
        crate::execution::phase::DETAIL_QA_RECORDING_REQUIRED => {
            Some(PublicAdvanceLateStageMode::Qa)
        }
        crate::execution::phase::DETAIL_FINISH_REVIEW_GATE_READY => {
            Some(PublicAdvanceLateStageMode::FinishReview)
        }
        crate::execution::phase::DETAIL_FINISH_COMPLETION_GATE_READY => {
            Some(PublicAdvanceLateStageMode::FinishCompletion)
        }
        _ => None,
    }
}
