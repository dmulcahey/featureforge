#[path = "support/files.rs"]
mod files_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use std::path::PathBuf;

use featureforge::cli::plan_execution::StatusArgs;
use featureforge::execution::query::{query_review_state, query_workflow_execution_state};
use featureforge::execution::state::ExecutionRuntime;
use workflow_support::{init_repo, install_full_contract_ready_artifacts};

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

#[test]
fn query_boundary_reports_empty_review_state_before_execution_starts() {
    let (repo_dir, _state_dir) = init_repo("execution-query-empty-review-state");
    let repo = repo_dir.path();
    install_full_contract_ready_artifacts(repo);

    let runtime = ExecutionRuntime::discover(repo).expect("runtime should discover");
    let plan = PathBuf::from(PLAN_REL);
    let status_args = StatusArgs { plan: plan.clone() };

    let review_state = query_review_state(&runtime, &status_args)
        .expect("review-state query should succeed before execution starts");
    assert!(
        review_state.current_task_closures.is_empty(),
        "fresh approved plans should not expose current task closures before execution starts",
    );
    assert!(
        review_state.current_branch_closure.is_none(),
        "fresh approved plans should not expose a current branch closure before execution starts",
    );
    assert!(
        review_state.superseded_closures.is_empty(),
        "fresh approved plans should not expose superseded closures before execution starts",
    );
    assert!(
        review_state.stale_unreviewed_closures.is_empty(),
        "fresh approved plans should not expose stale-unreviewed closures before execution starts",
    );

    let workflow_state = query_workflow_execution_state(&runtime, PLAN_REL)
        .expect("workflow query should succeed before execution starts");
    assert_eq!(
        workflow_state
            .execution_status
            .as_ref()
            .map(|status| status.execution_started.as_str()),
        Some("no"),
        "workflow query should carry the current execution status snapshot",
    );
    assert!(
        workflow_state.preflight.is_some(),
        "workflow query should surface preflight state before execution starts",
    );
    assert!(
        workflow_state.gate_review.is_none(),
        "workflow query should not expose review-gate state before execution starts",
    );
    assert!(
        workflow_state.gate_finish.is_none(),
        "workflow query should not expose finish-gate state before execution starts",
    );
    assert_eq!(
        workflow_state.task_review_dispatch_id, None,
        "fresh approved plans should not expose task review dispatch lineage before execution starts",
    );
    assert_eq!(
        workflow_state.final_review_dispatch_id, None,
        "fresh approved plans should not expose final-review dispatch lineage before execution starts",
    );
    assert_eq!(
        workflow_state.current_branch_closure_id, None,
        "fresh approved plans should not expose a branch closure before execution starts",
    );
    assert_eq!(
        workflow_state.finish_review_gate_pass_branch_closure_id, None,
        "fresh approved plans should not expose a finish-review gate pass branch closure before execution starts",
    );
    assert_eq!(
        workflow_state.current_release_readiness_result, None,
        "fresh approved plans should not expose release-readiness state before execution starts",
    );
}
