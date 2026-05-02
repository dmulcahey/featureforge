use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FinalReviewDispatchAuthority {
    pub(crate) dispatch_id: Option<String>,
    pub(crate) lineage_present: bool,
}

pub(crate) fn shared_repair_review_state_reroute_decision(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    event_authority_state: Option<&AuthoritativeTransitionState>,
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
    task_scope_overlay_restore_required: bool,
    additional_branch_drift_signal: bool,
) -> SharedRepairReviewStateRerouteDecision {
    let branch_reroute_assessment = branch_closure_rerecording_assessment(context).ok();
    let branch_reroute_still_valid = branch_reroute_assessment
        .as_ref()
        .is_some_and(|assessment| assessment.supported);
    let branch_drift_escapes_late_stage_surface = branch_reroute_assessment
        .as_ref()
        .and_then(|assessment| assessment.unsupported_reason)
        == Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface);
    let late_stage_surface_not_declared = branch_reroute_assessment
        .as_ref()
        .and_then(|assessment| assessment.unsupported_reason)
        == Some(BranchRerecordingUnsupportedReason::LateStageSurfaceNotDeclared)
        || (branch_reroute_assessment
            .as_ref()
            .is_some_and(|assessment| !assessment.supported)
            && (!status.current_task_closures.is_empty()
                || event_authority_state
                    .is_some_and(|state| !state.current_task_closure_results().is_empty()))
            && normalized_late_stage_surface(&context.plan_source)
                .is_ok_and(|surface| surface.is_empty()));
    let persisted_repair_follow_up =
        resolve_actionable_repair_follow_up_for_status(context, status, event_authority_state)
            .map(|record| record.kind.public_token().to_owned());
    let late_stage_stale_unreviewed = shared_public_review_state_stale_unreviewed_for_reroute(
        context,
        event_authority_state,
        status,
        gate_review,
        gate_finish,
    )
    .unwrap_or_else(|_| {
        shared_public_late_stage_stale_unreviewed(status, gate_review, gate_finish)
            || status.current_branch_meaningful_drift
    });
    let branch_scope_stale_unreviewed = late_stage_stale_unreviewed
        || status.current_branch_meaningful_drift
        || additional_branch_drift_signal
        || branch_drift_escapes_late_stage_surface;
    let raw_late_stage_review_state_status =
        live_review_state_status_for_reroute_from_status(status, branch_scope_stale_unreviewed);
    let task_scope_repair_precedence_active = shared_live_task_scope_repair_precedence_active(
        task_scope_overlay_restore_required,
        task_scope_structural_review_state_reason(status).is_some(),
        shared_task_scope_stale_review_state_reason_present(task_scope_review_state_repair_reason(
            status,
        )),
        persisted_repair_follow_up.as_deref(),
        branch_reroute_still_valid,
        raw_late_stage_review_state_status,
    );
    let repair_reroute = shared_live_review_state_repair_reroute(
        persisted_repair_follow_up.as_deref(),
        task_scope_repair_precedence_active,
        branch_reroute_still_valid,
        raw_late_stage_review_state_status,
        shared_branch_closure_refresh_missing_current_closure(status),
    );
    SharedRepairReviewStateRerouteDecision {
        branch_reroute_still_valid,
        branch_drift_escapes_late_stage_surface,
        late_stage_surface_not_declared,
        persisted_repair_follow_up,
        raw_late_stage_review_state_status,
        task_scope_repair_precedence_active,
        repair_reroute,
    }
}

pub(crate) fn current_task_review_dispatch_id_for_status(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Option<String> {
    let current_task_lineage_fingerprint = status
        .blocking_task
        .and_then(|task_number| task_completion_lineage_fingerprint(context, task_number));
    let current_task_semantic_reviewed_state_id = status.blocking_task.and_then(|_| {
        semantic_workspace_snapshot(context)
            .ok()
            .map(|snapshot| snapshot.semantic_workspace_tree_id)
    });
    shared_current_task_review_dispatch_id(
        status.blocking_task,
        current_task_lineage_fingerprint.as_deref(),
        current_task_semantic_reviewed_state_id.as_deref(),
        None,
        overlay,
    )
}

pub(crate) fn current_final_review_dispatch_authority_for_context(
    context: &ExecutionContext,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> FinalReviewDispatchAuthority {
    let usable_current_branch_closure_id =
        usable_current_branch_closure_identity_from_authoritative_state(
            context,
            authoritative_state,
        )
        .map(|identity| identity.branch_closure_id);
    current_final_review_dispatch_authority(
        usable_current_branch_closure_id.as_deref(),
        overlay,
        authoritative_state,
    )
}

pub(crate) fn current_final_review_dispatch_authority(
    usable_current_branch_closure_id: Option<&str>,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> FinalReviewDispatchAuthority {
    let mut dispatch_id = shared_current_final_review_dispatch_id(
        usable_current_branch_closure_id,
        overlay,
    )
    .or_else(|| {
        authoritative_state.and_then(|state| {
            if state.current_final_review_branch_closure_id() != usable_current_branch_closure_id {
                return None;
            }
            state
                .current_final_review_dispatch_id()
                .map(str::trim)
                .filter(|dispatch_id| !dispatch_id.is_empty())
                .map(ToOwned::to_owned)
        })
    });
    let current_final_review_record_non_current = authoritative_state.is_some_and(|state| {
        let Some(record_id) = state
            .current_final_review_record_id()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
        else {
            return false;
        };
        state
            .final_review_record_by_id(&record_id)
            .is_none_or(|record| record.record_status != "current")
    });
    if current_final_review_record_non_current {
        dispatch_id = None;
    }
    let lineage_present = !current_final_review_record_non_current
        && (overlay
            .and_then(|overlay| overlay.final_review_dispatch_lineage.as_ref())
            .and_then(|record| {
                let execution_run_id = record.execution_run_id.as_deref()?;
                if execution_run_id.trim().is_empty() {
                    return None;
                }
                let branch_closure_id = record.branch_closure_id.as_deref()?;
                if usable_current_branch_closure_id != Some(branch_closure_id) {
                    return None;
                }
                record
                    .dispatch_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
            .is_some()
            || dispatch_id.is_some());
    FinalReviewDispatchAuthority {
        dispatch_id,
        lineage_present,
    }
}
