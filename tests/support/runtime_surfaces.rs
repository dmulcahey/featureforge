use featureforge::cli::plan_execution::StatusArgs;
use featureforge::cli::workflow::OperatorArgs;
use featureforge::execution::state::ExecutionRuntime;
use featureforge::workflow::operator;
use serde::Serialize;
use serde_json::{Value, to_value};

pub fn workflow_operator_json(
    runtime: &ExecutionRuntime,
    plan: &str,
    external_review_result_ready: bool,
    context: &str,
) -> Value {
    serialize_json_value(
        operator::operator_for_runtime(
            runtime,
            &OperatorArgs {
                plan: plan.into(),
                external_review_result_ready,
                json: true,
            },
        ),
        &format!("{context}: workflow operator"),
    )
}

pub fn workflow_gate_review_json(runtime: &ExecutionRuntime, plan: &str, context: &str) -> Value {
    serialize_json_value(
        runtime.review_gate(&StatusArgs {
            plan: plan.into(),
            external_review_result_ready: false,
        }),
        &format!("{context}: workflow gate-review"),
    )
}

pub fn workflow_gate_finish_json(runtime: &ExecutionRuntime, plan: &str, context: &str) -> Value {
    serialize_json_value(
        runtime.finish_gate(&StatusArgs {
            plan: plan.into(),
            external_review_result_ready: false,
        }),
        &format!("{context}: workflow gate-finish"),
    )
}

fn serialize_json_value<T, E>(result: Result<T, E>, context: &str) -> Value
where
    T: Serialize,
    E: std::fmt::Debug,
{
    let value = result.unwrap_or_else(|error| panic!("{context} should succeed: {error:?}"));
    to_value(value).unwrap_or_else(|error| panic!("{context} should serialize: {error}"))
}
