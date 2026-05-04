use super::*;

pub(in crate::execution::commands) fn close_current_task_summary_hashes(
    args: &CloseCurrentTaskArgs,
) -> Result<(String, String), JsonFailure> {
    let review_summary = read_nonempty_summary_file(&args.review_summary_file, "review summary")?;
    let review_summary_hash = summary_hash(&review_summary);
    let verification_summary_hash = if matches!(
        args.verification_result,
        VerificationOutcomeArg::Pass | VerificationOutcomeArg::Fail
    ) {
        let verification_summary = read_nonempty_summary_file(
            args.verification_summary_file.as_ref().ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "verification_summary_required: close-current-task requires --verification-summary-file when --verification-result=pass|fail.",
                )
            })?,
            "verification summary",
        )?;
        summary_hash(&verification_summary)
    } else {
        String::new()
    };
    Ok((review_summary_hash, verification_summary_hash))
}

pub(in crate::execution::commands) fn superseded_branch_closure_ids_from_previous_current(
    overlay: Option<&StatusAuthoritativeOverlay>,
    branch_closure_id: &str,
) -> Vec<String> {
    overlay
        .and_then(|overlay| overlay.current_branch_closure_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != branch_closure_id)
        .map(|value| vec![value.to_owned()])
        .unwrap_or_default()
}

pub(in crate::execution::commands) fn read_nonempty_summary_file(
    path: &Path,
    label: &str,
) -> Result<String, JsonFailure> {
    let source = fs::read_to_string(path).map_err(|error| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!("Could not read {label} file {}: {error}", path.display()),
        )
    })?;
    let normalized = normalize_summary_content(&source);
    if normalized.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!("{label}_empty: {label} file may not be blank after whitespace normalization."),
        ));
    }
    Ok(normalized)
}

pub(in crate::execution::commands) fn optional_summary_hash(path: &Path) -> Option<String> {
    let source = fs::read_to_string(path).ok()?;
    let normalized = normalize_summary_content(&source);
    if normalized.is_empty() {
        return None;
    }
    Some(summary_hash(&normalized))
}

pub(in crate::execution::commands) fn current_plan_requires_browser_qa(
    context: &ExecutionContext,
) -> Option<bool> {
    match context.plan_document.qa_requirement.as_deref() {
        Some("required") => Some(true),
        Some("not-required") => Some(false),
        _ => None,
    }
}
