use std::path::Path;

use featureforge::cli::plan_execution::StatusArgs;
use featureforge::execution::state::ExecutionRuntime;
use serde::Serialize;
use serde_json::{Value, to_value};

pub fn discover_execution_runtime(
    repo: &Path,
    state_dir: &Path,
    context: &str,
) -> ExecutionRuntime {
    let mut runtime = ExecutionRuntime::discover(repo).unwrap_or_else(|error| {
        panic!("{context}: failed to discover execution runtime: {error:?}")
    });
    runtime.state_dir = state_dir.to_path_buf();
    runtime
}

pub fn plan_execution_status_json(
    runtime: &ExecutionRuntime,
    plan: &str,
    external_review_result_ready: bool,
    context: &str,
) -> Value {
    serialize_json_value(
        runtime.status(&StatusArgs {
            plan: plan.into(),
            external_review_result_ready,
        }),
        &format!("{context}: plan execution status"),
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
