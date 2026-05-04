use crate::execution::state::{PlanExecutionStatus, StatusBlockingRecord};

pub(crate) const TARGETLESS_STALE_RECONCILE_REASON_CODE: &str = "stale_unreviewed_target_missing";
pub(crate) const TARGETLESS_STALE_MISSING_AUTHORITY_CODE: &str =
    "missing_authoritative_stale_target";
pub(crate) const TARGETLESS_STALE_RECONCILE_PHASE_DETAIL: &str =
    crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED;
pub(crate) const TARGETLESS_STALE_RECONCILE_DETAIL: &str = "Review state is stale_unreviewed but no authoritative stale task, branch, or milestone target is bound.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TargetlessStaleReconcile;

impl TargetlessStaleReconcile {
    pub(crate) fn missing_reentry_target_requires_reconcile(
        status: &PlanExecutionStatus,
        review_state_status: &str,
    ) -> bool {
        review_state_status == "stale_unreviewed"
            || status.review_state_status == "stale_unreviewed"
            || status.phase_detail == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
    }

    pub(crate) fn status_needs_marker(
        review_state_status: &str,
        stale_unreviewed_closures: &[String],
        _has_task_closure_baseline_candidate: bool,
        has_authoritative_stale_target: bool,
    ) -> bool {
        review_state_status == "stale_unreviewed"
            && stale_unreviewed_closures.is_empty()
            && !has_authoritative_stale_target
    }

    pub(crate) fn status_needs_marker_with_authority(
        status: &PlanExecutionStatus,
        has_authoritative_stale_target: bool,
    ) -> bool {
        if status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code.as_str() == "negative_result_requires_execution_reentry")
        {
            return false;
        }
        Self::status_needs_marker(
            &status.review_state_status,
            &status.stale_unreviewed_closures,
            task_closure_baseline_repair_candidate_reason_present(status),
            has_authoritative_stale_target,
        )
    }

    pub(crate) fn status_needs_marker_for_status(status: &PlanExecutionStatus) -> bool {
        Self::status_has_diagnostic(status)
    }

    pub(crate) fn from_reason_code(reason_code: &str) -> Option<Self> {
        (reason_code == TARGETLESS_STALE_RECONCILE_REASON_CODE).then_some(Self)
    }

    pub(crate) fn from_phase_and_reason_codes(
        phase_detail: &str,
        reason_codes: &[String],
    ) -> Option<Self> {
        Self::from_phase_and_reason_code_strs(phase_detail, reason_codes.iter().map(String::as_str))
    }

    pub(crate) fn from_phase_and_reason_code_strs<'a>(
        phase_detail: &str,
        reason_codes: impl IntoIterator<Item = &'a str>,
    ) -> Option<Self> {
        (phase_detail == TARGETLESS_STALE_RECONCILE_PHASE_DETAIL
            && reason_codes
                .into_iter()
                .any(|code| code == TARGETLESS_STALE_RECONCILE_REASON_CODE))
        .then_some(Self)
    }

    pub(crate) fn status_has_diagnostic(status: &PlanExecutionStatus) -> bool {
        status.phase_detail == TARGETLESS_STALE_RECONCILE_PHASE_DETAIL
            && status
                .reason_codes
                .iter()
                .any(|reason_code| reason_code == TARGETLESS_STALE_RECONCILE_REASON_CODE)
            && status
                .blocking_reason_codes
                .iter()
                .any(|reason_code| reason_code == TARGETLESS_STALE_MISSING_AUTHORITY_CODE)
    }

    pub(crate) fn ensure_reason_codes(reason_codes: &mut Vec<String>) {
        push_reason_once(reason_codes, TARGETLESS_STALE_RECONCILE_REASON_CODE);
        push_reason_once(reason_codes, TARGETLESS_STALE_MISSING_AUTHORITY_CODE);
    }

    pub(crate) fn ensure_status_diagnostic(status: &mut PlanExecutionStatus) {
        push_reason_once(
            &mut status.reason_codes,
            TARGETLESS_STALE_RECONCILE_REASON_CODE,
        );
        Self::ensure_reason_codes(&mut status.blocking_reason_codes);
    }

    pub(crate) fn clear_status_diagnostic(status: &mut PlanExecutionStatus) {
        status
            .reason_codes
            .retain(|code| code != TARGETLESS_STALE_RECONCILE_REASON_CODE);
        status.blocking_reason_codes.retain(|code| {
            code != TARGETLESS_STALE_RECONCILE_REASON_CODE
                && code != TARGETLESS_STALE_MISSING_AUTHORITY_CODE
        });
    }

    pub(crate) fn status_blocking_record(
        status: &PlanExecutionStatus,
    ) -> Option<StatusBlockingRecord> {
        if !Self::status_needs_marker_for_status(status) && !Self::status_has_diagnostic(status) {
            return None;
        }
        Some(StatusBlockingRecord {
            code: String::from(TARGETLESS_STALE_RECONCILE_REASON_CODE),
            scope_type: String::from("runtime"),
            scope_key: String::from("targetless_stale_unreviewed"),
            record_type: String::from("review_state"),
            record_id: None,
            review_state_status: status.review_state_status.clone(),
            required_follow_up: None,
            message: String::from(TARGETLESS_STALE_RECONCILE_DETAIL),
        })
    }

    pub(crate) fn detail(&self) -> &'static str {
        TARGETLESS_STALE_RECONCILE_DETAIL
    }
}

pub(crate) fn push_reason_once(reason_codes: &mut Vec<String>, reason_code: &'static str) {
    if !reason_codes.iter().any(|existing| existing == reason_code) {
        reason_codes.push(reason_code.to_owned());
    }
}

pub(crate) fn task_closure_baseline_repair_candidate_reason_present(
    status: &PlanExecutionStatus,
) -> bool {
    status
        .reason_codes
        .iter()
        .any(|code| code == "task_closure_baseline_repair_candidate")
}
