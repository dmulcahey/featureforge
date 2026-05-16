use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::command_eligibility::{PublicCommandKind, PublicExecutionCommandTarget};
use crate::execution::status::PublicExecutionCommandContext;

pub(super) fn execution_command_context_target(
    execution_command_context: &PublicExecutionCommandContext,
) -> Result<PublicExecutionCommandTarget, JsonFailure> {
    let kind =
        PublicCommandKind::from_execution_mutation_name(&execution_command_context.command_kind)
            .ok_or_else(|| {
                missing_finalized_execution_route_projection_failure(
                    "execution_command_context.command_kind",
                )
            })?;
    Ok(PublicExecutionCommandTarget {
        kind,
        task: execution_command_context.task_number.ok_or_else(|| {
            missing_finalized_execution_route_projection_failure(
                "execution_command_context.task_number",
            )
        })?,
        step: execution_command_context.step_id.ok_or_else(|| {
            missing_finalized_execution_route_projection_failure(
                "execution_command_context.step_id",
            )
        })?,
    })
}

pub(super) fn require_public_argv_matches_execution_context(
    argv: &[String],
    expected_target: PublicExecutionCommandTarget,
) -> Result<(), JsonFailure> {
    let Some(actual_target) = PublicCommandKind::execution_target_from_public_argv(argv) else {
        return Err(inconsistent_finalized_execution_route_projection_failure(
            "recommended_public_command_argv",
        ));
    };
    if actual_target != expected_target {
        return Err(inconsistent_finalized_execution_route_projection_failure(
            "recommended_public_command_argv",
        ));
    }
    Ok(())
}

pub(super) fn missing_finalized_execution_route_projection_failure(field: &str) -> JsonFailure {
    JsonFailure::new(
        FailureClass::MalformedExecutionState,
        format!(
            "Finalized public execution route projection is missing required field `{field}`; re-query workflow/operator JSON and use its typed public route fields instead of recomputing route candidates."
        ),
    )
}

pub(super) fn inconsistent_finalized_execution_route_projection_failure(
    field: &str,
) -> JsonFailure {
    JsonFailure::new(
        FailureClass::MalformedExecutionState,
        format!(
            "Finalized public execution route projection has inconsistent execution_command_context and `{field}` fields; re-query workflow/operator JSON and use its typed public route fields instead of recomputing route candidates."
        ),
    )
}
