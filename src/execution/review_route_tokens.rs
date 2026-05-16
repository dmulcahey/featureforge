//! Central tokens for review-state and repair follow-up route decisions.
//!
//! These literals are serialized public/runtime contract values. Production
//! routing code should compare through these constants so status, router,
//! repair, and command surfaces do not drift through local string spelling.

use crate::diagnostics::FailureClass;
use crate::execution::observability::{
    REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED, REASON_CODE_STALE_PROVENANCE,
};

pub(crate) const REVIEW_STATE_STALE_UNREVIEWED: &str = "stale_unreviewed";
pub(crate) const REVIEW_STATE_MISSING_CURRENT_CLOSURE: &str = "missing_current_closure";

pub const PUBLIC_REVIEW_STATE_STATUS_VALUES: &[&str] = &[
    "clean",
    REVIEW_STATE_STALE_UNREVIEWED,
    REVIEW_STATE_MISSING_CURRENT_CLOSURE,
];

pub(crate) const FOLLOW_UP_REPAIR_REVIEW_STATE: &str = "repair_review_state";
pub(crate) const FOLLOW_UP_ADVANCE_LATE_STAGE: &str = "advance_late_stage";
pub(crate) const FOLLOW_UP_EXECUTION_REENTRY: &str = "execution_reentry";
pub(crate) const FOLLOW_UP_CLOSE_CURRENT_TASK: &str = "close_current_task";
pub(crate) const FOLLOW_UP_REQUEST_EXTERNAL_REVIEW: &str = "request_external_review";
pub(crate) const FOLLOW_UP_WAIT_FOR_EXTERNAL_REVIEW_RESULT: &str =
    "wait_for_external_review_result";
pub(crate) const FOLLOW_UP_RUN_VERIFICATION: &str = "run_verification";
pub(crate) const FOLLOW_UP_RESOLVE_RELEASE_BLOCKER: &str = "resolve_release_blocker";
pub(crate) const FOLLOW_UP_RECORD_HANDOFF: &str = "record_handoff";
pub(crate) const FOLLOW_UP_GATE_REVIEW: &str = "gate_review";
pub(crate) const FOLLOW_UP_GATE_FINISH: &str = "gate_finish";

pub const REQUIRED_FOLLOW_UP_SCHEMA_VALUES: &[&str] = &[
    FOLLOW_UP_EXECUTION_REENTRY,
    FOLLOW_UP_REPAIR_REVIEW_STATE,
    FOLLOW_UP_REQUEST_EXTERNAL_REVIEW,
    FOLLOW_UP_RUN_VERIFICATION,
    FOLLOW_UP_ADVANCE_LATE_STAGE,
    FOLLOW_UP_RESOLVE_RELEASE_BLOCKER,
    FOLLOW_UP_RECORD_HANDOFF,
];

pub(crate) const OUT_OF_PHASE_REQUERY_REQUIRED_CODE: &str = "out_of_phase_requery_required";

pub(crate) const REASON_DERIVED_REVIEW_STATE_MISSING: &str = "derived_review_state_missing";
pub(crate) const REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY: &str =
    "negative_result_requires_execution_reentry";
pub(crate) const REASON_RELEASE_DOCS_STATE_MISSING: &str = "release_docs_state_missing";
pub(crate) const REASON_RELEASE_DOCS_STATE_STALE: &str = "release_docs_state_stale";
pub(crate) const REASON_RELEASE_DOCS_STATE_NOT_FRESH: &str = "release_docs_state_not_fresh";
pub(crate) const REASON_FINAL_REVIEW_STATE_MISSING: &str = "final_review_state_missing";
pub(crate) const REASON_FINAL_REVIEW_STATE_STALE: &str = "final_review_state_stale";
pub(crate) const REASON_FINAL_REVIEW_STATE_NOT_FRESH: &str = "final_review_state_not_fresh";
pub(crate) const REASON_BROWSER_QA_STATE_MISSING: &str = "browser_qa_state_missing";
pub(crate) const REASON_BROWSER_QA_STATE_STALE: &str = "browser_qa_state_stale";
pub(crate) const REASON_BROWSER_QA_STATE_NOT_FRESH: &str = "browser_qa_state_not_fresh";
pub(crate) const REASON_PLAN_FINGERPRINT_MISMATCH: &str = "plan_fingerprint_mismatch";

pub(crate) const RELEASE_DOCS_FRESHNESS_REASON_CODES: &[&str] = &[
    REASON_RELEASE_DOCS_STATE_MISSING,
    REASON_RELEASE_DOCS_STATE_STALE,
    REASON_RELEASE_DOCS_STATE_NOT_FRESH,
];

pub(crate) const FINAL_REVIEW_FRESHNESS_REASON_CODES: &[&str] = &[
    REASON_FINAL_REVIEW_STATE_MISSING,
    REASON_FINAL_REVIEW_STATE_STALE,
    REASON_FINAL_REVIEW_STATE_NOT_FRESH,
];

pub(crate) const FINAL_REVIEW_REFRESH_REASON_CODES: &[&str] = &[
    REASON_FINAL_REVIEW_STATE_STALE,
    REASON_FINAL_REVIEW_STATE_NOT_FRESH,
];

pub(crate) const BROWSER_QA_FRESHNESS_REASON_CODES: &[&str] = &[
    REASON_BROWSER_QA_STATE_MISSING,
    REASON_BROWSER_QA_STATE_STALE,
    REASON_BROWSER_QA_STATE_NOT_FRESH,
];

pub(crate) fn is_release_docs_freshness_reason(reason_code: &str) -> bool {
    RELEASE_DOCS_FRESHNESS_REASON_CODES.contains(&reason_code)
}

pub(crate) fn is_final_review_freshness_reason(reason_code: &str) -> bool {
    FINAL_REVIEW_FRESHNESS_REASON_CODES.contains(&reason_code)
}

pub(crate) fn is_final_review_refresh_reason(reason_code: &str) -> bool {
    FINAL_REVIEW_REFRESH_REASON_CODES.contains(&reason_code)
}

pub(crate) fn is_browser_qa_freshness_reason(reason_code: &str) -> bool {
    BROWSER_QA_FRESHNESS_REASON_CODES.contains(&reason_code)
}

#[cfg(test)]
pub(crate) fn is_late_stage_freshness_reason(reason_code: &str) -> bool {
    is_release_docs_freshness_reason(reason_code)
        || is_final_review_freshness_reason(reason_code)
        || is_browser_qa_freshness_reason(reason_code)
}

pub(crate) const EXTERNAL_WAITING_FOR_EXTERNAL_REVIEW_RESULT: &str =
    "waiting_for_external_review_result";

const DOCTOR_SYNTHETIC_GATE_REVIEW_REASON_CODES: &[&str] = &[
    REASON_CODE_STALE_PROVENANCE,
    REVIEW_STATE_STALE_UNREVIEWED,
    REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
    REASON_FINAL_REVIEW_STATE_NOT_FRESH,
    REASON_BROWSER_QA_STATE_NOT_FRESH,
    REASON_RELEASE_DOCS_STATE_NOT_FRESH,
    REASON_PLAN_FINGERPRINT_MISMATCH,
];

const DOCTOR_SYNTHETIC_STALE_PROVENANCE_REASON_CODES: &[&str] = &[
    REASON_CODE_STALE_PROVENANCE,
    REVIEW_STATE_STALE_UNREVIEWED,
    REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
    REASON_PLAN_FINGERPRINT_MISMATCH,
];

pub(crate) fn doctor_synthetic_gate_review_reason_code(reason_code: &str) -> bool {
    DOCTOR_SYNTHETIC_GATE_REVIEW_REASON_CODES.contains(&reason_code)
}

pub(crate) fn doctor_synthetic_gate_review_failure_class<'a>(
    reason_codes: impl IntoIterator<Item = &'a str>,
) -> &'static str {
    if reason_codes
        .into_iter()
        .any(|reason_code| DOCTOR_SYNTHETIC_STALE_PROVENANCE_REASON_CODES.contains(&reason_code))
    {
        FailureClass::StaleProvenance.as_str()
    } else {
        FailureClass::ExecutionStateNotReady.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        REASON_BROWSER_QA_STATE_MISSING, REASON_BROWSER_QA_STATE_NOT_FRESH,
        REASON_BROWSER_QA_STATE_STALE, REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
        REASON_CODE_STALE_PROVENANCE, REASON_FINAL_REVIEW_STATE_MISSING,
        REASON_FINAL_REVIEW_STATE_NOT_FRESH, REASON_FINAL_REVIEW_STATE_STALE,
        REASON_PLAN_FINGERPRINT_MISMATCH, REASON_RELEASE_DOCS_STATE_MISSING,
        REASON_RELEASE_DOCS_STATE_NOT_FRESH, REASON_RELEASE_DOCS_STATE_STALE,
        REVIEW_STATE_STALE_UNREVIEWED, doctor_synthetic_gate_review_failure_class,
        doctor_synthetic_gate_review_reason_code, is_browser_qa_freshness_reason,
        is_final_review_freshness_reason, is_final_review_refresh_reason,
        is_late_stage_freshness_reason, is_release_docs_freshness_reason,
    };
    use crate::diagnostics::FailureClass;

    #[test]
    fn doctor_synthetic_gate_review_classification_is_execution_owned() {
        for reason_code in [
            REASON_CODE_STALE_PROVENANCE,
            REVIEW_STATE_STALE_UNREVIEWED,
            REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
            REASON_FINAL_REVIEW_STATE_NOT_FRESH,
            REASON_BROWSER_QA_STATE_NOT_FRESH,
            REASON_RELEASE_DOCS_STATE_NOT_FRESH,
            REASON_PLAN_FINGERPRINT_MISMATCH,
        ] {
            assert!(
                doctor_synthetic_gate_review_reason_code(reason_code),
                "doctor synthetic gate-review helper should classify {reason_code}"
            );
        }

        assert_eq!(
            doctor_synthetic_gate_review_failure_class([
                REASON_FINAL_REVIEW_STATE_NOT_FRESH,
                REASON_RELEASE_DOCS_STATE_NOT_FRESH,
            ]),
            FailureClass::ExecutionStateNotReady.as_str()
        );
        assert_eq!(
            doctor_synthetic_gate_review_failure_class([
                REASON_RELEASE_DOCS_STATE_NOT_FRESH,
                REVIEW_STATE_STALE_UNREVIEWED,
            ]),
            FailureClass::StaleProvenance.as_str()
        );
    }

    #[test]
    fn late_stage_freshness_reason_classification_is_centralized() {
        for reason_code in [
            REASON_RELEASE_DOCS_STATE_MISSING,
            REASON_RELEASE_DOCS_STATE_STALE,
            REASON_RELEASE_DOCS_STATE_NOT_FRESH,
        ] {
            assert!(is_release_docs_freshness_reason(reason_code));
            assert!(is_late_stage_freshness_reason(reason_code));
        }

        for reason_code in [
            REASON_FINAL_REVIEW_STATE_MISSING,
            REASON_FINAL_REVIEW_STATE_STALE,
            REASON_FINAL_REVIEW_STATE_NOT_FRESH,
        ] {
            assert!(is_final_review_freshness_reason(reason_code));
            assert!(is_late_stage_freshness_reason(reason_code));
        }

        for reason_code in [
            REASON_BROWSER_QA_STATE_MISSING,
            REASON_BROWSER_QA_STATE_STALE,
            REASON_BROWSER_QA_STATE_NOT_FRESH,
        ] {
            assert!(is_browser_qa_freshness_reason(reason_code));
            assert!(is_late_stage_freshness_reason(reason_code));
        }

        assert!(is_final_review_refresh_reason(
            REASON_FINAL_REVIEW_STATE_STALE
        ));
        assert!(is_final_review_refresh_reason(
            REASON_FINAL_REVIEW_STATE_NOT_FRESH
        ));
        assert!(!is_final_review_refresh_reason(
            REASON_FINAL_REVIEW_STATE_MISSING
        ));
        assert!(!is_late_stage_freshness_reason("review_artifact_malformed"));
        assert!(!is_late_stage_freshness_reason(
            "test_plan_artifact_authoritative_provenance_invalid"
        ));
    }
}
