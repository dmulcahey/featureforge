use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::diagnostics::{FailureClass, JsonFailure};
use crate::execution::gates::{
    ActiveContractState, GateAuthorityState, require_active_contract_state,
};
use crate::execution::leases::process_is_running;
use crate::execution::mutate::current_repo_tracked_tree_sha;
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
    fn as_str(self) -> &'static str {
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
                continue;
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
}

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

pub(crate) struct FinalReviewMilestoneRecord<'a> {
    pub(crate) branch_closure_id: &'a str,
    pub(crate) dispatch_id: &'a str,
    pub(crate) reviewer_source: &'a str,
    pub(crate) reviewer_id: &'a str,
    pub(crate) result: &'a str,
    pub(crate) final_review_fingerprint: Option<&'a str>,
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

pub(crate) struct BrowserQaResultRecord<'a> {
    pub(crate) branch_closure_id: &'a str,
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

pub(crate) struct TaskClosureResultRecord<'a> {
    pub(crate) task: u32,
    pub(crate) dispatch_id: &'a str,
    pub(crate) closure_record_id: &'a str,
    pub(crate) reviewed_state_id: &'a str,
    pub(crate) contract_identity: &'a str,
    pub(crate) effective_reviewed_surface_paths: &'a [String],
    pub(crate) review_result: &'a str,
    pub(crate) review_summary_hash: &'a str,
    pub(crate) verification_result: &'a str,
    pub(crate) verification_summary_hash: &'a str,
}

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
    pub(crate) reviewed_state_id: String,
    pub(crate) contract_identity: String,
    pub(crate) effective_reviewed_surface_paths: Vec<String>,
    pub(crate) review_result: String,
    pub(crate) review_summary_hash: String,
    pub(crate) verification_result: String,
    pub(crate) verification_summary_hash: String,
}

pub(crate) struct BranchClosureRecord {
    pub(crate) reviewed_state_id: String,
    pub(crate) contract_identity: String,
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
    pub(crate) dispatch_id: String,
    pub(crate) reviewer_source: String,
    pub(crate) reviewer_id: String,
    pub(crate) result: String,
    pub(crate) final_review_fingerprint: Option<String>,
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
        let (trigger, rationale, cycle_count, task_binding) = match cycle_target {
            Some((task, step)) => {
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
            }
            None => {
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
            }
        };
        let checkpoint_fingerprint = if cycle_count >= 3 {
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
            self.last_strategy_checkpoint_fingerprint()
        } else {
            self.record_strategy_checkpoint(
                context,
                "review_remediation",
                execution_mode,
                &trigger,
                &rationale,
                false,
            )?;
            self.last_strategy_checkpoint_fingerprint()
        };

        if let Some(strategy_checkpoint_fingerprint) = checkpoint_fingerprint {
            let execution_run_id = self.current_execution_run_id();
            if let Some((task, step)) = cycle_target {
                if let Some(task_completion_lineage) =
                    task_completion_lineage_fingerprint(context, task)
                {
                    let reviewed_state_id = format!(
                        "git_tree:{}",
                        current_repo_tracked_tree_sha(&context.runtime.repo_root)?
                    );
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
                let branch_closure_id = json_string(&self.state_payload, "current_branch_closure_id")
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
        let execution_run_id = self.current_execution_run_id();
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
                "plan={}\nplan_revision={}\nrun={execution_run_id}\ncheckpoint_kind={checkpoint_kind}\nselected_topology={selected_topology}\ntriggers={trigger_text}\nlane_decomposition={}\nlane_owner_map={}\nworktree_plan={worktree_plan}\nsubagent_dispatch_plan={subagent_dispatch_plan}\nacceptance={}\nreview={}\nrationale={}\n",
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
        {
            let lineage = self.dispatch_lineage_records_mut()?;
            lineage.insert(
                format!("task-{task}"),
                serde_json::json!({
                    "execution_run_id": execution_run_id,
                    "dispatch_id": strategy_checkpoint_fingerprint,
                    "reviewed_state_id": reviewed_state_id,
                    "source_task": task,
                    "source_step": source_step,
                    "strategy_checkpoint_fingerprint": strategy_checkpoint_fingerprint,
                    "task_completion_lineage_fingerprint": task_completion_lineage_fingerprint,
                }),
            );
        }
        self.dirty = true;
        Ok(())
    }

    fn upsert_final_review_dispatch_lineage(
        &mut self,
        execution_run_id: &str,
        branch_closure_id: &str,
        strategy_checkpoint_fingerprint: &str,
    ) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        root.insert(
            String::from("final_review_dispatch_lineage"),
            serde_json::json!({
                "execution_run_id": execution_run_id,
                "dispatch_id": strategy_checkpoint_fingerprint,
                "branch_closure_id": branch_closure_id,
            }),
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
        let reviewed_state_id = format!(
            "git_tree:{}",
            current_repo_tracked_tree_sha(&context.runtime.repo_root)?
        );
        let execution_run_id = self.current_execution_run_id();
        self.upsert_task_dispatch_lineage(
            task,
            &execution_run_id,
            source_step,
            &strategy_checkpoint_fingerprint,
            &task_completion_lineage_fingerprint,
            &reviewed_state_id,
        )
    }

    pub(crate) fn restore_downstream_truth(
        &mut self,
        final_review_fingerprint: &str,
        browser_qa_required: bool,
        browser_qa_fingerprint: Option<&str>,
        release_docs_fingerprint: Option<&str>,
    ) -> Result<(), JsonFailure> {
        let root = self.root_object_mut()?;
        root.insert(
            String::from("final_review_state"),
            Value::String(String::from("fresh")),
        );
        root.insert(
            String::from("last_final_review_artifact_fingerprint"),
            Value::String(final_review_fingerprint.to_owned()),
        );

        match (browser_qa_required, browser_qa_fingerprint) {
            (true, Some(fingerprint)) => {
                root.insert(
                    String::from("browser_qa_state"),
                    Value::String(String::from("fresh")),
                );
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::String(fingerprint.to_owned()),
                );
            }
            (true, None) => {
                root.insert(
                    String::from("browser_qa_state"),
                    Value::String(String::from("stale")),
                );
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::Null,
                );
            }
            (false, _) => {
                root.insert(
                    String::from("browser_qa_state"),
                    Value::String(String::from("not_required")),
                );
                root.insert(
                    String::from("last_browser_qa_artifact_fingerprint"),
                    Value::Null,
                );
            }
        }

        match release_docs_fingerprint {
            Some(fingerprint) => {
                root.insert(
                    String::from("release_docs_state"),
                    Value::String(String::from("fresh")),
                );
                root.insert(
                    String::from("last_release_docs_artifact_fingerprint"),
                    Value::String(fingerprint.to_owned()),
                );
            }
            None => {
                root.insert(
                    String::from("release_docs_state"),
                    Value::String(String::from("stale")),
                );
                root.insert(
                    String::from("last_release_docs_artifact_fingerprint"),
                    Value::Null,
                );
            }
        }
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn set_current_branch_closure_id(
        &mut self,
        branch_closure_id: &str,
        reviewed_state_id: &str,
        contract_identity: &str,
    ) -> Result<(), JsonFailure> {
        let previous_branch_closure_id =
            json_string(&self.state_payload, "current_branch_closure_id");
        let previous_release_readiness_record_id =
            json_string(&self.state_payload, "current_release_readiness_record_id");
        let previous_final_review_record_id =
            json_string(&self.state_payload, "current_final_review_record_id");
        let previous_qa_record_id = json_string(&self.state_payload, "current_qa_record_id");
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
        }
        self.dirty = true;

        if previous_branch_closure_id
            .as_deref()
            .is_some_and(|value| value != branch_closure_id)
        {
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
        if let Some(previous_final_review_record_id) = previous_final_review_record_id.as_deref() {
            let records = self.final_review_record_history_mut()?;
            mark_record_status(records, previous_final_review_record_id, "historical");
        }
        if let Some(previous_qa_record_id) = previous_qa_record_id.as_deref() {
            let records = self.browser_qa_record_history_mut()?;
            mark_record_status(records, previous_qa_record_id, "historical");
        }
        Ok(())
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
        let Some((record_id, payload)) = self.current_record_entry(
            "current_final_review_record_id",
            "final_review_record_history",
        ) else {
            return Ok(false);
        };
        let branch_closure_id = json_string(&payload, "branch_closure_id").ok_or_else(|| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                "Current final-review record is missing branch_closure_id.",
            )
        })?;
        if json_string(&self.state_payload, "current_branch_closure_id")
            .as_deref()
            .is_some_and(|current| current != branch_closure_id)
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
        if json_string(&self.state_payload, "current_branch_closure_id")
            .as_deref()
            .is_some_and(|current| current != branch_closure_id)
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
        if json_string(&self.state_payload, "current_branch_closure_id").as_deref()
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
        records.insert(
            record.branch_closure_id.to_owned(),
            serde_json::json!({
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
            }),
        );
        self.root_object_mut()?.insert(
            String::from("harness_phase"),
            Value::String(String::from("document_release_pending")),
        );
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn record_release_readiness_result(
        &mut self,
        record: ReleaseReadinessResultRecord<'_>,
    ) -> Result<(), JsonFailure> {
        let previous_record_id =
            json_string(&self.state_payload, "current_release_readiness_record_id");
        let source_plan_revision = record.source_plan_revision.to_string();
        let history = self.release_readiness_record_history_mut()?;
        let record_id = deterministic_record_id(
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
                "record_id": record_id.clone(),
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
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn record_final_review_result(
        &mut self,
        record: FinalReviewMilestoneRecord<'_>,
    ) -> Result<(), JsonFailure> {
        let previous_record_id = json_string(&self.state_payload, "current_final_review_record_id");
        let source_plan_revision = record.source_plan_revision.to_string();
        let browser_qa_required = record
            .browser_qa_required
            .map(|value| value.to_string())
            .unwrap_or_else(|| String::from("none"));
        let history = self.final_review_record_history_mut()?;
        let record_id = deterministic_record_id(
            "final-review",
            &[
                record.branch_closure_id,
                record.dispatch_id,
                record.reviewer_source,
                record.reviewer_id,
                record.result,
                record.summary_hash,
                record.final_review_fingerprint.unwrap_or("none"),
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
                "record_id": record_id.clone(),
                "record_sequence": record_sequence,
                "record_status": "current",
                "branch_closure_id": record.branch_closure_id,
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
                "browser_qa_required": record.browser_qa_required,
                "summary": record.summary,
                "summary_hash": record.summary_hash,
            }),
        );
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
        self.dirty = true;
        Ok(())
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
                "record_id": record_id.clone(),
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
            .map(|history| history.len() as u64 + 1)
            .unwrap_or(1);
        let payload = serde_json::json!({
            "task": record.task,
            "record_id": record.closure_record_id,
            "record_sequence": record_sequence,
            "record_status": "current",
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

    pub(crate) fn remove_current_task_closure_results(
        &mut self,
        tasks: impl IntoIterator<Item = u32>,
    ) -> Result<(), JsonFailure> {
        let tasks = tasks.into_iter().collect::<Vec<_>>();
        let removed_closure_ids = tasks
            .iter()
            .filter_map(|task| self.current_task_closure_result(*task))
            .map(|record| record.closure_record_id)
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
                mark_record_status(history, &closure_record_id, "historical");
            }
            removed_any = true;
        }
        if removed_any {
            self.dirty = true;
        }
        Ok(())
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
            .map(|history| history.len() as u64 + 1)
            .unwrap_or(1);
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
        self.task_closure_negative_result_history_mut()?
            .insert(format!("task-{}:{}", record.task, record.dispatch_id), payload);
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
        let payload = self.current_record_payload(
            "current_release_readiness_record_id",
            "release_readiness_record_history",
        )?;
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
        let payload = self.current_record_payload(
            "current_final_review_record_id",
            "final_review_record_history",
        )?;
        Some(CurrentFinalReviewRecord {
            record_status: json_string(&payload, "record_status")?,
            branch_closure_id: json_string(&payload, "branch_closure_id")?,
            dispatch_id: json_string(&payload, "dispatch_id")?,
            reviewer_source: json_string(&payload, "reviewer_source")?,
            reviewer_id: json_string(&payload, "reviewer_id")?,
            result: json_string(&payload, "result")?,
            final_review_fingerprint: json_string(&payload, "final_review_fingerprint"),
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

    pub(crate) fn raw_current_task_closure_results(&self) -> BTreeMap<u32, CurrentTaskClosureRecord> {
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
                        if json_string(&payload_value, "record_status").as_deref() != Some("current")
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
        let mut restored = serde_json::Map::new();
        for record in recoverable.into_values() {
            restored.insert(
                format!("task-{}", record.task),
                serde_json::json!({
                    "task": record.task,
                    "record_id": record.closure_record_id,
                    "record_status": "current",
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
        self.root_object_mut()?
            .insert(String::from("current_task_closure_records"), Value::Object(restored));
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
        let _ = json_string(&payload_value, "source_plan_path")?;
        let _ = json_u32(&payload_value, "source_plan_revision")?;
        let _ = json_string(&payload_value, "repo_slug")?;
        let _ = json_string(&payload_value, "branch_name")?;
        let _ = json_string(&payload_value, "base_branch")?;
        let _ = json_string(&payload_value, "effective_reviewed_branch_surface")?;
        let _ = json_string(&payload_value, "provenance_basis")?;
        let _ = json_string_array(&payload_value, "source_task_closure_ids");
        let _ = json_string_array(&payload_value, "superseded_branch_closure_ids");
        Some(BranchClosureRecord {
            reviewed_state_id: json_string(&payload_value, "reviewed_state_id")?,
            contract_identity: json_string(&payload_value, "contract_identity")?,
        })
    }

    pub(crate) fn recoverable_current_branch_closure_identity(
        &self,
    ) -> Option<CurrentBranchClosureIdentity> {
        if let Some(branch_closure_id) = json_string(&self.state_payload, "current_branch_closure_id")
            && let Some(record) = self.branch_closure_record(&branch_closure_id)
        {
            return Some(CurrentBranchClosureIdentity {
                branch_closure_id,
                reviewed_state_id: record.reviewed_state_id,
                contract_identity: record.contract_identity,
            });
        }

        let records = self
            .state_payload
            .get("branch_closure_records")
            .and_then(Value::as_object)?;
        let mut candidates = records.keys().filter_map(|branch_closure_id| {
            self.branch_closure_record(branch_closure_id)
                .map(|record| CurrentBranchClosureIdentity {
                    branch_closure_id: branch_closure_id.clone(),
                    reviewed_state_id: record.reviewed_state_id,
                    contract_identity: record.contract_identity,
                })
        });
        let current = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        Some(current)
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

    fn task_closure_negative_results_from_history(&self) -> BTreeMap<u32, TaskClosureNegativeResult> {
        self.state_payload
            .get("task_closure_negative_result_history")
            .and_then(Value::as_object)
            .map(|history| {
                history
                    .values()
                    .filter_map(Value::as_object)
                    .filter_map(|payload| {
                        let payload_value = Value::Object(payload.clone());
                        if json_string(&payload_value, "record_status").as_deref() != Some("current")
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
        let payload =
            self.current_record_payload("current_qa_record_id", "browser_qa_record_history")?;
        Some(CurrentBrowserQaRecord {
            record_status: json_string(&payload, "record_status")?,
            branch_closure_id: json_string(&payload, "branch_closure_id")?,
            source_plan_path: json_string(&payload, "source_plan_path")?,
            source_plan_revision: json_u32(&payload, "source_plan_revision")?,
            repo_slug: json_string(&payload, "repo_slug")?,
            branch_name: json_string(&payload, "branch_name")?,
            base_branch: json_string(&payload, "base_branch")?,
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

    fn current_record_payload(&self, current_id_key: &str, history_key: &str) -> Option<Value> {
        self.current_record_entry(current_id_key, history_key)
            .map(|(_, payload)| payload)
    }

    fn current_execution_run_id(&self) -> String {
        self.state_payload
            .get("run_identity")
            .and_then(Value::as_object)
            .and_then(|run_identity| run_identity.get("execution_run_id"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown-run")
            .to_owned()
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
        })
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

fn load_authoritative_transition_state_internal(
    context: &ExecutionContext,
    require_active_contract: bool,
) -> Result<Option<AuthoritativeTransitionState>, JsonFailure> {
    let state_path = harness_state_path(
        &context.runtime.state_dir,
        &context.runtime.repo_slug,
        &context.runtime.branch_name,
    );
    if !state_path.is_file() {
        return Ok(None);
    }

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
    let gate_state: GateAuthorityState =
        serde_json::from_value(state_payload.clone()).map_err(|error| {
            JsonFailure::new(
                FailureClass::MalformedExecutionState,
                format!(
                    "Authoritative harness state is malformed in {}: {error}",
                    state_path.display()
                ),
            )
        })?;

    let active_contract = if require_active_contract && has_active_contract_pointer(&gate_state) {
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
        } else {
            active
        }
    } else {
        None
    };

    Ok(Some(AuthoritativeTransitionState {
        state_path,
        state_payload,
        phase: gate_state.harness_phase.clone(),
        active_contract,
        dirty: false,
    }))
}

pub(crate) fn load_authoritative_transition_state(
    context: &ExecutionContext,
) -> Result<Option<AuthoritativeTransitionState>, JsonFailure> {
    load_authoritative_transition_state_internal(context, true)
}

pub(crate) fn load_authoritative_transition_state_relaxed(
    context: &ExecutionContext,
) -> Result<Option<AuthoritativeTransitionState>, JsonFailure> {
    load_authoritative_transition_state_internal(context, false)
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
    let message = gate
        .diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.clone())
        .unwrap_or_else(|| default_message.to_owned());
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
        reviewed_state_id: json_string(&payload_value, "reviewed_state_id")?,
        contract_identity: json_string(&payload_value, "contract_identity")?,
        effective_reviewed_surface_paths: json_string_array(
            &payload_value,
            "effective_reviewed_surface_paths",
        ),
        review_result: json_string(&payload_value, "review_result")?,
        review_summary_hash: json_string(&payload_value, "review_summary_hash")?,
        verification_result: json_string(&payload_value, "verification_result")?,
        verification_summary_hash: json_string(&payload_value, "verification_summary_hash")?,
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
    use super::FinalReviewMilestoneRecord;
    use super::ReleaseReadinessResultRecord;
    use super::remove_stale_write_authority_lock;
    use std::path::PathBuf;

    use serde_json::{Value, json};

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
        let tempdir = tempfile::tempdir().expect("tempdir should be creatable");
        let lock_path = tempdir.path().join("write-authority.lock");
        remove_stale_write_authority_lock(&lock_path)
            .expect("already-removed stale write-authority lock should be treated as reclaimed");
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
            .expect("current branch-closure identity should be recoverable");

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
            .expect("stale finish-review checkpoint check should succeed");

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
            .expect("first release-readiness record should succeed");
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
            .expect("second release-readiness record should append");

        let history = authoritative_state.state_payload["release_readiness_record_history"]
            .as_object()
            .expect("release readiness history should be an object");
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
    fn final_review_history_appends_in_record_order() {
        let mut authoritative_state = transition_state_with_milestone_histories();

        authoritative_state
            .record_final_review_result(FinalReviewMilestoneRecord {
                branch_closure_id: "branch-closure-current",
                dispatch_id: "dispatch-a",
                reviewer_source: "fresh-context-subagent",
                reviewer_id: "reviewer-a",
                result: "pass",
                final_review_fingerprint: Some("final-fingerprint-a"),
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
            .expect("first final-review record should succeed");
        authoritative_state
            .record_final_review_result(FinalReviewMilestoneRecord {
                branch_closure_id: "branch-closure-current",
                dispatch_id: "dispatch-b",
                reviewer_source: "fresh-context-subagent",
                reviewer_id: "reviewer-b",
                result: "pass",
                final_review_fingerprint: Some("final-fingerprint-b"),
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
            .expect("second final-review record should append");

        let history = authoritative_state.state_payload["final_review_record_history"]
            .as_object()
            .expect("final review history should be an object");
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
            .expect("first browser QA record should succeed");
        authoritative_state
            .record_browser_qa_result(BrowserQaResultRecord {
                branch_closure_id: "branch-closure-current",
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
            .expect("second browser QA record should append");

        let history = authoritative_state.state_payload["browser_qa_record_history"]
            .as_object()
            .expect("browser QA history should be an object");
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
            })
            .expect("second branch-closure record should succeed");
        authoritative_state
            .set_current_branch_closure_id(
                "branch-closure-new",
                "git_tree:new",
                "branch-contract-new",
            )
            .expect("current branch-closure update should succeed");

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
}
