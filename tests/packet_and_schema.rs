use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use featureforge::contracts::evidence::read_execution_evidence;
use featureforge::contracts::packet::{build_task_packet_with_timestamp, write_contract_schemas};
use featureforge::contracts::plan::parse_plan_file;
use featureforge::contracts::spec::parse_spec_file;
use featureforge::execution::state::write_plan_execution_schema;
use featureforge::repo_safety::write_repo_safety_schema;
use featureforge::update_check::write_update_check_schema;
use featureforge::workflow::status::write_workflow_schemas;
use serde_json::Value;

const SPEC_REL: &str = "docs/featureforge/specs/2026-03-22-plan-contract-fixture-design.md";
const PLAN_REL: &str = "docs/featureforge/plans/2026-03-22-plan-contract-fixture.md";

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("featureforge-{label}-{nanos}"));
    fs::create_dir_all(&dir).expect("temp dir should be created");
    dir
}

fn repo_fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn install_fixture(repo_root: &Path, fixture_name: &str, destination_rel: &str) {
    let destination = repo_root.join(destination_rel);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).expect("fixture parent directories should exist");
    }
    fs::copy(
        repo_fixture_path(&format!(
            "tests/codex-runtime/fixtures/plan-contract/{fixture_name}"
        )),
        destination,
    )
    .expect("fixture should copy");
}

fn install_valid_artifacts(repo_root: &Path) {
    install_fixture(repo_root, "valid-spec.md", SPEC_REL);
    install_fixture(repo_root, "valid-plan.md", PLAN_REL);
}

fn schema_properties(schema: &Value) -> &serde_json::Map<String, Value> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("schema should expose object properties")
}

fn schema_required_fields(schema: &Value, issues: &mut Vec<String>) -> BTreeSet<String> {
    match schema.get("required") {
        Some(Value::Array(required_fields)) => {
            let mut fields = BTreeSet::new();
            for field in required_fields {
                match field.as_str() {
                    Some(name) => {
                        fields.insert(name.to_owned());
                    }
                    None => {
                        issues.push(String::from(
                            "schema `required` array should contain only string field names",
                        ));
                    }
                }
            }
            fields
        }
        Some(_) => {
            issues.push(String::from(
                "schema should expose top-level `required` as an array",
            ));
            BTreeSet::new()
        }
        None => {
            issues.push(String::from("schema is missing top-level `required`"));
            BTreeSet::new()
        }
    }
}

fn resolve_local_schema_ref<'a>(schema: &'a Value, value: &'a Value) -> Option<&'a Value> {
    let mut current = value;
    let mut visited_refs = BTreeSet::new();
    loop {
        let Some(reference) = current.get("$ref").and_then(Value::as_str) else {
            return Some(current);
        };
        if !reference.starts_with('#') || !visited_refs.insert(reference.to_owned()) {
            return None;
        }
        let pointer = reference.trim_start_matches('#');
        current = if pointer.is_empty() {
            schema
        } else {
            schema.pointer(pointer)?
        };
    }
}

fn schema_type_set(schema: &Value, value: &Value) -> Option<BTreeSet<String>> {
    let resolved_variants = schema_resolved_variants(schema, value)?;
    let mut types = BTreeSet::new();
    for resolved in resolved_variants {
        match resolved.get("type") {
            Some(Value::String(type_name)) => {
                types.insert(type_name.clone());
            }
            Some(Value::Array(type_names)) => {
                for type_name in type_names {
                    types.insert(type_name.as_str()?.to_owned());
                }
            }
            _ => {}
        }
    }
    if types.is_empty() { None } else { Some(types) }
}

fn schema_enum_set(schema: &Value, value: &Value) -> Option<BTreeSet<String>> {
    let resolved_variants = schema_resolved_variants(schema, value)?;
    let mut enum_values = BTreeSet::new();
    for resolved in resolved_variants {
        let Some(values) = resolved.get("enum").and_then(Value::as_array) else {
            continue;
        };
        for value in values {
            enum_values.insert(value.as_str()?.to_owned());
        }
    }
    if enum_values.is_empty() {
        None
    } else {
        Some(enum_values)
    }
}

fn schema_resolved_variants<'a>(schema: &'a Value, value: &'a Value) -> Option<Vec<&'a Value>> {
    fn collect_variants<'a>(schema: &'a Value, value: &'a Value, out: &mut Vec<&'a Value>) -> bool {
        let Some(resolved) = resolve_local_schema_ref(schema, value) else {
            return false;
        };
        let mut found = false;
        if resolved.get("type").is_some() || resolved.get("enum").is_some() {
            out.push(resolved);
            found = true;
        }
        for keyword in ["anyOf", "oneOf"] {
            let Some(variants) = resolved.get(keyword).and_then(Value::as_array) else {
                continue;
            };
            for variant in variants {
                found |= collect_variants(schema, variant, out);
            }
        }
        if !found {
            out.push(resolved);
            return true;
        }
        found
    }

    let mut variants = Vec::new();
    if collect_variants(schema, value, &mut variants) {
        Some(variants)
    } else {
        None
    }
}

fn schema_property<'a>(
    properties: &'a serde_json::Map<String, Value>,
    field: &str,
    issues: &mut Vec<String>,
    missing_fields: &mut BTreeSet<String>,
) -> Option<&'a Value> {
    match properties.get(field) {
        Some(value) => Some(value),
        None => {
            if missing_fields.insert(field.to_owned()) {
                issues.push(format!("missing expected property `{field}`"));
            }
            None
        }
    }
}

fn assert_schema_types(
    schema: &Value,
    properties: &serde_json::Map<String, Value>,
    field: &str,
    expected_types: &[&str],
    issues: &mut Vec<String>,
    missing_fields: &mut BTreeSet<String>,
) {
    let Some(property) = schema_property(properties, field, issues, missing_fields) else {
        return;
    };
    let Some(actual_types) = schema_type_set(schema, property) else {
        issues.push(format!(
            "property `{field}` is missing a usable `type` definition"
        ));
        return;
    };
    let expected_types: BTreeSet<String> =
        expected_types.iter().map(|ty| (*ty).to_owned()).collect();
    if actual_types != expected_types {
        issues.push(format!(
            "property `{field}` has schema types {actual_types:?}, expected {expected_types:?}"
        ));
    }
}

fn assert_schema_required_field(
    required_fields: &BTreeSet<String>,
    field: &str,
    issues: &mut Vec<String>,
    missing_required_fields: &mut BTreeSet<String>,
) {
    if !required_fields.contains(field) && missing_required_fields.insert(field.to_owned()) {
        issues.push(format!(
            "field `{field}` should be present in the schema `required` array"
        ));
    }
}

fn assert_schema_optional_field(
    required_fields: &BTreeSet<String>,
    field: &str,
    issues: &mut Vec<String>,
    non_optional_fields: &mut BTreeSet<String>,
) {
    if required_fields.contains(field) && non_optional_fields.insert(field.to_owned()) {
        issues.push(format!(
            "field `{field}` should be optional and must not be present in the schema `required` array"
        ));
    }
}

fn assert_schema_enum(
    schema: &Value,
    properties: &serde_json::Map<String, Value>,
    field: &str,
    expected_values: &[&str],
    issues: &mut Vec<String>,
    missing_fields: &mut BTreeSet<String>,
) {
    let Some(property) = schema_property(properties, field, issues, missing_fields) else {
        return;
    };
    let Some(actual_values) = schema_enum_set(schema, property) else {
        issues.push(format!(
            "property `{field}` is missing a usable `enum` definition"
        ));
        return;
    };
    let expected_values: BTreeSet<String> = expected_values
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    if actual_values != expected_values {
        issues.push(format!(
            "property `{field}` has schema enum {actual_values:?}, expected {expected_values:?}"
        ));
    }
}

fn assert_schema_array_items_types(
    schema: &Value,
    properties: &serde_json::Map<String, Value>,
    field: &str,
    expected_types: &[&str],
    issues: &mut Vec<String>,
    missing_fields: &mut BTreeSet<String>,
) {
    let Some(property) = schema_property(properties, field, issues, missing_fields) else {
        return;
    };
    let Some(items) = property.get("items") else {
        issues.push(format!("property `{field}` is missing `items`"));
        return;
    };
    let Some(actual_types) = schema_type_set(schema, items) else {
        issues.push(format!(
            "property `{field}` items are missing a usable `type` definition"
        ));
        return;
    };
    let expected_types: BTreeSet<String> =
        expected_types.iter().map(|ty| (*ty).to_owned()).collect();
    if actual_types != expected_types {
        issues.push(format!(
            "property `{field}` items have schema types {actual_types:?}, expected {expected_types:?}"
        ));
    }
}

fn assert_schema_array_items_enum(
    schema: &Value,
    properties: &serde_json::Map<String, Value>,
    field: &str,
    expected_values: &[&str],
    issues: &mut Vec<String>,
    missing_fields: &mut BTreeSet<String>,
) {
    let Some(property) = schema_property(properties, field, issues, missing_fields) else {
        return;
    };
    let Some(items) = property.get("items") else {
        issues.push(format!("property `{field}` is missing `items`"));
        return;
    };
    let Some(actual_values) = schema_enum_set(schema, items) else {
        issues.push(format!(
            "property `{field}` items are missing a usable `enum` definition"
        ));
        return;
    };
    let expected_values: BTreeSet<String> = expected_values
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    if actual_values != expected_values {
        issues.push(format!(
            "property `{field}` items have schema enum {actual_values:?}, expected {expected_values:?}"
        ));
    }
}

fn assert_schema_pointer_enum(
    schema: &Value,
    pointer: &str,
    expected_values: &[&str],
    issues: &mut Vec<String>,
) {
    let Some(value) = schema.pointer(pointer) else {
        issues.push(format!("schema is missing pointer `{pointer}`"));
        return;
    };
    let Some(actual_enum) = schema_enum_set(schema, value) else {
        issues.push(format!(
            "schema pointer `{pointer}` is missing a usable `enum` definition"
        ));
        return;
    };
    let expected_enum: BTreeSet<String> = expected_values
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    if actual_enum != expected_enum {
        issues.push(format!(
            "schema pointer `{pointer}` has enum {actual_enum:?}, expected {expected_enum:?}"
        ));
    }
}

fn assert_schema_pointer_types(
    schema: &Value,
    pointer: &str,
    expected_types: &[&str],
    issues: &mut Vec<String>,
) {
    let Some(value) = schema.pointer(pointer) else {
        issues.push(format!("schema is missing pointer `{pointer}`"));
        return;
    };
    let Some(actual_types) = schema_type_set(schema, value) else {
        issues.push(format!(
            "schema pointer `{pointer}` is missing a usable `type` definition"
        ));
        return;
    };
    let expected_types: BTreeSet<String> = expected_types
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    if actual_types != expected_types {
        issues.push(format!(
            "schema pointer `{pointer}` has types {actual_types:?}, expected {expected_types:?}"
        ));
    }
}

fn assert_schema_pointer_required(
    schema: &Value,
    pointer: &str,
    expected_required: &[&str],
    issues: &mut Vec<String>,
) {
    let Some(value) = schema.pointer(pointer) else {
        issues.push(format!("schema is missing pointer `{pointer}`"));
        return;
    };
    let Some(required) = value.get("required").and_then(Value::as_array) else {
        issues.push(format!("schema pointer `{pointer}` is missing `required`"));
        return;
    };
    let actual_required: BTreeSet<String> = required
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let expected_required: BTreeSet<String> = expected_required
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    if actual_required != expected_required {
        issues.push(format!(
            "schema pointer `{pointer}` has required {actual_required:?}, expected {expected_required:?}"
        ));
    }
}

fn assert_phase_detail_recording_context_required(
    schema: &Value,
    phase_detail: &str,
    expected_required: &[&str],
    issues: &mut Vec<String>,
) {
    let Some(conditions) = schema.get("allOf").and_then(Value::as_array) else {
        issues.push(String::from(
            "schema is missing top-level `allOf` phase-bound conditions",
        ));
        return;
    };
    let Some(condition) = conditions.iter().find(|condition| {
        condition
            .pointer("/if/properties/phase_detail/const")
            .and_then(Value::as_str)
            == Some(phase_detail)
    }) else {
        issues.push(format!(
            "schema is missing phase-bound recording_context condition for `{phase_detail}`"
        ));
        return;
    };

    let actual_top_level_required: BTreeSet<String> = condition
        .pointer("/then/required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let expected_top_level_required = BTreeSet::from([String::from("recording_context")]);
    if actual_top_level_required != expected_top_level_required {
        issues.push(format!(
            "phase-bound recording_context condition `{phase_detail}` has required {actual_top_level_required:?}, expected {expected_top_level_required:?}"
        ));
    }

    let actual_required: BTreeSet<String> = condition
        .pointer("/then/properties/recording_context/required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let expected_required: BTreeSet<String> = expected_required
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    if actual_required != expected_required {
        issues.push(format!(
            "phase-bound recording_context condition `{phase_detail}` has required {actual_required:?}, expected {expected_required:?}"
        ));
    }
}

fn assert_phase_detail_field_forbidden_outside_allowed_phase_details(
    schema: &Value,
    field: &str,
    allowed_phase_details: &[&str],
    issues: &mut Vec<String>,
) {
    let Some(conditions) = schema.get("allOf").and_then(Value::as_array) else {
        issues.push(String::from(
            "schema is missing top-level `allOf` phase-bound conditions",
        ));
        return;
    };

    let expected_allowed: BTreeSet<String> = allowed_phase_details
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    let Some(condition) = conditions.iter().find(|condition| {
        let actual_allowed: BTreeSet<String> = condition
            .pointer("/if/properties/phase_detail/enum")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        actual_allowed == expected_allowed
    }) else {
        issues.push(format!(
            "schema is missing phase_detail omission contract for `{field}` outside {expected_allowed:?}"
        ));
        return;
    };

    let actual_else_required: BTreeSet<String> = condition
        .pointer("/else/not/required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let expected_else_required = BTreeSet::from([field.to_owned()]);
    if actual_else_required != expected_else_required {
        issues.push(format!(
            "phase_detail omission contract for `{field}` has else/not/required {actual_else_required:?}, expected {expected_else_required:?}"
        ));
    }
}

fn assert_phase_field_field_forbidden_outside_const_phase(
    schema: &Value,
    phase_field: &str,
    phase_value: &str,
    field: &str,
    issues: &mut Vec<String>,
) {
    let Some(conditions) = schema.get("allOf").and_then(Value::as_array) else {
        issues.push(String::from(
            "schema is missing top-level `allOf` phase-bound conditions",
        ));
        return;
    };

    let selector_pointer = format!("/if/properties/{phase_field}/const");
    let Some(condition) = conditions.iter().find(|condition| {
        condition.pointer(&selector_pointer).and_then(Value::as_str) == Some(phase_value)
    }) else {
        issues.push(format!(
            "schema is missing `{field}` omission contract outside {phase_field}={phase_value}"
        ));
        return;
    };

    let actual_else_required: BTreeSet<String> = condition
        .pointer("/else/not/required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let expected_else_required = BTreeSet::from([field.to_owned()]);
    if actual_else_required != expected_else_required {
        issues.push(format!(
            "phase-bound omission contract for `{field}` has else/not/required {actual_else_required:?}, expected {expected_else_required:?}"
        ));
    }
}

fn assert_phase_detail_field_omitted_only_in_lanes(
    schema: &Value,
    field: &str,
    omission_phase_details: &[&str],
    issues: &mut Vec<String>,
) {
    let Some(conditions) = schema.get("allOf").and_then(Value::as_array) else {
        issues.push(String::from(
            "schema is missing top-level `allOf` phase-bound conditions",
        ));
        return;
    };

    let expected_omission_lanes: BTreeSet<String> = omission_phase_details
        .iter()
        .map(|value| (*value).to_owned())
        .collect();
    let Some(condition) = conditions.iter().find(|condition| {
        let actual_omission_lanes: BTreeSet<String> = condition
            .pointer("/if/properties/phase_detail/enum")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        actual_omission_lanes == expected_omission_lanes
    }) else {
        issues.push(format!(
            "schema is missing phase_detail omission-lane contract for `{field}` in {expected_omission_lanes:?}"
        ));
        return;
    };

    let actual_then_required: BTreeSet<String> = condition
        .pointer("/then/not/required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let expected_then_required = BTreeSet::from([field.to_owned()]);
    if actual_then_required != expected_then_required {
        issues.push(format!(
            "omission-lane contract for `{field}` has then/not/required {actual_then_required:?}, expected {expected_then_required:?}"
        ));
    }

    let actual_else_required: BTreeSet<String> = condition
        .pointer("/else/required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let expected_else_required = BTreeSet::from([field.to_owned()]);
    if actual_else_required != expected_else_required {
        issues.push(format!(
            "omission-lane contract for `{field}` has else/required {actual_else_required:?}, expected {expected_else_required:?}"
        ));
    }
}

fn assert_schema_pointer_value(
    schema: &Value,
    pointer: &str,
    expected_value: Value,
    issues: &mut Vec<String>,
) {
    let Some(actual_value) = schema.pointer(pointer) else {
        issues.push(format!("schema is missing pointer `{pointer}`"));
        return;
    };
    if actual_value != &expected_value {
        issues.push(format!(
            "schema pointer `{pointer}` has value {actual_value:?}, expected {expected_value:?}"
        ));
    }
}

fn plan_execution_status_schema_issues(schema_json: &str) -> Vec<String> {
    let schema: Value = serde_json::from_str(schema_json).expect("schema should parse");
    let properties = schema_properties(&schema);
    let mut issues = Vec::new();
    let required_fields = schema_required_fields(&schema, &mut issues);

    let mut missing_fields = BTreeSet::new();
    let mut missing_required_fields = BTreeSet::new();
    let mut non_optional_fields = BTreeSet::new();

    macro_rules! check_types {
        ($field:literal, [$($expected:literal),+ $(,)?], required) => {
            assert_schema_types(
                &schema,
                &properties,
                $field,
                &[$($expected),+],
                &mut issues,
                &mut missing_fields,
            );
            assert_schema_required_field(
                &required_fields,
                $field,
                &mut issues,
                &mut missing_required_fields,
            );
        };
        ($field:literal, [$($expected:literal),+ $(,)?], optional) => {
            assert_schema_types(
                &schema,
                &properties,
                $field,
                &[$($expected),+],
                &mut issues,
                &mut missing_fields,
            );
            assert_schema_optional_field(
                &required_fields,
                $field,
                &mut issues,
                &mut non_optional_fields,
            );
        };
    }

    macro_rules! check_enum {
        ($field:literal, [$($expected:literal),+ $(,)?]) => {
            assert_schema_enum(
                &schema,
                &properties,
                $field,
                &[$($expected),+],
                &mut issues,
                &mut missing_fields,
            );
        };
    }

    macro_rules! check_array_items {
        ($field:literal, [$($expected:literal),+ $(,)?]) => {
            assert_schema_array_items_types(
                &schema,
                &properties,
                $field,
                &[$($expected),+],
                &mut issues,
                &mut missing_fields,
            );
        };
    }

    macro_rules! check_array_items_enum {
        ($field:literal, [$($expected:literal),+ $(,)?]) => {
            assert_schema_array_items_enum(
                &schema,
                &properties,
                $field,
                &[$($expected),+],
                &mut issues,
                &mut missing_fields,
            );
        };
    }

    check_types!("execution_run_id", ["string", "null"], optional);
    check_types!("latest_authoritative_sequence", ["integer"], required);
    check_types!("harness_phase", ["string"], required);
    check_enum!(
        "harness_phase",
        [
            "implementation_handoff",
            "execution_preflight",
            "contract_drafting",
            "contract_pending_approval",
            "contract_approved",
            "executing",
            "evaluating",
            "repairing",
            "pivot_required",
            "handoff_required",
            "final_review_pending",
            "qa_pending",
            "document_release_pending",
            "ready_for_branch_completion",
        ]
    );
    check_types!("chunk_id", ["string"], required);
    check_types!("chunking_strategy", ["string", "null"], optional);
    check_enum!("chunking_strategy", ["task", "task-group", "whole-run"]);
    check_types!("workspace_state_id", ["string"], required);
    check_types!(
        "current_branch_reviewed_state_id",
        ["string", "null"],
        optional
    );
    check_types!("current_branch_closure_id", ["string", "null"], optional);
    check_types!("current_task_closures", ["array"], required);
    check_array_items!("current_task_closures", ["object"]);
    check_types!("superseded_closures_summary", ["array"], required);
    check_array_items!("superseded_closures_summary", ["string"]);
    check_types!("stale_unreviewed_closures", ["array"], required);
    check_array_items!("stale_unreviewed_closures", ["string"]);
    check_types!(
        "current_release_readiness_state",
        ["string", "null"],
        optional
    );
    check_types!("current_final_review_state", ["string"], required);
    check_types!("current_qa_state", ["string"], required);
    check_types!(
        "current_final_review_branch_closure_id",
        ["string", "null"],
        optional
    );
    check_types!("current_final_review_result", ["string", "null"], optional);
    check_types!("current_qa_branch_closure_id", ["string", "null"], optional);
    check_types!("current_qa_result", ["string", "null"], optional);
    check_types!("qa_requirement", ["string", "null"], optional);
    check_enum!("qa_requirement", ["required", "not-required"]);
    check_types!("evaluator_policy", ["string", "null"], optional);
    check_types!("reset_policy", ["string", "null"], optional);
    check_enum!("reset_policy", ["none", "chunk-boundary", "adaptive"]);
    check_types!("review_stack", ["array", "null"], optional);
    check_array_items!("review_stack", ["string"]);
    check_types!("active_contract_path", ["string", "null"], optional);
    check_types!("active_contract_fingerprint", ["string", "null"], optional);
    check_types!("required_evaluator_kinds", ["array"], required);
    check_array_items!("required_evaluator_kinds", ["string"]);
    check_array_items_enum!(
        "required_evaluator_kinds",
        ["spec_compliance", "code_quality"]
    );
    check_types!("completed_evaluator_kinds", ["array"], required);
    check_array_items!("completed_evaluator_kinds", ["string"]);
    check_array_items_enum!(
        "completed_evaluator_kinds",
        ["spec_compliance", "code_quality"]
    );
    check_types!("pending_evaluator_kinds", ["array"], required);
    check_array_items!("pending_evaluator_kinds", ["string"]);
    check_array_items_enum!(
        "pending_evaluator_kinds",
        ["spec_compliance", "code_quality"]
    );
    check_types!("non_passing_evaluator_kinds", ["array"], required);
    check_array_items!("non_passing_evaluator_kinds", ["string"]);
    check_array_items_enum!(
        "non_passing_evaluator_kinds",
        ["spec_compliance", "code_quality"]
    );
    check_types!("aggregate_evaluation_state", ["string"], required);
    check_enum!(
        "aggregate_evaluation_state",
        ["pass", "pending", "fail", "blocked"]
    );
    check_types!("last_evaluation_report_path", ["string", "null"], optional);
    check_types!(
        "last_evaluation_report_fingerprint",
        ["string", "null"],
        optional
    );
    check_types!(
        "last_evaluation_evaluator_kind",
        ["string", "null"],
        optional
    );
    check_enum!(
        "last_evaluation_evaluator_kind",
        ["spec_compliance", "code_quality"]
    );
    check_types!("last_evaluation_verdict", ["string", "null"], optional);
    check_enum!("last_evaluation_verdict", ["pass", "fail", "blocked"]);
    check_types!("current_chunk_retry_count", ["integer"], required);
    check_types!("current_chunk_retry_budget", ["integer"], required);
    check_types!("current_chunk_pivot_threshold", ["integer"], required);
    check_types!("handoff_required", ["boolean"], required);
    check_types!("open_failed_criteria", ["array"], required);
    check_array_items!("open_failed_criteria", ["string"]);
    check_types!("write_authority_state", ["string"], required);
    check_types!("write_authority_holder", ["string", "null"], optional);
    check_types!("write_authority_worktree", ["string", "null"], optional);
    check_types!("repo_state_baseline_head_sha", ["string", "null"], optional);
    check_types!(
        "repo_state_baseline_worktree_fingerprint",
        ["string", "null"],
        optional
    );
    check_types!("repo_state_drift_state", ["string"], required);
    check_types!("dependency_index_state", ["string"], required);
    check_types!("final_review_state", ["string"], required);
    check_enum!(
        "final_review_state",
        ["not_required", "missing", "fresh", "stale"]
    );
    check_types!("browser_qa_state", ["string"], required);
    check_enum!(
        "browser_qa_state",
        ["not_required", "missing", "fresh", "stale"]
    );
    check_types!("release_docs_state", ["string"], required);
    check_enum!(
        "release_docs_state",
        ["not_required", "missing", "fresh", "stale"]
    );
    check_types!(
        "last_final_review_artifact_fingerprint",
        ["string", "null"],
        optional
    );
    check_types!(
        "last_browser_qa_artifact_fingerprint",
        ["string", "null"],
        optional
    );
    check_types!(
        "last_release_docs_artifact_fingerprint",
        ["string", "null"],
        optional
    );
    check_types!("strategy_state", ["string"], required);
    check_types!(
        "last_strategy_checkpoint_fingerprint",
        ["string", "null"],
        optional
    );
    check_types!("strategy_checkpoint_kind", ["string"], required);
    check_types!("strategy_reset_required", ["boolean"], required);
    check_types!("phase_detail", ["string"], required);
    check_enum!(
        "phase_detail",
        [
            "branch_closure_recording_required_for_release_readiness",
            "execution_in_progress",
            "execution_reentry_required",
            "final_review_dispatch_required",
            "final_review_outcome_pending",
            "final_review_recording_ready",
            "finish_completion_gate_ready",
            "finish_review_gate_ready",
            "handoff_recording_required",
            "planning_reentry_required",
            "qa_recording_required",
            "release_blocker_resolution_required",
            "release_readiness_recording_ready",
            "task_closure_recording_ready",
            "task_review_dispatch_required",
            "task_review_result_pending",
            "test_plan_refresh_required"
        ]
    );
    check_types!("review_state_status", ["string"], required);
    check_enum!(
        "review_state_status",
        ["clean", "stale_unreviewed", "missing_current_closure"]
    );
    check_types!("blocking_records", ["array"], required);
    check_array_items!("blocking_records", ["object"]);
    check_types!("next_action", ["string"], required);
    check_enum!(
        "next_action",
        [
            "advance late stage",
            "finish branch",
            "close current task",
            "continue execution",
            "request task review",
            "request final review",
            "execution reentry required",
            "hand off",
            "pivot / return to planning",
            "refresh test plan",
            "repair review state / reenter execution",
            "resolve release blocker",
            "run QA",
            "wait for external review result"
        ]
    );
    check_types!("recommended_command", ["string"], optional);
    check_enum!(
        "follow_up_override",
        ["none", "record_handoff", "record_pivot"]
    );
    check_types!("recording_context", ["object"], optional);
    check_types!("execution_command_context", ["object"], optional);
    assert_schema_pointer_enum(
        &schema,
        "/$defs/PublicExecutionCommandContext/properties/command_kind",
        &["begin", "complete", "reopen"],
        &mut issues,
    );
    assert_schema_pointer_required(
        &schema,
        "/$defs/PublicExecutionCommandContext",
        &["command_kind", "task_number", "step_id"],
        &mut issues,
    );
    assert_schema_pointer_types(
        &schema,
        "/$defs/PublicExecutionCommandContext/properties/task_number",
        &["integer"],
        &mut issues,
    );
    assert_schema_pointer_types(
        &schema,
        "/$defs/PublicExecutionCommandContext/properties/step_id",
        &["integer"],
        &mut issues,
    );
    assert_schema_pointer_value(
        &schema,
        "/$defs/PublicExecutionCommandContext/additionalProperties",
        Value::Bool(false),
        &mut issues,
    );
    assert_schema_pointer_types(
        &schema,
        "/$defs/PublicRecordingContext/properties/branch_closure_id",
        &["string"],
        &mut issues,
    );
    assert_schema_pointer_types(
        &schema,
        "/$defs/PublicRecordingContext/properties/dispatch_id",
        &["string"],
        &mut issues,
    );
    assert_schema_pointer_types(
        &schema,
        "/$defs/PublicRecordingContext/properties/task_number",
        &["integer"],
        &mut issues,
    );
    assert_schema_pointer_value(
        &schema,
        "/$defs/PublicRecordingContext/minProperties",
        Value::from(1),
        &mut issues,
    );
    assert_schema_pointer_value(
        &schema,
        "/$defs/PublicRecordingContext/additionalProperties",
        Value::Bool(false),
        &mut issues,
    );
    assert_schema_pointer_required(
        &schema,
        "/$defs/PublicRecordingContext/anyOf/0",
        &["branch_closure_id"],
        &mut issues,
    );
    assert_schema_pointer_required(
        &schema,
        "/$defs/PublicRecordingContext/anyOf/1",
        &["task_number", "dispatch_id"],
        &mut issues,
    );
    assert_phase_detail_recording_context_required(
        &schema,
        "task_closure_recording_ready",
        &["task_number", "dispatch_id"],
        &mut issues,
    );
    assert_phase_detail_recording_context_required(
        &schema,
        "release_readiness_recording_ready",
        &["branch_closure_id"],
        &mut issues,
    );
    assert_phase_detail_recording_context_required(
        &schema,
        "release_blocker_resolution_required",
        &["branch_closure_id"],
        &mut issues,
    );
    assert_phase_detail_recording_context_required(
        &schema,
        "final_review_recording_ready",
        &["dispatch_id", "branch_closure_id"],
        &mut issues,
    );
    assert_phase_detail_field_forbidden_outside_allowed_phase_details(
        &schema,
        "recording_context",
        &[
            "task_closure_recording_ready",
            "release_readiness_recording_ready",
            "release_blocker_resolution_required",
            "final_review_recording_ready",
        ],
        &mut issues,
    );
    assert_phase_field_field_forbidden_outside_const_phase(
        &schema,
        "harness_phase",
        "executing",
        "execution_command_context",
        &mut issues,
    );
    assert_phase_detail_field_omitted_only_in_lanes(
        &schema,
        "recommended_command",
        &[
            "task_review_result_pending",
            "final_review_outcome_pending",
            "test_plan_refresh_required",
        ],
        &mut issues,
    );
    check_types!(
        "finish_review_gate_pass_branch_closure_id",
        ["string", "null"],
        optional
    );
    check_types!("execution_mode", ["string"], required);
    check_types!("execution_fingerprint", ["string"], required);
    check_types!("evidence_path", ["string"], required);
    check_types!("execution_started", ["string"], required);
    check_types!("reason_codes", ["array"], required);
    check_array_items!("reason_codes", ["string"]);
    check_types!("warning_codes", ["array"], required);
    check_array_items!("warning_codes", ["string"]);
    check_types!("active_task", ["integer", "null"], optional);
    check_types!("active_step", ["integer", "null"], optional);
    check_types!("blocking_task", ["integer", "null"], optional);
    check_types!("blocking_step", ["integer", "null"], optional);
    check_types!("resume_task", ["integer", "null"], optional);
    check_types!("resume_step", ["integer", "null"], optional);
    check_types!("plan_revision", ["integer"], required);

    issues
}

#[test]
fn task_packet_build_is_deterministic_for_fixed_timestamp() {
    let repo_root = unique_temp_dir("packet-deterministic");
    install_valid_artifacts(&repo_root);

    let spec = parse_spec_file(repo_root.join(SPEC_REL)).expect("spec should parse");
    let plan = parse_plan_file(repo_root.join(PLAN_REL)).expect("plan should parse");

    let first = build_task_packet_with_timestamp(&spec, &plan, 1, "2026-03-23T15:00:00Z")
        .expect("first packet should build");
    let second = build_task_packet_with_timestamp(&spec, &plan, 1, "2026-03-23T15:00:00Z")
        .expect("second packet should build");

    assert_eq!(first.packet_fingerprint, second.packet_fingerprint);
    assert_eq!(first.markdown, second.markdown);
    assert_eq!(first.task_title, "Establish the plan contract");
    assert!(
        first
            .markdown
            .contains("Execution-bound specs must include a parseable `Requirement Index`")
    );
}

#[test]
fn contract_schema_files_are_generated_with_expected_titles() {
    let schemas_dir = unique_temp_dir("contract-schemas");
    write_contract_schemas(&schemas_dir).expect("schemas should write");

    let analyze_schema = fs::read_to_string(schemas_dir.join("plan-contract-analyze.schema.json"))
        .expect("analyze schema should read");
    let packet_schema = fs::read_to_string(schemas_dir.join("plan-contract-packet.schema.json"))
        .expect("packet schema should read");

    assert!(analyze_schema.contains("\"title\": \"AnalyzePlanReport\""));
    assert!(packet_schema.contains("\"title\": \"TaskPacket\""));
}

#[test]
fn checked_in_plan_execution_schema_matches_generated_output() {
    let schemas_dir = unique_temp_dir("plan-execution-schema");
    write_plan_execution_schema(&schemas_dir).expect("plan execution schema should write");

    let generated = fs::read_to_string(schemas_dir.join("plan-execution-status.schema.json"))
        .expect("generated plan execution schema should read");
    let checked_in = fs::read_to_string(repo_fixture_path(
        "schemas/plan-execution-status.schema.json",
    ))
    .expect("checked-in plan execution schema should read");

    assert_eq!(generated.trim_end(), checked_in.trim_end());

    let missing_generated = plan_execution_status_schema_issues(&generated);
    assert!(
        missing_generated.is_empty(),
        "generated plan-execution schema is missing expanded status fields or shapes: {missing_generated:?}"
    );

    let missing_checked_in = plan_execution_status_schema_issues(&checked_in);
    assert!(
        missing_checked_in.is_empty(),
        "checked-in plan-execution schema is missing expanded status fields or shapes: {missing_checked_in:?}"
    );
}

#[test]
fn checked_in_repo_safety_schema_matches_generated_output_and_session_entry_schema_is_absent() {
    let schemas_dir = unique_temp_dir("policy-schemas");
    write_repo_safety_schema(&schemas_dir).expect("repo-safety schema should write");

    let generated_repo_safety =
        fs::read_to_string(schemas_dir.join("repo-safety-check.schema.json"))
            .expect("generated repo-safety schema should read");
    let checked_in_repo_safety =
        fs::read_to_string(repo_fixture_path("schemas/repo-safety-check.schema.json"))
            .expect("checked-in repo-safety schema should read");
    assert_eq!(
        generated_repo_safety.trim_end(),
        checked_in_repo_safety.trim_end()
    );

    assert!(
        !schemas_dir
            .join("session-entry-resolve.schema.json")
            .exists(),
        "active schema writers should not emit a session-entry schema artifact"
    );

    assert!(
        !repo_fixture_path("schemas/session-entry-resolve.schema.json").exists(),
        "session-entry schema should not remain an active checked-in schema artifact"
    );
}

#[test]
fn checked_in_update_check_schema_matches_generated_output() {
    let schemas_dir = unique_temp_dir("update-check-schema");
    write_update_check_schema(&schemas_dir).expect("update-check schema should write");

    let generated = fs::read_to_string(schemas_dir.join("update-check.schema.json"))
        .expect("generated update-check schema should read");
    let checked_in = fs::read_to_string(repo_fixture_path("schemas/update-check.schema.json"))
        .expect("checked-in update-check schema should read");

    assert_eq!(generated.trim_end(), checked_in.trim_end());
}

#[test]
fn checked_in_runtime_root_schema_matches_generated_output() {
    let schemas_dir = unique_temp_dir("runtime-root-schema");
    write_contract_schemas(&schemas_dir).expect("schemas should write");

    let generated = fs::read_to_string(schemas_dir.join("repo-runtime-root.schema.json"))
        .expect("generated runtime-root schema should read");
    let checked_in = fs::read_to_string(repo_fixture_path("schemas/repo-runtime-root.schema.json"))
        .expect("checked-in runtime-root schema should read");

    assert_eq!(generated.trim_end(), checked_in.trim_end());
}

#[test]
fn checked_in_workflow_schemas_match_generated_output() {
    let schemas_dir = unique_temp_dir("workflow-schemas");
    write_workflow_schemas(&schemas_dir).expect("workflow schemas should write");

    let generated_status = fs::read_to_string(schemas_dir.join("workflow-status.schema.json"))
        .expect("generated workflow-status schema should read");
    let checked_in_status =
        fs::read_to_string(repo_fixture_path("schemas/workflow-status.schema.json"))
            .expect("checked-in workflow-status schema should read");
    let generated_status_json: Value = serde_json::from_str(&generated_status)
        .expect("generated workflow-status schema should parse");
    let checked_in_status_json: Value = serde_json::from_str(&checked_in_status)
        .expect("checked-in workflow-status schema should parse");
    assert_eq!(generated_status_json, checked_in_status_json);

    let generated_resolve = fs::read_to_string(schemas_dir.join("workflow-resolve.schema.json"))
        .expect("generated workflow-resolve schema should read");
    let checked_in_resolve =
        fs::read_to_string(repo_fixture_path("schemas/workflow-resolve.schema.json"))
            .expect("checked-in workflow-resolve schema should read");
    let generated_resolve_json: Value = serde_json::from_str(&generated_resolve)
        .expect("generated workflow-resolve schema should parse");
    let checked_in_resolve_json: Value = serde_json::from_str(&checked_in_resolve)
        .expect("checked-in workflow-resolve schema should parse");
    assert_eq!(generated_resolve_json, checked_in_resolve_json);

    let generated_operator = fs::read_to_string(schemas_dir.join("workflow-operator.schema.json"))
        .expect("generated workflow-operator schema should read");
    let checked_in_operator =
        fs::read_to_string(repo_fixture_path("schemas/workflow-operator.schema.json"))
            .expect("checked-in workflow-operator schema should read");
    let generated_operator_json: Value = serde_json::from_str(&generated_operator)
        .expect("generated workflow-operator schema should parse");
    let checked_in_operator_json: Value = serde_json::from_str(&checked_in_operator)
        .expect("checked-in workflow-operator schema should parse");
    assert_eq!(generated_operator_json, checked_in_operator_json);
}

#[test]
fn workflow_operator_schema_pins_public_phase_and_routing_vocab() {
    let schemas_dir = unique_temp_dir("workflow-operator-schema-vocab");
    write_workflow_schemas(&schemas_dir).expect("workflow schemas should write");

    let generated_operator = fs::read_to_string(schemas_dir.join("workflow-operator.schema.json"))
        .expect("generated workflow-operator schema should read");
    let generated_operator_json: Value = serde_json::from_str(&generated_operator)
        .expect("generated workflow-operator schema should parse");
    let properties = generated_operator_json["properties"]
        .as_object()
        .expect("workflow-operator schema should contain properties");

    assert_eq!(properties["schema_version"]["const"], Value::from(1));
    for phase in [
        "executing",
        "task_closure_pending",
        "document_release_pending",
        "final_review_pending",
        "qa_pending",
        "ready_for_branch_completion",
        "handoff_required",
        "pivot_required",
    ] {
        assert!(
            generated_operator.contains(&format!("\"{phase}\"")),
            "workflow-operator schema should include phase '{phase}' in the public phase vocabulary"
        );
    }
    for value in ["none", "record_handoff", "record_pivot"] {
        assert!(
            generated_operator.contains(&format!("\"{value}\"")),
            "workflow-operator schema should include follow_up_override '{value}'"
        );
    }
    let mut issues = Vec::new();
    assert_schema_pointer_required(
        &generated_operator_json,
        "/$defs/WorkflowOperatorExecutionCommandContext",
        &["command_kind", "task_number", "step_id"],
        &mut issues,
    );
    assert_schema_pointer_types(
        &generated_operator_json,
        "/$defs/WorkflowOperatorExecutionCommandContext/properties/task_number",
        &["integer"],
        &mut issues,
    );
    assert_schema_pointer_types(
        &generated_operator_json,
        "/$defs/WorkflowOperatorExecutionCommandContext/properties/step_id",
        &["integer"],
        &mut issues,
    );
    assert_schema_pointer_value(
        &generated_operator_json,
        "/$defs/WorkflowOperatorExecutionCommandContext/additionalProperties",
        Value::Bool(false),
        &mut issues,
    );
    assert_schema_pointer_types(
        &generated_operator_json,
        "/$defs/WorkflowOperatorRecordingContext/properties/branch_closure_id",
        &["string"],
        &mut issues,
    );
    assert_schema_pointer_types(
        &generated_operator_json,
        "/$defs/WorkflowOperatorRecordingContext/properties/dispatch_id",
        &["string"],
        &mut issues,
    );
    assert_schema_pointer_types(
        &generated_operator_json,
        "/$defs/WorkflowOperatorRecordingContext/properties/task_number",
        &["integer"],
        &mut issues,
    );
    assert_schema_pointer_value(
        &generated_operator_json,
        "/$defs/WorkflowOperatorRecordingContext/minProperties",
        Value::from(1),
        &mut issues,
    );
    assert_schema_pointer_value(
        &generated_operator_json,
        "/$defs/WorkflowOperatorRecordingContext/additionalProperties",
        Value::Bool(false),
        &mut issues,
    );
    assert_schema_pointer_required(
        &generated_operator_json,
        "/$defs/WorkflowOperatorRecordingContext/anyOf/0",
        &["branch_closure_id"],
        &mut issues,
    );
    assert_schema_pointer_required(
        &generated_operator_json,
        "/$defs/WorkflowOperatorRecordingContext/anyOf/1",
        &["task_number", "dispatch_id"],
        &mut issues,
    );
    assert_phase_detail_recording_context_required(
        &generated_operator_json,
        "task_closure_recording_ready",
        &["task_number", "dispatch_id"],
        &mut issues,
    );
    assert_phase_detail_recording_context_required(
        &generated_operator_json,
        "release_readiness_recording_ready",
        &["branch_closure_id"],
        &mut issues,
    );
    assert_phase_detail_recording_context_required(
        &generated_operator_json,
        "release_blocker_resolution_required",
        &["branch_closure_id"],
        &mut issues,
    );
    assert_phase_detail_recording_context_required(
        &generated_operator_json,
        "final_review_recording_ready",
        &["dispatch_id", "branch_closure_id"],
        &mut issues,
    );
    assert_phase_detail_field_forbidden_outside_allowed_phase_details(
        &generated_operator_json,
        "recording_context",
        &[
            "task_closure_recording_ready",
            "release_readiness_recording_ready",
            "release_blocker_resolution_required",
            "final_review_recording_ready",
        ],
        &mut issues,
    );
    assert_phase_field_field_forbidden_outside_const_phase(
        &generated_operator_json,
        "phase",
        "executing",
        "execution_command_context",
        &mut issues,
    );
    assert_phase_detail_field_omitted_only_in_lanes(
        &generated_operator_json,
        "recommended_command",
        &[
            "task_review_result_pending",
            "final_review_outcome_pending",
            "test_plan_refresh_required",
        ],
        &mut issues,
    );
    assert!(
        issues.is_empty(),
        "workflow-operator schema should lock non-null context ids and non-empty recording_context shapes: {issues:?}"
    );
}

#[test]
fn runtime_root_schema_bounds_the_source_contract() {
    let schemas_dir = unique_temp_dir("runtime-root-source-schema");
    write_contract_schemas(&schemas_dir).expect("schemas should write");

    let generated = fs::read_to_string(schemas_dir.join("repo-runtime-root.schema.json"))
        .expect("generated runtime-root schema should read");

    assert!(
        generated.contains("\"enum\""),
        "runtime-root schema should bound the source field with an enum"
    );
    for source in [
        "unresolved",
        "featureforge_dir_env",
        "repo_local",
        "binary_adjacent",
        "canonical_install",
    ] {
        assert!(
            generated.contains(&format!("\"{source}\"")),
            "runtime-root schema should include {source} in the bounded source set"
        );
    }
}

#[test]
fn execution_evidence_markdown_remains_readable() {
    let repo_root = unique_temp_dir("execution-evidence");
    let evidence_path = repo_root.join(
        "docs/featureforge/execution-evidence/2026-03-22-runtime-integration-hardening-r1-evidence.md",
    );
    if let Some(parent) = evidence_path.parent() {
        fs::create_dir_all(parent).expect("execution evidence parent should exist");
    }
    fs::write(
        &evidence_path,
        "# Execution Evidence: 2026-03-22-runtime-integration-hardening\n\n**Plan Path:** docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md\n**Plan Revision:** 1\n**Source Spec Path:** docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md\n**Source Spec Revision:** 1\n\n## Step Evidence\n\n### Task 1 Step 1\n#### Attempt 1\n**Status:** Completed\n**Recorded At:** 2026-03-22T12:00:00Z\n**Execution Source:** featureforge:executing-plans\n**Claim:** Added route-time red fixtures.\n**Files:**\n- tests/workflow_runtime.rs\n**Verification:**\n- cargo test --test workflow_runtime\n**Invalidation Reason:** N/A\n",
    )
    .expect("execution evidence fixture should write");

    let evidence =
        read_execution_evidence(&evidence_path).expect("execution evidence should parse");

    assert_eq!(
        evidence.plan_path,
        "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md"
    );
    assert_eq!(evidence.plan_revision, 1);
    assert!(!evidence.steps.is_empty());
    assert_eq!(evidence.steps[0].task_number, 1);
    assert_eq!(evidence.steps[0].step_number, 1);
    assert_eq!(evidence.steps[0].status, "Completed");
    assert!(
        evidence.steps[0]
            .claim
            .contains("Added route-time red fixtures")
    );
}
