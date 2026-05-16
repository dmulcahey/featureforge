use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::context::ExecutionContext;
use crate::execution::fields::FIELD_HANDOFF_REQUIRED;
use crate::execution::leases::{StatusAuthoritativeOverlay, authoritative_state_path};
use crate::execution::projection_renderer::ProjectionReadModelDetail;
use crate::execution::status::{GateProjectionInputs, PlanExecutionStatus};
pub(crate) use crate::execution::status_assembly::{
    FinalReviewDispatchAuthority, StatusAssemblyFacts, compute_status_blocking_records,
    current_final_review_dispatch_authority_for_context,
    current_task_review_dispatch_id_for_status,
};
use crate::execution::status_assembly::{
    status_with_facts_from_context_with_overlay,
    status_with_facts_from_context_with_overlay_and_projection_detail,
};
use crate::execution::transitions::AuthoritativeTransitionState;

pub(crate) struct ExecutionDerivedTruth {
    pub(crate) status: PlanExecutionStatus,
    pub(crate) status_facts: StatusAssemblyFacts,
    pub(crate) overlay: Option<StatusAuthoritativeOverlay>,
    pub(crate) task_review_dispatch_id: Option<String>,
    pub(crate) final_review_dispatch_authority: FinalReviewDispatchAuthority,
}

pub(crate) fn derive_execution_truth_from_authority(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<ExecutionDerivedTruth, JsonFailure> {
    derive_execution_truth_from_authority_with_gates(context, authoritative_state, None)
}

pub(crate) fn derive_execution_truth_from_authority_with_gates(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    gate_projection: Option<GateProjectionInputs<'_>>,
) -> Result<ExecutionDerivedTruth, JsonFailure> {
    derive_execution_truth_from_authority_with_gates_and_projection_detail(
        context,
        authoritative_state,
        gate_projection,
        ProjectionReadModelDetail::Full,
    )
}

pub(crate) fn derive_execution_truth_from_authority_with_projection_detail(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    projection_detail: ProjectionReadModelDetail,
) -> Result<ExecutionDerivedTruth, JsonFailure> {
    derive_execution_truth_from_authority_with_gates_and_projection_detail(
        context,
        authoritative_state,
        None,
        projection_detail,
    )
}

pub(crate) fn derive_execution_truth_from_authority_with_gates_and_projection_detail(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    gate_projection: Option<GateProjectionInputs<'_>>,
    projection_detail: ProjectionReadModelDetail,
) -> Result<ExecutionDerivedTruth, JsonFailure> {
    let overlay = status_overlay_from_authoritative_snapshot(context, authoritative_state)?;
    let output = if projection_detail == ProjectionReadModelDetail::Full {
        status_with_facts_from_context_with_overlay(
            context,
            overlay.as_ref(),
            true,
            authoritative_state,
            true,
            gate_projection,
        )?
    } else {
        status_with_facts_from_context_with_overlay_and_projection_detail(
            context,
            overlay.as_ref(),
            true,
            authoritative_state,
            true,
            gate_projection,
            projection_detail,
        )?
    };
    let status = output.status;
    let status_facts = output.facts;
    let task_review_dispatch_id =
        current_task_review_dispatch_id_for_status(context, &status, overlay.as_ref());
    let final_review_dispatch_authority = current_final_review_dispatch_authority_for_context(
        context,
        overlay.as_ref(),
        authoritative_state,
    );
    Ok(ExecutionDerivedTruth {
        status,
        status_facts,
        overlay,
        task_review_dispatch_id,
        final_review_dispatch_authority,
    })
}

fn status_overlay_from_authoritative_snapshot(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<Option<StatusAuthoritativeOverlay>, JsonFailure> {
    authoritative_state
        .map(|state| {
            serde_json::from_value(status_overlay_payload_from_authoritative_snapshot(
                &state.state_payload_snapshot(),
            ))
            .map_err(|error| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    format!(
                        "Authoritative harness state is malformed in {}: {error}",
                        authoritative_state_path(context).display()
                    ),
                )
            })
        })
        .transpose()
}

fn status_overlay_payload_from_authoritative_snapshot(
    snapshot: &serde_json::Value,
) -> serde_json::Value {
    let Some(source) = snapshot.as_object() else {
        return serde_json::Value::Object(serde_json::Map::new());
    };
    let mut overlay = serde_json::Map::new();
    for field in [
        "harness_phase",
        "chunk_id",
        "latest_authoritative_sequence",
        "authoritative_sequence",
        "active_contract_path",
        "active_contract_fingerprint",
        "required_evaluator_kinds",
        "completed_evaluator_kinds",
        "pending_evaluator_kinds",
        "non_passing_evaluator_kinds",
        "aggregate_evaluation_state",
        "last_evaluation_report_path",
        "last_evaluation_report_fingerprint",
        "last_evaluation_evaluator_kind",
        "last_evaluation_verdict",
        "current_chunk_retry_count",
        "current_chunk_retry_budget",
        "current_chunk_pivot_threshold",
        FIELD_HANDOFF_REQUIRED,
        "open_failed_criteria",
        "write_authority_state",
        "write_authority_holder",
        "write_authority_worktree",
        "repo_state_baseline_head_sha",
        "repo_state_baseline_worktree_fingerprint",
        "repo_state_drift_state",
        "dependency_index_state",
        "final_review_state",
        "browser_qa_state",
        "release_docs_state",
        "last_final_review_artifact_fingerprint",
        "last_browser_qa_artifact_fingerprint",
        "last_release_docs_artifact_fingerprint",
        "strategy_state",
        "last_strategy_checkpoint_fingerprint",
        "strategy_checkpoint_kind",
        "strategy_cycle_break_task",
        "strategy_cycle_break_step",
        "strategy_cycle_break_checkpoint_fingerprint",
        "strategy_reset_required",
        "strategy_review_dispatch_lineage",
        "final_review_dispatch_lineage",
        "current_branch_closure_id",
        "current_branch_closure_reviewed_state_id",
        "current_branch_closure_contract_identity",
        "current_release_readiness_result",
        "reason_codes",
    ] {
        if let Some(value) = source.get(field)
            && !value.is_null()
        {
            overlay.insert(field.to_owned(), value.clone());
        }
    }
    serde_json::Value::Object(overlay)
}
