use super::worktree_lease_truth::WorktreeLeaseRunIdentityProbe;
use super::{
    BTreeSet, ExecutionContext, ExecutionContract, FailureClass, GateState,
    INITIAL_AUTHORITATIVE_SEQUENCE, PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
    PUBLIC_WORKFLOW_OPERATOR_REMEDIATION, Path, PathBuf, WORKTREE_LEASE_VERSION, WorktreeLease,
    WorktreeLeaseState, authoritative_completed_steps_for_context, commit_object_fingerprint,
    current_run_plain_unit_review_receipt_paths, fs, harness_authoritative_artifact_path,
    latest_completed_attempts_by_step, load_status_authoritative_overlay_checked,
    parse_artifact_document, parse_contract_task_step_scope, sha256_hex, shared_is_ancestor_commit,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UnitReviewProofAuthority {
    /// Runtime-owned active contract state is present, so serial unit-review
    /// proof is derived from the active contract, completed-attempt provenance,
    /// and repository commit proof. Markdown receipt artifacts are projections.
    ActiveContractSerialRuntimeOwned,
    /// Plain current-run unit-review receipt artifacts have no active contract
    /// binding. They are diagnostics only and must never control gate routing.
    PlainReceiptDiagnosticOnly,
}

pub(super) fn classify_unit_review_proof_authority(
    active_contract_path: Option<&str>,
    active_contract_fingerprint: Option<&str>,
) -> UnitReviewProofAuthority {
    if active_contract_path.is_none() && active_contract_fingerprint.is_none() {
        UnitReviewProofAuthority::PlainReceiptDiagnosticOnly
    } else {
        UnitReviewProofAuthority::ActiveContractSerialRuntimeOwned
    }
}

pub(super) fn warn_plain_unit_review_receipts_diagnostic_only(
    context: &ExecutionContext,
    execution_run_id: &str,
    gate: &mut GateState,
) {
    match current_run_plain_unit_review_receipt_paths(context, execution_run_id) {
        Ok(paths) if !paths.is_empty() => {
            gate.warn("plain_unit_review_receipts_diagnostic_only");
        }
        Ok(_) => {}
        Err(_) => {
            gate.warn("plain_unit_review_receipts_unreadable_diagnostic_only");
        }
    }
}

pub(super) fn validate_authoritative_worktree_lease_fingerprint(
    source: &str,
    lease: &WorktreeLease,
    lease_path: String,
    gate: &mut GateState,
) -> bool {
    let Some(canonical_fingerprint) = canonical_worktree_lease_fingerprint(source) else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_fingerprint_unverifiable",
            format!(
                "Authoritative worktree lease fingerprint is unverifiable in {}.",
                lease_path
            ),
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return false;
    };

    if canonical_fingerprint != lease.lease_fingerprint {
        gate.fail(
            FailureClass::ArtifactIntegrityMismatch,
            "worktree_lease_fingerprint_mismatch",
            format!(
                "Authoritative worktree lease fingerprint does not match canonical content in {}.",
                lease_path
            ),
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return false;
    }

    true
}

pub(super) fn load_authoritative_active_contract(
    context: &ExecutionContext,
    gate: &mut GateState,
) -> Option<(PathBuf, String)> {
    let overlay = match load_status_authoritative_overlay_checked(context) {
        Ok(Some(overlay)) => overlay,
        Ok(None) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_state_unavailable",
                "Authoritative harness state is unavailable for execution-unit review gating.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return None;
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_state_unavailable",
                error.message,
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return None;
        }
    };
    let Some(active_contract_path) = overlay
        .active_contract_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_missing",
            "Authoritative harness state is missing the active contract path required to validate execution-unit review provenance.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    };
    let Some(active_contract_fingerprint) = overlay
        .active_contract_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_missing",
            "Authoritative harness state is missing the active contract fingerprint required to validate execution-unit review provenance.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    };
    if active_contract_path.contains('/') || active_contract_path.contains('\\') {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_path_invalid",
            "Authoritative active contract path must be a normalized relative filename.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    let expected_contract_filename = format!("contract-{active_contract_fingerprint}.md");
    if active_contract_path != expected_contract_filename {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_path_invalid",
            "Authoritative active contract path does not match the active contract fingerprint-derived filename.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    let active_contract_path = harness_authoritative_artifact_path(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
        active_contract_path,
    );
    let active_contract_metadata = match fs::symlink_metadata(&active_contract_path) {
        Ok(metadata) => metadata,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_contract_unreadable",
                format!(
                    "Could not inspect authoritative active contract {}: {error}",
                    active_contract_path.display()
                ),
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return None;
        }
    };
    if active_contract_metadata.file_type().is_symlink() || !active_contract_metadata.is_file() {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_unreadable",
            format!(
                "Authoritative active contract must be a regular file in {}.",
                active_contract_path.display()
            ),
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    Some((active_contract_path, active_contract_fingerprint.to_owned()))
}

fn canonical_worktree_lease_fingerprint(source: &str) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(source).ok()?;
    let object = value.as_object_mut()?;
    object.remove("lease_fingerprint");
    serde_json::to_vec(&value)
        .ok()
        .map(|bytes| sha256_hex(&bytes))
}

pub(super) fn worktree_lease_execution_context_key(
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

fn serial_execution_unit_id(task_number: u32, step_number: u32) -> String {
    format!("task-{task_number}-step-{step_number}")
}

fn serial_unit_review_lease_fingerprint(
    execution_run_id: &str,
    execution_unit_id: &str,
    execution_context_key: &str,
    reviewed_checkpoint_commit_sha: &str,
    approved_task_packet_fingerprint: &str,
    approved_unit_contract_fingerprint: &str,
) -> String {
    sha256_hex(
        format!(
            "serial-unit-review:{execution_run_id}:{execution_unit_id}:{execution_context_key}:{reviewed_checkpoint_commit_sha}:{approved_task_packet_fingerprint}:{approved_unit_contract_fingerprint}"
        )
        .as_bytes(),
    )
}

pub(super) fn approved_unit_contract_fingerprint_for_review(
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

pub(super) fn reconcile_result_proof_fingerprint_for_review(
    repo_root: &Path,
    reconcile_result_commit_sha: &str,
) -> Option<String> {
    commit_object_fingerprint(repo_root, reconcile_result_commit_sha)
}

pub(super) fn enforce_serial_unit_review_truth(
    context: &ExecutionContext,
    run_identity: &WorktreeLeaseRunIdentityProbe,
    active_contract: &ExecutionContract,
    active_contract_fingerprint: &str,
    gate: &mut GateState,
) {
    // This path is intentionally not evidence/projection fallback. It is only
    // reached after authoritative active-contract state has been loaded and
    // fingerprint-verified by the worktree lease gate.
    let Some(contract_steps) = serial_unit_review_contract_steps(active_contract, gate) else {
        return;
    };
    let authoritative_completed_steps = match authoritative_completed_steps_for_context(context) {
        Ok(steps) => steps.unwrap_or_default(),
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_completion_state_unavailable",
                error.message,
                PUBLIC_WORKFLOW_OPERATOR_REMEDIATION,
            );
            return;
        }
    };
    let latest_attempts = latest_completed_attempts_by_step(&context.evidence);
    for step in context.steps.iter().filter(|step| {
        let step_key = (step.task_number, step.step_number);
        contract_steps.contains(&step_key)
            && (step.checked || authoritative_completed_steps.contains(&step_key))
    }) {
        let Some(attempt_index) = latest_attempts
            .get(&(step.task_number, step.step_number))
            .copied()
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_evidence_missing",
                format!(
                    "Task {} Step {} is missing the completed attempt provenance required for active-contract serial unit-review gating.",
                    step.task_number, step.step_number
                ),
                PUBLIC_WORKFLOW_OPERATOR_REMEDIATION,
            );
            return;
        };
        let attempt = &context.evidence.attempts[attempt_index];
        let Some(approved_task_packet_fingerprint) = attempt
            .packet_fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_task_packet_missing",
                format!(
                    "Task {} Step {} is missing the packet fingerprint required for serial unit-review gating.",
                    step.task_number, step.step_number
                ),
                PUBLIC_WORKFLOW_OPERATOR_REMEDIATION,
            );
            return;
        };
        if !active_contract
            .source_task_packet_fingerprints
            .iter()
            .any(|candidate| candidate == approved_task_packet_fingerprint)
        {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_task_packet_not_authoritative",
                format!(
                    "Task {} Step {} completed attempt does not bind a task packet from the current authoritative contract.",
                    step.task_number, step.step_number
                ),
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
        let Some(reviewed_checkpoint_commit_sha) = attempt
            .head_sha
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_head_missing",
                format!(
                    "Task {} Step {} is missing the reviewed checkpoint SHA required for serial unit-review gating.",
                    step.task_number, step.step_number
                ),
                PUBLIC_WORKFLOW_OPERATOR_REMEDIATION,
            );
            return;
        };
        let execution_unit_id = serial_execution_unit_id(step.task_number, step.step_number);
        let expected_execution_context_key = worktree_lease_execution_context_key(
            &run_identity.execution_run_id,
            &execution_unit_id,
            &context.plan_rel,
            context.plan_document.plan_revision,
            &context.runtime.branch_name,
            reviewed_checkpoint_commit_sha,
        );
        let approved_unit_contract_fingerprint = approved_unit_contract_fingerprint_for_review(
            active_contract_fingerprint,
            approved_task_packet_fingerprint,
            &execution_unit_id,
        );
        let Some(reconcile_result_proof_fingerprint) =
            reconcile_result_proof_fingerprint_for_review(
                &context.runtime.repo_root,
                reviewed_checkpoint_commit_sha,
            )
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_reconcile_proof_unverifiable",
                format!(
                    "Task {} Step {} serial unit-review reconcile proof could not be verified against repository history.",
                    step.task_number, step.step_number
                ),
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        };
        let review_receipt_path = harness_authoritative_artifact_path(
            &context.runtime.state_dir,
            &context.runtime.repo_slug,
            &context.runtime.branch_name,
            &format!(
                "unit-review-{}-{}.md",
                run_identity.execution_run_id, execution_unit_id
            ),
        );
        let pseudo_lease = WorktreeLease {
            lease_version: WORKTREE_LEASE_VERSION,
            authoritative_sequence: INITIAL_AUTHORITATIVE_SEQUENCE + 1,
            execution_run_id: run_identity.execution_run_id.clone(),
            execution_context_key: expected_execution_context_key.clone(),
            source_plan_path: context.plan_rel.clone(),
            source_plan_revision: context.plan_document.plan_revision,
            execution_unit_id: execution_unit_id.clone(),
            source_branch: context.runtime.branch_name.clone(),
            authoritative_integration_branch: context.runtime.branch_name.clone(),
            worktree_path: fs::canonicalize(&context.runtime.repo_root)
                .unwrap_or_else(|_| context.runtime.repo_root.clone())
                .display()
                .to_string(),
            repo_state_baseline_head_sha: reviewed_checkpoint_commit_sha.to_owned(),
            repo_state_baseline_worktree_fingerprint: approved_task_packet_fingerprint.to_owned(),
            lease_state: WorktreeLeaseState::Cleaned,
            cleanup_state: String::from("cleaned"),
            reviewed_checkpoint_commit_sha: Some(reviewed_checkpoint_commit_sha.to_owned()),
            reconcile_result_commit_sha: Some(reviewed_checkpoint_commit_sha.to_owned()),
            reconcile_result_proof_fingerprint: Some(reconcile_result_proof_fingerprint.clone()),
            reconcile_mode: String::from("identity_preserving"),
            generated_by: String::from("featureforge:executing-plans"),
            generated_at: String::from("runtime-derived"),
            lease_fingerprint: serial_unit_review_lease_fingerprint(
                &run_identity.execution_run_id,
                &execution_unit_id,
                &expected_execution_context_key,
                reviewed_checkpoint_commit_sha,
                approved_task_packet_fingerprint,
                &approved_unit_contract_fingerprint,
            ),
        };
        warn_on_serial_unit_review_receipt_projection_drift(
            context,
            &run_identity.execution_run_id,
            &pseudo_lease,
            &review_receipt_path,
            UnitReviewReceiptExpectations {
                expected_execution_context_key: &expected_execution_context_key,
                expected_fingerprint: "",
                expected_task_packet_fingerprint: approved_task_packet_fingerprint,
                expected_approved_unit_contract_fingerprint: &approved_unit_contract_fingerprint,
                expected_reconcile_result_commit_sha: reviewed_checkpoint_commit_sha,
            },
            reviewed_checkpoint_commit_sha,
            gate,
        );
    }
}

fn warn_on_serial_unit_review_receipt_projection_drift(
    context: &ExecutionContext,
    execution_run_id: &str,
    pseudo_lease: &WorktreeLease,
    review_receipt_path: &Path,
    expectations: UnitReviewReceiptExpectations<'_>,
    reviewed_checkpoint_commit_sha: &str,
    gate: &mut GateState,
) {
    let review_metadata = match fs::symlink_metadata(review_receipt_path) {
        Ok(metadata) => metadata,
        Err(_) => {
            gate.warn("serial_unit_review_projection_missing_diagnostic_only");
            return;
        }
    };
    if review_metadata.file_type().is_symlink() || !review_metadata.is_file() {
        gate.warn("serial_unit_review_projection_path_invalid_diagnostic_only");
        return;
    }
    let review_source = match fs::read_to_string(review_receipt_path) {
        Ok(source) => source,
        Err(_) => {
            gate.warn("serial_unit_review_projection_unreadable_diagnostic_only");
            return;
        }
    };
    let Some(review_receipt_fingerprint) =
        canonical_unit_review_receipt_fingerprint(&review_source)
    else {
        gate.warn("serial_unit_review_projection_fingerprint_unverifiable_diagnostic_only");
        return;
    };
    let mut diagnostic_gate = GateState::default();
    let Some((receipt_checkpoint_commit_sha, receipt_reconciled_result_commit_sha)) =
        validate_authoritative_unit_review_receipt(
            context,
            execution_run_id,
            pseudo_lease,
            &review_source,
            review_receipt_path,
            UnitReviewReceiptExpectations {
                expected_execution_context_key: expectations.expected_execution_context_key,
                expected_fingerprint: &review_receipt_fingerprint,
                expected_task_packet_fingerprint: expectations.expected_task_packet_fingerprint,
                expected_approved_unit_contract_fingerprint: expectations
                    .expected_approved_unit_contract_fingerprint,
                expected_reconcile_result_commit_sha: expectations
                    .expected_reconcile_result_commit_sha,
            },
            &mut diagnostic_gate,
        )
    else {
        for reason_code in diagnostic_gate.reason_codes {
            gate.warn(&unit_review_projection_warning_code(&reason_code));
        }
        return;
    };
    if receipt_checkpoint_commit_sha != reviewed_checkpoint_commit_sha {
        gate.warn("serial_unit_review_projection_checkpoint_mismatch_diagnostic_only");
    }
    if receipt_reconciled_result_commit_sha != reviewed_checkpoint_commit_sha {
        gate.warn("serial_unit_review_projection_reconcile_result_mismatch_diagnostic_only");
    }
}

fn unit_review_projection_warning_code(reason_code: &str) -> String {
    if let Some(suffix) = reason_code.strip_prefix("worktree_lease_review_receipt") {
        format!("worktree_lease_review_projection{suffix}_diagnostic_only")
    } else if let Some(suffix) = reason_code.strip_prefix("serial_unit_review_receipt") {
        format!("serial_unit_review_projection{suffix}_diagnostic_only")
    } else {
        format!("{reason_code}_diagnostic_only")
    }
}

fn serial_unit_review_contract_steps(
    active_contract: &ExecutionContract,
    gate: &mut GateState,
) -> Option<BTreeSet<(u32, u32)>> {
    let mut contract_steps = BTreeSet::new();
    for covered_step in &active_contract.covered_steps {
        let Some(step_ref) = parse_contract_task_step_scope(covered_step) else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_contract_scope_malformed",
                "The authoritative active contract has malformed covered step scope required for serial unit-review gating.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return None;
        };
        contract_steps.insert(step_ref);
    }
    Some(contract_steps)
}

pub(super) struct UnitReviewReceiptExpectations<'a> {
    pub(super) expected_execution_context_key: &'a str,
    pub(super) expected_fingerprint: &'a str,
    pub(super) expected_task_packet_fingerprint: &'a str,
    pub(super) expected_approved_unit_contract_fingerprint: &'a str,
    pub(super) expected_reconcile_result_commit_sha: &'a str,
}

pub(super) fn validate_authoritative_unit_review_receipt(
    context: &ExecutionContext,
    execution_run_id: &str,
    lease: &WorktreeLease,
    source: &str,
    receipt_path: &Path,
    expectations: UnitReviewReceiptExpectations<'_>,
    gate: &mut GateState,
) -> Option<(String, String)> {
    let review_document = parse_artifact_document(receipt_path);
    if review_document.title.as_deref() != Some("# Unit Review Result") {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_malformed",
            "The runtime-owned worktree lease review binding is malformed.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Review Stage")
        .map(String::as_str)
        != Some("featureforge:unit-review")
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_stage_mismatch",
            "The runtime-owned worktree lease review binding has the wrong review stage.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Reviewer Provenance")
        .map(String::as_str)
        != Some("dedicated-independent")
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_not_dedicated",
            "The runtime-owned worktree lease review binding is not dedicated-independent.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Source Plan")
        .map(String::as_str)
        != Some(context.plan_rel.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_plan_mismatch",
            "The runtime-owned worktree lease review binding does not match the current plan.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Source Plan Revision")
        .and_then(|value| value.parse::<u32>().ok())
        != Some(context.plan_document.plan_revision)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_plan_revision_mismatch",
            "The runtime-owned worktree lease review binding does not match the current plan revision.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Execution Run ID")
        .map(String::as_str)
        != Some(execution_run_id)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_run_mismatch",
            "The runtime-owned worktree lease review binding does not match the current execution run.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Execution Unit ID")
        .map(String::as_str)
        != Some(lease.execution_unit_id.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_unit_mismatch",
            "The runtime-owned worktree lease review binding does not match the reviewed execution unit.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Lease Fingerprint")
        .map(String::as_str)
        != Some(lease.lease_fingerprint.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_lease_fingerprint_mismatch",
            "The runtime-owned worktree lease review binding does not match the reviewed lease fingerprint.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Execution Context Key")
        .map(String::as_str)
        != Some(expectations.expected_execution_context_key)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_context_key_mismatch",
            "The runtime-owned worktree lease review binding does not match the current execution context.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Approved Task Packet Fingerprint")
        .map(String::as_str)
        != Some(expectations.expected_task_packet_fingerprint)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_task_packet_mismatch",
            "The runtime-owned worktree lease review binding does not match the approved task packet.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Approved Unit Contract Fingerprint")
        .map(String::as_str)
        != Some(expectations.expected_approved_unit_contract_fingerprint)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_unit_contract_mismatch",
            "The runtime-owned worktree lease review binding does not bind the approved unit contract.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if expectations.expected_approved_unit_contract_fingerprint
        == expectations.expected_task_packet_fingerprint
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_unit_contract_mismatch",
            "The runtime-owned worktree lease review binding must bind a distinct approved unit contract fingerprint.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Reconcile Mode")
        .map(String::as_str)
        != Some("identity_preserving")
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_reconcile_mode_mismatch",
            "The runtime-owned worktree lease review binding does not prove an identity-preserving reconcile.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Reconciled Result SHA")
        .map(String::as_str)
        != Some(expectations.expected_reconcile_result_commit_sha)
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_identity_preserving_proof_mismatch",
            "The runtime-owned worktree lease review binding does not bind the exact reconciled commit.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    let Some(expected_reconcile_result_proof_fingerprint) =
        reconcile_result_proof_fingerprint_for_review(
            &context.runtime.repo_root,
            expectations.expected_reconcile_result_commit_sha,
        )
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_identity_preserving_proof_unverifiable",
            "The runtime-owned worktree lease review binding exact reconcile proof could not be verified against repository history.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    };
    if review_document
        .headers
        .get("Reconcile Result Proof Fingerprint")
        .map(String::as_str)
        != Some(expected_reconcile_result_proof_fingerprint.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_identity_preserving_proof_mismatch",
            "The runtime-owned worktree lease review binding does not bind the exact reconciled commit object.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Reviewed Worktree")
        .map(String::as_str)
        != Some(lease.worktree_path.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_worktree_mismatch",
            "The runtime-owned worktree lease review binding does not match the reviewed worktree.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document.headers.get("Result").map(String::as_str) != Some("pass") {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_not_pass",
            "The runtime-owned worktree lease review binding is not marked pass.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Generated By")
        .map(String::as_str)
        != Some("featureforge:unit-review")
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_generator_mismatch",
            "The runtime-owned worktree lease review binding does not come from the required review generator.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    let expected_receipt_filename = format!(
        "unit-review-{}-{}.md",
        execution_run_id,
        lease.execution_unit_id.trim_start_matches("unit-")
    );
    if receipt_path.file_name().and_then(|value| value.to_str())
        != Some(expected_receipt_filename.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_binding_path_invalid",
            "The runtime-owned worktree lease review binding path does not match the reviewed execution unit provenance.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    let Some(receipt_checkpoint_commit_sha) = review_document
        .headers
        .get("Reviewed Checkpoint SHA")
        .cloned()
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_head_missing",
            "The runtime-owned worktree lease review binding is missing its reviewed checkpoint.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    };

    let Some(canonical_fingerprint) = canonical_unit_review_receipt_fingerprint(source) else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_fingerprint_unverifiable",
            "Runtime-owned worktree lease review binding fingerprint is unverifiable.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    };
    if canonical_fingerprint != expectations.expected_fingerprint {
        gate.fail(
            FailureClass::ArtifactIntegrityMismatch,
            "worktree_lease_review_receipt_fingerprint_mismatch",
            "Runtime-owned worktree lease review binding fingerprint does not match canonical content.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if review_document
        .headers
        .get("Receipt Fingerprint")
        .map(String::as_str)
        != Some(expectations.expected_fingerprint)
    {
        gate.fail(
            FailureClass::ArtifactIntegrityMismatch,
            "worktree_lease_review_receipt_fingerprint_mismatch",
            "Runtime-owned worktree lease review binding fingerprint header does not match canonical content.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }

    Some((
        receipt_checkpoint_commit_sha,
        expectations.expected_reconcile_result_commit_sha.to_owned(),
    ))
}

fn canonical_unit_review_receipt_fingerprint(source: &str) -> Option<String> {
    let filtered = source
        .lines()
        .filter(|line| !line.trim().starts_with("**Receipt Fingerprint:**"))
        .collect::<Vec<_>>()
        .join("\n");
    Some(sha256_hex(filtered.as_bytes()))
}

pub(super) fn is_ancestor_commit(repo_root: &Path, ancestor: &str, descendant: &str) -> bool {
    shared_is_ancestor_commit(repo_root, ancestor, descendant)
}
