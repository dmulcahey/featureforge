use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::execution::commands) struct CurrentBranchClosureBinding {
    pub(in crate::execution::commands) branch_closure_id: String,
    pub(in crate::execution::commands) reviewed_state_id: String,
    pub(in crate::execution::commands) semantic_reviewed_state_id: Option<String>,
}

pub(in crate::execution::commands) fn current_authoritative_branch_closure_binding_optional(
    context: &ExecutionContext,
) -> Result<Option<CurrentBranchClosureBinding>, JsonFailure> {
    Ok(
        usable_current_branch_closure_identity(context).map(|current_identity| {
            CurrentBranchClosureBinding {
                branch_closure_id: current_identity.branch_closure_id,
                reviewed_state_id: current_identity.reviewed_state_id,
                semantic_reviewed_state_id: current_identity.semantic_reviewed_state_id,
            }
        }),
    )
}

pub(in crate::execution::commands) fn current_authoritative_branch_closure_id_optional(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    Ok(
        current_authoritative_branch_closure_binding_optional(context)?
            .map(|binding| binding.branch_closure_id),
    )
}

pub(in crate::execution::commands) fn authoritative_current_branch_closure_binding(
    context: &ExecutionContext,
    command_label: &str,
) -> Result<CurrentBranchClosureBinding, JsonFailure> {
    let Some(current_identity) = current_authoritative_branch_closure_binding_optional(context)?
    else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!("{command_label} requires a current branch closure."),
        ));
    };
    Ok(current_identity)
}

pub(in crate::execution::commands) fn equivalent_current_release_readiness_rerun(
    context: &ExecutionContext,
    current_branch_closure: &CurrentBranchClosureBinding,
    stage_path: &str,
    operation: &str,
    result: &str,
    summary_file: &Path,
) -> Result<Option<AdvanceLateStageOutput>, JsonFailure> {
    let Some(candidate_summary_hash) = optional_summary_hash(summary_file) else {
        return Ok(None);
    };
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(None);
    };
    let Some(record) = authoritative_state.current_release_readiness_record() else {
        return Ok(None);
    };
    if record.branch_closure_id != current_branch_closure.branch_closure_id
        || record.result != result
        || record.summary_hash != candidate_summary_hash
    {
        return Ok(None);
    }
    let recovery = public_recovery_contract_for_follow_up(
        Path::new(&context.plan_rel),
        None,
        (result == "blocked").then(|| String::from("resolve_release_blocker")),
        PublicFollowUpInputProfile::ReleaseReadiness,
    );
    Ok(Some(AdvanceLateStageOutput {
        action: String::from("already_current"),
        stage_path: stage_path.to_owned(),
        intent: String::from("advance_late_stage"),
        operation: operation.to_owned(),
        branch_closure_id: Some(current_branch_closure.branch_closure_id.clone()),
        dispatch_id: None,
        result: result.to_owned(),
        code: None,
        recommended_command: recovery.recommended_command,
        recommended_public_command_argv: recovery.recommended_public_command_argv,
        required_inputs: recovery.required_inputs,
        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
        required_follow_up: recovery.required_follow_up,
        trace_summary: String::from(
            "Current branch closure already has an equivalent recorded release-readiness outcome.",
        ),
    }))
}

pub(in crate::execution::commands) fn equivalent_current_final_review_rerun(
    context: &ExecutionContext,
    current_branch_closure: &CurrentBranchClosureBinding,
    params: EquivalentFinalReviewRerunParams<'_>,
) -> Result<Option<AdvanceLateStageOutput>, JsonFailure> {
    let Some(candidate_summary_hash) = optional_summary_hash(params.summary_file) else {
        return Ok(None);
    };
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(None);
    };
    let matches_current_record = authoritative_state.current_final_review_branch_closure_id()
        == Some(current_branch_closure.branch_closure_id.as_str())
        && authoritative_state.current_final_review_dispatch_id() == Some(params.dispatch_id)
        && authoritative_state.current_final_review_reviewer_source()
            == Some(params.reviewer_source)
        && authoritative_state.current_final_review_reviewer_id() == Some(params.reviewer_id)
        && authoritative_state.current_final_review_result() == Some(params.result)
        && authoritative_state.current_final_review_summary_hash()
            == Some(candidate_summary_hash.as_str());
    if !matches_current_record {
        return Ok(None);
    }
    if !current_final_review_record_is_still_authoritative(
        context,
        authoritative_state,
        CurrentFinalReviewAuthorityCheck {
            branch_closure_id: &current_branch_closure.branch_closure_id,
            dispatch_id: params.dispatch_id,
            reviewer_source: params.reviewer_source,
            reviewer_id: params.reviewer_id,
            result: params.result,
            normalized_summary_hash: &candidate_summary_hash,
        },
    )? {
        return Ok(None);
    }
    let recovery = public_recovery_contract_for_follow_up(
        Path::new(&context.plan_rel),
        None,
        params.required_follow_up,
        PublicFollowUpInputProfile::FinalReview,
    );
    Ok(Some(AdvanceLateStageOutput {
        action: String::from("already_current"),
        stage_path: params.stage_path.to_owned(),
        intent: String::from("advance_late_stage"),
        operation: params.operation.to_owned(),
        branch_closure_id: Some(current_branch_closure.branch_closure_id.clone()),
        dispatch_id: Some(params.dispatch_id.to_owned()),
        result: params.result.to_owned(),
        code: None,
        recommended_command: recovery.recommended_command,
        recommended_public_command_argv: recovery.recommended_public_command_argv,
        required_inputs: recovery.required_inputs,
        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
        required_follow_up: recovery.required_follow_up,
        trace_summary: String::from(
            "Current branch closure already has an equivalent recorded final-review outcome.",
        ),
    }))
}

pub(in crate::execution::commands) fn equivalent_current_browser_qa_rerun(
    context: &ExecutionContext,
    current_branch_closure: &CurrentBranchClosureBinding,
    gate_snapshot: &RuntimeGateSnapshot,
    result: &str,
    summary_file: &Path,
    required_follow_up: Option<String>,
) -> Result<Option<RecordQaOutput>, JsonFailure> {
    let Some(candidate_summary_hash) = optional_summary_hash(summary_file) else {
        return Ok(None);
    };
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(None);
    };
    let matches_current_record = authoritative_state.current_qa_branch_closure_id()
        == Some(current_branch_closure.branch_closure_id.as_str())
        && authoritative_state.current_qa_result() == Some(result)
        && authoritative_state.current_qa_summary_hash() == Some(candidate_summary_hash.as_str());
    if !matches_current_record {
        return Ok(None);
    }
    if rerun_invalidated_by_repo_writes(
        gate_snapshot.gate_review.as_ref(),
        gate_snapshot.gate_finish.as_ref(),
    ) {
        return Ok(None);
    }
    let current_record = authoritative_state.current_browser_qa_record();
    if current_record
        .as_ref()
        .and_then(|record| record.source_test_plan_fingerprint.as_deref())
        .map(str::trim)
        .filter(|fingerprint| !fingerprint.is_empty())
        .is_none()
    {
        match current_test_plan_artifact_path(context) {
            Ok(_) => {}
            Err(error)
                if error.error_class == FailureClass::ExecutionStateNotReady.as_str()
                    || error.error_class == FailureClass::QaArtifactNotFresh.as_str() =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
    }
    let recovery = public_recovery_contract_for_follow_up(
        Path::new(&context.plan_rel),
        None,
        required_follow_up,
        PublicFollowUpInputProfile::None,
    );
    Ok(Some(RecordQaOutput {
        action: String::from("already_current"),
        branch_closure_id: current_branch_closure.branch_closure_id.clone(),
        result: result.to_owned(),
        code: None,
        recommended_command: recovery.recommended_command,
        recommended_public_command_argv: recovery.recommended_public_command_argv,
        required_inputs: recovery.required_inputs,
        rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
        required_follow_up: recovery.required_follow_up,
        trace_summary: String::from(
            "Current branch closure already has an equivalent recorded browser QA outcome.",
        ),
    }))
}

pub(in crate::execution::commands) fn equivalent_current_browser_qa_rerun_allowed(
    operator: &ExecutionRoutingState,
    result: &str,
) -> bool {
    if operator.review_state_status != "clean" {
        return false;
    }
    match result {
        "pass" => {
            operator.phase == crate::execution::phase::PHASE_QA_PENDING
                && operator.phase_detail == crate::execution::phase::DETAIL_QA_RECORDING_REQUIRED
        }
        "fail" => matches!(
            operator.phase_detail.as_str(),
            crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
                | crate::execution::phase::DETAIL_HANDOFF_RECORDING_REQUIRED
                | crate::execution::phase::DETAIL_PLANNING_REENTRY_REQUIRED
        ),
        _ => false,
    }
}

pub(in crate::execution::commands) fn rerun_invalidated_by_repo_writes(
    gate_review: Option<&crate::execution::state::GateResult>,
    gate_finish: Option<&crate::execution::state::GateResult>,
) -> bool {
    const REPO_WRITE_INVALIDATION_CODES: &[&str] = &[
        "review_artifact_worktree_dirty",
        REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
    ];
    let gate_has_reason = |gate: Option<&crate::execution::state::GateResult>| {
        gate.is_some_and(|gate| {
            gate.reason_codes.iter().any(|code| {
                REPO_WRITE_INVALIDATION_CODES
                    .iter()
                    .any(|expected| code == expected)
            })
        })
    };
    gate_has_reason(gate_review) || gate_has_reason(gate_finish)
}

pub(in crate::execution::commands) fn current_test_plan_artifact_path(
    context: &ExecutionContext,
) -> Result<PathBuf, JsonFailure> {
    current_test_plan_artifact_path_for_qa_recording(context)
}

pub(in crate::execution::commands) fn current_authoritative_test_plan_path_from_qa_record(
    runtime: &ExecutionRuntime,
    authoritative_state: &AuthoritativeTransitionState,
    branch_closure_id: &str,
    final_review_record_id: &str,
) -> Option<PathBuf> {
    let record = authoritative_state.current_browser_qa_record()?;
    if record.branch_closure_id != branch_closure_id
        || record.final_review_record_id.as_deref() != Some(final_review_record_id)
    {
        return None;
    }
    let fingerprint = record
        .source_test_plan_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("test-plan-{fingerprint}.md"),
    ))
}
