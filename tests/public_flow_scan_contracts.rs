#[path = "support/public_flow_scan.rs"]
mod public_flow_scan;
#[path = "support/rust_source_scan.rs"]
mod rust_source_scan;

use public_flow_scan::{
    INTERNAL_RUNTIME_HELPER_HEADER, PublicFlowExceptionCategory, PublicRuntimeFlowGateCategory,
    diagnostic_pattern_violations_for_source, event_log_test_api_exception,
    explicit_internal_helper_scope_exception, hidden_literal,
    internal_semantic_non_public_flow_category, is_protected_public_flow_file,
    low_level_late_stage_recorder_tokens, protected_public_flow_test_files_from_contract,
    public_command_boundary_forbidden_test_helper_violations_for_source,
    public_diagnostic_forbidden_patterns, public_flow_hidden_command_or_flag_literals,
    public_flow_scanner_contract_exception, public_runtime_flow_gate_category,
    public_runtime_flow_gate_entries, public_runtime_flow_required_test_binaries,
    public_runtime_flow_test_binaries_from_script, public_runtime_flow_test_files, read_repo_file,
    scan_source_for_public_flow_violations, scan_stale_dispatch_public_flow_violations,
    token_only_blocked_follow_up_violations,
};

use featureforge::execution::command_eligibility::hidden_command_or_flag_tokens;

fn assert_has_violation(violations: &[String], needle: &str) {
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(needle)),
        "expected violation containing `{needle}`, got {violations:#?}"
    );
}

#[test]
fn scanner_rejects_hidden_helper_calls_and_hidden_command_args_in_public_flow_tests() {
    // Historical failure class: public-flow tests passed through direct helper
    // machinery or hidden/debug command literals that shipped CLI users could not run.
    let helper_call = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let hidden_command = hidden_literal(&["record", "-review-dispatch"]);
    let hidden_flag = hidden_literal(&["--dispatch", "-id"]);
    let fixture = format!(
        r#"
#[test]
fn public_flow_uses_hidden_surface() {{
    {helper_call}
    let _ = run_featureforge(repo, state, &["{hidden_command}", "{hidden_flag}"], context);
}}
"#
    );

    let violations = scan_source_for_public_flow_violations("tests/workflow_runtime.rs", &fixture);

    assert_has_violation(&violations, "plan_execution_output_direct(");
    assert_has_violation(&violations, &hidden_command);
    assert_has_violation(&violations, &hidden_flag);
}

#[test]
fn scanner_hidden_command_vocabulary_covers_runtime_and_low_level_tokens() {
    // Historical failure class: hidden/debug compatibility commands leaked back
    // into public flow tests and public route fixtures under slightly different
    // vocabularies.
    let hidden_literals = public_flow_hidden_command_or_flag_literals();
    for token in hidden_command_or_flag_tokens() {
        assert!(
            hidden_literals.iter().any(|literal| literal == token),
            "public-flow hidden-command checks must consume runtime hidden token `{token}`"
        );
    }
    for token in low_level_late_stage_recorder_tokens() {
        assert!(
            hidden_literals.iter().any(|literal| literal == token),
            "public-flow hidden-command checks must include low-level recorder token `{token}`"
        );
    }

    let scanner_hidden = low_level_late_stage_recorder_tokens()[0];
    let scanner_fixture = format!(
        r#"
        #[test]
        fn public_flow_hidden_command_regression() {{
            let command = "{scanner_hidden}";
            assert!(!command.is_empty());
        }}
        "#
    );
    let scanner_violations =
        scan_source_for_public_flow_violations("tests/workflow_runtime.rs", &scanner_fixture);

    assert_has_violation(&scanner_violations, scanner_hidden);
}

#[test]
fn scanner_rejects_internal_quarantine_header_inside_public_flow_tests() {
    // Historical failure class: public-flow tests hid helper-only coverage behind
    // the internal quarantine marker.
    let fixture = format!("{INTERNAL_RUNTIME_HELPER_HEADER}\n\n#[test]\nfn public_flow() {{}}\n");

    let violations =
        scan_source_for_public_flow_violations("tests/runtime_behavior_golden.rs", &fixture);

    assert_has_violation(&violations, "internal helper quarantine header");
}

#[test]
fn scanner_quarantines_public_runtime_contract_runner_from_executable_public_flow() {
    // Historical failure class: focused route-contract tests moved off real
    // subprocesses for speed, but executable public-flow proof must not quietly
    // inherit that in-process runner.
    let fixture = r#"
#[path = "support/public_runtime_contract_runner.rs"]
mod public_runtime_contract_runner;

#[test]
fn public_flow_uses_in_process_contract_runner() {}
"#;

    let executable_violations =
        scan_source_for_public_flow_violations("tests/public_replay_churn.rs", fixture);
    assert_has_violation(
        &executable_violations,
        "support/public_runtime_contract_runner.rs",
    );

    let focused_contract_violations =
        scan_source_for_public_flow_violations("tests/runtime_behavior_golden.rs", fixture);
    assert!(
        focused_contract_violations.is_empty(),
        "runtime_behavior_golden is the focused route-contract capture allowed to use the in-process public argv/parser runner, got {focused_contract_violations:#?}"
    );
}

#[test]
fn scanner_protects_mixed_final_review_boundary_suite() {
    // Historical failure class: mixed final-review files contain compiled CLI
    // assertions as well as parser coverage, so the public sections still need
    // hidden-helper and display-command protection.
    let rel = "tests/plan_execution_final_review.rs";
    assert!(
        is_protected_public_flow_file(rel),
        "{rel} mixes final-review receipt/parser coverage with compiled-CLI route assertions, so the public-flow scanner must still protect it from hidden helpers and display-command execution"
    );
}

#[test]
fn scanner_excludes_liveness_model_from_public_flow_proof() {
    // Historical failure class: in-process semantic liveness coverage was
    // mistaken for shipped-runtime public-flow proof.
    let rel = "tests/liveness_model_checker.rs";
    assert_eq!(
        internal_semantic_non_public_flow_category(rel),
        Some(PublicFlowExceptionCategory::InternalSemanticComparison),
        "liveness model checker should have a stable internal semantic exclusion category"
    );
    assert!(
        !is_protected_public_flow_file(rel),
        "{rel} must stay outside the protected public-flow set because it uses in-process semantic helpers by design"
    );
    assert!(
        !public_runtime_flow_test_files().contains(rel),
        "{rel} must not be selected by the public runtime flow gate"
    );

    let public_script = read_repo_file("scripts/run-public-runtime-flow-tests.sh");
    let selected_binaries = public_runtime_flow_test_binaries_from_script(&public_script);
    assert!(
        !selected_binaries
            .iter()
            .any(|binary| binary == "liveness_model_checker"),
        "public runtime flow gate must not cite liveness_model_checker as public-flow proof: {selected_binaries:?}"
    );
}

#[test]
fn public_runtime_flow_script_matches_classified_gate_contract() {
    // Historical failure class: the named public-flow script passed while
    // omitting compiled-CLI suites that the scanner treated as protected public
    // surfaces.
    let public_script = read_repo_file("scripts/run-public-runtime-flow-tests.sh");
    let selected_binaries = public_runtime_flow_test_binaries_from_script(&public_script);
    let expected_binaries = public_runtime_flow_required_test_binaries();
    assert_eq!(
        selected_binaries, expected_binaries,
        "run-public-runtime-flow-tests.sh must stay aligned with the classified public-flow gate"
    );

    for binary in &selected_binaries {
        let category = public_runtime_flow_gate_category(binary)
            .unwrap_or_else(|| panic!("{binary} should have a public-flow gate category"));
        let rel = format!("tests/{binary}.rs");
        assert!(
            is_protected_public_flow_file(&rel),
            "{rel} is in the public-flow script as `{}` and must be protected by the scanner",
            category.as_str()
        );
    }
}

#[test]
fn public_runtime_flow_script_parser_ignores_comments_and_non_nextest_invocations() {
    // Historical failure class: the script/scanner alignment test trusted
    // whitespace scanning and could be satisfied by commented or echoed
    // `--test` text that the public-flow gate never executed.
    let script = r#"
#!/usr/bin/env bash
# cargo nextest run --test comment_only_public_flow
echo cargo nextest run --test echoed_public_flow
cargo test --test cargo_test_not_public_gate
IGNORED=value cargo nextest run \
  --test workflow_shell_smoke \
  --test=public_replay_churn \
  # --test inline_comment_public_flow
  --all-features
"#;

    assert_eq!(
        public_runtime_flow_test_binaries_from_script(script),
        vec!["public_replay_churn", "workflow_shell_smoke"],
        "script parsing should count only active cargo nextest run --test selectors"
    );
}

#[test]
fn public_runtime_flow_gate_classification_keeps_executable_and_static_phases_explicit() {
    let mut executable = Vec::new();
    let mut mixed = Vec::new();
    let mut focused_contract = Vec::new();
    let mut static_guard = Vec::new();
    for entry in public_runtime_flow_gate_entries() {
        match entry.category {
            PublicRuntimeFlowGateCategory::ExecutablePublicFlowProof => {
                executable.push(entry.binary);
            }
            PublicRuntimeFlowGateCategory::MixedPublicAndInternalSemantic => {
                mixed.push(entry.binary);
            }
            PublicRuntimeFlowGateCategory::FocusedPublicContract => {
                focused_contract.push(entry.binary);
            }
            PublicRuntimeFlowGateCategory::StaticPublicGuard => {
                static_guard.push(entry.binary);
            }
        }
    }

    for required in [
        "public_replay_churn",
        "workflow_shell_smoke",
        "workflow_runtime_final_review",
    ] {
        assert!(
            executable.contains(&required),
            "{required} should remain classified as executable shipped-runtime public-flow proof"
        );
    }
    for required in [
        "workflow_runtime",
        "workflow_entry_shell_smoke",
        "plan_execution",
        "contracts_execution_runtime_boundaries",
        "execution_query",
    ] {
        assert!(
            mixed.contains(&required),
            "{required} should be explicitly classified as mixed public-flow and internal semantic coverage, not pure public-flow proof"
        );
    }
    assert!(
        focused_contract.contains(&"runtime_behavior_golden"),
        "runtime_behavior_golden should stay classified as public contract capture, not transition proof"
    );
    assert!(
        static_guard.contains(&"public_cli_flow_contracts"),
        "static public guards should stay explicit instead of being mislabeled as transition proof"
    );
    assert_eq!(
        PublicRuntimeFlowGateCategory::MixedPublicAndInternalSemantic.as_str(),
        "mixed_public_and_internal_semantic"
    );
}

#[test]
fn public_runtime_flow_gate_manifest_has_complete_typed_metadata() {
    let entries = public_runtime_flow_gate_entries();
    assert!(
        !entries.is_empty(),
        "public runtime-flow gate manifest should have explicit entries"
    );

    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        assert!(
            !entry.binary.trim().is_empty(),
            "public runtime-flow gate entry should name a test binary: {entry:?}"
        );
        assert!(
            !entry.proof_scope.trim().is_empty(),
            "public runtime-flow gate entry `{}` should document its proof scope",
            entry.binary
        );
        assert!(
            PublicRuntimeFlowGateCategory::ALL.contains(&entry.category),
            "public runtime-flow gate entry `{}` should use a supported category",
            entry.binary
        );
        assert!(
            seen.insert(entry.binary),
            "public runtime-flow gate manifest should not duplicate `{}`",
            entry.binary
        );

        let rel = format!("tests/{}.rs", entry.binary);
        assert!(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(&rel)
                .exists(),
            "public runtime-flow gate entry `{}` should point at an existing test file",
            entry.binary
        );
    }
}

#[test]
fn protected_public_flow_files_are_contract_derived_not_scanner_self_tests() {
    let protected = protected_public_flow_test_files_from_contract();
    for entry in public_runtime_flow_gate_entries() {
        let rel = format!("tests/{}.rs", entry.binary);
        assert!(
            protected.contains(&rel),
            "{rel} should be protected through the classified public-flow contract"
        );
    }
    assert!(
        !protected.contains("tests/public_flow_scan_contracts.rs"),
        "scanner self-tests should validate the gate but not become public-flow proof"
    );
    assert!(
        !public_runtime_flow_test_files().contains("tests/public_flow_scan_contracts.rs"),
        "scanner self-tests should not be selected by the public runtime flow script"
    );
}

#[test]
fn scanner_self_tests_have_typed_non_public_category() {
    let rel = "tests/public_flow_scan_contracts.rs";
    let exception = public_flow_scanner_contract_exception(rel)
        .expect("scanner self-test should have a typed non-public category");
    assert_eq!(
        exception.category,
        PublicFlowExceptionCategory::ScannerSelfTest
    );
    assert!(
        !is_protected_public_flow_file(rel),
        "{rel} must stay outside production public-flow proof"
    );
}

#[test]
fn scanner_exception_markers_choose_typed_categories_at_entry_points() {
    let focused_setup = explicit_internal_helper_scope_exception(
        "tests/workflow_shell_smoke.rs",
        "setup_qa_pending_case_slow",
    )
    .expect("fixture setup exception should be registered");
    assert_eq!(
        focused_setup.category,
        PublicFlowExceptionCategory::FocusedContractCoverage
    );
    assert!(
        explicit_internal_helper_scope_exception(
            "tests/workflow_shell_smoke.rs",
            "setup_qa_pending_case"
        )
        .is_none(),
        "focused public-flow setup exceptions require the explicit setup_*_slow marker"
    );

    let removed_command_rejection = explicit_internal_helper_scope_exception(
        "tests/workflow_shell_smoke.rs",
        "removed_command_rejection_workflow_record_pivot_operator_routes_publicly",
    )
    .expect("removed command rejection prefix should be registered");
    assert_eq!(
        removed_command_rejection.category,
        PublicFlowExceptionCategory::RemovedCommandRejection
    );
    assert!(
        explicit_internal_helper_scope_exception(
            "tests/workflow_shell_smoke.rs",
            "workflow_record_pivot_command_is_removed_and_operator_routes_publicly"
        )
        .is_none(),
        "removed-command exceptions should use the explicit removed_command_rejection_ prefix rather than exact test-name allowlists"
    );

    let internal_semantic = explicit_internal_helper_scope_exception(
        "tests/execution_query.rs",
        "internal_semantic_routing_snapshot_matches_workflow_operator_output_before_execution_starts",
    )
    .expect("internal semantic exception should be registered");
    assert_eq!(
        internal_semantic.category,
        PublicFlowExceptionCategory::InternalSemanticComparison
    );
    let doctor_source_shape = explicit_internal_helper_scope_exception(
        "tests/workflow_entry_shell_smoke.rs",
        "internal_semantic_fs17_doctor_public_entrypoints_keep_single_context_build_path",
    )
    .expect("doctor source-shape exception should use the internal_semantic_ marker");
    assert_eq!(
        doctor_source_shape.category,
        PublicFlowExceptionCategory::InternalSemanticComparison
    );
    assert!(
        explicit_internal_helper_scope_exception(
            "tests/execution_query.rs",
            "routing_snapshot_matches_workflow_operator_output_before_execution_starts",
        )
        .is_none(),
        "internal semantic direct-helper exceptions require the explicit internal_semantic_ marker"
    );
    assert!(
        explicit_internal_helper_scope_exception(
            "tests/workflow_entry_shell_smoke.rs",
            "fs17_doctor_public_entrypoints_keep_single_context_build_path"
        )
        .is_none(),
        "doctor source-shape exceptions must use the explicit internal_semantic_ marker rather than an exact test-name allowlist"
    );

    let synthetic_fixture = event_log_test_api_exception(
        "tests/workflow_runtime.rs",
        "synthetic_update_authoritative_harness_state",
    )
    .expect("synthetic event-log fixture exception should be registered");
    assert_eq!(
        synthetic_fixture.category,
        PublicFlowExceptionCategory::SyntheticFixtureSetup
    );
    assert!(
        event_log_test_api_exception(
            "tests/workflow_runtime.rs",
            "update_authoritative_harness_state",
        )
        .is_none(),
        "event-log authority exceptions require the explicit synthetic_ fixture marker"
    );
}

#[test]
fn public_command_boundary_helper_scanner_is_symbol_based_not_phrase_based() {
    // Historical failure class: support helpers regained retired public-command
    // wrappers. The guard should catch real symbols/imports/calls, but should
    // not fail on comments or explanatory strings that mention old names.
    let comments_only = r#"
// Historical note: LegacyWorkflowCli and run_runtime_plan were removed.
const NOTE: &str = "record_plan_fidelity_receipt_with_state_dir is retired";
"#;
    let comments_only_violations =
        public_command_boundary_forbidden_test_helper_violations_for_source(
            "tests/support/workflow_direct.rs",
            comments_only,
        );
    assert!(
        comments_only_violations.is_empty(),
        "comment/string mentions should not trip helper-boundary scanner: {comments_only_violations:#?}"
    );

    let workflow_symbols = r#"
use removed::LegacyWorkflowCli;
struct LegacyWorkflowCommand;
struct r#WorkflowPlanFidelityCli;
type WorkflowCliAlias = removed::LegacyWorkflowCli;
fn returns_removed_helper() -> removed::LegacyWorkflowCli {
    removed::LegacyWorkflowCli::new()
}
fn returns_raw_removed_helper() -> removed::r#LegacyWorkflowCli {
    removed::r#LegacyWorkflowCli::new()
}
#[cfg(test)]
fn cfg_test_returns_removed_helper() -> removed::LegacyWorkflowCli {
    removed::LegacyWorkflowCli::new()
}
fn allow_legacy_removed_commands() {}
"#;
    let workflow_violations = public_command_boundary_forbidden_test_helper_violations_for_source(
        "tests/support/workflow_direct.rs",
        workflow_symbols,
    );
    assert_has_violation(&workflow_violations, "LegacyWorkflowCli");
    assert_has_violation(&workflow_violations, "LegacyWorkflowCommand");
    assert_has_violation(&workflow_violations, "allow_legacy_removed_commands");

    let plan_execution_symbols = r#"
fn run_runtime_status_json() {}
fn run_record_plan_fidelity() {}
fn call_removed() {
    run_runtime_status_json();
    removed::run_internal_status_json();
}
fn r#run_runtime_raw_ident() {}
#[cfg(test)]
fn run_runtime_cfg_only() {}
"#;
    let plan_execution_violations =
        public_command_boundary_forbidden_test_helper_violations_for_source(
            "tests/support/plan_execution_direct.rs",
            plan_execution_symbols,
        );
    assert_has_violation(&plan_execution_violations, "run_runtime_");
    assert_has_violation(&plan_execution_violations, "run_internal_");
    assert_has_violation(&plan_execution_violations, "run_record_plan_fidelity");
}

#[test]
fn scanner_rejects_public_direct_runtime_surface_wrappers() {
    // Historical failure class: public tests wrapped in-process runtime query
    // helpers and bypassed the shipped CLI/operator surface.
    let status_marker = hidden_literal(&[".status", "("]);
    let operator_marker = hidden_literal(&["operator_for_", "runtime("]);
    let routing_query_marker = hidden_literal(&["query_workflow_", "routing_state_for_runtime("]);
    let fixture = format!(
        r#"
fn plan_execution_status_json(runtime: &ExecutionRuntime, args: &StatusArgs) {{
    runtime{status_marker}args);
}}

fn workflow_operator_json(runtime: &ExecutionRuntime, args: &OperatorArgs) {{
    operator::{operator_marker}runtime, args);
}}

fn workflow_query_json(runtime: &ExecutionRuntime, plan: &Path) {{
    execution::query::{routing_query_marker}runtime, Some(plan), false);
}}
"#
    );

    let violations =
        scan_source_for_public_flow_violations("tests/support/runtime_surfaces.rs", &fixture);

    assert_has_violation(&violations, "plan_execution_status_json");
    assert_has_violation(&violations, "workflow_operator_json");
    assert_has_violation(&violations, "workflow_query_json");
    assert_has_violation(&violations, "query_workflow_routing_state_for_runtime");
    assert_has_violation(&violations, "direct runtime surface");
}

#[test]
fn scanner_rejects_unregistered_event_log_authority_apis_in_public_replay_setup() {
    // Historical failure class: public replay fixtures mutated event authority
    // without a visible synthetic setup exception.
    let fixture = r#"
fn public_fixture(state_path: &Path, payload: &serde_json::Value) {
    featureforge::execution::event_log::load_reduced_authoritative_state_for_tests(state_path);
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(state_path, payload);
}
"#;

    let violations =
        scan_source_for_public_flow_violations("tests/public_replay_churn.rs", fixture);

    assert_has_violation(&violations, "load_reduced_authoritative_state_for_tests");
    assert_has_violation(&violations, "sync_fixture_event_log_for_tests");
    assert_has_violation(&violations, "synthetic fixture exception");
}

#[test]
fn scanner_allows_registered_synthetic_event_log_fixture_setup() {
    // Historical failure class: impossible legacy damage may be seeded
    // synthetically, but the exception must be explicit and narrow.
    let fixture = r#"
fn synthetic_historical_fixture_update_state_fields(
    state_path: &Path,
    payload: &serde_json::Value,
) {
    featureforge::execution::event_log::load_reduced_authoritative_state_for_tests(state_path);
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(state_path, payload);
}
"#;

    let violations =
        scan_source_for_public_flow_violations("tests/public_replay_churn.rs", fixture);

    assert!(
        violations.is_empty(),
        "registered synthetic fixture setup should be allowed, got {violations:#?}"
    );
}

#[test]
fn scanner_rejects_display_recommended_command_execution_in_public_flow_tests() {
    // Historical failure class: tests parsed display-only `recommended_command`
    // strings instead of executing typed public argv/templates.
    let fixture = r#"
fn public_flow_executes_display_summary(recommended_command: &str) {
    let _ = recommended_command.split_whitespace().collect::<Vec<_>>();
    let _ = shlex::split(recommended_command);
}

fn public_flow_aliases_display_summary(surface: &serde_json::Value) {
    let display = surface["recommended_command"].as_str().unwrap();
    let _ = shlex::split(display);
}
"#;

    let violations = scan_source_for_public_flow_violations("tests/workflow_runtime.rs", fixture);

    assert_has_violation(&violations, "recommended_command.split_whitespace()");
    assert_has_violation(&violations, "shlex::split(recommended_command)");
    assert_has_violation(&violations, "shlex::split(display)");
}

#[test]
fn stale_dispatch_scanner_rejects_hidden_dispatch_repair_paths_in_public_flow_tests() {
    // Historical failure class: stale-dispatch public-flow tests used hidden
    // dispatch repair flags instead of public close/repair routes.
    let dispatch_id = hidden_literal(&["--dispatch", "-id"]);
    let close_stale = hidden_literal(&["close-current-task", " --task 2"]);
    let fixture = format!(
        r#"
#[test]
fn public_close_current_task_records_positive_closure_after_stale_dispatch_lineage_without_dispatch_id() {{
    let _ = run_featureforge(repo, state, &["plan", "execution", "{close_stale}", "{dispatch_id}"], context);
}}
"#
    );

    let violations =
        scan_stale_dispatch_public_flow_violations("tests/workflow_shell_smoke.rs", &fixture);

    assert_has_violation(&violations, "stale-dispatch");
}

#[test]
fn token_only_blocked_output_scanner_catches_named_historical_shapes() {
    // Historical failure class: blocked outputs exposed only a follow-up token
    // with no public argv/template or required inputs, causing agents to loop.
    let cases = [
        (
            "public recovery contract",
            r#"
            PublicRecoveryContract {
                recommended_command: None,
                recommended_public_command_argv: None,
                recommended_public_command_template: None,
                required_inputs: Vec::new(),
                rederive_via_workflow_operator: None,
                required_follow_up: Some(String::from("repair_review_state")),
            }
            "#,
        ),
        (
            "repair review output",
            r#"
            RepairReviewStateOutput {
                action: String::from("blocked"),
                required_follow_up: Some(String::from("execution_reentry")),
                next_action: None,
                recommended_command: None,
                recommended_public_command_argv: None,
                recommended_public_command_template: None,
                required_inputs: Vec::new(),
            }
            "#,
        ),
    ];
    for (label, synthetic) in cases {
        let violations = token_only_blocked_follow_up_violations("synthetic.rs", synthetic);
        assert_eq!(
            violations.len(),
            1,
            "token-only scanner must catch the named historical {label} shape"
        );
    }
}

#[test]
fn production_diagnostic_scanner_rejects_hidden_helper_guidance_but_allows_comments() {
    // Historical failure class: public diagnostics told agents to run retired
    // hidden helpers such as gate-review.
    let forbidden_patterns = public_diagnostic_forbidden_patterns();
    let production_string = r#"
pub fn public_remediation() -> &'static str {
    "historical retry gate-review guidance must still fail"
}
"#;
    let block_comment = "/* historical:\n * retry gate-review was old wording\n */";

    let string_violations = diagnostic_pattern_violations_for_source(
        "src/execution/example.rs",
        production_string,
        &forbidden_patterns,
    );
    let comment_violations = diagnostic_pattern_violations_for_source(
        "src/execution/example.rs",
        block_comment,
        &forbidden_patterns,
    );

    assert_has_violation(&string_violations, "retry gate-review");
    assert!(
        comment_violations.is_empty(),
        "historical Rust comments should stay allowed, got {comment_violations:#?}"
    );
}
