use super::*;

pub fn gate_finish_from_context(context: &ExecutionContext) -> GateResult {
    let mut gate = GateState::default();
    enforce_finish_dependency_index_truth(context, &mut gate);
    merge_gate_result(&mut gate, gate_review_base_result(context, false));
    if !gate.allowed {
        return gate.finish();
    }
    let pre_checkpoint_allowed =
        evaluate_pre_checkpoint_finish_gate(context, &mut gate) && gate.allowed;
    if gate
        .reason_codes
        .iter()
        .any(|code| code == "qa_requirement_missing_or_invalid")
    {
        return gate.finish();
    }
    let review_truth_result = gate_review_from_context(context);
    merge_gate_result_without_failure_class(&mut gate, &review_truth_result);
    if !review_truth_result.allowed {
        gate.allowed = false;
        if should_replace_gate_failure_class(
            &gate.failure_class,
            &review_truth_result.failure_class,
        ) {
            gate.failure_class = review_truth_result.failure_class.clone();
        }
    }
    if !pre_checkpoint_allowed || !gate.allowed {
        return gate.finish();
    }

    match finish_review_gate_checkpoint_matches_current_branch_closure(context) {
        Ok(true) => {}
        Ok(false) => {
            gate.fail(
                FailureClass::ExecutionStateNotReady,
                "finish_review_gate_checkpoint_missing",
                "Finish readiness requires a persisted gate-review pass checkpoint for the current branch closure.",
                format!(
                    "Run `featureforge workflow operator --plan {}` and complete the recommended public command sequence before finishing.",
                    context.plan_rel
                ),
            );
        }
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "finish_review_gate_checkpoint_unavailable",
                format!(
                    "Finish readiness could not validate the persisted gate-review pass checkpoint: {}",
                    error.message
                ),
                "Restore authoritative finish-gate checkpoint state before running gate-finish.",
            );
        }
    }

    gate.finish()
}

pub(super) fn finish_review_gate_checkpoint_matches_current_branch_closure(
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    let Some(current_branch_closure_id) = current_branch_closure_id(context) else {
        return Ok(false);
    };
    let Some(authoritative_state) = load_authoritative_transition_state(context)? else {
        return Ok(false);
    };
    Ok(authoritative_state
        .finish_review_gate_pass_branch_closure_id()
        .as_deref()
        == Some(current_branch_closure_id.as_str()))
}

fn merge_gate_result(target: &mut GateState, incoming: GateResult) {
    merge_gate_result_impl(target, incoming, true);
}

fn merge_gate_result_without_failure_class(target: &mut GateState, incoming: &GateResult) {
    merge_gate_result_impl(target, incoming.clone(), false);
}

fn merge_gate_result_impl(target: &mut GateState, incoming: GateResult, merge_failure_class: bool) {
    let GateResult {
        allowed,
        action: _,
        failure_class,
        reason_codes,
        warning_codes,
        diagnostics,
        code: _,
        workspace_state_id: _,
        current_branch_reviewed_state_id: _,
        current_branch_closure_id: _,
        finish_review_gate_pass_branch_closure_id: _,
        recommended_command: _,
        rederive_via_workflow_operator: _,
    } = incoming;

    if !allowed {
        target.allowed = false;
    }
    if merge_failure_class
        && should_replace_gate_failure_class(&target.failure_class, &failure_class)
    {
        target.failure_class = failure_class;
    }

    for code in reason_codes {
        if !target.reason_codes.iter().any(|existing| existing == &code) {
            target.reason_codes.push(code);
        }
    }
    for code in warning_codes {
        if !target
            .warning_codes
            .iter()
            .any(|existing| existing == &code)
        {
            target.warning_codes.push(code);
        }
    }
    for diagnostic in diagnostics {
        if !target
            .diagnostics
            .iter()
            .any(|existing| existing.code == diagnostic.code)
        {
            target.diagnostics.push(diagnostic);
        }
    }
}

fn should_replace_gate_failure_class(current: &str, incoming: &str) -> bool {
    if incoming.is_empty() {
        return false;
    }
    current.is_empty()
        || (current == FailureClass::StaleProvenance.as_str()
            && incoming != FailureClass::StaleProvenance.as_str())
}

pub(super) fn enforce_review_authoritative_late_gate_truth(
    context: &ExecutionContext,
    gate: &mut GateState,
) {
    let overlay = match load_status_authoritative_overlay_checked(context) {
        Ok(overlay) => overlay,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_state_unavailable",
                error.message,
                "Restore authoritative harness state readability and validity before running gate-review.",
            );
            return;
        }
    };
    let Some(overlay) = overlay else {
        return;
    };

    validate_review_dependency_index_truth(overlay.dependency_index_state.as_deref(), gate);
    let authoritative_state = match load_authoritative_transition_state(context) {
        Ok(Some(authoritative_state)) => authoritative_state,
        Ok(None) => return,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_state_unavailable",
                error.message,
                "Restore authoritative event-log state before running gate-review.",
            );
            return;
        }
    };
    let Some(current_identity) = usable_current_branch_closure_identity_from_authoritative_state(
        context,
        Some(&authoritative_state),
    ) else {
        return;
    };
    let late_stage_bindings = shared_current_late_stage_branch_bindings(
        Some(&authoritative_state),
        Some(&current_identity.branch_closure_id),
        Some(&current_identity.reviewed_state_id),
    );
    if late_stage_bindings
        .current_release_readiness_result
        .is_none()
    {
        fail_missing_review_downstream_truth("release_docs_state", "release docs", gate);
    }
    if late_stage_bindings.current_final_review_result.is_none() {
        fail_missing_review_downstream_truth("final_review_state", "final review", gate);
    }
    if context.plan_document.qa_requirement.as_deref() == Some("required")
        && late_stage_bindings.current_qa_result.is_none()
    {
        fail_missing_review_downstream_truth("browser_qa_state", "browser QA", gate);
    }
}

fn fail_missing_review_downstream_truth(field_name: &str, field_label: &str, gate: &mut GateState) {
    gate.fail(
        FailureClass::StaleProvenance,
        &format!("{field_name}_missing"),
        format!("Authoritative {field_label} truth is missing for review readiness."),
        "Refresh authoritative late-gate truth before running gate-review.",
    );
}

fn enforce_finish_dependency_index_truth(context: &ExecutionContext, gate: &mut GateState) {
    let overlay = match load_status_authoritative_overlay_checked(context) {
        Ok(overlay) => overlay,
        Err(error) => {
            gate.fail(
                FailureClass::MalformedExecutionState,
                "authoritative_state_unavailable",
                error.message,
                "Restore authoritative harness state readability and validity before running gate-finish.",
            );
            return;
        }
    };
    let Some(overlay) = overlay else {
        return;
    };

    validate_finish_dependency_index_truth(overlay.dependency_index_state.as_deref(), gate);
}

fn validate_review_dependency_index_truth(raw_state: Option<&str>, gate: &mut GateState) {
    let state = normalize_optional_overlay_value(raw_state).unwrap_or("missing");
    if state == "fresh" {
        return;
    }

    let (code, message) = match state {
        "missing" => (
            "dependency_index_state_missing",
            "Authoritative dependency-index truth is missing for review readiness.",
        ),
        "stale" => (
            "dependency_index_state_stale",
            "Authoritative dependency-index truth is stale for review readiness.",
        ),
        _ => (
            "dependency_index_state_not_fresh",
            "Authoritative dependency-index truth is not fresh for review readiness.",
        ),
    };
    gate.fail(
        FailureClass::DependencyIndexMismatch,
        code,
        message,
        "Refresh authoritative dependency-index truth before running gate-review.",
    );
}

fn validate_finish_dependency_index_truth(raw_state: Option<&str>, gate: &mut GateState) {
    let state = normalize_optional_overlay_value(raw_state).unwrap_or("missing");
    if state == "fresh" {
        return;
    }

    let (code, message) = match state {
        "missing" => (
            "dependency_index_state_missing",
            "Authoritative dependency-index truth is missing for finish readiness.",
        ),
        "stale" => (
            "dependency_index_state_stale",
            "Authoritative dependency-index truth is stale for finish readiness.",
        ),
        _ => (
            "dependency_index_state_not_fresh",
            "Authoritative dependency-index truth is not fresh for finish readiness.",
        ),
    };
    gate.fail(
        FailureClass::DependencyIndexMismatch,
        code,
        message,
        "Refresh authoritative dependency-index truth before running gate-finish.",
    );
}
