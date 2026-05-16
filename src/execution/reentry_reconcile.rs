use crate::execution::state::{PlanExecutionStatus, StatusBlockingRecord};

pub(crate) const TARGETLESS_STALE_RECONCILE_REASON_CODE: &str = "stale_unreviewed_target_missing";
pub(crate) const TARGETLESS_STALE_MISSING_AUTHORITY_CODE: &str =
    "missing_authoritative_stale_target";
pub(crate) const TARGETLESS_STALE_RECONCILE_PHASE_DETAIL: &str =
    crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED;
pub(crate) const TARGETLESS_STALE_RECONCILE_DETAIL: &str = "Review state is stale_unreviewed but no authoritative stale task, branch, or milestone target is bound.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetlessStaleAuthority {
    has_authoritative_stale_target: bool,
}

impl TargetlessStaleAuthority {
    pub const fn new(has_authoritative_stale_target: bool) -> Self {
        Self {
            has_authoritative_stale_target,
        }
    }

    pub const fn has_authoritative_stale_target(self) -> bool {
        self.has_authoritative_stale_target
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TargetlessStaleReconcile;

impl TargetlessStaleReconcile {
    pub(crate) fn missing_reentry_target_requires_reconcile(
        status: &PlanExecutionStatus,
        review_state_status: &str,
    ) -> bool {
        review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
            || status.review_state_status
                == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
            || status.phase_detail == crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
    }

    pub(crate) fn status_needs_marker(
        review_state_status: &str,
        stale_unreviewed_closures: &[String],
        has_concrete_public_target: bool,
        has_authoritative_stale_target: bool,
    ) -> bool {
        review_state_status == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
            && stale_unreviewed_closures.is_empty()
            && !has_concrete_public_target
            && !has_authoritative_stale_target
    }

    pub(crate) fn status_needs_marker_with_authority(
        status: &PlanExecutionStatus,
        has_authoritative_stale_target: bool,
    ) -> bool {
        if status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code.as_str() == crate::execution::review_route_tokens::REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY)
        {
            return false;
        }
        Self::status_needs_marker(
            &status.review_state_status,
            &status.stale_unreviewed_closures,
            status_has_concrete_public_stale_target(status),
            has_authoritative_stale_target,
        )
    }

    pub(crate) fn status_needs_marker_for_authority(
        status: &PlanExecutionStatus,
        authority: TargetlessStaleAuthority,
    ) -> bool {
        Self::status_needs_marker_with_authority(status, authority.has_authoritative_stale_target())
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
            && status.review_state_status
                == crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED
            && status.stale_unreviewed_closures.is_empty()
            && !status_has_concrete_public_stale_target(status)
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
        if !Self::status_has_diagnostic(status) {
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
        .chain(status.blocking_reason_codes.iter())
        .any(|code| code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE)
}

fn status_has_concrete_public_stale_target(status: &PlanExecutionStatus) -> bool {
    status.blocking_task.is_some() || task_closure_baseline_repair_candidate_reason_present(status)
}
