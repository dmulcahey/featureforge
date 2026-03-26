#[path = "support/bin.rs"]
mod bin_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/json.rs"]
mod json_support;
#[path = "support/process.rs"]
mod process_support;

use files_support::write_file;
use json_support::parse_json;
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-25-execution-harness-state.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-03-25-execution-harness-state-design.md";

const EXPECTED_PUBLIC_HARNESS_PHASES: &[&str] = &[
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
];

const EXPECTED_REASON_CODES: &[&str] = &[
    "waiting_on_required_evaluator",
    "required_evaluator_failed",
    "required_evaluator_blocked",
    "handoff_required",
    "repair_within_budget",
    "pivot_threshold_exceeded",
    "blocked_on_plan_revision",
    "write_authority_conflict",
    "repo_state_drift",
    "stale_provenance",
    "recovering_incomplete_authoritative_mutation",
    "missing_required_evidence",
    "invalid_evidence_satisfaction_rule",
];

fn run(mut command: Command, context: &str) -> Output {
    command
        .output()
        .unwrap_or_else(|error| panic!("{context} should run: {error}"))
}

fn run_checked_output(command: Command, context: &str) -> Output {
    let output = run(command, context);
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn init_repo(name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();

    let mut git_init = Command::new("git");
    git_init.arg("init").current_dir(repo);
    run_checked_output(git_init, "git init");

    let mut git_config_name = Command::new("git");
    git_config_name
        .args(["config", "user.name", "FeatureForge Test"])
        .current_dir(repo);
    run_checked_output(git_config_name, "git config user.name");

    let mut git_config_email = Command::new("git");
    git_config_email
        .args(["config", "user.email", "featureforge-tests@example.com"])
        .current_dir(repo);
    run_checked_output(git_config_email, "git config user.email");

    write_file(&repo.join("README.md"), &format!("# {name}\n"));

    let mut git_add = Command::new("git");
    git_add.args(["add", "README.md"]).current_dir(repo);
    run_checked_output(git_add, "git add README");

    let mut git_commit = Command::new("git");
    git_commit.args(["commit", "-m", "init"]).current_dir(repo);
    run_checked_output(git_commit, "git commit init");

    (repo_dir, state_dir)
}

fn write_approved_spec(repo: &Path) {
    write_file(
        &repo.join(SPEC_REL),
        r#"# Execution Harness State Design

**Workflow State:** CEO Approved
**Spec Revision:** 2
**Last Reviewed By:** plan-ceo-review

## Summary

Fixture spec for harness state regression coverage.
"#,
    );
}

fn write_plan(repo: &Path, execution_mode: &str) {
    write_file(
        &repo.join(PLAN_REL),
        &format!(
            r#"# Execution Harness State Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** {execution_mode}
**Source Spec:** `{SPEC_REL}`
**Source Spec Revision:** 2
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1

## Task 1: Harness state fixture

**Spec Coverage:** REQ-001
**Task Outcome:** Harness state and storage fields are visible before execution starts.
**Plan Constraints:**
- Keep the fixture focused on status and state surfaces.
**Open Questions:** none

**Files:**
- Test: `tests/execution_harness_state.rs`

- [ ] **Step 1: Verify the harness state surface**
"#,
        ),
    );
}

fn run_plan_execution_json(repo: &Path, state: &Path, args: &[&str], context: &str) -> Value {
    let mut command = Command::new(bin_support::compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state)
        .args(["plan", "execution"])
        .args(args);
    parse_json(&run(command, context), context)
}

fn missing_string_fields(object: &Value, fields: &[&str]) -> Vec<String> {
    fields
        .iter()
        .copied()
        .filter(|field| object.get(*field).and_then(Value::as_str).is_none())
        .map(str::to_owned)
        .collect()
}

fn missing_or_empty_string_fields(object: &Value, fields: &[&str]) -> Vec<String> {
    fields
        .iter()
        .copied()
        .filter(|field| {
            !match object.get(*field) {
                Some(Value::String(value)) => !value.is_empty(),
                _ => false,
            }
        })
        .map(str::to_owned)
        .collect()
}

fn missing_or_invalid_nullable_string_fields(object: &Value, fields: &[&str]) -> Vec<String> {
    fields
        .iter()
        .copied()
        .filter(|field| {
            !match object.get(*field) {
                Some(Value::Null) => true,
                Some(Value::String(value)) => !value.is_empty(),
                _ => false,
            }
        })
        .map(str::to_owned)
        .collect()
}

fn missing_array_fields(object: &Value, fields: &[&str]) -> Vec<String> {
    fields
        .iter()
        .copied()
        .filter(|field| object.get(*field).and_then(Value::as_array).is_none())
        .map(str::to_owned)
        .collect()
}

fn missing_number_fields(object: &Value, fields: &[&str]) -> Vec<String> {
    fields
        .iter()
        .copied()
        .filter(|field| object.get(*field).and_then(Value::as_u64).is_none())
        .map(str::to_owned)
        .collect()
}

fn missing_bool_fields(object: &Value, fields: &[&str]) -> Vec<String> {
    fields
        .iter()
        .copied()
        .filter(|field| object.get(*field).and_then(Value::as_bool).is_none())
        .map(str::to_owned)
        .collect()
}

fn missing_null_fields(object: &Value, fields: &[&str]) -> Vec<String> {
    fields
        .iter()
        .copied()
        .filter(|field| !object.get(*field).is_some_and(Value::is_null))
        .map(str::to_owned)
        .collect()
}

fn assert_exact_public_harness_phase_set() {
    let spec = include_str!(
        "../docs/featureforge/specs/2026-03-25-featureforge-execution-harness-spec.md"
    );
    let public_harness_phases: Vec<String> = spec
        .lines()
        .scan(false, |in_phase_section, line| {
            let trimmed = line.trim();
            if trimmed == "### Public phase model" {
                *in_phase_section = true;
                return Some(None);
            }
            if *in_phase_section && trimmed.starts_with("### ") {
                *in_phase_section = false;
                return Some(None);
            }
            if *in_phase_section {
                return Some(
                    trimmed
                        .strip_prefix("- `")
                        .and_then(|value| value.strip_suffix('`'))
                        .map(str::to_owned),
                );
            }
            Some(None)
        })
        .flatten()
        .collect();

    assert_eq!(
        public_harness_phases,
        EXPECTED_PUBLIC_HARNESS_PHASES
            .iter()
            .map(|phase| (*phase).to_owned())
            .collect::<Vec<_>>(),
        "status should pin the exact public HarnessPhase vocabulary from the spec"
    );
}

fn assert_reason_code_minimum_vocabulary() {
    let spec = include_str!(
        "../docs/featureforge/specs/2026-03-25-featureforge-execution-harness-spec.md"
    );
    let req_040_line = spec
        .lines()
        .find(|line| line.contains("[REQ-040][behavior]"))
        .expect("spec should include REQ-040 minimum reason-code contract");
    let minimum_reason_code_clause = req_040_line
        .split("covering at least")
        .nth(1)
        .expect("REQ-040 should document the minimum reason-code set after 'covering at least'");
    let spec_minimum_reason_codes: Vec<_> = minimum_reason_code_clause
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();
    let missing_in_spec: Vec<_> = EXPECTED_REASON_CODES
        .iter()
        .filter(|code| !spec_minimum_reason_codes.iter().any(|value| value == *code))
        .copied()
        .collect();
    let missing_in_expected: Vec<_> = spec_minimum_reason_codes
        .iter()
        .filter(|code| !EXPECTED_REASON_CODES.iter().any(|value| value == code))
        .cloned()
        .collect();
    assert!(
        missing_in_spec.is_empty() && missing_in_expected.is_empty(),
        "REQ-040 minimum reason-code vocabulary drifted between local expected set and spec; missing_in_spec: {missing_in_spec:?}; missing_in_expected: {missing_in_expected:?}"
    );
}

#[test]
fn status_exposes_run_identity_policy_snapshot_and_authority_diagnostics_before_execution_starts() {
    let (repo_dir, state_dir) = init_repo("execution-harness-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "none");

    assert_exact_public_harness_phase_set();
    assert_reason_code_minimum_vocabulary();

    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "plan execution status for harness state",
    );

    let harness_phase = status["harness_phase"]
        .as_str()
        .expect("status should expose harness_phase");
    assert_eq!(
        harness_phase, "implementation_handoff",
        "status should expose the exact pre-execution harness phase"
    );

    let missing_or_invalid_nullable_string_fields = missing_or_invalid_nullable_string_fields(
        &status,
        &[
            "last_final_review_artifact_fingerprint",
            "last_browser_qa_artifact_fingerprint",
            "last_release_docs_artifact_fingerprint",
            "write_authority_holder",
            "write_authority_worktree",
            "repo_state_baseline_head_sha",
            "repo_state_baseline_worktree_fingerprint",
        ],
    );
    assert!(
        missing_or_invalid_nullable_string_fields.is_empty(),
        "status should expose nullable preflight diagnostics as null or non-empty strings, missing/invalid: {missing_or_invalid_nullable_string_fields:?}"
    );

    let missing_non_empty_string_fields =
        missing_or_empty_string_fields(&status, &["chunk_id", "write_authority_state"]);
    assert!(
        missing_non_empty_string_fields.is_empty(),
        "status should expose chunk_id and write_authority_state as non-empty run-scoped strings, missing/invalid: {missing_non_empty_string_fields:?}"
    );

    let missing_string_fields = missing_string_fields(
        &status,
        &[
            "aggregate_evaluation_state",
            "repo_state_drift_state",
            "dependency_index_state",
            "final_review_state",
            "browser_qa_state",
            "release_docs_state",
        ],
    );
    assert!(
        missing_string_fields.is_empty(),
        "status should expose the run-scoped string fields, missing: {missing_string_fields:?}"
    );

    let missing_active_pointer_null_fields = missing_null_fields(
        &status,
        &[
            "active_contract_path",
            "active_contract_fingerprint",
            "last_evaluation_report_path",
            "last_evaluation_report_fingerprint",
            "last_evaluation_evaluator_kind",
        ],
    );
    assert!(
        missing_active_pointer_null_fields.is_empty(),
        "status should keep active pointers authoritative-only before execution starts, missing null fields: {missing_active_pointer_null_fields:?}"
    );

    let missing_array_fields = missing_array_fields(
        &status,
        &[
            "required_evaluator_kinds",
            "completed_evaluator_kinds",
            "pending_evaluator_kinds",
            "non_passing_evaluator_kinds",
            "open_failed_criteria",
            "reason_codes",
        ],
    );
    assert!(
        missing_array_fields.is_empty(),
        "status should expose the run-scoped arrays, missing: {missing_array_fields:?}"
    );

    let missing_prestart_null_fields = missing_null_fields(
        &status,
        &[
            "execution_run_id",
            "chunking_strategy",
            "evaluator_policy",
            "reset_policy",
            "last_evaluation_verdict",
            "review_stack",
        ],
    );
    assert!(
        missing_prestart_null_fields.is_empty(),
        "status should keep pre-start authority fields unset before preflight/evaluation, missing null fields: {missing_prestart_null_fields:?}"
    );

    let missing_number_fields = missing_number_fields(
        &status,
        &[
            "latest_authoritative_sequence",
            "current_chunk_retry_count",
            "current_chunk_retry_budget",
            "current_chunk_pivot_threshold",
        ],
    );
    assert!(
        missing_number_fields.is_empty(),
        "status should expose the run-scoped counters, missing: {missing_number_fields:?}"
    );

    let missing_bool_fields = missing_bool_fields(&status, &["handoff_required"]);
    assert!(
        missing_bool_fields.is_empty(),
        "status should expose the run-scoped booleans, missing: {missing_bool_fields:?}"
    );

    let reason_codes = status["reason_codes"]
        .as_array()
        .expect("status should expose stable reason_codes");
    let non_string_reason_codes: Vec<_> = reason_codes
        .iter()
        .filter(|value| value.as_str().is_none())
        .map(ToString::to_string)
        .collect();
    assert!(
        non_string_reason_codes.is_empty(),
        "status should expose machine-readable reason_codes entries, got non-strings: {non_string_reason_codes:?}"
    );
}
