use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn execution_module_exports_query_boundary() {
    let execution_mod = fs::read_to_string(repo_root().join("src/execution/mod.rs"))
        .expect("execution mod should be readable");
    let query_source = fs::read_to_string(repo_root().join("src/execution/query.rs"))
        .expect("execution query source should be readable");
    let recording_source = fs::read_to_string(repo_root().join("src/execution/recording.rs"))
        .expect("execution recording source should be readable");
    let review_state_source = fs::read_to_string(repo_root().join("src/execution/review_state.rs"))
        .expect("execution review-state source should be readable");
    let operator_source = fs::read_to_string(repo_root().join("src/workflow/operator.rs"))
        .expect("workflow operator source should be readable");

    assert!(
        execution_mod.contains("pub mod query;"),
        "execution module should expose a dedicated query boundary for workflow and review-state consumers",
    );
    assert!(
        execution_mod.contains("pub mod recording;"),
        "execution module should expose a dedicated recording boundary for closure and milestone writers",
    );
    assert!(
        execution_mod.contains("query owns the authoritative review-state read model")
            && query_source.contains("workflow consumes this module as a read-only client")
            && recording_source
                .contains("intent adapters should delegate authoritative writes here")
            && review_state_source.contains(
                "reconcile/explain commands stay thin over query and recording boundaries"
            )
            && operator_source
                .contains("Workflow routing consumes the execution-owned query surface"),
        "U12 ownership boundaries should be documented in the owning modules",
    );
}

#[test]
fn workflow_operator_uses_execution_query_boundary_instead_of_raw_execution_internals() {
    let operator_source = fs::read_to_string(repo_root().join("src/workflow/operator.rs"))
        .expect("workflow operator source should be readable");
    let build_context_start = operator_source
        .find("fn build_context_with_plan(")
        .expect("workflow operator should keep build_context_with_plan");
    let build_context_end = operator_source[build_context_start..]
        .find("fn operator_plan_path(")
        .map(|offset| build_context_start + offset)
        .expect("workflow operator should keep operator_plan_path");
    let build_context_source = &operator_source[build_context_start..build_context_end];

    assert!(
        operator_source.contains("crate::execution::query::"),
        "workflow operator should consume the execution-owned query boundary",
    );
    assert!(
        operator_source.contains("query_workflow_routing_state("),
        "workflow operator should consume the execution-owned routing snapshot",
    );
    assert!(
        !build_context_source.contains("query_workflow_execution_state("),
        "workflow operator should not rebuild routing from the lower-level execution status query",
    );
    assert!(
        !build_context_source
            .contains("crate::execution::leases::load_status_authoritative_overlay_checked"),
        "workflow operator should not read authoritative overlays directly",
    );
    assert!(
        !build_context_source.contains("load_execution_context"),
        "workflow operator should not load execution context directly",
    );
    assert!(
        !build_context_source.contains("status_from_context"),
        "workflow operator should not assemble routing context from raw status internals",
    );
    assert!(
        !build_context_source.contains("runtime.status("),
        "workflow operator should consume execution status through the query boundary",
    );
    assert!(
        !build_context_source.contains("runtime.gate_review("),
        "workflow operator should consume review-gate state through the query boundary",
    );
    assert!(
        !build_context_source.contains("runtime.gate_finish("),
        "workflow operator should consume finish-gate state through the query boundary",
    );
    assert!(
        !build_context_source.contains("runtime.preflight_read_only("),
        "workflow operator should consume preflight state through the query boundary",
    );
    assert!(
        !build_context_source.contains("filter(|gate| gate.allowed)"),
        "workflow operator should not reconstruct finish-review gate pass identity from gate internals",
    );
}

#[test]
fn execution_query_boundary_stays_execution_owned() {
    let query_source = fs::read_to_string(repo_root().join("src/execution/query.rs"))
        .expect("execution query source should be readable");

    assert!(
        !query_source.contains("workflow::operator"),
        "execution query boundary should not depend on workflow/operator to derive review-state truth",
    );
    assert!(
        query_source.contains("pub struct ExecutionRoutingState")
            && query_source.contains("pub fn query_workflow_routing_state("),
        "execution query boundary should expose an execution-owned routing snapshot",
    );
    assert!(
        query_source.contains("pub finish_review_gate_pass_branch_closure_id: Option<String>"),
        "execution query boundary should expose finish-review gate pass branch closure identity",
    );
}

#[test]
fn mutate_and_review_state_use_recording_boundary_for_transition_writes() {
    let mutate_source = fs::read_to_string(repo_root().join("src/execution/mutate.rs"))
        .expect("execution mutate source should be readable");
    let review_state_source = fs::read_to_string(repo_root().join("src/execution/review_state.rs"))
        .expect("execution review_state source should be readable");

    assert!(
        mutate_source.contains("crate::execution::recording::"),
        "mutate.rs should consume the recording boundary for closure writes",
    );
    assert!(
        mutate_source.contains("query_workflow_routing_state"),
        "mutate.rs should consume the execution-owned routing boundary",
    );
    assert!(
        !mutate_source.contains("crate::workflow::operator"),
        "mutate.rs should not import or call workflow/operator directly",
    );
    for forbidden in [
        "record_task_closure_result(",
        "record_task_closure_negative_result(",
        "remove_current_task_closure_results(",
        "append_superseded_task_closure_ids(",
        "append_superseded_branch_closure_ids(",
        "set_current_branch_closure_id(",
        "record_final_review_result(",
        "record_release_readiness_result(",
        "record_browser_qa_result(",
    ] {
        assert!(
            !mutate_source.contains(forbidden),
            "mutate.rs should not call transition write primitive `{forbidden}` directly",
        );
    }

    assert!(
        review_state_source.contains("crate::execution::recording::"),
        "review_state.rs should consume the recording boundary for overlay restoration",
    );
    assert!(
        !review_state_source.contains("load_authoritative_transition_state("),
        "review_state.rs should not load transition state directly for overlay restoration",
    );
    assert!(
        !review_state_source.contains("set_current_branch_closure_id("),
        "review_state.rs should not call transition write primitives directly",
    );
    assert!(
        !review_state_source.contains("parse_artifact_document("),
        "review_state.rs should not parse rendered artifacts directly when reconciling authoritative state",
    );
}

#[test]
fn explicit_mutation_paths_keep_strict_authoritative_state_validation() {
    let state_source = fs::read_to_string(repo_root().join("src/execution/state.rs"))
        .expect("execution state source should be readable");
    let dispatch_start = state_source
        .find("fn record_review_dispatch_strategy_checkpoint(")
        .expect("state.rs should keep record_review_dispatch_strategy_checkpoint");
    let dispatch_end = state_source[dispatch_start..]
        .find("fn ensure_review_dispatch_authoritative_bootstrap(")
        .map(|offset| dispatch_start + offset)
        .expect("state.rs should keep ensure_review_dispatch_authoritative_bootstrap");
    let checkpoint_start = state_source
        .find("fn persist_finish_review_gate_pass_checkpoint(")
        .expect("state.rs should keep persist_finish_review_gate_pass_checkpoint");
    let checkpoint_end = state_source[checkpoint_start..]
        .find("fn gate_review_from_context_internal(")
        .map(|offset| checkpoint_start + offset)
        .expect("state.rs should keep gate_review_from_context_internal");
    let dispatch_source = &state_source[dispatch_start..dispatch_end];
    let checkpoint_source = &state_source[checkpoint_start..checkpoint_end];

    assert!(
        dispatch_source.contains("load_authoritative_transition_state("),
        "record-review-dispatch mutation should validate authoritative active-contract truth through the strict transition-state loader",
    );
    assert!(
        !dispatch_source.contains("load_authoritative_transition_state_relaxed("),
        "record-review-dispatch mutation must not bypass active-contract validation with the relaxed transition-state loader",
    );
    assert!(
        checkpoint_source.contains("load_authoritative_transition_state("),
        "gate-review checkpoint mutation should validate authoritative active-contract truth through the strict transition-state loader",
    );
    assert!(
        !checkpoint_source.contains("load_authoritative_transition_state_relaxed("),
        "gate-review checkpoint mutation must not bypass active-contract validation with the relaxed transition-state loader",
    );
}
