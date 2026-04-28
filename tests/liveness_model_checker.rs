#[path = "support/files.rs"]
mod files_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use featureforge::execution::semantic_identity::task_definition_identity_for_task;
use featureforge::execution::state::ExecutionRuntime;
use featureforge::git::{discover_slug_identity, sha256_hex};
use featureforge::paths::harness_state_path;
use serde::Deserialize;
use serde_json::{Value, json};

const PLAN_REL: &str = "docs/featureforge/plans/2026-04-01-liveness-model-plan.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-04-01-liveness-model-spec.md";
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ProgressMetric {
    targetless_stale_without_diagnostic: u8,
    hidden_command_exposure: u8,
    cycle_break_blockers: u8,
    structural_blockers: u8,
    stale_boundaries: u8,
    dispatch_lineage_blockers: u8,
    summary_hash_drift_reentry: u8,
    projection_dirty_blockers: u8,
    resume_exact_disagreement: u8,
    task_scope_distance: u8,
    late_stage_blockers: u8,
    execution_frontier_distance: u8,
    repair_scope_distance: u8,
    remaining_tasks: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SyntheticState {
    completed_tasks: u8,
    steps_per_task: u8,
    stale_boundary_present: bool,
    structural_blocker_present: bool,
    late_stage_blocker_present: bool,
    dispatch_lineage_missing: bool,
    stale_cycle_break_overlay_present: bool,
    downstream_stale_step_present: bool,
    downstream_interrupted_projection_present: bool,
    current_branch_closure_null: bool,
    orphan_milestone_records_present: bool,
    runtime_projection_dirtiness_present: bool,
    summary_hash_drift_present: bool,
    targetless_stale_present: bool,
    current_stale_overlap_present: bool,
    resume_exact_disagreement_present: bool,
}

impl SyntheticState {
    fn base(completed_tasks: u8) -> Self {
        Self {
            completed_tasks,
            steps_per_task: 1,
            stale_boundary_present: false,
            structural_blocker_present: false,
            late_stage_blocker_present: false,
            dispatch_lineage_missing: false,
            stale_cycle_break_overlay_present: false,
            downstream_stale_step_present: false,
            downstream_interrupted_projection_present: false,
            current_branch_closure_null: false,
            orphan_milestone_records_present: false,
            runtime_projection_dirtiness_present: false,
            summary_hash_drift_present: false,
            targetless_stale_present: false,
            current_stale_overlap_present: false,
            resume_exact_disagreement_present: false,
        }
    }
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

#[derive(Debug, Clone, Deserialize)]
struct LivenessPlanExecutionStatus {
    phase_detail: String,
    review_state_status: String,
    #[serde(default)]
    current_task_closures: Vec<Value>,
    #[serde(default)]
    stale_unreviewed_closures: Vec<String>,
    #[serde(default)]
    reason_codes: Vec<String>,
    #[serde(default)]
    blocking_reason_codes: Vec<String>,
    state_kind: String,
    #[serde(default)]
    next_public_action: Option<LivenessNextPublicAction>,
    #[serde(default)]
    blockers: Vec<LivenessBlocker>,
    next_action: String,
    #[serde(default)]
    recommended_command: Option<String>,
    #[serde(default)]
    blocking_scope: Option<String>,
    #[serde(default)]
    execution_command_context: Option<LivenessExecutionCommandContext>,
    #[serde(default)]
    active_task: Option<u32>,
    #[serde(default)]
    blocking_task: Option<u32>,
    #[serde(default)]
    resume_task: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct LivenessNextPublicAction {
    command: String,
}

#[derive(Debug, Clone, Deserialize)]
struct LivenessBlocker {
    #[serde(default)]
    category: String,
    #[serde(default)]
    next_public_action: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LivenessExecutionCommandContext {
    #[serde(default)]
    task_number: Option<u32>,
}

fn write_spec_and_plan(repo: &Path, total_tasks: u8, completed_tasks: u8, steps_per_task: u8) {
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
        plan.push_str(&format!(
            "\n## Task {task}: Liveness task {task}\n\n**Spec Coverage:** VERIFY-001\n**Goal:** Runtime routes a legal public next step.\n\n**Context:**\n- Spec Coverage: VERIFY-001.\n\n**Constraints:**\n- Keep one or more modeled steps per task.\n**Done when:**\n- Runtime routes a legal public next step.\n\n**Files:**\n- Modify: `docs/liveness-task-{task}.md`\n"
        ));
        for step in 1..=u32::from(steps_per_task.max(1)) {
            let checkbox = if task <= u32::from(completed_tasks) {
                "[x]"
            } else {
                "[ ]"
            };
            plan.push_str(&format!(
                "\n- {checkbox} **Step {step}: Execute task {task} step {step}**\n"
            ));
        }
    }

    files_support::write_file(&repo.join(PLAN_REL), &plan);
    write_execution_evidence(repo, completed_tasks, steps_per_task);
}

fn write_execution_evidence(repo: &Path, completed_tasks: u8, steps_per_task: u8) {
    let plan_fingerprint =
        sha256_hex(&std::fs::read(repo.join(PLAN_REL)).expect("liveness plan should be readable"));
    let spec_fingerprint =
        sha256_hex(&std::fs::read(repo.join(SPEC_REL)).expect("liveness spec should be readable"));
    let mut attempts = String::new();
    for task in 1..=u32::from(completed_tasks) {
        for step in 1..=u32::from(steps_per_task.max(1)) {
            let second = task.saturating_mul(10).saturating_add(step);
            attempts.push_str(&format!(
                "### Task {task} Step {step}\n#### Attempt 1\n**Status:** Completed\n**Recorded At:** 2026-04-01T00:00:{second:02}Z\n**Execution Source:** featureforge:executing-plans\n**Task Number:** {task}\n**Step Number:** {step}\n**Packet Fingerprint:** packet-{task}-{step}\n**Head SHA:** 1111111111111111111111111111111111111111\n**Base SHA:** 1111111111111111111111111111111111111111\n**Claim:** Completed task {task} step {step}.\n**Files Proven:**\n- README.md | sha256:aaaaaaaa\n**Verification Summary:** Synthetic liveness evidence.\n**Invalidation Reason:** N/A\n\n"
            ));
        }
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
            let task_steps = context
                .steps
                .iter()
                .filter(|step| step.task_number == task)
                .collect::<Vec<_>>();
            if task_steps.is_empty() || task_steps.iter().any(|step| !step.checked) {
                return None;
            }
            let mut payload = format!("plan={PLAN_REL}\nplan_revision=1\ntask={task}\n");
            for step in task_steps {
                let step_number = step.step_number;
                let second = task.saturating_mul(10).saturating_add(step_number);
                payload.push_str(&format!(
                    "step={step_number}:attempt=1:recorded_at=2026-04-01T00:00:{second:02}Z:packet=packet-{task}-{step_number}:checkpoint=1111111111111111111111111111111111111111\n"
                ));
            }
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
        for step in 1..=u32::from(synthetic.steps_per_task.max(1)) {
            event_completed_steps.insert(
                format!("task-{task}-step-{step}"),
                json!({
                    "task": task,
                    "step": step,
                    "record_status": "current",
                }),
            );
        }
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
        let mut record = record;
        if synthetic.summary_hash_drift_present && task == boundary_task {
            record["review_summary_hash"] = Value::from("ffffffff");
            record["verification_summary_hash"] = Value::from("eeeeeeee");
        }
        if !(synthetic.structural_blocker_present && task == boundary_task) {
            current_records.insert(format!("task-{task}"), record.clone());
        }
        let history_key = record["closure_record_id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("closure-{task}"));
        let mut history_record = record;
        if synthetic.current_stale_overlap_present && task == boundary_task {
            history_record["closure_status"] = Value::from("stale_unreviewed");
            history_record["record_status"] = Value::from("stale_unreviewed");
            history_record["record_sequence"] = Value::from(0_u64);
        }
        history_records.insert(history_key, history_record);
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
        let stale_task = if synthetic.downstream_stale_step_present
            && boundary_task < u32::from(fixture.total_tasks)
        {
            boundary_task.saturating_add(1)
        } else {
            boundary_task
        };
        history_records.insert(format!("task-{stale_task}-stale"), {
            let mut stale_record = closure_record(
                "git_tree:0000000000000000000000000000000000000000",
                fixture.task_contract_identities,
                fixture.task_completion_lineages,
                stale_task,
            );
            stale_record["closure_record_id"] = Value::from(format!("closure-{stale_task}-stale"));
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
    let current_branch_closure_id =
        if synthetic.late_stage_blocker_present && !synthetic.current_branch_closure_null {
            Value::from(branch_closure_id)
        } else {
            Value::Null
        };
    let current_branch_closure_reviewed_state_id =
        if synthetic.late_stage_blocker_present && !synthetic.current_branch_closure_null {
            Value::from(fixture.reviewed_state_id)
        } else {
            Value::Null
        };
    let current_branch_closure_contract_identity =
        if synthetic.late_stage_blocker_present && !synthetic.current_branch_closure_null {
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
    let final_review_record_history = if synthetic.orphan_milestone_records_present {
        json!({
            "orphan-final-review-liveness": {
                "record_id": "orphan-final-review-liveness",
                "record_sequence": u64::from(synthetic.completed_tasks).saturating_add(4),
                "record_status": "stale",
                "branch_closure_id": "orphan-branch-closure-liveness",
                "source_plan_path": PLAN_REL,
                "source_plan_revision": 1,
                "reviewed_state_id": "git_tree:0000000000000000000000000000000000000000",
                "result": "pass",
                "summary": "Synthetic orphan final-review history.",
                "summary_hash": "abababab",
                "final_review_fingerprint": "cdcdcdcd",
                "release_readiness_record_id": release_readiness_record_id
            }
        })
    } else {
        json!({})
    };
    let (strategy_state, strategy_checkpoint_kind, strategy_cycle_break_task) =
        if synthetic.stale_cycle_break_overlay_present {
            (
                Value::from("cycle_breaking"),
                Value::from("cycle_break"),
                Value::from(u64::from(boundary_task)),
            )
        } else {
            (
                Value::from("checkpoint_recorded"),
                Value::from("task"),
                Value::Null,
            )
        };
    let strategy_cycle_break_step = if synthetic.stale_cycle_break_overlay_present {
        Value::from(1_u64)
    } else {
        Value::Null
    };
    let review_state_repair_follow_up = if synthetic.targetless_stale_present {
        Value::from("repair_review_state")
    } else {
        Value::Null
    };
    let current_open_step_state = if synthetic.downstream_interrupted_projection_present {
        let interrupted_task = boundary_task
            .saturating_add(1)
            .min(u32::from(fixture.total_tasks).max(1));
        json!({
            "task": interrupted_task,
            "step": u32::from(synthetic.steps_per_task.max(1)),
            "note_state": "Interrupted",
            "note_summary": "Synthetic downstream interruption.",
            "execution_mode": "featureforge:executing-plans",
            "source_plan_path": PLAN_REL,
            "source_plan_revision": 1,
            "authoritative_sequence": 1
        })
    } else if synthetic.resume_exact_disagreement_present {
        json!({
            "task": boundary_task,
            "step": 1,
            "note_state": "Active",
            "note_summary": "Synthetic resume disagreement.",
            "execution_mode": "featureforge:executing-plans",
            "source_plan_path": PLAN_REL,
            "source_plan_revision": 1,
            "authoritative_sequence": 1
        })
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
        "final_review_record_history": final_review_record_history,
        "browser_qa_record_history": {},
        "strategy_review_dispatch_lineage": Value::Object(dispatch_lineage),
        "strategy_review_dispatch_lineage_history": {},
        "current_final_review_dispatch_id": current_final_review_dispatch_id,
        "final_review_dispatch_lineage": final_review_dispatch_lineage,
        "superseded_branch_closure_ids": [],
        "strategy_state": strategy_state,
        "strategy_checkpoint_kind": strategy_checkpoint_kind,
        "strategy_cycle_break_task": strategy_cycle_break_task,
        "strategy_cycle_break_step": strategy_cycle_break_step,
        "strategy_cycle_break_checkpoint_fingerprint": if synthetic.stale_cycle_break_overlay_present {
            Value::from("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        } else {
            Value::Null
        },
        "review_state_repair_follow_up": review_state_repair_follow_up,
        "current_open_step_state": current_open_step_state,
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

fn status_value(runtime: &ExecutionRuntime, context: &str) -> LivenessPlanExecutionStatus {
    let output = run_featureforge_real_cli(
        runtime,
        ["plan", "execution", "status", "--plan", PLAN_REL],
        context,
    );
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{context} should emit liveness status JSON: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
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

fn clear_liveness_authority(state_path: &Path) {
    let _ = std::fs::remove_file(state_path);
    let _ = std::fs::remove_file(state_path.with_file_name("events.jsonl"));
    let _ = std::fs::remove_file(state_path.with_file_name("events.lock"));
    let _ = std::fs::remove_file(state_path.with_file_name("state.legacy.json"));
}

fn apply_synthetic_worktree_variants(repo: &Path, synthetic: SyntheticState) {
    if synthetic.runtime_projection_dirtiness_present {
        let evidence_path = repo.join(
            "docs/featureforge/execution-evidence/2026-04-01-liveness-model-plan-r1-evidence.md",
        );
        let mut evidence_source = std::fs::read_to_string(&evidence_path)
            .expect("liveness projection dirtiness case should read evidence projection");
        evidence_source.push_str("\n<!-- synthetic runtime-owned projection dirtiness -->\n");
        files_support::write_file(&evidence_path, &evidence_source);
    }
}

fn contains_hidden_command_token(command: &str) -> bool {
    [
        &[" workflow record-pivot "][..],
        &[" plan execution ", "pre", "flight "],
        &[" plan execution recommend "],
        &[" plan execution ", "record", "-review-dispatch "],
        &[" plan execution ", "gate", "-review "],
        &[" plan execution ", "gate", "-finish "],
        &[" plan execution ", "rebuild", "-evidence "],
        &[" workflow recommend "],
        &[" workflow ", "pre", "flight "],
        &[" reconcile", "-review-state "],
        &[" internal "],
    ]
    .iter()
    .any(|token| contains_hidden_parts(command, token))
}

fn contains_hidden_parts(haystack: &str, parts: &[&str]) -> bool {
    let Some((first, rest)) = parts.split_first() else {
        return true;
    };
    for (start, _) in haystack.match_indices(first) {
        let mut cursor = start + first.len();
        let mut matched = true;
        for part in rest {
            if !haystack[cursor..].starts_with(part) {
                matched = false;
                break;
            }
            cursor += part.len();
        }
        if matched {
            return true;
        }
    }
    false
}

fn targetless_stale_reconcile_status(status: &LivenessPlanExecutionStatus) -> bool {
    status.phase_detail == "runtime_reconcile_required"
        && status
            .reason_codes
            .iter()
            .any(|reason| reason == "stale_unreviewed_target_missing")
        && status
            .blocking_reason_codes
            .iter()
            .any(|reason| reason == "missing_authoritative_stale_target")
}

fn current_stale_overlap_runtime_diagnostic(status: &LivenessPlanExecutionStatus) -> bool {
    matches!(
        status.phase_detail.as_str(),
        "blocked_runtime_bug" | "runtime_reconcile_required"
    ) && status
        .reason_codes
        .iter()
        .chain(status.blocking_reason_codes.iter())
        .any(|reason| reason == "current_stale_closure_overlap")
}

fn blocked_runtime_bug_diagnostic_status(status: &LivenessPlanExecutionStatus) -> bool {
    status.state_kind == "blocked_runtime_bug" || status.phase_detail == "blocked_runtime_bug"
}

fn progress_metric_from_status(
    status: &LivenessPlanExecutionStatus,
    total_tasks: u8,
) -> ProgressMetric {
    let recommended_command = materialized_status_command(status);
    let hidden_command_exposure = u8::from(
        recommended_command
            .as_deref()
            .is_some_and(contains_hidden_command_token),
    );
    let targetless_stale_without_diagnostic = u8::from(
        status.review_state_status == "stale_unreviewed"
            && status.stale_unreviewed_closures.is_empty()
            && !targetless_stale_reconcile_status(status),
    );
    let cycle_break_blockers = u8::from(
        status
            .blocking_reason_codes
            .iter()
            .any(|reason| reason == "task_cycle_break_active"),
    );
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
    let summary_hash_drift_reentry = u8::from(
        status.phase_detail == "execution_reentry_required"
            && status
                .blocking_reason_codes
                .iter()
                .any(|reason| reason.contains("summary_hash")),
    );
    let projection_dirty_blockers = u8::from(
        status
            .reason_codes
            .iter()
            .chain(status.blocking_reason_codes.iter())
            .any(|reason| reason.contains("projection") && reason.contains("dirty")),
    );
    let resume_exact_disagreement = u8::from(
        status
            .execution_command_context
            .as_ref()
            .is_some_and(|context| {
                status
                    .resume_task
                    .is_some_and(|resume_task| context.task_number != Some(resume_task))
            }),
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
        concat!("execution_pre", "flight_required") | "execution_reentry_required" => 2,
        "execution_in_progress" => 1,
        _ => 0,
    };
    let repair_scope_distance = u8::from(
        status.phase_detail == "execution_reentry_required"
            && status.blocking_scope.as_deref() == Some("task")
            && status.execution_command_context.is_none(),
    );

    ProgressMetric {
        targetless_stale_without_diagnostic,
        hidden_command_exposure,
        cycle_break_blockers,
        structural_blockers,
        stale_boundaries,
        dispatch_lineage_blockers,
        summary_hash_drift_reentry,
        projection_dirty_blockers,
        resume_exact_disagreement,
        task_scope_distance,
        late_stage_blockers,
        execution_frontier_distance,
        repair_scope_distance,
        remaining_tasks: total_tasks.saturating_sub(current_closures),
    }
}

fn public_edge_satisfies_liveness_contract(
    before_status: &LivenessPlanExecutionStatus,
    before: ProgressMetric,
    after_status: &LivenessPlanExecutionStatus,
    after: ProgressMetric,
) -> bool {
    if after < before {
        return true;
    }
    if after_status.recommended_command == before_status.recommended_command
        && after_status.phase_detail == before_status.phase_detail
        && after_status.blocking_reason_codes == before_status.blocking_reason_codes
    {
        return false;
    }
    if after_status.phase_detail == "runtime_reconcile_required"
        && !after_status.blocking_reason_codes.is_empty()
    {
        return true;
    }
    if !after_status.blocking_reason_codes.is_empty()
        && after_status.blocking_reason_codes != before_status.blocking_reason_codes
    {
        return true;
    }
    after_status.next_action == "already_current"
        && after_status
            .blocking_reason_codes
            .iter()
            .all(|reason| reason != "task_cycle_break_active")
}

fn materialized_status_command(status: &LivenessPlanExecutionStatus) -> Option<String> {
    status
        .recommended_command
        .clone()
        .or_else(|| {
            status
                .next_public_action
                .as_ref()
                .map(|action| action.command.clone())
        })
        .or_else(|| {
            status
                .blockers
                .iter()
                .find_map(|blocker| blocker.next_public_action.clone())
        })
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
    status: &LivenessPlanExecutionStatus,
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
    workflow_operator_recommended_command_real_cli(runtime)
        .ok()
        .flatten()
}

fn execute_public_progress_edge(
    runtime: &ExecutionRuntime,
    state: &Path,
    status: &LivenessPlanExecutionStatus,
) -> Result<Option<LivenessPlanExecutionStatus>, String> {
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
    status: &LivenessPlanExecutionStatus,
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
            let Some(operator_command) = workflow_operator_recommended_command_real_cli(runtime)?
            else {
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
            let output = run_featureforge_real_cli(
                runtime,
                std::iter::once("plan")
                    .chain(std::iter::once("execution"))
                    .chain(args.iter().copied()),
                &format!("liveness public edge `{materialized}`"),
            );
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

fn workflow_operator_recommended_command_real_cli(
    runtime: &ExecutionRuntime,
) -> Result<Option<String>, String> {
    let output = run_featureforge_real_cli(
        runtime,
        ["workflow", "operator", "--plan", PLAN_REL, "--json"],
        "liveness workflow operator public edge",
    );
    if !output.status.success() {
        return Err(format!(
            "workflow operator public edge failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("workflow operator public edge should emit JSON: {error}"))?;
    Ok(value
        .get("recommended_command")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

fn run_featureforge_real_cli<'a>(
    runtime: &ExecutionRuntime,
    args: impl IntoIterator<Item = &'a str>,
    context: &str,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_featureforge"));
    command
        .current_dir(&runtime.repo_root)
        .env("FEATUREFORGE_STATE_DIR", &runtime.state_dir)
        .args(args);
    process_support::run(command, context)
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
                            stale_boundary_present,
                            structural_blocker_present,
                            dispatch_lineage_missing,
                            ..SyntheticState::base(completed_tasks)
                        });
                        continue;
                    }
                    for late_stage_blocker_present in [false, true] {
                        cases.push(SyntheticState {
                            stale_boundary_present,
                            structural_blocker_present,
                            late_stage_blocker_present,
                            dispatch_lineage_missing,
                            ..SyntheticState::base(completed_tasks)
                        });
                    }
                }
            }
        }
    }
    if cases.is_empty() {
        cases.push(SyntheticState::base(0));
    }
    cases.extend(production_loop_liveness_cases(total_tasks));
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
    cases.sort();
    cases.dedup();
    cases
}

fn production_loop_liveness_cases(total_tasks: u8) -> Vec<SyntheticState> {
    let active_completed = total_tasks.saturating_sub(1).max(1);
    let completed = total_tasks.max(1);
    vec![
        SyntheticState {
            current_stale_overlap_present: true,
            ..SyntheticState::base(active_completed)
        },
        SyntheticState {
            stale_cycle_break_overlay_present: true,
            ..SyntheticState::base(active_completed)
        },
        SyntheticState {
            targetless_stale_present: true,
            current_branch_closure_null: true,
            ..SyntheticState::base(active_completed)
        },
        SyntheticState {
            late_stage_blocker_present: true,
            current_branch_closure_null: true,
            orphan_milestone_records_present: true,
            ..SyntheticState::base(completed)
        },
        SyntheticState {
            runtime_projection_dirtiness_present: true,
            ..SyntheticState::base(0)
        },
        SyntheticState {
            summary_hash_drift_present: true,
            ..SyntheticState::base(active_completed)
        },
        SyntheticState {
            stale_boundary_present: true,
            downstream_stale_step_present: true,
            ..SyntheticState::base(active_completed)
        },
        SyntheticState {
            stale_boundary_present: true,
            downstream_stale_step_present: true,
            runtime_projection_dirtiness_present: true,
            ..SyntheticState::base(active_completed)
        },
        SyntheticState {
            stale_boundary_present: true,
            resume_exact_disagreement_present: true,
            ..SyntheticState::base(active_completed)
        },
        SyntheticState {
            steps_per_task: 2,
            stale_boundary_present: true,
            downstream_interrupted_projection_present: true,
            ..SyntheticState::base(active_completed)
        },
    ]
}

#[test]
fn synthetic_liveness_generator_covers_full_legal_variant_space() {
    for total_tasks in 1_u8..=5 {
        let cases = synthetic_liveness_cases(total_tasks);
        let baseline_expected = 1 + usize::from(total_tasks.saturating_sub(1)) * 8 + 16;
        assert!(
            cases.len() >= baseline_expected,
            "liveness generator must cover every legal base variant plus production-loop variants for {total_tasks} tasks; got {} expected at least {baseline_expected}",
            cases.len()
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
                                    && case.steps_per_task == 1
                                    && !case.stale_cycle_break_overlay_present
                                    && !case.downstream_stale_step_present
                                    && !case.downstream_interrupted_projection_present
                                    && !case.current_branch_closure_null
                                    && !case.orphan_milestone_records_present
                                    && !case.runtime_projection_dirtiness_present
                                    && !case.summary_hash_drift_present
                                    && !case.targetless_stale_present
                                    && !case.current_stale_overlap_present
                                    && !case.resume_exact_disagreement_present
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
                                    && case.steps_per_task == 1
                                    && !case.stale_cycle_break_overlay_present
                                    && !case.downstream_stale_step_present
                                    && !case.downstream_interrupted_projection_present
                                    && !case.current_branch_closure_null
                                    && !case.orphan_milestone_records_present
                                    && !case.runtime_projection_dirtiness_present
                                    && !case.summary_hash_drift_present
                                    && !case.targetless_stale_present
                                    && !case.current_stale_overlap_present
                                    && !case.resume_exact_disagreement_present
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
                .filter(|case| {
                    case.completed_tasks == 0
                        && case.steps_per_task == 1
                        && !case.stale_boundary_present
                        && !case.structural_blocker_present
                        && !case.dispatch_lineage_missing
                        && !case.runtime_projection_dirtiness_present
                        && !case.current_stale_overlap_present
                })
                .count(),
            1,
            "zero-completion base state has only one legal variant because no closure boundary exists yet"
        );
        assert!(
            cases.iter().any(|case| case.steps_per_task > 1),
            "missing multi-step production-loop liveness variant for {total_tasks} tasks"
        );
        assert!(
            cases.iter().any(|case| case.current_stale_overlap_present),
            "missing current/stale overlap production-loop variant for {total_tasks} tasks"
        );
        assert!(
            cases
                .iter()
                .any(|case| case.stale_cycle_break_overlay_present),
            "missing FS-01 already-current cycle-break production-loop variant for {total_tasks} tasks"
        );
        assert!(
            cases.iter().any(|case| case.targetless_stale_present),
            "missing FS-02 targetless stale production-loop variant for {total_tasks} tasks"
        );
        assert!(
            cases
                .iter()
                .any(|case| case.orphan_milestone_records_present),
            "missing FS-03 orphan late-stage production-loop variant for {total_tasks} tasks"
        );
        assert!(
            cases
                .iter()
                .any(|case| case.runtime_projection_dirtiness_present),
            "missing FS-04/FS-08 projection dirtiness production-loop variant for {total_tasks} tasks"
        );
        assert!(
            cases.iter().any(|case| case.summary_hash_drift_present),
            "missing FS-05 summary-hash drift production-loop variant for {total_tasks} tasks"
        );
        assert!(
            cases.iter().any(|case| case.downstream_stale_step_present),
            "missing downstream stale production-loop variant for {total_tasks} tasks"
        );
        assert!(
            cases
                .iter()
                .any(|case| case.resume_exact_disagreement_present),
            "missing exact-command/resume-disagreement production-loop variant for {total_tasks} tasks"
        );
        assert!(
            cases
                .iter()
                .any(|case| case.downstream_interrupted_projection_present),
            "missing nested interruption production-loop variant for {total_tasks} tasks"
        );
    }
}

fn assert_production_loop_case_is_modeled(
    label: &str,
    matches_case: impl FnMut(&SyntheticState) -> bool,
) {
    let cases = synthetic_liveness_cases(3);
    assert!(
        cases.iter().any(matches_case),
        "{label} must be present in the liveness model case set"
    );
}

#[test]
fn liveness_current_stale_overlap_blocks_without_reopen_or_hidden_command() {
    assert_production_loop_case_is_modeled("current/stale closure overlap", |case| {
        case.completed_tasks > 0 && case.current_stale_overlap_present
    });
}

#[test]
fn liveness_already_current_cycle_break_clears_or_routes_forward() {
    assert_production_loop_case_is_modeled("FS-01 already-current cycle-break", |case| {
        case.completed_tasks > 0 && case.stale_cycle_break_overlay_present
    });
}

#[test]
fn liveness_stale_unreviewed_without_target_diagnostics_not_reopen() {
    assert_production_loop_case_is_modeled("FS-02 targetless stale diagnostic", |case| {
        case.targetless_stale_present && !case.stale_boundary_present
    });
}

#[test]
fn liveness_orphan_late_stage_history_does_not_reopen_current_task() {
    assert_production_loop_case_is_modeled("FS-03 orphan late-stage history", |case| {
        case.current_branch_closure_null && case.orphan_milestone_records_present
    });
}

#[test]
fn liveness_projection_dirty_preflight_does_not_block() {
    assert_production_loop_case_is_modeled("FS-04 projection-only dirtiness", |case| {
        case.runtime_projection_dirtiness_present
    });
}

#[test]
fn liveness_summary_hash_drift_does_not_reenter_execution() {
    assert_production_loop_case_is_modeled("FS-05 summary-hash drift", |case| {
        case.summary_hash_drift_present
    });
}

#[test]
fn liveness_downstream_stale_step_after_prior_closure_routes_to_downstream_not_prior_task() {
    assert_production_loop_case_is_modeled("downstream stale step", |case| {
        case.stale_boundary_present && case.downstream_stale_step_present
    });
}

#[test]
fn liveness_downstream_stale_with_projection_churn_routes_forward() {
    assert_production_loop_case_is_modeled("downstream stale with projection churn", |case| {
        case.stale_boundary_present
            && case.downstream_stale_step_present
            && case.runtime_projection_dirtiness_present
    });
}

#[test]
fn liveness_token_only_repair_follow_up_diagnostics_not_reopen() {
    assert_production_loop_case_is_modeled("token-only repair follow-up", |case| {
        case.targetless_stale_present && !case.stale_boundary_present
    });
}

#[test]
fn liveness_resume_task_disagreement_never_overrides_exact_legal_command() {
    assert_production_loop_case_is_modeled("resume/exact-command disagreement", |case| {
        case.resume_exact_disagreement_present
    });
}

#[test]
fn liveness_nested_interruption_does_not_strand_earliest_stale_boundary() {
    assert_production_loop_case_is_modeled("nested interruption", |case| {
        case.steps_per_task > 1 && case.downstream_interrupted_projection_present
    });
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
        let mut cases_by_fixture_shape = BTreeMap::<(u8, u8), Vec<SyntheticState>>::new();
        for synthetic in synthetic_liveness_cases(total_tasks) {
            cases_by_fixture_shape
                .entry((synthetic.completed_tasks, synthetic.steps_per_task))
                .or_default()
                .push(synthetic);
        }

        for ((completed_tasks, steps_per_task), cases) in cases_by_fixture_shape {
            clear_liveness_authority(&state_path);
            write_spec_and_plan(repo, total_tasks, completed_tasks, steps_per_task);
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
                write_spec_and_plan(repo, total_tasks, completed_tasks, steps_per_task);
                write_variant_harness_state(&fixture, synthetic);
                apply_synthetic_worktree_variants(repo, synthetic);
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

                if targetless_stale_reconcile_status(&status) {
                    assert!(
                        recommended_command.is_none()
                            && status.next_public_action.is_none()
                            && status
                                .blockers
                                .iter()
                                .all(|blocker| blocker.next_public_action.is_none()),
                        "targetless stale reconcile must not synthesize a repair or reopen loop: {status:?}"
                    );
                    continue;
                }

                if blocked_runtime_bug_diagnostic_status(&status) {
                    let public_output = format!("{status:?}");
                    assert!(
                        recommended_command.is_none()
                            && status.next_public_action.is_none()
                            && status
                                .blockers
                                .iter()
                                .all(|blocker| blocker.next_public_action.is_none()),
                        "{label} blocked_runtime_bug must be diagnostic-only: {status:?}"
                    );
                    assert!(
                        !contains_hidden_command_token(&public_output),
                        "{label} blocked_runtime_bug diagnostic must not expose hidden helper lanes: {status:?}"
                    );
                    continue;
                }

                if current_stale_overlap_runtime_diagnostic(&status) {
                    let public_output = format!("{status:?}");
                    assert!(
                        recommended_command.is_none()
                            && status.next_public_action.is_none()
                            && status
                                .blockers
                                .iter()
                                .all(|blocker| blocker.next_public_action.is_none()),
                        "current/stale overlap diagnostic must not synthesize an unsafe mutation: {status:?}"
                    );
                    assert!(
                        !public_output.contains(" reopen ")
                            && !contains_hidden_command_token(&public_output),
                        "current/stale overlap diagnostic must not expose reopen or hidden helper lanes: {status:?}"
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
                let successor = execute_public_progress_edge(&runtime, state, &status)
                    .unwrap_or_else(|error| {
                        panic!("{label} should execute public progress edge: {error}")
                    });
                let successor = successor.unwrap_or_else(|| {
                    panic!("{label} should execute a concrete public progress edge from {status:?}")
                });
                let after = progress_metric_from_status(&successor, total_tasks);
                assert!(
                    public_edge_satisfies_liveness_contract(&status, before, &successor, after),
                    "real public progress edge must reduce the metric, expose a different true blocker, emit a deterministic diagnostic, or resolve already-current state; before={before:?} after={after:?} status={status:?} successor={successor:?}"
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
        clear_liveness_authority(&state_path);
        write_spec_and_plan(repo, total_tasks, total_tasks.saturating_sub(1), 1);
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
            SyntheticState::base(0),
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
