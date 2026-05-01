use super::*;

#[cfg(test)]
pub(in crate::execution::commands) fn rebuild_downstream_truth_stale(
    message: impl Into<String>,
) -> JsonFailure {
    JsonFailure::new(FailureClass::StaleProvenance, message.into())
}

#[cfg(test)]
pub(in crate::execution::commands) fn rewrite_branch_final_review_artifacts(
    review_path: &Path,
    reviewer_artifact_path: &Path,
    current_head: &str,
    strategy_checkpoint_fingerprint: &str,
) -> Result<(), JsonFailure> {
    let _ = (
        review_path,
        reviewer_artifact_path,
        current_head,
        strategy_checkpoint_fingerprint,
    );
    Err(rebuild_downstream_truth_stale(
        "append_only_repair_required: rebuild-evidence may not rewrite historical final-review proof in place",
    ))
}

#[cfg(test)]
pub(in crate::execution::commands) fn rewrite_branch_head_bound_artifact(
    path: &Path,
    current_head: &str,
) -> Result<(), JsonFailure> {
    let _ = (path, current_head);
    Err(rebuild_downstream_truth_stale(
        "append_only_repair_required: rebuild-evidence may not rewrite historical head-bound artifacts in place",
    ))
}

#[cfg(test)]
pub(in crate::execution::commands) fn rewrite_branch_qa_artifact(
    qa_path: &Path,
    current_head: &str,
    test_plan_path: &Path,
) -> Result<(), JsonFailure> {
    let _ = (qa_path, current_head, test_plan_path);
    Err(rebuild_downstream_truth_stale(
        "append_only_repair_required: rebuild-evidence may not rewrite historical QA artifacts in place",
    ))
}

pub(in crate::execution::commands) fn rewrite_rebuild_source_test_plan_header(
    source: &str,
    test_plan_path: &Path,
) -> String {
    rewrite_markdown_header(
        source,
        "Source Test Plan",
        &format!("`{}`", test_plan_path.display()),
    )
}

pub(in crate::execution::commands) fn rewrite_markdown_header(
    source: &str,
    header: &str,
    value: &str,
) -> String {
    let prefix = format!("**{header}:**");
    let rewritten = source
        .lines()
        .map(|line| {
            if line.trim().starts_with(&prefix) {
                format!("**{header}:** {value}")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{rewritten}\n")
}

pub(in crate::execution::commands) fn refresh_task_closure_authoritative_lineage_with_context(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    refresh: TaskClosureLineageRefresh,
) -> Result<(), JsonFailure> {
    let _write_authority = refresh
        .claim_write_authority
        .then(|| claim_step_write_authority(runtime))
        .transpose()?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        authoritative_state.refresh_task_review_dispatch_lineage(context, refresh.task)?;
        authoritative_state
            .persist_if_dirty_with_failpoint_and_command(None, "close_current_task")?;
    }
    Ok(())
}

pub(in crate::execution::commands) fn materialize_current_task_closure_from_close_inputs(
    context: &ExecutionContext,
    authoritative_state: &mut AuthoritativeTransitionState,
    materialization: CurrentTaskClosureMaterialization<'_>,
) -> Result<(), JsonFailure> {
    record_current_task_closure(
        context,
        authoritative_state,
        CurrentTaskClosureWrite {
            task: materialization.task,
            dispatch_id: materialization.dispatch_id,
            closure_record_id: materialization.closure_record_id,
            execution_run_id: Some(materialization.execution_run_id),
            reviewed_state_id: materialization.reviewed_state_id,
            semantic_reviewed_state_id: Some(materialization.semantic_reviewed_state_id),
            contract_identity: materialization.contract_identity,
            effective_reviewed_surface_paths: materialization.effective_reviewed_surface_paths,
            review_result: materialization.review_result,
            review_summary_hash: materialization.review_summary_hash,
            verification_result: materialization.verification_result,
            verification_summary_hash: materialization.verification_summary_hash,
            superseded_tasks: materialization.superseded_tasks,
            superseded_task_closure_ids: materialization.superseded_task_closure_ids,
        },
    )
}

pub(in crate::execution::commands) fn latest_attempt_for_step(
    evidence: &ExecutionEvidence,
    task: u32,
    step: u32,
) -> Option<&EvidenceAttempt> {
    evidence
        .attempts
        .iter()
        .filter(|attempt| attempt.task_number == task && attempt.step_number == step)
        .max_by_key(|attempt| attempt.attempt_number)
}

#[cfg(test)]
pub(in crate::execution::commands) fn verify_command_launcher(
    verify_command: &str,
) -> (&'static str, Vec<String>) {
    if cfg!(windows) {
        ("cmd", vec![String::from("/C"), verify_command.to_owned()])
    } else {
        ("sh", vec![String::from("-lc"), verify_command.to_owned()])
    }
}

pub(in crate::execution::commands) fn reconcile_result_proof_fingerprint_for_review(
    repo_root: &Path,
    reconcile_result_commit_sha: &str,
) -> Option<String> {
    commit_object_fingerprint(repo_root, reconcile_result_commit_sha)
}

pub(in crate::execution::commands) fn planned_rebuild_target(
    candidate: &RebuildEvidenceCandidate,
) -> RebuildEvidenceTarget {
    RebuildEvidenceTarget {
        task_id: candidate.task,
        step_id: candidate.step,
        target_kind: candidate.target_kind.clone(),
        pre_invalidation_reason: candidate.pre_invalidation_reason.clone(),
        status: String::from("planned"),
        verify_mode: candidate.verify_mode.clone(),
        verify_command: candidate.verify_command.clone(),
        attempt_id_before: candidate
            .attempt_number
            .map(|attempt| format!("{}:{}:{}", candidate.task, candidate.step, attempt)),
        attempt_id_after: None,
        verification_hash: None,
        error: None,
        failure_class: None,
    }
}

pub(in crate::execution::commands) fn rebuild_scope_label(
    request: &crate::execution::state::RebuildEvidenceRequest,
) -> String {
    if !request.raw_steps.is_empty() {
        String::from("step")
    } else if !request.tasks.is_empty() {
        String::from("task")
    } else {
        String::from("all")
    }
}

pub(in crate::execution::commands) fn matched_rebuild_scope_ids(
    context: &ExecutionContext,
    request: &crate::execution::state::RebuildEvidenceRequest,
) -> Vec<String> {
    let task_filter = request.tasks.iter().copied().collect::<BTreeSet<_>>();
    let step_filter = request.steps.iter().copied().collect::<BTreeSet<_>>();
    context
        .steps
        .iter()
        .filter(|step| {
            (task_filter.is_empty() || task_filter.contains(&step.task_number))
                && (step_filter.is_empty()
                    || step_filter.contains(&(step.task_number, step.step_number)))
        })
        .map(|step| format!("{}:{}", step.task_number, step.step_number))
        .collect()
}
