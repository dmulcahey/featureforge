#[path = "support/rust_source_scan.rs"]
mod rust_source_scan;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
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

    let read_model = read_repo_file("src/execution/read_model.rs");
    let route_target_start = read_model
        .find("fn route_exposes_repair_review_state_target")
        .expect("read model should expose public route repair target projection");
    let route_target_end = read_model[route_target_start..]
        .find("\nfn should_preserve_local_preflight_route")
        .map(|offset| route_target_start + offset)
        .expect("read model target projection slice should have a stable following helper");
    let route_target_projection = &read_model[route_target_start..route_target_end];
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
            "recommended_public_command_is(status, PublicCommandKind::RepairReviewState)"
        ) && route_target_projection
            .contains("recommended_public_command_is(status, PublicCommandKind::AdvanceLateStage)"),
        "read-model public repair target projection should classify typed PublicCommand variants"
    );

    let transfer = read_repo_file("src/execution/commands/transfer.rs");
    assert!(
        transfer.contains("recommended_public_command_argv"),
        "transfer output should expose argv with any follow-up command"
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
fn public_test_files_do_not_use_internal_helpers_or_hidden_commands() {
    let mut violations = Vec::new();
    for file in rust_test_files(&repo_root().join("tests")) {
        let rel = repo_relative(&file);
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", file.display()));
        violations.extend(scan_source_for_public_flow_violations(&rel, &source));
    }

    assert!(
        violations.is_empty(),
        "public-flow tests must not use internal helpers or hidden command literals:\n{}",
        violations.join("\n")
    );
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
    assert!(
        internal_script.contains("internal_only_compatibility"),
        "internal compatibility gate should run explicitly quarantined internal-only tests"
    );
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
fn scanner_rejects_internal_only_quarantine_inside_public_gate_suite() {
    let helper = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let hidden_command = hidden_literal(&["record", "-review-dispatch"]);
    let fixture = format!(
        "#[test]\nfn internal_only_compatibility_fixture() {{\n    {helper}\n    let _ = &[\"{hidden_command}\"];\n}}\n"
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
    for needle in [
        "fn fs11_rebase_resume_recovery_budget_is_capped_without_hidden_helpers()",
        "FS11-REBASE-RESUME-BUDGET",
        "runtime_management_commands, 3",
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
    let status_marker = hidden_literal(&[".status", "("]);
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

fn workflow_operator_json(runtime: &ExecutionRuntime, args: &OperatorArgs) {{
    operator::{operator_marker}runtime, args);
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
        "workflow_operator_json",
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
    let plan_execution_direct = hidden_literal(&["support/", "plan_execution_direct.rs"]);
    let workflow_direct = hidden_literal(&["support/", "workflow_direct.rs"]);
    let fixture = format!(
        r#"
#[path = "{featureforge_support}"]
mod featureforge_support;
#[path = "{internal_runtime_direct}"]
mod internal_runtime_direct;
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
fn internal_quarantine_bridge_imports_are_explicitly_reasoned() {
    for rel in [
        "tests/contracts_execution_runtime_boundaries.rs",
        "tests/execution_harness_state.rs",
        "tests/execution_query.rs",
        "tests/plan_execution.rs",
        "tests/plan_execution_topology.rs",
        "tests/workflow_runtime.rs",
        "tests/workflow_runtime_final_review.rs",
        "tests/workflow_shell_smoke.rs",
    ] {
        let reason = protected_internal_quarantine_import_exception_reason(rel)
            .expect("mixed internal-helper quarantine bridge import exception should be listed");
        assert!(
            reason.len() > 40 && reason.contains("internal"),
            "{rel} internal-helper quarantine bridge import exception should have a specific reason"
        );
    }
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
            "tests/plan_execution.rs",
            "assert_begin_blocks_cross_task_without_prior_task_closure",
        ),
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
    let plan_execution_direct = hidden_literal(&["support/", "plan_execution_direct.rs"]);
    let fixture = format!(
        r#"
#[path = "{internal_runtime_direct}"]
mod internal_runtime_direct;
#[path = "{plan_execution_direct}"]
mod plan_execution_direct_support;

#[test]
fn internal_only_compatibility_fixture() {{}}
"#
    );
    let violations =
        scan_source_for_public_flow_violations("tests/workflow_shell_smoke.rs", &fixture);

    for forbidden in [internal_runtime_direct, plan_execution_direct] {
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
fn scanner_allows_explicit_internal_only_test_quarantine() {
    let helper = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let hidden_command = hidden_literal(&["record", "-review-dispatch"]);
    let fixture = format!(
        "#[test]\nfn internal_only_compatibility_fixture() {{\n    {helper}\n    let _ = &[\"{hidden_command}\"];\n}}\n"
    );

    assert!(
        scan_source_for_public_flow_violations("tests/public_fixture.rs", &fixture).is_empty(),
        "internal_only_* test scopes are explicit test-level quarantines"
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

fn protected_internal_quarantine_import_exception_reason(rel: &str) -> Option<&'static str> {
    match rel {
        "tests/contracts_execution_runtime_boundaries.rs" => Some(
            "mixed boundary contract file keeps internal-only compatibility probes in internal_only_* tests",
        ),
        "tests/execution_harness_state.rs" => Some(
            "execution harness state coverage intentionally exercises direct runtime helpers in internal_only_* tests",
        ),
        "tests/execution_query.rs" => Some(
            "execution query boundary coverage intentionally compares direct internal probes with public surfaces",
        ),
        "tests/plan_execution.rs" => Some(
            "plan execution compatibility matrix intentionally exercises direct internal helpers in internal_only_* tests",
        ),
        "tests/plan_execution_topology.rs" => Some(
            "topology compatibility coverage intentionally exercises direct internal recommendation helpers",
        ),
        "tests/workflow_runtime.rs" => Some(
            "workflow runtime compatibility matrix intentionally exercises direct internal helpers in internal_only_* tests",
        ),
        "tests/workflow_runtime_final_review.rs" => Some(
            "final-review runtime compatibility matrix intentionally exercises direct internal helpers in internal_only_* tests",
        ),
        "tests/workflow_shell_smoke.rs" => Some(
            "workflow shell smoke compatibility matrix intentionally exercises direct internal helpers in internal_only_* tests",
        ),
        _ => None,
    }
}

fn explicit_internal_helper_scope_exception_reason(
    rel: &str,
    function_name: &str,
) -> Option<&'static str> {
    match (rel, function_name) {
        (
            "tests/plan_execution.rs",
            "assert_begin_blocks_cross_task_without_prior_task_closure",
        ) => Some(
            "public begin-boundary assertion uses internal preflight acceptance strictly as fixture setup",
        ),
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
        _ => None,
    }
}

fn function_scope_allows_internal_helpers(rel: &str, function_name: &str) -> bool {
    if public_runtime_flow_test_files().contains(rel) {
        return false;
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
    if protected_internal_quarantine_import_exception_reason(rel).is_some() {
        return Vec::new();
    }
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
    let functions = rust_function_bodies(source);
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
    rust_function_bodies(source)
        .into_iter()
        .filter(|function| !function_scope_allows_internal_helpers(rel, &function.name))
        .filter(|function| function_calls_any(&call_hits, function, denied_helper_names))
        .map(|function| (function.start_line, function.name))
        .collect()
}

fn public_direct_runtime_surface_wrappers(rel: &str, source: &str) -> Vec<(usize, String, String)> {
    let call_hits = rust_source_scan::normalized_call_path_hits(rel, source, &[]);
    rust_function_bodies(source)
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
    if call_matches_name(call, "operator_for_runtime") {
        return Some("operator_for_runtime".to_owned());
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

fn rust_function_bodies(source: &str) -> Vec<RustFunctionBody<'_>> {
    let lines = source.lines().collect::<Vec<_>>();
    rust_source_scan::function_spans("public-flow-scanner.rs", source)
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
            "internal_only_compatibility_",
            "workflow_pre",
            "flight_json(",
        ]),
        concat!("internal_only_compatibility_", "workflow_gate_review_json(").to_owned(),
        concat!("internal_only_compatibility_", "workflow_gate_finish_json(").to_owned(),
        hidden_literal(&["internal_only_workflow_", "pre", "flight_output("]),
        concat!("internal_only_workflow_", "gate_review_output(").to_owned(),
        concat!("internal_only_workflow_", "gate_finish_output(").to_owned(),
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

fn rust_test_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_test_files(root, &mut files);
    files.sort();
    files
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
