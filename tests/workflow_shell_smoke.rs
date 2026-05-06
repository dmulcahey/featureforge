#[path = "support/dir_tree.rs"]
mod dir_tree_support;
#[path = "support/executable.rs"]
mod executable_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/prebuilt.rs"]
mod prebuilt_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/projection.rs"]
mod projection_support;
#[path = "support/public_featureforge_cli.rs"]
mod public_featureforge_cli;
#[path = "support/runtime_json.rs"]
mod runtime_json_support;
#[path = "support/runtime_surfaces.rs"]
mod runtime_surfaces_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use dir_tree_support::copy_dir_recursive;
use executable_support::make_executable;
use featureforge::contracts::harness::{
    WorktreeLease, WorktreeLeaseState, read_execution_contract,
};
use featureforge::contracts::plan::{PLAN_FIDELITY_REQUIRED_SURFACES, parse_plan_file};
use featureforge::contracts::spec::parse_spec_file;
use featureforge::execution::authority::{
    persist_active_worktree_lease_index, write_authoritative_worktree_lease_artifact,
};
use featureforge::execution::final_review::resolve_release_base_branch;
use featureforge::execution::follow_up::execution_step_repair_target_id;
use featureforge::execution::harness::{
    ChunkId, ExecutionRunId, RunIdentitySnapshot, WorktreeLeaseBindingSnapshot,
};
use featureforge::execution::semantic_identity::{
    branch_definition_identity_for_context, task_definition_identity_for_task,
};
use featureforge::execution::state::{
    current_head_sha as runtime_current_head_sha,
    current_tracked_tree_sha as runtime_current_tracked_tree_sha, hash_contract_plan,
    load_execution_context,
};
use featureforge::git::{discover_repo_identity, discover_repository, discover_slug_identity};
use featureforge::paths::{
    branch_storage_key, harness_authoritative_artifact_path, harness_state_path,
};
use featureforge::workflow::manifest::manifest_path;
use files_support::write_file;
use prebuilt_support::write_canonical_prebuilt_layout;
use process_support::{
    WORKSPACE_RUNTIME_LIVE_STATE_TEST_ALLOW_ENV, assert_workspace_runtime_uses_temp_state, run,
};
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

fn assert_release_readiness_required_inputs(surface: &Value) {
    assert!(
        surface.get("recommended_public_command_argv").is_none(),
        "release-readiness routes require external inputs and must not emit executable argv: {surface}"
    );
    assert!(
        surface["recommended_command"].is_null(),
        "release-readiness routes should expose typed inputs instead of a placeholder recommended_command: {surface}"
    );
    assert_eq!(
        surface["required_inputs"],
        json!([
            {
                "kind": "enum",
                "name": "result",
                "values": ["ready", "blocked"]
            },
            {
                "kind": "path",
                "must_exist": true,
                "name": "summary_file"
            }
        ]),
        "release-readiness route should expose typed missing inputs: {surface}"
    );
}

fn assert_qa_required_inputs(surface: &Value) {
    assert!(
        surface.get("recommended_public_command_argv").is_none(),
        "QA routes require external inputs and must not emit executable argv: {surface}"
    );
    assert!(
        surface["recommended_command"].is_null(),
        "QA routes should expose typed inputs instead of a placeholder recommended_command: {surface}"
    );
    assert_eq!(
        surface["required_inputs"],
        json!([
            {
                "kind": "enum",
                "name": "result",
                "values": ["pass", "fail"]
            },
            {
                "kind": "path",
                "must_exist": true,
                "name": "summary_file"
            }
        ]),
        "QA route should expose typed missing inputs: {surface}"
    );
}

fn assert_task_closure_required_inputs(surface: &Value, task: u32) {
    assert!(
        surface.get("recommended_public_command_argv").is_none(),
        "task-closure routes require review/verification inputs and must not emit executable argv: {surface}"
    );
    assert!(
        surface["recommended_command"].is_null(),
        "task-closure routes should expose typed inputs instead of a placeholder recommended_command: {surface}"
    );
    let task_target = surface["recording_context"]["task_number"]
        .as_u64()
        .or_else(|| surface["blocking_task"].as_u64());
    assert_eq!(
        task_target,
        Some(u64::from(task)),
        "task-closure route should keep the task in structured route metadata: {surface}"
    );
    assert_eq!(
        surface["required_inputs"],
        json!([
            {
                "kind": "enum",
                "name": "review_result",
                "values": ["pass", "fail"]
            },
            {
                "kind": "path",
                "must_exist": true,
                "name": "review_summary_file"
            },
            {
                "kind": "enum",
                "name": "verification_result",
                "values": ["pass", "fail", "not-run"]
            },
            {
                "kind": "path",
                "must_exist": true,
                "name": "verification_summary_file",
                "required_when": "verification_result!=not-run"
            }
        ]),
        "task-closure route should expose typed missing review/verification inputs: {surface}"
    );
}

fn record_task_closure_with_fixture_inputs_real_cli(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    task: u32,
    label: &str,
) -> Value {
    let safe_label: String = label
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let review_summary_path = repo.join(format!("task-{task}-{safe_label}-review-summary.md"));
    let verification_summary_path =
        repo.join(format!("task-{task}-{safe_label}-verification-summary.md"));
    write_file(
        &review_summary_path,
        &format!("Task {task} fixture independent review passed for {label}.\n"),
    );
    write_file(
        &verification_summary_path,
        &format!("Task {task} fixture verification passed for {label}.\n"),
    );
    let task_arg = task.to_string();

    run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            task_arg.as_str(),
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("task closure review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("task closure verification summary path should be utf-8"),
        ],
        label,
    )
}

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
    assert_eq!(route["plan_path"], plan_rel, "json: {route}");
    assert_task_closure_required_inputs(route, task);
}

fn assert_repair_review_state_route(route: &Value, plan_rel: &str, context: &str) {
    assert_eq!(
        route["next_action"], "repair review state / reenter execution",
        "{context}: route should require public repair-review-state: {route}"
    );
    assert_eq!(
        route["recommended_public_command_argv"],
        json!([
            "featureforge",
            "plan",
            "execution",
            "repair-review-state",
            "--plan",
            plan_rel
        ]),
        "{context}: route should expose exact public repair-review-state argv: {route}"
    );
    if route
        .get("execution_command_context")
        .is_some_and(|value| !value.is_null())
    {
        assert_eq!(
            route["execution_command_context"]["command_kind"], "repair-review-state",
            "{context}: non-null command context should match repair-review-state: {route}"
        );
    }
}

fn assert_no_worktree_lease_reason(route: &Value, context: &str) {
    let route_text = serde_json::to_string(route)
        .unwrap_or_else(|error| panic!("{context} should serialize for reason scan: {error}"));
    assert!(
        !route_text.contains("worktree_lease_"),
        "{context} should not retain a worktree lease blocker after public repair: {route_text}"
    );
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
        &["--dispatch", "-id"],
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

fn assert_no_stale_dispatch_public_replay_hidden_terms(route: &Value, context: &str) {
    let route_text = serde_json::to_string(route)
        .unwrap_or_else(|error| panic!("{context} should serialize for hidden-term scan: {error}"));
    let hidden_command_tokens = [
        &["record", "-review-dispatch"][..],
        &["gate", "-review"],
        &["rebuild", "-evidence"],
        &["--dispatch", "-id"],
    ];
    for hidden in hidden_command_tokens {
        assert!(
            !contains_hidden_parts(&route_text, hidden),
            "{context} must not expose hidden stale-dispatch replay term `{}` in {route_text}",
            hidden.join("")
        );
    }
}

fn assert_no_worktree_lease_manual_receipt_repair_terms(route: &Value, context: &str) {
    let route_text = serde_json::to_string(route).unwrap_or_else(|error| {
        panic!("{context} should serialize for worktree lease public-flow scan: {error}")
    });
    let route_text_lower = route_text.to_ascii_lowercase();
    let hidden_command_tokens = [
        &["record", "-review-dispatch"][..],
        &["gate", "-review"],
        &["gate", "-finish"],
        &["rebuild", "-evidence"],
    ];
    for forbidden in hidden_command_tokens {
        assert!(
            !contains_hidden_parts(&route_text, forbidden),
            "{context} must not expose hidden helper `{}`: {route_text}",
            forbidden.join("")
        );
    }
    for forbidden in [
        "manual receipt repair",
        "manually repair receipt",
        "manually edit receipt",
        "repair receipt",
        "write receipt",
        "record receipt",
        "unit-review receipt",
        "authoritative unit-review",
        "runtime-owned receipt",
        "receipt artifact",
        "receipt-repair",
    ] {
        assert!(
            !route_text_lower.contains(forbidden),
            "{context} must not expose public worktree-lease receipt wording `{forbidden}`: {route_text}"
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
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
}

fn install_ready_artifacts(repo: &Path) {
    install_full_contract_ready_artifacts(repo);
}

fn write_current_pass_plan_fidelity_review_artifact(
    repo: &Path,
    artifact_rel: &str,
    plan_rel: &str,
    spec_rel: &str,
) {
    let artifact_path = repo.join(artifact_rel);
    let plan = parse_plan_file(repo.join(plan_rel)).expect("plan fixture should parse");
    let spec = parse_spec_file(repo.join(spec_rel)).expect("spec fixture should parse");
    let plan_fingerprint = sha256_hex(&fs::read(repo.join(plan_rel)).expect("plan should read"));
    let spec_fingerprint = sha256_hex(&fs::read(repo.join(spec_rel)).expect("spec should read"));
    let verified_requirement_ids = spec
        .requirements
        .iter()
        .map(|requirement| requirement.id.clone())
        .collect::<Vec<_>>();

    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent).expect("plan-fidelity artifact parent should be creatable");
    }
    fs::write(
        artifact_path,
        format!(
            "## Plan Fidelity Review Summary\n\n**Review Stage:** featureforge:plan-fidelity-review\n**Review Verdict:** pass\n**Reviewed Plan:** `{plan_rel}`\n**Reviewed Plan Revision:** {}\n**Reviewed Plan Fingerprint:** {plan_fingerprint}\n**Reviewed Spec:** `{spec_rel}`\n**Reviewed Spec Revision:** {}\n**Reviewed Spec Fingerprint:** {spec_fingerprint}\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** fixture-plan-fidelity-reviewer\n**Distinct From Stages:** featureforge:writing-plans, featureforge:plan-eng-review\n**Verified Surfaces:** {}\n**Verified Requirement IDs:** {}\n",
            plan.plan_revision,
            spec.spec_revision,
            PLAN_FIDELITY_REQUIRED_SURFACES.join(", "),
            verified_requirement_ids.join(", "),
        ),
    )
    .expect("plan-fidelity review artifact should write");
}

fn write_current_pass_plan_fidelity_review_artifact_for_plan(repo: &Path, plan_rel: &str) {
    let plan = parse_plan_file(repo.join(plan_rel)).expect("plan fixture should parse");
    let spec_rel = plan.source_spec_path.clone();
    let plan_stem = Path::new(plan_rel)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("plan");
    let artifact_rel = format!(".featureforge/reviews/{plan_stem}-plan-fidelity.md");
    write_current_pass_plan_fidelity_review_artifact(repo, &artifact_rel, plan_rel, &spec_rel);
}

fn run_featureforge(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    assert_workspace_runtime_uses_temp_state(Some(repo), Some(state_dir), None, false, context);
    public_featureforge_cli::run_featureforge_real_cli(
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
    assert_workspace_runtime_uses_temp_state(Some(repo), Some(state_dir), None, false, context);
    public_featureforge_cli::run_featureforge_real_cli(
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
    let allow_live_state = extra_env
        .iter()
        .any(|(key, value)| *key == WORKSPACE_RUNTIME_LIVE_STATE_TEST_ALLOW_ENV && *value == "1");
    assert_workspace_runtime_uses_temp_state(
        Some(repo),
        Some(state_dir),
        None,
        allow_live_state,
        context,
    );
    public_featureforge_cli::run_featureforge_with_env_control_real_cli(
        Some(repo),
        Some(state_dir),
        None,
        &[],
        extra_env,
        args,
        context,
    )
}

fn run_featureforge_live_state_with_env(
    repo: &Path,
    home_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    context: &str,
) -> Output {
    let mut merged_env = Vec::with_capacity(extra_env.len() + 1);
    merged_env.extend_from_slice(extra_env);
    merged_env.push((WORKSPACE_RUNTIME_LIVE_STATE_TEST_ALLOW_ENV, "1"));
    assert_workspace_runtime_uses_temp_state(Some(repo), None, Some(home_dir), true, context);
    public_featureforge_cli::run_featureforge_with_env_control_real_cli(
        Some(repo),
        None,
        Some(home_dir),
        &["FEATUREFORGE_STATE_DIR"],
        &merged_env,
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

fn parse_json_from_output_trailing_line(output: &Output, context: &str) -> Value {
    let payload = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    if let Ok(value) = serde_json::from_slice(payload) {
        return value;
    }
    let payload_text = String::from_utf8_lossy(payload);
    let Some(line) = payload_text
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
    else {
        panic!(
            "{context} should emit json output in stdout/stderr\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    serde_json::from_str(line).unwrap_or_else(|error| {
        panic!(
            "{context} should emit parseable trailing-line json: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn run_featureforge_json_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let output = run_featureforge_real_cli(repo, state_dir, args, context);
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
    let output = run_featureforge_real_cli(repo, state_dir, &full_args, context);
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

fn run_plan_execution_json_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let mut full_args = Vec::with_capacity(args.len() + 2);
    full_args.extend(["plan", "execution"]);
    full_args.extend_from_slice(args);
    let output = run_featureforge_real_cli(repo, state_dir, &full_args, context);
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

fn recommended_public_argv(surface: &Value, context: &str) -> Vec<String> {
    let argv = surface["recommended_public_command_argv"]
        .as_array()
        .unwrap_or_else(|| {
            panic!("{context} should expose recommended_public_command_argv: {surface}")
        })
        .iter()
        .map(|part| {
            part.as_str()
                .unwrap_or_else(|| panic!("{context} argv entries should be strings: {surface}"))
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        argv.first().map(String::as_str),
        Some("featureforge"),
        "{context} argv should be rooted at the shipped public binary marker: {argv:?}"
    );
    argv
}

fn run_recommended_public_argv_json(
    repo: &Path,
    state_dir: &Path,
    surface: &Value,
    context: &str,
) -> Value {
    let argv = recommended_public_argv(surface, context);
    let args = argv.iter().skip(1).map(String::as_str).collect::<Vec<_>>();
    run_featureforge_with_env_json(repo, state_dir, &args, &[], context)
}

fn materialize_state_dir_projections(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    context: &str,
) -> Value {
    let materialized = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &["materialize-projections", "--plan", plan_rel],
        context,
    );
    assert_eq!(materialized["action"], Value::from("materialized"));
    assert_eq!(materialized["runtime_truth_changed"], Value::Bool(false));
    materialized
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
    let output = run_featureforge_real_cli(repo, state_dir, &full_args, context);
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
    run_featureforge_real_cli(
        repo,
        state_dir,
        &["plan", "execution"]
            .into_iter()
            .chain(command_args.iter().copied())
            .collect::<Vec<_>>(),
        context,
    )
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

fn commit_object_fingerprint(repo: &Path, commit_sha: &str) -> String {
    let repository =
        discover_repository(repo).expect("commit fingerprint helper should discover repo");
    let object_id = gix::hash::ObjectId::from_hex(commit_sha.as_bytes())
        .expect("commit fingerprint helper should parse commit object id");
    let commit = repository
        .find_commit(object_id)
        .expect("commit fingerprint helper should load commit object");
    sha256_hex(commit.data.as_slice())
}

fn canonical_worktree_lease_fingerprint(lease: &Value) -> String {
    let mut lease = lease.clone();
    lease
        .as_object_mut()
        .expect("worktree lease fixture should be a JSON object")
        .remove("lease_fingerprint");
    sha256_hex(
        &serde_json::to_vec(&lease)
            .expect("worktree lease fixture should serialize for fingerprint"),
    )
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
    materialize_state_dir_projections(
        repo,
        state_dir,
        plan_rel,
        "materialize evidence projection before deriving task completion lineage",
    );
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
    let mut status = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "plan execution status for shell-smoke review artifact fixture",
    );
    if status["last_strategy_checkpoint_fingerprint"]
        .as_str()
        .is_none()
    {
        let materialized = run_plan_execution_json(
            repo,
            state_dir,
            &["materialize-projections", "--plan", plan_rel],
            "materialize state-dir projections for shell-smoke review artifact fixture",
        );
        assert_eq!(materialized["runtime_truth_changed"], Value::Bool(false));
        status = run_plan_execution_json(
            repo,
            state_dir,
            &["status", "--plan", plan_rel],
            "plan execution status after shell-smoke review artifact fixture materialization",
        );
    }
    let strategy_checkpoint_fingerprint =
        if let Some(fingerprint) = status["last_strategy_checkpoint_fingerprint"].as_str() {
            fingerprint.to_owned()
        } else {
            let fingerprint = sha256_hex(
                format!(
                    "shell-smoke-review-artifact-fixture:{branch}:{plan_rel}:{}",
                    current_head_sha(repo)
                )
                .as_bytes(),
            );
            update_authoritative_harness_state(
                repo,
                state_dir,
                &[(
                    "last_strategy_checkpoint_fingerprint",
                    Value::String(fingerprint.clone()),
                )],
            );
            fingerprint
        };
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
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
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
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
}

fn complete_workflow_fixture_execution(repo: &Path, state_dir: &Path, plan_rel: &str) {
    complete_workflow_fixture_execution_with_qa_requirement(repo, state_dir, plan_rel, None, false);
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
        write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
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
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
}

fn update_authoritative_harness_state(repo: &Path, state_dir: &Path, updates: &[(&str, Value)]) {
    let state_path = harness_state_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &current_branch_name(repo),
    );
    let mut payload: Value = authoritative_harness_state_for_update(&state_path);
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

fn authoritative_harness_state_for_update(state_path: &Path) -> Value {
    if let Some(payload) = reduced_authoritative_harness_state_for_path(state_path) {
        return payload;
    }
    if state_path.is_file() {
        return serde_json::from_str(
            &fs::read_to_string(state_path)
                .expect("authoritative shell-smoke harness state should be readable"),
        )
        .expect("authoritative shell-smoke harness state should remain valid json");
    }
    Value::Object(serde_json::Map::new())
}

fn reduced_authoritative_harness_state_for_path(state_path: &Path) -> Option<Value> {
    featureforge::execution::event_log::load_reduced_authoritative_state_for_tests(state_path)
        .unwrap_or_else(|error| {
            panic!(
                "event-authoritative shell-smoke harness state should be reducible for {}: {}",
                state_path.display(),
                error.message
            )
        })
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
    authoritative_harness_state_for_update(&state_path)
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

fn current_worktree_lease_execution_context_key(
    execution_run_id: &str,
    execution_unit_id: &str,
    source_plan_path: &str,
    source_plan_revision: u32,
    authoritative_integration_branch: &str,
    reviewed_checkpoint_commit_sha: &str,
) -> String {
    sha256_hex(
        format!(
            "run={execution_run_id}\nunit={execution_unit_id}\nplan={source_plan_path}\nplan_revision={source_plan_revision}\nbranch={authoritative_integration_branch}\nreviewed_checkpoint={reviewed_checkpoint_commit_sha}\n"
        )
        .as_bytes(),
    )
}

fn approved_unit_contract_fingerprint_for_review(
    active_contract_fingerprint: &str,
    approved_task_packet_fingerprint: &str,
    execution_unit_id: &str,
) -> String {
    sha256_hex(
        format!(
            "approved-unit-contract:{active_contract_fingerprint}:{approved_task_packet_fingerprint}:{execution_unit_id}"
        )
        .as_bytes(),
    )
}

fn ensure_authoritative_active_contract_for_current_execution(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
) -> (String, String) {
    let mut authoritative_state = authoritative_harness_state(repo, state_dir);
    if let (Some(active_contract_path), Some(active_contract_fingerprint)) = (
        authoritative_state["active_contract_path"].as_str(),
        authoritative_state["active_contract_fingerprint"].as_str(),
    ) {
        return (
            active_contract_path.to_owned(),
            active_contract_fingerprint.to_owned(),
        );
    }

    let context = semantic_execution_context(repo, state_dir, plan_rel);
    let packet_fingerprints = context
        .evidence
        .attempts
        .iter()
        .filter(|attempt| attempt.status == "Completed")
        .filter_map(|attempt| attempt.packet_fingerprint.as_deref())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert!(
        !packet_fingerprints.is_empty(),
        "active contract fixture should have completed packet fingerprints"
    );
    let packet_lines = packet_fingerprints
        .iter()
        .map(|fingerprint| format!("- `{fingerprint}`"))
        .collect::<Vec<_>>()
        .join("\n");
    let covered_step_lines = context
        .evidence
        .attempts
        .iter()
        .filter(|attempt| attempt.status == "Completed")
        .map(|attempt| {
            format!(
                "- Task {} Step {}",
                attempt.task_number, attempt.step_number
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let requirement_lines = context
        .plan_document
        .tasks
        .iter()
        .flat_map(|task| task.spec_coverage.iter().map(String::as_str))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|requirement| format!("- {requirement}"))
        .collect::<Vec<_>>()
        .join("\n");
    let source_plan_fingerprint = hash_contract_plan(&context.plan_source);
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let contract_template = format!(
        r#"# Execution Contract

**Contract Version:** 1
**Authoritative Sequence:** 9000
**Source Plan Path:** `{plan_rel}`
**Source Plan Revision:** {}
**Source Plan Fingerprint:** `{source_plan_fingerprint}`
**Source Spec Path:** `{}`
**Source Spec Revision:** {}
**Source Spec Fingerprint:** `{source_spec_fingerprint}`
**Source Task Packet Fingerprints:**
{packet_lines}
**Chunk ID:** chunk-1
**Chunking Strategy:** single_chunk
**Covered Steps:**
{covered_step_lines}
**Requirement IDs:**
{requirement_lines}
**Criteria:**
### Criterion 1
**Criterion ID:** criterion-1
**Title:** Preserve active approved-plan scope
**Description:** Contract fixture stays within the approved plan scope.
**Requirement IDs:**
{requirement_lines}
**Covered Steps:**
{covered_step_lines}
**Verifier Types:**
- spec_compliance
**Threshold:** all
**Notes:** Fixture criterion for worktree lease routing validation.

**Non Goals:**
- none

**Verifiers:**
- spec_compliance

**Evidence Requirements:**
[]

**Retry Budget:** 1
**Pivot Threshold:** 1
**Reset Policy:** none
**Generated By:** featureforge:executing-plans
**Generated At:** 2026-03-27T12:00:00Z
**Contract Fingerprint:** __CONTRACT_FINGERPRINT__
"#,
        context.plan_document.plan_revision,
        context.plan_document.source_spec_path,
        context.plan_document.source_spec_revision,
    );
    let active_contract_fingerprint = sha256_hex(
        contract_template
            .replace("__CONTRACT_FINGERPRINT__", "")
            .as_bytes(),
    );
    let active_contract_source =
        contract_template.replace("__CONTRACT_FINGERPRINT__", &active_contract_fingerprint);
    let active_contract_file = format!("contract-{active_contract_fingerprint}.md");
    let active_contract_path = harness_authoritative_artifact_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &current_branch_name(repo),
        &active_contract_file,
    );
    write_file(&active_contract_path, &active_contract_source);
    authoritative_state
        .as_object_mut()
        .expect("authoritative state should remain an object")
        .extend([
            (
                String::from("active_contract_path"),
                Value::from(active_contract_file.clone()),
            ),
            (
                String::from("active_contract_fingerprint"),
                Value::from(active_contract_fingerprint.clone()),
            ),
            (
                String::from("required_evaluator_kinds"),
                json!(["spec_compliance"]),
            ),
            (String::from("completed_evaluator_kinds"), json!([])),
            (String::from("pending_evaluator_kinds"), json!([])),
            (
                String::from("aggregate_evaluation_state"),
                Value::from("pending"),
            ),
        ]);
    write_authoritative_harness_state(repo, state_dir, &authoritative_state);
    (active_contract_file, active_contract_fingerprint)
}

fn task_packet_fingerprint_for_step(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    task: u32,
    step: u32,
) -> String {
    let context = semantic_execution_context(repo, state_dir, plan_rel);
    let task_definition_identity = task_definition_identity_for_task(&context, task)
        .expect("worktree lease fixture should compute task packet identity")
        .expect("worktree lease fixture task should exist for packet identity");
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let payload = format!(
        "plan_path={plan_path}\nplan_revision={plan_revision}\ntask_definition_identity={task_definition_identity}\nsource_spec_path={source_spec_path}\nsource_spec_revision={source_spec_revision}\nsource_spec_fingerprint={source_spec_fingerprint}\ntask_number={task}\nstep_number={step}\n",
        plan_path = context.plan_rel,
        plan_revision = context.plan_document.plan_revision,
        source_spec_path = context.plan_document.source_spec_path,
        source_spec_revision = context.plan_document.source_spec_revision,
    );
    sha256_hex(payload.as_bytes())
}

fn write_authoritative_active_contract_for_steps(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    covered_steps: &[(u32, u32)],
) -> (String, String) {
    let context = semantic_execution_context(repo, state_dir, plan_rel);
    let packet_fingerprints = covered_steps
        .iter()
        .map(|(task, step)| {
            task_packet_fingerprint_for_step(repo, state_dir, plan_rel, *task, *step)
        })
        .collect::<Vec<_>>();
    let packet_lines = packet_fingerprints
        .iter()
        .map(|fingerprint| format!("- `{fingerprint}`"))
        .collect::<Vec<_>>()
        .join("\n");
    let covered_step_lines = covered_steps
        .iter()
        .map(|(task, step)| format!("- Task {task} Step {step}"))
        .collect::<Vec<_>>()
        .join("\n");
    let requirement_lines = context
        .plan_document
        .tasks
        .iter()
        .flat_map(|task| task.spec_coverage.iter().map(String::as_str))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|requirement| format!("- {requirement}"))
        .collect::<Vec<_>>()
        .join("\n");
    let source_plan_fingerprint = hash_contract_plan(&context.plan_source);
    let source_spec_fingerprint = sha256_hex(context.source_spec_source.as_bytes());
    let contract_template = format!(
        r#"# Execution Contract

**Contract Version:** 1
**Authoritative Sequence:** 9001
**Source Plan Path:** `{plan_rel}`
**Source Plan Revision:** {}
**Source Plan Fingerprint:** `{source_plan_fingerprint}`
**Source Spec Path:** `{}`
**Source Spec Revision:** {}
**Source Spec Fingerprint:** `{source_spec_fingerprint}`
**Source Task Packet Fingerprints:**
{packet_lines}
**Chunk ID:** chunk-1
**Chunking Strategy:** single_chunk
**Covered Steps:**
{covered_step_lines}
**Requirement IDs:**
{requirement_lines}
**Criteria:**
### Criterion 1
**Criterion ID:** criterion-1
**Title:** Preserve active approved-plan scope
**Description:** Contract fixture stays within the approved plan scope.
**Requirement IDs:**
{requirement_lines}
**Covered Steps:**
{covered_step_lines}
**Verifier Types:**
- spec_compliance
**Threshold:** all
**Notes:** Fixture criterion for task-scoped worktree lease routing validation.

**Non Goals:**
- none

**Verifiers:**
- spec_compliance

**Evidence Requirements:**
[]

**Retry Budget:** 1
**Pivot Threshold:** 1
**Reset Policy:** none
**Generated By:** featureforge:executing-plans
**Generated At:** 2026-03-27T12:00:00Z
**Contract Fingerprint:** __CONTRACT_FINGERPRINT__
"#,
        context.plan_document.plan_revision,
        context.plan_document.source_spec_path,
        context.plan_document.source_spec_revision,
    );
    let active_contract_fingerprint = sha256_hex(
        contract_template
            .replace("__CONTRACT_FINGERPRINT__", "")
            .as_bytes(),
    );
    let active_contract_source =
        contract_template.replace("__CONTRACT_FINGERPRINT__", &active_contract_fingerprint);
    let active_contract_file = format!("contract-{active_contract_fingerprint}.md");
    let active_contract_path = harness_authoritative_artifact_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &current_branch_name(repo),
        &active_contract_file,
    );
    write_file(&active_contract_path, &active_contract_source);
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            (
                "active_contract_path",
                Value::from(active_contract_file.clone()),
            ),
            (
                "active_contract_fingerprint",
                Value::from(active_contract_fingerprint.clone()),
            ),
            ("required_evaluator_kinds", json!(["spec_compliance"])),
            ("completed_evaluator_kinds", json!([])),
            ("pending_evaluator_kinds", json!([])),
            ("aggregate_evaluation_state", Value::from("pending")),
        ],
    );
    (active_contract_file, active_contract_fingerprint)
}

fn seed_cleaned_worktree_lease_missing_review_binding(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    tamper_lease_proof: bool,
) -> String {
    seed_worktree_lease_missing_review_binding(
        repo,
        state_dir,
        plan_rel,
        WorktreeLeaseState::Cleaned,
        "cleaned",
        tamper_lease_proof,
    )
}

#[derive(Clone)]
struct SeededWorktreeLeaseMissingReviewBinding {
    lease_fingerprint: String,
    binding: WorktreeLeaseBindingSnapshot,
}

fn seed_open_worktree_lease_missing_review_binding(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
) -> String {
    seed_worktree_lease_missing_review_binding(
        repo,
        state_dir,
        plan_rel,
        WorktreeLeaseState::Open,
        "open",
        false,
    )
}

fn seed_worktree_lease_missing_review_binding(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    lease_state: WorktreeLeaseState,
    cleanup_state: &str,
    tamper_lease_proof: bool,
) -> String {
    let (run_identity, chunk_id, seeded) = write_worktree_lease_missing_review_binding(
        repo,
        state_dir,
        plan_rel,
        WorktreeLeaseFixtureInput {
            task: 1,
            step: 1,
            execution_unit_id: "unit-public-lease-review-binding",
            worktree_label: "public-lease-review-binding",
            lease_state,
            cleanup_state,
            tamper_lease_proof,
        },
    );
    let runtime = discover_execution_runtime(
        repo,
        state_dir,
        "missing worktree lease review binding fixture",
    );
    persist_active_worktree_lease_index(
        &runtime,
        run_identity,
        chunk_id,
        vec![seeded.lease_fingerprint.clone()],
        vec![seeded.binding],
    )
    .expect("worktree lease fixture should persist missing review binding index");
    seeded.lease_fingerprint
}

struct WorktreeLeaseFixtureInput<'a> {
    task: u32,
    step: u32,
    execution_unit_id: &'a str,
    worktree_label: &'a str,
    lease_state: WorktreeLeaseState,
    cleanup_state: &'a str,
    tamper_lease_proof: bool,
}

fn write_worktree_lease_missing_review_binding(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    input: WorktreeLeaseFixtureInput<'_>,
) -> (
    RunIdentitySnapshot,
    ChunkId,
    SeededWorktreeLeaseMissingReviewBinding,
) {
    let status = run_plan_execution_json_real_cli(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "status before seeding missing worktree lease review binding",
    );
    let execution_run_id = status["execution_run_id"]
        .as_str()
        .expect("worktree lease fixture status should expose execution_run_id")
        .to_owned();
    let plan_revision = status["plan_revision"]
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .expect("worktree lease fixture status should expose numeric plan_revision");
    let (active_contract_file, active_contract_fingerprint) =
        ensure_authoritative_active_contract_for_current_execution(repo, state_dir, plan_rel);
    let authoritative_state = authoritative_harness_state(repo, state_dir);
    let chunk_id = authoritative_state["chunk_id"]
        .as_str()
        .or_else(|| status["chunk_id"].as_str())
        .expect("worktree lease fixture should expose chunk_id")
        .to_owned();
    let active_contract_path = harness_authoritative_artifact_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &current_branch_name(repo),
        &active_contract_file,
    );
    let active_contract = read_execution_contract(&active_contract_path)
        .expect("worktree lease fixture active contract should parse");
    assert_eq!(
        active_contract.contract_fingerprint, active_contract_fingerprint,
        "worktree lease fixture should bind the active contract fingerprint"
    );
    let approved_task_packet_fingerprint =
        task_packet_fingerprint_for_step(repo, state_dir, plan_rel, input.task, input.step);
    assert!(
        active_contract
            .source_task_packet_fingerprints
            .iter()
            .any(|fingerprint| fingerprint == &approved_task_packet_fingerprint),
        "worktree lease fixture active contract should bind task {} step {} packet {}",
        input.task,
        input.step,
        approved_task_packet_fingerprint
    );

    let current_head = current_head_sha(repo);
    let baseline_worktree_fingerprint = sha256_hex(
        format!(
            "worktree-lease-review-binding-baseline:{execution_run_id}:{}:{current_head}",
            input.execution_unit_id
        )
        .as_bytes(),
    );
    update_authoritative_harness_state(
        repo,
        state_dir,
        &[
            (
                "repo_state_baseline_head_sha",
                Value::from(current_head.clone()),
            ),
            (
                "repo_state_baseline_worktree_fingerprint",
                Value::from(baseline_worktree_fingerprint.clone()),
            ),
        ],
    );

    let terminal_lease = matches!(
        input.lease_state,
        WorktreeLeaseState::Reconciled | WorktreeLeaseState::Cleaned
    );
    let reviewed_checkpoint_commit_sha =
        (!matches!(input.lease_state, WorktreeLeaseState::Open)).then(|| current_head.clone());
    let reconcile_result_commit_sha = terminal_lease.then(|| current_head.clone());
    let reconcile_result_proof_fingerprint = terminal_lease.then(|| {
        if input.tamper_lease_proof {
            sha256_hex(b"tampered-worktree-lease-proof")
        } else {
            commit_object_fingerprint(repo, &current_head)
        }
    });
    let execution_context_key = current_worktree_lease_execution_context_key(
        &execution_run_id,
        input.execution_unit_id,
        plan_rel,
        plan_revision,
        &current_branch_name(repo),
        reviewed_checkpoint_commit_sha.as_deref().unwrap_or("open"),
    );
    let mut lease_payload = json!({
        "lease_version": 1,
        "authoritative_sequence": 10_000,
        "execution_run_id": execution_run_id,
        "execution_context_key": execution_context_key,
        "source_plan_path": plan_rel,
        "source_plan_revision": plan_revision,
        "execution_unit_id": input.execution_unit_id,
        "source_branch": current_branch_name(repo),
        "authoritative_integration_branch": current_branch_name(repo),
        "worktree_path": state_dir.join("worktrees").join(input.worktree_label).display().to_string(),
        "repo_state_baseline_head_sha": current_head,
        "repo_state_baseline_worktree_fingerprint": baseline_worktree_fingerprint,
        "lease_state": input.lease_state,
        "cleanup_state": input.cleanup_state,
        "reviewed_checkpoint_commit_sha": reviewed_checkpoint_commit_sha,
        "reconcile_result_commit_sha": reconcile_result_commit_sha,
        "reconcile_result_proof_fingerprint": reconcile_result_proof_fingerprint,
        "reconcile_mode": "identity_preserving",
        "generated_by": "featureforge:executing-plans",
        "generated_at": "2026-03-27T12:00:00Z",
        "lease_fingerprint": "",
    });
    let lease_fingerprint = canonical_worktree_lease_fingerprint(&lease_payload);
    lease_payload
        .as_object_mut()
        .expect("worktree lease fixture should remain an object")
        .insert(
            String::from("lease_fingerprint"),
            Value::from(lease_fingerprint.clone()),
        );
    let lease: WorktreeLease =
        serde_json::from_value(lease_payload).expect("worktree lease fixture should deserialize");
    let runtime = discover_execution_runtime(
        repo,
        state_dir,
        "missing worktree lease review binding fixture",
    );
    let lease_path = write_authoritative_worktree_lease_artifact(&runtime, &lease)
        .expect("worktree lease fixture should publish authoritative lease");
    (
        RunIdentitySnapshot {
            execution_run_id: ExecutionRunId::new(execution_run_id.clone()),
            source_plan_path: plan_rel.to_owned(),
            source_plan_revision: plan_revision,
        },
        ChunkId::new(chunk_id),
        SeededWorktreeLeaseMissingReviewBinding {
            lease_fingerprint: lease_fingerprint.clone(),
            binding: WorktreeLeaseBindingSnapshot {
                execution_run_id,
                lease_fingerprint: lease_fingerprint.clone(),
                lease_artifact_path: lease_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .expect("worktree lease artifact should have utf-8 file name")
                    .to_owned(),
                execution_context_key: Some(lease.execution_context_key.clone()),
                approved_task_packet_fingerprint: Some(approved_task_packet_fingerprint.clone()),
                approved_unit_contract_fingerprint: Some(
                    approved_unit_contract_fingerprint_for_review(
                        &active_contract_fingerprint,
                        &approved_task_packet_fingerprint,
                        input.execution_unit_id,
                    ),
                ),
                reconcile_result_proof_fingerprint: lease
                    .reconcile_result_proof_fingerprint
                    .clone(),
                reviewed_checkpoint_commit_sha: lease.reviewed_checkpoint_commit_sha.clone(),
                reconcile_result_commit_sha: lease.reconcile_result_commit_sha.clone(),
                reconcile_mode: Some(String::from("identity_preserving")),
                review_receipt_fingerprint: None,
                review_receipt_artifact_path: None,
            },
        },
    )
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
    let darwin_runtime = "#!/usr/bin/env bash\nprintf 'darwin runtime\\n'\n";
    write_canonical_prebuilt_layout(repo, "1.0.0", darwin_runtime, "windows runtime\n");
    write_repo_file(repo, "bin/featureforge", darwin_runtime);
    make_executable(&repo.join("bin/featureforge"));
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
fn workspace_runtime_blocks_live_repair_review_state() {
    let repo = repo_root();
    let home_dir = TempDir::new().expect("home tempdir should exist");

    let output = run_featureforge_live_state_with_env(
        &repo,
        home_dir.path(),
        &[
            "plan",
            "execution",
            "repair-review-state",
            "--plan",
            "docs/featureforge/plans/workspace-runtime-live-mutation-guard.md",
        ],
        &[],
        "workspace runtime live repair-review-state guard",
    );
    assert!(
        !output.status.success(),
        "workspace runtime live repair-review-state should fail closed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let failure = parse_json_from_output_trailing_line(
        &output,
        "workspace runtime live repair-review-state guard",
    );
    assert_eq!(
        failure["error_class"],
        Value::from("workspace_runtime_live_mutation_blocked")
    );
    let message = failure["message"]
        .as_str()
        .expect("guard failure should expose a string message");
    assert!(
        message.contains("blocked_command: plan execution repair-review-state"),
        "guard should report the blocked command, got {failure}"
    );
    assert!(
        message.contains("binary_path:"),
        "guard should report runtime binary path provenance, got {failure}"
    );
    assert!(
        message.contains("installed_binary_path:"),
        "guard should report installed runtime path provenance, got {failure}"
    );
    assert!(
        message.contains("state_dir:"),
        "guard should report live state dir provenance, got {failure}"
    );
    assert!(
        message.contains("rerun through `~/.featureforge/install/bin/featureforge`"),
        "guard should include installed-runtime remediation, got {failure}"
    );
}

#[test]
fn workspace_runtime_blocks_live_close_current_task() {
    let repo = repo_root();
    let home_dir = TempDir::new().expect("home tempdir should exist");
    let summary_dir = TempDir::new().expect("summary tempdir should exist");
    let review_summary = summary_dir.path().join("task-review.md");
    let verification_summary = summary_dir.path().join("task-verification.md");
    write_file(&review_summary, "Task review summary.\n");
    write_file(&verification_summary, "Task verification summary.\n");

    let output = run_featureforge_live_state_with_env(
        &repo,
        home_dir.path(),
        &[
            "plan",
            "execution",
            "close-current-task",
            "--plan",
            "docs/featureforge/plans/workspace-runtime-live-mutation-guard.md",
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary
                .to_str()
                .expect("review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary
                .to_str()
                .expect("verification summary path should be utf-8"),
        ],
        &[],
        "workspace runtime live close-current-task guard",
    );
    assert!(
        !output.status.success(),
        "workspace runtime live close-current-task should fail closed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let failure = parse_json_from_output_trailing_line(
        &output,
        "workspace runtime live close-current-task guard",
    );
    assert_eq!(
        failure["error_class"],
        Value::from("workspace_runtime_live_mutation_blocked")
    );
    assert!(
        failure["message"].as_str().is_some_and(
            |message| message.contains("blocked_command: plan execution close-current-task")
        ),
        "guard should report close-current-task as blocked command, got {failure}"
    );
}

#[test]
fn workspace_runtime_blocks_live_persisted_task_packet_cache() {
    let repo = repo_root();
    let home_dir = TempDir::new().expect("home tempdir should exist");

    let output = run_featureforge_live_state_with_env(
        &repo,
        home_dir.path(),
        &[
            "plan",
            "contract",
            "build-task-packet",
            "--plan",
            "docs/featureforge/plans/workspace-runtime-live-mutation-guard.md",
            "--task",
            "1",
            "--persist",
            "yes",
        ],
        &[],
        "workspace runtime live persisted task-packet guard",
    );
    assert!(
        !output.status.success(),
        "workspace runtime live persisted task-packet cache should fail closed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let failure = parse_json_from_output_trailing_line(
        &output,
        "workspace runtime live persisted task-packet guard",
    );
    assert_eq!(
        failure["error_class"],
        Value::from("workspace_runtime_live_mutation_blocked")
    );
    assert!(
        failure["message"].as_str().is_some_and(
            |message| message.contains("blocked_command: plan contract build-task-packet")
        ),
        "guard should report persisted task-packet cache as blocked command, got {failure}"
    );
}

#[test]
fn workspace_runtime_live_workflow_status_keeps_corrupt_manifest_read_only() {
    let repo = repo_root();
    let home_dir = TempDir::new().expect("home tempdir should exist");
    let state_dir = home_dir.path().join(".featureforge");
    let identity = discover_repo_identity(&repo).expect("repo identity should resolve");
    let workflow_manifest_path = manifest_path(&identity, &state_dir);
    let manifest_parent = workflow_manifest_path
        .parent()
        .expect("workflow manifest should have a parent directory");
    fs::create_dir_all(manifest_parent).expect("manifest parent should be creatable");
    fs::write(&workflow_manifest_path, "{ \"broken\": true\n")
        .expect("corrupt workflow manifest fixture should be writable");
    let before = fs::read(&workflow_manifest_path).expect("corrupt manifest should be readable");

    let output = run_featureforge_live_state_with_env(
        &repo,
        home_dir.path(),
        &["workflow", "status", "--json"],
        &[],
        "workspace runtime live workflow status read-only manifest inspection",
    );
    assert!(
        output.status.success(),
        "workflow status should remain a read-only diagnostic command under live state\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = parse_json_from_output_trailing_line(
        &output,
        "workspace runtime live workflow status read-only manifest inspection",
    );
    assert!(
        payload.get("error_class").is_none(),
        "workflow status should not emit a guard or manifest-repair failure: {payload}"
    );
    let after = fs::read(&workflow_manifest_path)
        .expect("workflow status should leave corrupt manifest in place");
    assert_eq!(
        after, before,
        "workflow status must not repair or rewrite live manifests from a workspace runtime"
    );
    let corrupt_backup_prefix = format!(
        "{}.corrupt-",
        workflow_manifest_path
            .file_name()
            .and_then(OsStr::to_str)
            .expect("workflow manifest filename should be utf-8")
    );
    let backups = fs::read_dir(manifest_parent)
        .expect("manifest parent should stay readable")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|file_name| file_name.starts_with(&corrupt_backup_prefix))
        .collect::<Vec<_>>();
    assert!(
        backups.is_empty(),
        "workflow status must not create corrupt-manifest backups for live read-only inspection: {backups:?}"
    );
}

#[test]
fn workspace_runtime_allows_fixture_repair_review_state_with_temp_state() {
    let (repo_dir, state_dir) = init_repo("workspace-runtime-temp-repair-allow");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let output = run_featureforge_real_cli(
        repo,
        state,
        &[
            "plan",
            "execution",
            "repair-review-state",
            "--plan",
            WORKFLOW_FIXTURE_PLAN_REL,
        ],
        "workspace runtime temp-state repair-review-state guard bypass",
    );
    let payload = parse_json_from_output_trailing_line(
        &output,
        "workspace runtime temp-state repair-review-state guard bypass",
    );
    if output.status.success() {
        assert!(
            payload.get("error_class").is_none(),
            "successful repair-review-state output should not be a guard failure: {payload}"
        );
    } else {
        assert_ne!(
            payload["error_class"],
            Value::from("workspace_runtime_live_mutation_blocked"),
            "temp-state workspace runtime should not trip live-mutation guard: {payload}"
        );
    }
}

#[test]
fn workspace_runtime_allows_fixture_task_packet_cache_with_temp_state() {
    let (repo_dir, state_dir) = init_repo("workspace-runtime-temp-task-packet-allow");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);

    let output = run_featureforge_real_cli(
        repo,
        state,
        &[
            "plan",
            "contract",
            "build-task-packet",
            "--plan",
            WORKFLOW_FIXTURE_PLAN_REL,
            "--task",
            "1",
            "--format",
            "json",
            "--persist",
            "yes",
        ],
        "workspace runtime temp-state persisted task-packet guard bypass",
    );
    let payload = parse_json_from_output_trailing_line(
        &output,
        "workspace runtime temp-state persisted task-packet guard bypass",
    );
    assert!(
        output.status.success(),
        "temp-state persisted task-packet should not trip live-mutation guard: {payload}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(payload["persisted"], Value::Bool(true));
    assert!(
        payload
            .get("workspace_runtime_live_mutation_warning")
            .is_none(),
        "temp-state task-packet cache should not emit a live-mutation override warning: {payload}"
    );
}

#[test]
fn workspace_runtime_live_mutation_override_emits_warning_in_json_and_text() {
    let repo = repo_root();
    let home_dir = TempDir::new().expect("home tempdir should exist");

    let output = run_featureforge_live_state_with_env(
        &repo,
        home_dir.path(),
        &[
            "plan",
            "execution",
            "repair-review-state",
            "--plan",
            "docs/featureforge/plans/workspace-runtime-live-mutation-override.md",
        ],
        &[("FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION", "1")],
        "workspace runtime live mutation override warning",
    );
    assert!(
        !output.status.success(),
        "override test expects downstream command validation failure after bypassing guard\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let failure = parse_json_from_output_trailing_line(
        &output,
        "workspace runtime live mutation override warning",
    );
    assert_ne!(
        failure["error_class"],
        Value::from("workspace_runtime_live_mutation_blocked"),
        "override should bypass the live-mutation block and continue to normal validation, got {failure}"
    );
    let warning = failure["workspace_runtime_live_mutation_warning"]
        .as_str()
        .expect("override should add workspace runtime warning to json output");
    assert!(
        warning.contains("FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION=1"),
        "override warning should mention explicit override env, got {warning}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("workspace_runtime_live_mutation_override_active"),
        "override warning should also be visible in stderr text, got stderr:\n{stderr}"
    );

    let packet_output = run_featureforge_live_state_with_env(
        &repo,
        home_dir.path(),
        &[
            "plan",
            "contract",
            "build-task-packet",
            "--plan",
            "docs/featureforge/plans/workspace-runtime-live-mutation-override.md",
            "--task",
            "1",
            "--persist",
            "yes",
        ],
        &[("FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION", "1")],
        "workspace runtime live persisted task-packet override warning",
    );
    assert!(
        !packet_output.status.success(),
        "task-packet override test expects downstream command validation failure after bypassing guard\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&packet_output.stdout),
        String::from_utf8_lossy(&packet_output.stderr)
    );
    let packet_failure = parse_json_from_output_trailing_line(
        &packet_output,
        "workspace runtime live persisted task-packet override warning",
    );
    assert_ne!(
        packet_failure["error_class"],
        Value::from("workspace_runtime_live_mutation_blocked"),
        "task-packet override should bypass the live-mutation block and continue to normal validation, got {packet_failure}"
    );
    assert!(
        packet_failure["workspace_runtime_live_mutation_warning"]
            .as_str()
            .is_some_and(
                |warning| warning.contains("FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION=1")
            ),
        "task-packet override should add workspace runtime warning to json output: {packet_failure}"
    );
    let packet_stderr = String::from_utf8_lossy(&packet_output.stderr);
    assert!(
        packet_stderr.contains("workspace_runtime_live_mutation_override_active"),
        "task-packet override warning should also be visible in stderr text, got stderr:\n{packet_stderr}"
    );
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
fn workflow_operator_text_labels_display_commands_as_non_authoritative() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-text-command-summary");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_ready_artifacts(repo);

    let output = run_featureforge(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md",
        ],
        "workflow operator text",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Display command summary:"),
        "workflow/operator text should label display command text as a summary:\n{stdout}"
    );
    assert!(
        stdout.contains("Use JSON recommended_public_command_argv for execution"),
        "workflow/operator text should point agents at JSON argv authority:\n{stdout}"
    );
    assert!(
        !stdout.contains("Recommended command:"),
        "workflow/operator text must not present display strings as executable authority:\n{stdout}"
    );
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

    let doctor_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "doctor", "--json"],
        &[],
        "workflow doctor public ready-plan route without explicit plan",
    );
    assert_eq!(doctor_json["phase"], concat!("execution_pre", "flight"));
    assert_eq!(
        doctor_json["plan_path"],
        Value::from(String::from(plan_rel))
    );
    assert!(
        doctor_json["runtime_provenance"].is_object(),
        "workflow doctor --json should expose runtime provenance, got {doctor_json}"
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
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);

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

    assert_eq!(operator_json["phase"], "execution_preflight");
    assert_eq!(operator_json["phase_detail"], "execution_in_progress");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "continue execution");
    assert_eq!(operator_json["state_kind"], "actionable_public_command");
    assert_eq!(
        operator_json["recommended_public_command_argv"],
        json!([
            "featureforge",
            "plan",
            "execution",
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--expect-execution-fingerprint",
            status_json["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint for exact begin argv"),
        ])
    );
    assert_eq!(
        operator_json["execution_command_context"]["command_kind"],
        "begin"
    );
    assert_eq!(status_json["phase_detail"], operator_json["phase_detail"]);
    assert_eq!(status_json["next_action"], operator_json["next_action"]);
    assert_eq!(status_json["state_kind"], operator_json["state_kind"]);
    assert_eq!(
        status_json["execution_command_context"],
        operator_json["execution_command_context"]
    );
}

#[test]
fn plan_execution_status_compiled_cli_smoke() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-status-compiled-cli");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "plan-execution-status-compiled-cli");

    let status = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status via compiled CLI smoke",
    );

    assert_eq!(
        status["phase_detail"],
        Value::from("execution_preflight_required")
    );
    assert_eq!(status["next_action"], Value::from("continue execution"));
    assert!(
        status["recommended_public_command_argv"]
            .as_array()
            .is_some_and(|argv| argv.iter().any(|arg| arg == plan_rel)),
        "compiled CLI status should expose typed argv containing the selected plan path, got {status:?}"
    );
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
    write_current_pass_plan_fidelity_review_artifact_for_plan(repo, plan_rel);
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
        let doctor_json = run_featureforge_json_real_cli(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            "workflow_shell_smoke late-stage doctor JSON",
        );
        let doctor_text_output = run_featureforge_real_cli(
            repo,
            state,
            &["workflow", "doctor"],
            "workflow_shell_smoke late-stage doctor text",
        );
        assert!(
            doctor_text_output.status.success(),
            "workflow doctor text should succeed for case {}\nstdout:\n{}\nstderr:\n{}",
            case.name,
            String::from_utf8_lossy(&doctor_text_output.stdout),
            String::from_utf8_lossy(&doctor_text_output.stderr)
        );
        let doctor_text = String::from_utf8_lossy(&doctor_text_output.stdout);

        assert_eq!(doctor_json["phase"], case.expected_phase);
        assert_eq!(doctor_json["next_action"], case.expected_next_action);
        assert!(doctor_text.contains(&format!("Phase: {}", case.expected_phase)));
        assert!(doctor_text.contains(&format!("Next action: {}", case.expected_next_action)));

        let next_step = doctor_text
            .lines()
            .find_map(|line| line.strip_prefix("Next: "))
            .unwrap_or_else(|| {
                panic!(
                    "workflow doctor text should expose Next line for case {}",
                    case.name
                )
            });
        assert_eq!(
            doctor_json["next_step"],
            Value::from(next_step),
            "workflow doctor json should mirror the same Next step from workflow doctor text for case {}",
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
    assert_task_closure_required_inputs(&status, 1);
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
    assert_eq!(
        operator_json["recommended_public_command_argv"],
        json!([
            "featureforge",
            "plan",
            "execution",
            "advance-late-stage",
            "--plan",
            plan_rel
        ]),
        "finish-completion route should expose executable public argv: {operator_json}"
    );
    let finish_json = run_recommended_public_argv_json(
        repo,
        state,
        &operator_json,
        "recommended advance-late-stage finish-completion argv should execute",
    );
    assert_eq!(finish_json["action"], "completed", "json: {finish_json}");
    assert_eq!(
        finish_json["operation"], "validate_finish_completion",
        "json: {finish_json}"
    );
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
    assert_eq!(
        operator_json["recommended_public_command_argv"],
        json!([
            "featureforge",
            "plan",
            "execution",
            "advance-late-stage",
            "--plan",
            plan_rel
        ]),
        "finish-review route should expose executable public argv: {operator_json}"
    );
    let checkpoint_json = run_recommended_public_argv_json(
        repo,
        state,
        &operator_json,
        "recommended advance-late-stage finish-review argv should execute",
    );
    assert_eq!(
        checkpoint_json["action"], "recorded",
        "json: {checkpoint_json}"
    );
    assert_eq!(
        checkpoint_json["operation"], "record_finish_review_gate_checkpoint",
        "json: {checkpoint_json}"
    );

    let post_checkpoint_operator = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route to finish completion after public finish-review checkpoint",
    );
    assert_eq!(
        post_checkpoint_operator["phase_detail"], "finish_completion_gate_ready",
        "json: {post_checkpoint_operator}"
    );
    assert_eq!(
        post_checkpoint_operator["recommended_public_command_argv"],
        json!([
            "featureforge",
            "plan",
            "execution",
            "advance-late-stage",
            "--plan",
            plan_rel
        ]),
        "finish-completion route should remain executable after public checkpoint: {post_checkpoint_operator}"
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
    assert_task_closure_required_inputs(&repair_json, 1);
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
        Value::from("advance_late_stage"),
        "repair-review-state should preserve the executable public late-stage follow-up after restoring overlays"
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
        repair["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("advance-late-stage")),
        "repair-review-state should expose the executable public late-stage command after restoring overlays, got {repair}"
    );
    assert_eq!(
        repair["recommended_public_command_argv"],
        json!([
            "featureforge",
            "plan",
            "execution",
            "advance-late-stage",
            "--plan",
            plan_rel
        ]),
        "repair-review-state should expose executable public argv after restoring overlays, got {repair}"
    );
    assert!(
        repair.get("next_public_action").is_none() || repair["next_public_action"].is_null(),
        "repair-review-state does not project next_public_action directly; executable argv should be carried on recommended_public_command_argv, got {repair}"
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
        "advance-late-stage should bootstrap final-review dispatch and record the review outcome",
    );
    assert_eq!(review_json["action"], "recorded", "json: {review_json}");
    assert_eq!(
        review_json["operation"],
        Value::from("record_final_review_outcome"),
        "json: {review_json}"
    );
    assert!(
        review_json["dispatch_id"].as_str().is_some(),
        "json: {review_json}"
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
    assert_release_readiness_required_inputs(&operator_json);
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
    assert_release_readiness_required_inputs(&status_json);

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
    assert_release_readiness_required_inputs(&operator_json);
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
        Value::from(format!(
            "featureforge workflow operator --plan {plan_rel} --json"
        ))
    );
    assert_eq!(
        release_json["recommended_public_command_argv"],
        json!([
            "featureforge",
            "workflow",
            "operator",
            "--plan",
            plan_rel,
            "--json"
        ])
    );
    assert_eq!(
        release_json["rederive_via_workflow_operator"],
        Value::Bool(true)
    );
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
    assert_release_readiness_required_inputs(&operator_json);
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
    assert_qa_required_inputs(&operator_json);
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
    assert_qa_required_inputs(&operator_json);
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
    assert_qa_required_inputs(&status_json);
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
    assert_release_readiness_required_inputs(&operator_json);
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
fn removed_workflow_handoff_and_phase_commands_fail_at_cli_boundary() {
    let (repo_dir, state_dir) = init_repo("workflow-removed-handoff-phase-boundary");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    for args in [
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
    assert_task_closure_required_inputs(&operator_ready, 1);
    runtime_management_commands += 1; // close-current-task
    let close_json = record_task_closure_with_fixture_inputs_real_cli(
        repo,
        state,
        plan_rel,
        1,
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
fn public_task_closure_unblocks_missing_worktree_lease_review_binding() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("public-worktree-lease-missing-review-binding");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let closure_lease_fingerprint =
        seed_cleaned_worktree_lease_missing_review_binding(repo, state, plan_rel, false);

    let operator_ready = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "workflow operator for missing worktree lease review binding",
    );
    assert_task_closure_recording_route(&operator_ready, plan_rel, 1);
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &operator_ready,
        "workflow operator for missing worktree lease review binding",
    );

    let status_ready = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for missing worktree lease review binding",
    );
    assert_public_route_parity(&operator_ready, &status_ready, None);
    assert_eq!(status_ready["phase"], "task_closure_pending");
    assert_eq!(
        status_ready["phase_detail"], "task_closure_recording_ready",
        "json: {status_ready}"
    );
    assert_task_closure_required_inputs(&status_ready, 1);
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &status_ready,
        "plan execution status for missing worktree lease review binding",
    );

    let close_json = record_task_closure_with_fixture_inputs_real_cli(
        repo,
        state,
        plan_rel,
        1,
        "missing-worktree-lease-review-binding",
    );
    assert_eq!(
        close_json["action"],
        Value::from("recorded"),
        "public close-current-task should record and release the repairable missing lease review binding: lease={closure_lease_fingerprint}, output={close_json}"
    );
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &close_json,
        "close-current-task for missing worktree lease review binding",
    );

    let operator_after = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "workflow operator after missing worktree lease review binding closure",
    );
    assert_repair_review_state_route(
        &operator_after,
        plan_rel,
        "workflow operator after missing worktree lease review binding closure",
    );
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &operator_after,
        "workflow operator after missing worktree lease review binding closure",
    );

    let status_after = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status after missing worktree lease review binding closure",
    );
    assert_public_route_parity(&operator_after, &status_after, None);
    assert_repair_review_state_route(
        &status_after,
        plan_rel,
        "plan execution status after missing worktree lease review binding closure",
    );
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &status_after,
        "plan execution status after missing worktree lease review binding closure",
    );

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state releases already-current missing worktree lease review binding",
    );
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &repair_json,
        "repair-review-state releases already-current missing worktree lease review binding",
    );
    let operator_after_repair = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "workflow operator after repair-review-state releases already-current missing worktree lease review binding",
    );
    assert_no_worktree_lease_reason(
        &operator_after_repair,
        "workflow operator after repair-review-state releases already-current missing worktree lease review binding",
    );
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &operator_after_repair,
        "workflow operator after repair-review-state releases already-current missing worktree lease review binding",
    );
    assert_ne!(
        operator_after_repair["execution_command_context"]["command_kind"],
        Value::from("repair-review-state"),
        "repair-review-state should release the repairable lease binding instead of looping: lease={closure_lease_fingerprint}, route={operator_after_repair}"
    );
}

#[test]
fn public_worktree_lease_canonical_artifact_without_release_marker_fails_closed() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("public-worktree-lease-unindexed-canonical-artifact");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");

    let close_json = record_task_closure_with_fixture_inputs_real_cli(
        repo,
        state,
        plan_rel,
        1,
        "unindexed-worktree-lease-canonical-artifact",
    );
    assert_eq!(
        close_json["action"],
        Value::from("recorded"),
        "public close-current-task should record before unindexed canonical lease artifact scan: {close_json}"
    );
    let lease_fingerprint =
        seed_cleaned_worktree_lease_missing_review_binding(repo, state, plan_rel, false);
    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("active_worktree_lease_fingerprints", json!([])),
            ("active_worktree_lease_bindings", json!([])),
            ("released_worktree_lease_records", json!([])),
        ],
    );

    let operator_after = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "workflow operator with unindexed canonical worktree lease artifact",
    );
    let operator_text = serde_json::to_string(&operator_after)
        .expect("unindexed canonical lease operator output should serialize");
    assert!(
        operator_text.contains("worktree_lease_authoritative_binding_missing"),
        "canonical current-run lease artifact without an explicit runtime release marker must fail closed instead of being ignored: lease={lease_fingerprint}, route={operator_after}"
    );
    assert_repair_review_state_route(
        &operator_after,
        plan_rel,
        "workflow operator with unindexed canonical worktree lease artifact",
    );
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &operator_after,
        "workflow operator with unindexed canonical worktree lease artifact",
    );
}

#[test]
fn public_worktree_lease_repair_releases_only_matching_task_scope() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("public-worktree-lease-task-scoped-release");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    write_authoritative_active_contract_for_steps(repo, state, plan_rel, &[(1, 1), (2, 1)]);

    let (run_identity, chunk_id, task_one_lease) = write_worktree_lease_missing_review_binding(
        repo,
        state,
        plan_rel,
        WorktreeLeaseFixtureInput {
            task: 1,
            step: 1,
            execution_unit_id: "unit-task-one-lease-review-binding",
            worktree_label: "task-one-lease-review-binding",
            lease_state: WorktreeLeaseState::Cleaned,
            cleanup_state: "cleaned",
            tamper_lease_proof: false,
        },
    );
    let (_, _, task_two_lease) = write_worktree_lease_missing_review_binding(
        repo,
        state,
        plan_rel,
        WorktreeLeaseFixtureInput {
            task: 2,
            step: 1,
            execution_unit_id: "unit-task-two-lease-review-binding",
            worktree_label: "task-two-lease-review-binding",
            lease_state: WorktreeLeaseState::Cleaned,
            cleanup_state: "cleaned",
            tamper_lease_proof: false,
        },
    );
    let runtime = discover_execution_runtime(
        repo,
        state,
        "task-scoped missing worktree lease review binding fixture",
    );
    persist_active_worktree_lease_index(
        &runtime,
        run_identity,
        chunk_id,
        vec![
            task_one_lease.lease_fingerprint.clone(),
            task_two_lease.lease_fingerprint.clone(),
        ],
        vec![
            task_one_lease.binding.clone(),
            task_two_lease.binding.clone(),
        ],
    )
    .expect("task-scoped worktree lease fixture should persist both active bindings");

    let operator_ready = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "workflow operator before task-scoped worktree lease repair",
    );
    assert_task_closure_recording_route(&operator_ready, plan_rel, 1);

    let close_json = record_task_closure_with_fixture_inputs_real_cli(
        repo,
        state,
        plan_rel,
        1,
        "task-scoped-worktree-lease-release",
    );
    assert_eq!(
        close_json["action"],
        Value::from("recorded"),
        "public close-current-task should record task 1 before task-scoped lease repair: {close_json}"
    );
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state releases only task-scoped worktree lease binding",
    );
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &repair_json,
        "repair-review-state releases only task-scoped worktree lease binding",
    );
    let harness_state = authoritative_harness_state(repo, state);
    let active_fingerprints = harness_state["active_worktree_lease_fingerprints"]
        .as_array()
        .expect("task-scoped repair should leave an active lease index")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert!(
        !active_fingerprints.contains(&task_one_lease.lease_fingerprint.as_str()),
        "task 1 closure repair should release its own lease: state={harness_state}"
    );
    assert!(
        active_fingerprints.contains(&task_two_lease.lease_fingerprint.as_str()),
        "task 1 closure repair must not release task 2's lease: state={harness_state}"
    );
    let release_records = harness_state["released_worktree_lease_records"]
        .as_array()
        .expect("task-scoped repair should record explicit lease release markers");
    assert!(
        release_records.iter().any(|record| {
            record["lease_fingerprint"].as_str() == Some(task_one_lease.lease_fingerprint.as_str())
                && record["source_task"].as_u64() == Some(1)
        }),
        "task 1 lease release marker should identify the closing task: state={harness_state}"
    );
    assert!(
        release_records.iter().all(|record| {
            record["lease_fingerprint"].as_str() != Some(task_two_lease.lease_fingerprint.as_str())
        }),
        "task 2 lease must not receive a release marker from task 1 closure: state={harness_state}"
    );

    let operator_after = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "workflow operator after task-scoped worktree lease repair",
    );
    let operator_text = serde_json::to_string(&operator_after)
        .expect("task-scoped lease operator output should serialize");
    assert!(
        operator_text.contains("worktree_lease_review_receipt_missing"),
        "unrelated active task 2 lease should remain blocking after task 1 repair: route={operator_after}"
    );
    assert_repair_review_state_route(
        &operator_after,
        plan_rel,
        "workflow operator after task-scoped worktree lease repair",
    );
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &operator_after,
        "workflow operator after task-scoped worktree lease repair",
    );
}

#[test]
fn public_task_closure_does_not_bypass_unsafe_worktree_lease_state() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("public-worktree-lease-unsafe-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");

    let operator_ready = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "workflow operator before unsafe worktree lease state closure",
    );
    assert_task_closure_recording_route(&operator_ready, plan_rel, 1);
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &operator_ready,
        "workflow operator before unsafe worktree lease state closure",
    );

    let close_json = record_task_closure_with_fixture_inputs_real_cli(
        repo,
        state,
        plan_rel,
        1,
        "unsafe-worktree-lease-state",
    );
    assert_eq!(
        close_json["action"],
        Value::from("recorded"),
        "public close-current-task should record the task boundary before the unsafe lease is re-evaluated: {close_json}"
    );

    let lease_fingerprint =
        seed_cleaned_worktree_lease_missing_review_binding(repo, state, plan_rel, true);
    let operator_after = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "workflow operator after unsafe worktree lease state closure",
    );
    let operator_after_text = serde_json::to_string(&operator_after)
        .expect("unsafe lease operator output should serialize for reason scan");
    assert!(
        operator_after_text.contains("worktree_lease_identity_preserving_lease_proof_mismatch"),
        "unsafe lease proof must fail closed after public task closure instead of being covered by closure metadata: lease={lease_fingerprint}, route={operator_after}"
    );
    assert_repair_review_state_route(
        &operator_after,
        plan_rel,
        "workflow operator after unsafe worktree lease state closure",
    );
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &operator_after,
        "workflow operator after unsafe worktree lease state closure",
    );
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state with unsafe worktree lease state",
    );
    let repair_text = serde_json::to_string(&repair_json)
        .expect("unsafe lease repair output should serialize for reason scan");
    assert!(
        repair_text.contains("worktree_lease_identity_preserving_lease_proof_mismatch"),
        "repair-review-state must preserve the unsafe lease blocker instead of clearing it: lease={lease_fingerprint}, output={repair_json}"
    );
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &repair_json,
        "repair-review-state with unsafe worktree lease state",
    );
}

#[test]
fn public_worktree_lease_open_state_fails_closed_without_hidden_helper() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("public-worktree-lease-open-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");

    let close_json = record_task_closure_with_fixture_inputs_real_cli(
        repo,
        state,
        plan_rel,
        1,
        "open-worktree-lease-state",
    );
    assert_eq!(
        close_json["action"],
        Value::from("recorded"),
        "public close-current-task should record before open lease state is evaluated: {close_json}"
    );

    let lease_fingerprint = seed_open_worktree_lease_missing_review_binding(repo, state, plan_rel);
    let operator_after = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "workflow operator after open worktree lease state closure",
    );
    let operator_after_text = serde_json::to_string(&operator_after)
        .expect("open lease operator output should serialize for reason scan");
    assert!(
        operator_after_text.contains("worktree_lease_open"),
        "open lease state must fail closed as a public diagnostic: lease={lease_fingerprint}, route={operator_after}"
    );
    assert_repair_review_state_route(
        &operator_after,
        plan_rel,
        "workflow operator after open worktree lease state closure",
    );
    assert_no_worktree_lease_manual_receipt_repair_terms(
        &operator_after,
        "workflow operator after open worktree lease state closure",
    );
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
    assert_task_closure_required_inputs(&operator_ready, 1);

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
fn public_close_current_task_records_positive_closure_after_stale_dispatch_lineage_without_dispatch_id()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("public-close-current-task-stale-dispatch-refresh");
    let repo = repo_dir.path();
    let state = state_dir.path();
    prepare_missing_task_closure_baseline_close_fixture(repo, state, plan_rel, "main");

    let mut authoritative_state = authoritative_harness_state(repo, state);
    authoritative_state["strategy_review_dispatch_lineage"]["task-1"] = json!({
        "dispatch_id": "task-1-stale-dispatch",
        "semantic_reviewed_state_id": "git_tree:stale-reviewed-state",
        "reviewed_state_id": "git_tree:stale-reviewed-state",
        "task_completion_lineage_fingerprint": "0000000000000000000000000000000000000000000000000000000000000000"
    });
    write_authoritative_harness_state(repo, state, &authoritative_state);

    let review_summary_path = repo.join("public-stale-dispatch-review-summary.md");
    let verification_summary_path = repo.join("public-stale-dispatch-verification-summary.md");
    write_file(
        &review_summary_path,
        "Public stale-dispatch independent review passed.\n",
    );
    write_file(
        &verification_summary_path,
        "Public stale-dispatch verification passed.\n",
    );

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
        "public stale-dispatch close-current-task operator route",
    );
    assert_eq!(
        operator_ready["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "public stale-dispatch route should stay on task closure recording: {operator_ready}"
    );
    assert_eq!(
        operator_ready["next_action"],
        Value::from("close current task")
    );
    assert!(
        operator_ready["blocking_reason_codes"]
            .as_array()
            .is_none_or(|codes| {
                !codes
                    .iter()
                    .any(|code| code.as_str() == Some("prior_task_review_dispatch_stale"))
            }),
        "stale dispatch lineage must remain diagnostic-only for public routing: {operator_ready}"
    );
    assert_no_stale_dispatch_public_replay_hidden_terms(
        &operator_ready,
        "public stale-dispatch operator route",
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
                .expect("public stale-dispatch review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("public stale-dispatch verification summary path should be utf-8"),
        ],
        "public stale-dispatch close-current-task",
    );
    assert_eq!(
        close_json["action"],
        Value::from("recorded"),
        "public close-current-task should record after refreshing stale dispatch lineage without hidden dispatch id: {close_json}"
    );
    assert_eq!(
        close_json["dispatch_validation_action"],
        Value::from("validated")
    );
    assert_no_stale_dispatch_public_replay_hidden_terms(
        &close_json,
        "public stale-dispatch close-current-task output",
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    let current_record = &authoritative_state["current_task_closure_records"]["task-1"];
    assert_eq!(current_record["closure_status"], Value::from("current"));
    assert_eq!(current_record["review_result"], Value::from("pass"));
    assert_eq!(current_record["verification_result"], Value::from("pass"));
    assert_ne!(
        current_record["dispatch_id"],
        Value::from("task-1-stale-dispatch"),
        "close-current-task should bind the refreshed current dispatch id, not stale historical lineage"
    );

    let operator_after = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "public stale-dispatch close-current-task follow-up operator route",
    );
    assert_ne!(
        operator_after["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "same task should not reroute to task closure after public stale-dispatch close succeeds: {operator_after}"
    );
    if operator_after["phase_detail"] == "execution_reentry_required" {
        assert_ne!(
            operator_after["blocking_task"],
            Value::from(1_u64),
            "same task should not reroute to execution reentry after public stale-dispatch close succeeds: {operator_after}"
        );
    }
    assert_no_stale_dispatch_public_replay_hidden_terms(
        &operator_after,
        "public stale-dispatch follow-up operator route",
    );

    let status_after = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "public stale-dispatch close-current-task follow-up status route",
    );
    assert_ne!(
        status_after["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "status should not reroute the same task to task closure after public stale-dispatch close succeeds: {status_after}"
    );
    if status_after["phase_detail"] == "execution_reentry_required" {
        assert_ne!(
            status_after["blocking_task"],
            Value::from(1_u64),
            "status should not reroute the same task to execution reentry after public stale-dispatch close succeeds: {status_after}"
        );
    }
    assert_no_stale_dispatch_public_replay_hidden_terms(
        &status_after,
        "public stale-dispatch follow-up status route",
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
    assert_task_closure_required_inputs(&operator_ready, 1);

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
    materialize_state_dir_projections(
        repo,
        state,
        plan_rel,
        "FS-20 materialize state-dir projections before downstream reopen runtime-owned churn",
    );
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
    materialize_state_dir_projections(
        repo,
        state,
        plan_rel,
        "FS-20 materialize state-dir projections before late-stage runtime-owned churn",
    );
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
    assert_task_closure_required_inputs(&status_json, 1);
    assert_eq!(
        operator_json["recommended_command"], status_json["recommended_command"],
        "FS-21 operator and status should agree on missing-input command absence when resume is preempted"
    );
    assert_task_closure_required_inputs(&operator_json, 1);
}

#[test]
fn fs11_operator_and_begin_target_parity_after_rebase_resume() {
    let plan_rel = "docs/featureforge/plans/2026-04-02-runtime-fs11-shell-smoke.md";
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs11-shell-smoke");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_fs11_rebase_resume_parity_fixture(repo, state, plan_rel);

    let operator_json = run_featureforge_json_real_cli(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "FS-11 shell-smoke compiled-cli workflow operator",
    );
    if let Some(blocking_task) = operator_json["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-11 shell-smoke operator should target Task 2 as the earliest stale boundary after rebase/resume overlays: {operator_json:?}",
        );
    } else {
        assert_eq!(
            operator_json["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-11 shell-smoke operator should target Task 2 via execution command context when blocker metadata is projected as a concrete command: {operator_json:?}",
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
    let operator_recommended = operator_json["recommended_command"]
        .as_str()
        .expect("FS-11 shell-smoke operator should expose recommended command");
    assert_task_closure_required_inputs(&repair_json, 2);
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
        if operator_follow_up_json["phase_detail"].as_str() == Some("task_closure_recording_ready")
        {
            assert_task_closure_required_inputs(&operator_follow_up_json, 2);
        }
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
                operator_follow_up_json["blocking_scope"], operator_json["blocking_scope"],
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
    assert_task_closure_required_inputs(&repair_json, 2);
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
    assert_task_closure_required_inputs(&repair_json, 2);
    assert_runtime_management_budget("FS11-REBASE-RESUME-BUDGET", runtime_management_commands, 2);
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
    let authoritative_state_after_tamper = authoritative_harness_state(repo, state);
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
    let authoritative_state = authoritative_harness_state(repo, state);
    assert!(
        authoritative_state["current_open_step_state"].is_null(),
        "FS-13 shell-smoke routed follow-up should clear the stale parked open-step state before surfacing the Task 2 close-current-task command"
    );
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
        "featureforge plan execution advance-late-stage --plan {plan_rel} --result ready --summary-file {}",
        release_summary_path.display()
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
