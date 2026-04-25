use std::collections::BTreeMap;

use serde::Serialize;

use crate::diagnostics::JsonFailure;
use crate::execution::current_truth::{
    BranchRerecordingAssessment, CurrentLateStageBranchBindings,
    branch_closure_rerecording_assessment_with_authority, current_late_stage_branch_bindings,
    release_readiness_result_for_branch_closure,
};
use crate::execution::follow_up::normalize_persisted_repair_follow_up_token;
use crate::execution::semantic_identity::{SemanticWorkspaceSnapshot, semantic_workspace_snapshot};
use crate::execution::state::{
    ExecutionContext, ExecutionDerivedTruth, ExecutionReadScope, FinalReviewDispatchAuthority,
    GateResult, GateState, PlanExecutionStatus, derive_execution_truth_from_authority,
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
    pub(crate) preflight: Option<GateResult>,
    pub(crate) gate_review: Option<GateResult>,
    pub(crate) gate_finish: Option<GateResult>,
    pub(crate) base_branch: Option<String>,
    pub(crate) authoritative_current_branch_closure_id: Option<String>,
    pub(crate) authoritative_current_branch_reviewed_state_id: Option<String>,
    pub(crate) late_stage_bindings: CurrentLateStageBranchBindings,
    pub(crate) persisted_repair_follow_up: Option<String>,
    pub(crate) release_readiness_result_for_current_branch: Option<String>,
    #[serde(skip)]
    pub(crate) branch_rerecording_assessment: Option<BranchRerecordingAssessment>,
    #[serde(skip)]
    pub(crate) task_closure_execution_run_ids: BTreeMap<u32, String>,
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
        false,
    )
}

pub(crate) fn reduce_runtime_state(
    context: &ExecutionContext,
    event_authority_state: Option<&AuthoritativeTransitionState>,
    semantic_workspace: SemanticWorkspaceSnapshot,
) -> Result<RuntimeState, JsonFailure> {
    let derived = derive_execution_truth_from_authority(context, event_authority_state)?;
    build_runtime_state_from_event_authority(
        context,
        derived,
        event_authority_state,
        semantic_workspace,
        true,
    )
}

fn build_runtime_state_from_event_authority(
    context: &ExecutionContext,
    derived: ExecutionDerivedTruth,
    event_authority_state: Option<&AuthoritativeTransitionState>,
    semantic_workspace: SemanticWorkspaceSnapshot,
    project_gate_checks: bool,
) -> Result<RuntimeState, JsonFailure> {
    let ExecutionDerivedTruth {
        status,
        overlay: _overlay,
        task_review_dispatch_id,
        final_review_dispatch_authority,
    } = derived;
    let preflight = if project_gate_checks && status.execution_started == "no" {
        Some(if status.execution_run_id.is_some() {
            GateState::default().finish()
        } else {
            preflight_from_context(context)
        })
    } else {
        None
    };
    let (gate_review, gate_finish) =
        if project_gate_checks && should_project_late_stage_gates(&status) {
            (
                Some(gate_review_from_context(context)),
                Some(gate_finish_from_context(context)),
            )
        } else {
            (None, None)
        };
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
    let persisted_repair_follow_up = event_authority_state
        .and_then(|state| {
            normalize_persisted_repair_follow_up_token(state.review_state_repair_follow_up())
        })
        .map(str::to_owned);
    let release_readiness_result_for_current_branch = release_readiness_result_for_branch_closure(
        event_authority_state,
        current_branch_closure_id,
    );
    let branch_rerecording_assessment =
        branch_closure_rerecording_assessment_with_authority(context, event_authority_state).ok();
    let task_closure_execution_run_ids = event_authority_state
        .map(|state| {
            state
                .current_task_closure_results()
                .into_iter()
                .filter_map(|(task, record)| record.execution_run_id.map(|run_id| (task, run_id)))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
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
    Ok(RuntimeState {
        context: context.clone(),
        semantic_workspace,
        status,
        preflight,
        gate_review,
        gate_finish,
        base_branch,
        authoritative_current_branch_closure_id: usable_current_branch_closure_id,
        authoritative_current_branch_reviewed_state_id: usable_current_branch_reviewed_state_id,
        late_stage_bindings,
        persisted_repair_follow_up,
        release_readiness_result_for_current_branch,
        branch_rerecording_assessment,
        task_closure_execution_run_ids,
        task_review_dispatch_id,
        final_review_dispatch_authority,
    })
}
