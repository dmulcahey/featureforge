use crate::diagnostics::JsonFailure;
use crate::execution::command_eligibility::{PublicCommand, PublicCommandKind};
use crate::execution::status::{PlanExecutionStatus, PublicExecutionCommandContext};
use crate::execution::status_assembly::exact_route_surfaces::{
    execution_command_context_target, inconsistent_finalized_execution_route_projection_failure,
    missing_finalized_execution_route_projection_failure,
    require_public_argv_matches_execution_context,
};
use crate::execution::status_assembly::exact_route_template::require_public_template_matches_execution_context;

pub(crate) fn require_public_execution_command_route_target(
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    if finalized_public_execution_command_route_present(status) {
        require_finalized_public_execution_route_fields(status)?;
    }
    Ok(())
}

fn finalized_public_execution_command_route_present(status: &PlanExecutionStatus) -> bool {
    status.execution_command_context.is_some()
        || status
            .recommended_public_command
            .as_ref()
            .is_some_and(public_command_is_execution_mutation)
        || status
            .recommended_public_command_argv
            .as_ref()
            .is_some_and(|argv| {
                PublicCommandKind::execution_mutation_name_from_public_argv(argv).is_some()
            })
        || status
            .recommended_public_command_template
            .as_ref()
            .is_some_and(|template| {
                PublicCommandKind::from_execution_mutation_name(&template.command_kind).is_some()
            })
}

fn require_finalized_public_execution_route_fields(
    status: &PlanExecutionStatus,
) -> Result<(), JsonFailure> {
    let execution_command_context = status.execution_command_context.as_ref().ok_or_else(|| {
        missing_finalized_execution_route_projection_failure("execution_command_context")
    })?;
    let expected_target = execution_command_context_target(execution_command_context)?;
    if status.recommended_public_command_argv.is_none()
        && status.recommended_public_command_template.is_none()
    {
        return Err(missing_finalized_execution_route_projection_failure(
            "recommended_public_command_argv or recommended_public_command_template",
        ));
    }
    if let Some(argv) = status.recommended_public_command_argv.as_ref() {
        require_public_argv_matches_execution_context(argv, expected_target)?;
    }
    if let Some(template) = status.recommended_public_command_template.as_ref() {
        require_public_template_matches_execution_context(template, expected_target)?;
    }
    if let Some(command) = status.recommended_public_command.as_ref()
        && !recommended_command_matches_execution_context(command, execution_command_context)
    {
        return Err(inconsistent_finalized_execution_route_projection_failure(
            "recommended_public_command",
        ));
    }
    Ok(())
}

fn public_command_is_execution_mutation(command: &PublicCommand) -> bool {
    command.kind().is_execution_mutation()
}

fn recommended_command_matches_execution_context(
    command: &PublicCommand,
    execution_command_context: &PublicExecutionCommandContext,
) -> bool {
    command.to_mutation_request().is_some_and(|request| {
        request
            .kind
            .matches_public_mutation_token(&execution_command_context.command_kind)
            && request.task == execution_command_context.task_number
            && request.step == execution_command_context.step_id
    })
}
