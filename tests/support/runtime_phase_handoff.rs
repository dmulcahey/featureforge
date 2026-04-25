use featureforge::execution::state::ExecutionRuntime;
use featureforge::workflow::operator;
use serde::Serialize;
use serde_json::{Value, to_value};

pub fn workflow_phase_json(runtime: &ExecutionRuntime, context: &str) -> Value {
    serialize_json_value(
        operator::phase_for_runtime(runtime),
        &format!("{context}: workflow phase"),
    )
}

pub fn workflow_handoff_json(runtime: &ExecutionRuntime, context: &str) -> Value {
    serialize_json_value(
        operator::handoff_for_runtime(runtime),
        &format!("{context}: workflow handoff"),
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
