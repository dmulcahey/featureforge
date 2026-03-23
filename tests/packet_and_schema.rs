use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use superpowers::contracts::evidence::read_execution_evidence;
use superpowers::contracts::packet::{build_task_packet_with_timestamp, write_contract_schemas};
use superpowers::contracts::plan::parse_plan_file;
use superpowers::contracts::spec::parse_spec_file;

const SPEC_REL: &str = "docs/superpowers/specs/2026-03-22-plan-contract-fixture-design.md";
const PLAN_REL: &str = "docs/superpowers/plans/2026-03-22-plan-contract-fixture.md";

fn unique_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("superpowers-{label}-{nanos}"));
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
        repo_fixture_path(&format!("tests/codex-runtime/fixtures/plan-contract/{fixture_name}")),
        destination,
    )
    .expect("fixture should copy");
}

fn install_valid_artifacts(repo_root: &Path) {
    install_fixture(repo_root, "valid-spec.md", SPEC_REL);
    install_fixture(repo_root, "valid-plan.md", PLAN_REL);
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
    assert!(first
        .markdown
        .contains("Execution-bound specs must include a parseable `Requirement Index`"));
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
fn legacy_execution_evidence_remains_readable() {
    let evidence = read_execution_evidence(repo_fixture_path(
        "docs/superpowers/execution-evidence/2026-03-22-runtime-integration-hardening-r1-evidence.md",
    ))
    .expect("legacy execution evidence should parse");

    assert_eq!(
        evidence.plan_path,
        "docs/superpowers/plans/2026-03-22-runtime-integration-hardening.md"
    );
    assert_eq!(evidence.plan_revision, 1);
    assert!(!evidence.steps.is_empty());
    assert_eq!(evidence.steps[0].task_number, 1);
    assert_eq!(evidence.steps[0].step_number, 1);
    assert_eq!(evidence.steps[0].status, "Completed");
    assert!(evidence.steps[0].claim.contains("Added route-time red fixtures"));
}
