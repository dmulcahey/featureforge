#[path = "support/bin.rs"]
mod bin_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/json.rs"]
mod json_support;
#[allow(dead_code)]
#[path = "support/plan_execution_direct.rs"]
mod plan_execution_direct_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/repo_template.rs"]
mod repo_template_support;

use bin_support::compiled_featureforge_path;
use featureforge::contracts::plan::parse_plan_file;
use featureforge::execution::final_review::{
    FinalReviewReceipt, FinalReviewReceiptExpectations, FinalReviewReceiptIssue,
    latest_branch_artifact_path, parse_final_review_receipt, resolve_release_base_branch,
    validate_final_review_receipt,
};
use featureforge::execution::semantic_identity::{
    branch_definition_identity_for_context, task_definition_identity_for_task,
};
use featureforge::execution::state::current_head_sha as runtime_current_head_sha;
use featureforge::execution::state::hash_contract_plan;
use featureforge::execution::state::{ExecutionRuntime, load_execution_context};
use featureforge::git::{discover_repository, discover_slug_identity};
use featureforge::paths::{
    branch_storage_key, harness_authoritative_artifact_path, harness_authoritative_artifacts_dir,
    harness_state_path,
};
use files_support::write_file;
use json_support::parse_json;
use process_support::{run, run_checked};
use repo_template_support::populate_repo_from_template;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-17-example-execution-plan.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-03-17-example-execution-plan-design.md";
const STRATEGY_CHECKPOINT_FINGERPRINT: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn validate_fixture_review_receipt(
    receipt: &FinalReviewReceipt,
    review_path: &Path,
    repo: &Path,
    expected_strategy_checkpoint_fingerprint: Option<&str>,
) -> Result<(), FinalReviewReceiptIssue> {
    let expectations = FinalReviewReceiptExpectations {
        expected_plan_path: PLAN_REL,
        expected_plan_revision: 1,
        expected_strategy_checkpoint_fingerprint,
        expected_branch: &branch_name(repo),
        expected_repo: &repo_slug(repo),
        expected_head_sha: &current_head_sha(repo),
        expected_base_branch: &expected_base_branch(repo),
        expected_result: "pass",
        deviations_required: false,
    };
    validate_final_review_receipt(receipt, review_path, &expectations)
}

fn init_repo(name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();

    populate_repo_from_template(repo);
    write_file(&repo.join("README.md"), &format!("# {name}\n"));
    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["add", "README.md"]).current_dir(repo);
            command
        },
        "git add README",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["commit", "-m", "rename fixture"])
                .current_dir(repo);
            command
        },
        "git commit rename fixture",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["checkout", "-B", "fixture-work"])
                .current_dir(repo);
            command
        },
        "git checkout fixture-work",
    );

    (repo_dir, state_dir)
}

fn write_approved_spec(repo: &Path) {
    write_file(
        &repo.join(SPEC_REL),
        r#"# Example Execution Plan Design

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

## Requirement Index

- [REQ-001][behavior] Execution fixtures must support a valid single-task plan path for routing and finish-gate coverage.

## Summary

Fixture spec for focused execution-helper regression coverage.
"#,
    );
}

fn write_single_step_plan(repo: &Path, execution_mode: &str) {
    write_file(
        &repo.join(PLAN_REL),
        &format!(
            r#"# Example Execution Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** {execution_mode}
**Source Spec:** `{SPEC_REL}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review
**QA Requirement:** not-required

## Requirement Coverage Matrix

- REQ-001 -> Task 1

## Execution Strategy

- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for downstream review routing.

## Dependency Diagram

```text
Task 1
```

## Task 1: Single Step Task

**Spec Coverage:** REQ-001
**Goal:** The workspace is prepared for execution.

**Context:**
- Spec Coverage: REQ-001.

**Constraints:**
- Keep the fixture single-step and deterministic.

**Done when:**
- The workspace is prepared for execution.

**Files:**
- Modify: `docs/example-output.md`

- [ ] **Step 1: Prepare the workspace for execution**
"#
        ),
    );
}

fn mark_all_plan_steps_checked(repo: &Path) {
    let path = repo.join(PLAN_REL);
    let source = fs::read_to_string(&path).expect("plan should be readable");
    fs::write(path, source.replace("- [ ]", "- [x]")).expect("plan should be writable");
}

fn sha256_hex(contents: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents);
    format!("{:x}", hasher.finalize())
}

fn current_head_sha(repo: &Path) -> String {
    runtime_current_head_sha(repo).expect("head sha should resolve")
}

fn current_head_tree_sha(repo: &Path) -> String {
    discover_repository(repo)
        .expect("head tree helper should discover repository")
        .head_tree_id_or_empty()
        .expect("head tree helper should resolve HEAD tree")
        .detach()
        .to_string()
}

fn git_dir_path(repo: &Path) -> PathBuf {
    discover_repository(repo)
        .expect("git dir helper should discover repository")
        .path()
        .to_path_buf()
}

fn branch_name(repo: &Path) -> String {
    discover_slug_identity(repo).branch_name
}

fn expected_base_branch(repo: &Path) -> String {
    let current = branch_name(repo);
    resolve_release_base_branch(&repo.join(".git"), &current).unwrap_or(current)
}

fn repo_slug(repo: &Path) -> String {
    discover_slug_identity(repo).repo_slug
}

fn branch_contract_identity(repo: &Path, state_dir: &Path, plan_rel: &str) -> String {
    let branch = branch_name(repo);
    let runtime = ExecutionRuntime {
        repo_root: repo.to_path_buf(),
        git_dir: git_dir_path(repo),
        branch_name: branch.clone(),
        repo_slug: repo_slug(repo),
        safe_branch: branch_storage_key(&branch),
        state_dir: state_dir.to_path_buf(),
    };
    let context = load_execution_context(&runtime, Path::new(plan_rel))
        .expect("plan_execution_final_review semantic branch identity fixture should load execution context");
    branch_definition_identity_for_context(&context)
}

fn task_contract_identity(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    task_number: u32,
) -> String {
    let branch = branch_name(repo);
    let runtime = ExecutionRuntime {
        repo_root: repo.to_path_buf(),
        git_dir: git_dir_path(repo),
        branch_name: branch.clone(),
        repo_slug: repo_slug(repo),
        safe_branch: branch_storage_key(&branch),
        state_dir: state_dir.to_path_buf(),
    };
    let context = load_execution_context(&runtime, Path::new(plan_rel)).expect(
        "plan_execution_final_review semantic task identity fixture should load execution context",
    );
    task_definition_identity_for_task(&context, task_number)
        .expect("plan_execution_final_review semantic task identity fixture should compute")
        .expect("plan_execution_final_review semantic task identity fixture should exist")
}

fn project_artifact_dir(repo: &Path, state: &Path) -> PathBuf {
    state.join("projects").join(repo_slug(repo))
}

fn execution_contract_plan_hash(repo: &Path) -> String {
    let source = fs::read_to_string(repo.join(PLAN_REL)).expect("plan should be readable");
    hash_contract_plan(&source)
}

fn evidence_plan_hash(repo: &Path) -> String {
    execution_contract_plan_hash(repo)
}

fn expected_packet_fingerprint(repo: &Path, task: u32, step: u32) -> String {
    let plan_document =
        parse_plan_file(repo.join(PLAN_REL)).expect("plan should parse for packet fingerprint");
    let task_definition_identity = plan_document
        .tasks
        .iter()
        .find(|candidate| candidate.number == task)
        .map(serde_json::to_string)
        .transpose()
        .expect("task should serialize for packet fingerprint")
        .map(|serialized| format!("task_def:{}", sha256_hex(serialized.as_bytes())))
        .expect("task should exist for packet fingerprint");
    let spec_fingerprint =
        sha256_hex(&fs::read(repo.join(SPEC_REL)).expect("spec should be readable"));
    let payload = format!(
        "plan_path={PLAN_REL}\nplan_revision=1\ntask_definition_identity={task_definition_identity}\nsource_spec_path={SPEC_REL}\nsource_spec_revision=1\nsource_spec_fingerprint={spec_fingerprint}\ntask_number={task}\nstep_number={step}\n"
    );
    sha256_hex(payload.as_bytes())
}

fn write_single_step_v2_completed_attempt(repo: &Path, packet_fingerprint: &str) {
    let evidence_path = repo.join(
        "docs/featureforge/execution-evidence/2026-03-17-example-execution-plan-r1-evidence.md",
    );
    let plan_fingerprint = evidence_plan_hash(repo);
    let spec_fingerprint =
        sha256_hex(&fs::read(repo.join(SPEC_REL)).expect("spec should be readable"));
    write_file(&repo.join("docs/example-output.md"), "verified output\n");
    let file_digest = sha256_hex(
        &fs::read(repo.join("docs/example-output.md")).expect("output should be readable"),
    );
    let head_sha = current_head_sha(repo);
    write_file(
        &evidence_path,
        &format!(
            "# Execution Evidence: 2026-03-17-example-execution-plan\n\n**Plan Path:** {PLAN_REL}\n**Plan Revision:** 1\n**Plan Fingerprint:** {plan_fingerprint}\n**Source Spec Path:** {SPEC_REL}\n**Source Spec Revision:** 1\n**Source Spec Fingerprint:** {spec_fingerprint}\n\n## Step Evidence\n\n### Task 1 Step 1\n#### Attempt 1\n**Status:** Completed\n**Recorded At:** 2026-03-17T14:22:31Z\n**Execution Source:** featureforge:executing-plans\n**Task Number:** 1\n**Step Number:** 1\n**Packet Fingerprint:** {packet_fingerprint}\n**Head SHA:** {head_sha}\n**Base SHA:** {head_sha}\n**Claim:** Prepared the workspace for execution.\n**Files Proven:**\n- docs/example-output.md | sha256:{file_digest}\n**Verification Summary:** Manual inspection only: Verified by fixture setup.\n**Invalidation Reason:** N/A\n"
        ),
    );
}

fn write_test_plan_artifact(repo: &Path, state: &Path, browser_required: &str) -> PathBuf {
    let branch = branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let head_sha = current_head_sha(repo);
    let artifact_path = project_artifact_dir(repo, state)
        .join(format!("tester-{safe_branch}-test-plan-20260322-170500.md"));
    let source = format!(
        "# Test Plan\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {head_sha}\n**Browser QA Required:** {browser_required}\n**Generated By:** featureforge:plan-eng-review\n**Generated At:** 2026-03-22T17:05:00Z\n",
        repo_slug(repo)
    );
    write_file(&artifact_path, &source);
    let authoritative_path = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("test-plan-{}.md", sha256_hex(source.as_bytes())),
    );
    write_file(&authoritative_path, &source);
    artifact_path
}

fn write_code_review_artifact(repo: &Path, state: &Path, base_branch: &str) -> PathBuf {
    let branch = branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let head_sha = current_head_sha(repo);
    let reviewer_artifact_path = project_artifact_dir(repo, state).join(format!(
        "tester-{safe_branch}-independent-review-20260322-170950.md"
    ));
    let reviewer_artifact_source = format!(
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Strategy Checkpoint Fingerprint:** {STRATEGY_CHECKPOINT_FINGERPRINT}\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-22T17:09:50Z\n\n## Summary\n- dedicated independent reviewer artifact fixture.\n",
        repo_slug(repo)
    );
    write_file(&reviewer_artifact_path, &reviewer_artifact_source);
    let reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&reviewer_artifact_path).expect("reviewer artifact should read"));
    let artifact_path = project_artifact_dir(repo, state).join(format!(
        "tester-{safe_branch}-code-review-20260322-171100.md"
    ));
    write_file(
        &artifact_path,
        &format!(
            "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** {reviewer_artifact_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Strategy Checkpoint Fingerprint:** {STRATEGY_CHECKPOINT_FINGERPRINT}\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-22T17:11:00Z\n",
            reviewer_artifact_path.display(),
            repo_slug(repo)
        ),
    );
    artifact_path
}

fn reviewer_artifact_path_from_review(review_path: &Path) -> PathBuf {
    let receipt = parse_final_review_receipt(review_path);
    let reviewer_artifact_path = receipt
        .reviewer_artifact_path
        .as_deref()
        .expect("review receipt should include reviewer artifact path");
    let reviewer_artifact_path = PathBuf::from(reviewer_artifact_path.trim_matches('`').trim());
    if reviewer_artifact_path.is_absolute() {
        reviewer_artifact_path
    } else {
        review_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(reviewer_artifact_path)
    }
}

fn write_active_contract_artifact(repo: &Path, state: &Path) -> String {
    let plan_fingerprint = execution_contract_plan_hash(repo);
    let spec_fingerprint =
        sha256_hex(&fs::read(repo.join(SPEC_REL)).expect("spec should be readable"));
    let packet_fingerprint = expected_packet_fingerprint(repo, 1, 1);
    let template = format!(
        r#"# Execution Contract

**Contract Version:** 1
**Authoritative Sequence:** 17
**Source Plan Path:** `{PLAN_REL}`
**Source Plan Revision:** 1
**Source Plan Fingerprint:** `{plan_fingerprint}`
**Source Spec Path:** `{SPEC_REL}`
**Source Spec Revision:** 1
**Source Spec Fingerprint:** `{spec_fingerprint}`
**Source Task Packet Fingerprints:**
- `{packet_fingerprint}`
**Chunk ID:** chunk-1
**Chunking Strategy:** single_chunk
**Covered Steps:**
- Task 1 Step 1
**Requirement IDs:**
- REQ-001
**Criteria:**
### Criterion 1
**Criterion ID:** criterion-1
**Title:** Preserve active approved-plan scope
**Description:** Contract fixture stays within the approved plan scope.
**Requirement IDs:**
- REQ-001
**Covered Steps:**
- Task 1 Step 1
**Verifier Types:**
- spec_compliance
**Threshold:** all
**Notes:** Fixture criterion for runtime gate validation.

**Non Goals:**
- none

**Verifiers:**
- spec_compliance

**Evidence Requirements:**
[]

**Retry Budget:** 1
**Pivot Threshold:** 1
**Reset Policy:** none
**Generated By:** featureforge:executing-plans
**Generated At:** 2026-03-25T12:00:00Z
**Contract Fingerprint:** __CONTRACT_FINGERPRINT__
"#
    );
    let fingerprint = sha256_hex(template.replace("__CONTRACT_FINGERPRINT__", "").as_bytes());
    let source = template.replace("__CONTRACT_FINGERPRINT__", &fingerprint);
    write_file(
        &repo.join("docs/featureforge/execution-evidence/active-execution-contract.md"),
        &source,
    );
    let path = harness_authoritative_artifacts_dir(state, &repo_slug(repo), &branch_name(repo))
        .join(format!("contract-{fingerprint}.md"));
    write_file(&path, &source);
    fingerprint
}

fn reconcile_result_proof_fingerprint(repo: &Path, commit_sha: &str) -> String {
    let output = run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["cat-file", "commit", commit_sha])
                .current_dir(repo);
            command
        },
        "git cat-file commit",
    );
    sha256_hex(&output.stdout)
}

fn canonical_unit_review_receipt_fingerprint(source: &str) -> String {
    let filtered = source
        .lines()
        .filter(|line| !line.trim().starts_with("**Receipt Fingerprint:**"))
        .collect::<Vec<_>>()
        .join("\n");
    sha256_hex(filtered.as_bytes())
}

fn write_serial_unit_review_receipt(repo: &Path, state: &Path, active_contract_fingerprint: &str) {
    let branch = branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let execution_run_id = format!("run-{safe_branch}-finish");
    let execution_unit_id = String::from("task-1-step-1");
    let reviewed_checkpoint_commit_sha = current_head_sha(repo);
    let approved_task_packet_fingerprint = expected_packet_fingerprint(repo, 1, 1);
    let execution_context_key = sha256_hex(
        format!(
            "run={execution_run_id}\nunit={execution_unit_id}\nplan={PLAN_REL}\nplan_revision=1\nbranch={branch}\nreviewed_checkpoint={reviewed_checkpoint_commit_sha}\n"
        )
        .as_bytes(),
    );
    let approved_unit_contract_fingerprint = sha256_hex(
        format!(
            "approved-unit-contract:{active_contract_fingerprint}:{approved_task_packet_fingerprint}:{execution_unit_id}"
        )
        .as_bytes(),
    );
    let reviewed_worktree = fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
    let reconcile_result_proof_fingerprint =
        reconcile_result_proof_fingerprint(repo, &reviewed_checkpoint_commit_sha);
    let lease_fingerprint = sha256_hex(
        format!(
            "serial-unit-review:{execution_run_id}:{execution_unit_id}:{execution_context_key}:{reviewed_checkpoint_commit_sha}:{approved_task_packet_fingerprint}:{approved_unit_contract_fingerprint}"
        )
        .as_bytes(),
    );
    let unsigned_source = format!(
        "# Unit Review Result\n**Review Stage:** featureforge:unit-review\n**Reviewer Provenance:** dedicated-independent\n**Source Plan:** {PLAN_REL}\n**Source Plan Revision:** 1\n**Execution Run ID:** {execution_run_id}\n**Execution Unit ID:** {execution_unit_id}\n**Lease Fingerprint:** {lease_fingerprint}\n**Execution Context Key:** {execution_context_key}\n**Approved Task Packet Fingerprint:** {approved_task_packet_fingerprint}\n**Approved Unit Contract Fingerprint:** {approved_unit_contract_fingerprint}\n**Reconciled Result SHA:** {reviewed_checkpoint_commit_sha}\n**Reconcile Result Proof Fingerprint:** {reconcile_result_proof_fingerprint}\n**Reconcile Mode:** identity_preserving\n**Reviewed Worktree:** {}\n**Reviewed Checkpoint SHA:** {reviewed_checkpoint_commit_sha}\n**Result:** pass\n**Generated By:** featureforge:unit-review\n**Generated At:** 2026-03-27T12:00:00Z\n",
        reviewed_worktree.display()
    );
    let receipt_fingerprint = canonical_unit_review_receipt_fingerprint(&unsigned_source);
    let source = format!(
        "# Unit Review Result\n**Receipt Fingerprint:** {receipt_fingerprint}\n{}",
        unsigned_source.trim_start_matches("# Unit Review Result\n")
    );
    let receipt_path =
        harness_authoritative_artifacts_dir(state, &repo_slug(repo), &branch_name(repo)).join(
            format!("unit-review-{execution_run_id}-{execution_unit_id}.md"),
        );
    write_file(&receipt_path, &source);
}

fn write_harness_state_payload(repo: &Path, state: &Path, payload: &Value) {
    let path = harness_state_path(state, &repo_slug(repo), &branch_name(repo));
    write_file(
        &path,
        &serde_json::to_string_pretty(payload).expect("harness state payload should serialize"),
    );
    let events_path = path.with_file_name("events.jsonl");
    let legacy_backup_path = path.with_file_name("state.legacy.json");
    let _ = fs::remove_file(events_path);
    let _ = fs::remove_file(legacy_backup_path);
}

fn merge_harness_state_payload(repo: &Path, state: &Path, patch: &Value) {
    let path = harness_state_path(state, &repo_slug(repo), &branch_name(repo));
    let source = fs::read_to_string(&path).expect("existing harness state should be readable");
    let mut payload: Value =
        serde_json::from_str(&source).expect("existing harness state should be valid json");
    let payload_object = payload
        .as_object_mut()
        .expect("existing harness state should be a json object");
    let patch_object = patch
        .as_object()
        .expect("harness state patch should be a json object");
    for (key, value) in patch_object {
        payload_object.insert(key.clone(), value.clone());
    }
    write_harness_state_payload(repo, state, &payload);
}

fn clear_current_record_binding(
    repo: &Path,
    state: &Path,
    current_id_field: &str,
    history_field: &str,
) {
    let path = harness_state_path(state, &repo_slug(repo), &branch_name(repo));
    let source = fs::read_to_string(&path).expect("existing harness state should be readable");
    let mut payload: Value =
        serde_json::from_str(&source).expect("existing harness state should be valid json");
    let payload_object = payload
        .as_object_mut()
        .expect("existing harness state should be a json object");
    if let Some(current_record_id) = payload_object
        .get(current_id_field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        && let Some(history) = payload_object
            .get_mut(history_field)
            .and_then(Value::as_object_mut)
    {
        history.remove(&current_record_id);
    }
    payload_object.insert(current_id_field.to_owned(), Value::Null);
    write_harness_state_payload(repo, state, &payload);
}

fn set_current_history_record_field(
    repo: &Path,
    state: &Path,
    current_id_field: &str,
    history_field: &str,
    field: &str,
    value: Value,
) {
    let path = harness_state_path(state, &repo_slug(repo), &branch_name(repo));
    let source = fs::read_to_string(&path).expect("existing harness state should be readable");
    let mut payload: Value =
        serde_json::from_str(&source).expect("existing harness state should be valid json");
    let current_record_id = payload
        .get(current_id_field)
        .and_then(Value::as_str)
        .expect("current record id should exist for fixture mutation")
        .to_owned();
    let history = payload
        .get_mut(history_field)
        .and_then(Value::as_object_mut)
        .expect("history should be a JSON object for fixture mutation");
    let record = history
        .get_mut(&current_record_id)
        .and_then(Value::as_object_mut)
        .expect("current record payload should exist for fixture mutation");
    record.insert(field.to_owned(), value);
    write_harness_state_payload(repo, state, &payload);
}

fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn write_finish_ready_harness_state_with_reason_codes(
    repo: &Path,
    state: &Path,
    reason_codes: &[&str],
) {
    let branch = branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let base_branch = expected_base_branch(repo);
    let artifact_dir = project_artifact_dir(repo, state);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let branch_closure_id = "branch-closure-ready";
    let branch_contract_identity = branch_contract_identity(repo, state, PLAN_REL);
    let review_path = latest_branch_artifact_path(&artifact_dir, &branch, "code-review");
    let has_review = review_path.is_some();
    let review_state = if review_path.is_some() {
        "fresh"
    } else {
        "missing"
    };
    let (
        review_fingerprint,
        final_review_record_id,
        mut final_review_record_history,
        current_final_review_dispatch_id,
        current_final_review_reviewer_source,
        current_final_review_reviewer_id,
        current_final_review_result,
        current_final_review_summary_hash,
    ) = if let Some(review_path) = review_path {
        let review_source =
            fs::read_to_string(&review_path).expect("code-review artifact should be readable");
        let review_fingerprint = sha256_hex(review_source.as_bytes());
        let authoritative_review_path =
            harness_authoritative_artifacts_dir(state, &repo_slug(repo), &branch)
                .join(format!("final-review-{review_fingerprint}.md"));
        write_file(&authoritative_review_path, &review_source);
        let final_review_summary =
            "Final whole-diff review artifact fixture for final-review gate coverage.";
        let final_review_summary_hash = sha256_hex(final_review_summary.as_bytes());
        let final_review_record_id = format!("final-review-record-{review_fingerprint}");
        (
            Value::from(review_fingerprint.clone()),
            Value::from(final_review_record_id.clone()),
            json!({
                final_review_record_id.clone(): {
                    "record_id": final_review_record_id,
                    "record_sequence": 1,
                    "record_status": "current",
                    "branch_closure_id": branch_closure_id,
                    "dispatch_id": "fixture-final-review-dispatch",
                    "reviewer_source": "fresh-context-subagent",
                    "reviewer_id": "reviewer-fixture-001",
                    "result": "pass",
                    "final_review_fingerprint": review_fingerprint,
                    "browser_qa_required": false,
                    "source_plan_path": PLAN_REL,
                    "source_plan_revision": 1,
                    "repo_slug": repo_slug(repo),
                    "branch_name": branch.clone(),
                    "base_branch": base_branch.clone(),
                    "reviewed_state_id": reviewed_state_id.clone(),
                    "summary": final_review_summary,
                    "summary_hash": final_review_summary_hash
                }
            }),
            Value::from("fixture-final-review-dispatch"),
            Value::from("fresh-context-subagent"),
            Value::from("reviewer-fixture-001"),
            Value::from("pass"),
            Value::from(final_review_summary_hash),
        )
    } else {
        (
            Value::Null,
            Value::Null,
            json!({}),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        )
    };

    let release_path = latest_branch_artifact_path(&artifact_dir, &branch, "release-readiness")
        .expect("finish-ready harness state should have a branch release-readiness artifact");
    let release_source =
        fs::read_to_string(&release_path).expect("release-readiness artifact should be readable");
    let release_fingerprint = sha256_hex(release_source.as_bytes());
    let authoritative_release_path =
        harness_authoritative_artifacts_dir(state, &repo_slug(repo), &branch)
            .join(format!("release-docs-{release_fingerprint}.md"));
    write_file(&authoritative_release_path, &release_source);
    let release_summary = "Release-readiness artifact fixture for final-review gate coverage.";
    let release_summary_hash = sha256_hex(release_summary.as_bytes());
    let release_record_id = format!("release-readiness-record-{release_fingerprint}");
    if let Some(record_id) = final_review_record_id.as_str()
        && let Some(record) = final_review_record_history
            .get_mut(record_id)
            .and_then(Value::as_object_mut)
    {
        record.insert(
            String::from("release_readiness_record_id"),
            Value::from(release_record_id.clone()),
        );
    }
    let task_review_summary = "Task closure review fixture for final-review gate coverage.";
    let task_review_summary_hash = sha256_hex(task_review_summary.as_bytes());
    let task_verification_summary =
        "Task closure verification fixture for final-review gate coverage.";
    let task_verification_summary_hash = sha256_hex(task_verification_summary.as_bytes());
    let task_closure_record = json!({
        "dispatch_id": "fixture-task-dispatch",
        "closure_record_id": "task-1-closure",
        "source_plan_path": PLAN_REL,
        "source_plan_revision": 1,
        "execution_run_id": format!("run-{safe_branch}-finish"),
        "reviewed_state_id": reviewed_state_id.clone(),
        "contract_identity": task_contract_identity(repo, state, PLAN_REL, 1),
        "effective_reviewed_surface_paths": ["README.md"],
        "review_result": "pass",
        "review_summary_hash": task_review_summary_hash,
        "verification_result": "pass",
        "verification_summary_hash": task_verification_summary_hash,
        "closure_status": "current",
    });

    let mut harness_state = json!({
        "schema_version": 1,
        "harness_phase": "executing",
        "run_identity": {
            "execution_run_id": format!("run-{safe_branch}-finish"),
            "source_plan_path": PLAN_REL,
            "source_plan_revision": 1
        },
        "chunk_id": format!("chunk-{safe_branch}-finish"),
        "latest_authoritative_sequence": 17,
        "repo_state_baseline_head_sha": current_head_sha(repo),
        "repo_state_baseline_worktree_fingerprint": "2222222222222222222222222222222222222222222222222222222222222222",
        "repo_state_drift_state": "reconciled",
        "current_branch_closure_id": branch_closure_id,
        "current_branch_closure_reviewed_state_id": reviewed_state_id,
        "current_branch_closure_contract_identity": branch_contract_identity,
        "branch_closure_records": {
            branch_closure_id: {
                "branch_closure_id": branch_closure_id,
                "source_plan_path": PLAN_REL,
                "source_plan_revision": 1,
                "repo_slug": repo_slug(repo),
                "branch_name": branch.clone(),
                "base_branch": base_branch.clone(),
                "reviewed_state_id": reviewed_state_id,
                "contract_identity": branch_contract_identity,
                "effective_reviewed_branch_surface": "repo_tracked_content",
                "source_task_closure_ids": ["task-1-closure"],
                "provenance_basis": "task_closure_lineage",
                "closure_status": "current",
                "superseded_branch_closure_ids": []
            }
        },
        "finish_review_gate_pass_branch_closure_id": branch_closure_id,
        "dependency_index_state": "fresh",
        "final_review_state": review_state,
        "browser_qa_state": "not_required",
        "release_docs_state": "fresh",
        "current_release_readiness_result": "ready",
        "current_release_readiness_summary_hash": release_summary_hash,
        "current_release_readiness_record_id": release_record_id,
        "release_readiness_record_history": {
            release_record_id.clone(): {
                "record_id": release_record_id,
                "record_sequence": 1,
                "record_status": "current",
                "branch_closure_id": branch_closure_id,
                "source_plan_path": PLAN_REL,
                "source_plan_revision": 1,
                "repo_slug": repo_slug(repo),
                "branch_name": branch.clone(),
                "base_branch": base_branch.clone(),
                "reviewed_state_id": reviewed_state_id.clone(),
                "result": "ready",
                "release_docs_fingerprint": release_fingerprint,
                "summary": release_summary,
                "summary_hash": release_summary_hash,
                "generated_by_identity": "featureforge/release-readiness"
            }
        },
        "current_final_review_branch_closure_id": if has_review { Value::from(branch_closure_id) } else { Value::Null },
        "current_final_review_dispatch_id": current_final_review_dispatch_id,
        "current_final_review_reviewer_source": current_final_review_reviewer_source,
        "current_final_review_reviewer_id": current_final_review_reviewer_id,
        "current_final_review_result": current_final_review_result,
        "current_final_review_summary_hash": current_final_review_summary_hash,
        "current_final_review_record_id": final_review_record_id,
        "final_review_record_history": final_review_record_history,
        "last_final_review_artifact_fingerprint": review_fingerprint,
        "last_release_docs_artifact_fingerprint": release_fingerprint,
        "active_worktree_lease_fingerprints": [],
        "active_worktree_lease_bindings": [],
        "strategy_state": "ready",
        "strategy_checkpoint_kind": "initial_dispatch",
        "last_strategy_checkpoint_fingerprint": STRATEGY_CHECKPOINT_FINGERPRINT,
        "strategy_reset_required": false,
        "reason_codes": reason_codes
    });
    harness_state["current_task_closure_records"] = json!({
        "task-1": task_closure_record.clone(),
    });
    harness_state["task_closure_record_history"] = json!({
        "task-1-closure": task_closure_record,
    });
    write_harness_state_payload(repo, state, &harness_state);
}

fn write_matching_topology_downgrade_record(repo: &Path, state: &Path, base_branch: &str) {
    let branch = branch_name(repo);
    let execution_context_key = format!("{branch}@{base_branch}");
    let source = json!({
        "record_version": 1,
        "authoritative_sequence": 18,
        "source_plan_path": PLAN_REL,
        "source_plan_revision": 1,
        "execution_context_key": execution_context_key,
        "primary_reason_class": "dependency_mismatch",
        "detail": {
            "trigger_summary": "Parallel lanes depended on shared write scope ordering.",
            "affected_units": ["task-1-step-1"],
            "blocking_evidence": {
                "summary": "Observed dependency mismatch while reconciling unit lane.",
                "references": ["artifact:unit-review-run-task-1-step-1"]
            },
            "operator_impact": {
                "severity": "warning",
                "changed_or_blocked_stage": "executing",
                "expected_response": "downgrade the slice"
            },
            "notes": ["runtime-authored test fixture"]
        },
        "rerun_guidance_superseded": false,
        "generated_by": "featureforge:execution-runtime",
        "generated_at": "2026-03-28T15:00:00Z",
        "record_fingerprint": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    });
    let record_path = harness_authoritative_artifacts_dir(state, &repo_slug(repo), &branch)
        .join("execution-topology-downgrade-dependency-mismatch.json");
    write_file(
        &record_path,
        &serde_json::to_string_pretty(&source)
            .expect("topology downgrade record fixture should serialize"),
    );
}

#[test]
fn dedicated_final_review_receipt_requires_dedicated_provenance() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-dedicated-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            "**Reviewer Provenance:** dedicated-independent",
            "**Reviewer Provenance:** implementation-context",
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("non-dedicated review provenance should fail validation");
    assert_eq!(error, FinalReviewReceiptIssue::ReviewerProvenanceMissing);
    assert_eq!(error.reason_code(), "review_receipt_not_dedicated");
}

#[test]
fn dedicated_final_review_receipt_requires_distinct_from_stages() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-distinct-stages");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            "**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n",
            "",
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("final review should declare which implementation stages it is distinct from");
    assert_eq!(error, FinalReviewReceiptIssue::DistinctFromStagesMissing);
    assert_eq!(
        error.reason_code(),
        "review_receipt_distinct_from_stages_missing"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_reviewer_identity_headers() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-identity-headers");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace("**Reviewer ID:** reviewer-fixture-001\n", ""),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("dedicated final review should require reviewer identity headers");
    assert_eq!(error, FinalReviewReceiptIssue::ReviewerIdentityMissing);
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_identity_missing"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_independent_reviewer_source() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-reviewer-source");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            "**Reviewer Source:** fresh-context-subagent",
            "**Reviewer Source:** implementation-context",
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None).expect_err(
        "dedicated final review should require an approved independent reviewer source",
    );
    assert_eq!(error, FinalReviewReceiptIssue::ReviewerSourceNotIndependent);
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_source_not_independent"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_strategy_checkpoint_fingerprint_when_expected() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-strategy-fingerprint-missing");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            &format!("**Strategy Checkpoint Fingerprint:** {STRATEGY_CHECKPOINT_FINGERPRINT}\n"),
            "",
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(
        &receipt,
        &review_path,
        repo,
        Some(STRATEGY_CHECKPOINT_FINGERPRINT),
    )
    .expect_err("dedicated final review should require strategy checkpoint binding");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::StrategyCheckpointFingerprintMissing
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_strategy_checkpoint_fingerprint_missing"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_matching_strategy_checkpoint_fingerprint() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-strategy-fingerprint-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            STRATEGY_CHECKPOINT_FINGERPRINT,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(
        &receipt,
        &review_path,
        repo,
        Some(STRATEGY_CHECKPOINT_FINGERPRINT),
    )
    .expect_err("dedicated final review should require matching strategy checkpoint binding");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::StrategyCheckpointFingerprintMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_strategy_checkpoint_fingerprint_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_reviewer_artifact_path() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-reviewer-artifact-path");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let receipt = parse_final_review_receipt(&review_path);
    let reviewer_artifact_path = receipt
        .reviewer_artifact_path
        .expect("fixture review artifact should include reviewer artifact path");
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            &format!("**Reviewer Artifact Path:** `{reviewer_artifact_path}`\n"),
            "",
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("dedicated final review should require reviewer artifact path binding");
    assert_eq!(error, FinalReviewReceiptIssue::ReviewerArtifactPathMissing);
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_artifact_path_missing"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_readable_reviewer_artifact_path() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-reviewer-artifact-unreadable");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let reviewer_artifact_path = reviewer_artifact_path_from_review(&review_path);
    fs::remove_file(&reviewer_artifact_path).expect("reviewer artifact should remove");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("dedicated final review should reject unreadable reviewer artifact paths");
    assert_eq!(error, FinalReviewReceiptIssue::ReviewerArtifactUnreadable);
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_artifact_unreadable"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_runtime_owned_reviewer_artifact() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-reviewer-artifact-runtime-owned");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let reviewer_artifact_path = reviewer_artifact_path_from_review(&review_path);
    let reviewer_artifact_source =
        fs::read_to_string(&reviewer_artifact_path).expect("reviewer artifact should read");
    let external_dir = TempDir::new().expect("external reviewer tempdir should exist");
    let external_reviewer_artifact_path = external_dir
        .path()
        .join("reviewer-artifact-outside-runtime.md");
    fs::write(&external_reviewer_artifact_path, &reviewer_artifact_source)
        .expect("external reviewer artifact should write");
    let external_reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&external_reviewer_artifact_path).expect("artifact should read"));
    let receipt = parse_final_review_receipt(&review_path);
    let reviewer_artifact_fingerprint = receipt
        .reviewer_artifact_fingerprint
        .clone()
        .expect("review receipt should include reviewer artifact fingerprint");
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original
            .replace(
                &format!("`{}`", reviewer_artifact_path.display()),
                &format!("`{}`", external_reviewer_artifact_path.display()),
            )
            .replace(
                &reviewer_artifact_fingerprint,
                &external_reviewer_artifact_fingerprint,
            ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(
        &receipt,
        &review_path,
        repo,
        None,
    )
    .expect_err("dedicated final review should reject reviewer artifacts outside runtime-owned project artifacts");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactNotRuntimeOwned
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_artifact_not_runtime_owned"
    );
}

#[test]
fn dedicated_final_review_receipt_rejects_sibling_project_reviewer_artifact() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-reviewer-artifact-sibling-project");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let reviewer_artifact_path = reviewer_artifact_path_from_review(&review_path);
    let reviewer_artifact_source =
        fs::read_to_string(&reviewer_artifact_path).expect("reviewer artifact should read");
    let sibling_project_dir = state.join("projects").join("different-project-slug");
    fs::create_dir_all(&sibling_project_dir).expect("sibling project artifact dir should create");
    let sibling_reviewer_artifact_path = sibling_project_dir.join("reviewer-artifact.md");
    fs::write(&sibling_reviewer_artifact_path, &reviewer_artifact_source)
        .expect("sibling reviewer artifact should write");
    let sibling_reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&sibling_reviewer_artifact_path).expect("artifact should read"));
    let receipt = parse_final_review_receipt(&review_path);
    let reviewer_artifact_fingerprint = receipt
        .reviewer_artifact_fingerprint
        .clone()
        .expect("review receipt should include reviewer artifact fingerprint");
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original
            .replace(
                &format!("`{}`", reviewer_artifact_path.display()),
                &format!("`{}`", sibling_reviewer_artifact_path.display()),
            )
            .replace(
                &reviewer_artifact_fingerprint,
                &sibling_reviewer_artifact_fingerprint,
            ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None).expect_err(
        "dedicated final review should reject reviewer artifacts from sibling project slugs",
    );
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactNotRuntimeOwned
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_artifact_not_runtime_owned"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_reviewer_artifact_fingerprint() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-reviewer-fingerprint");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let receipt = parse_final_review_receipt(&review_path);
    let reviewer_artifact_fingerprint = receipt
        .reviewer_artifact_fingerprint
        .clone()
        .expect("fixture review artifact should include reviewer fingerprint");
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            &format!("**Reviewer Artifact Fingerprint:** {reviewer_artifact_fingerprint}"),
            "**Reviewer Artifact Fingerprint:** not-a-fingerprint",
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("dedicated final review should require canonical reviewer fingerprint");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactFingerprintInvalid
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_fingerprint_invalid"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_matching_reviewer_artifact_fingerprint() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-reviewer-fingerprint-match");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let receipt = parse_final_review_receipt(&review_path);
    let reviewer_artifact_fingerprint = receipt
        .reviewer_artifact_fingerprint
        .clone()
        .expect("fixture review artifact should include reviewer fingerprint");
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            &format!("**Reviewer Artifact Fingerprint:** {reviewer_artifact_fingerprint}"),
            "**Reviewer Artifact Fingerprint:** ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("dedicated final review should require reviewer artifact fingerprint binding");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactFingerprintMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_fingerprint_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_matching_reviewer_artifact_identity() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-reviewer-identity-match");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            "**Reviewer Source:** fresh-context-subagent",
            "**Reviewer Source:** cross-model",
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None).expect_err(
        "dedicated final review should require reviewer identity to match reviewer artifact",
    );
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactIdentityMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_identity_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_reviewer_artifact_contract_binding() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-reviewer-contract-binding");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let reviewer_artifact_path = reviewer_artifact_path_from_review(&review_path);
    let reviewer_artifact_source =
        fs::read_to_string(&reviewer_artifact_path).expect("reviewer artifact should read");
    fs::write(
        &reviewer_artifact_path,
        reviewer_artifact_source.replace(
            &format!("**Head SHA:** {}", current_head_sha(repo)),
            "**Head SHA:** ffffffffffffffffffffffffffffffffffffffff",
        ),
    )
    .expect("reviewer artifact should write");
    let reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&reviewer_artifact_path).expect("reviewer artifact should read"));
    let receipt = parse_final_review_receipt(&review_path);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            receipt
                .reviewer_artifact_fingerprint
                .as_deref()
                .expect("review receipt should include reviewer artifact fingerprint"),
            &reviewer_artifact_fingerprint,
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("dedicated final review should require reviewer artifact contract binding");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactContractMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_artifact_contract_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_reviewer_artifact_strategy_checkpoint_binding() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-reviewer-strategy-checkpoint-binding");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let reviewer_artifact_path = reviewer_artifact_path_from_review(&review_path);
    let reviewer_artifact_source =
        fs::read_to_string(&reviewer_artifact_path).expect("reviewer artifact should read");
    fs::write(
        &reviewer_artifact_path,
        reviewer_artifact_source.replace(
            &format!(
                "**Strategy Checkpoint Fingerprint:** {STRATEGY_CHECKPOINT_FINGERPRINT}"
            ),
            "**Strategy Checkpoint Fingerprint:** ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ),
    )
    .expect("reviewer artifact should write");
    let reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&reviewer_artifact_path).expect("reviewer artifact should read"));
    let receipt = parse_final_review_receipt(&review_path);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            receipt
                .reviewer_artifact_fingerprint
                .as_deref()
                .expect("review receipt should include reviewer artifact fingerprint"),
            &reviewer_artifact_fingerprint,
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(
        &receipt,
        &review_path,
        repo,
        Some(STRATEGY_CHECKPOINT_FINGERPRINT),
    )
    .expect_err(
        "dedicated final review should require reviewer artifact strategy checkpoint binding",
    );
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactContractMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_artifact_contract_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_reviewer_artifact_deviation_disposition_binding() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-reviewer-deviation-disposition-binding");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let reviewer_artifact_path = reviewer_artifact_path_from_review(&review_path);
    let reviewer_artifact_source =
        fs::read_to_string(&reviewer_artifact_path).expect("reviewer artifact should read");
    fs::write(
        &reviewer_artifact_path,
        reviewer_artifact_source
            .replace(
                "**Recorded Execution Deviations:** none",
                "**Recorded Execution Deviations:** present",
            )
            .replace(
                "**Deviation Review Verdict:** not_required",
                "**Deviation Review Verdict:** pass",
            ),
    )
    .expect("reviewer artifact should write");
    let reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&reviewer_artifact_path).expect("reviewer artifact should read"));
    let receipt = parse_final_review_receipt(&review_path);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            receipt
                .reviewer_artifact_fingerprint
                .as_deref()
                .expect("review receipt should include reviewer artifact fingerprint"),
            &reviewer_artifact_fingerprint,
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None).expect_err(
        "dedicated final review should require reviewer artifact deviation disposition binding",
    );
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactContractMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_artifact_contract_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_reviewer_artifact_base_branch_binding() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-reviewer-base-branch-binding");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let reviewer_artifact_path = reviewer_artifact_path_from_review(&review_path);
    let reviewer_artifact_source =
        fs::read_to_string(&reviewer_artifact_path).expect("reviewer artifact should read");
    fs::write(
        &reviewer_artifact_path,
        reviewer_artifact_source.replace(
            &format!("**Base Branch:** {base_branch}"),
            "**Base Branch:** different-base",
        ),
    )
    .expect("reviewer artifact should write");
    let reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&reviewer_artifact_path).expect("reviewer artifact should read"));
    let receipt = parse_final_review_receipt(&review_path);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            receipt
                .reviewer_artifact_fingerprint
                .as_deref()
                .expect("review receipt should include reviewer artifact fingerprint"),
            &reviewer_artifact_fingerprint,
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("dedicated final review should require reviewer artifact base-branch binding");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactContractMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_artifact_contract_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_reviewer_artifact_branch_and_repo_binding() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-reviewer-branch-repo-binding");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let reviewer_artifact_path = reviewer_artifact_path_from_review(&review_path);
    let reviewer_artifact_source =
        fs::read_to_string(&reviewer_artifact_path).expect("reviewer artifact should read");
    fs::write(
        &reviewer_artifact_path,
        reviewer_artifact_source
            .replace(
                &format!("**Branch:** {}", branch_name(repo)),
                "**Branch:** different-branch",
            )
            .replace(
                &format!("**Repo:** {}", repo_slug(repo)),
                "**Repo:** different-repo",
            ),
    )
    .expect("reviewer artifact should write");
    let reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&reviewer_artifact_path).expect("reviewer artifact should read"));
    let receipt = parse_final_review_receipt(&review_path);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            receipt
                .reviewer_artifact_fingerprint
                .as_deref()
                .expect("review receipt should include reviewer artifact fingerprint"),
            &reviewer_artifact_fingerprint,
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("dedicated final review should require reviewer artifact branch/repo binding");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactContractMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_artifact_contract_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_rejects_self_referential_reviewer_artifact_path() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-reviewer-artifact-self-reference");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let receipt = parse_final_review_receipt(&review_path);
    let reviewer_artifact_path = receipt
        .reviewer_artifact_path
        .as_deref()
        .expect("fixture review artifact should include reviewer artifact path");
    let reviewer_artifact_fingerprint = receipt
        .reviewer_artifact_fingerprint
        .as_deref()
        .expect("fixture review artifact should include reviewer artifact fingerprint");
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original
            .replace(
                &format!("`{reviewer_artifact_path}`"),
                &format!("`{}`", review_path.display()),
            )
            .replace(
                reviewer_artifact_fingerprint,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("dedicated final review should reject self-referential reviewer artifacts");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactContractMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_artifact_contract_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_rejects_code_review_to_code_review_reference() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-reviewer-artifact-code-review-reference");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let decoy_review_path = project_artifact_dir(repo, state).join("decoy-code-review.md");
    let review_source = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(&decoy_review_path, &review_source).expect("decoy review artifact should write");
    let decoy_review_fingerprint =
        sha256_hex(&fs::read(&decoy_review_path).expect("decoy review artifact should read"));

    let receipt = parse_final_review_receipt(&review_path);
    let reviewer_artifact_path = receipt
        .reviewer_artifact_path
        .as_deref()
        .expect("fixture review artifact should include reviewer artifact path");
    let reviewer_artifact_fingerprint = receipt
        .reviewer_artifact_fingerprint
        .as_deref()
        .expect("fixture review artifact should include reviewer artifact fingerprint");
    fs::write(
        &review_path,
        review_source
            .replace(
                &format!("`{reviewer_artifact_path}`"),
                &format!("`{}`", decoy_review_path.display()),
            )
            .replace(reviewer_artifact_fingerprint, &decoy_review_fingerprint),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("dedicated final review should reject code-review-to-code-review references");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::ReviewerArtifactContractMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_reviewer_artifact_contract_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_implementation_stage_distinctness() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-distinct-stage-values");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            "**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development",
            "**Distinct From Stages:** featureforge:requesting-code-review",
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None)
        .expect_err("final review should prove independence from implementation stages");
    assert_eq!(error, FinalReviewReceiptIssue::DistinctFromStagesInvalid);
    assert_eq!(
        error.reason_code(),
        "review_receipt_distinct_from_stages_invalid"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_both_implementation_stages() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-both-stage-values");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original.replace(
            "**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development",
            "**Distinct From Stages:** featureforge:executing-plans",
        ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None).expect_err(
        "final review should name both implementation stages in its distinctness proof",
    );
    assert_eq!(error, FinalReviewReceiptIssue::DistinctFromStagesInvalid);
    assert_eq!(
        error.reason_code(),
        "review_receipt_distinct_from_stages_invalid"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_passed_deviation_disposition_when_needed() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-deviation-pass");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original
            .replace(
                "**Recorded Execution Deviations:** none",
                "**Recorded Execution Deviations:** present",
            )
            .replace(
                "**Deviation Review Verdict:** not_required",
                "**Deviation Review Verdict:** fail",
            ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let expectations = FinalReviewReceiptExpectations {
        expected_plan_path: PLAN_REL,
        expected_plan_revision: 1,
        expected_strategy_checkpoint_fingerprint: None,
        expected_branch: &branch_name(repo),
        expected_repo: &repo_slug(repo),
        expected_head_sha: &current_head_sha(repo),
        expected_base_branch: &expected_base_branch(repo),
        expected_result: "pass",
        deviations_required: true,
    };
    let error = validate_final_review_receipt(&receipt, &review_path, &expectations)
        .expect_err("deviation-aware final review should require a passing disposition");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::DeviationReviewVerdictMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_deviation_verdict_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_accepts_failed_result_with_independent_deviation_pass() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-deviation-pass-on-failed-review");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let reviewer_artifact_path = reviewer_artifact_path_from_review(&review_path);
    let reviewer_original =
        fs::read_to_string(&reviewer_artifact_path).expect("reviewer artifact should read");
    fs::write(
        &reviewer_artifact_path,
        reviewer_original
            .replace(
                "**Recorded Execution Deviations:** none",
                "**Recorded Execution Deviations:** present",
            )
            .replace(
                "**Deviation Review Verdict:** not_required",
                "**Deviation Review Verdict:** pass",
            )
            .replace("**Result:** pass", "**Result:** fail"),
    )
    .expect("reviewer artifact should write");
    let reviewer_artifact_fingerprint = sha256_hex(
        &fs::read(&reviewer_artifact_path).expect("updated reviewer artifact should read"),
    );
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    let current_review = parse_final_review_receipt(&review_path);
    fs::write(
        &review_path,
        original
            .replace(
                "**Recorded Execution Deviations:** none",
                "**Recorded Execution Deviations:** present",
            )
            .replace(
                "**Deviation Review Verdict:** not_required",
                "**Deviation Review Verdict:** pass",
            )
            .replace("**Result:** pass", "**Result:** fail")
            .replace(
                current_review
                    .reviewer_artifact_fingerprint
                    .as_deref()
                    .expect("review receipt should expose reviewer artifact fingerprint"),
                &reviewer_artifact_fingerprint,
            ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let expectations = FinalReviewReceiptExpectations {
        expected_plan_path: PLAN_REL,
        expected_plan_revision: 1,
        expected_strategy_checkpoint_fingerprint: None,
        expected_branch: &branch_name(repo),
        expected_repo: &repo_slug(repo),
        expected_head_sha: &current_head_sha(repo),
        expected_base_branch: &expected_base_branch(repo),
        expected_result: "fail",
        deviations_required: true,
    };
    validate_final_review_receipt(&receipt, &review_path, &expectations).expect(
        "deviation-aware final review should accept a passing deviation verdict even when the overall review result is fail",
    );
}

#[test]
fn dedicated_final_review_receipt_rejects_failed_result_with_failed_deviation_verdict() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-deviation-fail-on-failed-review");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let reviewer_artifact_path = reviewer_artifact_path_from_review(&review_path);
    let reviewer_original =
        fs::read_to_string(&reviewer_artifact_path).expect("reviewer artifact should read");
    fs::write(
        &reviewer_artifact_path,
        reviewer_original
            .replace(
                "**Recorded Execution Deviations:** none",
                "**Recorded Execution Deviations:** present",
            )
            .replace(
                "**Deviation Review Verdict:** not_required",
                "**Deviation Review Verdict:** fail",
            )
            .replace("**Result:** pass", "**Result:** fail"),
    )
    .expect("reviewer artifact should write");
    let reviewer_artifact_fingerprint = sha256_hex(
        &fs::read(&reviewer_artifact_path).expect("updated reviewer artifact should read"),
    );
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    let current_review = parse_final_review_receipt(&review_path);
    fs::write(
        &review_path,
        original
            .replace(
                "**Recorded Execution Deviations:** none",
                "**Recorded Execution Deviations:** present",
            )
            .replace(
                "**Deviation Review Verdict:** not_required",
                "**Deviation Review Verdict:** fail",
            )
            .replace("**Result:** pass", "**Result:** fail")
            .replace(
                current_review
                    .reviewer_artifact_fingerprint
                    .as_deref()
                    .expect("review receipt should expose reviewer artifact fingerprint"),
                &reviewer_artifact_fingerprint,
            ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let expectations = FinalReviewReceiptExpectations {
        expected_plan_path: PLAN_REL,
        expected_plan_revision: 1,
        expected_strategy_checkpoint_fingerprint: None,
        expected_branch: &branch_name(repo),
        expected_repo: &repo_slug(repo),
        expected_head_sha: &current_head_sha(repo),
        expected_base_branch: &expected_base_branch(repo),
        expected_result: "fail",
        deviations_required: true,
    };
    let error = validate_final_review_receipt(&receipt, &review_path, &expectations)
        .expect_err("deviation-aware final review should reject failed deviation verdicts");
    assert_eq!(
        error,
        FinalReviewReceiptIssue::DeviationReviewVerdictMismatch
    );
    assert_eq!(
        error.reason_code(),
        "review_receipt_deviation_verdict_mismatch"
    );
}

#[test]
fn dedicated_final_review_receipt_requires_explicit_no_deviation_disposition() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-no-deviation-disposition");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let original = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        original
            .replace(
                "**Recorded Execution Deviations:** none",
                "**Recorded Execution Deviations:** present",
            )
            .replace(
                "**Deviation Review Verdict:** not_required",
                "**Deviation Review Verdict:** pass",
            ),
    )
    .expect("review artifact should write");

    let receipt = parse_final_review_receipt(&review_path);
    let error = validate_fixture_review_receipt(&receipt, &review_path, repo, None).expect_err(
        "no-deviation receipts should still record explicit none/not_required disposition",
    );
    assert_eq!(error, FinalReviewReceiptIssue::DeviationRecordMismatch);
    assert_eq!(
        error.reason_code(),
        "review_receipt_deviation_record_mismatch"
    );
}

fn write_release_readiness_artifact(repo: &Path, state: &Path, base_branch: &str) -> PathBuf {
    let branch = branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let head_sha = current_head_sha(repo);
    let artifact_path = project_artifact_dir(repo, state).join(format!(
        "tester-{safe_branch}-release-readiness-20260322-171500.md"
    ));
    write_file(
        &artifact_path,
        &format!(
            "# Release Readiness Result\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Result:** pass\n**Generated By:** featureforge:document-release\n**Generated At:** 2026-03-22T17:15:00Z\n",
            repo_slug(repo)
        ),
    );
    artifact_path
}

#[test]
fn resolve_release_base_branch_reads_common_git_dir_in_worktrees() {
    let (repo_dir, _state_dir) = init_repo("plan-execution-final-review-worktree-base-branch");
    let repo = repo_dir.path();
    let worktree_root = repo.join("worktrees").join("review-lane");

    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["config", "branch.review-lane.gh-merge-base", "fixture-work"])
                .current_dir(repo);
            command
        },
        "git config branch.review-lane.gh-merge-base",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args([
                    "worktree",
                    "add",
                    "-b",
                    "review-lane",
                    worktree_root
                        .to_str()
                        .expect("worktree path should be utf-8"),
                ])
                .current_dir(repo);
            command
        },
        "git worktree add review-lane",
    );

    let git_dir = git_dir_path(&worktree_root);
    assert_eq!(
        resolve_release_base_branch(&git_dir, "review-lane").as_deref(),
        Some("fixture-work")
    );
}

#[test]
fn latest_branch_artifact_path_prefers_timestamp_over_username_prefix() {
    let artifact_dir = TempDir::new().expect("artifact tempdir should exist");
    let branch = "fixture-work";

    write_file(
        &artifact_dir
            .path()
            .join("zoe-fixture-work-code-review-20260322-171000.md"),
        &format!("# Code Review Result\n**Branch:** {branch}\n"),
    );
    let newest = artifact_dir
        .path()
        .join("alice-fixture-work-code-review-20260322-171100.md");
    write_file(
        &newest,
        &format!("# Code Review Result\n**Branch:** {branch}\n"),
    );

    assert_eq!(
        latest_branch_artifact_path(artifact_dir.path(), branch, "code-review").as_deref(),
        Some(newest.as_path())
    );
}

fn run_plan_execution_json_real_cli(
    repo: &Path,
    state: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state)
        .args(["plan", "execution"])
        .args(args);
    parse_json(&run(command, context), context)
}

fn run_runtime_preflight_gate_json(repo: &Path, state: &Path, plan_rel: &str) -> Value {
    plan_execution_direct_support::run_runtime_preflight_gate_json(
        repo,
        state,
        &featureforge::cli::plan_execution::StatusArgs {
            plan: PathBuf::from(plan_rel),
            external_review_result_ready: false,
        },
    )
    .expect("internal preflight helper should succeed")
}

fn run_runtime_finish_gate_json(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    external_review_result_ready: bool,
) -> Value {
    plan_execution_direct_support::run_runtime_finish_gate_json(
        repo,
        state,
        &featureforge::cli::plan_execution::StatusArgs {
            plan: PathBuf::from(plan_rel),
            external_review_result_ready,
        },
    )
    .expect("internal gate-finish helper should succeed")
}

fn internal_rebuild_evidence_args(
    plan_rel: &str,
) -> featureforge::cli::plan_execution::RebuildEvidenceArgs {
    featureforge::cli::plan_execution::RebuildEvidenceArgs {
        plan: PathBuf::from(plan_rel),
        all: false,
        tasks: Vec::new(),
        steps: Vec::new(),
        include_open: false,
        skip_manual_fallback: false,
        continue_on_error: false,
        dry_run: false,
        max_jobs: 1,
        no_output: false,
        json: true,
    }
}

fn run_internal_rebuild_evidence_json(repo: &Path, state: &Path, plan_rel: &str) -> Value {
    plan_execution_direct_support::run_internal_rebuild_evidence_json(
        repo,
        state,
        &internal_rebuild_evidence_args(plan_rel),
    )
    .expect("internal rebuild-evidence helper should succeed")
}

fn run_internal_rebuild_evidence_failure_json(repo: &Path, state: &Path, plan_rel: &str) -> Value {
    serde_json::from_str(
        &plan_execution_direct_support::run_internal_rebuild_evidence_json(
            repo,
            state,
            &internal_rebuild_evidence_args(plan_rel),
        )
        .expect_err("internal rebuild-evidence helper should fail"),
    )
    .expect("internal rebuild-evidence failure should serialize")
}

#[test]
fn gate_finish_requires_final_review_artifact() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-missing-review");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);
    clear_current_record_binding(
        repo,
        state,
        "current_final_review_record_id",
        "final_review_record_history",
    );
    merge_harness_state_payload(
        repo,
        state,
        &json!({
            "current_final_review_branch_closure_id": Value::Null,
            "current_final_review_dispatch_id": Value::Null,
            "current_final_review_reviewer_source": Value::Null,
            "current_final_review_reviewer_id": Value::Null,
            "current_final_review_result": Value::Null,
            "current_final_review_summary_hash": Value::Null
        }),
    );

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);

    assert_eq!(
        gate["allowed"], false,
        "missing review artifact should block finish"
    );
    assert!(
        gate["reason_codes"].as_array().is_some_and(|codes| {
            codes.iter().any(|code| {
                code == "review_artifact_missing" || code == "review_artifact_malformed"
            })
        }),
        "gate-finish should fail closed when final-review authoritative bindings are missing, got {}",
        pretty_json(&gate)
    );
}

#[test]
fn rebuild_evidence_rejects_tampered_authoritative_final_review_projection_content() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-projection-tampered-authoritative-content");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_base_branch(repo);
    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);
    let _ = run_runtime_preflight_gate_json(repo, state, PLAN_REL);

    let branch = branch_name(repo);
    let repo_slug = repo_slug(repo);
    let authoritative_review_fingerprint = sha256_hex(
        fs::read(&review_path)
            .expect(
                "source final-review artifact should be readable for authoritative tamper fixture",
            )
            .as_slice(),
    );
    let authoritative_review_path = harness_authoritative_artifacts_dir(state, &repo_slug, &branch)
        .join(format!(
            "final-review-{authoritative_review_fingerprint}.md"
        ));
    let harness_state_path = harness_state_path(state, &repo_slug, &branch);
    let harness_before: Value = serde_json::from_str(
        &fs::read_to_string(&harness_state_path).expect("harness state should be readable"),
    )
    .expect("harness state should remain valid json");
    let state_digest_before = sha256_hex(
        &serde_json::to_vec(&harness_before).expect("harness state json should serialize"),
    );
    let project_artifacts = project_artifact_dir(repo, state);
    let deleted_projection_path =
        latest_branch_artifact_path(&project_artifacts, &branch, "code-review")
            .expect("tamper fixture should expose a readable project code-review projection");
    fs::remove_file(&deleted_projection_path)
        .expect("tamper fixture should allow deleting the derived project code-review projection");
    write_file(
        &authoritative_review_path,
        "# Code Review Result\n**tampered:** true\n",
    );

    let failure = run_internal_rebuild_evidence_failure_json(repo, state, PLAN_REL);
    assert_eq!(failure["error_class"], "StaleProvenance", "json: {failure}");
    assert!(
        failure["message"].as_str().is_some_and(|message| message.contains(
            "Projection regeneration requires readable authoritative final review artifact"
        ) || message.contains("Projection regeneration refused authoritative final review artifact")
            || message.contains(
                "Projection regeneration could not restore reviewer projection content that matches authoritative final-review bindings."
            )),
        "failure should explain authoritative projection provenance mismatch, got {failure}",
    );
    assert_eq!(
        sha256_hex(
            &serde_json::to_vec(
                &serde_json::from_str::<Value>(
                    &fs::read_to_string(&harness_state_path)
                        .expect("harness state should remain readable after failed rebuild"),
                )
                .expect("harness state should remain valid json after failed rebuild"),
            )
            .expect("harness state json should serialize after failed rebuild"),
        ),
        state_digest_before,
        "tampered authoritative final-review projection regeneration must not mutate authoritative truth"
    );
    assert!(
        latest_branch_artifact_path(&project_artifacts, &branch, "code-review").is_none(),
        "tampered authoritative projection must not regenerate a derived code-review projection"
    );
}

#[test]
fn rebuild_evidence_regenerates_missing_authoritative_final_review_projection_from_machine_state() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-projection-missing-authoritative-content");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_base_branch(repo);
    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);
    let _ = run_runtime_preflight_gate_json(repo, state, PLAN_REL);

    let branch = branch_name(repo);
    let repo_slug = repo_slug(repo);
    let authoritative_review_fingerprint = sha256_hex(
        fs::read(&review_path)
            .expect(
                "source final-review artifact should be readable for authoritative missing fixture",
            )
            .as_slice(),
    );
    let authoritative_review_path = harness_authoritative_artifacts_dir(state, &repo_slug, &branch)
        .join(format!(
            "final-review-{authoritative_review_fingerprint}.md"
        ));
    let harness_state_path = harness_state_path(state, &repo_slug, &branch);
    let harness_before: Value = serde_json::from_str(
        &fs::read_to_string(&harness_state_path).expect("harness state should be readable"),
    )
    .expect("harness state should remain valid json");
    let state_digest_before = sha256_hex(
        &serde_json::to_vec(&harness_before).expect("harness state json should serialize"),
    );
    let final_review_record_id = harness_before["current_final_review_record_id"]
        .as_str()
        .expect("missing authoritative fixture should expose current final-review record id")
        .to_owned();
    let project_artifacts = project_artifact_dir(repo, state);
    let deleted_projection_path =
        latest_branch_artifact_path(&project_artifacts, &branch, "code-review").expect(
            "missing authoritative fixture should expose a readable project code-review projection",
        );
    fs::remove_file(&deleted_projection_path).expect(
        "missing authoritative fixture should allow deleting the derived code-review projection",
    );
    fs::remove_file(&authoritative_review_path)
        .expect("missing authoritative fixture should allow deleting the authoritative final-review projection");

    let gate_before_rebuild = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);
    assert_eq!(
        gate_before_rebuild["allowed"],
        Value::Bool(true),
        "json: {gate_before_rebuild}"
    );

    let rebuild = run_internal_rebuild_evidence_json(repo, state, PLAN_REL);
    assert_eq!(
        rebuild["counts"]["rebuilt"],
        Value::from(0),
        "json: {rebuild}"
    );
    assert!(
        latest_branch_artifact_path(&project_artifacts, &branch, "code-review").is_some(),
        "missing authoritative fixture should restore a readable code-review projection artifact"
    );

    let gate_after_rebuild = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);
    assert_eq!(
        gate_after_rebuild["allowed"],
        Value::Bool(true),
        "json: {gate_after_rebuild}"
    );

    let harness_after: Value =
        serde_json::from_str(&fs::read_to_string(&harness_state_path).expect(
            "harness state should remain readable after missing-authoritative regeneration",
        ))
        .expect("harness state should remain valid json after missing-authoritative regeneration");
    let state_digest_after = sha256_hex(
        &serde_json::to_vec(&harness_after).expect("harness state json should serialize"),
    );
    assert_eq!(
        harness_after["current_final_review_record_id"],
        Value::from(final_review_record_id)
    );
    assert_eq!(
        state_digest_after, state_digest_before,
        "missing authoritative final-review projection regeneration must not mutate authoritative truth"
    );
}

#[test]
fn rebuild_evidence_fails_closed_when_projection_refresh_hits_live_write_authority_conflict() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-projection-write-authority-conflict");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_base_branch(repo);
    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);
    let _ = run_runtime_preflight_gate_json(repo, state, PLAN_REL);

    let branch = branch_name(repo);
    let repo_slug = repo_slug(repo);
    let authoritative_review_fingerprint = sha256_hex(
        fs::read(&review_path)
            .expect("source final-review artifact should be readable for write-authority fixture")
            .as_slice(),
    );
    let authoritative_review_path = harness_authoritative_artifacts_dir(state, &repo_slug, &branch)
        .join(format!(
            "final-review-{authoritative_review_fingerprint}.md"
        ));
    fs::remove_file(&authoritative_review_path).expect(
        "write-authority fixture should allow deleting authoritative final-review projection",
    );
    let project_artifacts = project_artifact_dir(repo, state);
    let deleted_projection_path =
        latest_branch_artifact_path(&project_artifacts, &branch, "code-review").expect(
            "write-authority fixture should expose a readable project code-review projection",
        );
    fs::remove_file(&deleted_projection_path).expect(
        "write-authority fixture should allow deleting the derived project code-review projection",
    );

    let lock_path = harness_state_path(state, &repo_slug, &branch)
        .parent()
        .expect("harness state path should have a parent")
        .join("write-authority.lock");
    let mut holder_cmd = Command::new("sh");
    holder_cmd.args(["-c", "sleep 30"]);
    let mut holder = holder_cmd
        .spawn()
        .expect("live write-authority fixture process should spawn");
    write_file(&lock_path, &format!("pid={}\n", holder.id()));

    let failure = run_internal_rebuild_evidence_failure_json(repo, state, PLAN_REL);
    let _ = holder.kill();
    let _ = holder.wait();

    assert_eq!(
        failure["error_class"], "ConcurrentWriterConflict",
        "json: {failure}"
    );
    assert!(
        failure["message"].as_str().is_some_and(|message| message
            .contains("Another runtime writer currently holds authoritative mutation authority.")),
        "failure should report the live write-authority conflict, got {failure}",
    );
    assert!(
        latest_branch_artifact_path(&project_artifacts, &branch, "code-review").is_none(),
        "live write-authority conflicts must not regenerate derived projections"
    );
}

#[test]
fn gate_finish_rejects_current_final_review_without_authoritative_fingerprint_binding() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-missing-authoritative-fingerprint");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);
    set_current_history_record_field(
        repo,
        state,
        "current_final_review_record_id",
        "final_review_record_history",
        "final_review_fingerprint",
        Value::Null,
    );

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);

    assert_eq!(gate["allowed"], false, "{}", pretty_json(&gate));
    assert_eq!(gate["failure_class"], "MalformedExecutionState");
    assert!(
        gate["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "review_artifact_malformed")),
        "gate-finish should report review_artifact_malformed for missing authoritative fingerprint binding, got {}",
        pretty_json(&gate)
    );
}

#[test]
fn gate_finish_rejects_current_final_review_without_strategy_checkpoint_binding() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-missing-strategy-checkpoint-binding");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);
    merge_harness_state_payload(
        repo,
        state,
        &json!({
            "last_strategy_checkpoint_fingerprint": Value::Null,
        }),
    );

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);

    assert_eq!(gate["allowed"], false, "{}", pretty_json(&gate));
    assert_eq!(gate["failure_class"], "MalformedExecutionState");
    assert!(
        gate["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "review_artifact_malformed")),
        "gate-finish should report review_artifact_malformed when authoritative strategy checkpoint binding is missing, got {}",
        pretty_json(&gate)
    );
}

#[test]
fn gate_finish_accepts_fresh_non_browser_review_chain() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-pass");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);

    assert_eq!(
        gate["allowed"],
        true,
        "fresh late-stage artifacts should allow finish, got {}",
        pretty_json(&gate)
    );
}

#[test]
fn gate_finish_accepts_review_when_only_receipt_provenance_text_is_mutated() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-missing-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let review_source = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        review_source.replace(
            "**Reviewer Provenance:** dedicated-independent",
            "**Reviewer Provenance:** implementation-context",
        ),
    )
    .expect("review artifact should be writable");
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);

    assert_eq!(gate["allowed"], true, "{}", pretty_json(&gate));
}

#[test]
fn gate_finish_accepts_review_when_receipt_reviewer_source_text_is_mutated() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-non-independent-reviewer-source");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let review_source = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        review_source.replace(
            "**Reviewer Source:** fresh-context-subagent",
            "**Reviewer Source:** implementation-context",
        ),
    )
    .expect("review artifact should be writable");
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);

    assert_eq!(gate["allowed"], true, "{}", pretty_json(&gate));
}

#[test]
fn gate_finish_accepts_review_when_receipt_reviewer_artifact_path_is_missing() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-missing-reviewer-artifact-path");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let receipt = parse_final_review_receipt(&review_path);
    let reviewer_artifact_path = receipt
        .reviewer_artifact_path
        .expect("fixture review artifact should include reviewer artifact path");
    let review_source = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        review_source.replace(
            &format!("**Reviewer Artifact Path:** `{reviewer_artifact_path}`\n"),
            "",
        ),
    )
    .expect("review artifact should be writable");
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);

    assert_eq!(gate["allowed"], true, "{}", pretty_json(&gate));
}

#[test]
fn gate_finish_accepts_review_when_receipt_reviewer_artifact_path_is_unreadable() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-unreadable-reviewer-artifact-path");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let reviewer_artifact_path = reviewer_artifact_path_from_review(&review_path);
    fs::remove_file(&reviewer_artifact_path).expect("reviewer artifact should remove");
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);

    assert_eq!(gate["allowed"], true, "{}", pretty_json(&gate));
}

#[test]
fn gate_finish_accepts_review_when_receipt_deviation_verdict_text_is_mutated() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-invalid-deviation-verdict");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let review_source = fs::read_to_string(&review_path).expect("review artifact should read");
    fs::write(
        &review_path,
        review_source
            .replace(
                "**Recorded Execution Deviations:** none",
                "**Recorded Execution Deviations:** present",
            )
            .replace(
                "**Deviation Review Verdict:** not_required",
                "**Deviation Review Verdict:** fail",
            ),
    )
    .expect("review artifact should be writable");
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);

    assert_eq!(gate["allowed"], true, "{}", pretty_json(&gate));
}

#[test]
fn gate_finish_accepts_review_when_runtime_records_topology_downgrade_but_authoritative_review_is_current()
 {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-runtime-recorded-deviation");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    let active_contract_fingerprint = write_active_contract_artifact(repo, state);
    write_serial_unit_review_receipt(repo, state, &active_contract_fingerprint);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);
    merge_harness_state_payload(
        repo,
        state,
        &json!({
            "active_contract_path": format!("contract-{active_contract_fingerprint}.md"),
            "active_contract_fingerprint": active_contract_fingerprint
        }),
    );
    write_matching_topology_downgrade_record(repo, state, &base_branch);

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);

    assert_eq!(gate["allowed"], true, "{}", pretty_json(&gate));
}

#[test]
fn gate_finish_ignores_reason_code_deviation_without_matching_downgrade_record() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-reason-code-no-record");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    let active_contract_fingerprint = write_active_contract_artifact(repo, state);
    write_serial_unit_review_receipt(repo, state, &active_contract_fingerprint);
    write_finish_ready_harness_state_with_reason_codes(
        repo,
        state,
        &["recorded_execution_deviation_dependency_mismatch"],
    );
    merge_harness_state_payload(
        repo,
        state,
        &json!({
            "active_contract_path": format!("contract-{active_contract_fingerprint}.md"),
            "active_contract_fingerprint": active_contract_fingerprint
        }),
    );

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);
    assert_eq!(
        gate["allowed"],
        true,
        "gate-finish should ignore reason-code-only deviation hints, got {}",
        pretty_json(&gate)
    );
}

#[test]
fn status_routes_release_readiness_before_final_review_when_release_state_stales() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-fs11-release-precedence");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);
    merge_harness_state_payload(
        repo,
        state,
        &json!({
            "release_docs_state": "missing",
            "current_release_readiness_result": Value::Null,
            "current_release_readiness_summary_hash": Value::Null,
            "current_release_readiness_record_id": Value::Null
        }),
    );

    let status = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should route stale release truth before final review in the release-precedence fixture (compiled CLI contract)",
    );
    assert_eq!(status["phase"], Value::from("document_release_pending"));
    assert_eq!(status["next_action"], Value::from("advance late stage"));
    let phase_detail = status["phase_detail"]
        .as_str()
        .expect("compiled-CLI release-precedence fixture should expose phase_detail");
    assert!(
        matches!(
            phase_detail,
            "branch_closure_recording_required_for_release_readiness"
                | "release_readiness_recording_ready"
        ),
        "compiled-CLI release-precedence fixture must stay on the document-release lane, got {phase_detail}: {status}"
    );
    let expected_command =
        format!("featureforge plan execution advance-late-stage --plan {PLAN_REL}");
    assert_eq!(status["recommended_command"], Value::from(expected_command));
}

#[test]
fn gate_finish_rejects_final_review_release_binding_mismatch() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-fs11-release-binding-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);

    let state_path = harness_state_path(state, &repo_slug(repo), &branch_name(repo));
    let mut payload: Value = serde_json::from_str(
        &fs::read_to_string(&state_path).expect("harness state should be readable"),
    )
    .expect("harness state should be valid json");
    let final_review_record_id = payload["current_final_review_record_id"]
        .as_str()
        .expect("fixture should expose current final-review record id")
        .to_owned();
    payload
        .get_mut("final_review_record_history")
        .and_then(Value::as_object_mut)
        .and_then(|history| history.get_mut(&final_review_record_id))
        .and_then(Value::as_object_mut)
        .expect("fixture final-review record should exist")
        .insert(
            String::from("release_readiness_record_id"),
            Value::from("release-readiness-record-foreign"),
        );
    write_harness_state_payload(repo, state, &payload);

    let gate_finish = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);
    assert_eq!(gate_finish["allowed"], false, "json: {gate_finish}");
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "review_artifact_release_binding_mismatch")),
        "gate-finish should surface explicit final-review->release identity mismatch, got {gate_finish}"
    );
}

#[test]
fn missing_final_review_projection_regenerates_without_truth_mutation() {
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-fs12-projection-regen");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    write_code_review_artifact(repo, state, &base_branch);
    write_release_readiness_artifact(repo, state, &base_branch);
    write_finish_ready_harness_state_with_reason_codes(repo, state, &[]);
    let _ = run_runtime_preflight_gate_json(repo, state, PLAN_REL);
    let harness_state_path = harness_state_path(state, &repo_slug(repo), &branch_name(repo));
    let harness_before: Value = serde_json::from_str(
        &fs::read_to_string(&harness_state_path).expect("harness state should be readable"),
    )
    .expect("harness state should remain valid json");
    let state_digest_before = sha256_hex(
        &serde_json::to_vec(&harness_before).expect("harness state json should serialize"),
    );
    let final_review_record_id = harness_before["current_final_review_record_id"]
        .as_str()
        .expect("projection-regeneration fixture should expose current final-review record id")
        .to_owned();
    let project_artifacts = project_artifact_dir(repo, state);
    let deleted_projection_path = latest_branch_artifact_path(
        &project_artifacts,
        &branch_name(repo),
        "code-review",
    )
    .expect(
        "projection-regeneration fixture should expose a readable project code-review projection",
    );
    fs::remove_file(&deleted_projection_path)
        .expect("projection-regeneration fixture should allow deleting the derived project projection artifact");

    let rebuild = run_internal_rebuild_evidence_json(repo, state, PLAN_REL);
    assert_eq!(
        rebuild["counts"]["rebuilt"],
        Value::from(0),
        "json: {rebuild}"
    );
    assert!(
        latest_branch_artifact_path(&project_artifacts, &branch_name(repo), "code-review")
            .is_some(),
        "projection regeneration should restore a readable code-review projection artifact"
    );

    let harness_after: Value = serde_json::from_str(
        &fs::read_to_string(&harness_state_path)
            .expect("harness state should remain readable after projection regeneration"),
    )
    .expect("harness state should remain valid json after projection regeneration");
    let state_digest_after = sha256_hex(
        &serde_json::to_vec(&harness_after).expect("harness state json should serialize"),
    );
    assert_eq!(
        harness_after["current_final_review_record_id"],
        Value::from(final_review_record_id)
    );
    assert_eq!(
        state_digest_after, state_digest_before,
        "projection regeneration must not mutate authoritative truth"
    );

    let gate = run_runtime_finish_gate_json(repo, state, PLAN_REL, false);
    assert_eq!(gate["allowed"], Value::Bool(true), "json: {gate}");
}
