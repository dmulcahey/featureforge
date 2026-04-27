#[path = "../src/contracts/headers.rs"]
mod headers_support;
#[path = "support/workflow_direct.rs"]
mod workflow_direct_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::cargo::CommandCargoExt;
use featureforge::contracts::packet::{build_task_packet_with_timestamp, write_contract_schemas};
use featureforge::contracts::plan::{
    PLAN_FIDELITY_RECEIPT_SCHEMA_VERSION, PLAN_FIDELITY_REQUIRED_SURFACES,
    ParallelWorktreeRequirement, PlanFidelityGateReport, PlanFidelityReceipt,
    PlanFidelityReviewerProvenance, PlanFidelityVerification, analyze_plan,
    evaluate_plan_fidelity_receipt_at_path, parse_plan_file, plan_fidelity_receipt_path_for_repo,
};
use featureforge::contracts::runtime::plan_fidelity_receipt_path;
use featureforge::contracts::spec::parse_spec_file;
use featureforge::git::discover_slug_identity;
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

fn unique_temp_dir_under_docs(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let dir = std::env::temp_dir()
        .join("docs")
        .join(format!("featureforge-{label}-{nanos}"));
    fs::create_dir_all(&dir).expect("temp dir under docs should be created");
    dir
}

fn repo_fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn active_engineering_approved_plans_reference_existing_source_specs() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plans_dir = repo_root.join("docs/featureforge/plans");
    let mut missing = Vec::new();

    let Ok(entries) = fs::read_dir(&plans_dir) else {
        return;
    };

    for entry in entries {
        let entry = entry.expect("plan directory entry should be readable");
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }

        let plan = parse_plan_file(&path).unwrap_or_else(|error| {
            panic!(
                "active approved plan fixture should parse: {}: {error}",
                path.display()
            )
        });
        if plan.workflow_state != "Engineering Approved" {
            continue;
        }

        let source_spec_path = repo_root.join(&plan.source_spec_path);
        if !source_spec_path.is_file() {
            missing.push(format!(
                "{} -> {}",
                path.strip_prefix(repo_root)
                    .unwrap_or(path.as_path())
                    .display(),
                plan.source_spec_path
            ));
        }
    }

    assert!(
        missing.is_empty(),
        "every active Engineering Approved plan should point at an existing source spec, missing: {missing:?}"
    );
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

fn install_valid_draft_artifacts(repo_root: &Path) {
    install_valid_artifacts(repo_root);
    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "**Workflow State:** Engineering Approved",
        "**Workflow State:** Draft",
    );
    replace_in_file(
        &plan_path,
        "**Last Reviewed By:** plan-eng-review",
        "**Last Reviewed By:** writing-plans",
    );
}

fn write_plan_fidelity_receipt(path: &Path, receipt: &PlanFidelityReceipt) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("receipt parent directories should exist");
    }
    fs::write(
        path,
        serde_json::to_string_pretty(receipt).expect("receipt should serialize"),
    )
    .expect("receipt should write");
}

struct PlanFidelityReviewArtifactInput<'a> {
    artifact_rel: &'a str,
    plan_path: &'a str,
    plan_revision: u32,
    spec_path: &'a str,
    spec_revision: u32,
    review_verdict: &'a str,
    reviewer_source: &'a str,
    reviewer_id: &'a str,
    verified_surfaces: &'a [&'a str],
}

fn write_plan_fidelity_review_artifact(
    repo_root: &Path,
    input: PlanFidelityReviewArtifactInput<'_>,
) {
    let artifact_path = repo_root.join(input.artifact_rel);
    let plan_fingerprint = featureforge::git::sha256_hex(
        &fs::read(repo_root.join(input.plan_path)).expect("plan fixture should be readable"),
    );
    let spec_fingerprint = featureforge::git::sha256_hex(
        &fs::read(repo_root.join(input.spec_path)).expect("spec fixture should be readable"),
    );
    let verified_requirement_ids = parse_spec_file(repo_root.join(input.spec_path))
        .map(|spec| {
            spec.requirements
                .iter()
                .map(|requirement| requirement.id.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent).expect("review artifact parent directories should exist");
    }
    fs::write(
        artifact_path,
        format!(
            "## Plan Fidelity Review Summary\n\n**Review Stage:** featureforge:plan-fidelity-review\n**Review Verdict:** {review_verdict}\n**Reviewed Plan:** `{plan_path}`\n**Reviewed Plan Revision:** {plan_revision}\n**Reviewed Plan Fingerprint:** {plan_fingerprint}\n**Reviewed Spec:** `{spec_path}`\n**Reviewed Spec Revision:** {spec_revision}\n**Reviewed Spec Fingerprint:** {spec_fingerprint}\n**Reviewer Source:** {reviewer_source}\n**Reviewer ID:** {reviewer_id}\n**Distinct From Stages:** featureforge:writing-plans, featureforge:plan-eng-review\n**Verified Surfaces:** {}\n**Verified Requirement IDs:** {}\n",
            input.verified_surfaces.join(", "),
            verified_requirement_ids.join(", "),
            review_verdict = input.review_verdict,
            plan_path = input.plan_path,
            plan_revision = input.plan_revision,
            spec_path = input.spec_path,
            spec_revision = input.spec_revision,
            reviewer_source = input.reviewer_source,
            reviewer_id = input.reviewer_id,
        ),
    )
    .expect("review artifact should write");
}

fn seed_direct_plan_fidelity_review_artifact(repo_root: &Path) -> (String, String) {
    let artifact_rel = ".featureforge/reviews/plan-fidelity-direct.md";
    write_plan_fidelity_review_artifact(
        repo_root,
        PlanFidelityReviewArtifactInput {
            artifact_rel,
            plan_path: PLAN_REL,
            plan_revision: 1,
            spec_path: SPEC_REL,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-019d",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    let artifact_source = fs::read(repo_root.join(artifact_rel))
        .expect("direct review artifact should be readable after write");
    (
        artifact_rel.to_owned(),
        featureforge::git::sha256_hex(&artifact_source),
    )
}

fn build_matching_plan_fidelity_receipt(repo_root: &Path) -> PlanFidelityReceipt {
    let spec = parse_spec_file(repo_root.join(SPEC_REL)).expect("spec fixture should parse");
    let plan = parse_plan_file(repo_root.join(PLAN_REL)).expect("plan fixture should parse");
    let report = analyze_plan(repo_root.join(SPEC_REL), repo_root.join(PLAN_REL))
        .expect("report should build");
    let (review_artifact_path, review_artifact_fingerprint) =
        seed_direct_plan_fidelity_review_artifact(repo_root);

    PlanFidelityReceipt {
        schema_version: PLAN_FIDELITY_RECEIPT_SCHEMA_VERSION,
        receipt_kind: String::from("plan_fidelity_receipt"),
        verdict: String::from("pass"),
        spec_path: spec.path.clone(),
        spec_revision: spec.spec_revision,
        spec_fingerprint: report.spec_fingerprint,
        plan_path: plan.path.clone(),
        plan_revision: plan.plan_revision,
        plan_fingerprint: report.plan_fingerprint,
        review_artifact_path,
        review_artifact_fingerprint,
        reviewer_provenance: PlanFidelityReviewerProvenance {
            review_stage: String::from("featureforge:plan-fidelity-review"),
            reviewer_source: String::from("fresh-context-subagent"),
            reviewer_id: String::from("reviewer-019d"),
            distinct_from_stages: vec![
                String::from("featureforge:writing-plans"),
                String::from("featureforge:plan-eng-review"),
            ],
        },
        verification: PlanFidelityVerification {
            checked_surfaces: PLAN_FIDELITY_REQUIRED_SURFACES
                .iter()
                .map(|surface| (*surface).to_owned())
                .collect(),
            verified_requirement_ids: spec
                .requirements
                .iter()
                .map(|requirement| requirement.id.clone())
                .collect(),
        },
    }
}

fn evaluate_plan_fidelity_gate(repo_root: &Path, receipt_path: &Path) -> PlanFidelityGateReport {
    let spec = parse_spec_file(repo_root.join(SPEC_REL)).expect("spec fixture should parse");
    let plan = parse_plan_file(repo_root.join(PLAN_REL)).expect("plan fixture should parse");
    evaluate_plan_fidelity_receipt_at_path(&spec, &plan, repo_root, receipt_path)
}

fn run(mut command: Command, context: &str) -> Output {
    command
        .output()
        .unwrap_or_else(|error| panic!("{context} should run: {error}"))
}

fn parse_success_json(output: &Output, context: &str) -> Value {
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid success json: {error}"))
}

fn parse_failure_json(output: &Output, context: &str) -> Value {
    assert!(
        !output.status.success(),
        "{context} should fail, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    serde_json::from_slice(payload)
        .unwrap_or_else(|error| panic!("{context} should emit valid failure json: {error}"))
}

fn packet_has_requirement_statement(packet: &Value, expected: &str) -> bool {
    packet["requirement_statements"]
        .as_array()
        .is_some_and(|requirements| {
            requirements
                .iter()
                .any(|requirement| requirement["statement"].as_str() == Some(expected))
        })
}

fn run_helper(repo_root: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    let mut command =
        Command::cargo_bin("featureforge").expect("featureforge cargo binary should exist");
    command
        .current_dir(repo_root)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["plan", "contract"])
        .args(args);
    run(command, context)
}

fn run_rust(repo_root: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    let mut command =
        Command::cargo_bin("featureforge").expect("featureforge cargo binary should exist");
    command
        .current_dir(repo_root)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["plan", "contract"])
        .args(args);
    run(command, context)
}

fn run_rust_from_dir(current_dir: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    let mut command =
        Command::cargo_bin("featureforge").expect("featureforge cargo binary should exist");
    command
        .current_dir(current_dir)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["plan", "contract"])
        .args(args);
    run(command, context)
}

fn run_record_plan_fidelity(
    repo_root: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Output {
    let direct_args = std::iter::once("workflow")
        .chain(std::iter::once("plan-fidelity"))
        .chain(args.iter().copied())
        .collect::<Vec<_>>();
    workflow_direct_support::try_run_workflow_output_direct(
        repo_root,
        state_dir,
        &direct_args,
        context,
        true,
    )
    .unwrap_or_else(|error| panic!("{context} direct plan-fidelity helper should run: {error}"))
    .unwrap_or_else(|| {
        panic!("{context} direct plan-fidelity helper should handle legacy workflow command")
    })
}

fn replace_in_file(path: &Path, search: &str, replacement: &str) {
    let source = fs::read_to_string(path).expect("fixture should be readable");
    assert!(
        source.contains(search),
        "fixture should contain target text: {search}"
    );
    fs::write(path, source.replacen(search, replacement, 1)).expect("fixture should be writable");
}

fn inline_spec_with_fenced_requirement_index(repo_root: &Path) {
    let spec_path = repo_root.join(SPEC_REL);
    if let Some(parent) = spec_path.parent() {
        fs::create_dir_all(parent).expect("spec fixture parent should exist");
    }
    fs::write(
        spec_path,
        r#"# Plan Contract Fixture Design

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

## Summary

Fixture spec for plan-contract helper regression coverage.

## Proposed Design

Example:

```markdown
## Requirement Index

- [REQ-999][behavior] Example requirement only.
```

## Requirement Index

- [REQ-001][behavior] Execution-bound specs must include a parseable `Requirement Index`.
- [REQ-002][behavior] Implementation plans must include a parseable `Requirement Coverage Matrix` mapping every indexed requirement to one or more tasks.
- [REQ-003][behavior] FeatureForge must expose `featureforge plan contract` to lint traceability and build canonical task packets.
- [DEC-001][decision] Markdown artifacts remain authoritative and helper output must preserve exact approved statements rather than paraphrase them.
- [NONGOAL-001][non-goal] Do not introduce hidden workflow authority outside repo-visible markdown artifacts.
- [VERIFY-001][verification] Regression coverage must cover missing indexes, missing coverage, unknown IDs, unresolved open questions, malformed task structure, malformed `Files:` blocks, path traversal rejection, and stale packet handling.
"#,
    )
    .expect("inline spec fixture should write");
}

#[test]
fn parse_spec_headers_and_index_exactly() {
    let spec = parse_spec_file(repo_fixture_path(
        "tests/codex-runtime/fixtures/plan-contract/valid-spec.md",
    ))
    .expect("valid spec fixture should parse");

    assert_eq!(spec.workflow_state, "CEO Approved");
    assert_eq!(spec.spec_revision, 1);
    assert_eq!(spec.last_reviewed_by, "plan-ceo-review");
    assert_eq!(spec.requirements.len(), 6);
    assert_eq!(spec.requirements[0].id, "REQ-001");
    assert_eq!(spec.requirements[0].kind, "behavior");
}

#[test]
fn parse_spec_headers_and_index_with_trailing_ceo_review_summary() {
    let repo_root = unique_temp_dir("contract-parse-trailing-ceo-summary");
    install_valid_artifacts(&repo_root);

    let spec_path = repo_root.join(SPEC_REL);
    let source = fs::read_to_string(&spec_path).expect("valid spec fixture should read");
    fs::write(
        &spec_path,
        format!(
            "{source}\n\n## CEO Review Summary\n\n**Review Status:** clear\n**Reviewed At:** 2026-03-24T13:42:28Z\n**Review Mode:** hold_scope\n**Reviewed Spec Revision:** 1\n**Critical Gaps:** 0\n**UI Design Intent Required:** no\n**Outside Voice:** skipped\n"
        ),
    )
    .expect("spec fixture with trailing summary should write");

    let spec = parse_spec_file(&spec_path).expect("spec with trailing summary should parse");
    assert_eq!(spec.workflow_state, "CEO Approved");
    assert_eq!(spec.spec_revision, 1);
    assert_eq!(spec.last_reviewed_by, "plan-ceo-review");
    assert_eq!(spec.requirements.len(), 6);
    assert_eq!(spec.requirements[0].id, "REQ-001");
}

#[test]
fn parsed_artifact_paths_stay_repo_relative_even_when_parent_path_contains_docs() {
    let repo_root = unique_temp_dir_under_docs("contract-parse-repo-relative-paths");
    install_valid_artifacts(&repo_root);

    let spec = parse_spec_file(repo_root.join(SPEC_REL)).expect("spec should parse");
    let plan = parse_plan_file(repo_root.join(PLAN_REL)).expect("plan should parse");

    assert_eq!(spec.path, SPEC_REL);
    assert_eq!(plan.path, PLAN_REL);
}

#[test]
fn shared_header_helper_returns_exact_required_header_values() {
    let source = "\
# Example Artifact
\n\
**Workflow State:** CEO Approved
\n\
**Spec Revision:** 7
\n\
**Source Spec:** `docs/featureforge/specs/example.md`
";

    assert_eq!(
        headers_support::parse_required_header(source, "Workflow State"),
        Some(String::from("CEO Approved"))
    );
    assert_eq!(
        headers_support::parse_required_header(source, "Spec Revision"),
        Some(String::from("7"))
    );
    assert_eq!(
        headers_support::parse_required_header(source, "Source Spec"),
        Some(String::from("`docs/featureforge/specs/example.md`"))
    );
    assert_eq!(
        headers_support::parse_required_header(source, "Missing Header"),
        None
    );
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
    assert_eq!(report.task_count, 3);
    assert_eq!(report.packet_buildable_tasks, 3);
    assert!(report.coverage_complete);
    assert!(report.task_contract_valid);
    assert!(report.task_goal_valid);
    assert!(report.task_context_sufficient);
    assert!(report.task_constraints_valid);
    assert!(report.task_done_when_deterministic);
    assert!(report.tasks_self_contained);
    assert!(report.task_structure_valid);
    assert!(report.files_blocks_valid);
    assert!(report.execution_strategy_present);
    assert!(report.dependency_diagram_present);
    assert!(report.execution_topology_valid);
    assert!(report.serial_hazards_resolved);
    assert!(report.parallel_lane_ownership_valid);
    assert!(report.parallel_workspace_isolation_valid);
    assert_eq!(report.parallel_worktree_groups, vec![vec![2, 3]]);
    assert_eq!(
        report.parallel_worktree_requirements,
        vec![ParallelWorktreeRequirement {
            tasks: vec![2, 3],
            declared_worktrees: 2,
            required_worktrees: 2,
        }]
    );
    assert!(report.reason_codes.is_empty());
    assert!(report.overlapping_write_scopes.is_empty());
    assert!(report.diagnostics.is_empty());
}

#[test]
fn analyze_plan_maps_task_contract_parse_failures_to_task_booleans() {
    let repo_root = unique_temp_dir("contract-analyze-task-booleans");
    let state_dir = unique_temp_dir("contract-analyze-task-booleans-state");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "**Goal:** The plan contract is represented as canonical traceability blocks that preserve exact approved wording.\n",
        "",
    );

    let report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze missing goal task contract",
        ),
        "rust analyze missing goal task contract",
    );

    assert_eq!(report["contract_state"], "invalid");
    assert_eq!(report["task_contract_valid"], false);
    assert_eq!(report["task_goal_valid"], false);
    assert_eq!(report["tasks_self_contained"], false);
    assert!(
        report["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("task_missing_goal"))
    );
}

#[test]
fn analyze_plan_maps_legacy_task_fields_to_task_contract_booleans() {
    let repo_root = unique_temp_dir("contract-analyze-legacy-task-booleans");
    let state_dir = unique_temp_dir("contract-analyze-legacy-task-booleans-state");
    install_fixture(&repo_root, "valid-spec.md", SPEC_REL);
    install_fixture(
        &repo_root,
        "transition-only/invalid-open-questions-plan.md",
        PLAN_REL,
    );

    let report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze legacy task contract",
        ),
        "rust analyze legacy task contract",
    );

    assert_eq!(report["contract_state"], "invalid");
    assert_eq!(report["task_contract_valid"], false);
    assert_eq!(report["task_goal_valid"], false);
    assert_eq!(report["task_context_sufficient"], false);
    assert_eq!(report["task_constraints_valid"], false);
    assert_eq!(report["task_done_when_deterministic"], false);
    assert_eq!(report["tasks_self_contained"], false);
    assert!(
        report["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("legacy_task_field"))
    );
}

#[test]
fn final_cutover_regression_matrix_pins_task_contract_and_packet_behavior() {
    let repo_root = unique_temp_dir("contract-final-cutover");
    let state_dir = unique_temp_dir("contract-final-cutover-state");

    install_valid_artifacts(&repo_root);
    replace_in_file(
        &repo_root.join(PLAN_REL),
        "**Context:**\n- Spec Coverage: REQ-001, REQ-002, DEC-001.\n\n",
        "",
    );
    let missing_context = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze final cutover missing context",
        ),
        "rust analyze final cutover missing context",
    );
    assert_eq!(missing_context["contract_state"], "invalid");
    assert_eq!(missing_context["task_contract_valid"], false);
    assert_eq!(missing_context["task_context_sufficient"], false);
    assert!(
        missing_context["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("task_missing_context"))
    );

    install_valid_draft_artifacts(&repo_root);
    replace_in_file(
        &repo_root.join(PLAN_REL),
        "**Context:**\n- Spec Coverage: REQ-001, REQ-002, DEC-001.\n\n",
        "**Context:**\n- Spec Coverage: REQ-001, REQ-002, VERIFY-001.\n\n",
    );
    let unrelated_context_id = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze final cutover unrelated context id",
        ),
        "rust analyze final cutover unrelated context id",
    );
    assert_eq!(unrelated_context_id["contract_state"], "invalid");
    assert_eq!(unrelated_context_id["task_contract_valid"], false);
    assert_eq!(unrelated_context_id["task_context_sufficient"], false);
    assert!(
        unrelated_context_id["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("missing_spec_context"))
    );

    install_valid_draft_artifacts(&repo_root);
    replace_in_file(
        &repo_root.join(PLAN_REL),
        "**Context:**\n- Spec Coverage: REQ-003, VERIFY-001.\n\n",
        "**Context:**\n- The packet-backed CLI surface is implemented in the runtime.\n\n",
    );
    let spec_sensitive_context = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze final cutover spec-sensitive context",
        ),
        "rust analyze final cutover spec-sensitive context",
    );
    assert_eq!(spec_sensitive_context["contract_state"], "invalid");
    assert_eq!(spec_sensitive_context["task_contract_valid"], false);
    assert_eq!(spec_sensitive_context["task_context_sufficient"], false);
    assert!(
        spec_sensitive_context["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("missing_spec_context"))
    );

    install_valid_draft_artifacts(&repo_root);
    replace_in_file(
        &repo_root.join(PLAN_REL),
        "**Goal:** The plan contract is represented as canonical traceability blocks that preserve exact approved wording.",
        "**Goal:** The plan contract is represented as canonical traceability blocks that preserve exact approved wording.\n**Plan Constraints:** legacy scalar constraints must be quarantined.",
    );
    let scalar_legacy_plan_constraints = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze final cutover scalar legacy plan constraints",
        ),
        "rust analyze final cutover scalar legacy plan constraints",
    );
    assert_eq!(scalar_legacy_plan_constraints["contract_state"], "invalid");
    assert_eq!(scalar_legacy_plan_constraints["task_contract_valid"], false);
    assert!(
        scalar_legacy_plan_constraints["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("legacy_task_field"))
    );

    for (label, invalid_done_when) in [
        ("vague", "- The implementation is robust."),
        ("generic", "- The implementation works."),
        (
            "generic-qualified",
            "- The implementation works as expected.",
        ),
        (
            "generic-ready-review",
            "- The changes are ready for review.",
        ),
        ("generic-support", "- Support parser migration."),
        ("generic-handle", "- Handle plan contract updates."),
    ] {
        install_valid_draft_artifacts(&repo_root);
        replace_in_file(
            &repo_root.join(PLAN_REL),
            "- The plan contract is represented as canonical traceability blocks that preserve exact approved wording.",
            invalid_done_when,
        );
        let vague_done_when = parse_success_json(
            &run_rust(
                &repo_root,
                &state_dir,
                &[
                    "analyze-plan",
                    "--spec",
                    SPEC_REL,
                    "--plan",
                    PLAN_REL,
                    "--format",
                    "json",
                ],
                &format!("rust analyze final cutover {label} done when"),
            ),
            &format!("rust analyze final cutover {label} done when"),
        );
        assert_eq!(vague_done_when["contract_state"], "invalid");
        assert_eq!(vague_done_when["task_contract_valid"], false);
        assert_eq!(vague_done_when["task_done_when_deterministic"], false);
        assert!(
            vague_done_when["reason_codes"]
                .as_array()
                .expect("reason_codes should be present")
                .iter()
                .any(|code| code.as_str() == Some("task_nondeterministic_done_when"))
        );
    }

    install_valid_artifacts(&repo_root);
    replace_in_file(
        &repo_root.join(PLAN_REL),
        "- The plan contract is represented as canonical traceability blocks that preserve exact approved wording.",
        "- Handle legacy task fields by returning the legacy_task_field reason code.",
    );
    let concrete_handle_done_when = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze final cutover concrete handle done when",
        ),
        "rust analyze final cutover concrete handle done when",
    );
    assert_eq!(concrete_handle_done_when["contract_state"], "valid");
    assert_eq!(
        concrete_handle_done_when["task_done_when_deterministic"],
        true
    );

    install_valid_draft_artifacts(&repo_root);
    replace_in_file(
        &repo_root.join(PLAN_REL),
        "**Goal:** The plan contract is represented as canonical traceability blocks that preserve exact approved wording.",
        "**Goal:** The plan contract is represented. It preserves approved wording.",
    );
    let multi_sentence_goal = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze final cutover multi sentence goal",
        ),
        "rust analyze final cutover multi sentence goal",
    );
    assert_eq!(multi_sentence_goal["contract_state"], "invalid");
    assert_eq!(multi_sentence_goal["task_contract_valid"], false);
    assert_eq!(multi_sentence_goal["task_goal_valid"], false);
    assert!(
        multi_sentence_goal["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("task_goal_not_atomic"))
    );

    install_valid_draft_artifacts(&repo_root);
    replace_in_file(
        &repo_root.join(PLAN_REL),
        "**Goal:** The plan contract is represented as canonical traceability blocks that preserve exact approved wording.\n\n**Context:**\n- Spec Coverage: REQ-001, REQ-002, DEC-001.",
        "**Context:**\n- Spec Coverage: REQ-001, REQ-002, DEC-001.\n\n**Goal:** The plan contract is represented as canonical traceability blocks that preserve exact approved wording.",
    );
    let wrong_field_order = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze final cutover wrong task field order",
        ),
        "rust analyze final cutover wrong task field order",
    );
    assert_eq!(wrong_field_order["contract_state"], "invalid");
    assert_eq!(wrong_field_order["task_contract_valid"], false);
    assert_eq!(wrong_field_order["tasks_self_contained"], false);
    assert!(
        wrong_field_order["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("task_field_order_invalid"))
    );

    install_valid_artifacts(&repo_root);
    let packet = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "build-task-packet",
                "--plan",
                PLAN_REL,
                "--task",
                "1",
                "--format",
                "json",
            ],
            "rust build final cutover task packet",
        ),
        "rust build final cutover task packet",
    );
    assert_eq!(packet["status"], "ok");
    assert_eq!(packet["packet_contract_version"], "task-obligation-v2");
    assert_eq!(packet["constraint_obligations"][0]["id"], "CONSTRAINT_1");
    assert_eq!(packet["done_when_obligations"][0]["id"], "DONE_WHEN_1");
    assert!(
        packet["packet_markdown"]
            .as_str()
            .is_some_and(|markdown| markdown.contains("## Task Contract"))
    );
}

#[test]
fn plan_and_runtime_task_contract_parsers_share_bullet_normalization() {
    let repo_root = unique_temp_dir("contract-shared-task-bullets");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "\n- Spec Coverage: REQ-001, REQ-002, DEC-001.\n",
        "\n  - Spec Coverage: REQ-001, REQ-002, DEC-001.\n",
    );
    replace_in_file(
        &plan_path,
        "\n- Preserve exact approved statements instead of paraphrasing them.\n",
        "\n  - Preserve exact approved statements instead of paraphrasing them.\n",
    );
    replace_in_file(
        &plan_path,
        "\n- Keep markdown authoritative and fail closed on malformed structure.\n",
        "\n  - Keep markdown authoritative and fail closed on malformed structure.\n",
    );
    replace_in_file(
        &plan_path,
        "\n- The plan contract is represented as canonical traceability blocks that preserve exact approved wording.\n",
        "\n  - The plan contract is represented as canonical traceability blocks that preserve exact approved wording.\n",
    );

    let parsed = parse_plan_file(&plan_path).expect("typed parser should accept indented bullets");
    assert_eq!(
        parsed.tasks[0].context,
        vec![String::from("Spec Coverage: REQ-001, REQ-002, DEC-001.")]
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("runtime analyzer should share the same task-contract parser");
    assert_eq!(report.contract_state, "valid");
    assert!(report.task_contract_valid);
}

#[test]
fn task_contract_accepts_step_free_tasks_in_typed_and_runtime_parsers() {
    let repo_root = unique_temp_dir("contract-step-free-task");
    let state_dir = unique_temp_dir("contract-step-free-task-state");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "\n- [ ] **Step 1: Parse the source requirement index**\n",
        "\n",
    );
    replace_in_file(
        &plan_path,
        "\n- [ ] **Step 2: Validate the coverage matrix against the indexed requirements**\n",
        "\n",
    );

    let typed_plan = parse_plan_file(&plan_path).expect("typed parser should accept no task steps");
    assert!(typed_plan.tasks[0].steps.is_empty());

    let report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze step-free task",
        ),
        "rust analyze step-free task",
    );
    assert_eq!(report["contract_state"], "valid");
    assert_eq!(report["task_structure_valid"], true);
    assert_eq!(report["task_contract_valid"], true);
}

#[test]
fn task_contract_rejects_duplicate_spec_coverage_in_typed_and_runtime_parsers() {
    let repo_root = unique_temp_dir("contract-duplicate-spec-coverage");
    let state_dir = unique_temp_dir("contract-duplicate-spec-coverage-state");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "**Spec Coverage:** REQ-001, REQ-002, DEC-001\n",
        "**Spec Coverage:** REQ-001, REQ-002, DEC-001\n**Spec Coverage:** VERIFY-001\n",
    );

    let typed_error =
        parse_plan_file(&plan_path).expect_err("typed parser should reject duplicate task fields");
    assert!(
        typed_error
            .to_string()
            .contains("duplicate `Spec Coverage` fields")
    );

    let report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze duplicate spec coverage",
        ),
        "rust analyze duplicate spec coverage",
    );
    assert_eq!(report["contract_state"], "invalid");
    assert_eq!(report["task_contract_valid"], false);
    assert!(
        report["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("duplicate_task_field"))
    );
}

#[test]
fn typed_plan_parser_rejects_duplicate_task_numbers_like_runtime_parser() {
    let repo_root = unique_temp_dir("contract-duplicate-task-number");
    let state_dir = unique_temp_dir("contract-duplicate-task-number-state");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "## Task 3: Prove packet-backed execution handoffs",
        "## Task 2: Prove packet-backed execution handoffs",
    );

    let typed_error =
        parse_plan_file(&plan_path).expect_err("typed parser should reject duplicate task numbers");
    assert!(
        typed_error
            .to_string()
            .contains("Task numbers must be unique within the plan.")
    );

    let report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze duplicate task number",
        ),
        "rust analyze duplicate task number",
    );
    assert_eq!(report["contract_state"], "invalid");
    assert_eq!(report["task_structure_valid"], false);
    assert!(
        report["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("malformed_task_structure"))
    );
}

#[test]
fn typed_plan_parser_rejects_missing_or_malformed_files_blocks_like_runtime_parser() {
    let repo_root = unique_temp_dir("contract-typed-files-blocks");
    let state_dir = unique_temp_dir("contract-typed-files-blocks-state");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "**Files:**\n- Create: `bin/featureforge`\n- Modify: `skills/writing-plans/SKILL.md`\n- Test: `cargo test --test contracts_spec_plan`\n\n- [ ] **Step 1: Parse the source requirement index**\n- [ ] **Step 2: Validate the coverage matrix against the indexed requirements**\n",
        "",
    );
    let missing_files_error =
        parse_plan_file(&plan_path).expect_err("typed parser should reject missing Files blocks");
    assert!(
        missing_files_error
            .to_string()
            .contains("missing a parseable Files block")
    );
    let missing_files_report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze missing Files block",
        ),
        "rust analyze missing Files block",
    );
    assert_eq!(missing_files_report["contract_state"], "invalid");
    assert!(
        missing_files_report["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("malformed_files_block"))
    );

    install_valid_artifacts(&repo_root);
    replace_in_file(
        &plan_path,
        "- Create: `bin/featureforge`",
        "Create: `bin/featureforge`",
    );
    let malformed_files_error = parse_plan_file(&plan_path)
        .expect_err("typed parser should reject malformed Files block entries");
    assert!(
        malformed_files_error
            .to_string()
            .contains("Malformed files block entry")
    );
    let malformed_files_report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze malformed Files block",
        ),
        "rust analyze malformed Files block",
    );
    assert_eq!(malformed_files_report["contract_state"], "invalid");
    assert!(
        malformed_files_report["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("malformed_files_block"))
    );
}

#[test]
fn task_contract_bullet_sections_reject_unstructured_prose_in_typed_and_runtime_parsers() {
    for (field, search, replacement) in [
        (
            "Context",
            "**Context:**\n- Spec Coverage: REQ-001, REQ-002, DEC-001.",
            "**Context:**\nSpec Coverage: REQ-001, REQ-002, DEC-001.",
        ),
        (
            "Constraints",
            "- Preserve exact approved statements instead of paraphrasing them.",
            "Preserve exact approved statements instead of paraphrasing them.",
        ),
        (
            "Done when",
            "**Done when:**\n- The plan contract is represented as canonical traceability blocks that preserve exact approved wording.",
            "**Done when:**\nThe plan contract is represented as canonical traceability blocks that preserve exact approved wording.",
        ),
    ] {
        let slug = field.to_ascii_lowercase().replace(' ', "-");
        let repo_root = unique_temp_dir(&format!("contract-task-field-bullets-{slug}"));
        let state_dir = unique_temp_dir(&format!("contract-task-field-bullets-{slug}-state"));
        install_valid_artifacts(&repo_root);

        let plan_path = repo_root.join(PLAN_REL);
        replace_in_file(&plan_path, search, replacement);

        let typed_error = match parse_plan_file(&plan_path) {
            Ok(_) => panic!("typed parser should reject malformed {field} bullets"),
            Err(error) => error,
        };
        assert!(
            typed_error
                .to_string()
                .contains(&format!("`{field}` entries must be bullets")),
            "typed parser error for {field} should mention malformed bullet entries: {typed_error}",
        );

        let report = parse_success_json(
            &run_rust(
                &repo_root,
                &state_dir,
                &[
                    "analyze-plan",
                    "--spec",
                    SPEC_REL,
                    "--plan",
                    PLAN_REL,
                    "--format",
                    "json",
                ],
                &format!("rust analyze malformed {field} task bullets"),
            ),
            &format!("rust analyze malformed {field} task bullets"),
        );
        assert_eq!(report["contract_state"], "invalid");
        assert_eq!(report["task_contract_valid"], false);
        assert!(
            report["reason_codes"]
                .as_array()
                .expect("reason_codes should be present")
                .iter()
                .any(|code| code.as_str() == Some("malformed_task_contract_field")),
            "runtime analyzer should report malformed_task_contract_field for {field}: {report}",
        );
    }
}

#[test]
fn task_contract_rejects_ambiguous_context_and_constraints_in_typed_and_runtime_analyzers() {
    for (field, search, replacement, boolean_key) in [
        (
            "Context",
            "- Spec Coverage: REQ-001, REQ-002, DEC-001.",
            "- Where possible, preserve Spec Coverage: REQ-001, REQ-002, DEC-001.",
            "task_context_sufficient",
        ),
        (
            "Constraints",
            "- Preserve exact approved statements instead of paraphrasing them.",
            "- Preserve exact approved statements where possible instead of paraphrasing them.",
            "task_constraints_valid",
        ),
    ] {
        let slug = field.to_ascii_lowercase();
        let repo_root = unique_temp_dir(&format!("contract-ambiguous-{slug}"));
        let state_dir = unique_temp_dir(&format!("contract-ambiguous-{slug}-state"));
        install_valid_artifacts(&repo_root);

        let plan_path = repo_root.join(PLAN_REL);
        replace_in_file(&plan_path, search, replacement);

        let typed_report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
            .expect("typed analyzer should report ambiguous task wording");
        assert_eq!(typed_report.contract_state, "invalid");
        assert!(!typed_report.task_contract_valid);
        assert!(
            typed_report
                .reason_codes
                .iter()
                .any(|code| code == "ambiguous_task_wording")
        );

        let runtime_report = parse_success_json(
            &run_rust(
                &repo_root,
                &state_dir,
                &[
                    "analyze-plan",
                    "--spec",
                    SPEC_REL,
                    "--plan",
                    PLAN_REL,
                    "--format",
                    "json",
                ],
                &format!("rust analyze ambiguous {field}"),
            ),
            &format!("rust analyze ambiguous {field}"),
        );
        assert_eq!(runtime_report["contract_state"], "invalid");
        assert_eq!(runtime_report["task_contract_valid"], false);
        assert_eq!(runtime_report[boolean_key], false);
        assert!(
            runtime_report["reason_codes"]
                .as_array()
                .expect("reason_codes should be present")
                .iter()
                .any(|code| code.as_str() == Some("ambiguous_task_wording")),
            "runtime analyzer should report ambiguous_task_wording for {field}: {runtime_report}",
        );
    }
}

#[test]
fn task_contract_rejects_unparsed_prose_after_steps_in_typed_and_runtime_parsers() {
    let repo_root = unique_temp_dir("contract-unparsed-prose-after-steps");
    let state_dir = unique_temp_dir("contract-unparsed-prose-after-steps-state");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "- [ ] **Step 2: Validate the coverage matrix against the indexed requirements**",
        "- [ ] **Step 2: Validate the coverage matrix against the indexed requirements**\nThis prose is not part of a step detail fence.",
    );

    let typed_error = parse_plan_file(&plan_path)
        .expect_err("typed parser should reject unparsed task prose after steps");
    assert!(
        typed_error
            .to_string()
            .contains("contains unparsed task body line"),
        "typed parser should name the unparsed task body line: {typed_error}",
    );

    let runtime_report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze unparsed prose after steps",
        ),
        "rust analyze unparsed prose after steps",
    );
    assert_eq!(runtime_report["contract_state"], "invalid");
    assert_eq!(runtime_report["task_structure_valid"], false);
    assert!(
        runtime_report["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .any(|code| code.as_str() == Some("malformed_task_structure")),
        "runtime analyzer should report malformed_task_structure: {runtime_report}",
    );
}

#[test]
fn task_contract_allows_runtime_execution_note_projection_after_steps() {
    let repo_root = unique_temp_dir("contract-runtime-execution-note-projection");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "- [ ] **Step 2: Validate the coverage matrix against the indexed requirements**",
        "- [ ] **Step 2: Validate the coverage matrix against the indexed requirements**\n  **Execution Note:** Active - Validate the coverage matrix against the indexed requirements",
    );

    parse_plan_file(&plan_path).expect("runtime execution-note projections should parse");
}

#[test]
fn task_contract_rejects_non_note_indented_content_after_execution_note_projection() {
    let repo_root = unique_temp_dir("contract-runtime-note-non-note-content");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "- [ ] **Step 2: Validate the coverage matrix against the indexed requirements**",
        "- [ ] **Step 2: Validate the coverage matrix against the indexed requirements**\n  **Execution Note:** Active - Validate the coverage matrix against the indexed requirements\n  This same-indent line is user-authored task prose.",
    );

    let typed_error = parse_plan_file(&plan_path)
        .expect_err("same-indent prose after an execution-note projection should remain semantic");
    assert!(
        typed_error
            .to_string()
            .contains("contains unparsed task body line"),
        "typed parser should reject the preserved same-indent prose: {typed_error}",
    );
}

#[test]
fn analyze_plan_rejects_missing_execution_strategy() {
    let repo_root = unique_temp_dir("contract-analyze-missing-execution-strategy");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "## Execution Strategy\n\n- Execute Task 1 serially. It establishes the contract surface before packet-backed execution splits into lane-owned work.\n- After Task 1, create two worktrees and run Tasks 2 and 3 in parallel:\n  - Task 2 owns the CLI and packaged-binary surfaces for packet-backed execution.\n  - Task 3 owns the prompt and shell-proof surfaces for packet-backed execution.\n\n",
        "",
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "invalid");
    assert!(!report.execution_strategy_present);
    assert!(!report.execution_topology_valid);
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "missing_execution_strategy")
    );
}

#[test]
fn analyze_plan_rejects_missing_dependency_diagram() {
    let repo_root = unique_temp_dir("contract-analyze-missing-dependency-diagram");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "## Dependency Diagram\n\n```text\nTask 1\n  |\n  +--> Task 2\n  |\n  +--> Task 3\n```\n\n",
        "",
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "invalid");
    assert!(!report.dependency_diagram_present);
    assert!(!report.execution_topology_valid);
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "missing_dependency_diagram")
    );
}

#[test]
fn analyze_plan_rejects_dependency_diagram_that_lies_about_parallel_edges() {
    let repo_root = unique_temp_dir("contract-analyze-lying-dependency-diagram");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "Task 1\n  |\n  +--> Task 2\n  |\n  +--> Task 3\n",
        "Task 1\n  |\n  v\nTask 2\n",
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "invalid");
    assert!(!report.execution_topology_valid);
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "dependency_diagram_mismatch")
    );
}

#[test]
fn analyze_plan_rejects_dependency_diagram_with_extra_unplanned_edges() {
    let repo_root = unique_temp_dir("contract-analyze-extra-dependency-edge");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "Task 1\n  |\n  +--> Task 2\n  |\n  +--> Task 3\n",
        "Task 1\n  |\n  +--> Task 2\n  |\n  +--> Task 3\nTask 2 -> Task 3\n",
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "invalid");
    assert!(!report.execution_topology_valid);
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "dependency_diagram_mismatch")
    );
}

#[test]
fn analyze_plan_rejects_unjustified_serial_execution() {
    let repo_root = unique_temp_dir("contract-analyze-unjustified-serial");
    install_fixture(&repo_root, "valid-serialized-plan.md", PLAN_REL);
    install_fixture(&repo_root, "valid-spec.md", SPEC_REL);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "Both tasks revise the same contract boundary and the plan intentionally keeps that hotspot in one shared branch lane.",
        "",
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "invalid");
    assert!(!report.serial_hazards_resolved);
    assert!(!report.execution_topology_valid);
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "serial_execution_needs_reason")
    );
}

#[test]
fn analyze_plan_rejects_serial_by_default_topology_without_real_hazard() {
    let repo_root = unique_temp_dir("contract-analyze-serial-by-default");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "## Execution Strategy\n\n- Execute Task 1 serially. It establishes the contract surface before packet-backed execution splits into lane-owned work.\n- After Task 1, create two worktrees and run Tasks 2 and 3 in parallel:\n  - Task 2 owns the CLI and packaged-binary surfaces for packet-backed execution.\n  - Task 3 owns the prompt and shell-proof surfaces for packet-backed execution.\n\n",
        "## Execution Strategy\n\n- Execute Tasks 1, 2, and 3 serially. Keep the plan easy to follow in one lane.\n\n",
    );
    replace_in_file(
        &plan_path,
        "Task 1\n  |\n  +--> Task 2\n  |\n  +--> Task 3\n",
        "Task 1\n  |\n  v\nTask 2\n  |\n  v\nTask 3\n",
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "invalid");
    assert!(!report.serial_hazards_resolved);
    assert!(!report.execution_topology_valid);
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "serial_execution_unproven")
    );
}

#[test]
fn analyze_plan_rejects_parallel_lane_without_ownership_guidance() {
    let repo_root = unique_temp_dir("contract-analyze-missing-parallel-ownership");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "  - Task 3 owns the prompt and shell-proof surfaces for packet-backed execution.\n",
        "",
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "invalid");
    assert!(!report.parallel_lane_ownership_valid);
    assert!(!report.execution_topology_valid);
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "parallel_lane_missing_ownership")
    );
}

#[test]
fn analyze_plan_rejects_parallel_lane_without_per_task_worktrees() {
    let repo_root = unique_temp_dir("contract-analyze-shared-worktree");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "create two worktrees and run Tasks 2 and 3 in parallel",
        "create one worktree and run Tasks 2 and 3 in parallel",
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "invalid");
    assert!(!report.parallel_workspace_isolation_valid);
    assert!(!report.execution_topology_valid);
    assert_eq!(
        report.parallel_worktree_requirements,
        vec![ParallelWorktreeRequirement {
            tasks: vec![2, 3],
            declared_worktrees: 1,
            required_worktrees: 2,
        }]
    );
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "parallel_workspace_isolation_mismatch")
    );
}

#[test]
fn analyze_plan_rejects_fake_parallel_hotspots() {
    let repo_root = unique_temp_dir("contract-analyze-fake-parallel-hotspots");
    install_fixture(&repo_root, "fake-parallel-hotspot-plan.md", PLAN_REL);
    install_fixture(&repo_root, "valid-spec.md", SPEC_REL);

    let report = analyze_plan(repo_root.join(SPEC_REL), repo_root.join(PLAN_REL))
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "invalid");
    assert!(!report.execution_topology_valid);
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "parallel_hotspot_conflict")
    );
}

#[test]
fn analyze_plan_rejects_invalid_path_traversal_fixture_in_library_path() {
    let repo_root = unique_temp_dir("contract-analyze-library-path-traversal");
    install_fixture(&repo_root, "valid-spec.md", SPEC_REL);
    install_fixture(&repo_root, "invalid-path-traversal-plan.md", PLAN_REL);

    let error = analyze_plan(repo_root.join(SPEC_REL), repo_root.join(PLAN_REL))
        .expect_err("library analyzer should reject path traversal files entries");

    assert_eq!(error.failure_class(), "InstructionParseFailed");
    assert!(error.message().contains("Malformed files block entry"));
}

#[test]
fn analyze_plan_rejects_invalid_plan_headers_in_library_path() {
    let repo_root = unique_temp_dir("contract-analyze-invalid-library-headers");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "**Workflow State:** Engineering Approved",
        "**Workflow State:** Legacy Draft",
    );
    replace_in_file(
        &plan_path,
        "**Execution Mode:** none",
        "**Execution Mode:** made-up-mode",
    );
    replace_in_file(
        &plan_path,
        "**Last Reviewed By:** plan-eng-review",
        "**Last Reviewed By:** somebody-else",
    );

    let error = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect_err("library analyzer should reject invalid plan headers");

    assert_eq!(error.failure_class(), "InstructionParseFailed");
    assert!(error.message().contains("header is missing or malformed"));
}

#[test]
fn analyze_plan_accepts_valid_last_directive_with_immediate_predecessor_edge() {
    let repo_root = unique_temp_dir("contract-analyze-valid-last");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "## Execution Strategy\n\n- Execute Task 1 serially. It establishes the contract surface before packet-backed execution splits into lane-owned work.\n- After Task 1, create two worktrees and run Tasks 2 and 3 in parallel:\n  - Task 2 owns the CLI and packaged-binary surfaces for packet-backed execution.\n  - Task 3 owns the prompt and shell-proof surfaces for packet-backed execution.\n\n",
        "## Execution Strategy\n\n- Execute Task 1 serially. It establishes the contract surface before reintegration work begins.\n- Execute Task 2 serially after Task 1. It is the reintegration seam before the final gate.\n- Execute Task 3 last as the final ratification gate.\n\n",
    );
    replace_in_file(
        &plan_path,
        "Task 1\n  |\n  +--> Task 2\n  |\n  +--> Task 3\n",
        "Task 1\n  |\n  v\nTask 2\n  |\n  v\nTask 3\n",
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "valid");
    assert!(report.execution_topology_valid);
    assert!(report.serial_hazards_resolved);
}

#[test]
fn analyze_plan_requires_last_directive_to_wait_for_all_current_sink_tasks() {
    let repo_root = unique_temp_dir("contract-analyze-last-sinks");
    install_fixture(&repo_root, "valid-spec.md", SPEC_REL);

    let plan_path = repo_root.join(PLAN_REL);
    fs::create_dir_all(
        plan_path
            .parent()
            .expect("custom last-plan fixture should have a parent directory"),
    )
    .expect("custom last-plan fixture parent should exist");
    fs::write(
        &plan_path,
        format!(
            "# Plan Contract Fixture\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{SPEC_REL}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n- REQ-002 -> Task 1\n- REQ-003 -> Task 2, Task 3, Task 4\n- DEC-001 -> Task 1\n- NONGOAL-001 -> Task 4\n- VERIFY-001 -> Task 2, Task 3, Task 4\n\n## Execution Strategy\n\n- Execute Task 1 serially. It establishes the contract surface before lane-owned work starts.\n- After Task 1, create two isolated worktrees and run Tasks 2 and 3 in parallel:\n  - Task 2 owns the CLI surface.\n  - Task 3 owns the prompt surface.\n- Execute Task 4 last as the final ratification gate.\n\n## Dependency Diagram\n\n```text\nTask 1 -> Task 2\nTask 1 -> Task 3\nTask 2 -> Task 4\n```\n\n## Task 1: Establish the plan contract\n\n**Spec Coverage:** REQ-001, REQ-002, DEC-001\n**Goal:** Establishes the shared contract boundary.\n\n**Context:**\n- Spec Coverage: REQ-001, REQ-002, DEC-001.\n\n**Constraints:**\n- Keep markdown authoritative.\n**Done when:**\n- Establishes the shared contract boundary.\n\n**Files:**\n- Modify: `src/contracts/plan.rs`\n\n- [ ] **Step 1: Establish the boundary**\n\n## Task 2: Own the CLI surface\n\n**Spec Coverage:** REQ-003, VERIFY-001\n**Goal:** Owns the CLI packet surface.\n\n**Context:**\n- Spec Coverage: REQ-003, VERIFY-001.\n\n**Constraints:**\n- Keep this lane disjoint.\n**Done when:**\n- Owns the CLI packet surface.\n\n**Files:**\n- Modify: `src/cli/plan_contract.rs`\n\n- [ ] **Step 1: Land the CLI slice**\n\n## Task 3: Own the prompt surface\n\n**Spec Coverage:** REQ-003, VERIFY-001\n**Goal:** Owns the prompt packet surface.\n\n**Context:**\n- Spec Coverage: REQ-003, VERIFY-001.\n\n**Constraints:**\n- Keep this lane disjoint.\n**Done when:**\n- Owns the prompt packet surface.\n\n**Files:**\n- Modify: `skills/subagent-driven-development/implementer-prompt.md`\n\n- [ ] **Step 1: Land the prompt slice**\n\n## Task 4: Ratify the combined result\n\n**Spec Coverage:** REQ-003, NONGOAL-001, VERIFY-001\n**Goal:** Ratifies the combined result after both lane-owned units finish.\n\n**Context:**\n- Spec Coverage: REQ-003, NONGOAL-001, VERIFY-001.\n\n**Constraints:**\n- Do not begin until every unfinished lane is complete.\n**Done when:**\n- Ratifies the combined result after both lane-owned units finish.\n\n**Files:**\n- Test: `tests/contracts_spec_plan.rs`\n\n- [ ] **Step 1: Ratify the combined result**\n"
        ),
    )
    .expect("custom last-plan fixture should write");

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "invalid");
    assert!(!report.execution_topology_valid);
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "dependency_diagram_mismatch")
    );
}

#[test]
fn analyze_plan_accepts_parallel_lane_with_isolated_worktree_wording() {
    let repo_root = unique_temp_dir("contract-analyze-isolated-worktree-wording");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "create two worktrees and run Tasks 2 and 3 in parallel",
        "create one isolated worktree per task and run Tasks 2 and 3 in parallel",
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "valid");
    assert!(report.parallel_workspace_isolation_valid);
}

#[test]
fn analyze_plan_accepts_multi_task_serial_reintegration_seam() {
    let repo_root = unique_temp_dir("contract-analyze-serial-reintegration-seam");
    install_fixture(&repo_root, "valid-spec.md", SPEC_REL);
    let plan_path = repo_root.join(PLAN_REL);
    fs::create_dir_all(
        plan_path
            .parent()
            .expect("custom seam-plan fixture should have a parent directory"),
    )
    .expect("custom seam-plan fixture parent should exist");
    fs::write(
        &plan_path,
        format!(
            "# Plan Contract Fixture\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{SPEC_REL}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n- REQ-002 -> Task 1\n- REQ-003 -> Task 2, Task 3, Task 4, Task 5\n- DEC-001 -> Task 1\n- NONGOAL-001 -> Task 5\n- VERIFY-001 -> Task 2, Task 3, Task 4, Task 5\n\n## Execution Strategy\n\n- Execute Task 1 serially. It establishes the contract surface before lane-owned work starts.\n- After Task 1, create one isolated worktree per task and run Tasks 2 and 3 in parallel:\n  - Task 2 owns the CLI lane.\n  - Task 3 owns the prompt lane.\n- Execute Tasks 4 and 5 serially after Tasks 2 and 3. They form the reintegration seam before finish gating.\n\n## Dependency Diagram\n\n```text\nTask 1 -> Task 2\nTask 1 -> Task 3\nTask 2 -> Task 4\nTask 3 -> Task 4\nTask 4 -> Task 5\n```\n\n## Task 1: Establish the plan contract\n\n**Spec Coverage:** REQ-001, REQ-002, DEC-001\n**Goal:** Establishes the shared contract boundary.\n\n**Context:**\n- Spec Coverage: REQ-001, REQ-002, DEC-001.\n\n**Constraints:**\n- Keep markdown authoritative.\n**Done when:**\n- Establishes the shared contract boundary.\n\n**Files:**\n- Modify: `src/contracts/plan.rs`\n\n- [ ] **Step 1: Establish the boundary**\n\n## Task 2: Own the CLI lane\n\n**Spec Coverage:** REQ-003, VERIFY-001\n**Goal:** Owns the CLI lane in isolation.\n\n**Context:**\n- Spec Coverage: REQ-003, VERIFY-001.\n\n**Constraints:**\n- Keep this lane disjoint.\n**Done when:**\n- Owns the CLI lane in isolation.\n\n**Files:**\n- Modify: `src/cli/plan_contract.rs`\n\n- [ ] **Step 1: Land the CLI lane**\n\n## Task 3: Own the prompt lane\n\n**Spec Coverage:** REQ-003, VERIFY-001\n**Goal:** Owns the prompt lane in isolation.\n\n**Context:**\n- Spec Coverage: REQ-003, VERIFY-001.\n\n**Constraints:**\n- Keep this lane disjoint.\n**Done when:**\n- Owns the prompt lane in isolation.\n\n**Files:**\n- Modify: `skills/subagent-driven-development/implementer-prompt.md`\n\n- [ ] **Step 1: Land the prompt lane**\n\n## Task 4: Reintegrate the parallel lanes\n\n**Spec Coverage:** REQ-003, VERIFY-001\n**Goal:** Reintegrates the two parallel lanes into shared glue.\n\n**Context:**\n- Spec Coverage: REQ-003, VERIFY-001.\n\n**Constraints:**\n- Do not begin until Tasks 2 and 3 are complete.\n**Done when:**\n- Reintegrates the two parallel lanes into shared glue.\n\n**Files:**\n- Modify: `src/execution/harness.rs`\n\n- [ ] **Step 1: Reintegrate the parallel lanes**\n\n## Task 5: Ratify the combined result\n\n**Spec Coverage:** REQ-003, NONGOAL-001, VERIFY-001\n**Goal:** Ratifies the combined result after the reintegration seam finishes.\n\n**Context:**\n- Spec Coverage: REQ-003, NONGOAL-001, VERIFY-001.\n\n**Constraints:**\n- Do not begin until Task 4 is complete.\n**Done when:**\n- Ratifies the combined result after the reintegration seam finishes.\n\n**Files:**\n- Test: `tests/contracts_spec_plan.rs`\n\n- [ ] **Step 1: Ratify the combined result**\n"
        ),
    )
    .expect("custom seam-plan fixture should write");

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "valid");
    assert!(report.serial_hazards_resolved);
    assert!(report.execution_topology_valid);
}

#[test]
fn analyze_plan_rejects_multi_task_serial_plain_seam_wording() {
    let repo_root = unique_temp_dir("contract-analyze-plain-seam-wording");
    install_fixture(&repo_root, "valid-spec.md", SPEC_REL);

    let plan_path = repo_root.join(PLAN_REL);
    fs::create_dir_all(
        plan_path
            .parent()
            .expect("custom plain-seam fixture should have a parent directory"),
    )
    .expect("custom plain-seam fixture parent should exist");
    fs::write(
        &plan_path,
        format!(
            "# Plan Contract Fixture\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{SPEC_REL}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n- REQ-002 -> Task 1\n- REQ-003 -> Task 2, Task 3, Task 4, Task 5\n- DEC-001 -> Task 1\n- NONGOAL-001 -> Task 5\n- VERIFY-001 -> Task 2, Task 3, Task 4, Task 5\n\n## Execution Strategy\n\n- Execute Task 1 serially. It establishes the contract surface before lane-owned work starts.\n- After Task 1, create one isolated worktree per task and run Tasks 2 and 3 in parallel:\n  - Task 2 owns the CLI lane.\n  - Task 3 owns the prompt lane.\n- Execute Tasks 4 and 5 serially after Tasks 2 and 3. They are the seam before finish gating.\n\n## Dependency Diagram\n\n```text\nTask 1 -> Task 2\nTask 1 -> Task 3\nTask 2 -> Task 4\nTask 3 -> Task 4\nTask 4 -> Task 5\n```\n\n## Task 1: Establish the plan contract\n\n**Spec Coverage:** REQ-001, REQ-002, DEC-001\n**Goal:** Establishes the shared contract boundary.\n\n**Context:**\n- Spec Coverage: REQ-001, REQ-002, DEC-001.\n\n**Constraints:**\n- Keep markdown authoritative.\n**Done when:**\n- Establishes the shared contract boundary.\n\n**Files:**\n- Modify: `src/contracts/plan.rs`\n\n- [ ] **Step 1: Establish the boundary**\n\n## Task 2: Own the CLI lane\n\n**Spec Coverage:** REQ-003, VERIFY-001\n**Goal:** Owns the CLI lane in isolation.\n\n**Context:**\n- Spec Coverage: REQ-003, VERIFY-001.\n\n**Constraints:**\n- Keep this lane disjoint.\n**Done when:**\n- Owns the CLI lane in isolation.\n\n**Files:**\n- Modify: `src/cli/plan_contract.rs`\n\n- [ ] **Step 1: Land the CLI lane**\n\n## Task 3: Own the prompt lane\n\n**Spec Coverage:** REQ-003, VERIFY-001\n**Goal:** Owns the prompt lane in isolation.\n\n**Context:**\n- Spec Coverage: REQ-003, VERIFY-001.\n\n**Constraints:**\n- Keep this lane disjoint.\n**Done when:**\n- Owns the prompt lane in isolation.\n\n**Files:**\n- Modify: `skills/subagent-driven-development/implementer-prompt.md`\n\n- [ ] **Step 1: Land the prompt lane**\n\n## Task 4: Reintegrate the parallel lanes\n\n**Spec Coverage:** REQ-003, VERIFY-001\n**Goal:** Reintegrates the two parallel lanes into shared glue.\n\n**Context:**\n- Spec Coverage: REQ-003, VERIFY-001.\n\n**Constraints:**\n- Do not begin until Tasks 2 and 3 are complete.\n**Done when:**\n- Reintegrates the two parallel lanes into shared glue.\n\n**Files:**\n- Modify: `src/execution/harness.rs`\n\n- [ ] **Step 1: Reintegrate the parallel lanes**\n\n## Task 5: Ratify the combined result\n\n**Spec Coverage:** REQ-003, NONGOAL-001, VERIFY-001\n**Goal:** Ratifies the combined result after the reintegration seam finishes.\n\n**Context:**\n- Spec Coverage: REQ-003, NONGOAL-001, VERIFY-001.\n\n**Constraints:**\n- Do not begin until Task 4 is complete.\n**Done when:**\n- Ratifies the combined result after the reintegration seam finishes.\n\n**Files:**\n- Test: `tests/contracts_spec_plan.rs`\n\n- [ ] **Step 1: Ratify the combined result**\n"
        ),
    )
    .expect("custom plain-seam fixture should write");

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("analysis should still produce a report");

    assert_eq!(report.contract_state, "invalid");
    assert!(!report.serial_hazards_resolved);
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "serial_execution_unproven")
    );
}

#[test]
fn plan_fidelity_receipt_validation_accepts_matching_pass_receipt() {
    let repo_root = unique_temp_dir("plan-fidelity-receipt-valid");
    install_valid_artifacts(&repo_root);
    let receipt_path = plan_fidelity_receipt_path_for_repo(&repo_root);
    write_plan_fidelity_receipt(
        &receipt_path,
        &build_matching_plan_fidelity_receipt(&repo_root),
    );

    let gate = evaluate_plan_fidelity_gate(&repo_root, &receipt_path);

    assert_eq!(gate.state, "pass");
    assert!(gate.reason_codes.is_empty());
    assert!(gate.diagnostics.is_empty());
}

#[test]
fn plan_fidelity_receipt_validation_rejects_stale_plan_revision_binding() {
    let repo_root = unique_temp_dir("plan-fidelity-receipt-stale");
    install_valid_artifacts(&repo_root);
    let receipt_path = plan_fidelity_receipt_path_for_repo(&repo_root);
    let mut receipt = build_matching_plan_fidelity_receipt(&repo_root);
    receipt.plan_revision += 1;
    write_plan_fidelity_receipt(&receipt_path, &receipt);

    let gate = evaluate_plan_fidelity_gate(&repo_root, &receipt_path);

    assert_eq!(gate.state, "stale");
    assert!(
        gate.reason_codes
            .iter()
            .any(|code| code == "stale_plan_fidelity_receipt")
    );
}

#[test]
fn plan_fidelity_receipt_validation_rejects_non_independent_reviewer_provenance() {
    let repo_root = unique_temp_dir("plan-fidelity-receipt-provenance");
    install_valid_artifacts(&repo_root);
    let receipt_path = plan_fidelity_receipt_path_for_repo(&repo_root);
    let mut receipt = build_matching_plan_fidelity_receipt(&repo_root);
    receipt.reviewer_provenance.review_stage = String::from("featureforge:writing-plans");
    receipt.reviewer_provenance.distinct_from_stages =
        vec![String::from("featureforge:writing-plans")];
    write_plan_fidelity_receipt(&receipt_path, &receipt);

    let gate = evaluate_plan_fidelity_gate(&repo_root, &receipt_path);

    assert_eq!(gate.state, "invalid");
    assert!(
        gate.reason_codes
            .iter()
            .any(|code| code == "plan_fidelity_receipt_not_independent")
    );
}

#[test]
fn plan_fidelity_receipt_validation_rejects_missing_execution_topology_verification() {
    let repo_root = unique_temp_dir("plan-fidelity-receipt-topology");
    install_valid_artifacts(&repo_root);
    let receipt_path = plan_fidelity_receipt_path_for_repo(&repo_root);
    let mut receipt = build_matching_plan_fidelity_receipt(&repo_root);
    receipt.verification.checked_surfaces = vec![String::from("requirement_index")];
    write_plan_fidelity_receipt(&receipt_path, &receipt);

    let gate = evaluate_plan_fidelity_gate(&repo_root, &receipt_path);

    assert_eq!(gate.state, "invalid");
    assert!(
        gate.reason_codes
            .iter()
            .any(|code| code == "plan_fidelity_receipt_missing_execution_topology_check")
    );
}

#[test]
fn plan_fidelity_receipt_validation_rejects_old_two_surface_receipts() {
    let repo_root = unique_temp_dir("plan-fidelity-receipt-old-surfaces");
    install_valid_artifacts(&repo_root);
    let receipt_path = plan_fidelity_receipt_path_for_repo(&repo_root);
    let mut receipt = build_matching_plan_fidelity_receipt(&repo_root);
    receipt.verification.checked_surfaces = vec![
        String::from("requirement_index"),
        String::from("execution_topology"),
    ];
    write_plan_fidelity_receipt(&receipt_path, &receipt);

    let gate = evaluate_plan_fidelity_gate(&repo_root, &receipt_path);

    assert_eq!(gate.state, "invalid");
    assert!(
        gate.reason_codes
            .iter()
            .any(|code| code == "plan_fidelity_receipt_missing_task_contract_check")
    );
    assert!(
        gate.reason_codes
            .iter()
            .any(|code| code == "plan_fidelity_receipt_missing_task_determinism_check")
    );
    assert!(
        gate.reason_codes
            .iter()
            .any(|code| code == "plan_fidelity_receipt_missing_spec_reference_fidelity_check")
    );
}

#[test]
fn plan_fidelity_receipt_validation_rejects_pre_expansion_schema_version() {
    let repo_root = unique_temp_dir("plan-fidelity-receipt-old-schema");
    install_valid_artifacts(&repo_root);
    let receipt_path = plan_fidelity_receipt_path_for_repo(&repo_root);
    let mut receipt = build_matching_plan_fidelity_receipt(&repo_root);
    receipt.schema_version = 2;
    write_plan_fidelity_receipt(&receipt_path, &receipt);

    let gate = evaluate_plan_fidelity_gate(&repo_root, &receipt_path);

    assert_eq!(gate.state, "malformed");
    assert!(
        gate.reason_codes
            .iter()
            .any(|code| code == "malformed_plan_fidelity_receipt")
    );
}

#[test]
fn plan_fidelity_receipt_validation_rejects_missing_or_drifted_review_artifacts() {
    let repo_root = unique_temp_dir("plan-fidelity-receipt-artifact-binding");
    install_valid_artifacts(&repo_root);
    let receipt_path = plan_fidelity_receipt_path_for_repo(&repo_root);
    let mut receipt = build_matching_plan_fidelity_receipt(&repo_root);
    fs::remove_file(repo_root.join(&receipt.review_artifact_path))
        .expect("review artifact should be removable for missing-artifact coverage");
    write_plan_fidelity_receipt(&receipt_path, &receipt);

    let missing_artifact_gate = evaluate_plan_fidelity_gate(&repo_root, &receipt_path);
    assert_eq!(missing_artifact_gate.state, "invalid");
    assert!(
        missing_artifact_gate
            .reason_codes
            .iter()
            .any(|code| code == "plan_fidelity_review_artifact_missing")
    );

    let (artifact_rel, artifact_fingerprint) =
        seed_direct_plan_fidelity_review_artifact(&repo_root);
    receipt.review_artifact_path = artifact_rel;
    receipt.review_artifact_fingerprint = artifact_fingerprint;
    write_plan_fidelity_receipt(&receipt_path, &receipt);
    fs::write(
        repo_root.join(&receipt.review_artifact_path),
        "tampered review artifact contents\n",
    )
    .expect("tampered review artifact should write");

    let mismatch_gate = evaluate_plan_fidelity_gate(&repo_root, &receipt_path);
    assert_eq!(mismatch_gate.state, "invalid");
    assert!(
        mismatch_gate
            .reason_codes
            .iter()
            .any(|code| code == "plan_fidelity_review_artifact_fingerprint_mismatch")
    );

    write_plan_fidelity_review_artifact(
        &repo_root,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-old-surfaces.md",
            plan_path: PLAN_REL,
            plan_revision: 1,
            spec_path: SPEC_REL,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-019d",
            verified_surfaces: &["requirement_index", "execution_topology"],
        },
    );
    let old_surface_artifact =
        repo_root.join(".featureforge/reviews/plan-fidelity-old-surfaces.md");
    receipt = build_matching_plan_fidelity_receipt(&repo_root);
    receipt.review_artifact_path =
        String::from(".featureforge/reviews/plan-fidelity-old-surfaces.md");
    receipt.review_artifact_fingerprint = featureforge::git::sha256_hex(
        &fs::read(old_surface_artifact).expect("old artifact should read"),
    );
    write_plan_fidelity_receipt(&receipt_path, &receipt);

    let invalid_artifact_gate = evaluate_plan_fidelity_gate(&repo_root, &receipt_path);
    assert_eq!(invalid_artifact_gate.state, "invalid");
    assert!(
        invalid_artifact_gate
            .reason_codes
            .iter()
            .any(|code| code == "plan_fidelity_review_artifact_invalid")
    );
}

#[test]
fn exported_analyze_plan_reads_runtime_owned_plan_fidelity_receipt_path() {
    let repo_root = unique_temp_dir("contract-analyze-runtime-plan-fidelity-path");
    install_valid_draft_artifacts(&repo_root);
    let receipt_path = plan_fidelity_receipt_path_for_repo(&repo_root);
    write_plan_fidelity_receipt(
        &receipt_path,
        &build_matching_plan_fidelity_receipt(&repo_root),
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), repo_root.join(PLAN_REL))
        .expect("direct analyze_plan helper should read the runtime-owned receipt path");

    assert_eq!(report.contract_state, "valid");
    assert_eq!(report.plan_fidelity_receipt.state, "pass");
}

#[test]
fn analyze_plan_cli_resolves_repo_relative_paths_from_subdirectories() {
    let repo_root = unique_temp_dir("contract-analyze-cli-subdir");
    let state_dir = unique_temp_dir("contract-analyze-cli-subdir-state");
    install_valid_draft_artifacts(&repo_root);
    write_plan_fidelity_review_artifact(
        &repo_root,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-cli-subdir.md",
            plan_path: PLAN_REL,
            plan_revision: 1,
            spec_path: SPEC_REL,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "independent-reviewer-subdir",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    parse_success_json(
        &run_record_plan_fidelity(
            &repo_root,
            &state_dir,
            &[
                "record",
                "--plan",
                PLAN_REL,
                "--review-artifact",
                ".featureforge/reviews/plan-fidelity-cli-subdir.md",
                "--json",
            ],
            "record plan-fidelity receipt for subdirectory analyze-plan coverage",
        ),
        "record plan-fidelity receipt for subdirectory analyze-plan coverage",
    );
    fs::create_dir_all(repo_root.join("src/runtime")).expect("subdirectory should exist");

    let report = parse_success_json(
        &run_rust_from_dir(
            &repo_root.join("src/runtime"),
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "analyze-plan should resolve repo-relative paths from subdirectories",
        ),
        "analyze-plan should resolve repo-relative paths from subdirectories",
    );

    assert_eq!(report["contract_state"], "valid");
    assert_eq!(report["plan_fidelity_receipt"]["state"], "pass");
}

#[test]
fn analyze_valid_contract_fixture_with_trailing_engineering_review_summary() {
    let repo_root = unique_temp_dir("contract-analyze-trailing-eng-summary");
    let state_dir = unique_temp_dir("contract-analyze-trailing-eng-summary-state");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    let source = fs::read_to_string(&plan_path).expect("valid plan fixture should read");
    fs::write(
        &plan_path,
        format!(
            "{source}\n\n## Engineering Review Summary\n\n**Review Status:** clear\n**Reviewed At:** 2026-03-24T16:02:11Z\n**Review Mode:** big_change\n**Reviewed Plan Revision:** 1\n**Critical Gaps:** 0\n**Browser QA Required:** yes\n**Test Plan Artifact:** `~/.featureforge/projects/example/example-branch-test-plan-20260324T160211Z.md`\n**Outside Voice:** fresh-context-subagent\n"
        ),
    )
    .expect("plan fixture with trailing summary should write");

    let report = analyze_plan(repo_root.join(SPEC_REL), plan_path.clone())
        .expect("analysis should tolerate trailing engineering summary");
    assert_eq!(report.contract_state, "valid");
    assert_eq!(report.task_count, 3);
    assert_eq!(report.packet_buildable_tasks, 3);
    assert!(report.coverage_complete);

    let lint = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &["lint", "--spec", SPEC_REL, "--plan", PLAN_REL],
            "rust lint with trailing engineering summary",
        ),
        "rust lint with trailing engineering summary",
    );
    assert_eq!(lint["status"], "ok");
    assert_eq!(lint["plan_task_count"], 3);
    assert_eq!(lint["coverage"]["REQ-001"][0], 1);
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

    let report =
        analyze_plan(repo_root.join(SPEC_REL), plan_path).expect("analysis should succeed");
    assert_eq!(report.contract_state, "invalid");
    assert_eq!(
        report.reason_codes,
        vec![String::from("stale_spec_plan_linkage")]
    );
    assert!(report.coverage_complete);
}

#[test]
fn analyze_valid_contract_fixture_with_checked_steps_and_fenced_details() {
    let repo_root = unique_temp_dir("contract-analyze-checked-steps");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    let source = fs::read_to_string(&plan_path).expect("valid plan fixture should read");
    let source = source
        .replace(
            "- [ ] **Step 1: Parse the source requirement index**",
            "- [x] **Step 1: Parse the source requirement index**\n```text\nchecked step detail fixture\n```",
        )
        .replace(
            "- [ ] **Step 2: Validate the coverage matrix against the indexed requirements**",
            "- [x] **Step 2: Validate the coverage matrix against the indexed requirements**\n```text\ncoverage detail fixture\n```",
        )
        .replace(
            "- [ ] **Step 1: Build canonical task packets**",
            "- [x] **Step 1: Build canonical task packets**\n```text\npacket detail fixture\n```",
        )
        .replace(
            "- [ ] **Step 2: Rebuild stale packets from the current approved artifacts**",
            "- [x] **Step 2: Rebuild stale packets from the current approved artifacts**\n```text\nrebuild detail fixture\n```",
        );
    fs::write(&plan_path, source).expect("plan fixture with checked step details should write");

    let report = analyze_plan(repo_root.join(SPEC_REL), &plan_path)
        .expect("checked steps with fenced details should analyze");

    assert_eq!(report.contract_state, "valid");
    assert_eq!(report.task_count, 3);
    assert_eq!(report.packet_buildable_tasks, 3);
    assert!(report.coverage_complete);
}

#[test]
fn analyze_plan_rejects_malformed_checked_step_entries() {
    let repo_root = unique_temp_dir("contract-analyze-malformed-checked-step");
    install_valid_artifacts(&repo_root);

    let plan_path = repo_root.join(PLAN_REL);
    replace_in_file(
        &plan_path,
        "- [ ] **Step 1: Parse the source requirement index**",
        "- [x] **Step 1 Parse the source requirement index**",
    );

    let error = analyze_plan(repo_root.join(SPEC_REL), plan_path)
        .expect_err("malformed checked step entry should fail closed");

    assert_eq!(error.failure_class(), "InstructionParseFailed");
    assert!(
        error
            .message()
            .contains("Malformed step entry: - [x] **Step 1 Parse the source requirement index**"),
        "unexpected diagnostic: {}",
        error.message()
    );
}

#[test]
fn lint_valid_contract_matches_helper_and_canonical_cli() {
    let repo_root = unique_temp_dir("contract-lint-valid-cli");
    let state_dir = unique_temp_dir("contract-lint-valid-state");
    install_valid_artifacts(&repo_root);

    let helper = parse_success_json(
        &run_helper(
            &repo_root,
            &state_dir,
            &["lint", "--spec", SPEC_REL, "--plan", PLAN_REL],
            "helper lint valid contract",
        ),
        "helper lint valid contract",
    );
    let rust = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &["lint", "--spec", SPEC_REL, "--plan", PLAN_REL],
            "rust lint valid contract",
        ),
        "rust lint valid contract",
    );

    assert_eq!(rust, helper);
    assert_eq!(rust["status"], "ok");
    assert_eq!(rust["spec_requirement_count"], 6);
    assert_eq!(rust["plan_task_count"], 3);
    assert_eq!(rust["coverage"]["REQ-001"][0], 1);
    assert_eq!(rust["coverage"]["REQ-003"][0], 2);
    assert_eq!(rust["coverage"]["REQ-003"][1], 3);
}

#[test]
fn lint_ignores_fenced_example_requirement_index_blocks() {
    let repo_root = unique_temp_dir("contract-lint-fenced");
    let state_dir = unique_temp_dir("contract-lint-fenced-state");
    install_fixture(&repo_root, "valid-plan.md", PLAN_REL);
    inline_spec_with_fenced_requirement_index(&repo_root);

    let rust = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &["lint", "--spec", SPEC_REL, "--plan", PLAN_REL],
            "rust lint fenced requirement index fixture",
        ),
        "rust lint fenced requirement index fixture",
    );
    assert_eq!(rust["status"], "ok");
    assert_eq!(rust["spec_requirement_count"], 6);
    assert_eq!(rust["plan_task_count"], 3);
}

#[test]
fn analyze_plan_cli_reports_fixture_matrix() {
    let repo_root = unique_temp_dir("contract-analyze-cli-matrix");
    let state_dir = unique_temp_dir("contract-analyze-cli-state");

    install_valid_artifacts(&repo_root);
    let valid = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze valid contract",
        ),
        "rust analyze valid contract",
    );
    assert_eq!(valid["contract_state"], "valid");
    assert_eq!(valid["task_count"], 3);
    assert_eq!(valid["packet_buildable_tasks"], 3);
    assert_eq!(valid["coverage_complete"], true);
    assert_eq!(valid["task_contract_valid"], true);
    assert_eq!(valid["task_goal_valid"], true);
    assert_eq!(valid["task_context_sufficient"], true);
    assert_eq!(valid["task_constraints_valid"], true);
    assert_eq!(valid["task_done_when_deterministic"], true);
    assert_eq!(valid["tasks_self_contained"], true);
    assert_eq!(valid["task_structure_valid"], true);
    assert_eq!(valid["files_blocks_valid"], true);
    assert_eq!(valid["execution_strategy_present"], true);
    assert_eq!(valid["dependency_diagram_present"], true);
    assert_eq!(valid["execution_topology_valid"], true);
    assert_eq!(valid["serial_hazards_resolved"], true);
    assert_eq!(valid["parallel_lane_ownership_valid"], true);
    assert_eq!(valid["parallel_workspace_isolation_valid"], true);
    assert_eq!(
        valid["parallel_worktree_groups"],
        serde_json::json!([[2, 3]])
    );
    assert_eq!(
        valid["parallel_worktree_requirements"],
        serde_json::json!([{
            "tasks": [2, 3],
            "declared_worktrees": 2,
            "required_worktrees": 2
        }])
    );
    assert_eq!(valid["reason_codes"], Value::Array(vec![]));
    assert_eq!(valid["diagnostics"], Value::Array(vec![]));

    let stale_plan = repo_root.join(PLAN_REL);
    replace_in_file(
        &stale_plan,
        "**Source Spec Revision:** 1",
        "**Source Spec Revision:** 2",
    );
    let stale = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze stale source linkage",
        ),
        "rust analyze stale source linkage",
    );
    assert_eq!(stale["contract_state"], "invalid");
    assert_eq!(stale["reason_codes"][0], "stale_spec_plan_linkage");
    install_valid_artifacts(&repo_root);

    install_fixture(&repo_root, "invalid-missing-coverage-plan.md", PLAN_REL);
    let missing_coverage = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze missing coverage",
        ),
        "rust analyze missing coverage",
    );
    assert_eq!(missing_coverage["contract_state"], "invalid");
    assert_eq!(missing_coverage["coverage_complete"], false);
    assert_eq!(missing_coverage["packet_buildable_tasks"], 2);
    assert_eq!(
        missing_coverage["reason_codes"][0],
        "missing_requirement_coverage"
    );
    install_valid_artifacts(&repo_root);

    install_fixture(&repo_root, "invalid-malformed-files-plan.md", PLAN_REL);
    let malformed_files = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze malformed files",
        ),
        "rust analyze malformed files",
    );
    assert_eq!(malformed_files["contract_state"], "invalid");
    assert_eq!(malformed_files["packet_buildable_tasks"], 1);
    assert_eq!(malformed_files["files_blocks_valid"], false);
    assert_eq!(malformed_files["reason_codes"][0], "malformed_files_block");
    install_valid_artifacts(&repo_root);

    let spec_path = repo_root.join(SPEC_REL);
    replace_in_file(&spec_path, "**Spec Revision:** 1", "**Spec Revision:** one");
    let missing_spec_revision = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze missing spec revision",
        ),
        "rust analyze missing spec revision",
    );
    assert_eq!(missing_spec_revision["contract_state"], "invalid");
    assert_eq!(
        missing_spec_revision["reason_codes"][0],
        "missing_spec_revision"
    );
    install_valid_artifacts(&repo_root);

    install_fixture(
        &repo_root,
        "invalid-malformed-task-structure-plan.md",
        PLAN_REL,
    );
    let malformed_task = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze malformed task structure",
        ),
        "rust analyze malformed task structure",
    );
    assert_eq!(malformed_task["contract_state"], "invalid");
    assert_eq!(malformed_task["task_structure_valid"], false);
    assert_eq!(
        malformed_task["reason_codes"][0],
        "malformed_task_structure"
    );
    install_valid_artifacts(&repo_root);

    let coverage_plan = repo_root.join(PLAN_REL);
    replace_in_file(&coverage_plan, "- REQ-001 -> Task 1", "- REQ-001 -> Task 9");
    let coverage_mismatch = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze coverage mismatch",
        ),
        "rust analyze coverage mismatch",
    );
    assert_eq!(coverage_mismatch["contract_state"], "invalid");
    assert_eq!(
        coverage_mismatch["reason_codes"][0],
        "coverage_matrix_mismatch"
    );
    install_valid_artifacts(&repo_root);

    install_fixture(&repo_root, "overlapping-write-scopes-plan.md", PLAN_REL);
    let overlapping = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze overlapping write scopes",
        ),
        "rust analyze overlapping write scopes",
    );
    assert_eq!(overlapping["contract_state"], "valid");
    assert_eq!(
        overlapping["overlapping_write_scopes"][0]["path"],
        "skills/writing-plans/SKILL.md"
    );
    assert_eq!(
        overlapping["overlapping_write_scopes"][0]["tasks"],
        serde_json::json!([1, 2])
    );
    install_valid_artifacts(&repo_root);

    install_fixture(&repo_root, "fake-parallel-hotspot-plan.md", PLAN_REL);
    let fake_parallel = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze fake parallel hotspot plan",
        ),
        "rust analyze fake parallel hotspot plan",
    );
    assert_eq!(fake_parallel["contract_state"], "invalid");
    assert_eq!(fake_parallel["execution_topology_valid"], false);
    assert_eq!(
        fake_parallel["reason_codes"][0],
        "parallel_hotspot_conflict"
    );
    install_valid_artifacts(&repo_root);

    install_fixture(&repo_root, "invalid-missing-index-spec.md", SPEC_REL);
    let missing_index = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze missing requirement index",
        ),
        "rust analyze missing requirement index",
    );
    assert_eq!(missing_index["contract_state"], "invalid");
    assert_eq!(
        missing_index["reason_codes"][0],
        "missing_requirement_index"
    );
    install_valid_artifacts(&repo_root);

    replace_in_file(
        &repo_root.join(SPEC_REL),
        "- [REQ-001][behavior] Execution-bound specs must include a parseable `Requirement Index`.",
        "- REQ-001 behavior] Execution-bound specs must include a parseable `Requirement Index`.",
    );
    let malformed_index = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze malformed requirement index",
        ),
        "rust analyze malformed requirement index",
    );
    assert_eq!(malformed_index["contract_state"], "invalid");
    assert_eq!(
        malformed_index["reason_codes"][0],
        "malformed_requirement_index"
    );
    install_valid_artifacts(&repo_root);

    replace_in_file(
        &repo_root.join(PLAN_REL),
        "**Spec Coverage:** REQ-001, REQ-002, DEC-001",
        "**Spec Coverage:** ",
    );
    let missing_task_coverage = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze missing task spec coverage",
        ),
        "rust analyze missing task spec coverage",
    );
    assert_eq!(missing_task_coverage["contract_state"], "invalid");
    assert_eq!(
        missing_task_coverage["reason_codes"][0],
        "task_missing_spec_coverage"
    );
}

#[test]
fn analyze_plan_cli_failpoints_surface_unexpected_contract_failures() {
    let repo_root = unique_temp_dir("contract-analyze-failpoints");
    let state_dir = unique_temp_dir("contract-analyze-failpoints-state");
    install_valid_artifacts(&repo_root);

    for failpoint in [
        "requirement_index_unexpected_failure",
        "coverage_matrix_unexpected_failure",
    ] {
        let mut command =
            Command::cargo_bin("featureforge").expect("featureforge cargo binary should exist");
        command
            .current_dir(&repo_root)
            .env("FEATUREFORGE_STATE_DIR", &state_dir)
            .env("FEATUREFORGE_PLAN_CONTRACT_TEST_FAILPOINT", failpoint)
            .args([
                "plan",
                "contract",
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ]);
        let output = run(command, failpoint);
        let payload = parse_success_json(&output, failpoint);
        assert_eq!(payload["contract_state"], "invalid");
        assert_eq!(
            payload["reason_codes"][0],
            "unexpected_plan_contract_failure"
        );
        assert_eq!(
            payload["diagnostics"][0]["code"],
            "unexpected_plan_contract_failure"
        );
    }
}

#[test]
fn build_task_packet_preserves_contract_text_and_regenerates_persisted_cache() {
    let repo_root = unique_temp_dir("contract-packet");
    let state_dir = unique_temp_dir("contract-packet-state");
    install_valid_artifacts(&repo_root);
    replace_in_file(
        &repo_root.join(PLAN_REL),
        "- Create: `bin/featureforge`",
        "- Create: `./bin/featureforge:12`",
    );

    let json_packet = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "build-task-packet",
                "--plan",
                PLAN_REL,
                "--task",
                "1",
                "--format",
                "json",
                "--persist",
                "yes",
            ],
            "rust build json task packet",
        ),
        "rust build json task packet",
    );
    assert_eq!(json_packet["status"], "ok");
    assert_eq!(json_packet["packet_contract_version"], "task-obligation-v2");
    assert_eq!(json_packet["task_number"], 1);
    assert_eq!(json_packet["task_title"], "Establish the plan contract");
    assert_eq!(json_packet["persisted"], true);
    assert_eq!(json_packet["cache_status"], "fresh");
    assert!(packet_has_requirement_statement(
        &json_packet,
        "Execution-bound specs must include a parseable `Requirement Index`."
    ));
    assert_eq!(
        json_packet["constraint_obligations"][0]["id"],
        "CONSTRAINT_1"
    );
    assert_eq!(json_packet["done_when_obligations"][0]["id"], "DONE_WHEN_1");
    assert_eq!(json_packet["file_entries"][0]["action"], "Create");
    assert_eq!(
        json_packet["file_entries"][0]["path"],
        "./bin/featureforge:12"
    );
    assert_eq!(
        json_packet["file_entries"][0]["normalized_path"],
        "bin/featureforge"
    );
    assert!(
        json_packet["file_scope"]
            .as_array()
            .expect("file_scope should be present")
            .iter()
            .any(|path| path.as_str() == Some("bin/featureforge"))
    );
    assert!(
        json_packet["packet_markdown"]
            .as_str()
            .is_some_and(|packet| packet
                .contains("Execution-bound specs must include a parseable `Requirement Index`")
                && packet.contains("## Task Contract")
                && packet.contains("- CONSTRAINT_1:")
                && packet.contains("- DONE_WHEN_1:")
                && packet.contains("### File Scope\n\n- Create: `bin/featureforge`")
                && packet.contains("### File Scope")
                && !packet.contains("## Original Task Block")
                && !packet.contains("**Constraints:**")
                && !packet.contains("**Done when:**"))
    );
    let packet_path = PathBuf::from(
        json_packet["packet_path"]
            .as_str()
            .expect("persisted task packet path should exist"),
    );
    let first_fingerprint = json_packet["packet_fingerprint"]
        .as_str()
        .expect("packet fingerprint should exist")
        .to_owned();
    let typed_spec = parse_spec_file(repo_root.join(SPEC_REL)).expect("typed spec should parse");
    let typed_plan = parse_plan_file(repo_root.join(PLAN_REL)).expect("typed plan should parse");
    let typed_packet =
        build_task_packet_with_timestamp(&typed_spec, &typed_plan, 1, "2026-03-23T15:00:00Z")
            .expect("typed task packet should build");
    assert_eq!(
        first_fingerprint, typed_packet.packet_fingerprint,
        "runtime/CLI and typed packet builders should share one canonical fingerprint preimage"
    );

    let markdown_packet = run_rust(
        &repo_root,
        &state_dir,
        &[
            "build-task-packet",
            "--plan",
            PLAN_REL,
            "--task",
            "2",
            "--format",
            "markdown",
            "--persist",
            "no",
        ],
        "rust build markdown task packet",
    );
    assert!(
        markdown_packet.status.success(),
        "markdown task packet should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        markdown_packet.status,
        String::from_utf8_lossy(&markdown_packet.stdout),
        String::from_utf8_lossy(&markdown_packet.stderr)
    );
    let markdown = String::from_utf8(markdown_packet.stdout)
        .expect("markdown task packet output should be utf8");
    assert!(markdown.contains("## Task Packet"));
    assert!(markdown.contains("**Task Title:** Dispatch exact packet-backed execution"));
    assert!(markdown.contains("## Task Contract"));
    assert!(markdown.contains("### Goal"));
    assert!(markdown.contains("- CONSTRAINT_1:"));
    assert!(markdown.contains("- DONE_WHEN_1:"));
    assert!(markdown.contains("### File Scope"));
    assert!(!markdown.contains("## Original Task Block"));
    assert!(!markdown.contains("**Constraints:**"));
    assert!(!markdown.contains("**Done when:**"));
    assert!(!markdown.contains("**Open Questions:**"));

    replace_in_file(
        &repo_root.join(PLAN_REL),
        "**Plan Revision:** 1",
        "**Plan Revision:** 2",
    );
    let regenerated = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "build-task-packet",
                "--plan",
                PLAN_REL,
                "--task",
                "1",
                "--format",
                "json",
                "--persist",
                "yes",
            ],
            "rust regenerate persisted task packet after revision change",
        ),
        "rust regenerate persisted task packet after revision change",
    );
    assert_eq!(regenerated["plan_revision"], 2);
    assert_eq!(regenerated["cache_status"], "regenerated");
    assert_eq!(
        regenerated["packet_path"].as_str(),
        Some(packet_path.to_string_lossy().as_ref())
    );
    assert_ne!(
        regenerated["packet_fingerprint"].as_str(),
        Some(first_fingerprint.as_str())
    );

    fs::write(&packet_path, "tampered\n").expect("packet cache should be writable");
    let tampered = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "build-task-packet",
                "--plan",
                PLAN_REL,
                "--task",
                "1",
                "--format",
                "json",
                "--persist",
                "yes",
            ],
            "rust regenerate tampered persisted task packet",
        ),
        "rust regenerate tampered persisted task packet",
    );
    assert_eq!(tampered["cache_status"], "regenerated");
    assert!(
        fs::read_to_string(&packet_path)
            .expect("packet cache should remain readable")
            .contains("**Task Title:** Establish the plan contract")
    );
}

#[test]
fn lint_invalid_contract_failures_and_packet_cache_retention_match_cli_contract() {
    let repo_root = unique_temp_dir("contract-invalid-fixtures");
    let state_dir = unique_temp_dir("contract-invalid-fixtures-state");
    let cases = [
        (
            "invalid-missing-index-spec.md",
            "valid-plan.md",
            "MissingRequirementIndex",
        ),
        (
            "valid-spec.md",
            "invalid-missing-coverage-plan.md",
            "MissingRequirementCoverage",
        ),
        (
            "valid-spec.md",
            "invalid-unknown-id-plan.md",
            "UnknownRequirementId",
        ),
        (
            "valid-spec.md",
            "invalid-ambiguous-wording-plan.md",
            "AmbiguousTaskWording",
        ),
        (
            "valid-spec.md",
            "invalid-requirement-weakening-plan.md",
            "RequirementWeakeningDetected",
        ),
        (
            "valid-spec.md",
            "transition-only/invalid-open-questions-plan.md",
            "LegacyTaskField",
        ),
        (
            "valid-spec.md",
            "invalid-malformed-files-plan.md",
            "MalformedFilesBlock",
        ),
        (
            "valid-spec.md",
            "invalid-malformed-task-structure-plan.md",
            "MalformedTaskStructure",
        ),
        (
            "valid-spec.md",
            "invalid-path-traversal-plan.md",
            "MalformedFilesBlock",
        ),
    ];

    for (spec_fixture, plan_fixture, expected_error_class) in cases {
        install_fixture(&repo_root, spec_fixture, SPEC_REL);
        install_fixture(&repo_root, plan_fixture, PLAN_REL);

        let failure = parse_failure_json(
            &run_rust(
                &repo_root,
                &state_dir,
                &["lint", "--spec", SPEC_REL, "--plan", PLAN_REL],
                expected_error_class,
            ),
            expected_error_class,
        );
        assert_eq!(failure["error_class"], expected_error_class);
    }

    install_valid_artifacts(&repo_root);
    let unknown_task = parse_failure_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "build-task-packet",
                "--plan",
                PLAN_REL,
                "--task",
                "99",
                "--format",
                "json",
                "--persist",
                "no",
            ],
            "build-task-packet unknown task",
        ),
        "build-task-packet unknown task",
    );
    assert_eq!(unknown_task["error_class"], "TaskNotFound");

    let packet = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "build-task-packet",
                "--plan",
                PLAN_REL,
                "--task",
                "1",
                "--format",
                "json",
                "--persist",
                "yes",
            ],
            "seed retained packet cache",
        ),
        "seed retained packet cache",
    );
    let packet_path = PathBuf::from(
        packet["packet_path"]
            .as_str()
            .expect("packet path should exist for retained cache"),
    );
    let packet_dir = packet_path
        .parent()
        .expect("packet directory should exist")
        .to_path_buf();
    fs::write(packet_dir.join("stale-one.packet.md"), "stale one\n").expect("stale one packet");
    fs::write(packet_dir.join("stale-two.packet.md"), "stale two\n").expect("stale two packet");
    fs::write(packet_dir.join("stale-three.packet.md"), "stale three\n")
        .expect("stale three packet");

    let mut retained = Command::cargo_bin("featureforge").expect("featureforge cargo binary");
    retained
        .current_dir(&repo_root)
        .env("FEATUREFORGE_STATE_DIR", &state_dir)
        .env("FEATUREFORGE_PLAN_PACKET_RETENTION", "2")
        .args([
            "plan",
            "contract",
            "build-task-packet",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--format",
            "json",
            "--persist",
            "yes",
        ]);
    let retained_output = parse_success_json(
        &run(retained, "retained packet cache"),
        "retained packet cache",
    );
    assert_eq!(retained_output["persisted"], true);

    let retained_packets = fs::read_dir(&packet_dir)
        .expect("packet directory should remain readable")
        .flatten()
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("md"))
        .filter(|entry| entry.file_name().to_string_lossy().contains(".packet"))
        .count();
    assert_eq!(retained_packets, 2);
    assert_eq!(retained_output["packet_path"], packet["packet_path"]);
}

#[test]
fn lint_cache_invalidates_after_plan_change() {
    let repo_root = unique_temp_dir("contract-lint-cache");
    let state_dir = unique_temp_dir("contract-lint-cache-state");
    install_valid_artifacts(&repo_root);

    let seeded = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &["lint", "--spec", SPEC_REL, "--plan", PLAN_REL],
            "seed lint cache",
        ),
        "seed lint cache",
    );
    assert_eq!(seeded["status"], "ok");

    replace_in_file(
        &repo_root.join(PLAN_REL),
        "- REQ-003 -> Task 2",
        "- REQ-003 -> Task 1",
    );

    let failure = parse_failure_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &["lint", "--spec", SPEC_REL, "--plan", PLAN_REL],
            "lint after cached plan change",
        ),
        "lint after cached plan change",
    );
    assert_eq!(failure["error_class"], "CoverageMatrixMismatch");
}

#[test]
fn analyze_plan_reports_missing_plan_fidelity_receipt_for_draft_plan() {
    let repo_root = unique_temp_dir("contract-analyze-missing-plan-fidelity-receipt");
    let state_dir = unique_temp_dir("contract-analyze-missing-plan-fidelity-receipt-state");
    install_valid_draft_artifacts(&repo_root);

    let report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze draft plan without plan-fidelity receipt",
        ),
        "rust analyze draft plan without plan-fidelity receipt",
    );

    assert_eq!(report["contract_state"], "invalid");
    assert_eq!(report["plan_fidelity_receipt"]["state"], "missing");
    assert!(
        report["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .filter_map(Value::as_str)
            .any(|code| code == "missing_plan_fidelity_receipt")
    );
}

#[test]
fn analyze_plan_accepts_matching_pass_plan_fidelity_receipt_for_draft_plan() {
    let repo_root = unique_temp_dir("contract-analyze-pass-plan-fidelity-receipt");
    let state_dir = unique_temp_dir("contract-analyze-pass-plan-fidelity-receipt-state");
    install_valid_draft_artifacts(&repo_root);
    write_plan_fidelity_review_artifact(
        &repo_root,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-pass.md",
            plan_path: PLAN_REL,
            plan_revision: 1,
            spec_path: SPEC_REL,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "independent-reviewer-1",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let record = parse_success_json(
        &run_record_plan_fidelity(
            &repo_root,
            &state_dir,
            &[
                "record",
                "--plan",
                PLAN_REL,
                "--review-artifact",
                ".featureforge/reviews/plan-fidelity-pass.md",
                "--json",
            ],
            "record matching pass plan-fidelity receipt",
        ),
        "record matching pass plan-fidelity receipt",
    );
    assert_eq!(record["status"], "ok");

    let report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze draft plan with pass plan-fidelity receipt",
        ),
        "rust analyze draft plan with pass plan-fidelity receipt",
    );

    assert_eq!(report["contract_state"], "valid");
    assert_eq!(report["plan_fidelity_receipt"]["state"], "pass");
    assert_eq!(
        report["plan_fidelity_receipt"]["reviewer_stage"],
        "featureforge:plan-fidelity-review"
    );
    assert_eq!(
        report["plan_fidelity_receipt"]["provenance_source"],
        "fresh-context-subagent"
    );
    assert_eq!(
        report["plan_fidelity_receipt"]["verified_requirement_index"],
        true
    );
    assert_eq!(
        report["plan_fidelity_receipt"]["verified_execution_topology"],
        true
    );
    assert_eq!(
        report["plan_fidelity_receipt"]["verified_task_contract"],
        true
    );
    assert_eq!(
        report["plan_fidelity_receipt"]["verified_task_determinism"],
        true
    );
    assert_eq!(
        report["plan_fidelity_receipt"]["verified_spec_reference_fidelity"],
        true
    );
}

#[test]
fn analyze_plan_rejects_stale_or_non_independent_plan_fidelity_receipts() {
    let repo_root = unique_temp_dir("contract-analyze-stale-plan-fidelity-receipt");
    let state_dir = unique_temp_dir("contract-analyze-stale-plan-fidelity-receipt-state");
    install_valid_draft_artifacts(&repo_root);
    let slug_identity = discover_slug_identity(&repo_root);
    let runtime_receipt_path = plan_fidelity_receipt_path(
        &state_dir,
        &slug_identity.repo_slug,
        &slug_identity.branch_name,
    );
    let mut non_independent_receipt = build_matching_plan_fidelity_receipt(&repo_root);
    non_independent_receipt.reviewer_provenance.reviewer_source = String::from("same-context");
    non_independent_receipt.reviewer_provenance.reviewer_id = String::from("writer-context");
    write_plan_fidelity_receipt(&runtime_receipt_path, &non_independent_receipt);

    let non_independent = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze draft plan with non-independent plan-fidelity receipt",
        ),
        "rust analyze draft plan with non-independent plan-fidelity receipt",
    );
    assert_eq!(non_independent["contract_state"], "invalid");
    assert_eq!(non_independent["plan_fidelity_receipt"]["state"], "invalid");
    assert!(
        non_independent["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .filter_map(Value::as_str)
            .any(|code| code == "plan_fidelity_receipt_not_independent")
    );
    fs::remove_file(&runtime_receipt_path).expect("invalid runtime receipt should be removable");
    write_plan_fidelity_review_artifact(
        &repo_root,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-stale.md",
            plan_path: PLAN_REL,
            plan_revision: 1,
            spec_path: SPEC_REL,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "independent-reviewer-2",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    parse_success_json(
        &run_record_plan_fidelity(
            &repo_root,
            &state_dir,
            &[
                "record",
                "--plan",
                PLAN_REL,
                "--review-artifact",
                ".featureforge/reviews/plan-fidelity-stale.md",
                "--json",
            ],
            "record pass plan-fidelity receipt before plan revision change",
        ),
        "record pass plan-fidelity receipt before plan revision change",
    );

    replace_in_file(
        &repo_root.join(PLAN_REL),
        "**Plan Revision:** 1",
        "**Plan Revision:** 2",
    );

    let stale = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze draft plan with stale plan-fidelity receipt",
        ),
        "rust analyze draft plan with stale plan-fidelity receipt",
    );
    assert_eq!(stale["contract_state"], "invalid");
    assert_eq!(stale["plan_fidelity_receipt"]["state"], "stale");
    assert!(
        stale["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .filter_map(Value::as_str)
            .any(|code| code == "stale_plan_fidelity_receipt")
    );
}

#[test]
fn analyze_plan_requires_expanded_plan_fidelity_checks_in_receipt() {
    let repo_root = unique_temp_dir("contract-analyze-plan-fidelity-receipt-checks");
    let state_dir = unique_temp_dir("contract-analyze-plan-fidelity-receipt-checks-state");
    install_valid_draft_artifacts(&repo_root);
    let slug_identity = discover_slug_identity(&repo_root);
    let receipt_path = plan_fidelity_receipt_path(
        &state_dir,
        &slug_identity.repo_slug,
        &slug_identity.branch_name,
    );
    let mut receipt = build_matching_plan_fidelity_receipt(&repo_root);
    receipt.verification.checked_surfaces = Vec::new();
    receipt.verification.verified_requirement_ids = Vec::new();
    write_plan_fidelity_receipt(&receipt_path, &receipt);

    let report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze draft plan with incomplete plan-fidelity receipt",
        ),
        "rust analyze draft plan with incomplete plan-fidelity receipt",
    );

    assert_eq!(report["contract_state"], "invalid");
    assert_eq!(report["plan_fidelity_receipt"]["state"], "invalid");
    let reason_codes = report["reason_codes"]
        .as_array()
        .expect("reason_codes should be present")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert!(
        reason_codes.contains(&"plan_fidelity_receipt_missing_requirement_index_check"),
        "missing requirement-index verification should fail closed, got {reason_codes:?}"
    );
    assert!(
        reason_codes.contains(&"plan_fidelity_receipt_missing_execution_topology_check"),
        "missing execution-topology verification should fail closed, got {reason_codes:?}"
    );
    assert!(
        reason_codes.contains(&"plan_fidelity_receipt_missing_task_contract_check"),
        "missing task-contract verification should fail closed, got {reason_codes:?}"
    );
    assert!(
        reason_codes.contains(&"plan_fidelity_receipt_missing_task_determinism_check"),
        "missing task-determinism verification should fail closed, got {reason_codes:?}"
    );
    assert!(
        reason_codes.contains(&"plan_fidelity_receipt_missing_spec_reference_fidelity_check"),
        "missing spec-reference-fidelity verification should fail closed, got {reason_codes:?}"
    );
}

#[test]
fn analyze_plan_reports_invalid_fidelity_gate_when_spec_requirement_index_is_malformed() {
    let repo_root = unique_temp_dir("contract-analyze-plan-fidelity-malformed-spec");
    let state_dir = unique_temp_dir("contract-analyze-plan-fidelity-malformed-spec-state");
    install_valid_draft_artifacts(&repo_root);
    fs::write(
        repo_root.join(SPEC_REL),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Summary\n\nMalformed fixture without a Requirement Index.\n",
    )
    .expect("malformed spec fixture should write");

    let report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "rust analyze draft plan with malformed source spec",
        ),
        "rust analyze draft plan with malformed source spec",
    );

    assert_eq!(report["contract_state"], "invalid");
    assert_eq!(report["plan_fidelity_receipt"]["state"], "invalid");
    assert_ne!(
        report["plan_fidelity_receipt"]["state"], "not_applicable",
        "draft-plan analysis should still report the plan-fidelity gate when the source spec is malformed"
    );
}

#[test]
fn analyze_plan_rejects_pass_receipt_when_source_spec_is_not_workflow_valid_ceo_review() {
    let repo_root = unique_temp_dir("contract-analyze-plan-fidelity-invalid-spec-reviewer");
    install_valid_draft_artifacts(&repo_root);
    replace_in_file(
        &repo_root.join(SPEC_REL),
        "**Last Reviewed By:** plan-ceo-review",
        "**Last Reviewed By:** brainstorming",
    );
    let receipt_path = plan_fidelity_receipt_path_for_repo(&repo_root);
    write_plan_fidelity_receipt(
        &receipt_path,
        &build_matching_plan_fidelity_receipt(&repo_root),
    );

    let report = analyze_plan(repo_root.join(SPEC_REL), repo_root.join(PLAN_REL))
        .expect("analyze_plan should still return a report for draft plans with invalid spec reviewer provenance");

    assert_eq!(report.contract_state, "invalid");
    assert_eq!(report.plan_fidelity_receipt.state, "invalid");
    assert!(
        report
            .reason_codes
            .iter()
            .any(|code| code == "plan_fidelity_source_spec_not_ceo_approved")
    );
}

#[test]
fn analyze_plan_rejects_out_of_repo_source_spec_paths() {
    let repo_root = unique_temp_dir("contract-analyze-external-source-spec");
    let state_dir = unique_temp_dir("contract-analyze-external-source-spec-state");
    install_valid_draft_artifacts(&repo_root);
    replace_in_file(
        &repo_root.join(PLAN_REL),
        &format!("**Source Spec:** `{SPEC_REL}`"),
        "**Source Spec:** `../external/docs/featureforge/specs/outside-spec.md`",
    );

    let report = parse_success_json(
        &run_rust(
            &repo_root,
            &state_dir,
            &[
                "analyze-plan",
                "--spec",
                SPEC_REL,
                "--plan",
                PLAN_REL,
                "--format",
                "json",
            ],
            "analyze-plan should fail closed on out-of-repo Source Spec paths",
        ),
        "analyze-plan should fail closed on out-of-repo Source Spec paths",
    );

    assert_eq!(report["contract_state"], "invalid");
    assert!(
        report["reason_codes"]
            .as_array()
            .expect("reason_codes should be present")
            .iter()
            .filter_map(Value::as_str)
            .any(|code| code == "missing_source_spec"),
        "out-of-repo Source Spec paths should fail the plan header contract"
    );
}

#[test]
fn plan_contract_schemas_exist_with_expected_titles() {
    let analyze_schema_path = repo_fixture_path("schemas/plan-contract-analyze.schema.json");
    let packet_schema_path = repo_fixture_path("schemas/plan-contract-packet.schema.json");
    let generated_schema_dir = unique_temp_dir("generated-contract-schemas");
    write_contract_schemas(&generated_schema_dir).expect("generated contract schemas should write");

    let analyze_schema: Value = serde_json::from_str(
        &fs::read_to_string(&analyze_schema_path).expect("analyze schema should exist"),
    )
    .expect("analyze schema should be valid json");
    let packet_schema: Value = serde_json::from_str(
        &fs::read_to_string(&packet_schema_path).expect("packet schema should exist"),
    )
    .expect("packet schema should be valid json");
    let generated_analyze_schema =
        fs::read_to_string(generated_schema_dir.join("plan-contract-analyze.schema.json"))
            .expect("generated analyze schema should exist");
    let generated_packet_schema =
        fs::read_to_string(generated_schema_dir.join("plan-contract-packet.schema.json"))
            .expect("generated packet schema should exist");

    assert_eq!(analyze_schema["title"], "AnalyzePlanReport");
    assert_eq!(packet_schema["title"], "TaskPacket");
    assert!(
        packet_schema["required"]
            .as_array()
            .expect("packet schema required fields should be present")
            .iter()
            .any(|field| field.as_str() == Some("file_entries"))
    );
    assert!(
        packet_schema["required"]
            .as_array()
            .expect("packet schema required fields should be present")
            .iter()
            .any(|field| field.as_str() == Some("file_scope"))
    );
    assert_eq!(
        fs::read_to_string(&analyze_schema_path)
            .expect("checked-in analyze schema should be readable")
            .trim_end(),
        generated_analyze_schema.trim_end()
    );
    assert_eq!(
        fs::read_to_string(&packet_schema_path)
            .expect("checked-in packet schema should be readable")
            .trim_end(),
        generated_packet_schema.trim_end()
    );
}
