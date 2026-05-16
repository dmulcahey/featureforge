#[path = "support/files.rs"]
mod files_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/public_featureforge_cli.rs"]
mod public_featureforge_cli;
#[path = "support/internal_public_runtime_in_process.rs"]
mod semantic_public_argv_runtime;
#[path = "support/workflow.rs"]
mod workflow_support;

// This suite is an internal semantic/liveness model checker. It uses in-process
// helpers to keep the state matrix cheap, then samples a shipped compiled-CLI
// parity edge so the public argv runner does not drift from the real boundary.
// Do not cite this file as shipped-runtime public-flow proof; that proof lives
// in the compiled-CLI public runtime flow gate and public route goldens.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use featureforge::execution::semantic_identity::task_definition_identity_for_task;
use featureforge::execution::state::ExecutionRuntime;
use featureforge::git::{discover_slug_identity, sha256_hex};
use featureforge::paths::harness_state_path;
use serde::Deserialize;
use serde_json::{Value, json};

const PLAN_REL: &str = "docs/featureforge/plans/2026-04-01-liveness-model-plan.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-04-01-liveness-model-spec.md";
// The generator tests cover every legal task-count shape. This executed edge
// test uses one multi-task shape to keep the expensive runtime mutation matrix
// representative without making the full suite depend on duplicate one-task
// executions.
const RUNTIME_EXECUTED_LIVENESS_TASK_COUNTS: &[u8] = &[3];
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
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

type SyntheticCasePredicate = fn(&SyntheticState) -> bool;
type RequiredSyntheticCase<'a> = (&'a str, SyntheticCasePredicate);

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

type LivenessRecommendedPublicCommandArgv = Option<Vec<String>>;

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
    #[serde(default)]
    public_repair_targets: Vec<Value>,
    next_action: String,
    #[serde(default)]
    recommended_command: Option<String>,
    #[serde(default)]
    recommended_public_command_argv: LivenessRecommendedPublicCommandArgv,
    #[serde(default)]
    required_follow_up: Option<String>,
    #[serde(default)]
    required_inputs: Vec<Value>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivenessSemanticPublicCommandKey {
    command_kind: LivenessPublicCommandKind,
    task: Option<u32>,
    step: Option<u32>,
    mode: Option<String>,
    transfer_mode: Option<String>,
    transfer_scope: Option<String>,
    source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivenessRouteSignature {
    phase_detail: String,
    review_state_status: String,
    reason_codes: Vec<String>,
    blocking_reason_codes: Vec<String>,
    state_kind: String,
    next_action: String,
    recommended_public_command: Option<LivenessSemanticPublicCommandKey>,
    required_follow_up: Option<String>,
    required_input_names: Vec<String>,
    blocking_scope: Option<String>,
    active_task: Option<u32>,
    blocking_task: Option<u32>,
    resume_task: Option<u32>,
    next_public_action_command: Option<String>,
    blocker_categories: Vec<String>,
    blocker_next_public_action_commands: Vec<Option<String>>,
    public_repair_target_count: usize,
}

#[derive(Debug, Clone)]
struct LivenessPublicCommand {
    argv: Vec<String>,
    kind: LivenessPublicCommandKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LivenessPublicCommandKind {
    WorkflowOperator,
    PlanExecutionBegin,
    PlanExecutionCloseCurrentTask,
    PlanExecutionComplete,
    PlanExecutionReopen,
    PlanExecutionTransfer,
    PlanExecutionAdvanceLateStage,
    PlanExecutionMaterializeProjections,
    PlanExecutionRepairReviewState,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LivenessRuntimeRunner {
    SemanticInProcess,
    ShippedCli,
}

impl LivenessRuntimeRunner {
    fn label(self) -> &'static str {
        match self {
            Self::SemanticInProcess => "semantic in-process public argv",
            Self::ShippedCli => "shipped compiled CLI",
        }
    }
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
    next_public_action: Option<LivenessNextPublicAction>,
}

#[derive(Debug, Clone, Deserialize)]
struct LivenessExecutionCommandContext {
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
            let dispatch_reviewed_state_id =
                if synthetic.stale_boundary_present && task == boundary_task {
                    fixture.reviewed_state_id
                } else {
                    record_reviewed_state_id
                };
            dispatch_lineage.insert(
                format!("task-{task}"),
                json!({
                    "dispatch_id": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "reviewed_state_id": dispatch_reviewed_state_id,
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
        let mut stale_tasks = Vec::new();
        if synthetic.downstream_stale_step_present
            && synthetic.downstream_interrupted_projection_present
            && boundary_task < u32::from(fixture.total_tasks)
        {
            stale_tasks.push(boundary_task);
            stale_tasks.push(boundary_task.saturating_add(1));
        } else if synthetic.downstream_stale_step_present
            && boundary_task < u32::from(fixture.total_tasks)
        {
            stale_tasks.push(boundary_task.saturating_add(1));
        } else {
            stale_tasks.push(boundary_task);
        }
        for stale_task in stale_tasks {
            history_records.insert(format!("task-{stale_task}-stale"), {
                let mut stale_record = closure_record(
                    "git_tree:0000000000000000000000000000000000000000",
                    fixture.task_contract_identities,
                    fixture.task_completion_lineages,
                    stale_task,
                );
                stale_record["closure_record_id"] =
                    Value::from(format!("closure-{stale_task}-stale"));
                stale_record["review_summary_hash"] = Value::from("cccccccc");
                stale_record["verification_summary_hash"] = Value::from("dddddddd");
                stale_record["closure_status"] = Value::from("stale_unreviewed");
                stale_record["record_status"] = Value::from("stale_unreviewed");
                stale_record["record_sequence"] = Value::from(0);
                stale_record
            });
        }
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

fn semantic_status_value(runtime: &ExecutionRuntime, context: &str) -> LivenessPlanExecutionStatus {
    status_value_with_runner(runtime, context, LivenessRuntimeRunner::SemanticInProcess)
}

fn shipped_cli_status_value(
    runtime: &ExecutionRuntime,
    context: &str,
) -> LivenessPlanExecutionStatus {
    status_value_with_runner(runtime, context, LivenessRuntimeRunner::ShippedCli)
}

fn status_value_with_runner(
    runtime: &ExecutionRuntime,
    context: &str,
    runner: LivenessRuntimeRunner,
) -> LivenessPlanExecutionStatus {
    let output = run_featureforge_public_argv_with_runner(
        runtime,
        ["plan", "execution", "status", "--plan", PLAN_REL],
        context,
        runner,
    );
    assert!(
        output.status.success(),
        "{context} should succeed through {}, got {:?}\nstdout:\n{}\nstderr:\n{}",
        runner.label(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{context} should emit liveness status JSON through {}: {error}\nstdout:\n{}\nstderr:\n{}",
            runner.label(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn route_signature(status: &LivenessPlanExecutionStatus) -> LivenessRouteSignature {
    LivenessRouteSignature {
        phase_detail: status.phase_detail.clone(),
        review_state_status: status.review_state_status.clone(),
        reason_codes: stable_reason_codes(&status.reason_codes),
        blocking_reason_codes: stable_reason_codes(&status.blocking_reason_codes),
        state_kind: status.state_kind.clone(),
        next_action: status.next_action.clone(),
        recommended_public_command: status
            .recommended_public_command_argv
            .as_deref()
            .map(liveness_semantic_public_command_key),
        required_follow_up: status.required_follow_up.clone(),
        required_input_names: status
            .required_inputs
            .iter()
            .map(|input| {
                input
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("<unnamed>")
                    .to_owned()
            })
            .collect(),
        blocking_scope: status.blocking_scope.clone(),
        active_task: status.active_task,
        blocking_task: status.blocking_task,
        resume_task: status.resume_task,
        next_public_action_command: status
            .next_public_action
            .as_ref()
            .map(|action| action.command.clone()),
        blocker_categories: status
            .blockers
            .iter()
            .map(|blocker| blocker.category.clone())
            .collect(),
        blocker_next_public_action_commands: status
            .blockers
            .iter()
            .map(|blocker| {
                blocker
                    .next_public_action
                    .as_ref()
                    .map(|action| action.command.clone())
            })
            .collect(),
        public_repair_target_count: status.public_repair_targets.len(),
    }
}

fn stable_reason_codes(reason_codes: &[String]) -> Vec<String> {
    reason_codes
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn prepare_synthetic_liveness_fixture(
    repo: &Path,
    state: &Path,
    total_tasks: u8,
    completed_tasks: u8,
    steps_per_task: u8,
    synthetic: SyntheticState,
    context_label: &str,
) {
    let state_path = harness_state_file_path(repo, state);
    clear_liveness_authority(&state_path);
    write_spec_and_plan(repo, total_tasks, completed_tasks, steps_per_task);
    let task_contract_identities = task_contract_identities(repo, state, total_tasks);
    let task_completion_lineages = task_completion_lineage_fingerprints(repo, state, total_tasks);
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
            .expect("liveness fixture should resolve current tracked tree")
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
    write_variant_harness_state(&fixture, synthetic);
    apply_synthetic_worktree_variants(repo, synthetic);
    synthetic_migrate_variant_to_event_authority(&state_path, context_label);
}

fn synthetic_migrate_variant_to_event_authority(state_path: &Path, context: &str) {
    // Synthetic model-state generation only: each generated authority snapshot
    // is then exercised through exact public command parsing/runtime edges for liveness.
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
        &[" workflow ", "record", "-pivot "][..],
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
    if let (Some(before_argv), Some(after_argv)) = (
        before_status.recommended_public_command_argv.as_deref(),
        after_status.recommended_public_command_argv.as_deref(),
    ) && semantic_public_mutation_key(before_status, before_argv)
        == semantic_public_mutation_key(after_status, after_argv)
        && is_public_mutation_repeat_guard_command(after_status, after_argv)
    {
        return false;
    }
    if let (Some(before_argv), Some(after_argv)) = (
        before_status.recommended_public_command_argv.as_deref(),
        after_status.recommended_public_command_argv.as_deref(),
    ) && semantic_public_mutation_key(before_status, before_argv)
        != semantic_public_mutation_key(after_status, after_argv)
        && is_public_mutation_repeat_guard_command(after_status, after_argv)
    {
        return true;
    }
    if after_status.recommended_public_command_argv == before_status.recommended_public_command_argv
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
        .recommended_public_command_argv
        .as_ref()
        .map(|argv| exact_public_argv_display(argv))
}

fn display_status_command(status: &LivenessPlanExecutionStatus) -> Option<String> {
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
                .filter_map(|blocker| blocker.next_public_action.as_ref())
                .map(|action| action.command.clone())
                .next()
        })
}

fn exact_public_argv_display(argv: &[String]) -> String {
    argv.join(" ")
}

fn public_command_kind_from_argv(argv: &[String]) -> LivenessPublicCommandKind {
    match argv
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["featureforge", "workflow", "operator", ..] => LivenessPublicCommandKind::WorkflowOperator,
        ["featureforge", "plan", "execution", "begin", ..] => {
            LivenessPublicCommandKind::PlanExecutionBegin
        }
        [
            "featureforge",
            "plan",
            "execution",
            "close-current-task",
            ..,
        ] => LivenessPublicCommandKind::PlanExecutionCloseCurrentTask,
        ["featureforge", "plan", "execution", "complete", ..] => {
            LivenessPublicCommandKind::PlanExecutionComplete
        }
        ["featureforge", "plan", "execution", "reopen", ..] => {
            LivenessPublicCommandKind::PlanExecutionReopen
        }
        ["featureforge", "plan", "execution", "transfer", ..] => {
            LivenessPublicCommandKind::PlanExecutionTransfer
        }
        [
            "featureforge",
            "plan",
            "execution",
            "advance-late-stage",
            ..,
        ] => LivenessPublicCommandKind::PlanExecutionAdvanceLateStage,
        [
            "featureforge",
            "plan",
            "execution",
            "materialize-projections",
            ..,
        ] => LivenessPublicCommandKind::PlanExecutionMaterializeProjections,
        [
            "featureforge",
            "plan",
            "execution",
            "repair-review-state",
            ..,
        ] => LivenessPublicCommandKind::PlanExecutionRepairReviewState,
        _ => LivenessPublicCommandKind::Other,
    }
}

fn liveness_semantic_public_command_key(argv: &[String]) -> LivenessSemanticPublicCommandKey {
    LivenessSemanticPublicCommandKey {
        command_kind: public_command_kind_from_argv(argv),
        task: argv_flag_value(argv, "--task")
            .or_else(|| argv_flag_value(argv, "--repair-task"))
            .and_then(|value| value.parse().ok()),
        step: argv_flag_value(argv, "--step")
            .or_else(|| argv_flag_value(argv, "--repair-step"))
            .and_then(|value| value.parse().ok()),
        mode: argv_flag_value(argv, "--mode").map(str::to_owned),
        transfer_mode: liveness_transfer_mode_for_argv(argv),
        transfer_scope: argv_flag_value(argv, "--scope").map(str::to_owned),
        source: argv_flag_value(argv, "--source").map(str::to_owned),
    }
}

fn liveness_transfer_mode_for_argv(argv: &[String]) -> Option<String> {
    if public_command_kind_from_argv(argv) != LivenessPublicCommandKind::PlanExecutionTransfer {
        return argv_flag_value(argv, "--transfer-mode").map(str::to_owned);
    }
    if argv_flag_value(argv, "--repair-task").is_some()
        || argv_flag_value(argv, "--repair-step").is_some()
    {
        return Some(String::from("repair-step"));
    }
    if argv_flag_value(argv, "--scope").is_some() {
        return Some(String::from("workflow-handoff"));
    }
    argv_flag_value(argv, "--transfer-mode").map(str::to_owned)
}

fn exact_public_command_from_status(
    status: &LivenessPlanExecutionStatus,
) -> Option<LivenessPublicCommand> {
    status
        .recommended_public_command_argv
        .as_ref()
        .map(|argv| LivenessPublicCommand {
            argv: argv.clone(),
            kind: public_command_kind_from_argv(argv),
        })
}

fn status_missing_public_inputs(status: &LivenessPlanExecutionStatus) -> bool {
    !status.required_inputs.is_empty()
}

fn exact_public_progress_command(
    runtime: &ExecutionRuntime,
    status: &LivenessPlanExecutionStatus,
    runner: LivenessRuntimeRunner,
) -> Option<LivenessPublicCommand> {
    if let Some(command) = exact_public_command_from_status(status) {
        return Some(command);
    }
    workflow_operator_recommended_public_command(runtime, runner)
        .ok()
        .flatten()
}

fn execute_semantic_public_progress_edge(
    runtime: &ExecutionRuntime,
    status: &LivenessPlanExecutionStatus,
) -> Result<Option<LivenessPlanExecutionStatus>, String> {
    execute_public_progress_edge_with_runner(
        runtime,
        status,
        LivenessRuntimeRunner::SemanticInProcess,
    )
}

fn execute_public_progress_edge_with_runner(
    runtime: &ExecutionRuntime,
    status: &LivenessPlanExecutionStatus,
    runner: LivenessRuntimeRunner,
) -> Result<Option<LivenessPlanExecutionStatus>, String> {
    let Some(command) = exact_public_progress_command(runtime, status, runner) else {
        return Ok(None);
    };
    execute_exact_public_command_with_runner(runtime, status, &command.argv, runner)?;
    Ok(Some(status_value_with_runner(
        runtime,
        &format!(
            "liveness successor after {} edge `{}`",
            runner.label(),
            exact_public_argv_display(&command.argv)
        ),
        runner,
    )))
}

fn execute_exact_public_command_with_runner(
    runtime: &ExecutionRuntime,
    status: &LivenessPlanExecutionStatus,
    argv: &[String],
    runner: LivenessRuntimeRunner,
) -> Result<(), String> {
    assert_exact_public_argv_is_executable(argv)?;
    let words = argv.iter().map(String::as_str).collect::<Vec<_>>();
    match words.as_slice() {
        ["featureforge", "workflow", "operator", ..] => {
            let exact_args = argv[1..].iter().map(String::as_str);
            let exact_output = run_featureforge_public_argv_with_runner(
                runtime,
                exact_args,
                &format!(
                    "liveness exact {} edge `{}`",
                    runner.label(),
                    exact_public_argv_display(argv)
                ),
                runner,
            );
            if !exact_output.status.success() {
                return Err(format!(
                    "workflow operator exact {} edge failed: {}\nstdout:\n{}\nstderr:\n{}",
                    runner.label(),
                    exact_public_argv_display(argv),
                    String::from_utf8_lossy(&exact_output.stdout),
                    String::from_utf8_lossy(&exact_output.stderr)
                ));
            }
            let operator_value: Value = serde_json::from_slice(&exact_output.stdout).map_err(
                |error| {
                    format!(
                        "workflow operator exact {} edge should emit JSON from exact argv `{}`: {error}\nstdout:\n{}\nstderr:\n{}",
                        runner.label(),
                        exact_public_argv_display(argv),
                        String::from_utf8_lossy(&exact_output.stdout),
                        String::from_utf8_lossy(&exact_output.stderr)
                    )
                },
            )?;
            let Some(operator_command) = public_command_from_json_value(
                &operator_value,
                &format!(
                    "workflow operator exact {} edge `{}`",
                    runner.label(),
                    exact_public_argv_display(argv)
                ),
            )?
            else {
                return Err(String::from(
                    "workflow operator public argv edge did not return a concrete successor argv",
                ));
            };
            if operator_command.kind == LivenessPublicCommandKind::WorkflowOperator {
                return Err(format!(
                    "workflow operator public argv edge looped back to itself instead of surfacing a concrete successor: {}",
                    exact_public_argv_display(&operator_command.argv)
                ));
            }
            let mut routed_status = status.clone();
            routed_status.recommended_command =
                Some(exact_public_argv_display(&operator_command.argv));
            routed_status.recommended_public_command_argv = Some(operator_command.argv.clone());
            routed_status.required_inputs.clear();
            routed_status.next_public_action = None;
            routed_status.blockers.clear();
            execute_exact_public_command_with_runner(
                runtime,
                &routed_status,
                &operator_command.argv,
                runner,
            )
        }
        ["featureforge", "plan", "execution", args @ ..] => {
            let output = run_featureforge_public_argv_with_runner(
                runtime,
                std::iter::once("plan")
                    .chain(std::iter::once("execution"))
                    .chain(args.iter().copied()),
                &format!(
                    "liveness {} edge `{}`",
                    runner.label(),
                    exact_public_argv_display(argv)
                ),
                runner,
            );
            if output.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "liveness {} edge failed: {}\nstatus_phase_detail={}\nstatus_recommended_command={:?}\nstdout:\n{}\nstderr:\n{}",
                    runner.label(),
                    exact_public_argv_display(argv),
                    status.phase_detail,
                    status.recommended_command,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ))
            }
        }
        _ => Err(format!(
            "liveness public argv edge is not a supported public FeatureForge command: {}",
            exact_public_argv_display(argv)
        )),
    }
}

fn workflow_operator_recommended_public_command(
    runtime: &ExecutionRuntime,
    runner: LivenessRuntimeRunner,
) -> Result<Option<LivenessPublicCommand>, String> {
    let output = run_featureforge_public_argv_with_runner(
        runtime,
        ["workflow", "operator", "--plan", PLAN_REL, "--json"],
        "liveness workflow operator public argv edge",
        runner,
    );
    if !output.status.success() {
        return Err(format!(
            "workflow operator {} edge failed\nstdout:\n{}\nstderr:\n{}",
            runner.label(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("workflow operator public argv edge should emit JSON: {error}"))?;
    public_command_from_json_value(&value, "workflow operator public argv edge")
}

fn public_command_from_json_value(
    value: &Value,
    context: &str,
) -> Result<Option<LivenessPublicCommand>, String> {
    let Some(argv_value) = value
        .get("recommended_public_command_argv")
        .filter(|argv| !argv.is_null())
    else {
        let has_display_command = value
            .get("recommended_command")
            .and_then(Value::as_str)
            .is_some()
            || value
                .get("next_public_action")
                .and_then(|action| action.get("command"))
                .and_then(Value::as_str)
                .is_some()
            || value
                .get("blockers")
                .and_then(Value::as_array)
                .is_some_and(|blockers| {
                    blockers.iter().any(|blocker| {
                        blocker
                            .get("next_public_action")
                            .and_then(|action| action.get("command"))
                            .and_then(Value::as_str)
                            .is_some()
                    })
                });
        if value
            .get("required_inputs")
            .and_then(Value::as_array)
            .is_some_and(|inputs| !inputs.is_empty())
            || !has_display_command
        {
            return Ok(None);
        }
        return Err(format!(
            "{context} exposed a display command without executable argv or required_inputs: {value}"
        ));
    };
    let Some(argv_values) = argv_value.as_array() else {
        return Err(format!(
            "{context} recommended_public_command_argv should be an array: {value}"
        ));
    };
    let argv = argv_values
        .iter()
        .map(|part| {
            part.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{context} argv entry should be a string: {value}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_exact_public_argv_is_executable(&argv)?;
    Ok(Some(LivenessPublicCommand {
        kind: public_command_kind_from_argv(&argv),
        argv,
    }))
}

fn assert_exact_public_argv_is_executable(argv: &[String]) -> Result<(), String> {
    if argv.first().map(String::as_str) != Some("featureforge") {
        return Err(format!(
            "recommended_public_command_argv must start with featureforge: {argv:?}"
        ));
    }
    for part in argv {
        if public_argv_part_has_template_token(part) {
            return Err(format!(
                "recommended_public_command_argv must be executable, not templated: {argv:?}"
            ));
        }
    }
    if argv.windows(3).any(|window| {
        (window[0] == "[when" || window[0] == "when")
            && window[1] == "verification"
            && (window[2] == "ran]" || window[2] == "ran")
    }) {
        return Err(format!(
            "recommended_public_command_argv must be executable, not optional prose: {argv:?}"
        ));
    }
    Ok(())
}

fn run_featureforge_public_argv_with_runner<'a>(
    runtime: &ExecutionRuntime,
    args: impl IntoIterator<Item = &'a str>,
    context: &str,
    runner: LivenessRuntimeRunner,
) -> std::process::Output {
    let args = args.into_iter().collect::<Vec<_>>();
    match runner {
        LivenessRuntimeRunner::SemanticInProcess => {
            semantic_public_argv_runtime::run_semantic_public_argv_in_process(
                &runtime.repo_root,
                &runtime.state_dir,
                args.iter().copied(),
                context,
            )
        }
        LivenessRuntimeRunner::ShippedCli => {
            public_featureforge_cli::run_featureforge_with_env_control_real_cli(
                Some(&runtime.repo_root),
                Some(&runtime.state_dir),
                None,
                &[],
                &[],
                &args,
                context,
            )
        }
    }
}

fn public_argv_part_has_template_token(part: &str) -> bool {
    const DENYLIST: &[&str] = &[
        "<approved-plan-path>",
        "[when verification ran]",
        "when verification ran",
        "<path>",
        "<reason>",
        "<claim>",
        "<summary>",
        "<owner>",
        "<source>",
        "<id>",
        "pass|fail",
        "pass|fail|not-run",
        "ready|blocked",
        "task|branch",
    ];
    DENYLIST.iter().any(|token| {
        part == *token
            || part
                .split_once('=')
                .is_some_and(|(_, value)| value == *token)
    })
}

fn argv_flag_value<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
    argv.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].as_str()))
}

fn reopen_target(argv: &[String]) -> Option<(u32, u32)> {
    let words = argv.iter().map(String::as_str).collect::<Vec<_>>();
    if !words
        .windows(3)
        .any(|window| window == ["plan", "execution", "reopen"])
    {
        return None;
    }
    let task = argv_flag_value(argv, "--task")?.parse::<u32>().ok()?;
    let step = argv_flag_value(argv, "--step")?.parse::<u32>().ok()?;
    Some((task, step))
}

fn substantive_repo_work_invalidates_current_closure(synthetic: SyntheticState) -> bool {
    synthetic.stale_boundary_present || synthetic.downstream_stale_step_present
}

fn assert_no_same_current_task_reopen_without_substantive_work(
    label: &str,
    synthetic: SyntheticState,
    argv: &[String],
) {
    if synthetic.completed_tasks == 0
        || substantive_repo_work_invalidates_current_closure(synthetic)
    {
        return;
    }
    let Some((task, step)) = reopen_target(argv) else {
        return;
    };
    let command = exact_public_argv_display(argv);
    assert!(
        task > u32::from(synthetic.completed_tasks),
        "{label} must not recommend reopening already-current Task {task} Step {step} without substantive repo work invalidating that closure; command={command}"
    );
}

fn assert_multistep_reopen_uses_shared_exact_command_semantics(
    label: &str,
    synthetic: SyntheticState,
    argv: &[String],
) {
    if synthetic.steps_per_task <= 1 || synthetic.completed_tasks == 0 {
        return;
    }
    let Some((task, step)) = reopen_target(argv) else {
        return;
    };
    if task > u32::from(synthetic.completed_tasks) {
        return;
    }
    let command = exact_public_argv_display(argv);
    assert_eq!(
        step,
        u32::from(synthetic.steps_per_task),
        "{label} must reopen the latest attempted step for completed multi-step Task {task}; command={command}"
    );
    assert!(
        argv_flag_value(argv, "--source") == Some("featureforge:executing-plans"),
        "{label} must derive reopen source from the shared exact-command execution mode; command={command}"
    );
}

fn assert_reopen_liveness_contract(label: &str, synthetic: SyntheticState, argv: &[String]) {
    assert_no_same_current_task_reopen_without_substantive_work(label, synthetic, argv);
    assert_multistep_reopen_uses_shared_exact_command_semantics(label, synthetic, argv);
}

fn assert_targetless_stale_does_not_fabricate_current_task(
    label: &str,
    synthetic: SyntheticState,
    status: &LivenessPlanExecutionStatus,
) {
    if !synthetic.targetless_stale_present || synthetic.stale_boundary_present {
        return;
    }
    let completed_task = u32::from(synthetic.completed_tasks);
    let execution_context_task = status
        .execution_command_context
        .as_ref()
        .and_then(|context| context.task_number);
    assert!(
        status.stale_unreviewed_closures.is_empty(),
        "{label} targetless stale state must not fabricate stale closure ids: {status:?}"
    );
    assert!(
        status.blocking_task != Some(completed_task)
            && status.resume_task != Some(completed_task)
            && execution_context_task != Some(completed_task),
        "{label} targetless stale state must not fabricate the current task as the stale target: {status:?}"
    );
}

fn assert_earliest_stale_boundary_preempts_later_overlay(
    label: &str,
    synthetic: SyntheticState,
    status: &LivenessPlanExecutionStatus,
) {
    if !(synthetic.stale_boundary_present
        && synthetic.downstream_stale_step_present
        && synthetic.downstream_interrupted_projection_present)
        || status.review_state_status != "stale_unreviewed"
        || status.stale_unreviewed_closures.is_empty()
        || is_diagnostic_status(status)
    {
        return;
    }
    let earliest_task = u32::from(synthetic.completed_tasks.max(1));
    assert_eq!(
        status.blocking_task,
        Some(earliest_task),
        "{label} must converge on the earliest unresolved stale boundary before later stale/interrupted overlays: {status:?}"
    );
}

fn is_repair_review_state_command(argv: &[String]) -> bool {
    public_command_kind_from_argv(argv) == LivenessPublicCommandKind::PlanExecutionRepairReviewState
}

fn assert_repair_review_state_successor_contract(
    label: &str,
    before: &LivenessPlanExecutionStatus,
    after: &LivenessPlanExecutionStatus,
) {
    let same_tuple = after.recommended_public_command_argv
        == before.recommended_public_command_argv
        && after.phase_detail == before.phase_detail
        && after.blocking_reason_codes == before.blocking_reason_codes;
    assert!(
        !same_tuple,
        "{label} repair-review-state must not return the same public command tuple repeatedly: before={before:?} after={after:?}"
    );

    let phase = after.phase_detail.as_str();
    let records_closure = matches!(
        phase,
        "task_closure_recording_ready" | "branch_closure_recording_required_for_release_readiness"
    );
    let records_late_stage = matches!(
        phase,
        "release_readiness_recording_ready"
            | "release_blocker_resolution_required"
            | "final_review_dispatch_required"
            | "final_review_outcome_pending"
            | "final_review_recording_ready"
            | "qa_recording_required"
            | "test_plan_refresh_required"
            | "finish_review_gate_ready"
            | "finish_completion_gate_ready"
    );
    let diagnostic = blocked_runtime_bug_diagnostic_status(after)
        || targetless_stale_reconcile_status(after)
        || (phase == "runtime_reconcile_required" && !after.blocking_reason_codes.is_empty());
    let advances = after.active_task.is_some()
        || after
            .execution_command_context
            .as_ref()
            .and_then(|context| context.task_number)
            .is_some()
        || before.blocking_task.zip(after.blocking_task).is_some_and(
            |(before_task, after_task)| {
                after_task > before_task
                    && earlier_stale_boundary_consumed(before, before_task, after)
            },
        )
        || after.state_kind == "waiting_external_input"
        || after
            .recommended_public_command_argv
            .as_ref()
            .is_some_and(|argv| {
                matches!(
                    public_command_kind_from_argv(argv),
                    LivenessPublicCommandKind::PlanExecutionBegin
                        | LivenessPublicCommandKind::PlanExecutionComplete
                        | LivenessPublicCommandKind::PlanExecutionReopen
                        | LivenessPublicCommandKind::PlanExecutionCloseCurrentTask
                )
            });

    assert!(
        records_closure || records_late_stage || diagnostic || advances,
        "{label} repair-review-state successor must advance, record closure/late-stage state, or emit a diagnostic: before={before:?} after={after:?}"
    );
}

fn earlier_stale_boundary_consumed(
    before: &LivenessPlanExecutionStatus,
    before_task: u32,
    after: &LivenessPlanExecutionStatus,
) -> bool {
    if before.review_state_status != "stale_unreviewed"
        || before.blocking_task != Some(before_task)
        || before.stale_unreviewed_closures.is_empty()
    {
        return true;
    }
    after.review_state_status != "stale_unreviewed"
        || after.stale_unreviewed_closures.is_empty()
        || !status_stale_closure_ids_reference_task(after, before_task)
}

fn status_stale_closure_ids_reference_task(
    status: &LivenessPlanExecutionStatus,
    task: u32,
) -> bool {
    let task_marker = format!("task-{task}");
    let closure_marker = format!("closure-{task}");
    status.stale_unreviewed_closures.iter().any(|record_id| {
        record_id == &task_marker
            || record_id.starts_with(&format!("{task_marker}-"))
            || record_id.contains(&format!("-{task}-"))
            || record_id == &closure_marker
            || record_id.starts_with(&format!("{closure_marker}-"))
    })
}

fn reaches_real_work_or_wait(status: &LivenessPlanExecutionStatus) -> bool {
    status.state_kind == "terminal"
        || status.state_kind == "waiting_external_input"
        || status.active_task.is_some()
        || status.phase_detail == "execution_in_progress"
}

fn is_diagnostic_status(status: &LivenessPlanExecutionStatus) -> bool {
    blocked_runtime_bug_diagnostic_status(status)
        || current_stale_overlap_runtime_diagnostic(status)
        || targetless_stale_reconcile_status(status)
        || (status.phase_detail == "runtime_reconcile_required"
            && !status.blocking_reason_codes.is_empty())
}

fn assert_diagnostic_status_shape(label: &str, status: &LivenessPlanExecutionStatus) {
    assert!(
        status.recommended_public_command_argv.is_none(),
        "{label} diagnostic status must not expose executable public argv: {status:?}"
    );
    assert!(
        status.required_inputs.is_empty(),
        "{label} diagnostic status must not expose typed local inputs: {status:?}"
    );
    assert!(
        status.next_public_action.is_none(),
        "{label} diagnostic status must not expose next_public_action: {status:?}"
    );
    assert!(
        status.public_repair_targets.is_empty(),
        "{label} diagnostic status must not expose public repair targets: {status:?}"
    );
    assert!(
        status.blockers.is_empty(),
        "{label} diagnostic status must not expose route blockers: {status:?}"
    );
    assert_ne!(
        status.next_action, "repair review state",
        "{label} diagnostic status must not publish repair/reentry next_action wording: {status:?}"
    );
    assert_eq!(
        status.next_action, "runtime diagnostic required",
        "{label} diagnostic status should publish diagnostic next_action: {status:?}"
    );
}

fn is_legal_real_work_command(argv: &[String]) -> bool {
    matches!(
        public_command_kind_from_argv(argv),
        LivenessPublicCommandKind::PlanExecutionBegin
            | LivenessPublicCommandKind::PlanExecutionComplete
            | LivenessPublicCommandKind::PlanExecutionReopen
            | LivenessPublicCommandKind::PlanExecutionTransfer
    )
}

fn is_replayable_real_work_command(
    synthetic: SyntheticState,
    _status: &LivenessPlanExecutionStatus,
    argv: &[String],
) -> bool {
    match public_command_kind_from_argv(argv) {
        LivenessPublicCommandKind::PlanExecutionBegin
        | LivenessPublicCommandKind::PlanExecutionTransfer => true,
        // Completing a step is a human-work boundary. Bounded liveness stops
        // there unless the runtime emits a fully-bound executable argv for
        // substantive task work.
        LivenessPublicCommandKind::PlanExecutionComplete => false,
        LivenessPublicCommandKind::PlanExecutionReopen => reopen_target(argv)
            .is_some_and(|(task, _step)| task <= u32::from(synthetic.completed_tasks)),
        _ => false,
    }
}

fn command_requires_external_result_input(
    status: &LivenessPlanExecutionStatus,
    argv: &[String],
) -> bool {
    match public_command_kind_from_argv(argv) {
        LivenessPublicCommandKind::PlanExecutionCloseCurrentTask => true,
        LivenessPublicCommandKind::PlanExecutionAdvanceLateStage => {
            matches!(
                status.phase_detail.as_str(),
                "release_readiness_recording_ready"
                    | "release_blocker_resolution_required"
                    | "final_review_recording_ready"
                    | "qa_recording_required"
            ) && (argv_flag_value(argv, "--result").is_none()
                || argv
                    .iter()
                    .any(|part| public_argv_part_has_template_token(part)))
        }
        _ => false,
    }
}

fn assert_public_command_is_safe(label: &str, argv: &[String]) {
    let command = exact_public_argv_display(argv);
    assert!(
        argv.first().map(String::as_str) == Some("featureforge"),
        "{label} recommended argv must remain public: {command}"
    );
    assert!(
        !contains_hidden_command_token(&command),
        "{label} recommended command must not route through hidden command lanes: {command}"
    );
}

fn liveness_route_key(status: &LivenessPlanExecutionStatus, argv: &[String]) -> String {
    format!(
        "phase={};review={};state={};follow_up={:?};command={:?};scope={:?};task={:?};step={:?};reasons={:?};blocking_reasons={:?}",
        status.phase_detail,
        status.review_state_status,
        status.state_kind,
        status.required_follow_up,
        liveness_semantic_public_command_key(argv),
        status.blocking_scope,
        status.blocking_task,
        argv_flag_value(argv, "--step").and_then(|value| value.parse::<u32>().ok()),
        stable_reason_codes(&status.reason_codes),
        stable_reason_codes(&status.blocking_reason_codes)
    )
}

fn is_runtime_management_mutation_command(
    status: &LivenessPlanExecutionStatus,
    argv: &[String],
) -> bool {
    if is_legal_real_work_command(argv) || command_requires_external_result_input(status, argv) {
        return false;
    }

    matches!(
        public_command_kind_from_argv(argv),
        LivenessPublicCommandKind::PlanExecutionAdvanceLateStage
            | LivenessPublicCommandKind::PlanExecutionMaterializeProjections
            | LivenessPublicCommandKind::PlanExecutionRepairReviewState
    )
}

fn is_public_mutation_repeat_guard_command(
    status: &LivenessPlanExecutionStatus,
    argv: &[String],
) -> bool {
    if command_requires_external_result_input(status, argv) {
        return false;
    }

    matches!(
        public_command_kind_from_argv(argv),
        LivenessPublicCommandKind::PlanExecutionAdvanceLateStage
            | LivenessPublicCommandKind::PlanExecutionBegin
            | LivenessPublicCommandKind::PlanExecutionComplete
            | LivenessPublicCommandKind::PlanExecutionMaterializeProjections
            | LivenessPublicCommandKind::PlanExecutionReopen
            | LivenessPublicCommandKind::PlanExecutionRepairReviewState
            | LivenessPublicCommandKind::PlanExecutionTransfer
    )
}

fn semantic_public_mutation_key(status: &LivenessPlanExecutionStatus, argv: &[String]) -> String {
    let command = liveness_semantic_public_command_key(argv);
    if public_command_kind_from_argv(argv)
        == LivenessPublicCommandKind::PlanExecutionRepairReviewState
    {
        let execution_task = status
            .execution_command_context
            .as_ref()
            .and_then(|context| context.task_number);
        return format!(
            "{command:?};blocking_task={:?};execution_task={:?}",
            status.blocking_task, execution_task
        );
    }
    format!("{command:?}")
}

fn remember_public_mutation_command(
    seen_commands: &mut BTreeSet<String>,
    status: &LivenessPlanExecutionStatus,
    argv: &[String],
) -> Result<(), String> {
    if is_public_mutation_repeat_guard_command(status, argv)
        && !seen_commands.insert(semantic_public_mutation_key(status, argv))
    {
        return Err(format!(
            "repeated public mutation before convergence: {}",
            exact_public_argv_display(argv)
        ));
    }
    Ok(())
}

struct BoundedLivenessContext<'a> {
    runtime: &'a ExecutionRuntime,
    total_tasks: u8,
    label: &'a str,
    synthetic: SyntheticState,
}

fn assert_bounded_runtime_management_path_converges(
    context: &BoundedLivenessContext<'_>,
    first_status: &LivenessPlanExecutionStatus,
    first_successor: &LivenessPlanExecutionStatus,
    first_command: Option<&[String]>,
) {
    let mut seen_route_keys = BTreeSet::new();
    let mut seen_public_mutation_commands = BTreeSet::new();
    if let Some(command) = first_command {
        assert_reopen_liveness_contract(context.label, context.synthetic, command);
        seen_route_keys.insert(liveness_route_key(first_status, command));
        remember_public_mutation_command(&mut seen_public_mutation_commands, first_status, command)
            .unwrap_or_else(|error| panic!("{} {error}", context.label));
    }

    let mut previous = first_status.clone();
    let mut current = first_successor.clone();
    for edge in 2..=3 {
        if is_diagnostic_status(&current)
            || current.state_kind == "terminal"
            || current.state_kind == "waiting_external_input"
            || status_missing_public_inputs(&current)
        {
            return;
        }

        let maybe_command = current.recommended_public_command_argv.as_deref();
        if reaches_real_work_or_wait(&current)
            && maybe_command.is_none_or(|command| {
                !is_replayable_real_work_command(context.synthetic, &current, command)
            })
        {
            return;
        }

        let command = maybe_command.unwrap_or_else(|| {
            panic!(
                "{} bounded convergence edge {edge} reached a non-converged commandless state after excluding real work, wait, and diagnostics: previous={previous:?}; current={current:?}",
                context.label
            )
        });
        assert_public_command_is_safe(context.label, command);
        assert_reopen_liveness_contract(context.label, context.synthetic, command);
        let route_key = liveness_route_key(&current, command);
        assert!(
            seen_route_keys.insert(route_key.clone()),
            "{} repeated the same public route tuple after an intervening edge instead of converging; repeated={route_key}; previous={previous:?}; current={current:?}",
            context.label
        );
        remember_public_mutation_command(&mut seen_public_mutation_commands, &current, command)
            .unwrap_or_else(|error| {
                panic!(
                    "{} {error}; previous={previous:?}; current={current:?}",
                    context.label
                )
            });
        if command_requires_external_result_input(&current, command) {
            return;
        }
        if is_legal_real_work_command(command)
            && !is_replayable_real_work_command(context.synthetic, &current, command)
        {
            return;
        }

        let before = progress_metric_from_status(&current, context.total_tasks);
        let successor = execute_semantic_public_progress_edge(context.runtime, &current)
            .unwrap_or_else(|error| {
                panic!(
                    "{} bounded convergence edge {edge} should execute `{}`: {error}",
                    context.label,
                    exact_public_argv_display(command)
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "{} bounded convergence edge {edge} should expose a concrete successor from {current:?}",
                    context.label
                )
            });
        let after = progress_metric_from_status(&successor, context.total_tasks);
        assert!(
            public_edge_satisfies_liveness_contract(&current, before, &successor, after),
            "{} bounded convergence edge {edge} must reduce the metric, expose a different blocker, or reach a diagnostic; before={before:?} after={after:?} current={current:?} successor={successor:?}",
            context.label
        );
        if is_repair_review_state_command(command) {
            assert_repair_review_state_successor_contract(context.label, &current, &successor);
        }
        previous = current;
        current = successor;
    }

    if is_diagnostic_status(&current)
        || current.state_kind == "terminal"
        || current.state_kind == "waiting_external_input"
        || status_missing_public_inputs(&current)
    {
        return;
    }
    let maybe_command = current.recommended_public_command_argv.as_deref();
    if reaches_real_work_or_wait(&current)
        && maybe_command.is_none_or(|command| {
            !is_replayable_real_work_command(context.synthetic, &current, command)
        })
    {
        return;
    }
    let command = maybe_command.unwrap_or_else(|| {
        panic!(
            "{} semantic public-argv runtime-management path exhausted the bounded replay in a non-converged commandless state after excluding real work, wait, and diagnostics: current={current:?}",
            context.label
        )
    });
    assert_reopen_liveness_contract(context.label, context.synthetic, command);
    remember_public_mutation_command(&mut seen_public_mutation_commands, &current, command)
        .unwrap_or_else(|error| panic!("{} {error}; current={current:?}", context.label));
    let route_key = liveness_route_key(&current, command);
    assert!(
        !seen_route_keys.contains(&route_key)
            || command_requires_external_result_input(&current, command),
        "{} public mutation path repeated a prior route tuple within three edges; repeated={route_key}; current={current:?}",
        context.label
    );
}

fn minimal_liveness_status(
    phase_detail: &str,
    blocking_reason_codes: Vec<String>,
    recommended_public_command_argv: Option<Vec<String>>,
) -> LivenessPlanExecutionStatus {
    if let Some(argv) = recommended_public_command_argv.as_ref() {
        assert_exact_public_argv_is_executable(argv)
            .expect("minimal liveness fixtures must receive exact typed public argv");
    }
    let recommended_command = recommended_public_command_argv
        .as_ref()
        .map(|argv| exact_public_argv_display(argv));
    LivenessPlanExecutionStatus {
        phase_detail: phase_detail.to_owned(),
        review_state_status: String::from("stale_unreviewed"),
        current_task_closures: Vec::new(),
        stale_unreviewed_closures: Vec::new(),
        reason_codes: Vec::new(),
        blocking_reason_codes,
        state_kind: String::from("blocked"),
        next_public_action: None,
        blockers: Vec::new(),
        public_repair_targets: Vec::new(),
        next_action: String::from("repair review state"),
        recommended_command,
        recommended_public_command_argv,
        required_follow_up: None,
        required_inputs: Vec::new(),
        blocking_scope: Some(String::from("task")),
        execution_command_context: None,
        active_task: None,
        blocking_task: Some(1),
        resume_task: None,
    }
}

fn minimal_display_only_liveness_status(
    phase_detail: &str,
    blocking_reason_codes: Vec<String>,
    recommended_command: String,
) -> LivenessPlanExecutionStatus {
    let mut status = minimal_liveness_status(phase_detail, blocking_reason_codes, None);
    status.recommended_command = Some(recommended_command);
    status
}

fn task_closure_required_inputs() -> Vec<Value> {
    vec![
        json!({
            "name": "review_result",
            "kind": "enum",
            "values": ["pass", "fail"],
        }),
        json!({
            "name": "review_summary_file",
            "kind": "path",
            "must_exist": true,
        }),
        json!({
            "name": "verification_result",
            "kind": "enum",
            "values": ["pass", "fail", "not-run"],
        }),
    ]
}

fn test_public_argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

fn exact_test_public_argv(parts: &[&str]) -> Vec<String> {
    let argv = test_public_argv(parts);
    assert_exact_public_argv_is_executable(&argv).expect("test argv should be exact public argv");
    argv
}

#[test]
fn exact_public_argv_rejects_optional_verification_placeholder() {
    let argv = test_public_argv(&[
        "featureforge",
        "plan",
        "execution",
        "close-current-task",
        "--plan",
        "docs/plan.md",
        "--task",
        "1",
        "--review-result",
        "pass",
        "--review-summary-file",
        "review.md",
        "--verification-result",
        "pass",
        "[when verification ran]",
    ]);
    assert!(
        assert_exact_public_argv_is_executable(&argv).is_err(),
        "optional verification prose must not be accepted as executable argv"
    );

    let status = minimal_display_only_liveness_status(
        "task_closure_recording_ready",
        Vec::new(),
        exact_public_argv_display(&argv),
    );
    assert!(
        status.recommended_public_command_argv.is_none(),
        "liveness fixtures must keep optional prose display-only unless exact typed argv is supplied"
    );
}

#[test]
fn repeated_public_mutation_detection_ignores_reason_churn() {
    let command = exact_test_public_argv(&[
        "featureforge",
        "plan",
        "execution",
        "repair-review-state",
        "--plan",
        "docs/plan.md",
    ]);
    let mut seen_commands = BTreeSet::new();
    let before = minimal_liveness_status(
        "execution_reentry_required",
        vec![String::from("current_task_closure_stale")],
        Some(command.clone()),
    );
    let after = minimal_liveness_status(
        "execution_reentry_required",
        vec![String::from("projection_reconcile_required")],
        Some(command.clone()),
    );

    remember_public_mutation_command(
        &mut seen_commands,
        &before,
        before
            .recommended_public_command_argv
            .as_deref()
            .expect("test command should materialize typed argv"),
    )
    .expect("first runtime-management mutation should be accepted");
    assert!(
        remember_public_mutation_command(
            &mut seen_commands,
            &after,
            after
                .recommended_public_command_argv
                .as_deref()
                .expect("test command should materialize typed argv")
        )
        .is_err(),
        "repeated runtime-management mutation must be rejected even when route metadata changes"
    );
    assert!(
        !public_edge_satisfies_liveness_contract(
            &before,
            ProgressMetric::default(),
            &after,
            ProgressMetric::default(),
        ),
        "public-edge liveness must reject the same runtime-management mutation with changed reasons"
    );

    let reopen = exact_test_public_argv(&[
        "featureforge",
        "plan",
        "execution",
        "reopen",
        "--plan",
        "docs/plan.md",
        "--task",
        "1",
        "--step",
        "1",
    ]);
    let before = minimal_liveness_status(
        "execution_reentry_required",
        vec![String::from("current_task_closure_stale")],
        Some(reopen.clone()),
    );
    let after = minimal_liveness_status(
        "execution_reentry_required",
        vec![String::from("route_metadata_changed")],
        Some(reopen.clone()),
    );
    let mut seen_commands = BTreeSet::new();
    remember_public_mutation_command(
        &mut seen_commands,
        &before,
        before
            .recommended_public_command_argv
            .as_deref()
            .expect("test reopen should materialize typed argv"),
    )
    .expect("first real-work mutation should be accepted");
    assert!(
        remember_public_mutation_command(
            &mut seen_commands,
            &after,
            after
                .recommended_public_command_argv
                .as_deref()
                .expect("test reopen should materialize typed argv")
        )
        .is_err(),
        "repeated reopen mutation must be rejected even when route metadata changes"
    );
    assert!(
        !public_edge_satisfies_liveness_contract(
            &before,
            ProgressMetric::default(),
            &after,
            ProgressMetric::default(),
        ),
        "public-edge liveness must reject the same real-work mutation with changed reasons"
    );
}

#[test]
fn repeated_public_mutation_detection_ignores_fingerprint_churn() {
    let begin_with_fingerprint = |fingerprint: &str| {
        exact_test_public_argv(&[
            "featureforge",
            "plan",
            "execution",
            "begin",
            "--plan",
            "docs/plan.md",
            "--task",
            "1",
            "--step",
            "1",
            "--expect-execution-fingerprint",
            fingerprint,
            "--source",
            "featureforge:executing-plans",
        ])
    };
    let before_command = begin_with_fingerprint("fingerprint-a");
    let after_command = begin_with_fingerprint("fingerprint-b");
    let before = minimal_liveness_status(
        "execution_reentry_required",
        vec![String::from("execution_reentry_required")],
        Some(before_command.clone()),
    );
    let after = minimal_liveness_status(
        "execution_reentry_required",
        vec![String::from("execution_reentry_required")],
        Some(after_command.clone()),
    );
    let mut seen_commands = BTreeSet::new();

    remember_public_mutation_command(&mut seen_commands, &before, &before_command)
        .expect("first begin mutation should be accepted");
    assert!(
        remember_public_mutation_command(&mut seen_commands, &after, &after_command).is_err(),
        "fingerprint-only argv changes must not be treated as public-route progress"
    );
    assert_eq!(
        semantic_public_mutation_key(&before, &before_command),
        semantic_public_mutation_key(&after, &after_command),
        "semantic mutation keys must exclude --expect-execution-fingerprint values"
    );
    assert!(
        !public_edge_satisfies_liveness_contract(
            &before,
            ProgressMetric::default(),
            &after,
            ProgressMetric::default(),
        ),
        "public-edge liveness must reject fingerprint-only command churn"
    );
}

#[test]
fn repeated_public_mutation_detection_distinguishes_transfer_repair_targets() {
    let transfer_repair = |task: &str, step: &str, fingerprint: &str| {
        exact_test_public_argv(&[
            "featureforge",
            "plan",
            "execution",
            "transfer",
            "--plan",
            "docs/plan.md",
            "--repair-task",
            task,
            "--repair-step",
            step,
            "--expect-execution-fingerprint",
            fingerprint,
        ])
    };
    let task_1 = transfer_repair("1", "1", "fingerprint-a");
    let task_1_fingerprint_churn = transfer_repair("1", "1", "fingerprint-b");
    let task_2 = transfer_repair("2", "1", "fingerprint-c");
    let task_1_status = minimal_liveness_status(
        "execution_reentry_required",
        vec![String::from("execution_reentry_required")],
        Some(task_1.clone()),
    );
    let task_1_fingerprint_churn_status = minimal_liveness_status(
        "execution_reentry_required",
        vec![String::from("execution_reentry_required")],
        Some(task_1_fingerprint_churn.clone()),
    );
    let task_2_status = minimal_liveness_status(
        "execution_reentry_required",
        vec![String::from("execution_reentry_required")],
        Some(task_2.clone()),
    );

    assert_eq!(
        liveness_semantic_public_command_key(&task_1),
        liveness_semantic_public_command_key(&task_1_fingerprint_churn),
        "semantic transfer repair identity must ignore volatile expect-fingerprint argv values"
    );
    assert_ne!(
        liveness_semantic_public_command_key(&task_1),
        liveness_semantic_public_command_key(&task_2),
        "semantic transfer repair identity must include --repair-task/--repair-step targets"
    );
    assert_eq!(
        liveness_semantic_public_command_key(&task_1)
            .transfer_mode
            .as_deref(),
        Some("repair-step")
    );

    let mut seen_commands = BTreeSet::new();
    remember_public_mutation_command(&mut seen_commands, &task_1_status, &task_1)
        .expect("first transfer repair mutation should be accepted");
    assert!(
        remember_public_mutation_command(
            &mut seen_commands,
            &task_1_fingerprint_churn_status,
            &task_1_fingerprint_churn
        )
        .is_err(),
        "fingerprint-only transfer repair churn must be treated as a repeated route"
    );
    remember_public_mutation_command(&mut seen_commands, &task_2_status, &task_2)
        .expect("different transfer repair target should be treated as real progress");
}

#[test]
fn repeated_public_mutation_detection_rejects_repair_to_repeated_reopen_sequence() {
    let repair = exact_test_public_argv(&[
        "featureforge",
        "plan",
        "execution",
        "repair-review-state",
        "--plan",
        "docs/plan.md",
    ]);
    let reopen = exact_test_public_argv(&[
        "featureforge",
        "plan",
        "execution",
        "reopen",
        "--plan",
        "docs/plan.md",
        "--task",
        "1",
        "--step",
        "1",
    ]);
    let mut seen_commands = BTreeSet::new();
    let repair_status = minimal_liveness_status(
        "runtime_reconcile_required",
        vec![String::from(
            "current_task_closure_overlay_restore_required",
        )],
        Some(repair.clone()),
    );
    let reopen_status = minimal_liveness_status(
        "execution_reentry_required",
        vec![String::from("current_task_closure_stale")],
        Some(reopen.clone()),
    );
    let repeated_reopen_status = minimal_liveness_status(
        "execution_reentry_required",
        vec![String::from("route_metadata_changed_after_reopen")],
        Some(reopen.clone()),
    );

    remember_public_mutation_command(
        &mut seen_commands,
        &repair_status,
        repair_status
            .recommended_public_command_argv
            .as_deref()
            .expect("test repair command should materialize typed argv"),
    )
    .expect("repair-review-state should be accepted as the first public mutation");
    remember_public_mutation_command(
        &mut seen_commands,
        &reopen_status,
        reopen_status
            .recommended_public_command_argv
            .as_deref()
            .expect("test reopen command should materialize typed argv"),
    )
    .expect("first reopen after repair-review-state should be accepted and replayed");
    assert!(
        remember_public_mutation_command(
            &mut seen_commands,
            &repeated_reopen_status,
            repeated_reopen_status
                .recommended_public_command_argv
                .as_deref()
                .expect("test repeated reopen should materialize typed argv")
        )
        .is_err(),
        "bounded liveness replay must fail repair-review-state -> reopen -> same reopen loops"
    );
}

#[test]
fn close_current_task_is_a_real_recording_boundary_for_liveness() {
    let argv = exact_test_public_argv(&[
        "featureforge",
        "plan",
        "execution",
        "close-current-task",
        "--plan",
        "docs/plan.md",
        "--task",
        "1",
    ]);
    let mut status = minimal_liveness_status(
        "task_closure_recording_ready",
        vec![String::from("task_closure_baseline_repair_candidate")],
        None,
    );
    status.state_kind = String::from("waiting_external_input");
    status.next_action = String::from("close current task");
    status.required_inputs = task_closure_required_inputs();

    assert!(
        status_missing_public_inputs(&status),
        "close-current-task must expose typed required_inputs when external review or verification values are absent"
    );
    assert!(
        status.recommended_public_command_argv.is_none(),
        "missing external values must suppress exact public argv"
    );
    assert!(
        display_status_command(&status).is_none(),
        "the liveness external-input fixture must not model missing values with executable-looking display templates"
    );
    assert!(
        command_requires_external_result_input(&status, &argv),
        "close-current-task records review and verification results, so bounded runtime-management liveness should stop at that real-work boundary"
    );
    assert!(
        !is_runtime_management_mutation_command(&status, &argv),
        "close-current-task must not be classified as runtime-management churn"
    );
}

#[test]
fn targetless_stale_guard_rejects_fabricated_current_task_targets() {
    let synthetic = SyntheticState {
        completed_tasks: 2,
        targetless_stale_present: true,
        ..SyntheticState::base(2)
    };
    let clean_status = minimal_liveness_status("execution_reentry_required", Vec::new(), None);
    assert_targetless_stale_does_not_fabricate_current_task(
        "targetless-stale clean diagnostic",
        synthetic,
        &clean_status,
    );

    let mut stale_ids = clean_status.clone();
    stale_ids.stale_unreviewed_closures = vec![String::from("task-2")];
    assert!(
        std::panic::catch_unwind(|| {
            assert_targetless_stale_does_not_fabricate_current_task(
                "targetless-stale fabricated stale id",
                synthetic,
                &stale_ids,
            );
        })
        .is_err(),
        "targetless stale guard must reject fabricated stale closure ids"
    );

    let mut blocking_task = clean_status.clone();
    blocking_task.blocking_task = Some(2);
    assert!(
        std::panic::catch_unwind(|| {
            assert_targetless_stale_does_not_fabricate_current_task(
                "targetless-stale fabricated blocking task",
                synthetic,
                &blocking_task,
            );
        })
        .is_err(),
        "targetless stale guard must reject fabricated blocking_task current targets"
    );

    let mut resume_task = clean_status.clone();
    resume_task.resume_task = Some(2);
    assert!(
        std::panic::catch_unwind(|| {
            assert_targetless_stale_does_not_fabricate_current_task(
                "targetless-stale fabricated resume task",
                synthetic,
                &resume_task,
            );
        })
        .is_err(),
        "targetless stale guard must reject fabricated resume_task current targets"
    );

    let mut execution_context = clean_status;
    execution_context.execution_command_context = Some(LivenessExecutionCommandContext {
        task_number: Some(2),
    });
    assert!(
        std::panic::catch_unwind(|| {
            assert_targetless_stale_does_not_fabricate_current_task(
                "targetless-stale fabricated execution context",
                synthetic,
                &execution_context,
            );
        })
        .is_err(),
        "targetless stale guard must reject fabricated execution command context current targets"
    );
}

#[test]
fn same_current_reopen_guard_uses_exact_public_argv_lanes() {
    let synthetic = SyntheticState {
        completed_tasks: 2,
        ..SyntheticState::base(2)
    };
    let argv = exact_test_public_argv(&[
        "featureforge",
        "plan",
        "execution",
        "reopen",
        "--plan",
        "docs/plan.md",
        "--task",
        "2",
        "--step",
        "1",
    ]);
    let command = exact_public_argv_display(&argv);

    let mut display_only_next_action =
        minimal_liveness_status("execution_reentry_required", Vec::new(), None);
    display_only_next_action.next_public_action = Some(LivenessNextPublicAction {
        command: command.clone(),
    });
    assert!(
        materialized_status_command(&display_only_next_action).is_none(),
        "display-only next_public_action must not become an executable public command"
    );

    let exact_next_action =
        minimal_liveness_status("execution_reentry_required", Vec::new(), Some(argv.clone()));
    let effective = materialized_status_command(&exact_next_action)
        .expect("recommended_public_command_argv should be the effective public command");
    assert_eq!(effective, command);
    assert!(
        std::panic::catch_unwind(|| {
            assert_reopen_liveness_contract(
                "effective next_public_action reopen",
                synthetic,
                exact_next_action
                    .recommended_public_command_argv
                    .as_deref()
                    .expect("exact next-action argv should exist"),
            );
        })
        .is_err(),
        "same-current reopen guard must reject next_public_action reopen loops"
    );

    let mut display_only_blocker =
        minimal_liveness_status("execution_reentry_required", Vec::new(), None);
    display_only_blocker.blockers = vec![LivenessBlocker {
        category: String::from("stale"),
        next_public_action: Some(LivenessNextPublicAction {
            command: command.clone(),
        }),
    }];
    assert!(
        materialized_status_command(&display_only_blocker).is_none(),
        "display-only blocker action must not become an executable public command"
    );

    let exact_blocker =
        minimal_liveness_status("execution_reentry_required", Vec::new(), Some(argv));
    let effective = materialized_status_command(&exact_blocker)
        .expect("recommended_public_command_argv should be the effective blocker command");
    assert_eq!(effective, command);
    assert!(
        std::panic::catch_unwind(|| {
            assert_reopen_liveness_contract(
                "effective blocker reopen",
                synthetic,
                exact_blocker
                    .recommended_public_command_argv
                    .as_deref()
                    .expect("exact blocker argv should exist"),
            );
        })
        .is_err(),
        "same-current reopen guard must reject blocker-action reopen loops"
    );
}

#[test]
fn multistep_reopen_guard_rejects_step_or_source_divergence() {
    let synthetic = SyntheticState {
        completed_tasks: 2,
        steps_per_task: 2,
        stale_boundary_present: true,
        ..SyntheticState::base(2)
    };
    let step_one_reopen = exact_test_public_argv(&[
        "featureforge",
        "plan",
        "execution",
        "reopen",
        "--plan",
        "docs/plan.md",
        "--task",
        "2",
        "--step",
        "1",
        "--source",
        "featureforge:executing-plans",
    ]);
    assert!(
        std::panic::catch_unwind(|| {
            assert_reopen_liveness_contract(
                "multistep stale reopen step regression",
                synthetic,
                &step_one_reopen,
            );
        })
        .is_err(),
        "multistep reopen guard must reject stale routes that fall back to step 1"
    );
    let wrong_source_reopen = exact_test_public_argv(&[
        "featureforge",
        "plan",
        "execution",
        "reopen",
        "--plan",
        "docs/plan.md",
        "--task",
        "2",
        "--step",
        "2",
        "--source",
        "featureforge:subagent-driven-development",
    ]);
    assert!(
        std::panic::catch_unwind(|| {
            assert_reopen_liveness_contract(
                "multistep stale reopen source regression",
                synthetic,
                &wrong_source_reopen,
            );
        })
        .is_err(),
        "multistep reopen guard must reject stale routes that bypass shared execution-source derivation"
    );
    let shared_exact_reopen = exact_test_public_argv(&[
        "featureforge",
        "plan",
        "execution",
        "reopen",
        "--plan",
        "docs/plan.md",
        "--task",
        "2",
        "--step",
        "2",
        "--source",
        "featureforge:executing-plans",
    ]);
    assert_reopen_liveness_contract(
        "multistep stale reopen exact command",
        synthetic,
        &shared_exact_reopen,
    );
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
        SyntheticState {
            steps_per_task: 2,
            stale_boundary_present: true,
            downstream_stale_step_present: true,
            downstream_interrupted_projection_present: true,
            ..SyntheticState::base(active_completed)
        },
    ]
}

fn runtime_executed_liveness_case_is_representative(total_tasks: u8, case: SyntheticState) -> bool {
    let active_completed = total_tasks.saturating_sub(1).max(1);
    if [
        SyntheticState::base(0),
        SyntheticState::base(active_completed),
        SyntheticState {
            stale_boundary_present: true,
            ..SyntheticState::base(active_completed)
        },
        SyntheticState {
            stale_boundary_present: true,
            resume_exact_disagreement_present: true,
            ..SyntheticState::base(active_completed)
        },
    ]
    .contains(&case)
    {
        return true;
    }

    case.completed_tasks == active_completed
        && ((case.current_stale_overlap_present
            && case
                == SyntheticState {
                    current_stale_overlap_present: true,
                    ..SyntheticState::base(active_completed)
                })
            || (case.stale_cycle_break_overlay_present
                && case
                    == SyntheticState {
                        stale_cycle_break_overlay_present: true,
                        ..SyntheticState::base(active_completed)
                    })
            || (case.targetless_stale_present
                && case
                    == SyntheticState {
                        targetless_stale_present: true,
                        current_branch_closure_null: true,
                        ..SyntheticState::base(active_completed)
                    })
            || (case.downstream_stale_step_present
                && case
                    == SyntheticState {
                        stale_boundary_present: true,
                        downstream_stale_step_present: true,
                        ..SyntheticState::base(active_completed)
                    })
            || (case.downstream_interrupted_projection_present
                && case
                    == SyntheticState {
                        steps_per_task: 2,
                        stale_boundary_present: true,
                        downstream_interrupted_projection_present: true,
                        ..SyntheticState::base(active_completed)
                    })
            || (case.downstream_stale_step_present
                && case.downstream_interrupted_projection_present
                && case
                    == SyntheticState {
                        steps_per_task: 2,
                        stale_boundary_present: true,
                        downstream_stale_step_present: true,
                        downstream_interrupted_projection_present: true,
                        ..SyntheticState::base(active_completed)
                    }))
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
        assert!(
            cases.iter().any(|case| {
                case.stale_boundary_present
                    && case.downstream_stale_step_present
                    && case.downstream_interrupted_projection_present
            }),
            "missing earlier stale boundary plus later stale/interrupted overlay variant for {total_tasks} tasks"
        );
        let executed_cases = cases
            .iter()
            .filter(|case| runtime_executed_liveness_case_is_representative(total_tasks, **case))
            .count();
        assert!(
            executed_cases >= 9 && executed_cases < cases.len(),
            "runtime-executed liveness replay should keep a representative but bounded case set for {total_tasks} tasks; got {executed_cases} of {}",
            cases.len()
        );
    }
}

fn assert_runtime_executed_production_loop_case(
    label: &str,
    mut matches_case: impl FnMut(&SyntheticState) -> bool,
) {
    let cases = synthetic_liveness_cases(3);
    assert!(
        cases.iter().copied().any(|case| {
            runtime_executed_liveness_case_is_representative(3, case) && matches_case(&case)
        }),
        "{label} must be present in the runtime-executed representative liveness set"
    );
}

fn assert_fixture_only_production_loop_case_is_modeled(
    label: &str,
    matches_case: impl FnMut(&SyntheticState) -> bool,
) {
    let cases = synthetic_liveness_cases(3);
    assert!(
        cases.iter().any(matches_case),
        "{label} must be present in the fixture-only liveness model case set"
    );
}

#[test]
fn liveness_current_stale_overlap_blocks_without_reopen_or_hidden_command() {
    assert_runtime_executed_production_loop_case("current/stale closure overlap", |case| {
        case.completed_tasks > 0 && case.current_stale_overlap_present
    });
}

#[test]
fn liveness_already_current_cycle_break_clears_or_routes_forward() {
    assert_runtime_executed_production_loop_case("FS-01 already-current cycle-break", |case| {
        case.completed_tasks > 0 && case.stale_cycle_break_overlay_present
    });
}

#[test]
fn liveness_stale_unreviewed_without_target_diagnostics_not_reopen() {
    assert_runtime_executed_production_loop_case("FS-02 targetless stale diagnostic", |case| {
        case.targetless_stale_present && !case.stale_boundary_present
    });
}

#[test]
fn fixture_model_includes_orphan_late_stage_history_case() {
    assert_fixture_only_production_loop_case_is_modeled(
        "FS-03 orphan late-stage history",
        |case| case.current_branch_closure_null && case.orphan_milestone_records_present,
    );
}

#[test]
fn fixture_model_includes_projection_dirty_preflight_case() {
    assert_fixture_only_production_loop_case_is_modeled(
        "FS-04 projection-only dirtiness",
        |case| case.runtime_projection_dirtiness_present,
    );
}

#[test]
fn fixture_model_includes_summary_hash_drift_case() {
    assert_fixture_only_production_loop_case_is_modeled("FS-05 summary-hash drift", |case| {
        case.summary_hash_drift_present
    });
}

#[test]
fn liveness_downstream_stale_step_after_prior_closure_routes_to_downstream_not_prior_task() {
    assert_runtime_executed_production_loop_case("downstream stale step", |case| {
        case.stale_boundary_present && case.downstream_stale_step_present
    });
}

#[test]
fn fixture_model_includes_downstream_stale_with_projection_churn_case() {
    assert_fixture_only_production_loop_case_is_modeled(
        "downstream stale with projection churn",
        |case| {
            case.stale_boundary_present
                && case.downstream_stale_step_present
                && case.runtime_projection_dirtiness_present
        },
    );
}

#[test]
fn fixture_model_includes_token_only_repair_follow_up_case() {
    assert_fixture_only_production_loop_case_is_modeled("token-only repair follow-up", |case| {
        case.targetless_stale_present && !case.stale_boundary_present
    });
}

#[test]
fn liveness_resume_task_disagreement_never_overrides_exact_legal_command() {
    assert_runtime_executed_production_loop_case("resume/exact-command disagreement", |case| {
        case.resume_exact_disagreement_present
    });
}

#[test]
fn liveness_nested_interruption_does_not_strand_earliest_stale_boundary() {
    assert_runtime_executed_production_loop_case("nested interruption", |case| {
        case.steps_per_task > 1 && case.downstream_interrupted_projection_present
    });
}

#[test]
fn liveness_earlier_stale_boundary_preempts_later_stale_interrupted_overlay() {
    assert_runtime_executed_production_loop_case(
        "earlier stale plus later stale/interrupted overlay",
        |case| {
            case.steps_per_task > 1
                && case.stale_boundary_present
                && case.downstream_stale_step_present
                && case.downstream_interrupted_projection_present
        },
    );
}

#[test]
fn runtime_executed_liveness_representatives_cover_critical_stuck_state_families() {
    let cases = synthetic_liveness_cases(3)
        .into_iter()
        .filter(|case| runtime_executed_liveness_case_is_representative(3, *case))
        .collect::<Vec<_>>();
    let required: [RequiredSyntheticCase<'_>; 6] = [
        ("current/stale overlap", |case: &SyntheticState| {
            case.current_stale_overlap_present
        }),
        ("cycle-break", |case: &SyntheticState| {
            case.stale_cycle_break_overlay_present
        }),
        ("targetless stale", |case: &SyntheticState| {
            case.targetless_stale_present && !case.stale_boundary_present
        }),
        ("downstream stale", |case: &SyntheticState| {
            case.stale_boundary_present
                && case.downstream_stale_step_present
                && !case.downstream_interrupted_projection_present
        }),
        ("downstream interruption", |case: &SyntheticState| {
            case.stale_boundary_present
                && case.downstream_interrupted_projection_present
                && !case.downstream_stale_step_present
        }),
        (
            "downstream stale plus interruption",
            |case: &SyntheticState| {
                case.steps_per_task > 1
                    && case.stale_boundary_present
                    && case.downstream_stale_step_present
                    && case.downstream_interrupted_projection_present
            },
        ),
    ];
    for (label, matches_case) in required {
        assert!(
            cases.iter().any(matches_case),
            "runtime-executed liveness representatives must include {label}; representatives={cases:?}"
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

    for &total_tasks in RUNTIME_EXECUTED_LIVENESS_TASK_COUNTS {
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
            let mut worktree_requires_restore = false;
            for synthetic in cases {
                if !runtime_executed_liveness_case_is_representative(total_tasks, synthetic) {
                    continue;
                }
                if worktree_requires_restore {
                    write_spec_and_plan(repo, total_tasks, completed_tasks, steps_per_task);
                }
                write_variant_harness_state(&fixture, synthetic);
                apply_synthetic_worktree_variants(repo, synthetic);
                worktree_requires_restore = synthetic.runtime_projection_dirtiness_present;
                synthetic_migrate_variant_to_event_authority(
                    &state_path,
                    "liveness synthetic authority migration",
                );

                let label = format!(
                    "runtime-liveness total_tasks={total_tasks} completed_tasks={completed_tasks} synthetic={synthetic:?}",
                );
                let status = semantic_status_value(&runtime, &label);
                let state_kind = status.state_kind.as_str();
                let public_command = materialized_status_command(&status);
                assert_targetless_stale_does_not_fabricate_current_task(&label, synthetic, &status);
                assert_earliest_stale_boundary_preempts_later_overlay(&label, synthetic, &status);

                if state_kind == "terminal" {
                    assert!(
                        public_command.is_none(),
                        "terminal states must not expose a public command: {status:?}"
                    );
                    continue;
                }

                if state_kind == "waiting_external_input" {
                    assert!(
                        public_command.is_none(),
                        "waiting states must not expose a public command: {status:?}"
                    );
                    assert!(
                        status_missing_public_inputs(&status)
                            || display_status_command(&status).is_none(),
                        "waiting states with display-only commands must expose typed required_inputs: {status:?}"
                    );
                    continue;
                }

                if targetless_stale_reconcile_status(&status) {
                    assert_diagnostic_status_shape(&label, &status);
                    assert!(
                        public_command.is_none(),
                        "targetless stale reconcile must not synthesize a repair or reopen loop: {status:?}"
                    );
                    continue;
                }

                if blocked_runtime_bug_diagnostic_status(&status) {
                    assert_diagnostic_status_shape(&label, &status);
                    let public_output = format!("{status:?}");
                    assert!(
                        public_command.is_none(),
                        "{label} blocked_runtime_bug must be diagnostic-only: {status:?}"
                    );
                    assert!(
                        !contains_hidden_command_token(&public_output),
                        "{label} blocked_runtime_bug diagnostic must not expose hidden helper lanes: {status:?}"
                    );
                    continue;
                }

                if current_stale_overlap_runtime_diagnostic(&status) {
                    assert_diagnostic_status_shape(&label, &status);
                    let public_output = format!("{status:?}");
                    assert!(
                        public_command.is_none(),
                        "current/stale overlap diagnostic must not synthesize an unsafe mutation: {status:?}"
                    );
                    assert!(
                        !public_output.contains(" reopen ")
                            && !contains_hidden_command_token(&public_output),
                        "current/stale overlap diagnostic must not expose reopen or hidden helper lanes: {status:?}"
                    );
                    continue;
                }

                if is_diagnostic_status(&status) {
                    assert_diagnostic_status_shape(&label, &status);
                    continue;
                }

                if status_missing_public_inputs(&status) {
                    assert!(
                        public_command.is_none(),
                        "{label} missing-input states must not expose executable argv: {status:?}"
                    );
                    continue;
                }

                if public_command.is_some() {
                    let command_argv = status
                        .recommended_public_command_argv
                        .as_deref()
                        .expect("materialized command should come from typed argv");
                    assert_public_command_is_safe(&label, command_argv);
                    assert_no_same_current_task_reopen_without_substantive_work(
                        &label,
                        synthetic,
                        command_argv,
                    );
                } else {
                    panic!(
                        "actionable states must surface a concrete public-action lane: {status:?}"
                    );
                }
                let before = progress_metric_from_status(&status, total_tasks);
                let successor = execute_semantic_public_progress_edge(&runtime, &status)
                    .unwrap_or_else(|error| {
                        panic!("{label} should execute semantic public argv edge: {error}")
                    });
                let successor = successor.unwrap_or_else(|| {
                    panic!("{label} should execute a concrete semantic public argv edge from {status:?}")
                });
                let after = progress_metric_from_status(&successor, total_tasks);
                assert!(
                    public_edge_satisfies_liveness_contract(&status, before, &successor, after),
                    "semantic public argv edge must reduce the metric, expose a different true blocker, emit a deterministic diagnostic, or resolve already-current state; before={before:?} after={after:?} status={status:?} successor={successor:?}"
                );
                if status
                    .recommended_public_command_argv
                    .as_deref()
                    .is_some_and(is_repair_review_state_command)
                {
                    assert_repair_review_state_successor_contract(&label, &status, &successor);
                }
                let bounded_context = BoundedLivenessContext {
                    runtime: &runtime,
                    total_tasks,
                    label: &label,
                    synthetic,
                };
                assert_bounded_runtime_management_path_converges(
                    &bounded_context,
                    &status,
                    &successor,
                    status.recommended_public_command_argv.as_deref(),
                );
                executed_successor_edges = executed_successor_edges.saturating_add(1);
            }
        }
    }
    assert!(
        executed_successor_edges >= 3,
        "liveness checker must execute a meaningful set of semantic public argv successor edges, got {executed_successor_edges}",
    );
}

#[test]
fn liveness_semantic_public_argv_runner_matches_shipped_cli_for_sampled_edge() {
    let (semantic_repo_dir, semantic_state_dir) =
        workflow_support::init_repo("runtime-liveness-semantic-parity");
    let (shipped_repo_dir, shipped_state_dir) =
        workflow_support::init_repo("runtime-liveness-shipped-parity");
    let semantic_repo = semantic_repo_dir.path();
    let shipped_repo = shipped_repo_dir.path();
    let semantic_state = semantic_state_dir.path();
    let shipped_state = shipped_state_dir.path();
    let synthetic = SyntheticState::base(0);

    prepare_synthetic_liveness_fixture(
        semantic_repo,
        semantic_state,
        1,
        0,
        1,
        synthetic,
        "liveness semantic parity fixture migration",
    );
    prepare_synthetic_liveness_fixture(
        shipped_repo,
        shipped_state,
        1,
        0,
        1,
        synthetic,
        "liveness shipped CLI parity fixture migration",
    );

    let semantic_runtime = runtime_for_status(semantic_repo, semantic_state);
    let shipped_runtime = runtime_for_status(shipped_repo, shipped_state);
    let semantic_before =
        semantic_status_value(&semantic_runtime, "liveness semantic parity before edge");
    let shipped_before =
        shipped_cli_status_value(&shipped_runtime, "liveness shipped CLI parity before edge");
    assert_eq!(
        route_signature(&semantic_before),
        route_signature(&shipped_before),
        "semantic in-process and shipped CLI status should agree before the sampled public edge"
    );

    let command = exact_public_command_from_status(&semantic_before)
        .expect("sampled parity status should expose exact typed public argv");
    assert_eq!(
        command.kind,
        LivenessPublicCommandKind::PlanExecutionBegin,
        "sampled parity edge should cover a side-effecting begin mutation, got {:?}",
        command.argv
    );
    assert_public_command_is_safe("liveness shipped CLI parity sampled edge", &command.argv);
    execute_exact_public_command_with_runner(
        &semantic_runtime,
        &semantic_before,
        &command.argv,
        LivenessRuntimeRunner::SemanticInProcess,
    )
    .unwrap_or_else(|error| panic!("semantic sampled public edge should execute: {error}"));
    execute_exact_public_command_with_runner(
        &shipped_runtime,
        &shipped_before,
        &command.argv,
        LivenessRuntimeRunner::ShippedCli,
    )
    .unwrap_or_else(|error| panic!("shipped CLI sampled public edge should execute: {error}"));

    let semantic_after =
        semantic_status_value(&semantic_runtime, "liveness semantic parity after edge");
    let shipped_after =
        shipped_cli_status_value(&shipped_runtime, "liveness shipped CLI parity after edge");
    assert_eq!(
        route_signature(&semantic_after),
        route_signature(&shipped_after),
        "semantic in-process and shipped CLI status should agree after the sampled public edge"
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
        synthetic_migrate_variant_to_event_authority(
            &state_path,
            "liveness hidden-check authority migration",
        );

        let status = semantic_status_value(
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
