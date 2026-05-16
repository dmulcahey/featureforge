/// Authoritative event-log envelope command owners.
///
/// Public aggregate variants are the only owners normal workflow mutations
/// should persist. Internal variants preserve compatibility for old event logs
/// and explicit internal compatibility paths without making those primitive
/// names part of the public workflow contract again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventCommandOwner {
    PublicAdvanceLateStage,
    PublicCloseCurrentTask,
    InternalRecordBranchClosure,
    InternalRecordReleaseReadiness,
    InternalRecordFinalReview,
    InternalRecordQa,
    InternalRecordReviewDispatch,
}

impl EventCommandOwner {
    #[must_use]
    pub(crate) const fn is_public(self) -> bool {
        matches!(
            self,
            Self::PublicAdvanceLateStage | Self::PublicCloseCurrentTask
        )
    }

    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::PublicAdvanceLateStage => {
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE
            }
            Self::PublicCloseCurrentTask => {
                crate::execution::review_route_tokens::FOLLOW_UP_CLOSE_CURRENT_TASK
            }
            Self::InternalRecordBranchClosure => "record_branch_closure",
            Self::InternalRecordReleaseReadiness => "record_release_readiness",
            Self::InternalRecordFinalReview => "record_final_review",
            Self::InternalRecordQa => "record_qa",
            Self::InternalRecordReviewDispatch => "record_review_dispatch",
        }
    }
}
