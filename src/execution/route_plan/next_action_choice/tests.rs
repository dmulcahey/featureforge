use super::{
    AuthoritativeStaleReentryTarget, NEXT_ACTION_EXECUTION_REENTRY_REQUIRED,
    NextActionAuthorityInputs, NextActionKind, NextActionRequestInputs,
    compute_next_action_decision_with_authority_inputs,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use crate::contracts::plan::{PlanDocument, PlanStep, PlanTask, TaskFileEntry};
use crate::execution::context::EvidenceSourceOrigin;
use crate::execution::current_truth::REASON_BRANCH_DRIFT_ESCAPES_LATE_STAGE_SURFACE;
use crate::execution::harness::{
    AggregateEvaluationState, ChunkId, DownstreamFreshnessState, HarnessPhase,
};
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::runtime::ExecutionRuntime;
use crate::execution::stale_target_projection::AuthoritativeStaleTargetSource;
use crate::execution::state::{
    EvidenceAttempt, EvidenceFormat, ExecutionContext, ExecutionEvidence, PlanExecutionStatus,
    PlanStepState, PublicRepairTarget,
};
use crate::execution::status::{PublicExecutionCommandContext, PublicReviewStateTaskClosure};

fn test_runtime(root: &Path) -> ExecutionRuntime {
    ExecutionRuntime {
        repo_root: root.to_path_buf(),
        git_dir: root.join(".git"),
        branch_name: String::from("feature/test"),
        repo_slug: String::from("featureforge"),
        safe_branch: String::from("feature-test"),
        state_dir: root.join("state"),
    }
}

fn test_task(number: u32, step_number: u32) -> PlanTask {
    PlanTask {
        number,
        title: format!("Task {number}"),
        spec_coverage: vec![String::from("DR-TEST")],
        goal: String::from("Exercise routing behavior."),
        context: Vec::new(),
        constraints: Vec::new(),
        done_when: Vec::new(),
        files: vec![TaskFileEntry {
            action: String::from("Modify"),
            path: format!("src/task-{number}.rs"),
        }],
        steps: vec![PlanStep {
            number: step_number,
            text: format!("Step {step_number}"),
        }],
    }
}

fn test_context(root: &Path) -> ExecutionContext {
    let task4 = test_task(4, 4);
    let task5 = test_task(5, 3);
    let tasks_by_number = [(4, task4.clone()), (5, task5.clone())]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    ExecutionContext {
        runtime: test_runtime(root),
        plan_rel: String::from("docs/featureforge/plans/plan.md"),
        plan_abs: root.join("docs/featureforge/plans/plan.md"),
        plan_document: PlanDocument {
            path: String::from("docs/featureforge/plans/plan.md"),
            workflow_state: String::from("Engineering Approved"),
            plan_revision: 1,
            execution_mode: String::from("featureforge:executing-plans"),
            source_spec_path: String::from("docs/featureforge/specs/spec.md"),
            source_spec_revision: 1,
            last_reviewed_by: String::from("plan-eng-review"),
            qa_requirement: None,
            coverage_matrix: BTreeMap::new(),
            tasks: vec![task4, task5],
            source: String::new(),
        },
        plan_source: String::new(),
        steps: vec![
            PlanStepState {
                task_number: 4,
                step_number: 4,
                title: String::from("Step 4"),
                checked: false,
                note_state: None,
                note_summary: String::new(),
            },
            PlanStepState {
                task_number: 5,
                step_number: 3,
                title: String::from("Step 3"),
                checked: true,
                note_state: None,
                note_summary: String::new(),
            },
        ],
        local_execution_progress_markers_present: false,
        legacy_open_step_projection_present: false,
        tasks_by_number,
        evidence_rel: String::from("docs/featureforge/execution-evidence/plan-r1-evidence.md"),
        evidence_abs: root.join("docs/featureforge/execution-evidence/plan-r1-evidence.md"),
        evidence: ExecutionEvidence {
            format: EvidenceFormat::Empty,
            plan_path: String::from("docs/featureforge/plans/plan.md"),
            plan_revision: 1,
            plan_fingerprint: None,
            source_spec_path: String::from("docs/featureforge/specs/spec.md"),
            source_spec_revision: 1,
            source_spec_fingerprint: None,
            attempts: vec![EvidenceAttempt {
                task_number: 5,
                step_number: 3,
                attempt_number: 1,
                status: String::from("Completed"),
                recorded_at: String::from("2026-05-04T00:00:00Z"),
                execution_source: String::from("featureforge:executing-plans"),
                claim: String::from("Task 5 Step 3 was attempted."),
                files: Vec::new(),
                file_proofs: Vec::new(),
                verify_command: None,
                verification_summary: String::from("verified"),
                invalidation_reason: String::new(),
                packet_fingerprint: None,
                head_sha: None,
                base_sha: None,
                source_contract_path: None,
                source_contract_fingerprint: None,
                source_evaluation_report_fingerprint: None,
                evaluator_verdict: None,
                failing_criterion_ids: Vec::new(),
                source_handoff_fingerprint: None,
                repo_state_baseline_head_sha: None,
                repo_state_baseline_worktree_fingerprint: None,
            }],
            source: None,
            source_origin: EvidenceSourceOrigin::Empty,
            tracked_progress_present: false,
            tracked_source: None,
        },
        authoritative_evidence_projection_fingerprint: None,
        source_spec_source: String::new(),
        source_spec_path: root.join("docs/featureforge/specs/spec.md"),
        execution_fingerprint: String::from("fingerprint"),
        tracked_tree_sha_cache: OnceLock::new(),
        semantic_workspace_snapshot_cache: OnceLock::new(),
        reviewed_tree_sha_cache: RefCell::new(BTreeMap::new()),
        head_sha_cache: OnceLock::new(),
        release_base_branch_cache: OnceLock::new(),
        tracked_worktree_changes_excluding_execution_evidence_cache: OnceLock::new(),
    }
}

fn status_with_earlier_resume_and_later_stale_target() -> PlanExecutionStatus {
    PlanExecutionStatus {
        schema_version: 3,
        plan_revision: 1,
        execution_run_id: None,
        workspace_state_id: String::from("semantic_tree:test"),
        current_branch_reviewed_state_id: None,
        current_branch_closure_id: None,
        current_branch_meaningful_drift: false,
        current_task_closures: Vec::new(),
        superseded_closures_summary: Vec::new(),
        stale_unreviewed_closures: vec![String::from("task-closure-5")],
        current_release_readiness_state: None,
        current_final_review_state: String::from("not_required"),
        current_qa_state: String::from("not_required"),
        current_final_review_branch_closure_id: None,
        current_final_review_result: None,
        current_qa_branch_closure_id: None,
        current_qa_result: None,
        qa_requirement: None,
        latest_authoritative_sequence: 1,
        phase: Some(String::from(crate::execution::phase::PHASE_EXECUTING)),
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
        repo_state_drift_state: String::from("dirty"),
        dependency_index_state: String::from("fresh"),
        final_review_state: DownstreamFreshnessState::NotRequired,
        browser_qa_state: DownstreamFreshnessState::NotRequired,
        release_docs_state: DownstreamFreshnessState::NotRequired,
        last_final_review_artifact_fingerprint: None,
        last_browser_qa_artifact_fingerprint: None,
        last_release_docs_artifact_fingerprint: None,
        strategy_state: String::from("checkpoint_missing"),
        last_strategy_checkpoint_fingerprint: None,
        strategy_checkpoint_kind: String::from("none"),
        strategy_reset_required: false,
        phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        review_state_status: String::from(
            crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
        ),
        recording_context: None,
        execution_command_context: None,
        execution_reentry_target_source: None,
        public_repair_targets: Vec::new(),
        blocking_records: Vec::new(),
        blocking_scope: Some(String::from("task")),
        external_wait_state: None,
        blocking_reason_codes: vec![String::from(
            crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
        )],
        projection_diagnostics: Vec::new(),
        state_kind: String::from("actionable_public_command"),
        next_public_action: None,
        blockers: Vec::new(),
        runtime_provenance: None,
        semantic_workspace_tree_id: String::from("semantic_tree:test"),
        raw_workspace_tree_id: Some(String::from("git_tree:test")),
        next_action: String::from(NEXT_ACTION_EXECUTION_REENTRY_REQUIRED),
        recommended_public_command: None,
        recommended_public_command_argv: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        recommended_command: None,
        finish_review_gate_pass_branch_closure_id: None,
        reason_codes: vec![
            String::from(REASON_CODE_STALE_PROVENANCE),
            String::from(REASON_BRANCH_DRIFT_ESCAPES_LATE_STAGE_SURFACE),
            String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED),
        ],
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
        blocking_task: Some(5),
        blocking_step: None,
        resume_task: Some(4),
        resume_step: Some(4),
    }
}

fn status_with_completed_stale_target_missing_current_closure() -> PlanExecutionStatus {
    let mut status = status_with_earlier_resume_and_later_stale_target();
    status.resume_task = None;
    status.resume_step = None;
    status.blocking_task = Some(5);
    status.blocking_step = None;
    status.reason_codes = vec![String::from(
        crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
    )];
    status.blocking_reason_codes = vec![String::from(
        crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
    )];
    status
}

fn close_current_task_repair_target(task: u32) -> PublicRepairTarget {
    PublicRepairTarget {
            command_kind: String::from("close-current-task"),
            task: Some(task),
            step: None,
            reason_code: crate::execution::public_repair_target_reasons::PublicRepairTargetReason::TaskReviewDispatchClosureReady
                .reason_code(),
            source_record_id: Some(format!("task-review-dispatch:task-{task}")),
            expires_when_fingerprint_changes: true,
        }
}

#[test]
fn shared_next_action_resumes_earlier_parked_step_before_later_stale_reopen() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let context = test_context(temp.path());
    let mut status = status_with_earlier_resume_and_later_stale_target();
    status.execution_command_context = Some(PublicExecutionCommandContext {
        command_kind: String::from("begin"),
        task_number: Some(4),
        step_id: Some(4),
    });
    let stale_target = AuthoritativeStaleReentryTarget {
        task: 5,
        step: Some(3),
        reason_code: REASON_CODE_STALE_PROVENANCE,
        source: AuthoritativeStaleTargetSource::ClosureGraph,
        source_record_id: Some("task-closure-5"),
        task_closure_bridge_allowed: false,
    };

    let decision = compute_next_action_decision_with_authority_inputs(
        &context,
        &status,
        NextActionRequestInputs {
            plan_path: &context.plan_rel,
            external_review_result_ready: false,
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            final_review_dispatch_lineage_present: false,
            final_review_outcome_recorded_for_current_dispatch: false,
        },
        NextActionAuthorityInputs {
            has_authoritative_stale_target: true,
            authoritative_stale_target: Some(stale_target),
            ..NextActionAuthorityInputs::default()
        },
    )
    .expect("shared next action should produce a decision");

    assert_eq!(decision.kind, NextActionKind::Resume);
    assert_eq!(decision.task_number, Some(4));
    assert_eq!(decision.step_number, Some(4));
    assert_eq!(
        decision.phase_detail,
        crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS
    );
}

#[test]
fn shared_next_action_rejects_raw_resume_without_legal_begin_authority() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let context = test_context(temp.path());
    let status = status_with_earlier_resume_and_later_stale_target();
    let stale_target = AuthoritativeStaleReentryTarget {
        task: 5,
        step: Some(3),
        reason_code: REASON_CODE_STALE_PROVENANCE,
        source: AuthoritativeStaleTargetSource::ClosureGraph,
        source_record_id: Some("task-closure-5"),
        task_closure_bridge_allowed: false,
    };

    let decision = compute_next_action_decision_with_authority_inputs(
        &context,
        &status,
        NextActionRequestInputs {
            plan_path: &context.plan_rel,
            external_review_result_ready: false,
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            final_review_dispatch_lineage_present: false,
            final_review_outcome_recorded_for_current_dispatch: false,
        },
        NextActionAuthorityInputs {
            has_authoritative_stale_target: true,
            authoritative_stale_target: Some(stale_target),
            ..NextActionAuthorityInputs::default()
        },
    )
    .expect("shared next action should still produce a non-resume decision");

    assert_ne!(
        decision.kind,
        NextActionKind::Resume,
        "route ordering must not promote raw resume_task/resume_step without fingerprint-bound begin authority"
    );
}

#[test]
fn shared_next_action_blocks_parked_resume_for_non_task_stale_boundary() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let context = test_context(temp.path());
    let mut status = status_with_earlier_resume_and_later_stale_target();
    status.blocking_scope = Some(String::from("branch"));
    status.blocking_task = None;
    status.blocking_step = None;
    status.stale_unreviewed_closures.clear();
    status
        .stale_unreviewed_closures
        .push(String::from("branch-closure-stale"));

    let decision = compute_next_action_decision_with_authority_inputs(
        &context,
        &status,
        NextActionRequestInputs {
            plan_path: &context.plan_rel,
            external_review_result_ready: false,
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            final_review_dispatch_lineage_present: false,
            final_review_outcome_recorded_for_current_dispatch: false,
        },
        NextActionAuthorityInputs {
            has_authoritative_stale_target: true,
            authoritative_stale_target: None,
            ..NextActionAuthorityInputs::default()
        },
    )
    .expect("non-task stale boundary should route to reconcile instead of parked resume begin");

    assert_eq!(decision.kind, NextActionKind::RepairReviewState);
    assert_eq!(decision.task_number, None);
    assert_eq!(decision.step_number, None);
    assert_eq!(
        decision.phase_detail,
        crate::execution::reentry_reconcile::TARGETLESS_STALE_RECONCILE_PHASE_DETAIL
    );
}

#[test]
fn shared_next_action_leaves_completed_stale_target_preemption_to_route_plan() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut context = test_context(temp.path());
    for step in &mut context.steps {
        step.checked = true;
    }
    let status = status_with_completed_stale_target_missing_current_closure();
    let stale_target = AuthoritativeStaleReentryTarget {
        task: 5,
        step: Some(3),
        reason_code: crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
        source: AuthoritativeStaleTargetSource::ClosureGraph,
        source_record_id: Some("task-closure-5"),
        task_closure_bridge_allowed: true,
    };

    let decision = compute_next_action_decision_with_authority_inputs(
        &context,
        &status,
        NextActionRequestInputs {
            plan_path: &context.plan_rel,
            external_review_result_ready: false,
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            final_review_dispatch_lineage_present: false,
            final_review_outcome_recorded_for_current_dispatch: false,
        },
        NextActionAuthorityInputs {
            has_authoritative_stale_target: true,
            authoritative_stale_target: Some(stale_target),
            ..NextActionAuthorityInputs::default()
        },
    )
    .expect("shared next action should produce a decision");

    assert_eq!(decision.kind, NextActionKind::Reopen);
    assert_eq!(decision.task_number, Some(5));
    assert_eq!(decision.step_number, Some(3));
    assert_eq!(
        decision.phase_detail,
        crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
    );
}

#[test]
fn shared_next_action_uses_repair_candidates_for_baseline_bridge_close_route() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut context = test_context(temp.path());
    for step in &mut context.steps {
        step.checked = step.task_number == 4;
    }
    let status = status_with_completed_stale_target_missing_current_closure();
    let stale_target = AuthoritativeStaleReentryTarget {
        task: 4,
        step: None,
        reason_code: crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
        source: AuthoritativeStaleTargetSource::BaselineBridge,
        source_record_id: Some("task-closure-baseline-bridge"),
        task_closure_bridge_allowed: true,
    };
    let route_repair_target_candidates = vec![close_current_task_repair_target(4)];

    let decision = compute_next_action_decision_with_authority_inputs(
        &context,
        &status,
        NextActionRequestInputs {
            plan_path: &context.plan_rel,
            external_review_result_ready: false,
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            final_review_dispatch_lineage_present: false,
            final_review_outcome_recorded_for_current_dispatch: false,
        },
        NextActionAuthorityInputs {
            has_authoritative_stale_target: true,
            authoritative_stale_target: Some(stale_target),
            route_repair_target_candidates: &route_repair_target_candidates,
            ..NextActionAuthorityInputs::default()
        },
    )
    .expect("shared next action should produce a decision");

    assert_eq!(decision.kind, NextActionKind::CloseCurrentTask);
    assert_eq!(decision.task_number, Some(4));
    assert_eq!(
        decision.phase_detail,
        crate::execution::phase::DETAIL_TASK_CLOSURE_RECORDING_READY
    );
}

#[test]
fn shared_next_action_keeps_closed_stale_provenance_diagnostic_only() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let context = test_context(temp.path());
    let mut status = status_with_earlier_resume_and_later_stale_target();
    status.harness_phase = HarnessPhase::FinalReviewPending;
    status.review_state_status = String::from("clean");
    status.phase_detail =
        String::from(crate::execution::phase::DETAIL_RELEASE_READINESS_RECORDING_READY);
    status.blocking_task = None;
    status.blocking_step = None;
    status.resume_task = None;
    status.resume_step = None;
    status.current_branch_closure_id = Some(String::from("branch-closure-current"));
    status.current_branch_reviewed_state_id = Some(String::from("git_tree:current"));
    status.current_branch_meaningful_drift = false;
    status.current_task_closures = vec![PublicReviewStateTaskClosure {
        task: 4,
        closure_record_id: String::from("task-closure-current"),
        reviewed_state_id: String::from("git_tree:current"),
        contract_identity: String::from("task-contract-4"),
        effective_reviewed_surface_paths: vec![String::from("src/task-4.rs")],
    }];
    status.stale_unreviewed_closures.clear();
    status.current_release_readiness_state = None;
    status.current_final_review_result = None;
    status.current_qa_result = None;
    status.semantic_workspace_tree_id = String::from("semantic_tree:current");
    status.reason_codes = vec![String::from(REASON_CODE_STALE_PROVENANCE)];
    status.blocking_reason_codes = status.reason_codes.clone();

    let decision = compute_next_action_decision_with_authority_inputs(
        &context,
        &status,
        NextActionRequestInputs {
            plan_path: &context.plan_rel,
            external_review_result_ready: false,
            task_review_dispatch_id: None,
            final_review_dispatch_id: None,
            final_review_dispatch_lineage_present: false,
            final_review_outcome_recorded_for_current_dispatch: false,
        },
        NextActionAuthorityInputs::default(),
    )
    .expect("closed diagnostic stale provenance should still route through late-stage progress");

    assert_ne!(
        decision.phase_detail,
        crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
    );
    assert_ne!(
        decision.phase_detail,
        crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
    );
    assert_ne!(decision.kind, NextActionKind::RepairReviewState);
}

#[test]
fn shared_next_action_keeps_closed_stale_provenance_diagnostic_after_late_stage_records() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let context = test_context(temp.path());
    let mut status = status_with_earlier_resume_and_later_stale_target();
    status.harness_phase = HarnessPhase::QaPending;
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(crate::execution::phase::DETAIL_QA_RECORDING_REQUIRED);
    status.blocking_task = None;
    status.blocking_step = None;
    status.resume_task = None;
    status.resume_step = None;
    status.current_branch_closure_id = Some(String::from("branch-closure-current"));
    status.current_branch_reviewed_state_id = Some(String::from("git_tree:current"));
    status.current_branch_meaningful_drift = false;
    status.current_task_closures = vec![PublicReviewStateTaskClosure {
        task: 4,
        closure_record_id: String::from("task-closure-current"),
        reviewed_state_id: String::from("git_tree:current"),
        contract_identity: String::from("task-contract-4"),
        effective_reviewed_surface_paths: vec![String::from("src/task-4.rs")],
    }];
    status.stale_unreviewed_closures.clear();
    status.current_release_readiness_state = Some(String::from("ready"));
    status.current_final_review_result = Some(String::from("pass"));
    status.current_qa_result = None;
    status.semantic_workspace_tree_id = String::from("semantic_tree:current");
    status.reason_codes = vec![String::from(REASON_CODE_STALE_PROVENANCE)];
    status.blocking_reason_codes = status.reason_codes.clone();

    let decision = compute_next_action_decision_with_authority_inputs(
            &context,
            &status,
            NextActionRequestInputs {
                plan_path: &context.plan_rel,
                external_review_result_ready: false,
                task_review_dispatch_id: None,
                final_review_dispatch_id: None,
                final_review_dispatch_lineage_present: false,
                final_review_outcome_recorded_for_current_dispatch: false,
            },
            NextActionAuthorityInputs::default(),
        )
        .expect("closed stale provenance after late-stage records should still route through late-stage progress");

    assert_ne!(
        decision.phase_detail,
        crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED
    );
    assert_ne!(
        decision.phase_detail,
        crate::execution::phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
    );
    assert_ne!(decision.kind, NextActionKind::RepairReviewState);
}
