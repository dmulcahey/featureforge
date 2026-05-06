use super::*;

pub(in crate::execution::commands) struct BranchReviewedState {
    pub(in crate::execution::commands) base_branch: String,
    pub(in crate::execution::commands) contract_identity: String,
    pub(in crate::execution::commands) effective_reviewed_branch_surface: String,
    pub(in crate::execution::commands) provenance_basis: String,
    pub(in crate::execution::commands) reviewed_state_id: String,
    pub(in crate::execution::commands) semantic_reviewed_state_id: String,
    pub(in crate::execution::commands) source_task_closure_ids: Vec<String>,
}

pub(in crate::execution::commands) struct TaskClosureLineageRefresh {
    pub(in crate::execution::commands) task: u32,
    pub(in crate::execution::commands) claim_write_authority: bool,
}

pub(in crate::execution::commands) struct CurrentTaskClosureMaterialization<'a> {
    pub(in crate::execution::commands) task: u32,
    pub(in crate::execution::commands) dispatch_id: &'a str,
    pub(in crate::execution::commands) closure_record_id: &'a str,
    pub(in crate::execution::commands) execution_run_id: &'a str,
    pub(in crate::execution::commands) reviewed_state_id: &'a str,
    pub(in crate::execution::commands) semantic_reviewed_state_id: &'a str,
    pub(in crate::execution::commands) contract_identity: &'a str,
    pub(in crate::execution::commands) effective_reviewed_surface_paths: &'a [String],
    pub(in crate::execution::commands) review_result: &'a str,
    pub(in crate::execution::commands) review_summary_hash: &'a str,
    pub(in crate::execution::commands) verification_result: &'a str,
    pub(in crate::execution::commands) verification_summary_hash: &'a str,
    pub(in crate::execution::commands) superseded_tasks: &'a [u32],
    pub(in crate::execution::commands) superseded_task_closure_ids: &'a [String],
}

pub(in crate::execution::commands) fn current_branch_reviewed_state(
    context: &ExecutionContext,
) -> Result<BranchReviewedState, JsonFailure> {
    let source_task_closure_ids = current_branch_source_task_closure_ids(context)?;
    let base_branch = context.current_release_base_branch().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            "advance-late-stage branch-closure recording requires an authoritative base-branch binding.",
        )
    })?;
    let reviewed_state_id = format!("git_tree:{}", context.current_tracked_tree_sha()?);
    let semantic_reviewed_state_id =
        semantic_workspace_snapshot(context)?.semantic_workspace_tree_id;
    Ok(BranchReviewedState {
        base_branch: base_branch.clone(),
        contract_identity: branch_definition_identity_for_context(context),
        effective_reviewed_branch_surface: String::from("repo_tracked_content"),
        provenance_basis: String::from("task_closure_lineage"),
        reviewed_state_id,
        semantic_reviewed_state_id,
        source_task_closure_ids,
    })
}

pub(in crate::execution::commands) fn deterministic_branch_closure_record_id(
    context: &ExecutionContext,
    reviewed_state: &BranchReviewedState,
) -> String {
    let source_task_closure_ids = reviewed_state.source_task_closure_ids.join("\n");
    deterministic_record_id(
        "branch-closure",
        &[
            &context.plan_rel,
            &context.runtime.branch_name,
            &reviewed_state.base_branch,
            &reviewed_state.semantic_reviewed_state_id,
            &reviewed_state.contract_identity,
            &reviewed_state.provenance_basis,
            &reviewed_state.effective_reviewed_branch_surface,
            source_task_closure_ids.as_str(),
        ],
    )
}

pub(in crate::execution::commands) fn branch_closure_record_matches_reviewed_state(
    record: &BranchClosureRecord,
    context: &ExecutionContext,
    reviewed_state: &BranchReviewedState,
) -> Result<bool, JsonFailure> {
    let semantic_matches =
        branch_closure_record_semantically_matches_reviewed_state(record, context, reviewed_state)?;
    Ok(record.source_plan_path == context.plan_rel
        && record.source_plan_revision == context.plan_document.plan_revision
        && record.repo_slug == context.runtime.repo_slug
        && record.branch_name == context.runtime.branch_name
        && record.base_branch == reviewed_state.base_branch
        && semantic_matches
        && record.contract_identity == reviewed_state.contract_identity
        && record.source_task_closure_ids == reviewed_state.source_task_closure_ids
        && record.provenance_basis == reviewed_state.provenance_basis
        && record._effective_reviewed_branch_surface
            == reviewed_state.effective_reviewed_branch_surface)
}

pub(in crate::execution::commands) fn branch_closure_record_matches_empty_lineage_late_stage_exemption(
    record: &BranchClosureRecord,
    context: &ExecutionContext,
    reviewed_state: &BranchReviewedState,
) -> Result<bool, JsonFailure> {
    let semantic_matches =
        branch_closure_record_semantically_matches_reviewed_state(record, context, reviewed_state)?;
    Ok(record.source_plan_path == context.plan_rel
        && record.source_plan_revision == context.plan_document.plan_revision
        && record.repo_slug == context.runtime.repo_slug
        && record.branch_name == context.runtime.branch_name
        && record.base_branch == reviewed_state.base_branch
        && semantic_matches
        && record.contract_identity == reviewed_state.contract_identity
        && record.source_task_closure_ids.is_empty()
        && record.provenance_basis == "task_closure_lineage_plus_late_stage_surface_exemption"
        && branch_closure_record_matches_plan_exemption(context, record))
}

pub(in crate::execution::commands) fn branch_closure_record_is_empty_lineage_late_stage_exemption_baseline(
    record: &BranchClosureRecord,
    context: &ExecutionContext,
) -> bool {
    record.source_plan_path == context.plan_rel
        && record.source_plan_revision == context.plan_document.plan_revision
        && record.repo_slug == context.runtime.repo_slug
        && record.branch_name == context.runtime.branch_name
        && record.source_task_closure_ids.is_empty()
        && record.provenance_basis == "task_closure_lineage_plus_late_stage_surface_exemption"
        && branch_closure_record_matches_plan_exemption(context, record)
}

pub(in crate::execution::commands) fn branch_closure_record_semantically_matches_reviewed_state(
    record: &BranchClosureRecord,
    context: &ExecutionContext,
    reviewed_state: &BranchReviewedState,
) -> Result<bool, JsonFailure> {
    if record
        .semantic_reviewed_state_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|recorded| recorded == reviewed_state.semantic_reviewed_state_id)
    {
        return Ok(true);
    }
    let Some(recorded_raw_tree) = record
        .reviewed_state_id
        .strip_prefix("git_tree:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(false);
    };
    let Some(current_raw_tree) = reviewed_state
        .reviewed_state_id
        .strip_prefix("git_tree:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(false);
    };
    semantic_paths_changed_between_raw_trees(context, recorded_raw_tree, current_raw_tree)
        .map(|changed_paths| changed_paths.is_empty())
}

pub(in crate::execution::commands) fn blocked_branch_closure_output_for_invalid_current_task_closure(
    context: &ExecutionContext,
) -> Result<Option<RecordBranchClosureOutput>, JsonFailure> {
    if let Some(failure) = structural_current_task_closure_failures(context)?
        .into_iter()
        .next()
    {
        let recovery = public_recovery_contract_for_follow_up(
            Path::new(&context.plan_rel),
            None,
            Some(String::from("repair_review_state")),
            PublicFollowUpInputProfile::None,
        );
        return Ok(Some(RecordBranchClosureOutput {
            action: String::from("blocked"),
            branch_closure_id: None,
            code: None,
            recommended_command: recovery.recommended_command,
            recommended_public_command_argv: recovery.recommended_public_command_argv,
            required_inputs: recovery.required_inputs,
            rederive_via_workflow_operator: recovery.rederive_via_workflow_operator,
            superseded_branch_closure_ids: Vec::new(),
            required_follow_up: recovery.required_follow_up,
            trace_summary: format!(
                "advance-late-stage branch-closure recording failed closed because {}",
                failure.message
            ),
        }));
    }
    Ok(None)
}

pub(crate) fn task_closure_contributes_to_branch_surface(
    context: &ExecutionContext,
    current_record: &CurrentTaskClosureRecord,
) -> bool {
    shared_task_closure_contributes_to_branch_surface(context, current_record)
}

#[cfg(test)]
pub(in crate::execution::commands) fn task_closure_record_covers_path(
    current_record: &CurrentTaskClosureRecord,
    path: &str,
) -> bool {
    current_record
        .effective_reviewed_surface_paths
        .iter()
        .any(|surface_path| {
            path_matches_late_stage_surface(path, std::slice::from_ref(surface_path))
        })
}

pub(in crate::execution::commands) fn current_authoritative_branch_closure_id(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    current_authoritative_branch_closure_id_optional(context)
}

pub(in crate::execution::commands) fn branch_closure_already_current_output(
    context: &ExecutionContext,
    authoritative_state: &mut AuthoritativeTransitionState,
    reviewed_state: &BranchReviewedState,
) -> Result<Option<RecordBranchClosureOutput>, JsonFailure> {
    let Some(current_identity) = usable_current_branch_closure_identity(context) else {
        return Ok(None);
    };
    let current_record_matches = authoritative_state
        .branch_closure_record(&current_identity.branch_closure_id)
        .map(|record| {
            Ok::<bool, JsonFailure>(
                branch_closure_record_matches_reviewed_state(&record, context, reviewed_state)?
                    || branch_closure_record_matches_empty_lineage_late_stage_exemption(
                        &record,
                        context,
                        reviewed_state,
                    )?,
            )
        })
        .transpose()?
        .unwrap_or(false);
    if !current_record_matches {
        return Ok(None);
    }
    authoritative_state.restore_current_branch_closure_overlay_fields(
        &current_identity.branch_closure_id,
        &reviewed_state.reviewed_state_id,
        &reviewed_state.contract_identity,
    )?;
    authoritative_state.set_review_state_repair_follow_up(None)?;
    authoritative_state
        .persist_if_dirty_with_failpoint_and_command(None, "record_branch_closure")?;
    Ok(Some(RecordBranchClosureOutput {
        action: String::from("already_current"),
        branch_closure_id: Some(current_identity.branch_closure_id),
        code: None,
        recommended_command: None,
        recommended_public_command_argv: None,
        required_inputs: Vec::new(),
        rederive_via_workflow_operator: None,
        superseded_branch_closure_ids: Vec::new(),
        required_follow_up: None,
        trace_summary: String::from(
            "Current reviewed branch state already has an authoritative current branch closure.",
        ),
    }))
}

pub(in crate::execution::commands) fn branch_closure_already_current_empty_lineage_exemption_output(
    context: &ExecutionContext,
    authoritative_state: &mut AuthoritativeTransitionState,
    reviewed_state: &BranchReviewedState,
) -> Result<Option<RecordBranchClosureOutput>, JsonFailure> {
    let Some(current_identity) = usable_current_branch_closure_identity(context) else {
        return Ok(None);
    };
    let current_record_matches = authoritative_state
        .branch_closure_record(&current_identity.branch_closure_id)
        .map(|record| {
            branch_closure_record_matches_empty_lineage_late_stage_exemption(
                &record,
                context,
                reviewed_state,
            )
        })
        .transpose()?
        .unwrap_or(false);
    if !current_record_matches {
        return Ok(None);
    }
    authoritative_state.restore_current_branch_closure_overlay_fields(
        &current_identity.branch_closure_id,
        &reviewed_state.reviewed_state_id,
        &reviewed_state.contract_identity,
    )?;
    authoritative_state.set_review_state_repair_follow_up(None)?;
    authoritative_state
        .persist_if_dirty_with_failpoint_and_command(None, "record_branch_closure")?;
    Ok(Some(RecordBranchClosureOutput {
        action: String::from("already_current"),
        branch_closure_id: Some(current_identity.branch_closure_id),
        code: None,
        recommended_command: None,
        recommended_public_command_argv: None,
        required_inputs: Vec::new(),
        rederive_via_workflow_operator: None,
        superseded_branch_closure_ids: Vec::new(),
        required_follow_up: None,
        trace_summary: String::from(
            "Current reviewed branch state already has an authoritative current branch closure.",
        ),
    }))
}

#[derive(Debug, Clone)]
pub(in crate::execution::commands) struct SupersededTaskClosureRecord {
    pub(in crate::execution::commands) task: u32,
    pub(in crate::execution::commands) closure_record_id: String,
}

pub(in crate::execution::commands) fn current_branch_source_task_closure_ids(
    context: &ExecutionContext,
) -> Result<Vec<String>, JsonFailure> {
    Ok(shared_branch_source_task_closure_ids(
        context,
        &current_branch_task_closure_records(context)?,
        None,
    ))
}

pub(in crate::execution::commands) fn current_branch_task_closure_records(
    context: &ExecutionContext,
) -> Result<Vec<CurrentTaskClosureRecord>, JsonFailure> {
    if load_authoritative_transition_state(context)?.is_none() {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "advance-late-stage branch-closure recording requires authoritative current task-closure state.",
        ));
    }
    Ok(still_current_task_closure_records(context)?
        .into_iter()
        .filter(|record| task_closure_contributes_to_branch_surface(context, record))
        .collect())
}

pub(in crate::execution::commands) fn current_task_closure_record_id(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<String, JsonFailure> {
    let current_lineage = task_completion_lineage_fingerprint(context, task_number).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "advance-late-stage branch-closure recording could not determine still-current task-closure lineage for task {task_number}."
            ),
        )
    })?;
    Ok(deterministic_record_id(
        "task-closure",
        &[
            &context.plan_rel,
            &task_number.to_string(),
            &current_lineage,
        ],
    ))
}

pub(in crate::execution::commands) fn current_task_reviewed_state_id(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<String, JsonFailure> {
    if task_completion_lineage_fingerprint(context, task_number).is_none() {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "close-current-task could not determine the still-current reviewed state for task {task_number}."
            ),
        ));
    }
    Ok(semantic_workspace_snapshot(context)?.semantic_workspace_tree_id)
}

pub(in crate::execution::commands) fn current_task_raw_reviewed_state_id(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<String, JsonFailure> {
    if task_completion_lineage_fingerprint(context, task_number).is_none() {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "close-current-task could not determine the still-current raw reviewed state for task {task_number}."
            ),
        ));
    }
    Ok(format!("git_tree:{}", context.current_tracked_tree_sha()?))
}

pub(in crate::execution::commands) fn current_task_contract_identity(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<String, JsonFailure> {
    task_definition_identity_for_task(context, task_number)?.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "close-current-task could not determine semantic task contract identity for task {task_number}."
            ),
        )
    })
}

pub(in crate::execution::commands) fn current_task_effective_reviewed_surface_paths(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<Vec<String>, JsonFailure> {
    let mut surface_paths = context
        .tasks_by_number
        .get(&task_number)
        .map(|task| {
            task.files
                .iter()
                .map(|entry| entry.path.clone())
                .filter(|path| {
                    path != NO_REPO_FILES_MARKER
                        && !is_runtime_owned_execution_control_plane_path(context, path)
                })
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    for step in context
        .steps
        .iter()
        .filter(|step_state| step_state.task_number == task_number)
    {
        let attempt = latest_attempt_for_step(&context.evidence, task_number, step.step_number).ok_or_else(
            || {
                JsonFailure::new(
                    FailureClass::ExecutionStateNotReady,
                    format!(
                        "close-current-task could not resolve completed evidence for task {task_number} step {}.",
                        step.step_number
                    ),
                )
            },
        )?;
        if attempt.status != "Completed" {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                format!(
                    "close-current-task requires completed evidence for task {task_number} step {}.",
                    step.step_number
                ),
            ));
        }
        for path in attempt
            .files
            .iter()
            .chain(attempt.file_proofs.iter().map(|proof| &proof.path))
        {
            if path != NO_REPO_FILES_MARKER
                && !is_runtime_owned_execution_control_plane_path(context, path)
            {
                surface_paths.insert(path.clone());
            }
        }
    }
    if surface_paths.is_empty() {
        surface_paths.insert(String::from(NO_REPO_FILES_MARKER));
    }
    Ok(surface_paths.into_iter().collect())
}

pub(in crate::execution::commands) fn task_surface_paths_overlap(
    left: &[String],
    right: &[String],
) -> bool {
    let left_paths = normalized_effective_task_surface_paths(left);
    let right_paths = normalized_effective_task_surface_paths(right);
    !left_paths.is_disjoint(&right_paths)
}

pub(in crate::execution::commands) fn normalized_effective_task_surface_paths(
    paths: &[String],
) -> BTreeSet<String> {
    paths
        .iter()
        .filter(|path| path.as_str() != NO_REPO_FILES_MARKER)
        .filter_map(|path| normalize_repo_relative_path(path).ok())
        .collect::<BTreeSet<_>>()
}

pub(in crate::execution::commands) fn task_closure_record_matches_active_plan_and_runtime_scope(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    record: &CurrentTaskClosureRecord,
) -> bool {
    let plan_matches = record.source_plan_path.as_deref() == Some(context.plan_rel.as_str())
        && record.source_plan_revision == Some(context.plan_document.plan_revision);
    if !plan_matches {
        return false;
    }
    match authoritative_state.execution_run_id_opt() {
        Some(active_run_id) => record.execution_run_id.as_deref() == Some(active_run_id.as_str()),
        None => record
            .execution_run_id
            .as_deref()
            .is_none_or(|run_id| run_id.trim().is_empty()),
    }
}

pub(in crate::execution::commands) fn superseded_task_closure_records(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    task_number: u32,
    closure_record_id: &str,
    effective_reviewed_surface_paths: &[String],
) -> Vec<SupersededTaskClosureRecord> {
    authoritative_state
        .task_closure_history_records()
        .into_iter()
        .filter(|record| record.closure_record_id != closure_record_id)
        .filter(|record| {
            task_closure_record_matches_active_plan_and_runtime_scope(
                context,
                authoritative_state,
                record,
            )
        })
        .filter(|record| {
            record.task == task_number
                || task_surface_paths_overlap(
                    &record.effective_reviewed_surface_paths,
                    effective_reviewed_surface_paths,
                )
        })
        .map(|record| SupersededTaskClosureRecord {
            task: record.task,
            closure_record_id: record.closure_record_id,
        })
        .collect()
}

pub(in crate::execution::commands) fn current_final_review_record_is_still_authoritative(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    check: CurrentFinalReviewAuthorityCheck<'_>,
) -> Result<bool, JsonFailure> {
    let Some(record) = authoritative_state.current_final_review_record() else {
        return Ok(false);
    };
    if record.branch_closure_id != check.branch_closure_id
        || record.dispatch_id != check.dispatch_id
        || record.reviewer_source != check.reviewer_source
        || record.reviewer_id != check.reviewer_id
        || record.result != check.result
        || record.summary_hash != check.normalized_summary_hash
        || record.source_plan_path != context.plan_rel
        || record.source_plan_revision != context.plan_document.plan_revision
        || record.repo_slug != context.runtime.repo_slug
        || record.branch_name != context.runtime.branch_name
    {
        return Ok(false);
    }
    let Some(current_base_branch) = context.current_release_base_branch() else {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Current final-review authority could not resolve the expected base branch.",
        ));
    };
    if record.base_branch != current_base_branch {
        return Ok(false);
    }
    if check.result == "fail" {
        return Ok(true);
    }
    if !final_review_dispatch_lineage_is_current_for_rerun(
        context,
        authoritative_state,
        check.branch_closure_id,
        check.dispatch_id,
    )? {
        return Ok(false);
    }
    Ok(true)
}

pub(in crate::execution::commands) fn final_review_dispatch_lineage_is_current_for_rerun(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    expected_branch_closure_id: &str,
    expected_dispatch_id: &str,
) -> Result<bool, JsonFailure> {
    let runtime_state = crate::execution::reducer::reduce_runtime_state(
        context,
        Some(authoritative_state),
        semantic_workspace_snapshot(context)?,
    )?;
    if shared_final_review_dispatch_still_current(
        runtime_state.gate_snapshot.gate_review.as_ref(),
        runtime_state.gate_snapshot.gate_finish.as_ref(),
    ) {
        return match ensure_final_review_dispatch_id_matches(context, expected_dispatch_id) {
            Ok(_) => Ok(true),
            Err(error)
                if matches!(
                    error.error_class.as_str(),
                    "ExecutionStateNotReady" | "InvalidCommandInput"
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        };
    }
    Ok(
        authoritative_state.current_final_review_dispatch_id() == Some(expected_dispatch_id)
            && authoritative_state.current_final_review_branch_closure_id()
                == Some(expected_branch_closure_id),
    )
}

pub(in crate::execution::commands) fn resolve_final_review_evidence(
    context: &ExecutionContext,
) -> Result<ResolvedFinalReviewEvidence, JsonFailure> {
    let base_branch = context.current_release_base_branch().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ReviewArtifactNotFresh,
            "final-review recording requires a resolvable base branch.",
        )
    })?;
    let execution_context_key = format!("{}@{}", context.runtime.branch_name, base_branch);
    let deviations_required = authoritative_matching_execution_topology_downgrade_records_checked(
        context,
        &execution_context_key,
    )?
    .iter()
    .any(|record| !record.rerun_guidance_superseded);
    Ok(ResolvedFinalReviewEvidence {
        base_branch,
        deviations_required,
    })
}
