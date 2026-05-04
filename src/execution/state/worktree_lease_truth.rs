use super::*;

pub(super) fn enforce_worktree_lease_binding_truth(
    context: &ExecutionContext,
    gate: &mut GateState,
) {
    let authoritative_context = match load_worktree_lease_authoritative_context_checked(context) {
        Ok(Some(context)) => context,
        Ok(None) => {
            let has_any_binding_artifacts =
                match worktree_or_unit_review_binding_artifacts_exist(context) {
                    Ok(value) => value,
                    Err(error) => {
                        gate.fail(
                            FailureClass::MalformedExecutionState,
                            "worktree_lease_artifacts_unreadable",
                            error,
                            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                        );
                        return;
                    }
                };
            if has_any_binding_artifacts {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_authoritative_state_unavailable",
                    "Authoritative harness state is unavailable for worktree lease gating.",
                    PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                );
            }
            return;
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_state_unavailable",
                error.message,
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
    };
    let run_identity = match authoritative_context.run_identity.as_ref() {
        Some(run_identity) => run_identity,
        None => {
            let has_any_binding_artifacts =
                match worktree_or_unit_review_binding_artifacts_exist(context) {
                    Ok(value) => value,
                    Err(error) => {
                        gate.fail(
                            FailureClass::MalformedExecutionState,
                            "worktree_lease_artifacts_unreadable",
                            error,
                            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                        );
                        return;
                    }
                };
            if !has_any_binding_artifacts {
                return;
            }
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_run_identity_missing",
                "Authoritative harness state is missing its current run identity.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
    };
    if run_identity.source_plan_path != context.plan_rel
        || run_identity.source_plan_revision != context.plan_document.plan_revision
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_run_context_mismatch",
            "Authoritative run identity does not match the current plan context.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return;
    }

    let Some(active_worktree_lease_fingerprints) =
        authoritative_context.active_worktree_lease_fingerprints
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_index_missing",
            "Authoritative harness state is missing the active worktree lease fingerprint index for the current run.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return;
    };
    let Some(active_worktree_lease_bindings) = authoritative_context.active_worktree_lease_bindings
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_index_missing",
            "Authoritative harness state is missing the active worktree lease binding index for the current run.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return;
    };
    let current_run_fingerprint_count = active_worktree_lease_fingerprints.len();
    let current_run_fingerprints: BTreeSet<String> =
        active_worktree_lease_fingerprints.into_iter().collect();
    if current_run_fingerprints.len() != current_run_fingerprint_count {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_binding_duplicate",
            "Authoritative harness state contains duplicate active worktree lease fingerprints for the current run.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return;
    }

    let current_run_bindings = active_worktree_lease_bindings
        .iter()
        .filter(|binding| binding.execution_run_id == run_identity.execution_run_id)
        .collect::<Vec<_>>();
    if current_run_fingerprints.is_empty() {
        let current_run_artifacts_exist = match current_run_worktree_lease_artifacts_exist(
            context,
            &run_identity.execution_run_id,
            &authoritative_context.released_worktree_lease_records,
        ) {
            Ok(value) => value,
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_artifacts_unreadable",
                    error,
                    PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                );
                return;
            }
        };
        if !current_run_bindings.is_empty() || current_run_artifacts_exist {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_binding_missing",
                "Authoritative harness state is missing the active worktree lease fingerprint index for the current run.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
        if !context.steps.iter().any(|step| step.checked) {
            return;
        }
        let active_contract_overlay = match load_status_authoritative_overlay_checked(context) {
            Ok(Some(overlay)) => overlay,
            Ok(None) => return,
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_authoritative_state_unavailable",
                    error.message,
                    PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                );
                return;
            }
        };
        let active_contract_path = active_contract_overlay
            .active_contract_path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let active_contract_fingerprint = active_contract_overlay
            .active_contract_fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if active_contract_path.is_none() && active_contract_fingerprint.is_none() {
            enforce_plain_unit_review_truth(context, run_identity.execution_run_id.as_str(), gate);
            return;
        }
        let Some((_active_contract_path, active_contract_fingerprint)) =
            load_authoritative_active_contract(context, gate)
        else {
            return;
        };
        enforce_serial_unit_review_truth(context, run_identity, &active_contract_fingerprint, gate);
        return;
    }
    if current_run_bindings.is_empty() {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_binding_missing",
            "Authoritative harness state is missing one or more active worktree lease bindings for the current run.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return;
    }

    let Some((active_contract_path, active_contract_fingerprint)) =
        load_authoritative_active_contract(context, gate)
    else {
        return;
    };
    let active_contract = match read_execution_contract(&active_contract_path) {
        Ok(contract) => contract,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_contract_unreadable",
                format!(
                    "Authoritative active contract {} is malformed: {error}",
                    active_contract_path.display()
                ),
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
    };
    if active_contract.contract_fingerprint != active_contract_fingerprint {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_contract_unreadable",
            "Authoritative active contract fingerprint does not match its canonical content.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return;
    }
    let current_head = match context.current_head_sha() {
        Ok(head) => head,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_head_unavailable",
                error.message,
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
    };

    let mut binding_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut binding_by_fingerprint: BTreeMap<String, &WorktreeLeaseBindingProbe> = BTreeMap::new();
    for binding in current_run_bindings.iter().copied() {
        let fingerprint = binding.lease_fingerprint.trim().to_owned();
        if fingerprint.is_empty() || !current_run_fingerprints.contains(&fingerprint) {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_binding_missing",
                "Authoritative harness state contains a worktree lease binding that is not indexed by the current runtime state.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
        *binding_counts.entry(fingerprint.clone()).or_insert(0) += 1;
        binding_by_fingerprint.insert(fingerprint, binding);
    }
    if binding_counts.values().any(|count| *count > 1)
        || binding_by_fingerprint.len() != current_run_bindings.len()
        || binding_by_fingerprint.len() != current_run_fingerprints.len()
    {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_authoritative_binding_duplicate",
            "Authoritative harness state contains duplicate or missing active worktree lease bindings for the current run.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return;
    }

    for fingerprint in current_run_fingerprints {
        let binding = binding_by_fingerprint
            .get(&fingerprint)
            .expect("binding should exist for each current lease fingerprint");
        let lease_artifact_path = match normalize_authoritative_artifact_binding_path(
            &binding.lease_artifact_path,
            "worktree lease",
            gate,
        ) {
            Some(path) => path,
            None => return,
        };
        let lease_path = harness_authoritative_artifact_path(
            &context.runtime.state_dir,
            &context.runtime.repo_slug,
            &context.runtime.branch_name,
            lease_artifact_path.to_string_lossy().as_ref(),
        );
        let lease_metadata = match fs::symlink_metadata(&lease_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_metadata_unreadable",
                    format!(
                        "Could not inspect authoritative worktree lease {}: {error}",
                        lease_path.display()
                    ),
                    PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                );
                return;
            }
        };
        if lease_metadata.file_type().is_symlink() || !lease_metadata.is_file() {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_path_not_regular_file",
                format!(
                    "Authoritative worktree lease must be a regular file in {}.",
                    lease_path.display()
                ),
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }

        let source = match fs::read_to_string(&lease_path) {
            Ok(source) => source,
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_unreadable",
                    format!(
                        "Could not read authoritative worktree lease {}: {error}",
                        lease_path.display()
                    ),
                    PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                );
                return;
            }
        };

        let lease: WorktreeLease = match serde_json::from_str(&source) {
            Ok(lease) => lease,
            Err(error) => {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_malformed",
                    format!(
                        "Authoritative worktree lease is malformed in {}: {error}",
                        lease_path.display()
                    ),
                    PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                );
                return;
            }
        };

        let expected_lease_file_name = format!(
            "worktree-lease-{}-{}-{}.json",
            branch_storage_key(&context.runtime.branch_name),
            lease.execution_run_id,
            lease.execution_context_key
        );
        if lease_path.file_name().and_then(|value| value.to_str())
            != Some(expected_lease_file_name.as_str())
        {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_binding_path_invalid",
                "Authoritative worktree lease binding path does not match the canonical runtime-owned filename.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }

        if lease.lease_fingerprint != fingerprint {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_provenance_unindexed",
                "Authoritative worktree lease fingerprint is not indexed by the current runtime state.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }

        if lease.execution_run_id != run_identity.execution_run_id {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_run_id_mismatch",
                "Authoritative worktree lease body does not match the current execution run.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
        if !lease_applies_to_current_plan_context(context, &lease) {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_plan_context_mismatch",
                "Authoritative worktree lease does not match the current plan and execution context.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
        if let Err(error) = validate_worktree_lease(&lease) {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_validation_failed",
                error.message,
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
        if authoritative_context
            .repo_state_baseline_head_sha
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_state_missing",
                "Authoritative harness state is missing the baseline head provenance required for worktree lease gating.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
        if authoritative_context
            .repo_state_baseline_worktree_fingerprint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_authoritative_state_missing",
                "Authoritative harness state is missing the baseline worktree provenance required for worktree lease gating.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
        let expected_execution_context_key = worktree_lease_execution_context_key(
            &run_identity.execution_run_id,
            &lease.execution_unit_id,
            context.plan_rel.as_str(),
            context.plan_document.plan_revision,
            &lease.authoritative_integration_branch,
            lease
                .reviewed_checkpoint_commit_sha
                .as_deref()
                .unwrap_or("open"),
        );
        if lease.execution_context_key.trim() != expected_execution_context_key {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_execution_context_key_mismatch",
                "Authoritative worktree lease body does not match the current execution context.",
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            return;
        }
        if !validate_authoritative_worktree_lease_fingerprint(
            &source,
            &lease,
            lease_path.display().to_string(),
            gate,
        ) {
            return;
        }

        match lease.lease_state {
            WorktreeLeaseState::Open => {
                gate.fail(
                    FailureClass::ExecutionStateNotReady,
                    "worktree_lease_open",
                    "An authoritative worktree lease remains open.",
                    PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                );
                return;
            }
            WorktreeLeaseState::ReviewPassedPendingReconcile => {
                gate.fail(
                    FailureClass::ExecutionStateNotReady,
                    "worktree_lease_reconcile_pending",
                    "An authoritative worktree lease has passed review but not yet been reconciled.",
                    PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                );
                return;
            }
            WorktreeLeaseState::Reconciled | WorktreeLeaseState::Cleaned => {
                let approved_task_packet_fingerprint = binding
                    .approved_task_packet_fingerprint
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let terminal_lease_proof = match approved_task_packet_fingerprint {
                    Some(approved_task_packet_fingerprint) => {
                        if !active_contract
                            .source_task_packet_fingerprints
                            .iter()
                            .any(|candidate| candidate == approved_task_packet_fingerprint)
                        {
                            gate.fail(
                                FailureClass::MalformedExecutionState,
                                "worktree_lease_review_receipt_task_packet_not_authoritative",
                                "The runtime-owned worktree lease review binding does not bind a task packet from the current authoritative contract.",
                                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                            );
                            return;
                        }
                        let Some(terminal_lease_proof) = validate_terminal_worktree_lease_proof(
                            context,
                            &lease,
                            &current_head,
                            gate,
                        ) else {
                            return;
                        };
                        Some(terminal_lease_proof)
                    }
                    None => None,
                };
                let Some(review_receipt_fingerprint) = binding
                    .review_receipt_fingerprint
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_review_receipt_missing",
                        "A runtime-owned worktree lease review binding is required before a cleaned worktree lease can release dependent work.",
                        PUBLIC_CLOSE_CURRENT_TASK_REMEDIATION,
                    );
                    return;
                };
                let Some(approved_task_packet_fingerprint) = approved_task_packet_fingerprint
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_review_receipt_task_packet_missing",
                        "A runtime-owned worktree lease review binding is required to bind the approved task packet before a cleaned worktree lease can release dependent work.",
                        PUBLIC_CLOSE_CURRENT_TASK_REMEDIATION,
                    );
                    return;
                };
                let terminal_lease_proof = match terminal_lease_proof {
                    Some(terminal_lease_proof) => terminal_lease_proof,
                    None => {
                        let Some(terminal_lease_proof) = validate_terminal_worktree_lease_proof(
                            context,
                            &lease,
                            &current_head,
                            gate,
                        ) else {
                            return;
                        };
                        terminal_lease_proof
                    }
                };
                let Some(approved_unit_contract_fingerprint) = binding
                    .approved_unit_contract_fingerprint
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_review_receipt_unit_contract_missing",
                        "A runtime-owned worktree lease review binding is required to bind the approved unit contract before a cleaned worktree lease can release dependent work.",
                        PUBLIC_CLOSE_CURRENT_TASK_REMEDIATION,
                    );
                    return;
                };
                let expected_approved_unit_contract_fingerprint =
                    approved_unit_contract_fingerprint_for_review(
                        active_contract_fingerprint.as_str(),
                        approved_task_packet_fingerprint,
                        lease.execution_unit_id.as_str(),
                    );
                if approved_unit_contract_fingerprint != expected_approved_unit_contract_fingerprint
                {
                    gate.fail(
                        FailureClass::MalformedExecutionState,
                        "worktree_lease_review_receipt_unit_contract_mismatch",
                        "The runtime-owned worktree lease review binding does not bind the canonical approved unit contract fingerprint.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                let Some(reviewed_checkpoint_commit_sha) = binding
                    .reviewed_checkpoint_commit_sha
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_review_receipt_missing",
                        "A runtime-owned worktree lease review binding is required to bind the reviewed checkpoint before a cleaned worktree lease can release dependent work.",
                        PUBLIC_CLOSE_CURRENT_TASK_REMEDIATION,
                    );
                    return;
                };
                let Some(reconcile_mode) = binding
                    .reconcile_mode
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_reconcile_mode_missing",
                        "A runtime-owned worktree lease review binding is required to bind the identity-preserving reconcile mode before a cleaned worktree lease can release dependent work.",
                        PUBLIC_CLOSE_CURRENT_TASK_REMEDIATION,
                    );
                    return;
                };
                let Some(reconcile_result_commit_sha) = binding
                    .reconcile_result_commit_sha
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_identity_preserving_proof_missing",
                        "A runtime-owned worktree lease review binding is required to bind the exact reconciled commit before a cleaned worktree lease can release dependent work.",
                        PUBLIC_CLOSE_CURRENT_TASK_REMEDIATION,
                    );
                    return;
                };
                let Some(reconcile_result_proof_fingerprint) = binding
                    .reconcile_result_proof_fingerprint
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_identity_preserving_proof_missing",
                        "A runtime-owned worktree lease review binding is required to bind the exact reconciled commit object before a cleaned worktree lease can release dependent work.",
                        PUBLIC_CLOSE_CURRENT_TASK_REMEDIATION,
                    );
                    return;
                };
                if reconcile_result_proof_fingerprint
                    != terminal_lease_proof.reconcile_result_proof_fingerprint
                {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_proof_mismatch",
                        "The authoritative worktree lease exact reconciled commit object does not match the runtime-owned worktree lease review binding.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                let Some(review_receipt_path_name) = binding
                    .review_receipt_artifact_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_review_receipt_missing",
                        "A runtime-owned worktree lease review binding is required before a cleaned worktree lease can release dependent work.",
                        PUBLIC_CLOSE_CURRENT_TASK_REMEDIATION,
                    );
                    return;
                };
                let review_receipt_path_name = match normalize_authoritative_artifact_binding_path(
                    review_receipt_path_name,
                    "worktree lease review",
                    gate,
                ) {
                    Some(path) => path,
                    None => return,
                };
                let review_receipt_path = harness_authoritative_artifact_path(
                    &context.runtime.state_dir,
                    &context.runtime.repo_slug,
                    &context.runtime.branch_name,
                    review_receipt_path_name.to_string_lossy().as_ref(),
                );
                let review_metadata = match fs::symlink_metadata(&review_receipt_path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        gate.fail(
                            FailureClass::ExecutionStateNotReady,
                            "worktree_lease_review_receipt_missing",
                            format!(
                                "Could not inspect the runtime-owned worktree lease review binding artifact: {error}"
                            ),
                            PUBLIC_CLOSE_CURRENT_TASK_REMEDIATION,
                        );
                        return;
                    }
                };
                if review_metadata.file_type().is_symlink() || !review_metadata.is_file() {
                    gate.fail(
                        FailureClass::MalformedExecutionState,
                        "worktree_lease_review_receipt_path_not_regular_file",
                        "The runtime-owned worktree lease review binding must be a regular authoritative artifact.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                let expected_review_receipt_filename = format!(
                    "unit-review-{}-{}.md",
                    run_identity.execution_run_id,
                    lease.execution_unit_id.trim_start_matches("unit-")
                );
                if review_receipt_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    != Some(expected_review_receipt_filename.as_str())
                {
                    gate.fail(
                        FailureClass::MalformedExecutionState,
                        "worktree_lease_binding_path_invalid",
                        "The runtime-owned worktree lease review binding path does not match the reviewed execution unit provenance.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                let review_source = match fs::read_to_string(&review_receipt_path) {
                    Ok(source) => source,
                    Err(error) => {
                        gate.fail(
                            FailureClass::ExecutionStateNotReady,
                            "worktree_lease_review_receipt_unreadable",
                            format!(
                                "Could not read the runtime-owned worktree lease review binding artifact: {error}"
                            ),
                            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                        );
                        return;
                    }
                };
                let (receipt_checkpoint_commit_sha, receipt_reconciled_result_commit_sha) =
                    match validate_authoritative_unit_review_receipt(
                        context,
                        &run_identity.execution_run_id,
                        &lease,
                        &review_source,
                        &review_receipt_path,
                        UnitReviewReceiptExpectations {
                            expected_execution_context_key: &expected_execution_context_key,
                            expected_fingerprint: review_receipt_fingerprint,
                            expected_task_packet_fingerprint: approved_task_packet_fingerprint,
                            expected_approved_unit_contract_fingerprint:
                                approved_unit_contract_fingerprint,
                            expected_reconcile_result_commit_sha: terminal_lease_proof
                                .reconcile_result_commit_sha
                                .as_str(),
                        },
                        gate,
                    ) {
                        Some(values) => values,
                        None => return,
                    };

                if reviewed_checkpoint_commit_sha != receipt_checkpoint_commit_sha {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_provenance_mismatch",
                        "Authoritative worktree lease reviewed checkpoint does not match the runtime-owned worktree lease review binding.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                if reconcile_result_commit_sha != receipt_reconciled_result_commit_sha {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_proof_mismatch",
                        "Authoritative worktree lease reconciled result does not match the runtime-owned worktree lease review binding.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                if binding
                    .execution_context_key
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    != Some(expected_execution_context_key.as_str())
                {
                    gate.fail(
                        FailureClass::MalformedExecutionState,
                        "worktree_lease_execution_context_key_mismatch",
                        "Authoritative worktree lease binding does not match the current execution context.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                if reconcile_mode != "identity_preserving"
                    || lease.reconcile_mode.trim() != "identity_preserving"
                {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_reconcile_mode_mismatch",
                        "Authoritative worktree lease does not prove an identity-preserving reconcile.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }

                if lease.reviewed_checkpoint_commit_sha.as_deref()
                    != Some(receipt_checkpoint_commit_sha.as_str())
                {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_review_receipt_checkpoint_mismatch",
                        "Authoritative worktree lease reviewed checkpoint does not match the runtime-owned worktree lease review binding.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                if Some(lease.repo_state_baseline_head_sha.as_str())
                    != authoritative_context
                        .repo_state_baseline_head_sha
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_provenance_mismatch",
                        "Authoritative worktree lease baseline head provenance does not match the current authoritative baseline.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                if Some(lease.repo_state_baseline_worktree_fingerprint.as_str())
                    != authoritative_context
                        .repo_state_baseline_worktree_fingerprint
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_identity_preserving_provenance_mismatch",
                        "Authoritative worktree lease baseline worktree provenance does not match the current authoritative baseline.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                if !is_ancestor_commit(
                    &context.runtime.repo_root,
                    &receipt_checkpoint_commit_sha,
                    reconcile_result_commit_sha,
                ) {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_checkpoint_mismatch",
                        "Authoritative worktree lease reconciled result is not descended from the reviewed checkpoint.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                if !is_ancestor_commit(
                    &context.runtime.repo_root,
                    reconcile_result_commit_sha,
                    &current_head,
                ) {
                    gate.fail(
                        FailureClass::StaleProvenance,
                        "worktree_lease_checkpoint_mismatch",
                        "Authoritative worktree lease reconciled result is not contained in the current branch history.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
                if lease.cleanup_state.trim() != "cleaned" {
                    gate.fail(
                        FailureClass::ExecutionStateNotReady,
                        "worktree_lease_cleanup_pending",
                        "Authoritative worktree lease has not been cleaned up yet.",
                        PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                    );
                    return;
                }
            }
        }
    }
}

fn load_worktree_lease_authoritative_context_checked(
    context: &ExecutionContext,
) -> Result<Option<WorktreeLeaseAuthoritativeContextProbe>, JsonFailure> {
    let state_path = authoritative_state_path(context);
    let Some(payload) = load_reduced_authoritative_state_for_state_path(&state_path)? else {
        return Ok(None);
    };
    let context: WorktreeLeaseAuthoritativeContextProbe =
        serde_json::from_value(strip_top_level_null_fields(payload)).map_err(|error| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative reduced state is malformed in {}: {error}",
                    state_path.display()
                ),
            )
        })?;
    Ok(Some(context))
}

pub(crate) fn worktree_lease_public_gate_reason_code(reason_code: &str) -> bool {
    reason_code.starts_with("worktree_lease_")
}

pub(crate) fn releasable_terminal_worktree_lease_fingerprints_for_task_closure(
    context: &ExecutionContext,
    execution_run_id: Option<&str>,
    active_worktree_lease_fingerprints: &[String],
    active_worktree_lease_bindings: &[WorktreeLeaseBindingSnapshot],
    task_number: u32,
) -> BTreeSet<String> {
    let Some(execution_run_id) = execution_run_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return BTreeSet::new();
    };
    let active_fingerprint_index = active_worktree_lease_fingerprints
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if active_fingerprint_index.is_empty() {
        return BTreeSet::new();
    }
    let mut gate = GateState::default();
    let Some((active_contract_path, active_contract_fingerprint)) =
        load_authoritative_active_contract(context, &mut gate)
    else {
        return BTreeSet::new();
    };
    let Ok(active_contract) = read_execution_contract(&active_contract_path) else {
        return BTreeSet::new();
    };
    if active_contract.contract_fingerprint != active_contract_fingerprint {
        return BTreeSet::new();
    }
    let task_packet_fingerprints =
        task_packet_fingerprints_for_contract_task(context, &active_contract, task_number);
    if task_packet_fingerprints.is_empty() {
        return BTreeSet::new();
    }
    let Ok(current_head) = context.current_head_sha() else {
        return BTreeSet::new();
    };

    active_worktree_lease_bindings
        .iter()
        .filter(|binding| binding.execution_run_id.trim() == execution_run_id)
        .filter(|binding| active_fingerprint_index.contains(binding.lease_fingerprint.as_str()))
        .filter_map(|binding| {
            releasable_terminal_worktree_lease_fingerprint(
                context,
                binding,
                &active_contract,
                &active_contract_fingerprint,
                &current_head,
                &task_packet_fingerprints,
                &mut gate,
            )
        })
        .collect()
}

fn task_packet_fingerprints_for_contract_task(
    context: &ExecutionContext,
    active_contract: &crate::contracts::harness::ExecutionContract,
    task_number: u32,
) -> BTreeSet<String> {
    active_contract
        .covered_steps
        .iter()
        .filter_map(|step| crate::contracts::harness::parse_contract_task_step_scope(step))
        .filter(|(task, _step)| *task == task_number)
        .filter_map(|(_task, step)| {
            task_packet_fingerprint(
                context,
                &active_contract.source_spec_fingerprint,
                task_number,
                step,
            )
        })
        .filter(|fingerprint| {
            active_contract
                .source_task_packet_fingerprints
                .iter()
                .any(|candidate| candidate == fingerprint)
        })
        .collect()
}

fn releasable_terminal_worktree_lease_fingerprint(
    context: &ExecutionContext,
    binding: &WorktreeLeaseBindingSnapshot,
    active_contract: &crate::contracts::harness::ExecutionContract,
    active_contract_fingerprint: &str,
    current_head: &str,
    task_packet_fingerprints: &BTreeSet<String>,
    gate: &mut GateState,
) -> Option<String> {
    let approved_task_packet_fingerprint = binding
        .approved_task_packet_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if !task_packet_fingerprints.contains(approved_task_packet_fingerprint)
        || !active_contract
            .source_task_packet_fingerprints
            .iter()
            .any(|candidate| candidate == approved_task_packet_fingerprint)
    {
        return None;
    }
    let lease = load_worktree_lease_for_release(context, binding, gate)?;
    let approved_unit_contract_fingerprint = binding
        .approved_unit_contract_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if approved_unit_contract_fingerprint
        != approved_unit_contract_fingerprint_for_review(
            active_contract_fingerprint,
            approved_task_packet_fingerprint,
            lease.execution_unit_id.as_str(),
        )
    {
        return None;
    }
    if binding
        .execution_context_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        != Some(lease.execution_context_key.as_str())
    {
        return None;
    }
    let terminal_proof =
        validate_terminal_worktree_lease_proof(context, &lease, current_head, gate)?;
    if binding
        .reviewed_checkpoint_commit_sha
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        != lease.reviewed_checkpoint_commit_sha.as_deref()
    {
        return None;
    }
    if binding
        .reconcile_result_commit_sha
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        != Some(terminal_proof.reconcile_result_commit_sha.as_str())
    {
        return None;
    }
    if binding
        .reconcile_result_proof_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        != Some(terminal_proof.reconcile_result_proof_fingerprint.as_str())
    {
        return None;
    }
    if binding
        .reconcile_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        != Some("identity_preserving")
        || lease.reconcile_mode.trim() != "identity_preserving"
    {
        return None;
    }
    Some(binding.lease_fingerprint.trim().to_owned())
}

fn load_worktree_lease_for_release(
    context: &ExecutionContext,
    binding: &WorktreeLeaseBindingSnapshot,
    gate: &mut GateState,
) -> Option<WorktreeLease> {
    let lease_artifact_path = normalize_authoritative_artifact_binding_path(
        binding.lease_artifact_path.as_str(),
        "worktree lease",
        gate,
    )?;
    let lease_path = harness_authoritative_artifact_path(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
        lease_artifact_path.to_string_lossy().as_ref(),
    );
    let metadata = fs::symlink_metadata(&lease_path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let source = fs::read_to_string(&lease_path).ok()?;
    let lease: WorktreeLease = serde_json::from_str(&source).ok()?;
    let expected_lease_file_name = format!(
        "worktree-lease-{}-{}-{}.json",
        branch_storage_key(&context.runtime.branch_name),
        lease.execution_run_id,
        lease.execution_context_key
    );
    if lease_path.file_name().and_then(|value| value.to_str())
        != Some(expected_lease_file_name.as_str())
    {
        return None;
    }
    if lease.lease_fingerprint != binding.lease_fingerprint.trim() {
        return None;
    }
    if lease.execution_run_id != binding.execution_run_id.trim() {
        return None;
    }
    if !lease_applies_to_current_plan_context(context, &lease) {
        return None;
    }
    if validate_worktree_lease(&lease).is_err() {
        return None;
    }
    if !validate_authoritative_worktree_lease_fingerprint(
        &source,
        &lease,
        lease_path.display().to_string(),
        gate,
    ) {
        return None;
    }
    Some(lease)
}

struct TerminalWorktreeLeaseProof {
    reconcile_result_commit_sha: String,
    reconcile_result_proof_fingerprint: String,
}

fn validate_terminal_worktree_lease_proof(
    context: &ExecutionContext,
    lease: &WorktreeLease,
    current_head: &str,
    gate: &mut GateState,
) -> Option<TerminalWorktreeLeaseProof> {
    let Some(reviewed_checkpoint_commit_sha) = lease
        .reviewed_checkpoint_commit_sha
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "worktree_lease_identity_preserving_provenance_missing",
            "An authoritative worktree lease is missing the reviewed checkpoint required to release dependent work.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    };
    let Some(reconcile_result_commit_sha) = lease
        .reconcile_result_commit_sha
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "worktree_lease_identity_preserving_proof_missing",
            "An authoritative worktree lease is missing the exact reconciled commit proof required to release dependent work.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    };
    let Some(reconcile_result_proof_fingerprint) = lease
        .reconcile_result_proof_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "worktree_lease_identity_preserving_proof_missing",
            "An authoritative worktree lease is missing the exact reconciled commit object proof required to release dependent work.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    };
    let Some(computed_reconcile_result_proof_fingerprint) =
        reconcile_result_proof_fingerprint_for_review(
            &context.runtime.repo_root,
            reconcile_result_commit_sha,
        )
    else {
        gate.fail(
            FailureClass::MalformedExecutionState,
            "worktree_lease_identity_preserving_proof_unverifiable",
            "The authoritative worktree lease exact reconcile proof could not be verified against repository history.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    };
    if reconcile_result_proof_fingerprint != computed_reconcile_result_proof_fingerprint {
        gate.fail(
            FailureClass::StaleProvenance,
            "worktree_lease_identity_preserving_lease_proof_mismatch",
            "The authoritative worktree lease exact reconciled commit object proof does not match the reviewed reconcile proof.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if !is_ancestor_commit(
        &context.runtime.repo_root,
        reviewed_checkpoint_commit_sha,
        reconcile_result_commit_sha,
    ) {
        gate.fail(
            FailureClass::StaleProvenance,
            "worktree_lease_checkpoint_mismatch",
            "Authoritative worktree lease reconciled result is not descended from the reviewed checkpoint.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if !is_ancestor_commit(
        &context.runtime.repo_root,
        reconcile_result_commit_sha,
        current_head,
    ) {
        gate.fail(
            FailureClass::StaleProvenance,
            "worktree_lease_checkpoint_mismatch",
            "Authoritative worktree lease reconciled result is not contained in the current branch history.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    if lease.cleanup_state.trim() != "cleaned" {
        gate.fail(
            FailureClass::ExecutionStateNotReady,
            "worktree_lease_cleanup_pending",
            "Authoritative worktree lease has not been cleaned up yet.",
            PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
        );
        return None;
    }
    Some(TerminalWorktreeLeaseProof {
        reconcile_result_commit_sha: reconcile_result_commit_sha.to_owned(),
        reconcile_result_proof_fingerprint: computed_reconcile_result_proof_fingerprint,
    })
}

fn strip_top_level_null_fields(mut payload: serde_json::Value) -> serde_json::Value {
    if let Some(object) = payload.as_object_mut() {
        object.retain(|_, value| !value.is_null());
    }
    payload
}

fn lease_applies_to_current_plan_context(
    context: &ExecutionContext,
    lease: &WorktreeLease,
) -> bool {
    lease.source_plan_path == context.plan_rel
        && lease.source_plan_revision == context.plan_document.plan_revision
        && lease.authoritative_integration_branch == context.runtime.branch_name
        && !lease.source_branch.trim().is_empty()
}

fn normalize_authoritative_artifact_binding_path(
    raw_path: &str,
    artifact_kind: &str,
    gate: &mut GateState,
) -> Option<PathBuf> {
    let trimmed = raw_path.trim();
    let mut components = Path::new(trimmed).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(component)), None) => {
            let filename = component.to_string_lossy();
            if filename.is_empty() {
                gate.fail(
                    FailureClass::MalformedExecutionState,
                    "worktree_lease_binding_path_invalid",
                    format!(
                        "Authoritative {artifact_kind} binding path must be a normalized relative filename."
                    ),
                    PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
                );
                None
            } else {
                Some(PathBuf::from(filename.as_ref()))
            }
        }
        _ => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "worktree_lease_binding_path_invalid",
                format!(
                    "Authoritative {artifact_kind} binding path must be a normalized relative filename."
                ),
                PUBLIC_REPAIR_REVIEW_STATE_REMEDIATION,
            );
            None
        }
    }
}

fn current_run_worktree_lease_artifacts_exist(
    context: &ExecutionContext,
    execution_run_id: &str,
    released_worktree_lease_records: &[WorktreeLeaseReleaseRecord],
) -> Result<bool, String> {
    let artifacts_dir = harness_authoritative_artifacts_dir(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
    );
    let entries = match fs::read_dir(&artifacts_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Could not inspect authoritative worktree leases in {}: {error}",
                artifacts_dir.display()
            ));
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Could not inspect authoritative worktree leases in {}: {error}",
                artifacts_dir.display()
            )
        })?;
        let file_path = entry.path();
        let Some(file_name) = file_path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.ends_with(".json") {
            continue;
        }
        let canonical_prefix = format!(
            "worktree-lease-{}-{}-",
            branch_storage_key(&context.runtime.branch_name),
            execution_run_id
        );
        let canonical_candidate = file_name.starts_with(&canonical_prefix);
        let metadata = match fs::symlink_metadata(&file_path) {
            Ok(metadata) => metadata,
            Err(error) if canonical_candidate => {
                return Err(format!(
                    "Could not inspect authoritative worktree lease {}: {error}",
                    file_path.display()
                ));
            }
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            if canonical_candidate {
                return Err(format!(
                    "Authoritative worktree lease must be a regular file in {}.",
                    file_path.display()
                ));
            }
            continue;
        }
        let Ok(source) = fs::read_to_string(&file_path) else {
            if canonical_candidate {
                return Err(format!(
                    "Could not read authoritative worktree lease {}.",
                    file_path.display()
                ));
            }
            continue;
        };
        let lease = match serde_json::from_str::<WorktreeLease>(&source) {
            Ok(lease) => lease,
            Err(error) if canonical_candidate => {
                return Err(format!(
                    "Authoritative worktree lease is malformed in {}: {error}",
                    file_path.display()
                ));
            }
            Err(_) => continue,
        };
        let matches_current_run = lease.execution_run_id == execution_run_id
            && lease.source_plan_path == context.plan_rel
            && lease.source_plan_revision == context.plan_document.plan_revision
            && lease.authoritative_integration_branch == context.runtime.branch_name;
        if !matches_current_run {
            if canonical_candidate {
                return Err(format!(
                    "Authoritative worktree lease {} does not match the current run context.",
                    file_path.display()
                ));
            }
            continue;
        }
        let reviewed_checkpoint_commit_sha = lease
            .reviewed_checkpoint_commit_sha
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("open");
        let expected_execution_context_key = worktree_lease_execution_context_key(
            execution_run_id,
            lease.execution_unit_id.as_str(),
            context.plan_rel.as_str(),
            context.plan_document.plan_revision,
            lease.authoritative_integration_branch.as_str(),
            reviewed_checkpoint_commit_sha,
        );
        if lease.execution_context_key != expected_execution_context_key {
            if canonical_candidate {
                return Err(format!(
                    "Authoritative worktree lease {} does not match the current execution context.",
                    file_path.display()
                ));
            }
            continue;
        }
        if let Err(error) = validate_worktree_lease(&lease) {
            if canonical_candidate || matches_current_run {
                return Err(error.message);
            }
            continue;
        }
        if matches!(
            lease.lease_state,
            WorktreeLeaseState::Reconciled | WorktreeLeaseState::Cleaned
        ) && lease.cleanup_state.trim() == "cleaned"
        {
            let current_head = context
                .current_head_sha()
                .map_err(|error| error.message.clone())?;
            let mut proof_gate = GateState::default();
            if validate_terminal_worktree_lease_proof(
                context,
                &lease,
                &current_head,
                &mut proof_gate,
            )
            .is_some()
            {
                if canonical_candidate
                    && released_worktree_lease_records.iter().any(|record| {
                        record.execution_run_id == execution_run_id
                            && record.lease_fingerprint == lease.lease_fingerprint
                    })
                {
                    continue;
                }
                return Ok(true);
            }
            if canonical_candidate {
                return Err(proof_gate
                    .diagnostics
                    .into_iter()
                    .next()
                    .map(|reason| reason.message)
                    .unwrap_or_else(|| {
                        String::from(
                            "Authoritative terminal worktree lease artifact is not releaseable.",
                        )
                    }));
            }
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}

fn worktree_or_unit_review_binding_artifacts_exist(
    context: &ExecutionContext,
) -> Result<bool, String> {
    let artifacts_dir = harness_authoritative_artifacts_dir(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
    );
    let entries = match fs::read_dir(&artifacts_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "Could not inspect authoritative worktree leases in {}: {error}",
                artifacts_dir.display()
            ));
        }
    };
    Ok(entries.flatten().any(|entry| {
        entry
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| {
                (value.starts_with("worktree-lease-") && value.ends_with(".json"))
                    || (value.starts_with("unit-review-") && value.ends_with(".md"))
            })
    }))
}

pub(super) fn current_run_plain_unit_review_receipt_paths(
    context: &ExecutionContext,
    execution_run_id: &str,
) -> Result<Vec<PathBuf>, String> {
    let artifacts_dir = harness_authoritative_artifacts_dir(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
    );
    let entries = match fs::read_dir(&artifacts_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(format!(
                "Could not inspect authoritative unit-review receipts in {}: {error}",
                artifacts_dir.display()
            ));
        }
    };
    let canonical_prefix = format!("unit-review-{execution_run_id}-task-");
    let mut receipt_paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "Could not inspect authoritative unit-review receipts in {}: {error}",
                artifacts_dir.display()
            )
        })?;
        let file_path = entry.path();
        let Some(file_name) = file_path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if file_name.starts_with(&canonical_prefix) && file_name.ends_with(".md") {
            receipt_paths.push(file_path);
        }
    }
    receipt_paths.sort();
    Ok(receipt_paths)
}
