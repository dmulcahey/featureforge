use super::*;

pub fn gate_review_from_context(context: &ExecutionContext) -> GateResult {
    gate_review_from_context_internal(context, true)
}

pub(super) fn gate_result_current_branch_closure_id(
    context: &ExecutionContext,
    gate_allowed: bool,
) -> Option<String> {
    current_branch_closure_id(context).or_else(|| {
        gate_allowed.then(|| {
            usable_current_branch_closure_identity(context)
                .map(|identity| identity.branch_closure_id)
        })?
    })
}

pub(super) fn persist_finish_review_gate_pass_checkpoint(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    persist_finish_review_gate_pass_checkpoint_for_command(context, "gate_review")
}

pub(crate) fn persist_finish_review_gate_pass_checkpoint_for_command(
    context: &ExecutionContext,
    command_name: &'static str,
) -> Result<(), JsonFailure> {
    let Some(branch_closure_id) = usable_current_branch_closure_identity(context)
        .map(|identity| identity.branch_closure_id)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(());
    };
    if !authoritative_state
        .record_finish_review_gate_pass_checkpoint_if_current(&branch_closure_id)?
    {
        return Ok(());
    }
    authoritative_state.persist_if_dirty_with_failpoint_and_command(None, command_name)
}

pub(super) fn gate_review_base_result(
    context: &ExecutionContext,
    enforce_authoritative_late_gate_truth: bool,
) -> GateResult {
    let mut gate = GateState::default();
    let authoritative_completed_steps = authoritative_completed_steps_for_gate(context, &mut gate);
    if !gate.allowed {
        return gate.finish();
    }
    if let Some(step) = active_step(context, NoteState::Active) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "active_step_in_progress",
            format!(
                "Final review is blocked while Task {} Step {} remains active.",
                step.task_number, step.step_number
            ),
            "Complete, interrupt, or resolve the active step before review.",
        );
    }
    if let Some(step) = active_step(context, NoteState::Blocked) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "blocked_step",
            format!(
                "Final review is blocked while Task {} Step {} remains blocked.",
                step.task_number, step.step_number
            ),
            "Resolve the blocked step before review.",
        );
    }
    if let Some(step) = active_step(context, NoteState::Interrupted) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "interrupted_work_unresolved",
            format!(
                "Final review is blocked while Task {} Step {} remains interrupted.",
                step.task_number, step.step_number
            ),
            "Resume or explicitly resolve the interrupted work before review.",
        );
    }

    if let Some(step) = context
        .steps
        .iter()
        .find(|step| !gate_step_is_complete(step, authoritative_completed_steps.as_ref()))
    {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "unfinished_steps_remaining",
            format!(
                "Final review is blocked while Task {} Step {} remains unchecked.",
                step.task_number, step.step_number
            ),
            "Finish all approved plan steps before final review.",
        );
    }

    for step in context
        .steps
        .iter()
        .filter(|step| gate_step_is_complete(step, authoritative_completed_steps.as_ref()))
    {
        let Some(attempt) =
            latest_attempt_for_step(&context.evidence, step.task_number, step.step_number)
        else {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "checked_step_missing_evidence",
                format!(
                    "Task {} Step {} is checked but missing execution evidence.",
                    step.task_number, step.step_number
                ),
                "Reopen the step or record matching execution evidence.",
            );
            continue;
        };
        if attempt.status != "Completed" {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "checked_step_missing_evidence",
                format!(
                    "Task {} Step {} no longer has a completed evidence attempt.",
                    step.task_number, step.step_number
                ),
                "Reopen the step or complete it again with fresh evidence.",
            );
        }
    }

    if enforce_authoritative_late_gate_truth {
        enforce_review_authoritative_late_gate_truth(context, &mut gate);
    }
    enforce_worktree_lease_binding_truth(context, &mut gate);

    if context.evidence.format == EvidenceFormat::Legacy && !context.evidence.attempts.is_empty() {
        gate.warn("legacy_evidence_format");
    }
    if context.evidence.format == EvidenceFormat::V2 {
        validate_v2_evidence_provenance(context, &mut gate);
    }

    gate.finish()
}

fn gate_step_is_complete(
    step: &PlanStepState,
    authoritative_completed_steps: Option<&BTreeSet<(u32, u32)>>,
) -> bool {
    authoritative_completed_steps.map_or(step.checked, |completed_steps| {
        completed_steps.contains(&(step.task_number, step.step_number))
    })
}

fn authoritative_completed_steps_for_gate(
    context: &ExecutionContext,
    gate: &mut GateState,
) -> Option<BTreeSet<(u32, u32)>> {
    let authoritative_state = match load_authoritative_transition_state(context) {
        Ok(Some(authoritative_state)) => authoritative_state,
        Ok(None) => {
            if context.local_execution_progress_markers_present
                || !context.evidence.attempts.is_empty()
            {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "authoritative_completion_state_missing",
                    "Final review requires authoritative event-log completion state; projection-only plan/evidence state is not authoritative.",
                    "Restore or migrate authoritative event-log state before final review.",
                );
                return Some(BTreeSet::new());
            }
            return None;
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_completion_state_unavailable",
                format!(
                    "Final review could not load authoritative completion state: {}",
                    error.message
                ),
                "Restore authoritative event-log state before final review.",
            );
            return Some(BTreeSet::new());
        }
    };
    let mut completed_steps = BTreeSet::new();
    for task in authoritative_state.current_task_closure_results().keys() {
        completed_steps.extend(
            context
                .steps
                .iter()
                .filter(|step| step.task_number == *task)
                .map(|step| (step.task_number, step.step_number)),
        );
    }
    if let Some(event_completed_steps) = authoritative_state
        .state_payload_snapshot()
        .get("event_completed_steps")
        .and_then(serde_json::Value::as_object)
    {
        for entry in event_completed_steps.values() {
            if let (Some(task), Some(step)) = (
                entry
                    .get("task")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok()),
                entry
                    .get("step")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok()),
            ) {
                completed_steps.insert((task, step));
            }
        }
    }
    Some(completed_steps)
}

pub(super) fn gate_review_from_context_internal(
    context: &ExecutionContext,
    enforce_authoritative_late_gate_truth: bool,
) -> GateResult {
    let mut gate = GateState::from_result(gate_review_base_result(
        context,
        enforce_authoritative_late_gate_truth,
    ));
    if !gate.allowed {
        return gate.finish();
    }
    if !evaluate_pre_checkpoint_finish_gate(context, &mut gate) {
        return gate.finish();
    }
    gate.finish()
}

pub(super) fn evaluate_pre_checkpoint_finish_gate(
    context: &ExecutionContext,
    gate: &mut GateState,
) -> bool {
    match context.repo_has_tracked_worktree_changes_excluding_execution_evidence() {
        Ok(true) => {
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                "review_artifact_worktree_dirty",
                "Finish readiness is blocked by tracked worktree changes that landed after the last review artifacts were generated.",
                "Commit or discard tracked worktree changes, then rerun requesting-code-review and downstream finish artifacts.",
            );
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
                "Tracked repo writes after final review invalidated review freshness for terminal branch completion.",
                "Commit or discard tracked worktree changes, then rerun requesting-code-review and downstream finish artifacts.",
            );
            return false;
        }
        Ok(false) => {}
        Err(error) => {
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                "review_artifact_worktree_state_unavailable",
                format!(
                    "Finish readiness could not determine whether tracked worktree changes are present: {}",
                    error.message
                ),
                "Restore repository status inspection, then rerun requesting-code-review and downstream finish artifacts.",
            );
            return false;
        }
    }
    let Some(current_base_branch) = context.current_release_base_branch() else {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_artifact_base_branch_unresolved",
            "Finish readiness could not determine the expected base branch for the current workspace.",
            PUBLIC_ADVANCE_LATE_STAGE_REMEDIATION,
        );
        return false;
    };
    let authoritative_state = match load_authoritative_transition_state(context) {
        Ok(Some(state)) => state,
        Ok(None) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_transition_state_missing",
                "Finish readiness requires authoritative transition state.",
                PUBLIC_ADVANCE_LATE_STAGE_REMEDIATION,
            );
            return false;
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_transition_state_unavailable",
                format!(
                    "Finish readiness could not read authoritative transition state: {}",
                    error.message
                ),
                PUBLIC_ADVANCE_LATE_STAGE_REMEDIATION,
            );
            return false;
        }
    };
    let Some(current_branch_closure_id) = current_branch_closure_id(context) else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "current_branch_closure_id_missing",
            "Finish readiness requires a current branch-closure binding.",
            PUBLIC_ADVANCE_LATE_STAGE_REMEDIATION,
        );
        return false;
    };
    let current_branch_reviewed_state_id = current_branch_reviewed_state_id(context);
    let Some(current_branch_reviewed_state_id) = current_branch_reviewed_state_id else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "current_branch_reviewed_state_id_missing",
            "Finish readiness requires a current reviewed-branch-state binding.",
            PUBLIC_ADVANCE_LATE_STAGE_REMEDIATION,
        );
        return false;
    };
    match shared_current_branch_closure_has_tracked_drift(context, Some(&authoritative_state)) {
        Ok(true) => {
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
                "Tracked repo writes after final review invalidated review freshness for terminal branch completion.",
                "Record a fresh branch closure and rerun requesting-code-review and downstream finish artifacts.",
            );
            return false;
        }
        Ok(false) => {}
        Err(error) => {
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                "review_artifact_workspace_state_unavailable",
                format!(
                    "Finish readiness could not compare current workspace state with the reviewed branch closure: {}",
                    error.message
                ),
                "Restore repository state inspection, then rerun requesting-code-review and downstream finish artifacts.",
            );
            return false;
        }
    }
    if !require_current_release_readiness_ready_for_finish(
        context,
        &authoritative_state,
        &current_branch_closure_id,
        &current_branch_reviewed_state_id,
        &current_base_branch,
        gate,
    ) {
        return false;
    }
    if !require_current_final_review_pass_for_finish(
        context,
        &authoritative_state,
        &current_branch_closure_id,
        &current_branch_reviewed_state_id,
        &current_base_branch,
        gate,
    ) {
        return false;
    }

    let browser_qa_required = match context.plan_document.qa_requirement.as_deref() {
        Some("required") => true,
        Some("not-required") => false,
        _ => {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                "qa_requirement_missing_or_invalid",
                "Finish readiness requires approved-plan QA Requirement metadata to be present and valid.",
                "Record a workflow pivot so the approved plan can be corrected, then rerun the late-stage flow.",
            );
            return false;
        }
    };
    if browser_qa_required
        && !require_current_browser_qa_pass_for_finish(
            context,
            &authoritative_state,
            &current_branch_closure_id,
            &current_branch_reviewed_state_id,
            &current_base_branch,
            gate,
        )
    {
        return false;
    }

    true
}

// Barrier reconcile and receipt release:
//   open / review_passed_pending_reconcile
//                    |
//                    v
//       reconcile reviewed checkpoint commit
//                    |
//                    v
//          cleanup_state == cleaned
//                    |
//                    v
//      dependent work may be released at finish
