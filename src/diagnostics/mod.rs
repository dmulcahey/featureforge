use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    BranchDetectionFailed,
    InstructionParseFailed,
    InvalidRepoPath,
}

impl FailureClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BranchDetectionFailed => "BranchDetectionFailed",
            Self::InstructionParseFailed => "InstructionParseFailed",
            Self::InvalidRepoPath => "InvalidRepoPath",
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
}
