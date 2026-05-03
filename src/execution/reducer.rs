use serde::Serialize;

use crate::diagnostics::JsonFailure;
use crate::execution::current_truth::{
    BranchRerecordingAssessment, CurrentLateStageBranchBindings, CurrentTruthSnapshot,
    branch_closure_rerecording_assessment_with_authority, current_late_stage_branch_bindings,
    release_readiness_result_for_branch_closure, resolve_actionable_repair_follow_up,
};
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::public_repair_targets::{
    public_repair_target_candidates_from_authority, public_repair_target_warning_codes,
};
use crate::execution::semantic_identity::{SemanticWorkspaceSnapshot, semantic_workspace_snapshot};
use crate::execution::stale_target_projection::{
    RuntimeGateSnapshot, StaleTargetProjectionInputs, project_authoritative_stale_targets,
    project_stale_unreviewed_closures,
};
use crate::execution::state::{
    ExecutionContext, ExecutionDerivedTruth, ExecutionReadScope, FinalReviewDispatchAuthority,
    GateProjectionInputs, GateResult, GateState, PlanExecutionStatus, PublicRepairTarget,
    compute_status_blocking_records, current_task_review_dispatch_id_for_status,
    derive_execution_truth_from_authority, derive_execution_truth_from_authority_with_gates,
    gate_finish_from_context, gate_review_from_context, preflight_from_context,
    usable_current_branch_closure_identity_from_authoritative_state,
};
use crate::execution::transitions::AuthoritativeTransitionState;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RuntimeState {
    #[serde(skip)]
    pub(crate) context: ExecutionContext,
    pub(crate) semantic_workspace: SemanticWorkspaceSnapshot,
    pub(crate) status: PlanExecutionStatus,
    pub(crate) route_repair_target_candidates: Vec<PublicRepairTarget>,
    pub(crate) preflight: Option<GateResult>,
    pub(crate) gate_review: Option<GateResult>,
    pub(crate) gate_finish: Option<GateResult>,
    pub(crate) gate_snapshot: RuntimeGateSnapshot,
    pub(crate) base_branch: Option<String>,
    pub(crate) authoritative_current_branch_closure_id: Option<String>,
    pub(crate) authoritative_current_branch_reviewed_state_id: Option<String>,
    pub(crate) late_stage_bindings: CurrentLateStageBranchBindings,
    pub(crate) persisted_repair_follow_up: Option<String>,
    pub(crate) release_readiness_result_for_current_branch: Option<String>,
    #[serde(skip)]
    pub(crate) branch_rerecording_assessment: Option<BranchRerecordingAssessment>,
    pub(crate) task_review_dispatch_id: Option<String>,
    #[serde(skip)]
    pub(crate) final_review_dispatch_authority: FinalReviewDispatchAuthority,
}

fn should_project_late_stage_gates(status: &PlanExecutionStatus) -> bool {
    status.execution_started == "yes"
        && (matches!(
            status.harness_phase,
            crate::execution::harness::HarnessPhase::DocumentReleasePending
                | crate::execution::harness::HarnessPhase::FinalReviewPending
                | crate::execution::harness::HarnessPhase::QaPending
                | crate::execution::harness::HarnessPhase::ReadyForBranchCompletion
        ) || status.current_branch_closure_id.is_some()
            || status.current_release_readiness_state.is_some()
            || status.current_final_review_state != "not_required"
            || status.current_qa_state != "not_required"
            || status.finish_review_gate_pass_branch_closure_id.is_some())
}

pub(crate) fn reduce_execution_read_scope(
    read_scope: &ExecutionReadScope,
) -> Result<RuntimeState, JsonFailure> {
    reduce_runtime_state(
        &read_scope.context,
        read_scope.authoritative_state.as_ref(),
        semantic_workspace_snapshot(&read_scope.context)?,
    )
}

pub(crate) struct EventAuthoritySnapshot<'a> {
    pub(crate) context: &'a ExecutionContext,
    pub(crate) event_authority_state: Option<&'a AuthoritativeTransitionState>,
    pub(crate) semantic_workspace: SemanticWorkspaceSnapshot,
}

pub(crate) fn reduce_event_authority_for_migration_parity(
    snapshot: EventAuthoritySnapshot<'_>,
) -> Result<RuntimeState, JsonFailure> {
    let derived =
        derive_execution_truth_from_authority(snapshot.context, snapshot.event_authority_state)?;
    build_runtime_state_from_event_authority(
        snapshot.context,
        derived,
        snapshot.event_authority_state,
        snapshot.semantic_workspace,
        None,
        None,
        false,
    )
}

pub(crate) fn reduce_runtime_state(
    context: &ExecutionContext,
    event_authority_state: Option<&AuthoritativeTransitionState>,
    semantic_workspace: SemanticWorkspaceSnapshot,
) -> Result<RuntimeState, JsonFailure> {
    let gate_review = gate_review_from_context(context);
    let gate_finish = gate_finish_from_context(context);
    let derived = derive_execution_truth_from_authority_with_gates(
        context,
        event_authority_state,
        Some(GateProjectionInputs {
            gate_review: &gate_review,
            gate_finish: &gate_finish,
        }),
    )?;
    build_runtime_state_from_event_authority(
        context,
        derived,
        event_authority_state,
        semantic_workspace,
        Some(gate_review),
        Some(gate_finish),
        true,
    )
}

fn build_runtime_state_from_event_authority(
    context: &ExecutionContext,
    derived: ExecutionDerivedTruth,
    event_authority_state: Option<&AuthoritativeTransitionState>,
    semantic_workspace: SemanticWorkspaceSnapshot,
    precomputed_gate_review: Option<GateResult>,
    precomputed_gate_finish: Option<GateResult>,
    project_gate_checks: bool,
) -> Result<RuntimeState, JsonFailure> {
    let ExecutionDerivedTruth {
        mut status,
        overlay,
        task_review_dispatch_id,
        final_review_dispatch_authority,
    } = derived;
    let preflight = if project_gate_checks && status.execution_run_id.is_none() {
        Some(preflight_from_context(context))
    } else if project_gate_checks && status.execution_started == "no" {
        Some(GateState::default().finish())
    } else {
        None
    };
    let (gate_review, gate_finish) =
        if project_gate_checks && should_project_late_stage_gates(&status) {
            (
                Some(precomputed_gate_review.unwrap_or_else(|| gate_review_from_context(context))),
                Some(precomputed_gate_finish.unwrap_or_else(|| gate_finish_from_context(context))),
            )
        } else {
            (None, None)
        };
    let gate_snapshot = build_gate_snapshot(
        GateSnapshotBuildInputs {
            context,
            event_authority_state,
            overlay: overlay.as_ref(),
            overlay_current_branch_closure_id: overlay
                .as_ref()
                .and_then(|overlay| overlay.current_branch_closure_id.as_deref()),
            status: &status,
        },
        preflight.clone(),
        gate_review.clone(),
        gate_finish.clone(),
    )?;
    project_stale_unreviewed_closures(&mut status, &gate_snapshot);
    let fallback_gate_finish;
    let gate_finish_for_blocking_records = match gate_snapshot.gate_finish.as_ref() {
        Some(gate_finish) => gate_finish,
        None => {
            fallback_gate_finish = GateState::default().finish();
            &fallback_gate_finish
        }
    };
    status.blocking_records =
        compute_status_blocking_records(context, &status, gate_finish_for_blocking_records)?;
    for warning_code in public_repair_target_warning_codes(event_authority_state) {
        if !status
            .warning_codes
            .iter()
            .any(|existing| existing == warning_code)
        {
            status.warning_codes.push(warning_code.to_owned());
        }
    }
    let route_repair_target_candidates = public_repair_target_candidates_from_authority(
        context,
        &status,
        event_authority_state,
        None,
    );
    let usable_current_branch_closure_identity =
        usable_current_branch_closure_identity_from_authoritative_state(
            context,
            event_authority_state,
        );
    let usable_current_branch_closure_id = usable_current_branch_closure_identity
        .as_ref()
        .map(|identity| identity.branch_closure_id.clone());
    let usable_current_branch_reviewed_state_id = usable_current_branch_closure_identity
        .as_ref()
        .map(|identity| identity.reviewed_state_id.clone());
    let late_stage_bindings = current_late_stage_branch_bindings(
        event_authority_state,
        usable_current_branch_closure_id.as_deref(),
        usable_current_branch_reviewed_state_id.as_deref(),
    );
    let current_branch_closure_id = status.current_branch_closure_id.as_deref();
    let release_readiness_result_for_current_branch = release_readiness_result_for_branch_closure(
        event_authority_state,
        current_branch_closure_id,
    );
    let branch_rerecording_assessment =
        branch_closure_rerecording_assessment_with_authority(context, event_authority_state).ok();
    let base_branch = event_authority_state.and_then(|state| {
        usable_current_branch_closure_id
            .as_deref()
            .or(late_stage_bindings
                .current_final_review_branch_closure_id
                .as_deref())
            .or(late_stage_bindings.current_qa_branch_closure_id.as_deref())
            .or(late_stage_bindings
                .finish_review_gate_pass_branch_closure_id
                .as_deref())
            .and_then(|branch_closure_id| {
                state
                    .branch_closure_record(branch_closure_id)
                    .map(|record| record.base_branch)
            })
            .or_else(|| {
                state
                    .current_release_readiness_record()
                    .map(|record| record.base_branch)
            })
            .or_else(|| {
                state
                    .current_final_review_record()
                    .map(|record| record.base_branch)
            })
            .or_else(|| {
                state
                    .current_browser_qa_record()
                    .map(|record| record.base_branch)
            })
    });
    let task_review_dispatch_id = task_review_dispatch_id
        .or_else(|| current_task_review_dispatch_id_for_status(context, &status, overlay.as_ref()));
    let mut runtime_state = RuntimeState {
        context: context.clone(),
        semantic_workspace,
        status,
        route_repair_target_candidates,
        preflight,
        gate_review,
        gate_finish,
        gate_snapshot,
        base_branch,
        authoritative_current_branch_closure_id: usable_current_branch_closure_id,
        authoritative_current_branch_reviewed_state_id: usable_current_branch_reviewed_state_id,
        late_stage_bindings,
        persisted_repair_follow_up: None,
        release_readiness_result_for_current_branch,
        branch_rerecording_assessment,
        task_review_dispatch_id,
        final_review_dispatch_authority,
    };
    runtime_state.persisted_repair_follow_up = resolve_actionable_repair_follow_up(
        &runtime_state,
        &CurrentTruthSnapshot::from_authoritative_state(event_authority_state),
    )
    .map(|record| record.kind.public_token().to_owned());
    Ok(runtime_state)
}

struct GateSnapshotBuildInputs<'a> {
    context: &'a ExecutionContext,
    event_authority_state: Option<&'a AuthoritativeTransitionState>,
    overlay: Option<&'a StatusAuthoritativeOverlay>,
    overlay_current_branch_closure_id: Option<&'a str>,
    status: &'a PlanExecutionStatus,
}

fn build_gate_snapshot(
    inputs: GateSnapshotBuildInputs<'_>,
    preflight: Option<GateResult>,
    gate_review: Option<GateResult>,
    gate_finish: Option<GateResult>,
) -> Result<RuntimeGateSnapshot, JsonFailure> {
    let GateSnapshotBuildInputs {
        context,
        event_authority_state,
        overlay,
        overlay_current_branch_closure_id,
        status,
    } = inputs;
    let projection = project_authoritative_stale_targets(StaleTargetProjectionInputs {
        context,
        event_authority_state,
        overlay,
        overlay_current_branch_closure_id,
        status,
        preflight: preflight.as_ref(),
        gate_review: gate_review.as_ref(),
        gate_finish: gate_finish.as_ref(),
    })?;
    Ok(RuntimeGateSnapshot {
        preflight,
        gate_review,
        gate_finish,
        stale_reason_codes: projection.stale_reason_codes,
        stale_targets: projection.stale_targets,
        branch_closure_tracked_drift: projection.branch_closure_tracked_drift,
        late_stage_stale_unreviewed: projection.late_stage_stale_unreviewed,
        missing_current_closure_stale_provenance: projection
            .missing_current_closure_stale_provenance,
    })
}
