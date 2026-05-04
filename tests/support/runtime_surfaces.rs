use featureforge::execution::state::ExecutionRuntime;
use serde_json::Value;

use crate::runtime_json_support::run_featureforge_json_real_cli;

pub fn workflow_operator_json(
    runtime: &ExecutionRuntime,
    plan: &str,
    external_review_result_ready: bool,
    context: &str,
) -> Value {
    let mut args = vec!["workflow", "operator", "--plan", plan, "--json"];
    if external_review_result_ready {
        args.push("--external-review-result-ready");
    }
    run_featureforge_json_real_cli(
        &runtime.repo_root,
        &runtime.state_dir,
        &args,
        &format!("{context}: workflow operator"),
    )
}
