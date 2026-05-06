use super::*;

pub(in crate::execution::commands) fn consume_execution_reentry_repair_follow_up(
    authoritative_state: Option<&mut AuthoritativeTransitionState>,
) -> Result<bool, JsonFailure> {
    let Some(authoritative_state) = authoritative_state else {
        return Ok(false);
    };
    if authoritative_state
        .review_state_repair_follow_up_record()
        .is_none_or(|record| record.kind.public_token() != "execution_reentry")
    {
        return Ok(false);
    }
    authoritative_state.set_review_state_repair_follow_up(None)?;
    authoritative_state.set_harness_phase_executing()?;
    Ok(true)
}

pub(in crate::execution::commands) fn current_open_step_authoritative_sequence(
    context: &ExecutionContext,
) -> u64 {
    load_status_authoritative_overlay_checked(context)
        .ok()
        .flatten()
        .and_then(|overlay| {
            overlay
                .latest_authoritative_sequence
                .or(overlay.authoritative_sequence)
        })
        .unwrap_or(1)
}

pub(in crate::execution::commands) fn open_step_state_record(
    context: &ExecutionContext,
    task: u32,
    step: u32,
    note_state: crate::execution::state::NoteState,
    note_summary: &str,
) -> OpenStepStateRecord {
    OpenStepStateRecord {
        task,
        step,
        note_state: note_state.as_str().to_owned(),
        note_summary: truncate_summary(note_summary),
        execution_mode: Some(context.plan_document.execution_mode.clone()),
        repo_root: Some(context.runtime.repo_root.to_string_lossy().into_owned()),
        source_plan_path: context.plan_rel.clone(),
        source_plan_revision: context.plan_document.plan_revision,
        authoritative_sequence: current_open_step_authoritative_sequence(context),
    }
}

pub(in crate::execution::commands) fn begin_failure_class_from_blocking_reason_codes(
    reason_codes: &[String],
) -> FailureClass {
    if reason_codes
        .iter()
        .any(|reason_code| reason_code == "prior_task_current_closure_reviewed_state_malformed")
    {
        return FailureClass::MalformedExecutionState;
    }
    if reason_codes.iter().any(|reason_code| {
        crate::contracts::plan::is_engineering_approval_fidelity_reason_code(reason_code)
    }) {
        return FailureClass::PlanNotExecutionReady;
    }
    if reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "current_task_closure_overlay_restore_required"
                | "prior_task_current_closure_missing"
                | "prior_task_current_closure_stale"
                | "prior_task_current_closure_invalid"
                | "prior_task_review_not_green"
                | "prior_task_verification_missing"
                | "prior_task_verification_missing_legacy"
                | "task_review_not_independent"
                | "task_review_artifact_malformed"
                | "task_verification_summary_malformed"
                | "task_cycle_break_active"
        )
    }) {
        return FailureClass::ExecutionStateNotReady;
    }
    FailureClass::InvalidStepTransition
}

pub(in crate::execution::commands) fn begin_failure_class_from_status(
    status: &PlanExecutionStatus,
) -> FailureClass {
    let reason_codes = begin_failure_reason_codes(status);
    begin_failure_class_from_blocking_reason_codes(&reason_codes)
}

pub(in crate::execution::commands) fn begin_failure_reason_codes(
    status: &PlanExecutionStatus,
) -> Vec<String> {
    let mut reason_codes = status.blocking_reason_codes.clone();
    for reason_code in &status.reason_codes {
        if !reason_codes.iter().any(|existing| existing == reason_code) {
            reason_codes.push(reason_code.clone());
        }
    }
    reason_codes
}

pub(in crate::execution::commands) fn normalize_or_seed_source(
    source: &str,
    execution_mode: &mut String,
) -> Result<(), JsonFailure> {
    match source {
        "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {}
        _ => {
            return Err(JsonFailure::new(
                FailureClass::InvalidExecutionMode,
                "Execution source must be one of the supported execution modes.",
            ));
        }
    }
    if execution_mode == "none" {
        *execution_mode = source.to_owned();
        return Ok(());
    }
    normalize_source(source, execution_mode)
}

pub(in crate::execution::commands) fn status_with_shared_routing_or_context(
    runtime: &ExecutionRuntime,
    plan: &Path,
    fallback_context: &ExecutionContext,
) -> Result<PlanExecutionStatus, JsonFailure> {
    status_with_shared_routing_or_context_with_external_review(
        runtime,
        plan,
        fallback_context,
        false,
    )
}

pub(in crate::execution::commands) fn status_with_shared_routing_or_context_with_external_review(
    runtime: &ExecutionRuntime,
    plan: &Path,
    fallback_context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let unsanitized_post_status = public_status_from_context_with_shared_routing(
        runtime,
        fallback_context,
        external_review_result_ready,
    )
    .ok();
    if let Some(status) = unsanitized_post_status.as_ref() {
        enforce_post_mutation_shared_status_invariants(status)?;
    }
    let args = StatusArgs {
        plan: plan.to_path_buf(),
        external_review_result_ready,
    };
    match runtime.status(&args) {
        Ok(status) => {
            if unsanitized_post_status.is_none() {
                enforce_post_mutation_shared_status_invariants(&status)?;
            }
            enforce_post_mutation_semantic_workspace_invariant(
                fallback_context,
                unsanitized_post_status.as_ref(),
                &status,
            )?;
            Ok(status)
        }
        Err(error) => {
            let legacy_pre_harness_failure = error.error_class
                == FailureClass::MalformedExecutionState.as_str()
                && error
                    .message
                    .contains("Legacy pre-harness execution evidence is no longer accepted");
            let malformed_preflight_seed_failure = error.error_class
                == FailureClass::MalformedExecutionState.as_str()
                && error
                    .message
                    .contains("Persisted execution preflight acceptance");
            let exact_command_derivation_failure = error.error_class
                == FailureClass::MalformedExecutionState.as_str()
                && error
                    .message
                    .contains("could not derive the exact execution command");
            if legacy_pre_harness_failure
                || (malformed_preflight_seed_failure
                    && load_authoritative_transition_state(fallback_context)?
                        .as_ref()
                        .and_then(|state| state.execution_run_id_opt())
                        .is_some())
                || exact_command_derivation_failure
            {
                let fallback_status = public_status_from_context_with_shared_routing(
                    runtime,
                    fallback_context,
                    external_review_result_ready,
                )?;
                enforce_post_mutation_status_invariants(
                    fallback_context,
                    &fallback_status,
                    unsanitized_post_status.as_ref(),
                )?;
                return Ok(fallback_status);
            }
            Err(error)
        }
    }
}

pub(in crate::execution::commands) fn enforce_post_mutation_status_invariants(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    baseline_status: Option<&PlanExecutionStatus>,
) -> Result<(), JsonFailure> {
    enforce_post_mutation_shared_status_invariants(status)?;
    enforce_post_mutation_semantic_workspace_invariant(context, baseline_status, status)
}

pub(in crate::execution::commands) fn enforce_post_mutation_shared_status_invariants(
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    let injected_status =
        if std::env::var("FEATUREFORGE_PLAN_EXECUTION_POST_MUTATION_INVARIANT_TEST_INJECTION")
            .is_ok()
        {
            let mut injected_status = status.clone();
            crate::execution::invariants::inject_post_mutation_invariant_test_violation(
                &mut injected_status,
            );
            Some(injected_status)
        } else {
            None
        };
    let status = injected_status.as_ref().unwrap_or(status);
    let violations =
        check_runtime_status_invariants(status, InvariantEnforcementMode::PostMutation);
    if !violations.is_empty() {
        let details = violations
            .iter()
            .map(|violation| format!("{}: {}", violation.code, violation.detail))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!("Post-mutation invariant violated: {details}"),
        ));
    }
    Ok(())
}

pub(in crate::execution::commands) fn enforce_post_mutation_semantic_workspace_invariant(
    context: &ExecutionContext,
    baseline_status: Option<&PlanExecutionStatus>,
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    if let Some(baseline_status) = baseline_status {
        let semantic_workspace_changed =
            baseline_status.semantic_workspace_tree_id != status.semantic_workspace_tree_id;
        if semantic_workspace_changed
            && semantic_changed_paths_between_statuses(context, baseline_status, status)?.is_empty()
        {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Post-mutation invariant violated: semantic workspace identity changed without semantic repo-content changes.",
            ));
        }
    }
    Ok(())
}

pub(in crate::execution::commands) fn semantic_changed_paths_between_statuses(
    context: &ExecutionContext,
    baseline_status: &PlanExecutionStatus,
    status: &PlanExecutionStatus,
) -> Result<Vec<String>, JsonFailure> {
    let Some(baseline_raw_tree) = raw_workspace_tree_sha(baseline_status) else {
        return Ok(Vec::new());
    };
    let Some(current_raw_tree) = raw_workspace_tree_sha(status) else {
        return Ok(Vec::new());
    };
    semantic_paths_changed_between_raw_trees(context, baseline_raw_tree, current_raw_tree)
}

pub(in crate::execution::commands) fn raw_workspace_tree_sha(
    status: &PlanExecutionStatus,
) -> Option<&str> {
    status
        .raw_workspace_tree_id
        .as_deref()
        .and_then(|value| value.strip_prefix("git_tree:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(in crate::execution::commands) fn require_close_current_task_public_mutation(
    status: &PlanExecutionStatus,
    task: u32,
) -> Result<(), JsonFailure> {
    require_public_mutation(
        status,
        PublicMutationRequest {
            kind: PublicMutationKind::CloseCurrentTask,
            task: Some(task),
            step: None,
            expect_execution_fingerprint: None,
            transfer_mode: None,
            transfer_scope: None,
            command_name: "close-current-task",
        },
        FailureClass::ExecutionStateNotReady,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::execution::commands) enum CloseCurrentTaskOutcomeClass {
    Positive,
    Negative,
    Invalid,
}

pub(in crate::execution::commands) fn close_current_task_outcome_class(
    review_result: ReviewOutcomeArg,
    verification_result: VerificationOutcomeArg,
) -> CloseCurrentTaskOutcomeClass {
    match (review_result, verification_result) {
        (ReviewOutcomeArg::Pass, VerificationOutcomeArg::Pass) => {
            CloseCurrentTaskOutcomeClass::Positive
        }
        (ReviewOutcomeArg::Pass, VerificationOutcomeArg::NotRun) => {
            CloseCurrentTaskOutcomeClass::Invalid
        }
        (ReviewOutcomeArg::Fail, _) | (ReviewOutcomeArg::Pass, VerificationOutcomeArg::Fail) => {
            CloseCurrentTaskOutcomeClass::Negative
        }
    }
}

pub(in crate::execution::commands) fn advance_late_stage_result_label(
    result: Option<AdvanceLateStageResultArg>,
) -> &'static str {
    result
        .map(AdvanceLateStageResultArg::as_str)
        .unwrap_or("unspecified")
}

pub(in crate::execution::commands) fn require_advance_late_stage_summary_file<'a>(
    args: &'a AdvanceLateStageArgs,
    stage_label: &str,
) -> Result<&'a Path, JsonFailure> {
    args.summary_file.as_deref().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!(
                "summary_file_required: {stage_label} advance-late-stage requires --summary-file."
            ),
        )
    })
}

pub(in crate::execution::commands) fn require_advance_late_stage_public_mutation(
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    require_public_mutation(
        status,
        PublicMutationRequest {
            kind: PublicMutationKind::AdvanceLateStage,
            task: None,
            step: None,
            expect_execution_fingerprint: None,
            transfer_mode: None,
            transfer_scope: None,
            command_name: "advance-late-stage",
        },
        FailureClass::ExecutionStateNotReady,
    )
}

pub(in crate::execution::commands) fn is_rebuild_precondition_failure(failure_class: &str) -> bool {
    matches!(
        failure_class,
        "artifact_read_error" | "state_transition_blocked" | "target_race"
    )
}

pub(in crate::execution::commands) fn execute_rebuild_candidate_projection_only(
    request: &crate::execution::state::RebuildEvidenceRequest,
    candidate: &RebuildEvidenceCandidate,
) -> RebuildEvidenceTarget {
    let attempt_id_before = candidate
        .attempt_number
        .map(|attempt| format!("{}:{}:{}", candidate.task, candidate.step, attempt));
    let mut target = RebuildEvidenceTarget {
        task_id: candidate.task,
        step_id: candidate.step,
        target_kind: candidate.target_kind.clone(),
        pre_invalidation_reason: candidate.pre_invalidation_reason.clone(),
        status: String::from("planned"),
        verify_mode: candidate.verify_mode.clone(),
        verify_command: candidate.verify_command.clone(),
        attempt_id_before,
        attempt_id_after: None,
        verification_hash: None,
        error: None,
        failure_class: None,
    };

    if candidate.target_kind == "artifact_read_error" {
        target.status = String::from("failed");
        target.failure_class = Some(String::from("artifact_read_error"));
        target.error = Some(candidate.pre_invalidation_reason.clone());
        return target;
    }
    let projection_only_message = String::from(
        "projection_only: projection rebuild reports stale projection candidates without mutating runtime projections; run materialize-projections for explicit projection materialization or replay stale execution with reopen/begin/complete when execution work must be rerun.",
    );
    if request.skip_manual_fallback {
        target.status = String::from("failed");
        target.failure_class = Some(String::from("manual_required"));
        target.error = Some(format!("manual_required: {projection_only_message}"));
        return target;
    }
    if target.failure_class.is_none() {
        target.status = String::from("manual_required");
        target.failure_class = Some(String::from("manual_required"));
        target.error = Some(projection_only_message);
    }
    target
}
