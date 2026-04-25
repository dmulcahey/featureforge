#[path = "support/files.rs"]
mod files_support;
#[path = "support/plan_execution_direct.rs"]
mod plan_execution_direct_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use std::collections::BTreeMap;
use std::path::Path;

use featureforge::cli::plan_execution::StatusArgs;
use featureforge::cli::workflow::OperatorArgs;
use featureforge::execution::semantic_identity::task_definition_identity_for_task;
use featureforge::execution::state::{ExecutionRuntime, PlanExecutionStatus};
use featureforge::git::{discover_slug_identity, sha256_hex};
use featureforge::paths::harness_state_path;
use featureforge::workflow::operator;
use serde_json::{Value, json};

const PLAN_REL: &str = "docs/featureforge/plans/2026-04-01-liveness-model-plan.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-04-01-liveness-model-spec.md";
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ProgressMetric {
    structural_blockers: u8,
    stale_boundaries: u8,
    dispatch_lineage_blockers: u8,
    task_scope_distance: u8,
    late_stage_blockers: u8,
    execution_frontier_distance: u8,
    repair_scope_distance: u8,
    remaining_tasks: u8,
}

#[derive(Debug, Clone, Copy)]
struct SyntheticState {
    completed_tasks: u8,
    stale_boundary_present: bool,
    structural_blocker_present: bool,
    late_stage_blocker_present: bool,
    dispatch_lineage_missing: bool,
}

struct SyntheticFixtureContext<'a> {
    reviewed_state_id: &'a str,
    state_path: &'a Path,
    total_tasks: u8,
    task_contract_identities: &'a BTreeMap<u32, String>,
    task_completion_lineages: &'a BTreeMap<u32, String>,
    branch_definition_identity: &'a str,
    repo_slug: &'a str,
    branch_name: &'a str,
}

fn write_spec_and_plan(repo: &Path, total_tasks: u8, completed_tasks: u8) {
    files_support::write_file(
        &repo.join(SPEC_REL),
        "# Liveness Model Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [VERIFY-001][verification] Runtime routes a legal public next step.\n",
    );

    let execution_mode = if completed_tasks == 0 {
        "none"
    } else {
        "featureforge:executing-plans"
    };
    let mut plan = format!(
        "# Liveness Model Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** {execution_mode}\n**Source Spec:** `{SPEC_REL}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Index\n\n- [VERIFY-001][verification] Runtime routes a legal public next step.\n\n## Requirement Coverage Matrix\n\n- VERIFY-001 -> Task 1\n"
    );

    for task in 1..=u32::from(total_tasks) {
        let checkbox = if task <= u32::from(completed_tasks) {
            "[x]"
        } else {
            "[ ]"
        };
        plan.push_str(&format!(
            "\n## Task {task}: Liveness task {task}\n\n**Spec Coverage:** VERIFY-001\n**Goal:** Runtime routes a legal public next step.\n\n**Context:**\n- Spec Coverage: VERIFY-001.\n\n**Constraints:**\n- Keep one step per task.\n**Done when:**\n- Runtime routes a legal public next step.\n\n**Files:**\n- Modify: `docs/liveness-task-{task}.md`\n\n- {checkbox} **Step 1: Execute task {task}**\n"
        ));
    }

    files_support::write_file(&repo.join(PLAN_REL), &plan);
    write_execution_evidence(repo, completed_tasks);
}

fn write_execution_evidence(repo: &Path, completed_tasks: u8) {
    let plan_fingerprint =
        sha256_hex(&std::fs::read(repo.join(PLAN_REL)).expect("liveness plan should be readable"));
    let spec_fingerprint =
        sha256_hex(&std::fs::read(repo.join(SPEC_REL)).expect("liveness spec should be readable"));
    let mut attempts = String::new();
    for task in 1..=u32::from(completed_tasks) {
        attempts.push_str(&format!(
            "### Task {task} Step 1\n#### Attempt 1\n**Status:** Completed\n**Recorded At:** 2026-04-01T00:00:0{task}Z\n**Execution Source:** featureforge:executing-plans\n**Task Number:** {task}\n**Step Number:** 1\n**Packet Fingerprint:** packet-{task}\n**Head SHA:** 1111111111111111111111111111111111111111\n**Base SHA:** 1111111111111111111111111111111111111111\n**Claim:** Completed task {task} step 1.\n**Files Proven:**\n- README.md | sha256:aaaaaaaa\n**Verification Summary:** Synthetic liveness evidence.\n**Invalidation Reason:** N/A\n\n"
        ));
    }
    files_support::write_file(
        &repo.join(
            "docs/featureforge/execution-evidence/2026-04-01-liveness-model-plan-r1-evidence.md",
        ),
        &format!(
            "# Execution Evidence: 2026-04-01-liveness-model-plan\n\n**Plan Path:** {PLAN_REL}\n**Plan Revision:** 1\n**Plan Fingerprint:** {plan_fingerprint}\n**Source Spec Path:** {SPEC_REL}\n**Source Spec Revision:** 1\n**Source Spec Fingerprint:** {spec_fingerprint}\n\n## Step Evidence\n\n{attempts}"
        ),
    );
}

fn harness_state_file_path(repo: &Path, state: &Path) -> std::path::PathBuf {
    let identity = discover_slug_identity(repo);
    harness_state_path(
        state,
        identity.repo_slug.as_str(),
        identity.branch_name.as_str(),
    )
}

fn semantic_execution_context(
    repo: &Path,
    state_dir: &Path,
) -> featureforge::execution::state::ExecutionContext {
    let runtime = runtime_for_status(repo, state_dir);
    featureforge::execution::state::load_execution_context(&runtime, Path::new(PLAN_REL))
        .expect("liveness semantic execution context should load")
}

fn task_contract_identities(
    repo: &Path,
    state_dir: &Path,
    total_tasks: u8,
) -> BTreeMap<u32, String> {
    let context = semantic_execution_context(repo, state_dir);
    (1..=u32::from(total_tasks))
        .map(|task| {
            let identity = task_definition_identity_for_task(&context, task)
                .expect("liveness closure record should compute task semantic identity")
                .unwrap_or_else(|| format!("task-contract-{task}"));
            (task, identity)
        })
        .collect()
}

fn deterministic_record_id(prefix: &str, parts: &[&str]) -> String {
    let mut payload = prefix.to_owned();
    for part in parts {
        payload.push('\n');
        payload.push_str(part);
    }
    format!("{prefix}-{}", &sha256_hex(payload.as_bytes())[..16])
}

fn task_completion_lineage_fingerprints(
    repo: &Path,
    state_dir: &Path,
    total_tasks: u8,
) -> BTreeMap<u32, String> {
    let context = semantic_execution_context(repo, state_dir);
    (1..=u32::from(total_tasks))
        .filter_map(|task| {
            let step = context
                .steps
                .iter()
                .find(|step| step.task_number == task && step.step_number == 1)?;
            if !step.checked {
                return None;
            }
            let recorded_at = format!("2026-04-01T00:00:0{task}Z");
            let payload = format!(
                "plan={PLAN_REL}\nplan_revision=1\ntask={task}\nstep=1:attempt=1:recorded_at={recorded_at}:packet=packet-{task}:checkpoint=1111111111111111111111111111111111111111\n"
            );
            Some((task, sha256_hex(payload.as_bytes())))
        })
        .collect()
}

fn closure_record(
    reviewed_state_id: &str,
    task_contract_identities: &BTreeMap<u32, String>,
    task_completion_lineages: &BTreeMap<u32, String>,
    task: u32,
) -> Value {
    let contract_identity = task_contract_identities
        .get(&task)
        .cloned()
        .unwrap_or_else(|| format!("task-contract-{task}"));
    let closure_record_id = task_completion_lineages
        .get(&task)
        .map(|lineage| {
            deterministic_record_id("task-closure", &[PLAN_REL, &task.to_string(), lineage])
        })
        .unwrap_or_else(|| format!("closure-{task}"));
    json!({
        "task": task,
        "dispatch_id": format!("dispatch-{task}"),
        "closure_record_id": closure_record_id,
        "source_plan_path": PLAN_REL,
            "source_plan_revision": 1,
            "execution_run_id": "run-liveness",
            "reviewed_state_id": reviewed_state_id,
        "contract_identity": contract_identity,
        "effective_reviewed_surface_paths": ["README.md"],
        "review_result": "pass",
        "review_summary_hash": "aaaaaaaa",
        "verification_result": "pass",
        "verification_summary_hash": "bbbbbbbb",
        "closure_status": "current",
        "record_status": "current",
        "record_sequence": task
    })
}

fn write_variant_harness_state(fixture: &SyntheticFixtureContext<'_>, synthetic: SyntheticState) {
    let mut current_records = serde_json::Map::new();
    let mut history_records = serde_json::Map::new();
    let mut dispatch_lineage = serde_json::Map::new();
    let mut event_completed_steps = serde_json::Map::new();
    let boundary_task = u32::from(synthetic.completed_tasks.max(1));
    for task in 1..=u32::from(synthetic.completed_tasks) {
        event_completed_steps.insert(
            format!("task-{task}-step-1"),
            json!({
                "task": task,
                "step": 1,
                "record_status": "current",
            }),
        );
        let record_reviewed_state_id = if synthetic.stale_boundary_present && task == boundary_task
        {
            "git_tree:0000000000000000000000000000000000000000"
        } else {
            fixture.reviewed_state_id
        };
        let record = closure_record(
            record_reviewed_state_id,
            fixture.task_contract_identities,
            fixture.task_completion_lineages,
            task,
        );
        if !(synthetic.structural_blocker_present && task == boundary_task) {
            current_records.insert(format!("task-{task}"), record.clone());
        }
        let history_key = record["closure_record_id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("closure-{task}"));
        history_records.insert(history_key, record);
        if !synthetic.dispatch_lineage_missing {
            dispatch_lineage.insert(
                format!("task-{task}"),
                json!({
                    "dispatch_id": format!("dispatch-{task}"),
                    "reviewed_state_id": record_reviewed_state_id,
                    "execution_run_id": "run-liveness",
                    "source_task": task,
                    "source_step": 1,
                    "strategy_checkpoint_fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "task_completion_lineage_fingerprint": fixture.task_completion_lineages
                        .get(&task)
                        .cloned()
                        .unwrap_or_else(|| format!("task-lineage-{task}"))
                }),
            );
        }
    }

    let mut harness_phase =
        if synthetic.completed_tasks >= fixture.total_tasks && fixture.total_tasks > 0 {
            String::from("ready_for_branch_completion")
        } else {
            String::from("executing")
        };

    if synthetic.stale_boundary_present {
        history_records.insert(format!("task-{boundary_task}-stale"), {
            let mut stale_record = closure_record(
                "git_tree:0000000000000000000000000000000000000000",
                fixture.task_contract_identities,
                fixture.task_completion_lineages,
                boundary_task,
            );
            stale_record["closure_record_id"] =
                Value::from(format!("closure-{boundary_task}-stale"));
            stale_record["review_summary_hash"] = Value::from("cccccccc");
            stale_record["verification_summary_hash"] = Value::from("dddddddd");
            stale_record["closure_status"] = Value::from("stale_unreviewed");
            stale_record["record_status"] = Value::from("stale_unreviewed");
            stale_record["record_sequence"] = Value::from(0);
            stale_record
        });
    }

    if synthetic.late_stage_blocker_present {
        harness_phase = String::from("final_review_pending");
    }
    let branch_closure_id = "branch-closure-liveness";
    let source_task_closure_ids = (1..=u32::from(synthetic.completed_tasks))
        .map(|task| {
            fixture
                .task_completion_lineages
                .get(&task)
                .map(|lineage| {
                    deterministic_record_id("task-closure", &[PLAN_REL, &task.to_string(), lineage])
                })
                .unwrap_or_else(|| format!("closure-{task}"))
        })
        .collect::<Vec<_>>();
    let branch_closure_records = if synthetic.late_stage_blocker_present {
        json!({
            branch_closure_id: {
                "branch_closure_id": branch_closure_id,
                "source_plan_path": PLAN_REL,
                "source_plan_revision": 1,
                "repo_slug": fixture.repo_slug,
                "branch_name": fixture.branch_name,
                "base_branch": "main",
                "reviewed_state_id": fixture.reviewed_state_id,
                "contract_identity": fixture.branch_definition_identity,
                "effective_reviewed_branch_surface": "repo_tracked_content",
                "source_task_closure_ids": source_task_closure_ids,
                "provenance_basis": "task_closure_lineage",
                "closure_status": "current",
                "record_status": "current",
                "record_sequence": u64::from(synthetic.completed_tasks).saturating_add(1),
                "superseded_branch_closure_ids": []
            }
        })
    } else {
        json!({})
    };
    let current_branch_closure_id = if synthetic.late_stage_blocker_present {
        Value::from(branch_closure_id)
    } else {
        Value::Null
    };
    let current_branch_closure_reviewed_state_id = if synthetic.late_stage_blocker_present {
        Value::from(fixture.reviewed_state_id)
    } else {
        Value::Null
    };
    let current_branch_closure_contract_identity = if synthetic.late_stage_blocker_present {
        Value::from(fixture.branch_definition_identity)
    } else {
        Value::Null
    };
    let release_readiness_record_id = "release-readiness-liveness";
    let release_readiness_record_history = if synthetic.late_stage_blocker_present {
        json!({
            release_readiness_record_id: {
                "record_id": release_readiness_record_id,
                "record_sequence": u64::from(synthetic.completed_tasks).saturating_add(2),
                "record_status": "current",
                "branch_closure_id": branch_closure_id,
                "source_plan_path": PLAN_REL,
                "source_plan_revision": 1,
                "repo_slug": fixture.repo_slug,
                "branch_name": fixture.branch_name,
                "base_branch": "main",
                "reviewed_state_id": fixture.reviewed_state_id,
                "result": "ready",
                "summary": "Synthetic release readiness.",
                "summary_hash": "eeeeeeee",
                "generated_by_identity": "featureforge/liveness"
            }
        })
    } else {
        json!({})
    };
    let final_review_dispatch_id = "final-dispatch-liveness";
    let final_review_dispatch_lineage = if synthetic.late_stage_blocker_present {
        json!({
            "record_id": "final-review-dispatch-liveness",
            "record_sequence": u64::from(synthetic.completed_tasks).saturating_add(3),
            "record_status": "current",
            "status": "current",
            "execution_run_id": "run-liveness",
            "dispatch_id": final_review_dispatch_id,
            "branch_closure_id": branch_closure_id,
            "dispatch_provenance": {
                "scope_type": "final_review",
                "scope_key": branch_closure_id,
                "strategy_checkpoint_fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }
        })
    } else {
        Value::Null
    };
    let current_release_readiness_result = if synthetic.late_stage_blocker_present {
        Value::from("ready")
    } else {
        Value::Null
    };
    let current_release_readiness_record_id = if synthetic.late_stage_blocker_present {
        Value::from(release_readiness_record_id)
    } else {
        Value::Null
    };
    let current_final_review_dispatch_id = if synthetic.late_stage_blocker_present {
        Value::from(final_review_dispatch_id)
    } else {
        Value::Null
    };
    let payload = json!({
        "schema_version": 1,
        "harness_phase": harness_phase,
        "authoritative_sequence": 1,
        "latest_authoritative_sequence": 1,
        "source_plan_path": PLAN_REL,
        "source_plan_revision": 1,
        "execution_run_id": "run-liveness",
        "current_task_closure_records": Value::Object(current_records),
        "event_completed_steps": Value::Object(event_completed_steps),
        "task_closure_record_history": Value::Object(history_records),
        "task_closure_negative_result_records": {},
        "current_branch_closure_id": current_branch_closure_id,
        "current_branch_closure_reviewed_state_id": current_branch_closure_reviewed_state_id,
        "current_branch_closure_contract_identity": current_branch_closure_contract_identity,
        "branch_closure_records": branch_closure_records,
        "current_release_readiness_result": current_release_readiness_result,
        "current_release_readiness_record_id": current_release_readiness_record_id,
        "release_readiness_record_history": release_readiness_record_history,
        "final_review_record_history": {},
        "browser_qa_record_history": {},
        "strategy_review_dispatch_lineage": Value::Object(dispatch_lineage),
        "strategy_review_dispatch_lineage_history": {},
        "current_final_review_dispatch_id": current_final_review_dispatch_id,
        "final_review_dispatch_lineage": final_review_dispatch_lineage,
        "superseded_branch_closure_ids": [],
        "strategy_state": "checkpoint_recorded",
        "strategy_checkpoint_kind": "task",
        "last_strategy_checkpoint_fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    });
    files_support::write_file(
        fixture.state_path,
        &serde_json::to_string(&payload).expect("payload should serialize"),
    );
}

fn runtime_for_status(repo: &Path, state: &Path) -> ExecutionRuntime {
    let mut runtime =
        ExecutionRuntime::discover(repo).expect("liveness checker repo should be discoverable");
    runtime.state_dir = state.to_path_buf();
    runtime
}

fn status_value(runtime: &ExecutionRuntime, context: &str) -> PlanExecutionStatus {
    runtime
        .status(&StatusArgs {
            plan: PLAN_REL.into(),
            external_review_result_ready: false,
        })
        .unwrap_or_else(|error| panic!("{context} should succeed: {error:?}"))
}

fn migrate_variant_to_event_authority(state_path: &Path, context: &str) {
    let payload: Value = serde_json::from_str(
        &std::fs::read_to_string(state_path).unwrap_or_else(|error| {
            panic!("{context} should read synthetic state fixture: {error}")
        }),
    )
    .unwrap_or_else(|error| {
        panic!("{context} should deserialize synthetic state fixture: {error}")
    });
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(state_path, &payload)
        .unwrap_or_else(|error| panic!("{context} should succeed: {error:?}"));
    std::fs::remove_file(state_path)
        .expect("liveness migration should permit removing state projection after event publish");
}

fn contains_hidden_command_token(command: &str) -> bool {
    [
        " workflow record-pivot ",
        " plan execution preflight ",
        " plan execution recommend ",
        " plan execution record-review-dispatch ",
        " plan execution gate-review ",
        " plan execution gate-finish ",
        " plan execution rebuild-evidence ",
        " reconcile-review-state ",
        " internal ",
    ]
    .iter()
    .any(|token| command.contains(token))
}

fn progress_metric_from_status(status: &PlanExecutionStatus, total_tasks: u8) -> ProgressMetric {
    let stale_boundaries = u8::from(!status.stale_unreviewed_closures.is_empty());
    let structural_blockers = if status.blocking_reason_codes.iter().any(|reason| {
        matches!(
            reason.as_str(),
            "prior_task_current_closure_invalid"
                | "prior_task_current_closure_reviewed_state_malformed"
                | "current_task_closure_overlay_restore_required"
        )
    }) {
        if status
            .blocking_reason_codes
            .iter()
            .any(|reason| reason == "current_task_closure_overlay_restore_required")
        {
            2
        } else {
            3
        }
    } else {
        u8::from(
            status
                .blockers
                .iter()
                .any(|blocker| blocker.category == "structural"),
        )
    };
    let dispatch_lineage_blockers = u8::from(
        status
            .blocking_reason_codes
            .iter()
            .any(|reason| reason.contains("dispatch")),
    );
    let late_stage_blockers = match status.phase_detail.as_str() {
        "branch_closure_recording_required_for_release_readiness" => 6,
        "release_readiness_recording_ready" | "release_blocker_resolution_required" => 5,
        "final_review_dispatch_required" => 4,
        "final_review_outcome_pending" | "final_review_recording_ready" => 3,
        "test_plan_refresh_required" | "qa_recording_required" => 2,
        "finish_review_gate_ready" => 1,
        "finish_completion_gate_ready" => 0,
        _ => 0,
    };
    let current_closures = status
        .current_task_closures
        .len()
        .try_into()
        .unwrap_or(u8::MAX);
    let task_scope_distance = if status.active_task.is_some() {
        0
    } else {
        status
            .blocking_task
            .and_then(|task| u8::try_from(task).ok())
            .map(|task| total_tasks.saturating_sub(task).saturating_add(1))
            .unwrap_or_else(|| total_tasks.saturating_sub(current_closures))
    };
    let execution_frontier_distance = match status.phase_detail.as_str() {
        "execution_preflight_required" | "execution_reentry_required" => 2,
        "execution_in_progress" => 1,
        _ => 0,
    };
    let repair_scope_distance = u8::from(
        status.phase_detail == "execution_reentry_required"
            && status.blocking_scope.as_deref() == Some("task")
            && status.execution_command_context.is_none(),
    );

    ProgressMetric {
        structural_blockers,
        stale_boundaries,
        dispatch_lineage_blockers,
        task_scope_distance,
        late_stage_blockers,
        execution_frontier_distance,
        repair_scope_distance,
        remaining_tasks: total_tasks.saturating_sub(current_closures),
    }
}

fn summary_file(state: &Path, label: &str) -> std::path::PathBuf {
    let path = state.join(format!("{label}.md"));
    files_support::write_file(
        &path,
        &format!("# {label}\n\nSynthetic liveness summary.\n"),
    );
    path
}

fn materialize_public_progress_command(
    runtime: &ExecutionRuntime,
    status: &PlanExecutionStatus,
) -> Option<String> {
    if let Some(command) = status.recommended_command.as_deref() {
        return Some(command.to_owned());
    }
    if let Some(action) = status.next_public_action.as_ref() {
        return Some(action.command.clone());
    }
    if let Some(command) = status
        .blockers
        .iter()
        .find_map(|blocker| blocker.next_public_action.as_ref())
    {
        return Some(command.clone());
    }
    let operator_output = operator::operator_for_runtime(
        runtime,
        &OperatorArgs {
            plan: PLAN_REL.into(),
            external_review_result_ready: false,
            json: true,
        },
    )
    .ok()?;
    operator_output.recommended_command
}

fn execute_public_progress_edge(
    runtime: &ExecutionRuntime,
    state: &Path,
    status: &PlanExecutionStatus,
) -> Result<Option<PlanExecutionStatus>, String> {
    let Some(command) = materialize_public_progress_command(runtime, status) else {
        return Ok(None);
    };
    execute_materialized_public_command(runtime, state, status, &command)?;
    Ok(Some(status_value(
        runtime,
        &format!("liveness successor after `{command}`"),
    )))
}

fn execute_materialized_public_command(
    runtime: &ExecutionRuntime,
    state: &Path,
    status: &PlanExecutionStatus,
    command: &str,
) -> Result<(), String> {
    let materialized = materialize_public_command_template(state, command);
    let Some(parts) = shlex::split(&materialized) else {
        return Err(format!(
            "public command is not shell-parseable: {materialized}"
        ));
    };
    let words = parts.iter().map(String::as_str).collect::<Vec<_>>();
    match words.as_slice() {
        ["featureforge", "workflow", "operator", ..] => {
            let operator_output = operator::operator_for_runtime(
                runtime,
                &OperatorArgs {
                    plan: PLAN_REL.into(),
                    external_review_result_ready: false,
                    json: true,
                },
            )
            .map_err(|error| error.message)?;
            let Some(operator_command) = operator_output.recommended_command else {
                return Err(String::from(
                    "workflow operator public edge did not return a concrete successor command",
                ));
            };
            if is_workflow_operator_command(&operator_command) {
                return Err(format!(
                    "workflow operator public edge looped back to itself instead of surfacing a concrete successor: {operator_command}"
                ));
            }
            let mut routed_status = status.clone();
            routed_status.recommended_command = Some(operator_command.clone());
            routed_status.next_public_action = None;
            routed_status.blockers.clear();
            execute_materialized_public_command(runtime, state, &routed_status, &operator_command)
        }
        ["featureforge", "plan", "execution", args @ ..] => {
            let output = plan_execution_direct_support::try_run_plan_execution_output_direct(
                &runtime.repo_root,
                &runtime.state_dir,
                args,
                &format!("liveness public edge `{materialized}`"),
            )?
            .ok_or_else(|| {
                format!(
                    "liveness public edge was not accepted by the in-process public CLI parser: {materialized}"
                )
            })?;
            if output.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "liveness public edge failed: {materialized}\nstatus_phase_detail={}\nstatus_recommended_command={:?}\nstdout:\n{}\nstderr:\n{}",
                    status.phase_detail,
                    status.recommended_command,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ))
            }
        }
        _ => Err(format!(
            "liveness public edge is not a supported public FeatureForge command: {materialized}"
        )),
    }
}

fn is_workflow_operator_command(command: &str) -> bool {
    shlex::split(command).is_some_and(|parts| {
        matches!(
            parts
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
            ["featureforge", "workflow", "operator", ..]
        )
    })
}

fn materialize_public_command_template(state: &Path, command: &str) -> String {
    let summary = summary_file(state, "liveness-public-edge");
    let summary = summary.to_string_lossy();
    let mut materialized = command
        .replace(
            "[--verification-summary-file <path> when verification ran]",
            "--verification-summary-file <path>",
        )
        .replace("<approved-plan-path>", PLAN_REL)
        .replace("ready|blocked", "ready")
        .replace("pass|fail|not-run", "pass")
        .replace("pass|fail", "pass")
        .replace("<source>", "human-independent-reviewer")
        .replace("<id>", "liveness-model-checker")
        .replace("<path>", &summary);
    if let Some(optional_start) = materialized.find(" [") {
        materialized.truncate(optional_start);
    }
    materialized
}

fn synthetic_liveness_cases(total_tasks: u8) -> Vec<SyntheticState> {
    let mut cases = Vec::new();
    for completed_tasks in 0..=total_tasks {
        for stale_boundary_present in [false, true] {
            for structural_blocker_present in [false, true] {
                for dispatch_lineage_missing in [false, true] {
                    if completed_tasks == 0
                        && (stale_boundary_present
                            || structural_blocker_present
                            || dispatch_lineage_missing)
                    {
                        continue;
                    }
                    if completed_tasks < total_tasks {
                        cases.push(SyntheticState {
                            completed_tasks,
                            stale_boundary_present,
                            structural_blocker_present,
                            late_stage_blocker_present: false,
                            dispatch_lineage_missing,
                        });
                        continue;
                    }
                    for late_stage_blocker_present in [false, true] {
                        cases.push(SyntheticState {
                            completed_tasks,
                            stale_boundary_present,
                            structural_blocker_present,
                            late_stage_blocker_present,
                            dispatch_lineage_missing,
                        });
                    }
                }
            }
        }
    }
    if cases.is_empty() {
        cases.push(SyntheticState {
            completed_tasks: 0,
            stale_boundary_present: false,
            structural_blocker_present: false,
            late_stage_blocker_present: false,
            dispatch_lineage_missing: false,
        });
    }
    cases.retain(|case| {
        if case.late_stage_blocker_present && case.completed_tasks < total_tasks {
            return false;
        }
        if case.completed_tasks == 0 {
            !case.stale_boundary_present
                && !case.structural_blocker_present
                && !case.dispatch_lineage_missing
        } else {
            true
        }
    });
    cases.sort_by_key(|case| {
        (
            case.completed_tasks,
            case.stale_boundary_present,
            case.structural_blocker_present,
            case.dispatch_lineage_missing,
            case.late_stage_blocker_present,
        )
    });
    cases.dedup_by_key(|case| {
        (
            case.completed_tasks,
            case.stale_boundary_present,
            case.structural_blocker_present,
            case.dispatch_lineage_missing,
            case.late_stage_blocker_present,
        )
    });
    cases
}

#[test]
fn synthetic_liveness_generator_covers_full_legal_variant_space() {
    for total_tasks in 1_u8..=5 {
        let cases = synthetic_liveness_cases(total_tasks);
        let expected = 1 + usize::from(total_tasks.saturating_sub(1)) * 8 + 16;
        assert_eq!(
            cases.len(),
            expected,
            "liveness generator must cover every legal stale/structural/dispatch/late-stage variant for {total_tasks} tasks"
        );
        for completed_tasks in 1..total_tasks {
            for stale_boundary_present in [false, true] {
                for structural_blocker_present in [false, true] {
                    for dispatch_lineage_missing in [false, true] {
                        assert!(
                            cases.iter().any(|case| {
                                case.completed_tasks == completed_tasks
                                    && case.stale_boundary_present == stale_boundary_present
                                    && case.structural_blocker_present == structural_blocker_present
                                    && case.dispatch_lineage_missing == dispatch_lineage_missing
                                    && !case.late_stage_blocker_present
                            }),
                            "missing active legal variant for {total_tasks} tasks / {completed_tasks} completed"
                        );
                    }
                }
            }
        }
        for stale_boundary_present in [false, true] {
            for structural_blocker_present in [false, true] {
                for dispatch_lineage_missing in [false, true] {
                    for late_stage_blocker_present in [false, true] {
                        assert!(
                            cases.iter().any(|case| {
                                case.completed_tasks == total_tasks
                                    && case.stale_boundary_present == stale_boundary_present
                                    && case.structural_blocker_present == structural_blocker_present
                                    && case.dispatch_lineage_missing == dispatch_lineage_missing
                                    && case.late_stage_blocker_present == late_stage_blocker_present
                            }),
                            "missing completed legal variant for {total_tasks} tasks"
                        );
                    }
                }
            }
        }
        assert_eq!(
            cases
                .iter()
                .filter(|case| case.completed_tasks == 0)
                .count(),
            1,
            "zero-completion state has only one legal variant because no closure boundary exists yet"
        );
    }
}

#[test]
fn runtime_liveness_model_checker_requires_public_progress_edge() {
    let (repo_dir, state_dir) = workflow_support::init_repo("runtime-liveness-check");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let state_path = harness_state_file_path(repo, state);
    let mut executed_successor_edges = 0usize;

    for total_tasks in 1_u8..=5_u8 {
        let runtime = runtime_for_status(repo, state);
        let mut cases_by_completed = BTreeMap::<u8, Vec<SyntheticState>>::new();
        for synthetic in synthetic_liveness_cases(total_tasks) {
            cases_by_completed
                .entry(synthetic.completed_tasks)
                .or_default()
                .push(synthetic);
        }

        for (completed_tasks, cases) in cases_by_completed {
            write_spec_and_plan(repo, total_tasks, completed_tasks);
            let task_contract_identities = task_contract_identities(repo, state, total_tasks);
            let task_completion_lineages =
                task_completion_lineage_fingerprints(repo, state, total_tasks);
            let context = semantic_execution_context(repo, state);
            let branch_definition_identity =
                featureforge::execution::semantic_identity::branch_definition_identity_for_context(
                    &context,
                );
            let repo_slug = context.runtime.repo_slug.clone();
            let branch_name = context.runtime.branch_name.clone();
            let reviewed_state_id = format!(
                "git_tree:{}",
                featureforge::execution::state::current_tracked_tree_sha(repo)
                    .expect("liveness checker should resolve current tracked tree once per completed-task sweep")
            );
            let fixture = SyntheticFixtureContext {
                reviewed_state_id: &reviewed_state_id,
                state_path: &state_path,
                total_tasks,
                task_contract_identities: &task_contract_identities,
                task_completion_lineages: &task_completion_lineages,
                branch_definition_identity: &branch_definition_identity,
                repo_slug: &repo_slug,
                branch_name: &branch_name,
            };
            for synthetic in cases {
                write_spec_and_plan(repo, total_tasks, completed_tasks);
                write_variant_harness_state(&fixture, synthetic);
                migrate_variant_to_event_authority(
                    &state_path,
                    "liveness synthetic authority migration",
                );

                let label = format!(
                    "runtime-liveness total_tasks={total_tasks} completed_tasks={completed_tasks} synthetic={synthetic:?}",
                );
                let status = status_value(&runtime, &label);
                let state_kind = status.state_kind.as_str();
                let recommended_command = status.recommended_command.as_deref();

                if state_kind == "terminal" {
                    assert!(
                        recommended_command.is_none(),
                        "terminal states must not expose a public command: {status:?}"
                    );
                    continue;
                }

                if state_kind == "waiting_external_input" {
                    assert!(
                        recommended_command.is_none(),
                        "waiting states must not expose a public command: {status:?}"
                    );
                    continue;
                }

                if let Some(command) = recommended_command {
                    assert!(
                        command.starts_with("featureforge "),
                        "recommended command must remain public: {command}"
                    );
                    assert!(
                        !contains_hidden_command_token(command),
                        "recommended command must not route through hidden command lanes: {command}"
                    );
                } else {
                    assert!(
                        status.next_public_action.as_ref().is_some()
                            || status
                                .blockers
                                .iter()
                                .any(|blocker| { blocker.next_public_action.as_ref().is_some() }),
                        "actionable states without recommended_command must surface a concrete public-action lane: {status:?}"
                    );
                }

                let before = progress_metric_from_status(&status, total_tasks);
                if state_kind == "blocked_runtime_bug" {
                    let command =
                        materialize_public_progress_command(&runtime, &status).unwrap_or_else(
                            || {
                                panic!(
                                    "{label} blocked_runtime_bug state must still surface one actionable public recovery command: {status:?}"
                                )
                            },
                        );
                    assert!(
                        command.starts_with("featureforge "),
                        "blocked_runtime_bug recovery command must remain public: {command}"
                    );
                    assert!(
                        !contains_hidden_command_token(&command),
                        "blocked_runtime_bug recovery command must not use hidden lanes: {command}"
                    );
                }
                let successor = execute_public_progress_edge(&runtime, state, &status)
                    .unwrap_or_else(|error| {
                        panic!("{label} should execute public progress edge: {error}")
                    });
                let successor = successor.unwrap_or_else(|| {
                    panic!("{label} should execute a concrete public progress edge from {status:?}")
                });
                let after = progress_metric_from_status(&successor, total_tasks);
                assert!(
                    after < before,
                    "real public progress edge must monotonically reduce runtime-derived metric; before={before:?} after={after:?} status={status:?} successor={successor:?}"
                );
                executed_successor_edges = executed_successor_edges.saturating_add(1);
            }
        }
    }
    assert!(
        executed_successor_edges >= 8,
        "liveness checker must execute a meaningful set of real public successor edges, got {executed_successor_edges}",
    );
}

#[test]
fn runtime_liveness_model_checker_never_emits_hidden_recommendations() {
    let (repo_dir, state_dir) = workflow_support::init_repo("runtime-liveness-hidden-check");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let state_path = harness_state_file_path(repo, state);
    let mut hidden_hits = BTreeMap::<String, String>::new();
    for total_tasks in 1_u8..=5_u8 {
        write_spec_and_plan(repo, total_tasks, total_tasks.saturating_sub(1));
        let task_contract_identities = task_contract_identities(repo, state, total_tasks);
        let task_completion_lineages =
            task_completion_lineage_fingerprints(repo, state, total_tasks);
        let context = semantic_execution_context(repo, state);
        let branch_definition_identity =
            featureforge::execution::semantic_identity::branch_definition_identity_for_context(
                &context,
            );
        let repo_slug = context.runtime.repo_slug.clone();
        let branch_name = context.runtime.branch_name.clone();
        let reviewed_state_id = format!(
            "git_tree:{}",
            featureforge::execution::state::current_tracked_tree_sha(repo)
                .expect("hidden-command liveness checker should resolve current tracked tree once per task sweep")
        );
        let runtime = runtime_for_status(repo, state);
        write_variant_harness_state(
            &SyntheticFixtureContext {
                reviewed_state_id: &reviewed_state_id,
                state_path: &state_path,
                total_tasks,
                task_contract_identities: &task_contract_identities,
                task_completion_lineages: &task_completion_lineages,
                branch_definition_identity: &branch_definition_identity,
                repo_slug: &repo_slug,
                branch_name: &branch_name,
            },
            SyntheticState {
                completed_tasks: 0,
                stale_boundary_present: false,
                structural_blocker_present: false,
                late_stage_blocker_present: false,
                dispatch_lineage_missing: false,
            },
        );
        migrate_variant_to_event_authority(
            &state_path,
            "liveness hidden-check authority migration",
        );

        let status = status_value(
            &runtime,
            &format!("runtime-hidden-check total_tasks={total_tasks}"),
        );
        if let Some(command) = status.recommended_command.as_deref()
            && contains_hidden_command_token(command)
        {
            hidden_hits.insert(total_tasks.to_string(), command.to_owned());
        }
    }

    assert!(
        hidden_hits.is_empty(),
        "runtime liveness checks must not emit hidden command recommendations: {hidden_hits:?}"
    );
}
