#[path = "support/public_flow_scan.rs"]
mod public_flow_scan;
#[path = "support/rust_source_scan.rs"]
mod rust_source_scan;

use std::fs;
use std::path::Path;

const FORBIDDEN_FILES: &[&str] = &[
    "src/execution/state.rs",
    "src/execution/read_model.rs",
    "src/execution/query.rs",
    "src/execution/router.rs",
    "src/execution/next_action.rs",
    "src/execution/review_state.rs",
    "src/execution/closure_graph.rs",
    "src/execution/mutate.rs",
];

const FORBIDDEN_GATE_CALLS: &[&str] = &[
    concat!("pre", "flight_from_context"),
    "gate_review_from_context",
    "gate_finish_from_context",
];

const FORBIDDEN_STALE_RECOMPUTATION_FILES: &[&str] = &[
    "src/execution/read_model.rs",
    "src/execution/query.rs",
    "src/execution/router.rs",
    "src/execution/next_action.rs",
    "src/execution/review_state.rs",
    "src/execution/closure_graph.rs",
    "src/execution/mutate.rs",
];

const FORBIDDEN_STALE_RECOMPUTATION_CALLS: &[&str] = &[
    "stale_current_task_closure_record_ids",
    "execution_reentry_current_task_closure_targets",
];

const FORBIDDEN_STALE_TARGET_RECOMPUTATION_FILES: &[&str] = &[
    "src/execution/read_model.rs",
    "src/execution/query.rs",
    "src/execution/router.rs",
    "src/execution/next_action.rs",
    "src/execution/review_state.rs",
    "src/execution/closure_graph.rs",
];

const FORBIDDEN_STALE_TARGET_RECOMPUTATION_CALLS: &[&str] = &[
    "task_closure_baseline_repair_candidate",
    "earliest_unresolved_stale_task_from_closure_graph",
    "derive_stale_unreviewed_closures",
    "pre_reducer_earliest_unresolved_stale_task",
];

const FORBIDDEN_STALE_TARGET_RECOMPUTATION_FUNCTIONS: &[(&str, &str)] = &[(
    "src/execution/read_model.rs",
    "project_public_route_mutation_targets",
)];

const FORBIDDEN_STALE_TARGET_FABRICATION_PATTERNS: &[(&str, &str)] = &[
    (
        "src/execution/next_action.rs",
        "earliest_stale_task: status.blocking_task",
    ),
    (
        "src/execution/next_action.rs",
        concat!(
            "status.execution_reentry_target_source.as_deref() == Some(\"",
            "closure_graph",
            "_stale_target",
            "\")",
        ),
    ),
    (
        "src/execution/state.rs",
        "earliest_stale_task: derived_stale_task",
    ),
];

const STATE_DIRECT_GATE_COMMAND_BODIES: &[&str] = &[
    concat!("pre", "flight_gate_with_mode"),
    "review_gate",
    "finish_gate",
    "gate_review_command_phase_gate",
    concat!("pre", "flight_from_context"),
    "gate_review_from_context",
    "gate_finish_from_context",
];

const INTERNAL_PLAN_EXECUTION_ARG_STRUCTS: &[&str] = &[
    "RecordReviewDispatchArgs",
    "RecordBranchClosureArgs",
    "RecordReleaseReadinessArgs",
    "RecordFinalReviewArgs",
    "RecordQaArgs",
    "GateContractArgs",
    "RecordContractArgs",
    "GateEvaluatorArgs",
    "RecordEvaluationArgs",
    "GateHandoffArgs",
    "RecordHandoffArgs",
    "RecommendArgs",
    "RebuildEvidenceArgs",
    "NoteArgs",
];

#[test]
fn public_command_boundary_test_helpers_do_not_expose_removed_or_hidden_workflow_commands() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let violations =
        public_flow_scan::public_command_boundary_forbidden_test_helper_violations(repo_root);

    assert!(
        violations.is_empty(),
        "public command tests must not gain capabilities unavailable through the compiled CLI:\n{}",
        violations.join("\n")
    );
}

#[test]
fn plan_execution_cli_module_contains_only_public_command_args() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = fs::read_to_string(repo_root.join("src/cli/plan_execution.rs"))
        .expect("plan execution CLI module should be readable");
    let mut violations = Vec::new();

    for struct_name in INTERNAL_PLAN_EXECUTION_ARG_STRUCTS {
        let pattern = format!("struct {struct_name}");
        if source.contains(&pattern) {
            violations.push(pattern);
        }
    }

    assert!(
        violations.is_empty(),
        "src/cli/plan_execution.rs must not define internal-only argument structs for commands that are not public CLI variants:\n{}",
        violations.join("\n")
    );
}

#[test]
fn public_aggregate_mutations_own_event_log_command_identity() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let advance_source =
        fs::read_to_string(repo_root.join("src/execution/commands/advance_late_stage.rs"))
            .expect("advance_late_stage source should be readable");
    let advance_impl = rust_function_body(&advance_source, "advance_late_stage_impl")
        .expect("advance_late_stage_impl should exist");
    let close_source =
        fs::read_to_string(repo_root.join("src/execution/commands/close_current_task.rs"))
            .expect("close_current_task source should be readable");
    let close_impl = rust_function_body(&close_source, "close_current_task")
        .expect("close_current_task should exist");

    let hidden_advance_owners = [
        "EventCommandOwner::InternalRecordBranchClosure",
        "EventCommandOwner::InternalRecordReleaseReadiness",
        "EventCommandOwner::InternalRecordFinalReview",
        "EventCommandOwner::InternalRecordQa",
        "EventCommandOwner::InternalRecordReviewDispatch",
        "\"record_branch_closure\"",
        "\"record_release_readiness\"",
        "\"record_final_review\"",
        "\"record_qa\"",
        "\"record_review_dispatch\"",
    ];
    for forbidden in hidden_advance_owners {
        assert!(
            !advance_impl.contains(forbidden),
            "advance_late_stage_impl must persist authoritative event envelopes through the public aggregate owner, not `{forbidden}`"
        );
    }
    assert!(
        advance_impl.contains("EventCommandOwner::PublicAdvanceLateStage"),
        "advance_late_stage_impl must pass the typed public aggregate event owner into normal-path recording helpers"
    );
    let release_primitive_impl = rust_function_body(&advance_source, "record_release_readiness")
        .expect("record_release_readiness should exist");
    assert!(
        release_primitive_impl.contains("EventCommandOwner::InternalRecordReleaseReadiness"),
        "internal record_release_readiness compatibility path must keep primitive event owner identity"
    );
    assert!(
        !release_primitive_impl.contains("EventCommandOwner::PublicAdvanceLateStage"),
        "internal record_release_readiness compatibility path must not persist under the public aggregate event owner"
    );
    let final_review_primitive_impl = rust_function_body(&advance_source, "record_final_review")
        .expect("record_final_review should exist");
    assert!(
        final_review_primitive_impl.contains("EventCommandOwner::InternalRecordFinalReview"),
        "internal record_final_review compatibility path must keep primitive event owner identity"
    );
    assert!(
        !final_review_primitive_impl.contains("EventCommandOwner::PublicAdvanceLateStage"),
        "internal record_final_review compatibility path must not persist under the public aggregate event owner"
    );

    let hidden_close_owners = [
        "EventCommandOwner::InternalRecordReviewDispatch",
        "\"record_review_dispatch\"",
    ];
    for forbidden in hidden_close_owners {
        assert!(
            !close_impl.contains(forbidden),
            "close_current_task must refresh dispatch lineage through the public aggregate owner, not `{forbidden}`"
        );
    }
    assert!(
        close_impl.contains("EventCommandOwner::PublicCloseCurrentTask"),
        "close_current_task must pass the typed public aggregate event owner into dispatch refresh"
    );

    let branch_truth_source =
        fs::read_to_string(repo_root.join("src/execution/commands/common/branch_closure_truth.rs"))
            .expect("branch_closure_truth source should be readable");
    for helper_name in [
        "branch_closure_already_current_output",
        "branch_closure_already_current_empty_lineage_exemption_output",
    ] {
        let helper_body = rust_function_body(&branch_truth_source, helper_name)
            .unwrap_or_else(|| panic!("{helper_name} should exist"));
        assert!(
            helper_body.contains("command_owner.as_str()"),
            "{helper_name} must persist already-current branch-closure repair events with caller-owned command identity"
        );
        assert!(
            !helper_body.contains("\"record_branch_closure\""),
            "{helper_name} must not hard-code hidden primitive command identity"
        );
    }
}

#[test]
fn close_current_task_propagates_worktree_lease_cleanup_failures() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let close_source =
        fs::read_to_string(repo_root.join("src/execution/commands/close_current_task.rs"))
            .expect("close_current_task source should be readable");
    let close_impl = rust_function_body(&close_source, "close_current_task")
        .expect("close_current_task should exist");
    let _cleanup_helper = rust_function_body(
        &close_source,
        "release_resolved_worktree_leases_after_current_task_closure",
    )
    .expect("close-current-task cleanup helper should exist");

    assert!(
        !close_source
            .contains("let _ = release_worktree_leases_for_current_task_closures_and_persist"),
        "close-current-task must not silently discard worktree lease cleanup failures"
    );
    assert!(
        close_impl.contains("release_resolved_worktree_leases_after_current_task_closure")
            && close_impl.contains(")?;"),
        "close-current-task must propagate cleanup helper failures before reporting clean success"
    );
    assert!(
        close_source.contains(") -> Result<(), JsonFailure>")
            && close_source.contains("worktree lease cleanup failed")
            && close_source.contains("task closure remains authoritative"),
        "cleanup failure diagnostics must state that closure authority is known while cleanup failed"
    );
}

#[test]
fn gate_and_stale_decisioning_do_not_split_after_reducer() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    for relative in FORBIDDEN_FILES {
        let path = repo_root.join(relative);
        let source = fs::read_to_string(&path).expect("runtime source should be readable");
        let excluded_functions = if *relative == "src/execution/state.rs" {
            STATE_DIRECT_GATE_COMMAND_BODIES
        } else {
            &[]
        };
        violations.extend(rust_source_scan::forbidden_call_violations(
            relative,
            &source,
            FORBIDDEN_GATE_CALLS,
            excluded_functions,
        ));
    }
    for relative in FORBIDDEN_STALE_RECOMPUTATION_FILES {
        let path = repo_root.join(relative);
        let source = fs::read_to_string(&path).expect("runtime source should be readable");
        violations.extend(rust_source_scan::forbidden_call_violations(
            relative,
            &source,
            FORBIDDEN_STALE_RECOMPUTATION_CALLS,
            &[],
        ));
    }
    for relative in FORBIDDEN_STALE_TARGET_RECOMPUTATION_FILES {
        let path = repo_root.join(relative);
        let source = fs::read_to_string(&path).expect("runtime source should be readable");
        violations.extend(rust_source_scan::forbidden_call_violations(
            relative,
            &source,
            FORBIDDEN_STALE_TARGET_RECOMPUTATION_CALLS,
            &[],
        ));
    }
    for (relative, function_name) in FORBIDDEN_STALE_TARGET_RECOMPUTATION_FUNCTIONS {
        let path = repo_root.join(relative);
        let source = fs::read_to_string(&path).expect("runtime source should be readable");
        violations.extend(rust_source_scan::forbidden_call_violations_in_function(
            relative,
            &source,
            function_name,
            FORBIDDEN_STALE_TARGET_RECOMPUTATION_CALLS,
        ));
    }
    for (relative, forbidden_pattern) in FORBIDDEN_STALE_TARGET_FABRICATION_PATTERNS {
        let path = repo_root.join(relative);
        let source = fs::read_to_string(&path).expect("runtime source should be readable");
        if source.contains(forbidden_pattern) {
            violations.push(format!(
                "{relative} fabricates a stale target with `{forbidden_pattern}`"
            ));
        }
    }
    let repair_target_selection =
        fs::read_to_string(repo_root.join("src/execution/repair_target_selection.rs"))
            .expect("repair_target_selection source should be readable");
    let execution_reentry_target =
        rust_function_body(&repair_target_selection, "execution_reentry_target")
            .expect("repair_target_selection should contain `execution_reentry_target`");
    if execution_reentry_target.contains("ExecutionReentryTargetSource::ClosureGraphStaleTarget")
        && execution_reentry_target.contains("status.blocking_task")
    {
        violations.push(String::from(
            "src/execution/repair_target_selection.rs::execution_reentry_target fabricates closure-graph stale targets from status.blocking_task",
        ));
    }
    assert!(
        violations.is_empty(),
        "gate/stale truth must flow from reducer output, not direct gate recomputation:\n{}",
        violations.join("\n")
    );
}

#[test]
fn target_bound_repair_follow_up_bindings_cover_task_scoped_kinds() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repair_route_decision =
        fs::read_to_string(repo_root.join("src/execution/repair_route_decision.rs"))
            .expect("repair_route_decision source should be readable");
    let target_binding =
        rust_function_body(&repair_route_decision, "repair_follow_up_target_binding")
            .expect("shared repair route decision should contain repair_follow_up_target_binding");
    assert!(
        target_binding.contains("Some(RepairFollowUpKind::CloseTask)")
            && target_binding.contains("close_task_repair_follow_up_target"),
        "close-task repair follow-ups must bind a task target through the shared close-task helper"
    );
    let close_task_target =
        rust_function_body(&repair_route_decision, "close_task_repair_follow_up_target").expect(
            "shared repair route decision should contain close_task_repair_follow_up_target",
        );
    assert!(
        close_task_target.contains("repair_plan\n        .target_task")
            && close_task_target.contains("repair_plan.post_route_task")
            && close_task_target.contains("repair_plan.post_route_blocking_task"),
        "close-task repair follow-up binding must derive a deterministic task target from repair-plan routing state"
    );
}

fn rust_function_body<'a>(source: &'a str, function_name: &str) -> Option<&'a str> {
    let signature_start = source.find(&format!("fn {function_name}("))?;
    let open_brace_offset = source[signature_start..].find('{')?;
    let open_brace = signature_start + open_brace_offset;
    let close_brace = matching_close_brace(source, open_brace)?;
    source.get(open_brace..=close_brace)
}

fn matching_close_brace(source: &str, open_brace: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in source.as_bytes().iter().enumerate().skip(open_brace) {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}
