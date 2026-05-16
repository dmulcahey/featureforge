use std::path::Path;

use crate::execution::command_eligibility::{
    PublicCommand, PublicCommandInputRequirement, public_command_recommendation_surfaces,
};
use crate::execution::follow_up::{FollowUpAliasContext, FollowUpKind, normalize_follow_up_alias};
use crate::execution::public_command_types::{
    RecommendedPublicCommandArgv, RecommendedPublicCommandTemplate,
};
use crate::execution::public_route_guidance::workflow_operator_json_display_command;
use crate::execution::query::ExecutionRoutingState;

fn workflow_operator_json_requery_command(
    plan: &Path,
    external_review_result_ready: bool,
) -> PublicCommand {
    PublicCommand::WorkflowOperator {
        plan: plan.display().to_string(),
        external_review_result_ready,
        json: true,
    }
}

pub(crate) fn workflow_operator_requery_surfaces(
    plan: &Path,
    external_review_result_ready: bool,
) -> (String, Vec<String>) {
    let command = workflow_operator_json_requery_command(plan, external_review_result_ready);
    (
        workflow_operator_json_display_command(external_review_result_ready).to_owned(),
        command.to_argv(),
    )
}

pub(crate) fn workflow_operator_requery_optional_surfaces(
    plan: &Path,
    external_review_result_ready: bool,
) -> (Option<String>, Option<Vec<String>>) {
    let (recommended_command, recommended_public_command_argv) =
        workflow_operator_requery_surfaces(plan, external_review_result_ready);
    (
        Some(recommended_command),
        Some(recommended_public_command_argv),
    )
}

#[derive(Debug, Clone)]
pub(crate) struct PublicRecoveryContract {
    pub(crate) required_follow_up: Option<String>,
    pub(crate) recommended_command: Option<String>,
    pub(crate) recommended_public_command_argv: RecommendedPublicCommandArgv,
    pub(crate) recommended_public_command_template: RecommendedPublicCommandTemplate,
    pub(crate) required_inputs: Vec<PublicCommandInputRequirement>,
    pub(crate) rederive_via_workflow_operator: Option<bool>,
}

impl PublicRecoveryContract {
    fn empty() -> Self {
        Self {
            required_follow_up: None,
            recommended_command: None,
            recommended_public_command_argv: None,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: None,
        }
    }

    fn diagnostic_only() -> Self {
        Self::empty()
    }

    fn from_public_command(
        required_follow_up: String,
        command: Option<&PublicCommand>,
    ) -> Option<Self> {
        let (recommended_command, recommended_public_command_argv, template, required_inputs) =
            public_command_recommendation_surfaces(command);
        (recommended_public_command_argv.is_some() || !required_inputs.is_empty()).then_some(Self {
            required_follow_up: Some(required_follow_up),
            recommended_command,
            recommended_public_command_argv,
            recommended_public_command_template: template,
            required_inputs,
            rederive_via_workflow_operator: None,
        })
    }

    fn from_requery(
        plan: &Path,
        external_review_result_ready: bool,
        required_follow_up: String,
    ) -> Self {
        let (recommended_command, recommended_public_command_argv) =
            workflow_operator_requery_optional_surfaces(plan, external_review_result_ready);
        Self {
            required_follow_up: Some(required_follow_up),
            recommended_command,
            recommended_public_command_argv,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: Some(true),
        }
    }
}

pub(crate) fn public_recovery_contract_for_follow_up(
    plan: &Path,
    operator: Option<&ExecutionRoutingState>,
    required_follow_up: Option<String>,
) -> PublicRecoveryContract {
    let Some(required_follow_up) = required_follow_up else {
        return PublicRecoveryContract::empty();
    };
    match normalize_follow_up_alias(
        Some(&required_follow_up),
        FollowUpAliasContext::PublicRouting,
    ) {
        Some(FollowUpKind::RequestExternalReview | FollowUpKind::WaitForExternalReviewResult) => {
            return PublicRecoveryContract::from_requery(plan, false, required_follow_up);
        }
        Some(FollowUpKind::ExecutionReentry) => {
            if let Some(contract) = contract_from_matching_operator(&required_follow_up, operator) {
                return contract;
            }
            return PublicRecoveryContract::diagnostic_only();
        }
        _ => {}
    }
    if let Some(contract) = contract_from_matching_operator(&required_follow_up, operator) {
        return contract;
    }
    PublicRecoveryContract::diagnostic_only()
}

fn contract_from_matching_operator(
    required_follow_up: &str,
    operator: Option<&ExecutionRoutingState>,
) -> Option<PublicRecoveryContract> {
    let command = operator
        .and_then(|operator| operator.recommended_public_command.as_ref())
        .filter(|command| {
            close_current_task_command_matches_follow_up(Some(required_follow_up), command)
        });
    PublicRecoveryContract::from_public_command(required_follow_up.to_owned(), command)
}

pub(crate) fn close_current_task_command_matches_follow_up(
    required_follow_up: Option<&str>,
    recommended_command: &PublicCommand,
) -> bool {
    match normalize_follow_up_alias(required_follow_up, FollowUpAliasContext::PublicRouting) {
        Some(FollowUpKind::ExecutionReentry) => matches!(
            recommended_command,
            PublicCommand::Begin { .. }
                | PublicCommand::Reopen { .. }
                | PublicCommand::Complete { .. }
        ),
        Some(FollowUpKind::RepairReviewState) => {
            matches!(recommended_command, PublicCommand::RepairReviewState { .. })
        }
        Some(FollowUpKind::RequestExternalReview | FollowUpKind::WaitForExternalReviewResult) => {
            matches!(recommended_command, PublicCommand::WorkflowOperator { .. })
        }
        Some(FollowUpKind::RunVerification) => {
            recommended_command.task_closure_result_inputs_required()
        }
        Some(FollowUpKind::RecordHandoff) => matches!(
            recommended_command,
            PublicCommand::TransferHandoff { .. } | PublicCommand::TransferRepairStep { .. }
        ),
        Some(FollowUpKind::AdvanceLateStage | FollowUpKind::ResolveReleaseBlocker) => {
            matches!(recommended_command, PublicCommand::AdvanceLateStage { .. })
        }
        Some(
            FollowUpKind::CloseCurrentTask | FollowUpKind::GateReview | FollowUpKind::GateFinish,
        )
        | None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_operator_requery_surfaces_use_placeholder_display_and_concrete_argv() {
        let plan = Path::new("docs/featureforge/plans/plan with spaces.md");

        let (recommended_command, argv) = workflow_operator_requery_surfaces(plan, false);

        assert_eq!(
            recommended_command,
            "featureforge workflow operator --plan <approved-plan-path> --json"
        );
        assert!(
            !recommended_command.contains("plan with spaces"),
            "display command must not contain an unquoted concrete plan path"
        );
        assert_eq!(
            argv,
            vec![
                "featureforge",
                "workflow",
                "operator",
                "--plan",
                "docs/featureforge/plans/plan with spaces.md",
                "--json",
            ]
        );
    }

    #[test]
    fn workflow_operator_external_ready_requery_display_uses_placeholder() {
        let plan = Path::new("docs/featureforge/plans/plan with spaces.md");

        let (recommended_command, argv) = workflow_operator_requery_surfaces(plan, true);

        assert_eq!(
            recommended_command,
            "featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready --json"
        );
        assert_eq!(
            argv,
            vec![
                "featureforge",
                "workflow",
                "operator",
                "--plan",
                "docs/featureforge/plans/plan with spaces.md",
                "--external-review-result-ready",
                "--json",
            ]
        );
    }
}
