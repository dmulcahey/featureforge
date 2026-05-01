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
        "status.execution_reentry_target_source.as_deref() == Some(\"closure_graph_stale_target\")",
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

const PUBLIC_COMMAND_BOUNDARY_FORBIDDEN_TEST_HELPER_PATTERNS: &[(&str, &[&str])] = &[
    (
        "tests/support/workflow_direct.rs",
        &[
            "LegacyWorkflowCli",
            "LegacyWorkflowCommand",
            "allow_legacy_removed_commands",
            "WorkflowPlanFidelityCli",
            "record_plan_fidelity_receipt_with_state_dir",
        ],
    ),
    (
        "tests/support/plan_execution_direct.rs",
        &[
            "run_runtime_",
            "run_internal_",
            "run_record_plan_fidelity",
            "record_plan_fidelity_receipt_with_state_dir",
        ],
    ),
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

const ROUTING_AUTHORITY_RECEIPT_FREE_FILES: &[&str] = &[
    "src/execution/command_eligibility.rs",
    "src/execution/current_truth.rs",
    "src/execution/next_action.rs",
    "src/execution/query.rs",
    "src/execution/router.rs",
    "src/workflow/operator.rs",
    "src/workflow/status.rs",
];

#[test]
fn public_command_boundary_test_helpers_do_not_expose_removed_or_hidden_workflow_commands() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for (relative, forbidden_patterns) in PUBLIC_COMMAND_BOUNDARY_FORBIDDEN_TEST_HELPER_PATTERNS {
        let source = fs::read_to_string(repo_root.join(relative))
            .unwrap_or_else(|error| panic!("{relative} should be readable: {error}"));
        for forbidden_pattern in *forbidden_patterns {
            if source.contains(forbidden_pattern) {
                violations.push(format!("{relative} contains `{forbidden_pattern}`"));
            }
        }
    }

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
fn production_routing_authority_uses_artifacts_not_receipts() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for relative in ROUTING_AUTHORITY_RECEIPT_FREE_FILES {
        let source = fs::read_to_string(repo_root.join(relative))
            .unwrap_or_else(|error| panic!("{relative} should be readable: {error}"));
        if source.to_ascii_lowercase().contains("receipt") {
            violations.push(format!("{relative} contains receipt terminology"));
        }
    }

    assert!(
        violations.is_empty(),
        "public routing authority must not depend on receipt-shaped runtime contracts:\n{}",
        violations.join("\n")
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
    let next_action = fs::read_to_string(repo_root.join("src/execution/next_action.rs"))
        .expect("next_action source should be readable");
    let execution_reentry_target = rust_function_body(&next_action, "execution_reentry_target")
        .expect("next_action should contain `execution_reentry_target`");
    if execution_reentry_target.contains("ExecutionReentryTargetSource::ClosureGraphStaleTarget")
        && execution_reentry_target.contains("status.blocking_task")
    {
        violations.push(String::from(
            "src/execution/next_action.rs::execution_reentry_target fabricates closure-graph stale targets from status.blocking_task",
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
    let review_state = fs::read_to_string(repo_root.join("src/execution/review_state.rs"))
        .expect("review_state source should be readable");
    let target_binding = rust_function_body(&review_state, "repair_follow_up_target_binding")
        .expect("review_state should contain repair_follow_up_target_binding");
    assert!(
        target_binding.contains("Some(\"record_task_closure\")")
            && target_binding.contains("close_task_repair_follow_up_target"),
        "record_task_closure follow-ups must bind a task target through the shared close-task helper"
    );
    let close_task_target = rust_function_body(&review_state, "close_task_repair_follow_up_target")
        .expect("review_state should contain close_task_repair_follow_up_target");
    assert!(
        close_task_target.contains("repair_plan\n        .target_task")
            && close_task_target.contains("post_repair_route_action.task_number")
            && close_task_target.contains("post_repair_route_action.blocking_task"),
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
