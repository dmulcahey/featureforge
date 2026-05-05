use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::json;
use tempfile::TempDir;

use super::{
    AdvanceLateStageOutputContext, CloseCurrentTaskOutcomeClass, CurrentFinalReviewAuthorityCheck,
    FinalReviewProjectionInput, advance_late_stage_follow_up_or_requery_output,
    blocked_close_current_task_output_from_operator, blocked_follow_up_for_operator,
    close_current_task_outcome_class, close_current_task_required_follow_up,
    current_final_review_record_is_still_authoritative, late_stage_required_follow_up,
    normalized_late_stage_surface, path_matches_late_stage_surface, render_final_review_artifacts,
    rewrite_branch_final_review_artifacts, rewrite_branch_head_bound_artifact,
    rewrite_branch_qa_artifact, superseded_branch_closure_ids_from_previous_current,
    task_closure_contributes_to_branch_surface, task_closure_record_covers_path,
    verify_command_launcher,
};
use crate::cli::plan_execution::{ReviewOutcomeArg, VerificationOutcomeArg};
use crate::contracts::plan::parse_plan_file;
use crate::diagnostics::FailureClass;
use crate::execution::command_eligibility::PublicCommand;
use crate::execution::context::EvidenceSourceOrigin;
use crate::execution::final_review::resolve_release_base_branch;
use crate::execution::leases::StatusAuthoritativeOverlay;
use crate::execution::leases::authoritative_state_path;
use crate::execution::query::ExecutionRoutingState;
use crate::execution::state::{
    EvidenceFormat, ExecutionContext, ExecutionEvidence, ExecutionRuntime, NO_REPO_FILES_MARKER,
};
use crate::execution::transitions::CurrentTaskClosureRecord;
use crate::execution::transitions::load_authoritative_transition_state;
use crate::git::sha256_hex;
use crate::paths::harness_authoritative_artifact_path;
use crate::workflow::status::WorkflowRoute;

fn repair_review_state_public_command(plan: &str) -> Option<PublicCommand> {
    Some(PublicCommand::RepairReviewState {
        plan: plan.to_owned(),
    })
}

#[test]
fn verify_command_launcher_matches_platform_contract() {
    let (program, args) = verify_command_launcher("printf rebuilt");
    if cfg!(windows) {
        assert_eq!(program, "cmd");
        assert_eq!(
            args,
            vec![String::from("/C"), String::from("printf rebuilt")]
        );
    } else {
        assert_eq!(program, "sh");
        assert_eq!(
            args,
            vec![String::from("-lc"), String::from("printf rebuilt")]
        );
    }
}

#[test]
fn task_closure_contributes_to_branch_surface_excludes_no_repo_marker_only_records() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut runtime =
        ExecutionRuntime::discover(&repo_root).expect("repo runtime should be discoverable");
    let tempdir = TempDir::new().expect("tempdir should exist");
    runtime.state_dir = tempdir.path().join("state");
    fs::create_dir_all(&runtime.state_dir).expect("state dir should be creatable");
    let plan_rel = "tests/codex-runtime/fixtures/plan-contract/valid-plan.md";
    let plan_abs = repo_root.join(plan_rel);
    let plan_document = parse_plan_file(&plan_abs).expect("plan document should parse");
    let plan_source = fs::read_to_string(&plan_abs).expect("plan source should read");
    let context = ExecutionContext {
        runtime,
        plan_rel: String::from(plan_rel),
        plan_abs: plan_abs.clone(),
        plan_document,
        plan_source,
        steps: Vec::new(),
        local_execution_progress_markers_present: false,
        legacy_open_step_projection_present: false,
        tasks_by_number: Default::default(),
        evidence_rel: String::from("docs/archive/featureforge/execution-evidence/placeholder.md"),
        evidence_abs: repo_root.join("docs/archive/featureforge/execution-evidence/placeholder.md"),
        evidence: ExecutionEvidence {
            format: EvidenceFormat::Empty,
            plan_path: String::from(plan_rel),
            plan_revision: 0,
            plan_fingerprint: None,
            source_spec_path: String::from(
                "docs/archive/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md",
            ),
            source_spec_revision: 0,
            source_spec_fingerprint: None,
            attempts: Vec::new(),
            source: None,
            source_origin: EvidenceSourceOrigin::Empty,
            tracked_progress_present: false,
            tracked_source: None,
        },
        authoritative_evidence_projection_fingerprint: None,
        source_spec_source: String::new(),
        source_spec_path: repo_root
            .join("docs/archive/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md"),
        execution_fingerprint: String::from("unit-test-execution-fingerprint"),
        tracked_tree_sha_cache: OnceLock::new(),
        semantic_workspace_snapshot_cache: OnceLock::new(),
        reviewed_tree_sha_cache: std::cell::RefCell::new(BTreeMap::new()),
        head_sha_cache: OnceLock::new(),
        release_base_branch_cache: OnceLock::new(),
        tracked_worktree_changes_excluding_execution_evidence_cache: OnceLock::new(),
    };
    let no_repo_only = CurrentTaskClosureRecord {
        task: 1,
        source_plan_path: Some(String::from("docs/featureforge/plans/example.md")),
        source_plan_revision: Some(1),
        execution_run_id: Some(String::from("run-1")),
        dispatch_id: String::from("dispatch-1"),
        closure_record_id: String::from("task-1-closure"),
        reviewed_state_id: String::from("git_tree:abc123"),
        semantic_reviewed_state_id: Some(String::from("semantic_tree:abc123")),
        contract_identity: String::from("contract-1"),
        effective_reviewed_surface_paths: vec![String::from(NO_REPO_FILES_MARKER)],
        review_result: String::from("pass"),
        review_summary_hash: String::from("summary"),
        verification_result: String::from("pass"),
        verification_summary_hash: String::from("verification"),
        closure_status: Some(String::from("current")),
    };
    let mixed_surface = CurrentTaskClosureRecord {
        effective_reviewed_surface_paths: vec![
            String::from(NO_REPO_FILES_MARKER),
            String::from("src/runtime.rs"),
        ],
        ..no_repo_only.clone()
    };

    assert!(
        !task_closure_contributes_to_branch_surface(&context, &no_repo_only),
        "no-repo-only task closures must not influence branch-surface baseline derivation"
    );
    assert!(
        task_closure_contributes_to_branch_surface(&context, &mixed_surface),
        "task closures that still cover repo-visible paths must contribute to branch-surface baseline derivation"
    );
}

#[test]
fn rewrite_branch_final_review_artifacts_refuses_to_rebind_review_history() {
    let tempdir = TempDir::new().expect("tempdir should exist");
    let reviewer_artifact = tempdir.path().join("reviewer.md");
    let review_receipt = tempdir.path().join("review.md");
    let original_reviewer =
        "**Strategy Checkpoint Fingerprint:** old-checkpoint\n**Head SHA:** old-head\n";
    let original_review = format!(
        "**Strategy Checkpoint Fingerprint:** old-checkpoint\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** old-fingerprint\n**Head SHA:** old-head\n",
        reviewer_artifact.display()
    );
    fs::write(&reviewer_artifact, original_reviewer)
        .expect("reviewer artifact fixture should write");
    fs::write(&review_receipt, &original_review).expect("review receipt fixture should write");

    let error = rewrite_branch_final_review_artifacts(
        &review_receipt,
        &reviewer_artifact,
        "new-head",
        "new-checkpoint",
    )
    .expect_err("append-only repair must not rewrite historical final-review proof in place");

    assert_eq!(error.error_class, FailureClass::StaleProvenance.as_str());
    assert_eq!(
        fs::read_to_string(&reviewer_artifact).expect("reviewer artifact should remain readable"),
        original_reviewer
    );
    assert_eq!(
        fs::read_to_string(&review_receipt).expect("review receipt should remain readable"),
        original_review
    );
}

#[test]
fn current_final_review_record_authoritativeness_prefers_runtime_record_over_artifact_tamper() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut runtime =
        ExecutionRuntime::discover(&repo_root).expect("repo runtime should be discoverable");
    let tempdir = TempDir::new().expect("tempdir should exist");
    runtime.state_dir = tempdir.path().join("state");
    fs::create_dir_all(&runtime.state_dir).expect("state dir should be creatable");

    let plan_rel = "tests/codex-runtime/fixtures/plan-contract/valid-plan.md";
    let plan_abs = repo_root.join(plan_rel);
    let plan_document = parse_plan_file(&plan_abs).expect("plan document should parse");
    let plan_source = fs::read_to_string(&plan_abs).expect("plan source should read");
    let context = ExecutionContext {
        runtime: runtime.clone(),
        plan_rel: String::from(plan_rel),
        plan_abs: plan_abs.clone(),
        plan_document,
        plan_source,
        steps: Vec::new(),
        local_execution_progress_markers_present: false,
        legacy_open_step_projection_present: false,
        tasks_by_number: Default::default(),
        evidence_rel: String::from("docs/archive/featureforge/execution-evidence/placeholder.md"),
        evidence_abs: repo_root.join("docs/archive/featureforge/execution-evidence/placeholder.md"),
        evidence: ExecutionEvidence {
            format: EvidenceFormat::Empty,
            plan_path: String::from(plan_rel),
            plan_revision: 0,
            plan_fingerprint: None,
            source_spec_path: String::from(
                "docs/archive/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md",
            ),
            source_spec_revision: 0,
            source_spec_fingerprint: None,
            attempts: Vec::new(),
            source: None,
            source_origin: EvidenceSourceOrigin::Empty,
            tracked_progress_present: false,
            tracked_source: None,
        },
        authoritative_evidence_projection_fingerprint: None,
        source_spec_source: String::new(),
        source_spec_path: repo_root
            .join("docs/archive/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md"),
        execution_fingerprint: String::from("unit-test-execution-fingerprint"),
        tracked_tree_sha_cache: OnceLock::new(),
        semantic_workspace_snapshot_cache: OnceLock::new(),
        reviewed_tree_sha_cache: std::cell::RefCell::new(BTreeMap::new()),
        head_sha_cache: OnceLock::new(),
        release_base_branch_cache: OnceLock::new(),
        tracked_worktree_changes_excluding_execution_evidence_cache: OnceLock::new(),
    };
    let base_branch =
        resolve_release_base_branch(&context.runtime.git_dir, &context.runtime.branch_name)
            .expect("base branch should resolve for the current repo");
    let branch_closure_id = "unit-test-branch-closure";
    let reviewed_state_id = format!(
        "git_tree:{}",
        crate::execution::current_truth::current_repo_tracked_tree_sha(&context.runtime.repo_root)
            .expect("tracked tree sha should resolve for unit coverage")
    );
    let execution_run_id = "run-unit-test";
    let branch_contract_identity = super::branch_definition_identity_for_context(&context);
    let dispatch_id = "unit-test-final-review-dispatch";
    let reviewer_source = "fresh-context-subagent";
    let reviewer_id = "unit-reviewer-001";
    let summary = "Independent final review passed in unit coverage.";
    let summary_hash = sha256_hex(summary.as_bytes());
    let strategy_checkpoint_fingerprint = sha256_hex(b"unit-test-strategy-checkpoint");
    let state_path = authoritative_state_path(&context);
    fs::create_dir_all(
        state_path
            .parent()
            .expect("authoritative state path should have a parent dir"),
    )
    .expect("authoritative state dir should be creatable");
    fs::write(
        &state_path,
        serde_json::to_string_pretty(&json!({
            "latest_authoritative_sequence": 1,
            "harness_phase": crate::execution::phase::PHASE_FINAL_REVIEW_PENDING,
            "last_strategy_checkpoint_fingerprint": strategy_checkpoint_fingerprint,
        }))
        .expect("seed authoritative state should serialize"),
    )
    .expect("seed authoritative state should write");
    let rendered = render_final_review_artifacts(
        &runtime,
        &context,
        branch_closure_id,
        reviewed_state_id.as_str(),
        &base_branch,
        FinalReviewProjectionInput {
            dispatch_id,
            reviewer_source,
            reviewer_id,
            result: "pass",
            deviations_required: false,
            summary,
        },
    )
    .expect("final-review artifacts should render for unit coverage");
    let final_review_fingerprint = sha256_hex(rendered.final_review_source.as_bytes());
    let final_review_path = harness_authoritative_artifact_path(
        &runtime.state_dir,
        &runtime.repo_slug,
        &runtime.branch_name,
        &format!("final-review-{final_review_fingerprint}.md"),
    );
    fs::create_dir_all(
        rendered
            .reviewer_artifact_path
            .parent()
            .expect("reviewer artifact should have a parent directory"),
    )
    .expect("reviewer artifact dir should be creatable");
    fs::create_dir_all(
        final_review_path
            .parent()
            .expect("final-review artifact should have a parent directory"),
    )
    .expect("final-review artifact dir should be creatable");
    fs::write(
        &rendered.reviewer_artifact_path,
        &rendered.reviewer_source_text,
    )
    .expect("reviewer artifact should write");
    fs::write(&final_review_path, &rendered.final_review_source)
        .expect("final-review artifact should write");

    fs::write(
        &state_path,
        serde_json::to_string_pretty(&json!({
            "latest_authoritative_sequence": 1,
            "harness_phase": crate::execution::phase::PHASE_READY_FOR_BRANCH_COMPLETION,
            "last_strategy_checkpoint_fingerprint": strategy_checkpoint_fingerprint,
            "current_branch_closure_id": branch_closure_id,
            "current_branch_closure_reviewed_state_id": reviewed_state_id.as_str(),
            "current_branch_closure_contract_identity": branch_contract_identity.clone(),
            "current_task_closure_records": {
                "task-1": {
                    "task": 1,
                    "source_plan_path": context.plan_rel,
                    "source_plan_revision": context.plan_document.plan_revision,
                    "execution_run_id": execution_run_id,
                    "dispatch_id": "unit-test-task-dispatch",
                    "closure_record_id": "task-1-closure",
                    "reviewed_state_id": reviewed_state_id.as_str(),
                    "contract_identity": super::current_task_contract_identity(&context, 1)
                        .expect("task contract identity should resolve"),
                    "effective_reviewed_surface_paths": ["README.md"],
                    "review_result": "pass",
                    "review_summary_hash": "task-review-summary",
                    "verification_result": "pass",
                    "verification_summary_hash": "task-verification-summary",
                    "closure_status": "current"
                }
            },
            "task_closure_record_history": {
                "task-1-closure": {
                    "task": 1,
                    "source_plan_path": context.plan_rel,
                    "source_plan_revision": context.plan_document.plan_revision,
                    "execution_run_id": execution_run_id,
                    "dispatch_id": "unit-test-task-dispatch",
                    "closure_record_id": "task-1-closure",
                    "reviewed_state_id": reviewed_state_id.as_str(),
                    "contract_identity": super::current_task_contract_identity(&context, 1)
                        .expect("task contract identity should resolve"),
                    "effective_reviewed_surface_paths": ["README.md"],
                    "review_result": "pass",
                    "review_summary_hash": "task-review-summary",
                    "verification_result": "pass",
                    "verification_summary_hash": "task-verification-summary",
                    "closure_status": "current"
                }
            },
            "branch_closure_records": {
                (branch_closure_id): {
                    "branch_closure_id": branch_closure_id,
                    "source_plan_path": context.plan_rel,
                    "source_plan_revision": context.plan_document.plan_revision,
                    "repo_slug": runtime.repo_slug,
                    "branch_name": runtime.branch_name,
                    "base_branch": base_branch,
                    "reviewed_state_id": reviewed_state_id.as_str(),
                    "contract_identity": branch_contract_identity,
                    "effective_reviewed_branch_surface": "repo_tracked_content",
                    "source_task_closure_ids": ["task-1-closure"],
                    "provenance_basis": "task_closure_lineage",
                    "closure_status": "current",
                    "superseded_branch_closure_ids": []
                }
            },
            "current_final_review_record_id": "unit-final-review-record",
            "current_final_review_branch_closure_id": branch_closure_id,
            "current_final_review_dispatch_id": dispatch_id,
            "current_final_review_reviewer_source": reviewer_source,
            "current_final_review_reviewer_id": reviewer_id,
            "current_final_review_result": "pass",
            "current_final_review_summary_hash": summary_hash,
            "final_review_dispatch_lineage": {
                "execution_run_id": execution_run_id,
                "dispatch_id": dispatch_id,
                "branch_closure_id": branch_closure_id
            },
            "final_review_record_history": {
                "unit-final-review-record": {
                    "record_id": "unit-final-review-record",
                    "record_sequence": 1,
                    "record_status": "current",
                    "branch_closure_id": branch_closure_id,
                    "source_plan_path": context.plan_rel,
                    "source_plan_revision": context.plan_document.plan_revision,
                    "repo_slug": runtime.repo_slug,
                    "branch_name": runtime.branch_name,
                    "base_branch": base_branch,
                    "reviewed_state_id": reviewed_state_id.as_str(),
                    "dispatch_id": dispatch_id,
                    "reviewer_source": reviewer_source,
                    "reviewer_id": reviewer_id,
                    "result": "pass",
                    "final_review_fingerprint": final_review_fingerprint,
                    "browser_qa_required": false,
                    "summary": summary,
                    "summary_hash": summary_hash
                }
            }
        }))
        .expect("authoritative state fixture should serialize"),
    )
    .expect("authoritative state fixture should write");
    let fixture_payload: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("fixture should be readable"))
            .expect("fixture should deserialize after write");
    crate::execution::event_log::sync_fixture_event_log_for_tests(&state_path, &fixture_payload)
        .expect("unit test fixture sync should publish typed event replay");

    let authoritative_state = load_authoritative_transition_state(&context)
        .expect("authoritative state should load")
        .expect("authoritative state should exist");
    assert!(
        current_final_review_record_is_still_authoritative(
            &context,
            &authoritative_state,
            CurrentFinalReviewAuthorityCheck {
                branch_closure_id,
                dispatch_id,
                reviewer_source,
                reviewer_id,
                result: "pass",
                normalized_summary_hash: &summary_hash,
            },
        )
        .expect("authoritativeness check should succeed for intact artifacts")
    );

    fs::write(
        &final_review_path,
        "# Code Review Result\n\nTampered final-review receipt.\n",
    )
    .expect("tampered final-review receipt should write");

    let authoritative_state = load_authoritative_transition_state(&context)
        .expect("authoritative state should reload after final-review tamper")
        .expect("authoritative state should still exist after final-review tamper");
    assert!(
            current_final_review_record_is_still_authoritative(
                &context,
                &authoritative_state,
                CurrentFinalReviewAuthorityCheck {
                    branch_closure_id,
                    dispatch_id,
                    reviewer_source,
                    reviewer_id,
                    result: "pass",
                    normalized_summary_hash: &summary_hash,
                },
            )
            .expect("authoritativeness check should keep trusting the current record after final-review artifact tamper")
        );

    fs::write(&final_review_path, &rendered.final_review_source)
        .expect("intact final-review receipt should restore");
    fs::write(
        &rendered.reviewer_artifact_path,
        "# Code Review Result\n\nTampered reviewer artifact.\n",
    )
    .expect("tampered reviewer artifact should write");

    let authoritative_state = load_authoritative_transition_state(&context)
        .expect("authoritative state should reload")
        .expect("authoritative state should still exist");
    assert!(
            current_final_review_record_is_still_authoritative(
                &context,
                &authoritative_state,
                CurrentFinalReviewAuthorityCheck {
                    branch_closure_id,
                    dispatch_id,
                    reviewer_source,
                    reviewer_id,
                    result: "pass",
                    normalized_summary_hash: &summary_hash,
                },
            )
            .expect("authoritativeness check should keep trusting the current record after reviewer-artifact tamper")
        );
}

#[test]
fn rewrite_branch_head_bound_artifact_refuses_to_rebind_history() {
    let tempdir = TempDir::new().expect("tempdir should exist");
    let artifact = tempdir.path().join("artifact.md");
    let original = "**Head SHA:** old-head\n";
    fs::write(&artifact, original).expect("head-bound artifact fixture should write");

    let error = rewrite_branch_head_bound_artifact(&artifact, "new-head")
        .expect_err("append-only repair must not rewrite historical head-bound artifacts");

    assert_eq!(error.error_class, FailureClass::StaleProvenance.as_str());
    assert_eq!(
        fs::read_to_string(&artifact).expect("artifact should remain readable"),
        original
    );
}

#[test]
fn rewrite_branch_qa_artifact_refuses_to_rebind_history() {
    let tempdir = TempDir::new().expect("tempdir should exist");
    let qa_artifact = tempdir.path().join("qa.md");
    let test_plan = tempdir.path().join("test-plan.md");
    let original = "**Head SHA:** old-head\n**Source Test Plan:** `old-plan.md`\n";
    fs::write(&qa_artifact, original).expect("qa artifact fixture should write");
    fs::write(&test_plan, "placeholder").expect("test plan fixture should write");

    let error = rewrite_branch_qa_artifact(&qa_artifact, "new-head", &test_plan)
        .expect_err("append-only repair must not rewrite historical QA artifacts");

    assert_eq!(error.error_class, FailureClass::StaleProvenance.as_str());
    assert_eq!(
        fs::read_to_string(&qa_artifact).expect("qa artifact should remain readable"),
        original
    );
}

#[test]
fn superseded_branch_closure_ids_reports_previous_current_binding() {
    let overlay: StatusAuthoritativeOverlay = serde_json::from_value(json!({
        "current_branch_closure_id": "branch-release-closure-old"
    }))
    .expect("overlay fixture should deserialize");

    let superseded = superseded_branch_closure_ids_from_previous_current(
        Some(&overlay),
        "branch-release-closure-new",
    );

    assert_eq!(superseded, vec![String::from("branch-release-closure-old")]);
}

#[test]
fn normalized_late_stage_surface_rejects_invalid_entries() {
    for invalid in [
        "/README.md",
        "../README.md",
        "C:/README.md",
        "C:\\README.md",
        "docs/*.md",
        "docs/?",
        "docs/[a]",
        "docs/{a}",
    ] {
        let error = normalized_late_stage_surface(&format!("**Late-Stage Surface:** {invalid}\n"))
            .expect_err("invalid Late-Stage Surface entries must fail closed");
        assert_eq!(
            error.error_class,
            FailureClass::InvalidCommandInput.as_str()
        );
    }
}

#[test]
fn path_matches_late_stage_surface_distinguishes_file_and_directory_entries() {
    assert!(path_matches_late_stage_surface(
        "docs/release.md",
        &[String::from("docs/")]
    ));
    assert!(path_matches_late_stage_surface(
        "docs",
        &[String::from("docs/")]
    ));
    assert!(!path_matches_late_stage_surface(
        "docs-release.md",
        &[String::from("docs/")]
    ));
    assert!(path_matches_late_stage_surface(
        "README.md",
        &[String::from("README.md")]
    ));
    assert!(!path_matches_late_stage_surface(
        "README.md.bak",
        &[String::from("README.md")]
    ));
}

#[test]
fn path_matches_late_stage_surface_is_case_sensitive() {
    assert!(path_matches_late_stage_surface(
        "README.md",
        &[String::from("README.md")]
    ));
    assert!(!path_matches_late_stage_surface(
        "readme.md",
        &[String::from("README.md")]
    ));
}

#[test]
fn task_closure_record_covers_path_respects_directory_surface_entries() {
    let record = CurrentTaskClosureRecord {
        task: 1,
        dispatch_id: String::from("task-1-dispatch"),
        closure_record_id: String::from("task-1-closure"),
        source_plan_path: Some(String::from("docs/featureforge/plans/test-plan.md")),
        source_plan_revision: Some(1),
        execution_run_id: Some(String::from("run-1")),
        reviewed_state_id: String::from("git_tree:deadbeef"),
        semantic_reviewed_state_id: Some(String::from("semantic_tree:deadbeef")),
        contract_identity: String::from("task-1-contract"),
        effective_reviewed_surface_paths: vec![String::from("src/")],
        review_result: String::from("pass"),
        review_summary_hash: String::from("review-hash"),
        verification_result: String::from("pass"),
        verification_summary_hash: String::from("verification-hash"),
        closure_status: Some(String::from("current")),
    };

    assert!(task_closure_record_covers_path(&record, "src/lib.rs"));
    assert!(task_closure_record_covers_path(&record, "src"));
    assert!(!task_closure_record_covers_path(
        &record,
        "src-generated/lib.rs"
    ));
}

#[test]
fn blocked_follow_up_prefers_shared_repair_route_before_branch_closure_fallback() {
    let operator = ExecutionRoutingState {
        route_decision: None,
        runtime_provenance: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from("ok"),
            next_skill: String::from("featureforge:workflow"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::new(),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 0,
            plan_candidate_count: 0,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase_detail: String::from(
            crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
        ),
        review_state_status: String::from("stale_unreviewed"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::new(),
        recommended_public_command: repair_review_state_public_command("<approved-plan-path>"),
        recommended_command: Some(String::from(
            "featureforge plan execution repair-review-state --plan <approved-plan-path>",
        )),
        blocking_scope: None,
        blocking_task: None,
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };

    assert_eq!(
        blocked_follow_up_for_operator(&operator),
        Some(String::from("repair_review_state"))
    );
    assert_eq!(
        late_stage_required_follow_up("final_review", &operator),
        Some(String::from("repair_review_state"))
    );
}

#[test]
fn advance_late_stage_final_review_with_dispatch_id_requeries_when_dispatch_follow_up_is_required()
{
    let operator = ExecutionRoutingState {
        route_decision: None,
        runtime_provenance: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from("ok"),
            next_skill: String::from("featureforge:workflow"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::new(),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 0,
            plan_candidate_count: 0,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase: String::from(crate::execution::phase::PHASE_FINAL_REVIEW_PENDING),
        phase_detail: String::from(crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::new(),
        recommended_public_command: repair_review_state_public_command(
            "docs/featureforge/plans/example.md",
        ),
        recommended_command: None,
        blocking_scope: None,
        blocking_task: None,
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };

    let output = advance_late_stage_follow_up_or_requery_output(
        &operator,
        Path::new("docs/featureforge/plans/example.md"),
        false,
        AdvanceLateStageOutputContext {
            stage_path: "final_review",
            operation: "record_final_review_outcome",
            branch_closure_id: Some(String::from("branch-closure-1")),
            dispatch_id: Some(String::from("dispatch-123")),
            result: "pass",
            external_review_result_ready: true,
            trace_summary: "advance-late-stage failed closed because workflow/operator requery is required.",
        },
    );

    assert_eq!(output.action, "blocked");
    assert_eq!(
        output.code.as_deref(),
        Some("out_of_phase_requery_required")
    );
    assert_eq!(
        output.recommended_command.as_deref(),
        Some(
            "featureforge workflow operator --plan docs/featureforge/plans/example.md --external-review-result-ready",
        )
    );
    assert_eq!(
        output.recommended_public_command_argv,
        Some(vec![
            String::from("featureforge"),
            String::from("workflow"),
            String::from("operator"),
            String::from("--plan"),
            String::from("docs/featureforge/plans/example.md"),
            String::from("--external-review-result-ready"),
        ])
    );
    assert_eq!(output.rederive_via_workflow_operator, Some(true));
    assert_eq!(output.required_follow_up, None);
}

#[test]
fn advance_late_stage_final_review_with_matching_dispatch_lineage_keeps_dispatch_follow_up() {
    let operator = ExecutionRoutingState {
        route_decision: None,
        runtime_provenance: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from("ok"),
            next_skill: String::from("featureforge:workflow"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::new(),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 0,
            plan_candidate_count: 0,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase: String::from(crate::execution::phase::PHASE_FINAL_REVIEW_PENDING),
        phase_detail: String::from(crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::new(),
        recommended_public_command: repair_review_state_public_command(
            "docs/featureforge/plans/example.md",
        ),
        recommended_command: None,
        blocking_scope: None,
        blocking_task: None,
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };

    let output = advance_late_stage_follow_up_or_requery_output(
        &operator,
        Path::new("docs/featureforge/plans/example.md"),
        true,
        AdvanceLateStageOutputContext {
            stage_path: "final_review",
            operation: "record_final_review_outcome",
            branch_closure_id: Some(String::from("branch-closure-1")),
            dispatch_id: Some(String::from("dispatch-123")),
            result: "pass",
            external_review_result_ready: true,
            trace_summary: "advance-late-stage follow-up required.",
        },
    );

    assert_eq!(output.action, "blocked");
    assert_eq!(output.code, None);
    assert_eq!(output.recommended_command, None);
    assert_eq!(output.recommended_public_command_argv, None);
    assert_eq!(output.rederive_via_workflow_operator, None);
    assert_eq!(
        output.required_follow_up,
        Some(String::from("request_external_review"))
    );
}

#[test]
fn blocked_follow_up_routes_clean_execution_reentry_repair_state_to_repair_review_state() {
    let operator = ExecutionRoutingState {
        route_decision: None,
        runtime_provenance: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from("ok"),
            next_skill: String::from("featureforge:workflow"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::new(),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 0,
            plan_candidate_count: 0,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::from("repair review state / reenter execution"),
        recommended_public_command: repair_review_state_public_command(
            "docs/featureforge/plans/example.md",
        ),
        recommended_command: Some(String::from(
            "featureforge plan execution repair-review-state --plan docs/featureforge/plans/example.md",
        )),
        blocking_scope: None,
        blocking_task: None,
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };

    assert_eq!(
        blocked_follow_up_for_operator(&operator),
        Some(String::from("repair_review_state"))
    );
    assert_eq!(
        late_stage_required_follow_up("release_readiness", &operator),
        Some(String::from("repair_review_state"))
    );
    assert_eq!(
        late_stage_required_follow_up("final_review", &operator),
        Some(String::from("repair_review_state"))
    );
}

#[test]
fn close_current_task_follow_up_preserves_structural_repair_state_lane() {
    let operator = ExecutionRoutingState {
        route_decision: None,
        runtime_provenance: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from("ok"),
            next_skill: String::from("featureforge:workflow"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::new(),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 0,
            plan_candidate_count: 0,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::from("repair review state / reenter execution"),
        recommended_public_command: repair_review_state_public_command(
            "docs/featureforge/plans/example.md",
        ),
        recommended_command: Some(String::from(
            "featureforge plan execution repair-review-state --plan docs/featureforge/plans/example.md",
        )),
        blocking_scope: None,
        blocking_task: None,
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };

    assert_eq!(
        close_current_task_required_follow_up(&operator),
        Some(String::from("repair_review_state"))
    );
}

#[test]
fn close_current_task_follow_up_preserves_stale_repair_state_lane() {
    let operator = ExecutionRoutingState {
        route_decision: None,
        runtime_provenance: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from("ok"),
            next_skill: String::from("featureforge:workflow"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::new(),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 0,
            plan_candidate_count: 0,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        review_state_status: String::from("stale_unreviewed"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::from("repair review state / reenter execution"),
        recommended_public_command: repair_review_state_public_command(
            "docs/featureforge/plans/example.md",
        ),
        recommended_command: Some(String::from(
            "featureforge plan execution repair-review-state --plan docs/featureforge/plans/example.md",
        )),
        blocking_scope: None,
        blocking_task: Some(2),
        external_wait_state: None,
        blocking_reason_codes: vec![String::from("prior_task_current_closure_stale")],
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };

    assert_eq!(
        close_current_task_required_follow_up(&operator),
        Some(String::from("repair_review_state"))
    );
}

#[test]
fn close_current_task_follow_up_waits_for_external_review_result_when_task_review_is_pending() {
    let operator = ExecutionRoutingState {
        route_decision: None,
        runtime_provenance: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from("ok"),
            next_skill: String::from("featureforge:workflow"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::new(),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 0,
            plan_candidate_count: 0,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase: String::from(crate::execution::phase::PHASE_TASK_CLOSURE_PENDING),
        phase_detail: String::from(crate::execution::phase::DETAIL_TASK_REVIEW_RESULT_PENDING),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::from("wait for external review result"),
        recommended_public_command: repair_review_state_public_command(
            "docs/featureforge/plans/example.md",
        ),
        recommended_command: None,
        blocking_scope: Some(String::from("task")),
        blocking_task: Some(1),
        external_wait_state: Some(String::from("waiting_for_external_review_result")),
        blocking_reason_codes: vec![String::from("prior_task_review_not_green")],
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: Some(String::from("dispatch-task-1")),
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };

    assert_eq!(
        close_current_task_required_follow_up(&operator),
        Some(String::from("wait_for_external_review_result"))
    );
}

#[test]
fn close_current_task_follow_up_preserves_request_external_review_for_non_task_dispatch_phase() {
    let operator = ExecutionRoutingState {
        route_decision: None,
        runtime_provenance: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from("ok"),
            next_skill: String::from("featureforge:workflow"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::new(),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 0,
            plan_candidate_count: 0,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(crate::execution::phase::PHASE_FINAL_REVIEW_PENDING),
        phase: String::from(crate::execution::phase::PHASE_FINAL_REVIEW_PENDING),
        phase_detail: String::from(crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::from("request external review"),
        recommended_public_command: repair_review_state_public_command(
            "docs/featureforge/plans/example.md",
        ),
        recommended_command: None,
        blocking_scope: Some(String::from("branch")),
        blocking_task: None,
        external_wait_state: None,
        blocking_reason_codes: vec![String::from("final_review_dispatch_missing")],
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: Some(String::from("branch-closure-1")),
        current_release_readiness_result: Some(String::from("pass")),
        base_branch: None,
    };

    assert_eq!(
        close_current_task_required_follow_up(&operator),
        Some(String::from("request_external_review"))
    );
}

#[test]
fn close_current_task_follow_up_requires_verification_when_verification_is_missing() {
    let operator = ExecutionRoutingState {
        route_decision: None,
        runtime_provenance: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from("ok"),
            next_skill: String::from("featureforge:workflow"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::new(),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 0,
            plan_candidate_count: 0,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase: String::from(crate::execution::phase::PHASE_TASK_CLOSURE_PENDING),
        phase_detail: String::from(crate::execution::phase::DETAIL_TASK_REVIEW_RESULT_PENDING),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::from("wait for external review result"),
        recommended_public_command: repair_review_state_public_command(
            "docs/featureforge/plans/example.md",
        ),
        recommended_command: None,
        blocking_scope: Some(String::from("task")),
        blocking_task: Some(1),
        external_wait_state: None,
        blocking_reason_codes: vec![String::from("prior_task_verification_missing")],
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: Some(String::from("dispatch-task-1")),
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };

    assert_eq!(
        close_current_task_required_follow_up(&operator),
        Some(String::from("run_verification"))
    );
}

#[test]
fn close_current_task_outcome_class_treats_review_fail_verification_pass_as_negative() {
    assert_eq!(
        close_current_task_outcome_class(ReviewOutcomeArg::Fail, VerificationOutcomeArg::Pass),
        CloseCurrentTaskOutcomeClass::Negative
    );
}

#[test]
fn blocked_close_current_task_output_from_operator_keeps_shared_follow_up_and_command() {
    let plan_with_spaces = "docs/featureforge/plans/example plan.md";
    let operator = ExecutionRoutingState {
        route_decision: None,
        runtime_provenance: None,
        route: WorkflowRoute {
            schema_version: 3,
            status: String::from("ok"),
            next_skill: String::from("featureforge:workflow"),
            spec_path: String::new(),
            plan_path: String::new(),
            contract_state: String::new(),
            reason_codes: Vec::new(),
            diagnostics: Vec::new(),
            plan_fidelity_review: None,
            scan_truncated: false,
            spec_candidate_count: 0,
            plan_candidate_count: 0,
            manifest_path: String::new(),
            root: String::new(),
            reason: String::new(),
            note: String::new(),
        },
        execution_status: None,
        preflight: None,
        gate_review: None,
        gate_finish: None,
        workflow_phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase: String::from(crate::execution::phase::PHASE_EXECUTING),
        phase_detail: String::from(crate::execution::phase::DETAIL_EXECUTION_REENTRY_REQUIRED),
        review_state_status: String::from("clean"),
        qa_requirement: None,
        finish_review_gate_pass_branch_closure_id: None,
        recording_context: None,
        execution_command_context: None,
        next_action: String::from("repair review state / reenter execution"),
        recommended_public_command: repair_review_state_public_command(plan_with_spaces),
        recommended_command: Some(String::from(
            "featureforge plan execution repair-review-state --plan stale-display-path.md",
        )),
        blocking_scope: None,
        blocking_task: Some(3),
        external_wait_state: None,
        blocking_reason_codes: vec![String::from(
            "prior_task_current_closure_reviewed_state_malformed",
        )],
        reason_family: String::new(),
        diagnostic_reason_codes: Vec::new(),
        task_review_dispatch_id: None,
        final_review_dispatch_id: None,
        current_branch_closure_id: None,
        current_release_readiness_result: None,
        base_branch: None,
    };
    let output = blocked_close_current_task_output_from_operator(
        3,
        &operator,
        "close-current-task must return the shared blocked route when routing is out-of-phase.",
    );
    assert_eq!(output.action, "blocked");
    assert_eq!(output.code, None);
    assert_eq!(
        output.required_follow_up,
        Some(String::from("repair_review_state"))
    );
    assert_eq!(
        output.recommended_command.as_deref(),
        Some(
            "featureforge plan execution repair-review-state --plan docs/featureforge/plans/example plan.md"
        )
    );
    assert_eq!(
        output.recommended_public_command_argv,
        Some(vec![
            String::from("featureforge"),
            String::from("plan"),
            String::from("execution"),
            String::from("repair-review-state"),
            String::from("--plan"),
            String::from(plan_with_spaces),
        ])
    );
    assert_eq!(output.rederive_via_workflow_operator, None);
}
