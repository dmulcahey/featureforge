use crate::diagnostics::JsonFailure;
use crate::execution::closure_diagnostics::BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED;
use crate::execution::closure_graph::{AuthoritativeClosureGraph, ClosureGraphSignals};
use crate::execution::context::ExecutionContext;
use crate::execution::current_closure_projection::{
    project_current_task_closures_from_authoritative_state,
    still_current_task_closure_records_from_authoritative_state,
    structural_current_task_closure_failures_from_authoritative_state,
};
use crate::execution::current_truth::{
    branch_closure_rerecording_assessment_with_authority,
    current_branch_closure_has_tracked_drift as shared_current_branch_closure_has_tracked_drift,
    current_late_stage_branch_bindings as shared_current_late_stage_branch_bindings,
    current_task_negative_result_task as shared_current_task_negative_result_task,
    late_stage_missing_current_closure_stale_provenance_present_with_authority as shared_late_stage_missing_current_closure_stale_provenance_present_with_authority,
    late_stage_qa_blocked as shared_late_stage_qa_blocked,
    late_stage_release_blocked as shared_late_stage_release_blocked,
    late_stage_review_blocked as shared_late_stage_review_blocked,
    late_stage_review_truth_blocked as shared_late_stage_review_truth_blocked,
    late_stage_surface_not_declared_reason_code as shared_late_stage_surface_not_declared_reason_code,
    negative_result_requires_execution_reentry as shared_negative_result_requires_execution_reentry,
    normalized_plan_qa_requirement as shared_normalized_plan_qa_requirement,
    public_late_stage_rederivation_basis_present,
    qa_requirement_policy_invalid as shared_qa_requirement_policy_invalid,
    release_readiness_result_for_branch_closure as shared_release_readiness_result_for_branch_closure,
    task_closure_contributes_to_branch_surface,
};
use crate::execution::gate_reason_codes::QA_REQUIREMENT_MISSING_OR_INVALID;
use crate::execution::harness::{
    DownstreamFreshnessState, HarnessPhase, INITIAL_AUTHORITATIVE_SEQUENCE,
};
use crate::execution::late_stage_precedence::{
    GateState as PrecedenceGateState, LateStageSignals, resolve as resolve_late_stage_precedence,
};
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::reentry_reconcile::{
    TARGETLESS_STALE_MISSING_AUTHORITY_CODE, TARGETLESS_STALE_RECONCILE_REASON_CODE,
};
use crate::execution::review_route_tokens::is_release_docs_freshness_reason;
use crate::execution::status::{GateProjectionInputs, GateResult, PlanExecutionStatus};
use crate::execution::status_support::{
    context_all_task_scopes_closed_from_authority, qa_pending_requires_test_plan_refresh,
    resolve_branch_closure_reviewed_tree_sha,
};
use crate::execution::transitions::AuthoritativeTransitionState;

use super::branch_gate::validated_current_branch_closure_identity_from_authoritative_state;
use super::{
    StatusRepairFollowUpFacts, is_late_stage_phase, normalize_optional_overlay_value,
    parse_harness_phase, push_status_reason_code_once, task_scope_review_state_repair_reason,
    task_scope_structural_review_state_reason,
    usable_current_branch_closure_identity_from_authoritative_state,
};

pub(crate) fn apply_authoritative_late_stage_status_fields(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    overlay: Option<&StatusAuthoritativeOverlay>,
    event_authority_state: Option<&AuthoritativeTransitionState>,
) -> Result<Option<String>, JsonFailure> {
    if let Some(current_identity) =
        validated_current_branch_closure_identity_from_authoritative_state(
            context,
            event_authority_state,
        )
    {
        status.current_branch_closure_id = Some(current_identity.branch_closure_id.clone());
        if resolve_branch_closure_reviewed_tree_sha(
            &context.runtime.repo_root,
            &current_identity.branch_closure_id,
            &current_identity.reviewed_state_id,
        )
        .is_ok()
        {
            status.current_branch_reviewed_state_id = Some(current_identity.reviewed_state_id);
        } else {
            status.current_branch_reviewed_state_id = None;
            push_status_reason_code_once(
                status,
                BRANCH_BOUNDARY_REASON_CURRENT_BRANCH_CLOSURE_REVIEWED_STATE_MALFORMED,
            );
        }
    } else {
        status.current_branch_closure_id = None;
        status.current_branch_reviewed_state_id = None;
    }

    let closure_graph = AuthoritativeClosureGraph::from_state(
        event_authority_state,
        &ClosureGraphSignals::from_authoritative_state(
            event_authority_state,
            overlay.and_then(|overlay| overlay.current_branch_closure_id.as_deref()),
            false,
            false,
            Vec::new(),
        ),
    );
    status.current_release_readiness_state = None;
    status.current_final_review_branch_closure_id = None;
    status.current_final_review_result = None;
    status.current_qa_branch_closure_id = None;
    status.current_qa_result = None;
    let current_late_stage_branch_closure_id = status
        .current_branch_reviewed_state_id
        .as_ref()
        .and(status.current_branch_closure_id.as_ref())
        .cloned();
    let late_stage_bindings = shared_current_late_stage_branch_bindings(
        event_authority_state,
        current_late_stage_branch_closure_id.as_deref(),
        status.current_branch_reviewed_state_id.as_deref(),
    );
    status.current_release_readiness_state =
        late_stage_bindings.current_release_readiness_result.clone();
    status.current_final_review_branch_closure_id = late_stage_bindings
        .current_final_review_branch_closure_id
        .clone();
    status.current_final_review_result = late_stage_bindings.current_final_review_result.clone();
    status.current_qa_branch_closure_id = late_stage_bindings.current_qa_branch_closure_id.clone();
    status.current_qa_result = late_stage_bindings.current_qa_result.clone();
    status.qa_requirement =
        shared_normalized_plan_qa_requirement(context.plan_document.qa_requirement.as_deref());
    if status.current_release_readiness_state.is_some() {
        status.release_docs_state = DownstreamFreshnessState::Fresh;
    } else {
        status.release_docs_state = DownstreamFreshnessState::NotRequired;
        status.last_release_docs_artifact_fingerprint = None;
    }
    if status.current_final_review_branch_closure_id.is_some()
        && status.current_final_review_result.is_some()
    {
        status.final_review_state = DownstreamFreshnessState::Fresh;
    } else {
        status.final_review_state = DownstreamFreshnessState::NotRequired;
        status.last_final_review_artifact_fingerprint = None;
    }
    if status.current_qa_branch_closure_id.is_some() && status.current_qa_result.is_some() {
        status.browser_qa_state = DownstreamFreshnessState::Fresh;
    } else if status.current_final_review_result.is_some()
        && status.qa_requirement.as_deref() == Some("required")
    {
        status.browser_qa_state = DownstreamFreshnessState::Missing;
        status.last_browser_qa_artifact_fingerprint = None;
    } else {
        status.browser_qa_state = DownstreamFreshnessState::NotRequired;
        status.last_browser_qa_artifact_fingerprint = None;
    }
    let authoritative_downstream_truth_present = status.current_branch_closure_id.is_some()
        || event_authority_state.is_some_and(|state| {
            state.current_release_readiness_record_id().is_some()
                || state.current_final_review_record_id().is_some()
                || state.current_qa_record_id().is_some()
        });
    if !authoritative_downstream_truth_present {
        status.final_review_state = DownstreamFreshnessState::NotRequired;
        status.browser_qa_state = DownstreamFreshnessState::NotRequired;
        status.release_docs_state = DownstreamFreshnessState::NotRequired;
        status.last_final_review_artifact_fingerprint = None;
        status.last_browser_qa_artifact_fingerprint = None;
        status.last_release_docs_artifact_fingerprint = None;
    }
    status.current_final_review_state =
        downstream_freshness_state_label(status.final_review_state).to_owned();
    status.current_qa_state = downstream_freshness_state_label(status.browser_qa_state).to_owned();
    status.current_branch_meaningful_drift =
        shared_current_branch_closure_has_tracked_drift(context, event_authority_state)
            .unwrap_or(false);
    status.current_task_closures =
        project_current_task_closures_from_authoritative_state(context, event_authority_state)?;
    status.superseded_closures_summary = closure_graph.superseded_record_ids();
    status.finish_review_gate_pass_branch_closure_id =
        late_stage_bindings.finish_review_gate_pass_branch_closure_id;
    if let Some(late_stage_phase) = canonical_late_stage_phase_from_bindings(status) {
        status.harness_phase = late_stage_phase;
    }
    Ok(current_late_stage_branch_closure_id)
}

pub(crate) struct LateStageRepairStatusOverlayInputs<'a> {
    pub(crate) gate_finish: &'a GateResult,
    pub(crate) overlay: Option<&'a StatusAuthoritativeOverlay>,
    pub(crate) event_authority_state: Option<&'a AuthoritativeTransitionState>,
    pub(crate) current_late_stage_branch_closure_id: Option<&'a str>,
    pub(crate) repair_follow_up_facts: &'a StatusRepairFollowUpFacts,
    pub(crate) task_scope_overlay_restore_required: bool,
    pub(crate) branch_closure_recording_basis_missing: bool,
}

pub(crate) fn apply_late_stage_repair_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    inputs: LateStageRepairStatusOverlayInputs<'_>,
) -> Result<(), JsonFailure> {
    let persisted_repair_follow_up = inputs
        .repair_follow_up_facts
        .persisted_repair_follow_up
        .as_deref();
    let authoritative_release_readiness_result = shared_release_readiness_result_for_branch_closure(
        inputs.event_authority_state,
        inputs.current_late_stage_branch_closure_id,
    );
    let authoritative_release_readiness_current = authoritative_release_readiness_result.is_some();
    let confined_late_stage_branch_drift_with_release_readiness =
        authoritative_release_readiness_current
            && inputs.repair_follow_up_facts.branch_reroute_still_valid
            && inputs.current_late_stage_branch_closure_id.is_some()
            && status
                .reason_codes
                .iter()
                .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE);
    if (inputs.repair_follow_up_facts.records_branch_closure
        || confined_late_stage_branch_drift_with_release_readiness)
        && authoritative_release_readiness_current
        && status.current_release_readiness_state.is_none()
    {
        status.current_release_readiness_state = authoritative_release_readiness_result;
        if status.current_release_readiness_state.as_deref() == Some("ready") {
            status.release_docs_state = DownstreamFreshnessState::Fresh;
        }
    }
    if inputs.repair_follow_up_facts.requires_execution_reentry {
        status.harness_phase = HarnessPhase::Executing;
    } else if inputs.repair_follow_up_facts.requires_planning_reentry {
        status.harness_phase = HarnessPhase::PivotRequired;
    } else if inputs.repair_follow_up_facts.records_branch_closure
        && persisted_repair_follow_up
            == Some(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE)
    {
        status.harness_phase = if status.current_release_readiness_state.is_some()
            || authoritative_release_readiness_current
        {
            HarnessPhase::FinalReviewPending
        } else {
            HarnessPhase::DocumentReleasePending
        };
    }

    let authoritative_task_closure_baseline_present =
        inputs.event_authority_state.is_some_and(|state| {
            !state.current_task_closure_results().is_empty()
                || context
                    .tasks_by_number
                    .keys()
                    .any(|task| state.raw_current_task_closure_state_entry(*task).is_some())
        });
    let late_stage_surface_requires_planning_reentry = status.current_branch_closure_id.is_none()
        && status.current_task_closures.is_empty()
        && !authoritative_task_closure_baseline_present
        && status.blocking_task.is_none()
        && !status.reason_codes.iter().any(|reason_code| {
            reason_code
                == crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING
        })
        && status
            .reason_codes
            .iter()
            .any(|reason_code| shared_late_stage_surface_not_declared_reason_code(reason_code));
    let late_stage_missing_current_closure_stale_provenance =
        shared_late_stage_missing_current_closure_stale_provenance_present_with_authority(
            context,
            status,
            inputs.event_authority_state,
        )?;
    let preserve_canonical_late_stage_harness_phase = inputs.branch_closure_recording_basis_missing
        && is_late_stage_phase(status.harness_phase)
        && (late_stage_missing_current_closure_stale_provenance
            || status.latest_authoritative_sequence != INITIAL_AUTHORITATIVE_SEQUENCE
            || persisted_repair_follow_up
                == Some(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE))
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE);
    if authoritative_task_closure_baseline_present
        && status.harness_phase == HarnessPhase::PivotRequired
        && status.current_branch_closure_id.is_none()
    {
        status.harness_phase = HarnessPhase::Executing;
    }
    if late_stage_surface_requires_planning_reentry
        && status.current_branch_closure_id.is_none()
        && let Some(task) = context.tasks_by_number.keys().copied().max()
    {
        status.harness_phase = HarnessPhase::Executing;
        status.blocking_task = Some(task);
        status.blocking_step = None;
        push_status_reason_code_once(status, crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING);
        push_status_reason_code_once(status, crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE);
    }
    if late_stage_surface_requires_planning_reentry && status.blocking_task.is_none() {
        status.harness_phase = HarnessPhase::PivotRequired;
    } else if inputs.branch_closure_recording_basis_missing
        && !preserve_canonical_late_stage_harness_phase
        && !late_stage_surface_requires_planning_reentry
    {
        status.harness_phase = HarnessPhase::Executing;
    }
    apply_negative_result_status_overlay(
        context,
        status,
        inputs.gate_finish,
        inputs.overlay,
        inputs.event_authority_state,
    );
    if (inputs.task_scope_overlay_restore_required || inputs.branch_closure_recording_basis_missing)
        && !preserve_canonical_late_stage_harness_phase
    {
        status.harness_phase = HarnessPhase::Executing;
    }
    let persisted_branch_reroute_projection = status.execution_started == "yes"
        && !inputs.task_scope_overlay_restore_required
        && status.current_branch_closure_id.is_some()
        && status.review_state_status
            == crate::execution::review_route_tokens::REVIEW_STATE_MISSING_CURRENT_CLOSURE
        && inputs.repair_follow_up_facts.branch_reroute_still_valid
        && persisted_repair_follow_up
            == Some(crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE);
    if persisted_branch_reroute_projection {
        status.harness_phase = HarnessPhase::DocumentReleasePending;
    }
    Ok(())
}

fn canonical_late_stage_phase_from_bindings(status: &PlanExecutionStatus) -> Option<HarnessPhase> {
    if status.execution_started != "yes"
        || status.current_branch_closure_id.is_none()
        || status.active_task.is_some()
        || status.active_step.is_some()
        || status.resume_task.is_some()
        || status.resume_step.is_some()
        || status.blocking_task.is_some()
        || status.blocking_step.is_some()
        || matches!(
            status.harness_phase,
            HarnessPhase::PivotRequired | HarnessPhase::HandoffRequired
        )
    {
        return None;
    }
    if status.current_release_readiness_state.as_deref() != Some("ready") {
        return Some(HarnessPhase::DocumentReleasePending);
    }
    if status.current_final_review_result.is_none() {
        return Some(HarnessPhase::FinalReviewPending);
    }
    if status.qa_requirement.as_deref() == Some("required") && status.current_qa_result.is_none() {
        return Some(HarnessPhase::QaPending);
    }
    Some(HarnessPhase::ReadyForBranchCompletion)
}

fn downstream_freshness_state_label(state: DownstreamFreshnessState) -> &'static str {
    match state {
        DownstreamFreshnessState::NotRequired => "not_required",
        DownstreamFreshnessState::Missing => "missing",
        DownstreamFreshnessState::Fresh => "fresh",
        DownstreamFreshnessState::Stale => "stale",
    }
}

pub(crate) fn has_authoritative_late_stage_progress(overlay: &StatusAuthoritativeOverlay) -> bool {
    normalize_optional_overlay_value(overlay.current_branch_closure_id.as_deref()).is_some()
        || overlay.final_review_dispatch_lineage.is_some()
        || normalize_optional_overlay_value(overlay.current_release_readiness_result.as_deref())
            .is_some()
        || normalize_optional_overlay_value(overlay.final_review_state.as_deref()).is_some()
        || normalize_optional_overlay_value(overlay.browser_qa_state.as_deref()).is_some()
        || normalize_optional_overlay_value(overlay.release_docs_state.as_deref()).is_some()
}

fn current_task_closure_set_ready_for_late_stage_with_authority(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    let Some(authoritative_state) = authoritative_state else {
        return false;
    };
    if !structural_current_task_closure_failures_from_authoritative_state(
        context,
        authoritative_state,
    )
    .is_empty()
    {
        return false;
    }
    let current_task_closures = match still_current_task_closure_records_from_authoritative_state(
        context,
        authoritative_state,
    ) {
        Ok(current_task_closures) => current_task_closures,
        Err(_) => return false,
    };
    if !current_task_closures
        .iter()
        .any(|record| task_closure_contributes_to_branch_surface(context, record))
    {
        return false;
    }
    branch_closure_rerecording_assessment_with_authority(context, Some(authoritative_state))
        .map(|assessment| assessment.supported)
        .unwrap_or(false)
}

pub(super) fn authoritative_late_stage_rederivation_basis_present_with_authority(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    authoritative_overlay: Option<&StatusAuthoritativeOverlay>,
) -> bool {
    if public_late_stage_rederivation_basis_present(status) {
        return true;
    }
    if current_task_closure_set_ready_for_late_stage_with_authority(context, authoritative_state) {
        return true;
    }
    if authoritative_state.is_some_and(|state| {
        validated_current_branch_closure_identity_from_authoritative_state(context, Some(state))
            .is_some()
            || state.current_release_readiness_record().is_some()
            || state.current_final_review_record().is_some()
            || state.current_browser_qa_record().is_some()
    }) {
        return true;
    }
    authoritative_overlay.is_some_and(has_authoritative_late_stage_progress)
}

pub(crate) fn apply_late_stage_precedence_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    authoritative_overlay: Option<&StatusAuthoritativeOverlay>,
    gate_projection: Option<GateProjectionInputs<'_>>,
) {
    if status.execution_started != "yes" {
        return;
    }
    hydrate_status_authority_fields_for_routing(context, status, authoritative_state);

    let ordinary_execution_remaining = status.active_task.is_some()
        || status.resume_task.is_some()
        || status.blocking_task.is_some()
        || !context_all_task_scopes_closed_from_authority(context, authoritative_state);
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
        authoritative_late_stage_rederivation_basis_present_with_authority(
            context,
            status,
            authoritative_state,
            authoritative_overlay,
        );
    if !late_stage_basis_present {
        if is_late_stage_phase(authoritative_phase) {
            push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
            if task_scope_review_state_repair_reason(status).is_some() {
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
        push_status_reason_code_once(status, QA_REQUIREMENT_MISSING_OR_INVALID);
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
        || gate_review
            .reason_codes
            .iter()
            .any(|code| is_release_docs_freshness_reason(code));
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
        && let Ok(current_task_closures) = project_current_task_closures_from_authoritative_state(
            context,
            Some(authoritative_state),
        )
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

fn status_workflow_phase(status: &PlanExecutionStatus) -> &'static str {
    match status.harness_phase {
        HarnessPhase::DocumentReleasePending
        | HarnessPhase::FinalReviewPending
        | HarnessPhase::QaPending
        | HarnessPhase::ReadyForBranchCompletion
        | HarnessPhase::HandoffRequired
        | HarnessPhase::PivotRequired
        | HarnessPhase::Executing => status.harness_phase.as_str(),
        _ => HarnessPhase::Executing.as_str(),
    }
}

fn status_late_stage_prerequisite_reroute_active(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    gate_finish: &GateResult,
) -> bool {
    match status.harness_phase {
        HarnessPhase::DocumentReleasePending => true,
        HarnessPhase::FinalReviewPending => {
            status.current_branch_closure_id.is_none()
                || status.current_release_readiness_state.as_deref() != Some("ready")
        }
        HarnessPhase::QaPending => {
            status.current_branch_closure_id.is_none()
                || (shared_normalized_plan_qa_requirement(
                    context.plan_document.qa_requirement.as_deref(),
                )
                .as_deref()
                    == Some("required")
                    && qa_pending_requires_test_plan_refresh(context, Some(gate_finish)))
        }
        _ => false,
    }
}

fn apply_negative_result_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    gate_finish: &GateResult,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    if status_late_stage_prerequisite_reroute_active(context, status, gate_finish) {
        return false;
    }
    let task_negative_result_task =
        shared_current_task_negative_result_task(status, overlay, authoritative_state);
    if task_negative_result_task.is_some() {
        push_status_reason_code_once(
            status,
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN,
        );
    }
    if !shared_negative_result_requires_execution_reentry(
        task_negative_result_task.is_some(),
        status_workflow_phase(status),
        status.current_branch_closure_id.as_deref(),
        status.current_final_review_branch_closure_id.as_deref(),
        status.current_final_review_result.as_deref(),
        status.current_qa_branch_closure_id.as_deref(),
        status.current_qa_result.as_deref(),
    ) {
        return false;
    }
    status.harness_phase = HarnessPhase::Executing;
    status.review_state_status = String::from("clean");
    status.stale_unreviewed_closures.clear();
    status.reason_codes.retain(|reason_code| {
        reason_code != TARGETLESS_STALE_RECONCILE_REASON_CODE
            && reason_code != TARGETLESS_STALE_MISSING_AUTHORITY_CODE
    });
    push_status_reason_code_once(
        status,
        crate::execution::review_route_tokens::REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY,
    );
    true
}

fn status_release_blocked(gate_finish: &GateResult) -> bool {
    shared_late_stage_release_blocked(Some(gate_finish))
}

fn status_review_blocked(gate_finish: &GateResult) -> bool {
    shared_late_stage_review_blocked(Some(gate_finish))
}

fn status_review_truth_blocked(gate_review: &GateResult) -> bool {
    shared_late_stage_review_truth_blocked(Some(gate_review))
}

fn status_qa_blocked(gate_finish: &GateResult) -> bool {
    shared_late_stage_qa_blocked(Some(gate_finish))
}
