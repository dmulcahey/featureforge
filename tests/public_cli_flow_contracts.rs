#[path = "support/public_flow_scan.rs"]
mod public_flow_scan;
#[path = "support/rust_source_scan.rs"]
mod rust_source_scan;

use public_flow_scan::{
    PublicFlowExceptionCategory, collect_files_with_extensions, contains_bytes,
    denied_helper_calls, diagnostic_pattern_violations_for_source,
    event_log_fixture_authority_call_name, event_log_test_api_exception_category,
    file_name_is_internal_quarantine, internal_compatibility_function_names,
    is_protected_public_flow_file, is_public_flow_scanner_contract_file, line_number_for_byte,
    production_command_authority_files, production_command_authority_scan_subject,
    production_source_files, public_diagnostic_forbidden_patterns,
    public_diagnostic_hidden_command_token_patterns, public_flow_hidden_command_or_flag_literals,
    public_runtime_flow_required_test_binaries, public_runtime_flow_test_binaries_from_script,
    public_runtime_flow_test_files, repo_relative, rust_function_bodies, rust_test_files,
    scan_source_for_public_flow_violations, scan_stale_dispatch_public_flow_violations,
    token_only_blocked_follow_up_violations, top_level_rust_test_files,
};

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use featureforge::execution::phase;

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

fn source_for_public_diagnostic_hidden_token_scan(rel: &str, source: &str) -> String {
    if rel != "src/execution/command_eligibility.rs" {
        return source.to_owned();
    }

    let registry_start = source
        .find("pub const HIDDEN_COMMAND_OR_FLAG_TOKENS:")
        .unwrap_or_else(|| {
            panic!("{rel} should define the canonical hidden command token registry")
        });
    let registry_end = registry_start
        + source[registry_start..]
            .find("];")
            .unwrap_or_else(|| panic!("{rel} hidden command token registry should be an array"))
        + "];".len();

    let mut redacted = String::with_capacity(source.len());
    redacted.push_str(&source[..registry_start]);
    redacted.push_str("pub const HIDDEN_COMMAND_OR_FLAG_TOKENS: &[&str] = &[];\n");
    redacted.push_str(&source[registry_end..]);
    assert!(
        redacted.contains("pub fn hidden_command_or_flag_tokens()"),
        "{rel} redaction must preserve the public registry accessor"
    );
    redacted
}

fn assert_contains_all(content: &str, label: &str, fragments: &[&str]) {
    for fragment in fragments {
        assert!(
            content.contains(fragment),
            "{label} should include public contract fragment `{fragment}`"
        );
    }
}

fn public_runtime_route_golden() -> Value {
    serde_json::from_str(&read_repo_file(
        "tests/fixtures/runtime-goldens/public-runtime-routes.json",
    ))
    .expect("public runtime route golden should parse as JSON")
}

fn route_payload_for_surface(scenario: &Value, surface: &str) -> Option<Value> {
    if let Some(json) = scenario
        .get(surface)
        .and_then(|surface| surface.get("json"))
    {
        return Some(json.clone());
    }
    let semantics = scenario.get("route_semantics")?.as_object()?;
    let mut payload = semantics.clone();
    if let Some(surface_specific) = scenario
        .get("surface_specific")
        .and_then(|specific| specific.get(surface))
        .and_then(Value::as_object)
    {
        for (key, value) in surface_specific {
            payload.insert(key.clone(), value.clone());
        }
    }
    Some(Value::Object(payload))
}

fn public_runtime_route_payloads(golden: &Value) -> Vec<(String, Value)> {
    let scenarios = golden["scenarios"]
        .as_array()
        .expect("public runtime route golden should contain scenarios");
    let mut payloads = Vec::new();
    for scenario in scenarios {
        let label = scenario["label"]
            .as_str()
            .expect("runtime golden scenario should have a label");
        for surface in ["plan_execution_status", "workflow_operator"] {
            if let Some(json) = route_payload_for_surface(scenario, surface) {
                payloads.push((format!("{label}/{surface}"), json));
            }
        }
    }
    assert!(
        !payloads.is_empty(),
        "public runtime route golden should include route payloads"
    );
    payloads
}

fn public_runtime_route_payload(golden: &Value, label: &str, surface: &str) -> Value {
    let scenario = golden["scenarios"]
        .as_array()
        .expect("public runtime route golden should contain scenarios")
        .iter()
        .find(|scenario| scenario["label"].as_str() == Some(label))
        .unwrap_or_else(|| panic!("public runtime route golden should include `{label}`"));
    route_payload_for_surface(scenario, surface).unwrap_or_else(|| {
        panic!("public runtime route golden should include {label}/{surface} JSON")
    })
}

fn public_argv_tokens<'a>(context: &str, argv: &'a [Value]) -> Vec<&'a str> {
    argv.iter()
        .map(|token| {
            token
                .as_str()
                .unwrap_or_else(|| panic!("{context}: public argv entries must be strings"))
        })
        .collect()
}

fn hidden_public_argv_hits(tokens: &[&str]) -> Vec<String> {
    let joined = tokens.join(" ");
    public_flow_hidden_command_or_flag_literals()
        .into_iter()
        .filter(|hidden| joined.contains(hidden))
        .collect()
}

fn assert_public_argv_tokens(context: &str, argv: &[Value]) {
    let tokens = public_argv_tokens(context, argv);
    assert_eq!(
        tokens.first().copied(),
        Some("featureforge"),
        "{context}: public argv must be executable argv beginning with the shipped binary name"
    );
    let hidden_hits = hidden_public_argv_hits(&tokens);
    assert!(
        hidden_hits.is_empty(),
        "{context}: public argv must not expose hidden/debug command fragments {hidden_hits:?}: {tokens:?}"
    );
}

fn assert_public_command_template(context: &str, template: &Value) {
    let base_argv = template["base_argv"]
        .as_array()
        .unwrap_or_else(|| panic!("{context}: command template should expose base_argv"));
    assert_public_argv_tokens(context, base_argv);
    assert!(
        template["command_kind"]
            .as_str()
            .is_some_and(|kind| !kind.is_empty()),
        "{context}: command template should expose command_kind"
    );
    assert!(
        template["input_bindings"]
            .as_array()
            .is_some_and(|bindings| !bindings.is_empty()),
        "{context}: command template should expose parseable input_bindings"
    );
}

fn assert_required_inputs_match_template(context: &str, payload: &Value) {
    let required_inputs = payload["required_inputs"]
        .as_array()
        .unwrap_or_else(|| panic!("{context}: required_inputs should be an array"));
    assert!(
        !required_inputs.is_empty(),
        "{context}: required_inputs should name at least one missing input"
    );
    let template = payload
        .get("recommended_public_command_template")
        .unwrap_or_else(|| {
            panic!("{context}: required_inputs must be paired with a typed command template")
        });
    assert_public_command_template(context, template);

    let required_names = required_inputs
        .iter()
        .map(|input| {
            input["name"]
                .as_str()
                .unwrap_or_else(|| panic!("{context}: required input should have a name"))
        })
        .collect::<Vec<_>>();
    let template_names = template["required_input_names"]
        .as_array()
        .unwrap_or_else(|| panic!("{context}: template should expose required_input_names"))
        .iter()
        .map(|name| {
            name.as_str().unwrap_or_else(|| {
                panic!("{context}: template required_input_names entries should be strings")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        required_names, template_names,
        "{context}: required_inputs should match template required_input_names"
    );
}

fn assert_public_route_payload_uses_typed_command_authority(context: &str, payload: &Value) {
    assert!(
        payload.get("recommended_command").is_none(),
        "{context}: route goldens should omit display-only recommended_command"
    );
    assert!(
        payload.get("next_public_action").is_none(),
        "{context}: route goldens should omit display-only next_public_action"
    );
    let has_argv = payload.get("recommended_public_command_argv").is_some();
    let has_template = payload.get("recommended_public_command_template").is_some();
    assert!(
        !(has_argv && has_template),
        "{context}: route should expose executable argv or an input template, not both"
    );
    if let Some(argv) = payload
        .get("recommended_public_command_argv")
        .and_then(Value::as_array)
    {
        assert_public_argv_tokens(context, argv);
    }
    if let Some(template) = payload.get("recommended_public_command_template") {
        assert_public_command_template(context, template);
    }
    if payload.get("required_inputs").is_some() {
        assert_required_inputs_match_template(context, payload);
    }
}

fn assert_text_near(source: &str, anchor: &str, needle: &str, context: &str) {
    let anchor_index = source
        .find(anchor)
        .unwrap_or_else(|| panic!("{context}: missing anchor `{anchor}`"));
    let window_start = anchor_index.saturating_sub(1_000);
    let window_end = (anchor_index + anchor.len() + 1_000).min(source.len());
    assert!(
        source[window_start..window_end].contains(needle),
        "{context}: expected `{needle}` near `{anchor}`"
    );
}

#[test]
fn public_route_goldens_use_typed_command_authority_before_display_rendering() {
    let golden = public_runtime_route_golden();
    let normalization = golden["normalization"]
        .as_array()
        .expect("public runtime route golden should describe normalization")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert!(
        normalization
            .iter()
            .any(|rule| rule.contains("omit display-only command summaries")),
        "public route golden must explicitly omit display-only command summaries"
    );

    let mut argv_routes = 0;
    let mut template_routes = 0;
    let mut input_required_routes = 0;
    for (context, payload) in public_runtime_route_payloads(&golden) {
        assert_public_route_payload_uses_typed_command_authority(&context, &payload);
        argv_routes += payload.get("recommended_public_command_argv").is_some() as usize;
        template_routes += payload.get("recommended_public_command_template").is_some() as usize;
        input_required_routes += payload.get("required_inputs").is_some() as usize;
    }
    assert!(
        argv_routes > 0 && template_routes > 0 && input_required_routes > 0,
        "public route golden should cover executable argv routes, template routes, and input-required routes"
    );
}

#[test]
fn public_gate_required_inputs_are_paired_with_typed_command_templates() {
    let golden = public_runtime_route_golden();
    let mut checked = 0;
    for (context, payload) in public_runtime_route_payloads(&golden) {
        if payload.get("required_inputs").is_some() {
            assert_required_inputs_match_template(&context, &payload);
            checked += 1;
        }
    }
    assert!(
        checked >= 3,
        "public route golden should cover multiple input-required public routes, checked {checked}"
    );

    for rel in [
        "schemas/plan-execution-status.schema.json",
        "schemas/workflow-handoff.schema.json",
        "schemas/workflow-operator.schema.json",
    ] {
        let schema = read_repo_file(rel);
        assert_contains_all(
            &schema,
            rel,
            &[
                "recommended_public_command_template",
                "input_bindings",
                "required_inputs",
                "Non-executable public command template",
                "Input names required",
            ],
        );
    }
    let workflow_operator =
        public_runtime_route_payload(&golden, "qa_pending", "workflow_operator");
    assert_required_inputs_match_template("qa_pending/workflow_operator", &workflow_operator);
    assert_eq!(
        workflow_operator["recommended_public_command_template"]["command_kind"].as_str(),
        Some("advance_late_stage"),
        "input-required late-stage route should expose a typed command kind"
    );
}

#[test]
fn public_command_input_kind_schema_exposes_only_supported_materialized_kinds() {
    let expected = vec![
        Value::from("text"),
        Value::from("enum"),
        Value::from("path"),
    ];
    for rel in [
        "schemas/plan-execution-status.schema.json",
        "schemas/workflow-handoff.schema.json",
        "schemas/workflow-operator.schema.json",
    ] {
        let schema: Value = serde_json::from_str(&read_repo_file(rel))
            .unwrap_or_else(|error| panic!("{rel} should parse as JSON: {error}"));
        assert_eq!(
            schema["$defs"]["PublicCommandInputKind"]["enum"],
            Value::Array(expected.clone()),
            "{rel} should expose only command input kinds that materialize and validate through public_command_types.rs"
        );
    }

    let public_command_types = read_repo_file("src/execution/public_command_types.rs");
    for unsupported in ["Boolean", "RepeatableList"] {
        assert!(
            !public_command_types.contains(unsupported),
            "PublicCommandInputKind should not expose unsupported `{unsupported}` materialization semantics"
        );
    }
}

#[test]
fn display_command_parsing_is_test_only_not_route_authority() {
    let command_eligibility = read_repo_file("src/execution/command_eligibility.rs");
    assert!(
        command_eligibility.contains("#[cfg(test)]\n    pub(crate) fn parse_display_command"),
        "display-command parsing should be compiled only for public-command boundary tests"
    );

    let scan_subjects = production_command_authority_files()
        .iter()
        .map(|file| repo_relative(file))
        .collect::<Vec<_>>();
    for expected in [
        "src/execution/route_plan.rs",
        "src/execution/route_plan/status_application.rs",
        "src/execution/status_assembly.rs",
        "src/execution/status_assembly/overlay.rs",
        "src/execution/public_recovery.rs",
        "src/execution/commands/transfer.rs",
        "src/workflow/operator.rs",
        "src/workflow/status.rs",
    ] {
        assert!(
            scan_subjects.iter().any(|rel| rel == expected),
            "production command authority discovery should include {expected}; scanned: {scan_subjects:?}"
        );
    }
    for expected_exemption in [
        "src/execution/route_plan/unit_tests.rs",
        "src/execution/route_plan/next_action_choice/tests.rs",
        "src/execution/command_eligibility.rs",
        "src/execution/public_command_types.rs",
    ] {
        assert!(
            !production_command_authority_scan_subject(expected_exemption),
            "production command authority discovery should exempt {expected_exemption}"
        );
        assert!(
            !scan_subjects.iter().any(|rel| rel == expected_exemption),
            "production command authority discovery should not scan {expected_exemption}"
        );
    }

    let mut violations = Vec::new();
    for rel in scan_subjects {
        let file = repo_root().join(&rel);
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
        "src/execution/public_recovery.rs",
        "src/execution/review_state.rs",
    ];
    let mut violations = Vec::new();
    for rel in files {
        let source = read_repo_file(rel);
        violations.extend(token_only_blocked_follow_up_violations(rel, &source));
    }

    assert!(
        violations.is_empty(),
        "normal public blocked outputs must not strand agents on token-only follow-ups:\n{}",
        violations.join("\n")
    );

    let public_recovery = read_repo_file("src/execution/public_recovery.rs");
    for required in [
        "pub(crate) fn public_recovery_contract_for_follow_up",
        "workflow_operator_requery_optional_surfaces(plan, external_review_result_ready)",
        "PublicRecoveryContract::from_requery(plan, false, required_follow_up)",
        "contract_from_matching_operator",
        "PublicCommand::WorkflowOperator",
        "json: true",
    ] {
        assert!(
            public_recovery.contains(required),
            "public follow-up recovery should stay centralized through `{required}`"
        );
    }
    for forbidden_recovery_picker in [
        "fallback_public_recovery_contract",
        "repair_review_state_public_command(",
        "public_advance_late_stage_command_for_phase_detail(",
    ] {
        assert!(
            !public_recovery.contains(forbidden_recovery_picker),
            "blocked-output recovery must not synthesize commands outside the selected route via `{forbidden_recovery_picker}`"
        );
    }
    for forbidden in [
        "\"featureforge workflow operator --plan {}",
        "String::from(\"workflow\"),\n        String::from(\"operator\")",
    ] {
        assert!(
            !public_recovery.contains(forbidden),
            "workflow-operator JSON requery surfaces must use typed PublicCommand authority, not hand-built command fragments via `{forbidden}`"
        );
    }
}

#[test]
fn public_text_and_schemas_mark_recommended_command_as_display_only() {
    let workflow_operator = read_repo_file("src/workflow/operator.rs");
    assert!(
        !workflow_operator.contains("Recommended command:"),
        "workflow text renderers must not label display strings as recommended executable commands"
    );
    for forbidden in ["Next public action: {}", "Next public action:", "next={}"] {
        assert!(
            !workflow_operator.contains(forbidden),
            "workflow text renderers must not emit command-shaped action text without display-only labeling via `{forbidden}`"
        );
    }

    let command_eligibility = read_repo_file("src/execution/command_eligibility.rs");
    for required in ["typed_public_route=", "route_field=", "route_guidance="] {
        assert!(
            command_eligibility.contains(required),
            "JsonFailure mutation-oracle text should expose structured diagnostic route metadata via `{required}`"
        );
    }
    for forbidden in [
        "Next public action: {next_public_command}",
        "Next public action: featureforge",
    ] {
        assert!(
            !command_eligibility.contains(forbidden),
            "JsonFailure mutation-oracle text must not make display commands practical execution authority via `{forbidden}`"
        );
    }
    let review_state = read_repo_file("src/execution/review_state.rs");
    assert_contains_all(
        &review_state,
        "repair-review-state follow-up output",
        &["public transfer route", "before continuing"],
    );
    assert!(
        !review_state.contains("record a handoff before continuing"),
        "repair-review-state follow-up output must not sound like a retired handoff recorder command"
    );

    for rel in [
        "schemas/plan-execution-status.schema.json",
        "schemas/workflow-handoff.schema.json",
        "schemas/workflow-operator.schema.json",
    ] {
        let schema = read_repo_file(rel);
        assert_contains_all(
            &schema,
            rel,
            &[
                "Display-only",
                "not executable",
                "recommended_public_command_argv",
                "recommended_public_command_template",
                "input_bindings",
                "required_inputs",
                "Public command intent",
            ],
        );
    }
    for rel in [
        "schemas/plan-execution-status.schema.json",
        "schemas/workflow-handoff.schema.json",
    ] {
        let schema = read_repo_file(rel);
        assert_contains_all(
            &schema,
            rel,
            &[
                "Required follow-up intent token",
                "record_handoff",
                "compatibility metadata",
                "not executable",
            ],
        );
    }
}

#[test]
fn review_state_reconcile_output_does_not_use_recommended_command_for_prose() {
    let review_state = read_repo_file("src/execution/review_state.rs");

    assert!(
        review_state.contains("pub struct ReconcileReviewStateOutput"),
        "review-state reconciliation output should remain an explicit public DTO"
    );
    assert!(
        review_state.contains("pub operator_requery_instruction: String"),
        "review-state reconciliation prose should live in a non-command instruction field"
    );
    assert!(
        !review_state.contains("pub recommended_command: String"),
        "review-state reconciliation prose must not use a display-command compatibility field"
    );
    assert!(
        review_state
            .contains("operator_requery_instruction: reconcile_operator_rerun_instruction()"),
        "review-state reconciliation constructors should populate operator_requery_instruction"
    );
    assert!(
        review_state.contains("PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT"),
        "review-state reconciliation instructions should reuse the shared typed route contract"
    );
    assert!(
        !review_state.contains("RECONCILE_OPERATOR_RERUN_INSTRUCTION"),
        "review-state reconciliation must not grow a local duplicate typed route-law constant"
    );
    assert!(
        !review_state.contains("json: false"),
        "review-state fallback operator display commands should remain JSON-mode"
    );
    assert!(
        !review_state.contains("recommended_command: reconcile_operator_rerun_instruction()"),
        "review-state reconciliation constructors must not expose prose as recommended_command"
    );
}

#[test]
fn typed_route_law_prose_has_single_runtime_owner() {
    let public_route_guidance = read_repo_file("src/execution/public_route_guidance.rs");
    assert!(
        public_route_guidance.contains("PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT")
            && public_route_guidance.contains("WORKFLOW_OPERATOR_TEMPLATE_JSON_QUERY"),
        "public route guidance should own reusable typed-route law snippets"
    );

    let copied_route_law_fragments = [
        "when `recommended_public_command_template` needs `required_inputs`",
        "treat recommended_public_command_template.input_bindings as template metadata",
        "follow typed recommended_public_command_argv or same-plan operator materialization",
    ];
    for rel in [
        "scripts/gen-skill-docs.mjs",
        "src/execution/command_eligibility.rs",
        "src/execution/commands/common/mutation_guards.rs",
    ] {
        let source = read_repo_file(rel);
        for fragment in copied_route_law_fragments {
            assert!(
                !source.contains(fragment),
                "{rel} must consume shared route guidance or point to references/operator-route-authority.md instead of copying route-law prose fragment `{fragment}`"
            );
        }
    }

    let command_eligibility = read_repo_file("src/execution/command_eligibility.rs");
    assert!(
        command_eligibility.contains("EXECUTE_RECOMMENDED_PUBLIC_ARGV_GUIDANCE")
            && command_eligibility.contains("WORKFLOW_OPERATOR_TEMPLATE_JSON_QUERY")
            && command_eligibility.contains("OPERATOR_ROUTE_AUTHORITY_REFERENCE"),
        "command eligibility mutation-denial text should compose from shared route guidance constants"
    );
}

#[test]
fn projection_only_rebuild_labels_are_not_manual_runtime_progress() {
    let mutation_guards = read_repo_file("src/execution/commands/common/mutation_guards.rs");
    let rebuild_evidence = read_repo_file("src/execution/commands/rebuild_evidence.rs");
    let retired_projection_label = concat!("manual", "_required");

    for (label, source) in [
        ("mutation guards", mutation_guards.as_str()),
        ("rebuild-evidence command", rebuild_evidence.as_str()),
    ] {
        assert!(
            !source.contains(retired_projection_label),
            "{label} must not label projection-only rebuild diagnostics as manual runtime progress"
        );
        assert!(
            source.contains("projection_export_not_progress_route"),
            "{label} should use the projection-only failure class"
        );
    }
    assert!(
        mutation_guards.contains("projection_only: projection rebuild reports stale projection candidates without mutating runtime truth"),
        "projection-only rebuild diagnostics should explicitly say they are not runtime truth mutation"
    );
}

#[test]
fn public_text_surfaces_do_not_emit_compound_recording_or_failure_actions() {
    let mut rust_files = Vec::new();
    collect_files_with_extensions(&repo_root().join("src"), &["rs"], &mut rust_files);
    let forbidden = [
        "Next public action:",
        "record or refresh",
        "Record or refresh",
        "record/refresh",
        "recorded/refreshed",
        "Dispatch or record",
        "dispatch or record",
        "close-current-task or repair-review-state",
        "repair-review-state or close-current-task",
        "records final-review",
        "records finish-review",
        "records task-review",
        "records review-dispatch",
    ];
    let mut violations = Vec::new();
    for path in rust_files {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
        for needle in forbidden {
            if text.contains(needle) {
                violations.push(format!("{} contains `{needle}`", path.display()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "public runtime text must avoid command-shaped JsonFailure authority and compound recording prose:\n{}",
        violations.join("\n")
    );
}

#[test]
fn active_public_workflow_text_never_renders_plan_none() {
    let active_public_surfaces = [
        "src/workflow/operator.rs",
        "schemas/workflow-operator.schema.json",
        "schemas/workflow-handoff.schema.json",
        "tests/fixtures/runtime-goldens/public-runtime-routes.json",
    ];
    let forbidden = [
        hidden_literal(&["featureforge workflow operator --plan ", "none"]),
        hidden_literal(&["featureforge workflow doctor --plan ", "none"]),
        hidden_literal(&["--plan ", "none"]),
    ];
    let mut violations = Vec::new();
    for rel in active_public_surfaces {
        let source = read_repo_file(rel);
        for term in &forbidden {
            if let Some(byte_offset) = source.find(term) {
                violations.push(format!(
                    "{rel}:{} contains executable-looking no-plan guidance `{term}`",
                    line_number_for_byte(&source, byte_offset)
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "active public workflow text and goldens must not render a synthetic `none` plan path:\n{}",
        violations.join("\n")
    );
}

#[test]
fn rust_authored_remediations_do_not_consume_json_fields_from_text_operator() {
    let mut files = Vec::new();
    collect_files_with_extensions(&repo_root().join("src"), &["rs"], &mut files);
    let json_only_fields = [
        "phase",
        "phase_detail",
        "recommended_public_command_argv",
        "recommended_public_command_template",
        "required_inputs",
        "recording_context",
        "base_branch",
    ];
    let mut violations = Vec::new();

    for path in files {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
        for (start, _) in source.match_indices("workflow operator --plan") {
            let end = source[start..]
                .find('\n')
                .map(|offset| start + offset)
                .unwrap_or(source.len());
            let line = &source[start..end];
            if line.contains("--json") {
                continue;
            }
            if let Some(field) = json_only_fields.iter().find(|field| line.contains(*field)) {
                let rel = path.strip_prefix(repo_root()).unwrap_or(&path);
                violations.push(format!(
                    "{}:{} references `{field}` after text-mode workflow/operator: {}",
                    rel.display(),
                    line_number_for_byte(&source, start),
                    line.trim()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Rust-authored runtime guidance must use workflow/operator --json before naming JSON-only route fields:\n{}",
        violations.join("\n")
    );
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
    let hidden_flags = public_flow_hidden_command_or_flag_literals()
        .into_iter()
        .filter(|token| token.starts_with("--"))
        .collect::<Vec<_>>();
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
            is_protected_public_flow_file(&rel) && !is_public_flow_scanner_contract_file(&rel),
            "public runtime flow gate binary `{binary}` should be protected public-flow proof, not scanner self-test coverage"
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
    let selected_binaries = public_runtime_flow_test_binaries_from_script(&public_script);
    assert_eq!(
        selected_binaries,
        public_runtime_flow_required_test_binaries(),
        "public runtime flow gate membership should be owned by the typed public-flow manifest"
    );
    assert!(
        public_script.contains("--no-fail-fast"),
        "public runtime flow gate should keep no-fail-fast coverage"
    );
    for required_binary in public_runtime_flow_required_test_binaries() {
        let required = format!("--test {required_binary}");
        assert!(
            public_script.contains(&required),
            "public runtime flow gate must include `{required}`"
        );
    }
    for forbidden in [
        "internal_only_compatibility",
        "tests/internal_",
        "--test internal_",
        "--test public_flow_scan_contracts",
        "--test liveness_model_checker",
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
}

#[test]
fn runtime_behavior_golden_late_stage_marks_public_and_synthetic_setup() {
    let golden = public_runtime_route_golden();
    let late_stage_expectations = [
        (
            "release_readiness_pending",
            phase::PHASE_DOCUMENT_RELEASE_PENDING,
            phase::DETAIL_RELEASE_READINESS_RECORDING_READY,
        ),
        (
            "final_review_pending",
            phase::PHASE_FINAL_REVIEW_PENDING,
            phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
        ),
        (
            "ready_for_branch_completion",
            phase::PHASE_READY_FOR_BRANCH_COMPLETION,
            phase::DETAIL_FINISH_COMPLETION_GATE_READY,
        ),
        (
            "qa_pending",
            phase::PHASE_QA_PENDING,
            phase::DETAIL_QA_RECORDING_REQUIRED,
        ),
    ];
    for (label, expected_phase, expected_detail) in late_stage_expectations {
        let payload = public_runtime_route_payload(&golden, label, "workflow_operator");
        assert_eq!(
            payload["phase"].as_str(),
            Some(expected_phase),
            "{label}: late-stage golden should expose public phase"
        );
        assert_eq!(
            payload["phase_detail"].as_str(),
            Some(expected_detail),
            "{label}: late-stage golden should expose public phase detail"
        );
        assert_public_route_payload_uses_typed_command_authority(label, &payload);
    }
}

#[test]
fn synthetic_authority_fixture_setup_uses_registered_scanner_exceptions() {
    let mut registered_event_authority_calls = Vec::new();
    for rel in [
        "tests/plan_execution.rs",
        "tests/public_replay_churn.rs",
        "tests/runtime_behavior_golden.rs",
        "tests/workflow_entry_shell_smoke.rs",
        "tests/workflow_runtime.rs",
        "tests/workflow_runtime_final_review.rs",
        "tests/workflow_shell_smoke.rs",
    ] {
        let source = read_repo_file(rel);
        let function_spans = rust_source_scan::function_spans(rel, &source);
        for call in rust_source_scan::normalized_call_path_hits(rel, &source, &[]) {
            let Some(api) = event_log_fixture_authority_call_name(&call) else {
                continue;
            };
            let scope = function_spans
                .iter()
                .find(|function| call.line >= function.start_line && call.line <= function.end_line)
                .unwrap_or_else(|| {
                    panic!(
                        "{rel}:{} event-authority API `{api}` should be inside a function",
                        call.line
                    )
                });
            let category =
                event_log_test_api_exception_category(rel, &scope.name).unwrap_or_else(|| {
                panic!(
                    "{rel}:{} event-authority API `{api}` in `{}` must be registered with a synthetic fixture exception",
                    call.line, scope.name
                )
            });
            assert!(
                scope.name.contains("synthetic")
                    && category == PublicFlowExceptionCategory::SyntheticFixtureSetup,
                "{rel}:{} event-authority API `{api}` should stay visibly synthetic in code and scanner category",
                call.line
            );
            registered_event_authority_calls.push(format!("{rel}:{}:{api}", call.line));
        }
    }
    assert!(
        registered_event_authority_calls.len() >= 5,
        "public-flow fixture setup should exercise registered synthetic authority exceptions, got {registered_event_authority_calls:?}"
    );
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
fn production_diagnostics_do_not_route_to_hidden_gates_or_receipt_repair() {
    let mut violations = Vec::new();
    let forbidden_patterns = public_diagnostic_forbidden_patterns();
    let hidden_command_token_patterns = public_diagnostic_hidden_command_token_patterns();
    for file in production_source_files() {
        let rel = repo_relative(&file);
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", file.display()));
        let mut patterns = forbidden_patterns.clone();
        patterns.extend(hidden_command_token_patterns.iter().cloned());
        let source = source_for_public_diagnostic_hidden_token_scan(&rel, &source);
        violations.extend(diagnostic_pattern_violations_for_source(
            &rel, &source, &patterns,
        ));
    }

    assert!(
        violations.is_empty(),
        "production public diagnostics must not revive hidden-gate or receipt-repair wording:\n{}",
        violations.join("\n")
    );
}

#[test]
fn production_diagnostic_hidden_token_scanner_catches_concat_literals() {
    let source = r#"
pub fn hidden_runtime_diagnostic() -> &'static str {
    concat!("reconcile", "-review-state", " requires authoritative harness state.")
}
"#;
    let violations = diagnostic_pattern_violations_for_source(
        "src/example.rs",
        source,
        &public_diagnostic_hidden_command_token_patterns(),
    );

    assert!(
        !violations.is_empty(),
        "production diagnostic scanner must evaluate concat! string values, not only raw source substrings"
    );
}

#[test]
fn production_diagnostic_hidden_token_scanner_scans_command_eligibility_diagnostics() {
    let hidden_record_dispatch = hidden_literal(&["record", "-review-dispatch"]);
    let source = r#"
pub const HIDDEN_COMMAND_OR_FLAG_TOKENS: &[&str] = &[
    "__HIDDEN_RECORD_DISPATCH__",
];

pub fn hidden_command_or_flag_tokens() -> &'static [&'static str] {
    HIDDEN_COMMAND_OR_FLAG_TOKENS
}

pub fn command_eligibility_diagnostic() -> String {
    format!("{{}}", concat!("record", "-review-dispatch", " requires authoritative harness state."))
}
"#
    .replace("__HIDDEN_RECORD_DISPATCH__", &hidden_record_dispatch);
    let source = source_for_public_diagnostic_hidden_token_scan(
        "src/execution/command_eligibility.rs",
        &source,
    );
    let violations = diagnostic_pattern_violations_for_source(
        "src/execution/command_eligibility.rs",
        &source,
        &public_diagnostic_hidden_command_token_patterns(),
    );

    assert!(
        !violations.is_empty(),
        "production diagnostic scanner must scan command_eligibility diagnostics after redacting only the canonical hidden-token registry"
    );
}

#[test]
fn production_diagnostic_hidden_token_scanner_allows_command_eligibility_registry_only() {
    let hidden_record_dispatch = hidden_literal(&["record", "-review-dispatch"]);
    let hidden_reconcile = hidden_literal(&["reconcile", "-review-state"]);
    let source = r#"
pub const HIDDEN_COMMAND_OR_FLAG_TOKENS: &[&str] = &[
    "__HIDDEN_RECORD_DISPATCH__",
    "__HIDDEN_RECONCILE__",
];

pub fn hidden_command_or_flag_tokens() -> &'static [&'static str] {
    HIDDEN_COMMAND_OR_FLAG_TOKENS
}

pub fn command_eligibility_diagnostic() -> &'static str {
    "repair-review-state requires authoritative harness state."
}
"#
    .replace("__HIDDEN_RECORD_DISPATCH__", &hidden_record_dispatch)
    .replace("__HIDDEN_RECONCILE__", &hidden_reconcile);
    let source = source_for_public_diagnostic_hidden_token_scan(
        "src/execution/command_eligibility.rs",
        &source,
    );
    let violations = diagnostic_pattern_violations_for_source(
        "src/execution/command_eligibility.rs",
        &source,
        &public_diagnostic_hidden_command_token_patterns(),
    );

    assert!(
        violations.is_empty(),
        "hidden-token scanner should exempt only the canonical registry values, not the whole command_eligibility source file: {violations:?}"
    );
}

#[test]
fn production_diagnostic_hidden_token_scanner_catches_registry_tokens_beyond_reconcile() {
    let source = r#"
pub fn hidden_runtime_diagnostic() -> &'static str {
    concat!("record", "-review-dispatch", " requires authoritative harness state.")
}
"#;
    let violations = diagnostic_pattern_violations_for_source(
        "src/example.rs",
        source,
        &public_diagnostic_hidden_command_token_patterns(),
    );

    assert!(
        !violations.is_empty(),
        "production diagnostic scanner must consume the shared hidden command token registry, not a single hand-picked retired command"
    );
}

#[test]
fn production_diagnostic_hidden_token_scanner_catches_format_split_literals() {
    let source = r#"
pub fn hidden_runtime_diagnostic() -> String {
    format!("{}{} requires authoritative harness state.", "record", "-review-dispatch")
}
"#;
    let violations = diagnostic_pattern_violations_for_source(
        "src/example.rs",
        source,
        &public_diagnostic_hidden_command_token_patterns(),
    );

    assert!(
        !violations.is_empty(),
        "production diagnostic scanner must evaluate literal-only format! constructions that materialize hidden command tokens"
    );
}

#[test]
fn production_diagnostic_hidden_token_scanner_catches_array_concat_split_literals() {
    let source = r#"
pub fn hidden_runtime_diagnostic() -> String {
    ["record", "-review-dispatch", " requires authoritative harness state."].concat()
}
"#;
    let violations = diagnostic_pattern_violations_for_source(
        "src/example.rs",
        source,
        &public_diagnostic_hidden_command_token_patterns(),
    );

    assert!(
        !violations.is_empty(),
        "production diagnostic scanner must evaluate literal-only array concat constructions that materialize hidden command tokens"
    );
}

#[test]
fn gate_diagnostic_remediations_route_artifact_refresh_through_operator_json() {
    let gates_source = read_repo_file("src/execution/gates.rs");
    let authority_source = read_repo_file("src/execution/authority.rs");
    let route_guidance_source = read_repo_file("src/execution/public_route_guidance.rs");
    assert!(
        gates_source.contains("pub(crate) fn public_gate_remediation(")
            && gates_source.contains("public_gate_remediation_for_plan(")
            && gates_source.contains("PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT"),
        "gate diagnostics should share public-route remediation text with typed operator authority"
    );
    assert!(
        authority_source.contains("public_gate_remediation(")
            && authority_source.contains("public_gate_remediation_for_plan("),
        "authoritative record diagnostics should reuse public gate remediation helpers instead of plain artifact-repair instructions"
    );
    for required in [
        "recommended_public_command_argv",
        "required_inputs",
        "recommended_public_command_template.input_bindings",
    ] {
        assert!(
            route_guidance_source.contains(required),
            "public_route_guidance should own typed operator route contract field `{required}` for gate remediation reuse"
        );
    }

    let forbidden_manual_fragments = [
        "Regenerate the contract",
        "Regenerate the report",
        "Regenerate the evaluation report",
        "Regenerate the handoff",
        "Regenerate evidence_refs",
        "Regenerate evidence refs",
        "Regenerate criterion_results",
        "Regenerate affected_steps",
        "Regenerate repo evidence sources",
        "retry gate-contract",
        "retry gate-evaluator",
        "retry gate-handoff",
        "retry the gate command",
    ];
    let mut violations = Vec::new();
    for (rel, source) in [
        ("src/execution/gates.rs", gates_source.as_str()),
        ("src/execution/authority.rs", authority_source.as_str()),
    ] {
        let source_lower = source.to_ascii_lowercase();
        for fragment in forbidden_manual_fragments {
            let fragment_lower = fragment.to_ascii_lowercase();
            for (start, _) in source_lower.match_indices(&fragment_lower) {
                let window_start = start.saturating_sub(320);
                let window_end = (start + fragment_lower.len() + 320).min(source_lower.len());
                let window = &source_lower[window_start..window_end];
                if !window.contains("public_gate_remediation")
                    && !window.contains("public_typed_operator_route_contract")
                {
                    violations.push(format!(
                        "{rel}:{} manual remediation fragment `{fragment}` is not routed through public operator JSON",
                        line_number_for_byte(source, start)
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "gate diagnostic remediation must not tell agents to manually regenerate proof artifacts or rerun low-level gates without a public route cue:\n{}",
        violations.join("\n")
    );
}

#[test]
fn state_gate_public_remediations_route_through_operator_authority() {
    let direct_mutation_commands = [
        hidden_literal(&["featureforge plan execution repair", "-review-state"]),
        hidden_literal(&["featureforge plan execution advance", "-late-stage"]),
    ];
    let files = [
        "src/execution/state.rs",
        "src/execution/state/preflight.rs",
        "src/execution/state/rebuild_evidence.rs",
        "src/execution/state/review_gate.rs",
        "src/execution/state/runtime_methods.rs",
    ];
    let mut violations = Vec::new();

    for rel in files {
        let source = read_repo_file(rel);
        for pattern in &direct_mutation_commands {
            for (start, _) in source.match_indices(pattern) {
                violations.push(format!(
                    "{rel}:{} public gate remediation must route through workflow operator/status typed argv instead of hard-coding `{pattern}`",
                    line_number_for_byte(&source, start)
                ));
            }
        }
    }

    let state_source = read_repo_file("src/execution/state.rs");
    let route_guidance_source = read_repo_file("src/execution/public_route_guidance.rs");
    for required in [
        "follow `recommended_public_command_argv` when present",
        "use `required_inputs` as validation metadata",
        "recommended_public_command_template.input_bindings",
        "If neither executable surface is present, stop and report the route diagnostic",
    ] {
        assert!(
            route_guidance_source.contains(required),
            "public_route_guidance should own the full typed operator route contract `{required}`"
        );
        assert!(
            !state_source.contains(required),
            "state.rs must consume the shared typed operator route contract without duplicating `{required}`"
        );
    }
    let state_dependency_paths =
        rust_source_scan::normalized_dependency_paths("src/execution/state.rs", &state_source);
    let preflight_source = read_repo_file("src/execution/state/preflight.rs");
    let preflight_dependency_paths = rust_source_scan::normalized_dependency_paths(
        "src/execution/state/preflight.rs",
        &preflight_source,
    );
    for required_path in [
        "crate::execution::status_support::PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT",
        "crate::execution::status_support::public_typed_operator_route_contract",
    ] {
        assert!(
            state_dependency_paths
                .iter()
                .any(|path| path == required_path),
            "state.rs should depend on shared typed operator route contract owner `{required_path}` instead of defining local remediation vocabulary. Dependencies: {state_dependency_paths:?}"
        );
    }
    assert!(
        preflight_dependency_paths
            .iter()
            .any(|path| path == "crate::execution::state::PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT"),
        "preflight.rs should consume the shared typed operator route contract through the state facade instead of hand-rolling field-level route guidance. Dependencies: {preflight_dependency_paths:?}"
    );
    assert!(
        !preflight_source
            .contains("follow its typed `recommended_public_command_argv` or `recommended_public_command_template`"),
        "preflight public recovery text should reuse PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT instead of local field-level wording"
    );

    assert!(
        violations.is_empty(),
        "state gate public remediations must not bypass routed public command authority:\n{}",
        violations.join("\n")
    );

    let runtime_methods = read_repo_file("src/execution/state/runtime_methods.rs");
    assert!(
        runtime_methods.contains(
            "fn review_dispatch_out_of_phase_gate(context: &ExecutionContext, message: String)"
        ) && runtime_methods.contains(
            "public_typed_operator_route_remediation_for_plan(\n            \"Re-query the routed public step for review-dispatch authority.\",\n            &context.plan_rel,"
        ) && !runtime_methods.contains(
            "Run `featureforge workflow operator --plan <approved-plan-path> --json` and follow the returned typed public route."
        ),
        "review dispatch out-of-phase remediation must use the shared typed operator route helper with the concrete plan context"
    );
}

#[test]
fn repair_review_state_outputs_do_not_recommend_repair_review_state_again() {
    let source = read_repo_file("src/execution/commands/repair_review_state.rs");
    assert!(
        source.contains("repair_review_state_self_loop_blocked_output"),
        "repair-review-state command output should have an explicit fail-closed self-loop guard"
    );
    let self_loop_section = source
        .split("fn repair_review_state_self_loop_blocked_output")
        .nth(1)
        .and_then(|rest| rest.split("fn clear_resolved_task_cycle_break").next())
        .expect("self-loop guard section should be present");
    assert!(
        self_loop_section.contains("recommended_public_command_argv: None")
            && self_loop_section.contains("recommended_public_command_template: None")
            && self_loop_section.contains("recommended_command: None"),
        "repair-review-state self-loop guard must clear all executable/display command surfaces"
    );
    assert!(
        source.contains("required_follow_up_kind == Some(FollowUpKind::RepairReviewState)")
            && source.contains("route_action.kind == RepairRouteActionKind::RepairReviewState"),
        "repair-review-state command must guard both required-follow-up and route-action self-loop paths"
    );
}

#[test]
fn finish_gate_public_remediations_do_not_teach_skill_chains_or_low_level_recording() {
    let stale_patterns = [
        "Record a fresh branch closure",
        "Record a workflow pivot",
        "Run document-release and return",
        "Run document-release, then rerun",
        "Run document-release for",
        "Run document-release before",
        "satisfy the release-readiness `required_inputs`",
        "returned `advance-late-stage` route",
        "Resolve the release blocker and rerun document-release",
        "Run requesting-code-review and return",
        "Run requesting-code-review, then rerun",
        "rerun requesting-code-review",
        "downstream finish artifacts",
        "Run qa-only and return",
        "Run qa-only using",
        "Address the QA findings and rerun qa-only",
        "Address the final-review findings and rerun",
    ];
    let files = [
        "src/execution/status_support.rs",
        "src/execution/state/review_gate.rs",
        "src/execution/state/finish_gate.rs",
        "src/execution/state/artifact_finish_truth.rs",
        "src/execution/state/runtime_methods.rs",
        "src/workflow/doctor_dashboard.rs",
    ];
    let mut violations = Vec::new();

    for rel in files {
        let source = read_repo_file(rel);
        for pattern in stale_patterns {
            for (start, _) in source.match_indices(pattern) {
                violations.push(format!(
                    "{rel}:{} finish/gate remediation must point to workflow operator typed public routes instead of `{pattern}`",
                    line_number_for_byte(&source, start)
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "finish/gate public remediations must not teach low-level recording or multi-skill chains:\n{}",
        violations.join("\n")
    );

    let doctor_dashboard = read_repo_file("src/workflow/doctor_dashboard.rs");
    assert!(
        doctor_dashboard
            .contains("use crate::execution::status_support::public_typed_operator_route_contract"),
        "doctor dashboard should compose public route text from the shared typed operator route contract"
    );
    let route_guidance = read_repo_file("src/execution/public_route_guidance.rs");
    let status_support = read_repo_file("src/execution/status_support.rs");
    for required in [
        "recommended_public_command_argv",
        "required_inputs",
        "recommended_public_command_template.input_bindings",
    ] {
        assert!(
            route_guidance.contains(required),
            "public_route_guidance should own typed route contract field `{required}`"
        );
    }
    for duplicated in [
        "Follow `recommended_public_command_argv` when present, otherwise bind concrete values",
        "follow recommended_public_command_argv or recommended_public_command_template.input_bindings",
    ] {
        assert!(
            !doctor_dashboard.contains(duplicated),
            "doctor dashboard should derive typed route law from the shared macro instead of duplicating `{duplicated}`"
        );
    }

    let workflow_operator = read_repo_file("src/workflow/operator.rs");
    assert!(
        workflow_operator.contains("PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT"),
        "workflow operator public text should reuse the shared typed route contract"
    );
    assert!(
        !workflow_operator
            .contains("follow recommended_public_command_argv or bind recommended_public_command_template.input_bindings"),
        "workflow operator test-plan refresh guidance should not duplicate partial typed route law"
    );

    let runtime_methods = read_repo_file("src/execution/state/runtime_methods.rs");
    assert!(
        runtime_methods.contains("public_typed_operator_route_remediation_for_plan"),
        "runtime dispatch remediation should use the shared full typed route remediation helper"
    );
    assert!(
        status_support.contains("fn task_boundary_public_route_remediation("),
        "task-boundary public remediation should use the shared status-support typed route helper"
    );
    for required in [
        "Primary next step: query workflow operator JSON for",
        "WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND",
        "WORKFLOW_OPERATOR_EXTERNAL_READY_JSON_DISPLAY_COMMAND",
        "Diagnostic hint: use `--external-review-result-ready` only after an external review result exists",
        "verification results alone do not justify that flag",
        "otherwise do not pass `--external-review-result-ready`",
    ] {
        assert!(
            status_support.contains(required),
            "task-boundary public remediation should include typed and conditional route guidance `{required}`"
        );
    }
    for required in [
        "featureforge workflow operator --plan <approved-plan-path> --json",
        "featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready --json",
    ] {
        assert!(
            route_guidance.contains(required),
            "public_route_guidance should own safe workflow/operator display form `{required}`"
        );
    }
    assert!(
        !status_support.contains("external review or verification result"),
        "task-boundary remediation must reserve --external-review-result-ready for actual external review results"
    );
}

#[test]
fn public_diagnostics_use_route_authority_instead_of_retired_repair_wording() {
    let checked_files = [
        "src/execution/state/runtime_methods.rs",
        "src/execution/state/review_gate.rs",
        "src/workflow/status.rs",
    ];
    let forbidden = [
        "task review dispatch",
        "dispatching task review",
        "before dispatching task review",
        "Repair the spec",
        "Repair the plan",
        "repair the spec",
        "repair the plan",
        "retry final-review dispatch",
        "review-state repair",
    ];
    let mut violations = Vec::new();

    for rel in checked_files {
        let source = read_repo_file(rel);
        for pattern in forbidden {
            for (start, _) in source.match_indices(pattern) {
                violations.push(format!(
                    "{rel}:{} public diagnostics should route through workflow/operator JSON, next_skill, or typed argv/template instead of `{pattern}`",
                    line_number_for_byte(&source, start)
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "public diagnostics must avoid retired dispatch/manual repair wording:\n{}",
        violations.join("\n")
    );

    let runtime_methods = read_repo_file("src/execution/state/runtime_methods.rs");
    assert!(
        runtime_methods.contains("current task-boundary route")
            && runtime_methods.contains("public_typed_operator_route_remediation_for_plan"),
        "task-boundary gate diagnostics should keep domain detail while using the shared typed route helper"
    );
    let review_gate = read_repo_file("src/execution/state/review_gate.rs");
    assert!(
        review_gate.contains("workflow/operator JSON for the current approved-plan route")
            && review_gate.contains("public_typed_operator_route_remediation_for_plan"),
        "final-review gate diagnostics should point back to operator JSON plus typed route authority"
    );
    let workflow_status = read_repo_file("src/workflow/status.rs");
    assert!(
        workflow_status.contains("featureforge:plan-ceo-review")
            && workflow_status.contains("featureforge:writing-plans")
            && workflow_status.contains("current plan-draft route"),
        "workflow status plan/spec diagnostics should name the public review/authoring route instead of free-form repair"
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
fn public_aggregate_event_owner_paths_are_static_guarded_in_public_flow_suite() {
    let advance_rel = "src/execution/commands/advance_late_stage.rs";
    let advance_source = read_repo_file(advance_rel);
    assert_public_owner_paths_do_not_mix_hidden_owners(
        advance_rel,
        &advance_source,
        "EventCommandOwner::PublicAdvanceLateStage",
    );

    let close_rel = "src/execution/commands/close_current_task.rs";
    let close_source = read_repo_file(close_rel);
    assert_public_owner_paths_do_not_mix_hidden_owners(
        close_rel,
        &close_source,
        "EventCommandOwner::PublicCloseCurrentTask",
    );

    let branch_truth_rel = "src/execution/commands/common/branch_closure_truth.rs";
    let branch_truth_source = read_repo_file(branch_truth_rel);
    assert!(
        branch_truth_source.contains("EventCommandOwner")
            && branch_truth_source.contains("command_owner.as_str()"),
        "branch-closure truth repair paths should persist through the caller-provided event owner"
    );
    for forbidden in hidden_aggregate_event_owner_tokens() {
        assert!(
            !branch_truth_source.contains(forbidden),
            "branch-closure truth repair paths must not hard-code hidden primitive event owner `{forbidden}`"
        );
    }
}

fn assert_public_owner_paths_do_not_mix_hidden_owners(rel: &str, source: &str, public_owner: &str) {
    let public_owner_functions = rust_function_bodies(rel, source)
        .into_iter()
        .filter_map(|function| {
            let body = function.lines.join("\n");
            body.contains(public_owner).then_some((function.name, body))
        })
        .collect::<Vec<_>>();
    assert!(
        !public_owner_functions.is_empty(),
        "{rel} should pass {public_owner} through at least one public aggregate event-owner path"
    );
    for (function_name, body) in public_owner_functions {
        for forbidden in hidden_aggregate_event_owner_tokens() {
            assert!(
                !body.contains(forbidden),
                "{rel}::{function_name} must not mix public aggregate event owner `{public_owner}` with hidden primitive owner `{forbidden}`"
            );
        }
    }
}

fn hidden_aggregate_event_owner_tokens() -> [&'static str; 10] {
    [
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
    ]
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
fn public_replay_command_budget_gates_are_explicit() {
    let replay = read_repo_file("tests/public_replay_churn.rs");
    assert!(
        replay.matches("assert_public_command_budget(").count() >= 6,
        "public_replay_churn.rs should keep budget assertions on historical recovery scenarios"
    );
    for needle in [
        r#"cli.delta_since(&checkpoint, "begin")"#,
        "bridge should need one public begin after route discovery",
        r#"cli.delta_since(&checkpoint, "close-current-task")"#,
        r#"cli.delta_since(&checkpoint, "reopen")"#,
        "cycle-break recovery must not loop through reopen",
        "approved plan fidelity gate",
        "receipt",
    ] {
        assert!(
            replay.contains(needle),
            "public_replay_churn.rs must keep the public replay budget/fidelity gate assertion `{needle}`"
        );
    }

    let shell = read_repo_file("tests/workflow_shell_smoke.rs");
    assert_text_near(
        &shell,
        "FS11-REBASE-RESUME-BUDGET",
        "runtime_management_commands, 2",
        "workflow_shell_smoke FS11 budget",
    );
    assert_text_near(
        &shell,
        "TASK-CLOSE-BUDGET",
        "runtime_management_commands, 2",
        "workflow_shell_smoke task-close budget",
    );
    for needle in ["TASK-CLOSE-BUDGET", "FS11-REBASE-RESUME-BUDGET"] {
        assert!(
            shell.contains(needle),
            "workflow_shell_smoke.rs must keep the runtime-management budget assertion `{needle}`"
        );
    }
}
