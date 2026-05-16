use crate::execution::closure_dispatch::{
    current_final_review_dispatch_id_from_authority, current_task_review_dispatch_id_for_task,
};
use crate::execution::context::ExecutionContext;
use crate::execution::current_truth::{
    BranchRerecordingUnsupportedReason,
    branch_closure_refresh_missing_current_closure as shared_branch_closure_refresh_missing_current_closure,
    branch_closure_rerecording_assessment_with_authority,
    live_review_state_repair_reroute as shared_live_review_state_repair_reroute,
    live_review_state_status_for_reroute as shared_live_review_state_status_for_reroute,
    live_task_scope_repair_precedence_active as shared_live_task_scope_repair_precedence_active,
    normalized_late_stage_surface, public_late_stage_rederivation_basis_present,
    public_late_stage_stale_unreviewed as shared_public_late_stage_stale_unreviewed,
    public_review_state_stale_unreviewed_for_reroute as shared_public_review_state_stale_unreviewed_for_reroute,
    resolve_actionable_repair_follow_up_for_status,
    stale_provenance_after_authoritative_closure_is_diagnostic,
    task_scope_stale_review_state_reason_present as shared_task_scope_stale_review_state_reason_present,
};
use crate::execution::harness::HarnessPhase;
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::status::{GateResult, PlanExecutionStatus};
use crate::execution::transitions::AuthoritativeTransitionState;

use super::blocking_records::task_scope_overlay_repair_required;
use super::{
    SharedRepairReviewStateRerouteDecision, StatusReviewStateInputs,
    current_branch_closure_structural_review_state_reason, effective_review_state_status,
    prerelease_branch_closure_refresh_required,
    task_closure_baseline_repair_candidate_reason_present, task_scope_review_state_repair_reason,
    task_scope_structural_review_state_reason,
    usable_current_branch_closure_identity_from_authoritative_state,
};

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
    let branch_reroute_assessment =
        branch_closure_rerecording_assessment_with_authority(context, event_authority_state).ok();
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
        branch_rerecording_assessment: branch_reroute_assessment,
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
    current_task_review_dispatch_id_for_task(context, status.blocking_task, overlay)
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
    let mut dispatch_id = current_final_review_dispatch_id_from_authority(
        usable_current_branch_closure_id,
        overlay,
        authoritative_state,
    );
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

pub(crate) fn live_review_state_status_for_reroute_from_status(
    status: &PlanExecutionStatus,
    late_stage_stale_unreviewed: bool,
) -> Option<&'static str> {
    if shared_branch_closure_refresh_missing_current_closure(status) {
        return Some(crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE);
    }
    shared_live_review_state_status_for_reroute(
        late_stage_stale_unreviewed,
        current_branch_closure_structural_review_state_reason(status).is_some()
            || shared_branch_closure_refresh_missing_current_closure(status)
            || (matches!(
                status.harness_phase,
                HarnessPhase::DocumentReleasePending
                    | HarnessPhase::FinalReviewPending
                    | HarnessPhase::QaPending
                    | HarnessPhase::ReadyForBranchCompletion
            ) && status.current_branch_closure_id.is_none()),
    )
}

pub(crate) fn derive_status_review_state_fact(
    status: &PlanExecutionStatus,
    gate_review: &GateResult,
    gate_finish: &GateResult,
    facts: &StatusReviewStateInputs,
) -> String {
    let candidate = derive_status_review_state_candidate(status, gate_review, gate_finish, facts);
    effective_review_state_status(status, candidate.as_str())
}

fn derive_status_review_state_candidate(
    status: &PlanExecutionStatus,
    gate_review: &GateResult,
    gate_finish: &GateResult,
    facts: &StatusReviewStateInputs,
) -> String {
    let task_boundary_stale_unreviewed_bridge = facts.task_boundary_unresolved_stale
        && status.blocking_task.is_some()
        && status.blocking_step.is_none()
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && task_closure_baseline_repair_candidate_reason_present(status)
        && status.reason_codes.iter().any(|code| {
            code == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
        })
        && !status.reason_codes.iter().any(|code| {
            crate::execution::closure_diagnostics::task_boundary_projection_diagnostic_reason_code(
                code,
            ) || crate::execution::closure_diagnostics::task_boundary_negative_review_reason_code(
                code,
            ) || crate::execution::closure_diagnostics::task_boundary_current_closure_stale_reason_code(
                code,
            )
        });
    let task_scope_stale_unreviewed =
        !task_closure_baseline_repair_candidate_reason_present(status)
            && status.reason_codes.iter().any(|code| {
                matches!(
                    code.as_str(),
                    crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE
                )
            });
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
    let resumed_task_stale_unreviewed = (status.resume_task.is_some()
        || status.resume_step.is_some())
        && status
            .reason_codes
            .iter()
            .any(|code| code == REASON_CODE_STALE_PROVENANCE);
    let late_stage_stale_signals =
        shared_public_late_stage_stale_unreviewed(status, Some(gate_review), Some(gate_finish))
            || facts.branch_scope_stale_unreviewed;
    let stale_provenance_closed_boundary_diagnostic = execution_evidence_fingerprint_mismatch
        && stale_provenance_after_authoritative_closure_is_diagnostic(status)
        && !facts.task_boundary_unresolved_stale
        && !facts.branch_scope_stale_unreviewed;
    let task_scope_execution_reentry_active = (status.active_task.is_some()
        || status.resume_task.is_some()
        || status.blocking_step.is_some())
        && status.current_branch_closure_id.is_none()
        && !public_late_stage_rederivation_basis_present(status);
    let late_stage_stale_unreviewed =
        late_stage_stale_signals && !task_scope_execution_reentry_active;
    let prerelease_refresh_missing_current_closure =
        prerelease_branch_closure_refresh_required(status);
    let stale_provenance_with_real_stale_target = !stale_provenance_closed_boundary_diagnostic
        && status
            .reason_codes
            .iter()
            .any(|code| code == REASON_CODE_STALE_PROVENANCE)
        && (facts.task_boundary_unresolved_stale || !status.stale_unreviewed_closures.is_empty());
    if task_boundary_stale_unreviewed_bridge {
        return String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED);
    }
    if stale_provenance_with_real_stale_target {
        return String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED);
    }
    if stale_provenance_closed_boundary_diagnostic {
        return String::from("clean");
    }
    if facts.repair_follow_up_requires_execution_reentry
        && !prerelease_refresh_missing_current_closure
        && !facts.branch_scope_stale_unreviewed
        && !status
            .reason_codes
            .iter()
            .any(|code| code == REASON_CODE_STALE_PROVENANCE)
    {
        return String::from("clean");
    }
    if status.stale_unreviewed_closures.is_empty()
        && !facts.task_boundary_unresolved_stale
        && !status.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                REASON_CODE_STALE_PROVENANCE | crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE
            )
        })
        && (task_scope_structural_review_state_reason(status).is_some()
            || task_scope_overlay_repair_required(status))
    {
        return String::from("clean");
    }
    if resumed_task_stale_unreviewed {
        return String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED);
    }
    if current_branch_closure_structural_review_state_reason(status).is_some() {
        return String::from(
            crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE,
        );
    }
    if facts.repair_follow_up_records_branch_closure {
        if status.current_release_readiness_state.is_some() {
            return String::from("clean");
        }
        return String::from(
            crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE,
        );
    }
    if prerelease_refresh_missing_current_closure {
        return String::from(
            crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE,
        );
    }
    if task_scope_stale_unreviewed {
        return String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED);
    }
    if status.harness_phase == HarnessPhase::DocumentReleasePending
        && status.current_branch_closure_id.is_some()
        && status.current_release_readiness_state.is_none()
        && !status.current_branch_meaningful_drift
        && !facts.branch_scope_stale_unreviewed
    {
        return String::from("clean");
    }
    if late_stage_stale_unreviewed && status.current_branch_closure_id.is_some() {
        return String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED);
    }
    if matches!(
        status.harness_phase,
        HarnessPhase::DocumentReleasePending
            | HarnessPhase::FinalReviewPending
            | HarnessPhase::QaPending
            | HarnessPhase::ReadyForBranchCompletion
    ) && (status.current_branch_closure_id.is_none()
        || prerelease_branch_closure_refresh_required(status))
    {
        return String::from(
            crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE,
        );
    }
    if late_stage_stale_unreviewed {
        return String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED);
    }
    String::from("clean")
}
