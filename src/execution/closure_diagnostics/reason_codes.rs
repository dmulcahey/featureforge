use crate::execution::phase;

pub(crate) const TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_MISSING: &str =
    "prior_task_review_dispatch_missing";
pub(crate) const TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_STALE: &str =
    "prior_task_review_dispatch_stale";
pub(crate) const TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING: &str =
    "prior_task_verification_missing";
pub(crate) const TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING_LEGACY: &str =
    "prior_task_verification_missing_legacy";
pub(crate) const TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_NOT_INDEPENDENT: &str =
    "task_review_not_independent";
pub(crate) const TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_ARTIFACT_MALFORMED: &str =
    "task_review_artifact_malformed";
pub(crate) const TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_VERIFICATION_SUMMARY_MALFORMED: &str =
    "task_verification_summary_malformed";

pub(crate) const TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING: &str =
    "prior_task_current_closure_missing";
pub(crate) const TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE: &str =
    "prior_task_current_closure_stale";
pub(crate) const TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID: &str =
    "prior_task_current_closure_invalid";
pub(crate) const TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED: &str =
    "prior_task_current_closure_reviewed_state_malformed";
pub(crate) const TASK_BOUNDARY_REASON_TASK_CYCLE_BREAK_ACTIVE: &str = "task_cycle_break_active";
pub(crate) const TASK_BOUNDARY_REASON_CURRENT_TASK_CLOSURE_OVERLAY_RESTORE_REQUIRED: &str =
    "current_task_closure_overlay_restore_required";
pub(crate) const TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN: &str =
    "prior_task_review_not_green";
pub(crate) const TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE: &str =
    "task_closure_baseline_repair_candidate";
pub(crate) const TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_BRIDGE_READY: &str =
    "task_closure_baseline_bridge_ready";
pub(crate) const TASK_BOUNDARY_REASON_TASK_CLOSURE_RECORDING_READY: &str =
    phase::DETAIL_TASK_CLOSURE_RECORDING_READY;
pub(crate) const BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED: &str =
    "current_branch_closure_reviewed_state_malformed";

pub(crate) const TASK_BOUNDARY_PROJECTION_DIAGNOSTIC_REASON_CODES: &[&str] = &[
    TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_MISSING,
    TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_STALE,
    TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING,
    TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING_LEGACY,
    TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_NOT_INDEPENDENT,
    TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_ARTIFACT_MALFORMED,
    TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_VERIFICATION_SUMMARY_MALFORMED,
];

pub(crate) const PUBLIC_TASK_BOUNDARY_REASON_CODES: &[&str] = &[
    TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING,
    TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE,
    TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID,
    TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED,
    TASK_BOUNDARY_REASON_TASK_CYCLE_BREAK_ACTIVE,
    TASK_BOUNDARY_REASON_CURRENT_TASK_CLOSURE_OVERLAY_RESTORE_REQUIRED,
    TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN,
    TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE,
    TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_BRIDGE_READY,
    TASK_BOUNDARY_REASON_TASK_CLOSURE_RECORDING_READY,
];

pub(crate) fn task_boundary_projection_diagnostic_reason_code(reason_code: &str) -> bool {
    TASK_BOUNDARY_PROJECTION_DIAGNOSTIC_REASON_CODES.contains(&reason_code)
}

pub(crate) fn public_task_boundary_reason_code(reason_code: &str) -> bool {
    PUBLIC_TASK_BOUNDARY_REASON_CODES.contains(&reason_code)
}

pub(crate) fn task_boundary_cycle_break_reason_code(reason_code: &str) -> bool {
    reason_code == TASK_BOUNDARY_REASON_TASK_CYCLE_BREAK_ACTIVE
}

pub(crate) fn task_boundary_overlay_restore_reason_code(reason_code: &str) -> bool {
    reason_code == TASK_BOUNDARY_REASON_CURRENT_TASK_CLOSURE_OVERLAY_RESTORE_REQUIRED
}

pub(crate) fn task_boundary_negative_review_reason_code(reason_code: &str) -> bool {
    reason_code == TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN
}

pub(crate) fn task_boundary_current_closure_missing_reason_code(reason_code: &str) -> bool {
    reason_code == TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
}

pub(crate) fn task_boundary_current_closure_stale_reason_code(reason_code: &str) -> bool {
    reason_code == TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE
}

pub(crate) fn task_boundary_current_closure_structural_reason_code(reason_code: &str) -> bool {
    matches!(
        reason_code,
        TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID
            | TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED
    )
}

pub(crate) fn task_boundary_current_closure_repair_reason_code(reason_code: &str) -> bool {
    task_boundary_current_closure_stale_reason_code(reason_code)
        || task_boundary_current_closure_structural_reason_code(reason_code)
}

pub(crate) fn task_boundary_current_closure_boundary_reason_code(reason_code: &str) -> bool {
    task_boundary_current_closure_missing_reason_code(reason_code)
        || task_boundary_current_closure_repair_reason_code(reason_code)
        || task_boundary_cycle_break_reason_code(reason_code)
}

pub(crate) fn task_boundary_closure_baseline_repair_candidate_reason_code(
    reason_code: &str,
) -> bool {
    reason_code == TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE
}

pub(crate) fn task_boundary_closure_baseline_bridge_ready_reason_code(reason_code: &str) -> bool {
    reason_code == TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_BRIDGE_READY
}

pub(crate) fn task_boundary_closure_recording_ready_reason_code(reason_code: &str) -> bool {
    task_boundary_closure_baseline_repair_candidate_reason_code(reason_code)
        || task_boundary_closure_baseline_bridge_ready_reason_code(reason_code)
        || reason_code == TASK_BOUNDARY_REASON_TASK_CLOSURE_RECORDING_READY
}

pub(crate) fn current_branch_closure_reviewed_state_malformed_reason_code(
    reason_code: &str,
) -> bool {
    reason_code == BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED
}

pub(crate) fn task_boundary_progress_edge_reason_code(reason_code: &str) -> bool {
    task_boundary_current_closure_missing_reason_code(reason_code)
        || task_boundary_current_closure_stale_reason_code(reason_code)
        || task_boundary_overlay_restore_reason_code(reason_code)
}

pub(crate) fn task_boundary_stale_truth_reason_code(reason_code: &str) -> bool {
    task_boundary_current_closure_missing_reason_code(reason_code)
        || task_boundary_current_closure_stale_reason_code(reason_code)
        || task_boundary_closure_baseline_repair_candidate_reason_code(reason_code)
        || task_boundary_closure_baseline_bridge_ready_reason_code(reason_code)
        || task_boundary_cycle_break_reason_code(reason_code)
}

pub(crate) fn task_boundary_blocks_closure_baseline_bridge_reason_code(reason_code: &str) -> bool {
    task_boundary_current_closure_structural_reason_code(reason_code)
        || task_boundary_negative_review_reason_code(reason_code)
}

pub(crate) fn task_boundary_begin_block_reason_code(reason_code: &str) -> bool {
    task_boundary_negative_review_reason_code(reason_code)
        || task_boundary_current_closure_stale_reason_code(reason_code)
        || task_boundary_current_closure_structural_reason_code(reason_code)
        || task_boundary_cycle_break_reason_code(reason_code)
        || task_boundary_overlay_restore_reason_code(reason_code)
}

pub(crate) fn task_boundary_verification_diagnostic_reason_code(reason_code: &str) -> bool {
    matches!(
        reason_code,
        TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING
            | TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING_LEGACY
            | TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_VERIFICATION_SUMMARY_MALFORMED
    )
}

pub(crate) fn task_boundary_pending_review_projection_reason_code(reason_code: &str) -> bool {
    matches!(
        reason_code,
        TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING
            | TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING_LEGACY
            | TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_NOT_INDEPENDENT
            | TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_ARTIFACT_MALFORMED
            | TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_VERIFICATION_SUMMARY_MALFORMED
    )
}

pub(crate) fn reason_codes_include(reason_codes: &[String], predicate: fn(&str) -> bool) -> bool {
    reason_codes.iter().map(String::as_str).any(predicate)
}
