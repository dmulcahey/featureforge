//! Plan contract integration/benchmark crate.
mod common;

use featureforge::contracts::plan::analyze_plan;
use featureforge::expect_ext::ExpectValueExt as _;

fn main() {
    let config = common::parse_args("plan_contract");
    if let Some(message) = common::render_run_gate_message(&config) {
        eprintln!("{message}");
        return;
    }
    let repo_dir = common::create_plan_contract_fixture_repo();
    let repo = repo_dir.path();
    let spec = repo.join(common::PLAN_CONTRACT_SPEC_REL);
    let plan = repo.join(common::PLAN_CONTRACT_PLAN_REL);

    let report = common::run_benchmark(&config, || {
        analyze_plan(&spec, &plan).expect_or_abort("plan-contract benchmark should succeed");
    });

    common::emit_report(&config, &report);
}
