use super::{PublicCommandKind, late_stage::PublicAdvanceLateStageMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicMutationRequest {
    pub kind: PublicCommandKind,
    pub task: Option<u32>,
    pub step: Option<u32>,
    pub expect_execution_fingerprint: Option<String>,
    pub transfer_mode: Option<PublicTransferMode>,
    pub transfer_scope: Option<String>,
    pub advance_late_stage_mode: Option<PublicAdvanceLateStageMode>,
}

impl PublicMutationRequest {
    pub fn repair_review_state() -> Self {
        Self::from_public_mutation_parts(
            PublicCommandKind::RepairReviewState,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    pub fn begin(task: u32, step: u32, expect_execution_fingerprint: Option<String>) -> Self {
        Self::from_public_mutation_parts(
            PublicCommandKind::Begin,
            Some(task),
            Some(step),
            expect_execution_fingerprint,
            None,
            None,
            None,
        )
    }

    pub fn complete(task: u32, step: u32, expect_execution_fingerprint: Option<String>) -> Self {
        Self::from_public_mutation_parts(
            PublicCommandKind::Complete,
            Some(task),
            Some(step),
            expect_execution_fingerprint,
            None,
            None,
            None,
        )
    }

    pub fn reopen(task: u32, step: u32, expect_execution_fingerprint: Option<String>) -> Self {
        Self::from_public_mutation_parts(
            PublicCommandKind::Reopen,
            Some(task),
            Some(step),
            expect_execution_fingerprint,
            None,
            None,
            None,
        )
    }

    pub fn transfer_repair_step(
        task: u32,
        step: u32,
        expect_execution_fingerprint: Option<String>,
    ) -> Self {
        Self::from_public_mutation_parts(
            PublicCommandKind::Transfer,
            Some(task),
            Some(step),
            expect_execution_fingerprint,
            Some(PublicTransferMode::RepairStep),
            None,
            None,
        )
    }

    pub fn transfer_handoff(scope: Option<String>) -> Self {
        Self::from_public_mutation_parts(
            PublicCommandKind::Transfer,
            None,
            None,
            None,
            Some(PublicTransferMode::WorkflowHandoff),
            scope,
            None,
        )
    }

    pub fn close_current_task(task: Option<u32>) -> Self {
        Self::from_public_mutation_parts(
            PublicCommandKind::CloseCurrentTask,
            task,
            None,
            None,
            None,
            None,
            None,
        )
    }

    pub fn advance_late_stage(mode: PublicAdvanceLateStageMode) -> Self {
        Self::from_public_mutation_parts(
            PublicCommandKind::AdvanceLateStage,
            None,
            None,
            None,
            None,
            None,
            Some(mode),
        )
    }

    fn from_public_mutation_parts(
        kind: PublicCommandKind,
        task: Option<u32>,
        step: Option<u32>,
        expect_execution_fingerprint: Option<String>,
        transfer_mode: Option<PublicTransferMode>,
        transfer_scope: Option<String>,
        advance_late_stage_mode: Option<PublicAdvanceLateStageMode>,
    ) -> Self {
        debug_assert!(
            kind.is_public_mutation(),
            "PublicMutationRequest requires a public mutation command kind"
        );
        Self {
            kind,
            task,
            step,
            expect_execution_fingerprint,
            transfer_mode,
            transfer_scope,
            advance_late_stage_mode,
        }
    }

    pub fn public_command_name(&self) -> Option<&'static str> {
        self.kind.public_mutation_name()
    }

    pub fn command_name_for_diagnostics(&self) -> &'static str {
        self.public_command_name()
            .unwrap_or_else(|| self.kind.as_str())
    }

    pub(super) fn execution_command_kind(&self) -> Option<&'static str> {
        self.kind.execution_mutation_name()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicTransferMode {
    RepairStep,
    WorkflowHandoff,
}
