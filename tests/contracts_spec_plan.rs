use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use superpowers::contracts::plan::analyze_plan;
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
fn parse_spec_headers_and_index_exactly() {
    let spec = parse_spec_file(repo_fixture_path("tests/codex-runtime/fixtures/plan-contract/valid-spec.md"))
        .expect("valid spec fixture should parse");

    assert_eq!(spec.workflow_state, "CEO Approved");
    assert_eq!(spec.spec_revision, 1);
    assert_eq!(spec.last_reviewed_by, "plan-ceo-review");
    assert_eq!(spec.requirements.len(), 6);
    assert_eq!(spec.requirements[0].id, "REQ-001");
    assert_eq!(spec.requirements[0].kind, "behavior");
}

#[test]
fn analyze_valid_contract_fixture_reports_clean_coverage() {
    let repo_root = unique_temp_dir("contract-analyze-valid");
    install_valid_artifacts(&repo_root);

    let report = analyze_plan(repo_root.join(SPEC_REL), repo_root.join(PLAN_REL))
        .expect("valid fixture should analyze");

    assert_eq!(report.contract_state, "valid");
    assert_eq!(report.spec_path, SPEC_REL);
    assert_eq!(report.spec_revision, 1);
    assert_eq!(report.plan_path, PLAN_REL);
    assert_eq!(report.plan_revision, 1);
    assert_eq!(report.task_count, 2);
    assert_eq!(report.packet_buildable_tasks, 2);
    assert!(report.coverage_complete);
    assert!(report.open_questions_resolved);
    assert!(report.task_structure_valid);
    assert!(report.files_blocks_valid);
    assert!(report.reason_codes.is_empty());
    assert!(report.overlapping_write_scopes.is_empty());
    assert!(report.diagnostics.is_empty());
}

#[test]
fn analyze_plan_detects_stale_source_spec_linkage() {
    let repo_root = unique_temp_dir("contract-analyze-stale");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    let source = fs::read_to_string(&plan_path).expect("plan fixture should read");
    fs::write(
        &plan_path,
        source.replace("**Source Spec Revision:** 1", "**Source Spec Revision:** 2"),
    )
    .expect("stale plan fixture should write");

    let report = analyze_plan(repo_root.join(SPEC_REL), plan_path).expect("analysis should succeed");
    assert_eq!(report.contract_state, "invalid");
    assert_eq!(report.reason_codes, vec![String::from("stale_spec_plan_linkage")]);
    assert!(report.coverage_complete);
}
