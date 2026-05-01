use super::*;

pub(super) fn enforce_plain_unit_review_truth(
    context: &ExecutionContext,
    execution_run_id: &str,
    gate: &mut GateState,
) {
    let current_run_receipts = match current_run_plain_unit_review_receipt_paths(
        context,
        execution_run_id,
    ) {
        Ok(paths) => paths,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipts_unreadable",
                error,
                "Restore authoritative unit-review receipt readability and retry gate-review or gate-finish.",
            );
            return;
        }
    };
    if current_run_receipts.is_empty() {
        return;
    }

    let expected_strategy_checkpoint_fingerprint =
        match authoritative_strategy_checkpoint_fingerprint_checked(context) {
            Ok(Some(fingerprint)) if !fingerprint.trim().is_empty() => fingerprint,
            Ok(_) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "plain_unit_review_receipt_strategy_checkpoint_missing",
                    "Authoritative strategy checkpoint provenance is missing for current-run unit-review receipt validation.",
                    "Restore authoritative strategy checkpoint provenance and retry gate-review or gate-finish.",
                );
                return;
            }
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "plain_unit_review_receipt_strategy_checkpoint_missing",
                    error.message,
                    "Restore authoritative strategy checkpoint provenance and retry gate-review or gate-finish.",
                );
                return;
            }
        };

    let latest_attempts = latest_completed_attempts_by_step(&context.evidence);
    let expected_receipt_paths = context
        .steps
        .iter()
        .filter(|step| step.checked)
        .map(|step| {
            (
                authoritative_unit_review_receipt_path(
                    context,
                    execution_run_id,
                    step.task_number,
                    step.step_number,
                )
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_owned(),
                (step.task_number, step.step_number),
            )
        })
        .collect::<BTreeMap<_, _>>();

    for receipt_path in current_run_receipts {
        let Some(receipt_file_name) = receipt_path
            .file_name()
            .and_then(|value| value.to_str())
            .map(str::to_owned)
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipt_malformed",
                "A current-run unit-review receipt has an unreadable filename.",
                "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
            );
            return;
        };
        let Some((task_number, step_number)) =
            expected_receipt_paths.get(&receipt_file_name).copied()
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipt_malformed",
                format!(
                    "Current-run unit-review receipt {} does not match any checked plan step.",
                    receipt_path.display()
                ),
                "Remove or repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
            );
            return;
        };
        let Some(attempt_index) = latest_attempts.get(&(task_number, step_number)).copied() else {
            gate.fail(
                FailureClass::StaleExecutionEvidence,
                "plain_unit_review_receipt_provenance_mismatch",
                format!(
                    "Current-run unit-review receipt {} has no completed evidence attempt to validate against.",
                    receipt_path.display()
                ),
                "Rebuild the execution evidence for the affected step and retry gate-review or gate-finish.",
            );
            return;
        };
        let attempt = &context.evidence.attempts[attempt_index];
        let Some(expected_task_packet_fingerprint) = attempt
            .packet_fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipt_malformed",
                format!(
                    "Task {} Step {} is missing packet fingerprint provenance required to validate plain unit-review receipts.",
                    task_number, step_number
                ),
                "Repair the execution evidence for the affected step and retry gate-review or gate-finish.",
            );
            return;
        };
        let Some(expected_reviewed_checkpoint_sha) = attempt
            .head_sha
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipt_malformed",
                format!(
                    "Task {} Step {} is missing reviewed checkpoint provenance required to validate plain unit-review receipts.",
                    task_number, step_number
                ),
                "Repair the execution evidence for the affected step and retry gate-review or gate-finish.",
            );
            return;
        };
        let review_source = match fs::read_to_string(&receipt_path) {
            Ok(source) => source,
            Err(error) => {
                gate.fail(
                    FailureClass::ExecutionStateNotReady,
                    "plain_unit_review_receipt_unreadable",
                    format!(
                        "Could not read current-run unit-review receipt {}: {error}",
                        receipt_path.display()
                    ),
                    "Restore the authoritative unit-review receipt and retry gate-review or gate-finish.",
                );
                return;
            }
        };
        if !validate_plain_unit_review_receipt(
            context,
            execution_run_id,
            &review_source,
            &receipt_path,
            PlainUnitReviewReceiptExpectations {
                expected_strategy_checkpoint_fingerprint: expected_strategy_checkpoint_fingerprint
                    .as_str(),
                expected_task_packet_fingerprint,
                expected_reviewed_checkpoint_sha,
                expected_execution_unit_id: serial_execution_unit_id(task_number, step_number),
            },
            gate,
        ) {
            return;
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
            "Repair the authoritative worktree lease artifact and retry gate-review or gate-finish.",
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
            "Regenerate the authoritative worktree lease artifact from canonical content and retry gate-review or gate-finish.",
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
                "Restore authoritative harness state readability and retry gate-review or gate-finish.",
            );
            return None;
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_state_unavailable",
                error.message,
                "Restore authoritative harness state readability and retry gate-review or gate-finish.",
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
            "Restore the authoritative active contract and retry gate-review or gate-finish.",
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
            "Restore the authoritative active contract and retry gate-review or gate-finish.",
        );
        return None;
    };
    if active_contract_path.contains('/') || active_contract_path.contains('\\') {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_path_invalid",
            "Authoritative active contract path must be a normalized relative filename.",
            "Restore the authoritative active contract path and retry gate-review or gate-finish.",
        );
        return None;
    }
    let expected_contract_filename = format!("contract-{active_contract_fingerprint}.md");
    if active_contract_path != expected_contract_filename {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_path_invalid",
            "Authoritative active contract path does not match the active contract fingerprint-derived filename.",
            "Restore the authoritative active contract path and retry gate-review or gate-finish.",
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
                "Restore the authoritative active contract and retry gate-review or gate-finish.",
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
            "Restore the authoritative active contract and retry gate-review or gate-finish.",
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
    active_contract_fingerprint: &str,
    gate: &mut GateState,
) {
    let latest_attempts = latest_completed_attempts_by_step(&context.evidence);
    for step in context.steps.iter().filter(|step| step.checked) {
        let Some(attempt_index) = latest_attempts
            .get(&(step.task_number, step.step_number))
            .copied()
        else {
            continue;
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
                "Rebuild the execution evidence for the completed step and retry gate-review or gate-finish.",
            );
            return;
        };
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
                "Rebuild the execution evidence for the completed step and retry gate-review or gate-finish.",
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
                "Restore repository history readability and retry gate-review or gate-finish.",
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
        let review_metadata = match fs::symlink_metadata(&review_receipt_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                gate.fail(
                    FailureClass::ExecutionStateNotReady,
                    "serial_unit_review_receipt_missing",
                    format!(
                        "Task {} Step {} is missing its authoritative serial unit-review receipt {}: {error}",
                        step.task_number,
                        step.step_number,
                        review_receipt_path.display()
                    ),
                    "Record a dedicated-independent serial unit-review receipt for the completed execution unit and retry gate-review or gate-finish.",
                );
                return;
            }
        };
        if review_metadata.file_type().is_symlink() || !review_metadata.is_file() {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_receipt_path_invalid",
                format!(
                    "Task {} Step {} serial unit-review receipt must be a regular file in {}.",
                    step.task_number,
                    step.step_number,
                    review_receipt_path.display()
                ),
                "Restore the authoritative serial unit-review receipt and retry gate-review or gate-finish.",
            );
            return;
        }
        let review_source = match fs::read_to_string(&review_receipt_path) {
            Ok(source) => source,
            Err(error) => {
                gate.fail(
                    FailureClass::ExecutionStateNotReady,
                    "serial_unit_review_receipt_unreadable",
                    format!(
                        "Could not read authoritative serial unit-review receipt {}: {error}",
                        review_receipt_path.display()
                    ),
                    "Restore the authoritative serial unit-review receipt and retry gate-review or gate-finish.",
                );
                return;
            }
        };
        let Some(review_receipt_fingerprint) =
            canonical_unit_review_receipt_fingerprint(&review_source)
        else {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "serial_unit_review_receipt_fingerprint_unverifiable",
                format!(
                    "Task {} Step {} serial unit-review receipt fingerprint is unverifiable in {}.",
                    step.task_number,
                    step.step_number,
                    review_receipt_path.display()
                ),
                "Regenerate the authoritative serial unit-review receipt from canonical content and retry gate-review or gate-finish.",
            );
            return;
        };
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
        let (receipt_checkpoint_commit_sha, receipt_reconciled_result_commit_sha) =
            match validate_authoritative_unit_review_receipt(
                context,
                &run_identity.execution_run_id,
                &pseudo_lease,
                &review_source,
                &review_receipt_path,
                UnitReviewReceiptExpectations {
                    expected_execution_context_key: &expected_execution_context_key,
                    expected_fingerprint: &review_receipt_fingerprint,
                    expected_task_packet_fingerprint: approved_task_packet_fingerprint,
                    expected_approved_unit_contract_fingerprint:
                        &approved_unit_contract_fingerprint,
                    expected_reconcile_result_commit_sha: reviewed_checkpoint_commit_sha,
                },
                gate,
            ) {
                Some(values) => values,
                None => return,
            };
        if receipt_checkpoint_commit_sha != reviewed_checkpoint_commit_sha {
            gate.fail(
                FailureClass::StaleProvenance,
                "serial_unit_review_receipt_checkpoint_mismatch",
                format!(
                    "Task {} Step {} serial unit-review receipt does not bind the completed step checkpoint.",
                    step.task_number, step.step_number
                ),
                "Regenerate the authoritative serial unit-review receipt from the completed step checkpoint and retry gate-review or gate-finish.",
            );
            return;
        }
        if receipt_reconciled_result_commit_sha != reviewed_checkpoint_commit_sha {
            gate.fail(
                FailureClass::StaleProvenance,
                "serial_unit_review_receipt_reconcile_result_mismatch",
                format!(
                    "Task {} Step {} serial unit-review receipt does not bind the completed step result commit.",
                    step.task_number, step.step_number
                ),
                "Regenerate the authoritative serial unit-review receipt from the completed step result and retry gate-review or gate-finish.",
            );
            return;
        }
    }
}

pub(super) struct UnitReviewReceiptExpectations<'a> {
    pub(super) expected_execution_context_key: &'a str,
    pub(super) expected_fingerprint: &'a str,
    pub(super) expected_task_packet_fingerprint: &'a str,
    pub(super) expected_approved_unit_contract_fingerprint: &'a str,
    pub(super) expected_reconcile_result_commit_sha: &'a str,
}

struct PlainUnitReviewReceiptExpectations<'a> {
    expected_strategy_checkpoint_fingerprint: &'a str,
    expected_task_packet_fingerprint: &'a str,
    expected_reviewed_checkpoint_sha: &'a str,
    expected_execution_unit_id: String,
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
            "The authoritative unit-review receipt is malformed.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt has the wrong review stage.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt is not dedicated-independent.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not match the current plan.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not match the current plan revision.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not match the current execution run.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not match the reviewed execution unit.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not match the reviewed lease fingerprint.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not match the current execution context.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not match the approved task packet.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not bind the approved unit contract.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if expectations.expected_approved_unit_contract_fingerprint
        == expectations.expected_task_packet_fingerprint
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_unit_contract_mismatch",
            "The authoritative unit-review receipt must bind a distinct approved unit contract fingerprint.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not prove an identity-preserving reconcile.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not bind the exact reconciled commit.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt exact reconcile proof could not be verified against repository history.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not bind the exact reconciled commit object.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not match the reviewed worktree.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    }
    if review_document.headers.get("Result").map(String::as_str) != Some("pass") {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_not_pass",
            "The authoritative unit-review receipt is not marked pass.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt does not come from the unit-review generator.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt path does not match the reviewed execution unit provenance.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
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
            "The authoritative unit-review receipt is missing its reviewed checkpoint.",
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    };

    let Some(canonical_fingerprint) = canonical_unit_review_receipt_fingerprint(source) else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_review_receipt_fingerprint_unverifiable",
            format!(
                "Authoritative unit-review receipt fingerprint is unverifiable in {}.",
                receipt_path.display()
            ),
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return None;
    };
    if canonical_fingerprint != expectations.expected_fingerprint {
        gate.fail(
            FailureClass::ArtifactIntegrityMismatch,
            "worktree_lease_review_receipt_fingerprint_mismatch",
            format!(
                "Authoritative unit-review receipt fingerprint does not match canonical content in {}.",
                receipt_path.display()
            ),
            "Regenerate the authoritative unit-review receipt from canonical content and retry gate-review or gate-finish.",
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
            format!(
                "Authoritative unit-review receipt fingerprint header does not match canonical content in {}.",
                receipt_path.display()
            ),
            "Regenerate the authoritative unit-review receipt from canonical content and retry gate-review or gate-finish.",
        );
        return None;
    }

    Some((
        receipt_checkpoint_commit_sha,
        expectations.expected_reconcile_result_commit_sha.to_owned(),
    ))
}

fn validate_plain_unit_review_receipt(
    context: &ExecutionContext,
    execution_run_id: &str,
    source: &str,
    receipt_path: &Path,
    expectations: PlainUnitReviewReceiptExpectations<'_>,
    gate: &mut GateState,
) -> bool {
    let review_document = parse_artifact_document(receipt_path);
    if review_document.title.as_deref() != Some("# Unit Review Result")
        || review_document
            .headers
            .get("Review Stage")
            .map(String::as_str)
            != Some("featureforge:unit-review")
        || review_document
            .headers
            .get("Reviewer Provenance")
            .map(String::as_str)
            != Some("dedicated-independent")
        || !matches!(
            review_document
                .headers
                .get("Reviewer Source")
                .map(String::as_str)
                .unwrap_or_default(),
            "fresh-context-subagent" | "cross-model"
        )
        || review_document.headers.get("Result").map(String::as_str) != Some("pass")
        || review_document
            .headers
            .get("Generated By")
            .map(String::as_str)
            != Some("featureforge:unit-review")
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "plain_unit_review_receipt_malformed",
            format!(
                "Current-run unit-review receipt {} is malformed.",
                receipt_path.display()
            ),
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return false;
    }

    for forbidden_header in [
        "Lease Fingerprint",
        "Execution Context Key",
        "Approved Unit Contract Fingerprint",
        "Reconciled Result SHA",
        "Reconcile Result Proof Fingerprint",
        "Reconcile Mode",
        "Reviewed Worktree",
    ] {
        if review_document.headers.contains_key(forbidden_header) {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "plain_unit_review_receipt_malformed",
                format!(
                    "Current-run unit-review receipt {} unexpectedly includes {} without an active authoritative contract.",
                    receipt_path.display(),
                    forbidden_header
                ),
                "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
            );
            return false;
        }
    }

    let expected_file_name = format!(
        "unit-review-{}-{}.md",
        execution_run_id, expectations.expected_execution_unit_id
    );
    if receipt_path.file_name().and_then(|value| value.to_str())
        != Some(expected_file_name.as_str())
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "plain_unit_review_receipt_malformed",
            format!(
                "Current-run unit-review receipt path {} does not match the reviewed execution unit provenance.",
                receipt_path.display()
            ),
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return false;
    }

    let Some(canonical_fingerprint) = canonical_unit_review_receipt_fingerprint(source) else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "plain_unit_review_receipt_fingerprint_unverifiable",
            format!(
                "Current-run unit-review receipt fingerprint is unverifiable in {}.",
                receipt_path.display()
            ),
            "Repair the authoritative unit-review receipt and retry gate-review or gate-finish.",
        );
        return false;
    };
    if review_document
        .headers
        .get("Receipt Fingerprint")
        .map(String::as_str)
        != Some(canonical_fingerprint.as_str())
    {
        gate.fail(
            FailureClass::ArtifactIntegrityMismatch,
            "plain_unit_review_receipt_fingerprint_mismatch",
            format!(
                "Current-run unit-review receipt fingerprint header does not match canonical content in {}.",
                receipt_path.display()
            ),
            "Regenerate the authoritative unit-review receipt from canonical content and retry gate-review or gate-finish.",
        );
        return false;
    }

    let mut mismatched_fields = Vec::new();
    let mut mismatch_details = Vec::new();
    if review_document
        .headers
        .get("Source Plan")
        .map(String::as_str)
        != Some(context.plan_rel.as_str())
    {
        mismatched_fields.push("Source Plan");
        mismatch_details.push(format!(
            "Source Plan expected={} actual={}",
            context.plan_rel,
            review_document
                .headers
                .get("Source Plan")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Source Plan Revision")
        .and_then(|value| value.parse::<u32>().ok())
        != Some(context.plan_document.plan_revision)
    {
        mismatched_fields.push("Source Plan Revision");
        mismatch_details.push(format!(
            "Source Plan Revision expected={} actual={}",
            context.plan_document.plan_revision,
            review_document
                .headers
                .get("Source Plan Revision")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Execution Run ID")
        .map(String::as_str)
        != Some(execution_run_id)
    {
        mismatched_fields.push("Execution Run ID");
        mismatch_details.push(format!(
            "Execution Run ID expected={} actual={}",
            execution_run_id,
            review_document
                .headers
                .get("Execution Run ID")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Execution Unit ID")
        .map(String::as_str)
        != Some(expectations.expected_execution_unit_id.as_str())
    {
        mismatched_fields.push("Execution Unit ID");
        mismatch_details.push(format!(
            "Execution Unit ID expected={} actual={}",
            expectations.expected_execution_unit_id,
            review_document
                .headers
                .get("Execution Unit ID")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Strategy Checkpoint Fingerprint")
        .map(String::as_str)
        != Some(expectations.expected_strategy_checkpoint_fingerprint)
    {
        mismatched_fields.push("Strategy Checkpoint Fingerprint");
        mismatch_details.push(format!(
            "Strategy Checkpoint Fingerprint expected={} actual={}",
            expectations.expected_strategy_checkpoint_fingerprint,
            review_document
                .headers
                .get("Strategy Checkpoint Fingerprint")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Approved Task Packet Fingerprint")
        .map(String::as_str)
        != Some(expectations.expected_task_packet_fingerprint)
    {
        mismatched_fields.push("Approved Task Packet Fingerprint");
        mismatch_details.push(format!(
            "Approved Task Packet Fingerprint expected={} actual={}",
            expectations.expected_task_packet_fingerprint,
            review_document
                .headers
                .get("Approved Task Packet Fingerprint")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if review_document
        .headers
        .get("Reviewed Checkpoint SHA")
        .map(String::as_str)
        != Some(expectations.expected_reviewed_checkpoint_sha)
    {
        mismatched_fields.push("Reviewed Checkpoint SHA");
        mismatch_details.push(format!(
            "Reviewed Checkpoint SHA expected={} actual={}",
            expectations.expected_reviewed_checkpoint_sha,
            review_document
                .headers
                .get("Reviewed Checkpoint SHA")
                .map(String::as_str)
                .unwrap_or("<missing>")
        ));
    }
    if !mismatched_fields.is_empty() {
        gate.fail(
            FailureClass::StaleProvenance,
            "plain_unit_review_receipt_provenance_mismatch",
            format!(
                "Current-run unit-review receipt {} does not match the active task checkpoint provenance (mismatched fields: {}; details: {}).",
                receipt_path.display(),
                mismatched_fields.join(", ")
                , mismatch_details.join("; ")
            ),
            "Regenerate the authoritative unit-review receipt for the completed step and retry gate-review or gate-finish.",
        );
        return false;
    }

    true
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
