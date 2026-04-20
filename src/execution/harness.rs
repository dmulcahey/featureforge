use std::error::Error;
use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Runtime constant.
pub const INITIAL_AUTHORITATIVE_SEQUENCE: u64 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Runtime enum.
pub enum HarnessPhase {
    /// Runtime enum variant.
    ImplementationHandoff,
    /// Runtime enum variant.
    ExecutionPreflight,
    /// Runtime enum variant.
    ContractDrafting,
    /// Runtime enum variant.
    ContractPendingApproval,
    /// Runtime enum variant.
    ContractApproved,
    /// Runtime enum variant.
    Executing,
    /// Runtime enum variant.
    Evaluating,
    /// Runtime enum variant.
    Repairing,
    /// Runtime enum variant.
    PivotRequired,
    /// Runtime enum variant.
    HandoffRequired,
    /// Runtime enum variant.
    FinalReviewPending,
    /// Runtime enum variant.
    QaPending,
    /// Runtime enum variant.
    DocumentReleasePending,
    /// Runtime enum variant.
    ReadyForBranchCompletion,
}

impl HarnessPhase {
    /// Runtime constant.
    pub const ALL: [Self; 14] = [
        Self::ImplementationHandoff,
        Self::ExecutionPreflight,
        Self::ContractDrafting,
        Self::ContractPendingApproval,
        Self::ContractApproved,
        Self::Executing,
        Self::Evaluating,
        Self::Repairing,
        Self::PivotRequired,
        Self::HandoffRequired,
        Self::FinalReviewPending,
        Self::QaPending,
        Self::DocumentReleasePending,
        Self::ReadyForBranchCompletion,
    ];

    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ImplementationHandoff => "implementation_handoff",
            Self::ExecutionPreflight => "execution_preflight",
            Self::ContractDrafting => "contract_drafting",
            Self::ContractPendingApproval => "contract_pending_approval",
            Self::ContractApproved => "contract_approved",
            Self::Executing => "executing",
            Self::Evaluating => "evaluating",
            Self::Repairing => "repairing",
            Self::PivotRequired => "pivot_required",
            Self::HandoffRequired => "handoff_required",
            Self::FinalReviewPending => "final_review_pending",
            Self::QaPending => "qa_pending",
            Self::DocumentReleasePending => "document_release_pending",
            Self::ReadyForBranchCompletion => "ready_for_branch_completion",
        }
    }

    #[must_use]
    /// Runtime function.
    pub fn is_public_phase(value: &str) -> bool {
        Self::ALL.iter().any(|phase| phase.as_str() == value)
    }
}

impl fmt::Display for HarnessPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for HarnessPhase {
    type Err = ParseHarnessPhaseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "implementation_handoff" => Ok(Self::ImplementationHandoff),
            "execution_preflight" => Ok(Self::ExecutionPreflight),
            "contract_drafting" => Ok(Self::ContractDrafting),
            "contract_pending_approval" => Ok(Self::ContractPendingApproval),
            "contract_approved" => Ok(Self::ContractApproved),
            "executing" => Ok(Self::Executing),
            "evaluating" => Ok(Self::Evaluating),
            "repairing" => Ok(Self::Repairing),
            "pivot_required" => Ok(Self::PivotRequired),
            "handoff_required" => Ok(Self::HandoffRequired),
            "final_review_pending" => Ok(Self::FinalReviewPending),
            "qa_pending" => Ok(Self::QaPending),
            "document_release_pending" => Ok(Self::DocumentReleasePending),
            "ready_for_branch_completion" => Ok(Self::ReadyForBranchCompletion),
            _ => Err(ParseHarnessPhaseError {
                invalid_value: value.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Runtime struct.
pub struct ParseHarnessPhaseError {
    /// Runtime field.
    pub invalid_value: String,
}

impl fmt::Display for ParseHarnessPhaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown harness phase value `{}`", self.invalid_value)
    }
}

impl Error for ParseHarnessPhaseError {}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct ExecutionRunId(pub String);

impl ExecutionRunId {
    /// Runtime function.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    /// Runtime function.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ExecutionRunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct ChunkId(pub String);

impl ChunkId {
    /// Runtime function.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    /// Runtime function.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ChunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime enum.
pub enum ChunkingStrategy {
    #[serde(rename = "task")]
    /// Runtime enum variant.
    Task,
    #[serde(rename = "task-group")]
    /// Runtime enum variant.
    TaskGroup,
    #[serde(rename = "whole-run")]
    /// Runtime enum variant.
    WholeRun,
}

impl ChunkingStrategy {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::TaskGroup => "task-group",
            Self::WholeRun => "whole-run",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime enum.
pub enum ResetPolicy {
    #[serde(rename = "none")]
    /// Runtime enum variant.
    None,
    #[serde(rename = "chunk-boundary")]
    /// Runtime enum variant.
    ChunkBoundary,
    #[serde(rename = "adaptive")]
    /// Runtime enum variant.
    Adaptive,
}

impl ResetPolicy {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ChunkBoundary => "chunk-boundary",
            Self::Adaptive => "adaptive",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct EvaluatorPolicyName(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct FrozenPolicySnapshot {
    /// Runtime field.
    pub chunking_strategy: ChunkingStrategy,
    /// Runtime field.
    pub evaluator_policy: EvaluatorPolicyName,
    /// Runtime field.
    pub reset_policy: ResetPolicy,
    /// Runtime field.
    pub review_stack: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct RunIdentitySnapshot {
    /// Runtime field.
    pub execution_run_id: ExecutionRunId,
    /// Runtime field.
    pub source_plan_path: String,
    /// Runtime field.
    pub source_plan_revision: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Runtime enum.
pub enum EvaluatorKind {
    /// Runtime enum variant.
    SpecCompliance,
    /// Runtime enum variant.
    CodeQuality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Runtime enum.
pub enum EvaluationVerdict {
    /// Runtime enum variant.
    Pass,
    /// Runtime enum variant.
    Fail,
    /// Runtime enum variant.
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Runtime enum.
pub enum AggregateEvaluationState {
    /// Runtime enum variant.
    Pass,
    /// Runtime enum variant.
    Pending,
    /// Runtime enum variant.
    Fail,
    /// Runtime enum variant.
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct AuthoritativeArtifactPointers {
    /// Runtime field.
    pub active_contract_path: Option<String>,
    /// Runtime field.
    pub active_contract_fingerprint: Option<String>,
    /// Runtime field.
    pub last_evaluation_report_path: Option<String>,
    /// Runtime field.
    pub last_evaluation_report_fingerprint: Option<String>,
    /// Runtime field.
    pub last_evaluation_evaluator_kind: Option<EvaluatorKind>,
    /// Runtime field.
    pub last_evaluation_verdict: Option<EvaluationVerdict>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct WorktreeLeaseBindingSnapshot {
    /// Runtime field.
    pub execution_run_id: String,
    /// Runtime field.
    pub lease_fingerprint: String,
    /// Runtime field.
    pub lease_artifact_path: String,
    #[serde(default)]
    /// Runtime field.
    pub execution_context_key: Option<String>,
    #[serde(default)]
    /// Runtime field.
    pub approved_task_packet_fingerprint: Option<String>,
    #[serde(default)]
    /// Runtime field.
    pub approved_unit_contract_fingerprint: Option<String>,
    #[serde(default)]
    /// Runtime field.
    pub reconcile_result_proof_fingerprint: Option<String>,
    #[serde(default)]
    /// Runtime field.
    pub reviewed_checkpoint_commit_sha: Option<String>,
    #[serde(default)]
    /// Runtime field.
    pub reconcile_result_commit_sha: Option<String>,
    #[serde(default)]
    /// Runtime field.
    pub reconcile_mode: Option<String>,
    #[serde(default)]
    /// Runtime field.
    pub review_receipt_fingerprint: Option<String>,
    #[serde(default)]
    /// Runtime field.
    pub review_receipt_artifact_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct EvaluatorSetSnapshot {
    /// Runtime field.
    pub required_evaluator_kinds: Vec<EvaluatorKind>,
    /// Runtime field.
    pub completed_evaluator_kinds: Vec<EvaluatorKind>,
    /// Runtime field.
    pub pending_evaluator_kinds: Vec<EvaluatorKind>,
    /// Runtime field.
    pub non_passing_evaluator_kinds: Vec<EvaluatorKind>,
    /// Runtime field.
    pub aggregate_evaluation_state: AggregateEvaluationState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct ChunkRetrySnapshot {
    /// Runtime field.
    pub current_chunk_retry_count: u32,
    /// Runtime field.
    pub current_chunk_retry_budget: u32,
    /// Runtime field.
    pub current_chunk_pivot_threshold: u32,
    /// Runtime field.
    pub handoff_required: bool,
    /// Runtime field.
    pub open_failed_criteria: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Runtime enum.
pub enum DownstreamFreshnessState {
    /// Runtime enum variant.
    NotRequired,
    /// Runtime enum variant.
    Missing,
    /// Runtime enum variant.
    Fresh,
    /// Runtime enum variant.
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct DownstreamFreshnessSnapshot {
    /// Runtime field.
    pub final_review_state: DownstreamFreshnessState,
    /// Runtime field.
    pub browser_qa_state: DownstreamFreshnessState,
    /// Runtime field.
    pub release_docs_state: DownstreamFreshnessState,
    /// Runtime field.
    pub last_final_review_artifact_fingerprint: Option<String>,
    /// Runtime field.
    pub last_browser_qa_artifact_fingerprint: Option<String>,
    /// Runtime field.
    pub last_release_docs_artifact_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct RepoStateSnapshot {
    /// Runtime field.
    pub repo_state_baseline_head_sha: Option<String>,
    /// Runtime field.
    pub repo_state_baseline_worktree_fingerprint: Option<String>,
    /// Runtime field.
    pub repo_state_drift_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct WriteAuthorityDiagnostics {
    /// Runtime field.
    pub write_authority_state: String,
    /// Runtime field.
    pub write_authority_holder: Option<String>,
    /// Runtime field.
    pub write_authority_worktree: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct AuthoritativeOrderingState {
    /// Runtime field.
    pub latest_authoritative_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct AuthoritativeHarnessState {
    /// Runtime field.
    pub harness_phase: HarnessPhase,
    /// Runtime field.
    pub chunk_id: ChunkId,
    /// Runtime field.
    pub run_identity: Option<RunIdentitySnapshot>,
    /// Runtime field.
    pub ordering: AuthoritativeOrderingState,
    #[serde(default)]
    /// Runtime field.
    pub active_worktree_lease_fingerprints: Option<Vec<String>>,
    #[serde(default)]
    /// Runtime field.
    pub active_worktree_lease_bindings: Option<Vec<WorktreeLeaseBindingSnapshot>>,
    /// Runtime field.
    pub policy_snapshot: Option<FrozenPolicySnapshot>,
    /// Runtime field.
    pub artifact_pointers: AuthoritativeArtifactPointers,
    /// Runtime field.
    pub evaluators: EvaluatorSetSnapshot,
    /// Runtime field.
    pub retry: ChunkRetrySnapshot,
    /// Runtime field.
    pub write_authority: WriteAuthorityDiagnostics,
    /// Runtime field.
    pub repo_state: RepoStateSnapshot,
    /// Runtime field.
    pub dependency_index_state: String,
    /// Runtime field.
    pub downstream_freshness: DownstreamFreshnessSnapshot,
    /// Runtime field.
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct LearnedTopologyGuidance {
    /// Runtime field.
    pub approved_plan_revision: u32,
    /// Runtime field.
    pub execution_context_key: String,
    /// Runtime field.
    pub primary_reason_class: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct TopologySelectionContext {
    /// Runtime field.
    pub execution_context_key: String,
    /// Runtime field.
    pub tasks_independent: bool,
    /// Runtime field.
    pub isolated_agents_available: String,
    /// Runtime field.
    pub session_intent: String,
    /// Runtime field.
    pub workspace_prepared: String,
    /// Runtime field.
    pub current_parallel_path_ready: bool,
    #[serde(default)]
    /// Runtime field.
    pub learned_guidance: Option<LearnedTopologyGuidance>,
}
