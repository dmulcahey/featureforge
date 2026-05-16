use super::{
    AuthoritativeTransitionStateRef, BTreeSet, EvidenceFormat, ExecutionContext, FailureClass,
    GateResult, GateState, JsonFailure, NoteState, PUBLIC_ADVANCE_LATE_STAGE_REMEDIATION,
    PlanStepState, REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED, active_step,
    authoritative_completed_steps_for_context,
    current_branch_gate_bindings_from_authoritative_state,
    enforce_review_authoritative_late_gate_truth, enforce_worktree_lease_binding_truth,
    latest_attempt_for_step, load_authoritative_transition_state,
    public_typed_operator_route_remediation, public_typed_operator_route_remediation_for_plan,
    public_workflow_operator_remediation_for_plan, require_current_browser_qa_pass_for_finish,
    require_current_final_review_pass_for_finish,
    require_current_release_readiness_ready_for_finish,
    shared_current_branch_closure_has_tracked_drift, step_completed_by_authoritative_truth,
    usable_current_branch_closure_identity_from_authoritative_state,
    validate_v2_evidence_provenance_for_completed_steps,
};
use crate::execution::gate_reason_codes::QA_REQUIREMENT_MISSING_OR_INVALID;

fn branch_freshness_typed_route_remediation() -> String {
    public_typed_operator_route_remediation(
        "Refresh branch-closure and final-review freshness before finish readiness continues.",
    )
}

fn qa_requirement_typed_route_remediation() -> String {
    public_typed_operator_route_remediation(
        "Correct approved-plan QA Requirement metadata before finish readiness continues.",
    )
}

fn worktree_changes_typed_route_remediation() -> String {
    public_typed_operator_route_remediation(
        "Commit or discard tracked worktree changes, then refresh review and finish-readiness prerequisites.",
    )
}

fn repo_state_inspection_typed_route_remediation() -> String {
    public_typed_operator_route_remediation(
        "Restore repository state inspection, then refresh review and finish-readiness prerequisites.",
    )
}

pub fn gate_review_from_context(context: &ExecutionContext) -> GateResult {
    let authoritative_state = load_authoritative_transition_state(context);
    gate_review_from_context_with_authoritative_state(
        context,
        authoritative_state.as_ref().map(|state| state.as_ref()),
        true,
    )
}

pub(crate) fn persist_finish_review_gate_pass_checkpoint_for_command_with_authoritative_state(
    context: &ExecutionContext,
    command_name: &'static str,
    authoritative_state: &mut Result<
        Option<crate::execution::transitions::AuthoritativeTransitionState>,
        JsonFailure,
    >,
) -> Result<(), JsonFailure> {
    let Some(authoritative_state) = authoritative_state
        .as_mut()
        .map_err(|error| error.clone())?
    else {
        return Ok(());
    };
    let Some(branch_closure_id) = usable_current_branch_closure_identity_from_authoritative_state(
        context,
        Some(authoritative_state),
    )
    .map(|identity| identity.branch_closure_id)
    .map(|value| value.trim().to_owned())
    .filter(|value| !value.is_empty()) else {
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
    authoritative_state: AuthoritativeTransitionStateRef<'_>,
) -> GateResult {
    let mut gate = GateState::default();
    let final_review_route_remediation = public_typed_operator_route_remediation_for_plan(
        "Return to workflow/operator JSON for the current approved-plan route before requesting final review.",
        &context.plan_rel,
    );
    let public_repair_remediation =
        public_workflow_operator_remediation_for_plan(&context.plan_rel);
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
            final_review_route_remediation.clone(),
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
            final_review_route_remediation.clone(),
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
            final_review_route_remediation.clone(),
        );
    }

    if let Some(step) = context.steps.iter().find(|step| {
        !step_completed_by_authoritative_truth(step, authoritative_completed_steps.as_ref())
    }) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "unfinished_steps_remaining",
            format!(
                "Final review is blocked while Task {} Step {} remains unchecked.",
                step.task_number, step.step_number
            ),
            final_review_route_remediation,
        );
    }

    for step in context.steps.iter().filter(|step| {
        step_completed_by_authoritative_truth(step, authoritative_completed_steps.as_ref())
    }) {
        verify_completed_step_evidence_projection(
            context,
            &mut gate,
            step,
            &public_repair_remediation,
        );
    }

    if enforce_authoritative_late_gate_truth {
        enforce_review_authoritative_late_gate_truth(context, &mut gate, authoritative_state);
    }
    enforce_worktree_lease_binding_truth(context, &mut gate);

    if context.evidence.format == EvidenceFormat::Legacy && !context.evidence.attempts.is_empty() {
        gate.warn("legacy_evidence_format");
    }
    if context.evidence.format == EvidenceFormat::V2 {
        validate_v2_evidence_provenance_for_completed_steps(
            context,
            &mut gate,
            authoritative_completed_steps.as_ref(),
        );
    }

    gate.finish()
}

pub(super) fn verify_completed_step_evidence_projection(
    context: &ExecutionContext,
    gate: &mut GateState,
    step: &PlanStepState,
    remediation: &str,
) {
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
            remediation.to_owned(),
        );
        return;
    };
    if attempt.status == "Completed" {
        return;
    }
    gate.fail(
        FailureClass::StaleExecutionEvidence,
        "checked_step_missing_evidence",
        format!(
            "Task {} Step {} no longer has a completed evidence attempt.",
            step.task_number, step.step_number
        ),
        remediation.to_owned(),
    );
}

fn authoritative_completed_steps_for_gate(
    context: &ExecutionContext,
    gate: &mut GateState,
) -> Option<BTreeSet<(u32, u32)>> {
    match authoritative_completed_steps_for_context(context) {
        Ok(Some(completed_steps)) => Some(completed_steps),
        Ok(None) => {
            if context.local_execution_progress_markers_present
                || !context.evidence.attempts.is_empty()
            {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "authoritative_completion_state_missing",
                    "Final review requires authoritative event-log completion state; projection-only plan/evidence state is not authoritative.",
                    public_workflow_operator_remediation_for_plan(&context.plan_rel),
                );
                return Some(BTreeSet::new());
            }
            None
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_completion_state_unavailable",
                format!(
                    "Final review could not load authoritative completion state: {}",
                    error.message
                ),
                public_workflow_operator_remediation_for_plan(&context.plan_rel),
            );
            Some(BTreeSet::new())
        }
    }
}

pub(super) fn gate_review_from_context_internal(
    context: &ExecutionContext,
    enforce_authoritative_late_gate_truth: bool,
) -> GateResult {
    let authoritative_state = load_authoritative_transition_state(context);
    gate_review_from_context_with_authoritative_state(
        context,
        authoritative_state.as_ref().map(|state| state.as_ref()),
        enforce_authoritative_late_gate_truth,
    )
}

pub(crate) fn gate_review_from_context_with_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: AuthoritativeTransitionStateRef<'_>,
    enforce_authoritative_late_gate_truth: bool,
) -> GateResult {
    let mut gate = GateState::from_result(gate_review_base_result(
        context,
        enforce_authoritative_late_gate_truth,
        authoritative_state,
    ));
    if !gate.allowed {
        return gate.finish();
    }
    if !evaluate_pre_checkpoint_finish_gate(context, &mut gate, authoritative_state) {
        return gate.finish();
    }
    gate.finish()
}

pub(super) fn evaluate_pre_checkpoint_finish_gate(
    context: &ExecutionContext,
    gate: &mut GateState,
    authoritative_state: AuthoritativeTransitionStateRef<'_>,
) -> bool {
    match context.repo_has_tracked_worktree_changes_excluding_execution_evidence() {
        Ok(true) => {
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                "review_artifact_worktree_dirty",
                "Finish readiness is blocked by tracked worktree changes that landed after the last review artifacts were generated.",
                worktree_changes_typed_route_remediation(),
            );
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
                "Tracked repo writes after final review invalidated review freshness for terminal branch completion.",
                worktree_changes_typed_route_remediation(),
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
                repo_state_inspection_typed_route_remediation(),
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
    let authoritative_state = match authoritative_state {
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
    let current_branch_bindings = current_branch_gate_bindings_from_authoritative_state(
        context,
        Some(authoritative_state),
        false,
    );
    let Some(current_branch_closure_id) =
        current_branch_bindings.current_branch_closure_id.as_deref()
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "current_branch_closure_id_missing",
            "Finish readiness requires a current branch-closure binding.",
            PUBLIC_ADVANCE_LATE_STAGE_REMEDIATION,
        );
        return false;
    };
    let Some(current_branch_reviewed_state_id) = current_branch_bindings
        .current_branch_reviewed_state_id
        .as_deref()
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "current_branch_reviewed_state_id_missing",
            "Finish readiness requires a current reviewed-branch-state binding.",
            PUBLIC_ADVANCE_LATE_STAGE_REMEDIATION,
        );
        return false;
    };
    match shared_current_branch_closure_has_tracked_drift(context, Some(authoritative_state)) {
        Ok(true) => {
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
                "Tracked repo writes after final review invalidated review freshness for terminal branch completion.",
                branch_freshness_typed_route_remediation(),
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
                repo_state_inspection_typed_route_remediation(),
            );
            return false;
        }
    }
    if !require_current_release_readiness_ready_for_finish(
        context,
        authoritative_state,
        current_branch_closure_id,
        current_branch_reviewed_state_id,
        &current_base_branch,
        gate,
    ) {
        return false;
    }
    if !require_current_final_review_pass_for_finish(
        context,
        authoritative_state,
        current_branch_closure_id,
        current_branch_reviewed_state_id,
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
                QA_REQUIREMENT_MISSING_OR_INVALID,
                "Finish readiness requires approved-plan QA Requirement metadata to be present and valid.",
                qa_requirement_typed_route_remediation(),
            );
            return false;
        }
    };
    if browser_qa_required
        && !require_current_browser_qa_pass_for_finish(
            context,
            authoritative_state,
            current_branch_closure_id,
            current_branch_reviewed_state_id,
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
