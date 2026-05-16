use crate::diagnostics::JsonFailure;
use crate::execution::command_eligibility::{
    PublicCommandKind, PublicCommandTemplate, PublicExecutionCommandTarget,
    execution_template_inputs_are_bindable,
};
use crate::execution::status_assembly::exact_route_surfaces::inconsistent_finalized_execution_route_projection_failure;

pub(super) fn require_public_template_matches_execution_context(
    template: &PublicCommandTemplate,
    expected_target: PublicExecutionCommandTarget,
) -> Result<(), JsonFailure> {
    let command_kind = PublicCommandKind::from_execution_mutation_name(&template.command_kind)
        .ok_or_else(|| {
            inconsistent_finalized_execution_route_projection_failure(
                "recommended_public_command_template.command_kind",
            )
        })?;
    if command_kind != expected_target.kind {
        return Err(inconsistent_finalized_execution_route_projection_failure(
            "recommended_public_command_template.command_kind",
        ));
    }
    let Some(actual_target) =
        PublicCommandKind::execution_target_from_public_template_base_argv(&template.base_argv)
    else {
        return Err(inconsistent_finalized_execution_route_projection_failure(
            "recommended_public_command_template.base_argv",
        ));
    };
    if actual_target != expected_target {
        return Err(inconsistent_finalized_execution_route_projection_failure(
            "recommended_public_command_template.base_argv",
        ));
    }
    if !execution_template_inputs_are_bindable(template, expected_target.kind) {
        return Err(inconsistent_finalized_execution_route_projection_failure(
            "recommended_public_command_template.input_bindings",
        ));
    }
    Ok(())
}
