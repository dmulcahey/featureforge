#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateState {
    Ready,
    Blocked,
}

impl GateState {
    pub const fn from_blocked(blocked: bool) -> Self {
        if blocked { Self::Blocked } else { Self::Ready }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LateStageSignals {
    pub release: GateState,
    pub review: GateState,
    pub qa: GateState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LateStageDecision {
    pub phase: &'static str,
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
        phase: crate::execution::phase::PHASE_DOCUMENT_RELEASE_PENDING,
        reason_family: "release_readiness",
    },
    LateStageRow {
        release: GateState::Blocked,
        review: GateState::Blocked,
        qa: GateState::Ready,
        phase: crate::execution::phase::PHASE_DOCUMENT_RELEASE_PENDING,
        reason_family: "release_readiness",
    },
    LateStageRow {
        release: GateState::Blocked,
        review: GateState::Ready,
        qa: GateState::Blocked,
        phase: crate::execution::phase::PHASE_DOCUMENT_RELEASE_PENDING,
        reason_family: "release_readiness",
    },
    LateStageRow {
        release: GateState::Blocked,
        review: GateState::Ready,
        qa: GateState::Ready,
        phase: crate::execution::phase::PHASE_DOCUMENT_RELEASE_PENDING,
        reason_family: "release_readiness",
    },
    LateStageRow {
        release: GateState::Ready,
        review: GateState::Blocked,
        qa: GateState::Blocked,
        phase: crate::execution::phase::PHASE_FINAL_REVIEW_PENDING,
        reason_family: "final_review_freshness",
    },
    LateStageRow {
        release: GateState::Ready,
        review: GateState::Blocked,
        qa: GateState::Ready,
        phase: crate::execution::phase::PHASE_FINAL_REVIEW_PENDING,
        reason_family: "final_review_freshness",
    },
    LateStageRow {
        release: GateState::Ready,
        review: GateState::Ready,
        qa: GateState::Blocked,
        phase: crate::execution::phase::PHASE_QA_PENDING,
        reason_family: "qa_freshness",
    },
    LateStageRow {
        release: GateState::Ready,
        review: GateState::Ready,
        qa: GateState::Ready,
        phase: crate::execution::phase::PHASE_READY_FOR_BRANCH_COMPLETION,
        reason_family: "all_fresh",
    },
];

pub fn resolve(signals: LateStageSignals) -> LateStageDecision {
    PRECEDENCE_ROWS
        .iter()
        .find(|row| {
            row.release == signals.release && row.review == signals.review && row.qa == signals.qa
        })
        .map(|row| LateStageDecision {
            phase: row.phase,
            reason_family: row.reason_family,
        })
        .unwrap_or(LateStageDecision {
            phase: crate::execution::phase::PHASE_FINAL_REVIEW_PENDING,
            reason_family: "fallback_fail_closed",
        })
}
