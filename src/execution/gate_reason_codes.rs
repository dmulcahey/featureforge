pub(crate) const FINISH_REVIEW_GATE_ALREADY_CURRENT: &str = "finish_review_gate_already_current";
pub(crate) const FILES_PROVEN_DRIFTED: &str = "files_proven_drifted";
pub(crate) const QA_REQUIREMENT_MISSING_OR_INVALID: &str = "qa_requirement_missing_or_invalid";

pub(crate) fn finish_review_gate_already_current_reason_code(reason_code: &str) -> bool {
    reason_code == FINISH_REVIEW_GATE_ALREADY_CURRENT
}

pub(crate) fn files_proven_drifted_reason_code(reason_code: &str) -> bool {
    reason_code == FILES_PROVEN_DRIFTED
}

pub(crate) fn qa_requirement_missing_or_invalid_reason_code(reason_code: &str) -> bool {
    reason_code == QA_REQUIREMENT_MISSING_OR_INVALID
}

pub(crate) fn push_files_proven_drifted_reason_code_once(reason_codes: &mut Vec<String>) {
    if !reason_codes
        .iter()
        .any(|reason_code| files_proven_drifted_reason_code(reason_code))
    {
        reason_codes.push(String::from(FILES_PROVEN_DRIFTED));
    }
}
