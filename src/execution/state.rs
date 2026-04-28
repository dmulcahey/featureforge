use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use gix::bstr::ByteSlice;
use jiff::Timestamp;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};

use crate::cli::plan_execution::{BeginArgs, CompleteArgs, ReopenArgs, StatusArgs, TransferArgs};
use crate::cli::repo_safety::{RepoSafetyCheckArgs, RepoSafetyIntentArg, RepoSafetyWriteTargetArg};
use crate::contracts::harness::{
    ExecutionTopologyDowngradeRecord, WORKTREE_LEASE_VERSION, WorktreeLease, WorktreeLeaseState,
    read_execution_contract,
};
use crate::contracts::plan::analyze_documents;
use crate::contracts::plan::{PlanDocument, PlanTask, parse_plan_file};
use crate::contracts::spec::parse_spec_file;
use crate::contracts::task_contract::{
    RuntimeExecutionNoteProjectionBlock, known_runtime_step_projection_lines,
    parse_task_step_projection_line,
};
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::authority::{
    ensure_preflight_authoritative_bootstrap,
    ensure_preflight_authoritative_bootstrap_with_existing_authority,
};
use crate::execution::closure_graph::{AuthoritativeClosureGraph, ClosureGraphSignals};
use crate::execution::current_truth::public_task_boundary_decision;
#[cfg(test)]
use crate::execution::current_truth::task_review_result_pending_task;
use crate::execution::current_truth::{
    BranchRerecordingUnsupportedReason, RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS,
    branch_closure_refresh_missing_current_closure as shared_branch_closure_refresh_missing_current_closure,
    branch_closure_rerecording_assessment,
    branch_source_task_closure_ids as shared_branch_source_task_closure_ids,
    current_branch_closure_has_tracked_drift as shared_current_branch_closure_has_tracked_drift,
    current_final_review_dispatch_id as shared_current_final_review_dispatch_id,
    current_late_stage_branch_bindings as shared_current_late_stage_branch_bindings,
    current_repo_tracked_tree_sha,
    current_task_negative_result_task as shared_current_task_negative_result_task,
    current_task_review_dispatch_id as shared_current_task_review_dispatch_id,
    final_review_dispatch_still_current as shared_final_review_dispatch_still_current,
    finish_requires_test_plan_refresh, is_runtime_owned_execution_control_plane_path,
    late_stage_missing_current_closure_stale_provenance_present as shared_late_stage_missing_current_closure_stale_provenance_present,
    late_stage_missing_task_closure_baseline_bridge_supported,
    late_stage_qa_blocked as shared_late_stage_qa_blocked,
    late_stage_release_blocked as shared_late_stage_release_blocked,
    late_stage_review_blocked as shared_late_stage_review_blocked,
    late_stage_review_truth_blocked as shared_late_stage_review_truth_blocked,
    legacy_repair_follow_up_unbound,
    live_review_state_repair_reroute as shared_live_review_state_repair_reroute,
    live_review_state_status_for_reroute as shared_live_review_state_status_for_reroute,
    live_task_scope_repair_precedence_active as shared_live_task_scope_repair_precedence_active,
    negative_result_requires_execution_reentry as shared_negative_result_requires_execution_reentry,
    normalize_summary_content, normalized_late_stage_surface,
    normalized_plan_qa_requirement as shared_normalized_plan_qa_requirement,
    parse_late_stage_surface_only_branch_surface, path_matches_late_stage_surface,
    public_late_stage_rederivation_basis_present,
    public_late_stage_stale_unreviewed as shared_public_late_stage_stale_unreviewed,
    public_review_state_stale_unreviewed_for_reroute as shared_public_review_state_stale_unreviewed_for_reroute,
    qa_requirement_policy_invalid as shared_qa_requirement_policy_invalid,
    release_readiness_result_for_branch_closure as shared_release_readiness_result_for_branch_closure,
    resolve_actionable_repair_follow_up_for_status,
    resolve_actionable_repair_follow_up_for_status_with_source_hash,
    reviewer_source_is_valid as shared_reviewer_source_is_valid,
    stale_reason_codes_for_late_stage_projection as shared_stale_reason_codes_for_late_stage_projection,
    task_closure_contributes_to_branch_surface,
    task_scope_overlay_restore_required as shared_task_scope_overlay_restore_required,
    task_scope_stale_review_state_reason_present as shared_task_scope_stale_review_state_reason_present,
};
use crate::execution::event_log::{
    load_reduced_authoritative_state, load_reduced_authoritative_state_for_state_path,
};
use crate::execution::final_review::{
    authoritative_strategy_checkpoint_fingerprint_checked, is_canonical_fingerprint,
    parse_artifact_document, resolve_release_base_branch,
};
use crate::execution::follow_up::{
    FollowUpAliasContext, FollowUpKind, direct_gate_follow_up_from_reason_codes,
    execution_step_repair_target_id, materialized_follow_up_kind_command,
    missing_branch_closure_gate_follow_up, normalize_follow_up_alias,
    repair_follow_up_source_decision_hash,
};
use crate::execution::harness::TopologySelectionContext;
use crate::execution::harness::{
    AggregateEvaluationState, ChunkId, ChunkingStrategy, DownstreamFreshnessState,
    EvaluationVerdict, EvaluatorKind, EvaluatorPolicyName, ExecutionRunId, HarnessPhase,
    INITIAL_AUTHORITATIVE_SEQUENCE, LearnedTopologyGuidance, ResetPolicy, RunIdentitySnapshot,
};
use crate::execution::internal_args::{
    GateContractArgs, GateEvaluatorArgs, GateHandoffArgs, IsolatedAgentsArg, NoteArgs,
    NoteStateArg, RebuildEvidenceArgs, RecommendArgs, RecordContractArgs, RecordEvaluationArgs,
    RecordHandoffArgs, RecordReviewDispatchArgs, ReviewDispatchScopeArg,
};
use crate::execution::leases::authoritative_matching_execution_topology_downgrade_records_checked;
use crate::execution::leases::{
    PreflightWriteAuthorityState, StatusAuthoritativeOverlay, authoritative_state_path,
    load_status_authoritative_overlay_checked, preflight_requires_authoritative_handoff,
    preflight_requires_authoritative_mutation_recovery, preflight_write_authority_state,
    validate_worktree_lease,
};
#[cfg(test)]
use crate::execution::next_action::{NextActionDecision, NextActionKind, public_next_action_text};
use crate::execution::next_action::{
    compute_next_action_decision, exact_execution_command_from_decision, execution_reentry_target,
};
use crate::execution::observability::{
    REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED, REASON_CODE_STALE_PROVENANCE,
};
use crate::execution::projection_renderer::{
    execution_projection_read_model_metadata, normal_projection_write_mode,
    projection_source_matches_fingerprint, read_state_dir_projection,
    render_canonical_evidence_projection_source,
    render_canonical_evidence_projection_source_with_fingerprints,
    state_dir_projection_matches_recorded_output_fingerprint,
};
use crate::execution::query::{ExecutionRoutingState, required_follow_up_from_routing};
use crate::execution::recording::current_task_closure_postconditions_would_mutate;
use crate::execution::reducer::RuntimeState;
use crate::execution::reentry_reconcile::{
    TARGETLESS_STALE_MISSING_AUTHORITY_CODE, TARGETLESS_STALE_RECONCILE_REASON_CODE,
    TargetlessStaleReconcile,
    task_closure_baseline_repair_candidate_reason_present as shared_task_closure_baseline_repair_candidate_reason_present,
};
use crate::execution::router::{
    Blocker as RuntimeBlocker, NextPublicAction as RuntimeNextPublicAction, RouteDecision,
    route_decision_with_status_blockers,
};
use crate::execution::semantic_identity::{
    branch_definition_identity_for_context, normalized_plan_source_for_approved_plan_preflight,
    normalized_plan_source_for_semantic_identity, semantic_paths_changed_between_raw_trees,
    semantic_workspace_snapshot, task_definition_identity_for_task,
};
use crate::execution::topology::{
    RecommendOutput, default_preflight_chunking_strategy, default_preflight_evaluator_policy,
    default_preflight_reset_policy, default_preflight_review_stack, recommend_topology,
    tasks_are_independent,
};
use crate::execution::topology::{
    authoritative_run_identity_present, load_preflight_acceptance, pending_chunk_id,
    persist_preflight_acceptance, preflight_acceptance_for_context,
};
use crate::execution::transitions::{
    AuthoritativeTransitionState, CurrentBrowserQaRecord, OpenStepStateRecord,
    PersistedReviewStateFieldClass, authoritative_state_optional_string_field_for_runtime,
    claim_step_write_authority, classify_review_state_field, load_authoritative_transition_state,
    load_authoritative_transition_state_relaxed,
};
use crate::execution::workflow_operator_requery_command;
use crate::git::{
    canonicalize_repo_root_path, commit_object_fingerprint, derive_repo_slug,
    discover_repo_context, discover_repository, is_ancestor_commit as shared_is_ancestor_commit,
    sha256_hex, stored_repo_root_matches_current,
};
use crate::paths::{
    RepoPath, branch_storage_key, featureforge_state_dir, harness_authoritative_artifact_path,
    harness_authoritative_artifacts_dir, normalize_repo_relative_path, normalize_whitespace,
};
use crate::repo_safety::RepoSafetyRuntime;
use crate::workflow::late_stage_precedence::{
    GateState as PrecedenceGateState, LateStageSignals, resolve as resolve_late_stage_precedence,
};
use crate::workflow::manifest::{ManifestLoadResult, WorkflowManifest, load_manifest_read_only};
use crate::workflow::markdown_scan::markdown_files_under;
#[cfg(test)]
use crate::workflow::pivot::{
    WorkflowPivotRecordIdentity, current_workflow_pivot_record_exists, pivot_decision_reason_codes,
};

pub const NO_REPO_FILES_MARKER: &str = "__featureforge__/no-repo-files";
const ACTIVE_SPEC_ROOT: &str = "docs/featureforge/specs";
const ACTIVE_PLAN_ROOT: &str = "docs/featureforge/plans";
const ACTIVE_EVIDENCE_ROOT: &str = "docs/featureforge/execution-evidence";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ReviewStateStatusSchema {
    Clean,
    StaleUnreviewed,
    MissingCurrentClosure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum PhaseDetailSchema {
    BlockedRuntimeBug,
    BranchClosureRecordingRequiredForReleaseReadiness,
    ExecutionInProgress,
    ExecutionReentryRequired,
    FinalReviewDispatchRequired,
    FinalReviewOutcomePending,
    FinalReviewRecordingReady,
    FinishCompletionGateReady,
    FinishReviewGateReady,
    HandoffRecordingRequired,
    PlanningReentryRequired,
    QaRecordingRequired,
    ReleaseBlockerResolutionRequired,
    ReleaseReadinessRecordingReady,
    RuntimeReconcileRequired,
    TaskClosureRecordingReady,
    TaskReviewResultPending,
    TestPlanRefreshRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum StateKindSchema {
    ActionablePublicCommand,
    WaitingExternalInput,
    Terminal,
    BlockedRuntimeBug,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
enum QaRequirementSchema {
    #[serde(rename = "required")]
    Required,
    #[serde(rename = "not-required")]
    NotRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ExecutionCommandKindSchema {
    Begin,
    Complete,
    Reopen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum PublicRepairTargetCommandKindSchema {
    Begin,
    Complete,
    Reopen,
    Transfer,
    CloseCurrentTask,
    RepairReviewState,
    AdvanceLateStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RequiredFollowUpSchema {
    ExecutionReentry,
    RepairReviewState,
    RequestExternalReview,
    RunVerification,
    AdvanceLateStage,
    ResolveReleaseBlocker,
    RecordHandoff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
enum NextActionSchema {
    #[serde(rename = "advance late stage")]
    AdvanceLateStage,
    #[serde(rename = "finish branch")]
    FinishBranch,
    #[serde(rename = "close current task")]
    CloseCurrentTask,
    #[serde(rename = "continue execution")]
    ContinueExecution,
    #[serde(rename = "request final review")]
    RequestFinalReview,
    #[serde(rename = "execution reentry required")]
    ExecutionReentryRequired,
    #[serde(rename = "hand off")]
    HandOff,
    #[serde(rename = "pivot / return to planning")]
    PivotReturnToPlanning,
    #[serde(rename = "refresh test plan")]
    RefreshTestPlan,
    #[serde(rename = "repair review state / reenter execution")]
    RepairReviewStateReenterExecution,
    #[serde(rename = "resolve release blocker")]
    ResolveReleaseBlocker,
    #[serde(rename = "run QA")]
    RunQa,
    #[serde(rename = "wait for external review result")]
    WaitForExternalReviewResult,
    #[serde(rename = "run verification")]
    RunVerification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PlanExecutionStatus {
    #[schemars(range(min = 3, max = 3))]
    pub schema_version: u32,
    pub plan_revision: u32,
    pub execution_run_id: Option<ExecutionRunId>,
    #[serde(skip_serializing)]
    #[schemars(skip)]
    pub workspace_state_id: String,
    pub current_branch_reviewed_state_id: Option<String>,
    pub current_branch_closure_id: Option<String>,
    #[serde(skip_serializing)]
    #[schemars(skip)]
    pub current_branch_meaningful_drift: bool,
    pub current_task_closures: Vec<PublicReviewStateTaskClosure>,
    pub superseded_closures_summary: Vec<String>,
    pub stale_unreviewed_closures: Vec<String>,
    pub current_release_readiness_state: Option<String>,
    pub current_final_review_state: String,
    pub current_qa_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_final_review_branch_closure_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_final_review_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_qa_branch_closure_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_qa_result: Option<String>,
    #[schemars(with = "Option<QaRequirementSchema>")]
    pub qa_requirement: Option<String>,
    pub latest_authoritative_sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(skip)]
    pub phase: Option<String>,
    pub harness_phase: HarnessPhase,
    pub chunk_id: ChunkId,
    pub chunking_strategy: Option<ChunkingStrategy>,
    pub evaluator_policy: Option<EvaluatorPolicyName>,
    pub reset_policy: Option<ResetPolicy>,
    pub review_stack: Option<Vec<String>>,
    pub active_contract_path: Option<String>,
    pub active_contract_fingerprint: Option<String>,
    pub required_evaluator_kinds: Vec<EvaluatorKind>,
    pub completed_evaluator_kinds: Vec<EvaluatorKind>,
    pub pending_evaluator_kinds: Vec<EvaluatorKind>,
    pub non_passing_evaluator_kinds: Vec<EvaluatorKind>,
    pub aggregate_evaluation_state: AggregateEvaluationState,
    pub last_evaluation_report_path: Option<String>,
    pub last_evaluation_report_fingerprint: Option<String>,
    pub last_evaluation_evaluator_kind: Option<EvaluatorKind>,
    pub last_evaluation_verdict: Option<EvaluationVerdict>,
    pub current_chunk_retry_count: u32,
    pub current_chunk_retry_budget: u32,
    pub current_chunk_pivot_threshold: u32,
    pub handoff_required: bool,
    pub open_failed_criteria: Vec<String>,
    pub write_authority_state: String,
    pub write_authority_holder: Option<String>,
    pub write_authority_worktree: Option<String>,
    pub repo_state_baseline_head_sha: Option<String>,
    pub repo_state_baseline_worktree_fingerprint: Option<String>,
    pub repo_state_drift_state: String,
    pub dependency_index_state: String,
    pub final_review_state: DownstreamFreshnessState,
    pub browser_qa_state: DownstreamFreshnessState,
    pub release_docs_state: DownstreamFreshnessState,
    pub last_final_review_artifact_fingerprint: Option<String>,
    pub last_browser_qa_artifact_fingerprint: Option<String>,
    pub last_release_docs_artifact_fingerprint: Option<String>,
    pub strategy_state: String,
    pub last_strategy_checkpoint_fingerprint: Option<String>,
    pub strategy_checkpoint_kind: String,
    pub strategy_reset_required: bool,
    #[schemars(with = "PhaseDetailSchema")]
    pub phase_detail: String,
    #[schemars(with = "ReviewStateStatusSchema")]
    pub review_state_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PublicRecordingContext")]
    pub recording_context: Option<PublicRecordingContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "PublicExecutionCommandContext")]
    pub execution_command_context: Option<PublicExecutionCommandContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_reentry_target_source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_repair_targets: Vec<PublicRepairTarget>,
    pub blocking_records: Vec<StatusBlockingRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocking_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_wait_state: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub blocking_reason_codes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub projection_diagnostics: Vec<String>,
    #[schemars(with = "StateKindSchema")]
    pub state_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_public_action: Option<RuntimeNextPublicAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<RuntimeBlocker>,
    pub semantic_workspace_tree_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_workspace_tree_id: Option<String>,
    #[schemars(with = "NextActionSchema")]
    pub next_action: String,
    pub recommended_command: Option<String>,
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub reason_codes: Vec<String>,
    pub execution_mode: String,
    pub execution_fingerprint: String,
    pub evidence_path: String,
    pub projection_mode: String,
    pub state_dir_projection_paths: Vec<String>,
    pub tracked_projection_paths: Vec<String>,
    pub tracked_projections_current: bool,
    pub execution_started: String,
    pub warning_codes: Vec<String>,
    pub active_task: Option<u32>,
    pub active_step: Option<u32>,
    pub blocking_task: Option<u32>,
    pub blocking_step: Option<u32>,
    pub resume_task: Option<u32>,
    pub resume_step: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RebuildEvidenceCounts {
    pub planned: u32,
    pub rebuilt: u32,
    pub manual: u32,
    pub failed: u32,
    pub noop: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RebuildEvidenceFilter {
    pub all: bool,
    pub tasks: Vec<u32>,
    pub steps: Vec<String>,
    pub include_open: bool,
    pub skip_manual_fallback: bool,
    pub continue_on_error: bool,
    pub max_jobs: u32,
    pub no_output: bool,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RebuildEvidenceTarget {
    pub task_id: u32,
    pub step_id: u32,
    pub target_kind: String,
    pub pre_invalidation_reason: String,
    pub status: String,
    pub verify_mode: String,
    pub verify_command: Option<String>,
    pub attempt_id_before: Option<String>,
    pub attempt_id_after: Option<String>,
    pub verification_hash: Option<String>,
    pub error: Option<String>,
    pub failure_class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RebuildEvidenceOutput {
    pub session_root: String,
    pub dry_run: bool,
    pub filter: RebuildEvidenceFilter,
    pub scope: String,
    pub counts: RebuildEvidenceCounts,
    pub duration_ms: u64,
    pub targets: Vec<RebuildEvidenceTarget>,
    #[serde(skip_serializing)]
    pub exit_code: u8,
}

impl RebuildEvidenceOutput {
    pub fn exit_code(&self) -> u8 {
        self.exit_code
    }

    pub fn render_text(&self) -> String {
        let mut lines = Vec::with_capacity(self.targets.len() + 1);
        lines.push(format!(
            "summary scope={} dry_run={} planned={} rebuilt={} manual={} failed={} noop={}",
            render_text_value(&self.scope),
            self.dry_run,
            self.counts.planned,
            self.counts.rebuilt,
            self.counts.manual,
            self.counts.failed,
            self.counts.noop,
        ));
        for target in &self.targets {
            lines.push(format!(
                "target task_id={} step_id={} status={} target_kind={} pre_invalidation_reason={} verify_mode={} verify_command={} attempt_id_before={} attempt_id_after={} verification_hash={} error={} failure_class={}",
                target.task_id,
                target.step_id,
                render_text_value(&target.status),
                render_text_value(&target.target_kind),
                render_text_value(&target.pre_invalidation_reason),
                render_text_value(&target.verify_mode),
                render_optional_text_value(target.verify_command.as_deref()),
                render_optional_text_value(target.attempt_id_before.as_deref()),
                render_optional_text_value(target.attempt_id_after.as_deref()),
                render_optional_text_value(target.verification_hash.as_deref()),
                render_optional_text_value(target.error.as_deref()),
                render_optional_text_value(target.failure_class.as_deref()),
            ));
        }
        lines.join("\n") + "\n"
    }
}

fn render_text_value(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| String::from("\"<serialization-error>\""))
}

fn render_optional_text_value(value: Option<&str>) -> String {
    value
        .map(render_text_value)
        .unwrap_or_else(|| String::from("null"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GateDiagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub remediation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct StatusBlockingRecord {
    pub code: String,
    pub scope_type: String,
    pub scope_key: String,
    pub record_type: String,
    pub record_id: Option<String>,
    pub review_state_status: String,
    #[schemars(with = "Option<RequiredFollowUpSchema>")]
    pub required_follow_up: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PublicReviewStateTaskClosure {
    pub task: u32,
    pub closure_record_id: String,
    pub reviewed_state_id: String,
    pub contract_identity: String,
    pub effective_reviewed_surface_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PublicRecordingContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch_closure_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PublicExecutionCommandContext {
    #[schemars(with = "ExecutionCommandKindSchema")]
    pub command_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct PublicRepairTarget {
    #[schemars(with = "PublicRepairTargetCommandKindSchema")]
    pub command_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    pub reason_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_record_id: Option<String>,
    pub expires_when_fingerprint_changes: bool,
}

#[derive(Debug, Deserialize)]
struct WorktreeLeaseRunIdentityProbe {
    execution_run_id: String,
    source_plan_path: String,
    source_plan_revision: u32,
}

#[derive(Debug, Deserialize)]
struct WorktreeLeaseBindingProbe {
    execution_run_id: String,
    lease_fingerprint: String,
    lease_artifact_path: String,
    #[serde(default)]
    execution_context_key: Option<String>,
    #[serde(default)]
    approved_task_packet_fingerprint: Option<String>,
    #[serde(default)]
    approved_unit_contract_fingerprint: Option<String>,
    #[serde(default)]
    reconcile_result_proof_fingerprint: Option<String>,
    #[serde(default)]
    reviewed_checkpoint_commit_sha: Option<String>,
    #[serde(default)]
    reconcile_result_commit_sha: Option<String>,
    #[serde(default)]
    reconcile_mode: Option<String>,
    #[serde(default)]
    review_receipt_fingerprint: Option<String>,
    #[serde(default)]
    review_receipt_artifact_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorktreeLeaseAuthoritativeContextProbe {
    #[serde(default)]
    run_identity: Option<WorktreeLeaseRunIdentityProbe>,
    #[serde(default)]
    repo_state_baseline_head_sha: Option<String>,
    #[serde(default)]
    repo_state_baseline_worktree_fingerprint: Option<String>,
    active_worktree_lease_fingerprints: Option<Vec<String>>,
    active_worktree_lease_bindings: Option<Vec<WorktreeLeaseBindingProbe>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct GateResult {
    pub allowed: bool,
    pub action: String,
    pub failure_class: String,
    pub reason_codes: Vec<String>,
    pub warning_codes: Vec<String>,
    pub diagnostics: Vec<GateDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub workspace_state_id: Option<String>,
    pub current_branch_reviewed_state_id: Option<String>,
    pub current_branch_closure_id: Option<String>,
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
}

#[derive(Clone, Copy)]
pub(crate) struct GateProjectionInputs<'a> {
    pub(crate) gate_review: &'a GateResult,
    pub(crate) gate_finish: &'a GateResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RecordReviewDispatchOutput {
    pub allowed: bool,
    pub failure_class: String,
    pub reason_codes: Vec<String>,
    pub warning_codes: Vec<String>,
    pub diagnostics: Vec<GateDiagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rederive_via_workflow_operator: Option<bool>,
    pub scope: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionRuntime {
    pub repo_root: PathBuf,
    pub git_dir: PathBuf,
    pub branch_name: String,
    pub repo_slug: String,
    pub safe_branch: String,
    pub state_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteState {
    Active,
    Blocked,
    Interrupted,
}

impl NoteState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Blocked => "Blocked",
            Self::Interrupted => "Interrupted",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanStepState {
    pub task_number: u32,
    pub step_number: u32,
    pub title: String,
    pub checked: bool,
    pub note_state: Option<NoteState>,
    pub note_summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceFormat {
    Empty,
    Legacy,
    V2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvidenceSourceOrigin {
    Empty,
    TrackedFile,
    StateDirProjection,
    AuthoritativeState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateDirProjectionCurrentness {
    Unbound,
    Current,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileProof {
    pub path: String,
    pub proof: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceAttempt {
    pub task_number: u32,
    pub step_number: u32,
    pub attempt_number: u32,
    pub status: String,
    pub recorded_at: String,
    pub execution_source: String,
    pub claim: String,
    pub files: Vec<String>,
    pub file_proofs: Vec<FileProof>,
    pub verify_command: Option<String>,
    pub verification_summary: String,
    pub invalidation_reason: String,
    pub packet_fingerprint: Option<String>,
    pub head_sha: Option<String>,
    pub base_sha: Option<String>,
    pub source_contract_path: Option<String>,
    pub source_contract_fingerprint: Option<String>,
    pub source_evaluation_report_fingerprint: Option<String>,
    pub evaluator_verdict: Option<String>,
    pub failing_criterion_ids: Vec<String>,
    pub source_handoff_fingerprint: Option<String>,
    pub repo_state_baseline_head_sha: Option<String>,
    pub repo_state_baseline_worktree_fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionEvidence {
    pub format: EvidenceFormat,
    pub plan_path: String,
    pub plan_revision: u32,
    pub plan_fingerprint: Option<String>,
    pub source_spec_path: String,
    pub source_spec_revision: u32,
    pub source_spec_fingerprint: Option<String>,
    pub attempts: Vec<EvidenceAttempt>,
    pub source: Option<String>,
    pub(crate) source_origin: EvidenceSourceOrigin,
    pub(crate) tracked_progress_present: bool,
    pub(crate) tracked_source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub runtime: ExecutionRuntime,
    pub plan_rel: String,
    pub plan_abs: PathBuf,
    pub plan_document: PlanDocument,
    pub plan_source: String,
    pub steps: Vec<PlanStepState>,
    pub(crate) local_execution_progress_markers_present: bool,
    pub(crate) legacy_open_step_projection_present: bool,
    pub tasks_by_number: BTreeMap<u32, PlanTask>,
    pub evidence_rel: String,
    pub evidence_abs: PathBuf,
    pub evidence: ExecutionEvidence,
    pub(crate) authoritative_evidence_projection_fingerprint: Option<String>,
    pub source_spec_source: String,
    pub source_spec_path: PathBuf,
    pub execution_fingerprint: String,
    pub(crate) tracked_tree_sha_cache: OnceLock<Result<String, JsonFailure>>,
    pub(crate) semantic_workspace_snapshot_cache: OnceLock<
        Result<crate::execution::semantic_identity::SemanticWorkspaceSnapshot, JsonFailure>,
    >,
    pub(crate) reviewed_tree_sha_cache: RefCell<BTreeMap<String, String>>,
    pub(crate) head_sha_cache: OnceLock<Result<String, JsonFailure>>,
    pub(crate) release_base_branch_cache: OnceLock<Option<String>>,
    pub(crate) tracked_worktree_changes_excluding_execution_evidence_cache:
        OnceLock<Result<bool, JsonFailure>>,
}

impl ExecutionContext {
    pub(crate) fn current_tracked_tree_sha(&self) -> Result<String, JsonFailure> {
        match self
            .tracked_tree_sha_cache
            .get_or_init(|| current_repo_tracked_tree_sha(&self.runtime.repo_root))
        {
            Ok(tree_sha) => Ok(tree_sha.clone()),
            Err(error) => Err(error.clone()),
        }
    }

    pub(crate) fn repo_has_tracked_worktree_changes_excluding_execution_evidence(
        &self,
    ) -> Result<bool, JsonFailure> {
        match self
            .tracked_worktree_changes_excluding_execution_evidence_cache
            .get_or_init(|| {
                compute_repo_has_tracked_worktree_changes_excluding_execution_evidence(self)
            }) {
            Ok(has_changes) => Ok(*has_changes),
            Err(error) => Err(error.clone()),
        }
    }

    pub(crate) fn cached_reviewed_tree_sha(
        &self,
        reviewed_state_id: &str,
        resolver: impl FnOnce(&Path, &str) -> Result<String, JsonFailure>,
    ) -> Result<String, JsonFailure> {
        if let Some(cached) = self
            .reviewed_tree_sha_cache
            .borrow()
            .get(reviewed_state_id)
            .cloned()
        {
            return Ok(cached);
        }
        let resolved = resolver(&self.runtime.repo_root, reviewed_state_id)?;
        self.reviewed_tree_sha_cache
            .borrow_mut()
            .insert(reviewed_state_id.to_owned(), resolved.clone());
        Ok(resolved)
    }

    pub(crate) fn current_head_sha(&self) -> Result<String, JsonFailure> {
        match self
            .head_sha_cache
            .get_or_init(|| current_head_sha(&self.runtime.repo_root))
        {
            Ok(head_sha) => Ok(head_sha.clone()),
            Err(error) => Err(error.clone()),
        }
    }

    pub(crate) fn current_release_base_branch(&self) -> Option<String> {
        self.release_base_branch_cache
            .get_or_init(|| {
                resolve_release_base_branch(&self.runtime.git_dir, &self.runtime.branch_name)
            })
            .clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildEvidenceRequest {
    pub plan: PathBuf,
    pub all: bool,
    pub tasks: Vec<u32>,
    pub steps: Vec<(u32, u32)>,
    pub raw_steps: Vec<String>,
    pub include_open: bool,
    pub skip_manual_fallback: bool,
    pub continue_on_error: bool,
    pub dry_run: bool,
    pub max_jobs: u32,
    pub no_output: bool,
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildEvidenceCandidate {
    pub task: u32,
    pub step: u32,
    pub order_key: (u32, u32),
    pub target_kind: String,
    pub pre_invalidation_reason: String,
    pub verify_command: Option<String>,
    pub verify_mode: String,
    pub claim: String,
    pub files: Vec<String>,
    pub attempt_number: Option<u32>,
    pub artifact_epoch: Option<String>,
    pub needs_reopen: bool,
}

#[derive(Debug, Clone)]
pub struct CompleteRequest {
    pub task: u32,
    pub step: u32,
    pub source: String,
    pub claim: String,
    pub files: Vec<String>,
    pub verify_command: Option<String>,
    pub verification_summary: String,
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone)]
pub struct BeginRequest {
    pub task: u32,
    pub step: u32,
    pub execution_mode: Option<String>,
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone)]
pub struct NoteRequest {
    pub task: u32,
    pub step: u32,
    pub state: NoteState,
    pub message: String,
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone)]
pub struct ReopenRequest {
    pub task: u32,
    pub step: u32,
    pub source: String,
    pub reason: String,
    pub expect_execution_fingerprint: String,
}

#[derive(Debug, Clone)]
pub struct TransferRequest {
    pub reason: String,
    pub mode: TransferRequestMode,
}

#[derive(Debug, Clone)]
pub enum TransferRequestMode {
    RepairStep {
        repair_task: u32,
        repair_step: u32,
        source: String,
        expect_execution_fingerprint: String,
    },
    WorkflowHandoff {
        scope: String,
        to: String,
    },
}

impl ExecutionRuntime {
    pub fn discover(current_dir: &Path) -> Result<Self, JsonFailure> {
        let context = discover_repo_context(current_dir).map_err(JsonFailure::from)?;
        let identity = context.identity;

        Ok(Self {
            repo_root: identity.repo_root.clone(),
            git_dir: context.git_dir,
            branch_name: identity.branch_name.clone(),
            repo_slug: derive_repo_slug(&identity.repo_root, identity.remote_url.as_deref()),
            safe_branch: branch_storage_key(&identity.branch_name),
            state_dir: state_dir(),
        })
    }

    pub fn status(&self, args: &StatusArgs) -> Result<PlanExecutionStatus, JsonFailure> {
        let mut read_scope = load_execution_read_scope(self, &args.plan, true)?;
        apply_shared_routing_projection_to_read_scope(
            self,
            &mut read_scope,
            args.external_review_result_ready,
            false,
        )?;
        apply_public_read_invariants_to_status(&mut read_scope.status);
        Ok(read_scope.status)
    }

    pub fn topology_recommendation(
        &self,
        args: &RecommendArgs,
    ) -> Result<RecommendOutput, JsonFailure> {
        let read_scope = load_execution_read_scope(self, &args.plan, true)?;
        let context = read_scope.context;
        if read_scope.status.execution_started == "yes" {
            return Err(JsonFailure::new(
                FailureClass::RecommendAfterExecutionStart,
                "recommend is only valid before execution has started for this plan revision.",
            ));
        }
        let (chunking_strategy, evaluator_policy, reset_policy, review_stack, policy_reason_codes) =
            if let Some(preflight_acceptance) = preflight_acceptance_for_context(&context)? {
                (
                    preflight_acceptance.chunking_strategy,
                    preflight_acceptance.evaluator_policy,
                    preflight_acceptance.reset_policy,
                    preflight_acceptance.review_stack,
                    vec![String::from("reused_preflight_acceptance_policy_tuple")],
                )
            } else {
                (
                    default_preflight_chunking_strategy(),
                    default_preflight_evaluator_policy(),
                    default_preflight_reset_policy(),
                    default_preflight_review_stack(),
                    vec![String::from("default_preflight_policy_tuple")],
                )
            };

        let isolated_agents_available = match args.isolated_agents {
            Some(IsolatedAgentsArg::Available) => "yes",
            Some(IsolatedAgentsArg::Unavailable) => "no",
            None => "unknown",
        };
        let session_intent = args
            .session_intent
            .map(|value| value.as_str())
            .unwrap_or("unknown");
        let workspace_prepared = args
            .workspace_prepared
            .map(|value| value.as_str())
            .unwrap_or("unknown");
        let spec_document = parse_spec_file(&context.source_spec_path).map_err(|error| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Could not analyze execution topology because source spec {} is unreadable: {}",
                    context.source_spec_path.display(),
                    error.message()
                ),
            )
        })?;
        let topology_report = analyze_documents(&spec_document, &context.plan_document);
        let execution_context_key = recommendation_execution_context_key(&context);
        let downgrade_records =
            authoritative_matching_execution_topology_downgrade_records_checked(
                &context,
                &execution_context_key,
            )?;
        let learned_guidance = select_active_learned_topology_guidance(
            &downgrade_records,
            topology_report.plan_revision,
            &execution_context_key,
        );

        let tasks_independent = tasks_are_independent(&context.plan_document);
        let current_parallel_path_ready = topology_report.execution_topology_valid
            && topology_report.parallel_lane_ownership_valid
            && topology_report.parallel_workspace_isolation_valid
            && !topology_report.parallel_worktree_groups.is_empty()
            && tasks_independent
            && isolated_agents_available == "yes"
            && workspace_prepared == "yes";
        let topology_context = TopologySelectionContext {
            execution_context_key,
            tasks_independent,
            isolated_agents_available: isolated_agents_available.to_owned(),
            session_intent: session_intent.to_owned(),
            workspace_prepared: workspace_prepared.to_owned(),
            current_parallel_path_ready,
            learned_guidance,
        };
        let topology_recommendation = recommend_topology(&topology_report, &topology_context);

        Ok(RecommendOutput {
            selected_topology: topology_recommendation.selected_topology,
            recommended_skill: topology_recommendation.recommended_skill,
            reason: topology_recommendation.reason,
            decision_flags: topology_recommendation.decision_flags,
            reason_codes: topology_recommendation.reason_codes,
            learned_downgrade_reused: topology_recommendation.learned_downgrade_reused,
            chunking_strategy,
            evaluator_policy,
            reset_policy,
            review_stack,
            policy_reason_codes,
        })
    }

    pub fn preflight_gate(&self, args: &StatusArgs) -> Result<GateResult, JsonFailure> {
        self.preflight_gate_with_mode(args, true)
    }

    pub fn gate_contract(&self, args: &GateContractArgs) -> Result<GateResult, JsonFailure> {
        crate::execution::gates::gate_contract(self, args)
    }

    pub fn record_contract(&self, args: &RecordContractArgs) -> Result<GateResult, JsonFailure> {
        crate::execution::authority::record_contract(self, args)
    }

    pub fn gate_evaluator(&self, args: &GateEvaluatorArgs) -> Result<GateResult, JsonFailure> {
        crate::execution::gates::gate_evaluator(self, args)
    }

    pub fn record_evaluation(
        &self,
        args: &RecordEvaluationArgs,
    ) -> Result<GateResult, JsonFailure> {
        crate::execution::authority::record_evaluation(self, args)
    }

    pub fn gate_handoff(&self, args: &GateHandoffArgs) -> Result<GateResult, JsonFailure> {
        crate::execution::gates::gate_handoff(self, args)
    }

    pub fn record_handoff(&self, args: &RecordHandoffArgs) -> Result<GateResult, JsonFailure> {
        crate::execution::authority::record_handoff(self, args)
    }

    fn preflight_gate_with_mode(
        &self,
        args: &StatusArgs,
        persist_acceptance: bool,
    ) -> Result<GateResult, JsonFailure> {
        let context = if persist_acceptance {
            load_execution_context_for_exact_plan(self, &args.plan)?
        } else {
            load_execution_read_scope(self, &args.plan, true)?.context
        };
        let gate = preflight_from_context(&context);
        if persist_acceptance && gate.allowed {
            let acceptance = persist_preflight_acceptance(&context)?;
            ensure_preflight_authoritative_bootstrap(
                &context.runtime,
                RunIdentitySnapshot {
                    execution_run_id: acceptance.execution_run_id.clone(),
                    source_plan_path: context.plan_rel.clone(),
                    source_plan_revision: context.plan_document.plan_revision,
                },
                acceptance.chunk_id,
            )?;
        }
        Ok(gate)
    }

    pub fn review_gate(&self, args: &StatusArgs) -> Result<GateResult, JsonFailure> {
        match load_execution_context_for_exact_plan(self, &args.plan) {
            Ok(context) => {
                let gate_preview = gate_review_from_context(&context);
                if let Some(mut gate) = gate_review_command_phase_gate(&context, &gate_preview) {
                    gate.workspace_state_id = Some(status_workspace_state_id(&context)?);
                    gate.current_branch_reviewed_state_id =
                        current_branch_reviewed_state_id(&context);
                    gate.current_branch_closure_id = current_branch_closure_id(&context);
                    gate.finish_review_gate_pass_branch_closure_id =
                        finish_review_gate_pass_branch_closure_id(&context)?;
                    if !gate.allowed {
                        if gate_should_rederive_via_workflow_operator(
                            &context,
                            &gate,
                            args.external_review_result_ready,
                        ) {
                            apply_out_of_phase_gate_contract(
                                &context,
                                &mut gate,
                                args.external_review_result_ready,
                            );
                        } else {
                            apply_specific_gate_follow_up_contract(
                                &context,
                                &mut gate,
                                args.external_review_result_ready,
                            );
                        }
                    }
                    return Ok(gate);
                }
                let _write_authority = claim_step_write_authority(self)?;
                let context = load_execution_context_for_exact_plan(self, &args.plan)?;
                let mut gate = gate_review_from_context(&context);
                if gate.allowed {
                    persist_finish_review_gate_pass_checkpoint(&context)?;
                    gate.finish_review_gate_pass_branch_closure_id =
                        load_authoritative_transition_state(&context)?
                            .as_ref()
                            .and_then(|state| state.finish_review_gate_pass_branch_closure_id());
                }
                gate.workspace_state_id = Some(status_workspace_state_id(&context)?);
                gate.current_branch_reviewed_state_id = current_branch_reviewed_state_id(&context);
                gate.current_branch_closure_id = current_branch_closure_id(&context);
                if !gate.allowed {
                    if gate_should_rederive_via_workflow_operator(
                        &context,
                        &gate,
                        args.external_review_result_ready,
                    ) {
                        apply_out_of_phase_gate_contract(
                            &context,
                            &mut gate,
                            args.external_review_result_ready,
                        );
                    } else {
                        apply_specific_gate_follow_up_contract(
                            &context,
                            &mut gate,
                            args.external_review_result_ready,
                        );
                    }
                }
                Ok(gate)
            }
            Err(error) if error.error_class == FailureClass::PlanNotExecutionReady.as_str() => {
                let mut gate = GateState::default();
                gate.fail(
                    FailureClass::PlanNotExecutionReady,
                    "plan_not_execution_ready",
                    error.message,
                    "Refresh the approved plan/spec pair before continuing through workflow/operator or plan execution status.",
                );
                Ok(gate.finish())
            }
            Err(error) => Err(error),
        }
    }

    pub fn record_review_dispatch_authority(
        &self,
        args: &RecordReviewDispatchArgs,
    ) -> Result<RecordReviewDispatchOutput, JsonFailure> {
        let initial_context = match load_execution_context_for_exact_plan(self, &args.plan) {
            Ok(context) => context,
            Err(error) if error.error_class == FailureClass::PlanNotExecutionReady.as_str() => {
                return Ok(record_review_dispatch_blocked_output(
                    args,
                    review_dispatch_plan_not_ready_gate(error.message),
                ));
            }
            Err(error) => return Err(error),
        };
        let cycle_target = review_dispatch_cycle_target(&initial_context);
        if let Err(error) = validate_review_dispatch_request(&initial_context, args, cycle_target) {
            if error.error_class == FailureClass::ExecutionStateNotReady.as_str() {
                return Ok(record_review_dispatch_blocked_output_from_gate(
                    &initial_context,
                    args,
                    review_dispatch_out_of_phase_gate(error.message),
                ));
            }
            return Err(error);
        }
        let gate = review_dispatch_gate_from_context(&initial_context, args, cycle_target);
        if !gate.allowed {
            return Ok(record_review_dispatch_blocked_output_from_gate(
                &initial_context,
                args,
                gate,
            ));
        }
        ensure_review_dispatch_authoritative_bootstrap(&initial_context)?;
        let context = match load_execution_context_for_exact_plan(self, &args.plan) {
            Ok(context) => context,
            Err(error) if error.error_class == FailureClass::PlanNotExecutionReady.as_str() => {
                return Ok(record_review_dispatch_blocked_output(
                    args,
                    review_dispatch_plan_not_ready_gate(error.message),
                ));
            }
            Err(error) => return Err(error),
        };
        let cycle_target = review_dispatch_cycle_target(&context);
        if let Err(error) = validate_review_dispatch_request(&context, args, cycle_target) {
            if error.error_class == FailureClass::ExecutionStateNotReady.as_str() {
                return Ok(record_review_dispatch_blocked_output_from_gate(
                    &context,
                    args,
                    review_dispatch_out_of_phase_gate(error.message),
                ));
            }
            return Err(error);
        }
        let gate = review_dispatch_gate_from_context(&context, args, cycle_target);
        if !gate.allowed {
            return Ok(record_review_dispatch_blocked_output_from_gate(
                &context, args, gate,
            ));
        }
        let action = record_review_dispatch_strategy_checkpoint(&context, args, cycle_target)?;
        let refreshed = load_execution_context_for_exact_plan(self, &args.plan)?;
        let gate = review_dispatch_gate_from_context(&refreshed, args, cycle_target);
        let dispatch_id = match action {
            ReviewDispatchMutationAction::Recorded => {
                current_review_dispatch_id_from_lineage(&refreshed, args)?
            }
            ReviewDispatchMutationAction::AlreadyCurrent => {
                current_review_dispatch_id_if_still_current(&refreshed, args)?
            }
        };
        if dispatch_id.is_none() {
            return Err(JsonFailure::new(
                FailureClass::ExecutionStateNotReady,
                "record-review-dispatch recorded lineage but could not reload the current dispatch id.",
            ));
        }
        Ok(RecordReviewDispatchOutput {
            allowed: gate.allowed,
            failure_class: gate.failure_class.clone(),
            reason_codes: gate.reason_codes.clone(),
            warning_codes: gate.warning_codes.clone(),
            diagnostics: gate.diagnostics.clone(),
            code: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
            scope: review_dispatch_scope_label(args.scope),
            action: match action {
                ReviewDispatchMutationAction::Recorded => String::from("recorded"),
                ReviewDispatchMutationAction::AlreadyCurrent => String::from("already_current"),
            },
            dispatch_id,
            recorded_at: matches!(action, ReviewDispatchMutationAction::Recorded)
                .then(|| Timestamp::now().to_string()),
        })
    }

    pub fn finish_gate(&self, args: &StatusArgs) -> Result<GateResult, JsonFailure> {
        let context = load_execution_context_for_exact_plan(self, &args.plan)?;
        let mut gate = gate_finish_from_context(&context);
        gate.workspace_state_id = Some(status_workspace_state_id(&context)?);
        gate.current_branch_reviewed_state_id = current_branch_reviewed_state_id(&context);
        gate.current_branch_closure_id = current_branch_closure_id(&context);
        gate.finish_review_gate_pass_branch_closure_id =
            finish_review_gate_pass_branch_closure_id(&context)?;
        if !gate.allowed {
            if gate_should_rederive_via_workflow_operator(
                &context,
                &gate,
                args.external_review_result_ready,
            ) {
                apply_out_of_phase_gate_contract(
                    &context,
                    &mut gate,
                    args.external_review_result_ready,
                );
            } else {
                apply_specific_gate_follow_up_contract(
                    &context,
                    &mut gate,
                    args.external_review_result_ready,
                );
            }
        }
        Ok(gate)
    }
}

fn project_routing_decision_onto_status(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    routing: &ExecutionRoutingState,
    route_decision: &RouteDecision,
    require_exact_execution_command: bool,
    authoritative_stale_target: Option<
        crate::execution::next_action::AuthoritativeStaleReentryTarget<'_>,
    >,
) {
    if !require_exact_execution_command
        && should_preserve_local_preflight_route(status, route_decision)
    {
        status.phase = Some(String::from("execution_preflight"));
        status.phase_detail = String::from("execution_preflight_required");
        status.review_state_status = route_decision.review_state_status.clone();
        status.recording_context = None;
        status.execution_command_context = None;
        status.execution_reentry_target_source = None;
        status.public_repair_targets.clear();
        status.next_action = String::from("execution preflight");
        status.recommended_command = None;
        status.blocking_task = None;
        status.blocking_scope = None;
        status.external_wait_state = None;
        status.blocking_reason_codes.clear();
        status.projection_diagnostics.clear();
        return;
    }
    status.phase = Some(route_decision.phase.clone());
    status.harness_phase = if status.execution_started == "no"
        && matches!(status.harness_phase, HarnessPhase::ImplementationHandoff)
    {
        status.harness_phase
    } else {
        match route_decision.phase.as_str() {
            "document_release_pending" => HarnessPhase::DocumentReleasePending,
            "final_review_pending" => HarnessPhase::FinalReviewPending,
            "qa_pending" => HarnessPhase::QaPending,
            "ready_for_branch_completion" => HarnessPhase::ReadyForBranchCompletion,
            "pivot_required" => HarnessPhase::PivotRequired,
            "handoff_required" => HarnessPhase::HandoffRequired,
            "executing" | "task_closure_pending" => HarnessPhase::Executing,
            _ => status.harness_phase,
        }
    };
    status.phase_detail = route_decision.phase_detail.clone();
    status.review_state_status = route_decision.review_state_status.clone();
    status.recording_context =
        routing
            .recording_context
            .as_ref()
            .map(|context| PublicRecordingContext {
                task_number: context.task_number,
                dispatch_id: context.dispatch_id.clone(),
                branch_closure_id: context.branch_closure_id.clone(),
            });
    status.execution_command_context =
        routing
            .execution_command_context
            .as_ref()
            .map(|context| PublicExecutionCommandContext {
                command_kind: context.command_kind.clone(),
                task_number: context.task_number,
                step_id: context.step_id,
            });
    status.next_action = route_decision.next_action.clone();
    status.recommended_command = route_decision.recommended_command.clone();
    status.blocking_task = routing.blocking_task;
    status.blocking_scope = routing.blocking_scope.clone();
    status.external_wait_state = routing.external_wait_state.clone();
    status.blocking_reason_codes = routing.blocking_reason_codes.clone();
    status.projection_diagnostics = public_task_boundary_decision(status).diagnostic_reason_codes;
    let public_execution_reentry_target = (route_decision.phase_detail
        == "execution_reentry_required")
        .then(|| {
            execution_reentry_target(
                context,
                status,
                &context.plan_rel,
                crate::execution::next_action::NextActionAuthorityInputs {
                    authoritative_stale_target,
                    ..crate::execution::next_action::NextActionAuthorityInputs::default()
                },
            )
        })
        .flatten();
    status.execution_reentry_target_source = public_execution_reentry_target
        .as_ref()
        .map(|target| target.source.as_str().to_owned());
    status.public_repair_targets = public_execution_reentry_target
        .map(|target| {
            vec![PublicRepairTarget {
                command_kind: String::from("reopen"),
                task: Some(target.task),
                step: target.step,
                reason_code: target.reason_code,
                source_record_id: target
                    .source_record_id
                    .or_else(|| Some(target.source.as_str().to_owned())),
                expires_when_fingerprint_changes: true,
            }]
        })
        .unwrap_or_default();
    if route_decision.phase_detail == "branch_closure_recording_required_for_release_readiness"
        && route_decision.review_state_status == "missing_current_closure"
        && status.current_branch_closure_id.is_none()
    {
        status.blocking_task = None;
        status.blocking_scope = Some(String::from("branch"));
        status.blocking_records = vec![StatusBlockingRecord {
            code: String::from("missing_current_closure"),
            scope_type: String::from("branch"),
            scope_key: String::from("current"),
            record_type: String::from("branch_closure"),
            record_id: None,
            review_state_status: String::from("missing_current_closure"),
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "An authoritative current branch closure record is required before late-stage progression can continue.",
            ),
        }];
    }
    if route_decision.phase_detail == "execution_reentry_required"
        && let Some(task_number) = status
            .execution_command_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        status.blocking_scope = Some(String::from("task"));
        status.blocking_task = Some(task_number);
    }
}

pub(crate) fn project_persisted_public_repair_targets(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    source_route_decision_hash: Option<&str>,
) {
    let Some(authoritative_state) = authoritative_state else {
        return;
    };
    if legacy_repair_follow_up_unbound(Some(authoritative_state)) {
        push_status_warning_code_once(status, "legacy_follow_up_unbound");
    }
    let persisted_follow_up_record =
        resolve_actionable_repair_follow_up_for_status_with_source_hash(
            context,
            status,
            Some(authoritative_state),
            source_route_decision_hash,
        );
    let persisted_follow_up = persisted_follow_up_record
        .as_ref()
        .map(|record| record.kind.public_token());
    if let Some(follow_up) = persisted_follow_up {
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("repair-review-state"),
                task: persisted_follow_up_record
                    .as_ref()
                    .and_then(|record| record.target_task),
                step: persisted_follow_up_record
                    .as_ref()
                    .and_then(|record| record.target_step),
                reason_code: format!("persisted_review_state_repair_follow_up:{follow_up}"),
                source_record_id: persisted_follow_up_record
                    .as_ref()
                    .and_then(|record| record.target_record_id.clone())
                    .or_else(|| Some(format!("review_state_repair_follow_up:{follow_up}"))),
                expires_when_fingerprint_changes: true,
            },
        );
    }
    for target in authoritative_state.explicit_reopen_repair_targets() {
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("reopen"),
                task: Some(target.task),
                step: Some(target.step),
                reason_code: String::from("explicit_reopen_repair_target"),
                source_record_id: target
                    .target_record_id
                    .or_else(|| Some(execution_step_repair_target_id(target.task, target.step))),
                expires_when_fingerprint_changes: target.expires_on_plan_fingerprint_change,
            },
        );
    }
    for record in authoritative_state
        .current_task_closure_results()
        .into_values()
    {
        if current_task_closure_postconditions_would_mutate(
            authoritative_state,
            record.task,
            &record.closure_record_id,
            &record.reviewed_state_id,
        ) {
            push_public_repair_target_once(
                status,
                PublicRepairTarget {
                    command_kind: String::from("close-current-task"),
                    task: Some(record.task),
                    step: None,
                    reason_code: String::from("authoritative_task_closure_postcondition_cleanup"),
                    source_record_id: Some(record.closure_record_id),
                    expires_when_fingerprint_changes: true,
                },
            );
        }
    }
    for task in context.tasks_by_number.keys().copied() {
        if authoritative_state
            .current_task_closure_result(task)
            .is_some()
            || authoritative_state.task_review_dispatch_id(task).is_none()
            || !context
                .steps
                .iter()
                .filter(|step| step.task_number == task)
                .all(|step| step.checked)
        {
            continue;
        }
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("close-current-task"),
                task: Some(task),
                step: None,
                reason_code: String::from("task_review_dispatch_closure_ready"),
                source_record_id: Some(format!("task-review-dispatch:task-{task}")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
    if authoritative_state.execution_run_id_opt().is_some()
        && load_preflight_acceptance(&context.runtime).is_err()
    {
        for entry in authoritative_state
            .raw_current_task_closure_state_entries()
            .into_iter()
            .filter(|entry| entry.task.is_some())
        {
            push_public_repair_target_once(
                status,
                PublicRepairTarget {
                    command_kind: String::from("close-current-task"),
                    task: entry.task,
                    step: None,
                    reason_code: String::from("authoritative_preflight_recovery_task_closure"),
                    source_record_id: entry.closure_record_id,
                    expires_when_fingerprint_changes: true,
                },
            );
        }
    }
    if persisted_follow_up != Some("execution_reentry") {
        return;
    }
    let Some(record) = persisted_follow_up_record.as_ref() else {
        return;
    };
    let Some(task) = record.target_task else {
        return;
    };
    let Some(step) = record.target_step else {
        return;
    };
    let target = PublicRepairTarget {
        command_kind: String::from("reopen"),
        task: Some(task),
        step: Some(step),
        reason_code: String::from("persisted_execution_reentry_follow_up"),
        source_record_id: record
            .target_record_id
            .clone()
            .or_else(|| Some(format!("review_state_repair_follow_up_task:{task}"))),
        expires_when_fingerprint_changes: true,
    };
    push_public_repair_target_once(status, target);
}

fn push_public_repair_target_once(status: &mut PlanExecutionStatus, target: PublicRepairTarget) {
    if !status.public_repair_targets.iter().any(|existing| {
        existing.command_kind == target.command_kind
            && existing.task == target.task
            && existing.step == target.step
    }) {
        status.public_repair_targets.push(target);
    }
}

fn explicit_public_target_allowed(status: &PlanExecutionStatus) -> bool {
    status.phase_detail != "runtime_reconcile_required"
        && status.state_kind != "blocked_runtime_bug"
}

fn route_exposes_repair_review_state_target(status: &PlanExecutionStatus) -> bool {
    status
        .recommended_command
        .as_deref()
        .is_some_and(|command| {
            command.starts_with("featureforge plan execution repair-review-state --plan ")
        })
        || status.next_public_action.as_ref().is_some_and(|action| {
            action
                .command
                .starts_with("featureforge plan execution repair-review-state --plan ")
        })
        || status.review_state_status != "clean"
        || matches!(
            status.phase_detail.as_str(),
            "execution_reentry_required"
                | "final_review_dispatch_required"
                | "final_review_outcome_pending"
                | "release_readiness_recording_ready"
                | "runtime_reconcile_required"
        )
        || (status.phase_detail == "finish_completion_gate_ready"
            && status.state_kind == "terminal")
        || status.blocking_reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_missing"
                    | "prior_task_review_dispatch_stale"
                    | "stale_provenance"
                    | "task_closure_baseline_repair_candidate"
            )
        })
}

fn project_public_route_mutation_targets(status: &mut PlanExecutionStatus) {
    if status.phase_detail == "task_closure_recording_ready"
        && let Some(task) = status
            .recording_context
            .as_ref()
            .and_then(|context| context.task_number)
    {
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("close-current-task"),
                task: Some(task),
                step: None,
                reason_code: String::from("route_task_closure_recording_ready"),
                source_record_id: Some(String::from("route_decision:task_closure_recording_ready")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
    let route_exposes_task_closure_repair = status.phase_detail == "task_closure_recording_ready"
        && status.blocking_reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_missing"
                    | "prior_task_review_dispatch_stale"
                    | "task_closure_baseline_repair_candidate"
            )
        });
    let repair_review_state_target_allowed = explicit_public_target_allowed(status)
        || status.phase_detail == "runtime_reconcile_required";
    if (route_exposes_task_closure_repair || route_exposes_repair_review_state_target(status))
        && repair_review_state_target_allowed
    {
        let reason_code = if route_exposes_task_closure_repair {
            "route_task_closure_repair_state_refresh"
        } else {
            "route_repair_review_state_available"
        };
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("repair-review-state"),
                task: None,
                step: None,
                reason_code: String::from(reason_code),
                source_record_id: Some(format!("route_decision:{}", status.phase_detail)),
                expires_when_fingerprint_changes: true,
            },
        );
    }

    let recommended_advance = status
        .recommended_command
        .as_deref()
        .is_some_and(|command| {
            command.starts_with("featureforge plan execution advance-late-stage --plan ")
        })
        || status.next_public_action.as_ref().is_some_and(|action| {
            action
                .command
                .starts_with("featureforge plan execution advance-late-stage --plan ")
        });
    if (recommended_advance || status.phase_detail == "final_review_outcome_pending")
        && explicit_public_target_allowed(status)
    {
        push_public_repair_target_once(
            status,
            PublicRepairTarget {
                command_kind: String::from("advance-late-stage"),
                task: None,
                step: None,
                reason_code: String::from("route_advance_late_stage_ready"),
                source_record_id: Some(String::from("route_decision:advance_late_stage")),
                expires_when_fingerprint_changes: true,
            },
        );
    }
}

fn should_preserve_local_preflight_route(
    status: &PlanExecutionStatus,
    route_decision: &RouteDecision,
) -> bool {
    status.execution_started == "no"
        && route_decision.phase_detail == "execution_reentry_required"
        && route_decision.review_state_status == "clean"
        && status.active_task.is_none()
        && status.active_step.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && status.current_task_closures.is_empty()
        && status.reason_codes.is_empty()
}

pub(crate) fn apply_shared_routing_projection_to_read_scope(
    _runtime: &ExecutionRuntime,
    read_scope: &mut ExecutionReadScope,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<(), JsonFailure> {
    apply_shared_routing_projection_to_read_scope_with_routing(
        read_scope,
        external_review_result_ready,
        require_exact_execution_command,
    )?;
    Ok(())
}

pub(crate) fn apply_shared_routing_projection_to_read_scope_with_routing(
    read_scope: &mut ExecutionReadScope,
    external_review_result_ready: bool,
    require_exact_execution_command: bool,
) -> Result<(ExecutionRoutingState, RouteDecision), JsonFailure> {
    project_persisted_public_repair_targets(
        &read_scope.context,
        &mut read_scope.status,
        read_scope.authoritative_state.as_ref(),
        None,
    );
    let (routing, route_decision, runtime_state) =
        crate::execution::router::project_runtime_routing_state_with_reduced_state(
            read_scope,
            external_review_result_ready,
            require_exact_execution_command,
        )?;
    project_routing_decision_onto_status(
        &read_scope.context,
        &mut read_scope.status,
        &routing,
        &route_decision,
        require_exact_execution_command,
        runtime_state
            .gate_snapshot
            .earliest_task_stale_target_details()
            .and_then(
                crate::execution::next_action::AuthoritativeStaleReentryTarget::from_stale_target,
            ),
    );
    let source_route_decision_hash = repair_follow_up_source_decision_hash(&route_decision);
    project_persisted_public_repair_targets(
        &read_scope.context,
        &mut read_scope.status,
        read_scope.authoritative_state.as_ref(),
        source_route_decision_hash.as_deref(),
    );
    read_scope.status.stale_unreviewed_closures =
        runtime_state.status.stale_unreviewed_closures.clone();
    let fallback_gate_finish;
    let gate_finish = match runtime_state.gate_snapshot.gate_finish.as_ref() {
        Some(gate_finish) => gate_finish,
        None => {
            fallback_gate_finish = GateState::default().finish();
            &fallback_gate_finish
        }
    };
    read_scope.status.blocking_records =
        compute_status_blocking_records(&read_scope.context, &read_scope.status, gate_finish)?;
    let route_decision = route_decision_with_status_blockers(route_decision, &read_scope.status);
    read_scope.status.state_kind = route_decision.state_kind.clone();
    read_scope.status.next_public_action = route_decision.next_public_action.clone();
    read_scope.status.blockers = route_decision.blockers.clone();
    project_public_route_mutation_targets(&mut read_scope.status);
    project_reducer_stale_target_source(&runtime_state, &mut read_scope.status);
    read_scope.status.semantic_workspace_tree_id = runtime_state
        .semantic_workspace
        .semantic_workspace_tree_id
        .clone();
    read_scope.status.raw_workspace_tree_id = Some(
        runtime_state
            .semantic_workspace
            .raw_workspace_tree_id
            .clone(),
    );
    if require_exact_execution_command {
        require_public_exact_execution_command(&read_scope.context, &read_scope.status)?;
    }
    read_scope.runtime_state = Some(runtime_state);
    Ok((routing, route_decision))
}

fn project_reducer_stale_target_source(
    runtime_state: &RuntimeState,
    status: &mut PlanExecutionStatus,
) {
    let Some(blocking_task) = status.blocking_task else {
        return;
    };
    let Some(stale_target) = runtime_state
        .gate_snapshot
        .earliest_task_stale_target_details()
    else {
        if status.phase_detail == "task_closure_recording_ready"
            && status.reason_codes.iter().any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    "prior_task_current_closure_missing" | "task_closure_baseline_repair_candidate"
                )
            })
        {
            status.execution_reentry_target_source = Some(String::from("baseline_bridge"));
        }
        return;
    };
    if stale_target.task != Some(blocking_task) {
        return;
    }
    let execution_reentry_target_source = match stale_target.source.as_str() {
        "closure_graph" => "closure_graph_stale_target",
        source => source,
    };
    status.execution_reentry_target_source = Some(execution_reentry_target_source.to_owned());
    for target in &mut status.public_repair_targets {
        if target.task == Some(blocking_task) && target.command_kind == "reopen" {
            target.reason_code.clone_from(&stale_target.reason_code);
            target.source_record_id = stale_target
                .record_id
                .clone()
                .or_else(|| Some(stale_target.source.as_str().to_owned()));
        }
    }
}

pub(crate) fn status_from_context_with_shared_routing(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let mut read_scope =
        load_execution_read_scope_for_mutation(runtime, Path::new(&context.plan_rel), true)?;
    apply_shared_routing_projection_to_read_scope(
        runtime,
        &mut read_scope,
        external_review_result_ready,
        true,
    )?;
    Ok(read_scope.status)
}

pub(crate) fn apply_public_read_invariants_to_status(status: &mut PlanExecutionStatus) {
    crate::execution::invariants::inject_read_surface_invariant_test_violation(status);
    crate::execution::invariants::apply_read_surface_invariants(status);
}

pub(crate) fn public_status_from_context_with_shared_routing(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let mut status =
        status_from_context_with_shared_routing(runtime, context, external_review_result_ready)?;
    apply_public_read_invariants_to_status(&mut status);
    Ok(status)
}

pub(crate) fn public_status_from_supplied_context_with_shared_routing(
    context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let mut context = context.clone();
    let authoritative_state = load_authoritative_transition_state_relaxed(&context)?;
    overlay_execution_evidence_attempts_from_authority(&mut context, authoritative_state.as_ref())?;
    overlay_step_state_from_authority(&mut context, authoritative_state.as_ref())?;
    refresh_execution_fingerprint(&mut context);
    let derived = derive_execution_truth_from_authority(&context, authoritative_state.as_ref())?;
    let mut read_scope = ExecutionReadScope {
        context,
        status: derived.status,
        overlay: derived.overlay,
        authoritative_state,
        runtime_state: None,
    };
    apply_shared_routing_projection_to_read_scope_with_routing(
        &mut read_scope,
        external_review_result_ready,
        true,
    )?;
    apply_public_read_invariants_to_status(&mut read_scope.status);
    Ok(read_scope.status)
}

pub(crate) struct ExecutionReadScope {
    pub(crate) context: ExecutionContext,
    pub(crate) status: PlanExecutionStatus,
    pub(crate) overlay: Option<StatusAuthoritativeOverlay>,
    pub(crate) authoritative_state: Option<AuthoritativeTransitionState>,
    pub(crate) runtime_state: Option<RuntimeState>,
}

pub(crate) struct ExecutionDerivedTruth {
    pub(crate) status: PlanExecutionStatus,
    pub(crate) overlay: Option<StatusAuthoritativeOverlay>,
    pub(crate) task_review_dispatch_id: Option<String>,
    pub(crate) final_review_dispatch_authority: FinalReviewDispatchAuthority,
}

pub(crate) struct SharedRepairReviewStateRerouteDecision {
    pub(crate) branch_reroute_still_valid: bool,
    pub(crate) branch_drift_escapes_late_stage_surface: bool,
    pub(crate) late_stage_surface_not_declared: bool,
    pub(crate) persisted_repair_follow_up: Option<String>,
    pub(crate) raw_late_stage_review_state_status: Option<&'static str>,
    pub(crate) task_scope_repair_precedence_active: bool,
    pub(crate) repair_reroute: crate::execution::current_truth::ReviewStateRepairReroute,
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
    let branch_reroute_assessment = branch_closure_rerecording_assessment(context).ok();
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
        branch_reroute_still_valid,
        branch_drift_escapes_late_stage_surface,
        late_stage_surface_not_declared,
        persisted_repair_follow_up,
        raw_late_stage_review_state_status,
        task_scope_repair_precedence_active,
        repair_reroute,
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FinalReviewDispatchAuthority {
    pub(crate) dispatch_id: Option<String>,
    pub(crate) lineage_present: bool,
}

pub(crate) fn current_task_review_dispatch_id_for_status(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Option<String> {
    let current_task_lineage_fingerprint = status
        .blocking_task
        .and_then(|task_number| task_completion_lineage_fingerprint(context, task_number));
    let current_task_semantic_reviewed_state_id = status.blocking_task.and_then(|_| {
        semantic_workspace_snapshot(context)
            .ok()
            .map(|snapshot| snapshot.semantic_workspace_tree_id)
    });
    shared_current_task_review_dispatch_id(
        status.blocking_task,
        current_task_lineage_fingerprint.as_deref(),
        current_task_semantic_reviewed_state_id.as_deref(),
        None,
        overlay,
    )
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
    let mut dispatch_id = shared_current_final_review_dispatch_id(
        usable_current_branch_closure_id,
        overlay,
    )
    .or_else(|| {
        authoritative_state.and_then(|state| {
            if state.current_final_review_branch_closure_id() != usable_current_branch_closure_id {
                return None;
            }
            state
                .current_final_review_dispatch_id()
                .map(str::trim)
                .filter(|dispatch_id| !dispatch_id.is_empty())
                .map(ToOwned::to_owned)
        })
    });
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
    let overlay = status_overlay_from_authoritative_snapshot(context, authoritative_state)?;
    let status = status_from_context_with_overlay(
        context,
        overlay.as_ref(),
        true,
        authoritative_state,
        true,
        gate_projection,
    )?;
    let task_review_dispatch_id =
        current_task_review_dispatch_id_for_status(context, &status, overlay.as_ref());
    let final_review_dispatch_authority = current_final_review_dispatch_authority_for_context(
        context,
        overlay.as_ref(),
        authoritative_state,
    );
    Ok(ExecutionDerivedTruth {
        status,
        overlay,
        task_review_dispatch_id,
        final_review_dispatch_authority,
    })
}

fn gate_follow_up_routing_state(
    context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Option<ExecutionRoutingState> {
    let read_scope =
        load_execution_read_scope(&context.runtime, Path::new(&context.plan_rel), true).ok()?;
    crate::execution::router::project_runtime_routing_state_with_exact_command_requirement(
        &read_scope,
        external_review_result_ready,
        false,
    )
    .ok()
    .map(|(routing, _)| routing)
}

fn required_follow_up_kind_from_routing(routing: &ExecutionRoutingState) -> Option<FollowUpKind> {
    normalize_follow_up_alias(
        required_follow_up_from_routing(routing).as_deref(),
        FollowUpAliasContext::PublicRouting,
    )
}

fn current_branch_closure_missing_gate_follow_up(
    routing: Option<&ExecutionRoutingState>,
) -> FollowUpKind {
    missing_branch_closure_gate_follow_up(
        routing.map(|routing| routing.review_state_status.as_str()),
        routing.and_then(required_follow_up_kind_from_routing),
    )
}

fn gate_should_rederive_via_workflow_operator(
    context: &ExecutionContext,
    gate: &GateResult,
    external_review_result_ready: bool,
) -> bool {
    gate.allowed
        || specific_gate_direct_recommended_command(context, gate, external_review_result_ready)
            .is_none()
}

fn specific_gate_reason_is_explicit_direct_follow_up(
    gate: &GateResult,
    routing: Option<&ExecutionRoutingState>,
) -> Option<FollowUpKind> {
    direct_gate_follow_up_from_reason_codes(
        gate.reason_codes.iter().map(String::as_str),
        routing.map(|routing| routing.review_state_status.as_str()),
        routing.and_then(required_follow_up_kind_from_routing),
    )
}

fn specific_gate_reason_is_direct_follow_up(
    context: &ExecutionContext,
    gate: &GateResult,
    external_review_result_ready: bool,
) -> Option<FollowUpKind> {
    let routing = gate_follow_up_routing_state(context, external_review_result_ready);
    if let Some(reason) = specific_gate_reason_is_explicit_direct_follow_up(gate, routing.as_ref())
    {
        return Some(reason);
    }
    if let Some(routing) = routing.as_ref() {
        if required_follow_up_kind_from_routing(routing) == Some(FollowUpKind::RepairReviewState) {
            return Some(FollowUpKind::RepairReviewState);
        }
        if routing.review_state_status == "missing_current_closure" {
            return Some(current_branch_closure_missing_gate_follow_up(Some(routing)));
        }
    }
    None
}

fn specific_gate_direct_recommended_command(
    context: &ExecutionContext,
    gate: &GateResult,
    external_review_result_ready: bool,
) -> Option<String> {
    let routing = gate_follow_up_routing_state(context, external_review_result_ready);
    if let Some(follow_up) =
        specific_gate_reason_is_explicit_direct_follow_up(gate, routing.as_ref())
        && let Some(command) = materialized_follow_up_kind_command(
            follow_up,
            Path::new(&context.plan_rel),
            external_review_result_ready,
        )
    {
        return Some(command);
    }

    if let Some(route_decision) = routing
        .as_ref()
        .and_then(|routing| routing.route_decision.as_ref())
        && let Some(command) = route_decision.recommended_command.as_deref()
    {
        return Some(command.to_owned());
    }

    specific_gate_reason_is_direct_follow_up(context, gate, external_review_result_ready).and_then(
        |follow_up| {
            materialized_follow_up_kind_command(
                follow_up,
                Path::new(&context.plan_rel),
                external_review_result_ready,
            )
        },
    )
}

fn apply_out_of_phase_gate_contract(
    context: &ExecutionContext,
    gate: &mut GateResult,
    external_review_result_ready: bool,
) {
    if let Some(command) = gate_follow_up_routing_state(context, external_review_result_ready)
        .and_then(|routing| routing.route_decision)
        .and_then(|decision| decision.recommended_command)
        .filter(|command| !command.starts_with("featureforge workflow operator --plan "))
    {
        gate.code = None;
        gate.recommended_command = Some(command);
        gate.rederive_via_workflow_operator = None;
        return;
    }
    gate.code = Some(String::from("out_of_phase_requery_required"));
    gate.recommended_command = Some(workflow_operator_requery_command(
        Path::new(&context.plan_rel),
        external_review_result_ready,
    ));
    gate.rederive_via_workflow_operator = Some(true);
}

fn apply_out_of_phase_requery_contract(
    context: &ExecutionContext,
    gate: &mut GateResult,
    external_review_result_ready: bool,
) {
    gate.code = Some(String::from("out_of_phase_requery_required"));
    gate.recommended_command = Some(workflow_operator_requery_command(
        Path::new(&context.plan_rel),
        external_review_result_ready,
    ));
    gate.rederive_via_workflow_operator = Some(true);
}

fn apply_specific_gate_follow_up_contract(
    context: &ExecutionContext,
    gate: &mut GateResult,
    external_review_result_ready: bool,
) {
    if gate.recommended_command.is_some() {
        return;
    }
    gate.recommended_command =
        specific_gate_direct_recommended_command(context, gate, external_review_result_ready);
}

fn record_review_dispatch_blocked_output(
    args: &RecordReviewDispatchArgs,
    gate: GateResult,
) -> RecordReviewDispatchOutput {
    let GateResult {
        failure_class,
        reason_codes,
        warning_codes,
        diagnostics,
        code,
        recommended_command,
        rederive_via_workflow_operator,
        ..
    } = gate;
    RecordReviewDispatchOutput {
        allowed: false,
        failure_class,
        reason_codes,
        warning_codes,
        diagnostics,
        code,
        recommended_command,
        rederive_via_workflow_operator,
        scope: review_dispatch_scope_label(args.scope),
        action: String::from("blocked"),
        dispatch_id: None,
        recorded_at: None,
    }
}

fn record_review_dispatch_blocked_output_from_gate(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    mut gate: GateResult,
) -> RecordReviewDispatchOutput {
    if matches!(args.scope, ReviewDispatchScopeArg::FinalReview)
        && gate.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "derived_review_state_missing" | "current_branch_reviewed_state_id_missing"
            )
        })
    {
        gate.recommended_command = Some(format!(
            "featureforge plan execution repair-review-state --plan {}",
            context.plan_rel
        ));
    } else if matches!(args.scope, ReviewDispatchScopeArg::FinalReview)
        && gate.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "branch_closure_recording_required_for_release_readiness"
                    | "release_blocker_resolution_required"
                    | "release_readiness_recording_ready"
            )
        })
    {
        gate.recommended_command = if gate
            .reason_codes
            .iter()
            .any(|code| code == "branch_closure_recording_required_for_release_readiness")
        {
            Some(format!(
                "featureforge plan execution advance-late-stage --plan {}",
                context.plan_rel
            ))
        } else {
            Some(format!(
                "featureforge plan execution advance-late-stage --plan {} --result ready|blocked --summary-file <path>",
                context.plan_rel
            ))
        };
    } else {
        let routing = gate_follow_up_routing_state(context, false);
        let direct_follow_up =
            specific_gate_reason_is_explicit_direct_follow_up(&gate, routing.as_ref());
        let task_scope_prior_task_requires_requery =
            matches!(args.scope, ReviewDispatchScopeArg::Task)
                && gate
                    .reason_codes
                    .iter()
                    .any(|code| code.starts_with("prior_task_"));
        if gate.allowed || direct_follow_up.is_none() || task_scope_prior_task_requires_requery {
            apply_out_of_phase_requery_contract(context, &mut gate, false);
        } else {
            gate.recommended_command = match direct_follow_up {
                Some(follow_up) => materialized_follow_up_kind_command(
                    follow_up,
                    Path::new(&context.plan_rel),
                    false,
                ),
                None => None,
            };
        }
    }
    record_review_dispatch_blocked_output(args, gate)
}

fn review_dispatch_scope_label(scope: ReviewDispatchScopeArg) -> String {
    match scope {
        ReviewDispatchScopeArg::Task => String::from("task"),
        ReviewDispatchScopeArg::FinalReview => String::from("final-review"),
    }
}

fn review_dispatch_out_of_phase_gate(message: String) -> GateResult {
    let mut gate = GateState::default();
    gate.fail(
        FailureClass::ExecutionStateNotReady,
        "record_review_dispatch_out_of_phase",
        message,
        "Run `featureforge workflow operator --plan <approved-plan-path>` to re-derive the current workflow phase before recording review dispatch.",
    );
    gate.finish()
}

fn review_dispatch_plan_not_ready_gate(message: String) -> GateResult {
    let mut gate = GateState::default();
    gate.fail(
        FailureClass::PlanNotExecutionReady,
        "plan_not_execution_ready",
        message,
        "Refresh the approved plan/spec pair before continuing through workflow/operator or plan execution status.",
    );
    gate.finish()
}

enum ReviewDispatchMutationAction {
    Recorded,
    AlreadyCurrent,
}

fn gate_review_command_phase_gate(
    context: &ExecutionContext,
    gate_review: &GateResult,
) -> Option<GateResult> {
    if !gate_review.allowed {
        return None;
    }
    let checkpoint_current = matches!(
        finish_review_gate_checkpoint_matches_current_branch_closure(context),
        Ok(true)
    );
    if !checkpoint_current || !gate_finish_from_context(context).allowed {
        return None;
    }
    let mut gate = GateState::default();
    gate.fail(
        FailureClass::ExecutionStateNotReady,
        "finish_review_gate_already_current",
        "gate-review is out of phase because the current branch closure already has a fresh persisted finish-review gate checkpoint.",
        format!(
            "Run `featureforge workflow operator --plan {}` and follow the recommended public next step.",
            context.plan_rel
        ),
    );
    Some(gate.finish())
}

fn current_review_dispatch_id_if_still_current(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
) -> Result<Option<String>, JsonFailure> {
    let lineage_dispatch_id = current_review_dispatch_id_from_lineage(context, args)?;
    Ok(match args.scope {
        ReviewDispatchScopeArg::Task => lineage_dispatch_id,
        ReviewDispatchScopeArg::FinalReview => {
            let Some(dispatch_id) = lineage_dispatch_id else {
                return Ok(None);
            };
            let authoritative_state = load_authoritative_transition_state(context)?;
            let runtime_state = crate::execution::reducer::reduce_runtime_state(
                context,
                authoritative_state.as_ref(),
                semantic_workspace_snapshot(context)?,
            )?;
            final_review_dispatch_still_current_for_gates(
                runtime_state.gate_snapshot.gate_review.as_ref(),
                runtime_state.gate_snapshot.gate_finish.as_ref(),
            )
            .then_some(dispatch_id)
        }
    })
}

fn current_review_dispatch_id_from_lineage(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
) -> Result<Option<String>, JsonFailure> {
    let overlay = load_status_authoritative_overlay_checked(context)?;
    let Some(overlay) = overlay else {
        return Ok(None);
    };
    let current_task_semantic_reviewed_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    Ok(match args.scope {
        ReviewDispatchScopeArg::Task => shared_current_task_review_dispatch_id(
            args.task,
            args.task
                .and_then(|task| task_completion_lineage_fingerprint(context, task))
                .as_deref(),
            current_task_semantic_reviewed_state_id.as_deref(),
            None,
            Some(&overlay),
        ),
        ReviewDispatchScopeArg::FinalReview => {
            let usable_current_branch_closure_id = usable_current_branch_closure_identity(context)
                .map(|identity| identity.branch_closure_id);
            shared_current_final_review_dispatch_id(
                usable_current_branch_closure_id.as_deref(),
                Some(&overlay),
            )
        }
    })
}

pub(crate) fn ensure_current_review_dispatch_id(
    context: &ExecutionContext,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
    expected_dispatch_id: Option<&str>,
) -> Result<String, JsonFailure> {
    let args = RecordReviewDispatchArgs {
        plan: PathBuf::from(context.plan_rel.clone()),
        scope,
        task,
    };
    let cycle_target = review_dispatch_cycle_target(context);
    validate_review_dispatch_request(context, &args, cycle_target)?;
    if let Some(dispatch_id) = current_review_dispatch_id_if_still_current(context, &args)? {
        validate_expected_dispatch_id(&dispatch_id, expected_dispatch_id, scope, task)?;
        return Ok(dispatch_id);
    }
    if let Some(expected_dispatch_id) = expected_dispatch_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(expected_dispatch_id.to_owned());
    }
    ensure_review_dispatch_authoritative_bootstrap(context)?;
    let action = record_review_dispatch_strategy_checkpoint(context, &args, cycle_target)?;
    let refreshed = load_execution_context_for_exact_plan(&context.runtime, &args.plan)?;
    let dispatch_id = match action {
        ReviewDispatchMutationAction::Recorded => {
            current_review_dispatch_id_from_lineage(&refreshed, &args)?
        }
        ReviewDispatchMutationAction::AlreadyCurrent => {
            current_review_dispatch_id_if_still_current(&refreshed, &args)?
        }
    }
    .ok_or_else(|| {
        JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "review-dispatch lineage binding did not yield a current dispatch id.",
        )
    })?;
    validate_expected_dispatch_id(&dispatch_id, expected_dispatch_id, scope, task)?;
    Ok(dispatch_id)
}

pub(crate) fn current_review_dispatch_id_candidate(
    context: &ExecutionContext,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
    expected_dispatch_id: Option<&str>,
) -> Result<Option<String>, JsonFailure> {
    if let Some(expected_dispatch_id) = expected_dispatch_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(expected_dispatch_id.to_owned()));
    }
    let args = RecordReviewDispatchArgs {
        plan: PathBuf::from(context.plan_rel.clone()),
        scope,
        task,
    };
    let overlay_dispatch_id = current_review_dispatch_id_if_still_current(context, &args)?;
    if overlay_dispatch_id.is_some() {
        return Ok(overlay_dispatch_id);
    }
    if matches!(scope, ReviewDispatchScopeArg::Task)
        && let Some(task) = task
        && let Some(dispatch_id) = load_authoritative_transition_state(context)?
            .as_ref()
            .and_then(|state| state.task_review_dispatch_id(task))
    {
        return Ok(Some(dispatch_id));
    }
    Ok(None)
}

fn validate_expected_dispatch_id(
    actual_dispatch_id: &str,
    expected_dispatch_id: Option<&str>,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
) -> Result<(), JsonFailure> {
    let Some(expected_dispatch_id) = expected_dispatch_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    if actual_dispatch_id.trim() == expected_dispatch_id {
        return Ok(());
    }
    let detail = match scope {
        ReviewDispatchScopeArg::Task => format!(
            "close-current-task expected dispatch `{expected_dispatch_id}` for task {}.",
            task.unwrap_or_default()
        ),
        ReviewDispatchScopeArg::FinalReview => {
            format!("advance-late-stage expected final-review dispatch `{expected_dispatch_id}`.")
        }
    };
    Err(JsonFailure::new(
        FailureClass::InvalidCommandInput,
        format!("dispatch_id_mismatch: {detail}"),
    ))
}

fn recommendation_execution_context_key(context: &ExecutionContext) -> String {
    let base_branch = context
        .current_release_base_branch()
        .unwrap_or_else(|| String::from("unknown"));
    format!("{}@{}", context.runtime.branch_name, base_branch)
}

fn record_review_dispatch_strategy_checkpoint_without_claim(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
) -> Result<ReviewDispatchMutationAction, JsonFailure> {
    if current_review_dispatch_id_if_still_current(context, args)?.is_some() {
        return Ok(ReviewDispatchMutationAction::AlreadyCurrent);
    }
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "Authoritative harness state is required before record-review-dispatch can record review-dispatch proof.",
        ));
    };
    let cycle_target = match cycle_target {
        ReviewDispatchCycleTarget::Bound(_, _)
            if matches!(args.scope, ReviewDispatchScopeArg::FinalReview)
                && context.steps.iter().all(|step| step.checked) =>
        {
            None
        }
        ReviewDispatchCycleTarget::Bound(task, step) => Some((task, step)),
        ReviewDispatchCycleTarget::UnboundCompletedPlan => None,
        ReviewDispatchCycleTarget::None => return Ok(ReviewDispatchMutationAction::AlreadyCurrent),
    };
    authoritative_state.record_review_dispatch_strategy_checkpoint(
        context,
        &context.plan_document.execution_mode,
        cycle_target,
    )?;
    authoritative_state
        .persist_if_dirty_with_failpoint_and_command(None, "record_review_dispatch")?;
    Ok(ReviewDispatchMutationAction::Recorded)
}

fn record_review_dispatch_strategy_checkpoint(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
) -> Result<ReviewDispatchMutationAction, JsonFailure> {
    let _ = load_authoritative_transition_state(context)?;
    let _write_authority = claim_step_write_authority(&context.runtime)?;
    record_review_dispatch_strategy_checkpoint_without_claim(context, args, cycle_target)
}

fn ensure_review_dispatch_authoritative_bootstrap(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    if load_authoritative_transition_state(context)?
        .as_ref()
        .is_some_and(|state| state.execution_run_id_opt().is_some())
    {
        return Ok(());
    }
    let acceptance = persist_preflight_acceptance(context)?;
    ensure_preflight_authoritative_bootstrap_with_existing_authority(
        &context.runtime,
        RunIdentitySnapshot {
            execution_run_id: acceptance.execution_run_id.clone(),
            source_plan_path: context.plan_rel.clone(),
            source_plan_revision: context.plan_document.plan_revision,
        },
        acceptance.chunk_id,
    )
}

#[derive(Clone, Copy)]
enum ReviewDispatchCycleTarget {
    Bound(u32, u32),
    UnboundCompletedPlan,
    None,
}

fn validate_review_dispatch_request(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
) -> Result<(), JsonFailure> {
    match args.scope {
        ReviewDispatchScopeArg::Task => {
            let requested_task = args.task.ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "record-review-dispatch --scope task requires --task <n>.",
                )
            })?;
            let observed_task = match cycle_target {
                ReviewDispatchCycleTarget::Bound(task, _) => task,
                ReviewDispatchCycleTarget::UnboundCompletedPlan => {
                    return Err(JsonFailure::new(
                        FailureClass::InvalidCommandInput,
                        format!(
                            "record-review-dispatch --scope task --task {requested_task} is invalid because the approved plan is already at final-review dispatch scope."
                        ),
                    ));
                }
                ReviewDispatchCycleTarget::None => {
                    return Err(JsonFailure::new(
                        FailureClass::ExecutionStateNotReady,
                        "record-review-dispatch --scope task requires a current task review-dispatch target.",
                    ));
                }
            };
            if requested_task != observed_task {
                return Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    format!(
                        "record-review-dispatch --scope task --task {requested_task} does not match the current task review-dispatch target Task {observed_task} for plan {}.",
                        context.plan_rel
                    ),
                ));
            }
            Ok(())
        }
        ReviewDispatchScopeArg::FinalReview => {
            if args.task.is_some() {
                return Err(JsonFailure::new(
                    FailureClass::InvalidCommandInput,
                    "record-review-dispatch --scope final-review does not accept --task.",
                ));
            }
            match cycle_target {
                ReviewDispatchCycleTarget::UnboundCompletedPlan => Ok(()),
                ReviewDispatchCycleTarget::Bound(_, _)
                    if context_all_task_scopes_closed_by_authority(context, None) =>
                {
                    Ok(())
                }
                ReviewDispatchCycleTarget::Bound(_, _) | ReviewDispatchCycleTarget::None => {
                    Err(JsonFailure::new(
                        FailureClass::ExecutionStateNotReady,
                        "record-review-dispatch --scope final-review requires a completed-plan final-review dispatch target.",
                    ))
                }
            }
        }
    }
}

fn review_dispatch_cycle_target(context: &ExecutionContext) -> ReviewDispatchCycleTarget {
    if let Some(boundary_target) = review_dispatch_task_boundary_target(context) {
        return boundary_target;
    }
    for state in [
        NoteState::Active,
        NoteState::Blocked,
        NoteState::Interrupted,
    ] {
        if let Some(step) = active_step(context, state) {
            return ReviewDispatchCycleTarget::Bound(step.task_number, step.step_number);
        }
    }
    if context_all_task_scopes_closed_by_authority(context, None) {
        let overlay = load_status_authoritative_overlay_checked(context)
            .ok()
            .and_then(|overlay| overlay);
        let authoritative_phase = overlay.as_ref().and_then(|overlay| {
            normalize_optional_overlay_value(overlay.harness_phase.as_deref())
                .and_then(parse_harness_phase)
        });
        if authoritative_phase.is_some_and(is_late_stage_phase)
            || overlay
                .as_ref()
                .is_some_and(has_authoritative_late_stage_progress)
        {
            return ReviewDispatchCycleTarget::UnboundCompletedPlan;
        }
        if let Some(final_task) = context.tasks_by_number.keys().copied().max() {
            let final_task_closure_missing = load_authoritative_transition_state(context)
                .ok()
                .and_then(|state| state)
                .and_then(|state| {
                    (!state.current_task_closure_overlay_needs_restore()).then_some(state)
                })
                .and_then(|state| state.raw_current_task_closure_result(final_task))
                .is_none();
            if final_task_closure_missing
                && let Some(final_step) = context
                    .steps
                    .iter()
                    .filter(|step| step.task_number == final_task)
                    .map(|step| step.step_number)
                    .max()
            {
                return ReviewDispatchCycleTarget::Bound(final_task, final_step);
            }
        }
        return ReviewDispatchCycleTarget::UnboundCompletedPlan;
    }
    if let Some(attempt) = context.evidence.attempts.iter().rev().find(|attempt| {
        context.steps.iter().any(|step| {
            step.task_number == attempt.task_number && step.step_number == attempt.step_number
        })
    }) {
        return ReviewDispatchCycleTarget::Bound(attempt.task_number, attempt.step_number);
    }
    if let Some(step) = context.steps.iter().rev().find(|step| step.checked) {
        return ReviewDispatchCycleTarget::Bound(step.task_number, step.step_number);
    }
    if let Some(step) = context
        .steps
        .iter()
        .find(|step| step.note_state.is_some() && !step.checked)
    {
        return ReviewDispatchCycleTarget::Bound(step.task_number, step.step_number);
    }
    if !context.evidence.attempts.is_empty()
        && let Some(attempt) = context.evidence.attempts.last()
    {
        return ReviewDispatchCycleTarget::Bound(attempt.task_number, attempt.step_number);
    }
    ReviewDispatchCycleTarget::None
}

fn context_all_task_scopes_closed_by_authority(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    let loaded_authoritative_state;
    let authoritative_state = if authoritative_state.is_some() {
        authoritative_state
    } else {
        loaded_authoritative_state = load_authoritative_transition_state_relaxed(context)
            .ok()
            .flatten();
        loaded_authoritative_state.as_ref()
    };
    if let Some(authoritative_state) = authoritative_state {
        let closed_tasks = authoritative_state
            .current_task_closure_results()
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        if !closed_tasks.is_empty() {
            return context
                .tasks_by_number
                .keys()
                .all(|task| closed_tasks.contains(task));
        }
    }
    context.steps.iter().all(|step| step.checked)
}

fn review_dispatch_task_boundary_target(
    context: &ExecutionContext,
) -> Option<ReviewDispatchCycleTarget> {
    let status = status_from_context(context).ok();
    let earliest_stale_boundary_task = status
        .as_ref()
        .and_then(|status| pre_reducer_earliest_unresolved_stale_task(context, status));
    if let Some(stale_task) = earliest_stale_boundary_task
        .filter(|task_number| review_dispatch_boundary_blocked_for_task(context, *task_number))
    {
        let step_number = latest_attempted_step_for_task(context, stale_task).or_else(|| {
            context
                .steps
                .iter()
                .filter(|step| step.task_number == stale_task)
                .map(|step| step.step_number)
                .max()
        })?;
        return Some(ReviewDispatchCycleTarget::Bound(stale_task, step_number));
    }
    if let Some(status) = status.as_ref() {
        let boundary_reason_present = status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_current_closure_missing"
                    | "prior_task_current_closure_stale"
                    | "prior_task_current_closure_invalid"
                    | "prior_task_current_closure_reviewed_state_malformed"
                    | "task_cycle_break_active"
            )
        });
        if boundary_reason_present
            && let Some(task_number) = status
                .blocking_task
                .or(status.resume_task)
                .or(status.active_task)
            && review_dispatch_boundary_blocked_for_task(context, task_number)
        {
            let step_number =
                latest_attempted_step_for_task(context, task_number).or_else(|| {
                    context
                        .steps
                        .iter()
                        .filter(|step| step.task_number == task_number)
                        .map(|step| step.step_number)
                        .max()
                })?;
            return Some(ReviewDispatchCycleTarget::Bound(task_number, step_number));
        }
    }
    let task_number = status.as_ref().and_then(|status| {
        context
            .tasks_by_number
            .keys()
            .copied()
            .filter(|candidate_task| {
                review_dispatch_boundary_blocked_for_task(context, *candidate_task)
            })
            .find(|candidate_task| {
                task_closure_baseline_repair_candidate_with_stale_target(
                    context,
                    status,
                    *candidate_task,
                    pre_reducer_earliest_unresolved_stale_task(context, status),
                )
                .ok()
                .flatten()
                .is_some()
            })
    })?;
    let step_number = latest_attempted_step_for_task(context, task_number).or_else(|| {
        context
            .steps
            .iter()
            .filter(|step| step.task_number == task_number)
            .map(|step| step.step_number)
            .max()
    })?;
    Some(ReviewDispatchCycleTarget::Bound(task_number, step_number))
}

fn review_dispatch_boundary_blocked_for_task(context: &ExecutionContext, task_number: u32) -> bool {
    task_closure_recording_prerequisites(context, task_number)
        .ok()
        .is_some_and(|prerequisites| {
            prerequisites
                .dispatch_id
                .as_deref()
                .is_none_or(|dispatch_id| dispatch_id.trim().is_empty())
                || prerequisites
                    .blocking_reason_codes
                    .iter()
                    .any(|reason_code| {
                        matches!(
                            reason_code.as_str(),
                            "prior_task_review_dispatch_missing"
                                | "prior_task_review_dispatch_stale"
                        )
                    })
        })
}

fn review_dispatch_gate_from_context(
    context: &ExecutionContext,
    args: &RecordReviewDispatchArgs,
    cycle_target: ReviewDispatchCycleTarget,
) -> GateResult {
    match args.scope {
        ReviewDispatchScopeArg::Task => {
            let task_number = args.task.or(match cycle_target {
                ReviewDispatchCycleTarget::Bound(task_number, _) => Some(task_number),
                _ => None,
            });
            if let Some(task_number) = task_number {
                return task_review_dispatch_gate_from_context(context, task_number);
            }
        }
        ReviewDispatchScopeArg::FinalReview => {
            return final_review_dispatch_gate_from_context(context);
        }
    }
    gate_review_from_context_internal(context, false)
}

fn final_review_dispatch_gate_from_context(context: &ExecutionContext) -> GateResult {
    let mut gate = GateState::from_result(gate_review_base_result(context, false));
    if !gate.allowed {
        return gate.finish();
    }

    let authoritative_state = match load_authoritative_transition_state(context) {
        Ok(state) => state,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_state_unreadable",
                error.message,
                "Restore authoritative harness state readability and retry final-review dispatch.",
            );
            return gate.finish();
        }
    };
    let overlay = match load_status_authoritative_overlay_checked(context) {
        Ok(overlay) => overlay,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_overlay_unreadable",
                error.message,
                "Restore authoritative overlay readability and retry final-review dispatch.",
            );
            return gate.finish();
        }
    };
    let missing_derived_overlays =
        missing_derived_review_state_fields(authoritative_state.as_ref(), overlay.as_ref());
    if missing_derived_overlays.iter().any(|field| {
        matches!(
            field.as_str(),
            "current_branch_closure_id"
                | "current_branch_closure_reviewed_state_id"
                | "current_branch_closure_contract_identity"
        )
    }) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "derived_review_state_missing",
            "Final-review dispatch is blocked because current branch-closure bindings require review-state repair before late-stage progression can continue.",
            format!(
                "Run `featureforge plan execution repair-review-state --plan {}` before dispatching final review.",
                context.plan_rel
            ),
        );
        return gate.finish();
    }
    let Some(current_branch_closure_id) = validated_current_branch_closure_identity(context)
        .map(|identity| identity.branch_closure_id)
    else {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "branch_closure_recording_required_for_release_readiness",
            "Final-review dispatch is blocked because no current reviewed branch closure exists.",
            format!(
                "Run `featureforge plan execution advance-late-stage --plan {}` before dispatching final review.",
                context.plan_rel
            ),
        );
        return gate.finish();
    };
    if current_branch_reviewed_state_id(context).is_none() {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "current_branch_reviewed_state_id_missing",
            "Final-review dispatch is blocked because the current branch-closure reviewed state requires repair before late-stage progression can continue.",
            format!(
                "Run `featureforge plan execution repair-review-state --plan {}` before dispatching final review.",
                context.plan_rel
            ),
        );
        return gate.finish();
    }

    let release_readiness_result = authoritative_state
        .as_ref()
        .and_then(|state| {
            state
                .current_release_readiness_record_id()
                .as_deref()
                .and_then(|record_id| state.release_readiness_record_by_id(record_id))
        })
        .and_then(|record| {
            (record.branch_closure_id == current_branch_closure_id).then_some(record.result)
        });
    if release_readiness_result.as_deref() == Some("blocked") {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "release_blocker_resolution_required",
            "Final-review dispatch is blocked because the current branch closure still has a blocked release-readiness result.",
            format!(
                "Run `featureforge plan execution advance-late-stage --plan {} --result ready|blocked --summary-file <path>` after resolving the release blocker.",
                context.plan_rel
            ),
        );
        return gate.finish();
    }
    if release_readiness_result.as_deref() != Some("ready") {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "release_readiness_recording_ready",
            "Final-review dispatch is blocked because the current branch closure does not yet have a current release-readiness result `ready`.",
            format!(
                "Run `featureforge plan execution advance-late-stage --plan {} --result ready|blocked --summary-file <path>` before dispatching final review.",
                context.plan_rel
            ),
        );
    }
    gate.finish()
}

fn task_review_dispatch_gate_from_context(
    context: &ExecutionContext,
    task_number: u32,
) -> GateResult {
    let mut gate = GateState::default();
    let task_steps: Vec<_> = context
        .steps
        .iter()
        .filter(|step| step.task_number == task_number)
        .collect();
    if task_steps.is_empty() {
        gate.fail(
            FailureClass::InvalidCommandInput,
            "task_not_found",
            format!(
                "Task {task_number} does not exist in the approved plan and cannot be used for record-review-dispatch."
            ),
            "Choose a valid task number from the approved plan.",
        );
        return gate.finish();
    }

    if current_task_closure_overlay_restore_required(context).unwrap_or(false) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "current_task_closure_overlay_restore_required",
            format!(
                "Task {task_number} review dispatch is blocked because current task-closure overlays are missing and must be repaired before recording more review-dispatch lineage for this task."
            ),
            format!(
                "Run `featureforge plan execution repair-review-state --plan {}` before recording more review-dispatch lineage for Task {task_number}.",
                context.plan_rel
            ),
        );
        return gate.finish();
    }

    for state in [
        NoteState::Active,
        NoteState::Blocked,
        NoteState::Interrupted,
    ] {
        if let Some(step) =
            active_step(context, state).filter(|step| step.task_number == task_number)
        {
            let (reason_code, message, remediation) = match state {
                NoteState::Active => (
                    "active_step_in_progress",
                    format!(
                        "Task {task_number} review dispatch is blocked while Step {} remains active.",
                        step.step_number
                    ),
                    "Complete, interrupt, or resolve the active step before dispatching task review.",
                ),
                NoteState::Blocked => (
                    "blocked_step",
                    format!(
                        "Task {task_number} review dispatch is blocked while Step {} remains blocked.",
                        step.step_number
                    ),
                    "Resolve the blocked step before dispatching task review.",
                ),
                NoteState::Interrupted => (
                    "interrupted_work_unresolved",
                    format!(
                        "Task {task_number} review dispatch is blocked while Step {} remains interrupted.",
                        step.step_number
                    ),
                    "Resume or explicitly resolve the interrupted step before dispatching task review.",
                ),
            };
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                reason_code,
                message,
                remediation,
            );
        }
    }

    for step in task_steps {
        if !step.checked {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                "unfinished_task_steps_remaining",
                format!(
                    "Task {task_number} review dispatch is blocked while Step {} remains unchecked.",
                    step.step_number
                ),
                "Finish all steps in the task before dispatching task review.",
            );
            continue;
        }
        let Some(attempt) =
            latest_attempt_for_step(&context.evidence, step.task_number, step.step_number)
        else {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "checked_step_missing_evidence",
                format!(
                    "Task {task_number} Step {} is checked but missing execution evidence.",
                    step.step_number
                ),
                "Reopen the step or record matching execution evidence before dispatching task review.",
            );
            continue;
        };
        if attempt.status != "Completed" {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "checked_step_missing_evidence",
                format!(
                    "Task {task_number} Step {} no longer has a completed evidence attempt.",
                    step.step_number
                ),
                "Reopen the step or complete it again with fresh evidence before dispatching task review.",
            );
        }
    }

    match task_current_closure_status(context, task_number) {
        Ok(TaskCurrentClosureStatus::Current) => {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                "task_current_closure_already_current",
                format!(
                    "Task {task_number} review dispatch is out of phase because Task {task_number} already has a current passing task closure for the active approved plan."
                ),
                "Re-derive the workflow phase before recording more review-dispatch lineage for this task.",
            );
        }
        Ok(TaskCurrentClosureStatus::Missing) => {}
        Ok(TaskCurrentClosureStatus::Stale) => {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                "prior_task_current_closure_stale",
                format!(
                    "Task {task_number} review dispatch is blocked because Task {task_number} current task closure no longer matches the current reviewed workspace state."
                ),
                format!(
                    "Run `featureforge plan execution repair-review-state --plan {}` before recording fresh review-dispatch lineage for Task {task_number}.",
                    context.plan_rel
                ),
            );
        }
        Err(error) => {
            let failure_class =
                if error.error_class == FailureClass::MalformedExecutionState.as_str() {
                    FailureClass::MalformedExecutionState
                } else {
                    FailureClass::ExecutionStateNotReady
                };
            let reason_code = task_boundary_reason_code_from_message(&error.message)
                .unwrap_or("task_current_closure_state_invalid");
            gate.fail(
                failure_class,
                reason_code,
                format!(
                    "Task {task_number} review dispatch is blocked because the current task-closure state is not trustworthy: {}",
                    error.message
                ),
                "Repair the current task-closure state before recording more review-dispatch lineage for this task.",
            );
        }
    }

    gate.finish()
}

fn select_active_learned_topology_guidance(
    records: &[ExecutionTopologyDowngradeRecord],
    plan_revision: u32,
    execution_context_key: &str,
) -> Option<LearnedTopologyGuidance> {
    records
        .iter()
        .rev()
        .find(|record| {
            record.source_plan_revision == plan_revision
                && record.execution_context_key == execution_context_key
                && !record.rerun_guidance_superseded
        })
        .map(|record| LearnedTopologyGuidance {
            approved_plan_revision: plan_revision,
            execution_context_key: record.execution_context_key.clone(),
            primary_reason_class: record.primary_reason_class.as_str().to_owned(),
        })
}

pub fn write_plan_execution_schema(output_dir: &Path) -> Result<(), JsonFailure> {
    fs::create_dir_all(output_dir).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!(
                "Could not create schema directory {}: {error}",
                output_dir.display()
            ),
        )
    })?;
    let schema = schema_for!(PlanExecutionStatus);
    let mut schema_json = serde_json::to_value(&schema).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Could not serialize plan execution schema value: {error}"),
        )
    })?;
    if let Some(required) = schema_json
        .get_mut("required")
        .and_then(serde_json::Value::as_array_mut)
    {
        required.retain(|field| {
            !matches!(
                field.as_str(),
                Some("recording_context" | "execution_command_context")
            )
        });
    }
    tighten_plan_execution_public_context_schemas(&mut schema_json)?;
    tighten_plan_execution_routing_field_schemas(&mut schema_json)?;
    tighten_plan_execution_phase_bound_recording_context_contracts(&mut schema_json)?;
    let payload = serde_json::to_string_pretty(&schema_json).map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Could not serialize plan execution schema: {error}"),
        )
    })?;
    fs::write(
        output_dir.join("plan-execution-status.schema.json"),
        payload,
    )
    .map_err(|error| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            format!("Could not write plan execution schema: {error}"),
        )
    })?;
    Ok(())
}

fn tighten_plan_execution_public_context_schemas(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    let defs = schema_json
        .get_mut("$defs")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `$defs`.",
            )
        })?;
    let execution_context = defs
        .get_mut("PublicExecutionCommandContext")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `PublicExecutionCommandContext`.",
            )
        })?;
    tighten_public_execution_command_context_schema(execution_context)?;
    let recording_context = defs
        .get_mut("PublicRecordingContext")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing `PublicRecordingContext`.",
            )
        })?;
    tighten_public_recording_context_schema(recording_context)?;
    Ok(())
}

fn tighten_plan_execution_routing_field_schemas(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    let properties = schema_json
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema is missing top-level `properties`.",
            )
        })?;
    tighten_schema_property_type(properties, "recommended_command", "string")?;
    Ok(())
}

fn tighten_plan_execution_phase_bound_recording_context_contracts(
    schema_json: &mut serde_json::Value,
) -> Result<(), JsonFailure> {
    append_phase_bound_recording_context_requirements(
        schema_json,
        "task_closure_recording_ready",
        &["task_number"],
    )?;
    append_phase_bound_recording_context_requirements(
        schema_json,
        "release_readiness_recording_ready",
        &["branch_closure_id"],
    )?;
    append_phase_bound_recording_context_requirements(
        schema_json,
        "release_blocker_resolution_required",
        &["branch_closure_id"],
    )?;
    append_phase_bound_recording_context_requirements(
        schema_json,
        "final_review_recording_ready",
        &["branch_closure_id"],
    )?;
    append_phase_detail_field_forbidden_outside_allowed_phase_details(
        schema_json,
        "recording_context",
        &[
            "task_closure_recording_ready",
            "release_readiness_recording_ready",
            "release_blocker_resolution_required",
            "final_review_recording_ready",
        ],
    )?;
    append_phase_field_forbidden_outside_const_phase(
        schema_json,
        "harness_phase",
        "executing",
        "execution_command_context",
    )?;
    append_phase_detail_field_omitted_only_in_lanes(
        schema_json,
        "recommended_command",
        RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS,
    )?;
    Ok(())
}

fn tighten_public_execution_command_context_schema(
    schema: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFailure> {
    let properties = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Execution command context schema is missing `properties`.",
            )
        })?;
    tighten_schema_property_type(properties, "task_number", "integer")?;
    tighten_schema_property_type(properties, "step_id", "integer")?;
    schema.insert(
        String::from("required"),
        serde_json::json!(["command_kind", "task_number", "step_id"]),
    );
    schema.insert(
        String::from("additionalProperties"),
        serde_json::Value::Bool(false),
    );
    Ok(())
}

fn tighten_public_recording_context_schema(
    schema: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFailure> {
    let properties = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Recording context schema is missing `properties`.",
            )
        })?;
    tighten_schema_property_type(properties, "branch_closure_id", "string")?;
    tighten_schema_property_type(properties, "dispatch_id", "string")?;
    tighten_schema_property_type(properties, "task_number", "integer")?;
    schema.insert(
        String::from("additionalProperties"),
        serde_json::Value::Bool(false),
    );
    schema.insert(String::from("minProperties"), serde_json::Value::from(1));
    schema.insert(
        String::from("anyOf"),
        serde_json::json!([
            {"required": ["branch_closure_id"]},
            {"required": ["task_number"]}
        ]),
    );
    Ok(())
}

fn tighten_schema_property_type(
    properties: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
    expected_type: &str,
) -> Result<(), JsonFailure> {
    let property = properties
        .get_mut(field)
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                format!("Schema is missing property `{field}`."),
            )
        })?;
    property.insert(
        String::from("type"),
        serde_json::Value::String(String::from(expected_type)),
    );
    Ok(())
}

fn append_phase_bound_recording_context_requirements(
    schema_json: &mut serde_json::Value,
    phase_detail: &str,
    required_fields: &[&str],
) -> Result<(), JsonFailure> {
    let root = schema_json.as_object_mut().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            "Plan execution schema root is not an object.",
        )
    })?;
    let all_of = root
        .entry(String::from("allOf"))
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema `allOf` is not an array.",
            )
        })?;
    all_of.push(serde_json::json!({
        "if": {
            "properties": {
                "phase_detail": { "const": phase_detail }
            }
        },
        "then": {
            "required": ["recording_context"],
            "properties": {
                "recording_context": {
                    "required": required_fields
                }
            }
        }
    }));
    Ok(())
}

fn append_phase_detail_field_forbidden_outside_allowed_phase_details(
    schema_json: &mut serde_json::Value,
    field: &str,
    allowed_phase_details: &[&str],
) -> Result<(), JsonFailure> {
    let root = schema_json.as_object_mut().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            "Plan execution schema root is not an object.",
        )
    })?;
    let all_of = root
        .entry(String::from("allOf"))
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema `allOf` is not an array.",
            )
        })?;
    all_of.push(serde_json::json!({
        "if": {
            "properties": {
                "phase_detail": { "enum": allowed_phase_details }
            }
        },
        "else": {
            "not": {
                "required": [field]
            }
        }
    }));
    Ok(())
}

fn append_phase_field_forbidden_outside_const_phase(
    schema_json: &mut serde_json::Value,
    phase_field: &str,
    phase_value: &str,
    field: &str,
) -> Result<(), JsonFailure> {
    let root = schema_json.as_object_mut().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            "Plan execution schema root is not an object.",
        )
    })?;
    let all_of = root
        .entry(String::from("allOf"))
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema `allOf` is not an array.",
            )
        })?;
    all_of.push(serde_json::json!({
        "if": {
            "properties": {
                (phase_field): { "const": phase_value }
            }
        },
        "else": {
            "not": {
                "required": [field]
            }
        }
    }));
    Ok(())
}

fn append_phase_detail_field_omitted_only_in_lanes(
    schema_json: &mut serde_json::Value,
    field: &str,
    omission_phase_details: &[&str],
) -> Result<(), JsonFailure> {
    let root = schema_json.as_object_mut().ok_or_else(|| {
        JsonFailure::new(
            FailureClass::EvidenceWriteFailed,
            "Plan execution schema root is not an object.",
        )
    })?;
    let all_of = root
        .entry(String::from("allOf"))
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::EvidenceWriteFailed,
                "Plan execution schema `allOf` is not an array.",
            )
        })?;
    all_of.push(serde_json::json!({
        "if": {
            "properties": {
                "phase_detail": { "enum": omission_phase_details }
            }
        },
        "then": {
            "not": {
                "required": [field]
            }
        },
        "else": {
            "required": [field]
        }
    }));
    Ok(())
}

pub fn load_execution_context(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
) -> Result<ExecutionContext, JsonFailure> {
    load_execution_context_with_policies(
        runtime,
        plan_path,
        LegacyEvidencePolicy::Reject,
        TrackedEvidenceProjectionPolicy::Ignore,
        ApprovedArtifactSelectionPolicy::RequireUnique,
        true,
    )
}

pub fn load_execution_context_for_mutation(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
) -> Result<ExecutionContext, JsonFailure> {
    load_execution_context_with_policies(
        runtime,
        plan_path,
        LegacyEvidencePolicy::Allow,
        TrackedEvidenceProjectionPolicy::Ignore,
        ApprovedArtifactSelectionPolicy::AllowExactPlan,
        true,
    )
}

pub(crate) fn load_execution_context_for_rebuild(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
) -> Result<ExecutionContext, JsonFailure> {
    load_execution_context_with_policies(
        runtime,
        plan_path,
        LegacyEvidencePolicy::Allow,
        TrackedEvidenceProjectionPolicy::AllowExplicitImport,
        ApprovedArtifactSelectionPolicy::AllowExactPlan,
        true,
    )
}

pub(crate) fn load_execution_context_for_exact_plan(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
) -> Result<ExecutionContext, JsonFailure> {
    load_execution_context_with_policies(
        runtime,
        plan_path,
        LegacyEvidencePolicy::Reject,
        TrackedEvidenceProjectionPolicy::Ignore,
        ApprovedArtifactSelectionPolicy::AllowExactPlan,
        true,
    )
}

pub(crate) fn load_execution_context_without_authority_overlay(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
) -> Result<ExecutionContext, JsonFailure> {
    load_execution_context_with_policies(
        runtime,
        plan_path,
        LegacyEvidencePolicy::Reject,
        TrackedEvidenceProjectionPolicy::Ignore,
        ApprovedArtifactSelectionPolicy::AllowExactPlan,
        false,
    )
}

pub(crate) fn load_execution_read_scope(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
    exact_plan_override: bool,
) -> Result<ExecutionReadScope, JsonFailure> {
    let context = load_execution_read_context(runtime, plan_path, exact_plan_override)?;
    finalize_execution_read_scope(runtime, exact_plan_override, context)
}

pub(crate) fn load_execution_read_scope_for_mutation(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
    exact_plan_override: bool,
) -> Result<ExecutionReadScope, JsonFailure> {
    let context = load_execution_context_for_mutation(runtime, plan_path)?;
    finalize_execution_read_scope(runtime, exact_plan_override, context)
}

fn finalize_execution_read_scope(
    runtime: &ExecutionRuntime,
    exact_plan_override: bool,
    mut context: ExecutionContext,
) -> Result<ExecutionReadScope, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state_relaxed(&context)?;
    overlay_execution_evidence_attempts_from_authority(&mut context, authoritative_state.as_ref())?;
    overlay_step_state_from_authority(&mut context, authoritative_state.as_ref())?;
    refresh_execution_fingerprint(&mut context);
    let derived = derive_execution_truth_from_authority(&context, authoritative_state.as_ref())?;
    let overlay = derived.overlay;
    let mut status = derived.status;
    let local_contract_plan_fingerprint = hash_contract_plan(&context.plan_source);
    let local_evidence_progress_present = context.evidence.tracked_progress_present;
    let local_projection_only_execution_started =
        status.execution_started == "yes" && !context.local_execution_progress_markers_present;
    let local_has_other_same_branch_worktree = has_other_same_branch_worktree(runtime);
    let local_started_execution = status.execution_started == "yes";
    let local_probe = LocalSameBranchReadScopeProbe {
        plan_rel: &context.plan_rel,
        contract_plan_fingerprint: &local_contract_plan_fingerprint,
        evidence_progress_present: local_evidence_progress_present,
        projection_only_execution_started: local_projection_only_execution_started,
        started_execution: local_started_execution,
        semantic_workspace_state_id: &status_workspace_state_id(&context)?,
    };
    let read_scope = if let Some(adopted_scope) =
        started_execution_read_scope_from_same_branch_worktree(
            runtime,
            local_probe,
            exact_plan_override,
        )? {
        adopted_scope
    } else {
        if local_started_execution
            && local_projection_only_execution_started
            && local_has_other_same_branch_worktree
        {
            clear_projection_only_execution_progress(&mut context);
            refresh_execution_fingerprint(&mut context);
            status = derive_execution_truth_from_authority(&context, None)?.status;
            normalize_non_started_same_branch_status(&mut status);
            return Ok(ExecutionReadScope {
                context,
                status,
                overlay: None,
                authoritative_state: None,
                runtime_state: None,
            });
        }
        if local_has_other_same_branch_worktree {
            normalize_non_started_same_branch_status(&mut status);
        }
        ExecutionReadScope {
            context,
            status,
            overlay,
            authoritative_state,
            runtime_state: None,
        }
    };
    Ok(read_scope)
}

fn normalize_non_started_same_branch_status(status: &mut PlanExecutionStatus) {
    if status.execution_started == "yes" && status.phase_detail == "execution_in_progress" {
        status.execution_started = String::from("no");
        status.active_task = None;
        status.active_step = None;
        status.resume_task = None;
        status.resume_step = None;
    } else if status.execution_started != "no"
        || status.phase_detail != "execution_reentry_required"
    {
        return;
    }
    status.phase = Some(String::from("execution_preflight"));
    status.phase_detail = String::from("execution_preflight_required");
    status.next_action = String::from("execution preflight");
    status.recommended_command = None;
    status.recording_context = None;
    status.execution_command_context = None;
    status.blocking_scope = None;
    status.blocking_task = None;
    status.blocking_reason_codes.clear();
}

pub(crate) fn status_overlay_from_authoritative_snapshot(
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
        "handoff_required",
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

fn load_execution_read_context(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
    exact_plan_override: bool,
) -> Result<ExecutionContext, JsonFailure> {
    if exact_plan_override {
        load_execution_context_for_exact_plan(runtime, plan_path)
    } else {
        load_execution_context(runtime, plan_path)
    }
}

struct LocalSameBranchReadScopeProbe<'a> {
    plan_rel: &'a str,
    contract_plan_fingerprint: &'a str,
    evidence_progress_present: bool,
    projection_only_execution_started: bool,
    started_execution: bool,
    semantic_workspace_state_id: &'a str,
}

fn started_execution_read_scope_from_same_branch_worktree(
    current_runtime: &ExecutionRuntime,
    local_probe: LocalSameBranchReadScopeProbe<'_>,
    exact_plan_override: bool,
) -> Result<Option<ExecutionReadScope>, JsonFailure> {
    if local_probe.started_execution && !local_probe.projection_only_execution_started {
        return Ok(None);
    }
    if local_probe.evidence_progress_present {
        return Ok(None);
    }
    let relative_plan = Path::new(local_probe.plan_rel);
    Ok(same_branch_worktrees(&current_runtime.repo_root)
        .into_iter()
        .filter(|root| root != &current_runtime.repo_root)
        .find_map(|worktree_root| {
            let discovered_runtime = ExecutionRuntime::discover(&worktree_root).ok()?;
            if current_runtime.branch_name == "current"
                || discovered_runtime.branch_name == "current"
                || discovered_runtime.branch_name != current_runtime.branch_name
            {
                return None;
            }
            let runtime = ExecutionRuntime {
                state_dir: current_runtime.state_dir.clone(),
                ..discovered_runtime
            };
            let mut context =
                load_execution_read_context(&runtime, relative_plan, exact_plan_override).ok()?;
            if hash_contract_plan(&context.plan_source) != local_probe.contract_plan_fingerprint {
                return None;
            }
            let authoritative_state = load_authoritative_transition_state_relaxed(&context).ok()?;
            overlay_step_state_from_authority(&mut context, authoritative_state.as_ref()).ok()?;
            let derived =
                derive_execution_truth_from_authority(&context, authoritative_state.as_ref())
                    .ok()?;
            let semantic_workspace_state_id = status_workspace_state_id(&context).ok()?;
            (derived.status.execution_started == "yes"
                && semantic_workspace_state_id == local_probe.semantic_workspace_state_id)
                .then_some(ExecutionReadScope {
                    context,
                    status: derived.status,
                    overlay: derived.overlay,
                    authoritative_state,
                    runtime_state: None,
                })
        }))
}

fn has_other_same_branch_worktree(current_runtime: &ExecutionRuntime) -> bool {
    if current_runtime.branch_name == "current" {
        return false;
    }
    same_branch_worktrees(&current_runtime.repo_root)
        .into_iter()
        .filter(|root| root != &current_runtime.repo_root)
        .filter_map(|root| ExecutionRuntime::discover(&root).ok())
        .any(|runtime| {
            runtime.branch_name != "current" && runtime.branch_name == current_runtime.branch_name
        })
}

fn clear_open_step_projections_for_steps(steps: &mut [PlanStepState]) {
    for step in steps {
        step.note_state = None;
        step.note_summary.clear();
    }
}

fn clear_open_step_projections(context: &mut ExecutionContext) {
    clear_open_step_projections_for_steps(&mut context.steps);
}

fn clear_projection_only_execution_progress(context: &mut ExecutionContext) {
    clear_open_step_projections(context);
    if matches!(
        context.evidence.source_origin,
        EvidenceSourceOrigin::StateDirProjection | EvidenceSourceOrigin::AuthoritativeState
    ) {
        let tracked_progress_present = context.evidence.tracked_progress_present;
        let tracked_source = context.evidence.tracked_source.clone();
        let source_origin = if tracked_source.is_some() {
            EvidenceSourceOrigin::TrackedFile
        } else {
            EvidenceSourceOrigin::Empty
        };
        context.evidence = ExecutionEvidence {
            format: EvidenceFormat::Empty,
            plan_path: context.plan_rel.clone(),
            plan_revision: context.plan_document.plan_revision,
            plan_fingerprint: None,
            source_spec_path: context.plan_document.source_spec_path.clone(),
            source_spec_revision: context.plan_document.source_spec_revision,
            source_spec_fingerprint: None,
            attempts: Vec::new(),
            source: tracked_source.clone(),
            source_origin,
            tracked_progress_present,
            tracked_source,
        };
        context.authoritative_evidence_projection_fingerprint = None;
        if !context.local_execution_progress_markers_present {
            context.plan_document.execution_mode = String::from("none");
        }
    }
}

fn infer_execution_mode_from_evidence(
    plan_document: &mut PlanDocument,
    evidence: &ExecutionEvidence,
) {
    if plan_document.execution_mode != "none" {
        return;
    }
    if let Some(execution_source) = evidence
        .attempts
        .iter()
        .rev()
        .map(|attempt| attempt.execution_source.as_str())
        .find(|source| {
            matches!(
                *source,
                "featureforge:executing-plans" | "featureforge:subagent-driven-development"
            )
        })
    {
        plan_document.execution_mode = execution_source.to_owned();
    }
}

fn overlay_step_state_from_authority(
    context: &mut ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<(), JsonFailure> {
    let Some(authoritative_state) = authoritative_state else {
        return Ok(());
    };
    let state_payload = authoritative_state.state_payload_snapshot();
    overlay_task_closure_completed_steps(context, &state_payload)?;
    let Some(completed_steps) = state_payload
        .get("event_completed_steps")
        .and_then(serde_json::Value::as_object)
    else {
        overlay_authoritative_open_step_state_from_state(context, authoritative_state)?;
        return Ok(());
    };
    overlay_event_completed_steps(context, completed_steps)?;
    overlay_authoritative_open_step_state_from_state(context, authoritative_state)
}

fn overlay_execution_evidence_attempts_from_authority(
    context: &mut ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<(), JsonFailure> {
    let Some(authoritative_state) = authoritative_state else {
        return Ok(());
    };
    let state_payload = authoritative_state.state_payload_snapshot();
    context.authoritative_evidence_projection_fingerprint = state_payload
        .get("execution_evidence_projection_fingerprint")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let Some(attempts_value) = state_payload
        .get("execution_evidence_attempts")
        .filter(|value| !value.is_null())
        .cloned()
    else {
        if context.evidence.attempts.is_empty() {
            synthesize_legacy_authoritative_evidence_attempts(context, &state_payload)?;
        }
        return Ok(());
    };
    let attempts =
        serde_json::from_value::<Vec<EvidenceAttempt>>(attempts_value).map_err(|error| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!("Authoritative execution evidence attempts are malformed: {error}"),
            )
        })?;
    context.evidence.format = if attempts.is_empty() {
        EvidenceFormat::Empty
    } else {
        EvidenceFormat::V2
    };
    context.evidence.plan_fingerprint = Some(hash_contract_plan(&context.plan_source));
    context.evidence.source_spec_fingerprint =
        Some(sha256_hex(context.source_spec_source.as_bytes()));
    context.evidence.attempts = attempts;
    context.evidence.source = Some(render_canonical_evidence_projection_source(
        &context.plan_rel,
        &context.plan_document,
        &context.plan_source,
        &context.source_spec_source,
        &context.steps,
        &context.evidence,
    ));
    context.evidence.source_origin = EvidenceSourceOrigin::AuthoritativeState;
    Ok(())
}

fn synthesize_legacy_authoritative_evidence_attempts(
    context: &mut ExecutionContext,
    state_payload: &serde_json::Value,
) -> Result<(), JsonFailure> {
    let completed_steps = authoritative_completed_steps_for_evidence(context, state_payload)?;
    if completed_steps.is_empty() {
        return Ok(());
    }
    let execution_source = if context.plan_document.execution_mode == "none" {
        context.plan_document.execution_mode = String::from("featureforge:executing-plans");
        String::from("featureforge:executing-plans")
    } else {
        context.plan_document.execution_mode.clone()
    };
    context.evidence.format = EvidenceFormat::V2;
    let plan_fingerprint = hash_contract_plan(&context.plan_source);
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let head_sha = state_payload
        .get("repo_state_baseline_head_sha")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            context
                .current_head_sha()
                .unwrap_or_else(|_| String::from("authoritative-event-log"))
        });
    context.evidence.plan_fingerprint = Some(plan_fingerprint);
    context.evidence.source_spec_fingerprint = Some(source_spec_fingerprint.clone());
    context.evidence.attempts = completed_steps
        .into_iter()
        .map(|((task_number, step_number), files)| {
            let files = if files.is_empty() {
                vec![NO_REPO_FILES_MARKER.to_owned()]
            } else {
                files
            };
            let file_proofs = files
                .iter()
                .map(|path| FileProof {
                    path: path.clone(),
                    proof: current_file_proof(&context.runtime.repo_root, path),
                })
                .collect::<Vec<_>>();
            let packet_fingerprint = task_packet_fingerprint(
                context,
                &source_spec_fingerprint,
                task_number,
                step_number,
            );
            EvidenceAttempt {
                task_number,
                step_number,
                attempt_number: 1,
                status: String::from("Completed"),
                recorded_at: String::from("authoritative-event-log"),
                execution_source: execution_source.clone(),
                claim: format!(
                    "Authoritative event log marks Task {task_number} Step {step_number} complete."
                ),
                files,
                file_proofs,
                verify_command: None,
                verification_summary: String::from(
                    "Recovered from authoritative completed-step state.",
                ),
                invalidation_reason: String::from("N/A"),
                packet_fingerprint,
                head_sha: Some(head_sha.clone()),
                base_sha: Some(head_sha.clone()),
                source_contract_path: None,
                source_contract_fingerprint: None,
                source_evaluation_report_fingerprint: None,
                evaluator_verdict: None,
                failing_criterion_ids: Vec::new(),
                source_handoff_fingerprint: None,
                repo_state_baseline_head_sha: None,
                repo_state_baseline_worktree_fingerprint: None,
            }
        })
        .collect();
    context.evidence.source = Some(render_canonical_evidence_projection_source(
        &context.plan_rel,
        &context.plan_document,
        &context.plan_source,
        &context.source_spec_source,
        &context.steps,
        &context.evidence,
    ));
    context.evidence.source_origin = EvidenceSourceOrigin::AuthoritativeState;
    Ok(())
}

fn authoritative_completed_steps_for_evidence(
    context: &ExecutionContext,
    state_payload: &serde_json::Value,
) -> Result<BTreeMap<(u32, u32), Vec<String>>, JsonFailure> {
    let mut completed_steps = BTreeMap::<(u32, u32), Vec<String>>::new();
    if let Some(event_completed_steps) = state_payload
        .get("event_completed_steps")
        .and_then(serde_json::Value::as_object)
    {
        for step in parse_authoritative_completed_steps(event_completed_steps)? {
            completed_steps.entry(step).or_default();
        }
    }
    if let Some(records) = state_payload
        .get("current_task_closure_records")
        .and_then(serde_json::Value::as_object)
    {
        for (record_key, record) in records {
            let Some(task) = task_number_from_task_closure_record(record_key, record) else {
                continue;
            };
            let reviewed_surface_paths = record
                .get("effective_reviewed_surface_paths")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            for step in context.steps.iter().filter(|step| step.task_number == task) {
                let entry = completed_steps
                    .entry((step.task_number, step.step_number))
                    .or_default();
                if !reviewed_surface_paths.is_empty() {
                    *entry = reviewed_surface_paths.clone();
                }
            }
        }
    }
    Ok(completed_steps)
}

fn overlay_event_completed_steps(
    context: &mut ExecutionContext,
    completed_steps: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFailure> {
    for (task, step) in parse_authoritative_completed_steps(completed_steps)? {
        let Some(plan_step) = context
            .steps
            .iter_mut()
            .find(|candidate| candidate.task_number == task && candidate.step_number == step)
        else {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative event_completed_steps points to missing Task {task} Step {step}."
                ),
            ));
        };
        plan_step.checked = true;
    }
    Ok(())
}

fn parse_authoritative_completed_steps(
    completed_steps: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<(u32, u32)>, JsonFailure> {
    completed_steps
        .values()
        .map(|entry| {
            let task = entry
                .get("task")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Authoritative event_completed_steps entry is missing a numeric task.",
                    )
                })?;
            let step = entry
                .get("step")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Authoritative event_completed_steps entry is missing a numeric step.",
                    )
                })?;
            Ok((task, step))
        })
        .collect()
}

fn overlay_task_closure_completed_steps(
    context: &mut ExecutionContext,
    state_payload: &serde_json::Value,
) -> Result<(), JsonFailure> {
    let Some(records) = state_payload
        .get("current_task_closure_records")
        .and_then(serde_json::Value::as_object)
    else {
        return Ok(());
    };
    for (record_key, record) in records {
        let Some(task) = task_number_from_task_closure_record(record_key, record) else {
            continue;
        };
        for step in context
            .steps
            .iter_mut()
            .filter(|candidate| candidate.task_number == task)
        {
            step.checked = true;
        }
    }
    Ok(())
}

fn task_number_from_closure_record_key(record_key: &str) -> Option<u32> {
    record_key
        .strip_prefix("task-")
        .unwrap_or(record_key)
        .parse::<u32>()
        .ok()
}

fn task_number_from_task_closure_record(
    record_key: &str,
    record: &serde_json::Value,
) -> Option<u32> {
    record
        .get("task")
        .or_else(|| record.get("task_number"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| task_number_from_closure_record_key(record_key))
}

fn same_branch_worktrees(current_repo_root: &Path) -> Vec<PathBuf> {
    let repo = match discover_repository(current_repo_root) {
        Ok(repo) => repo,
        _ => return Vec::new(),
    };
    let listing_repo = repo.main_repo().unwrap_or(repo);
    let mut entries = listing_repo
        .workdir()
        .map(|work_dir| vec![canonicalize_repo_root_path(work_dir)])
        .unwrap_or_default();
    if let Ok(worktrees) = listing_repo.worktrees() {
        entries.extend(
            worktrees
                .into_iter()
                .filter_map(|worktree| worktree.base().ok())
                .map(|root| canonicalize_repo_root_path(&root)),
        );
    }
    entries.sort();
    entries.dedup();

    entries
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegacyEvidencePolicy {
    Reject,
    Allow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackedEvidenceProjectionPolicy {
    Ignore,
    AllowExplicitImport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovedArtifactSelectionPolicy {
    RequireUnique,
    AllowExactPlan,
}

fn load_execution_context_with_policies(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
    legacy_evidence_policy: LegacyEvidencePolicy,
    tracked_evidence_policy: TrackedEvidenceProjectionPolicy,
    selection_policy: ApprovedArtifactSelectionPolicy,
    apply_authority_overlay: bool,
) -> Result<ExecutionContext, JsonFailure> {
    let plan_rel = normalize_plan_path(plan_path)?;
    let plan_abs = runtime.repo_root.join(&plan_rel);
    if !plan_abs.is_file() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "Approved plan file does not exist.",
        ));
    }

    let mut plan_document = parse_plan_file(&plan_abs).map_err(|error| {
        JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            format!("Approved plan headers are missing or malformed: {error}"),
        )
    })?;
    if plan_document.workflow_state != "Engineering Approved" {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Plan is not Engineering Approved.",
        ));
    }
    match plan_document.execution_mode.as_str() {
        "none" | "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {}
        _ => {
            return Err(JsonFailure::new(
                FailureClass::PlanNotExecutionReady,
                "Execution Mode header is missing, malformed, or out of range.",
            ));
        }
    }
    if plan_document.last_reviewed_by != "plan-eng-review" {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan Last Reviewed By header is missing or malformed.",
        ));
    }
    if plan_document.tasks.iter().any(|task| task.files.is_empty()) {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan tasks require a parseable Files block.",
        ));
    }

    let plan_source = fs::read_to_string(&plan_abs).map_err(|error| {
        JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            format!(
                "Could not read approved plan {}: {error}",
                plan_abs.display()
            ),
        )
    })?;
    let mut parsed_steps = parse_step_state(&plan_source, &plan_document)?;
    let markdown_has_checked_steps = parsed_steps.iter().any(|step| step.checked);
    let markdown_has_open_step_notes = parsed_steps.iter().any(|step| step.note_state.is_some());

    // Legacy markdown execution marks and notes are migration candidates only.
    // They must not remain live read-surface authority once captured.
    clear_open_step_projections_for_steps(&mut parsed_steps);
    for step in &mut parsed_steps {
        step.checked = false;
    }

    let source_spec_path = runtime.repo_root.join(&plan_document.source_spec_path);
    let source_spec_source = fs::read_to_string(&source_spec_path).map_err(|_| {
        JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan source spec does not exist.",
        )
    })?;
    let matching_manifest = matching_workflow_manifest(runtime);
    validate_source_spec(
        &source_spec_source,
        &plan_document.source_spec_path,
        plan_document.source_spec_revision,
        runtime,
        matching_manifest.as_ref(),
        selection_policy,
    )?;
    validate_unique_approved_plan(
        &plan_rel,
        &plan_document.source_spec_path,
        plan_document.source_spec_revision,
        runtime,
        matching_manifest.as_ref(),
        selection_policy,
    )?;

    let evidence_rel = derive_evidence_rel_path(&plan_rel, plan_document.plan_revision);
    let evidence_abs = runtime.repo_root.join(&evidence_rel);
    let plan_header_execution_mode = plan_document.execution_mode.clone();
    let evidence = parse_execution_evidence_projection(ExecutionEvidenceProjectionParseInput {
        runtime,
        evidence_rel: &evidence_rel,
        evidence_abs: &evidence_abs,
        plan_source: &plan_source,
        expected_plan_path: &plan_rel,
        plan_document: &plan_document,
        steps: &parsed_steps,
        expected_spec_path: &plan_document.source_spec_path,
        source_spec_source: &source_spec_source,
        allow_legacy_unbound: legacy_evidence_policy == LegacyEvidencePolicy::Allow,
        allow_tracked_projection: tracked_evidence_policy
            == TrackedEvidenceProjectionPolicy::AllowExplicitImport,
    })?;

    infer_execution_mode_from_evidence(&mut plan_document, &evidence);

    if legacy_evidence_policy == LegacyEvidencePolicy::Reject
        && evidence.format == EvidenceFormat::Legacy
        && !evidence.attempts.is_empty()
    {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Legacy pre-harness execution evidence is no longer accepted; regenerate execution evidence using the harness v2 format.",
        ));
    }

    if plan_document.execution_mode == "none" && !evidence.attempts.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Execution evidence history cannot exist while Execution Mode is none.",
        ));
    }
    let local_execution_progress_markers_present = plan_header_execution_mode != "none"
        || (evidence.source_origin == EvidenceSourceOrigin::TrackedFile
            && !evidence.attempts.is_empty());
    let tasks_by_number = plan_document
        .tasks
        .iter()
        .cloned()
        .map(|task| (task.number, task))
        .collect();
    let mut context = ExecutionContext {
        runtime: runtime.clone(),
        plan_rel,
        plan_abs,
        plan_document,
        plan_source,
        steps: parsed_steps,
        local_execution_progress_markers_present,
        legacy_open_step_projection_present: markdown_has_open_step_notes,
        tasks_by_number,
        evidence_rel,
        evidence_abs,
        evidence,
        authoritative_evidence_projection_fingerprint: None,
        source_spec_source,
        source_spec_path,
        execution_fingerprint: String::new(),
        tracked_tree_sha_cache: OnceLock::new(),
        semantic_workspace_snapshot_cache: OnceLock::new(),
        reviewed_tree_sha_cache: RefCell::new(BTreeMap::new()),
        head_sha_cache: OnceLock::new(),
        release_base_branch_cache: OnceLock::new(),
        tracked_worktree_changes_excluding_execution_evidence_cache: OnceLock::new(),
    };

    let authoritative_state = if apply_authority_overlay {
        load_authoritative_transition_state_relaxed(&context)?
    } else {
        None
    };
    if apply_authority_overlay {
        overlay_execution_evidence_attempts_from_authority(
            &mut context,
            authoritative_state.as_ref(),
        )?;
        infer_execution_mode_from_evidence(&mut context.plan_document, &context.evidence);
        overlay_step_state_from_authority(&mut context, authoritative_state.as_ref())?;
    }
    refresh_execution_fingerprint(&mut context);

    if context.plan_document.execution_mode == "none"
        && (markdown_has_checked_steps || markdown_has_open_step_notes)
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Newly approved plan revisions must start execution-clean.",
        ));
    }

    for attempt in &context.evidence.attempts {
        if !context.steps.iter().any(|step| {
            step.task_number == attempt.task_number && step.step_number == attempt.step_number
        }) {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Execution evidence references a task/step that does not exist in the approved plan.",
            ));
        }
        normalize_source(
            &attempt.execution_source,
            &context.plan_document.execution_mode,
        )
        .map_err(|_| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Execution evidence source must match the persisted execution mode.",
            )
        })?;
    }

    Ok(context)
}

fn refresh_execution_fingerprint(context: &mut ExecutionContext) {
    let normalized_plan_source = normalized_plan_source_for_semantic_identity(&context.plan_source);
    context.execution_fingerprint = compute_execution_fingerprint(
        &normalized_plan_source,
        context.evidence.source.as_deref(),
        context
            .authoritative_evidence_projection_fingerprint
            .as_deref(),
        &execution_state_fingerprint_source(context),
    );
}

pub fn validate_expected_fingerprint(
    context: &ExecutionContext,
    expected: &str,
) -> Result<(), JsonFailure> {
    if context.execution_fingerprint != expected {
        return Err(JsonFailure::new(
            FailureClass::StaleMutation,
            "Execution state changed since the last parsed execution fingerprint.",
        ));
    }
    Ok(())
}

pub fn status_from_context(context: &ExecutionContext) -> Result<PlanExecutionStatus, JsonFailure> {
    status_from_context_with_overlay(context, None, false, None, false, None)
}

pub(crate) fn status_from_context_with_overlay(
    context: &ExecutionContext,
    preloaded_overlay: Option<&StatusAuthoritativeOverlay>,
    use_preloaded_overlay: bool,
    preloaded_authoritative_state: Option<&AuthoritativeTransitionState>,
    use_preloaded_authoritative_state: bool,
    gate_projection: Option<GateProjectionInputs<'_>>,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let loaded_authoritative_state;
    let authoritative_state = if use_preloaded_authoritative_state {
        preloaded_authoritative_state
    } else {
        loaded_authoritative_state = load_authoritative_transition_state(context)?;
        loaded_authoritative_state.as_ref()
    };
    let preflight_acceptance = match preflight_acceptance_for_context(context) {
        Ok(acceptance) => acceptance,
        Err(error) => {
            if authoritative_execution_run_id_from_state(authoritative_state).is_some() {
                None
            } else {
                return Err(error);
            }
        }
    };
    let started = execution_started(context, authoritative_state);
    let warning_codes = Vec::new();
    let execution_run_id = current_execution_run_id_with_authority(context, authoritative_state)?
        .map(ExecutionRunId::new);
    let chunk_id = preflight_acceptance
        .as_ref()
        .map(|acceptance| acceptance.chunk_id.clone())
        .unwrap_or_else(|| pending_chunk_id(context));
    let chunking_strategy = preflight_acceptance
        .as_ref()
        .map(|acceptance| acceptance.chunking_strategy);
    let evaluator_policy = preflight_acceptance
        .as_ref()
        .map(|acceptance| acceptance.evaluator_policy.clone());
    let reset_policy = preflight_acceptance
        .as_ref()
        .map(|acceptance| acceptance.reset_policy);
    let review_stack = preflight_acceptance
        .as_ref()
        .map(|acceptance| acceptance.review_stack.clone());
    let semantic_snapshot = semantic_workspace_snapshot(context)?;
    let projection_metadata =
        execution_projection_read_model_metadata(context, normal_projection_write_mode()?)?;

    let mut status = PlanExecutionStatus {
        schema_version: 3,
        plan_revision: context.plan_document.plan_revision,
        execution_run_id,
        workspace_state_id: semantic_snapshot.raw_workspace_tree_id.clone(),
        current_branch_reviewed_state_id: None,
        current_branch_closure_id: None,
        current_branch_meaningful_drift: false,
        current_task_closures: Vec::new(),
        superseded_closures_summary: Vec::new(),
        stale_unreviewed_closures: Vec::new(),
        current_release_readiness_state: None,
        current_final_review_state: String::from("not_required"),
        current_qa_state: String::from("not_required"),
        current_final_review_branch_closure_id: None,
        current_final_review_result: None,
        current_qa_branch_closure_id: None,
        current_qa_result: None,
        qa_requirement: None,
        latest_authoritative_sequence: INITIAL_AUTHORITATIVE_SEQUENCE,
        phase: None,
        harness_phase: if started {
            HarnessPhase::Executing
        } else if preflight_acceptance.is_some() {
            HarnessPhase::ExecutionPreflight
        } else {
            HarnessPhase::ImplementationHandoff
        },
        chunk_id,
        chunking_strategy,
        evaluator_policy,
        reset_policy,
        review_stack,
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
        write_authority_state: String::from("preflight_pending"),
        write_authority_holder: None,
        write_authority_worktree: None,
        repo_state_baseline_head_sha: None,
        repo_state_baseline_worktree_fingerprint: None,
        repo_state_drift_state: String::from("preflight_pending"),
        dependency_index_state: String::from("missing"),
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
        phase_detail: String::from("planning_reentry_required"),
        review_state_status: String::from("clean"),
        recording_context: None,
        execution_command_context: None,
        execution_reentry_target_source: None,
        public_repair_targets: Vec::new(),
        blocking_records: Vec::new(),
        blocking_scope: None,
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        projection_diagnostics: Vec::new(),
        state_kind: String::from("actionable_public_command"),
        next_public_action: None,
        blockers: Vec::new(),
        semantic_workspace_tree_id: semantic_snapshot.semantic_workspace_tree_id,
        raw_workspace_tree_id: Some(semantic_snapshot.raw_workspace_tree_id),
        next_action: String::from("inspect_workflow"),
        recommended_command: None,
        finish_review_gate_pass_branch_closure_id: None,
        reason_codes: Vec::new(),
        execution_mode: context.plan_document.execution_mode.clone(),
        execution_fingerprint: context.execution_fingerprint.clone(),
        evidence_path: context.evidence_rel.clone(),
        projection_mode: projection_metadata.projection_mode,
        state_dir_projection_paths: projection_metadata.state_dir_projection_paths,
        tracked_projection_paths: projection_metadata.tracked_projection_paths,
        tracked_projections_current: projection_metadata.tracked_projections_current,
        execution_started: if started {
            String::from("yes")
        } else {
            String::from("no")
        },
        warning_codes,
        active_task: None,
        active_step: None,
        blocking_task: None,
        blocking_step: None,
        resume_task: None,
        resume_step: None,
    };

    project_authoritative_open_step_status_fields(context, &mut status);

    apply_authoritative_status_overlay(
        context,
        &mut status,
        preloaded_overlay,
        use_preloaded_overlay,
    )?;
    apply_task_boundary_status_overlay(context, &mut status);
    apply_current_task_closure_repair_status_overlay(context, &mut status);
    suppress_preempted_resume_status_fields(context, &mut status);
    apply_late_stage_precedence_status_overlay(
        context,
        &mut status,
        authoritative_state,
        gate_projection,
    );
    populate_public_status_contract_fields(
        context,
        &mut status,
        preloaded_overlay,
        use_preloaded_overlay,
        authoritative_state,
        true,
        gate_projection,
    )?;
    Ok(status)
}

fn project_authoritative_open_step_status_fields(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
) {
    if let Some(step) = active_step(context, NoteState::Active) {
        status.active_task = Some(step.task_number);
        status.active_step = Some(step.step_number);
        status.resume_task = None;
        status.resume_step = None;
        if status.blocking_step.is_some() {
            status.blocking_task = None;
            status.blocking_step = None;
        }
        return;
    }
    if let Some(step) = active_step(context, NoteState::Blocked) {
        status.active_task = None;
        status.active_step = None;
        status.resume_task = None;
        status.resume_step = None;
        status.blocking_task = Some(step.task_number);
        status.blocking_step = Some(step.step_number);
        return;
    }
    if let Some(step) = active_step(context, NoteState::Interrupted) {
        status.active_task = None;
        status.active_step = None;
        status.resume_task = Some(step.task_number);
        status.resume_step = Some(step.step_number);
        if status.blocking_step.is_some() {
            status.blocking_task = None;
            status.blocking_step = None;
        }
    }
}

fn apply_authoritative_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    preloaded_overlay: Option<&StatusAuthoritativeOverlay>,
    use_preloaded_overlay: bool,
) -> Result<(), JsonFailure> {
    let state_path = authoritative_state_path(context);
    let loaded_overlay;
    let overlay = if use_preloaded_overlay {
        preloaded_overlay
    } else {
        loaded_overlay = load_status_authoritative_overlay_checked(context)?;
        loaded_overlay.as_ref()
    };
    let Some(overlay) = overlay else {
        return Ok(());
    };

    if let Some(phase) = normalize_optional_overlay_value(overlay.harness_phase.as_deref()) {
        status.harness_phase = parse_harness_phase(phase).ok_or_else(|| {
            malformed_overlay_field(
                &state_path,
                "harness_phase",
                phase,
                "must be one of the public harness phases",
            )
        })?;
    }

    if let Some(chunk_id) = normalize_optional_overlay_value(overlay.chunk_id.as_deref()) {
        status.chunk_id = ChunkId::new(chunk_id.to_owned());
    }

    if let Some(sequence) = overlay
        .latest_authoritative_sequence
        .or(overlay.authoritative_sequence)
    {
        status.latest_authoritative_sequence = sequence;
    }

    let (active_contract_path, active_contract_fingerprint) = parse_overlay_active_contract_fields(
        overlay.active_contract_path.as_deref(),
        overlay.active_contract_fingerprint.as_deref(),
        &state_path,
    )?;
    status.active_contract_path = active_contract_path;
    status.active_contract_fingerprint = active_contract_fingerprint;

    status.required_evaluator_kinds = parse_evaluator_kinds(
        &overlay.required_evaluator_kinds,
        "required_evaluator_kinds",
        &state_path,
    )?;
    status.completed_evaluator_kinds = parse_evaluator_kinds(
        &overlay.completed_evaluator_kinds,
        "completed_evaluator_kinds",
        &state_path,
    )?;
    status.pending_evaluator_kinds = parse_evaluator_kinds(
        &overlay.pending_evaluator_kinds,
        "pending_evaluator_kinds",
        &state_path,
    )?;
    status.non_passing_evaluator_kinds = parse_evaluator_kinds(
        &overlay.non_passing_evaluator_kinds,
        "non_passing_evaluator_kinds",
        &state_path,
    )?;

    if let Some(value) =
        normalize_optional_overlay_value(overlay.aggregate_evaluation_state.as_deref())
    {
        status.aggregate_evaluation_state =
            parse_aggregate_evaluation_state(value).ok_or_else(|| {
                malformed_overlay_field(
                    &state_path,
                    "aggregate_evaluation_state",
                    value,
                    "must be pass, fail, blocked, or pending",
                )
            })?;
    }

    status.last_evaluation_report_path =
        normalize_optional_overlay_value(overlay.last_evaluation_report_path.as_deref())
            .map(str::to_owned);
    status.last_evaluation_report_fingerprint =
        normalize_optional_overlay_value(overlay.last_evaluation_report_fingerprint.as_deref())
            .map(str::to_owned);
    status.last_evaluation_evaluator_kind = parse_optional_evaluator_kind(
        overlay.last_evaluation_evaluator_kind.as_deref(),
        "last_evaluation_evaluator_kind",
        &state_path,
    )?;
    status.last_evaluation_verdict = parse_optional_evaluation_verdict(
        overlay.last_evaluation_verdict.as_deref(),
        "last_evaluation_verdict",
        &state_path,
    )?;

    if let Some(value) = overlay.current_chunk_retry_count {
        status.current_chunk_retry_count = value;
    }
    if let Some(value) = overlay.current_chunk_retry_budget {
        status.current_chunk_retry_budget = value;
    }
    if let Some(value) = overlay.current_chunk_pivot_threshold {
        status.current_chunk_pivot_threshold = value;
    }
    if let Some(value) = overlay.handoff_required {
        status.handoff_required = value;
    }
    if !overlay.open_failed_criteria.is_empty() {
        status.open_failed_criteria = overlay.open_failed_criteria.clone();
    }
    if let Some(value) = normalize_optional_overlay_value(overlay.write_authority_state.as_deref())
    {
        status.write_authority_state = value.to_owned();
    }
    status.write_authority_holder =
        normalize_optional_overlay_value(overlay.write_authority_holder.as_deref())
            .map(str::to_owned);
    status.write_authority_worktree =
        normalize_optional_overlay_value(overlay.write_authority_worktree.as_deref())
            .map(str::to_owned);
    status.repo_state_baseline_head_sha =
        normalize_optional_overlay_value(overlay.repo_state_baseline_head_sha.as_deref())
            .map(str::to_owned);
    status.repo_state_baseline_worktree_fingerprint = normalize_optional_overlay_value(
        overlay.repo_state_baseline_worktree_fingerprint.as_deref(),
    )
    .map(str::to_owned);
    if let Some(value) = normalize_optional_overlay_value(overlay.repo_state_drift_state.as_deref())
    {
        status.repo_state_drift_state = value.to_owned();
    }
    if let Some(value) = normalize_optional_overlay_value(overlay.dependency_index_state.as_deref())
    {
        status.dependency_index_state = value.to_owned();
    }
    if let Some(value) = parse_optional_downstream_freshness_state(
        overlay.final_review_state.as_deref(),
        "final_review_state",
        &state_path,
    )? {
        status.final_review_state = value;
    }
    if let Some(value) = parse_optional_downstream_freshness_state(
        overlay.browser_qa_state.as_deref(),
        "browser_qa_state",
        &state_path,
    )? {
        status.browser_qa_state = value;
    }
    if let Some(value) = parse_optional_downstream_freshness_state(
        overlay.release_docs_state.as_deref(),
        "release_docs_state",
        &state_path,
    )? {
        status.release_docs_state = value;
    }
    status.last_final_review_artifact_fingerprint =
        normalize_optional_overlay_value(overlay.last_final_review_artifact_fingerprint.as_deref())
            .map(str::to_owned);
    status.last_browser_qa_artifact_fingerprint =
        normalize_optional_overlay_value(overlay.last_browser_qa_artifact_fingerprint.as_deref())
            .map(str::to_owned);
    status.last_release_docs_artifact_fingerprint =
        normalize_optional_overlay_value(overlay.last_release_docs_artifact_fingerprint.as_deref())
            .map(str::to_owned);
    if let Some(value) = normalize_optional_overlay_value(overlay.strategy_state.as_deref()) {
        status.strategy_state = value.to_owned();
    }
    status.last_strategy_checkpoint_fingerprint =
        normalize_optional_overlay_value(overlay.last_strategy_checkpoint_fingerprint.as_deref())
            .map(str::to_owned);
    if let Some(value) =
        normalize_optional_overlay_value(overlay.strategy_checkpoint_kind.as_deref())
    {
        status.strategy_checkpoint_kind = value.to_owned();
    }
    if let Some(value) = overlay.strategy_reset_required {
        status.strategy_reset_required = value;
    }
    if !overlay.reason_codes.is_empty() {
        status.reason_codes =
            parse_reason_codes(&overlay.reason_codes, "reason_codes", &state_path)?;
    }
    status.current_branch_closure_id =
        normalize_optional_overlay_value(overlay.current_branch_closure_id.as_deref())
            .map(str::to_owned);
    status.current_branch_reviewed_state_id = normalize_optional_overlay_value(
        overlay.current_branch_closure_reviewed_state_id.as_deref(),
    )
    .map(str::to_owned);
    status.current_release_readiness_state =
        normalize_optional_overlay_value(overlay.current_release_readiness_result.as_deref())
            .map(str::to_owned);

    Ok(())
}

fn normalize_optional_overlay_value(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn push_missing_derived_field(missing: &mut Vec<String>, field: &str) {
    if classify_review_state_field(field)
        == Some(PersistedReviewStateFieldClass::AuthoritativeAppendOnlyHistory)
    {
        return;
    }
    if !missing.iter().any(|existing| existing == field) {
        missing.push(field.to_owned());
    }
}

pub(crate) fn missing_derived_review_state_fields(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    overlay: Option<&StatusAuthoritativeOverlay>,
) -> Vec<String> {
    let mut missing = Vec::new();
    if let Some(authoritative_state) = authoritative_state
        && authoritative_state.current_task_closure_overlay_needs_restore()
    {
        push_missing_derived_field(&mut missing, "current_task_closure_records");
    }

    let Some(overlay) = overlay else {
        return missing;
    };
    let overlay_current_branch_closure_id =
        normalize_optional_overlay_value(overlay.current_branch_closure_id.as_deref());

    let Some(authoritative_state) = authoritative_state else {
        if overlay_current_branch_closure_id.is_some() {
            push_missing_derived_field(&mut missing, "current_branch_closure_id");
            if normalize_optional_overlay_value(
                overlay.current_branch_closure_reviewed_state_id.as_deref(),
            )
            .is_none()
            {
                push_missing_derived_field(
                    &mut missing,
                    "current_branch_closure_reviewed_state_id",
                );
            }
            if normalize_optional_overlay_value(
                overlay.current_branch_closure_contract_identity.as_deref(),
            )
            .is_none()
            {
                push_missing_derived_field(
                    &mut missing,
                    "current_branch_closure_contract_identity",
                );
            }
        }
        return missing;
    };

    let bound_current_branch_closure = authoritative_state.bound_current_branch_closure_identity();
    let current_branch_closure_id = bound_current_branch_closure
        .as_ref()
        .map(|identity| identity.branch_closure_id.as_str());
    if let Some(current_identity) = bound_current_branch_closure.as_ref() {
        if overlay_current_branch_closure_id != Some(current_identity.branch_closure_id.as_str()) {
            push_missing_derived_field(&mut missing, "current_branch_closure_id");
        }
        if normalize_optional_overlay_value(
            overlay.current_branch_closure_reviewed_state_id.as_deref(),
        ) != Some(current_identity.reviewed_state_id.as_str())
        {
            push_missing_derived_field(&mut missing, "current_branch_closure_reviewed_state_id");
        }
        if normalize_optional_overlay_value(
            overlay.current_branch_closure_contract_identity.as_deref(),
        ) != Some(current_identity.contract_identity.as_str())
        {
            push_missing_derived_field(&mut missing, "current_branch_closure_contract_identity");
        }
    } else if overlay_current_branch_closure_id.is_some() {
        push_missing_derived_field(&mut missing, "current_branch_closure_id");
        if normalize_optional_overlay_value(
            overlay.current_branch_closure_reviewed_state_id.as_deref(),
        )
        .is_none()
        {
            push_missing_derived_field(&mut missing, "current_branch_closure_reviewed_state_id");
        }
        if normalize_optional_overlay_value(
            overlay.current_branch_closure_contract_identity.as_deref(),
        )
        .is_none()
        {
            push_missing_derived_field(&mut missing, "current_branch_closure_contract_identity");
        }
    }

    if let Some(record) = authoritative_state.current_final_review_record()
        && current_branch_closure_id == Some(record.branch_closure_id.as_str())
    {
        if authoritative_state
            .current_final_review_record_id()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_record_id");
        }
        if authoritative_state
            .current_final_review_branch_closure_id()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_branch_closure_id");
        }
        if authoritative_state
            .current_final_review_dispatch_id()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_dispatch_id");
        }
        if authoritative_state
            .current_final_review_reviewer_source()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_reviewer_source");
        }
        if authoritative_state
            .current_final_review_reviewer_id()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_reviewer_id");
        }
        if authoritative_state.current_final_review_result().is_none() {
            push_missing_derived_field(&mut missing, "current_final_review_result");
        }
        if authoritative_state
            .current_final_review_summary_hash()
            .is_none()
        {
            push_missing_derived_field(&mut missing, "current_final_review_summary_hash");
        }
    }

    if let Some(record) = authoritative_state.current_browser_qa_record()
        && current_branch_closure_id == Some(record.branch_closure_id.as_str())
    {
        if authoritative_state.current_qa_record_id().is_none() {
            push_missing_derived_field(&mut missing, "current_qa_record_id");
        }
        if authoritative_state.current_qa_branch_closure_id().is_none() {
            push_missing_derived_field(&mut missing, "current_qa_branch_closure_id");
        }
        if authoritative_state.current_qa_result().is_none() {
            push_missing_derived_field(&mut missing, "current_qa_result");
        }
        if authoritative_state.current_qa_summary_hash().is_none() {
            push_missing_derived_field(&mut missing, "current_qa_summary_hash");
        }
    }

    missing
}

fn apply_task_boundary_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
) {
    if status.blocking_task.is_some() {
        return;
    }
    if let Some(active_task) = status.active_task {
        if pre_reducer_earliest_unresolved_stale_task(context, status).is_none()
            && let Some(prior_task) = prior_task_number_for_begin(context, active_task)
            && let Err(error) = require_prior_task_closure_for_begin(context, active_task)
        {
            let mut missing_current_closure_boundary = false;
            if let Some(reason_code) = task_boundary_reason_code_from_message(&error.message)
                && !status
                    .reason_codes
                    .iter()
                    .any(|existing| existing == reason_code)
            {
                status.reason_codes.push(reason_code.to_owned());
                missing_current_closure_boundary =
                    reason_code == "prior_task_current_closure_missing";
            }
            status.blocking_task = Some(prior_task);
            status.blocking_step = None;
            status.active_task = None;
            status.active_step = None;
            if missing_current_closure_boundary {
                push_task_closure_recording_status_reasons(context, status, prior_task);
            }
        }
        return;
    }
    if let Some(resume_task) = status.resume_task {
        if pre_reducer_earliest_unresolved_stale_task(context, status).is_none()
            && let Some(prior_task) = prior_task_number_for_begin(context, resume_task)
            && let Err(error) = require_prior_task_closure_for_begin(context, resume_task)
        {
            let mut missing_current_closure_boundary = false;
            if let Some(reason_code) = task_boundary_reason_code_from_message(&error.message)
                && !status
                    .reason_codes
                    .iter()
                    .any(|existing| existing == reason_code)
            {
                status.reason_codes.push(reason_code.to_owned());
                missing_current_closure_boundary =
                    reason_code == "prior_task_current_closure_missing";
            }
            status.blocking_task = Some(prior_task);
            status.blocking_step = None;
            status.resume_task = None;
            status.resume_step = None;
            if missing_current_closure_boundary {
                push_task_closure_recording_status_reasons(context, status, prior_task);
            }
        }
        return;
    }
    let Some(next_unchecked_task) = context
        .steps
        .iter()
        .find(|step| !step.checked)
        .map(|step| step.task_number)
    else {
        let Some(missing_task) = completed_plan_missing_current_closure_task(context, status)
        else {
            return;
        };
        let overlay = load_status_authoritative_overlay_checked(context)
            .ok()
            .and_then(|overlay| overlay);
        let stale_provenance_recovery_candidate = status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
            && !status
                .reason_codes
                .iter()
                .any(|reason_code| reason_code == "late_stage_surface_not_declared");
        if !stale_provenance_recovery_candidate
            && ((status.latest_authoritative_sequence != INITIAL_AUTHORITATIVE_SEQUENCE
                && status.harness_phase != HarnessPhase::Executing)
                || is_late_stage_phase(status.harness_phase)
                || authoritative_late_stage_rederivation_basis_present(context, status)
                || overlay
                    .as_ref()
                    .is_some_and(has_authoritative_late_stage_progress))
        {
            return;
        }
        if !stale_provenance_recovery_candidate {
            push_task_closure_recording_status_reasons(context, status, missing_task);
        }
        push_status_reason_code_once(status, "prior_task_current_closure_missing");
        status.blocking_task = Some(missing_task);
        status.blocking_step = None;
        return;
    };
    {
        let Some(prior_task) = prior_task_number_for_begin(context, next_unchecked_task) else {
            return;
        };
        let Err(error) = require_prior_task_closure_for_begin(context, next_unchecked_task) else {
            return;
        };
        let mut missing_current_closure_boundary = false;
        if let Some(reason_code) = task_boundary_reason_code_from_message(&error.message)
            && !status
                .reason_codes
                .iter()
                .any(|existing| existing == reason_code)
        {
            status.reason_codes.push(reason_code.to_owned());
            missing_current_closure_boundary = reason_code == "prior_task_current_closure_missing";
        }
        status.blocking_task = Some(prior_task);
        if missing_current_closure_boundary {
            push_task_closure_recording_status_reasons(context, status, prior_task);
        }
    }
}

fn push_task_closure_recording_status_reasons(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    task: u32,
) {
    let Ok(prerequisites) = task_closure_recording_prerequisites(context, task) else {
        return;
    };
    let current_dispatch_ready = prerequisites
        .dispatch_id
        .as_deref()
        .is_some_and(|dispatch_id| !dispatch_id.trim().is_empty());
    let baseline_candidate_present = task_closure_baseline_repair_candidate_with_stale_target(
        context,
        status,
        task,
        pre_reducer_earliest_unresolved_stale_task(context, status),
    )
    .ok()
    .flatten()
    .is_some();
    let stale_bridge_ready =
        stale_unreviewed_allows_task_closure_baseline_bridge(context, status, task)
            .unwrap_or(false);
    if current_dispatch_ready || baseline_candidate_present {
        push_status_reason_code_once(status, "task_closure_baseline_repair_candidate");
    }
    if stale_bridge_ready {
        push_status_reason_code_once(status, "task_closure_baseline_bridge_ready");
    }
    for reason_code in prerequisites
        .blocking_reason_codes
        .iter()
        .filter(|reason_code| task_closure_recording_reason_code(reason_code))
    {
        push_status_reason_code_once(status, reason_code);
    }
}

fn has_authoritative_late_stage_progress(overlay: &StatusAuthoritativeOverlay) -> bool {
    normalize_optional_overlay_value(overlay.current_branch_closure_id.as_deref()).is_some()
        || overlay.final_review_dispatch_lineage.is_some()
        || normalize_optional_overlay_value(overlay.current_release_readiness_result.as_deref())
            .is_some()
        || normalize_optional_overlay_value(overlay.final_review_state.as_deref()).is_some()
        || normalize_optional_overlay_value(overlay.browser_qa_state.as_deref()).is_some()
        || normalize_optional_overlay_value(overlay.release_docs_state.as_deref()).is_some()
}

fn current_task_closure_set_ready_for_late_stage(context: &ExecutionContext) -> bool {
    if structural_current_task_closure_failures(context)
        .map(|failures| !failures.is_empty())
        .unwrap_or(true)
    {
        return false;
    }
    let current_task_closures = match still_current_task_closure_records(context) {
        Ok(current_task_closures) => current_task_closures,
        Err(_) => return false,
    };
    if !current_task_closures
        .iter()
        .any(|record| task_closure_contributes_to_branch_surface(context, record))
    {
        return false;
    }
    branch_closure_rerecording_assessment(context)
        .map(|assessment| assessment.supported)
        .unwrap_or(false)
}

fn authoritative_late_stage_rederivation_basis_present(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    if public_late_stage_rederivation_basis_present(status) {
        return true;
    }
    if current_task_closure_set_ready_for_late_stage(context) {
        return true;
    }
    if load_authoritative_transition_state(context)
        .ok()
        .flatten()
        .is_some_and(|state| {
            validated_current_branch_closure_identity(context).is_some()
                || state.current_release_readiness_record().is_some()
                || state.current_final_review_record().is_some()
                || state.current_browser_qa_record().is_some()
        })
    {
        return true;
    }
    load_status_authoritative_overlay_checked(context)
        .ok()
        .flatten()
        .is_some_and(|overlay| {
            overlay.final_review_dispatch_lineage.is_some()
                || normalize_optional_overlay_value(overlay.current_branch_closure_id.as_deref())
                    .is_some()
                || normalize_optional_overlay_value(
                    overlay.current_release_readiness_result.as_deref(),
                )
                .is_some()
                || normalize_optional_overlay_value(overlay.final_review_state.as_deref()).is_some()
                || normalize_optional_overlay_value(overlay.browser_qa_state.as_deref()).is_some()
                || normalize_optional_overlay_value(overlay.release_docs_state.as_deref()).is_some()
        })
}

fn apply_late_stage_precedence_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    gate_projection: Option<GateProjectionInputs<'_>>,
) {
    if status.execution_started != "yes" {
        return;
    }
    hydrate_status_authority_fields_for_routing(context, status, authoritative_state);

    let ordinary_execution_remaining = status.active_task.is_some()
        || status.resume_task.is_some()
        || status.blocking_task.is_some()
        || !context_all_task_scopes_closed_by_authority(context, authoritative_state);
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
        authoritative_late_stage_rederivation_basis_present(context, status);
    if !late_stage_basis_present {
        if is_late_stage_phase(authoritative_phase) {
            push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
            if task_scope_review_state_repair_reason(status).is_some()
                || status
                    .reason_codes
                    .iter()
                    .any(|reason_code| reason_code == "derived_review_state_missing")
            {
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
        push_status_reason_code_once(status, "qa_requirement_missing_or_invalid");
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
        || gate_review.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "release_docs_state_missing"
                    | "release_docs_state_stale"
                    | "release_docs_state_not_fresh"
            )
        });
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
        && let Ok(records) = still_current_task_closure_records_from_authoritative_state(
            context,
            authoritative_state,
        )
    {
        status.current_task_closures = records
            .into_iter()
            .map(|record| PublicReviewStateTaskClosure {
                task: record.task,
                closure_record_id: record.closure_record_id,
                reviewed_state_id: record.reviewed_state_id,
                contract_identity: record.contract_identity,
                effective_reviewed_surface_paths: record.effective_reviewed_surface_paths,
            })
            .collect();
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

fn apply_current_task_closure_repair_status_overlay(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
) {
    if context.steps.iter().any(|step| !step.checked) {
        return;
    }
    let Ok(structural_failures) = structural_current_task_closure_failures(context) else {
        return;
    };
    for failure in structural_failures {
        push_status_reason_code_once(status, &failure.reason_code);
    }
    let Ok(current_records) = valid_current_task_closure_records(context) else {
        return;
    };
    for record in current_records {
        match task_closure_matches_current_workspace(context, &record) {
            Ok(true) => {}
            Ok(false) => {
                push_status_reason_code_once(status, "prior_task_current_closure_stale");
            }
            Err(error) => {
                if let Some(reason_code) = task_boundary_reason_code_from_message(&error.message) {
                    push_status_reason_code_once(status, reason_code);
                }
            }
        }
    }
}

fn suppress_preempted_resume_status_fields(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
) {
    let Some(resume_task) = status.resume_task else {
        return;
    };
    let stale_preempts_resume = pre_reducer_earliest_unresolved_stale_task(context, status)
        .is_some_and(|earliest_task| earliest_task < resume_task);
    let bridge_preempts_resume = status.blocking_task.is_some_and(|blocking_task| {
        task_closure_baseline_repair_candidate_with_stale_target(
            context,
            status,
            blocking_task,
            pre_reducer_earliest_unresolved_stale_task(context, status),
        )
        .ok()
        .flatten()
        .is_some()
            && stale_unreviewed_allows_task_closure_baseline_bridge(context, status, blocking_task)
                .unwrap_or(false)
    });
    let execution_reentry_preempts_resume = status.phase_detail == "execution_reentry_required"
        && status.blocking_task.is_some_and(|blocking_task| {
            blocking_task != resume_task && blocking_task < resume_task
        });
    let cycle_break_preempts_resume = status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "task_cycle_break_active")
        && load_status_authoritative_overlay_checked(context)
            .ok()
            .flatten()
            .and_then(|overlay| overlay.strategy_cycle_break_task)
            .is_some_and(|cycle_break_task| {
                cycle_break_task != resume_task && cycle_break_task < resume_task
            });
    if stale_preempts_resume
        || bridge_preempts_resume
        || execution_reentry_preempts_resume
        || cycle_break_preempts_resume
    {
        status.resume_task = None;
        status.resume_step = None;
    }
}

fn populate_public_status_contract_fields(
    context: &ExecutionContext,
    status: &mut PlanExecutionStatus,
    preloaded_overlay: Option<&StatusAuthoritativeOverlay>,
    use_preloaded_overlay: bool,
    preloaded_authoritative_state: Option<&AuthoritativeTransitionState>,
    use_preloaded_authoritative_state: bool,
    gate_projection: Option<GateProjectionInputs<'_>>,
) -> Result<(), JsonFailure> {
    let loaded_overlay;
    let overlay = if use_preloaded_overlay {
        preloaded_overlay
    } else {
        loaded_overlay = load_status_authoritative_overlay_checked(context)?;
        loaded_overlay.as_ref()
    };
    let loaded_event_authority_state;
    // This wrapper is reduced from `execution-harness/events.jsonl`; it is not a direct
    // `state.json` truth read, even though the helper retains the historical type name.
    let event_authority_state = if use_preloaded_authoritative_state {
        preloaded_authoritative_state
    } else {
        loaded_event_authority_state = load_authoritative_transition_state(context)?;
        loaded_event_authority_state.as_ref()
    };
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
            push_status_reason_code_once(status, "current_branch_closure_reviewed_state_malformed");
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
    let current_task_closures = match event_authority_state {
        Some(state) => still_current_task_closure_records_from_authoritative_state(context, state)?,
        None => still_current_task_closure_records(context)?,
    }
    .into_iter()
    .map(|record| PublicReviewStateTaskClosure {
        task: record.task,
        closure_record_id: record.closure_record_id,
        reviewed_state_id: record.reviewed_state_id,
        contract_identity: record.contract_identity,
        effective_reviewed_surface_paths: record.effective_reviewed_surface_paths,
    })
    .collect::<Vec<_>>();
    status.current_task_closures = current_task_closures;
    status.superseded_closures_summary = closure_graph.superseded_record_ids();
    status.finish_review_gate_pass_branch_closure_id =
        late_stage_bindings.finish_review_gate_pass_branch_closure_id;
    if let Some(late_stage_phase) = canonical_late_stage_phase_from_bindings(status) {
        status.harness_phase = late_stage_phase;
    }

    let fallback_gate_review;
    let fallback_gate_finish;
    let (gate_review, gate_finish) = match gate_projection {
        Some(gate_projection) => (gate_projection.gate_review, gate_projection.gate_finish),
        None => {
            fallback_gate_review = GateState::default().finish();
            fallback_gate_finish = GateState::default().finish();
            (&fallback_gate_review, &fallback_gate_finish)
        }
    };
    let missing_derived_overlays =
        missing_derived_review_state_fields(event_authority_state, overlay);
    if !missing_derived_overlays.is_empty() {
        push_status_reason_code_once(status, "derived_review_state_missing");
    }
    let task_scope_overlay_restore_required = status.execution_started == "yes"
        && shared_task_scope_overlay_restore_required(
            &missing_derived_overlays,
            event_authority_state,
        );
    if let Some(event_authority_state) = event_authority_state
        && event_authority_state.current_task_closure_overlay_needs_restore()
    {
        push_status_reason_code_once(status, "current_task_closure_overlay_restore_required");
    }
    if task_scope_overlay_restore_required {
        status.harness_phase = HarnessPhase::Executing;
    }
    let repair_route_decision = shared_repair_review_state_reroute_decision(
        context,
        status,
        event_authority_state,
        Some(gate_review),
        Some(gate_finish),
        task_scope_overlay_restore_required,
        false,
    );
    let branch_reroute_still_valid = repair_route_decision.branch_reroute_still_valid;
    let branch_drift_escapes_late_stage_surface =
        repair_route_decision.branch_drift_escapes_late_stage_surface;
    if repair_route_decision.late_stage_surface_not_declared {
        push_status_reason_code_once(status, "late_stage_surface_not_declared");
    }
    if branch_drift_escapes_late_stage_surface {
        push_status_reason_code_once(status, REASON_CODE_STALE_PROVENANCE);
        push_status_reason_code_once(status, "branch_drift_escapes_late_stage_surface");
    }
    let persisted_repair_follow_up = repair_route_decision.persisted_repair_follow_up.as_deref();
    let raw_late_stage_review_state_status =
        repair_route_decision.raw_late_stage_review_state_status;
    let task_scope_repair_precedence_active =
        repair_route_decision.task_scope_repair_precedence_active;
    let repair_reroute = repair_route_decision.repair_reroute;
    if status.blocking_task.is_none()
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && status.current_branch_closure_id.is_none()
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
        && !status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "late_stage_surface_not_declared")
        && let Some(missing_task) = completed_plan_missing_current_closure_task(context, status)
    {
        push_status_reason_code_once(status, "prior_task_current_closure_missing");
        status.blocking_task = Some(missing_task);
        status.blocking_step = None;
    }
    let execution_reentry_target_available = execution_reentry_target(
        context,
        status,
        context.plan_rel.as_str(),
        crate::execution::next_action::NextActionAuthorityInputs::default(),
    )
    .is_some();
    let repair_follow_up_requires_execution_reentry = repair_reroute
        == crate::execution::current_truth::ReviewStateRepairReroute::ExecutionReentry
        && execution_reentry_target_available;
    let repair_follow_up_requires_planning_reentry = repair_reroute
        == crate::execution::current_truth::ReviewStateRepairReroute::ExecutionReentry
        && !execution_reentry_target_available;
    let persisted_branch_reroute_without_current_binding =
        !repair_follow_up_requires_execution_reentry
            && persisted_repair_follow_up == Some("advance_late_stage")
            && !task_scope_repair_precedence_active
            && branch_reroute_still_valid
            && status.current_branch_closure_id.is_some();
    let persisted_branch_reroute_with_current_binding = !repair_follow_up_requires_execution_reentry
        && persisted_repair_follow_up == Some("advance_late_stage")
        && !task_scope_repair_precedence_active
        && branch_reroute_still_valid
        && raw_late_stage_review_state_status == Some("stale_unreviewed")
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE)
        && status.current_branch_closure_id.is_some();
    let repair_follow_up_records_branch_closure = repair_reroute
        == crate::execution::current_truth::ReviewStateRepairReroute::RecordBranchClosure
        || persisted_branch_reroute_without_current_binding
        || persisted_branch_reroute_with_current_binding;
    let authoritative_release_readiness_result =
        authoritative_release_readiness_result_for_current_branch(
            event_authority_state,
            current_late_stage_branch_closure_id.as_deref(),
        );
    let authoritative_release_readiness_current = authoritative_release_readiness_result.is_some();
    let confined_late_stage_branch_drift_with_release_readiness =
        authoritative_release_readiness_current
            && repair_route_decision.branch_reroute_still_valid
            && current_late_stage_branch_closure_id.is_some()
            && status
                .reason_codes
                .iter()
                .any(|reason_code| reason_code == REASON_CODE_STALE_PROVENANCE);
    if (repair_follow_up_records_branch_closure
        || confined_late_stage_branch_drift_with_release_readiness)
        && authoritative_release_readiness_current
        && status.current_release_readiness_state.is_none()
    {
        status.current_release_readiness_state = authoritative_release_readiness_result;
        if status.current_release_readiness_state.as_deref() == Some("ready") {
            status.release_docs_state = DownstreamFreshnessState::Fresh;
        }
    }
    let branch_closure_refresh_missing_current_closure =
        shared_branch_closure_refresh_missing_current_closure(status);
    if repair_follow_up_requires_execution_reentry {
        status.harness_phase = HarnessPhase::Executing;
    } else if repair_follow_up_requires_planning_reentry {
        status.harness_phase = HarnessPhase::PivotRequired;
    } else if repair_follow_up_records_branch_closure
        && persisted_repair_follow_up == Some("advance_late_stage")
    {
        status.harness_phase = if status.current_release_readiness_state.is_some()
            || authoritative_release_readiness_current
        {
            HarnessPhase::FinalReviewPending
        } else {
            HarnessPhase::DocumentReleasePending
        };
    }
    let task_boundary_unresolved_stale =
        pre_reducer_earliest_unresolved_stale_task(context, status).is_some();
    status.review_state_status = derive_public_review_state_status(
        status,
        gate_review,
        gate_finish,
        repair_follow_up_requires_execution_reentry,
        repair_follow_up_records_branch_closure,
        branch_drift_escapes_late_stage_surface,
        task_boundary_unresolved_stale,
    );
    let persisted_branch_reroute_viable = persisted_repair_follow_up == Some("advance_late_stage")
        && status.current_branch_closure_id.is_some();
    let branch_closure_recording_basis_missing = status.review_state_status
        == "missing_current_closure"
        && !branch_reroute_still_valid
        && !branch_closure_refresh_missing_current_closure
        && !persisted_branch_reroute_viable;
    let authoritative_task_closure_baseline_present = event_authority_state.is_some_and(|state| {
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
        && !status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "prior_task_current_closure_missing")
        && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "late_stage_surface_not_declared");
    let late_stage_missing_current_closure_stale_provenance =
        shared_late_stage_missing_current_closure_stale_provenance_present(context, status)?;
    let preserve_canonical_late_stage_harness_phase = branch_closure_recording_basis_missing
        && is_late_stage_phase(status.harness_phase)
        && (late_stage_missing_current_closure_stale_provenance
            || status.latest_authoritative_sequence != INITIAL_AUTHORITATIVE_SEQUENCE
            || persisted_repair_follow_up == Some("advance_late_stage"))
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
        push_status_reason_code_once(status, "prior_task_current_closure_missing");
        push_status_reason_code_once(status, "task_closure_baseline_repair_candidate");
    }
    if late_stage_surface_requires_planning_reentry && status.blocking_task.is_none() {
        status.harness_phase = HarnessPhase::PivotRequired;
    } else if branch_closure_recording_basis_missing
        && !preserve_canonical_late_stage_harness_phase
        && !late_stage_surface_requires_planning_reentry
    {
        status.harness_phase = HarnessPhase::Executing;
    }
    let _negative_result_phase_detail = apply_negative_result_status_overlay(
        context,
        status,
        gate_finish,
        overlay,
        event_authority_state,
    );
    if TargetlessStaleReconcile::status_needs_marker_for_status(status) {
        push_status_reason_code_once(status, TARGETLESS_STALE_RECONCILE_REASON_CODE);
    }
    clear_route_projection_fields(status);
    if (task_scope_overlay_restore_required || branch_closure_recording_basis_missing)
        && !preserve_canonical_late_stage_harness_phase
    {
        status.harness_phase = HarnessPhase::Executing;
    }
    let persisted_branch_reroute_projection = status.execution_started == "yes"
        && !task_scope_overlay_restore_required
        && status.current_branch_closure_id.is_some()
        && status.review_state_status == "missing_current_closure"
        && branch_reroute_still_valid
        && persisted_repair_follow_up == Some("advance_late_stage");
    if persisted_branch_reroute_projection {
        status.harness_phase = HarnessPhase::DocumentReleasePending;
    }
    status.blocking_records = compute_status_blocking_records(context, status, gate_finish)?;

    Ok(())
}

fn clear_route_projection_fields(status: &mut PlanExecutionStatus) {
    status.phase = None;
    status.phase_detail.clear();
    status.recording_context = None;
    status.execution_command_context = None;
    status.execution_reentry_target_source = None;
    status.public_repair_targets.clear();
    status.next_action.clear();
    status.recommended_command = None;
    status.blocking_scope = None;
    status.external_wait_state = None;
    status.blocking_reason_codes.clear();
    status.state_kind.clear();
    status.next_public_action = None;
    status.blockers.clear();
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

pub(crate) fn compute_status_blocking_records(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    gate_finish: &GateResult,
) -> Result<Vec<StatusBlockingRecord>, JsonFailure> {
    let task_structural_records =
        derive_structural_current_task_closure_blocking_records(context, status)?;
    let base_blocking_records = derive_public_blocking_records(status, gate_finish);
    if let Some(structural_records) = task_structural_records
        .as_ref()
        .filter(|records| !records.is_empty())
    {
        if status.review_state_status == "stale_unreviewed" {
            return Ok(merge_status_blocking_records(
                base_blocking_records,
                structural_records.clone(),
            ));
        }
        return Ok(structural_records.clone());
    }
    if let Some(record) = derive_branch_rerecording_blocking_record(context, status)? {
        return Ok(vec![record]);
    }
    let branch_structural_records =
        derive_structural_current_branch_closure_blocking_records(status);
    let blocking_records = if status.review_state_status == "stale_unreviewed" {
        task_structural_records
            .into_iter()
            .chain(branch_structural_records)
            .fold(base_blocking_records, merge_status_blocking_records)
    } else if let Some(structural_records) =
        task_structural_records.filter(|records| !records.is_empty())
    {
        structural_records
    } else if let Some(structural_records) =
        branch_structural_records.filter(|records| !records.is_empty())
    {
        structural_records
    } else {
        base_blocking_records
    };
    Ok(blocking_records)
}

fn authoritative_release_readiness_result_for_current_branch(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    current_branch_closure_id: Option<&str>,
) -> Option<String> {
    shared_release_readiness_result_for_branch_closure(
        authoritative_state,
        current_branch_closure_id,
    )
}

fn derive_branch_rerecording_blocking_record(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> Result<Option<StatusBlockingRecord>, JsonFailure> {
    if !semantic_branch_rerecording_required(context, status) {
        return Ok(None);
    }
    let assessment = branch_closure_rerecording_assessment(context)?;
    let branch_closure_id = status
        .current_branch_closure_id
        .clone()
        .unwrap_or_else(|| String::from("current"));
    let review_state_status = if status.review_state_status == "clean" {
        String::from("missing_current_closure")
    } else {
        status.review_state_status.clone()
    };
    if assessment.supported {
        return Ok(Some(StatusBlockingRecord {
            code: String::from("missing_current_closure"),
            scope_type: String::from("branch"),
            scope_key: branch_closure_id,
            record_type: String::from("branch_closure"),
            record_id: None,
            review_state_status,
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "The current branch closure must be re-recorded before late-stage progression can continue.",
            ),
        }));
    }
    let message = match assessment.unsupported_reason {
        Some(BranchRerecordingUnsupportedReason::MissingTaskClosureBaseline) => String::from(
            "The current branch closure can no longer be safely re-recorded from authoritative current task-closure truth, so review-state repair must reroute execution before late-stage progression can continue.",
        ),
        Some(BranchRerecordingUnsupportedReason::LateStageSurfaceNotDeclared) => String::from(
            "The current branch closure cannot be safely re-recorded because the approved plan does not declare Late-Stage Surface metadata for classifying post-closure drift. Repair review state must reroute through execution reentry before late-stage progression can continue.",
        ),
        Some(BranchRerecordingUnsupportedReason::DriftEscapesLateStageSurface) | None => {
            String::from(
                "The current branch closure cannot be safely re-recorded because branch drift escapes the approved Late-Stage Surface. Repair review state must reroute execution before late-stage progression can continue.",
            )
        }
    };
    Ok(Some(StatusBlockingRecord {
        code: String::from("missing_current_closure"),
        scope_type: String::from("branch"),
        scope_key: branch_closure_id.clone(),
        record_type: String::from("review_state"),
        record_id: Some(branch_closure_id),
        review_state_status,
        required_follow_up: Some(String::from("repair_review_state")),
        message,
    }))
}

fn semantic_branch_rerecording_required(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    let persisted_branch_follow_up = resolve_actionable_repair_follow_up_for_status(
        context,
        status,
        authoritative_state.as_ref(),
    )
    .is_some_and(|record| record.kind.public_token() == "advance_late_stage");
    if status.current_branch_meaningful_drift {
        let release_readiness_already_recorded =
            authoritative_release_readiness_result_for_current_branch(
                authoritative_state.as_ref(),
                status.current_branch_closure_id.as_deref(),
            )
            .is_some();
        return !(persisted_branch_follow_up && release_readiness_already_recorded);
    }
    if status.current_branch_closure_id.is_none() {
        return false;
    }
    persisted_branch_follow_up
}

fn downstream_freshness_state_label(state: DownstreamFreshnessState) -> &'static str {
    match state {
        DownstreamFreshnessState::NotRequired => "not_required",
        DownstreamFreshnessState::Missing => "missing",
        DownstreamFreshnessState::Fresh => "fresh",
        DownstreamFreshnessState::Stale => "stale",
    }
}

fn merge_status_blocking_records(
    mut base_records: Vec<StatusBlockingRecord>,
    extra_records: Vec<StatusBlockingRecord>,
) -> Vec<StatusBlockingRecord> {
    for record in extra_records {
        if !base_records.contains(&record) {
            base_records.push(record);
        }
    }
    base_records
}

pub(crate) fn prerelease_branch_closure_refresh_required(status: &PlanExecutionStatus) -> bool {
    status.harness_phase == HarnessPhase::DocumentReleasePending
        && status.current_release_readiness_state.is_none()
        && status.current_branch_closure_id.is_some()
        && status.current_branch_meaningful_drift
}

pub(crate) fn live_review_state_status_for_reroute_from_status(
    status: &PlanExecutionStatus,
    late_stage_stale_unreviewed: bool,
) -> Option<&'static str> {
    if shared_branch_closure_refresh_missing_current_closure(status) {
        return Some("missing_current_closure");
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

fn derive_public_review_state_status(
    status: &PlanExecutionStatus,
    gate_review: &GateResult,
    gate_finish: &GateResult,
    repair_follow_up_requires_execution_reentry: bool,
    repair_follow_up_records_branch_closure: bool,
    branch_scope_stale_unreviewed: bool,
    task_boundary_unresolved_stale: bool,
) -> String {
    let task_boundary_stale_unreviewed_bridge = task_boundary_unresolved_stale
        && status.blocking_task.is_some()
        && status.blocking_step.is_none()
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && task_closure_baseline_repair_candidate_reason_present(status)
        && status
            .reason_codes
            .iter()
            .any(|code| code == "prior_task_current_closure_missing")
        && !status.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                "prior_task_review_dispatch_missing"
                    | "prior_task_review_dispatch_stale"
                    | "prior_task_review_not_green"
                    | "prior_task_verification_missing"
                    | "prior_task_verification_missing_legacy"
                    | "task_review_not_independent"
                    | "task_review_artifact_malformed"
                    | "task_verification_summary_malformed"
                    | "prior_task_current_closure_stale"
            )
        });
    let task_scope_stale_unreviewed =
        !task_closure_baseline_repair_candidate_reason_present(status)
            && status.reason_codes.iter().any(|code| {
                matches!(
                    code.as_str(),
                    "prior_task_review_dispatch_stale" | "prior_task_current_closure_stale"
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
            || branch_scope_stale_unreviewed;
    let stale_provenance_task_boundary = !status.current_task_closures.is_empty()
        && status.active_task.is_none()
        && status.resume_task.is_none()
        && status.resume_step.is_none()
        && status.current_branch_closure_id.is_some()
        && status.current_branch_reviewed_state_id.is_some()
        && !status.semantic_workspace_tree_id.is_empty()
        && !status.current_branch_meaningful_drift
        && status.current_release_readiness_state.is_none()
        && status.current_final_review_result.is_none()
        && status.current_qa_result.is_none()
        && execution_evidence_fingerprint_mismatch
        && status
            .reason_codes
            .iter()
            .any(|code| code == REASON_CODE_STALE_PROVENANCE)
        && public_late_stage_rederivation_basis_present(status)
        && !branch_scope_stale_unreviewed;
    let task_scope_execution_reentry_active = (status.active_task.is_some()
        || status.resume_task.is_some()
        || status.blocking_step.is_some())
        && status.current_branch_closure_id.is_none()
        && !public_late_stage_rederivation_basis_present(status);
    let late_stage_stale_unreviewed =
        late_stage_stale_signals && !task_scope_execution_reentry_active;
    let prerelease_refresh_missing_current_closure =
        prerelease_branch_closure_refresh_required(status);
    if task_boundary_stale_unreviewed_bridge {
        return String::from("stale_unreviewed");
    }
    if stale_provenance_task_boundary {
        return String::from("stale_unreviewed");
    }
    if repair_follow_up_requires_execution_reentry
        && !prerelease_refresh_missing_current_closure
        && !branch_scope_stale_unreviewed
        && !status
            .reason_codes
            .iter()
            .any(|code| code == REASON_CODE_STALE_PROVENANCE)
    {
        return String::from("clean");
    }
    if status.stale_unreviewed_closures.is_empty()
        && !task_boundary_unresolved_stale
        && !status.reason_codes.iter().any(|code| {
            matches!(
                code.as_str(),
                REASON_CODE_STALE_PROVENANCE | "prior_task_current_closure_stale"
            )
        })
        && (task_scope_structural_review_state_reason(status).is_some()
            || task_scope_overlay_repair_required(status))
    {
        return String::from("clean");
    }
    if resumed_task_stale_unreviewed {
        return String::from("stale_unreviewed");
    }
    if current_branch_closure_structural_review_state_reason(status).is_some() {
        return String::from("missing_current_closure");
    }
    if repair_follow_up_records_branch_closure {
        if status.current_release_readiness_state.is_some() {
            return String::from("clean");
        }
        return String::from("missing_current_closure");
    }
    if prerelease_refresh_missing_current_closure {
        return String::from("missing_current_closure");
    }
    if task_scope_stale_unreviewed {
        return String::from("stale_unreviewed");
    }
    if status.harness_phase == HarnessPhase::DocumentReleasePending
        && status.current_branch_closure_id.is_some()
        && status.current_release_readiness_state.is_none()
        && !status.current_branch_meaningful_drift
        && !branch_scope_stale_unreviewed
    {
        return String::from("clean");
    }
    if late_stage_stale_unreviewed && status.current_branch_closure_id.is_some() {
        return String::from("stale_unreviewed");
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
        return String::from("missing_current_closure");
    }
    if late_stage_stale_unreviewed {
        return String::from("stale_unreviewed");
    }
    String::from("clean")
}

fn status_workflow_phase(status: &PlanExecutionStatus) -> &'static str {
    match status.harness_phase {
        HarnessPhase::DocumentReleasePending => "document_release_pending",
        HarnessPhase::FinalReviewPending => "final_review_pending",
        HarnessPhase::QaPending => "qa_pending",
        HarnessPhase::ReadyForBranchCompletion => "ready_for_branch_completion",
        HarnessPhase::HandoffRequired => "handoff_required",
        HarnessPhase::PivotRequired => "pivot_required",
        HarnessPhase::Executing => "executing",
        _ => "executing",
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
) -> Option<&'static str> {
    if status_late_stage_prerequisite_reroute_active(context, status, gate_finish) {
        return None;
    }
    let task_negative_result_task =
        shared_current_task_negative_result_task(status, overlay, authoritative_state);
    if task_negative_result_task.is_some() {
        push_status_reason_code_once(status, "prior_task_review_not_green");
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
        return None;
    }
    status.harness_phase = HarnessPhase::Executing;
    status.review_state_status = String::from("clean");
    status.stale_unreviewed_closures.clear();
    status.reason_codes.retain(|reason_code| {
        reason_code != TARGETLESS_STALE_RECONCILE_REASON_CODE
            && reason_code != TARGETLESS_STALE_MISSING_AUTHORITY_CODE
    });
    status.blocking_reason_codes.retain(|reason_code| {
        reason_code != TARGETLESS_STALE_RECONCILE_REASON_CODE
            && reason_code != TARGETLESS_STALE_MISSING_AUTHORITY_CODE
    });
    push_status_reason_code_once(status, "negative_result_requires_execution_reentry");
    Some("execution_reentry_required")
}

#[cfg(test)]
fn current_workflow_pivot_record_exists_for_status_decision(
    context: &ExecutionContext,
    reason_codes: &[String],
    qa_requirement: Option<&str>,
) -> bool {
    if context.plan_rel.trim().is_empty() {
        return false;
    }
    let head_sha = match context.current_head_sha() {
        Ok(head_sha) => head_sha,
        Err(_) => return false,
    };
    let qa_requirement_missing_or_invalid =
        !matches!(qa_requirement, Some("required") | Some("not-required"));
    let decision_reason_codes =
        pivot_decision_reason_codes(reason_codes, true, qa_requirement_missing_or_invalid);
    current_workflow_pivot_record_exists(
        &context.runtime.state_dir,
        WorkflowPivotRecordIdentity {
            repo_slug: &context.runtime.repo_slug,
            safe_branch: &context.runtime.safe_branch,
            plan_path: &context.plan_rel,
            branch_name: &context.runtime.branch_name,
            head_sha: &head_sha,
            decision_reason_codes: &decision_reason_codes,
        },
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExecutionReentryCurrentTaskClosureTargets {
    pub(crate) stale_tasks: Vec<u32>,
    pub(crate) structural_tasks: Vec<u32>,
    pub(crate) structural_scope_keys: Vec<String>,
}

pub(crate) fn execution_reentry_current_task_closure_targets_from_stale_tasks(
    context: &ExecutionContext,
    stale_tasks: impl IntoIterator<Item = u32>,
) -> Result<ExecutionReentryCurrentTaskClosureTargets, JsonFailure> {
    let stale_tasks = stale_tasks.into_iter().collect::<BTreeSet<_>>();
    let mut structural_tasks = BTreeSet::new();
    let mut structural_scope_keys = BTreeSet::new();
    for failure in structural_current_task_closure_failures(context)? {
        if let Some(task_number) = failure.task {
            structural_tasks.insert(task_number);
        } else {
            structural_scope_keys.insert(failure.scope_key);
        }
    }

    Ok(ExecutionReentryCurrentTaskClosureTargets {
        stale_tasks: stale_tasks.into_iter().collect(),
        structural_tasks: structural_tasks.into_iter().collect(),
        structural_scope_keys: structural_scope_keys.into_iter().collect(),
    })
}

pub(crate) fn closure_baseline_candidate_task(context: &ExecutionContext) -> Option<u32> {
    if let Some(next_unchecked_task) = context
        .steps
        .iter()
        .find(|step| !step.checked)
        .map(|step| step.task_number)
    {
        return context
            .tasks_by_number
            .keys()
            .copied()
            .filter(|task_number| *task_number < next_unchecked_task)
            .max();
    }
    context.tasks_by_number.keys().copied().max()
}

pub(crate) fn stale_current_task_closure_records(
    context: &ExecutionContext,
) -> Result<Vec<crate::execution::transitions::CurrentTaskClosureRecord>, JsonFailure> {
    Ok(valid_current_task_closure_records(context)?
        .into_iter()
        .filter(|record| {
            matches!(
                task_closure_matches_current_workspace(context, record),
                Ok(false)
            )
        })
        .collect())
}

#[cfg(test)]
fn derive_public_phase_detail(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    gate_review: &GateResult,
    gate_finish: &GateResult,
    review_state_status: &str,
    task_review_dispatch_id: Option<&str>,
    final_review_dispatch_id: Option<&str>,
) -> String {
    if status.harness_phase != HarnessPhase::PivotRequired
        && task_closure_baseline_repair_candidate_reason_present(status)
        && status.blocking_step.is_none()
        && status.blocking_task.is_some_and(|task| {
            task_closure_baseline_repair_candidate_with_stale_target(
                context,
                status,
                task,
                projected_earliest_stale_task_from_status(status),
            )
            .map(|candidate| candidate.is_some())
            .unwrap_or(false)
        })
    {
        return String::from("task_closure_recording_ready");
    }
    if execution_reentry_requires_review_state_repair(Some(context), status) {
        return String::from("execution_reentry_required");
    }
    if task_review_result_pending_task(status, task_review_dispatch_id).is_some() {
        return String::from("task_review_result_pending");
    }
    if review_state_status == "missing_current_closure"
        && status.current_branch_closure_id.is_none()
        && crate::execution::current_truth::worktree_drift_escapes_late_stage_surface(context)
            .unwrap_or(false)
    {
        return String::from("execution_reentry_required");
    }
    if review_state_status == "missing_current_closure" {
        return String::from("branch_closure_recording_required_for_release_readiness");
    }
    if review_state_status == "stale_unreviewed" {
        return String::from("execution_reentry_required");
    }

    match status.harness_phase {
        HarnessPhase::ReadyForBranchCompletion => {
            if status
                .finish_review_gate_pass_branch_closure_id
                .as_ref()
                .zip(status.current_branch_closure_id.as_ref())
                .is_some_and(|(checkpoint, current)| checkpoint == current)
                && gate_finish.allowed
            {
                String::from("finish_completion_gate_ready")
            } else {
                String::from("finish_review_gate_ready")
            }
        }
        HarnessPhase::DocumentReleasePending => {
            document_release_pending_phase_detail(status).to_owned()
        }
        HarnessPhase::FinalReviewPending => {
            if status.current_branch_closure_id.is_none() {
                String::from("branch_closure_recording_required_for_release_readiness")
            } else if status.current_release_readiness_state.as_deref() != Some("ready") {
                if status.current_release_readiness_state.as_deref() == Some("blocked") {
                    String::from("release_blocker_resolution_required")
                } else {
                    String::from("release_readiness_recording_ready")
                }
            } else if final_review_dispatch_id.is_some()
                && shared_final_review_dispatch_still_current(Some(gate_review), Some(gate_finish))
            {
                String::from("final_review_outcome_pending")
            } else {
                String::from("final_review_dispatch_required")
            }
        }
        HarnessPhase::QaPending => {
            if status.current_branch_closure_id.is_none() {
                String::from("branch_closure_recording_required_for_release_readiness")
            } else if shared_normalized_plan_qa_requirement(
                context.plan_document.qa_requirement.as_deref(),
            )
            .as_deref()
                == Some("required")
                && qa_pending_requires_test_plan_refresh(context, Some(gate_finish))
            {
                String::from("test_plan_refresh_required")
            } else {
                String::from("qa_recording_required")
            }
        }
        HarnessPhase::ExecutionPreflight => String::from("execution_preflight_required"),
        HarnessPhase::Executing => {
            if status.active_task.is_some()
                || status.blocking_step.is_some()
                || status.resume_task.is_some()
            {
                String::from("execution_in_progress")
            } else {
                String::from("execution_reentry_required")
            }
        }
        HarnessPhase::PivotRequired => String::from("planning_reentry_required"),
        HarnessPhase::HandoffRequired => String::from("handoff_recording_required"),
        _ => String::from("execution_in_progress"),
    }
}

pub(crate) fn document_release_pending_phase_detail(status: &PlanExecutionStatus) -> &'static str {
    match (
        status.current_release_readiness_state.as_deref(),
        status.release_docs_state,
    ) {
        (Some("blocked"), _) => "release_blocker_resolution_required",
        (_, DownstreamFreshnessState::Fresh) => "final_review_dispatch_required",
        _ => "release_readiness_recording_ready",
    }
}

fn task_closure_baseline_repair_candidate_reason_present(status: &PlanExecutionStatus) -> bool {
    shared_task_closure_baseline_repair_candidate_reason_present(status)
}

#[cfg(test)]
fn derive_public_next_action(
    status: &PlanExecutionStatus,
    phase_detail: &str,
    recommended_command: Option<&str>,
) -> String {
    let kind = match phase_detail {
        "execution_preflight_required" => NextActionKind::Begin,
        "task_review_result_pending" => NextActionKind::WaitForTaskReviewResult,
        "task_closure_recording_ready" => NextActionKind::CloseCurrentTask,
        "finish_completion_gate_ready" | "finish_review_gate_ready" => NextActionKind::FinishBranch,
        "branch_closure_recording_required_for_release_readiness"
        | "release_readiness_recording_ready"
        | "final_review_recording_ready" => NextActionKind::AdvanceLateStage,
        "release_blocker_resolution_required" => NextActionKind::AdvanceLateStage,
        "final_review_dispatch_required" => NextActionKind::RequestFinalReview,
        "final_review_outcome_pending" => NextActionKind::WaitForFinalReviewResult,
        "test_plan_refresh_required" => NextActionKind::RefreshTestPlan,
        "qa_recording_required" => NextActionKind::RunQa,
        "execution_reentry_required" => NextActionKind::Resume,
        "handoff_recording_required" => NextActionKind::Handoff,
        "planning_reentry_required" => NextActionKind::PlanningReentry,
        _ => NextActionKind::Resume,
    };
    public_next_action_text(&NextActionDecision {
        kind,
        phase: status
            .phase
            .clone()
            .unwrap_or_else(|| String::from("executing")),
        phase_detail: String::from(phase_detail),
        review_state_status: status.review_state_status.clone(),
        task_number: status.active_task.or(status.resume_task),
        step_number: status.active_step.or(status.resume_step),
        blocking_task: status.blocking_task,
        blocking_reason_codes: status.reason_codes.clone(),
        recommended_command: recommended_command.map(str::to_owned),
    })
}

pub(crate) struct ExactExecutionCommand {
    pub command_kind: &'static str,
    pub task_number: u32,
    pub step_id: Option<u32>,
    pub recommended_command: String,
}

pub(crate) fn resolve_exact_execution_command(
    status: &PlanExecutionStatus,
    plan_path: &str,
) -> Option<ExactExecutionCommand> {
    let execution_source = recommended_execution_source(status.execution_mode.as_str());
    if let Some((task_number, step_id)) = status.active_task.zip(status.active_step) {
        return Some(ExactExecutionCommand {
            command_kind: "complete",
            task_number,
            step_id: Some(step_id),
            recommended_command: format!(
                "featureforge plan execution complete --plan {plan_path} --task {task_number} --step {step_id} --source {} --claim <claim> --manual-verify-summary <summary> --expect-execution-fingerprint {}",
                execution_source, status.execution_fingerprint
            ),
        });
    }
    if let Some((task_number, step_id)) = status.resume_task.zip(status.resume_step) {
        return Some(ExactExecutionCommand {
            command_kind: "begin",
            task_number,
            step_id: Some(step_id),
            recommended_command: format!(
                "featureforge plan execution begin --plan {plan_path} --task {task_number} --step {step_id} --expect-execution-fingerprint {}",
                status.execution_fingerprint
            ),
        });
    }
    if let Some((task_number, step_id)) = status.blocking_task.zip(status.blocking_step) {
        return Some(ExactExecutionCommand {
            command_kind: "begin",
            task_number,
            step_id: Some(step_id),
            recommended_command: format!(
                "featureforge plan execution begin --plan {plan_path} --task {task_number} --step {step_id} --expect-execution-fingerprint {}",
                status.execution_fingerprint
            ),
        });
    }
    None
}

pub(crate) fn reopen_exact_execution_command_for_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    task_number: u32,
) -> Option<ExactExecutionCommand> {
    let execution_source = recommended_execution_source(status.execution_mode.as_str());
    let step_id = latest_attempted_step_for_task(context, task_number).or_else(|| {
        context
            .steps
            .iter()
            .find(|step| step.task_number == task_number)
            .map(|step| step.step_number)
    })?;
    Some(ExactExecutionCommand {
        command_kind: "reopen",
        task_number,
        step_id: Some(step_id),
        recommended_command: format!(
            "featureforge plan execution reopen --plan {plan_path} --task {task_number} --step {step_id} --source {} --reason <reason> --expect-execution-fingerprint {}",
            execution_source, status.execution_fingerprint
        ),
    })
}

pub(crate) fn recommended_execution_source(execution_mode: &str) -> &str {
    match execution_mode {
        "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {
            execution_mode
        }
        _ => "featureforge:executing-plans",
    }
}

// Bootstrap-only helper used while constructing the reducer input status. After RuntimeState
// exists, consumers must use reducer-projected stale targets instead.
fn pre_reducer_earliest_unresolved_stale_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> Option<u32> {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    let late_stage_missing_current_closure_stale_provenance =
        shared_late_stage_missing_current_closure_stale_provenance_present(context, status)
            .unwrap_or(false);
    let closure_graph = AuthoritativeClosureGraph::from_state(
        authoritative_state.as_ref(),
        &ClosureGraphSignals::from_authoritative_state(
            authoritative_state.as_ref(),
            None,
            status.review_state_status == "stale_unreviewed",
            status.review_state_status == "missing_current_closure"
                && late_stage_missing_current_closure_stale_provenance,
            shared_stale_reason_codes_for_late_stage_projection(
                status,
                std::iter::empty::<&String>(),
            ),
        ),
    );
    closure_graph.earliest_unresolved_stale_task_number()
}

fn completed_plan_missing_current_closure_task(
    context: &ExecutionContext,
    _status: &PlanExecutionStatus,
) -> Option<u32> {
    if context.steps.iter().any(|step| !step.checked) {
        return None;
    }
    let current_task_closures = still_current_task_closure_records(context)
        .ok()?
        .into_iter()
        .map(|closure| closure.task)
        .collect::<BTreeSet<_>>();
    let highest_current_task_closure = current_task_closures.iter().next_back().copied();
    let mut completed_tasks = context
        .steps
        .iter()
        .filter(|step| step.checked)
        .map(|step| step.task_number)
        .collect::<Vec<_>>();
    completed_tasks.sort_unstable();
    completed_tasks.dedup();
    completed_tasks.into_iter().find(|task| {
        !current_task_closures.contains(task)
            && highest_current_task_closure.is_none_or(|current_task| *task > current_task)
    })
}

pub(crate) fn resolve_exact_execution_command_from_context(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
) -> Option<ExactExecutionCommand> {
    let decision = compute_next_action_decision(context, status, plan_path)?;
    exact_execution_command_from_decision(status, &decision, plan_path)
}

#[cfg(test)]
mod exact_execution_command_tests {
    use super::*;
    use crate::test_support::init_committed_test_repo;
    use serde_json::Value;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn unresolved_execution_context() -> (TempDir, ExecutionContext, String) {
        let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/codex-runtime/fixtures/workflow-artifacts");
        let repo_dir = TempDir::new().expect("exact-command temp repo should exist");
        let repo_root = repo_dir.path();
        let plan_rel =
            String::from("docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md");
        let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
        let plan_path = repo_root.join(&plan_rel);
        let spec_path = repo_root.join(spec_rel);

        init_committed_test_repo(
            repo_root,
            "# exact-command-test\n",
            "exact-command unit tests",
        );

        fs::create_dir_all(
            spec_path
                .parent()
                .expect("spec fixture path should have a parent"),
        )
        .expect("spec fixture directory should create");
        fs::create_dir_all(
            plan_path
                .parent()
                .expect("plan fixture path should have a parent"),
        )
        .expect("plan fixture directory should create");
        fs::copy(
            fixture_root.join("specs/2026-03-22-runtime-integration-hardening-design.md"),
            &spec_path,
        )
        .expect("exact-command unit-test spec fixture should copy");
        let plan_source = fs::read_to_string(
            fixture_root.join("plans/2026-03-22-runtime-integration-hardening.md"),
        )
        .expect("exact-command unit-test plan fixture should read")
        .replace(
            "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md",
            spec_rel,
        );
        fs::write(&plan_path, plan_source)
            .expect("exact-command unit-test plan fixture should write");

        let runtime =
            ExecutionRuntime::discover(repo_root).expect("temp repo runtime should discover");
        let context = load_execution_context(&runtime, Path::new(&plan_rel))
            .expect("runtime integration hardening plan should load for exact-command unit tests");
        (repo_dir, context, plan_rel)
    }

    fn closure_baseline_candidate_context() -> (TempDir, ExecutionContext, String) {
        let (repo_dir, mut context, plan_rel) = unresolved_execution_context();
        for step in &mut context.steps {
            if step.task_number == 1 {
                step.checked = true;
            }
        }
        let head_sha = context
            .current_head_sha()
            .expect("closure-baseline candidate fixture should resolve head sha");
        context.evidence.attempts = context
            .steps
            .iter()
            .filter(|step| step.task_number == 1)
            .map(|step| EvidenceAttempt {
                task_number: step.task_number,
                step_number: step.step_number,
                attempt_number: 1,
                status: String::from("Completed"),
                recorded_at: String::from("2026-04-19T00:00:00Z"),
                execution_source: String::from("featureforge:executing-plans"),
                claim: format!(
                    "closure-baseline candidate fixture completed task {} step {}",
                    step.task_number, step.step_number
                ),
                files: Vec::new(),
                file_proofs: Vec::new(),
                verify_command: None,
                verification_summary: String::from("closure-baseline candidate fixture"),
                invalidation_reason: String::new(),
                packet_fingerprint: Some(format!(
                    "packet-fingerprint-task-{}-step-{}",
                    step.task_number, step.step_number
                )),
                head_sha: Some(head_sha.clone()),
                base_sha: Some(head_sha.clone()),
                source_contract_path: None,
                source_contract_fingerprint: None,
                source_evaluation_report_fingerprint: None,
                evaluator_verdict: None,
                failing_criterion_ids: Vec::new(),
                source_handoff_fingerprint: None,
                repo_state_baseline_head_sha: None,
                repo_state_baseline_worktree_fingerprint: None,
            })
            .collect();
        let authoritative_state_path = authoritative_state_path(&context);
        fs::create_dir_all(
            authoritative_state_path
                .parent()
                .expect("authoritative state path should have a parent"),
        )
        .expect("authoritative state directory should create");
        fs::write(
            &authoritative_state_path,
            serde_json::json!({
                "last_strategy_checkpoint_fingerprint": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "run_identity": {
                    "execution_run_id": "run-exact-phase-detail"
                },
                "task_closure_record_history": {
                    "task-closure-1-historical": {
                        "task": 1,
                        "record_status": "historical"
                    }
                }
            })
            .to_string(),
        )
        .expect("authoritative state for closure-baseline candidate should write");
        (repo_dir, context, plan_rel)
    }

    fn late_stage_status_for_review_state_tests() -> PlanExecutionStatus {
        let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
        let mut status =
            status_from_context(&context).expect("status should derive for review-state tests");
        status.execution_started = String::from("yes");
        status.harness_phase = HarnessPhase::FinalReviewPending;
        status.current_branch_closure_id = Some(String::from("branch-closure-1"));
        status
    }

    #[test]
    fn branch_closure_refresh_missing_current_closure_uses_meaningful_drift_not_raw_id_mismatch() {
        let mut status = late_stage_status_for_review_state_tests();
        status.current_branch_reviewed_state_id = Some(String::from("git_tree:baseline"));
        status.workspace_state_id = String::from("git_tree:current");
        status.current_release_readiness_state = None;

        status.current_branch_meaningful_drift = false;
        assert!(
            !shared_branch_closure_refresh_missing_current_closure(&status),
            "raw reviewed/workspace state-id mismatch without meaningful filtered drift must not trigger branch-closure refresh"
        );

        status.current_branch_meaningful_drift = true;
        assert!(
            shared_branch_closure_refresh_missing_current_closure(&status),
            "branch-closure refresh should trigger when meaningful filtered drift is present"
        );
    }

    #[test]
    fn prerelease_branch_closure_refresh_requires_meaningful_drift_signal() {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::DocumentReleasePending;
        status.current_branch_reviewed_state_id = Some(String::from("git_tree:baseline"));
        status.workspace_state_id = String::from("git_tree:current");
        status.current_release_readiness_state = None;

        status.current_branch_meaningful_drift = false;
        assert!(
            !prerelease_branch_closure_refresh_required(&status),
            "DocumentReleasePending must not require branch closure refresh when only raw reviewed/workspace mismatch is present"
        );

        status.current_branch_meaningful_drift = true;
        assert!(
            prerelease_branch_closure_refresh_required(&status),
            "DocumentReleasePending should require branch closure refresh when meaningful filtered drift is present"
        );
    }

    fn gate_result_with_reason(reason_code: &str) -> GateResult {
        GateResult {
            allowed: false,
            action: String::from("blocked"),
            failure_class: String::from("StaleProvenance"),
            reason_codes: vec![reason_code.to_owned()],
            warning_codes: Vec::new(),
            diagnostics: Vec::new(),
            code: None,
            workspace_state_id: None,
            current_branch_reviewed_state_id: None,
            current_branch_closure_id: None,
            finish_review_gate_pass_branch_closure_id: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
        }
    }

    #[test]
    fn resolve_exact_execution_command_from_context_uses_first_unchecked_step_without_markers() {
        let (_repo_dir, context, plan_rel) = unresolved_execution_context();
        let mut status =
            status_from_context(&context).expect("status should derive for exact-command test");
        status.execution_started = String::from("yes");
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from("execution_in_progress");
        status.harness_phase = HarnessPhase::Executing;
        status.execution_mode = String::from("featureforge:executing-plans");

        let resolved =
            resolve_exact_execution_command_from_context(&context, &status, plan_rel.as_str())
                .expect("marker-free started execution should derive the first unchecked step");

        assert_eq!(resolved.command_kind, "begin");
        assert_eq!(resolved.task_number, 1);
        assert_eq!(resolved.step_id, Some(1));
        assert_eq!(
            resolved.recommended_command,
            format!(
                "featureforge plan execution begin --plan {plan_rel} --task 1 --step 1 --expect-execution-fingerprint {}",
                status.execution_fingerprint
            )
        );
    }

    #[test]
    fn resolve_exact_execution_command_from_context_fails_closed_for_malformed_active_marker() {
        let (_repo_dir, context, plan_rel) = unresolved_execution_context();
        let mut status =
            status_from_context(&context).expect("status should derive for exact-command test");
        status.execution_started = String::from("yes");
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from("execution_in_progress");
        status.harness_phase = HarnessPhase::Executing;
        status.active_task = Some(1);
        status.active_step = None;

        assert!(
            resolve_exact_execution_command_from_context(&context, &status, plan_rel.as_str())
                .is_none(),
            "malformed active execution markers must fail closed instead of synthesizing a begin command"
        );
    }

    #[test]
    fn derive_public_review_state_status_treats_not_fresh_late_gate_reasons_as_stale_unreviewed() {
        for reason_code in [
            "release_docs_state_not_fresh",
            "final_review_state_not_fresh",
            "browser_qa_state_not_fresh",
        ] {
            let status = late_stage_status_for_review_state_tests();
            let gate_review = gate_result_with_reason(reason_code);
            let gate_finish = gate_result_with_reason(reason_code);
            assert_eq!(
                derive_public_review_state_status(
                    &status,
                    &gate_review,
                    &gate_finish,
                    false,
                    false,
                    false,
                    false,
                ),
                "stale_unreviewed",
                "late-stage reason code `{reason_code}` must classify as stale_unreviewed",
            );
        }
    }

    #[test]
    fn derive_public_review_state_status_ignores_late_stage_staleness_during_execution_reentry() {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.resume_task = Some(1);
        status.resume_step = Some(1);
        status.current_branch_closure_id = None;

        let gate_review = gate_result_with_reason("release_docs_state_not_fresh");
        let gate_finish = gate_result_with_reason("release_docs_state_not_fresh");

        assert_eq!(
            derive_public_review_state_status(
                &status,
                &gate_review,
                &gate_finish,
                false,
                false,
                false,
                false,
            ),
            "clean",
            "late-stage stale gate reasons must not override task-scope execution reentry truth",
        );
    }

    #[test]
    fn derive_public_review_state_status_marks_resumed_late_stage_reroute_as_stale_unreviewed() {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.resume_task = Some(1);
        status.resume_step = Some(1);
        status.current_branch_closure_id = None;
        status
            .reason_codes
            .push(String::from(REASON_CODE_STALE_PROVENANCE));

        let gate_review = gate_result_with_reason("release_docs_state_not_fresh");
        let gate_finish = gate_result_with_reason("release_docs_state_not_fresh");

        assert_eq!(
            derive_public_review_state_status(
                &status,
                &gate_review,
                &gate_finish,
                false,
                false,
                false,
                false,
            ),
            "stale_unreviewed",
            "a resumed task rerouted out of late-stage phase must require review-state repair",
        );
    }

    #[test]
    fn derive_public_review_state_status_marks_stale_late_stage_truth_even_when_harness_phase_stays_executing()
     {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.current_release_readiness_state = Some(String::from("ready"));

        let gate_review = gate_result_with_reason("release_docs_state_not_fresh");
        let gate_finish = gate_result_with_reason("release_docs_state_not_fresh");

        assert_eq!(
            derive_public_review_state_status(
                &status,
                &gate_review,
                &gate_finish,
                false,
                false,
                false,
                false,
            ),
            "stale_unreviewed",
            "late-stage stale truth must surface from current branch bindings even if harness phase lags in executing",
        );
    }

    #[test]
    fn derive_public_review_state_status_marks_late_stage_stale_provenance_execution_reentry_as_stale_unreviewed()
     {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.blocking_task = Some(1);
        status.current_branch_reviewed_state_id = status.raw_workspace_tree_id.clone();
        status.current_task_closures = vec![PublicReviewStateTaskClosure {
            task: 1,
            closure_record_id: String::from("task-closure-current"),
            reviewed_state_id: String::from("git_tree:current"),
            contract_identity: String::from("task-contract-1"),
            effective_reviewed_surface_paths: vec![String::from("README.md")],
        }];
        status
            .reason_codes
            .push(String::from(REASON_CODE_STALE_PROVENANCE));
        status.release_docs_state = DownstreamFreshnessState::Fresh;
        status.final_review_state = DownstreamFreshnessState::Fresh;
        status.browser_qa_state = DownstreamFreshnessState::Fresh;

        assert_eq!(
            derive_public_review_state_status(
                &status,
                &gate_result_with_reason("plan_fingerprint_mismatch"),
                &gate_result_with_reason("plan_fingerprint_mismatch"),
                true,
                false,
                false,
                false,
            ),
            "stale_unreviewed",
            "late-stage stale provenance routed back to execution must remain stale_unreviewed for shared review-state consumers",
        );
    }

    #[test]
    fn public_late_stage_stale_unreviewed_requires_bound_late_stage_target_ids() {
        let mut status = late_stage_status_for_review_state_tests();
        status.current_branch_closure_id = None;
        status.finish_review_gate_pass_branch_closure_id = None;
        status.current_final_review_branch_closure_id = None;
        status.current_final_review_result = None;
        status.current_qa_branch_closure_id = None;
        status.current_qa_result = None;
        status.final_review_state = DownstreamFreshnessState::Stale;

        let gate_review = gate_result_with_reason("final_review_state_not_fresh");
        let gate_finish = gate_result_with_reason("final_review_state_not_fresh");

        assert!(
            public_late_stage_rederivation_basis_present(&status),
            "fixture should still surface late-stage informational basis even after bound target ids are cleared"
        );
        assert!(
            !shared_public_late_stage_stale_unreviewed(
                &status,
                Some(&gate_review),
                Some(&gate_finish),
            ),
            "late-stage stale routing must not activate when no branch/final-review/qa binding ids remain"
        );
    }

    #[test]
    fn derive_public_review_state_status_ignores_unbound_late_stage_staleness_after_current_task_closure_refresh()
     {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.current_branch_closure_id = None;
        status.finish_review_gate_pass_branch_closure_id = None;
        status.current_final_review_branch_closure_id = None;
        status.current_final_review_result = None;
        status.current_qa_branch_closure_id = None;
        status.current_qa_result = None;
        status.final_review_state = DownstreamFreshnessState::Stale;
        status.current_task_closures = vec![PublicReviewStateTaskClosure {
            task: 1,
            closure_record_id: String::from("task-closure-current"),
            reviewed_state_id: String::from("git_tree:current"),
            contract_identity: String::from("task-contract-1"),
            effective_reviewed_surface_paths: vec![String::from("README.md")],
        }];

        let gate_review = gate_result_with_reason("final_review_state_not_fresh");
        let gate_finish = gate_result_with_reason("final_review_state_not_fresh");

        assert_eq!(
            derive_public_review_state_status(
                &status,
                &gate_review,
                &gate_finish,
                false,
                false,
                false,
                false,
            ),
            "clean",
            "unbound late-stage stale signals must remain informational once the current task closure is refreshed and no late-stage binding ids remain"
        );
    }

    #[test]
    fn task_scope_review_state_repair_reason_prefers_structural_current_closure_failures() {
        let mut status = late_stage_status_for_review_state_tests();
        status.reason_codes = vec![
            String::from("prior_task_current_closure_stale"),
            String::from("prior_task_current_closure_invalid"),
        ];

        assert_eq!(
            task_scope_review_state_repair_reason(&status),
            Some("prior_task_current_closure_invalid")
        );
        assert_eq!(
            task_scope_structural_review_state_reason(&status),
            Some("prior_task_current_closure_invalid")
        );
    }

    #[test]
    fn derive_public_blocking_records_includes_follow_up_for_finish_checkpoint_blocker() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from("finish_completion_gate_ready");
        let gate_finish = gate_result_with_reason("finish_review_gate_checkpoint_missing");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);
        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            "finish_review_gate_checkpoint_missing"
        );
        assert_eq!(
            blocking_records[0].required_follow_up,
            Some(String::from("advance_late_stage")),
            "finish-checkpoint blockers should expose a concrete public follow-up lane",
        );
    }

    #[test]
    fn record_review_dispatch_blocked_output_uses_shared_out_of_phase_contract_when_requery_is_required()
     {
        let (_repo_dir, context, plan_rel) = unresolved_execution_context();
        let args = RecordReviewDispatchArgs {
            plan: PathBuf::from(&plan_rel),
            scope: ReviewDispatchScopeArg::Task,
            task: Some(1),
        };
        let gate = gate_result_with_reason("task_closure_not_recording_ready");

        let output = record_review_dispatch_blocked_output_from_gate(&context, &args, gate);
        let output_json =
            serde_json::to_value(output).expect("record-review-dispatch output should serialize");

        assert_eq!(
            output_json["code"],
            Value::from("out_of_phase_requery_required")
        );
        assert_eq!(
            output_json["recommended_command"],
            Value::from(format!(
                "featureforge workflow operator --plan {}",
                context.plan_rel
            ))
        );
        assert_eq!(
            output_json["rederive_via_workflow_operator"],
            Value::Bool(true)
        );
    }

    #[test]
    fn derive_public_blocking_records_omits_task_review_dispatch_required_lane() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from("task_review_dispatch_required");
        status.blocking_task = Some(2);
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);
        assert!(
            blocking_records.is_empty(),
            "task-review dispatch projection lineage is diagnostic-only and must not create public blockers: {blocking_records:?}"
        );
    }

    #[test]
    fn derive_public_blocking_records_routes_targetless_stale_to_runtime_diagnostic() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("stale_unreviewed");
        status.stale_unreviewed_closures.clear();
        status.current_branch_closure_id = None;
        status.finish_review_gate_pass_branch_closure_id = None;
        status.current_final_review_branch_closure_id = None;
        status.current_final_review_result = None;
        status.current_qa_branch_closure_id = None;
        status.current_qa_result = None;
        status.current_task_closures.clear();
        status.reason_codes.clear();
        status.blocking_task = None;
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);

        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            TARGETLESS_STALE_RECONCILE_REASON_CODE
        );
        assert_eq!(blocking_records[0].scope_type, "runtime");
        assert_eq!(blocking_records[0].scope_key, "targetless_stale_unreviewed");
        assert_eq!(blocking_records[0].record_id, None);
        assert_eq!(blocking_records[0].required_follow_up, None);
    }

    #[test]
    fn derive_public_blocking_records_never_fabricates_current_branch_for_targetless_stale() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("stale_unreviewed");
        status.stale_unreviewed_closures.clear();
        status.current_branch_closure_id = Some(String::from("branch-closure-current"));
        status.current_task_closures.clear();
        status.reason_codes.clear();
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);

        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            TARGETLESS_STALE_RECONCILE_REASON_CODE
        );
        assert_eq!(blocking_records[0].scope_type, "runtime");
        assert_eq!(blocking_records[0].scope_key, "targetless_stale_unreviewed");
        assert_eq!(blocking_records[0].record_id, None);
        assert!(
            blocking_records
                .iter()
                .all(|record| record.scope_key != "current"
                    && record.record_id.as_deref() != Some("current")
                    && record.record_id.as_deref() != Some("branch-closure-current")),
            "targetless stale records must not invent current or branch targets: {blocking_records:?}"
        );
    }

    #[test]
    fn derive_public_blocking_records_targetless_stale_preempts_derived_current_fallback() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("stale_unreviewed");
        status.stale_unreviewed_closures.clear();
        status.current_branch_closure_id = None;
        status.current_task_closures.clear();
        status.reason_codes = vec![String::from("derived_review_state_missing")];
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);

        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            TARGETLESS_STALE_RECONCILE_REASON_CODE
        );
        assert_eq!(blocking_records[0].scope_type, "runtime");
        assert_eq!(blocking_records[0].scope_key, "targetless_stale_unreviewed");
        assert_eq!(blocking_records[0].record_id, None);
        assert_eq!(blocking_records[0].required_follow_up, None);
    }

    #[test]
    fn derive_public_next_action_uses_verification_lane_for_task_review_verification_blockers() {
        let mut status = late_stage_status_for_review_state_tests();
        status.phase_detail = String::from("task_review_result_pending");
        status.blocking_task = Some(1);
        status.reason_codes = vec![String::from("prior_task_verification_missing")];

        let next_action = derive_public_next_action(&status, "task_review_result_pending", None);
        assert_eq!(
            next_action, "run verification",
            "verification-missing task-boundary blockers should route public next_action through the verification lane"
        );

        status.reason_codes = vec![String::from("prior_task_review_not_green")];
        let wait_action = derive_public_next_action(&status, "task_review_result_pending", None);
        assert_eq!(
            wait_action, "wait for external review result",
            "review-pending blockers should remain in the external-review wait lane"
        );
    }

    #[test]
    fn derive_public_phase_detail_allows_close_current_task_when_baseline_candidate_lacks_dispatch()
    {
        let (_repo_dir, context, _plan_rel) = closure_baseline_candidate_context();
        let mut status = status_from_context(&context)
            .expect("status should derive for task-closure baseline candidate phase-detail test");
        status.execution_started = String::from("yes");
        status.harness_phase = HarnessPhase::Executing;
        status.review_state_status = String::from("clean");
        status.current_task_closures.clear();
        status.reason_codes = vec![
            String::from("task_closure_baseline_repair_candidate"),
            String::from("prior_task_review_dispatch_missing"),
        ];
        status.blocking_task = Some(1);
        status.blocking_step = None;

        let gate_review = gate_result_with_reason("irrelevant");
        let gate_finish = gate_result_with_reason("irrelevant");
        let phase_detail = derive_public_phase_detail(
            &context,
            &status,
            &gate_review,
            &gate_finish,
            "clean",
            None,
            None,
        );
        assert_eq!(
            phase_detail, "task_closure_recording_ready",
            "task-closure baseline repair candidates should route directly to closure recording when dispatch lineage can be derived by close-current-task",
        );
        assert_eq!(
            derive_public_next_action(&status, &phase_detail, None),
            "close current task",
            "task-closure baseline repair candidates should keep next_action on close-current-task",
        );
    }

    #[test]
    fn derive_public_phase_detail_keeps_close_current_task_lane_for_verification_pending_baseline_repair()
     {
        let (_repo_dir, context, _plan_rel) = closure_baseline_candidate_context();
        let mut status = status_from_context(&context)
            .expect("status should derive for verification-pending closure routing test");
        status.execution_started = String::from("yes");
        status.harness_phase = HarnessPhase::Executing;
        status.review_state_status = String::from("clean");
        status.blocking_task = Some(1);
        status.blocking_step = None;
        status.current_task_closures.clear();
        status.reason_codes = vec![
            String::from("prior_task_current_closure_missing"),
            String::from("task_closure_baseline_repair_candidate"),
            String::from("prior_task_verification_missing"),
        ];

        let gate_review = gate_result_with_reason("irrelevant");
        let gate_finish = gate_result_with_reason("irrelevant");
        let phase_detail = derive_public_phase_detail(
            &context,
            &status,
            &gate_review,
            &gate_finish,
            "clean",
            Some("dispatch-task-1"),
            None,
        );
        assert_eq!(
            phase_detail, "task_closure_recording_ready",
            "verification-pending missing-baseline routes must stay on close-current-task so the mutation guard can return the exact verification follow-up"
        );
        assert_eq!(
            derive_public_next_action(&status, &phase_detail, None),
            "close current task",
            "verification-pending missing-baseline routes must keep next_action on close-current-task"
        );
    }

    #[test]
    fn derive_public_blocking_records_includes_qa_recording_required_lane() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from("qa_recording_required");
        status.current_branch_closure_id = Some(String::from("branch-closure-qa"));
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);
        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(blocking_records[0].code, "qa_recording_required");
        assert_eq!(blocking_records[0].scope_type, "branch");
        assert_eq!(blocking_records[0].scope_key, "branch-closure-qa");
        assert_eq!(blocking_records[0].record_type, "qa_result");
        assert_eq!(
            blocking_records[0].required_follow_up,
            Some(String::from("advance_late_stage"))
        );
    }

    #[test]
    fn follow_up_override_pivot_status_check_rejects_body_only_decoy_strings() {
        let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
        let head_sha = context
            .current_head_sha()
            .expect("head sha should resolve for pivot override check");
        let reason_codes = vec![String::from("blocked_on_plan_revision")];
        let expected_decision_reason_codes =
            pivot_decision_reason_codes(&reason_codes, true, false).join(", ");
        let artifact_dir = context
            .runtime
            .state_dir
            .join("projects")
            .join(&context.runtime.repo_slug);
        fs::create_dir_all(&artifact_dir).expect("pivot artifact dir should be creatable");
        let artifact_path = artifact_dir.join(format!(
            "test-{}-workflow-pivot-999999999.md",
            context.runtime.safe_branch
        ));
        let decoy_source = format!(
            "# Workflow Pivot Record\n\
**Source Plan:** `docs/featureforge/plans/wrong.md`\n\
**Branch:** wrong-branch\n\
**Repo:** wrong/repo\n\
**Head SHA:** deadbeef\n\
**Decision Reason Codes:** wrong\n\
**Generated By:** featureforge:workflow-record-pivot\n\
\n\
mirror **Source Plan:** `{}`\n\
mirror **Branch:** {}\n\
mirror **Repo:** {}\n\
mirror **Head SHA:** {}\n\
mirror **Decision Reason Codes:** {}\n\
mirror **Generated By:** featureforge:workflow-record-pivot\n",
            context.plan_rel,
            context.runtime.branch_name,
            context.runtime.repo_slug,
            head_sha,
            expected_decision_reason_codes
        );
        fs::write(&artifact_path, decoy_source).expect("decoy pivot artifact should write");

        let matched = current_workflow_pivot_record_exists_for_status_decision(
            &context,
            &reason_codes,
            Some("required"),
        );
        fs::remove_file(&artifact_path).expect("decoy pivot artifact should clean up");

        assert!(
            !matched,
            "pivot follow_up_override clearing must not accept body-only decoy strings"
        );
    }
}

pub(crate) fn require_exact_execution_command(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    plan_path: &str,
    context_label: &str,
) -> Result<ExactExecutionCommand, JsonFailure> {
    resolve_exact_execution_command_from_context(context, status, plan_path).ok_or_else(|| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "{context_label} could not derive the exact execution command for the current execution state."
            ),
        )
    })
}

fn public_exact_execution_command_basis_present(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    status.active_task.is_some()
        || status.active_step.is_some()
        || status.resume_task.is_some()
        || status.resume_step.is_some()
        || status.blocking_task.is_some()
        || status.blocking_step.is_some()
        || status.execution_run_id.is_some()
        || !context.evidence.attempts.is_empty()
        || !status.current_task_closures.is_empty()
        || context.steps.iter().any(|step| !step.checked)
        || status.latest_authoritative_sequence != INITIAL_AUTHORITATIVE_SEQUENCE
        || status
            .active_contract_path
            .as_ref()
            .zip(status.active_contract_fingerprint.as_ref())
            .is_some()
}

fn public_exact_execution_command_required(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> bool {
    let execution_context_present = (status.harness_phase == HarnessPhase::Executing
        || status.harness_phase == HarnessPhase::ExecutionPreflight
        || status.active_task.is_some()
        || status.resume_task.is_some()
        || status.blocking_task.is_some())
        && public_exact_execution_command_basis_present(context, status);
    let exact_execution_route = (status.execution_started == "yes"
        && matches!(status.phase_detail.as_str(), "execution_in_progress"))
        || (status.execution_started != "yes"
            && status.harness_phase == HarnessPhase::ExecutionPreflight
            && matches!(status.phase_detail.as_str(), "execution_preflight_required"));
    execution_context_present
        && exact_execution_route
        && status.review_state_status == "clean"
        && !execution_reentry_requires_review_state_repair(Some(context), status)
}

pub(crate) fn require_public_exact_execution_command(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    if public_exact_execution_command_required(context, status) {
        if status.execution_command_context.is_some() && status.recommended_command.is_some() {
            return Ok(());
        }
        let _ = require_exact_execution_command(context, status, &context.plan_rel, "status")?;
    }
    Ok(())
}

fn derive_public_blocking_records(
    status: &PlanExecutionStatus,
    gate_finish: &GateResult,
) -> Vec<StatusBlockingRecord> {
    if let Some(blocking_record) = TargetlessStaleReconcile::status_blocking_record(status) {
        return vec![blocking_record];
    }

    if status
        .reason_codes
        .iter()
        .any(|reason| reason == "derived_review_state_missing")
    {
        if branch_scope_missing_derived_overlays_require_rerecord(status) {
            return vec![StatusBlockingRecord {
                code: String::from("missing_current_closure"),
                scope_type: String::from("branch"),
                scope_key: String::from("current"),
                record_type: String::from("branch_closure"),
                record_id: None,
                review_state_status: status.review_state_status.clone(),
                required_follow_up: Some(String::from("advance_late_stage")),
                message: String::from(
                    "The active late-stage phase requires a current branch closure, but the authoritative branch-closure record is missing and must be re-recorded before late-stage progression can continue.",
                ),
            }];
        }
        let scope_key = status
            .current_branch_closure_id
            .clone()
            .or_else(|| {
                status
                    .current_task_closures
                    .first()
                    .map(|closure| closure.closure_record_id.clone())
            })
            .unwrap_or_else(|| String::from("current"));
        return vec![StatusBlockingRecord {
            code: String::from("derived_review_state_missing"),
            scope_type: String::from(if scope_key.starts_with("task-") {
                "task"
            } else {
                "branch"
            }),
            scope_key: scope_key.clone(),
            record_type: String::from("review_state"),
            record_id: Some(scope_key),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("repair_review_state")),
            message: String::from(
                "Derived review-state overlays or milestone indexes are missing and must be repaired before late-stage progression can continue.",
            ),
        }];
    }

    if status.review_state_status == "stale_unreviewed" {
        if status.stale_unreviewed_closures.is_empty() {
            return TargetlessStaleReconcile::status_blocking_record(status)
                .into_iter()
                .collect();
        }
        let late_stage_surface_not_declared = status
            .reason_codes
            .iter()
            .any(|reason| reason == "late_stage_surface_not_declared");
        let code = if late_stage_surface_not_declared {
            String::from("late_stage_surface_not_declared")
        } else {
            String::from("stale_unreviewed")
        };
        let message = if late_stage_surface_not_declared {
            String::from(
                "The current reviewed state is stale, and the approved plan does not declare Late-Stage Surface metadata to classify post-closure drift as trusted late-stage-only. Repair review state must reroute through execution reentry.",
            )
        } else {
            String::from(
                "The current reviewed state is stale because later workspace changes landed after the latest reviewed closure.",
            )
        };
        return status
            .stale_unreviewed_closures
            .iter()
            .cloned()
            .map(|scope_key| StatusBlockingRecord {
                code: code.clone(),
                scope_type: String::from(if scope_key.starts_with("task-") {
                    "task"
                } else {
                    "branch"
                }),
                scope_key: scope_key.clone(),
                record_type: String::from("review_state"),
                record_id: Some(scope_key),
                review_state_status: status.review_state_status.clone(),
                required_follow_up: Some(String::from("repair_review_state")),
                message: message.clone(),
            })
            .collect();
    }

    if let Some(reason_code) = task_scope_structural_review_state_reason(status) {
        let task_number = status
            .blocking_task
            .or_else(|| {
                status
                    .current_task_closures
                    .first()
                    .map(|closure| closure.task)
            })
            .unwrap_or_default();
        let message = match reason_code {
            "prior_task_current_closure_invalid" => format!(
                "Task {task_number} is blocked because the current task-closure provenance is not valid for the active approved plan."
            ),
            "prior_task_current_closure_reviewed_state_malformed" => format!(
                "Task {task_number} is blocked because the current task-closure reviewed-state identity is malformed."
            ),
            _ => format!(
                "Task {task_number} is blocked because the current task-closure review state requires repair before execution can continue."
            ),
        };
        return vec![StatusBlockingRecord {
            code: reason_code.to_owned(),
            scope_type: String::from("task"),
            scope_key: format!("task-{task_number}"),
            record_type: String::from("review_state"),
            record_id: status
                .current_task_closures
                .iter()
                .find(|closure| closure.task == task_number)
                .map(|closure| closure.closure_record_id.clone()),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("repair_review_state")),
            message,
        }];
    }

    if status.current_branch_closure_id.is_some()
        && let Some(reason_code) = current_branch_closure_structural_review_state_reason(status)
    {
        let branch_closure_id = status
            .current_branch_closure_id
            .clone()
            .unwrap_or_else(|| String::from("current"));
        let message = match reason_code {
            "current_branch_closure_reviewed_state_malformed" => format!(
                "Branch closure {branch_closure_id} is blocked because the current branch-closure reviewed-state identity is malformed."
            ),
            _ => format!(
                "Branch closure {branch_closure_id} requires review-state repair before late-stage progression can continue."
            ),
        };
        return vec![StatusBlockingRecord {
            code: reason_code.to_owned(),
            scope_type: String::from("branch"),
            scope_key: branch_closure_id.clone(),
            record_type: String::from("review_state"),
            record_id: Some(branch_closure_id),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("repair_review_state")),
            message,
        }];
    }

    if status.review_state_status == "missing_current_closure" {
        if execution_reentry_requires_review_state_repair(None, status)
            || status.reason_codes.iter().any(|reason| {
                matches!(
                    reason.as_str(),
                    "late_stage_surface_not_declared" | "branch_drift_escapes_late_stage_surface"
                )
            })
        {
            let scope_key = status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current"));
            let late_stage_surface_not_declared = status
                .reason_codes
                .iter()
                .any(|reason| reason == "late_stage_surface_not_declared");
            return vec![StatusBlockingRecord {
                code: String::from("missing_current_closure"),
                scope_type: String::from("branch"),
                scope_key: scope_key.clone(),
                record_type: String::from("review_state"),
                record_id: Some(scope_key),
                review_state_status: status.review_state_status.clone(),
                required_follow_up: Some(String::from("repair_review_state")),
                message: if late_stage_surface_not_declared {
                    String::from(
                        "The current branch closure cannot be safely re-recorded because the approved plan does not declare Late-Stage Surface metadata for classifying post-closure drift. Repair review state must reroute through execution reentry before late-stage progression can continue.",
                    )
                } else {
                    String::from(
                        "The current branch closure can no longer be safely re-recorded from authoritative current task-closure truth, so review-state repair must reroute execution before late-stage progression can continue.",
                    )
                },
            }];
        }
        return vec![StatusBlockingRecord {
            code: String::from("missing_current_closure"),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("branch_closure"),
            record_id: None,
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "The current branch closure must be recorded before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == "release_blocker_resolution_required" {
        return vec![StatusBlockingRecord {
            code: String::from("release_blocker_resolution_required"),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("release_readiness"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("resolve_release_blocker")),
            message: String::from(
                "The latest release-readiness result for the current branch closure is blocked and must be resolved before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == "release_readiness_recording_ready" {
        return vec![StatusBlockingRecord {
            code: String::from("release_readiness_recording_ready"),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("release_readiness"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "A current release-readiness result for the active branch closure is required before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == "final_review_dispatch_required" {
        return vec![StatusBlockingRecord {
            code: String::from("final_review_dispatch_required"),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("final_review_dispatch"),
            record_id: None,
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("request_external_review")),
            message: String::from(
                "A fresh external final review is required before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == "qa_recording_required" {
        return vec![StatusBlockingRecord {
            code: String::from("qa_recording_required"),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("qa_result"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "A current QA result for the active branch closure is required before late-stage progression can continue.",
            ),
        }];
    }

    if status.phase_detail == "finish_completion_gate_ready" && !gate_finish.allowed {
        return vec![StatusBlockingRecord {
            code: String::from("finish_review_gate_checkpoint_missing"),
            scope_type: String::from("branch"),
            scope_key: status
                .current_branch_closure_id
                .clone()
                .unwrap_or_else(|| String::from("current")),
            record_type: String::from("finish_review_gate_pass_checkpoint"),
            record_id: status.current_branch_closure_id.clone(),
            review_state_status: status.review_state_status.clone(),
            required_follow_up: Some(String::from("advance_late_stage")),
            message: String::from(
                "The current branch closure still needs a fresh gate-review checkpoint before branch completion can proceed.",
            ),
        }];
    }

    Vec::new()
}

fn branch_scope_missing_derived_overlays_require_rerecord(status: &PlanExecutionStatus) -> bool {
    status.current_branch_closure_id.is_none()
        && status.review_state_status == "missing_current_closure"
        && status.phase_detail == "branch_closure_recording_required_for_release_readiness"
}

fn derive_structural_current_task_closure_blocking_records(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
) -> Result<Option<Vec<StatusBlockingRecord>>, JsonFailure> {
    if task_scope_structural_review_state_reason(status).is_none() {
        return Ok(None);
    }
    let structural_records = structural_current_task_closure_failures(context)?
        .into_iter()
        .filter_map(|failure| {
            let message = match failure.reason_code.as_str() {
                "prior_task_current_closure_invalid" => match failure.task {
                    Some(task_number) => format!(
                        "Task {task_number} is blocked because the current task-closure provenance is not valid for the active approved plan."
                    ),
                    None => format!(
                        "Current task-closure entry `{}` is blocked because its authoritative provenance is not valid for the active approved plan.",
                        failure.scope_key
                    ),
                },
                "prior_task_current_closure_reviewed_state_malformed" => {
                    let task_number = failure.task?;
                    format!(
                        "Task {task_number} is blocked because the current task-closure reviewed-state identity is malformed."
                    )
                }
                _ => return None,
            };
            Some(StatusBlockingRecord {
                code: failure.reason_code,
                scope_type: String::from("task"),
                scope_key: failure.scope_key,
                record_type: String::from("review_state"),
                record_id: failure.closure_record_id,
                review_state_status: status.review_state_status.clone(),
                required_follow_up: Some(String::from("repair_review_state")),
                message,
            })
        })
        .collect::<Vec<_>>();
    if !structural_records.is_empty() {
        return Ok(Some(structural_records));
    }
    Ok(None)
}

fn derive_structural_current_branch_closure_blocking_records(
    status: &PlanExecutionStatus,
) -> Option<Vec<StatusBlockingRecord>> {
    let reason_code = current_branch_closure_structural_review_state_reason(status)?;
    let branch_closure_id = status.current_branch_closure_id.clone()?;
    let message = match reason_code {
        "current_branch_closure_reviewed_state_malformed" => format!(
            "Branch closure {branch_closure_id} is blocked because the current branch-closure reviewed-state identity is malformed."
        ),
        _ => format!(
            "Branch closure {branch_closure_id} requires review-state repair before late-stage progression can continue."
        ),
    };
    Some(vec![StatusBlockingRecord {
        code: reason_code.to_owned(),
        scope_type: String::from("branch"),
        scope_key: branch_closure_id.clone(),
        record_type: String::from("review_state"),
        record_id: Some(branch_closure_id),
        review_state_status: status.review_state_status.clone(),
        required_follow_up: Some(String::from("repair_review_state")),
        message,
    }])
}

fn status_workspace_state_id(context: &ExecutionContext) -> Result<String, JsonFailure> {
    Ok(semantic_workspace_snapshot(context)?.semantic_workspace_tree_id)
}

pub(crate) fn validated_current_branch_closure_identity(
    context: &ExecutionContext,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    validated_current_branch_closure_identity_from_authoritative_state(
        context,
        authoritative_state.as_ref(),
    )
}

fn validated_current_branch_closure_identity_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let state = authoritative_state?;
    let identity = state.bound_current_branch_closure_identity()?;
    let record = state.branch_closure_record(&identity.branch_closure_id)?;
    let current_base_branch = context.current_release_base_branch()?;
    let semantic_contract_identity = branch_definition_identity_for_context(context);
    let contract_identity_matches = identity.contract_identity == record.contract_identity
        && normalized_branch_contract_identity_for_current_truth(
            context,
            &current_base_branch,
            &identity.contract_identity,
        )
        .is_some_and(|normalized| normalized == semantic_contract_identity);
    let late_stage_surface =
        if record.provenance_basis == "task_closure_lineage_plus_late_stage_surface_exemption" {
            normalized_late_stage_surface(&context.plan_source).ok()
        } else {
            None
        };
    let expected_source_task_closure_ids = shared_branch_source_task_closure_ids(
        context,
        &still_current_task_closure_records_from_authoritative_state(context, state).ok()?,
        late_stage_surface.as_deref(),
    );
    let mut normalized_record_source_task_closure_ids = record.source_task_closure_ids.clone();
    normalized_record_source_task_closure_ids.sort();
    normalized_record_source_task_closure_ids.dedup();
    (record.source_plan_path == context.plan_rel
        && record.source_plan_revision == context.plan_document.plan_revision
        && record.repo_slug == context.runtime.repo_slug
        && record.branch_name == context.runtime.branch_name
        && record.base_branch == current_base_branch
        && contract_identity_matches
        && record.source_task_closure_ids.len() == normalized_record_source_task_closure_ids.len()
        && normalized_record_source_task_closure_ids == expected_source_task_closure_ids
        && branch_closure_record_matches_plan_exemption(context, &record))
    .then_some(identity)
}

fn normalized_branch_contract_identity_for_current_truth(
    context: &ExecutionContext,
    _base_branch: &str,
    observed_identity: &str,
) -> Option<String> {
    let semantic = branch_definition_identity_for_context(context);
    (observed_identity == semantic).then_some(semantic)
}

pub(crate) fn usable_current_branch_closure_identity(
    context: &ExecutionContext,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    usable_current_branch_closure_identity_from_authoritative_state(
        context,
        authoritative_state.as_ref(),
    )
}

pub(crate) fn usable_current_branch_closure_identity_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let identity = validated_current_branch_closure_identity_from_authoritative_state(
        context,
        authoritative_state,
    )?;
    resolve_branch_closure_reviewed_tree_sha(
        &context.runtime.repo_root,
        &identity.branch_closure_id,
        &identity.reviewed_state_id,
    )
    .ok()?;
    Some(identity)
}

pub(crate) fn branch_closure_record_matches_plan_exemption(
    context: &ExecutionContext,
    record: &crate::execution::transitions::BranchClosureRecord,
) -> bool {
    if record.provenance_basis != "task_closure_lineage_plus_late_stage_surface_exemption"
        || !record.source_task_closure_ids.is_empty()
    {
        return true;
    }
    let Ok(late_stage_surface) = normalized_late_stage_surface(&context.plan_source) else {
        return false;
    };
    !late_stage_surface.is_empty()
        && parse_late_stage_surface_only_branch_surface(&record._effective_reviewed_branch_surface)
            .is_some_and(|recorded_surface| {
                !recorded_surface.is_empty()
                    && recorded_surface
                        .iter()
                        .all(|entry| path_matches_late_stage_surface(entry, &late_stage_surface))
            })
}

fn task_contract_identity_matches_expected(
    context: &ExecutionContext,
    task_number: u32,
    observed_identity: &str,
) -> Result<bool, JsonFailure> {
    Ok(normalized_task_contract_identity_for_current_truth(
        context,
        task_number,
        observed_identity,
    )?
    .is_some())
}

fn normalized_task_contract_identity_for_current_truth(
    context: &ExecutionContext,
    task_number: u32,
    observed_identity: &str,
) -> Result<Option<String>, JsonFailure> {
    let Some(semantic) = task_definition_identity_for_task(context, task_number)? else {
        return Ok(None);
    };
    Ok((observed_identity == semantic).then_some(semantic))
}

fn current_branch_reviewed_state_id(context: &ExecutionContext) -> Option<String> {
    let identity = usable_current_branch_closure_identity(context)?;
    Some(identity.reviewed_state_id)
}

fn current_branch_closure_id(context: &ExecutionContext) -> Option<String> {
    validated_current_branch_closure_identity(context).map(|identity| identity.branch_closure_id)
}

fn finish_review_gate_pass_branch_closure_id(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    Ok(shared_current_late_stage_branch_bindings(
        load_authoritative_transition_state(context)?.as_ref(),
        current_branch_closure_id(context).as_deref(),
        current_branch_reviewed_state_id(context).as_deref(),
    )
    .finish_review_gate_pass_branch_closure_id)
}

fn push_status_reason_code_once(status: &mut PlanExecutionStatus, reason_code: &str) {
    if !status
        .reason_codes
        .iter()
        .any(|existing| existing == reason_code)
    {
        status.reason_codes.push(reason_code.to_owned());
    }
}

fn push_status_warning_code_once(status: &mut PlanExecutionStatus, warning_code: &str) {
    if !status
        .warning_codes
        .iter()
        .any(|existing| existing == warning_code)
    {
        status.warning_codes.push(warning_code.to_owned());
    }
}

pub(crate) fn task_scope_review_state_repair_reason(status: &PlanExecutionStatus) -> Option<&str> {
    status
        .reason_codes
        .iter()
        .map(String::as_str)
        .find(|code| {
            matches!(
                *code,
                "prior_task_current_closure_invalid"
                    | "prior_task_current_closure_reviewed_state_malformed"
            )
        })
        .or_else(|| {
            status
                .reason_codes
                .iter()
                .map(String::as_str)
                .find(|code| matches!(*code, "prior_task_current_closure_stale"))
        })
}

pub(crate) fn current_branch_closure_structural_review_state_reason(
    status: &PlanExecutionStatus,
) -> Option<&str> {
    status
        .reason_codes
        .iter()
        .map(String::as_str)
        .find(|code| matches!(*code, "current_branch_closure_reviewed_state_malformed"))
}

pub(crate) fn task_scope_structural_review_state_reason(
    status: &PlanExecutionStatus,
) -> Option<&str> {
    task_scope_review_state_repair_reason(status).filter(|reason_code| {
        matches!(
            *reason_code,
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
        )
    })
}

pub(crate) fn execution_reentry_requires_review_state_repair(
    context: Option<&ExecutionContext>,
    status: &PlanExecutionStatus,
) -> bool {
    let task_scope_repair_required = task_scope_overlay_repair_required(status)
        || task_scope_structural_review_state_reason(status).is_some()
        || (matches!(
            status.harness_phase,
            HarnessPhase::Executing | HarnessPhase::ExecutionPreflight
        ) && status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "prior_task_current_closure_stale"));
    if task_closure_baseline_repair_candidate_reason_present(status) && !task_scope_repair_required
    {
        if status.review_state_status == "stale_unreviewed" {
            if let Some(context) = context
                && let Some(task) = status.blocking_task
                && stale_unreviewed_allows_task_closure_baseline_bridge(context, status, task)
                    .unwrap_or(false)
            {
                return false;
            }
        } else {
            return false;
        }
    }
    execution_reentry_repair_projection_active(status)
        || (status
            .reason_codes
            .iter()
            .any(|reason_code| reason_code == "derived_review_state_missing")
            && (status.current_branch_closure_id.is_some()
                || task_scope_overlay_repair_required(status)))
        || (status.current_branch_closure_id.is_some()
            && current_branch_closure_structural_review_state_reason(status).is_some())
        || task_scope_repair_required
}

fn execution_reentry_repair_projection_active(status: &PlanExecutionStatus) -> bool {
    status.phase_detail == "execution_reentry_required"
        && (status.review_state_status == "stale_unreviewed"
            || status.reason_codes.iter().any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    "derived_review_state_missing"
                        | "prior_task_current_closure_invalid"
                        | "prior_task_current_closure_reviewed_state_malformed"
                        | "current_branch_closure_reviewed_state_malformed"
                )
            }))
}

fn task_scope_overlay_repair_required(status: &PlanExecutionStatus) -> bool {
    status.harness_phase == HarnessPhase::Executing
        && status.reason_codes.iter().any(|reason_code| {
            reason_code == "current_task_closure_overlay_restore_required"
                || reason_code == "task_closure_negative_result_overlay_restore_required"
        })
        && status.current_branch_closure_id.is_none()
}

fn is_late_stage_phase(phase: HarnessPhase) -> bool {
    matches!(
        phase,
        HarnessPhase::FinalReviewPending
            | HarnessPhase::QaPending
            | HarnessPhase::DocumentReleasePending
            | HarnessPhase::ReadyForBranchCompletion
    )
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

pub(crate) fn final_review_dispatch_still_current_for_gates(
    gate_review: Option<&GateResult>,
    gate_finish: Option<&GateResult>,
) -> bool {
    shared_final_review_dispatch_still_current(gate_review, gate_finish)
}

fn status_qa_blocked(gate_finish: &GateResult) -> bool {
    shared_late_stage_qa_blocked(Some(gate_finish))
}

fn parse_harness_phase(value: &str) -> Option<HarnessPhase> {
    match value {
        "implementation_handoff" => Some(HarnessPhase::ImplementationHandoff),
        "execution_preflight" => Some(HarnessPhase::ExecutionPreflight),
        "contract_drafting" => Some(HarnessPhase::ContractDrafting),
        "contract_pending_approval" => Some(HarnessPhase::ContractPendingApproval),
        "contract_approved" => Some(HarnessPhase::ContractApproved),
        "executing" => Some(HarnessPhase::Executing),
        "evaluating" => Some(HarnessPhase::Evaluating),
        "repairing" => Some(HarnessPhase::Repairing),
        "pivot_required" => Some(HarnessPhase::PivotRequired),
        "handoff_required" => Some(HarnessPhase::HandoffRequired),
        "final_review_pending" => Some(HarnessPhase::FinalReviewPending),
        "qa_pending" => Some(HarnessPhase::QaPending),
        "document_release_pending" => Some(HarnessPhase::DocumentReleasePending),
        "ready_for_branch_completion" => Some(HarnessPhase::ReadyForBranchCompletion),
        _ => None,
    }
}

fn parse_aggregate_evaluation_state(value: &str) -> Option<AggregateEvaluationState> {
    match value {
        "pass" => Some(AggregateEvaluationState::Pass),
        "fail" => Some(AggregateEvaluationState::Fail),
        "blocked" => Some(AggregateEvaluationState::Blocked),
        "pending" => Some(AggregateEvaluationState::Pending),
        _ => None,
    }
}

fn parse_downstream_freshness_state(value: &str) -> Option<DownstreamFreshnessState> {
    match value {
        "not_required" => Some(DownstreamFreshnessState::NotRequired),
        "missing" => Some(DownstreamFreshnessState::Missing),
        "fresh" => Some(DownstreamFreshnessState::Fresh),
        "stale" => Some(DownstreamFreshnessState::Stale),
        _ => None,
    }
}

fn parse_overlay_active_contract_fields(
    active_contract_path: Option<&str>,
    active_contract_fingerprint: Option<&str>,
    state_path: &Path,
) -> Result<(Option<String>, Option<String>), JsonFailure> {
    let active_contract_path =
        normalize_optional_overlay_value(active_contract_path).map(str::to_owned);
    let active_contract_fingerprint =
        normalize_optional_overlay_value(active_contract_fingerprint).map(str::to_owned);

    let (Some(active_contract_path), Some(active_contract_fingerprint)) = (
        active_contract_path.clone(),
        active_contract_fingerprint.clone(),
    ) else {
        if active_contract_path.is_some() || active_contract_fingerprint.is_some() {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative harness state must set active_contract_path and active_contract_fingerprint together in {}.",
                    state_path.display()
                ),
            ));
        }
        return Ok((None, None));
    };

    if active_contract_path.contains('/') || active_contract_path.contains('\\') {
        return Err(non_authoritative_overlay_field(
            state_path,
            "active_contract_path",
            &active_contract_path,
            "must be a single authoritative artifact file name",
        ));
    }

    let expected_file = format!("contract-{active_contract_fingerprint}.md");
    if active_contract_path != expected_file {
        let expectation = format!("must match `{expected_file}`");
        return Err(malformed_overlay_field(
            state_path,
            "active_contract_path",
            &active_contract_path,
            &expectation,
        ));
    }

    Ok((
        Some(active_contract_path),
        Some(active_contract_fingerprint),
    ))
}

fn malformed_overlay_field(
    state_path: &Path,
    field_name: &str,
    value: &str,
    expectation: &str,
) -> JsonFailure {
    JsonFailure::new(
        FailureClass::MalformedExecutionState,
        format!(
            "Authoritative harness state field `{field_name}` is malformed in {}: `{value}` ({expectation}).",
            state_path.display()
        ),
    )
}

fn non_authoritative_overlay_field(
    state_path: &Path,
    field_name: &str,
    value: &str,
    expectation: &str,
) -> JsonFailure {
    JsonFailure::new(
        FailureClass::NonAuthoritativeArtifact,
        format!(
            "Authoritative harness state field `{field_name}` is non-authoritative in {}: `{value}` ({expectation}).",
            state_path.display()
        ),
    )
}

fn parse_evaluator_kinds(
    values: &[String],
    field_name: &str,
    state_path: &Path,
) -> Result<Vec<EvaluatorKind>, JsonFailure> {
    values
        .iter()
        .map(|value| {
            let value = value.trim();
            parse_evaluator_kind(value).ok_or_else(|| {
                malformed_overlay_field(
                    state_path,
                    field_name,
                    value,
                    "must contain only spec_compliance or code_quality",
                )
            })
        })
        .collect()
}

fn parse_evaluator_kind(value: &str) -> Option<EvaluatorKind> {
    match value {
        "spec_compliance" => Some(EvaluatorKind::SpecCompliance),
        "code_quality" => Some(EvaluatorKind::CodeQuality),
        _ => None,
    }
}

fn parse_evaluation_verdict(value: &str) -> Option<EvaluationVerdict> {
    match value {
        "pass" => Some(EvaluationVerdict::Pass),
        "fail" => Some(EvaluationVerdict::Fail),
        "blocked" => Some(EvaluationVerdict::Blocked),
        _ => None,
    }
}

fn parse_optional_evaluator_kind(
    value: Option<&str>,
    field_name: &str,
    state_path: &Path,
) -> Result<Option<EvaluatorKind>, JsonFailure> {
    let Some(value) = normalize_optional_overlay_value(value) else {
        return Ok(None);
    };
    parse_evaluator_kind(value).map(Some).ok_or_else(|| {
        malformed_overlay_field(
            state_path,
            field_name,
            value,
            "must be spec_compliance or code_quality",
        )
    })
}

fn parse_optional_evaluation_verdict(
    value: Option<&str>,
    field_name: &str,
    state_path: &Path,
) -> Result<Option<EvaluationVerdict>, JsonFailure> {
    let Some(value) = normalize_optional_overlay_value(value) else {
        return Ok(None);
    };
    parse_evaluation_verdict(value).map(Some).ok_or_else(|| {
        malformed_overlay_field(
            state_path,
            field_name,
            value,
            "must be pass, fail, or blocked",
        )
    })
}

fn parse_optional_downstream_freshness_state(
    value: Option<&str>,
    field_name: &str,
    state_path: &Path,
) -> Result<Option<DownstreamFreshnessState>, JsonFailure> {
    let Some(value) = normalize_optional_overlay_value(value) else {
        return Ok(None);
    };
    parse_downstream_freshness_state(value)
        .map(Some)
        .ok_or_else(|| {
            malformed_overlay_field(
                state_path,
                field_name,
                value,
                "must be not_required, missing, fresh, or stale",
            )
        })
}

fn parse_reason_codes(
    values: &[String],
    field_name: &str,
    state_path: &Path,
) -> Result<Vec<String>, JsonFailure> {
    values
        .iter()
        .map(|value| {
            let value = value.trim();
            if value.is_empty() {
                return Err(malformed_overlay_field(
                    state_path,
                    field_name,
                    "<empty>",
                    "must contain non-empty strings",
                ));
            }
            Ok(value.to_owned())
        })
        .collect()
}

pub fn require_preflight_acceptance(context: &ExecutionContext) -> Result<(), JsonFailure> {
    crate::execution::topology::require_preflight_acceptance(context)
}

enum PublicIntentPreflightReadiness {
    AlreadyReady,
    AllowedNeedsPersistence,
}

fn public_intent_preflight_readiness(
    context: &ExecutionContext,
    command_name: &str,
) -> Result<PublicIntentPreflightReadiness, JsonFailure> {
    if authoritative_run_identity_present(context)?
        || preflight_acceptance_for_context(context)?.is_some()
    {
        return Ok(PublicIntentPreflightReadiness::AlreadyReady);
    }

    let read_scope = load_execution_read_scope_for_mutation(
        &context.runtime,
        Path::new(&context.plan_rel),
        true,
    )?;
    let reduced_state = crate::execution::reducer::reduce_execution_read_scope(&read_scope)?;
    let Some(gate) = reduced_state
        .gate_snapshot
        .preflight
        .or(reduced_state.preflight)
    else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "{command_name} is blocked because the reduced runtime state did not expose an execution preflight gate. Run {} to recover a public route.",
                repair_review_state_preflight_recovery_command(context)
            ),
        ));
    };
    if !gate.allowed {
        return Err(JsonFailure::new(
            failure_class_for_gate_result(&gate),
            preflight_gate_failure_message(command_name, &gate),
        ));
    }

    Ok(PublicIntentPreflightReadiness::AllowedNeedsPersistence)
}

pub fn validate_public_intent_preflight_allowed(
    context: &ExecutionContext,
    command_name: &str,
) -> Result<(), JsonFailure> {
    public_intent_preflight_readiness(context, command_name).map(|_| ())
}

pub fn public_intent_preflight_persistence_required(
    context: &ExecutionContext,
    command_name: &str,
) -> Result<bool, JsonFailure> {
    Ok(matches!(
        public_intent_preflight_readiness(context, command_name)?,
        PublicIntentPreflightReadiness::AllowedNeedsPersistence
    ))
}

fn ensure_public_intent_preflight_bootstrap_is_safe(
    context: &ExecutionContext,
    command_name: &str,
) -> Result<(), JsonFailure> {
    if command_name == "begin" {
        return Ok(());
    }
    if let Some(step) = context.steps.iter().find(|step| {
        matches!(
            step.note_state,
            Some(NoteState::Active | NoteState::Blocked | NoteState::Interrupted)
        )
    }) {
        let note_state = step.note_state.map(NoteState::as_str).unwrap_or("unknown");
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!(
                "{command_name} cannot bootstrap execution preflight while Task {} Step {} is {note_state}. Run {} to recover a public route.",
                step.task_number,
                step.step_number,
                repair_review_state_preflight_recovery_command(context)
            ),
        ));
    }
    Ok(())
}

fn repair_review_state_preflight_recovery_command(context: &ExecutionContext) -> String {
    format!(
        "featureforge plan execution repair-review-state --plan {}",
        context.plan_rel
    )
}

fn persist_allowed_public_intent_preflight(
    context: &ExecutionContext,
    command_name: &str,
    use_existing_authority: bool,
) -> Result<(), JsonFailure> {
    if authoritative_run_identity_present(context)?
        || preflight_acceptance_for_context(context)?.is_some()
    {
        return Ok(());
    }
    ensure_public_intent_preflight_bootstrap_is_safe(context, command_name)?;
    let acceptance = persist_preflight_acceptance(context)?;
    let run_identity = RunIdentitySnapshot {
        execution_run_id: acceptance.execution_run_id.clone(),
        source_plan_path: context.plan_rel.clone(),
        source_plan_revision: context.plan_document.plan_revision,
    };
    if use_existing_authority {
        ensure_preflight_authoritative_bootstrap_with_existing_authority(
            &context.runtime,
            run_identity,
            acceptance.chunk_id,
        )
    } else {
        ensure_preflight_authoritative_bootstrap(
            &context.runtime,
            run_identity,
            acceptance.chunk_id,
        )
    }
}

pub fn ensure_public_intent_preflight_ready(
    context: &ExecutionContext,
    command_name: &str,
) -> Result<(), JsonFailure> {
    validate_public_intent_preflight_allowed(context, command_name)?;
    persist_allowed_public_intent_preflight(context, command_name, false)
}

pub fn validate_public_begin_preflight_allowed(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    validate_public_intent_preflight_allowed(context, "begin")
}

pub fn public_begin_preflight_persistence_required(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    public_intent_preflight_persistence_required(context, "begin")
}

pub fn persist_allowed_public_begin_preflight(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    persist_allowed_public_intent_preflight(context, "begin", true)
}

pub fn ensure_public_begin_preflight_ready(context: &ExecutionContext) -> Result<(), JsonFailure> {
    validate_public_intent_preflight_allowed(context, "begin")?;
    if authoritative_run_identity_present(context)?
        || preflight_acceptance_for_context(context)?.is_some()
    {
        return Ok(());
    }
    let acceptance = persist_preflight_acceptance(context)?;
    ensure_preflight_authoritative_bootstrap(
        &context.runtime,
        RunIdentitySnapshot {
            execution_run_id: acceptance.execution_run_id.clone(),
            source_plan_path: context.plan_rel.clone(),
            source_plan_revision: context.plan_document.plan_revision,
        },
        acceptance.chunk_id,
    )
}

fn failure_class_for_gate_result(gate: &GateResult) -> FailureClass {
    match gate.failure_class.as_str() {
        "WorkspaceNotSafe" => FailureClass::WorkspaceNotSafe,
        "MalformedExecutionState" => FailureClass::MalformedExecutionState,
        "ConcurrentWriterConflict" => FailureClass::ConcurrentWriterConflict,
        "PartialAuthoritativeMutation" => FailureClass::PartialAuthoritativeMutation,
        _ => FailureClass::ExecutionStateNotReady,
    }
}

fn preflight_gate_failure_message(command_name: &str, gate: &GateResult) -> String {
    let Some(diagnostic) = gate.diagnostics.first() else {
        return format!("{command_name} is blocked because execution preflight is not allowed.");
    };
    format!(
        "{command_name} is blocked by execution preflight: {} Remediation: {}",
        diagnostic.message, diagnostic.remediation
    )
}

pub fn preflight_from_context(context: &ExecutionContext) -> GateResult {
    let mut gate = GateState::default();
    match preflight_write_authority_state(context) {
        Ok(PreflightWriteAuthorityState::Clear) => {}
        Ok(PreflightWriteAuthorityState::Conflict) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "write_authority_conflict",
            "Execution preflight cannot continue while another runtime writer holds write authority.",
            "Retry once the active writer releases write authority.",
        ),
        Err(error) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "write_authority_unavailable",
            error.message,
            "Restore write-authority lock access before retrying preflight.",
        ),
    }

    match preflight_requires_authoritative_handoff(context) {
        Ok(true) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "authoritative_handoff_required",
            "Execution preflight cannot continue while authoritative harness state requires handoff.",
            "Publish a valid handoff (or clear handoff_required in authoritative state) before retrying preflight.",
        ),
        Ok(false) => {}
        Err(error) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "authoritative_state_unavailable",
            error.message,
            "Restore authoritative harness state readability and validity before retrying preflight.",
        ),
    }
    match preflight_requires_authoritative_mutation_recovery(context) {
        Ok(true) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "authoritative_mutation_recovery_required",
            "Execution preflight cannot continue while authoritative artifact history is ahead of persisted harness state.",
            "Recover interrupted authoritative mutation state before retrying preflight.",
        ),
        Ok(false) => {}
        Err(error) => gate.fail(
            FailureClass::ExecutionStateNotReady,
            "authoritative_state_unavailable",
            error.message,
            "Restore authoritative harness state and artifact readability before retrying preflight.",
        ),
    }

    if let Some(step) = active_step(context, NoteState::Active) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "active_step_in_progress",
            format!(
                "Execution preflight cannot continue while Task {} Step {} is already active.",
                step.task_number, step.step_number
            ),
            "Resume or resolve the active step first.",
        );
    }
    if let Some(step) = active_step(context, NoteState::Blocked) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "blocked_step",
            format!(
                "Execution preflight cannot continue while Task {} Step {} is blocked.",
                step.task_number, step.step_number
            ),
            "Resolve the blocked step first.",
        );
    }
    if let Some(step) = active_step(context, NoteState::Interrupted) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "interrupted_work_unresolved",
            format!(
                "Execution preflight cannot continue while Task {} Step {} remains interrupted.",
                step.task_number, step.step_number
            ),
            "Resume or explicitly resolve the interrupted step first.",
        );
    }

    match repo_head_detached(context) {
        Ok(true) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "detached_head",
            "Execution preflight requires a branch-based workspace.",
            "Check out a branch before continuing execution.",
        ),
        Ok(false) => {}
        Err(error) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "branch_unavailable",
            error.message,
            "Restore repository availability before continuing execution.",
        ),
    }
    match RepoSafetyRuntime::discover(&context.runtime.repo_root) {
        Ok(runtime) => {
            let args = RepoSafetyCheckArgs {
                intent: RepoSafetyIntentArg::Write,
                stage: repo_safety_stage(context),
                task_id: Some(context.plan_rel.clone()),
                paths: vec![context.plan_rel.clone()],
                write_targets: vec![RepoSafetyWriteTargetArg::ExecutionTaskSlice],
            };
            match runtime.check(&args) {
                Ok(result) if result.outcome == "blocked" => gate.fail(
                    FailureClass::WorkspaceNotSafe,
                    &result.reason,
                    repo_safety_preflight_message(&result),
                    repo_safety_preflight_remediation(&result),
                ),
                Ok(_) => {}
                Err(error) => gate.fail(
                    FailureClass::WorkspaceNotSafe,
                    "repo_safety_unavailable",
                    error.message(),
                    "Restore repo-safety availability before continuing execution.",
                ),
            }
        }
        Err(error) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "repo_safety_unavailable",
            error.message(),
            "Restore repo-safety availability before continuing execution.",
        ),
    }
    match repo_has_non_runtime_projection_tracked_changes(context) {
        Ok(Some(reason)) => {
            let (message, remediation) = if reason == "approved_plan_semantic_drift" {
                (
                    "Execution preflight does not allow semantic approved-plan edits.",
                    "Restore, commit, or re-approve semantic approved-plan changes before continuing execution.",
                )
            } else {
                (
                    "Execution preflight does not allow tracked worktree changes.",
                    "Commit or discard tracked worktree changes before continuing execution.",
                )
            };
            gate.fail(
                FailureClass::WorkspaceNotSafe,
                &reason,
                message,
                remediation,
            );
        }
        Ok(None) => {}
        Err(error) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "worktree_state_unavailable",
            error.message,
            "Restore repository status inspection before continuing execution.",
        ),
    }

    if context.runtime.git_dir.join("MERGE_HEAD").exists() {
        gate.fail(
            FailureClass::WorkspaceNotSafe,
            "merge_in_progress",
            "Execution preflight does not allow an in-progress merge.",
            "Resolve or abort the merge before continuing.",
        );
    }
    if context.runtime.git_dir.join("rebase-merge").exists()
        || context.runtime.git_dir.join("rebase-apply").exists()
    {
        gate.fail(
            FailureClass::WorkspaceNotSafe,
            "rebase_in_progress",
            "Execution preflight does not allow an in-progress rebase.",
            "Resolve or abort the rebase before continuing.",
        );
    }
    if context.runtime.git_dir.join("CHERRY_PICK_HEAD").exists() {
        gate.fail(
            FailureClass::WorkspaceNotSafe,
            "cherry_pick_in_progress",
            "Execution preflight does not allow an in-progress cherry-pick.",
            "Resolve or abort the cherry-pick before continuing.",
        );
    }
    match repo_has_unresolved_index_entries(&context.runtime.repo_root) {
        Ok(true) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "unresolved_index_entries",
            "Execution preflight does not allow unresolved index entries.",
            "Resolve index conflicts before continuing.",
        ),
        Ok(false) => {}
        Err(error) => gate.fail(
            FailureClass::WorkspaceNotSafe,
            "index_unavailable",
            error.message,
            "Restore repository index availability before continuing execution.",
        ),
    }

    gate.finish()
}

pub fn gate_review_from_context(context: &ExecutionContext) -> GateResult {
    gate_review_from_context_internal(context, true)
}

fn persist_finish_review_gate_pass_checkpoint(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    let Some(branch_closure_id) = usable_current_branch_closure_identity(context)
        .map(|identity| identity.branch_closure_id)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    let mut authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_mut() else {
        return Ok(());
    };
    if !authoritative_state
        .record_finish_review_gate_pass_checkpoint_if_current(&branch_closure_id)?
    {
        return Ok(());
    }
    authoritative_state.persist_if_dirty_with_failpoint_and_command(None, "gate_review")
}

fn gate_review_base_result(
    context: &ExecutionContext,
    enforce_authoritative_late_gate_truth: bool,
) -> GateResult {
    let mut gate = GateState::default();
    let authoritative_completed_steps = authoritative_completed_steps_for_gate(context, &mut gate);
    if !gate.allowed {
        return gate.finish();
    }
    if let Some(step) = active_step(context, NoteState::Active) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "active_step_in_progress",
            format!(
                "Final review is blocked while Task {} Step {} remains active.",
                step.task_number, step.step_number
            ),
            "Complete, interrupt, or resolve the active step before review.",
        );
    }
    if let Some(step) = active_step(context, NoteState::Blocked) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "blocked_step",
            format!(
                "Final review is blocked while Task {} Step {} remains blocked.",
                step.task_number, step.step_number
            ),
            "Resolve the blocked step before review.",
        );
    }
    if let Some(step) = active_step(context, NoteState::Interrupted) {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "interrupted_work_unresolved",
            format!(
                "Final review is blocked while Task {} Step {} remains interrupted.",
                step.task_number, step.step_number
            ),
            "Resume or explicitly resolve the interrupted work before review.",
        );
    }

    if let Some(step) = context
        .steps
        .iter()
        .find(|step| !gate_step_is_complete(step, authoritative_completed_steps.as_ref()))
    {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "unfinished_steps_remaining",
            format!(
                "Final review is blocked while Task {} Step {} remains unchecked.",
                step.task_number, step.step_number
            ),
            "Finish all approved plan steps before final review.",
        );
    }

    for step in context
        .steps
        .iter()
        .filter(|step| gate_step_is_complete(step, authoritative_completed_steps.as_ref()))
    {
        let Some(attempt) =
            latest_attempt_for_step(&context.evidence, step.task_number, step.step_number)
        else {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "checked_step_missing_evidence",
                format!(
                    "Task {} Step {} is checked but missing execution evidence.",
                    step.task_number, step.step_number
                ),
                "Reopen the step or record matching execution evidence.",
            );
            continue;
        };
        if attempt.status != "Completed" {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "checked_step_missing_evidence",
                format!(
                    "Task {} Step {} no longer has a completed evidence attempt.",
                    step.task_number, step.step_number
                ),
                "Reopen the step or complete it again with fresh evidence.",
            );
        }
    }

    if enforce_authoritative_late_gate_truth {
        enforce_review_authoritative_late_gate_truth(context, &mut gate);
    }
    enforce_worktree_lease_binding_truth(context, &mut gate);

    if context.evidence.format == EvidenceFormat::Legacy && !context.evidence.attempts.is_empty() {
        gate.warn("legacy_evidence_format");
    }
    if context.evidence.format == EvidenceFormat::V2 {
        validate_v2_evidence_provenance(context, &mut gate);
    }

    gate.finish()
}

fn gate_step_is_complete(
    step: &PlanStepState,
    authoritative_completed_steps: Option<&BTreeSet<(u32, u32)>>,
) -> bool {
    authoritative_completed_steps.map_or(step.checked, |completed_steps| {
        completed_steps.contains(&(step.task_number, step.step_number))
    })
}

fn authoritative_completed_steps_for_gate(
    context: &ExecutionContext,
    gate: &mut GateState,
) -> Option<BTreeSet<(u32, u32)>> {
    let authoritative_state = match load_authoritative_transition_state(context) {
        Ok(Some(authoritative_state)) => authoritative_state,
        Ok(None) => {
            if context.local_execution_progress_markers_present
                || !context.evidence.attempts.is_empty()
            {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "authoritative_completion_state_missing",
                    "Final review requires authoritative event-log completion state; projection-only plan/evidence state is not authoritative.",
                    "Restore or migrate authoritative event-log state before final review.",
                );
                return Some(BTreeSet::new());
            }
            return None;
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_completion_state_unavailable",
                format!(
                    "Final review could not load authoritative completion state: {}",
                    error.message
                ),
                "Restore authoritative event-log state before final review.",
            );
            return Some(BTreeSet::new());
        }
    };
    let mut completed_steps = BTreeSet::new();
    for task in authoritative_state.current_task_closure_results().keys() {
        completed_steps.extend(
            context
                .steps
                .iter()
                .filter(|step| step.task_number == *task)
                .map(|step| (step.task_number, step.step_number)),
        );
    }
    if let Some(event_completed_steps) = authoritative_state
        .state_payload_snapshot()
        .get("event_completed_steps")
        .and_then(serde_json::Value::as_object)
    {
        for entry in event_completed_steps.values() {
            if let (Some(task), Some(step)) = (
                entry
                    .get("task")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok()),
                entry
                    .get("step")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok()),
            ) {
                completed_steps.insert((task, step));
            }
        }
    }
    Some(completed_steps)
}

fn gate_review_from_context_internal(
    context: &ExecutionContext,
    enforce_authoritative_late_gate_truth: bool,
) -> GateResult {
    let mut gate = GateState::from_result(gate_review_base_result(
        context,
        enforce_authoritative_late_gate_truth,
    ));
    if !gate.allowed {
        return gate.finish();
    }
    if !evaluate_pre_checkpoint_finish_gate(context, &mut gate) {
        return gate.finish();
    }
    gate.finish()
}

fn evaluate_pre_checkpoint_finish_gate(context: &ExecutionContext, gate: &mut GateState) -> bool {
    match context.repo_has_tracked_worktree_changes_excluding_execution_evidence() {
        Ok(true) => {
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                "review_artifact_worktree_dirty",
                "Finish readiness is blocked by tracked worktree changes that landed after the last review artifacts were generated.",
                "Commit or discard tracked worktree changes, then rerun requesting-code-review and downstream finish artifacts.",
            );
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
                "Tracked repo writes after final review invalidated review freshness for terminal branch completion.",
                "Commit or discard tracked worktree changes, then rerun requesting-code-review and downstream finish artifacts.",
            );
            return false;
        }
        Ok(false) => {}
        Err(error) => {
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                "review_artifact_worktree_state_unavailable",
                format!(
                    "Finish readiness could not determine whether tracked worktree changes are present: {}",
                    error.message
                ),
                "Restore repository status inspection, then rerun requesting-code-review and downstream finish artifacts.",
            );
            return false;
        }
    }
    let Some(current_base_branch) = context.current_release_base_branch() else {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_artifact_base_branch_unresolved",
            "Finish readiness could not determine the expected base branch for the current workspace.",
            "Resolve the release base branch before running gate-finish.",
        );
        return false;
    };
    let authoritative_state = match load_authoritative_transition_state(context) {
        Ok(Some(state)) => state,
        Ok(None) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_transition_state_missing",
                "Finish readiness requires authoritative transition state.",
                "Restore authoritative transition state before running gate-finish.",
            );
            return false;
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_transition_state_unavailable",
                format!(
                    "Finish readiness could not read authoritative transition state: {}",
                    error.message
                ),
                "Restore authoritative transition state before running gate-finish.",
            );
            return false;
        }
    };
    let Some(current_branch_closure_id) = current_branch_closure_id(context) else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "current_branch_closure_id_missing",
            "Finish readiness requires a current branch-closure binding.",
            "Record or repair the current branch closure before running gate-finish.",
        );
        return false;
    };
    let current_branch_reviewed_state_id = current_branch_reviewed_state_id(context);
    let Some(current_branch_reviewed_state_id) = current_branch_reviewed_state_id else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "current_branch_reviewed_state_id_missing",
            "Finish readiness requires a current reviewed-branch-state binding.",
            "Repair authoritative branch-closure overlays before running gate-finish.",
        );
        return false;
    };
    match shared_current_branch_closure_has_tracked_drift(context, Some(&authoritative_state)) {
        Ok(true) => {
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
                "Tracked repo writes after final review invalidated review freshness for terminal branch completion.",
                "Record a fresh branch closure and rerun requesting-code-review and downstream finish artifacts.",
            );
            return false;
        }
        Ok(false) => {}
        Err(error) => {
            gate.fail(
                FailureClass::ReviewArtifactNotFresh,
                "review_artifact_workspace_state_unavailable",
                format!(
                    "Finish readiness could not compare current workspace state with the reviewed branch closure: {}",
                    error.message
                ),
                "Restore repository state inspection, then rerun requesting-code-review and downstream finish artifacts.",
            );
            return false;
        }
    }
    if !require_current_release_readiness_ready_for_finish(
        context,
        &authoritative_state,
        &current_branch_closure_id,
        &current_branch_reviewed_state_id,
        &current_base_branch,
        gate,
    ) {
        return false;
    }
    if !require_current_final_review_pass_for_finish(
        context,
        &authoritative_state,
        &current_branch_closure_id,
        &current_branch_reviewed_state_id,
        &current_base_branch,
        gate,
    ) {
        return false;
    }

    let browser_qa_required = match context.plan_document.qa_requirement.as_deref() {
        Some("required") => true,
        Some("not-required") => false,
        _ => {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                "qa_requirement_missing_or_invalid",
                "Finish readiness requires approved-plan QA Requirement metadata to be present and valid.",
                "Record a workflow pivot so the approved plan can be corrected, then rerun the late-stage flow.",
            );
            return false;
        }
    };
    if browser_qa_required
        && !require_current_browser_qa_pass_for_finish(
            context,
            &authoritative_state,
            &current_branch_closure_id,
            &current_branch_reviewed_state_id,
            &current_base_branch,
            gate,
        )
    {
        return false;
    }

    true
}

// Barrier reconcile and receipt release:
//   open / review_passed_pending_reconcile
//                    |
//                    v
//       reconcile reviewed checkpoint commit
//                    |
//                    v
//          cleanup_state == cleaned
//                    |
//                    v
//      dependent work may be released at finish
fn enforce_worktree_lease_binding_truth(context: &ExecutionContext, gate: &mut GateState) {
    let authoritative_context = match load_worktree_lease_authoritative_context_checked(context) {
        Ok(Some(context)) => context,
        Ok(None) => {
            let artifacts_dir = harness_authoritative_artifacts_dir(
                &context.runtime.state_dir,
                &context.runtime.repo_slug,
                &context.runtime.branch_name,
            );
            let has_any_binding_artifacts = match fs::read_dir(&artifacts_dir) {
                Ok(entries) => entries.flatten().any(|entry| {
                    entry
                        .path()
                        .file_name()
                        .and_then(|value| value.to_str())
                        .is_some_and(|value| {
                            (value.starts_with("worktree-lease-") && value.ends_with(".json"))
                                || (value.starts_with("unit-review-") && value.ends_with(".md"))
                        })
                }),
                Err(error) if error.kind() == ErrorKind::NotFound => false,
                Err(error) => {
                    gate.fail(
                        FailureClass::MalformedExecutionState,
                        "worktree_lease_artifacts_unreadable",
                        format!(
                            "Could not inspect authoritative worktree leases in {}: {error}",
                            artifacts_dir.display()
                        ),
                        "Restore authoritative worktree lease readability and retry gate-review or gate-finish.",
                    );
                    return;
                }
            };
            if has_any_binding_artifacts {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_authoritative_state_unavailable",
                    "Authoritative harness state is unavailable for worktree lease gating.",
                    "Restore authoritative harness state readability and retry gate-review or gate-finish.",
                );
            }
            return;
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_state_unavailable",
                error.message,
                "Restore authoritative harness state readability and retry gate-review or gate-finish.",
            );
            return;
        }
    };
    let run_identity = match authoritative_context.run_identity.as_ref() {
        Some(run_identity) => run_identity,
        None => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_run_identity_missing",
                "Authoritative harness state is missing its current run identity.",
                "Restore authoritative harness state readability and retry gate-review or gate-finish.",
            );
            return;
        }
    };
    if run_identity.source_plan_path != context.plan_rel
        || run_identity.source_plan_revision != context.plan_document.plan_revision
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_run_context_mismatch",
            "Authoritative run identity does not match the current plan context.",
            "Restore authoritative harness state readability and retry gate-review or gate-finish.",
        );
        return;
    }

    let Some(active_worktree_lease_fingerprints) =
        authoritative_context.active_worktree_lease_fingerprints
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_index_missing",
            "Authoritative harness state is missing the active worktree lease fingerprint index for the current run.",
            "Restore the authoritative worktree lease fingerprints and retry gate-review or gate-finish.",
        );
        return;
    };
    let Some(active_worktree_lease_bindings) = authoritative_context.active_worktree_lease_bindings
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_index_missing",
            "Authoritative harness state is missing the active worktree lease binding index for the current run.",
            "Restore the authoritative worktree lease bindings and retry gate-review or gate-finish.",
        );
        return;
    };
    let current_run_fingerprint_count = active_worktree_lease_fingerprints.len();
    let current_run_fingerprints: BTreeSet<String> =
        active_worktree_lease_fingerprints.into_iter().collect();
    if current_run_fingerprints.len() != current_run_fingerprint_count {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_binding_duplicate",
            "Authoritative harness state contains duplicate active worktree lease fingerprints for the current run.",
            "Restore the authoritative worktree lease fingerprints and retry gate-review or gate-finish.",
        );
        return;
    }

    let current_run_bindings = active_worktree_lease_bindings
        .iter()
        .filter(|binding| binding.execution_run_id == run_identity.execution_run_id)
        .collect::<Vec<_>>();
    if current_run_fingerprints.is_empty() {
        let current_run_artifacts_exist = match current_run_worktree_lease_artifacts_exist(
            context,
            &run_identity.execution_run_id,
        ) {
            Ok(value) => value,
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_artifacts_unreadable",
                    error,
                    "Restore authoritative worktree lease readability and retry gate-review or gate-finish.",
                );
                return;
            }
        };
        if !current_run_bindings.is_empty() || current_run_artifacts_exist {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_binding_missing",
                "Authoritative harness state is missing the active worktree lease fingerprint index for the current run.",
                "Restore the authoritative worktree lease fingerprints and retry gate-review or gate-finish.",
            );
            return;
        }
        if !context.steps.iter().any(|step| step.checked) {
            return;
        }
        let active_contract_overlay = match load_status_authoritative_overlay_checked(context) {
            Ok(Some(overlay)) => overlay,
            Ok(None) => return,
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_authoritative_state_unavailable",
                    error.message,
                    "Restore authoritative harness state readability and retry gate-review or gate-finish.",
                );
                return;
            }
        };
        let active_contract_path = active_contract_overlay
            .active_contract_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let active_contract_fingerprint = active_contract_overlay
            .active_contract_fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if active_contract_path.is_none() && active_contract_fingerprint.is_none() {
            enforce_plain_unit_review_truth(context, run_identity.execution_run_id.as_str(), gate);
            return;
        }
        let Some((_active_contract_path, active_contract_fingerprint)) =
            load_authoritative_active_contract(context, gate)
        else {
            return;
        };
        enforce_serial_unit_review_truth(context, run_identity, &active_contract_fingerprint, gate);
        return;
    }
    if current_run_bindings.is_empty() {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_binding_missing",
            "Authoritative harness state is missing one or more active worktree lease bindings for the current run.",
            "Restore the authoritative worktree lease bindings and retry gate-review or gate-finish.",
        );
        return;
    }

    let Some((active_contract_path, active_contract_fingerprint)) =
        load_authoritative_active_contract(context, gate)
    else {
        return;
    };
    let active_contract = match read_execution_contract(&active_contract_path) {
        Ok(contract) => contract,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_contract_unreadable",
                format!(
                    "Authoritative active contract {} is malformed: {error}",
                    active_contract_path.display()
                ),
                "Restore the authoritative active contract and retry gate-review or gate-finish.",
            );
            return;
        }
    };
    if active_contract.contract_fingerprint != active_contract_fingerprint {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_unreadable",
            "Authoritative active contract fingerprint does not match its canonical content.",
            "Restore the authoritative active contract and retry gate-review or gate-finish.",
        );
        return;
    }

    let current_head = match context.current_head_sha() {
        Ok(head) => head,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_head_unavailable",
                error.message,
                "Restore repository HEAD inspection and retry gate-review or gate-finish.",
            );
            return;
        }
    };

    let mut binding_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut binding_by_fingerprint: BTreeMap<String, &WorktreeLeaseBindingProbe> = BTreeMap::new();
    for binding in current_run_bindings.iter().copied() {
        let fingerprint = binding.lease_fingerprint.trim().to_owned();
        if fingerprint.is_empty() || !current_run_fingerprints.contains(&fingerprint) {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_binding_missing",
                "Authoritative harness state contains a worktree lease binding that is not indexed by the current runtime state.",
                "Restore the authoritative worktree lease bindings and retry gate-review or gate-finish.",
            );
            return;
        }
        *binding_counts.entry(fingerprint.clone()).or_insert(0) += 1;
        binding_by_fingerprint.insert(fingerprint, binding);
    }
    if binding_counts.values().any(|count| *count > 1)
        || binding_by_fingerprint.len() != current_run_bindings.len()
        || binding_by_fingerprint.len() != current_run_fingerprints.len()
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_binding_duplicate",
            "Authoritative harness state contains duplicate or missing active worktree lease bindings for the current run.",
            "Restore the authoritative worktree lease bindings and retry gate-review or gate-finish.",
        );
        return;
    }

    for fingerprint in current_run_fingerprints {
        let binding = binding_by_fingerprint
            .get(&fingerprint)
            .expect("binding should exist for each current lease fingerprint");
        let lease_artifact_path = match normalize_authoritative_artifact_binding_path(
            &binding.lease_artifact_path,
            "worktree lease",
            gate,
        ) {
            Some(path) => path,
            None => return,
        };
        let lease_path = harness_authoritative_artifact_path(
            &context.runtime.state_dir,
            &context.runtime.repo_slug,
            &context.runtime.branch_name,
            lease_artifact_path.to_string_lossy().as_ref(),
        );
        let lease_metadata = match fs::symlink_metadata(&lease_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_metadata_unreadable",
                    format!(
                        "Could not inspect authoritative worktree lease {}: {error}",
                        lease_path.display()
                    ),
                    "Restore authoritative worktree lease readability and retry gate-review or gate-finish.",
                );
                return;
            }
        };
        if lease_metadata.file_type().is_symlink() || !lease_metadata.is_file() {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_path_not_regular_file",
                format!(
                    "Authoritative worktree lease must be a regular file in {}.",
                    lease_path.display()
                ),
                "Restore authoritative worktree lease readability and retry gate-review or gate-finish.",
            );
            return;
        }

        let source = match fs::read_to_string(&lease_path) {
            Ok(source) => source,
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_unreadable",
                    format!(
                        "Could not read authoritative worktree lease {}: {error}",
                        lease_path.display()
                    ),
                    "Restore authoritative worktree lease readability and retry gate-review or gate-finish.",
                );
                return;
            }
        };

        let lease: WorktreeLease = match serde_json::from_str(&source) {
            Ok(lease) => lease,
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_malformed",
                    format!(
                        "Authoritative worktree lease is malformed in {}: {error}",
                        lease_path.display()
                    ),
                    "Repair the authoritative worktree lease artifact and retry gate-review or gate-finish.",
                );
                return;
            }
        };

        let expected_lease_file_name = format!(
            "worktree-lease-{}-{}-{}.json",
            branch_storage_key(&context.runtime.branch_name),
            lease.execution_run_id,
            lease.execution_context_key
        );
        if lease_path.file_name().and_then(|value| value.to_str())
            != Some(expected_lease_file_name.as_str())
        {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_binding_path_invalid",
                "Authoritative worktree lease binding path does not match the canonical runtime-owned filename.",
                "Restore the authoritative worktree lease binding path and retry gate-review or gate-finish.",
            );
            return;
        }

        if lease.lease_fingerprint != fingerprint {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_provenance_unindexed",
                "Authoritative worktree lease fingerprint is not indexed by the current runtime state.",
                "Regenerate the authoritative worktree lease from the current runtime and retry gate-review or gate-finish.",
            );
            return;
        }

        if lease.execution_run_id != run_identity.execution_run_id {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_run_id_mismatch",
                "Authoritative worktree lease body does not match the current execution run.",
                "Regenerate the authoritative worktree lease from the current runtime and retry gate-review or gate-finish.",
            );
            return;
        }
        if !lease_applies_to_current_plan_context(context, &lease) {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_plan_context_mismatch",
                "Authoritative worktree lease does not match the current plan and execution context.",
                "Regenerate the authoritative worktree lease from the current runtime and retry gate-review or gate-finish.",
            );
            return;
        }
        if let Err(error) = validate_worktree_lease(&lease) {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_validation_failed",
                error.message,
                "Repair the authoritative worktree lease artifact and retry gate-review or gate-finish.",
            );
            return;
        }
        if authoritative_context
            .repo_state_baseline_head_sha
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_state_missing",
                "Authoritative harness state is missing the baseline head provenance required for worktree lease gating.",
                "Restore the authoritative worktree lease baseline provenance and retry gate-review or gate-finish.",
            );
            return;
        }
        if authoritative_context
            .repo_state_baseline_worktree_fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_state_missing",
                "Authoritative harness state is missing the baseline worktree provenance required for worktree lease gating.",
                "Restore the authoritative worktree lease baseline provenance and retry gate-review or gate-finish.",
            );
            return;
        }
        let expected_execution_context_key = worktree_lease_execution_context_key(
            &run_identity.execution_run_id,
            &lease.execution_unit_id,
            context.plan_rel.as_str(),
            context.plan_document.plan_revision,
            &lease.authoritative_integration_branch,
            lease
                .reviewed_checkpoint_commit_sha
                .as_deref()
                .unwrap_or("open"),
        );
        if lease.execution_context_key.trim() != expected_execution_context_key {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_execution_context_key_mismatch",
                "Authoritative worktree lease body does not match the current execution context.",
                "Regenerate the authoritative worktree lease from the current runtime context and retry gate-review or gate-finish.",
            );
            return;
        }
        if !validate_authoritative_worktree_lease_fingerprint(
            &source,
            &lease,
            lease_path.display().to_string(),
            gate,
        ) {
            return;
        }

        match lease.lease_state {
            WorktreeLeaseState::Open => {
                gate.fail(
                    FailureClass::ExecutionStateNotReady,
                    "worktree_lease_open",
                    "An authoritative worktree lease remains open.",
                    "Reconcile and clean the worktree lease before rerunning gate-review or gate-finish.",
                );
                return;
            }
            WorktreeLeaseState::ReviewPassedPendingReconcile => {
                gate.fail(
                    FailureClass::ExecutionStateNotReady,
                    "worktree_lease_reconcile_pending",
                    "An authoritative worktree lease has passed review but not yet been reconciled.",
                    "Reconcile the reviewed checkpoint back onto the active branch before rerunning gate-review or gate-finish.",
                );
                return;
            }
            WorktreeLeaseState::Reconciled | WorktreeLeaseState::Cleaned => {
                let Some(review_receipt_fingerprint) = binding
                    .review_receipt_fingerprint
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_review_receipt_missing",
                        "An authoritative unit-review receipt is required before a cleaned worktree lease can release dependent work.",
                        "Record the authoritative unit-review receipt for the current reviewed checkpoint and retry gate-review or gate-finish.",
                    );
                    return;
                };
                let Some(approved_task_packet_fingerprint) = binding
                    .approved_task_packet_fingerprint
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_review_receipt_task_packet_missing",
                        "An authoritative unit-review receipt is required to bind the approved task packet before a cleaned worktree lease can release dependent work.",
                        "Record the authoritative unit-review receipt for the current approved task packet and retry gate-review or gate-finish.",
                    );
                    return;
                };
                if !active_contract
                    .source_task_packet_fingerprints
                    .iter()
                    .any(|candidate| candidate == approved_task_packet_fingerprint)
                {
                    gate.fail(
                        FailureClass::MalformedExecutionState,
                        "worktree_lease_review_receipt_task_packet_not_authoritative",
                        "The authoritative unit-review receipt does not bind a task packet from the current authoritative contract.",
                        "Record the authoritative unit-review receipt for the current approved task packet and retry gate-review or gate-finish.",
                    );
                    return;
                }
                let Some(approved_unit_contract_fingerprint) = binding
                    .approved_unit_contract_fingerprint
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_review_receipt_unit_contract_missing",
                        "An authoritative unit-review receipt is required to bind the approved unit contract before a cleaned worktree lease can release dependent work.",
                        "Record the authoritative unit-review receipt for the current approved unit contract and retry gate-review or gate-finish.",
                    );
                    return;
                };
                let expected_approved_unit_contract_fingerprint =
                    approved_unit_contract_fingerprint_for_review(
                        active_contract_fingerprint.as_str(),
                        approved_task_packet_fingerprint,
                        lease.execution_unit_id.as_str(),
                    );
                if approved_unit_contract_fingerprint != expected_approved_unit_contract_fingerprint
                {
                    gate.fail(
                        FailureClass::MalformedExecutionState,
                        "worktree_lease_review_receipt_unit_contract_mismatch",
                        "The authoritative unit-review receipt does not bind the canonical approved unit contract fingerprint.",
                        "Record the authoritative unit-review receipt for the current approved unit contract and retry gate-review or gate-finish.",
                    );
                    return;
                }
                let Some(reviewed_checkpoint_commit_sha) = binding
                    .reviewed_checkpoint_commit_sha
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_review_receipt_missing",
                        "An authoritative unit-review receipt is required to bind the reviewed checkpoint before a cleaned worktree lease can release dependent work.",
                        "Record the authoritative unit-review receipt for the current reviewed checkpoint and retry gate-review or gate-finish.",
                    );
                    return;
                };
                let Some(reconcile_mode) = binding
                    .reconcile_mode
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_reconcile_mode_missing",
                        "An authoritative unit-review receipt is required to bind the identity-preserving reconcile mode before a cleaned worktree lease can release dependent work.",
                        "Record the authoritative unit-review receipt for the current identity-preserving reconcile and retry gate-review or gate-finish.",
                    );
                    return;
                };
                let Some(reconcile_result_commit_sha) = binding
                    .reconcile_result_commit_sha
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_identity_preserving_proof_missing",
                        "An authoritative unit-review receipt is required to bind the exact reconciled commit before a cleaned worktree lease can release dependent work.",
                        "Record the authoritative unit-review receipt for the current exact reconciled commit and retry gate-review or gate-finish.",
                    );
                    return;
                };
                let Some(reconcile_result_proof_fingerprint) = binding
                    .reconcile_result_proof_fingerprint
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_identity_preserving_proof_missing",
                        "An authoritative unit-review receipt is required to bind the exact reconciled commit object before a cleaned worktree lease can release dependent work.",
                        "Record the authoritative unit-review receipt for the current exact reconciled commit object and retry gate-review or gate-finish.",
                    );
                    return;
                };
                let Some(expected_reconcile_result_commit_sha) = lease
                    .reconcile_result_commit_sha
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_identity_preserving_proof_missing",
                        "An authoritative worktree lease is missing the exact reconciled commit proof required to release dependent work.",
                        "Regenerate the authoritative worktree lease from the recorded identity-preserving reconcile and retry gate-review or gate-finish.",
                    );
                    return;
                };
                let Some(expected_reconcile_result_proof_fingerprint) = lease
                    .reconcile_result_proof_fingerprint
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_identity_preserving_proof_missing",
                        "An authoritative worktree lease is missing the exact reconciled commit object proof required to release dependent work.",
                        "Regenerate the authoritative worktree lease from the recorded identity-preserving reconcile and retry gate-review or gate-finish.",
                    );
                    return;
                };
                let Some(computed_reconcile_result_proof_fingerprint) =
                    reconcile_result_proof_fingerprint_for_review(
                        &context.runtime.repo_root,
                        expected_reconcile_result_commit_sha,
                    )
                else {
                    gate.fail(
                        FailureClass::MalformedExecutionState,
                        "worktree_lease_identity_preserving_proof_unverifiable",
                        "The authoritative worktree lease exact reconcile proof could not be verified against repository history.",
                        "Regenerate the authoritative worktree lease from the recorded identity-preserving reconcile and retry gate-review or gate-finish.",
                    );
                    return;
                };
                if expected_reconcile_result_proof_fingerprint
                    != computed_reconcile_result_proof_fingerprint
                {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_lease_proof_mismatch",
                        "The authoritative worktree lease exact reconciled commit object proof does not match the reviewed reconcile proof.",
                        "Regenerate the authoritative worktree lease from the recorded identity-preserving reconcile and retry gate-review or gate-finish.",
                    );
                    return;
                }
                if reconcile_result_proof_fingerprint != computed_reconcile_result_proof_fingerprint
                {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_proof_mismatch",
                        "The authoritative worktree lease exact reconciled commit object does not match the authoritative unit-review receipt.",
                        "Regenerate the authoritative worktree lease from the recorded unit-review receipt and retry gate-review or gate-finish.",
                    );
                    return;
                }
                let Some(review_receipt_path_name) = binding
                    .review_receipt_artifact_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_review_receipt_missing",
                        "An authoritative unit-review receipt is required before a cleaned worktree lease can release dependent work.",
                        "Record the authoritative unit-review receipt for the current reviewed checkpoint and retry gate-review or gate-finish.",
                    );
                    return;
                };
                let review_receipt_path_name = match normalize_authoritative_artifact_binding_path(
                    review_receipt_path_name,
                    "unit-review receipt",
                    gate,
                ) {
                    Some(path) => path,
                    None => return,
                };
                let review_receipt_path = harness_authoritative_artifact_path(
                    &context.runtime.state_dir,
                    &context.runtime.repo_slug,
                    &context.runtime.branch_name,
                    review_receipt_path_name.to_string_lossy().as_ref(),
                );
                let review_metadata = match fs::symlink_metadata(&review_receipt_path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        gate.fail(
                            FailureClass::ExecutionStateNotReady,
                            "worktree_lease_review_receipt_missing",
                            format!(
                                "Could not inspect authoritative unit-review receipt {}: {error}",
                                review_receipt_path.display()
                            ),
                            "Record the authoritative unit-review receipt for the current reviewed checkpoint and retry gate-review or gate-finish.",
                        );
                        return;
                    }
                };
                if review_metadata.file_type().is_symlink() || !review_metadata.is_file() {
                    gate.fail(
                        FailureClass::MalformedExecutionState,
                        "worktree_lease_review_receipt_path_not_regular_file",
                        format!(
                            "Authoritative unit-review receipt must be a regular file in {}.",
                            review_receipt_path.display()
                        ),
                        "Restore the authoritative unit-review receipt and retry gate-review or gate-finish.",
                        );
                    return;
                }
                let expected_review_receipt_filename = format!(
                    "unit-review-{}-{}.md",
                    run_identity.execution_run_id,
                    lease.execution_unit_id.trim_start_matches("unit-")
                );
                if review_receipt_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    != Some(expected_review_receipt_filename.as_str())
                {
                    gate.fail(
                        FailureClass::MalformedExecutionState,
                        "worktree_lease_binding_path_invalid",
                        "Authoritative unit-review receipt binding path does not match the reviewed execution unit provenance.",
                        "Restore the authoritative unit-review receipt binding path and retry gate-review or gate-finish.",
                    );
                    return;
                }
                let review_source = match fs::read_to_string(&review_receipt_path) {
                    Ok(source) => source,
                    Err(error) => {
                        gate.fail(
                            FailureClass::ExecutionStateNotReady,
                            "worktree_lease_review_receipt_unreadable",
                            format!(
                                "Could not read authoritative unit-review receipt {}: {error}",
                                review_receipt_path.display()
                            ),
                            "Restore the authoritative unit-review receipt and retry gate-review or gate-finish.",
                        );
                        return;
                    }
                };
                let (receipt_checkpoint_commit_sha, receipt_reconciled_result_commit_sha) =
                    match validate_authoritative_unit_review_receipt(
                        context,
                        &run_identity.execution_run_id,
                        &lease,
                        &review_source,
                        &review_receipt_path,
                        UnitReviewReceiptExpectations {
                            expected_execution_context_key: &expected_execution_context_key,
                            expected_fingerprint: review_receipt_fingerprint,
                            expected_task_packet_fingerprint: approved_task_packet_fingerprint,
                            expected_approved_unit_contract_fingerprint:
                                approved_unit_contract_fingerprint,
                            expected_reconcile_result_commit_sha,
                        },
                        gate,
                    ) {
                        Some(values) => values,
                        None => return,
                    };

                if reviewed_checkpoint_commit_sha != receipt_checkpoint_commit_sha {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_provenance_mismatch",
                        "Authoritative worktree lease reviewed checkpoint does not match the runtime-owned unit-review binding.",
                        "Regenerate the authoritative worktree lease from the recorded unit-review receipt and retry gate-review or gate-finish.",
                    );
                    return;
                }
                if reconcile_result_commit_sha != receipt_reconciled_result_commit_sha {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_proof_mismatch",
                        "Authoritative worktree lease reconciled result does not match the runtime-owned unit-review binding.",
                        "Regenerate the authoritative worktree lease from the recorded unit-review receipt and retry gate-review or gate-finish.",
                    );
                    return;
                }
                if binding
                    .execution_context_key
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    != Some(expected_execution_context_key.as_str())
                {
                    gate.fail(
                        FailureClass::MalformedExecutionState,
                        "worktree_lease_execution_context_key_mismatch",
                        "Authoritative worktree lease binding does not match the current execution context.",
                        "Regenerate the authoritative worktree lease binding from the current runtime context and retry gate-review or gate-finish.",
                    );
                    return;
                }
                if reconcile_mode != "identity_preserving"
                    || lease.reconcile_mode.trim() != "identity_preserving"
                {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_reconcile_mode_mismatch",
                        "Authoritative worktree lease does not prove an identity-preserving reconcile.",
                        "Regenerate the authoritative worktree lease from the recorded identity-preserving reconcile and retry gate-review or gate-finish.",
                    );
                    return;
                }

                if lease.reviewed_checkpoint_commit_sha.as_deref()
                    != Some(receipt_checkpoint_commit_sha.as_str())
                {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_review_receipt_checkpoint_mismatch",
                        "Authoritative worktree lease reviewed checkpoint does not match the authoritative unit-review receipt.",
                        "Regenerate the authoritative worktree lease from the recorded unit-review receipt and retry gate-review or gate-finish.",
                    );
                    return;
                }
                if Some(lease.repo_state_baseline_head_sha.as_str())
                    != authoritative_context
                        .repo_state_baseline_head_sha
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_provenance_mismatch",
                        "Authoritative worktree lease baseline head provenance does not match the current authoritative baseline.",
                        "Regenerate the authoritative worktree lease from the identity-preserving reviewed checkpoint and retry gate-review or gate-finish.",
                    );
                    return;
                }
                if Some(lease.repo_state_baseline_worktree_fingerprint.as_str())
                    != authoritative_context
                        .repo_state_baseline_worktree_fingerprint
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_provenance_mismatch",
                        "Authoritative worktree lease baseline worktree provenance does not match the current authoritative baseline.",
                        "Regenerate the authoritative worktree lease from the identity-preserving reviewed checkpoint and retry gate-review or gate-finish.",
                    );
                    return;
                }
                if !is_ancestor_commit(
                    &context.runtime.repo_root,
                    &receipt_checkpoint_commit_sha,
                    reconcile_result_commit_sha,
                ) {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_checkpoint_mismatch",
                        "Authoritative worktree lease reconciled result is not descended from the reviewed checkpoint.",
                        "Reconcile the reviewed checkpoint back onto the active branch history and rerun gate-review or gate-finish with a fresh lease.",
                    );
                    return;
                }
                if !is_ancestor_commit(
                    &context.runtime.repo_root,
                    reconcile_result_commit_sha,
                    &current_head,
                ) {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_checkpoint_mismatch",
                        "Authoritative worktree lease reconciled result is not contained in the current branch history.",
                        "Reconcile the reviewed checkpoint back onto the active branch history and rerun gate-review or gate-finish with a fresh lease.",
                    );
                    return;
                }
                if lease.cleanup_state.trim() != "cleaned" {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_cleanup_pending",
                        "Authoritative worktree lease has not been cleaned up yet.",
                        "Clean the temporary worktree before rerunning gate-review or gate-finish.",
                    );
                    return;
                }
            }
        }
    }
}

fn load_worktree_lease_authoritative_context_checked(
    context: &ExecutionContext,
) -> Result<Option<WorktreeLeaseAuthoritativeContextProbe>, JsonFailure> {
    let state_path = authoritative_state_path(context);
    let Some(payload) = load_reduced_authoritative_state_for_state_path(&state_path)? else {
        return Ok(None);
    };
    let context: WorktreeLeaseAuthoritativeContextProbe =
        serde_json::from_value(strip_top_level_null_fields(payload)).map_err(|error| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative reduced state is malformed in {}: {error}",
                    state_path.display()
                ),
            )
        })?;
    Ok(Some(context))
}

fn strip_top_level_null_fields(mut payload: serde_json::Value) -> serde_json::Value {
    if let Some(object) = payload.as_object_mut() {
        object.retain(|_, value| !value.is_null());
    }
    payload
}

fn lease_applies_to_current_plan_context(
    context: &ExecutionContext,
    lease: &WorktreeLease,
) -> bool {
    lease.source_plan_path == context.plan_rel
        && lease.source_plan_revision == context.plan_document.plan_revision
        && lease.authoritative_integration_branch == context.runtime.branch_name
        && !lease.source_branch.trim().is_empty()
}

fn normalize_authoritative_artifact_binding_path(
    raw_path: &str,
    artifact_kind: &str,
    gate: &mut GateState,
) -> Option<PathBuf> {
    let trimmed = raw_path.trim();
    let mut components = Path::new(trimmed).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None) => {
            let filename = component.to_string_lossy();
            if filename.is_empty() {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_binding_path_invalid",
                    format!(
                        "Authoritative {artifact_kind} binding path must be a normalized relative filename."
                    ),
                    format!(
                        "Restore the authoritative {artifact_kind} binding path and retry gate-review or gate-finish."
                    ),
                );
                None
            } else {
                Some(PathBuf::from(filename.as_ref()))
            }
        }
        _ => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_binding_path_invalid",
                format!(
                    "Authoritative {artifact_kind} binding path must be a normalized relative filename."
                ),
                format!(
                    "Restore the authoritative {artifact_kind} binding path and retry gate-review or gate-finish."
                ),
            );
            None
        }
    }
}

fn current_run_worktree_lease_artifacts_exist(
    context: &ExecutionContext,
    execution_run_id: &str,
) -> Result<bool, String> {
    let artifacts_dir = harness_authoritative_artifacts_dir(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
    );
    let entries = match fs::read_dir(&artifacts_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Could not inspect authoritative worktree leases in {}: {error}",
                artifacts_dir.display()
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Could not inspect authoritative worktree leases in {}: {error}",
                artifacts_dir.display()
            )
        })?;
        let file_path = entry.path();
        let Some(file_name) = file_path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".json") {
            continue;
        }
        let canonical_prefix = format!(
            "worktree-lease-{}-{}-",
            branch_storage_key(&context.runtime.branch_name),
            execution_run_id
        );
        let canonical_candidate = file_name.starts_with(&canonical_prefix);
        let metadata = match fs::symlink_metadata(&file_path) {
            Ok(metadata) => metadata,
            Err(error) if canonical_candidate => {
                return Err(format!(
                    "Could not inspect authoritative worktree lease {}: {error}",
                    file_path.display()
                ));
            }
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            if canonical_candidate {
                return Err(format!(
                    "Authoritative worktree lease must be a regular file in {}.",
                    file_path.display()
                ));
            }
            continue;
        }
        let Ok(source) = fs::read_to_string(&file_path) else {
            if canonical_candidate {
                return Err(format!(
                    "Could not read authoritative worktree lease {}.",
                    file_path.display()
                ));
            }
            continue;
        };
        let lease = match serde_json::from_str::<WorktreeLease>(&source) {
            Ok(lease) => lease,
            Err(error) if canonical_candidate => {
                return Err(format!(
                    "Authoritative worktree lease is malformed in {}: {error}",
                    file_path.display()
                ));
            }
            Err(_) => continue,
        };
        let matches_current_run = lease.execution_run_id == execution_run_id
            && lease.source_plan_path == context.plan_rel
            && lease.source_plan_revision == context.plan_document.plan_revision
            && lease.authoritative_integration_branch == context.runtime.branch_name;
        if !matches_current_run {
            if canonical_candidate {
                return Err(format!(
                    "Authoritative worktree lease {} does not match the current run context.",
                    file_path.display()
                ));
            }
            continue;
        }
        let reviewed_checkpoint_commit_sha = lease
            .reviewed_checkpoint_commit_sha
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("open");
        let expected_execution_context_key = worktree_lease_execution_context_key(
            execution_run_id,
            lease.execution_unit_id.as_str(),
            context.plan_rel.as_str(),
            context.plan_document.plan_revision,
            lease.authoritative_integration_branch.as_str(),
            reviewed_checkpoint_commit_sha,
        );
        if lease.execution_context_key != expected_execution_context_key {
            if canonical_candidate {
                return Err(format!(
                    "Authoritative worktree lease {} does not match the current execution context.",
                    file_path.display()
                ));
            }
            continue;
        }
        if let Err(error) = validate_worktree_lease(&lease) {
            if canonical_candidate || matches_current_run {
                return Err(error.message);
            }
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}

fn current_run_plain_unit_review_receipt_paths(
    context: &ExecutionContext,
    execution_run_id: &str,
) -> Result<Vec<PathBuf>, String> {
    let artifacts_dir = harness_authoritative_artifacts_dir(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
    );
    let entries = match fs::read_dir(&artifacts_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "Could not inspect authoritative unit-review receipts in {}: {error}",
                artifacts_dir.display()
            ));
        }
    };
    let canonical_prefix = format!("unit-review-{execution_run_id}-task-");
    let mut receipt_paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Could not inspect authoritative unit-review receipts in {}: {error}",
                artifacts_dir.display()
            )
        })?;
        let file_path = entry.path();
        let Some(file_name) = file_path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if file_name.starts_with(&canonical_prefix) && file_name.ends_with(".md") {
            receipt_paths.push(file_path);
        }
    }
    receipt_paths.sort();
    Ok(receipt_paths)
}

fn enforce_plain_unit_review_truth(
    context: &ExecutionContext,
    execution_run_id: &str,
    gate: &mut GateState,
) {
    let current_run_receipts = match current_run_plain_unit_review_receipt_paths(
        context,
        execution_run_id,
    ) {
        Ok(paths) => paths,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipts_unreadable",
                error,
                "Restore authoritative unit-review receipt readability and retry gate-review or gate-finish.",
            );
            return;
        }
    };
    if current_run_receipts.is_empty() {
        return;
    }

    let expected_strategy_checkpoint_fingerprint =
        match authoritative_strategy_checkpoint_fingerprint_checked(context) {
            Ok(Some(fingerprint)) if !fingerprint.trim().is_empty() => fingerprint,
            Ok(_) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "plain_unit_review_receipt_strategy_checkpoint_missing",
                    "Authoritative strategy checkpoint provenance is missing for current-run unit-review receipt validation.",
                    "Restore authoritative strategy checkpoint provenance and retry gate-review or gate-finish.",
                );
                return;
            }
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "plain_unit_review_receipt_strategy_checkpoint_missing",
                    error.message,
                    "Restore authoritative strategy checkpoint provenance and retry gate-review or gate-finish.",
                );
                return;
            }
        };

    let latest_attempts = latest_completed_attempts_by_step(&context.evidence);
    let expected_receipt_paths = context
        .steps
        .iter()
        .filter(|step| step.checked)
        .map(|step| {
            (
                authoritative_unit_review_receipt_path(
                    context,
                    execution_run_id,
                    step.task_number,
                    step.step_number,
                )
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_owned(),
                (step.task_number, step.step_number),
            )
        })
        .collect::<BTreeMap<_, _>>();

    for receipt_path in current_run_receipts {
        let Some(receipt_file_name) = receipt_path
            .file_name()
            .and_then(|value| value.to_str())
            .map(str::to_owned)
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipt_malformed",
                "A current-run unit-review receipt has an unreadable filename.",
                "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
            );
            return;
        };
        let Some((task_number, step_number)) =
            expected_receipt_paths.get(&receipt_file_name).copied()
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipt_malformed",
                format!(
                    "Current-run unit-review receipt {} does not match any checked plan step.",
                    receipt_path.display()
                ),
                "Remove or repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
            );
            return;
        };
        let Some(attempt_index) = latest_attempts.get(&(task_number, step_number)).copied() else {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "plain_unit_review_receipt_provenance_mismatch",
                format!(
                    "Current-run unit-review receipt {} has no completed evidence attempt to validate against.",
                    receipt_path.display()
                ),
                "Rebuild the execution evidence for the affected step and retry gate-review or gate-finish.",
            );
            return;
        };
        let attempt = &context.evidence.attempts[attempt_index];
        let Some(expected_task_packet_fingerprint) = attempt
            .packet_fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipt_malformed",
                format!(
                    "Task {} Step {} is missing packet fingerprint provenance required to validate plain unit-review receipts.",
                    task_number, step_number
                ),
                "Repair the execution evidence for the affected step and retry gate-review or gate-finish.",
            );
            return;
        };
        let Some(expected_reviewed_checkpoint_sha) = attempt
            .head_sha
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipt_malformed",
                format!(
                    "Task {} Step {} is missing reviewed checkpoint provenance required to validate plain unit-review receipts.",
                    task_number, step_number
                ),
                "Repair the execution evidence for the affected step and retry gate-review or gate-finish.",
            );
            return;
        };
        let review_source = match fs::read_to_string(&receipt_path) {
            Ok(source) => source,
            Err(error) => {
                gate.fail(
                    FailureClass::ExecutionStateNotReady,
                    "plain_unit_review_receipt_unreadable",
                    format!(
                        "Could not read current-run unit-review receipt {}: {error}",
                        receipt_path.display()
                    ),
                    "Restore the authoritative unit-review receipt and retry gate-review or gate-finish.",
                );
                return;
            }
        };
        if !validate_plain_unit_review_receipt(
            context,
            execution_run_id,
            &review_source,
            &receipt_path,
            PlainUnitReviewReceiptExpectations {
                expected_strategy_checkpoint_fingerprint: expected_strategy_checkpoint_fingerprint
                    .as_str(),
                expected_task_packet_fingerprint,
                expected_reviewed_checkpoint_sha,
                expected_execution_unit_id: serial_execution_unit_id(task_number, step_number),
            },
            gate,
        ) {
            return;
        }
    }
}

fn validate_authoritative_worktree_lease_fingerprint(
    source: &str,
    lease: &WorktreeLease,
    lease_path: String,
    gate: &mut GateState,
) -> bool {
    let Some(canonical_fingerprint) = canonical_worktree_lease_fingerprint(source) else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_fingerprint_unverifiable",
            format!(
                "Authoritative worktree lease fingerprint is unverifiable in {}.",
                lease_path
            ),
            "Repair the authoritative worktree lease artifact and retry gate-review or gate-finish.",
        );
        return false;
    };

    if canonical_fingerprint != lease.lease_fingerprint {
        gate.fail(
            FailureClass::ArtifactIntegrityMismatch,
            "worktree_lease_fingerprint_mismatch",
            format!(
                "Authoritative worktree lease fingerprint does not match canonical content in {}.",
                lease_path
            ),
            "Regenerate the authoritative worktree lease artifact from canonical content and retry gate-review or gate-finish.",
        );
        return false;
    }

    true
}

fn load_authoritative_active_contract(
    context: &ExecutionContext,
    gate: &mut GateState,
) -> Option<(PathBuf, String)> {
    let overlay = match load_status_authoritative_overlay_checked(context) {
        Ok(Some(overlay)) => overlay,
        Ok(None) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_state_unavailable",
                "Authoritative harness state is unavailable for execution-unit review gating.",
                "Restore authoritative harness state readability and retry gate-review or gate-finish.",
            );
            return None;
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_state_unavailable",
                error.message,
                "Restore authoritative harness state readability and retry gate-review or gate-finish.",
            );
            return None;
        }
    };
    let Some(active_contract_path) = overlay
        .active_contract_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_missing",
            "Authoritative harness state is missing the active contract path required to validate execution-unit review provenance.",
            "Restore the authoritative active contract and retry gate-review or gate-finish.",
        );
        return None;
    };
    let Some(active_contract_fingerprint) = overlay
        .active_contract_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_missing",
            "Authoritative harness state is missing the active contract fingerprint required to validate execution-unit review provenance.",
            "Restore the authoritative active contract and retry gate-review or gate-finish.",
        );
        return None;
    };
    if active_contract_path.contains('/') || active_contract_path.contains('\\') {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_path_invalid",
            "Authoritative active contract path must be a normalized relative filename.",
            "Restore the authoritative active contract path and retry gate-review or gate-finish.",
        );
        return None;
    }
    let expected_contract_filename = format!("contract-{active_contract_fingerprint}.md");
    if active_contract_path != expected_contract_filename {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_path_invalid",
            "Authoritative active contract path does not match the active contract fingerprint-derived filename.",
            "Restore the authoritative active contract path and retry gate-review or gate-finish.",
        );
        return None;
    }
    let active_contract_path = harness_authoritative_artifact_path(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
        active_contract_path,
    );
    let active_contract_metadata = match fs::symlink_metadata(&active_contract_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_contract_unreadable",
                format!(
                    "Could not inspect authoritative active contract {}: {error}",
                    active_contract_path.display()
                ),
                "Restore the authoritative active contract and retry gate-review or gate-finish.",
            );
            return None;
        }
    };
    if active_contract_metadata.file_type().is_symlink() || !active_contract_metadata.is_file() {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_unreadable",
            format!(
                "Authoritative active contract must be a regular file in {}.",
                active_contract_path.display()
            ),
            "Restore the authoritative active contract and retry gate-review or gate-finish.",
        );
        return None;
    }
    Some((active_contract_path, active_contract_fingerprint.to_owned()))
}

fn canonical_worktree_lease_fingerprint(source: &str) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(source).ok()?;
    let object = value.as_object_mut()?;
    object.remove("lease_fingerprint");
    serde_json::to_vec(&value)
        .ok()
        .map(|bytes| sha256_hex(&bytes))
}

fn worktree_lease_execution_context_key(
    execution_run_id: &str,
    execution_unit_id: &str,
    source_plan_path: &str,
    source_plan_revision: u32,
    authoritative_integration_branch: &str,
    reviewed_checkpoint_commit_sha: &str,
) -> String {
    sha256_hex(
        format!(
            "run={execution_run_id}\nunit={execution_unit_id}\nplan={source_plan_path}\nplan_revision={source_plan_revision}\nbranch={authoritative_integration_branch}\nreviewed_checkpoint={reviewed_checkpoint_commit_sha}\n"
        )
        .as_bytes(),
    )
}

fn serial_execution_unit_id(task_number: u32, step_number: u32) -> String {
    format!("task-{task_number}-step-{step_number}")
}

fn serial_unit_review_lease_fingerprint(
    execution_run_id: &str,
    execution_unit_id: &str,
    execution_context_key: &str,
    reviewed_checkpoint_commit_sha: &str,
    approved_task_packet_fingerprint: &str,
    approved_unit_contract_fingerprint: &str,
) -> String {
    sha256_hex(
        format!(
            "serial-unit-review:{execution_run_id}:{execution_unit_id}:{execution_context_key}:{reviewed_checkpoint_commit_sha}:{approved_task_packet_fingerprint}:{approved_unit_contract_fingerprint}"
        )
        .as_bytes(),
    )
}

fn approved_unit_contract_fingerprint_for_review(
    active_contract_fingerprint: &str,
    approved_task_packet_fingerprint: &str,
    execution_unit_id: &str,
) -> String {
    sha256_hex(
        format!(
            "approved-unit-contract:{active_contract_fingerprint}:{approved_task_packet_fingerprint}:{execution_unit_id}"
        )
            .as_bytes(),
    )
}

fn reconcile_result_proof_fingerprint_for_review(
    repo_root: &Path,
    reconcile_result_commit_sha: &str,
) -> Option<String> {
    commit_object_fingerprint(repo_root, reconcile_result_commit_sha)
}

fn enforce_serial_unit_review_truth(
    context: &ExecutionContext,
    run_identity: &WorktreeLeaseRunIdentityProbe,
    active_contract_fingerprint: &str,
    gate: &mut GateState,
) {
    let latest_attempts = latest_completed_attempts_by_step(&context.evidence);
    for step in context.steps.iter().filter(|step| step.checked) {
        let Some(attempt_index) = latest_attempts
            .get(&(step.task_number, step.step_number))
            .copied()
        else {
            continue;
        };
        let attempt = &context.evidence.attempts[attempt_index];
        let Some(approved_task_packet_fingerprint) = attempt
            .packet_fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_task_packet_missing",
                format!(
                    "Task {} Step {} is missing the packet fingerprint required for serial unit-review gating.",
                    step.task_number, step.step_number
                ),
                "Rebuild the execution evidence for the completed step and retry gate-review or gate-finish.",
            );
            return;
        };
        let Some(reviewed_checkpoint_commit_sha) = attempt
            .head_sha
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_head_missing",
                format!(
                    "Task {} Step {} is missing the reviewed checkpoint SHA required for serial unit-review gating.",
                    step.task_number, step.step_number
                ),
                "Rebuild the execution evidence for the completed step and retry gate-review or gate-finish.",
            );
            return;
        };
        let execution_unit_id = serial_execution_unit_id(step.task_number, step.step_number);
        let expected_execution_context_key = worktree_lease_execution_context_key(
            &run_identity.execution_run_id,
            &execution_unit_id,
            &context.plan_rel,
            context.plan_document.plan_revision,
            &context.runtime.branch_name,
            reviewed_checkpoint_commit_sha,
        );
        let approved_unit_contract_fingerprint = approved_unit_contract_fingerprint_for_review(
            active_contract_fingerprint,
            approved_task_packet_fingerprint,
            &execution_unit_id,
        );
        let Some(reconcile_result_proof_fingerprint) =
            reconcile_result_proof_fingerprint_for_review(
                &context.runtime.repo_root,
                reviewed_checkpoint_commit_sha,
            )
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_reconcile_proof_unverifiable",
                format!(
                    "Task {} Step {} serial unit-review reconcile proof could not be verified against repository history.",
                    step.task_number, step.step_number
                ),
                "Restore repository history readability and retry gate-review or gate-finish.",
            );
            return;
        };
        let review_receipt_path = harness_authoritative_artifact_path(
            &context.runtime.state_dir,
            &context.runtime.repo_slug,
            &context.runtime.branch_name,
            &format!(
                "unit-review-{}-{}.md",
                run_identity.execution_run_id, execution_unit_id
            ),
        );
        let review_metadata = match fs::symlink_metadata(&review_receipt_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                gate.fail(
                    FailureClass::ExecutionStateNotReady,
                    "serial_unit_review_receipt_missing",
                    format!(
                        "Task {} Step {} is missing its authoritative serial unit-review receipt {}: {error}",
                        step.task_number,
                        step.step_number,
                        review_receipt_path.display()
                    ),
                    "Record a dedicated-independent serial unit-review receipt for the completed execution unit and retry gate-review or gate-finish.",
                );
                return;
            }
        };
        if review_metadata.file_type().is_symlink() || !review_metadata.is_file() {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_receipt_path_invalid",
                format!(
                    "Task {} Step {} serial unit-review receipt must be a regular file in {}.",
                    step.task_number,
                    step.step_number,
                    review_receipt_path.display()
                ),
                "Restore the authoritative serial unit-review receipt and retry gate-review or gate-finish.",
            );
            return;
        }
        let review_source = match fs::read_to_string(&review_receipt_path) {
            Ok(source) => source,
            Err(error) => {
                gate.fail(
                    FailureClass::ExecutionStateNotReady,
                    "serial_unit_review_receipt_unreadable",
                    format!(
                        "Could not read authoritative serial unit-review receipt {}: {error}",
                        review_receipt_path.display()
                    ),
                    "Restore the authoritative serial unit-review receipt and retry gate-review or gate-finish.",
                );
                return;
            }
        };
        let Some(review_receipt_fingerprint) =
            canonical_unit_review_receipt_fingerprint(&review_source)
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_receipt_fingerprint_unverifiable",
                format!(
                    "Task {} Step {} serial unit-review receipt fingerprint is unverifiable in {}.",
                    step.task_number,
                    step.step_number,
                    review_receipt_path.display()
                ),
                "Regenerate the authoritative serial unit-review receipt from canonical content and retry gate-review or gate-finish.",
            );
            return;
        };
        let pseudo_lease = WorktreeLease {
            lease_version: WORKTREE_LEASE_VERSION,
            authoritative_sequence: INITIAL_AUTHORITATIVE_SEQUENCE + 1,
            execution_run_id: run_identity.execution_run_id.clone(),
            execution_context_key: expected_execution_context_key.clone(),
            source_plan_path: context.plan_rel.clone(),
            source_plan_revision: context.plan_document.plan_revision,
            execution_unit_id: execution_unit_id.clone(),
            source_branch: context.runtime.branch_name.clone(),
            authoritative_integration_branch: context.runtime.branch_name.clone(),
            worktree_path: fs::canonicalize(&context.runtime.repo_root)
                .unwrap_or_else(|_| context.runtime.repo_root.clone())
                .display()
                .to_string(),
            repo_state_baseline_head_sha: reviewed_checkpoint_commit_sha.to_owned(),
            repo_state_baseline_worktree_fingerprint: approved_task_packet_fingerprint.to_owned(),
            lease_state: WorktreeLeaseState::Cleaned,
            cleanup_state: String::from("cleaned"),
            reviewed_checkpoint_commit_sha: Some(reviewed_checkpoint_commit_sha.to_owned()),
            reconcile_result_commit_sha: Some(reviewed_checkpoint_commit_sha.to_owned()),
            reconcile_result_proof_fingerprint: Some(reconcile_result_proof_fingerprint.clone()),
            reconcile_mode: String::from("identity_preserving"),
            generated_by: String::from("featureforge:executing-plans"),
            generated_at: String::from("runtime-derived"),
            lease_fingerprint: serial_unit_review_lease_fingerprint(
                &run_identity.execution_run_id,
                &execution_unit_id,
                &expected_execution_context_key,
                reviewed_checkpoint_commit_sha,
                approved_task_packet_fingerprint,
                &approved_unit_contract_fingerprint,
            ),
        };
        let (receipt_checkpoint_commit_sha, receipt_reconciled_result_commit_sha) =
            match validate_authoritative_unit_review_receipt(
                context,
                &run_identity.execution_run_id,
                &pseudo_lease,
                &review_source,
                &review_receipt_path,
                UnitReviewReceiptExpectations {
                    expected_execution_context_key: &expected_execution_context_key,
                    expected_fingerprint: &review_receipt_fingerprint,
                    expected_task_packet_fingerprint: approved_task_packet_fingerprint,
                    expected_approved_unit_contract_fingerprint:
                        &approved_unit_contract_fingerprint,
                    expected_reconcile_result_commit_sha: reviewed_checkpoint_commit_sha,
                },
                gate,
            ) {
                Some(values) => values,
                None => return,
            };
        if receipt_checkpoint_commit_sha != reviewed_checkpoint_commit_sha {
            gate.fail(
                FailureClass::StaleProvenance,
                "serial_unit_review_receipt_checkpoint_mismatch",
                format!(
                    "Task {} Step {} serial unit-review receipt does not bind the completed step checkpoint.",
                    step.task_number, step.step_number
                ),
                "Regenerate the authoritative serial unit-review receipt from the completed step checkpoint and retry gate-review or gate-finish.",
            );
            return;
        }
        if receipt_reconciled_result_commit_sha != reviewed_checkpoint_commit_sha {
            gate.fail(
                FailureClass::StaleProvenance,
                "serial_unit_review_receipt_reconcile_result_mismatch",
                format!(
                    "Task {} Step {} serial unit-review receipt does not bind the completed step result commit.",
                    step.task_number, step.step_number
                ),
                "Regenerate the authoritative serial unit-review receipt from the completed step result and retry gate-review or gate-finish.",
            );
            return;
        }
    }
}

struct UnitReviewReceiptExpectations<'a> {
    expected_execution_context_key: &'a str,
    expected_fingerprint: &'a str,
    expected_task_packet_fingerprint: &'a str,
    expected_approved_unit_contract_fingerprint: &'a str,
    expected_reconcile_result_commit_sha: &'a str,
}

struct PlainUnitReviewReceiptExpectations<'a> {
    expected_strategy_checkpoint_fingerprint: &'a str,
    expected_task_packet_fingerprint: &'a str,
    expected_reviewed_checkpoint_sha: &'a str,
    expected_execution_unit_id: String,
}

fn validate_authoritative_unit_review_receipt(
    context: &ExecutionContext,
    execution_run_id: &str,
    lease: &WorktreeLease,
    source: &str,
    receipt_path: &Path,
    expectations: UnitReviewReceiptExpectations<'_>,
    gate: &mut GateState,
) -> Option<(String, String)> {
    let review_document = parse_artifact_document(receipt_path);
    if review_document.title.as_deref() != Some("# Unit Review Result") {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_malformed",
            "The authoritative unit-review receipt is malformed.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Review Stage")
        .map(String::as_str)
        != Some("featureforge:unit-review")
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_stage_mismatch",
            "The authoritative unit-review receipt has the wrong review stage.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Reviewer Provenance")
        .map(String::as_str)
        != Some("dedicated-independent")
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_not_dedicated",
            "The authoritative unit-review receipt is not dedicated-independent.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Source Plan")
        .map(String::as_str)
        != Some(context.plan_rel.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_plan_mismatch",
            "The authoritative unit-review receipt does not match the current plan.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Source Plan Revision")
        .and_then(|value| value.parse::<u32>().ok())
        != Some(context.plan_document.plan_revision)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_plan_revision_mismatch",
            "The authoritative unit-review receipt does not match the current plan revision.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Execution Run ID")
        .map(String::as_str)
        != Some(execution_run_id)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_run_mismatch",
            "The authoritative unit-review receipt does not match the current execution run.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Execution Unit ID")
        .map(String::as_str)
        != Some(lease.execution_unit_id.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_unit_mismatch",
            "The authoritative unit-review receipt does not match the reviewed execution unit.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Lease Fingerprint")
        .map(String::as_str)
        != Some(lease.lease_fingerprint.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_lease_fingerprint_mismatch",
            "The authoritative unit-review receipt does not match the reviewed lease fingerprint.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Execution Context Key")
        .map(String::as_str)
        != Some(expectations.expected_execution_context_key)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_context_key_mismatch",
            "The authoritative unit-review receipt does not match the current execution context.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Approved Task Packet Fingerprint")
        .map(String::as_str)
        != Some(expectations.expected_task_packet_fingerprint)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_task_packet_mismatch",
            "The authoritative unit-review receipt does not match the approved task packet.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Approved Unit Contract Fingerprint")
        .map(String::as_str)
        != Some(expectations.expected_approved_unit_contract_fingerprint)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_unit_contract_mismatch",
            "The authoritative unit-review receipt does not bind the approved unit contract.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if expectations.expected_approved_unit_contract_fingerprint
        == expectations.expected_task_packet_fingerprint
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_unit_contract_mismatch",
            "The authoritative unit-review receipt must bind a distinct approved unit contract fingerprint.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Reconcile Mode")
        .map(String::as_str)
        != Some("identity_preserving")
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_reconcile_mode_mismatch",
            "The authoritative unit-review receipt does not prove an identity-preserving reconcile.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Reconciled Result SHA")
        .map(String::as_str)
        != Some(expectations.expected_reconcile_result_commit_sha)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_identity_preserving_proof_mismatch",
            "The authoritative unit-review receipt does not bind the exact reconciled commit.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    let Some(expected_reconcile_result_proof_fingerprint) =
        reconcile_result_proof_fingerprint_for_review(
            &context.runtime.repo_root,
            expectations.expected_reconcile_result_commit_sha,
        )
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_identity_preserving_proof_unverifiable",
            "The authoritative unit-review receipt exact reconcile proof could not be verified against repository history.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    };
    if review_document
        .headers
        .get("Reconcile Result Proof Fingerprint")
        .map(String::as_str)
        != Some(expected_reconcile_result_proof_fingerprint.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_identity_preserving_proof_mismatch",
            "The authoritative unit-review receipt does not bind the exact reconciled commit object.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Reviewed Worktree")
        .map(String::as_str)
        != Some(lease.worktree_path.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_worktree_mismatch",
            "The authoritative unit-review receipt does not match the reviewed worktree.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document.headers.get("Result").map(String::as_str) != Some("pass") {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_not_pass",
            "The authoritative unit-review receipt is not marked pass.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Generated By")
        .map(String::as_str)
        != Some("featureforge:unit-review")
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_generator_mismatch",
            "The authoritative unit-review receipt does not come from the unit-review generator.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    let expected_receipt_filename = format!(
        "unit-review-{}-{}.md",
        execution_run_id,
        lease.execution_unit_id.trim_start_matches("unit-")
    );
    if receipt_path.file_name().and_then(|value| value.to_str())
        != Some(expected_receipt_filename.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_binding_path_invalid",
            "The authoritative unit-review receipt path does not match the reviewed execution unit provenance.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    let Some(receipt_checkpoint_commit_sha) = review_document
        .headers
        .get("Reviewed Checkpoint SHA")
        .cloned()
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_head_missing",
            "The authoritative unit-review receipt is missing its reviewed checkpoint.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    };

    let Some(canonical_fingerprint) = canonical_unit_review_receipt_fingerprint(source) else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_fingerprint_unverifiable",
            format!(
                "Authoritative unit-review receipt fingerprint is unverifiable in {}.",
                receipt_path.display()
            ),
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    };
    if canonical_fingerprint != expectations.expected_fingerprint {
        gate.fail(
            FailureClass::ArtifactIntegrityMismatch,
            "worktree_lease_review_receipt_fingerprint_mismatch",
            format!(
                "Authoritative unit-review receipt fingerprint does not match canonical content in {}.",
                receipt_path.display()
            ),
            "Regenerate the authoritative unit-review receipt from canonical content and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document
        .headers
        .get("Receipt Fingerprint")
        .map(String::as_str)
        != Some(expectations.expected_fingerprint)
    {
        gate.fail(
            FailureClass::ArtifactIntegrityMismatch,
            "worktree_lease_review_receipt_fingerprint_mismatch",
            format!(
                "Authoritative unit-review receipt fingerprint header does not match canonical content in {}.",
                receipt_path.display()
            ),
            "Regenerate the authoritative unit-review receipt from canonical content and retry gate-review or gate-finish.",
        );
        return None;
    }

    Some((
        receipt_checkpoint_commit_sha,
        expectations.expected_reconcile_result_commit_sha.to_owned(),
    ))
}

fn validate_plain_unit_review_receipt(
    context: &ExecutionContext,
    execution_run_id: &str,
    source: &str,
    receipt_path: &Path,
    expectations: PlainUnitReviewReceiptExpectations<'_>,
    gate: &mut GateState,
) -> bool {
    let review_document = parse_artifact_document(receipt_path);
    if review_document.title.as_deref() != Some("# Unit Review Result")
        || review_document
            .headers
            .get("Review Stage")
            .map(String::as_str)
            != Some("featureforge:unit-review")
        || review_document
            .headers
            .get("Reviewer Provenance")
            .map(String::as_str)
            != Some("dedicated-independent")
        || !matches!(
            review_document
                .headers
                .get("Reviewer Source")
                .map(String::as_str)
                .unwrap_or_default(),
            "fresh-context-subagent" | "cross-model"
        )
        || review_document.headers.get("Result").map(String::as_str) != Some("pass")
        || review_document
            .headers
            .get("Generated By")
            .map(String::as_str)
            != Some("featureforge:unit-review")
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "plain_unit_review_receipt_malformed",
            format!(
                "Current-run unit-review receipt {} is malformed.",
                receipt_path.display()
            ),
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return false;
    }

    for forbidden_header in [
        "Lease Fingerprint",
        "Execution Context Key",
        "Approved Unit Contract Fingerprint",
        "Reconciled Result SHA",
        "Reconcile Result Proof Fingerprint",
        "Reconcile Mode",
        "Reviewed Worktree",
    ] {
        if review_document.headers.contains_key(forbidden_header) {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipt_malformed",
                format!(
                    "Current-run unit-review receipt {} unexpectedly includes {} without an active authoritative contract.",
                    receipt_path.display(),
                    forbidden_header
                ),
                "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
            );
            return false;
        }
    }

    let expected_file_name = format!(
        "unit-review-{}-{}.md",
        execution_run_id, expectations.expected_execution_unit_id
    );
    if receipt_path.file_name().and_then(|value| value.to_str())
        != Some(expected_file_name.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "plain_unit_review_receipt_malformed",
            format!(
                "Current-run unit-review receipt path {} does not match the reviewed execution unit provenance.",
                receipt_path.display()
            ),
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return false;
    }

    let Some(canonical_fingerprint) = canonical_unit_review_receipt_fingerprint(source) else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "plain_unit_review_receipt_fingerprint_unverifiable",
            format!(
                "Current-run unit-review receipt fingerprint is unverifiable in {}.",
                receipt_path.display()
            ),
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return false;
    };
    if review_document
        .headers
        .get("Receipt Fingerprint")
        .map(String::as_str)
        != Some(canonical_fingerprint.as_str())
    {
        gate.fail(
            FailureClass::ArtifactIntegrityMismatch,
            "plain_unit_review_receipt_fingerprint_mismatch",
            format!(
                "Current-run unit-review receipt fingerprint header does not match canonical content in {}.",
                receipt_path.display()
            ),
            "Regenerate the authoritative unit-review receipt from canonical content and retry gate-review or gate-finish.",
        );
        return false;
    }

    let mut mismatched_fields = Vec::new();
    let mut mismatch_details = Vec::new();
    if review_document
        .headers
        .get("Source Plan")
        .map(String::as_str)
        != Some(context.plan_rel.as_str())
    {
        mismatched_fields.push("Source Plan");
        mismatch_details.push(format!(
            "Source Plan expected={} actual={}",
            context.plan_rel,
            review_document
                .headers
                .get("Source Plan")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Source Plan Revision")
        .and_then(|value| value.parse::<u32>().ok())
        != Some(context.plan_document.plan_revision)
    {
        mismatched_fields.push("Source Plan Revision");
        mismatch_details.push(format!(
            "Source Plan Revision expected={} actual={}",
            context.plan_document.plan_revision,
            review_document
                .headers
                .get("Source Plan Revision")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Execution Run ID")
        .map(String::as_str)
        != Some(execution_run_id)
    {
        mismatched_fields.push("Execution Run ID");
        mismatch_details.push(format!(
            "Execution Run ID expected={} actual={}",
            execution_run_id,
            review_document
                .headers
                .get("Execution Run ID")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Execution Unit ID")
        .map(String::as_str)
        != Some(expectations.expected_execution_unit_id.as_str())
    {
        mismatched_fields.push("Execution Unit ID");
        mismatch_details.push(format!(
            "Execution Unit ID expected={} actual={}",
            expectations.expected_execution_unit_id,
            review_document
                .headers
                .get("Execution Unit ID")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Strategy Checkpoint Fingerprint")
        .map(String::as_str)
        != Some(expectations.expected_strategy_checkpoint_fingerprint)
    {
        mismatched_fields.push("Strategy Checkpoint Fingerprint");
        mismatch_details.push(format!(
            "Strategy Checkpoint Fingerprint expected={} actual={}",
            expectations.expected_strategy_checkpoint_fingerprint,
            review_document
                .headers
                .get("Strategy Checkpoint Fingerprint")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Approved Task Packet Fingerprint")
        .map(String::as_str)
        != Some(expectations.expected_task_packet_fingerprint)
    {
        mismatched_fields.push("Approved Task Packet Fingerprint");
        mismatch_details.push(format!(
            "Approved Task Packet Fingerprint expected={} actual={}",
            expectations.expected_task_packet_fingerprint,
            review_document
                .headers
                .get("Approved Task Packet Fingerprint")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Reviewed Checkpoint SHA")
        .map(String::as_str)
        != Some(expectations.expected_reviewed_checkpoint_sha)
    {
        mismatched_fields.push("Reviewed Checkpoint SHA");
        mismatch_details.push(format!(
            "Reviewed Checkpoint SHA expected={} actual={}",
            expectations.expected_reviewed_checkpoint_sha,
            review_document
                .headers
                .get("Reviewed Checkpoint SHA")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if !mismatched_fields.is_empty() {
        gate.fail(
            FailureClass::StaleProvenance,
            "plain_unit_review_receipt_provenance_mismatch",
            format!(
                "Current-run unit-review receipt {} does not match the active task checkpoint provenance (mismatched fields: {}; details: {}).",
                receipt_path.display(),
                mismatched_fields.join(", ")
                , mismatch_details.join("; ")
            ),
            "Regenerate the authoritative unit-review receipt for the completed step and retry gate-review or gate-finish.",
        );
        return false;
    }

    true
}

fn canonical_unit_review_receipt_fingerprint(source: &str) -> Option<String> {
    let filtered = source
        .lines()
        .filter(|line| !line.trim().starts_with("**Receipt Fingerprint:**"))
        .collect::<Vec<_>>()
        .join("\n");
    Some(sha256_hex(filtered.as_bytes()))
}

fn is_ancestor_commit(repo_root: &Path, ancestor: &str, descendant: &str) -> bool {
    shared_is_ancestor_commit(repo_root, ancestor, descendant)
}

struct ArtifactGateValidationFailure {
    failure_class: FailureClass,
    message: String,
}

fn summary_hash_matches(summary: &str, expected_hash: &str) -> bool {
    sha256_hex(normalize_summary_content(summary).as_bytes()) == expected_hash
}

fn validate_current_branch_test_plan_candidate_for_finish(
    context: &ExecutionContext,
    test_plan_path: &Path,
    current_head: &str,
) -> Result<(), ArtifactGateValidationFailure> {
    let test_plan = parse_artifact_document(test_plan_path);
    if test_plan.title.as_deref() != Some("# Test Plan") {
        return Err(ArtifactGateValidationFailure {
            failure_class: FailureClass::QaArtifactNotFresh,
            message: String::from("The latest test-plan artifact is malformed."),
        });
    }
    if test_plan.headers.get("Generated By") != Some(&String::from("featureforge:plan-eng-review"))
    {
        return Err(ArtifactGateValidationFailure {
            failure_class: FailureClass::QaArtifactNotFresh,
            message: String::from(
                "The latest test-plan artifact was not generated by plan-eng-review.",
            ),
        });
    }
    let expected_source_plan = format!("`{}`", context.plan_rel);
    let expected_source_plan_revision = context.plan_document.plan_revision.to_string();
    let expected_branch = &context.runtime.branch_name;
    let expected_repo = &context.runtime.repo_slug;
    let head_matches = test_plan
        .headers
        .get("Head SHA")
        .is_some_and(|value| value == current_head);
    if test_plan.headers.get("Source Plan") != Some(&expected_source_plan)
        || test_plan.headers.get("Source Plan Revision") != Some(&expected_source_plan_revision)
        || test_plan.headers.get("Branch") != Some(expected_branch)
        || test_plan.headers.get("Repo") != Some(expected_repo)
        || !head_matches
    {
        let message = if !head_matches {
            "The latest test-plan artifact does not match the current HEAD."
        } else if test_plan.headers.get("Source Plan") != Some(&expected_source_plan) {
            "The latest test-plan artifact does not match the current approved plan path."
        } else if test_plan.headers.get("Source Plan Revision")
            != Some(&expected_source_plan_revision)
        {
            "The latest test-plan artifact does not match the current approved plan revision."
        } else if test_plan.headers.get("Branch") != Some(expected_branch) {
            "The latest test-plan artifact does not match the current branch."
        } else {
            "The latest test-plan artifact does not match the current repo."
        };
        return Err(ArtifactGateValidationFailure {
            failure_class: FailureClass::QaArtifactNotFresh,
            message: String::from(message),
        });
    }
    Ok(())
}

pub(crate) fn current_test_plan_artifact_path_for_qa_recording(
    context: &ExecutionContext,
) -> Result<PathBuf, JsonFailure> {
    let current_head = context.current_head_sha()?;
    let Some(authoritative_path) =
        select_authoritative_test_plan_artifact_candidate_for_qa_recording(context, &current_head)
            .map_err(|failure| JsonFailure::new(failure.failure_class, failure.message))?
    else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            "Current late-stage recording requires a current authoritative test-plan artifact for the current branch.",
        ));
    };
    Ok(authoritative_path)
}

pub(crate) fn qa_pending_requires_test_plan_refresh(
    context: &ExecutionContext,
    gate_finish: Option<&GateResult>,
) -> bool {
    let _ = context;
    finish_requires_test_plan_refresh(gate_finish)
}

fn select_authoritative_test_plan_artifact_candidate_for_qa_recording(
    context: &ExecutionContext,
    current_head: &str,
) -> Result<Option<PathBuf>, ArtifactGateValidationFailure> {
    let artifacts_dir = harness_authoritative_artifacts_dir(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
    );
    let entries = match fs::read_dir(&artifacts_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ArtifactGateValidationFailure {
                failure_class: FailureClass::MalformedExecutionState,
                message: format!(
                    "Could not inspect authoritative test-plan artifacts in {}: {error}",
                    artifacts_dir.display()
                ),
            });
        }
    };
    let mut candidate_paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| ArtifactGateValidationFailure {
            failure_class: FailureClass::MalformedExecutionState,
            message: format!(
                "Could not inspect authoritative test-plan artifacts in {}: {error}",
                artifacts_dir.display()
            ),
        })?;
        let candidate_path = entry.path();
        let Some(file_name) = candidate_path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with("test-plan-") || !file_name.ends_with(".md") {
            continue;
        }
        let metadata = match fs::symlink_metadata(&candidate_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                return Err(ArtifactGateValidationFailure {
                    failure_class: FailureClass::MalformedExecutionState,
                    message: format!(
                        "Could not inspect authoritative test-plan artifact {}: {error}",
                        candidate_path.display()
                    ),
                });
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ArtifactGateValidationFailure {
                failure_class: FailureClass::MalformedExecutionState,
                message: format!(
                    "Authoritative test-plan artifact {} must be a regular file.",
                    candidate_path.display()
                ),
            });
        }
        candidate_paths.push(candidate_path);
    }
    candidate_paths.sort();
    let mut first_failure = None;
    for candidate_path in candidate_paths {
        match validate_current_branch_test_plan_candidate_for_finish(
            context,
            &candidate_path,
            current_head,
        ) {
            Ok(()) => return Ok(Some(candidate_path)),
            Err(failure) if first_failure.is_none() => first_failure = Some(failure),
            Err(_) => {}
        }
    }
    match first_failure {
        Some(failure) => Err(failure),
        None => Ok(None),
    }
}

fn require_current_release_readiness_ready_for_finish(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    current_branch_closure_id: &str,
    current_branch_reviewed_state_id: &str,
    current_base_branch: &str,
    gate: &mut GateState,
) -> bool {
    let Some(current_release_readiness_record_id) = authoritative_state
        .current_release_readiness_record_id()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_docs_state_missing",
            "Finish readiness requires a current release-readiness milestone for the current branch closure.",
            "Run document-release and return with a fresh release-readiness result.",
        );
        return false;
    };
    let Some(record) =
        authoritative_state.release_readiness_record_by_id(&current_release_readiness_record_id)
    else {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_docs_state_missing",
            "Finish readiness requires a current release-readiness milestone for the current branch closure.",
            "Run document-release and return with a fresh release-readiness result.",
        );
        return false;
    };
    if record.record_status != "current" {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "release_artifact_malformed",
            "The current release-readiness record is not marked current.",
            "Repair the authoritative release-readiness record and retry gate-finish.",
        );
        return false;
    }
    if !summary_hash_matches(&record.summary, &record.summary_hash) {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "release_artifact_malformed",
            "The current release-readiness record has an invalid summary hash binding.",
            "Repair the authoritative release-readiness record and retry gate-finish.",
        );
        return false;
    }
    if record.branch_closure_id != current_branch_closure_id {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_docs_state_missing",
            "The current release-readiness milestone is not bound to the still-current branch closure.",
            "Run document-release for the current branch closure and retry gate-finish.",
        );
        return false;
    }
    if record.source_plan_path != context.plan_rel
        || record.source_plan_revision != context.plan_document.plan_revision
    {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_artifact_plan_mismatch",
            "The current release-readiness milestone does not match the current approved plan revision.",
            "Run document-release for the current approved plan revision.",
        );
        return false;
    }
    if record.branch_name != context.runtime.branch_name {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_artifact_branch_mismatch",
            "The current release-readiness milestone does not match the current branch.",
            "Run document-release for the current approved plan revision.",
        );
        return false;
    }
    if record.base_branch.trim().is_empty() {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_artifact_base_branch_unresolved",
            "The current release-readiness milestone is missing its base branch binding.",
            "Run document-release for the current approved plan revision.",
        );
        return false;
    }
    if record.base_branch != current_base_branch {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_artifact_base_branch_mismatch",
            "The current release-readiness milestone does not match the expected base branch.",
            "Run document-release for the current approved plan revision.",
        );
        return false;
    }
    if record.repo_slug != context.runtime.repo_slug {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_artifact_repo_mismatch",
            "The current release-readiness milestone does not match the current repo.",
            "Run document-release for the current approved plan revision.",
        );
        return false;
    }
    if record.reviewed_state_id != current_branch_reviewed_state_id {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_artifact_head_mismatch",
            "The current release-readiness milestone does not match the current reviewed branch state.",
            "Run document-release for the current branch closure and retry gate-finish.",
        );
        return false;
    }
    if record.generated_by_identity != "featureforge/release-readiness" {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_artifact_generator_mismatch",
            "The current release-readiness milestone has an invalid generated-by identity.",
            "Repair the authoritative release-readiness record and retry gate-finish.",
        );
        return false;
    }
    if record.result != "ready" {
        gate.fail(
            FailureClass::ReleaseArtifactNotFresh,
            "release_result_not_pass",
            "The current release-readiness milestone is not ready.",
            "Resolve the release blocker and rerun document-release.",
        );
        return false;
    }
    true
}

fn require_current_final_review_pass_for_finish(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    current_branch_closure_id: &str,
    current_branch_reviewed_state_id: &str,
    current_base_branch: &str,
    gate: &mut GateState,
) -> bool {
    let Some(current_release_readiness_record_id) = authoritative_state
        .current_release_readiness_record_id()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_artifact_release_binding_missing",
            "The current final-review milestone is missing its release-readiness dependency binding.",
            "Run document-release, then rerun requesting-code-review for the current branch closure.",
        );
        return false;
    };
    let Some(current_final_review_record_id) = authoritative_state
        .current_final_review_record_id()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_artifact_missing",
            "Finish readiness requires a current final-review milestone for the current branch closure.",
            "Run requesting-code-review and return with a fresh final-review result.",
        );
        return false;
    };
    let Some(record) =
        authoritative_state.final_review_record_by_id(&current_final_review_record_id)
    else {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_artifact_missing",
            "Finish readiness requires a current final-review milestone for the current branch closure.",
            "Run requesting-code-review and return with a fresh final-review result.",
        );
        return false;
    };
    if record.record_status != "current" {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "review_artifact_malformed",
            "The current final-review record is not marked current.",
            "Repair the authoritative final-review record and retry gate-finish.",
        );
        return false;
    }
    if !summary_hash_matches(&record.summary, &record.summary_hash) {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "review_artifact_malformed",
            "The current final-review record has an invalid summary hash binding.",
            "Repair the authoritative final-review record and retry gate-finish.",
        );
        return false;
    }
    if record.branch_closure_id != current_branch_closure_id {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_artifact_missing",
            "The current final-review milestone is not bound to the still-current branch closure.",
            "Run requesting-code-review and return with a fresh final-review result.",
        );
        return false;
    }
    if record.release_readiness_record_id.as_deref()
        != Some(current_release_readiness_record_id.as_str())
    {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_artifact_release_binding_mismatch",
            "The current final-review milestone is not bound to the current release-readiness milestone identity.",
            "Run requesting-code-review and return with a fresh final-review result.",
        );
        return false;
    }
    if record.source_plan_path != context.plan_rel
        || record.source_plan_revision != context.plan_document.plan_revision
    {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_artifact_plan_mismatch",
            "The current final-review milestone does not match the current approved plan revision.",
            "Run requesting-code-review and return with a fresh final-review result.",
        );
        return false;
    }
    if record.branch_name != context.runtime.branch_name {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_artifact_branch_mismatch",
            "The current final-review milestone does not match the current branch.",
            "Run requesting-code-review and return with a fresh final-review result.",
        );
        return false;
    }
    if record.base_branch.trim().is_empty() {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_artifact_base_branch_unresolved",
            "The current final-review milestone is missing its base branch binding.",
            "Run requesting-code-review and return with a fresh final-review result.",
        );
        return false;
    }
    if record.base_branch != current_base_branch {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_artifact_base_branch_mismatch",
            "The current final-review milestone does not match the expected base branch.",
            "Run requesting-code-review and return with a fresh final-review result.",
        );
        return false;
    }
    if record.repo_slug != context.runtime.repo_slug {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_artifact_repo_mismatch",
            "The current final-review milestone does not match the current repo.",
            "Run requesting-code-review and return with a fresh final-review result.",
        );
        return false;
    }
    if record.reviewed_state_id != current_branch_reviewed_state_id {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_artifact_reviewed_state_mismatch",
            "The current final-review milestone does not match the current reviewed branch state.",
            "Run requesting-code-review and return with a fresh final-review result.",
        );
        return false;
    }
    if !shared_reviewer_source_is_valid(&record.reviewer_source) {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "review_artifact_reviewer_source_invalid",
            "The current final-review milestone has an invalid reviewer source.",
            "Repair the authoritative final-review record and retry gate-finish.",
        );
        return false;
    }
    if record.dispatch_id.trim().is_empty() || record.reviewer_id.trim().is_empty() {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "review_artifact_malformed",
            "The current final-review milestone is missing dispatch or reviewer identity bindings.",
            "Repair the authoritative final-review record and retry gate-finish.",
        );
        return false;
    }
    let expected_browser_qa_required = match context.plan_document.qa_requirement.as_deref() {
        Some("required") => Some(true),
        Some("not-required") => Some(false),
        _ => None,
    };
    if record.browser_qa_required != expected_browser_qa_required {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "review_artifact_malformed",
            "The current final-review milestone has a QA-requirement binding that does not match the approved plan.",
            "Repair the authoritative final-review record and retry gate-finish.",
        );
        return false;
    }
    if record.result != "pass" {
        gate.fail(
            FailureClass::ReviewArtifactNotFresh,
            "review_result_not_pass",
            "The current final-review milestone is not pass.",
            "Address the final-review findings and rerun requesting-code-review.",
        );
        return false;
    }
    if !validate_current_final_review_authoritative_bindings(context, &record, gate) {
        return false;
    }
    true
}

fn validate_current_final_review_authoritative_bindings(
    context: &ExecutionContext,
    record: &crate::execution::transitions::CurrentFinalReviewRecord,
    gate: &mut GateState,
) -> bool {
    let Some(final_review_fingerprint) = record
        .final_review_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "review_artifact_malformed",
            "The current final-review milestone is missing its authoritative artifact fingerprint binding.",
            "Repair the authoritative final-review record and retry gate-finish.",
        );
        return false;
    };
    if !is_canonical_sha256_fingerprint(final_review_fingerprint) {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "review_artifact_malformed",
            "The current final-review milestone has a non-canonical authoritative artifact fingerprint binding.",
            "Repair the authoritative final-review record and retry gate-finish.",
        );
        return false;
    }
    let expected_strategy_checkpoint_fingerprint =
        match authoritative_strategy_checkpoint_fingerprint_checked(context) {
            Ok(Some(fingerprint)) if !fingerprint.trim().is_empty() => fingerprint,
            Ok(_) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "review_artifact_malformed",
                    "Authoritative strategy checkpoint provenance is missing for current final-review validation.",
                    "Restore authoritative strategy checkpoint provenance and retry gate-finish.",
                );
                return false;
            }
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "review_artifact_malformed",
                    error.message,
                    "Restore authoritative strategy checkpoint provenance and retry gate-finish.",
                );
                return false;
            }
        };
    let expected_strategy_checkpoint_fingerprint =
        expected_strategy_checkpoint_fingerprint.trim().to_owned();
    if !is_canonical_sha256_fingerprint(&expected_strategy_checkpoint_fingerprint) {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "review_artifact_malformed",
            "Authoritative strategy checkpoint provenance must be a canonical fingerprint for current final-review validation.",
            "Restore authoritative strategy checkpoint provenance and retry gate-finish.",
        );
        return false;
    };
    true
}

fn is_canonical_sha256_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn require_current_browser_qa_pass_for_finish(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
    current_branch_closure_id: &str,
    current_branch_reviewed_state_id: &str,
    current_base_branch: &str,
    gate: &mut GateState,
) -> bool {
    let Some(current_final_review_record_id) = authoritative_state
        .current_final_review_record_id()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "qa_artifact_malformed",
            "The current QA milestone is missing its final-review dependency binding.",
            "Run requesting-code-review, then rerun qa-only for the current branch closure.",
        );
        return false;
    };
    let Some(current_qa_record_id) = authoritative_state
        .current_qa_record_id()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_artifact_missing",
            "Finish readiness requires a current QA milestone for the current branch closure.",
            "Run qa-only and return with a fresh QA result.",
        );
        return false;
    };
    let Some(record) = authoritative_state.browser_qa_record_by_id(&current_qa_record_id) else {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_artifact_missing",
            "Finish readiness requires a current QA milestone for the current branch closure.",
            "Run qa-only and return with a fresh QA result.",
        );
        return false;
    };
    if record.record_status != "current" {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "qa_artifact_malformed",
            "The current QA record is not marked current.",
            "Repair the authoritative QA record and retry gate-finish.",
        );
        return false;
    }
    if !summary_hash_matches(&record.summary, &record.summary_hash) {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "qa_artifact_malformed",
            "The current QA record has an invalid summary hash binding.",
            "Repair the authoritative QA record and retry gate-finish.",
        );
        return false;
    }
    if !require_authoritative_test_plan_binding_for_current_qa(context, &record, gate) {
        return false;
    }
    if record.branch_closure_id != current_branch_closure_id {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_artifact_missing",
            "The current QA milestone is not bound to the still-current branch closure.",
            "Run qa-only and return with a fresh QA result.",
        );
        return false;
    }
    if record.final_review_record_id.as_deref() != Some(current_final_review_record_id.as_str()) {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_artifact_review_binding_mismatch",
            "The current QA milestone is not bound to the current final-review milestone identity.",
            "Run qa-only and return with a fresh QA result.",
        );
        return false;
    }
    if record.source_plan_path != context.plan_rel
        || record.source_plan_revision != context.plan_document.plan_revision
    {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_artifact_plan_mismatch",
            "The current QA milestone does not match the current approved plan revision.",
            "Run qa-only using the latest test-plan handoff.",
        );
        return false;
    }
    if record.branch_name != context.runtime.branch_name {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_artifact_branch_mismatch",
            "The current QA milestone does not match the current branch.",
            "Run qa-only using the latest test-plan handoff.",
        );
        return false;
    }
    if record.base_branch.trim().is_empty() {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_artifact_base_branch_unresolved",
            "The current QA milestone is missing its base branch binding.",
            "Run qa-only using the latest test-plan handoff.",
        );
        return false;
    }
    if record.base_branch != current_base_branch {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_artifact_base_branch_mismatch",
            "The current QA milestone does not match the expected base branch.",
            "Run qa-only using the latest test-plan handoff.",
        );
        return false;
    }
    if record.repo_slug != context.runtime.repo_slug {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_artifact_repo_mismatch",
            "The current QA milestone does not match the current repo.",
            "Run qa-only using the latest test-plan handoff.",
        );
        return false;
    }
    if record.reviewed_state_id != current_branch_reviewed_state_id {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_artifact_head_mismatch",
            "The current QA milestone does not match the current reviewed branch state.",
            "Run qa-only using the latest test-plan handoff.",
        );
        return false;
    }
    if record.generated_by_identity != "featureforge/qa" {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_artifact_generator_mismatch",
            "The current QA milestone has an invalid generated-by identity.",
            "Repair the authoritative QA record and retry gate-finish.",
        );
        return false;
    }
    if record.result != "pass" {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_result_not_pass",
            "The current QA milestone is not pass.",
            "Address the QA findings and rerun qa-only.",
        );
        return false;
    }
    true
}

fn require_authoritative_test_plan_binding_for_current_qa(
    _context: &ExecutionContext,
    record: &CurrentBrowserQaRecord,
    gate: &mut GateState,
) -> bool {
    let Some(_test_plan_fingerprint) =
        record
            .source_test_plan_fingerprint
            .as_deref()
            .and_then(|value| {
                let value = value.trim();
                (!value.is_empty()).then_some(value)
            })
    else {
        gate.fail(
            FailureClass::QaArtifactNotFresh,
            "qa_source_test_plan_mismatch",
            "The current QA milestone is missing its authoritative test-plan binding.",
            "Refresh the current test plan and rerun QA before gate-finish.",
        );
        return false;
    };
    true
}

pub fn gate_finish_from_context(context: &ExecutionContext) -> GateResult {
    let mut gate = GateState::default();
    enforce_finish_dependency_index_truth(context, &mut gate);
    merge_gate_result(&mut gate, gate_review_base_result(context, false));
    if !gate.allowed {
        return gate.finish();
    }
    let pre_checkpoint_allowed =
        evaluate_pre_checkpoint_finish_gate(context, &mut gate) && gate.allowed;
    if gate
        .reason_codes
        .iter()
        .any(|code| code == "qa_requirement_missing_or_invalid")
    {
        return gate.finish();
    }
    let review_truth_result = gate_review_from_context(context);
    merge_gate_result_without_failure_class(&mut gate, &review_truth_result);
    if !review_truth_result.allowed {
        gate.allowed = false;
        if should_replace_gate_failure_class(
            &gate.failure_class,
            &review_truth_result.failure_class,
        ) {
            gate.failure_class = review_truth_result.failure_class.clone();
        }
    }
    if !pre_checkpoint_allowed || !gate.allowed {
        return gate.finish();
    }

    match finish_review_gate_checkpoint_matches_current_branch_closure(context) {
        Ok(true) => {}
        Ok(false) => {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                "finish_review_gate_checkpoint_missing",
                "Finish readiness requires a persisted gate-review pass checkpoint for the current branch closure.",
                format!(
                    "Run `featureforge workflow operator --plan {}` and complete the recommended public command sequence before finishing.",
                    context.plan_rel
                ),
            );
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "finish_review_gate_checkpoint_unavailable",
                format!(
                    "Finish readiness could not validate the persisted gate-review pass checkpoint: {}",
                    error.message
                ),
                "Restore authoritative finish-gate checkpoint state before running gate-finish.",
            );
        }
    }

    gate.finish()
}

fn finish_review_gate_checkpoint_matches_current_branch_closure(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    let Some(current_branch_closure_id) = current_branch_closure_id(context) else {
        return Ok(false);
    };
    let Some(authoritative_state) = load_authoritative_transition_state(context)? else {
        return Ok(false);
    };
    Ok(authoritative_state
        .finish_review_gate_pass_branch_closure_id()
        .as_deref()
        == Some(current_branch_closure_id.as_str()))
}

fn merge_gate_result(target: &mut GateState, incoming: GateResult) {
    merge_gate_result_impl(target, incoming, true);
}

fn merge_gate_result_without_failure_class(target: &mut GateState, incoming: &GateResult) {
    merge_gate_result_impl(target, incoming.clone(), false);
}

fn merge_gate_result_impl(target: &mut GateState, incoming: GateResult, merge_failure_class: bool) {
    let GateResult {
        allowed,
        action: _,
        failure_class,
        reason_codes,
        warning_codes,
        diagnostics,
        code: _,
        workspace_state_id: _,
        current_branch_reviewed_state_id: _,
        current_branch_closure_id: _,
        finish_review_gate_pass_branch_closure_id: _,
        recommended_command: _,
        rederive_via_workflow_operator: _,
    } = incoming;

    if !allowed {
        target.allowed = false;
    }
    if merge_failure_class
        && should_replace_gate_failure_class(&target.failure_class, &failure_class)
    {
        target.failure_class = failure_class;
    }

    for code in reason_codes {
        if !target.reason_codes.iter().any(|existing| existing == &code) {
            target.reason_codes.push(code);
        }
    }
    for code in warning_codes {
        if !target
            .warning_codes
            .iter()
            .any(|existing| existing == &code)
        {
            target.warning_codes.push(code);
        }
    }
    for diagnostic in diagnostics {
        if !target
            .diagnostics
            .iter()
            .any(|existing| existing.code == diagnostic.code)
        {
            target.diagnostics.push(diagnostic);
        }
    }
}

fn should_replace_gate_failure_class(current: &str, incoming: &str) -> bool {
    if incoming.is_empty() {
        return false;
    }
    current.is_empty()
        || (current == FailureClass::StaleProvenance.as_str()
            && incoming != FailureClass::StaleProvenance.as_str())
}

fn enforce_review_authoritative_late_gate_truth(context: &ExecutionContext, gate: &mut GateState) {
    let overlay = match load_status_authoritative_overlay_checked(context) {
        Ok(overlay) => overlay,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_state_unavailable",
                error.message,
                "Restore authoritative harness state readability and validity before running gate-review.",
            );
            return;
        }
    };
    let Some(overlay) = overlay else {
        return;
    };

    validate_review_dependency_index_truth(overlay.dependency_index_state.as_deref(), gate);
    let authoritative_state = match load_authoritative_transition_state(context) {
        Ok(Some(authoritative_state)) => authoritative_state,
        Ok(None) => return,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_state_unavailable",
                error.message,
                "Restore authoritative event-log state before running gate-review.",
            );
            return;
        }
    };
    let Some(current_identity) = usable_current_branch_closure_identity_from_authoritative_state(
        context,
        Some(&authoritative_state),
    ) else {
        return;
    };
    let late_stage_bindings = shared_current_late_stage_branch_bindings(
        Some(&authoritative_state),
        Some(&current_identity.branch_closure_id),
        Some(&current_identity.reviewed_state_id),
    );
    if late_stage_bindings
        .current_release_readiness_result
        .is_none()
    {
        fail_missing_review_downstream_truth("release_docs_state", "release docs", gate);
    }
    if late_stage_bindings.current_final_review_result.is_none() {
        fail_missing_review_downstream_truth("final_review_state", "final review", gate);
    }
    if context.plan_document.qa_requirement.as_deref() == Some("required")
        && late_stage_bindings.current_qa_result.is_none()
    {
        fail_missing_review_downstream_truth("browser_qa_state", "browser QA", gate);
    }
}

fn fail_missing_review_downstream_truth(field_name: &str, field_label: &str, gate: &mut GateState) {
    gate.fail(
        FailureClass::StaleProvenance,
        &format!("{field_name}_missing"),
        format!("Authoritative {field_label} truth is missing for review readiness."),
        "Refresh authoritative late-gate truth before running gate-review.",
    );
}

fn enforce_finish_dependency_index_truth(context: &ExecutionContext, gate: &mut GateState) {
    let overlay = match load_status_authoritative_overlay_checked(context) {
        Ok(overlay) => overlay,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_state_unavailable",
                error.message,
                "Restore authoritative harness state readability and validity before running gate-finish.",
            );
            return;
        }
    };
    let Some(overlay) = overlay else {
        return;
    };

    validate_finish_dependency_index_truth(overlay.dependency_index_state.as_deref(), gate);
}

fn validate_review_dependency_index_truth(raw_state: Option<&str>, gate: &mut GateState) {
    let state = normalize_optional_overlay_value(raw_state).unwrap_or("missing");
    if state == "fresh" {
        return;
    }

    let (code, message) = match state {
        "missing" => (
            "dependency_index_state_missing",
            "Authoritative dependency-index truth is missing for review readiness.",
        ),
        "stale" => (
            "dependency_index_state_stale",
            "Authoritative dependency-index truth is stale for review readiness.",
        ),
        _ => (
            "dependency_index_state_not_fresh",
            "Authoritative dependency-index truth is not fresh for review readiness.",
        ),
    };
    gate.fail(
        FailureClass::DependencyIndexMismatch,
        code,
        message,
        "Refresh authoritative dependency-index truth before running gate-review.",
    );
}

fn validate_finish_dependency_index_truth(raw_state: Option<&str>, gate: &mut GateState) {
    let state = normalize_optional_overlay_value(raw_state).unwrap_or("missing");
    if state == "fresh" {
        return;
    }

    let (code, message) = match state {
        "missing" => (
            "dependency_index_state_missing",
            "Authoritative dependency-index truth is missing for finish readiness.",
        ),
        "stale" => (
            "dependency_index_state_stale",
            "Authoritative dependency-index truth is stale for finish readiness.",
        ),
        _ => (
            "dependency_index_state_not_fresh",
            "Authoritative dependency-index truth is not fresh for finish readiness.",
        ),
    };
    gate.fail(
        FailureClass::DependencyIndexMismatch,
        code,
        message,
        "Refresh authoritative dependency-index truth before running gate-finish.",
    );
}

pub fn normalize_begin_request(args: &BeginArgs) -> BeginRequest {
    BeginRequest {
        task: args.task,
        step: args.step,
        execution_mode: args.execution_mode.map(|value| value.as_str().to_owned()),
        expect_execution_fingerprint: args.expect_execution_fingerprint.clone(),
    }
}

pub fn normalize_note_request(args: &NoteArgs) -> Result<NoteRequest, JsonFailure> {
    let message = require_normalized_text(
        &args.message,
        FailureClass::InvalidCommandInput,
        "Execution note summaries may not be blank after whitespace normalization.",
    )?;
    if message.chars().count() > 120 {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "Execution note summaries may not exceed 120 characters.",
        ));
    }
    let state = match args.state {
        NoteStateArg::Blocked => NoteState::Blocked,
        NoteStateArg::Interrupted => NoteState::Interrupted,
    };

    Ok(NoteRequest {
        task: args.task,
        step: args.step,
        state,
        message,
        expect_execution_fingerprint: args.expect_execution_fingerprint.clone(),
    })
}

pub fn normalize_complete_request(args: &CompleteArgs) -> Result<CompleteRequest, JsonFailure> {
    let claim = require_normalized_text(
        &args.claim,
        FailureClass::InvalidCommandInput,
        "Completion claims may not be blank after whitespace normalization.",
    )?;
    let verification_summary = match (
        args.verify_command.as_deref(),
        args.verify_result.as_deref(),
        args.manual_verify_summary.as_deref(),
    ) {
        (Some(_), Some(_), Some(_)) | (Some(_), None, _) | (None, Some(_), _) => {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "complete accepts exactly one verification mode.",
            ));
        }
        (Some(command), Some(result), None) => {
            let command = require_normalized_text(
                command,
                FailureClass::InvalidCommandInput,
                "Verification commands may not be blank after whitespace normalization.",
            )?;
            let result = require_normalized_text(
                result,
                FailureClass::InvalidCommandInput,
                "Verification results may not be blank after whitespace normalization.",
            )?;
            format!("`{command}` -> {result}")
        }
        (None, None, Some(summary)) => {
            let summary = require_normalized_text(
                summary,
                FailureClass::InvalidCommandInput,
                "Manual verification summaries may not be blank after whitespace normalization.",
            )?;
            format!("Manual inspection only: {summary}")
        }
        (None, None, None) => {
            return Err(JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "complete requires exactly one verification mode.",
            ));
        }
    };

    Ok(CompleteRequest {
        task: args.task,
        step: args.step,
        source: args.source.as_str().to_owned(),
        claim,
        files: args.files.clone(),
        verify_command: args
            .verify_command
            .as_deref()
            .map(normalize_whitespace)
            .filter(|value| !value.is_empty()),
        verification_summary,
        expect_execution_fingerprint: args.expect_execution_fingerprint.clone(),
    })
}

pub fn normalize_reopen_request(args: &ReopenArgs) -> Result<ReopenRequest, JsonFailure> {
    Ok(ReopenRequest {
        task: args.task,
        step: args.step,
        source: args.source.as_str().to_owned(),
        reason: require_normalized_text(
            &args.reason,
            FailureClass::InvalidCommandInput,
            "Reopen reasons may not be blank after whitespace normalization.",
        )?,
        expect_execution_fingerprint: args.expect_execution_fingerprint.clone(),
    })
}

pub fn normalize_transfer_request(args: &TransferArgs) -> Result<TransferRequest, JsonFailure> {
    let reason = require_normalized_text(
        &args.reason,
        FailureClass::InvalidCommandInput,
        "Transfer reasons may not be blank after whitespace normalization.",
    )?;
    let routed_shape_present = args.scope.is_some() || args.to.is_some();
    let legacy_shape_present = args.repair_task.is_some()
        || args.repair_step.is_some()
        || args.source.is_some()
        || args.expect_execution_fingerprint.is_some();

    if routed_shape_present && legacy_shape_present {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "transfer accepts either the routed handoff shape (--scope/--to/--reason) or the legacy repair-step shape (--repair-task/--repair-step/--source/--expect-execution-fingerprint), but not both at once.",
        ));
    }

    if routed_shape_present {
        let scope = args.scope.ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "transfer routed handoff mode requires --scope.",
            )
        })?;
        let to = require_normalized_text(
            args.to.as_deref().unwrap_or_default(),
            FailureClass::InvalidCommandInput,
            "transfer routed handoff mode requires --to.",
        )?;
        return Ok(TransferRequest {
            reason,
            mode: TransferRequestMode::WorkflowHandoff {
                scope: scope.as_str().to_owned(),
                to,
            },
        });
    }

    let repair_task = args.repair_task.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "transfer legacy repair-step mode requires --repair-task.",
        )
    })?;
    let repair_step = args.repair_step.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "transfer legacy repair-step mode requires --repair-step.",
        )
    })?;
    let source = args.source.ok_or_else(|| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "transfer legacy repair-step mode requires --source.",
        )
    })?;
    let expect_execution_fingerprint =
        args.expect_execution_fingerprint.clone().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "transfer legacy repair-step mode requires --expect-execution-fingerprint.",
            )
        })?;

    Ok(TransferRequest {
        reason,
        mode: TransferRequestMode::RepairStep {
            repair_task,
            repair_step,
            source: source.as_str().to_owned(),
            expect_execution_fingerprint,
        },
    })
}

pub fn normalize_rebuild_evidence_request(
    args: &RebuildEvidenceArgs,
) -> Result<RebuildEvidenceRequest, JsonFailure> {
    let mut parsed_steps = Vec::with_capacity(args.steps.len());
    for raw in &args.steps {
        let (task, step) = raw.split_once(':').ok_or_else(|| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "--step must use task:step selectors such as 1:2.",
            )
        })?;
        let task = task.parse::<u32>().map_err(|_| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "--step must use numeric task:step selectors such as 1:2.",
            )
        })?;
        let step = step.parse::<u32>().map_err(|_| {
            JsonFailure::new(
                FailureClass::InvalidCommandInput,
                "--step must use numeric task:step selectors such as 1:2.",
            )
        })?;
        parsed_steps.push((task, step));
    }

    Ok(RebuildEvidenceRequest {
        plan: args.plan.clone(),
        all: args.all || (args.tasks.is_empty() && args.steps.is_empty()),
        tasks: args.tasks.clone(),
        steps: parsed_steps,
        raw_steps: args.steps.clone(),
        include_open: args.include_open,
        skip_manual_fallback: args.skip_manual_fallback,
        continue_on_error: args.continue_on_error,
        dry_run: args.dry_run,
        max_jobs: args.max_jobs,
        no_output: args.no_output,
        json: args.json,
    })
}

pub fn parse_command_verification_summary(summary: &str) -> Option<String> {
    let trimmed = normalize_whitespace(summary);
    let suffix = trimmed.strip_prefix('`')?;
    let (command, _) = suffix.split_once("` -> ")?;
    let command = normalize_whitespace(command);
    (!command.is_empty()).then_some(command)
}

struct RebuildCandidateScan {
    session_provenance_reason: Option<String>,
    source_spec_fingerprint: String,
    latest_attempts: BTreeMap<(u32, u32), usize>,
    latest_completed: BTreeMap<(u32, u32), usize>,
    latest_file_proofs: BTreeMap<String, usize>,
}

fn prepare_rebuild_candidate_scan(context: &ExecutionContext) -> RebuildCandidateScan {
    let contract_plan_fingerprint = hash_contract_plan(&context.plan_source);
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let session_provenance_reason = if context.evidence.plan_fingerprint.as_deref()
        != Some(contract_plan_fingerprint.as_str())
    {
        Some(String::from("plan_fingerprint_mismatch"))
    } else if context.evidence.source_spec_fingerprint.as_deref()
        != Some(source_spec_fingerprint.as_str())
    {
        Some(String::from("source_spec_fingerprint_mismatch"))
    } else {
        None
    };
    let latest_attempts = latest_attempt_indices_by_step(&context.evidence);
    let latest_completed = latest_completed_attempts_by_step(&context.evidence);
    let latest_file_proofs =
        latest_completed_attempts_by_file(&context.evidence, &latest_completed);

    RebuildCandidateScan {
        session_provenance_reason,
        source_spec_fingerprint,
        latest_attempts,
        latest_completed,
        latest_file_proofs,
    }
}

fn rebuild_candidate_for_step(
    context: &ExecutionContext,
    scan: &RebuildCandidateScan,
    step: &PlanStepState,
    include_open: bool,
) -> Option<RebuildEvidenceCandidate> {
    let step_key = (step.task_number, step.step_number);
    let latest_attempt = scan
        .latest_attempts
        .get(&step_key)
        .map(|index| &context.evidence.attempts[*index]);
    let latest_completed_index = scan.latest_completed.get(&step_key).copied();
    let latest_completed_attempt =
        latest_completed_index.map(|index| &context.evidence.attempts[index]);

    let mut pre_invalidation_reason = None;
    let mut target_kind = String::new();
    let mut needs_reopen = false;

    if step.checked
        && let Some(reason) = scan.session_provenance_reason.as_ref()
        && latest_completed_attempt.is_some()
    {
        pre_invalidation_reason = Some(reason.clone());
        target_kind = String::from("stale_completed_attempt");
        needs_reopen = true;
    }

    if let Some(attempt) = latest_attempt
        && attempt.status == "Invalidated"
        && attempt.invalidation_reason != "N/A"
    {
        pre_invalidation_reason = Some(attempt.invalidation_reason.clone());
        target_kind = String::from("invalidated_attempt");
        needs_reopen = step.checked;
    }

    if pre_invalidation_reason.is_none()
        && step.checked
        && let Some(attempt) = latest_completed_attempt
    {
        let expected_packet = task_packet_fingerprint(
            context,
            &scan.source_spec_fingerprint,
            step.task_number,
            step.step_number,
        )?;
        if attempt.packet_fingerprint.as_deref() != Some(expected_packet.as_str()) {
            pre_invalidation_reason = Some(String::from("packet_fingerprint_mismatch"));
            target_kind = String::from("stale_completed_attempt");
            needs_reopen = true;
        } else {
            for proof in &attempt.file_proofs {
                if proof.path == NO_REPO_FILES_MARKER
                    || proof.path == context.plan_rel
                    || proof.path == context.evidence_rel
                {
                    continue;
                }
                if scan
                    .latest_file_proofs
                    .get(&proof.path)
                    .is_some_and(|latest_index| {
                        latest_completed_index
                            .is_some_and(|attempt_index| *latest_index != attempt_index)
                    })
                {
                    continue;
                }
                match current_file_proof_checked(&context.runtime.repo_root, &proof.path) {
                    Ok(current_proof) => {
                        if current_proof != proof.proof {
                            pre_invalidation_reason = Some(String::from("files_proven_drifted"));
                            target_kind = String::from("stale_completed_attempt");
                            needs_reopen = true;
                            break;
                        }
                    }
                    Err(error) => {
                        pre_invalidation_reason = Some(format!(
                            "artifact_read_error: could not read {} ({error})",
                            proof.path
                        ));
                        target_kind = String::from("artifact_read_error");
                        needs_reopen = false;
                        break;
                    }
                }
            }
        }
    }

    if pre_invalidation_reason.is_none()
        && include_open
        && !step.checked
        && (step.note_state.is_some() || latest_attempt.is_some())
    {
        pre_invalidation_reason = Some(String::from("open_step_requested"));
        target_kind = String::from("open_step");
    }

    let pre_invalidation_reason = pre_invalidation_reason?;
    let attempt = latest_attempt.or(latest_completed_attempt);
    let verify_command = attempt.and_then(|candidate| candidate.verify_command.clone());
    let verify_mode = if verify_command.is_some() {
        String::from("command")
    } else {
        String::from("manual")
    };
    let claim = attempt
        .map(|candidate| candidate.claim.clone())
        .unwrap_or_else(|| {
            format!(
                "Rebuilt evidence for Task {} Step {}.",
                step.task_number, step.step_number
            )
        });
    let files = attempt
        .map(|candidate| candidate.files.clone())
        .unwrap_or_default();
    let attempt_number = attempt.map(|candidate| candidate.attempt_number);
    let artifact_epoch = attempt.map(|candidate| candidate.recorded_at.clone());

    Some(RebuildEvidenceCandidate {
        task: step.task_number,
        step: step.step_number,
        order_key: (step.task_number, step.step_number),
        target_kind,
        pre_invalidation_reason,
        verify_command,
        verify_mode,
        claim,
        files,
        attempt_number,
        artifact_epoch,
        needs_reopen,
    })
}

pub fn discover_rebuild_candidates(
    context: &ExecutionContext,
    request: &RebuildEvidenceRequest,
) -> Result<Vec<RebuildEvidenceCandidate>, JsonFailure> {
    let task_filter = request.tasks.iter().copied().collect::<BTreeSet<_>>();
    let step_filter = request.steps.iter().copied().collect::<BTreeSet<_>>();

    let matching_steps = context
        .steps
        .iter()
        .filter(|step| {
            (task_filter.is_empty() || task_filter.contains(&step.task_number))
                && (step_filter.is_empty()
                    || step_filter.contains(&(step.task_number, step.step_number)))
        })
        .collect::<Vec<_>>();
    if (!request.tasks.is_empty() || !request.steps.is_empty()) && matching_steps.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "scope_no_matches: no approved plan steps matched the requested filters.",
        ));
    }

    let scan = prepare_rebuild_candidate_scan(context);
    let mut candidates = Vec::new();

    for step in matching_steps {
        if let Some(candidate) =
            rebuild_candidate_for_step(context, &scan, step, request.include_open)
        {
            candidates.push(candidate);
        }
    }

    candidates.sort_by_key(|candidate| candidate.order_key);

    Ok(candidates)
}

pub fn normalize_source(source: &str, execution_mode: &str) -> Result<(), JsonFailure> {
    match source {
        "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {}
        _ => {
            return Err(JsonFailure::new(
                FailureClass::InvalidExecutionMode,
                "Execution source must be one of the supported execution modes.",
            ));
        }
    }
    if source != execution_mode {
        return Err(JsonFailure::new(
            FailureClass::InvalidExecutionMode,
            "Execution source must exactly match the persisted execution mode for this plan revision.",
        ));
    }
    Ok(())
}

pub fn validate_v2_evidence_provenance(context: &ExecutionContext, gate: &mut GateState) {
    let contract_plan_fingerprint = hash_contract_plan(&context.plan_source);
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let latest_attempts = latest_completed_attempts_by_step(&context.evidence);
    let latest_file_proofs = latest_completed_attempts_by_file(&context.evidence, &latest_attempts);

    if context.evidence.plan_fingerprint.as_deref() != Some(contract_plan_fingerprint.as_str()) {
        gate.fail(
            FailureClass::StaleExecutionEvidence,
            "plan_fingerprint_mismatch",
            "Execution evidence plan fingerprint no longer matches the approved plan source.",
            "Rebuild the execution evidence for the current approved plan revision.",
        );
    }
    if context.evidence.source_spec_fingerprint.as_deref() != Some(source_spec_fingerprint.as_str())
    {
        gate.fail(
            FailureClass::StaleExecutionEvidence,
            "source_spec_fingerprint_mismatch",
            "Execution evidence source spec fingerprint no longer matches the approved source spec.",
            "Rebuild the execution evidence for the current approved spec revision.",
        );
    }

    for step in context.steps.iter().filter(|step| step.checked) {
        let Some(attempt_index) = latest_attempts
            .get(&(step.task_number, step.step_number))
            .copied()
        else {
            continue;
        };
        let attempt = &context.evidence.attempts[attempt_index];
        let expected_packet = task_packet_fingerprint(
            context,
            &source_spec_fingerprint,
            step.task_number,
            step.step_number,
        );
        if attempt.packet_fingerprint.as_deref() != expected_packet.as_deref() {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "packet_fingerprint_mismatch",
                format!(
                    "Task {} Step {} evidence packet provenance no longer matches the current approved plan/spec pair.",
                    step.task_number, step.step_number
                ),
                "Rebuild the packet and reopen the affected step.",
            );
        }
        for proof in &attempt.file_proofs {
            if proof.path == NO_REPO_FILES_MARKER
                || proof.path == context.plan_rel
                || proof.path == context.evidence_rel
            {
                continue;
            }
            if latest_file_proofs
                .get(&proof.path)
                .is_some_and(|latest_index| *latest_index != attempt_index)
            {
                continue;
            }
            let current = current_file_proof(&context.runtime.repo_root, &proof.path);
            if current != proof.proof {
                gate.fail(
                    FailureClass::MissedReopenRequired,
                    "files_proven_drifted",
                    format!(
                        "Task {} Step {} proved file '{}' no longer matches its recorded fingerprint.",
                        step.task_number, step.step_number, proof.path
                    ),
                    "Reopen the step and rebuild its evidence.",
                );
            }
        }
    }
}

pub fn derive_evidence_rel_path(plan_rel: &str, revision: u32) -> String {
    let base = Path::new(plan_rel)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("plan");
    format!("{ACTIVE_EVIDENCE_ROOT}/{base}-r{revision}-evidence.md")
}

pub fn hash_contract_plan(source: &str) -> String {
    let sanitized_steps = parse_contract_render(source);
    let semantic_plan = normalized_plan_source_for_semantic_identity(&sanitized_steps);
    sha256_hex(semantic_plan.as_bytes())
}

pub fn render_contract_plan(source: &str) -> String {
    parse_contract_render(source)
}

pub struct PacketFingerprintInput<'a> {
    pub plan_path: &'a str,
    pub plan_revision: u32,
    pub task_definition_identity: &'a str,
    pub source_spec_path: &'a str,
    pub source_spec_revision: u32,
    pub source_spec_fingerprint: &'a str,
    pub task: u32,
    pub step: u32,
}

pub fn compute_packet_fingerprint(input: PacketFingerprintInput<'_>) -> String {
    let payload = format!(
        "plan_path={plan_path}\nplan_revision={plan_revision}\ntask_definition_identity={task_definition_identity}\nsource_spec_path={source_spec_path}\nsource_spec_revision={source_spec_revision}\nsource_spec_fingerprint={source_spec_fingerprint}\ntask_number={task}\nstep_number={step}\n",
        plan_path = input.plan_path,
        plan_revision = input.plan_revision,
        task_definition_identity = input.task_definition_identity,
        source_spec_path = input.source_spec_path,
        source_spec_revision = input.source_spec_revision,
        source_spec_fingerprint = input.source_spec_fingerprint,
        task = input.task,
        step = input.step,
    );
    sha256_hex(payload.as_bytes())
}

pub fn task_packet_fingerprint(
    context: &ExecutionContext,
    source_spec_fingerprint: &str,
    task: u32,
    step: u32,
) -> Option<String> {
    let task_definition_identity = task_definition_identity_for_task(context, task).ok()??;
    Some(compute_packet_fingerprint(PacketFingerprintInput {
        plan_path: &context.plan_rel,
        plan_revision: context.plan_document.plan_revision,
        task_definition_identity: &task_definition_identity,
        source_spec_path: &context.plan_document.source_spec_path,
        source_spec_revision: context.plan_document.source_spec_revision,
        source_spec_fingerprint,
        task,
        step,
    }))
}

pub fn current_head_sha(repo_root: &Path) -> Result<String, JsonFailure> {
    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not discover the current repository: {error}"),
        )
    })?;
    let head = repo.head_id().map_err(|error| {
        JsonFailure::new(
            FailureClass::BranchDetectionFailed,
            format!("Could not determine the current HEAD commit: {error}"),
        )
    })?;
    Ok(head.detach().to_string())
}

pub fn current_tracked_tree_sha(repo_root: &Path) -> Result<String, JsonFailure> {
    current_repo_tracked_tree_sha(repo_root)
}

fn repo_has_non_runtime_projection_tracked_changes(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    let repo = discover_repository(&context.runtime.repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not discover the current repository: {error}"),
        )
    })?;
    let mut status_iter = repo
        .status(gix::progress::Discard)
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!("Could not prepare tracked worktree status: {error}"),
            )
        })?
        .untracked_files(gix::status::UntrackedFiles::None)
        .index_worktree_rewrites(None)
        .tree_index_track_renames(gix::status::tree_index::TrackRenames::Disabled)
        .into_iter(Vec::<gix::bstr::BString>::new())
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!("Could not determine tracked worktree status: {error}"),
            )
        })?;
    for item in &mut status_iter {
        let item = item.map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!("Could not inspect tracked worktree change: {error}"),
            )
        })?;
        let path = item.location().to_string();
        if is_runtime_owned_execution_control_plane_path(context, &path) {
            continue;
        }
        if path == context.plan_rel {
            if approved_plan_change_is_clean_for_preflight(context, &path)? {
                continue;
            }
            return Ok(Some(String::from("approved_plan_semantic_drift")));
        }
        return Ok(Some(String::from("tracked_worktree_dirty")));
    }
    Ok(None)
}

fn approved_plan_change_is_projection_only(
    context: &ExecutionContext,
    path: &str,
) -> Result<bool, JsonFailure> {
    approved_plan_sources_match_after_normalization(
        context,
        path,
        normalized_plan_source_for_semantic_identity,
    )
}

fn approved_plan_change_is_clean_for_preflight(
    context: &ExecutionContext,
    path: &str,
) -> Result<bool, JsonFailure> {
    approved_plan_sources_match_after_normalization(
        context,
        path,
        normalized_plan_source_for_approved_plan_preflight,
    )
}

fn approved_plan_sources_match_after_normalization(
    context: &ExecutionContext,
    path: &str,
    normalize: fn(&str) -> String,
) -> Result<bool, JsonFailure> {
    let Some(head_source) = head_blob_source_for_path(&context.runtime.repo_root, path)? else {
        return Ok(false);
    };
    let Some(index_source) = index_blob_source_for_path(&context.runtime.repo_root, path)? else {
        return Ok(false);
    };
    let worktree_source =
        fs::read_to_string(context.runtime.repo_root.join(path)).map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!(
                    "Could not read approved plan {path} while checking semantic drift: {error}"
                ),
            )
        })?;
    let normalized_head = normalize(&head_source);
    Ok(normalized_head == normalize(&index_source)
        && normalized_head == normalize(&worktree_source))
}

fn head_blob_source_for_path(repo_root: &Path, path: &str) -> Result<Option<String>, JsonFailure> {
    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not discover repository while reading HEAD content for {path}: {error}"),
        )
    })?;
    let tree = repo.head_tree().map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not read HEAD tree while checking semantic drift for {path}: {error}"),
        )
    })?;
    let Some(entry) = tree.lookup_entry_by_path(path).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not inspect HEAD tree path {path}: {error}"),
        )
    })?
    else {
        return Ok(None);
    };
    let blob = repo.find_blob(entry.object_id()).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not load HEAD blob for {path}: {error}"),
        )
    })?;
    std::str::from_utf8(&blob.data)
        .map(|content| Some(content.to_owned()))
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!("HEAD content for {path} was not valid UTF-8: {error}"),
            )
        })
}

fn index_blob_source_for_path(repo_root: &Path, path: &str) -> Result<Option<String>, JsonFailure> {
    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!(
                "Could not discover repository while reading index content for {path}: {error}"
            ),
        )
    })?;
    let index = repo.open_index().map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!(
                "Could not open repository index while checking semantic drift for {path}: {error}"
            ),
        )
    })?;
    let Some(entry) = index.entry_by_path(path.as_bytes().as_bstr()) else {
        return Ok(None);
    };
    if entry.stage_raw() != 0 {
        return Ok(None);
    }
    let blob = repo.find_blob(entry.id).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not load index blob for {path}: {error}"),
        )
    })?;
    std::str::from_utf8(&blob.data)
        .map(|content| Some(content.to_owned()))
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!("Index content for {path} was not valid UTF-8: {error}"),
            )
        })
}

fn compute_repo_has_tracked_worktree_changes_excluding_execution_evidence(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    let repo = discover_repository(&context.runtime.repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not discover the current repository: {error}"),
        )
    })?;
    let mut status_iter = repo
        .status(gix::progress::Discard)
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!(
                    "Could not prepare tracked worktree status while filtering execution evidence: {error}"
                ),
            )
        })?
        .untracked_files(gix::status::UntrackedFiles::None)
        .index_worktree_rewrites(None)
        .tree_index_track_renames(gix::status::tree_index::TrackRenames::Disabled)
        .into_iter(Vec::<gix::bstr::BString>::new())
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!(
                    "Could not determine whether tracked worktree changes remain outside execution evidence: {error}"
                ),
            )
        })?;
    for item in &mut status_iter {
        let item = item.map_err(|error| {
            JsonFailure::new(
                FailureClass::WorkspaceNotSafe,
                format!(
                    "Could not determine whether tracked worktree changes remain outside execution evidence: {error}"
                ),
            )
        })?;
        let path = item.location().to_string();
        if is_runtime_owned_execution_control_plane_path(context, &path) {
            continue;
        }
        if path == context.plan_rel && approved_plan_change_is_projection_only(context, &path)? {
            continue;
        }
        match item {
            gix::status::Item::TreeIndex(_) => return Ok(true),
            gix::status::Item::IndexWorktree(change) if change.summary().is_some() => {
                return Ok(true);
            }
            gix::status::Item::IndexWorktree(_) => {}
        }
    }
    Ok(false)
}

pub fn state_dir() -> PathBuf {
    featureforge_state_dir()
}

pub fn current_file_proof(repo_root: &Path, path: &str) -> String {
    if path == NO_REPO_FILES_MARKER {
        return String::from("sha256:none");
    }
    let abs = repo_root.join(path);
    match fs::read(&abs) {
        Ok(contents) => format!("sha256:{}", sha256_hex(&contents)),
        Err(_) => String::from("sha256:missing"),
    }
}

pub fn current_file_proof_checked(repo_root: &Path, path: &str) -> Result<String, String> {
    if path == NO_REPO_FILES_MARKER {
        return Ok(String::from("sha256:none"));
    }
    let abs = repo_root.join(path);
    match fs::read(&abs) {
        Ok(contents) => Ok(format!("sha256:{}", sha256_hex(&contents))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::from("sha256:missing")),
        Err(error) => Err(error.to_string()),
    }
}

fn normalize_persisted_file_path(path: &str) -> Result<String, JsonFailure> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Execution evidence must include at least one repo-relative file entry.",
        ));
    }
    normalize_repo_relative_path(trimmed).map_err(JsonFailure::from)
}

pub fn require_normalized_text(
    value: &str,
    failure_class: FailureClass,
    message: &str,
) -> Result<String, JsonFailure> {
    let normalized = normalize_whitespace(value);
    if normalized.is_empty() {
        return Err(JsonFailure::new(failure_class, message));
    }
    Ok(normalized)
}

fn repo_head_detached(context: &ExecutionContext) -> Result<bool, HeadError> {
    let repo = discover_repository(&context.runtime.repo_root).map_err(|error| HeadError {
        message: format!("Could not discover the current repository: {error}"),
    })?;
    let head = repo.head().map_err(|error| HeadError {
        message: format!("Could not determine the current branch: {error}"),
    })?;
    Ok(head.is_detached())
}

#[derive(Debug)]
struct HeadError {
    message: String,
}

#[derive(Debug)]
pub struct GateState {
    pub allowed: bool,
    pub failure_class: String,
    pub reason_codes: Vec<String>,
    pub warning_codes: Vec<String>,
    pub diagnostics: Vec<GateDiagnostic>,
    pub action: String,
    pub code: Option<String>,
    pub workspace_state_id: Option<String>,
    pub current_branch_reviewed_state_id: Option<String>,
    pub current_branch_closure_id: Option<String>,
    pub finish_review_gate_pass_branch_closure_id: Option<String>,
    pub recommended_command: Option<String>,
    pub rederive_via_workflow_operator: Option<bool>,
}

impl Default for GateState {
    fn default() -> Self {
        Self {
            allowed: true,
            failure_class: String::new(),
            reason_codes: Vec::new(),
            warning_codes: Vec::new(),
            diagnostics: Vec::new(),
            action: String::from("passed"),
            code: None,
            workspace_state_id: None,
            current_branch_reviewed_state_id: None,
            current_branch_closure_id: None,
            finish_review_gate_pass_branch_closure_id: None,
            recommended_command: None,
            rederive_via_workflow_operator: None,
        }
    }
}

impl GateState {
    pub fn from_result(result: GateResult) -> Self {
        Self {
            allowed: result.allowed,
            action: result.action,
            failure_class: result.failure_class,
            reason_codes: result.reason_codes,
            warning_codes: result.warning_codes,
            diagnostics: result.diagnostics,
            code: result.code,
            workspace_state_id: result.workspace_state_id,
            current_branch_reviewed_state_id: result.current_branch_reviewed_state_id,
            current_branch_closure_id: result.current_branch_closure_id,
            finish_review_gate_pass_branch_closure_id: result
                .finish_review_gate_pass_branch_closure_id,
            recommended_command: result.recommended_command,
            rederive_via_workflow_operator: result.rederive_via_workflow_operator,
        }
    }

    pub fn fail(
        &mut self,
        failure_class: FailureClass,
        code: &str,
        message: impl Into<String>,
        remediation: impl Into<String>,
    ) {
        self.allowed = false;
        if self.failure_class.is_empty() {
            self.failure_class = failure_class.as_str().to_owned();
        }
        if !self.reason_codes.iter().any(|existing| existing == code) {
            self.reason_codes.push(code.to_owned());
            self.diagnostics.push(GateDiagnostic {
                code: code.to_owned(),
                severity: String::from("error"),
                message: message.into(),
                remediation: remediation.into(),
            });
        }
    }

    pub fn warn(&mut self, code: &str) {
        if !self.warning_codes.iter().any(|existing| existing == code) {
            self.warning_codes.push(code.to_owned());
        }
    }

    pub fn finish(mut self) -> GateResult {
        if self.failure_class.is_empty() {
            self.allowed = true;
        }
        GateResult {
            allowed: self.allowed,
            action: if self.allowed {
                String::from("passed")
            } else {
                String::from("blocked")
            },
            failure_class: self.failure_class,
            reason_codes: self.reason_codes,
            warning_codes: self.warning_codes,
            diagnostics: self.diagnostics,
            code: self.code,
            workspace_state_id: self.workspace_state_id,
            current_branch_reviewed_state_id: self.current_branch_reviewed_state_id,
            current_branch_closure_id: self.current_branch_closure_id,
            finish_review_gate_pass_branch_closure_id: self
                .finish_review_gate_pass_branch_closure_id,
            recommended_command: self.recommended_command,
            rederive_via_workflow_operator: self.rederive_via_workflow_operator,
        }
    }
}

fn normalize_plan_path(plan_path: &Path) -> Result<String, JsonFailure> {
    let raw = plan_path.to_string_lossy();
    let normalized = RepoPath::parse(&raw).map_err(|_| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "Plan path must be a normalized repo-relative path.",
        )
    })?;
    let required_prefix = format!("{ACTIVE_PLAN_ROOT}/");
    if !normalized.as_str().starts_with(&required_prefix) {
        return Err(JsonFailure::new(
            FailureClass::InvalidCommandInput,
            "Plan path must live under docs/featureforge/plans/.",
        ));
    }
    Ok(normalized.as_str().to_owned())
}

fn validate_source_spec(
    source: &str,
    expected_path: &str,
    expected_revision: u32,
    runtime: &ExecutionRuntime,
    matching_manifest: Option<&WorkflowManifest>,
    selection_policy: ApprovedArtifactSelectionPolicy,
) -> Result<(), JsonFailure> {
    let headers = parse_headers(source);
    if headers.get("Workflow State") != Some(&String::from("CEO Approved")) {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan source spec is not CEO Approved.",
        ));
    }
    if headers
        .get("Spec Revision")
        .and_then(|value| value.parse::<u32>().ok())
        != Some(expected_revision)
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan source spec path or revision is stale.",
        ));
    }
    match headers.get("Last Reviewed By").map(String::as_str) {
        Some("plan-ceo-review") => {}
        _ => {
            return Err(JsonFailure::new(
                FailureClass::PlanNotExecutionReady,
                "Approved plan source spec Last Reviewed By header is missing or malformed.",
            ));
        }
    }
    let approved_spec_candidates = approved_spec_candidate_paths(&runtime.repo_root);
    let manifest_selected_spec =
        matching_manifest.is_some_and(|manifest| manifest.expected_spec_path == expected_path);
    if approved_spec_candidates.len() > 1
        && !manifest_selected_spec
        && !matches!(
            selection_policy,
            ApprovedArtifactSelectionPolicy::AllowExactPlan
        )
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved spec candidates are ambiguous.",
        ));
    }
    if !approved_spec_candidates
        .iter()
        .any(|candidate| candidate == expected_path)
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan source spec path or revision is stale.",
        ));
    }
    Ok(())
}

fn validate_unique_approved_plan(
    expected_plan_path: &str,
    source_spec_path: &str,
    source_spec_revision: u32,
    runtime: &ExecutionRuntime,
    matching_manifest: Option<&WorkflowManifest>,
    selection_policy: ApprovedArtifactSelectionPolicy,
) -> Result<(), JsonFailure> {
    let approved_plan_candidates =
        approved_plan_candidate_paths(&runtime.repo_root, source_spec_path, source_spec_revision);
    let manifest_selected_plan =
        matching_manifest.is_some_and(|manifest| manifest.expected_plan_path == expected_plan_path);
    if approved_plan_candidates.len() > 1
        && !manifest_selected_plan
        && !matches!(
            selection_policy,
            ApprovedArtifactSelectionPolicy::AllowExactPlan
        )
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan candidates are ambiguous.",
        ));
    }
    if !approved_plan_candidates
        .iter()
        .any(|candidate| candidate == expected_plan_path)
    {
        return Err(JsonFailure::new(
            FailureClass::PlanNotExecutionReady,
            "Approved plan is not the unique current approved plan for its source spec.",
        ));
    }
    Ok(())
}

fn matching_workflow_manifest(runtime: &ExecutionRuntime) -> Option<WorkflowManifest> {
    let user_name = env::var("USER").unwrap_or_else(|_| String::from("user"));
    let manifest_path = runtime
        .state_dir
        .join("projects")
        .join(&runtime.repo_slug)
        .join(format!(
            "{user_name}-{}-workflow-state.json",
            runtime.safe_branch
        ));
    let ManifestLoadResult::Loaded(manifest) = load_manifest_read_only(&manifest_path) else {
        return None;
    };
    if stored_repo_root_matches_current(&manifest.repo_root, &runtime.repo_root)
        && manifest.branch == runtime.branch_name
    {
        Some(manifest)
    } else {
        None
    }
}

fn repo_safety_stage(context: &ExecutionContext) -> String {
    match context.plan_document.execution_mode.as_str() {
        "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {
            context.plan_document.execution_mode.clone()
        }
        _ => String::from("featureforge:execution-preflight"),
    }
}

fn repo_safety_preflight_message(result: &crate::repo_safety::RepoSafetyResult) -> String {
    match result.failure_class.as_str() {
        "ProtectedBranchDetected" => format!(
            "Execution preflight cannot continue on protected branch {} without explicit approval.",
            result.branch
        ),
        "ApprovalScopeMismatch" => String::from(
            "Execution preflight repo-safety approval does not match the current scope.",
        ),
        "ApprovalFingerprintMismatch" => String::from(
            "Execution preflight repo-safety approval does not match the current branch or write scope.",
        ),
        _ => String::from("Execution preflight is blocked by repo-safety policy."),
    }
}

fn repo_safety_preflight_remediation(result: &crate::repo_safety::RepoSafetyResult) -> String {
    if !result.suggested_next_skill.is_empty() {
        format!(
            "Use {} or explicitly approve the protected-branch execution scope before continuing.",
            result.suggested_next_skill
        )
    } else {
        String::from("Resolve the repo-safety blocker before continuing execution.")
    }
}

fn repo_has_unresolved_index_entries(repo_root: &Path) -> Result<bool, JsonFailure> {
    let repo = discover_repository(repo_root).map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not discover the current repository: {error}"),
        )
    })?;
    let index = repo.open_index().map_err(|error| {
        JsonFailure::new(
            FailureClass::WorkspaceNotSafe,
            format!("Could not open the repository index: {error}"),
        )
    })?;
    Ok(index
        .entries()
        .iter()
        .any(|entry| entry.stage() != gix::index::entry::Stage::Unconflicted))
}

fn parse_headers(source: &str) -> BTreeMap<String, String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("**")?;
            let (key, value) = rest.split_once(":** ")?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

fn parse_headers_file(path: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(path)
        .ok()
        .map(|source| parse_headers(&source))
        .unwrap_or_default()
}

fn approved_spec_candidate_paths(repo_root: &Path) -> Vec<String> {
    let mut candidates = markdown_files_under(&repo_root.join(ACTIVE_SPEC_ROOT))
        .into_iter()
        .filter_map(|path| {
            let headers = parse_headers_file(&path);
            if headers.get("Workflow State").map(String::as_str) != Some("CEO Approved") {
                return None;
            }
            let revision_valid = headers
                .get("Spec Revision")
                .and_then(|value| value.parse::<u32>().ok())
                .is_some();
            let reviewed_by_valid =
                headers.get("Last Reviewed By").map(String::as_str) == Some("plan-ceo-review");
            if !revision_valid || !reviewed_by_valid {
                return None;
            }
            path.strip_prefix(repo_root)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
}

fn approved_plan_candidate_paths(
    repo_root: &Path,
    source_spec_path: &str,
    source_spec_revision: u32,
) -> Vec<String> {
    let mut candidates = markdown_files_under(&repo_root.join(ACTIVE_PLAN_ROOT))
        .into_iter()
        .filter_map(|path| {
            let headers = parse_headers_file(&path);
            if headers.get("Workflow State").map(String::as_str) != Some("Engineering Approved") {
                return None;
            }
            let execution_mode_valid = matches!(
                headers.get("Execution Mode").map(String::as_str),
                Some("none")
                    | Some("featureforge:executing-plans")
                    | Some("featureforge:subagent-driven-development")
            );
            let reviewed_by_valid =
                headers.get("Last Reviewed By").map(String::as_str) == Some("plan-eng-review");
            let source_path_matches =
                headers.get("Source Spec") == Some(&format!("`{source_spec_path}`"));
            let source_revision_matches = headers
                .get("Source Spec Revision")
                .and_then(|value| value.parse::<u32>().ok())
                == Some(source_spec_revision);
            let plan_revision_valid = headers
                .get("Plan Revision")
                .and_then(|value| value.parse::<u32>().ok())
                .is_some();
            if !execution_mode_valid
                || !reviewed_by_valid
                || !source_path_matches
                || !source_revision_matches
                || !plan_revision_valid
            {
                return None;
            }
            path.strip_prefix(repo_root)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
}

fn parse_step_state(
    source: &str,
    plan_document: &PlanDocument,
) -> Result<Vec<PlanStepState>, JsonFailure> {
    let mut step_titles = BTreeMap::new();
    for task in &plan_document.tasks {
        for step in &task.steps {
            step_titles.insert((task.number, step.number), step.text.clone());
        }
    }

    let lines = source.lines().collect::<Vec<_>>();
    let mut current_task = None::<u32>;
    let mut steps = Vec::new();
    let mut line_index = 0;
    while line_index < lines.len() {
        let line = lines[line_index];
        if let Some(rest) = line.strip_prefix("## Task ") {
            current_task = rest
                .split(':')
                .next()
                .and_then(|value| value.parse::<u32>().ok());
            line_index += 1;
            continue;
        }

        if let Some((checked, step_number, title)) = parse_step_line(line) {
            let task_number = current_task.ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::PlanNotExecutionReady,
                    "Plan step headings must live within a task section.",
                )
            })?;
            let canonical_title = step_titles
                .get(&(task_number, step_number))
                .cloned()
                .unwrap_or(title);
            let mut note_state = None;
            let mut note_summary = String::new();
            let mut cursor = line_index + 1;
            while cursor < lines.len() && lines[cursor].is_empty() {
                cursor += 1;
            }
            if cursor < lines.len()
                && let Some((parsed_state, parsed_summary)) = parse_note_line(lines[cursor])
            {
                if parsed_summary.is_empty() {
                    return Err(JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Execution note summaries may not be blank after whitespace normalization.",
                    ));
                }
                if parsed_summary.chars().count() > 120 {
                    return Err(JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Execution note summaries may not exceed 120 characters.",
                    ));
                }
                note_state = Some(parsed_state);
                note_summary = parsed_summary;
                let mut duplicate_cursor = cursor + 1;
                while duplicate_cursor < lines.len() && lines[duplicate_cursor].is_empty() {
                    duplicate_cursor += 1;
                }
                if duplicate_cursor < lines.len()
                    && parse_note_line(lines[duplicate_cursor]).is_some()
                {
                    return Err(JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Plan may have at most one execution note per step.",
                    ));
                }
            }

            steps.push(PlanStepState {
                task_number,
                step_number,
                title: canonical_title,
                checked,
                note_state,
                note_summary,
            });
        }
        line_index += 1;
    }

    Ok(steps)
}

pub(crate) fn parse_step_line(line: &str) -> Option<(bool, u32, String)> {
    let (checked, step) = parse_task_step_projection_line(line).ok()??;
    Some((checked, step.number, step.text))
}

fn parse_note_line(line: &str) -> Option<(NoteState, String)> {
    let rest = line.trim_start().strip_prefix("**Execution Note:** ")?;
    let (state, summary) = rest.split_once(" - ")?;
    let note_state = match state {
        "Active" => NoteState::Active,
        "Blocked" => NoteState::Blocked,
        "Interrupted" => NoteState::Interrupted,
        _ => return None,
    };
    Some((note_state, normalize_whitespace(summary)))
}

fn overlay_authoritative_open_step_state_from_state(
    context: &mut ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
) -> Result<(), JsonFailure> {
    let Some(record) = authoritative_state.current_open_step_state_checked()? else {
        return Ok(());
    };
    if !open_step_record_matches_or_can_share_current_workspace(context, &record) {
        return Ok(());
    }
    if context.plan_document.execution_mode == "none"
        && let Some(execution_mode) = record.execution_mode.as_deref()
    {
        match execution_mode {
            "featureforge:executing-plans" | "featureforge:subagent-driven-development" => {
                context.plan_document.execution_mode = execution_mode.to_owned();
            }
            _ => {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    format!(
                        "Authoritative current_open_step_state for {} has invalid execution_mode `{execution_mode}`.",
                        context.plan_rel
                    ),
                ));
            }
        }
    }
    apply_authoritative_open_step_record_to_steps(
        &mut context.steps,
        &record,
        &context.plan_rel,
        context.plan_document.plan_revision,
    )
}

fn open_step_record_matches_or_can_share_current_workspace(
    context: &ExecutionContext,
    record: &OpenStepStateRecord,
) -> bool {
    let Some(record_repo_root) = record.repo_root.as_deref() else {
        return true;
    };
    let record_repo_root = canonicalize_repo_root_path(Path::new(record_repo_root));
    if record_repo_root == context.runtime.repo_root {
        return true;
    }
    if context
        .evidence
        .source
        .as_deref()
        .is_some_and(|source| source.contains("### Task "))
    {
        return false;
    }
    let Ok(discovered_runtime) = ExecutionRuntime::discover(&record_repo_root) else {
        return false;
    };
    if context.runtime.branch_name == "current"
        || discovered_runtime.branch_name == "current"
        || discovered_runtime.branch_name != context.runtime.branch_name
    {
        return false;
    }
    let record_runtime = ExecutionRuntime {
        state_dir: context.runtime.state_dir.clone(),
        ..discovered_runtime
    };
    let Ok(record_context) = load_execution_context_without_authority_overlay(
        &record_runtime,
        Path::new(&context.plan_rel),
    ) else {
        return false;
    };
    hash_contract_plan(&record_context.plan_source) == hash_contract_plan(&context.plan_source)
        && status_workspace_state_id(&record_context).ok()
            == status_workspace_state_id(context).ok()
}

fn apply_authoritative_open_step_record_to_steps(
    steps: &mut [PlanStepState],
    record: &OpenStepStateRecord,
    plan_rel: &str,
    plan_revision: u32,
) -> Result<(), JsonFailure> {
    if normalize_whitespace(&record.source_plan_path) != plan_rel {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative current_open_step_state for {plan_rel} points to source_plan_path `{}`.",
                record.source_plan_path
            ),
        ));
    }
    if record.source_plan_revision != plan_revision {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative current_open_step_state for {plan_rel} points to source_plan_revision {}, expected {}.",
                record.source_plan_revision, plan_revision
            ),
        ));
    }
    let note_state = match record.note_state.as_str() {
        "Active" => NoteState::Active,
        "Blocked" => NoteState::Blocked,
        "Interrupted" => NoteState::Interrupted,
        _ => {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative current_open_step_state for {plan_rel} has invalid note_state `{}`.",
                    record.note_state
                ),
            ));
        }
    };

    let Some(open_index) = steps
        .iter()
        .position(|step| step.task_number == record.task && step.step_number == record.step)
    else {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative current_open_step_state for {plan_rel} points to missing Task {} Step {}.",
                record.task, record.step
            ),
        ));
    };

    let note_summary = normalize_whitespace(&record.note_summary);
    if note_summary.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative current_open_step_state for {plan_rel} has a blank note_summary.",
            ),
        ));
    }
    if note_summary.chars().count() > 120 {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative current_open_step_state for {plan_rel} has note_summary longer than 120 characters.",
            ),
        ));
    }

    for step in steps.iter_mut() {
        step.note_state = None;
        step.note_summary.clear();
    }
    // Projection lag in the plan markdown must not override authoritative
    // runtime open-step truth once the event log has committed it.
    steps[open_index].checked = false;
    steps[open_index].note_state = Some(note_state);
    steps[open_index].note_summary = note_summary;
    Ok(())
}

struct ExecutionEvidenceProjectionParseInput<'a> {
    runtime: &'a ExecutionRuntime,
    evidence_rel: &'a str,
    evidence_abs: &'a Path,
    plan_source: &'a str,
    expected_plan_path: &'a str,
    plan_document: &'a PlanDocument,
    steps: &'a [PlanStepState],
    expected_spec_path: &'a str,
    source_spec_source: &'a str,
    allow_legacy_unbound: bool,
    allow_tracked_projection: bool,
}

fn parse_execution_evidence_projection(
    input: ExecutionEvidenceProjectionParseInput<'_>,
) -> Result<ExecutionEvidence, JsonFailure> {
    let expected_plan_revision = input.plan_document.plan_revision;
    let expected_spec_revision = input.plan_document.source_spec_revision;
    if let Some(source) = read_state_dir_projection(input.runtime, input.evidence_rel)? {
        match state_dir_evidence_projection_currentness_for_source(
            input.runtime,
            input.evidence_rel,
            &source,
        )? {
            StateDirProjectionCurrentness::Current => {
                return parse_evidence_source(
                    source,
                    EvidenceSourceParseInput {
                        expected_plan_path: input.expected_plan_path,
                        expected_plan_revision,
                        expected_spec_path: input.expected_spec_path,
                        expected_spec_revision,
                        source_origin: EvidenceSourceOrigin::StateDirProjection,
                        tracked_progress_present: false,
                        tracked_source: None,
                    },
                );
            }
            StateDirProjectionCurrentness::Stale => {
                match classify_stale_state_dir_evidence_projection(
                    &input,
                    &source,
                    expected_plan_revision,
                    expected_spec_revision,
                )? {
                    StaleStateDirEvidenceProjection::CurrentEvidence(parsed) => return Ok(parsed),
                    StaleStateDirEvidenceProjection::RuntimeGeneratedReadModel => {}
                }
            }
            StateDirProjectionCurrentness::Unbound => {
                if let Some(parsed) = parse_self_bound_legacy_state_dir_evidence_projection(
                    &source, &input, false, None,
                )? {
                    return Ok(parsed);
                }
                if input.allow_legacy_unbound {
                    return parse_evidence_source(
                        source,
                        EvidenceSourceParseInput {
                            expected_plan_path: input.expected_plan_path,
                            expected_plan_revision,
                            expected_spec_path: input.expected_spec_path,
                            expected_spec_revision,
                            source_origin: EvidenceSourceOrigin::StateDirProjection,
                            tracked_progress_present: false,
                            tracked_source: None,
                        },
                    );
                }
                if evidence_source_has_progress(&source) {
                    return Err(JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "State-dir execution evidence projection is not bound to authoritative runtime state.",
                    ));
                }
            }
        }
    }
    if input.allow_tracked_projection
        && let Some(source) = read_tracked_evidence_source(input.evidence_abs)?
    {
        let tracked_source = Some(source.clone());
        return parse_evidence_source(
            source,
            EvidenceSourceParseInput {
                expected_plan_path: input.expected_plan_path,
                expected_plan_revision,
                expected_spec_path: input.expected_spec_path,
                expected_spec_revision,
                source_origin: EvidenceSourceOrigin::TrackedFile,
                tracked_progress_present: true,
                tracked_source,
            },
        );
    }
    // Tracked execution evidence is an optional export in normal operation.
    // The only tracked read retained here is the legacy pre-harness guard: a
    // legacy evidence file with progress is rejected or imported through the
    // existing legacy policy instead of being treated as a current projection.
    if !authoritative_state_owns_evidence_history(input.runtime, input.expected_plan_path)?
        && let Some(source) = read_tracked_legacy_evidence_source(input.evidence_abs)?
    {
        let tracked_source = Some(source.clone());
        return parse_evidence_source(
            source,
            EvidenceSourceParseInput {
                expected_plan_path: input.expected_plan_path,
                expected_plan_revision,
                expected_spec_path: input.expected_spec_path,
                expected_spec_revision,
                source_origin: EvidenceSourceOrigin::TrackedFile,
                tracked_progress_present: true,
                tracked_source,
            },
        );
    }

    Ok(ExecutionEvidence {
        format: EvidenceFormat::Empty,
        plan_path: input.expected_plan_path.to_owned(),
        plan_revision: expected_plan_revision,
        plan_fingerprint: None,
        source_spec_path: input.expected_spec_path.to_owned(),
        source_spec_revision: expected_spec_revision,
        source_spec_fingerprint: None,
        attempts: Vec::new(),
        source: None,
        source_origin: EvidenceSourceOrigin::Empty,
        tracked_progress_present: false,
        tracked_source: None,
    })
}

enum StaleStateDirEvidenceProjection {
    CurrentEvidence(ExecutionEvidence),
    RuntimeGeneratedReadModel,
}

fn classify_stale_state_dir_evidence_projection(
    input: &ExecutionEvidenceProjectionParseInput<'_>,
    source: &str,
    expected_plan_revision: u32,
    expected_spec_revision: u32,
) -> Result<StaleStateDirEvidenceProjection, JsonFailure> {
    if let Some(expected_fingerprint) =
        state_dir_evidence_projection_expected_fingerprint(input.runtime, input.evidence_rel)?
    {
        let parsed = parse_evidence_source(
            source.to_owned(),
            EvidenceSourceParseInput {
                expected_plan_path: input.expected_plan_path,
                expected_plan_revision,
                expected_spec_path: input.expected_spec_path,
                expected_spec_revision,
                source_origin: EvidenceSourceOrigin::StateDirProjection,
                tracked_progress_present: false,
                tracked_source: None,
            },
        )?;
        let canonical_source = render_canonical_evidence_projection_source(
            input.expected_plan_path,
            input.plan_document,
            input.plan_source,
            input.source_spec_source,
            input.steps,
            &parsed,
        );
        if projection_source_matches_fingerprint(&canonical_source, &expected_fingerprint) {
            return Ok(StaleStateDirEvidenceProjection::CurrentEvidence(parsed));
        }
    }
    if state_dir_projection_matches_recorded_output_fingerprint(
        input.runtime,
        input.evidence_rel,
        source,
    )? {
        // This is an older runtime-generated read model. Do not parse it as
        // authority, but allow the authoritative reducer to rebuild the current
        // read model for normal paths.
        return Ok(StaleStateDirEvidenceProjection::RuntimeGeneratedReadModel);
    }
    Err(JsonFailure::new(
        FailureClass::MalformedExecutionState,
        "State-dir execution evidence projection does not match authoritative runtime state.",
    ))
}

pub(crate) fn validate_state_dir_evidence_projection_before_materialization(
    context: &ExecutionContext,
) -> Result<(), JsonFailure> {
    let Some(source) = read_state_dir_projection(&context.runtime, &context.evidence_rel)? else {
        return Ok(());
    };
    if state_dir_evidence_projection_currentness_for_source(
        &context.runtime,
        &context.evidence_rel,
        &source,
    )? != StateDirProjectionCurrentness::Stale
    {
        return Ok(());
    }
    let input = ExecutionEvidenceProjectionParseInput {
        runtime: &context.runtime,
        evidence_rel: &context.evidence_rel,
        evidence_abs: &context.evidence_abs,
        plan_source: &context.plan_source,
        expected_plan_path: &context.plan_rel,
        plan_document: &context.plan_document,
        steps: &context.steps,
        expected_spec_path: &context.plan_document.source_spec_path,
        source_spec_source: &context.source_spec_source,
        allow_legacy_unbound: false,
        allow_tracked_projection: false,
    };
    classify_stale_state_dir_evidence_projection(
        &input,
        &source,
        context.plan_document.plan_revision,
        context.plan_document.source_spec_revision,
    )?;
    Ok(())
}

fn state_dir_evidence_projection_currentness_for_source(
    runtime: &ExecutionRuntime,
    evidence_rel: &str,
    source: &str,
) -> Result<StateDirProjectionCurrentness, JsonFailure> {
    let Some(expected_fingerprint) =
        state_dir_evidence_projection_expected_fingerprint(runtime, evidence_rel)?
    else {
        return Ok(StateDirProjectionCurrentness::Unbound);
    };
    if projection_source_matches_fingerprint(source, &expected_fingerprint) {
        Ok(StateDirProjectionCurrentness::Current)
    } else {
        Ok(StateDirProjectionCurrentness::Stale)
    }
}

fn state_dir_evidence_projection_expected_fingerprint(
    runtime: &ExecutionRuntime,
    _evidence_rel: &str,
) -> Result<Option<String>, JsonFailure> {
    match authoritative_state_optional_string_field_for_runtime(
        runtime,
        "execution_evidence_projection_fingerprint",
    )? {
        Some(Some(fingerprint)) => Ok(Some(fingerprint)),
        Some(None) | None => Ok(None),
    }
}

fn parse_self_bound_legacy_state_dir_evidence_projection(
    source: &str,
    input: &ExecutionEvidenceProjectionParseInput<'_>,
    tracked_progress_present: bool,
    tracked_source: Option<String>,
) -> Result<Option<ExecutionEvidence>, JsonFailure> {
    let headers = parse_headers(source);
    if !headers.contains_key("Plan Fingerprint") {
        return Ok(None);
    }
    let expected_plan_revision = input.plan_document.plan_revision.to_string();
    let expected_spec_revision = input.plan_document.source_spec_revision.to_string();
    let Some(header_plan_fingerprint) = headers.get("Plan Fingerprint").map(String::as_str) else {
        return Ok(None);
    };
    if !is_canonical_fingerprint(header_plan_fingerprint) {
        return Ok(None);
    }
    let expected_source_spec_fingerprint = sha256_hex(input.source_spec_source.as_bytes());
    if headers.get("Plan Path").map(String::as_str) != Some(input.expected_plan_path)
        || headers.get("Plan Revision").map(String::as_str) != Some(expected_plan_revision.as_str())
        || headers.get("Source Spec Path").map(String::as_str) != Some(input.expected_spec_path)
        || headers.get("Source Spec Revision").map(String::as_str)
            != Some(expected_spec_revision.as_str())
        || headers.get("Source Spec Fingerprint").map(String::as_str)
            != Some(expected_source_spec_fingerprint.as_str())
    {
        return Ok(None);
    }
    let parsed = parse_evidence_source(
        source.to_owned(),
        EvidenceSourceParseInput {
            expected_plan_path: input.expected_plan_path,
            expected_plan_revision: input.plan_document.plan_revision,
            expected_spec_path: input.expected_spec_path,
            expected_spec_revision: input.plan_document.source_spec_revision,
            source_origin: EvidenceSourceOrigin::StateDirProjection,
            tracked_progress_present,
            tracked_source,
        },
    )?;
    let canonical_source = render_canonical_evidence_projection_source_with_fingerprints(
        input.expected_plan_path,
        input.plan_document,
        header_plan_fingerprint,
        &expected_source_spec_fingerprint,
        input.steps,
        &parsed,
    );
    let canonical_fingerprint = sha256_hex(canonical_source.as_bytes());
    let source_matches_canonical =
        projection_source_matches_fingerprint(source, &canonical_fingerprint);
    if source_matches_canonical {
        Ok(Some(parsed))
    } else {
        Ok(None)
    }
}

fn evidence_source_has_progress(source: &str) -> bool {
    source.contains("### Task ")
}

fn authoritative_state_owns_evidence_history(
    runtime: &ExecutionRuntime,
    _expected_plan_path: &str,
) -> Result<bool, JsonFailure> {
    let Some(state_payload) = load_reduced_authoritative_state(runtime)? else {
        return Ok(false);
    };
    let evidence_attempts_present = state_payload
        .get("execution_evidence_attempts")
        .is_some_and(|attempts| !attempts.is_null());
    let event_completed_steps_present = state_payload
        .get("event_completed_steps")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|steps| !steps.is_empty());
    let task_closure_records_present = state_payload
        .get("current_task_closure_records")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|records| !records.is_empty());
    Ok(evidence_attempts_present || event_completed_steps_present || task_closure_records_present)
}

fn read_tracked_legacy_evidence_source(evidence_abs: &Path) -> Result<Option<String>, JsonFailure> {
    let Some(source) = read_tracked_evidence_source(evidence_abs)? else {
        return Ok(None);
    };
    let is_legacy_evidence = !parse_headers(&source).contains_key("Plan Fingerprint");
    Ok(is_legacy_evidence.then_some(source))
}

fn read_tracked_evidence_source(evidence_abs: &Path) -> Result<Option<String>, JsonFailure> {
    let source = match fs::read_to_string(evidence_abs) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Could not read legacy execution evidence {}: {error}",
                    evidence_abs.display()
                ),
            ));
        }
    };
    Ok(evidence_source_has_progress(&source).then_some(source))
}

struct EvidenceSourceParseInput<'a> {
    expected_plan_path: &'a str,
    expected_plan_revision: u32,
    expected_spec_path: &'a str,
    expected_spec_revision: u32,
    source_origin: EvidenceSourceOrigin,
    tracked_progress_present: bool,
    tracked_source: Option<String>,
}

fn parse_evidence_source(
    source: String,
    input: EvidenceSourceParseInput<'_>,
) -> Result<ExecutionEvidence, JsonFailure> {
    let headers = parse_headers(&source);
    let format = if headers.contains_key("Plan Fingerprint") {
        EvidenceFormat::V2
    } else {
        EvidenceFormat::Legacy
    };
    let attempts = parse_evidence_attempts(&source, format)?;
    if attempts.is_empty() {
        return Ok(ExecutionEvidence {
            format: EvidenceFormat::Empty,
            plan_path: input.expected_plan_path.to_owned(),
            plan_revision: input.expected_plan_revision,
            plan_fingerprint: headers.get("Plan Fingerprint").cloned(),
            source_spec_path: headers
                .get("Source Spec Path")
                .cloned()
                .unwrap_or_else(|| input.expected_spec_path.to_owned()),
            source_spec_revision: headers
                .get("Source Spec Revision")
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(input.expected_spec_revision),
            source_spec_fingerprint: headers.get("Source Spec Fingerprint").cloned(),
            attempts,
            source: Some(source),
            source_origin: input.source_origin,
            tracked_progress_present: input.tracked_progress_present,
            tracked_source: input.tracked_source,
        });
    }

    Ok(ExecutionEvidence {
        format,
        plan_path: headers
            .get("Plan Path")
            .cloned()
            .unwrap_or_else(|| input.expected_plan_path.to_owned()),
        plan_revision: headers
            .get("Plan Revision")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(input.expected_plan_revision),
        plan_fingerprint: headers.get("Plan Fingerprint").cloned(),
        source_spec_path: headers
            .get("Source Spec Path")
            .cloned()
            .unwrap_or_else(|| input.expected_spec_path.to_owned()),
        source_spec_revision: headers
            .get("Source Spec Revision")
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(input.expected_spec_revision),
        source_spec_fingerprint: headers.get("Source Spec Fingerprint").cloned(),
        attempts,
        source: Some(source),
        source_origin: input.source_origin,
        tracked_progress_present: input.tracked_progress_present,
        tracked_source: input.tracked_source,
    })
}

fn parse_evidence_attempts(
    source: &str,
    format: EvidenceFormat,
) -> Result<Vec<EvidenceAttempt>, JsonFailure> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut attempts = Vec::new();
    let mut next_attempt_by_step = BTreeMap::<(u32, u32), u32>::new();
    let mut line_index = 0;
    let mut current_task = None::<u32>;
    let mut current_step = None::<u32>;

    while line_index < lines.len() {
        let line = lines[line_index];
        if let Some(rest) = line.strip_prefix("### Task ") {
            let (task, step) = rest.split_once(" Step ").ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence step heading is malformed.",
                )
            })?;
            current_task = task.parse::<u32>().ok();
            current_step = step.parse::<u32>().ok();
            line_index += 1;
            continue;
        }

        if let Some(rest) = line.strip_prefix("#### Attempt ") {
            let task_number = current_task.ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence attempt is missing its step heading.",
                )
            })?;
            let step_number = current_step.ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence attempt is missing its step heading.",
                )
            })?;
            let attempt_number = rest.parse::<u32>().map_err(|_| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence attempt number is malformed.",
                )
            })?;
            let expected_attempt = next_attempt_by_step
                .get(&(task_number, step_number))
                .copied()
                .unwrap_or(1);
            if attempt_number != expected_attempt {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence attempts must start at 1 and increase sequentially per step.",
                ));
            }
            next_attempt_by_step.insert((task_number, step_number), expected_attempt + 1);

            let mut status = String::new();
            let mut recorded_at = String::new();
            let mut execution_source = String::new();
            let mut claim = String::new();
            let mut files = Vec::new();
            let mut file_proofs = Vec::new();
            let mut verify_command = None;
            let mut verification_summary = String::new();
            let mut invalidation_reason = String::new();
            let mut packet_fingerprint = None;
            let mut head_sha = None;
            let mut base_sha = None;
            let mut source_contract_path = None;
            let mut source_contract_fingerprint = None;
            let mut source_evaluation_report_fingerprint = None;
            let mut evaluator_verdict = None;
            let mut failing_criterion_ids = Vec::new();
            let mut source_handoff_fingerprint = None;
            let mut repo_state_baseline_head_sha = None;
            let mut repo_state_baseline_worktree_fingerprint = None;

            line_index += 1;
            while line_index < lines.len() {
                let line = lines[line_index];
                if line.starts_with("#### Attempt ") || line.starts_with("### Task ") {
                    line_index = line_index.saturating_sub(1);
                    break;
                }

                if let Some(value) = line.strip_prefix("**Status:** ") {
                    status = normalize_whitespace(value);
                } else if let Some(value) = line.strip_prefix("**Recorded At:** ") {
                    recorded_at = value.to_owned();
                } else if let Some(value) = line.strip_prefix("**Execution Source:** ") {
                    execution_source = normalize_whitespace(value);
                } else if let Some(value) = line.strip_prefix("**Packet Fingerprint:** ") {
                    packet_fingerprint = Some(normalize_whitespace(value));
                } else if let Some(value) = line.strip_prefix("**Head SHA:** ") {
                    head_sha = Some(normalize_whitespace(value));
                } else if let Some(value) = line.strip_prefix("**Base SHA:** ") {
                    base_sha = Some(normalize_whitespace(value));
                } else if let Some(value) = line.strip_prefix("**Claim:** ") {
                    claim = normalize_whitespace(value);
                } else if let Some(value) = line.strip_prefix("**Source Contract Path:** ") {
                    source_contract_path = parse_optional_evidence_scalar(value);
                } else if let Some(value) = line.strip_prefix("**Source Contract Fingerprint:** ") {
                    source_contract_fingerprint = parse_optional_evidence_scalar(value);
                } else if let Some(value) =
                    line.strip_prefix("**Source Evaluation Report Fingerprint:** ")
                {
                    source_evaluation_report_fingerprint = parse_optional_evidence_scalar(value);
                } else if let Some(value) = line.strip_prefix("**Evaluator Verdict:** ") {
                    evaluator_verdict = parse_optional_evidence_scalar(value);
                } else if line == "**Failing Criterion IDs:**" {
                    line_index += 1;
                    while line_index < lines.len() {
                        let criterion_line = lines[line_index].trim();
                        if criterion_line.is_empty() {
                            line_index += 1;
                            continue;
                        }
                        if criterion_line == "[]" {
                            line_index += 1;
                            continue;
                        }
                        if criterion_line.starts_with("**")
                            || criterion_line.starts_with("### ")
                            || criterion_line.starts_with("#### ")
                        {
                            line_index = line_index.saturating_sub(1);
                            break;
                        }
                        if let Some(value) = criterion_line.strip_prefix("- ") {
                            if let Some(criterion_id) = parse_optional_evidence_scalar(value) {
                                failing_criterion_ids.push(criterion_id);
                            }
                            line_index += 1;
                            continue;
                        }
                        line_index = line_index.saturating_sub(1);
                        break;
                    }
                } else if let Some(value) = line.strip_prefix("**Source Handoff Fingerprint:** ") {
                    source_handoff_fingerprint = parse_optional_evidence_scalar(value);
                } else if let Some(value) = line.strip_prefix("**Repo State Baseline Head SHA:** ")
                {
                    repo_state_baseline_head_sha = parse_optional_evidence_scalar(value);
                } else if let Some(value) =
                    line.strip_prefix("**Repo State Baseline Worktree Fingerprint:** ")
                {
                    repo_state_baseline_worktree_fingerprint =
                        parse_optional_evidence_scalar(value);
                } else if line == "**Files Proven:**" {
                    line_index += 1;
                    while line_index < lines.len() {
                        let proof_line = lines[line_index];
                        if let Some(proof_entry) = proof_line.strip_prefix("- ") {
                            let (path, proof) = proof_entry.split_once(" | ").ok_or_else(|| {
                                JsonFailure::new(
                                    FailureClass::MalformedExecutionState,
                                    "Execution evidence Files Proven bullets must include a proof suffix.",
                                )
                            })?;
                            let path = normalize_persisted_file_path(path).map_err(|_| {
                                JsonFailure::new(
                                    FailureClass::MalformedExecutionState,
                                    "Execution evidence Files Proven bullets must use canonical repo-relative paths.",
                                )
                            })?;
                            files.push(path.clone());
                            file_proofs.push(FileProof {
                                path,
                                proof: proof.to_owned(),
                            });
                            line_index += 1;
                            continue;
                        }
                        line_index = line_index.saturating_sub(1);
                        break;
                    }
                } else if line == "**Files:**" {
                    line_index += 1;
                    while line_index < lines.len() {
                        let legacy_line = lines[line_index];
                        if let Some(path) = legacy_line.strip_prefix("- ") {
                            let path = normalize_persisted_file_path(path).map_err(|_| {
                                JsonFailure::new(
                                    FailureClass::MalformedExecutionState,
                                    "Execution evidence Files bullets must use canonical repo-relative paths.",
                                )
                            })?;
                            files.push(path.clone());
                            file_proofs.push(FileProof {
                                path,
                                proof: String::from("sha256:unknown"),
                            });
                            line_index += 1;
                            continue;
                        }
                        line_index = line_index.saturating_sub(1);
                        break;
                    }
                } else if let Some(value) = line.strip_prefix("**Verify Command:** ") {
                    verify_command = parse_optional_evidence_scalar(value).or_else(|| {
                        Some(normalize_whitespace(value)).filter(|candidate| !candidate.is_empty())
                    });
                } else if let Some(value) = line.strip_prefix("**Verification Summary:** ") {
                    verification_summary = normalize_whitespace(value);
                } else if line == "**Verification:**" {
                    line_index += 1;
                    if line_index < lines.len()
                        && let Some(value) = lines[line_index].strip_prefix("- ")
                    {
                        verification_summary = normalize_whitespace(value);
                    }
                } else if let Some(value) = line.strip_prefix("**Invalidation Reason:** ") {
                    invalidation_reason = normalize_whitespace(value);
                }

                line_index += 1;
            }

            if !matches!(status.as_str(), "Completed" | "Invalidated") {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence status must be Completed or Invalidated.",
                ));
            }
            if recorded_at.trim().is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence recorded-at timestamps may not be blank.",
                ));
            }
            if execution_source.is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence source may not be blank.",
                ));
            }
            if !matches!(
                execution_source.as_str(),
                "featureforge:executing-plans" | "featureforge:subagent-driven-development"
            ) {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence source must be one of the supported execution modes.",
                ));
            }
            if claim.is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence claims may not be blank after whitespace normalization.",
                ));
            }
            if files.is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence must include at least one repo-relative file entry.",
                ));
            }
            if verification_summary.is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence verification summaries may not be blank after whitespace normalization.",
                ));
            }
            if invalidation_reason.is_empty() {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Execution evidence invalidation reasons may not be blank after whitespace normalization.",
                ));
            }
            if status == "Invalidated" && invalidation_reason == "N/A" {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Invalidated execution evidence must carry a real invalidation reason.",
                ));
            }

            let verify_command = verify_command
                .or_else(|| parse_command_verification_summary(&verification_summary));

            attempts.push(EvidenceAttempt {
                task_number,
                step_number,
                attempt_number,
                status,
                recorded_at,
                execution_source,
                claim,
                files,
                file_proofs,
                verify_command,
                verification_summary,
                invalidation_reason,
                packet_fingerprint,
                head_sha,
                base_sha,
                source_contract_path,
                source_contract_fingerprint,
                source_evaluation_report_fingerprint,
                evaluator_verdict,
                failing_criterion_ids,
                source_handoff_fingerprint,
                repo_state_baseline_head_sha,
                repo_state_baseline_worktree_fingerprint,
            });
        }

        line_index += 1;
    }

    if format == EvidenceFormat::V2 && attempts.is_empty() && source.contains("### Task ") {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "Execution evidence v2 attempts could not be parsed.",
        ));
    }
    Ok(attempts)
}

fn parse_optional_evidence_scalar(value: &str) -> Option<String> {
    let normalized = normalize_whitespace(value);
    let trimmed = normalized.trim().trim_matches('`').trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn execution_state_fingerprint_source(context: &ExecutionContext) -> String {
    let mut source = String::new();
    source.push_str("execution-mode=");
    source.push_str(&context.plan_document.execution_mode);
    source.push('\n');
    for step in &context.steps {
        source.push_str("step=");
        source.push_str(&step.task_number.to_string());
        source.push('.');
        source.push_str(&step.step_number.to_string());
        source.push_str(";checked=");
        source.push_str(if step.checked { "true" } else { "false" });
        source.push_str(";note=");
        if let Some(note_state) = step.note_state {
            source.push_str(note_state.as_str());
        } else {
            source.push_str("none");
        }
        source.push_str(";summary=");
        source.push_str(&step.note_summary);
        source.push('\n');
    }
    source
}

fn compute_execution_fingerprint(
    plan_source: &str,
    evidence_source: Option<&str>,
    authoritative_evidence_projection_fingerprint: Option<&str>,
    execution_state_source: &str,
) -> String {
    let mut payload = String::from("plan\n");
    payload.push_str(plan_source);
    payload.push_str("\n--evidence--\n");
    if let Some(fingerprint) = authoritative_evidence_projection_fingerprint {
        payload.push_str("authoritative-projection-fingerprint=");
        payload.push_str(fingerprint);
        payload.push('\n');
    } else if let Some(source) = evidence_source {
        if source.contains("### Task ") {
            payload.push_str(source);
        } else {
            payload.push_str("__EMPTY_EVIDENCE__\n");
        }
    } else {
        payload.push_str("__EMPTY_EVIDENCE__\n");
    }
    payload.push_str("\n--execution-state--\n");
    payload.push_str(execution_state_source);
    sha256_hex(payload.as_bytes())
}

fn parse_contract_render(source: &str) -> String {
    let known_runtime_steps = known_runtime_step_projection_lines(source);
    let lines = source.lines().collect::<Vec<_>>();
    let mut rendered = Vec::new();
    let mut current_task = None::<u32>;
    let mut current_task_files_seen = false;
    let mut in_fenced_block = false;
    let mut pending_note_after_step = false;
    let mut suppress_note_block = None::<RuntimeExecutionNoteProjectionBlock>;

    for line in lines {
        if let Some(note_block) = suppress_note_block {
            if note_block.continues(line) {
                continue;
            }
            suppress_note_block = None;
        }
        if pending_note_after_step {
            if line.trim().is_empty() {
                continue;
            }
            pending_note_after_step = false;
            if let Some(note_block) = RuntimeExecutionNoteProjectionBlock::start(line) {
                suppress_note_block = Some(note_block);
                continue;
            }
        }
        if let Some(rest) = line.strip_prefix("## Task ") {
            current_task = rest
                .split(':')
                .next()
                .and_then(|value| value.parse::<u32>().ok());
            current_task_files_seen = false;
            in_fenced_block = false;
            rendered.push(line.to_owned());
            continue;
        }
        if line.starts_with("## ") {
            current_task = None;
            current_task_files_seen = false;
            in_fenced_block = false;
        }
        let trimmed = line.trim();
        if trimmed == "**Files:**" {
            current_task_files_seen = true;
        }
        if trimmed.starts_with("```") {
            in_fenced_block = !in_fenced_block;
            rendered.push(line.to_owned());
            continue;
        }
        if line.starts_with("**Execution Mode:** ") {
            rendered.push(String::from("**Execution Mode:** none"));
            continue;
        }
        if let Some((_, step_number, title)) = parse_step_line(line) {
            let known_step = current_task
                .and_then(|task_number| known_runtime_steps.get(&(task_number, step_number)))
                .is_some_and(|known_title| known_title == &title);
            if in_fenced_block || !current_task_files_seen || !known_step {
                rendered.push(line.to_owned());
                continue;
            }
            rendered.push(format!("- [ ] **Step {step_number}: {title}**"));
            pending_note_after_step = true;
            continue;
        }
        rendered.push(line.to_owned());
    }

    format!("{}\n", rendered.join("\n"))
}

pub(crate) fn prior_task_number_for_begin(
    context: &ExecutionContext,
    target_task: u32,
) -> Option<u32> {
    context
        .tasks_by_number
        .keys()
        .copied()
        .filter(|task_number| *task_number < target_task)
        .max()
}

pub(crate) fn require_prior_task_closure_for_begin(
    context: &ExecutionContext,
    target_task: u32,
) -> Result<(), JsonFailure> {
    let Some(prior_task) = prior_task_number_for_begin(context, target_task) else {
        return Ok(());
    };

    if prior_task_cycle_break_active(context, prior_task)? {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "task_cycle_break_active",
            format!(
                "Task {prior_task} is in cycle-break remediation; Task {target_task} may not begin until remediation closes."
            ),
        ));
    }

    if current_task_closure_overlay_restore_required(context)? {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "current_task_closure_overlay_restore_required",
            format!(
                "Task {target_task} may not begin because current task-closure overlays are missing and must be repaired before task-boundary advancement can continue. Run `featureforge plan execution repair-review-state --plan {}` before starting Task {target_task}.",
                context.plan_rel
            ),
        ));
    }

    match task_current_closure_status(context, prior_task)? {
        TaskCurrentClosureStatus::Current => return Ok(()),
        TaskCurrentClosureStatus::Stale => {
            return Err(task_boundary_error(
                FailureClass::ExecutionStateNotReady,
                "prior_task_current_closure_stale",
                format!(
                    "Task {target_task} may not begin because Task {prior_task} current task closure no longer matches the current reviewed workspace state. Run `featureforge plan execution repair-review-state --plan {}` before starting Task {target_task}.",
                    context.plan_rel
                ),
            ));
        }
        TaskCurrentClosureStatus::Missing => {}
    }

    ensure_prior_task_current_closure_record(context, prior_task, target_task)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskClosureBaselineRepairCandidate {
    pub(crate) task: u32,
    pub(crate) dispatch_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskClosureRecordingPrerequisites {
    pub(crate) task: u32,
    pub(crate) dispatch_id: Option<String>,
    pub(crate) blocking_reason_codes: Vec<String>,
}

fn task_closure_recording_reason_code(reason_code: &str) -> bool {
    matches!(
        reason_code,
        "prior_task_review_dispatch_missing"
            | "prior_task_review_dispatch_stale"
            | "prior_task_review_not_green"
            | "prior_task_verification_missing"
            | "prior_task_verification_missing_legacy"
            | "task_review_not_independent"
            | "task_review_artifact_malformed"
            | "task_verification_summary_malformed"
            | "prior_task_current_closure_stale"
    )
}

fn task_closure_dispatch_lineage_reason_code(reason_code: &str) -> bool {
    matches!(
        reason_code,
        "prior_task_review_dispatch_missing" | "prior_task_review_dispatch_stale"
    )
}

fn push_task_closure_recording_reason_code_once(reason_codes: &mut Vec<String>, reason_code: &str) {
    if !reason_codes.iter().any(|existing| existing == reason_code) {
        reason_codes.push(reason_code.to_owned());
    }
}

fn task_closure_recording_blocking_reason_codes(
    task: u32,
    dispatch_id: Option<&str>,
    current_semantic_reviewed_state_id: Option<&str>,
    _current_raw_reviewed_state_id: Option<&str>,
    overlay: Option<&StatusAuthoritativeOverlay>,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Vec<String> {
    let mut blocking_reason_codes = Vec::new();
    let lineage_key = format!("task-{task}");
    let lineage_record =
        overlay.and_then(|overlay| overlay.strategy_review_dispatch_lineage.get(&lineage_key));
    let lineage_dispatch_id = lineage_record
        .and_then(|record| record.dispatch_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let lineage_semantic_reviewed_state_id = lineage_record
        .and_then(|record| record.semantic_reviewed_state_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let dispatch_id = dispatch_id.map(str::trim).filter(|value| !value.is_empty());
    let current_semantic_reviewed_state_id = current_semantic_reviewed_state_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let lineage_reviewed_state_matches = current_semantic_reviewed_state_id
        .zip(lineage_semantic_reviewed_state_id)
        .is_some_and(|(current, lineage)| current == lineage);
    match (dispatch_id, lineage_dispatch_id) {
        (None, Some(_)) | (Some(_), None) => {
            push_task_closure_recording_reason_code_once(
                &mut blocking_reason_codes,
                "prior_task_review_dispatch_stale",
            );
        }
        (Some(dispatch_id), Some(lineage_dispatch_id)) if dispatch_id != lineage_dispatch_id => {
            push_task_closure_recording_reason_code_once(
                &mut blocking_reason_codes,
                "prior_task_review_dispatch_stale",
            );
        }
        _ => {}
    }
    if dispatch_id.is_some() && !lineage_reviewed_state_matches {
        push_task_closure_recording_reason_code_once(
            &mut blocking_reason_codes,
            "prior_task_review_dispatch_stale",
        );
    }
    let current_reviewed_state_id = current_semantic_reviewed_state_id;
    if authoritative_state
        .and_then(|state| state.task_closure_negative_result(task))
        .is_some_and(|negative_result| {
            let Some(negative_result_reviewed_state_id) =
                negative_result.semantic_reviewed_state_id.as_deref()
            else {
                return false;
            };
            task_closure_negative_result_blocks_current_reviewed_state(
                negative_result_reviewed_state_id,
                current_reviewed_state_id,
            )
        })
    {
        push_task_closure_recording_reason_code_once(
            &mut blocking_reason_codes,
            "prior_task_review_not_green",
        );
    }
    blocking_reason_codes
}

pub(crate) fn push_task_closure_pending_verification_reason_codes_for_run(
    context: &ExecutionContext,
    task: u32,
    execution_run_id: &str,
    treat_missing_review_receipts_as_malformed: bool,
    blocking_reason_codes: &mut Vec<String>,
) -> Result<(), JsonFailure> {
    let mut any_review_receipt_present = false;
    let mut all_review_receipts_valid = true;

    for step_state in context
        .steps
        .iter()
        .filter(|step_state| step_state.task_number == task && step_state.checked)
    {
        let review_receipt_path = authoritative_unit_review_receipt_path(
            context,
            execution_run_id,
            task,
            step_state.step_number,
        );
        let review_receipt_metadata = match fs::symlink_metadata(&review_receipt_path) {
            Ok(metadata) => {
                any_review_receipt_present = true;
                metadata
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                all_review_receipts_valid = false;
                if treat_missing_review_receipts_as_malformed {
                    push_task_closure_recording_reason_code_once(
                        blocking_reason_codes,
                        "task_review_artifact_malformed",
                    );
                }
                continue;
            }
            Err(_) => {
                any_review_receipt_present = true;
                all_review_receipts_valid = false;
                push_task_closure_recording_reason_code_once(
                    blocking_reason_codes,
                    "task_review_artifact_malformed",
                );
                continue;
            }
        };
        if review_receipt_metadata.file_type().is_symlink() || !review_receipt_metadata.is_file() {
            all_review_receipts_valid = false;
            push_task_closure_recording_reason_code_once(
                blocking_reason_codes,
                "task_review_artifact_malformed",
            );
            continue;
        }
        let review_document = parse_artifact_document(&review_receipt_path);
        if review_document.title.as_deref() != Some("# Unit Review Result")
            || review_document
                .headers
                .get("Review Stage")
                .map(String::as_str)
                != Some("featureforge:unit-review")
            || review_document.headers.get("Result").map(String::as_str) != Some("pass")
            || review_document
                .headers
                .get("Generated By")
                .map(String::as_str)
                != Some("featureforge:unit-review")
        {
            all_review_receipts_valid = false;
            push_task_closure_recording_reason_code_once(
                blocking_reason_codes,
                "task_review_artifact_malformed",
            );
            continue;
        }
        match review_document
            .headers
            .get("Reviewer Provenance")
            .map(String::as_str)
        {
            Some("dedicated-independent") => {}
            Some(_) => {
                all_review_receipts_valid = false;
                push_task_closure_recording_reason_code_once(
                    blocking_reason_codes,
                    "task_review_not_independent",
                );
            }
            None => {
                all_review_receipts_valid = false;
                push_task_closure_recording_reason_code_once(
                    blocking_reason_codes,
                    "task_review_artifact_malformed",
                );
            }
        }
    }

    if !any_review_receipt_present || !all_review_receipts_valid {
        return Ok(());
    }

    let verification_receipt_path =
        authoritative_task_verification_receipt_path(context, execution_run_id, task);
    let verification_receipt_metadata = match fs::symlink_metadata(&verification_receipt_path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            push_task_closure_recording_reason_code_once(
                blocking_reason_codes,
                "prior_task_verification_missing",
            );
            None
        }
        Err(_) => {
            push_task_closure_recording_reason_code_once(
                blocking_reason_codes,
                "task_verification_summary_malformed",
            );
            None
        }
    };
    if let Some(verification_receipt_metadata) = verification_receipt_metadata {
        if verification_receipt_metadata.file_type().is_symlink()
            || !verification_receipt_metadata.is_file()
        {
            push_task_closure_recording_reason_code_once(
                blocking_reason_codes,
                "task_verification_summary_malformed",
            );
        } else {
            let verification_document = parse_artifact_document(&verification_receipt_path);
            let task_header_matches = verification_document
                .headers
                .get("Task Number")
                .and_then(|value| value.trim().parse::<u32>().ok())
                == Some(task);
            if verification_document.title.as_deref() != Some("# Task Verification Result")
                || verification_document
                    .headers
                    .get("Result")
                    .map(String::as_str)
                    != Some("pass")
                || verification_document
                    .headers
                    .get("Generated By")
                    .map(String::as_str)
                    != Some("featureforge:verification-before-completion")
                || verification_document
                    .headers
                    .get("Execution Run ID")
                    .map(String::as_str)
                    != Some(execution_run_id)
                || verification_document
                    .headers
                    .get("Source Plan")
                    .map(String::as_str)
                    != Some(context.plan_rel.as_str())
                || !task_header_matches
            {
                push_task_closure_recording_reason_code_once(
                    blocking_reason_codes,
                    "task_verification_summary_malformed",
                );
            }
        }
    }

    Ok(())
}

pub(crate) fn task_closure_negative_result_blocks_current_reviewed_state(
    negative_result_reviewed_state_id: &str,
    current_reviewed_state_id: Option<&str>,
) -> bool {
    current_reviewed_state_id.is_some_and(|reviewed_state_id| {
        !reviewed_state_id.trim().is_empty()
            && reviewed_state_id == negative_result_reviewed_state_id
    })
}

pub(crate) fn task_closure_recording_prerequisites(
    context: &ExecutionContext,
    task: u32,
) -> Result<TaskClosureRecordingPrerequisites, JsonFailure> {
    let current_lineage_fingerprint = task_completion_lineage_fingerprint(context, task);
    let current_semantic_reviewed_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    let overlay = load_status_authoritative_overlay_checked(context)?;
    let authoritative_state = load_authoritative_transition_state(context)?;
    let dispatch_args = RecordReviewDispatchArgs {
        plan: context.plan_abs.clone(),
        scope: ReviewDispatchScopeArg::Task,
        task: Some(task),
    };
    let dispatch_id = shared_current_task_review_dispatch_id(
        Some(task),
        current_lineage_fingerprint.as_deref(),
        current_semantic_reviewed_state_id.as_deref(),
        None,
        overlay.as_ref(),
    )
    .or_else(|| {
        current_review_dispatch_id_if_still_current(context, &dispatch_args)
            .ok()
            .flatten()
    })
    .or_else(|| {
        authoritative_state
            .as_ref()
            .and_then(|state| state.task_review_dispatch_id(task))
    });
    let mut blocking_reason_codes = task_closure_recording_blocking_reason_codes(
        task,
        dispatch_id.as_deref(),
        current_semantic_reviewed_state_id.as_deref(),
        None,
        overlay.as_ref(),
        authoritative_state.as_ref(),
    );
    let dispatch_lineage_blocked = blocking_reason_codes
        .iter()
        .any(|reason_code| task_closure_dispatch_lineage_reason_code(reason_code));
    let current_positive_task_closure_present = authoritative_state.as_ref().is_some_and(|state| {
        task_current_closure_status_from_authoritative_state(context, task, state)
            .is_ok_and(|status| status == TaskCurrentClosureStatus::Current)
    });
    if !current_positive_task_closure_present
        && !dispatch_lineage_blocked
        && dispatch_id
            .as_deref()
            .is_some_and(|dispatch_id| !dispatch_id.trim().is_empty())
        && let Some(execution_run_id) = current_execution_run_id(context)?
    {
        push_task_closure_pending_verification_reason_codes_for_run(
            context,
            task,
            execution_run_id.as_str(),
            false,
            &mut blocking_reason_codes,
        )?;
    }
    Ok(TaskClosureRecordingPrerequisites {
        task,
        dispatch_id,
        blocking_reason_codes,
    })
}

fn task_cycle_break_reason_targets_repaired_task(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
) -> Result<bool, JsonFailure> {
    if !status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "task_cycle_break_active")
    {
        return Ok(true);
    }
    let cycle_break_binding = load_status_authoritative_overlay_checked(context)?.map(|overlay| {
        (
            overlay.strategy_cycle_break_task,
            overlay.strategy_cycle_break_step,
            normalize_optional_overlay_value(
                overlay
                    .strategy_cycle_break_checkpoint_fingerprint
                    .as_deref(),
            )
            .map(str::to_owned),
        )
    });
    if let Some((Some(bound_cycle_break_task), _bound_step, _bound_checkpoint_fingerprint)) =
        cycle_break_binding
    {
        return Ok(bound_cycle_break_task == task);
    }
    if matches!(
        cycle_break_binding.as_ref(),
        Some((None, Some(_), _)) | Some((None, _, Some(_)))
    ) {
        return Ok(false);
    }
    Ok(status.blocking_task == Some(task))
}

pub(crate) fn stale_unreviewed_allows_task_closure_baseline_bridge(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
) -> Result<bool, JsonFailure> {
    stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target(
        context,
        status,
        task,
        projected_earliest_stale_task_from_status(status)
            .or_else(|| pre_reducer_earliest_unresolved_stale_task(context, status)),
    )
}

pub(crate) fn projected_earliest_stale_task_from_status(
    status: &PlanExecutionStatus,
) -> Option<u32> {
    status
        .blocking_records
        .iter()
        .filter(|record| record.scope_type == "task")
        .filter_map(|record| task_number_from_task_scope_key(&record.scope_key))
        .chain(
            status
                .stale_unreviewed_closures
                .iter()
                .filter_map(|record_id| task_number_from_task_scope_key(record_id)),
        )
        .chain(
            status
                .public_repair_targets
                .iter()
                .filter_map(|target| target.task),
        )
        .min()
}

fn task_number_from_task_scope_key(scope_key: &str) -> Option<u32> {
    let raw = scope_key.strip_prefix("task-")?;
    let digits = raw
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    (!digits.is_empty())
        .then(|| digits.parse::<u32>().ok())
        .flatten()
}

pub(crate) fn stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    earliest_unresolved_stale_task: Option<u32>,
) -> Result<bool, JsonFailure> {
    let reducer_stale_reentry_targets_task = status.execution_reentry_target_source.as_deref()
        == Some("closure_graph_stale_target")
        && earliest_unresolved_stale_task.is_some_and(|earliest_task| earliest_task == task);
    if status.review_state_status != "stale_unreviewed" && !reducer_stale_reentry_targets_task {
        return Ok(false);
    }
    let task_steps = context
        .steps
        .iter()
        .filter(|step| step.task_number == task)
        .collect::<Vec<_>>();
    if task_steps.is_empty() || task_steps.iter().any(|step| !step.checked) {
        return Ok(false);
    }

    if earliest_unresolved_stale_task.unwrap_or(task) != task {
        return Ok(false);
    }
    if earliest_unresolved_stale_task.is_some_and(|earliest_task| earliest_task < task) {
        return Ok(false);
    }
    if status.blocking_step.is_some() {
        return Ok(false);
    }
    if task_scope_structural_review_state_reason(status).is_some() {
        return Ok(false);
    }
    if status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
                | "late_stage_surface_not_declared"
                | "prior_task_review_not_green"
        )
    }) {
        return Ok(false);
    }
    if !task_closure_recording_runtime_truth_ready(context, task)? {
        return Ok(false);
    }
    let authoritative_state = load_authoritative_transition_state(context)?;
    if reducer_stale_reentry_targets_task
        && !authoritative_task_closure_history_lineage_present(authoritative_state.as_ref(), task)
    {
        return Ok(false);
    }
    if !task_cycle_break_reason_targets_repaired_task(context, status, task)? {
        return Ok(false);
    }
    let task_boundary_stale_truth_blocker = status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "prior_task_current_closure_missing"
                | "prior_task_current_closure_stale"
                | "task_closure_baseline_repair_candidate"
                | "task_cycle_break_active"
        )
    });
    if !task_boundary_stale_truth_blocker && !reducer_stale_reentry_targets_task {
        return Ok(false);
    }
    if status.active_task == Some(task) {
        return Ok(false);
    }
    let current_reviewed_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    if authoritative_state
        .and_then(|state| state.task_closure_negative_result(task))
        .is_some_and(|negative_result| {
            task_closure_negative_result_blocks_current_reviewed_state(
                negative_result
                    .semantic_reviewed_state_id
                    .as_deref()
                    .unwrap_or(negative_result.reviewed_state_id.as_str()),
                current_reviewed_state_id.as_deref(),
            )
        })
    {
        return Ok(false);
    }
    Ok(true)
}

fn authoritative_task_closure_history_lineage_present(
    authoritative_state: Option<&AuthoritativeTransitionState>,
    task: u32,
) -> bool {
    authoritative_state.is_some_and(|state| {
        state.task_closure_history_records().iter().any(|record| {
            record.task == task
                && !record.dispatch_id.trim().is_empty()
                && record
                    .execution_run_id
                    .as_deref()
                    .is_some_and(|execution_run_id| !execution_run_id.trim().is_empty())
        })
    })
}

pub(crate) fn task_closure_baseline_repair_candidate_with_stale_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    earliest_unresolved_stale_task: Option<u32>,
) -> Result<Option<TaskClosureBaselineRepairCandidate>, JsonFailure> {
    let task_steps = context
        .steps
        .iter()
        .filter(|step| step.task_number == task)
        .collect::<Vec<_>>();
    if task_steps.is_empty() || task_steps.iter().any(|step| !step.checked) {
        return Ok(None);
    }
    if earliest_unresolved_stale_task.is_some_and(|earliest_task| earliest_task < task) {
        return Ok(None);
    }
    let Some(authoritative_state) = load_authoritative_transition_state(context)? else {
        return Ok(None);
    };
    if authoritative_state.execution_run_id_opt().is_none() {
        return Ok(None);
    }
    let strategy_checkpoint_present =
        authoritative_strategy_checkpoint_fingerprint_checked(context)?.is_some();
    let current_reviewed_state_id = semantic_workspace_snapshot(context)
        .ok()
        .map(|snapshot| snapshot.semantic_workspace_tree_id);
    if current_reviewed_state_id
        .as_deref()
        .is_none_or(|reviewed_state_id| reviewed_state_id.trim().is_empty())
    {
        return Ok(None);
    }
    match task_current_closure_status_from_authoritative_state(context, task, &authoritative_state)
    {
        Ok(TaskCurrentClosureStatus::Missing) => {}
        Ok(TaskCurrentClosureStatus::Current | TaskCurrentClosureStatus::Stale) | Err(_) => {
            // Current positive task-closure records are authoritative. Review/verification
            // markdown projections cannot create a task-boundary repair lane once the shared
            // currentness classifier sees a task-closure record for this task.
            return Ok(None);
        }
    }
    let prerequisites = task_closure_recording_prerequisites(context, task)?;
    let mut dispatch_id = prerequisites
        .dispatch_id
        .clone()
        .or_else(|| authoritative_state.task_review_dispatch_id(task));
    let next_unchecked_task = context
        .steps
        .iter()
        .find(|step| !step.checked)
        .map(|step| step.task_number);
    let task_scope_matches_task = (status.blocking_step.is_none()
        && status.blocking_task == Some(task))
        || status.active_task == Some(task)
        || status.resume_task == Some(task)
        || next_unchecked_task.is_some_and(|next_task| task < next_task)
        || (next_unchecked_task.is_none()
            && context.tasks_by_number.keys().copied().max() == Some(task));
    let closure_recording_runtime_truth_ready =
        task_scope_matches_task && task_closure_recording_runtime_truth_ready(context, task)?;
    let stale_bridge_allowed =
        stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target(
            context,
            status,
            task,
            earliest_unresolved_stale_task,
        )?;
    if !strategy_checkpoint_present {
        return Ok(None);
    }
    if !closure_recording_runtime_truth_ready {
        return Ok(None);
    }
    let dispatch_args = RecordReviewDispatchArgs {
        plan: context.plan_abs.clone(),
        scope: ReviewDispatchScopeArg::Task,
        task: Some(task),
    };
    if let Some(current_dispatch_id) =
        current_review_dispatch_id_if_still_current(context, &dispatch_args)?
    {
        dispatch_id = Some(current_dispatch_id);
    }
    let close_current_task_bridge_blocked =
        prerequisites
            .blocking_reason_codes
            .iter()
            .any(|reason_code| {
                matches!(
                    reason_code.as_str(),
                    "prior_task_review_not_green"
                        | "prior_task_current_closure_stale"
                            if !stale_bridge_allowed
                )
            });
    if close_current_task_bridge_blocked {
        return Ok(None);
    }
    let late_stage_missing_task_closure_baseline_bridge =
        status.current_branch_closure_id.is_none()
            && context.steps.iter().all(|step| step.checked)
            && (status.review_state_status == "missing_current_closure"
                || status.phase_detail == "execution_reentry_required")
            && late_stage_missing_task_closure_baseline_bridge_supported(
                &branch_closure_rerecording_assessment(context)?,
            );
    if late_stage_missing_task_closure_baseline_bridge
        && !authoritative_task_closure_baseline_truth_present(&authoritative_state, task)
    {
        return Ok(None);
    }
    if authoritative_state
        .task_closure_negative_result(task)
        .is_some_and(|negative_result| {
            task_closure_negative_result_blocks_current_reviewed_state(
                negative_result
                    .semantic_reviewed_state_id
                    .as_deref()
                    .unwrap_or(negative_result.reviewed_state_id.as_str()),
                current_reviewed_state_id.as_deref(),
            )
        })
    {
        return Ok(None);
    }
    if status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
                | "late_stage_surface_not_declared"
        )
    }) {
        return Ok(None);
    }
    if status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "prior_task_review_not_green" | "prior_task_current_closure_stale"
        )
    }) && !(status.review_state_status == "stale_unreviewed" && stale_bridge_allowed)
    {
        return Ok(None);
    }
    if status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "task_cycle_break_active")
        && !task_cycle_break_reason_targets_repaired_task(context, status, task)?
    {
        return Ok(None);
    }
    if status.review_state_status == "stale_unreviewed" && !stale_bridge_allowed {
        let replay_required = status.reason_codes.iter().any(|reason_code| {
            matches!(
                reason_code.as_str(),
                "prior_task_review_not_green" | "prior_task_current_closure_stale"
            )
        }) || status.active_task == Some(task)
            || status.resume_task == Some(task)
            || status.blocking_step.is_some();
        if replay_required {
            return Ok(None);
        }
    }
    let closure_repair_phase_supported = match status.harness_phase {
        HarnessPhase::Executing | HarnessPhase::ExecutionPreflight => true,
        HarnessPhase::DocumentReleasePending if status.current_branch_closure_id.is_none() => {
            late_stage_missing_task_closure_baseline_bridge_supported(
                &branch_closure_rerecording_assessment(context)?,
            )
        }
        _ => false,
    };
    if !closure_repair_phase_supported {
        return Ok(None);
    }
    if status.current_branch_closure_id.is_none()
        && task_closures_are_non_branch_contributing(status)
    {
        return Ok(None);
    }
    Ok(Some(TaskClosureBaselineRepairCandidate {
        task,
        dispatch_id,
    }))
}

pub(crate) fn task_closure_baseline_bridge_ready_for_stale_target(
    context: &ExecutionContext,
    status: &PlanExecutionStatus,
    task: u32,
    earliest_unresolved_stale_task: Option<u32>,
) -> Result<bool, JsonFailure> {
    if earliest_unresolved_stale_task.is_some_and(|earliest_task| earliest_task != task) {
        return Ok(false);
    }
    if task_scope_structural_review_state_reason(status).is_some() {
        return Ok(false);
    }
    if status.reason_codes.iter().any(|reason_code| {
        matches!(
            reason_code.as_str(),
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
                | "late_stage_surface_not_declared"
                | "prior_task_review_not_green"
        )
    }) {
        return Ok(false);
    }
    if task_closure_baseline_repair_candidate_with_stale_target(
        context,
        status,
        task,
        earliest_unresolved_stale_task,
    )?
    .is_none()
    {
        return Ok(false);
    }
    if stale_unreviewed_allows_task_closure_baseline_bridge_with_stale_target(
        context,
        status,
        task,
        earliest_unresolved_stale_task,
    )? {
        return Ok(true);
    }
    let cycle_break_targets_task = status
        .reason_codes
        .iter()
        .any(|reason_code| reason_code == "task_cycle_break_active")
        && task_cycle_break_reason_targets_repaired_task(context, status, task)?;
    if cycle_break_targets_task {
        return task_closure_recording_runtime_truth_ready(context, task);
    }
    Ok(false)
}

pub(crate) fn task_closure_recording_runtime_truth_ready(
    context: &ExecutionContext,
    task: u32,
) -> Result<bool, JsonFailure> {
    Ok(
        authoritative_strategy_checkpoint_fingerprint_checked(context)?.is_some()
            && task_completion_lineage_fingerprint(context, task).is_some()
            && context
                .current_tracked_tree_sha()
                .ok()
                .is_some_and(|tree_sha| !tree_sha.trim().is_empty()),
    )
}

fn authoritative_task_closure_baseline_truth_present(
    authoritative_state: &AuthoritativeTransitionState,
    task: u32,
) -> bool {
    authoritative_state
        .raw_current_task_closure_state_entry(task)
        .is_some()
        || authoritative_state
            .current_task_closure_result(task)
            .is_some()
        || authoritative_state.task_closure_history_contains_task(task)
}

pub(crate) fn task_closures_are_non_branch_contributing(status: &PlanExecutionStatus) -> bool {
    !status.current_task_closures.is_empty()
        && status.current_task_closures.iter().all(|closure| {
            !closure.effective_reviewed_surface_paths.is_empty()
                && closure
                    .effective_reviewed_surface_paths
                    .iter()
                    .all(|path| path == NO_REPO_FILES_MARKER)
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskCurrentClosureStatus {
    Missing,
    Current,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrentTaskClosureStructuralFailure {
    pub(crate) task: Option<u32>,
    pub(crate) scope_key: String,
    pub(crate) closure_record_id: Option<String>,
    pub(crate) reason_code: String,
    pub(crate) message: String,
}

fn invalid_current_task_closure_error_for_raw_entry(
    entry: &crate::execution::transitions::RawCurrentTaskClosureStateEntry,
) -> JsonFailure {
    match entry.task {
        Some(task_number) => task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Task {task_number} current task closure is malformed or missing authoritative provenance for the active approved plan."
            ),
        ),
        None => task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Current task-closure entry `{}` is malformed or not bound to a valid task for the active approved plan.",
                entry.scope_key
            ),
        ),
    }
}

fn current_task_closure_structural_failure_from_entry(
    context: &ExecutionContext,
    entry: crate::execution::transitions::RawCurrentTaskClosureStateEntry,
) -> Option<CurrentTaskClosureStructuralFailure> {
    let error = match entry.record.as_ref() {
        Some(record) => validate_current_task_closure_record(context, record).err()?,
        None => invalid_current_task_closure_error_for_raw_entry(&entry),
    };
    Some(CurrentTaskClosureStructuralFailure {
        task: entry.task,
        scope_key: entry.scope_key,
        closure_record_id: entry.closure_record_id,
        reason_code: task_boundary_reason_code_from_message(&error.message)
            .unwrap_or("prior_task_current_closure_invalid")
            .to_owned(),
        message: error.message,
    })
}

fn current_task_closure_structural_failure_from_record(
    context: &ExecutionContext,
    record: crate::execution::transitions::CurrentTaskClosureRecord,
) -> Option<CurrentTaskClosureStructuralFailure> {
    let error = validate_current_task_closure_record(context, &record).err()?;
    Some(CurrentTaskClosureStructuralFailure {
        task: Some(record.task),
        scope_key: format!("task-{}", record.task),
        closure_record_id: Some(record.closure_record_id),
        reason_code: task_boundary_reason_code_from_message(&error.message)
            .unwrap_or("prior_task_current_closure_invalid")
            .to_owned(),
        message: error.message,
    })
}

pub(crate) fn structural_current_task_closure_failures(
    context: &ExecutionContext,
) -> Result<Vec<CurrentTaskClosureStructuralFailure>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(Vec::new());
    };
    let recoverable_current_records = authoritative_state.current_task_closure_results();
    let mut failures = authoritative_state
        .raw_current_task_closure_state_entries()
        .into_iter()
        .filter_map(|entry| match entry.task {
            Some(task_number)
                if recoverable_current_records
                    .get(&task_number)
                    .is_some_and(|record| {
                        validate_current_task_closure_record(context, record).is_ok()
                    }) =>
            {
                None
            }
            _ => current_task_closure_structural_failure_from_entry(context, entry),
        })
        .collect::<Vec<_>>();
    let structurally_invalid_tasks = failures
        .iter()
        .filter_map(|failure| failure.task)
        .collect::<BTreeSet<_>>();
    failures.extend(
        recoverable_current_records
            .into_values()
            .filter(|record| !structurally_invalid_tasks.contains(&record.task))
            .filter_map(|record| {
                current_task_closure_structural_failure_from_record(context, record)
            }),
    );
    Ok(failures)
}

pub(crate) fn valid_current_task_closure_records(
    context: &ExecutionContext,
) -> Result<Vec<crate::execution::transitions::CurrentTaskClosureRecord>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(Vec::new());
    };
    Ok(authoritative_state
        .current_task_closure_results()
        .into_values()
        .filter(|record| validate_current_task_closure_record(context, record).is_ok())
        .collect())
}

pub(crate) fn still_current_task_closure_records(
    context: &ExecutionContext,
) -> Result<Vec<crate::execution::transitions::CurrentTaskClosureRecord>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(Vec::new());
    };
    still_current_task_closure_records_from_authoritative_state(context, authoritative_state)
}

fn still_current_task_closure_records_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: &AuthoritativeTransitionState,
) -> Result<Vec<crate::execution::transitions::CurrentTaskClosureRecord>, JsonFailure> {
    let mut records = Vec::new();
    for record in authoritative_state
        .current_task_closure_results()
        .into_values()
    {
        if validate_current_task_closure_record(context, &record).is_err() {
            continue;
        }
        if task_closure_matches_current_workspace(context, &record)? {
            records.push(record);
        }
    }
    Ok(records)
}

fn task_current_closure_status(
    context: &ExecutionContext,
    task_number: u32,
) -> Result<TaskCurrentClosureStatus, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    let Some(authoritative_state) = authoritative_state.as_ref() else {
        return Ok(TaskCurrentClosureStatus::Missing);
    };
    task_current_closure_status_from_authoritative_state(context, task_number, authoritative_state)
}

fn task_current_closure_status_from_authoritative_state(
    context: &ExecutionContext,
    task_number: u32,
    authoritative_state: &AuthoritativeTransitionState,
) -> Result<TaskCurrentClosureStatus, JsonFailure> {
    let Some(current_closure) = authoritative_state.current_task_closure_result(task_number) else {
        if let Some(entry) = authoritative_state.raw_current_task_closure_state_entry(task_number) {
            return Err(invalid_current_task_closure_error_for_raw_entry(&entry));
        }
        return Ok(TaskCurrentClosureStatus::Missing);
    };
    validate_current_task_closure_record(context, &current_closure)?;
    if task_closure_matches_current_workspace(context, &current_closure)? {
        Ok(TaskCurrentClosureStatus::Current)
    } else {
        Ok(TaskCurrentClosureStatus::Stale)
    }
}

fn current_task_closure_overlay_restore_required(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    Ok(load_authoritative_transition_state(context)?
        .as_ref()
        .is_some_and(|state| state.current_task_closure_overlay_needs_restore()))
}

pub(crate) fn validate_current_task_closure_record(
    context: &ExecutionContext,
    closure: &crate::execution::transitions::CurrentTaskClosureRecord,
) -> Result<(), JsonFailure> {
    if closure.source_plan_path.as_deref() != Some(context.plan_rel.as_str())
        || closure.source_plan_revision != Some(context.plan_document.plan_revision)
    {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Task {} current task closure is not bound to the active approved plan revision.",
                closure.task
            ),
        ));
    }
    if closure.review_result != "pass" || closure.verification_result != "pass" {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Task {} current task closure is not a passing reviewed closure for the active approved plan.",
                closure.task
            ),
        ));
    }
    if closure.contract_identity.trim().is_empty() {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Task {} current task closure is missing contract identity provenance for the active approved plan.",
                closure.task
            ),
        ));
    }
    if !task_contract_identity_matches_expected(context, closure.task, &closure.contract_identity)?
    {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Task {} current task closure is not bound to the active task contract for the approved plan.",
                closure.task
            ),
        ));
    }
    if closure
        .execution_run_id
        .as_deref()
        .map(str::trim)
        .is_none_or(str::is_empty)
    {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Task {} current task closure is missing execution-run provenance for the active approved plan.",
                closure.task
            ),
        ));
    }
    if closure
        .closure_status
        .as_deref()
        .is_some_and(|status| status != "current")
    {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Task {} current task closure is not current for the active approved plan.",
                closure.task
            ),
        ));
    }
    if closure.effective_reviewed_surface_paths.is_empty() {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Task {} current task closure is missing authoritative reviewed-surface provenance for the active approved plan.",
                closure.task
            ),
        ));
    }
    if closure
        .effective_reviewed_surface_paths
        .iter()
        .any(|path| path == NO_REPO_FILES_MARKER)
        && closure.effective_reviewed_surface_paths.len() != 1
    {
        return Err(task_boundary_error(
            FailureClass::ExecutionStateNotReady,
            "prior_task_current_closure_invalid",
            format!(
                "Task {} current task closure mixes the no-repo-files sentinel with tracked reviewed-surface paths.",
                closure.task
            ),
        ));
    }
    cached_task_closure_reviewed_tree_sha(context, closure)?;
    Ok(())
}

fn task_closure_matches_current_workspace(
    context: &ExecutionContext,
    closure: &crate::execution::transitions::CurrentTaskClosureRecord,
) -> Result<bool, JsonFailure> {
    let surface_paths = closure
        .effective_reviewed_surface_paths
        .iter()
        .filter(|path| {
            path.as_str() != NO_REPO_FILES_MARKER
                && !is_runtime_owned_execution_control_plane_path(context, path)
        })
        .cloned()
        .collect::<Vec<_>>();
    if surface_paths.is_empty() {
        return Ok(true);
    }
    let reviewed_tree_sha = cached_task_closure_reviewed_tree_sha(context, closure)?;
    let current_tree_sha = context.current_tracked_tree_sha()?;
    let changed_paths =
        semantic_paths_changed_between_raw_trees(context, &reviewed_tree_sha, &current_tree_sha)
            .map_err(|error| {
                task_boundary_error(
                    FailureClass::BranchDetectionFailed,
                    "prior_task_current_closure_stale",
                    format!(
                        "Task {} current task closure freshness could not be validated: {}",
                        closure.task, error.message
                    ),
                )
            })?;
    let late_stage_surface =
        normalized_late_stage_surface(&context.plan_source).unwrap_or_default();
    if !late_stage_surface.is_empty()
        && changed_paths
            .iter()
            .all(|path| path_matches_late_stage_surface(path, &late_stage_surface))
    {
        return Ok(true);
    }
    Ok(changed_paths
        .into_iter()
        .all(|path| !path_matches_late_stage_surface(&path, &surface_paths)))
}

fn cached_task_closure_reviewed_tree_sha(
    context: &ExecutionContext,
    closure: &crate::execution::transitions::CurrentTaskClosureRecord,
) -> Result<String, JsonFailure> {
    context.cached_reviewed_tree_sha(
        &closure.reviewed_state_id,
        |repo_root, reviewed_state_id| {
            resolve_task_closure_reviewed_tree_sha(repo_root, closure.task, reviewed_state_id)
        },
    )
}

fn resolve_canonical_reviewed_tree_sha(
    repo_root: &Path,
    reviewed_state_id: &str,
    malformed_error: impl Fn(String) -> JsonFailure,
    unresolved_error: impl Fn(String) -> JsonFailure,
) -> Result<String, JsonFailure> {
    static CANONICAL_REVIEWED_TREE_SHA_CACHE: OnceLock<Mutex<BTreeMap<String, String>>> =
        OnceLock::new();

    let reviewed_state_id = reviewed_state_id.trim();
    let cache_key = format!("{}::{}", repo_root.display(), reviewed_state_id);
    if let Some(cached) = CANONICAL_REVIEWED_TREE_SHA_CACHE
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .expect("canonical reviewed tree cache lock should not be poisoned")
        .get(&cache_key)
        .cloned()
    {
        return Ok(cached);
    }
    let Some(tree_sha) = reviewed_state_id
        .strip_prefix("git_tree:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(malformed_error(format!(
            "reviewed_state_id must use canonical git_tree identity, got `{reviewed_state_id}`."
        )));
    };
    let object_id = gix::hash::ObjectId::from_hex(tree_sha.as_bytes()).map_err(|error| {
        malformed_error(format!(
            "reviewed_state_id must use a canonical git_tree object id, got `{reviewed_state_id}`: {error}"
        ))
    })?;
    if object_id.to_string() != tree_sha {
        return Err(malformed_error(format!(
            "reviewed_state_id must name the canonical tree object id directly, got `{reviewed_state_id}`."
        )));
    }
    let repo =
        discover_repository(repo_root).map_err(|error| unresolved_error(error.to_string()))?;
    let object = repo
        .find_object(object_id)
        .map_err(|error| unresolved_error(error.to_string()))?;
    if object.kind != gix::object::Kind::Tree {
        return Err(malformed_error(format!(
            "reviewed_state_id must resolve to a tree object directly, got `{}` for `{reviewed_state_id}`.",
            object.kind
        )));
    }
    let resolved_tree_sha = object.id.to_string();
    if !resolved_tree_sha.is_empty() {
        CANONICAL_REVIEWED_TREE_SHA_CACHE
            .get_or_init(|| Mutex::new(BTreeMap::new()))
            .lock()
            .expect("canonical reviewed tree cache lock should not be poisoned")
            .insert(cache_key, resolved_tree_sha.clone());
        return Ok(resolved_tree_sha);
    }
    Err(malformed_error(format!(
        "reviewed_state_id must resolve to a git_tree identity, got `{reviewed_state_id}`."
    )))
}

pub(crate) fn resolve_task_closure_reviewed_tree_sha(
    repo_root: &Path,
    task_number: u32,
    reviewed_state_id: &str,
) -> Result<String, JsonFailure> {
    resolve_canonical_reviewed_tree_sha(
        repo_root,
        reviewed_state_id,
        |detail| {
            task_boundary_error(
                FailureClass::MalformedExecutionState,
                "prior_task_current_closure_reviewed_state_malformed",
                format!("Task {task_number} current task closure {detail}"),
            )
        },
        |detail| {
            task_boundary_error(
                FailureClass::MalformedExecutionState,
                "prior_task_current_closure_reviewed_state_malformed",
                format!(
                    "Task {task_number} current task closure reviewed_state_id could not be resolved: {detail}"
                ),
            )
        },
    )
}

pub(crate) fn resolve_branch_closure_reviewed_tree_sha(
    repo_root: &Path,
    branch_closure_id: &str,
    reviewed_state_id: &str,
) -> Result<String, JsonFailure> {
    resolve_canonical_reviewed_tree_sha(
        repo_root,
        reviewed_state_id,
        |detail| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "current_branch_closure_reviewed_state_malformed: Branch closure {branch_closure_id} {detail}"
                ),
            )
        },
        |detail| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "current_branch_closure_reviewed_state_malformed: Branch closure {branch_closure_id} reviewed_state_id could not be resolved: {detail}"
                ),
            )
        },
    )
}

fn ensure_prior_task_current_closure_record(
    context: &ExecutionContext,
    prior_task: u32,
    target_task: u32,
) -> Result<(), JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?.ok_or_else(|| {
        task_boundary_error(
            FailureClass::MalformedExecutionState,
            "prior_task_current_closure_missing",
            format!(
                "Task {target_task} may not begin because Task {prior_task} current task closure state is unavailable."
            ),
        )
    })?;
    let current_record = authoritative_state
        .current_task_closure_result(prior_task)
        .ok_or_else(|| {
            task_boundary_error(
                FailureClass::ExecutionStateNotReady,
                "prior_task_current_closure_missing",
                format!(
                    "Task {target_task} may not begin because Task {prior_task} does not yet have a current task closure. Run `featureforge workflow operator --plan {} --external-review-result-ready`, then follow the recommended `close-current-task` command before starting Task {target_task}.",
                    context.plan_rel
                ),
            )
        })?;
    validate_current_task_closure_record(context, &current_record)?;
    Ok(())
}

fn prior_task_cycle_break_active(
    context: &ExecutionContext,
    prior_task: u32,
) -> Result<bool, JsonFailure> {
    let Some(overlay) = load_status_authoritative_overlay_checked(context)? else {
        return Ok(false);
    };
    let strategy_state = overlay
        .strategy_state
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    let strategy_checkpoint_kind = overlay
        .strategy_checkpoint_kind
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if strategy_state != "cycle_breaking" && strategy_checkpoint_kind != "cycle_break" {
        return Ok(false);
    }
    let Some(cycle_break_task) = overlay.strategy_cycle_break_task else {
        return Ok(false);
    };
    Ok(cycle_break_task == prior_task)
}

fn current_execution_run_id(context: &ExecutionContext) -> Result<Option<String>, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state(context)?;
    current_execution_run_id_with_authority(context, authoritative_state.as_ref())
}

fn current_execution_run_id_with_authority(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Result<Option<String>, JsonFailure> {
    if let Some(execution_run_id) = authoritative_execution_run_id_from_state(authoritative_state) {
        return Ok(Some(execution_run_id));
    }
    fallback_preflight_execution_run_id(context)
}

fn authoritative_execution_run_id_from_state(
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<String> {
    authoritative_state.and_then(AuthoritativeTransitionState::execution_run_id_opt)
}

fn fallback_preflight_execution_run_id(
    context: &ExecutionContext,
) -> Result<Option<String>, JsonFailure> {
    Ok(preflight_acceptance_for_context(context)?
        .map(|acceptance| acceptance.execution_run_id.as_str().to_owned()))
}

fn authoritative_unit_review_receipt_path(
    context: &ExecutionContext,
    execution_run_id: &str,
    task_number: u32,
    step_number: u32,
) -> PathBuf {
    harness_authoritative_artifact_path(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
        &format!("unit-review-{execution_run_id}-task-{task_number}-step-{step_number}.md"),
    )
}

fn authoritative_task_verification_receipt_path(
    context: &ExecutionContext,
    execution_run_id: &str,
    task_number: u32,
) -> PathBuf {
    harness_authoritative_artifact_path(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
        &format!("task-verification-{execution_run_id}-task-{task_number}.md"),
    )
}

fn task_boundary_error(
    failure_class: FailureClass,
    reason_code: &str,
    message: impl Into<String>,
) -> JsonFailure {
    JsonFailure::new(failure_class, format!("{reason_code}: {}", message.into()))
}

fn task_boundary_reason_code_from_message(message: &str) -> Option<&str> {
    let (candidate, _) = message.split_once(':')?;
    let candidate = candidate.trim();
    if candidate.is_empty() {
        return None;
    }
    if candidate
        .as_bytes()
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        Some(candidate)
    } else {
        None
    }
}

fn latest_attempt_for_step(
    evidence: &ExecutionEvidence,
    task_number: u32,
    step_number: u32,
) -> Option<&EvidenceAttempt> {
    evidence
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.task_number == task_number && attempt.step_number == step_number)
}

pub(crate) fn latest_attempted_step_for_task(
    context: &ExecutionContext,
    task_number: u32,
) -> Option<u32> {
    context.evidence.attempts.iter().rev().find_map(|attempt| {
        (attempt.task_number == task_number
            && context.steps.iter().any(|step| {
                step.task_number == task_number && step.step_number == attempt.step_number
            }))
        .then_some(attempt.step_number)
    })
}

pub(crate) fn task_completion_lineage_fingerprint(
    context: &ExecutionContext,
    task_number: u32,
) -> Option<String> {
    let task_steps = context
        .steps
        .iter()
        .filter(|step| step.task_number == task_number)
        .collect::<Vec<_>>();
    if task_steps.is_empty() {
        return None;
    }

    let mut payload = format!(
        "plan={}\nplan_revision={}\ntask={task_number}\n",
        context.plan_rel, context.plan_document.plan_revision
    );
    for step in task_steps {
        if !step.checked {
            return None;
        }
        let attempt = latest_attempt_for_step(&context.evidence, task_number, step.step_number)?;
        if attempt.status != "Completed" {
            return None;
        }
        let packet_fingerprint = attempt
            .packet_fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let checkpoint_sha = attempt
            .head_sha
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let recorded_at = attempt.recorded_at.trim();
        if recorded_at.is_empty() {
            return None;
        }
        payload.push_str(&format!(
            "step={}:attempt={}:recorded_at={recorded_at}:packet={packet_fingerprint}:checkpoint={checkpoint_sha}\n",
            step.step_number, attempt.attempt_number
        ));
    }
    Some(sha256_hex(payload.as_bytes()))
}

fn latest_attempt_indices_by_step(evidence: &ExecutionEvidence) -> BTreeMap<(u32, u32), usize> {
    let mut indices = BTreeMap::new();
    for (index, attempt) in evidence.attempts.iter().enumerate() {
        indices.insert((attempt.task_number, attempt.step_number), index);
    }
    indices
}

fn latest_completed_attempts_by_step(evidence: &ExecutionEvidence) -> BTreeMap<(u32, u32), usize> {
    let mut indices = BTreeMap::new();
    for (index, attempt) in evidence.attempts.iter().enumerate() {
        if attempt.status == "Completed" {
            indices.insert((attempt.task_number, attempt.step_number), index);
        }
    }
    indices
}

fn latest_completed_attempts_by_file(
    evidence: &ExecutionEvidence,
    latest_attempts_by_step: &BTreeMap<(u32, u32), usize>,
) -> BTreeMap<String, usize> {
    let mut latest_attempts_by_file = BTreeMap::new();
    for index in latest_attempts_by_step.values().copied() {
        let attempt = &evidence.attempts[index];
        for proof in &attempt.file_proofs {
            if proof.path == NO_REPO_FILES_MARKER {
                continue;
            }
            latest_attempts_by_file.insert(proof.path.clone(), index);
        }
    }
    latest_attempts_by_file
}

fn execution_started(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> bool {
    authoritative_state.map_or_else(
        || {
            context
                .steps
                .iter()
                .any(|step| step.checked || step.note_state.is_some())
                || !context.evidence.attempts.is_empty()
        },
        AuthoritativeTransitionState::has_authoritative_execution_progress,
    )
}

fn active_step(context: &ExecutionContext, note_state: NoteState) -> Option<&PlanStepState> {
    context
        .steps
        .iter()
        .find(|step| step.note_state == Some(note_state))
}
