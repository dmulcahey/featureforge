//! Execution-owned recording services for authoritative reviewed-closure writes.
//!
//! intent adapters should delegate authoritative writes here so mutation orchestration
//! stays separate from workflow routing, artifact rendering, and CLI phrasing.

use std::collections::BTreeSet;

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::current_truth::current_late_stage_branch_bindings as shared_current_late_stage_branch_bindings;
use crate::execution::state::{
    ExecutionContext, ExecutionRuntime, validated_current_branch_closure_identity,
};
use crate::execution::transitions::{
    AuthoritativeTransitionState, BranchClosureResultRecord, BrowserQaResultRecord,
    FinalReviewMilestoneRecord, ReleaseReadinessResultRecord, TaskClosureNegativeResultRecord,
    TaskClosureResultRecord, claim_step_write_authority, load_authoritative_transition_state,
};

pub(crate) struct CurrentTaskClosureWrite<'a> {
    pub(crate) task: u32,
    pub(crate) dispatch_id: &'a str,
    pub(crate) closure_record_id: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) contract_identity: &'a str,
    pub(crate) effective_reviewed_surface_paths: &'a [String],
    pub(crate) review_result: &'a str,
    pub(crate) review_summary_hash: &'a str,
    pub(crate) verification_result: &'a str,
    pub(crate) verification_summary_hash: &'a str,
    pub(crate) superseded_tasks: &'a [u32],
    pub(crate) superseded_task_closure_ids: &'a [String],
}

pub(crate) struct NegativeTaskClosureWrite<'a> {
    pub(crate) task: u32,
    pub(crate) dispatch_id: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) contract_identity: &'a str,
    pub(crate) review_result: &'a str,
    pub(crate) review_summary_hash: &'a str,
    pub(crate) verification_result: &'a str,
    pub(crate) verification_summary_hash: &'a str,
}

pub(crate) struct BranchClosureWrite<'a> {
    pub(crate) branch_closure_id: &'a str,
    pub(crate) source_plan_path: &'a str,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: &'a str,
    pub(crate) branch_name: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) contract_identity: &'a str,
    pub(crate) effective_reviewed_branch_surface: &'a str,
    pub(crate) source_task_closure_ids: &'a [String],
    pub(crate) provenance_basis: &'a str,
    pub(crate) closure_status: &'a str,
    pub(crate) superseded_branch_closure_ids: &'a [String],
    pub(crate) branch_closure_fingerprint: Option<&'a str>,
}

pub(crate) struct ReleaseReadinessWrite<'a> {
    pub(crate) branch_closure_id: &'a str,
    pub(crate) source_plan_path: &'a str,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: &'a str,
    pub(crate) branch_name: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) result: &'a str,
    pub(crate) release_docs_fingerprint: Option<&'a str>,
    pub(crate) summary: &'a str,
    pub(crate) summary_hash: &'a str,
    pub(crate) generated_by_identity: &'a str,
}

pub(crate) struct FinalReviewWrite<'a> {
    pub(crate) branch_closure_id: &'a str,
    pub(crate) release_readiness_record_id: &'a str,
    pub(crate) dispatch_id: &'a str,
    pub(crate) reviewer_source: &'a str,
    pub(crate) reviewer_id: &'a str,
    pub(crate) result: &'a str,
    pub(crate) final_review_fingerprint: Option<&'a str>,
    pub(crate) deviations_required: Option<bool>,
    pub(crate) browser_qa_required: Option<bool>,
    pub(crate) source_plan_path: &'a str,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: &'a str,
    pub(crate) branch_name: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) summary: &'a str,
    pub(crate) summary_hash: &'a str,
}

pub(crate) struct BrowserQaWrite<'a> {
    pub(crate) branch_closure_id: &'a str,
    pub(crate) final_review_record_id: &'a str,
    pub(crate) source_plan_path: &'a str,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: &'a str,
    pub(crate) branch_name: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) result: &'a str,
    pub(crate) browser_qa_fingerprint: Option<&'a str>,
    pub(crate) source_test_plan_fingerprint: Option<&'a str>,
    pub(crate) summary: &'a str,
    pub(crate) summary_hash: &'a str,
    pub(crate) generated_by_identity: &'a str,
}

pub(crate) fn record_current_task_closure(
    authoritative_state: &mut AuthoritativeTransitionState,
    input: CurrentTaskClosureWrite<'_>,
) -> Result<(), JsonFailure> {
    authoritative_state
        .remove_current_task_closure_results(input.superseded_tasks.iter().copied())?;
    authoritative_state.clear_task_closure_negative_result(input.task)?;
    authoritative_state.append_superseded_task_closure_ids(
        input.superseded_task_closure_ids.iter().map(String::as_str),
    )?;
    authoritative_state.record_task_closure_result(TaskClosureResultRecord {
        task: input.task,
        dispatch_id: input.dispatch_id,
        closure_record_id: input.closure_record_id,
        reviewed_state_id: input.reviewed_state_id,
        contract_identity: input.contract_identity,
        effective_reviewed_surface_paths: input.effective_reviewed_surface_paths,
        review_result: input.review_result,
        review_summary_hash: input.review_summary_hash,
        verification_result: input.verification_result,
        verification_summary_hash: input.verification_summary_hash,
    })?;
    authoritative_state.persist_if_dirty_with_failpoint(None)
}

pub(crate) fn record_negative_task_closure(
    authoritative_state: &mut AuthoritativeTransitionState,
    input: NegativeTaskClosureWrite<'_>,
) -> Result<(), JsonFailure> {
    authoritative_state.record_task_closure_negative_result(TaskClosureNegativeResultRecord {
        task: input.task,
        dispatch_id: input.dispatch_id,
        reviewed_state_id: input.reviewed_state_id,
        contract_identity: input.contract_identity,
        review_result: input.review_result,
        review_summary_hash: input.review_summary_hash,
        verification_result: input.verification_result,
        verification_summary_hash: input.verification_summary_hash,
    })?;
    authoritative_state.persist_if_dirty_with_failpoint(None)
}

pub(crate) fn record_current_branch_closure(
    authoritative_state: &mut AuthoritativeTransitionState,
    input: BranchClosureWrite<'_>,
) -> Result<(), JsonFailure> {
    authoritative_state.record_branch_closure(BranchClosureResultRecord {
        branch_closure_id: input.branch_closure_id,
        source_plan_path: input.source_plan_path,
        source_plan_revision: input.source_plan_revision,
        repo_slug: input.repo_slug,
        branch_name: input.branch_name,
        base_branch: input.base_branch,
        reviewed_state_id: input.reviewed_state_id,
        contract_identity: input.contract_identity,
        effective_reviewed_branch_surface: input.effective_reviewed_branch_surface,
        source_task_closure_ids: input.source_task_closure_ids,
        provenance_basis: input.provenance_basis,
        closure_status: input.closure_status,
        superseded_branch_closure_ids: input.superseded_branch_closure_ids,
        branch_closure_fingerprint: input.branch_closure_fingerprint,
    })?;
    authoritative_state.append_superseded_branch_closure_ids(
        input
            .superseded_branch_closure_ids
            .iter()
            .map(String::as_str),
    )?;
    authoritative_state.set_current_branch_closure_id(
        input.branch_closure_id,
        input.reviewed_state_id,
        input.contract_identity,
    )?;
    authoritative_state.persist_if_dirty_with_failpoint(None)
}

pub(crate) fn record_release_readiness(
    authoritative_state: &mut AuthoritativeTransitionState,
    input: ReleaseReadinessWrite<'_>,
) -> Result<(), JsonFailure> {
    authoritative_state.record_release_readiness_result(ReleaseReadinessResultRecord {
        branch_closure_id: input.branch_closure_id,
        source_plan_path: input.source_plan_path,
        source_plan_revision: input.source_plan_revision,
        repo_slug: input.repo_slug,
        branch_name: input.branch_name,
        base_branch: input.base_branch,
        reviewed_state_id: input.reviewed_state_id,
        result: input.result,
        release_docs_fingerprint: input.release_docs_fingerprint,
        summary: input.summary,
        summary_hash: input.summary_hash,
        generated_by_identity: input.generated_by_identity,
    })?;
    authoritative_state.persist_if_dirty_with_failpoint(None)
}

pub(crate) fn record_final_review(
    authoritative_state: &mut AuthoritativeTransitionState,
    input: FinalReviewWrite<'_>,
) -> Result<(), JsonFailure> {
    authoritative_state.record_final_review_result(FinalReviewMilestoneRecord {
        branch_closure_id: input.branch_closure_id,
        release_readiness_record_id: input.release_readiness_record_id,
        dispatch_id: input.dispatch_id,
        reviewer_source: input.reviewer_source,
        reviewer_id: input.reviewer_id,
        result: input.result,
        final_review_fingerprint: input.final_review_fingerprint,
        deviations_required: input.deviations_required,
        browser_qa_required: input.browser_qa_required,
        source_plan_path: input.source_plan_path,
        source_plan_revision: input.source_plan_revision,
        repo_slug: input.repo_slug,
        branch_name: input.branch_name,
        base_branch: input.base_branch,
        reviewed_state_id: input.reviewed_state_id,
        summary: input.summary,
        summary_hash: input.summary_hash,
    })?;
    authoritative_state.persist_if_dirty_with_failpoint(None)
}

pub(crate) fn record_browser_qa(
    authoritative_state: &mut AuthoritativeTransitionState,
    input: BrowserQaWrite<'_>,
) -> Result<(), JsonFailure> {
    authoritative_state.record_browser_qa_result(BrowserQaResultRecord {
        branch_closure_id: input.branch_closure_id,
        final_review_record_id: input.final_review_record_id,
        source_plan_path: input.source_plan_path,
        source_plan_revision: input.source_plan_revision,
        repo_slug: input.repo_slug,
        branch_name: input.branch_name,
        base_branch: input.base_branch,
        reviewed_state_id: input.reviewed_state_id,
        result: input.result,
        browser_qa_fingerprint: input.browser_qa_fingerprint,
        source_test_plan_fingerprint: input.source_test_plan_fingerprint,
        summary: input.summary,
        summary_hash: input.summary_hash,
        generated_by_identity: input.generated_by_identity,
    })?;
    authoritative_state.persist_if_dirty_with_failpoint(None)
}

pub(crate) fn clear_current_branch_closure_for_structural_repair(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(false);
    };
    if !authoritative_state.clear_current_branch_closure_for_structural_repair()? {
        return Ok(false);
    }
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(true)
}

pub(crate) fn restore_review_state_projection_overlays(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
) -> Result<Vec<String>, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "reconcile-review-state requires authoritative harness state.",
        ));
    };

    let mut actions_performed = Vec::new();
    let mut push_action = |action: &str| {
        if !actions_performed.iter().any(|existing| existing == action) {
            actions_performed.push(action.to_owned());
        }
    };
    if authoritative_state.restore_current_task_closure_records_from_history()? {
        push_action("restored_current_task_closure_records");
    }
    if authoritative_state.restore_task_closure_negative_result_records_from_history()? {
        push_action("restored_task_closure_negative_result_records");
    }

    let current_branch_identity = validated_current_branch_closure_identity(context);
    if let Some(current_identity) = current_branch_identity.as_ref() {
        let branch_id_changed = authoritative_state
            .current_branch_closure_overlay_id()
            .as_deref()
            != Some(current_identity.branch_closure_id.as_str());
        let reviewed_state_changed = authoritative_state
            .current_branch_closure_overlay_reviewed_state_id()
            .as_deref()
            != Some(current_identity.reviewed_state_id.as_str());
        let contract_identity_changed = authoritative_state
            .current_branch_closure_overlay_contract_identity()
            .as_deref()
            != Some(current_identity.contract_identity.as_str());
        if branch_id_changed || reviewed_state_changed || contract_identity_changed {
            if branch_id_changed {
                push_action("restored_current_branch_closure_id");
            }
            if reviewed_state_changed {
                push_action("restored_current_branch_closure_reviewed_state");
            }
            if contract_identity_changed {
                push_action("restored_current_branch_closure_contract_identity");
            }
            authoritative_state.restore_current_branch_closure_overlay_fields(
                &current_identity.branch_closure_id,
                &current_identity.reviewed_state_id,
                &current_identity.contract_identity,
            )?;
        }
    }
    let late_stage_bindings = shared_current_late_stage_branch_bindings(
        Some(authoritative_state),
        current_branch_identity
            .as_ref()
            .map(|identity| identity.branch_closure_id.as_str()),
        current_branch_identity
            .as_ref()
            .map(|identity| identity.reviewed_state_id.as_str()),
    );

    let current_release_readiness_record_id =
        late_stage_bindings.current_release_readiness_record_id.clone();
    if authoritative_state
        .current_release_readiness_record_id()
        .as_deref()
        != current_release_readiness_record_id.as_deref()
    {
        authoritative_state.set_current_release_readiness_record_id_cache(
            current_release_readiness_record_id.as_deref(),
        )?;
        push_action("restored_current_release_readiness_overlay");
    }
    if current_release_readiness_record_id.is_some()
        && authoritative_state.restore_current_release_readiness_overlay_fields()?
    {
        push_action("restored_current_release_readiness_overlay");
    }

    let current_final_review_record_id = late_stage_bindings.current_final_review_record_id.clone();
    if authoritative_state
        .current_final_review_record_id()
        .as_deref()
        != current_final_review_record_id.as_deref()
    {
        authoritative_state
            .set_current_final_review_record_id_cache(current_final_review_record_id.as_deref())?;
        push_action("restored_current_final_review_overlay");
    }
    if current_final_review_record_id.is_some()
        && authoritative_state.restore_current_final_review_overlay_fields()?
    {
        push_action("restored_current_final_review_overlay");
    }

    let current_qa_record_id = late_stage_bindings.current_qa_record_id.clone();
    if authoritative_state.current_qa_record_id().as_deref() != current_qa_record_id.as_deref() {
        authoritative_state.set_current_qa_record_id_cache(current_qa_record_id.as_deref())?;
        push_action("restored_current_browser_qa_overlay");
    }
    if current_qa_record_id.is_some()
        && authoritative_state.restore_current_browser_qa_overlay_fields()?
    {
        push_action("restored_current_browser_qa_overlay");
    }
    if actions_performed.is_empty() {
        return Ok(actions_performed);
    }

    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(actions_performed)
}

pub(crate) fn clear_task_review_dispatch_lineage_for_execution_reentry(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    task_number: u32,
) -> Result<bool, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(false);
    };
    if !authoritative_state.clear_task_review_dispatch_lineage(task_number)? {
        return Ok(false);
    }
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(true)
}

pub(crate) fn clear_task_review_dispatch_lineage_for_structural_repair(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    task_number: u32,
) -> Result<bool, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(false);
    };
    if !authoritative_state.clear_task_review_dispatch_lineage_for_structural_repair(task_number)? {
        return Ok(false);
    }
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(true)
}

pub(crate) fn clear_current_task_closure_results_for_execution_reentry(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    tasks: impl IntoIterator<Item = u32>,
) -> Result<Vec<u32>, JsonFailure> {
    let tasks = tasks.into_iter().collect::<Vec<_>>();
    if tasks.is_empty() {
        return Ok(Vec::new());
    }
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(Vec::new());
    };
    let cleared_tasks = tasks
        .into_iter()
        .filter(|task| {
            authoritative_state
                .raw_current_task_closure_state_entry(*task)
                .is_some()
                || authoritative_state
                    .current_task_closure_result(*task)
                    .is_some()
        })
        .collect::<Vec<_>>();
    if cleared_tasks.is_empty() {
        return Ok(Vec::new());
    }
    authoritative_state
        .clear_current_task_closure_results_for_execution_reentry(cleared_tasks.iter().copied())?;
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(cleared_tasks)
}

pub(crate) fn clear_current_task_closure_results_for_structural_repair(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    tasks: impl IntoIterator<Item = u32>,
) -> Result<Vec<u32>, JsonFailure> {
    let tasks = tasks.into_iter().collect::<Vec<_>>();
    if tasks.is_empty() {
        return Ok(Vec::new());
    }
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(Vec::new());
    };
    let cleared_tasks = tasks
        .into_iter()
        .filter(|task| {
            authoritative_state
                .raw_current_task_closure_state_entry(*task)
                .is_some()
                || authoritative_state
                    .current_task_closure_result(*task)
                    .is_some()
        })
        .collect::<Vec<_>>();
    if cleared_tasks.is_empty() {
        return Ok(Vec::new());
    }
    authoritative_state
        .clear_current_task_closure_results_for_structural_repair(cleared_tasks.iter().copied())?;
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(cleared_tasks)
}

pub(crate) fn clear_current_task_closure_results_for_structural_repair_scope_keys(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    scope_keys: impl IntoIterator<Item = String>,
) -> Result<Vec<String>, JsonFailure> {
    let scope_keys = scope_keys.into_iter().collect::<Vec<_>>();
    if scope_keys.is_empty() {
        return Ok(Vec::new());
    }
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(Vec::new());
    };
    let available_scope_keys = authoritative_state
        .raw_current_task_closure_state_entries()
        .into_iter()
        .map(|entry| entry.scope_key)
        .collect::<BTreeSet<_>>();
    let cleared_scope_keys = scope_keys
        .into_iter()
        .filter(|scope_key| available_scope_keys.contains(scope_key))
        .collect::<Vec<_>>();
    if cleared_scope_keys.is_empty() {
        return Ok(Vec::new());
    }
    authoritative_state.clear_current_task_closure_results_for_structural_repair_scope_keys(
        cleared_scope_keys.clone(),
    )?;
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(cleared_scope_keys)
}

pub(crate) fn persist_review_state_repair_follow_up(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    follow_up: Option<&str>,
) -> Result<(), JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(());
    };
    authoritative_state.set_review_state_repair_follow_up(follow_up)?;
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(())
}
