#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Runtime enum.
pub enum GateState {
    /// Runtime enum variant.
    Ready,
    /// Runtime enum variant.
    Blocked,
}

impl GateState {
    #[must_use]
    /// Runtime constant.
    pub const fn from_blocked(blocked: bool) -> Self {
        if blocked { Self::Blocked } else { Self::Ready }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Runtime struct.
pub struct LateStageSignals {
    /// Runtime field.
    pub release: GateState,
    /// Runtime field.
    pub review: GateState,
    /// Runtime field.
    pub qa: GateState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Runtime struct.
pub struct LateStageDecision {
    /// Runtime field.
    pub phase: &'static str,
    /// Runtime field.
    pub reason_family: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LateStageRow {
    release: GateState,
    review: GateState,
    qa: GateState,
    phase: &'static str,
    reason_family: &'static str,
}

const PRECEDENCE_ROWS: [LateStageRow; 8] = [
    LateStageRow {
        release: GateState::Blocked,
        review: GateState::Blocked,
        qa: GateState::Blocked,
        phase: "document_release_pending",
        reason_family: "release_readiness",
    },
    LateStageRow {
        release: GateState::Blocked,
        review: GateState::Blocked,
        qa: GateState::Ready,
        phase: "document_release_pending",
        reason_family: "release_readiness",
    },
    LateStageRow {
        release: GateState::Blocked,
        review: GateState::Ready,
        qa: GateState::Blocked,
        phase: "document_release_pending",
        reason_family: "release_readiness",
    },
    LateStageRow {
        release: GateState::Blocked,
        review: GateState::Ready,
        qa: GateState::Ready,
        phase: "document_release_pending",
        reason_family: "release_readiness",
    },
    LateStageRow {
        release: GateState::Ready,
        review: GateState::Blocked,
        qa: GateState::Blocked,
        phase: "final_review_pending",
        reason_family: "final_review_freshness",
    },
    LateStageRow {
        release: GateState::Ready,
        review: GateState::Blocked,
        qa: GateState::Ready,
        phase: "final_review_pending",
        reason_family: "final_review_freshness",
    },
    LateStageRow {
        release: GateState::Ready,
        review: GateState::Ready,
        qa: GateState::Blocked,
        phase: "qa_pending",
        reason_family: "qa_freshness",
    },
    LateStageRow {
        release: GateState::Ready,
        review: GateState::Ready,
        qa: GateState::Ready,
        phase: "ready_for_branch_completion",
        reason_family: "all_fresh",
    },
];

#[must_use]
/// Runtime function.
pub fn resolve(signals: LateStageSignals) -> LateStageDecision {
    PRECEDENCE_ROWS
        .iter()
        .find(|row| {
            row.release == signals.release && row.review == signals.review && row.qa == signals.qa
        })
        .map_or(
            LateStageDecision {
                phase: "final_review_pending",
                reason_family: "fallback_fail_closed",
            },
            |row| LateStageDecision {
                phase: row.phase,
                reason_family: row.reason_family,
            },
        )
}
