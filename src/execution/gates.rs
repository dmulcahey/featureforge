use std::collections::{BTreeMap, BTreeSet};
use std::{fs, path::Path};

use serde::Deserialize;

use crate::contracts::harness::{
    EvaluationReport, ExecutionContract, ExecutionHandoff, parse_contract_task_step_scope,
    read_evaluation_report, read_evidence_artifact, read_execution_contract,
    read_execution_handoff,
};
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::context::{
    hash_contract_plan, load_execution_context_without_authority_overlay, task_packet_fingerprint,
};
use crate::execution::event_log::load_reduced_authoritative_state;
use crate::execution::internal_args::{GateContractArgs, GateEvaluatorArgs, GateHandoffArgs};
use crate::execution::state::{ExecutionContext, ExecutionRuntime, GateResult, GateState};
use crate::execution::status_support::{
    PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT, WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND,
};
use crate::git::sha256_hex;
use crate::paths::{
    harness_authoritative_artifact_path, harness_authoritative_artifacts_dir, harness_state_path,
    normalize_repo_relative_path,
};

const HARNESS_OWNED_PRODUCERS: &[&str] = &[
    "featureforge:executing-plans",
    "featureforge:subagent-driven-development",
    "featureforge:spec_compliance",
    "featureforge:code_quality",
];

pub(crate) fn public_gate_remediation(
    context: &ExecutionContext,
    action: impl AsRef<str>,
) -> String {
    public_gate_remediation_for_plan(&context.plan_rel, action)
}

pub(crate) fn public_gate_remediation_for_plan(plan_rel: &str, action: impl AsRef<str>) -> String {
    let diagnostic_context = action.as_ref().trim().trim_end_matches('.');
    let mut remediation = format!(
        "Query workflow/operator JSON for the approved plan by running `{WORKFLOW_OPERATOR_JSON_DISPLAY_COMMAND}` for `{plan_rel}`; {PUBLIC_TYPED_OPERATOR_ROUTE_CONTRACT}."
    );
    if !diagnostic_context.is_empty() {
        remediation.push_str(" Diagnostic context: ");
        remediation.push_str(diagnostic_context);
        remediation.push('.');
    }
    remediation.push_str(" Do not hand-edit or reconstruct proof artifacts.");
    remediation
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct GateAuthorityState {
    #[serde(default)]
    pub(crate) harness_phase: Option<String>,
    #[serde(default)]
    pub(crate) latest_authoritative_sequence: Option<u64>,
    #[serde(default)]
    pub(crate) authoritative_sequence: Option<u64>,
    #[serde(default)]
    pub(crate) active_contract_path: Option<String>,
    #[serde(default)]
    pub(crate) active_contract_fingerprint: Option<String>,
    #[serde(default)]
    pub(crate) required_evaluator_kinds: Vec<String>,
    #[serde(default)]
    pub(crate) handoff_required: bool,
    #[serde(default)]
    pub(crate) open_failed_criteria: Vec<String>,
}

impl GateAuthorityState {
    fn latest_authoritative_sequence(&self) -> u64 {
        self.latest_authoritative_sequence
            .or(self.authoritative_sequence)
            .unwrap_or(0)
    }
}

#[derive(Debug)]
pub(crate) struct ActiveContractState {
    pub(crate) fingerprint: String,
    pub(crate) contract: ExecutionContract,
}

pub fn gate_contract(
    runtime: &ExecutionRuntime,
    args: &GateContractArgs,
) -> Result<GateResult, JsonFailure> {
    let context = load_execution_context_without_authority_overlay(runtime, &args.plan)?;
    gate_contract_from_context(&context, &args.contract)
}

pub fn gate_contract_from_context(
    context: &ExecutionContext,
    artifact_path: &Path,
) -> Result<GateResult, JsonFailure> {
    let mut gate = GateState::default();
    let artifact_rel = normalize_artifact_repo_path(artifact_path, "Contract")?;
    let artifact_abs = context.runtime.repo_root.join(&artifact_rel);
    let contract = match read_execution_contract(&artifact_abs) {
        Ok(contract) => contract,
        Err(error) => {
            gate.fail(
                FailureClass::ContractMismatch,
                "contract_artifact_unreadable",
                error.to_string(),
                public_gate_remediation(
                    context,
                    "Refresh the execution contract artifact through the routed public workflow",
                ),
            );
            return Ok(gate.finish());
        }
    };
    let contract_source = match fs::read_to_string(&artifact_abs) {
        Ok(source) => source,
        Err(error) => {
            gate.fail(
                FailureClass::ContractMismatch,
                "contract_artifact_unreadable",
                format!(
                    "Could not read execution contract artifact {}: {error}",
                    artifact_abs.display()
                ),
                public_gate_remediation(
                    context,
                    "Refresh the execution contract artifact through the routed public workflow",
                ),
            );
            return Ok(gate.finish());
        }
    };
    if verify_declared_fingerprint(
        &contract_source,
        DeclaredFingerprintValidation {
            header_label: "Contract Fingerprint",
            declared_fingerprint: &contract.contract_fingerprint,
            unverifiable_reason_code: "contract_fingerprint_unverifiable",
            mismatch_reason_code: "contract_fingerprint_mismatch",
            artifact_label: "contract",
            plan_rel: &context.plan_rel,
        },
        &mut gate,
    )
    .is_none()
    {
        return Ok(gate.finish());
    }

    validate_contract_provenance(context, &contract, &mut gate);
    validate_harness_provenance(
        &contract.generated_by,
        FailureClass::NonHarnessProvenance,
        "contract_non_harness_provenance",
        &context.plan_rel,
        &mut gate,
    );
    validate_contract_authority_state(context, &contract, &mut gate);
    Ok(gate.finish())
}

pub fn gate_evaluator(
    runtime: &ExecutionRuntime,
    args: &GateEvaluatorArgs,
) -> Result<GateResult, JsonFailure> {
    let context = load_execution_context_without_authority_overlay(runtime, &args.plan)?;
    gate_evaluator_from_context(&context, &args.evaluation)
}

pub fn gate_evaluator_from_context(
    context: &ExecutionContext,
    artifact_path: &Path,
) -> Result<GateResult, JsonFailure> {
    let mut gate = GateState::default();
    let artifact_rel = normalize_artifact_repo_path(artifact_path, "Evaluation")?;
    let artifact_abs = context.runtime.repo_root.join(&artifact_rel);
    let report = match read_evaluation_report(&artifact_abs) {
        Ok(report) => report,
        Err(error) => {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "evaluation_artifact_unreadable",
                error.to_string(),
                public_gate_remediation(
                    context,
                    "Refresh the evaluation artifact through the routed public workflow",
                ),
            );
            return Ok(gate.finish());
        }
    };
    let report_source = match fs::read_to_string(&artifact_abs) {
        Ok(source) => source,
        Err(error) => {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "evaluation_artifact_unreadable",
                format!(
                    "Could not read evaluation report artifact {}: {error}",
                    artifact_abs.display()
                ),
                public_gate_remediation(
                    context,
                    "Refresh the evaluation artifact through the routed public workflow",
                ),
            );
            return Ok(gate.finish());
        }
    };
    if verify_declared_fingerprint(
        &report_source,
        DeclaredFingerprintValidation {
            header_label: "Report Fingerprint",
            declared_fingerprint: &report.report_fingerprint,
            unverifiable_reason_code: "evaluation_fingerprint_unverifiable",
            mismatch_reason_code: "evaluation_fingerprint_mismatch",
            artifact_label: "evaluation",
            plan_rel: &context.plan_rel,
        },
        &mut gate,
    )
    .is_none()
    {
        return Ok(gate.finish());
    }

    validate_report_provenance(context, &report, &mut gate);
    validate_harness_provenance(
        &report.generated_by,
        FailureClass::NonHarnessProvenance,
        "evaluation_non_harness_provenance",
        &context.plan_rel,
        &mut gate,
    );
    validate_evaluator_authority_state(context, &report, &mut gate);

    if !matches!(report.verdict.as_str(), "pass" | "fail" | "blocked") {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_verdict_illegal",
            "Evaluation verdict must be pass, fail, or blocked.",
            public_gate_remediation(
                context,
                "Refresh the evaluation report with a legal verdict",
            ),
        );
    }
    if !matches!(
        report.recommended_action.as_str(),
        "continue" | "repair" | "pivot" | "handoff" | "escalate"
    ) {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_recommended_action_illegal",
            "Evaluation recommended_action must be continue, repair, pivot, handoff, or escalate.",
            public_gate_remediation(
                context,
                "Refresh the evaluation report with a legal recommended_action",
            ),
        );
    }

    Ok(gate.finish())
}

pub fn gate_handoff(
    runtime: &ExecutionRuntime,
    args: &GateHandoffArgs,
) -> Result<GateResult, JsonFailure> {
    let context = load_execution_context_without_authority_overlay(runtime, &args.plan)?;
    gate_handoff_from_context(&context, &args.handoff)
}

pub fn gate_handoff_from_context(
    context: &ExecutionContext,
    artifact_path: &Path,
) -> Result<GateResult, JsonFailure> {
    let mut gate = GateState::default();
    let artifact_rel = normalize_artifact_repo_path(artifact_path, "Handoff")?;
    let artifact_abs = context.runtime.repo_root.join(&artifact_rel);
    let handoff = match read_execution_handoff(&artifact_abs) {
        Ok(handoff) => handoff,
        Err(error) => {
            gate.fail(
                FailureClass::MissingRequiredHandoff,
                "handoff_artifact_unreadable",
                error.to_string(),
                public_gate_remediation(
                    context,
                    "Refresh the execution handoff artifact through the routed public workflow",
                ),
            );
            return Ok(gate.finish());
        }
    };
    let handoff_source = match fs::read_to_string(&artifact_abs) {
        Ok(source) => source,
        Err(error) => {
            gate.fail(
                FailureClass::MissingRequiredHandoff,
                "handoff_artifact_unreadable",
                format!(
                    "Could not read execution handoff artifact {}: {error}",
                    artifact_abs.display()
                ),
                public_gate_remediation(
                    context,
                    "Refresh the execution handoff artifact through the routed public workflow",
                ),
            );
            return Ok(gate.finish());
        }
    };
    if verify_declared_fingerprint(
        &handoff_source,
        DeclaredFingerprintValidation {
            header_label: "Handoff Fingerprint",
            declared_fingerprint: &handoff.handoff_fingerprint,
            unverifiable_reason_code: "handoff_fingerprint_unverifiable",
            mismatch_reason_code: "handoff_fingerprint_mismatch",
            artifact_label: "handoff",
            plan_rel: &context.plan_rel,
        },
        &mut gate,
    )
    .is_none()
    {
        return Ok(gate.finish());
    }

    validate_handoff_provenance(context, &handoff, &mut gate);
    validate_harness_provenance(
        &handoff.generated_by,
        FailureClass::NonHarnessProvenance,
        "handoff_non_harness_provenance",
        &context.plan_rel,
        &mut gate,
    );
    validate_handoff_authority_state(context, &handoff, &mut gate);

    if !handoff.open_criteria.is_empty() && handoff.open_findings.is_empty() {
        gate.fail(
            FailureClass::MissingRequiredHandoff,
            "handoff_unresolved_criteria_missing_findings",
            "Execution handoff with unresolved criteria must include open findings.",
            public_gate_remediation(
                context,
                "Refresh the handoff with concrete unresolved findings",
            ),
        );
    }

    Ok(gate.finish())
}

pub(crate) fn normalize_artifact_repo_path(
    artifact_path: &Path,
    artifact_label: &str,
) -> Result<String, JsonFailure> {
    normalize_repo_relative_path(&artifact_path.to_string_lossy()).map_err(|_| {
        JsonFailure::new(
            FailureClass::InvalidCommandInput,
            format!("{artifact_label} path must be a normalized repo-relative path."),
        )
    })
}

pub(crate) fn validate_contract_provenance(
    context: &ExecutionContext,
    contract: &ExecutionContract,
    gate: &mut GateState,
) {
    if contract.source_plan_path != context.plan_rel
        || contract.source_plan_revision != context.plan_document.plan_revision
    {
        gate.fail(
            FailureClass::StaleProvenance,
            "contract_plan_provenance_mismatch",
            "Execution contract does not match the current approved plan path/revision.",
            public_gate_remediation(
                context,
                "Refresh the contract for the current approved plan revision",
            ),
        );
    }
    if contract.source_spec_path != context.plan_document.source_spec_path
        || contract.source_spec_revision != context.plan_document.source_spec_revision
    {
        gate.fail(
            FailureClass::StaleProvenance,
            "contract_spec_provenance_mismatch",
            "Execution contract does not match the current approved source spec path/revision.",
            public_gate_remediation(
                context,
                "Refresh the contract for the current approved spec revision",
            ),
        );
    }
    let expected_plan_fingerprint = hash_contract_plan(&context.plan_source);
    if contract.source_plan_fingerprint != expected_plan_fingerprint {
        gate.fail(
            FailureClass::StaleProvenance,
            "contract_plan_fingerprint_mismatch",
            "Execution contract plan fingerprint no longer matches the approved plan source.",
            public_gate_remediation(
                context,
                "Refresh the contract for the current approved plan source",
            ),
        );
    }
    let expected_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    if contract.source_spec_fingerprint != expected_spec_fingerprint {
        gate.fail(
            FailureClass::StaleProvenance,
            "contract_spec_fingerprint_mismatch",
            "Execution contract source spec fingerprint no longer matches the approved source spec content.",
            public_gate_remediation(
                context,
                "Refresh the contract for the current approved source spec content",
            ),
        );
    }
    validate_contract_scope_against_approved_plan(
        context,
        contract,
        &expected_plan_fingerprint,
        &expected_spec_fingerprint,
        gate,
    );
}

fn validate_contract_scope_against_approved_plan(
    context: &ExecutionContext,
    contract: &ExecutionContract,
    _expected_plan_fingerprint: &str,
    expected_spec_fingerprint: &str,
    gate: &mut GateState,
) {
    let approved_steps: BTreeSet<(u32, u32)> = context
        .steps
        .iter()
        .map(|step| (step.task_number, step.step_number))
        .collect();
    let mut contract_scope: BTreeSet<(u32, u32)> = BTreeSet::new();

    for step in &contract.covered_steps {
        let Some(step_ref) = parse_contract_task_step_scope(step) else {
            gate.fail(
                FailureClass::ContractMismatch,
                "contract_covered_step_malformed",
                "Execution contract covered_steps entries must use `Task <n> Step <m>` scope format.",
                public_gate_remediation(
                    context,
                    "Refresh the contract with canonical covered_steps scope labels",
                ),
            );
            continue;
        };
        if !approved_steps.contains(&step_ref) {
            gate.fail(
                FailureClass::ContractMismatch,
                "contract_covered_step_out_of_scope",
                "Execution contract covered_steps includes steps outside the approved plan slice.",
                public_gate_remediation(
                    context,
                    "Refresh the contract with covered_steps that map only to approved plan task/step pairs",
                ),
            );
            continue;
        }
        contract_scope.insert(step_ref);
    }

    for criterion in &contract.criteria {
        for step in &criterion.covered_steps {
            let Some(step_ref) = parse_contract_task_step_scope(step) else {
                gate.fail(
                    FailureClass::ContractMismatch,
                    "contract_criterion_covered_step_malformed",
                    format!(
                        "Execution contract criterion `{}` uses a malformed covered_steps entry.",
                        criterion.criterion_id
                    ),
                    public_gate_remediation(
                        context,
                        "Refresh criteria covered_steps using `Task <n> Step <m>` labels",
                    ),
                );
                continue;
            };
            if !approved_steps.contains(&step_ref) || !contract_scope.contains(&step_ref) {
                gate.fail(
                    FailureClass::ContractMismatch,
                    "contract_criterion_covered_step_out_of_scope",
                    format!(
                        "Execution contract criterion `{}` covered_steps is outside the approved plan slice.",
                        criterion.criterion_id
                    ),
                    public_gate_remediation(
                        context,
                        "Refresh criteria covered_steps so every criterion scope maps to approved contract scope in the active plan slice",
                    ),
                );
            }
        }
    }

    for requirement in &contract.evidence_requirements {
        for step in &requirement.covered_steps {
            let Some(step_ref) = parse_contract_task_step_scope(step) else {
                gate.fail(
                    FailureClass::ContractMismatch,
                    "contract_evidence_covered_step_malformed",
                    format!(
                        "Execution contract evidence requirement `{}` uses a malformed covered_steps entry.",
                        requirement.evidence_requirement_id
                    ),
                    public_gate_remediation(
                        context,
                        "Update evidence requirement covered_steps using `Task <n> Step <m>` labels",
                    ),
                );
                continue;
            };
            if !approved_steps.contains(&step_ref) || !contract_scope.contains(&step_ref) {
                gate.fail(
                    FailureClass::ContractMismatch,
                    "contract_evidence_covered_step_out_of_scope",
                    format!(
                        "Execution contract evidence requirement `{}` covered_steps is outside the approved plan slice.",
                        requirement.evidence_requirement_id
                    ),
                    public_gate_remediation(
                        context,
                        "Update evidence requirement covered_steps so every requirement scope maps to approved contract scope in the active plan slice",
                    ),
                );
            }
        }
    }

    if contract_scope.is_empty() {
        return;
    }

    let expected_packet_fingerprints: BTreeSet<String> = contract_scope
        .iter()
        .filter_map(|(task, step)| {
            task_packet_fingerprint(context, expected_spec_fingerprint, *task, *step)
        })
        .collect();
    let declared_packet_fingerprints: BTreeSet<String> = contract
        .source_task_packet_fingerprints
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    if declared_packet_fingerprints != expected_packet_fingerprints {
        gate.fail(
            FailureClass::StaleProvenance,
            "contract_task_packet_scope_mismatch",
            "Execution contract source task packet fingerprints do not match the approved plan slice coverage scope.",
            public_gate_remediation(
                context,
                "Refresh the contract so source task packet fingerprints resolve from the approved plan/slice task coverage",
            ),
        );
    }
}

pub(crate) fn validate_report_provenance(
    context: &ExecutionContext,
    report: &EvaluationReport,
    gate: &mut GateState,
) {
    if report.source_plan_path != context.plan_rel
        || report.source_plan_revision != context.plan_document.plan_revision
    {
        gate.fail(
            FailureClass::StaleProvenance,
            "evaluation_plan_provenance_mismatch",
            "Evaluation report does not match the current approved plan path/revision.",
            public_gate_remediation(
                context,
                "Refresh the report for the current approved plan revision",
            ),
        );
    }
    let expected_plan_fingerprint = hash_contract_plan(&context.plan_source);
    if report.source_plan_fingerprint != expected_plan_fingerprint {
        gate.fail(
            FailureClass::StaleProvenance,
            "evaluation_plan_fingerprint_mismatch",
            "Evaluation report plan fingerprint no longer matches the approved plan source.",
            public_gate_remediation(
                context,
                "Refresh the report for the current approved plan source",
            ),
        );
    }
}

pub(crate) fn validate_handoff_provenance(
    context: &ExecutionContext,
    handoff: &ExecutionHandoff,
    gate: &mut GateState,
) {
    if handoff.source_plan_path != context.plan_rel
        || handoff.source_plan_revision != context.plan_document.plan_revision
    {
        gate.fail(
            FailureClass::StaleProvenance,
            "handoff_plan_provenance_mismatch",
            "Execution handoff does not match the current approved plan path/revision.",
            public_gate_remediation(
                context,
                "Refresh the handoff for the current approved plan revision",
            ),
        );
    }
}

pub(crate) fn validate_harness_provenance(
    generated_by: &str,
    failure_class: FailureClass,
    reason_code: &str,
    plan_rel: &str,
    gate: &mut GateState,
) {
    let generated_by = generated_by.trim();
    if !HARNESS_OWNED_PRODUCERS.contains(&generated_by) {
        gate.fail(
            failure_class,
            reason_code,
            "Artifact was not generated by a harness-owned featureforge producer.",
            public_gate_remediation_for_plan(
                plan_rel,
                "Refresh the artifact through a harness-owned public workflow",
            ),
        );
    }
}

fn validate_evaluator_authority_state(
    context: &ExecutionContext,
    report: &EvaluationReport,
    gate: &mut GateState,
) {
    let Some(state) = load_gate_authority_state(context, gate) else {
        return;
    };
    let Some(active_contract) = require_active_contract_state(context, &state, gate) else {
        return;
    };

    if !matches!(
        state.harness_phase.as_deref(),
        Some(crate::execution::phase::PHASE_EXECUTING | "evaluating" | "repairing")
    ) {
        gate.fail(
            FailureClass::IllegalHarnessPhase,
            "evaluation_illegal_phase",
            "Evaluation artifacts are only legal while the harness phase is executing, evaluating, or repairing.",
            public_gate_remediation(
                context,
                "Return to the public route that advances the harness into an evaluation-ready phase",
            ),
        );
    }

    if report.source_contract_fingerprint != active_contract.fingerprint {
        gate.fail(
            FailureClass::DependencyIndexMismatch,
            "evaluation_contract_fingerprint_mismatch",
            "Evaluation report depends on a contract fingerprint that is not the active authoritative contract.",
            public_gate_remediation(
                context,
                "Refresh the evaluation report from the currently active authoritative contract",
            ),
        );
    }

    if !state
        .required_evaluator_kinds
        .iter()
        .any(|kind| kind == &report.evaluator_kind)
    {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluator_kind_not_required",
            "Evaluation report evaluator_kind is not required by the active authoritative harness contract state.",
            public_gate_remediation(
                context,
                "Refresh the evaluation report with an evaluator_kind listed in required_evaluator_kinds for the active contract",
            ),
        );
    }

    validate_authoritative_ordering(
        report.authoritative_sequence,
        state.latest_authoritative_sequence(),
        gate,
        &context.plan_rel,
        "evaluation",
    );
    validate_evaluator_semantics(context, report, &active_contract.contract, gate);
}

fn validate_contract_authority_state(
    context: &ExecutionContext,
    contract: &ExecutionContract,
    gate: &mut GateState,
) {
    let Some(state) = load_gate_authority_state(context, gate) else {
        return;
    };

    if !matches!(
        state.harness_phase.as_deref(),
        Some("contract_pending_approval")
    ) {
        gate.fail(
            FailureClass::IllegalHarnessPhase,
            "contract_illegal_phase",
            "Execution contract approval and recording are only legal while the authoritative harness phase is contract_pending_approval.",
            public_gate_remediation(
                context,
                "Return to the public route that advances the harness to contract_pending_approval after execution_preflight acceptance",
            ),
        );
    }

    validate_authoritative_ordering(
        contract.authoritative_sequence,
        state.latest_authoritative_sequence(),
        gate,
        &context.plan_rel,
        "contract",
    );
}

fn validate_handoff_authority_state(
    context: &ExecutionContext,
    handoff: &ExecutionHandoff,
    gate: &mut GateState,
) {
    let Some(state) = load_gate_authority_state(context, gate) else {
        return;
    };
    let Some(active_contract) = require_active_contract_state(context, &state, gate) else {
        return;
    };

    if !matches!(
        state.harness_phase.as_deref(),
        Some(crate::execution::phase::PHASE_HANDOFF_REQUIRED)
    ) {
        gate.fail(
            FailureClass::IllegalHarnessPhase,
            "handoff_illegal_phase",
            "Execution handoff artifacts are only legal while the authoritative harness phase is handoff_required.",
            public_gate_remediation(
                context,
                "Return to the public route that advances the harness to handoff_required",
            ),
        );
    }

    if !state.handoff_required {
        gate.fail(
            FailureClass::MissingRequiredHandoff,
            "handoff_not_required",
            "Authoritative harness state does not currently require a handoff artifact.",
            public_gate_remediation(
                context,
                "Return to the public route that determines whether handoff is required in authoritative state",
            ),
        );
    }

    if handoff.source_contract_fingerprint != active_contract.fingerprint {
        gate.fail(
            FailureClass::DependencyIndexMismatch,
            "handoff_contract_fingerprint_mismatch",
            "Execution handoff depends on a contract fingerprint that is not the active authoritative contract.",
            public_gate_remediation(
                context,
                "Refresh the handoff artifact from the active authoritative contract",
            ),
        );
    }

    validate_authoritative_ordering(
        handoff.authoritative_sequence,
        state.latest_authoritative_sequence(),
        gate,
        &context.plan_rel,
        "handoff",
    );
    validate_handoff_semantics(
        context,
        handoff,
        &active_contract.contract,
        &state.open_failed_criteria,
        gate,
    );
}

fn load_gate_authority_state(
    context: &ExecutionContext,
    gate: &mut GateState,
) -> Option<GateAuthorityState> {
    let state_path = harness_state_path(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
    );
    let payload = match load_reduced_authoritative_state(&context.runtime) {
        Ok(Some(payload)) => payload,
        Ok(None) => {
            gate.fail(
                FailureClass::NonAuthoritativeArtifact,
                "active_contract_missing",
                format!(
                    "No authoritative harness state was found at {}.",
                    state_path.display()
                ),
                public_gate_remediation(
                    context,
                    "Return to the public route that publishes authoritative harness state before artifact gates",
                ),
            );
            return None;
        }
        Err(error) => {
            gate.fail(
                FailureClass::NonAuthoritativeArtifact,
                "active_contract_missing",
                format!(
                    "Could not load authoritative reduced state {}: {}",
                    state_path.display(),
                    error.message
                ),
                public_gate_remediation(
                    context,
                    "Restore authoritative harness state readability through the owning public workflow",
                ),
            );
            return None;
        }
    };

    match serde_json::from_value::<GateAuthorityState>(payload) {
        Ok(state) => Some(state),
        Err(error) => {
            gate.fail(
                FailureClass::NonAuthoritativeArtifact,
                "active_contract_missing",
                format!(
                    "Authoritative reduced state is malformed in {}: {error}",
                    state_path.display()
                ),
                public_gate_remediation(
                    context,
                    "Repair the authoritative harness state and republish valid authoritative execution state before gating artifacts",
                ),
            );
            None
        }
    }
}

pub(crate) fn require_active_contract_state(
    context: &ExecutionContext,
    state: &GateAuthorityState,
    gate: &mut GateState,
) -> Option<ActiveContractState> {
    let contract_path = state
        .active_contract_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let contract_fingerprint = state
        .active_contract_fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let Some(contract_path) = contract_path else {
        gate.fail(
            FailureClass::NonAuthoritativeArtifact,
            "active_contract_missing",
            "Authoritative harness state is missing active_contract_path.",
            public_gate_remediation(
                context,
                "Return to the public route that publishes authoritative state with a valid active_contract_path before evaluator or handoff gates",
            ),
        );
        return None;
    };
    let Some(contract_fingerprint) = contract_fingerprint else {
        gate.fail(
            FailureClass::NonAuthoritativeArtifact,
            "active_contract_missing",
            "Authoritative harness state is missing active_contract_fingerprint.",
            public_gate_remediation(
                context,
                "Return to the public route that publishes authoritative state with a valid active_contract_fingerprint before evaluator or handoff gates",
            ),
        );
        return None;
    };

    if contract_path.contains('/') || contract_path.contains('\\') {
        gate.fail(
            FailureClass::NonAuthoritativeArtifact,
            "active_contract_missing",
            "Authoritative harness state active_contract_path must reference an authoritative artifact file name.",
            public_gate_remediation(
                context,
                "Repair active_contract_path through the owning public workflow so it points at the authoritative artifact file name",
            ),
        );
        return None;
    }

    let expected_file_name = format!("contract-{contract_fingerprint}.md");
    if contract_path != expected_file_name {
        gate.fail(
            FailureClass::NonAuthoritativeArtifact,
            "active_contract_fingerprint_mismatch",
            "Authoritative harness state active_contract_path does not match the active contract fingerprint-derived authoritative artifact path.",
            public_gate_remediation(
                context,
                "Republish authoritative harness state so active_contract_path and active_contract_fingerprint refer to the same authoritative artifact",
            ),
        );
        return None;
    }

    let active_contract_abs = harness_authoritative_artifact_path(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
        contract_path,
    );
    if !active_contract_abs.is_file() {
        gate.fail(
            FailureClass::NonAuthoritativeArtifact,
            "active_contract_missing",
            format!(
                "Active authoritative contract path does not exist: {}",
                active_contract_abs.display()
            ),
            public_gate_remediation(
                context,
                "Restore the active authoritative contract artifact before evaluator or handoff gates",
            ),
        );
        return None;
    }

    let contract_source = match fs::read_to_string(&active_contract_abs) {
        Ok(source) => source,
        Err(error) => {
            gate.fail(
                FailureClass::NonAuthoritativeArtifact,
                "active_contract_missing",
                format!(
                    "Could not read active authoritative contract {}: {error}",
                    active_contract_abs.display()
                ),
                public_gate_remediation(
                    context,
                    "Restore the active authoritative contract artifact readability before evaluator or handoff gates",
                ),
            );
            return None;
        }
    };

    let contract = match read_execution_contract(&active_contract_abs) {
        Ok(contract) => contract,
        Err(error) => {
            gate.fail(
                FailureClass::NonAuthoritativeArtifact,
                "active_contract_missing",
                format!(
                    "Active authoritative contract {} is malformed: {error}",
                    active_contract_abs.display()
                ),
                public_gate_remediation(
                    context,
                    "Repair the active authoritative contract artifact through the owning public workflow",
                ),
            );
            return None;
        }
    };

    let canonical_contract_fingerprint = verify_declared_fingerprint(
        &contract_source,
        DeclaredFingerprintValidation {
            header_label: "Contract Fingerprint",
            declared_fingerprint: &contract.contract_fingerprint,
            unverifiable_reason_code: "active_contract_fingerprint_mismatch",
            mismatch_reason_code: "active_contract_fingerprint_mismatch",
            artifact_label: "active contract",
            plan_rel: &context.plan_rel,
        },
        gate,
    )?;

    if contract_fingerprint != canonical_contract_fingerprint {
        gate.fail(
            FailureClass::NonAuthoritativeArtifact,
            "active_contract_fingerprint_mismatch",
            "Authoritative harness state active_contract_fingerprint does not match active contract content at active_contract_path.",
            public_gate_remediation(
                context,
                "Republish authoritative harness state so active_contract_fingerprint matches the active contract artifact content",
            ),
        );
        return None;
    }

    validate_contract_provenance(context, &contract, gate);
    validate_harness_provenance(
        &contract.generated_by,
        FailureClass::NonHarnessProvenance,
        "active_contract_non_harness_provenance",
        &context.plan_rel,
        gate,
    );

    Some(ActiveContractState {
        fingerprint: canonical_contract_fingerprint,
        contract,
    })
}

fn validate_authoritative_ordering(
    candidate_sequence: u64,
    latest_authoritative_sequence: u64,
    gate: &mut GateState,
    plan_rel: &str,
    artifact_label: &str,
) {
    if candidate_sequence < latest_authoritative_sequence {
        gate.fail(
            FailureClass::AuthoritativeOrderingMismatch,
            "authoritative_sequence_stale",
            format!(
                "Candidate {artifact_label} authoritative_sequence {candidate_sequence} is older than latest authoritative sequence {latest_authoritative_sequence}."
            ),
            public_gate_remediation_for_plan(
                plan_rel,
                "Refresh the artifact with a fresh authoritative sequence before rerunning the public route",
            ),
        );
    }
}

pub(crate) fn validate_evaluator_semantics(
    context: &ExecutionContext,
    report: &EvaluationReport,
    contract: &ExecutionContract,
    gate: &mut GateState,
) {
    let contract_criteria: BTreeMap<&str, &crate::contracts::harness::ContractCriterion> = contract
        .criteria
        .iter()
        .map(|criterion| (criterion.criterion_id.as_str(), criterion))
        .collect();
    let required_criterion_ids: BTreeSet<&str> = contract
        .criteria
        .iter()
        .filter(|criterion| {
            criterion
                .verifier_types
                .iter()
                .any(|kind| kind == &report.evaluator_kind)
        })
        .map(|criterion| criterion.criterion_id.as_str())
        .collect();
    let mut reported_criterion_ids: BTreeSet<&str> = BTreeSet::new();
    let evidence_ref_ids: BTreeSet<&str> = report
        .evidence_refs
        .iter()
        .map(|evidence| evidence.evidence_ref_id.as_str())
        .collect();
    let affected_steps: BTreeSet<&str> = report.affected_steps.iter().map(String::as_str).collect();
    let authoritative_artifact_fingerprints =
        load_authoritative_artifact_fingerprints(context, gate);

    for result in &report.criterion_results {
        reported_criterion_ids.insert(result.criterion_id.as_str());
        if !matches!(result.status.as_str(), "pass" | "fail" | "blocked") {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "evaluation_criterion_status_illegal",
                "Evaluation criterion status must be pass, fail, or blocked.",
                public_gate_remediation(
                    context,
                    "Refresh criterion_results with legal criterion statuses",
                ),
            );
        }
        let Some(contract_criterion) = contract_criteria.get(result.criterion_id.as_str()) else {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "evaluation_unknown_criterion_id",
                "Evaluation report includes a criterion_id that is not declared in the active contract criteria.",
                public_gate_remediation(
                    context,
                    "Refresh the report so criterion_results reference only active contract criteria",
                ),
            );
            continue;
        };

        if !contract_criterion
            .verifier_types
            .iter()
            .any(|kind| kind == &report.evaluator_kind)
        {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "evaluation_criterion_evaluator_mismatch",
                "Evaluation report includes criterion results that are not owned by this evaluator kind in the active contract.",
                public_gate_remediation(
                    context,
                    "Refresh criterion_results so each criterion belongs to the current evaluator kind",
                ),
            );
        }

        if !result
            .requirement_ids
            .iter()
            .all(|requirement| contract_criterion.requirement_ids.contains(requirement))
        {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "evaluation_requirement_mapping_invalid",
                "Criterion result requirement_ids are not a subset of the active contract criterion requirement mappings.",
                public_gate_remediation(
                    context,
                    "Refresh criterion_results with requirement_ids mapped to the active contract criterion",
                ),
            );
        }
        if !result
            .covered_steps
            .iter()
            .all(|step| contract_criterion.covered_steps.contains(step))
        {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "evaluation_covered_step_mapping_invalid",
                "Criterion result covered_steps are not a subset of the active contract criterion covered steps.",
                public_gate_remediation(
                    context,
                    "Refresh criterion_results with covered_steps mapped to the active contract criterion",
                ),
            );
        }
        if matches!(result.status.as_str(), "fail" | "blocked")
            && (result.requirement_ids.is_empty() || result.covered_steps.is_empty())
        {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "evaluation_non_passing_scope_incomplete",
                "Failing or blocked criterion results must include requirement_ids and covered_steps.",
                public_gate_remediation(
                    context,
                    "Refresh failing criterion_results with explicit requirement and covered-step mappings",
                ),
            );
        }
        if matches!(result.status.as_str(), "fail" | "blocked")
            && result
                .covered_steps
                .iter()
                .any(|step| !affected_steps.contains(step.as_str()))
        {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "evaluation_non_passing_affected_steps_missing",
                "Failing or blocked criterion_results covered_steps must be represented in affected_steps.",
                public_gate_remediation(
                    context,
                    "Refresh affected_steps to include every covered step from fail/blocked criterion_results",
                ),
            );
        }

        for evidence_ref in &result.evidence_refs {
            if !evidence_ref_ids.contains(evidence_ref.as_str()) {
                gate.fail(
                    FailureClass::EvaluationMismatch,
                    "evaluation_criterion_evidence_ref_missing",
                    "Criterion result references an evidence ref id that is not present in evidence_refs[].",
                    public_gate_remediation(
                        context,
                        "Refresh criterion_results so all referenced evidence refs are declared in evidence_refs[]",
                    ),
                );
            }
        }
    }

    for required in required_criterion_ids {
        if !reported_criterion_ids.contains(required) {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "evaluation_missing_criterion_result",
                "Evaluation report is missing a criterion result required by the active contract and evaluator kind.",
                public_gate_remediation(
                    context,
                    "Refresh the report with criterion_results for every contract criterion assigned to this evaluator",
                ),
            );
        }
    }

    if report
        .affected_steps
        .iter()
        .any(|step| !contract.covered_steps.contains(step))
    {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_affected_step_out_of_scope",
            "Evaluation report affected_steps contains steps outside the active contract coverage scope.",
            public_gate_remediation(
                context,
                "Refresh affected_steps using only active contract covered_steps",
            ),
        );
    }

    if report.verdict == "pass"
        && report
            .criterion_results
            .iter()
            .any(|result| result.status != "pass")
    {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_pass_contains_non_passing_criteria",
            "Evaluation verdict is pass but criterion_results still include fail or blocked statuses.",
            public_gate_remediation(
                context,
                "Refresh the report so a pass verdict has only passing criterion_results",
            ),
        );
    }
    if matches!(report.verdict.as_str(), "fail" | "blocked")
        && report
            .criterion_results
            .iter()
            .all(|result| result.status == "pass")
    {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_non_pass_verdict_all_pass_criteria",
            "Evaluation verdict is fail or blocked but every criterion_result status is pass.",
            public_gate_remediation(
                context,
                "Refresh the report so fail/blocked verdicts include at least one non-passing criterion_result",
            ),
        );
    }

    let contract_evidence_requirements: BTreeMap<
        &str,
        &crate::contracts::harness::EvidenceRequirement,
    > = contract
        .evidence_requirements
        .iter()
        .map(|requirement| (requirement.evidence_requirement_id.as_str(), requirement))
        .collect();

    for reference in &report.evidence_refs {
        validate_evidence_ref(
            context,
            reference,
            authoritative_artifact_fingerprints.as_ref(),
            gate,
        );
        for requirement_id in &reference.evidence_requirement_ids {
            if !contract_evidence_requirements.contains_key(requirement_id.as_str()) {
                gate.fail(
                    FailureClass::EvaluationMismatch,
                    "evaluation_unknown_evidence_requirement",
                    "Evaluation evidence ref references an unknown contract evidence requirement id.",
                    public_gate_remediation(
                        context,
                        "Update evidence_refs so evidence_requirement_ids map to active contract evidence requirements",
                    ),
                );
            }
        }
    }

    for requirement in &contract.evidence_requirements {
        let matching_refs: Vec<_> = report
            .evidence_refs
            .iter()
            .filter(|reference| {
                reference.kind.as_str() == requirement.kind.as_str()
                    && reference
                        .evidence_requirement_ids
                        .iter()
                        .any(|requirement_id| {
                            requirement_id == &requirement.evidence_requirement_id
                        })
            })
            .collect();

        let satisfies = match requirement.satisfaction_rule.as_str() {
            "all_of" => {
                let requirement_ids_satisfied =
                    requirement.requirement_ids.iter().all(|required| {
                        matching_refs.iter().any(|reference| {
                            reference
                                .requirement_ids
                                .iter()
                                .any(|value| value == required)
                        })
                    });
                let covered_steps_satisfied =
                    requirement.covered_steps.iter().all(|covered_step| {
                        matching_refs.iter().any(|reference| {
                            reference
                                .covered_steps
                                .iter()
                                .any(|value| value == covered_step)
                        })
                    });
                if requirement.requirement_ids.is_empty() && requirement.covered_steps.is_empty() {
                    !matching_refs.is_empty()
                } else {
                    requirement_ids_satisfied && covered_steps_satisfied
                }
            }
            "any_of" => {
                matching_refs.iter().any(|reference| {
                    requirement.requirement_ids.is_empty()
                        || reference
                            .requirement_ids
                            .iter()
                            .any(|id| requirement.requirement_ids.contains(id))
                }) && matching_refs.iter().any(|reference| {
                    requirement.covered_steps.is_empty()
                        || reference
                            .covered_steps
                            .iter()
                            .any(|step| requirement.covered_steps.contains(step))
                })
            }
            "per_step" => {
                if requirement.covered_steps.is_empty() {
                    !matching_refs.is_empty()
                } else {
                    requirement.covered_steps.iter().all(|covered_step| {
                        matching_refs.iter().any(|reference| {
                            reference
                                .covered_steps
                                .iter()
                                .any(|value| value == covered_step)
                        })
                    })
                }
            }
            _ => {
                gate.fail(
                    FailureClass::EvaluationMismatch,
                    "invalid_evidence_satisfaction_rule",
                    "Active contract uses an unsupported evidence satisfaction_rule.",
                    public_gate_remediation(
                        context,
                        "Repair the active contract evidence requirement satisfaction_rule to one of all_of, any_of, or per_step",
                    ),
                );
                false
            }
        };

        if !satisfies {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "missing_required_evidence",
                format!(
                    "Required evidence requirement `{}` is unsatisfied by evaluation evidence refs.",
                    requirement.evidence_requirement_id
                ),
                public_gate_remediation(
                    context,
                    "Update evidence refs so every required contract evidence requirement is satisfied",
                ),
            );
        }
    }
}

pub(crate) fn validate_handoff_semantics(
    context: &ExecutionContext,
    handoff: &ExecutionHandoff,
    contract: &ExecutionContract,
    authoritative_open_failed_criteria: &[String],
    gate: &mut GateState,
) {
    let known_criteria: BTreeSet<&str> = contract
        .criteria
        .iter()
        .map(|criterion| criterion.criterion_id.as_str())
        .collect();
    let open_criteria: BTreeSet<&str> = handoff.open_criteria.iter().map(String::as_str).collect();
    let satisfied_criteria: BTreeSet<&str> = handoff
        .satisfied_criteria
        .iter()
        .map(String::as_str)
        .collect();

    if handoff.harness_phase != crate::execution::phase::PHASE_HANDOFF_REQUIRED {
        gate.fail(
            FailureClass::MissingRequiredHandoff,
            "handoff_phase_mismatch",
            "Execution handoff must declare harness_phase handoff_required while satisfying handoff gate policy.",
            public_gate_remediation(
                context,
                "Refresh the handoff with Harness Phase set to handoff_required",
            ),
        );
    }

    for criterion in &handoff.open_criteria {
        if !known_criteria.contains(criterion.as_str()) {
            gate.fail(
                FailureClass::MissingRequiredHandoff,
                "handoff_unknown_open_criterion",
                "Execution handoff open_criteria references a criterion that is not in the active contract.",
                public_gate_remediation(
                    context,
                    "Refresh open_criteria so each criterion id exists in the active contract",
                ),
            );
        }
    }
    for criterion in &handoff.satisfied_criteria {
        if !known_criteria.contains(criterion.as_str()) {
            gate.fail(
                FailureClass::MissingRequiredHandoff,
                "handoff_unknown_satisfied_criterion",
                "Execution handoff satisfied_criteria references a criterion that is not in the active contract.",
                public_gate_remediation(
                    context,
                    "Refresh satisfied_criteria so each criterion id exists in the active contract",
                ),
            );
        }
    }

    if open_criteria
        .intersection(&satisfied_criteria)
        .next()
        .is_some()
    {
        gate.fail(
            FailureClass::MissingRequiredHandoff,
            "handoff_criteria_overlap",
            "Execution handoff may not mark the same criterion as both open and satisfied.",
            public_gate_remediation(
                context,
                "Refresh the handoff so open_criteria and satisfied_criteria are disjoint",
            ),
        );
    }

    let authoritative_open: BTreeSet<&str> = authoritative_open_failed_criteria
        .iter()
        .map(String::as_str)
        .collect();
    let reported_open: BTreeSet<&str> = handoff.open_criteria.iter().map(String::as_str).collect();
    for required_open in authoritative_open_failed_criteria {
        if !reported_open.contains(required_open.as_str()) {
            gate.fail(
                FailureClass::MissingRequiredHandoff,
                "handoff_unresolved_criteria_missing",
                "Execution handoff omitted unresolved criteria that remain open in authoritative state.",
                public_gate_remediation(
                    context,
                    "Refresh the handoff with open_criteria and open_findings for each unresolved authoritative criterion",
                ),
            );
        }
    }
    if reported_open
        .iter()
        .any(|criterion| !authoritative_open.contains(criterion))
    {
        gate.fail(
            FailureClass::MissingRequiredHandoff,
            "handoff_unresolved_criteria_superset",
            "Execution handoff open_criteria contains unresolved criteria that are not open in authoritative state.",
            public_gate_remediation(
                context,
                "Refresh the handoff so open_criteria exactly matches authoritative unresolved criteria",
            ),
        );
    }
    if !authoritative_open.is_empty() && handoff.open_findings.is_empty() {
        gate.fail(
            FailureClass::MissingRequiredHandoff,
            "handoff_unresolved_criteria_missing_findings",
            "Execution handoff must include open findings when unresolved criteria remain open.",
            public_gate_remediation(
                context,
                "Refresh the handoff with concrete open findings for unresolved criteria",
            ),
        );
    }
}

fn validate_evidence_ref(
    context: &ExecutionContext,
    reference: &crate::contracts::harness::EvaluationEvidenceRef,
    authoritative_artifact_fingerprints: Option<&AuthoritativeArtifactFingerprints>,
    gate: &mut GateState,
) {
    match reference.kind.as_str() {
        "code_location" => validate_repo_source(context, reference, gate),
        "command_output" => validate_artifact_source(
            context,
            reference,
            "command_artifact:",
            authoritative_artifact_fingerprints,
            EvidenceLocatorResolutionScope::EvidenceArtifactsWithKind("command_output"),
            gate,
        ),
        "test_result" => validate_artifact_source(
            context,
            reference,
            "test_artifact:",
            authoritative_artifact_fingerprints,
            EvidenceLocatorResolutionScope::EvidenceArtifactsWithKind("test_result"),
            gate,
        ),
        "artifact_ref" => validate_artifact_source(
            context,
            reference,
            "artifact:",
            authoritative_artifact_fingerprints,
            EvidenceLocatorResolutionScope::AnyAuthoritativeArtifact,
            gate,
        ),
        "browser_capture" => validate_artifact_source(
            context,
            reference,
            "browser_artifact:",
            authoritative_artifact_fingerprints,
            EvidenceLocatorResolutionScope::EvidenceArtifactsWithKind("browser_capture"),
            gate,
        ),
        _ => gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_evidence_kind_unsupported",
            "Evaluation evidence ref kind is unsupported.",
            public_gate_remediation(
                context,
                "Update evidence_refs using supported kinds: code_location, command_output, test_result, artifact_ref, browser_capture",
            ),
        ),
    }
}

fn validate_repo_source(
    context: &ExecutionContext,
    reference: &crate::contracts::harness::EvaluationEvidenceRef,
    gate: &mut GateState,
) {
    let Some(locator) = reference.source.strip_prefix("repo:") else {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_evidence_source_invalid",
            "Evaluation code_location evidence source must use repo:<relative_path>[#L<line>] locator grammar.",
            public_gate_remediation(context, "Update evidence_refs with a valid repo locator"),
        );
        return;
    };

    let (path_part, line_part) = match locator.split_once("#L") {
        Some((path, line)) => (path, Some(line)),
        None => (locator, None),
    };
    if normalize_repo_relative_path(path_part).is_err() {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_evidence_source_invalid",
            "Evaluation repo evidence source path is not a normalized repo-relative path.",
            public_gate_remediation(
                context,
                "Update repo evidence sources with normalized repo-relative paths",
            ),
        );
    }
    if let Some(line) = line_part
        && line
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .is_none()
    {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_evidence_source_invalid",
            "Evaluation repo evidence source line suffix must be #L<positive-integer>.",
            public_gate_remediation(
                context,
                "Update repo evidence sources with valid #L line suffixes",
            ),
        );
    }
}

fn validate_artifact_source(
    context: &ExecutionContext,
    reference: &crate::contracts::harness::EvaluationEvidenceRef,
    prefix: &str,
    authoritative_artifact_fingerprints: Option<&AuthoritativeArtifactFingerprints>,
    resolution_scope: EvidenceLocatorResolutionScope,
    gate: &mut GateState,
) {
    let Some(locator) = reference.source.strip_prefix(prefix) else {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_evidence_source_invalid",
            "Evaluation evidence source uses an invalid locator prefix for its kind.",
            public_gate_remediation(
                context,
                "Update evidence_refs with kind-compatible source locator prefixes",
            ),
        );
        return;
    };

    let fingerprint = locator.split('@').next().unwrap_or_default().trim();
    if !is_lower_hex_fingerprint(fingerprint) {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_evidence_artifact_ref_noncanonical",
            "Artifact-backed evidence sources must resolve via canonical artifact fingerprint.",
            public_gate_remediation(
                context,
                "Update evidence source locators to use canonical artifact fingerprints",
            ),
        );
        return;
    }

    let Some(authoritative_artifact_fingerprints) = authoritative_artifact_fingerprints else {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_evidence_artifact_ref_unresolved",
            "Artifact-backed evidence ref could not resolve against authoritative artifacts.",
            public_gate_remediation(
                context,
                "Publish authoritative artifacts through the owning public workflow so artifact-backed locators resolve to authoritative artifact fingerprints",
            ),
        );
        return;
    };

    let resolves = match resolution_scope {
        EvidenceLocatorResolutionScope::AnyAuthoritativeArtifact => {
            authoritative_artifact_fingerprints
                .all_fingerprints
                .contains(fingerprint)
        }
        EvidenceLocatorResolutionScope::EvidenceArtifactsWithKind(required_kind) => {
            authoritative_artifact_fingerprints
                .evidence_artifact_fingerprints_by_kind
                .get(required_kind)
                .is_some_and(|fingerprints| fingerprints.contains(fingerprint))
        }
    };
    if !resolves {
        gate.fail(
            FailureClass::EvaluationMismatch,
            "evaluation_evidence_artifact_ref_unresolved",
            "Artifact-backed evidence ref does not resolve to an authoritative artifact fingerprint.",
            public_gate_remediation(
                context,
                "Update evidence refs so each artifact-backed locator resolves to a published authoritative artifact fingerprint",
            ),
        );
    }
}

fn is_lower_hex_fingerprint(value: &str) -> bool {
    value.len() == 64 && value.as_bytes().iter().all(|byte| byte.is_ascii_hexdigit())
}

struct AuthoritativeArtifactFingerprints {
    all_fingerprints: BTreeSet<String>,
    evidence_artifact_fingerprints_by_kind: BTreeMap<String, BTreeSet<String>>,
}

enum EvidenceLocatorResolutionScope {
    AnyAuthoritativeArtifact,
    EvidenceArtifactsWithKind(&'static str),
}

#[derive(Clone, Copy)]
enum AuthoritativePublicArtifactKind {
    Contract,
    Evaluation,
    Handoff,
    EvidenceArtifact,
}

impl AuthoritativePublicArtifactKind {
    fn fingerprint_header_label(&self) -> &'static str {
        match self {
            Self::Contract => "Contract Fingerprint",
            Self::Evaluation => "Report Fingerprint",
            Self::Handoff => "Handoff Fingerprint",
            Self::EvidenceArtifact => "Evidence Artifact Fingerprint",
        }
    }
}

struct AuthoritativeArtifactIdentity {
    expected_fingerprint: String,
    artifact_kind: AuthoritativePublicArtifactKind,
}

struct DeclaredFingerprintValidation<'a> {
    header_label: &'a str,
    declared_fingerprint: &'a str,
    unverifiable_reason_code: &'a str,
    mismatch_reason_code: &'a str,
    artifact_label: &'a str,
    plan_rel: &'a str,
}

fn load_authoritative_artifact_fingerprints(
    context: &ExecutionContext,
    gate: &mut GateState,
) -> Option<AuthoritativeArtifactFingerprints> {
    let artifacts_dir = harness_authoritative_artifacts_dir(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
    );
    let entries = match fs::read_dir(&artifacts_dir) {
        Ok(entries) => entries,
        Err(error) => {
            gate.fail(
                FailureClass::EvaluationMismatch,
                "evaluation_evidence_artifact_ref_unresolved",
                format!(
                    "Could not read authoritative artifacts directory {}: {error}",
                    artifacts_dir.display()
                ),
                public_gate_remediation(
                    context,
                    "Restore authoritative artifact storage readability before validating artifact-backed evidence refs",
                ),
            );
            return None;
        }
    };
    let mut fingerprints = AuthoritativeArtifactFingerprints {
        all_fingerprints: BTreeSet::new(),
        evidence_artifact_fingerprints_by_kind: BTreeMap::new(),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                gate.fail(
                    FailureClass::EvaluationMismatch,
                    "evaluation_evidence_artifact_ref_unresolved",
                    format!(
                        "Could not enumerate authoritative artifacts in {}: {error}",
                        artifacts_dir.display()
                    ),
                    public_gate_remediation(
                        context,
                        "Restore authoritative artifact storage readability before validating artifact-backed evidence refs",
                    ),
                );
                return None;
            }
        };
        if let Some(file_name) = entry.file_name().to_str() {
            let Some(identity) = authoritative_artifact_identity_from_name(file_name) else {
                continue;
            };
            let source = match fs::read_to_string(entry.path()) {
                Ok(source) => source,
                Err(error) => {
                    gate.fail(
                        FailureClass::EvaluationMismatch,
                        "evaluation_evidence_artifact_ref_unresolved",
                        format!(
                            "Could not read authoritative artifact {}: {error}",
                            entry.path().display()
                        ),
                        public_gate_remediation(
                            context,
                            "Restore authoritative artifact storage readability before validating artifact-backed evidence refs",
                        ),
                    );
                    return None;
                }
            };
            let Some(canonical_fingerprint) =
                verify_authoritative_public_artifact(entry.path().as_path(), &source, &identity)
            else {
                continue;
            };
            fingerprints
                .all_fingerprints
                .insert(canonical_fingerprint.clone());
            if matches!(
                identity.artifact_kind,
                AuthoritativePublicArtifactKind::EvidenceArtifact
            ) {
                let Ok(evidence_artifact) = read_evidence_artifact(entry.path()) else {
                    continue;
                };
                fingerprints
                    .evidence_artifact_fingerprints_by_kind
                    .entry(evidence_artifact.evidence_kind.trim().to_owned())
                    .or_default()
                    .insert(canonical_fingerprint);
            }
        }
    }
    Some(fingerprints)
}

fn authoritative_artifact_identity_from_name(
    file_name: &str,
) -> Option<AuthoritativeArtifactIdentity> {
    let stem = file_name.strip_suffix(".md")?;
    let (prefix, fingerprint) = stem.rsplit_once('-')?;
    if !is_lower_hex_fingerprint(fingerprint) {
        return None;
    }
    let artifact_kind = match prefix {
        "contract" => AuthoritativePublicArtifactKind::Contract,
        "evaluation" => AuthoritativePublicArtifactKind::Evaluation,
        "handoff" => AuthoritativePublicArtifactKind::Handoff,
        "evidence" => AuthoritativePublicArtifactKind::EvidenceArtifact,
        _ => return None,
    };
    Some(AuthoritativeArtifactIdentity {
        expected_fingerprint: fingerprint.to_owned(),
        artifact_kind,
    })
}

fn verify_authoritative_public_artifact(
    path: &Path,
    source: &str,
    identity: &AuthoritativeArtifactIdentity,
) -> Option<String> {
    let declared_fingerprint = match identity.artifact_kind {
        AuthoritativePublicArtifactKind::Contract => {
            let artifact = read_execution_contract(path).ok()?;
            if !HARNESS_OWNED_PRODUCERS
                .iter()
                .any(|producer| *producer == artifact.generated_by.trim())
            {
                return None;
            }
            artifact.contract_fingerprint
        }
        AuthoritativePublicArtifactKind::Evaluation => {
            let artifact = read_evaluation_report(path).ok()?;
            if !HARNESS_OWNED_PRODUCERS
                .iter()
                .any(|producer| *producer == artifact.generated_by.trim())
            {
                return None;
            }
            artifact.report_fingerprint
        }
        AuthoritativePublicArtifactKind::Handoff => {
            let artifact = read_execution_handoff(path).ok()?;
            if !HARNESS_OWNED_PRODUCERS
                .iter()
                .any(|producer| *producer == artifact.generated_by.trim())
            {
                return None;
            }
            artifact.handoff_fingerprint
        }
        AuthoritativePublicArtifactKind::EvidenceArtifact => {
            let artifact = read_evidence_artifact(path).ok()?;
            if !HARNESS_OWNED_PRODUCERS
                .iter()
                .any(|producer| *producer == artifact.generated_by.trim())
            {
                return None;
            }
            artifact.evidence_artifact_fingerprint
        }
    };
    let canonical_fingerprint = canonical_fingerprint_without_header_value(
        source,
        identity.artifact_kind.fingerprint_header_label(),
    )?;
    if canonical_fingerprint != declared_fingerprint
        || canonical_fingerprint != identity.expected_fingerprint
    {
        return None;
    }
    Some(canonical_fingerprint)
}

fn verify_declared_fingerprint(
    source: &str,
    validation: DeclaredFingerprintValidation<'_>,
    gate: &mut GateState,
) -> Option<String> {
    let Some(canonical_fingerprint) =
        canonical_fingerprint_without_header_value(source, validation.header_label)
    else {
        gate.fail(
            FailureClass::ArtifactIntegrityMismatch,
            validation.unverifiable_reason_code,
            format!(
                "Could not recompute canonical {} fingerprint because `{}` is missing or malformed.",
                validation.artifact_label, validation.header_label
            ),
            public_gate_remediation_for_plan(
                validation.plan_rel,
                format!(
                    "Refresh the {} artifact with a valid `{}` header",
                    validation.artifact_label, validation.header_label
                ),
            ),
        );
        return None;
    };

    if canonical_fingerprint != validation.declared_fingerprint {
        gate.fail(
            FailureClass::ArtifactIntegrityMismatch,
            validation.mismatch_reason_code,
            format!(
                "Declared {} fingerprint does not match canonical content-derived fingerprint.",
                validation.artifact_label
            ),
            public_gate_remediation_for_plan(
                validation.plan_rel,
                format!(
                    "Refresh the {} artifact and ensure `{}` is computed from canonical content",
                    validation.artifact_label, validation.header_label
                ),
            ),
        );
        return None;
    }

    Some(canonical_fingerprint)
}

fn canonical_fingerprint_without_header_value(source: &str, header_label: &str) -> Option<String> {
    let marker = format!("**{header_label}:**");
    let mut canonical_source = String::with_capacity(source.len());
    let mut replaced = false;

    for segment in source.split_inclusive('\n') {
        let (line, newline) = match segment.strip_suffix('\n') {
            Some(line) => (line, "\n"),
            None => (segment, ""),
        };

        if !replaced && let Some(marker_index) = line.find(&marker) {
            let after_marker = &line[marker_index + marker.len()..];
            let leading_whitespace_len = after_marker
                .chars()
                .take_while(|ch| matches!(ch, ' ' | '\t'))
                .map(char::len_utf8)
                .sum::<usize>();
            canonical_source
                .push_str(&line[..marker_index + marker.len() + leading_whitespace_len]);
            canonical_source.push_str(newline);
            replaced = true;
            continue;
        }

        canonical_source.push_str(segment);
    }

    replaced.then(|| sha256_hex(canonical_source.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_gate_remediation_starts_with_public_operator_query() {
        let remediation = public_gate_remediation_for_plan(
            "docs/featureforge/plans/example.md",
            "Repair authoritative harness state.",
        );

        assert!(
            remediation.starts_with(
                "Query workflow/operator JSON for the approved plan by running `featureforge workflow operator --plan <approved-plan-path> --json` for `docs/featureforge/plans/example.md`"
            ),
            "gate remediation should make the public operator query the first visible action: {remediation}"
        );
        assert!(
            remediation.contains("follow `recommended_public_command_argv` when present"),
            "gate remediation should preserve typed public route guidance: {remediation}"
        );
        assert!(
            remediation.contains("Diagnostic context: Repair authoritative harness state."),
            "artifact-specific repair text should be diagnostic context only: {remediation}"
        );
        assert!(
            remediation.ends_with("Do not hand-edit or reconstruct proof artifacts."),
            "gate remediation should end with the manual-artifact edit prohibition: {remediation}"
        );
    }

    #[test]
    fn public_gate_remediation_omits_empty_diagnostic_context() {
        let remediation = public_gate_remediation_for_plan("plan.md", "   . ");

        assert!(
            !remediation.contains("Diagnostic context:"),
            "empty gate remediation context should not render a diagnostic sentence: {remediation}"
        );
    }
}
