use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::follow_up::normalize_persisted_repair_follow_up_token;
use crate::execution::reducer::{
    EventAuthoritySnapshot, reduce_event_authority_for_migration_parity,
};
use crate::execution::router::route_runtime_state;
use crate::execution::semantic_identity::semantic_workspace_snapshot;
use crate::execution::state::{ExecutionRuntime, load_execution_context_without_authority_overlay};
use crate::execution::transitions::AuthoritativeTransitionState;
use crate::git::sha256_hex;
use crate::paths::write_atomic as write_atomic_file;
use crate::paths::{harness_state_path, normalize_identifier_token};

const EVENT_LOG_SCHEMA_VERSION: u32 = 1;
thread_local! {
    static IN_FLIGHT_MIGRATION_PAYLOADS: RefCell<BTreeMap<PathBuf, Value>> =
        const { RefCell::new(BTreeMap::new()) };
}

struct InFlightMigrationGuard {
    state_path: PathBuf,
    previous_payload: Option<Value>,
}

impl Drop for InFlightMigrationGuard {
    fn drop(&mut self) {
        IN_FLIGHT_MIGRATION_PAYLOADS.with(|payloads| {
            let mut payloads = payloads.borrow_mut();
            if let Some(previous_payload) = self.previous_payload.take() {
                payloads.insert(self.state_path.clone(), previous_payload);
            } else {
                payloads.remove(&self.state_path);
            }
        });
    }
}

fn in_flight_migration_payload(state_path: &Path) -> Option<Value> {
    IN_FLIGHT_MIGRATION_PAYLOADS.with(|payloads| payloads.borrow().get(state_path).cloned())
}

fn enter_in_flight_migration(state_path: &Path, payload: &Value) -> InFlightMigrationGuard {
    let previous_payload = IN_FLIGHT_MIGRATION_PAYLOADS.with(|payloads| {
        payloads
            .borrow_mut()
            .insert(state_path.to_path_buf(), payload.clone())
    });
    InFlightMigrationGuard {
        state_path: state_path.to_path_buf(),
        previous_payload,
    }
}

const RUN_METADATA_FIELDS: &[&str] = &[
    "schema_version",
    "harness_phase",
    "latest_authoritative_sequence",
    "authoritative_sequence",
    "source_plan_path",
    "source_plan_revision",
    "execution_plan_projection_fingerprint",
    "execution_evidence_projection_fingerprint",
    "execution_evidence_attempts",
    "run_identity",
    "execution_run_id",
    "chunk_id",
    "active_contract_path",
    "active_contract_fingerprint",
    "active_worktree_lease_fingerprints",
    "active_worktree_lease_bindings",
    "required_evaluator_kinds",
    "completed_evaluator_kinds",
    "pending_evaluator_kinds",
    "non_passing_evaluator_kinds",
    "failed_evaluator_kinds",
    "blocked_evaluator_kinds",
    "aggregate_evaluation_state",
    "last_evaluation_report_path",
    "last_evaluation_report_fingerprint",
    "last_evaluation_evaluator_kind",
    "last_evaluation_verdict",
    "current_chunk_retry_count",
    "current_chunk_retry_budget",
    "current_chunk_pivot_threshold",
    "handoff_required",
    "open_failed_criteria",
    "write_authority_state",
    "write_authority_holder",
    "write_authority_worktree",
    "repo_state_baseline_head_sha",
    "repo_state_baseline_worktree_fingerprint",
    "repo_state_drift_state",
    "reason_codes",
    "strategy_state",
    "last_strategy_checkpoint_fingerprint",
    "strategy_checkpoint_kind",
    "strategy_reset_required",
    "strategy_cycle_counts",
    "strategy_review_dispatch_credits",
    "dependency_index_state",
];
const HANDOFF_FIELDS: &[&str] = &[
    "handoff_required",
    "last_handoff_path",
    "last_handoff_fingerprint",
    "harness_phase",
];

const STEP_EVENT_FIELDS: &[&str] = &[
    "current_open_step_state",
    "execution_plan_projection_fingerprint",
    "execution_evidence_projection_fingerprint",
    "execution_evidence_attempts",
    "harness_phase",
    "handoff_required",
    "aggregate_evaluation_state",
    "review_state_repair_follow_up",
    "strategy_checkpoints",
    "strategy_state",
    "strategy_checkpoint_kind",
    "last_strategy_checkpoint_fingerprint",
    "strategy_reset_required",
    "strategy_cycle_counts",
    "strategy_review_dispatch_credits",
    "strategy_cycle_break_task",
    "strategy_cycle_break_step",
    "strategy_cycle_break_checkpoint_fingerprint",
    "last_evaluation_report_path",
    "last_evaluation_report_fingerprint",
    "last_evaluation_evaluator_kind",
    "last_evaluation_verdict",
    "last_handoff_path",
    "last_handoff_fingerprint",
    "last_pivot_path",
    "last_pivot_fingerprint",
];

const TRANSFER_EVENT_FIELDS: &[&str] = &[
    "current_transfer_scope",
    "current_transfer_to",
    "current_open_step_state",
    "harness_phase",
    "handoff_required",
    "last_handoff_path",
    "last_handoff_fingerprint",
];

const DISPATCH_EVENT_FIELDS: &[&str] = &[
    "current_task_review_dispatch_id",
    "current_final_review_dispatch_id",
    "task_closure_negative_result_records",
    "task_closure_negative_result_history",
    "strategy_review_dispatch_lineage",
    "strategy_review_dispatch_lineage_history",
    "final_review_dispatch_lineage",
    "final_review_dispatch_lineage_history",
    "harness_phase",
    "strategy_checkpoints",
    "strategy_state",
    "strategy_checkpoint_kind",
    "last_strategy_checkpoint_fingerprint",
    "strategy_reset_required",
    "strategy_cycle_counts",
    "strategy_review_dispatch_credits",
    "strategy_cycle_break_task",
    "strategy_cycle_break_step",
    "strategy_cycle_break_checkpoint_fingerprint",
];

const TASK_CLOSURE_EVENT_FIELDS: &[&str] = &[
    "current_open_step_state",
    "harness_phase",
    "current_task_closure_records",
    "task_closure_record_history",
    "task_closure_negative_result_records",
    "task_closure_negative_result_history",
    "superseded_task_closure_ids",
    "review_state_repair_follow_up",
    "current_task_review_dispatch_id",
    "strategy_review_dispatch_lineage",
    "strategy_review_dispatch_lineage_history",
    "strategy_checkpoints",
    "strategy_state",
    "strategy_checkpoint_kind",
    "last_strategy_checkpoint_fingerprint",
    "strategy_reset_required",
    "strategy_cycle_counts",
    "strategy_review_dispatch_credits",
    "strategy_cycle_break_task",
    "strategy_cycle_break_step",
    "strategy_cycle_break_checkpoint_fingerprint",
];

const BRANCH_CLOSURE_EVENT_FIELDS: &[&str] = &[
    "branch_closure_records",
    "current_branch_closure_id",
    "current_branch_closure_reviewed_state_id",
    "current_branch_closure_contract_identity",
    "superseded_branch_closure_ids",
    "release_readiness_record_history",
    "current_release_readiness_record_id",
    "current_release_readiness_result",
    "current_release_readiness_summary_hash",
    "final_review_record_history",
    "current_final_review_branch_closure_id",
    "current_final_review_dispatch_id",
    "current_final_review_reviewer_source",
    "current_final_review_reviewer_id",
    "current_final_review_result",
    "current_final_review_summary_hash",
    "current_final_review_record_id",
    "browser_qa_record_history",
    "current_qa_branch_closure_id",
    "current_qa_result",
    "current_qa_summary_hash",
    "current_qa_record_id",
    "finish_review_gate_pass_branch_closure_id",
    "harness_phase",
];

const RELEASE_READINESS_EVENT_FIELDS: &[&str] = &[
    "release_readiness_record_history",
    "current_release_readiness_record_id",
    "current_release_readiness_result",
    "current_release_readiness_summary_hash",
    "current_final_review_branch_closure_id",
    "current_final_review_dispatch_id",
    "current_final_review_reviewer_source",
    "current_final_review_reviewer_id",
    "current_final_review_result",
    "current_final_review_summary_hash",
    "current_final_review_record_id",
    "current_qa_branch_closure_id",
    "current_qa_result",
    "current_qa_summary_hash",
    "current_qa_record_id",
    "final_review_dispatch_lineage",
    "final_review_dispatch_lineage_history",
    "finish_review_gate_pass_branch_closure_id",
    "harness_phase",
];

const FINAL_REVIEW_EVENT_FIELDS: &[&str] = &[
    "final_review_record_history",
    "current_final_review_branch_closure_id",
    "current_final_review_dispatch_id",
    "current_final_review_reviewer_source",
    "current_final_review_reviewer_id",
    "current_final_review_result",
    "current_final_review_summary_hash",
    "current_final_review_record_id",
    "current_qa_branch_closure_id",
    "current_qa_result",
    "current_qa_summary_hash",
    "current_qa_record_id",
    "finish_review_gate_pass_branch_closure_id",
    "harness_phase",
];

const QA_EVENT_FIELDS: &[&str] = &[
    "browser_qa_record_history",
    "current_qa_branch_closure_id",
    "current_qa_result",
    "current_qa_summary_hash",
    "current_qa_record_id",
    "finish_review_gate_pass_branch_closure_id",
    "harness_phase",
];

const REPAIR_EVENT_FIELDS: &[&str] = &[
    "current_open_step_state",
    "current_task_closure_records",
    "task_closure_record_history",
    "task_closure_negative_result_records",
    "task_closure_negative_result_history",
    "superseded_task_closure_ids",
    "current_branch_closure_id",
    "current_branch_closure_reviewed_state_id",
    "current_branch_closure_contract_identity",
    "branch_closure_records",
    "superseded_branch_closure_ids",
    "current_release_readiness_record_id",
    "current_release_readiness_result",
    "current_release_readiness_summary_hash",
    "release_readiness_record_history",
    "current_final_review_branch_closure_id",
    "current_final_review_dispatch_id",
    "current_final_review_reviewer_source",
    "current_final_review_reviewer_id",
    "current_final_review_result",
    "current_final_review_summary_hash",
    "current_final_review_record_id",
    "final_review_record_history",
    "current_qa_branch_closure_id",
    "current_qa_result",
    "current_qa_summary_hash",
    "current_qa_record_id",
    "browser_qa_record_history",
    "review_state_repair_follow_up",
    "harness_phase",
    "current_task_review_dispatch_id",
    "current_final_review_dispatch_id",
    "strategy_review_dispatch_lineage",
    "strategy_review_dispatch_lineage_history",
    "final_review_dispatch_lineage",
    "final_review_dispatch_lineage_history",
    "strategy_checkpoints",
    "strategy_state",
    "strategy_checkpoint_kind",
    "last_strategy_checkpoint_fingerprint",
    "strategy_reset_required",
    "strategy_cycle_counts",
    "strategy_review_dispatch_credits",
    "strategy_cycle_break_task",
    "strategy_cycle_break_step",
    "strategy_cycle_break_checkpoint_fingerprint",
    "finish_review_gate_pass_branch_closure_id",
];

const STRATEGY_EVENT_FIELDS: &[&str] = &[
    "strategy_checkpoints",
    "strategy_state",
    "strategy_checkpoint_kind",
    "last_strategy_checkpoint_fingerprint",
    "strategy_reset_required",
    "strategy_cycle_counts",
    "strategy_review_dispatch_credits",
    "strategy_cycle_break_task",
    "strategy_cycle_break_step",
    "strategy_cycle_break_checkpoint_fingerprint",
    "finish_review_gate_pass_branch_closure_id",
    "harness_phase",
];
const AUTHORITATIVE_EVENT_FIELDS: &[&str] = &[
    "schema_version",
    "harness_phase",
    "latest_authoritative_sequence",
    "authoritative_sequence",
    "source_plan_path",
    "source_plan_revision",
    "execution_plan_projection_fingerprint",
    "execution_evidence_projection_fingerprint",
    "execution_evidence_attempts",
    "run_identity",
    "execution_run_id",
    "chunk_id",
    "active_contract_path",
    "active_contract_fingerprint",
    "active_worktree_lease_fingerprints",
    "active_worktree_lease_bindings",
    "required_evaluator_kinds",
    "completed_evaluator_kinds",
    "pending_evaluator_kinds",
    "non_passing_evaluator_kinds",
    "failed_evaluator_kinds",
    "blocked_evaluator_kinds",
    "aggregate_evaluation_state",
    "last_evaluation_report_path",
    "last_evaluation_report_fingerprint",
    "last_evaluation_evaluator_kind",
    "last_evaluation_verdict",
    "current_chunk_retry_count",
    "current_chunk_retry_budget",
    "current_chunk_pivot_threshold",
    "handoff_required",
    "open_failed_criteria",
    "write_authority_state",
    "write_authority_holder",
    "write_authority_worktree",
    "repo_state_baseline_head_sha",
    "repo_state_baseline_worktree_fingerprint",
    "repo_state_drift_state",
    "reason_codes",
    "dependency_index_state",
    "current_open_step_state",
    "current_transfer_scope",
    "current_transfer_to",
    "current_task_closure_records",
    "task_closure_record_history",
    "task_closure_negative_result_records",
    "task_closure_negative_result_history",
    "superseded_task_closure_ids",
    "branch_closure_records",
    "current_branch_closure_id",
    "current_branch_closure_reviewed_state_id",
    "current_branch_closure_contract_identity",
    "superseded_branch_closure_ids",
    "release_readiness_record_history",
    "current_release_readiness_record_id",
    "current_release_readiness_result",
    "current_release_readiness_summary_hash",
    "final_review_record_history",
    "current_final_review_branch_closure_id",
    "current_final_review_dispatch_id",
    "current_final_review_reviewer_source",
    "current_final_review_reviewer_id",
    "current_final_review_result",
    "current_final_review_summary_hash",
    "current_final_review_record_id",
    "browser_qa_record_history",
    "current_qa_branch_closure_id",
    "current_qa_result",
    "current_qa_summary_hash",
    "current_qa_record_id",
    "review_state_repair_follow_up",
    "current_task_review_dispatch_id",
    "strategy_review_dispatch_lineage",
    "strategy_review_dispatch_lineage_history",
    "final_review_dispatch_lineage",
    "final_review_dispatch_lineage_history",
    "strategy_checkpoints",
    "strategy_state",
    "strategy_checkpoint_kind",
    "last_strategy_checkpoint_fingerprint",
    "strategy_reset_required",
    "strategy_cycle_counts",
    "strategy_review_dispatch_credits",
    "strategy_cycle_break_task",
    "strategy_cycle_break_step",
    "strategy_cycle_break_checkpoint_fingerprint",
    "finish_review_gate_pass_branch_closure_id",
    "last_handoff_path",
    "last_handoff_fingerprint",
    "last_pivot_path",
    "last_pivot_fingerprint",
];

const RECORD_MAP_DELTA_FIELDS: &[&str] = &[
    "current_task_closure_records",
    "task_closure_record_history",
    "task_closure_negative_result_records",
    "task_closure_negative_result_history",
    "branch_closure_records",
    "release_readiness_record_history",
    "final_review_record_history",
    "browser_qa_record_history",
    "strategy_review_dispatch_lineage",
    "strategy_review_dispatch_lineage_history",
    "final_review_dispatch_lineage",
    "final_review_dispatch_lineage_history",
];

macro_rules! authoritative_fact_fields {
    ($($field_ident:ident => $field:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Default, PartialEq)]
        pub(crate) struct AuthoritativeFactBuilder {
            $(
                pub(crate) $field_ident: Option<Value>,
            )+
        }

        impl AuthoritativeFactBuilder {
            #[cfg(test)]
            fn get(&self, field: &str) -> Option<&Value> {
                match field {
                    $($field => self.$field_ident.as_ref(),)+
                    _ => None,
                }
            }

            fn set_field_value(&mut self, field: &str, value: Value) -> bool {
                match field {
                    $($field => {
                        self.$field_ident = Some(value);
                        true
                    },)+
                    _ => false,
                }
            }

            #[cfg(test)]
            fn populated_field_names(&self) -> Vec<&'static str> {
                let mut fields = Vec::new();
                $(
                    if self.$field_ident.is_some() {
                        fields.push($field);
                    }
                )+
                fields
            }

            #[cfg(test)]
            fn contains_field(&self, field: &str) -> bool {
                self.get(field).is_some()
            }
        }

        impl FromIterator<(String, Value)> for AuthoritativeFactBuilder {
            fn from_iter<T: IntoIterator<Item = (String, Value)>>(iter: T) -> Self {
                let mut facts = Self::default();
                for (field, value) in iter {
                    facts.set_field_value(&field, value);
                }
                facts
            }
        }
    };
}

fn deserialize_optional_fact_value<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

authoritative_fact_fields! {
    schema_version => "schema_version",
    harness_phase => "harness_phase",
    latest_authoritative_sequence => "latest_authoritative_sequence",
    authoritative_sequence => "authoritative_sequence",
    source_plan_path => "source_plan_path",
    source_plan_revision => "source_plan_revision",
    execution_plan_projection_fingerprint => "execution_plan_projection_fingerprint",
    execution_evidence_projection_fingerprint => "execution_evidence_projection_fingerprint",
    execution_evidence_attempts => "execution_evidence_attempts",
    run_identity => "run_identity",
    execution_run_id => "execution_run_id",
    chunk_id => "chunk_id",
    active_contract_path => "active_contract_path",
    active_contract_fingerprint => "active_contract_fingerprint",
    active_worktree_lease_fingerprints => "active_worktree_lease_fingerprints",
    active_worktree_lease_bindings => "active_worktree_lease_bindings",
    required_evaluator_kinds => "required_evaluator_kinds",
    completed_evaluator_kinds => "completed_evaluator_kinds",
    pending_evaluator_kinds => "pending_evaluator_kinds",
    non_passing_evaluator_kinds => "non_passing_evaluator_kinds",
    failed_evaluator_kinds => "failed_evaluator_kinds",
    blocked_evaluator_kinds => "blocked_evaluator_kinds",
    aggregate_evaluation_state => "aggregate_evaluation_state",
    last_evaluation_report_path => "last_evaluation_report_path",
    last_evaluation_report_fingerprint => "last_evaluation_report_fingerprint",
    last_evaluation_evaluator_kind => "last_evaluation_evaluator_kind",
    last_evaluation_verdict => "last_evaluation_verdict",
    current_chunk_retry_count => "current_chunk_retry_count",
    current_chunk_retry_budget => "current_chunk_retry_budget",
    current_chunk_pivot_threshold => "current_chunk_pivot_threshold",
    handoff_required => "handoff_required",
    open_failed_criteria => "open_failed_criteria",
    write_authority_state => "write_authority_state",
    write_authority_holder => "write_authority_holder",
    write_authority_worktree => "write_authority_worktree",
    repo_state_baseline_head_sha => "repo_state_baseline_head_sha",
    repo_state_baseline_worktree_fingerprint => "repo_state_baseline_worktree_fingerprint",
    repo_state_drift_state => "repo_state_drift_state",
    reason_codes => "reason_codes",
    strategy_state => "strategy_state",
    last_strategy_checkpoint_fingerprint => "last_strategy_checkpoint_fingerprint",
    strategy_checkpoint_kind => "strategy_checkpoint_kind",
    strategy_reset_required => "strategy_reset_required",
    strategy_cycle_counts => "strategy_cycle_counts",
    strategy_review_dispatch_credits => "strategy_review_dispatch_credits",
    dependency_index_state => "dependency_index_state",
    current_open_step_state => "current_open_step_state",
    current_transfer_scope => "current_transfer_scope",
    current_transfer_to => "current_transfer_to",
    current_task_closure_records => "current_task_closure_records",
    task_closure_record_history => "task_closure_record_history",
    task_closure_negative_result_records => "task_closure_negative_result_records",
    task_closure_negative_result_history => "task_closure_negative_result_history",
    superseded_task_closure_ids => "superseded_task_closure_ids",
    branch_closure_records => "branch_closure_records",
    current_branch_closure_id => "current_branch_closure_id",
    current_branch_closure_reviewed_state_id => "current_branch_closure_reviewed_state_id",
    current_branch_closure_contract_identity => "current_branch_closure_contract_identity",
    superseded_branch_closure_ids => "superseded_branch_closure_ids",
    release_readiness_record_history => "release_readiness_record_history",
    current_release_readiness_record_id => "current_release_readiness_record_id",
    current_release_readiness_result => "current_release_readiness_result",
    current_release_readiness_summary_hash => "current_release_readiness_summary_hash",
    final_review_record_history => "final_review_record_history",
    current_final_review_branch_closure_id => "current_final_review_branch_closure_id",
    current_final_review_dispatch_id => "current_final_review_dispatch_id",
    current_final_review_reviewer_source => "current_final_review_reviewer_source",
    current_final_review_reviewer_id => "current_final_review_reviewer_id",
    current_final_review_result => "current_final_review_result",
    current_final_review_summary_hash => "current_final_review_summary_hash",
    current_final_review_record_id => "current_final_review_record_id",
    browser_qa_record_history => "browser_qa_record_history",
    current_qa_branch_closure_id => "current_qa_branch_closure_id",
    current_qa_result => "current_qa_result",
    current_qa_summary_hash => "current_qa_summary_hash",
    current_qa_record_id => "current_qa_record_id",
    review_state_repair_follow_up => "review_state_repair_follow_up",
    current_task_review_dispatch_id => "current_task_review_dispatch_id",
    strategy_review_dispatch_lineage => "strategy_review_dispatch_lineage",
    strategy_review_dispatch_lineage_history => "strategy_review_dispatch_lineage_history",
    final_review_dispatch_lineage => "final_review_dispatch_lineage",
    final_review_dispatch_lineage_history => "final_review_dispatch_lineage_history",
    strategy_checkpoints => "strategy_checkpoints",
    strategy_cycle_break_task => "strategy_cycle_break_task",
    strategy_cycle_break_step => "strategy_cycle_break_step",
    strategy_cycle_break_checkpoint_fingerprint => "strategy_cycle_break_checkpoint_fingerprint",
    finish_review_gate_pass_branch_closure_id => "finish_review_gate_pass_branch_closure_id",
    last_handoff_path => "last_handoff_path",
    last_handoff_fingerprint => "last_handoff_fingerprint",
    last_pivot_path => "last_pivot_path",
    last_pivot_fingerprint => "last_pivot_fingerprint",
}

trait EventFactPayload {
    fn get(&self, field: &str) -> Option<&Value>;
    fn populated_field_names(&self) -> Vec<&'static str>;
}

macro_rules! event_fact_payload {
    ($name:ident { $($field_ident:ident => $field:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct $name {
            $(
                #[serde(
                    default,
                    rename = $field,
                    deserialize_with = "deserialize_optional_fact_value",
                    skip_serializing_if = "Option::is_none"
                )]
                pub(crate) $field_ident: Option<Value>,
            )+
        }

        impl From<AuthoritativeFactBuilder> for $name {
            fn from(value: AuthoritativeFactBuilder) -> Self {
                Self {
                    $($field_ident: value.$field_ident,)+
                }
            }
        }

        impl EventFactPayload for $name {
            fn get(&self, field: &str) -> Option<&Value> {
                match field {
                    $($field => self.$field_ident.as_ref(),)+
                    _ => None,
                }
            }

            fn populated_field_names(&self) -> Vec<&'static str> {
                let mut fields = Vec::new();
                $(
                    if self.$field_ident.is_some() {
                        fields.push($field);
                    }
                )+
                fields
            }
        }
    }
}

event_fact_payload! { StepEventFacts {
    current_open_step_state => "current_open_step_state",
    execution_plan_projection_fingerprint => "execution_plan_projection_fingerprint",
    execution_evidence_projection_fingerprint => "execution_evidence_projection_fingerprint",
    execution_evidence_attempts => "execution_evidence_attempts",
    harness_phase => "harness_phase",
    handoff_required => "handoff_required",
    aggregate_evaluation_state => "aggregate_evaluation_state",
    review_state_repair_follow_up => "review_state_repair_follow_up",
    strategy_checkpoints => "strategy_checkpoints",
    strategy_state => "strategy_state",
    strategy_checkpoint_kind => "strategy_checkpoint_kind",
    last_strategy_checkpoint_fingerprint => "last_strategy_checkpoint_fingerprint",
    strategy_reset_required => "strategy_reset_required",
    strategy_cycle_counts => "strategy_cycle_counts",
    strategy_review_dispatch_credits => "strategy_review_dispatch_credits",
    strategy_cycle_break_task => "strategy_cycle_break_task",
    strategy_cycle_break_step => "strategy_cycle_break_step",
    strategy_cycle_break_checkpoint_fingerprint => "strategy_cycle_break_checkpoint_fingerprint",
    last_evaluation_report_path => "last_evaluation_report_path",
    last_evaluation_report_fingerprint => "last_evaluation_report_fingerprint",
    last_evaluation_evaluator_kind => "last_evaluation_evaluator_kind",
    last_evaluation_verdict => "last_evaluation_verdict",
    last_handoff_path => "last_handoff_path",
    last_handoff_fingerprint => "last_handoff_fingerprint",
    last_pivot_path => "last_pivot_path",
    last_pivot_fingerprint => "last_pivot_fingerprint",
} }

event_fact_payload! { TransferEventFacts {
    current_transfer_scope => "current_transfer_scope",
    current_transfer_to => "current_transfer_to",
    current_open_step_state => "current_open_step_state",
    harness_phase => "harness_phase",
    handoff_required => "handoff_required",
    last_handoff_path => "last_handoff_path",
    last_handoff_fingerprint => "last_handoff_fingerprint",
} }

event_fact_payload! { DispatchEventFacts {
    current_task_review_dispatch_id => "current_task_review_dispatch_id",
    current_final_review_dispatch_id => "current_final_review_dispatch_id",
    task_closure_negative_result_records => "task_closure_negative_result_records",
    task_closure_negative_result_history => "task_closure_negative_result_history",
    strategy_review_dispatch_lineage => "strategy_review_dispatch_lineage",
    strategy_review_dispatch_lineage_history => "strategy_review_dispatch_lineage_history",
    final_review_dispatch_lineage => "final_review_dispatch_lineage",
    final_review_dispatch_lineage_history => "final_review_dispatch_lineage_history",
    harness_phase => "harness_phase",
    strategy_checkpoints => "strategy_checkpoints",
    strategy_state => "strategy_state",
    strategy_checkpoint_kind => "strategy_checkpoint_kind",
    last_strategy_checkpoint_fingerprint => "last_strategy_checkpoint_fingerprint",
    strategy_reset_required => "strategy_reset_required",
    strategy_cycle_counts => "strategy_cycle_counts",
    strategy_review_dispatch_credits => "strategy_review_dispatch_credits",
    strategy_cycle_break_task => "strategy_cycle_break_task",
    strategy_cycle_break_step => "strategy_cycle_break_step",
    strategy_cycle_break_checkpoint_fingerprint => "strategy_cycle_break_checkpoint_fingerprint",
} }

event_fact_payload! { TaskClosureEventFacts {
    current_open_step_state => "current_open_step_state",
    harness_phase => "harness_phase",
    current_task_closure_records => "current_task_closure_records",
    task_closure_record_history => "task_closure_record_history",
    task_closure_negative_result_records => "task_closure_negative_result_records",
    task_closure_negative_result_history => "task_closure_negative_result_history",
    superseded_task_closure_ids => "superseded_task_closure_ids",
    review_state_repair_follow_up => "review_state_repair_follow_up",
    current_task_review_dispatch_id => "current_task_review_dispatch_id",
    strategy_review_dispatch_lineage => "strategy_review_dispatch_lineage",
    strategy_review_dispatch_lineage_history => "strategy_review_dispatch_lineage_history",
    strategy_checkpoints => "strategy_checkpoints",
    strategy_state => "strategy_state",
    strategy_checkpoint_kind => "strategy_checkpoint_kind",
    last_strategy_checkpoint_fingerprint => "last_strategy_checkpoint_fingerprint",
    strategy_reset_required => "strategy_reset_required",
    strategy_cycle_counts => "strategy_cycle_counts",
    strategy_review_dispatch_credits => "strategy_review_dispatch_credits",
    strategy_cycle_break_task => "strategy_cycle_break_task",
    strategy_cycle_break_step => "strategy_cycle_break_step",
    strategy_cycle_break_checkpoint_fingerprint => "strategy_cycle_break_checkpoint_fingerprint",
} }

event_fact_payload! { BranchClosureEventFacts {
    branch_closure_records => "branch_closure_records",
    current_branch_closure_id => "current_branch_closure_id",
    current_branch_closure_reviewed_state_id => "current_branch_closure_reviewed_state_id",
    current_branch_closure_contract_identity => "current_branch_closure_contract_identity",
    superseded_branch_closure_ids => "superseded_branch_closure_ids",
    release_readiness_record_history => "release_readiness_record_history",
    current_release_readiness_record_id => "current_release_readiness_record_id",
    current_release_readiness_result => "current_release_readiness_result",
    current_release_readiness_summary_hash => "current_release_readiness_summary_hash",
    final_review_record_history => "final_review_record_history",
    current_final_review_branch_closure_id => "current_final_review_branch_closure_id",
    current_final_review_dispatch_id => "current_final_review_dispatch_id",
    current_final_review_reviewer_source => "current_final_review_reviewer_source",
    current_final_review_reviewer_id => "current_final_review_reviewer_id",
    current_final_review_result => "current_final_review_result",
    current_final_review_summary_hash => "current_final_review_summary_hash",
    current_final_review_record_id => "current_final_review_record_id",
    browser_qa_record_history => "browser_qa_record_history",
    current_qa_branch_closure_id => "current_qa_branch_closure_id",
    current_qa_result => "current_qa_result",
    current_qa_summary_hash => "current_qa_summary_hash",
    current_qa_record_id => "current_qa_record_id",
    finish_review_gate_pass_branch_closure_id => "finish_review_gate_pass_branch_closure_id",
    harness_phase => "harness_phase",
} }

event_fact_payload! { ReleaseReadinessEventFacts {
    release_readiness_record_history => "release_readiness_record_history",
    current_release_readiness_record_id => "current_release_readiness_record_id",
    current_release_readiness_result => "current_release_readiness_result",
    current_release_readiness_summary_hash => "current_release_readiness_summary_hash",
    current_final_review_branch_closure_id => "current_final_review_branch_closure_id",
    current_final_review_dispatch_id => "current_final_review_dispatch_id",
    current_final_review_reviewer_source => "current_final_review_reviewer_source",
    current_final_review_reviewer_id => "current_final_review_reviewer_id",
    current_final_review_result => "current_final_review_result",
    current_final_review_summary_hash => "current_final_review_summary_hash",
    current_final_review_record_id => "current_final_review_record_id",
    current_qa_branch_closure_id => "current_qa_branch_closure_id",
    current_qa_result => "current_qa_result",
    current_qa_summary_hash => "current_qa_summary_hash",
    current_qa_record_id => "current_qa_record_id",
    final_review_dispatch_lineage => "final_review_dispatch_lineage",
    final_review_dispatch_lineage_history => "final_review_dispatch_lineage_history",
    finish_review_gate_pass_branch_closure_id => "finish_review_gate_pass_branch_closure_id",
    harness_phase => "harness_phase",
} }

event_fact_payload! { FinalReviewEventFacts {
    final_review_record_history => "final_review_record_history",
    current_final_review_branch_closure_id => "current_final_review_branch_closure_id",
    current_final_review_dispatch_id => "current_final_review_dispatch_id",
    current_final_review_reviewer_source => "current_final_review_reviewer_source",
    current_final_review_reviewer_id => "current_final_review_reviewer_id",
    current_final_review_result => "current_final_review_result",
    current_final_review_summary_hash => "current_final_review_summary_hash",
    current_final_review_record_id => "current_final_review_record_id",
    current_qa_branch_closure_id => "current_qa_branch_closure_id",
    current_qa_result => "current_qa_result",
    current_qa_summary_hash => "current_qa_summary_hash",
    current_qa_record_id => "current_qa_record_id",
    finish_review_gate_pass_branch_closure_id => "finish_review_gate_pass_branch_closure_id",
    harness_phase => "harness_phase",
} }

event_fact_payload! { QaEventFacts {
    browser_qa_record_history => "browser_qa_record_history",
    current_qa_branch_closure_id => "current_qa_branch_closure_id",
    current_qa_result => "current_qa_result",
    current_qa_summary_hash => "current_qa_summary_hash",
    current_qa_record_id => "current_qa_record_id",
    finish_review_gate_pass_branch_closure_id => "finish_review_gate_pass_branch_closure_id",
    harness_phase => "harness_phase",
} }

event_fact_payload! { RepairEventFacts {
    current_open_step_state => "current_open_step_state",
    current_task_closure_records => "current_task_closure_records",
    task_closure_record_history => "task_closure_record_history",
    task_closure_negative_result_records => "task_closure_negative_result_records",
    task_closure_negative_result_history => "task_closure_negative_result_history",
    superseded_task_closure_ids => "superseded_task_closure_ids",
    current_branch_closure_id => "current_branch_closure_id",
    current_branch_closure_reviewed_state_id => "current_branch_closure_reviewed_state_id",
    current_branch_closure_contract_identity => "current_branch_closure_contract_identity",
    branch_closure_records => "branch_closure_records",
    superseded_branch_closure_ids => "superseded_branch_closure_ids",
    current_release_readiness_record_id => "current_release_readiness_record_id",
    current_release_readiness_result => "current_release_readiness_result",
    current_release_readiness_summary_hash => "current_release_readiness_summary_hash",
    release_readiness_record_history => "release_readiness_record_history",
    current_final_review_branch_closure_id => "current_final_review_branch_closure_id",
    current_final_review_dispatch_id => "current_final_review_dispatch_id",
    current_final_review_reviewer_source => "current_final_review_reviewer_source",
    current_final_review_reviewer_id => "current_final_review_reviewer_id",
    current_final_review_result => "current_final_review_result",
    current_final_review_summary_hash => "current_final_review_summary_hash",
    current_final_review_record_id => "current_final_review_record_id",
    final_review_record_history => "final_review_record_history",
    current_qa_branch_closure_id => "current_qa_branch_closure_id",
    current_qa_result => "current_qa_result",
    current_qa_summary_hash => "current_qa_summary_hash",
    current_qa_record_id => "current_qa_record_id",
    browser_qa_record_history => "browser_qa_record_history",
    review_state_repair_follow_up => "review_state_repair_follow_up",
    harness_phase => "harness_phase",
    current_task_review_dispatch_id => "current_task_review_dispatch_id",
    strategy_review_dispatch_lineage => "strategy_review_dispatch_lineage",
    strategy_review_dispatch_lineage_history => "strategy_review_dispatch_lineage_history",
    final_review_dispatch_lineage => "final_review_dispatch_lineage",
    final_review_dispatch_lineage_history => "final_review_dispatch_lineage_history",
    strategy_checkpoints => "strategy_checkpoints",
    strategy_state => "strategy_state",
    strategy_checkpoint_kind => "strategy_checkpoint_kind",
    last_strategy_checkpoint_fingerprint => "last_strategy_checkpoint_fingerprint",
    strategy_reset_required => "strategy_reset_required",
    strategy_cycle_counts => "strategy_cycle_counts",
    strategy_review_dispatch_credits => "strategy_review_dispatch_credits",
    strategy_cycle_break_task => "strategy_cycle_break_task",
    strategy_cycle_break_step => "strategy_cycle_break_step",
    strategy_cycle_break_checkpoint_fingerprint => "strategy_cycle_break_checkpoint_fingerprint",
    finish_review_gate_pass_branch_closure_id => "finish_review_gate_pass_branch_closure_id",
} }

event_fact_payload! { StrategyEventFacts {
    strategy_checkpoints => "strategy_checkpoints",
    strategy_state => "strategy_state",
    strategy_checkpoint_kind => "strategy_checkpoint_kind",
    last_strategy_checkpoint_fingerprint => "last_strategy_checkpoint_fingerprint",
    strategy_reset_required => "strategy_reset_required",
    strategy_cycle_counts => "strategy_cycle_counts",
    strategy_review_dispatch_credits => "strategy_review_dispatch_credits",
    strategy_cycle_break_task => "strategy_cycle_break_task",
    strategy_cycle_break_step => "strategy_cycle_break_step",
    strategy_cycle_break_checkpoint_fingerprint => "strategy_cycle_break_checkpoint_fingerprint",
    finish_review_gate_pass_branch_closure_id => "finish_review_gate_pass_branch_closure_id",
    harness_phase => "harness_phase",
} }

event_fact_payload! { RunMetadataEventFacts {
    schema_version => "schema_version",
    harness_phase => "harness_phase",
    latest_authoritative_sequence => "latest_authoritative_sequence",
    authoritative_sequence => "authoritative_sequence",
    source_plan_path => "source_plan_path",
    source_plan_revision => "source_plan_revision",
    execution_plan_projection_fingerprint => "execution_plan_projection_fingerprint",
    execution_evidence_projection_fingerprint => "execution_evidence_projection_fingerprint",
    execution_evidence_attempts => "execution_evidence_attempts",
    run_identity => "run_identity",
    execution_run_id => "execution_run_id",
    chunk_id => "chunk_id",
    active_contract_path => "active_contract_path",
    active_contract_fingerprint => "active_contract_fingerprint",
    active_worktree_lease_fingerprints => "active_worktree_lease_fingerprints",
    active_worktree_lease_bindings => "active_worktree_lease_bindings",
    required_evaluator_kinds => "required_evaluator_kinds",
    completed_evaluator_kinds => "completed_evaluator_kinds",
    pending_evaluator_kinds => "pending_evaluator_kinds",
    non_passing_evaluator_kinds => "non_passing_evaluator_kinds",
    failed_evaluator_kinds => "failed_evaluator_kinds",
    blocked_evaluator_kinds => "blocked_evaluator_kinds",
    aggregate_evaluation_state => "aggregate_evaluation_state",
    last_evaluation_report_path => "last_evaluation_report_path",
    last_evaluation_report_fingerprint => "last_evaluation_report_fingerprint",
    last_evaluation_evaluator_kind => "last_evaluation_evaluator_kind",
    last_evaluation_verdict => "last_evaluation_verdict",
    current_chunk_retry_count => "current_chunk_retry_count",
    current_chunk_retry_budget => "current_chunk_retry_budget",
    current_chunk_pivot_threshold => "current_chunk_pivot_threshold",
    handoff_required => "handoff_required",
    open_failed_criteria => "open_failed_criteria",
    write_authority_state => "write_authority_state",
    write_authority_holder => "write_authority_holder",
    write_authority_worktree => "write_authority_worktree",
    repo_state_baseline_head_sha => "repo_state_baseline_head_sha",
    repo_state_baseline_worktree_fingerprint => "repo_state_baseline_worktree_fingerprint",
    repo_state_drift_state => "repo_state_drift_state",
    reason_codes => "reason_codes",
    strategy_state => "strategy_state",
    last_strategy_checkpoint_fingerprint => "last_strategy_checkpoint_fingerprint",
    strategy_checkpoint_kind => "strategy_checkpoint_kind",
    strategy_reset_required => "strategy_reset_required",
    strategy_cycle_counts => "strategy_cycle_counts",
    strategy_review_dispatch_credits => "strategy_review_dispatch_credits",
    dependency_index_state => "dependency_index_state",
} }

event_fact_payload! { HandoffEventFacts {
    handoff_required => "handoff_required",
    last_handoff_path => "last_handoff_path",
    last_handoff_fingerprint => "last_handoff_fingerprint",
    harness_phase => "harness_phase",
} }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct BranchContext {
    pub(crate) repo_slug: String,
    pub(crate) branch_name: String,
    pub(crate) safe_branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ExecutionEvent {
    MigrationImported {
        legacy_state_backup_path: String,
    },
    Begin {
        task: Option<u32>,
        step: Option<u32>,
        facts: Box<StepEventFacts>,
    },
    Note {
        task: Option<u32>,
        step: Option<u32>,
        note_state: Option<String>,
        facts: Box<StepEventFacts>,
    },
    Complete {
        task: Option<u32>,
        step: Option<u32>,
        facts: Box<StepEventFacts>,
    },
    Reopen {
        task: Option<u32>,
        step: Option<u32>,
        facts: Box<StepEventFacts>,
    },
    Transfer {
        scope: Option<String>,
        assignee: Option<String>,
        facts: Box<TransferEventFacts>,
    },
    DispatchRecorded {
        scope: Option<String>,
        dispatch_id: Option<String>,
        facts: Box<DispatchEventFacts>,
    },
    TaskClosureRecorded {
        task: Option<u32>,
        closure_record_id: Option<String>,
        facts: Box<TaskClosureEventFacts>,
    },
    BranchClosureRecorded {
        branch_closure_id: Option<String>,
        facts: Box<BranchClosureEventFacts>,
    },
    ReleaseReadinessRecorded {
        release_readiness_record_id: Option<String>,
        facts: Box<ReleaseReadinessEventFacts>,
    },
    FinalReviewRecorded {
        final_review_record_id: Option<String>,
        dispatch_id: Option<String>,
        facts: Box<FinalReviewEventFacts>,
    },
    QaRecorded {
        qa_record_id: Option<String>,
        facts: Box<QaEventFacts>,
    },
    RepairFollowUpSet {
        follow_up: String,
        facts: Box<RepairEventFacts>,
    },
    RepairFollowUpCleared {
        facts: Box<RepairEventFacts>,
    },
    StrategyCheckpointRecorded {
        checkpoint_kind: Option<String>,
        checkpoint_fingerprint: Option<String>,
        facts: Box<StrategyEventFacts>,
    },
    StrategyCheckpointCleared {
        facts: Box<StrategyEventFacts>,
    },
    PreflightBootstrapRecorded {
        facts: Box<RunMetadataEventFacts>,
    },
    ContractRecorded {
        facts: Box<RunMetadataEventFacts>,
    },
    EvaluationRecorded {
        facts: Box<RunMetadataEventFacts>,
    },
    HandoffRecorded {
        facts: Box<HandoffEventFacts>,
    },
    WorktreeLeaseIndexUpdated {
        facts: Box<RunMetadataEventFacts>,
    },
    RecordStatusTransition {
        record_family: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        record_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        record_status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        record_sequence: Option<u64>,
    },
}

impl ExecutionEvent {
    fn facts(&self) -> Option<&dyn EventFactPayload> {
        match self {
            Self::MigrationImported { .. } | Self::RecordStatusTransition { .. } => None,
            Self::Begin { facts, .. }
            | Self::Note { facts, .. }
            | Self::Complete { facts, .. }
            | Self::Reopen { facts, .. } => Some(facts.as_ref()),
            Self::Transfer { facts, .. } => Some(facts.as_ref()),
            Self::DispatchRecorded { facts, .. } => Some(facts.as_ref()),
            Self::TaskClosureRecorded { facts, .. } => Some(facts.as_ref()),
            Self::BranchClosureRecorded { facts, .. } => Some(facts.as_ref()),
            Self::ReleaseReadinessRecorded { facts, .. } => Some(facts.as_ref()),
            Self::FinalReviewRecorded { facts, .. } => Some(facts.as_ref()),
            Self::QaRecorded { facts, .. } => Some(facts.as_ref()),
            Self::RepairFollowUpSet { facts, .. } | Self::RepairFollowUpCleared { facts } => {
                Some(facts.as_ref())
            }
            Self::StrategyCheckpointRecorded { facts, .. }
            | Self::StrategyCheckpointCleared { facts } => Some(facts.as_ref()),
            Self::PreflightBootstrapRecorded { facts }
            | Self::ContractRecorded { facts }
            | Self::EvaluationRecorded { facts }
            | Self::WorktreeLeaseIndexUpdated { facts } => Some(facts.as_ref()),
            Self::HandoffRecorded { facts } => Some(facts.as_ref()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ExecutionEventEnvelope {
    pub(crate) schema_version: u32,
    pub(crate) event_id: String,
    pub(crate) sequence: u64,
    pub(crate) timestamp: String,
    pub(crate) command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) plan_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) plan_revision: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) execution_run_id: Option<String>,
    pub(crate) branch_context: BranchContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) previous_event_hash: Option<String>,
    pub(crate) event_hash: String,
    pub(crate) payload: ExecutionEvent,
}

#[derive(Debug, Serialize)]
struct EventHashShape<'a> {
    schema_version: u32,
    event_id: &'a str,
    sequence: u64,
    timestamp: &'a str,
    command: &'a str,
    plan_path: Option<&'a str>,
    plan_revision: Option<u32>,
    execution_run_id: Option<&'a str>,
    branch_context: &'a BranchContext,
    previous_event_hash: Option<&'a str>,
    payload: &'a ExecutionEvent,
}

#[derive(Debug, Clone)]
struct EventEnvelopeMetadata {
    command: String,
    plan_path: Option<String>,
    plan_revision: Option<u32>,
    execution_run_id: Option<String>,
    branch_context: BranchContext,
}

struct EventLogLockGuard {
    lock_path: PathBuf,
}

impl Drop for EventLogLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}

pub(crate) fn load_reduced_authoritative_state(
    runtime: &ExecutionRuntime,
) -> Result<Option<Value>, JsonFailure> {
    let state_path =
        harness_state_path(&runtime.state_dir, &runtime.repo_slug, &runtime.branch_name);
    load_reduced_authoritative_state_for_state_path(&state_path)
}

pub(crate) fn load_reduced_authoritative_state_for_state_path(
    state_path: &Path,
) -> Result<Option<Value>, JsonFailure> {
    if let Some(payload) = in_flight_migration_payload(state_path) {
        return Ok(Some(payload));
    }
    ensure_event_log_migrated_from_legacy_state_path(state_path, None, None)?;
    let events_path = events_log_path_for_state_path(state_path);
    let events = load_event_log(&events_path)?;
    validate_event_log(&events)?;
    Ok(reduce_events_to_state(&events))
}

pub(crate) fn append_typed_state_event_for_state_path(
    state_path: &Path,
    command: &str,
    state_payload: &Value,
    reason: &str,
) -> Result<(), JsonFailure> {
    append_typed_state_event_for_state_path_with_step_hint(
        state_path,
        command,
        state_payload,
        reason,
        None,
    )
}

pub(crate) fn append_typed_state_event_for_state_path_with_step_hint(
    state_path: &Path,
    command: &str,
    state_payload: &Value,
    reason: &str,
    step_hint: Option<(u32, u32)>,
) -> Result<(), JsonFailure> {
    let events_path = events_log_path_for_state_path(state_path);
    let lock_path = events_lock_path_for_state_path(state_path);
    let _lock_guard = acquire_event_log_lock(&lock_path)?;
    let mut events = load_event_log(&events_path)?;
    validate_event_log(&events)?;

    let branch_context = branch_context_from_state_path(state_path, state_payload);
    let plan_path = json_string(state_payload, "source_plan_path");
    let plan_revision = json_u32(state_payload, "source_plan_revision");
    let execution_run_id =
        json_string_from_path(state_payload, &["run_identity", "execution_run_id"])
            .or_else(|| json_string(state_payload, "execution_run_id"));
    let current_state = reduce_events_to_state(&events).unwrap_or(Value::Null);
    let typed_event = event_from_command_authoritative_delta(
        command,
        &current_state,
        state_payload,
        reason,
        step_hint,
    )?;
    let next = next_event_envelope(
        &events,
        EventEnvelopeMetadata {
            command: command.to_owned(),
            plan_path: plan_path.clone(),
            plan_revision,
            execution_run_id: execution_run_id.clone(),
            branch_context: branch_context.clone(),
        },
        typed_event.clone(),
    )?;
    append_event_line(&events_path, &next)?;
    events.push(next);
    validate_event_log(&events)?;
    Ok(())
}

#[doc(hidden)]
pub fn sync_fixture_event_log_for_tests(
    state_path: &Path,
    state_payload: &Value,
) -> Result<(), JsonFailure> {
    let events_path = events_log_path_for_state_path(state_path);
    let lock_path = events_lock_path_for_state_path(state_path);
    let _lock_guard = acquire_event_log_lock(&lock_path)?;
    let branch_context = branch_context_from_state_path(state_path, state_payload);
    let plan_path = json_string(state_payload, "source_plan_path");
    let plan_revision = json_u32(state_payload, "source_plan_revision");
    let execution_run_id =
        json_string_from_path(state_payload, &["run_identity", "execution_run_id"])
            .or_else(|| json_string(state_payload, "execution_run_id"));
    let mut staged_events = Vec::new();
    let migration_event = next_event_envelope(
        &staged_events,
        EventEnvelopeMetadata {
            command: String::from("unit_test_fixture_sync"),
            plan_path: plan_path.clone(),
            plan_revision,
            execution_run_id: execution_run_id.clone(),
            branch_context: branch_context.clone(),
        },
        ExecutionEvent::MigrationImported {
            legacy_state_backup_path: String::from("unit_test_fixture_sync"),
        },
    )?;
    staged_events.push(migration_event);
    for replay_payload in migration_replay_events_from_legacy_state(state_payload) {
        let replay_event = next_event_envelope(
            &staged_events,
            EventEnvelopeMetadata {
                command: String::from("unit_test_fixture_sync_replay"),
                plan_path: plan_path.clone(),
                plan_revision,
                execution_run_id: execution_run_id.clone(),
                branch_context: branch_context.clone(),
            },
            replay_payload,
        )?;
        staged_events.push(replay_event);
    }
    validate_event_log(&staged_events)?;
    write_event_log_atomic(&events_path, &staged_events)
}

pub(crate) fn ensure_event_log_migrated_from_legacy_state(
    runtime: &ExecutionRuntime,
    legacy_state_path: &Path,
) -> Result<(), JsonFailure> {
    ensure_event_log_migrated_from_legacy_state_path(
        legacy_state_path,
        Some(branch_context_from_runtime(runtime)),
        Some(runtime),
    )
}

fn ensure_event_log_migrated_from_legacy_state_path(
    legacy_state_path: &Path,
    fallback_branch_context: Option<BranchContext>,
    runtime: Option<&ExecutionRuntime>,
) -> Result<(), JsonFailure> {
    if in_flight_migration_payload(legacy_state_path).is_some() {
        return Ok(());
    }
    let events_path = events_log_path_for_state_path(legacy_state_path);
    let events_exists = ensure_regular_file_if_present(&events_path, "Authoritative event log")?;
    if events_exists {
        let events_len = fs::metadata(&events_path).map_err(|error| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Could not inspect authoritative event log {}: {error}",
                    events_path.display()
                ),
            )
        })?;
        if events_len.len() > 0 {
            return Ok(());
        }
    }
    let legacy_state_exists =
        ensure_regular_file_if_present(legacy_state_path, "Authoritative harness state")?;
    if events_exists && !legacy_state_exists {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Event log {} is empty and no legacy authoritative state exists at {}.",
                events_path.display(),
                legacy_state_path.display()
            ),
        ));
    }
    if !legacy_state_exists {
        return Ok(());
    }

    let state_source = match fs::read_to_string(legacy_state_path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Could not inspect authoritative harness state {}: {error}",
                    legacy_state_path.display()
                ),
            ));
        }
    };
    let state_payload: Value = serde_json::from_str(&state_source).map_err(|error| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative harness state is malformed in {}: {error}",
                legacy_state_path.display()
            ),
        )
    })?;
    let migration_parity_source = state_payload.clone();
    if !state_payload.is_object() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative harness state must be a JSON object in {}.",
                legacy_state_path.display()
            ),
        ));
    }
    let _in_flight_migration_guard = enter_in_flight_migration(legacy_state_path, &state_payload);

    let backup_path = legacy_state_backup_path_for_state_path(legacy_state_path);
    write_atomic_file(&backup_path, &state_source).map_err(|error| {
        JsonFailure::new(
            FailureClass::PartialAuthoritativeMutation,
            format!(
                "Could not write legacy authoritative backup {}: {error}",
                backup_path.display()
            ),
        )
    })?;
    let plan_path = json_string(&state_payload, "source_plan_path")
        .or_else(|| runtime.and_then(infer_unique_approved_plan_path));
    let plan_revision = json_u32(&state_payload, "source_plan_revision");
    let execution_run_id =
        json_string_from_path(&state_payload, &["run_identity", "execution_run_id"])
            .or_else(|| json_string(&state_payload, "execution_run_id"));
    let branch_context = fallback_branch_context
        .unwrap_or_else(|| branch_context_from_state_path(legacy_state_path, &state_payload));

    let mut staged_events = Vec::new();
    let migration_event = next_event_envelope(
        &staged_events,
        EventEnvelopeMetadata {
            command: String::from("migrate_legacy_authoritative_state"),
            plan_path: plan_path.clone(),
            plan_revision,
            execution_run_id: execution_run_id.clone(),
            branch_context: branch_context.clone(),
        },
        ExecutionEvent::MigrationImported {
            legacy_state_backup_path: backup_path.display().to_string(),
        },
    )?;
    staged_events.push(migration_event);

    for replay_payload in migration_replay_events_from_legacy_state(&state_payload) {
        let replay_event = next_event_envelope(
            &staged_events,
            EventEnvelopeMetadata {
                command: String::from("migrate_legacy_authoritative_state_replay"),
                plan_path: plan_path.clone(),
                plan_revision,
                execution_run_id: execution_run_id.clone(),
                branch_context: branch_context.clone(),
            },
            replay_payload,
        )?;
        staged_events.push(replay_event);
    }
    validate_event_log(&staged_events)?;
    validate_migration_parity(
        &migration_parity_source,
        &staged_events,
        runtime,
        legacy_state_path,
    )?;

    let lock_path = events_lock_path_for_state_path(legacy_state_path);
    let _lock_guard = acquire_event_log_lock(&lock_path)?;
    let events = load_event_log(&events_path)?;
    if !events.is_empty() {
        validate_event_log(&events)?;
        return Ok(());
    }

    write_event_log_atomic(&events_path, &staged_events)?;
    Ok(())
}

fn events_log_path_for_state_path(state_path: &Path) -> PathBuf {
    state_path.with_file_name("events.jsonl")
}

fn events_lock_path_for_state_path(state_path: &Path) -> PathBuf {
    state_path.with_file_name("events.lock")
}

fn legacy_state_backup_path_for_state_path(state_path: &Path) -> PathBuf {
    state_path.with_file_name("state.legacy.json")
}

fn ensure_regular_file_if_present(path: &Path, label: &str) -> Result<bool, JsonFailure> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!("Could not inspect {label} {}: {error}", path.display()),
            ));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!("{label} path must not be a symlink in {}.", path.display()),
        ));
    }
    if !metadata.is_file() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!("{label} must be a regular file in {}.", path.display()),
        ));
    }
    Ok(true)
}

fn acquire_event_log_lock(lock_path: &Path) -> Result<EventLogLockGuard, JsonFailure> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            JsonFailure::new(
                FailureClass::PartialAuthoritativeMutation,
                format!(
                    "Could not prepare event-log lock dir {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path)
    {
        Ok(mut file) => {
            let _ = writeln!(file, "pid={}", std::process::id());
            Ok(EventLogLockGuard {
                lock_path: lock_path.to_path_buf(),
            })
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Err(JsonFailure::new(
            FailureClass::ConcurrentWriterConflict,
            format!(
                "Another runtime writer holds event-log authority via lock {}.",
                lock_path.display()
            ),
        )),
        Err(error) => Err(JsonFailure::new(
            FailureClass::PartialAuthoritativeMutation,
            format!(
                "Could not acquire event-log lock {}: {error}",
                lock_path.display()
            ),
        )),
    }
}

fn load_event_log(events_path: &Path) -> Result<Vec<ExecutionEventEnvelope>, JsonFailure> {
    if !ensure_regular_file_if_present(events_path, "Authoritative event log")? {
        return Ok(Vec::new());
    }
    let source = match fs::read_to_string(events_path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Could not read event log {}: {error}",
                    events_path.display()
                ),
            ));
        }
    };
    source
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then_some((idx + 1, trimmed))
        })
        .map(|(line_no, line)| {
            serde_json::from_str::<ExecutionEventEnvelope>(line).map_err(|error| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    format!(
                        "Event log is malformed in {} at line {}: {error}",
                        events_path.display(),
                        line_no
                    ),
                )
            })
        })
        .collect()
}

fn append_event_line(
    events_path: &Path,
    event: &ExecutionEventEnvelope,
) -> Result<(), JsonFailure> {
    if let Some(parent) = events_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            JsonFailure::new(
                FailureClass::PartialAuthoritativeMutation,
                format!(
                    "Could not prepare event-log dir {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    let serialized = serde_json::to_string(event).map_err(|error| {
        JsonFailure::new(
            FailureClass::PartialAuthoritativeMutation,
            format!(
                "Could not serialize event-log envelope for {}: {error}",
                events_path.display()
            ),
        )
    })?;
    let mut sink = OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_path)
        .map_err(|error| {
            JsonFailure::new(
                FailureClass::PartialAuthoritativeMutation,
                format!(
                    "Could not open event log {}: {error}",
                    events_path.display()
                ),
            )
        })?;
    sink.write_all(serialized.as_bytes()).map_err(|error| {
        JsonFailure::new(
            FailureClass::PartialAuthoritativeMutation,
            format!(
                "Could not write event log {}: {error}",
                events_path.display()
            ),
        )
    })?;
    sink.write_all(b"\n").map_err(|error| {
        JsonFailure::new(
            FailureClass::PartialAuthoritativeMutation,
            format!(
                "Could not finalize event log {}: {error}",
                events_path.display()
            ),
        )
    })?;
    Ok(())
}

fn write_event_log_atomic(
    events_path: &Path,
    events: &[ExecutionEventEnvelope],
) -> Result<(), JsonFailure> {
    let mut serialized = String::new();
    for event in events {
        let line = serde_json::to_string(event).map_err(|error| {
            JsonFailure::new(
                FailureClass::PartialAuthoritativeMutation,
                format!(
                    "Could not serialize event-log envelope for {}: {error}",
                    events_path.display()
                ),
            )
        })?;
        serialized.push_str(&line);
        serialized.push('\n');
    }
    write_atomic_file(events_path, serialized).map_err(|error| {
        JsonFailure::new(
            FailureClass::PartialAuthoritativeMutation,
            format!(
                "Could not atomically publish event log {}: {error}",
                events_path.display()
            ),
        )
    })
}

fn migration_replay_events_from_legacy_state(state: &Value) -> Vec<ExecutionEvent> {
    let mut ordered = Vec::<(u64, usize, ExecutionEvent)>::new();
    let mut ordinal = 0usize;

    ordered.push((
        0,
        ordinal,
        ExecutionEvent::PreflightBootstrapRecorded {
            facts: Box::new(migration_run_metadata_state_fields(state).into()),
        },
    ));
    ordinal = ordinal.saturating_add(1);

    let open_step_task = json_u32_from_path(state, &["current_open_step_state", "task"]);
    let open_step_step = json_u32_from_path(state, &["current_open_step_state", "step"]);
    let open_step_note_state =
        json_string_from_path(state, &["current_open_step_state", "note_state"]);
    if open_step_task.is_some() || open_step_step.is_some() {
        let event = match open_step_note_state.as_deref() {
            Some("Active") => ExecutionEvent::Begin {
                task: open_step_task,
                step: open_step_step,
                facts: Box::new(migration_step_state_fields(state).into()),
            },
            Some("Completed") => ExecutionEvent::Complete {
                task: open_step_task,
                step: open_step_step,
                facts: Box::new(migration_step_state_fields(state).into()),
            },
            Some("Interrupted") => ExecutionEvent::Reopen {
                task: open_step_task,
                step: open_step_step,
                facts: Box::new(migration_step_state_fields(state).into()),
            },
            _ => ExecutionEvent::Note {
                task: open_step_task,
                step: open_step_step,
                note_state: open_step_note_state.clone(),
                facts: Box::new(migration_step_state_fields(state).into()),
            },
        };
        ordered.push((0, ordinal, event));
        ordinal = ordinal.saturating_add(1);
    }

    for (task, step) in legacy_completed_step_events(state) {
        if Some(task) == open_step_task
            && Some(step) == open_step_step
            && open_step_note_state.as_deref() == Some("Completed")
        {
            continue;
        }
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::Complete {
                task: Some(task),
                step: Some(step),
                facts: Box::<StepEventFacts>::default(),
            },
        ));
        ordinal = ordinal.saturating_add(1);
    }

    collect_history_replay_events(
        state,
        "current_task_closure_records",
        "task_closure_record",
        Some("current"),
        |entry_key, entry| {
            let task = entry
                .get("task")
                .and_then(Value::as_u64)
                .and_then(|raw| u32::try_from(raw).ok());
            let closure_record_id = entry
                .get("closure_record_id")
                .or_else(|| entry.get("record_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            ExecutionEvent::TaskClosureRecorded {
                task,
                closure_record_id,
                facts: Box::new(
                    migration_record_entry_fields("current_task_closure_records", entry_key, entry)
                        .into(),
                ),
            }
        },
        &mut ordered,
        &mut ordinal,
    );

    collect_history_replay_events(
        state,
        "task_closure_record_history",
        "task_closure_record",
        None,
        |entry_key, entry| {
            let task = entry
                .get("task")
                .and_then(Value::as_u64)
                .and_then(|raw| u32::try_from(raw).ok());
            let closure_record_id = entry
                .get("closure_record_id")
                .or_else(|| entry.get("record_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            ExecutionEvent::TaskClosureRecorded {
                task,
                closure_record_id,
                facts: Box::new(
                    migration_record_entry_fields("task_closure_record_history", entry_key, entry)
                        .into(),
                ),
            }
        },
        &mut ordered,
        &mut ordinal,
    );

    collect_history_replay_events(
        state,
        "branch_closure_records",
        "branch_closure_record",
        None,
        |entry_key, entry| {
            let branch_closure_id = entry
                .get("branch_closure_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            ExecutionEvent::BranchClosureRecorded {
                branch_closure_id: branch_closure_id.clone(),
                facts: Box::new(
                    migration_branch_closure_entry_fields(
                        state,
                        entry_key,
                        entry,
                        branch_closure_id.as_deref(),
                    )
                    .into(),
                ),
            }
        },
        &mut ordered,
        &mut ordinal,
    );
    if record_history_empty(state, "branch_closure_records")
        && json_string(state, "current_branch_closure_id").is_some()
    {
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::BranchClosureRecorded {
                branch_closure_id: json_string(state, "current_branch_closure_id"),
                facts: Box::new(migration_branch_closure_current_fields(state).into()),
            },
        ));
        ordinal = ordinal.saturating_add(1);
    }

    collect_history_replay_events(
        state,
        "release_readiness_record_history",
        "release_readiness_record",
        None,
        |entry_key, entry| {
            let release_readiness_record_id = entry
                .get("record_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            ExecutionEvent::ReleaseReadinessRecorded {
                release_readiness_record_id: release_readiness_record_id.clone(),
                facts: Box::new(
                    migration_release_readiness_entry_fields(
                        state,
                        entry_key,
                        entry,
                        release_readiness_record_id.as_deref(),
                    )
                    .into(),
                ),
            }
        },
        &mut ordered,
        &mut ordinal,
    );
    if legacy_current_scalar_fields_present(
        state,
        &[
            "current_release_readiness_record_id",
            "current_release_readiness_result",
            "current_release_readiness_summary_hash",
        ],
    ) && scalar_current_not_replayed_from_history(
        state,
        "release_readiness_record_history",
        "current_release_readiness_record_id",
    ) {
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::ReleaseReadinessRecorded {
                release_readiness_record_id: json_string(
                    state,
                    "current_release_readiness_record_id",
                ),
                facts: Box::new(migration_release_readiness_current_fields(state).into()),
            },
        ));
        ordinal = ordinal.saturating_add(1);
    }

    collect_history_replay_events(
        state,
        "final_review_record_history",
        "final_review_record",
        None,
        |entry_key, entry| {
            let final_review_record_id = entry
                .get("record_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            let dispatch_id = entry
                .get("dispatch_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            ExecutionEvent::FinalReviewRecorded {
                final_review_record_id: final_review_record_id.clone(),
                dispatch_id,
                facts: Box::new(
                    migration_final_review_entry_fields(
                        state,
                        entry_key,
                        entry,
                        final_review_record_id.as_deref(),
                    )
                    .into(),
                ),
            }
        },
        &mut ordered,
        &mut ordinal,
    );
    if legacy_current_scalar_fields_present(
        state,
        &[
            "current_final_review_branch_closure_id",
            "current_final_review_dispatch_id",
            "current_final_review_reviewer_source",
            "current_final_review_reviewer_id",
            "current_final_review_result",
            "current_final_review_summary_hash",
            "current_final_review_record_id",
        ],
    ) && scalar_current_not_replayed_from_history(
        state,
        "final_review_record_history",
        "current_final_review_record_id",
    ) {
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::FinalReviewRecorded {
                final_review_record_id: json_string(state, "current_final_review_record_id"),
                dispatch_id: json_string(state, "current_final_review_dispatch_id"),
                facts: Box::new(migration_final_review_current_fields(state).into()),
            },
        ));
        ordinal = ordinal.saturating_add(1);
    }

    collect_history_replay_events(
        state,
        "browser_qa_record_history",
        "browser_qa_record",
        None,
        |entry_key, entry| {
            let qa_record_id = entry
                .get("record_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            ExecutionEvent::QaRecorded {
                qa_record_id: qa_record_id.clone(),
                facts: Box::new(
                    migration_qa_entry_fields(state, entry_key, entry, qa_record_id.as_deref())
                        .into(),
                ),
            }
        },
        &mut ordered,
        &mut ordinal,
    );
    if legacy_current_scalar_fields_present(
        state,
        &[
            "current_qa_branch_closure_id",
            "current_qa_result",
            "current_qa_summary_hash",
            "current_qa_record_id",
        ],
    ) && scalar_current_not_replayed_from_history(
        state,
        "browser_qa_record_history",
        "current_qa_record_id",
    ) {
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::QaRecorded {
                qa_record_id: json_string(state, "current_qa_record_id"),
                facts: Box::new(migration_qa_current_fields(state).into()),
            },
        ));
        ordinal = ordinal.saturating_add(1);
    }

    if let Some(dispatch_id) = json_string(state, "current_task_review_dispatch_id") {
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::DispatchRecorded {
                scope: Some(String::from("task")),
                dispatch_id: Some(dispatch_id),
                facts: Box::new(migration_dispatch_state_fields(state).into()),
            },
        ));
        ordinal = ordinal.saturating_add(1);
    }
    if let Some(dispatch_id) = json_string(state, "current_final_review_dispatch_id") {
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::DispatchRecorded {
                scope: Some(String::from("final_review")),
                dispatch_id: Some(dispatch_id),
                facts: Box::new(migration_dispatch_state_fields(state).into()),
            },
        ));
        ordinal = ordinal.saturating_add(1);
    }
    ordered.push((
        0,
        ordinal,
        ExecutionEvent::DispatchRecorded {
            scope: None,
            dispatch_id: None,
            facts: Box::new(migration_dispatch_state_fields(state).into()),
        },
    ));
    ordinal = ordinal.saturating_add(1);

    if json_string(state, "last_handoff_path").is_some()
        || json_string(state, "last_handoff_fingerprint").is_some()
        || state.get("handoff_required").is_some()
    {
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::HandoffRecorded {
                facts: Box::new(migration_handoff_state_fields(state).into()),
            },
        ));
        ordinal = ordinal.saturating_add(1);
    }

    if let Some(follow_up) = json_string(state, "review_state_repair_follow_up")
        .as_deref()
        .and_then(|follow_up| normalize_persisted_repair_follow_up_token(Some(follow_up)))
        .map(str::to_owned)
    {
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::RepairFollowUpSet {
                follow_up,
                facts: Box::new(migration_repair_state_fields(state).into()),
            },
        ));
        ordinal = ordinal.saturating_add(1);
    } else {
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::RepairFollowUpCleared {
                facts: Box::new(migration_repair_state_fields(state).into()),
            },
        ));
        ordinal = ordinal.saturating_add(1);
    }

    let strategy_kind = json_string(state, "strategy_checkpoint_kind");
    let strategy_fingerprint = json_string(state, "last_strategy_checkpoint_fingerprint");
    if strategy_kind.is_some() || strategy_fingerprint.is_some() {
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::StrategyCheckpointRecorded {
                checkpoint_kind: strategy_kind,
                checkpoint_fingerprint: strategy_fingerprint,
                facts: Box::new(migration_strategy_state_fields(state).into()),
            },
        ));
    } else {
        ordered.push((
            0,
            ordinal,
            ExecutionEvent::StrategyCheckpointCleared {
                facts: Box::new(migration_strategy_state_fields(state).into()),
            },
        ));
    }

    ordered.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    ordered.into_iter().map(|(_, _, payload)| payload).collect()
}

fn legacy_completed_step_events(state: &Value) -> BTreeSet<(u32, u32)> {
    let Some(completed_steps) = state
        .get("event_completed_steps")
        .and_then(Value::as_object)
    else {
        return BTreeSet::new();
    };
    completed_steps
        .iter()
        .filter_map(|(key, entry)| legacy_completed_step_from_entry(key, entry))
        .collect()
}

fn legacy_completed_step_from_entry(key: &str, entry: &Value) -> Option<(u32, u32)> {
    let parsed_from_entry = entry
        .as_object()
        .and_then(|_| {
            Some((
                entry.get("task").and_then(Value::as_u64)?,
                entry.get("step").and_then(Value::as_u64)?,
            ))
        })
        .and_then(|(task, step)| Some((u32::try_from(task).ok()?, u32::try_from(step).ok()?)));
    parsed_from_entry.or_else(|| legacy_completed_step_from_key(key))
}

fn legacy_completed_step_from_key(key: &str) -> Option<(u32, u32)> {
    let (task, step) = key.split_once('.')?;
    Some((task.trim().parse().ok()?, step.trim().parse().ok()?))
}

fn collect_history_replay_events<F>(
    state: &Value,
    field: &str,
    record_family: &str,
    default_record_status: Option<&str>,
    mut make_event: F,
    ordered: &mut Vec<(u64, usize, ExecutionEvent)>,
    ordinal: &mut usize,
) where
    F: FnMut(&str, &Value) -> ExecutionEvent,
{
    let Some(entries) = state.get(field).and_then(Value::as_object) else {
        return;
    };
    for (entry_key, entry) in entries {
        let sequence = entry
            .get("record_sequence")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let record_id = entry
            .get("record_id")
            .or_else(|| entry.get("closure_record_id"))
            .or_else(|| entry.get("branch_closure_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                let trimmed = entry_key.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_owned())
            });
        let record_status = entry
            .get("record_status")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let record_status = record_status.or_else(|| default_record_status.map(str::to_owned));
        let status_transition_would_promote_historical_current = field
            == "task_closure_record_history"
            && record_family == "task_closure_record"
            && record_status.as_deref() == Some("current");
        ordered.push((sequence, *ordinal, make_event(entry_key, entry)));
        *ordinal = ordinal.saturating_add(1);
        if !status_transition_would_promote_historical_current {
            ordered.push((
                sequence,
                *ordinal,
                ExecutionEvent::RecordStatusTransition {
                    record_family: record_family.to_owned(),
                    record_id,
                    record_status,
                    record_sequence: Some(sequence),
                },
            ));
            *ordinal = ordinal.saturating_add(1);
        }
    }
}

fn migration_record_entry_fields(
    field: &str,
    entry_key: &str,
    entry: &Value,
) -> AuthoritativeFactBuilder {
    let mut facts = AuthoritativeFactBuilder::default();
    facts.set_field_value(field, single_record_delta(entry_key, entry));
    facts
}

fn migration_branch_closure_entry_fields(
    state: &Value,
    entry_key: &str,
    entry: &Value,
    branch_closure_id: Option<&str>,
) -> AuthoritativeFactBuilder {
    let mut facts = migration_record_entry_fields("branch_closure_records", entry_key, entry);
    let is_current = branch_closure_id
        .zip(json_string(state, "current_branch_closure_id").as_deref())
        .is_some_and(|(entry_id, current_id)| entry_id == current_id);
    if is_current {
        copy_present_fields(
            state,
            &mut facts,
            &[
                "current_branch_closure_id",
                "current_branch_closure_reviewed_state_id",
                "current_branch_closure_contract_identity",
            ],
        );
    }
    facts
}

fn migration_release_readiness_entry_fields(
    state: &Value,
    entry_key: &str,
    entry: &Value,
    record_id: Option<&str>,
) -> AuthoritativeFactBuilder {
    let mut facts =
        migration_record_entry_fields("release_readiness_record_history", entry_key, entry);
    let is_current = record_id
        .zip(json_string(state, "current_release_readiness_record_id").as_deref())
        .is_some_and(|(entry_id, current_id)| entry_id == current_id);
    if is_current {
        copy_present_fields(
            state,
            &mut facts,
            &[
                "current_release_readiness_record_id",
                "current_release_readiness_result",
                "current_release_readiness_summary_hash",
            ],
        );
    }
    facts
}

fn migration_final_review_entry_fields(
    state: &Value,
    entry_key: &str,
    entry: &Value,
    record_id: Option<&str>,
) -> AuthoritativeFactBuilder {
    let mut facts = migration_record_entry_fields("final_review_record_history", entry_key, entry);
    let is_current = record_id
        .zip(json_string(state, "current_final_review_record_id").as_deref())
        .is_some_and(|(entry_id, current_id)| entry_id == current_id);
    if is_current {
        copy_present_fields(
            state,
            &mut facts,
            &[
                "current_final_review_branch_closure_id",
                "current_final_review_dispatch_id",
                "current_final_review_reviewer_source",
                "current_final_review_reviewer_id",
                "current_final_review_result",
                "current_final_review_summary_hash",
                "current_final_review_record_id",
            ],
        );
    }
    facts
}

fn migration_qa_entry_fields(
    state: &Value,
    entry_key: &str,
    entry: &Value,
    record_id: Option<&str>,
) -> AuthoritativeFactBuilder {
    let mut facts = migration_record_entry_fields("browser_qa_record_history", entry_key, entry);
    let is_current = record_id
        .zip(json_string(state, "current_qa_record_id").as_deref())
        .is_some_and(|(entry_id, current_id)| entry_id == current_id);
    if is_current {
        copy_present_fields(
            state,
            &mut facts,
            &[
                "current_qa_branch_closure_id",
                "current_qa_result",
                "current_qa_summary_hash",
                "current_qa_record_id",
            ],
        );
    }
    facts
}

fn single_record_delta(entry_key: &str, entry: &Value) -> Value {
    let mut map = serde_json::Map::new();
    map.insert(entry_key.to_owned(), entry.clone());
    Value::Object(map)
}

fn copy_present_fields(state: &Value, facts: &mut AuthoritativeFactBuilder, fields: &[&str]) {
    for field in fields {
        if let Some(value) = state.get(*field) {
            facts.set_field_value(field, value.clone());
        }
    }
}

fn record_history_empty(state: &Value, field: &str) -> bool {
    state
        .get(field)
        .and_then(Value::as_object)
        .is_none_or(serde_json::Map::is_empty)
}

fn legacy_current_scalar_fields_present(state: &Value, fields: &[&str]) -> bool {
    fields
        .iter()
        .any(|field| state.get(*field).is_some_and(|value| !value.is_null()))
}

fn scalar_current_not_replayed_from_history(
    state: &Value,
    history_field: &str,
    current_id_field: &str,
) -> bool {
    let Some(current_id) = json_string(state, current_id_field) else {
        return true;
    };
    let Some(history) = state.get(history_field).and_then(Value::as_object) else {
        return true;
    };
    !history.iter().any(|(entry_key, entry)| {
        record_entry_matches_id(entry_key, entry, &current_id)
            && entry
                .get("record_status")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|status| status == "current")
    })
}

fn migration_step_state_fields(state: &Value) -> AuthoritativeFactBuilder {
    collect_authoritative_fields(state, STEP_EVENT_FIELDS)
}

fn migration_run_metadata_state_fields(state: &Value) -> AuthoritativeFactBuilder {
    collect_authoritative_fields(state, RUN_METADATA_FIELDS)
}

fn migration_branch_closure_current_fields(state: &Value) -> AuthoritativeFactBuilder {
    collect_authoritative_fields(
        state,
        &[
            "current_branch_closure_id",
            "current_branch_closure_reviewed_state_id",
            "current_branch_closure_contract_identity",
            "harness_phase",
        ],
    )
}

fn migration_release_readiness_current_fields(state: &Value) -> AuthoritativeFactBuilder {
    collect_authoritative_fields(
        state,
        &[
            "current_release_readiness_record_id",
            "current_release_readiness_result",
            "current_release_readiness_summary_hash",
            "harness_phase",
        ],
    )
}

fn migration_final_review_current_fields(state: &Value) -> AuthoritativeFactBuilder {
    collect_authoritative_fields(
        state,
        &[
            "current_final_review_branch_closure_id",
            "current_final_review_dispatch_id",
            "current_final_review_reviewer_source",
            "current_final_review_reviewer_id",
            "current_final_review_result",
            "current_final_review_summary_hash",
            "current_final_review_record_id",
            "harness_phase",
        ],
    )
}

fn migration_qa_current_fields(state: &Value) -> AuthoritativeFactBuilder {
    collect_authoritative_fields(
        state,
        &[
            "current_qa_branch_closure_id",
            "current_qa_result",
            "current_qa_summary_hash",
            "current_qa_record_id",
            "harness_phase",
        ],
    )
}

fn migration_dispatch_state_fields(state: &Value) -> AuthoritativeFactBuilder {
    collect_authoritative_fields(
        state,
        &[
            "current_task_review_dispatch_id",
            "current_final_review_dispatch_id",
            "task_closure_negative_result_records",
            "task_closure_negative_result_history",
            "strategy_review_dispatch_lineage",
            "strategy_review_dispatch_lineage_history",
            "final_review_dispatch_lineage",
            "final_review_dispatch_lineage_history",
            "harness_phase",
        ],
    )
}

fn migration_handoff_state_fields(state: &Value) -> AuthoritativeFactBuilder {
    collect_authoritative_fields(state, HANDOFF_FIELDS)
}

fn migration_repair_state_fields(state: &Value) -> AuthoritativeFactBuilder {
    collect_authoritative_fields(
        state,
        &[
            "review_state_repair_follow_up",
            "harness_phase",
            "current_open_step_state",
            "current_task_closure_records",
            "task_closure_record_history",
            "task_closure_negative_result_records",
            "task_closure_negative_result_history",
            "superseded_task_closure_ids",
            "current_branch_closure_id",
            "current_branch_closure_reviewed_state_id",
            "current_branch_closure_contract_identity",
            "branch_closure_records",
            "superseded_branch_closure_ids",
        ],
    )
}

fn migration_strategy_state_fields(state: &Value) -> AuthoritativeFactBuilder {
    collect_authoritative_fields(state, STRATEGY_EVENT_FIELDS)
}

fn next_event_envelope(
    prior_events: &[ExecutionEventEnvelope],
    metadata: EventEnvelopeMetadata,
    payload: ExecutionEvent,
) -> Result<ExecutionEventEnvelope, JsonFailure> {
    let EventEnvelopeMetadata {
        command,
        plan_path,
        plan_revision,
        execution_run_id,
        branch_context,
    } = metadata;
    let previous_event_hash = prior_events.last().map(|event| event.event_hash.clone());
    let sequence = prior_events
        .last()
        .map_or(1_u64, |event| event.sequence.saturating_add(1));
    let timestamp = Timestamp::now().to_string();
    let seed = format!(
        "{}:{}:{}:{}",
        sequence,
        command,
        timestamp,
        previous_event_hash.as_deref().unwrap_or("none")
    );
    let event_id = format!("evt-{}-{}", sequence, &sha256_hex(seed.as_bytes())[..12]);
    let mut envelope = ExecutionEventEnvelope {
        schema_version: EVENT_LOG_SCHEMA_VERSION,
        event_id,
        sequence,
        timestamp,
        command,
        plan_path,
        plan_revision,
        execution_run_id,
        branch_context,
        previous_event_hash,
        event_hash: String::new(),
        payload,
    };
    envelope.event_hash = compute_event_hash(&envelope)?;
    Ok(envelope)
}

fn validate_event_log(events: &[ExecutionEventEnvelope]) -> Result<(), JsonFailure> {
    for (idx, event) in events.iter().enumerate() {
        let expected_sequence = (idx as u64).saturating_add(1);
        if event.sequence != expected_sequence {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Event log sequence mismatch at entry {}: expected {}, found {}.",
                    idx + 1,
                    expected_sequence,
                    event.sequence
                ),
            ));
        }
        if event.schema_version != EVENT_LOG_SCHEMA_VERSION {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Event log schema mismatch at entry {}: expected {}, found {}.",
                    idx + 1,
                    EVENT_LOG_SCHEMA_VERSION,
                    event.schema_version
                ),
            ));
        }
        let expected_previous = idx
            .checked_sub(1)
            .and_then(|prior| events.get(prior))
            .map(|prior| prior.event_hash.clone());
        if event.previous_event_hash != expected_previous {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Event log continuity mismatch at entry {}: previous hash does not match.",
                    idx + 1
                ),
            ));
        }
        let computed_hash = compute_event_hash(event)?;
        if computed_hash != event.event_hash {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Event log hash mismatch at entry {}: payload hash does not match envelope hash.",
                    idx + 1
                ),
            ));
        }
        if let Some(facts) = event.payload.facts() {
            for field in facts.populated_field_names() {
                if !event_fact_field_allowed(&event.payload, field) {
                    return Err(JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        format!(
                            "Event log entry {} contains non-authoritative field `{field}` for `{}` event scope.",
                            idx + 1,
                            event.payload.kind_name()
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

impl ExecutionEvent {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::MigrationImported { .. } => "migration_imported",
            Self::Begin { .. } => "begin",
            Self::Note { .. } => "note",
            Self::Complete { .. } => "complete",
            Self::Reopen { .. } => "reopen",
            Self::Transfer { .. } => "transfer",
            Self::DispatchRecorded { .. } => "dispatch_recorded",
            Self::TaskClosureRecorded { .. } => "task_closure_recorded",
            Self::BranchClosureRecorded { .. } => "branch_closure_recorded",
            Self::ReleaseReadinessRecorded { .. } => "release_readiness_recorded",
            Self::FinalReviewRecorded { .. } => "final_review_recorded",
            Self::QaRecorded { .. } => "qa_recorded",
            Self::RepairFollowUpSet { .. } => "repair_follow_up_set",
            Self::RepairFollowUpCleared { .. } => "repair_follow_up_cleared",
            Self::StrategyCheckpointRecorded { .. } => "strategy_checkpoint_recorded",
            Self::StrategyCheckpointCleared { .. } => "strategy_checkpoint_cleared",
            Self::PreflightBootstrapRecorded { .. } => "preflight_bootstrap_recorded",
            Self::ContractRecorded { .. } => "contract_recorded",
            Self::EvaluationRecorded { .. } => "evaluation_recorded",
            Self::HandoffRecorded { .. } => "handoff_recorded",
            Self::WorktreeLeaseIndexUpdated { .. } => "worktree_lease_index_updated",
            Self::RecordStatusTransition { .. } => "record_status_transition",
        }
    }
}

fn event_fact_field_allowed(event: &ExecutionEvent, field: &str) -> bool {
    if !authoritative_event_field_allowed(field) {
        return false;
    }
    match event {
        ExecutionEvent::MigrationImported { .. }
        | ExecutionEvent::RecordStatusTransition { .. } => false,
        ExecutionEvent::Begin { .. }
        | ExecutionEvent::Note { .. }
        | ExecutionEvent::Complete { .. }
        | ExecutionEvent::Reopen { .. } => STEP_EVENT_FIELDS.contains(&field),
        ExecutionEvent::Transfer { .. } => TRANSFER_EVENT_FIELDS.contains(&field),
        ExecutionEvent::DispatchRecorded { .. } => DISPATCH_EVENT_FIELDS.contains(&field),
        ExecutionEvent::TaskClosureRecorded { .. } => TASK_CLOSURE_EVENT_FIELDS.contains(&field),
        ExecutionEvent::BranchClosureRecorded { .. } => {
            BRANCH_CLOSURE_EVENT_FIELDS.contains(&field)
        }
        ExecutionEvent::ReleaseReadinessRecorded { .. } => {
            RELEASE_READINESS_EVENT_FIELDS.contains(&field)
        }
        ExecutionEvent::FinalReviewRecorded { .. } => FINAL_REVIEW_EVENT_FIELDS.contains(&field),
        ExecutionEvent::QaRecorded { .. } => QA_EVENT_FIELDS.contains(&field),
        ExecutionEvent::RepairFollowUpSet { .. } | ExecutionEvent::RepairFollowUpCleared { .. } => {
            REPAIR_EVENT_FIELDS.contains(&field)
        }
        ExecutionEvent::StrategyCheckpointRecorded { .. }
        | ExecutionEvent::StrategyCheckpointCleared { .. } => {
            STRATEGY_EVENT_FIELDS.contains(&field)
        }
        ExecutionEvent::PreflightBootstrapRecorded { .. }
        | ExecutionEvent::ContractRecorded { .. }
        | ExecutionEvent::EvaluationRecorded { .. }
        | ExecutionEvent::WorktreeLeaseIndexUpdated { .. } => RUN_METADATA_FIELDS.contains(&field),
        ExecutionEvent::HandoffRecorded { .. } => HANDOFF_FIELDS.contains(&field),
    }
}

fn compute_event_hash(event: &ExecutionEventEnvelope) -> Result<String, JsonFailure> {
    let hash_shape = EventHashShape {
        schema_version: event.schema_version,
        event_id: &event.event_id,
        sequence: event.sequence,
        timestamp: &event.timestamp,
        command: &event.command,
        plan_path: event.plan_path.as_deref(),
        plan_revision: event.plan_revision,
        execution_run_id: event.execution_run_id.as_deref(),
        branch_context: &event.branch_context,
        previous_event_hash: event.previous_event_hash.as_deref(),
        payload: &event.payload,
    };
    let serialized = serde_json::to_string(&hash_shape).map_err(|error| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!("Could not serialize event hash shape: {error}"),
        )
    })?;
    Ok(sha256_hex(serialized.as_bytes()))
}

fn reduce_events_to_state(events: &[ExecutionEventEnvelope]) -> Option<Value> {
    let mut reduced = Value::Null;
    let mut reduced_any = false;
    for event in events {
        reduced_any |= apply_typed_event_to_state(&mut reduced, event);
    }
    reduced_any.then_some(reduced)
}

fn apply_typed_event_to_state(target: &mut Value, event: &ExecutionEventEnvelope) -> bool {
    match &event.payload {
        ExecutionEvent::MigrationImported { .. } => false,
        ExecutionEvent::RecordStatusTransition {
            record_family,
            record_id,
            record_status,
            record_sequence,
        } => apply_record_status_transition(
            target,
            record_family,
            record_id.as_deref(),
            record_status.as_deref(),
            *record_sequence,
        ),
        ExecutionEvent::Begin { facts, .. } | ExecutionEvent::Note { facts, .. } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
        ExecutionEvent::Complete { task, step, facts } => {
            let applied = apply_typed_authoritative_facts(target, facts.as_ref());
            apply_completed_step_event(target, *task, *step, true) || applied
        }
        ExecutionEvent::Reopen { task, step, facts } => {
            let applied = apply_typed_authoritative_facts(target, facts.as_ref());
            apply_completed_step_event(target, *task, *step, false) || applied
        }
        ExecutionEvent::Transfer { facts, .. } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
        ExecutionEvent::DispatchRecorded { facts, .. } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
        ExecutionEvent::TaskClosureRecorded { facts, .. } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
        ExecutionEvent::BranchClosureRecorded { facts, .. } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
        ExecutionEvent::ReleaseReadinessRecorded { facts, .. } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
        ExecutionEvent::FinalReviewRecorded { facts, .. } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
        ExecutionEvent::QaRecorded { facts, .. } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
        ExecutionEvent::RepairFollowUpSet { facts, .. }
        | ExecutionEvent::RepairFollowUpCleared { facts } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
        ExecutionEvent::StrategyCheckpointRecorded { facts, .. }
        | ExecutionEvent::StrategyCheckpointCleared { facts } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
        ExecutionEvent::PreflightBootstrapRecorded { facts }
        | ExecutionEvent::ContractRecorded { facts }
        | ExecutionEvent::EvaluationRecorded { facts }
        | ExecutionEvent::WorktreeLeaseIndexUpdated { facts } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
        ExecutionEvent::HandoffRecorded { facts } => {
            apply_typed_authoritative_facts(target, facts.as_ref())
        }
    }
}

fn apply_typed_authoritative_facts(target: &mut Value, facts: &dyn EventFactPayload) -> bool {
    let selected = facts
        .populated_field_names()
        .iter()
        .filter_map(|field| {
            facts
                .get(field)
                .map(|value| ((*field).to_owned(), value.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    apply_authoritative_fields(target, &selected);
    !selected.is_empty()
}

fn apply_completed_step_event(
    target: &mut Value,
    task: Option<u32>,
    step: Option<u32>,
    completed: bool,
) -> bool {
    let (Some(task), Some(step)) = (task, step) else {
        return false;
    };
    if !target.is_object() {
        *target = Value::Object(serde_json::Map::new());
    }
    let target_map = target
        .as_object_mut()
        .expect("authoritative state should normalize to object");
    let completed_steps = target_map
        .entry(String::from("event_completed_steps"))
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !completed_steps.is_object() {
        *completed_steps = Value::Object(serde_json::Map::new());
    }
    let completed_map = completed_steps
        .as_object_mut()
        .expect("event_completed_steps should normalize to object");
    let key = format!("{task}.{step}");
    if completed {
        completed_map.insert(
            key,
            serde_json::json!({
                "task": task,
                "step": step,
            }),
        );
    } else {
        completed_map.remove(&key);
    }
    true
}

fn apply_authoritative_fields(target: &mut Value, authoritative_fields: &BTreeMap<String, Value>) {
    if authoritative_fields.is_empty() {
        return;
    }
    if !target.is_object() {
        *target = Value::Object(serde_json::Map::new());
    }
    let target_map = target
        .as_object_mut()
        .expect("authoritative state should normalize to object");
    for (field, value) in authoritative_fields {
        if value.is_null() {
            target_map.remove(field);
        } else if record_map_delta_field(field) {
            apply_record_map_delta(target_map, field, value);
        } else {
            target_map.insert(field.clone(), value.clone());
        }
    }
}

fn record_map_delta_field(field: &str) -> bool {
    RECORD_MAP_DELTA_FIELDS.contains(&field)
}

fn apply_record_map_delta(
    target_map: &mut serde_json::Map<String, Value>,
    field: &str,
    delta: &Value,
) {
    let Some(delta_entries) = delta.as_object() else {
        target_map.insert(field.to_owned(), delta.clone());
        return;
    };
    let existing = target_map
        .entry(field.to_owned())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !existing.is_object() {
        *existing = Value::Object(serde_json::Map::new());
    }
    let existing_entries = existing
        .as_object_mut()
        .expect("record-map delta target should normalize to object");
    for (record_key, record_value) in delta_entries {
        if record_value.is_null() {
            existing_entries.remove(record_key);
        } else {
            existing_entries.insert(record_key.clone(), record_value.clone());
        }
    }
}

fn apply_record_status_transition(
    target: &mut Value,
    record_family: &str,
    record_id: Option<&str>,
    record_status: Option<&str>,
    record_sequence: Option<u64>,
) -> bool {
    let Some(record_id) = record_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let Some(map_fields) = record_status_map_fields(record_family) else {
        return false;
    };
    if !target.is_object() {
        *target = Value::Object(serde_json::Map::new());
    }
    let target_map = target
        .as_object_mut()
        .expect("authoritative state should normalize to object");

    let mut applied = false;
    for map_field in map_fields {
        if record_family == "task_closure_record"
            && *map_field == "current_task_closure_records"
            && record_status != Some("current")
        {
            continue;
        }
        if record_family == "task_closure_record"
            && *map_field == "task_closure_record_history"
            && record_status == Some("current")
        {
            continue;
        }
        let map_value = target_map
            .entry((*map_field).to_owned())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !map_value.is_object() {
            *map_value = Value::Object(serde_json::Map::new());
        }
        let records = map_value
            .as_object_mut()
            .expect("record status target should normalize to object");
        let existing_key = records.iter().find_map(|(key, entry)| {
            record_entry_matches_id(key, entry, record_id).then(|| key.clone())
        });
        let entry_was_present = existing_key.is_some();
        let existing_key = existing_key.unwrap_or_else(|| record_id.to_owned());
        let entry = records
            .entry(existing_key)
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(serde_json::Map::new());
        }
        if entry_was_present {
            applied = true;
            continue;
        }
        let entry_map = entry
            .as_object_mut()
            .expect("record status entry should normalize to object");
        entry_map.insert(
            String::from("record_id"),
            Value::String(record_id.to_owned()),
        );
        if let Some(record_status) = record_status
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            entry_map.insert(
                String::from("record_status"),
                Value::String(record_status.to_owned()),
            );
        }
        if let Some(record_sequence) = record_sequence {
            entry_map.insert(
                String::from("record_sequence"),
                Value::Number(serde_json::Number::from(record_sequence)),
            );
        }
        applied = true;
    }
    applied
}

fn record_status_map_fields(record_family: &str) -> Option<&'static [&'static str]> {
    match record_family {
        "task_closure_record" => Some(&[
            "current_task_closure_records",
            "task_closure_record_history",
        ]),
        "branch_closure_record" => Some(&["branch_closure_records"]),
        "release_readiness_record" => Some(&["release_readiness_record_history"]),
        "final_review_record" => Some(&["final_review_record_history"]),
        "browser_qa_record" | "qa_record" => Some(&["browser_qa_record_history"]),
        _ => None,
    }
}

fn record_entry_matches_id(entry_key: &str, entry: &Value, record_id: &str) -> bool {
    entry_key == record_id
        || entry
            .get("record_id")
            .or_else(|| entry.get("closure_record_id"))
            .or_else(|| entry.get("branch_closure_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|candidate| candidate == record_id)
}

fn event_from_command_authoritative_delta(
    command: &str,
    current_state: &Value,
    state: &Value,
    _reason: &str,
    step_hint: Option<(u32, u32)>,
) -> Result<ExecutionEvent, JsonFailure> {
    let open_step_task = step_hint
        .map(|(task, _)| task)
        .or_else(|| json_u32_from_path(state, &["current_open_step_state", "task"]))
        .or_else(|| {
            (command == "complete")
                .then(|| json_u32_from_path(current_state, &["current_open_step_state", "task"]))
                .flatten()
        });
    let open_step_step = step_hint
        .map(|(_, step)| step)
        .or_else(|| json_u32_from_path(state, &["current_open_step_state", "step"]))
        .or_else(|| {
            (command == "complete")
                .then(|| json_u32_from_path(current_state, &["current_open_step_state", "step"]))
                .flatten()
        });
    let open_step_note_state =
        json_string_from_path(state, &["current_open_step_state", "note_state"]);
    let follow_up = json_string(state, "review_state_repair_follow_up")
        .as_deref()
        .and_then(|follow_up| normalize_persisted_repair_follow_up_token(Some(follow_up)))
        .map(str::to_owned);
    let strategy_kind = json_string(state, "strategy_checkpoint_kind");
    let strategy_fingerprint = json_string(state, "last_strategy_checkpoint_fingerprint");
    let step_state = collect_changed_authoritative_fields(current_state, state, STEP_EVENT_FIELDS);
    let dispatch_state =
        collect_changed_authoritative_fields(current_state, state, DISPATCH_EVENT_FIELDS);
    let task_closure_state =
        collect_changed_authoritative_fields(current_state, state, TASK_CLOSURE_EVENT_FIELDS);
    let branch_closure_state =
        collect_changed_authoritative_fields(current_state, state, BRANCH_CLOSURE_EVENT_FIELDS);
    let release_readiness_state =
        collect_changed_authoritative_fields(current_state, state, RELEASE_READINESS_EVENT_FIELDS);
    let final_review_state =
        collect_changed_authoritative_fields(current_state, state, FINAL_REVIEW_EVENT_FIELDS);
    let qa_state = collect_changed_authoritative_fields(current_state, state, QA_EVENT_FIELDS);
    let repair_state =
        collect_changed_authoritative_fields(current_state, state, REPAIR_EVENT_FIELDS);
    let strategy_state =
        collect_changed_authoritative_fields(current_state, state, STRATEGY_EVENT_FIELDS);
    match command.trim() {
        "begin" => Ok(ExecutionEvent::Begin {
            task: open_step_task,
            step: open_step_step,
            facts: Box::new(step_state.into()),
        }),
        "note" => Ok(ExecutionEvent::Note {
            task: open_step_task,
            step: open_step_step,
            note_state: open_step_note_state,
            facts: Box::new(step_state.into()),
        }),
        "complete" => Ok(ExecutionEvent::Complete {
            task: open_step_task,
            step: open_step_step,
            facts: Box::new(step_state.into()),
        }),
        "reopen" => Ok(ExecutionEvent::Reopen {
            task: open_step_task,
            step: open_step_step,
            facts: Box::new(step_state.into()),
        }),
        "transfer" => Ok(ExecutionEvent::Transfer {
            scope: json_string(state, "current_transfer_scope"),
            assignee: json_string(state, "current_transfer_to"),
            facts: Box::new(
                collect_changed_authoritative_fields(current_state, state, TRANSFER_EVENT_FIELDS)
                    .into(),
            ),
        }),
        "record_review_dispatch" => Ok(ExecutionEvent::DispatchRecorded {
            scope: dispatch_scope_from_state(state),
            dispatch_id: current_dispatch_id_from_state(state),
            facts: Box::new(dispatch_state.into()),
        }),
        "close_current_task" => Ok(ExecutionEvent::TaskClosureRecorded {
            task: newest_current_task_from_state(state),
            closure_record_id: newest_current_task_closure_id_from_state(state),
            facts: Box::new(task_closure_state.into()),
        }),
        "record_branch_closure" => Ok(ExecutionEvent::BranchClosureRecorded {
            branch_closure_id: json_string(state, "current_branch_closure_id"),
            facts: Box::new(branch_closure_state.into()),
        }),
        "record_release_readiness" => Ok(ExecutionEvent::ReleaseReadinessRecorded {
            release_readiness_record_id: json_string(state, "current_release_readiness_record_id"),
            facts: Box::new(release_readiness_state.into()),
        }),
        "record_final_review" => Ok(ExecutionEvent::FinalReviewRecorded {
            final_review_record_id: json_string(state, "current_final_review_record_id"),
            dispatch_id: json_string(state, "current_final_review_dispatch_id"),
            facts: Box::new(final_review_state.into()),
        }),
        "record_qa" => Ok(ExecutionEvent::QaRecorded {
            qa_record_id: json_string(state, "current_qa_record_id"),
            facts: Box::new(qa_state.into()),
        }),
        "repair_review_state" => Ok(match follow_up {
            Some(follow_up) => ExecutionEvent::RepairFollowUpSet {
                follow_up,
                facts: Box::new(repair_state.into()),
            },
            None => ExecutionEvent::RepairFollowUpCleared {
                facts: Box::new(repair_state.into()),
            },
        }),
        "gate_review" | "advance_late_stage" => {
            if strategy_fingerprint.is_some() || strategy_kind.is_some() {
                Ok(ExecutionEvent::StrategyCheckpointRecorded {
                    checkpoint_kind: strategy_kind,
                    checkpoint_fingerprint: strategy_fingerprint,
                    facts: Box::new(strategy_state.into()),
                })
            } else {
                Ok(ExecutionEvent::StrategyCheckpointCleared {
                    facts: Box::new(strategy_state.into()),
                })
            }
        }
        "authoritative_state_persist" => Ok(ExecutionEvent::PreflightBootstrapRecorded {
            facts: Box::new(
                collect_changed_authoritative_fields(current_state, state, RUN_METADATA_FIELDS)
                    .into(),
            ),
        }),
        "worktree_lease_index_update" => Ok(ExecutionEvent::WorktreeLeaseIndexUpdated {
            facts: Box::new(
                collect_changed_authoritative_fields(current_state, state, RUN_METADATA_FIELDS)
                    .into(),
            ),
        }),
        "preflight_bootstrap" => Ok(ExecutionEvent::PreflightBootstrapRecorded {
            facts: Box::new(
                collect_changed_authoritative_fields(current_state, state, RUN_METADATA_FIELDS)
                    .into(),
            ),
        }),
        "record_contract" => Ok(ExecutionEvent::ContractRecorded {
            facts: Box::new(
                collect_changed_authoritative_fields(current_state, state, RUN_METADATA_FIELDS)
                    .into(),
            ),
        }),
        "record_evaluation" => Ok(ExecutionEvent::EvaluationRecorded {
            facts: Box::new(
                collect_changed_authoritative_fields(current_state, state, RUN_METADATA_FIELDS)
                    .into(),
            ),
        }),
        "record_handoff" => Ok(ExecutionEvent::HandoffRecorded {
            facts: Box::new(
                collect_changed_authoritative_fields(current_state, state, HANDOFF_FIELDS).into(),
            ),
        }),
        other => Err(JsonFailure::new(
            FailureClass::PartialAuthoritativeMutation,
            format!(
                "Event-log append for command `{other}` is blocked until it is mapped to an explicit typed execution event."
            ),
        )),
    }
}

fn collect_authoritative_fields(state: &Value, fields: &[&str]) -> AuthoritativeFactBuilder {
    fields
        .iter()
        .filter(|field| authoritative_event_field_allowed(field))
        .filter_map(|field| {
            state
                .get(*field)
                .cloned()
                .map(|value| ((*field).to_owned(), value))
        })
        .collect()
}

fn collect_changed_authoritative_fields(
    before: &Value,
    after: &Value,
    fields: &[&str],
) -> AuthoritativeFactBuilder {
    fields
        .iter()
        .filter(|field| authoritative_event_field_allowed(field))
        .filter_map(|field| {
            changed_authoritative_field_value(before, after, field)
                .map(|value| ((*field).to_owned(), value))
        })
        .collect()
}

fn changed_authoritative_field_value(before: &Value, after: &Value, field: &str) -> Option<Value> {
    let before_value = before.get(field);
    let after_value = after.get(field);
    if before_value == after_value {
        return None;
    }
    if record_map_delta_field(field) {
        return Some(changed_record_map_entries(before_value, after_value));
    }
    Some(after_value.cloned().unwrap_or(Value::Null))
}

fn changed_record_map_entries(before: Option<&Value>, after: Option<&Value>) -> Value {
    let before_entries = before.and_then(Value::as_object);
    let after_entries = after.and_then(Value::as_object);
    let mut changed = serde_json::Map::new();

    if let Some(after_entries) = after_entries {
        for (key, after_value) in after_entries {
            if before_entries.and_then(|entries| entries.get(key)) != Some(after_value) {
                changed.insert(key.clone(), after_value.clone());
            }
        }
    }
    if let Some(before_entries) = before_entries {
        for key in before_entries.keys() {
            if after_entries.is_none_or(|entries| !entries.contains_key(key)) {
                changed.insert(key.clone(), Value::Null);
            }
        }
    }

    Value::Object(changed)
}

fn authoritative_event_field_allowed(field: &str) -> bool {
    AUTHORITATIVE_EVENT_FIELDS.contains(&field)
}

fn dispatch_scope_from_state(state: &Value) -> Option<String> {
    if json_string(state, "current_final_review_dispatch_id").is_some() {
        return Some(String::from("final_review"));
    }
    if json_string(state, "current_task_review_dispatch_id").is_some() {
        return Some(String::from("task"));
    }
    None
}

fn current_dispatch_id_from_state(state: &Value) -> Option<String> {
    json_string(state, "current_task_review_dispatch_id")
        .or_else(|| json_string(state, "current_final_review_dispatch_id"))
}

fn newest_current_task_from_state(state: &Value) -> Option<u32> {
    let records = state.get("current_task_closure_records")?.as_object()?;
    records
        .values()
        .filter_map(|entry| {
            entry
                .get("task")
                .and_then(Value::as_u64)
                .and_then(|task| u32::try_from(task).ok())
        })
        .max()
}

fn newest_current_task_closure_id_from_state(state: &Value) -> Option<String> {
    let records = state.get("current_task_closure_records")?.as_object()?;
    records
        .values()
        .filter_map(|entry| {
            entry
                .get("closure_record_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .next_back()
        .map(ToOwned::to_owned)
}

fn migration_parity_projection(state: &Value) -> Value {
    let mut projection = serde_json::Map::new();
    for key in [
        "current_task_closure_records",
        "task_closure_record_history",
        "task_closure_negative_result_records",
        "event_completed_steps",
        "execution_evidence_attempts",
        "current_branch_closure_id",
        "current_branch_closure_reviewed_state_id",
        "current_branch_closure_contract_identity",
        "branch_closure_records",
        "superseded_branch_closure_ids",
        "current_release_readiness_record_id",
        "current_release_readiness_result",
        "release_readiness_record_history",
        "current_final_review_record_id",
        "current_final_review_result",
        "current_final_review_branch_closure_id",
        "final_review_record_history",
        "current_qa_record_id",
        "current_qa_result",
        "current_qa_branch_closure_id",
        "browser_qa_record_history",
        "current_task_review_dispatch_id",
        "current_final_review_dispatch_id",
        "strategy_review_dispatch_lineage",
        "strategy_review_dispatch_lineage_history",
        "final_review_dispatch_lineage",
        "final_review_dispatch_lineage_history",
        "review_state_repair_follow_up",
        "strategy_checkpoints",
        "strategy_state",
        "strategy_checkpoint_kind",
        "last_strategy_checkpoint_fingerprint",
        "strategy_reset_required",
        "strategy_cycle_break_task",
        "strategy_cycle_break_step",
        "strategy_cycle_break_checkpoint_fingerprint",
        "finish_review_gate_pass_branch_closure_id",
        "harness_phase",
    ] {
        let value = state
            .get(key)
            .filter(|value| !value.is_null())
            .cloned()
            .unwrap_or_else(|| migration_projection_default_value(key));
        let value = if key == "event_completed_steps" {
            normalize_event_completed_steps_projection(&value)
        } else {
            value
        };
        projection.insert(key.to_owned(), value);
    }
    Value::Object(projection)
}

fn normalize_event_completed_steps_projection(value: &Value) -> Value {
    let Some(entries) = value.as_object() else {
        return Value::Object(serde_json::Map::new());
    };
    let mut normalized = serde_json::Map::new();
    for (key, entry) in entries {
        if let Some((task, step)) = legacy_completed_step_from_entry(key, entry) {
            normalized.insert(
                format!("{task}.{step}"),
                serde_json::json!({
                    "task": task,
                    "step": step,
                }),
            );
        }
    }
    Value::Object(normalized)
}

fn migration_projection_default_value(field: &str) -> Value {
    match field {
        "current_task_closure_records"
        | "task_closure_record_history"
        | "task_closure_negative_result_records"
        | "task_closure_negative_result_history"
        | "event_completed_steps"
        | "branch_closure_records"
        | "release_readiness_record_history"
        | "final_review_record_history"
        | "browser_qa_record_history"
        | "strategy_review_dispatch_lineage"
        | "strategy_review_dispatch_lineage_history"
        | "final_review_dispatch_lineage"
        | "final_review_dispatch_lineage_history" => Value::Object(serde_json::Map::new()),
        "superseded_task_closure_ids"
        | "superseded_branch_closure_ids"
        | "strategy_checkpoints"
        | "execution_evidence_attempts" => Value::Array(Vec::new()),
        "handoff_required" | "strategy_reset_required" => Value::Bool(false),
        _ => Value::Null,
    }
}

fn migration_route_parity_projection(
    state: &Value,
    runtime: Option<&ExecutionRuntime>,
    state_path: &Path,
) -> Result<Value, JsonFailure> {
    if let Some(runtime) = runtime {
        return migration_route_parity_projection_from_router(state, runtime, state_path);
    }
    let mut projection = serde_json::Map::new();
    for key in ["phase", "phase_detail", "next_action"] {
        if let Some(value) = route_string(state, key) {
            projection.insert(key.to_owned(), Value::String(value));
        }
    }
    if let Some(command) = route_string(state, "recommended_command") {
        projection.insert(
            String::from("recommended_command_shape"),
            Value::String(command_shape(Some(command.as_str()))),
        );
    }
    if let Some(command) = state
        .get("next_public_action")
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
    {
        projection.insert(
            String::from("next_public_action_shape"),
            Value::String(command_shape(Some(command))),
        );
    }
    if projection.is_empty() {
        projection.insert(
            String::from("phase"),
            Value::String(legacy_route_phase_from_authority(state)),
        );
        projection.insert(
            String::from("phase_detail"),
            Value::String(legacy_route_phase_detail_from_authority(state)),
        );
        projection.insert(
            String::from("next_action_shape"),
            Value::String(legacy_route_next_action_shape_from_authority(state)),
        );
    }
    Ok(Value::Object(projection))
}

fn migration_route_parity_projection_from_router(
    state: &Value,
    runtime: &ExecutionRuntime,
    state_path: &Path,
) -> Result<Value, JsonFailure> {
    let plan_path = json_string(state, "source_plan_path")
        .or_else(|| infer_unique_approved_plan_path(runtime))
        .ok_or_else(|| {
            JsonFailure::new(
                FailureClass::BlockedRuntimeBug,
                "blocked_runtime_bug: event-log migration route parity requires source_plan_path or exactly one approved plan in the runtime repo.",
            )
        })?;
    let _candidate_payload_guard = enter_in_flight_migration(state_path, state);
    let context = load_execution_context_without_authority_overlay(runtime, Path::new(&plan_path))?;
    let authoritative_state = AuthoritativeTransitionState::from_reduced_event_payload(
        state_path.to_path_buf(),
        state.clone(),
    )?;
    let runtime_state = reduce_event_authority_for_migration_parity(EventAuthoritySnapshot {
        context: &context,
        event_authority_state: Some(&authoritative_state),
        semantic_workspace: semantic_workspace_snapshot(&context)?,
    })?;
    let route_decision = route_runtime_state(&runtime_state, false);
    let mut projection = serde_json::Map::new();
    projection.insert(
        String::from("state_kind"),
        Value::String(route_decision.state_kind),
    );
    projection.insert(String::from("phase"), Value::String(route_decision.phase));
    projection.insert(
        String::from("phase_detail"),
        Value::String(route_decision.phase_detail),
    );
    projection.insert(
        String::from("review_state_status"),
        Value::String(route_decision.review_state_status),
    );
    projection.insert(
        String::from("next_action"),
        Value::String(route_decision.next_action),
    );
    if let Some(command) = route_decision.recommended_command {
        projection.insert(String::from("recommended_command"), Value::String(command));
    }
    if let Some(required_follow_up) = route_decision.required_follow_up {
        projection.insert(
            String::from("required_follow_up"),
            Value::String(required_follow_up),
        );
    }
    if let Some(next_public_action) = route_decision.next_public_action {
        projection.insert(
            String::from("next_public_action"),
            serde_json::to_value(next_public_action).map_err(|error| {
                JsonFailure::new(
                    FailureClass::BlockedRuntimeBug,
                    format!(
                        "blocked_runtime_bug: event-log migration route parity could not serialize next_public_action: {error}"
                    ),
                )
            })?,
        );
    }
    projection.insert(
        String::from("blocking_reason_codes"),
        serde_json::to_value(route_decision.blocking_reason_codes).map_err(|error| {
            JsonFailure::new(
                FailureClass::BlockedRuntimeBug,
                format!(
                    "blocked_runtime_bug: event-log migration route parity could not serialize blocking_reason_codes: {error}"
                ),
            )
        })?,
    );
    Ok(Value::Object(projection))
}

fn legacy_route_phase_from_authority(state: &Value) -> String {
    match json_string(state, "harness_phase").as_deref() {
        Some("ready_for_branch_completion") => String::from("ready_for_branch_completion"),
        Some("document_release_pending") => String::from("document_release_pending"),
        Some("final_review_pending") => String::from("final_review_pending"),
        Some("qa_pending") => String::from("qa_pending"),
        Some("executing") | Some("repairing") => String::from("executing"),
        Some("execution_preflight") => String::from("execution_preflight"),
        _ => String::from("implementation_handoff"),
    }
}

fn legacy_route_phase_detail_from_authority(state: &Value) -> String {
    if state
        .get("current_open_step_state")
        .is_some_and(|value| !value.is_null())
    {
        return String::from("execution_in_progress");
    }
    if newest_current_task_from_state(state).is_some()
        && json_string(state, "current_branch_closure_id").is_none()
    {
        return String::from("branch_closure_recording_required_for_release_readiness");
    }
    match json_string(state, "harness_phase").as_deref() {
        Some("ready_for_branch_completion") => String::from("finish_completion_gate_ready"),
        Some("document_release_pending") => String::from("release_readiness_recording_ready"),
        Some("final_review_pending") => String::from("final_review_dispatch_required"),
        Some("qa_pending") => String::from("qa_recording_required"),
        Some("execution_preflight") => String::from("execution_preflight_required"),
        Some("executing") | Some("repairing") => String::from("execution_in_progress"),
        _ => String::from("implementation_handoff_required"),
    }
}

fn legacy_route_next_action_shape_from_authority(state: &Value) -> String {
    command_shape(
        match legacy_route_phase_detail_from_authority(state).as_str() {
            "branch_closure_recording_required_for_release_readiness"
            | "release_readiness_recording_ready"
            | "final_review_recording_ready"
            | "qa_recording_required"
            | "finish_completion_gate_ready" => {
                Some("featureforge plan execution advance-late-stage")
            }
            "execution_in_progress" => Some("featureforge plan execution complete"),
            "execution_preflight_required" => Some("featureforge plan execution begin"),
            _ => None,
        },
    )
}

fn route_string(state: &Value, key: &str) -> Option<String> {
    state
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn infer_unique_approved_plan_path(runtime: &ExecutionRuntime) -> Option<String> {
    let mut stack = vec![runtime.repo_root.join("docs/featureforge/plans")];
    let mut candidates = Vec::new();
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if !metadata.is_file()
                || path.extension().and_then(|value| value.to_str()) != Some("md")
            {
                continue;
            }
            let Ok(source) = fs::read_to_string(&path) else {
                continue;
            };
            if !source.contains("**Workflow State:** Engineering Approved") {
                continue;
            }
            let rel = path
                .strip_prefix(&runtime.repo_root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            candidates.push(rel);
            if candidates.len() > 1 {
                return None;
            }
        }
    }
    candidates.pop()
}

fn command_shape(command: Option<&str>) -> String {
    let Some(command) = command.map(str::trim).filter(|value| !value.is_empty()) else {
        return String::from("none");
    };
    for token in [
        "workflow operator",
        "plan execution status",
        "plan execution repair-review-state",
        "plan execution begin",
        "plan execution complete",
        "plan execution reopen",
        "plan execution transfer",
        "plan execution close-current-task",
        "plan execution advance-late-stage",
    ] {
        if command.contains(token) {
            return token.to_owned();
        }
    }
    String::from("other")
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RecordStatusTransitionExpectation {
    record_family: String,
    record_id: Option<String>,
    record_status: Option<String>,
    record_sequence: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TaskClosureReplayExpectation {
    task: Option<u32>,
    closure_record_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CompletedStepReplayExpectation {
    task: u32,
    step: u32,
}

fn record_id_from_migration_entry(entry_key: &str, entry: &Value) -> Option<String> {
    entry
        .get("record_id")
        .or_else(|| entry.get("closure_record_id"))
        .or_else(|| entry.get("branch_closure_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let trimmed = entry_key.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        })
}

fn collect_expected_record_status_transitions(
    state: &Value,
    field: &str,
    record_family: &str,
    default_record_status: Option<&str>,
    out: &mut BTreeSet<RecordStatusTransitionExpectation>,
) {
    let Some(entries) = state.get(field).and_then(Value::as_object) else {
        return;
    };
    for (entry_key, entry) in entries {
        let record_status = entry
            .get("record_status")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| default_record_status.map(str::to_owned));
        let record_sequence = Some(
            entry
                .get("record_sequence")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        );
        if field == "task_closure_record_history"
            && record_family == "task_closure_record"
            && record_status.as_deref() == Some("current")
        {
            continue;
        }
        out.insert(RecordStatusTransitionExpectation {
            record_family: record_family.to_owned(),
            record_id: record_id_from_migration_entry(entry_key, entry),
            record_status,
            record_sequence,
        });
    }
}

fn expected_record_status_transitions_from_legacy(
    legacy_state: &Value,
) -> BTreeSet<RecordStatusTransitionExpectation> {
    let mut expected = BTreeSet::new();
    collect_expected_record_status_transitions(
        legacy_state,
        "current_task_closure_records",
        "task_closure_record",
        Some("current"),
        &mut expected,
    );
    collect_expected_record_status_transitions(
        legacy_state,
        "task_closure_record_history",
        "task_closure_record",
        None,
        &mut expected,
    );
    collect_expected_record_status_transitions(
        legacy_state,
        "branch_closure_records",
        "branch_closure_record",
        None,
        &mut expected,
    );
    collect_expected_record_status_transitions(
        legacy_state,
        "release_readiness_record_history",
        "release_readiness_record",
        None,
        &mut expected,
    );
    collect_expected_record_status_transitions(
        legacy_state,
        "final_review_record_history",
        "final_review_record",
        None,
        &mut expected,
    );
    collect_expected_record_status_transitions(
        legacy_state,
        "browser_qa_record_history",
        "browser_qa_record",
        None,
        &mut expected,
    );
    expected
}

fn observed_record_status_transitions(
    migrated_events: &[ExecutionEventEnvelope],
) -> BTreeSet<RecordStatusTransitionExpectation> {
    migrated_events
        .iter()
        .filter_map(|event| match &event.payload {
            ExecutionEvent::RecordStatusTransition {
                record_family,
                record_id,
                record_status,
                record_sequence,
            } => Some(RecordStatusTransitionExpectation {
                record_family: record_family.clone(),
                record_id: record_id.clone(),
                record_status: record_status.clone(),
                record_sequence: *record_sequence,
            }),
            _ => None,
        })
        .collect()
}

fn expected_current_task_closure_replay(
    legacy_state: &Value,
) -> BTreeSet<TaskClosureReplayExpectation> {
    let mut expected = BTreeSet::new();
    let Some(entries) = legacy_state
        .get("current_task_closure_records")
        .and_then(Value::as_object)
    else {
        return expected;
    };
    for (entry_key, entry) in entries {
        let task = entry
            .get("task")
            .and_then(Value::as_u64)
            .and_then(|raw| u32::try_from(raw).ok());
        expected.insert(TaskClosureReplayExpectation {
            task,
            closure_record_id: entry
                .get("closure_record_id")
                .or_else(|| entry.get("record_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| {
                    let trimmed = entry_key.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_owned())
                }),
        });
    }
    expected
}

fn observed_task_closure_replay(
    migrated_events: &[ExecutionEventEnvelope],
) -> BTreeSet<TaskClosureReplayExpectation> {
    migrated_events
        .iter()
        .filter_map(|event| match &event.payload {
            ExecutionEvent::TaskClosureRecorded {
                task,
                closure_record_id,
                ..
            } => Some(TaskClosureReplayExpectation {
                task: *task,
                closure_record_id: closure_record_id.clone(),
            }),
            _ => None,
        })
        .collect()
}

fn expected_completed_step_replay(
    legacy_state: &Value,
) -> BTreeSet<CompletedStepReplayExpectation> {
    legacy_completed_step_events(legacy_state)
        .into_iter()
        .map(|(task, step)| CompletedStepReplayExpectation { task, step })
        .collect()
}

fn observed_completed_step_replay(
    migrated_events: &[ExecutionEventEnvelope],
) -> BTreeSet<CompletedStepReplayExpectation> {
    migrated_events
        .iter()
        .filter_map(|event| match &event.payload {
            ExecutionEvent::Complete {
                task: Some(task),
                step: Some(step),
                ..
            } => Some(CompletedStepReplayExpectation {
                task: *task,
                step: *step,
            }),
            _ => None,
        })
        .collect()
}

fn validate_migration_replay_coverage(
    legacy_state: &Value,
    migrated_events: &[ExecutionEventEnvelope],
) -> Result<(), JsonFailure> {
    let expected_transitions = expected_record_status_transitions_from_legacy(legacy_state);
    let observed_transitions = observed_record_status_transitions(migrated_events);
    let missing_transitions = expected_transitions
        .difference(&observed_transitions)
        .cloned()
        .collect::<Vec<_>>();
    if !missing_transitions.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::BlockedRuntimeBug,
            format!(
                "blocked_runtime_bug: event-log migration replay omitted expected record-status transitions: {missing_transitions:?}"
            ),
        ));
    }

    let expected_task_closure_replay = expected_current_task_closure_replay(legacy_state);
    let observed_task_closure_replay = observed_task_closure_replay(migrated_events);
    let missing_task_closure_replay = expected_task_closure_replay
        .difference(&observed_task_closure_replay)
        .cloned()
        .collect::<Vec<_>>();
    if !missing_task_closure_replay.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::BlockedRuntimeBug,
            format!(
                "blocked_runtime_bug: event-log migration replay omitted current task-closure events: {missing_task_closure_replay:?}"
            ),
        ));
    }

    let expected_completed_step_replay = expected_completed_step_replay(legacy_state);
    let observed_completed_step_replay = observed_completed_step_replay(migrated_events);
    let missing_completed_step_replay = expected_completed_step_replay
        .difference(&observed_completed_step_replay)
        .copied()
        .collect::<Vec<_>>();
    if !missing_completed_step_replay.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::BlockedRuntimeBug,
            format!(
                "blocked_runtime_bug: event-log migration replay omitted completed-step events: {missing_completed_step_replay:?}"
            ),
        ));
    }

    Ok(())
}

fn validate_migration_parity(
    legacy_state: &Value,
    migrated_events: &[ExecutionEventEnvelope],
    runtime: Option<&ExecutionRuntime>,
    legacy_state_path: &Path,
) -> Result<(), JsonFailure> {
    let reduced_state = match reduce_events_to_state(migrated_events) {
        Some(reduced_state) => reduced_state,
        None => {
            let legacy_route_projection =
                migration_route_parity_projection(legacy_state, runtime, legacy_state_path)?;
            if !legacy_route_projection
                .as_object()
                .is_none_or(serde_json::Map::is_empty)
            {
                return Err(JsonFailure::new(
                    FailureClass::BlockedRuntimeBug,
                    format!(
                        "blocked_runtime_bug: event-log migration route parity mismatch.\nlegacy={legacy_route_projection}\nreduced=null"
                    ),
                ));
            }
            return Err(JsonFailure::new(
                FailureClass::BlockedRuntimeBug,
                "blocked_runtime_bug: event-log migration parity failed because reduced state is empty.",
            ));
        }
    };
    let legacy_projection = migration_parity_projection(legacy_state);
    let reduced_projection = migration_parity_projection(&reduced_state);
    if legacy_projection != reduced_projection {
        return Err(JsonFailure::new(
            FailureClass::BlockedRuntimeBug,
            format!(
                "blocked_runtime_bug: event-log migration parity mismatch for authoritative closure/dispatch/checkpoint fields.\nlegacy={legacy_projection}\nreduced={reduced_projection}"
            ),
        ));
    }
    let legacy_route_projection =
        migration_route_parity_projection(legacy_state, runtime, legacy_state_path)?;
    let reduced_route_projection =
        migration_route_parity_projection(&reduced_state, runtime, legacy_state_path)?;
    if legacy_route_projection != reduced_route_projection {
        return Err(JsonFailure::new(
            FailureClass::BlockedRuntimeBug,
            format!(
                "blocked_runtime_bug: event-log migration route parity mismatch.\nlegacy={legacy_route_projection}\nreduced={reduced_route_projection}"
            ),
        ));
    }
    validate_migration_replay_coverage(legacy_state, migrated_events)?;
    Ok(())
}

fn branch_context_from_runtime(runtime: &ExecutionRuntime) -> BranchContext {
    BranchContext {
        repo_slug: runtime.repo_slug.clone(),
        branch_name: runtime.branch_name.clone(),
        safe_branch: runtime.safe_branch.clone(),
    }
}

fn branch_context_from_state_path(state_path: &Path, state_payload: &Value) -> BranchContext {
    let mut repo_slug =
        json_string(state_payload, "repo_slug").unwrap_or_else(|| String::from("unknown-repo"));
    let mut safe_branch = String::from("unknown-branch");
    let mut components = state_path.components().peekable();
    while let Some(component) = components.next() {
        if component == Component::Normal("projects".as_ref()) {
            if let Some(Component::Normal(repo)) = components.next() {
                repo_slug = repo.to_string_lossy().to_string();
            }
            continue;
        }
        if component == Component::Normal("branches".as_ref())
            && let Some(Component::Normal(branch)) = components.next()
        {
            safe_branch = branch.to_string_lossy().to_string();
        }
    }
    let branch_name = json_string(state_payload, "branch_name").unwrap_or_else(|| {
        let normalized = normalize_identifier_token(&safe_branch);
        if normalized.is_empty() {
            safe_branch.clone()
        } else {
            normalized
        }
    });
    BranchContext {
        repo_slug,
        branch_name,
        safe_branch,
    }
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn json_string_from_path(value: &Value, path: &[&str]) -> Option<String> {
    let mut cursor = value;
    for segment in path {
        cursor = cursor.get(*segment)?;
    }
    cursor
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn json_u32(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|raw| u32::try_from(raw).ok())
}

fn json_u32_from_path(value: &Value, path: &[&str]) -> Option<u32> {
    let mut cursor = value;
    for segment in path {
        cursor = cursor.get(*segment)?;
    }
    cursor.as_u64().and_then(|raw| u32::try_from(raw).ok())
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use serde_json::{Value, json};

    use super::{
        AuthoritativeFactBuilder, BranchContext, EventEnvelopeMetadata, ExecutionEvent,
        StepEventFacts, ensure_event_log_migrated_from_legacy_state_path,
        event_from_command_authoritative_delta, events_log_path_for_state_path,
        legacy_state_backup_path_for_state_path, load_event_log,
        load_reduced_authoritative_state_for_state_path, next_event_envelope, validate_event_log,
        validate_migration_parity,
    };
    use crate::diagnostics::FailureClass;

    fn minimal_legacy_state() -> Value {
        json!({
            "schema_version": 1,
            "harness_phase": "executing",
            "source_plan_path": "docs/featureforge/plans/example.md",
            "source_plan_revision": 1,
            "execution_run_id": "run-1",
            "repo_slug": "repo",
            "branch_name": "feature-branch",
            "current_task_closure_records": {},
            "task_closure_record_history": {},
            "task_closure_negative_result_records": {},
            "branch_closure_records": {},
            "superseded_branch_closure_ids": [],
            "release_readiness_record_history": {},
            "final_review_record_history": {},
            "browser_qa_record_history": {},
            "strategy_review_dispatch_lineage": {},
            "strategy_review_dispatch_lineage_history": {},
            "final_review_dispatch_lineage": {},
            "strategy_checkpoint_kind": "none"
        })
    }

    #[test]
    fn legacy_state_auto_migrates_to_event_log_with_backup() {
        let tempdir = tempfile::tempdir().expect("tempdir should be creatable");
        let state_path = tempdir.path().join("state.json");
        let legacy_state = minimal_legacy_state();
        fs::write(
            &state_path,
            serde_json::to_string_pretty(&legacy_state)
                .expect("legacy state fixture should serialize"),
        )
        .expect("legacy state fixture should write");

        ensure_event_log_migrated_from_legacy_state_path(&state_path, None, None)
            .expect("migration should succeed for well-formed legacy state");

        let events_path = events_log_path_for_state_path(&state_path);
        let backup_path = legacy_state_backup_path_for_state_path(&state_path);
        assert!(
            events_path.exists(),
            "migration should publish authoritative events.jsonl"
        );
        assert!(
            backup_path.exists(),
            "migration should preserve state.legacy.json backup"
        );

        let events = load_event_log(&events_path).expect("event log should parse");
        validate_event_log(&events).expect("migrated event log should validate");
        assert!(
            matches!(
                events.first().map(|event| &event.payload),
                Some(ExecutionEvent::MigrationImported { .. })
            ),
            "first migrated event should preserve migration_imported marker"
        );
        assert!(
            events.iter().any(|event| matches!(
                event.payload,
                ExecutionEvent::TaskClosureRecorded { .. }
                    | ExecutionEvent::BranchClosureRecorded { .. }
                    | ExecutionEvent::RepairFollowUpCleared { .. }
                    | ExecutionEvent::StrategyCheckpointRecorded { .. }
                    | ExecutionEvent::StrategyCheckpointCleared { .. }
            )),
            "migrated logs should replay typed authoritative facts instead of publishing a state snapshot"
        );

        let reduced = load_reduced_authoritative_state_for_state_path(&state_path)
            .expect("reduced authoritative state should load")
            .expect("reduced authoritative state should be present");
        assert_eq!(
            super::migration_parity_projection(&reduced),
            super::migration_parity_projection(&legacy_state),
            "event-log reduction should preserve legacy critical authoritative truth"
        );
    }

    #[test]
    fn legacy_migration_excludes_explicit_projection_summary_states_from_authority() {
        let tempdir = tempfile::tempdir().expect("tempdir should be creatable");
        let state_path = tempdir.path().join("state.json");
        let legacy_state = json!({
            "schema_version": 1,
            "harness_phase": "final_review_pending",
            "source_plan_path": "docs/featureforge/plans/example.md",
            "source_plan_revision": 1,
            "execution_run_id": "run-1",
            "repo_slug": "repo",
            "branch_name": "feature-branch",
            "current_branch_closure_id": "branch-closure-1",
            "current_branch_closure_reviewed_state_id": "git_tree:abc123",
            "current_branch_closure_contract_identity": "branch-contract-1",
            "current_release_readiness_result": "ready",
            "release_docs_state": "stale",
            "last_release_docs_artifact_fingerprint": "aaaaaaaa",
            "final_review_state": "stale",
            "last_final_review_artifact_fingerprint": "bbbbbbbb",
            "browser_qa_state": "not_required",
            "last_browser_qa_artifact_fingerprint": "cccccccc",
            "current_task_closure_records": {},
            "task_closure_record_history": {},
            "task_closure_negative_result_records": {},
            "branch_closure_records": {},
            "superseded_branch_closure_ids": [],
            "release_readiness_record_history": {},
            "final_review_record_history": {},
            "browser_qa_record_history": {},
            "strategy_review_dispatch_lineage": {},
            "strategy_review_dispatch_lineage_history": {},
            "final_review_dispatch_lineage": {},
            "strategy_checkpoint_kind": "none"
        });
        fs::write(
            &state_path,
            serde_json::to_string_pretty(&legacy_state)
                .expect("legacy state fixture should serialize"),
        )
        .expect("legacy state fixture should write");

        ensure_event_log_migrated_from_legacy_state_path(&state_path, None, None)
            .expect("migration should succeed while excluding projection summaries");

        let reduced = load_reduced_authoritative_state_for_state_path(&state_path)
            .expect("reduced authoritative state should load")
            .expect("reduced authoritative state should be present");
        for omitted in [
            "release_docs_state",
            "last_release_docs_artifact_fingerprint",
            "final_review_state",
            "last_final_review_artifact_fingerprint",
            "browser_qa_state",
            "last_browser_qa_artifact_fingerprint",
        ] {
            assert!(
                reduced.get(omitted).is_none_or(Value::is_null),
                "event-log reduction must not preserve non-authoritative projection field `{omitted}`"
            );
        }
    }

    #[test]
    fn unknown_mutation_commands_fail_closed_until_mapped_to_explicit_events() {
        let state = minimal_legacy_state();
        let error = event_from_command_authoritative_delta(
            "unmapped_new_command",
            &json!(null),
            &state,
            "unit-test-unmapped-command",
            None,
        )
        .expect_err("unmapped commands must not silently fall back to snapshot authority");
        assert_eq!(
            error.error_class,
            FailureClass::PartialAuthoritativeMutation.as_str()
        );
        assert!(
            error
                .message
                .contains("mapped to an explicit typed execution event"),
            "unexpected error message: {}",
            error.message
        );
    }

    #[test]
    fn migration_failure_keeps_backup_and_avoids_partial_event_publish() {
        let tempdir = tempfile::tempdir().expect("tempdir should be creatable");
        let state_path = tempdir.path().join("state.json");
        let lock_path = state_path.with_file_name("events.lock");
        let legacy_state = minimal_legacy_state();
        fs::write(
            &state_path,
            serde_json::to_string_pretty(&legacy_state)
                .expect("legacy state fixture should serialize"),
        )
        .expect("legacy state fixture should write");
        fs::write(&lock_path, "occupied").expect("lock fixture should write");

        let error = ensure_event_log_migrated_from_legacy_state_path(&state_path, None, None)
            .expect_err("migration must fail closed when event-log writer lock is already held");
        assert_eq!(
            error.error_class,
            FailureClass::ConcurrentWriterConflict.as_str(),
            "lock conflict should surface as concurrent writer conflict"
        );

        let backup_path = legacy_state_backup_path_for_state_path(&state_path);
        let events_path = events_log_path_for_state_path(&state_path);
        assert!(
            backup_path.exists(),
            "failed migrations must still leave state.legacy.json backup in place"
        );
        assert!(
            !events_path.exists(),
            "failed migrations must not partially publish events.jsonl"
        );
    }

    #[test]
    fn migration_parity_failures_are_blocked_runtime_bug() {
        let legacy_state = minimal_legacy_state();
        let mismatched_state = json!({
            "schema_version": 1,
            "harness_phase": "ready_for_branch_completion"
        });
        let branch_context = BranchContext {
            repo_slug: String::from("repo"),
            branch_name: String::from("feature-branch"),
            safe_branch: String::from("feature-branch"),
        };
        let mut events = Vec::new();
        let migrated = next_event_envelope(
            &events,
            EventEnvelopeMetadata {
                command: String::from("migrate_legacy_authoritative_state"),
                plan_path: Some(String::from("docs/featureforge/plans/example.md")),
                plan_revision: Some(1),
                execution_run_id: Some(String::from("run-1")),
                branch_context: branch_context.clone(),
            },
            ExecutionEvent::MigrationImported {
                legacy_state_backup_path: String::from("state.legacy.json"),
            },
        )
        .expect("migration event should envelope");
        events.push(migrated);
        let mismatched_snapshot = next_event_envelope(
            &events,
            EventEnvelopeMetadata {
                command: String::from("migrate_legacy_authoritative_state"),
                plan_path: Some(String::from("docs/featureforge/plans/example.md")),
                plan_revision: Some(1),
                execution_run_id: Some(String::from("run-1")),
                branch_context,
            },
            ExecutionEvent::Note {
                task: None,
                step: None,
                note_state: Some(String::from("migration_parity_mismatch_fixture")),
                facts: Box::new(
                    [(
                        String::from("harness_phase"),
                        mismatched_state["harness_phase"].clone(),
                    )]
                    .into_iter()
                    .collect::<AuthoritativeFactBuilder>()
                    .into(),
                ),
            },
        )
        .expect("typed mismatch event should envelope");
        events.push(mismatched_snapshot);

        let error =
            validate_migration_parity(&legacy_state, &events, None, Path::new("state.json"))
                .expect_err("parity mismatch must fail closed");
        assert_eq!(
            error.error_class,
            FailureClass::BlockedRuntimeBug.as_str(),
            "parity mismatch should classify as BlockedRuntimeBug"
        );
        assert!(
            error.message.contains("blocked_runtime_bug"),
            "parity mismatch should explicitly surface blocked_runtime_bug, got {}",
            error.message
        );
    }

    #[test]
    fn migration_parity_failure_keeps_backup_and_does_not_publish_events() {
        let tempdir = tempfile::tempdir().expect("tempdir should be creatable");
        let state_path = tempdir.path().join("state.json");
        let backup_path = legacy_state_backup_path_for_state_path(&state_path);
        let events_path = events_log_path_for_state_path(&state_path);
        let legacy_state = minimal_legacy_state();
        let state_source = serde_json::to_string_pretty(&legacy_state)
            .expect("legacy state fixture should serialize");
        fs::write(&state_path, &state_source).expect("legacy state fixture should write");
        fs::write(&backup_path, &state_source)
            .expect("backup should write before parity validation");

        let mismatched_state = json!({
            "schema_version": 1,
            "harness_phase": "ready_for_branch_completion"
        });
        let branch_context = BranchContext {
            repo_slug: String::from("repo"),
            branch_name: String::from("feature-branch"),
            safe_branch: String::from("feature-branch"),
        };
        let mut staged_events = Vec::new();
        let migration_event = next_event_envelope(
            &staged_events,
            EventEnvelopeMetadata {
                command: String::from("migrate_legacy_authoritative_state"),
                plan_path: Some(String::from("docs/featureforge/plans/example.md")),
                plan_revision: Some(1),
                execution_run_id: Some(String::from("run-1")),
                branch_context: branch_context.clone(),
            },
            ExecutionEvent::MigrationImported {
                legacy_state_backup_path: backup_path.display().to_string(),
            },
        )
        .expect("migration event should envelope");
        staged_events.push(migration_event);
        let mismatched_refresh = next_event_envelope(
            &staged_events,
            EventEnvelopeMetadata {
                command: String::from("migrate_legacy_authoritative_state_import"),
                plan_path: Some(String::from("docs/featureforge/plans/example.md")),
                plan_revision: Some(1),
                execution_run_id: Some(String::from("run-1")),
                branch_context,
            },
            ExecutionEvent::Note {
                task: None,
                step: None,
                note_state: Some(String::from("migration_parity_mismatch_fixture")),
                facts: Box::new(
                    [(
                        String::from("harness_phase"),
                        mismatched_state["harness_phase"].clone(),
                    )]
                    .into_iter()
                    .collect::<AuthoritativeFactBuilder>()
                    .into(),
                ),
            },
        )
        .expect("typed mismatch event should envelope");
        staged_events.push(mismatched_refresh);

        validate_event_log(&staged_events).expect("staged events should validate structurally");
        let error =
            validate_migration_parity(&legacy_state, &staged_events, None, Path::new("state.json"))
                .expect_err("parity mismatch must fail before publish");
        assert_eq!(error.error_class, FailureClass::BlockedRuntimeBug.as_str());
        assert!(
            backup_path.exists(),
            "parity failure should leave state.legacy.json backup in place"
        );
        assert!(
            !events_path.exists(),
            "parity failure must not publish events.jsonl before validation succeeds"
        );
    }

    #[test]
    fn migration_route_parity_failure_rejects_lost_legacy_route_truth() {
        let mut legacy_state = minimal_legacy_state();
        let legacy_root = legacy_state
            .as_object_mut()
            .expect("legacy fixture should be an object");
        legacy_root.insert(String::from("phase"), Value::from("executing"));
        legacy_root.insert(
            String::from("phase_detail"),
            Value::from("execution_in_progress"),
        );
        legacy_root.insert(
            String::from("next_action"),
            Value::from("continue execution"),
        );
        legacy_root.insert(
            String::from("recommended_command"),
            Value::from("featureforge plan execution begin --plan docs/featureforge/plans/example.md --task 1 --step 1"),
        );
        let branch_context = BranchContext {
            repo_slug: String::from("repo"),
            branch_name: String::from("feature-branch"),
            safe_branch: String::from("feature-branch"),
        };
        let events = vec![
            next_event_envelope(
                &[],
                EventEnvelopeMetadata {
                    command: String::from("migrate_legacy_authoritative_state"),
                    plan_path: Some(String::from("docs/featureforge/plans/example.md")),
                    plan_revision: Some(1),
                    execution_run_id: Some(String::from("run-1")),
                    branch_context: branch_context.clone(),
                },
                ExecutionEvent::MigrationImported {
                    legacy_state_backup_path: String::from("state.legacy.json"),
                },
            )
            .expect("migration marker should envelope"),
            next_event_envelope(
                &[],
                EventEnvelopeMetadata {
                    command: String::from("migrate_legacy_authoritative_state_import"),
                    plan_path: Some(String::from("docs/featureforge/plans/example.md")),
                    plan_revision: Some(1),
                    execution_run_id: Some(String::from("run-1")),
                    branch_context,
                },
                ExecutionEvent::Begin {
                    task: None,
                    step: None,
                    facts: Box::<StepEventFacts>::default(),
                },
            )
            .expect("empty route-loss event should envelope"),
        ];

        let error =
            validate_migration_parity(&legacy_state, &events, None, Path::new("state.json"))
                .expect_err("lost legacy route truth must fail migration parity");
        assert_eq!(error.error_class, FailureClass::BlockedRuntimeBug.as_str());
        assert!(
            error
                .message
                .contains("event-log migration route parity mismatch"),
            "route parity failure should be explicit, got {}",
            error.message
        );
    }

    #[test]
    fn event_log_field_allowlist_excludes_public_routing_read_model_fields() {
        let state = json!({
            "harness_phase": "executing",
            "dependency_index_state": "healthy",
            "phase": "executing",
            "phase_detail": "execution_in_progress",
            "next_action": "continue execution",
            "recommended_command": "featureforge workflow operator --plan docs/featureforge/plans/example.md",
            "state_kind": "actionable_public_command",
            "next_public_action": {
                "command": "featureforge workflow operator --plan docs/featureforge/plans/example.md"
            },
            "blockers": [{"category": "workflow"}],
            "semantic_workspace_tree_id": "semantic_tree:abc",
            "raw_workspace_tree_id": "git_tree:def",
            "release_docs_state": "fresh",
            "last_release_docs_artifact_fingerprint": "aaaaaaaa",
            "final_review_state": "fresh",
            "last_final_review_artifact_fingerprint": "bbbbbbbb",
            "browser_qa_state": "fresh",
            "last_browser_qa_artifact_fingerprint": "cccccccc",
        });

        let refreshed =
            super::collect_authoritative_fields(&state, super::AUTHORITATIVE_EVENT_FIELDS);

        assert_eq!(
            refreshed.get("harness_phase"),
            Some(&Value::from("executing"))
        );
        assert_eq!(
            refreshed.get("dependency_index_state"),
            Some(&Value::from("healthy"))
        );
        for omitted in [
            "phase_detail",
            "next_action",
            "recommended_command",
            "state_kind",
            "next_public_action",
            "blockers",
            "semantic_workspace_tree_id",
            "raw_workspace_tree_id",
            "release_docs_state",
            "last_release_docs_artifact_fingerprint",
            "final_review_state",
            "last_final_review_artifact_fingerprint",
            "browser_qa_state",
            "last_browser_qa_artifact_fingerprint",
        ] {
            assert!(
                !refreshed.contains_field(omitted),
                "authority refresh events must not persist public read-model field `{omitted}`"
            );
        }
    }

    #[test]
    fn event_log_validation_rejects_non_authoritative_projection_facts() {
        assert!(
            !AuthoritativeFactBuilder::default()
                .set_field_value("release_docs_state", Value::from("fresh")),
            "projection fields must not have authoritative event fact fields"
        );
        let source = json!({
            "release_docs_state": "fresh"
        });
        assert!(
            serde_json::from_value::<StepEventFacts>(source).is_err(),
            "projection facts must be rejected by typed event-log payload deserialization"
        );
    }

    #[test]
    fn changed_record_map_facts_are_per_record_deltas() {
        let before = json!({
            "current_task_closure_records": {
                "task-1": {
                    "record_id": "task-1",
                    "task": 1,
                    "record_status": "current"
                },
                "task-2": {
                    "record_id": "task-2",
                    "task": 2,
                    "record_status": "current"
                }
            }
        });
        let after = json!({
            "current_task_closure_records": {
                "task-1": {
                    "record_id": "task-1",
                    "task": 1,
                    "record_status": "current"
                },
                "task-2": {
                    "record_id": "task-2",
                    "task": 2,
                    "record_status": "superseded"
                },
                "task-3": {
                    "record_id": "task-3",
                    "task": 3,
                    "record_status": "current"
                }
            }
        });

        let facts = super::collect_changed_authoritative_fields(
            &before,
            &after,
            &["current_task_closure_records"],
        );
        assert_eq!(facts.populated_field_names().len(), 1);
        let delta = facts
            .get("current_task_closure_records")
            .and_then(Value::as_object)
            .expect("record-map fact should be an object delta");
        assert!(
            !delta.contains_key("task-1"),
            "unchanged record entries must not be persisted in event facts"
        );
        assert!(delta.contains_key("task-2"));
        assert!(delta.contains_key("task-3"));

        let mut reduced = before.clone();
        super::apply_authoritative_fields(
            &mut reduced,
            &[(
                String::from("current_task_closure_records"),
                Value::Object(delta.clone()),
            )]
            .into_iter()
            .collect(),
        );
        assert_eq!(reduced, after);
    }

    #[test]
    fn record_status_transition_events_reduce_into_authoritative_record_maps() {
        let branch_context = BranchContext {
            repo_slug: String::from("repo"),
            branch_name: String::from("feature-branch"),
            safe_branch: String::from("feature-branch"),
        };
        let event = next_event_envelope(
            &[],
            EventEnvelopeMetadata {
                command: String::from("record_status_transition"),
                plan_path: Some(String::from("docs/featureforge/plans/example.md")),
                plan_revision: Some(1),
                execution_run_id: Some(String::from("run-1")),
                branch_context,
            },
            ExecutionEvent::RecordStatusTransition {
                record_family: String::from("branch_closure_record"),
                record_id: Some(String::from("branch-closure-1")),
                record_status: Some(String::from("stale_unreviewed")),
                record_sequence: Some(42),
            },
        )
        .expect("record-status transition event should envelope");

        let reduced = super::reduce_events_to_state(&[event])
            .expect("record-status transition should reduce to authoritative state");
        let record = &reduced["branch_closure_records"]["branch-closure-1"];
        assert_eq!(record["record_id"], "branch-closure-1");
        assert_eq!(record["record_status"], "stale_unreviewed");
        assert_eq!(record["record_sequence"], 42);
    }

    #[test]
    fn migration_replay_preserves_record_status_transitions_explicitly() {
        let tempdir = tempfile::tempdir().expect("tempdir should be creatable");
        let state_path = tempdir.path().join("state.json");
        let legacy_state = json!({
            "schema_version": 1,
            "harness_phase": "executing",
            "source_plan_path": "docs/featureforge/plans/example.md",
            "source_plan_revision": 1,
            "execution_run_id": "run-1",
            "repo_slug": "repo",
            "branch_name": "feature-branch",
            "current_task_closure_records": {
                "task-current": {
                    "record_id": "task-current",
                    "closure_record_id": "task-current",
                    "task": 1,
                    "record_sequence": 3,
                    "record_status": "current"
                }
            },
            "task_closure_record_history": {
                "task-historical": {
                    "record_id": "task-historical",
                    "closure_record_id": "task-historical",
                    "task": 1,
                    "record_sequence": 2,
                    "record_status": "historical"
                }
            },
            "task_closure_negative_result_records": {},
            "branch_closure_records": {
                "branch-stale": {
                    "record_id": "branch-stale",
                    "branch_closure_id": "branch-stale",
                    "record_sequence": 4,
                    "record_status": "stale_unreviewed"
                }
            },
            "superseded_branch_closure_ids": [],
            "release_readiness_record_history": {
                "release-superseded": {
                    "record_id": "release-superseded",
                    "record_sequence": 5,
                    "record_status": "superseded"
                }
            },
            "final_review_record_history": {},
            "browser_qa_record_history": {},
            "strategy_review_dispatch_lineage": {},
            "strategy_review_dispatch_lineage_history": {},
            "final_review_dispatch_lineage": {},
            "strategy_checkpoint_kind": "none"
        });
        fs::write(
            &state_path,
            serde_json::to_string_pretty(&legacy_state)
                .expect("legacy state fixture should serialize"),
        )
        .expect("legacy state fixture should write");

        ensure_event_log_migrated_from_legacy_state_path(&state_path, None, None)
            .expect("migration should succeed");

        let events_path = events_log_path_for_state_path(&state_path);
        let events = load_event_log(&events_path).expect("event log should parse");
        let transitions = events
            .iter()
            .filter_map(|event| match &event.payload {
                ExecutionEvent::RecordStatusTransition {
                    record_status,
                    record_sequence,
                    ..
                } => Some((record_status.clone(), record_sequence.unwrap_or(0))),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            transitions
                .iter()
                .any(|(status, _)| status.as_deref() == Some("current")),
            "migration should emit explicit current record-status transitions from current maps"
        );
        assert!(
            transitions
                .iter()
                .any(|(status, _)| status.as_deref() == Some("historical")),
            "migration should emit explicit historical record-status transitions"
        );
        assert!(
            transitions
                .iter()
                .any(|(status, _)| status.as_deref() == Some("superseded")),
            "migration should emit explicit superseded record-status transitions"
        );
        assert!(
            transitions
                .iter()
                .any(|(status, _)| status.as_deref() == Some("stale_unreviewed")),
            "migration should emit explicit stale_unreviewed record-status transitions"
        );
        let sorted_sequences = {
            let mut values = transitions
                .iter()
                .map(|(_, sequence)| *sequence)
                .collect::<Vec<_>>();
            values.sort_unstable();
            values
        };
        let observed_sequences = transitions
            .iter()
            .map(|(_, sequence)| *sequence)
            .collect::<Vec<_>>();
        assert_eq!(
            observed_sequences, sorted_sequences,
            "record-status transition events should preserve authoritative record-sequence ordering"
        );
        assert!(
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    ExecutionEvent::TaskClosureRecorded {
                        task: Some(1),
                        closure_record_id: Some(record_id),
                        ..
                    } if record_id == "task-current"
                )
            }),
            "migration should replay explicit task-closure events from current_task_closure_records"
        );
    }

    #[cfg(unix)]
    #[test]
    fn reduced_state_loader_rejects_symlinked_legacy_state_path() {
        let tempdir = tempfile::tempdir().expect("tempdir should be creatable");
        let state_path = tempdir.path().join("state.json");
        symlink("missing-legacy-state.json", &state_path)
            .expect("dangling legacy state symlink should be creatable");

        let error = load_reduced_authoritative_state_for_state_path(&state_path)
            .expect_err("reduced-state loader must reject symlinked legacy authoritative state");
        assert_eq!(
            error.error_class,
            FailureClass::MalformedExecutionState.as_str()
        );
        assert!(
            error
                .message
                .contains("Authoritative harness state path must not be a symlink"),
            "symlinked legacy-state failure should explain trust-boundary rejection"
        );
    }

    #[cfg(unix)]
    #[test]
    fn reduced_state_loader_rejects_symlinked_event_log_path() {
        let tempdir = tempfile::tempdir().expect("tempdir should be creatable");
        let state_path = tempdir.path().join("state.json");
        let legacy_state = minimal_legacy_state();
        fs::write(
            &state_path,
            serde_json::to_string_pretty(&legacy_state)
                .expect("legacy state fixture should serialize"),
        )
        .expect("legacy state fixture should write");
        ensure_event_log_migrated_from_legacy_state_path(&state_path, None, None)
            .expect("migration should seed event log");

        let events_path = events_log_path_for_state_path(&state_path);
        fs::remove_file(&events_path).expect("event log fixture should be removable");
        symlink("missing-events.jsonl", &events_path)
            .expect("dangling event log symlink should be creatable");

        let error = load_reduced_authoritative_state_for_state_path(&state_path)
            .expect_err("reduced-state loader must reject symlinked authoritative event logs");
        assert_eq!(
            error.error_class,
            FailureClass::MalformedExecutionState.as_str()
        );
        assert!(
            error
                .message
                .contains("Authoritative event log path must not be a symlink"),
            "symlinked event-log failure should explain trust-boundary rejection"
        );
    }

    #[test]
    fn reduced_state_loader_rejects_non_file_event_log_path() {
        let tempdir = tempfile::tempdir().expect("tempdir should be creatable");
        let state_path = tempdir.path().join("state.json");
        let legacy_state = minimal_legacy_state();
        fs::write(
            &state_path,
            serde_json::to_string_pretty(&legacy_state)
                .expect("legacy state fixture should serialize"),
        )
        .expect("legacy state fixture should write");
        ensure_event_log_migrated_from_legacy_state_path(&state_path, None, None)
            .expect("migration should seed event log");

        let events_path = events_log_path_for_state_path(&state_path);
        fs::remove_file(&events_path).expect("event log fixture should be removable");
        fs::create_dir_all(&events_path).expect("event log path directory fixture should create");

        let error = load_reduced_authoritative_state_for_state_path(&state_path)
            .expect_err("reduced-state loader must reject non-file authoritative event logs");
        assert_eq!(
            error.error_class,
            FailureClass::MalformedExecutionState.as_str()
        );
        assert!(
            error
                .message
                .contains("Authoritative event log must be a regular file"),
            "non-file event-log failure should explain trust-boundary rejection"
        );
    }
}
