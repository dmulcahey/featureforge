#[path = "support/rust_source_scan.rs"]
mod rust_source_scan;

use std::collections::HashSet;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const INTERNAL_RUNTIME_HELPER_HEADER: &str = "//! INTERNAL_RUNTIME_HELPER_TEST: this file intentionally exercises unavailable runtime internals.";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read_repo_file(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"))
}

fn hidden_literal(parts: &[&str]) -> String {
    parts.concat()
}

#[test]
fn typed_public_commands_are_route_authority_before_display_rendering() {
    let next_action = read_repo_file("src/execution/next_action.rs");
    assert!(
        !next_action.contains("recommended_command: Option<String>"),
        "next-action routing decisions must not carry string recommendations as authority"
    );
    assert!(
        next_action.contains("recommended_public_command: Option<PublicCommand>"),
        "next-action routing decisions should carry typed public command authority"
    );

    let router = read_repo_file("src/execution/router.rs");
    assert!(
        router.contains("pub(crate) struct PublicRouteDecision"),
        "public route decisions should be the single serialized public route contract"
    );
    assert!(
        router.contains("pub(crate) invocation: Option<PublicCommandInvocation>"),
        "route decisions should own the executable argv surface, not leave status/operator to re-render it"
    );
    assert!(
        router
            .matches("public_command_recommendation_surfaces(command)")
            .count()
            == 1,
        "router should derive display, argv, and required_inputs only through PublicRouteDecision::command_surfaces"
    );
    for forbidden in [
        "PublicCommand::parse_display_command",
        "public_mutation_request_from_command",
        "public_command_and_display",
        "public_command_from_recommended_command",
    ] {
        assert!(
            !router.contains(forbidden),
            "router must not reparse rendered public command strings via `{forbidden}`"
        );
    }

    let review_state = read_repo_file("src/execution/review_state.rs");
    for forbidden in [
        "public_mutation_request_from_command",
        "route_decision.recommended_command",
        "routing.recommended_command",
        "final_routing.recommended_command",
    ] {
        assert!(
            !review_state.contains(forbidden),
            "repair-state routing must not recover authority from rendered strings via `{forbidden}`"
        );
    }

    let operator_outputs = read_repo_file("src/execution/commands/common/operator_outputs.rs");
    for forbidden in [
        "recommended_command.starts_with(",
        "recommended_command.contains(",
        "operator.recommended_command.clone().filter",
    ] {
        assert!(
            !operator_outputs.contains(forbidden),
            "close-current-task follow-up routing must not recover authority from rendered strings via `{forbidden}`"
        );
    }
    assert!(
        operator_outputs.contains("operator.recommended_public_command"),
        "close-current-task follow-up routing should use typed public command authority"
    );

    let workflow_operator = read_repo_file("src/workflow/operator.rs");
    assert!(
        workflow_operator.contains("route_decision.public_command_argv()"),
        "workflow operator should project executable argv from the route decision contract"
    );
    for forbidden in [
        "recommended_public_command_argv(route_decision.recommended_public_command.as_ref())",
        "required_inputs_for_public_command(route_decision.recommended_public_command.as_ref())",
    ] {
        assert!(
            !workflow_operator.contains(forbidden),
            "workflow operator must not rebuild route command surfaces independently via `{forbidden}`"
        );
    }

    let read_model_route_projection =
        read_repo_file("src/execution/read_model/public_route_projection.rs");
    assert!(
        read_model_route_projection.contains("route_decision.public_command_argv()"),
        "plan execution status should project argv from the route decision contract"
    );
    assert!(
        !read_model_route_projection.contains(
            "recommended_public_command_argv(status.recommended_public_command.as_ref())"
        ),
        "plan execution status must not independently re-render executable argv from typed commands"
    );
    assert!(
        read_model_route_projection.contains(
            "read_scope.status.public_repair_targets = route_decision.public_repair_targets.clone()"
        ),
        "read-model public repair targets should be copied from the route decision contract"
    );
    for forbidden in [
        "project_persisted_public_repair_targets",
        "status.public_repair_targets.push",
        "push_public_repair_target_once(status",
    ] {
        assert!(
            !read_model_route_projection.contains(forbidden),
            "read-model public repair targets must not independently derive route authority via `{forbidden}`"
        );
    }

    let route_target_start = router
        .find("fn public_repair_targets_from_route_decision")
        .expect("router should derive public repair targets from the route decision contract");
    let route_target_end = router[route_target_start..]
        .find("\nfn project_routing_from_runtime_state")
        .map(|offset| route_target_start + offset)
        .expect("router public repair target slice should have a stable following helper");
    let route_target_projection = &router[route_target_start..route_target_end];
    for forbidden in [
        "recommended_command",
        "next_public_action",
        ".starts_with(",
        ".contains(",
    ] {
        assert!(
            !route_target_projection.contains(forbidden),
            "read-model public repair targets must use typed command authority, not rendered route text via `{forbidden}`"
        );
    }
    assert!(
        route_target_projection.contains(
            "route_recommended_public_command_is(route_decision, PublicCommandKind::RepairReviewState)"
        ) && route_target_projection
            .contains("route_recommended_public_command_is(route_decision, PublicCommandKind::AdvanceLateStage)"),
        "route repair target projection should classify typed PublicCommand variants"
    );
    assert!(
        !route_target_projection.contains("phase::DETAIL_FINAL_REVIEW_OUTCOME_PENDING"),
        "waiting final-review routes must not synthesize local public repair targets"
    );

    let transfer = read_repo_file("src/execution/commands/transfer.rs");
    assert!(
        transfer.contains("recommended_public_command_argv"),
        "transfer output should expose argv with any follow-up command"
    );
    assert!(
        transfer.contains("required_inputs"),
        "transfer output should expose typed required_inputs when a follow-up argv is absent"
    );
    assert!(
        transfer.contains("PublicCommand::TransferHandoff"),
        "transfer scope reroutes should derive display and argv from typed PublicCommand::TransferHandoff"
    );
    assert!(
        !transfer.contains(
            "format!(\n                    \"featureforge plan execution transfer --plan"
        ),
        "transfer must not hand-build public reroute command strings"
    );

    let eligibility = read_repo_file("src/execution/command_eligibility.rs");
    assert!(
        eligibility.contains("input_enum(\"scope\", [\"task\", \"branch\"])"),
        "unresolved handoff scope should be modeled as a typed enum input"
    );
    let route_guard_start = eligibility
        .find("fn route_exposes_public_mutation_request")
        .expect("typed route mutation guard should exist");
    let route_guard = &eligibility[route_guard_start
        ..eligibility[route_guard_start..]
            .find("\nfn public_repair_target_matches_request")
            .map(|offset| route_guard_start + offset)
            .expect("typed route mutation guard should have a stable following helper")];
    assert!(
        route_guard.contains("recommended_public_command"),
        "mutation guards should compare against the typed public route command"
    );
    assert!(
        !route_guard.contains("next_public_action"),
        "mutation guards must not fall back to parsing rendered next_public_action strings"
    );
    assert!(
        eligibility.contains("fn public_transfer_scope_matches"),
        "mutation guards should satisfy unresolved handoff scope from typed required-input values"
    );
}

#[test]
fn display_command_parsing_is_test_only_not_route_authority() {
    let command_eligibility = read_repo_file("src/execution/command_eligibility.rs");
    assert!(
        command_eligibility.contains("#[cfg(test)]\n    pub(crate) fn parse_display_command"),
        "display-command parsing should be compiled only for public-command boundary tests"
    );

    let mut violations = Vec::new();
    for file in production_command_authority_files() {
        let rel = repo_relative(&file);
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", file.display()));
        for call in rust_source_scan::normalized_call_path_hits(&rel, &source, &[]) {
            if call.raw_path == "PublicCommand::parse_display_command"
                || call
                    .path
                    .ends_with("::PublicCommand::parse_display_command")
            {
                violations.push(format!(
                    "{rel}:{} calls `{}` as production command authority",
                    call.line, call.raw_path
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "production routing, mutation, and read-model code must not parse display command strings:\n{}",
        violations.join("\n")
    );
}

#[test]
fn public_blocked_outputs_do_not_emit_token_only_follow_ups() {
    let files = [
        "src/execution/commands/advance_late_stage.rs",
        "src/execution/commands/close_current_task.rs",
        "src/execution/commands/common/branch_closure_truth.rs",
        "src/execution/commands/common/late_stage_reruns.rs",
        "src/execution/commands/common/operator_outputs.rs",
        "src/execution/review_state.rs",
    ];
    let token_only_pattern = concat!(
        "recommended_command: None,\n",
        "recommended_public_command_argv: None,\n",
        "required_inputs: Vec::new(),\n",
        "rederive_via_workflow_operator: None,\n",
        "required_follow_up: Some"
    );
    let mut violations = Vec::new();
    for rel in files {
        let source = read_repo_file(rel);
        let normalized = source.lines().map(str::trim).collect::<Vec<_>>().join("\n");
        for (start, _) in normalized.match_indices(token_only_pattern) {
            violations.push(format!(
                "{rel}:{} emits required_follow_up without argv, inputs, requery, or diagnostic-only null follow-up",
                line_number_for_byte(&normalized, start)
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "normal public blocked outputs must not strand agents on token-only follow-ups:\n{}",
        violations.join("\n")
    );

    let operator_outputs = read_repo_file("src/execution/commands/common/operator_outputs.rs");
    for required in [
        "pub(crate) fn public_recovery_contract_for_follow_up",
        "workflow_operator_requery_optional_surfaces(plan, external_review_result_ready)",
        "required_inputs_for_follow_up_profile",
        "PublicCommand::WorkflowOperator",
        "json: true",
    ] {
        assert!(
            operator_outputs.contains(required),
            "public follow-up recovery should stay centralized through `{required}`"
        );
    }
    for forbidden in [
        "\"featureforge workflow operator --plan {}",
        "String::from(\"workflow\"),\n        String::from(\"operator\")",
    ] {
        assert!(
            !operator_outputs.contains(forbidden),
            "workflow-operator JSON requery surfaces must use typed PublicCommand authority, not hand-built command fragments via `{forbidden}`"
        );
    }
}

#[test]
fn public_text_and_schemas_mark_recommended_command_as_display_only() {
    let workflow_operator = read_repo_file("src/workflow/operator.rs");
    for required in [
        "Display command summary:",
        "Next public action display summary:",
        "next_display_summary=",
        "Use JSON recommended_public_command_argv for execution",
    ] {
        assert!(
            workflow_operator.contains(required),
            "workflow text renderers should include `{required}`"
        );
    }
    assert!(
        !workflow_operator.contains("Recommended command:"),
        "workflow text renderers must not label display strings as recommended executable commands"
    );
    for forbidden in ["Next public action: {}", "next={}"] {
        assert!(
            !workflow_operator.contains(forbidden),
            "workflow text renderers must not emit command-shaped action text without display-only labeling via `{forbidden}`"
        );
    }

    for rel in [
        "schemas/plan-execution-status.schema.json",
        "schemas/workflow-operator.schema.json",
    ] {
        let schema = read_repo_file(rel);
        for required in [
            "Display-only compatibility summary; do not parse or execute this string.",
            "Executable public command argv authority when present.",
            "Parseable input contract for the routed public command",
        ] {
            assert!(
                schema.contains(required),
                "{rel} should document public command authority via `{required}`"
            );
        }
    }
}

fn concat_source_expr(parts: &[&str]) -> String {
    let quoted_parts = parts
        .iter()
        .map(|part| format!("{part:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("concat!({quoted_parts})")
}

#[test]
fn internal_plan_execution_helpers_are_explicitly_quarantined() {
    let source = read_repo_file("tests/support/plan_execution_direct.rs");

    assert!(
        source.starts_with(INTERNAL_RUNTIME_HELPER_HEADER),
        "plan_execution_direct.rs must start with the internal-only quarantine contract"
    );
    assert!(
        !source.contains("pub fn internal_test_"),
        "plan_execution_direct.rs must not expose ambiguous internal_test_* helpers"
    );
    assert!(
        source.contains("pub fn internal_only_"),
        "plan_execution_direct.rs should keep internal helpers visibly prefixed"
    );

    let source = read_repo_file("tests/support/internal_runtime_direct.rs");
    assert!(
        source.starts_with(INTERNAL_RUNTIME_HELPER_HEADER),
        "internal_runtime_direct.rs must start with the internal-only quarantine contract"
    );
    assert!(
        !source.contains("pub fn run_featureforge_real_cli")
            && !source.contains("pub fn run_public_featureforge_cli_json")
            && !source.contains("pub fn run_featureforge_with_env_control_real_cli"),
        "internal_runtime_direct.rs must not expose public compiled-CLI helpers"
    );
    assert!(
        source.contains("pub fn internal_only_"),
        "internal_runtime_direct.rs should keep direct-runtime helpers visibly prefixed"
    );
}

#[test]
fn public_cli_json_helper_uses_the_compiled_binary_only() {
    let source = read_repo_file("tests/support/public_featureforge_cli.rs");
    assert!(
        !source.starts_with(INTERNAL_RUNTIME_HELPER_HEADER),
        "public_featureforge_cli.rs must not use the internal helper quarantine header"
    );
    for forbidden in [
        "support/featureforge.rs",
        "support/plan_execution_direct.rs",
        "support/workflow_direct.rs",
        "support/internal_runtime_direct.rs",
        "featureforge::execution",
        "featureforge::workflow",
        "ExecutionRuntime::discover",
        "execution::mutate",
        "workflow::operator",
    ] {
        assert!(
            !source.contains(forbidden),
            "public CLI helper file must not import or call internal runtime surface `{forbidden}`"
        );
    }
    let helper_start = source
        .find("pub fn run_public_featureforge_cli_json")
        .expect("public CLI JSON helper should exist");
    let helper_body = &source[helper_start
        ..source[helper_start..]
            .find("\n}\n\n")
            .map(|offset| helper_start + offset + 3)
            .expect("public CLI JSON helper body should be bounded")];

    assert!(
        helper_body.contains("run_featureforge_with_env_control_real_cli"),
        "public CLI helper must invoke the compiled featureforge binary"
    );
    for forbidden in denied_helper_calls() {
        assert!(
            !helper_body.contains(&forbidden),
            "public CLI helper must not use internal helper `{forbidden}`"
        );
    }
}

#[test]
fn public_normal_path_help_hides_internal_compatibility_flags() {
    let hidden_flags = [
        hidden_literal(&["--dispatch", "-id"]),
        hidden_literal(&["--branch", "-closure-id"]),
    ];
    for command in ["close-current-task", "advance-late-stage"] {
        let output = Command::new(env!("CARGO_BIN_EXE_featureforge"))
            .args(["plan", "execution", command, "--help"])
            .output()
            .unwrap_or_else(|error| panic!("plan execution {command} --help should run: {error}"));
        assert!(
            output.status.success(),
            "plan execution {command} --help should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout)
            .expect("normal-path command help stdout should be utf-8");
        for hidden_flag in &hidden_flags {
            assert!(
                !stdout.contains(hidden_flag),
                "public normal-path help for `{command}` must not expose hidden compatibility flag `{hidden_flag}`:\n{stdout}"
            );
        }
    }
}

#[test]
fn public_test_files_do_not_use_internal_helpers_or_hidden_commands() {
    let mut violations = Vec::new();
    for file in rust_test_files(&repo_root().join("tests")) {
        let rel = repo_relative(&file);
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", file.display()));
        violations.extend(scan_source_for_public_flow_violations(&rel, &source));
        violations.extend(scan_stale_dispatch_public_flow_violations(&rel, &source));
    }

    assert!(
        violations.is_empty(),
        "public-flow tests must not use internal helpers or hidden command literals:\n{}",
        violations.join("\n")
    );
}

#[test]
fn stale_dispatch_public_flow_test_is_static_guarded() {
    let source = read_repo_file("tests/workflow_shell_smoke.rs");
    let violations =
        scan_stale_dispatch_public_flow_violations("tests/workflow_shell_smoke.rs", &source);
    assert!(
        violations.is_empty(),
        "stale-dispatch public-flow tests must not use hidden helpers or dispatch flags:\n{}",
        violations.join("\n")
    );
}

#[test]
fn active_docs_do_not_teach_internal_compatibility_flags_or_env_gate() {
    let denied_terms = internal_compatibility_hidden_surface_terms();
    let mut violations = Vec::new();
    for path in internal_compatibility_active_doc_files() {
        let rel = repo_relative(&path);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
        for term in &denied_terms {
            for (start, _) in source.match_indices(term) {
                violations.push(format!(
                    "{rel}:{} active docs and prompts must not teach internal compatibility flag/env `{term}`",
                    line_number_for_byte(&source, start)
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "active docs/prompts must not teach hidden compatibility flags or env gate:\n{}",
        violations.join("\n")
    );
}

#[test]
fn internal_execution_flag_gate_documents_reason_and_expiry() {
    let outputs = read_repo_file("src/execution/commands/common/outputs.rs");
    for required in [
        "INTERNAL_EXECUTION_FLAGS_COMPATIBILITY_REASON",
        "temporary migration support for pre-public dispatch and branch-closure identifiers",
        "INTERNAL_EXECUTION_FLAGS_EXPIRY_CONDITION",
        "internal migration coverage no longer requires explicit dispatch or branch-closure ids",
    ] {
        assert!(
            outputs.contains(required),
            "internal execution flag gate must document compatibility purpose and expiry via `{required}`"
        );
    }
}

#[test]
fn public_runtime_flow_gate_suites_are_all_protected() {
    let public_script = read_repo_file("scripts/run-public-runtime-flow-tests.sh");
    let binaries = public_runtime_flow_test_binaries_from_script(&public_script);
    assert!(
        !binaries.is_empty(),
        "public runtime flow gate should select explicit test binaries"
    );

    for binary in binaries {
        let rel = format!("tests/{binary}.rs");
        assert!(
            public_runtime_flow_test_files().contains(&rel),
            "public runtime flow gate binary `{binary}` should be tracked as a public-flow file"
        );
        assert!(
            is_protected_public_flow_file(&rel),
            "public runtime flow gate binary `{binary}` should be protected from internal helper imports and hidden commands"
        );
    }
}

#[test]
fn release_gates_keep_public_flow_and_internal_compatibility_suites_separate() {
    let public_script = read_repo_file("scripts/run-public-runtime-flow-tests.sh");
    assert!(
        public_script.contains("cargo nextest run"),
        "public runtime flow gate should be a runnable nextest command"
    );
    for required in [
        "--test public_cli_flow_contracts",
        "--test public_replay_churn",
        "--test runtime_behavior_golden",
        "--no-fail-fast",
    ] {
        assert!(
            public_script.contains(required),
            "public runtime flow gate must include `{required}`"
        );
    }
    for forbidden in [
        "internal_only_compatibility",
        "tests/internal_",
        "--test internal_",
        "support/internal_only_direct_helpers.rs",
        "support/internal_runtime_direct.rs",
        "support/plan_execution_direct.rs",
        "support/workflow_direct.rs",
    ] {
        assert!(
            !public_script.contains(forbidden),
            "public runtime flow gate must not depend on internal helper coverage via `{forbidden}`"
        );
    }

    let internal_script = read_repo_file("scripts/run-internal-runtime-compatibility-tests.sh");
    assert!(
        internal_script.contains("cargo nextest run"),
        "internal compatibility gate should be a runnable nextest command"
    );
    for required in [
        "tests/internal_*.rs",
        "internal_test_args+=(--test \"$test_name\")",
        "--no-fail-fast",
    ] {
        assert!(
            internal_script.contains(required),
            "internal compatibility gate must include internal test files through `{required}`"
        );
    }
    for forbidden in [
        "--test public_cli_flow_contracts",
        "--test public_replay_churn",
        "--test runtime_behavior_golden",
    ] {
        assert!(
            !internal_script.contains(forbidden),
            "internal compatibility gate must not be presented as the public runtime flow suite via `{forbidden}`"
        );
    }

    let testing_docs = read_repo_file("docs/testing.md");
    for required in [
        "scripts/run-public-runtime-flow-tests.sh",
        "scripts/run-internal-runtime-compatibility-tests.sh",
        "Public-flow proof",
        "internal runtime compatibility",
        "do not count it as public-flow",
        "public UX proof",
    ] {
        assert!(
            testing_docs.contains(required),
            "release checklist docs must report public-flow and internal compatibility results separately via `{required}`"
        );
    }

    let release_notes = read_repo_file("RELEASE-NOTES.md");
    for required in [
        "separate public-flow and internal runtime compatibility gates",
        "internal-helper suites are not",
        "public UX proof",
    ] {
        assert!(
            release_notes.contains(required),
            "release notes must not cite internal-helper tests as public-flow proof; missing `{required}`"
        );
    }
}

#[test]
fn internal_compatibility_test_names_live_only_in_internal_files() {
    let mut violations = Vec::new();
    let mut internal_files_with_compatibility_names = Vec::new();
    for file in top_level_rust_test_files(&repo_root().join("tests")) {
        let rel = repo_relative(&file);
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", file.display()));
        let compatibility_names = internal_compatibility_function_names(&rel, &source);
        if compatibility_names.is_empty() {
            continue;
        }
        if file_name_is_internal_quarantine(&rel) {
            internal_files_with_compatibility_names.push(rel);
        } else {
            violations.push(format!(
                "{rel} declares internal compatibility functions outside a tests/internal_*.rs file: {}",
                compatibility_names.join(", ")
            ));
        }
    }

    assert!(
        !internal_files_with_compatibility_names.is_empty(),
        "internal compatibility coverage should live in top-level tests/internal_*.rs files"
    );
    assert!(
        violations.is_empty(),
        "internal compatibility tests must be split out of public/runtime test files:\n{}",
        violations.join("\n")
    );
}

#[test]
fn internal_compatibility_name_scan_catches_cfg_test_wrapped_items() {
    let compatibility_fn = hidden_literal(&[
        "internal_only_",
        "compatibility_hidden_inside_cfg_test_module",
    ]);
    let fixture = format!(
        r#"
#[cfg(test)]
mod tests {{
    #[test]
    fn {compatibility_fn}() {{}}
}}
"#
    );

    let compatibility_names =
        internal_compatibility_function_names("tests/public_fixture.rs", &fixture);

    assert_eq!(
        compatibility_names,
        vec![compatibility_fn],
        "internal compatibility split scan must include cfg(test)-wrapped test declarations"
    );
}

#[test]
fn production_diagnostics_do_not_route_to_hidden_gates_or_receipt_repair() {
    let mut violations = Vec::new();
    let forbidden_patterns = public_diagnostic_forbidden_patterns();
    for file in production_source_and_active_doc_files() {
        let rel = repo_relative(&file);
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", file.display()));
        violations.extend(diagnostic_pattern_violations_for_source(
            &rel,
            &source,
            &forbidden_patterns,
        ));
    }

    assert!(
        violations.is_empty(),
        "production public diagnostics and active docs must not revive hidden-gate or receipt-repair wording:\n{}",
        violations.join("\n")
    );
}

#[test]
fn advance_late_stage_public_outputs_do_not_expose_low_level_primitives() {
    let denylist = vec![
        hidden_literal(&["delegated", "_primitive"]),
        hidden_literal(&["record", "-branch-closure"]),
        hidden_literal(&["record", "-release-readiness"]),
        hidden_literal(&["record", "-final-review"]),
        hidden_literal(&["record", "-qa"]),
    ];
    let files = [
        "src/execution/commands/advance_late_stage.rs",
        "src/execution/commands/common/late_stage_reruns.rs",
        "src/execution/commands/common/operator_outputs.rs",
        "src/execution/commands/common/outputs.rs",
        "src/execution/current_truth.rs",
        "src/execution/projection_renderer.rs",
    ];
    let mut violations = Vec::new();

    for rel in files {
        let source = fs::read_to_string(repo_root().join(rel))
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        for pattern in &denylist {
            for (start, _) in source.match_indices(pattern) {
                violations.push(format!(
                    "{rel}:{} public advance-late-stage output source must expose intent and operation instead of `{pattern}`",
                    line_number_for_byte(&source, start)
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "public advance-late-stage output must not leak low-level primitive names:\n{}",
        violations.join("\n")
    );
}

#[test]
fn packaged_binaries_do_not_expose_advance_late_stage_low_level_primitives() {
    let denylist = vec![
        hidden_literal(&["delegated", "_primitive"]),
        hidden_literal(&["record", "-branch-closure"]),
        hidden_literal(&["record", "-release-readiness"]),
        hidden_literal(&["record", "-final-review"]),
        hidden_literal(&["record", "-qa"]),
    ];
    let binaries = [
        "bin/featureforge",
        "bin/prebuilt/darwin-arm64/featureforge",
        "bin/prebuilt/windows-x64/featureforge.exe",
    ];
    let mut violations = Vec::new();

    for rel in binaries {
        let contents = fs::read(repo_root().join(rel))
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        for pattern in &denylist {
            if contains_bytes(&contents, pattern.as_bytes()) {
                violations.push(format!(
                    "{rel}: checked-in packaged runtime must not expose `{pattern}`"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "checked-in packaged runtimes must not leak low-level advance-late-stage primitives:\n{}",
        violations.join("\n")
    );
}

#[test]
fn active_prompt_and_public_docs_do_not_expose_task4_denied_control_plane_terms() {
    let denylist = task4_denied_public_control_plane_terms();
    let mut violations = Vec::new();

    for path in task4_active_public_surface_files() {
        let rel = repo_relative(&path);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
        for pattern in &denylist {
            for (start, _) in source.match_indices(pattern) {
                violations.push(format!(
                    "{rel}:{} active public/prompt surface must not expose denied control-plane term `{pattern}`",
                    line_number_for_byte(&source, start)
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "active public/prompt surfaces must not teach stale binary/control-plane terms:\n{}",
        violations.join("\n")
    );
}

#[test]
fn production_diagnostic_scan_includes_root_and_review_active_docs() {
    let files = production_source_and_active_doc_files()
        .into_iter()
        .map(|file| repo_relative(&file))
        .collect::<HashSet<_>>();

    for required in ["AGENTS.md", "README.md", "RELEASE-NOTES.md", "TODOS.md"] {
        assert!(
            files.contains(required),
            "root active doc `{required}` must be covered by the hidden-gate and receipt-repair wording scan"
        );
    }
    assert!(
        files.contains("review/checklist.md"),
        "active review guidance must be covered by the hidden-gate and receipt-repair wording scan"
    );
}

#[test]
fn production_diagnostic_scanner_only_allows_explicit_historical_comments() {
    let forbidden_patterns = public_diagnostic_forbidden_patterns();
    let active_doc = "This historical note says retry gate-review.";
    let production_string =
        r#"let remediation = "historical retry gate-review guidance must still fail";"#;
    let raw_string_star_line = "let remediation = r#\"\n* historical retry gate-review\n\"#;";
    let raw_string_line_comment = "let remediation = r#\"// historical: retry gate-review\"#;";
    let rebuild_evidence_remediation =
        r#"let remediation = "Rebuild the execution evidence for the affected step";"#;
    let no_article_receipt_repair =
        r#"let remediation = "Restore authoritative unit-review receipt readability";"#;
    let source_comment = "// historical: retry gate-review was old wording";
    let block_comment = "/* historical:\n * retry gate-review was old wording\n */";

    assert!(
        diagnostic_pattern_violations_for_source(
            "docs/testing.md",
            active_doc,
            &forbidden_patterns
        )
        .iter()
        .any(|violation| violation.contains("retry gate-review")),
        "active docs must not bypass the scan by saying historical"
    );
    assert!(
        diagnostic_pattern_violations_for_source(
            "src/execution/example.rs",
            production_string,
            &forbidden_patterns
        )
        .iter()
        .any(|violation| violation.contains("retry gate-review")),
        "production string literals must not bypass the scan by saying historical"
    );
    assert!(
        diagnostic_pattern_violations_for_source(
            "src/execution/example.rs",
            raw_string_star_line,
            &forbidden_patterns
        )
        .iter()
        .any(|violation| violation.contains("retry gate-review")),
        "raw production string lines must not bypass the scan by looking like block comments"
    );
    assert!(
        diagnostic_pattern_violations_for_source(
            "src/execution/example.rs",
            raw_string_line_comment,
            &forbidden_patterns
        )
        .iter()
        .any(|violation| violation.contains("retry gate-review")),
        "raw production strings must not bypass the scan by containing line-comment markers"
    );
    assert!(
        diagnostic_pattern_violations_for_source(
            "src/execution/example.rs",
            rebuild_evidence_remediation,
            &forbidden_patterns
        )
        .iter()
        .any(|violation| violation.contains("rebuild the execution evidence")),
        "scanner must catch direct rebuild-evidence remediation wording"
    );
    assert!(
        diagnostic_pattern_violations_for_source(
            "src/execution/example.rs",
            no_article_receipt_repair,
            &forbidden_patterns
        )
        .iter()
        .any(|violation| violation.contains("restore authoritative unit-review receipt")),
        "scanner must catch the original no-article receipt repair wording"
    );
    assert!(
        diagnostic_pattern_violations_for_source(
            "src/execution/example.rs",
            source_comment,
            &forbidden_patterns
        )
        .is_empty(),
        "explicitly historical Rust comments may document removed wording"
    );
    assert!(
        diagnostic_pattern_violations_for_source(
            "src/execution/example.rs",
            block_comment,
            &forbidden_patterns
        )
        .is_empty(),
        "explicitly historical Rust block comments may document removed wording"
    );
}

#[test]
fn scanner_rejects_internal_only_quarantine_inside_public_gate_suite() {
    let helper = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let hidden_command = hidden_literal(&["record", "-review-dispatch"]);
    let compatibility_fn = hidden_literal(&["internal_only_", "compatibility_fixture"]);
    let fixture = format!(
        "#[test]\nfn {compatibility_fn}() {{\n    {helper}\n    let _ = &[\"{hidden_command}\"];\n}}\n"
    );

    let violations =
        scan_source_for_public_flow_violations("tests/runtime_behavior_golden.rs", &fixture);

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("plan_execution_output_direct")),
        "public gate suites must not hide internal-helper coverage in internal_only_* tests, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(&hidden_command)),
        "public gate suites must not hide hidden command coverage in internal_only_* tests, got {violations:?}"
    );
}

#[test]
fn public_replay_command_budget_gates_are_explicit() {
    let replay = read_repo_file("tests/public_replay_churn.rs");
    for needle in [
        "fn public_replay_begin_owns_allowed_preflight_without_hidden_command()",
        r#"cli.delta_since(&checkpoint, "begin")"#,
        "bridge should need one public begin after route discovery",
        "fn public_replay_cycle_break_clears_on_current_closure_refresh_without_loop()",
        r#"cli.delta_since(&checkpoint, "close-current-task")"#,
        r#"cli.delta_since(&checkpoint, "reopen")"#,
        "cycle-break recovery must not loop through reopen",
        "fn public_replay_engineering_approved_plan_without_fidelity_cannot_bypass_to_implementation()",
        "approved plan fidelity gate",
        "receipt",
    ] {
        assert!(
            replay.contains(needle),
            "public_replay_churn.rs must keep the public replay budget/fidelity gate assertion `{needle}`"
        );
    }

    let shell = read_repo_file("tests/workflow_shell_smoke.rs");
    let fs11_budget_test = shell
        .split("fn fs11_rebase_resume_recovery_budget_is_capped_without_hidden_helpers()")
        .nth(1)
        .and_then(|source| source.split("\n#[test]").next())
        .expect("workflow_shell_smoke.rs must keep the FS11 rebase/resume budget test");
    for needle in [
        "FS11-REBASE-RESUME-BUDGET",
        "runtime_management_commands, 2",
    ] {
        assert!(
            fs11_budget_test.contains(needle),
            "FS11 rebase/resume budget test must keep the assertion `{needle}`"
        );
    }
    for needle in [
        "fn task_close_happy_path_runtime_management_budget_is_capped()",
        "TASK-CLOSE-BUDGET",
    ] {
        assert!(
            shell.contains(needle),
            "workflow_shell_smoke.rs must keep the runtime-management budget assertion `{needle}`"
        );
    }
}

#[test]
fn scanner_rejects_public_internal_helper_and_hidden_command_fixtures() {
    let helper = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let helper_name = hidden_literal(&["internal_only_try_run_", "plan_execution_output_direct"]);
    let root_helper = hidden_literal(&["internal_only_try_run_", "root_output_direct"]);
    let wrapper_helper = hidden_literal(&[
        "internal_only_runtime_",
        "pre",
        "flight_gate_json(repo, state, plan, context);",
    ]);
    let split_wrapper_helper =
        hidden_literal(&["internal_only_runtime_", "pre", "flight_gate_json"]);
    let internal_fixture_call = hidden_literal(&["internal_only_", "plan_execution_fixture_json"]);
    let hidden_command = hidden_literal(&["record", "-review-dispatch"]);
    let hidden_flag = hidden_literal(&["--dispatch", "-id"]);
    let concat_hidden_command = concat_source_expr(&["record", "-review-dispatch"]);
    let concat_hidden_flag = concat_source_expr(&["--dispatch", "-id"]);
    let fixture = format!(
        "use crate::support::plan_execution_direct::{helper_name} as aliased_direct_helper;\nuse crate::support::plan_execution_direct::{{\n    {helper_name} as multiline_aliased_direct_helper,\n}};\nconst HIDDEN_COMMANDS: &[&str] = &[\n    \"{hidden_command}\",\n    {concat_hidden_command},\n];\nconst HIDDEN_FLAGS: &[&str] = &[\"{hidden_flag}\", {concat_hidden_flag}];\nconst CMD_ALIAS: &str = {concat_hidden_command};\nconst MULTILINE_CMD_ALIAS: &str =\n    {concat_hidden_command};\n\nfn public_wrapper(repo: &Path, state: &Path, plan: &str, context: &str) {{\n    {wrapper_helper}\n}}\n\npub fn\npublic_split_signature_wrapper(repo: &Path, state: &Path, plan: &str, context: &str) {{\n    {wrapper_helper}\n}}\n\nfn public_split_wrapper(repo: &Path, state: &Path, plan: &str, context: &str) {{\n    {split_wrapper_helper}\n        (repo, state, plan, context);\n}}\n\n#[test]\nfn public_fixture() {{\n    {helper}\n    {root_helper}(repo, state, args, context);\n    let direct_helper_alias = {helper_name};\n    let split_direct_helper_alias =\n        {helper_name};\n    aliased_direct_helper(repo, state, args, context);\n    multiline_aliased_direct_helper(repo, state, args, context);\n    direct_helper_alias(repo, state, args, context);\n    split_direct_helper_alias(repo, state, args, context);\n    let plain_hidden_literal = [\"{hidden_command}\"];\n    let dispatch_flag = {concat_hidden_flag};\n    let args_alias = [CMD_ALIAS, dispatch_flag];\n    let multiline_args_alias = [\n        MULTILINE_CMD_ALIAS,\n        {concat_hidden_flag},\n    ];\n    let split_args_alias =\n        [\n            CMD_ALIAS,\n            {concat_hidden_flag},\n        ];\n    let mut pushed_args = Vec::new();\n    pushed_args.push(CMD_ALIAS);\n    pushed_args.extend([{concat_hidden_command}]);\n    pushed_args.extend_from_slice(&multiline_args_alias);\n    let _ = plain_hidden_literal;\n    let _ = HIDDEN_FLAGS;\n    let _ = run_featureforge(repo, state, &[\n        \"{hidden_command}\",\n        \"{hidden_flag}\",\n    ], context);\n    let _ = run_featureforge(repo, state, &[\n        {concat_hidden_command},\n        {concat_hidden_flag},\n    ], context);\n    let _ = run_featureforge(repo, state, &args_alias, context);\n    let _ = run_featureforge(repo, state, &multiline_args_alias, context);\n    let _ = run_featureforge(repo, state, &split_args_alias, context);\n    let _ = run_featureforge(repo, state, &pushed_args, context);\n    let _child = Command::new(\"featureforge\").arg(CMD_ALIAS).arg(dispatch_flag);\n    let _ = {internal_fixture_call}(repo, state, &[\n        \"{hidden_command}\",\n    ], context);\n}}\n"
    );
    let violations = scan_source_for_public_flow_violations("tests/public_fixture.rs", &fixture);

    assert!(
        violations
            .iter()
            .any(|violation| violation
                .contains(concat!("try_run_", "plan_execution_output_direct("))),
        "scanner should reject public direct-helper calls, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("public_wrapper")),
        "scanner should reject public wrappers around internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("public_split_wrapper")),
        "scanner should reject public wrappers around split-line internal helper calls, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("public_split_signature_wrapper")),
        "scanner should reject public split-signature wrappers around internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("aliased_direct_helper")),
        "scanner should reject use-alias calls to internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("multiline_aliased_direct_helper")),
        "scanner should reject multiline use-alias calls to internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("direct_helper_alias")),
        "scanner should reject function-pointer aliases to internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("split_direct_helper_alias")),
        "scanner should reject split-line function-pointer aliases to internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(&hidden_command)),
        "scanner should reject public hidden command invocations, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(&hidden_flag)),
        "scanner should reject public hidden flags, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("CMD_ALIAS")),
        "scanner should reject public hidden command aliases, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("MULTILINE_CMD_ALIAS")),
        "scanner should reject public multiline hidden command aliases, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("args_alias")),
        "scanner should reject public prebuilt hidden arg arrays, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("multiline_args_alias")),
        "scanner should reject public multiline prebuilt hidden arg arrays, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("split_args_alias")),
        "scanner should reject public split-line hidden arg arrays, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("pushed_args")),
        "scanner should reject push/extend-built hidden arg arrays, got {violations:?}"
    );
}

#[test]
fn scanner_rejects_public_direct_runtime_surface_wrapper_fixture() {
    let operator_marker = hidden_literal(&["operator_for_", "runtime("]);
    let doctor_marker = hidden_literal(&["doctor_for_", "runtime("]);
    let doctor_with_args_marker = hidden_literal(&["doctor_for_", "runtime_with_args("]);
    let doctor_phase_next_marker =
        hidden_literal(&["doctor_phase_and_next_for_", "runtime_with_args("]);
    let phase_marker = hidden_literal(&["phase_for_", "runtime("]);
    let handoff_marker = hidden_literal(&["handoff_for_", "runtime("]);
    let status_marker = hidden_literal(&[".status", "("]);
    let status_refresh_marker = hidden_literal(&[".status_", "refresh("]);
    let review_gate_marker = hidden_literal(&[".review_gate", "("]);
    let finish_gate_marker = hidden_literal(&[".finish_gate", "("]);
    let fixture = format!(
        "
fn status_value(runtime: &ExecutionRuntime, args: &StatusArgs) {{
    runtime{status_marker}args);
}}

fn plan_execution_status_json(runtime: &ExecutionRuntime, plan: &str) {{
    runtime{status_marker}&StatusArgs {{ plan: plan.into(), external_review_result_ready: false }});
}}

fn workflow_status_refresh_json(runtime: &mut WorkflowRuntime) {{
    runtime{status_refresh_marker});
}}

fn workflow_operator_json(runtime: &ExecutionRuntime, args: &OperatorArgs) {{
    operator::{operator_marker}runtime, args);
}}

fn workflow_doctor_json(runtime: &ExecutionRuntime) {{
    operator::{doctor_marker}runtime);
}}

fn workflow_doctor_with_args_json(runtime: &ExecutionRuntime, args: &DoctorArgs) {{
    operator::{doctor_with_args_marker}runtime, args);
}}

fn workflow_doctor_phase_next_json(runtime: &ExecutionRuntime, args: &DoctorArgs) {{
    operator::{doctor_phase_next_marker}runtime, args);
}}

fn workflow_phase_json(runtime: &ExecutionRuntime) {{
    operator::{phase_marker}runtime);
}}

fn workflow_handoff_json(runtime: &ExecutionRuntime) {{
    operator::{handoff_marker}runtime);
}}

fn workflow_gate_review_json(runtime: &ExecutionRuntime, args: &StatusArgs) {{
    runtime{review_gate_marker}args);
}}

fn workflow_gate_finish_json(runtime: &ExecutionRuntime, args: &StatusArgs) {{
    runtime{finish_gate_marker}args);
}}

fn status_alias_value(rt: &ExecutionRuntime, args: &StatusArgs) {{
    rt{status_marker}args);
}}

fn review_gate_local_alias_value(runtime: &ExecutionRuntime, args: &StatusArgs) {{
    let direct = runtime;
    direct{review_gate_marker}args);
}}
"
    );
    let violations =
        scan_source_for_public_flow_violations("tests/support/runtime_surfaces.rs", &fixture);

    for wrapper in [
        "status_value",
        "plan_execution_status_json",
        "workflow_status_refresh_json",
        "workflow_operator_json",
        "workflow_doctor_json",
        "workflow_doctor_with_args_json",
        "workflow_doctor_phase_next_json",
        "workflow_phase_json",
        "workflow_handoff_json",
        "workflow_gate_review_json",
        "workflow_gate_finish_json",
        "status_alias_value",
        "review_gate_local_alias_value",
    ] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(wrapper)
                    && violation.contains("direct runtime surface")),
            "scanner should reject public direct runtime surface wrapper `{wrapper}`, got {violations:?}"
        );
    }
}

#[test]
fn scanner_rejects_cfg_test_wrapped_public_direct_runtime_surface_wrappers() {
    let doctor_marker = hidden_literal(&["doctor_for_", "runtime("]);
    let status_refresh_marker = hidden_literal(&["runtime.status_", "refresh("]);
    let fixture = format!(
        r#"
#[cfg(test)]
mod tests {{
    use super::*;

    fn cfg_test_doctor_wrapper(runtime: &ExecutionRuntime) {{
        operator::{doctor_marker}runtime);
    }}

    fn cfg_test_status_refresh_wrapper(runtime: &mut WorkflowRuntime) {{
        {status_refresh_marker});
    }}
}}
"#
    );

    let violations = scan_source_for_public_flow_violations("tests/workflow_runtime.rs", &fixture);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("cfg_test_doctor_wrapper")
                && violation.contains("doctor_for_runtime")
                && violation.contains("direct runtime surface")),
        "scanner should reject cfg(test)-wrapped doctor_for_runtime wrappers, got {violations:?}"
    );
    assert!(
        violations.iter().any(
            |violation| violation.contains("cfg_test_status_refresh_wrapper")
                && violation.contains("status_refresh")
                && violation.contains("direct runtime surface")
        ),
        "scanner should reject cfg(test)-wrapped status_refresh wrappers, got {violations:?}"
    );
}

#[test]
fn scanner_rejects_cfg_test_aliased_public_direct_runtime_surface_wrappers() {
    let doctor_import = hidden_literal(&[
        "use featureforge::workflow::operator::doctor_for_",
        "runtime as cfg_test_doctor;",
    ]);
    let phase_import = hidden_literal(&[
        "use featureforge::workflow::operator::phase_for_",
        "runtime as cfg_test_phase;",
    ]);
    let fixture = format!(
        r#"
#[cfg(test)]
mod tests {{
    {doctor_import}
    {phase_import}

    fn cfg_test_aliased_doctor_wrapper(runtime: &ExecutionRuntime) {{
        cfg_test_doctor(runtime);
    }}

    fn cfg_test_aliased_phase_wrapper(runtime: &ExecutionRuntime) {{
        cfg_test_phase(runtime);
    }}
}}
"#
    );

    let violations = scan_source_for_public_flow_violations("tests/workflow_runtime.rs", &fixture);
    assert!(
        violations.iter().any(
            |violation| violation.contains("cfg_test_aliased_doctor_wrapper")
                && violation.contains("doctor_for_runtime")
                && violation.contains("direct runtime surface")
        ),
        "scanner should reject cfg(test)-aliased doctor_for_runtime wrappers, got {violations:?}"
    );
    assert!(
        violations.iter().any(
            |violation| violation.contains("cfg_test_aliased_phase_wrapper")
                && violation.contains("phase_for_runtime")
                && violation.contains("direct runtime surface")
        ),
        "scanner should reject cfg(test)-aliased phase_for_runtime wrappers, got {violations:?}"
    );
}

#[test]
fn scanner_rejects_public_flow_header_bypass_fixture() {
    let helper = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let fixture = format!(
        "{INTERNAL_RUNTIME_HELPER_HEADER}\n\n#[test]\nfn public_replay_fixture() {{\n    {helper}\n}}\n"
    );
    let violations =
        scan_source_for_public_flow_violations("tests/public_replay_churn.rs", &fixture);

    assert!(
        violations
            .iter()
            .any(|violation| violation
                .contains("must not use the internal helper quarantine header")),
        "scanner should reject quarantine headers on protected public-flow files, got {violations:?}"
    );
}

#[test]
fn scanner_rejects_public_flow_internal_support_imports() {
    let featureforge_support = hidden_literal(&["support/", "featureforge.rs"]);
    let internal_runtime_direct = hidden_literal(&["support/", "internal_runtime_direct.rs"]);
    let internal_runtime_phase_handoff =
        hidden_literal(&["support/", "internal_runtime_phase_handoff.rs"]);
    let plan_execution_direct = hidden_literal(&["support/", "plan_execution_direct.rs"]);
    let workflow_direct = hidden_literal(&["support/", "workflow_direct.rs"]);
    let fixture = format!(
        r#"
#[path = "{featureforge_support}"]
mod featureforge_support;
#[path = "{internal_runtime_direct}"]
mod internal_runtime_direct;
#[path = "{internal_runtime_phase_handoff}"]
mod internal_runtime_phase_handoff_support;
#[path = "{plan_execution_direct}"]
mod plan_execution_direct_support;
#[path = "{workflow_direct}"]
mod workflow_direct_support;

#[test]
fn public_replay_fixture() {{}}
"#
    );
    let violations =
        scan_source_for_public_flow_violations("tests/public_replay_churn.rs", &fixture);

    for forbidden in [
        featureforge_support,
        internal_runtime_direct,
        internal_runtime_phase_handoff,
        plan_execution_direct,
        workflow_direct,
    ] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(&forbidden)),
            "scanner should reject public-flow import `{forbidden}`, got {violations:?}"
        );
    }
}

#[test]
fn scanner_rejects_any_internal_only_helper_call_not_just_hardcoded_list() {
    let omitted_helper = "internal_only_unit_record_contract_json";
    let fixture = format!(
        r#"
use crate::support::internal_runtime_direct::{omitted_helper} as omitted_record_contract;

fn public_wrapper(repo: &Path, state: &Path, args: &RecordContractArgs) {{
    {omitted_helper}(repo, state, args);
}}

#[test]
fn public_replay_fixture() {{
    omitted_record_contract(repo, state, args);
    let direct_record_contract = {omitted_helper};
    direct_record_contract(repo, state, args);
    public_wrapper(repo, state, args);
}}
"#
    );
    let violations = scan_source_for_public_flow_violations("tests/public_fixture.rs", &fixture);

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("internal_only_unit_record_contract_json(")),
        "scanner should reject any internal_only_* helper call, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("omitted_record_contract")),
        "scanner should reject aliases to any internal_only_* helper, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("direct_record_contract")),
        "scanner should reject function-pointer aliases to any internal_only_* helper, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("public_wrapper")),
        "scanner should reject wrappers around any internal_only_* helper, got {violations:?}"
    );
}

#[test]
fn scanner_rejects_stale_dispatch_public_flow_hidden_terms() {
    let hidden_command = hidden_literal(&["record", "-review-dispatch"]);
    let hidden_gate = hidden_literal(&["gate", "-review"]);
    let hidden_rebuild = hidden_literal(&["rebuild", "-evidence"]);
    let hidden_flag = hidden_literal(&["--dispatch", "-id"]);
    let fixture = format!(
        r#"
fn public_close_current_task_records_positive_closure_after_stale_dispatch_lineage_without_dispatch_id() {{
    internal_only_seed_dispatch(repo, state);
    let _ = run_plan_execution_json_real_cli(repo, state, &[
        "{hidden_command}",
        "{hidden_gate}",
        "{hidden_rebuild}",
        "{hidden_flag}",
    ], "stale dispatch public replay should reject hidden terms");
}}

fn internal_only_compatibility_plan_execution_close_current_task_rejects_explicit_stale_dispatch_id_after_drift() {{
    let _ = run_featureforge_with_env(repo, state, &[
        "plan",
        "execution",
        "close-current-task",
        "{hidden_flag}",
        "stale-dispatch",
    ], &[], "internal fixture may use hidden dispatch id");
}}
"#
    );
    let violations =
        scan_stale_dispatch_public_flow_violations("tests/workflow_shell_smoke.rs", &fixture);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("internal_only_seed_dispatch")),
        "stale-dispatch scanner should reject internal_only_* helpers in public replay tests, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(&hidden_command)),
        "stale-dispatch scanner should reject hidden dispatch commands in public replay tests, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(&hidden_gate)),
        "stale-dispatch scanner should reject hidden review gates in public replay tests, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(&hidden_rebuild)),
        "stale-dispatch scanner should reject hidden evidence rebuilds in public replay tests, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(&hidden_flag)),
        "stale-dispatch scanner should reject hidden dispatch flags in public replay tests, got {violations:?}"
    );
    assert!(
        violations.iter().all(|violation| !violation.contains(
            "internal_only_compatibility_plan_execution_close_current_task_rejects_explicit_stale_dispatch_id_after_drift"
        )),
        "stale-dispatch scanner should keep internal compatibility tests quarantined, got {violations:?}"
    );
}

#[test]
fn internal_helper_bridge_is_quarantined() {
    let source = read_repo_file("tests/support/internal_only_direct_helpers.rs");
    assert!(
        source.starts_with(INTERNAL_RUNTIME_HELPER_HEADER),
        "internal_only_direct_helpers.rs must start with the internal-only quarantine contract"
    );
    assert!(
        source.contains("internal_runtime_direct.rs"),
        "internal helper bridge should route direct helper access through the quarantined internal runtime helper"
    );
    for duplicate_prone_import in ["plan_execution_direct.rs", "workflow_direct.rs"] {
        assert!(
            !source.contains(duplicate_prone_import),
            "internal helper bridge must not duplicate nested imports for `{duplicate_prone_import}`"
        );
    }
}

#[test]
fn explicit_internal_helper_scope_exceptions_are_reasoned() {
    for (rel, function_name) in [
        (
            "tests/workflow_shell_smoke.rs",
            "setup_qa_pending_case_slow",
        ),
        (
            "tests/workflow_shell_smoke.rs",
            "setup_ready_for_finish_case_slow",
        ),
        (
            "tests/workflow_shell_smoke.rs",
            "setup_ready_for_finish_case_with_qa_requirement_slow",
        ),
    ] {
        let reason = explicit_internal_helper_scope_exception_reason(rel, function_name)
            .expect("explicit internal-helper scope exception should be listed");
        assert!(
            reason.len() > 50 && reason.contains("fixture"),
            "{rel}:{function_name} exception should explain the fixture-only boundary"
        );
    }
}

#[test]
fn scanner_allows_reasoned_fixture_setup_exception_without_tainting_public_caller() {
    let fixture = r#"
fn setup_qa_pending_case_slow(repo: &Path, state: &Path, args: &RecordContractArgs) {
    internal_only_unit_record_contract_json(repo, state, args);
}

fn setup_qa_pending_case(repo: &Path, state: &Path, args: &RecordContractArgs) {
    setup_qa_pending_case_slow(repo, state, args);
}

#[test]
fn public_fixture() {
    setup_qa_pending_case(repo, state, args);
}
"#;

    assert!(
        scan_source_for_public_flow_violations("tests/workflow_shell_smoke.rs", fixture).is_empty(),
        "reasoned fixture setup exceptions should not taint public callers"
    );
}

#[test]
fn scanner_rejects_direct_internal_support_imports_even_in_mixed_protected_files() {
    let internal_runtime_direct = hidden_literal(&["support/", "internal_runtime_direct.rs"]);
    let internal_runtime_phase_handoff =
        hidden_literal(&["support/", "internal_runtime_phase_handoff.rs"]);
    let plan_execution_direct = hidden_literal(&["support/", "plan_execution_direct.rs"]);
    let fixture = format!(
        r#"
#[path = "{internal_runtime_direct}"]
mod internal_runtime_direct;
#[path = "{internal_runtime_phase_handoff}"]
mod internal_runtime_phase_handoff_support;
#[path = "{plan_execution_direct}"]
mod plan_execution_direct_support;

#[test]
fn internal_compatibility_fixture() {{}}
"#
    );
    let violations =
        scan_source_for_public_flow_violations("tests/workflow_shell_smoke.rs", &fixture);

    for forbidden in [
        internal_runtime_direct,
        internal_runtime_phase_handoff,
        plan_execution_direct,
    ] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(&forbidden)),
            "scanner should reject direct internal support import `{forbidden}` even in mixed protected files, got {violations:?}"
        );
    }
}

#[test]
fn scanner_rejects_internal_quarantine_bridge_imports_in_public_replay_files() {
    let fixture = r#"
#[path = "support/internal_only_direct_helpers.rs"]
mod internal_only_direct_helpers;

#[test]
fn public_replay_fixture() {}
"#;
    let violations =
        scan_source_for_public_flow_violations("tests/public_replay_churn.rs", fixture);

    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("internal_only_direct_helpers.rs")),
        "scanner should reject internal quarantine bridge imports from public replay files, got {violations:?}"
    );
}

#[test]
fn scanner_allows_explicitly_quarantined_internal_file_fixture() {
    let helper = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let hidden_command = hidden_literal(&["record", "-review-dispatch"]);
    let fixture = format!(
        "{INTERNAL_RUNTIME_HELPER_HEADER}\n\n#[test]\nfn internal_{}_fixture() {{\n    {helper}\n    let _ = &[\"{hidden_command}\"];\n}}\n",
        "only"
    );

    assert!(
        scan_source_for_public_flow_violations("tests/internal_fixture.rs", &fixture).is_empty(),
        "quarantined internal files should be allowed to exercise internal helpers"
    );
}

#[test]
fn scanner_rejects_internal_only_direct_wrappers_in_protected_public_files() {
    let helper = hidden_literal(&[
        "internal_only_unit_",
        "record_contract_json(repo, state, args);",
    ]);
    let direct_refresh = hidden_literal(&["runtime.status_", "refresh();"]);
    let fixture = format!(
        r#"
fn internal_only_helper_wrapper(repo: &Path, state: &Path, args: &RecordContractArgs) {{
    {helper}
}}

fn internal_only_direct_status_refresh(runtime: &mut WorkflowRuntime) {{
    {direct_refresh}
}}
"#
    );

    let violations = scan_source_for_public_flow_violations("tests/workflow_runtime.rs", &fixture);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("internal_only_helper_wrapper")),
        "protected public-flow files must reject internal_only helper wrappers, got {violations:?}"
    );
    assert!(
        violations.iter().any(|violation| violation
            .contains("internal_only_direct_status_refresh")
            && violation.contains("direct runtime surface")),
        "protected public-flow files must reject internal_only direct runtime surface wrappers, got {violations:?}"
    );
}

#[test]
fn scanner_allows_internal_only_helper_wrapper_quarantine_outside_public_gate_files() {
    let helper = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let hidden_command = hidden_literal(&["record", "-review-dispatch"]);
    let compatibility_fn = hidden_literal(&["internal_only_", "helper_fixture"]);
    let fixture = format!(
        "#[test]\nfn {compatibility_fn}() {{\n    {helper}\n    let _ = &[\"{hidden_command}\"];\n}}\n"
    );

    assert!(
        scan_source_for_public_flow_violations("tests/public_fixture.rs", &fixture).is_empty(),
        "non-public helper-wrapper fixtures may still quarantine low-level helper coverage by name"
    );
}

fn scan_source_for_public_flow_violations(rel: &str, source: &str) -> Vec<String> {
    let mut violations = Vec::new();
    if file_name_is_internal_quarantine(rel) {
        return Vec::new();
    }
    if has_internal_runtime_helper_header(source) {
        if is_protected_public_flow_file(rel) {
            violations.push(format!(
                "{rel}:1 must not use the internal helper quarantine header on a protected public-flow test surface"
            ));
        } else {
            return Vec::new();
        }
    }
    if is_protected_public_flow_file(rel) {
        for forbidden in forbidden_internal_support_imports(rel, source) {
            violations.push(format!(
                "{rel} imports internal support module `{forbidden}` from a protected public-flow test surface"
            ));
        }
        for forbidden in internal_quarantine_bridge_imports(rel, source) {
            violations.push(format!(
                "{rel} imports internal-only quarantine bridge `{forbidden}` from a protected public-flow test surface"
            ));
        }
    }

    let denied_helper_calls = denied_helper_calls();
    let denied_helper_names = denied_helper_names(source, &denied_helper_calls);
    let denied_hidden_literals = denied_hidden_literals();
    let mut concat_collector = ConcatLiteralCollector::default();
    let mut hidden_string_bindings = HashSet::new();
    let mut hidden_arg_bindings = HashSet::new();
    let mut pending_assignment = None::<PendingAssignment>;
    let mut inside_command_invocation = false;
    let mut inside_command_args_array = false;
    let function_spans = rust_source_scan::function_spans(rel, source);
    let call_hits = rust_source_scan::normalized_call_path_hits(rel, source, &[]);
    let tainted_functions = tainted_runtime_helper_wrappers(rel, source, &denied_helper_names);
    for (line, function_name) in
        public_tainted_runtime_helper_wrappers(rel, source, &denied_helper_names)
    {
        violations.push(format!(
            "{rel}:{line} defines public wrapper `{function_name}` around an internal runtime helper outside an internal-only quarantine or test"
        ));
    }
    for (line, function_name, marker) in public_direct_runtime_surface_wrappers(rel, source) {
        violations.push(format!(
            "{rel}:{line} defines public direct runtime surface wrapper `{function_name}` using `{marker}` outside an internal-only quarantine or test"
        ));
    }
    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        let current_fn = function_name_for_line(&function_spans, line_number);
        let current_scope = current_fn.unwrap_or("<module>");
        if starts_command_invocation(trimmed) {
            inside_command_invocation = true;
            inside_command_args_array =
                starts_command_args_array(trimmed) || contains_inline_command_args_array(trimmed);
        } else if inside_command_invocation && starts_command_args_array(trimmed) {
            inside_command_args_array = true;
        }
        if current_fn.is_some_and(|name| function_scope_allows_internal_helpers(rel, name)) {
            continue;
        }
        let candidate_literals = candidate_string_literals(trimmed, &mut concat_collector);
        let hidden_literal_hits = hidden_literal_hits(&candidate_literals, &denied_hidden_literals);
        let hidden_identifiers = hidden_identifier_hits(trimmed, &hidden_string_bindings);
        let hidden_arg_identifiers = hidden_identifier_hits(trimmed, &hidden_arg_bindings);
        let line_assignment = (assignment_can_bind_hidden_data(trimmed))
            .then(|| assignment_binding_name(trimmed))
            .flatten()
            .map(|binding| PendingAssignment {
                binding,
                start_line: index + 1,
                is_arg_collection: assignment_binds_arg_collection(trimmed),
                saw_hidden_value: false,
                saw_hidden_arg_collection: false,
            });
        let mut assignment_to_finalize = None;
        if pending_assignment.is_none() {
            pending_assignment = line_assignment;
        }
        if let Some(assignment) = pending_assignment.as_mut() {
            assignment.observe_line(
                trimmed,
                !hidden_literal_hits.is_empty() || !hidden_identifiers.is_empty(),
                !hidden_arg_identifiers.is_empty(),
            );
            if assignment_ends(trimmed) {
                assignment_to_finalize = pending_assignment.take();
            }
        }
        if let Some(assignment) = assignment_to_finalize {
            finalize_assignment(
                rel,
                current_scope,
                &assignment,
                &mut hidden_string_bindings,
                &mut hidden_arg_bindings,
                &mut violations,
            );
        }
        if let Some(binding) = arg_collection_mutation_binding(trimmed)
            && (!hidden_literal_hits.is_empty()
                || !hidden_identifiers.is_empty()
                || !hidden_arg_identifiers.is_empty())
        {
            hidden_arg_bindings.insert(binding.clone());
            violations.push(format!(
                "{rel}:{} mutates hidden command or flag data into arg collection `{binding}` outside an internal-only quarantine or test in `{}`",
                line_number,
                current_scope
            ));
        }
        for call in call_hits.iter().filter(|call| call.line == line_number) {
            if denied_helper_names
                .iter()
                .any(|forbidden| call_matches_name(call, forbidden))
            {
                let displayed = call_display_name(call);
                violations.push(format!(
                    "{rel}:{} uses internal helper `{displayed}(` outside an internal-only quarantine or test in `{}`",
                    line_number,
                    current_scope
                ));
            }
            if tainted_functions.iter().any(|helper_name| {
                current_fn != Some(helper_name.as_str()) && call_matches_name(call, helper_name)
            }) {
                let displayed = call_display_name(call);
                violations.push(format!(
                    "{rel}:{} calls tainted internal runtime helper wrapper `{displayed}` outside an internal-only quarantine or test in `{}`",
                    line_number,
                    current_scope
                ));
            }
        }
        let hidden_literals_are_executable_args =
            inside_command_invocation || inside_command_args_array;
        for hit in &hidden_literal_hits {
            if hit.always_hidden || hidden_literals_are_executable_args {
                violations.push(format!(
                    "{rel}:{} exposes hidden command or flag literal `{}` outside an internal-only quarantine or test in `{}`",
                    line_number,
                    hit.literal,
                    current_scope
                ));
            }
        }
        if hidden_literals_are_executable_args {
            for identifier in hidden_identifiers {
                violations.push(format!(
                    "{rel}:{} passes hidden command or flag alias `{identifier}` to an executable command outside an internal-only quarantine or test in `{}`",
                    line_number,
                    current_scope
                ));
            }
            for identifier in hidden_arg_identifiers {
                violations.push(format!(
                    "{rel}:{} passes hidden command arg collection `{identifier}` to an executable command outside an internal-only quarantine or test in `{}`",
                    line_number,
                    current_scope
                ));
            }
        }
        if inside_command_args_array && ends_command_args_array(trimmed) {
            inside_command_args_array = false;
        }
        if inside_command_invocation && trimmed.ends_with(");") {
            inside_command_invocation = false;
            inside_command_args_array = false;
        }
    }
    violations
}

fn scan_stale_dispatch_public_flow_violations(rel: &str, source: &str) -> Vec<String> {
    if !rel.ends_with(".rs") {
        return Vec::new();
    }
    let function_spans = rust_source_scan::function_spans(rel, source);
    let protected_stale_dispatch_public_functions = function_spans
        .iter()
        .filter(|span| {
            span.name.contains("stale_dispatch")
                && !span.name.starts_with("internal_only_")
                && !span.name.starts_with("setup_")
                && !span.name.starts_with("scanner_")
                && span.name != "scan_stale_dispatch_public_flow_violations"
                && span.name != "stale_dispatch_public_flow_test_is_static_guarded"
        })
        .collect::<Vec<_>>();
    if protected_stale_dispatch_public_functions.is_empty() {
        return Vec::new();
    }
    let denied_literals = [
        hidden_literal(&["record", "-review-dispatch"]),
        hidden_literal(&["gate", "-review"]),
        hidden_literal(&["rebuild", "-evidence"]),
        hidden_literal(&["--dispatch", "-id"]),
    ];
    let mut concat_collector = ConcatLiteralCollector::default();
    let mut violations = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        let Some(function_name) = function_name_for_line(&function_spans, line_number) else {
            continue;
        };
        if !protected_stale_dispatch_public_functions
            .iter()
            .any(|span| span.name == function_name)
        {
            continue;
        }
        let trimmed = line.trim();
        if let Some(call) = trimmed
            .split(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
            .find(|token| token.starts_with("internal_only_"))
        {
            violations.push(format!(
                "{rel}:{line_number} stale-dispatch public-flow test `{function_name}` must not call internal helper `{call}`"
            ));
        }
        for candidate in candidate_string_literals(trimmed, &mut concat_collector) {
            for denied in &denied_literals {
                if candidate.value == *denied {
                    violations.push(format!(
                        "{rel}:{line_number} stale-dispatch public-flow test `{function_name}` must not use hidden command or flag `{denied}`"
                    ));
                }
            }
        }
    }
    violations
}

fn file_name_is_internal_quarantine(rel: &str) -> bool {
    let file_name = Path::new(rel)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    file_name.starts_with("internal_")
}

fn has_internal_runtime_helper_header(source: &str) -> bool {
    source.lines().next() == Some(INTERNAL_RUNTIME_HELPER_HEADER)
}

fn is_protected_public_flow_file(rel: &str) -> bool {
    if rel.starts_with("tests/support/") {
        return false;
    }
    if public_runtime_flow_test_files().contains(rel) {
        return true;
    }
    let file_name = Path::new(rel)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    matches!(
        file_name,
        "contracts_execution_runtime_boundaries.rs"
            | "execution_harness_state.rs"
            | "execution_query.rs"
            | "liveness_model_checker.rs"
            | "plan_execution.rs"
            | "plan_execution_topology.rs"
            | "workflow_entry_shell_smoke.rs"
            | "workflow_runtime.rs"
            | "workflow_runtime_final_review.rs"
            | "workflow_shell_smoke.rs"
    )
}

fn explicit_internal_helper_scope_exception_reason(
    rel: &str,
    function_name: &str,
) -> Option<&'static str> {
    match (rel, function_name) {
        ("tests/workflow_shell_smoke.rs", "setup_qa_pending_case_slow") => Some(
            "late-stage fixture setup seeds dispatched branch review artifact before public routing assertions",
        ),
        ("tests/workflow_shell_smoke.rs", "setup_ready_for_finish_case_slow") => Some(
            "late-stage fixture setup seeds dispatched branch review artifact before public finish-routing assertions",
        ),
        (
            "tests/workflow_shell_smoke.rs",
            "setup_ready_for_finish_case_with_qa_requirement_slow",
        ) => Some(
            "late-stage fixture setup seeds dispatched branch review artifact before public QA-routing assertions",
        ),
        (
            "tests/workflow_entry_shell_smoke.rs",
            "fs17_doctor_public_entrypoints_keep_single_context_build_path",
        ) => Some(
            "source-level doctor architecture scan inspects direct runtime helper bodies without executing them as public-flow routing",
        ),
        _ => None,
    }
}

fn function_scope_allows_internal_helpers(rel: &str, function_name: &str) -> bool {
    if is_protected_public_flow_file(rel) {
        return explicit_internal_helper_scope_exception_reason(rel, function_name).is_some();
    }
    function_name.starts_with("internal_only_")
        || explicit_internal_helper_scope_exception_reason(rel, function_name).is_some()
}

fn forbidden_internal_support_imports(rel: &str, source: &str) -> Vec<String> {
    let syntax = rust_source_scan::parse_rust_source(rel, source);
    let forbidden = forbidden_internal_support_paths();
    syntax
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) => Some(module),
            _ => None,
        })
        .flat_map(|module| module.attrs.iter())
        .filter_map(path_attr_value)
        .filter(|path| forbidden.contains(path))
        .collect()
}

fn internal_quarantine_bridge_imports(rel: &str, source: &str) -> Vec<String> {
    rust_source_scan::parse_rust_source(rel, source)
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) => Some(module),
            _ => None,
        })
        .flat_map(|module| module.attrs.iter())
        .filter_map(path_attr_value)
        .filter(|path| path == "support/internal_only_direct_helpers.rs")
        .collect()
}

fn forbidden_internal_support_paths() -> HashSet<String> {
    [
        hidden_literal(&["support/", "featureforge.rs"]),
        hidden_literal(&["support/", "internal_runtime_phase_handoff.rs"]),
        hidden_literal(&["support/", "plan_execution_direct.rs"]),
        hidden_literal(&["support/", "workflow_direct.rs"]),
        hidden_literal(&["support/", "internal_runtime_direct.rs"]),
    ]
    .into_iter()
    .collect()
}

fn path_attr_value(attr: &syn::Attribute) -> Option<String> {
    if !attr.path().is_ident("path") {
        return None;
    }
    let syn::Meta::NameValue(name_value) = &attr.meta else {
        return None;
    };
    let syn::Expr::Lit(expr_lit) = &name_value.value else {
        return None;
    };
    let syn::Lit::Str(lit) = &expr_lit.lit else {
        return None;
    };
    Some(lit.value())
}

fn public_runtime_flow_test_files() -> &'static HashSet<String> {
    static PUBLIC_RUNTIME_FLOW_TEST_FILES: OnceLock<HashSet<String>> = OnceLock::new();
    PUBLIC_RUNTIME_FLOW_TEST_FILES.get_or_init(|| {
        let script = read_repo_file("scripts/run-public-runtime-flow-tests.sh");
        public_runtime_flow_test_binaries_from_script(&script)
            .into_iter()
            .map(|binary| format!("tests/{binary}.rs"))
            .collect()
    })
}

fn public_runtime_flow_test_binaries_from_script(script: &str) -> Vec<String> {
    let mut binaries = Vec::new();
    let mut tokens = script.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "--test"
            && let Some(binary) = tokens.next()
        {
            binaries.push(binary.trim_matches('\\').to_owned());
        }
    }
    binaries.sort();
    binaries.dedup();
    binaries
}

fn function_name_for_line(
    spans: &[rust_source_scan::RustFunctionSpan],
    line: usize,
) -> Option<&str> {
    spans
        .iter()
        .rev()
        .find(|span| line >= span.start_line && line <= span.end_line)
        .map(|span| span.name.as_str())
}

fn call_matches_name(call: &rust_source_scan::RustCallPath, function_name: &str) -> bool {
    let normalized_leaf = call.path.rsplit("::").next().unwrap_or(call.path.as_str());
    let raw_leaf = call
        .raw_path
        .rsplit("::")
        .next()
        .unwrap_or(call.raw_path.as_str());
    normalized_leaf == function_name || raw_leaf == function_name
}

fn call_display_name(call: &rust_source_scan::RustCallPath) -> &str {
    call.raw_path
        .rsplit("::")
        .next()
        .unwrap_or(call.raw_path.as_str())
}

#[derive(Debug)]
struct RustFunctionBody<'a> {
    name: String,
    start_line: usize,
    lines: Vec<&'a str>,
}

fn tainted_runtime_helper_wrappers(
    rel: &str,
    source: &str,
    denied_helper_names: &[String],
) -> HashSet<String> {
    let functions = rust_function_bodies(rel, source);
    let call_hits = rust_source_scan::normalized_call_path_hits(rel, source, &[]);
    let mut tainted = functions
        .iter()
        .filter(|function| !function_scope_allows_internal_helpers(rel, &function.name))
        .filter(|function| function_calls_any(&call_hits, function, denied_helper_names))
        .map(|function| function.name.clone())
        .collect::<HashSet<_>>();

    let mut changed = true;
    while changed {
        changed = false;
        for function in &functions {
            if tainted.contains(&function.name) {
                continue;
            }
            if function_scope_allows_internal_helpers(rel, &function.name) {
                continue;
            }
            let tainted_names = tainted.iter().cloned().collect::<Vec<_>>();
            if function_calls_any(&call_hits, function, &tainted_names) {
                changed |= tainted.insert(function.name.clone());
            }
        }
    }

    tainted
}

fn public_tainted_runtime_helper_wrappers(
    rel: &str,
    source: &str,
    denied_helper_names: &[String],
) -> Vec<(usize, String)> {
    let call_hits = rust_source_scan::normalized_call_path_hits(rel, source, &[]);
    rust_function_bodies(rel, source)
        .into_iter()
        .filter(|function| !function_scope_allows_internal_helpers(rel, &function.name))
        .filter(|function| function_calls_any(&call_hits, function, denied_helper_names))
        .map(|function| (function.start_line, function.name))
        .collect()
}

fn public_direct_runtime_surface_wrappers(rel: &str, source: &str) -> Vec<(usize, String, String)> {
    let call_hits = rust_source_scan::normalized_call_path_hits(rel, source, &[]);
    rust_function_bodies(rel, source)
        .into_iter()
        .filter(|function| !function_scope_allows_internal_helpers(rel, &function.name))
        .filter_map(|function| {
            call_hits
                .iter()
                .filter(|call| {
                    call.line >= function.start_line
                        && call.line < function.start_line + function.lines.len()
                })
                .find_map(direct_runtime_surface_marker)
                .map(|marker| (function.start_line, function.name, marker))
        })
        .collect()
}

fn function_calls_any(
    call_hits: &[rust_source_scan::RustCallPath],
    function: &RustFunctionBody<'_>,
    names: &[String],
) -> bool {
    names
        .iter()
        .any(|name| function_calls_name(call_hits, function, name))
}

fn function_calls_name(
    call_hits: &[rust_source_scan::RustCallPath],
    function: &RustFunctionBody<'_>,
    name: &str,
) -> bool {
    call_hits.iter().any(|call| {
        call.line >= function.start_line
            && call.line < function.start_line + function.lines.len()
            && call_matches_name(call, name)
    })
}

fn direct_runtime_surface_marker(call: &rust_source_scan::RustCallPath) -> Option<String> {
    for marker in [
        "operator_for_runtime",
        "doctor_for_runtime",
        "doctor_for_runtime_with_args",
        "doctor_phase_and_next_for_runtime_with_args",
        "phase_for_runtime",
        "handoff_for_runtime",
        "status_refresh",
    ] {
        if call_matches_name(call, marker) {
            return Some(marker.to_owned());
        }
    }
    call.receiver_runtime_path.as_ref()?;
    ["status", "review_gate", "finish_gate"]
        .into_iter()
        .find(|marker| call_matches_name(call, marker))
        .map(str::to_owned)
}

fn denied_helper_names(source: &str, denied_helper_calls: &[String]) -> Vec<String> {
    let mut names = denied_helper_calls
        .iter()
        .map(|call| call.trim_end_matches('(').to_owned())
        .chain(quarantine_prefixed_helper_names(source))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn quarantine_prefixed_helper_names(source: &str) -> Vec<String> {
    let code = line_without_string_literals(source);
    identifier_tokens(&code)
        .filter(|identifier| identifier.starts_with("internal_only_"))
        .map(str::to_owned)
        .collect()
}

#[derive(Debug)]
struct PendingAssignment {
    binding: String,
    start_line: usize,
    is_arg_collection: bool,
    saw_hidden_value: bool,
    saw_hidden_arg_collection: bool,
}

impl PendingAssignment {
    fn observe_line(
        &mut self,
        trimmed: &str,
        saw_hidden_value: bool,
        saw_hidden_arg_collection: bool,
    ) {
        self.is_arg_collection |= assignment_binds_arg_collection(trimmed);
        self.saw_hidden_value |= saw_hidden_value;
        self.saw_hidden_arg_collection |= saw_hidden_arg_collection;
    }
}

fn finalize_assignment(
    rel: &str,
    current_scope: &str,
    assignment: &PendingAssignment,
    hidden_string_bindings: &mut HashSet<String>,
    hidden_arg_bindings: &mut HashSet<String>,
    violations: &mut Vec<String>,
) {
    if assignment.saw_hidden_value {
        if assignment.is_arg_collection {
            hidden_arg_bindings.insert(assignment.binding.clone());
        } else {
            hidden_string_bindings.insert(assignment.binding.clone());
        }
        violations.push(format!(
            "{rel}:{} binds hidden command or flag data to `{}` outside an internal-only quarantine or test in `{current_scope}`",
            assignment.start_line, assignment.binding
        ));
    } else if assignment.is_arg_collection && assignment.saw_hidden_arg_collection {
        hidden_arg_bindings.insert(assignment.binding.clone());
        violations.push(format!(
            "{rel}:{} aliases hidden command arg collection as `{}` outside an internal-only quarantine or test in `{current_scope}`",
            assignment.start_line, assignment.binding
        ));
    }
}

fn assignment_ends(trimmed: &str) -> bool {
    trimmed.ends_with(';')
}

fn rust_function_bodies<'a>(rel: &str, source: &'a str) -> Vec<RustFunctionBody<'a>> {
    let lines = source.lines().collect::<Vec<_>>();
    rust_source_scan::function_spans(rel, source)
        .into_iter()
        .map(|span| RustFunctionBody {
            name: span.name,
            start_line: span.start_line,
            lines: lines[(span.start_line - 1)..span.end_line].to_vec(),
        })
        .collect()
}

#[derive(Debug)]
struct HiddenLiteralHit {
    literal: String,
    always_hidden: bool,
}

fn hidden_literal_hits(
    candidate_literals: &[CandidateLiteral],
    denied_hidden_literals: &[String],
) -> Vec<HiddenLiteralHit> {
    let mut hits = Vec::new();
    for literal in denied_hidden_literals {
        for candidate in candidate_literals {
            if candidate.value == *literal {
                hits.push(HiddenLiteralHit {
                    literal: literal.clone(),
                    always_hidden: candidate.kind == CandidateLiteralKind::Raw,
                });
                break;
            }
            if shell_command_mentions_hidden_literal(&candidate.value, literal)
                || literal.starts_with("--")
                    && shell_words(&candidate.value).any(|word| word == literal)
            {
                hits.push(HiddenLiteralHit {
                    literal: literal.clone(),
                    always_hidden: candidate.kind == CandidateLiteralKind::Raw,
                });
                break;
            }
        }
    }
    hits
}

fn hidden_identifier_hits(line: &str, hidden_bindings: &HashSet<String>) -> Vec<String> {
    identifier_tokens(line)
        .filter(|identifier| hidden_bindings.contains(*identifier))
        .map(str::to_owned)
        .collect()
}

fn identifier_tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split(|character: char| !is_rust_identifier_character(character))
        .filter(|token| {
            !token.is_empty() && !token.chars().all(|character| character.is_ascii_digit())
        })
}

fn is_rust_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn assignment_binding_name(trimmed: &str) -> Option<String> {
    let rest = trimmed
        .strip_prefix("const ")
        .or_else(|| trimmed.strip_prefix("static "))
        .or_else(|| trimmed.strip_prefix("let "))?;
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let name = rest.split([':', '=', '[', ' ', '\t', ',', ';']).next()?;
    if name.is_empty() || !name.chars().all(is_rust_identifier_character) {
        return None;
    }
    Some(name.to_owned())
}

fn assignment_can_bind_hidden_data(trimmed: &str) -> bool {
    let Some((left, right)) = trimmed.split_once('=') else {
        return false;
    };
    let right = right.trim_start();
    if left.contains("&str") || left.contains("String") {
        return true;
    }
    right.is_empty()
        || right.starts_with('"')
        || right.starts_with("concat!(")
        || right.starts_with('[')
        || right.starts_with("&[")
        || right.starts_with("vec![")
}

fn assignment_binds_arg_collection(trimmed: &str) -> bool {
    trimmed.contains('[') || trimmed.contains("vec![")
}

fn arg_collection_mutation_binding(trimmed: &str) -> Option<String> {
    [".push(", ".extend(", ".extend_from_slice("]
        .iter()
        .find_map(|needle| {
            let (receiver, _) = trimmed.split_once(needle)?;
            receiver
                .split(|character: char| !is_rust_identifier_character(character))
                .rfind(|token| !token.is_empty())
                .map(str::to_owned)
        })
}

fn shell_command_mentions_hidden_literal(string_literal: &str, literal: &str) -> bool {
    [
        format!("featureforge plan execution {literal}"),
        format!("plan execution {literal}"),
        format!("featureforge workflow {literal}"),
        format!("workflow {literal}"),
    ]
    .iter()
    .any(|needle| string_literal.contains(needle))
}

fn shell_words(string_literal: &str) -> impl Iterator<Item = &str> {
    string_literal.split_whitespace().map(|word| {
        word.trim_matches(|character: char| {
            matches!(
                character,
                ',' | ';' | ':' | ')' | '(' | '[' | ']' | '{' | '}' | '\'' | '"' | '`'
            )
        })
    })
}

#[derive(Default)]
struct ConcatLiteralCollector {
    active: bool,
    depth: usize,
    value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CandidateLiteralKind {
    Raw,
    Concat,
}

#[derive(Clone, Debug)]
struct CandidateLiteral {
    value: String,
    kind: CandidateLiteralKind,
}

fn candidate_string_literals(
    line: &str,
    concat_collector: &mut ConcatLiteralCollector,
) -> Vec<CandidateLiteral> {
    let mut candidates = string_literals(line)
        .into_iter()
        .map(|value| CandidateLiteral {
            value,
            kind: CandidateLiteralKind::Raw,
        })
        .collect::<Vec<_>>();
    candidates.extend(
        concat_collector
            .collect(line)
            .into_iter()
            .map(|value| CandidateLiteral {
                value,
                kind: CandidateLiteralKind::Concat,
            }),
    );
    candidates
}

impl ConcatLiteralCollector {
    fn collect(&mut self, line: &str) -> Vec<String> {
        let mut values = Vec::new();
        let mut cursor = 0;
        while cursor < line.len() {
            if self.active {
                cursor = self.collect_active(line, cursor, &mut values);
                continue;
            }
            let Some(relative_start) = line[cursor..].find("concat!(") else {
                break;
            };
            let start = cursor + relative_start;
            self.active = true;
            self.depth = 1;
            self.value.clear();
            cursor = self.collect_active(line, start + "concat!(".len(), &mut values);
        }
        values
    }

    fn collect_active(&mut self, line: &str, mut cursor: usize, values: &mut Vec<String>) -> usize {
        let bytes = line.as_bytes();
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'"' => {
                    cursor += 1;
                    while cursor < bytes.len() {
                        match bytes[cursor] {
                            b'\\' => {
                                if cursor + 1 < bytes.len() {
                                    self.value.push(bytes[cursor + 1] as char);
                                    cursor += 2;
                                } else {
                                    cursor += 1;
                                }
                            }
                            b'"' => {
                                cursor += 1;
                                break;
                            }
                            byte => {
                                self.value.push(byte as char);
                                cursor += 1;
                            }
                        }
                    }
                }
                b'(' => {
                    self.depth += 1;
                    cursor += 1;
                }
                b')' => {
                    self.depth = self.depth.saturating_sub(1);
                    cursor += 1;
                    if self.depth == 0 {
                        values.push(self.value.clone());
                        self.active = false;
                        self.value.clear();
                        return cursor;
                    }
                }
                _ => {
                    cursor += 1;
                }
            }
        }
        cursor
    }
}

fn starts_command_invocation(trimmed: &str) -> bool {
    !trimmed.starts_with("fn ")
        && [
            "run_featureforge(",
            "run_featureforge_json(",
            "run_featureforge_real_cli(",
            "run_featureforge_with_env_json(",
            "run_featureforge_json_real_cli(",
            "run_plan_execution(",
            "run_plan_execution_json(",
            "run_plan_execution_json_real_cli(",
            concat!("internal_only_", "plan_execution_fixture_json("),
            "run_public_featureforge_cli_json(",
            "run_public_featureforge_cli_failure_json(",
            "run_public_cli(",
            "run_shell(",
            "run_shell_json(",
            "run_rust(",
            "run_rust_json(",
            "run_rust_with_env(",
            ".arg(",
            ".args(",
            ".args([",
            ".args(&[",
            ".push(",
            ".extend(",
            ".extend_from_slice(",
        ]
        .iter()
        .any(|needle| trimmed.contains(needle))
}

fn starts_command_args_array(trimmed: &str) -> bool {
    trimmed.starts_with("&[")
        || trimmed.starts_with('[')
        || trimmed.contains(".args([")
        || trimmed.contains(".args(&[")
}

fn contains_inline_command_args_array(trimmed: &str) -> bool {
    trimmed.contains("&[") || trimmed.contains(".args([") || trimmed.contains(".args(&[")
}

fn ends_command_args_array(trimmed: &str) -> bool {
    trimmed.starts_with(']') || trimmed.contains("],") || trimmed.contains("])")
}

fn string_literals(line: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some((_, character)) = chars.next() {
        if character != '"' {
            continue;
        }
        let mut literal = String::new();
        let mut escaped = false;
        for (_, next) in chars.by_ref() {
            if escaped {
                literal.push(next);
                escaped = false;
                continue;
            }
            match next {
                '\\' => escaped = true,
                '"' => {
                    literals.push(literal);
                    break;
                }
                _ => literal.push(next),
            }
        }
    }
    literals
}

fn line_without_string_literals(line: &str) -> String {
    let mut stripped = String::with_capacity(line.len());
    let mut chars = line.chars();
    let mut in_string = false;
    let mut escaped = false;
    for character in chars.by_ref() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
                stripped.push('"');
            } else {
                stripped.push(' ');
            }
            continue;
        }
        if character == '"' {
            in_string = true;
        }
        stripped.push(character);
    }
    stripped
}

fn denied_helper_calls() -> Vec<String> {
    vec![
        concat!("run_", "rust_featureforge(").to_owned(),
        concat!("run_", "rust_featureforge_with_env_control(").to_owned(),
        concat!("try_run_", "plan_execution_output_direct(").to_owned(),
        concat!("try_run_", "root_output_direct(").to_owned(),
        concat!("try_run_", "workflow_output_direct(").to_owned(),
        concat!("internal_only_try_run_", "plan_execution_output_direct(").to_owned(),
        concat!("internal_only_try_run_", "root_output_direct(").to_owned(),
        concat!("internal_only_try_run_", "workflow_output_direct(").to_owned(),
        concat!("internal_only_run_", "featureforge_direct_or_cli(").to_owned(),
        concat!(
            "internal_only_run_",
            "featureforge_with_env_control_direct_or_cli("
        )
        .to_owned(),
        concat!("internal_only_run_", "rust_direct_or_cli(").to_owned(),
        concat!("internal_only_run_", "rust_json_direct_or_cli(").to_owned(),
        concat!("internal_only_run_", "plan_execution_json_direct_or_cli(").to_owned(),
        hidden_literal(&["internal_only_runtime_", "pre", "flight_gate_json("]),
        concat!("internal_only_runtime_", "review_gate_json(").to_owned(),
        concat!("internal_only_runtime_", "finish_gate_json(").to_owned(),
        concat!("internal_only_runtime_", "review_dispatch_authority_json(").to_owned(),
        hidden_literal(&["internal_only_unit_", "plan_execution_pre", "flight_json("]),
        concat!("internal_only_", "plan_execution_fixture_json(").to_owned(),
        hidden_literal(&[
            "internal_only_",
            "compatibility_",
            "workflow_pre",
            "flight_json(",
        ]),
        hidden_literal(&[
            "internal_only_",
            "compatibility_",
            "workflow_gate_review_json(",
        ]),
        hidden_literal(&[
            "internal_only_",
            "compatibility_",
            "workflow_gate_finish_json(",
        ]),
        hidden_literal(&["internal_only_workflow_", "pre", "flight_output("]),
        concat!("internal_only_workflow_", "gate_review_output(").to_owned(),
        concat!("internal_only_workflow_", "gate_finish_output(").to_owned(),
        concat!("concrete_public_", "command_args(").to_owned(),
        concat!("materialize_public_", "command_template(").to_owned(),
        concat!("fill_public_argv_", "template_value(").to_owned(),
    ]
}

fn denied_hidden_literals() -> Vec<String> {
    vec![
        hidden_literal(&["pre", "flight"]),
        hidden_literal(&["gate", "-review"]),
        hidden_literal(&["gate", "-finish"]),
        hidden_literal(&["record", "-review-dispatch"]),
        hidden_literal(&["record", "-branch-closure"]),
        hidden_literal(&["record", "-release-readiness"]),
        hidden_literal(&["record", "-final-review"]),
        hidden_literal(&["record", "-qa"]),
        hidden_literal(&["rebuild", "-evidence"]),
        hidden_literal(&["explain", "-review-state"]),
        hidden_literal(&["reconcile", "-review-state"]),
        hidden_literal(&["--dispatch", "-id"]),
        hidden_literal(&["--branch", "-closure-id"]),
        hidden_literal(&["FEATUREFORGE", "_ALLOW_INTERNAL_EXECUTION_FLAGS"]),
    ]
}

fn public_diagnostic_forbidden_patterns() -> Vec<String> {
    vec![
        hidden_literal(&["retry gate", "-review"]),
        hidden_literal(&["retry gate", "-finish"]),
        "rebuild the execution evidence".to_owned(),
        "record a dedicated-independent serial unit-review receipt".to_owned(),
        "record the authoritative unit-review receipt".to_owned(),
        "repair the authoritative unit-review receipt".to_owned(),
        "restore authoritative unit-review receipt".to_owned(),
        "restore the authoritative unit-review receipt".to_owned(),
    ]
}

fn task4_denied_public_control_plane_terms() -> Vec<String> {
    vec![
        hidden_literal(&["record", "-review-dispatch"]),
        hidden_literal(&["gate", "-review"]),
        hidden_literal(&["gate", "-finish"]),
        hidden_literal(&["rebuild", "-evidence"]),
        hidden_literal(&["record", "-branch-closure"]),
        hidden_literal(&["record", "-final-review"]),
        hidden_literal(&["record", "-qa"]),
        hidden_literal(&["plan", "_fidelity_receipt"]),
        hidden_literal(&["plan", "-fidelity-receipt"]),
        hidden_literal(&["Plan", "-fidelity receipt"]),
        hidden_literal(&["workflow", " preflight"]),
        hidden_literal(&["workflow", " recommend"]),
        hidden_literal(&["plan", " execution preflight"]),
        hidden_literal(&["plan", " execution recommend"]),
        hidden_literal(&["execution", "-preflight-acceptance"]),
    ]
}

fn task4_active_public_surface_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = vec![root.join("AGENTS.md"), root.join("README.md")];
    collect_files_with_extensions(&root.join("skills"), &["md", "tmpl"], &mut files);
    collect_files_with_extensions(&root.join("references"), &["md"], &mut files);
    collect_active_doc_files(&root.join("docs"), &mut files);
    collect_active_doc_files(&root.join("review"), &mut files);
    files.sort();
    files.dedup();
    files
}

fn internal_compatibility_active_doc_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = Vec::new();
    collect_root_active_doc_files(&mut files);
    collect_files_with_extensions(&root.join("skills"), &["md", "tmpl"], &mut files);
    collect_files_with_extensions(&root.join("references"), &["md"], &mut files);
    collect_active_doc_files(&root.join("docs"), &mut files);
    collect_active_doc_files(&root.join("review"), &mut files);
    files.sort();
    files.dedup();
    files
}

fn internal_compatibility_hidden_surface_terms() -> Vec<String> {
    vec![
        hidden_literal(&["--dispatch", "-id"]),
        hidden_literal(&["--branch", "-closure-id"]),
        hidden_literal(&["FEATUREFORGE", "_ALLOW_INTERNAL_EXECUTION_FLAGS"]),
    ]
}

fn diagnostic_pattern_violations_for_source(
    rel: &str,
    source: &str,
    forbidden_patterns: &[String],
) -> Vec<String> {
    let mut violations = Vec::new();
    let source_lower = source.to_ascii_lowercase();
    let historical_comment_ranges = if rel.ends_with(".rs") {
        rust_historical_comment_ranges(source)
    } else {
        Vec::new()
    };

    for pattern in forbidden_patterns {
        let pattern_lower = pattern.to_ascii_lowercase();
        for (start, _) in source_lower.match_indices(&pattern_lower) {
            let end = start + pattern_lower.len();
            if !range_is_inside_any(start..end, &historical_comment_ranges) {
                violations.push(format!(
                    "{rel}:{} public diagnostics/docs must route through workflow operator, repair-review-state, close-current-task, or advance-late-stage instead of `{pattern}`",
                    line_number_for_byte(source, start)
                ));
            }
        }
    }
    violations
}

fn rust_historical_comment_ranges(source: &str) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if let Some(raw_string_end) = rust_raw_string_end(bytes, index) {
            index = raw_string_end;
            continue;
        }

        if bytes[index] == b'"' {
            index = rust_quoted_string_end(bytes, index);
            continue;
        }

        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            let start = index;
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            if rust_comment_is_explicitly_historical(&source[start..index]) {
                ranges.push(start..index);
            }
            continue;
        }

        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let start = index;
            index = rust_block_comment_end(bytes, index);
            if rust_comment_is_explicitly_historical(&source[start..index]) {
                ranges.push(start..index);
            }
            continue;
        }

        index += 1;
    }

    ranges
}

fn rust_raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let raw_prefix_index = if bytes.get(start) == Some(&b'r') {
        start
    } else if matches!(bytes.get(start), Some(b'b' | b'c')) && bytes.get(start + 1) == Some(&b'r') {
        start + 1
    } else {
        return None;
    };

    let mut quote_index = raw_prefix_index + 1;
    while bytes.get(quote_index) == Some(&b'#') {
        quote_index += 1;
    }
    if bytes.get(quote_index) != Some(&b'"') {
        return None;
    }

    let hashes = quote_index - raw_prefix_index - 1;
    let mut index = quote_index + 1;
    while index < bytes.len() {
        if bytes[index] == b'"'
            && bytes
                .get(index + 1..index + 1 + hashes)
                .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
        {
            return Some(index + 1 + hashes);
        }
        index += 1;
    }

    Some(bytes.len())
}

fn rust_quoted_string_end(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 1;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return index + 1;
        }
        index += 1;
    }
    bytes.len()
}

fn rust_block_comment_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 1usize;
    let mut index = start + 2;
    while index + 1 < bytes.len() {
        if bytes[index] == b'/' && bytes[index + 1] == b'*' {
            depth += 1;
            index += 2;
        } else if bytes[index] == b'*' && bytes[index + 1] == b'/' {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return index;
            }
        } else {
            index += 1;
        }
    }
    bytes.len()
}

fn rust_comment_is_explicitly_historical(comment: &str) -> bool {
    let trimmed = comment.trim_start();
    let body = if let Some(body) = trimmed.strip_prefix("//") {
        body
    } else if let Some(body) = trimmed.strip_prefix("/*") {
        body
    } else {
        return false;
    };
    let body = body.trim_start_matches(['/', '*', '!']).trim_start();
    body.to_ascii_lowercase().starts_with("historical")
}

fn range_is_inside_any(range: Range<usize>, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|allowed| range.start >= allowed.start && range.end <= allowed.end)
}

fn line_number_for_byte(source: &str, byte_index: usize) -> usize {
    source[..byte_index]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn production_source_and_active_doc_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_root_active_doc_files(&mut files);
    collect_files_with_extensions(&repo_root().join("src"), &["rs"], &mut files);
    collect_files_with_extensions(&repo_root().join("skills"), &["md", "tmpl"], &mut files);
    collect_files_with_extensions(&repo_root().join("references"), &["md"], &mut files);
    collect_active_doc_files(&repo_root().join("docs"), &mut files);
    collect_active_doc_files(&repo_root().join("review"), &mut files);
    files.sort();
    files.dedup();
    files
}

fn collect_root_active_doc_files(files: &mut Vec<PathBuf>) {
    let root = repo_root();
    for entry in fs::read_dir(&root).unwrap_or_else(|error| {
        panic!("{} should be readable: {error}", root.display());
    }) {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.ends_with(".md") || name.ends_with(".md.tmpl") {
            files.push(path);
        }
    }
}

fn collect_active_doc_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if dir.file_name().and_then(|name| name.to_str()) == Some("archive") {
        return;
    }
    collect_files_with_extensions(dir, &["md"], files);
}

fn rust_test_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_test_files(root, &mut files);
    files.sort();
    files
}

fn top_level_rust_test_files(root: &Path) -> Vec<PathBuf> {
    let mut files = fs::read_dir(root)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", root.display()))
        .map(|entry| {
            entry
                .expect("test directory entry should be readable")
                .path()
        })
        .filter(|path| {
            path.is_file()
                && path.extension().and_then(|extension| extension.to_str()) == Some("rs")
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn internal_compatibility_function_names(rel: &str, source: &str) -> Vec<String> {
    let syntax = rust_source_scan::parse_rust_source(rel, source);
    let mut names = Vec::new();
    collect_item_function_names_including_cfg_test(&syntax.items, &mut names);
    names.sort();
    names.dedup();
    let compatibility_prefix = hidden_literal(&["internal_only_", "compatibility_"]);
    let fs_prefix = hidden_literal(&["internal_only_", "fs"]);
    names
        .into_iter()
        .filter(|name| name.starts_with(&compatibility_prefix) || name.starts_with(&fs_prefix))
        .collect()
}

fn collect_item_function_names_including_cfg_test(items: &[syn::Item], names: &mut Vec<String>) {
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => names.push(item_fn.sig.ident.to_string()),
            syn::Item::Mod(item_mod) => {
                if let Some((_, nested_items)) = &item_mod.content {
                    collect_item_function_names_including_cfg_test(nested_items, names);
                }
            }
            _ => {}
        }
    }
}

fn collect_files_with_extensions(dir: &Path, extensions: &[&str], files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|error| {
        panic!("{} should be readable: {error}", dir.display());
    }) {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("archive") {
                continue;
            }
            collect_files_with_extensions(&path, extensions, files);
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extensions.contains(&extension))
        {
            files.push(path);
        }
    }
}

fn production_command_authority_files() -> Vec<PathBuf> {
    let mut files = [
        "src/execution/command_eligibility.rs",
        "src/execution/mutate.rs",
        "src/execution/next_action.rs",
        "src/execution/query.rs",
        "src/execution/read_model.rs",
        "src/execution/review_state.rs",
        "src/execution/router.rs",
        "src/execution/state.rs",
        "src/execution/state/runtime_methods.rs",
        "src/workflow/operator.rs",
        "src/workflow/status.rs",
    ]
    .into_iter()
    .map(|relative| repo_root().join(relative))
    .collect::<Vec<_>>();
    files.extend(rust_test_files(&repo_root().join("src/execution/commands")));
    files.sort();
    files.dedup();
    files
}

fn collect_rust_test_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|error| {
        panic!("{} should be readable: {error}", dir.display());
    }) {
        let entry = entry.expect("test directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            collect_rust_test_files(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

fn repo_relative(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
