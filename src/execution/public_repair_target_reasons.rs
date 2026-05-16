#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicRepairTargetReason {
    AuthoritativePreflightRecoveryTaskClosure,
    AuthoritativeTaskClosurePostconditionCleanup,
    CurrentTaskClosureWorktreeLeaseCleanup,
    ExplicitReopenRepairTarget,
    PersistedExecutionReentryFollowUp,
    PersistedReviewStateRepairFollowUp,
    PersistedTaskClosureFollowUp,
    RouteAdvanceLateStageReady,
    RouteExecutionReentryRequired,
    RouteRepairReviewStateAvailable,
    RouteTaskClosureRecordingReady,
    RouteTaskClosureRepairStateRefresh,
    StatusTaskClosureRecordingReady,
    TaskReviewDispatchClosureReady,
}

impl PublicRepairTargetReason {
    pub const ALL: &'static [Self] = &[
        Self::AuthoritativePreflightRecoveryTaskClosure,
        Self::AuthoritativeTaskClosurePostconditionCleanup,
        Self::CurrentTaskClosureWorktreeLeaseCleanup,
        Self::ExplicitReopenRepairTarget,
        Self::PersistedExecutionReentryFollowUp,
        Self::PersistedReviewStateRepairFollowUp,
        Self::PersistedTaskClosureFollowUp,
        Self::RouteAdvanceLateStageReady,
        Self::RouteExecutionReentryRequired,
        Self::RouteRepairReviewStateAvailable,
        Self::RouteTaskClosureRecordingReady,
        Self::RouteTaskClosureRepairStateRefresh,
        Self::StatusTaskClosureRecordingReady,
        Self::TaskReviewDispatchClosureReady,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AuthoritativePreflightRecoveryTaskClosure => {
                "authoritative_preflight_recovery_task_closure"
            }
            Self::AuthoritativeTaskClosurePostconditionCleanup => {
                "authoritative_task_closure_postcondition_cleanup"
            }
            Self::CurrentTaskClosureWorktreeLeaseCleanup => {
                "current_task_closure_worktree_lease_cleanup"
            }
            Self::ExplicitReopenRepairTarget => "explicit_reopen_repair_target",
            Self::PersistedExecutionReentryFollowUp => "persisted_execution_reentry_follow_up",
            Self::PersistedReviewStateRepairFollowUp => {
                PERSISTED_REVIEW_STATE_REPAIR_FOLLOW_UP_REASON_PREFIX
            }
            Self::PersistedTaskClosureFollowUp => "persisted_task_closure_follow_up",
            Self::RouteAdvanceLateStageReady => "route_advance_late_stage_ready",
            Self::RouteExecutionReentryRequired => "route_execution_reentry_required",
            Self::RouteRepairReviewStateAvailable => "route_repair_review_state_available",
            Self::RouteTaskClosureRecordingReady => "route_task_closure_recording_ready",
            Self::RouteTaskClosureRepairStateRefresh => "route_task_closure_repair_state_refresh",
            Self::StatusTaskClosureRecordingReady => "status_task_closure_recording_ready",
            Self::TaskReviewDispatchClosureReady => "task_review_dispatch_closure_ready",
        }
    }

    pub fn reason_code(self) -> String {
        String::from(self.as_str())
    }

    pub fn matches(self, reason_code: &str) -> bool {
        match self {
            Self::PersistedReviewStateRepairFollowUp => {
                is_persisted_review_state_repair_follow_up_reason(reason_code)
            }
            _ => reason_code == self.as_str(),
        }
    }

    pub fn is_reopen_route_candidate(reason_code: &str) -> bool {
        Self::ExplicitReopenRepairTarget.matches(reason_code)
            || Self::PersistedExecutionReentryFollowUp.matches(reason_code)
    }

    pub fn is_close_current_task_explicit(reason_code: &str) -> bool {
        matches!(
            reason_code,
            reason if Self::PersistedTaskClosureFollowUp.matches(reason)
                || Self::AuthoritativeTaskClosurePostconditionCleanup.matches(reason)
                || Self::CurrentTaskClosureWorktreeLeaseCleanup.matches(reason)
                || Self::TaskReviewDispatchClosureReady.matches(reason)
                || Self::AuthoritativePreflightRecoveryTaskClosure.matches(reason)
                || Self::StatusTaskClosureRecordingReady.matches(reason)
        )
    }
}

pub const PERSISTED_REVIEW_STATE_REPAIR_FOLLOW_UP_REASON_PREFIX: &str =
    "persisted_review_state_repair_follow_up";

pub fn persisted_review_state_repair_follow_up_reason(follow_up: &str) -> String {
    format!("{PERSISTED_REVIEW_STATE_REPAIR_FOLLOW_UP_REASON_PREFIX}:{follow_up}")
}

pub fn is_persisted_review_state_repair_follow_up_reason(reason_code: &str) -> bool {
    reason_code
        .strip_prefix(PERSISTED_REVIEW_STATE_REPAIR_FOLLOW_UP_REASON_PREFIX)
        .and_then(|suffix| suffix.strip_prefix(':'))
        .is_some_and(|follow_up| !follow_up.trim().is_empty())
}
