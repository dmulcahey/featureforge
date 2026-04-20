use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Runtime enum.
pub enum FailureClass {
    /// Runtime enum variant.
    ApprovalWriteFailed,
    /// Runtime enum variant.
    ArtifactIntegrityMismatch,
    /// Runtime enum variant.
    AuthoritativeOrderingMismatch,
    /// Runtime enum variant.
    BlockedOnPlanPivot,
    /// Runtime enum variant.
    BranchDetectionFailed,
    /// Runtime enum variant.
    ConcurrentWriterConflict,
    /// Runtime enum variant.
    ContractMismatch,
    /// Runtime enum variant.
    DecisionReadFailed,
    /// Runtime enum variant.
    DecisionWriteFailed,
    /// Runtime enum variant.
    DependencyIndexMismatch,
    /// Runtime enum variant.
    EvidenceWriteFailed,
    /// Runtime enum variant.
    EvaluationMismatch,
    /// Runtime enum variant.
    ExecutionStateNotReady,
    /// Runtime enum variant.
    IdempotencyConflict,
    /// Runtime enum variant.
    IllegalHarnessPhase,
    /// Runtime enum variant.
    InstructionParseFailed,
    /// Runtime enum variant.
    InvalidCommandInput,
    /// Runtime enum variant.
    InvalidConfigFormat,
    /// Runtime enum variant.
    InvalidExecutionMode,
    /// Runtime enum variant.
    InvalidRepoPath,
    /// Runtime enum variant.
    InvalidWriteTarget,
    /// Runtime enum variant.
    InvalidStepTransition,
    /// Runtime enum variant.
    MalformedExecutionState,
    /// Runtime enum variant.
    MissedReopenRequired,
    /// Runtime enum variant.
    MissingRequiredHandoff,
    /// Runtime enum variant.
    NonAuthoritativeArtifact,
    /// Runtime enum variant.
    NonHarnessProvenance,
    /// Runtime enum variant.
    PartialAuthoritativeMutation,
    /// Runtime enum variant.
    PlanNotExecutionReady,
    /// Runtime enum variant.
    PromptPayloadBuildFailed,
    /// Runtime enum variant.
    QaArtifactNotFresh,
    /// Runtime enum variant.
    RepoStateDrift,
    /// Runtime enum variant.
    ReviewArtifactNotFresh,
    /// Runtime enum variant.
    RecommendAfterExecutionStart,
    /// Runtime enum variant.
    RepoContextUnavailable,
    /// Runtime enum variant.
    ReleaseArtifactNotFresh,
    /// Runtime enum variant.
    ResolverContractViolation,
    /// Runtime enum variant.
    ResolverRuntimeFailure,
    /// Runtime enum variant.
    StaleProvenance,
    /// Runtime enum variant.
    StaleExecutionEvidence,
    /// Runtime enum variant.
    StaleMutation,
    /// Runtime enum variant.
    UnsupportedArtifactVersion,
    /// Runtime enum variant.
    UpdateCheckStateFailed,
    /// Runtime enum variant.
    WorkspaceNotSafe,
}

impl FailureClass {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ApprovalWriteFailed => "ApprovalWriteFailed",
            Self::ArtifactIntegrityMismatch => "ArtifactIntegrityMismatch",
            Self::AuthoritativeOrderingMismatch => "AuthoritativeOrderingMismatch",
            Self::BlockedOnPlanPivot => "BlockedOnPlanPivot",
            Self::BranchDetectionFailed => "BranchDetectionFailed",
            Self::ConcurrentWriterConflict => "ConcurrentWriterConflict",
            Self::ContractMismatch => "ContractMismatch",
            Self::DecisionReadFailed => "DecisionReadFailed",
            Self::DecisionWriteFailed => "DecisionWriteFailed",
            Self::DependencyIndexMismatch => "DependencyIndexMismatch",
            Self::EvidenceWriteFailed => "EvidenceWriteFailed",
            Self::EvaluationMismatch => "EvaluationMismatch",
            Self::ExecutionStateNotReady => "ExecutionStateNotReady",
            Self::IdempotencyConflict => "IdempotencyConflict",
            Self::IllegalHarnessPhase => "IllegalHarnessPhase",
            Self::InstructionParseFailed => "InstructionParseFailed",
            Self::InvalidCommandInput => "InvalidCommandInput",
            Self::InvalidConfigFormat => "InvalidConfigFormat",
            Self::InvalidExecutionMode => "InvalidExecutionMode",
            Self::InvalidRepoPath => "InvalidRepoPath",
            Self::InvalidWriteTarget => "InvalidWriteTarget",
            Self::InvalidStepTransition => "InvalidStepTransition",
            Self::MalformedExecutionState => "MalformedExecutionState",
            Self::MissedReopenRequired => "MissedReopenRequired",
            Self::MissingRequiredHandoff => "MissingRequiredHandoff",
            Self::NonAuthoritativeArtifact => "NonAuthoritativeArtifact",
            Self::NonHarnessProvenance => "NonHarnessProvenance",
            Self::PartialAuthoritativeMutation => "PartialAuthoritativeMutation",
            Self::PlanNotExecutionReady => "PlanNotExecutionReady",
            Self::PromptPayloadBuildFailed => "PromptPayloadBuildFailed",
            Self::QaArtifactNotFresh => "QaArtifactNotFresh",
            Self::RepoStateDrift => "RepoStateDrift",
            Self::ReviewArtifactNotFresh => "ReviewArtifactNotFresh",
            Self::RecommendAfterExecutionStart => "RecommendAfterExecutionStart",
            Self::RepoContextUnavailable => "RepoContextUnavailable",
            Self::ReleaseArtifactNotFresh => "ReleaseArtifactNotFresh",
            Self::ResolverContractViolation => "ResolverContractViolation",
            Self::ResolverRuntimeFailure => "ResolverRuntimeFailure",
            Self::StaleProvenance => "StaleProvenance",
            Self::StaleExecutionEvidence => "StaleExecutionEvidence",
            Self::StaleMutation => "StaleMutation",
            Self::UnsupportedArtifactVersion => "UnsupportedArtifactVersion",
            Self::UpdateCheckStateFailed => "UpdateCheckStateFailed",
            Self::WorkspaceNotSafe => "WorkspaceNotSafe",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
/// Runtime struct.
pub struct DiagnosticError {
    failure_class: FailureClass,
    message: String,
}

impl DiagnosticError {
    /// Runtime function.
    pub fn new(failure_class: FailureClass, message: impl Into<String>) -> Self {
        Self {
            failure_class,
            message: message.into(),
        }
    }

    #[must_use]
    /// Runtime constant.
    pub const fn failure_class_enum(&self) -> FailureClass {
        self.failure_class
    }

    #[must_use]
    /// Runtime constant.
    pub const fn failure_class(&self) -> &'static str {
        self.failure_class.as_str()
    }

    #[must_use]
    /// Runtime function.
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
/// Runtime struct.
pub struct JsonFailure {
    /// Runtime field.
    pub error_class: String,
    /// Runtime field.
    pub message: String,
}

impl JsonFailure {
    /// Runtime function.
    pub fn new(failure_class: FailureClass, message: impl Into<String>) -> Self {
        Self {
            error_class: failure_class.as_str().to_owned(),
            message: message.into(),
        }
    }
}

impl From<DiagnosticError> for JsonFailure {
    fn from(value: DiagnosticError) -> Self {
        Self {
            error_class: value.failure_class().to_owned(),
            message: value.message().to_owned(),
        }
    }
}
