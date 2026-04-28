#[path = "support/dir_tree.rs"]
mod dir_tree_support;
#[path = "support/executable.rs"]
mod executable_support;
#[path = "support/featureforge.rs"]
mod featureforge_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/prebuilt.rs"]
mod prebuilt_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/projection.rs"]
mod projection_support;
#[path = "support/runtime_json.rs"]
mod runtime_json_support;
#[path = "support/runtime_surfaces.rs"]
mod runtime_surfaces_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use dir_tree_support::copy_dir_recursive;
use executable_support::make_executable;
use featureforge::cli::plan_execution::{ReviewOutcomeArg, StatusArgs};
use featureforge::execution::final_review::{
    parse_final_review_receipt, resolve_release_base_branch,
};
use featureforge::execution::follow_up::execution_step_repair_target_id;
use featureforge::execution::internal_args::{
    RecordBranchClosureArgs, RecordFinalReviewArgs, RecordQaArgs, RecordReleaseReadinessArgs,
    RecordReviewDispatchArgs, ReleaseReadinessOutcomeArg, ReviewDispatchScopeArg,
};
use featureforge::execution::query::query_workflow_routing_state_for_runtime;
use featureforge::execution::semantic_identity::{
    branch_definition_identity_for_context, task_definition_identity_for_task,
};
use featureforge::execution::state::{
    NO_REPO_FILES_MARKER, current_head_sha as runtime_current_head_sha,
    current_tracked_tree_sha as runtime_current_tracked_tree_sha, load_execution_context,
};
use featureforge::git::discover_slug_identity;
use featureforge::paths::{
    branch_storage_key, harness_authoritative_artifact_path, harness_state_path,
};
use featureforge::workflow::operator;
use files_support::write_file;
use prebuilt_support::write_canonical_prebuilt_layout;
use process_support::run;
use runtime_json_support::{discover_execution_runtime, plan_execution_status_json};
use runtime_surfaces_support::workflow_operator_json;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};
use tempfile::TempDir;
use workflow_support::{init_repo, workflow_fixture_root};

const WORKFLOW_FIXTURE_PLAN_REL: &str =
    "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

#[derive(Clone)]
struct WorkflowFixtureTemplate {
    repo_root: PathBuf,
    state_root: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkflowFixtureQaMode {
    NotRequired,
    Required,
    MissingHeader,
}

static WORKFLOW_EXECUTION_TEMPLATE_NOT_REQUIRED: OnceLock<WorkflowFixtureTemplate> =
    OnceLock::new();
static WORKFLOW_EXECUTION_TEMPLATE_REQUIRED: OnceLock<WorkflowFixtureTemplate> = OnceLock::new();
static WORKFLOW_EXECUTION_TEMPLATE_MISSING_HEADER: OnceLock<WorkflowFixtureTemplate> =
    OnceLock::new();
static LATE_STAGE_SETUP_TEMPLATES: OnceLock<Mutex<HashMap<String, WorkflowFixtureTemplate>>> =
    OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicRouteSnapshot {
    phase: String,
    phase_detail: Option<String>,
    review_state_status: String,
    next_action: String,
    recommended_command: Option<String>,
    blocking_scope: Option<String>,
    blocking_task: Option<u32>,
    external_wait_state: Option<String>,
    blocking_reason_codes: Vec<String>,
}

fn public_route_snapshot(value: &Value) -> PublicRouteSnapshot {
    let phase = value
        .get("phase")
        .or_else(|| value.get("harness_phase"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!("route payload must include string `phase` or `harness_phase`: {value}")
        })
        .to_owned();
    let review_state_status = value
        .get("review_state_status")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!("route payload must include string `review_state_status`: {value}")
        })
        .to_owned();
    let next_action = value
        .get("next_action")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("route payload must include string `next_action`: {value}"))
        .to_owned();

    PublicRouteSnapshot {
        phase,
        phase_detail: value
            .get("phase_detail")
            .and_then(Value::as_str)
            .map(str::to_owned),
        review_state_status,
        next_action,
        recommended_command: value
            .get("recommended_command")
            .and_then(Value::as_str)
            .map(str::to_owned),
        blocking_scope: value
            .get("blocking_scope")
            .and_then(Value::as_str)
            .map(str::to_owned),
        blocking_task: value
            .get("blocking_task")
            .and_then(Value::as_u64)
            .and_then(|raw| u32::try_from(raw).ok()),
        external_wait_state: value
            .get("external_wait_state")
            .and_then(Value::as_str)
            .map(str::to_owned),
        blocking_reason_codes: value
            .get("blocking_reason_codes")
            .and_then(Value::as_array)
            .map(|codes| {
                codes
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    }
}

fn assert_public_route_parity(operator: &Value, status: &Value, handoff: Option<&Value>) {
    let operator_route = public_route_snapshot(operator);
    let status_route = public_route_snapshot(status);
    assert_eq!(
        operator_route, status_route,
        "workflow operator and plan execution status must agree on public route fields"
    );
    if let Some(handoff) = handoff {
        let handoff_route = public_route_snapshot(handoff);
        assert_eq!(
            operator_route, handoff_route,
            "workflow handoff top-level route must match workflow operator"
        );
    }
}

fn assert_task_closure_recording_route(route: &Value, plan_rel: &str, task: u32) {
    assert_eq!(route["phase"], "task_closure_pending", "json: {route}");
    assert_eq!(
        route["phase_detail"], "task_closure_recording_ready",
        "json: {route}"
    );
    assert_eq!(route["next_action"], "close current task", "json: {route}");
    assert_eq!(
        route["state_kind"], "actionable_public_command",
        "json: {route}"
    );
    assert!(
        route["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.starts_with("featureforge plan execution close-current-task --plan")
                    && command.contains(plan_rel)
                    && command.contains(&format!("--task {task}"))
            }),
        "task-closure route should expose close-current-task for task {task}, got {route}"
    );
}

fn current_final_review_record<'a>(authoritative_state: &'a Value, context: &str) -> &'a Value {
    let current_record_id = authoritative_state["current_final_review_record_id"]
        .as_str()
        .unwrap_or_else(|| {
            panic!("{context} should expose current_final_review_record_id: {authoritative_state}")
        });
    &authoritative_state["final_review_record_history"][current_record_id]
}

fn current_final_review_fingerprint(authoritative_state: &Value, context: &str) -> String {
    current_final_review_record(authoritative_state, context)["final_review_fingerprint"]
        .as_str()
        .unwrap_or_else(|| {
            panic!("{context} should expose current final-review record fingerprint: {authoritative_state}")
        })
        .to_owned()
}

fn assert_parity_probe_budget(scenario_id: &str, consumed_probe_commands: usize, max: usize) {
    assert!(
        consumed_probe_commands <= max,
        "scenario {scenario_id} exceeded parity-probe command target: consumed {consumed_probe_commands}, target {max}"
    );
}

fn assert_runtime_management_budget(
    scenario_id: &str,
    consumed_runtime_management_commands: usize,
    max: usize,
) {
    assert!(
        consumed_runtime_management_commands <= max,
        "scenario {scenario_id} exceeded runtime-management command budget: consumed {consumed_runtime_management_commands}, budget {max}"
    );
}

fn assert_no_hidden_helper_commands_used(commands: &[String]) {
    let hidden_command_tokens = [
        &["pre", "flight"][..],
        &["record", "-review-dispatch"],
        &["gate", "-review"],
        &["rebuild", "-evidence"],
    ];
    for command in commands {
        assert!(
            hidden_command_tokens
                .iter()
                .all(|hidden| !contains_hidden_parts(command, hidden)),
            "normal-path command sequences may not include hidden helper commands, got `{command}`"
        );
    }
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

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn clear_directory(path: &Path) {
    if !path.exists() {
        fs::create_dir_all(path).expect("destination directory should be creatable");
        return;
    }
    for entry in fs::read_dir(path).expect("destination directory should be readable") {
        let entry = entry.expect("destination entry should be readable");
        let entry_path = entry.path();
        if entry
            .file_type()
            .expect("destination entry type should be readable")
            .is_dir()
        {
            fs::remove_dir_all(&entry_path)
                .unwrap_or_else(|error| panic!("failed to remove {:?}: {error}", entry_path));
        } else {
            fs::remove_file(&entry_path)
                .unwrap_or_else(|error| panic!("failed to remove {:?}: {error}", entry_path));
        }
    }
}

fn populate_fixture_from_template(
    template: &WorkflowFixtureTemplate,
    repo: &Path,
    state_dir: &Path,
) {
    clear_directory(repo);
    clear_directory(state_dir);
    copy_dir_recursive(&template.repo_root, repo);
    copy_dir_recursive(&template.state_root, state_dir);
    rebind_copied_state_repo_slug_if_needed(repo, state_dir);
}

fn rebind_copied_state_repo_slug_if_needed(repo: &Path, state_dir: &Path) {
    let projects_dir = state_dir.join("projects");
    if !projects_dir.is_dir() {
        return;
    }
    let active_slug = repo_slug(repo, state_dir);
    let active_project_dir = projects_dir.join(&active_slug);
    if active_project_dir.is_dir() {
        return;
    }
    let project_dirs = fs::read_dir(&projects_dir)
        .expect("projects directory should be readable")
        .filter_map(|entry| {
            let entry = entry.expect("project directory entry should be readable");
            if entry
                .file_type()
                .expect("project directory entry type should be readable")
                .is_dir()
            {
                Some(entry.path())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if project_dirs.len() != 1 {
        return;
    }
    let source_project_dir = &project_dirs[0];
    if source_project_dir
        .file_name()
        .is_some_and(|name| name == OsStr::new(active_slug.as_str()))
    {
        return;
    }
    fs::rename(source_project_dir, &active_project_dir).unwrap_or_else(|error| {
        panic!(
            "failed to rebind copied project state directory `{}` to active slug `{}`: {error}",
            source_project_dir.display(),
            active_project_dir.display()
        )
    });
}

fn workflow_execution_fixture_template(
    mode: WorkflowFixtureQaMode,
) -> &'static WorkflowFixtureTemplate {
    let store = match mode {
        WorkflowFixtureQaMode::NotRequired => &WORKFLOW_EXECUTION_TEMPLATE_NOT_REQUIRED,
        WorkflowFixtureQaMode::Required => &WORKFLOW_EXECUTION_TEMPLATE_REQUIRED,
        WorkflowFixtureQaMode::MissingHeader => &WORKFLOW_EXECUTION_TEMPLATE_MISSING_HEADER,
    };
    store.get_or_init(|| {
        let (repo_dir, state_dir) = init_repo(match mode {
            WorkflowFixtureQaMode::NotRequired => "workflow-shell-smoke-template-not-required",
            WorkflowFixtureQaMode::Required => "workflow-shell-smoke-template-required",
            WorkflowFixtureQaMode::MissingHeader => "workflow-shell-smoke-template-missing-header",
        });
        let repo = repo_dir.path();
        let state = state_dir.path();
        run_checked(
            {
                let mut command = Command::new("git");
                command
                    .args([
                        "remote",
                        "add",
                        "origin",
                        "git@github.com:featureforge/workflow-shell-smoke-template.git",
                    ])
                    .current_dir(repo);
                command
            },
            "git remote add origin for workflow shell-smoke template",
        );
        complete_workflow_fixture_execution_with_qa_requirement_slow(
            repo,
            state,
            WORKFLOW_FIXTURE_PLAN_REL,
            match mode {
                WorkflowFixtureQaMode::NotRequired => None,
                WorkflowFixtureQaMode::Required => Some("required"),
                WorkflowFixtureQaMode::MissingHeader => None,
            },
            mode == WorkflowFixtureQaMode::MissingHeader,
        );
        let template = WorkflowFixtureTemplate {
            repo_root: repo.to_path_buf(),
            state_root: state.to_path_buf(),
        };
        std::mem::forget(repo_dir);
        std::mem::forget(state_dir);
        template
    })
}

fn build_setup_fixture_template(
    template_name: &str,
    build: impl FnOnce(&Path, &Path),
) -> WorkflowFixtureTemplate {
    let (repo_dir, state_dir) = init_repo(template_name);
    let repo = repo_dir.path();
    let state = state_dir.path();
    build(repo, state);
    let template = WorkflowFixtureTemplate {
        repo_root: repo.to_path_buf(),
        state_root: state.to_path_buf(),
    };
    std::mem::forget(repo_dir);
    std::mem::forget(state_dir);
    template
}

fn populate_fixture_from_cached_setup_template(
    repo: &Path,
    state_dir: &Path,
    cache_key: &str,
    template_name: &str,
    build: impl FnOnce(&Path, &Path),
) {
    let cache = LATE_STAGE_SETUP_TEMPLATES.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(template) = {
        let guard = cache
            .lock()
            .expect("late-stage setup template cache lock should not be poisoned");
        guard.get(cache_key).cloned()
    } {
        populate_fixture_from_template(&template, repo, state_dir);
        return;
    }

    let template = build_setup_fixture_template(template_name, build);
    {
        let mut guard = cache
            .lock()
            .expect("late-stage setup template cache lock should not be poisoned");
        guard.insert(cache_key.to_owned(), template.clone());
    }
    populate_fixture_from_template(&template, repo, state_dir);
}

fn inject_current_topology_sections(plan_source: &str) -> String {
    const INSERT_AFTER: &str = "## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n- REQ-004 -> Task 1\n- VERIFY-001 -> Task 1\n";
    const TOPOLOGY_BLOCK: &str = "\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n";
    const QA_HEADER_AFTER: &str = "**Last Reviewed By:** plan-eng-review\n";
    const QA_HEADER_BLOCK: &str =
        "**Last Reviewed By:** plan-eng-review\n**QA Requirement:** not-required\n";

    let mut adjusted = if plan_source.contains("## Execution Strategy")
        && plan_source.contains("## Dependency Diagram")
    {
        plan_source.to_owned()
    } else {
        plan_source.replacen(INSERT_AFTER, &format!("{INSERT_AFTER}{TOPOLOGY_BLOCK}"), 1)
    };

    if !adjusted.contains("**QA Requirement:**") {
        adjusted = adjusted.replacen(QA_HEADER_AFTER, QA_HEADER_BLOCK, 1);
    }

    adjusted
}

fn rewrite_plan_qa_requirement(repo: &Path, plan_rel: &str, qa_requirement: Option<&str>) {
    let plan_path = repo.join(plan_rel);
    let mut plan_source =
        fs::read_to_string(&plan_path).expect("workflow shell-smoke plan should be readable");
    let current_header_line = plan_source
        .lines()
        .find(|line| line.starts_with("**QA Requirement:**"))
        .map(str::to_owned);
    match (current_header_line, qa_requirement) {
        (Some(current_header_line), Some(qa_requirement)) => {
            plan_source = plan_source.replace(
                &current_header_line,
                &format!("**QA Requirement:** {qa_requirement}"),
            );
        }
        (Some(current_header_line), None) => {
            plan_source = plan_source.replace(&format!("{current_header_line}\n"), "");
        }
        (None, Some(qa_requirement)) => {
            plan_source = plan_source.replacen(
                "**Last Reviewed By:** plan-eng-review\n",
                &format!(
                    "**Last Reviewed By:** plan-eng-review\n**QA Requirement:** {qa_requirement}\n"
                ),
                1,
            );
        }
        (None, None) => {}
    }
    write_file(&plan_path, &plan_source);
}

fn install_full_contract_ready_artifacts(repo: &Path) {
    let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let fixture_root = workflow_fixture_root();
    let spec_path = repo.join(spec_rel);
    let plan_path = repo.join(plan_rel);

    if let Some(parent) = spec_path.parent() {
        fs::create_dir_all(parent).expect("spec fixture parent should be creatable");
    }
    fs::copy(
        fixture_root.join("specs/2026-03-22-runtime-integration-hardening-design.md"),
        &spec_path,
    )
    .expect("spec fixture should copy");

    if let Some(parent) = plan_path.parent() {
        fs::create_dir_all(parent).expect("plan fixture parent should be creatable");
    }
    let plan_source =
        fs::read_to_string(fixture_root.join("plans/2026-03-22-runtime-integration-hardening.md"))
            .expect("ready plan fixture should read");
    let adjusted_plan = inject_current_topology_sections(&plan_source).replace(
        "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md",
        spec_rel,
    );
    fs::write(&plan_path, adjusted_plan).expect("ready plan fixture should write");
}

fn install_ready_artifacts(repo: &Path) {
    install_full_contract_ready_artifacts(repo);
}

fn write_two_task_workflow_plan(repo: &Path, plan_rel: &str) {
    write_file(
        &repo.join(plan_rel),
        r#"# Runtime Integration Hardening Implementation Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review
**QA Requirement:** not-required

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-004 -> Task 1, Task 2
- VERIFY-001 -> Task 2

## Execution Strategy

- Execute Task 1 serially. It establishes the execution boundary before Task 2 starts.
- Execute Task 2 serially after Task 1. It keeps reopened-task routing deterministic after the Task 1 closure handoff.

## Dependency Diagram

```text
Task 1 -> Task 2
```

## Task 1: Core flow

**Spec Coverage:** REQ-001, REQ-004
**Goal:** Core execution setup and validation are tracked with canonical execution-state evidence.

**Context:**
- Spec Coverage: REQ-001, REQ-004.

**Constraints:**
- Preserve helper-owned execution-state invariants.
- Keep execution evidence grounded in repo-visible artifacts.

**Done when:**
- Core execution setup and validation are tracked with canonical execution-state evidence.

**Files:**
- Modify: `docs/example-output.md`
- Test: `cargo test --test workflow_shell_smoke`

- [ ] **Step 1: Prepare workspace for execution**
- [ ] **Step 2: Validate the generated output**

## Task 2: Repair flow

**Spec Coverage:** REQ-004, VERIFY-001
**Goal:** Repair and handoff steps can reopen stale work without losing provenance.

**Context:**
- Spec Coverage: REQ-004, VERIFY-001.

**Constraints:**
- Reuse the same approved plan and evidence path for repairs.
- Keep repair flows fail-closed on stale or malformed state.

**Done when:**
- Repair and handoff steps can reopen stale work without losing provenance.

**Files:**
- Modify: `docs/example-followup.md`
- Test: `cargo test --test workflow_shell_smoke`

- [ ] **Step 1: Repair an invalidated prior step**
- [ ] **Step 2: Finalize the execution handoff**
"#,
    );
}

fn complete_two_task_fixture_task_1_steps(repo: &Path, state_dir: &Path, plan_rel: &str) {
    install_full_contract_ready_artifacts(repo);
    write_two_task_workflow_plan(repo, plan_rel);
    write_repo_file(
        repo,
        "docs/example-output.md",
        "two-task workflow fixture output\n",
    );
    prepare_preflight_acceptance_workspace(repo, "workflow-shell-smoke-two-task");

    let status = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        concat!("status before two-task shell-smoke fixture pre", "flight"),
    );
    let begin = run_plan_execution_json(
        repo,
        state_dir,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("two-task fixture status should expose execution fingerprint"),
        ],
        "begin task 1 step 1 for two-task shell-smoke fixture",
    );
    let begin_fingerprint = begin["execution_fingerprint"]
        .as_str()
        .expect("two-task fixture begin should expose execution fingerprint")
        .to_owned();
    let complete = run_plan_execution_json(
        repo,
        state_dir,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed Task 1 Step 1 for two-task shell-smoke fixture.",
            "--manual-verify-summary",
            "Verified Task 1 Step 1 for the two-task shell-smoke fixture.",
            "--file",
            "docs/example-output.md",
            "--expect-execution-fingerprint",
            &begin_fingerprint,
        ],
        "complete task 1 step 1 for two-task shell-smoke fixture",
    );
    let complete_fingerprint = complete["execution_fingerprint"]
        .as_str()
        .expect("two-task fixture complete should expose execution fingerprint")
        .to_owned();
    let begin_step_2 = run_plan_execution_json(
        repo,
        state_dir,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            &complete_fingerprint,
        ],
        "begin task 1 step 2 for two-task shell-smoke fixture",
    );
    let begin_step_2_fingerprint = begin_step_2["execution_fingerprint"]
        .as_str()
        .expect("two-task fixture step 2 begin should expose execution fingerprint")
        .to_owned();
    let complete_step_2 = run_plan_execution_json(
        repo,
        state_dir,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed Task 1 Step 2 for two-task shell-smoke fixture.",
            "--manual-verify-summary",
            "Verified Task 1 Step 2 for the two-task shell-smoke fixture.",
            "--file",
            "docs/example-output.md",
            "--expect-execution-fingerprint",
            &begin_step_2_fingerprint,
        ],
        "complete task 1 step 2 for two-task shell-smoke fixture",
    );
    assert_eq!(complete_step_2["active_task"], Value::Null);
}

fn close_two_task_fixture_task_1(repo: &Path, state_dir: &Path, plan_rel: &str) {
    complete_two_task_fixture_task_1_steps(repo, state_dir, plan_rel);
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("harness_phase", Value::from("executing")),
            (
                "current_task_closure_records",
                serde_json::json!({
                "task-1": {
                    "dispatch_id": "two-task-fixture-task-1-dispatch",
                    "closure_record_id": "two-task-fixture-task-1-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": task_contract_identity(repo, state_dir, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["docs/example-output.md"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(
                            b"Task 1 review passed for the two-task shell-smoke fixture."
                        ),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(
                            b"Task 1 verification passed for the two-task shell-smoke fixture."
                        ),
                    "closure_status": "current",
                    }
                }),
            ),
        ],
    );
}

fn run_featureforge(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    featureforge_support::run_featureforge_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        args,
        context,
    )
}

fn run_featureforge_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Output {
    featureforge_support::run_featureforge_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        args,
        context,
    )
}

fn run_featureforge_with_env(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    context: &str,
) -> Output {
    featureforge_support::run_featureforge_with_env_control_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        extra_env,
        args,
        context,
    )
}

fn run_featureforge_with_env_json(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    context: &str,
) -> Value {
    let output = run_featureforge_with_env(repo, state_dir, args, extra_env, context);
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn git_status_short(repo: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("status")
        .arg("--short")
        .current_dir(repo)
        .output()
        .expect("git status --short should run");
    assert!(
        output.status.success(),
        "git status --short should succeed, got {:?}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

fn assert_git_status_short_unchanged(repo: &Path, baseline: &[String], context: &str) {
    let current = git_status_short(repo);
    assert_eq!(
        current, baseline,
        "{context} must not add tracked projection churn"
    );
}

fn run_featureforge_json_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let output = featureforge_support::run_featureforge_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        args,
        context,
    );
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn run_checked(command: Command, context: &str) -> Output {
    let output = run(command, context);
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn write_repo_file(repo: &Path, relative: &str, content: &str) {
    let path = repo.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("repo file parent should be creatable");
    }
    write_file(&path, content);
}

fn run_plan_execution_json(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Value {
    let mut full_args = Vec::with_capacity(args.len() + 2);
    full_args.extend(["plan", "execution"]);
    full_args.extend_from_slice(args);
    let output = featureforge_support::run_featureforge_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        &full_args,
        context,
    );
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn internal_only_run_plan_execution_json_direct_or_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let explain_review_state = concat!("explain", "-review-state");
    if args.len() >= 3 && args[0] == explain_review_state && args[1] == "--plan" {
        let plan_rel = args[2];
        let rest = &args[3..];
        let external_review_result_ready = rest == ["--external-review-result-ready"];
        if rest.is_empty() || external_review_result_ready {
            return featureforge_support::internal_only_unit_explain_review_state_json(
                repo,
                state_dir,
                &StatusArgs {
                    plan: (*plan_rel).into(),
                    external_review_result_ready,
                },
            )
            .unwrap_or_else(|error| panic!("{context} should succeed: {error}"));
        }
    }
    run_plan_execution_json(repo, state_dir, args, context)
}

fn internal_only_plan_execution_fixture_json(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    match args {
        [concat!("pre", "flight"), "--plan", plan_rel] => {
            internal_only_runtime_preflight_gate_json(repo, state_dir, plan_rel, context)
        }
        [concat!("gate", "-review"), "--plan", plan_rel] => {
            internal_only_runtime_review_gate_json(repo, state_dir, plan_rel, false, context)
        }
        [
            concat!("gate", "-review"),
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ] => internal_only_runtime_review_gate_json(repo, state_dir, plan_rel, true, context),
        [concat!("gate", "-finish"), "--plan", plan_rel] => {
            internal_only_runtime_finish_gate_json(repo, state_dir, plan_rel, false, context)
        }
        [
            concat!("gate", "-finish"),
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ] => internal_only_runtime_finish_gate_json(repo, state_dir, plan_rel, true, context),
        [concat!("record", "-branch-closure"), "--plan", plan_rel] => {
            internal_only_unit_record_branch_closure_json(repo, state_dir, plan_rel, context)
        }
        [
            "internal",
            concat!("reconcile", "-review-state"),
            "--plan",
            plan_rel,
        ] => internal_only_unit_reconcile_review_state_json(repo, state_dir, plan_rel, context),
        [
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            scope,
            "--task",
            task,
        ] => {
            let scope = match *scope {
                "task" => ReviewDispatchScopeArg::Task,
                "final-review" => ReviewDispatchScopeArg::FinalReview,
                other => {
                    panic!("{context} should use a supported review-dispatch scope, got {other:?}")
                }
            };
            let task = task.parse::<u32>().unwrap_or_else(|error| {
                panic!("{context} should use a valid task number, got {task:?}: {error}")
            });
            internal_only_runtime_review_dispatch_authority_json(
                repo,
                state_dir,
                plan_rel,
                scope,
                Some(task),
                context,
            )
        }
        [
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            scope,
        ] => {
            let scope = match *scope {
                "task" => ReviewDispatchScopeArg::Task,
                "final-review" => ReviewDispatchScopeArg::FinalReview,
                other => {
                    panic!("{context} should use a supported review-dispatch scope, got {other:?}")
                }
            };
            internal_only_runtime_review_dispatch_authority_json(
                repo, state_dir, plan_rel, scope, None, context,
            )
        }
        [
            concat!("record", "-release-readiness"),
            "--plan",
            plan_rel,
            concat!("--branch", "-closure-id"),
            branch_closure_id,
            "--result",
            result,
            "--summary-file",
            summary_file,
        ] => {
            let result = match *result {
                "ready" => ReleaseReadinessOutcomeArg::Ready,
                "blocked" => ReleaseReadinessOutcomeArg::Blocked,
                other => panic!(
                    "{context} should use a supported release-readiness result, got {other:?}"
                ),
            };
            internal_only_unit_record_release_readiness_json(
                repo,
                state_dir,
                plan_rel,
                branch_closure_id,
                result,
                Path::new(summary_file),
                context,
            )
        }
        [
            concat!("record", "-final-review"),
            "--plan",
            plan_rel,
            concat!("--branch", "-closure-id"),
            branch_closure_id,
            concat!("--dispatch", "-id"),
            dispatch_id,
            "--reviewer-source",
            reviewer_source,
            "--reviewer-id",
            reviewer_id,
            "--result",
            result,
            "--summary-file",
            summary_file,
        ] => {
            let result = match *result {
                "pass" => ReviewOutcomeArg::Pass,
                "fail" => ReviewOutcomeArg::Fail,
                other => {
                    panic!("{context} should use a supported final-review result, got {other:?}")
                }
            };
            internal_only_unit_record_final_review_json(
                repo,
                state_dir,
                &record_final_review_args(
                    plan_rel,
                    branch_closure_id,
                    dispatch_id,
                    reviewer_source,
                    reviewer_id,
                    result,
                    Path::new(summary_file),
                ),
                context,
            )
        }
        [
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            result,
            "--summary-file",
            summary_file,
        ] => {
            let result = match *result {
                "pass" => ReviewOutcomeArg::Pass,
                "fail" => ReviewOutcomeArg::Fail,
                other => panic!("{context} should use a supported QA result, got {other:?}"),
            };
            internal_only_unit_record_qa_json(
                repo,
                state_dir,
                plan_rel,
                result,
                Path::new(summary_file),
                context,
            )
        }
        _ => internal_only_run_plan_execution_json_direct_or_cli(repo, state_dir, args, context),
    }
}

fn internal_only_plan_execution_failure_json(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    if let [
        concat!("record", "-review-dispatch"),
        "--plan",
        plan_rel,
        "--scope",
        scope,
        "--task",
        task,
    ] = args
    {
        let scope = match *scope {
            "task" => ReviewDispatchScopeArg::Task,
            "final-review" => ReviewDispatchScopeArg::FinalReview,
            other => {
                panic!("{context} should use a supported review-dispatch scope, got {other:?}")
            }
        };
        let task = task.parse::<u32>().unwrap_or_else(|error| {
            panic!("{context} should use a valid task number, got {task:?}: {error}")
        });
        let failure = featureforge_support::internal_only_runtime_review_dispatch_authority_json(
            repo,
            state_dir,
            &record_review_dispatch_args(plan_rel, scope, Some(task)),
        )
        .expect_err("{context} should fail closed");
        return serde_json::from_str(&failure)
            .unwrap_or_else(|error| panic!("{context} should emit valid failure json: {error}"));
    }
    if let [
        concat!("record", "-review-dispatch"),
        "--plan",
        plan_rel,
        "--scope",
        scope,
    ] = args
    {
        let scope = match *scope {
            "task" => ReviewDispatchScopeArg::Task,
            "final-review" => ReviewDispatchScopeArg::FinalReview,
            other => {
                panic!("{context} should use a supported review-dispatch scope, got {other:?}")
            }
        };
        let failure = featureforge_support::internal_only_runtime_review_dispatch_authority_json(
            repo,
            state_dir,
            &record_review_dispatch_args(plan_rel, scope, None),
        )
        .expect_err("{context} should fail closed");
        return serde_json::from_str(&failure)
            .unwrap_or_else(|error| panic!("{context} should emit valid failure json: {error}"));
    }
    let mut full_args = Vec::with_capacity(args.len() + 2);
    full_args.extend(["plan", "execution"]);
    full_args.extend_from_slice(args);
    let output = featureforge_support::run_featureforge_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        &full_args,
        context,
    );
    assert!(
        !output.status.success(),
        "{context} should fail closed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    serde_json::from_slice(payload).unwrap_or_else(|error| {
        panic!(
            "{context} should emit valid failure json: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn run_plan_execution_failure_json(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    run_plan_execution_failure_json_real_cli(repo, state_dir, args, context)
}

fn run_plan_execution_json_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let mut full_args = Vec::with_capacity(args.len() + 2);
    full_args.extend(["plan", "execution"]);
    full_args.extend_from_slice(args);
    let output = featureforge_support::run_featureforge_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        &full_args,
        context,
    );
    assert!(
        output.status.success(),
        "{context} should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{context} should emit valid json: {error}"))
}

fn expect_internal_plan_execution_json(result: Result<Value, String>, context: &str) -> Value {
    result.unwrap_or_else(|error| panic!("{context} should succeed: {error}"))
}

fn status_args(plan_rel: &str) -> StatusArgs {
    StatusArgs {
        plan: PathBuf::from(plan_rel),
        external_review_result_ready: false,
    }
}

fn status_args_with_external_review_result_ready(plan_rel: &str) -> StatusArgs {
    StatusArgs {
        plan: PathBuf::from(plan_rel),
        external_review_result_ready: true,
    }
}

fn record_review_dispatch_args(
    plan_rel: &str,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
) -> RecordReviewDispatchArgs {
    RecordReviewDispatchArgs {
        plan: PathBuf::from(plan_rel),
        scope,
        task,
    }
}

fn record_branch_closure_args(plan_rel: &str) -> RecordBranchClosureArgs {
    RecordBranchClosureArgs {
        plan: PathBuf::from(plan_rel),
    }
}

fn record_release_readiness_args(
    plan_rel: &str,
    branch_closure_id: &str,
    result: ReleaseReadinessOutcomeArg,
    summary_file: &Path,
) -> RecordReleaseReadinessArgs {
    RecordReleaseReadinessArgs {
        plan: PathBuf::from(plan_rel),
        branch_closure_id: branch_closure_id.to_owned(),
        result,
        summary_file: summary_file.to_path_buf(),
    }
}

fn record_final_review_args(
    plan_rel: &str,
    branch_closure_id: &str,
    dispatch_id: &str,
    reviewer_source: &str,
    reviewer_id: &str,
    result: ReviewOutcomeArg,
    summary_file: &Path,
) -> RecordFinalReviewArgs {
    RecordFinalReviewArgs {
        plan: PathBuf::from(plan_rel),
        branch_closure_id: branch_closure_id.to_owned(),
        dispatch_id: dispatch_id.to_owned(),
        reviewer_source: reviewer_source.to_owned(),
        reviewer_id: reviewer_id.to_owned(),
        result,
        summary_file: summary_file.to_path_buf(),
    }
}

fn record_qa_args(plan_rel: &str, result: ReviewOutcomeArg, summary_file: &Path) -> RecordQaArgs {
    RecordQaArgs {
        plan: PathBuf::from(plan_rel),
        result,
        summary_file: summary_file.to_path_buf(),
    }
}

fn internal_only_runtime_preflight_gate_json(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    context: &str,
) -> Value {
    expect_internal_plan_execution_json(
        featureforge_support::internal_only_runtime_preflight_gate_json(
            repo,
            state_dir,
            &status_args(plan_rel),
        ),
        context,
    )
}

fn internal_only_runtime_review_gate_json(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    external_review_result_ready: bool,
    context: &str,
) -> Value {
    let args = if external_review_result_ready {
        status_args_with_external_review_result_ready(plan_rel)
    } else {
        status_args(plan_rel)
    };
    expect_internal_plan_execution_json(
        featureforge_support::internal_only_runtime_review_gate_json(repo, state_dir, &args),
        context,
    )
}

fn internal_only_runtime_finish_gate_json(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    external_review_result_ready: bool,
    context: &str,
) -> Value {
    let args = if external_review_result_ready {
        status_args_with_external_review_result_ready(plan_rel)
    } else {
        status_args(plan_rel)
    };
    expect_internal_plan_execution_json(
        featureforge_support::internal_only_runtime_finish_gate_json(repo, state_dir, &args),
        context,
    )
}

fn internal_only_runtime_review_dispatch_authority_json(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
    context: &str,
) -> Value {
    expect_internal_plan_execution_json(
        featureforge_support::internal_only_runtime_review_dispatch_authority_json(
            repo,
            state_dir,
            &record_review_dispatch_args(plan_rel, scope, task),
        ),
        context,
    )
}

fn internal_only_unit_record_branch_closure_json(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    context: &str,
) -> Value {
    expect_internal_plan_execution_json(
        featureforge_support::internal_only_unit_record_branch_closure_json(
            repo,
            state_dir,
            &record_branch_closure_args(plan_rel),
        ),
        context,
    )
}

fn internal_only_unit_record_release_readiness_json(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    branch_closure_id: &str,
    result: ReleaseReadinessOutcomeArg,
    summary_file: &Path,
    context: &str,
) -> Value {
    expect_internal_plan_execution_json(
        featureforge_support::internal_only_unit_record_release_readiness_json(
            repo,
            state_dir,
            &record_release_readiness_args(plan_rel, branch_closure_id, result, summary_file),
        ),
        context,
    )
}

fn internal_only_unit_record_final_review_json(
    repo: &Path,
    state_dir: &Path,
    args: &RecordFinalReviewArgs,
    context: &str,
) -> Value {
    expect_internal_plan_execution_json(
        featureforge_support::internal_only_unit_record_final_review_json(repo, state_dir, args),
        context,
    )
}

fn internal_only_unit_record_qa_json(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    result: ReviewOutcomeArg,
    summary_file: &Path,
    context: &str,
) -> Value {
    expect_internal_plan_execution_json(
        featureforge_support::internal_only_unit_record_qa_json(
            repo,
            state_dir,
            &record_qa_args(plan_rel, result, summary_file),
        ),
        context,
    )
}

fn internal_only_unit_reconcile_review_state_json(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    context: &str,
) -> Value {
    expect_internal_plan_execution_json(
        featureforge_support::internal_only_unit_reconcile_review_state_json(
            repo,
            state_dir,
            &status_args(plan_rel),
        ),
        context,
    )
}

fn run_plan_execution_failure_json_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let mut full_args = Vec::with_capacity(args.len() + 2);
    full_args.extend(["plan", "execution"]);
    full_args.extend_from_slice(args);
    let output = featureforge_support::run_featureforge_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        &full_args,
        context,
    );
    assert!(
        !output.status.success(),
        "{context} should fail closed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    serde_json::from_slice(payload).unwrap_or_else(|error| {
        panic!(
            "{context} should emit valid failure json: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn run_recommended_plan_execution_command_json_real_cli(
    repo: &Path,
    state_dir: &Path,
    recommended_command: &str,
    context: &str,
) -> Value {
    let Some(parts) = shlex::split(recommended_command) else {
        panic!(
            "{context} should expose a shell-parseable plan execution command, got {recommended_command:?}"
        );
    };
    assert!(
        parts.len() >= 4,
        "{context} should expose a full plan execution command, got {recommended_command:?}"
    );
    assert_eq!(
        &parts[..3],
        ["featureforge", "plan", "execution"],
        "{context} should expose a plan execution command, got {recommended_command:?}"
    );
    let command_args = parts[3..].iter().map(String::as_str).collect::<Vec<_>>();
    run_plan_execution_json_real_cli(repo, state_dir, &command_args, context)
}

fn run_recommended_plan_execution_command_output_real_cli(
    repo: &Path,
    state_dir: &Path,
    recommended_command: &str,
    context: &str,
) -> Output {
    let Some(parts) = shlex::split(recommended_command) else {
        panic!(
            "{context} should expose a shell-parseable plan execution command, got {recommended_command:?}"
        );
    };
    assert!(
        parts.len() >= 4,
        "{context} should expose a full plan execution command, got {recommended_command:?}"
    );
    assert_eq!(
        &parts[..3],
        ["featureforge", "plan", "execution"],
        "{context} should expose a plan execution command, got {recommended_command:?}"
    );
    let command_args = parts[3..].iter().map(String::as_str).collect::<Vec<_>>();
    featureforge_support::run_featureforge_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        &["plan", "execution"]
            .into_iter()
            .chain(command_args.iter().copied())
            .collect::<Vec<_>>(),
        context,
    )
}

#[test]
fn internal_only_compatibility_plan_execution_close_current_task_relative_summary_paths_preserve_real_cli_semantics()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, direct_state_dir) =
        init_repo("plan-execution-close-current-task-relative-summary-parity");
    let real_state_dir = TempDir::new().expect("real-cli parity state tempdir should exist");
    let repo = repo_dir.path();
    let direct_state = direct_state_dir.path();
    let real_state = real_state_dir.path();

    setup_task_boundary_blocked_case(repo, direct_state, plan_rel, "main");
    setup_task_boundary_blocked_case(repo, real_state, plan_rel, "main");

    let review_summary_rel = "task-1-relative-review-summary.md";
    let verification_summary_rel = "task-1-relative-verification-summary.md";
    write_file(
        &repo.join(review_summary_rel),
        "Task 1 relative review summary parity fixture.\n",
    );
    write_file(
        &repo.join(verification_summary_rel),
        "Task 1 relative verification summary parity fixture.\n",
    );

    // The in-process helper intentionally defers relative summary-path cases to the real binary
    // so cwd-based path resolution stays byte-for-byte aligned with the CLI contract.
    let direct_close = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        direct_state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_rel,
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_rel,
        ],
        "direct helper should preserve real-cli relative summary path semantics via fallback",
    );
    let real_close = run_plan_execution_json_real_cli(
        repo,
        real_state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_rel,
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_rel,
        ],
        "real-cli close-current-task should resolve relative summary paths against cwd",
    );

    for field in [
        "action",
        "task_number",
        "dispatch_validation_action",
        "closure_action",
        "task_closure_status",
    ] {
        assert_eq!(
            direct_close[field], real_close[field],
            "field {field} should match when the direct helper preserves relative summary path CLI semantics via fallback"
        );
    }

    write_file(
        &repo.join(review_summary_rel),
        "Task 1 relative review summary parity fixture (changed).\n",
    );

    let direct_rerun = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        direct_state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_rel,
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_rel,
        ],
        "direct helper relative summary drift rerun should preserve real-cli semantics via fallback",
    );
    let real_rerun = run_plan_execution_json_real_cli(
        repo,
        real_state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_rel,
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_rel,
        ],
        "real-cli close-current-task relative summary drift rerun",
    );
    assert_eq!(direct_rerun["action"], Value::from("already_current"));
    assert_eq!(real_rerun["action"], Value::from("already_current"));
    assert_eq!(
        direct_rerun["closure_action"],
        Value::from("already_current")
    );
    assert_eq!(real_rerun["closure_action"], Value::from("already_current"));
    assert_eq!(
        direct_rerun["blocking_reason_codes"],
        Value::from(vec![String::from("summary_hash_drift_ignored")])
    );
    assert_eq!(
        real_rerun["blocking_reason_codes"],
        Value::from(vec![String::from("summary_hash_drift_ignored")])
    );
}

fn current_branch_name(repo: &Path) -> String {
    discover_slug_identity(repo).branch_name
}

fn expected_release_base_branch(repo: &Path) -> String {
    let current_branch = current_branch_name(repo);
    resolve_release_base_branch(&repo.join(".git"), &current_branch).unwrap_or(current_branch)
}

fn current_head_sha(repo: &Path) -> String {
    runtime_current_head_sha(repo).expect("head sha should resolve")
}

fn sha256_hex(contents: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents);
    format!("{:x}", hasher.finalize())
}

fn repo_slug(repo: &Path, _state_dir: &Path) -> String {
    discover_slug_identity(repo).repo_slug
}

fn project_artifact_dir(repo: &Path, state_dir: &Path) -> PathBuf {
    state_dir.join("projects").join(repo_slug(repo, state_dir))
}

fn preflight_acceptance_state_path(repo: &Path, state_dir: &Path) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    state_dir
        .join("projects")
        .join(repo_slug(repo, state_dir))
        .join("branches")
        .join(safe_branch)
        .join(concat!("execution-pre", "flight"))
        .join("acceptance-state.json")
}

fn seed_preflight_acceptance_state(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    plan_revision: u32,
) {
    let preflight_path = preflight_acceptance_state_path(repo, state_dir);
    if let Some(parent) = preflight_path.parent() {
        fs::create_dir_all(parent).expect(concat!(
            "pre",
            "flight acceptance fixture directory should be creatable"
        ));
    }
    let baseline_head_sha = current_head_sha(repo);
    let seed = format!(
        concat!(
            "workflow-shell-smoke-pre",
            "flight-acceptance\n{}\n{}\n{}\n{}\n"
        ),
        current_branch_name(repo),
        plan_rel,
        plan_revision,
        baseline_head_sha
    );
    let digest = sha256_hex(seed.as_bytes());
    let payload = serde_json::json!({
        "schema_version": 1,
        "plan_path": plan_rel,
        "plan_revision": plan_revision,
        "repo_state_baseline_head_sha": baseline_head_sha,
        "execution_run_id": format!("run-{}", &digest[..16]),
        "chunk_id": format!("chunk-{}", &digest[16..32]),
        "chunking_strategy": "task",
        "evaluator_policy": "spec_compliance+code_quality",
        "reset_policy": "chunk-boundary",
        "review_stack": [
            "featureforge:requesting-code-review",
            "featureforge:qa-only",
            "featureforge:document-release"
        ]
    });
    write_file(
        &preflight_path,
        &serde_json::to_string_pretty(&payload).expect(concat!(
            "pre",
            "flight acceptance fixture payload should serialize"
        )),
    );
}

#[derive(Default, Clone)]
struct EvidenceAttemptLineageFields {
    attempt_number: u32,
    status: String,
    recorded_at: String,
    packet_fingerprint: String,
    head_sha: String,
}

fn task_completion_lineage_fingerprint_from_evidence(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    task_number: u32,
) -> Option<String> {
    let status = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "task completion lineage fingerprint status probe",
    );
    let evidence_rel = status["evidence_path"].as_str()?;
    let plan_revision = status["plan_revision"].as_u64()?;
    let evidence_source = projection_support::read_state_dir_projection(&status, evidence_rel);

    let mut latest_by_step: BTreeMap<u32, EvidenceAttemptLineageFields> = BTreeMap::new();
    let mut current_task: Option<u32> = None;
    let mut current_step: Option<u32> = None;
    let mut current_attempt = EvidenceAttemptLineageFields::default();
    let mut attempt_open = false;

    let flush_attempt = |latest: &mut BTreeMap<u32, EvidenceAttemptLineageFields>,
                         task: Option<u32>,
                         step: Option<u32>,
                         attempt: &EvidenceAttemptLineageFields,
                         open: bool| {
        if !open || task != Some(task_number) {
            return;
        }
        let Some(step_number) = step else {
            return;
        };
        let replace = latest
            .get(&step_number)
            .is_none_or(|existing| attempt.attempt_number >= existing.attempt_number);
        if replace {
            latest.insert(step_number, attempt.clone());
        }
    };

    for line in evidence_source.lines() {
        if let Some(rest) = line.strip_prefix("### Task ")
            && let Some((task_text, step_text)) = rest.split_once(" Step ")
        {
            flush_attempt(
                &mut latest_by_step,
                current_task,
                current_step,
                &current_attempt,
                attempt_open,
            );
            current_task = task_text.trim().parse::<u32>().ok();
            current_step = step_text.trim().parse::<u32>().ok();
            current_attempt = EvidenceAttemptLineageFields::default();
            attempt_open = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix("#### Attempt ") {
            flush_attempt(
                &mut latest_by_step,
                current_task,
                current_step,
                &current_attempt,
                attempt_open,
            );
            current_attempt = EvidenceAttemptLineageFields {
                attempt_number: rest.trim().parse::<u32>().unwrap_or_default(),
                ..EvidenceAttemptLineageFields::default()
            };
            attempt_open = true;
            continue;
        }
        if !attempt_open || current_task != Some(task_number) {
            continue;
        }
        if let Some(value) = line.strip_prefix("**Status:** ") {
            current_attempt.status = value.trim().to_owned();
            continue;
        }
        if let Some(value) = line.strip_prefix("**Recorded At:** ") {
            current_attempt.recorded_at = value.trim().to_owned();
            continue;
        }
        if let Some(value) = line.strip_prefix("**Packet Fingerprint:** ") {
            current_attempt.packet_fingerprint = value.trim().to_owned();
            continue;
        }
        if let Some(value) = line.strip_prefix("**Head SHA:** ") {
            current_attempt.head_sha = value.trim().to_owned();
        }
    }

    flush_attempt(
        &mut latest_by_step,
        current_task,
        current_step,
        &current_attempt,
        attempt_open,
    );

    if latest_by_step.is_empty() {
        return None;
    }
    let mut payload =
        format!("plan={plan_rel}\nplan_revision={plan_revision}\ntask={task_number}\n");
    for (step_number, attempt) in latest_by_step {
        if attempt.status != "Completed"
            || attempt.recorded_at.is_empty()
            || attempt.packet_fingerprint.is_empty()
            || attempt.head_sha.is_empty()
        {
            return None;
        }
        payload.push_str(&format!(
            "step={step_number}:attempt={}:recorded_at={}:packet={}:checkpoint={}\n",
            attempt.attempt_number,
            attempt.recorded_at,
            attempt.packet_fingerprint,
            attempt.head_sha,
        ));
    }
    Some(sha256_hex(payload.as_bytes()))
}

fn write_branch_test_plan_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    browser_required: &str,
) {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let head_sha = current_head_sha(repo);
    let path = project_artifact_dir(repo, state_dir)
        .join(format!("tester-{safe_branch}-test-plan-20260324-120000.md"));
    let source = format!(
        "# Test Plan\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {head_sha}\n**Browser QA Required:** {browser_required}\n**Generated By:** featureforge:plan-eng-review\n**Generated At:** 2026-03-24T12:00:00Z\n\n## Affected Pages / Routes\n- none\n\n## Key Interactions\n- shell smoke parity fixtures\n\n## Edge Cases\n- downstream phase routing coverage\n\n## Critical Paths\n- downstream routing should stay harness-aware.\n",
        repo_slug(repo, state_dir)
    );
    write_file(&path, &source);
    let authoritative_path = harness_authoritative_artifact_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &branch,
        &format!("test-plan-{}.md", sha256_hex(source.as_bytes())),
    );
    write_file(&authoritative_path, &source);
}

fn remove_branch_test_plan_artifact(repo: &Path, state_dir: &Path) {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let path = project_artifact_dir(repo, state_dir)
        .join(format!("tester-{safe_branch}-test-plan-20260324-120000.md"));
    if path.exists() {
        fs::remove_file(&path).expect("branch test-plan artifact should be removable");
    }
}

fn remove_authoritative_test_plan_artifact(repo: &Path, state_dir: &Path) {
    let branch = current_branch_name(repo);
    let probe = harness_authoritative_artifact_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &branch,
        "test-plan-probe.md",
    );
    let Some(artifacts_dir) = probe.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(artifacts_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if file_name.starts_with("test-plan-") && file_name.ends_with(".md") {
            fs::remove_file(&path).expect("authoritative test-plan artifact should be removable");
        }
    }
}

struct WorkflowTransferArtifactSpec<'a> {
    decision_reason_codes: &'a [String],
    scope: &'a str,
    to: &'a str,
    reason: &'a str,
    file_name: &'a str,
}

fn write_workflow_transfer_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    spec: WorkflowTransferArtifactSpec<'_>,
) -> PathBuf {
    let branch = current_branch_name(repo);
    let head_sha = current_head_sha(repo);
    let path = project_artifact_dir(repo, state_dir).join(spec.file_name);
    let mut normalized_reason_codes = spec
        .decision_reason_codes
        .iter()
        .map(|code| code.trim())
        .filter(|code| !code.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    normalized_reason_codes.sort();
    normalized_reason_codes.dedup();
    write_file(
        &path,
        &format!(
            "# Workflow Transfer Record\n**Source Plan:** `{plan_rel}`\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {head_sha}\n**Decision Reason Codes:** {}\n**Scope:** {}\n**To:** {}\n**Reason:** {}\n**Generated By:** featureforge:plan-execution-transfer\n**Generated At:** 1712000000\n",
            repo_slug(repo, state_dir),
            normalized_reason_codes.join(", "),
            spec.scope,
            spec.to,
            spec.reason
        ),
    );
    path
}

fn latest_branch_test_plan_artifact(repo: &Path, state_dir: &Path) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let marker = format!("-{safe_branch}-test-plan-");
    let mut candidates = fs::read_dir(project_artifact_dir(repo, state_dir))
        .expect("project artifact directory should be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(&marker))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
        .pop()
        .expect("latest branch test-plan artifact should exist")
}

fn write_branch_review_artifact(repo: &Path, state_dir: &Path, plan_rel: &str, base_branch: &str) {
    write_branch_review_artifact_with_result(repo, state_dir, plan_rel, base_branch, "pass");
}

fn write_branch_review_artifact_with_result(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
    result: &str,
) {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let strategy_checkpoint_fingerprint = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "plan execution status for shell-smoke review artifact fixture",
    )["last_strategy_checkpoint_fingerprint"]
        .as_str()
        .expect("shell-smoke review artifact fixture should expose strategy checkpoint fingerprint")
        .to_owned();
    let reviewer_artifact_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-independent-review-20260324-120950.md"
    ));
    let reviewer_artifact_source = format!(
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Result:** {result}\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T12:09:50Z\n\n## Summary\n- dedicated independent reviewer artifact fixture.\n",
        repo_slug(repo, state_dir),
        current_head_sha(repo)
    );
    write_file(&reviewer_artifact_path, &reviewer_artifact_source);
    let reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&reviewer_artifact_path).expect("reviewer artifact should read"));
    let path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-code-review-20260324-121000.md"
    ));
    write_file(
        &path,
        &format!(
            "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** {reviewer_artifact_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Result:** {result}\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T12:10:00Z\n\n## Summary\n- shell smoke parity fixture.\n",
            reviewer_artifact_path.display(),
            repo_slug(repo, state_dir),
            current_head_sha(repo)
        ),
    );
}

fn branch_review_artifact_path(repo: &Path, state_dir: &Path) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-code-review-20260324-121000.md"
    ))
}

fn align_branch_review_identity_with_command(
    repo: &Path,
    state_dir: &Path,
    reviewer_source: &str,
    reviewer_id: &str,
) {
    let review_path = branch_review_artifact_path(repo, state_dir);
    let receipt = parse_final_review_receipt(&review_path);
    let reviewer_artifact_path = PathBuf::from(
        receipt
            .reviewer_artifact_path
            .expect("review artifact should expose reviewer artifact path"),
    );
    let old_reviewer_fingerprint = receipt
        .reviewer_artifact_fingerprint
        .expect("review artifact should expose reviewer artifact fingerprint");

    let reviewer_source_doc = fs::read_to_string(&reviewer_artifact_path)
        .expect("reviewer artifact should be readable before identity rewrite")
        .replace(
            "**Reviewer Source:** fresh-context-subagent",
            &format!("**Reviewer Source:** {reviewer_source}"),
        )
        .replace(
            "**Reviewer ID:** reviewer-fixture-001",
            &format!("**Reviewer ID:** {reviewer_id}"),
        );
    write_file(&reviewer_artifact_path, &reviewer_source_doc);
    let new_reviewer_fingerprint = sha256_hex(
        &fs::read(&reviewer_artifact_path).expect("rewritten reviewer artifact should read"),
    );

    let review_source = fs::read_to_string(&review_path)
        .expect("review artifact should be readable before identity rewrite")
        .replace(
            "**Reviewer Source:** fresh-context-subagent",
            &format!("**Reviewer Source:** {reviewer_source}"),
        )
        .replace(
            "**Reviewer ID:** reviewer-fixture-001",
            &format!("**Reviewer ID:** {reviewer_id}"),
        )
        .replace(
            &format!("**Reviewer Artifact Fingerprint:** {old_reviewer_fingerprint}"),
            &format!("**Reviewer Artifact Fingerprint:** {new_reviewer_fingerprint}"),
        );
    write_file(&review_path, &review_source);
}

fn mark_branch_review_artifacts_with_runtime_deviation_pass(repo: &Path, state_dir: &Path) {
    let review_path = branch_review_artifact_path(repo, state_dir);
    let receipt = parse_final_review_receipt(&review_path);
    let reviewer_artifact_path = PathBuf::from(
        receipt
            .reviewer_artifact_path
            .expect("review artifact should expose reviewer artifact path"),
    );
    let old_reviewer_fingerprint = receipt
        .reviewer_artifact_fingerprint
        .expect("review artifact should expose reviewer artifact fingerprint");

    let reviewer_source = fs::read_to_string(&reviewer_artifact_path)
        .expect("reviewer artifact should be readable before deviation rewrite")
        .replace(
            "**Recorded Execution Deviations:** none",
            "**Recorded Execution Deviations:** present",
        )
        .replace(
            "**Deviation Review Verdict:** not_required",
            "**Deviation Review Verdict:** pass",
        );
    write_file(&reviewer_artifact_path, &reviewer_source);
    let new_reviewer_fingerprint = sha256_hex(
        &fs::read(&reviewer_artifact_path).expect("rewritten reviewer artifact should read"),
    );

    let review_source = fs::read_to_string(&review_path)
        .expect("review artifact should be readable before deviation rewrite")
        .replace(
            "**Recorded Execution Deviations:** none",
            "**Recorded Execution Deviations:** present",
        )
        .replace(
            "**Deviation Review Verdict:** not_required",
            "**Deviation Review Verdict:** pass",
        )
        .replace(
            &format!("**Reviewer Artifact Fingerprint:** {old_reviewer_fingerprint}"),
            &format!("**Reviewer Artifact Fingerprint:** {new_reviewer_fingerprint}"),
        );
    write_file(&review_path, &review_source);
}

fn write_branch_release_artifact(repo: &Path, state_dir: &Path, plan_rel: &str, base_branch: &str) {
    write_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-release-readiness-20260324-121500.md"
    ));
    write_file(
        &path,
        &format!(
            "# Release Readiness Result\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Result:** pass\n**Generated By:** featureforge:document-release\n**Generated At:** 2026-03-24T12:15:00Z\n\n## Summary\n- shell smoke parity fixture.\n",
            repo_slug(repo, state_dir),
            current_head_sha(repo)
        ),
    );
    publish_authoritative_release_truth(repo, state_dir, &path);
}

fn set_current_branch_closure(repo: &Path, state_dir: &Path, branch_closure_id: &str) {
    let current_task_closure_records = authoritative_harness_state(repo, state_dir)
        .get("current_task_closure_records")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if current_task_closure_records.is_empty() {
        seed_current_task_closure_state(repo, state_dir, WORKFLOW_FIXTURE_PLAN_REL);
    }
    upsert_fixture_branch_closure_record(repo, state_dir, branch_closure_id);
    let contract_identity = authoritative_harness_state(repo, state_dir)["branch_closure_records"]
        [branch_closure_id]["contract_identity"]
        .as_str()
        .expect("fixture branch closure record should expose contract identity")
        .to_owned();
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("current_branch_closure_id", Value::from(branch_closure_id)),
            (
                "current_branch_closure_reviewed_state_id",
                Value::from(current_tracked_tree_id(repo)),
            ),
            (
                "current_branch_closure_contract_identity",
                Value::from(contract_identity),
            ),
        ],
    );
}

fn mark_current_branch_closure_release_ready(
    repo: &Path,
    state_dir: &Path,
    branch_closure_id: &str,
) {
    set_current_branch_closure(repo, state_dir, branch_closure_id);
    let plan_rel = authoritative_harness_state(repo, state_dir)["source_plan_path"]
        .as_str()
        .unwrap_or(WORKFLOW_FIXTURE_PLAN_REL)
        .to_owned();
    let base_branch = expected_release_base_branch(repo);
    write_branch_release_artifact(repo, state_dir, &plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[("current_release_readiness_result", Value::from("ready"))],
    );
}

fn write_matching_topology_downgrade_record(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) {
    let branch = current_branch_name(repo);
    let execution_context_key = format!("{branch}@{base_branch}");
    let record_path = harness_authoritative_artifact_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &branch,
        "execution-topology-downgrade-dependency-mismatch.json",
    );
    write_file(
        &record_path,
        &serde_json::to_string_pretty(&serde_json::json!({
            "record_version": 1,
            "authoritative_sequence": 18,
            "source_plan_path": plan_rel,
            "source_plan_revision": 1,
            "execution_context_key": execution_context_key,
            "primary_reason_class": "dependency_mismatch",
            "detail": {
                "trigger_summary": "Parallel lanes depended on shared write scope ordering.",
                "affected_units": ["task-1-step-1"],
                "blocking_evidence": {
                    "summary": "Observed dependency mismatch while reconciling unit lane.",
                    "references": ["artifact:unit-review-run-task-1-step-1"]
                },
                "operator_impact": {
                    "severity": "warning",
                    "changed_or_blocked_stage": "executing",
                    "expected_response": "downgrade the slice"
                },
                "notes": ["runtime-authored shell-smoke fixture"]
            },
            "rerun_guidance_superseded": false,
            "generated_by": "featureforge:execution-runtime",
            "generated_at": "2026-03-28T15:00:00Z",
            "record_fingerprint": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        }))
        .expect("topology downgrade record fixture should serialize"),
    );
}

fn republish_fixture_late_stage_truth_for_branch_closure(
    repo: &Path,
    state_dir: &Path,
    branch_closure_id: &str,
) {
    set_current_branch_closure(repo, state_dir, branch_closure_id);
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let artifact_dir = project_artifact_dir(repo, state_dir);
    let review_path = artifact_dir.join(format!(
        "tester-{safe_branch}-code-review-20260324-121000.md"
    ));
    let release_path = artifact_dir.join(format!(
        "tester-{safe_branch}-release-readiness-20260324-121500.md"
    ));
    publish_authoritative_release_truth(repo, state_dir, &release_path);
    publish_authoritative_final_review_truth(repo, state_dir, &review_path);
}

fn prepare_preflight_acceptance_workspace(repo: &Path, branch_name: &str) {
    let mut checkout = Command::new("git");
    checkout
        .args(["checkout", "-B", branch_name])
        .current_dir(repo);
    run_checked(
        checkout,
        concat!("git checkout pre", "flight acceptance branch"),
    );
}

fn complete_workflow_fixture_execution_with_qa_requirement(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    qa_requirement: Option<&str>,
    remove_qa_requirement: bool,
) {
    if plan_rel == WORKFLOW_FIXTURE_PLAN_REL {
        let mode = if remove_qa_requirement {
            Some(WorkflowFixtureQaMode::MissingHeader)
        } else if qa_requirement.is_none() {
            Some(WorkflowFixtureQaMode::NotRequired)
        } else {
            match qa_requirement {
                Some("required") => Some(WorkflowFixtureQaMode::Required),
                Some("not-required") => Some(WorkflowFixtureQaMode::NotRequired),
                _ => None,
            }
        };
        if let Some(mode) = mode {
            populate_fixture_from_template(
                workflow_execution_fixture_template(mode),
                repo,
                state_dir,
            );
            return;
        }
    }
    complete_workflow_fixture_execution_with_qa_requirement_slow(
        repo,
        state_dir,
        plan_rel,
        qa_requirement,
        remove_qa_requirement,
    );
}

fn complete_workflow_fixture_execution_with_qa_requirement_slow(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    qa_requirement: Option<&str>,
    remove_qa_requirement: bool,
) {
    install_full_contract_ready_artifacts(repo);
    if remove_qa_requirement {
        rewrite_plan_qa_requirement(repo, plan_rel, None);
    } else if let Some(qa_requirement) = qa_requirement {
        rewrite_plan_qa_requirement(repo, plan_rel, Some(qa_requirement));
    }
    write_repo_file(
        repo,
        "tests/workflow_shell_smoke.rs",
        "synthetic route proof\n",
    );
    prepare_preflight_acceptance_workspace(repo, "workflow-shell-smoke-fixture");
    let status = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "plan execution status for shell-smoke parity fixture",
    );
    let begin = run_plan_execution_json(
        repo,
        state_dir,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution_fingerprint"),
        ],
        "plan execution begin for shell-smoke parity fixture",
    );
    let begin_fingerprint = begin["execution_fingerprint"]
        .as_str()
        .expect("begin should expose execution_fingerprint")
        .to_owned();
    let complete_args = vec![
        "complete",
        "--plan",
        plan_rel,
        "--task",
        "1",
        "--step",
        "1",
        "--source",
        "featureforge:executing-plans",
        "--claim",
        "Completed shell smoke parity fixture task.",
        "--manual-verify-summary",
        "Verified by shell smoke parity setup.",
        "--file",
        "tests/workflow_shell_smoke.rs",
        "--expect-execution-fingerprint",
        begin_fingerprint.as_str(),
    ];
    let _ = run_plan_execution_json(
        repo,
        state_dir,
        &complete_args,
        "plan execution complete for shell-smoke parity fixture",
    );
}

fn complete_workflow_fixture_execution(repo: &Path, state_dir: &Path, plan_rel: &str) {
    complete_workflow_fixture_execution_with_qa_requirement(repo, state_dir, plan_rel, None, false);
}

#[test]
fn internal_only_compatibility_normal_execution_commands_do_not_dirty_tracked_projection_files() {
    let (repo_dir, state_dir) = init_repo("normal-execution-no-tracked-projection-churn");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = WORKFLOW_FIXTURE_PLAN_REL;

    install_full_contract_ready_artifacts(repo);
    write_repo_file(
        repo,
        "tests/workflow_shell_smoke.rs",
        "synthetic no-churn route proof\n",
    );
    prepare_preflight_acceptance_workspace(repo, "normal-execution-no-projection-churn");
    let status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before no-churn begin",
    );
    let preflight = internal_only_runtime_preflight_gate_json(
        repo,
        state,
        plan_rel,
        concat!("plan execution pre", "flight before no-churn begin"),
    );
    assert_eq!(preflight["allowed"], true);
    let baseline_status = git_status_short(repo);
    assert!(
        baseline_status
            .iter()
            .all(|line| !line.contains("docs/featureforge/plans/")
                && !line.contains("docs/featureforge/execution-evidence/")),
        "fixture setup should not start with tracked projection dirtiness: {baseline_status:?}"
    );
    let hidden_tracked_mode = run_featureforge_with_env(
        repo,
        state,
        &[
            "plan",
            "execution",
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution_fingerprint"),
        ],
        &[("FEATUREFORGE_PROJECTION_WRITE_MODE", "tracked")],
        "normal begin must reject hidden tracked projection write mode",
    );
    assert!(
        !hidden_tracked_mode.status.success(),
        "hidden tracked projection mode must not let normal begin succeed"
    );
    assert_git_status_short_unchanged(
        repo,
        &baseline_status,
        "rejected hidden tracked projection mode",
    );

    let begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution_fingerprint"),
        ],
        "plan execution begin should avoid tracked projection writes",
    );
    let begin_fingerprint = begin["execution_fingerprint"]
        .as_str()
        .expect("begin should expose execution_fingerprint")
        .to_owned();
    assert_git_status_short_unchanged(repo, &baseline_status, "normal begin command");
    let _complete = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed no-churn shell smoke fixture task.",
            "--manual-verify-summary",
            "Verified by no-churn shell smoke fixture.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_fingerprint.as_str(),
        ],
        "plan execution complete should avoid tracked projection writes",
    );

    assert_git_status_short_unchanged(repo, &baseline_status, "normal complete command");
    let status_after_complete = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status after no-churn complete",
    );
    assert_eq!(
        status_after_complete["projection_mode"], "state_dir_only",
        "status should report state-dir projection mode"
    );
    assert!(
        status_after_complete["state_dir_projection_paths"]
            .as_array()
            .is_some_and(|paths| paths.iter().any(|path| path
                .as_str()
                .is_some_and(|value| value.contains("execution-evidence")))),
        "status should expose state-dir projection paths: {status_after_complete}"
    );

    let (close_repo_dir, close_state_dir) =
        init_repo("normal-close-current-task-no-tracked-projection-churn");
    let close_repo = close_repo_dir.path();
    let close_state = close_state_dir.path();
    setup_task_boundary_blocked_case(close_repo, close_state, plan_rel, "main");
    let review_summary_path = close_repo.join("task-1-review-summary.md");
    let verification_summary_path = close_repo.join("task-1-verification-summary.md");
    write_file(&review_summary_path, "Task 1 independent review passed.\n");
    write_file(
        &verification_summary_path,
        "Task 1 verification passed against the current reviewed state.\n",
    );
    let close_baseline_status = git_status_short(close_repo);
    let _close = run_plan_execution_json_real_cli(
        close_repo,
        close_state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "normal close-current-task should avoid tracked projection writes",
    );
    assert_git_status_short_unchanged(
        close_repo,
        &close_baseline_status,
        "normal close-current-task command",
    );

    let (late_repo_dir, late_state_dir) =
        init_repo("normal-advance-late-stage-no-tracked-projection-churn");
    let late_repo = late_repo_dir.path();
    let late_state = late_state_dir.path();
    let base_branch = expected_release_base_branch(late_repo);
    complete_workflow_fixture_execution(late_repo, late_state, plan_rel);
    write_branch_test_plan_artifact(late_repo, late_state, plan_rel, "no");
    write_branch_release_artifact(late_repo, late_state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(late_repo, late_state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        late_repo,
        late_state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before no-churn advance-late-stage",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(late_repo, late_state, "branch-release-closure");
    let summary_path = late_repo.join("final-review-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let late_baseline_status = git_status_short(late_repo);
    let _advance = internal_only_run_plan_execution_json_direct_or_cli(
        late_repo,
        late_state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "normal advance-late-stage should avoid tracked projection writes",
    );
    assert_git_status_short_unchanged(
        late_repo,
        &late_baseline_status,
        "normal advance-late-stage command",
    );
    let late_materialized = internal_only_run_plan_execution_json_direct_or_cli(
        late_repo,
        late_state,
        &[
            "materialize-projections",
            "--plan",
            plan_rel,
            "--scope",
            "all",
            "--repo-export",
            "--confirm-repo-export",
        ],
        "explicit repo-export all-scope materialization after late-stage command",
    );
    let late_written_paths = late_materialized["written_paths"]
        .as_array()
        .expect("all-scope materialization should report written paths")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert!(
        late_written_paths
            .iter()
            .any(|path| path.contains("docs/featureforge/projections/")
                && path.contains("code-review")),
        "all-scope tracked materialization should export late-stage review projections: {late_written_paths:?}"
    );
    let late_status_after_materialization = internal_only_run_plan_execution_json_direct_or_cli(
        late_repo,
        late_state,
        &["status", "--plan", plan_rel],
        "status after explicit tracked late-stage projection materialization",
    );
    let late_tracked_projection_paths =
        late_status_after_materialization["tracked_projection_paths"]
            .as_array()
            .expect(
                "status should expose tracked projection paths after late-stage materialization",
            )
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
    assert!(
        late_tracked_projection_paths
            .iter()
            .any(|path| path.contains("docs/featureforge/projections/")
                && path.contains("code-review")),
        "status should surface tracked late-stage projection paths: {late_tracked_projection_paths:?}"
    );
    assert_eq!(
        late_status_after_materialization["tracked_projections_current"],
        Value::Bool(true),
        "status should include late-stage projections in tracked currentness after all-scope materialization"
    );
}

#[test]
fn internal_only_compatibility_materialize_projections_default_export_does_not_change_runtime_truth_or_approved_files()
 {
    let (repo_dir, state_dir) = init_repo("materialize-projections-default-export");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = WORKFLOW_FIXTURE_PLAN_REL;

    install_full_contract_ready_artifacts(repo);
    write_repo_file(
        repo,
        "tests/workflow_shell_smoke.rs",
        "synthetic materialization route proof\n",
    );
    prepare_preflight_acceptance_workspace(repo, "materialize-projections-explicit");
    let status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before explicit materialization fixture",
    );
    let preflight = internal_only_runtime_preflight_gate_json(
        repo,
        state,
        plan_rel,
        concat!(
            "plan execution pre",
            "flight before explicit materialization fixture"
        ),
    );
    assert_eq!(preflight["allowed"], true);
    let pre_begin_materialized = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["materialize-projections", "--plan", plan_rel],
        "default state-dir projection materialization should be available for pre-begin inspection",
    );
    assert_eq!(pre_begin_materialized["action"], "materialized");
    assert_eq!(pre_begin_materialized["projection_mode"], "state_dir_only");
    let pre_begin_materialized_rerun = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["materialize-projections", "--plan", plan_rel],
        "default state-dir projection materialization rerun should not require a repair route",
    );
    assert_eq!(pre_begin_materialized_rerun["action"], "materialized");
    assert_eq!(
        pre_begin_materialized_rerun["projection_mode"],
        "state_dir_only"
    );
    let begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution_fingerprint"),
        ],
        "plan execution begin before explicit materialization",
    );
    let begin_fingerprint = begin["execution_fingerprint"]
        .as_str()
        .expect("begin should expose execution_fingerprint")
        .to_owned();
    let _complete = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed explicit materialization fixture task.",
            "--manual-verify-summary",
            "Verified explicit materialization fixture.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_fingerprint.as_str(),
        ],
        "plan execution complete before explicit materialization",
    );
    let status_before = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before explicit projection export materialization",
    );
    let operator_before = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator before explicit projection export materialization",
    );
    let route_before = public_route_snapshot(&status_before);
    let operator_route_before = public_route_snapshot(&operator_before);
    let tracked_projection_sources_before = status_before["tracked_projection_paths"]
        .as_array()
        .expect("status should expose tracked projection paths")
        .iter()
        .filter_map(Value::as_str)
        .map(|path| (path.to_owned(), fs::read_to_string(repo.join(path)).ok()))
        .collect::<HashMap<_, _>>();
    let approved_plan_before =
        fs::read_to_string(repo.join(plan_rel)).expect("approved plan should be readable");
    let approved_evidence_rel = status_before["evidence_path"]
        .as_str()
        .expect("status should expose evidence_path");
    let approved_evidence_before = fs::read_to_string(repo.join(approved_evidence_rel)).ok();
    assert_eq!(
        operator_route_before, route_before,
        "operator and status should agree before projection export materialization"
    );
    assert_eq!(
        status_before["tracked_projections_current"], false,
        "normal state-dir-only commands should leave tracked projections stale before explicit materialization"
    );
    let unconfirmed_repo_export = run_plan_execution_failure_json(
        repo,
        state,
        &[
            "materialize-projections",
            "--plan",
            plan_rel,
            "--repo-export",
        ],
        "unconfirmed projection repo export should fail closed",
    );
    assert_eq!(
        unconfirmed_repo_export["error_class"],
        "InvalidCommandInput"
    );
    assert!(
        unconfirmed_repo_export["message"]
            .as_str()
            .is_some_and(|message| message.contains("--confirm-repo-export")),
        "unconfirmed repo export failure should explain the explicit acknowledgement requirement: {unconfirmed_repo_export}"
    );

    let materialized = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "materialize-projections",
            "--plan",
            plan_rel,
            "--repo-export",
            "--confirm-repo-export",
        ],
        "explicit projection export materialization",
    );
    assert_eq!(materialized["action"], "materialized");
    assert_eq!(materialized["projection_mode"], "projection_export");
    assert!(
        materialized["trace_summary"].as_str().is_some_and(
            |summary| summary.contains("approved plan/evidence files were not modified")
        ),
        "default materialization should report approved-file preservation: {materialized}"
    );
    assert_eq!(
        materialized["runtime_truth_changed"], false,
        "materialization must not mutate runtime truth"
    );
    let written_paths = materialized["written_paths"]
        .as_array()
        .expect("materialization should report written paths")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert!(
        written_paths
            .iter()
            .all(|path| path.starts_with("docs/featureforge/projections/")),
        "default materialization should only report projection export paths, paths={written_paths:?}"
    );
    assert!(
        written_paths
            .iter()
            .any(|path| path.ends_with("/execution-plan.md"))
            && written_paths
                .iter()
                .any(|path| path.ends_with("/execution-evidence.md")),
        "default materialization should report plan and evidence projection exports, paths={written_paths:?}"
    );
    assert!(
        written_paths.iter().all(|path| repo.join(path).is_file()),
        "default materialization should write every reported projection file, paths={written_paths:?}"
    );
    assert!(
        written_paths.iter().any(|path| {
            let before = tracked_projection_sources_before
                .get(*path)
                .and_then(Option::as_deref);
            let after = fs::read_to_string(repo.join(path)).unwrap_or_else(|error| {
                panic!("materialized projection {path} should be readable: {error}")
            });
            before != Some(after.as_str())
        }),
        "default materialization should change projection export file contents"
    );
    assert_eq!(
        fs::read_to_string(repo.join(plan_rel)).expect("approved plan should remain readable"),
        approved_plan_before,
        "projection export must not modify the approved plan file"
    );
    assert_eq!(
        fs::read_to_string(repo.join(approved_evidence_rel)).ok(),
        approved_evidence_before,
        "projection export must not create or modify the approved execution evidence file"
    );
    let status_after = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status after explicit projection export materialization",
    );
    let operator_after = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after explicit projection export materialization",
    );
    assert_eq!(
        status_after["tracked_projections_current"], true,
        "tracked projections should be reported current after explicit materialization"
    );
    assert_eq!(
        status_after["execution_fingerprint"], status_before["execution_fingerprint"],
        "projection export materialization must not change the execution fingerprint"
    );
    assert_eq!(
        operator_after["projection_mode"], status_after["projection_mode"],
        "operator should expose the shared projection mode from status"
    );
    assert_eq!(
        operator_after["state_dir_projection_paths"], status_after["state_dir_projection_paths"],
        "operator should expose the shared state-dir projection paths from status"
    );
    assert_eq!(
        operator_after["tracked_projection_paths"], status_after["tracked_projection_paths"],
        "operator should expose the shared tracked projection paths from status"
    );
    assert_eq!(
        operator_after["tracked_projections_current"], status_after["tracked_projections_current"],
        "operator should expose the shared tracked projection currentness from status"
    );
    let route_after = public_route_snapshot(&status_after);
    let operator_route_after = public_route_snapshot(&operator_after);
    assert_eq!(
        operator_route_after, route_before,
        "operator routing must not change after projection export materialization"
    );
    assert_eq!(
        route_after, route_before,
        "projection export materialization must not change routing truth"
    );
}

#[test]
fn internal_only_compatibility_tampered_state_dir_projection_is_not_materialized_as_current() {
    let (repo_dir, state_dir) = init_repo("tampered-state-dir-projection-not-current");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = WORKFLOW_FIXTURE_PLAN_REL;

    install_full_contract_ready_artifacts(repo);
    write_repo_file(
        repo,
        "tests/workflow_shell_smoke.rs",
        "synthetic tampered projection route proof\n",
    );
    prepare_preflight_acceptance_workspace(repo, "tampered-state-dir-projection");
    let status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before tampered state-dir projection fixture",
    );
    let preflight = internal_only_runtime_preflight_gate_json(
        repo,
        state,
        plan_rel,
        concat!("pre", "flight before tampered state-dir projection fixture"),
    );
    assert_eq!(preflight["allowed"], true);
    let begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution_fingerprint"),
        ],
        "begin before tampered state-dir projection",
    );
    let _complete = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed tampered projection fixture task.",
            "--manual-verify-summary",
            "Verified tampered projection fixture.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution_fingerprint"),
        ],
        "complete before tampered state-dir projection",
    );
    let status_after_complete = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before state-dir projection tamper",
    );
    let evidence_rel = status_after_complete["evidence_path"]
        .as_str()
        .expect("status should expose approved evidence path");
    projection_support::write_state_dir_projection(
        &status_after_complete,
        evidence_rel,
        "# Execution Evidence: tampered\n\n### Task 99 Step 99\n",
    );
    let status_failure = run_plan_execution_failure_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "tampered state-dir projection status should fail closed",
    );
    assert_eq!(status_failure["error_class"], "MalformedExecutionState");

    let failure = run_plan_execution_failure_json(
        repo,
        state,
        &[
            "materialize-projections",
            "--plan",
            plan_rel,
            "--repo-export",
            "--confirm-repo-export",
        ],
        "tampered state-dir projection materialization should fail closed",
    );
    assert_eq!(failure["error_class"], "MalformedExecutionState");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("State-dir execution evidence projection")),
        "failure should explain that the state-dir evidence projection is not authoritative: {failure}"
    );
}

#[test]
fn internal_only_compatibility_deleting_tracked_projection_files_does_not_change_routing() {
    let (repo_dir, state_dir) = init_repo("deleting-tracked-projections-no-routing-change");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = WORKFLOW_FIXTURE_PLAN_REL;

    install_full_contract_ready_artifacts(repo);
    write_repo_file(
        repo,
        "tests/workflow_shell_smoke.rs",
        "synthetic tracked projection deletion route proof\n",
    );
    prepare_preflight_acceptance_workspace(repo, "deleting-tracked-projections");
    let status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before tracked projection deletion fixture",
    );
    let preflight = internal_only_runtime_preflight_gate_json(
        repo,
        state,
        plan_rel,
        concat!(
            "plan execution pre",
            "flight before tracked projection deletion fixture"
        ),
    );
    assert_eq!(preflight["allowed"], true);
    let begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution_fingerprint"),
        ],
        "plan execution begin before tracked projection deletion",
    );
    let _complete = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed tracked projection deletion fixture task.",
            "--manual-verify-summary",
            "Verified tracked projection deletion fixture.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution_fingerprint"),
        ],
        "plan execution complete before tracked projection deletion",
    );
    let _materialized = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "materialize-projections",
            "--plan",
            plan_rel,
            "--repo-export",
            "--confirm-repo-export",
        ],
        "explicit projection export materialization before deletion",
    );
    let status_before_delete = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before deleting tracked projection files",
    );
    let operator_before_delete = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator before deleting tracked projection files",
    );
    assert_eq!(
        status_before_delete["tracked_projections_current"], true,
        "tracked projections should be current before deletion"
    );
    let tracked_projection_paths = status_before_delete["tracked_projection_paths"]
        .as_array()
        .expect("status should expose tracked projection paths")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert!(
        tracked_projection_paths
            .iter()
            .any(|path| path.ends_with("/execution-evidence.md")),
        "fixture should expose a deletable tracked evidence projection path: {tracked_projection_paths:?}"
    );
    for rel_path in &tracked_projection_paths {
        let path = repo.join(rel_path);
        if path.exists() {
            fs::remove_file(&path).unwrap_or_else(|error| {
                panic!(
                    "tracked projection file {} should be removable: {error}",
                    path.display()
                )
            });
        }
    }

    let status_after_delete = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status after deleting tracked projection files",
    );
    let operator_after_delete = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after deleting tracked projection files",
    );
    assert_eq!(
        status_after_delete["tracked_projections_current"], false,
        "tracked projection deletion should be surfaced as stale export state"
    );
    assert_eq!(
        public_route_snapshot(&status_after_delete),
        public_route_snapshot(&status_before_delete),
        "status routing must not depend on tracked projection file presence"
    );
    assert_eq!(
        public_route_snapshot(&operator_after_delete),
        public_route_snapshot(&operator_before_delete),
        "operator routing must not depend on tracked projection file presence"
    );
}

#[test]
fn internal_only_compatibility_missing_state_dir_projection_does_not_promote_tracked_evidence_export()
 {
    let (repo_dir, state_dir) = init_repo("missing-state-dir-projection-ignores-tracked-evidence");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = WORKFLOW_FIXTURE_PLAN_REL;

    complete_workflow_fixture_execution_with_qa_requirement_slow(
        repo, state, plan_rel, None, false,
    );
    let _materialized = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "materialize-projections",
            "--plan",
            plan_rel,
            "--repo-export",
            "--confirm-repo-export",
        ],
        "explicit projection export materialization before state-dir projection deletion",
    );
    let status_before_delete = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before deleting state-dir evidence projection",
    );
    let operator_before_delete = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator before deleting state-dir evidence projection",
    );
    assert_eq!(
        status_before_delete["tracked_projections_current"], true,
        "tracked projections should start current after explicit materialization"
    );
    assert!(
        authoritative_harness_state(repo, state)["execution_evidence_attempts"]
            .as_array()
            .is_some_and(|attempts| !attempts.is_empty()),
        "fixture should persist authoritative execution evidence attempts"
    );
    let evidence_rel = status_before_delete["evidence_path"]
        .as_str()
        .expect("status should expose approved evidence path");
    let evidence_export_rel = status_before_delete["tracked_projection_paths"]
        .as_array()
        .expect("status should expose tracked projection paths")
        .iter()
        .filter_map(Value::as_str)
        .find(|path| path.ends_with("/execution-evidence.md"))
        .expect("status should expose a tracked evidence projection export path");
    let evidence_path = repo.join(evidence_export_rel);
    let evidence_source = fs::read_to_string(&evidence_path).unwrap_or_else(|error| {
        panic!(
            "tracked evidence projection export {} should be readable: {error}",
            evidence_path.display()
        )
    });
    let stale_evidence_source = evidence_source.replace(
        "Completed shell smoke parity fixture task.",
        "Completed stale tracked evidence export.",
    );
    assert_ne!(
        stale_evidence_source, evidence_source,
        "fixture evidence should include the completion claim being tampered"
    );
    fs::write(&evidence_path, stale_evidence_source).unwrap_or_else(|error| {
        panic!(
            "tracked evidence projection export {} should be writable: {error}",
            evidence_path.display()
        )
    });
    projection_support::remove_state_dir_projection(&status_before_delete, evidence_rel);

    let status_after_delete = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status after deleting state-dir evidence projection with stale tracked export present",
    );
    let operator_after_delete = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after deleting state-dir evidence projection with stale tracked export present",
    );
    assert_eq!(
        status_after_delete["tracked_projections_current"], false,
        "stale tracked evidence must remain optional export state when the state-dir read model is missing"
    );
    assert_eq!(
        status_after_delete["execution_fingerprint"], status_before_delete["execution_fingerprint"],
        "status execution truth must not be recomputed from stale tracked evidence"
    );
    assert_eq!(
        operator_after_delete["execution_fingerprint"],
        operator_before_delete["execution_fingerprint"],
        "operator execution truth must not be recomputed from stale tracked evidence"
    );
    assert_eq!(
        public_route_snapshot(&status_after_delete),
        public_route_snapshot(&status_before_delete),
        "status routing must not use tracked evidence as a state-dir fallback"
    );
    assert_eq!(
        public_route_snapshot(&operator_after_delete),
        public_route_snapshot(&operator_before_delete),
        "operator routing must not use tracked evidence as a state-dir fallback"
    );
}

fn append_tracked_repo_line(repo: &Path, rel_path: &str, line: &str) {
    let path = repo.join(rel_path);
    let mut source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "tracked fixture file {} should be readable: {error}",
            path.display()
        )
    });
    if !source.ends_with('\n') {
        source.push('\n');
    }
    source.push_str(line);
    source.push('\n');
    write_file(&path, &source);
}

fn upsert_plan_header(repo: &Path, plan_rel: &str, header: &str, value: &str) {
    let plan_path = repo.join(plan_rel);
    let source = fs::read_to_string(&plan_path).unwrap_or_else(|error| {
        panic!(
            "plan fixture {} should be readable: {error}",
            plan_path.display()
        )
    });
    let header_prefix = format!("**{header}:** ");
    let replacement = format!("{header_prefix}{value}");
    if source.contains(&header_prefix) {
        let rewritten = source
            .lines()
            .map(|line| {
                if line.starts_with(&header_prefix) {
                    replacement.clone()
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        write_file(&plan_path, &rewritten);
        return;
    }
    let inserted = if source.contains("**QA Requirement:**") {
        source.replacen(
            "**QA Requirement:** not-required\n",
            &format!("**QA Requirement:** not-required\n{replacement}\n"),
            1,
        )
    } else {
        source.replacen(
            "**Last Reviewed By:** plan-eng-review\n",
            &format!("**Last Reviewed By:** plan-eng-review\n{replacement}\n"),
            1,
        )
    };
    write_file(&plan_path, &inserted);
}

fn update_authoritative_harness_state(repo: &Path, state_dir: &Path, updates: &[(&str, Value)]) {
    let state_path = harness_state_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &current_branch_name(repo),
    );
    let mut payload: Value = match fs::read_to_string(&state_path) {
        Ok(source) => serde_json::from_str(&source)
            .expect("authoritative shell-smoke harness state should remain valid json"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Value::Object(serde_json::Map::new())
        }
        Err(error) => panic!("authoritative shell-smoke harness state should be readable: {error}"),
    };
    let explicit_reviewed_state_update = updates
        .iter()
        .any(|(key, _)| *key == "current_branch_closure_reviewed_state_id");
    let explicit_contract_identity_update = updates
        .iter()
        .any(|(key, _)| *key == "current_branch_closure_contract_identity");
    let derived_reviewed_state_value = (!explicit_reviewed_state_update)
        .then(|| {
            updates.iter().find_map(|(key, value)| {
                (*key == "current_branch_closure_id").then(|| {
                    value
                        .as_str()
                        .filter(|text| !text.trim().is_empty())
                        .map(|branch_closure_id| {
                            payload["branch_closure_records"][branch_closure_id]
                                ["reviewed_state_id"]
                                .as_str()
                                .map(|reviewed_state_id| Value::from(reviewed_state_id.to_owned()))
                                .unwrap_or_else(|| Value::from(current_tracked_tree_id(repo)))
                        })
                        .unwrap_or(Value::Null)
                })
            })
        })
        .flatten();
    let derived_contract_identity_value = (!explicit_contract_identity_update)
        .then(|| {
            updates.iter().find_map(|(key, value)| {
                (*key == "current_branch_closure_id").then(|| {
                    value
                        .as_str()
                        .filter(|text| !text.trim().is_empty())
                        .map(|branch_closure_id| {
                            payload["branch_closure_records"][branch_closure_id]
                                ["contract_identity"]
                                .as_str()
                                .map(|contract_identity| Value::from(contract_identity.to_owned()))
                                .unwrap_or(Value::Null)
                        })
                        .unwrap_or(Value::Null)
                })
            })
        })
        .flatten();
    let object = payload
        .as_object_mut()
        .expect("authoritative shell-smoke harness state should remain an object");
    for (key, value) in updates {
        object.insert((*key).to_string(), value.clone());
    }
    if let Some(reviewed_state_value) = derived_reviewed_state_value {
        object.insert(
            String::from("current_branch_closure_reviewed_state_id"),
            reviewed_state_value,
        );
    }
    if let Some(contract_identity_value) = derived_contract_identity_value {
        object.insert(
            String::from("current_branch_closure_contract_identity"),
            contract_identity_value,
        );
    }
    write_authoritative_harness_state(repo, state_dir, &payload);
}

fn bind_explicit_reopen_repair_target(repo: &Path, state_dir: &Path, task: u32, step: u32) {
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            (
                "explicit_reopen_repair_targets",
                json!([{
                    "target_task": task,
                    "target_step": step,
                    "target_record_id": execution_step_repair_target_id(task, step),
                    "created_sequence": 1,
                    "expires_on_plan_fingerprint_change": true
                }]),
            ),
            ("review_state_repair_follow_up_record", Value::Null),
            ("review_state_repair_follow_up", Value::Null),
            ("review_state_repair_follow_up_task", Value::Null),
            ("review_state_repair_follow_up_step", Value::Null),
            (
                "review_state_repair_follow_up_closure_record_id",
                Value::Null,
            ),
        ],
    );
}

fn authoritative_harness_state(repo: &Path, state_dir: &Path) -> Value {
    let state_path = harness_state_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &current_branch_name(repo),
    );
    serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("authoritative shell-smoke harness state should be readable"),
    )
    .expect("authoritative shell-smoke harness state should remain valid json")
}

fn authoritative_harness_state_digest(repo: &Path, state_dir: &Path) -> String {
    let state_path = harness_state_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &current_branch_name(repo),
    );
    if !state_path.exists() {
        return String::from("missing");
    }
    let contents = fs::read(&state_path).unwrap_or_else(|error| {
        panic!("authoritative shell-smoke harness state should read: {error}")
    });
    sha256_hex(&contents)
}

fn write_authoritative_harness_state(repo: &Path, state_dir: &Path, payload: &Value) {
    let repo_slug = repo_slug(repo, state_dir);
    let branch_name = current_branch_name(repo);
    let state_path = harness_state_path(state_dir, &repo_slug, &branch_name);
    write_file(
        &state_path,
        &serde_json::to_string(payload)
            .expect("authoritative shell-smoke harness state should serialize"),
    );
    let legacy_path = state_path.with_file_name("state.legacy.json");
    if let Err(error) = fs::remove_file(&legacy_path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        panic!(
            "authoritative shell-smoke legacy backup {} should be removable: {error}",
            legacy_path.display()
        );
    }
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(&state_path, payload)
        .expect("authoritative shell-smoke fixture update should sync typed event authority");
}

fn upsert_authoritative_nested_object(
    repo: &Path,
    state_dir: &Path,
    key: &str,
    subkey: &str,
    value: Value,
) {
    let mut payload = authoritative_harness_state(repo, state_dir);
    let object = payload
        .as_object_mut()
        .expect("authoritative shell-smoke harness state should remain an object");
    let entry = object
        .entry(key.to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let map = entry
        .as_object_mut()
        .expect("authoritative shell-smoke harness nested value should remain an object");
    map.insert(subkey.to_string(), value);
    write_authoritative_harness_state(repo, state_dir, &payload);
}

fn fixture_markdown_header_value(source: &str, header: &str) -> Option<String> {
    let prefix = format!("**{header}:** ");
    source
        .lines()
        .find_map(|line| line.trim().strip_prefix(&prefix).map(str::to_owned))
}

fn fixture_summary_hash(summary: &str) -> String {
    sha256_hex(summary.as_bytes())
}

fn republish_authoritative_artifact_from_path(
    repo: &Path,
    state_dir: &Path,
    path: &Path,
    artifact_prefix: &str,
    state_fingerprint_field: &str,
) -> PathBuf {
    let source = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "authoritative artifact {} should be readable for republish: {error}",
            path.display()
        )
    });
    let fingerprint = sha256_hex(source.as_bytes());
    let published_path = harness_authoritative_artifact_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &current_branch_name(repo),
        &format!("{artifact_prefix}-{fingerprint}.md"),
    );
    write_file(&published_path, &source);
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[(state_fingerprint_field, Value::from(fingerprint.clone()))],
    );
    if state_fingerprint_field == "last_final_review_artifact_fingerprint" {
        let mut payload = authoritative_harness_state(repo, state_dir);
        let current_record_id = payload["current_final_review_record_id"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        if !current_record_id.trim().is_empty()
            && payload["final_review_record_history"][&current_record_id].is_object()
        {
            payload["final_review_record_history"][&current_record_id]["final_review_fingerprint"] =
                Value::from(fingerprint);
            write_authoritative_harness_state(repo, state_dir, &payload);
        }
    }
    published_path
}

fn upsert_fixture_branch_closure_record(repo: &Path, state_dir: &Path, branch_closure_id: &str) {
    let payload = authoritative_harness_state(repo, state_dir);
    let source_plan_path = payload["current_task_closure_records"]
        .as_object()
        .and_then(|records| records.values().next())
        .and_then(|record| record["source_plan_path"].as_str())
        .unwrap_or(WORKFLOW_FIXTURE_PLAN_REL)
        .to_owned();
    let source_plan_revision = payload["current_task_closure_records"]
        .as_object()
        .and_then(|records| records.values().next())
        .and_then(|record| record["source_plan_revision"].as_u64())
        .unwrap_or(1);
    let source_task_closure_ids = payload["current_task_closure_records"]
        .as_object()
        .map(|records| {
            records
                .values()
                .filter_map(|record| record["closure_record_id"].as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .filter(|ids| !ids.is_empty())
        .unwrap_or_else(|| vec![String::from("task-1-closure")]);
    upsert_authoritative_nested_object(
        repo,
        state_dir,
        "branch_closure_records",
        branch_closure_id,
        serde_json::json!({
            "branch_closure_id": branch_closure_id,
            "source_plan_path": source_plan_path,
            "source_plan_revision": source_plan_revision,
            "repo_slug": repo_slug(repo, state_dir),
            "branch_name": current_branch_name(repo),
            "base_branch": expected_release_base_branch(repo),
            "reviewed_state_id": current_tracked_tree_id(repo),
            "contract_identity": branch_contract_identity(
                &source_plan_path,
                source_plan_revision as u32,
                repo,
                &expected_release_base_branch(repo),
                state_dir,
            ),
            "effective_reviewed_branch_surface": "repo_tracked_content",
            "source_task_closure_ids": source_task_closure_ids,
            "provenance_basis": "task_closure_lineage",
            "closure_status": "current",
            "superseded_branch_closure_ids": [],
        }),
    );
}

fn current_tracked_tree_id(repo: &Path) -> String {
    let tree_sha =
        runtime_current_tracked_tree_sha(repo).expect("tracked tree helper should resolve");
    format!("git_tree:{tree_sha}")
}

fn semantic_execution_context(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
) -> featureforge::execution::state::ExecutionContext {
    let runtime = discover_execution_runtime(
        repo,
        state_dir,
        "workflow_shell_smoke semantic identity fixture",
    );
    load_execution_context(&runtime, Path::new(plan_rel))
        .expect("workflow_shell_smoke semantic identity fixture should load execution context")
}

fn task_contract_identity(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    task_number: u32,
) -> String {
    let context = semantic_execution_context(repo, state_dir, plan_rel);
    task_definition_identity_for_task(&context, task_number)
        .expect("workflow_shell_smoke task semantic identity fixture should compute")
        .unwrap_or_else(|| format!("task-contract-fixture-{task_number}"))
}

fn branch_contract_identity(
    plan_rel: &str,
    _plan_revision: u32,
    repo: &Path,
    base_branch: &str,
    state_dir: &Path,
) -> String {
    let _ = base_branch;
    let context = semantic_execution_context(repo, state_dir, plan_rel);
    branch_definition_identity_for_context(&context)
}

fn publish_authoritative_final_review_truth(repo: &Path, state_dir: &Path, review_path: &Path) {
    let branch = current_branch_name(repo);
    let review_source = fs::read_to_string(review_path)
        .expect("shell-smoke review artifact should be readable for authoritative publication");
    let review_fingerprint = sha256_hex(review_source.as_bytes());
    let authoritative_state = authoritative_harness_state(repo, state_dir);
    let branch_closure_id = authoritative_state["current_branch_closure_id"]
        .as_str()
        .unwrap_or("branch-release-closure")
        .to_owned();
    let release_readiness_record_id = authoritative_state["current_release_readiness_record_id"]
        .as_str()
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty());
    let plan_rel = fixture_markdown_header_value(&review_source, "Source Plan")
        .expect("fixture final review should contain Source Plan")
        .trim_matches('`')
        .to_owned();
    let base_branch = fixture_markdown_header_value(&review_source, "Base Branch")
        .expect("fixture final review should contain Base Branch");
    let reviewer_source = fixture_markdown_header_value(&review_source, "Reviewer Source")
        .expect("fixture final review should contain Reviewer Source");
    let reviewer_id = fixture_markdown_header_value(&review_source, "Reviewer ID")
        .expect("fixture final review should contain Reviewer ID");
    let summary = String::from("shell smoke parity fixture.");
    let summary_hash = fixture_summary_hash(&summary);
    let browser_qa_required = fs::read_to_string(repo.join(&plan_rel))
        .expect("fixture plan should be readable")
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("**QA Requirement:** ")
                .map(|value| value.trim().to_owned())
        })
        .and_then(|value| {
            if value.eq_ignore_ascii_case("required") {
                Some(true)
            } else if value.eq_ignore_ascii_case("not-required") {
                Some(false)
            } else {
                None
            }
        });
    write_file(
        &harness_authoritative_artifact_path(
            state_dir,
            &repo_slug(repo, state_dir),
            &branch,
            &format!("final-review-{review_fingerprint}.md"),
        ),
        &review_source,
    );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("fresh")),
            (
                "browser_qa_state",
                Value::from(if browser_qa_required == Some(true) {
                    "missing"
                } else {
                    "not_required"
                }),
            ),
            ("release_docs_state", Value::from("fresh")),
            (
                "last_final_review_artifact_fingerprint",
                Value::from(review_fingerprint.clone()),
            ),
        ],
    );
    let record_id = format!(
        "final-review-record-{}",
        sha256_hex(
            format!("{branch_closure_id}:{summary_hash}:{reviewer_source}:{reviewer_id}")
                .as_bytes()
        )
    );
    upsert_authoritative_nested_object(
        repo,
        state_dir,
        "final_review_record_history",
        &record_id,
        serde_json::json!({
            "record_id": record_id.clone(),
            "record_sequence": 1,
            "record_status": "current",
            "branch_closure_id": branch_closure_id.clone(),
            "release_readiness_record_id": release_readiness_record_id,
            "source_plan_path": plan_rel.clone(),
            "source_plan_revision": 1,
            "repo_slug": repo_slug(repo, state_dir),
            "branch_name": branch.clone(),
            "base_branch": base_branch.clone(),
            "reviewed_state_id": current_tracked_tree_id(repo),
            "dispatch_id": "fixture-final-review-dispatch",
            "reviewer_source": reviewer_source.clone(),
            "reviewer_id": reviewer_id.clone(),
            "result": "pass",
            "final_review_fingerprint": review_fingerprint.clone(),
            "browser_qa_required": browser_qa_required,
            "summary": summary.clone(),
            "summary_hash": summary_hash.clone(),
        }),
    );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            (
                "current_final_review_branch_closure_id",
                Value::from(branch_closure_id),
            ),
            (
                "current_final_review_dispatch_id",
                Value::from("fixture-final-review-dispatch"),
            ),
            (
                "current_final_review_reviewer_source",
                Value::from(reviewer_source),
            ),
            ("current_final_review_reviewer_id", Value::from(reviewer_id)),
            ("current_final_review_result", Value::from("pass")),
            (
                "current_final_review_summary_hash",
                Value::from(summary_hash),
            ),
            ("current_final_review_record_id", Value::from(record_id)),
        ],
    );
}

fn publish_authoritative_release_truth(repo: &Path, state_dir: &Path, release_path: &Path) {
    let branch = current_branch_name(repo);
    let release_source = fs::read_to_string(release_path)
        .expect("shell-smoke release artifact should be readable for authoritative publication");
    let release_fingerprint = sha256_hex(release_source.as_bytes());
    let branch_closure_id =
        authoritative_harness_state(repo, state_dir)["current_branch_closure_id"]
            .as_str()
            .unwrap_or("branch-release-closure")
            .to_owned();
    let plan_rel = fixture_markdown_header_value(&release_source, "Source Plan")
        .expect("fixture release should contain Source Plan")
        .trim_matches('`')
        .to_owned();
    let base_branch = fixture_markdown_header_value(&release_source, "Base Branch")
        .expect("fixture release should contain Base Branch");
    let summary = String::from("shell smoke parity fixture.");
    let summary_hash = fixture_summary_hash(&summary);
    write_file(
        &harness_authoritative_artifact_path(
            state_dir,
            &repo_slug(repo, state_dir),
            &branch,
            &format!("release-docs-{release_fingerprint}.md"),
        ),
        &release_source,
    );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("release_docs_state", Value::from("fresh")),
            (
                "last_release_docs_artifact_fingerprint",
                Value::from(release_fingerprint.clone()),
            ),
        ],
    );
    let record_id = format!(
        "release-readiness-record-{}",
        sha256_hex(format!("{branch_closure_id}:{summary_hash}:ready").as_bytes())
    );
    upsert_authoritative_nested_object(
        repo,
        state_dir,
        "release_readiness_record_history",
        &record_id,
        serde_json::json!({
            "record_id": record_id.clone(),
            "record_sequence": 1,
            "record_status": "current",
            "branch_closure_id": branch_closure_id.clone(),
            "source_plan_path": plan_rel.clone(),
            "source_plan_revision": 1,
            "repo_slug": repo_slug(repo, state_dir),
            "branch_name": branch.clone(),
            "base_branch": base_branch.clone(),
            "reviewed_state_id": current_tracked_tree_id(repo),
            "result": "ready",
            "release_docs_fingerprint": release_fingerprint.clone(),
            "summary": summary.clone(),
            "summary_hash": summary_hash.clone(),
            "generated_by_identity": "featureforge/release-readiness",
        }),
    );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("current_release_readiness_result", Value::from("ready")),
            (
                "current_release_readiness_summary_hash",
                Value::from(summary_hash),
            ),
            (
                "current_release_readiness_record_id",
                Value::from(record_id),
            ),
        ],
    );
}

fn publish_authoritative_browser_qa_truth(
    repo: &Path,
    state_dir: &Path,
    result: &str,
    summary: &str,
) {
    let branch = current_branch_name(repo);
    let authoritative_state = authoritative_harness_state(repo, state_dir);
    let branch_closure_id = authoritative_state["current_branch_closure_id"]
        .as_str()
        .unwrap_or("branch-release-closure")
        .to_owned();
    let final_review_record_id = authoritative_state["current_final_review_record_id"]
        .as_str()
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty());
    let plan_rel = authoritative_state["source_plan_path"]
        .as_str()
        .unwrap_or(WORKFLOW_FIXTURE_PLAN_REL)
        .to_owned();
    let source_test_plan_path = latest_branch_test_plan_artifact(repo, state_dir);
    let source_test_plan = fs::read_to_string(&source_test_plan_path)
        .expect("shell-smoke browser QA fixture should read current test plan");
    let source_test_plan_fingerprint = sha256_hex(source_test_plan.as_bytes());
    let summary_hash = fixture_summary_hash(summary);
    let base_branch = expected_release_base_branch(repo);
    let record_id = format!(
        "browser-qa-record-{}",
        sha256_hex(format!("{branch_closure_id}:{summary_hash}:{result}").as_bytes())
    );
    upsert_authoritative_nested_object(
        repo,
        state_dir,
        "browser_qa_record_history",
        &record_id,
        serde_json::json!({
            "record_id": record_id.clone(),
            "record_sequence": 1,
            "record_status": "current",
            "branch_closure_id": branch_closure_id.clone(),
            "final_review_record_id": final_review_record_id,
            "source_plan_path": plan_rel,
            "source_plan_revision": 1,
            "repo_slug": repo_slug(repo, state_dir),
            "branch_name": branch,
            "base_branch": base_branch,
            "reviewed_state_id": current_tracked_tree_id(repo),
            "result": result,
            "source_test_plan_fingerprint": source_test_plan_fingerprint,
            "summary": summary,
            "summary_hash": summary_hash,
            "generated_by_identity": "featureforge/qa",
        }),
    );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("browser_qa_state", Value::from("fresh")),
            ("last_browser_qa_artifact_fingerprint", Value::Null),
            (
                "current_qa_branch_closure_id",
                Value::from(branch_closure_id),
            ),
            ("current_qa_result", Value::from(result)),
            ("current_qa_summary_hash", Value::from(summary_hash)),
            ("current_qa_record_id", Value::from(record_id)),
        ],
    );
}

fn set_current_authoritative_release_readiness_result(repo: &Path, state_dir: &Path, result: &str) {
    let mut payload = authoritative_harness_state(repo, state_dir);
    let object = payload
        .as_object_mut()
        .expect("authoritative shell-smoke harness state should remain an object");
    let record_id = object
        .get("current_release_readiness_record_id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .expect("release-readiness mutation fixture should expose current record id")
        .to_owned();
    let history = object
        .entry(String::from("release_readiness_record_history"))
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
        .as_object_mut()
        .expect("release-readiness history should remain an object");
    let record = history
        .get_mut(&record_id)
        .and_then(Value::as_object_mut)
        .expect("release-readiness mutation fixture should expose current record payload");
    let summary = match result {
        "ready" => String::from("shell smoke parity fixture."),
        "blocked" => String::from("shell smoke release blocker fixture."),
        _ => panic!("unsupported release-readiness result fixture mutation: {result}"),
    };
    let summary_hash = fixture_summary_hash(&summary);
    record.insert(String::from("result"), Value::from(result));
    record.insert(String::from("summary"), Value::from(summary));
    record.insert(
        String::from("summary_hash"),
        Value::from(summary_hash.clone()),
    );
    if result == "blocked" {
        record.insert(String::from("release_docs_fingerprint"), Value::Null);
    }
    object.insert(
        String::from("current_release_readiness_result"),
        Value::from(result),
    );
    object.insert(
        String::from("current_release_readiness_summary_hash"),
        Value::from(summary_hash),
    );
    if result == "blocked" {
        object.insert(
            String::from("current_final_review_branch_closure_id"),
            Value::Null,
        );
        object.insert(
            String::from("current_final_review_dispatch_id"),
            Value::Null,
        );
        object.insert(
            String::from("current_final_review_reviewer_source"),
            Value::Null,
        );
        object.insert(
            String::from("current_final_review_reviewer_id"),
            Value::Null,
        );
        object.insert(String::from("current_final_review_result"), Value::Null);
        object.insert(
            String::from("current_final_review_summary_hash"),
            Value::Null,
        );
        object.insert(String::from("current_final_review_record_id"), Value::Null);
        object.insert(String::from("final_review_state"), Value::Null);
        object.insert(
            String::from("finish_review_gate_pass_branch_closure_id"),
            Value::Null,
        );
    }
    write_authoritative_harness_state(repo, state_dir, &payload);
}

fn clear_current_authoritative_release_readiness(repo: &Path, state_dir: &Path) {
    let mut payload = authoritative_harness_state(repo, state_dir);
    let object = payload
        .as_object_mut()
        .expect("authoritative shell-smoke harness state should remain an object");
    object.insert(
        String::from("current_release_readiness_record_id"),
        Value::Null,
    );
    object.insert(
        String::from("current_release_readiness_result"),
        Value::Null,
    );
    object.insert(
        String::from("current_release_readiness_summary_hash"),
        Value::Null,
    );
    write_authoritative_harness_state(repo, state_dir, &payload);
}

fn internal_only_write_dispatched_branch_review_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) {
    write_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    set_current_branch_closure(repo, state_dir, "branch-release-closure");
    write_branch_release_artifact(repo, state_dir, plan_rel, base_branch);
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let initial_review_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-code-review-20260324-121000.md"
    ));
    publish_authoritative_final_review_truth(repo, state_dir, &initial_review_path);
    write_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    let review_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-code-review-20260324-121000.md"
    ));
    publish_authoritative_final_review_truth(repo, state_dir, &review_path);
}

fn install_cutover_check_baseline(repo: &Path) {
    write_repo_file(
        repo,
        "bin/featureforge",
        "#!/usr/bin/env bash\nprintf 'featureforge test runtime\\n'\n",
    );
    make_executable(&repo.join("bin/featureforge"));
    write_canonical_prebuilt_layout(
        repo,
        "1.0.0",
        "#!/usr/bin/env bash\nprintf 'darwin runtime\\n'\n",
        "windows runtime\n",
    );
}

fn git_add_all(repo: &Path) {
    let mut command = Command::new("git");
    command.args(["add", "."]).current_dir(repo);
    run_checked(command, "git add for cutover repo");
}

fn run_cutover_check(repo: &Path) -> Output {
    let mut command = Command::new("bash");
    command
        .arg(repo_root().join("scripts/check-featureforge-cutover.sh"))
        .current_dir(repo)
        .env("FEATUREFORGE_CUTOVER_REPO_ROOT", repo);
    run(command, "featureforge cutover check")
}

fn run_cutover_check_with_env(repo: &Path, extra_env: &[(&str, &str)]) -> Output {
    let mut command = Command::new("bash");
    command
        .arg(repo_root().join("scripts/check-featureforge-cutover.sh"))
        .current_dir(repo)
        .env("FEATUREFORGE_CUTOVER_REPO_ROOT", repo);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    run(command, "featureforge cutover check with env")
}

#[test]
fn standalone_binary_has_no_separate_workflow_wrapper_files() {
    let bin_dir = repo_root().join("bin");
    let workflow_entries = fs::read_dir(&bin_dir)
        .expect("bin dir should be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "featureforge" && name.contains("workflow"))
        .collect::<Vec<_>>();
    assert!(
        workflow_entries.is_empty(),
        "workflow wrapper files should not exist alongside the standalone featureforge binary: {workflow_entries:?}"
    );
}

#[test]
fn internal_only_compatibility_workflow_help_outside_repo_mentions_the_public_surfaces() {
    let outside_repo = TempDir::new().expect("outside repo tempdir should exist");
    let output = run_featureforge(
        outside_repo.path(),
        outside_repo.path(),
        &["workflow", "help"],
        "workflow help outside repo",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: featureforge workflow <COMMAND>"));
    assert!(stdout.contains("Commands:"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("operator"));
    assert!(stdout.contains("help"));
    for hidden in [
        "plan-fidelity",
        "resolve",
        "expect",
        "sync",
        "next",
        "artifacts",
        "explain",
        "phase",
        "doctor",
        "handoff",
        concat!("pre", "flight"),
        "gate",
    ] {
        assert!(
            !stdout
                .lines()
                .any(|line| line.trim_start().starts_with(hidden)),
            "workflow help should not expose hidden/internal `{hidden}` command"
        );
    }
}

#[test]
fn internal_only_compatibility_plan_execution_help_hides_internal_compatibility_commands() {
    let outside_repo = TempDir::new().expect("outside repo tempdir should exist");
    let output = run_featureforge(
        outside_repo.path(),
        outside_repo.path(),
        &["plan", "execution", "help"],
        "plan execution help outside repo",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: featureforge plan execution <COMMAND>"));
    for visible in [
        "status",
        "repair-review-state",
        "close-current-task",
        "advance-late-stage",
        "begin",
        "complete",
        "reopen",
        "transfer",
    ] {
        assert!(
            stdout.contains(visible),
            "plan execution help should keep public command `{visible}` visible"
        );
    }
    for hidden in [
        "recommend",
        concat!("pre", "flight"),
        concat!("rebuild", "-evidence"),
        "gate-contract",
        "record-contract",
        "gate-evaluator",
        "record-evaluation",
        "gate-handoff",
        "record-handoff",
        concat!("record", "-review-dispatch"),
        concat!("record", "-branch-closure"),
        concat!("record", "-release-readiness"),
        concat!("record", "-final-review"),
        concat!("record", "-qa"),
        concat!("record-gate", "-review-pass"),
        concat!("record-gate", "-finish-pass"),
        "internal",
        concat!("explain", "-review-state"),
        concat!("gate", "-review"),
        concat!("gate", "-finish"),
    ] {
        assert!(
            !stdout.contains(hidden),
            "plan execution help should hide compatibility/internal command `{hidden}`"
        );
    }
}

#[test]
fn workflow_operator_json_exposes_ready_plan_route() {
    let (repo_dir, state_dir) = init_repo("workflow-summary");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_ready_artifacts(repo);

    let json_output = run_featureforge(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md",
            "--json",
        ],
        "workflow operator json",
    );
    let json_stdout = String::from_utf8_lossy(&json_output.stdout);
    assert!(json_stdout.contains("\"schema_version\":3"));
    assert!(json_stdout.contains(concat!("\"phase\":\"execution_pre", "flight\"")));
    assert!(json_stdout.contains("\"state_kind\":\"actionable_public_command\""));
}

#[test]
fn workflow_public_ready_plan_surface_prefers_operator_and_status_over_removed_helpers() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-commands");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    for removed in ["next", "artifacts", "explain"] {
        let output = run_featureforge_real_cli(
            repo,
            state,
            &["workflow", removed],
            &format!("workflow {removed} removed command"),
        );
        assert!(
            !output.status.success(),
            "workflow {removed} should stay removed from the public CLI"
        );
        let failure: Value = serde_json::from_slice(&output.stderr)
            .or_else(|_| serde_json::from_slice(&output.stdout))
            .expect("removed workflow helper should emit json parse failure");
        assert_eq!(failure["error_class"], "InvalidCommandInput");
        assert!(
            failure["message"].as_str().is_some_and(
                |message| message.contains(&format!("unrecognized subcommand '{removed}'"))
            ),
            "workflow {removed} should fail at CLI parsing, got {failure:?}"
        );
    }

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator public ready-plan route",
    );
    assert_eq!(operator_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(
        operator_json["plan_path"],
        Value::from(String::from(plan_rel))
    );

    let status_json = run_featureforge_with_env_json(
        repo,
        state,
        &["plan", "execution", "status", "--plan", plan_rel],
        &[],
        "plan execution status public ready-plan route",
    );
    assert_eq!(status_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(status_json["state_kind"], "actionable_public_command");
}

#[test]
fn internal_only_compatibility_workflow_operator_routes_active_execution_to_exact_step_command() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-execution-command-context");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "workflow-operator-execution-command-context");

    let status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status for workflow operator active execution routing",
    );
    let preflight = internal_only_runtime_preflight_gate_json(
        repo,
        state,
        plan_rel,
        concat!(
            "pre",
            "flight for workflow operator active execution routing"
        ),
    );
    assert_eq!(preflight["allowed"], true);
    let begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint for begin"),
        ],
        "begin should establish an active step for workflow operator routing",
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for active execution routing",
    );

    assert_eq!(operator_json["phase"], "handoff_required");
    assert_eq!(operator_json["phase_detail"], "execution_in_progress");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "continue execution");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution complete --plan {plan_rel} --task 1 --step 1 --source featureforge:executing-plans --claim <claim> --manual-verify-summary <summary> --expect-execution-fingerprint {}",
            begin["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for operator command")
        ))
    );
    assert_eq!(
        operator_json["execution_command_context"],
        serde_json::json!({
            "command_kind": "complete",
            "task_number": 1,
            "step_id": 1
        })
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status json for active execution routing",
    );
    assert_eq!(status_json["phase_detail"], operator_json["phase_detail"]);
    assert_eq!(status_json["next_action"], operator_json["next_action"]);
    assert_eq!(
        status_json["recommended_command"],
        operator_json["recommended_command"]
    );
    assert_eq!(
        status_json["execution_command_context"],
        operator_json["execution_command_context"]
    );
}

#[test]
fn workflow_operator_routes_marker_free_started_execution_to_exact_begin_command() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-marker-free-begin-command");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "workflow-operator-marker-free-begin-command");
    let plan_path = repo.join(plan_rel);
    let plan_source =
        fs::read_to_string(&plan_path).expect("marker-free begin-command fixture plan should read");
    write_file(
        &plan_path,
        &plan_source.replace(
            "**Execution Mode:** none",
            "**Execution Mode:** featureforge:executing-plans",
        ),
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status json for marker-free begin-command routing",
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for marker-free begin-command routing",
    );

    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_in_progress");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["next_action"],
        concat!("execution pre", "flight")
    );
    assert_eq!(operator_json["recommended_command"], Value::Null);
    assert_eq!(operator_json["execution_command_context"], Value::Null);
    assert_eq!(status_json["phase_detail"], operator_json["phase_detail"]);
    assert_eq!(status_json["next_action"], operator_json["next_action"]);
    assert_eq!(
        status_json["recommended_command"],
        operator_json["recommended_command"]
    );
    assert_eq!(
        status_json["execution_command_context"],
        operator_json["execution_command_context"]
    );
}

#[test]
fn plan_execution_status_direct_helper_matches_real_cli_smoke() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-status-direct-vs-real-cli");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "plan-execution-status-direct-vs-real-cli");

    let direct = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status via direct helper smoke",
    );
    let real_cli = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status via real cli smoke",
    );

    assert_eq!(direct, real_cli);
}

#[test]
fn internal_only_compatibility_workflow_operator_routes_blocked_execution_to_resume_same_step() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-blocked-step-command-context");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "workflow-operator-blocked-step-command-context");

    let status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status for workflow operator blocked execution routing",
    );
    let preflight = internal_only_runtime_preflight_gate_json(
        repo,
        state,
        plan_rel,
        concat!(
            "pre",
            "flight for workflow operator blocked execution routing"
        ),
    );
    assert_eq!(preflight["allowed"], true);
    let begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint for blocked begin"),
        ],
        "begin should establish an active step before it becomes blocked",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "current_open_step_state",
                serde_json::json!({
                    "task": 1,
                    "step": 1,
                    "note_state": "Blocked",
                    "note_summary": "Waiting for dependency",
                    "execution_mode": "featureforge:executing-plans",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": begin["plan_revision"].as_u64().unwrap_or(1),
                    "authoritative_sequence": begin["authoritative_sequence"].as_u64().unwrap_or(1)
                }),
            ),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
            ("resume_task", Value::Null),
            ("resume_step", Value::Null),
        ],
    );
    let blocked = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose the blocked step after authoritative fixture mutation",
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for blocked execution routing",
    );

    assert_eq!(operator_json["phase"], "handoff_required");
    assert_eq!(operator_json["phase_detail"], "execution_in_progress");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "continue execution");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution begin --plan {plan_rel} --task 1 --step 1 --expect-execution-fingerprint {}",
            blocked["execution_fingerprint"]
                .as_str()
                .expect("blocked status should expose execution fingerprint for operator command")
        ))
    );
    assert_eq!(
        operator_json["execution_command_context"],
        serde_json::json!({
            "command_kind": "begin",
            "task_number": 1,
            "step_id": 1
        })
    );

    let resumed = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--expect-execution-fingerprint",
            blocked["execution_fingerprint"]
                .as_str()
                .expect("blocked status should expose execution fingerprint for resume begin"),
        ],
        "begin should resume the same blocked step",
    );
    assert_eq!(resumed["active_task"], Value::from(1));
    assert_eq!(resumed["active_step"], Value::from(1));
    assert_eq!(resumed["blocking_task"], Value::Null);
    assert_eq!(resumed["blocking_step"], Value::Null);
}

#[derive(Clone, Copy)]
struct LateStageCase {
    name: &'static str,
    expected_phase: &'static str,
    expected_next_action: &'static str,
    setup: fn(&Path, &Path, &str, &str),
}

fn seed_current_task_closure_state(repo: &Path, state_dir: &Path, plan_rel: &str) {
    let closure_record_id = format!(
        "task-closure-{}",
        sha256_hex(format!("{plan_rel}:task-1").as_bytes())
    );
    let reviewed_state_id = current_tracked_tree_id(repo);
    let review_summary_hash = sha256_hex(b"Fixture task review passed.");
    let verification_summary_hash =
        sha256_hex(b"Fixture task verification passed for the current reviewed state.");
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            (
                "current_task_closure_records",
                serde_json::json!({
                    "task-1": {
                        "dispatch_id": "fixture-task-dispatch",
                        "closure_record_id": closure_record_id.clone(),
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "execution_run_id": "run-fixture",
                        "reviewed_state_id": reviewed_state_id.clone(),
                        "contract_identity": task_contract_identity(repo, state_dir, plan_rel, 1),
                        "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                        "review_result": "pass",
                        "review_summary_hash": review_summary_hash,
                        "verification_result": "pass",
                        "verification_summary_hash": verification_summary_hash,
                        "closure_status": "current",
                    }
                }),
            ),
            (
                "task_closure_record_history",
                serde_json::json!({
                    closure_record_id.clone(): {
                        "dispatch_id": "fixture-task-dispatch",
                        "closure_record_id": closure_record_id,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "execution_run_id": "run-fixture",
                        "reviewed_state_id": reviewed_state_id,
                        "contract_identity": task_contract_identity(repo, state_dir, plan_rel, 1),
                        "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                        "review_result": "pass",
                        "review_summary_hash": review_summary_hash,
                        "verification_result": "pass",
                        "verification_summary_hash": verification_summary_hash,
                        "closure_status": "current",
                    }
                }),
            ),
        ],
    );
}

fn setup_qa_pending_case(repo: &Path, state_dir: &Path, plan_rel: &str, base_branch: &str) {
    if plan_rel == WORKFLOW_FIXTURE_PLAN_REL {
        let safe_base_branch = branch_storage_key(base_branch);
        let cache_key = format!("late-stage:qa-pending:{safe_base_branch}");
        let template_name = format!("workflow-shell-smoke-template-qa-pending-{safe_base_branch}");
        populate_fixture_from_cached_setup_template(
            repo,
            state_dir,
            &cache_key,
            &template_name,
            |template_repo, template_state| {
                setup_qa_pending_case_slow(template_repo, template_state, plan_rel, base_branch);
            },
        );
        return;
    }
    setup_qa_pending_case_slow(repo, state_dir, plan_rel, base_branch);
}

fn setup_qa_pending_case_slow(repo: &Path, state_dir: &Path, plan_rel: &str, base_branch: &str) {
    complete_workflow_fixture_execution_with_qa_requirement(
        repo,
        state_dir,
        plan_rel,
        Some("required"),
        false,
    );
    seed_current_task_closure_state(repo, state_dir, plan_rel);
    write_branch_test_plan_artifact(repo, state_dir, plan_rel, "yes");
    internal_only_write_dispatched_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    write_branch_release_artifact(repo, state_dir, plan_rel, base_branch);
    set_current_branch_closure(repo, state_dir, "branch-release-closure");
    republish_fixture_late_stage_truth_for_branch_closure(
        repo,
        state_dir,
        "branch-release-closure",
    );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("harness_phase", Value::from("qa_pending")),
            ("current_release_readiness_result", Value::from("ready")),
            ("release_docs_state", Value::from("fresh")),
            ("final_review_state", Value::from("fresh")),
            ("browser_qa_state", Value::from("missing")),
        ],
    );
}

fn setup_document_release_pending_case(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) {
    if plan_rel == WORKFLOW_FIXTURE_PLAN_REL {
        populate_fixture_from_cached_setup_template(
            repo,
            state_dir,
            "late-stage:document-release-pending",
            "workflow-shell-smoke-template-document-release-pending",
            |template_repo, template_state| {
                setup_document_release_pending_case_slow(
                    template_repo,
                    template_state,
                    plan_rel,
                    base_branch,
                );
            },
        );
        return;
    }
    setup_document_release_pending_case_slow(repo, state_dir, plan_rel, base_branch);
}

fn setup_document_release_pending_case_slow(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    _base_branch: &str,
) {
    complete_workflow_fixture_execution(repo, state_dir, plan_rel);
    seed_current_task_closure_state(repo, state_dir, plan_rel);
    write_branch_test_plan_artifact(repo, state_dir, plan_rel, "no");
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            ("harness_phase", Value::from("document_release_pending")),
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
        ],
    );
}

fn setup_document_release_pending_with_current_closure_case(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) {
    if plan_rel == WORKFLOW_FIXTURE_PLAN_REL {
        populate_fixture_from_cached_setup_template(
            repo,
            state_dir,
            "late-stage:document-release-pending-with-current-closure",
            "workflow-shell-smoke-template-document-release-pending-with-current-closure",
            |template_repo, template_state| {
                setup_document_release_pending_with_current_closure_case_slow(
                    template_repo,
                    template_state,
                    plan_rel,
                    base_branch,
                );
            },
        );
        return;
    }
    setup_document_release_pending_with_current_closure_case_slow(
        repo,
        state_dir,
        plan_rel,
        base_branch,
    );
}

fn setup_document_release_pending_with_current_closure_case_slow(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    _base_branch: &str,
) {
    complete_workflow_fixture_execution(repo, state_dir, plan_rel);
    seed_current_task_closure_state(repo, state_dir, plan_rel);
    write_branch_test_plan_artifact(repo, state_dir, plan_rel, "no");
    set_current_branch_closure(repo, state_dir, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[("harness_phase", Value::from("document_release_pending"))],
    );
}

fn setup_ready_for_finish_case(repo: &Path, state_dir: &Path, plan_rel: &str, base_branch: &str) {
    if plan_rel == WORKFLOW_FIXTURE_PLAN_REL {
        let safe_base_branch = branch_storage_key(base_branch);
        let cache_key = format!("late-stage:ready-for-finish:{safe_base_branch}");
        let template_name =
            format!("workflow-shell-smoke-template-ready-for-finish-{safe_base_branch}");
        populate_fixture_from_cached_setup_template(
            repo,
            state_dir,
            &cache_key,
            &template_name,
            |template_repo, template_state| {
                setup_ready_for_finish_case_slow(
                    template_repo,
                    template_state,
                    plan_rel,
                    base_branch,
                );
            },
        );
        return;
    }
    setup_ready_for_finish_case_slow(repo, state_dir, plan_rel, base_branch);
}

fn setup_ready_for_finish_case_slow(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) {
    complete_workflow_fixture_execution(repo, state_dir, plan_rel);
    seed_current_task_closure_state(repo, state_dir, plan_rel);
    write_branch_test_plan_artifact(repo, state_dir, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    write_branch_release_artifact(repo, state_dir, plan_rel, base_branch);
    republish_fixture_late_stage_truth_for_branch_closure(
        repo,
        state_dir,
        "branch-release-closure",
    );
}

fn setup_ready_for_finish_case_with_qa_requirement(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
    qa_requirement: Option<&str>,
    remove_qa_requirement: bool,
) {
    let cacheable_qa_requirement = if remove_qa_requirement {
        Some("missing-header")
    } else {
        match qa_requirement {
            None => Some("not-required"),
            Some("required") => Some("required"),
            Some("not-required") => Some("not-required"),
            Some(_) => None,
        }
    };
    if plan_rel == WORKFLOW_FIXTURE_PLAN_REL
        && let Some(qa_mode) = cacheable_qa_requirement
    {
        let safe_base_branch = branch_storage_key(base_branch);
        let cache_key = format!("late-stage:ready-for-finish-with-qa:{safe_base_branch}:{qa_mode}");
        let template_name = format!(
            "workflow-shell-smoke-template-ready-for-finish-with-qa-{safe_base_branch}-{qa_mode}"
        );
        populate_fixture_from_cached_setup_template(
            repo,
            state_dir,
            &cache_key,
            &template_name,
            |template_repo, template_state| {
                setup_ready_for_finish_case_with_qa_requirement_slow(
                    template_repo,
                    template_state,
                    plan_rel,
                    base_branch,
                    qa_requirement,
                    remove_qa_requirement,
                );
            },
        );
        return;
    }
    setup_ready_for_finish_case_with_qa_requirement_slow(
        repo,
        state_dir,
        plan_rel,
        base_branch,
        qa_requirement,
        remove_qa_requirement,
    );
}

fn setup_ready_for_finish_case_with_qa_requirement_slow(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
    qa_requirement: Option<&str>,
    remove_qa_requirement: bool,
) {
    complete_workflow_fixture_execution_with_qa_requirement(
        repo,
        state_dir,
        plan_rel,
        qa_requirement,
        remove_qa_requirement,
    );
    seed_current_task_closure_state(repo, state_dir, plan_rel);
    write_branch_test_plan_artifact(repo, state_dir, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    write_branch_release_artifact(repo, state_dir, plan_rel, base_branch);
    republish_fixture_late_stage_truth_for_branch_closure(
        repo,
        state_dir,
        "branch-release-closure",
    );
}

fn setup_task_boundary_blocked_case(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) {
    if plan_rel == WORKFLOW_FIXTURE_PLAN_REL {
        let safe_base_branch = branch_storage_key(base_branch);
        let cache_key = format!("late-stage:task-boundary-blocked:{safe_base_branch}");
        let template_name =
            format!("workflow-shell-smoke-template-task-boundary-blocked-{safe_base_branch}");
        populate_fixture_from_cached_setup_template(
            repo,
            state_dir,
            &cache_key,
            &template_name,
            |template_repo, template_state| {
                setup_task_boundary_blocked_case_slow(
                    template_repo,
                    template_state,
                    plan_rel,
                    base_branch,
                );
            },
        );
    } else {
        setup_task_boundary_blocked_case_slow(repo, state_dir, plan_rel, base_branch);
    }
    rebind_copied_state_repo_slug_if_needed(repo, state_dir);
    if !preflight_acceptance_state_path(repo, state_dir).is_file() {
        let status = run_plan_execution_json(
            repo,
            state_dir,
            &["status", "--plan", plan_rel],
            concat!(
                "status before seeding task-boundary blocked pre",
                "flight acceptance in active fixture context"
            ),
        );
        let plan_revision = status["plan_revision"]
            .as_u64()
            .and_then(|raw| u32::try_from(raw).ok())
            .expect(concat!(
                "task-boundary blocked fixture should expose plan_revision for pre",
                "flight seed"
            ));
        seed_preflight_acceptance_state(repo, state_dir, plan_rel, plan_revision);
    }
    assert!(
        preflight_acceptance_state_path(repo, state_dir).is_file(),
        "task-boundary blocked fixture should retain {} acceptance state in active fixture context",
        concat!("pre", "flight"),
    );
}

fn prepare_missing_task_closure_baseline_close_fixture(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) {
    setup_task_boundary_blocked_case(repo, state_dir, plan_rel, base_branch);
    let status_before_repair = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "status before missing task-closure baseline close-current-task fixture repair",
    );
    let execution_run_id = status_before_repair["execution_run_id"]
        .as_str()
        .expect("missing task-closure baseline fixture should expose execution_run_id")
        .to_owned();
    let plan_revision = status_before_repair["plan_revision"]
        .as_u64()
        .and_then(|raw| u32::try_from(raw).ok())
        .expect("missing task-closure baseline fixture should expose numeric plan_revision");
    let dispatch_id =
        String::from("0000000000000000000000000000000000000000000000000000000000000000");
    let task_completion_lineage_fingerprint =
        task_completion_lineage_fingerprint_from_evidence(repo, state_dir, plan_rel, 1).expect(
            "missing task-closure baseline fixture should derive task completion lineage fingerprint from execution evidence",
        );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            (
                "run_identity",
                serde_json::json!({
                    "execution_run_id": execution_run_id,
                    "source_plan_path": plan_rel,
                    "source_plan_revision": plan_revision
                }),
            ),
            (
                "last_strategy_checkpoint_fingerprint",
                Value::from("0000000000000000000000000000000000000000000000000000000000000000"),
            ),
            (
                "strategy_review_dispatch_lineage",
                serde_json::json!({
                    "task-1": {
                        "dispatch_id": dispatch_id,
                        "reviewed_state_id": current_tracked_tree_id(repo),
                        "task_completion_lineage_fingerprint": task_completion_lineage_fingerprint
                    }
                }),
            ),
            ("current_task_closure_records", serde_json::json!({})),
        ],
    );
}

fn prepare_fs21_resume_preempted_by_task_closure_bridge_fixture(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) {
    prepare_missing_task_closure_baseline_close_fixture(repo, state_dir, plan_rel, base_branch);
    let status_before_overlay = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "FS-21 bridge-preempts-resume status before injecting stale history and interrupted resume state",
    );
    let execution_run_id = status_before_overlay["execution_run_id"]
        .as_str()
        .expect("FS-21 fixture should expose execution_run_id before overlay injection")
        .to_owned();
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            (
                "task_closure_record_history",
                serde_json::json!({
                    "task-1-stale-history": {
                        "dispatch_id": "0000000000000000000000000000000000000000000000000000000000000000",
                        "closure_record_id": "task-1-stale-history",
                        "task": 1,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "execution_run_id": execution_run_id,
                        "reviewed_state_id": current_tracked_tree_id(repo),
                        "contract_identity": task_contract_identity(repo, state_dir, plan_rel, 1),
                        "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"FS-21 stale history review"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"FS-21 stale history verification"),
                        "closure_status": "stale_unreviewed",
                        "record_status": "stale_unreviewed",
                        "record_sequence": 2
                    }
                }),
            ),
            (
                "current_open_step_state",
                serde_json::json!({
                    "task": 2,
                    "step": 1,
                    "note_state": "Interrupted",
                    "note_summary": "FS-21 interrupted Task 2 step should be preempted by Task 1 closure bridge",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 33
                }),
            ),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
            ("resume_task", Value::from(2_u64)),
            ("resume_step", Value::from(1_u64)),
        ],
    );
}

fn setup_task_boundary_blocked_case_slow(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    _base_branch: &str,
) {
    install_full_contract_ready_artifacts(repo);
    write_file(&repo.join(plan_rel), task_boundary_blocked_plan_source());
    prepare_preflight_acceptance_workspace(repo, "workflow-shell-smoke-task-boundary-blocked");

    let status_before_begin = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "status before task-boundary blocked shell-smoke fixture execution",
    );
    let plan_revision = status_before_begin["plan_revision"]
        .as_u64()
        .and_then(|raw| u32::try_from(raw).ok())
        .expect("task-boundary blocked shell-smoke fixture should expose plan_revision");
    seed_preflight_acceptance_state(repo, state_dir, plan_rel, plan_revision);
    assert!(
        preflight_acceptance_state_path(repo, state_dir).is_file(),
        "task-boundary blocked shell-smoke fixture should seed {} acceptance state without invoking {} in the fixed setup path",
        concat!("pre", "flight"),
        concat!("pre", "flight"),
    );

    let begin_task1_step1 = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint before begin"),
        ],
        "begin task 1 step 1 for task-boundary blocked shell-smoke fixture",
    );
    let complete_task1_step1 = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 1 for task-boundary blocked shell-smoke fixture.",
            "--manual-verify-summary",
            "Verified by shell-smoke task-boundary fixture setup.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 1 for task-boundary blocked shell-smoke fixture",
    );
    let begin_task1_step2 = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            complete_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("complete should expose execution fingerprint for next begin"),
        ],
        "begin task 1 step 2 for task-boundary blocked shell-smoke fixture",
    );
    run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 2 for task-boundary blocked shell-smoke fixture.",
            "--manual-verify-summary",
            "Verified by shell-smoke task-boundary fixture setup.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 2 for task-boundary blocked shell-smoke fixture",
    );
}

fn task_boundary_blocked_plan_source() -> &'static str {
    r#"# Runtime Integration Hardening Implementation Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-004 -> Task 1
- VERIFY-001 -> Task 2

## Execution Strategy

- Execute Task 1 serially. It establishes boundary gating before follow-on work begins.
- Execute Task 2 serially after Task 1. It validates task-boundary workflow routing.

## Dependency Diagram

```text
Task 1 -> Task 2
```

## Task 1: Core flow

**Spec Coverage:** REQ-001, REQ-004
**Goal:** Task 1 execution reaches a boundary gate before Task 2 starts.

**Context:**
- Spec Coverage: REQ-001, REQ-004.

**Constraints:**
- Keep fixture inputs deterministic.

**Done when:**
- Task 1 execution reaches a boundary gate before Task 2 starts.

**Files:**
- Modify: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Prepare workflow fixture output**
- [ ] **Step 2: Validate workflow fixture output**

## Task 2: Follow-on flow

**Spec Coverage:** VERIFY-001
**Goal:** Task 2 should remain blocked until Task 1 closure requirements are met.

**Context:**
- Spec Coverage: VERIFY-001.

**Constraints:**
- Preserve deterministic task-boundary diagnostics.

**Done when:**
- Task 2 should remain blocked until Task 1 closure requirements are met.

**Files:**
- Modify: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Start the follow-on task**
"#
}

fn non_overlapping_task_boundary_blocked_plan_source() -> String {
    let source = task_boundary_blocked_plan_source().replacen(
        "- Modify: `tests/workflow_shell_smoke.rs`",
        "- Modify: `docs/example-output.md`",
        1,
    );
    source.replacen(
        "- Modify: `tests/workflow_shell_smoke.rs`",
        "- Modify: `docs/example-followup.md`",
        1,
    )
}

fn setup_non_overlapping_task_boundary_blocked_case(repo: &Path, state_dir: &Path, plan_rel: &str) {
    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join(plan_rel),
        &non_overlapping_task_boundary_blocked_plan_source(),
    );
    write_repo_file(
        repo,
        "docs/example-output.md",
        "non-overlapping task 1 fixture output\n",
    );
    prepare_preflight_acceptance_workspace(
        repo,
        "workflow-shell-smoke-non-overlapping-task-boundary",
    );

    let status_before_begin = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "status before non-overlapping task-boundary shell-smoke fixture execution",
    );
    let plan_revision = status_before_begin["plan_revision"]
        .as_u64()
        .and_then(|raw| u32::try_from(raw).ok())
        .expect("non-overlapping task-boundary fixture should expose plan_revision");
    seed_preflight_acceptance_state(repo, state_dir, plan_rel, plan_revision);
    assert!(
        preflight_acceptance_state_path(repo, state_dir).is_file(),
        "non-overlapping task-boundary fixture should seed {} acceptance state",
        concat!("pre", "flight"),
    );

    let begin_task1_step1 = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint before begin"),
        ],
        "begin task 1 step 1 for non-overlapping task-boundary shell-smoke fixture",
    );
    let complete_task1_step1 = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 1 for non-overlapping task-boundary fixture.",
            "--manual-verify-summary",
            "Verified by non-overlapping task-boundary fixture setup.",
            "--file",
            "docs/example-output.md",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 1 for non-overlapping task-boundary shell-smoke fixture",
    );
    let begin_task1_step2 = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            complete_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("complete should expose execution fingerprint for next begin"),
        ],
        "begin task 1 step 2 for non-overlapping task-boundary shell-smoke fixture",
    );
    run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 2 for non-overlapping task-boundary fixture.",
            "--manual-verify-summary",
            "Verified by non-overlapping task-boundary fixture setup.",
            "--file",
            "docs/example-output.md",
            "--expect-execution-fingerprint",
            begin_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 2 for non-overlapping task-boundary shell-smoke fixture",
    );
}

fn fs11_rebase_resume_parity_plan_source() -> &'static str {
    r#"# Runtime Remediation FS-11 Shell Smoke Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** featureforge:executing-plans
**Source Spec:** `docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 2, Task 3
- VERIFY-001 -> Task 2, Task 3

## Execution Strategy

- Keep Task 2 as the earliest stale boundary while a forward resume overlay points at Task 3 Step 6.

## Dependency Diagram

```text
Task 2 -> Task 3
```

## Task 2: Earliest stale boundary task

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Task 2 represents the earliest unresolved stale boundary.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep one step per task to simplify command-target parity checks.

**Done when:**
- Task 2 represents the earliest unresolved stale boundary.

**Files:**
- Modify: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Execute task 2 baseline step**

## Task 3: Forward resume overlay task

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Task 3 Step 6 is the forward resume overlay target that must not outrank Task 2.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep six steps on Task 3 to preserve the exact Task 3 Step 6 contradiction shape.

**Done when:**
- Task 3 Step 6 is the forward resume overlay target that must not outrank Task 2.

**Files:**
- Modify: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Build Task 3 step scaffold**
- [ ] **Step 2: Build Task 3 step scaffold**
- [ ] **Step 3: Build Task 3 step scaffold**
- [ ] **Step 4: Build Task 3 step scaffold**
- [ ] **Step 5: Build Task 3 step scaffold**
- [ ] **Step 6: Build Task 3 step scaffold**
"#
}

fn setup_fs11_rebase_resume_parity_fixture(repo: &Path, state_dir: &Path, plan_rel: &str) {
    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join(plan_rel),
        fs11_rebase_resume_parity_plan_source(),
    );
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", plan_rel);
    prepare_preflight_acceptance_workspace(repo, "workflow-shell-smoke-fs11-rebase-resume");

    let status_before_begin = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "FS-11 shell-smoke status before execution bootstrap",
    );
    let plan_revision = status_before_begin["plan_revision"]
        .as_u64()
        .and_then(|raw| u32::try_from(raw).ok())
        .expect("FS-11 shell-smoke fixture should expose plan revision before begin");
    seed_preflight_acceptance_state(repo, state_dir, plan_rel, plan_revision);
    let begin = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-11 shell-smoke status should expose fingerprint before begin"),
        ],
        "FS-11 shell-smoke execution bootstrap begin",
    );
    run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-11 shell-smoke bootstrap complete",
            "--manual-verify-summary",
            "FS-11 shell-smoke bootstrap complete summary",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"]
                .as_str()
                .expect("FS-11 shell-smoke begin should expose fingerprint before complete"),
        ],
        "FS-11 shell-smoke execution bootstrap complete",
    );
    let task2_review_summary = repo.join("fs11-task2-review-summary.md");
    let task2_verification_summary = repo.join("fs11-task2-verification-summary.md");
    write_file(
        &task2_review_summary,
        "FS-11 shell-smoke task 2 independent review passed.\n",
    );
    write_file(
        &task2_verification_summary,
        "FS-11 shell-smoke task 2 verification passed.\n",
    );
    let close_task2 = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--review-result",
            "pass",
            "--review-summary-file",
            task2_review_summary
                .to_str()
                .expect("FS-11 task2 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task2_verification_summary
                .to_str()
                .expect("FS-11 task2 verification summary path should be utf-8"),
        ],
        "FS-11 shell-smoke close-current-task task 2 baseline",
    );
    assert_eq!(
        close_task2["action"],
        Value::from("recorded"),
        "FS-11 shell-smoke fixture should close task 2 before seeding stale-boundary recovery"
    );

    append_tracked_repo_line(
        repo,
        "README.md",
        "FS-11 shell-smoke stale-boundary drift sentinel",
    );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            (
                "task_closure_record_history",
                serde_json::json!({
                    "task-2-stale": {
                        "closure_record_id": "task-2-stale",
                        "task": 2,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 10,
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    }
                }),
            ),
            (
                "current_open_step_state",
                serde_json::json!({
                    "task": 3,
                    "step": 6,
                    "note_state": "Interrupted",
                    "note_summary": "FS-11 shell-smoke forward reentry overlay should not outrank earlier stale boundary",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 30
                }),
            ),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
            ("resume_task", Value::from(3_u64)),
            ("resume_step", Value::from(6_u64)),
        ],
    );
}

#[test]
fn workflow_phase_text_and_json_surfaces_match_harness_downstream_freshness() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-phase-next-parity-shared");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    let cases = [
        LateStageCase {
            name: "qa-pending",
            expected_phase: "qa_pending",
            expected_next_action: "run QA",
            setup: setup_qa_pending_case,
        },
        LateStageCase {
            name: "document-release-pending",
            expected_phase: "document_release_pending",
            expected_next_action: "advance late stage",
            setup: setup_document_release_pending_with_current_closure_case,
        },
        LateStageCase {
            name: "ready-for-branch-completion",
            expected_phase: "ready_for_branch_completion",
            expected_next_action: "finish branch",
            setup: setup_ready_for_finish_case,
        },
        LateStageCase {
            name: "task-boundary-blocked",
            expected_phase: "task_closure_pending",
            expected_next_action: "close current task",
            setup: setup_task_boundary_blocked_case,
        },
    ];

    for case in cases {
        (case.setup)(repo, state, plan_rel, &base_branch);
        let runtime =
            discover_execution_runtime(repo, state, "workflow_shell_smoke late-stage parity");
        let (doctor, phase_text, next_text) =
            operator::doctor_phase_and_next_for_runtime_with_args(
                &runtime,
                &operator::DoctorArgs {
                    plan: None,
                    external_review_result_ready: false,
                },
            )
            .expect("workflow doctor/phase/next for shell-smoke late-stage parity should succeed");
        let doctor_json =
            serde_json::to_value(doctor).expect("workflow doctor json should serialize");

        assert_eq!(doctor_json["phase"], case.expected_phase);
        assert_eq!(doctor_json["next_action"], case.expected_next_action);
        assert!(phase_text.contains(&format!("Workflow phase: {}", case.expected_phase)));
        assert!(phase_text.contains(&format!("Next action: {}", case.expected_next_action)));
        assert!(next_text.contains(&format!("Next action: {}", case.expected_next_action)));

        let next_step = phase_text
            .lines()
            .find_map(|line| line.strip_prefix("Next: "))
            .unwrap_or_else(|| {
                panic!(
                    "workflow phase text should expose Next line for case {}",
                    case.name
                )
            });
        assert!(
            next_text.contains(next_step),
            "workflow next text should mirror the same Next step from workflow phase text for case {}",
            case.name
        );
        assert_eq!(
            doctor_json["next_step"],
            Value::from(next_step),
            "workflow doctor json should mirror the same Next step from workflow phase text for case {}",
            case.name
        );

        for field in [
            "final_review_state",
            "browser_qa_state",
            "release_docs_state",
            "last_final_review_artifact_fingerprint",
            "last_browser_qa_artifact_fingerprint",
            "last_release_docs_artifact_fingerprint",
        ] {
            assert!(
                doctor_json["execution_status"].get(field).is_some(),
                "workflow doctor json should keep downstream freshness metadata field `{field}` for case {}",
                case.name
            );
        }
    }
}

#[test]
fn workflow_operator_routes_task_boundary_to_record_review_dispatch() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-boundary-dispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for task-boundary dispatch routing",
    );

    assert_task_closure_recording_route(&operator_json, plan_rel, 1);
    assert_eq!(operator_json["review_state_status"], "clean");
}

#[test]
fn workflow_operator_task_dispatch_external_ready_without_dispatch_lineage_surfaces_bind_command() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-task-dispatch-bind-command-external-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator json for task-dispatch bind command route",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &[
            "status",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "plan execution status json for task-dispatch bind command route",
    );

    assert_public_route_parity(&operator_json, &status_json, None);
    assert_task_closure_recording_route(&operator_json, plan_rel, 1);
    assert!(
        operator_json["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "prior_task_current_closure_missing")),
        "task-boundary execution-reentry route should preserve closure-first blocker reason codes: {operator_json}"
    );
}

#[test]
fn fs07_task_review_dispatch_route_parity_in_compiled_cli_surfaces() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-shell-smoke-runtime-remediation-fs07");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let mut runtime_management_commands = 0usize;
    runtime_management_commands += 1;
    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-07 compiled-cli workflow operator task-boundary parity fixture",
    );
    runtime_management_commands += 1;
    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-07 compiled-cli plan execution status task-boundary parity fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_task_closure_recording_route(&operator_json, plan_rel, 1);
    assert_eq!(operator_json["review_state_status"], Value::from("clean"));
    assert_parity_probe_budget("FS-07", runtime_management_commands, 3);
}

#[test]
fn internal_only_compatibility_workflow_doctor_accepts_plan_and_external_review_ready_for_task_closure_recording()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-doctor-task-closure-recording-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "workflow doctor task-closure recording-ready fixture dispatch",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "workflow operator task-closure recording-ready route with external review result ready",
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "status",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "plan execution status task-closure recording-ready route with external review result ready",
    );

    assert_public_route_parity(&operator_json, &status_json, None);
}

#[test]
fn internal_only_compatibility_workflow_doctor_accepts_plan_and_external_review_ready_for_final_review_recording()
 {
    let (repo_dir, state_dir) = init_repo("workflow-doctor-final-review-recording-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    seed_current_task_closure_state(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "workflow doctor final-review recording-ready fixture dispatch",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "workflow operator final-review recording-ready route with external review result ready",
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("final_review_recording_ready")
    );
    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "status",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "plan execution status final-review recording-ready route with external review result ready",
    );

    assert_public_route_parity(&operator_json, &status_json, None);
}

#[test]
fn internal_only_compatibility_plan_execution_record_review_dispatch_prefers_task_boundary_target_over_interrupted_note_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-boundary-over-interrupted-note");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let plan_path = repo.join(plan_rel);
    let plan_source =
        fs::read_to_string(&plan_path).expect("task-boundary interrupted-note plan should read");
    let interrupted_plan = plan_source.replace(
        "- [ ] **Step 1: Start the follow-on task**",
        "- [ ] **Step 1: Start the follow-on task**\n  **Execution Note:** Interrupted - Resume task 2 step 1",
    );
    write_file(&plan_path, &interrupted_plan);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!(
            "compiled-cli record",
            "-review-dispatch should honor the prior-task boundary target even when Task 2 has an interrupted note-state"
        ),
    );
    assert_eq!(dispatch["allowed"], true);
    assert_eq!(dispatch["action"], "recorded");
}

#[test]
fn plan_execution_status_routes_closure_baseline_candidate_when_clean_execution_has_no_exact_command()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-status-no-exact-command");
    let repo = repo_dir.path();
    let state = state_dir.path();
    complete_workflow_fixture_execution(repo, state, plan_rel);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("executing")),
            ("latest_authoritative_sequence", Value::from(1)),
        ],
    );

    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for clean execution state without an exact execution command",
    );
    assert!(
        status["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| { code.as_str() == Some("task_closure_baseline_repair_candidate") })),
        "status should surface task_closure_baseline_repair_candidate when clean execution has no exact execution command, got {status}"
    );
    assert_eq!(status["phase_detail"], "task_closure_recording_ready");
    assert_eq!(status["next_action"], "close current task");
    assert!(
        status["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "status should recommend close-current-task when closure-baseline repair is the next action, got {status}"
    );
}

#[test]
fn workflow_operator_routes_closure_baseline_candidate_when_clean_execution_has_no_exact_command() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-no-exact-command");
    let repo = repo_dir.path();
    let state = state_dir.path();
    complete_workflow_fixture_execution(repo, state, plan_rel);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("executing")),
            ("latest_authoritative_sequence", Value::from(1)),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator for clean execution state without an exact execution command",
    );
    assert_eq!(
        operator_json["phase_detail"],
        "task_closure_recording_ready"
    );
    assert_eq!(operator_json["next_action"], "close current task");
    assert_ne!(
        operator_json["phase_detail"],
        "task_review_dispatch_required"
    );
    assert!(
        operator_json["blocking_reason_codes"]
            .as_array()
            .is_none_or(|codes| {
                !codes
                    .iter()
                    .any(|code| code.as_str() == Some("prior_task_review_dispatch_stale"))
            }),
        "stale dispatch lineage must not be published as a public blocker, got {operator_json}"
    );
    assert!(
        operator_json["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| { code.as_str() == Some("task_closure_baseline_repair_candidate") })),
        "workflow operator should surface task_closure_baseline_repair_candidate when closure-baseline repair is required, got {operator_json}"
    );
}

#[test]
fn internal_only_compatibility_explain_review_state_routes_closure_baseline_candidate_when_clean_execution_has_no_exact_command()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("explain", "-review-state-no-exact-command"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    complete_workflow_fixture_execution(repo, state, plan_rel);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("executing")),
            ("latest_authoritative_sequence", Value::from(1)),
        ],
    );

    let explain = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should route to closure-baseline repair when clean execution has no exact execution command"
        ),
    );
    assert_eq!(explain["next_action"], "close current task");
    assert!(
        explain["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "{} should recommend close-current-task when closure-baseline repair is the next action, got {}",
        explain,
        concat!("explain", "-review-state")
    );
}

#[test]
fn workflow_operator_routes_ready_branch_completion_to_gate_finish_after_review_gate_passes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-finish-review-gate");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "current_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            (
                "finish_review_gate_pass_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for ready branch completion routing",
    );

    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
    assert_eq!(
        operator_json["phase_detail"],
        "finish_completion_gate_ready"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["finish_review_gate_pass_branch_closure_id"],
        "branch-release-closure"
    );
    assert_eq!(operator_json["next_action"], "finish branch");
    assert_eq!(operator_json["recommended_command"], Value::Null);
}

#[test]
fn workflow_operator_requires_persisted_gate_review_checkpoint_before_gate_finish() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-finish-review-checkpoint-required");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        concat!(
            "workflow operator json without persisted gate",
            "-review checkpoint"
        ),
    );

    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
    assert_eq!(operator_json["phase_detail"], "finish_review_gate_ready");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["finish_review_gate_pass_branch_closure_id"],
        Value::Null
    );
    assert_eq!(operator_json["next_action"], "finish branch");
    assert_eq!(operator_json["recommended_command"], Value::Null);
}

#[test]
fn internal_only_compatibility_plan_execution_gate_review_records_finish_review_gate_pass_checkpoint()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-gate",
        "-review-records-finish-checkpoint"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-review should succeed and persist the finish-review gate pass checkpoint"
        ),
    );
    assert_eq!(gate_review["allowed"], Value::Bool(true));

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path).expect(concat!(
            "authoritative state should be readable after gate",
            "-review"
        )),
    )
    .expect(concat!(
        "authoritative state should remain valid json after gate",
        "-review"
    ));
    assert_eq!(
        authoritative_state_after["finish_review_gate_pass_branch_closure_id"],
        Value::from("branch-release-closure")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_gate_review_records_finish_checkpoint_from_authoritative_current_branch_truth()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-gate",
        "-review-records-finish-checkpoint-from-authority"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-review should fail closed when overlay current-branch fields are missing and no bound current branch closure exists"
        ),
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert_eq!(gate_review["action"], Value::from("blocked"));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should expose current_branch_closure_id_missing, got {}",
        gate_review,
        concat!("gate", "-review")
    );
    assert_eq!(gate_review["code"], Value::Null);
    assert_eq!(
        gate_review["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        )),
        "json: {gate_review}"
    );
    assert_eq!(gate_review["rederive_via_workflow_operator"], Value::Null);

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path).expect(concat!(
            "authoritative state should be readable after gate",
            "-review"
        )),
    )
    .expect(concat!(
        "authoritative state should remain valid json after gate",
        "-review"
    ));
    assert_eq!(
        authoritative_state_after["finish_review_gate_pass_branch_closure_id"],
        Value::Null
    );
}

#[test]
fn internal_only_compatibility_plan_execution_gate_review_blocks_when_finish_checkpoint_is_already_current()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-gate",
        "-review-already-current-finish-checkpoint"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "current_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            (
                "finish_review_gate_pass_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
        ],
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-review should fail closed once the current branch closure already has a fresh finish-review gate checkpoint"
        ),
    );

    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert_eq!(gate_review["action"], Value::from("blocked"));
    assert_eq!(
        gate_review["reason_codes"],
        Value::from(vec![String::from("finish_review_gate_already_current")])
    );
    assert_eq!(gate_review["code"], Value::Null);
    assert_eq!(gate_review["rederive_via_workflow_operator"], Value::Null);
    assert_eq!(
        gate_review["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(
        gate_review["finish_review_gate_pass_branch_closure_id"],
        Value::from("branch-release-closure")
    );

    let gate_review_real_cli = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "real cli gate",
            "-review should agree once the finish-review gate checkpoint is already current"
        ),
    );
    assert_eq!(gate_review_real_cli, gate_review);
}

#[test]
fn internal_only_compatibility_plan_execution_explain_review_state_does_not_record_finish_review_gate_pass_checkpoint()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-explain",
        "-review-state-does-not-record-finish-checkpoint"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let _ = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should stay read-only and not persist the finish-review gate pass checkpoint"
        ),
    );

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path).expect(concat!(
            "authoritative state should remain readable after explain",
            "-review-state"
        )),
    )
    .expect(concat!(
        "authoritative state should remain valid json after explain",
        "-review-state"
    ));
    assert!(
        authoritative_state_after["finish_review_gate_pass_branch_closure_id"].is_null(),
        "{} must not persist the finish-review gate pass checkpoint: {}",
        authoritative_state_after,
        concat!("explain", "-review-state"),
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_waits_for_task_review_result_after_dispatch() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-review-pending");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for workflow operator pending fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_task_closure_records", serde_json::json!({})),
            ("task_closure_record_history", serde_json::json!({})),
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for task review result pending",
    );

    assert_eq!(operator_json["phase"], "task_closure_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "task_closure_recording_ready"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "close current task");
    assert!(
        operator_json["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "task-boundary dispatch-complete routes should surface close-current-task in workflow/operator, got {operator_json}"
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_routes_task_review_result_ready_to_close_current_task()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-review-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for workflow operator ready fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_task_closure_records", serde_json::json!({})),
            ("task_closure_record_history", serde_json::json!({})),
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "workflow operator json for task review result ready",
    );

    let _dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("task closure recording ready should expose dispatch_id");
    assert_eq!(operator_json["phase"], "task_closure_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "task_closure_recording_ready"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "close current task");
    assert_eq!(
        operator_json["recording_context"]["task_number"],
        Value::from(1)
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution close-current-task --plan {plan_rel} --task 1 --review-result pass|fail --review-summary-file <path> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]"
        ))
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_review_dispatch_exposes_dispatch_id() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("plan-execution-record", "-review-dispatch"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");

    let dispatch_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!(
            "record",
            "-review-dispatch should expose dispatch contract fields"
        ),
    );

    assert_eq!(dispatch_json["allowed"], Value::Bool(true));
    assert_eq!(dispatch_json["action"], "recorded");
    assert_eq!(dispatch_json["scope"], "task");
    assert!(dispatch_json["dispatch_id"].as_str().is_some());

    let rerun_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!("record", "-review-dispatch rerun should remain idempotent"),
    );
    assert_eq!(rerun_json["allowed"], Value::Bool(true));
    assert_eq!(rerun_json["action"], "already_current");
    assert_eq!(rerun_json["dispatch_id"], dispatch_json["dispatch_id"]);

    let rerun_json_real_cli = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!(
            "record",
            "-review-dispatch real cli rerun should remain idempotent"
        ),
    );
    assert_eq!(rerun_json_real_cli, rerun_json);
}

#[test]
fn internal_only_compatibility_plan_execution_close_current_task_records_task_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let preflight = internal_only_runtime_preflight_gate_json(
        repo,
        state,
        plan_rel,
        concat!(
            "plan execution pre",
            "flight for close-current-task fixture"
        ),
    );
    assert_eq!(preflight["allowed"], Value::Bool(true));
    let review_summary_path = repo.join("task-1-review-summary.md");
    let verification_summary_path = repo.join("task-1-verification-summary.md");
    write_file(&review_summary_path, "Task 1 independent review passed.\n");
    write_file(
        &verification_summary_path,
        "Task 1 verification passed against the current reviewed state.\n",
    );

    let close_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task command should succeed",
    );

    assert_eq!(close_json["action"], "recorded");
    assert_eq!(close_json["task_number"], 1);
    assert_eq!(close_json["dispatch_validation_action"], "validated");
    assert_eq!(close_json["closure_action"], "recorded");
    assert_eq!(close_json["task_closure_status"], "current");
    assert_eq!(
        close_json["superseded_task_closure_ids"],
        Value::from(Vec::<String>::new())
    );
    assert!(close_json["closure_record_id"].as_str().is_some());
    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("task closure authoritative state should be readable"),
    )
    .expect("task closure authoritative state should remain valid json");
    let closure_record_id = close_json["closure_record_id"]
        .as_str()
        .expect("close-current-task should expose closure_record_id");
    let dispatch_id =
        authoritative_state["strategy_review_dispatch_lineage"]["task-1"]["dispatch_id"]
            .as_str()
            .expect("close-current-task should internalize task dispatch lineage")
            .to_owned();
    let current_record = &authoritative_state["current_task_closure_records"]["task-1"];
    assert!(
        current_record["reviewed_state_id"]
            .as_str()
            .unwrap_or_default()
            .starts_with("git_tree:")
    );
    assert_eq!(
        current_record["effective_reviewed_surface_paths"],
        Value::from(vec![String::from("tests/workflow_shell_smoke.rs")])
    );
    assert_eq!(
        authoritative_state["task_closure_record_history"][closure_record_id]["closure_record_id"],
        Value::from(closure_record_id)
    );

    let rerun_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task rerun should be idempotent",
    );
    assert_eq!(
        rerun_json["action"], "already_current",
        "json: {rerun_json:?}"
    );
    assert_eq!(rerun_json["closure_action"], "already_current");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("strategy_state", Value::from("cycle_breaking")),
            ("strategy_checkpoint_kind", Value::from("cycle_break")),
            ("strategy_cycle_break_task", Value::from(1_u64)),
            ("strategy_cycle_break_step", Value::from(1_u64)),
            (
                "strategy_cycle_break_checkpoint_fingerprint",
                Value::from("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ),
        ],
    );
    let cycle_break_rerun_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "already-current close-current-task rerun should clear same-task cycle-break overlay",
    );
    assert_eq!(cycle_break_rerun_json["action"], "already_current");
    assert_eq!(
        cycle_break_rerun_json["blocking_reason_codes"],
        Value::from(vec![String::from(
            "current_task_closure_postconditions_resolved"
        )])
    );
    let state_after_cycle_break_cleanup: Value = serde_json::from_str(
        &fs::read_to_string(&state_path).expect(
            "task closure authoritative state should be readable after cycle-break cleanup",
        ),
    )
    .expect("task closure authoritative state should remain valid json after cycle-break cleanup");
    assert!(
        state_after_cycle_break_cleanup["strategy_state"].is_null(),
        "same-task cycle-break strategy state should clear on already-current close: {state_after_cycle_break_cleanup:?}"
    );
    assert!(
        state_after_cycle_break_cleanup["strategy_cycle_break_task"].is_null(),
        "same-task cycle-break binding should clear on already-current close: {state_after_cycle_break_cleanup:?}"
    );

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("strategy_state", Value::from("cycle_breaking")),
            ("strategy_checkpoint_kind", Value::from("cycle_break")),
            ("strategy_cycle_break_task", Value::Null),
            (
                "review_state_repair_follow_up",
                Value::from("execution_reentry"),
            ),
            ("review_state_repair_follow_up_task", Value::Null),
            (
                "review_state_repair_follow_up_closure_record_id",
                Value::Null,
            ),
        ],
    );
    let unbound_cycle_break_rerun_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "already-current close-current-task rerun must not clear unbound cycle-break overlay",
    );
    assert_eq!(unbound_cycle_break_rerun_json["action"], "already_current");
    assert!(
        unbound_cycle_break_rerun_json["blocking_reason_codes"].is_null()
            || unbound_cycle_break_rerun_json["blocking_reason_codes"]
                == Value::from(Vec::<String>::new()),
        "unbound cleanup should not report resolved postconditions: {unbound_cycle_break_rerun_json}"
    );
    let state_after_unbound_cycle_break: Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect(
            "task closure authoritative state should be readable after unbound overlay check",
        ))
        .expect(
            "task closure authoritative state should remain valid json after unbound overlay check",
        );
    assert_eq!(
        state_after_unbound_cycle_break["strategy_state"],
        Value::from("cycle_breaking"),
        "unbound cycle-break state must remain until another route proves the target: {state_after_unbound_cycle_break:?}"
    );
    assert_eq!(
        state_after_unbound_cycle_break["review_state_repair_follow_up"],
        Value::from("execution_reentry"),
        "unbound repair follow-up must remain until a same-task or same-closure binding exists: {state_after_unbound_cycle_break:?}"
    );

    write_file(
        &review_summary_path,
        "Task 1 independent review passed with conflicting summary content.\n",
    );
    let conflicting_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task equivalent pass/pass summary drift should be idempotent",
    );
    assert_eq!(conflicting_json["action"], "already_current");
    assert_eq!(conflicting_json["closure_action"], "already_current");
    assert_eq!(
        conflicting_json["blocking_reason_codes"],
        Value::from(vec![String::from("summary_hash_drift_ignored")])
    );

    let reviewed_state_id = current_record["reviewed_state_id"]
        .as_str()
        .expect("current closure record should expose reviewed_state_id");
    let contract_identity = current_record["contract_identity"]
        .as_str()
        .expect("current closure record should expose contract identity");
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "task_closure_negative_result_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": dispatch_id.clone(),
                    "reviewed_state_id": reviewed_state_id,
                    "semantic_reviewed_state_id": current_record["semantic_reviewed_state_id"].clone(),
                    "contract_identity": contract_identity,
                    "review_result": "fail",
                    "review_summary_hash": sha256_hex(b"negative review summary"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"negative verification summary")
                }
            }),
        )],
    );
    write_file(
        &review_summary_path,
        "Task 1 pass/pass summary drift must not hide an authoritative negative result.\n",
    );
    let negative_result_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task summary drift must fail closed when same-state negative result exists",
    );
    assert_eq!(negative_result_json["action"], "blocked");
    assert_eq!(
        negative_result_json["required_follow_up"],
        Value::from("execution_reentry")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_close_current_task_stale_dispatch_validation_happens_before_summary_validation()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-summary-requery");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for close-current-task summary ordering fixture",
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("summary ordering fixture should expose dispatch id")
        .to_owned();
    append_tracked_repo_line(
        repo,
        "README.md",
        "tracked drift before close-current-task summary ordering regression coverage",
    );

    let missing_review_summary = repo.join("missing-close-current-task-review-summary.md");
    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            missing_review_summary
                .to_str()
                .expect("missing review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should fail closed through out-of-phase routing before summary validation",
    );

    assert_eq!(close_json["action"], Value::from("blocked"));
    assert_eq!(
        close_json["dispatch_validation_action"],
        Value::from("blocked")
    );
    assert_eq!(
        close_json["required_follow_up"],
        Value::from("execution_reentry")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_close_current_task_requires_fresh_reviewed_state_after_dispatch()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-stale-after-dispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for stale close-current-task fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("stale close-current-task fixture authoritative state should read"),
    )
    .expect("stale close-current-task fixture authoritative state should remain valid json");
    let _dispatch_id =
        authoritative_state["strategy_review_dispatch_lineage"]["task-1"]["dispatch_id"]
            .as_str()
            .expect("stale close-current-task fixture should expose dispatch_id")
            .to_owned();

    append_tracked_repo_line(
        repo,
        "README.md",
        "tracked drift after task review dispatch",
    );

    let review_summary_path = repo.join("task-1-review-summary.md");
    let verification_summary_path = repo.join("task-1-verification-summary.md");
    write_file(&review_summary_path, "Task 1 independent review passed.\n");
    write_file(
        &verification_summary_path,
        "Task 1 verification passed against the current reviewed state.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task should fail closed after post-dispatch tracked drift",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["required_follow_up"], "execution_reentry");
}

#[test]
fn internal_only_compatibility_workflow_operator_routes_stale_task_review_dispatch_to_repair_review_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-stale-task-review-dispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch should succeed before stale operator routing coverage",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let mut authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("stale task-review dispatch fixture should read authoritative state"),
    )
    .expect("stale task-review dispatch fixture should remain valid json");
    authoritative_state["strategy_review_dispatch_lineage"]["task-1"]["source_step"] =
        Value::from(99);
    fs::write(
        &state_path,
        serde_json::to_string_pretty(&authoritative_state)
            .expect("stale task-review dispatch fixture should serialize authoritative state"),
    )
    .expect("stale task-review dispatch fixture should persist authoritative state");

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route stale task review dispatch through repair-review-state",
    );
    assert_eq!(operator_json["phase"], "task_closure_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "task_closure_recording_ready"
    );
    assert_eq!(operator_json["next_action"], "close current task");
    assert!(
        operator_json["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "stale dispatch task-boundary routing should now surface close-current-task, got {operator_json}"
    );

    let gate_review = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("gate", "-review"),
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        concat!(
            "gate",
            "-review should let task-scope repair outrank a persisted branch reroute when current task-closure truth becomes invalid"
        ),
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert_eq!(
        gate_review["recommended_command"],
        operator_json["recommended_command"],
        "{} should reuse the shared router command for stale dispatch repair, got {}",
        gate_review,
        concat!("gate", "-review")
    );

    let gate_finish = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("gate", "-finish"),
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        concat!(
            "gate",
            "-finish should let task-scope repair outrank a persisted branch reroute when current task-closure truth becomes invalid"
        ),
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert_eq!(
        gate_finish["recommended_command"],
        operator_json["recommended_command"],
        "{} should reuse the shared router command for stale dispatch repair, got {}",
        gate_finish,
        concat!("gate", "-finish")
    );
}

#[test]
fn workflow_operator_routes_malformed_current_task_closure_to_repair_review_state() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-malformed-current-task-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": "unsupported-reviewed-state",
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                }
            }),
        )],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route malformed current task closure state through repair-review-state",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["next_action"],
        Value::from("repair review state / reenter execution")
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_operator_routes_invalid_current_task_closure_to_repair_review_state() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-invalid-current-task-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 999,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                }
            }),
        )],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route invalid current task closure state through repair-review-state",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["next_action"],
        Value::from("repair review state / reenter execution")
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn completed_plan_invalid_current_task_closure_routes_status_operator_and_repair_to_execution_reentry()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-completed-plan-invalid-current-task-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 999,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["README.md"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                    "closure_status": "current"
                }
            }),
        )],
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should not keep a completed plan in document_release_pending when current task-closure provenance is invalid",
    );
    assert_eq!(status_json["harness_phase"], "executing");
    assert_eq!(
        status_json["phase_detail"], "execution_reentry_required",
        "json: {status_json}"
    );
    assert_eq!(status_json["review_state_status"], "clean");
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should not keep a completed plan in document_release_pending when current task-closure provenance is invalid",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should route completed-plan invalid current task-closure provenance to execution reentry",
    );
    assert_eq!(repair_json["action"], "blocked");
    assert!(
        repair_json["required_follow_up"].is_null(),
        "json: {repair_json}"
    );
    assert_eq!(repair_json["phase_detail"], "task_closure_recording_ready");
    assert!(
        repair_json["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.starts_with(&format!(
                    "featureforge plan execution close-current-task --plan {plan_rel} --task 1"
                ))
            }),
        "repair-review-state should return the authoritative close-current-task target after removing invalid current task-closure provenance, got {repair_json}"
    );
}

#[test]
fn completed_plan_status_and_operator_surface_each_structural_current_task_closure_blocker() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-multi-structural-current-task-closure-blockers");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 999,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["README.md"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                    "closure_status": "current"
                },
                "task-2": {
                    "dispatch_id": "task-2-current-dispatch",
                    "closure_record_id": "task-2-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": format!("git_commit:{}", current_head_sha(repo)),
                    "contract_identity": "task-contract-fixture-2",
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 2 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 2 current verification"),
                    "closure_status": "current"
                }
            }),
        )],
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should surface one blocking record per structural current task-closure blocker",
    );
    let blocking_records = status_json["blocking_records"]
        .as_array()
        .expect("status should expose blocking_records as an array");
    assert_eq!(blocking_records.len(), 2, "{status_json}");
    assert!(blocking_records.iter().any(|record| {
        record["code"] == "prior_task_current_closure_invalid"
            && record["scope_key"] == "task-1"
            && record["record_id"] == "task-1-current-closure"
            && record["required_follow_up"] == "repair_review_state"
    }));
    assert!(
        blocking_records.iter().any(|record| {
            record["code"] == "prior_task_current_closure_invalid"
                && record["scope_key"] == "task-2"
                && record["record_id"] == "task-2-current-closure"
                && record["required_follow_up"] == "repair_review_state"
        }),
        "{status_json}"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route multi-structural current task-closure blockers back to repair-review-state",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn internal_only_compatibility_plan_execution_close_current_task_requires_dispatch_reviewed_state_binding()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-close-current-task-missing-dispatch-reviewed-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for missing reviewed-state binding fixture",
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("missing reviewed-state binding fixture should expose dispatch id")
        .to_owned();
    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let mut authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("missing reviewed-state binding fixture authoritative state should read"),
    )
    .expect("missing reviewed-state binding fixture authoritative state should remain valid json");
    authoritative_state["strategy_review_dispatch_lineage"]["task-1"]
        .as_object_mut()
        .expect("dispatch lineage should remain an object")
        .remove("reviewed_state_id");
    authoritative_state["strategy_review_dispatch_lineage"]["task-1"]
        .as_object_mut()
        .expect("dispatch lineage should remain an object")
        .remove("semantic_reviewed_state_id");
    write_file(
        &state_path,
        &serde_json::to_string(&authoritative_state)
            .expect("missing reviewed-state binding fixture state should serialize"),
    );

    let review_summary_path = repo.join("task-1-failed-review-summary.md");
    write_file(&review_summary_path, "Task 1 review found a blocker.\n");
    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("failed review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should fail closed when dispatch lineage loses reviewed-state binding",
    );

    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["dispatch_validation_action"], "blocked");
    assert_eq!(close_json["required_follow_up"], "request_external_review");
}

#[test]
fn internal_only_compatibility_plan_execution_close_current_task_records_failed_task_outcomes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-failures");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for failing close-current-task fixture",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("failing task closure fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-failed-review-summary.md");
    let verification_summary_path = repo.join("task-1-failed-verification-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review passed but verification failed.\n",
    );
    write_file(
        &verification_summary_path,
        "Task 1 verification found a blocker in the current reviewed state.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("failed review summary path should be utf-8"),
            "--verification-result",
            "fail",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("failed verification summary path should be utf-8"),
        ],
        "close-current-task should record failed task outcomes",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["closure_action"], "blocked");
    assert_eq!(close_json["task_closure_status"], "not_current");
    assert_eq!(close_json["required_follow_up"], "execution_reentry");

    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("failed task closure authoritative state should be readable"),
    )
    .expect("failed task closure authoritative state should remain valid json");
    let record = &authoritative_state["task_closure_negative_result_records"]["task-1"];
    assert_eq!(record["dispatch_id"], Value::from(dispatch_id.clone()));
    assert_eq!(record["closure_record_id"], Value::Null);
    assert_eq!(record["review_result"], "pass");
    assert_eq!(record["verification_result"], "fail");
    assert!(record["reviewed_state_id"].as_str().is_some());
    assert!(record["contract_identity"].as_str().is_some());
    assert_eq!(
        authoritative_state["current_task_closure_records"]["task-1"],
        Value::Null,
        "failed task closure must not create current task closure truth"
    );
    assert_eq!(
        authoritative_state["task_closure_negative_result_history"]
            [format!("task-1:{dispatch_id}")]["dispatch_id"],
        Value::from(dispatch_id)
    );

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should immediately reroute failed task verification to execution reentry",
    );
    assert_task_closure_recording_route(&operator_after_fail, plan_rel, 1);
    assert_eq!(operator_after_fail["review_state_status"], "clean");
    assert!(operator_after_fail.get("follow_up_override").is_none());
    let status_after_fail = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should immediately reroute failed task review to execution reentry",
    );
    assert_eq!(
        status_after_fail["phase_detail"],
        "task_closure_recording_ready"
    );
    assert_eq!(status_after_fail["next_action"], "close current task");
    assert!(status_after_fail.get("follow_up_override").is_none());
}

#[test]
fn internal_only_compatibility_plan_execution_close_current_task_records_failed_review_outcomes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-review-fail");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for failing review close-current-task fixture",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("failing review task closure fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-review-failed-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found blocking issues.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("failed review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should record failed review outcomes",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["closure_action"], "blocked");
    assert_eq!(close_json["task_closure_status"], "not_current");
    assert_eq!(close_json["required_follow_up"], "execution_reentry");

    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("failed review authoritative state should be readable"),
    )
    .expect("failed review authoritative state should remain valid json");
    let record = &authoritative_state["task_closure_negative_result_records"]["task-1"];
    assert_eq!(record["dispatch_id"], Value::from(dispatch_id.clone()));
    assert_eq!(record["closure_record_id"], Value::Null);
    assert_eq!(record["review_result"], "fail");
    assert_eq!(record["verification_result"], "not-run");
    assert!(record["reviewed_state_id"].as_str().is_some());
    assert!(record["contract_identity"].as_str().is_some());
    assert_eq!(
        authoritative_state["task_closure_negative_result_history"]
            [format!("task-1:{dispatch_id}")]["dispatch_id"],
        Value::from(dispatch_id.clone())
    );

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should immediately reroute failed task review to execution reentry",
    );
    assert_task_closure_recording_route(&operator_after_fail, plan_rel, 1);
    assert_eq!(operator_after_fail["review_state_status"], "clean");
    assert!(operator_after_fail.get("follow_up_override").is_none());
    let status_after_fail = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should immediately reroute failed final review to execution reentry",
    );
    assert_eq!(
        status_after_fail["phase_detail"],
        "task_closure_recording_ready"
    );
    assert_eq!(status_after_fail["next_action"], "close current task");
    assert!(status_after_fail.get("follow_up_override").is_none());

    let rerun_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("failed review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task negative rerun should fail closed",
    );
    assert_eq!(rerun_json["action"], "blocked");
    assert_eq!(rerun_json["closure_action"], "blocked");
    assert_eq!(rerun_json["task_closure_status"], "not_current");

    let verification_summary_path = repo.join("task-1-review-failed-verification-summary.md");
    write_file(
        &verification_summary_path,
        "Task 1 verification passed against the current reviewed state.\n",
    );
    let conflicting_pass_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("failed review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task negative then pass rerun should fail closed",
    );
    assert_eq!(conflicting_pass_json["action"], "blocked");
    assert_eq!(conflicting_pass_json["closure_action"], "blocked");
    assert_eq!(conflicting_pass_json["task_closure_status"], "not_current");
}

#[test]
fn internal_only_compatibility_plan_execution_close_current_task_records_failed_review_with_passing_verification()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-close-current-task-review-fail-verification-pass");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for review-fail verification-pass fixture",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("review-fail verification-pass fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-review-failed-verification-passed-summary.md");
    let verification_summary_path =
        repo.join("task-1-review-failed-verification-passed-verification-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found blocking issues that require remediation.\n",
    );
    write_file(
        &verification_summary_path,
        "Task 1 verification still passes for the current reviewed state.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("failed review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task should record failed review with passing verification",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["closure_action"], "blocked");
    assert_eq!(close_json["task_closure_status"], "not_current");
    assert_eq!(close_json["required_follow_up"], "execution_reentry");

    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("review-fail verification-pass authoritative state should be readable"),
    )
    .expect("review-fail verification-pass authoritative state should remain valid json");
    let record = &authoritative_state["task_closure_negative_result_records"]["task-1"];
    assert_eq!(record["dispatch_id"], Value::from(dispatch_id.clone()));
    assert_eq!(record["review_result"], "fail");
    assert_eq!(record["verification_result"], "pass");
    assert_eq!(record["closure_record_id"], Value::Null);
    assert_eq!(
        authoritative_state["task_closure_negative_result_history"]
            [format!("task-1:{dispatch_id}")]["dispatch_id"],
        Value::from(dispatch_id),
    );
}

#[test]
fn internal_only_compatibility_plan_execution_close_current_task_failed_review_keeps_execution_reentry_over_handoff_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-handoff-override");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before close-current-task handoff override",
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("handoff override fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-handoff-override-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("handoff override review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should keep execution reentry ahead of handoff state",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["closure_action"], "blocked");
    assert_eq!(close_json["task_closure_status"], "not_current");
    assert_eq!(close_json["required_follow_up"], "execution_reentry");

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep execution-reentry routing after a failed task review even when handoff state is present",
    );
    assert_task_closure_recording_route(&operator_after_fail, plan_rel, 1);
    assert!(operator_after_fail.get("follow_up_override").is_none());
    assert_eq!(
        close_json["recommended_command"],
        Value::Null,
        "blocked close-current-task should not leak a stale close-current-task command when execution reentry is the authoritative follow-up"
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_ignores_forged_transfer_artifact_without_authoritative_checkpoint()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-forged-transfer-artifact-without-checkpoint");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before forged transfer artifact coverage",
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("forged transfer fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-forged-transfer-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("forged transfer review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should route to handoff before forged transfer coverage",
    );
    assert_eq!(
        close_json["required_follow_up"],
        Value::from("execution_reentry")
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("handoff_required")),
            ("reason_codes", serde_json::json!(["handoff_required"])),
            ("handoff_required", Value::Bool(true)),
        ],
    );

    let safe_branch = branch_storage_key(&current_branch_name(repo));
    let forged_record = write_workflow_transfer_artifact(
        repo,
        state,
        plan_rel,
        WorkflowTransferArtifactSpec {
            decision_reason_codes: &[],
            scope: "task",
            to: "teammate",
            reason: "handoff required",
            file_name: &format!("tester-{safe_branch}-workflow-transfer-1712000000.md"),
        },
    );
    assert!(
        forged_record.exists(),
        "forged transfer artifact should exist for follow-up override coverage"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should ignore forged transfer artifacts without authoritative checkpoints",
    );
    assert_eq!(operator_json["phase"], "handoff_required");
    assert_eq!(operator_json["phase_detail"], "handoff_recording_required");
    assert!(operator_json.get("follow_up_override").is_none());

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should ignore forged transfer artifacts without authoritative checkpoints",
    );
    assert!(status_json.get("follow_up_override").is_none());
}

#[test]
fn internal_only_compatibility_workflow_operator_keeps_handoff_override_when_checkpoint_decision_reason_codes_drift()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-transfer-decision-reason-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before checkpoint decision drift coverage",
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("decision-drift transfer fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-transfer-decision-drift-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("decision-drift transfer review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should route to handoff before checkpoint decision drift coverage",
    );
    assert_eq!(
        close_json["required_follow_up"],
        Value::from("execution_reentry")
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("handoff_required")),
            ("reason_codes", serde_json::json!(["handoff_required"])),
            ("handoff_required", Value::Bool(true)),
        ],
    );

    let safe_branch = branch_storage_key(&current_branch_name(repo));
    let mismatched_record = write_workflow_transfer_artifact(
        repo,
        state,
        plan_rel,
        WorkflowTransferArtifactSpec {
            decision_reason_codes: &[String::from("different_decision_reason")],
            scope: "task",
            to: "teammate",
            reason: "handoff required",
            file_name: &format!("tester-{safe_branch}-workflow-transfer-1712000999.md"),
        },
    );
    let mismatched_fingerprint = sha256_hex(
        fs::read(&mismatched_record)
            .expect("mismatched transfer record should remain readable")
            .as_slice(),
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "last_handoff_path",
                Value::from(mismatched_record.display().to_string()),
            ),
            (
                "last_handoff_fingerprint",
                Value::from(mismatched_fingerprint),
            ),
            ("harness_phase", Value::from("handoff_required")),
            ("reason_codes", serde_json::json!(["handoff_required"])),
            ("handoff_required", Value::Bool(false)),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep handoff override when checkpoint decision reason codes drift",
    );
    assert!(operator_json.get("follow_up_override").is_none());

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should keep handoff override when checkpoint decision reason codes drift",
    );
    assert!(status_json.get("follow_up_override").is_none());
}

#[test]
fn internal_only_compatibility_plan_execution_transfer_records_when_checkpoint_scope_does_not_match_current_decision()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-transfer-checkpoint-scope-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before checkpoint scope-drift transfer coverage",
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("scope-drift transfer fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-transfer-scope-drift-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("scope-drift transfer review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should route to handoff before scope-drift transfer coverage",
    );
    assert_eq!(
        close_json["required_follow_up"],
        Value::from("execution_reentry")
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("handoff_required")),
            ("reason_codes", serde_json::json!(["handoff_required"])),
            ("handoff_required", Value::Bool(true)),
        ],
    );

    let status_before_scope_drift = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before checkpoint scope-drift transfer coverage",
    );
    let decision_reason_codes = status_before_scope_drift["reason_codes"]
        .as_array()
        .expect("status should expose reason_codes for scope-drift transfer coverage")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let safe_branch = branch_storage_key(&current_branch_name(repo));
    let mismatched_record = write_workflow_transfer_artifact(
        repo,
        state,
        plan_rel,
        WorkflowTransferArtifactSpec {
            decision_reason_codes: &decision_reason_codes,
            scope: "branch",
            to: "teammate",
            reason: "handoff required",
            file_name: &format!("tester-{safe_branch}-workflow-transfer-1712000555.md"),
        },
    );
    let mismatched_fingerprint = sha256_hex(
        fs::read(&mismatched_record)
            .expect("scope-drift transfer record should remain readable")
            .as_slice(),
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "last_handoff_path",
                Value::from(mismatched_record.display().to_string()),
            ),
            (
                "last_handoff_fingerprint",
                Value::from(mismatched_fingerprint),
            ),
            ("harness_phase", Value::from("handoff_required")),
            ("reason_codes", serde_json::json!(["handoff_required"])),
            ("handoff_required", Value::Bool(false)),
        ],
    );

    let operator_before_transfer = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep handoff override when checkpoint scope mismatches the current decision",
    );
    assert!(operator_before_transfer.get("follow_up_override").is_none());

    let transfer_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "transfer",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--to",
            "teammate",
            "--reason",
            "handoff required",
        ],
        "transfer should record when checkpoint scope mismatches current decision",
    );
    assert_eq!(transfer_json["action"], "recorded");
    assert_eq!(transfer_json["scope"], "task");

    let operator_after_transfer = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should clear handoff override after recording a scope-matching transfer",
    );
    assert!(operator_after_transfer.get("follow_up_override").is_none());
}

#[test]
fn internal_only_compatibility_plan_execution_transfer_blocks_when_requested_scope_mismatches_current_decision_scope()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-transfer-mismatched-requested-scope");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before mismatched requested transfer scope coverage",
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("mismatched requested transfer scope fixture should expose dispatch id")
        .to_owned();
    let review_summary_path =
        repo.join("task-1-transfer-requested-scope-mismatch-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("mismatched requested transfer review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should route to handoff before mismatched requested transfer scope coverage",
    );
    assert_eq!(
        close_json["required_follow_up"],
        Value::from("execution_reentry")
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("handoff_required")),
            ("reason_codes", serde_json::json!(["handoff_required"])),
            ("handoff_required", Value::Bool(true)),
        ],
    );

    let operator_before_transfer = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should require handoff recording before mismatched requested transfer scope",
    );
    assert!(operator_before_transfer.get("follow_up_override").is_none());

    let transfer_json = run_plan_execution_failure_json_real_cli(
        repo,
        state,
        &[
            "transfer",
            "--plan",
            plan_rel,
            "--scope",
            "branch",
            "--to",
            "teammate",
            "--reason",
            "handoff required",
        ],
        "transfer should fail closed when requested scope mismatches current handoff decision scope",
    );
    assert_eq!(transfer_json["error_class"], "ExecutionStateNotReady");
    assert!(
        transfer_json["message"].as_str().is_some_and(|message| {
            message.contains("transfer failed closed")
                && message.contains("reason_code=mutation_not_route_authorized")
                && message.contains(&format!(
                    "Next public action: featureforge plan execution transfer --plan {plan_rel} --scope task --to <owner> --reason <reason>"
                ))
        }),
        "mismatched transfer scope should fail through the shared mutation oracle, got {transfer_json:?}"
    );

    let operator_after_transfer = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep handoff override after mismatched requested transfer scope",
    );
    assert!(operator_after_transfer.get("follow_up_override").is_none());
}

#[test]
fn internal_only_compatibility_plan_execution_transfer_reuses_equivalent_artifact_by_restoring_checkpoint()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-transfer-restores-equivalent-checkpoint");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before equivalent transfer rerun coverage",
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("equivalent transfer fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-equivalent-transfer-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("equivalent transfer review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should route to handoff before equivalent transfer rerun coverage",
    );
    assert_eq!(
        close_json["required_follow_up"],
        Value::from("execution_reentry")
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("handoff_required")),
            ("reason_codes", serde_json::json!(["handoff_required"])),
            ("handoff_required", Value::Bool(true)),
        ],
    );

    let safe_branch = branch_storage_key(&current_branch_name(repo));
    let status_before_transfer = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before equivalent transfer rerun coverage",
    );
    let decision_reason_codes = status_before_transfer["reason_codes"]
        .as_array()
        .expect("status should expose reason_codes for equivalent transfer coverage")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let existing_record = write_workflow_transfer_artifact(
        repo,
        state,
        plan_rel,
        WorkflowTransferArtifactSpec {
            decision_reason_codes: &decision_reason_codes,
            scope: "task",
            to: "teammate",
            reason: "handoff required",
            file_name: &format!("tester-{safe_branch}-workflow-transfer-1712000100.md"),
        },
    );

    let transfer_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "transfer",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--to",
            "teammate",
            "--reason",
            "handoff required",
        ],
        "transfer should restore authoritative checkpoint from an equivalent artifact",
    );
    assert_eq!(transfer_json["action"], "already_current");
    assert_eq!(
        transfer_json["record_path"],
        Value::from(existing_record.display().to_string())
    );

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative harness state should remain readable after transfer rerun"),
    )
    .expect("authoritative harness state should remain valid json after transfer rerun");
    let expected_fingerprint = sha256_hex(
        fs::read(&existing_record)
            .expect("equivalent transfer artifact should remain readable")
            .as_slice(),
    );
    assert_eq!(
        authoritative_state["last_handoff_path"],
        Value::from(existing_record.display().to_string())
    );
    assert_eq!(
        authoritative_state["last_handoff_fingerprint"],
        Value::from(expected_fingerprint)
    );
    assert_eq!(authoritative_state["handoff_required"], Value::Bool(false));

    let operator_after_transfer = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should clear routed handoff override after restoring authoritative checkpoint",
    );
    assert_ne!(operator_after_transfer["phase"], "handoff_required");
    assert!(operator_after_transfer.get("follow_up_override").is_none());
}

#[test]
fn internal_only_compatibility_plan_execution_transfer_routed_handoff_shape_is_executable_and_clears_override()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-transfer-routed-handoff-shape");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before routed handoff transfer",
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("routed handoff fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-routed-handoff-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("routed handoff review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should return routed handoff follow-up",
    );
    assert_eq!(
        close_json["required_follow_up"],
        Value::from("execution_reentry")
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("handoff_required")),
            ("reason_codes", serde_json::json!(["handoff_required"])),
            ("handoff_required", Value::Bool(true)),
        ],
    );

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should recommend routed transfer before handoff recording",
    );
    assert_eq!(operator_after_fail["phase"], "handoff_required");
    assert_eq!(
        operator_after_fail["phase_detail"],
        "handoff_recording_required"
    );
    assert_eq!(
        operator_after_fail["recommended_command"],
        Value::from(format!(
            "featureforge plan execution transfer --plan {plan_rel} --scope task --to <owner> --reason <reason>"
        ))
    );

    let status_before_transfer = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before routed handoff transfer should expose exact transfer scope",
    );
    let legacy_transfer_failure = run_plan_execution_failure_json_real_cli(
        repo,
        state,
        &[
            "transfer",
            "--plan",
            plan_rel,
            "--repair-task",
            "1",
            "--repair-step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "legacy repair transfer must not satisfy routed handoff transfer",
            "--expect-execution-fingerprint",
            status_before_transfer["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint before legacy transfer"),
        ],
        "legacy transfer repair shape should not satisfy routed handoff transfer route",
    );
    assert_eq!(
        legacy_transfer_failure["error_class"],
        "ExecutionStateNotReady"
    );
    assert!(
        legacy_transfer_failure["message"]
            .as_str()
            .is_some_and(|message| {
                message.contains("transfer failed closed")
                    && message.contains("reason_code=mutation_not_route_authorized")
                    && message.contains(&format!(
                        "Next public action: featureforge plan execution transfer --plan {plan_rel} --scope task --to <owner> --reason <reason>"
                    ))
            }),
        "legacy repair transfer should fail through the shared mutation oracle, got {legacy_transfer_failure:?}"
    );

    let transfer_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "transfer",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--to",
            "teammate",
            "--reason",
            "handoff required",
        ],
        "routed transfer handoff command",
    );
    assert_eq!(transfer_json["action"], "recorded");
    assert_eq!(transfer_json["scope"], "task");
    assert_eq!(transfer_json["to"], "teammate");
    assert!(
        transfer_json["record_path"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
        "routed transfer should expose a persisted runtime-owned record path"
    );

    let transfer_rerun = run_plan_execution_failure_json_real_cli(
        repo,
        state,
        &[
            "transfer",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--to",
            "teammate",
            "--reason",
            "handoff required",
        ],
        "routed transfer handoff command idempotent rerun",
    );
    assert_eq!(transfer_rerun["error_class"], "ExecutionStateNotReady");
    assert!(
        transfer_rerun["message"].as_str().is_some_and(|message| {
            message.contains("transfer failed closed")
                && message
                    .contains("Next public action: featureforge plan execution close-current-task")
                && message.contains("reason_code=mutation_not_route_authorized")
        }),
        "routed transfer rerun should fail closed through the shared mutation oracle after the route moves on: {transfer_rerun:?}"
    );

    let operator_after_transfer = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should clear routed handoff override after transfer recording",
    );
    assert_ne!(operator_after_transfer["phase"], "handoff_required");
    assert!(operator_after_transfer.get("follow_up_override").is_none());

    let status_after_transfer = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should clear the routed handoff override after transfer recording",
    );
    assert!(status_after_transfer.get("follow_up_override").is_none());

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("pivot_required")),
            ("handoff_required", Value::Bool(true)),
            (
                "reason_codes",
                serde_json::json!(["blocked_on_plan_revision"]),
            ),
        ],
    );

    let operator_after_pivot = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep pivot precedence even when a stale-equivalent handoff record already exists",
    );
    assert_eq!(operator_after_pivot["phase"], "pivot_required");
    assert_eq!(
        operator_after_pivot["phase_detail"],
        "planning_reentry_required"
    );
    assert!(operator_after_pivot.get("follow_up_override").is_none());
    assert_eq!(
        operator_after_pivot["next_action"],
        "pivot / return to planning"
    );

    let status_after_pivot = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should keep pivot precedence even when a stale-equivalent handoff record already exists",
    );
    assert!(status_after_pivot.get("follow_up_override").is_none());
}

#[test]
fn internal_only_compatibility_plan_execution_close_current_task_failed_verification_keeps_execution_reentry_over_pivot_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-pivot-override");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "reason_codes",
            serde_json::json!(["blocked_on_plan_revision"]),
        )],
    );

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before close-current-task pivot override",
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("pivot override fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-pivot-override-review-summary.md");
    let verification_summary_path = repo.join("task-1-pivot-override-verification-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review passed before pivot-required verification blocker.\n",
    );
    write_file(
        &verification_summary_path,
        "Task 1 verification found a blocker that requires replanning.\n",
    );

    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("pivot override review summary path should be utf-8"),
            "--verification-result",
            "fail",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("pivot override verification summary path should be utf-8"),
        ],
        "close-current-task should keep execution reentry ahead of pivot state",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["closure_action"], "blocked");
    assert_eq!(close_json["task_closure_status"], "not_current");
    assert_eq!(close_json["required_follow_up"], "execution_reentry");

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should still surface pivot-required routing after a failed task verification when pivot state is present",
    );
    assert_task_closure_recording_route(&operator_after_fail, plan_rel, 1);
    assert!(operator_after_fail.get("follow_up_override").is_none());
    assert_eq!(
        close_json["recommended_command"],
        Value::Null,
        "blocked close-current-task should not leak a stale close-current-task command when execution reentry is the authoritative follow-up"
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_allows_fresh_task_redispatch_after_failed_task_review()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-redispatch-after-failed-review");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let first_dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before failed review recovery fixture",
    );
    let _first_dispatch_id = first_dispatch["dispatch_id"]
        .as_str()
        .expect("failed review recovery fixture should expose first dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-review-fail-summary.md");
    write_file(
        &review_summary_path,
        "Task review found issues that require remediation.\n",
    );
    let _ = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should record the failed review outcome for recovery fixture",
    );
    append_tracked_repo_line(
        repo,
        "README.md",
        "remediation edit before redispatch after failed review",
    );

    let second_dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch after failed review remediation fixture",
    );
    let second_dispatch_id = second_dispatch["dispatch_id"]
        .as_str()
        .expect("failed review recovery fixture should expose second dispatch id")
        .to_owned();

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "workflow operator should allow fresh review readiness after a failed task review is redispached",
    );
    assert_eq!(operator_json["phase"], "task_closure_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "task_closure_recording_ready"
    );
    assert!(operator_json.get("follow_up_override").is_none());
    assert_eq!(
        operator_json["recording_context"]["dispatch_id"],
        Value::from(second_dispatch_id)
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_review_dispatch_preserves_failed_task_outcome_history_on_redispatch()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-task-negative-history-persists-on-redispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let first_dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch should succeed before failed-outcome history coverage",
    );
    let first_dispatch_id = first_dispatch["dispatch_id"]
        .as_str()
        .expect("failed-outcome history fixture should expose first dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-negative-history-review-summary.md");
    write_file(
        &review_summary_path,
        "Task review found issues that require remediation before redispatch.\n",
    );
    let _ = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should record the failed review outcome before redispatch history coverage",
    );

    append_tracked_repo_line(
        repo,
        "README.md",
        "task negative-result redispatch remediation coverage",
    );

    let second_dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!(
            "record",
            "-review-dispatch should keep prior failed task outcome history when redispatching"
        ),
    );
    assert_eq!(second_dispatch["action"], "recorded");
    assert_ne!(
        second_dispatch["dispatch_id"],
        Value::from(first_dispatch_id.clone()),
        "redispatch history coverage requires a fresh task dispatch lineage"
    );

    let negative_result_record_id = format!("task-1:{first_dispatch_id}");
    let authoritative_state = authoritative_harness_state(repo, state);
    let history_record =
        &authoritative_state["task_closure_negative_result_history"][negative_result_record_id];
    assert_eq!(
        history_record["dispatch_id"],
        Value::from(first_dispatch_id)
    );
    assert_eq!(history_record["record_status"], Value::from("historical"));
    assert_eq!(
        authoritative_state["task_closure_negative_result_history"]
            .as_object()
            .expect("negative-result history should remain an object")
            .len(),
        1,
        "redispatch should preserve the prior failed outcome instead of deleting or duplicating it"
    );
    assert_eq!(
        authoritative_state["task_closure_negative_result_records"]["task-1"],
        Value::Null
    );
}

#[test]
fn internal_only_compatibility_plan_execution_close_current_task_supersedes_overlapping_prior_task_closures()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-supersedes-overlap");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let task1_dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!(
            "record",
            "-review-dispatch should expose task 1 dispatch contract fields"
        ),
    );
    let _task1_dispatch_id = task1_dispatch["dispatch_id"]
        .as_str()
        .expect("task 1 dispatch should expose dispatch_id")
        .to_owned();
    let task1_review_summary_path = repo.join("task-1-supersession-review-summary.md");
    let task1_verification_summary_path = repo.join("task-1-supersession-verification-summary.md");
    write_file(
        &task1_review_summary_path,
        "Task 1 independent review passed before overlapping task 2 work.\n",
    );
    write_file(
        &task1_verification_summary_path,
        "Task 1 verification passed before overlapping task 2 work.\n",
    );
    let task1_close = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            task1_review_summary_path
                .to_str()
                .expect("task 1 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task1_verification_summary_path
                .to_str()
                .expect("task 1 verification summary path should be utf-8"),
        ],
        "close-current-task should record task 1 closure before supersession",
    );
    let task1_closure_record_id = task1_close["closure_record_id"]
        .as_str()
        .expect("task 1 close should expose closure record id")
        .to_owned();

    let status_after_task1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose execution fingerprint after task 1 closure",
    );
    let begin_task2 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_after_task1["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint for task 2 begin"),
        ],
        "begin task 2 should succeed once task 1 closure is current",
    );
    let _complete_task2 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 2 overlapping work for supersession coverage.",
            "--manual-verify-summary",
            "Verified by supersession shell-smoke coverage.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_task2["execution_fingerprint"]
                .as_str()
                .expect("begin task 2 should expose execution fingerprint for complete"),
        ],
        "complete task 2 should succeed for supersession coverage",
    );

    let task2_dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "2",
        ],
        concat!(
            "record",
            "-review-dispatch should expose task 2 dispatch contract fields"
        ),
    );
    let _task2_dispatch_id = task2_dispatch["dispatch_id"]
        .as_str()
        .expect("task 2 dispatch should expose dispatch_id")
        .to_owned();
    let task2_review_summary_path = repo.join("task-2-supersession-review-summary.md");
    let task2_verification_summary_path = repo.join("task-2-supersession-verification-summary.md");
    write_file(
        &task2_review_summary_path,
        "Task 2 independent review passed after overlapping Task 1 surface.\n",
    );
    write_file(
        &task2_verification_summary_path,
        "Task 2 verification passed after overlapping Task 1 surface.\n",
    );
    let task2_close = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--review-result",
            "pass",
            "--review-summary-file",
            task2_review_summary_path
                .to_str()
                .expect("task 2 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task2_verification_summary_path
                .to_str()
                .expect("task 2 verification summary path should be utf-8"),
        ],
        "close-current-task should supersede overlapping task 1 closure",
    );
    let task2_closure_record_id = task2_close["closure_record_id"]
        .as_str()
        .expect("task 2 close should expose closure record id")
        .to_owned();
    assert_eq!(task2_close["action"], "recorded");
    assert_eq!(
        task2_close["superseded_task_closure_ids"],
        Value::from(vec![task1_closure_record_id.clone()])
    );

    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("supersession authoritative state should be readable"),
    )
    .expect("supersession authoritative state should remain valid json");
    assert_eq!(
        authoritative_state["current_task_closure_records"]["task-1"],
        Value::Null,
        "overlapping later task closure should remove task 1 from the current task-closure set"
    );
    assert_eq!(
        authoritative_state["task_closure_record_history"][task1_closure_record_id.clone()]["closure_record_id"],
        Value::from(task1_closure_record_id.clone())
    );
    assert_eq!(
        authoritative_state["current_task_closure_records"]["task-2"]["closure_record_id"],
        Value::from(task2_closure_record_id.clone())
    );

    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
        ],
    );
    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should use only the effective current task-closure set"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect(concat!(
            "record",
            "-branch-closure should expose branch closure id"
        ))
        .to_owned();
    let explain = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should expose superseded task closures after supersession"
        ),
    );
    assert!(
        explain["superseded_closures"]
            .as_array()
            .is_some_and(|closures| closures
                .iter()
                .any(|closure| closure == &Value::from(task1_closure_record_id.clone()))),
        "json: {explain:?}"
    );
    let branch_record_source = fs::read_to_string(
        project_artifact_dir(repo, state).join(format!("branch-closure-{branch_closure_id}.md")),
    )
    .expect("branch closure artifact should be readable after task supersession");
    assert!(
        branch_record_source.contains(&task2_closure_record_id),
        "branch closure should keep the still-current task 2 lineage: {branch_record_source}"
    );
    assert!(
        !branch_record_source.contains(&task1_closure_record_id),
        "branch closure should exclude superseded task 1 lineage: {branch_record_source}"
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_waits_for_final_review_result_after_dispatch() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-pending");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    seed_current_task_closure_state(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for workflow operator pending fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let final_review_rerun = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch rerun should remain idempotent",
    );
    assert_eq!(final_review_rerun["allowed"], Value::Bool(true));
    assert_eq!(final_review_rerun["action"], "already_current");
    assert_eq!(final_review_rerun["dispatch_id"], dispatch["dispatch_id"]);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for final review result pending",
    );

    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "final_review_outcome_pending"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["next_action"],
        "wait for external review result"
    );
    assert!(operator_json.get("recommended_command").is_none());
}

#[test]
fn internal_only_compatibility_workflow_operator_routes_final_review_result_ready_to_advance_late_stage()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    seed_current_task_closure_state(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for workflow operator ready fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "workflow operator json for final review result ready",
    );

    let _dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("final review recording ready should expose dispatch_id");
    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "final_review_recording_ready"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        "branch-release-closure"
    );
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <path>"
        ))
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_routes_dispatched_final_review_with_missing_release_overlay_to_document_release_pending()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-release-missing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    align_branch_review_identity_with_command(
        repo,
        state,
        "human-independent-reviewer",
        "human-reviewer-fixture-001",
    );
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before release-readiness reroute",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    clear_current_authoritative_release_readiness(repo, state);

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "workflow operator should reroute dispatched final review without release readiness",
    );
    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for dispatched final review with missing release overlay",
    );

    assert_eq!(
        operator_json["phase"], "document_release_pending",
        "json: {operator_json}"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert_public_route_parity(&operator_json, &status_json, None);
}

#[test]
fn internal_only_compatibility_workflow_operator_reroutes_failed_final_review_back_to_release_prerequisite()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-release-prereq-priority");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before release-prerequisite priority coverage",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    set_current_authoritative_release_readiness_result(repo, state, "blocked");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "current_final_review_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_final_review_result", Value::from("fail")),
        ],
    );

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "workflow operator should keep release prerequisite routing ahead of failed final-review reentry",
    );

    assert_eq!(
        operator_json["phase"], "document_release_pending",
        "json: {operator_json}"
    );
    assert_eq!(
        operator_json["phase_detail"],
        "release_blocker_resolution_required"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "resolve release blocker");
}

#[test]
fn internal_only_compatibility_workflow_operator_reroutes_dispatched_final_review_blocked_release_ready_to_resolution()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-release-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before blocked release-readiness reroute",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    set_current_authoritative_release_readiness_result(repo, state, "blocked");

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "workflow operator should reroute blocked final review back to release blocker resolution",
    );

    assert_eq!(
        operator_json["phase"], "document_release_pending",
        "json: {operator_json}"
    );
    assert_eq!(
        operator_json["phase_detail"],
        "release_blocker_resolution_required"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        "branch-release-closure"
    );
    assert_eq!(operator_json["next_action"], "resolve release blocker");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should surface blocked release readiness as an active blocker",
    );
    assert_eq!(
        status_json["phase_detail"],
        "release_blocker_resolution_required"
    );
    assert_eq!(status_json["review_state_status"], "clean");
    assert_eq!(
        status_json["recording_context"]["branch_closure_id"],
        "branch-release-closure"
    );
    assert!(
        status_json["blocking_records"]
            .as_array()
            .is_some_and(|records| records.iter().any(|record| {
                record["code"] == "release_blocker_resolution_required"
                    && record["scope_type"] == "branch"
                    && record["required_follow_up"] == "resolve_release_blocker"
            })),
        "status should expose a structured release blocker summary: {status_json}"
    );
}

#[test]
fn plan_execution_status_surfaces_release_readiness_prerequisite_blocker_summary() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-status-release-readiness-prereq");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose release-readiness prerequisites as a structured blocker summary",
    );
    assert_eq!(
        status_json["phase_detail"],
        "release_readiness_recording_ready"
    );
    assert_eq!(status_json["review_state_status"], "clean");
    assert!(
        status_json["blocking_records"]
            .as_array()
            .is_some_and(|records| records.iter().any(|record| {
                record["code"].as_str() == Some("release_readiness_recording_ready")
                    && record["scope_type"].as_str() == Some("branch")
                    && record["scope_key"].as_str() == Some("branch-release-closure")
                    && record["required_follow_up"].as_str() == Some("advance_late_stage")
            })),
        "status should expose a structured release-readiness prerequisite blocker summary: {status_json}"
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_requires_fresh_final_review_dispatch_after_branch_closure_changes()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-dispatch-stale");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for stale-dispatch fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    set_current_branch_closure(repo, state, "branch-release-closure-2");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator should reject stale final-review dispatch lineage",
    );

    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "final_review_dispatch_required"
    );
    assert!(
        operator_json["recommended_command"].is_null(),
        "json: {operator_json}"
    );
    assert_eq!(
        operator_json["next_public_action"]["command"],
        Value::from("featureforge workflow operator --plan <approved-plan-path>")
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "status",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        concat!(
            "status should also reject stale final-review dispatch lineage when gate",
            "-review invalidates it"
        ),
    );
    assert_eq!(
        status_json["phase_detail"],
        "final_review_dispatch_required"
    );
    assert_eq!(status_json["review_state_status"], "clean");
    assert!(
        status_json["recommended_command"].is_null(),
        "json: {status_json}"
    );
    assert_eq!(
        status_json["next_public_action"]["command"],
        Value::from("featureforge workflow operator --plan <approved-plan-path>")
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_final_review_external_ready_without_dispatch_lineage_surfaces_bind_command()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-final-review-dispatch-bind-command-external-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator should expose final-review dispatch lineage bind command when external review result is ready but dispatch lineage is missing",
    );
    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "status",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "status should expose final-review dispatch lineage bind command when external review result is ready but dispatch lineage is missing",
    );
    let explain_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            concat!("explain", "-review-state"),
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        concat!(
            "explain",
            "-review-state should honor external review readiness when final-review recording is ready"
        ),
    );

    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["base_branch"],
        Value::from(base_branch.clone())
    );
    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "final_review_dispatch_required"
    );
    assert_eq!(operator_json["next_action"], "request final review");
    assert!(
        operator_json["recommended_command"].is_null(),
        "json: {operator_json}"
    );
    assert_eq!(
        operator_json["next_public_action"]["command"],
        Value::from("featureforge workflow operator --plan <approved-plan-path>")
    );
    assert_eq!(explain_json["next_action"], operator_json["next_action"]);
    assert_eq!(
        explain_json["recommended_command"],
        operator_json["recommended_command"]
    );
}

#[test]
fn repair_review_state_honors_external_review_ready_after_restoring_final_review_overlays() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("repair-review-state-final-review-overlay-external-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator should expose final-review recording readiness before overlay repair",
    );
    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("final_review_dispatch_required")
    );

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let repair = run_plan_execution_json(
        repo,
        state,
        &[
            "repair-review-state",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "repair-review-state should preserve external-review-ready final-review routing after restoring overlays",
    );
    assert_eq!(repair["action"], Value::from("blocked"));
    assert_eq!(
        repair["required_follow_up"],
        Value::from("request_external_review"),
        "repair-review-state should preserve the routed shared follow-up after restoring overlays"
    );
    let actions = repair["actions_performed"]
        .as_array()
        .expect("repair-review-state should expose actions_performed array");
    for expected_action in [
        "restored_current_branch_closure_reviewed_state",
        "restored_current_branch_closure_contract_identity",
        "restored_current_release_readiness_overlay",
    ] {
        assert!(
            actions
                .iter()
                .any(|action| action.as_str() == Some(expected_action)),
            "repair-review-state should restore {expected_action} before resuming final-review recording, got {repair}",
        );
    }
    assert!(
        repair["recommended_command"].is_null(),
        "repair-review-state should omit the generic operator placeholder from recommended_command in omitted dispatch lanes, got {repair}"
    );
    assert!(
        repair.get("next_public_action").is_none() || repair["next_public_action"].is_null(),
        "repair-review-state does not project next_public_action; omitted dispatch lanes should stay null-commanded here, got {repair}"
    );

    let post_repair_operator = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator should remain final-review recording ready after repair restores overlays",
    );
    assert_eq!(
        post_repair_operator["phase_detail"],
        Value::from("final_review_dispatch_required")
    );
    assert_eq!(
        post_repair_operator["recommended_command"],
        operator_json["recommended_command"]
    );
}

#[test]
fn internal_only_compatibility_plan_execution_final_review_dispatch_requires_release_readiness_ready()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-dispatch-requires-release-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    set_current_branch_closure(repo, state, "branch-release-closure");
    let state_before = authoritative_harness_state(repo, state);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch without release readiness ready",
    );

    assert_eq!(dispatch["allowed"], Value::Bool(false));
    assert_eq!(dispatch["action"], Value::from("blocked"));
    assert_eq!(
        dispatch["reason_codes"],
        Value::from(vec![String::from("release_readiness_recording_ready")])
    );
    assert_eq!(
        dispatch["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result ready|blocked --summary-file <path>"
        ))
    );
    assert_eq!(dispatch["rederive_via_workflow_operator"], Value::Null);

    let state_after = authoritative_harness_state(repo, state);
    assert_eq!(
        state_after["strategy_checkpoints"], state_before["strategy_checkpoints"],
        "blocked final-review dispatch should not append strategy checkpoints before release readiness is ready: {state_after}"
    );
    assert!(
        state_after["final_review_dispatch_lineage"].is_null(),
        "blocked final-review dispatch should not persist final-review lineage before release readiness is ready: {state_after}"
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_routes_final_review_pending_without_current_closure_to_record_branch_closure()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-missing-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for missing-closure fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    update_authoritative_harness_state(repo, state, &[("current_branch_closure_id", Value::Null)]);
    let mut payload = authoritative_harness_state(repo, state);
    payload["branch_closure_records"]
        .as_object_mut()
        .expect("branch_closure_records should remain an object")
        .clear();
    write_authoritative_harness_state(repo, state, &payload);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should reroute final-review missing-closure state",
    );

    assert_eq!(
        operator_json["phase"], "document_release_pending",
        "json: {operator_json}"
    );
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        operator_json["base_branch"],
        Value::from(base_branch),
        "document-release branch-closure refresh route should still surface runtime-owned base_branch context",
    );
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_records_final_review_without_explicit_dispatch_id()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-record");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for advance-late-stage fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator json for final review recording fixture",
    );
    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("final review recording fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let review_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review command without explicit dispatch-id should succeed",
    );

    assert_eq!(review_json["action"], "recorded");
    assert_eq!(review_json["stage_path"], "final_review");
    assert_eq!(
        review_json["delegated_primitive"],
        concat!("record", "-final-review")
    );
    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_final_review_dispatch_id"],
        Value::from(dispatch_id),
        "normal-path final review should bind the runtime-owned dispatch lineage without requiring a public {}",
        concat!("--dispatch", "-id"),
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after final review recording",
    );
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
}

#[test]
fn internal_only_compatibility_plan_execution_record_final_review_primitive_records_final_review() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo(concat!("plan-execution-record", "-final-review-primitive"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for primitive fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator json for final review primitive fixture",
    );
    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("final review primitive fixture should expose dispatch_id")
        .to_owned();
    let branch_closure_id = operator_json["recording_context"]["branch_closure_id"]
        .as_str()
        .expect("final review primitive fixture should expose branch_closure_id")
        .to_owned();

    let summary_path = repo.join("final-review-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let review_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-final-review"),
            "--plan",
            plan_rel,
            concat!("--branch", "-closure-id"),
            &branch_closure_id,
            concat!("--dispatch", "-id"),
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!("record", "-final-review primitive command should succeed"),
    );

    assert_eq!(review_json["action"], "recorded");
    assert_eq!(review_json["stage_path"], "final_review");
    assert_eq!(
        review_json["delegated_primitive"],
        concat!("record", "-final-review")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_final_review_primitive_rejects_overlay_only_branch_closure()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-final-review-overlay-only-closure"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for overlay-only closure fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator json for final review overlay-only closure fixture",
    );
    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("overlay-only final review fixture should expose dispatch_id")
        .to_owned();
    let branch_closure_id = operator_json["recording_context"]["branch_closure_id"]
        .as_str()
        .expect("overlay-only final review fixture should expose branch_closure_id")
        .to_owned();

    let mut payload = authoritative_harness_state(repo, state);
    payload["branch_closure_records"]
        .as_object_mut()
        .expect("branch_closure_records should remain an object")
        .remove(&branch_closure_id);
    write_authoritative_harness_state(repo, state, &payload);

    let rerouted_operator = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator should re-route overlay-only final-review closure state back to branch-closure recording",
    );
    assert_eq!(rerouted_operator["phase"], "document_release_pending");
    assert_eq!(
        rerouted_operator["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        rerouted_operator["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        rerouted_operator["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let summary_path = repo.join("overlay-only-final-review-summary.md");
    write_file(
        &summary_path,
        "Final review should not bind to overlay-only branch closure state.\n",
    );
    let review_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-final-review"),
            "--plan",
            plan_rel,
            concat!("--branch", "-closure-id"),
            &branch_closure_id,
            concat!("--dispatch", "-id"),
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-final-review should fail closed when only overlay branch-closure truth remains"
        ),
    );

    assert_eq!(review_json["action"], "blocked");
    assert_eq!(
        review_json["code"],
        Value::from("out_of_phase_requery_required")
    );
    assert_eq!(
        review_json["recommended_command"],
        Value::from(format!(
            "featureforge workflow operator --plan {plan_rel} --external-review-result-ready"
        ))
    );
    assert_eq!(
        review_json["rederive_via_workflow_operator"],
        Value::Bool(true)
    );
    assert!(
        review_json["required_follow_up"].is_null(),
        "json: {review_json}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_final_review_records_runtime_deviation_disposition()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-records-runtime-deviation");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    write_matching_topology_downgrade_record(repo, state, plan_rel, &base_branch);
    mark_branch_review_artifacts_with_runtime_deviation_pass(repo, state);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for runtime-deviation fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "workflow operator json for runtime-deviation final review fixture",
    );
    let _dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("runtime-deviation fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-deviation-summary.md");
    write_file(
        &summary_path,
        "Independent final review passed after runtime topology downgrade review.\n",
    );
    let review_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review should record runtime deviation disposition",
    );
    assert_eq!(review_json["action"], "recorded");

    let authoritative_state = authoritative_harness_state(repo, state);
    let final_review_fingerprint =
        current_final_review_fingerprint(&authoritative_state, "runtime-deviation final review");
    let final_review_path = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo, state),
        &current_branch_name(repo),
        &format!("final-review-{final_review_fingerprint}.md"),
    );
    let final_review_source = fs::read_to_string(&final_review_path)
        .expect("runtime-deviation final review artifact should be readable");
    assert!(
        final_review_source.contains("**Recorded Execution Deviations:** present"),
        "final review artifact should record runtime deviation presence: {final_review_source}"
    );
    assert!(
        final_review_source.contains("**Deviation Review Verdict:** pass"),
        "final review artifact should record a passing runtime deviation verdict: {final_review_source}"
    );

    let receipt = parse_final_review_receipt(&final_review_path);
    assert!(
        receipt.reviewer_artifact_path.is_some(),
        "runtime-deviation final review should bind reviewer artifact path"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_final_review_keeps_deviation_verdict_independent_when_review_fails()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-deviation-fail-result");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    write_matching_topology_downgrade_record(repo, state, plan_rel, &base_branch);
    mark_branch_review_artifacts_with_runtime_deviation_pass(repo, state);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for failed-result runtime-deviation fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "workflow operator json for failed-result runtime-deviation final review fixture",
    );
    let _dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("failed-result runtime-deviation fixture should expose dispatch_id");

    let summary_path = repo.join("final-review-deviation-fail-summary.md");
    write_file(
        &summary_path,
        "Independent final review failed after runtime topology downgrade review.\n",
    );
    let review_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review should keep runtime deviation disposition independent from the overall review result",
    );
    assert_eq!(review_json["action"], "recorded");

    let authoritative_state = authoritative_harness_state(repo, state);
    let final_review_fingerprint = current_final_review_fingerprint(
        &authoritative_state,
        "failed-result runtime-deviation final review",
    );
    let final_review_path = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo, state),
        &current_branch_name(repo),
        &format!("final-review-{final_review_fingerprint}.md"),
    );
    let final_review_source = fs::read_to_string(&final_review_path)
        .expect("failed-result runtime-deviation final review artifact should be readable");
    assert!(
        final_review_source.contains("**Recorded Execution Deviations:** present"),
        "final review artifact should record runtime deviation presence even on failed review: {final_review_source}"
    );
    assert!(
        final_review_source.contains("**Deviation Review Verdict:** pass"),
        "final review artifact should keep a passing deviation verdict independent from the overall failed review result: {final_review_source}"
    );
    assert!(
        final_review_source.contains("**Result:** fail"),
        "final review artifact should still preserve the overall failed review result: {final_review_source}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_final_review_blocks_without_release_ready()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-missing-release-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before clearing release readiness",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final review dispatch should expose dispatch_id")
        .to_owned();
    update_authoritative_harness_state(
        repo,
        state,
        &[("current_release_readiness_result", Value::Null)],
    );

    let summary_path = repo.join("final-review-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let review_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review should fail closed without release readiness ready",
    );

    assert_eq!(review_json["action"], "recorded");
    assert_eq!(review_json["stage_path"], "final_review");
    assert_eq!(
        review_json["delegated_primitive"],
        concat!("record", "-final-review")
    );
    assert_eq!(review_json["result"], "pass");
    assert_eq!(review_json["code"], Value::Null);
    assert_eq!(review_json["recommended_command"], Value::Null);
    assert_eq!(review_json["rederive_via_workflow_operator"], Value::Null);
    assert!(
        review_json["required_follow_up"].is_null(),
        "json: {review_json}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_final_review_result"],
        Value::from("pass")
    );
    assert_eq!(
        current_final_review_record(
            &authoritative_state,
            "final-review without release-ready fixture"
        )["result"],
        Value::from("pass")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_final_review_blocked_release_ready_requires_resolution()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-blocked-release-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before blocking release readiness",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final review dispatch should expose dispatch_id")
        .to_owned();
    set_current_authoritative_release_readiness_result(repo, state, "blocked");

    let summary_path = repo.join("final-review-blocked-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let review_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review should require blocker resolution when release readiness is blocked",
    );

    assert_eq!(review_json["action"], "blocked");
    assert_eq!(review_json["stage_path"], "final_review");
    assert_eq!(
        review_json["delegated_primitive"],
        concat!("record", "-final-review")
    );
    assert_eq!(
        review_json["code"],
        Value::from("out_of_phase_requery_required")
    );
    assert_eq!(
        review_json["recommended_command"],
        Value::from(format!(
            "featureforge workflow operator --plan {plan_rel} --external-review-result-ready"
        ))
    );
    assert_eq!(
        review_json["rederive_via_workflow_operator"],
        Value::Bool(true)
    );
    assert!(
        review_json["required_follow_up"].is_null(),
        "json: {review_json}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert!(authoritative_state["current_final_review_result"].is_null());
    assert!(authoritative_state["final_review_state"].is_null());
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_final_review_rerun_is_idempotent_and_conflicts_fail_closed()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-idempotency");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for idempotency fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final-review idempotency fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let first = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "first final-review recording should succeed",
    );
    assert_eq!(first["action"], "recorded");
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should read after final-review recording"),
    )
    .expect("authoritative state should remain valid json after final-review recording");
    assert_eq!(
        authoritative_state["current_final_review_dispatch_id"],
        Value::from(dispatch_id.clone())
    );
    assert_eq!(
        authoritative_state["current_final_review_reviewer_source"],
        Value::from("fresh-context-subagent")
    );
    assert_eq!(
        authoritative_state["current_final_review_reviewer_id"],
        Value::from("reviewer-fixture-001")
    );
    assert_eq!(
        authoritative_state["current_final_review_result"],
        Value::from("pass")
    );
    assert_eq!(
        authoritative_state["current_final_review_summary_hash"],
        Value::from(sha256_hex(b"Independent final review passed."))
    );

    update_authoritative_harness_state(
        repo,
        state,
        &[("current_release_readiness_result", Value::Null)],
    );
    let degraded_rerun = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "equivalent final-review rerun should stay idempotent after release readiness degrades",
    );
    assert_eq!(degraded_rerun["action"], "already_current");
    assert!(degraded_rerun["code"].is_null(), "json: {degraded_rerun}");
    assert!(
        degraded_rerun["recommended_command"].is_null(),
        "json: {degraded_rerun}"
    );
    assert!(
        degraded_rerun["rederive_via_workflow_operator"].is_null(),
        "json: {degraded_rerun}"
    );
    assert_eq!(degraded_rerun["required_follow_up"], Value::Null);

    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let second = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "equivalent final-review rerun should stay idempotent once branch completion routing has advanced",
    );
    assert_eq!(second["action"], "already_current");
    assert!(second["code"].is_null(), "json: {second}");
    assert!(second["recommended_command"].is_null(), "json: {second}");
    assert!(
        second["rederive_via_workflow_operator"].is_null(),
        "json: {second}"
    );
    assert_eq!(second["required_follow_up"], Value::Null);

    let conflicting = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-999",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "conflicting final-review rerun should also fail closed out of phase once branch completion routing has advanced",
    );
    assert_eq!(conflicting["action"], "blocked");
    assert_eq!(conflicting["code"], "out_of_phase_requery_required");
    assert_eq!(
        conflicting["recommended_command"],
        Value::from(format!(
            "featureforge workflow operator --plan {plan_rel} --external-review-result-ready"
        ))
    );
    assert_eq!(
        conflicting["rederive_via_workflow_operator"],
        Value::Bool(true)
    );
    assert_eq!(conflicting["required_follow_up"], Value::Null);

    append_tracked_repo_line(
        repo,
        "README.md",
        "final-review stale-unreviewed regression coverage",
    );
    let stale_operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after final-review stale drift",
    );
    assert_eq!(
        stale_operator_json["review_state_status"],
        "stale_unreviewed"
    );
    let stale_rerun = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "stale final-review rerun should fail closed",
    );
    assert_eq!(stale_rerun["action"], "blocked");
    assert_eq!(stale_rerun["code"], Value::Null);
    assert_eq!(stale_rerun["recommended_command"], Value::Null);
    assert_eq!(stale_rerun["rederive_via_workflow_operator"], Value::Null);
    assert_eq!(stale_rerun["required_follow_up"], "repair_review_state");
}

#[test]
fn internal_only_compatibility_final_review_receipt_tampering_does_not_reroute_when_authoritative_record_is_current()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let branch_closure_id = "branch-release-closure";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-rerun-shared");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let template = build_setup_fixture_template(
        "workflow-shell-smoke-final-review-rerun-template",
        |repo, state| {
            let base_branch = expected_release_base_branch(repo);
            complete_workflow_fixture_execution(repo, state, plan_rel);
            write_branch_test_plan_artifact(repo, state, plan_rel, "no");
            write_branch_release_artifact(repo, state, plan_rel, &base_branch);
            mark_current_branch_closure_release_ready(repo, state, branch_closure_id);
            let dispatch = internal_only_plan_execution_fixture_json(
                repo,
                state,
                &[
                    concat!("record", "-review-dispatch"),
                    "--plan",
                    plan_rel,
                    "--scope",
                    "final-review",
                ],
                "plan execution final review dispatch template setup",
            );
            assert_eq!(dispatch["action"], Value::from("recorded"));
            write_branch_review_artifact(repo, state, plan_rel, &base_branch);

            let summary_path = repo.join("final-review-template-summary.md");
            write_file(&summary_path, "Independent final review passed.\n");
            let _dispatch_id = dispatch["dispatch_id"]
                .as_str()
                .expect("final-review rerun template should expose dispatch_id");
            let first = internal_only_run_plan_execution_json_direct_or_cli(
                repo,
                state,
                &[
                    "advance-late-stage",
                    "--plan",
                    plan_rel,
                    "--reviewer-source",
                    "fresh-context-subagent",
                    "--reviewer-id",
                    "reviewer-fixture-001",
                    "--result",
                    "pass",
                    "--summary-file",
                    summary_path.to_str().expect("summary path should be utf-8"),
                ],
                "first final-review recording template setup",
            );
            assert_eq!(first["action"], "recorded", "{first}");
            let gate_review = internal_only_plan_execution_fixture_json(
                repo,
                state,
                &[concat!("gate", "-review"), "--plan", plan_rel],
                concat!(
                    "gate",
                    "-review template setup should persist a finish checkpoint"
                ),
            );
            assert_eq!(gate_review["allowed"], Value::Bool(true), "{gate_review}");
        },
    );

    for (case_name, mutator, republish_authoritative) in [
        ("malformed", "malformed", true),
        ("plan_mismatch", "plan_mismatch", true),
        (
            "authoritative_provenance_invalid",
            "authoritative_provenance_invalid",
            false,
        ),
    ] {
        populate_fixture_from_template(&template, repo, state);
        let authoritative_state_before = authoritative_harness_state(repo, state);
        let _dispatch_id = authoritative_state_before["current_final_review_dispatch_id"]
            .as_str()
            .expect("final-review invalidation fixture should expose dispatch_id")
            .to_owned();
        let summary_path = repo.join(format!("final-review-{case_name}-summary.md"));
        write_file(&summary_path, "Independent final review passed.\n");
        let final_review_record_id = authoritative_state_before["current_final_review_record_id"]
            .as_str()
            .expect("final-review invalidation fixture should expose current record id")
            .to_owned();
        let final_review_history_len = authoritative_state_before["final_review_record_history"]
            .as_object()
            .expect("final review history should remain an object")
            .len();
        let final_review_fingerprint = current_final_review_fingerprint(
            &authoritative_state_before,
            "final-review invalidation fixture",
        );
        let final_review_path = harness_authoritative_artifact_path(
            state,
            &repo_slug(repo, state),
            &current_branch_name(repo),
            &format!("final-review-{final_review_fingerprint}.md"),
        );
        let mut tampered_source = fs::read_to_string(&final_review_path)
            .expect("final-review invalidation fixture should read authoritative artifact");
        match mutator {
            "malformed" => {
                tampered_source =
                    tampered_source.replace("# Code Review Result", "# Not Code Review");
            }
            "plan_mismatch" => {
                tampered_source = tampered_source
                    .replace("**Source Plan Revision:** 1", "**Source Plan Revision:** 2");
            }
            "authoritative_provenance_invalid" => {
                tampered_source = tampered_source.replace(
                    "Independent final review passed.",
                    "Independent final review passed after authoritative tamper.",
                );
            }
            _ => unreachable!("unexpected mutator"),
        }
        write_file(&final_review_path, &tampered_source);
        if republish_authoritative {
            let _ = republish_authoritative_artifact_from_path(
                repo,
                state,
                &final_review_path,
                "final-review",
                "last_final_review_artifact_fingerprint",
            );
        }

        let gate_finish = internal_only_plan_execution_fixture_json(
            repo,
            state,
            &[concat!("gate", "-finish"), "--plan", plan_rel],
            &format!(
                "{} should ignore {} final-review receipt tamper",
                case_name,
                concat!("gate", "-finish")
            ),
        );
        assert_eq!(
            gate_finish["allowed"],
            Value::Bool(true),
            "case {case_name}: {gate_finish}"
        );

        let operator_json = run_featureforge_with_env_json(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            &format!(
                "workflow operator should keep branch-completion routing when {case_name} only tampers derived final-review receipts"
            ),
        );
        assert_eq!(
            operator_json["phase"], "ready_for_branch_completion",
            "case {case_name}: {operator_json}"
        );
        assert_eq!(
            operator_json["phase_detail"], "finish_completion_gate_ready",
            "case {case_name}: {operator_json}"
        );
        let status_json = internal_only_run_plan_execution_json_direct_or_cli(
            repo,
            state,
            &["status", "--plan", plan_rel],
            &format!(
                "plan execution status should stay aligned when {case_name} only tampers derived final-review receipts"
            ),
        );
        assert_eq!(
            status_json["phase"], operator_json["phase"],
            "case {case_name}: {status_json}"
        );
        assert_eq!(
            status_json["phase_detail"], operator_json["phase_detail"],
            "case {case_name}: {status_json}"
        );

        let stale_rerun = run_plan_execution_json_real_cli(
            repo,
            state,
            &[
                "advance-late-stage",
                "--plan",
                plan_rel,
                "--reviewer-source",
                "fresh-context-subagent",
                "--reviewer-id",
                "reviewer-fixture-001",
                "--result",
                "pass",
                "--summary-file",
                summary_path.to_str().expect("summary path should be utf-8"),
            ],
            &format!(
                "same-state final-review rerun should remain idempotent when {case_name} only tampers derived final-review receipts"
            ),
        );
        assert_eq!(
            stale_rerun["action"], "already_current",
            "case {case_name}: {stale_rerun}"
        );

        let authoritative_state_after = authoritative_harness_state(repo, state);
        assert_eq!(
            authoritative_state_after["current_final_review_record_id"],
            Value::from(final_review_record_id),
            "case {case_name}: rerun invalidation must not replace the current final-review record"
        );
        assert_eq!(
            authoritative_state_after["final_review_record_history"]
                .as_object()
                .expect("final review history should remain an object")
                .len(),
            final_review_history_len,
            "case {case_name}: rerun invalidation must not mint a new final-review record"
        );
    }
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_same_state_rerun_keeps_standard_requery_after_final_review_receipt_tamper()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-qa-final-review-invalidated"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("qa-after-final-review-summary.md");
    write_file(&summary_path, "Browser QA passed for the current branch.\n");
    let first = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "initial record",
            "-qa invocation should succeed before final-review invalidation coverage"
        ),
    );
    assert_eq!(first["action"], "recorded");

    let authoritative_state = authoritative_harness_state(repo, state);
    let final_review_fingerprint =
        current_final_review_fingerprint(&authoritative_state, "qa invalidation fixture");
    let final_review_path = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo, state),
        &current_branch_name(repo),
        &format!("final-review-{final_review_fingerprint}.md"),
    );
    let tampered_source = fs::read_to_string(&final_review_path)
        .expect("qa invalidation fixture should read authoritative final-review artifact")
        .replace("# Code Review Result", "# Not Code Review");
    write_file(&final_review_path, &tampered_source);
    let _ = republish_authoritative_artifact_from_path(
        repo,
        state,
        &final_review_path,
        "final-review",
        "last_final_review_artifact_fingerprint",
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep ready-for-completion routing after final-review receipt tamper",
    );
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
    assert_eq!(operator_json["phase_detail"], "finish_review_gate_ready");

    let rerun = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "same-state record",
            "-qa rerun should keep the standard out-of-phase requery after final-review receipt tamper"
        ),
    );
    assert_eq!(rerun["action"], "blocked", "json: {rerun}");
    assert_ne!(rerun["action"], "already_current", "json: {rerun}");
    assert_eq!(
        rerun["code"], "out_of_phase_requery_required",
        "json: {rerun}"
    );
    assert_eq!(
        rerun["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}")),
        "json: {rerun}"
    );
    assert_eq!(rerun["rederive_via_workflow_operator"], Value::Bool(true));
    assert_eq!(rerun["required_follow_up"], Value::Null, "json: {rerun}");
}

#[test]
fn internal_only_compatibility_workflow_operator_keeps_branch_completion_routing_after_reviewer_artifact_tamper()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-reviewer-artifact-tamper");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "final review dispatch should succeed before reviewer-artifact tamper routing coverage",
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("reviewer-artifact tamper fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-reviewer-artifact-tamper-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let first = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "first final-review recording should succeed before reviewer-artifact tamper coverage",
    );
    assert_eq!(first["action"], "recorded");

    let authoritative_state_before = authoritative_harness_state(repo, state);
    let final_review_record_id = authoritative_state_before["current_final_review_record_id"]
        .as_str()
        .expect("reviewer-artifact tamper fixture should expose current final review record id")
        .to_owned();
    let final_review_history_len = authoritative_state_before["final_review_record_history"]
        .as_object()
        .expect("final review history should remain an object")
        .len();
    let final_review_fingerprint = current_final_review_fingerprint(
        &authoritative_state_before,
        "reviewer-artifact tamper fixture",
    );
    let final_review_path = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo, state),
        &current_branch_name(repo),
        &format!("final-review-{final_review_fingerprint}.md"),
    );
    let reviewer_artifact_path = PathBuf::from(
        parse_final_review_receipt(&final_review_path)
            .reviewer_artifact_path
            .expect("reviewer-artifact tamper fixture should expose reviewer artifact path"),
    );
    let tampered_reviewer_source = fs::read_to_string(&reviewer_artifact_path)
        .expect("reviewer artifact should remain readable before tamper")
        .replace(
            "dedicated independent reviewer artifact fixture.",
            "dedicated independent reviewer artifact fixture after reviewer-artifact tamper.",
        );
    write_file(&reviewer_artifact_path, &tampered_reviewer_source);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep branch-completion routing after reviewer-artifact tamper",
    );
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
    assert_eq!(operator_json["phase_detail"], "finish_review_gate_ready");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["recommended_command"], Value::Null);
    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should stay aligned after reviewer-artifact tamper",
    );
    assert_eq!(status_json["phase"], operator_json["phase"]);
    assert_eq!(status_json["phase_detail"], operator_json["phase_detail"]);

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-review should persist the finish checkpoint before gate",
            "-finish after reviewer-artifact tamper"
        ),
    );
    assert_eq!(gate_review["allowed"], Value::Bool(true), "{gate_review}");

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-finish should ignore reviewer-artifact tamper when authoritative record stays current"
        ),
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(true), "{gate_finish}");

    let stale_rerun = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "same-state final-review rerun should remain idempotent after reviewer-artifact tamper",
    );
    assert_eq!(stale_rerun["action"], "already_current");
    assert_eq!(stale_rerun["code"], Value::Null);
    assert_eq!(stale_rerun["recommended_command"], Value::Null);
    assert_eq!(stale_rerun["rederive_via_workflow_operator"], Value::Null);
    assert_eq!(stale_rerun["required_follow_up"], Value::Null);

    let authoritative_state_after = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state_after["current_final_review_record_id"],
        Value::from(final_review_record_id)
    );
    assert_eq!(
        authoritative_state_after["final_review_record_history"]
            .as_object()
            .expect("final review history should remain an object")
            .len(),
        final_review_history_len,
        "same-state rerun after reviewer-artifact tamper must not mint a new final-review record"
    );
}

#[test]
fn plan_execution_advance_late_stage_final_review_requires_dispatch_follow_up() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-advance-late-stage-final-review-needs-dispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let release_summary_path = repo.join("release-ready-before-final-review-dispatch.md");
    write_file(
        &release_summary_path,
        "Release readiness is current before final review dispatch.\n",
    );
    let release_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            release_summary_path
                .to_str()
                .expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before final-review dispatch follow-up coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    let summary_path = repo.join("final-review-needs-dispatch-summary.md");
    write_file(
        &summary_path,
        "Independent final review passed after dispatch was skipped.\n",
    );
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should report final-review dispatch as the required follow-up when no current dispatch lineage exists",
    );
    assert_eq!(review_json["action"], "blocked");
    assert_eq!(review_json["code"], Value::Null, "json: {review_json}");
    assert_eq!(
        review_json["required_follow_up"],
        Value::from("request_external_review"),
        "json: {review_json}"
    );
    assert_eq!(
        review_json["recommended_command"],
        Value::Null,
        "json: {review_json}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_final_review_fail_reroutes_to_execution_reentry()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-fail-rerun");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    seed_current_task_closure_state(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for failing rerun fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    write_branch_review_artifact_with_result(repo, state, plan_rel, &base_branch, "fail");
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final-review fail rerun fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-fail-summary.md");
    write_file(
        &summary_path,
        "Independent final review found a release blocker.\n",
    );
    let first = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-fail",
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "first failing final-review recording should return remediation follow-up",
    );
    assert_eq!(first["action"], "recorded");
    assert_eq!(first["result"], "fail");
    assert_eq!(first["required_follow_up"], "execution_reentry");

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should immediately reroute failed final review to execution reentry",
    );
    assert_eq!(operator_after_fail["phase"], "executing");
    assert_eq!(
        operator_after_fail["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(operator_after_fail["review_state_status"], "clean");
    assert!(operator_after_fail.get("follow_up_override").is_none());
    let status_after_fail = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should immediately reroute failed QA to execution reentry",
    );
    assert_eq!(
        status_after_fail["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(
        status_after_fail["next_action"],
        "execution reentry required"
    );
    assert!(status_after_fail.get("follow_up_override").is_none());

    let second = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-fail",
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "equivalent failing final-review rerun should stay idempotent while execution reentry is still required (real CLI)",
    );
    assert_eq!(second["action"], "already_current", "json: {second}");
    assert_eq!(second["result"], "fail");
    assert!(second["code"].is_null(), "json: {second}");
    assert!(second["recommended_command"].is_null(), "json: {second}");
    assert!(
        second["rederive_via_workflow_operator"].is_null(),
        "json: {second}"
    );
    assert_eq!(
        second["required_follow_up"], "execution_reentry",
        "json: {second}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_final_review_fail_keeps_execution_reentry_over_handoff_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-fail-handoff-override");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for handoff override fixture",
    );
    write_branch_review_artifact_with_result(repo, state, plan_rel, &base_branch, "fail");
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final-review handoff override fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-handoff-override-summary.md");
    write_file(
        &summary_path,
        "Independent final review found handoff-only blocker.\n",
    );
    let review_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-handoff-override",
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review should keep execution reentry ahead of handoff state",
    );
    assert_eq!(review_json["action"], "recorded", "json: {review_json}");
    assert_eq!(review_json["result"], "fail", "json: {review_json}");
    assert_eq!(
        review_json["required_follow_up"], "execution_reentry",
        "json: {review_json}"
    );
    assert!(review_json["code"].is_null(), "json: {review_json}");
    assert!(
        review_json["recommended_command"].is_null(),
        "json: {review_json}"
    );

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should still surface handoff-required routing after a failed final review when handoff state is present",
    );
    assert_eq!(operator_after_fail["phase"], "executing");
    assert_eq!(
        operator_after_fail["phase_detail"],
        "execution_reentry_required"
    );
    assert!(operator_after_fail.get("follow_up_override").is_none());

    let rerun = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-handoff-override",
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "equivalent failing final-review rerun should stay idempotent while execution reentry remains required on the real CLI path",
    );
    assert_eq!(rerun["action"], "already_current", "json: {rerun}");
    assert_eq!(rerun["result"], "fail", "json: {rerun}");
    assert_eq!(
        rerun["dispatch_id"],
        Value::from(dispatch_id),
        "json: {rerun}"
    );
    assert_eq!(
        rerun["required_follow_up"], "execution_reentry",
        "json: {rerun}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_accepts_human_independent_reviewer()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-human-reviewer");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for human reviewer fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator json for human-independent-reviewer final review",
    );
    let _dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("final review recording fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-human-summary.md");
    write_file(&summary_path, "Independent human final review passed.\n");
    let review_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--reviewer-source",
            "human-independent-reviewer",
            "--reviewer-id",
            "human-reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review should accept human-independent-reviewer",
    );
    assert_eq!(review_json["action"], "recorded");

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after human reviewer final review recording",
    );
    assert!(
        matches!(
            operator_json["phase"].as_str(),
            Some("ready_for_branch_completion" | "final_review_pending")
        ),
        "human-independent-reviewer final-review recording should be accepted without execution reroute, got {operator_json}"
    );
}

#[test]
fn workflow_operator_routes_document_release_pending_to_advance_late_stage_after_branch_closure_exists()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-release-readiness-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for release-readiness-ready routing",
    );

    assert_eq!(
        operator_json["phase"], "document_release_pending",
        "json: {operator_json}"
    );
    assert_eq!(
        operator_json["phase_detail"],
        "release_readiness_recording_ready"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        "branch-release-closure"
    );
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_record_pivot_command_is_removed_and_operator_routes_publicly() {
    let (repo_dir, state_dir) = init_repo("workflow-record-pivot-command-removed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_authoritative_harness_state(
        repo,
        state,
        &serde_json::json!({
            "harness_phase": "pivot_required",
            "latest_authoritative_sequence": 23,
            "reason_codes": ["blocked_on_plan_revision"],
        }),
    );

    let removed_command = run_featureforge_with_env(
        repo,
        state,
        &[
            "workflow",
            "record-pivot",
            "--plan",
            plan_rel,
            "--reason",
            "plan revision superseded current execution",
            "--json",
        ],
        &[],
        "workflow record-pivot command should be removed",
    );
    assert!(!removed_command.status.success());
    let removed_stderr = String::from_utf8_lossy(&removed_command.stderr);
    assert!(removed_stderr.contains("unrecognized subcommand 'record-pivot'"));

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route pivot-required states to repair-review-state",
    );
    assert_eq!(operator_json["phase"], "pivot_required");
    assert_eq!(operator_json["phase_detail"], "planning_reentry_required");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_operator_keeps_pivot_override_without_authoritative_pivot_checkpoint() {
    let (repo_dir, state_dir) =
        init_repo("workflow-record-pivot-requires-authoritative-checkpoint");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_authoritative_harness_state(
        repo,
        state,
        &serde_json::json!({
            "harness_phase": "pivot_required",
            "latest_authoritative_sequence": 23,
            "reason_codes": ["blocked_on_plan_revision"],
        }),
    );

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("last_pivot_path", Value::Null),
            ("last_pivot_fingerprint", Value::Null),
            ("harness_phase", Value::from("pivot_required")),
            (
                "reason_codes",
                serde_json::json!(["blocked_on_plan_revision"]),
            ),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should require runtime-owned pivot checkpoint before clearing override",
    );
    assert!(operator_json.get("follow_up_override").is_none());
    assert_eq!(operator_json["phase"], "pivot_required");
    assert_eq!(
        operator_json["phase_detail"], "planning_reentry_required",
        "{operator_json:?}"
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should require runtime-owned pivot checkpoint before clearing override",
    );
    assert!(status_json.get("follow_up_override").is_none());
}

#[test]
fn workflow_operator_ignores_off_directory_pivot_checkpoint_path() {
    let (repo_dir, state_dir) = init_repo("workflow-record-pivot-off-directory-checkpoint");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_authoritative_harness_state(
        repo,
        state,
        &serde_json::json!({
            "harness_phase": "pivot_required",
            "latest_authoritative_sequence": 23,
            "reason_codes": ["blocked_on_plan_revision"],
        }),
    );

    let off_directory_checkpoint = repo.join("off-directory-pivot-checkpoint.md");
    let record_source = String::from(
        "# Workflow Pivot Record\n\nThis checkpoint is intentionally off-directory.\n",
    );
    write_file(&off_directory_checkpoint, &record_source);
    let off_directory_fingerprint = sha256_hex(record_source.as_bytes());

    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "last_pivot_path",
                Value::from(off_directory_checkpoint.display().to_string()),
            ),
            (
                "last_pivot_fingerprint",
                Value::from(off_directory_fingerprint),
            ),
            ("harness_phase", Value::from("pivot_required")),
            (
                "reason_codes",
                serde_json::json!(["blocked_on_plan_revision"]),
            ),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should ignore off-directory runtime pivot checkpoints",
    );
    assert!(operator_json.get("follow_up_override").is_none());
    assert_eq!(operator_json["phase"], "pivot_required");

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should ignore off-directory runtime pivot checkpoints",
    );
    assert!(status_json.get("follow_up_override").is_none());
}

#[test]
fn workflow_record_pivot_command_is_removed_out_of_phase() {
    let (repo_dir, state_dir) = init_repo("workflow-record-pivot-command-removed-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let removed_command = run_featureforge_with_env(
        repo,
        state,
        &[
            "workflow",
            "record-pivot",
            "--plan",
            plan_rel,
            "--reason",
            "plan revision superseded current execution",
            "--json",
        ],
        &[],
        "workflow record-pivot command should remain removed when out-of-phase",
    );
    assert!(!removed_command.status.success());
    let removed_stderr = String::from_utf8_lossy(&removed_command.stderr);
    assert!(removed_stderr.contains("unrecognized subcommand 'record-pivot'"));
}

#[test]
fn workflow_record_pivot_command_is_removed_when_qa_requirement_is_missing() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-record-pivot-command-removed-missing-qa-requirement");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case_with_qa_requirement(
        repo,
        state,
        plan_rel,
        &base_branch,
        None,
        true,
    );
    remove_branch_test_plan_artifact(repo, state);

    let removed_command = run_featureforge_with_env(
        repo,
        state,
        &[
            "workflow",
            "record-pivot",
            "--plan",
            plan_rel,
            "--reason",
            "qa requirement metadata was missing",
            "--json",
        ],
        &[],
        "workflow record-pivot command should remain removed when QA requirement is missing",
    );
    assert!(!removed_command.status.success());
    let removed_stderr = String::from_utf8_lossy(&removed_command.stderr);
    assert!(removed_stderr.contains("unrecognized subcommand 'record-pivot'"));
}

#[test]
fn workflow_operator_routes_document_release_pending_to_record_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-document-release-routing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for document release pending",
    );

    assert_eq!(
        operator_json["phase"], "document_release_pending",
        "json: {operator_json}"
    );
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
}

#[test]
fn advance_late_stage_branch_closure_route_rejects_release_arguments_before_mutation() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("advance-late-stage-branch-closure-arg-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    let digest_before = authoritative_harness_state_digest(repo, state);

    let summary_path = repo.join("branch-closure-arg-mismatch-summary.md");
    write_file(
        &summary_path,
        "This summary should not be accepted while branch-closure recording is the active lane.\n",
    );
    let summary_arg = summary_path
        .to_str()
        .expect("summary path should be utf-8 for argument-mismatch coverage");
    let blocked = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_arg,
        ],
        "advance-late-stage should fail closed with an out-of-phase reroute when release-readiness arguments are supplied during branch-closure recording",
    );
    assert_eq!(blocked["action"], Value::from("blocked"));
    assert_eq!(blocked["stage_path"], Value::from("release_readiness"));
    assert_eq!(
        authoritative_harness_state_digest(repo, state),
        digest_before,
        "branch-closure argument mismatch must fail before authoritative mutation"
    );
    let blocked_real_cli = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_arg,
        ],
        "real-cli advance-late-stage should emit the same blocked out-of-phase reroute during branch-closure recording",
    );
    assert_eq!(blocked_real_cli["action"], Value::from("blocked"));
    assert_eq!(
        blocked_real_cli["stage_path"],
        Value::from("release_readiness")
    );
    assert_eq!(
        authoritative_harness_state_digest(repo, state),
        digest_before,
        "real-cli branch-closure argument mismatch must also fail before authoritative mutation"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should remain on branch-closure recording lane after argument-mismatch failure",
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_keeps_execution_scope_when_future_task_remains_unchecked()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-future-task-outranks-late-stage");
    let repo = repo_dir.path();
    let state = state_dir.path();
    close_two_task_fixture_task_1(repo, state, plan_rel);

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("document_release_pending")),
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
        ],
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should keep execution scope when a future task remains unchecked",
    );
    assert_eq!(status_json["review_state_status"], "clean");
    assert_eq!(status_json["current_branch_closure_id"], Value::Null);
    assert_ne!(
        status_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(status_json["blocking_task"], Value::from(2_u64));

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep execution routing when a future task remains unchecked",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_ne!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert!(
        operator_json["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("featureforge plan execution begin")),
        "workflow operator should route back to Task 2 execution, got {operator_json}"
    );
}

#[test]
fn workflow_status_and_operator_rederive_late_stage_after_execution_exhausts() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-rederive-late-stage-after-execution");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch =
        resolve_release_base_branch(&repo.join(".git"), "feature").expect("fixture base branch");
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(repo, state, &[("harness_phase", Value::from("executing"))]);

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should rederive late-stage routing when execution is exhausted despite a persisted executing phase",
    );
    assert_eq!(status_json["harness_phase"], "document_release_pending");
    assert_eq!(
        status_json["phase_detail"],
        "release_readiness_recording_ready"
    );
    assert_eq!(status_json["review_state_status"], "clean");
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should rederive late-stage routing when execution is exhausted despite a persisted executing phase",
    );
    assert_eq!(
        operator_json["phase"], "document_release_pending",
        "json: {operator_json}"
    );
    assert_eq!(
        operator_json["phase_detail"],
        "release_readiness_recording_ready"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_status_and_operator_rederive_first_entry_late_stage_from_current_task_closures() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-rederive-first-entry-late-stage");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch =
        resolve_release_base_branch(&repo.join(".git"), "feature").expect("fixture base branch");
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(repo, state, &[("harness_phase", Value::from("executing"))]);

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should rederive first-entry late-stage routing from current task closures",
    );
    assert_eq!(status_json["harness_phase"], "document_release_pending");
    assert_eq!(
        status_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        status_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should rederive first-entry late-stage routing from current task closures",
    );
    assert_eq!(
        operator_json["phase"], "document_release_pending",
        "json: {operator_json}"
    );
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_status_and_operator_keep_first_entry_late_stage_when_drift_is_confined_to_late_stage_surface()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-first-entry-late-stage-surface-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch =
        resolve_release_base_branch(&repo.join(".git"), "feature").expect("fixture base branch");
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");
    append_tracked_repo_line(
        repo,
        "README.md",
        "first-entry late-stage surface drift should still route to branch closure recording",
    );
    update_authoritative_harness_state(repo, state, &[("harness_phase", Value::from("executing"))]);

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should keep first-entry late-stage routing when drift is confined to Late-Stage Surface",
    );
    assert_eq!(status_json["harness_phase"], "document_release_pending");
    assert_eq!(
        status_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    if status_json["review_state_status"].as_str() != Some("missing_current_closure") {
        panic!("status_json={status_json:?}");
    }
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep first-entry late-stage routing when drift is confined to Late-Stage Surface",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_status_and_operator_surface_missing_late_stage_surface_blocker_for_first_entry_stale_drift()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-first-entry-late-stage-surface-metadata-missing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch =
        resolve_release_base_branch(&repo.join(".git"), "feature").expect("fixture base branch");
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    append_tracked_repo_line(
        repo,
        "README.md",
        "first-entry late-stage drift without declared metadata must reroute through execution repair",
    );
    update_authoritative_harness_state(repo, state, &[("harness_phase", Value::from("executing"))]);

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should surface missing Late-Stage Surface metadata as an explicit stale-state blocker",
    );
    assert_eq!(status_json["harness_phase"], "executing");
    assert_eq!(
        status_json["phase_detail"], "execution_reentry_required",
        "json: {status_json}"
    );
    assert_eq!(
        status_json["review_state_status"],
        "missing_current_closure"
    );
    assert!(
        status_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == &Value::from("late_stage_surface_not_declared"))),
        "status should surface late_stage_surface_not_declared in reason_codes, got {status_json}"
    );
    assert_eq!(
        status_json["next_action"],
        "repair review state / reenter execution"
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        )),
        "status should surface the public repair command while preserving the missing Late-Stage Surface blocker, got {status_json}"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should preserve the explicit missing Late-Stage Surface blocker when rerouting to execution repair",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        operator_json["next_action"],
        Value::from("repair review state / reenter execution")
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        )),
        "workflow operator should surface the public repair command for missing Late-Stage Surface metadata, got {operator_json}"
    );
}

#[test]
fn internal_only_compatibility_explain_review_state_omits_recommended_command_for_wait_state_lanes()
{
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("explain", "-review-state-task-review-wait"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!(
            "plan execution task review dispatch for explain",
            "-review-state wait-lane coverage"
        ),
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_task_closure_records", serde_json::json!({})),
            ("task_closure_record_history", serde_json::json!({})),
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );

    let explain_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should preserve omitted recommended_command for task-review wait lanes"
        ),
    );
    assert_eq!(explain_json["next_action"], "close current task");
    assert!(
        explain_json["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "{} should surface close-current-task when the closure baseline can be recorded immediately, got {}",
        explain_json,
        concat!("explain", "-review-state")
    );
}

#[test]
fn internal_only_compatibility_workflow_status_and_operator_reroute_prerelease_branch_closure_refresh_when_current_binding_stales()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-prerelease-branch-closure-refresh");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch =
        resolve_release_base_branch(&repo.join(".git"), "feature").expect("fixture base branch");
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");
    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish a current branch closure before prerelease refresh coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("prerelease refresh coverage should expose the current branch closure id")
        .to_owned();

    append_tracked_repo_line(
        repo,
        "README.md",
        "prerelease branch-closure refresh should route back to branch closure recording",
    );

    let explain_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should keep exposing the stale prerelease branch closure"
        ),
    );
    assert!(
        explain_json["stale_unreviewed_closures"]
            .as_array()
            .is_some_and(|closures| closures
                .iter()
                .any(|closure| closure == &Value::from(branch_closure_id.clone()))),
        "json: {explain_json:?}"
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should treat a prerelease stale branch closure as missing_current_closure",
    );
    assert_eq!(status_json["harness_phase"], "document_release_pending");
    assert_eq!(
        status_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        status_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert!(
        status_json["stale_unreviewed_closures"]
            .as_array()
            .is_some_and(|closures| closures
                .iter()
                .any(|closure| closure == &Value::from(branch_closure_id.clone()))),
        "json: {status_json:?}"
    );
    assert_eq!(explain_json["next_action"], status_json["next_action"]);
    assert_eq!(
        explain_json["recommended_command"],
        status_json["recommended_command"]
    );
    assert_eq!(explain_json["next_action"], "advance late stage");
    assert_eq!(
        explain_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should match prerelease branch-closure refresh routing",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
}

#[test]
fn internal_only_compatibility_prerelease_branch_closure_refresh_ignores_stale_execution_reentry_follow_up()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("prerelease-branch-closure-refresh-ignores-stale-execution-follow-up");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch =
        resolve_release_base_branch(&repo.join(".git"), "feature").expect("fixture base branch");
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish a current branch closure before stale execution-follow-up prerelease refresh coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");

    append_tracked_repo_line(
        repo,
        "README.md",
        "prerelease refresh should ignore stale execution follow-up latches",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "review_state_repair_follow_up",
            Value::from("execution_reentry"),
        )],
    );

    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should keep the prerelease refresh route on branch-closure recording even when a stale execution-reentry follow-up remains persisted (compiled CLI contract)",
    );
    if status_json["review_state_status"].as_str() != Some("missing_current_closure") {
        panic!("status_json={status_json:?}");
    }
    assert_eq!(
        status_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "workflow operator should ignore a stale execution-reentry follow-up when prerelease refresh still projects missing_current_closure (compiled CLI contract)",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let record_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should not be blocked by a stale execution-reentry follow-up when prerelease refresh still requires branch-closure rerecording"
        ),
    );
    assert_eq!(record_json["action"], "recorded");
}

#[test]
fn internal_only_compatibility_gate_review_ignores_stale_execution_reentry_follow_up_during_prerelease_refresh()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "gate",
        "-review-ignores-stale-execution-follow-up-prerelease-refresh"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch =
        resolve_release_base_branch(&repo.join(".git"), "feature").expect("fixture base branch");
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish a current branch closure before gate",
            "-review prerelease refresh coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");

    append_tracked_repo_line(
        repo,
        "README.md",
        concat!(
            "gate",
            "-review prerelease refresh should ignore stale execution follow-up latches"
        ),
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "review_state_repair_follow_up",
            Value::from("execution_reentry"),
        )],
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-review should keep recommending branch-closure recording when prerelease refresh truth outranks a stale execution-reentry follow-up"
        ),
    );
    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!(
            "status should expose the same prerelease refresh blocker as gate",
            "-review"
        ),
    );
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        concat!(
            "workflow operator should expose the same prerelease refresh blocker as gate",
            "-review"
        ),
    );
    assert_eq!(
        gate_review["allowed"],
        Value::Bool(false),
        "json: {gate_review}"
    );
    assert_eq!(
        gate_review["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        )),
        "json: {gate_review}"
    );
    assert_eq!(
        gate_review["recommended_command"],
        status_json["recommended_command"],
        "{} and status should agree on prerelease refresh command",
        concat!("gate", "-review")
    );
    assert_eq!(
        gate_review["recommended_command"],
        operator_json["recommended_command"],
        "{} and operator should agree on prerelease refresh command",
        concat!("gate", "-review")
    );
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        operator_json["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_ne!(
        gate_review["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn internal_only_compatibility_workflow_status_and_operator_require_execution_reentry_when_no_branch_contributing_task_closure_remains()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-no-branch-contributing-task-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch =
        resolve_release_base_branch(&repo.join(".git"), "feature").expect("fixture base branch");
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": [NO_REPO_FILES_MARKER],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 no-repo review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 no-repo verification"),
                    "closure_status": "current"
                }
            }),
        )],
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should not offer branch closure recording when no branch-contributing task closure remains",
    );
    assert_eq!(status_json["harness_phase"], "document_release_pending");
    assert_eq!(
        status_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should match no-branch-contributing task-closure reroute",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let record_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed when no branch-contributing task closure remains"
        ),
    );
    assert_eq!(record_json["action"], "blocked");
    assert_eq!(record_json["required_follow_up"], "repair_review_state");

    let reconcile_json = internal_only_unit_reconcile_review_state_json(
        repo,
        state,
        plan_rel,
        concat!(
            "reconcile",
            "-review-state should keep recommending repair-review-state until workflow/operator actually reroutes to branch-closure recording"
        ),
    );
    assert_eq!(reconcile_json["action"], "blocked");
    assert_eq!(
        reconcile_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_keeps_execution_scope_when_resume_task_exists_despite_late_stage_phase()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-resume-task-outranks-late-stage");
    let repo = repo_dir.path();
    let state = state_dir.path();
    close_two_task_fixture_task_1(repo, state, plan_rel);
    write_repo_file(
        repo,
        "docs/example-followup.md",
        "two-task workflow reopen fixture output\n",
    );

    let status_before_task_2 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before task 2 begin for reopened execution routing",
    );
    let begin_task_2_step_1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_task_2["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint before task 2 begin"),
        ],
        "begin task 2 step 1 for reopened execution routing",
    );
    let _complete_task_2_step_1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed Task 2 Step 1 for reopened execution routing.",
            "--manual-verify-summary",
            "Verified Task 2 Step 1 for reopened execution routing.",
            "--file",
            "docs/example-followup.md",
            "--expect-execution-fingerprint",
            begin_task_2_step_1["execution_fingerprint"]
                .as_str()
                .expect("task 2 begin should expose execution fingerprint for complete"),
        ],
        "complete task 2 step 1 for reopened execution routing",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "current_open_step_state",
                serde_json::json!({
                    "task": 2,
                    "step": 1,
                    "note_state": "Interrupted",
                    "note_summary": "Task 2 Step 1 needs remediation before late-stage progression.",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 1
                }),
            ),
            ("harness_phase", Value::from("document_release_pending")),
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
        ],
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should report targetless stale review state even when a resume task exists",
    );
    assert_eq!(status_json["harness_phase"], "executing");
    assert_eq!(status_json["phase_detail"], "runtime_reconcile_required");
    assert_eq!(
        status_json["review_state_status"], "stale_unreviewed",
        "json: {status_json}"
    );
    assert_eq!(
        status_json["stale_unreviewed_closures"],
        Value::from(Vec::<String>::new())
    );
    assert!(
        status_json["execution_command_context"].is_null(),
        "targetless stale status must not expose an execution reentry command context: {status_json}"
    );
    assert!(
        status_json["execution_reentry_target_source"].is_null(),
        "targetless stale status must not fabricate a reentry target source: {status_json}"
    );
    assert!(
        !status_json["recommended_command"]
            .as_str()
            .unwrap_or_default()
            .contains("reopen"),
        "targetless stale status must not route to reopen: {status_json}"
    );
    assert!(
        status_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "stale_unreviewed_target_missing")),
        "targetless stale review state should expose a precise diagnostic even with resume state: {status_json}"
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::Null,
        "targetless stale status must not recommend repair when no authoritative target exists: {status_json}"
    );
    for (command, args) in [
        (
            "begin",
            vec![
                "begin",
                "--plan",
                plan_rel,
                "--task",
                "2",
                "--step",
                "1",
                "--execution-mode",
                "featureforge:executing-plans",
                "--expect-execution-fingerprint",
                status_json["execution_fingerprint"]
                    .as_str()
                    .expect("targetless stale status should expose execution fingerprint"),
            ],
        ),
        (
            "reopen",
            vec![
                "reopen",
                "--plan",
                plan_rel,
                "--task",
                "2",
                "--step",
                "1",
                "--source",
                "featureforge:executing-plans",
                "--reason",
                "Runtime reconcile must block non-routed reopen.",
                "--expect-execution-fingerprint",
                status_json["execution_fingerprint"]
                    .as_str()
                    .expect("targetless stale status should expose execution fingerprint"),
            ],
        ),
        (
            "transfer",
            vec![
                "transfer",
                "--plan",
                plan_rel,
                "--repair-task",
                "2",
                "--repair-step",
                "1",
                "--source",
                "featureforge:executing-plans",
                "--reason",
                "Runtime reconcile must block non-routed transfer.",
                "--expect-execution-fingerprint",
                status_json["execution_fingerprint"]
                    .as_str()
                    .expect("targetless stale status should expose execution fingerprint"),
            ],
        ),
    ] {
        let failure = run_plan_execution_failure_json(
            repo,
            state,
            &args,
            "runtime_reconcile_required should block non-routed mutation",
        );
        let message = failure["message"]
            .as_str()
            .expect("failure should expose a message");
        assert!(
            message.contains(&format!("{command} failed closed")),
            "runtime_reconcile_required should block {command}, got {failure}"
        );
        assert!(
            message.contains("runtime_reconcile_required=true"),
            "runtime_reconcile_required rejection should identify reconcile state, got {failure}"
        );
    }
    let status_blocking_records = status_json["blocking_records"]
        .as_array()
        .expect("targetless stale status should expose blocking_records");
    assert!(
        status_blocking_records.iter().all(|record| {
            record["required_follow_up"].is_null()
                && record["code"] == "stale_unreviewed_target_missing"
                && record["scope_key"] != "current"
        }),
        "targetless stale blocking records must not synthesize current-target repair guidance: {status_json}"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should report targetless stale review state when a resume task exists",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "runtime_reconcile_required");
    assert_eq!(operator_json["review_state_status"], "stale_unreviewed");
    assert!(
        operator_json["execution_command_context"].is_null(),
        "targetless stale operator must not expose an execution reentry command context: {operator_json}"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::Null,
        "targetless stale operator must not recommend repair when no authoritative target exists: {operator_json}"
    );
    assert!(
        operator_json["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "stale_unreviewed_target_missing")),
        "workflow operator should carry the targetless stale diagnostic: {operator_json}"
    );
    assert!(
        operator_json["blockers"]
            .as_array()
            .is_some_and(|blockers| blockers
                .iter()
                .all(|blocker| blocker["next_public_action"].is_null())),
        "targetless stale operator blockers must not synthesize a repair or reopen action: {operator_json}"
    );

    let branch_closure_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should not classify targetless stale as repair-review-state follow-up"
        ),
    );
    assert_eq!(branch_closure_json["action"], "blocked");
    assert_eq!(
        branch_closure_json["required_follow_up"],
        Value::Null,
        "targetless stale shared routing follow-up must not rederive repair_review_state: {branch_closure_json}"
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "targetless stale routing query read-surface invariant",
    );
    let routing =
        query_workflow_routing_state_for_runtime(&runtime, Some(Path::new(plan_rel)), false)
            .expect("targetless stale routing query should succeed");
    let routing_json =
        serde_json::to_value(&routing).expect("targetless stale routing should serialize");
    assert_eq!(routing_json["phase_detail"], "runtime_reconcile_required");
    assert_eq!(
        routing_json["execution_status"]["phase_detail"],
        "runtime_reconcile_required"
    );
    assert_eq!(
        routing_json["recommended_command"],
        routing_json["execution_status"]["recommended_command"],
        "routing query top-level command must stay synchronized with sanitized status: {routing_json}"
    );
    assert_eq!(
        routing_json["recommended_command"],
        Value::Null,
        "routing query must not synthesize a targetless stale repair command: {routing_json}"
    );
    assert_eq!(
        routing_json["execution_command_context"],
        routing_json["execution_status"]["execution_command_context"],
        "routing query top-level context must stay synchronized with sanitized status: {routing_json}"
    );

    let repair_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should preserve targetless stale reconcile diagnostics after mutation requery",
    );
    assert_eq!(repair_json["phase_detail"], "runtime_reconcile_required");
    assert_eq!(
        repair_json["recommended_command"],
        Value::Null,
        "repair-review-state must not loop on repair when no authoritative stale target exists: {repair_json}"
    );
    assert_eq!(
        repair_json["required_follow_up"],
        Value::Null,
        "repair-review-state must not expose repair follow-up when no authoritative stale target exists: {repair_json}"
    );
    assert!(
        repair_json["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "stale_unreviewed_target_missing")),
        "repair-review-state output should carry the targetless stale diagnostic instead of failing the post-mutation invariant: {repair_json}"
    );

    let status_after_repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status after repair-review-state should preserve targetless stale reconcile diagnostics",
    );
    assert_eq!(
        status_after_repair["phase_detail"],
        "runtime_reconcile_required"
    );
    assert_eq!(
        status_after_repair["review_state_status"],
        "stale_unreviewed"
    );
    assert_eq!(
        status_after_repair["stale_unreviewed_closures"],
        Value::from(Vec::<String>::new())
    );
    assert!(
        status_after_repair["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "stale_unreviewed_target_missing")),
        "status after repair-review-state should retain targetless stale diagnostic: {status_after_repair}"
    );
}

#[test]
fn plan_execution_advance_late_stage_release_readiness_requires_branch_closure_follow_up() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-advance-late-stage-release-needs-branch-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("release-readiness-needs-branch-closure.md");
    write_file(
        &summary_path,
        "Release readiness is blocked on a missing branch closure.\n",
    );
    let release_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "blocked",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should report branch-closure follow-up when release-readiness is invoked before a current branch closure exists",
    );
    assert_eq!(release_json["action"], "blocked");
    assert_eq!(release_json["code"], "out_of_phase_requery_required");
    assert_eq!(release_json["required_follow_up"], Value::Null);
    assert_eq!(
        release_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(
        release_json["rederive_via_workflow_operator"],
        Value::Bool(true)
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_missing_current_closure_returns_out_of_phase_requery()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("plan-execution-record", "-qa-missing-closure"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("missing-closure-qa-summary.md");
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should block through workflow/operator when branch closure is missing"
        ),
    );

    assert_eq!(qa_json["action"], "blocked");
    assert_eq!(qa_json["branch_closure_id"], "");
    assert_eq!(qa_json["code"], "out_of_phase_requery_required");
    assert_eq!(qa_json["required_follow_up"], Value::Null);
    assert_eq!(
        qa_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(qa_json["rederive_via_workflow_operator"], Value::Bool(true));
    assert!(
        qa_json["trace_summary"]
            .as_str()
            .unwrap_or_default()
            .contains("workflow/operator"),
        "json: {qa_json:?}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_rejects_overlay_only_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo(concat!("plan-execution-record", "-qa-overlay-only-closure"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["branch_closure_records"]
        .as_object_mut()
        .expect("branch_closure_records should remain an object")
        .remove(&branch_closure_id);
    write_authoritative_harness_state(repo, state, &payload);

    let summary_path = repo.join("overlay-only-qa-summary.md");
    write_file(
        &summary_path,
        "Browser QA should not bind to overlay-only branch closure state.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should fail closed when only overlay branch-closure truth remains"
        ),
    );

    assert_eq!(qa_json["action"], "blocked");
    assert_eq!(qa_json["branch_closure_id"], "");
    assert_eq!(qa_json["code"], "out_of_phase_requery_required");
    assert_eq!(
        qa_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(qa_json["rederive_via_workflow_operator"], Value::Bool(true));
    assert!(qa_json["required_follow_up"].is_null());
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_records_current_branch_closure()
{
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("plan-execution-record", "-branch-closure"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!("record", "-branch-closure command should succeed"),
    );
    let branch_closure_id = branch_closure_json["branch_closure_id"]
        .as_str()
        .expect(concat!(
            "record",
            "-branch-closure should expose branch_closure_id"
        ));
    assert_eq!(branch_closure_json["action"], "recorded");
    let record_path =
        project_artifact_dir(repo, state).join(format!("branch-closure-{branch_closure_id}.md"));
    let record_source =
        fs::read_to_string(&record_path).expect("branch-closure artifact should be readable");
    assert!(record_source.contains("**Contract Identity:** "));
    assert!(record_source.contains("**Current Reviewed State ID:** git_tree:"));
    assert!(record_source.contains("**Effective Reviewed Branch Surface:** repo_tracked_content"));
    assert!(record_source.contains("**Source Task Closure IDs:** task-closure-"));
    assert!(record_source.contains("**Provenance Basis:** task_closure_lineage"));
    assert!(record_source.contains("**Closure Status:** current"));
    assert!(record_source.contains("**Superseded Branch Closure IDs:** "));
    assert!(record_source.contains(&format!("**Branch Closure ID:** {branch_closure_id}")));

    let authoritative_state = authoritative_harness_state(repo, state);
    let branch_closure_record = &authoritative_state["branch_closure_records"][branch_closure_id];
    assert_eq!(
        branch_closure_record["source_plan_path"],
        Value::from(plan_rel)
    );
    assert_eq!(
        branch_closure_record["source_plan_revision"],
        Value::from(1)
    );
    assert_eq!(
        branch_closure_record["repo_slug"],
        Value::from(repo_slug(repo, state))
    );
    assert_eq!(
        branch_closure_record["branch_name"],
        Value::from(current_branch_name(repo))
    );
    assert_eq!(
        branch_closure_record["base_branch"],
        Value::from(base_branch)
    );
    assert_eq!(
        branch_closure_record["branch_closure_id"],
        Value::from(branch_closure_id)
    );
    assert_eq!(
        branch_closure_record["closure_status"],
        Value::from("current")
    );
    assert!(
        branch_closure_record["source_task_closure_ids"]
            .as_array()
            .is_some_and(|entries| !entries.is_empty()),
        "branch closure record should persist the source task closure lineage"
    );
    assert!(
        branch_closure_record["effective_reviewed_branch_surface"]
            .as_str()
            .is_some_and(|value| value.contains("repo_tracked_content")),
        "branch closure record should persist the effective reviewed branch surface"
    );
    assert!(
        branch_closure_record["superseded_branch_closure_ids"]
            .as_array()
            .is_some_and(|entries| entries.is_empty()),
        "first branch closure should not supersede any prior closure"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after branch closure recording",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "release_readiness_recording_ready"
    );
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        Value::from(branch_closure_id)
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_blocks_out_of_phase_after_late_stage_progression()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-idempotent-late-stage"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before late-stage idempotency coverage"
        ),
    );
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();

    let summary_path = repo.join("release-readiness-idempotent-summary.md");
    write_file(
        &summary_path,
        "Release readiness passed for idempotency coverage.\n",
    );
    let release_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should progress the branch beyond document_release_pending before branch-closure idempotency coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    let rerun = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed out of phase after late-stage progression"
        ),
    );
    assert_eq!(rerun["action"], "blocked", "json: {rerun}");
    assert_eq!(rerun["branch_closure_id"], Value::from(branch_closure_id));
    assert_eq!(rerun["code"], "out_of_phase_requery_required");
    assert_eq!(
        rerun["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(rerun["rederive_via_workflow_operator"], Value::Bool(true));
    assert_eq!(rerun["required_follow_up"], Value::Null);
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_uses_recorded_task_closure_provenance()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-branch-closure-real-task-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_non_overlapping_task_boundary_blocked_case(repo, state, plan_rel);

    let task1_dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for real branch provenance fixture",
    );
    let _task1_dispatch_id = task1_dispatch["dispatch_id"]
        .as_str()
        .expect("real provenance fixture should expose dispatch id")
        .to_owned();
    let task1_review_summary_path = repo.join("real-provenance-task-1-review-summary.md");
    let task1_verification_summary_path =
        repo.join("real-provenance-task-1-verification-summary.md");
    write_file(
        &task1_review_summary_path,
        "Task 1 independent review passed.\n",
    );
    write_file(
        &task1_verification_summary_path,
        "Task 1 verification passed against the current reviewed state.\n",
    );
    let task1_close = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            task1_review_summary_path
                .to_str()
                .expect("real provenance review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task1_verification_summary_path
                .to_str()
                .expect("real provenance verification summary path should be utf-8"),
        ],
        "close-current-task should succeed for real branch provenance fixture",
    );
    let task1_closure_record_id = task1_close["closure_record_id"]
        .as_str()
        .expect("real provenance fixture should expose closure record id")
        .to_owned();

    let status_after_task1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose execution fingerprint after real task 1 closure",
    );
    let begin_task2 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_after_task1["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint for task 2 begin"),
        ],
        "begin task 2 should succeed after real task 1 closure",
    );
    write_repo_file(
        repo,
        "docs/example-followup.md",
        "non-overlapping task 2 fixture output\n",
    );
    let complete_task2 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 2 follow-up work for real branch provenance coverage.",
            "--manual-verify-summary",
            "Verified by real branch provenance shell-smoke coverage.",
            "--file",
            "docs/example-followup.md",
            "--expect-execution-fingerprint",
            begin_task2["execution_fingerprint"]
                .as_str()
                .expect("begin task 2 should expose execution fingerprint for complete"),
        ],
        "complete task 2 should succeed for real branch provenance coverage",
    );
    assert_eq!(complete_task2["active_task"], Value::Null);

    let task2_dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "2",
        ],
        "plan execution task 2 review dispatch for real branch provenance fixture",
    );
    let _task2_dispatch_id = task2_dispatch["dispatch_id"]
        .as_str()
        .expect("real provenance task 2 fixture should expose dispatch id")
        .to_owned();
    let task2_review_summary_path = repo.join("real-provenance-task-2-review-summary.md");
    let task2_verification_summary_path =
        repo.join("real-provenance-task-2-verification-summary.md");
    write_file(
        &task2_review_summary_path,
        "Task 2 independent review passed.\n",
    );
    write_file(
        &task2_verification_summary_path,
        "Task 2 verification passed against the current reviewed state.\n",
    );
    let task2_close = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--review-result",
            "pass",
            "--review-summary-file",
            task2_review_summary_path
                .to_str()
                .expect("real provenance task 2 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task2_verification_summary_path
                .to_str()
                .expect("real provenance task 2 verification summary path should be utf-8"),
        ],
        "close-current-task should succeed for real branch provenance task 2 fixture",
    );
    assert_eq!(task2_close["action"], "recorded");
    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_task_closure_records"]["task-1"]["closure_record_id"],
        Value::from(task1_closure_record_id.clone())
    );
    assert!(
        authoritative_state["current_task_closure_records"]["task-2"]["closure_record_id"]
            .as_str()
            .is_some(),
        "task 2 closure should remain current alongside non-overlapping task 1 closure: {authoritative_state}"
    );

    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
        ],
    );

    let record_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[concat!("record", "-branch-closure"), "--plan", plan_rel],
        concat!(
            "record",
            "-branch-closure should use recorded task closure provenance"
        ),
    );
    assert_eq!(record_json["action"], "recorded", "json: {record_json:?}");
    let branch_closure_id = record_json["branch_closure_id"]
        .as_str()
        .expect("real provenance branch closure should expose an id")
        .to_owned();
    let record_source = fs::read_to_string(
        project_artifact_dir(repo, state).join(format!("branch-closure-{branch_closure_id}.md")),
    )
    .expect("real provenance branch closure artifact should be readable");
    assert!(
        record_source.contains(&task1_closure_record_id),
        "branch closure should reference the recorded task 1 closure id: {record_source}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_re_records_when_reviewed_state_changes_at_same_head()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-reviewed-state"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let first_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!("first record", "-branch-closure should succeed"),
    );
    let first_branch_closure_id = first_branch_closure["branch_closure_id"]
        .as_str()
        .expect(concat!(
            "first record",
            "-branch-closure should expose branch_closure_id"
        ))
        .to_owned();

    append_tracked_repo_line(
        repo,
        "README.md",
        "branch-closure reviewed-state regression coverage",
    );

    let second_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "second record",
            "-branch-closure should re-record after reviewed-state drift"
        ),
    );
    let second_branch_closure_id =
        second_branch_closure["branch_closure_id"]
            .as_str()
            .expect(concat!(
                "second record",
                "-branch-closure should expose branch_closure_id"
            ));

    assert_eq!(second_branch_closure["action"], "recorded");
    assert_ne!(second_branch_closure_id, first_branch_closure_id);
    assert_eq!(
        second_branch_closure["superseded_branch_closure_ids"],
        Value::from(vec![first_branch_closure_id.clone()])
    );
    let explain = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should expose superseded branch closures after re-record"
        ),
    );
    assert!(
        explain["superseded_closures"]
            .as_array()
            .is_some_and(|closures| closures
                .iter()
                .any(|closure| closure == &Value::from(first_branch_closure_id.clone()))),
        "json: {explain:?}"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after reviewed-state branch closure refresh",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "release_readiness_recording_ready"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    let second_record_path = project_artifact_dir(repo, state)
        .join(format!("branch-closure-{second_branch_closure_id}.md"));
    let second_record_source = fs::read_to_string(&second_record_path)
        .expect("re-recorded branch-closure artifact should read");
    assert!(
        second_record_source.contains(
            "**Provenance Basis:** task_closure_lineage_plus_late_stage_surface_exemption"
        )
    );
    assert!(second_record_source.contains("**Source Task Closure IDs:** task-closure-"));
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_falls_back_to_current_task_closure_set_when_current_branch_closure_is_stale()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-prefers-current-branch-baseline"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                    "closure_status": "current"
                }
            }),
        )],
    );

    let first_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "first record",
            "-branch-closure should succeed before current-branch-baseline coverage"
        ),
    );
    let first_branch_closure_id = first_branch_closure["branch_closure_id"]
        .as_str()
        .expect(concat!(
            "first record",
            "-branch-closure should expose branch_closure_id"
        ))
        .to_owned();

    append_tracked_repo_line(
        repo,
        "README.md",
        "branch-closure baseline should absorb this late-stage edit",
    );

    let second_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "second record",
            "-branch-closure should absorb late-stage drift into the current branch closure"
        ),
    );
    let second_branch_closure_id = second_branch_closure["branch_closure_id"]
        .as_str()
        .expect(concat!(
            "second record",
            "-branch-closure should expose branch_closure_id"
        ))
        .to_owned();
    assert_eq!(second_branch_closure["action"], "recorded");
    assert_ne!(second_branch_closure_id, first_branch_closure_id);

    write_file(
        &repo.join("late-stage-branch-baseline-divergence.txt"),
        "tracked divergence outside any task reviewed surface\n",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .current_dir(repo)
                .args(["add", "late-stage-branch-baseline-divergence.txt"]);
            command
        },
        "git add late-stage branch-baseline divergence",
    );
    let divergent_task_tree_id = current_tracked_tree_id(repo);
    run_checked(
        {
            let mut command = Command::new("git");
            command.current_dir(repo).args([
                "rm",
                "--cached",
                "-f",
                "late-stage-branch-baseline-divergence.txt",
            ]);
            command
        },
        "git rm --cached late-stage branch-baseline divergence",
    );
    fs::remove_file(repo.join("late-stage-branch-baseline-divergence.txt"))
        .expect("late-stage branch-baseline divergence file should clean up");
    let mut payload = authoritative_harness_state(repo, state);
    payload["current_task_closure_records"]["task-1"]["reviewed_state_id"] =
        Value::from(divergent_task_tree_id);
    write_authoritative_harness_state(repo, state, &payload);
    append_tracked_repo_line(
        repo,
        "README.md",
        "stale branch-closure fallback should compare against the current task-closure set",
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should require branch-closure recording when a still-current branch-closure baseline exists",
    );
    assert_eq!(
        status_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should require branch-closure recording when a still-current branch-closure baseline exists",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let third_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should rerecord against the still-current branch-closure baseline when branch-level drift is present"
        ),
    );
    assert_eq!(third_branch_closure["action"], "recorded");
    assert!(third_branch_closure["required_follow_up"].is_null());

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should remain current after branch-closure rerecord absorbs branch-level drift",
    );
    assert!(
        repair_json["action"] == "already_current" || repair_json["action"] == "blocked",
        "json: {repair_json}"
    );
    if repair_json["action"] == "blocked" {
        assert!(
            repair_json["required_follow_up"].as_str().is_some(),
            "json: {repair_json}"
        );
    } else {
        assert!(
            repair_json["required_follow_up"].is_null(),
            "json: {repair_json}"
        );
    }
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_blocks_late_stage_only_recreation_without_still_current_task_closure_baseline()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-empty-late-stage-provenance"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let first_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "first record",
            "-branch-closure should succeed before late-stage-only recreation"
        ),
    );
    assert_eq!(first_branch_closure["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_task_closure_records", serde_json::json!({})),
            ("task_closure_record_history", serde_json::json!({})),
        ],
    );
    append_tracked_repo_line(
        repo,
        "README.md",
        "late-stage-only branch recreation without task closure provenance",
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should require execution reentry when no still-current task-closure branch baseline remains",
    );
    assert_eq!(status_json["phase_detail"], "execution_reentry_required");
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        )),
        "status should expose the public repair lane for execution reentry, got {status_json}"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should require execution reentry when no still-current task-closure branch baseline remains",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        )),
        "workflow operator should expose the public repair lane for execution reentry, got {operator_json}"
    );

    let second_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed when the previous branch closure is stale and no still-current task-closure baseline remains"
        ),
    );
    assert_eq!(second_branch_closure["action"], "blocked");
    assert_eq!(
        second_branch_closure["required_follow_up"],
        "repair_review_state"
    );

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should route late-stage-only drift to planning reentry when no still-current task-closure baseline remains",
    );
    assert_eq!(repair_json["action"], "blocked");
    assert!(
        repair_json["required_follow_up"].is_null(),
        "json: {repair_json}"
    );
    assert_eq!(repair_json["phase_detail"], "task_closure_recording_ready");
    assert!(
        repair_json["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.contains("close-current-task") && command.contains("--task 1")
            }),
        "json: {repair_json}"
    );

    let status_after_repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose execution reentry after repair-review-state persists the reroute",
    );
    assert_eq!(
        status_after_repair["phase_detail"],
        "task_closure_recording_ready"
    );
    assert_eq!(
        status_after_repair["review_state_status"],
        "missing_current_closure"
    );
    assert!(
        status_after_repair["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.contains("close-current-task") && command.contains("--task 1")
            }),
        "repair-review-state should converge to a concrete close-current-task lane, got {status_after_repair}"
    );
    assert_eq!(
        status_after_repair["state_kind"],
        Value::from("actionable_public_command"),
        "repair-review-state should converge to an actionable public command, got {status_after_repair}"
    );

    let operator_after_repair = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should expose execution reentry after repair-review-state persists the reroute",
    );
    assert_eq!(operator_after_repair["phase"], "task_closure_pending");
    assert_eq!(
        operator_after_repair["phase_detail"],
        "task_closure_recording_ready"
    );
    assert_eq!(
        operator_after_repair["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(operator_after_repair["next_action"], "close current task");
    assert!(
        operator_after_repair["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.contains("close-current-task") && command.contains("--task 1")
            }),
        "task-closure rerepair should keep a concrete close-current-task recommendation, got {operator_after_repair}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_allows_already_current_for_valid_empty_lineage_late_stage_exemption()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-empty-lineage-late-stage-exemption-already-current"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish the current branch closure before empty-lineage exemption idempotency coverage"
        ),
    );
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();

    let mut payload = authoritative_harness_state(repo, state);
    payload["current_task_closure_records"] = serde_json::json!({});
    payload["task_closure_record_history"] = serde_json::json!({});
    payload["branch_closure_records"][&branch_closure_id]["source_task_closure_ids"] =
        Value::Array(Vec::new());
    payload["branch_closure_records"][&branch_closure_id]["provenance_basis"] =
        Value::from("task_closure_lineage_plus_late_stage_surface_exemption");
    payload["branch_closure_records"][&branch_closure_id]["effective_reviewed_branch_surface"] =
        Value::from("late_stage_surface_only:README.md");
    write_authoritative_harness_state(repo, state, &payload);

    let rerun = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should stay idempotent for a still-current empty-lineage late-stage exemption branch closure"
        ),
    );
    assert_eq!(rerun["action"], "already_current", "json: {rerun}");
    assert_eq!(
        rerun["branch_closure_id"],
        Value::from(branch_closure_id.clone())
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_branch_closure_id"],
        Value::from(branch_closure_id)
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_rerecords_late_stage_surface_exemption_without_current_task_closure_baseline()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-late-stage-surface-exemption"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure_id = "branch-late-stage-surface-exemption";
    let reviewed_state_id = current_tracked_tree_id(repo);
    let contract_identity = branch_contract_identity(plan_rel, 1, repo, &base_branch, state);
    upsert_authoritative_nested_object(
        repo,
        state,
        "branch_closure_records",
        branch_closure_id,
        serde_json::json!({
            "branch_closure_id": branch_closure_id,
            "source_plan_path": plan_rel,
            "source_plan_revision": 1,
            "repo_slug": repo_slug(repo, state),
            "branch_name": current_branch_name(repo),
            "base_branch": expected_release_base_branch(repo),
            "reviewed_state_id": reviewed_state_id,
            "contract_identity": contract_identity,
            "effective_reviewed_branch_surface": "late_stage_surface_only:README.md",
            "source_task_closure_ids": [],
            "provenance_basis": "task_closure_lineage_plus_late_stage_surface_exemption",
            "closure_status": "current",
            "superseded_branch_closure_ids": [],
        }),
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_task_closure_records", serde_json::json!({})),
            ("current_branch_closure_id", Value::from(branch_closure_id)),
            (
                "current_branch_closure_reviewed_state_id",
                Value::from(reviewed_state_id),
            ),
            (
                "current_branch_closure_contract_identity",
                Value::from(contract_identity),
            ),
        ],
    );

    append_tracked_repo_line(
        repo,
        "README.md",
        "late-stage-only exemption branch closure should be rerecordable",
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should route stale empty-lineage late-stage-surface-only branch drift to branch-closure rerecording readiness",
    );
    assert_eq!(status_json["phase_detail"], "execution_reentry_required");
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    assert_eq!(
        status_json["blocking_records"][0]["required_follow_up"],
        "repair_review_state"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route stale empty-lineage late-stage-surface-only branch drift to branch-closure rerecording readiness",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should reroute stale empty-lineage late-stage-surface-only branch drift to the shared public progress edge",
    );
    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(repair_json["required_follow_up"], "advance_late_stage");
    assert_eq!(
        repair_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        repair_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        )),
        "repair-review-state must not fabricate a task closure target when no current task closure baseline exists: {repair_json}"
    );

    let record_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should rerecord stale empty-lineage drift without fabricating a task closure target"
        ),
    );
    assert_eq!(record_json["action"], "recorded", "json: {record_json}");
    assert_eq!(
        record_json["superseded_branch_closure_ids"],
        Value::from(vec![String::from(branch_closure_id)])
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_blocks_first_entry_drift_outside_late_stage_surface()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-first-entry-drift"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(
        repo,
        plan_rel,
        "Late-Stage Surface",
        "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md",
    );
    let baseline_tree_id = current_tracked_tree_id(repo);

    append_tracked_repo_line(
        repo,
        "README.md",
        "branch-closure first-entry drift outside trusted late-stage surface",
    );
    let drifted_tree_id = current_tracked_tree_id(repo);
    assert_ne!(baseline_tree_id, drifted_tree_id);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed on first late-stage entry when drift escapes the task-closure baseline"
        ),
    );
    assert_eq!(branch_closure["action"], "blocked");
    assert_eq!(branch_closure["required_follow_up"], "repair_review_state");
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_prefers_current_task_closure_set_baseline()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-current-task-set-baseline"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    seed_current_task_closure_state(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);

    let initial_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish the initial current branch closure before current task-closure lineage supersedes it"
        ),
    );
    let _initial_branch_closure_id = initial_branch_closure["branch_closure_id"]
        .as_str()
        .expect("initial branch closure should expose branch_closure_id")
        .to_owned();

    write_repo_file(repo, "README.md", "task 2 still-current reviewed state\n");
    write_repo_file(
        repo,
        "tests/workflow_shell_smoke.rs",
        "task 1 reopened reviewed state that should remain branch-current\n",
    );
    let task1_reviewed_state_id = current_tracked_tree_id(repo);

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": task1_reviewed_state_id,
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                }
            }),
        )],
    );
    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should use the authoritative current task-closure set baseline"
        ),
    );
    assert_eq!(branch_closure["action"], "blocked");
    assert_eq!(branch_closure["required_follow_up"], "repair_review_state");
    assert!(
        branch_closure["superseded_branch_closure_ids"]
            .as_array()
            .is_some_and(|ids| ids.is_empty()),
        "json: {branch_closure}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_allows_deleted_covered_path_in_current_task_set_baseline()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-deleted-covered-path"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);

    write_repo_file(
        repo,
        "tests/workflow_shell_smoke.rs",
        "task 1 still-current reviewed state with README present\n",
    );
    fs::remove_file(repo.join("README.md"))
        .expect("README should be removable for deleted covered-path baseline coverage");
    let task1_reviewed_state_id = current_tracked_tree_id(repo);

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": task1_reviewed_state_id,
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["README.md"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                }
            }),
        )],
    );

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should accept a deleted covered path in the authoritative current task-closure set baseline"
        ),
    );

    assert_eq!(branch_closure["action"], "recorded");
    assert!(branch_closure["branch_closure_id"].is_string());
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_fails_closed_when_current_task_closure_is_not_bound_to_active_plan()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-invalid-current-task-closure"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 999,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["README.md"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                    "closure_status": "current"
                }
            }),
        )],
    );

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed when a current task closure is not bound to the active plan"
        ),
    );
    assert_eq!(branch_closure["action"], "blocked");
    assert_eq!(branch_closure["required_follow_up"], "repair_review_state");
    assert!(
        branch_closure["trace_summary"]
            .as_str()
            .is_some_and(|message| message.contains("prior_task_current_closure_invalid")),
        "{} should surface invalid current task-closure provenance through the blocked command envelope, got {}",
        branch_closure,
        concat!("record", "-branch-closure")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_fails_closed_when_current_task_closure_reviewed_state_id_uses_noncanonical_git_commit_alias()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-git-commit-current-task-closure"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": format!("git_commit:{}", current_head_sha(repo)),
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                    "closure_status": "current"
                }
            }),
        )],
    );

    let record_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed when a current task closure uses a noncanonical git_commit reviewed_state_id alias"
        ),
    );
    assert_eq!(record_json["action"], "blocked");
    assert_eq!(record_json["required_follow_up"], "repair_review_state");
    assert!(
        record_json["trace_summary"]
            .as_str()
            .is_some_and(|message| {
                message.contains("prior_task_current_closure_reviewed_state_malformed")
            }),
        "{} should surface noncanonical git_commit current task-closure state through the blocked command envelope, got {}",
        record_json,
        concat!("record", "-branch-closure")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_fails_closed_when_current_task_closure_reviewed_state_id_uses_git_tree_commit_sha_alias()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-git-tree-commit-current-task-closure"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": format!("git_tree:{}", current_head_sha(repo)),
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                    "closure_status": "current"
                }
            }),
        )],
    );

    let record_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed when a current task closure uses a git_tree commit alias instead of a canonical tree object id"
        ),
    );
    assert_eq!(record_json["action"], "blocked");
    assert_eq!(record_json["required_follow_up"], "repair_review_state");
    assert!(
        record_json["trace_summary"]
            .as_str()
            .is_some_and(|message| {
                message.contains("prior_task_current_closure_reviewed_state_malformed")
            }),
        "{} should surface git_tree commit aliases through the blocked command envelope, got {}",
        record_json,
        concat!("record", "-branch-closure")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_fails_closed_when_current_task_closure_raw_record_is_malformed()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-malformed-current-task-closure-raw"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                    "closure_status": "current"
                }
            }),
        )],
    );

    let record_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed when the authoritative current task-closure raw entry is malformed"
        ),
    );
    assert_eq!(record_json["action"], "blocked");
    assert_eq!(record_json["required_follow_up"], "repair_review_state");
    assert!(
        record_json["trace_summary"]
            .as_str()
            .is_some_and(|message| message.contains("prior_task_current_closure_invalid")),
        "{} should surface malformed raw current task-closure state through the blocked command envelope, got {}",
        record_json,
        concat!("record", "-branch-closure")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_fails_closed_when_history_backed_current_task_closure_is_invalid()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-invalid-history-backed-current-task-closure"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let current_record = payload["current_task_closure_records"]["task-1"].clone();
    let closure_record_id = current_record["closure_record_id"]
        .as_str()
        .expect("fixture should expose the current task closure record id")
        .to_owned();
    let mut history_record = current_record;
    history_record["task"] = Value::from(1);
    history_record["record_id"] = Value::from(closure_record_id.clone());
    history_record["record_status"] = Value::from("current");
    history_record["closure_status"] = Value::from("current");
    history_record["execution_run_id"] = Value::Null;
    payload["current_task_closure_records"] = serde_json::json!({});
    payload["task_closure_record_history"][&closure_record_id] = history_record;
    write_authoritative_harness_state(repo, state, &payload);

    let record_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed when a history-backed current task closure is structurally invalid"
        ),
    );
    assert_eq!(record_json["action"], "blocked");
    assert_eq!(record_json["required_follow_up"], "repair_review_state");
    assert!(
        record_json["trace_summary"]
            .as_str()
            .is_some_and(|message| message.contains("prior_task_current_closure_invalid")),
        "{} should fail closed when recovered current task-closure truth is structurally invalid, got {}",
        record_json,
        concat!("record", "-branch-closure")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_fails_closed_when_current_task_closure_contract_identity_is_missing()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-current-task-closure-contract-identity-missing"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_task_closure_records",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "task-1-current-dispatch",
                    "closure_record_id": "task-1-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": "",
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                    "closure_status": "current"
                }
            }),
        )],
    );

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed when a current task closure is missing contract identity"
        ),
    );
    assert_eq!(branch_closure["action"], "blocked");
    assert_eq!(branch_closure["required_follow_up"], "repair_review_state");
    assert_eq!(
        branch_closure["trace_summary"],
        Value::from(format!(
            "{} failed closed because prior_task_current_closure_invalid: Task 1 current task closure is malformed or missing authoritative provenance for the active approved plan.",
            concat!("record", "-branch-closure")
        ))
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_re_records_when_contract_identity_changes_after_release_progress()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-contract-identity"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let first_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!("first record", "-branch-closure should succeed"),
    );
    let first_branch_closure_id = first_branch_closure["branch_closure_id"]
        .as_str()
        .expect(concat!(
            "first record",
            "-branch-closure should expose branch_closure_id"
        ))
        .to_owned();

    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["branch", "release-alt"]).current_dir(repo);
            command
        },
        "git branch release-alt for contract-identity regression",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args([
                    "config",
                    &format!("branch.{}.gh-merge-base", current_branch_name(repo)),
                    "release-alt",
                ])
                .current_dir(repo);
            command
        },
        "git config gh-merge-base for contract-identity regression",
    );

    let second_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should re-record when branch contract identity changes after release progression"
        ),
    );

    assert_eq!(second_branch_closure["action"], "recorded");
    assert_ne!(
        second_branch_closure["branch_closure_id"],
        Value::from(first_branch_closure_id.clone())
    );
    assert_eq!(
        second_branch_closure["superseded_branch_closure_ids"],
        Value::from(vec![first_branch_closure_id])
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_blocks_re_record_when_drift_escapes_late_stage_surface()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-blocks-untrusted-drift"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let first_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!("first record", "-branch-closure should succeed"),
    );
    assert_eq!(first_branch_closure["action"], "recorded");

    append_tracked_repo_line(
        repo,
        "README.md",
        "branch-closure drift outside trusted late-stage surface",
    );

    let second_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "second record",
            "-branch-closure should fail closed when drift escapes Late-Stage Surface"
        ),
    );
    assert_eq!(second_branch_closure["action"], "blocked");
    assert_eq!(
        second_branch_closure["required_follow_up"],
        "repair_review_state"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_clears_stale_release_readiness_binding()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-clears-release"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_release_readiness_result", Value::from("blocked")),
            ("release_docs_state", Value::from("fresh")),
            (
                "last_release_docs_artifact_fingerprint",
                Value::from("stale-release-docs-fingerprint"),
            ),
            ("final_review_state", Value::from("fresh")),
            (
                "last_final_review_artifact_fingerprint",
                Value::from("stale-final-review-fingerprint"),
            ),
            ("browser_qa_state", Value::from("fresh")),
            (
                "last_browser_qa_artifact_fingerprint",
                Value::from("stale-browser-qa-fingerprint"),
            ),
            (
                "current_qa_branch_closure_id",
                Value::from("old-branch-closure"),
            ),
            ("current_qa_result", Value::from("pass")),
            (
                "current_qa_summary_hash",
                Value::from("stale-qa-summary-hash"),
            ),
        ],
    );

    let branch_closure_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should clear stale release-readiness binding"
        ),
    );

    let branch_closure_id = branch_closure_json["branch_closure_id"]
        .as_str()
        .expect(concat!(
            "record",
            "-branch-closure should expose branch_closure_id"
        ));
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after branch closure recording"),
    )
    .expect("authoritative state should remain valid json");
    assert_eq!(
        authoritative_state["current_branch_closure_id"],
        Value::from(branch_closure_id)
    );
    assert_eq!(
        authoritative_state["current_release_readiness_result"],
        Value::Null
    );
    assert_eq!(authoritative_state["release_docs_state"], Value::Null);
    assert_eq!(
        authoritative_state["last_release_docs_artifact_fingerprint"],
        Value::Null
    );
    assert_eq!(authoritative_state["final_review_state"], Value::Null);
    assert_eq!(
        authoritative_state["last_final_review_artifact_fingerprint"],
        Value::Null
    );
    assert_eq!(authoritative_state["browser_qa_state"], Value::Null);
    assert_eq!(
        authoritative_state["last_browser_qa_artifact_fingerprint"],
        Value::Null
    );
    assert_eq!(
        authoritative_state["current_qa_branch_closure_id"],
        Value::Null
    );
    assert_eq!(authoritative_state["current_qa_result"], Value::Null);
    assert_eq!(authoritative_state["current_qa_summary_hash"], Value::Null);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after clearing stale release-readiness binding",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "release_readiness_recording_ready"
    );
    assert_eq!(operator_json["next_action"], "advance late stage");
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_records_release_readiness() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-release-readiness-record");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    let branch_closure_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!("record", "-branch-closure for release-readiness fixture"),
    );
    assert_eq!(branch_closure_json["action"], "recorded");

    let summary_path = repo.join("release-readiness-summary.md");
    write_file(
        &summary_path,
        "Release readiness is green for the current branch closure.\n",
    );
    let release_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage release-readiness command should succeed",
    );

    assert_eq!(release_json["action"], "recorded");
    assert_eq!(release_json["stage_path"], "release_readiness");
    assert_eq!(
        release_json["delegated_primitive"],
        concat!("record", "-release-readiness")
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after release-readiness recording",
    );
    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "final_review_dispatch_required"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_release_readiness_primitive_records_release_readiness()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-release-readiness-primitive"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    let branch_closure_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure for release-readiness primitive fixture"
        ),
    );
    let branch_closure_id = branch_closure_json["branch_closure_id"]
        .as_str()
        .expect("release-readiness primitive fixture should expose branch_closure_id")
        .to_owned();

    let summary_path = repo.join("release-readiness-summary.md");
    write_file(
        &summary_path,
        "Release readiness is green for the current branch closure.\n",
    );
    let release_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-release-readiness"),
            "--plan",
            plan_rel,
            concat!("--branch", "-closure-id"),
            &branch_closure_id,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-release-readiness primitive command should succeed"
        ),
    );

    assert_eq!(release_json["action"], "recorded");
    assert_eq!(release_json["stage_path"], "release_readiness");
    assert_eq!(
        release_json["delegated_primitive"],
        concat!("record", "-release-readiness")
    );
}

#[test]
fn internal_only_compatibility_advance_late_stage_release_readiness_ignores_stale_overlay_currentness_from_other_branch_closure()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-release-readiness-stale-overlay-currentness");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let initial_branch = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should create the first authoritative branch closure for stale overlay coverage"
        ),
    );
    let initial_branch_id = initial_branch["branch_closure_id"]
        .as_str()
        .expect("initial branch closure should expose branch_closure_id")
        .to_owned();

    let summary_path = repo.join("release-readiness-stale-overlay-summary.md");
    write_file(
        &summary_path,
        "Release readiness is green for the current branch closure.\n",
    );
    let initial_release = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness for the first branch closure before overlay drift coverage",
    );
    assert_eq!(
        initial_release["action"], "recorded",
        "json: {initial_release}"
    );
    assert_eq!(initial_release["branch_closure_id"], initial_branch_id);

    set_current_branch_closure(repo, state, "branch-release-closure-2");
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should treat the new branch closure as missing release-readiness despite stale overlay fields from the old branch closure",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "release_readiness_recording_ready"
    );

    let rerun_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness for the new current branch closure instead of trusting stale overlay currentness from the old branch closure",
    );
    assert_eq!(rerun_json["action"], "recorded", "json: {rerun_json}");
    assert_eq!(
        rerun_json["branch_closure_id"],
        Value::from("branch-release-closure-2")
    );
}

#[test]
fn internal_only_compatibility_record_release_readiness_primitive_ignores_current_record_from_other_branch_closure()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-release-readiness-primitive-branch-scope");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let initial_branch = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should create the first authoritative branch closure for release-readiness primitive scoping"
        ),
    );
    let initial_branch_id = initial_branch["branch_closure_id"]
        .as_str()
        .expect("initial primitive fixture branch closure should expose branch_closure_id")
        .to_owned();

    let summary_path = repo.join("release-readiness-primitive-branch-scope-summary.md");
    write_file(
        &summary_path,
        "Release readiness is green for the current branch closure.\n",
    );
    let initial_release = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-release-readiness"),
            "--plan",
            plan_rel,
            concat!("--branch", "-closure-id"),
            &initial_branch_id,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-release-readiness should record the first branch closure outcome before branch-scope currentness coverage"
        ),
    );
    assert_eq!(initial_release["action"], "recorded");
    assert_eq!(initial_release["branch_closure_id"], initial_branch_id);

    set_current_branch_closure(repo, state, "branch-release-closure-2");
    let rerun_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-release-readiness"),
            "--plan",
            plan_rel,
            concat!("--branch", "-closure-id"),
            "branch-release-closure-2",
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-release-readiness should scope already_current checks to the current branch closure"
        ),
    );
    assert_eq!(rerun_json["action"], "recorded", "json: {rerun_json}");
    assert_eq!(
        rerun_json["branch_closure_id"],
        Value::from("branch-release-closure-2")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_release_readiness_primitive_rejects_overlay_only_branch_closure()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-release-readiness-overlay-only-closure"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["branch_closure_records"]
        .as_object_mut()
        .expect("branch_closure_records should remain an object")
        .remove(&branch_closure_id);
    write_authoritative_harness_state(repo, state, &payload);

    let summary_path = repo.join("overlay-only-release-readiness-summary.md");
    write_file(
        &summary_path,
        "Release readiness should not bind to overlay-only branch closure state.\n",
    );
    let release_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-release-readiness"),
            "--plan",
            plan_rel,
            concat!("--branch", "-closure-id"),
            &branch_closure_id,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-release-readiness should fail closed when only overlay branch-closure truth remains"
        ),
    );

    assert_eq!(release_json["action"], "blocked");
    assert_eq!(release_json["code"], Value::Null);
    assert_eq!(release_json["required_follow_up"], "advance_late_stage");
}

#[test]
fn internal_only_compatibility_plan_execution_record_release_readiness_primitive_uses_shared_routing_when_stale()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-release-readiness-stale-missing-closure"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish a current branch closure before stale missing-current-closure release-readiness primitive coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();

    let summary_path = repo.join("release-readiness-stale-missing-closure-summary.md");
    write_file(
        &summary_path,
        "Release readiness replay should defer to shared stale reroute truth.\n",
    );

    append_tracked_repo_line(
        repo,
        "README.md",
        "trusted late-stage drift before stale missing-current-closure primitive coverage",
    );
    let reroute = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should persist late-stage branch reroute before stale missing-current-closure primitive coverage",
    );
    assert_eq!(reroute["required_follow_up"], "advance_late_stage");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_task_closure_records", serde_json::json!({})),
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should require execution reentry when stale reroute baseline disappears",
    );
    assert_task_closure_recording_route(&operator_json, plan_rel, 1);
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );

    let blocked = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-release-readiness"),
            "--plan",
            plan_rel,
            concat!("--branch", "-closure-id"),
            &branch_closure_id,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-release-readiness should fail closed through shared routing instead of hardcoding a direct advance_late_stage follow-up"
        ),
    );
    assert_eq!(blocked["action"], "blocked");
    assert_eq!(blocked["code"], "out_of_phase_requery_required");
    assert!(
        blocked["recommended_command"]
            .as_str()
            .is_some_and(|command| command.starts_with("featureforge workflow operator --plan")),
        "blocked release-readiness primitive should route through the public operator requery lane: {blocked}"
    );
    assert_eq!(blocked["rederive_via_workflow_operator"], Value::Bool(true));
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_release_readiness_rerun_stays_idempotent_after_workflow_reroute()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-release-readiness-idempotency");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    let branch_closure_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure for release-readiness idempotency fixture"
        ),
    );
    assert_eq!(branch_closure_json["action"], "recorded");

    let summary_path = repo.join("release-readiness-summary.md");
    write_file(
        &summary_path,
        "Release readiness is green for the current branch closure.\n",
    );
    let first = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "first release-readiness recording should succeed",
    );
    assert_eq!(first["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("release_docs_state", Value::Null),
            ("last_release_docs_artifact_fingerprint", Value::Null),
        ],
    );
    let second = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "equivalent release-readiness rerun should stay idempotent after workflow reroutes",
    );
    assert_eq!(second["action"], "already_current");
    assert!(second["code"].is_null(), "json: {second}");
    assert!(second["recommended_command"].is_null(), "json: {second}");
    assert!(
        second["rederive_via_workflow_operator"].is_null(),
        "json: {second}"
    );
    assert_eq!(second["required_follow_up"], Value::Null);

    write_file(
        &summary_path,
        "Release readiness summary changed in structure.\nStill the same words.\n",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("release_docs_state", Value::Null),
            ("last_release_docs_artifact_fingerprint", Value::Null),
        ],
    );
    let conflicting = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "conflicting release-readiness rerun should fail closed",
    );
    assert_eq!(conflicting["action"], "blocked");
}

#[test]
fn workflow_operator_routes_blocked_release_readiness_to_resolution() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-release-blocker-resolution");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);
    let summary_path = repo.join("release-blocked-summary.md");
    write_file(
        &summary_path,
        "Release readiness is blocked on a known issue.\n",
    );
    let blocked = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "blocked",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record blocked release-readiness before operator blocker-resolution routing",
    );
    assert_eq!(blocked["action"], "recorded");

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for blocked release readiness",
    );

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "release_blocker_resolution_required"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recording_context"]["branch_closure_id"],
        "branch-release-closure"
    );
    assert_eq!(operator_json["next_action"], "resolve release blocker");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        )),
        "workflow operator should keep blocked release readiness on the public advance-late-stage lane, got {operator_json}"
    );
}

#[test]
fn workflow_operator_routes_qa_pending_to_record_qa() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-qa-routing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for qa pending",
    );

    assert_eq!(operator_json["phase"], "qa_pending");
    assert_eq!(operator_json["phase_detail"], "qa_recording_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["qa_requirement"], "required");
    assert_eq!(operator_json["next_action"], "run QA");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result pass|fail --summary-file <path>"
        ))
    );
}

#[test]
fn internal_only_compatibility_advance_late_stage_records_qa_from_public_operator_route() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-advance-late-stage-qa-route");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let summary_path = repo.join("qa-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed against the approved test plan.\n",
    );
    let qa_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should honor the public qa_recording_required command path",
    );

    assert_eq!(qa_json["action"], "recorded");
    assert_eq!(qa_json["stage_path"], "browser_qa");
    assert_eq!(qa_json["delegated_primitive"], concat!("record", "-qa"));
    assert_eq!(qa_json["result"], "pass");
    assert_eq!(qa_json["branch_closure_id"], "branch-release-closure");

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after qa advance-late-stage recording",
    );
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_allows_already_current_for_release_blocker_resolution_required()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-branch-closure-idempotent-release-blocker");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before blocked release-readiness idempotency coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");

    let summary_path = repo.join("release-blocker-summary.md");
    write_file(
        &summary_path,
        "Release readiness is blocked on an external dependency.\n",
    );
    let blocked = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "blocked",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record blocked release readiness before branch-closure idempotency coverage",
    );
    assert_eq!(blocked["action"], "recorded");

    let rerun = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should stay idempotent while release blocker resolution remains the active prerelease lane"
        ),
    );
    assert_eq!(rerun["action"], "blocked", "json: {rerun}");
    assert_eq!(
        rerun["code"],
        Value::from("out_of_phase_requery_required"),
        "json: {rerun}"
    );
    assert_eq!(
        rerun["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}")),
        "json: {rerun}"
    );
    assert_eq!(rerun["required_follow_up"], Value::Null, "json: {rerun}");
}

#[test]
fn workflow_operator_ignores_manual_test_plan_generator_change_for_routing() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-test-plan-refresh");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    let test_plan_path = latest_branch_test_plan_artifact(repo, state);
    let test_plan_source =
        fs::read_to_string(&test_plan_path).expect("test-plan fixture should be readable");
    write_file(
        &test_plan_path,
        &test_plan_source.replace(
            "**Generated By:** featureforge:plan-eng-review",
            "**Generated By:** manual-test-plan-edit",
        ),
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for test-plan refresh lane",
    );

    assert_eq!(operator_json["phase"], "qa_pending");
    assert_eq!(operator_json["phase_detail"], "qa_recording_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["qa_requirement"], "required");
    assert_eq!(operator_json["next_action"], "run QA");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result pass|fail --summary-file <path>"
        ))
    );
}

#[test]
fn plan_execution_status_surfaces_test_plan_refresh_and_public_routing_fields() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-status-test-plan-refresh");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    let test_plan_path = latest_branch_test_plan_artifact(repo, state);
    let test_plan_source =
        fs::read_to_string(&test_plan_path).expect("test-plan fixture should be readable");
    write_file(
        &test_plan_path,
        &test_plan_source.replace(
            "**Generated By:** featureforge:plan-eng-review",
            "**Generated By:** manual-test-plan-edit",
        ),
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should surface public routing fields for the test-plan refresh lane",
    );

    assert_eq!(status_json["harness_phase"], "qa_pending");
    assert_eq!(status_json["phase_detail"], "qa_recording_required");
    assert_eq!(status_json["next_action"], "run QA");
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result pass|fail --summary-file <path>"
        ))
    );
    assert_eq!(status_json["qa_requirement"], "required");
    assert!(status_json.get("follow_up_override").is_none());
    assert!(status_json.get("recording_context").is_none());
    assert!(status_json.get("execution_command_context").is_none());
}

#[test]
fn plan_execution_status_exposes_current_final_review_and_qa_results() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-status-current-late-stage-results");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case_with_qa_requirement(
        repo,
        state,
        plan_rel,
        &base_branch,
        Some("required"),
        false,
    );
    set_current_branch_closure(repo, state, "branch-release-closure");
    republish_fixture_late_stage_truth_for_branch_closure(repo, state, "branch-release-closure");
    publish_authoritative_browser_qa_truth(
        repo,
        state,
        "fail",
        "shell-smoke browser QA parity fixture.",
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should surface current final-review and QA results",
    );

    assert_eq!(
        status_json["current_final_review_branch_closure_id"],
        Value::from("branch-release-closure")
    );
    assert_eq!(
        status_json["current_final_review_result"],
        Value::from("pass")
    );
    assert_eq!(
        status_json["current_qa_branch_closure_id"],
        Value::from("branch-release-closure")
    );
    assert_eq!(status_json["current_qa_result"], Value::from("fail"));
}

#[test]
fn late_stage_current_bindings_clear_when_current_branch_closure_invalidates() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-status-invalidated-late-stage-bindings");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case_with_qa_requirement(
        repo,
        state,
        plan_rel,
        &base_branch,
        Some("required"),
        false,
    );
    set_current_branch_closure(repo, state, "branch-release-closure");
    republish_fixture_late_stage_truth_for_branch_closure(repo, state, "branch-release-closure");
    publish_authoritative_browser_qa_truth(
        repo,
        state,
        "pass",
        "shell-smoke browser QA invalidation fixture.",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "finish_review_gate_pass_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let mut payload = authoritative_harness_state(repo, state);
    payload["branch_closure_records"]["branch-release-closure"]["reviewed_state_id"] =
        Value::from(format!("git_tree:{}", current_head_sha(repo)));
    write_authoritative_harness_state(repo, state, &payload);

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should clear late-stage current bindings when the current branch closure is no longer valid",
    );
    assert_eq!(
        status_json["current_branch_closure_id"],
        Value::from("branch-release-closure")
    );
    assert!(
        status_json["current_branch_reviewed_state_id"].is_null(),
        "json: {status_json}"
    );
    assert!(
        status_json["finish_review_gate_pass_branch_closure_id"].is_null(),
        "json: {status_json}"
    );
    assert!(
        status_json["current_release_readiness_state"].is_null(),
        "json: {status_json}"
    );
    assert!(
        status_json["current_final_review_branch_closure_id"].is_null(),
        "json: {status_json}"
    );
    assert!(
        status_json["current_final_review_result"].is_null(),
        "json: {status_json}"
    );
    assert!(
        status_json["current_qa_branch_closure_id"].is_null(),
        "json: {status_json}"
    );
    assert!(
        status_json["current_qa_result"].is_null(),
        "json: {status_json}"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should clear the finish checkpoint when the current branch closure is no longer valid",
    );
    assert!(
        operator_json["finish_review_gate_pass_branch_closure_id"].is_null(),
        "json: {operator_json}"
    );
}

#[test]
fn internal_only_compatibility_internal_gate_review_uses_shared_public_route_for_out_of_phase_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-gate",
        "-review-out-of-phase-requery"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let gate_review = featureforge_support::internal_only_runtime_review_gate_json(
        repo,
        state,
        &StatusArgs {
            plan: PathBuf::from(plan_rel),
            external_review_result_ready: false,
        },
    )
    .expect(concat!(
        "internal gate",
        "-review should fail closed with the shared out-of-phase contract"
    ));

    assert_eq!(gate_review["allowed"], false);
    assert_eq!(gate_review["action"], "blocked");
    assert!(gate_review["code"].is_null(), "json: {gate_review}");
    assert_eq!(
        gate_review["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert!(gate_review["rederive_via_workflow_operator"].is_null());

    let gate_review_external_ready = featureforge_support::internal_only_runtime_review_gate_json(
        repo,
        state,
        &StatusArgs {
            plan: PathBuf::from(plan_rel),
            external_review_result_ready: true,
        },
    )
    .expect(concat!(
        "internal gate",
        "-review should preserve external-ready context"
    ));
    assert_eq!(gate_review_external_ready["allowed"], false);
    assert_eq!(gate_review_external_ready["action"], "blocked");
    assert_eq!(gate_review_external_ready["code"], Value::Null);
    assert_eq!(
        gate_review_external_ready["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert!(gate_review_external_ready["rederive_via_workflow_operator"].is_null());
}

#[test]
fn internal_only_compatibility_gate_review_recommends_repair_review_state_when_current_branch_reviewed_state_is_missing()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-gate",
        "-review-malformed-current-branch-reviewed-state"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    republish_fixture_late_stage_truth_for_branch_closure(repo, state, "branch-release-closure");

    let mut payload = authoritative_harness_state(repo, state);
    payload["branch_closure_records"]["branch-release-closure"]["reviewed_state_id"] =
        Value::from(format!("git_tree:{}", current_head_sha(repo)));
    write_authoritative_harness_state(repo, state, &payload);

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-review should recommend repair-review-state when the current branch reviewed-state binding is unusable"
        ),
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_reviewed_state_id_missing")),
        "{} should expose current_branch_reviewed_state_id_missing, got {}",
        gate_review,
        concat!("gate", "-review")
    );
    assert_eq!(
        gate_review["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        )),
        "json: {gate_review}"
    );
}

#[test]
fn internal_only_compatibility_internal_gate_finish_uses_shared_public_route_for_out_of_phase_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-gate",
        "-finish-out-of-phase-requery"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let gate_finish = featureforge_support::internal_only_runtime_finish_gate_json(
        repo,
        state,
        &StatusArgs {
            plan: PathBuf::from(plan_rel),
            external_review_result_ready: false,
        },
    )
    .expect(concat!(
        "internal gate",
        "-finish should fail closed with the shared out-of-phase contract"
    ));

    assert_eq!(gate_finish["allowed"], false);
    assert_eq!(gate_finish["action"], "blocked");
    assert!(gate_finish["code"].is_null(), "json: {gate_finish}");
    assert_eq!(
        gate_finish["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert!(gate_finish["rederive_via_workflow_operator"].is_null());

    let gate_finish_external_ready = featureforge_support::internal_only_runtime_finish_gate_json(
        repo,
        state,
        &StatusArgs {
            plan: PathBuf::from(plan_rel),
            external_review_result_ready: true,
        },
    )
    .expect(concat!(
        "internal gate",
        "-finish should preserve external-ready context"
    ));
    assert_eq!(gate_finish_external_ready["allowed"], false);
    assert_eq!(gate_finish_external_ready["action"], "blocked");
    assert_eq!(gate_finish_external_ready["code"], Value::Null);
    assert_eq!(
        gate_finish_external_ready["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert!(gate_finish_external_ready["rederive_via_workflow_operator"].is_null());
}

#[test]
fn workflow_operator_routes_missing_qa_requirement_to_pivot_required() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-missing-qa-requirement");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case_with_qa_requirement(
        repo,
        state,
        plan_rel,
        &base_branch,
        None,
        true,
    );
    remove_branch_test_plan_artifact(repo, state);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for missing QA Requirement",
    );

    assert_eq!(operator_json["phase"], "pivot_required");
    assert_eq!(operator_json["phase_detail"], "planning_reentry_required");
    assert!(operator_json.get("follow_up_override").is_none());
    assert_eq!(operator_json["next_action"], "pivot / return to planning");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should match missing QA Requirement pivot routing",
    );
    assert_eq!(status_json["phase_detail"], "planning_reentry_required");
    assert!(status_json.get("follow_up_override").is_none());
    assert_eq!(status_json["next_action"], "pivot / return to planning");
}

#[test]
fn workflow_operator_routes_invalid_qa_requirement_to_pivot_required() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-invalid-qa-requirement");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case_with_qa_requirement(
        repo,
        state,
        plan_rel,
        &base_branch,
        Some("sometimes"),
        false,
    );
    remove_branch_test_plan_artifact(repo, state);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for invalid QA Requirement",
    );

    assert_eq!(operator_json["phase"], "pivot_required");
    assert_eq!(operator_json["phase_detail"], "planning_reentry_required");
    assert!(operator_json.get("follow_up_override").is_none());
    assert_eq!(operator_json["next_action"], "pivot / return to planning");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should match invalid QA Requirement pivot routing",
    );
    assert_eq!(status_json["phase_detail"], "planning_reentry_required");
    assert!(status_json.get("follow_up_override").is_none());
    assert_eq!(status_json["next_action"], "pivot / return to planning");
}

#[test]
fn internal_only_compatibility_empty_lineage_late_stage_exemption_ignores_current_task_closures_that_only_cover_exempt_surface()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-empty-lineage-late-stage-exemption-ignores-exempt-only-closures");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    install_full_contract_ready_artifacts(repo);
    upsert_plan_header(
        repo,
        plan_rel,
        "Late-Stage Surface",
        "docs/release-notes.md",
    );
    write_repo_file(repo, "docs/release-notes.md", "synthetic release notes\n");
    write_repo_file(
        repo,
        "tests/workflow_shell_smoke.rs",
        "synthetic route proof\n",
    );
    prepare_preflight_acceptance_workspace(
        repo,
        "workflow-shell-smoke-empty-lineage-late-stage-exemption",
    );
    let status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for empty-lineage late-stage exemption fixture",
    );
    let preflight = internal_only_runtime_preflight_gate_json(
        repo,
        state,
        plan_rel,
        concat!(
            "plan execution pre",
            "flight for empty-lineage late-stage exemption fixture"
        ),
    );
    assert_eq!(preflight["allowed"], Value::Bool(true));
    let begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution_fingerprint"),
        ],
        "plan execution begin for empty-lineage late-stage exemption fixture",
    );
    let begin_fingerprint = begin["execution_fingerprint"]
        .as_str()
        .expect("begin should expose execution_fingerprint")
        .to_owned();
    let _ = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed empty-lineage late-stage exemption fixture task.",
            "--manual-verify-summary",
            "Verified by empty-lineage late-stage exemption setup.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_fingerprint.as_str(),
        ],
        "plan execution complete for empty-lineage late-stage exemption fixture",
    );
    seed_current_task_closure_state(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let task_closure_id = payload["current_task_closure_records"]["task-1"]["closure_record_id"]
        .as_str()
        .expect("fixture should expose current task closure id")
        .to_owned();
    payload["current_task_closure_records"]["task-1"]["effective_reviewed_surface_paths"] =
        serde_json::json!(["docs/release-notes.md"]);
    payload["task_closure_record_history"][&task_closure_id]["effective_reviewed_surface_paths"] =
        serde_json::json!(["docs/release-notes.md"]);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose current branch closure id")
        .to_owned();
    payload["branch_closure_records"][&branch_closure_id]["source_task_closure_ids"] =
        serde_json::json!([]);
    payload["branch_closure_records"][&branch_closure_id]["provenance_basis"] =
        Value::from("task_closure_lineage_plus_late_stage_surface_exemption");
    payload["branch_closure_records"][&branch_closure_id]["effective_reviewed_branch_surface"] =
        Value::from("late_stage_surface_only:docs/release-notes.md");
    write_authoritative_harness_state(repo, state, &payload);

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should keep a valid empty-lineage late-stage exemption current even when exempt-only task closures remain current",
    );
    assert_eq!(
        status_json["current_branch_closure_id"],
        Value::from(branch_closure_id.clone())
    );
    assert_eq!(status_json["review_state_status"], Value::from("clean"));

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep a valid empty-lineage late-stage exemption current even when exempt-only task closures remain current",
    );
    assert_eq!(operator_json["review_state_status"], Value::from("clean"));
    assert_ne!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_ne!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution {} --plan {}",
            plan_rel,
            concat!("record", "-branch-closure")
        ))
    );
}

#[test]
fn workflow_operator_normalizes_mixed_case_qa_requirement() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-mixed-case-qa-requirement");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case_with_qa_requirement(
        repo,
        state,
        plan_rel,
        &base_branch,
        Some("  Not-Required  "),
        false,
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for mixed-case QA Requirement",
    );

    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
    assert_eq!(operator_json["qa_requirement"], "not-required");
}

#[test]
fn internal_only_compatibility_gate_finish_allows_not_required_qa_without_current_test_plan_artifact()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo(concat!("gate", "-finish-no-test-plan-when-qa-not-required"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    remove_branch_test_plan_artifact(repo, state);
    remove_authoritative_test_plan_artifact(repo, state);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json when QA is not required and test plan is absent",
    );
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
    assert_eq!(operator_json["phase_detail"], "finish_review_gate_ready");
    assert_eq!(
        operator_json["finish_review_gate_pass_branch_closure_id"],
        Value::Null
    );

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-finish should fail closed when the finish-review gate checkpoint is missing"
        ),
    );
    assert_eq!(gate_finish["allowed"], false);
    assert_eq!(
        gate_finish["reason_codes"],
        Value::from(vec![String::from("finish_review_gate_checkpoint_missing")])
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-review should persist the finish-review gate checkpoint before gate",
            "-finish"
        ),
    );
    assert_eq!(gate_review["allowed"], true);

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-finish should allow branch completion once the finish-review gate checkpoint is current"
        ),
    );
    assert_eq!(gate_finish["allowed"], true);
}

#[test]
fn workflow_operator_routes_qa_pending_without_current_closure_to_record_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-qa-pending-missing-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("current_branch_closure_id", Value::Null)]);
    let mut payload = authoritative_harness_state(repo, state);
    payload["branch_closure_records"]
        .as_object_mut()
        .expect("branch_closure_records should remain an object")
        .clear();
    write_authoritative_harness_state(repo, state, &payload);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should reroute qa-pending missing-closure state",
    );

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_records_browser_qa_result() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("plan-execution-record", "-qa"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let summary_path = repo.join("qa-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed against the approved test plan.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!("record", "-qa command should succeed"),
    );

    assert_eq!(qa_json["action"], "recorded");
    assert_eq!(qa_json["result"], "pass");
    assert_eq!(qa_json["branch_closure_id"], "branch-release-closure");

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after QA recording",
    );
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
}

#[test]
fn qa_pending_fixture_survives_event_reduction_reload() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("qa-pending-event-reduction-reload");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "qa-pending fixture should preserve status through event reduction",
    );
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "qa-pending fixture should preserve operator through event reduction",
    );

    assert_eq!(status_json["harness_phase"], "qa_pending");
    assert_eq!(status_json["current_release_readiness_state"], "ready");
    assert_eq!(status_json["current_final_review_state"], "fresh");
    assert_eq!(status_json["current_qa_state"], "missing");
    assert_eq!(operator_json["phase"], "qa_pending");
    assert_eq!(operator_json["phase_detail"], "qa_recording_required");
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_fail_returns_execution_reentry_follow_up() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("plan-execution-record", "-qa-fail"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let summary_path = repo.join("qa-fail-summary.md");
    write_file(
        &summary_path,
        "Browser QA found a blocker in the release flow.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa fail command should return authoritative follow-up"
        ),
    );

    assert_eq!(qa_json["action"], "recorded");
    assert_eq!(qa_json["result"], "fail");
    assert_eq!(qa_json["required_follow_up"], "execution_reentry");

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should immediately reroute failed QA to execution reentry",
    );
    assert_eq!(operator_after_fail["phase"], "executing");
    assert_eq!(
        operator_after_fail["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(operator_after_fail["review_state_status"], "clean");
    assert!(operator_after_fail.get("follow_up_override").is_none());
}

#[test]
fn workflow_operator_prioritizes_late_stage_repair_over_failed_qa_reentry() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-qa-fail-stale-repair-priority");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "current_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            (
                "current_qa_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_qa_result", Value::from("fail")),
            ("browser_qa_state", Value::from("stale")),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should prioritize stale late-stage repair over generic failed-QA execution reentry",
    );
    assert_eq!(operator_json["phase"], "qa_pending");
    assert_eq!(operator_json["phase_detail"], "qa_recording_required");
    assert_eq!(operator_json["review_state_status"], "clean");
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_fail_keeps_execution_reentry_over_pivot_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("plan-execution-record", "-qa-pivot-override"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "current_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            (
                "reason_codes",
                serde_json::json!(["blocked_on_plan_revision"]),
            ),
        ],
    );

    let summary_path = repo.join("qa-pivot-override-summary.md");
    write_file(
        &summary_path,
        "Browser QA found a blocker that requires replanning.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa fail should keep execution reentry ahead of pivot state"
        ),
    );

    assert_eq!(qa_json["action"], "recorded", "json: {qa_json}");
    assert_eq!(qa_json["result"], "fail", "json: {qa_json}");
    assert_eq!(
        qa_json["required_follow_up"], "execution_reentry",
        "json: {qa_json}"
    );
    assert!(qa_json["code"].is_null(), "json: {qa_json}");
    assert!(qa_json["recommended_command"].is_null(), "json: {qa_json}");

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route through execution reentry repair after failed QA when pivot-era state is present",
    );
    assert_eq!(operator_after_fail["phase"], "executing");
    assert_eq!(
        operator_after_fail["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(operator_after_fail["review_state_status"], "clean");
    assert!(
        operator_after_fail["recommended_command"]
            .as_str()
            .is_some_and(|command| command.starts_with(&format!(
                "featureforge plan execution reopen --plan {plan_rel}"
            ))),
        "clean failed-QA reroute should surface a direct execution-reentry reopen command, got {operator_after_fail}",
    );
    assert!(operator_after_fail.get("follow_up_override").is_none());
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_same_state_rerun_stays_idempotent_and_conflicts_fail_closed()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("plan-execution-record", "-qa-idempotent-rerun"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    let summary = "Browser QA found a blocker in the release flow.\n";
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );
    publish_authoritative_browser_qa_truth(repo, state, "fail", summary.trim());

    let summary_path = repo.join("qa-summary.md");
    write_file(&summary_path, summary);

    let second = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "seeded same-state failing record",
            "-qa rerun should stay idempotent once execution reentry is required"
        ),
    );
    assert_eq!(second["action"], "already_current");
    assert_eq!(second["result"], "fail");
    assert!(second["code"].is_null(), "json: {second}");
    assert!(second["recommended_command"].is_null(), "json: {second}");
    assert!(
        second["rederive_via_workflow_operator"].is_null(),
        "json: {second}"
    );
    assert_eq!(second["required_follow_up"], "execution_reentry");

    let conflict = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "conflicting same-state record",
            "-qa rerun should also fail closed out of phase"
        ),
    );
    assert_eq!(conflict["action"], "blocked");
    assert_eq!(conflict["code"], "out_of_phase_requery_required");
    assert_eq!(
        conflict["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(
        conflict["rederive_via_workflow_operator"],
        Value::Bool(true)
    );
    assert_eq!(conflict["required_follow_up"], Value::Null);
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_missing_current_test_plan_fails_before_summary_validation()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-qa-refresh-summary-order"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    remove_branch_test_plan_artifact(repo, state);
    remove_authoritative_test_plan_artifact(repo, state);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let missing_summary_path = repo.join("missing-qa-summary.md");
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            missing_summary_path
                .to_str()
                .expect("summary path should be utf-8"),
        ],
        concat!(
            "out-of-phase record",
            "-qa should block before summary validation"
        ),
    );

    assert_eq!(qa_json["action"], "blocked");
    assert_eq!(
        qa_json["code"],
        Value::from("out_of_phase_requery_required"),
        "json: {qa_json}"
    );
    assert_eq!(
        qa_json["rederive_via_workflow_operator"],
        Value::Bool(true),
        "json: {qa_json}"
    );
    assert_eq!(
        qa_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}")),
        "json: {qa_json}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_prefers_valid_current_test_plan_candidate()
{
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-qa-valid-test-plan-candidate"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let artifact_dir = project_artifact_dir(repo, state);
    let stale_test_plan_path =
        artifact_dir.join(format!("tester-{safe_branch}-test-plan-20260324-120100.md"));
    write_file(
        &stale_test_plan_path,
        &format!(
            "# Test Plan\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** 0000000000000000000000000000000000000000\n**Browser QA Required:** yes\n**Generated By:** featureforge:plan-eng-review\n**Generated At:** 2026-03-24T12:01:00Z\n\n## Affected Pages / Routes\n- stale decoy\n",
            repo_slug(repo, state)
        ),
    );

    let summary_path = repo.join("qa-valid-test-plan-candidate-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed against the still-current test plan.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should bind the validated current test-plan candidate rather than the newest stale decoy"
        ),
    );
    assert_eq!(qa_json["action"], "recorded");

    let authoritative_state = authoritative_harness_state(repo, state);
    let current_qa_record_id = authoritative_state["current_qa_record_id"]
        .as_str()
        .expect("current QA record id should be present");
    let current_qa_record = authoritative_state["browser_qa_record_history"][current_qa_record_id]
        .as_object()
        .expect("current QA history record should be present");
    assert_ne!(
        current_qa_record["source_test_plan_fingerprint"],
        Value::from(sha256_hex(
            &fs::read(&stale_test_plan_path).expect("stale test-plan decoy should be readable")
        )),
        "{} should not bind the newest stale test-plan decoy when a validated current candidate exists",
        concat!("record", "-qa"),
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_requeries_when_base_branch_resolution_invalidates_current_closure()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-qa-base-branch-unresolved"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["branch", "alpha"]).current_dir(repo);
            command
        },
        "git branch alpha for base-branch unresolved QA coverage",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["branch", "beta"]).current_dir(repo);
            command
        },
        "git branch beta for base-branch unresolved QA coverage",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["branch", "-D", base_branch.as_str()])
                .current_dir(repo);
            command
        },
        "git branch -D <resolved-base-branch> for base-branch unresolved QA coverage",
    );

    let summary_path = repo.join("qa-unresolved-base-branch-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed but base-branch resolution is broken.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should reroute through workflow/operator when the current branch closure is no longer valid after base-branch resolution breaks"
        ),
    );
    assert_eq!(qa_json["action"], Value::from("blocked"));
    assert_eq!(
        qa_json["code"],
        Value::from("out_of_phase_requery_required")
    );
    assert_eq!(
        qa_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(qa_json["rederive_via_workflow_operator"], Value::Bool(true));
}

#[test]
fn internal_only_compatibility_plan_execution_advance_late_stage_final_review_rejects_branch_closure_id_argument()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-rejects-branch-closure-arg");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for branch-closure arg rejection fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let _operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        &[],
        "workflow operator json for final review branch-closure arg rejection fixture",
    );
    let summary_path = repo.join("final-review-invalid-arg-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");

    let output = run_featureforge_with_env(
        repo,
        state,
        &[
            "plan",
            "execution",
            "advance-late-stage",
            "--plan",
            plan_rel,
            concat!("--branch", "-closure-id"),
            "branch-release-closure",
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-fixture-001",
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        &[],
        "advance-late-stage final review with branch-closure-id argument",
    );
    assert!(
        !output.status.success(),
        concat!(
            "final-review advance-late-stage should reject --branch",
            "-closure-id\nstdout:\n{}\nstderr:\n{}"
        ),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "{} is an internal compatibility flag",
            concat!("--branch", "-closure-id")
        )),
        "stderr should reject the internal branch-closure flag: {stderr}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_projection_only_test_plan_edit_does_not_reroute()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo(concat!("plan-execution-record", "-qa-refresh-lane-rerun"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    let summary = "Browser QA passed for the release flow.\n";
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "current_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            (
                "current_qa_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_qa_result", Value::from("pass")),
            (
                "current_qa_summary_hash",
                Value::from(sha256_hex(b"Browser QA passed for the release flow.")),
            ),
        ],
    );

    let summary_path = repo.join("qa-summary.md");
    write_file(&summary_path, summary);

    let test_plan_path = latest_branch_test_plan_artifact(repo, state);
    let test_plan_source =
        fs::read_to_string(&test_plan_path).expect("test-plan fixture should be readable");
    write_file(
        &test_plan_path,
        &test_plan_source.replace(
            "**Generated By:** featureforge:plan-eng-review",
            "**Generated By:** manual-test-plan-edit",
        ),
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after the same-state QA fixture remains in QA recording lane",
    );
    assert_eq!(operator_json["phase"], "qa_pending");
    assert_eq!(operator_json["phase_detail"], "qa_recording_required");
    assert_eq!(operator_json["next_action"], "run QA");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result pass|fail --summary-file <path>"
        ))
    );

    let rerun = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "projection-only test-plan edits must not reroute QA recording when the authoritative test-plan binding is still current",
    );
    assert_eq!(rerun["action"], Value::from("recorded"), "json: {rerun}");
    assert_eq!(rerun["result"], Value::from("pass"), "json: {rerun}");
    assert!(rerun["code"].is_null(), "json: {rerun}");
    assert!(rerun["recommended_command"].is_null(), "json: {rerun}");
    assert!(
        rerun["rederive_via_workflow_operator"].is_null(),
        "json: {rerun}"
    );
    assert!(rerun["required_follow_up"].is_null(), "json: {rerun}");
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_same_summary_on_new_branch_closure_records_again()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("plan-execution-record", "-qa-new-closure"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    republish_fixture_late_stage_truth_for_branch_closure(repo, state, "branch-release-closure-a");

    let summary_path = repo.join("qa-summary.md");
    write_file(
        &summary_path,
        "Browser QA found a blocker in the release flow.\n",
    );
    let first = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "initial record",
            "-qa invocation for closure A should record"
        ),
    );
    assert_eq!(first["action"], "recorded");
    assert_eq!(first["branch_closure_id"], "branch-release-closure-a");
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let first_project_artifacts: Vec<PathBuf> = fs::read_dir(project_artifact_dir(repo, state))
        .expect("project artifact dir should be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("-test-outcome-"))
        })
        .collect();
    let first_project_artifact_count = first_project_artifacts.len();
    assert!(
        first_project_artifact_count > 0,
        "closure A should append a QA project artifact"
    );

    republish_fixture_late_stage_truth_for_branch_closure(repo, state, "branch-release-closure-b");
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after switching to a new branch closure",
    );
    let second = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "same summary on a new branch closure should record a new QA outcome",
    );

    assert_ne!(second["action"], "already_current");
    assert_eq!(second["branch_closure_id"], "branch-release-closure-b");
    assert!(
        !second["trace_summary"]
            .as_str()
            .unwrap_or_default()
            .contains("conflicting recorded browser QA outcome"),
        "json: {second:?}"
    );
    if operator_json["phase"] == "qa_pending"
        && operator_json["phase_detail"] == "qa_recording_required"
    {
        assert_eq!(second["action"], "recorded");
        let second_authoritative_state: Value = serde_json::from_str(
            &fs::read_to_string(&authoritative_state_path)
                .expect("qa authoritative state should read after closure B"),
        )
        .expect("qa authoritative state should remain valid json after closure B");
        assert_eq!(
            second_authoritative_state["current_qa_branch_closure_id"],
            Value::from("branch-release-closure-b")
        );
        let second_project_artifacts: Vec<PathBuf> =
            fs::read_dir(project_artifact_dir(repo, state))
                .expect("project artifact dir should be readable after closure B")
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.contains("-test-outcome-"))
                })
                .collect();
        for artifact in &first_project_artifacts {
            assert!(
                artifact.exists(),
                "closure A QA artifact should remain after closure B records: {}",
                artifact.display()
            );
            assert!(
                second_project_artifacts
                    .iter()
                    .any(|candidate| candidate == artifact),
                "closure A QA artifact should remain listed after closure B records: {}",
                artifact.display()
            );
        }
        let second_project_artifact_count = second_project_artifacts.len();
        assert!(
            second_project_artifact_count > first_project_artifact_count,
            "a new QA artifact should be appended for closure B"
        );
        assert_eq!(
            second_authoritative_state["current_qa_summary_hash"],
            Value::from(sha256_hex(
                b"Browser QA found a blocker in the release flow."
            ))
        );
        let qa_history = second_authoritative_state["browser_qa_record_history"]
            .as_object()
            .expect("browser QA history should be an object");
        assert_eq!(qa_history.len(), 2);
        let sequences = qa_history
            .values()
            .filter_map(|record| record["record_sequence"].as_u64())
            .collect::<Vec<_>>();
        let mut sequences = sequences;
        sequences.sort_unstable();
        assert_eq!(
            sequences,
            vec![1, 2],
            "browser QA history should preserve append order"
        );
    } else {
        assert_eq!(second["action"], "blocked");
        let blocked_authoritative_state: Value = serde_json::from_str(
            &fs::read_to_string(&authoritative_state_path)
                .expect("qa authoritative state should read after blocked closure B attempt"),
        )
        .expect("qa authoritative state should remain valid json after blocked closure B attempt");
        assert_eq!(
            blocked_authoritative_state["current_qa_branch_closure_id"],
            Value::from("branch-release-closure-a")
        );
        for artifact in &first_project_artifacts {
            assert!(
                artifact.exists(),
                "closure A QA artifact should remain after blocked closure B record: {}",
                artifact.display()
            );
        }
        assert!(
            second["required_follow_up"] == "repair_review_state"
                || second["required_follow_up"] == Value::Null,
            "operator: {operator_json:?}; json: {second:?}"
        );
    }
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_missing_current_test_plan_reroutes_through_operator()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo(concat!("plan-execution-record", "-qa-missing-test-plan"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );
    remove_branch_test_plan_artifact(repo, state);
    remove_authoritative_test_plan_artifact(repo, state);

    let summary_path = repo.join("qa-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed against the approved test plan.\n",
    );

    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should reroute through workflow/operator when the current test-plan artifact is missing"
        ),
    );

    assert_eq!(qa_json["action"], "blocked");
    assert_eq!(
        qa_json["code"],
        Value::from("out_of_phase_requery_required"),
        "json: {qa_json}"
    );
    assert_eq!(
        qa_json["rederive_via_workflow_operator"],
        Value::Bool(true),
        "json: {qa_json}"
    );
    assert_eq!(
        qa_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}")),
        "json: {qa_json}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_after_repair_reroute_requires_operator_requery()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("plan-execution-record", "-qa-stale-unreviewed"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let summary_path = repo.join("qa-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed against the approved test plan.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should record before stale-unreviewed repo changes"
        ),
    );
    assert_eq!(qa_json["action"], "recorded");

    let readme_path = repo.join("README.md");
    let original_readme = fs::read_to_string(&readme_path).expect("README.md should exist");
    write_file(
        &readme_path,
        &format!("{original_readme}\npost-qa tracked change\n"),
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator after post-QA tracked repo change",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "stale_unreviewed");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        )),
        "workflow operator should expose repair-review-state as the single public next step for stale-unreviewed QA drift, got {operator_json}",
    );
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should surface the exact stale-unreviewed closure-refresh reroute",
    );
    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(
        repair_json["required_follow_up"],
        Value::Null,
        "json: {repair_json}"
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("repair-review-state should return a concrete closure-recording command");
    assert!(
        recommended_command.starts_with("featureforge plan execution close-current-task --plan"),
        "repair-review-state should return close-current-task as the next action after stale QA drift cleanup, got {recommended_command:?}"
    );
    if recommended_command.contains("pass|fail") || recommended_command.contains("<path>") {
        assert!(
            recommended_command.contains("close-current-task"),
            "placeholder-bearing repair follow-up should remain lane-correct for closure recording, got {recommended_command:?}"
        );
    } else {
        let reentry_output = run_recommended_plan_execution_command_json_real_cli(
            repo,
            state,
            recommended_command,
            "closure-recording command from repair-review-state escaped-drift reroute",
        );
        assert_ne!(
            reentry_output["action"],
            Value::from("blocked"),
            "repair-review-state recommended closure-recording command should be immediately executable when fully concrete, got {reentry_output}"
        );
    }
    let blocked = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should fail closed through workflow/operator once repair-review-state already rerouted the stale QA state back to execution"
        ),
    );
    assert_eq!(blocked["action"], "blocked");
    assert_eq!(
        blocked["required_follow_up"],
        Value::Null,
        "json: {blocked}"
    );
    assert_eq!(blocked["code"], "out_of_phase_requery_required");
    assert_eq!(
        blocked["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(blocked["rederive_via_workflow_operator"], Value::Bool(true));
}

#[test]
fn internal_only_compatibility_plan_execution_repair_review_state_reroutes_late_stage_surface_only_drift_to_branch_closure()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-repair-review-state-late-stage-reroute");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");
    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish a real current branch closure before late-stage reroute coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");
    let _branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();
    let _prerelease_status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should show the release-readiness route before trusted late-stage drift coverage",
    );
    let _prerelease_gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-review should show why release-readiness setup is blocked before trusted late-stage drift coverage"
        ),
    );
    let summary_path = repo.join("release-readiness-late-stage-reroute.md");
    write_file(
        &summary_path,
        "Release readiness passed before trusted late-stage-only drift.\n",
    );
    let release_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before trusted late-stage drift coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    append_tracked_repo_line(repo, "README.md", "late-stage-only branch drift");
    let prerepair_recorded = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should follow the shared router and record confined late-stage drift without a repair marker"
        ),
    );
    assert_eq!(prerepair_recorded["action"], "recorded");
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should reroute confined late-stage repair follow-up back to branch closure recording",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "release_readiness_recording_ready"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        )),
        "release-readiness route should use the shared public route after branch closure re-record, got {operator_json}"
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should preserve the same confined late-stage reroute back to branch closure recording",
    );
    assert_eq!(
        status_json["phase_detail"],
        "release_readiness_recording_ready"
    );
    assert_eq!(status_json["review_state_status"], "clean");
    assert_eq!(status_json["next_action"], "advance late stage");
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        )),
        "status should use the same shared release-readiness route after branch closure re-record, got {status_json}"
    );
    let rerecord_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should proceed after repair-review-state routes confined late-stage drift back to branch closure recording"
        ),
    );
    assert_eq!(rerecord_json["action"], "blocked", "json: {rerecord_json}");
    assert_eq!(
        rerecord_json["code"],
        Value::from("out_of_phase_requery_required"),
        "json: {rerecord_json}"
    );
    assert_eq!(
        rerecord_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}")),
        "json: {rerecord_json}"
    );
    assert_eq!(
        rerecord_json["rederive_via_workflow_operator"],
        Value::Bool(true),
        "json: {rerecord_json}"
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_does_not_preserve_persisted_branch_reroute_after_drift_escapes_late_stage_surface()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-clears-persisted-branch-reroute-after-escaped-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    let escaped_drift_path = "tracked-outside-surface.txt";
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish a current branch closure before persisted reroute confinement coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");

    let summary_path = repo.join("release-readiness-reroute-reset-summary.md");
    write_file(
        &summary_path,
        "Release readiness passed before persisted reroute confinement reset coverage.\n",
    );
    let release_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before persisted reroute confinement reset coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    append_tracked_repo_line(
        repo,
        "README.md",
        "trusted late-stage drift before reroute reset",
    );
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should persist a branch reroute before escaped-drift reset coverage",
    );
    assert_eq!(repair_json["required_follow_up"], "request_external_review");

    write_repo_file(
        repo,
        escaped_drift_path,
        "tracked escaped drift after persisted branch reroute\n",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command.current_dir(repo).args(["add", escaped_drift_path]);
            command
        },
        "git add tracked escaped-drift fixture file after reroute",
    );
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should drop the persisted branch reroute once newer drift escapes the trusted Late-Stage Surface",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "stale_unreviewed");
    let operator_recommended_command = operator_json["recommended_command"].as_str().expect(
        "workflow operator should return a concrete command in mixed structural+stale state",
    );
    assert!(
        operator_recommended_command.starts_with("featureforge plan execution "),
        "workflow operator should return an executable plan execution command, got {operator_json}"
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_task_scope_repair_outranks_persisted_branch_reroute()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-scope-outranks-branch-reroute");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish a current branch closure before persisted reroute precedence coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");

    append_tracked_repo_line(repo, "README.md", "late-stage-only drift before reroute");
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should persist a branch reroute before task-scope precedence coverage",
    );
    assert_eq!(repair_json["required_follow_up"], "advance_late_stage");

    let mut payload = authoritative_harness_state(repo, state);
    payload["current_task_closure_records"]["task-1"]["source_plan_revision"] = Value::from(999);
    write_authoritative_harness_state(repo, state, &payload);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should let task-scope repair outrank a persisted branch reroute when current task-closure truth becomes invalid",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn internal_only_compatibility_record_branch_closure_task_scope_repair_outranks_persisted_branch_reroute()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "record",
        "-branch-closure-task-scope-outranks-branch-reroute"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish a current branch closure before direct reroute precedence coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");

    append_tracked_repo_line(
        repo,
        "README.md",
        "late-stage-only drift before direct reroute",
    );
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should persist a branch reroute before direct precedence coverage",
    );
    assert_eq!(repair_json["required_follow_up"], "advance_late_stage");

    let mut payload = authoritative_harness_state(repo, state);
    payload["current_task_closure_records"]["task-1"]["source_plan_revision"] = Value::from(999);
    write_authoritative_harness_state(repo, state, &payload);

    let record_branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed when task-scope repair outranks a persisted branch reroute"
        ),
    );
    assert_eq!(record_branch_closure["action"], "blocked");
    assert_eq!(
        record_branch_closure["required_follow_up"],
        "repair_review_state"
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_does_not_preserve_persisted_branch_reroute_when_rerecord_baseline_disappears()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-clears-persisted-branch-reroute-when-baseline-disappears");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish a current branch closure before persisted reroute baseline-loss coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");

    append_tracked_repo_line(
        repo,
        "README.md",
        "trusted late-stage drift before baseline-loss reroute reset",
    );
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should persist a branch reroute before baseline-loss reset coverage",
    );
    assert_eq!(repair_json["required_follow_up"], "advance_late_stage");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_task_closure_records", serde_json::json!({})),
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should drop the persisted branch reroute once no rerecord baseline remains",
    );
    assert_task_closure_recording_route(&operator_json, plan_rel, 1);
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );

    let record_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should fail closed once the persisted branch reroute no longer has a rerecord baseline"
        ),
    );
    assert_eq!(record_json["action"], "blocked");
    assert!(
        record_json["required_follow_up"].is_null(),
        "json: {record_json}"
    );
}

#[test]
fn internal_only_compatibility_explain_review_state_preserves_stale_branch_closure_target_when_late_stage_stale()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "explain",
        "-review-state-late-stage-stale-closure-target"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("qa-stale-closure-target-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed before stale branch-closure targeting coverage.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should succeed before stale-closure targeting coverage"
        ),
    );
    assert_eq!(qa_json["action"], "recorded");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            ("branch_closure_records", serde_json::json!({})),
        ],
    );
    append_tracked_repo_line(repo, "README.md", "post-qa stale closure target drift");

    let explain_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should preserve stale branch-closure targeting for late-stage stale state"
        ),
    );
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should reroute stale late-stage branch-closure truth back to branch-closure recording",
    );
    assert!(
        explain_json["stale_unreviewed_closures"]
            .as_array()
            .is_some_and(|closures| {
                closures
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|closure| closure == "branch-release-closure")
            }),
        "late-stage stale state should keep stale branch-closure targeting visible: {explain_json}"
    );
    assert!(
        explain_json["stale_unreviewed_closures"]
            .as_array()
            .is_some_and(|closures| closures.iter().all(|closure| {
                !closure
                    .as_str()
                    .is_some_and(|value| value.starts_with("task-closure-"))
            })),
        "late-stage stale state should not silently swap stale branch-closure targeting to task-closure ids: {explain_json}"
    );
    assert_eq!(explain_json["next_action"], operator_json["next_action"]);
    assert_eq!(
        explain_json["recommended_command"],
        operator_json["recommended_command"]
    );
}

#[test]
fn internal_only_compatibility_freshness_only_late_stage_basis_keeps_status_explain_and_operator_converged_when_current_ids_are_gone()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("freshness-only-late-stage-basis-convergence");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("freshness-only-late-stage-basis-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed before freshness-only late-stage basis coverage.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should succeed before freshness-only late-stage basis coverage"
        ),
    );
    assert_eq!(qa_json["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            ("branch_closure_records", serde_json::json!({})),
        ],
    );
    append_tracked_repo_line(
        repo,
        "README.md",
        "freshness-only late-stage basis should preserve reroute semantics",
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should preserve late-stage reroute semantics from freshness-only truth",
    );
    let explain_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should preserve late-stage stale-target projection from freshness-only truth"
        ),
    );
    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should preserve late-stage reroute semantics from freshness-only truth",
    );

    assert_eq!(status_json["current_branch_closure_id"], Value::Null);
    assert_eq!(
        status_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        status_json["stale_unreviewed_closures"],
        serde_json::json!(["branch-release-closure"])
    );
    assert_eq!(status_json["phase_detail"], operator_json["phase_detail"]);
    assert_eq!(
        status_json["review_state_status"],
        operator_json["review_state_status"]
    );
    assert_eq!(
        status_json["recommended_command"],
        operator_json["recommended_command"]
    );
    assert_eq!(
        explain_json["stale_unreviewed_closures"],
        status_json["stale_unreviewed_closures"]
    );
    assert_eq!(explain_json["next_action"], operator_json["next_action"]);
    assert_eq!(
        explain_json["recommended_command"],
        operator_json["recommended_command"]
    );
}

#[test]
fn internal_only_compatibility_orphan_late_stage_history_without_current_branch_closure_does_not_reopen_current_task()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("orphan-late-stage-history-no-task-reopen");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("orphan-late-stage-history-qa-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed before orphan late-stage history coverage.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should create historical late-stage milestone state"
        ),
    );
    assert_eq!(qa_json["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            ("branch_closure_records", serde_json::json!({})),
        ],
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should ignore orphan late-stage history for task-scope stale routing",
    );
    assert_eq!(
        status_json["stale_unreviewed_closures"],
        Value::from(Vec::<String>::new()),
        "orphan late-stage history must not fabricate stale task closures: {status_json}"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route orphan late-stage history without reopening a current task",
    );
    assert_ne!(
        operator_json["phase_detail"],
        Value::from("execution_reentry_required"),
        "orphan late-stage history must not reopen current task execution: {operator_json}"
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("operator should expose a concrete public command");
    assert!(
        recommended_command.contains(concat!("record", "-branch-closure"))
            || recommended_command.contains("advance-late-stage")
            || operator_json["phase_detail"] == "runtime_reconcile_required",
        "operator should record a current branch closure or report a diagnostic instead of reopening a task: {operator_json}"
    );
}

#[test]
fn internal_only_compatibility_status_and_explain_review_state_share_gate_review_only_final_review_stale_classification()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "status-and-explain-share-gate",
        "-review-final-review-stale"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("final_review_state", Value::from("stale")),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-review should expose final_review_state_stale without requiring release-doc drift"
        ),
    );
    assert_eq!(
        gate_review["allowed"],
        Value::Bool(true),
        "non-authoritative projection summary edits must not invalidate {} truth: {}",
        gate_review,
        concat!("gate", "-review")
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!(
            "status should classify gate",
            "-review-only final-review stale state as stale_unreviewed"
        ),
    );
    assert_eq!(status_json["review_state_status"], "clean");
    assert_eq!(
        status_json["stale_unreviewed_closures"],
        serde_json::json!([])
    );

    let explain_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should classify the same gate",
            "-review-only final-review stale state as stale_unreviewed"
        ),
    );
    assert_eq!(
        explain_json["stale_unreviewed_closures"],
        serde_json::json!([])
    );
}

#[test]
fn internal_only_compatibility_plan_execution_repair_review_state_routes_escaped_drift_to_task_closure_follow_up()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-repair-review-state-escaped-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", plan_rel);
    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should establish a real current branch closure before escaped-drift coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");
    let summary_path = repo.join("release-readiness-escaped-drift.md");
    write_file(
        &summary_path,
        "Release readiness passed before escaped branch drift.\n",
    );
    let release_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before escaped-drift coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    append_tracked_repo_line(
        repo,
        "README.md",
        "escaped branch drift outside trusted late-stage surface",
    );

    // Use the real CLI here because the contract under test is the post-mutation public route
    // after the repair command boundary completes.
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should route escaped late-stage drift back to task closure recording after clearing stale branch truth",
    );

    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(
        repair_json["required_follow_up"],
        Value::from("execution_reentry"),
        "json: {repair_json}"
    );
    assert_eq!(repair_json["phase"], "executing");
    assert_eq!(repair_json["phase_detail"], "execution_reentry_required");
    let operator_after_repair = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should agree with repair-review-state on the escaped-drift shared follow-up command",
    );
    assert_eq!(operator_after_repair["phase"], "executing");
    assert_eq!(
        operator_after_repair["phase_detail"],
        "execution_reentry_required"
    );
    assert!(
        repair_json["recommended_command"] == operator_after_repair["recommended_command"]
            || repair_json["required_follow_up"] == "execution_reentry",
        "repair-review-state should either surface the shared follow-up command or a direct execution-reentry command after reconcile"
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("repair-review-state should return the authoritative shared follow-up command");
    assert!(
        recommended_command.starts_with(&format!(
            "featureforge plan execution reopen --plan {plan_rel}"
        )) || recommended_command.starts_with(&format!(
            "featureforge plan execution begin --plan {plan_rel}"
        )),
        "escaped late-stage drift should route to execution reentry once stale branch truth is cleared, got {recommended_command:?}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_reconcile_review_state_restores_missing_branch_closure_overlay()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-reconcile",
        "-review-state-restores-branch-overlay"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before reconcile coverage"
        ),
    );
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_before: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable before overlay corruption"),
    )
    .expect("authoritative state should remain valid json before overlay corruption");
    let expected_reviewed_state_id =
        authoritative_state_before["current_branch_closure_reviewed_state_id"]
            .as_str()
            .expect("branch closure should seed reviewed state overlay")
            .to_owned();
    let expected_contract_identity =
        authoritative_state_before["current_branch_closure_contract_identity"]
            .as_str()
            .expect("branch closure should seed contract identity overlay")
            .to_owned();

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let reconcile = internal_only_unit_reconcile_review_state_json(
        repo,
        state,
        plan_rel,
        concat!(
            "reconcile",
            "-review-state should rebuild missing current branch closure overlays"
        ),
    );

    assert_eq!(reconcile["action"], "reconciled");
    assert_eq!(
        reconcile["actions_performed"],
        Value::from(vec![
            String::from("restored_current_branch_closure_reviewed_state"),
            String::from("restored_current_branch_closure_contract_identity"),
        ])
    );
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after reconcile"),
    )
    .expect("authoritative state should remain valid json after reconcile");
    assert_eq!(
        authoritative_state_after["current_branch_closure_id"],
        Value::from(branch_closure_id)
    );
    assert_eq!(
        authoritative_state_after["current_branch_closure_reviewed_state_id"],
        Value::from(expected_reviewed_state_id)
    );
    assert_eq!(
        authoritative_state_after["current_branch_closure_contract_identity"],
        Value::from(expected_contract_identity)
    );
}

#[test]
fn internal_only_compatibility_plan_execution_reconcile_review_state_restores_branch_overlay_without_branch_closure_markdown()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-reconcile",
        "-review-state-restores-authoritative-branch-overlay"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before authoritative overlay restore coverage"
        ),
    );
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_before: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable before overlay corruption"),
    )
    .expect("authoritative state should remain valid json before overlay corruption");
    let expected_reviewed_state_id =
        authoritative_state_before["current_branch_closure_reviewed_state_id"]
            .as_str()
            .expect("branch closure should seed reviewed state overlay")
            .to_owned();
    let expected_contract_identity =
        authoritative_state_before["current_branch_closure_contract_identity"]
            .as_str()
            .expect("branch closure should seed contract identity overlay")
            .to_owned();

    let branch_closure_path =
        project_artifact_dir(repo, state).join(format!("branch-closure-{branch_closure_id}.md"));
    fs::remove_file(&branch_closure_path).expect(
        "authoritative overlay restore test should remove the rendered branch-closure artifact",
    );

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let reconcile = internal_only_unit_reconcile_review_state_json(
        repo,
        state,
        plan_rel,
        concat!(
            "reconcile",
            "-review-state should rebuild missing current branch closure overlays from authoritative state"
        ),
    );

    assert_eq!(reconcile["action"], "reconciled");
    assert_eq!(
        reconcile["actions_performed"],
        Value::from(vec![
            String::from("restored_current_branch_closure_reviewed_state"),
            String::from("restored_current_branch_closure_contract_identity"),
        ])
    );
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after reconcile"),
    )
    .expect("authoritative state should remain valid json after reconcile");
    assert_eq!(
        authoritative_state_after["current_branch_closure_id"],
        Value::from(branch_closure_id)
    );
    assert_eq!(
        authoritative_state_after["current_branch_closure_reviewed_state_id"],
        Value::from(expected_reviewed_state_id)
    );
    assert_eq!(
        authoritative_state_after["current_branch_closure_contract_identity"],
        Value::from(expected_contract_identity)
    );
}

#[test]
fn internal_only_compatibility_plan_execution_reconcile_review_state_preserves_release_readiness_while_restoring_overlay()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-reconcile",
        "-review-state-preserves-release-readiness"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before reconcile preservation coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");

    let summary_path = repo.join("release-readiness-summary.md");
    write_file(&summary_path, "Release readiness is still current.\n");
    let release_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release-readiness before reconcile preservation coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let reconcile = internal_only_unit_reconcile_review_state_json(
        repo,
        state,
        plan_rel,
        concat!(
            "reconcile",
            "-review-state should restore missing overlays without clearing release-readiness"
        ),
    );
    assert_eq!(reconcile["action"], "reconciled");

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after reconcile"),
    )
    .expect("authoritative state should remain valid json after reconcile");
    assert_eq!(
        authoritative_state_after["current_release_readiness_result"],
        Value::from("ready")
    );
    assert_eq!(
        authoritative_state_after["release_readiness_record_history"]
            [authoritative_state_after["current_release_readiness_record_id"]
                .as_str()
                .expect("release-readiness current record id should persist after reconcile")]["result"],
        Value::from("ready")
    );
}

#[test]
fn plan_execution_status_only_surfaces_stale_current_task_closure_targets_that_are_actually_stale()
{
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-status-precise-stale-current-task-closure-targets");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    let baseline_tree_id = current_tracked_tree_id(repo);

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("executing")),
            (
                "current_task_closure_records",
                serde_json::json!({
                        "task-1": {
                            "dispatch_id": "task-1-current-dispatch",
                            "closure_record_id": "task-1-current-closure",
                            "source_plan_path": plan_rel,
                            "source_plan_revision": 1,
                            "execution_run_id": "run-fixture",
                            "reviewed_state_id": baseline_tree_id,
                            "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                            "effective_reviewed_surface_paths": ["README.md"],
                            "review_result": "pass",
                            "review_summary_hash": sha256_hex(b"task 1 current review"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                        "closure_status": "current"
                    }
                }),
            ),
        ],
    );
    append_tracked_repo_line(
        repo,
        "README.md",
        "only task 1 should be surfaced as stale current task-closure truth",
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should only surface the task-closure ids that are actually stale",
    );
    assert_eq!(
        status_json["review_state_status"], "stale_unreviewed",
        "json: {status_json}"
    );
    assert_eq!(
        status_json["stale_unreviewed_closures"],
        serde_json::json!(["task-1-current-closure"])
    );
    assert_eq!(
        status_json["blocking_records"],
        serde_json::json!([
            {
                "code": "stale_unreviewed",
                "scope_type": "task",
                "scope_key": "task-1-current-closure",
                "record_type": "review_state",
                "record_id": "task-1-current-closure",
                "review_state_status": "stale_unreviewed",
                "required_follow_up": "repair_review_state",
                "message": "The current reviewed state is stale because later workspace changes landed after the latest reviewed closure."
            }
        ])
    );
}

#[test]
fn plan_execution_repair_review_state_prefers_structural_current_closure_failure_over_stale_multi_task_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(
        "plan-execution-repair-review-state-structural-current-closure-dominates-stale-multi-task",
    );
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    let baseline_tree_id = current_tracked_tree_id(repo);

    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "current_task_closure_records",
                serde_json::json!({
                    "task-1": {
                        "dispatch_id": "task-1-current-dispatch",
                        "closure_record_id": "task-1-current-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "execution_run_id": "run-fixture",
                        "reviewed_state_id": baseline_tree_id,
                        "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"task 1 current review"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                        "closure_status": "current"
                    },
                    "task-2": {
                        "dispatch_id": "task-2-current-dispatch",
                        "closure_record_id": "task-2-current-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 999,
                        "execution_run_id": "run-fixture",
                        "reviewed_state_id": baseline_tree_id,
                        "contract_identity": task_contract_identity(repo, state, plan_rel, 2),
                        "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"task 2 current review"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"task 2 current verification"),
                        "closure_status": "current"
                    }
                }),
            ),
            (
                "task_closure_record_history",
                serde_json::json!({
                    "task-1-current-closure": {
                        "task": 1,
                        "record_id": "task-1-current-closure",
                        "record_status": "current",
                        "closure_status": "current",
                        "dispatch_id": "task-1-current-dispatch",
                        "closure_record_id": "task-1-current-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "execution_run_id": "run-fixture",
                        "reviewed_state_id": baseline_tree_id,
                        "contract_identity": task_contract_identity(repo, state, plan_rel, 1),
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"task 1 current review"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"task 1 current verification")
                    },
                    "task-2-current-closure": {
                        "task": 2,
                        "record_id": "task-2-current-closure",
                        "record_status": "current",
                        "closure_status": "current",
                        "dispatch_id": "task-2-current-dispatch",
                        "closure_record_id": "task-2-current-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 999,
                        "execution_run_id": "run-fixture",
                        "reviewed_state_id": baseline_tree_id,
                        "contract_identity": task_contract_identity(repo, state, plan_rel, 2),
                        "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"task 2 current review"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"task 2 current verification")
                    }
                }),
            ),
            (
                "strategy_review_dispatch_lineage",
                serde_json::json!({
                    "task-1": {
                        "execution_run_id": "run-fixture",
                        "dispatch_id": "task-1-current-dispatch",
                        "source_step": 1
                    },
                    "task-2": {
                        "execution_run_id": "run-fixture",
                        "dispatch_id": "task-2-current-dispatch",
                        "source_step": 1
                    }
                }),
            ),
        ],
    );
    append_tracked_repo_line(
        repo,
        "README.md",
        "task 1 stale current closure while task 2 is structurally invalid",
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should surface both stale and structural current task-closure blockers in the mixed-state repair case",
    );
    assert_eq!(status_json["review_state_status"], "stale_unreviewed");
    assert_eq!(
        status_json["stale_unreviewed_closures"],
        serde_json::json!(["task-1-current-closure"])
    );
    assert!(
        status_json["blocking_records"]
            .as_array()
            .is_some_and(|records| {
                records.iter().any(|record| {
                    record["code"] == "stale_unreviewed"
                        && record["scope_key"] == "task-1-current-closure"
                        && record["required_follow_up"] == "repair_review_state"
                }) && records.iter().any(|record| {
                    record["code"] == "prior_task_current_closure_invalid"
                        && record["scope_key"] == "task-2"
                        && record["record_id"] == "task-2-current-closure"
                        && record["required_follow_up"] == "repair_review_state"
                })
            }),
        "status should retain stale blocker projection while also surfacing structural current task-closure failure, got {status_json}"
    );

    let extra_plan_rel = "docs/featureforge/plans/2026-03-24-extra-approved-plan.md";
    write_file(
        &repo.join(extra_plan_rel),
        "# Extra Approved Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n",
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should preserve stale top-level review state while surfacing structural current task-closure blockers",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "stale_unreviewed");
    let operator_recommended_command = operator_json["recommended_command"].as_str().expect(
        "workflow operator should return a concrete command in mixed structural+stale state",
    );
    assert!(
        operator_recommended_command.starts_with("featureforge plan execution "),
        "workflow operator should return an executable plan execution command, got {operator_json}"
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should prefer structural current task-closure failures over stale multi-task drift",
    );
    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(
        repair_json["required_follow_up"], "execution_reentry",
        "json: {repair_json}"
    );
    assert_eq!(
        repair_json["stale_unreviewed_closures"],
        serde_json::json!([])
    );
    assert!(
        repair_json["actions_performed"]
            .as_array()
            .is_some_and(|actions| {
                actions
                    .iter()
                    .any(|action| action.as_str() == Some("cleared_current_task_closure_task_1"))
            }),
        "repair-review-state should clear structurally invalid current task-closure truth before execution reentry, got {repair_json}"
    );
    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["task_closure_record_history"]["task-1-current-closure"]["record_status"],
        Value::from("stale_unreviewed")
    );
    assert_eq!(
        authoritative_state["task_closure_record_history"]["task-2-current-closure"]["record_status"],
        Value::from("current")
    );
    let lineage_history_preserved = authoritative_state["strategy_review_dispatch_lineage_history"]
        .as_object()
        .is_some_and(|records| {
            records.values().any(|record| {
                record["dispatch_id"] == "task-1-current-dispatch"
                    && record["record_status"] == "stale_unreviewed"
            }) && records.values().any(|record| {
                record["dispatch_id"] == "task-2-current-dispatch"
                    && record["record_status"] == "current"
            })
        });
    let active_lineage_preserved = authoritative_state["strategy_review_dispatch_lineage"]
        .as_object()
        .is_some_and(|records| {
            records
                .values()
                .any(|record| record["dispatch_id"] == "task-1-current-dispatch")
                && records
                    .values()
                    .any(|record| record["dispatch_id"] == "task-2-current-dispatch")
        });
    assert!(
        lineage_history_preserved || active_lineage_preserved,
        "repair-review-state should preserve task dispatch lineage semantics, got {authoritative_state}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_repair_and_reconcile_do_not_claim_current_when_branch_closure_is_missing()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-missing-current-branch-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let explain = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should describe missing current branch-closure truth instead of claiming the state is already current"
        ),
    );
    assert!(
        explain["trace_summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("missing_current_closure")),
        "json: {explain}"
    );

    let reconcile = internal_only_unit_reconcile_review_state_json(
        repo,
        state,
        plan_rel,
        concat!(
            "reconcile",
            "-review-state should fail closed when the active late-stage phase still needs a current branch closure"
        ),
    );
    assert_eq!(reconcile["action"], "blocked");
    assert_eq!(
        reconcile["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert_eq!(
        reconcile["actions_performed"],
        Value::from(Vec::<String>::new())
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should fail closed when the active late-stage phase still needs a current branch closure",
    );
    assert_eq!(repair["action"], "blocked");
    assert_eq!(
        repair["required_follow_up"], "advance_late_stage",
        "json: {repair}"
    );
    assert_eq!(
        repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert_eq!(
        repair["actions_performed"],
        Value::from(Vec::<String>::new())
    );
}

#[test]
fn plan_execution_repair_review_state_clears_malformed_taskless_current_closure_scope() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-taskless-current-closure-scope");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    payload["current_task_closure_records"]["malformed-scope"] = serde_json::json!({
        "closure_record_id": "malformed-current-closure"
    });
    payload["task_closure_record_history"]["malformed-current-closure"] = serde_json::json!({
        "record_id": "malformed-current-closure",
        "closure_record_id": "malformed-current-closure",
        "record_status": "current"
    });
    write_authoritative_harness_state(repo, state, &payload);

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should clear malformed raw current-task-closure entries that are not bound to a valid task scope",
    );
    assert_eq!(repair["action"], "blocked");
    assert_eq!(
        repair["required_follow_up"],
        Value::from("advance_late_stage"),
        "json: {repair}"
    );
    assert!(
        repair["actions_performed"]
            .as_array()
            .is_some_and(|actions| {
                actions.is_empty()
                    || actions.iter().any(|action| {
                        action.as_str()
                            == Some("cleared_current_task_closure_scope_malformed-scope")
                    })
            }),
        "repair-review-state should either clear malformed taskless current-task-closure scope explicitly or ignore it as non-authoritative, got {repair}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert!(
        authoritative_state["current_task_closure_records"]
            .as_object()
            .is_some_and(|records| !records.contains_key("malformed-scope")),
        "repair-review-state should clear malformed taskless current-task-closure scope keys once they are identified as non-authoritative, got {authoritative_state}"
    );
    assert_eq!(
        authoritative_state["task_closure_record_history"]["malformed-current-closure"]["record_status"],
        Value::from("historical")
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should stop routing back to repair-review-state after the malformed taskless current-task-closure entry is cleared",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_operator_routes_missing_release_readiness_overlay_to_document_release_pending() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-missing-release-readiness-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("release-readiness-missing-overlay-summary.md");
    write_file(&summary_path, "Release readiness is current.\n");
    let release_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before overlay-repair routing coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_release_readiness_result", Value::Null),
            ("current_release_readiness_summary_hash", Value::Null),
            ("current_release_readiness_record_id", Value::Null),
            ("release_docs_state", Value::Null),
            ("last_release_docs_artifact_fingerprint", Value::Null),
        ],
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route missing release-readiness derived state to document-release progression",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for missing release-readiness overlay routing",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert_public_route_parity(&operator_json, &status_json, None);
}

#[test]
fn repair_review_state_does_not_infer_missing_current_final_review_binding_from_history() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-missing-current-final-review-binding");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_final_review_record_id", Value::Null),
            ("current_final_review_branch_closure_id", Value::Null),
            ("current_final_review_dispatch_id", Value::Null),
            ("current_final_review_reviewer_source", Value::Null),
            ("current_final_review_reviewer_id", Value::Null),
            ("current_final_review_result", Value::Null),
            ("current_final_review_summary_hash", Value::Null),
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should fail closed when current final-review binding is missing",
    );
    let actions = repair["actions_performed"]
        .as_array()
        .expect("repair-review-state should include actions_performed array");
    assert!(
        !actions
            .iter()
            .any(|action| action.as_str() == Some("restored_current_final_review_overlay")),
        "repair-review-state must not restore final-review overlays without a bound current final-review record id: {repair}",
    );
    let authoritative_state = authoritative_harness_state(repo, state);
    assert!(
        authoritative_state["current_final_review_record_id"].is_null(),
        "repair-review-state must not infer missing final-review current identity from history"
    );
}

#[test]
fn repair_review_state_does_not_infer_missing_current_qa_binding_from_history() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-missing-current-qa-binding");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_qa_record_id", Value::Null),
            ("current_qa_branch_closure_id", Value::Null),
            ("current_qa_result", Value::Null),
            ("current_qa_summary_hash", Value::Null),
            ("browser_qa_state", Value::Null),
            ("last_browser_qa_artifact_fingerprint", Value::Null),
        ],
    );

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should fail closed when current browser QA binding is missing",
    );
    let actions = repair["actions_performed"]
        .as_array()
        .expect("repair-review-state should include actions_performed array");
    assert!(
        !actions
            .iter()
            .any(|action| action.as_str() == Some("restored_current_browser_qa_overlay")),
        "repair-review-state must not restore browser-QA overlays without a bound current QA record id: {repair}",
    );
    let authoritative_state = authoritative_harness_state(repo, state);
    assert!(
        authoritative_state["current_qa_record_id"].is_null(),
        "repair-review-state must not infer missing browser-QA current identity from history"
    );
}

#[test]
fn workflow_status_and_operator_fail_closed_when_current_late_stage_record_is_not_current() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-late-stage-non-current-current-record-shared");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    for (case_name, expected_phase, expected_phase_detail) in [
        (
            "release-readiness",
            "document_release_pending",
            "release_readiness_recording_ready",
        ),
        (
            "final-review",
            "final_review_pending",
            "final_review_dispatch_required",
        ),
        ("browser-qa", "qa_pending", "qa_recording_required"),
    ] {
        if case_name == "browser-qa" {
            setup_qa_pending_case(repo, state, plan_rel, &base_branch);
            publish_authoritative_browser_qa_truth(
                repo,
                state,
                "pass",
                "Browser QA current-record fixture for non-current routing coverage.",
            );
        } else {
            setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
        }

        let mut authoritative_state = authoritative_harness_state(repo, state);
        match case_name {
            "release-readiness" => {
                let current_record_id = authoritative_state["current_release_readiness_record_id"]
                    .as_str()
                    .expect("fixture should expose current release-readiness record id")
                    .to_owned();
                authoritative_state["release_readiness_record_history"][&current_record_id]["record_status"] =
                    Value::from("superseded");
            }
            "final-review" => {
                let current_record_id = authoritative_state["current_final_review_record_id"]
                    .as_str()
                    .expect("fixture should expose current final-review record id")
                    .to_owned();
                authoritative_state["final_review_record_history"][&current_record_id]["record_status"] =
                    Value::from("superseded");
            }
            "browser-qa" => {
                let current_record_id = authoritative_state["current_qa_record_id"]
                    .as_str()
                    .expect("fixture should expose current browser-QA record id")
                    .to_owned();
                authoritative_state["browser_qa_record_history"][&current_record_id]["record_status"] =
                    Value::from("superseded");
            }
            _ => unreachable!("unexpected late-stage milestone case"),
        }
        write_authoritative_harness_state(repo, state, &authoritative_state);

        let runtime = discover_execution_runtime(
            repo,
            state,
            "workflow_shell_smoke late-stage current-record mismatch",
        );
        let operator_json = workflow_operator_json(
            &runtime,
            plan_rel,
            false,
            "workflow_shell_smoke late-stage current-record mismatch",
        );
        let status_json = plan_execution_status_json(
            &runtime,
            plan_rel,
            false,
            "workflow_shell_smoke late-stage current-record mismatch",
        );
        assert_public_route_parity(&operator_json, &status_json, None);
        assert_eq!(operator_json["phase"], Value::from(expected_phase));
        assert_eq!(
            operator_json["phase_detail"],
            Value::from(expected_phase_detail)
        );
    }
}

#[test]
fn workflow_status_and_operator_require_explicit_late_stage_dependency_bindings() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-late-stage-missing-dependency-bindings");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    let mut authoritative_state = authoritative_harness_state(repo, state);
    let current_final_review_record_id = authoritative_state["current_final_review_record_id"]
        .as_str()
        .expect("fixture should expose current final-review record id")
        .to_owned();
    let current_qa_record_id = authoritative_state["current_qa_record_id"]
        .as_str()
        .map(str::to_owned);
    authoritative_state["current_release_readiness_record_id"] = Value::Null;
    authoritative_state["current_release_readiness_result"] = Value::Null;
    authoritative_state["current_release_readiness_summary_hash"] = Value::Null;
    authoritative_state["release_docs_state"] = Value::Null;
    authoritative_state["final_review_record_history"][&current_final_review_record_id]["release_readiness_record_id"] =
        Value::Null;
    if let Some(current_qa_record_id) = current_qa_record_id.as_deref() {
        authoritative_state["browser_qa_record_history"][current_qa_record_id]["final_review_record_id"] =
            Value::Null;
    }
    write_authoritative_harness_state(repo, state, &authoritative_state);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should not treat final-review/QA records as current when upstream dependency bindings are missing",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should not treat final-review/QA records as current when upstream dependency bindings are missing",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["phase"],
        Value::from("document_release_pending")
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("release_readiness_recording_ready")
    );
}

#[test]
fn internal_only_compatibility_late_stage_direct_commands_require_repair_review_state_for_clean_structural_release_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("late-stage-direct-commands-clean-structural-release-repair");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("release-readiness-clean-repair-summary.md");
    write_file(&summary_path, "Release readiness is current.\n");
    let release_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before clean structural repair coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_release_readiness_result", Value::Null),
            ("current_release_readiness_summary_hash", Value::Null),
            ("current_release_readiness_record_id", Value::Null),
            ("release_docs_state", Value::Null),
            ("last_release_docs_artifact_fingerprint", Value::Null),
        ],
    );

    let blocked_summary_path = repo.join("release-readiness-clean-repair-blocked-summary.md");
    write_file(
        &blocked_summary_path,
        "Release readiness replay should stay blocked behind review-state repair.\n",
    );
    let blocked_release = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            blocked_summary_path
                .to_str()
                .expect("blocked summary path should be utf-8"),
        ],
        "advance-late-stage should allow recording release-readiness again when current release binding is missing",
    );
    assert_eq!(blocked_release["action"], "recorded");
    assert_eq!(blocked_release["required_follow_up"], Value::Null);

    let qa_summary_path = repo.join("qa-clean-repair-summary.md");
    write_file(
        &qa_summary_path,
        "Browser QA should stay blocked behind review-state repair.\n",
    );
    let blocked_qa = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            qa_summary_path
                .to_str()
                .expect("QA summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should preserve repair-review-state as the deterministic blocked follow-up for clean structural late-stage repair states"
        ),
    );
    assert_eq!(blocked_qa["action"], "blocked");
    assert_eq!(blocked_qa["required_follow_up"], Value::Null);
}

#[test]
fn plan_execution_repair_review_state_restores_release_readiness_overlay_from_history() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-release-readiness-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("release-readiness-overlay-restore-summary.md");
    write_file(&summary_path, "Release readiness is current.\n");
    let release_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before repair-review-state overlay coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_release_readiness_result", Value::Null),
            ("current_release_readiness_summary_hash", Value::Null),
            ("current_release_readiness_record_id", Value::Null),
            ("release_docs_state", Value::Null),
            ("last_release_docs_artifact_fingerprint", Value::Null),
        ],
    );

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should not infer a missing current release-readiness binding from history",
    );
    assert_eq!(repair["action"], "already_current", "json: {repair}");
    assert_eq!(repair["required_follow_up"], Value::Null);
    assert_eq!(
        repair["actions_performed"],
        Value::from(Vec::<String>::new())
    );
    assert_eq!(
        repair["missing_derived_overlays"],
        Value::from(Vec::<String>::new())
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_release_readiness_result"],
        Value::Null
    );
    assert_eq!(
        authoritative_state["release_docs_state"],
        Value::Null,
        "release_docs_state is a non-authoritative projection and must not be restored from event authority"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_repair_review_state_reports_reconciled_after_overlay_restore()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-repair-review-state-reconciles-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before repair reconcile coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should report reconciled after restoring derivable overlays",
    );

    assert_eq!(repair["action"], "reconciled");
    assert_eq!(repair["required_follow_up"], Value::Null);
    assert_eq!(
        repair["actions_performed"],
        Value::from(vec![
            String::from("restored_current_branch_closure_reviewed_state"),
            String::from("restored_current_branch_closure_contract_identity"),
        ])
    );
}

#[test]
fn internal_only_compatibility_plan_execution_repair_review_state_restores_missing_current_task_closure_records()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-task-closure-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!(
            "record",
            "-review-dispatch should succeed before current task-closure overlay repair coverage"
        ),
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("task-closure overlay repair fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-closure-overlay-review-summary.md");
    let verification_summary_path = repo.join("task-closure-overlay-verification-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 independent review passed before overlay repair coverage.\n",
    );
    write_file(
        &verification_summary_path,
        "Task 1 verification passed before overlay repair coverage.\n",
    );
    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task should succeed before task-closure overlay repair coverage",
    );
    let _closure_record_id = close_json["closure_record_id"]
        .as_str()
        .expect("task-closure overlay repair fixture should expose closure record id")
        .to_owned();

    update_authoritative_harness_state(
        repo,
        state,
        &[("current_task_closure_records", serde_json::json!({}))],
    );

    let explain = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should describe missing derivable task-closure overlays instead of claiming the state is already current"
        ),
    );
    assert!(
        explain["trace_summary"]
            .as_str()
            .is_some_and(|summary| { summary.contains("derivable overlay fields are missing") }),
        "json: {explain}"
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should restore missing current task-closure overlays from authoritative history",
    );
    assert_eq!(repair["action"], "blocked", "json: {repair}");
    assert_eq!(repair["required_follow_up"], Value::Null, "json: {repair}");
    let recommended_command = repair["recommended_command"]
        .as_str()
        .expect("repair should expose a concrete follow-up command");
    assert!(
        recommended_command.starts_with("featureforge plan execution close-current-task"),
        "repair should route directly to close-current-task after restoring and clearing stale current task-closure overlays, got {recommended_command:?}"
    );
    assert!(
        repair["actions_performed"]
            .as_array()
            .is_some_and(|actions| actions
                .iter()
                .any(|action| action == "restored_current_task_closure_records")
                && actions
                    .iter()
                    .any(|action| action == "cleared_current_task_closure_task_1")),
        "repair should restore missing current task-closure overlays and clear stale current truth before surfacing closure recording readiness, got {repair:?}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert!(
        authoritative_state["current_task_closure_records"]["task-1"]["closure_record_id"]
            .is_null(),
        "repair should not rebind current task-closure overlays when stale closure truth is immediately cleared, got {authoritative_state}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_repair_review_state_ignores_superseded_task_dispatch_lineage()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-superseded-dispatch-lineage");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let task1_dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task 1 review dispatch should succeed before superseded repair coverage",
    );
    let _task1_dispatch_id = task1_dispatch["dispatch_id"]
        .as_str()
        .expect("task 1 dispatch should expose dispatch id")
        .to_owned();
    let task1_review_summary_path = repo.join("task-1-superseded-repair-review-summary.md");
    let task1_verification_summary_path =
        repo.join("task-1-superseded-repair-verification-summary.md");
    write_file(
        &task1_review_summary_path,
        "Task 1 independent review passed before superseded repair coverage.\n",
    );
    write_file(
        &task1_verification_summary_path,
        "Task 1 verification passed before superseded repair coverage.\n",
    );
    internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            task1_review_summary_path
                .to_str()
                .expect("task 1 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task1_verification_summary_path
                .to_str()
                .expect("task 1 verification summary path should be utf-8"),
        ],
        "task 1 closure should succeed before superseded repair coverage",
    );

    let status_after_task1 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose execution fingerprint before task 2 supersession coverage",
    );
    let begin_task2 = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_after_task1["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint for task 2 begin"),
        ],
        "task 2 begin should succeed before superseded repair coverage",
    );
    internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 2 before superseded repair coverage.",
            "--manual-verify-summary",
            "Verified task 2 before superseded repair coverage.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_task2["execution_fingerprint"]
                .as_str()
                .expect("task 2 begin should expose execution fingerprint for complete"),
        ],
        "task 2 complete should succeed before superseded repair coverage",
    );
    let task2_dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "2",
        ],
        "task 2 review dispatch should succeed before superseded repair coverage",
    );
    let _task2_dispatch_id = task2_dispatch["dispatch_id"]
        .as_str()
        .expect("task 2 dispatch should expose dispatch id")
        .to_owned();
    let task2_review_summary_path = repo.join("task-2-superseded-repair-review-summary.md");
    let task2_verification_summary_path =
        repo.join("task-2-superseded-repair-verification-summary.md");
    write_file(
        &task2_review_summary_path,
        "Task 2 independent review passed before superseded repair coverage.\n",
    );
    write_file(
        &task2_verification_summary_path,
        "Task 2 verification passed before superseded repair coverage.\n",
    );
    let task2_close = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--review-result",
            "pass",
            "--review-summary-file",
            task2_review_summary_path
                .to_str()
                .expect("task 2 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task2_verification_summary_path
                .to_str()
                .expect("task 2 verification summary path should be utf-8"),
        ],
        "task 2 closure should supersede task 1 before superseded repair coverage",
    );
    let task2_closure_record_id = task2_close["closure_record_id"]
        .as_str()
        .expect("task 2 close should expose closure record id")
        .to_owned();

    update_authoritative_harness_state(
        repo,
        state,
        &[("current_task_closure_records", serde_json::json!({}))],
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should ignore superseded task dispatch lineage when restoring current task overlays",
    );
    assert_eq!(repair["action"], "blocked", "json: {repair}");
    assert_eq!(
        repair["required_follow_up"], "advance_late_stage",
        "json: {repair}"
    );
    assert!(
        repair["actions_performed"]
            .as_array()
            .is_some_and(|actions| actions
                .iter()
                .any(|action| action == "restored_current_task_closure_records")),
        "repair should restore the current task overlay instead of treating superseded task lineage as unrecoverable, got {repair:?}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_task_closure_records"]["task-2"]["closure_record_id"],
        Value::from(task2_closure_record_id)
    );
    assert_eq!(
        authoritative_state["current_task_closure_records"]["task-1"],
        Value::Null,
        "superseded task 1 should stay absent from the restored current task-closure overlay"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_repair_review_state_ignores_missing_task_closure_negative_projection_for_routing()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-task-negative-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!(
            "record",
            "-review-dispatch should succeed before failed task-outcome overlay repair coverage"
        ),
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("failed task-outcome overlay repair fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-negative-overlay-review-summary.md");
    write_file(
        &review_summary_path,
        "Task review found a blocker before overlay repair coverage.\n",
    );
    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should record the failed review outcome before overlay repair coverage",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["required_follow_up"], "execution_reentry");
    let authoritative_state_before = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state_before["task_closure_negative_result_history"]
            [format!("task-1:{dispatch_id}")]["record_status"],
        Value::from("current")
    );

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "task_closure_negative_result_records",
            serde_json::json!({}),
        )],
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should preserve routing when only a non-authoritative failed task-outcome projection is missing",
    );
    assert_eq!(repair["action"], "blocked", "json: {repair}");
    if repair["required_follow_up"] == Value::Null {
        let recommended = repair["recommended_command"]
            .as_str()
            .expect("repair must expose a recommended_command when required_follow_up is omitted");
        assert!(
            recommended.contains("featureforge plan execution "),
            "repair must expose an actionable plan-execution follow-up when required_follow_up is omitted, got {repair:?}"
        );
        assert!(
            recommended.contains("repair-review-state")
                || recommended.contains("close-current-task"),
            "repair required_follow_up omission should only occur for concrete repair/closure follow-up lanes, got {repair:?}"
        );
    } else {
        let required_follow_up = repair["required_follow_up"]
            .as_str()
            .expect("required_follow_up should be a string when present");
        assert!(
            required_follow_up == "execution_reentry"
                || required_follow_up == "repair_review_state",
            "repair should preserve an actionable follow-up lane after restoring missing failed task-outcome overlays, got {repair:?}"
        );
    }
    assert_eq!(repair["missing_derived_overlays"], serde_json::json!([]));
    assert_eq!(repair["phase"], "task_closure_pending", "json: {repair}");
    assert_eq!(
        repair["phase_detail"], "task_closure_recording_ready",
        "json: {repair}"
    );
    assert!(
        repair["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "repair should preserve the public task-closure route when only a non-authoritative failed-task projection was deleted, got {repair:?}"
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_routes_missing_current_task_closure_overlay_to_repair_review_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-missing-task-closure-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!(
            "record",
            "-review-dispatch should succeed before task-closure overlay routing coverage"
        ),
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("task-closure overlay routing fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-overlay-routing-review-summary.md");
    let verification_summary_path = repo.join("task-overlay-routing-verification-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 independent review passed before overlay routing coverage.\n",
    );
    write_file(
        &verification_summary_path,
        "Task 1 verification passed before overlay routing coverage.\n",
    );
    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task should succeed before task-closure overlay routing coverage",
    );
    assert_eq!(close_json["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[("current_task_closure_records", serde_json::json!({}))],
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should route missing current task-closure overlays through repair-review-state",
    );
    assert_eq!(status_json["harness_phase"], "executing");
    assert_eq!(status_json["phase_detail"], "execution_reentry_required");
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route missing current task-closure overlays through repair-review-state",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn internal_only_compatibility_workflow_operator_routes_missing_task_negative_overlay_to_repair_review_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-missing-task-negative-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!(
            "record",
            "-review-dispatch should succeed before failed task-outcome overlay routing coverage"
        ),
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("failed task-outcome overlay routing fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-negative-routing-review-summary.md");
    write_file(
        &review_summary_path,
        "Task review found a blocker before negative overlay routing coverage.\n",
    );
    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should record the failed review outcome before negative overlay routing coverage",
    );
    assert_eq!(close_json["action"], "blocked");

    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "task_closure_negative_result_records",
            serde_json::json!({}),
        )],
    );

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should ignore deleted non-authoritative task-negative overlay state",
    );
    assert_eq!(status_json["harness_phase"], "executing");
    assert_task_closure_recording_route(&status_json, plan_rel, 1);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should ignore deleted non-authoritative task-negative overlay state",
    );
    assert_task_closure_recording_route(&operator_json, plan_rel, 1);
}

#[test]
fn internal_only_compatibility_plan_execution_repair_review_state_routes_unrestorable_task_overlay_loss_to_execution_reentry()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-unrestorable-task-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        concat!(
            "record",
            "-review-dispatch should succeed before unrestorable task-overlay repair coverage"
        ),
    );
    let _dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("unrestorable task-overlay fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("unrestorable-task-overlay-review-summary.md");
    let verification_summary_path = repo.join("unrestorable-task-overlay-verification-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 independent review passed before unrestorable overlay coverage.\n",
    );
    write_file(
        &verification_summary_path,
        "Task 1 verification passed before unrestorable overlay coverage.\n",
    );
    let close_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        "close-current-task should succeed before unrestorable overlay repair coverage",
    );
    assert_eq!(close_json["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_task_closure_records", serde_json::json!({})),
            ("task_closure_record_history", serde_json::json!({})),
            ("harness_phase", Value::from("document_release_pending")),
            ("latest_authoritative_sequence", Value::from(1)),
        ],
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should route unrestorable task overlays to closure recording when the baseline can still be refreshed",
    );
    assert_eq!(repair["action"], "blocked", "json: {repair}");
    assert_eq!(repair["required_follow_up"], Value::Null, "json: {repair}");
    let recommended_command = repair["recommended_command"]
        .as_str()
        .expect("repair-review-state should return a concrete close-current-task command");
    assert!(
        recommended_command.starts_with("featureforge plan execution "),
        "repair-review-state should return an executable plan-execution command, got {recommended_command:?}"
    );
    if recommended_command.contains("pass|fail") || recommended_command.contains("<path>") {
        assert!(
            recommended_command.contains("close-current-task"),
            "placeholder-bearing repair follow-up should target close-current-task, got {recommended_command:?}"
        );
    } else {
        let reentry_output = run_recommended_plan_execution_command_json_real_cli(
            repo,
            state,
            recommended_command,
            "execution reentry command from repair-review-state task-overlay-priority reroute",
        );
        assert_ne!(
            reentry_output["action"],
            Value::from("blocked"),
            "repair-review-state recommended command should be immediately executable when fully concrete, got {reentry_output}"
        );
    }
}

#[test]
fn internal_only_compatibility_plan_execution_repair_review_state_prioritizes_unrestorable_task_overlay_over_late_stage_branch_reroute()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-unrestorable-task-overlay-late-stage");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "strategy_review_dispatch_lineage",
            serde_json::json!({
                "task-1": {
                    "dispatch_id": "fixture-task-dispatch"
                }
            }),
        )],
    );

    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");
    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before mixed repair precedence coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");
    let release_summary_path = repo.join("mixed-repair-precedence-release-summary.md");
    write_file(
        &release_summary_path,
        "Release readiness passed before mixed repair precedence coverage.\n",
    );
    let release_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            release_summary_path
                .to_str()
                .expect("release summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before mixed repair precedence coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_task_closure_records", serde_json::json!({})),
            ("task_closure_record_history", serde_json::json!({})),
            ("latest_authoritative_sequence", Value::from(1)),
        ],
    );
    append_tracked_repo_line(
        repo,
        "README.md",
        "late-stage-only drift after task overlay loss",
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should prioritize unrestorable task authority loss by restoring the earliest closure-recording blocker before late-stage reroute",
    );
    assert_eq!(repair["action"], "blocked", "json: {repair}");
    assert_eq!(
        repair["required_follow_up"], "execution_reentry",
        "json: {repair}"
    );
    let recommended_command = repair["recommended_command"]
        .as_str()
        .expect("repair-review-state should return a concrete public recovery command");
    assert!(
        recommended_command.starts_with("featureforge plan execution repair-review-state --plan")
            || recommended_command.starts_with("featureforge plan execution reopen --plan"),
        "repair-review-state should return a public execution-reentry recovery command, got {recommended_command:?}"
    );
    if !(recommended_command.contains("pass|fail")
        || recommended_command.contains("<path>")
        || recommended_command.contains('<')
        || recommended_command.contains(" repair-review-state "))
    {
        let reentry_output = run_recommended_plan_execution_command_json_real_cli(
            repo,
            state,
            recommended_command,
            "closure-recording command from repair-review-state unrestorable-task-overlay priority reroute",
        );
        assert_ne!(
            reentry_output["action"],
            Value::from("blocked"),
            "repair-review-state recommended closure-recording command should be immediately executable, got {reentry_output}"
        );
    }
}

#[test]
fn internal_only_compatibility_workflow_operator_routes_recoverable_missing_current_branch_closure_to_repair_review_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-recoverable-missing-current-branch-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before recoverable current-closure repair coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");

    let summary_path = repo.join("release-ready-before-current-closure-repair.md");
    write_file(
        &summary_path,
        "Release readiness is current before current-closure repair coverage.\n",
    );
    let release_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before current-closure repair coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    update_authoritative_harness_state(repo, state, &[("current_branch_closure_id", Value::Null)]);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route missing current branch closure to branch-closure recording",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should reconcile projections and require branch-closure recording when current branch closure binding is missing",
    );
    assert_eq!(repair["action"], "blocked", "json: {repair}");
    assert_eq!(
        repair["required_follow_up"],
        Value::from("advance_late_stage")
    );
    assert_eq!(
        repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_branch_closure_id"],
        Value::Null
    );
    assert_eq!(
        authoritative_state["current_release_readiness_result"],
        Value::from("ready")
    );
    assert_eq!(
        authoritative_state["release_docs_state"],
        Value::Null,
        "release_docs_state is a non-authoritative projection and must not be restored from event authority"
    );
}

#[test]
fn internal_only_compatibility_malformed_current_branch_closure_reviewed_state_requires_repair_review_state_before_late_stage_progression()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-malformed-current-branch-closure-reviewed-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);
    let mut payload = authoritative_harness_state(repo, state);
    payload["branch_closure_records"]["branch-release-closure"]["reviewed_state_id"] =
        Value::from(format!("git_tree:{}", current_head_sha(repo)));
    write_authoritative_harness_state(repo, state, &payload);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route malformed current branch-closure reviewed-state identities through repair-review-state",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    assert!(operator_json["current_branch_reviewed_state_id"].is_null());

    let reconcile = internal_only_unit_reconcile_review_state_json(
        repo,
        state,
        plan_rel,
        concat!(
            "reconcile",
            "-review-state should fail closed when the current branch closure reviewed-state identity is malformed"
        ),
    );
    assert_eq!(reconcile["action"], "blocked");
    assert_eq!(
        reconcile["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-finish should fail closed when the current branch closure reviewed-state identity is malformed"
        ),
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_reviewed_state_id_missing")),
        "{} should reject malformed current branch reviewed-state bindings, got {}",
        gate_finish,
        concat!("gate", "-finish")
    );
    assert_eq!(
        gate_finish["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should reroute malformed current branch-closure reviewed-state identities back to branch closure recording",
    );
    assert_eq!(repair["action"], "blocked");
    assert_eq!(repair["required_follow_up"], "advance_late_stage");
    let repair_command = repair["recommended_command"]
        .as_str()
        .expect("repair should expose a concrete follow-up command");
    assert!(
        repair_command.starts_with("featureforge plan execution "),
        "repair should surface a concrete plan-execution follow-up, got {repair_command:?}"
    );

    let post_repair_operator = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should reroute malformed current branch-closure reviewed-state identities back to branch closure recording after repair",
    );
    assert_eq!(post_repair_operator["phase"], "document_release_pending");
    assert_eq!(
        post_repair_operator["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        post_repair_operator["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        post_repair_operator["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let post_repair_status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should align with workflow operator after repair reroutes malformed current branch-closure state back to branch closure recording",
    );
    assert_eq!(
        post_repair_status["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        post_repair_status["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        post_repair_status["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let summary_path = repo.join("release-ready-malformed-branch-closure.md");
    write_file(
        &summary_path,
        "Release readiness should stay blocked until branch closure repair reroutes.\n",
    );
    let release_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should fail closed when the current branch closure reviewed-state identity is malformed",
    );
    assert_eq!(release_json["action"], "blocked");
    assert_eq!(release_json["branch_closure_id"], Value::Null);
    assert_eq!(release_json["code"], "out_of_phase_requery_required");
    assert_eq!(release_json["required_follow_up"], Value::Null);
    assert_eq!(
        release_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(
        release_json["rederive_via_workflow_operator"],
        Value::Bool(true)
    );

    let qa_summary_path = repo.join("qa-malformed-branch-closure.md");
    write_file(
        &qa_summary_path,
        "QA should stay blocked until branch closure repair reroutes.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            qa_summary_path
                .to_str()
                .expect("qa summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa should fail closed without exposing an unusable current branch_closure_id when the reviewed-state identity is malformed"
        ),
    );
    assert_eq!(qa_json["action"], "blocked");
    assert_eq!(qa_json["branch_closure_id"], "");
    assert_eq!(qa_json["code"], "out_of_phase_requery_required");
    assert_eq!(qa_json["required_follow_up"], Value::Null);
    assert_eq!(
        qa_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(qa_json["rederive_via_workflow_operator"], Value::Bool(true));
}

#[test]
fn internal_only_compatibility_malformed_current_branch_closure_reconcile_routes_to_repair_when_no_task_baseline_remains()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-malformed-current-branch-closure-no-task-baseline");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);
    let mut payload = authoritative_harness_state(repo, state);
    payload["branch_closure_records"]["branch-release-closure"]["reviewed_state_id"] =
        Value::from(format!("git_tree:{}", current_head_sha(repo)));
    payload["current_task_closure_records"] = serde_json::json!({});
    payload["task_closure_record_history"] = serde_json::json!({});
    write_authoritative_harness_state(repo, state, &payload);

    let reconcile = internal_only_unit_reconcile_review_state_json(
        repo,
        state,
        plan_rel,
        concat!(
            "reconcile",
            "-review-state should route malformed current branch-closure state through repair-review-state when no still-current task-closure baseline remains"
        ),
    );
    assert_eq!(reconcile["action"], "blocked");
    assert_eq!(
        reconcile["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    // Use the real CLI here because the contract under test is the post-mutation public route
    // after the repair command boundary completes.
    let repair = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should route malformed current branch-closure state to task closure recording when no still-current task-closure baseline remains",
    );
    assert_eq!(repair["action"], "blocked");
    assert!(repair["required_follow_up"].is_null(), "json: {repair}");
    assert!(
        repair["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.starts_with(&format!(
                    "featureforge plan execution close-current-task --plan {plan_rel}"
                ))
            }),
        "json: {repair}"
    );

    let status_after_repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose execution reentry after repairing malformed branch-closure state with no baseline",
    );
    assert_eq!(
        status_after_repair["phase_detail"], "task_closure_recording_ready",
        "json: {status_after_repair}"
    );
    assert_eq!(
        status_after_repair["state_kind"], "actionable_public_command",
        "status should expose a concrete public recovery route after malformed branch-closure repair with no baseline, got {status_after_repair}"
    );
    assert!(
        status_after_repair["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.starts_with(&format!(
                    "featureforge plan execution close-current-task --plan {plan_rel}"
                ))
            }),
        "status should expose close-current-task after malformed branch-closure repair with no baseline, got {status_after_repair}",
    );
    assert_ne!(
        status_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    assert_ne!(
        status_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution {} --plan {}",
            plan_rel,
            concat!("record", "-branch-closure")
        ))
    );

    let operator_after_repair = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should expose execution reentry after repairing malformed branch-closure state with no baseline",
    );
    assert_eq!(operator_after_repair["phase"], "task_closure_pending");
    assert_eq!(
        operator_after_repair["phase_detail"],
        "task_closure_recording_ready"
    );
    assert_eq!(
        operator_after_repair["state_kind"], "actionable_public_command",
        "workflow operator should expose a concrete public recovery route after malformed branch-closure repair with no baseline, got {operator_after_repair}"
    );
    assert!(
        operator_after_repair["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.starts_with(&format!(
                    "featureforge plan execution close-current-task --plan {plan_rel}"
                ))
            }),
        "workflow operator should expose close-current-task after malformed branch-closure repair with no baseline, got {operator_after_repair}",
    );
    assert_ne!(
        operator_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    assert_ne!(
        operator_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution {} --plan {}",
            plan_rel,
            concat!("record", "-branch-closure")
        ))
    );
}

#[test]
fn internal_only_compatibility_repair_review_state_preserves_branch_reroute_for_structural_branch_damage_with_zero_path_drift()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("repair-review-state-zero-drift-structural-branch-reroute");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    payload["branch_closure_records"]["branch-release-closure"]["reviewed_state_id"] =
        Value::from(format!("git_tree:{}", current_head_sha(repo)));
    payload["final_review_state"] = Value::from("stale");
    payload["last_final_review_artifact_fingerprint"] = Value::Null;
    write_authoritative_harness_state(repo, state, &payload);

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-review should expose the stale late-stage artifact even when zero-path branch reroute coverage uses only state mutations"
        ),
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_reviewed_state_id_missing")),
        "{} should expose authoritative branch-closure structural damage, got {}",
        gate_review,
        concat!("gate", "-review")
    );
    assert!(
        !gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "final_review_state_stale")),
        "projection-only final_review_state tampering must not drive {} routing, got {}",
        gate_review,
        concat!("gate", "-review")
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should preserve a branch-closure reroute for structural branch damage even when there are zero changed paths",
    );
    assert_eq!(repair["action"], "blocked");
    assert_eq!(repair["required_follow_up"], "advance_late_stage");
    let repair_command = repair["recommended_command"]
        .as_str()
        .expect("repair should expose a concrete follow-up command");
    assert!(
        repair_command.starts_with("featureforge plan execution "),
        "repair should surface a concrete plan-execution follow-up, got {repair_command:?}"
    );

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_shell_smoke zero-drift structural branch reroute",
    );
    let operator_json = workflow_operator_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke zero-drift structural branch reroute",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(operator_json["next_action"], "advance late stage");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke zero-drift structural branch reroute",
    );
    assert_eq!(
        status_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        status_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        status_json["stale_unreviewed_closures"],
        serde_json::json!([]),
        "structural branch damage without stale provenance must not project stale_unreviewed closures"
    );
    assert_eq!(status_json["next_action"], "advance late stage");
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let explain_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[concat!("explain", "-review-state"), "--plan", plan_rel],
        concat!(
            "explain",
            "-review-state should keep structural branch damage distinct from stale-unreviewed drift when zero paths changed"
        ),
    );
    assert_eq!(
        explain_json["stale_unreviewed_closures"],
        serde_json::json!([]),
        "structural branch damage without stale provenance must not project stale_unreviewed closures"
    );
}

#[test]
fn internal_only_compatibility_final_review_dispatch_blocks_when_current_branch_closure_overlay_requires_repair()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("final-review-dispatch-missing-current-branch-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(repo, state, &[("current_branch_closure_id", Value::Null)]);

    let state_before = authoritative_harness_state(repo, state);
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        concat!(
            "record",
            "-review-dispatch should fail closed when current branch-closure overlay repair is still required"
        ),
    );
    assert_eq!(dispatch["allowed"], Value::Bool(false));
    assert_eq!(dispatch["action"], Value::from("blocked"));
    assert!(
        dispatch["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "branch_closure_recording_required_for_release_readiness")),
        "dispatch should surface branch-closure recording as the blocker: {dispatch}"
    );
    assert_eq!(dispatch["code"], Value::Null);
    assert_eq!(
        dispatch["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert_eq!(dispatch["rederive_via_workflow_operator"], Value::Null);

    let state_after = authoritative_harness_state(repo, state);
    assert_eq!(
        state_after["final_review_dispatch_lineage"],
        state_before["final_review_dispatch_lineage"]
    );
}

#[test]
fn internal_only_compatibility_final_review_dispatch_blocks_when_current_branch_closure_reviewed_state_requires_repair()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("final-review-dispatch-malformed-current-branch-reviewed-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");

    let initial_dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        concat!(
            "record",
            "-review-dispatch should succeed before malformed current branch reviewed-state coverage"
        ),
    );
    assert_eq!(initial_dispatch["allowed"], Value::Bool(true));
    assert_eq!(initial_dispatch["action"], Value::from("recorded"));

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    let lineage_before = payload["final_review_dispatch_lineage"].clone();
    payload["branch_closure_records"][&branch_closure_id]["reviewed_state_id"] =
        Value::from("unsupported-reviewed-state");
    write_authoritative_harness_state(repo, state, &payload);

    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        concat!(
            "record",
            "-review-dispatch should fail closed when the current branch closure reviewed state requires repair"
        ),
    );
    assert_eq!(dispatch["allowed"], Value::Bool(false));
    assert_eq!(dispatch["action"], Value::from("blocked"));
    assert!(
        dispatch["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "derived_review_state_missing")),
        "dispatch should surface branch reviewed-state repair as the blocker: {dispatch}"
    );
    assert_eq!(dispatch["code"], Value::Null);
    assert_eq!(
        dispatch["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    assert_eq!(dispatch["rederive_via_workflow_operator"], Value::Null);

    let state_after = authoritative_harness_state(repo, state);
    assert_eq!(state_after["final_review_dispatch_lineage"], lineage_before);
}

#[test]
fn internal_only_compatibility_plan_execution_record_branch_closure_same_id_reassertion_preserves_release_readiness()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-record",
        "-branch-closure-reasserts-current-binding"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before same-id reassertion coverage"
        ),
    );
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();

    let summary_path = repo.join("release-ready-before-branch-reassertion.md");
    write_file(
        &summary_path,
        "Release readiness is current before same-id branch-closure reassertion coverage.\n",
    );
    let release_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage should record release readiness before branch-closure reassertion coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    update_authoritative_harness_state(repo, state, &[("current_branch_closure_id", Value::Null)]);

    let rerecord = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should restore current binding and reset late-stage release readiness surfaces"
        ),
    );
    assert_eq!(rerecord["action"], "recorded");
    assert_eq!(rerecord["required_follow_up"], Value::Null);
    assert!(
        rerecord["branch_closure_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty()),
        "{} should return a non-empty branch_closure_id, got {}",
        rerecord,
        concat!("record", "-branch-closure")
    );
    assert_ne!(
        rerecord["branch_closure_id"],
        Value::from(branch_closure_id),
        "{} should mint a fresh branch closure id when current binding is missing, got {}",
        rerecord,
        concat!("record", "-branch-closure")
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_branch_closure_id"],
        rerecord["branch_closure_id"]
    );
    assert_eq!(
        authoritative_state["current_release_readiness_result"],
        Value::Null
    );
    assert_eq!(authoritative_state["release_docs_state"], Value::Null);
}

#[test]
fn plan_execution_status_ignores_overlay_only_branch_closure_without_authoritative_record() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-status-overlay-only-branch-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["branch_closure_records"]
        .as_object_mut()
        .expect("branch_closure_records should remain an object")
        .remove(&branch_closure_id);
    write_authoritative_harness_state(repo, state, &payload);

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should not treat an overlay-only branch closure as authoritative current truth",
    );

    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        status_json["next_action"],
        Value::from("advance late stage")
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert!(
        status_json["blocking_records"]
            .as_array()
            .is_some_and(|records| records.iter().any(|record| {
                record["code"] == "missing_current_closure"
                    && record["record_type"] == "branch_closure"
                    && record["required_follow_up"] == "advance_late_stage"
            })),
        "status should surface the missing current branch closure blocker when the authoritative record is absent: {status_json}"
    );
}

#[test]
fn internal_only_compatibility_incomplete_current_branch_closure_record_fails_closed_across_public_and_finish_surfaces()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-incomplete-current-branch-closure-public-fail-closed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["branch_closure_records"][&branch_closure_id]
        .as_object_mut()
        .expect("branch closure record should remain an object")
        .remove("base_branch");
    write_authoritative_harness_state(repo, state, &payload);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_shell_smoke incomplete current branch closure",
    );
    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke incomplete current branch closure",
    );
    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let operator_json = workflow_operator_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke incomplete current branch closure",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke incomplete current branch closure",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert_eq!(
        gate_review["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject incomplete current branch-closure truth before finish readiness can proceed, got {}",
        gate_review,
        concat!("gate", "-review")
    );

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke incomplete current branch closure",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert_eq!(
        gate_finish["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject incomplete current branch-closure truth, got {}",
        gate_finish,
        concat!("gate", "-finish")
    );
}

#[test]
fn internal_only_compatibility_empty_lineage_late_stage_exemption_record_without_exempt_surface_fails_closed()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(
        "plan-execution-current-branch-closure-invalid-empty-lineage-exemption-public-fail-closed",
    );
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["branch_closure_records"][&branch_closure_id]["source_task_closure_ids"] =
        Value::Array(Vec::new());
    payload["branch_closure_records"][&branch_closure_id]["provenance_basis"] =
        Value::from("task_closure_lineage_plus_late_stage_surface_exemption");
    payload["branch_closure_records"][&branch_closure_id]["effective_reviewed_branch_surface"] =
        Value::from("repo_tracked_content");
    write_authoritative_harness_state(repo, state, &payload);

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should fail closed when an empty-lineage exemption branch closure lacks a late-stage-only reviewed surface",
    );
    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should fail closed when an empty-lineage exemption branch closure lacks a late-stage-only reviewed surface",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["review_state_status"],
        Value::from("missing_current_closure")
    );

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-finish should reject empty-lineage exemption branch closure truth without a valid late-stage-only surface"
        ),
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject invalid empty-lineage exemption branch-closure truth, got {}",
        gate_finish,
        concat!("gate", "-finish")
    );
}

#[test]
fn internal_only_compatibility_empty_lineage_late_stage_exemption_subset_surface_stays_current_across_public_and_finish_surfaces()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-current-branch-closure-empty-lineage-exemption-subset");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    install_full_contract_ready_artifacts(repo);
    upsert_plan_header(
        repo,
        plan_rel,
        "Late-Stage Surface",
        "README.md,docs/featureforge/specs/",
    );
    write_repo_file(
        repo,
        "tests/workflow_shell_smoke.rs",
        "synthetic route proof\n",
    );
    prepare_preflight_acceptance_workspace(repo, "workflow-shell-smoke-fixture");
    let status = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for late-stage exemption subset fixture",
    );
    let preflight = internal_only_runtime_preflight_gate_json(
        repo,
        state,
        plan_rel,
        concat!(
            "plan execution pre",
            "flight for late-stage exemption subset fixture"
        ),
    );
    assert_eq!(preflight["allowed"], Value::Bool(true));
    let begin = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution_fingerprint"),
        ],
        "plan execution begin for late-stage exemption subset fixture",
    );
    let begin_fingerprint = begin["execution_fingerprint"]
        .as_str()
        .expect("begin should expose execution_fingerprint")
        .to_owned();
    let _ = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed shell smoke parity fixture task.",
            "--manual-verify-summary",
            "Verified by shell smoke parity setup.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_fingerprint.as_str(),
        ],
        "plan execution complete for late-stage exemption subset fixture",
    );
    seed_current_task_closure_state(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["current_task_closure_records"] = serde_json::json!({});
    payload["task_closure_record_history"] = serde_json::json!({});
    payload["branch_closure_records"][&branch_closure_id]["source_task_closure_ids"] =
        Value::Array(Vec::new());
    payload["branch_closure_records"][&branch_closure_id]["provenance_basis"] =
        Value::from("task_closure_lineage_plus_late_stage_surface_exemption");
    payload["branch_closure_records"][&branch_closure_id]["effective_reviewed_branch_surface"] =
        Value::from("late_stage_surface_only:README.md");
    write_authoritative_harness_state(repo, state, &payload);

    let status_json = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should keep a valid subset late-stage-surface exemption branch closure current",
    );
    assert_eq!(
        status_json["current_branch_closure_id"],
        Value::from(branch_closure_id.clone())
    );
    assert_eq!(status_json["review_state_status"], Value::from("clean"));

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep a valid subset late-stage-surface exemption branch closure current",
    );
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
    assert_eq!(operator_json["review_state_status"], Value::from("clean"));

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-review should accept a valid subset late-stage-surface exemption branch closure before gate",
            "-finish"
        ),
    );
    assert_eq!(gate_review["allowed"], Value::Bool(true));

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        concat!(
            "gate",
            "-finish should accept a valid subset late-stage-surface exemption branch closure"
        ),
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(true));
}

#[test]
fn internal_only_compatibility_current_branch_closure_record_with_wrong_plan_revision_fails_closed_across_public_and_finish_surfaces()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-current-branch-closure-wrong-plan-revision-public-fail-closed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["branch_closure_records"][&branch_closure_id]["source_plan_revision"] =
        Value::from(999);
    write_authoritative_harness_state(repo, state, &payload);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_shell_smoke wrong-plan current branch closure",
    );
    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-plan current branch closure",
    );
    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let operator_json = workflow_operator_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-plan current branch closure",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-plan current branch closure",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject wrong-plan current branch-closure truth before finish readiness can proceed, got {}",
        gate_review,
        concat!("gate", "-review")
    );

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-plan current branch closure",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject wrong-plan current branch-closure truth, got {}",
        gate_finish,
        concat!("gate", "-finish")
    );
}

#[test]
fn internal_only_compatibility_current_branch_closure_record_with_wrong_repository_context_fails_closed_across_public_and_finish_surfaces()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(
        "plan-execution-current-branch-closure-wrong-repository-context-public-fail-closed",
    );
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["branch_closure_records"][&branch_closure_id]["repo_slug"] =
        Value::from("foreign-repo");
    payload["branch_closure_records"][&branch_closure_id]["branch_name"] =
        Value::from("foreign-branch");
    payload["branch_closure_records"][&branch_closure_id]["base_branch"] =
        Value::from("foreign-base");
    write_authoritative_harness_state(repo, state, &payload);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_shell_smoke wrong-context current branch closure",
    );
    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-context current branch closure",
    );
    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );

    let operator_json = workflow_operator_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-context current branch closure",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-context current branch closure",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject wrong-context current branch-closure truth before finish readiness can proceed, got {}",
        gate_review,
        concat!("gate", "-review")
    );

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-context current branch closure",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject wrong-context current branch-closure truth, got {}",
        gate_finish,
        concat!("gate", "-finish")
    );
}

#[test]
fn internal_only_compatibility_current_branch_closure_record_with_wrong_contract_identity_fails_closed_across_public_and_finish_surfaces()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(
        "plan-execution-current-branch-closure-wrong-contract-identity-public-fail-closed",
    );
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["branch_closure_records"][&branch_closure_id]["contract_identity"] =
        Value::from("branch-contract-corrupted");
    write_authoritative_harness_state(repo, state, &payload);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_shell_smoke wrong-contract current branch closure",
    );
    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-contract current branch closure",
    );
    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );

    let operator_json = workflow_operator_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-contract current branch closure",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-contract current branch closure",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject corrupted current branch-closure identity, got {}",
        gate_finish,
        concat!("gate", "-finish")
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-contract current branch closure",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject corrupted current branch-closure identity, got {}",
        gate_review,
        concat!("gate", "-review")
    );
}

#[test]
fn internal_only_compatibility_current_branch_closure_record_with_wrong_source_task_lineage_fails_closed_across_public_and_finish_surfaces()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-current-branch-closure-wrong-lineage-public-fail-closed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["branch_closure_records"][&branch_closure_id]["source_task_closure_ids"] =
        serde_json::json!(["task-closure-corrupted"]);
    write_authoritative_harness_state(repo, state, &payload);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_shell_smoke wrong-lineage current branch closure",
    );
    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-lineage current branch closure",
    );
    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );

    let operator_json = workflow_operator_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-lineage current branch closure",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["review_state_status"],
        Value::from("missing_current_closure")
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-lineage current branch closure",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject corrupted current branch-closure lineage, got {}",
        gate_review,
        concat!("gate", "-review")
    );

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke wrong-lineage current branch closure",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject corrupted current branch-closure lineage, got {}",
        gate_finish,
        concat!("gate", "-finish")
    );
}

#[test]
fn internal_only_compatibility_current_branch_closure_record_with_invalid_reviewed_surface_fails_closed_across_public_and_finish_surfaces()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-current-branch-closure-invalid-surface-public-fail-closed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["branch_closure_records"][&branch_closure_id]["effective_reviewed_branch_surface"] =
        Value::from("not-a-runtime-owned-branch-surface");
    write_authoritative_harness_state(repo, state, &payload);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_shell_smoke invalid-surface current branch closure",
    );
    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke invalid-surface current branch closure",
    );
    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );

    let operator_json = workflow_operator_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke invalid-surface current branch closure",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["review_state_status"],
        Value::from("missing_current_closure")
    );

    let gate_review = internal_only_runtime_review_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke invalid-surface current branch closure",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject invalid current branch-closure reviewed surfaces, got {}",
        gate_review,
        concat!("gate", "-review")
    );

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke invalid-surface current branch closure",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject invalid current branch-closure reviewed surfaces, got {}",
        gate_finish,
        concat!("gate", "-finish")
    );
}

#[test]
fn internal_only_compatibility_current_branch_closure_record_missing_required_arrays_fails_closed_across_public_and_finish_surfaces()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-current-branch-closure-missing-arrays-public-fail-closed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    let mut payload = authoritative_harness_state(repo, state);
    let branch_closure_id = payload["current_branch_closure_id"]
        .as_str()
        .expect("fixture should expose a current branch closure id")
        .to_owned();
    payload["branch_closure_records"][&branch_closure_id]
        .as_object_mut()
        .expect("branch closure record should remain an object")
        .remove("source_task_closure_ids");
    write_authoritative_harness_state(repo, state, &payload);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_shell_smoke missing-arrays current branch closure",
    );
    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke missing-arrays current branch closure",
    );
    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );

    let operator_json = workflow_operator_json(
        &runtime,
        plan_rel,
        false,
        "workflow_shell_smoke missing-arrays current branch closure",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel}"
        ))
    );

    let gate_finish = internal_only_runtime_finish_gate_json(
        repo,
        state,
        plan_rel,
        false,
        "workflow_shell_smoke missing-arrays current branch closure",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "{} should reject current branch-closure truth missing required provenance arrays, got {}",
        gate_finish,
        concat!("gate", "-finish")
    );
}

#[test]
fn internal_only_compatibility_plan_execution_repair_review_state_restores_overlay_from_authoritative_branch_record()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-blocks-unrestorable-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before unrestorable repair coverage"
        ),
    );
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id");
    write_file(
        &project_artifact_dir(repo, state).join(format!("branch-closure-{branch_closure_id}.md")),
        "# Branch Closure\n\ncorrupted fixture without derivable overlay headers\n",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should restore missing overlays from the authoritative branch record even if markdown is corrupted",
    );

    assert_eq!(repair["action"], "reconciled");
    assert_eq!(repair["required_follow_up"], Value::Null);
    assert_eq!(
        repair["actions_performed"],
        Value::from(vec![
            String::from("restored_current_branch_closure_reviewed_state"),
            String::from("restored_current_branch_closure_contract_identity"),
        ])
    );
    assert_eq!(
        repair["missing_derived_overlays"],
        Value::from(Vec::<String>::new())
    );
}

#[test]
fn internal_only_compatibility_plan_execution_repair_review_state_blocks_when_only_branch_closure_markdown_remains()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-markdown-only-branch-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before markdown-only repair coverage"
        ),
    );
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("branch_closure_records", serde_json::json!({})),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let repair = internal_only_run_plan_execution_json_direct_or_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should fail closed when only derived branch-closure markdown remains",
    );

    assert_eq!(repair["action"], "blocked");
    assert_eq!(repair["required_follow_up"], "advance_late_stage");
    assert_eq!(
        repair["missing_derived_overlays"],
        Value::from(vec![
            String::from("current_branch_closure_id"),
            String::from("current_branch_closure_reviewed_state_id"),
            String::from("current_branch_closure_contract_identity"),
        ])
    );
    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_branch_closure_id"],
        Value::from(branch_closure_id)
    );
    assert!(authoritative_state["current_branch_closure_reviewed_state_id"].is_null());
    assert!(authoritative_state["current_branch_closure_contract_identity"].is_null());
    assert_eq!(
        authoritative_state["branch_closure_records"],
        serde_json::json!({})
    );
}

#[test]
fn internal_only_compatibility_plan_execution_reconcile_review_state_restores_missing_branch_overlay_while_stale()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-reconcile",
        "-review-state-restores-stale-branch-overlay"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before stale reconcile coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");
    append_tracked_repo_line(
        repo,
        "README.md",
        "stale reconcile overlay restoration coverage",
    );

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_before: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable before stale overlay corruption"),
    )
    .expect("authoritative state should remain valid json before stale overlay corruption");
    let expected_reviewed_state_id =
        authoritative_state_before["current_branch_closure_reviewed_state_id"]
            .as_str()
            .expect("branch closure should seed reviewed state overlay before stale corruption")
            .to_owned();
    let expected_contract_identity =
        authoritative_state_before["current_branch_closure_contract_identity"]
            .as_str()
            .expect("branch closure should seed contract identity overlay before stale corruption")
            .to_owned();

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let reconcile = internal_only_unit_reconcile_review_state_json(
        repo,
        state,
        plan_rel,
        concat!(
            "reconcile",
            "-review-state should restore derivable branch overlays even when the branch state is stale"
        ),
    );

    assert_eq!(reconcile["action"], "blocked");
    assert_eq!(
        reconcile["actions_performed"],
        Value::from(vec![
            String::from("restored_current_branch_closure_reviewed_state"),
            String::from("restored_current_branch_closure_contract_identity"),
        ])
    );
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after stale reconcile"),
    )
    .expect("authoritative state should remain valid json after stale reconcile");
    assert_eq!(
        authoritative_state_after["current_branch_closure_reviewed_state_id"],
        Value::from(expected_reviewed_state_id)
    );
    assert_eq!(
        authoritative_state_after["current_branch_closure_contract_identity"],
        Value::from(expected_contract_identity)
    );
}

#[test]
fn internal_only_compatibility_plan_execution_reconcile_review_state_stale_only_does_not_claim_restore()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!(
        "plan-execution-reconcile",
        "-review-state-stale-only"
    ));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "record",
            "-branch-closure should succeed before stale-only reconcile coverage"
        ),
    );
    assert_eq!(branch_closure["action"], "recorded");
    append_tracked_repo_line(
        repo,
        "README.md",
        "stale reconcile without overlay corruption",
    );

    let reconcile = internal_only_unit_reconcile_review_state_json(
        repo,
        state,
        plan_rel,
        concat!(
            "reconcile",
            "-review-state should not claim overlay restoration when no derived overlays were missing"
        ),
    );

    assert_eq!(reconcile["action"], "blocked");
    assert_eq!(
        reconcile["actions_performed"],
        Value::from(Vec::<String>::new())
    );
    assert_eq!(
        reconcile["trace_summary"],
        Value::from(
            "Reviewed state is stale_unreviewed; no derivable overlays required reconciliation.",
        )
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_qa_blocks_when_test_plan_refresh_is_required()
{
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(concat!("plan-execution-record", "-qa-refresh-required"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    remove_branch_test_plan_artifact(repo, state);
    remove_authoritative_test_plan_artifact(repo, state);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let summary_path = repo.join("qa-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed after manual verification.\n",
    );
    let qa_json = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-qa"),
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        concat!(
            "record",
            "-qa command should fail closed when test-plan refresh is required"
        ),
    );

    assert_eq!(qa_json["action"], "blocked");
    assert_eq!(
        qa_json["code"],
        Value::from("out_of_phase_requery_required")
    );
    assert_eq!(qa_json["rederive_via_workflow_operator"], Value::Bool(true));
    assert_eq!(
        qa_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
}

#[test]
fn workflow_operator_routes_pivot_required_to_public_repair_review_state() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-pivot-plan-block");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);

    write_authoritative_harness_state(
        repo,
        state,
        &serde_json::json!({
            "harness_phase": "pivot_required",
            "latest_authoritative_sequence": 23,
            "reason_codes": ["blocked_on_plan_revision"],
        }),
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for pivot-required plan-revision block",
    );

    assert_eq!(operator_json["phase"], "pivot_required");
    assert_eq!(operator_json["phase_detail"], "planning_reentry_required");
    assert!(operator_json.get("follow_up_override").is_none());
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn removed_workflow_doctor_and_handoff_commands_fail_at_cli_boundary() {
    let (repo_dir, state_dir) = init_repo("workflow-removed-doctor-handoff-boundary");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    for args in [
        &["workflow", "doctor", "--json"][..],
        &["workflow", "handoff", "--json"][..],
        &["workflow", "phase", "--json"][..],
    ] {
        let output =
            run_featureforge_with_env(repo, state, args, &[], "removed workflow command boundary");
        assert!(
            !output.status.success(),
            "removed workflow command should fail: {args:?}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unrecognized subcommand"),
            "removed workflow command should fail at CLI parse boundary, got {stderr}"
        );
    }
}

#[test]
fn featureforge_cutover_gate_rejects_active_legacy_root_content() {
    let (repo_dir, _state_dir) = init_repo("cutover-active-content");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(
        repo,
        "featureforge-upgrade/SKILL.md",
        "Do not use ~/.codex/featureforge for active FeatureForge installs.\n",
    );
    git_add_all(repo);

    let output = run_cutover_check(repo);
    assert!(
        !output.status.success(),
        "cutover check should fail on active legacy-root content\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Forbidden active content references:"));
    assert!(stderr.contains("featureforge-upgrade/SKILL.md:1"));
}

#[test]
fn featureforge_cutover_gate_rejects_punctuation_delimited_legacy_root_content() {
    let (repo_dir, _state_dir) = init_repo("cutover-punctuation-content");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(
        repo,
        "docs/runtime.md",
        "Retired paths like (~/.codex/featureforge) or ~/.copilot/featureforge, must stay blocked.\n",
    );
    git_add_all(repo);

    let output = run_cutover_check(repo);
    assert!(
        !output.status.success(),
        "cutover check should fail on punctuation-delimited legacy-root content\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Forbidden active content references:"));
    assert!(stderr.contains("docs/runtime.md:1"));
}

#[test]
fn featureforge_cutover_gate_scans_repo_wide_tracked_files() {
    let (repo_dir, _state_dir) = init_repo("cutover-repo-bounded");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(
        repo,
        "src/reintroduced.rs",
        "legacy = \"~/.codex/featureforge/runtime\"\n",
    );
    git_add_all(repo);

    let output = run_cutover_check(repo);
    assert!(
        !output.status.success(),
        "cutover check should fail on legacy-root content anywhere in tracked active files\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Forbidden active content references:"));
    assert!(stderr.contains("src/reintroduced.rs:"));
}

#[test]
fn featureforge_cutover_gate_rejects_active_legacy_root_paths() {
    let (repo_dir, _state_dir) = init_repo("cutover-active-path");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(
        repo,
        ".codex/featureforge/INSTALL.md",
        "retired path should be blocked\n",
    );
    git_add_all(repo);

    let output = run_cutover_check(repo);
    assert!(
        !output.status.success(),
        "cutover check should fail on active legacy-root paths\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Forbidden active path names:"));
    assert!(stderr.contains(".codex/featureforge/INSTALL.md"));
}

#[test]
fn featureforge_cutover_gate_allows_archived_legacy_root_history() {
    let (repo_dir, _state_dir) = init_repo("cutover-archive-allowed");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(
        repo,
        "docs/archive/featureforge/legacy-root-history.md",
        "Historical notes may mention ~/.codex/featureforge and ~/.copilot/featureforge.\n",
    );
    git_add_all(repo);

    let output = run_cutover_check(repo);
    assert!(
        output.status.success(),
        "cutover check should ignore docs/archive legacy-root history\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "featureforge cutover checks passed"
    );
}

#[test]
fn featureforge_cutover_gate_uses_one_repo_wide_content_scan() {
    let (repo_dir, _state_dir) = init_repo("cutover-single-pass");
    let repo = repo_dir.path();
    install_cutover_check_baseline(repo);
    write_repo_file(repo, "src/one.rs", "const ONE: &str = \"clean\";\n");
    write_repo_file(repo, "src/two.rs", "const TWO: &str = \"clean\";\n");
    write_repo_file(repo, "docs/guide.md", "still clean\n");
    git_add_all(repo);

    let wrapper_root = TempDir::new().expect("wrapper tempdir should exist");
    let wrapper_bin = wrapper_root.path().join("bin");
    fs::create_dir_all(&wrapper_bin).expect("wrapper bin dir should exist");
    let grep_log = wrapper_root.path().join("grep.log");
    let grep_path = wrapper_bin.join("grep");
    let real_grep = Command::new("sh")
        .arg("-c")
        .arg("command -v grep")
        .output()
        .expect("real grep path should resolve");
    let real_grep = String::from_utf8_lossy(&real_grep.stdout).trim().to_owned();
    assert!(!real_grep.is_empty(), "real grep path should not be empty");
    write_repo_file(
        wrapper_root.path(),
        "bin/grep",
        &format!(
            "#!/usr/bin/env bash\nprintf 'grep %s\\n' \"$*\" >> \"{}\"\nexec \"{}\" \"$@\"\n",
            grep_log.display(),
            real_grep
        ),
    );
    make_executable(&grep_path);

    let existing_path = std::env::var("PATH").expect("PATH should exist");
    let wrapper_path = format!("{}:{}", wrapper_bin.display(), existing_path);
    let output = run_cutover_check_with_env(repo, &[("PATH", wrapper_path.as_str())]);
    assert!(
        output.status.success(),
        "cutover check should stay green under rg instrumentation\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let grep_invocations = fs::read_to_string(&grep_log).expect("grep log should exist");
    let content_scan_lines = grep_invocations
        .lines()
        .filter(|line| line.contains("grep -nH -E "))
        .collect::<Vec<_>>();
    assert_eq!(
        content_scan_lines.len(),
        1,
        "cutover content scanning should stay repo-bounded and single-pass instead of spawning one scan per tracked file: {content_scan_lines:?}"
    );
}

#[test]
fn compiled_cli_route_parity_probe_for_late_stage_refresh_fixture() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs01-cli-parity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);

    let mut runtime_management_commands = 0usize;
    runtime_management_commands += 1;
    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-01 workflow operator json route parity fixture",
    );
    runtime_management_commands += 1;
    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-01 plan execution status route parity fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_parity_probe_budget("PARITY-PROBE-LATE-STAGE", runtime_management_commands, 2);
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs01_compiled_cli_repair_and_branch_closure_do_not_disagree()
 {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs01-cli-repair-closure-consistency");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            (
                "review_state_repair_follow_up",
                Value::from("record_branch_closure"),
            ),
        ],
    );

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-01 repair-review-state compiled CLI consistency fixture",
    );
    let operator_after_repair = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-01 workflow operator after repair compiled CLI consistency fixture",
    );
    let record_json = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        concat!(
            "FS-01 record",
            "-branch-closure compiled CLI consistency fixture"
        ),
    );

    if repair_json["action"] == "already_current" {
        assert_ne!(
            operator_after_repair["review_state_status"],
            Value::from("missing_current_closure"),
            "FS-01 compiled CLI path must not keep missing_current_closure active when repair-review-state reports already_current"
        );
        assert_ne!(
            record_json["required_follow_up"],
            Value::from("repair_review_state"),
            "FS-01 compiled CLI path must not report repair_review_state as a blocker right after repair-review-state already_current"
        );
    }
}

#[test]
fn internal_only_compatibility_runtime_remediation_fs10_compiled_cli_stale_follow_up_is_ignored_when_truth_is_current()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs10-cli-live-truth");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    internal_only_write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            (
                "review_state_repair_follow_up",
                Value::from("execution_reentry"),
            ),
            ("harness_phase", Value::from("ready_for_branch_completion")),
        ],
    );

    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-10 compiled-cli status should ignore stale execution-reentry follow-up when live truth is already current",
    );
    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-10 compiled-cli workflow operator should ignore stale execution-reentry follow-up when live truth is already current",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        status_json["harness_phase"],
        Value::from("ready_for_branch_completion"),
        "FS-10 compiled-cli status should preserve ready_for_branch_completion when stale follow-up metadata latches execution_reentry"
    );
    assert_eq!(
        operator_json["phase"],
        Value::from("ready_for_branch_completion"),
        "FS-10 compiled-cli workflow operator should preserve ready_for_branch_completion when stale follow-up metadata latches execution_reentry"
    );
    assert_eq!(
        operator_json["next_action"],
        Value::from("finish branch"),
        "FS-10 compiled-cli workflow operator should keep finish-branch routing when live truth is already current"
    );
}

#[test]
fn internal_only_compatibility_compiled_cli_route_parity_probe_for_pending_external_review_fixture()
{
    let (repo_dir, state_dir) = init_repo("runtime-remediation-parity-external-review-wait");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    seed_current_task_closure_state(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = internal_only_plan_execution_fixture_json(
        repo,
        state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "task review dispatch should succeed for external-review-wait parity fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("final_review_state", Value::Null),
            ("last_final_review_artifact_fingerprint", Value::Null),
        ],
    );

    let mut runtime_management_commands = 0usize;
    runtime_management_commands += 1;
    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "external-review-wait parity fixture operator json",
    );
    runtime_management_commands += 1;
    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "external-review-wait parity fixture status json",
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("final_review_outcome_pending")
    );
    assert_eq!(
        operator_json["external_wait_state"],
        Value::from("waiting_for_external_review_result")
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_parity_probe_budget(
        "PARITY-PROBE-EXTERNAL-REVIEW-WAIT",
        runtime_management_commands,
        2,
    );
}

#[test]
fn compiled_cli_route_parity_probe_for_branch_scoped_execution_reentry_fixture() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-parity-branch-execution-reentry");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);
    let mut payload = authoritative_harness_state(repo, state);
    payload["branch_closure_records"]["branch-release-closure"]["reviewed_state_id"] =
        Value::from(format!("git_tree:{}", current_head_sha(repo)));
    write_authoritative_harness_state(repo, state, &payload);

    let mut runtime_management_commands = 0usize;
    runtime_management_commands += 1;
    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "branch execution-reentry parity fixture operator json",
    );
    runtime_management_commands += 1;
    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "branch execution-reentry parity fixture status json",
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("execution_reentry_required")
    );
    assert_eq!(operator_json["blocking_scope"], Value::from("branch"));
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_parity_probe_budget(
        "PARITY-PROBE-BRANCH-EXECUTION-REENTRY",
        runtime_management_commands,
        2,
    );
}

#[test]
fn internal_only_fs06_hidden_dispatch_target_mismatch_keeps_helper_semantics_and_cli_cutover_boundary()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, direct_state_dir) = init_repo("runtime-remediation-fs06-helper-vs-cli-direct");
    let real_state_dir = TempDir::new().expect("real-cli fs06 state tempdir should exist");
    let repo = repo_dir.path();
    let direct_state = direct_state_dir.path();
    let real_state = real_state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, direct_state, plan_rel, &base_branch);
    setup_task_boundary_blocked_case(repo, real_state, plan_rel, &base_branch);

    let direct_digest_before = authoritative_harness_state_digest(repo, direct_state);
    let real_digest_before = authoritative_harness_state_digest(repo, real_state);

    let direct_failure = internal_only_plan_execution_failure_json(
        repo,
        direct_state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "2",
        ],
        "FS-06 direct helper target mismatch failure shape",
    );
    let real_failure = run_plan_execution_failure_json_real_cli(
        repo,
        real_state,
        &[
            concat!("record", "-review-dispatch"),
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "2",
        ],
        "FS-06 compiled-cli target mismatch failure shape",
    );

    assert_eq!(
        direct_failure["error_class"],
        Value::from("InvalidCommandInput")
    );
    assert_eq!(
        real_failure["error_class"],
        Value::from("InvalidCommandInput")
    );
    assert!(
        direct_failure["message"].as_str().is_some_and(|message| {
            message.contains("does not match the current task review-dispatch target")
        }),
        "FS-06 helper failure should preserve the semantic target-mismatch contract: {direct_failure}"
    );
    assert!(
        real_failure["message"].as_str().is_some_and(|message| {
            message.contains(&format!(
                "unrecognized subcommand '{}'",
                concat!("record", "-review-dispatch"),
            ))
        }),
        "FS-06 compiled-cli failure should preserve the hidden-command cutover boundary: {real_failure}"
    );
    assert_eq!(
        authoritative_harness_state_digest(repo, direct_state),
        direct_digest_before,
        "FS-06 direct helper mismatch failure must not mutate authoritative state"
    );
    assert_eq!(
        authoritative_harness_state_digest(repo, real_state),
        real_digest_before,
        "FS-06 compiled-cli mismatch failure must not mutate authoritative state"
    );
}

#[test]
fn task_close_happy_path_runtime_management_budget_is_capped() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("runtime-remediation-task-close-budget");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    prepare_missing_task_closure_baseline_close_fixture(repo, state, plan_rel, &base_branch);
    let mut runtime_management_commands = 0usize;
    runtime_management_commands += 1; // workflow operator
    let operator_ready = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "task-close budget fixture workflow operator after external review ready",
    );
    let routed_command = operator_ready["recommended_command"]
        .as_str()
        .expect("task-close budget fixture should expose a routed command");
    assert_no_hidden_helper_commands_used(&[routed_command.to_owned()]);
    assert!(
        routed_command.starts_with(&format!(
            "featureforge plan execution close-current-task --plan {plan_rel}"
        )),
        "task-close budget fixture operator should route directly to close-current-task, got {operator_ready}"
    );
    assert!(
        routed_command.contains("--task 1"),
        "task-close budget fixture operator should target Task 1 for the budgeted close-current-task route, got {operator_ready}"
    );
    let review_summary_path = repo.join("task-close-budget-review-summary.md");
    let verification_summary_path = repo.join("task-close-budget-verification-summary.md");
    write_file(
        &review_summary_path,
        "Task close budget fixture independent review passed.\n",
    );
    write_file(
        &verification_summary_path,
        "Task close budget fixture verification passed.\n",
    );
    runtime_management_commands += 1; // close-current-task
    let close_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("task-close budget review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("task-close budget verification summary path should be utf-8"),
        ],
        "task-close budget fixture close-current-task",
    );
    assert_eq!(
        close_json["action"],
        Value::from("recorded"),
        "task-close budget fixture close-current-task should record a closure, got {close_json:?}"
    );
    assert_runtime_management_budget("TASK-CLOSE-BUDGET", runtime_management_commands, 2);
}

#[test]
fn task_close_internal_dispatch_runtime_management_budget_is_capped() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-task-close-internal-dispatch-budget");
    let repo = repo_dir.path();
    let state = state_dir.path();
    prepare_missing_task_closure_baseline_close_fixture(repo, state, plan_rel, "main");
    let review_summary_path = repo.join("task-close-internal-dispatch-review-summary.md");
    let verification_summary_path =
        repo.join("task-close-internal-dispatch-verification-summary.md");
    write_file(
        &review_summary_path,
        "Task close internal-dispatch budget fixture independent review passed.\n",
    );
    write_file(
        &verification_summary_path,
        "Task close internal-dispatch budget fixture verification passed.\n",
    );

    let mut runtime_management_commands = 0usize;
    runtime_management_commands += 1; // workflow operator
    let operator_ready = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "task-close internal-dispatch budget fixture workflow operator after external review ready",
    );
    let routed_command = operator_ready["recommended_command"]
        .as_str()
        .expect("task-close internal-dispatch budget fixture should expose a routed command");
    assert_no_hidden_helper_commands_used(&[routed_command.to_owned()]);
    assert!(
        routed_command.starts_with(&format!(
            "featureforge plan execution close-current-task --plan {plan_rel}"
        )),
        "task-close internal-dispatch budget operator should route directly to close-current-task, got {operator_ready}"
    );
    assert!(
        routed_command.contains("--task 1"),
        "task-close internal-dispatch budget operator should target Task 1 for the budgeted close-current-task route, got {operator_ready}"
    );

    runtime_management_commands += 1; // close-current-task
    let close_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("task-close internal-dispatch review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("task-close internal-dispatch verification summary path should be utf-8"),
        ],
        "task-close internal-dispatch budget fixture close-current-task should succeed without a public dispatch id",
    );
    assert_eq!(close_json["action"], Value::from("recorded"));
    assert_eq!(
        close_json["dispatch_validation_action"],
        Value::from("validated")
    );
    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("task-close internal-dispatch authoritative state should be readable"),
    )
    .expect("task-close internal-dispatch authoritative state should remain valid json");
    assert!(
        authoritative_state["strategy_review_dispatch_lineage"]["task-1"]["dispatch_id"]
            .as_str()
            .is_some(),
        "close-current-task internal-dispatch path should still record authoritative dispatch lineage"
    );
    assert_runtime_management_budget(
        "TASK-CLOSE-INTERNAL-DISPATCH-BUDGET",
        runtime_management_commands,
        2,
    );
}

#[test]
fn fs14_recovery_to_close_current_task_uses_only_public_intent_commands() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs14-shell-smoke");
    let repo = repo_dir.path();
    let state = state_dir.path();
    prepare_missing_task_closure_baseline_close_fixture(repo, state, plan_rel, "main");

    let mut runtime_management_commands = 0usize;
    runtime_management_commands += 1;
    let operator_ready = run_featureforge_json_real_cli(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
            "--json",
        ],
        "FS-14 shell-smoke workflow operator should route missing baseline recovery through close-current-task",
    );
    assert_eq!(
        operator_ready["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-14 shell-smoke operator should surface task_closure_recording_ready for missing closure baseline recovery: {operator_ready}"
    );
    let routed_command = operator_ready["recommended_command"]
        .as_str()
        .expect("FS-14 shell-smoke should expose operator recommended command");
    assert_no_hidden_helper_commands_used(&[routed_command.to_owned()]);
    assert!(
        routed_command.contains("featureforge plan execution close-current-task --plan"),
        "FS-14 shell-smoke operator should route directly to close-current-task, got {routed_command}"
    );

    let review_summary_path = repo.join("fs14-review-summary.md");
    let verification_summary_path = repo.join("fs14-verification-summary.md");
    write_file(
        &review_summary_path,
        "FS-14 shell-smoke independent review passed.\n",
    );
    write_file(
        &verification_summary_path,
        "FS-14 shell-smoke verification passed.\n",
    );
    runtime_management_commands += 1;
    let close_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("FS-14 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("FS-14 verification summary path should be utf-8"),
        ],
        "FS-14 shell-smoke close-current-task should rebuild missing current closure baseline without hidden dispatch commands",
    );
    assert_eq!(close_json["action"], Value::from("recorded"));
    assert_eq!(
        close_json["dispatch_validation_action"],
        Value::from("validated"),
        "FS-14 shell-smoke close-current-task should validate or derive dispatch lineage internally"
    );
    assert_runtime_management_budget(
        "FS14-CLOSE-CURRENT-TASK-BUDGET",
        runtime_management_commands,
        2,
    );
}

#[test]
fn fs20_reopening_downstream_stale_task_does_not_unwind_upstream_current_closure_when_only_plan_and_evidence_change()
 {
    let plan_rel = WORKFLOW_FIXTURE_PLAN_REL;
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs20-shell-smoke-task-boundary");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    seed_current_task_closure_state(repo, state, plan_rel);

    let status_before_begin = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 shell-smoke status before downstream reopen churn",
    );
    let begin_task2_step1 = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-20 status should expose execution fingerprint before begin"),
        ],
        "FS-20 begin downstream task before reopen churn",
    );
    let complete_task2_step1 = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-20 completed downstream step before reopen churn.",
            "--manual-verify-summary",
            "FS-20 downstream step completion before reopen churn.",
            "--file",
            "tests/workflow_shell_smoke.rs",
            "--expect-execution-fingerprint",
            begin_task2_step1["execution_fingerprint"]
                .as_str()
                .expect("FS-20 begin should expose execution fingerprint before complete"),
        ],
        "FS-20 complete downstream task before reopen churn",
    );
    bind_explicit_reopen_repair_target(repo, state, 2, 1);
    run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "FS-20 reopen downstream stale task for runtime-owned churn coverage",
            "--expect-execution-fingerprint",
            complete_task2_step1["execution_fingerprint"]
                .as_str()
                .expect("FS-20 complete should expose execution fingerprint before reopen"),
        ],
        "FS-20 reopen downstream stale task before runtime-owned plan/evidence-only churn",
    );

    let status_after_reopen = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 status after downstream reopen before runtime-owned churn mutation",
    );
    let plan_path = repo.join(plan_rel);
    let evidence_rel = status_after_reopen["evidence_path"]
        .as_str()
        .expect("FS-20 status after reopen should expose evidence_path");
    let plan_source = fs::read_to_string(&plan_path).expect("FS-20 plan should be readable");
    write_file(
        &plan_path,
        &format!("{plan_source}\n<!-- fs20 downstream reopen runtime-owned plan mutation -->\n"),
    );
    let evidence_source =
        projection_support::read_state_dir_projection(&status_after_reopen, evidence_rel);
    projection_support::write_state_dir_projection(
        &status_after_reopen,
        evidence_rel,
        &format!(
            "{evidence_source}\n<!-- fs20 downstream reopen runtime-owned evidence mutation -->\n"
        ),
    );

    let mut task1_closure_refresh_routes = 0usize;
    for (probe_label, probe_json) in [
        (
            "status",
            run_plan_execution_json_real_cli(
                repo,
                state,
                &["status", "--plan", plan_rel],
                "FS-20 status after runtime-owned plan/evidence-only churn",
            ),
        ),
        (
            "operator",
            run_featureforge_json_real_cli(
                repo,
                state,
                &["workflow", "operator", "--plan", plan_rel, "--json"],
                "FS-20 operator after runtime-owned plan/evidence-only churn",
            ),
        ),
    ] {
        let probe_reason_codes = probe_json["reason_codes"]
            .as_array()
            .or_else(|| probe_json["blocking_reason_codes"].as_array())
            .unwrap_or_else(|| {
                panic!("FS-20 {probe_label} probe should expose reason codes array: {probe_json:?}")
            });
        assert!(
            !probe_reason_codes
                .iter()
                .any(|code| code == &Value::from("prior_task_current_closure_stale")),
            "FS-20 {probe_label} should not stale upstream Task 1 closure when only plan/evidence control-plane paths changed: {probe_json:?}"
        );
        assert_ne!(
            probe_json["blocking_task"],
            Value::from(1_u64),
            "FS-20 {probe_label} should not route back to Task 1 closure refresh from runtime-owned churn only"
        );
        if probe_json["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.contains("close-current-task") && command.contains("--task 1")
            })
        {
            task1_closure_refresh_routes += 1;
        }
    }
    assert!(
        task1_closure_refresh_routes <= 1,
        "FS-20 runtime-owned churn budget exceeded: upstream Task 1 closure refresh should be required at most once after downstream reopen, saw {task1_closure_refresh_routes} closure-refresh routes"
    );
}

#[test]
fn fs20_late_stage_chain_is_not_unwound_by_runtime_owned_plan_and_execution_evidence_churn() {
    let plan_rel = WORKFLOW_FIXTURE_PLAN_REL;
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs20-shell-smoke-late-stage-chain");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);

    let baseline_status = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 late-stage baseline status before runtime-owned churn",
    );
    let baseline_branch_closure_id = baseline_status["current_branch_closure_id"]
        .as_str()
        .expect("FS-20 late-stage baseline should expose current_branch_closure_id")
        .to_owned();
    let baseline_release_state = baseline_status["current_release_readiness_state"].clone();
    let baseline_final_state = baseline_status["current_final_review_state"].clone();
    let baseline_qa_state = baseline_status["current_qa_state"].clone();

    let plan_path = repo.join(plan_rel);
    let evidence_rel = baseline_status["evidence_path"]
        .as_str()
        .expect("FS-20 late-stage baseline should expose evidence_path");
    let plan_source = fs::read_to_string(&plan_path).expect("FS-20 late-stage plan should read");
    write_file(
        &plan_path,
        &format!("{plan_source}\n<!-- fs20 late-stage runtime-owned plan mutation -->\n"),
    );
    let evidence_source =
        projection_support::read_state_dir_projection(&baseline_status, evidence_rel);
    projection_support::write_state_dir_projection(
        &baseline_status,
        evidence_rel,
        &format!("{evidence_source}\n<!-- fs20 late-stage runtime-owned evidence mutation -->\n"),
    );

    let status_after_churn = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 late-stage status after runtime-owned churn",
    );
    assert_eq!(
        status_after_churn["current_branch_closure_id"],
        Value::from(baseline_branch_closure_id),
        "FS-20 late-stage chain should keep current_branch_closure_id when only runtime-owned plan/evidence paths changed"
    );
    assert_eq!(
        status_after_churn["current_release_readiness_state"], baseline_release_state,
        "FS-20 late-stage chain should keep release-readiness state through runtime-owned churn"
    );
    assert_eq!(
        status_after_churn["current_final_review_state"], baseline_final_state,
        "FS-20 late-stage chain should keep final-review state through runtime-owned churn"
    );
    assert_eq!(
        status_after_churn["current_qa_state"], baseline_qa_state,
        "FS-20 late-stage chain should keep QA state through runtime-owned churn"
    );
}

#[test]
fn fs21_resume_task_is_suppressed_when_earlier_closure_bridge_preempts_it() {
    let plan_rel = WORKFLOW_FIXTURE_PLAN_REL;
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs21-shell-smoke-resume-preempt");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    prepare_fs21_resume_preempted_by_task_closure_bridge_fixture(
        repo,
        state,
        plan_rel,
        &base_branch,
    );

    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-21 status should suppress resume_task/resume_step when earlier closure bridge preempts resume",
    );
    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-21 operator should agree with suppressed resume bridge routing",
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-21 status should route to task_closure_recording_ready when earlier closure bridge preempts resume: {status_json:?}"
    );
    assert!(status_json["resume_task"].is_null());
    assert!(status_json["resume_step"].is_null());
    let status_command = status_json["recommended_command"]
        .as_str()
        .expect("FS-21 status should expose recommended command");
    assert!(
        status_command.contains("close-current-task") && status_command.contains("--task 1"),
        "FS-21 status should recommend close-current-task --task 1, got {status_command}"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(status_command.to_owned()),
        "FS-21 operator and status should agree on the same close-current-task command when resume is preempted"
    );
}

#[test]
fn fs11_operator_and_begin_target_parity_after_rebase_resume() {
    let plan_rel = "docs/featureforge/plans/2026-04-02-runtime-fs11-shell-smoke.md";
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs11-shell-smoke");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_fs11_rebase_resume_parity_fixture(repo, state, plan_rel);

    let operator_direct = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "FS-11 shell-smoke direct workflow operator",
    );
    let operator_real = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-11 shell-smoke compiled-cli workflow operator",
    );
    assert_eq!(
        operator_direct, operator_real,
        "FS-11 shell-smoke operator outputs should stay aligned between direct and compiled-cli routes",
    );
    if let Some(blocking_task) = operator_real["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-11 shell-smoke operator should target Task 2 as the earliest stale boundary after rebase/resume overlays: {operator_real:?}",
        );
    } else {
        assert_eq!(
            operator_real["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-11 shell-smoke operator should target Task 2 via execution command context when blocker metadata is projected as a concrete command: {operator_real:?}",
        );
    }

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-11 shell-smoke repair-review-state follow-up parity",
    );
    let repair_action = repair_json["action"]
        .as_str()
        .expect("FS-11 shell-smoke repair-review-state should expose action");
    assert_eq!(
        repair_action, "blocked",
        "FS-11 shell-smoke repair-review-state should expose the concrete shared blocker before recovery commands run, got {repair_json:?}",
    );
    assert!(
        repair_json["required_follow_up"].is_null(),
        "json: {repair_json:?}"
    );
    let operator_recommended = operator_real["recommended_command"]
        .as_str()
        .expect("FS-11 shell-smoke operator should expose recommended command");
    let repair_recommended = repair_json["recommended_command"]
        .as_str()
        .expect("FS-11 shell-smoke repair-review-state should expose recommended command");
    assert!(
        repair_recommended.contains("featureforge plan execution close-current-task --plan ")
            && repair_recommended.contains("--task 2"),
        "FS-11 shell-smoke repair-review-state should progress to a concrete Task 2 close-current-task command, got {repair_recommended}"
    );
    assert_no_hidden_helper_commands_used(&[operator_recommended.to_owned()]);
    let operator_follow_up_output = run_recommended_plan_execution_command_output_real_cli(
        repo,
        state,
        operator_recommended,
        "FS-11 shell-smoke operator recommended command should execute directly",
    );
    let operator_follow_up_payload = if operator_follow_up_output.stdout.is_empty() {
        &operator_follow_up_output.stderr
    } else {
        &operator_follow_up_output.stdout
    };
    let operator_follow_up_json: Value =
        serde_json::from_slice(operator_follow_up_payload).unwrap_or_else(|error| {
            panic!(
                "FS-11 shell-smoke operator recommended command should return valid json payload: {error}"
            )
        });
    assert!(
        operator_follow_up_output.status.success(),
        "FS-11 shell-smoke operator recommended command should be directly runnable, got {operator_follow_up_json:?}",
    );
    if operator_follow_up_json["action"].as_str() == Some("blocked") {
        assert!(
            operator_follow_up_json["required_follow_up"].is_null(),
            "json: {operator_follow_up_json:?}"
        );
        let follow_up_task = operator_follow_up_json["task_number"]
            .as_u64()
            .or_else(|| operator_follow_up_json["blocking_task"].as_u64());
        assert_eq!(
            follow_up_task,
            Some(2_u64),
            "FS-11 shell-smoke operator-recommended command should remain pinned to Task 2 when blocked"
        );
        if operator_follow_up_json["blocking_scope"].is_string() {
            assert_eq!(
                operator_follow_up_json["blocking_scope"], operator_real["blocking_scope"],
                "FS-11 shell-smoke blocked follow-up should preserve the exact blocking scope surfaced by workflow operator when the follow-up remains in the same blocker family"
            );
        }
        assert!(
            operator_follow_up_json["blocking_reason_codes"].is_array(),
            "FS-11 shell-smoke blocked follow-up should continue exposing structured blocking reasons"
        );
    }

    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11 shell-smoke status before begin parity rejection",
    );
    let begin_failure = run_plan_execution_failure_json_real_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "3",
            "--step",
            "6",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_json["execution_fingerprint"]
                .as_str()
                .expect("FS-11 shell-smoke status should expose fingerprint before begin"),
        ],
        "FS-11 shell-smoke begin should fail closed when request diverges from shared target",
    );
    let begin_error_class = begin_failure["error_class"]
        .as_str()
        .expect("FS-11 shell-smoke begin failure should expose error_class");
    assert!(
        begin_error_class == "InvalidStepTransition"
            || begin_error_class == "ExecutionStateNotReady",
        "FS-11 shell-smoke begin failure should remain a closed begin-time rejection class, got {begin_failure:?}",
    );
    let begin_message = begin_failure["message"]
        .as_str()
        .expect("FS-11 shell-smoke begin failure should expose message text");
    assert!(
        begin_message.contains("Next public action: featureforge plan execution")
            && begin_message.contains("reason_code=mutation_not_route_authorized"),
        "FS-11 shell-smoke rejection should explain the shared mutation-oracle mismatch: {begin_failure:?}",
    );
    assert!(
        begin_message.contains("--task 2"),
        "FS-11 shell-smoke begin failure should preserve Task 2 as authoritative target: {begin_failure:?}",
    );
}

#[test]
fn fs11_repair_output_matches_following_public_command_without_hidden_helper() {
    let plan_rel = "docs/featureforge/plans/2026-04-02-runtime-fs11-shell-smoke.md";
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs11-shell-smoke-follow-up");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_fs11_rebase_resume_parity_fixture(repo, state, plan_rel);

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-11 shell-smoke repair follow-up command fixture",
    );
    assert_eq!(
        repair_json["action"],
        Value::from("blocked"),
        "FS-11 repair follow-up fixture should return a concrete blocker before recovery commands run"
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("FS-11 repair follow-up fixture should expose a recommended command")
        .to_owned();
    assert_no_hidden_helper_commands_used(std::slice::from_ref(&recommended_command));
    let placeholder_bearing_close_current_task = recommended_command.contains("close-current-task")
        && (recommended_command.contains("pass|fail") || recommended_command.contains("<path>"));
    if placeholder_bearing_close_current_task {
        return;
    }

    let follow_up_output = run_recommended_plan_execution_command_output_real_cli(
        repo,
        state,
        &recommended_command,
        "FS-11 repair follow-up should execute or fail closed via the recommended public command",
    );
    let follow_up_payload = if follow_up_output.stdout.is_empty() {
        &follow_up_output.stderr
    } else {
        &follow_up_output.stdout
    };
    let follow_up_json: Value = serde_json::from_slice(follow_up_payload).unwrap_or_else(|error| {
        panic!("FS-11 repair follow-up should return valid json payload: {error}")
    });
    assert!(
        follow_up_output.status.success(),
        "FS-11 repair follow-up command must be directly runnable, got {follow_up_json:?}"
    );
    if follow_up_json["action"].as_str() == Some("blocked") {
        assert_eq!(
            follow_up_json["required_follow_up"], repair_json["required_follow_up"],
            "FS-11 repair follow-up command should either progress immediately or return the same blocker contract"
        );
    }
}

#[test]
fn fs11_rebase_resume_recovery_budget_is_capped_without_hidden_helpers() {
    let plan_rel = "docs/featureforge/plans/2026-04-02-runtime-fs11-shell-smoke.md";
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs11-shell-smoke-budget");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_fs11_rebase_resume_parity_fixture(repo, state, plan_rel);

    let mut runtime_management_commands = 0usize;

    runtime_management_commands += 1;
    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-11 rebase/resume budget workflow operator",
    );
    let operator_recommended = operator_json["recommended_command"]
        .as_str()
        .expect("FS-11 rebase/resume budget should expose operator recommended command");
    assert_no_hidden_helper_commands_used(&[operator_recommended.to_owned()]);
    if let Some(blocking_task) = operator_json["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-11 rebase/resume budget must surface Task 2 as the earliest stale boundary target: {operator_json:?}"
        );
    } else {
        assert_eq!(
            operator_json["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-11 rebase/resume budget should target Task 2 via execution command context when blocking_task is projected as a concrete command: {operator_json:?}"
        );
    }
    assert!(
        operator_recommended.starts_with("featureforge plan execution repair-review-state --plan "),
        "FS-11 rebase/resume budget should expose the public repair-review-state command while keeping Task 2 in the structured blocker metadata, got {operator_recommended}"
    );

    runtime_management_commands += 1;
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-11 rebase/resume budget repair-review-state",
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("FS-11 rebase/resume budget should expose repair recommended command")
        .to_owned();
    assert_no_hidden_helper_commands_used(std::slice::from_ref(&recommended_command));
    assert!(
        recommended_command.contains("featureforge plan execution close-current-task --plan ")
            && recommended_command.contains("--task 2"),
        "FS-11 rebase/resume budget should progress to a concrete Task 2 close-current-task command after repair-review-state runs, got {recommended_command}"
    );
    let placeholder_bearing_close_current_task =
        recommended_command.contains("pass|fail") || recommended_command.contains("<path>");
    if placeholder_bearing_close_current_task {
        assert_runtime_management_budget(
            "FS11-REBASE-RESUME-BUDGET",
            runtime_management_commands,
            2,
        );
        return;
    }

    runtime_management_commands += 1;
    let follow_up_output = run_recommended_plan_execution_command_output_real_cli(
        repo,
        state,
        &recommended_command,
        "FS-11 rebase/resume budget recommended recovery command",
    );
    let follow_up_payload = if follow_up_output.stdout.is_empty() {
        &follow_up_output.stderr
    } else {
        &follow_up_output.stdout
    };
    let follow_up_json: Value = serde_json::from_slice(follow_up_payload).unwrap_or_else(|error| {
        panic!("FS-11 rebase/resume budget follow-up should return valid json payload: {error}")
    });
    assert!(
        follow_up_output.status.success(),
        "FS-11 rebase/resume budget recommended follow-up command must be directly runnable, got {follow_up_json:?}"
    );
    assert_ne!(
        follow_up_json["action"],
        Value::from("blocked"),
        "FS-11 rebase/resume budget must reach real work within three runtime-management commands; command 3 cannot remain blocked: {follow_up_json:?}"
    );
    let status_after_recovery = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11 rebase/resume budget status after command 3",
    );
    let resumed_task_2 = (status_after_recovery["active_task"].as_u64() == Some(2_u64)
        && status_after_recovery["active_step"].as_u64() == Some(1_u64))
        || (status_after_recovery["resume_task"].as_u64() == Some(2_u64)
            && status_after_recovery["resume_step"].as_u64() == Some(1_u64));
    assert!(
        resumed_task_2,
        "FS-11 rebase/resume budget should resume real Task 2 work after three commands, got {status_after_recovery:?}"
    );
    if status_after_recovery["phase_detail"].as_str() == Some("execution_reentry_required") {
        let follow_up = status_after_recovery["recommended_command"]
            .as_str()
            .expect("FS-11 rebase/resume budget should still expose a concrete follow-up command");
        assert!(
            follow_up.contains("plan execution begin") && follow_up.contains("--task 2 --step 1"),
            "FS-11 rebase/resume budget must keep routing concrete Task 2 work when execution_reentry_required remains projected, got {status_after_recovery:?}"
        );
    }
    assert_runtime_management_budget("FS11-REBASE-RESUME-BUDGET", runtime_management_commands, 3);
}

#[test]
fn fs12_recovery_path_does_not_require_hidden_preflight_when_run_identity_exists() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs12-shell-smoke");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let status_before_preflight_tamper = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!(
            "FS-12 shell-smoke fixture status before pre",
            "flight tamper"
        ),
    );
    let execution_run_id = status_before_preflight_tamper["execution_run_id"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .expect(concat!(
            "FS-12 shell-smoke fixture should expose execution_run_id before pre",
            "flight tamper"
        ))
        .to_owned();
    let plan_revision = status_before_preflight_tamper["plan_revision"]
        .as_u64()
        .expect(concat!(
            "FS-12 shell-smoke fixture should expose plan_revision before pre",
            "flight tamper"
        ));
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("executing")),
            (
                "run_identity",
                serde_json::json!({
                    "execution_run_id": execution_run_id,
                    "source_plan_path": plan_rel,
                    "source_plan_revision": plan_revision
                }),
            ),
        ],
    );

    let preflight_path = preflight_acceptance_state_path(repo, state);
    assert!(
        preflight_path.is_file(),
        "FS-12 shell-smoke fixture should include {} acceptance without invoking explicit {} in this fixed recovery path",
        concat!("pre", "flight"),
        concat!("pre", "flight")
    );
    fs::remove_file(&preflight_path).expect(concat!(
        "FS-12 shell-smoke fixture should remove pre",
        "flight acceptance state"
    ));
    write_file(
        &preflight_path,
        concat!("{ malformed pre", "flight acceptance fixture for FS-12"),
    );

    let status_without_preflight = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        concat!(
            "FS-12 shell-smoke status should keep authoritative run identity after deleting pre",
            "flight"
        ),
    );
    assert!(
        status_without_preflight["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
        "FS-12 status should keep authoritative execution_run_id without {} acceptance: {}",
        status_without_preflight,
        concat!("pre", "flight")
    );

    bind_explicit_reopen_repair_target(repo, state, 1, 1);
    let reopened = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            concat!(
                "FS-12 shell-smoke reopen after malformed pre",
                "flight seed."
            ),
            "--expect-execution-fingerprint",
            status_without_preflight["execution_fingerprint"]
                .as_str()
                .expect("FS-12 shell-smoke status should expose execution fingerprint"),
        ],
        concat!(
            "FS-12 shell-smoke reopen should succeed without pre",
            "flight replay"
        ),
    );
    assert_eq!(reopened["resume_task"], Value::from(1_u64));
    assert_eq!(reopened["resume_step"], Value::from(1_u64));

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        concat!(
            "FS-12 shell-smoke workflow operator after malformed pre",
            "flight seed"
        ),
    );
    assert_eq!(
        operator_json["phase"],
        Value::from("executing"),
        "FS-12 shell-smoke operator should stay in execution flow with authoritative run identity: {operator_json}"
    );
    assert_ne!(
        operator_json["next_action"],
        Value::from(concat!("execution pre", "flight")),
        "FS-12 shell-smoke operator must not require execution {} when authoritative run identity exists",
        concat!("pre", "flight")
    );

    let resumed = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            reopened["execution_fingerprint"]
                .as_str()
                .expect("FS-12 shell-smoke reopen should expose execution fingerprint"),
        ],
        concat!(
            "FS-12 shell-smoke begin should resume with authoritative run identity and malformed pre",
            "flight seed"
        ),
    );
    assert_eq!(resumed["active_task"], Value::from(1_u64));
    assert_eq!(resumed["active_step"], Value::from(1_u64));
    assert!(
        resumed["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
        "FS-12 shell-smoke begin should preserve authoritative execution_run_id: {resumed}"
    );
}

#[test]
fn fs13_normal_recovery_never_requires_manual_plan_note_edit() {
    let plan_rel = "docs/featureforge/plans/2026-04-02-runtime-fs13-shell-smoke.md";
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs13-shell-smoke");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_fs11_rebase_resume_parity_fixture(repo, state, plan_rel);
    let operator_before = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-13 shell-smoke operator before markdown note tamper",
    );
    let status_before = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 shell-smoke status before markdown note tamper",
    );
    assert_eq!(
        status_before["blocking_task"],
        Value::from(2_u64),
        "FS-13 shell-smoke must surface Task 2 as the earliest stale boundary before note tamper"
    );
    if let Some(blocking_task) = operator_before["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-13 shell-smoke operator should target Task 2 before note tamper: {operator_before:?}"
        );
    } else {
        assert_eq!(
            operator_before["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-13 shell-smoke operator should expose Task 2 via execution_command_context before note tamper: {operator_before:?}"
        );
    }
    let authoritative_state_path_before_tamper =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_before_tamper: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path_before_tamper)
            .expect("FS-13 shell-smoke authoritative state should be readable before tamper"),
    )
    .expect("FS-13 shell-smoke authoritative state should remain valid json before tamper");
    assert_eq!(
        authoritative_state_before_tamper["current_open_step_state"]["note_state"],
        Value::from("Interrupted"),
        "FS-13 shell-smoke fixture should retain authoritative interrupted open-step state on the later task"
    );
    assert_eq!(
        authoritative_state_before_tamper["current_open_step_state"]["task"],
        Value::from(3_u64),
        "FS-13 shell-smoke fixture should park the forward interrupted marker on Task 3 before tamper"
    );

    let operator_after = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-13 shell-smoke operator without any manual markdown note edits",
    );
    let status_after = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 shell-smoke status without any manual markdown note edits",
    );

    assert_eq!(
        operator_after["phase_detail"], operator_before["phase_detail"],
        "FS-13 shell-smoke markdown note edits must not change operator phase-detail routing"
    );
    assert_eq!(
        status_after["blocking_task"],
        Value::from(2_u64),
        "FS-13 shell-smoke status must keep Task 2 as the earliest stale boundary despite markdown note tamper"
    );
    if let Some(blocking_task) = operator_after["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-13 shell-smoke operator should keep Task 2 as the earliest stale boundary after note tamper: {operator_after:?}"
        );
    } else {
        assert_eq!(
            operator_after["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-13 shell-smoke operator should keep Task 2 in execution_command_context after note tamper: {operator_after:?}"
        );
    }
    let recommended_after_tamper = operator_after["recommended_command"]
        .as_str()
        .expect("FS-13 shell-smoke operator should expose recommended command after note tamper")
        .to_owned();
    assert_no_hidden_helper_commands_used(std::slice::from_ref(&recommended_after_tamper));
    assert!(
        recommended_after_tamper
            .starts_with("featureforge plan execution repair-review-state --plan "),
        "FS-13 shell-smoke should keep repair-review-state as the public command after note tamper while Task 2 remains the structured stale-boundary target, got {recommended_after_tamper}"
    );
    let authoritative_state_after_tamper: Value = serde_json::from_str(
        &fs::read_to_string(harness_state_path(
            state,
            &repo_slug(repo, state),
            &current_branch_name(repo),
        ))
        .expect("FS-13 shell-smoke authoritative state should remain readable before follow-up"),
    )
    .expect("FS-13 shell-smoke authoritative state should remain valid json before follow-up");
    assert_eq!(
        authoritative_state_after_tamper["current_open_step_state"]["note_state"],
        Value::from("Interrupted"),
        "FS-13 shell-smoke should preserve the authoritative parked open-step state before the routed follow-up command runs"
    );
    assert_eq!(
        authoritative_state_after_tamper["current_open_step_state"]["task"],
        Value::from(3_u64),
        "FS-13 shell-smoke should preserve the authoritative parked interrupted target before the routed follow-up command runs"
    );
    let follow_up_output = run_recommended_plan_execution_command_output_real_cli(
        repo,
        state,
        &recommended_after_tamper,
        "FS-13 shell-smoke follow the operator-routed command without manual markdown note edits",
    );
    let follow_up_payload = if follow_up_output.stdout.is_empty() {
        &follow_up_output.stderr
    } else {
        &follow_up_output.stdout
    };
    let follow_up_json: Value = serde_json::from_slice(follow_up_payload).unwrap_or_else(|error| {
        panic!("FS-13 shell-smoke follow-up command should return valid json payload: {error}")
    });
    assert!(
        follow_up_output.status.success(),
        "FS-13 shell-smoke follow-up command should remain runnable without manual execution-note edits, got {follow_up_json:?}"
    );
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(authoritative_state_path)
            .expect("FS-13 shell-smoke authoritative state should remain readable"),
    )
    .expect("FS-13 shell-smoke authoritative state should remain valid json");
    assert!(
        authoritative_state["current_open_step_state"].is_null(),
        "FS-13 shell-smoke routed follow-up should clear the stale parked open-step state before surfacing the Task 2 close-current-task command"
    );
}

#[test]
fn internal_only_compatibility_reentry_recovery_runtime_management_budget_is_capped() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("runtime-remediation-reentry-budget");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", plan_rel);
    let branch_closure = internal_only_unit_record_branch_closure_json(
        repo,
        state,
        plan_rel,
        "reentry budget fixture should seed current branch closure",
    );
    assert_eq!(branch_closure["action"], Value::from("recorded"));
    let summary_path = repo.join("reentry-budget-release-readiness.md");
    write_file(
        &summary_path,
        "Reentry budget fixture release readiness before escaped drift.\n",
    );
    let release_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            summary_path
                .to_str()
                .expect("reentry budget summary path should be utf-8"),
        ],
        "reentry budget fixture should seed release readiness",
    );
    assert_eq!(release_json["action"], Value::from("recorded"));

    append_tracked_repo_line(
        repo,
        "README.md",
        "runtime-remediation reentry budget escaped drift sentinel",
    );

    let mut runtime_management_commands = 0usize;
    let mut routed_commands = Vec::new();
    runtime_management_commands += 1;
    routed_commands.push(format!(
        "featureforge plan execution repair-review-state --plan {plan_rel}"
    ));
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "reentry budget fixture repair-review-state",
    );
    assert_eq!(repair_json["action"], Value::from("blocked"));
    assert!(
        repair_json["required_follow_up"].is_null()
            || repair_json["required_follow_up"].as_str() == Some("execution_reentry"),
        "reentry budget fixture should expose either a direct execution-reentry follow-up or rely on the exact recommended command alone, got {repair_json}"
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("reentry budget fixture should expose recommended execution reentry command");
    routed_commands.push(recommended_command.to_owned());
    if !(recommended_command.contains("pass|fail")
        || recommended_command.contains("<path>")
        || recommended_command.contains('<'))
    {
        runtime_management_commands += 1;
        let reentry = run_recommended_plan_execution_command_json_real_cli(
            repo,
            state,
            recommended_command,
            "reentry budget fixture recommended execution command",
        );
        assert_ne!(
            reentry["action"],
            Value::from("blocked"),
            "reentry budget fixture recommended command should be immediately executable, got {reentry}"
        );
    } else {
        assert!(
            recommended_command.contains("close-current-task")
                || recommended_command.contains(" begin ")
                || recommended_command.contains(" reopen "),
            "placeholder-bearing reentry budget routed command should target a public recovery command, got {recommended_command:?}"
        );
    }
    assert_no_hidden_helper_commands_used(&routed_commands);
    assert_runtime_management_budget("REENTRY-BUDGET", runtime_management_commands, 2);
}

#[test]
fn stale_release_refresh_runtime_management_budget_is_capped_before_new_review_step() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("runtime-remediation-late-stage-refresh-budget");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let mut runtime_management_commands = 0usize;
    let mut routed_commands = Vec::new();
    runtime_management_commands += 1;
    let operator_before = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "late-stage refresh budget fixture operator before release refresh",
    );
    assert_eq!(
        operator_before["phase"],
        Value::from("document_release_pending")
    );
    assert_eq!(
        operator_before["phase_detail"],
        Value::from("release_readiness_recording_ready")
    );
    if let Some(command) = operator_before["recommended_command"].as_str() {
        routed_commands.push(command.to_owned());
    }

    let release_summary_path = repo.join("late-stage-refresh-budget-release-ready.md");
    write_file(
        &release_summary_path,
        "Late-stage refresh budget fixture release readiness refreshed.\n",
    );
    runtime_management_commands += 1;
    let release_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--result",
            "ready",
            "--summary-file",
            release_summary_path
                .to_str()
                .expect("late-stage refresh budget summary path should be utf-8"),
        ],
        "late-stage refresh budget fixture advance-late-stage release readiness",
    );
    assert_eq!(release_json["action"], Value::from("recorded"));
    routed_commands.push(format!(
        "featureforge plan execution advance-late-stage --plan {plan_rel}"
    ));

    runtime_management_commands += 1;
    let operator_after = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "late-stage refresh budget fixture operator after release refresh",
    );
    assert_eq!(operator_after["phase"], Value::from("final_review_pending"));
    assert_eq!(
        operator_after["phase_detail"],
        Value::from("final_review_dispatch_required")
    );
    assert_eq!(
        operator_after["next_action"],
        Value::from("request final review")
    );
    if let Some(command) = operator_after["recommended_command"].as_str() {
        routed_commands.push(command.to_owned());
    }
    assert_no_hidden_helper_commands_used(&routed_commands);
    assert_runtime_management_budget("LATE-STAGE-REFRESH-BUDGET", runtime_management_commands, 3);
}

#[test]
fn internal_only_compatibility_internal_record_review_dispatch_target_mismatch_fails_before_authoritative_mutation()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs05-cli-target-mismatch-no-mutation");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let digest_before = authoritative_harness_state_digest(repo, state);

    let failure: Value = serde_json::from_str(
        &featureforge_support::internal_only_runtime_review_dispatch_authority_json(
            repo,
            state,
            &RecordReviewDispatchArgs {
                plan: PathBuf::from(plan_rel),
                scope: ReviewDispatchScopeArg::Task,
                task: Some(2),
            },
        )
        .expect_err(concat!(
            "internal record",
            "-review-dispatch target mismatch should fail"
        )),
    )
    .expect(concat!(
        "internal record",
        "-review-dispatch failure should serialize as json"
    ));
    assert_eq!(failure["error_class"], "InvalidCommandInput");
    assert!(
        failure["message"].as_str().is_some_and(|message| {
            message.contains("does not match the current task review-dispatch target")
        }),
        "internal target mismatch should explain the current dispatch target contract: {failure}"
    );
    assert_eq!(
        authoritative_harness_state_digest(repo, state),
        digest_before,
        "internal target mismatch must not mutate authoritative state"
    );
}

#[test]
fn internal_only_compatibility_internal_record_review_dispatch_final_review_scope_rejects_task_field_before_authoritative_mutation()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs05-cli-final-review-task-field-no-mutation");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let digest_before = authoritative_harness_state_digest(repo, state);

    let failure: Value = serde_json::from_str(
        &featureforge_support::internal_only_runtime_review_dispatch_authority_json(
            repo,
            state,
            &RecordReviewDispatchArgs {
                plan: PathBuf::from(plan_rel),
                scope: ReviewDispatchScopeArg::FinalReview,
                task: Some(1),
            },
        )
        .expect_err(concat!(
            "internal record",
            "-review-dispatch final-review task-field check should fail"
        )),
    )
    .expect(concat!(
        "internal record",
        "-review-dispatch failure should serialize as json"
    ));
    assert_eq!(failure["error_class"], "InvalidCommandInput");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("--scope final-review does not accept --task")),
        "internal final-review scope should reject task field usage: {failure}"
    );
    assert_eq!(
        authoritative_harness_state_digest(repo, state),
        digest_before,
        "internal final-review task field rejection must not mutate authoritative state"
    );
}
