#[path = "support/bin.rs"]
mod bin_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/json.rs"]
mod json_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/repo_template.rs"]
mod repo_template_support;
#[path = "support/runtime_json.rs"]
mod runtime_json_support;
#[path = "support/runtime_phase_handoff.rs"]
mod runtime_phase_handoff_support;

use bin_support::compiled_featureforge_path;
use featureforge::contracts::plan::{PLAN_FIDELITY_REQUIRED_SURFACES, parse_plan_file};
use featureforge::contracts::spec::parse_spec_file;
use featureforge::execution::final_review::resolve_release_base_branch;
use featureforge::execution::semantic_identity::{
    branch_definition_identity_for_context, task_definition_identity_for_task,
};
use featureforge::execution::state::current_head_sha as runtime_current_head_sha;
use featureforge::execution::state::hash_contract_plan;
use featureforge::execution::state::load_execution_context;
use featureforge::git::{discover_repository, discover_slug_identity};
use featureforge::paths::{
    branch_storage_key, harness_authoritative_artifact_path, harness_state_path,
};
use files_support::write_file;
use json_support::parse_json;
use process_support::{run, run_checked};
use repo_template_support::populate_repo_from_template;
use runtime_json_support::{discover_execution_runtime, plan_execution_status_json};
use runtime_phase_handoff_support::{workflow_handoff_json, workflow_phase_json};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-17-example-execution-plan.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-03-17-example-execution-plan-design.md";
const FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn init_repo(name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();

    populate_repo_from_template(repo);
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
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args([
                    "remote",
                    "add",
                    "origin",
                    &format!("git@github.com:example/{name}.git"),
                ])
                .current_dir(repo);
            command
        },
        "git remote add origin",
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
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, PLAN_REL);
}

fn mark_all_plan_steps_checked(repo: &Path) {
    let path = repo.join(PLAN_REL);
    let source = fs::read_to_string(&path).expect("plan should be readable");
    fs::write(path, source.replace("- [ ]", "- [x]")).expect("plan should be writable");
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, PLAN_REL);
}

fn sha256_hex(contents: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents);
    format!("{:x}", hasher.finalize())
}

fn write_current_pass_plan_fidelity_review_artifact_for_plan(repo: &Path, plan_rel: &str) {
    let plan = parse_plan_file(repo.join(plan_rel)).expect("plan fixture should parse");
    let spec_rel = plan.source_spec_path.clone();
    let spec = parse_spec_file(repo.join(&spec_rel)).expect("spec fixture should parse");
    let plan_fingerprint = sha256_hex(&fs::read(repo.join(plan_rel)).expect("plan should read"));
    let spec_fingerprint = sha256_hex(&fs::read(repo.join(&spec_rel)).expect("spec should read"));
    let verified_requirement_ids = spec
        .requirements
        .iter()
        .map(|requirement| requirement.id.clone())
        .collect::<Vec<_>>();
    let plan_stem = Path::new(plan_rel)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("plan");
    let artifact_path = repo
        .join(".featureforge")
        .join("reviews")
        .join(format!("{plan_stem}-plan-fidelity.md"));
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent).expect("plan-fidelity artifact parent should be creatable");
    }
    write_file(
        &artifact_path,
        &format!(
            "## Plan Fidelity Review Summary\n\n**Review Stage:** featureforge:plan-fidelity-review\n**Review Verdict:** pass\n**Reviewed Plan:** `{plan_rel}`\n**Reviewed Plan Revision:** {}\n**Reviewed Plan Fingerprint:** {plan_fingerprint}\n**Reviewed Spec:** `{spec_rel}`\n**Reviewed Spec Revision:** {}\n**Reviewed Spec Fingerprint:** {spec_fingerprint}\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** fixture-plan-fidelity-reviewer\n**Distinct From Stages:** featureforge:writing-plans, featureforge:plan-eng-review\n**Verified Surfaces:** {}\n**Verified Requirement IDs:** {}\n",
            plan.plan_revision,
            spec.spec_revision,
            PLAN_FIDELITY_REQUIRED_SURFACES.join(", "),
            verified_requirement_ids.join(", "),
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
    let runtime = discover_execution_runtime(
        repo,
        state_dir,
        "workflow_runtime_final_review semantic identity fixture",
    );
    let context = load_execution_context(&runtime, Path::new(plan_rel))
        .expect("workflow_runtime_final_review semantic branch identity fixture should load execution context");
    branch_definition_identity_for_context(&context)
}

fn task_contract_identity(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    task_number: u32,
) -> String {
    let runtime = discover_execution_runtime(
        repo,
        state_dir,
        "workflow_runtime_final_review semantic identity fixture",
    );
    let context = load_execution_context(&runtime, Path::new(plan_rel))
        .expect("workflow_runtime_final_review semantic task identity fixture should load execution context");
    task_definition_identity_for_task(&context, task_number)
        .expect("workflow_runtime_final_review semantic task identity fixture should compute")
        .expect("workflow_runtime_final_review semantic task identity fixture should exist")
}

fn semantic_workspace_tree_id(repo: &Path, state_dir: &Path, plan_rel: &str) -> String {
    let runtime = discover_execution_runtime(
        repo,
        state_dir,
        "workflow_runtime_final_review semantic workspace fixture",
    );
    plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_runtime_final_review semantic workspace fixture",
    )["semantic_workspace_tree_id"]
        .as_str()
        .expect("workflow_runtime_final_review semantic workspace fixture should expose semantic workspace tree id")
        .to_owned()
}

fn project_artifact_dir(repo: &Path, state: &Path) -> PathBuf {
    state.join("projects").join(repo_slug(repo))
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
    let plan_fingerprint = execution_contract_plan_hash(repo);
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
    let strategy_checkpoint_fingerprint = FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT;
    let reviewer_artifact_path = project_artifact_dir(repo, state).join(format!(
        "tester-{safe_branch}-independent-review-20260322-170950.md"
    ));
    let reviewer_artifact_source = format!(
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-22T17:09:50Z\n\n## Summary\n- dedicated independent reviewer artifact fixture.\n",
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
            "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** {reviewer_artifact_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Source Plan:** `{PLAN_REL}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {head_sha}\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-22T17:11:00Z\n",
            reviewer_artifact_path.display(),
            repo_slug(repo)
        ),
    );
    artifact_path
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

fn update_authoritative_harness_state(repo: &Path, state: &Path, updates: &[(&str, Value)]) {
    let branch = branch_name(repo);
    let state_path = harness_state_path(state, &repo_slug(repo), &branch);
    let mut payload: Value =
        match featureforge::execution::event_log::load_reduced_authoritative_state_for_tests(
            &state_path,
        )
        .unwrap_or_else(|error| {
            panic!(
                "event-authoritative final-review runtime state should reduce for {}: {}",
                state_path.display(),
                error.message
            )
        }) {
            Some(payload) => payload,
            None => match fs::read_to_string(&state_path) {
                Ok(source) => {
                    serde_json::from_str(&source).expect("harness state should be valid json")
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
                Err(error) => {
                    panic!("harness state should be readable for fixture mutation: {error}")
                }
            },
        };
    let object = payload
        .as_object_mut()
        .expect("harness state payload should remain a json object");
    object
        .entry("schema_version".to_string())
        .or_insert_with(|| Value::from(1));
    object.entry("run_identity".to_string()).or_insert_with(|| {
        json!({
            "execution_run_id": "run-fixture",
            "source_plan_path": PLAN_REL,
            "source_plan_revision": 1
        })
    });
    object
        .entry("active_worktree_lease_fingerprints".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    object
        .entry("active_worktree_lease_bindings".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    object
        .entry("dependency_index_state".to_string())
        .or_insert_with(|| Value::from("fresh"));
    object
        .entry("final_review_state".to_string())
        .or_insert_with(|| Value::from("not_required"));
    object
        .entry("browser_qa_state".to_string())
        .or_insert_with(|| Value::from("not_required"));
    object
        .entry("release_docs_state".to_string())
        .or_insert_with(|| Value::from("not_required"));
    object
        .entry("strategy_state".to_string())
        .or_insert_with(|| Value::from("ready"));
    object
        .entry("strategy_checkpoint_kind".to_string())
        .or_insert_with(|| Value::from("review_remediation"));
    object
        .entry("last_strategy_checkpoint_fingerprint".to_string())
        .or_insert_with(|| Value::from(FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT));
    object
        .entry("strategy_reset_required".to_string())
        .or_insert_with(|| Value::Bool(false));
    for (key, value) in updates {
        object.insert((*key).to_string(), value.clone());
    }
    write_file(
        &state_path,
        &serde_json::to_string(&payload).expect("harness state payload should serialize"),
    );
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(&state_path, &payload)
        .expect("authoritative final-review fixture update should sync typed event authority");
}

fn seed_current_branch_closure_truth(repo: &Path, state: &Path) {
    let branch = branch_name(repo);
    let base_branch = expected_base_branch(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let semantic_reviewed_state_id = semantic_workspace_tree_id(repo, state, PLAN_REL);
    let execution_run_id = String::from("run-fixture");
    let branch_contract_identity = branch_contract_identity(repo, state, PLAN_REL);
    let task_contract_identity = task_contract_identity(repo, state, PLAN_REL, 1);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "current_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            (
                "current_branch_closure_reviewed_state_id",
                Value::from(reviewed_state_id.clone()),
            ),
            (
                "current_branch_closure_contract_identity",
                Value::from(branch_contract_identity.clone()),
            ),
            (
                "branch_closure_records",
                json!({
                    "branch-release-closure": {
                        "branch_closure_id": "branch-release-closure",
                        "reviewed_state_id": reviewed_state_id,
                        "semantic_reviewed_state_id": semantic_reviewed_state_id,
                        "contract_identity": branch_contract_identity,
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "repo_slug": repo_slug(repo),
                        "branch_name": branch,
                        "base_branch": base_branch,
                        "effective_reviewed_branch_surface": "repo_tracked_content",
                        "source_task_closure_ids": ["task-1-closure"],
                        "provenance_basis": "task_closure_lineage",
                        "closure_status": "current",
                        "superseded_branch_closure_ids": []
                    }
                }),
            ),
            (
                "current_task_closure_records",
                json!({
                    "task-1": {
                        "dispatch_id": "fixture-task-dispatch",
                        "closure_record_id": "task-1-closure",
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "execution_run_id": execution_run_id.clone(),
                        "reviewed_state_id": reviewed_state_id.clone(),
                        "semantic_reviewed_state_id": semantic_reviewed_state_id.clone(),
                        "contract_identity": task_contract_identity.clone(),
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"workflow runtime final-review task closure review fixture"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"workflow runtime final-review task closure verification fixture"),
                        "closure_status": "current"
                    }
                }),
            ),
            (
                "task_closure_record_history",
                json!({
                    "task-1-closure": {
                        "dispatch_id": "fixture-task-dispatch",
                        "closure_record_id": "task-1-closure",
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "execution_run_id": execution_run_id,
                        "reviewed_state_id": reviewed_state_id.clone(),
                        "semantic_reviewed_state_id": semantic_reviewed_state_id,
                        "contract_identity": task_contract_identity,
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"workflow runtime final-review task closure review fixture"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"workflow runtime final-review task closure verification fixture"),
                        "closure_status": "current"
                    }
                }),
            ),
        ],
    );
}

fn publish_authoritative_release_truth(
    repo: &Path,
    state: &Path,
    release_path: &Path,
    base_branch: &str,
) {
    seed_current_branch_closure_truth(repo, state);
    let branch = branch_name(repo);
    let repo_slug = repo_slug(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let release_source =
        fs::read_to_string(release_path).expect("release artifact should be readable");
    let release_fingerprint = sha256_hex(release_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug,
            &branch,
            &format!("release-docs-{release_fingerprint}.md"),
        ),
        &release_source,
    );
    let release_summary = "Final-review fixture release-readiness milestone.";
    let release_summary_hash = sha256_hex(release_summary.as_bytes());
    let release_record_id = format!("release-readiness-record-{release_fingerprint}");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("release_docs_state", Value::from("fresh")),
            (
                "last_release_docs_artifact_fingerprint",
                Value::from(release_fingerprint.clone()),
            ),
            ("current_release_readiness_result", Value::from("ready")),
            (
                "current_release_readiness_summary_hash",
                Value::from(release_summary_hash.clone()),
            ),
            (
                "current_release_readiness_record_id",
                Value::from(release_record_id.clone()),
            ),
            (
                "release_readiness_record_history",
                json!({
                    release_record_id.clone(): {
                        "record_id": release_record_id,
                        "record_sequence": 1,
                        "record_status": "current",
                        "branch_closure_id": "branch-release-closure",
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "repo_slug": repo_slug,
                        "branch_name": branch,
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id,
                        "result": "ready",
                        "release_docs_fingerprint": release_fingerprint,
                        "summary": release_summary,
                        "summary_hash": release_summary_hash,
                        "generated_by_identity": "featureforge/release-readiness"
                    }
                }),
            ),
        ],
    );
}

fn publish_authoritative_final_review_truth(
    repo: &Path,
    state: &Path,
    review_path: &Path,
    base_branch: &str,
) {
    seed_current_branch_closure_truth(repo, state);
    let branch = branch_name(repo);
    let repo_slug = repo_slug(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let review_source =
        fs::read_to_string(review_path).expect("review artifact should be readable");
    let review_fingerprint = sha256_hex(review_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug,
            &branch,
            &format!("final-review-{review_fingerprint}.md"),
        ),
        &review_source,
    );
    let final_review_summary = "Final-review fixture authoritative milestone.";
    let final_review_summary_hash = sha256_hex(final_review_summary.as_bytes());
    let final_review_record_id = format!("final-review-record-{review_fingerprint}");
    let authoritative_state_path = harness_state_path(state, &repo_slug, &branch);
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative harness state should be readable"),
    )
    .expect("authoritative harness state should remain valid json");
    let release_readiness_record_id = authoritative_state
        .get("current_release_readiness_record_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("final_review_state", Value::from("fresh")),
            (
                "last_final_review_artifact_fingerprint",
                Value::from(review_fingerprint.clone()),
            ),
            (
                "current_final_review_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            (
                "current_final_review_dispatch_id",
                Value::from("fixture-final-review-dispatch"),
            ),
            (
                "current_final_review_reviewer_source",
                Value::from("fresh-context-subagent"),
            ),
            (
                "current_final_review_reviewer_id",
                Value::from("reviewer-fixture-001"),
            ),
            ("current_final_review_result", Value::from("pass")),
            (
                "current_final_review_summary_hash",
                Value::from(final_review_summary_hash.clone()),
            ),
            (
                "current_final_review_record_id",
                Value::from(final_review_record_id.clone()),
            ),
            (
                "final_review_record_history",
                json!({
                    final_review_record_id.clone(): {
                        "record_id": final_review_record_id,
                        "record_sequence": 1,
                        "record_status": "current",
                        "branch_closure_id": "branch-release-closure",
                        "release_readiness_record_id": release_readiness_record_id,
                        "dispatch_id": "fixture-final-review-dispatch",
                        "reviewer_source": "fresh-context-subagent",
                        "reviewer_id": "reviewer-fixture-001",
                        "result": "pass",
                        "final_review_fingerprint": review_fingerprint,
                        "browser_qa_required": false,
                        "source_plan_path": PLAN_REL,
                        "source_plan_revision": 1,
                        "repo_slug": repo_slug,
                        "branch_name": branch,
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id,
                        "summary": final_review_summary,
                        "summary_hash": final_review_summary_hash
                    }
                }),
            ),
            (
                "finish_review_gate_pass_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
        ],
    );
}

fn run_featureforge_with_env(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    context: &str,
) -> Value {
    // This suite intentionally exercises the real workflow CLI boundary.
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    if let Some(value) = std::env::var_os("FEATUREFORGE_DEBUG_FS02") {
        command.env("FEATUREFORGE_DEBUG_FS02", value);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    parse_json(&run(command, context), context)
}

fn run_plan_execution(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Value {
    // This suite intentionally exercises the real plan-execution CLI boundary.
    let mut command_args = Vec::with_capacity(args.len() + 2);
    command_args.push("plan");
    command_args.push("execution");
    command_args.extend_from_slice(args);
    run_featureforge_with_env(repo, state_dir, command_args.as_slice(), &[], context)
}

#[test]
fn document_release_precedes_final_review_after_release_truth_stales() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-fs11-release-before-final-review");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let release_path = write_release_readiness_artifact(repo, state, &base_branch);
    publish_authoritative_release_truth(repo, state, &release_path, &base_branch);
    publish_authoritative_final_review_truth(repo, state, &review_path, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("release_docs_state", Value::from("missing")),
            ("current_release_readiness_result", Value::Null),
            ("current_release_readiness_summary_hash", Value::Null),
            ("current_release_readiness_record_id", Value::Null),
        ],
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow runtime for release-before-final-review regression",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow phase for release-before-final-review regression",
    );
    let operator_json = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "operator", "--plan", PLAN_REL, "--json"],
        &[],
        "workflow operator for release-before-final-review regression",
    );
    let handoff_json = workflow_handoff_json(
        &runtime,
        "workflow handoff for release-before-final-review regression",
    );
    let status_json = run_plan_execution(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "plan execution status for release-before-final-review regression",
    );

    assert_eq!(phase_json["phase"], "document_release_pending");
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(handoff_json["phase"], "document_release_pending");
    assert_eq!(status_json["phase"], "document_release_pending");
    assert_eq!(phase_json["next_action"], "advance late stage");
    assert_eq!(operator_json["phase"], status_json["phase"]);
    assert_eq!(operator_json["phase_detail"], status_json["phase_detail"]);
    assert_eq!(handoff_json["phase"], operator_json["phase"]);
    assert_eq!(handoff_json["phase_detail"], operator_json["phase_detail"]);
    let phase_detail = status_json["phase_detail"]
        .as_str()
        .expect("release-precedence regression should include phase_detail");
    assert!(
        matches!(
            phase_detail,
            "branch_closure_recording_required_for_release_readiness"
                | "release_readiness_recording_ready"
        ),
        "release-precedence regression must stay on the document-release lane, got {phase_detail}: {status_json}"
    );
    assert_eq!(
        status_json["next_action"],
        Value::from("advance late stage")
    );
    if phase_detail == "release_readiness_recording_ready" {
        assert_eq!(status_json["recommended_command"], Value::Null);
        assert_eq!(operator_json["recommended_command"], Value::Null);
        assert_eq!(handoff_json["recommended_command"], Value::Null);
        let required_inputs = json!([
            {
                "kind": "enum",
                "name": "result",
                "values": ["ready", "blocked"]
            },
            {
                "kind": "path",
                "must_exist": true,
                "name": "summary_file"
            }
        ]);
        assert!(status_json.get("recommended_public_command_argv").is_none());
        assert!(
            operator_json
                .get("recommended_public_command_argv")
                .is_none()
        );
        assert!(
            handoff_json
                .get("recommended_public_command_argv")
                .is_none()
        );
        assert_eq!(status_json["required_inputs"], required_inputs);
        assert_eq!(operator_json["required_inputs"], required_inputs);
        assert_eq!(handoff_json["required_inputs"], required_inputs);
    } else {
        let expected_command =
            format!("featureforge plan execution advance-late-stage --plan {PLAN_REL}");
        assert_eq!(
            status_json["recommended_command"],
            Value::from(expected_command.clone())
        );
        assert_eq!(
            operator_json["recommended_command"],
            Value::from(expected_command.clone())
        );
        assert_eq!(
            handoff_json["recommended_command"],
            Value::from(expected_command)
        );
    }
}

#[test]
fn fs02_late_stage_drift_routes_consistently_across_operator_and_status() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-fs02-late-stage-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "featureforge:executing-plans");
    mark_all_plan_steps_checked(repo);
    write_single_step_v2_completed_attempt(repo, &expected_packet_fingerprint(repo, 1, 1));
    write_test_plan_artifact(repo, state, "no");
    let base_branch = expected_base_branch(repo);
    let review_path = write_code_review_artifact(repo, state, &base_branch);
    let release_path = write_release_readiness_artifact(repo, state, &base_branch);
    publish_authoritative_release_truth(repo, state, &release_path, &base_branch);
    publish_authoritative_final_review_truth(repo, state, &review_path, &base_branch);
    let plan_path = repo.join(PLAN_REL);
    let drifted_plan = format!(
        "{}\n<!-- FS-02 fixture drift on repo-owned plan/evidence surface -->\n",
        fs::read_to_string(&plan_path).expect("plan should be readable before FS-02 drift")
    );
    write_file(&plan_path, &drifted_plan);

    let operator_json = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "operator", "--plan", PLAN_REL, "--json"],
        &[],
        "workflow operator for FS-02 late-stage drift regression",
    );
    let status_json = run_plan_execution(
        repo,
        state,
        &["status", "--plan", PLAN_REL],
        "plan execution status for FS-02 late-stage drift regression",
    );

    assert_eq!(operator_json["phase"], status_json["phase"]);
    assert_eq!(operator_json["phase_detail"], status_json["phase_detail"]);
    assert_eq!(
        operator_json["review_state_status"],
        status_json["review_state_status"]
    );
    assert_eq!(
        operator_json["recommended_command"],
        status_json["recommended_command"]
    );
    let phase_detail = status_json["phase_detail"]
        .as_str()
        .expect("FS-02 late-stage drift regression should include phase_detail");
    assert_eq!(
        phase_detail, "execution_reentry_required",
        "FS-02 late-stage drift regression should deterministically classify this fixture as execution reentry, got {status_json}"
    );
    assert_eq!(
        status_json["next_action"],
        Value::from("repair review state / reenter execution"),
        "FS-02 execution-reentry classification should require repair/reentry"
    );
}
