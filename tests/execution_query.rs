#[path = "support/process.rs"]
mod process_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use std::path::PathBuf;

use featureforge::cli::plan_execution::StatusArgs;
use featureforge::cli::workflow::OperatorArgs;
use featureforge::execution::query::{
    query_review_state, query_workflow_execution_state, query_workflow_routing_state,
};
use featureforge::execution::state::ExecutionRuntime;
use featureforge::workflow::operator::operator as workflow_operator;
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

#[test]
fn routing_snapshot_matches_workflow_operator_output_before_execution_starts() {
    let (repo_dir, _state_dir) = init_repo("execution-query-routing-snapshot");
    let repo = repo_dir.path();
    install_full_contract_ready_artifacts(repo);

    let plan = PathBuf::from(PLAN_REL);
    let routing = query_workflow_routing_state(repo, Some(&plan), false)
        .expect("routing query should succeed before execution starts");
    let operator = workflow_operator(
        repo,
        &OperatorArgs {
            plan: plan.clone(),
            external_review_result_ready: false,
            json: false,
        },
    )
    .expect("workflow operator should succeed before execution starts");

    assert_eq!(
        routing.phase, operator.phase,
        "routing phase should match workflow/operator"
    );
    assert_eq!(
        routing.phase_detail, operator.phase_detail,
        "routing phase detail should match workflow/operator",
    );
    assert_eq!(
        routing.review_state_status, operator.review_state_status,
        "routing review-state status should match workflow/operator",
    );
    assert_eq!(
        routing.qa_requirement, operator.qa_requirement,
        "routing QA requirement should match workflow/operator",
    );
    assert_eq!(
        routing.follow_up_override, operator.follow_up_override,
        "routing follow-up override should match workflow/operator",
    );
    assert_eq!(
        routing.finish_review_gate_pass_branch_closure_id,
        operator.finish_review_gate_pass_branch_closure_id,
        "routing finish-review gate pass identity should match workflow/operator",
    );
    assert_eq!(
        routing.next_action, operator.next_action,
        "routing next action should match workflow/operator",
    );
    assert_eq!(
        routing.recommended_command, operator.recommended_command,
        "routing recommended command should match workflow/operator",
    );
    assert_eq!(
        routing.recording_context.is_some(),
        operator.recording_context.is_some(),
        "routing recording context presence should match workflow/operator",
    );
    assert_eq!(
        routing.execution_command_context.is_some(),
        operator.execution_command_context.is_some(),
        "routing execution command context presence should match workflow/operator",
    );
}
