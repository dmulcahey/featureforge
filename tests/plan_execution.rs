#[path = "support/bin.rs"]
mod bin_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/projection.rs"]
mod projection_support;
#[path = "support/repo_template.rs"]
mod repo_template_support;
#[path = "support/runtime.rs"]
mod runtime_support;

use bin_support::compiled_featureforge_path;
use featureforge::cli::plan_execution::{ExecutionModeArg, TransferArgs};
use featureforge::contracts::harness::{WorktreeLease, WorktreeLeaseState};
use featureforge::contracts::plan::parse_plan_file;
use featureforge::execution::authority::{
    persist_active_worktree_lease_index, write_authoritative_unit_review_receipt_artifact,
    write_authoritative_worktree_lease_artifact,
};
use featureforge::execution::follow_up::execution_step_repair_target_id;
use featureforge::execution::harness::{ChunkId, ExecutionRunId, RunIdentitySnapshot};
use featureforge::execution::semantic_identity::{
    branch_definition_identity_for_context, task_definition_identity_for_task,
};
use featureforge::execution::state::{
    ExecutionRuntime, TransferRequestMode, current_head_sha as runtime_current_head_sha,
    hash_contract_plan, load_execution_context, normalize_transfer_request,
};
use featureforge::git::{discover_repository, discover_slug_identity};
use featureforge::paths::{branch_storage_key, harness_authoritative_artifact_path};
use runtime_support::execution_runtime;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use tempfile::TempDir;

type HarnessStateFixtureInput<'a> = (
    &'a Path,
    &'a Path,
    &'a str,
    &'a str,
    &'a str,
    &'a [&'a str],
    &'a [&'a str],
    bool,
);

const FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

macro_rules! write_harness_state_fixture {
    ($repo:expr, $state:expr, $harness_phase:expr, $active_contract_path:expr, $active_contract_fingerprint:expr, $required_evaluator_kinds:expr, $pending_evaluator_kinds:expr, $handoff_required:expr $(,)?) => {{
        write_harness_state_fixture_impl((
            $repo,
            $state,
            $harness_phase,
            $active_contract_path,
            $active_contract_fingerprint,
            $required_evaluator_kinds,
            $pending_evaluator_kinds,
            $handoff_required,
        ))
    }};
}

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-17-example-execution-plan.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-03-17-example-execution-plan-design.md";
const EXPECTED_PUBLIC_HARNESS_PHASES: &[&str] = &[
    "implementation_handoff",
    concat!("execution_pre", "flight"),
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

struct ApprovedSingleStepExecutionFixtureTemplate {
    repo_root: PathBuf,
    head_sha: String,
}

static APPROVED_SINGLE_STEP_EXECUTION_FIXTURE_TEMPLATE: OnceLock<
    ApprovedSingleStepExecutionFixtureTemplate,
> = OnceLock::new();

fn run(mut command: Command, context: &str) -> Output {
    command
        .output()
        .unwrap_or_else(|error| panic!("{context} should run: {error}"))
}

fn run_checked(command: Command, context: &str) -> Output {
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

fn parse_json(output: &Output, context: &str) -> Value {
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn parse_failure_json(output: &Output, context: &str) -> Value {
    assert!(
        !output.status.success(),
        "{context} should fail closed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    serde_json::from_slice(payload)
        .unwrap_or_else(|error| panic!("{context} should emit valid failure json: {error}"))
}

fn missing_null_fields(object: &Value, fields: &[&str]) -> Vec<String> {
    fields
        .iter()
        .copied()
        .filter(|field| !object.get(*field).is_some_and(Value::is_null))
        .map(str::to_owned)
        .collect()
}

fn missing_string_fields(object: &Value, fields: &[&str]) -> Vec<String> {
    fields
        .iter()
        .copied()
        .filter(|field| {
            object
                .get(*field)
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        })
        .map(str::to_owned)
        .collect()
}

fn assert_exact_public_harness_phase_set() {
    let spec = include_str!(
        "../docs/archive/featureforge/specs/2026-03-25-featureforge-execution-harness-spec.md"
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

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directory should be creatable");
    }
    fs::write(path, contents).expect("file should be writable");
    if is_execution_harness_state_path(path) {
        sync_or_clear_execution_harness_event_log(path, contents);
    }
}

fn sync_or_clear_execution_harness_event_log(path: &Path, contents: &str) {
    if let Ok(payload) = serde_json::from_str::<Value>(contents)
        && payload.is_object()
    {
        featureforge::execution::event_log::sync_fixture_event_log_for_tests(path, &payload)
            .unwrap_or_else(|error| {
                panic!(
                    "execution harness fixture event log should sync for {}: {}",
                    path.display(),
                    error.message
                )
            });
        let _ = fs::remove_file(path.with_file_name("state.legacy.json"));
        return;
    }
    let _ = fs::remove_file(path.with_file_name("events.jsonl"));
    let _ = fs::remove_file(path.with_file_name("events.lock"));
    let _ = fs::remove_file(path.with_file_name("state.legacy.json"));
}

fn is_execution_harness_state_path(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "state.json")
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "execution-harness")
}

fn init_repo(_name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    repo_template_support::populate_repo_from_template(repo_dir.path());

    (repo_dir, state_dir)
}

fn copy_repo_relative_fixture_file(source_repo: &Path, destination_repo: &Path, rel: &str) {
    let source = source_repo.join(rel);
    assert!(
        source.is_file(),
        "fixture template source file should exist: {}",
        source.display()
    );
    let destination = destination_repo.join(rel);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).expect("fixture destination parent should be creatable");
    }
    fs::copy(&source, &destination).unwrap_or_else(|error| {
        panic!(
            "fixture template copy should succeed from {} to {}: {error}",
            source.display(),
            destination.display()
        )
    });
}

fn write_default_approved_single_step_execution_fixture_uncached(repo: &Path) {
    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    let packet_fingerprint = expected_packet_fingerprint(repo, 1, 1);
    write_single_step_v2_completed_attempt(repo, &packet_fingerprint);
}

fn approved_single_step_execution_fixture_template()
-> &'static ApprovedSingleStepExecutionFixtureTemplate {
    APPROVED_SINGLE_STEP_EXECUTION_FIXTURE_TEMPLATE.get_or_init(|| {
        let template_dir = tempfile::Builder::new()
            .prefix("featureforge-plan-execution-single-step-template-")
            .tempdir()
            .expect("single-step fixture template tempdir should exist");
        let template_repo_root = template_dir.path().to_path_buf();
        repo_template_support::populate_repo_from_template(&template_repo_root);
        write_default_approved_single_step_execution_fixture_uncached(&template_repo_root);
        let template_head_sha = current_head_sha(&template_repo_root);
        std::mem::forget(template_dir);
        ApprovedSingleStepExecutionFixtureTemplate {
            repo_root: template_repo_root,
            head_sha: template_head_sha,
        }
    })
}

fn write_default_approved_single_step_execution_fixture(repo: &Path, state: &Path) {
    let template = approved_single_step_execution_fixture_template();
    if current_head_sha(repo) != template.head_sha {
        write_default_approved_single_step_execution_fixture_uncached(repo);
    } else {
        copy_repo_relative_fixture_file(&template.repo_root, repo, SPEC_REL);
        copy_repo_relative_fixture_file(&template.repo_root, repo, PLAN_REL);
        copy_repo_relative_fixture_file(&template.repo_root, repo, "docs/example-output.md");
        let evidence_rel = evidence_rel_path();
        copy_repo_relative_fixture_file(&template.repo_root, repo, evidence_rel.as_str());
    }
    write_completed_single_step_authority(repo, state);
}

fn write_completed_single_step_authority(repo: &Path, state: &Path) {
    write_completed_task_authority(repo, state, &[1], &["README.md", "docs/example-output.md"]);
}

fn write_completed_task_authority(
    repo: &Path,
    state: &Path,
    tasks: &[u32],
    surface_paths: &[&str],
) {
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let review_summary = "Completed single-step fixture review passed.";
    let verification_summary = "Completed single-step fixture verification passed.";
    let mut current_records = serde_json::Map::new();
    let mut history_records = serde_json::Map::new();
    for task in tasks {
        let task_id = task.to_string();
        let task_scope_key = format!("task-{task}");
        let task_closure_record_id = deterministic_record_id(
            "task-closure",
            &[PLAN_REL, task_id.as_str(), reviewed_state_id.as_str()],
        );
        let task_closure_record = json!({
            "task": task,
            "dispatch_id": format!("fixture-task-{task}-dispatch"),
            "closure_record_id": task_closure_record_id,
            "record_id": task_closure_record_id,
            "record_sequence": task,
            "record_status": "current",
            "source_plan_path": PLAN_REL,
            "source_plan_revision": 1,
            "execution_run_id": "run-single-step-fixture",
            "reviewed_state_id": reviewed_state_id,
            "contract_identity": task_contract_identity(repo, state, PLAN_REL, *task),
            "effective_reviewed_surface_paths": surface_paths,
            "review_result": "pass",
            "review_summary_hash": sha256_hex(review_summary.as_bytes()),
            "verification_result": "pass",
            "verification_summary_hash": sha256_hex(verification_summary.as_bytes()),
            "closure_status": "current",
        });
        current_records.insert(task_scope_key, task_closure_record.clone());
        history_records.insert(task_closure_record_id, task_closure_record);
    }
    write_authoritative_harness_fixture_payload(
        repo,
        state,
        &json!({
            "schema_version": 1,
            "harness_phase": "ready_for_branch_completion",
            "authoritative_sequence": 1,
            "latest_authoritative_sequence": 1,
            "source_plan_path": PLAN_REL,
            "source_plan_revision": 1,
            "run_identity": {
                "execution_run_id": "run-single-step-fixture",
                "source_plan_path": PLAN_REL,
            "source_plan_revision": 1
            },
            "execution_run_id": "run-single-step-fixture",
            "dependency_index_state": "fresh",
            "active_worktree_lease_fingerprints": [],
            "active_worktree_lease_bindings": [],
            "current_task_closure_records": Value::Object(current_records),
            "task_closure_record_history": Value::Object(history_records),
        }),
    );
}

fn write_approved_spec(repo: &Path) {
    write_file(
        &repo.join(SPEC_REL),
        r#"# Example Execution Plan Design

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

## Requirement Index

- [REQ-001][behavior] Core execution setup and validation must stay grounded in canonical execution-state evidence.
- [REQ-002][behavior] Execution helpers must preserve authoritative runtime invariants.
- [REQ-003][behavior] Repair and handoff flows must fail closed on stale or malformed state.
- [VERIFY-001][verification] Runtime coverage must exercise the execution and repair flows through plan execution tests.

## Summary

Fixture spec for plan execution helper regression coverage.
"#,
    );
}

fn write_newer_approved_spec_same_revision_different_path(repo: &Path) {
    write_file(
        &repo.join("docs/featureforge/specs/2026-03-17-example-execution-plan-design-v2.md"),
        r#"# Example Execution Plan Design V2

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

## Requirement Index

- [REQ-001][behavior] Core execution setup and validation must stay grounded in canonical execution-state evidence.
- [REQ-002][behavior] Execution helpers must preserve authoritative runtime invariants.
- [REQ-003][behavior] Repair and handoff flows must fail closed on stale or malformed state.
- [VERIFY-001][verification] Runtime coverage must exercise the execution and repair flows through plan execution tests.

## Summary

Fixture spec representing a newer approved spec path with the same revision.
"#,
    );
}

fn write_plan(repo: &Path, execution_mode: &str) {
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
- REQ-002 -> Task 1
- REQ-003 -> Task 2
- VERIFY-001 -> Task 1, Task 2

## Task 1: Core flow

**Spec Coverage:** REQ-001, REQ-002, VERIFY-001
**Goal:** Core execution setup and validation are tracked with canonical execution-state evidence.

**Context:**
- Spec Coverage: REQ-001, REQ-002, VERIFY-001.

**Constraints:**
- Preserve helper-owned execution-state invariants.
- Keep execution evidence grounded in repo-visible artifacts.

**Done when:**
- Core execution setup and validation are tracked with canonical execution-state evidence.

**Files:**
- Modify: `docs/example-output.md`
- Test: `cargo test --test plan_execution`

- [ ] **Step 1: Prepare workspace for execution**
- [ ] **Step 2: Validate the generated output**

## Task 2: Repair flow

**Spec Coverage:** REQ-003, VERIFY-001
**Goal:** Repair and handoff steps can reopen stale work without losing provenance.

**Context:**
- Spec Coverage: REQ-003, VERIFY-001.

**Constraints:**
- Reuse the same approved plan and evidence path for repairs.
- Keep repair flows fail-closed on stale or malformed state.

**Done when:**
- Repair and handoff steps can reopen stale work without losing provenance.

**Files:**
- Modify: `docs/example-output.md`
- Test: `cargo test --test plan_execution`

- [ ] **Step 1: Repair an invalidated prior step**
- [ ] **Step 2: Finalize the execution handoff**
"#
        ),
    );
}

fn write_second_approved_plan_same_spec(repo: &Path, execution_mode: &str) {
    write_file(
        &repo.join("docs/featureforge/plans/2026-03-18-example-execution-plan-v2.md"),
        &format!(
            r#"# Example Execution Plan V2

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** {execution_mode}
**Source Spec:** `{SPEC_REL}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review
**QA Requirement:** not-required

## Requirement Coverage Matrix

- REQ-001 -> Task 1

## Task 1: Alternate flow

**Spec Coverage:** REQ-001
**Goal:** Alternate approved plan candidate for ambiguity coverage.

**Context:**
- Spec Coverage: REQ-001.

**Constraints:**
- Keep the fixture minimal.

**Done when:**
- Alternate approved plan candidate for ambiguity coverage.

**Files:**
- Test: `tests/plan_execution.rs`

- [ ] **Step 1: Preserve ambiguity coverage**
"#,
        ),
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

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- VERIFY-001 -> Task 1

## Task 1: Single-step fixture

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Single-step fixtures isolate completion and review behavior.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep the fixture to one step.

**Done when:**
- Single-step fixtures isolate completion and review behavior.

**Files:**
- Modify: `docs/example-output.md`
- Test: `cargo test --test plan_execution`

- [ ] **Step 1: Complete the single-step fixture**
"#
        ),
    );
}

fn set_plan_qa_requirement(repo: &Path, qa_requirement: &str) {
    let plan_path = repo.join(PLAN_REL);
    let source = fs::read_to_string(&plan_path).expect("plan fixture should be readable");
    let updated = if let Some(current_line) = source
        .lines()
        .find(|line| line.starts_with("**QA Requirement:**"))
    {
        source.replace(
            current_line,
            &format!("**QA Requirement:** {qa_requirement}"),
        )
    } else {
        source.replace(
            "**Last Reviewed By:** plan-eng-review\n",
            &format!(
                "**Last Reviewed By:** plan-eng-review\n**QA Requirement:** {qa_requirement}\n"
            ),
        )
    };
    assert_ne!(
        source, updated,
        "plan QA requirement rewrite should change the fixture contents"
    );
    fs::write(&plan_path, updated).expect("plan fixture should be writable");
}

fn mark_all_plan_steps_checked(repo: &Path) {
    let path = repo.join(PLAN_REL);
    let source = fs::read_to_string(&path).expect("plan should be readable");
    fs::write(path, source.replace("- [ ] **Step", "- [x] **Step"))
        .expect("plan should be writable");
}

fn add_fenced_step_details(repo: &Path) {
    let path = repo.join(PLAN_REL);
    let source = fs::read_to_string(&path).expect("plan should be readable");
    let updated = source
        .replacen(
            "- [ ] **Step 1: Prepare workspace for execution**",
            "- [ ] **Step 1: Prepare workspace for execution**\n```text\nstatus detail fixture\n```",
            1,
        )
        .replacen(
            "- [ ] **Step 2: Validate the generated output**",
            "- [ ] **Step 2: Validate the generated output**\n```text\nverification detail fixture\n```",
            1,
        )
        .replacen(
            "- [ ] **Step 1: Repair an invalidated prior step**",
            "- [ ] **Step 1: Repair an invalidated prior step**\n```text\nrepair detail fixture\n```",
            1,
        )
        .replacen(
            "- [ ] **Step 2: Finalize the execution handoff**",
            "- [ ] **Step 2: Finalize the execution handoff**\n```text\nhandoff detail fixture\n```",
            1,
        );
    fs::write(path, updated).expect("plan should be writable");
}

fn sha256_hex(contents: &[u8]) -> String {
    let digest = Sha256::digest(contents);
    format!("{digest:x}")
}

fn evidence_rel_path() -> String {
    "docs/featureforge/execution-evidence/2026-03-17-example-execution-plan-r1-evidence.md".into()
}

fn execution_contract_plan_hash(repo: &Path) -> String {
    let source = fs::read_to_string(repo.join(PLAN_REL)).expect("plan should be readable");
    hash_contract_plan(&source)
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
    let spec_fingerprint = sha256_hex(
        &fs::read(repo.join(SPEC_REL)).expect("spec should be readable for packet fingerprint"),
    );
    let payload = format!(
        "plan_path={PLAN_REL}\nplan_revision=1\ntask_definition_identity={task_definition_identity}\nsource_spec_path={SPEC_REL}\nsource_spec_revision=1\nsource_spec_fingerprint={spec_fingerprint}\ntask_number={task}\nstep_number={step}\n"
    );
    sha256_hex(payload.as_bytes())
}

fn write_single_step_v2_completed_attempt(repo: &Path, packet_fingerprint: &str) {
    let evidence_path = repo.join(evidence_rel_path());
    let plan_fingerprint = execution_contract_plan_hash(repo);
    let spec_fingerprint =
        sha256_hex(&fs::read(repo.join(SPEC_REL)).expect("spec should be readable for evidence"));
    write_file(&repo.join("docs/example-output.md"), "verified output\n");
    let file_digest = sha256_hex(
        &fs::read(repo.join("docs/example-output.md")).expect("output should be readable"),
    );

    let head_sha = current_head_sha(repo);
    let base_sha = head_sha.clone();
    write_file(
        &evidence_path,
        &format!(
            "# Execution Evidence: 2026-03-17-example-execution-plan\n\n**Plan Path:** {PLAN_REL}\n**Plan Revision:** 1\n**Plan Fingerprint:** {plan_fingerprint}\n**Source Spec Path:** {SPEC_REL}\n**Source Spec Revision:** 1\n**Source Spec Fingerprint:** {spec_fingerprint}\n\n## Step Evidence\n\n### Task 1 Step 1\n#### Attempt 1\n**Status:** Completed\n**Recorded At:** 2026-03-17T14:22:31Z\n**Execution Source:** featureforge:executing-plans\n**Task Number:** 1\n**Step Number:** 1\n**Packet Fingerprint:** {packet_fingerprint}\n**Head SHA:** {head_sha}\n**Base SHA:** {base_sha}\n**Claim:** Prepared the workspace for execution.\n**Files Proven:**\n- docs/example-output.md | sha256:{file_digest}\n**Verification Summary:** Manual inspection only: Verified by fixture setup.\n**Invalidation Reason:** N/A\n"
        ),
    );
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

fn deterministic_record_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    for part in parts {
        hasher.update(b"\n");
        hasher.update(part.as_bytes());
    }
    let digest = format!("{:x}", hasher.finalize());
    format!("{prefix}-{}", &digest[..16])
}

fn branch_contract_identity(repo: &Path, state_dir: &Path, plan_rel: &str) -> String {
    let runtime = execution_runtime(repo, state_dir);
    let context = load_execution_context(&runtime, Path::new(plan_rel))
        .expect("plan_execution semantic branch identity fixture should load execution context");
    branch_definition_identity_for_context(&context)
}

fn task_contract_identity(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    task_number: u32,
) -> String {
    let runtime = execution_runtime(repo, state_dir);
    let context = load_execution_context(&runtime, Path::new(plan_rel))
        .expect("plan_execution semantic task identity fixture should load execution context");
    task_definition_identity_for_task(&context, task_number)
        .expect("plan_execution semantic task identity fixture should compute")
        .expect("plan_execution semantic task identity fixture should exist")
}

fn branch_name(repo: &Path) -> String {
    discover_slug_identity(repo).branch_name
}

fn normalize_identifier(value: &str) -> String {
    branch_storage_key(value)
}

fn repo_slug(repo: &Path) -> String {
    discover_slug_identity(repo).repo_slug
}

fn project_artifact_dir(repo: &Path, state: &Path) -> PathBuf {
    state.join("projects").join(repo_slug(repo))
}

fn harness_branch_dir(repo: &Path, state: &Path) -> PathBuf {
    let branch = branch_name(repo);
    let safe_branch = normalize_identifier(&branch);
    state
        .join("projects")
        .join(repo_slug(repo))
        .join("branches")
        .join(safe_branch)
}

fn harness_state_file_path(repo: &Path, state: &Path) -> PathBuf {
    harness_branch_dir(repo, state)
        .join("execution-harness")
        .join("state.json")
}

fn reduced_authoritative_harness_state_for_path(state_path: &Path) -> Option<Value> {
    featureforge::execution::event_log::load_reduced_authoritative_state_for_tests(state_path)
        .unwrap_or_else(|error| {
            panic!(
                "event-authoritative harness state should be reducible for {}: {}",
                state_path.display(),
                error.message
            )
        })
}

fn authoritative_harness_state_for_merge(state_path: &Path) -> Option<Value> {
    reduced_authoritative_harness_state_for_path(state_path).or_else(|| {
        state_path.is_file().then(|| {
            serde_json::from_str(
                &fs::read_to_string(state_path)
                    .expect("existing harness state should be readable for merge"),
            )
            .expect("existing harness state should be valid json for merge")
        })
    })
}

fn read_authoritative_harness_state(repo: &Path, state: &Path, purpose: &str) -> Value {
    let state_path = harness_state_file_path(repo, state);
    authoritative_harness_state_for_merge(&state_path).unwrap_or_else(|| {
        panic!(
            "harness state should be present for {purpose} at {}",
            state_path.display()
        )
    })
}

fn write_harness_state_payload(repo: &Path, state: &Path, payload: &Value) {
    let state_path = harness_state_file_path(repo, state);
    let mut merged = if let Some(existing) = authoritative_harness_state_for_merge(&state_path) {
        match (existing, payload.clone()) {
            (Value::Object(mut existing), Value::Object(patch)) => {
                for (key, value) in patch {
                    existing.insert(key, value);
                }
                Value::Object(existing)
            }
            (_, replacement) => replacement,
        }
    } else {
        payload.clone()
    };
    if let Value::Object(object) = &mut merged {
        object
            .entry("strategy_state".to_string())
            .or_insert_with(|| Value::from("ready"));
        object
            .entry("strategy_checkpoint_kind".to_string())
            .or_insert_with(|| Value::from("initial_dispatch"));
        object
            .entry("last_strategy_checkpoint_fingerprint".to_string())
            .or_insert_with(|| Value::from(FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT));
        object
            .entry("strategy_reset_required".to_string())
            .or_insert_with(|| Value::Bool(false));
    }
    write_file(
        &state_path,
        &serde_json::to_string_pretty(&merged).expect("harness state payload should serialize"),
    );
    let events_path = state_path.with_file_name("events.jsonl");
    let legacy_backup_path = state_path.with_file_name("state.legacy.json");
    let _ = fs::remove_file(events_path);
    let _ = fs::remove_file(legacy_backup_path);
}

fn write_authoritative_harness_fixture_payload(repo: &Path, state: &Path, payload: &Value) {
    write_harness_state_payload(repo, state, payload);
    let state_path = harness_state_file_path(repo, state);
    let state_json: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("authoritative harness fixture state should be readable for event sync"),
    )
    .expect("authoritative harness fixture state should remain valid json for event sync");
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(&state_path, &state_json)
        .expect("authoritative harness fixture should sync typed event authority");
}

fn write_non_authoritative_harness_projection_payload(repo: &Path, state: &Path, payload: &Value) {
    let state_path = harness_state_file_path(repo, state);
    let merged = if state_path.is_file() {
        let existing: Value = serde_json::from_str(
            &fs::read_to_string(&state_path)
                .expect("existing harness state should be readable for projection merge"),
        )
        .expect("existing harness state should be valid json for projection merge");
        match (existing, payload.clone()) {
            (Value::Object(mut existing), Value::Object(patch)) => {
                for (key, value) in patch {
                    existing.insert(key, value);
                }
                Value::Object(existing)
            }
            (_, replacement) => replacement,
        }
    } else {
        payload.clone()
    };
    if let Some(parent) = state_path.parent() {
        fs::create_dir_all(parent).expect("harness projection parent should be creatable");
    }
    fs::write(
        &state_path,
        serde_json::to_string_pretty(&merged).expect("harness projection payload should serialize"),
    )
    .expect("harness projection payload should be writable without touching event authority");
}

fn public_repair_targets_include_persisted_follow_up(
    status: &Value,
    follow_up: &str,
    task: Option<u64>,
    step: Option<u64>,
    source_record_id: Option<&str>,
) -> bool {
    let reason_code = format!("persisted_review_state_repair_follow_up:{follow_up}");
    status["public_repair_targets"]
        .as_array()
        .is_some_and(|targets| {
            targets.iter().any(|target| {
                target["reason_code"].as_str() == Some(reason_code.as_str())
                    && task.is_none_or(|expected| target["task"].as_u64() == Some(expected))
                    && step.is_none_or(|expected| target["step"].as_u64() == Some(expected))
                    && source_record_id.is_none_or(|expected| {
                        target["source_record_id"].as_str() == Some(expected)
                    })
            })
        })
}

fn public_repair_targets_include_any_persisted_follow_up(status: &Value) -> bool {
    status["public_repair_targets"]
        .as_array()
        .is_some_and(|targets| {
            targets.iter().any(|target| {
                target["reason_code"].as_str().is_some_and(|reason| {
                    reason.starts_with("persisted_review_state_repair_follow_up")
                })
            })
        })
}

fn write_initial_dispatch_harness_state(repo: &Path, state: &Path, execution_run_id: &str) {
    write_harness_state_payload(
        repo,
        state,
        &json!({
            "schema_version": 1,
            "run_identity": {
                "execution_run_id": execution_run_id,
                "source_plan_path": PLAN_REL,
                "source_plan_revision": 1
            },
            "last_strategy_checkpoint_fingerprint": FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT,
            "strategy_state": "executing",
            "strategy_checkpoint_kind": "initial_dispatch"
        }),
    );
}

fn bind_explicit_reopen_repair_target(repo: &Path, state: &Path, task: u32, step: u32) {
    let _ = run_rust_json(
        repo,
        state,
        &[
            "materialize-projections",
            "--plan",
            PLAN_REL,
            "--scope",
            "late-stage",
        ],
        "materialize event-authoritative state before binding explicit reopen repair target",
    );
    write_harness_state_payload(
        repo,
        state,
        &json!({
            "explicit_reopen_repair_targets": [{
                "target_task": task,
                "target_step": step,
                "target_record_id": execution_step_repair_target_id(task, step),
                "created_sequence": 1,
                "expires_on_plan_fingerprint_change": true
            }],
            "review_state_repair_follow_up_record": null,
            "review_state_repair_follow_up": null,
            "review_state_repair_follow_up_task": null,
            "review_state_repair_follow_up_step": null,
            "review_state_repair_follow_up_closure_record_id": null,
        }),
    );
}

fn current_authoritative_run_identity(repo: &Path, state: &Path) -> (String, String) {
    let state_json = read_authoritative_harness_state(repo, state, "current run identity");
    let run_identity = state_json
        .get("run_identity")
        .and_then(Value::as_object)
        .expect("authoritative harness state should expose run_identity");
    let execution_run_id = run_identity
        .get("execution_run_id")
        .and_then(Value::as_str)
        .expect("authoritative harness state should expose execution_run_id")
        .to_owned();
    let chunk_id = state_json
        .get("chunk_id")
        .and_then(Value::as_str)
        .expect("authoritative harness state should expose chunk_id")
        .to_owned();
    (execution_run_id, chunk_id)
}

fn current_worktree_lease_execution_context_key(
    execution_run_id: &str,
    execution_unit_id: &str,
    source_plan_path: &str,
    source_plan_revision: u32,
    authoritative_integration_branch: &str,
    reviewed_checkpoint_commit_sha: &str,
) -> String {
    sha256_hex(
        format!(
            "run={execution_run_id}\nunit={execution_unit_id}\nplan={source_plan_path}\nplan_revision={source_plan_revision}\nbranch={authoritative_integration_branch}\nreviewed_checkpoint={reviewed_checkpoint_commit_sha}\n"
        )
        .as_bytes(),
    )
}

fn approved_unit_contract_fingerprint_for_review(
    active_contract_fingerprint: &str,
    approved_task_packet_fingerprint: &str,
    execution_unit_id: &str,
) -> String {
    sha256_hex(
        format!(
            "approved-unit-contract:{active_contract_fingerprint}:{approved_task_packet_fingerprint}:{execution_unit_id}"
        )
            .as_bytes(),
    )
}

fn write_harness_state_fixture_impl(input: HarnessStateFixtureInput<'_>) {
    let (
        repo,
        state,
        harness_phase,
        active_contract_path,
        active_contract_fingerprint,
        required_evaluator_kinds,
        pending_evaluator_kinds,
        handoff_required,
    ) = input;
    let source_contract = fs::read_to_string(repo.join(active_contract_path))
        .expect("harness-state fixture source contract should be readable");
    let authoritative_contract_file = format!("contract-{active_contract_fingerprint}.md");
    let authoritative_contract_path = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch_name(repo),
        &authoritative_contract_file,
    );
    write_file(&authoritative_contract_path, &source_contract);

    let payload = json!({
        "schema_version": 1,
        "harness_phase": harness_phase,
        "latest_authoritative_sequence": 0,
        "active_contract_path": authoritative_contract_file,
        "active_contract_fingerprint": active_contract_fingerprint,
        "required_evaluator_kinds": required_evaluator_kinds,
        "completed_evaluator_kinds": [],
        "pending_evaluator_kinds": pending_evaluator_kinds,
        "non_passing_evaluator_kinds": [],
        "aggregate_evaluation_state": "pending",
        "current_chunk_retry_count": 0,
        "current_chunk_retry_budget": 1,
        "current_chunk_pivot_threshold": 1,
        "handoff_required": handoff_required,
        "open_failed_criteria": []
    });
    write_harness_state_payload(repo, state, &payload);
}

fn write_execution_contract_artifact(
    repo: &Path,
    artifact_rel: &str,
    fingerprint_override: Option<&str>,
) -> String {
    write_execution_contract_artifact_custom(
        repo,
        artifact_rel,
        17,
        "[]",
        1,
        1,
        fingerprint_override,
    )
}

fn write_execution_contract_artifact_custom(
    repo: &Path,
    artifact_rel: &str,
    authoritative_sequence: u64,
    evidence_requirements_section: &str,
    retry_budget: u32,
    pivot_threshold: u32,
    fingerprint_override: Option<&str>,
) -> String {
    let plan_fingerprint = execution_contract_plan_hash(repo);
    let spec_fingerprint =
        sha256_hex(&fs::read(repo.join(SPEC_REL)).expect("spec should be readable"));
    let packet_fingerprint = expected_packet_fingerprint(repo, 1, 1);
    let template = format!(
        r#"# Execution Contract

**Contract Version:** 1
**Authoritative Sequence:** {authoritative_sequence}
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
{evidence_requirements_section}

**Retry Budget:** {retry_budget}
**Pivot Threshold:** {pivot_threshold}
**Reset Policy:** none
**Generated By:** featureforge:executing-plans
**Generated At:** 2026-03-25T12:00:00Z
**Contract Fingerprint:** __CONTRACT_FINGERPRINT__
"#
    );
    let canonical_fingerprint =
        sha256_hex(template.replace("__CONTRACT_FINGERPRINT__", "").as_bytes());
    let declared_fingerprint = fingerprint_override.unwrap_or(canonical_fingerprint.as_str());
    write_file(
        &repo.join(artifact_rel),
        &template.replace("__CONTRACT_FINGERPRINT__", declared_fingerprint),
    );
    canonical_fingerprint
}

fn write_test_plan_artifact(repo: &Path, state: &Path, browser_required: &str) -> PathBuf {
    let branch = branch_name(repo);
    let safe_branch = normalize_identifier(&branch);
    let head_sha = current_head_sha(repo);
    let artifact_path = project_artifact_dir(repo, state)
        .join(format!("tester-{safe_branch}-test-plan-20260322-170500.md"));
    let source = format!(
        "# Test Plan\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {head_sha}\n**Browser QA Required:** {browser_required}\n**Generated By:** featureforge:plan-eng-review\n**Generated At:** 2026-03-22T17:05:00Z\n\n## Affected Pages / Routes\n- /runtime-hardening - verify helper-backed finish gating\n\n## Key Interactions\n- finish-gate handoff on /runtime-hardening\n\n## Edge Cases\n- stale or missing release-readiness evidence\n\n## Critical Paths\n- approved-plan finish handoff stays blocked until QA and release artifacts are fresh\n",
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

fn write_qa_result_artifact(repo: &Path, state: &Path, test_plan_path: &Path) -> PathBuf {
    let branch = branch_name(repo);
    let safe_branch = normalize_identifier(&branch);
    let head_sha = current_head_sha(repo);
    let artifact_path = project_artifact_dir(repo, state).join(format!(
        "tester-{safe_branch}-test-outcome-20260322-170900.md"
    ));
    write_file(
        &artifact_path,
        &format!(
            "# QA Result\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Source Test Plan:** `{}`\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {head_sha}\n**Result:** pass\n**Generated By:** featureforge/qa\n**Generated At:** 2026-03-22T17:09:00Z\n\n## Summary\n- Browser QA artifact fixture for {} coverage.\n",
            test_plan_path.display(),
            repo_slug(repo),
            concat!("gate", "-finish")
        ),
    );
    artifact_path
}

fn write_code_review_artifact(repo: &Path, state: &Path, base_branch: &str) -> PathBuf {
    let branch = branch_name(repo);
    let safe_branch = normalize_identifier(&branch);
    let head_sha = current_head_sha(repo);
    let reviewer_artifact_path = project_artifact_dir(repo, state).join(format!(
        "tester-{safe_branch}-independent-review-20260322-170950.md"
    ));
    let reviewer_artifact_source = format!(
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-22T17:09:50Z\n\n## Summary\n- dedicated independent reviewer artifact fixture.\n",
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
            "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT}\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** {reviewer_artifact_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-22T17:11:00Z\n\n## Summary\n- Final whole-diff review artifact fixture for finish-gate coverage.\n",
            reviewer_artifact_path.display(),
            repo_slug(repo)
        ),
    );
    artifact_path
}

fn write_release_readiness_artifact(repo: &Path, state: &Path, base_branch: &str) -> PathBuf {
    let branch = branch_name(repo);
    let safe_branch = normalize_identifier(&branch);
    let head_sha = current_head_sha(repo);
    let artifact_path = project_artifact_dir(repo, state).join(format!(
        "tester-{safe_branch}-release-readiness-20260322-171500.md"
    ));
    write_file(
        &artifact_path,
        &format!(
            "# Release Readiness Result\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Result:** pass\n**Generated By:** featureforge:document-release\n**Generated At:** 2026-03-22T17:15:00Z\n\n## Summary\n- Release-readiness artifact fixture for finish-gate coverage.\n",
            repo_slug(repo)
        ),
    );
    artifact_path
}

fn write_serial_unit_review_receipt_artifact(
    repo: &Path,
    state: &Path,
    execution_run_id: &str,
    task_number: u32,
    step_number: u32,
    reviewed_checkpoint_commit_sha: &str,
) -> (PathBuf, String) {
    let branch = branch_name(repo);
    let reviewed_worktree = fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
    let execution_unit_id = format!("task-{task_number}-step-{step_number}");
    let approved_task_packet_fingerprint =
        expected_packet_fingerprint(repo, task_number, step_number);
    let state_json: Value = serde_json::from_str(
        &fs::read_to_string(harness_state_file_path(repo, state))
            .expect("harness state should be readable for serial unit-review receipt"),
    )
    .expect("harness state should be valid json for serial unit-review receipt");
    let active_contract_fingerprint = state_json
        .get("active_contract_fingerprint")
        .and_then(Value::as_str)
        .expect("active contract fingerprint should be present for serial unit-review receipt");
    let approved_unit_contract_fingerprint = approved_unit_contract_fingerprint_for_review(
        active_contract_fingerprint,
        &approved_task_packet_fingerprint,
        &execution_unit_id,
    );
    let execution_context_key = current_worktree_lease_execution_context_key(
        execution_run_id,
        &execution_unit_id,
        PLAN_REL,
        1,
        &branch,
        reviewed_checkpoint_commit_sha,
    );
    let lease_fingerprint = sha256_hex(
        format!(
            "serial-unit-review:{execution_run_id}:{execution_unit_id}:{execution_context_key}:{reviewed_checkpoint_commit_sha}:{approved_task_packet_fingerprint}:{approved_unit_contract_fingerprint}"
        )
        .as_bytes(),
    );
    let reconcile_result_proof_fingerprint =
        commit_object_fingerprint(repo, reviewed_checkpoint_commit_sha);
    let receipt_path = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("unit-review-{execution_run_id}-{execution_unit_id}.md"),
    );
    let unsigned_source = format!(
        "# Unit Review Result\n**Review Stage:** featureforge:unit-review\n**Reviewer Provenance:** dedicated-independent\n**Strategy Checkpoint Fingerprint:** {FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT}\n**Source Plan:** {PLAN_REL}\n**Source Plan Revision:** 1\n**Execution Run ID:** {execution_run_id}\n**Execution Unit ID:** {execution_unit_id}\n**Lease Fingerprint:** {lease_fingerprint}\n**Execution Context Key:** {execution_context_key}\n**Approved Task Packet Fingerprint:** {approved_task_packet_fingerprint}\n**Approved Unit Contract Fingerprint:** {approved_unit_contract_fingerprint}\n**Reconciled Result SHA:** {reviewed_checkpoint_commit_sha}\n**Reconcile Result Proof Fingerprint:** {reconcile_result_proof_fingerprint}\n**Reconcile Mode:** identity_preserving\n**Reviewed Worktree:** {}\n**Reviewed Checkpoint SHA:** {reviewed_checkpoint_commit_sha}\n**Result:** pass\n**Generated By:** featureforge:unit-review\n**Generated At:** 2026-03-27T12:00:00Z\n",
        reviewed_worktree.display()
    );
    let receipt_fingerprint = canonical_unit_review_receipt_fingerprint(&unsigned_source);
    let source = format!(
        "# Unit Review Result\n**Receipt Fingerprint:** {receipt_fingerprint}\n{}",
        unsigned_source.trim_start_matches("# Unit Review Result\n")
    );
    write_file(&receipt_path, &source);
    (receipt_path, receipt_fingerprint)
}

fn begin_task_1_step_1_for_mutation_oracle_fixture(repo: &Path, state: &Path) -> Value {
    write_approved_spec(repo);
    write_plan(repo, "none");
    checkout_execution_fixture_branch(repo);
    let status_before_begin = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status before task 1 step 1 mutation oracle fixture begin",
    );
    run_rust_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status should expose fingerprint before mutation oracle begin"),
        ],
        "begin task 1 step 1 for mutation oracle fixture",
    )
}

fn complete_task_1_step_1_for_mutation_oracle_fixture(repo: &Path, state: &Path) -> Value {
    let begin = begin_task_1_step_1_for_mutation_oracle_fixture(repo, state);
    run_rust_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 1 for mutation oracle fixture.",
            "--file",
            "README.md",
            "--manual-verify-summary",
            "Fixture verification for mutation oracle coverage.",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"]
                .as_str()
                .expect("begin should expose fingerprint for mutation oracle complete"),
        ],
        "complete task 1 step 1 for mutation oracle fixture",
    )
}

#[test]
fn mutation_oracle_rejects_non_exact_reopen_even_when_step_is_completed() {
    let (repo_dir, state_dir) = init_repo("mutation-oracle-rejects-non-exact-reopen");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let complete = complete_task_1_step_1_for_mutation_oracle_fixture(repo, state);
    let failure = parse_failure_json(
        &run_rust(
            repo,
            state,
            &[
                "reopen",
                "--plan",
                PLAN_REL,
                "--task",
                "1",
                "--step",
                "1",
                "--source",
                "featureforge:executing-plans",
                "--reason",
                "Attempt to reopen a completed but non-routed step.",
                "--expect-execution-fingerprint",
                complete["execution_fingerprint"]
                    .as_str()
                    .expect("complete should expose fingerprint for rejected reopen"),
            ],
            "non-exact completed-step reopen should fail closed",
        ),
        "non-exact completed-step reopen should fail closed",
    );
    let message = failure["message"]
        .as_str()
        .expect("failure should expose a message");
    assert!(message.contains("reopen failed closed"), "got {failure}");
    assert!(
        message.contains("reason_code=mutation_not_route_authorized"),
        "got {failure}"
    );
    assert!(
        message.contains("featureforge plan execution begin --plan"),
        "got {failure}"
    );
}

#[test]
fn mutation_oracle_rejects_non_exact_transfer_even_when_active_step_exists() {
    let (repo_dir, state_dir) = init_repo("mutation-oracle-rejects-non-exact-transfer");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let begin = begin_task_1_step_1_for_mutation_oracle_fixture(repo, state);
    let failure = parse_failure_json(
        &run_rust(
            repo,
            state,
            &[
                "transfer",
                "--plan",
                PLAN_REL,
                "--repair-task",
                "1",
                "--repair-step",
                "1",
                "--source",
                "featureforge:executing-plans",
                "--reason",
                "Attempt to transfer without transfer routing.",
                "--expect-execution-fingerprint",
                begin["execution_fingerprint"]
                    .as_str()
                    .expect("begin should expose fingerprint for rejected transfer"),
            ],
            "non-exact active-step transfer should fail closed",
        ),
        "non-exact active-step transfer should fail closed",
    );
    let message = failure["message"]
        .as_str()
        .expect("failure should expose a message");
    assert!(message.contains("transfer failed closed"), "got {failure}");
    assert!(
        message.contains("reason_code=mutation_not_route_authorized"),
        "got {failure}"
    );
}

fn commit_object_fingerprint(repo: &Path, commit_sha: &str) -> String {
    let repository =
        discover_repository(repo).expect("commit fingerprint helper should discover repo");
    let object_id = gix::hash::ObjectId::from_hex(commit_sha.as_bytes())
        .expect("commit fingerprint helper should parse commit object id");
    let commit = repository
        .find_commit(object_id)
        .expect("commit fingerprint helper should load commit object");
    sha256_hex(commit.data.as_slice())
}

fn canonical_unit_review_receipt_fingerprint(source: &str) -> String {
    let filtered = source
        .lines()
        .filter(|line| !line.trim().starts_with("**Receipt Fingerprint:**"))
        .collect::<Vec<_>>()
        .join("\n");
    sha256_hex(filtered.as_bytes())
}

fn advance_repo_head_empty_commit(repo: &Path, message: &str) {
    let mut git_commit = Command::new("git");
    git_commit
        .args(["commit", "--allow-empty", "-m", message])
        .current_dir(repo);
    run_checked(git_commit, "git commit advance head empty");
}

fn commit_repo_changes(repo: &Path, message: &str) {
    let mut git_add = Command::new("git");
    git_add.args(["add", "-A"]).current_dir(repo);
    run_checked(git_add, "git add repo changes");

    let mut git_commit = Command::new("git");
    git_commit.args(["commit", "-m", message]).current_dir(repo);
    run_checked(git_commit, "git commit repo changes");
}

fn canonical_worktree_lease_fingerprint(lease: &Value) -> String {
    let mut lease = lease.clone();
    let lease_object = lease
        .as_object_mut()
        .expect("worktree lease artifact should be a JSON object");
    lease_object.remove("lease_fingerprint");
    sha256_hex(
        &serde_json::to_vec(&lease).expect("lease artifact should be serializable for fingerprint"),
    )
}

fn replace_in_file(path: &Path, from: &str, to: &str) {
    let source = fs::read_to_string(path).expect("fixture file should be readable for mutation");
    let updated = source.replace(from, to);
    assert_ne!(
        source,
        updated,
        "fixture mutation should change the file contents for {}",
        path.display()
    );
    fs::write(path, updated).expect("fixture file should be writable for mutation");
}

fn rewrite_source_test_plan_header(source: &str, source_test_plan: &Path) -> String {
    let replacement = format!("**Source Test Plan:** `{}`", source_test_plan.display());
    let mut replaced = false;
    let rewritten = source
        .lines()
        .map(|line| {
            if line.trim().starts_with("**Source Test Plan:**") {
                replaced = true;
                replacement.clone()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        replaced,
        "QA artifact should include a Source Test Plan header"
    );
    format!("{rewritten}\n")
}

fn prepare_finished_single_step_finish_gate_fixture(
    repo: &Path,
    state: &Path,
    browser_required: &str,
    include_qa: bool,
    base_branch: &str,
) -> (PathBuf, Option<PathBuf>, PathBuf, PathBuf) {
    prepare_finished_single_step_finish_gate_fixture_with_plan_qa_requirement(
        repo,
        state,
        browser_required,
        include_qa,
        base_branch,
        if browser_required == "yes" {
            "required"
        } else {
            "not-required"
        },
    )
}

fn prepare_finished_single_step_finish_gate_fixture_with_plan_qa_requirement(
    repo: &Path,
    state: &Path,
    browser_required: &str,
    include_qa: bool,
    base_branch: &str,
    qa_requirement: &str,
) -> (PathBuf, Option<PathBuf>, PathBuf, PathBuf) {
    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    set_plan_qa_requirement(repo, qa_requirement);
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    let branch_test_plan = write_test_plan_artifact(repo, state, browser_required);
    let branch_qa_path = if include_qa {
        Some(write_qa_result_artifact(repo, state, &branch_test_plan))
    } else {
        None
    };
    let branch_review_path = write_code_review_artifact(repo, state, base_branch);
    let branch_release_path = write_release_readiness_artifact(repo, state, base_branch);
    let safe_branch = normalize_identifier(&branch_name(repo));
    let current_head = current_head_sha(repo);
    let current_tree = current_head_tree_sha(repo);
    let reviewed_state_id = format!("git_tree:{current_tree}");
    let repo_slug = repo_slug(repo);
    let branch = branch_name(repo);
    let branch_closure_id = "branch-closure-ready";
    let branch_contract_identity = branch_contract_identity(repo, state, PLAN_REL);
    let browser_qa_required = if qa_requirement == "required" {
        Some(true)
    } else if qa_requirement == "not-required" {
        Some(false)
    } else {
        None
    };

    let authoritative_test_plan_source = fs::read_to_string(&branch_test_plan)
        .expect("source test-plan artifact should be readable for authoritative finish fixture");
    let authoritative_test_plan_fingerprint = sha256_hex(authoritative_test_plan_source.as_bytes());
    let authoritative_test_plan = harness_authoritative_artifact_path(
        state,
        &repo_slug,
        &branch,
        &format!("test-plan-{authoritative_test_plan_fingerprint}.md"),
    );
    write_file(&authoritative_test_plan, &authoritative_test_plan_source);

    let authoritative_qa = branch_qa_path.as_ref().map(|branch_qa_path| {
        let qa_source = rewrite_source_test_plan_header(
            &fs::read_to_string(branch_qa_path)
                .expect("source QA artifact should be readable for authoritative finish fixture"),
            &authoritative_test_plan,
        );
        let qa_fingerprint = sha256_hex(qa_source.as_bytes());
        let qa_path = harness_authoritative_artifact_path(
            state,
            &repo_slug,
            &branch,
            &format!("browser-qa-{qa_fingerprint}.md"),
        );
        write_file(&qa_path, &qa_source);
        (qa_path, qa_fingerprint)
    });

    let authoritative_review_source = fs::read_to_string(&branch_review_path)
        .expect("source review artifact should be readable for authoritative finish fixture");
    let authoritative_review_fingerprint = sha256_hex(authoritative_review_source.as_bytes());
    let authoritative_review = harness_authoritative_artifact_path(
        state,
        &repo_slug,
        &branch,
        &format!("final-review-{authoritative_review_fingerprint}.md"),
    );
    write_file(&authoritative_review, &authoritative_review_source);

    let authoritative_release_source = fs::read_to_string(&branch_release_path)
        .expect("source release artifact should be readable for authoritative finish fixture");
    let authoritative_release_fingerprint = sha256_hex(authoritative_release_source.as_bytes());
    let authoritative_release = harness_authoritative_artifact_path(
        state,
        &repo_slug,
        &branch,
        &format!("release-docs-{authoritative_release_fingerprint}.md"),
    );
    write_file(&authoritative_release, &authoritative_release_source);
    let release_summary = "Release-readiness artifact fixture for finish-gate coverage.";
    let release_summary_hash = sha256_hex(release_summary.as_bytes());
    let release_record_id = format!("release-readiness-record-{authoritative_release_fingerprint}");
    let final_review_summary = "Final whole-diff review artifact fixture for finish-gate coverage.";
    let final_review_summary_hash = sha256_hex(final_review_summary.as_bytes());
    let final_review_record_id = format!("final-review-record-{authoritative_review_fingerprint}");
    let qa_summary = concat!("Browser QA artifact fixture for gate", "-finish coverage.");
    let qa_summary_hash = sha256_hex(qa_summary.as_bytes());
    let task_review_summary = "Task closure review fixture for finish-gate coverage.";
    let task_review_summary_hash = sha256_hex(task_review_summary.as_bytes());
    let task_verification_summary = "Task closure verification fixture for finish-gate coverage.";
    let task_verification_summary_hash = sha256_hex(task_verification_summary.as_bytes());
    let qa_record_id = authoritative_qa
        .as_ref()
        .map(|(_, fingerprint)| format!("browser-qa-record-{fingerprint}"));
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
    let branch_closure_records = json!({
        branch_closure_id: {
            "branch_closure_id": branch_closure_id,
            "source_plan_path": PLAN_REL,
            "source_plan_revision": 1,
            "repo_slug": repo_slug.clone(),
            "branch_name": branch.clone(),
            "base_branch": base_branch,
            "reviewed_state_id": reviewed_state_id.clone(),
            "contract_identity": branch_contract_identity,
            "effective_reviewed_branch_surface": "repo_tracked_content",
            "source_task_closure_ids": ["task-1-closure"],
            "provenance_basis": "task_closure_lineage",
            "closure_status": "current",
            "superseded_branch_closure_ids": []
        }
    });
    let release_readiness_record_history = json!({
        release_record_id.clone(): {
            "record_id": release_record_id.clone(),
            "record_sequence": 1,
            "record_status": "current",
            "branch_closure_id": branch_closure_id,
            "source_plan_path": PLAN_REL,
            "source_plan_revision": 1,
            "repo_slug": repo_slug.clone(),
            "branch_name": branch.clone(),
            "base_branch": base_branch,
            "reviewed_state_id": reviewed_state_id.clone(),
            "result": "ready",
            "release_docs_fingerprint": authoritative_release_fingerprint.clone(),
            "summary": release_summary,
            "summary_hash": release_summary_hash.clone(),
            "generated_by_identity": "featureforge/release-readiness"
        }
    });
    let final_review_record_history = json!({
        final_review_record_id.clone(): {
            "record_id": final_review_record_id.clone(),
            "record_sequence": 1,
            "record_status": "current",
            "branch_closure_id": branch_closure_id,
            "release_readiness_record_id": release_record_id.clone(),
            "dispatch_id": "fixture-final-review-dispatch",
            "reviewer_source": "fresh-context-subagent",
            "reviewer_id": "reviewer-fixture-001",
            "result": "pass",
            "final_review_fingerprint": authoritative_review_fingerprint.clone(),
            "browser_qa_required": browser_qa_required,
            "source_plan_path": PLAN_REL,
            "source_plan_revision": 1,
            "repo_slug": repo_slug.clone(),
            "branch_name": branch.clone(),
            "base_branch": base_branch,
            "reviewed_state_id": reviewed_state_id.clone(),
            "summary": final_review_summary,
            "summary_hash": final_review_summary_hash.clone()
        }
    });
    let browser_qa_record_history = if include_qa {
        let (_, qa_fingerprint) = authoritative_qa
            .as_ref()
            .expect("QA fingerprint should exist when QA is included");
        let qa_record_id = qa_record_id
            .clone()
            .expect("QA record id should exist when QA is included");
        json!({
            qa_record_id.clone(): {
                "record_id": qa_record_id,
                "record_sequence": 1,
                "record_status": "current",
                "branch_closure_id": branch_closure_id,
                "final_review_record_id": final_review_record_id.clone(),
                "source_plan_path": PLAN_REL,
                "source_plan_revision": 1,
                "repo_slug": repo_slug.clone(),
                "branch_name": branch.clone(),
                "base_branch": base_branch,
                "reviewed_state_id": reviewed_state_id.clone(),
                "result": "pass",
                "browser_qa_fingerprint": qa_fingerprint.clone(),
                "source_test_plan_fingerprint": authoritative_test_plan_fingerprint.clone(),
                "summary": qa_summary,
                "summary_hash": qa_summary_hash.clone(),
                "generated_by_identity": "featureforge/qa"
            }
        })
    } else {
        json!({})
    };
    let qa_required_without_record = !include_qa && qa_requirement == "required";
    let current_qa_branch_closure_id = if include_qa || qa_required_without_record {
        Value::from(branch_closure_id)
    } else {
        Value::Null
    };
    let current_qa_result = if include_qa {
        Value::from("pass")
    } else {
        Value::Null
    };
    let current_qa_summary_hash = if include_qa {
        Value::from(qa_summary_hash.clone())
    } else {
        Value::Null
    };
    let current_qa_record_id = if include_qa {
        qa_record_id.clone().map(Value::from).unwrap_or(Value::Null)
    } else if qa_required_without_record {
        Value::from("browser-qa-record-missing")
    } else {
        Value::Null
    };

    let active_contract_rel = "docs/featureforge/execution-evidence/active-execution-contract.md";
    let active_contract_fingerprint =
        write_execution_contract_artifact(repo, active_contract_rel, None);
    let active_contract_source = fs::read_to_string(repo.join(active_contract_rel))
        .expect("source active contract should be readable for finish fixture");
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug,
            &branch,
            &format!("contract-{active_contract_fingerprint}.md"),
        ),
        &active_contract_source,
    );
    let execution_run_id = format!("run-{safe_branch}-finish");
    let mut harness_state = json!({
        "schema_version": 1,
        "harness_phase": "ready_for_branch_completion",
        "run_identity": {
            "execution_run_id": execution_run_id,
            "source_plan_path": PLAN_REL,
            "source_plan_revision": 1
        },
        "chunk_id": format!("chunk-{safe_branch}-finish"),
        "latest_authoritative_sequence": 17,
        "repo_state_baseline_head_sha": current_head,
        "repo_state_baseline_worktree_fingerprint": "2222222222222222222222222222222222222222222222222222222222222222",
        "repo_state_drift_state": "reconciled",
        "active_contract_path": format!("contract-{active_contract_fingerprint}.md"),
        "active_contract_fingerprint": active_contract_fingerprint,
        "dependency_index_state": "fresh",
        "final_review_state": "fresh",
        "browser_qa_state": if include_qa || qa_required_without_record {
            "fresh"
        } else {
            "not_required"
        },
        "release_docs_state": "fresh",
        "last_final_review_artifact_fingerprint": authoritative_review_fingerprint,
        "last_browser_qa_artifact_fingerprint": authoritative_qa.as_ref().map(|(_, fingerprint)| fingerprint.clone()),
        "last_release_docs_artifact_fingerprint": authoritative_release_fingerprint,
        "current_branch_closure_id": branch_closure_id,
        "current_branch_closure_reviewed_state_id": reviewed_state_id,
        "current_branch_closure_contract_identity": branch_contract_identity,
        "branch_closure_records": branch_closure_records,
        "current_release_readiness_result": "ready",
        "current_release_readiness_summary_hash": release_summary_hash,
        "current_release_readiness_record_id": release_record_id,
        "release_readiness_record_history": release_readiness_record_history,
        "current_final_review_branch_closure_id": branch_closure_id,
        "current_final_review_dispatch_id": "fixture-final-review-dispatch",
        "current_final_review_reviewer_source": "fresh-context-subagent",
        "current_final_review_reviewer_id": "reviewer-fixture-001",
        "current_final_review_result": "pass",
        "current_final_review_summary_hash": final_review_summary_hash,
        "current_final_review_record_id": final_review_record_id,
        "final_review_record_history": final_review_record_history,
        "current_qa_branch_closure_id": current_qa_branch_closure_id,
        "current_qa_result": current_qa_result,
        "current_qa_summary_hash": current_qa_summary_hash,
        "current_qa_record_id": current_qa_record_id,
        "browser_qa_record_history": browser_qa_record_history,
        "finish_review_gate_pass_branch_closure_id": branch_closure_id,
        "active_worktree_lease_fingerprints": [],
        "active_worktree_lease_bindings": [],
    });
    harness_state["current_task_closure_records"] = json!({
        "task-1": task_closure_record.clone(),
    });
    harness_state["task_closure_record_history"] = json!({
        "task-1-closure": task_closure_record,
    });
    write_authoritative_harness_fixture_payload(repo, state, &harness_state);
    write_authoritative_harness_fixture_payload(
        repo,
        state,
        &json!({
            "final_review_dispatch_lineage": {
                "execution_run_id": execution_run_id,
                "dispatch_id": "fixture-final-review-dispatch",
                "branch_closure_id": branch_closure_id
            }
        }),
    );
    write_serial_unit_review_receipt_artifact(repo, state, &execution_run_id, 1, 1, &current_head);
    (
        authoritative_test_plan,
        authoritative_qa.map(|(path, _)| path),
        authoritative_review,
        authoritative_release,
    )
}

fn run_shell(repo: &Path, state: &Path, args: &[&str], context: &str) -> Output {
    let mut command = Command::new(compiled_featureforge_path());
    let compat_bin = compiled_featureforge_path();
    command
        .current_dir(repo)
        .env("FEATUREFORGE_COMPAT_BIN", compat_bin)
        .env("FEATUREFORGE_STATE_DIR", state)
        .args(["plan", "execution"])
        .args(args);
    run(command, context)
}

fn run_shell_json(repo: &Path, state: &Path, args: &[&str], context: &str) -> Value {
    parse_json(&run_shell(repo, state, args, context), context)
}

fn run_rust(repo: &Path, state: &Path, args: &[&str], context: &str) -> Output {
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state)
        .args(["plan", "execution"])
        .args(args);
    run(command, context)
}

fn run_rust_with_env(
    repo: &Path,
    state: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    context: &str,
) -> Output {
    if env.is_empty() {
        return run_rust(repo, state, args, context);
    }
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state)
        .args(["plan", "execution"])
        .args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    run(command, context)
}

fn run_rust_json(repo: &Path, state: &Path, args: &[&str], context: &str) -> Value {
    parse_json(&run_rust(repo, state, args, context), context)
}

fn materialize_state_dir_projections(repo: &Path, state: &Path, context: &str) {
    let materialized = run_rust_json(
        repo,
        state,
        &["materialize-projections", "--plan", PLAN_REL],
        context,
    );
    assert_eq!(materialized["action"], Value::from("materialized"));
    assert_eq!(materialized["runtime_truth_changed"], Value::Bool(false));
}

fn checkout_execution_fixture_branch(repo: &Path) {
    let mut checkout = Command::new("git");
    checkout
        .args(["checkout", "-B", concat!("execution-pre", "flight-fixture")])
        .current_dir(repo);
    run_checked(
        checkout,
        concat!("git checkout execution-pre", "flight-fixture"),
    );
}

#[test]
fn canonical_status_matches_helper_for_clean_plan() {
    let (repo_dir, state_dir) = init_repo("plan-execution-status");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");

    let helper = run_shell_json(repo, state, &["status", "--plan", PLAN_REL], "shell status");
    let rust = run_rust_json(repo, state, &["status", "--plan", PLAN_REL], "rust status");

    for field in [
        "plan_revision",
        "execution_mode",
        "execution_started",
        "evidence_path",
        "active_task",
        "active_step",
        "blocking_task",
        "blocking_step",
        "resume_task",
        "resume_step",
    ] {
        assert_eq!(rust[field], helper[field], "field {field} should match");
    }
    assert!(
        rust["execution_fingerprint"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
}

#[test]
fn canonical_status_exposes_harness_state_surface_before_execution_starts() {
    let (repo_dir, state_dir) = init_repo("plan-execution-harness-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");

    assert_exact_public_harness_phase_set();

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "rust status for harness state",
    );

    let harness_phase = status["harness_phase"]
        .as_str()
        .expect("status should expose harness_phase");
    assert_eq!(
        harness_phase, "implementation_handoff",
        "status should expose the exact pre-execution harness phase"
    );
    let chunk_id = status
        .get("chunk_id")
        .expect("status should expose chunk_id");
    assert!(
        chunk_id.as_str().is_some_and(|value| !value.is_empty()),
        "status should expose chunk_id as a non-empty string before execution starts, got {chunk_id:?}"
    );
    let execution_run_id = status
        .get("execution_run_id")
        .expect("status should expose execution_run_id");
    assert!(
        execution_run_id.is_null(),
        "status should keep execution_run_id null before execution_{} accepts a run identity, got {:?}",
        execution_run_id,
        concat!("pre", "flight")
    );
    assert_eq!(status["latest_authoritative_sequence"], Value::from(0));
    assert_eq!(status["active_task"], Value::Null);
    assert_eq!(status["blocking_task"], Value::Null);
    assert_eq!(status["resume_task"], Value::Null);

    for field in ["chunking_strategy", "evaluator_policy", "reset_policy"] {
        let value = status
            .get(field)
            .unwrap_or_else(|| panic!("status should expose {field}"));
        assert!(
            value.is_null(),
            "status should keep {} null before execution_{} accepts authoritative policy, got {:?}",
            field,
            value,
            concat!("pre", "flight")
        );
    }

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
        "status should expose the frozen policy and downstream freshness fields as strings, missing: {missing_string_fields:?}"
    );

    for field in [
        "repo_state_baseline_head_sha",
        "repo_state_baseline_worktree_fingerprint",
    ] {
        let value = status
            .get(field)
            .unwrap_or_else(|| panic!("status should expose {field}"));
        assert!(
            value.is_null() || value.as_str().is_some_and(|value| !value.is_empty()),
            "status should expose {} as null before execution_{} acceptance or as a non-empty string after acceptance, got {:?}",
            field,
            value,
            concat!("pre", "flight")
        );
    }

    let write_authority_state = status
        .get("write_authority_state")
        .expect("status should expose write_authority_state");
    assert!(
        write_authority_state
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "status should expose write_authority_state as a non-empty string before execution starts, got {write_authority_state:?}"
    );

    for field in ["write_authority_holder", "write_authority_worktree"] {
        let value = status
            .get(field)
            .unwrap_or_else(|| panic!("status should expose {field}"));
        assert!(
            value.is_null() || value.as_str().is_some_and(|value| !value.is_empty()),
            "status should expose {field} as null when unknown pre-start or as non-empty diagnostic metadata once known, got {value:?}"
        );
    }

    let missing_null_fields = missing_null_fields(
        &status,
        &[
            "active_contract_path",
            "active_contract_fingerprint",
            "last_evaluation_report_path",
            "last_evaluation_report_fingerprint",
            "last_evaluation_evaluator_kind",
            "last_evaluation_verdict",
        ],
    );
    assert!(
        missing_null_fields.is_empty(),
        "status should keep active pointers null before execution starts, missing: {missing_null_fields:?}"
    );

    for field in [
        "required_evaluator_kinds",
        "completed_evaluator_kinds",
        "pending_evaluator_kinds",
        "non_passing_evaluator_kinds",
        "open_failed_criteria",
        "reason_codes",
    ] {
        assert!(
            status.get(field).and_then(Value::as_array).is_some(),
            "status should expose array field {field} for harness state"
        );
    }

    for field in [
        "current_chunk_retry_count",
        "current_chunk_retry_budget",
        "current_chunk_pivot_threshold",
    ] {
        assert!(
            status.get(field).and_then(Value::as_u64).is_some(),
            "status should expose numeric field {field} for harness state"
        );
    }

    assert!(
        status
            .get("handoff_required")
            .and_then(Value::as_bool)
            .is_some(),
        "status should expose handoff_required for harness state"
    );

    let reason_codes = status["reason_codes"]
        .as_array()
        .expect("status should expose reason_codes as an array");
    assert!(
        reason_codes.is_empty(),
        "pre-start status should not surface blocking reason codes, got: {reason_codes:?}"
    );

    let review_stack = status
        .get("review_stack")
        .expect("status should expose review_stack");
    assert!(
        review_stack.is_null(),
        "status should keep review_stack null before execution_{} accepts authoritative policy, got {:?}",
        review_stack,
        concat!("pre", "flight")
    );

    for field in [
        "final_review_state",
        "browser_qa_state",
        "release_docs_state",
    ] {
        let freshness = status[field]
            .as_str()
            .unwrap_or_else(|| panic!("status should expose {field} as a freshness string"));
        assert!(
            matches!(freshness, "not_required" | "missing" | "fresh" | "stale"),
            "status should keep {field} on the stable freshness vocabulary, got {freshness:?}"
        );
    }

    for (state_field, fingerprint_field) in [
        (
            "final_review_state",
            "last_final_review_artifact_fingerprint",
        ),
        ("browser_qa_state", "last_browser_qa_artifact_fingerprint"),
        (
            "release_docs_state",
            "last_release_docs_artifact_fingerprint",
        ),
    ] {
        let freshness = status
            .get(state_field)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("status should expose {state_field} as a freshness string"));
        let fingerprint = status
            .get(fingerprint_field)
            .unwrap_or_else(|| panic!("status should expose {fingerprint_field}"));
        match freshness {
            "fresh" | "stale" => assert!(
                fingerprint.as_str().is_some_and(|value| !value.is_empty()),
                "status should expose non-empty {fingerprint_field} when {state_field} is {freshness}"
            ),
            "not_required" | "missing" => assert!(
                fingerprint.is_null()
                    || fingerprint.as_str().is_some_and(|value| !value.is_empty()),
                "status should expose {fingerprint_field} as null or a non-empty authoritative fingerprint while {state_field} is {freshness}"
            ),
            freshness => panic!("unexpected freshness value for {state_field}: {freshness}"),
        }
    }
}

#[test]
fn canonical_status_rejects_checked_steps_with_fenced_step_details_before_execution_starts() {
    let (repo_dir, state_dir) = init_repo("plan-execution-checked-step-details");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "none");
    add_fenced_step_details(repo);
    mark_all_plan_steps_checked(repo);

    let rust = run_rust(repo, state, &["status", "--plan", PLAN_REL], "rust status");
    let failure = parse_failure_json(
        &rust,
        "status should reject newly approved plans that begin with checked steps",
    );
    assert_eq!(failure["error_class"], Value::from("PlanNotExecutionReady"));
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("start execution-clean")),
        "status should explain the execution-clean requirement, got {failure}",
    );
}

#[test]
fn canonical_status_accepts_explicit_plan_when_newer_sibling_spec_exists() {
    let (repo_dir, state_dir) = init_repo("plan-execution-stale-sibling-spec");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "none");
    write_newer_approved_spec_same_revision_different_path(repo);

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "rust status with newer sibling approved spec and explicit plan",
    );
    assert_eq!(status["execution_started"], Value::from("no"));
    assert_eq!(status["plan_revision"], Value::from(1));
}

#[test]
fn canonical_status_rejects_approved_plan_with_draft_reviewer_provenance() {
    let (repo_dir, state_dir) = init_repo("plan-execution-approved-plan-reviewer-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "none");
    replace_in_file(
        &repo.join(PLAN_REL),
        "**Last Reviewed By:** plan-eng-review",
        "**Last Reviewed By:** writing-plans",
    );

    let output = run_rust(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "rust status with approved plan reviewer drift",
    );
    assert!(
        !output.status.success(),
        "status should fail closed when an Engineering Approved plan keeps draft reviewer provenance, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let json: Value =
        serde_json::from_slice(payload).expect("approved plan reviewer drift error should be json");
    assert_eq!(json["error_class"], "PlanNotExecutionReady");
}

#[test]
fn canonical_status_rejects_approved_source_spec_with_draft_reviewer_provenance() {
    let (repo_dir, state_dir) = init_repo("plan-execution-approved-spec-reviewer-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    replace_in_file(
        &repo.join(SPEC_REL),
        "**Last Reviewed By:** plan-ceo-review",
        "**Last Reviewed By:** brainstorming",
    );
    write_plan(repo, "none");

    let output = run_rust(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "rust status with approved source spec reviewer drift",
    );
    assert!(
        !output.status.success(),
        "status should fail closed when a CEO Approved source spec keeps draft reviewer provenance, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let json: Value = serde_json::from_slice(payload)
        .expect("approved source spec reviewer drift error should be json");
    assert_eq!(json["error_class"], "PlanNotExecutionReady");
}

#[test]
fn canonical_status_accepts_explicit_plan_when_approved_specs_are_ambiguous() {
    let (repo_dir, state_dir) = init_repo("plan-execution-ambiguous-approved-specs");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let newer_spec_rel = "docs/featureforge/specs/2026-03-17-example-execution-plan-design-v2.md";
    write_approved_spec(repo);
    write_newer_approved_spec_same_revision_different_path(repo);
    write_plan(repo, "none");
    replace_in_file(
        &repo.join(PLAN_REL),
        &format!("**Source Spec:** `{SPEC_REL}`"),
        &format!("**Source Spec:** `{newer_spec_rel}`"),
    );

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "rust status with ambiguous approved specs and explicit plan",
    );
    assert_eq!(status["execution_started"], Value::from("no"));
    assert_eq!(status["plan_revision"], Value::from(1));
}

#[test]
fn canonical_status_accepts_explicit_plan_when_approved_plans_are_ambiguous() {
    let (repo_dir, state_dir) = init_repo("plan-execution-ambiguous-approved-plans");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "none");
    write_second_approved_plan_same_spec(repo, "none");

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "rust status with ambiguous approved plans and explicit plan",
    );
    assert_eq!(status["execution_started"], Value::from("no"));
    assert_eq!(status["plan_revision"], Value::from(1));
}

fn assert_begin_blocks_cross_task_without_prior_task_closure() {
    let (repo_dir, state_dir) = init_repo("plan-execution-task-boundary-begin-requires-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "none");
    checkout_execution_fixture_branch(repo);

    let status_before_task1 = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status before task 1 step 1 begin",
    );
    let begin_task1_step1 = run_rust_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_task1["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "begin task 1 step 1",
    );
    let complete_task1_step1 = run_rust_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 1 for task-boundary closure test.",
            "--file",
            "README.md",
            "--manual-verify-summary",
            "Fixture verification for task 1 step 1.",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("execution fingerprint should be present after begin"),
        ],
        "complete task 1 step 1",
    );
    let begin_task1_step2 = run_rust_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "2",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            complete_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("execution fingerprint should be present after complete"),
        ],
        "begin task 1 step 2",
    );
    let complete_task1_step2 = run_rust_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "2",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 2 for task-boundary closure test.",
            "--file",
            "README.md",
            "--manual-verify-summary",
            "Fixture verification for task 1 step 2.",
            "--expect-execution-fingerprint",
            begin_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("execution fingerprint should be present after begin"),
        ],
        "complete task 1 step 2",
    );
    let execution_run_id = complete_task1_step2["execution_run_id"]
        .as_str()
        .expect("execution run id should be present after task 1 completion");
    write_initial_dispatch_harness_state(repo, state, execution_run_id);

    let begin_task2_step1 = run_rust(
        repo,
        state,
        &[
            "begin",
            "--plan",
            PLAN_REL,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            complete_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("execution fingerprint should be present after task 1 completion"),
        ],
        "begin task 2 step 1 without prior-task review and verification closure",
    );
    let failure = parse_failure_json(&begin_task2_step1, "task-boundary begin gate");
    assert_eq!(
        failure["error_class"], "ExecutionStateNotReady",
        "task-boundary begin should fail with execution-state-not-ready diagnostics, got {failure}"
    );
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("prior_task_current_closure_missing")),
        "task-boundary begin should expose prior_task_current_closure_missing diagnostics when no current prior-task closure exists, got {failure}"
    );
}

#[test]
fn task_boundary_begin_blocked_without_prior_task_closure() {
    assert_begin_blocks_cross_task_without_prior_task_closure();
}

#[test]
fn status_ignores_legacy_open_step_projection_when_authoritative_state_is_missing() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-status-legacy-open-step-projection-visibility");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "featureforge:executing-plans");

    replace_in_file(
        &repo.join(PLAN_REL),
        "- [ ] **Step 1: Prepare workspace for execution**",
        "- [ ] **Step 1: Prepare workspace for execution**\n  **Execution Note:** Interrupted - Legacy open-step projection visibility fixture.",
    );
    assert!(
        !harness_state_file_path(repo, state).exists(),
        "fixture should start without authoritative harness state",
    );

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should ignore legacy open-step projection before authoritative materialization",
    );
    assert_eq!(status["active_task"], Value::Null);
    assert_eq!(status["active_step"], Value::Null);
    assert_eq!(status["resume_task"], Value::Null);
    assert_eq!(status["resume_step"], Value::Null);
}

#[test]
fn write_authoritative_worktree_lease_artifact_rejects_unsafe_execution_context_key() {
    let (repo_dir, state_dir) = init_repo("plan-execution-unsafe-worktree-lease-path");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "featureforge:executing-plans");
    let safe_branch = normalize_identifier(&branch_name(repo));
    write_harness_state_payload(
        repo,
        state,
        &json!({
            "schema_version": 1,
            "harness_phase": "executing",
            "run_identity": {
                "execution_run_id": format!("run-{safe_branch}-unsafe"),
                "source_plan_path": PLAN_REL,
                "source_plan_revision": 1
            },
            "chunk_id": format!("chunk-{safe_branch}-unsafe"),
            "latest_authoritative_sequence": 17,
            "active_worktree_lease_fingerprints": [],
        }),
    );

    let runtime = execution_runtime(repo, state);
    let (execution_run_id, _chunk_id) = current_authoritative_run_identity(repo, state);
    let current_head = current_head_sha(repo);
    let original_state =
        fs::read_to_string(harness_state_file_path(repo, state)).expect("state should exist");
    let lease_payload = json!({
        "lease_version": 1,
        "authoritative_sequence": 19,
        "execution_run_id": execution_run_id,
        "execution_context_key": "unsafe/../../state",
        "source_plan_path": PLAN_REL,
        "source_plan_revision": 1,
        "execution_unit_id": "unit-unsafe-path",
        "source_branch": branch_name(repo),
        "authoritative_integration_branch": branch_name(repo),
        "worktree_path": state.join("worktrees").join("unsafe-path").display().to_string(),
        "repo_state_baseline_head_sha": current_head.clone(),
        "repo_state_baseline_worktree_fingerprint": "2222222222222222222222222222222222222222222222222222222222222222",
        "lease_state": WorktreeLeaseState::Reconciled,
        "cleanup_state": "cleaned",
        "reviewed_checkpoint_commit_sha": current_head.clone(),
        "reconcile_result_commit_sha": current_head.clone(),
        "reconcile_result_proof_fingerprint": commit_object_fingerprint(repo, &current_head),
        "reconcile_mode": "identity_preserving",
        "generated_by": "featureforge:executing-plans",
        "generated_at": "2026-03-27T12:00:00Z",
        "lease_fingerprint": "",
    });
    let mut lease: WorktreeLease =
        serde_json::from_value(lease_payload.clone()).expect("lease payload should deserialize");
    lease.lease_fingerprint = canonical_worktree_lease_fingerprint(&lease_payload);

    let failure = write_authoritative_worktree_lease_artifact(&runtime, &lease)
        .expect_err("unsafe execution context key should be rejected");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert_eq!(
        fs::read_to_string(harness_state_file_path(repo, state))
            .expect("state should remain readable after rejected lease write"),
        original_state
    );
}

#[test]
fn write_authoritative_unit_review_receipt_artifact_rejects_unsafe_execution_unit_id() {
    let (repo_dir, state_dir) = init_repo("plan-execution-unsafe-unit-review-path");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "featureforge:executing-plans");
    let safe_branch = normalize_identifier(&branch_name(repo));
    write_harness_state_payload(
        repo,
        state,
        &json!({
            "schema_version": 1,
            "harness_phase": "executing",
            "run_identity": {
                "execution_run_id": format!("run-{safe_branch}-unsafe"),
                "source_plan_path": PLAN_REL,
                "source_plan_revision": 1
            },
            "chunk_id": format!("chunk-{safe_branch}-unsafe"),
            "latest_authoritative_sequence": 17,
            "active_worktree_lease_fingerprints": [],
        }),
    );

    let runtime = execution_runtime(repo, state);
    let (execution_run_id, _chunk_id) = current_authoritative_run_identity(repo, state);
    let original_state =
        fs::read_to_string(harness_state_file_path(repo, state)).expect("state should exist");
    let unsafe_execution_unit_id = "unit-unsafe/../../state";
    let approved_task_packet_fingerprint = expected_packet_fingerprint(repo, 1, 1);
    let approved_unit_contract_fingerprint = approved_unit_contract_fingerprint_for_review(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        &approved_task_packet_fingerprint,
        unsafe_execution_unit_id,
    );
    let reconcile_result_commit_sha = current_head_sha(repo);
    let reconcile_result_proof_fingerprint =
        commit_object_fingerprint(repo, &reconcile_result_commit_sha);
    let unsigned_receipt = format!(
        "# Unit Review Result\n**Review Stage:** featureforge:unit-review\n**Reviewer Provenance:** dedicated-independent\n**Strategy Checkpoint Fingerprint:** {FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT}\n**Source Plan:** {PLAN_REL}\n**Source Plan Revision:** 1\n**Execution Run ID:** {execution_run_id}\n**Execution Unit ID:** {unsafe_execution_unit_id}\n**Lease Fingerprint:** bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n**Execution Context Key:** context-key\n**Approved Task Packet Fingerprint:** {approved_task_packet_fingerprint}\n**Approved Unit Contract Fingerprint:** {approved_unit_contract_fingerprint}\n**Reconciled Result SHA:** {reconcile_result_commit_sha}\n**Reconcile Result Proof Fingerprint:** {reconcile_result_proof_fingerprint}\n**Reconcile Mode:** identity_preserving\n**Reviewed Worktree:** /tmp/worktree\n**Reviewed Checkpoint SHA:** {reconcile_result_commit_sha}\n**Result:** pass\n**Generated By:** featureforge:unit-review\n**Generated At:** 2026-03-27T12:00:00Z\n"
    );
    let receipt_fingerprint = canonical_unit_review_receipt_fingerprint(&unsigned_receipt);
    let receipt_source = format!(
        "# Unit Review Result\n**Receipt Fingerprint:** {receipt_fingerprint}\n{}",
        unsigned_receipt.trim_start_matches("# Unit Review Result\n")
    );

    let failure = write_authoritative_unit_review_receipt_artifact(
        &runtime,
        &execution_run_id,
        unsafe_execution_unit_id,
        &receipt_source,
    )
    .expect_err("unsafe execution unit id should be rejected");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert_eq!(
        fs::read_to_string(harness_state_file_path(repo, state))
            .expect("state should remain readable after rejected receipt write"),
        original_state
    );
}

#[test]
fn persist_active_worktree_lease_index_respects_write_authority_lock() {
    let (repo_dir, state_dir) = init_repo("plan-execution-lease-index-lock");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "featureforge:executing-plans");
    let safe_branch = normalize_identifier(&branch_name(repo));
    write_harness_state_payload(
        repo,
        state,
        &json!({
            "schema_version": 1,
            "harness_phase": "executing",
            "run_identity": {
                "execution_run_id": format!("run-{safe_branch}-lock"),
                "source_plan_path": PLAN_REL,
                "source_plan_revision": 1
            },
            "chunk_id": format!("chunk-{safe_branch}-lock"),
            "latest_authoritative_sequence": 17,
            "active_worktree_lease_fingerprints": [],
        }),
    );

    let runtime = execution_runtime(repo, state);
    let (execution_run_id, chunk_id) = current_authoritative_run_identity(repo, state);
    let lock_path = harness_state_file_path(repo, state)
        .parent()
        .expect("harness state should live under execution-harness")
        .join("write-authority.lock");
    write_file(&lock_path, &format!("pid={}\n", std::process::id()));

    let failure = persist_active_worktree_lease_index(
        &runtime,
        RunIdentitySnapshot {
            execution_run_id: ExecutionRunId::new(execution_run_id),
            source_plan_path: PLAN_REL.to_owned(),
            source_plan_revision: 1,
        },
        ChunkId::new(chunk_id),
        Vec::new(),
        Vec::new(),
    )
    .expect_err("index persistence should fail while write authority is held");

    assert_eq!(failure.error_class, "ConcurrentWriterConflict");
}

#[test]
fn canonical_execution_runtime_uses_canonical_repo_slug() {
    let (repo_dir, state_dir) = init_repo("plan-execution-runtime-slug");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "none");

    let runtime = ExecutionRuntime::discover(repo).expect("execution runtime should resolve");

    assert_eq!(runtime.repo_slug, repo_slug(repo));
    assert_eq!(
        project_artifact_dir(repo, state),
        state.join("projects").join(&runtime.repo_slug)
    );
}

#[test]
fn canonical_reopen_invalidates_completed_attempt_and_sets_resume_state() {
    let (repo_dir, state_dir) = init_repo("plan-execution-reopen");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_default_approved_single_step_execution_fixture(repo, state);
    bind_explicit_reopen_repair_target(repo, state, 1, 1);

    let status_after_repair = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should expose explicit canonical reopen target",
    );
    assert!(
        status_after_repair["public_repair_targets"]
            .as_array()
            .is_some_and(|targets| targets.iter().any(|target| {
                target["command_kind"] == "reopen" && target["task"] == 1 && target["step"] == 1
            })),
        "status should expose a typed public repair target for canonical reopen, got {status_after_repair}"
    );

    let reopened = run_rust_json(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "Explicit repair target requires execution reentry",
            "--expect-execution-fingerprint",
            status_after_repair["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint before explicit repair reopen"),
        ],
        "explicit-target canonical reopen",
    );

    assert_eq!(reopened["active_task"], Value::Null);
    assert_eq!(reopened["active_step"], Value::Null);
    assert_eq!(reopened["resume_task"], Value::from(1));
    assert_eq!(reopened["resume_step"], Value::from(1));

    materialize_state_dir_projections(repo, state, "materialize projection after canonical reopen");
    let plan = projection_support::read_state_dir_projection(&reopened, PLAN_REL);
    assert!(plan.contains("- [ ] **Step 1: Complete the single-step fixture**"));
    assert!(plan.contains(
        "**Execution Note:** Interrupted - Explicit repair target requires execution reentry"
    ));

    let evidence = projection_support::read_state_dir_projection(&reopened, &evidence_rel_path());
    assert!(evidence.contains("**Status:** Invalidated"));
    assert!(
        evidence
            .contains("**Invalidation Reason:** Explicit repair target requires execution reentry")
    );
}

#[test]
fn task4_reopen_stales_active_evaluation_handoff_and_downstream_provenance() {
    let (repo_dir, state_dir) = init_repo("plan-execution-task4-reopen-stales-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_default_approved_single_step_execution_fixture(repo, state);

    let contract_rel = "docs/featureforge/execution-evidence/task4-reopen-provenance-contract.md";
    let contract_fingerprint = write_execution_contract_artifact(repo, contract_rel, None);
    let authoritative_contract_file = format!("contract-{contract_fingerprint}.md");
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch_name(repo),
            &authoritative_contract_file,
        ),
        &fs::read_to_string(repo.join(contract_rel)).expect("contract source should be readable"),
    );
    write_harness_state_payload(
        repo,
        state,
        &json!({
            "schema_version": 1,
            "harness_phase": "executing",
            "latest_authoritative_sequence": 41,
            "active_contract_path": authoritative_contract_file,
            "active_contract_fingerprint": contract_fingerprint,
            "required_evaluator_kinds": ["spec_compliance"],
            "completed_evaluator_kinds": ["spec_compliance"],
            "pending_evaluator_kinds": [],
            "non_passing_evaluator_kinds": [],
            "aggregate_evaluation_state": "pass",
            "last_evaluation_report_path": "evaluation-before-reopen.md",
            "last_evaluation_report_fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "last_evaluation_evaluator_kind": "spec_compliance",
            "last_evaluation_verdict": "pass",
            "current_chunk_retry_count": 0,
            "current_chunk_retry_budget": 2,
            "current_chunk_pivot_threshold": 2,
            "handoff_required": false,
            "open_failed_criteria": [],
            "last_handoff_path": "handoff-before-reopen.md",
            "last_handoff_fingerprint": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "final_review_state": "fresh",
            "browser_qa_state": "fresh",
            "release_docs_state": "fresh",
            "last_final_review_artifact_fingerprint": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            "last_browser_qa_artifact_fingerprint": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            "last_release_docs_artifact_fingerprint": "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
        }),
    );

    bind_explicit_reopen_repair_target(repo, state, 1, 1);
    let status_before = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status before reopen provenance stale cascade",
    );
    assert_eq!(
        status_before["last_evaluation_report_fingerprint"],
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );

    let _ = run_rust_json(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "Reopen should stale macro provenance graph.",
            "--expect-execution-fingerprint",
            status_before["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "reopen should stale active evaluation/handoff/downstream provenance",
    );

    let status_after = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status after reopen provenance stale cascade",
    );
    assert_eq!(
        status_after["last_evaluation_report_path"],
        Value::Null,
        "reopen should stale active evaluation provenance path"
    );
    assert_eq!(
        status_after["last_evaluation_report_fingerprint"],
        Value::Null,
        "reopen should stale active evaluation provenance fingerprint"
    );
    assert_eq!(
        status_after["last_evaluation_evaluator_kind"],
        Value::Null,
        "reopen should stale evaluator provenance kind"
    );
    assert_eq!(
        status_after["last_evaluation_verdict"],
        Value::Null,
        "reopen should stale evaluator provenance verdict"
    );

    let persisted = read_authoritative_harness_state(repo, state, "reopen stale provenance check");
    assert_eq!(
        persisted["final_review_state"],
        Value::Null,
        "reopen must not persist downstream final-review projection state as event authority"
    );
    assert_eq!(
        persisted["browser_qa_state"],
        Value::Null,
        "reopen must not persist downstream browser-qa projection state as event authority"
    );
    assert_eq!(
        persisted["release_docs_state"],
        Value::Null,
        "reopen must not persist downstream release-doc projection state as event authority"
    );
    for field in [
        "last_handoff_path",
        "last_handoff_fingerprint",
        "last_final_review_artifact_fingerprint",
        "last_browser_qa_artifact_fingerprint",
        "last_release_docs_artifact_fingerprint",
    ] {
        assert!(
            persisted.get(field).is_none() || persisted[field].is_null(),
            "reopen should stale `{field}` provenance pointer, got {}",
            persisted[field]
        );
    }
}

#[test]
fn task4_reopen_rolls_back_plan_evidence_and_harness_state_when_state_publish_fails() {
    let (repo_dir, state_dir) = init_repo("plan-execution-task4-reopen-state-publish-rollback");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_default_approved_single_step_execution_fixture(repo, state);

    let contract_rel = "docs/featureforge/execution-evidence/task4-reopen-rollback-contract.md";
    let contract_fingerprint = write_execution_contract_artifact(repo, contract_rel, None);
    write_harness_state_fixture!(
        repo,
        state,
        "executing",
        contract_rel,
        &contract_fingerprint,
        &["spec_compliance"],
        &[],
        false,
    );

    bind_explicit_reopen_repair_target(repo, state, 1, 1);
    let status_before = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status before reopen rollback failpoint",
    );
    let plan_before = fs::read_to_string(repo.join(PLAN_REL))
        .expect("plan should remain readable before reopen rollback failpoint");
    let evidence_path = repo.join(evidence_rel_path());
    let evidence_before = fs::read_to_string(&evidence_path)
        .expect("evidence should remain readable before reopen rollback failpoint");
    let harness_before = fs::read_to_string(harness_state_file_path(repo, state))
        .expect("harness state should remain readable before reopen rollback failpoint");

    let reopen = run_rust_with_env(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "Reopen should roll back plan/evidence/state when state publish fails.",
            "--expect-execution-fingerprint",
            status_before["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        &[(
            "FEATUREFORGE_PLAN_EXECUTION_TEST_FAILPOINT",
            "reopen_after_plan_and_evidence_write_before_authoritative_state_publish",
        )],
        "reopen with authoritative state publish failpoint",
    );
    let failure = parse_failure_json(&reopen, "reopen authoritative state publish failpoint");
    assert_eq!(
        failure["error_class"], "PartialAuthoritativeMutation",
        "reopen should classify authoritative state publish failures as partial mutations"
    );

    assert_eq!(
        fs::read_to_string(repo.join(PLAN_REL)).expect("plan should remain readable after reopen"),
        plan_before,
        "reopen should roll back plan mutation when authoritative state publish fails"
    );
    assert_eq!(
        fs::read_to_string(&evidence_path).expect("evidence should remain readable after reopen"),
        evidence_before,
        "reopen should roll back evidence mutation when authoritative state publish fails"
    );
    assert_eq!(
        fs::read_to_string(harness_state_file_path(repo, state))
            .expect("harness state should remain readable after reopen"),
        harness_before,
        "reopen should roll back authoritative harness state mutation when publish fails"
    );
}

#[test]
fn task4_reopen_keeps_plan_and_evidence_when_projection_refresh_fails_after_event_commit() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-task4-reopen-projection-fail-after-event-commit");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_default_approved_single_step_execution_fixture(repo, state);

    let contract_rel =
        "docs/featureforge/execution-evidence/task4-reopen-projection-fail-contract.md";
    let contract_fingerprint = write_execution_contract_artifact(repo, contract_rel, None);
    write_harness_state_fixture!(
        repo,
        state,
        "executing",
        contract_rel,
        &contract_fingerprint,
        &["spec_compliance"],
        &[],
        false,
    );

    bind_explicit_reopen_repair_target(repo, state, 1, 1);
    let status_before = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status before projection-refresh failure after event commit",
    );
    let plan_before = fs::read_to_string(repo.join(PLAN_REL))
        .expect("plan should remain readable before staged projection-refresh failure");
    let evidence_path = repo.join(evidence_rel_path());
    let evidence_before = fs::read_to_string(&evidence_path)
        .expect("evidence should remain readable before staged projection-refresh failure");
    let harness_before = fs::read_to_string(harness_state_file_path(repo, state))
        .expect("harness state should remain readable before staged projection-refresh failure");
    let events_path = harness_state_file_path(repo, state).with_file_name("events.jsonl");
    let events_before = fs::read_to_string(&events_path)
        .expect("event log should remain readable before staged projection-refresh failure")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();

    let reopen = run_rust_with_env(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "Reopen should preserve committed authority when projection refresh fails after append.",
            "--expect-execution-fingerprint",
            status_before["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        &[(
            "FEATUREFORGE_PLAN_EXECUTION_TEST_FAILPOINT",
            "reopen_after_plan_and_evidence_write_before_authoritative_state_publish:after_event_append",
        )],
        "reopen with staged projection-refresh failure after event commit",
    );
    let failure = parse_failure_json(
        &reopen,
        "reopen staged projection-refresh failure after event commit",
    );
    assert_eq!(
        failure["error_class"], "PartialAuthoritativeMutation",
        "staged projection-refresh failure should remain classified as a partial authoritative mutation"
    );

    let plan_after = fs::read_to_string(repo.join(PLAN_REL))
        .expect("plan should remain readable after staged projection-refresh failure");
    assert_eq!(
        plan_after, plan_before,
        "event-first reopen should not start projection rendering once staged post-append failure is raised"
    );
    let evidence_after = fs::read_to_string(&evidence_path)
        .expect("evidence should remain readable after staged projection-refresh failure");
    assert_eq!(
        evidence_after, evidence_before,
        "event-first reopen should leave evidence projection unchanged when staged post-append failure occurs before rendering"
    );
    assert_eq!(
        fs::read_to_string(harness_state_file_path(repo, state))
            .expect("harness state should remain readable after staged projection-refresh failure"),
        harness_before,
        "state projection should remain unchanged when projection refresh fails after event commit"
    );

    let events_after_source = fs::read_to_string(&events_path)
        .expect("event log should remain readable after staged projection-refresh failure");
    let events_after = events_after_source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    assert!(
        events_after > events_before,
        "event log should append authoritative reopen snapshot before projection-refresh failure"
    );
    assert!(
        events_after_source.contains("\"command\":\"reopen\""),
        "event log should include the committed reopen command envelope after staged failure"
    );

    let status_after = run_rust(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status after projection-refresh failure after committed event append",
    );
    let status_json = parse_json(
        &status_after,
        "status after projection-refresh failure after committed event append",
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("execution_reentry_required")
    );
    assert!(
        status_json["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("plan execution begin --plan")),
        "status should continue routing from event authority even when plan/evidence projections are stale: {status_json:?}"
    );
}

#[test]
fn transfer_workflow_handoff_fails_closed_without_authoritative_open_step_state() {
    let (repo_dir, state_dir) = init_repo("plan-execution-transfer-handoff-materialize-open-step");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    replace_in_file(
        &repo.join(PLAN_REL),
        "- [ ] **Step 1: Complete the single-step fixture**",
        "- [ ] **Step 1: Complete the single-step fixture**\n  **Execution Note:** Interrupted - Legacy open-step note for routed transfer materialization.",
    );
    assert!(
        !harness_state_file_path(repo, state).exists(),
        "fixture should begin without authoritative harness state",
    );

    let transfer = run_rust(
        repo,
        state,
        &[
            "transfer",
            "--plan",
            PLAN_REL,
            "--scope",
            "task",
            "--to",
            "teammate",
            "--reason",
            "legacy open-step transfer materialization fixture",
        ],
        "routed transfer should fail closed without authoritative open-step state",
    );
    let failure = parse_failure_json(
        &transfer,
        "routed transfer should fail closed without authoritative open-step state",
    );
    assert_eq!(
        failure["error_class"],
        Value::from("ExecutionStateNotReady")
    );
    assert!(
        failure["message"].as_str().is_some_and(|message| {
            message.contains("transfer requires authoritative harness state")
        }),
        "routed transfer should require authoritative harness state instead of materializing markdown notes, got {failure:?}"
    );
    assert!(
        !harness_state_file_path(repo, state).exists(),
        "routed transfer must not create authoritative state from legacy markdown notes"
    );
}

#[test]
fn repair_review_state_fails_closed_without_authoritative_open_step_state() {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-materialize-open-step");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    replace_in_file(
        &repo.join(PLAN_REL),
        "- [ ] **Step 1: Complete the single-step fixture**",
        "- [ ] **Step 1: Complete the single-step fixture**\n  **Execution Note:** Interrupted - Legacy open-step note for repair-review-state materialization.",
    );
    assert!(
        !harness_state_file_path(repo, state).exists(),
        "fixture should begin without authoritative harness state",
    );

    let repair = run_rust(
        repo,
        state,
        &["repair-review-state", "--plan", PLAN_REL],
        "repair-review-state should fail closed instead of materializing legacy open-step state at command entry",
    );
    let failure = parse_failure_json(
        &repair,
        "repair-review-state should fail closed without authoritative open-step state",
    );
    assert_eq!(
        failure["error_class"],
        Value::from("ExecutionStateNotReady")
    );
    assert!(
        failure["message"].as_str().is_some_and(|message| {
            message.contains(
                "advance-late-stage branch-closure recording requires authoritative current task-closure state",
            )
        }),
        "repair-review-state should fail closed on the authoritative blocker instead of materializing markdown notes, got {failure:?}"
    );
    assert!(
        !harness_state_file_path(repo, state).exists(),
        "repair-review-state must not create authoritative state from legacy markdown notes"
    );
}

#[test]
fn normalize_transfer_request_rejects_mixed_legacy_and_routed_shapes() {
    let failure = normalize_transfer_request(&TransferArgs {
        plan: PathBuf::from(PLAN_REL),
        scope: Some(featureforge::cli::plan_execution::TransferScopeArg::Task),
        to: Some(String::from("teammate")),
        repair_task: Some(1),
        repair_step: None,
        source: None,
        reason: String::from("handoff required"),
        expect_execution_fingerprint: None,
    })
    .expect_err("mixed transfer shapes should fail closed");

    assert_eq!(failure.error_class.as_str(), "InvalidCommandInput");
    assert!(
        failure.message.contains("either the routed handoff shape"),
        "failure should explain the mixed transfer shape contract: {failure:?}"
    );
}

#[test]
fn normalize_transfer_request_requires_routed_fields() {
    let missing_scope = normalize_transfer_request(&TransferArgs {
        plan: PathBuf::from(PLAN_REL),
        scope: None,
        to: Some(String::from("teammate")),
        repair_task: None,
        repair_step: None,
        source: None,
        reason: String::from("handoff required"),
        expect_execution_fingerprint: None,
    })
    .expect_err("routed transfer mode should require scope");
    assert_eq!(missing_scope.error_class.as_str(), "InvalidCommandInput");
    assert!(
        missing_scope.message.contains("requires --scope"),
        "missing scope failure should explain the routed shape requirement: {missing_scope:?}"
    );

    let missing_to = normalize_transfer_request(&TransferArgs {
        plan: PathBuf::from(PLAN_REL),
        scope: Some(featureforge::cli::plan_execution::TransferScopeArg::Branch),
        to: Some(String::from("   ")),
        repair_task: None,
        repair_step: None,
        source: None,
        reason: String::from("handoff required"),
        expect_execution_fingerprint: None,
    })
    .expect_err("routed transfer mode should require to");
    assert_eq!(missing_to.error_class.as_str(), "InvalidCommandInput");
    assert!(
        missing_to.message.contains("requires --to"),
        "missing to failure should explain the routed shape requirement: {missing_to:?}"
    );
}

#[test]
fn normalize_transfer_request_requires_legacy_fields() {
    for (repair_task, repair_step, source, expect_execution_fingerprint, expected_message) in [
        (
            None,
            Some(2),
            Some(ExecutionModeArg::ExecutingPlans),
            Some(String::from("fp")),
            "--repair-task",
        ),
        (
            Some(1),
            None,
            Some(ExecutionModeArg::ExecutingPlans),
            Some(String::from("fp")),
            "--repair-step",
        ),
        (Some(1), Some(2), None, Some(String::from("fp")), "--source"),
        (
            Some(1),
            Some(2),
            Some(ExecutionModeArg::ExecutingPlans),
            None,
            "--expect-execution-fingerprint",
        ),
    ] {
        let failure = normalize_transfer_request(&TransferArgs {
            plan: PathBuf::from(PLAN_REL),
            scope: None,
            to: None,
            repair_task,
            repair_step,
            source,
            reason: String::from("repair needed"),
            expect_execution_fingerprint,
        })
        .expect_err("legacy transfer mode should require every legacy field");
        assert_eq!(failure.error_class.as_str(), "InvalidCommandInput");
        assert!(
            failure.message.contains(expected_message),
            "legacy failure should mention {expected_message}: {failure:?}"
        );
    }
}

#[test]
fn normalize_transfer_request_accepts_routed_and_legacy_shapes() {
    let routed = normalize_transfer_request(&TransferArgs {
        plan: PathBuf::from(PLAN_REL),
        scope: Some(featureforge::cli::plan_execution::TransferScopeArg::Task),
        to: Some(String::from("teammate")),
        repair_task: None,
        repair_step: None,
        source: None,
        reason: String::from("handoff required"),
        expect_execution_fingerprint: None,
    })
    .expect("routed transfer request should normalize");
    match routed.mode {
        TransferRequestMode::WorkflowHandoff { scope, to } => {
            assert_eq!(scope, "task");
            assert_eq!(to, "teammate");
            assert_eq!(routed.reason, "handoff required");
        }
        other => panic!("expected routed workflow handoff mode, got {other:?}"),
    }

    let legacy = normalize_transfer_request(&TransferArgs {
        plan: PathBuf::from(PLAN_REL),
        scope: None,
        to: None,
        repair_task: Some(1),
        repair_step: Some(2),
        source: Some(ExecutionModeArg::ExecutingPlans),
        reason: String::from("repair step reopened"),
        expect_execution_fingerprint: Some(String::from("fingerprint-123")),
    })
    .expect("legacy transfer request should normalize");
    match legacy.mode {
        TransferRequestMode::RepairStep {
            repair_task,
            repair_step,
            source,
            expect_execution_fingerprint,
        } => {
            assert_eq!(repair_task, 1);
            assert_eq!(repair_step, 2);
            assert_eq!(source, "featureforge:executing-plans");
            assert_eq!(expect_execution_fingerprint, "fingerprint-123");
            assert_eq!(legacy.reason, "repair step reopened");
        }
        other => panic!("expected legacy repair-step mode, got {other:?}"),
    }
}

#[test]
fn canonical_status_rejects_non_sequential_evidence_attempt_numbers() {
    let (repo_dir, state_dir) = init_repo("plan-execution-malformed-attempt-number");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "featureforge:executing-plans");
    write_file(
        &repo.join(evidence_rel_path()),
        &format!(
            "# Execution Evidence: 2026-03-17-example-execution-plan\n\n**Plan Path:** {PLAN_REL}\n**Plan Revision:** 1\n\n## Step Evidence\n\n### Task 1 Step 1\n#### Attempt 2\n**Status:** Completed\n**Recorded At:** 2026-03-17T14:22:31Z\n**Execution Source:** featureforge:executing-plans\n**Claim:** Prepared the workspace for execution.\n**Files:**\n- docs/example-output.md\n**Verification:**\n- `cargo test --test plan_execution` -> passed in fixture setup\n**Invalidation Reason:** N/A\n"
        ),
    );

    let output = run_rust(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status with non-sequential attempt number",
    );
    assert!(
        !output.status.success(),
        "status should fail for non-sequential attempt numbers, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let json: Value =
        serde_json::from_slice(payload).expect("non-sequential attempt error should be json");
    assert_eq!(json["error_class"], "MalformedExecutionState");
}

#[test]
fn canonical_status_omits_legacy_latest_attempt_metadata_fields() {
    let (repo_dir, state_dir) = init_repo("plan-execution-latest-completed-by-recorded-at");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "featureforge:executing-plans");
    write_file(&repo.join("docs/example-output.md"), "verified output\n");
    let file_digest = sha256_hex(
        &fs::read(repo.join("docs/example-output.md")).expect("output should be readable"),
    );
    let plan_fingerprint = execution_contract_plan_hash(repo);
    let spec_fingerprint =
        sha256_hex(&fs::read(repo.join(SPEC_REL)).expect("spec should be readable for evidence"));
    let newer_packet = expected_packet_fingerprint(repo, 1, 1);
    let older_packet = expected_packet_fingerprint(repo, 1, 2);

    write_file(
        &repo.join(evidence_rel_path()),
        &format!(
            "# Execution Evidence: 2026-03-17-example-execution-plan\n\n**Plan Path:** {PLAN_REL}\n**Plan Revision:** 1\n**Plan Fingerprint:** {plan_fingerprint}\n**Source Spec Path:** {SPEC_REL}\n**Source Spec Revision:** 1\n**Source Spec Fingerprint:** {spec_fingerprint}\n\n## Step Evidence\n\n### Task 1 Step 1\n#### Attempt 1\n**Status:** Completed\n**Recorded At:** 2026-03-17T14:22:45Z\n**Execution Source:** featureforge:executing-plans\n**Task Number:** 1\n**Step Number:** 1\n**Packet Fingerprint:** {newer_packet}\n**Head SHA:** 1111111111111111111111111111111111111111\n**Base SHA:** aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n**Claim:** Newer completed attempt.\n**Files Proven:**\n- docs/example-output.md | sha256:{file_digest}\n**Verification Summary:** Manual inspection only: Verified by fixture setup.\n**Invalidation Reason:** N/A\n\n### Task 1 Step 2\n#### Attempt 1\n**Status:** Completed\n**Recorded At:** 2026-03-17T14:22:31Z\n**Execution Source:** featureforge:executing-plans\n**Task Number:** 1\n**Step Number:** 2\n**Packet Fingerprint:** {older_packet}\n**Head SHA:** 2222222222222222222222222222222222222222\n**Base SHA:** bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n**Claim:** Older completed attempt recorded later in document order.\n**Files Proven:**\n- docs/example-output.md | sha256:{file_digest}\n**Verification Summary:** Manual inspection only: Verified by fixture setup.\n**Invalidation Reason:** N/A\n"
        ),
    );

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should omit legacy freshest completed attempt metadata fields",
    );

    let status_object = status
        .as_object()
        .expect("status should serialize as a JSON object");
    assert!(
        !status_object.contains_key("latest_head_sha"),
        "status should omit legacy latest_head_sha field"
    );
    assert!(
        !status_object.contains_key("latest_base_sha"),
        "status should omit legacy latest_base_sha field"
    );
    assert!(
        !status_object.contains_key("latest_packet_fingerprint"),
        "status should omit legacy latest_packet_fingerprint field"
    );
}

#[test]
fn canonical_status_rejects_whitespace_only_persisted_file_entry() {
    let (repo_dir, state_dir) = init_repo("plan-execution-whitespace-only-file-entry");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "featureforge:executing-plans");
    write_file(
        &repo.join(evidence_rel_path()),
        &format!(
            "# Execution Evidence: 2026-03-17-example-execution-plan\n\n**Plan Path:** {PLAN_REL}\n**Plan Revision:** 1\n\n## Step Evidence\n\n### Task 1 Step 1\n#### Attempt 1\n**Status:** Completed\n**Recorded At:** 2026-03-17T14:22:31Z\n**Execution Source:** featureforge:executing-plans\n**Claim:** Prepared the workspace for execution.\n**Files:**\n-   \n**Verification:**\n- `cargo test --test plan_execution` -> passed in fixture setup\n**Invalidation Reason:** N/A\n"
        ),
    );

    let output = run_rust(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status with whitespace-only persisted file entry",
    );
    assert!(
        !output.status.success(),
        "status should fail for whitespace-only persisted file entries, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let json: Value =
        serde_json::from_slice(payload).expect("whitespace-only file entry error should be json");
    assert_eq!(json["error_class"], "MalformedExecutionState");
}

#[test]
fn repair_review_state_clears_stale_follow_up_when_review_state_is_already_current() {
    let (repo_dir, state_dir) = init_repo("plan-execution-repair-review-state-clears-follow-up");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
    let _baseline_repair = run_rust_json(
        repo,
        state,
        &["repair-review-state", "--plan", PLAN_REL],
        "repair-review-state baseline before stale branch-reroute projection injection",
    );
    let _ = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should materialize event authority before non-authoritative branch-reroute projection injection",
    );
    write_non_authoritative_harness_projection_payload(
        repo,
        state,
        &json!({
            "review_state_repair_follow_up": "execution_reentry"
        }),
    );

    let repair = run_rust_json(
        repo,
        state,
        &["repair-review-state", "--plan", PLAN_REL],
        "repair-review-state should clear stale persisted follow-up when state is already current",
    );
    assert_eq!(
        repair["action"],
        Value::from("already_current"),
        "json: {repair}"
    );
    assert_eq!(repair["required_follow_up"], Value::Null, "json: {repair}");
    assert_eq!(
        repair["phase_detail"],
        Value::from("finish_completion_gate_ready"),
        "json: {repair}"
    );

    let authoritative_state =
        read_authoritative_harness_state(repo, state, "repair-review-state clear");
    assert_eq!(
        authoritative_state["review_state_repair_follow_up"],
        Value::Null,
        "repair-review-state should clear the persisted reroute latch when no follow-up is required",
    );
}

#[test]
fn repair_review_state_clears_stale_record_branch_closure_follow_up_when_review_state_is_already_current()
 {
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-clears-branch-reroute-follow-up");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
    let baseline_repair = run_rust_json(
        repo,
        state,
        &["repair-review-state", "--plan", PLAN_REL],
        "repair-review-state baseline before stale branch-reroute projection injection",
    );
    write_non_authoritative_harness_projection_payload(
        repo,
        state,
        &json!({
            "review_state_repair_follow_up": "record_branch_closure"
        }),
    );

    let repair = run_rust_json(
        repo,
        state,
        &["repair-review-state", "--plan", PLAN_REL],
        "repair-review-state should clear a stale persisted branch-reroute follow-up when state is already current",
    );
    assert_eq!(
        repair["action"], baseline_repair["action"],
        "json: {repair}"
    );
    assert_eq!(
        repair["required_follow_up"], baseline_repair["required_follow_up"],
        "json: {repair}"
    );
    assert_eq!(
        repair["phase_detail"], baseline_repair["phase_detail"],
        "json: {repair}"
    );

    let authoritative_state =
        read_authoritative_harness_state(repo, state, "stale branch reroute clear");
    assert_eq!(
        authoritative_state["review_state_repair_follow_up"],
        Value::Null,
        "repair-review-state should not preserve a non-authoritative branch-reroute projection latch",
    );
}

#[test]
fn repair_review_state_rebase_then_repair_returns_exact_closure_recording_target() {
    let (repo_dir, state_dir) = init_repo("plan-execution-repair-review-state-rebase-then-repair");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);

    advance_repo_head_empty_commit(repo, "rebase-only head drift before repair");
    write_file(
        &repo.join("docs/example-output.md"),
        "rebased output after conflict resolution\n",
    );
    commit_repo_changes(repo, "record replayed evidence after rebase change");

    let repair = run_rust_json(
        repo,
        state,
        &["repair-review-state", "--plan", PLAN_REL],
        "repair-review-state should fail closed to execution reentry when post-rebase drift cannot be classified as trusted late-stage-only",
    );
    assert_eq!(repair["action"], Value::from("blocked"), "json: {repair}");
    assert_eq!(
        repair["required_follow_up"],
        Value::from("execution_reentry"),
        "json: {repair}"
    );
    assert_eq!(
        repair["phase_detail"],
        Value::from("execution_reentry_required"),
        "json: {repair}"
    );

    let recommended_command = repair["recommended_command"]
        .as_str()
        .expect("repair-review-state should return a concrete follow-up command");
    assert!(
        recommended_command.starts_with("featureforge plan execution reopen --plan"),
        "repair-review-state should reroute through reopen after rebase drift when late-stage-only classification is unavailable, got {recommended_command:?}"
    );
    assert!(
        recommended_command.contains("--task 1 --step 1"),
        "repair-review-state should keep the reopened execution target pinned to Task 1 Step 1, got {recommended_command:?}"
    );
    assert!(
        repair["trace_summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("Late-Stage Surface metadata")),
        "repair-review-state should explain that missing Late-Stage Surface metadata prevents late-stage-only repair, got {repair}"
    );
}

#[test]
fn late_stage_status_ignores_stale_execution_reentry_follow_up_when_current_truth_is_clean() {
    let (repo_dir, state_dir) = init_repo("plan-execution-status-ignores-stale-follow-up");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
    let baseline_status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status baseline before injecting stale execution follow-up latch",
    );
    write_non_authoritative_harness_projection_payload(
        repo,
        state,
        &json!({
            "review_state_repair_follow_up": "execution_reentry"
        }),
    );

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should ignore stale persisted execution-reentry follow-up when late-stage truth is current",
    );

    assert_eq!(
        status["review_state_status"],
        Value::from("clean"),
        "json: {status}"
    );
    assert_eq!(status["phase"], baseline_status["phase"], "json: {status}");
    assert_eq!(
        status["phase_detail"], baseline_status["phase_detail"],
        "json: {status}"
    );
    assert_eq!(
        status["recommended_command"], baseline_status["recommended_command"],
        "json: {status}"
    );
    assert_eq!(
        status["execution_command_context"], baseline_status["execution_command_context"],
        "json: {status}"
    );
    assert_ne!(
        status["phase_detail"],
        Value::from("execution_reentry_required"),
        "status should not get trapped in execution reentry from a stale persisted follow-up: {status}",
    );
    assert_ne!(
        status["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {PLAN_REL}"
        )),
        "status should not keep recommending repair-review-state after live truth is already current: {status}",
    );
}

#[test]
fn legacy_record_branch_closure_follow_up_token_is_non_actionable_when_unbound() {
    for legacy_token in ["record_branch_closure", "advance_late_stage"] {
        let (repo_dir, state_dir) = init_repo(&format!(
            "legacy-branch-follow-up-token-unbound-{}",
            legacy_token.replace('_', "-")
        ));
        let repo = repo_dir.path();
        let state = state_dir.path();
        let base_branch = branch_name(repo);
        prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
        let baseline_status = run_rust_json(
            repo,
            state,
            &["status", "--plan", PLAN_REL],
            "status baseline before injecting unbound legacy branch follow-up token",
        );
        write_harness_state_payload(
            repo,
            state,
            &json!({
                "review_state_repair_follow_up": legacy_token
            }),
        );

        let status = run_rust_json(
            repo,
            state,
            &["status", "--plan", PLAN_REL],
            "status should quarantine unbound legacy branch follow-up token",
        );

        assert_eq!(status["phase"], baseline_status["phase"], "json: {status}");
        assert_eq!(
            status["phase_detail"], baseline_status["phase_detail"],
            "json: {status}"
        );
        assert_ne!(
            status["recommended_command"],
            Value::from(format!(
                "featureforge plan execution advance-late-stage --plan {PLAN_REL}"
            )),
            "legacy {legacy_token} token must not reactivate late-stage routing: {status}"
        );
        assert!(
            status["warning_codes"]
                .as_array()
                .is_some_and(|warnings| warnings
                    .iter()
                    .any(|warning| warning == &Value::from("legacy_follow_up_unbound"))),
            "legacy unbound token should be marked diagnostic-only: {status}"
        );
        assert!(
            !public_repair_targets_include_any_persisted_follow_up(&status),
            "legacy unbound token must not create actionable public repair targets: {status}"
        );
    }
}

#[test]
fn target_bound_branch_closure_follow_up_routes_only_while_exact_target_bound() {
    let (repo_dir, state_dir) = init_repo("target-bound-branch-closure-follow-up-bound-target");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
    let baseline_status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status baseline before injecting target-bound branch follow-up",
    );
    let semantic_workspace_id = baseline_status["semantic_workspace_tree_id"]
        .as_str()
        .expect("baseline status should expose semantic workspace id")
        .to_owned();
    let state_json = read_authoritative_harness_state(repo, state, "branch follow-up injection");
    let branch_closure_id = state_json["current_branch_closure_id"]
        .as_str()
        .expect("finished fixture should expose a current branch closure id")
        .to_owned();

    write_harness_state_payload(
        repo,
        state,
        &json!({
            "current_branch_closure_id": Value::Null,
            "review_state_repair_follow_up_record": {
                "kind": "record_branch_closure",
                "target_scope": "branch_closure",
                "target_record_id": branch_closure_id,
                "semantic_workspace_state_id": semantic_workspace_id,
                "source_route_decision_hash": "prior-route-decision-for-bound-branch-target",
                "created_sequence": 18,
                "expires_on_plan_fingerprint_change": true
            },
            "review_state_repair_follow_up": null
        }),
    );
    let injected_state =
        read_authoritative_harness_state(repo, state, "branch follow-up injection result");
    assert_eq!(
        injected_state["review_state_repair_follow_up_record"]["kind"],
        Value::from("record_branch_closure"),
        "structured branch follow-up should retain its authoritative kind: {injected_state}",
    );
    assert_eq!(
        injected_state["review_state_repair_follow_up_record"]["target_scope"],
        Value::from("branch_closure"),
        "structured branch follow-up should bind to branch scope: {injected_state}",
    );

    let bound_status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should surface target-bound branch follow-up while branch target remains bound",
    );

    assert!(
        public_repair_targets_include_persisted_follow_up(
            &bound_status,
            "advance_late_stage",
            None,
            None,
            Some(&branch_closure_id),
        ),
        "target-bound branch follow-up should remain public while exact branch target is bound: {bound_status}",
    );
    assert!(
        !bound_status["warning_codes"]
            .as_array()
            .is_some_and(|warnings| warnings
                .iter()
                .any(|warning| warning == &Value::from("legacy_follow_up_unbound"))),
        "structured branch follow-up should not be quarantined as a legacy token: {bound_status}",
    );

    write_harness_state_payload(
        repo,
        state,
        &json!({
            "branch_closure_records": {},
            "current_branch_closure_id": Value::Null
        }),
    );

    let unbound_status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should expire target-bound branch follow-up once exact branch target is gone",
    );

    assert!(
        !public_repair_targets_include_persisted_follow_up(
            &unbound_status,
            "advance_late_stage",
            None,
            None,
            Some(&branch_closure_id),
        ),
        "target-bound branch follow-up must expire when exact branch target is no longer bound: {unbound_status}",
    );
}

#[test]
fn repair_review_state_persists_target_bound_execution_reentry_follow_up_record() {
    let (repo_dir, state_dir) = init_repo("target-bound-execution-reentry-follow-up-record");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);

    advance_repo_head_empty_commit(repo, "head drift before target-bound repair follow-up");
    write_file(
        &repo.join("docs/example-output.md"),
        "rebased output after target-bound repair follow-up\n",
    );
    commit_repo_changes(repo, "record target-bound repair follow-up source change");

    let repair = run_rust_json(
        repo,
        state,
        &["repair-review-state", "--plan", PLAN_REL],
        "repair-review-state should persist a target-bound execution reentry follow-up",
    );
    assert_eq!(
        repair["required_follow_up"],
        Value::from("execution_reentry"),
        "json: {repair}"
    );

    let state_json = read_authoritative_harness_state(repo, state, "target-bound repair follow-up");
    let record = &state_json["review_state_repair_follow_up_record"];
    assert_eq!(state_json["review_state_repair_follow_up"], Value::Null);
    assert_eq!(record["kind"], Value::from("execution_reentry"));
    assert_eq!(record["target_scope"], Value::from("execution_step"));
    assert_eq!(record["target_task"], Value::from(1));
    assert_eq!(record["target_step"], Value::from(1));
    assert!(record["semantic_workspace_state_id"].as_str().is_some());
    assert!(record["source_route_decision_hash"].as_str().is_some());
    assert!(
        record["created_sequence"]
            .as_u64()
            .is_some_and(|sequence| sequence > 0)
    );

    let events_path = harness_state_file_path(repo, state).with_file_name("events.jsonl");
    let events_text = fs::read_to_string(&events_path)
        .expect("event log should be readable after target-bound repair follow-up");
    let repair_follow_up_event: Value = events_text
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|event| event["payload"]["kind"].as_str() == Some("repair_follow_up_set"))
        .expect("repair follow-up set event should be recorded");
    assert_eq!(
        repair_follow_up_event["payload"]["record"]["kind"],
        Value::from("execution_reentry"),
        "repair follow-up set event must persist the structured target-bound record: {repair_follow_up_event}"
    );
    assert!(
        repair_follow_up_event["payload"].get("follow_up").is_none(),
        "repair follow-up set event must not persist a bare follow-up token: {repair_follow_up_event}"
    );
}

#[test]
fn target_bound_execution_reentry_follow_up_expires_after_current_closure_repair() {
    let (repo_dir, state_dir) = init_repo("target-bound-follow-up-expires-after-current-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
    let baseline_status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status baseline before injecting target-bound follow-up for already-current closure",
    );
    let state_json =
        read_authoritative_harness_state(repo, state, "structured follow-up injection");
    let current_closure = &state_json["current_task_closure_records"]["task-1"];
    let closure_record_id = current_closure["closure_record_id"]
        .as_str()
        .expect("finished fixture should have a current task closure id");
    write_harness_state_payload(
        repo,
        state,
        &json!({
            "review_state_repair_follow_up_record": {
                "kind": "execution_reentry",
                "target_scope": "execution_step",
                "target_task": 1,
                "target_step": 1,
                "target_record_id": execution_step_repair_target_id(1, 1),
                "source_route_decision_hash": "route-hash-before-current-closure-repair",
                "created_sequence": 1,
                "expires_on_plan_fingerprint_change": true
            },
            "review_state_repair_follow_up": null
        }),
    );
    assert_eq!(
        closure_record_id, "task-1-closure",
        "fixture should keep the current closure target concrete before testing synthetic execution-step expiry",
    );

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should expire target-bound follow-up once target task closure is current pass/pass",
    );

    assert_eq!(
        status["phase_detail"], baseline_status["phase_detail"],
        "json: {status}"
    );
    assert!(
        !public_repair_targets_include_any_persisted_follow_up(&status),
        "expired target-bound follow-up must not be public/actionable: {status}"
    );
}

#[test]
fn legacy_task_follow_up_without_step_expires_after_current_closure_repair() {
    let (repo_dir, state_dir) = init_repo("legacy-task-follow-up-expires-after-current-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
    let baseline_status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status baseline before injecting legacy task-scoped follow-up",
    );

    write_harness_state_payload(
        repo,
        state,
        &json!({
            "review_state_repair_follow_up": "execution_reentry",
            "review_state_repair_follow_up_task": 1,
            "review_state_repair_follow_up_step": Value::Null,
            "review_state_repair_follow_up_closure_record_id": Value::Null
        }),
    );

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should expire legacy task-scoped follow-up once current closure is pass/pass",
    );

    assert_eq!(
        status["phase_detail"], baseline_status["phase_detail"],
        "json: {status}"
    );
    assert!(
        !public_repair_targets_include_any_persisted_follow_up(&status),
        "legacy task-scoped follow-up must not remain actionable after current pass/pass closure repair: {status}",
    );
}

#[test]
fn legacy_step_follow_up_expires_after_current_closure_repair() {
    let (repo_dir, state_dir) = init_repo("legacy-step-follow-up-expires-after-current-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
    let baseline_status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status baseline before injecting legacy step-scoped follow-up",
    );

    write_harness_state_payload(
        repo,
        state,
        &json!({
            "review_state_repair_follow_up": "execution_reentry",
            "review_state_repair_follow_up_task": 1,
            "review_state_repair_follow_up_step": 1,
            "review_state_repair_follow_up_closure_record_id": Value::Null
        }),
    );

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should expire legacy step-scoped follow-up once current closure is pass/pass",
    );

    assert_eq!(
        status["phase_detail"], baseline_status["phase_detail"],
        "json: {status}"
    );
    assert!(
        !public_repair_targets_include_any_persisted_follow_up(&status),
        "legacy step-scoped follow-up must not remain actionable after current pass/pass closure repair: {status}",
    );
}

#[test]
fn target_bound_follow_up_expires_on_semantic_workspace_change() {
    let (repo_dir, state_dir) = init_repo("target-bound-follow-up-semantic-workspace-change");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
    let baseline_status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status baseline before injecting semantic-mismatched follow-up",
    );
    write_harness_state_payload(
        repo,
        state,
        &json!({
            "review_state_repair_follow_up_record": {
                "kind": "execution_reentry",
                "target_scope": "execution_step",
                "target_task": 1,
                "target_step": 1,
                "target_record_id": "task-1",
                "semantic_workspace_state_id": "semantic_tree:not-current",
                "created_sequence": 1,
                "expires_on_plan_fingerprint_change": true
            },
            "review_state_repair_follow_up": null
        }),
    );

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should ignore target-bound follow-up after semantic workspace change",
    );

    assert_eq!(
        status["phase_detail"], baseline_status["phase_detail"],
        "json: {status}"
    );
    assert_ne!(
        status["phase_detail"],
        Value::from("execution_reentry_required"),
        "semantic-mismatched follow-up must not reroute execution: {status}"
    );
}

#[test]
fn target_bound_follow_up_survives_projection_only_change_with_same_semantic_workspace() {
    let (repo_dir, state_dir) = init_repo("target-bound-follow-up-projection-only-change");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
    let baseline_status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status baseline before injecting projection-stable follow-up",
    );
    let semantic_workspace_id = baseline_status["semantic_workspace_tree_id"]
        .as_str()
        .expect("baseline status should expose semantic workspace id")
        .to_owned();
    write_harness_state_payload(
        repo,
        state,
        &json!({
            "current_task_closure_records": {},
            "task_closure_record_history": {},
            "review_state_repair_follow_up_record": {
                "kind": "execution_reentry",
                "target_scope": "execution_step",
                "target_task": 1,
                "target_step": 1,
                "target_record_id": "task-1",
                "semantic_workspace_state_id": semantic_workspace_id,
                "source_route_decision_hash": "route-hash-from-before-projection-only-change",
                "created_sequence": 1,
                "expires_on_plan_fingerprint_change": true
            },
            "review_state_repair_follow_up": null
        }),
    );
    let evidence_rel = baseline_status["evidence_path"]
        .as_str()
        .expect("baseline status should expose evidence path");
    let evidence_projection_path =
        projection_support::state_dir_projection_path(&baseline_status, evidence_rel);
    if let Some(parent) = evidence_projection_path.parent() {
        fs::create_dir_all(parent).expect("state-dir projection parent should be creatable");
    }
    write_file(
        &evidence_projection_path,
        "<!-- projection-only follow-up churn -->\n",
    );
    let plan_path = repo.join(PLAN_REL);
    let plan_source = fs::read_to_string(&plan_path).expect("plan fixture should be readable");
    write_file(
        &plan_path,
        &plan_source.replace(
            "- [ ] **Step 1: Complete the single-step fixture**",
            "  - [x] **Step 1: Complete the single-step fixture**\n\n    **Execution Note:** Interrupted - Runtime projection note wraps\n      across an indented continuation line.",
        ),
    );

    let status = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should keep target-bound follow-up across projection-only churn",
    );

    assert_eq!(
        status["semantic_workspace_tree_id"],
        Value::from(semantic_workspace_id),
        "projection-only churn must not change semantic workspace identity: {status}"
    );
    assert!(
        public_repair_targets_include_persisted_follow_up(
            &status,
            "execution_reentry",
            Some(1),
            Some(1),
            None,
        ),
        "target-bound follow-up should remain matched by semantic identity across projection-only churn: {status}"
    );
}

#[test]
fn workflow_operator_ignores_stale_record_branch_closure_follow_up_when_current_truth_is_clean() {
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-ignores-stale-branch-reroute-follow-up");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
    let mut baseline_command = Command::new(compiled_featureforge_path());
    baseline_command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state)
        .args(["workflow", "operator", "--plan", PLAN_REL, "--json"]);
    let baseline_operator = parse_json(
        &run(
            baseline_command,
            "workflow/operator baseline before stale branch-reroute follow-up injection",
        ),
        "workflow/operator baseline stale branch reroute ignore",
    );
    let _ = run_rust_json(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "status should materialize event authority before non-authoritative workflow branch-reroute projection injection",
    );
    write_non_authoritative_harness_projection_payload(
        repo,
        state,
        &json!({
            "review_state_repair_follow_up": "record_branch_closure"
        }),
    );

    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state)
        .args(["workflow", "operator", "--plan", PLAN_REL, "--json"]);
    let operator = parse_json(
        &run(
            command,
            concat!(
                "workflow/operator should ignore stale persisted record",
                "-branch-closure follow-up when late-stage truth is already current"
            ),
        ),
        "workflow/operator stale branch reroute ignore",
    );

    assert_eq!(
        operator["review_state_status"], baseline_operator["review_state_status"],
        "json: {operator}"
    );
    assert_eq!(
        operator["phase"], baseline_operator["phase"],
        "json: {operator}"
    );
    assert_eq!(
        operator["phase_detail"], baseline_operator["phase_detail"],
        "json: {operator}"
    );
    assert_eq!(
        operator["next_action"], baseline_operator["next_action"],
        "json: {operator}"
    );
    assert_ne!(
        operator["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness"),
        "workflow/operator should not keep routing to branch-closure recording from a stale persisted follow-up: {operator}",
    );
    assert_ne!(
        operator["recommended_command"],
        Value::from(format!(
            "featureforge plan execution {} --plan {PLAN_REL}",
            concat!("record", "-branch-closure")
        )),
        "workflow/operator should not keep recommending {} after live truth is already current: {}",
        operator,
        concat!("record", "-branch-closure"),
    );
}

#[test]
fn workflow_operator_ignores_stale_execution_reentry_follow_up_when_current_truth_is_clean() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-ignores-stale-follow-up");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = branch_name(repo);
    prepare_finished_single_step_finish_gate_fixture(repo, state, "no", false, &base_branch);
    let mut baseline_command = Command::new(compiled_featureforge_path());
    baseline_command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state)
        .args(["workflow", "operator", "--plan", PLAN_REL, "--json"]);
    let baseline_operator = parse_json(
        &run(
            baseline_command,
            "workflow/operator baseline before stale execution-reentry follow-up injection",
        ),
        "workflow/operator baseline stale follow-up ignore",
    );
    write_harness_state_payload(
        repo,
        state,
        &json!({
            "review_state_repair_follow_up": "execution_reentry"
        }),
    );

    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state)
        .args(["workflow", "operator", "--plan", PLAN_REL, "--json"]);
    let operator = parse_json(
        &run(
            command,
            "workflow/operator should ignore stale persisted execution-reentry follow-up when late-stage truth is current",
        ),
        "workflow/operator stale follow-up ignore",
    );

    assert_eq!(
        operator["review_state_status"],
        Value::from("clean"),
        "json: {operator}"
    );
    assert_eq!(
        operator["phase"], baseline_operator["phase"],
        "json: {operator}"
    );
    assert_eq!(
        operator["phase_detail"], baseline_operator["phase_detail"],
        "json: {operator}"
    );
    assert_eq!(
        operator["next_action"], baseline_operator["next_action"],
        "json: {operator}"
    );
    assert_eq!(
        operator["recommended_command"], baseline_operator["recommended_command"],
        "json: {operator}"
    );
    assert_ne!(
        operator["phase_detail"],
        Value::from("execution_reentry_required"),
        "workflow/operator should not stay in fake execution reentry from a stale persisted follow-up: {operator}",
    );
    assert_ne!(
        operator["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {PLAN_REL}"
        )),
        "workflow/operator should not keep recommending repair-review-state after live truth is already current: {operator}",
    );
}

#[test]
fn runtime_remediation_inventory_includes_plan_execution_invariant_regressions() {
    let inventory = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime-remediation/README.md"),
    )
    .expect("runtime-remediation inventory should be readable");
    for scenario in [
        "FS-03", "FS-04", "FS-05", "FS-12", "FS-13", "FS-14", "FS-16",
    ] {
        assert!(
            inventory.contains(scenario),
            "runtime-remediation inventory should include {scenario}"
        );
    }
    assert!(
        inventory.contains(
            "tests/plan_execution.rs::runtime_remediation_fs03_compiled_cli_dispatch_target_acceptance_and_mismatch"
        ),
        "runtime-remediation inventory should map FS-03 to an explicit compiled-cli plan-execution regression"
    );
    assert!(
        inventory.contains(
            "tests/plan_execution.rs::runtime_remediation_fs04_rebuild_evidence_preserves_authoritative_state_digest"
        ),
        "runtime-remediation inventory should map FS-04 to the authoritative-state-digest invariant in plan execution"
    );
    assert!(
        inventory.contains(
            "tests/plan_execution.rs::record_review_dispatch_task_target_mismatch_fails_before_authoritative_mutation"
        ),
        "runtime-remediation inventory should map FS-05 to explicit no-mutation target-mismatch coverage in plan execution"
    );
    assert!(
        inventory.contains(
            "tests/plan_execution.rs::record_review_dispatch_final_review_scope_rejects_task_field_before_authoritative_mutation"
        ),
        "runtime-remediation inventory should map FS-05 to final-review scope no-mutation coverage in plan execution"
    );
    assert!(
        inventory.contains(
            "tests/plan_execution.rs::record_final_review_rejects_unapproved_reviewer_source_before_mutation"
        ),
        "runtime-remediation inventory should map FS-05 to final-review reviewer-source no-mutation coverage in plan execution"
    );
    assert!(
        inventory.contains(&format!(
            "tests/plan_execution.rs::runtime_remediation_fs12_close_current_task_uses_authoritative_run_identity_without_hidden_{}",
            concat!("pre", "flight")
        )),
        "runtime-remediation inventory should map FS-12 to authoritative run-identity closure recording coverage in plan execution"
    );
    assert!(
        inventory.contains(
            "tests/plan_execution.rs::runtime_remediation_fs13_reopen_and_begin_update_authoritative_open_step_state"
        ),
        "runtime-remediation inventory should map FS-13 to authoritative open-step state mutation coverage in plan execution"
    );
    assert!(
        inventory.contains(
            "tests/plan_execution.rs::runtime_remediation_fs14_close_current_task_rebuilds_missing_current_closure_baseline_without_hidden_dispatch"
        ),
        "runtime-remediation inventory should map FS-14 to closure-baseline regeneration coverage in plan execution"
    );
    assert!(
        inventory.contains(
            "tests/plan_execution.rs::runtime_remediation_fs16_begin_no_longer_reads_prior_task_dispatch_or_receipts"
        ),
        "runtime-remediation inventory should map FS-16 to begin-time closure-authority coverage in plan execution"
    );
}
