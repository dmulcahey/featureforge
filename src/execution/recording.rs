//! Execution-owned recording services for authoritative reviewed-closure writes.
//!
//! intent adapters should delegate authoritative writes here so mutation orchestration
//! stays separate from workflow routing, artifact rendering, and CLI phrasing.

use std::path::PathBuf;

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::final_review::parse_artifact_document;
use crate::execution::state::{ExecutionContext, ExecutionRuntime};
use crate::execution::transitions::{
    AuthoritativeTransitionState, FinalReviewResultRecord, TaskClosureNegativeResultRecord,
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

pub(crate) struct FinalReviewWrite<'a> {
    pub(crate) branch_closure_id: &'a str,
    pub(crate) dispatch_id: &'a str,
    pub(crate) reviewer_source: &'a str,
    pub(crate) reviewer_id: &'a str,
    pub(crate) result: &'a str,
    pub(crate) final_review_fingerprint: Option<&'a str>,
    pub(crate) browser_qa_required: Option<bool>,
    pub(crate) summary_hash: &'a str,
}

pub(crate) fn record_current_task_closure(
    authoritative_state: &mut AuthoritativeTransitionState,
    input: CurrentTaskClosureWrite<'_>,
) -> Result<(), JsonFailure> {
    authoritative_state.remove_current_task_closure_results(input.superseded_tasks.iter().copied())?;
    authoritative_state
        .append_superseded_task_closure_ids(input.superseded_task_closure_ids.iter().map(String::as_str))?;
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
    branch_closure_id: &str,
    reviewed_state_id: &str,
    contract_identity: &str,
    superseded_branch_closure_ids: &[String],
) -> Result<(), JsonFailure> {
    authoritative_state.record_branch_closure(
        branch_closure_id,
        reviewed_state_id,
        contract_identity,
    )?;
    authoritative_state.append_superseded_branch_closure_ids(
        superseded_branch_closure_ids.iter().map(String::as_str),
    )?;
    authoritative_state.set_current_branch_closure_id(
        branch_closure_id,
        reviewed_state_id,
        contract_identity,
    )?;
    authoritative_state.persist_if_dirty_with_failpoint(None)
}

pub(crate) fn record_release_readiness(
    authoritative_state: &mut AuthoritativeTransitionState,
    result: &str,
    release_docs_fingerprint: Option<&str>,
    summary_hash: &str,
) -> Result<(), JsonFailure> {
    authoritative_state.record_release_readiness_result(
        result,
        release_docs_fingerprint,
        summary_hash,
    )?;
    authoritative_state.persist_if_dirty_with_failpoint(None)
}

pub(crate) fn record_final_review(
    authoritative_state: &mut AuthoritativeTransitionState,
    input: FinalReviewWrite<'_>,
) -> Result<(), JsonFailure> {
    authoritative_state.record_final_review_result(FinalReviewResultRecord {
        branch_closure_id: input.branch_closure_id,
        dispatch_id: input.dispatch_id,
        reviewer_source: input.reviewer_source,
        reviewer_id: input.reviewer_id,
        result: input.result,
        final_review_fingerprint: input.final_review_fingerprint,
        browser_qa_required: input.browser_qa_required,
        summary_hash: input.summary_hash,
    })?;
    authoritative_state.persist_if_dirty_with_failpoint(None)
}

pub(crate) fn record_browser_qa(
    authoritative_state: &mut AuthoritativeTransitionState,
    branch_closure_id: &str,
    result: &str,
    browser_qa_fingerprint: Option<&str>,
    summary_hash: &str,
) -> Result<(), JsonFailure> {
    authoritative_state.record_browser_qa_result(
        branch_closure_id,
        result,
        browser_qa_fingerprint,
        summary_hash,
    )?;
    authoritative_state.persist_if_dirty_with_failpoint(None)
}

pub(crate) fn restore_current_branch_closure_overlay(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    branch_closure_id: &str,
    reviewed_state_id: &str,
    contract_identity: &str,
) -> Result<bool, JsonFailure> {
    let _write_authority = claim_step_write_authority(runtime)?;
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "reconcile-review-state requires authoritative harness state.",
        ));
    };
    authoritative_state.record_branch_closure(
        branch_closure_id,
        reviewed_state_id,
        contract_identity,
    )?;
    let restored = authoritative_state.restore_current_branch_closure_overlay_fields_if_current(
        branch_closure_id,
        reviewed_state_id,
        contract_identity,
    )?;
    if !restored {
        return Ok(false);
    }
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(true)
}

pub(crate) fn resolve_branch_closure_identity(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    branch_closure_id: &str,
) -> Result<Option<(String, String)>, JsonFailure> {
    if let Some(authoritative_state) = load_authoritative_transition_state(context)?
        && let Some(record) = authoritative_state.branch_closure_record(branch_closure_id)
    {
        return Ok(Some((record.reviewed_state_id, record.contract_identity)));
    }

    let artifact_path = branch_closure_artifact_path(runtime, branch_closure_id);
    if !artifact_path.is_file() {
        return Ok(None);
    }
    let document = parse_artifact_document(&artifact_path);
    let Some(reviewed_state_id) = document.headers.get("Current Reviewed State ID").cloned() else {
        return Ok(None);
    };
    let Some(contract_identity) = document.headers.get("Contract Identity").cloned() else {
        return Ok(None);
    };
    Ok(Some((reviewed_state_id, contract_identity)))
}

fn branch_closure_artifact_path(runtime: &ExecutionRuntime, branch_closure_id: &str) -> PathBuf {
    runtime
        .state_dir
        .join("projects")
        .join(&runtime.repo_slug)
        .join(format!("branch-closure-{branch_closure_id}.md"))
}
