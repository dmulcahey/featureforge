use super::*;

pub(in crate::execution::commands) fn ensure_task_dispatch_id_matches(
    context: &ExecutionContext,
    task: u32,
    dispatch_id: &str,
) -> Result<(), JsonFailure> {
    let lineage_key = format!("task-{task}");
    let overlay = load_status_authoritative_overlay_checked(context)?;
    let expected_dispatch_from_lineage = overlay
        .as_ref()
        .and_then(|overlay| {
            overlay
                .strategy_review_dispatch_lineage
                .get(&lineage_key)
                .and_then(|record| record.dispatch_id.as_deref())
                .map(str::to_owned)
        })
        .or_else(|| {
            load_authoritative_transition_state(context)
                .ok()
                .flatten()
                .and_then(|state| state.task_review_dispatch_id(task))
        })
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if let Some(expected_dispatch) = expected_dispatch_from_lineage.as_deref() {
        if expected_dispatch != dispatch_id.trim() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                format!(
                    "dispatch_id_mismatch: close-current-task expected dispatch `{expected_dispatch}` for task {task}."
                ),
            ));
        }
        return Ok(());
    }
    Err(JsonFailure::new(
        FailureClass::ExecutionStateNotReady,
        format!(
            "close-current-task requires a current task review dispatch lineage for task {task}."
        ),
    ))
}

pub(in crate::execution::commands) fn task_dispatch_reviewed_state_status(
    context: &ExecutionContext,
    task: u32,
    semantic_reviewed_state_id: &str,
    raw_reviewed_state_id: &str,
) -> Result<TaskDispatchReviewedStateStatus, JsonFailure> {
    let overlay = load_status_authoritative_overlay_checked(context)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "close-current-task requires authoritative review-dispatch lineage state.",
        )
    })?;
    let lineage_key = format!("task-{task}");
    let recorded_semantic_reviewed_state_id = overlay
        .strategy_review_dispatch_lineage
        .get(&lineage_key)
        .and_then(|record| record.semantic_reviewed_state_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let recorded_raw_reviewed_state_id = overlay
        .strategy_review_dispatch_lineage
        .get(&lineage_key)
        .and_then(|record| record.reviewed_state_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    Ok(
        match (
            recorded_semantic_reviewed_state_id,
            recorded_raw_reviewed_state_id,
        ) {
            (Some(recorded), _) if recorded == semantic_reviewed_state_id.trim() => {
                TaskDispatchReviewedStateStatus::Current
            }
            (Some(_), _) => TaskDispatchReviewedStateStatus::StaleReviewedState,
            (None, Some(recorded)) if recorded == raw_reviewed_state_id.trim() => {
                TaskDispatchReviewedStateStatus::Current
            }
            (None, Some(_)) => TaskDispatchReviewedStateStatus::StaleReviewedState,
            (None, None) => TaskDispatchReviewedStateStatus::MissingReviewedStateBinding,
        },
    )
}

pub(in crate::execution::commands) fn ensure_final_review_dispatch_id_matches(
    context: &ExecutionContext,
    dispatch_id: &str,
) -> Result<(), JsonFailure> {
    let current_branch_closure =
        authoritative_current_branch_closure_binding(context, "advance-late-stage final-review")?;
    let overlay = load_status_authoritative_overlay_checked(context)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "advance-late-stage final-review path requires authoritative dispatch lineage state.",
        )
    })?;
    let expected_dispatch_from_lineage = overlay
        .final_review_dispatch_lineage
        .as_ref()
        .and_then(|record| {
            let expected_branch_closure_id = record.branch_closure_id.as_deref()?;
            if current_branch_closure.branch_closure_id != expected_branch_closure_id {
                return None;
            }
            record.dispatch_id.as_deref()
        })
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(expected_dispatch) = expected_dispatch_from_lineage {
        if expected_dispatch != dispatch_id.trim() {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                format!(
                    "dispatch_id_mismatch: advance-late-stage expected final-review dispatch `{expected_dispatch}`."
                ),
            ));
        }
        return Ok(());
    }
    Err(JsonFailure::new(
        FailureClass::ExecutionStateNotReady,
        "advance-late-stage final-review path requires a current final-review dispatch lineage.",
    ))
}

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
