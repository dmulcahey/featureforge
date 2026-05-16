use crate::execution::follow_up::FollowUpKind;
use crate::execution::review_route_tokens::{
    FOLLOW_UP_ADVANCE_LATE_STAGE, FOLLOW_UP_REPAIR_REVIEW_STATE,
};

use super::execution_target::{
    execution_mutation_name_from_public_argv, execution_target_from_public_argv,
    execution_target_from_public_template_base_argv,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicCommandKind {
    WorkflowOperator,
    Status,
    RepairReviewState,
    CloseCurrentTask,
    AdvanceLateStage,
    Begin,
    Complete,
    Reopen,
    Transfer,
    MaterializeProjectionsStateDirOnly,
}

pub const PUBLIC_EXECUTION_COMMAND_KIND_VALUES: &[&str] = &["begin", "complete", "reopen"];
pub const PUBLIC_REPAIR_TARGET_COMMAND_KIND_VALUES: &[&str] = &[
    "begin",
    "complete",
    "reopen",
    "transfer",
    "close-current-task",
    "repair-review-state",
    "advance-late-stage",
];

const PUBLIC_MUTATION_COMMAND_KINDS: &[PublicCommandKind] = &[
    PublicCommandKind::Begin,
    PublicCommandKind::Complete,
    PublicCommandKind::Reopen,
    PublicCommandKind::Transfer,
    PublicCommandKind::CloseCurrentTask,
    PublicCommandKind::RepairReviewState,
    PublicCommandKind::AdvanceLateStage,
];

pub fn public_mutation_command_tokens() -> impl Iterator<Item = &'static str> {
    PUBLIC_MUTATION_COMMAND_KINDS
        .iter()
        .copied()
        .map(PublicCommandKind::public_mutation_token)
}

impl PublicCommandKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkflowOperator => "workflow_operator",
            Self::Status => "status",
            Self::RepairReviewState => FOLLOW_UP_REPAIR_REVIEW_STATE,
            Self::CloseCurrentTask => FollowUpKind::CloseCurrentTask.public_token(),
            Self::AdvanceLateStage => FOLLOW_UP_ADVANCE_LATE_STAGE,
            Self::Begin => "begin",
            Self::Complete => "complete",
            Self::Reopen => "reopen",
            Self::Transfer => "transfer",
            Self::MaterializeProjectionsStateDirOnly => "materialize_projections_state_dir_only",
        }
    }

    pub const fn public_mutation_name(self) -> Option<&'static str> {
        match self {
            Self::Begin => Some("begin"),
            Self::Complete => Some("complete"),
            Self::Reopen => Some("reopen"),
            Self::Transfer => Some("transfer"),
            Self::CloseCurrentTask => Some("close-current-task"),
            Self::RepairReviewState => Some("repair-review-state"),
            Self::AdvanceLateStage => Some("advance-late-stage"),
            Self::WorkflowOperator | Self::Status | Self::MaterializeProjectionsStateDirOnly => {
                None
            }
        }
    }

    pub const fn execution_mutation_name(self) -> Option<&'static str> {
        if self.is_execution_mutation() {
            self.public_mutation_name()
        } else {
            None
        }
    }

    pub const fn is_public_mutation(self) -> bool {
        self.public_mutation_name().is_some()
    }

    pub fn public_mutation_token(self) -> &'static str {
        self.public_mutation_name()
            .expect("public command kind should have a public mutation token")
    }

    pub fn matches_public_mutation_token(self, token: &str) -> bool {
        self.public_mutation_name() == Some(token)
    }

    pub const fn is_execution_mutation(self) -> bool {
        matches!(self, Self::Begin | Self::Complete | Self::Reopen)
    }

    pub fn from_execution_mutation_name(command_kind: &str) -> Option<Self> {
        match command_kind {
            "begin" => Some(Self::Begin),
            "complete" => Some(Self::Complete),
            "reopen" => Some(Self::Reopen),
            _ => None,
        }
    }

    pub fn from_public_mutation_token(command_kind: &str) -> Option<Self> {
        PUBLIC_MUTATION_COMMAND_KINDS
            .iter()
            .copied()
            .find(|kind| kind.matches_public_mutation_token(command_kind))
    }

    pub fn execution_mutation_name_from_public_argv(argv: &[String]) -> Option<&str> {
        execution_mutation_name_from_public_argv(argv)
    }

    pub(crate) fn execution_target_from_public_argv(
        argv: &[String],
    ) -> Option<super::execution_target::PublicExecutionCommandTarget> {
        execution_target_from_public_argv(argv)
    }

    pub(crate) fn execution_target_from_public_template_base_argv(
        argv: &[String],
    ) -> Option<super::execution_target::PublicExecutionCommandTarget> {
        execution_target_from_public_template_base_argv(argv)
    }
}
