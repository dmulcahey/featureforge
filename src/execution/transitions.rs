use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::current_truth::parse_late_stage_surface_only_branch_surface;
use crate::execution::gates::{
    ActiveContractState, GateAuthorityState, require_active_contract_state,
};
use crate::execution::leases::process_is_running;
use crate::execution::state::{
    ExecutionContext, ExecutionRuntime, GateState, NoteState, latest_attempted_step_for_task,
    task_completion_lineage_fingerprint,
};
use crate::git::sha256_hex;
use crate::paths::{harness_branch_root, harness_state_path, write_atomic as write_atomic_file};

#[derive(Debug, Clone, Copy)]
pub(crate) enum StepCommand {
    Begin,
    Note,
    Complete,
    Reopen,
    Transfer,
}

impl StepCommand {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Note => "note",
            Self::Complete => "complete",
            Self::Reopen => "reopen",
            Self::Transfer => "transfer",
        }
    }
}

pub(crate) struct StepWriteAuthorityGuard {
    lock_path: PathBuf,
}

impl Drop for StepWriteAuthorityGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}

pub(crate) fn claim_step_write_authority(
    runtime: &ExecutionRuntime,
) -> Result<StepWriteAuthorityGuard, JsonFailure> {
    {
        let lock_path =
            harness_branch_root(&runtime.state_dir, &runtime.repo_slug, &runtime.branch_name)
                .join("write-authority.lock");
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                JsonFailure::new(
                    FailureClass::PartialAuthoritativeMutation,
                    format!(
                        "Could not prepare write-authority directory {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }

        let mut file = loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(file) => break file,
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    let metadata = match fs::symlink_metadata(&lock_path) {
                        Ok(metadata) => metadata,
                        Err(metadata_error) if metadata_error.kind() == ErrorKind::NotFound => {
                            continue;
                        }
                        Err(metadata_error) => {
                            return Err(JsonFailure::new(
                                FailureClass::ConcurrentWriterConflict,
                                format!(
                                    "Another runtime writer currently holds authoritative mutation authority, and the lock {} could not be inspected: {metadata_error}",
                                    lock_path.display()
                                ),
                            ));
                        }
                    };
                    if metadata.file_type().is_symlink() {
                        return Err(JsonFailure::new(
                            FailureClass::PartialAuthoritativeMutation,
                            format!(
                                "Write-authority lock path must not be a symlink in {}.",
                                lock_path.display()
                            ),
                        ));
                    }
                    if !metadata.is_file() {
                        return Err(JsonFailure::new(
                            FailureClass::PartialAuthoritativeMutation,
                            format!(
                                "Write-authority lock must be a regular file in {}.",
                                lock_path.display()
                            ),
                        ));
                    }
                    let source = fs::read_to_string(&lock_path).map_err(|read_error| {
                        JsonFailure::new(
                            FailureClass::ConcurrentWriterConflict,
                            format!(
                                "Another runtime writer currently holds authoritative mutation authority, and the lock {} could not be inspected: {read_error}",
                                lock_path.display()
                            ),
                        )
                    })?;
                    let holder_pid = source.lines().find_map(|line| {
                        line.trim()
                            .strip_prefix("pid=")
                            .and_then(|value| value.trim().parse::<u32>().ok())
                    });
                    let Some(holder_pid) = holder_pid else {
                        return Err(JsonFailure::new(
                            FailureClass::ConcurrentWriterConflict,
                            "Another runtime writer currently holds authoritative mutation authority.",
                        ));
                    };
                    if process_is_running(holder_pid) {
                        return Err(JsonFailure::new(
                            FailureClass::ConcurrentWriterConflict,
                            "Another runtime writer currently holds authoritative mutation authority.",
                        ));
                    }
                    remove_stale_write_authority_lock(&lock_path)?;
                }
                Err(error) => {
                    return Err(JsonFailure::new(
                        FailureClass::PartialAuthoritativeMutation,
                        format!(
                            "Could not acquire write-authority lock {}: {error}",
                            lock_path.display()
                        ),
                    ));
                }
            }
        };

        writeln!(file, "pid={}", std::process::id()).map_err(|error| {
            let _ = fs::remove_file(&lock_path);
            JsonFailure::new(
                FailureClass::PartialAuthoritativeMutation,
                format!(
                    "Could not initialize write-authority lock {}: {error}",
                    lock_path.display()
                ),
            )
        })?;
        Ok(StepWriteAuthorityGuard { lock_path })
    }
}

fn remove_stale_write_authority_lock(lock_path: &Path) -> Result<(), JsonFailure> {
    match fs::remove_file(lock_path) {
        Ok(()) => Ok(()),
        Err(remove_error) if remove_error.kind() == ErrorKind::NotFound => Ok(()),
        Err(remove_error) => Err(JsonFailure::new(
            FailureClass::ConcurrentWriterConflict,
            format!(
                "A stale write-authority lock was found at {}, but it could not be removed: {remove_error}",
                lock_path.display()
            ),
        )),
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct StepEvidenceProvenance {
    pub(crate) source_contract_path: Option<String>,
    pub(crate) source_contract_fingerprint: Option<String>,
    pub(crate) source_evaluation_report_fingerprint: Option<String>,
    pub(crate) evaluator_verdict: Option<String>,
    pub(crate) failing_criterion_ids: Vec<String>,
    pub(crate) source_handoff_fingerprint: Option<String>,
    pub(crate) repo_state_baseline_head_sha: Option<String>,
    pub(crate) repo_state_baseline_worktree_fingerprint: Option<String>,
}

pub(crate) struct AuthoritativeTransitionState {
    state_path: PathBuf,
    state_payload: Value,
    phase: Option<String>,
    active_contract: Option<ActiveContractState>,
    dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthoritativeStateCacheStamp {
    modified: Option<SystemTime>,
    len: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    ctime: i64,
    #[cfg(unix)]
    ctime_nsec: i64,
}

#[derive(Debug, Clone)]
struct CachedAuthoritativeStatePayload {
    stamp: AuthoritativeStateCacheStamp,
    state_payload: Value,
    gate_state: GateAuthorityState,
}

fn authoritative_state_payload_cache()
-> &'static Mutex<BTreeMap<PathBuf, CachedAuthoritativeStatePayload>> {
    static CACHE: OnceLock<Mutex<BTreeMap<PathBuf, CachedAuthoritativeStatePayload>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn lock_authoritative_state_payload_cache()
-> std::sync::MutexGuard<'static, BTreeMap<PathBuf, CachedAuthoritativeStatePayload>> {
    match authoritative_state_payload_cache().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn authoritative_state_cache_stamp(metadata: &fs::Metadata) -> AuthoritativeStateCacheStamp {
    AuthoritativeStateCacheStamp {
        modified: metadata.modified().ok(),
        len: metadata.len(),
        #[cfg(unix)]
        inode: metadata.ino(),
        #[cfg(unix)]
        ctime: metadata.ctime(),
        #[cfg(unix)]
        ctime_nsec: metadata.ctime_nsec(),
    }
}

fn invalidate_authoritative_state_payload_cache(state_path: &Path) {
    lock_authoritative_state_payload_cache().remove(state_path);
}

fn refresh_authoritative_state_payload_cache(state_path: &Path, state_payload: &Value) {
    let metadata = match fs::symlink_metadata(state_path) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_file() => metadata,
        _ => {
            invalidate_authoritative_state_payload_cache(state_path);
            return;
        }
    };
    let gate_state: GateAuthorityState =
        if let Ok(gate_state) = serde_json::from_value(state_payload.clone()) {
            gate_state
        } else {
            invalidate_authoritative_state_payload_cache(state_path);
            return;
        };
    lock_authoritative_state_payload_cache().insert(
        state_path.to_path_buf(),
        CachedAuthoritativeStatePayload {
            stamp: authoritative_state_cache_stamp(&metadata),
            state_payload: state_payload.clone(),
            gate_state,
        },
    );
}

#[derive(Clone, Copy)]
pub(crate) struct BranchClosureResultRecord<'a> {
    pub(crate) branch_closure_id: &'a str,
    pub(crate) source_plan_path: &'a str,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: &'a str,
    pub(crate) branch_name: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) contract_identity: &'a str,
    pub(crate) effective_reviewed_branch_surface: &'a str,
    pub(crate) source_task_closure_ids: &'a [String],
    pub(crate) provenance_basis: &'a str,
    pub(crate) closure_status: &'a str,
    pub(crate) superseded_branch_closure_ids: &'a [String],
    pub(crate) branch_closure_fingerprint: Option<&'a str>,
}

#[derive(Clone, Copy)]
pub(crate) struct ReleaseReadinessResultRecord<'a> {
    pub(crate) branch_closure_id: &'a str,
    pub(crate) source_plan_path: &'a str,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: &'a str,
    pub(crate) branch_name: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) result: &'a str,
    pub(crate) release_docs_fingerprint: Option<&'a str>,
    pub(crate) summary: &'a str,
    pub(crate) summary_hash: &'a str,
    pub(crate) generated_by_identity: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct FinalReviewMilestoneRecord<'a> {
    pub(crate) branch_closure_id: &'a str,
    pub(crate) release_readiness_record_id: &'a str,
    pub(crate) dispatch_id: &'a str,
    pub(crate) reviewer_source: &'a str,
    pub(crate) reviewer_id: &'a str,
    pub(crate) result: &'a str,
    pub(crate) final_review_fingerprint: Option<&'a str>,
    pub(crate) deviations_required: Option<bool>,
    pub(crate) browser_qa_required: Option<bool>,
    pub(crate) source_plan_path: &'a str,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: &'a str,
    pub(crate) branch_name: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) summary: &'a str,
    pub(crate) summary_hash: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct BrowserQaResultRecord<'a> {
    pub(crate) branch_closure_id: &'a str,
    pub(crate) final_review_record_id: &'a str,
    pub(crate) source_plan_path: &'a str,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: &'a str,
    pub(crate) branch_name: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) result: &'a str,
    pub(crate) browser_qa_fingerprint: Option<&'a str>,
    pub(crate) source_test_plan_fingerprint: Option<&'a str>,
    pub(crate) summary: &'a str,
    pub(crate) summary_hash: &'a str,
    pub(crate) generated_by_identity: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct TaskClosureResultRecord<'a> {
    pub(crate) task: u32,
    pub(crate) dispatch_id: &'a str,
    pub(crate) closure_record_id: &'a str,
    pub(crate) execution_run_id: Option<&'a str>,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) contract_identity: &'a str,
    pub(crate) effective_reviewed_surface_paths: &'a [String],
    pub(crate) review_result: &'a str,
    pub(crate) review_summary_hash: &'a str,
    pub(crate) verification_result: &'a str,
    pub(crate) verification_summary_hash: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct TaskClosureNegativeResultRecord<'a> {
    pub(crate) task: u32,
    pub(crate) dispatch_id: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) contract_identity: &'a str,
    pub(crate) review_result: &'a str,
    pub(crate) review_summary_hash: &'a str,
    pub(crate) verification_result: &'a str,
    pub(crate) verification_summary_hash: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CurrentTaskClosureRecord {
    pub(crate) task: u32,
    pub(crate) dispatch_id: String,
    pub(crate) closure_record_id: String,
    pub(crate) source_plan_path: Option<String>,
    pub(crate) source_plan_revision: Option<u32>,
    pub(crate) execution_run_id: Option<String>,
    pub(crate) reviewed_state_id: String,
    pub(crate) contract_identity: String,
    pub(crate) effective_reviewed_surface_paths: Vec<String>,
    pub(crate) review_result: String,
    pub(crate) review_summary_hash: String,
    pub(crate) verification_result: String,
    pub(crate) verification_summary_hash: String,
    pub(crate) closure_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawCurrentTaskClosureStateEntry {
    pub(crate) scope_key: String,
    pub(crate) task: Option<u32>,
    pub(crate) closure_record_id: Option<String>,
    pub(crate) record: Option<CurrentTaskClosureRecord>,
}

pub(crate) struct BranchClosureRecord {
    pub(crate) source_plan_path: String,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: String,
    pub(crate) branch_name: String,
    pub(crate) base_branch: String,
    pub(crate) reviewed_state_id: String,
    pub(crate) contract_identity: String,
    pub(crate) effective_reviewed_branch_surface: String,
    pub(crate) source_task_closure_ids: Vec<String>,
    pub(crate) provenance_basis: String,
    pub(crate) branch_closure_fingerprint: Option<String>,
}

pub(crate) struct CurrentBranchClosureIdentity {
    pub(crate) branch_closure_id: String,
    pub(crate) reviewed_state_id: String,
    pub(crate) contract_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskClosureNegativeResult {
    pub(crate) dispatch_id: String,
    pub(crate) reviewed_state_id: String,
    pub(crate) contract_identity: String,
    pub(crate) review_result: String,
    pub(crate) review_summary_hash: String,
    pub(crate) verification_result: String,
    pub(crate) verification_summary_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct OpenStepStateRecord {
    pub(crate) task: u32,
    pub(crate) step: u32,
    pub(crate) note_state: String,
    pub(crate) note_summary: String,
    pub(crate) source_plan_path: String,
    pub(crate) source_plan_revision: u32,
    pub(crate) authoritative_sequence: u64,
}

pub(crate) struct CurrentReleaseReadinessRecord {
    pub(crate) record_status: String,
    pub(crate) branch_closure_id: String,
    pub(crate) source_plan_path: String,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: String,
    pub(crate) branch_name: String,
    pub(crate) base_branch: String,
    pub(crate) reviewed_state_id: String,
    pub(crate) result: String,
    pub(crate) release_docs_fingerprint: Option<String>,
    pub(crate) summary: String,
    pub(crate) summary_hash: String,
    pub(crate) generated_by_identity: String,
}

pub(crate) struct CurrentFinalReviewRecord {
    pub(crate) record_status: String,
    pub(crate) branch_closure_id: String,
    pub(crate) release_readiness_record_id: Option<String>,
    pub(crate) dispatch_id: String,
    pub(crate) reviewer_source: String,
    pub(crate) reviewer_id: String,
    pub(crate) result: String,
    pub(crate) final_review_fingerprint: Option<String>,
    pub(crate) deviations_required: Option<bool>,
    pub(crate) browser_qa_required: Option<bool>,
    pub(crate) source_plan_path: String,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: String,
    pub(crate) branch_name: String,
    pub(crate) base_branch: String,
    pub(crate) reviewed_state_id: String,
    pub(crate) summary: String,
    pub(crate) summary_hash: String,
}

pub(crate) struct CurrentBrowserQaRecord {
    pub(crate) record_status: String,
    pub(crate) branch_closure_id: String,
    pub(crate) final_review_record_id: Option<String>,
    pub(crate) source_plan_path: String,
    pub(crate) source_plan_revision: u32,
    pub(crate) repo_slug: String,
    pub(crate) branch_name: String,
    pub(crate) base_branch: String,
    pub(crate) reviewed_state_id: String,
    pub(crate) result: String,
    pub(crate) browser_qa_fingerprint: Option<String>,
    pub(crate) source_test_plan_fingerprint: Option<String>,
    pub(crate) summary: String,
    pub(crate) summary_hash: String,
    pub(crate) generated_by_identity: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ClosureHistorySnapshot {
    pub(crate) task_closure_record_history: BTreeMap<String, Value>,
    pub(crate) branch_closure_records: BTreeMap<String, Value>,
    pub(crate) release_readiness_record_history: BTreeMap<String, Value>,
    pub(crate) final_review_record_history: BTreeMap<String, Value>,
    pub(crate) browser_qa_record_history: BTreeMap<String, Value>,
    pub(crate) current_branch_closure_id: Option<String>,
    pub(crate) current_release_readiness_record_id: Option<String>,
    pub(crate) current_final_review_record_id: Option<String>,
    pub(crate) current_qa_record_id: Option<String>,
    pub(crate) superseded_task_closure_ids: Vec<String>,
    pub(crate) superseded_branch_closure_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PersistedReviewStateFieldClass {
    AuthoritativeAppendOnlyHistory,
    AuthoritativeMutableControlState,
    DerivedCache,
    ProjectionSummary,
    Obsolete,
}

pub(crate) fn classify_review_state_field(field: &str) -> Option<PersistedReviewStateFieldClass> {
    Some(match field {
        "task_closure_record_history"
        | "branch_closure_records"
        | "release_readiness_record_history"
        | "final_review_record_history"
        | "browser_qa_record_history" => {
            PersistedReviewStateFieldClass::AuthoritativeAppendOnlyHistory
        }
        "current_open_step_state" => {
            PersistedReviewStateFieldClass::AuthoritativeMutableControlState
        }
        "current_task_closure_records"
        | "task_closure_negative_result_records"
        | "current_branch_closure_id"
        | "current_branch_closure_reviewed_state_id"
        | "current_branch_closure_contract_identity"
        | "current_release_readiness_record_id"
        | "current_release_readiness_result"
        | "current_release_readiness_summary_hash"
        | "current_final_review_record_id"
        | "current_final_review_branch_closure_id"
        | "current_final_review_dispatch_id"
        | "current_final_review_reviewer_source"
        | "current_final_review_reviewer_id"
        | "current_final_review_result"
        | "current_final_review_summary_hash"
        | "current_qa_record_id"
        | "current_qa_branch_closure_id"
        | "current_qa_result"
        | "current_qa_summary_hash"
        | "review_state_repair_follow_up" => PersistedReviewStateFieldClass::DerivedCache,
        "release_docs_state"
        | "final_review_state"
        | "browser_qa_state"
        | "last_release_docs_artifact_fingerprint"
        | "last_final_review_artifact_fingerprint"
        | "last_browser_qa_artifact_fingerprint" => {
            PersistedReviewStateFieldClass::ProjectionSummary
        }
        "current_release_readiness_state" | "current_final_review_state" | "current_qa_state" => {
            PersistedReviewStateFieldClass::Obsolete
        }
        _ => return None,
    })
}

impl AuthoritativeTransitionState {
    pub(crate) fn apply_note_reset_policy(
        &mut self,
        note_state: NoteState,
    ) -> Result<(), JsonFailure> {
        if !matches!(note_state, NoteState::Blocked | NoteState::Interrupted) {
            return Ok(());
        }
        let Some(active_contract) = self.active_contract.as_ref() else {
            return Ok(());
        };
        let reset_policy = active_contract.contract.reset_policy.trim();
        if !matches!(reset_policy, "adaptive" | "chunk-boundary") {
            return Ok(());
        }

        let pivot_threshold = json_u64(&self.state_payload, "current_chunk_pivot_threshold");
        let retry_count = json_u64(&self.state_payload, "current_chunk_retry_count");
        let next_phase = if pivot_threshold > 0 && retry_count >= pivot_threshold {
            "pivot_required"
        } else {
            "handoff_required"
        };

        let root = self.root_object_mut()?;
        root.insert(
            String::from("harness_phase"),
            Value::String(next_phase.to_owned()),
        );
        root.insert(String::from("handoff_required"), Value::Bool(true));
        root.insert(
            String::from("aggregate_evaluation_state"),
            Value::String(String::from("blocked")),
        );

        self.phase = Some(next_phase.to_owned());
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn stale_reopen_provenance(&mut self) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        for field in [
            "last_evaluation_report_path",
            "last_evaluation_report_fingerprint",
            "last_evaluation_evaluator_kind",
            "last_evaluation_verdict",
            "last_handoff_path",
            "last_handoff_fingerprint",
            "last_pivot_path",
            "last_pivot_fingerprint",
            "last_final_review_artifact_fingerprint",
            "last_browser_qa_artifact_fingerprint",
            "last_release_docs_artifact_fingerprint",
        ] {
            root.insert(field.to_owned(), Value::Null);
        }
        root.insert(
            String::from("final_review_state"),
            Value::String(String::from("stale")),
        );
        root.insert(
            String::from("browser_qa_state"),
            Value::String(String::from("stale")),
        );
        root.insert(
            String::from("release_docs_state"),
            Value::String(String::from("stale")),
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn record_runtime_handoff_checkpoint(
        &mut self,
        record_path: &str,
        record_fingerprint: &str,
    ) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        root.insert(
            String::from("harness_phase"),
            Value::String(String::from("executing")),
        );
        root.insert(String::from("handoff_required"), Value::Bool(false));
        root.insert(
            String::from("last_handoff_path"),
            Value::String(record_path.to_owned()),
        );
        root.insert(
            String::from("last_handoff_fingerprint"),
            Value::String(record_fingerprint.to_owned()),
        );
        self.phase = Some(String::from("executing"));
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn record_runtime_pivot_checkpoint(
        &mut self,
        record_path: &str,
        record_fingerprint: &str,
    ) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        root.insert(
            String::from("last_pivot_path"),
            Value::String(record_path.to_owned()),
        );
        root.insert(
            String::from("last_pivot_fingerprint"),
            Value::String(record_fingerprint.to_owned()),
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn set_harness_phase_executing(&mut self) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        root.insert(
            String::from("harness_phase"),
            Value::String(String::from("executing")),
        );
        self.phase = Some(String::from("executing"));
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn ensure_initial_dispatch_strategy_checkpoint(
        &mut self,
        context: &ExecutionContext,
        execution_mode: &str,
    ) -> Result<(), JsonFailure> {
        let has_checkpoint = self
            .state_payload
            .get("last_strategy_checkpoint_fingerprint")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        if has_checkpoint {
            return Ok(());
        }
        self.record_strategy_checkpoint(
            context,
            "initial_dispatch",
            execution_mode,
            &[],
            "Runtime recorded the initial dispatch strategy checkpoint before repo-writing execution.",
            false,
        )?;
        Ok(())
    }

    pub(crate) fn record_reopen_strategy_checkpoint(
        &mut self,
        context: &ExecutionContext,
        execution_mode: &str,
        task: u32,
        step: u32,
        reason: &str,
    ) -> Result<(), JsonFailure> {
        self.ensure_initial_dispatch_strategy_checkpoint(context, execution_mode)?;
        if self.consume_task_dispatch_credit(task)? {
            let cycle_count = self.current_task_cycle_count(task)?;
            let cycle_breaking = self
                .state_payload
                .get("strategy_checkpoint_kind")
                .and_then(Value::as_str)
                .is_some_and(|value| value == "cycle_break")
                || self
                    .state_payload
                    .get("strategy_state")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == "cycle_breaking");
            let trigger_cycle = if cycle_count == 0 { 1 } else { cycle_count };
            let trigger = vec![format!(
                "task-{task}:step-{step}:cycle-{trigger_cycle}:reopen-after-review-dispatch"
            )];
            if cycle_breaking || cycle_count >= 3 {
                self.record_strategy_checkpoint(
                    context,
                    "cycle_break",
                    execution_mode,
                    &trigger,
                    "Runtime preserved cycle-break strategy while reopening remediation after a bound review dispatch.",
                    true,
                )?;
                self.set_task_cycle_count(task, 0)?;
            } else {
                self.record_strategy_checkpoint(
                    context,
                    "review_remediation",
                    execution_mode,
                    &trigger,
                    reason,
                    false,
                )?;
            }
            return Ok(());
        }
        let stale_bound_dispatch_tasks = self.clear_task_dispatch_credits()?;
        let _ = stale_bound_dispatch_tasks;
        let bound_unbound_dispatch = self.consume_unbound_dispatch_credit()?;
        let cycle_count = self.increment_task_cycle_count(task)?;
        let trigger = if bound_unbound_dispatch {
            vec![format!(
                "task-{task}:step-{step}:cycle-{cycle_count}:bound-from-unbound-review-dispatch"
            )]
        } else {
            vec![format!("task-{task}:step-{step}:cycle-{cycle_count}")]
        };
        if cycle_count >= 3 {
            self.record_strategy_checkpoint(
                context,
                "cycle_break",
                execution_mode,
                &trigger,
                "Runtime detected churn after three reviewable dispatch/remediation cycles for the same task and auto-entered cycle-break strategy.",
                true,
            )?;
            self.set_task_cycle_count(task, 0)?;
        } else {
            self.record_strategy_checkpoint(
                context,
                "review_remediation",
                execution_mode,
                &trigger,
                reason,
                false,
            )?;
        }
        Ok(())
    }

    pub(crate) fn record_review_dispatch_strategy_checkpoint(
        &mut self,
        context: &ExecutionContext,
        execution_mode: &str,
        cycle_target: Option<(u32, u32)>,
    ) -> Result<(), JsonFailure> {
        self.ensure_initial_dispatch_strategy_checkpoint(context, execution_mode)?;
        self.clear_dispatch_credits()?;
        let (trigger, rationale, cycle_count, task_binding) = if let Some((task, step)) =
            cycle_target
        {
            let cycle_count = self.increment_task_cycle_count(task)?;
            self.increment_task_dispatch_credit(task)?;
            (
                vec![format!(
                    "task-{task}:step-{step}:cycle-{cycle_count}:review-dispatch"
                )],
                format!(
                    "Runtime recorded reviewer dispatch cycle tracking for task {task} step {step}."
                ),
                cycle_count,
                Some(task),
            )
        } else {
            let pending = self.increment_unbound_dispatch_credit()?;
            (
                vec![format!(
                    "task-unbound:step-unbound:pending-review-dispatch-{pending}"
                )],
                String::from(
                    "Runtime recorded reviewer dispatch cycle tracking for completed-plan review pending reopen task binding.",
                ),
                0,
                None,
            )
        };
        if cycle_count >= 3 {
            self.record_strategy_checkpoint(
                context,
                "cycle_break",
                execution_mode,
                &trigger,
                "Runtime detected churn after three reviewable dispatch/remediation cycles for the same task and auto-entered cycle-break strategy.",
                true,
            )?;
            if let Some(task) = task_binding {
                self.set_task_cycle_count(task, 0)?;
            }
        } else {
            self.record_strategy_checkpoint(
                context,
                "review_remediation",
                execution_mode,
                &trigger,
                &rationale,
                false,
            )?;
        }
        let checkpoint_fingerprint = self.last_strategy_checkpoint_fingerprint();

        if let Some(strategy_checkpoint_fingerprint) = checkpoint_fingerprint
            && let Some(execution_run_id) = self.execution_run_id_opt()
        {
            if let Some((task, step)) = cycle_target {
                if let Some(task_completion_lineage) =
                    task_completion_lineage_fingerprint(context, task)
                {
                    let reviewed_state_id =
                        format!("git_tree:{}", context.current_tracked_tree_sha()?);
                    self.upsert_task_dispatch_lineage(
                        task,
                        &execution_run_id,
                        step,
                        &strategy_checkpoint_fingerprint,
                        &task_completion_lineage,
                        &reviewed_state_id,
                    )?;
                }
            } else {
                let branch_closure_id = self
                    .recoverable_current_branch_closure_identity()
                    .map(|identity| identity.branch_closure_id)
                    .ok_or_else(|| {
                        JsonFailure::new(
                            FailureClass::ExecutionStateNotReady,
                            "record-review-dispatch final-review scope requires a current branch closure.",
                        )
                    })?;
                self.upsert_final_review_dispatch_lineage(
                    &execution_run_id,
                    &branch_closure_id,
                    &strategy_checkpoint_fingerprint,
                )?;
            }
        }
        Ok(())
    }

    fn record_strategy_checkpoint(
        &mut self,
        context: &ExecutionContext,
        checkpoint_kind: &str,
        execution_mode: &str,
        trigger_fingerprints: &[String],
        rationale: &str,
        cycle_breaking: bool,
    ) -> Result<String, JsonFailure> {
        let execution_run_id = self.execution_run_id_opt();
        let execution_run_label = execution_run_id.as_deref().unwrap_or("<none>");
        let selected_topology = selected_topology_from_execution_mode(execution_mode);
        let lane_decomposition = context
            .plan_document
            .tasks
            .iter()
            .map(|task| format!("task-{}", task.number))
            .collect::<Vec<_>>();
        let lane_owner_map = lane_decomposition
            .iter()
            .map(|lane| format!("{lane}=runtime"))
            .collect::<Vec<_>>();
        let worktree_plan = if selected_topology == "worktree-backed-parallel" {
            "worktree-backed-isolated-lanes"
        } else {
            "single-worktree-serialized"
        };
        let subagent_dispatch_plan = if selected_topology == "worktree-backed-parallel" {
            "parallel-lane-owned-subagents"
        } else {
            "serial-single-lane-subagent"
        };
        let acceptance_requirements = vec![
            String::from("preflight_accepted"),
            String::from("approved_plan_revision_bound"),
        ];
        let review_requirements = vec![
            String::from("dedicated_final_review"),
            String::from("gate_finish"),
        ];
        let generated_at = Timestamp::now().to_string();
        let trigger_text = if trigger_fingerprints.is_empty() {
            String::from("none")
        } else {
            trigger_fingerprints.join("|")
        };
        let fingerprint = sha256_hex(
            format!(
                "plan={}\nplan_revision={}\nrun={execution_run_label}\ncheckpoint_kind={checkpoint_kind}\nselected_topology={selected_topology}\ntriggers={trigger_text}\nlane_decomposition={}\nlane_owner_map={}\nworktree_plan={worktree_plan}\nsubagent_dispatch_plan={subagent_dispatch_plan}\nacceptance={}\nreview={}\nrationale={}\n",
                context.plan_rel,
                context.plan_document.plan_revision,
                lane_decomposition.join(","),
                lane_owner_map.join(","),
                acceptance_requirements.join(","),
                review_requirements.join(","),
                rationale.trim()
            )
            .as_bytes(),
        );

        let checkpoint = serde_json::json!({
            "source_plan_path": context.plan_rel,
            "source_plan_revision": context.plan_document.plan_revision,
            "execution_run_id": execution_run_id,
            "trigger_fingerprints": trigger_fingerprints,
            "checkpoint_kind": checkpoint_kind,
            "selected_topology": selected_topology,
            "lane_decomposition": lane_decomposition,
            "lane_owner_map": lane_owner_map,
            "worktree_plan": worktree_plan,
            "subagent_dispatch_plan": subagent_dispatch_plan,
            "acceptance_requirements": acceptance_requirements,
            "review_requirements": review_requirements,
            "rationale": rationale.trim(),
            "generated_at": generated_at,
            "fingerprint": fingerprint,
        });

        let root = self.root_object_mut()?;
        let checkpoints = root
            .entry(String::from("strategy_checkpoints"))
            .or_insert_with(|| Value::Array(Vec::new()));
        let Some(checkpoints) = checkpoints.as_array_mut() else {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness strategy_checkpoints must be a JSON array.",
            ));
        };
        checkpoints.push(checkpoint);
        root.insert(
            String::from("strategy_state"),
            Value::String(if cycle_breaking {
                String::from("cycle_breaking")
            } else {
                String::from("ready")
            }),
        );
        root.insert(
            String::from("strategy_checkpoint_kind"),
            Value::String(checkpoint_kind.to_owned()),
        );
        root.insert(
            String::from("last_strategy_checkpoint_fingerprint"),
            Value::String(fingerprint.clone()),
        );
        root.insert(String::from("strategy_reset_required"), Value::Bool(false));
        self.dirty = true;
        Ok(fingerprint)
    }

    fn increment_task_cycle_count(&mut self, task: u32) -> Result<u64, JsonFailure> {
        let root = self.root_object_mut()?;
        let cycle_counts = root
            .entry(String::from("strategy_cycle_counts"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let Some(cycle_counts) = cycle_counts.as_object_mut() else {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness strategy_cycle_counts must be a JSON object.",
            ));
        };
        let key = format!("task-{task}");
        let current = cycle_counts.get(&key).and_then(Value::as_u64).unwrap_or(0);
        let next = current.saturating_add(1);
        cycle_counts.insert(key, Value::Number(next.into()));
        self.dirty = true;
        Ok(next)
    }

    fn current_task_cycle_count(&mut self, task: u32) -> Result<u64, JsonFailure> {
        let root = self.root_object_mut()?;
        let cycle_counts = root
            .entry(String::from("strategy_cycle_counts"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let Some(cycle_counts) = cycle_counts.as_object_mut() else {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness strategy_cycle_counts must be a JSON object.",
            ));
        };
        Ok(cycle_counts
            .get(&format!("task-{task}"))
            .and_then(Value::as_u64)
            .unwrap_or(0))
    }

    fn set_task_cycle_count(&mut self, task: u32, value: u64) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        let cycle_counts = root
            .entry(String::from("strategy_cycle_counts"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let Some(cycle_counts) = cycle_counts.as_object_mut() else {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness strategy_cycle_counts must be a JSON object.",
            ));
        };
        cycle_counts.insert(format!("task-{task}"), Value::Number(value.into()));
        self.dirty = true;
        Ok(())
    }

    fn dispatch_credit_counts_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let credits = root
            .entry(String::from("strategy_review_dispatch_credits"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        credits.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness strategy_review_dispatch_credits must be a JSON object.",
            )
        })
    }

    fn dispatch_lineage_records_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let lineage = root
            .entry(String::from("strategy_review_dispatch_lineage"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        lineage.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness strategy_review_dispatch_lineage must be a JSON object.",
            )
        })
    }

    fn current_task_closure_records_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let records = root
            .entry(String::from("current_task_closure_records"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        records.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness current_task_closure_records must be a JSON object.",
            )
        })
    }

    fn task_closure_record_history_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let records = root
            .entry(String::from("task_closure_record_history"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        records.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness task_closure_record_history must be a JSON object.",
            )
        })
    }

    fn task_closure_negative_result_records_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let records = root
            .entry(String::from("task_closure_negative_result_records"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        records.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness task_closure_negative_result_records must be a JSON object.",
            )
        })
    }

    fn task_closure_negative_result_history_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let records = root
            .entry(String::from("task_closure_negative_result_history"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        records.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness task_closure_negative_result_history must be a JSON object.",
            )
        })
    }

    fn branch_closure_records_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let records = root
            .entry(String::from("branch_closure_records"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        records.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness branch_closure_records must be a JSON object.",
            )
        })
    }

    fn release_readiness_record_history_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let records = root
            .entry(String::from("release_readiness_record_history"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        records.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness release_readiness_record_history must be a JSON object.",
            )
        })
    }

    fn final_review_record_history_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let records = root
            .entry(String::from("final_review_record_history"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        records.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness final_review_record_history must be a JSON object.",
            )
        })
    }

    fn browser_qa_record_history_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let records = root
            .entry(String::from("browser_qa_record_history"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        records.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness browser_qa_record_history must be a JSON object.",
            )
        })
    }

    fn dispatch_lineage_history_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let history = root
            .entry(String::from("strategy_review_dispatch_lineage_history"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        history.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness strategy_review_dispatch_lineage_history must be a JSON object.",
            )
        })
    }

    fn final_review_dispatch_lineage_history_mut(
        &mut self,
    ) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        let root = self.root_object_mut()?;
        let history = root
            .entry(String::from("final_review_dispatch_lineage_history"))
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        history.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Authoritative harness final_review_dispatch_lineage_history must be a JSON object.",
            )
        })
    }

    fn archive_task_dispatch_lineage_record(
        &mut self,
        task: u32,
        status: &str,
    ) -> Result<Option<String>, JsonFailure> {
        let lineage_key = format!("task-{task}");
        let current_payload = self
            .state_payload
            .get("strategy_review_dispatch_lineage")
            .and_then(Value::as_object)
            .and_then(|records| records.get(&lineage_key))
            .and_then(Value::as_object)
            .cloned();
        let Some(current_payload) = current_payload else {
            return Ok(None);
        };
        let payload_value = Value::Object(current_payload.clone());
        let dispatch_id = json_string(&payload_value, "dispatch_id");
        let Some(record_id) = json_string(&payload_value, "record_id").or_else(|| {
            dispatch_id.as_deref().map(|dispatch_id| {
                deterministic_record_id(
                    "task-review-dispatch",
                    &[lineage_key.as_str(), dispatch_id],
                )
            })
        }) else {
            return Ok(None);
        };
        let recorded_at = Timestamp::now().to_string();
        let record_sequence = {
            let history = self.dispatch_lineage_history_mut()?;
            history
                .get(&record_id)
                .and_then(record_sequence_from_dispatch_record)
                .or_else(|| {
                    let sequence = json_u64(&payload_value, "record_sequence");
                    (sequence > 0).then_some(sequence)
                })
                .unwrap_or_else(|| next_dispatch_record_sequence(history))
        };
        let mut archived_payload = current_payload;
        archived_payload.insert(String::from("record_id"), Value::String(record_id.clone()));
        archived_payload.insert(
            String::from("record_sequence"),
            Value::Number(record_sequence.into()),
        );
        archived_payload.insert(
            String::from("record_status"),
            Value::String(status.to_owned()),
        );
        archived_payload.insert(String::from("status"), Value::String(status.to_owned()));
        archived_payload.insert(
            String::from("status_updated_at"),
            Value::String(recorded_at.clone()),
        );
        if json_string(&Value::Object(archived_payload.clone()), "recorded_at").is_none() {
            archived_payload.insert(String::from("recorded_at"), Value::String(recorded_at));
        }
        if !archived_payload.contains_key("dispatch_provenance") {
            archived_payload.insert(
                String::from("dispatch_provenance"),
                serde_json::json!({
                    "scope_type": "task",
                    "scope_key": lineage_key,
                }),
            );
        }
        self.dispatch_lineage_history_mut()?
            .insert(record_id.clone(), Value::Object(archived_payload));
        self.dirty = true;
        Ok(Some(record_id))
    }

    fn archive_final_review_dispatch_lineage_record(
        &mut self,
        status: &str,
    ) -> Result<Option<String>, JsonFailure> {
        let current_payload = self
            .state_payload
            .get("final_review_dispatch_lineage")
            .and_then(Value::as_object)
            .cloned();
        let Some(current_payload) = current_payload else {
            return Ok(None);
        };
        let payload_value = Value::Object(current_payload.clone());
        let dispatch_id = json_string(&payload_value, "dispatch_id");
        let branch_closure_id = json_string(&payload_value, "branch_closure_id");
        let Some(record_id) = json_string(&payload_value, "record_id").or_else(|| {
            dispatch_id.as_deref().map(|dispatch_id| {
                deterministic_record_id(
                    "final-review-dispatch",
                    &[branch_closure_id.as_deref().unwrap_or("none"), dispatch_id],
                )
            })
        }) else {
            return Ok(None);
        };
        let recorded_at = Timestamp::now().to_string();
        let record_sequence = {
            let history = self.final_review_dispatch_lineage_history_mut()?;
            history
                .get(&record_id)
                .and_then(record_sequence_from_dispatch_record)
                .or_else(|| {
                    let sequence = json_u64(&payload_value, "record_sequence");
                    (sequence > 0).then_some(sequence)
                })
                .unwrap_or_else(|| next_dispatch_record_sequence(history))
        };
        let mut archived_payload = current_payload;
        archived_payload.insert(String::from("record_id"), Value::String(record_id.clone()));
        archived_payload.insert(
            String::from("record_sequence"),
            Value::Number(record_sequence.into()),
        );
        archived_payload.insert(
            String::from("record_status"),
            Value::String(status.to_owned()),
        );
        archived_payload.insert(String::from("status"), Value::String(status.to_owned()));
        archived_payload.insert(
            String::from("status_updated_at"),
            Value::String(recorded_at.clone()),
        );
        if json_string(&Value::Object(archived_payload.clone()), "recorded_at").is_none() {
            archived_payload.insert(String::from("recorded_at"), Value::String(recorded_at));
        }
        if !archived_payload.contains_key("dispatch_provenance") {
            archived_payload.insert(
                String::from("dispatch_provenance"),
                serde_json::json!({
                    "scope_type": "final_review",
                    "scope_key": branch_closure_id.as_deref().unwrap_or("unknown"),
                }),
            );
        }
        self.final_review_dispatch_lineage_history_mut()?
            .insert(record_id.clone(), Value::Object(archived_payload));
        self.dirty = true;
        Ok(Some(record_id))
    }

    fn upsert_task_dispatch_lineage(
        &mut self,
        task: u32,
        execution_run_id: &str,
        source_step: u32,
        strategy_checkpoint_fingerprint: &str,
        task_completion_lineage_fingerprint: &str,
        reviewed_state_id: &str,
    ) -> Result<(), JsonFailure> {
        self.clear_task_closure_negative_result(task)?;
        let lineage_key = format!("task-{task}");
        let _ = self.archive_task_dispatch_lineage_record(task, "historical")?;
        let record_id = deterministic_record_id(
            "task-review-dispatch",
            &[lineage_key.as_str(), strategy_checkpoint_fingerprint],
        );
        let now = Timestamp::now().to_string();
        let record_sequence = {
            let history = self.dispatch_lineage_history_mut()?;
            history
                .get(&record_id)
                .and_then(record_sequence_from_dispatch_record)
                .unwrap_or_else(|| next_dispatch_record_sequence(history))
        };
        let record_payload = serde_json::json!({
            "record_id": record_id.as_str(),
            "record_sequence": record_sequence,
            "record_status": "current",
            "status": "current",
            "recorded_at": now,
            "status_updated_at": now,
            "execution_run_id": execution_run_id,
            "dispatch_id": strategy_checkpoint_fingerprint,
            "reviewed_state_id": reviewed_state_id,
            "source_task": task,
            "source_step": source_step,
            "strategy_checkpoint_fingerprint": strategy_checkpoint_fingerprint,
            "task_completion_lineage_fingerprint": task_completion_lineage_fingerprint,
            "dispatch_provenance": {
                "scope_type": "task",
                "scope_key": lineage_key,
                "source_task": task,
                "source_step": source_step,
                "strategy_checkpoint_fingerprint": strategy_checkpoint_fingerprint,
            }
        });
        self.dispatch_lineage_history_mut()?
            .insert(record_id, record_payload.clone());
        self.dispatch_lineage_records_mut()?
            .insert(lineage_key, record_payload);
        self.dirty = true;
        Ok(())
    }

    fn upsert_final_review_dispatch_lineage(
        &mut self,
        execution_run_id: &str,
        branch_closure_id: &str,
        strategy_checkpoint_fingerprint: &str,
    ) -> Result<(), JsonFailure> {
        let _ = self.archive_final_review_dispatch_lineage_record("historical")?;
        let record_id = deterministic_record_id(
            "final-review-dispatch",
            &[branch_closure_id, strategy_checkpoint_fingerprint],
        );
        let now = Timestamp::now().to_string();
        let record_sequence = {
            let history = self.final_review_dispatch_lineage_history_mut()?;
            history
                .get(&record_id)
                .and_then(record_sequence_from_dispatch_record)
                .unwrap_or_else(|| next_dispatch_record_sequence(history))
        };
        let record_payload = serde_json::json!({
            "record_id": record_id.as_str(),
            "record_sequence": record_sequence,
            "record_status": "current",
            "status": "current",
            "recorded_at": now,
            "status_updated_at": now,
            "execution_run_id": execution_run_id,
            "dispatch_id": strategy_checkpoint_fingerprint,
            "branch_closure_id": branch_closure_id,
            "dispatch_provenance": {
                "scope_type": "final_review",
                "scope_key": branch_closure_id,
                "strategy_checkpoint_fingerprint": strategy_checkpoint_fingerprint,
            }
        });
        self.final_review_dispatch_lineage_history_mut()?
            .insert(record_id, record_payload.clone());
        self.root_object_mut()?.insert(
            String::from("final_review_dispatch_lineage"),
            record_payload,
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn refresh_task_review_dispatch_lineage(
        &mut self,
        context: &ExecutionContext,
        task: u32,
    ) -> Result<(), JsonFailure> {
        let lineage_key = format!("task-{task}");
        let has_existing_lineage = self
            .state_payload
            .get("strategy_review_dispatch_lineage")
            .and_then(Value::as_object)
            .is_some_and(|lineage| lineage.contains_key(&lineage_key));
        if !has_existing_lineage {
            return Ok(());
        }
        self.ensure_task_review_dispatch_lineage(context, task)
    }

    pub(crate) fn ensure_task_review_dispatch_lineage(
        &mut self,
        context: &ExecutionContext,
        task: u32,
    ) -> Result<(), JsonFailure> {
        let Some(strategy_checkpoint_fingerprint) = self.last_strategy_checkpoint_fingerprint()
        else {
            return Ok(());
        };
        let Some(task_completion_lineage_fingerprint) =
            task_completion_lineage_fingerprint(context, task)
        else {
            return Ok(());
        };
        let Some(source_step) = latest_attempted_step_for_task(context, task) else {
            return Ok(());
        };
        let reviewed_state_id = format!("git_tree:{}", context.current_tracked_tree_sha()?);
        let Some(execution_run_id) = self.execution_run_id_opt() else {
            return Ok(());
        };
        self.upsert_task_dispatch_lineage(
            task,
            &execution_run_id,
            source_step,
            &strategy_checkpoint_fingerprint,
            &task_completion_lineage_fingerprint,
            &reviewed_state_id,
        )
    }

    fn clear_task_review_dispatch_lineage_with_record_status(
        &mut self,
        task: u32,
        record_status: &str,
    ) -> Result<bool, JsonFailure> {
        let lineage_key = format!("task-{task}");
        let _ = self.archive_task_dispatch_lineage_record(task, record_status)?;
        let removed_lineage = self
            .dispatch_lineage_records_mut()?
            .remove(&lineage_key)
            .is_some();
        let removed_credit = self
            .dispatch_credit_counts_mut()?
            .remove(&lineage_key)
            .is_some();
        if removed_lineage || removed_credit {
            self.dirty = true;
        }
        Ok(removed_lineage || removed_credit)
    }

    pub(crate) fn clear_task_review_dispatch_lineage(
        &mut self,
        task: u32,
    ) -> Result<bool, JsonFailure> {
        self.clear_task_review_dispatch_lineage_with_record_status(task, "stale_unreviewed")
    }

    pub(crate) fn clear_task_review_dispatch_lineage_for_structural_repair(
        &mut self,
        task: u32,
    ) -> Result<bool, JsonFailure> {
        self.clear_task_review_dispatch_lineage_with_record_status(task, "historical")
    }

    pub(crate) fn set_current_branch_closure_id(
        &mut self,
        branch_closure_id: &str,
        reviewed_state_id: &str,
        contract_identity: &str,
    ) -> Result<(), JsonFailure> {
        {
            let previous_branch_closure_id =
                json_string(&self.state_payload, "current_branch_closure_id");
            let branch_closure_changed = previous_branch_closure_id
                .as_deref()
                .is_some_and(|value| value != branch_closure_id);
            let previous_release_readiness_record_id =
                json_string(&self.state_payload, "current_release_readiness_record_id");
            let previous_final_review_record_id =
                json_string(&self.state_payload, "current_final_review_record_id");
            let previous_qa_record_id = json_string(&self.state_payload, "current_qa_record_id");
            if branch_closure_changed {
                let _ = self.archive_final_review_dispatch_lineage_record("stale_unreviewed")?;
            }
            {
                let root = self.root_object_mut()?;
                root.insert(
                    String::from("current_branch_closure_id"),
                    Value::String(branch_closure_id.to_owned()),
                );
                root.insert(
                    String::from("current_branch_closure_reviewed_state_id"),
                    Value::String(reviewed_state_id.to_owned()),
                );
                root.insert(
                    String::from("current_branch_closure_contract_identity"),
                    Value::String(contract_identity.to_owned()),
                );
                root.insert(
                    String::from("current_release_readiness_result"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_release_readiness_summary_hash"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_release_readiness_record_id"),
                    Value::Null,
                );
                root.insert(String::from("release_docs_state"), Value::Null);
                root.insert(
                    String::from("last_release_docs_artifact_fingerprint"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_final_review_branch_closure_id"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_final_review_dispatch_id"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_final_review_reviewer_source"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_final_review_reviewer_id"),
                    Value::Null,
                );
                root.insert(String::from("current_final_review_result"), Value::Null);
                root.insert(
                    String::from("current_final_review_summary_hash"),
                    Value::Null,
                );
                root.insert(String::from("current_final_review_record_id"), Value::Null);
                root.insert(String::from("final_review_state"), Value::Null);
                root.insert(
                    String::from("last_final_review_artifact_fingerprint"),
                    Value::Null,
                );
                root.insert(String::from("browser_qa_state"), Value::Null);
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::Null,
                );
                root.insert(String::from("current_qa_branch_closure_id"), Value::Null);
                root.insert(String::from("current_qa_result"), Value::Null);
                root.insert(String::from("current_qa_summary_hash"), Value::Null);
                root.insert(String::from("current_qa_record_id"), Value::Null);
                root.insert(String::from("final_review_dispatch_lineage"), Value::Null);
                root.insert(
                    String::from("finish_review_gate_pass_branch_closure_id"),
                    Value::Null,
                );
                root.insert(String::from("review_state_repair_follow_up"), Value::Null);
            }
            self.dirty = true;

            if branch_closure_changed {
                let records = self.branch_closure_records_mut()?;
                if let Some(previous_branch_closure_id) = previous_branch_closure_id.as_deref() {
                    mark_record_status(records, previous_branch_closure_id, "superseded");
                }
            }
            if let Some(previous_release_readiness_record_id) =
                previous_release_readiness_record_id.as_deref()
            {
                let records = self.release_readiness_record_history_mut()?;
                mark_record_status(records, previous_release_readiness_record_id, "historical");
            }
            if let Some(previous_final_review_record_id) =
                previous_final_review_record_id.as_deref()
            {
                let records = self.final_review_record_history_mut()?;
                mark_record_status(records, previous_final_review_record_id, "historical");
            }
            if let Some(previous_qa_record_id) = previous_qa_record_id.as_deref() {
                let records = self.browser_qa_record_history_mut()?;
                mark_record_status(records, previous_qa_record_id, "historical");
            }
            Ok(())
        }
    }

    pub(crate) fn restore_current_branch_closure_overlay_fields(
        &mut self,
        branch_closure_id: &str,
        reviewed_state_id: &str,
        contract_identity: &str,
    ) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        root.insert(
            String::from("current_branch_closure_id"),
            Value::String(branch_closure_id.to_owned()),
        );
        root.insert(
            String::from("current_branch_closure_reviewed_state_id"),
            Value::String(reviewed_state_id.to_owned()),
        );
        root.insert(
            String::from("current_branch_closure_contract_identity"),
            Value::String(contract_identity.to_owned()),
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn clear_current_branch_closure_for_structural_repair(
        &mut self,
    ) -> Result<bool, JsonFailure> {
        {
            let previous_branch_closure_id =
                json_string(&self.state_payload, "current_branch_closure_id");
            let previous_release_readiness_record_id =
                json_string(&self.state_payload, "current_release_readiness_record_id");
            let previous_final_review_record_id =
                json_string(&self.state_payload, "current_final_review_record_id");
            let previous_qa_record_id = json_string(&self.state_payload, "current_qa_record_id");
            let had_final_review_dispatch_lineage = self
                .state_payload
                .get("final_review_dispatch_lineage")
                .and_then(Value::as_object)
                .is_some();
            let had_finish_review_checkpoint = self
                .state_payload
                .get("finish_review_gate_pass_branch_closure_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|value| !value.is_empty());

            if previous_branch_closure_id.is_none()
                && previous_release_readiness_record_id.is_none()
                && previous_final_review_record_id.is_none()
                && previous_qa_record_id.is_none()
                && !had_final_review_dispatch_lineage
                && !had_finish_review_checkpoint
            {
                return Ok(false);
            }

            let _ = self.archive_final_review_dispatch_lineage_record("historical")?;
            {
                let root = self.root_object_mut()?;
                root.insert(String::from("current_branch_closure_id"), Value::Null);
                root.insert(
                    String::from("current_branch_closure_reviewed_state_id"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_branch_closure_contract_identity"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_release_readiness_result"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_release_readiness_summary_hash"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_release_readiness_record_id"),
                    Value::Null,
                );
                root.insert(String::from("release_docs_state"), Value::Null);
                root.insert(
                    String::from("last_release_docs_artifact_fingerprint"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_final_review_branch_closure_id"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_final_review_dispatch_id"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_final_review_reviewer_source"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_final_review_reviewer_id"),
                    Value::Null,
                );
                root.insert(String::from("current_final_review_result"), Value::Null);
                root.insert(
                    String::from("current_final_review_summary_hash"),
                    Value::Null,
                );
                root.insert(String::from("current_final_review_record_id"), Value::Null);
                root.insert(String::from("final_review_state"), Value::Null);
                root.insert(
                    String::from("last_final_review_artifact_fingerprint"),
                    Value::Null,
                );
                root.insert(String::from("browser_qa_state"), Value::Null);
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::Null,
                );
                root.insert(String::from("current_qa_branch_closure_id"), Value::Null);
                root.insert(String::from("current_qa_result"), Value::Null);
                root.insert(String::from("current_qa_summary_hash"), Value::Null);
                root.insert(String::from("current_qa_record_id"), Value::Null);
                root.insert(String::from("final_review_dispatch_lineage"), Value::Null);
                root.insert(
                    String::from("finish_review_gate_pass_branch_closure_id"),
                    Value::Null,
                );
            }
            self.dirty = true;

            if let Some(previous_branch_closure_id) = previous_branch_closure_id.as_deref() {
                let records = self.branch_closure_records_mut()?;
                mark_record_status(records, previous_branch_closure_id, "historical");
            }
            if let Some(previous_release_readiness_record_id) =
                previous_release_readiness_record_id.as_deref()
            {
                let records = self.release_readiness_record_history_mut()?;
                mark_record_status(records, previous_release_readiness_record_id, "historical");
            }
            if let Some(previous_final_review_record_id) =
                previous_final_review_record_id.as_deref()
            {
                let records = self.final_review_record_history_mut()?;
                mark_record_status(records, previous_final_review_record_id, "historical");
            }
            if let Some(previous_qa_record_id) = previous_qa_record_id.as_deref() {
                let records = self.browser_qa_record_history_mut()?;
                mark_record_status(records, previous_qa_record_id, "historical");
            }
            Ok(true)
        }
    }

    pub(crate) fn review_state_repair_follow_up(&self) -> Option<&str> {
        self.state_payload
            .get("review_state_repair_follow_up")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn closure_history_snapshot(&self) -> ClosureHistorySnapshot {
        fn snapshot_map(payload: &Value, key: &str) -> BTreeMap<String, Value> {
            payload
                .get(key)
                .and_then(Value::as_object)
                .map(|records| {
                    records
                        .iter()
                        .map(|(record_id, record)| (record_id.clone(), record.clone()))
                        .collect::<BTreeMap<_, _>>()
                })
                .unwrap_or_default()
        }

        ClosureHistorySnapshot {
            task_closure_record_history: snapshot_map(
                &self.state_payload,
                "task_closure_record_history",
            ),
            branch_closure_records: snapshot_map(&self.state_payload, "branch_closure_records"),
            release_readiness_record_history: snapshot_map(
                &self.state_payload,
                "release_readiness_record_history",
            ),
            final_review_record_history: snapshot_map(
                &self.state_payload,
                "final_review_record_history",
            ),
            browser_qa_record_history: snapshot_map(
                &self.state_payload,
                "browser_qa_record_history",
            ),
            current_branch_closure_id: json_string(
                &self.state_payload,
                "current_branch_closure_id",
            ),
            current_release_readiness_record_id: self.current_release_readiness_record_id(),
            current_final_review_record_id: self.current_final_review_record_id(),
            current_qa_record_id: self.current_qa_record_id(),
            superseded_task_closure_ids: self.superseded_task_closure_ids(),
            superseded_branch_closure_ids: self.superseded_branch_closure_ids(),
        }
    }

    pub(crate) fn state_payload_snapshot(&self) -> Value {
        self.state_payload.clone()
    }

    pub(crate) fn set_review_state_repair_follow_up(
        &mut self,
        follow_up: Option<&str>,
    ) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        root.insert(
            String::from("review_state_repair_follow_up"),
            follow_up.map_or(Value::Null, |value| Value::String(value.to_owned())),
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn set_current_release_readiness_record_id_cache(
        &mut self,
        record_id: Option<&str>,
    ) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        root.insert(
            String::from("current_release_readiness_record_id"),
            record_id.map_or(Value::Null, |value| Value::String(value.to_owned())),
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn set_current_final_review_record_id_cache(
        &mut self,
        record_id: Option<&str>,
    ) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        root.insert(
            String::from("current_final_review_record_id"),
            record_id.map_or(Value::Null, |value| Value::String(value.to_owned())),
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn set_current_qa_record_id_cache(
        &mut self,
        record_id: Option<&str>,
    ) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        root.insert(
            String::from("current_qa_record_id"),
            record_id.map_or(Value::Null, |value| Value::String(value.to_owned())),
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn restore_current_release_readiness_overlay_fields(
        &mut self,
    ) -> Result<bool, JsonFailure> {
        let Some((record_id, payload)) = self.current_record_entry(
            "current_release_readiness_record_id",
            "release_readiness_record_history",
        ) else {
            return Ok(false);
        };
        let branch_closure_id = json_string(&payload, "branch_closure_id").ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Current release-readiness record is missing branch_closure_id.",
            )
        })?;
        let reviewed_state_id = json_string(&payload, "reviewed_state_id").ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Current release-readiness record is missing reviewed_state_id.",
            )
        })?;
        let Some(current_identity) = self.recoverable_current_branch_closure_identity() else {
            return Ok(false);
        };
        if current_identity.branch_closure_id != branch_closure_id
            || current_identity.reviewed_state_id != reviewed_state_id
        {
            return Ok(false);
        }
        if json_string(&self.state_payload, "current_branch_closure_id")
            .as_deref()
            .is_some_and(|current| current != branch_closure_id)
        {
            return Ok(false);
        }
        let result = json_string(&payload, "result").ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Current release-readiness record is missing result.",
            )
        })?;
        let summary_hash = json_string(&payload, "summary_hash").ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Current release-readiness record is missing summary_hash.",
            )
        })?;
        let release_docs_fingerprint = json_string(&payload, "release_docs_fingerprint");
        let root = self.root_object_mut()?;
        root.insert(
            String::from("current_release_readiness_result"),
            Value::String(result),
        );
        root.insert(
            String::from("current_release_readiness_summary_hash"),
            Value::String(summary_hash),
        );
        root.insert(
            String::from("current_release_readiness_record_id"),
            Value::String(record_id),
        );
        root.insert(
            String::from("release_docs_state"),
            Value::String(String::from("fresh")),
        );
        match release_docs_fingerprint {
            Some(fingerprint) => {
                root.insert(
                    String::from("last_release_docs_artifact_fingerprint"),
                    Value::String(fingerprint),
                );
            }
            None => {
                root.insert(
                    String::from("last_release_docs_artifact_fingerprint"),
                    Value::Null,
                );
            }
        }
        self.dirty = true;
        Ok(true)
    }

    pub(crate) fn restore_current_final_review_overlay_fields(
        &mut self,
    ) -> Result<bool, JsonFailure> {
        {
            let Some((record_id, payload)) = self.current_record_entry(
                "current_final_review_record_id",
                "final_review_record_history",
            ) else {
                return Ok(false);
            };
            let branch_closure_id =
                json_string(&payload, "branch_closure_id").ok_or_else(|| {
                    JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Current final-review record is missing branch_closure_id.",
                    )
                })?;
            let reviewed_state_id =
                json_string(&payload, "reviewed_state_id").ok_or_else(|| {
                    JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        "Current final-review record is missing reviewed_state_id.",
                    )
                })?;
            let Some(current_identity) = self.recoverable_current_branch_closure_identity() else {
                return Ok(false);
            };
            if current_identity.branch_closure_id != branch_closure_id
                || current_identity.reviewed_state_id != reviewed_state_id
            {
                return Ok(false);
            }
            if json_string(&self.state_payload, "current_branch_closure_id")
                .as_deref()
                .is_some_and(|current| current != branch_closure_id)
            {
                return Ok(false);
            }
            let Some(current_release_readiness_record_id) =
                self.current_release_readiness_record_id()
            else {
                return Ok(false);
            };
            if json_string(&payload, "release_readiness_record_id").as_deref()
                != Some(current_release_readiness_record_id.as_str())
            {
                return Ok(false);
            }
            let dispatch_id = json_string(&payload, "dispatch_id").ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Current final-review record is missing dispatch_id.",
                )
            })?;
            let reviewer_source = json_string(&payload, "reviewer_source").ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Current final-review record is missing reviewer_source.",
                )
            })?;
            let reviewer_id = json_string(&payload, "reviewer_id").ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Current final-review record is missing reviewer_id.",
                )
            })?;
            let result = json_string(&payload, "result").ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Current final-review record is missing result.",
                )
            })?;
            let summary_hash = json_string(&payload, "summary_hash").ok_or_else(|| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    "Current final-review record is missing summary_hash.",
                )
            })?;
            let final_review_fingerprint = json_string(&payload, "final_review_fingerprint");
            let browser_qa_required = payload.get("browser_qa_required").and_then(Value::as_bool);
            let root = self.root_object_mut()?;
            root.insert(
                String::from("current_final_review_branch_closure_id"),
                Value::String(branch_closure_id),
            );
            root.insert(
                String::from("current_final_review_dispatch_id"),
                Value::String(dispatch_id),
            );
            root.insert(
                String::from("current_final_review_reviewer_source"),
                Value::String(reviewer_source),
            );
            root.insert(
                String::from("current_final_review_reviewer_id"),
                Value::String(reviewer_id),
            );
            root.insert(
                String::from("current_final_review_result"),
                Value::String(result),
            );
            root.insert(
                String::from("current_final_review_summary_hash"),
                Value::String(summary_hash),
            );
            root.insert(
                String::from("current_final_review_record_id"),
                Value::String(record_id),
            );
            root.insert(
                String::from("final_review_state"),
                Value::String(String::from("fresh")),
            );
            match final_review_fingerprint {
                Some(fingerprint) => {
                    root.insert(
                        String::from("last_final_review_artifact_fingerprint"),
                        Value::String(fingerprint),
                    );
                }
                None => {
                    root.insert(
                        String::from("last_final_review_artifact_fingerprint"),
                        Value::Null,
                    );
                }
            }
            if browser_qa_required == Some(false) {
                root.insert(
                    String::from("browser_qa_state"),
                    Value::String(String::from("not_required")),
                );
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::Null,
                );
            }
            self.dirty = true;
            Ok(true)
        }
    }

    pub(crate) fn restore_current_browser_qa_overlay_fields(
        &mut self,
    ) -> Result<bool, JsonFailure> {
        let Some((record_id, payload)) =
            self.current_record_entry("current_qa_record_id", "browser_qa_record_history")
        else {
            return Ok(false);
        };
        let branch_closure_id = json_string(&payload, "branch_closure_id").ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Current browser QA record is missing branch_closure_id.",
            )
        })?;
        let reviewed_state_id = json_string(&payload, "reviewed_state_id").ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Current browser QA record is missing reviewed_state_id.",
            )
        })?;
        let Some(current_identity) = self.recoverable_current_branch_closure_identity() else {
            return Ok(false);
        };
        if current_identity.branch_closure_id != branch_closure_id
            || current_identity.reviewed_state_id != reviewed_state_id
        {
            return Ok(false);
        }
        if json_string(&self.state_payload, "current_branch_closure_id")
            .as_deref()
            .is_some_and(|current| current != branch_closure_id)
        {
            return Ok(false);
        }
        let Some(current_final_review_record_id) = self.current_final_review_record_id() else {
            return Ok(false);
        };
        if json_string(&payload, "final_review_record_id").as_deref()
            != Some(current_final_review_record_id.as_str())
        {
            return Ok(false);
        }
        let result = json_string(&payload, "result").ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Current browser QA record is missing result.",
            )
        })?;
        let summary_hash = json_string(&payload, "summary_hash").ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Current browser QA record is missing summary_hash.",
            )
        })?;
        let browser_qa_fingerprint = json_string(&payload, "browser_qa_fingerprint");
        let root = self.root_object_mut()?;
        root.insert(
            String::from("current_qa_branch_closure_id"),
            Value::String(branch_closure_id),
        );
        root.insert(String::from("current_qa_result"), Value::String(result));
        root.insert(
            String::from("current_qa_summary_hash"),
            Value::String(summary_hash),
        );
        root.insert(
            String::from("current_qa_record_id"),
            Value::String(record_id),
        );
        root.insert(
            String::from("browser_qa_state"),
            Value::String(String::from("fresh")),
        );
        match browser_qa_fingerprint {
            Some(fingerprint) => {
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::String(fingerprint),
                );
            }
            None => {
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::Null,
                );
            }
        }
        self.dirty = true;
        Ok(true)
    }

    pub(crate) fn record_finish_review_gate_pass_checkpoint(
        &mut self,
        branch_closure_id: &str,
    ) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        root.insert(
            String::from("finish_review_gate_pass_branch_closure_id"),
            Value::String(branch_closure_id.to_owned()),
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn record_finish_review_gate_pass_checkpoint_if_current(
        &mut self,
        branch_closure_id: &str,
    ) -> Result<bool, JsonFailure> {
        if self
            .recoverable_current_branch_closure_identity()
            .as_ref()
            .map(|identity| identity.branch_closure_id.as_str())
            != Some(branch_closure_id)
        {
            return Ok(false);
        }
        self.record_finish_review_gate_pass_checkpoint(branch_closure_id)?;
        Ok(true)
    }

    pub(crate) fn record_branch_closure(
        &mut self,
        record: BranchClosureResultRecord<'_>,
    ) -> Result<(), JsonFailure> {
        let records = self.branch_closure_records_mut()?;
        let record_sequence = records.len() as u64 + 1;
        let mut payload = serde_json::json!({
            "branch_closure_id": record.branch_closure_id,
            "source_plan_path": record.source_plan_path,
            "source_plan_revision": record.source_plan_revision,
            "repo_slug": record.repo_slug,
            "branch_name": record.branch_name,
            "base_branch": record.base_branch,
            "reviewed_state_id": record.reviewed_state_id,
            "contract_identity": record.contract_identity,
            "effective_reviewed_branch_surface": record.effective_reviewed_branch_surface,
            "source_task_closure_ids": record.source_task_closure_ids,
            "provenance_basis": record.provenance_basis,
            "closure_status": record.closure_status,
            "superseded_branch_closure_ids": record.superseded_branch_closure_ids,
            "record_sequence": record_sequence,
        });
        if let Some(branch_closure_fingerprint) = record.branch_closure_fingerprint
            && let Some(payload_object) = payload.as_object_mut()
        {
            payload_object.insert(
                String::from("branch_closure_fingerprint"),
                Value::String(branch_closure_fingerprint.to_owned()),
            );
        }
        records.insert(record.branch_closure_id.to_owned(), payload);
        self.root_object_mut()?.insert(
            String::from("harness_phase"),
            Value::String(String::from("document_release_pending")),
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn next_available_branch_closure_record_id(&self, base_record_id: &str) -> String {
        next_available_history_record_id(
            self.state_payload
                .get("branch_closure_records")
                .and_then(Value::as_object),
            base_record_id,
            None,
        )
    }

    pub(crate) fn record_release_readiness_result(
        &mut self,
        record: ReleaseReadinessResultRecord<'_>,
    ) -> Result<(), JsonFailure> {
        {
            let previous_record_id =
                json_string(&self.state_payload, "current_release_readiness_record_id");
            let previous_final_review_record_id =
                json_string(&self.state_payload, "current_final_review_record_id");
            let previous_qa_record_id = json_string(&self.state_payload, "current_qa_record_id");
            let source_plan_revision = record.source_plan_revision.to_string();
            let history = self.release_readiness_record_history_mut()?;
            let base_record_id = deterministic_record_id(
                "release-readiness",
                &[
                    record.branch_closure_id,
                    record.reviewed_state_id,
                    record.source_plan_path,
                    source_plan_revision.as_str(),
                    record.repo_slug,
                    record.branch_name,
                    record.base_branch,
                    record.result,
                    record.summary_hash,
                    record.generated_by_identity,
                    record.release_docs_fingerprint.unwrap_or("none"),
                ],
            );
            let record_id = next_available_history_record_id(
                Some(history),
                &base_record_id,
                previous_record_id.as_deref(),
            );
            let record_sequence = history.len() as u64 + 1;
            if previous_record_id
                .as_deref()
                .is_some_and(|value| value != record_id)
                && let Some(previous_record_id) = previous_record_id.as_deref()
            {
                mark_record_status(history, previous_record_id, "historical");
            }
            history.insert(
                record_id.clone(),
                serde_json::json!({
                    "record_id": record_id,
                    "record_sequence": record_sequence,
                    "record_status": "current",
                    "branch_closure_id": record.branch_closure_id,
                    "source_plan_path": record.source_plan_path,
                    "source_plan_revision": record.source_plan_revision,
                    "repo_slug": record.repo_slug,
                    "branch_name": record.branch_name,
                    "base_branch": record.base_branch,
                    "reviewed_state_id": record.reviewed_state_id,
                    "result": record.result,
                    "release_docs_fingerprint": record.release_docs_fingerprint,
                    "summary": record.summary,
                    "summary_hash": record.summary_hash,
                    "generated_by_identity": record.generated_by_identity,
                }),
            );
            let release_record_changed = previous_record_id
                .as_deref()
                .is_none_or(|value| value != record_id);
            if release_record_changed {
                let _ = self.archive_final_review_dispatch_lineage_record("stale_unreviewed")?;
            }
            let root = self.root_object_mut()?;
            root.insert(
                String::from("current_release_readiness_result"),
                Value::String(record.result.to_owned()),
            );
            root.insert(
                String::from("current_release_readiness_summary_hash"),
                Value::String(record.summary_hash.to_owned()),
            );
            root.insert(
                String::from("current_release_readiness_record_id"),
                Value::String(record_id),
            );
            root.insert(
                String::from("release_docs_state"),
                Value::String(String::from("fresh")),
            );
            root.insert(
                String::from("harness_phase"),
                Value::String(if record.result == "ready" {
                    String::from("final_review_pending")
                } else {
                    String::from("document_release_pending")
                }),
            );
            match record.release_docs_fingerprint {
                Some(fingerprint) => {
                    root.insert(
                        String::from("last_release_docs_artifact_fingerprint"),
                        Value::String(fingerprint.to_owned()),
                    );
                }
                None => {
                    root.insert(
                        String::from("last_release_docs_artifact_fingerprint"),
                        Value::Null,
                    );
                }
            }
            if release_record_changed {
                root.insert(
                    String::from("current_final_review_branch_closure_id"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_final_review_dispatch_id"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_final_review_reviewer_source"),
                    Value::Null,
                );
                root.insert(
                    String::from("current_final_review_reviewer_id"),
                    Value::Null,
                );
                root.insert(String::from("current_final_review_result"), Value::Null);
                root.insert(
                    String::from("current_final_review_summary_hash"),
                    Value::Null,
                );
                root.insert(String::from("current_final_review_record_id"), Value::Null);
                root.insert(String::from("final_review_state"), Value::Null);
                root.insert(
                    String::from("last_final_review_artifact_fingerprint"),
                    Value::Null,
                );
                root.insert(String::from("browser_qa_state"), Value::Null);
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::Null,
                );
                root.insert(String::from("current_qa_branch_closure_id"), Value::Null);
                root.insert(String::from("current_qa_result"), Value::Null);
                root.insert(String::from("current_qa_summary_hash"), Value::Null);
                root.insert(String::from("current_qa_record_id"), Value::Null);
                root.insert(String::from("final_review_dispatch_lineage"), Value::Null);
            }
            self.dirty = true;
            if release_record_changed {
                if let Some(previous_final_review_record_id) =
                    previous_final_review_record_id.as_deref()
                {
                    let records = self.final_review_record_history_mut()?;
                    mark_record_status(records, previous_final_review_record_id, "historical");
                }
                if let Some(previous_qa_record_id) = previous_qa_record_id.as_deref() {
                    let records = self.browser_qa_record_history_mut()?;
                    mark_record_status(records, previous_qa_record_id, "historical");
                }
            }
            Ok(())
        }
    }

    pub(crate) fn record_final_review_result(
        &mut self,
        record: FinalReviewMilestoneRecord<'_>,
    ) -> Result<(), JsonFailure> {
        {
            let previous_record_id =
                json_string(&self.state_payload, "current_final_review_record_id");
            let previous_qa_record_id = json_string(&self.state_payload, "current_qa_record_id");
            let source_plan_revision = record.source_plan_revision.to_string();
            let browser_qa_required = record
                .browser_qa_required
                .map_or_else(|| String::from("none"), |value| value.to_string());
            let deviations_required = record
                .deviations_required
                .map_or_else(|| String::from("none"), |value| value.to_string());
            let history = self.final_review_record_history_mut()?;
            let record_id = deterministic_record_id(
                "final-review",
                &[
                    record.branch_closure_id,
                    record.release_readiness_record_id,
                    record.dispatch_id,
                    record.reviewer_source,
                    record.reviewer_id,
                    record.result,
                    record.summary_hash,
                    record.final_review_fingerprint.unwrap_or("none"),
                    deviations_required.as_str(),
                    browser_qa_required.as_str(),
                    record.source_plan_path,
                    source_plan_revision.as_str(),
                    record.repo_slug,
                    record.branch_name,
                    record.base_branch,
                    record.reviewed_state_id,
                ],
            );
            let record_sequence = history.len() as u64 + 1;
            if previous_record_id
                .as_deref()
                .is_some_and(|value| value != record_id)
                && let Some(previous_record_id) = previous_record_id.as_deref()
            {
                mark_record_status(history, previous_record_id, "historical");
            }
            history.insert(
                record_id.clone(),
                serde_json::json!({
                    "record_id": record_id,
                    "record_sequence": record_sequence,
                    "record_status": "current",
                    "branch_closure_id": record.branch_closure_id,
                    "release_readiness_record_id": record.release_readiness_record_id,
                    "source_plan_path": record.source_plan_path,
                    "source_plan_revision": record.source_plan_revision,
                    "repo_slug": record.repo_slug,
                    "branch_name": record.branch_name,
                    "base_branch": record.base_branch,
                    "reviewed_state_id": record.reviewed_state_id,
                    "dispatch_id": record.dispatch_id,
                    "reviewer_source": record.reviewer_source,
                    "reviewer_id": record.reviewer_id,
                    "result": record.result,
                    "final_review_fingerprint": record.final_review_fingerprint,
                    "deviations_required": record.deviations_required,
                    "browser_qa_required": record.browser_qa_required,
                    "summary": record.summary,
                    "summary_hash": record.summary_hash,
                }),
            );
            let final_review_record_changed = previous_record_id
                .as_deref()
                .is_none_or(|value| value != record_id);
            let root = self.root_object_mut()?;
            root.insert(
                String::from("current_final_review_branch_closure_id"),
                Value::String(record.branch_closure_id.to_owned()),
            );
            root.insert(
                String::from("current_final_review_dispatch_id"),
                Value::String(record.dispatch_id.to_owned()),
            );
            root.insert(
                String::from("current_final_review_reviewer_source"),
                Value::String(record.reviewer_source.to_owned()),
            );
            root.insert(
                String::from("current_final_review_reviewer_id"),
                Value::String(record.reviewer_id.to_owned()),
            );
            root.insert(
                String::from("current_final_review_result"),
                Value::String(record.result.to_owned()),
            );
            root.insert(
                String::from("current_final_review_summary_hash"),
                Value::String(record.summary_hash.to_owned()),
            );
            root.insert(
                String::from("current_final_review_record_id"),
                Value::String(record_id),
            );
            root.insert(
                String::from("final_review_state"),
                Value::String(String::from("fresh")),
            );
            if record.result == "pass" {
                root.insert(
                    String::from("harness_phase"),
                    Value::String(if record.browser_qa_required == Some(true) {
                        String::from("qa_pending")
                    } else {
                        String::from("ready_for_branch_completion")
                    }),
                );
            }
            match record.final_review_fingerprint {
                Some(fingerprint) => {
                    root.insert(
                        String::from("last_final_review_artifact_fingerprint"),
                        Value::String(fingerprint.to_owned()),
                    );
                }
                None => {
                    root.insert(
                        String::from("last_final_review_artifact_fingerprint"),
                        Value::Null,
                    );
                }
            }
            if record.browser_qa_required == Some(false) {
                root.insert(
                    String::from("browser_qa_state"),
                    Value::String(String::from("not_required")),
                );
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::Null,
                );
            }
            if final_review_record_changed {
                root.insert(String::from("current_qa_branch_closure_id"), Value::Null);
                root.insert(String::from("current_qa_result"), Value::Null);
                root.insert(String::from("current_qa_summary_hash"), Value::Null);
                root.insert(String::from("current_qa_record_id"), Value::Null);
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::Null,
                );
                if record.browser_qa_required != Some(false) {
                    root.insert(String::from("browser_qa_state"), Value::Null);
                }
            }
            self.dirty = true;
            if final_review_record_changed
                && let Some(previous_qa_record_id) = previous_qa_record_id.as_deref()
            {
                let records = self.browser_qa_record_history_mut()?;
                mark_record_status(records, previous_qa_record_id, "historical");
            }
            Ok(())
        }
    }

    pub(crate) fn record_browser_qa_result(
        &mut self,
        record: BrowserQaResultRecord<'_>,
    ) -> Result<(), JsonFailure> {
        let previous_record_id = json_string(&self.state_payload, "current_qa_record_id");
        let source_plan_revision = record.source_plan_revision.to_string();
        let history = self.browser_qa_record_history_mut()?;
        let record_id = deterministic_record_id(
            "browser-qa",
            &[
                record.branch_closure_id,
                record.final_review_record_id,
                record.source_plan_path,
                source_plan_revision.as_str(),
                record.repo_slug,
                record.branch_name,
                record.base_branch,
                record.reviewed_state_id,
                record.result,
                record.summary_hash,
                record.generated_by_identity,
                record.source_test_plan_fingerprint.unwrap_or("none"),
                record.browser_qa_fingerprint.unwrap_or("none"),
            ],
        );
        let record_sequence = history.len() as u64 + 1;
        if previous_record_id
            .as_deref()
            .is_some_and(|value| value != record_id)
            && let Some(previous_record_id) = previous_record_id.as_deref()
        {
            mark_record_status(history, previous_record_id, "historical");
        }
        history.insert(
            record_id.clone(),
            serde_json::json!({
                "record_id": record_id,
                "record_sequence": record_sequence,
                "record_status": "current",
                "branch_closure_id": record.branch_closure_id,
                "final_review_record_id": record.final_review_record_id,
                "source_plan_path": record.source_plan_path,
                "source_plan_revision": record.source_plan_revision,
                "repo_slug": record.repo_slug,
                "branch_name": record.branch_name,
                "base_branch": record.base_branch,
                "reviewed_state_id": record.reviewed_state_id,
                "result": record.result,
                "browser_qa_fingerprint": record.browser_qa_fingerprint,
                "source_test_plan_fingerprint": record.source_test_plan_fingerprint,
                "summary": record.summary,
                "summary_hash": record.summary_hash,
                "generated_by_identity": record.generated_by_identity,
            }),
        );
        let root = self.root_object_mut()?;
        root.insert(
            String::from("current_qa_branch_closure_id"),
            Value::String(record.branch_closure_id.to_owned()),
        );
        root.insert(
            String::from("current_qa_result"),
            Value::String(record.result.to_owned()),
        );
        root.insert(
            String::from("current_qa_summary_hash"),
            Value::String(record.summary_hash.to_owned()),
        );
        root.insert(
            String::from("current_qa_record_id"),
            Value::String(record_id),
        );
        root.insert(
            String::from("browser_qa_state"),
            Value::String(String::from("fresh")),
        );
        if record.result == "pass" {
            root.insert(
                String::from("harness_phase"),
                Value::String(String::from("ready_for_branch_completion")),
            );
        }
        match record.browser_qa_fingerprint {
            Some(fingerprint) => {
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::String(fingerprint.to_owned()),
                );
            }
            None => {
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::Null,
                );
            }
        }
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn record_task_closure_result(
        &mut self,
        record: TaskClosureResultRecord<'_>,
    ) -> Result<(), JsonFailure> {
        let record_sequence = self
            .state_payload
            .get("task_closure_record_history")
            .and_then(Value::as_object)
            .map_or(1, |history| history.len() as u64 + 1);
        let source_plan_path = self
            .state_payload
            .get("run_identity")
            .and_then(|run_identity| json_string(run_identity, "source_plan_path"));
        let source_plan_revision = self
            .state_payload
            .get("run_identity")
            .and_then(|run_identity| json_u32(run_identity, "source_plan_revision"));
        let execution_run_id = record
            .execution_run_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                self.state_payload
                    .get("run_identity")
                    .and_then(|run_identity| json_string(run_identity, "execution_run_id"))
            });
        let payload = serde_json::json!({
            "task": record.task,
            "record_id": record.closure_record_id,
            "record_sequence": record_sequence,
            "record_status": "current",
            "closure_status": "current",
            "source_plan_path": source_plan_path,
            "source_plan_revision": source_plan_revision,
            "execution_run_id": execution_run_id,
            "dispatch_id": record.dispatch_id,
            "closure_record_id": record.closure_record_id,
            "reviewed_state_id": record.reviewed_state_id,
            "contract_identity": record.contract_identity,
            "effective_reviewed_surface_paths": record.effective_reviewed_surface_paths,
            "review_result": record.review_result,
            "review_summary_hash": record.review_summary_hash,
            "verification_result": record.verification_result,
            "verification_summary_hash": record.verification_summary_hash,
        });
        let records = self.current_task_closure_records_mut()?;
        records.insert(format!("task-{}", record.task), payload.clone());
        self.task_closure_record_history_mut()?
            .insert(record.closure_record_id.to_owned(), payload);
        self.dirty = true;
        Ok(())
    }

    fn remove_current_task_closure_results_with_record_status(
        &mut self,
        tasks: impl IntoIterator<Item = u32>,
        record_status: &str,
    ) -> Result<(), JsonFailure> {
        let tasks = tasks.into_iter().collect::<Vec<_>>();
        let removed_closure_ids = tasks
            .iter()
            .filter_map(|task| {
                self.raw_current_task_closure_state_entry(*task)
                    .and_then(|entry| entry.closure_record_id)
                    .or_else(|| {
                        self.current_task_closure_result(*task)
                            .map(|record| record.closure_record_id)
                    })
            })
            .collect::<Vec<_>>();
        let mut removed_any = false;
        {
            let records = self.current_task_closure_records_mut()?;
            for task in tasks {
                removed_any |= records.remove(&format!("task-{task}")).is_some();
            }
        }
        if !removed_closure_ids.is_empty() {
            let history = self.task_closure_record_history_mut()?;
            for closure_record_id in removed_closure_ids {
                mark_record_status(history, &closure_record_id, record_status);
            }
            removed_any = true;
        }
        if removed_any {
            self.dirty = true;
        }
        Ok(())
    }

    fn remove_current_task_closure_results_with_record_status_by_scope_keys(
        &mut self,
        scope_keys: impl IntoIterator<Item = String>,
        record_status: &str,
    ) -> Result<(), JsonFailure> {
        let scope_keys = scope_keys.into_iter().collect::<Vec<_>>();
        if scope_keys.is_empty() {
            return Ok(());
        }

        let removed_closure_ids = scope_keys
            .iter()
            .filter_map(|scope_key| {
                self.state_payload
                    .get("current_task_closure_records")
                    .and_then(Value::as_object)
                    .and_then(|records| records.get(scope_key))
                    .and_then(|payload| json_string(payload, "closure_record_id"))
            })
            .collect::<Vec<_>>();
        let mut removed_any = false;
        {
            let records = self.current_task_closure_records_mut()?;
            for scope_key in scope_keys {
                removed_any |= records.remove(&scope_key).is_some();
            }
        }
        if !removed_closure_ids.is_empty() {
            let history = self.task_closure_record_history_mut()?;
            for closure_record_id in removed_closure_ids {
                mark_record_status(history, &closure_record_id, record_status);
            }
            removed_any = true;
        }
        if removed_any {
            self.dirty = true;
        }
        Ok(())
    }

    pub(crate) fn remove_current_task_closure_results(
        &mut self,
        tasks: impl IntoIterator<Item = u32>,
    ) -> Result<(), JsonFailure> {
        self.remove_current_task_closure_results_with_record_status(tasks, "superseded")
    }

    pub(crate) fn clear_current_task_closure_results_for_execution_reentry(
        &mut self,
        tasks: impl IntoIterator<Item = u32>,
    ) -> Result<(), JsonFailure> {
        self.remove_current_task_closure_results_with_record_status(tasks, "stale_unreviewed")
    }

    pub(crate) fn clear_current_task_closure_results_for_structural_repair(
        &mut self,
        tasks: impl IntoIterator<Item = u32>,
    ) -> Result<(), JsonFailure> {
        self.remove_current_task_closure_results_with_record_status(tasks, "historical")
    }

    pub(crate) fn clear_current_task_closure_results_for_structural_repair_scope_keys(
        &mut self,
        scope_keys: impl IntoIterator<Item = String>,
    ) -> Result<(), JsonFailure> {
        self.remove_current_task_closure_results_with_record_status_by_scope_keys(
            scope_keys,
            "historical",
        )
    }

    pub(crate) fn record_task_closure_negative_result(
        &mut self,
        record: TaskClosureNegativeResultRecord<'_>,
    ) -> Result<(), JsonFailure> {
        let record_id = format!("task-{}:{}", record.task, record.dispatch_id);
        let record_sequence = self
            .state_payload
            .get("task_closure_negative_result_history")
            .and_then(Value::as_object)
            .map_or(1, |history| history.len() as u64 + 1);
        let payload = serde_json::json!({
            "task": record.task,
            "record_id": record_id,
            "record_sequence": record_sequence,
            "record_status": "current",
            "dispatch_id": record.dispatch_id,
            "closure_record_id": Value::Null,
            "reviewed_state_id": record.reviewed_state_id,
            "contract_identity": record.contract_identity,
            "review_result": record.review_result,
            "review_summary_hash": record.review_summary_hash,
            "verification_result": record.verification_result,
            "verification_summary_hash": record.verification_summary_hash,
        });
        let records = self.task_closure_negative_result_records_mut()?;
        records.insert(format!("task-{}", record.task), payload.clone());
        self.task_closure_negative_result_history_mut()?.insert(
            format!("task-{}:{}", record.task, record.dispatch_id),
            payload,
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn clear_task_closure_negative_result(
        &mut self,
        task: u32,
    ) -> Result<(), JsonFailure> {
        let negative_result = self.task_closure_negative_result(task);
        let removed = self
            .task_closure_negative_result_records_mut()?
            .remove(&format!("task-{task}"))
            .is_some();
        if let Some(ref negative_result) = negative_result {
            let history = self.task_closure_negative_result_history_mut()?;
            mark_record_status(
                history,
                &format!("task-{task}:{}", negative_result.dispatch_id),
                "historical",
            );
        }
        if removed || negative_result.is_some() {
            self.dirty = true;
        }
        Ok(())
    }

    pub(crate) fn current_qa_branch_closure_id(&self) -> Option<&str> {
        self.state_payload
            .get("current_qa_branch_closure_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn current_qa_result(&self) -> Option<&str> {
        self.state_payload
            .get("current_qa_result")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn current_qa_summary_hash(&self) -> Option<&str> {
        self.state_payload
            .get("current_qa_summary_hash")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn current_release_readiness_result(&self) -> Option<&str> {
        self.state_payload
            .get("current_release_readiness_result")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn current_release_readiness_summary_hash(&self) -> Option<&str> {
        self.state_payload
            .get("current_release_readiness_summary_hash")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn current_release_readiness_record_id(&self) -> Option<String> {
        json_string(&self.state_payload, "current_release_readiness_record_id")
    }

    pub(crate) fn current_release_readiness_record(&self) -> Option<CurrentReleaseReadinessRecord> {
        let (record_id, _) = self.current_record_entry(
            "current_release_readiness_record_id",
            "release_readiness_record_history",
        )?;
        self.release_readiness_record_by_id(&record_id)
    }

    pub(crate) fn release_readiness_record_by_id(
        &self,
        record_id: &str,
    ) -> Option<CurrentReleaseReadinessRecord> {
        let payload = self.record_payload_by_id("release_readiness_record_history", record_id)?;
        Some(CurrentReleaseReadinessRecord {
            record_status: json_string(&payload, "record_status")?,
            branch_closure_id: json_string(&payload, "branch_closure_id")?,
            source_plan_path: json_string(&payload, "source_plan_path")?,
            source_plan_revision: json_u32(&payload, "source_plan_revision")?,
            repo_slug: json_string(&payload, "repo_slug")?,
            branch_name: json_string(&payload, "branch_name")?,
            base_branch: json_string(&payload, "base_branch")?,
            reviewed_state_id: json_string(&payload, "reviewed_state_id")?,
            result: json_string(&payload, "result")?,
            release_docs_fingerprint: json_string(&payload, "release_docs_fingerprint"),
            summary: json_string(&payload, "summary")?,
            summary_hash: json_string(&payload, "summary_hash")?,
            generated_by_identity: json_string(&payload, "generated_by_identity")?,
        })
    }

    pub(crate) fn current_final_review_branch_closure_id(&self) -> Option<&str> {
        self.state_payload
            .get("current_final_review_branch_closure_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn current_final_review_dispatch_id(&self) -> Option<&str> {
        self.state_payload
            .get("current_final_review_dispatch_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn current_final_review_reviewer_source(&self) -> Option<&str> {
        self.state_payload
            .get("current_final_review_reviewer_source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn current_final_review_reviewer_id(&self) -> Option<&str> {
        self.state_payload
            .get("current_final_review_reviewer_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn current_final_review_result(&self) -> Option<&str> {
        self.state_payload
            .get("current_final_review_result")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn current_final_review_summary_hash(&self) -> Option<&str> {
        self.state_payload
            .get("current_final_review_summary_hash")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub(crate) fn current_final_review_record_id(&self) -> Option<String> {
        json_string(&self.state_payload, "current_final_review_record_id")
    }

    pub(crate) fn current_final_review_record(&self) -> Option<CurrentFinalReviewRecord> {
        let (record_id, _) = self.current_record_entry(
            "current_final_review_record_id",
            "final_review_record_history",
        )?;
        self.final_review_record_by_id(&record_id)
    }

    pub(crate) fn final_review_record_by_id(
        &self,
        record_id: &str,
    ) -> Option<CurrentFinalReviewRecord> {
        let payload = self.record_payload_by_id("final_review_record_history", record_id)?;
        Some(CurrentFinalReviewRecord {
            record_status: json_string(&payload, "record_status")?,
            branch_closure_id: json_string(&payload, "branch_closure_id")?,
            release_readiness_record_id: json_string(&payload, "release_readiness_record_id"),
            dispatch_id: json_string(&payload, "dispatch_id")?,
            reviewer_source: json_string(&payload, "reviewer_source")?,
            reviewer_id: json_string(&payload, "reviewer_id")?,
            result: json_string(&payload, "result")?,
            final_review_fingerprint: json_string(&payload, "final_review_fingerprint"),
            deviations_required: payload.get("deviations_required").and_then(Value::as_bool),
            browser_qa_required: payload.get("browser_qa_required").and_then(Value::as_bool),
            source_plan_path: json_string(&payload, "source_plan_path")?,
            source_plan_revision: json_u32(&payload, "source_plan_revision")?,
            repo_slug: json_string(&payload, "repo_slug")?,
            branch_name: json_string(&payload, "branch_name")?,
            base_branch: json_string(&payload, "base_branch")?,
            reviewed_state_id: json_string(&payload, "reviewed_state_id")?,
            summary: json_string(&payload, "summary")?,
            summary_hash: json_string(&payload, "summary_hash")?,
        })
    }

    pub(crate) fn raw_current_task_closure_result(
        &self,
        task: u32,
    ) -> Option<CurrentTaskClosureRecord> {
        let payload = self
            .state_payload
            .get("current_task_closure_records")
            .and_then(Value::as_object)?
            .get(&format!("task-{task}"))?
            .as_object()?;
        current_task_closure_record_from_payload(task, payload)
    }

    pub(crate) fn raw_current_task_closure_state_entry(
        &self,
        task: u32,
    ) -> Option<RawCurrentTaskClosureStateEntry> {
        let scope_key = format!("task-{task}");
        let payload = self
            .state_payload
            .get("current_task_closure_records")
            .and_then(Value::as_object)?
            .get(&scope_key)?;
        let closure_record_id = json_string(payload, "closure_record_id");
        let record = payload
            .as_object()
            .and_then(|payload| current_task_closure_record_from_payload(task, payload));
        Some(RawCurrentTaskClosureStateEntry {
            scope_key,
            task: Some(task),
            closure_record_id,
            record,
        })
    }

    pub(crate) fn raw_current_task_closure_state_entries(
        &self,
    ) -> Vec<RawCurrentTaskClosureStateEntry> {
        self.state_payload
            .get("current_task_closure_records")
            .and_then(Value::as_object)
            .map(|records| {
                records
                    .iter()
                    .map(|(scope_key, payload)| {
                        let task = scope_key
                            .strip_prefix("task-")
                            .and_then(|task| task.parse::<u32>().ok());
                        let closure_record_id = json_string(payload, "closure_record_id");
                        let record = task.and_then(|task| {
                            payload.as_object().and_then(|payload| {
                                current_task_closure_record_from_payload(task, payload)
                            })
                        });
                        RawCurrentTaskClosureStateEntry {
                            scope_key: scope_key.clone(),
                            task,
                            closure_record_id,
                            record,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    pub(crate) fn raw_current_task_closure_results(
        &self,
    ) -> BTreeMap<u32, CurrentTaskClosureRecord> {
        self.state_payload
            .get("current_task_closure_records")
            .and_then(Value::as_object)
            .map(|records| {
                records
                    .keys()
                    .filter_map(|key| {
                        key.strip_prefix("task-")
                            .and_then(|task| task.parse::<u32>().ok())
                            .and_then(|task| self.raw_current_task_closure_result(task))
                            .map(|record| (record.task, record))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default()
    }

    fn current_task_closure_results_from_history(&self) -> BTreeMap<u32, CurrentTaskClosureRecord> {
        self.state_payload
            .get("task_closure_record_history")
            .and_then(Value::as_object)
            .map(|history| {
                history
                    .values()
                    .filter_map(Value::as_object)
                    .filter_map(|payload| {
                        let payload_value = Value::Object(payload.clone());
                        if json_string(&payload_value, "record_status").as_deref()
                            != Some("current")
                        {
                            return None;
                        }
                        let task = json_u32(&payload_value, "task")?;
                        current_task_closure_record_from_payload(task, payload)
                            .map(|record| (record.task, record))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default()
    }

    pub(crate) fn current_task_closure_result(
        &self,
        task: u32,
    ) -> Option<CurrentTaskClosureRecord> {
        self.current_task_closure_results_from_history()
            .remove(&task)
            .or_else(|| self.raw_current_task_closure_result(task))
    }

    pub(crate) fn current_task_closure_results(&self) -> BTreeMap<u32, CurrentTaskClosureRecord> {
        let mut records = self.raw_current_task_closure_results();
        records.extend(self.current_task_closure_results_from_history());
        records
    }

    pub(crate) fn task_closure_history_contains_task(&self, task: u32) -> bool {
        self.state_payload
            .get("task_closure_record_history")
            .and_then(Value::as_object)
            .is_some_and(|history| {
                history
                    .values()
                    .filter_map(Value::as_object)
                    .any(|payload| {
                        payload.get("task").and_then(Value::as_u64) == Some(u64::from(task))
                    })
            })
    }

    pub(crate) fn current_task_closure_overlay_needs_restore(&self) -> bool {
        let recoverable = self.current_task_closure_results_from_history();
        if recoverable.is_empty() {
            return false;
        }
        self.raw_current_task_closure_results() != recoverable
    }

    pub(crate) fn restore_current_task_closure_records_from_history(
        &mut self,
    ) -> Result<bool, JsonFailure> {
        let recoverable = self.current_task_closure_results_from_history();
        if recoverable.is_empty() || !self.current_task_closure_overlay_needs_restore() {
            return Ok(false);
        }
        let mut restored = self
            .state_payload
            .get("current_task_closure_records")
            .and_then(Value::as_object)
            .map(|records| {
                records
                    .iter()
                    .filter_map(|(scope_key, payload)| {
                        let parsed_task = scope_key
                            .strip_prefix("task-")
                            .and_then(|task| task.parse::<u32>().ok());
                        parsed_task
                            .is_none()
                            .then(|| (scope_key.clone(), payload.clone()))
                    })
                    .collect::<serde_json::Map<_, _>>()
            })
            .unwrap_or_default();
        for record in recoverable.into_values() {
            let closure_status = record
                .closure_status
                .clone()
                .unwrap_or_else(|| String::from("current"));
            restored.insert(
                format!("task-{}", record.task),
                serde_json::json!({
                    "task": record.task,
                    "record_id": record.closure_record_id,
                    "record_status": "current",
                    "closure_status": closure_status,
                    "source_plan_path": record.source_plan_path,
                    "source_plan_revision": record.source_plan_revision,
                    "execution_run_id": record.execution_run_id,
                    "dispatch_id": record.dispatch_id,
                    "closure_record_id": record.closure_record_id,
                    "reviewed_state_id": record.reviewed_state_id,
                    "contract_identity": record.contract_identity,
                    "effective_reviewed_surface_paths": record.effective_reviewed_surface_paths,
                    "review_result": record.review_result,
                    "review_summary_hash": record.review_summary_hash,
                    "verification_result": record.verification_result,
                    "verification_summary_hash": record.verification_summary_hash,
                }),
            );
        }
        self.root_object_mut()?.insert(
            String::from("current_task_closure_records"),
            Value::Object(restored),
        );
        self.dirty = true;
        Ok(true)
    }

    pub(crate) fn branch_closure_record(
        &self,
        branch_closure_id: &str,
    ) -> Option<BranchClosureRecord> {
        let payload = self
            .state_payload
            .get("branch_closure_records")
            .and_then(Value::as_object)?
            .get(branch_closure_id)?
            .as_object()?;
        let payload_value = Value::Object(payload.clone());
        if json_string(&payload_value, "branch_closure_id")?.trim() != branch_closure_id {
            return None;
        }
        if json_string(&payload_value, "closure_status")?.trim() != "current" {
            return None;
        }
        let source_plan_path = json_string(&payload_value, "source_plan_path")?;
        let source_plan_revision = json_u32(&payload_value, "source_plan_revision")?;
        let repo_slug = json_string(&payload_value, "repo_slug")?;
        let branch_name = json_string(&payload_value, "branch_name")?;
        let base_branch = json_string(&payload_value, "base_branch")?;
        let effective_reviewed_branch_surface =
            json_string(&payload_value, "effective_reviewed_branch_surface")?;
        let provenance_basis = json_string(&payload_value, "provenance_basis")?;
        let source_task_closure_ids =
            json_string_array_strict(&payload_value, "source_task_closure_ids")?;
        let _ = json_string_array_strict(&payload_value, "superseded_branch_closure_ids")?;
        match provenance_basis.as_str() {
            "task_closure_lineage" => {
                if source_task_closure_ids.is_empty()
                    || effective_reviewed_branch_surface != "repo_tracked_content"
                {
                    return None;
                }
            }
            "task_closure_lineage_plus_late_stage_surface_exemption" => {
                if source_task_closure_ids.is_empty() {
                    if !well_formed_late_stage_surface_only_branch_surface(
                        &effective_reviewed_branch_surface,
                    ) {
                        return None;
                    }
                } else if effective_reviewed_branch_surface != "repo_tracked_content" {
                    return None;
                }
            }
            _ => return None,
        }
        Some(BranchClosureRecord {
            source_plan_path,
            source_plan_revision,
            repo_slug,
            branch_name,
            base_branch,
            reviewed_state_id: json_string(&payload_value, "reviewed_state_id")?,
            contract_identity: json_string(&payload_value, "contract_identity")?,
            effective_reviewed_branch_surface,
            source_task_closure_ids,
            provenance_basis,
            branch_closure_fingerprint: json_string(&payload_value, "branch_closure_fingerprint"),
        })
    }

    pub(crate) fn recoverable_current_branch_closure_identity(
        &self,
    ) -> Option<CurrentBranchClosureIdentity> {
        if let Some(identity) = self.bound_current_branch_closure_identity() {
            return Some(identity);
        }

        let records = self
            .state_payload
            .get("branch_closure_records")
            .and_then(Value::as_object)?;
        let mut candidates = records.keys().filter_map(|branch_closure_id| {
            self.branch_closure_record(branch_closure_id).map(|record| {
                CurrentBranchClosureIdentity {
                    branch_closure_id: branch_closure_id.clone(),
                    reviewed_state_id: record.reviewed_state_id,
                    contract_identity: record.contract_identity,
                }
            })
        });
        let current = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        Some(current)
    }

    pub(crate) fn bound_current_branch_closure_identity(
        &self,
    ) -> Option<CurrentBranchClosureIdentity> {
        if let Some(branch_closure_id) =
            json_string(&self.state_payload, "current_branch_closure_id")
            && let Some(record) = self.branch_closure_record(&branch_closure_id)
        {
            return Some(CurrentBranchClosureIdentity {
                branch_closure_id,
                reviewed_state_id: record.reviewed_state_id,
                contract_identity: record.contract_identity,
            });
        }
        None
    }

    pub(crate) fn current_branch_closure_overlay_id(&self) -> Option<String> {
        json_string(&self.state_payload, "current_branch_closure_id")
    }

    pub(crate) fn current_branch_closure_overlay_reviewed_state_id(&self) -> Option<String> {
        json_string(
            &self.state_payload,
            "current_branch_closure_reviewed_state_id",
        )
    }

    pub(crate) fn current_branch_closure_overlay_contract_identity(&self) -> Option<String> {
        json_string(
            &self.state_payload,
            "current_branch_closure_contract_identity",
        )
    }

    pub(crate) fn finish_review_gate_pass_branch_closure_id(&self) -> Option<String> {
        json_string(
            &self.state_payload,
            "finish_review_gate_pass_branch_closure_id",
        )
    }

    pub(crate) fn task_review_dispatch_id(&self, task: u32) -> Option<String> {
        let payload = self
            .state_payload
            .get("strategy_review_dispatch_lineage")
            .and_then(Value::as_object)?
            .get(&format!("task-{task}"))?;
        json_string(payload, "dispatch_id")
    }

    pub(crate) fn append_superseded_task_closure_ids<'a>(
        &mut self,
        closure_ids: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), JsonFailure> {
        append_unique_string_array(
            self.root_object_mut()?,
            "superseded_task_closure_ids",
            closure_ids,
        )?;
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn superseded_task_closure_ids(&self) -> Vec<String> {
        json_string_array(&self.state_payload, "superseded_task_closure_ids")
    }

    pub(crate) fn append_superseded_branch_closure_ids<'a>(
        &mut self,
        closure_ids: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), JsonFailure> {
        append_unique_string_array(
            self.root_object_mut()?,
            "superseded_branch_closure_ids",
            closure_ids,
        )?;
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn superseded_branch_closure_ids(&self) -> Vec<String> {
        json_string_array(&self.state_payload, "superseded_branch_closure_ids")
    }

    pub(crate) fn raw_task_closure_negative_result(
        &self,
        task: u32,
    ) -> Option<TaskClosureNegativeResult> {
        let payload = self
            .state_payload
            .get("task_closure_negative_result_records")
            .and_then(Value::as_object)?
            .get(&format!("task-{task}"))?
            .as_object()?;
        task_closure_negative_result_from_payload(payload)
    }

    fn task_closure_negative_results_from_history(
        &self,
    ) -> BTreeMap<u32, TaskClosureNegativeResult> {
        self.state_payload
            .get("task_closure_negative_result_history")
            .and_then(Value::as_object)
            .map(|history| {
                history
                    .values()
                    .filter_map(Value::as_object)
                    .filter_map(|payload| {
                        let payload_value = Value::Object(payload.clone());
                        if json_string(&payload_value, "record_status").as_deref()
                            != Some("current")
                        {
                            return None;
                        }
                        let task = json_u32(&payload_value, "task")?;
                        task_closure_negative_result_from_payload(payload)
                            .map(|record| (task, record))
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default()
    }

    pub(crate) fn task_closure_negative_result(
        &self,
        task: u32,
    ) -> Option<TaskClosureNegativeResult> {
        self.task_closure_negative_results_from_history()
            .remove(&task)
            .or_else(|| self.raw_task_closure_negative_result(task))
    }

    pub(crate) fn task_closure_negative_result_overlay_needs_restore(&self) -> bool {
        let recoverable = self.task_closure_negative_results_from_history();
        if recoverable.is_empty() {
            return false;
        }
        let raw = self
            .state_payload
            .get("task_closure_negative_result_records")
            .and_then(Value::as_object)
            .map(|records| {
                records
                    .keys()
                    .filter_map(|key| {
                        key.strip_prefix("task-")
                            .and_then(|task| task.parse::<u32>().ok())
                            .and_then(|task| {
                                self.raw_task_closure_negative_result(task)
                                    .map(|record| (task, record))
                            })
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        raw != recoverable
    }

    pub(crate) fn restore_task_closure_negative_result_records_from_history(
        &mut self,
    ) -> Result<bool, JsonFailure> {
        let recoverable = self.task_closure_negative_results_from_history();
        if recoverable.is_empty() || !self.task_closure_negative_result_overlay_needs_restore() {
            return Ok(false);
        }
        let mut restored = serde_json::Map::new();
        for (task, record) in recoverable {
            restored.insert(
                format!("task-{task}"),
                serde_json::json!({
                    "task": task,
                    "record_status": "current",
                    "dispatch_id": record.dispatch_id,
                    "closure_record_id": Value::Null,
                    "reviewed_state_id": record.reviewed_state_id,
                    "contract_identity": record.contract_identity,
                    "review_result": record.review_result,
                    "review_summary_hash": record.review_summary_hash,
                    "verification_result": record.verification_result,
                    "verification_summary_hash": record.verification_summary_hash,
                }),
            );
        }
        self.root_object_mut()?.insert(
            String::from("task_closure_negative_result_records"),
            Value::Object(restored),
        );
        self.dirty = true;
        Ok(true)
    }

    pub(crate) fn current_browser_qa_record(&self) -> Option<CurrentBrowserQaRecord> {
        let (record_id, _) =
            self.current_record_entry("current_qa_record_id", "browser_qa_record_history")?;
        self.browser_qa_record_by_id(&record_id)
    }

    pub(crate) fn browser_qa_record_by_id(
        &self,
        record_id: &str,
    ) -> Option<CurrentBrowserQaRecord> {
        let payload = self.record_payload_by_id("browser_qa_record_history", record_id)?;
        Some(CurrentBrowserQaRecord {
            record_status: json_string(&payload, "record_status")?,
            branch_closure_id: json_string(&payload, "branch_closure_id")?,
            final_review_record_id: json_string(&payload, "final_review_record_id"),
            source_plan_path: json_string(&payload, "source_plan_path")?,
            source_plan_revision: json_u32(&payload, "source_plan_revision")?,
            repo_slug: json_string(&payload, "repo_slug")?,
            branch_name: json_string(&payload, "branch_name")?,
            // Preserve explicit-but-empty bindings so gate logic can emit
            // base-branch unresolved reason codes instead of collapsing to
            // a generic missing-record failure.
            base_branch: json_string_allow_empty(&payload, "base_branch")?,
            reviewed_state_id: json_string(&payload, "reviewed_state_id")?,
            result: json_string(&payload, "result")?,
            browser_qa_fingerprint: json_string(&payload, "browser_qa_fingerprint"),
            source_test_plan_fingerprint: json_string(&payload, "source_test_plan_fingerprint"),
            summary: json_string(&payload, "summary")?,
            summary_hash: json_string(&payload, "summary_hash")?,
            generated_by_identity: json_string(&payload, "generated_by_identity")?,
        })
    }

    pub(crate) fn current_qa_record_id(&self) -> Option<String> {
        json_string(&self.state_payload, "current_qa_record_id")
    }

    fn current_record_entry(
        &self,
        current_id_key: &str,
        history_key: &str,
    ) -> Option<(String, Value)> {
        let history = self
            .state_payload
            .get(history_key)
            .and_then(Value::as_object)?;
        if let Some(record_id) = self
            .state_payload
            .get(current_id_key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            && let Some(payload) = history.get(record_id).cloned()
            && json_string(&payload, "record_status").as_deref() == Some("current")
        {
            return Some((record_id.to_owned(), payload));
        }

        // Late-stage milestone identity must be explicitly persisted via current_*_record_id
        // bindings. Do not infer "current" from history for these surfaces.
        if matches!(
            current_id_key,
            "current_release_readiness_record_id"
                | "current_final_review_record_id"
                | "current_qa_record_id"
        ) {
            return None;
        }

        let mut current_records = history
            .iter()
            .filter(|(_, payload)| {
                json_string(payload, "record_status").as_deref() == Some("current")
            })
            .map(|(record_id, payload)| (record_id.clone(), payload.clone()));
        let current_record = current_records.next()?;
        if current_records.next().is_some() {
            return None;
        }
        Some(current_record)
    }

    fn record_payload_by_id(&self, history_key: &str, record_id: &str) -> Option<Value> {
        let record_id = record_id.trim();
        if record_id.is_empty() {
            return None;
        }
        self.state_payload
            .get(history_key)
            .and_then(Value::as_object)?
            .get(record_id)
            .cloned()
    }

    pub(crate) fn current_open_step_state(&self) -> Option<OpenStepStateRecord> {
        self.current_open_step_state_checked().ok().flatten()
    }

    pub(crate) fn current_open_step_state_checked(
        &self,
    ) -> Result<Option<OpenStepStateRecord>, JsonFailure> {
        let Some(payload) = self.state_payload.get("current_open_step_state") else {
            return Ok(None);
        };
        if payload.is_null() {
            return Ok(None);
        }
        let record: OpenStepStateRecord =
            serde_json::from_value(payload.clone()).map_err(|error| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    format!(
                        "Authoritative harness current_open_step_state is malformed in {}: {error}",
                        self.state_path.display()
                    ),
                )
            })?;
        Ok(Some(normalize_open_step_state_record(
            record,
            &self.state_path,
        )?))
    }

    pub(crate) fn record_open_step_state(
        &mut self,
        record: OpenStepStateRecord,
    ) -> Result<(), JsonFailure> {
        let record = normalize_open_step_state_record(record, &self.state_path)?;
        let state_path = self.state_path.display().to_string();
        let serialized_record = serde_json::to_value(record).map_err(|error| {
            JsonFailure::new(
                FailureClass::PartialAuthoritativeMutation,
                format!(
                    "Could not serialize authoritative current_open_step_state for {state_path}: {error}"
                ),
            )
        })?;
        let root = self.root_object_mut()?;
        root.insert(String::from("current_open_step_state"), serialized_record);
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn clear_open_step_state(&mut self) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        if root.get("current_open_step_state").is_some() {
            root.insert(String::from("current_open_step_state"), Value::Null);
            self.dirty = true;
        }
        Ok(())
    }

    pub(crate) fn harness_phase_opt(&self) -> Option<String> {
        self.state_payload
            .get("harness_phase")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }

    pub(crate) fn chunk_id_opt(&self) -> Option<String> {
        self.state_payload
            .get("chunk_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }

    pub(crate) fn execution_run_id_opt(&self) -> Option<String> {
        self.state_payload
            .get("run_identity")
            .and_then(Value::as_object)
            .and_then(|run_identity| run_identity.get("execution_run_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }

    fn last_strategy_checkpoint_fingerprint(&self) -> Option<String> {
        self.state_payload
            .get("last_strategy_checkpoint_fingerprint")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }

    fn increment_task_dispatch_credit(&mut self, task: u32) -> Result<u64, JsonFailure> {
        let key = format!("task-{task}");
        let credits = self.dispatch_credit_counts_mut()?;
        credits.insert(key, Value::Number(1_u64.into()));
        self.dirty = true;
        Ok(1)
    }

    fn consume_task_dispatch_credit(&mut self, task: u32) -> Result<bool, JsonFailure> {
        let key = format!("task-{task}");
        let credits = self.dispatch_credit_counts_mut()?;
        if !credits.contains_key(&key) {
            return Ok(false);
        }
        credits.remove(&key);
        self.dirty = true;
        Ok(true)
    }

    fn increment_unbound_dispatch_credit(&mut self) -> Result<u64, JsonFailure> {
        let credits = self.dispatch_credit_counts_mut()?;
        credits.insert(String::from("unbound"), Value::Number(1_u64.into()));
        self.dirty = true;
        Ok(1)
    }

    fn consume_unbound_dispatch_credit(&mut self) -> Result<bool, JsonFailure> {
        let credits = self.dispatch_credit_counts_mut()?;
        let key = String::from("unbound");
        if !credits.contains_key(&key) {
            return Ok(false);
        }
        credits.remove(&key);
        self.dirty = true;
        Ok(true)
    }

    fn clear_dispatch_credits(&mut self) -> Result<(), JsonFailure> {
        let credits = self.dispatch_credit_counts_mut()?;
        if credits.is_empty() {
            return Ok(());
        }
        credits.clear();
        self.dirty = true;
        Ok(())
    }

    fn clear_task_dispatch_credits(&mut self) -> Result<Vec<u32>, JsonFailure> {
        let credits = self.dispatch_credit_counts_mut()?;
        let keys = credits
            .keys()
            .filter(|key| key.starts_with("task-"))
            .cloned()
            .collect::<Vec<_>>();
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let tasks = keys
            .iter()
            .filter_map(|key| key.strip_prefix("task-"))
            .filter_map(|value| value.parse::<u32>().ok())
            .collect::<Vec<_>>();
        for key in keys {
            credits.remove(&key);
        }
        self.dirty = true;
        Ok(tasks)
    }

    pub(crate) fn evidence_provenance(&self) -> StepEvidenceProvenance {
        StepEvidenceProvenance {
            source_contract_path: json_string(&self.state_payload, "active_contract_path"),
            source_contract_fingerprint: json_string(
                &self.state_payload,
                "active_contract_fingerprint",
            ),
            source_evaluation_report_fingerprint: json_string(
                &self.state_payload,
                "last_evaluation_report_fingerprint",
            ),
            evaluator_verdict: json_string(&self.state_payload, "last_evaluation_verdict"),
            failing_criterion_ids: json_string_array(&self.state_payload, "open_failed_criteria"),
            source_handoff_fingerprint: json_string(
                &self.state_payload,
                "last_handoff_fingerprint",
            ),
            repo_state_baseline_head_sha: json_string(
                &self.state_payload,
                "repo_state_baseline_head_sha",
            ),
            repo_state_baseline_worktree_fingerprint: json_string(
                &self.state_payload,
                "repo_state_baseline_worktree_fingerprint",
            ),
        }
    }

    pub(crate) fn persist_if_dirty_with_failpoint(
        &self,
        failpoint: Option<&str>,
    ) -> Result<(), JsonFailure> {
        if !self.dirty {
            return Ok(());
        }
        maybe_trigger_authoritative_state_failpoint(failpoint)?;
        let serialized = serde_json::to_string_pretty(&self.state_payload).map_err(|error| {
            JsonFailure::new(
                FailureClass::PartialAuthoritativeMutation,
                format!(
                    "Could not serialize authoritative harness state mutation {}: {error}",
                    self.state_path.display()
                ),
            )
        })?;
        write_atomic_file(&self.state_path, serialized).map_err(|error| {
            JsonFailure::new(
                FailureClass::PartialAuthoritativeMutation,
                format!(
                    "Could not persist authoritative harness state {}: {error}",
                    self.state_path.display()
                ),
            )
        })?;
        refresh_authoritative_state_payload_cache(&self.state_path, &self.state_payload);
        Ok(())
    }

    fn root_object_mut(&mut self) -> Result<&mut serde_json::Map<String, Value>, JsonFailure> {
        self.state_payload.as_object_mut().ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative harness state is malformed in {}: expected a JSON object root.",
                    self.state_path.display()
                ),
            )
        })
    }
}

fn well_formed_late_stage_surface_only_branch_surface(value: &str) -> bool {
    parse_late_stage_surface_only_branch_surface(value).is_some_and(|entries| !entries.is_empty())
}

fn load_authoritative_transition_state_internal(
    context: &ExecutionContext,
    require_active_contract: bool,
) -> Result<Option<AuthoritativeTransitionState>, JsonFailure> {
    {
        let state_path = harness_state_path(
            &context.runtime.state_dir,
            &context.runtime.repo_slug,
            &context.runtime.branch_name,
        );
        let metadata = match fs::symlink_metadata(&state_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                invalidate_authoritative_state_payload_cache(&state_path);
                return Ok(None);
            }
            Err(error) => {
                return Err(JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    format!(
                        "Could not inspect authoritative harness state {}: {error}",
                        state_path.display()
                    ),
                ));
            }
        };
        if metadata.file_type().is_symlink() {
            invalidate_authoritative_state_payload_cache(&state_path);
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative harness state path must not be a symlink in {}.",
                    state_path.display()
                ),
            ));
        }
        if !metadata.is_file() {
            invalidate_authoritative_state_payload_cache(&state_path);
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative harness state must be a regular file in {}.",
                    state_path.display()
                ),
            ));
        }
        let stamp = authoritative_state_cache_stamp(&metadata);
        let cached = lock_authoritative_state_payload_cache()
            .get(&state_path)
            .cloned();
        let (state_payload, gate_state) = if let Some(cached) = cached
            && cached.stamp == stamp
        {
            (cached.state_payload, cached.gate_state)
        } else {
            let source = fs::read_to_string(&state_path).map_err(|error| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    format!(
                        "Could not read authoritative harness state {}: {error}",
                        state_path.display()
                    ),
                )
            })?;
            let state_payload: Value = serde_json::from_str(&source).map_err(|error| {
                JsonFailure::new(
                    FailureClass::MalformedExecutionState,
                    format!(
                        "Authoritative harness state is malformed in {}: {error}",
                        state_path.display()
                    ),
                )
            })?;
            let gate_state: GateAuthorityState = serde_json::from_value(state_payload.clone())
                .map_err(|error| {
                    JsonFailure::new(
                        FailureClass::MalformedExecutionState,
                        format!(
                            "Authoritative harness state is malformed in {}: {error}",
                            state_path.display()
                        ),
                    )
                })?;
            lock_authoritative_state_payload_cache().insert(
                state_path.clone(),
                CachedAuthoritativeStatePayload {
                    stamp,
                    state_payload: state_payload.clone(),
                    gate_state: gate_state.clone(),
                },
            );
            (state_payload, gate_state)
        };

        let active_contract = if require_active_contract && has_active_contract_pointer(&gate_state)
        {
            let mut gate = GateState::default();
            let active = require_active_contract_state(context, &gate_state, &mut gate);
            if !gate.allowed {
                return Err(gate_failure(
                    gate,
                    FailureClass::NonAuthoritativeArtifact,
                    "Could not load active authoritative contract state.",
                ));
            }
            if active.is_none() {
                return Err(JsonFailure::new(
                    FailureClass::NonAuthoritativeArtifact,
                    "Could not load active authoritative contract state.",
                ));
            }
            active
        } else {
            None
        };

        Ok(Some(AuthoritativeTransitionState {
            state_path,
            state_payload,
            phase: gate_state.harness_phase,
            active_contract,
            dirty: false,
        }))
    }
}

pub(crate) fn read_authoritative_transition_state_source(
    state_path: &Path,
) -> Result<Option<String>, JsonFailure> {
    let metadata = match fs::symlink_metadata(state_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Could not inspect authoritative harness state {}: {error}",
                    state_path.display()
                ),
            ));
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative harness state path must not be a symlink in {}.",
                state_path.display()
            ),
        ));
    }
    if !metadata.is_file() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative harness state must be a regular file in {}.",
                state_path.display()
            ),
        ));
    }

    let source = fs::read_to_string(state_path).map_err(|error| {
        JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Could not read authoritative harness state {}: {error}",
                state_path.display()
            ),
        )
    })?;
    Ok(Some(source))
}

pub(crate) fn load_authoritative_transition_state(
    context: &ExecutionContext,
) -> Result<Option<AuthoritativeTransitionState>, JsonFailure> {
    load_authoritative_transition_state_internal(context, true)
}

pub(crate) fn load_or_initialize_authoritative_transition_state(
    context: &ExecutionContext,
) -> Result<AuthoritativeTransitionState, JsonFailure> {
    if let Some(state) = load_authoritative_transition_state(context)? {
        return Ok(state);
    }
    Ok(AuthoritativeTransitionState {
        state_path: harness_state_path(
            &context.runtime.state_dir,
            &context.runtime.repo_slug,
            &context.runtime.branch_name,
        ),
        state_payload: Value::Object(serde_json::Map::new()),
        phase: None,
        active_contract: None,
        dirty: false,
    })
}

pub(crate) fn load_authoritative_transition_state_relaxed(
    context: &ExecutionContext,
) -> Result<Option<AuthoritativeTransitionState>, JsonFailure> {
    load_authoritative_transition_state_internal(context, false)
}

pub(crate) fn materialize_legacy_open_step_state_if_needed(
    runtime: &ExecutionRuntime,
    context: &ExecutionContext,
) -> Result<bool, JsonFailure> {
    let mut authoritative_state = load_authoritative_transition_state_relaxed(context)?;
    if let Some(authoritative_state) = authoritative_state.as_mut() {
        if authoritative_state.current_open_step_state().is_some() {
            return Ok(false);
        }
        if authoritative_state
            .current_open_step_state_checked()?
            .is_some()
        {
            return Ok(false);
        }
        let Some(candidate) = single_legacy_open_step_state_candidate(context)? else {
            return Ok(false);
        };
        authoritative_state.record_open_step_state(candidate)?;
        authoritative_state.persist_if_dirty_with_failpoint(None)?;
        return Ok(true);
    }

    let Some(candidate) = single_legacy_open_step_state_candidate(context)? else {
        return Ok(false);
    };
    let state_path =
        harness_state_path(&runtime.state_dir, &runtime.repo_slug, &runtime.branch_name);
    let mut authoritative_state = AuthoritativeTransitionState {
        state_path,
        state_payload: Value::Object(serde_json::Map::new()),
        phase: None,
        active_contract: None,
        dirty: false,
    };
    authoritative_state.record_open_step_state(candidate)?;
    authoritative_state.persist_if_dirty_with_failpoint(None)?;
    Ok(true)
}

fn single_legacy_open_step_state_candidate(
    context: &ExecutionContext,
) -> Result<Option<OpenStepStateRecord>, JsonFailure> {
    let candidate = match context.legacy_open_step_state_candidates.as_slice() {
        [] => return Ok(None),
        [candidate] => candidate,
        _ => {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Execution plan markdown contains multiple open-step execution notes for {}; persist a single authoritative open step before running mutating commands.",
                    context.plan_rel
                ),
            ));
        }
    };

    let Some(step) = context
        .steps
        .iter()
        .find(|step| step.task_number == candidate.task && step.step_number == candidate.step)
    else {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Legacy open-step execution note in {} points to missing Task {} Step {}.",
                context.plan_rel, candidate.task, candidate.step
            ),
        ));
    };
    if step.checked {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Legacy open-step execution note in {} points to completed Task {} Step {}.",
                context.plan_rel, candidate.task, candidate.step
            ),
        ));
    }
    Ok(Some(candidate.clone()))
}

pub(crate) fn enforce_authoritative_phase(
    authority: Option<&AuthoritativeTransitionState>,
    command: StepCommand,
) -> Result<(), JsonFailure> {
    let Some(authority) = authority else {
        return Ok(());
    };
    if json_bool(&authority.state_payload, "strategy_reset_required") {
        return Err(JsonFailure::new(
            FailureClass::BlockedOnPlanPivot,
            format!(
                "{} is blocked while runtime strategy reset is required.",
                command.as_str()
            ),
        ));
    }
    let phase = authority
        .phase
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match phase {
        Some("handoff_required") => Err(JsonFailure::new(
            FailureClass::IllegalHarnessPhase,
            format!(
                "{} is blocked while the authoritative harness phase is handoff_required.",
                command.as_str()
            ),
        )),
        Some("pivot_required") => Err(JsonFailure::new(
            FailureClass::BlockedOnPlanPivot,
            format!(
                "{} is blocked while the authoritative harness phase is pivot_required.",
                command.as_str()
            ),
        )),
        _ => Ok(()),
    }
}

pub(crate) fn enforce_active_contract_scope(
    authority: Option<&AuthoritativeTransitionState>,
    command: StepCommand,
    task: u32,
    step: u32,
) -> Result<(), JsonFailure> {
    let Some(authority) = authority else {
        return Ok(());
    };
    let Some(active_contract) = authority.active_contract.as_ref() else {
        return Ok(());
    };
    let covered_steps = parse_contract_scope(&active_contract.contract.covered_steps)?;
    if covered_steps.contains(&(task, step)) {
        return Ok(());
    }

    Err(JsonFailure::new(
        FailureClass::ContractMismatch,
        format!(
            "{} target Task {} Step {} is outside the active authoritative contract scope.",
            command.as_str(),
            task,
            step
        ),
    ))
}

fn parse_contract_scope(covered_steps: &[String]) -> Result<BTreeSet<(u32, u32)>, JsonFailure> {
    let mut parsed = BTreeSet::new();
    for step in covered_steps {
        let Some(step_ref) = parse_task_step_scope(step) else {
            return Err(JsonFailure::new(
                FailureClass::ContractMismatch,
                "Execution contract covered_steps entries must use `Task <n> Step <m>` scope format.",
            ));
        };
        parsed.insert(step_ref);
    }
    Ok(parsed)
}

fn parse_task_step_scope(value: &str) -> Option<(u32, u32)> {
    let mut parts = value.split_whitespace();
    if parts.next()? != "Task" {
        return None;
    }
    let task = parts.next()?.parse::<u32>().ok()?;
    if parts.next()? != "Step" {
        return None;
    }
    let step = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((task, step))
}

fn has_active_contract_pointer(state: &GateAuthorityState) -> bool {
    state
        .active_contract_path
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        || state
            .active_contract_fingerprint
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
}

fn gate_failure(
    gate: GateState,
    default_class: FailureClass,
    default_message: &str,
) -> JsonFailure {
    let error_class = if gate.failure_class.is_empty() {
        default_class.as_str().to_owned()
    } else {
        gate.failure_class
    };
    let message = gate.diagnostics.first().map_or_else(
        || default_message.to_owned(),
        |diagnostic| diagnostic.message.clone(),
    );
    JsonFailure {
        error_class,
        message,
    }
}

fn maybe_trigger_authoritative_state_failpoint(failpoint: Option<&str>) -> Result<(), JsonFailure> {
    let Some(failpoint) = failpoint else {
        return Ok(());
    };
    if std::env::var("FEATUREFORGE_PLAN_EXECUTION_TEST_FAILPOINT")
        .ok()
        .as_deref()
        == Some(failpoint)
    {
        return Err(JsonFailure::new(
            FailureClass::PartialAuthoritativeMutation,
            format!("Injected plan execution failpoint: {failpoint}"),
        ));
    }
    Ok(())
}

fn json_string(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn json_string_allow_empty(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_owned)
}

fn json_u64(payload: &Value, key: &str) -> u64 {
    payload.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn json_u32(payload: &Value, key: &str) -> Option<u32> {
    payload
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn json_bool(payload: &Value, key: &str) -> bool {
    payload.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn normalize_open_step_state_record(
    mut record: OpenStepStateRecord,
    state_path: &Path,
) -> Result<OpenStepStateRecord, JsonFailure> {
    if record.task == 0 || record.step == 0 {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative harness current_open_step_state is malformed in {}: task and step must be positive integers.",
                state_path.display()
            ),
        ));
    }

    record.note_state = match record.note_state.trim() {
        "Active" => String::from("Active"),
        "Blocked" => String::from("Blocked"),
        "Interrupted" => String::from("Interrupted"),
        _ => {
            return Err(JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative harness current_open_step_state is malformed in {}: note_state must be Active, Blocked, or Interrupted.",
                    state_path.display()
                ),
            ));
        }
    };

    record.note_summary = record
        .note_summary
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if record.note_summary.is_empty() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative harness current_open_step_state is malformed in {}: note_summary may not be blank.",
                state_path.display()
            ),
        ));
    }
    if record.note_summary.chars().count() > 120 {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative harness current_open_step_state is malformed in {}: note_summary may not exceed 120 characters.",
                state_path.display()
            ),
        ));
    }
    if record.source_plan_path.trim().is_empty() {
        return Err(JsonFailure::new(
            FailureClass::MalformedExecutionState,
            format!(
                "Authoritative harness current_open_step_state is malformed in {}: source_plan_path may not be blank.",
                state_path.display()
            ),
        ));
    }
    Ok(record)
}

fn json_string_array(payload: &Value, key: &str) -> Vec<String> {
    payload
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn json_string_array_strict(payload: &Value, key: &str) -> Option<Vec<String>> {
    let values = payload.get(key)?.as_array()?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

fn append_unique_string_array<'a>(
    root: &mut serde_json::Map<String, Value>,
    key: &str,
    values: impl IntoIterator<Item = &'a str>,
) -> Result<(), JsonFailure> {
    let entry = root
        .entry(String::from(key))
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(items) = entry.as_array_mut() else {
        return Err(JsonFailure::new(
            FailureClass::ExecutionStateNotReady,
            format!("Authoritative harness {key} must be a JSON array."),
        ));
    };
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !items.iter().any(|item| item.as_str() == Some(trimmed)) {
            items.push(Value::String(trimmed.to_owned()));
        }
    }
    Ok(())
}

fn record_sequence_from_dispatch_record(record: &Value) -> Option<u64> {
    record
        .as_object()
        .and_then(|record| record.get("record_sequence"))
        .and_then(Value::as_u64)
        .filter(|sequence| *sequence > 0)
}

fn next_dispatch_record_sequence(records: &serde_json::Map<String, Value>) -> u64 {
    records
        .values()
        .filter_map(record_sequence_from_dispatch_record)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn mark_record_status(records: &mut serde_json::Map<String, Value>, record_id: &str, status: &str) {
    if let Some(record) = records.get_mut(record_id)
        && let Some(record) = record.as_object_mut()
    {
        if record.contains_key("closure_status") {
            record.insert(
                String::from("closure_status"),
                Value::String(status.to_owned()),
            );
        }
        record.insert(
            String::from("record_status"),
            Value::String(status.to_owned()),
        );
    }
}

fn current_task_closure_record_from_payload(
    task: u32,
    payload: &serde_json::Map<String, Value>,
) -> Option<CurrentTaskClosureRecord> {
    let payload_value = Value::Object(payload.clone());
    Some(CurrentTaskClosureRecord {
        task,
        dispatch_id: json_string(&payload_value, "dispatch_id")?,
        closure_record_id: json_string(&payload_value, "closure_record_id")?,
        source_plan_path: json_string(&payload_value, "source_plan_path"),
        source_plan_revision: json_u32(&payload_value, "source_plan_revision"),
        execution_run_id: json_string(&payload_value, "execution_run_id"),
        reviewed_state_id: json_string(&payload_value, "reviewed_state_id")?,
        contract_identity: json_string(&payload_value, "contract_identity")?,
        effective_reviewed_surface_paths: json_string_array_strict(
            &payload_value,
            "effective_reviewed_surface_paths",
        )?,
        review_result: json_string(&payload_value, "review_result")?,
        review_summary_hash: json_string(&payload_value, "review_summary_hash")?,
        verification_result: json_string(&payload_value, "verification_result")?,
        verification_summary_hash: json_string(&payload_value, "verification_summary_hash")?,
        closure_status: json_string(&payload_value, "closure_status"),
    })
}

fn task_closure_negative_result_from_payload(
    payload: &serde_json::Map<String, Value>,
) -> Option<TaskClosureNegativeResult> {
    let payload_value = Value::Object(payload.clone());
    Some(TaskClosureNegativeResult {
        dispatch_id: json_string(&payload_value, "dispatch_id")?,
        reviewed_state_id: json_string(&payload_value, "reviewed_state_id")?,
        contract_identity: json_string(&payload_value, "contract_identity")?,
        review_result: json_string(&payload_value, "review_result")?,
        review_summary_hash: json_string(&payload_value, "review_summary_hash")?,
        verification_result: json_string(&payload_value, "verification_result")?,
        verification_summary_hash: json_string(&payload_value, "verification_summary_hash"),
    })
}

fn deterministic_record_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    for part in parts {
        hasher.update(b"\n");
        hasher.update(part.as_bytes());
    }
    let digest = format!("{:x}", hasher.finalize());
    format!("{prefix}-{}", &digest[..16])
}

fn next_available_history_record_id(
    records: Option<&serde_json::Map<String, Value>>,
    base_record_id: &str,
    active_record_id: Option<&str>,
) -> String {
    let Some(records) = records else {
        return base_record_id.to_owned();
    };
    if !records.contains_key(base_record_id) || active_record_id == Some(base_record_id) {
        return base_record_id.to_owned();
    }
    let mut occurrence = 2_u64;
    loop {
        let candidate = format!("{base_record_id}-{occurrence}");
        if !records.contains_key(&candidate) {
            return candidate;
        }
        occurrence += 1;
    }
}

fn selected_topology_from_execution_mode(execution_mode: &str) -> &'static str {
    match execution_mode.trim() {
        "featureforge:subagent-driven-development" => "worktree-backed-parallel",
        _ => "conservative-fallback",
    }
}

#[cfg(test)]
mod tests {
    use super::AuthoritativeTransitionState;
    use super::BranchClosureResultRecord;
    use super::BrowserQaResultRecord;
    use super::CachedAuthoritativeStatePayload;
    use super::FinalReviewMilestoneRecord;
    use super::PersistedReviewStateFieldClass;
    use super::ReleaseReadinessResultRecord;
    use super::TaskClosureResultRecord;
    use super::authoritative_state_cache_stamp;
    use super::authoritative_state_payload_cache;
    use super::claim_step_write_authority;
    use super::classify_review_state_field;
    use super::read_authoritative_transition_state_source;
    use super::remove_stale_write_authority_lock;
    use crate::expect_ext::{ExpectErrExt, ExpectValueExt};
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::path::Path;
    use std::path::PathBuf;

    use crate::execution::state::ExecutionRuntime;
    use crate::paths::harness_branch_root;
    use serde_json::{Value, json};

    fn abort_test(message: &str) -> ! {
        eprintln!("{message}");
        std::process::abort();
    }

    fn test_runtime(state_dir: &Path) -> ExecutionRuntime {
        ExecutionRuntime {
            repo_root: state_dir.to_path_buf(),
            git_dir: state_dir.join(".git"),
            branch_name: String::from("feature"),
            repo_slug: String::from("repo"),
            safe_branch: String::from("feature"),
            state_dir: state_dir.to_path_buf(),
        }
    }

    fn transition_state_with_current_branch_closure(
        current_branch_closure_id: &str,
    ) -> AuthoritativeTransitionState {
        AuthoritativeTransitionState {
            state_path: PathBuf::from("/tmp/authoritative-state.json"),
            state_payload: json!({
                "current_branch_closure_id": current_branch_closure_id,
                "current_branch_closure_reviewed_state_id": "git_tree:current",
                "current_branch_closure_contract_identity": "branch-contract-current",
                "finish_review_gate_pass_branch_closure_id": Value::Null,
            }),
            phase: None,
            active_contract: None,
            dirty: false,
        }
    }

    fn transition_state_with_milestone_histories() -> AuthoritativeTransitionState {
        AuthoritativeTransitionState {
            state_path: PathBuf::from("/tmp/authoritative-state.json"),
            state_payload: json!({
                "branch_closure_records": {},
                "release_readiness_record_history": {},
                "final_review_record_history": {},
                "browser_qa_record_history": {},
                "current_branch_closure_id": "branch-closure-current",
                "current_branch_closure_reviewed_state_id": "git_tree:current",
                "current_branch_closure_contract_identity": "branch-contract-current",
                "current_release_readiness_result": Value::Null,
                "current_release_readiness_summary_hash": Value::Null,
                "current_release_readiness_record_id": Value::Null,
                "current_final_review_record_id": Value::Null,
                "current_qa_record_id": Value::Null,
                "release_docs_state": Value::Null,
                "final_review_state": Value::Null,
                "browser_qa_state": Value::Null,
            }),
            phase: None,
            active_contract: None,
            dirty: false,
        }
    }

    #[test]
    fn removing_already_gone_stale_write_authority_lock_is_allowed() {
        let tempdir = tempfile::tempdir().expect_or_abort("tempdir should be creatable");
        let lock_path = tempdir.path().join("write-authority.lock");
        remove_stale_write_authority_lock(&lock_path).expect_or_abort(
            "already-removed stale write-authority lock should be treated as reclaimed",
        );
    }

    #[cfg(unix)]
    #[test]
    fn claim_step_write_authority_fails_closed_when_lock_path_is_symlink() {
        let tempdir = tempfile::tempdir().expect_or_abort("tempdir should be creatable");
        let runtime = test_runtime(tempdir.path());
        let lock_path =
            harness_branch_root(&runtime.state_dir, &runtime.repo_slug, &runtime.branch_name)
                .join("write-authority.lock");
        fs::create_dir_all(
            lock_path
                .parent()
                .expect_or_abort("lock path should have a parent directory"),
        )
        .expect_or_abort("lock parent should be creatable");
        let lock_target_path = tempdir.path().join("lock-target.pid");
        fs::write(&lock_target_path, format!("pid={}\n", std::process::id()))
            .expect_or_abort("lock target should be writable");
        symlink(&lock_target_path, &lock_path)
            .expect_or_abort("symlink lock path fixture should be creatable");

        let error = match claim_step_write_authority(&runtime) {
            Ok(_guard) => abort_test("symlink write-authority lock path must fail closed"),
            Err(error) => error,
        };
        assert!(
            error.message.contains("must not be a symlink"),
            "symlink lock failure should explain trust-boundary rejection"
        );
    }

    #[cfg(unix)]
    #[test]
    fn transition_state_source_load_fails_closed_when_state_path_is_symlink() {
        let tempdir = tempfile::tempdir().expect_or_abort("tempdir should be creatable");
        let state_path = tempdir.path().join("state.json");
        let state_target_path = tempdir.path().join("state-target.json");
        fs::write(&state_target_path, r#"{"harness_phase":"executing"}"#)
            .expect_or_abort("state target should be writable");
        symlink(&state_target_path, &state_path)
            .expect_or_abort("symlink authoritative state fixture should be creatable");

        let error = read_authoritative_transition_state_source(&state_path)
            .expect_err_or_abort("authoritative transition state loader must reject symlink paths");
        assert!(
            error.message.contains("must not be a symlink"),
            "symlink state failure should explain trust-boundary rejection"
        );
    }

    #[test]
    fn persist_if_dirty_refreshes_authoritative_state_cache_entry() {
        let tempdir = tempfile::tempdir().expect_or_abort("tempdir should be creatable");
        let state_path = tempdir.path().join("authoritative-state.json");
        fs::write(&state_path, r#"{"harness_phase":"executing"}"#)
            .expect_or_abort("authoritative state fixture should be writable");
        let initial_metadata = fs::metadata(&state_path)
            .expect_or_abort("authoritative state fixture should be stat-able");
        let initial_payload = json!({"harness_phase":"executing"});
        let initial_gate_state = serde_json::from_value(initial_payload.clone())
            .expect_or_abort("initial gate-state payload should parse");
        authoritative_state_payload_cache()
            .lock()
            .expect_or_abort("authoritative state payload cache lock should not be poisoned")
            .insert(
                state_path.clone(),
                CachedAuthoritativeStatePayload {
                    stamp: authoritative_state_cache_stamp(&initial_metadata),
                    state_payload: initial_payload,
                    gate_state: initial_gate_state,
                },
            );

        let refreshed_payload = json!({
            "harness_phase": "final_review_pending",
            "latest_authoritative_sequence": 42
        });
        let state = AuthoritativeTransitionState {
            state_path: state_path.clone(),
            state_payload: refreshed_payload.clone(),
            phase: None,
            active_contract: None,
            dirty: true,
        };
        state
            .persist_if_dirty_with_failpoint(None)
            .expect_or_abort("persist should refresh cache entry");

        let cached = authoritative_state_payload_cache()
            .lock()
            .expect_or_abort("authoritative state payload cache lock should not be poisoned")
            .get(&state_path)
            .cloned()
            .expect_or_abort("persisted state should leave a refreshed cache entry");
        assert_eq!(
            cached.state_payload, refreshed_payload,
            "cache entry should track the latest persisted authoritative payload",
        );
    }

    #[test]
    fn recoverable_current_branch_closure_identity_prefers_the_current_binding() {
        let mut authoritative_state =
            transition_state_with_current_branch_closure("branch-closure-new");
        authoritative_state.state_payload["branch_closure_records"] = json!({
            "branch-closure-new": {
                "branch_closure_id": "branch-closure-new",
                "source_plan_path": "docs/featureforge/plans/example.md",
                "source_plan_revision": 1,
                "repo_slug": "repo-slug",
                "branch_name": "feature-branch",
                "base_branch": "main",
                "reviewed_state_id": "git_tree:new",
                "contract_identity": "branch-contract-new",
                "effective_reviewed_branch_surface": "repo_tracked_content",
                "source_task_closure_ids": ["task-1-closure"],
                "provenance_basis": "task_closure_lineage",
                "closure_status": "current",
                "superseded_branch_closure_ids": [],
                "record_sequence": 1
            }
        });

        let current_identity = authoritative_state
            .recoverable_current_branch_closure_identity()
            .expect_or_abort("current branch-closure identity should be recoverable");

        assert!(
            current_identity.branch_closure_id == "branch-closure-new",
            "recoverable current identity should stay bound to the current branch closure"
        );
        assert_eq!(
            authoritative_state.state_payload["current_branch_closure_id"],
            "branch-closure-new"
        );
        assert_eq!(
            authoritative_state.state_payload["current_branch_closure_reviewed_state_id"],
            "git_tree:current"
        );
        assert_eq!(
            authoritative_state.state_payload["current_branch_closure_contract_identity"],
            "branch-contract-current"
        );
    }

    #[test]
    fn finish_review_checkpoint_requires_the_same_current_branch_closure() {
        let mut authoritative_state =
            transition_state_with_current_branch_closure("branch-closure-new");

        let recorded = authoritative_state
            .record_finish_review_gate_pass_checkpoint_if_current("branch-closure-old")
            .expect_or_abort("stale finish-review checkpoint check should succeed");

        assert!(
            !recorded,
            "finish-review checkpoint should skip stale branch-closure ids"
        );
        assert!(
            authoritative_state.state_payload["finish_review_gate_pass_branch_closure_id"]
                .is_null()
        );
    }

    #[test]
    fn task_dispatch_lineage_history_tracks_current_and_historical_records() {
        let mut authoritative_state = AuthoritativeTransitionState {
            state_path: PathBuf::from("/tmp/authoritative-state.json"),
            state_payload: json!({
                "run_identity": {
                    "execution_run_id": "run-unit-test",
                    "source_plan_path": "docs/featureforge/plans/example.md",
                    "source_plan_revision": 7
                },
                "strategy_review_dispatch_lineage": {},
                "strategy_review_dispatch_lineage_history": {},
                "task_closure_negative_result_records": {}
            }),
            phase: None,
            active_contract: None,
            dirty: false,
        };

        authoritative_state
            .upsert_task_dispatch_lineage(
                1,
                "run-unit-test",
                1,
                "dispatch-a",
                "task-lineage-a",
                "git_tree:a",
            )
            .expect_or_abort("first task dispatch lineage should record");
        authoritative_state
            .upsert_task_dispatch_lineage(
                1,
                "run-unit-test",
                1,
                "dispatch-b",
                "task-lineage-b",
                "git_tree:b",
            )
            .expect_or_abort("second task dispatch lineage should append");

        let history = authoritative_state.state_payload["strategy_review_dispatch_lineage_history"]
            .as_object()
            .expect_or_abort("task dispatch lineage history should be an object");
        assert_eq!(history.len(), 2);
        let historical = history
            .values()
            .find(|record| record["dispatch_id"] == "dispatch-a")
            .expect_or_abort("first task dispatch should be present in history");
        let current = history
            .values()
            .find(|record| record["dispatch_id"] == "dispatch-b")
            .expect_or_abort("latest task dispatch should be present in history");
        assert_eq!(historical["record_status"], "historical");
        assert_eq!(historical["status"], "historical");
        assert_eq!(current["record_status"], "current");
        assert_eq!(current["status"], "current");
        assert_eq!(
            authoritative_state.state_payload["strategy_review_dispatch_lineage"]["task-1"]["dispatch_id"],
            "dispatch-b"
        );
    }

    #[test]
    fn clearing_task_dispatch_lineage_preserves_stale_history() {
        let mut authoritative_state = AuthoritativeTransitionState {
            state_path: PathBuf::from("/tmp/authoritative-state.json"),
            state_payload: json!({
                "run_identity": {
                    "execution_run_id": "run-unit-test",
                    "source_plan_path": "docs/featureforge/plans/example.md",
                    "source_plan_revision": 7
                },
                "strategy_review_dispatch_lineage": {},
                "strategy_review_dispatch_lineage_history": {},
                "strategy_review_dispatch_credits": {},
                "task_closure_negative_result_records": {}
            }),
            phase: None,
            active_contract: None,
            dirty: false,
        };
        authoritative_state
            .upsert_task_dispatch_lineage(
                2,
                "run-unit-test",
                3,
                "dispatch-stale",
                "task-lineage-stale",
                "git_tree:stale",
            )
            .expect_or_abort("task dispatch lineage should record");

        let cleared = authoritative_state
            .clear_task_review_dispatch_lineage(2)
            .expect_or_abort("clearing task dispatch lineage should succeed");
        assert!(cleared);
        assert!(
            authoritative_state.state_payload["strategy_review_dispatch_lineage"]
                .as_object()
                .is_some_and(|lineage| !lineage.contains_key("task-2"))
        );
        let history = authoritative_state.state_payload["strategy_review_dispatch_lineage_history"]
            .as_object()
            .expect_or_abort("task dispatch history should be an object");
        let stale_record = history
            .values()
            .find(|record| record["dispatch_id"] == "dispatch-stale")
            .expect_or_abort("cleared dispatch lineage should remain in history");
        assert_eq!(stale_record["record_status"], "stale_unreviewed");
        assert_eq!(stale_record["status"], "stale_unreviewed");
    }

    #[test]
    fn branch_reclosure_preserves_stale_final_review_dispatch_history() {
        let mut authoritative_state = AuthoritativeTransitionState {
            state_path: PathBuf::from("/tmp/authoritative-state.json"),
            state_payload: json!({
                "current_branch_closure_id": "branch-closure-1",
                "current_branch_closure_reviewed_state_id": "git_tree:one",
                "current_branch_closure_contract_identity": "contract-one",
                "current_release_readiness_record_id": Value::Null,
                "current_final_review_record_id": Value::Null,
                "current_qa_record_id": Value::Null,
                "final_review_dispatch_lineage": {
                    "execution_run_id": "run-unit-test",
                    "dispatch_id": "dispatch-final",
                    "branch_closure_id": "branch-closure-1"
                }
            }),
            phase: None,
            active_contract: None,
            dirty: false,
        };

        authoritative_state
            .set_current_branch_closure_id("branch-closure-2", "git_tree:two", "contract-two")
            .expect_or_abort("branch reclosure should succeed");

        assert!(
            authoritative_state.state_payload["final_review_dispatch_lineage"].is_null(),
            "current final-review dispatch lineage should be cleared after branch reclosure"
        );
        let history = authoritative_state.state_payload["final_review_dispatch_lineage_history"]
            .as_object()
            .expect_or_abort("final-review dispatch history should be an object");
        let stale_record = history
            .values()
            .find(|record| record["dispatch_id"] == "dispatch-final")
            .expect_or_abort("previous final-review dispatch lineage should remain in history");
        assert_eq!(stale_record["record_status"], "stale_unreviewed");
        assert_eq!(stale_record["status"], "stale_unreviewed");
    }

    #[test]
    fn release_readiness_history_appends_in_record_order() {
        let mut authoritative_state = transition_state_with_milestone_histories();

        authoritative_state
            .record_release_readiness_result(ReleaseReadinessResultRecord {
                branch_closure_id: "branch-closure-current",
                source_plan_path: "docs/featureforge/plans/example.md",
                source_plan_revision: 1,
                repo_slug: "repo-slug",
                branch_name: "feature-branch",
                base_branch: "main",
                reviewed_state_id: "git_tree:current",
                result: "blocked",
                release_docs_fingerprint: None,
                summary: "summary a",
                summary_hash: "summary-a",
                generated_by_identity: "featureforge/release-readiness",
            })
            .expect_or_abort("first release-readiness record should succeed");
        authoritative_state
            .record_release_readiness_result(ReleaseReadinessResultRecord {
                branch_closure_id: "branch-closure-current",
                source_plan_path: "docs/featureforge/plans/example.md",
                source_plan_revision: 1,
                repo_slug: "repo-slug",
                branch_name: "feature-branch",
                base_branch: "main",
                reviewed_state_id: "git_tree:current",
                result: "blocked",
                release_docs_fingerprint: Some("release-fingerprint-b"),
                summary: "summary b",
                summary_hash: "summary-b",
                generated_by_identity: "featureforge/release-readiness",
            })
            .expect_or_abort("second release-readiness record should append");

        let history = authoritative_state.state_payload["release_readiness_record_history"]
            .as_object()
            .expect_or_abort("release readiness history should be an object");
        assert_eq!(history.len(), 2);
        let mut records = history.values().collect::<Vec<_>>();
        records.sort_by_key(|record| record["record_sequence"].as_u64().unwrap_or_default());
        assert_eq!(records[0]["record_sequence"], 1);
        assert_eq!(records[1]["record_sequence"], 2);
        assert_eq!(records[0]["record_status"], "historical");
        assert_eq!(records[1]["record_status"], "current");
        assert_eq!(
            authoritative_state.state_payload["current_release_readiness_record_id"],
            records[1]["record_id"]
        );
    }

    #[test]
    fn release_readiness_history_preserves_historical_records_on_content_id_collision() {
        let mut authoritative_state = transition_state_with_milestone_histories();

        authoritative_state
            .record_release_readiness_result(ReleaseReadinessResultRecord {
                branch_closure_id: "branch-closure-current",
                source_plan_path: "docs/featureforge/plans/example.md",
                source_plan_revision: 1,
                repo_slug: "repo-slug",
                branch_name: "feature-branch",
                base_branch: "main",
                reviewed_state_id: "git_tree:current",
                result: "blocked",
                release_docs_fingerprint: None,
                summary: "summary a",
                summary_hash: "summary-a",
                generated_by_identity: "featureforge/release-readiness",
            })
            .expect_or_abort("first release-readiness record should succeed");
        authoritative_state
            .record_release_readiness_result(ReleaseReadinessResultRecord {
                branch_closure_id: "branch-closure-current",
                source_plan_path: "docs/featureforge/plans/example.md",
                source_plan_revision: 1,
                repo_slug: "repo-slug",
                branch_name: "feature-branch",
                base_branch: "main",
                reviewed_state_id: "git_tree:current",
                result: "ready",
                release_docs_fingerprint: Some("release-fingerprint-b"),
                summary: "summary b",
                summary_hash: "summary-b",
                generated_by_identity: "featureforge/release-readiness",
            })
            .expect_or_abort("second release-readiness record should succeed");
        authoritative_state
            .record_release_readiness_result(ReleaseReadinessResultRecord {
                branch_closure_id: "branch-closure-current",
                source_plan_path: "docs/featureforge/plans/example.md",
                source_plan_revision: 1,
                repo_slug: "repo-slug",
                branch_name: "feature-branch",
                base_branch: "main",
                reviewed_state_id: "git_tree:current",
                result: "blocked",
                release_docs_fingerprint: None,
                summary: "summary a",
                summary_hash: "summary-a",
                generated_by_identity: "featureforge/release-readiness",
            })
            .expect_or_abort(
                "third release-readiness record should preserve collided historical content",
            );

        let history = authoritative_state.state_payload["release_readiness_record_history"]
            .as_object()
            .expect_or_abort("release readiness history should be an object");
        assert_eq!(history.len(), 3);
        let matching_blocked_records = history
            .values()
            .filter(|record| record["result"] == "blocked" && record["summary_hash"] == "summary-a")
            .collect::<Vec<_>>();
        assert_eq!(matching_blocked_records.len(), 2);
        let distinct_record_ids = matching_blocked_records
            .iter()
            .map(|record| record["record_id"].as_str().unwrap_or_default().to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(distinct_record_ids.len(), 2);
    }

    #[test]
    fn final_review_history_appends_in_record_order() {
        let mut authoritative_state = transition_state_with_milestone_histories();

        authoritative_state
            .record_final_review_result(FinalReviewMilestoneRecord {
                branch_closure_id: "branch-closure-current",
                release_readiness_record_id: "release-record-current",
                dispatch_id: "dispatch-a",
                reviewer_source: "fresh-context-subagent",
                reviewer_id: "reviewer-a",
                result: "pass",
                final_review_fingerprint: Some("final-fingerprint-a"),
                deviations_required: Some(false),
                browser_qa_required: Some(true),
                source_plan_path: "docs/featureforge/plans/example.md",
                source_plan_revision: 1,
                repo_slug: "repo-slug",
                branch_name: "feature-branch",
                base_branch: "main",
                reviewed_state_id: "git_tree:current",
                summary: "summary a",
                summary_hash: "summary-a",
            })
            .expect_or_abort("first final-review record should succeed");
        authoritative_state
            .record_final_review_result(FinalReviewMilestoneRecord {
                branch_closure_id: "branch-closure-current",
                release_readiness_record_id: "release-record-current",
                dispatch_id: "dispatch-b",
                reviewer_source: "fresh-context-subagent",
                reviewer_id: "reviewer-b",
                result: "pass",
                final_review_fingerprint: Some("final-fingerprint-b"),
                deviations_required: Some(false),
                browser_qa_required: Some(true),
                source_plan_path: "docs/featureforge/plans/example.md",
                source_plan_revision: 1,
                repo_slug: "repo-slug",
                branch_name: "feature-branch",
                base_branch: "main",
                reviewed_state_id: "git_tree:current",
                summary: "summary b",
                summary_hash: "summary-b",
            })
            .expect_or_abort("second final-review record should append");

        let history = authoritative_state.state_payload["final_review_record_history"]
            .as_object()
            .expect_or_abort("final review history should be an object");
        assert_eq!(history.len(), 2);
        let mut records = history.values().collect::<Vec<_>>();
        records.sort_by_key(|record| record["record_sequence"].as_u64().unwrap_or_default());
        assert_eq!(records[0]["record_sequence"], 1);
        assert_eq!(records[1]["record_sequence"], 2);
        assert_eq!(records[0]["record_status"], "historical");
        assert_eq!(records[1]["record_status"], "current");
        assert_eq!(
            authoritative_state.state_payload["current_final_review_record_id"],
            records[1]["record_id"]
        );
    }

    #[test]
    fn browser_qa_history_appends_in_record_order() {
        let mut authoritative_state = transition_state_with_milestone_histories();

        authoritative_state
            .record_browser_qa_result(BrowserQaResultRecord {
                branch_closure_id: "branch-closure-current",
                final_review_record_id: "final-review-record-current",
                source_plan_path: "docs/featureforge/plans/example.md",
                source_plan_revision: 1,
                repo_slug: "repo-slug",
                branch_name: "feature-branch",
                base_branch: "main",
                reviewed_state_id: "git_tree:current",
                result: "fail",
                browser_qa_fingerprint: None,
                source_test_plan_fingerprint: Some("test-plan-fingerprint-a"),
                summary: "summary a",
                summary_hash: "summary-a",
                generated_by_identity: "featureforge/qa",
            })
            .expect_or_abort("first browser QA record should succeed");
        authoritative_state
            .record_browser_qa_result(BrowserQaResultRecord {
                branch_closure_id: "branch-closure-current",
                final_review_record_id: "final-review-record-current",
                source_plan_path: "docs/featureforge/plans/example.md",
                source_plan_revision: 1,
                repo_slug: "repo-slug",
                branch_name: "feature-branch",
                base_branch: "main",
                reviewed_state_id: "git_tree:current",
                result: "fail",
                browser_qa_fingerprint: Some("browser-qa-fingerprint-b"),
                source_test_plan_fingerprint: Some("test-plan-fingerprint-b"),
                summary: "summary b",
                summary_hash: "summary-b",
                generated_by_identity: "featureforge/qa",
            })
            .expect_or_abort("second browser QA record should append");

        let history = authoritative_state.state_payload["browser_qa_record_history"]
            .as_object()
            .expect_or_abort("browser QA history should be an object");
        assert_eq!(history.len(), 2);
        let mut records = history.values().collect::<Vec<_>>();
        records.sort_by_key(|record| record["record_sequence"].as_u64().unwrap_or_default());
        assert_eq!(records[0]["record_sequence"], 1);
        assert_eq!(records[1]["record_sequence"], 2);
        assert_eq!(records[0]["record_status"], "historical");
        assert_eq!(records[1]["record_status"], "current");
        assert_eq!(
            authoritative_state.state_payload["current_qa_record_id"],
            records[1]["record_id"]
        );
    }

    #[test]
    fn restore_current_final_review_overlay_requires_matching_release_dependency() {
        let mut authoritative_state = transition_state_with_milestone_histories();
        authoritative_state.state_payload["current_release_readiness_record_id"] =
            Value::from("release-record-current");
        authoritative_state.state_payload["current_final_review_record_id"] =
            Value::from("final-review-record-current");
        authoritative_state.state_payload["release_readiness_record_history"] = json!({
            "release-record-current": {
                "record_id": "release-record-current",
                "record_status": "current",
                "branch_closure_id": "branch-closure-current",
                "reviewed_state_id": "git_tree:current",
                "result": "ready",
                "summary_hash": "release-summary-hash"
            }
        });
        authoritative_state.state_payload["final_review_record_history"] = json!({
            "final-review-record-current": {
                "record_id": "final-review-record-current",
                "record_status": "current",
                "branch_closure_id": "branch-closure-current",
                "reviewed_state_id": "git_tree:current",
                "release_readiness_record_id": "release-record-stale",
                "dispatch_id": "dispatch-final",
                "reviewer_source": "fresh-context-subagent",
                "reviewer_id": "reviewer",
                "result": "pass",
                "summary_hash": "final-summary-hash"
            }
        });

        let restored = authoritative_state
            .restore_current_final_review_overlay_fields()
            .expect_or_abort("overlay restore should not error on dependency mismatch");
        assert!(
            !restored,
            "final-review overlay restore must fail closed when release dependency is mismatched"
        );
        assert!(
            !authoritative_state.dirty,
            "mismatched dependency should not mutate derived final-review overlay state"
        );
    }

    #[test]
    fn restore_current_browser_qa_overlay_requires_matching_final_review_dependency() {
        let mut authoritative_state = transition_state_with_milestone_histories();
        authoritative_state.state_payload["current_final_review_record_id"] =
            Value::from("final-review-record-current");
        authoritative_state.state_payload["current_qa_record_id"] =
            Value::from("qa-record-current");
        authoritative_state.state_payload["final_review_record_history"] = json!({
            "final-review-record-current": {
                "record_id": "final-review-record-current",
                "record_status": "current",
                "branch_closure_id": "branch-closure-current",
                "reviewed_state_id": "git_tree:current",
                "release_readiness_record_id": "release-record-current",
                "dispatch_id": "dispatch-final",
                "reviewer_source": "fresh-context-subagent",
                "reviewer_id": "reviewer",
                "result": "pass",
                "summary_hash": "final-summary-hash"
            }
        });
        authoritative_state.state_payload["browser_qa_record_history"] = json!({
            "qa-record-current": {
                "record_id": "qa-record-current",
                "record_status": "current",
                "branch_closure_id": "branch-closure-current",
                "reviewed_state_id": "git_tree:current",
                "final_review_record_id": "final-review-record-stale",
                "result": "pass",
                "summary_hash": "qa-summary-hash"
            }
        });

        let restored = authoritative_state
            .restore_current_browser_qa_overlay_fields()
            .expect_or_abort("overlay restore should not error on dependency mismatch");
        assert!(
            !restored,
            "browser-QA overlay restore must fail closed when final-review dependency is mismatched"
        );
        assert!(
            !authoritative_state.dirty,
            "mismatched dependency should not mutate derived QA overlay state"
        );
    }

    #[test]
    fn branch_closure_history_marks_replaced_record_superseded() {
        let mut authoritative_state =
            transition_state_with_current_branch_closure("branch-closure-old");
        authoritative_state.state_payload["branch_closure_records"] = json!({
            "branch-closure-old": {
                "branch_closure_id": "branch-closure-old",
                "source_plan_path": "docs/featureforge/plans/example.md",
                "source_plan_revision": 1,
                "repo_slug": "repo-slug",
                "branch_name": "feature-branch",
                "base_branch": "main",
                "reviewed_state_id": "git_tree:old",
                "contract_identity": "branch-contract-old",
                "effective_reviewed_branch_surface": "repo_tracked_content",
                "source_task_closure_ids": ["task-1-closure"],
                "provenance_basis": "task_closure_lineage",
                "closure_status": "current",
                "superseded_branch_closure_ids": [],
                "record_sequence": 1
            }
        });

        authoritative_state
            .record_branch_closure(BranchClosureResultRecord {
                branch_closure_id: "branch-closure-new",
                source_plan_path: "docs/featureforge/plans/example.md",
                source_plan_revision: 1,
                repo_slug: "repo-slug",
                branch_name: "feature-branch",
                base_branch: "main",
                reviewed_state_id: "git_tree:new",
                contract_identity: "branch-contract-new",
                effective_reviewed_branch_surface: "repo_tracked_content",
                source_task_closure_ids: &["task-1-closure".to_owned()],
                provenance_basis: "task_closure_lineage",
                closure_status: "current",
                superseded_branch_closure_ids: &["branch-closure-old".to_owned()],
                branch_closure_fingerprint: None,
            })
            .expect_or_abort("second branch-closure record should succeed");
        authoritative_state
            .set_current_branch_closure_id(
                "branch-closure-new",
                "git_tree:new",
                "branch-contract-new",
            )
            .expect_or_abort("current branch-closure update should succeed");

        assert_eq!(
            authoritative_state.state_payload["branch_closure_records"]["branch-closure-old"]["closure_status"],
            "superseded"
        );
        assert_eq!(
            authoritative_state.state_payload["branch_closure_records"]["branch-closure-new"]["closure_status"],
            "current"
        );
    }

    #[test]
    fn next_available_branch_closure_record_id_suffixes_historical_collisions() {
        let mut authoritative_state =
            transition_state_with_current_branch_closure("branch-closure-current");
        authoritative_state.state_payload["branch_closure_records"] = json!({
            "branch-closure-current": {
                "branch_closure_id": "branch-closure-current",
                "source_plan_path": "docs/featureforge/plans/example.md",
                "source_plan_revision": 1,
                "repo_slug": "repo-slug",
                "branch_name": "feature-branch",
                "base_branch": "main",
                "reviewed_state_id": "git_tree:current",
                "contract_identity": "branch-contract-current",
                "effective_reviewed_branch_surface": "repo_tracked_content",
                "source_task_closure_ids": ["task-1-closure"],
                "provenance_basis": "task_closure_lineage",
                "closure_status": "historical",
                "superseded_branch_closure_ids": [],
                "record_sequence": 1
            },
            "branch-closure-current-2": {
                "branch_closure_id": "branch-closure-current-2",
                "source_plan_path": "docs/featureforge/plans/example.md",
                "source_plan_revision": 1,
                "repo_slug": "repo-slug",
                "branch_name": "feature-branch",
                "base_branch": "main",
                "reviewed_state_id": "git_tree:newer",
                "contract_identity": "branch-contract-newer",
                "effective_reviewed_branch_surface": "repo_tracked_content",
                "source_task_closure_ids": ["task-2-closure"],
                "provenance_basis": "task_closure_lineage",
                "closure_status": "current",
                "superseded_branch_closure_ids": ["branch-closure-current"],
                "record_sequence": 2
            }
        });

        assert_eq!(
            authoritative_state.next_available_branch_closure_record_id("branch-closure-current"),
            "branch-closure-current-3"
        );
    }

    #[test]
    fn branch_closure_record_rejects_non_current_or_incomplete_records() {
        let mut authoritative_state =
            transition_state_with_current_branch_closure("branch-closure-current");
        authoritative_state.state_payload["branch_closure_records"] = json!({
            "branch-closure-current": {
                "branch_closure_id": "branch-closure-current",
                "source_plan_path": "docs/featureforge/plans/example.md",
                "source_plan_revision": 1,
                "repo_slug": "repo-slug",
                "branch_name": "feature-branch",
                "base_branch": "main",
                "reviewed_state_id": "git_tree:current",
                "contract_identity": "branch-contract-current",
                "effective_reviewed_branch_surface": "repo_tracked_content",
                "source_task_closure_ids": ["task-1-closure"],
                "provenance_basis": "task_closure_lineage",
                "closure_status": "historical",
                "superseded_branch_closure_ids": [],
                "record_sequence": 1
            }
        });
        assert!(
            authoritative_state
                .branch_closure_record("branch-closure-current")
                .is_none(),
            "historical branch-closure records must not satisfy current authoritative identity lookups"
        );

        authoritative_state.state_payload["branch_closure_records"]["branch-closure-current"] = json!({
            "branch_closure_id": "branch-closure-current",
            "source_plan_path": "docs/featureforge/plans/example.md",
            "source_plan_revision": 1,
            "repo_slug": "repo-slug",
            "branch_name": "feature-branch",
            "base_branch": "main",
            "reviewed_state_id": "git_tree:current",
            "contract_identity": "branch-contract-current",
            "provenance_basis": "task_closure_lineage",
            "closure_status": "current",
            "superseded_branch_closure_ids": [],
            "record_sequence": 1
        });
        assert!(
            authoritative_state
                .branch_closure_record("branch-closure-current")
                .is_none(),
            "incomplete branch-closure records must not satisfy current authoritative identity lookups"
        );
    }

    #[test]
    fn clear_current_branch_closure_for_structural_repair_clears_late_stage_currentness() {
        {
            let mut authoritative_state = AuthoritativeTransitionState {
                state_path: PathBuf::from("/tmp/authoritative-state.json"),
                state_payload: json!({
                    "current_branch_closure_id": "branch-closure-current",
                    "current_branch_closure_reviewed_state_id": "git_tree:current",
                    "current_branch_closure_contract_identity": "branch-contract-current",
                    "current_release_readiness_record_id": "release-record-current",
                    "current_final_review_record_id": "final-review-record-current",
                    "current_qa_record_id": "qa-record-current",
                    "final_review_dispatch_lineage": {
                        "dispatch_id": "dispatch-final",
                        "branch_closure_id": "branch-closure-current"
                    },
                    "finish_review_gate_pass_branch_closure_id": "branch-closure-current",
                    "branch_closure_records": {
                        "branch-closure-current": {
                            "branch_closure_id": "branch-closure-current",
                            "source_plan_path": "docs/featureforge/plans/example.md",
                            "source_plan_revision": 1,
                            "repo_slug": "repo-slug",
                            "branch_name": "feature-branch",
                            "base_branch": "main",
                            "reviewed_state_id": "git_tree:current",
                            "contract_identity": "branch-contract-current",
                            "effective_reviewed_branch_surface": "repo_tracked_content",
                            "source_task_closure_ids": ["task-1-closure"],
                            "provenance_basis": "task_closure_lineage",
                            "closure_status": "current",
                            "superseded_branch_closure_ids": [],
                            "record_sequence": 1
                        }
                    },
                    "release_readiness_record_history": {
                        "release-record-current": {
                            "record_id": "release-record-current",
                            "record_status": "current"
                        }
                    },
                    "final_review_record_history": {
                        "final-review-record-current": {
                            "record_id": "final-review-record-current",
                            "record_status": "current"
                        }
                    },
                    "browser_qa_record_history": {
                        "qa-record-current": {
                            "record_id": "qa-record-current",
                            "record_status": "current"
                        }
                    }
                }),
                phase: None,
                active_contract: None,
                dirty: false,
            };

            let cleared = authoritative_state
                .clear_current_branch_closure_for_structural_repair()
                .expect_or_abort("structural branch repair clear should succeed");
            assert!(cleared);
            assert!(
                authoritative_state.state_payload["current_branch_closure_id"].is_null(),
                "current branch closure binding should be cleared"
            );
            assert!(
                authoritative_state.state_payload["current_release_readiness_record_id"].is_null(),
                "current release-readiness binding should be cleared"
            );
            assert!(
                authoritative_state.state_payload["current_final_review_record_id"].is_null(),
                "current final-review binding should be cleared"
            );
            assert!(
                authoritative_state.state_payload["current_qa_record_id"].is_null(),
                "current QA binding should be cleared"
            );
            assert!(
                authoritative_state.state_payload["final_review_dispatch_lineage"].is_null(),
                "current final-review dispatch lineage should be cleared"
            );
            assert!(
                authoritative_state.state_payload["finish_review_gate_pass_branch_closure_id"]
                    .is_null(),
                "finish checkpoint should be cleared"
            );
            assert_eq!(
                authoritative_state.state_payload["branch_closure_records"]["branch-closure-current"]
                    ["closure_status"],
                "historical"
            );
            assert_eq!(
                authoritative_state.state_payload["release_readiness_record_history"]["release-record-current"]
                    ["record_status"],
                "historical"
            );
            assert_eq!(
                authoritative_state.state_payload["final_review_record_history"]["final-review-record-current"]
                    ["record_status"],
                "historical"
            );
            assert_eq!(
                authoritative_state.state_payload["browser_qa_record_history"]["qa-record-current"]
                    ["record_status"],
                "historical"
            );
            let history =
                authoritative_state.state_payload["final_review_dispatch_lineage_history"]
                    .as_object()
                    .expect_or_abort("final-review dispatch history should be preserved");
            let archived = history
                .values()
                .find(|record| record["dispatch_id"] == "dispatch-final")
                .expect_or_abort("previous final-review dispatch should remain in history");
            assert_eq!(archived["record_status"], "historical");
            assert!(
                authoritative_state
                    .recoverable_current_branch_closure_identity()
                    .is_none(),
                "cleared structural branch state must not remain recoverable as current truth"
            );
        }
    }

    #[test]
    fn recoverable_current_branch_closure_identity_rejects_incomplete_current_record_binding() {
        let mut authoritative_state =
            transition_state_with_current_branch_closure("branch-closure-current");
        authoritative_state.state_payload["branch_closure_records"] = json!({
            "branch-closure-current": {
                "reviewed_state_id": "git_tree:current",
                "contract_identity": "branch-contract-current"
            }
        });

        assert!(
            authoritative_state
                .recoverable_current_branch_closure_identity()
                .is_none(),
            "incomplete current branch-closure records must not satisfy recoverable authoritative identity lookups"
        );
    }

    #[test]
    fn recoverable_current_branch_closure_identity_rejects_current_record_missing_arrays() {
        let mut authoritative_state =
            transition_state_with_current_branch_closure("branch-closure-current");
        authoritative_state.state_payload["branch_closure_records"] = json!({
            "branch-closure-current": {
                "branch_closure_id": "branch-closure-current",
                "source_plan_path": "docs/featureforge/plans/example.md",
                "source_plan_revision": 1,
                "repo_slug": "repo-slug",
                "branch_name": "feature-branch",
                "base_branch": "main",
                "reviewed_state_id": "git_tree:current",
                "contract_identity": "branch-contract-current",
                "effective_reviewed_branch_surface": "repo_tracked_content",
                "provenance_basis": "task_closure_lineage",
                "closure_status": "current"
            }
        });

        assert!(
            authoritative_state
                .recoverable_current_branch_closure_identity()
                .is_none(),
            "current branch-closure records missing required provenance arrays must not satisfy recoverable authoritative identity lookups"
        );
    }

    #[test]
    fn recoverable_current_branch_closure_identity_rejects_current_record_non_string_arrays() {
        let mut authoritative_state =
            transition_state_with_current_branch_closure("branch-closure-current");
        authoritative_state.state_payload["branch_closure_records"] = json!({
            "branch-closure-current": {
                "branch_closure_id": "branch-closure-current",
                "source_plan_path": "docs/featureforge/plans/example.md",
                "source_plan_revision": 1,
                "repo_slug": "repo-slug",
                "branch_name": "feature-branch",
                "base_branch": "main",
                "reviewed_state_id": "git_tree:current",
                "contract_identity": "branch-contract-current",
                "effective_reviewed_branch_surface": "repo_tracked_content",
                "source_task_closure_ids": [1],
                "provenance_basis": "task_closure_lineage",
                "closure_status": "current",
                "superseded_branch_closure_ids": []
            }
        });

        assert!(
            authoritative_state
                .recoverable_current_branch_closure_identity()
                .is_none(),
            "current branch-closure records with non-string provenance arrays must not satisfy recoverable authoritative identity lookups"
        );
    }

    #[test]
    fn recoverable_current_branch_closure_identity_rejects_empty_ordinary_lineage() {
        let mut authoritative_state =
            transition_state_with_current_branch_closure("branch-closure-current");
        authoritative_state.state_payload["branch_closure_records"] = json!({
            "branch-closure-current": {
                "branch_closure_id": "branch-closure-current",
                "source_plan_path": "docs/featureforge/plans/example.md",
                "source_plan_revision": 1,
                "repo_slug": "repo-slug",
                "branch_name": "feature-branch",
                "base_branch": "main",
                "reviewed_state_id": "git_tree:current",
                "contract_identity": "branch-contract-current",
                "effective_reviewed_branch_surface": "repo_tracked_content",
                "source_task_closure_ids": [],
                "provenance_basis": "task_closure_lineage",
                "closure_status": "current",
                "superseded_branch_closure_ids": []
            }
        });

        assert!(
            authoritative_state
                .recoverable_current_branch_closure_identity()
                .is_none(),
            "current branch-closure records with ordinary lineage but no source task closures must fail closed"
        );
    }

    #[test]
    fn recoverable_current_branch_closure_identity_rejects_unknown_provenance_basis() {
        let mut authoritative_state =
            transition_state_with_current_branch_closure("branch-closure-current");
        authoritative_state.state_payload["branch_closure_records"] = json!({
            "branch-closure-current": {
                "branch_closure_id": "branch-closure-current",
                "source_plan_path": "docs/featureforge/plans/example.md",
                "source_plan_revision": 1,
                "repo_slug": "repo-slug",
                "branch_name": "feature-branch",
                "base_branch": "main",
                "reviewed_state_id": "git_tree:current",
                "contract_identity": "branch-contract-current",
                "effective_reviewed_branch_surface": "repo_tracked_content",
                "source_task_closure_ids": ["task-1-closure"],
                "provenance_basis": "unknown_lineage_basis",
                "closure_status": "current",
                "superseded_branch_closure_ids": []
            }
        });

        assert!(
            authoritative_state
                .recoverable_current_branch_closure_identity()
                .is_none(),
            "current branch-closure records with unknown provenance basis must fail closed"
        );
    }

    #[test]
    fn task_closure_history_marks_replaced_record_superseded() {
        let mut authoritative_state = AuthoritativeTransitionState {
            state_path: PathBuf::from("/tmp/authoritative-state.json"),
            state_payload: json!({
                "run_identity": {
                    "execution_run_id": "run-unit-test",
                    "source_plan_path": "docs/featureforge/plans/example.md",
                    "source_plan_revision": 7
                },
                "current_task_closure_records": {
                    "task-1": {
                        "task": 1,
                        "record_id": "task-closure-old",
                        "record_sequence": 1,
                        "record_status": "current",
                        "closure_status": "current",
                        "dispatch_id": "dispatch-1",
                        "closure_record_id": "task-closure-old",
                        "reviewed_state_id": "git_tree:old",
                        "contract_identity": "task-contract-1",
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": "review-old",
                        "verification_result": "pass",
                        "verification_summary_hash": "verify-old"
                    }
                },
                "task_closure_record_history": {
                    "task-closure-old": {
                        "task": 1,
                        "record_id": "task-closure-old",
                        "record_sequence": 1,
                        "record_status": "current",
                        "closure_status": "current",
                        "dispatch_id": "dispatch-1",
                        "closure_record_id": "task-closure-old",
                        "reviewed_state_id": "git_tree:old",
                        "contract_identity": "task-contract-1",
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": "review-old",
                        "verification_result": "pass",
                        "verification_summary_hash": "verify-old"
                    }
                }
            }),
            phase: None,
            active_contract: None,
            dirty: false,
        };

        authoritative_state
            .remove_current_task_closure_results([1])
            .expect_or_abort("removing superseded task closure should succeed");

        assert!(
            authoritative_state.state_payload["current_task_closure_records"]["task-1"].is_null(),
            "removed current task closure overlay should no longer retain the superseded task record"
        );
        assert_eq!(
            authoritative_state.state_payload["task_closure_record_history"]["task-closure-old"]["record_status"],
            "superseded"
        );
        assert_eq!(
            authoritative_state.state_payload["task_closure_record_history"]["task-closure-old"]["closure_status"],
            "superseded"
        );
    }

    #[test]
    fn task_closure_history_marks_execution_reentry_clear_stale_unreviewed() {
        let mut authoritative_state = AuthoritativeTransitionState {
            state_path: PathBuf::from("/tmp/authoritative-state.json"),
            state_payload: json!({
                "run_identity": {
                    "execution_run_id": "run-unit-test",
                    "source_plan_path": "docs/featureforge/plans/example.md",
                    "source_plan_revision": 7
                },
                "current_task_closure_records": {
                    "task-1": {
                        "task": 1,
                        "record_id": "task-closure-old",
                        "record_sequence": 1,
                        "record_status": "current",
                        "closure_status": "current",
                        "dispatch_id": "dispatch-1",
                        "closure_record_id": "task-closure-old",
                        "reviewed_state_id": "git_tree:old",
                        "contract_identity": "task-contract-1",
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": "review-old",
                        "verification_result": "pass",
                        "verification_summary_hash": "verify-old"
                    }
                },
                "task_closure_record_history": {
                    "task-closure-old": {
                        "task": 1,
                        "record_id": "task-closure-old",
                        "record_sequence": 1,
                        "record_status": "current",
                        "closure_status": "current",
                        "dispatch_id": "dispatch-1",
                        "closure_record_id": "task-closure-old",
                        "reviewed_state_id": "git_tree:old",
                        "contract_identity": "task-contract-1",
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": "review-old",
                        "verification_result": "pass",
                        "verification_summary_hash": "verify-old"
                    }
                }
            }),
            phase: None,
            active_contract: None,
            dirty: false,
        };

        authoritative_state
            .clear_current_task_closure_results_for_execution_reentry([1])
            .expect_or_abort(
                "clearing stale current task closure for execution reentry should succeed",
            );

        assert!(
            authoritative_state.state_payload["current_task_closure_records"]["task-1"].is_null(),
            "execution-reentry clear should remove the current task closure overlay"
        );
        assert_eq!(
            authoritative_state.state_payload["task_closure_record_history"]["task-closure-old"]["record_status"],
            "stale_unreviewed"
        );
        assert_eq!(
            authoritative_state.state_payload["task_closure_record_history"]["task-closure-old"]["closure_status"],
            "stale_unreviewed"
        );
    }

    #[test]
    fn task_closure_record_persists_run_identity_bindings() {
        let mut authoritative_state = AuthoritativeTransitionState {
            state_path: PathBuf::from("/tmp/authoritative-state.json"),
            state_payload: json!({
                "run_identity": {
                    "execution_run_id": "run-unit-test",
                    "source_plan_path": "docs/featureforge/plans/example.md",
                    "source_plan_revision": 7
                },
                "current_task_closure_records": {},
                "task_closure_record_history": {}
            }),
            phase: None,
            active_contract: None,
            dirty: false,
        };

        authoritative_state
            .record_task_closure_result(TaskClosureResultRecord {
                task: 1,
                dispatch_id: "dispatch-1",
                closure_record_id: "task-closure-new",
                execution_run_id: None,
                reviewed_state_id: "git_tree:new",
                contract_identity: "task-contract-1",
                effective_reviewed_surface_paths: &["README.md".to_owned()],
                review_result: "pass",
                review_summary_hash: "review-new",
                verification_result: "pass",
                verification_summary_hash: "verify-new",
            })
            .expect_or_abort("task closure record should persist");

        let payload =
            &authoritative_state.state_payload["task_closure_record_history"]["task-closure-new"];
        assert_eq!(
            payload["source_plan_path"],
            "docs/featureforge/plans/example.md"
        );
        assert_eq!(payload["source_plan_revision"], 7);
        assert_eq!(payload["execution_run_id"], "run-unit-test");
        assert_eq!(payload["closure_status"], "current");
    }

    #[test]
    fn review_state_field_classification_marks_history_vs_overlays() {
        assert_eq!(
            classify_review_state_field("task_closure_record_history"),
            Some(PersistedReviewStateFieldClass::AuthoritativeAppendOnlyHistory)
        );
        assert_eq!(
            classify_review_state_field("current_open_step_state"),
            Some(PersistedReviewStateFieldClass::AuthoritativeMutableControlState)
        );
        assert_eq!(
            classify_review_state_field("current_task_closure_records"),
            Some(PersistedReviewStateFieldClass::DerivedCache)
        );
        assert_eq!(
            classify_review_state_field("review_state_repair_follow_up"),
            Some(PersistedReviewStateFieldClass::DerivedCache)
        );
        assert_eq!(
            classify_review_state_field("release_docs_state"),
            Some(PersistedReviewStateFieldClass::ProjectionSummary)
        );
        assert_eq!(
            classify_review_state_field("current_release_readiness_state"),
            Some(PersistedReviewStateFieldClass::Obsolete)
        );
    }
}
