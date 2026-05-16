use super::decision::NextPublicAction;
#[cfg(test)]
use super::public_commands::repair_review_state_public_command;
#[cfg(test)]
use crate::execution::command_eligibility::public_advance_late_stage_command_for_phase_detail;
use crate::execution::command_eligibility::{PublicCommand, PublicCommandKind};
use crate::execution::phase;
use crate::execution::public_route_guidance::workflow_operator_json_display_command;

pub(crate) fn synthesize_next_public_action(
    recommended_public_command: Option<&PublicCommand>,
    phase_detail: &str,
    _plan_path: &str,
) -> Option<NextPublicAction> {
    if let Some(command) = recommended_public_command
        .filter(|_| !phase::RECOMMENDED_COMMAND_OMITTED_PHASE_DETAILS.contains(&phase_detail))
        .filter(|command| command.kind() != PublicCommandKind::WorkflowOperator)
        .filter(|command| command.to_invocation().is_some())
        .map(PublicCommand::to_display_command)
    {
        return Some(NextPublicAction::display_summary(command));
    }
    let command = match phase_detail {
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED => {
            workflow_operator_json_display_command(false).to_owned()
        }
        _ => return None,
    };
    Some(NextPublicAction::display_summary(command))
}

#[cfg(test)]
pub(crate) fn public_command_for_phase_detail(
    phase_detail: &str,
    plan_path: &str,
) -> Option<PublicCommand> {
    match phase_detail {
        phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED => Some(PublicCommand::WorkflowOperator {
            plan: plan_path.to_owned(),
            external_review_result_ready: false,
            json: true,
        }),
        phase::DETAIL_EXECUTION_REENTRY_REQUIRED => {
            Some(repair_review_state_public_command(plan_path))
        }
        phase::DETAIL_PLANNING_REENTRY_REQUIRED => None,
        _ => public_advance_late_stage_command_for_phase_detail(plan_path, phase_detail),
    }
}
