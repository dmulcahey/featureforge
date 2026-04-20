use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::execution::harness::{ChunkId, ExecutionRunId};

/// Runtime constant.
pub const DEPENDENCY_INDEX_VERSION: u32 = 1;
/// Runtime constant.
pub const DEFAULT_RETENTION_WINDOW_DAYS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
/// Runtime enum.
pub enum DependencyIndexState {
    /// Runtime enum variant.
    Healthy,
    #[default]
    /// Runtime enum variant.
    Missing,
    /// Runtime enum variant.
    Malformed,
    /// Runtime enum variant.
    Inconsistent,
    /// Runtime enum variant.
    Recovering,
}

impl DependencyIndexState {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Missing => "missing",
            Self::Malformed => "malformed",
            Self::Inconsistent => "inconsistent",
            Self::Recovering => "recovering",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct DependencyIndexIssue {
    /// Runtime field.
    pub code: String,
    /// Runtime field.
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct DependencyIndexHealth {
    /// Runtime field.
    pub state: DependencyIndexState,
    /// Runtime field.
    pub issues: Vec<DependencyIndexIssue>,
    /// Runtime field.
    pub requires_fail_closed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct DependencyNodeId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Runtime enum.
pub enum IndexedArtifactKind {
    /// Runtime enum variant.
    Contract,
    /// Runtime enum variant.
    EvaluationReport,
    /// Runtime enum variant.
    Handoff,
    /// Runtime enum variant.
    EvidenceArtifact,
    /// Runtime enum variant.
    FinalReviewArtifact,
    /// Runtime enum variant.
    BrowserQaArtifact,
    /// Runtime enum variant.
    ReleaseDocsArtifact,
    /// Runtime enum variant.
    CandidateContract,
    /// Runtime enum variant.
    CandidateEvaluationReport,
    /// Runtime enum variant.
    CandidateHandoff,
}

impl IndexedArtifactKind {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contract => "contract",
            Self::EvaluationReport => "evaluation_report",
            Self::Handoff => "handoff",
            Self::EvidenceArtifact => "evidence_artifact",
            Self::FinalReviewArtifact => "final_review_artifact",
            Self::BrowserQaArtifact => "browser_qa_artifact",
            Self::ReleaseDocsArtifact => "release_docs_artifact",
            Self::CandidateContract => "candidate_contract",
            Self::CandidateEvaluationReport => "candidate_evaluation_report",
            Self::CandidateHandoff => "candidate_handoff",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct DependencyNode {
    /// Runtime field.
    pub node_id: DependencyNodeId,
    /// Runtime field.
    pub artifact_kind: IndexedArtifactKind,
    /// Runtime field.
    pub artifact_fingerprint: String,
    /// Runtime field.
    pub authoritative: bool,
    /// Runtime field.
    pub execution_run_id: Option<ExecutionRunId>,
    /// Runtime field.
    pub chunk_id: Option<ChunkId>,
    /// Runtime field.
    pub authoritative_sequence: Option<u64>,
    /// Runtime field.
    pub source_plan_path: Option<String>,
    /// Runtime field.
    pub source_plan_revision: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Runtime enum.
pub enum DependencyEdgeKind {
    /// Runtime enum variant.
    DependsOn,
    /// Runtime enum variant.
    Supersedes,
    /// Runtime enum variant.
    Invalidates,
    /// Runtime enum variant.
    RequiredByGate,
    /// Runtime enum variant.
    CandidateRetentionClaim,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct DependencyEdge {
    /// Runtime field.
    pub from: DependencyNodeId,
    /// Runtime field.
    pub to: DependencyNodeId,
    /// Runtime field.
    pub kind: DependencyEdgeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct CandidateArtifactDependencyClaim {
    /// Runtime field.
    pub claim_id: String,
    /// Runtime field.
    pub artifact_fingerprint: String,
    /// Runtime field.
    pub artifact_kind: IndexedArtifactKind,
    /// Runtime field.
    pub execution_run_id: Option<ExecutionRunId>,
    /// Runtime field.
    pub chunk_id: Option<ChunkId>,
    /// Runtime field.
    pub controller_id: String,
    /// Runtime field.
    pub reason: String,
    /// Runtime field.
    pub created_at: String,
    /// Runtime field.
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct DependencyIndex {
    /// Runtime field.
    pub version: u32,
    /// Runtime field.
    pub state: DependencyIndexState,
    /// Runtime field.
    pub health: DependencyIndexHealth,
    /// Runtime field.
    pub nodes: Vec<DependencyNode>,
    /// Runtime field.
    pub edges: Vec<DependencyEdge>,
    /// Runtime field.
    pub candidate_claims: Vec<CandidateArtifactDependencyClaim>,
}

impl DependencyIndex {
    #[must_use]
    /// Runtime constant.
    pub const fn healthy_empty() -> Self {
        Self {
            version: DEPENDENCY_INDEX_VERSION,
            state: DependencyIndexState::Healthy,
            health: DependencyIndexHealth::healthy(),
            nodes: Vec::new(),
            edges: Vec::new(),
            candidate_claims: Vec::new(),
        }
    }
}

impl DependencyIndexHealth {
    #[must_use]
    /// Runtime constant.
    pub const fn healthy() -> Self {
        Self {
            state: DependencyIndexState::Healthy,
            issues: Vec::new(),
            requires_fail_closed: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct RetentionWindow {
    /// Runtime field.
    pub max_age_days: u32,
}

impl Default for RetentionWindow {
    fn default() -> Self {
        Self {
            max_age_days: DEFAULT_RETENTION_WINDOW_DAYS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct RetentionEligibility {
    /// Runtime field.
    pub artifact_fingerprint: String,
    /// Runtime field.
    pub retain: bool,
    /// Runtime field.
    pub reasons: Vec<String>,
}

impl RetentionEligibility {
    /// Runtime function.
    pub fn retain(artifact_fingerprint: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            artifact_fingerprint: artifact_fingerprint.into(),
            retain: true,
            reasons: vec![reason.into()],
        }
    }

    /// Runtime function.
    pub fn prune(artifact_fingerprint: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            artifact_fingerprint: artifact_fingerprint.into(),
            retain: false,
            reasons: vec![reason.into()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct RetentionEligibilityReport {
    /// Runtime field.
    pub window: RetentionWindow,
    /// Runtime field.
    pub dependency_index_state: DependencyIndexState,
    /// Runtime field.
    pub decisions: Vec<RetentionEligibility>,
    /// Runtime field.
    pub skipped: bool,
    /// Runtime field.
    pub skip_reason: Option<String>,
}
