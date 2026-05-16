use std::path::Path;

use crate::diagnostics::JsonFailure;
#[cfg(test)]
use crate::execution::context::EvidenceAttempt;
use crate::execution::context::{
    ExecutionContext, clear_projection_only_execution_progress, has_other_same_branch_worktree,
    hash_contract_plan, load_execution_context, load_execution_context_for_exact_plan,
    load_execution_context_for_mutation, overlay_execution_evidence_attempts_from_authority,
    overlay_step_state_from_authority, refresh_execution_fingerprint, same_branch_worktrees,
};
#[cfg(test)]
use crate::execution::current_truth::{
    branch_closure_refresh_missing_current_closure as shared_branch_closure_refresh_missing_current_closure,
    public_late_stage_rederivation_basis_present,
    public_late_stage_stale_unreviewed as shared_public_late_stage_stale_unreviewed,
};
#[cfg(test)]
use crate::execution::harness::DownstreamFreshnessState;
use crate::execution::implementation_gate::apply_pre_execution_plan_fidelity_gate;
#[cfg(test)]
use crate::execution::internal_args::{RecordReviewDispatchArgs, ReviewDispatchScopeArg};
use crate::execution::leases::StatusAuthoritativeOverlay;
#[cfg(test)]
use crate::execution::leases::authoritative_state_path;
#[cfg(test)]
use crate::execution::observability::REASON_CODE_STALE_PROVENANCE;
use crate::execution::phase;
use crate::execution::projection_renderer::ProjectionReadModelDetail;
use crate::execution::reducer::RuntimeState;
#[cfg(test)]
use crate::execution::reentry_reconcile::TARGETLESS_STALE_RECONCILE_REASON_CODE;
#[cfg(test)]
use crate::execution::review_route_tokens::{
    REASON_BROWSER_QA_STATE_NOT_FRESH, REASON_FINAL_REVIEW_STATE_NOT_FRESH,
    REASON_RELEASE_DOCS_STATE_NOT_FRESH,
};
#[cfg(test)]
pub(crate) use crate::execution::route_plan::execution_command_route_target_from_status_context as resolve_execution_command_route_target_from_context;
use crate::execution::runtime::ExecutionRuntime;
use crate::execution::runtime_truth::{
    derive_execution_truth_from_authority,
    derive_execution_truth_from_authority_with_projection_detail,
};
use crate::execution::stale_target_projection::targetless_stale_authority_for_gate_snapshot;
#[cfg(test)]
use crate::execution::state::record_review_dispatch_blocked_output_from_gate;
#[cfg(test)]
use crate::execution::status::GateResult;
use crate::execution::status::PlanExecutionStatus;
#[cfg(test)]
use crate::execution::status::PublicReviewStateTaskClosure;
#[cfg(test)]
use crate::execution::status_assembly::derive_public_blocking_records;
use crate::execution::transitions::{
    AuthoritativeTransitionState, load_authoritative_transition_state_relaxed,
};
#[cfg(test)]
use crate::workflow::pivot::pivot_decision_reason_codes;

mod public_route_projection;

#[cfg(test)]
use crate::execution::status_assembly::prerelease_branch_closure_refresh_required;
pub use crate::execution::status_assembly::status_from_context;
pub(crate) use crate::execution::status_assembly::{
    ExecutionReentryCurrentTaskClosureTargets, branch_closure_record_matches_plan_exemption,
    current_branch_closure_structural_review_state_reason,
    current_branch_gate_bindings_from_authoritative_state,
    execution_reentry_current_task_closure_targets_from_inputs,
    execution_reentry_requires_review_state_repair,
    execution_reentry_requires_review_state_repair_with_authority,
    final_review_dispatch_still_current_for_gates, has_authoritative_late_stage_progress,
    is_late_stage_phase, missing_derived_review_state_fields, normalize_optional_overlay_value,
    parse_harness_phase, recommended_execution_source, shared_repair_review_state_reroute_decision,
    status_workspace_state_id, task_scope_review_state_repair_reason,
    task_scope_structural_review_state_reason, usable_current_branch_closure_identity,
    usable_current_branch_closure_identity_from_authoritative_state,
    validated_current_branch_closure_identity,
    validated_current_branch_closure_identity_from_authoritative_state,
};
#[cfg(test)]
pub(crate) use crate::execution::status_assembly::{
    StatusReviewStateInputs, current_workflow_pivot_record_exists_for_status_decision,
    derive_status_review_state_fact,
};
pub(crate) use public_route_projection::{
    apply_shared_routing_projection_to_read_scope,
    apply_shared_routing_projection_to_read_scope_with_routing,
};

pub(crate) struct ExecutionReadScope {
    pub(crate) context: ExecutionContext,
    pub(crate) status: PlanExecutionStatus,
    pub(crate) overlay: Option<StatusAuthoritativeOverlay>,
    pub(crate) authoritative_state: Option<AuthoritativeTransitionState>,
    pub(crate) runtime_state: Option<RuntimeState>,
    pub(crate) projection_detail: ProjectionReadModelDetail,
}

pub(crate) fn apply_public_read_invariants_to_read_scope(read_scope: &mut ExecutionReadScope) {
    crate::execution::invariants::inject_read_surface_invariant_test_violation(
        &mut read_scope.status,
    );
    let targetless_stale_authority = read_scope.runtime_state.as_ref().map(|runtime_state| {
        targetless_stale_authority_for_gate_snapshot(&runtime_state.gate_snapshot)
    });
    crate::execution::invariants::apply_read_surface_invariants_with_targetless_authority(
        &mut read_scope.status,
        targetless_stale_authority,
    );
}

pub(crate) fn public_status_from_context_with_shared_routing(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let mut read_scope =
        load_execution_read_scope_for_mutation(runtime, Path::new(&context.plan_rel), true)?;
    apply_shared_routing_projection_to_read_scope(
        runtime,
        &mut read_scope,
        external_review_result_ready,
        true,
    )?;
    apply_pre_execution_plan_fidelity_gate(&read_scope.context, &mut read_scope.status);
    apply_public_read_invariants_to_read_scope(&mut read_scope);
    Ok(read_scope.status)
}

pub(crate) fn public_status_from_supplied_context_with_shared_routing(
    context: &ExecutionContext,
    external_review_result_ready: bool,
) -> Result<PlanExecutionStatus, JsonFailure> {
    let mut context = context.clone();
    let authoritative_state = load_authoritative_transition_state_relaxed(&context)?;
    overlay_execution_evidence_attempts_from_authority(&mut context, authoritative_state.as_ref())?;
    overlay_step_state_from_authority(&mut context, authoritative_state.as_ref())?;
    refresh_execution_fingerprint(&mut context);
    let derived = derive_execution_truth_from_authority(&context, authoritative_state.as_ref())?;
    let mut read_scope = ExecutionReadScope {
        context,
        status: derived.status,
        overlay: derived.overlay,
        authoritative_state,
        runtime_state: None,
        projection_detail: ProjectionReadModelDetail::Full,
    };
    apply_shared_routing_projection_to_read_scope_with_routing(
        &mut read_scope,
        external_review_result_ready,
        true,
    )?;
    apply_pre_execution_plan_fidelity_gate(&read_scope.context, &mut read_scope.status);
    apply_public_read_invariants_to_read_scope(&mut read_scope);
    Ok(read_scope.status)
}

pub(crate) fn load_execution_read_scope(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
    exact_plan_override: bool,
) -> Result<ExecutionReadScope, JsonFailure> {
    let context = load_execution_read_context(runtime, plan_path, exact_plan_override)?;
    finalize_execution_read_scope(
        runtime,
        exact_plan_override,
        context,
        ProjectionReadModelDetail::Full,
    )
}

pub(crate) fn load_execution_read_scope_for_mutation(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
    exact_plan_override: bool,
) -> Result<ExecutionReadScope, JsonFailure> {
    let context = load_execution_context_for_mutation(runtime, plan_path)?;
    finalize_execution_read_scope(
        runtime,
        exact_plan_override,
        context,
        ProjectionReadModelDetail::RuntimeDecision,
    )
}

fn finalize_execution_read_scope(
    runtime: &ExecutionRuntime,
    exact_plan_override: bool,
    mut context: ExecutionContext,
    projection_detail: ProjectionReadModelDetail,
) -> Result<ExecutionReadScope, JsonFailure> {
    let authoritative_state = load_authoritative_transition_state_relaxed(&context)?;
    overlay_execution_evidence_attempts_from_authority(&mut context, authoritative_state.as_ref())?;
    overlay_step_state_from_authority(&mut context, authoritative_state.as_ref())?;
    refresh_execution_fingerprint(&mut context);
    let derived = derive_execution_truth_from_authority_with_projection_detail(
        &context,
        authoritative_state.as_ref(),
        projection_detail,
    )?;
    let overlay = derived.overlay;
    let mut status = derived.status;
    let local_contract_plan_fingerprint = hash_contract_plan(&context.plan_source);
    let local_evidence_progress_present = context.evidence.tracked_progress_present;
    let local_projection_only_execution_started =
        status.execution_started == "yes" && !context.local_execution_progress_markers_present;
    let local_has_other_same_branch_worktree = has_other_same_branch_worktree(runtime);
    let local_started_execution = status.execution_started == "yes";
    let local_probe = LocalSameBranchReadScopeProbe {
        plan_rel: &context.plan_rel,
        contract_plan_fingerprint: &local_contract_plan_fingerprint,
        evidence_progress_present: local_evidence_progress_present,
        projection_only_execution_started: local_projection_only_execution_started,
        started_execution: local_started_execution,
        semantic_workspace_state_id: &status_workspace_state_id(&context)?,
    };
    let read_scope = if let Some(adopted_scope) =
        started_execution_read_scope_from_same_branch_worktree(
            runtime,
            local_probe,
            exact_plan_override,
            projection_detail,
        )? {
        adopted_scope
    } else {
        if local_started_execution
            && local_projection_only_execution_started
            && local_has_other_same_branch_worktree
        {
            clear_projection_only_execution_progress(&mut context);
            refresh_execution_fingerprint(&mut context);
            status = derive_execution_truth_from_authority_with_projection_detail(
                &context,
                None,
                projection_detail,
            )?
            .status;
            normalize_non_started_same_branch_status(&mut status);
            return Ok(ExecutionReadScope {
                context,
                status,
                overlay: None,
                authoritative_state: None,
                runtime_state: None,
                projection_detail,
            });
        }
        if local_has_other_same_branch_worktree {
            normalize_non_started_same_branch_status(&mut status);
        }
        ExecutionReadScope {
            context,
            status,
            overlay,
            authoritative_state,
            runtime_state: None,
            projection_detail,
        }
    };
    Ok(read_scope)
}

fn normalize_non_started_same_branch_status(status: &mut PlanExecutionStatus) {
    if status.execution_started == "yes"
        && status.phase_detail == phase::DETAIL_EXECUTION_IN_PROGRESS
    {
        status.execution_started = String::from("no");
        status.active_task = None;
        status.active_step = None;
        status.resume_task = None;
        status.resume_step = None;
    }
}

fn load_execution_read_context(
    runtime: &ExecutionRuntime,
    plan_path: &Path,
    exact_plan_override: bool,
) -> Result<ExecutionContext, JsonFailure> {
    if exact_plan_override {
        load_execution_context_for_exact_plan(runtime, plan_path)
    } else {
        load_execution_context(runtime, plan_path)
    }
}

struct LocalSameBranchReadScopeProbe<'a> {
    plan_rel: &'a str,
    contract_plan_fingerprint: &'a str,
    evidence_progress_present: bool,
    projection_only_execution_started: bool,
    started_execution: bool,
    semantic_workspace_state_id: &'a str,
}

fn started_execution_read_scope_from_same_branch_worktree(
    current_runtime: &ExecutionRuntime,
    local_probe: LocalSameBranchReadScopeProbe<'_>,
    exact_plan_override: bool,
    projection_detail: ProjectionReadModelDetail,
) -> Result<Option<ExecutionReadScope>, JsonFailure> {
    if local_probe.started_execution && !local_probe.projection_only_execution_started {
        return Ok(None);
    }
    if local_probe.evidence_progress_present {
        return Ok(None);
    }
    let relative_plan = Path::new(local_probe.plan_rel);
    Ok(same_branch_worktrees(&current_runtime.repo_root)
        .into_iter()
        .filter(|root| root != &current_runtime.repo_root)
        .find_map(|worktree_root| {
            let discovered_runtime = ExecutionRuntime::discover(&worktree_root).ok()?;
            if current_runtime.branch_name == "current"
                || discovered_runtime.branch_name == "current"
                || discovered_runtime.branch_name != current_runtime.branch_name
            {
                return None;
            }
            let runtime = ExecutionRuntime {
                state_dir: current_runtime.state_dir.clone(),
                ..discovered_runtime
            };
            let mut context =
                load_execution_read_context(&runtime, relative_plan, exact_plan_override).ok()?;
            if hash_contract_plan(&context.plan_source) != local_probe.contract_plan_fingerprint {
                return None;
            }
            let authoritative_state = load_authoritative_transition_state_relaxed(&context).ok()?;
            overlay_step_state_from_authority(&mut context, authoritative_state.as_ref()).ok()?;
            let derived = derive_execution_truth_from_authority_with_projection_detail(
                &context,
                authoritative_state.as_ref(),
                projection_detail,
            )
            .ok()?;
            let semantic_workspace_state_id = status_workspace_state_id(&context).ok()?;
            (derived.status.execution_started == "yes"
                && semantic_workspace_state_id == local_probe.semantic_workspace_state_id)
                .then_some(ExecutionReadScope {
                    context,
                    status: derived.status,
                    overlay: derived.overlay,
                    authoritative_state,
                    runtime_state: None,
                    projection_detail,
                })
        }))
}

#[cfg(test)]
mod execution_command_route_target_tests {
    use super::*;
    use crate::execution::harness::HarnessPhase;
    use crate::execution::reentry_reconcile::TargetlessStaleReconcile;
    use crate::test_support::init_committed_test_repo;
    use serde_json::Value;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn unresolved_execution_context() -> (TempDir, ExecutionContext, String) {
        let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/codex-runtime/fixtures/workflow-artifacts");
        let repo_dir = TempDir::new().expect("exact-command temp repo should exist");
        let repo_root = repo_dir.path();
        let plan_rel =
            String::from("docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md");
        let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
        let plan_path = repo_root.join(&plan_rel);
        let spec_path = repo_root.join(spec_rel);

        init_committed_test_repo(
            repo_root,
            "# exact-command-test\n",
            "exact-command unit tests",
        );

        fs::create_dir_all(
            spec_path
                .parent()
                .expect("spec fixture path should have a parent"),
        )
        .expect("spec fixture directory should create");
        fs::create_dir_all(
            plan_path
                .parent()
                .expect("plan fixture path should have a parent"),
        )
        .expect("plan fixture directory should create");
        fs::copy(
            fixture_root.join("specs/2026-03-22-runtime-integration-hardening-design.md"),
            &spec_path,
        )
        .expect("exact-command unit-test spec fixture should copy");
        let plan_source = fs::read_to_string(
            fixture_root.join("plans/2026-03-22-runtime-integration-hardening.md"),
        )
        .expect("exact-command unit-test plan fixture should read")
        .replace(
            "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md",
            spec_rel,
        );
        fs::write(&plan_path, plan_source)
            .expect("exact-command unit-test plan fixture should write");

        let runtime =
            ExecutionRuntime::discover(repo_root).expect("temp repo runtime should discover");
        let context = load_execution_context(&runtime, Path::new(&plan_rel))
            .expect("runtime integration hardening plan should load for exact-command unit tests");
        (repo_dir, context, plan_rel)
    }

    fn closure_baseline_candidate_context() -> (TempDir, ExecutionContext, String) {
        let (repo_dir, mut context, plan_rel) = unresolved_execution_context();
        for step in &mut context.steps {
            if step.task_number == 1 {
                step.checked = true;
            }
        }
        let head_sha = context
            .current_head_sha()
            .expect("closure-baseline candidate fixture should resolve head sha");
        context.evidence.attempts = context
            .steps
            .iter()
            .filter(|step| step.task_number == 1)
            .map(|step| EvidenceAttempt {
                task_number: step.task_number,
                step_number: step.step_number,
                attempt_number: 1,
                status: String::from("Completed"),
                recorded_at: String::from("2026-04-19T00:00:00Z"),
                execution_source: String::from("featureforge:executing-plans"),
                claim: format!(
                    "closure-baseline candidate fixture completed task {} step {}",
                    step.task_number, step.step_number
                ),
                files: Vec::new(),
                file_proofs: Vec::new(),
                verify_command: None,
                verification_summary: String::from("closure-baseline candidate fixture"),
                invalidation_reason: String::new(),
                packet_fingerprint: Some(format!(
                    "packet-fingerprint-task-{}-step-{}",
                    step.task_number, step.step_number
                )),
                head_sha: Some(head_sha.clone()),
                base_sha: Some(head_sha.clone()),
                source_contract_path: None,
                source_contract_fingerprint: None,
                source_evaluation_report_fingerprint: None,
                evaluator_verdict: None,
                failing_criterion_ids: Vec::new(),
                source_handoff_fingerprint: None,
                repo_state_baseline_head_sha: None,
                repo_state_baseline_worktree_fingerprint: None,
            })
            .collect();
        let authoritative_state_path = authoritative_state_path(&context);
        fs::create_dir_all(
            authoritative_state_path
                .parent()
                .expect("authoritative state path should have a parent"),
        )
        .expect("authoritative state directory should create");
        fs::write(
            &authoritative_state_path,
            serde_json::json!({
                "last_strategy_checkpoint_fingerprint": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "run_identity": {
                    "execution_run_id": "run-exact-phase-detail"
                },
                "task_closure_record_history": {
                    "task-closure-1-historical": {
                        "task": 1,
                        "record_status": "historical"
                    }
                }
            })
            .to_string(),
        )
        .expect("authoritative state for closure-baseline candidate should write");
        (repo_dir, context, plan_rel)
    }

    fn shared_route_projection_for_status(
        context: &ExecutionContext,
        status: PlanExecutionStatus,
    ) -> (
        PlanExecutionStatus,
        crate::execution::route_plan::RouteDecision,
    ) {
        let authoritative_state = load_authoritative_transition_state_relaxed(context)
            .expect("route-projection test authoritative state should load");
        let mut read_scope = ExecutionReadScope {
            context: context.clone(),
            status,
            overlay: None,
            authoritative_state,
            runtime_state: None,
            projection_detail: ProjectionReadModelDetail::Full,
        };
        let (_routing, route_decision) =
            apply_shared_routing_projection_to_read_scope_with_routing(
                &mut read_scope,
                false,
                false,
            )
            .expect("shared route projection should derive public route for test status");
        (read_scope.status, route_decision)
    }

    fn late_stage_status_for_review_state_tests() -> PlanExecutionStatus {
        let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
        let mut status =
            status_from_context(&context).expect("status should derive for review-state tests");
        status.execution_started = String::from("yes");
        status.harness_phase = HarnessPhase::FinalReviewPending;
        status.current_branch_closure_id = Some(String::from("branch-closure-1"));
        status
    }

    #[test]
    fn branch_closure_refresh_missing_current_closure_uses_meaningful_drift_not_raw_id_mismatch() {
        let mut status = late_stage_status_for_review_state_tests();
        status.current_branch_reviewed_state_id = Some(String::from("git_tree:baseline"));
        status.workspace_state_id = String::from("git_tree:current");
        status.current_release_readiness_state = None;

        status.current_branch_meaningful_drift = false;
        assert!(
            !shared_branch_closure_refresh_missing_current_closure(&status),
            "raw reviewed/workspace state-id mismatch without meaningful filtered drift must not trigger branch-closure refresh"
        );

        status.current_branch_meaningful_drift = true;
        assert!(
            shared_branch_closure_refresh_missing_current_closure(&status),
            "branch-closure refresh should trigger when meaningful filtered drift is present"
        );
    }

    #[test]
    fn prerelease_branch_closure_refresh_requires_meaningful_drift_signal() {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::DocumentReleasePending;
        status.current_branch_reviewed_state_id = Some(String::from("git_tree:baseline"));
        status.workspace_state_id = String::from("git_tree:current");
        status.current_release_readiness_state = None;

        status.current_branch_meaningful_drift = false;
        assert!(
            !prerelease_branch_closure_refresh_required(&status),
            "DocumentReleasePending must not require branch closure refresh when only raw reviewed/workspace mismatch is present"
        );

        status.current_branch_meaningful_drift = true;
        assert!(
            prerelease_branch_closure_refresh_required(&status),
            "DocumentReleasePending should require branch closure refresh when meaningful filtered drift is present"
        );
    }

    #[test]
    fn derive_public_blocking_records_ignores_derived_overlay_freshness() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("clean");
        status.reason_codes = vec![String::from(
            crate::execution::review_route_tokens::REASON_DERIVED_REVIEW_STATE_MISSING,
        )];
        status.projection_diagnostics = status.reason_codes.clone();

        let blocking_records = derive_public_blocking_records(&status, &clean_gate_result());

        assert!(
            blocking_records.is_empty(),
            "derived overlay freshness must remain diagnostic-only and not create public blockers: {blocking_records:?}"
        );
    }

    fn gate_result_with_reason(reason_code: &str) -> GateResult {
        GateResult {
            allowed: false,
            action: String::from("blocked"),
            failure_class: String::from("StaleProvenance"),
            reason_codes: vec![reason_code.to_owned()],
            warning_codes: Vec::new(),
            diagnostics: Vec::new(),
            code: None,
            workspace_state_id: None,
            current_branch_reviewed_state_id: None,
            current_branch_closure_id: None,
            finish_review_gate_pass_branch_closure_id: None,
            recommended_command: None,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: None,
        }
    }

    fn clean_gate_result() -> GateResult {
        GateResult {
            allowed: true,
            action: String::from("allowed"),
            failure_class: String::new(),
            reason_codes: Vec::new(),
            warning_codes: Vec::new(),
            diagnostics: Vec::new(),
            code: None,
            workspace_state_id: None,
            current_branch_reviewed_state_id: None,
            current_branch_closure_id: None,
            finish_review_gate_pass_branch_closure_id: None,
            recommended_command: None,
            recommended_public_command_template: None,
            required_inputs: Vec::new(),
            rederive_via_workflow_operator: None,
        }
    }

    fn status_review_inputs(
        repair_follow_up_requires_execution_reentry: bool,
        repair_follow_up_records_branch_closure: bool,
        branch_scope_stale_unreviewed: bool,
        task_boundary_unresolved_stale: bool,
    ) -> StatusReviewStateInputs {
        StatusReviewStateInputs {
            repair_follow_up_requires_execution_reentry,
            repair_follow_up_records_branch_closure,
            branch_scope_stale_unreviewed,
            task_boundary_unresolved_stale,
        }
    }

    #[test]
    fn resolve_execution_command_route_target_from_context_uses_first_unchecked_step_without_markers()
     {
        let (_repo_dir, context, plan_rel) = unresolved_execution_context();
        let mut status =
            status_from_context(&context).expect("status should derive for exact-command test");
        status.execution_started = String::from("yes");
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
        status.harness_phase = HarnessPhase::Executing;
        status.execution_mode = String::from("featureforge:executing-plans");

        let resolved = resolve_execution_command_route_target_from_context(
            &context,
            &status,
            plan_rel.as_str(),
        )
        .expect("marker-free started execution should derive the first unchecked step");

        assert_eq!(resolved.command_kind(), "begin");
        assert_eq!(resolved.task_number, 1);
        assert_eq!(resolved.step_id, Some(1));
    }

    #[test]
    fn resolve_execution_command_route_target_from_context_fails_closed_for_malformed_active_marker()
     {
        let (_repo_dir, context, plan_rel) = unresolved_execution_context();
        let mut status =
            status_from_context(&context).expect("status should derive for exact-command test");
        status.execution_started = String::from("yes");
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
        status.harness_phase = HarnessPhase::Executing;
        status.active_task = Some(1);
        status.active_step = None;

        assert!(
            resolve_execution_command_route_target_from_context(
                &context,
                &status,
                plan_rel.as_str()
            )
            .is_none(),
            "malformed active execution markers must fail closed instead of synthesizing a begin command"
        );
    }

    mod exact_route_tests;

    #[test]
    fn derive_status_review_state_fact_keeps_not_fresh_late_gate_reasons_diagnostic_only() {
        for reason_code in [
            REASON_RELEASE_DOCS_STATE_NOT_FRESH,
            REASON_FINAL_REVIEW_STATE_NOT_FRESH,
            REASON_BROWSER_QA_STATE_NOT_FRESH,
        ] {
            let status = late_stage_status_for_review_state_tests();
            let gate_review = gate_result_with_reason(reason_code);
            let gate_finish = gate_result_with_reason(reason_code);
            assert_eq!(
                derive_status_review_state_fact(
                    &status,
                    &gate_review,
                    &gate_finish,
                    &status_review_inputs(false, false, false, false),
                ),
                "clean",
                "late-stage reason code `{reason_code}` must stay diagnostic-only and not classify as stale_unreviewed",
            );
        }
    }

    #[test]
    fn derive_status_review_state_fact_keeps_dispatch_stale_projection_diagnostic_only() {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.current_branch_closure_id = None;
        status.blocking_task = Some(1);
        status.blocking_step = None;
        status.active_task = None;
        status.resume_task = None;
        status.reason_codes = vec![String::from(
            crate::execution::closure_diagnostics::TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_STALE,
        )];

        assert_eq!(
            derive_status_review_state_fact(
                &status,
                &clean_gate_result(),
                &clean_gate_result(),
                &status_review_inputs(false, false, false, false),
            ),
            "clean",
            "stale dispatch lineage is a projection diagnostic and must not classify task scope as stale_unreviewed",
        );
    }

    #[test]
    fn derive_status_review_state_fact_treats_control_plane_late_gate_reasons_as_stale_unreviewed()
    {
        for reason_code in [
            "review_artifact_worktree_dirty",
            "post_review_repo_write_detected",
            "files_proven_drifted",
        ] {
            let status = late_stage_status_for_review_state_tests();
            let gate_review = gate_result_with_reason(reason_code);
            let gate_finish = gate_result_with_reason(reason_code);
            assert_eq!(
                derive_status_review_state_fact(
                    &status,
                    &gate_review,
                    &gate_finish,
                    &status_review_inputs(false, false, false, false),
                ),
                crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
                "control-plane reason code `{reason_code}` must still classify as stale_unreviewed",
            );
        }
    }

    #[test]
    fn derive_status_review_state_fact_ignores_late_stage_staleness_during_execution_reentry() {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.resume_task = Some(1);
        status.resume_step = Some(1);
        status.current_branch_closure_id = None;

        let gate_review = gate_result_with_reason(REASON_RELEASE_DOCS_STATE_NOT_FRESH);
        let gate_finish = gate_result_with_reason(REASON_RELEASE_DOCS_STATE_NOT_FRESH);

        assert_eq!(
            derive_status_review_state_fact(
                &status,
                &gate_review,
                &gate_finish,
                &status_review_inputs(false, false, false, false),
            ),
            "clean",
            "late-stage stale gate reasons must not override task-scope execution reentry truth",
        );
    }

    #[test]
    fn derive_status_review_state_fact_marks_resumed_late_stage_reroute_as_stale_unreviewed() {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.resume_task = Some(1);
        status.resume_step = Some(1);
        status.current_branch_closure_id = None;
        status
            .reason_codes
            .push(String::from(REASON_CODE_STALE_PROVENANCE));

        let gate_review = gate_result_with_reason(REASON_RELEASE_DOCS_STATE_NOT_FRESH);
        let gate_finish = gate_result_with_reason(REASON_RELEASE_DOCS_STATE_NOT_FRESH);

        assert_eq!(
            derive_status_review_state_fact(
                &status,
                &gate_review,
                &gate_finish,
                &status_review_inputs(false, false, false, false),
            ),
            crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            "a resumed task rerouted out of late-stage phase must require review-state repair",
        );
    }

    #[test]
    fn derive_status_review_state_fact_keeps_document_freshness_diagnostic_even_when_harness_phase_stays_executing()
     {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.current_release_readiness_state = Some(String::from("ready"));

        let gate_review = gate_result_with_reason(REASON_RELEASE_DOCS_STATE_NOT_FRESH);
        let gate_finish = gate_result_with_reason(REASON_RELEASE_DOCS_STATE_NOT_FRESH);

        assert_eq!(
            derive_status_review_state_fact(
                &status,
                &gate_review,
                &gate_finish,
                &status_review_inputs(false, false, false, false),
            ),
            "clean",
            "late-stage document freshness diagnostics must not become stale route truth even if harness phase lags in executing",
        );
    }

    #[test]
    fn derive_status_review_state_fact_keeps_late_stage_stale_provenance_diagnostic_after_authoritative_closure()
     {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.current_branch_reviewed_state_id = status.raw_workspace_tree_id.clone();
        status.current_task_closures = vec![PublicReviewStateTaskClosure {
            task: 1,
            closure_record_id: String::from("task-closure-current"),
            reviewed_state_id: String::from("git_tree:current"),
            contract_identity: String::from("task-contract-1"),
            effective_reviewed_surface_paths: vec![String::from("README.md")],
        }];
        status
            .reason_codes
            .push(String::from(REASON_CODE_STALE_PROVENANCE));
        status.release_docs_state = DownstreamFreshnessState::Fresh;
        status.final_review_state = DownstreamFreshnessState::Fresh;
        status.browser_qa_state = DownstreamFreshnessState::Fresh;

        assert_eq!(
            derive_status_review_state_fact(
                &status,
                &gate_result_with_reason("plan_fingerprint_mismatch"),
                &gate_result_with_reason("plan_fingerprint_mismatch"),
                &status_review_inputs(true, false, false, false),
            ),
            "clean",
            "late-stage stale provenance must remain diagnostic once authoritative task and branch closure state exists",
        );
    }

    #[test]
    fn derive_status_review_state_fact_preserves_real_stale_closure_with_stale_provenance() {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.current_branch_reviewed_state_id = status.raw_workspace_tree_id.clone();
        status.current_task_closures = vec![PublicReviewStateTaskClosure {
            task: 1,
            closure_record_id: String::from("task-closure-current"),
            reviewed_state_id: String::from("git_tree:current"),
            contract_identity: String::from("task-contract-1"),
            effective_reviewed_surface_paths: vec![String::from("README.md")],
        }];
        status
            .stale_unreviewed_closures
            .push(String::from("task-closure-stale"));
        status.blocking_task = Some(1);
        status
            .reason_codes
            .push(String::from(REASON_CODE_STALE_PROVENANCE));
        status.release_docs_state = DownstreamFreshnessState::Fresh;
        status.final_review_state = DownstreamFreshnessState::Fresh;
        status.browser_qa_state = DownstreamFreshnessState::Fresh;

        assert_eq!(
            derive_status_review_state_fact(
                &status,
                &gate_result_with_reason("plan_fingerprint_mismatch"),
                &gate_result_with_reason("plan_fingerprint_mismatch"),
                &status_review_inputs(true, false, false, true),
            ),
            crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED,
            "real stale closure targets must stay stale_unreviewed even when stale provenance is otherwise diagnostic",
        );
    }

    #[test]
    fn public_late_stage_stale_unreviewed_requires_bound_late_stage_target_ids() {
        let mut status = late_stage_status_for_review_state_tests();
        status.current_branch_closure_id = None;
        status.finish_review_gate_pass_branch_closure_id = None;
        status.current_final_review_branch_closure_id = None;
        status.current_final_review_result = None;
        status.current_qa_branch_closure_id = None;
        status.current_qa_result = None;
        status.final_review_state = DownstreamFreshnessState::Stale;

        let gate_review = gate_result_with_reason(REASON_FINAL_REVIEW_STATE_NOT_FRESH);
        let gate_finish = gate_result_with_reason(REASON_FINAL_REVIEW_STATE_NOT_FRESH);

        assert!(
            public_late_stage_rederivation_basis_present(&status),
            "fixture should still surface late-stage informational basis even after bound target ids are cleared"
        );
        assert!(
            !shared_public_late_stage_stale_unreviewed(
                &status,
                Some(&gate_review),
                Some(&gate_finish),
            ),
            "late-stage stale routing must not activate when no branch/final-review/qa binding ids remain"
        );
    }

    #[test]
    fn derive_status_review_state_fact_ignores_unbound_late_stage_staleness_after_current_task_closure_refresh()
     {
        let mut status = late_stage_status_for_review_state_tests();
        status.harness_phase = HarnessPhase::Executing;
        status.current_branch_closure_id = None;
        status.finish_review_gate_pass_branch_closure_id = None;
        status.current_final_review_branch_closure_id = None;
        status.current_final_review_result = None;
        status.current_qa_branch_closure_id = None;
        status.current_qa_result = None;
        status.final_review_state = DownstreamFreshnessState::Stale;
        status.current_task_closures = vec![PublicReviewStateTaskClosure {
            task: 1,
            closure_record_id: String::from("task-closure-current"),
            reviewed_state_id: String::from("git_tree:current"),
            contract_identity: String::from("task-contract-1"),
            effective_reviewed_surface_paths: vec![String::from("README.md")],
        }];

        let gate_review = gate_result_with_reason(REASON_FINAL_REVIEW_STATE_NOT_FRESH);
        let gate_finish = gate_result_with_reason(REASON_FINAL_REVIEW_STATE_NOT_FRESH);

        assert_eq!(
            derive_status_review_state_fact(
                &status,
                &gate_review,
                &gate_finish,
                &status_review_inputs(false, false, false, false),
            ),
            "clean",
            "unbound late-stage stale signals must remain informational once the current task closure is refreshed and no late-stage binding ids remain"
        );
    }

    #[test]
    fn task_scope_review_state_repair_reason_prefers_structural_current_closure_failures() {
        let mut status = late_stage_status_for_review_state_tests();
        status.reason_codes = vec![
            String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_STALE),
            String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID),
        ];

        assert_eq!(
            task_scope_review_state_repair_reason(&status),
            Some(crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID)
        );
        assert_eq!(
            task_scope_structural_review_state_reason(&status),
            Some(crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_INVALID)
        );
    }

    #[test]
    fn derive_public_blocking_records_includes_follow_up_for_finish_checkpoint_blocker() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from(phase::DETAIL_FINISH_COMPLETION_GATE_READY);
        let gate_finish = gate_result_with_reason("finish_review_gate_checkpoint_missing");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);
        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            "finish_review_gate_checkpoint_missing"
        );
        assert_eq!(
            blocking_records[0].required_follow_up,
            Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE
            )),
            "finish-checkpoint blockers should expose a concrete public follow-up lane",
        );
    }

    #[test]
    fn record_review_dispatch_blocked_output_uses_shared_out_of_phase_contract_when_requery_is_required()
     {
        let (_repo_dir, context, plan_rel) = unresolved_execution_context();
        let args = RecordReviewDispatchArgs {
            plan: PathBuf::from(&plan_rel),
            scope: ReviewDispatchScopeArg::Task,
            task: Some(1),
        };
        let gate = gate_result_with_reason("task_closure_not_recording_ready");

        let output = record_review_dispatch_blocked_output_from_gate(&context, &args, gate);
        let output_json =
            serde_json::to_value(output).expect("review-dispatch output should serialize");

        assert_eq!(
            output_json["code"],
            Value::from(crate::execution::review_route_tokens::OUT_OF_PHASE_REQUERY_REQUIRED_CODE)
        );
        assert!(
            output_json["recommended_command"].is_null(),
            "out-of-phase requery should not expose a nested display-only command string: {output_json}"
        );
        assert_eq!(
            output_json["rederive_via_workflow_operator"],
            Value::Bool(true)
        );
    }

    #[test]
    fn derive_public_blocking_records_omits_task_review_dispatch_required_lane() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from(phase::DETAIL_TASK_REVIEW_DISPATCH_REQUIRED);
        status.blocking_task = Some(2);
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);
        assert!(
            blocking_records.is_empty(),
            "task-review dispatch projection lineage is diagnostic-only and must not create public blockers: {blocking_records:?}"
        );
    }

    #[test]
    fn derive_public_blocking_records_routes_targetless_stale_to_runtime_diagnostic() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status =
            String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED);
        status.stale_unreviewed_closures.clear();
        status.current_branch_closure_id = None;
        status.finish_review_gate_pass_branch_closure_id = None;
        status.current_final_review_branch_closure_id = None;
        status.current_final_review_result = None;
        status.current_qa_branch_closure_id = None;
        status.current_qa_result = None;
        status.current_task_closures.clear();
        status.reason_codes.clear();
        status.blocking_task = None;
        status.phase_detail = String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
        TargetlessStaleReconcile::ensure_status_diagnostic(&mut status);
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);

        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            TARGETLESS_STALE_RECONCILE_REASON_CODE
        );
        assert_eq!(blocking_records[0].scope_type, "runtime");
        assert_eq!(blocking_records[0].scope_key, "targetless_stale_unreviewed");
        assert_eq!(blocking_records[0].record_id, None);
        assert_eq!(blocking_records[0].required_follow_up, None);
    }

    #[test]
    fn derive_public_blocking_records_never_fabricates_current_branch_for_targetless_stale() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status =
            String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED);
        status.stale_unreviewed_closures.clear();
        status.current_branch_closure_id = Some(String::from("branch-closure-current"));
        status.current_task_closures.clear();
        status.reason_codes.clear();
        status.phase_detail = String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
        TargetlessStaleReconcile::ensure_status_diagnostic(&mut status);
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);

        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            TARGETLESS_STALE_RECONCILE_REASON_CODE
        );
        assert_eq!(blocking_records[0].scope_type, "runtime");
        assert_eq!(blocking_records[0].scope_key, "targetless_stale_unreviewed");
        assert_eq!(blocking_records[0].record_id, None);
        assert!(
            blocking_records
                .iter()
                .all(|record| record.scope_key != "current"
                    && record.record_id.as_deref() != Some("current")
                    && record.record_id.as_deref() != Some("branch-closure-current")),
            "targetless stale records must not invent current or branch targets: {blocking_records:?}"
        );
    }

    #[test]
    fn derive_public_blocking_records_targetless_stale_preempts_derived_current_fallback() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status =
            String::from(crate::execution::review_route_tokens::REVIEW_STATE_STALE_UNREVIEWED);
        status.stale_unreviewed_closures.clear();
        status.current_branch_closure_id = None;
        status.current_task_closures.clear();
        status.reason_codes = vec![String::from("derived_review_state_missing")];
        status.phase_detail = String::from(phase::DETAIL_RUNTIME_RECONCILE_REQUIRED);
        TargetlessStaleReconcile::ensure_status_diagnostic(&mut status);
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);

        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            TARGETLESS_STALE_RECONCILE_REASON_CODE
        );
        assert_eq!(blocking_records[0].scope_type, "runtime");
        assert_eq!(blocking_records[0].scope_key, "targetless_stale_unreviewed");
        assert_eq!(blocking_records[0].record_id, None);
        assert_eq!(blocking_records[0].required_follow_up, None);
    }

    #[test]
    fn shared_route_projection_allows_close_current_task_when_baseline_candidate_lacks_dispatch() {
        let (_repo_dir, context, _plan_rel) = closure_baseline_candidate_context();
        let mut status = status_from_context(&context)
            .expect("status should derive for task-closure baseline candidate phase-detail test");
        status.execution_started = String::from("yes");
        status.harness_phase = HarnessPhase::Executing;
        status.review_state_status = String::from("clean");
        status.current_task_closures.clear();
        status.reason_codes = vec![
            String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE),
            String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_MISSING),
        ];
        status.blocking_task = Some(1);
        status.blocking_step = None;

        let (projected_status, route_decision) =
            shared_route_projection_for_status(&context, status);
        assert_eq!(
            route_decision.phase_detail,
            phase::DETAIL_TASK_CLOSURE_RECORDING_READY,
            "task-closure baseline repair candidates should route directly to closure recording when dispatch lineage can be derived by close-current-task",
        );
        assert_eq!(
            route_decision.next_action,
            crate::execution::next_action::NEXT_ACTION_CLOSE_CURRENT_TASK,
            "task-closure baseline repair candidates should keep next_action on close-current-task",
        );
        assert_eq!(projected_status.phase_detail, route_decision.phase_detail);
        assert_eq!(projected_status.next_action, route_decision.next_action);
        assert_eq!(
            projected_status
                .recommended_public_command_template
                .as_ref()
                .map(|template| template.command_kind.as_str()),
            Some("close_current_task"),
            "input-required closure recording routes should expose the typed public command template"
        );
    }

    #[test]
    fn shared_route_projection_keeps_close_current_task_lane_for_verification_pending_baseline_repair()
     {
        let (_repo_dir, context, _plan_rel) = closure_baseline_candidate_context();
        let mut status = status_from_context(&context)
            .expect("status should derive for verification-pending closure routing test");
        status.execution_started = String::from("yes");
        status.harness_phase = HarnessPhase::Executing;
        status.review_state_status = String::from("clean");
        status.blocking_task = Some(1);
        status.blocking_step = None;
        status.current_task_closures.clear();
        status.reason_codes = vec![
            String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING),
            String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE),
            String::from(crate::execution::closure_diagnostics::TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_VERIFICATION_MISSING),
        ];

        let (projected_status, route_decision) =
            shared_route_projection_for_status(&context, status);
        assert_eq!(
            route_decision.phase_detail,
            phase::DETAIL_TASK_CLOSURE_RECORDING_READY,
            "verification-pending missing-baseline routes must stay on close-current-task so the mutation guard can return the exact verification follow-up"
        );
        assert_eq!(
            route_decision.next_action,
            crate::execution::next_action::NEXT_ACTION_CLOSE_CURRENT_TASK,
            "verification-pending missing-baseline routes must keep next_action on close-current-task"
        );
        assert_eq!(projected_status.phase_detail, route_decision.phase_detail);
        assert_eq!(projected_status.next_action, route_decision.next_action);
        assert_eq!(
            projected_status
                .recommended_public_command_template
                .as_ref()
                .map(|template| template.command_kind.as_str()),
            Some("close_current_task"),
            "verification-pending baseline repairs should expose the close-current-task public command template so the mutation guard can return exact review/verification follow-up"
        );
    }

    #[test]
    fn derive_public_blocking_records_includes_qa_recording_required_lane() {
        let mut status = late_stage_status_for_review_state_tests();
        status.review_state_status = String::from("clean");
        status.phase_detail = String::from(phase::DETAIL_QA_RECORDING_REQUIRED);
        status.current_branch_closure_id = Some(String::from("branch-closure-qa"));
        let gate_finish = gate_result_with_reason("irrelevant");

        let blocking_records = derive_public_blocking_records(&status, &gate_finish);
        assert_eq!(blocking_records.len(), 1, "{blocking_records:?}");
        assert_eq!(
            blocking_records[0].code,
            phase::DETAIL_QA_RECORDING_REQUIRED
        );
        assert_eq!(blocking_records[0].scope_type, "branch");
        assert_eq!(blocking_records[0].scope_key, "branch-closure-qa");
        assert_eq!(blocking_records[0].record_type, "qa_result");
        assert_eq!(
            blocking_records[0].required_follow_up,
            Some(String::from(
                crate::execution::review_route_tokens::FOLLOW_UP_ADVANCE_LATE_STAGE
            ))
        );
    }

    #[test]
    fn follow_up_override_pivot_status_check_rejects_body_only_decoy_strings() {
        let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
        let head_sha = context
            .current_head_sha()
            .expect("head sha should resolve for pivot override check");
        let reason_codes = vec![String::from("blocked_on_plan_revision")];
        let expected_decision_reason_codes =
            pivot_decision_reason_codes(&reason_codes, true, false).join(", ");
        let artifact_dir = context
            .runtime
            .state_dir
            .join("projects")
            .join(&context.runtime.repo_slug);
        fs::create_dir_all(&artifact_dir).expect("pivot artifact dir should be creatable");
        let artifact_path = artifact_dir.join(format!(
            "test-{}-workflow-pivot-999999999.md",
            context.runtime.safe_branch
        ));
        let decoy_source = format!(
            "# Workflow Pivot Record\n\
**Source Plan:** `docs/featureforge/plans/wrong.md`\n\
**Branch:** wrong-branch\n\
**Repo:** wrong/repo\n\
**Head SHA:** deadbeef\n\
**Decision Reason Codes:** wrong\n\
**Generated By:** featureforge:workflow-record-pivot\n\
\n\
mirror **Source Plan:** `{}`\n\
mirror **Branch:** {}\n\
mirror **Repo:** {}\n\
mirror **Head SHA:** {}\n\
mirror **Decision Reason Codes:** {}\n\
mirror **Generated By:** featureforge:workflow-record-pivot\n",
            context.plan_rel,
            context.runtime.branch_name,
            context.runtime.repo_slug,
            head_sha,
            expected_decision_reason_codes
        );
        fs::write(&artifact_path, decoy_source).expect("decoy pivot artifact should write");

        let matched = current_workflow_pivot_record_exists_for_status_decision(
            &context,
            &reason_codes,
            Some("required"),
        );
        fs::remove_file(&artifact_path).expect("decoy pivot artifact should clean up");

        assert!(
            !matched,
            "pivot follow_up_override clearing must not accept body-only decoy strings"
        );
    }
}
