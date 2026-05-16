use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use super::next_action_choice::{
    NEXT_ACTION_ADVANCE_LATE_STAGE, NEXT_ACTION_CLOSE_CURRENT_TASK,
    NEXT_ACTION_EXECUTION_REENTRY_REQUIRED, NEXT_ACTION_PLANNING_REENTRY,
    NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED, NEXT_ACTION_WAIT_FOR_EXTERNAL_REVIEW_RESULT,
};
use crate::contracts::plan::{PlanDocument, PlanStep, PlanTask, TaskFileEntry};
use crate::contracts::workflow::WorkflowRoute;
use crate::execution::command_eligibility::{
    PublicCommand, PublicCommandInvocation, PublicCommandKind, command_invokes_hidden_lane,
    hidden_command_tokens,
};
use crate::execution::context::EvidenceSourceOrigin;
use crate::execution::current_truth::{
    BranchRerecordingAssessment, CurrentLateStageBranchBindings,
};
use crate::execution::follow_up::follow_up_command_template as follow_up_to_command_template;
use crate::execution::harness::{
    AggregateEvaluationState, ChunkId, DownstreamFreshnessState, ExecutionRunId, HarnessPhase,
};
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::phase;
use crate::execution::public_repair_target_reasons::PublicRepairTargetReason;
use crate::execution::public_repair_targets::{
    public_repair_targets_for_route_decision, route_decision_exposes_repair_review_state_target,
};
use crate::execution::query::{
    ExecutionRoutingExecutionCommandContext, ExecutionRoutingRecordingContext,
    ExecutionRoutingState,
};
use crate::execution::reducer::RuntimeState;
use crate::execution::repair_target_selection::{
    AuthoritativeStaleReentryTarget, ExecutionReentryTargetSource, NextActionAuthorityInputs,
    execution_reentry_target, task_boundary_blocking_task,
};
use crate::execution::resume_stale_precedence::{
    ResumeStalePrecedence, ResumeStalePrecedenceInputs, StalePreemptionTarget,
};
use crate::execution::review_route_tokens::{
    FOLLOW_UP_ADVANCE_LATE_STAGE, FOLLOW_UP_EXECUTION_REENTRY, FOLLOW_UP_REPAIR_REVIEW_STATE,
    REVIEW_STATE_MISSING_CURRENT_CLOSURE, REVIEW_STATE_STALE_UNREVIEWED,
};
use crate::execution::runtime::ExecutionRuntime;
use crate::execution::runtime_truth::FinalReviewDispatchAuthority;
use crate::execution::semantic_identity::SemanticWorkspaceSnapshot;
use crate::execution::stale_target_projection::{
    AuthoritativeStaleTarget, AuthoritativeStaleTargetScope, AuthoritativeStaleTargetSource,
    CLOSURE_GRAPH_STALE_TARGET_SOURCE_TOKEN, RuntimeGateSnapshot,
    project_stale_unreviewed_closures,
};
use crate::execution::stale_target_selection::select_route_projected_stale_boundary_task;
use crate::execution::state::{
    CurrentTaskClosureBranchRouteFacts, GateState, PlanExecutionStatus, PublicRepairTarget,
    PublicReviewStateTaskClosure, StatusBlockingRecord,
};
use crate::execution::state::{
    EvidenceAttempt, EvidenceFormat, ExecutionContext, ExecutionEvidence, PlanStepState,
};
use crate::execution::status::PublicExecutionCommandContext;
use crate::execution::status_assembly::{
    StatusReviewStateInputs, derive_status_review_state_fact, effective_review_state_status,
    effective_route_review_state_status,
};

use super::blockers::primary_blocker_for_route;
use super::decision::{Blocker, NextPublicAction};
use super::execution_target_authority::{
    execution_command_route_target_has_authority, legal_execution_begin_route,
};
use super::execution_targets::{
    ExecutionCommandRouteTarget, execution_command_route_target_matches_public_status,
    fingerprint_bound_begin_route_matches_public_status,
};
use super::finalization_facts::{ExecutionReentryTaskClosureBridgeFacts, PersistedReopenTarget};
use super::planning_facts::{RoutePlanningFactInputs, RoutePlanningFacts};
use super::public_action::{public_command_for_phase_detail, synthesize_next_public_action};
use super::stale_repair_target::projected_stale_repair_record_task;
use super::{
    RouteDecision, STATE_KIND_PLANNING_REENTRY_REQUIRED, STATE_KIND_TERMINAL, classify_state_kind,
    derive_required_follow_up, execution_reentry_target_source_for_route,
    public_command_for_required_follow_up, route_decision_from_non_runtime_workflow_routing,
    route_decision_from_runtime_state_with_inputs, task_closure_recording_reentry_target_source,
};

fn closed_stale_provenance_status() -> PlanExecutionStatus {
    PlanExecutionStatus {
        schema_version: 3,
        plan_revision: 1,
        execution_run_id: None,
        workspace_state_id: String::from("semantic_tree:current"),
        current_branch_reviewed_state_id: Some(String::from("git_tree:current")),
        current_branch_closure_id: Some(String::from("branch-closure-current")),
        current_branch_meaningful_drift: false,
        current_task_closures: vec![PublicReviewStateTaskClosure {
            task: 1,
            closure_record_id: String::from("task-closure-current"),
            reviewed_state_id: String::from("git_tree:current"),
            contract_identity: String::from("task-contract-1"),
            effective_reviewed_surface_paths: vec![String::from("src/task-1.rs")],
        }],
        superseded_closures_summary: Vec::new(),
        stale_unreviewed_closures: Vec::new(),
        current_release_readiness_state: Some(String::from("ready")),
        current_final_review_state: String::from("fresh"),
        current_qa_state: String::from("not_required"),
        current_final_review_branch_closure_id: Some(String::from("branch-closure-current")),
        current_final_review_result: Some(String::from("pass")),
        current_qa_branch_closure_id: None,
        current_qa_result: None,
        qa_requirement: None,
        latest_authoritative_sequence: 1,
        phase: Some(String::from(phase::PHASE_FINAL_REVIEW_PENDING)),
        harness_phase: HarnessPhase::FinalReviewPending,
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
        final_review_state: DownstreamFreshnessState::Fresh,
        browser_qa_state: DownstreamFreshnessState::NotRequired,
        release_docs_state: DownstreamFreshnessState::Fresh,
        last_final_review_artifact_fingerprint: None,
        last_browser_qa_artifact_fingerprint: None,
        last_release_docs_artifact_fingerprint: None,
        strategy_state: String::from("ready"),
        last_strategy_checkpoint_fingerprint: None,
        strategy_checkpoint_kind: String::from("review_remediation"),
        strategy_reset_required: false,
        phase_detail: String::from(phase::DETAIL_RELEASE_READINESS_RECORDING_READY),
        review_state_status: String::from("clean"),
        recording_context: None,
        execution_command_context: None,
        execution_reentry_target_source: None,
        public_repair_targets: Vec::new(),
        blocking_records: Vec::new(),
        blocking_scope: Some(String::from("branch")),
        external_wait_state: None,
        blocking_reason_codes: vec![String::from(REASON_CODE_STALE_PROVENANCE)],
        projection_diagnostics: Vec::new(),
        state_kind: String::from("actionable_public_command"),
        next_public_action: None,
        blockers: Vec::new(),
        runtime_provenance: None,
        semantic_workspace_tree_id: String::from("semantic_tree:current"),
        raw_workspace_tree_id: Some(String::from("git_tree:current")),
        next_action: String::from(NEXT_ACTION_ADVANCE_LATE_STAGE),
        recommended_public_command: None,
        recommended_public_command_argv: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        recommended_command: None,
        finish_review_gate_pass_branch_closure_id: None,
        reason_codes: vec![String::from(REASON_CODE_STALE_PROVENANCE)],
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

fn route_plan_test_runtime(root: &Path) -> ExecutionRuntime {
    ExecutionRuntime {
        repo_root: root.to_path_buf(),
        git_dir: root.join(".git"),
        branch_name: String::from("feature/test"),
        repo_slug: String::from("featureforge"),
        safe_branch: String::from("feature-test"),
        state_dir: root.join("state"),
    }
}

fn route_plan_test_task(number: u32, step_number: u32) -> PlanTask {
    PlanTask {
        number,
        title: format!("Task {number}"),
        spec_coverage: vec![String::from("DR-TEST")],
        goal: String::from("Exercise route-planning behavior."),
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

fn completed_stale_target_route_plan_context(root: &Path) -> ExecutionContext {
    let task4 = route_plan_test_task(4, 4);
    let task5 = route_plan_test_task(5, 3);
    let tasks_by_number = [(4, task4.clone()), (5, task5.clone())]
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    ExecutionContext {
        runtime: route_plan_test_runtime(root),
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
                checked: true,
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

fn completed_stale_target_missing_current_closure_status() -> PlanExecutionStatus {
    let mut status = closed_stale_provenance_status();
    status.current_branch_reviewed_state_id = None;
    status.current_branch_closure_id = None;
    status.current_task_closures = Vec::new();
    status.current_release_readiness_state = None;
    status.current_final_review_state = String::from("not_required");
    status.current_qa_state = String::from("not_required");
    status.current_final_review_branch_closure_id = None;
    status.current_final_review_result = None;
    status.current_qa_branch_closure_id = None;
    status.current_qa_result = None;
    status.phase = Some(String::from(phase::PHASE_EXECUTING));
    status.harness_phase = HarnessPhase::Executing;
    status.final_review_state = DownstreamFreshnessState::NotRequired;
    status.browser_qa_state = DownstreamFreshnessState::NotRequired;
    status.release_docs_state = DownstreamFreshnessState::NotRequired;
    status.strategy_state = String::from("checkpoint_missing");
    status.strategy_checkpoint_kind = String::from("none");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED);
    status.review_state_status = String::from(REVIEW_STATE_STALE_UNREVIEWED);
    status.blocking_reason_codes = vec![String::from(REVIEW_STATE_STALE_UNREVIEWED)];
    status.next_action = String::from(NEXT_ACTION_EXECUTION_REENTRY_REQUIRED);
    status.reason_codes = vec![String::from(REVIEW_STATE_STALE_UNREVIEWED)];
    status.recommended_public_command = None;
    status.recommended_public_command_argv = None;
    status.recommended_public_command_template = None;
    status.required_inputs = Vec::new();
    status.recommended_command = None;
    status.active_task = None;
    status.active_step = None;
    status.blocking_task = Some(5);
    status.blocking_step = None;
    status.resume_task = None;
    status.resume_step = None;
    status
}

fn completed_stale_target_runtime_state(root: &Path) -> RuntimeState {
    let context = completed_stale_target_route_plan_context(root);
    let stale_target = AuthoritativeStaleTarget {
        scope: AuthoritativeStaleTargetScope::Task,
        task: Some(5),
        step: Some(3),
        record_id: Some(String::from("task-closure-5")),
        source: AuthoritativeStaleTargetSource::ClosureGraph,
        reason_code: String::from(REVIEW_STATE_STALE_UNREVIEWED),
        task_closure_bridge_allowed: true,
    };
    RuntimeState {
        context,
        semantic_workspace: SemanticWorkspaceSnapshot {
            raw_workspace_tree_id: String::from("git_tree:test"),
            semantic_workspace_tree_id: String::from("semantic_tree:test"),
            plan_definition_identity: String::from("plan:test"),
            task_definition_identity: BTreeMap::new(),
            branch_definition_identity: String::from("branch:test"),
        },
        status: completed_stale_target_missing_current_closure_status(),
        overlay: None,
        route_repair_target_candidates: Vec::new(),
        preflight: None,
        gate_review: None,
        gate_finish: None,
        gate_snapshot: RuntimeGateSnapshot {
            preflight: None,
            gate_review: None,
            gate_finish: None,
            stale_reason_codes: vec![String::from(REVIEW_STATE_STALE_UNREVIEWED)],
            stale_targets: vec![stale_target],
            branch_closure_tracked_drift: false,
            late_stage_stale_unreviewed: false,
            missing_current_closure_stale_provenance: false,
        },
        base_branch: None,
        authoritative_current_branch_closure_id: None,
        authoritative_current_branch_reviewed_state_id: None,
        late_stage_bindings: CurrentLateStageBranchBindings::default(),
        persisted_repair_follow_up: None,
        release_readiness_result_for_current_branch: None,
        branch_rerecording_assessment: None,
        task_review_dispatch_id: None,
        final_review_dispatch_authority: FinalReviewDispatchAuthority::default(),
        final_review_outcome_recorded_for_current_dispatch: false,
    }
}

fn release_readiness_route_decision() -> RouteDecision {
    RouteDecision {
        state_kind: String::from("actionable_public_command"),
        phase: String::from(phase::PHASE_FINAL_REVIEW_PENDING),
        phase_detail: String::from(phase::DETAIL_RELEASE_READINESS_RECORDING_READY),
        review_state_status: String::from("clean"),
        next_action: String::from(NEXT_ACTION_ADVANCE_LATE_STAGE),
        blocking_reason_codes: vec![String::from(REASON_CODE_STALE_PROVENANCE)],
        blocking_scope: Some(String::from("branch")),
        blocking_task: None,
        external_wait_state: None,
        recommended_command: None,
        recommended_public_command: None,
        invocation: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        required_follow_up: None,
        next_public_action: None,
        blockers: Vec::new(),
        public_repair_targets: Vec::new(),
        execution_reentry_target_source: None,
        execution_command_context: None,
        recording_context: None,
    }
}

#[test]
fn diagnostic_stale_provenance_does_not_expose_repair_target_from_phase() {
    let status = closed_stale_provenance_status();
    let route_decision = release_readiness_route_decision();

    assert!(
        !route_decision_exposes_repair_review_state_target(&status, &route_decision),
        "diagnostic stale provenance after authoritative closure must not expose repair-review-state from late-stage phase alone"
    );
}

#[test]
fn real_stale_closure_still_exposes_repair_target() {
    let mut status = closed_stale_provenance_status();
    push_status_stale_unreviewed_closure(&mut status, "task-closure-stale");
    let mut route_decision = release_readiness_route_decision();
    route_decision.review_state_status = String::from(REVIEW_STATE_STALE_UNREVIEWED);

    assert!(
        route_decision_exposes_repair_review_state_target(&status, &route_decision),
        "real stale closure targets must still expose repair-review-state"
    );
}

fn clean_review_state_inputs() -> StatusReviewStateInputs {
    StatusReviewStateInputs {
        repair_follow_up_requires_execution_reentry: false,
        repair_follow_up_records_branch_closure: false,
        branch_scope_stale_unreviewed: false,
        task_boundary_unresolved_stale: false,
    }
}

fn derived_and_route_review_state_status(mut status: PlanExecutionStatus) -> (String, String) {
    let gate_review = GateState::default().finish();
    let gate_finish = GateState::default().finish();
    let derived = derive_status_review_state_fact(
        &status,
        &gate_review,
        &gate_finish,
        &clean_review_state_inputs(),
    );
    status.review_state_status.clone_from(&derived);
    let route_status = effective_review_state_status(&status, status.review_state_status.as_str());
    (derived, route_status)
}

#[test]
fn effective_review_state_status_stays_clean_for_clean_status() {
    let mut status = closed_stale_provenance_status();
    status.reason_codes.clear();
    status.blocking_reason_codes.clear();
    status.stale_unreviewed_closures.clear();
    status.review_state_status = String::from("clean");

    let (status_assembly, route_planning) = derived_and_route_review_state_status(status);

    assert_eq!(status_assembly, "clean");
    assert_eq!(route_planning, status_assembly);
}

#[test]
fn effective_review_state_status_marks_branch_refresh_missing_current_closure() {
    let mut status = closed_stale_provenance_status();
    status.reason_codes.clear();
    status.blocking_reason_codes.clear();
    status.stale_unreviewed_closures.clear();
    status.review_state_status = String::from("clean");
    status.harness_phase = HarnessPhase::DocumentReleasePending;
    status.phase = Some(String::from(phase::PHASE_DOCUMENT_RELEASE_PENDING));
    status.phase_detail =
        String::from(phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS);
    status.current_release_readiness_state = None;
    status.current_branch_closure_id = Some(String::from("branch-closure-current"));
    status.current_branch_meaningful_drift = true;

    let (status_assembly, route_planning) = derived_and_route_review_state_status(status);

    assert_eq!(status_assembly, REVIEW_STATE_MISSING_CURRENT_CLOSURE);
    assert_eq!(route_planning, status_assembly);
}

#[test]
fn effective_route_review_state_status_marks_branch_recording_route_missing_current_closure() {
    let mut status = closed_stale_provenance_status();
    status.reason_codes.clear();
    status.blocking_reason_codes.clear();
    status.stale_unreviewed_closures.clear();
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_RELEASE_READINESS_RECORDING_READY);

    let route_status = effective_route_review_state_status(
        &status,
        phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
        &status.review_state_status,
    );

    assert_eq!(route_status, REVIEW_STATE_MISSING_CURRENT_CLOSURE);
}

#[test]
fn effective_review_state_status_preserves_stale_unreviewed_consistency() {
    let mut status = closed_stale_provenance_status();
    status.reason_codes.clear();
    status.blocking_reason_codes.clear();
    status.review_state_status = String::from("clean");
    push_status_stale_unreviewed_closure(&mut status, "task-closure-stale");

    let (status_assembly, route_planning) = derived_and_route_review_state_status(status);

    assert_eq!(status_assembly, REVIEW_STATE_STALE_UNREVIEWED);
    assert_eq!(route_planning, status_assembly);
}

#[test]
fn effective_review_state_status_preserves_missing_current_closure_reason_consistency() {
    let mut status = closed_stale_provenance_status();
    status.reason_codes = vec![String::from(REVIEW_STATE_MISSING_CURRENT_CLOSURE)];
    status.blocking_reason_codes = status.reason_codes.clone();
    status.stale_unreviewed_closures.clear();
    status.review_state_status = String::from("clean");
    status.current_branch_closure_id = None;

    let (status_assembly, route_planning) = derived_and_route_review_state_status(status);

    assert_eq!(status_assembly, REVIEW_STATE_MISSING_CURRENT_CLOSURE);
    assert_eq!(route_planning, status_assembly);
}

#[test]
fn public_repair_target_assembler_emits_route_owned_reopen_target() {
    let status = closed_stale_provenance_status();
    let mut route_decision = release_readiness_route_decision();
    route_decision.phase_detail = String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED);
    route_decision.recommended_public_command = Some(PublicCommand::Reopen {
        plan: String::from("docs/featureforge/plans/plan.md"),
        task: 2,
        step: 3,
        source: None,
        reason: None,
        fingerprint: None,
    });

    let targets = public_repair_targets_for_route_decision(&status, &route_decision, &[]);

    assert_eq!(
        targets.len(),
        1,
        "route-owned reopen should expose one repair target"
    );
    assert_eq!(
        targets[0].command_kind,
        PublicCommandKind::Reopen.public_mutation_token()
    );
    assert_eq!(targets[0].task, Some(2));
    assert_eq!(targets[0].step, Some(3));
    assert_eq!(
        targets[0].reason_code,
        PublicRepairTargetReason::RouteExecutionReentryRequired.reason_code()
    );
}

#[test]
fn public_repair_target_assembler_emits_route_owned_close_current_task_target() {
    let status = closed_stale_provenance_status();
    let mut route_decision = release_readiness_route_decision();
    route_decision.phase_detail = String::from(phase::DETAIL_TASK_CLOSURE_RECORDING_READY);
    route_decision.recording_context = Some(ExecutionRoutingRecordingContext {
        task_number: Some(4),
        dispatch_id: Some(String::from("dispatch-4")),
        branch_closure_id: None,
    });

    let targets = public_repair_targets_for_route_decision(&status, &route_decision, &[]);

    assert_eq!(
        targets.len(),
        1,
        "route-owned close-current-task should expose one repair target"
    );
    assert_eq!(
        targets[0].command_kind,
        PublicCommandKind::CloseCurrentTask.public_mutation_token()
    );
    assert_eq!(targets[0].task, Some(4));
    assert_eq!(targets[0].step, None);
    assert_eq!(
        targets[0].reason_code,
        PublicRepairTargetReason::RouteTaskClosureRecordingReady.reason_code()
    );
}

#[test]
fn public_repair_target_assembler_dedupes_route_and_authority_close_current_task() {
    let status = closed_stale_provenance_status();
    let mut route_decision = release_readiness_route_decision();
    route_decision.phase_detail = String::from(phase::DETAIL_TASK_CLOSURE_RECORDING_READY);
    route_decision.recording_context = Some(ExecutionRoutingRecordingContext {
        task_number: Some(4),
        dispatch_id: Some(String::from("dispatch-4")),
        branch_closure_id: None,
    });
    let authority_candidates = vec![PublicRepairTarget {
        command_kind: String::from(PublicCommandKind::CloseCurrentTask.public_mutation_token()),
        task: Some(4),
        step: None,
        reason_code: PublicRepairTargetReason::TaskReviewDispatchClosureReady.reason_code(),
        source_record_id: Some(String::from("dispatch-4")),
        expires_when_fingerprint_changes: true,
    }];

    let targets =
        public_repair_targets_for_route_decision(&status, &route_decision, &authority_candidates);

    assert_eq!(
        targets.len(),
        1,
        "central assembler should dedupe matching route and authority close-current-task targets"
    );
    assert_eq!(
        targets[0].reason_code,
        PublicRepairTargetReason::RouteTaskClosureRecordingReady.reason_code(),
        "the route-owned target should remain the canonical target when it duplicates authority"
    );
}

#[test]
fn public_repair_target_assembler_suppresses_diagnostic_only_targets() {
    let status = closed_stale_provenance_status();
    let mut route_decision = release_readiness_route_decision();
    route_decision.state_kind = String::from(phase::DETAIL_BLOCKED_RUNTIME_BUG);
    route_decision.phase_detail = String::from(phase::DETAIL_BLOCKED_RUNTIME_BUG);
    let authority_candidates = vec![PublicRepairTarget {
        command_kind: String::from(PublicCommandKind::CloseCurrentTask.public_mutation_token()),
        task: Some(4),
        step: None,
        reason_code: PublicRepairTargetReason::TaskReviewDispatchClosureReady.reason_code(),
        source_record_id: Some(String::from("dispatch-4")),
        expires_when_fingerprint_changes: true,
    }];

    let targets =
        public_repair_targets_for_route_decision(&status, &route_decision, &authority_candidates);

    assert!(
        targets.is_empty(),
        "diagnostic-only routes must not expose authority repair targets"
    );
}

#[test]
fn route_public_projection_preserves_route_selected_reconcile_target() {
    let status = closed_stale_provenance_status();
    let mut route_decision = release_readiness_route_decision();
    route_decision.phase_detail = String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
    route_decision.review_state_status = String::from(REVIEW_STATE_STALE_UNREVIEWED);
    route_decision.blocking_scope = Some(String::from("task"));
    route_decision.blocking_task = Some(2);
    route_decision.blocking_reason_codes = vec![String::from(
        crate::execution::reentry_reconcile::TARGETLESS_STALE_RECONCILE_REASON_CODE,
    )];

    route_decision.apply_public_route_projection(Some(&status), false);

    assert_eq!(
        route_decision.blocking_task,
        Some(2),
        "route-owned target selection must survive public projection even when the status fallback is still targetless"
    );
    assert_eq!(route_decision.blocking_scope.as_deref(), Some("task"));
}

#[test]
fn terminal_finish_completion_does_not_expose_repair_review_state_target() {
    let status = closed_stale_provenance_status();
    let mut route_decision = release_readiness_route_decision();
    route_decision.phase_detail = String::from(phase::DETAIL_FINISH_COMPLETION_GATE_READY);
    route_decision.state_kind = String::from("terminal");
    route_decision.next_action = String::from("complete");

    route_decision.apply_public_route_projection(Some(&status), false);

    assert!(
        !route_decision
            .public_repair_targets
            .iter()
            .any(|target| target.command_kind == "repair-review-state"),
        "terminal finish completion must not expose repair-review-state as a public repair target: {route_decision:?}"
    );
}

#[test]
fn dispatch_stale_projection_diagnostic_does_not_expose_repair_review_state_target() {
    let mut status = closed_stale_provenance_status();
    status.phase_detail = String::from(phase::DETAIL_TASK_CLOSURE_RECORDING_READY);
    status.reason_codes = vec![String::from(
        crate::execution::closure_diagnostics::TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_STALE,
    )];
    let mut route_decision = release_readiness_route_decision();
    route_decision.phase_detail = String::from(phase::DETAIL_TASK_CLOSURE_RECORDING_READY);
    route_decision.blocking_reason_codes = status.reason_codes.clone();
    route_decision.recording_context = None;

    route_decision.apply_public_route_projection(Some(&status), false);

    assert!(
        !route_decision
            .public_repair_targets
            .iter()
            .any(|target| target.command_kind == "repair-review-state"),
        "projection-only stale dispatch diagnostics must not expose repair-review-state targets: {route_decision:?}"
    );
}

#[test]
fn stale_projection_does_not_treat_status_stale_ids_as_authority() {
    let mut status = closed_stale_provenance_status();
    status.review_state_status = String::from(REVIEW_STATE_STALE_UNREVIEWED);
    push_status_stale_unreviewed_closure(&mut status, "task-closure-projected");
    let gate_snapshot = RuntimeGateSnapshot {
        preflight: None,
        gate_review: None,
        gate_finish: None,
        stale_reason_codes: Vec::new(),
        stale_targets: Vec::new(),
        branch_closure_tracked_drift: false,
        late_stage_stale_unreviewed: false,
        missing_current_closure_stale_provenance: false,
    };

    project_stale_unreviewed_closures(&mut status, &gate_snapshot);

    assert!(
        status.stale_unreviewed_closures.is_empty(),
        "projected status stale ids must be cleared when reducer/gate authority has no stale target"
    );
    assert!(
        status.reason_codes.iter().any(|code| code
            == crate::execution::reentry_reconcile::TARGETLESS_STALE_RECONCILE_REASON_CODE),
        "targetless stale without reducer authority must become diagnostic-only"
    );
}

#[test]
fn targetless_stale_route_ignores_status_stale_ids_without_gate_authority() {
    let root = std::env::temp_dir().join("featureforge-targetless-stale-route-authority");
    let mut runtime_state = completed_stale_target_runtime_state(&root);
    runtime_state.gate_snapshot.stale_targets.clear();
    runtime_state.gate_snapshot.stale_reason_codes.clear();
    runtime_state.status.review_state_status = String::from(REVIEW_STATE_STALE_UNREVIEWED);
    push_status_stale_unreviewed_closure(&mut runtime_state.status, "task-closure-projected");
    runtime_state.status.reason_codes.clear();
    runtime_state.status.blocking_reason_codes.clear();
    runtime_state.status.blocking_records.clear();

    let decision = route_decision_from_runtime_state_with_inputs(&runtime_state, false, false)
        .expect("route decision should project");

    assert_eq!(
        decision.phase_detail,
        phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
    );
    assert!(
        decision.recommended_public_command.is_none()
            && decision.invocation.is_none()
            && decision.public_repair_targets.is_empty(),
        "status-only stale ids must not produce repair-review-state argv or public repair targets without gate authority: {decision:?}"
    );
}

#[test]
fn derived_overlay_diagnostics_do_not_select_repair_review_state_route() {
    let root = std::env::temp_dir().join("featureforge-derived-overlay-diagnostic-route");
    let mut runtime_state = completed_stale_target_runtime_state(&root);
    runtime_state.status = closed_stale_provenance_status();
    runtime_state.status.reason_codes.clear();
    runtime_state.status.blocking_reason_codes.clear();
    runtime_state.status.blocking_records.clear();
    runtime_state.status.projection_diagnostics = vec![String::from(
        crate::execution::review_route_tokens::REASON_DERIVED_REVIEW_STATE_MISSING,
    )];
    runtime_state.gate_snapshot.stale_reason_codes.clear();
    runtime_state.gate_snapshot.stale_targets.clear();
    runtime_state.gate_snapshot.branch_closure_tracked_drift = false;
    runtime_state.gate_snapshot.late_stage_stale_unreviewed = false;
    runtime_state
        .gate_snapshot
        .missing_current_closure_stale_provenance = false;
    runtime_state.authoritative_current_branch_closure_id =
        runtime_state.status.current_branch_closure_id.clone();
    runtime_state.authoritative_current_branch_reviewed_state_id = runtime_state
        .status
        .current_branch_reviewed_state_id
        .clone();

    let decision = route_decision_from_runtime_state_with_inputs(&runtime_state, false, false)
        .expect("route decision should project");

    assert_ne!(
        decision.required_follow_up.as_deref(),
        Some(FOLLOW_UP_REPAIR_REVIEW_STATE),
        "projection diagnostics alone must not choose repair-review-state: {decision:?}"
    );
    assert!(
        !matches!(
            decision.recommended_public_command.as_ref(),
            Some(PublicCommand::RepairReviewState { .. })
        ),
        "projection diagnostics alone must not expose repair-review-state argv: {decision:?}"
    );
    assert!(
        !decision.blocking_reason_codes.iter().any(|code| {
            code == crate::execution::review_route_tokens::REASON_DERIVED_REVIEW_STATE_MISSING
        }),
        "derived overlay freshness belongs in projection_diagnostics, not public route blockers: {decision:?}"
    );
}

#[test]
fn projection_only_milestone_target_routes_to_targetless_reconcile() {
    let root = std::env::temp_dir().join("featureforge-projection-only-milestone-target");
    let mut runtime_state = completed_stale_target_runtime_state(&root);
    runtime_state.gate_snapshot.stale_targets = vec![AuthoritativeStaleTarget {
        scope: AuthoritativeStaleTargetScope::Milestone,
        task: None,
        step: None,
        record_id: Some(String::from("projection-only-final-review")),
        source: AuthoritativeStaleTargetSource::ProjectionOnly,
        reason_code: String::from("projection_only_stale_target"),
        task_closure_bridge_allowed: false,
    }];
    runtime_state.status.stale_unreviewed_closures.clear();
    runtime_state.status.blocking_records.clear();
    runtime_state.status.public_repair_targets.clear();

    let decision = route_decision_from_runtime_state_with_inputs(&runtime_state, false, false)
        .expect("route decision should project");

    assert_eq!(
        decision.phase_detail,
        phase::DETAIL_RUNTIME_RECONCILE_REQUIRED,
        "projection-only stale milestone ids should become diagnostic reconcile, not execution reentry"
    );
    assert!(
        decision.recommended_public_command.is_none()
            && decision.invocation.is_none()
            && decision.public_repair_targets.is_empty(),
        "projection-only stale milestone ids must not expose public repair authority: {decision:?}"
    );
}

#[test]
fn targetless_projection_diagnostic_does_not_emit_repair_follow_up() {
    let mut status = closed_stale_provenance_status();
    status.phase_detail = String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
    status.review_state_status = String::from(REVIEW_STATE_STALE_UNREVIEWED);
    status.blocking_reason_codes = vec![String::from(
        crate::execution::reentry_reconcile::TARGETLESS_STALE_RECONCILE_REASON_CODE,
    )];
    status.reason_codes = status.blocking_reason_codes.clone();

    let follow_up = derive_required_follow_up(
        &status,
        &status.phase_detail,
        &status.review_state_status,
        status.blocking_reason_codes.iter().map(String::as_str),
        None,
    );

    assert!(
        follow_up.is_none(),
        "targetless projection diagnostics must remain diagnostic-only in the active follow-up helper path"
    );
}

#[test]
fn closure_graph_reentry_source_token_flows_through_route_and_status_bridge() {
    let root = std::env::temp_dir().join("featureforge-closure-graph-source-token");
    let runtime_state = completed_stale_target_runtime_state(&root);
    let authoritative_stale_target = runtime_state
        .gate_snapshot
        .stale_targets
        .first()
        .expect("fixture should include an authoritative stale target");
    let source = execution_reentry_target_source_for_route(
        &runtime_state,
        &runtime_state.status,
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
        NextActionAuthorityInputs {
            route_repair_target_candidates: &runtime_state.route_repair_target_candidates,
            has_authoritative_stale_target: true,
            authoritative_stale_target: AuthoritativeStaleReentryTarget::from_stale_target(
                authoritative_stale_target,
            ),
            ..Default::default()
        },
    );

    assert_eq!(
        source.as_deref(),
        Some(CLOSURE_GRAPH_STALE_TARGET_SOURCE_TOKEN),
        "route source projection should use the shared closure-graph stale-target token"
    );

    let mut status = runtime_state.status.clone();
    status.execution_reentry_target_source = source;

    assert_eq!(
        select_route_projected_stale_boundary_task(&status),
        Some(5),
        "status bridge should preserve the route target when the shared closure-graph token is present"
    );
}

#[test]
fn non_closure_stale_reentry_source_token_flows_from_selected_target() {
    let root = std::env::temp_dir().join("featureforge-gate-review-source-token");
    let mut runtime_state = completed_stale_target_runtime_state(&root);
    runtime_state.gate_snapshot.stale_targets[0].source =
        AuthoritativeStaleTargetSource::GateReview;
    let authoritative_stale_target = runtime_state
        .gate_snapshot
        .stale_targets
        .first()
        .expect("fixture should include an authoritative stale target");

    let source = execution_reentry_target_source_for_route(
        &runtime_state,
        &runtime_state.status,
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
        NextActionAuthorityInputs {
            route_repair_target_candidates: &runtime_state.route_repair_target_candidates,
            has_authoritative_stale_target: true,
            authoritative_stale_target: AuthoritativeStaleReentryTarget::from_stale_target(
                authoritative_stale_target,
            ),
            ..Default::default()
        },
    );

    assert_eq!(
        source.as_deref(),
        Some("gate_review"),
        "selected stale reentry target should carry the original non-closure stale source"
    );
}

#[test]
fn stale_task_record_ids_do_not_count_as_concrete_public_targets() {
    let root = std::env::temp_dir().join("featureforge-stale-task-record-id-target");
    let mut runtime_state = completed_stale_target_runtime_state(&root);
    runtime_state.gate_snapshot.stale_targets = vec![AuthoritativeStaleTarget {
        scope: AuthoritativeStaleTargetScope::Task,
        task: Some(5),
        step: None,
        record_id: Some(String::from("closure-5-stale")),
        source: AuthoritativeStaleTargetSource::BaselineBridge,
        reason_code: String::from(REVIEW_STATE_STALE_UNREVIEWED),
        task_closure_bridge_allowed: true,
    }];
    runtime_state.status.blocking_scope = Some(String::from("task"));
    runtime_state.status.blocking_task = Some(5);
    push_status_stale_unreviewed_closure(&mut runtime_state.status, "closure-5-stale");
    runtime_state.status.blocking_records = vec![review_state_blocking_record(
        REVIEW_STATE_STALE_UNREVIEWED,
        "closure-5-stale",
        Some(FOLLOW_UP_REPAIR_REVIEW_STATE),
    )];

    let decision = route_decision_from_runtime_state_with_inputs(&runtime_state, false, false)
        .expect("route decision should project");

    assert_eq!(
        decision.phase_detail,
        phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
    );
    assert!(
        decision.recommended_public_command.is_none()
            && decision.invocation.is_none()
            && decision.public_repair_targets.is_empty(),
        "stale closure record ids are diagnostic identities, not executable task-scope targets: {decision:?}"
    );
}

fn push_status_stale_unreviewed_closure(status: &mut PlanExecutionStatus, record_id: &str) {
    status
        .stale_unreviewed_closures
        .push(String::from(record_id));
}

fn review_state_blocking_record(
    code: &str,
    scope_key: &str,
    required_follow_up: Option<&str>,
) -> StatusBlockingRecord {
    StatusBlockingRecord {
        code: String::from(code),
        scope_type: String::from("task"),
        scope_key: String::from(scope_key),
        record_type: String::from("review_state"),
        record_id: Some(String::from(scope_key)),
        review_state_status: String::from(REVIEW_STATE_STALE_UNREVIEWED),
        required_follow_up: required_follow_up.map(String::from),
        message: String::from("test blocking record"),
    }
}

fn resume_stale_precedence_for_status(
    status: &PlanExecutionStatus,
    exact_resume_stale_task_target: Option<u32>,
    legal_resume_begin_route: bool,
) -> ResumeStalePrecedence {
    ResumeStalePrecedence::from_inputs(ResumeStalePrecedenceInputs {
        status,
        review_state_status: REVIEW_STATE_STALE_UNREVIEWED,
        open_step_task: None,
        authoritative_stale_boundary: None,
        baseline_stale_boundary_task: None,
        exact_resume_stale_task_target,
        stale_preemption_target: None,
        legal_resume_begin_route,
        targetless_stale_has_concrete_public_target: true,
    })
}

#[test]
fn projected_stale_repair_record_task_ignores_structural_task_records() {
    let mut status = closed_stale_provenance_status();
    status.blocking_task = Some(7);
    status.blocking_records = vec![review_state_blocking_record(
        crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED,
        "task-7",
        Some(FOLLOW_UP_REPAIR_REVIEW_STATE),
    )];

    assert_eq!(
        projected_stale_repair_record_task(&status),
        None,
        "targetless-stale route finalization must not treat structural task blockers as stale repair targets"
    );
}

#[test]
fn projected_stale_repair_record_task_accepts_stale_task_records() {
    let mut status = closed_stale_provenance_status();
    status.blocking_records = vec![
        review_state_blocking_record(
            REVIEW_STATE_STALE_UNREVIEWED,
            "task-4",
            Some(FOLLOW_UP_REPAIR_REVIEW_STATE),
        ),
        review_state_blocking_record(
            REVIEW_STATE_STALE_UNREVIEWED,
            "task-2",
            Some(FOLLOW_UP_REPAIR_REVIEW_STATE),
        ),
    ];

    assert_eq!(
        projected_stale_repair_record_task(&status),
        Some(2),
        "targetless-stale route finalization may convert only task-scoped stale review-state records"
    );
}

#[test]
fn resume_stale_precedence_exact_binding_ignores_structural_task_records() {
    let mut status = closed_stale_provenance_status();
    status.resume_task = Some(7);
    status.resume_step = Some(1);
    status.blocking_records = vec![review_state_blocking_record(
        crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_REVIEWED_STATE_MALFORMED,
        "task-7",
        Some(FOLLOW_UP_REPAIR_REVIEW_STATE),
    )];

    assert_eq!(
        resume_stale_precedence_for_status(&status, None, true).exact_resume_stale_task,
        None,
        "exact resume stale binding must ignore structural review-state task records"
    );
}

#[test]
fn resume_stale_precedence_exact_binding_requires_stale_repair_follow_up() {
    let mut status = closed_stale_provenance_status();
    status.resume_task = Some(7);
    status.resume_step = Some(1);
    status.blocking_records = vec![review_state_blocking_record(
        REVIEW_STATE_STALE_UNREVIEWED,
        "task-7",
        None,
    )];

    assert_eq!(
        resume_stale_precedence_for_status(&status, None, true).exact_resume_stale_task,
        None,
        "exact resume stale binding must require a repair-review-state stale task record"
    );
}

#[test]
fn route_planning_facts_derive_baseline_bridge_without_route_projection_phase() {
    let mut status = closed_stale_provenance_status();
    status.phase_detail.clear();
    status.review_state_status = String::from("clean");
    status.blocking_task = Some(2);
    status.reason_codes = vec![
        String::from(crate::execution::reentry_reconcile::TARGETLESS_STALE_RECONCILE_REASON_CODE),
        String::from(
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE,
        ),
    ];

    let facts = RoutePlanningFacts::from_inputs(RoutePlanningFactInputs {
        status: &status,
        review_state_status: String::from("clean"),
        earliest_stale_task_target: Some(2),
        legal_resume_begin_route: false,
        authoritative_stale_target_bound: false,
        actionable_stale_reentry_target_bound: false,
        baseline_bridge_repair_review_state_ready: true,
        baseline_bridge_close_current_task_candidate: Some(2),
        baseline_bridge_execution_reentry_task: None,
        execution_reentry_task_closure_bridge_facts:
            ExecutionReentryTaskClosureBridgeFacts::default(),
        execution_reentry_target_source: None,
        completed_task_closure_preemption_tasks: Default::default(),
        fallback_completed_task_closure_preemption_task: None,
        persisted_close_current_task_bridge_task: None,
        persisted_reopen_target: None,
        persisted_repair_follow_up: Some(FOLLOW_UP_REPAIR_REVIEW_STATE),
        current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts::inactive(),
    });

    assert!(
        facts.targetless_stale_reconcile_required,
        "route-planning facts should detect targetless stale reconcile before route projection"
    );
    assert!(
        facts.baseline_bridge_repair_review_state_ready,
        "baseline-bridge route facts must not require route-projected phase_detail"
    );
    assert_eq!(facts.baseline_bridge_close_current_task_candidate, Some(2));
    assert_eq!(
        facts.persisted_repair_follow_up(),
        Some(FOLLOW_UP_REPAIR_REVIEW_STATE)
    );
}

#[test]
fn persisted_execution_reentry_fallback_is_selected_before_finalization() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut runtime_state = completed_stale_target_runtime_state(temp.path());
    runtime_state.status = closed_stale_provenance_status();
    runtime_state.gate_snapshot.stale_reason_codes.clear();
    runtime_state.gate_snapshot.stale_targets.clear();
    runtime_state.persisted_repair_follow_up = Some(String::from(FOLLOW_UP_EXECUTION_REENTRY));
    runtime_state.route_repair_target_candidates = vec![PublicRepairTarget {
        command_kind: PublicCommandKind::Reopen.public_mutation_token().to_owned(),
        task: Some(5),
        step: Some(3),
        reason_code: PublicRepairTargetReason::PersistedExecutionReentryFollowUp.reason_code(),
        source_record_id: Some(String::from("persisted-reentry-target")),
        expires_when_fingerprint_changes: true,
    }];

    let authority_inputs = super::next_action_authority_inputs_for_route_plan(
        &runtime_state,
        &runtime_state.status,
        None,
    );
    let route_facts = super::route_planning_authority_for_status(
        &runtime_state,
        &runtime_state.status,
        authority_inputs,
    );
    let route_decision = super::select_runtime_route_decision(
        &runtime_state,
        &route_facts,
        authority_inputs,
        false,
        false,
    );
    assert_eq!(
        route_decision
            .recommended_public_command
            .as_ref()
            .map(PublicCommand::kind),
        Some(PublicCommandKind::Reopen),
        "persisted execution reentry must be selected directly by route planning"
    );
    assert_eq!(
        route_decision.phase_detail,
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED
    );
    assert_eq!(
        route_decision.required_follow_up.as_deref(),
        Some(FOLLOW_UP_EXECUTION_REENTRY)
    );
    assert!(
        route_decision.blocking_reason_codes.iter().any(|reason| {
            PublicRepairTargetReason::PersistedExecutionReentryFollowUp.matches(reason)
        }),
        "persisted execution reentry remains part of the public diagnostic contract"
    );

    let route_argv_before_finalization = route_decision.public_command_argv();
    let route_status =
        super::status_for_route_plan_projection(&runtime_state, &route_decision, None)
            .expect("route status projection should succeed");
    let route_after_finalization = super::finalize_route_decision_for_route_plan(
        route_decision,
        &route_status,
        &runtime_state,
        false,
    );

    assert_eq!(
        route_after_finalization.public_command_argv(),
        route_argv_before_finalization,
        "route finalization must not select a different mutation command"
    );
    assert_eq!(
        route_after_finalization
            .recommended_public_command
            .as_ref()
            .map(PublicCommand::kind),
        Some(PublicCommandKind::Reopen)
    );
    assert_eq!(
        route_after_finalization.execution_command_context.as_ref(),
        Some(&ExecutionRoutingExecutionCommandContext {
            command_kind: String::from("reopen"),
            task_number: Some(5),
            step_id: Some(3),
        })
    );
}

#[test]
fn persisted_execution_reentry_fallback_does_not_override_begin_or_reopen() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let runtime_state = completed_stale_target_runtime_state(temp.path());
    let route_facts = RoutePlanningFacts::from_inputs(RoutePlanningFactInputs {
        status: &runtime_state.status,
        review_state_status: String::from(REVIEW_STATE_STALE_UNREVIEWED),
        earliest_stale_task_target: None,
        legal_resume_begin_route: false,
        authoritative_stale_target_bound: false,
        actionable_stale_reentry_target_bound: false,
        baseline_bridge_repair_review_state_ready: false,
        baseline_bridge_close_current_task_candidate: None,
        baseline_bridge_execution_reentry_task: None,
        execution_reentry_task_closure_bridge_facts:
            ExecutionReentryTaskClosureBridgeFacts::default(),
        execution_reentry_target_source: None,
        completed_task_closure_preemption_tasks: Default::default(),
        fallback_completed_task_closure_preemption_task: None,
        persisted_close_current_task_bridge_task: None,
        persisted_reopen_target: Some(PersistedReopenTarget {
            task_number: 5,
            step_number: 3,
        }),
        persisted_repair_follow_up: Some(FOLLOW_UP_EXECUTION_REENTRY),
        current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts::inactive(),
    });

    let commands = [
        PublicCommand::Begin {
            plan: String::from("docs/featureforge/plans/plan.md"),
            task: 5,
            step: 3,
            execution_mode: Some(String::from("featureforge:executing-plans")),
            fingerprint: Some(String::from("fingerprint")),
        },
        PublicCommand::Reopen {
            plan: String::from("docs/featureforge/plans/plan.md"),
            task: 5,
            step: 3,
            source: Some(String::from("featureforge:executing-plans")),
            reason: Some(String::from("execution_reentry")),
            fingerprint: Some(String::from("fingerprint")),
        },
    ];

    for command in commands {
        let (recommended_command, invocation, template, required_inputs) =
            RouteDecision::command_surfaces(Some(&command));
        let mut selected_route = release_readiness_route_decision();
        selected_route.recommended_command = recommended_command;
        selected_route.recommended_public_command = Some(command.clone());
        selected_route.invocation = invocation;
        selected_route.recommended_public_command_template = template;
        selected_route.required_inputs = required_inputs;
        let selected_argv = selected_route.public_command_argv();

        let chosen_route =
            super::select_route_planning_candidate(selected_route, &runtime_state, &route_facts);

        assert_eq!(
            chosen_route.public_command_argv(),
            selected_argv,
            "persisted execution-reentry fallback must not override an already-selected legal {:?} route",
            command.kind()
        );
    }
}

#[test]
fn route_planning_facts_do_not_infer_baseline_bridge_without_ready_candidate() {
    let mut status = closed_stale_provenance_status();
    status.phase_detail.clear();
    status.review_state_status = String::from("clean");
    status.blocking_task = Some(2);
    status.reason_codes = vec![
        String::from(crate::execution::reentry_reconcile::TARGETLESS_STALE_RECONCILE_REASON_CODE),
        String::from(
            crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE,
        ),
    ];

    let facts = RoutePlanningFacts::from_inputs(RoutePlanningFactInputs {
        status: &status,
        review_state_status: String::from("clean"),
        earliest_stale_task_target: Some(2),
        legal_resume_begin_route: false,
        authoritative_stale_target_bound: false,
        actionable_stale_reentry_target_bound: false,
        baseline_bridge_repair_review_state_ready: false,
        baseline_bridge_close_current_task_candidate: None,
        baseline_bridge_execution_reentry_task: None,
        execution_reentry_task_closure_bridge_facts:
            ExecutionReentryTaskClosureBridgeFacts::default(),
        execution_reentry_target_source: None,
        completed_task_closure_preemption_tasks: Default::default(),
        fallback_completed_task_closure_preemption_task: None,
        persisted_close_current_task_bridge_task: None,
        persisted_reopen_target: None,
        persisted_repair_follow_up: None,
        current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts::inactive(),
    });

    assert!(
        !facts.baseline_bridge_repair_review_state_ready,
        "baseline bridge route facts should trust the shared readiness owner instead of re-inferring readiness from reason codes"
    );
}

#[test]
fn route_planning_facts_use_canonical_review_state_for_stale_booleans() {
    let mut status = closed_stale_provenance_status();
    status.review_state_status = String::from("clean");
    status.blocking_scope = Some(String::from("task"));
    status.blocking_task = Some(2);
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status
        .stale_unreviewed_closures
        .push(String::from("task-2-stale"));

    let facts = RoutePlanningFacts::from_inputs(RoutePlanningFactInputs {
        status: &status,
        review_state_status: String::from(REVIEW_STATE_STALE_UNREVIEWED),
        earliest_stale_task_target: Some(2),
        legal_resume_begin_route: true,
        authoritative_stale_target_bound: true,
        actionable_stale_reentry_target_bound: false,
        baseline_bridge_repair_review_state_ready: false,
        baseline_bridge_close_current_task_candidate: None,
        baseline_bridge_execution_reentry_task: None,
        execution_reentry_task_closure_bridge_facts:
            ExecutionReentryTaskClosureBridgeFacts::default(),
        execution_reentry_target_source: None,
        completed_task_closure_preemption_tasks: Default::default(),
        fallback_completed_task_closure_preemption_task: None,
        persisted_close_current_task_bridge_task: None,
        persisted_reopen_target: None,
        persisted_repair_follow_up: None,
        current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts::inactive(),
    });

    assert!(facts.stale_task_scope_lacks_concrete_public_target);
    assert!(facts.stale_resume_begin_route_candidate);
}

#[test]
fn route_planning_facts_bind_exact_resume_from_earliest_stale_target() {
    let mut status = closed_stale_provenance_status();
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status.blocking_task = Some(2);
    status.blocking_records.clear();

    let matching_facts = RoutePlanningFacts::from_inputs(RoutePlanningFactInputs {
        status: &status,
        review_state_status: String::from(REVIEW_STATE_STALE_UNREVIEWED),
        earliest_stale_task_target: Some(2),
        legal_resume_begin_route: true,
        authoritative_stale_target_bound: false,
        actionable_stale_reentry_target_bound: false,
        baseline_bridge_repair_review_state_ready: false,
        baseline_bridge_close_current_task_candidate: None,
        baseline_bridge_execution_reentry_task: None,
        execution_reentry_task_closure_bridge_facts:
            ExecutionReentryTaskClosureBridgeFacts::default(),
        execution_reentry_target_source: None,
        completed_task_closure_preemption_tasks: Default::default(),
        fallback_completed_task_closure_preemption_task: None,
        persisted_close_current_task_bridge_task: None,
        persisted_reopen_target: None,
        persisted_repair_follow_up: None,
        current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts::inactive(),
    });
    let mismatched_facts = RoutePlanningFacts::from_inputs(RoutePlanningFactInputs {
        status: &status,
        review_state_status: String::from(REVIEW_STATE_STALE_UNREVIEWED),
        earliest_stale_task_target: Some(1),
        legal_resume_begin_route: true,
        authoritative_stale_target_bound: false,
        actionable_stale_reentry_target_bound: false,
        baseline_bridge_repair_review_state_ready: false,
        baseline_bridge_close_current_task_candidate: None,
        baseline_bridge_execution_reentry_task: None,
        execution_reentry_task_closure_bridge_facts:
            ExecutionReentryTaskClosureBridgeFacts::default(),
        execution_reentry_target_source: None,
        completed_task_closure_preemption_tasks: Default::default(),
        fallback_completed_task_closure_preemption_task: None,
        persisted_close_current_task_bridge_task: None,
        persisted_reopen_target: None,
        persisted_repair_follow_up: None,
        current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts::inactive(),
    });

    assert_eq!(matching_facts.exact_resume_stale_task, Some(2));
    assert_eq!(
        mismatched_facts.exact_resume_stale_task, None,
        "resume_task is diagnostic unless it matches the earliest stale reducer target"
    );
    assert!(
        matching_facts.stale_resume_begin_route_candidate,
        "matching task-scoped stale target may bind parked resume begin"
    );
    assert!(
        !mismatched_facts.stale_resume_begin_route_candidate,
        "mismatched task-scoped stale target must not bind parked resume begin even when status.blocking_task matches resume_task"
    );
}

#[test]
fn fingerprint_bound_begin_route_rejects_resume_fields_without_public_authority() {
    let mut status = closed_stale_provenance_status();
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status.execution_fingerprint = String::from("fingerprint-bound-route");
    let target = ExecutionCommandRouteTarget {
        kind: PublicCommandKind::Begin,
        task_number: 2,
        step_id: Some(1),
    };

    assert!(
        !execution_command_route_target_matches_public_status(&status, &target),
        "resume fields alone must not satisfy public route authority"
    );
    assert!(
        !fingerprint_bound_begin_route_matches_public_status(&status, &target),
        "fingerprint-bound resume fields alone must not satisfy begin authority"
    );

    status.execution_fingerprint.clear();
    assert!(
        !fingerprint_bound_begin_route_matches_public_status(&status, &target),
        "resume fields without a fingerprint must not satisfy begin authority"
    );

    status.execution_fingerprint = String::from("fingerprint-bound-route");
    status.execution_command_context = Some(PublicExecutionCommandContext {
        command_kind: String::from("begin"),
        task_number: Some(2),
        step_id: Some(1),
    });
    assert!(
        fingerprint_bound_begin_route_matches_public_status(&status, &target),
        "route-owned execution command context plus fingerprint should satisfy begin authority"
    );

    status.execution_fingerprint.clear();
    assert!(
        !fingerprint_bound_begin_route_matches_public_status(&status, &target),
        "begin authority must be fingerprint-bound"
    );

    status.execution_fingerprint = String::from("fingerprint-bound-route");
    status.execution_command_context = None;
    status.resume_task = None;
    status.resume_step = None;
    status.blocking_task = Some(2);
    status.blocking_step = Some(1);
    assert!(
        fingerprint_bound_begin_route_matches_public_status(&status, &target),
        "authoritative blocked open-step status plus fingerprint should satisfy begin authority"
    );
}

#[test]
fn authoritative_run_identity_authorizes_exact_resume_begin_route() {
    let mut status = closed_stale_provenance_status();
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status.execution_run_id = Some(ExecutionRunId(String::from("run-authority-2-1")));
    status.execution_fingerprint = String::from("fingerprint-bound-route");
    let target = ExecutionCommandRouteTarget {
        kind: PublicCommandKind::Begin,
        task_number: 2,
        step_id: Some(1),
    };

    assert!(
        fingerprint_bound_begin_route_matches_public_status(&status, &target),
        "authoritative run identity plus matching resume task/step should satisfy exact begin authority"
    );
    assert!(
        legal_execution_begin_route(&status, "docs/featureforge/plans/plan.md", &[]),
        "legal begin route should not require reopen repair-target authority when run identity is authoritative"
    );

    status.execution_run_id = None;
    assert!(
        !fingerprint_bound_begin_route_matches_public_status(&status, &target),
        "without authoritative run identity, resume fields remain diagnostic"
    );
}

#[test]
fn reopen_repair_target_does_not_authorize_resume_begin_route() {
    let mut status = closed_stale_provenance_status();
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status.execution_fingerprint = String::from("fingerprint-bound-route");
    status.public_repair_targets = vec![PublicRepairTarget {
        command_kind: String::from(PublicCommandKind::Reopen.public_mutation_token()),
        task: Some(2),
        step: Some(1),
        reason_code: PublicRepairTargetReason::RouteExecutionReentryRequired.reason_code(),
        source_record_id: Some(String::from("reopen-target-2-1")),
        expires_when_fingerprint_changes: true,
    }];
    let target = ExecutionCommandRouteTarget {
        kind: PublicCommandKind::Begin,
        task_number: 2,
        step_id: Some(1),
    };

    assert!(
        !fingerprint_bound_begin_route_matches_public_status(&status, &target),
        "a fingerprint-bound reopen repair target must not satisfy begin authority"
    );
    let route_repair_target_candidates = status.public_repair_targets.clone();
    let legal_begin = legal_execution_begin_route(
        &status,
        "docs/featureforge/plans/plan.md",
        &route_repair_target_candidates,
    );
    assert!(
        !legal_begin,
        "route-local reopen candidates must not authorize resume begin"
    );

    let facts = RoutePlanningFacts::from_inputs(RoutePlanningFactInputs {
        status: &status,
        review_state_status: String::from(REVIEW_STATE_STALE_UNREVIEWED),
        earliest_stale_task_target: Some(2),
        legal_resume_begin_route: legal_begin,
        authoritative_stale_target_bound: false,
        actionable_stale_reentry_target_bound: false,
        baseline_bridge_repair_review_state_ready: false,
        baseline_bridge_close_current_task_candidate: None,
        baseline_bridge_execution_reentry_task: None,
        execution_reentry_task_closure_bridge_facts:
            ExecutionReentryTaskClosureBridgeFacts::default(),
        execution_reentry_target_source: None,
        completed_task_closure_preemption_tasks: Default::default(),
        fallback_completed_task_closure_preemption_task: None,
        persisted_close_current_task_bridge_task: None,
        persisted_reopen_target: None,
        persisted_repair_follow_up: None,
        current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts::inactive(),
    });

    assert_eq!(
        facts.exact_resume_stale_task,
        Some(2),
        "exact stale binding may remain diagnostic for the matching resume task"
    );
    assert!(
        !facts.stale_resume_begin_route_candidate,
        "a matching resume/stale diagnostic must not become executable through a reopen repair target"
    );
}

fn begin_route_candidate(task: u32, step: u32, fingerprint_bound: bool) -> PublicRepairTarget {
    PublicRepairTarget {
        command_kind: String::from(PublicCommandKind::Begin.public_mutation_token()),
        task: Some(task),
        step: Some(step),
        reason_code: PublicRepairTargetReason::RouteExecutionReentryRequired.reason_code(),
        source_record_id: Some(format!("route-candidate-begin-{task}-{step}")),
        expires_when_fingerprint_changes: fingerprint_bound,
    }
}

#[test]
fn route_repair_candidate_can_authorize_matching_resume_begin_before_status_projection() {
    let mut status = closed_stale_provenance_status();
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.state_kind = String::from("actionable_public_command");
    status.execution_fingerprint = String::from("fingerprint-bound-route");
    status.execution_run_id = None;
    status.execution_command_context = None;
    status.public_repair_targets = Vec::new();
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status.blocking_task = None;
    status.blocking_step = None;
    let target = ExecutionCommandRouteTarget {
        kind: PublicCommandKind::Begin,
        task_number: 2,
        step_id: Some(1),
    };

    assert!(
        !fingerprint_bound_begin_route_matches_public_status(&status, &target),
        "resume diagnostics alone must not authorize begin"
    );
    assert!(
        !legal_execution_begin_route(&status, "docs/featureforge/plans/plan.md", &[]),
        "resume diagnostics alone must not become a legal begin route"
    );

    let candidates = vec![begin_route_candidate(2, 1, true)];
    assert!(
        execution_command_route_target_has_authority(&status, &target, &candidates),
        "a matching fingerprint-bound route candidate should authorize the exact begin target before status publishes it"
    );
    assert!(
        legal_execution_begin_route(&status, "docs/featureforge/plans/plan.md", &candidates),
        "legal begin route should consume matching route repair candidates"
    );
}

#[test]
fn begin_route_candidate_authority_remains_fingerprint_and_status_bound() {
    let mut status = closed_stale_provenance_status();
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.state_kind = String::from("actionable_public_command");
    status.execution_fingerprint = String::from("fingerprint-bound-route");
    status.execution_run_id = None;
    status.execution_command_context = None;
    status.public_repair_targets = Vec::new();
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status.blocking_task = None;
    status.blocking_step = None;
    let target = ExecutionCommandRouteTarget {
        kind: PublicCommandKind::Begin,
        task_number: 2,
        step_id: Some(1),
    };
    let non_fingerprint_bound_candidates = vec![begin_route_candidate(2, 1, false)];
    assert!(
        !execution_command_route_target_has_authority(
            &status,
            &target,
            &non_fingerprint_bound_candidates,
        ),
        "begin route candidate authority must require fingerprint-bound candidates"
    );

    let fingerprint_bound_candidates = vec![begin_route_candidate(2, 1, true)];
    status.execution_fingerprint.clear();
    assert!(
        !execution_command_route_target_has_authority(
            &status,
            &target,
            &fingerprint_bound_candidates
        ),
        "begin route candidate authority must require a non-empty status execution fingerprint"
    );

    status.execution_fingerprint = String::from("fingerprint-bound-route");
    status.phase_detail = String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
    assert!(
        !execution_command_route_target_has_authority(
            &status,
            &target,
            &fingerprint_bound_candidates
        ),
        "runtime reconcile must block route-candidate begin authority"
    );
}

#[test]
fn legal_begin_route_rejects_authoritative_non_begin_route() {
    let mut status = closed_stale_provenance_status();
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.state_kind = String::from("actionable_public_command");
    status.execution_started = String::from("yes");
    status.active_task = Some(1);
    status.active_step = Some(1);
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status.execution_fingerprint = String::from("fingerprint-bound-route");
    status.execution_command_context = Some(PublicExecutionCommandContext {
        command_kind: String::from("complete"),
        task_number: Some(1),
        step_id: Some(1),
    });
    let complete_target = ExecutionCommandRouteTarget {
        kind: PublicCommandKind::Complete,
        task_number: 1,
        step_id: Some(1),
    };

    assert!(
        execution_command_route_target_matches_public_status(&status, &complete_target),
        "fixture must contain a real executable non-begin public route"
    );
    assert!(
        !legal_execution_begin_route(&status, "docs/featureforge/plans/plan.md", &[]),
        "a legal non-begin route must not satisfy the legal begin predicate"
    );

    let precedence = ResumeStalePrecedence::from_inputs(ResumeStalePrecedenceInputs {
        status: &status,
        review_state_status: REVIEW_STATE_STALE_UNREVIEWED,
        open_step_task: status.active_task,
        authoritative_stale_boundary: None,
        baseline_stale_boundary_task: None,
        exact_resume_stale_task_target: None,
        stale_preemption_target: Some(StalePreemptionTarget {
            task: 3,
            step: Some(1),
        }),
        legal_resume_begin_route: legal_execution_begin_route(
            &status,
            "docs/featureforge/plans/plan.md",
            &[],
        ),
        targetless_stale_has_concrete_public_target: true,
    });

    assert!(
        precedence.stale_preempted_by_resume.is_none(),
        "raw resume fields must not preempt stale routing when the executable public route is complete"
    );
}

#[test]
fn task_boundary_blocking_task_rejects_resume_only_boundary_reason() {
    let mut status = closed_stale_provenance_status();
    status.blocking_task = None;
    status.blocking_step = None;
    status.active_task = None;
    status.active_step = None;
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status.reason_codes = vec![String::from(
        crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE,
    )];

    assert_eq!(
        task_boundary_blocking_task(&status),
        None,
        "task-boundary reentry target selection must not promote raw resume fields"
    );

    status.blocking_task = Some(2);
    assert_eq!(
        task_boundary_blocking_task(&status),
        Some(2),
        "task-boundary selection may still use the explicit blocking task"
    );
}

#[test]
fn execution_reentry_target_rejects_resume_exact_route_without_legal_begin_authority() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let context = completed_stale_target_route_plan_context(temp.path());
    let mut status = closed_stale_provenance_status();
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.state_kind = String::from("actionable_public_command");
    status.execution_started = String::from("yes");
    status.active_task = None;
    status.active_step = None;
    status.blocking_task = None;
    status.blocking_step = None;
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status.execution_command_context = None;
    status.public_repair_targets.clear();
    status.reason_codes.clear();
    status.blocking_reason_codes.clear();
    status.stale_unreviewed_closures.clear();
    status.blocking_records.clear();

    assert_eq!(
        execution_reentry_target(
            &context,
            &status,
            "docs/featureforge/plans/plan.md",
            NextActionAuthorityInputs::default()
        ),
        None,
        "loose resume-shaped status must not reach the exact-route fallback"
    );

    status.execution_command_context = Some(PublicExecutionCommandContext {
        command_kind: String::from("begin"),
        task_number: Some(2),
        step_id: Some(1),
    });
    let target = execution_reentry_target(
        &context,
        &status,
        "docs/featureforge/plans/plan.md",
        NextActionAuthorityInputs::default(),
    )
    .expect("fingerprint-bound begin context should allow resume reentry");
    assert_eq!(target.task, 2);
    assert_eq!(target.step, Some(1));
    assert_eq!(target.source, ExecutionReentryTargetSource::ResumeStep);
}

#[test]
fn resume_stale_precedence_requires_legal_begin_for_executable_resume_binding() {
    let mut status = closed_stale_provenance_status();
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status.blocking_task = Some(2);
    status.blocking_records.clear();

    let precedence = resume_stale_precedence_for_status(&status, Some(2), false);

    assert_eq!(
        precedence.exact_resume_stale_task,
        Some(2),
        "exact stale binding may still carry the diagnostic stale task target"
    );
    assert!(
        !precedence.stale_resume_begin_route_candidate,
        "route planning must not promote exact resume/stale binding without legal begin authority"
    );
}

#[test]
fn resume_stale_precedence_requires_legal_begin_for_stale_preempted_by_resume() {
    let mut status = closed_stale_provenance_status();
    status.resume_task = Some(1);
    status.resume_step = Some(1);
    status.blocking_task = Some(2);

    let stale_preemption_target = Some(StalePreemptionTarget {
        task: 2,
        step: None,
    });
    let illegal_precedence = ResumeStalePrecedence::from_inputs(ResumeStalePrecedenceInputs {
        status: &status,
        review_state_status: REVIEW_STATE_STALE_UNREVIEWED,
        open_step_task: None,
        authoritative_stale_boundary: None,
        baseline_stale_boundary_task: None,
        exact_resume_stale_task_target: None,
        stale_preemption_target,
        legal_resume_begin_route: false,
        targetless_stale_has_concrete_public_target: true,
    });
    let legal_precedence = ResumeStalePrecedence::from_inputs(ResumeStalePrecedenceInputs {
        status: &status,
        review_state_status: REVIEW_STATE_STALE_UNREVIEWED,
        open_step_task: None,
        authoritative_stale_boundary: None,
        baseline_stale_boundary_task: None,
        exact_resume_stale_task_target: None,
        stale_preemption_target,
        legal_resume_begin_route: true,
        targetless_stale_has_concrete_public_target: true,
    });

    assert!(
        illegal_precedence.stale_preempted_by_resume.is_none(),
        "earlier resume fields must not preempt stale routing unless begin is legal"
    );
    assert_eq!(
        legal_precedence
            .stale_preempted_by_resume
            .map(|binding| binding.task),
        Some(1),
        "legal earlier begin remains able to preempt a later stale target"
    );
}

#[test]
fn route_planning_facts_do_not_bind_resume_for_targetless_stale_boundaries() {
    let mut status = closed_stale_provenance_status();
    status.resume_task = Some(2);
    status.resume_step = Some(1);
    status.blocking_task = None;
    status.blocking_step = None;
    status.blocking_records.clear();
    status.reason_codes = vec![String::from(
        crate::execution::reentry_reconcile::TARGETLESS_STALE_RECONCILE_REASON_CODE,
    )];
    status
        .stale_unreviewed_closures
        .push(String::from("branch-stale-without-task-target"));

    let facts = RoutePlanningFacts::from_inputs(RoutePlanningFactInputs {
        status: &status,
        review_state_status: String::from(REVIEW_STATE_STALE_UNREVIEWED),
        earliest_stale_task_target: None,
        legal_resume_begin_route: false,
        authoritative_stale_target_bound: true,
        actionable_stale_reentry_target_bound: false,
        baseline_bridge_repair_review_state_ready: false,
        baseline_bridge_close_current_task_candidate: None,
        baseline_bridge_execution_reentry_task: None,
        execution_reentry_task_closure_bridge_facts:
            ExecutionReentryTaskClosureBridgeFacts::default(),
        execution_reentry_target_source: None,
        completed_task_closure_preemption_tasks: Default::default(),
        fallback_completed_task_closure_preemption_task: None,
        persisted_close_current_task_bridge_task: None,
        persisted_reopen_target: None,
        persisted_repair_follow_up: None,
        current_task_closure_branch_route_facts: CurrentTaskClosureBranchRouteFacts::inactive(),
    });

    assert!(
        !facts.stale_resume_begin_route_candidate,
        "targetless stale boundaries must not make parked resume_task executable"
    );
    assert_eq!(
        facts.exact_resume_stale_task, None,
        "resume_task remains diagnostic when no exact stale task target exists"
    );
    assert!(
        facts.targetless_stale_reconcile_required,
        "targetless stale route must remain on runtime reconcile instead of parked begin"
    );
}

#[test]
fn task_closure_recording_source_labels_baseline_bridge_seed_routes() {
    let source = task_closure_recording_reentry_target_source(
        phase::DETAIL_TASK_CLOSURE_RECORDING_READY,
        &[
            String::from(
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING,
            ),
            String::from(
                crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE,
            ),
        ],
    );

    assert_eq!(
        source.as_deref(),
        Some("baseline_bridge"),
        "task_closure_recording_ready routes must preserve the route-owned baseline-bridge target source"
    );
}

#[test]
fn route_plan_closes_completed_stale_target_before_reopening_same_step() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let runtime_state = completed_stale_target_runtime_state(temp.path());

    let decision = route_decision_from_runtime_state_with_inputs(&runtime_state, false, false)
        .expect("route decision should project");

    assert_eq!(decision.next_action, NEXT_ACTION_CLOSE_CURRENT_TASK);
    assert_eq!(
        decision.phase_detail,
        phase::DETAIL_TASK_CLOSURE_RECORDING_READY
    );
    assert_eq!(decision.review_state_status, REVIEW_STATE_STALE_UNREVIEWED);
    assert!(
        decision.public_command_argv().is_none(),
        "unbound close-current-task routes require a typed template until review and verification inputs are supplied: {decision:#?}"
    );
    let template = decision
        .public_command_template()
        .expect("close-current-task route should expose a bindable typed template");
    assert_eq!(template.command_kind, "close_current_task");
    assert_eq!(
        template.base_argv,
        vec![
            String::from("featureforge"),
            String::from("plan"),
            String::from("execution"),
            String::from("close-current-task"),
            String::from("--plan"),
            String::from("docs/featureforge/plans/plan.md"),
            String::from("--task"),
            String::from("5"),
        ],
        "route_plan, not next_action, must convert the completed stale-target seed into the public close route"
    );
    assert_eq!(
        template.required_input_names,
        vec![
            String::from("review_result"),
            String::from("review_summary_file"),
            String::from("verification_result"),
            String::from("verification_summary_file"),
        ]
    );
}

#[test]
fn runtime_owned_current_task_closure_surface_does_not_enable_branch_closure_rerecording() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut runtime_state = completed_stale_target_runtime_state(temp.path());
    runtime_state.gate_snapshot.stale_targets.clear();
    runtime_state.gate_snapshot.stale_reason_codes.clear();
    runtime_state.status.current_branch_closure_id = None;
    runtime_state.status.current_task_closures = vec![PublicReviewStateTaskClosure {
        task: 5,
        closure_record_id: String::from("task-closure-runtime-owned-only"),
        reviewed_state_id: String::from("git_tree:current"),
        contract_identity: String::from("task-contract-5"),
        effective_reviewed_surface_paths: vec![String::from(
            "docs/featureforge/execution-evidence/plan-r1-evidence.md",
        )],
    }];
    runtime_state.status.harness_phase = HarnessPhase::DocumentReleasePending;
    runtime_state.status.phase = Some(String::from(phase::PHASE_DOCUMENT_RELEASE_PENDING));
    runtime_state.status.phase_detail =
        String::from(phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS);
    runtime_state.status.review_state_status = String::from(REVIEW_STATE_MISSING_CURRENT_CLOSURE);
    runtime_state.status.reason_codes = vec![String::from(REVIEW_STATE_MISSING_CURRENT_CLOSURE)];
    runtime_state.status.blocking_reason_codes =
        vec![String::from(REVIEW_STATE_MISSING_CURRENT_CLOSURE)];
    runtime_state.status.next_action = String::from(NEXT_ACTION_ADVANCE_LATE_STAGE);
    runtime_state.branch_rerecording_assessment = Some(BranchRerecordingAssessment {
        changed_paths: vec![String::from("README.md")],
        late_stage_surface: vec![String::from("README.md")],
        drift_confined_to_late_stage_surface: true,
        supported: true,
        unsupported_reason: None,
    });

    let decision = route_decision_from_runtime_state_with_inputs(&runtime_state, false, false)
        .expect("route decision should project");

    assert_ne!(
        decision.phase_detail,
        phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
        "runtime-owned-only task closures must not be classified as branch-contributing: {decision:#?}"
    );
    assert_ne!(
        decision.next_action, NEXT_ACTION_ADVANCE_LATE_STAGE,
        "runtime-owned-only task closures must not advance into branch-closure rerecording: {decision:#?}"
    );
}

#[test]
fn runtime_owned_current_task_closure_stale_boundary_does_not_enable_branch_closure_rerecording() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let mut runtime_state = completed_stale_target_runtime_state(temp.path());
    runtime_state.status.current_branch_closure_id = None;
    runtime_state.status.current_task_closures = vec![PublicReviewStateTaskClosure {
        task: 5,
        closure_record_id: String::from("task-closure-runtime-owned-only"),
        reviewed_state_id: String::from("git_tree:current"),
        contract_identity: String::from("task-contract-5"),
        effective_reviewed_surface_paths: vec![String::from(
            "docs/featureforge/execution-evidence/plan-r1-evidence.md",
        )],
    }];

    let decision = route_decision_from_runtime_state_with_inputs(&runtime_state, false, false)
        .expect("route decision should project");

    assert_ne!(
        decision.phase_detail,
        phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
        "stale-boundary routing must use the same non-branch closure predicate as the primary route path: {decision:#?}"
    );
    assert_ne!(
        decision.next_action, NEXT_ACTION_ADVANCE_LATE_STAGE,
        "runtime-owned-only stale-boundary routing must not advance into branch-closure rerecording: {decision:#?}"
    );
}

#[test]
fn public_follow_up_templates_do_not_surface_removed_hidden_commands() {
    let follow_ups = [
        FOLLOW_UP_REPAIR_REVIEW_STATE,
        FOLLOW_UP_ADVANCE_LATE_STAGE,
        "resolve_release_blocker",
        "record_handoff",
        FOLLOW_UP_EXECUTION_REENTRY,
        "request_external_review",
        "wait_for_external_review_result",
        "run_verification",
    ];
    for follow_up in follow_ups {
        let template = follow_up_to_command_template(Some(follow_up))
            .expect("known follow-up should map to a command template");
        for hidden in hidden_command_tokens() {
            assert!(
                !template.contains(hidden.as_str()),
                "public follow-up templates must not reference removed hidden commands, saw `{hidden}` in `{template}`"
            );
        }
    }
}

#[test]
fn workflow_operator_fallback_follow_ups_requery_json_surface() {
    for follow_up in [
        FOLLOW_UP_EXECUTION_REENTRY,
        "request_external_review",
        "wait_for_external_review_result",
        "run_verification",
    ] {
        let command = public_command_for_required_follow_up(
            Some(follow_up),
            "docs/featureforge/plans/plan.md",
            phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
            None,
        )
        .unwrap_or_else(|| panic!("{follow_up} should map to workflow/operator requery"));
        let argv = command.to_argv();
        assert_eq!(
            argv,
            vec![
                "featureforge",
                "workflow",
                "operator",
                "--plan",
                "docs/featureforge/plans/plan.md",
                "--json",
            ],
            "fallback {follow_up} should use JSON operator requery argv"
        );
        assert!(
            command.to_display_command().ends_with(" --json"),
            "fallback {follow_up} display text should point at JSON operator requery"
        );
    }
}

#[test]
fn task_review_dispatch_lane_does_not_expose_public_action_or_blocker_command() {
    assert!(
        synthesize_next_public_action(
            None,
            phase::DETAIL_TASK_REVIEW_DISPATCH_REQUIRED,
            "docs/featureforge/plans/plan.md"
        )
        .is_none(),
        "task-review dispatch is no longer a public route"
    );
    let routing = ExecutionRoutingState {
        route_decision: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from(phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
            next_skill: String::from("featureforge:executing-plans"),
            spec_path: String::from("docs/featureforge/specs/spec.md"),
            plan_path: String::from("docs/featureforge/plans/plan.md"),
            contract_state: String::from("clean"),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 1,
            plan_candidate_count: 1,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        runtime_provenance: None,
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(phase::PHASE_TASK_CLOSURE_PENDING),
        phase: String::from(phase::PHASE_TASK_CLOSURE_PENDING),
        phase_detail: String::from(phase::DETAIL_TASK_REVIEW_DISPATCH_REQUIRED),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::from(NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED),
        recommended_public_command: None,
        recommended_command: None,
        blocking_scope: Some(String::from("task")),
        blocking_task: Some(2),
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };
    let blockers = primary_blocker_for_route(&routing, &[], "actionable_public_command", None);
    assert!(
        blockers.is_empty(),
        "legacy task-review dispatch lanes must not create public blockers: {blockers:?}"
    );
}

#[test]
fn waiting_external_input_omits_public_follow_up_until_result_arrives() {
    let routing = ExecutionRoutingState {
        route_decision: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from(phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
            next_skill: String::from("featureforge:requesting-code-review"),
            spec_path: String::from("docs/featureforge/specs/spec.md"),
            plan_path: String::from("docs/featureforge/plans/plan.md"),
            contract_state: String::from("clean"),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 1,
            plan_candidate_count: 1,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        runtime_provenance: None,
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(phase::PHASE_FINAL_REVIEW_PENDING),
        phase: String::from(phase::PHASE_FINAL_REVIEW_PENDING),
        phase_detail: String::from(phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::from(NEXT_ACTION_WAIT_FOR_EXTERNAL_REVIEW_RESULT),
        recommended_public_command: None,
        recommended_command: None,
        blocking_scope: Some(String::from("branch")),
        blocking_task: None,
        external_wait_state: Some(String::from("waiting_for_external_review_result")),
        blocking_reason_codes: Vec::new(),
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: Some(String::from("dispatch-1")),
        current_branch_closure_id: Some(String::from("branch-1")),
        current_release_readiness_result: Some(String::from("ready")),
        base_branch: Some(String::from("main")),
    };

    let decision = route_decision_from_non_runtime_workflow_routing(&routing, &[], false);
    assert_eq!(decision.state_kind, "waiting_external_input");
    assert!(decision.next_public_action.is_none());
    assert_eq!(decision.blockers.len(), 1);
    assert_eq!(decision.blockers[0].category, "external_input");
    assert_eq!(decision.blockers[0].scope_type, "branch");
    assert_eq!(
        decision.blockers[0].scope_key,
        phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING
    );
    assert!(decision.blockers[0].next_public_action.is_none());
}

#[test]
fn diagnostic_phase_details_without_commands_preserve_diagnostic_state_kind() {
    assert_eq!(
        classify_state_kind(None, false, phase::DETAIL_RUNTIME_RECONCILE_REQUIRED, None),
        phase::DETAIL_RUNTIME_RECONCILE_REQUIRED
    );
    assert_eq!(
        classify_state_kind(None, false, phase::DETAIL_BLOCKED_RUNTIME_BUG, None),
        phase::DETAIL_BLOCKED_RUNTIME_BUG
    );
    assert_eq!(
        classify_state_kind(None, false, phase::DETAIL_EXECUTION_REENTRY_REQUIRED, None),
        phase::DETAIL_BLOCKED_RUNTIME_BUG
    );
    assert_eq!(
        classify_state_kind(None, false, phase::DETAIL_PLANNING_REENTRY_REQUIRED, None),
        phase::DETAIL_PLANNING_REENTRY_REQUIRED
    );
}

#[test]
fn execution_route_matching_rejects_blocking_state_kinds_even_with_stale_targets() {
    let target = ExecutionCommandRouteTarget {
        kind: PublicCommandKind::Begin,
        task_number: 1,
        step_id: Some(1),
    };
    for state_kind in [STATE_KIND_PLANNING_REENTRY_REQUIRED, STATE_KIND_TERMINAL] {
        let mut status = closed_stale_provenance_status();
        status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
        status.execution_started = String::from("yes");
        status.active_task = None;
        status.active_step = None;
        status.resume_task = Some(1);
        status.resume_step = Some(1);
        status.state_kind = String::from(state_kind);

        assert!(
            !execution_command_route_target_matches_public_status(&status, &target),
            "{state_kind} must suppress stale execution route target matching"
        );
    }
}

#[test]
fn planning_reentry_without_public_route_is_not_external_wait_blocker() {
    let routing = ExecutionRoutingState {
        route_decision: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from(phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
            next_skill: String::from("featureforge:plan-eng-review"),
            spec_path: String::from("docs/featureforge/specs/spec.md"),
            plan_path: String::from("docs/featureforge/plans/plan.md"),
            contract_state: String::from("clean"),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 1,
            plan_candidate_count: 1,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        runtime_provenance: None,
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(phase::PHASE_PIVOT_REQUIRED),
        phase: String::from(phase::PHASE_PIVOT_REQUIRED),
        phase_detail: String::from(phase::DETAIL_PLANNING_REENTRY_REQUIRED),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::from(NEXT_ACTION_PLANNING_REENTRY),
        recommended_public_command: None,
        recommended_command: None,
        blocking_scope: Some(String::from("workflow")),
        blocking_task: None,
        external_wait_state: None,
        blocking_reason_codes: vec![String::from("missing_plan_fidelity_review_artifact")],
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };

    let decision = route_decision_from_non_runtime_workflow_routing(&routing, &[], false);
    assert_eq!(decision.state_kind, phase::DETAIL_PLANNING_REENTRY_REQUIRED);
    assert_eq!(decision.next_action, NEXT_ACTION_PLANNING_REENTRY);
    assert_eq!(decision.blockers.len(), 1);
    assert_eq!(decision.blockers[0].category, "workflow");
    assert_eq!(decision.blockers[0].scope_type, "workflow");
    assert_eq!(
        decision.blockers[0].scope_key,
        phase::DETAIL_PLANNING_REENTRY_REQUIRED
    );
    assert_eq!(
        decision.blockers[0].details,
        "Return to featureforge:plan-eng-review for planning reentry before continuing execution."
    );
    assert!(
        !decision.blockers[0]
            .details
            .contains("external review result"),
        "planning reentry blocker must not masquerade as external wait: {:?}",
        decision.blockers[0]
    );
}

#[test]
fn blocked_runtime_bug_suppresses_public_action_surfaces() {
    let routing = ExecutionRoutingState {
        route_decision: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from(phase::WORKFLOW_STATUS_IMPLEMENTATION_READY),
            next_skill: String::from("featureforge:executing-plans"),
            spec_path: String::from("docs/featureforge/specs/spec.md"),
            plan_path: String::from("docs/featureforge/plans/plan.md"),
            contract_state: String::from("clean"),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 1,
            plan_candidate_count: 1,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        runtime_provenance: None,
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(phase::PHASE_EXECUTING),
        phase: String::from(phase::PHASE_EXECUTING),
        phase_detail: String::from(phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::from(NEXT_ACTION_EXECUTION_REENTRY_REQUIRED),
        recommended_public_command: None,
        recommended_command: None,
        blocking_scope: Some(String::from("workflow")),
        blocking_task: None,
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };

    let decision = route_decision_from_non_runtime_workflow_routing(&routing, &[], false);
    assert_eq!(decision.state_kind, phase::DETAIL_BLOCKED_RUNTIME_BUG);
    assert!(decision.next_public_action.is_none());
    assert!(decision.recommended_command.is_none());
    assert!(decision.required_follow_up.is_none());
    assert_eq!(
        decision.next_action,
        NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED
    );
    assert!(decision.blockers.is_empty());
    assert!(decision.public_repair_targets.is_empty());
}

#[test]
fn diagnostic_normalizer_strips_seeded_executable_surfaces() {
    let command = PublicCommand::CloseCurrentTask {
        plan: String::from("docs/featureforge/plans/plan.md"),
        task: Some(1),
        result_inputs_required: true,
    };
    let (recommended_command, _, template, required_inputs) =
        RouteDecision::command_surfaces(Some(&command));
    assert!(
        template.is_some() && !required_inputs.is_empty(),
        "test command must seed template and input surfaces"
    );

    let mut decision = release_readiness_route_decision();
    decision.state_kind = String::from("actionable_public_command");
    decision.phase_detail = String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
    decision.next_action = String::from(NEXT_ACTION_CLOSE_CURRENT_TASK);
    decision.recommended_command = recommended_command;
    decision.recommended_public_command = Some(command);
    decision.invocation = Some(PublicCommandInvocation {
        argv: vec![
            String::from("featureforge"),
            String::from("plan"),
            String::from("execution"),
            String::from("begin"),
        ],
    });
    decision.recommended_public_command_template = template;
    decision.required_inputs = required_inputs;
    decision.required_follow_up = Some(String::from(FOLLOW_UP_REPAIR_REVIEW_STATE));
    decision.next_public_action = Some(NextPublicAction {
        display_only: true,
        command: String::from("featureforge plan execution begin --plan docs/plan.md"),
        args_template: Some(String::from(
            "featureforge plan execution begin --plan docs/plan.md",
        )),
    });
    decision.blockers = vec![Blocker {
        category: String::from("runtime"),
        scope_type: String::from("task"),
        scope_key: String::from("1"),
        record_id: Some(String::from("record-1")),
        next_public_action: Some(NextPublicAction {
            display_only: true,
            command: String::from(
                "featureforge plan execution repair-review-state --plan docs/plan.md",
            ),
            args_template: None,
        }),
        details: String::from("seeded blocker action must be stripped"),
    }];
    decision.public_repair_targets = vec![PublicRepairTarget {
        command_kind: String::from("repair-review-state"),
        task: Some(1),
        step: Some(1),
        reason_code: String::from("seeded"),
        source_record_id: Some(String::from("record-1")),
        expires_when_fingerprint_changes: true,
    }];
    decision.execution_reentry_target_source = Some(String::from("seeded"));
    decision.execution_command_context = Some(ExecutionRoutingExecutionCommandContext {
        command_kind: String::from("begin"),
        task_number: Some(1),
        step_id: Some(1),
    });
    decision.recording_context = Some(ExecutionRoutingRecordingContext {
        task_number: Some(1),
        dispatch_id: Some(String::from("dispatch-1")),
        branch_closure_id: None,
    });

    decision.normalize_diagnostic_next_action();

    assert_eq!(
        decision.next_action,
        NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED
    );
    assert!(decision.recommended_command.is_none());
    assert!(decision.recommended_public_command.is_none());
    assert!(decision.invocation.is_none());
    assert!(decision.recommended_public_command_template.is_none());
    assert!(decision.required_inputs.is_empty());
    assert!(decision.required_follow_up.is_none());
    assert!(decision.next_public_action.is_none());
    assert!(decision.blockers.is_empty());
    assert!(decision.public_repair_targets.is_empty());
    assert!(decision.execution_reentry_target_source.is_none());
    assert!(decision.execution_command_context.is_none());
    assert!(decision.recording_context.is_none());
}

#[test]
fn hidden_string_recommendations_are_not_route_authority() {
    let status = PlanExecutionStatus {
        schema_version: 3,
        plan_revision: 1,
        execution_run_id: None,
        workspace_state_id: String::from("semantic_tree:ignored"),
        current_branch_reviewed_state_id: None,
        current_branch_closure_id: None,
        current_branch_meaningful_drift: false,
        current_task_closures: Vec::new(),
        superseded_closures_summary: Vec::new(),
        stale_unreviewed_closures: Vec::new(),
        current_release_readiness_state: None,
        current_final_review_state: String::from("missing"),
        current_qa_state: String::from("missing"),
        current_final_review_branch_closure_id: None,
        current_final_review_result: None,
        current_qa_branch_closure_id: None,
        current_qa_result: None,
        qa_requirement: None,
        latest_authoritative_sequence: 1,
        phase: Some(String::from(phase::PHASE_EXECUTING)),
        harness_phase: HarnessPhase::Executing,
        chunk_id: ChunkId(String::from("chunk-1")),
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
        dependency_index_state: String::from("clean"),
        final_review_state: DownstreamFreshnessState::Missing,
        browser_qa_state: DownstreamFreshnessState::Missing,
        release_docs_state: DownstreamFreshnessState::Missing,
        last_final_review_artifact_fingerprint: None,
        last_browser_qa_artifact_fingerprint: None,
        last_release_docs_artifact_fingerprint: None,
        strategy_state: String::from("clean"),
        last_strategy_checkpoint_fingerprint: None,
        strategy_checkpoint_kind: String::from("none"),
        strategy_reset_required: false,
        phase_detail: String::from(phase::DETAIL_TASK_REVIEW_DISPATCH_REQUIRED),
        review_state_status: String::from("clean"),
        recording_context: None,
        execution_command_context: None,
        execution_reentry_target_source: None,
        public_repair_targets: Vec::new(),
        blocking_records: Vec::new(),
        blocking_scope: Some(String::from("task")),
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        projection_diagnostics: Vec::new(),
        state_kind: String::from("actionable_public_command"),
        next_public_action: None,
        blockers: Vec::new(),
        runtime_provenance: None,
        semantic_workspace_tree_id: String::from("semantic_tree:authoritative"),
        raw_workspace_tree_id: Some(String::from("git_tree:debug")),
        next_action: String::from(NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED),
        recommended_public_command: None,
        recommended_public_command_argv: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        recommended_command: Some(format!(
            "featureforge plan execution {} --plan docs/featureforge/plans/example.md --scope task --task 1",
            ["record", "review", "dispatch"].join("-")
        )),
        finish_review_gate_pass_branch_closure_id: None,
        reason_codes: Vec::new(),
        execution_mode: String::from("none"),
        execution_fingerprint: String::from("fingerprint"),
        evidence_path: String::from("docs/featureforge/execution-evidence/example"),
        projection_mode: String::from("state_dir_only"),
        state_dir_projection_paths: Vec::new(),
        tracked_projection_paths: Vec::new(),
        tracked_projections_current: false,
        execution_started: String::from("yes"),
        warning_codes: Vec::new(),
        active_task: None,
        active_step: None,
        blocking_task: Some(1),
        blocking_step: None,
        resume_task: None,
        resume_step: None,
    };

    assert!(
        synthesize_next_public_action(
            status.recommended_public_command.as_ref(),
            &status.phase_detail,
            "docs/featureforge/plans/plan.md"
        )
        .is_none(),
        "task-review dispatch is diagnostic-only and must not synthesize a public operator loop"
    );
}

#[test]
fn next_public_action_uses_placeholder_display_for_operator_fallbacks() {
    let plan_path = "docs/featureforge/plans/plan with spaces.md";
    let action = synthesize_next_public_action(
        None,
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
        plan_path,
    )
    .expect("final review dispatch should route through workflow/operator");

    assert_eq!(
        action.command,
        "featureforge workflow operator --plan <approved-plan-path> --json"
    );
    assert_eq!(
        action.args_template.as_deref(),
        Some("featureforge workflow operator --plan <approved-plan-path> --json"),
        "args_template should keep the same non-authoritative placeholder display: {action:?}"
    );
    assert!(
        !action.command.contains(plan_path)
            && !action
                .args_template
                .as_deref()
                .unwrap_or_default()
                .contains(plan_path),
        "operator fallback display must not interpolate concrete plan paths with shell-sensitive spaces: {action:?}"
    );
}

#[test]
fn test_plan_refresh_required_does_not_synthesize_operator_requery_action() {
    let plan_path = "docs/featureforge/plans/plan.md";

    assert!(
        synthesize_next_public_action(None, phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED, plan_path)
            .is_none(),
        "test-plan refresh is a plan-eng-review handoff lane and must not synthesize a workflow/operator requery action"
    );
    assert!(
        public_command_for_phase_detail(phase::DETAIL_TEST_PLAN_REFRESH_REQUIRED, plan_path)
            .is_none(),
        "test-plan refresh has no executable public command fallback"
    );
}

#[test]
fn removed_plan_execution_commands_are_not_public_route_commands() {
    for removed in ["preflight", "recommend"] {
        let command = format!("featureforge plan execution {removed} --plan docs/plan.md --json");
        assert!(command_invokes_hidden_lane(&command));
        assert_eq!(
            public_command_for_phase_detail(
                phase::DETAIL_EXECUTION_REENTRY_REQUIRED,
                "<approved-plan-path>",
            )
            .map(|command| command.to_display_command())
            .as_deref(),
            Some("featureforge plan execution repair-review-state --plan <approved-plan-path>"),
            "removed `{removed}` command cannot be parsed into a route command; the typed fallback is explicit"
        );
    }
}
