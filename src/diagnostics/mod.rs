use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    BranchDetectionFailed,
    EvidenceWriteFailed,
    ExecutionStateNotReady,
    InstructionParseFailed,
    InvalidCommandInput,
    InvalidExecutionMode,
    InvalidRepoPath,
    InvalidStepTransition,
    MalformedExecutionState,
    MissedReopenRequired,
    PlanNotExecutionReady,
    QaArtifactNotFresh,
    RecommendAfterExecutionStart,
    ReleaseArtifactNotFresh,
    StaleExecutionEvidence,
    StaleMutation,
    WorkspaceNotSafe,
}

impl FailureClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BranchDetectionFailed => "BranchDetectionFailed",
            Self::EvidenceWriteFailed => "EvidenceWriteFailed",
            Self::ExecutionStateNotReady => "ExecutionStateNotReady",
            Self::InstructionParseFailed => "InstructionParseFailed",
            Self::InvalidCommandInput => "InvalidCommandInput",
            Self::InvalidExecutionMode => "InvalidExecutionMode",
            Self::InvalidRepoPath => "InvalidRepoPath",
            Self::InvalidStepTransition => "InvalidStepTransition",
            Self::MalformedExecutionState => "MalformedExecutionState",
            Self::MissedReopenRequired => "MissedReopenRequired",
            Self::PlanNotExecutionReady => "PlanNotExecutionReady",
            Self::QaArtifactNotFresh => "QaArtifactNotFresh",
            Self::RecommendAfterExecutionStart => "RecommendAfterExecutionStart",
            Self::ReleaseArtifactNotFresh => "ReleaseArtifactNotFresh",
            Self::StaleExecutionEvidence => "StaleExecutionEvidence",
            Self::StaleMutation => "StaleMutation",
            Self::WorkspaceNotSafe => "WorkspaceNotSafe",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct DiagnosticError {
    failure_class: FailureClass,
    message: String,
}

impl DiagnosticError {
    pub fn new(failure_class: FailureClass, message: impl Into<String>) -> Self {
        Self {
            failure_class,
            message: message.into(),
        }
    }

    pub const fn failure_class_enum(&self) -> FailureClass {
        self.failure_class
    }

    pub const fn failure_class(&self) -> &'static str {
        self.failure_class.as_str()
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JsonFailure {
    pub error_class: String,
    pub message: String,
}

impl JsonFailure {
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
