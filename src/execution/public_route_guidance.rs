//! Shared public route guidance text.
//!
//! Keep executable-route law centralized here and the detailed agent-facing
//! binding examples in `references/operator-route-authority.md`.

macro_rules! public_typed_operator_route_contract {
    () => {
        "follow `recommended_public_command_argv` when present; when a template requires input, use `required_inputs` as validation metadata and rerun the same plan-bound workflow/operator query with `--input NAME=VALUE` so Rust materializes `recommended_public_command_argv`. Treat `recommended_public_command_template.input_bindings` as machine-readable template metadata, not a second executable path. If neither executable surface is present, stop and report the route diagnostic"
    };
}

pub(crate) use public_typed_operator_route_contract;

pub(crate) const PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT: &str =
    public_typed_operator_route_contract!();

pub(crate) const WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND: &str =
    "featureforge workflow operator --plan <approved-plan-path> --json";

pub(crate) const WORKFLOW_OPERATOR_EXTERNAL_READY_JSON_DISPLAY_COMMAND: &str = "featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready --json";

pub(crate) const fn workflow_operator_json_display_command(
    external_review_result_ready: bool,
) -> &'static str {
    if external_review_result_ready {
        WORKFLOW_OPERATOR_EXTERNAL_READY_JSON_DISPLAY_COMMAND
    } else {
        WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND
    }
}

pub(crate) const EXECUTE_RECOMMENDED_PUBLIC_ARGV_GUIDANCE: &str = "query workflow operator JSON with `workflow operator --plan <approved-plan-path> --json` and execute recommended_public_command_argv";

pub(crate) const WORKFLOW_OPERATOR_TEMPLATE_JSON_QUERY: &str = "query workflow operator JSON with `workflow operator --plan <approved-plan-path> --input NAME=VALUE --json` so Rust materializes recommended_public_command_argv";

pub(crate) const OPERATOR_ROUTE_AUTHORITY_REFERENCE: &str =
    "follow the shared route law in references/operator-route-authority.md";
