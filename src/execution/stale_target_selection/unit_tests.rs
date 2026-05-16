use super::*;
use crate::execution::harness::{
    AggregateEvaluationState, ChunkId, DownstreamFreshnessState, HarnessPhase,
};
use crate::execution::stale_target_projection::{
    AuthoritativeStaleTargetScope, AuthoritativeStaleTargetSource,
};
use crate::execution::status::{PublicRepairTarget, StatusBlockingRecord};

fn stale_target(
    scope: AuthoritativeStaleTargetScope,
    task: Option<u32>,
    record_id: Option<&str>,
    source: AuthoritativeStaleTargetSource,
    reason_code: &str,
) -> AuthoritativeStaleTarget {
    AuthoritativeStaleTarget {
        scope,
        task,
        step: None,
        record_id: record_id.map(str::to_owned),
        source,
        reason_code: reason_code.to_owned(),
        task_closure_bridge_allowed: false,
    }
}

fn status_blocking_record(task: u32) -> StatusBlockingRecord {
    StatusBlockingRecord {
        code: format!("task-{task}-stale"),
        scope_type: String::from("task"),
        scope_key: format!("task-{task}"),
        record_type: String::from("task_closure"),
        record_id: Some(format!("closure-task-{task}")),
        review_state_status: String::from(REVIEW_STATE_STALE_UNREVIEWED),
        required_follow_up: None,
        message: String::from("task closure is stale"),
    }
}

fn repair_target(task: u32) -> PublicRepairTarget {
    PublicRepairTarget {
        command_kind: String::from("reopen"),
        task: Some(task),
        step: Some(1),
        reason_code: String::from("test_repair_target"),
        source_record_id: None,
        expires_when_fingerprint_changes: true,
    }
}

fn projected_status() -> PlanExecutionStatus {
    PlanExecutionStatus {
        schema_version: 3,
        plan_revision: 1,
        execution_run_id: None,
        workspace_state_id: String::from("semantic_tree:test"),
        current_branch_reviewed_state_id: Some(String::from("git_tree:test")),
        current_branch_closure_id: None,
        current_branch_meaningful_drift: false,
        current_task_closures: Vec::new(),
        superseded_closures_summary: Vec::new(),
        stale_unreviewed_closures: Vec::new(),
        current_release_readiness_state: None,
        current_final_review_state: String::from("not_started"),
        current_qa_state: String::from("not_required"),
        current_final_review_branch_closure_id: None,
        current_final_review_result: None,
        current_qa_branch_closure_id: None,
        current_qa_result: None,
        qa_requirement: None,
        latest_authoritative_sequence: 1,
        phase: Some(String::from(phase::PHASE_EXECUTING)),
        harness_phase: HarnessPhase::Executing,
        chunk_id: ChunkId(String::from("chunk-test")),
        chunking_strategy: None,
        evaluator_policy: None,
        reset_policy: None,
        review_stack: None,
        active_contract_path: None,
        active_contract_fingerprint: None,
        required_evaluator_kinds: Vec::new(),
        completed_evaluator_kinds: Vec::new(),
        pending_evaluator_kinds: Vec::new(),
        non_passing_evaluator_kinds: Vec::new(),
        aggregate_evaluation_state: AggregateEvaluationState::Pending,
        last_evaluation_report_path: None,
        last_evaluation_report_fingerprint: None,
        last_evaluation_evaluator_kind: None,
        last_evaluation_verdict: None,
        current_chunk_retry_count: 0,
        current_chunk_retry_budget: 0,
        current_chunk_pivot_threshold: 0,
        handoff_required: false,
        open_failed_criteria: Vec::new(),
        write_authority_state: String::from("idle"),
        write_authority_holder: None,
        write_authority_worktree: None,
        repo_state_baseline_head_sha: None,
        repo_state_baseline_worktree_fingerprint: None,
        repo_state_drift_state: String::from("clean"),
        dependency_index_state: String::from("fresh"),
        final_review_state: DownstreamFreshnessState::Missing,
        browser_qa_state: DownstreamFreshnessState::NotRequired,
        release_docs_state: DownstreamFreshnessState::Missing,
        last_final_review_artifact_fingerprint: None,
        last_browser_qa_artifact_fingerprint: None,
        last_release_docs_artifact_fingerprint: None,
        strategy_state: String::from("ready"),
        last_strategy_checkpoint_fingerprint: None,
        strategy_checkpoint_kind: String::from("review_remediation"),
        strategy_reset_required: false,
        phase_detail: String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        review_state_status: String::from(REVIEW_STATE_STALE_UNREVIEWED),
        recording_context: None,
        execution_command_context: None,
        execution_reentry_target_source: None,
        public_repair_targets: Vec::new(),
        blocking_records: Vec::new(),
        blocking_scope: Some(String::from("task")),
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        projection_diagnostics: Vec::new(),
        state_kind: String::from("repairable_stale_state"),
        next_public_action: None,
        blockers: Vec::new(),
        runtime_provenance: None,
        semantic_workspace_tree_id: String::from("semantic_tree:test"),
        raw_workspace_tree_id: Some(String::from("git_tree:test")),
        next_action: String::from("reopen task"),
        recommended_public_command: None,
        recommended_public_command_argv: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        recommended_command: None,
        finish_review_gate_pass_branch_closure_id: None,
        reason_codes: Vec::new(),
        execution_mode: String::from("featureforge:executing-plans"),
        execution_fingerprint: String::from("fingerprint"),
        evidence_path: String::from("docs/featureforge/execution-evidence/plan-r1-evidence.md"),
        projection_mode: String::from("state_dir_only"),
        state_dir_projection_paths: Vec::new(),
        tracked_projection_paths: Vec::new(),
        tracked_projections_current: false,
        execution_started: String::from("yes"),
        warning_codes: Vec::new(),
        active_task: None,
        active_step: None,
        blocking_task: None,
        blocking_step: None,
        resume_task: None,
        resume_step: None,
    }
}

#[test]
fn earliest_task_stale_target_uses_task_then_record_then_reason() {
    let targets = [
        stale_target(
            AuthoritativeStaleTargetScope::Branch,
            None,
            Some("branch"),
            AuthoritativeStaleTargetSource::GateFinish,
            "branch_reason",
        ),
        stale_target(
            AuthoritativeStaleTargetScope::Task,
            Some(2),
            Some("record-a"),
            AuthoritativeStaleTargetSource::ClosureGraph,
            "reason-a",
        ),
        stale_target(
            AuthoritativeStaleTargetScope::Task,
            Some(1),
            Some("record-b"),
            AuthoritativeStaleTargetSource::ClosureGraph,
            "reason-a",
        ),
        stale_target(
            AuthoritativeStaleTargetScope::Task,
            Some(1),
            Some("record-a"),
            AuthoritativeStaleTargetSource::GateReview,
            "reason-z",
        ),
        stale_target(
            AuthoritativeStaleTargetScope::Task,
            Some(1),
            Some("record-a"),
            AuthoritativeStaleTargetSource::Preflight,
            "reason-a",
        ),
    ];

    let selected =
        select_earliest_task_stale_target(targets.iter()).expect("expected task stale target");

    assert_eq!(selected.task, Some(1));
    assert_eq!(selected.record_id.as_deref(), Some("record-a"));
    assert_eq!(selected.reason_code, "reason-a");
}

#[test]
fn projected_earliest_stale_task_uses_lowest_public_status_candidate() {
    let mut status = projected_status();
    status.blocking_task = Some(5);
    status.blocking_records = vec![status_blocking_record(4)];
    status.public_repair_targets = vec![repair_target(3)];

    assert_eq!(
        projected_earliest_stale_task_candidate_from_status(&status),
        Some(3)
    );
}

#[test]
fn projected_earliest_stale_task_includes_task_scoped_stale_blocking_task() {
    let mut status = projected_status();
    status.blocking_task = Some(5);

    assert_eq!(
        projected_earliest_stale_task_candidate_from_status(&status),
        Some(5)
    );
}

#[test]
fn projected_earliest_stale_task_reads_task_prefixed_stale_closure_record_ids() {
    let mut status = projected_status();
    status
        .stale_unreviewed_closures
        .push(String::from("task-1-current-closure"));

    assert_eq!(
        projected_earliest_stale_task_candidate_from_status(&status),
        Some(1)
    );
}

#[test]
fn projected_earliest_stale_task_ignores_workflow_scoped_blocking_task() {
    let mut status = projected_status();
    status.blocking_scope = Some(String::from("workflow"));
    status.blocking_task = Some(5);

    assert_eq!(
        projected_earliest_stale_task_candidate_from_status(&status),
        None
    );
}

#[test]
fn repair_plan_stale_target_prefers_task_scope_before_late_stage_fallbacks() {
    assert_eq!(
        select_repair_plan_stale_target_task(&[7, 3, 5], Some(2), Some(1)),
        Some(3)
    );
    assert_eq!(
        select_repair_plan_stale_target_task(&[], Some(2), Some(1)),
        Some(2)
    );
    assert_eq!(
        select_repair_plan_stale_target_task(&[], None, Some(1)),
        Some(1)
    );
}

#[test]
fn stale_boundary_candidate_preserves_baseline_source_when_it_preempts_authoritative_task() {
    let selected = select_earliest_stale_boundary_candidate(
        Some(StaleBoundaryCandidate::from_authoritative_stale_target(
            5,
            AuthoritativeStaleTargetSource::ClosureGraph,
        )),
        Some(4),
    )
    .expect("baseline bridge candidate should be selected");

    assert_eq!(selected.task(), 4);
    assert_eq!(
        selected.source(),
        StaleBoundaryCandidateSource::TaskClosureBaselineBridge
    );
}

#[test]
fn stale_boundary_candidate_preserves_authoritative_baseline_bridge_source() {
    let selected = select_earliest_stale_boundary_candidate(
        Some(StaleBoundaryCandidate::from_authoritative_stale_target(
            5,
            AuthoritativeStaleTargetSource::BaselineBridge,
        )),
        Some(5),
    )
    .expect("authoritative baseline bridge candidate should be selected");

    assert_eq!(selected.task(), 5);
    assert_eq!(
        selected.source(),
        StaleBoundaryCandidateSource::TaskClosureBaselineBridge
    );
}
