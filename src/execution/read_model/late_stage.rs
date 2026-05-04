use super::*;

pub(crate) fn has_authoritative_late_stage_progress(overlay: &StatusAuthoritativeOverlay) -> bool {
    normalize_optional_overlay_value(overlay.current_branch_closure_id.as_deref()).is_some()
        || overlay.final_review_dispatch_lineage.is_some()
        || normalize_optional_overlay_value(overlay.current_release_readiness_result.as_deref())
            .is_some()
        || normalize_optional_overlay_value(overlay.final_review_state.as_deref()).is_some()
        || normalize_optional_overlay_value(overlay.browser_qa_state.as_deref()).is_some()
        || normalize_optional_overlay_value(overlay.release_docs_state.as_deref()).is_some()
}

fn current_task_closure_set_ready_for_late_stage(context: &ExecutionContext) -> bool {
    if structural_current_task_closure_failures(context)
        .map(|failures| !failures.is_empty())
        .unwrap_or(true)
    {
        return false;
    }
    let current_task_closures = match still_current_task_closure_records(context) {
        Ok(current_task_closures) => current_task_closures,
        Err(_) => return false,
    };
    if !current_task_closures
        .iter()
        .any(|record| task_closure_contributes_to_branch_surface(context, record))
    {
        return false;
    }
    branch_closure_rerecording_assessment(context)
        .map(|assessment| assessment.supported)
        .unwrap_or(false)
}

pub(super) fn authoritative_late_stage_rederivation_basis_present(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    if public_late_stage_rederivation_basis_present(status) {
        return true;
    }
    if current_task_closure_set_ready_for_late_stage(context) {
        return true;
    }
    if load_authoritative_transition_state(context)
        .ok()
        .flatten()
        .is_some_and(|state| {
            validated_current_branch_closure_identity(context).is_some()
                || state.current_release_readiness_record().is_some()
                || state.current_final_review_record().is_some()
                || state.current_browser_qa_record().is_some()
        })
    {
        return true;
    }
    load_status_authoritative_overlay_checked(context)
        .ok()
        .flatten()
        .is_some_and(|overlay| {
            overlay.final_review_dispatch_lineage.is_some()
                || normalize_optional_overlay_value(overlay.current_branch_closure_id.as_deref())
                    .is_some()
                || normalize_optional_overlay_value(
                    overlay.current_release_readiness_result.as_deref(),
                )
                .is_some()
                || normalize_optional_overlay_value(overlay.final_review_state.as_deref()).is_some()
                || normalize_optional_overlay_value(overlay.browser_qa_state.as_deref()).is_some()
                || normalize_optional_overlay_value(overlay.release_docs_state.as_deref()).is_some()
        })
}

pub(crate) fn apply_late_stage_precedence_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    gate_projection: Option<GateProjectionInputs<'_>>,
) {
    if status.execution_started != "yes" {
        return;
    }
    hydrate_status_authority_fields_for_routing(context, status, authoritative_state);

    let ordinary_execution_remaining = status.active_task.is_some()
        || status.resume_task.is_some()
        || status.blocking_task.is_some()
        || !context_all_task_scopes_closed_by_authority(context, authoritative_state);
    if ordinary_execution_remaining {
        if is_late_stage_phase(status.harness_phase) {
            if status.resume_task.is_some() || status.resume_step.is_some() {
                push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
            }
            status.harness_phase = HarnessPhase::Executing;
        }
        return;
    }

    if is_late_stage_phase(status.harness_phase)
        && task_scope_structural_review_state_reason(status).is_some()
    {
        push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
        status.harness_phase = HarnessPhase::Executing;
        return;
    }

    let authoritative_phase = status.harness_phase;
    let late_stage_basis_present =
        authoritative_late_stage_rederivation_basis_present(context, status);
    if !late_stage_basis_present {
        if is_late_stage_phase(authoritative_phase) {
            push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
            if task_scope_review_state_repair_reason(status).is_some()
                || status
                    .reason_codes
                    .iter()
                    .any(|reason_code| reason_code == "derived_review_state_missing")
            {
                status.harness_phase = HarnessPhase::Executing;
            } else {
                status.harness_phase = HarnessPhase::DocumentReleasePending;
            }
        }
        return;
    }
    if status.latest_authoritative_sequence != INITIAL_AUTHORITATIVE_SEQUENCE
        && !matches!(
            authoritative_phase,
            HarnessPhase::Executing
                | HarnessPhase::Repairing
                | HarnessPhase::DocumentReleasePending
                | HarnessPhase::FinalReviewPending
                | HarnessPhase::QaPending
                | HarnessPhase::ReadyForBranchCompletion
        )
    {
        return;
    }
    let Some(gate_projection) = gate_projection else {
        return;
    };
    let gate_review = gate_projection.gate_review;
    let gate_finish = gate_projection.gate_finish;
    if shared_qa_requirement_policy_invalid(Some(gate_finish)) {
        push_status_reason_code_once(status, "qa_requirement_missing_or_invalid");
        status.harness_phase = HarnessPhase::PivotRequired;
        return;
    }
    let execution_evidence_fingerprint_mismatch = gate_review
        .reason_codes
        .iter()
        .chain(gate_finish.reason_codes.iter())
        .any(|code| {
            matches!(
                code.as_str(),
                "plan_fingerprint_mismatch" | "spec_fingerprint_mismatch"
            )
        });
    if execution_evidence_fingerprint_mismatch
        && status.current_branch_closure_id.is_some()
        && status.current_release_readiness_state.is_none()
        && status.current_branch_meaningful_drift
    {
        push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
        status.harness_phase = HarnessPhase::Executing;
        return;
    }
    let release_blocked = status_release_blocked(gate_finish)
        || gate_review.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "release_docs_state_missing"
                    | "release_docs_state_stale"
                    | "release_docs_state_not_fresh"
            )
        });
    let review_blocked =
        status_review_truth_blocked(gate_review) || status_review_blocked(gate_finish);
    let qa_blocked = status_qa_blocked(gate_finish);
    let decision = resolve_late_stage_precedence(LateStageSignals {
        release: PrecedenceGateState::from_blocked(release_blocked),
        review: PrecedenceGateState::from_blocked(review_blocked),
        qa: PrecedenceGateState::from_blocked(qa_blocked),
    });
    let canonical_phase =
        parse_harness_phase(decision.phase).unwrap_or(HarnessPhase::FinalReviewPending);

    let checkpoint_missing = gate_finish
        .reason_codes
        .iter()
        .any(|code| code == "finish_review_gate_checkpoint_missing");

    if !(gate_finish.allowed || release_blocked || review_blocked || qa_blocked) {
        if status.current_branch_closure_id.is_none() {
            push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
            status.harness_phase = HarnessPhase::DocumentReleasePending;
            return;
        }
        if status.current_release_readiness_state.is_none() {
            if status.current_branch_meaningful_drift {
                push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
            }
            status.harness_phase = HarnessPhase::DocumentReleasePending;
            return;
        }
        if checkpoint_missing && canonical_phase == HarnessPhase::ReadyForBranchCompletion {
            status.harness_phase = HarnessPhase::ReadyForBranchCompletion;
            return;
        }
        push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
        status.harness_phase = HarnessPhase::FinalReviewPending;
        return;
    }

    if is_late_stage_phase(authoritative_phase) && authoritative_phase != canonical_phase {
        push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
        status.harness_phase = canonical_phase;
        return;
    }

    status.harness_phase = canonical_phase;
}

fn hydrate_status_authority_fields_for_routing(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) {
    if status.current_task_closures.is_empty()
        && let Some(authoritative_state) = authoritative_state
        && let Ok(current_task_closures) =
            project_current_task_closures(context, Some(authoritative_state))
    {
        status.current_task_closures = current_task_closures;
    }
    let Some(event_authority_state) = authoritative_state else {
        return;
    };
    if status.current_branch_closure_id.is_none()
        && let Some(current_identity) =
            usable_current_branch_closure_identity_from_authoritative_state(
                context,
                Some(event_authority_state),
            )
    {
        status.current_branch_closure_id = Some(current_identity.branch_closure_id);
        status.current_branch_reviewed_state_id = Some(current_identity.reviewed_state_id);
    }
    let current_late_stage_branch_closure_id = status
        .current_branch_reviewed_state_id
        .as_ref()
        .and(status.current_branch_closure_id.as_ref())
        .cloned();
    let late_stage_bindings = shared_current_late_stage_branch_bindings(
        Some(event_authority_state),
        current_late_stage_branch_closure_id.as_deref(),
        status.current_branch_reviewed_state_id.as_deref(),
    );
    if status.current_release_readiness_state.is_none() {
        status.current_release_readiness_state =
            late_stage_bindings.current_release_readiness_result.clone();
    }
    if status.current_final_review_branch_closure_id.is_none() {
        status.current_final_review_branch_closure_id =
            late_stage_bindings.current_final_review_branch_closure_id;
    }
    if status.current_final_review_result.is_none() {
        status.current_final_review_result = late_stage_bindings.current_final_review_result;
    }
    if status.current_qa_branch_closure_id.is_none() {
        status.current_qa_branch_closure_id = late_stage_bindings.current_qa_branch_closure_id;
    }
    if status.current_qa_result.is_none() {
        status.current_qa_result = late_stage_bindings.current_qa_result;
    }
    if status.finish_review_gate_pass_branch_closure_id.is_none() {
        status.finish_review_gate_pass_branch_closure_id =
            late_stage_bindings.finish_review_gate_pass_branch_closure_id;
    }
}
