// Internal compatibility tests extracted from tests/execution_harness_state.rs.
// This file intentionally reuses the source fixture scaffolding from the public-facing integration test.

#[path = "support/bin.rs"]
mod bin_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/git.rs"]
mod git_support;
#[path = "support/internal_only_direct_helpers.rs"]
mod internal_only_direct_helpers;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/projection.rs"]
mod projection_support;

use featureforge::contracts::plan::parse_plan_file;
use featureforge::execution::internal_args::RecordContractArgs;
use featureforge::execution::state::{
    ExecutionRuntime, PacketFingerprintInput, compute_packet_fingerprint, hash_contract_plan,
};
use featureforge::paths::{harness_dependency_index_path, harness_state_path};
use files_support::write_file;
use internal_only_direct_helpers::internal_runtime_direct as plan_execution_direct_support;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-25-execution-harness-state.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-03-25-execution-harness-state-design.md";

fn init_repo(name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();

    git_support::init_repo_with_initial_commit(repo, &format!("# {name}\n"), "init");

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
**Goal:** Harness state and storage fields are visible before execution starts.

**Context:**
- Spec Coverage: REQ-001.

**Constraints:**
- Keep the fixture focused on status and state surfaces.

**Done when:**
- Harness state and storage fields are visible before execution starts.

**Files:**
- Test: `tests/execution_harness_state.rs`

- [ ] **Step 1: Verify the harness state surface**
"#,
        ),
    );
}

fn harness_state_file_path(repo: &Path, state: &Path) -> PathBuf {
    let runtime = ExecutionRuntime::discover(repo)
        .expect("execution runtime should discover fixture repository");
    harness_state_path(state, &runtime.repo_slug, &runtime.branch_name)
}

fn repo_slug(repo: &Path) -> String {
    ExecutionRuntime::discover(repo)
        .expect("execution runtime should discover fixture repository")
        .repo_slug
}

fn branch_name(repo: &Path) -> String {
    ExecutionRuntime::discover(repo)
        .expect("execution runtime should discover fixture repository")
        .branch_name
}

fn write_harness_state_payload(repo: &Path, state: &Path, payload: &Value) {
    let state_path = harness_state_file_path(repo, state);
    write_file(
        &state_path,
        &serde_json::to_string_pretty(payload)
            .expect("harness-state fixture payload should serialize"),
    );
    let events_path = state_path.with_file_name("events.jsonl");
    let legacy_backup_path = state_path.with_file_name("state.legacy.json");
    let _ = fs::remove_file(events_path);
    let _ = fs::remove_file(legacy_backup_path);
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn write_candidate_contract_fixture(repo: &Path, artifact_rel: &str) -> String {
    let plan_source =
        fs::read_to_string(repo.join(PLAN_REL)).expect("plan should be readable for contract");
    let source_spec_source =
        fs::read_to_string(repo.join(SPEC_REL)).expect("spec should be readable for contract");
    let plan_fingerprint = hash_contract_plan(&plan_source);
    let source_spec_fingerprint = sha256_hex(source_spec_source.as_bytes());
    let plan_document =
        parse_plan_file(repo.join(PLAN_REL)).expect("plan should parse for contract");
    let task_definition_identity = plan_document
        .tasks
        .iter()
        .find(|task| task.number == 1)
        .map(serde_json::to_string)
        .transpose()
        .expect("task should serialize for contract")
        .map(|serialized| format!("task_def:{}", sha256_hex(serialized.as_bytes())))
        .expect("task should exist for contract");
    let packet_fingerprint = compute_packet_fingerprint(PacketFingerprintInput {
        plan_path: PLAN_REL,
        plan_revision: 1,
        task_definition_identity: &task_definition_identity,
        source_spec_path: SPEC_REL,
        source_spec_revision: 2,
        source_spec_fingerprint: &source_spec_fingerprint,
        task: 1,
        step: 1,
    });

    let template = format!(
        r#"# Execution Contract

**Contract Version:** 1
**Authoritative Sequence:** 17
**Source Plan Path:** `{PLAN_REL}`
**Source Plan Revision:** 1
**Source Plan Fingerprint:** `{plan_fingerprint}`
**Source Spec Path:** `{SPEC_REL}`
**Source Spec Revision:** 2
**Source Spec Fingerprint:** `{source_spec_fingerprint}`
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
**Criterion ID:** criterion-contract-scope
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

**Retry Budget:** 2
**Pivot Threshold:** 3
**Reset Policy:** none
**Generated By:** featureforge:executing-plans
**Generated At:** 2026-03-25T12:00:00Z
**Contract Fingerprint:** __CONTRACT_FINGERPRINT__
"#
    );
    let contract_fingerprint =
        sha256_hex(template.replace("__CONTRACT_FINGERPRINT__", "").as_bytes());
    write_file(
        &repo.join(artifact_rel),
        &template.replace("__CONTRACT_FINGERPRINT__", &contract_fingerprint),
    );
    contract_fingerprint
}

#[test]
fn internal_only_compatibility_record_contract_persists_dependency_index_with_authoritative_contract_node()
 {
    let (repo_dir, state_dir) =
        init_repo("execution-harness-state-dependency-index-record-contract");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "none");

    let contract_rel = "docs/featureforge/execution-evidence/dependency-index-record-contract.md";
    let contract_fingerprint = write_candidate_contract_fixture(repo, contract_rel);

    write_harness_state_payload(
        repo,
        state,
        &json!({
            "schema_version": 1,
            "harness_phase": "contract_pending_approval",
            "latest_authoritative_sequence": 0,
            "current_chunk_retry_count": 0,
            "current_chunk_retry_budget": 0,
            "current_chunk_pivot_threshold": 0,
            "handoff_required": false,
            "open_failed_criteria": []
        }),
    );

    let record_json = plan_execution_direct_support::internal_only_unit_record_contract_json(
        repo,
        state,
        &RecordContractArgs {
            plan: PathBuf::from(PLAN_REL),
            contract: PathBuf::from(contract_rel),
        },
    )
    .unwrap_or_else(|error| {
        panic!("record-contract dependency-index persistence fixture should succeed: {error}")
    });
    assert_eq!(record_json["allowed"], Value::Bool(true));

    let dependency_index_path =
        harness_dependency_index_path(state, &repo_slug(repo), &branch_name(repo));
    assert!(
        dependency_index_path.is_file(),
        "record-contract should persist a durable dependency index at {}",
        dependency_index_path.display()
    );

    let dependency_index: Value = serde_json::from_str(
        &fs::read_to_string(&dependency_index_path)
            .expect("dependency index should be readable after record-contract"),
    )
    .expect("dependency index should remain valid json after record-contract");
    let nodes = dependency_index["nodes"]
        .as_array()
        .expect("dependency index should expose a nodes array");
    let has_authoritative_contract_node = nodes.iter().any(|node| {
        node["artifact_kind"] == "contract"
            && node["artifact_fingerprint"] == contract_fingerprint
            && node["authoritative"] == Value::Bool(true)
    });
    assert!(
        has_authoritative_contract_node,
        "record-contract should index an authoritative contract dependency node for {contract_fingerprint}, got {dependency_index}"
    );
}

#[test]
fn internal_only_compatibility_record_contract_persists_observability_event_and_authoritative_mutation_counter()
 {
    let (repo_dir, state_dir) = init_repo("execution-harness-state-observability-record-contract");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_approved_spec(repo);
    write_plan(repo, "none");

    let contract_rel = "docs/featureforge/execution-evidence/observability-record-contract.md";
    let contract_fingerprint = write_candidate_contract_fixture(repo, contract_rel);

    write_harness_state_payload(
        repo,
        state,
        &json!({
            "schema_version": 1,
            "harness_phase": "contract_pending_approval",
            "latest_authoritative_sequence": 0,
            "current_chunk_retry_count": 0,
            "current_chunk_retry_budget": 0,
            "current_chunk_pivot_threshold": 0,
            "handoff_required": false,
            "open_failed_criteria": []
        }),
    );

    let record_json = plan_execution_direct_support::internal_only_unit_record_contract_json(
        repo,
        state,
        &RecordContractArgs {
            plan: PathBuf::from(PLAN_REL),
            contract: PathBuf::from(contract_rel),
        },
    )
    .unwrap_or_else(|error| {
        panic!("record-contract observability persistence fixture should succeed: {error}")
    });
    assert_eq!(record_json["allowed"], Value::Bool(true));

    let harness_root = harness_state_path(state, &repo_slug(repo), &branch_name(repo))
        .parent()
        .expect("harness state path should always live under a branch-scoped harness root")
        .to_path_buf();
    let events_path = harness_root.join("observability-events.jsonl");
    let telemetry_path = harness_root.join("telemetry-counters.json");

    assert!(
        events_path.is_file(),
        "record-contract should persist a branch-scoped observability event sink at {}",
        events_path.display()
    );
    assert!(
        telemetry_path.is_file(),
        "record-contract should persist branch-scoped telemetry counters at {}",
        telemetry_path.display()
    );

    let first_event_line = fs::read_to_string(&events_path)
        .expect("observability event sink should be readable after record-contract")
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .expect("observability event sink should contain at least one structured event line");
    let event_json: Value = serde_json::from_str(&first_event_line)
        .expect("first observability event line should be valid json");
    assert_eq!(
        event_json["event_kind"],
        Value::String("authoritative_mutation_recorded".to_owned()),
        "record-contract should emit authoritative_mutation_recorded in the persisted sink"
    );
    assert_eq!(
        event_json["command_name"],
        Value::String("record-contract".to_owned()),
        "record-contract observability should keep command_name machine-readable"
    );
    assert_eq!(
        event_json["active_contract_fingerprint"],
        Value::String(contract_fingerprint),
        "record-contract observability should carry the active contract fingerprint"
    );

    let telemetry_json: Value = serde_json::from_str(
        &fs::read_to_string(&telemetry_path)
            .expect("telemetry counters sink should be readable after record-contract"),
    )
    .expect("telemetry counters sink should remain valid json after record-contract");
    assert_eq!(
        telemetry_json["authoritative_mutation_count"],
        Value::Number(1_u64.into()),
        "record-contract should increment authoritative_mutation_count exactly once"
    );
}
