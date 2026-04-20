use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::contracts::harness::{
    DowngradeBlockingEvidence, DowngradeOperatorImpact, DowngradeOperatorImpactSeverity,
    DowngradeReasonClass, EXECUTION_TOPOLOGY_DOWNGRADE_RECORD_VERSION,
    ExecutionTopologyDowngradeDetail, ExecutionTopologyDowngradeRecord,
};
use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::harness::{ChunkId, EvaluatorKind, ExecutionRunId, HarnessPhase};

/// Runtime constant.
pub const REASON_CODE_WAITING_ON_REQUIRED_EVALUATOR: &str = "waiting_on_required_evaluator";
/// Runtime constant.
pub const REASON_CODE_REQUIRED_EVALUATOR_FAILED: &str = "required_evaluator_failed";
/// Runtime constant.
pub const REASON_CODE_REQUIRED_EVALUATOR_BLOCKED: &str = "required_evaluator_blocked";
/// Runtime constant.
pub const REASON_CODE_HANDOFF_REQUIRED: &str = "handoff_required";
/// Runtime constant.
pub const REASON_CODE_REPAIR_WITHIN_BUDGET: &str = "repair_within_budget";
/// Runtime constant.
pub const REASON_CODE_PIVOT_THRESHOLD_EXCEEDED: &str = "pivot_threshold_exceeded";
/// Runtime constant.
pub const REASON_CODE_BLOCKED_ON_PLAN_REVISION: &str = "blocked_on_plan_revision";
/// Runtime constant.
pub const REASON_CODE_WRITE_AUTHORITY_CONFLICT: &str = "write_authority_conflict";
/// Runtime constant.
pub const REASON_CODE_REPO_STATE_DRIFT: &str = "repo_state_drift";
/// Runtime constant.
pub const REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED: &str = "post_review_repo_write_detected";
/// Runtime constant.
pub const REASON_CODE_STALE_PROVENANCE: &str = "stale_provenance";
/// Runtime constant.
pub const REASON_CODE_RECOVERING_INCOMPLETE_AUTHORITATIVE_MUTATION: &str =
    "recovering_incomplete_authoritative_mutation";
/// Runtime constant.
pub const REASON_CODE_MISSING_REQUIRED_EVIDENCE: &str = "missing_required_evidence";
/// Runtime constant.
pub const REASON_CODE_INVALID_EVIDENCE_SATISFACTION_RULE: &str =
    "invalid_evidence_satisfaction_rule";

/// Runtime constant.
pub const STABLE_REASON_CODES: [&str; 14] = [
    REASON_CODE_WAITING_ON_REQUIRED_EVALUATOR,
    REASON_CODE_REQUIRED_EVALUATOR_FAILED,
    REASON_CODE_REQUIRED_EVALUATOR_BLOCKED,
    REASON_CODE_HANDOFF_REQUIRED,
    REASON_CODE_REPAIR_WITHIN_BUDGET,
    REASON_CODE_PIVOT_THRESHOLD_EXCEEDED,
    REASON_CODE_BLOCKED_ON_PLAN_REVISION,
    REASON_CODE_WRITE_AUTHORITY_CONFLICT,
    REASON_CODE_REPO_STATE_DRIFT,
    REASON_CODE_POST_REVIEW_REPO_WRITE_DETECTED,
    REASON_CODE_STALE_PROVENANCE,
    REASON_CODE_RECOVERING_INCOMPLETE_AUTHORITATIVE_MUTATION,
    REASON_CODE_MISSING_REQUIRED_EVIDENCE,
    REASON_CODE_INVALID_EVIDENCE_SATISFACTION_RULE,
];

/// Runtime constant.
pub const EVENT_KIND_PHASE_TRANSITION: &str = "phase_transition";
/// Runtime constant.
pub const EVENT_KIND_GATE_RESULT: &str = "gate_result";
/// Runtime constant.
pub const EVENT_KIND_BLOCKED_STATE_ENTERED: &str = "blocked_state_entered";
/// Runtime constant.
pub const EVENT_KIND_BLOCKED_STATE_CLEARED: &str = "blocked_state_cleared";
/// Runtime constant.
pub const EVENT_KIND_WRITE_AUTHORITY_CONFLICT: &str = "write_authority_conflict";
/// Runtime constant.
pub const EVENT_KIND_WRITE_AUTHORITY_RECLAIMED: &str = "write_authority_reclaimed";
/// Runtime constant.
pub const EVENT_KIND_REPLAY_ACCEPTED: &str = "replay_accepted";
/// Runtime constant.
pub const EVENT_KIND_REPLAY_CONFLICT: &str = "replay_conflict";
/// Runtime constant.
pub const EVENT_KIND_REPO_STATE_DRIFT_DETECTED: &str = "repo_state_drift_detected";
/// Runtime constant.
pub const EVENT_KIND_REPO_STATE_RECONCILED: &str = "repo_state_reconciled";
/// Runtime constant.
pub const EVENT_KIND_INTEGRITY_MISMATCH_DETECTED: &str = "integrity_mismatch_detected";
/// Runtime constant.
pub const EVENT_KIND_PARTIAL_MUTATION_RECOVERED: &str = "partial_mutation_recovered";
/// Runtime constant.
pub const EVENT_KIND_DOWNSTREAM_GATE_REJECTED: &str = "downstream_gate_rejected";
/// Runtime constant.
pub const EVENT_KIND_RECOMMENDATION_PROPOSED: &str = "recommendation_proposed";
/// Runtime constant.
pub const EVENT_KIND_POLICY_ACCEPTED: &str = "policy_accepted";
/// Runtime constant.
pub const EVENT_KIND_AUTHORITATIVE_MUTATION_RECORDED: &str = "authoritative_mutation_recorded";
/// Runtime constant.
pub const EVENT_KIND_ORDERING_GAP_DETECTED: &str = "ordering_gap_detected";

/// Runtime constant.
pub const STABLE_EVENT_KINDS: [&str; 17] = [
    EVENT_KIND_PHASE_TRANSITION,
    EVENT_KIND_GATE_RESULT,
    EVENT_KIND_BLOCKED_STATE_ENTERED,
    EVENT_KIND_BLOCKED_STATE_CLEARED,
    EVENT_KIND_WRITE_AUTHORITY_CONFLICT,
    EVENT_KIND_WRITE_AUTHORITY_RECLAIMED,
    EVENT_KIND_REPLAY_ACCEPTED,
    EVENT_KIND_REPLAY_CONFLICT,
    EVENT_KIND_REPO_STATE_DRIFT_DETECTED,
    EVENT_KIND_REPO_STATE_RECONCILED,
    EVENT_KIND_INTEGRITY_MISMATCH_DETECTED,
    EVENT_KIND_PARTIAL_MUTATION_RECOVERED,
    EVENT_KIND_DOWNSTREAM_GATE_REJECTED,
    EVENT_KIND_RECOMMENDATION_PROPOSED,
    EVENT_KIND_POLICY_ACCEPTED,
    EVENT_KIND_AUTHORITATIVE_MUTATION_RECORDED,
    EVENT_KIND_ORDERING_GAP_DETECTED,
];

/// Runtime constant.
pub const STABLE_DOWNGRADE_REASON_CLASSES: [DowngradeReasonClass; 6] = DowngradeReasonClass::ALL;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Runtime enum.
pub enum HarnessEventKind {
    /// Runtime enum variant.
    PhaseTransition,
    /// Runtime enum variant.
    GateResult,
    /// Runtime enum variant.
    BlockedStateEntered,
    /// Runtime enum variant.
    BlockedStateCleared,
    /// Runtime enum variant.
    WriteAuthorityConflict,
    /// Runtime enum variant.
    WriteAuthorityReclaimed,
    /// Runtime enum variant.
    ReplayAccepted,
    /// Runtime enum variant.
    ReplayConflict,
    /// Runtime enum variant.
    RepoStateDriftDetected,
    /// Runtime enum variant.
    RepoStateReconciled,
    /// Runtime enum variant.
    IntegrityMismatchDetected,
    /// Runtime enum variant.
    PartialMutationRecovered,
    /// Runtime enum variant.
    DownstreamGateRejected,
    /// Runtime enum variant.
    RecommendationProposed,
    /// Runtime enum variant.
    PolicyAccepted,
    /// Runtime enum variant.
    AuthoritativeMutationRecorded,
    /// Runtime enum variant.
    OrderingGapDetected,
}

impl HarnessEventKind {
    #[must_use]
    /// Runtime constant.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PhaseTransition => EVENT_KIND_PHASE_TRANSITION,
            Self::GateResult => EVENT_KIND_GATE_RESULT,
            Self::BlockedStateEntered => EVENT_KIND_BLOCKED_STATE_ENTERED,
            Self::BlockedStateCleared => EVENT_KIND_BLOCKED_STATE_CLEARED,
            Self::WriteAuthorityConflict => EVENT_KIND_WRITE_AUTHORITY_CONFLICT,
            Self::WriteAuthorityReclaimed => EVENT_KIND_WRITE_AUTHORITY_RECLAIMED,
            Self::ReplayAccepted => EVENT_KIND_REPLAY_ACCEPTED,
            Self::ReplayConflict => EVENT_KIND_REPLAY_CONFLICT,
            Self::RepoStateDriftDetected => EVENT_KIND_REPO_STATE_DRIFT_DETECTED,
            Self::RepoStateReconciled => EVENT_KIND_REPO_STATE_RECONCILED,
            Self::IntegrityMismatchDetected => EVENT_KIND_INTEGRITY_MISMATCH_DETECTED,
            Self::PartialMutationRecovered => EVENT_KIND_PARTIAL_MUTATION_RECOVERED,
            Self::DownstreamGateRejected => EVENT_KIND_DOWNSTREAM_GATE_REJECTED,
            Self::RecommendationProposed => EVENT_KIND_RECOMMENDATION_PROPOSED,
            Self::PolicyAccepted => EVENT_KIND_POLICY_ACCEPTED,
            Self::AuthoritativeMutationRecorded => EVENT_KIND_AUTHORITATIVE_MUTATION_RECORDED,
            Self::OrderingGapDetected => EVENT_KIND_ORDERING_GAP_DETECTED,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
/// Runtime struct.
pub struct HarnessObservabilityEvent {
    /// Runtime field.
    pub event_kind: HarnessEventKind,
    /// Runtime field.
    pub timestamp: String,
    /// Runtime field.
    pub execution_run_id: Option<ExecutionRunId>,
    /// Runtime field.
    pub authoritative_sequence: Option<u64>,
    /// Runtime field.
    pub source_plan_path: Option<String>,
    /// Runtime field.
    pub source_plan_revision: Option<u32>,
    /// Runtime field.
    pub harness_phase: Option<HarnessPhase>,
    /// Runtime field.
    pub chunk_id: Option<ChunkId>,
    /// Runtime field.
    pub evaluator_kind: Option<EvaluatorKind>,
    /// Runtime field.
    pub active_contract_fingerprint: Option<String>,
    /// Runtime field.
    pub evaluation_report_fingerprint: Option<String>,
    /// Runtime field.
    pub handoff_fingerprint: Option<String>,
    /// Runtime field.
    pub command_name: Option<String>,
    /// Runtime field.
    pub gate_name: Option<String>,
    /// Runtime field.
    pub failure_class: Option<String>,
    /// Runtime field.
    pub reason_codes: Vec<String>,
}

impl HarnessObservabilityEvent {
    /// Runtime function.
    pub fn new(event_kind: HarnessEventKind, timestamp: impl Into<String>) -> Self {
        Self {
            event_kind,
            timestamp: timestamp.into(),
            execution_run_id: None,
            authoritative_sequence: None,
            source_plan_path: None,
            source_plan_revision: None,
            harness_phase: None,
            chunk_id: None,
            evaluator_kind: None,
            active_contract_fingerprint: None,
            evaluation_report_fingerprint: None,
            handoff_fingerprint: None,
            command_name: None,
            gate_name: None,
            failure_class: None,
            reason_codes: Vec::new(),
        }
    }

    /// Runtime function.
    pub fn add_reason_code(&mut self, code: impl Into<String>) {
        let code = code.into();
        if !self.reason_codes.iter().any(|existing| existing == &code) {
            self.reason_codes.push(code);
        }
    }
}

#[must_use]
/// Runtime function.
pub fn is_stable_reason_code(code: &str) -> bool {
    STABLE_REASON_CODES.contains(&code)
}

#[must_use]
/// Runtime function.
pub fn is_stable_event_kind(kind: &str) -> bool {
    STABLE_EVENT_KINDS.contains(&kind)
}

#[must_use]
/// Runtime constant.
pub const fn downgrade_reason_classes() -> &'static [DowngradeReasonClass] {
    &STABLE_DOWNGRADE_REASON_CLASSES
}

#[must_use]
/// Runtime function.
pub fn downgrade_records_share_rerun_guidance(
    left: &ExecutionTopologyDowngradeRecord,
    right: &ExecutionTopologyDowngradeRecord,
) -> bool {
    downgrade_record_is_active_guidance(left)
        && downgrade_record_is_active_guidance(right)
        && left.primary_reason_class == right.primary_reason_class
}

#[must_use]
/// Runtime constant.
pub const fn downgrade_rerun_guidance_key(
    record: &ExecutionTopologyDowngradeRecord,
) -> DowngradeReasonClass {
    record.primary_reason_class
}

#[must_use]
/// Runtime constant.
pub const fn downgrade_record_is_active_guidance(
    record: &ExecutionTopologyDowngradeRecord,
) -> bool {
    !record.rerun_guidance_superseded
}

#[must_use]
/// Runtime constant.
pub const fn downgrade_record_is_superseded_guidance(
    record: &ExecutionTopologyDowngradeRecord,
) -> bool {
    record.rerun_guidance_superseded
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn validate_execution_topology_downgrade_record(
    record: &ExecutionTopologyDowngradeRecord,
) -> Result<(), JsonFailure> {
    if record.record_version != EXECUTION_TOPOLOGY_DOWNGRADE_RECORD_VERSION {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "ExecutionTopologyDowngradeRecord has unsupported record_version {}.",
                record.record_version
            ),
        ));
    }

    require_non_empty(&record.source_plan_path, "source_plan_path")?;
    require_non_empty(&record.execution_context_key, "execution_context_key")?;
    require_non_empty(&record.generated_by, "generated_by")?;
    require_non_empty(&record.generated_at, "generated_at")?;
    require_non_empty(&record.record_fingerprint, "record_fingerprint")?;

    validate_execution_topology_downgrade_detail(&record.detail)?;

    Ok(())
}

/// # Errors
/// Returns an error when validation, parsing, IO, or runtime state checks fail.
pub fn validate_execution_topology_downgrade_detail(
    detail: &ExecutionTopologyDowngradeDetail,
) -> Result<(), JsonFailure> {
    require_non_empty(&detail.trigger_summary, "trigger_summary")?;
    if detail.affected_units.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "ExecutionTopologyDowngradeDetail must include at least one affected_units entry.",
        ));
    }
    for (index, unit) in detail.affected_units.iter().enumerate() {
        require_non_empty(unit, &format!("affected_units[{index}]"))?;
    }

    validate_blocking_evidence(&detail.blocking_evidence)?;
    validate_operator_impact(&detail.operator_impact)?;

    for (index, note) in detail.notes.iter().enumerate() {
        require_non_empty(note, &format!("notes[{index}]"))?;
    }

    Ok(())
}

fn validate_blocking_evidence(evidence: &DowngradeBlockingEvidence) -> Result<(), JsonFailure> {
    require_non_empty(&evidence.summary, "blocking_evidence.summary")?;
    for (index, reference) in evidence.references.iter().enumerate() {
        validate_blocking_evidence_reference(reference.as_str(), index)?;
    }
    Ok(())
}

fn validate_blocking_evidence_reference(reference: &str, index: usize) -> Result<(), JsonFailure> {
    let trimmed = reference.trim();
    if trimmed.is_empty()
        || trimmed != reference
        || trimmed.chars().any(char::is_whitespace)
        || trimmed.split_once(':').is_none()
    {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "ExecutionTopologyDowngradeRecord has malformed blocking_evidence.references[{index}] locator."
            ),
        ));
    }

    let Some((scheme, payload)) = trimmed.split_once(':') else {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "ExecutionTopologyDowngradeRecord has malformed blocking_evidence.references[{index}] locator."
            ),
        ));
    };
    if scheme.trim().is_empty() || payload.trim().is_empty() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "ExecutionTopologyDowngradeRecord has malformed blocking_evidence.references[{index}] locator."
            ),
        ));
    }

    Ok(())
}

fn validate_operator_impact(impact: &DowngradeOperatorImpact) -> Result<(), JsonFailure> {
    if !matches!(
        impact.severity,
        DowngradeOperatorImpactSeverity::Info
            | DowngradeOperatorImpactSeverity::Warning
            | DowngradeOperatorImpactSeverity::Blocking
    ) {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            "ExecutionTopologyDowngradeRecord has an unsupported operator_impact.severity value.",
        ));
    }
    require_non_empty(
        &impact.changed_or_blocked_stage,
        "operator_impact.changed_or_blocked_stage",
    )?;
    require_non_empty(
        &impact.expected_response,
        "operator_impact.expected_response",
    )?;
    Ok(())
}

fn require_non_empty(value: &str, field_name: &str) -> Result<(), JsonFailure> {
    if value.trim().is_empty() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!("ExecutionTopologyDowngradeRecord is missing non-empty {field_name}."),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
/// Runtime struct.
pub struct HarnessTelemetryCounters {
    /// Runtime field.
    pub phase_transition_count: u64,
    /// Runtime field.
    pub blocked_state_entries_by_reason: BTreeMap<String, u64>,
    /// Runtime field.
    pub gate_failures_by_gate: BTreeMap<String, u64>,
    /// Runtime field.
    pub retry_count: u64,
    /// Runtime field.
    pub pivot_count: u64,
    /// Runtime field.
    pub authoritative_mutation_count: u64,
    /// Runtime field.
    pub evaluator_outcomes: BTreeMap<String, u64>,
    /// Runtime field.
    pub ordering_gap_count: u64,
    /// Runtime field.
    pub replay_accepted_count: u64,
    /// Runtime field.
    pub replay_conflict_count: u64,
    /// Runtime field.
    pub write_authority_conflict_count: u64,
    /// Runtime field.
    pub write_authority_reclaim_count: u64,
    /// Runtime field.
    pub repo_state_drift_count: u64,
    /// Runtime field.
    pub integrity_mismatch_count: u64,
    /// Runtime field.
    pub partial_mutation_recovery_count: u64,
    /// Runtime field.
    pub downstream_gate_rejection_count: u64,
}

impl HarnessTelemetryCounters {
    /// Runtime constant.
    pub const fn record_phase_transition(&mut self) {
        self.phase_transition_count += 1;
    }

    /// Runtime function.
    pub fn record_blocked_state_entry(&mut self, reason_code: impl Into<String>) {
        increment_map_counter(
            &mut self.blocked_state_entries_by_reason,
            reason_code.into(),
        );
    }

    /// Runtime function.
    pub fn record_gate_failure(&mut self, gate_name: impl Into<String>) {
        increment_map_counter(&mut self.gate_failures_by_gate, gate_name.into());
    }

    /// Runtime constant.
    pub const fn record_retry(&mut self) {
        self.retry_count += 1;
    }

    /// Runtime constant.
    pub const fn record_pivot(&mut self) {
        self.pivot_count += 1;
    }

    /// Runtime constant.
    pub const fn record_authoritative_mutation(&mut self) {
        self.authoritative_mutation_count += 1;
    }

    /// Runtime function.
    pub fn record_evaluator_outcome(&mut self, outcome: impl Into<String>) {
        increment_map_counter(&mut self.evaluator_outcomes, outcome.into());
    }

    /// Runtime constant.
    pub const fn record_ordering_gap(&mut self) {
        self.ordering_gap_count += 1;
    }

    /// Runtime constant.
    pub const fn record_replay_accepted(&mut self) {
        self.replay_accepted_count += 1;
    }

    /// Runtime constant.
    pub const fn record_replay_conflict(&mut self) {
        self.replay_conflict_count += 1;
    }

    /// Runtime constant.
    pub const fn record_write_authority_conflict(&mut self) {
        self.write_authority_conflict_count += 1;
    }

    /// Runtime constant.
    pub const fn record_write_authority_reclaim(&mut self) {
        self.write_authority_reclaim_count += 1;
    }

    /// Runtime constant.
    pub const fn record_repo_state_drift(&mut self) {
        self.repo_state_drift_count += 1;
    }

    /// Runtime constant.
    pub const fn record_integrity_mismatch(&mut self) {
        self.integrity_mismatch_count += 1;
    }

    /// Runtime constant.
    pub const fn record_partial_mutation_recovery(&mut self) {
        self.partial_mutation_recovery_count += 1;
    }

    /// Runtime constant.
    pub const fn record_downstream_gate_rejection(&mut self) {
        self.downstream_gate_rejection_count += 1;
    }
}

fn increment_map_counter(map: &mut BTreeMap<String, u64>, key: String) {
    let entry = map.entry(key).or_insert(0);
    *entry += 1;
}
