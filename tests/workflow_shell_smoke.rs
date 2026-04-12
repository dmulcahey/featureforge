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
#[path = "support/workflow.rs"]
mod workflow_support;

use executable_support::make_executable;
use featureforge::execution::final_review::{
    parse_final_review_receipt, resolve_release_base_branch,
};
use featureforge::execution::state::{
    NO_REPO_FILES_MARKER, current_head_sha as runtime_current_head_sha,
    current_tracked_tree_sha as runtime_current_tracked_tree_sha,
};
use featureforge::git::discover_slug_identity;
use featureforge::paths::{
    branch_storage_key, harness_authoritative_artifact_path, harness_state_path,
};
use files_support::write_file;
use prebuilt_support::write_canonical_prebuilt_layout;
use process_support::run;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn copy_dir_recursive(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("destination directory should be creatable");
    for entry in fs::read_dir(source).expect("source directory should be readable") {
        let entry = entry.expect("source entry should be readable");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .expect("source entry type should be readable");
        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path);
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)
                .unwrap_or_else(|error| panic!("failed to copy {:?}: {error}", source_path));
        }
    }
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
**Task Outcome:** Core execution setup and validation are tracked with canonical execution-state evidence.
**Plan Constraints:**
- Preserve helper-owned execution-state invariants.
- Keep execution evidence grounded in repo-visible artifacts.
**Open Questions:** none

**Files:**
- Modify: `docs/example-output.md`
- Test: `cargo test --test workflow_shell_smoke`

- [ ] **Step 1: Prepare workspace for execution**
- [ ] **Step 2: Validate the generated output**

## Task 2: Repair flow

**Spec Coverage:** REQ-004, VERIFY-001
**Task Outcome:** Repair and handoff steps can reopen stale work without losing provenance.
**Plan Constraints:**
- Reuse the same approved plan and evidence path for repairs.
- Keep repair flows fail-closed on stale or malformed state.
**Open Questions:** none

**Files:**
- Modify: `docs/example-followup.md`
- Test: `cargo test --test workflow_shell_smoke`

- [ ] **Step 1: Repair an invalidated prior step**
- [ ] **Step 2: Finalize the execution handoff**
"#,
    );
}

fn close_two_task_fixture_task_1(repo: &Path, state_dir: &Path, plan_rel: &str) {
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
        "status before two-task shell-smoke fixture preflight",
    );
    let preflight = run_plan_execution_json(
        repo,
        state_dir,
        &["preflight", "--plan", plan_rel],
        "preflight for two-task shell-smoke fixture",
    );
    assert_eq!(preflight["allowed"], true);

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
                    "contract_identity": task_contract_identity(plan_rel, 1),
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
    featureforge_support::run_rust_featureforge(
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
    featureforge_support::run_rust_featureforge_with_env_control(
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
    let output = featureforge_support::run_rust_featureforge(
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

fn run_plan_execution_json_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let mut full_args = Vec::with_capacity(args.len() + 2);
    full_args.extend(["plan", "execution"]);
    full_args.extend_from_slice(args);
    let output = featureforge_support::run_rust_featureforge_real_cli(
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

#[test]
fn plan_execution_close_current_task_relative_summary_paths_preserve_real_cli_semantics() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, direct_state_dir) =
        init_repo("plan-execution-close-current-task-relative-summary-parity");
    let real_state_dir = TempDir::new().expect("real-cli parity state tempdir should exist");
    let repo = repo_dir.path();
    let direct_state = direct_state_dir.path();
    let real_state = real_state_dir.path();

    setup_task_boundary_blocked_case(repo, direct_state, plan_rel, "main");
    setup_task_boundary_blocked_case(repo, real_state, plan_rel, "main");

    let dispatch_direct = run_plan_execution_json(
        repo,
        direct_state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "direct record-review-dispatch for close-current-task relative summary parity",
    );
    let dispatch_real = run_plan_execution_json_real_cli(
        repo,
        real_state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "real-cli record-review-dispatch for close-current-task relative summary parity",
    );
    assert_eq!(dispatch_direct["allowed"], Value::Bool(true));
    assert_eq!(dispatch_real["allowed"], Value::Bool(true));

    let dispatch_id_direct = dispatch_direct["dispatch_id"]
        .as_str()
        .expect("direct dispatch should expose dispatch id")
        .to_owned();
    let dispatch_id_real = dispatch_real["dispatch_id"]
        .as_str()
        .expect("real-cli dispatch should expose dispatch id")
        .to_owned();

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
    let direct_close = run_plan_execution_json(
        repo,
        direct_state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id_direct,
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
            "--dispatch-id",
            &dispatch_id_real,
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

    let direct_rerun = run_plan_execution_json(
        repo,
        direct_state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id_direct,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_rel,
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_rel,
        ],
        "direct helper relative summary conflicting rerun should preserve real-cli semantics via fallback",
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
            "--dispatch-id",
            &dispatch_id_real,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_rel,
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_rel,
        ],
        "real-cli close-current-task relative summary conflicting rerun",
    );
    assert_eq!(direct_rerun["action"], Value::from("blocked"));
    assert_eq!(real_rerun["action"], Value::from("blocked"));
    assert_eq!(direct_rerun["closure_action"], Value::from("blocked"));
    assert_eq!(real_rerun["closure_action"], Value::from("blocked"));
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
    write_file(
        &path,
        &format!(
            "# Test Plan\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {head_sha}\n**Browser QA Required:** {browser_required}\n**Generated By:** featureforge:plan-eng-review\n**Generated At:** 2026-03-24T12:00:00Z\n\n## Affected Pages / Routes\n- none\n\n## Key Interactions\n- shell smoke parity fixtures\n\n## Edge Cases\n- downstream phase routing coverage\n\n## Critical Paths\n- downstream routing should stay harness-aware.\n",
            repo_slug(repo, state_dir)
        ),
    );
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
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T12:09:50Z\n\n## Summary\n- dedicated independent reviewer artifact fixture.\n",
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
            "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** {reviewer_artifact_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T12:10:00Z\n\n## Summary\n- shell smoke parity fixture.\n",
            reviewer_artifact_path.display(),
            repo_slug(repo, state_dir),
            current_head_sha(repo)
        ),
    );
}

fn write_branch_release_artifact(repo: &Path, state_dir: &Path, plan_rel: &str, base_branch: &str) {
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
    publish_authoritative_final_review_truth(repo, state_dir, &review_path);
    publish_authoritative_release_truth(repo, state_dir, &release_path);
}

fn prepare_preflight_acceptance_workspace(repo: &Path, branch_name: &str) {
    let mut checkout = Command::new("git");
    checkout
        .args(["checkout", "-B", branch_name])
        .current_dir(repo);
    run_checked(checkout, "git checkout preflight acceptance branch");
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
    let preflight = run_plan_execution_json(
        repo,
        state_dir,
        &["preflight", "--plan", plan_rel],
        "plan execution preflight for shell-smoke parity fixture",
    );
    assert_eq!(preflight["allowed"], true);
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
    write_file(
        &state_path,
        &serde_json::to_string(&payload)
            .expect("authoritative shell-smoke harness state should serialize"),
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

fn write_authoritative_harness_state(repo: &Path, state_dir: &Path, payload: &Value) {
    let state_path = harness_state_path(
        state_dir,
        &repo_slug(repo, state_dir),
        &current_branch_name(repo),
    );
    write_file(
        &state_path,
        &serde_json::to_string(payload)
            .expect("authoritative shell-smoke harness state should serialize"),
    );
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
        &[(state_fingerprint_field, Value::from(fingerprint))],
    );
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

fn deterministic_fixture_record_id(prefix: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    for part in parts {
        hasher.update(b"\n");
        hasher.update(part.as_bytes());
    }
    let digest = format!("{:x}", hasher.finalize());
    format!("{prefix}-{}", &digest[..16])
}

fn task_contract_identity(plan_rel: &str, task_number: u32) -> String {
    deterministic_fixture_record_id("task-contract", &[plan_rel, "1", &task_number.to_string()])
}

fn branch_contract_identity(
    plan_rel: &str,
    plan_revision: u32,
    repo: &Path,
    base_branch: &str,
    state_dir: &Path,
) -> String {
    deterministic_fixture_record_id(
        "branch-contract",
        &[
            plan_rel,
            &plan_revision.to_string(),
            &repo_slug(repo, state_dir),
            &current_branch_name(repo),
            base_branch,
        ],
    )
}

fn publish_authoritative_final_review_truth(repo: &Path, state_dir: &Path, review_path: &Path) {
    let branch = current_branch_name(repo);
    let review_source = fs::read_to_string(review_path)
        .expect("shell-smoke review artifact should be readable for authoritative publication");
    let review_fingerprint = sha256_hex(review_source.as_bytes());
    let branch_closure_id =
        authoritative_harness_state(repo, state_dir)["current_branch_closure_id"]
            .as_str()
            .unwrap_or("branch-release-closure")
            .to_owned();
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
            ("browser_qa_state", Value::from("not_required")),
            ("release_docs_state", Value::from("not_required")),
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
    let branch_closure_id =
        authoritative_harness_state(repo, state_dir)["current_branch_closure_id"]
            .as_str()
            .unwrap_or("branch-release-closure")
            .to_owned();
    let plan_rel = authoritative_harness_state(repo, state_dir)["source_plan_path"]
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

fn write_dispatched_branch_review_artifact(
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
    let gate_review = run_plan_execution_json(
        repo,
        state_dir,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution gate-review dispatch for shell-smoke review fixture",
    );
    assert_eq!(
        gate_review["allowed"],
        Value::Bool(true),
        "shell-smoke review fixture should prime a passing gate-review dispatch before minting a final-review artifact: {gate_review:?}"
    );
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
fn workflow_help_outside_repo_mentions_the_public_surfaces() {
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
    assert!(stdout.contains("help"));
}

#[test]
fn workflow_status_summary_matches_json_semantics_for_ready_plans() {
    let (repo_dir, state_dir) = init_repo("workflow-summary");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_ready_artifacts(repo);

    let json_output = run_featureforge(
        repo,
        state,
        &["workflow", "status", "--refresh"],
        "workflow status json",
    );
    let json_stdout = String::from_utf8_lossy(&json_output.stdout);
    assert!(json_stdout.contains("\"schema_version\":3"));
    assert!(json_stdout.contains("\"status\":\"implementation_ready\""));
    assert!(json_stdout.contains("\"next_skill\":\"\""));

    let summary_output = run_featureforge(
        repo,
        state,
        &["workflow", "status", "--refresh", "--summary"],
        "workflow status summary",
    );
    let summary_stdout = String::from_utf8_lossy(&summary_output.stdout);
    assert!(!summary_stdout.contains("{\"status\""));
    assert!(summary_stdout.contains("status=implementation_ready"));
    assert!(summary_stdout.contains("next=execution_preflight"));
    assert!(summary_stdout.contains(
        "spec=docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"
    ));
    assert!(
        summary_stdout
            .contains("plan=docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md")
    );
}

#[test]
fn workflow_operator_commands_work_for_ready_plan() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-commands");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);

    let next_output = run_featureforge(repo, state, &["workflow", "next"], "workflow next");
    let next_stdout = String::from_utf8_lossy(&next_output.stdout);
    assert!(next_stdout.contains("Next safe step:"));
    assert!(next_stdout.contains(
        "Return to execution preflight for the approved plan: docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md"
    ));
    assert!(!next_stdout.contains("session-entry"));

    let artifacts_output = run_featureforge(
        repo,
        state,
        &["workflow", "artifacts"],
        "workflow artifacts",
    );
    let artifacts_stdout = String::from_utf8_lossy(&artifacts_output.stdout);
    assert!(artifacts_stdout.contains("Workflow artifacts"));
    assert!(artifacts_stdout.contains(
        "Spec: docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"
    ));
    assert!(
        artifacts_stdout
            .contains("Plan: docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md")
    );

    let explain_output =
        run_featureforge(repo, state, &["workflow", "explain"], "workflow explain");
    let explain_stdout = String::from_utf8_lossy(&explain_output.stdout);
    assert!(explain_stdout.contains("Why FeatureForge chose this state"));
    assert!(explain_stdout.contains("What to do:"));
    assert!(!explain_stdout.contains("session-entry"));
}

#[test]
fn workflow_operator_routes_active_execution_to_exact_step_command() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-execution-command-context");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "workflow-operator-execution-command-context");

    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status for workflow operator active execution routing",
    );
    let preflight = run_plan_execution_json(
        repo,
        state,
        &["preflight", "--plan", plan_rel],
        "preflight for workflow operator active execution routing",
    );
    assert_eq!(preflight["allowed"], true);
    let begin = run_plan_execution_json(
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

    assert_eq!(operator_json["phase"], "executing");
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

    let status_json = run_plan_execution_json(
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
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "execution reentry required");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution begin --plan {plan_rel} --task 1 --step 1 --execution-mode featureforge:executing-plans --expect-execution-fingerprint {}",
            status_json["execution_fingerprint"].as_str().expect(
                "status should expose execution fingerprint for marker-free operator command"
            )
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
fn workflow_operator_routes_blocked_execution_to_resume_same_step() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-blocked-step-command-context");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "workflow-operator-blocked-step-command-context");

    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status for workflow operator blocked execution routing",
    );
    let preflight = run_plan_execution_json(
        repo,
        state,
        &["preflight", "--plan", plan_rel],
        "preflight for workflow operator blocked execution routing",
    );
    assert_eq!(preflight["allowed"], true);
    let begin = run_plan_execution_json(
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
    let blocked = run_plan_execution_json(
        repo,
        state,
        &[
            "note",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--state",
            "blocked",
            "--message",
            "Waiting for dependency",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for blocked note"),
        ],
        "note blocked should establish a blocked step for workflow operator routing",
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator json for blocked execution routing",
    );

    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_in_progress");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "continue execution");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution begin --plan {plan_rel} --task 1 --step 1 --expect-execution-fingerprint {}",
            blocked["execution_fingerprint"]
                .as_str()
                .expect("blocked note should expose execution fingerprint for operator command")
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

    let resumed = run_plan_execution_json(
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
                .expect("blocked note should expose execution fingerprint for resume begin"),
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
                        "contract_identity": task_contract_identity(plan_rel, 1),
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
                        "contract_identity": task_contract_identity(plan_rel, 1),
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
    write_dispatched_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    write_branch_release_artifact(repo, state_dir, plan_rel, base_branch);
    set_current_branch_closure(repo, state_dir, "branch-release-closure");
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
    write_dispatched_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    write_branch_release_artifact(repo, state_dir, plan_rel, base_branch);
}

fn setup_ready_for_finish_case_with_qa_requirement(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
    qa_requirement: Option<&str>,
    remove_qa_requirement: bool,
) {
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
    write_dispatched_branch_review_artifact(repo, state_dir, plan_rel, base_branch);
    write_branch_release_artifact(repo, state_dir, plan_rel, base_branch);
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
        return;
    }
    setup_task_boundary_blocked_case_slow(repo, state_dir, plan_rel, base_branch);
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
    let preflight = run_plan_execution_json(
        repo,
        state_dir,
        &["preflight", "--plan", plan_rel],
        "preflight for task-boundary blocked shell-smoke fixture execution",
    );
    assert_eq!(preflight["allowed"], true);

    let begin_task1_step1 = run_plan_execution_json(
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
    let complete_task1_step1 = run_plan_execution_json(
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
    let begin_task1_step2 = run_plan_execution_json(
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
    run_plan_execution_json(
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
**Task Outcome:** Task 1 execution reaches a boundary gate before Task 2 starts.
**Plan Constraints:**
- Keep fixture inputs deterministic.
**Open Questions:** none

**Files:**
- Modify: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Prepare workflow fixture output**
- [ ] **Step 2: Validate workflow fixture output**

## Task 2: Follow-on flow

**Spec Coverage:** VERIFY-001
**Task Outcome:** Task 2 should remain blocked until Task 1 closure requirements are met.
**Plan Constraints:**
- Preserve deterministic task-boundary diagnostics.
**Open Questions:** none

**Files:**
- Modify: `tests/workflow_shell_smoke.rs`

- [ ] **Step 1: Start the follow-on task**
"#
}

#[test]
fn workflow_phase_text_and_json_surfaces_match_harness_downstream_freshness() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
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
            expected_next_action: "run finish review gate",
            setup: setup_ready_for_finish_case,
        },
        LateStageCase {
            name: "task-boundary-blocked",
            expected_phase: "task_closure_pending",
            expected_next_action: "dispatch review",
            setup: setup_task_boundary_blocked_case,
        },
    ];

    for case in cases {
        let (repo_dir, state_dir) = init_repo(&format!("workflow-phase-next-parity-{}", case.name));
        let repo = repo_dir.path();
        let state = state_dir.path();
        let base_branch = expected_release_base_branch(repo);
        (case.setup)(repo, state, plan_rel, &base_branch);

        let phase_json = run_featureforge_with_env_json(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[],
            "workflow phase json for shell-smoke late-stage parity",
        );
        let doctor_json = run_featureforge_with_env_json(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[],
            "workflow doctor json for shell-smoke late-stage parity",
        );
        let phase_text_output = run_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase"],
            &[],
            "workflow phase text for shell-smoke late-stage parity",
        );
        assert!(
            phase_text_output.status.success(),
            "workflow phase text should succeed for case {}, got {:?}",
            case.name,
            phase_text_output.status
        );
        let phase_text = String::from_utf8_lossy(&phase_text_output.stdout);
        let next_output = run_featureforge_with_env(
            repo,
            state,
            &["workflow", "next"],
            &[],
            "workflow next text for shell-smoke late-stage parity",
        );
        assert!(
            next_output.status.success(),
            "workflow next text should succeed for case {}, got {:?}",
            case.name,
            next_output.status
        );
        let next_text = String::from_utf8_lossy(&next_output.stdout);

        assert_eq!(phase_json["phase"], case.expected_phase);
        assert_eq!(phase_json["next_action"], case.expected_next_action);
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
            phase_json["next_step"],
            Value::from(next_step),
            "workflow phase json should mirror the same Next step from workflow phase text for case {}",
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

    assert_eq!(operator_json["phase"], "task_closure_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "task_review_dispatch_required"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "dispatch review");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-review-dispatch --plan {plan_rel} --scope task --task 1"
        ))
    );
}

#[test]
fn plan_execution_record_review_dispatch_prefers_task_boundary_target_over_interrupted_note_state()
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

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch should honor the prior-task boundary target even when Task 2 has an interrupted note-state",
    );
    assert_eq!(dispatch["allowed"], true);
    assert_eq!(dispatch["action"], "recorded");
}

#[test]
fn plan_execution_status_fails_closed_when_clean_execution_has_no_exact_command() {
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

    let output = run_featureforge(
        repo,
        state,
        &["plan", "execution", "status", "--plan", plan_rel],
        "plan execution status for ambiguous clean execution state",
    );
    assert!(
        !output.status.success(),
        "status should fail closed when clean execution has no exact command\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let failure: Value =
        serde_json::from_slice(&output.stderr).expect("status failure should emit json on stderr");
    assert_eq!(failure["error_class"], "MalformedExecutionState");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|value| value.contains("exact execution command")),
        "failure should mention the missing exact execution command: {failure}"
    );
}

#[test]
fn workflow_operator_fails_closed_when_clean_execution_has_no_exact_command() {
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

    let output = run_featureforge(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        "workflow operator for ambiguous clean execution state",
    );
    assert!(
        !output.status.success(),
        "workflow operator should fail closed when clean execution has no exact command\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let failure: Value = serde_json::from_slice(&output.stderr)
        .expect("workflow operator failure should emit json on stderr");
    assert_eq!(failure["error_class"], "MalformedExecutionState");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|value| value.contains("exact execution command")),
        "failure should mention the missing exact execution command: {failure}"
    );
}

#[test]
fn explain_review_state_falls_back_to_generic_operator_guidance_when_clean_execution_has_no_exact_command()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("explain-review-state-no-exact-command");
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

    let explain = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should stay best-effort when exact execution-command derivation fails",
    );
    assert_eq!(explain["next_action"], "requery workflow operator");
    assert_eq!(
        explain["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
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
    assert_eq!(operator_json["next_action"], "run finish completion gate");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution gate-finish --plan {plan_rel}"
        ))
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
        "workflow operator json without persisted gate-review checkpoint",
    );

    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
    assert_eq!(operator_json["phase_detail"], "finish_review_gate_ready");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["finish_review_gate_pass_branch_closure_id"],
        Value::Null
    );
    assert_eq!(operator_json["next_action"], "run finish review gate");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution gate-review --plan {plan_rel}"
        ))
    );
}

#[test]
fn plan_execution_gate_review_records_finish_review_gate_pass_checkpoint() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-gate-review-records-finish-checkpoint");
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

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should succeed and persist the finish-review gate pass checkpoint",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(true));

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after gate-review"),
    )
    .expect("authoritative state should remain valid json after gate-review");
    assert_eq!(
        authoritative_state_after["finish_review_gate_pass_branch_closure_id"],
        Value::from("branch-release-closure")
    );
}

#[test]
fn plan_execution_gate_review_records_finish_checkpoint_from_authoritative_current_branch_truth() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-gate-review-records-finish-checkpoint-from-authority");
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

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should still persist the finish-review gate pass checkpoint from authoritative current branch closure truth when overlay current-branch fields are missing",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(true));

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after gate-review"),
    )
    .expect("authoritative state should remain valid json after gate-review");
    assert_eq!(
        authoritative_state_after["finish_review_gate_pass_branch_closure_id"],
        Value::from("branch-release-closure")
    );
}

#[test]
fn plan_execution_gate_review_blocks_when_finish_checkpoint_is_already_current() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-gate-review-already-current-finish-checkpoint");
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

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should fail closed once the current branch closure already has a fresh finish-review gate checkpoint",
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
        Value::from(format!(
            "featureforge plan execution gate-finish --plan {plan_rel}"
        ))
    );
    assert_eq!(
        gate_review["finish_review_gate_pass_branch_closure_id"],
        Value::from("branch-release-closure")
    );

    let gate_review_real_cli = run_plan_execution_json_real_cli(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "real cli gate-review should agree once the finish-review gate checkpoint is already current",
    );
    assert_eq!(gate_review_real_cli, gate_review);
}

#[test]
fn plan_execution_explain_review_state_does_not_record_finish_review_gate_pass_checkpoint() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-explain-review-state-does-not-record-finish-checkpoint");
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

    let _ = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should stay read-only and not persist the finish-review gate pass checkpoint",
    );

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state_after: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should remain readable after explain-review-state"),
    )
    .expect("authoritative state should remain valid json after explain-review-state");
    assert!(
        authoritative_state_after["finish_review_gate_pass_branch_closure_id"].is_null(),
        "explain-review-state must not persist the finish-review gate pass checkpoint: {authoritative_state_after}",
    );
}

#[test]
fn workflow_operator_waits_for_task_review_result_after_dispatch() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-review-pending");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let dispatch = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "record-review-dispatch",
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
    assert_eq!(operator_json["phase_detail"], "task_review_result_pending");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["next_action"],
        "wait for external review result"
    );
    assert!(operator_json.get("recommended_command").is_none());
}

#[test]
fn workflow_operator_routes_task_review_result_ready_to_close_current_task() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-review-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
        "workflow operator json for task review result ready",
    );

    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
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
            "featureforge plan execution close-current-task --plan {plan_rel} --task 1 --dispatch-id {dispatch_id} --review-result pass|fail --review-summary-file <path> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]"
        ))
    );
}

#[test]
fn plan_execution_record_review_dispatch_exposes_dispatch_id() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-review-dispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");

    let dispatch_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch should expose dispatch contract fields",
    );

    assert_eq!(dispatch_json["allowed"], Value::Bool(true));
    assert_eq!(dispatch_json["action"], "recorded");
    assert_eq!(dispatch_json["scope"], "task");
    assert!(dispatch_json["dispatch_id"].as_str().is_some());

    let rerun_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch rerun should remain idempotent",
    );
    assert_eq!(rerun_json["allowed"], Value::Bool(true));
    assert_eq!(rerun_json["action"], "already_current");
    assert_eq!(rerun_json["dispatch_id"], dispatch_json["dispatch_id"]);

    let rerun_json_real_cli = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch real cli rerun should remain idempotent",
    );
    assert_eq!(rerun_json_real_cli, rerun_json);
}

#[test]
fn plan_execution_close_current_task_records_task_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for close-current-task fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let state_path = harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&state_path)
            .expect("task closure fixture authoritative state should read"),
    )
    .expect("task closure fixture authoritative state should remain valid json");
    let dispatch_id =
        authoritative_state["strategy_review_dispatch_lineage"]["task-1"]["dispatch_id"]
            .as_str()
            .expect("task closure fixture should expose dispatch_id")
            .to_owned();

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
            "--dispatch-id",
            &dispatch_id,
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
            "--dispatch-id",
            &dispatch_id,
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
            "--dispatch-id",
            &dispatch_id,
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
        "close-current-task conflicting rerun should fail closed",
    );
    assert_eq!(conflicting_json["action"], "blocked");
    assert_eq!(conflicting_json["closure_action"], "blocked");
}

#[test]
fn plan_execution_close_current_task_stale_dispatch_validation_happens_before_summary_validation() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-summary-requery");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for close-current-task summary ordering fixture",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("summary ordering fixture should expose dispatch id")
        .to_owned();
    append_tracked_repo_line(
        repo,
        "README.md",
        "tracked drift before close-current-task summary ordering regression coverage",
    );

    let missing_review_summary = repo.join("missing-close-current-task-review-summary.md");
    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
fn plan_execution_close_current_task_requires_fresh_reviewed_state_after_dispatch() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-stale-after-dispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
    let dispatch_id =
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

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
fn workflow_operator_routes_stale_task_review_dispatch_to_repair_review_state() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-stale-task-review-dispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "stale_unreviewed");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should let task-scope repair outrank a persisted branch reroute when current task-closure truth becomes invalid",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert_eq!(
        gate_review["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should let task-scope repair outrank a persisted branch reroute when current task-closure truth becomes invalid",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert_eq!(
        gate_finish["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
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
                    "contract_identity": task_contract_identity(plan_rel, 1),
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
                    "contract_identity": task_contract_identity(plan_rel, 1),
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
                    "contract_identity": task_contract_identity(plan_rel, 1),
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
        "repair-review-state should route completed-plan invalid current task-closure provenance back to execution reentry",
    );
    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(repair_json["required_follow_up"], "execution_reentry");
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
                    "contract_identity": task_contract_identity(plan_rel, 1),
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
                    "contract_identity": task_contract_identity(plan_rel, 2),
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
    assert!(blocking_records.iter().any(|record| {
        record["code"] == "prior_task_current_closure_reviewed_state_malformed"
            && record["scope_key"] == "task-2"
            && record["record_id"] == "task-2-current-closure"
            && record["required_follow_up"] == "repair_review_state"
    }));

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
fn plan_execution_close_current_task_requires_dispatch_reviewed_state_binding() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-close-current-task-missing-dispatch-reviewed-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for missing reviewed-state binding fixture",
    );
    let dispatch_id = dispatch["dispatch_id"]
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
    write_file(
        &state_path,
        &serde_json::to_string(&authoritative_state)
            .expect("missing reviewed-state binding fixture state should serialize"),
    );

    let review_summary_path = repo.join("task-1-failed-review-summary.md");
    write_file(&review_summary_path, "Task 1 review found a blocker.\n");
    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
    assert_eq!(close_json["required_follow_up"], "record_review_dispatch");
}

#[test]
fn plan_execution_close_current_task_records_failed_task_outcomes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-failures");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
    assert_eq!(operator_after_fail["phase"], "executing");
    assert_eq!(
        operator_after_fail["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(operator_after_fail["review_state_status"], "clean");
    assert_eq!(operator_after_fail["follow_up_override"], "none");
    let status_after_fail = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should immediately reroute failed task review to execution reentry",
    );
    assert_eq!(
        status_after_fail["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(
        status_after_fail["next_action"],
        "execution reentry required"
    );
    assert_eq!(status_after_fail["follow_up_override"], "none");
}

#[test]
fn plan_execution_close_current_task_records_failed_review_outcomes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-review-fail");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
    assert_eq!(operator_after_fail["phase"], "executing");
    assert_eq!(
        operator_after_fail["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(operator_after_fail["review_state_status"], "clean");
    assert_eq!(operator_after_fail["follow_up_override"], "none");
    let status_after_fail = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should immediately reroute failed final review to execution reentry",
    );
    assert_eq!(
        status_after_fail["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(
        status_after_fail["next_action"],
        "execution reentry required"
    );
    assert_eq!(status_after_fail["follow_up_override"], "none");

    let rerun_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
    let conflicting_pass_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
fn plan_execution_close_current_task_records_failed_review_with_passing_verification() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-close-current-task-review-fail-verification-pass");
    let repo = repo_dir.path();
    let state = state_dir.path();
    setup_task_boundary_blocked_case(repo, state, plan_rel, "main");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
fn plan_execution_close_current_task_failed_review_prefers_handoff_override() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-handoff-override");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before close-current-task handoff override",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("handoff override fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-handoff-override-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "fail",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("handoff override review summary path should be utf-8"),
            "--verification-result",
            "not-run",
        ],
        "close-current-task should prefer handoff override over execution reentry",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["closure_action"], "blocked");
    assert_eq!(close_json["task_closure_status"], "not_current");
    assert_eq!(close_json["required_follow_up"], "record_handoff");

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route failed task review to handoff when override is active",
    );
    assert_eq!(operator_after_fail["phase"], "handoff_required");
    assert_eq!(
        operator_after_fail["phase_detail"],
        "handoff_recording_required"
    );
    assert_eq!(operator_after_fail["follow_up_override"], "record_handoff");
}

#[test]
fn workflow_operator_ignores_forged_transfer_artifact_without_authoritative_checkpoint() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-forged-transfer-artifact-without-checkpoint");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before forged transfer artifact coverage",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("forged transfer fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-forged-transfer-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
    assert_eq!(close_json["required_follow_up"], "record_handoff");

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
    assert_eq!(operator_json["follow_up_override"], "record_handoff");

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should ignore forged transfer artifacts without authoritative checkpoints",
    );
    assert_eq!(status_json["follow_up_override"], "record_handoff");
}

#[test]
fn workflow_operator_keeps_handoff_override_when_checkpoint_decision_reason_codes_drift() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-transfer-decision-reason-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before checkpoint decision drift coverage",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("decision-drift transfer fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-transfer-decision-drift-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
    assert_eq!(close_json["required_follow_up"], "record_handoff");

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
    assert_eq!(operator_json["follow_up_override"], "record_handoff");

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should keep handoff override when checkpoint decision reason codes drift",
    );
    assert_eq!(status_json["follow_up_override"], "record_handoff");
}

#[test]
fn plan_execution_transfer_records_when_checkpoint_scope_does_not_match_current_decision() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-transfer-checkpoint-scope-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before checkpoint scope-drift transfer coverage",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("scope-drift transfer fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-transfer-scope-drift-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
    assert_eq!(close_json["required_follow_up"], "record_handoff");

    let status_before_scope_drift = run_plan_execution_json(
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
    assert_eq!(
        operator_before_transfer["follow_up_override"],
        "record_handoff"
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
    assert_eq!(operator_after_transfer["follow_up_override"], "none");
}

#[test]
fn plan_execution_transfer_blocks_when_requested_scope_mismatches_current_decision_scope() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-transfer-mismatched-requested-scope");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before mismatched requested transfer scope coverage",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("mismatched requested transfer scope fixture should expose dispatch id")
        .to_owned();
    let review_summary_path =
        repo.join("task-1-transfer-requested-scope-mismatch-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
    assert_eq!(close_json["required_follow_up"], "record_handoff");

    let operator_before_transfer = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should require handoff recording before mismatched requested transfer scope",
    );
    assert_eq!(
        operator_before_transfer["follow_up_override"],
        "record_handoff"
    );

    let transfer_json = run_plan_execution_json_real_cli(
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
    assert_eq!(transfer_json["action"], "blocked");
    assert_eq!(transfer_json["scope"], "branch");
    assert_eq!(transfer_json["code"], Value::Null);
    assert_eq!(
        transfer_json["trace_summary"],
        Value::from(
            "transfer failed closed because the requested scope does not satisfy the current handoff decision scope.",
        )
    );
    assert_eq!(
        transfer_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution transfer --plan {plan_rel} --scope task --to <owner> --reason <reason>"
        ))
    );
    assert_eq!(transfer_json["rederive_via_workflow_operator"], Value::Null);

    let operator_after_transfer = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep handoff override after mismatched requested transfer scope",
    );
    assert_eq!(
        operator_after_transfer["follow_up_override"],
        "record_handoff"
    );
}

#[test]
fn plan_execution_transfer_reuses_equivalent_artifact_by_restoring_checkpoint() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-transfer-restores-equivalent-checkpoint");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before equivalent transfer rerun coverage",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("equivalent transfer fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-equivalent-transfer-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
    assert_eq!(close_json["required_follow_up"], "record_handoff");

    let safe_branch = branch_storage_key(&current_branch_name(repo));
    let status_before_transfer = run_plan_execution_json(
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
    assert_eq!(operator_after_transfer["phase"], "executing");
    assert_eq!(
        operator_after_transfer["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(operator_after_transfer["follow_up_override"], "none");
}

#[test]
fn plan_execution_transfer_routed_handoff_shape_is_executable_and_clears_override() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-transfer-routed-handoff-shape");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(repo, state, &[("handoff_required", Value::Bool(true))]);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before routed handoff transfer",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("routed handoff fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-1-routed-handoff-review-summary.md");
    write_file(
        &review_summary_path,
        "Task 1 review found a blocker that requires handoff.\n",
    );

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
    assert_eq!(close_json["required_follow_up"], "record_handoff");

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
            "featureforge plan execution transfer --plan {plan_rel} --scope task|branch --to <owner> --reason <reason>"
        ))
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

    let transfer_rerun = run_plan_execution_json_real_cli(
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
    assert_eq!(transfer_rerun["action"], "blocked");
    assert_eq!(transfer_rerun["code"], "out_of_phase_requery_required");
    assert_eq!(transfer_rerun["rederive_via_workflow_operator"], true);
    assert_eq!(
        transfer_rerun["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );

    let operator_after_transfer = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should clear routed handoff override after transfer recording",
    );
    assert_eq!(operator_after_transfer["phase"], "executing");
    assert_eq!(
        operator_after_transfer["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(operator_after_transfer["follow_up_override"], "none");

    let status_after_transfer = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should clear the routed handoff override after transfer recording",
    );
    assert_eq!(status_after_transfer["follow_up_override"], "none");

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
    assert_eq!(operator_after_pivot["follow_up_override"], "record_pivot");
    assert_eq!(
        operator_after_pivot["next_action"],
        "pivot / return to planning"
    );

    let status_after_pivot = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should keep pivot precedence even when a stale-equivalent handoff record already exists",
    );
    assert_eq!(status_after_pivot["follow_up_override"], "record_pivot");
}

#[test]
fn plan_execution_close_current_task_failed_verification_prefers_pivot_override() {
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

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before close-current-task pivot override",
    );
    let dispatch_id = dispatch["dispatch_id"]
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

    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
        "close-current-task should prefer pivot override over execution reentry",
    );
    assert_eq!(close_json["action"], "blocked");
    assert_eq!(close_json["closure_action"], "blocked");
    assert_eq!(close_json["task_closure_status"], "not_current");
    assert_eq!(close_json["required_follow_up"], "record_pivot");

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route failed task verification to pivot when override is active",
    );
    assert_eq!(operator_after_fail["phase"], "pivot_required");
    assert_eq!(
        operator_after_fail["phase_detail"],
        "planning_reentry_required"
    );
    assert_eq!(operator_after_fail["follow_up_override"], "record_pivot");
}

#[test]
fn workflow_operator_allows_fresh_task_redispatch_after_failed_task_review() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-redispatch-after-failed-review");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let first_dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task review dispatch before failed review recovery fixture",
    );
    let first_dispatch_id = first_dispatch["dispatch_id"]
        .as_str()
        .expect("failed review recovery fixture should expose first dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-review-fail-summary.md");
    write_file(
        &review_summary_path,
        "Task review found issues that require remediation.\n",
    );
    let _ = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &first_dispatch_id,
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

    let second_dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
        "workflow operator should allow fresh review readiness after a failed task review is redispached",
    );
    assert_eq!(operator_json["phase"], "task_closure_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "task_closure_recording_ready"
    );
    assert_eq!(operator_json["follow_up_override"], "none");
    assert_eq!(
        operator_json["recording_context"]["dispatch_id"],
        Value::from(second_dispatch_id)
    );
}

#[test]
fn plan_execution_record_review_dispatch_preserves_failed_task_outcome_history_on_redispatch() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-task-negative-history-persists-on-redispatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let first_dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
    let _ = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &first_dispatch_id,
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

    let second_dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch should keep prior failed task outcome history when redispatching",
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
fn plan_execution_close_current_task_supersedes_overlapping_prior_task_closures() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-close-current-task-supersedes-overlap");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let task1_dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch should expose task 1 dispatch contract fields",
    );
    let task1_dispatch_id = task1_dispatch["dispatch_id"]
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
    let task1_close = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &task1_dispatch_id,
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

    let status_after_task1 = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose execution fingerprint after task 1 closure",
    );
    let begin_task2 = run_plan_execution_json(
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
    let _complete_task2 = run_plan_execution_json(
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

    let task2_dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "2",
        ],
        "record-review-dispatch should expose task 2 dispatch contract fields",
    );
    let task2_dispatch_id = task2_dispatch["dispatch_id"]
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
    let task2_close = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--dispatch-id",
            &task2_dispatch_id,
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
    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should use only the effective current task-closure set",
    );
    assert_eq!(branch_closure["action"], "recorded");
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("record-branch-closure should expose branch closure id")
        .to_owned();
    let explain = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should expose superseded task closures after supersession",
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
fn workflow_operator_waits_for_final_review_result_after_dispatch() {
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
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for workflow operator pending fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let final_review_rerun = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
fn workflow_operator_routes_final_review_result_ready_to_advance_late_stage() {
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
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for workflow operator ready fixture",
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
        "workflow operator json for final review result ready",
    );

    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
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
            "featureforge plan execution advance-late-stage --plan {plan_rel} --dispatch-id {dispatch_id} --reviewer-source <source> --reviewer-id <id> --result pass|fail --summary-file <path>"
        ))
    );
}

#[test]
fn workflow_operator_routes_dispatched_final_review_with_missing_release_overlay_to_repair_review_state()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-release-missing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before release-readiness reroute",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    clear_current_authoritative_release_readiness(repo, state);

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
        "workflow operator should reroute dispatched final review without release readiness",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for dispatched final review with missing release overlay",
    );

    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        status_json["reason_codes"],
        Value::from(vec![String::from("derived_review_state_missing")])
    );
    assert_eq!(status_json["review_state_status"], "clean");
    assert!(
        status_json["blocking_records"]
            .as_array()
            .is_some_and(|records| records.iter().any(|record| {
                record["code"] == "derived_review_state_missing"
                    && record["required_follow_up"] == "repair_review_state"
            })),
        "status should surface derived review-state loss as a structured blocker: {status_json}"
    );
    assert_eq!(
        operator_json["next_action"],
        "repair review state / reenter execution"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_operator_reroutes_failed_final_review_back_to_release_prerequisite() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-release-prereq-priority");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
        "workflow operator should keep release prerequisite routing ahead of failed final-review reentry",
    );

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "release_blocker_resolution_required"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["next_action"], "resolve release blocker");
}

#[test]
fn workflow_operator_reroutes_dispatched_final_review_blocked_release_ready_to_resolution() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-release-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before blocked release-readiness reroute",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    set_current_authoritative_release_readiness_result(repo, state, "blocked");

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
        "workflow operator should reroute blocked final review back to release blocker resolution",
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
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result ready|blocked --summary-file <path>"
        ))
    );

    let status_json = run_plan_execution_json(
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
                    && record["required_follow_up"].is_null()
            })),
        "status should expose a structured release-readiness prerequisite blocker summary: {status_json}"
    );
}

#[test]
fn workflow_operator_requires_fresh_final_review_dispatch_after_branch_closure_changes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-final-review-dispatch-stale");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-review-dispatch --plan {plan_rel} --scope final-review"
        ))
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should also reject stale final-review dispatch lineage when gate-review invalidates it",
    );
    assert_eq!(
        status_json["phase_detail"],
        "final_review_dispatch_required"
    );
    assert_eq!(status_json["review_state_status"], "clean");
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-review-dispatch --plan {plan_rel} --scope final-review"
        ))
    );
    assert!(
        status_json["blocking_records"]
            .as_array()
            .is_some_and(|records| records.iter().any(|record| {
                record["code"] == "final_review_dispatch_required"
                    && record["required_follow_up"] == "record_review_dispatch"
            })),
        "status should expose the same final-review redispatch blocker: {status_json}"
    );
}

#[test]
fn plan_execution_final_review_dispatch_requires_release_readiness_ready() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-final-review-dispatch-requires-release-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    set_current_branch_closure(repo, state, "branch-release-closure");
    let state_before = authoritative_harness_state(repo, state);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
fn workflow_operator_routes_final_review_pending_without_current_closure_to_record_branch_closure()
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
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(operator_json["next_action"], "record branch closure");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn plan_execution_advance_late_stage_records_final_review() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-record");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
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
        "advance-late-stage final review command should succeed",
    );

    assert_eq!(review_json["action"], "recorded");
    assert_eq!(review_json["stage_path"], "final_review");
    assert_eq!(review_json["delegated_primitive"], "record-final-review");

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
fn plan_execution_record_final_review_primitive_records_final_review() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-final-review-primitive");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for primitive fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));

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
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-final-review",
            "--plan",
            plan_rel,
            "--branch-closure-id",
            &branch_closure_id,
            "--dispatch-id",
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
        "record-final-review primitive command should succeed",
    );

    assert_eq!(review_json["action"], "recorded");
    assert_eq!(review_json["stage_path"], "final_review");
    assert_eq!(review_json["delegated_primitive"], "record-final-review");
}

#[test]
fn plan_execution_record_final_review_primitive_rejects_overlay_only_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-final-review-overlay-only-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for overlay-only closure fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));

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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let summary_path = repo.join("overlay-only-final-review-summary.md");
    write_file(
        &summary_path,
        "Final review should not bind to overlay-only branch closure state.\n",
    );
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-final-review",
            "--plan",
            plan_rel,
            "--branch-closure-id",
            &branch_closure_id,
            "--dispatch-id",
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
        "record-final-review should fail closed when only overlay branch-closure truth remains",
    );

    assert_eq!(review_json["action"], "blocked");
    assert_eq!(
        review_json["code"],
        Value::from("out_of_phase_requery_required")
    );
    assert_eq!(
        review_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
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
fn plan_execution_advance_late_stage_final_review_records_runtime_deviation_disposition() {
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

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for runtime-deviation fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));

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
        "workflow operator json for runtime-deviation final review fixture",
    );
    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("runtime-deviation fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-deviation-summary.md");
    write_file(
        &summary_path,
        "Independent final review passed after runtime topology downgrade review.\n",
    );
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
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
        "advance-late-stage final review should record runtime deviation disposition",
    );
    assert_eq!(review_json["action"], "recorded");

    let authoritative_state = authoritative_harness_state(repo, state);
    let final_review_fingerprint = authoritative_state["last_final_review_artifact_fingerprint"]
        .as_str()
        .expect("runtime-deviation final review should publish authoritative artifact fingerprint");
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
    let reviewer_artifact_path = PathBuf::from(
        receipt
            .reviewer_artifact_path
            .expect("runtime-deviation final review should bind reviewer artifact path"),
    );
    let reviewer_artifact_source = fs::read_to_string(&reviewer_artifact_path)
        .expect("runtime-deviation reviewer artifact should be readable");
    assert!(
        reviewer_artifact_source.contains("**Recorded Execution Deviations:** present"),
        "reviewer artifact should record runtime deviation presence: {reviewer_artifact_source}"
    );
    assert!(
        reviewer_artifact_source.contains("**Deviation Review Verdict:** pass"),
        "reviewer artifact should record a passing runtime deviation verdict: {reviewer_artifact_source}"
    );
}

#[test]
fn plan_execution_advance_late_stage_final_review_blocks_without_release_ready() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-missing-release-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before clearing release readiness",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let dispatch_id = dispatch["dispatch_id"]
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
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
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
        "advance-late-stage final review should fail closed without release readiness ready",
    );

    assert_eq!(review_json["action"], "blocked");
    assert_eq!(review_json["stage_path"], "final_review");
    assert_eq!(review_json["delegated_primitive"], "record-final-review");
    assert_eq!(review_json["code"], Value::Null);
    assert_eq!(review_json["recommended_command"], Value::Null);
    assert_eq!(review_json["rederive_via_workflow_operator"], Value::Null);
    assert_eq!(review_json["required_follow_up"], "repair_review_state");
    assert!(
        review_json["trace_summary"]
            .as_str()
            .is_some_and(|value| {
                value.contains(
                    "phase must be re-derived through workflow/operator before final-review recording can proceed"
                )
            }),
        "json: {review_json}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert!(authoritative_state["current_final_review_result"].is_null());
    assert!(authoritative_state["final_review_state"].is_null());
}

#[test]
fn plan_execution_advance_late_stage_final_review_blocked_release_ready_requires_resolution() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-blocked-release-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch before blocking release readiness",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final review dispatch should expose dispatch_id")
        .to_owned();
    set_current_authoritative_release_readiness_result(repo, state, "blocked");

    let summary_path = repo.join("final-review-blocked-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
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
        "advance-late-stage final review should require blocker resolution when release readiness is blocked",
    );

    assert_eq!(review_json["action"], "blocked");
    assert_eq!(review_json["stage_path"], "final_review");
    assert_eq!(review_json["delegated_primitive"], "record-final-review");
    assert_eq!(
        review_json["code"],
        Value::from("out_of_phase_requery_required")
    );
    assert_eq!(
        review_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
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
fn plan_execution_advance_late_stage_final_review_rerun_is_idempotent_and_conflicts_fail_closed() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-idempotency");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
    let first = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
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
            "--dispatch-id",
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
    let second = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
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

    let conflicting = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
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
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
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
    let stale_rerun = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
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
        "stale final-review rerun should fail closed",
    );
    assert_eq!(stale_rerun["action"], "blocked");
    assert_eq!(stale_rerun["code"], Value::Null);
    assert_eq!(stale_rerun["recommended_command"], Value::Null);
    assert_eq!(stale_rerun["rederive_via_workflow_operator"], Value::Null);
    assert_eq!(stale_rerun["required_follow_up"], "repair_review_state");
}

#[test]
fn final_review_artifact_invalidations_reroute_back_to_final_review_dispatch_when_branch_closure_is_unchanged()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let branch_closure_id = "branch-release-closure";
    for (case_name, mutator, expected_reason_code, republish_authoritative) in [
        ("malformed", "malformed", "review_artifact_malformed", true),
        (
            "plan_mismatch",
            "plan_mismatch",
            "review_artifact_plan_mismatch",
            true,
        ),
        (
            "authoritative_provenance_invalid",
            "authoritative_provenance_invalid",
            "review_artifact_authoritative_provenance_invalid",
            false,
        ),
    ] {
        let (repo_dir, state_dir) =
            init_repo(&format!("plan-execution-final-review-rerun-{case_name}"));
        let repo = repo_dir.path();
        let state = state_dir.path();
        let base_branch = expected_release_base_branch(repo);
        complete_workflow_fixture_execution(repo, state, plan_rel);
        write_branch_test_plan_artifact(repo, state, plan_rel, "no");
        write_branch_release_artifact(repo, state, plan_rel, &base_branch);
        mark_current_branch_closure_release_ready(repo, state, branch_closure_id);
        let dispatch = run_plan_execution_json(
            repo,
            state,
            &[
                "record-review-dispatch",
                "--plan",
                plan_rel,
                "--scope",
                "final-review",
            ],
            &format!("plan execution final review dispatch for {case_name} invalidation coverage"),
        );
        let dispatch_id = dispatch["dispatch_id"]
            .as_str()
            .expect("final-review invalidation fixture should expose dispatch_id")
            .to_owned();

        let summary_path = repo.join(format!("final-review-{case_name}-summary.md"));
        write_file(&summary_path, "Independent final review passed.\n");
        let first = run_plan_execution_json(
            repo,
            state,
            &[
                "advance-late-stage",
                "--plan",
                plan_rel,
                "--dispatch-id",
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
            &format!(
                "first final-review recording should succeed before {case_name} invalidation coverage"
            ),
        );
        assert_eq!(first["action"], "recorded", "case {case_name}: {first}");
        let gate_review = run_plan_execution_json_real_cli(
            repo,
            state,
            &["gate-review", "--plan", plan_rel],
            &format!(
                "gate-review should persist a finish checkpoint before {case_name} invalidation coverage"
            ),
        );
        assert_eq!(
            gate_review["allowed"],
            Value::Bool(true),
            "case {case_name}: {gate_review}"
        );

        let authoritative_state_before = authoritative_harness_state(repo, state);
        let final_review_record_id = authoritative_state_before["current_final_review_record_id"]
            .as_str()
            .expect("final-review invalidation fixture should expose current record id")
            .to_owned();
        let final_review_history_len = authoritative_state_before["final_review_record_history"]
            .as_object()
            .expect("final review history should remain an object")
            .len();
        let final_review_fingerprint =
            authoritative_state_before["last_final_review_artifact_fingerprint"]
                .as_str()
                .expect("final-review invalidation fixture should expose artifact fingerprint")
                .to_owned();
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

        let gate_finish = run_plan_execution_json(
            repo,
            state,
            &["gate-finish", "--plan", plan_rel],
            &format!("gate-finish should expose {case_name} final-review invalidation"),
        );
        assert_eq!(
            gate_finish["allowed"],
            Value::Bool(false),
            "case {case_name}: {gate_finish}"
        );
        assert!(
            gate_finish["reason_codes"]
                .as_array()
                .is_some_and(|codes| codes.iter().any(|code| code == expected_reason_code)),
            "case {case_name}: expected gate-finish to include {expected_reason_code}, got {gate_finish}"
        );

        let operator_json = run_featureforge_with_env_json(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            &format!(
                "workflow operator should reroute to final-review dispatch when {case_name} invalidates authoritative reviewer/final-review truth"
            ),
        );
        assert_eq!(
            operator_json["phase"], "final_review_pending",
            "case {case_name}: {operator_json}"
        );
        assert_eq!(
            operator_json["phase_detail"], "final_review_dispatch_required",
            "case {case_name}: {operator_json}"
        );
        assert_eq!(
            operator_json["review_state_status"], "clean",
            "case {case_name}: {operator_json}"
        );
        assert_eq!(
            operator_json["recommended_command"],
            Value::from(format!(
                "featureforge plan execution record-review-dispatch --plan {plan_rel} --scope final-review"
            )),
            "case {case_name}: {operator_json}"
        );

        let status_json = run_plan_execution_json(
            repo,
            state,
            &["status", "--plan", plan_rel],
            &format!(
                "status should require final-review dispatch when {case_name} invalidates authoritative reviewer/final-review truth"
            ),
        );
        assert_eq!(
            status_json["review_state_status"], "clean",
            "case {case_name}: {status_json}"
        );
        assert_eq!(
            status_json["phase_detail"], "final_review_dispatch_required",
            "case {case_name}: {status_json}"
        );
        assert!(
            status_json["blocking_records"]
                .as_array()
                .is_some_and(|records| records
                    .iter()
                    .any(|record| record["code"] == "final_review_dispatch_required")),
            "case {case_name}: status should require final-review redispatch: {status_json}"
        );

        let stale_rerun = run_plan_execution_json_real_cli(
            repo,
            state,
            &[
                "advance-late-stage",
                "--plan",
                plan_rel,
                "--dispatch-id",
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
            &format!(
                "same-state final-review rerun should requery when {case_name} invalidates authoritative reviewer/final-review truth"
            ),
        );
        assert_eq!(
            stale_rerun["action"], "blocked",
            "case {case_name}: {stale_rerun}"
        );
        assert_eq!(
            stale_rerun["code"],
            Value::Null,
            "case {case_name}: {stale_rerun}"
        );
        assert_eq!(
            stale_rerun["recommended_command"],
            Value::Null,
            "case {case_name}: {stale_rerun}"
        );
        assert_eq!(
            stale_rerun["rederive_via_workflow_operator"],
            Value::Null,
            "case {case_name}: {stale_rerun}"
        );
        assert_eq!(
            stale_rerun["required_follow_up"],
            Value::from("record_review_dispatch"),
            "case {case_name}: {stale_rerun}"
        );

        let primitive_rerun = run_plan_execution_json_real_cli(
            repo,
            state,
            &[
                "record-final-review",
                "--plan",
                plan_rel,
                "--branch-closure-id",
                branch_closure_id,
                "--dispatch-id",
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
            &format!(
                "record-final-review rerun should requery when {case_name} invalidates authoritative reviewer/final-review truth"
            ),
        );
        assert_eq!(
            primitive_rerun["action"], "blocked",
            "case {case_name}: {primitive_rerun}"
        );
        assert_eq!(
            primitive_rerun["code"],
            Value::Null,
            "case {case_name}: {primitive_rerun}"
        );
        assert_eq!(
            primitive_rerun["required_follow_up"],
            Value::from("record_review_dispatch"),
            "case {case_name}: {primitive_rerun}"
        );
        assert_eq!(
            primitive_rerun["recommended_command"],
            Value::Null,
            "case {case_name}: {primitive_rerun}"
        );
        assert_eq!(
            primitive_rerun["rederive_via_workflow_operator"],
            Value::Null,
            "case {case_name}: {primitive_rerun}"
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
fn plan_execution_record_qa_same_state_rerun_requeries_when_final_review_is_invalidated() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-final-review-invalidated");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("qa-after-final-review-summary.md");
    write_file(&summary_path, "Browser QA passed for the current branch.\n");
    let first = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "initial record-qa invocation should succeed before final-review invalidation coverage",
    );
    assert_eq!(first["action"], "recorded");

    let authoritative_state = authoritative_harness_state(repo, state);
    let final_review_fingerprint = authoritative_state["last_final_review_artifact_fingerprint"]
        .as_str()
        .expect("qa invalidation fixture should expose final-review fingerprint")
        .to_owned();
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
        "workflow operator should require final-review redispatch after final-review artifact invalidation",
    );
    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "final_review_dispatch_required"
    );

    let rerun = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "same-state record-qa rerun should requery instead of returning already_current after final-review invalidation",
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
fn workflow_operator_routes_tampered_reviewer_artifact_back_to_final_review_dispatch() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-reviewer-artifact-tamper");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "final review dispatch should succeed before reviewer-artifact tamper routing coverage",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("reviewer-artifact tamper fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-reviewer-artifact-tamper-summary.md");
    write_file(&summary_path, "Independent final review passed.\n");
    let first = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
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
    let final_review_fingerprint =
        authoritative_state_before["last_final_review_artifact_fingerprint"]
            .as_str()
            .expect(
                "reviewer-artifact tamper fixture should expose final-review artifact fingerprint",
            )
            .to_owned();
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
            "Independent final review passed.",
            "Independent final review passed after reviewer-artifact tamper.",
        );
    write_file(&reviewer_artifact_path, &tampered_reviewer_source);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should reroute to final-review dispatch after reviewer-artifact tamper",
    );
    assert_eq!(operator_json["phase"], "final_review_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "final_review_dispatch_required"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-review-dispatch --plan {plan_rel} --scope final-review"
        ))
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should require final-review redispatch after reviewer-artifact tamper",
    );
    assert_eq!(status_json["review_state_status"], "clean");
    assert_eq!(
        status_json["phase_detail"],
        "final_review_dispatch_required"
    );
    assert!(
        status_json["blocking_records"]
            .as_array()
            .is_some_and(|records| records
                .iter()
                .any(|record| record["code"] == "final_review_dispatch_required")),
        "status should require final-review redispatch after reviewer-artifact tamper: {status_json}"
    );

    let stale_rerun = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
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
        "same-state final-review rerun should requery after reviewer-artifact tamper",
    );
    assert_eq!(stale_rerun["action"], "blocked");
    assert_eq!(stale_rerun["code"], Value::Null);
    assert_eq!(stale_rerun["recommended_command"], Value::Null);
    assert_eq!(stale_rerun["rederive_via_workflow_operator"], Value::Null);
    assert_eq!(stale_rerun["required_follow_up"], "record_review_dispatch");

    let redispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "record-review-dispatch should mint a new current dispatch after reviewer-artifact tamper",
    );
    assert_eq!(redispatch["allowed"], Value::Bool(true));
    assert_eq!(redispatch["action"], Value::from("recorded"));
    assert_eq!(redispatch["dispatch_id"], Value::from(dispatch_id.clone()));

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
            "--dispatch-id",
            "dispatch-missing",
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
    assert!(review_json["code"].is_null(), "json: {review_json}");
    assert_eq!(review_json["required_follow_up"], "record_review_dispatch");
}

#[test]
fn plan_execution_advance_late_stage_final_review_fail_reroutes_to_execution_reentry() {
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
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for failing rerun fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final-review fail rerun fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-fail-summary.md");
    write_file(
        &summary_path,
        "Independent final review found a release blocker.\n",
    );
    let first = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
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
    assert_eq!(operator_after_fail["follow_up_override"], "none");
    let status_after_fail = run_plan_execution_json(
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
    assert_eq!(status_after_fail["follow_up_override"], "none");

    let second = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
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
fn plan_execution_advance_late_stage_final_review_fail_prefers_handoff_override() {
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
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for handoff override fixture",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("final-review handoff override fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-handoff-override-summary.md");
    write_file(
        &summary_path,
        "Independent final review found handoff-only blocker.\n",
    );
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-handoff-override",
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "advance-late-stage final review should prefer handoff override over execution reentry",
    );
    assert_eq!(review_json["action"], "recorded", "json: {review_json}");
    assert_eq!(
        review_json["required_follow_up"], "record_handoff",
        "json: {review_json}"
    );

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route failed final review to handoff when override is active",
    );
    assert_eq!(operator_after_fail["phase"], "handoff_required");
    assert_eq!(
        operator_after_fail["phase_detail"],
        "handoff_recording_required"
    );
    assert_eq!(operator_after_fail["follow_up_override"], "record_handoff");

    let rerun = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "reviewer-handoff-override",
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "equivalent failing final-review rerun should stay idempotent and preserve the handoff follow-up on the real CLI path",
    );
    assert_eq!(rerun["action"], "already_current", "json: {rerun}");
    assert!(rerun["code"].is_null(), "json: {rerun}");
    assert!(rerun["recommended_command"].is_null(), "json: {rerun}");
    assert!(
        rerun["rederive_via_workflow_operator"].is_null(),
        "json: {rerun}"
    );
    assert_eq!(
        rerun["required_follow_up"], "record_handoff",
        "json: {rerun}"
    );
}

#[test]
fn plan_execution_advance_late_stage_accepts_human_independent_reviewer() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-human-reviewer");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
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
    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("final review recording fixture should expose dispatch_id")
        .to_owned();

    let summary_path = repo.join("final-review-human-summary.md");
    write_file(&summary_path, "Independent human final review passed.\n");
    let review_json = run_plan_execution_json(
        repo,
        state,
        &[
            "advance-late-stage",
            "--plan",
            plan_rel,
            "--dispatch-id",
            &dispatch_id,
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
    assert_eq!(operator_json["phase"], "ready_for_branch_completion");
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

    assert_eq!(operator_json["phase"], "document_release_pending");
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
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result ready|blocked --summary-file <path>"
        ))
    );
}

#[test]
fn workflow_record_pivot_writes_project_artifact() {
    let (repo_dir, state_dir) = init_repo("workflow-record-pivot");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    write_file(
        &authoritative_state_path,
        r#"{"harness_phase":"pivot_required","latest_authoritative_sequence":23,"reason_codes":["blocked_on_plan_revision"]}"#,
    );

    let pivot_json = run_featureforge_with_env_json(
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
        "workflow record-pivot json",
    );

    assert_eq!(pivot_json["action"], "recorded");
    assert_eq!(pivot_json["plan_path"], plan_rel);
    assert_eq!(
        pivot_json["reason"],
        "plan revision superseded current execution"
    );
    let record_path = pivot_json["record_path"]
        .as_str()
        .expect("workflow record-pivot should emit record_path");
    let record_source =
        fs::read_to_string(record_path).expect("workflow record-pivot artifact should be readable");
    assert!(record_source.contains("# Workflow Pivot Record"));
    assert!(record_source.contains(&format!("**Source Plan:** `{plan_rel}`")));
    assert!(record_source.contains("**Reason:** plan revision superseded current execution"));
    assert!(record_source.contains("blocked_on_plan_revision"));
    assert!(record_source.contains("follow_up_override_record_pivot"));
    assert!(Path::new(record_path).starts_with(project_artifact_dir(repo, state)));

    let idempotent_json = run_featureforge_with_env_json(
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
        "workflow record-pivot idempotent json",
    );
    assert_eq!(idempotent_json["action"], "already_current");
    let idempotent_record_path = idempotent_json["record_path"]
        .as_str()
        .expect("workflow record-pivot idempotent run should emit record_path");
    assert_eq!(
        fs::canonicalize(record_path).expect("record_path should canonicalize"),
        fs::canonicalize(idempotent_record_path)
            .expect("idempotent record_path should canonicalize")
    );

    let equivalent_plan_spelling_json = run_featureforge_with_env_json(
        repo,
        state,
        &[
            "workflow",
            "record-pivot",
            "--plan",
            "./docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md",
            "--reason",
            "plan revision superseded current execution",
            "--json",
        ],
        &[],
        "workflow record-pivot equivalent plan spelling json",
    );
    assert_eq!(equivalent_plan_spelling_json["action"], "already_current");
    assert_eq!(equivalent_plan_spelling_json["plan_path"], plan_rel);
    let equivalent_record_path = equivalent_plan_spelling_json["record_path"]
        .as_str()
        .expect("equivalent plan spelling should emit record_path");
    assert_eq!(
        fs::canonicalize(record_path).expect("record_path should canonicalize"),
        fs::canonicalize(equivalent_record_path)
            .expect("equivalent record_path should canonicalize")
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
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    write_file(
        &authoritative_state_path,
        r#"{"harness_phase":"pivot_required","latest_authoritative_sequence":23,"reason_codes":["blocked_on_plan_revision"]}"#,
    );

    let pivot_json = run_featureforge_with_env_json(
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
        "workflow record-pivot json before clearing authoritative checkpoint",
    );
    assert_eq!(pivot_json["action"], "recorded");
    let record_path = pivot_json["record_path"]
        .as_str()
        .expect("workflow record-pivot should emit record_path");
    assert!(
        Path::new(record_path).is_file(),
        "pivot artifact should exist before authoritative checkpoint is cleared"
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
    assert_eq!(operator_json["follow_up_override"], "record_pivot");
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
    assert_eq!(status_json["follow_up_override"], "record_pivot");
}

#[test]
fn workflow_operator_ignores_off_directory_pivot_checkpoint_path() {
    let (repo_dir, state_dir) = init_repo("workflow-record-pivot-off-directory-checkpoint");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);
    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    write_file(
        &authoritative_state_path,
        r#"{"harness_phase":"pivot_required","latest_authoritative_sequence":23,"reason_codes":["blocked_on_plan_revision"]}"#,
    );

    let pivot_json = run_featureforge_with_env_json(
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
        "workflow record-pivot json before injecting an off-directory checkpoint path",
    );
    assert_eq!(pivot_json["action"], "recorded");
    let record_path = pivot_json["record_path"]
        .as_str()
        .expect("workflow record-pivot should emit record_path");
    let record_source =
        fs::read_to_string(record_path).expect("workflow record-pivot artifact should be readable");
    let off_directory_checkpoint = repo.join("off-directory-pivot-checkpoint.md");
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
    assert_eq!(operator_json["follow_up_override"], "record_pivot");
    assert_eq!(operator_json["phase"], "pivot_required");

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should ignore off-directory runtime pivot checkpoints",
    );
    assert_eq!(status_json["follow_up_override"], "record_pivot");
}

#[test]
fn workflow_record_pivot_blocks_out_of_phase() {
    let (repo_dir, state_dir) = init_repo("workflow-record-pivot-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let pivot_json = run_featureforge_with_env_json(
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
        "workflow record-pivot blocked json",
    );

    assert_eq!(pivot_json["action"], "blocked");
    assert_eq!(pivot_json["record_path"], Value::Null);
    assert!(
        pivot_json["remediation"]
            .as_str()
            .is_some_and(|text| text.contains("pivot_required")),
        "{pivot_json:?}"
    );
}

#[test]
fn workflow_record_pivot_preserves_missing_qa_requirement_reason_code() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-record-pivot-missing-qa-requirement");
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

    let pivot_json = run_featureforge_with_env_json(
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
        "workflow record-pivot json for missing QA Requirement",
    );

    assert_eq!(pivot_json["action"], "recorded");
    let record_path = pivot_json["record_path"]
        .as_str()
        .expect("workflow record-pivot should emit record_path");
    let record_source =
        fs::read_to_string(record_path).expect("workflow record-pivot artifact should be readable");
    assert!(record_source.contains("qa_requirement_missing_or_invalid"));
    assert!(record_source.contains("follow_up_override_record_pivot"));
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

    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(operator_json["next_action"], "record branch closure");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_operator_keeps_execution_scope_when_future_task_remains_unchecked() {
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

    let status_json = run_plan_execution_json(
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
    assert_eq!(status_json["blocking_task"], Value::Null);

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
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result ready|blocked --summary-file <path>"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should rederive late-stage routing when execution is exhausted despite a persisted executing phase",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "release_readiness_recording_ready"
    );
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result ready|blocked --summary-file <path>"
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should rederive first-entry late-stage routing from current task closures",
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
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
    assert_eq!(
        status_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn explain_review_state_omits_recommended_command_for_wait_state_lanes() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("explain-review-state-task-review-wait");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for explain-review-state wait-lane coverage",
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

    let explain_json = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should preserve omitted recommended_command for task-review wait lanes",
    );
    assert_eq!(
        explain_json["next_action"],
        "wait for external review result"
    );
    assert!(explain_json["recommended_command"].is_null());
}

#[test]
fn workflow_status_and_operator_reroute_prerelease_branch_closure_refresh_when_current_binding_stales()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-prerelease-branch-closure-refresh");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch =
        resolve_release_base_branch(&repo.join(".git"), "feature").expect("fixture base branch");
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");
    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should establish a current branch closure before prerelease refresh coverage",
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

    let explain_json = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should keep exposing the stale prerelease branch closure",
    );
    assert!(
        explain_json["stale_unreviewed_closures"]
            .as_array()
            .is_some_and(|closures| closures
                .iter()
                .any(|closure| closure == &Value::from(branch_closure_id.clone()))),
        "json: {explain_json:?}"
    );

    let status_json = run_plan_execution_json(
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
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
    assert_eq!(explain_json["next_action"], "record branch closure");
    assert_eq!(
        explain_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_status_and_operator_require_execution_reentry_when_no_branch_contributing_task_closure_remains()
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
                    "contract_identity": task_contract_identity(plan_rel, 1),
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

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should not offer branch closure recording when no branch-contributing task closure remains",
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
        "workflow operator should match no-branch-contributing task-closure reroute",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let record_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed when no branch-contributing task closure remains",
    );
    assert_eq!(record_json["action"], "blocked");
    assert_eq!(record_json["required_follow_up"], "repair_review_state");

    let reconcile_json = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should keep recommending repair-review-state until workflow/operator actually reroutes to branch-closure recording",
    );
    assert_eq!(reconcile_json["action"], "blocked");
    assert_eq!(
        reconcile_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_operator_keeps_execution_scope_when_resume_task_exists_despite_late_stage_phase() {
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

    let status_before_task_2 = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before task 2 begin for reopened execution routing",
    );
    let begin_task_2_step_1 = run_plan_execution_json(
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
    let complete_task_2_step_1 = run_plan_execution_json(
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
    let reopened = run_plan_execution_json(
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
            "Task 2 Step 1 needs remediation before late-stage progression.",
            "--expect-execution-fingerprint",
            complete_task_2_step_1["execution_fingerprint"]
                .as_str()
                .expect("task 2 complete should expose execution fingerprint for reopen"),
        ],
        "reopen task 2 step 1 for reopened execution routing",
    );
    assert_eq!(reopened["resume_task"], Value::from(2));
    assert_eq!(reopened["resume_step"], Value::from(1));

    update_authoritative_harness_state(
        repo,
        state,
        &[
            ("harness_phase", Value::from("document_release_pending")),
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
        ],
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should keep execution scope when a resume task exists",
    );
    assert_eq!(status_json["harness_phase"], "executing");
    assert_eq!(status_json["phase_detail"], "execution_reentry_required");
    assert_eq!(status_json["review_state_status"], "stale_unreviewed");
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
        "workflow operator should keep execution routing when a resume task exists",
    );
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "stale_unreviewed");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
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
    assert!(release_json["code"].is_null(), "json: {release_json}");
    assert_eq!(release_json["required_follow_up"], "record_branch_closure");
}

#[test]
fn plan_execution_record_qa_missing_current_closure_returns_out_of_phase_requery() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-missing-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("missing-closure-qa-summary.md");
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa should block through workflow/operator when branch closure is missing",
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
fn plan_execution_record_qa_rejects_overlay_only_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-overlay-only-closure");
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
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa should fail closed when only overlay branch-closure truth remains",
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
fn plan_execution_record_branch_closure_records_current_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-branch-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure command should succeed",
    );
    let branch_closure_id = branch_closure_json["branch_closure_id"]
        .as_str()
        .expect("record-branch-closure should expose branch_closure_id");
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
fn plan_execution_record_branch_closure_blocks_out_of_phase_after_late_stage_progression() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-idempotent-late-stage");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before late-stage idempotency coverage",
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
        "advance-late-stage should progress the branch beyond document_release_pending before branch-closure idempotency coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    let rerun = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed out of phase after late-stage progression",
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
fn plan_execution_record_branch_closure_uses_recorded_task_closure_provenance() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-branch-closure-real-task-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "plan execution task review dispatch for real branch provenance fixture",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("real provenance fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("real-provenance-review-summary.md");
    let verification_summary_path = repo.join("real-provenance-verification-summary.md");
    write_file(&review_summary_path, "Task 1 independent review passed.\n");
    write_file(
        &verification_summary_path,
        "Task 1 verification passed against the current reviewed state.\n",
    );
    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("real provenance review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("real provenance verification summary path should be utf-8"),
        ],
        "close-current-task should succeed for real branch provenance fixture",
    );
    let closure_record_id = close_json["closure_record_id"]
        .as_str()
        .expect("real provenance fixture should expose closure record id")
        .to_owned();

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

    let record_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should use recorded task closure provenance",
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
        record_source.contains(&closure_record_id),
        "branch closure should reference the recorded task 1 closure id: {record_source}"
    );
}

#[test]
fn plan_execution_record_branch_closure_re_records_when_reviewed_state_changes_at_same_head() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-branch-closure-reviewed-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let first_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "first record-branch-closure should succeed",
    );
    let first_branch_closure_id = first_branch_closure["branch_closure_id"]
        .as_str()
        .expect("first record-branch-closure should expose branch_closure_id")
        .to_owned();

    append_tracked_repo_line(
        repo,
        "README.md",
        "branch-closure reviewed-state regression coverage",
    );

    let second_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "second record-branch-closure should re-record after reviewed-state drift",
    );
    let second_branch_closure_id = second_branch_closure["branch_closure_id"]
        .as_str()
        .expect("second record-branch-closure should expose branch_closure_id");

    assert_eq!(second_branch_closure["action"], "recorded");
    assert_ne!(second_branch_closure_id, first_branch_closure_id);
    assert_eq!(
        second_branch_closure["superseded_branch_closure_ids"],
        Value::from(vec![first_branch_closure_id.clone()])
    );
    let explain = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should expose superseded branch closures after re-record",
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
        operator_json["recording_context"]["branch_closure_id"],
        Value::from(second_branch_closure_id)
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
fn plan_execution_record_branch_closure_falls_back_to_current_task_closure_set_when_current_branch_closure_is_stale()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-prefers-current-branch-baseline");
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
                    "contract_identity": task_contract_identity(plan_rel, 1),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
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
                    "reviewed_state_id": current_tracked_tree_id(repo),
                    "contract_identity": task_contract_identity(plan_rel, 2),
                    "effective_reviewed_surface_paths": [plan_rel],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 2 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 2 current verification"),
                    "closure_status": "current"
                }
            }),
        )],
    );

    let first_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "first record-branch-closure should succeed before current-branch-baseline coverage",
    );
    let first_branch_closure_id = first_branch_closure["branch_closure_id"]
        .as_str()
        .expect("first record-branch-closure should expose branch_closure_id")
        .to_owned();

    append_tracked_repo_line(
        repo,
        "README.md",
        "branch-closure baseline should absorb this late-stage edit",
    );

    let second_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "second record-branch-closure should absorb late-stage drift into the current branch closure",
    );
    let second_branch_closure_id = second_branch_closure["branch_closure_id"]
        .as_str()
        .expect("second record-branch-closure should expose branch_closure_id")
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

    let status_json = run_plan_execution_json(
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let third_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should rerecord against the still-current branch-closure baseline when branch-level drift is present",
    );
    assert_eq!(third_branch_closure["action"], "recorded");
    assert!(third_branch_closure["required_follow_up"].is_null());

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should remain current after branch-closure rerecord absorbs branch-level drift",
    );
    assert_eq!(repair_json["action"], "already_current");
    assert!(repair_json["required_follow_up"].is_null());
}

#[test]
fn plan_execution_record_branch_closure_blocks_late_stage_only_recreation_without_still_current_task_closure_baseline()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-empty-late-stage-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let first_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "first record-branch-closure should succeed before late-stage-only recreation",
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

    let status_json = run_plan_execution_json(
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
        ))
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
        ))
    );

    let second_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed when the previous branch closure is stale and no still-current task-closure baseline remains",
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
        "repair-review-state should route late-stage-only drift back to execution reentry when no still-current task-closure baseline remains",
    );
    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(repair_json["required_follow_up"], "execution_reentry");

    let status_after_repair = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose execution reentry after repair-review-state persists the reroute",
    );
    assert_eq!(
        status_after_repair["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(status_after_repair["review_state_status"], "clean");
    assert_ne!(
        status_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    assert_ne!(
        status_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let operator_after_repair = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should expose execution reentry after repair-review-state persists the reroute",
    );
    assert_eq!(operator_after_repair["phase"], "executing");
    assert_eq!(
        operator_after_repair["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(operator_after_repair["review_state_status"], "clean");
    assert_ne!(
        operator_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    assert_ne!(
        operator_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let reentry_command = operator_after_repair["recommended_command"]
        .as_str()
        .expect("execution reentry should expose an exact plan execution command")
        .to_owned();
    let _reentry_result = run_recommended_plan_execution_command_json_real_cli(
        repo,
        state,
        &reentry_command,
        "execution reentry exact command should succeed after repair-review-state persists the reroute",
    );

    let status_after_reentry = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should reflect the exact execution reentry command result",
    );
    assert_ne!(
        status_after_reentry["phase_detail"],
        Value::from("execution_reentry_required"),
        "successful execution reentry should advance status out of the repair-only reroute"
    );
    assert_eq!(status_after_reentry["review_state_status"], "clean");
    let operator_after_reentry = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should clear execution_reentry_required after the exact reentry command succeeds",
    );
    assert_ne!(
        operator_after_reentry["phase_detail"],
        Value::from("execution_reentry_required"),
        "successful execution reentry should consume the persisted reroute latch"
    );
    assert_ne!(
        operator_after_reentry["recommended_command"],
        Value::from(reentry_command),
        "workflow operator should move on after the exact reentry command succeeds"
    );
}

#[test]
fn plan_execution_record_branch_closure_allows_already_current_for_valid_empty_lineage_late_stage_exemption()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(
        "plan-execution-record-branch-closure-empty-lineage-late-stage-exemption-already-current",
    );
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should establish the current branch closure before empty-lineage exemption idempotency coverage",
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

    let rerun = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should stay idempotent for a still-current empty-lineage late-stage exemption branch closure",
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
fn plan_execution_record_branch_closure_blocks_late_stage_surface_exemption_rerecord_without_current_task_closure_baseline()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-late-stage-surface-exemption");
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

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should route stale empty-lineage late-stage-surface-only branch drift to branch-closure rerecording readiness",
    );
    assert_eq!(
        status_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
    assert_eq!(
        status_json["blocking_records"][0]["required_follow_up"],
        "record_branch_closure"
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route stale empty-lineage late-stage-surface-only branch drift to branch-closure rerecording readiness",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should reroute stale empty-lineage late-stage-surface-only branch drift back to branch-closure recording",
    );
    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(repair_json["required_follow_up"], "record_branch_closure");
    assert_eq!(
        repair_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let record_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should proceed after repair-review-state reroutes stale empty-lineage late-stage-surface-only drift back to branch-closure recording",
    );
    assert_eq!(record_json["action"], "recorded", "json: {record_json}");
    assert!(
        record_json["branch_closure_id"].as_str().is_some(),
        "recorded reroute should emit branch_closure_id, got {record_json}"
    );
}

#[test]
fn plan_execution_record_branch_closure_blocks_first_entry_drift_outside_late_stage_surface() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-branch-closure-first-entry-drift");
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

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed on first late-stage entry when drift escapes the task-closure baseline",
    );
    assert_eq!(branch_closure["action"], "blocked");
    assert_eq!(branch_closure["required_follow_up"], "repair_review_state");
}

#[test]
fn plan_execution_record_branch_closure_prefers_current_task_closure_set_baseline() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-current-task-set-baseline");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    seed_current_task_closure_state(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);

    let initial_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should establish the initial current branch closure before current task-closure lineage supersedes it",
    );
    let initial_branch_closure_id = initial_branch_closure["branch_closure_id"]
        .as_str()
        .expect("initial branch closure should expose branch_closure_id")
        .to_owned();

    write_repo_file(repo, "README.md", "task 2 still-current reviewed state\n");
    let task2_reviewed_state_id = current_tracked_tree_id(repo);
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
                    "contract_identity": task_contract_identity(plan_rel, 1),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                },
                "task-2": {
                    "dispatch_id": "task-2-current-dispatch",
                    "closure_record_id": "task-2-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": task2_reviewed_state_id,
                    "contract_identity": task_contract_identity(plan_rel, 2),
                    "effective_reviewed_surface_paths": ["README.md"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 2 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 2 current verification"),
                }
            }),
        )],
    );

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should use the authoritative current task-closure set baseline",
    );

    assert_eq!(branch_closure["action"], "recorded");
    let rererecorded_branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("re-recorded branch closure should expose branch_closure_id");
    assert_ne!(rererecorded_branch_closure_id, initial_branch_closure_id);
    assert_eq!(
        branch_closure["superseded_branch_closure_ids"],
        Value::from(vec![initial_branch_closure_id])
    );
}

#[test]
fn plan_execution_record_branch_closure_allows_deleted_covered_path_in_current_task_set_baseline() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-deleted-covered-path");
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
    let task1_reviewed_state_id = current_tracked_tree_id(repo);
    fs::remove_file(repo.join("README.md"))
        .expect("README should be removable for deleted covered-path baseline coverage");
    let task2_reviewed_state_id = current_tracked_tree_id(repo);

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
                    "contract_identity": task_contract_identity(plan_rel, 1),
                    "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 1 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 1 current verification"),
                },
                "task-2": {
                    "dispatch_id": "task-2-current-dispatch",
                    "closure_record_id": "task-2-current-closure",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "execution_run_id": "run-fixture",
                    "reviewed_state_id": task2_reviewed_state_id,
                    "contract_identity": task_contract_identity(plan_rel, 2),
                    "effective_reviewed_surface_paths": ["README.md"],
                    "review_result": "pass",
                    "review_summary_hash": sha256_hex(b"task 2 current review"),
                    "verification_result": "pass",
                    "verification_summary_hash": sha256_hex(b"task 2 current verification"),
                }
            }),
        )],
    );

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should accept a deleted covered path in the authoritative current task-closure set baseline",
    );

    assert_eq!(branch_closure["action"], "recorded");
    assert!(branch_closure["branch_closure_id"].is_string());
}

#[test]
fn plan_execution_record_branch_closure_fails_closed_when_current_task_closure_is_not_bound_to_active_plan()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-invalid-current-task-closure");
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
                    "contract_identity": task_contract_identity(plan_rel, 1),
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

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed when a current task closure is not bound to the active plan",
    );
    assert_eq!(branch_closure["action"], "blocked");
    assert_eq!(branch_closure["required_follow_up"], "repair_review_state");
    assert!(
        branch_closure["trace_summary"]
            .as_str()
            .is_some_and(|message| message.contains("prior_task_current_closure_invalid")),
        "record-branch-closure should surface invalid current task-closure provenance through the blocked command envelope, got {branch_closure}"
    );
}

#[test]
fn plan_execution_record_branch_closure_fails_closed_when_current_task_closure_reviewed_state_id_uses_noncanonical_git_commit_alias()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-git-commit-current-task-closure");
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
                    "contract_identity": task_contract_identity(plan_rel, 1),
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

    let record_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed when a current task closure uses a noncanonical git_commit reviewed_state_id alias",
    );
    assert_eq!(record_json["action"], "blocked");
    assert_eq!(record_json["required_follow_up"], "repair_review_state");
    assert!(
        record_json["trace_summary"]
            .as_str()
            .is_some_and(|message| {
                message.contains("prior_task_current_closure_reviewed_state_malformed")
            }),
        "record-branch-closure should surface noncanonical git_commit current task-closure state through the blocked command envelope, got {record_json}"
    );
}

#[test]
fn plan_execution_record_branch_closure_fails_closed_when_current_task_closure_reviewed_state_id_uses_git_tree_commit_sha_alias()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-git-tree-commit-current-task-closure");
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
                    "contract_identity": task_contract_identity(plan_rel, 1),
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

    let record_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed when a current task closure uses a git_tree commit alias instead of a canonical tree object id",
    );
    assert_eq!(record_json["action"], "blocked");
    assert_eq!(record_json["required_follow_up"], "repair_review_state");
    assert!(
        record_json["trace_summary"]
            .as_str()
            .is_some_and(|message| {
                message.contains("prior_task_current_closure_reviewed_state_malformed")
            }),
        "record-branch-closure should surface git_tree commit aliases through the blocked command envelope, got {record_json}"
    );
}

#[test]
fn plan_execution_record_branch_closure_fails_closed_when_current_task_closure_raw_record_is_malformed()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-malformed-current-task-closure-raw");
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
                    "contract_identity": task_contract_identity(plan_rel, 1),
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

    let record_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed when the authoritative current task-closure raw entry is malformed",
    );
    assert_eq!(record_json["action"], "blocked");
    assert_eq!(record_json["required_follow_up"], "repair_review_state");
    assert!(
        record_json["trace_summary"]
            .as_str()
            .is_some_and(|message| message.contains("prior_task_current_closure_invalid")),
        "record-branch-closure should surface malformed raw current task-closure state through the blocked command envelope, got {record_json}"
    );
}

#[test]
fn plan_execution_record_branch_closure_fails_closed_when_history_backed_current_task_closure_is_invalid()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(
        "plan-execution-record-branch-closure-invalid-history-backed-current-task-closure",
    );
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

    let record_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed when a history-backed current task closure is structurally invalid",
    );
    assert_eq!(record_json["action"], "blocked");
    assert_eq!(record_json["required_follow_up"], "repair_review_state");
    assert!(
        record_json["trace_summary"]
            .as_str()
            .is_some_and(|message| message.contains("prior_task_current_closure_invalid")),
        "record-branch-closure should fail closed when recovered current task-closure truth is structurally invalid, got {record_json}"
    );
}

#[test]
fn plan_execution_record_branch_closure_fails_closed_when_current_task_closure_contract_identity_is_missing()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo(
        "plan-execution-record-branch-closure-current-task-closure-contract-identity-missing",
    );
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

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed when a current task closure is missing contract identity",
    );
    assert_eq!(branch_closure["action"], "blocked");
    assert_eq!(branch_closure["required_follow_up"], "repair_review_state");
    assert_eq!(
        branch_closure["trace_summary"],
        Value::from(
            "record-branch-closure failed closed because prior_task_current_closure_invalid: Task 1 current task closure is malformed or missing authoritative provenance for the active approved plan."
        )
    );
}

#[test]
fn plan_execution_record_branch_closure_re_records_when_contract_identity_changes_after_release_progress()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-branch-closure-contract-identity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let first_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "first record-branch-closure should succeed",
    );
    let first_branch_closure_id = first_branch_closure["branch_closure_id"]
        .as_str()
        .expect("first record-branch-closure should expose branch_closure_id")
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

    let second_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should re-record when branch contract identity changes after release progression",
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
fn plan_execution_record_branch_closure_blocks_re_record_when_drift_escapes_late_stage_surface() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-blocks-untrusted-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let first_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "first record-branch-closure should succeed",
    );
    assert_eq!(first_branch_closure["action"], "recorded");

    append_tracked_repo_line(
        repo,
        "README.md",
        "branch-closure drift outside trusted late-stage surface",
    );

    let second_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "second record-branch-closure should fail closed when drift escapes Late-Stage Surface",
    );
    assert_eq!(second_branch_closure["action"], "blocked");
    assert_eq!(
        second_branch_closure["required_follow_up"],
        "repair_review_state"
    );
}

#[test]
fn plan_execution_record_branch_closure_clears_stale_release_readiness_binding() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-branch-closure-clears-release");
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

    let branch_closure_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should clear stale release-readiness binding",
    );

    let branch_closure_id = branch_closure_json["branch_closure_id"]
        .as_str()
        .expect("record-branch-closure should expose branch_closure_id");
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
fn plan_execution_advance_late_stage_records_release_readiness() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-release-readiness-record");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    let branch_closure_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure for release-readiness fixture",
    );
    assert_eq!(branch_closure_json["action"], "recorded");

    let summary_path = repo.join("release-readiness-summary.md");
    write_file(
        &summary_path,
        "Release readiness is green for the current branch closure.\n",
    );
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
        "advance-late-stage release-readiness command should succeed",
    );

    assert_eq!(release_json["action"], "recorded");
    assert_eq!(release_json["stage_path"], "release_readiness");
    assert_eq!(
        release_json["delegated_primitive"],
        "record-release-readiness"
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
fn plan_execution_record_release_readiness_primitive_records_release_readiness() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-release-readiness-primitive");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    let branch_closure_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure for release-readiness primitive fixture",
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
    let release_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-release-readiness",
            "--plan",
            plan_rel,
            "--branch-closure-id",
            &branch_closure_id,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-release-readiness primitive command should succeed",
    );

    assert_eq!(release_json["action"], "recorded");
    assert_eq!(release_json["stage_path"], "release_readiness");
    assert_eq!(
        release_json["delegated_primitive"],
        "record-release-readiness"
    );
}

#[test]
fn advance_late_stage_release_readiness_ignores_stale_overlay_currentness_from_other_branch_closure()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-release-readiness-stale-overlay-currentness");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let initial_branch = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should create the first authoritative branch closure for stale overlay coverage",
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
    let initial_release = run_plan_execution_json(
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

    let rerun_json = run_plan_execution_json(
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
fn record_release_readiness_primitive_ignores_current_record_from_other_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-release-readiness-primitive-branch-scope");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let initial_branch = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should create the first authoritative branch closure for release-readiness primitive scoping",
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
    let initial_release = run_plan_execution_json(
        repo,
        state,
        &[
            "record-release-readiness",
            "--plan",
            plan_rel,
            "--branch-closure-id",
            &initial_branch_id,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-release-readiness should record the first branch closure outcome before branch-scope currentness coverage",
    );
    assert_eq!(initial_release["action"], "recorded");
    assert_eq!(initial_release["branch_closure_id"], initial_branch_id);

    set_current_branch_closure(repo, state, "branch-release-closure-2");
    let rerun_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-release-readiness",
            "--plan",
            plan_rel,
            "--branch-closure-id",
            "branch-release-closure-2",
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-release-readiness should scope already_current checks to the current branch closure",
    );
    assert_eq!(rerun_json["action"], "recorded", "json: {rerun_json}");
    assert_eq!(
        rerun_json["branch_closure_id"],
        Value::from("branch-release-closure-2")
    );
}

#[test]
fn plan_execution_record_release_readiness_primitive_rejects_overlay_only_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-release-readiness-overlay-only-closure");
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
    let release_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-release-readiness",
            "--plan",
            plan_rel,
            "--branch-closure-id",
            &branch_closure_id,
            "--result",
            "ready",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-release-readiness should fail closed when only overlay branch-closure truth remains",
    );

    assert_eq!(release_json["action"], "blocked");
    assert_eq!(release_json["code"], Value::Null);
    assert_eq!(release_json["required_follow_up"], "record_branch_closure");
}

#[test]
fn plan_execution_advance_late_stage_release_readiness_rerun_stays_idempotent_after_workflow_reroute()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-release-readiness-idempotency");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    let branch_closure_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure for release-readiness idempotency fixture",
    );
    assert_eq!(branch_closure_json["action"], "recorded");

    let summary_path = repo.join("release-readiness-summary.md");
    write_file(
        &summary_path,
        "Release readiness is green for the current branch closure.\n",
    );
    let first = run_plan_execution_json(
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
    let second = run_plan_execution_json(
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
    let conflicting = run_plan_execution_json(
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
            "featureforge plan execution advance-late-stage --plan {plan_rel} --result ready|blocked --summary-file <path>"
        ))
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
            "featureforge plan execution record-qa --plan {plan_rel} --result pass|fail --summary-file <path>"
        ))
    );
}

#[test]
fn plan_execution_record_branch_closure_allows_already_current_for_release_blocker_resolution_required()
 {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-branch-closure-idempotent-release-blocker");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before blocked release-readiness idempotency coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");

    let summary_path = repo.join("release-blocker-summary.md");
    write_file(
        &summary_path,
        "Release readiness is blocked on an external dependency.\n",
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
        "advance-late-stage should record blocked release readiness before branch-closure idempotency coverage",
    );
    assert_eq!(blocked["action"], "recorded");

    let rerun = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should stay idempotent while release blocker resolution remains the active prerelease lane",
    );
    assert_eq!(rerun["action"], "already_current", "json: {rerun}");
    assert!(rerun["code"].is_null(), "json: {rerun}");
    assert!(rerun["recommended_command"].is_null(), "json: {rerun}");
    assert!(
        rerun["rederive_via_workflow_operator"].is_null(),
        "json: {rerun}"
    );
    assert_eq!(rerun["required_follow_up"], Value::Null, "json: {rerun}");
}

#[test]
fn workflow_operator_routes_manual_test_plan_generator_change_to_refresh_lane() {
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
    assert_eq!(operator_json["phase_detail"], "test_plan_refresh_required");
    assert_eq!(operator_json["review_state_status"], "clean");
    assert_eq!(operator_json["qa_requirement"], "required");
    assert_eq!(operator_json["next_action"], "refresh test plan");
    assert!(operator_json["recommended_command"].is_null());
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
    assert_eq!(status_json["phase_detail"], "test_plan_refresh_required");
    assert_eq!(status_json["next_action"], "refresh test plan");
    assert!(status_json["recommended_command"].is_null());
    assert_eq!(status_json["qa_requirement"], "required");
    assert_eq!(status_json["follow_up_override"], "none");
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
fn plan_execution_gate_review_out_of_phase_requires_workflow_requery() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-gate-review-out-of-phase-requery");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should fail closed with the shared out-of-phase contract before release readiness is current",
    );

    assert_eq!(gate_review["allowed"], false);
    assert_eq!(gate_review["action"], "blocked");
    assert_eq!(gate_review["code"], "out_of_phase_requery_required");
    assert_eq!(
        gate_review["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(
        gate_review["rederive_via_workflow_operator"],
        Value::Bool(true)
    );
}

#[test]
fn gate_review_recommends_repair_review_state_when_current_branch_reviewed_state_is_missing() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-gate-review-malformed-current-branch-reviewed-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    republish_fixture_late_stage_truth_for_branch_closure(repo, state, "branch-release-closure");

    let mut payload = authoritative_harness_state(repo, state);
    payload["branch_closure_records"]["branch-release-closure"]["reviewed_state_id"] =
        Value::from(format!("git_tree:{}", current_head_sha(repo)));
    write_authoritative_harness_state(repo, state, &payload);

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should recommend repair-review-state when the current branch reviewed-state binding is unusable",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_reviewed_state_id_missing")),
        "gate-review should expose current_branch_reviewed_state_id_missing, got {gate_review}"
    );
    assert_eq!(
        gate_review["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn plan_execution_gate_finish_out_of_phase_requires_workflow_requery() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-gate-finish-out-of-phase-requery");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should fail closed with the shared out-of-phase contract before release readiness is current",
    );

    assert_eq!(gate_finish["allowed"], false);
    assert_eq!(gate_finish["action"], "blocked");
    assert_eq!(gate_finish["code"], "out_of_phase_requery_required");
    assert_eq!(
        gate_finish["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(
        gate_finish["rederive_via_workflow_operator"],
        Value::Bool(true)
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
    assert_eq!(operator_json["follow_up_override"], "record_pivot");
    assert_eq!(operator_json["next_action"], "pivot / return to planning");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge workflow record-pivot --plan {plan_rel} --reason <reason>"
        ))
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should match missing QA Requirement pivot routing",
    );
    assert_eq!(status_json["phase_detail"], "planning_reentry_required");
    assert_eq!(status_json["follow_up_override"], "record_pivot");
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
    assert_eq!(operator_json["follow_up_override"], "record_pivot");
    assert_eq!(operator_json["next_action"], "pivot / return to planning");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge workflow record-pivot --plan {plan_rel} --reason <reason>"
        ))
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should match invalid QA Requirement pivot routing",
    );
    assert_eq!(status_json["phase_detail"], "planning_reentry_required");
    assert_eq!(status_json["follow_up_override"], "record_pivot");
    assert_eq!(status_json["next_action"], "pivot / return to planning");
}

#[test]
fn empty_lineage_late_stage_exemption_ignores_current_task_closures_that_only_cover_exempt_surface()
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
    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for empty-lineage late-stage exemption fixture",
    );
    let preflight = run_plan_execution_json(
        repo,
        state,
        &["preflight", "--plan", plan_rel],
        "plan execution preflight for empty-lineage late-stage exemption fixture",
    );
    assert_eq!(preflight["allowed"], Value::Bool(true));
    let begin = run_plan_execution_json(
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
    let _ = run_plan_execution_json(
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
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);

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

    let status_json = run_plan_execution_json(
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
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
fn gate_finish_allows_not_required_qa_without_current_test_plan_artifact() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("gate-finish-no-test-plan-when-qa-not-required");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_ready_for_finish_case(repo, state, plan_rel, &base_branch);
    remove_branch_test_plan_artifact(repo, state);

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

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should fail closed when the finish-review gate checkpoint is missing",
    );
    assert_eq!(gate_finish["allowed"], false);
    assert_eq!(
        gate_finish["reason_codes"],
        Value::from(vec![String::from("finish_review_gate_checkpoint_missing")])
    );

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should persist the finish-review gate checkpoint before gate-finish",
    );
    assert_eq!(gate_review["allowed"], true);

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should allow branch completion once the finish-review gate checkpoint is current",
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
    assert_eq!(operator_json["next_action"], "record branch closure");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn plan_execution_record_qa_records_browser_qa_result() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa");
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
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa command should succeed",
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
fn plan_execution_record_qa_fail_returns_execution_reentry_follow_up() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-fail");
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
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa fail command should return authoritative follow-up",
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
    assert_eq!(operator_after_fail["follow_up_override"], "none");
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
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(operator_json["review_state_status"], "stale_unreviewed");
    assert_eq!(
        operator_json["next_action"],
        "repair review state / reenter execution"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn plan_execution_record_qa_fail_prefers_pivot_override() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-pivot-override");
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
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa fail should prefer pivot override over execution reentry",
    );

    assert_eq!(qa_json["action"], "recorded", "json: {qa_json}");
    assert_eq!(
        qa_json["required_follow_up"], "record_pivot",
        "json: {qa_json}"
    );

    let operator_after_fail = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route failed QA to pivot when override is active",
    );
    assert_eq!(operator_after_fail["phase"], "pivot_required");
    assert_eq!(
        operator_after_fail["phase_detail"],
        "planning_reentry_required"
    );
    assert_eq!(operator_after_fail["follow_up_override"], "record_pivot");
}

#[test]
fn plan_execution_record_qa_same_state_rerun_stays_idempotent_and_conflicts_fail_closed() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-idempotent-rerun");
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

    let second = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "seeded same-state failing record-qa rerun should stay idempotent once execution reentry is required",
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

    let conflict = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "conflicting same-state record-qa rerun should also fail closed out of phase",
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
fn plan_execution_record_qa_missing_current_test_plan_fails_before_summary_validation() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-refresh-summary-order");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    remove_branch_test_plan_artifact(repo, state);
    update_authoritative_harness_state(
        repo,
        state,
        &[(
            "current_branch_closure_id",
            Value::from("branch-release-closure"),
        )],
    );

    let missing_summary_path = repo.join("missing-qa-summary.md");
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            missing_summary_path
                .to_str()
                .expect("summary path should be utf-8"),
        ],
        "out-of-phase record-qa should block before summary validation",
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
fn plan_execution_record_qa_prefers_valid_current_test_plan_candidate() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-valid-test-plan-candidate");
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
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa should bind the validated current test-plan candidate rather than the newest stale decoy",
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
        "record-qa should not bind the newest stale test-plan decoy when a validated current candidate exists",
    );
}

#[test]
fn plan_execution_record_qa_requeries_when_base_branch_resolution_invalidates_current_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-base-branch-unresolved");
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
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa should reroute through workflow/operator when the current branch closure is no longer valid after base-branch resolution breaks",
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
fn plan_execution_advance_late_stage_final_review_rejects_branch_closure_id_argument() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-final-review-rejects-branch-closure-arg");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    mark_current_branch_closure_release_ready(repo, state, "branch-release-closure");
    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "plan execution final review dispatch for branch-closure arg rejection fixture",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(true));
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
        "workflow operator json for final review branch-closure arg rejection fixture",
    );
    let dispatch_id = operator_json["recording_context"]["dispatch_id"]
        .as_str()
        .expect("final review branch-closure arg rejection fixture should expose dispatch_id");
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
            "--dispatch-id",
            dispatch_id,
            "--branch-closure-id",
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
        "final-review advance-late-stage should reject --branch-closure-id\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("branch_closure_id"),
        "stderr should mention branch_closure_id: {stderr}"
    );
}

#[test]
fn plan_execution_record_qa_same_state_rerun_requeries_when_test_plan_refresh_is_required() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-refresh-lane-rerun");
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
        "workflow operator after the same-state QA fixture reroutes to test-plan refresh",
    );
    assert_eq!(operator_json["phase"], "qa_pending");
    assert_eq!(operator_json["phase_detail"], "test_plan_refresh_required");
    assert_eq!(operator_json["next_action"], "refresh test plan");
    assert!(operator_json["recommended_command"].is_null());

    let rerun = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "same-state rerun should fail closed through workflow/operator when the latest test plan must be refreshed",
    );
    assert_eq!(rerun["action"], Value::from("blocked"));
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
    assert_eq!(
        rerun["rederive_via_workflow_operator"],
        Value::Bool(true),
        "json: {rerun}"
    );
    assert_eq!(rerun["required_follow_up"], Value::Null);
}

#[test]
fn plan_execution_record_qa_same_summary_on_new_branch_closure_records_again() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-new-closure");
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
    let first = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "fail",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "initial record-qa invocation for closure A should record",
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
    let second = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
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
fn plan_execution_record_qa_missing_current_test_plan_reroutes_through_operator() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-missing-test-plan");
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

    let summary_path = repo.join("qa-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed against the approved test plan.\n",
    );

    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa should reroute through workflow/operator when the current test-plan artifact is missing",
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
fn plan_execution_record_qa_after_repair_reroute_requires_operator_requery() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-stale-unreviewed");
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
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa should record before stale-unreviewed repo changes",
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
        ))
    );
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should surface the exact stale-unreviewed reroute",
    );
    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(repair_json["required_follow_up"], "execution_reentry");
    assert_eq!(
        repair_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );

    let blocked = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa should fail closed through workflow/operator once repair-review-state already rerouted the stale QA state back to execution",
    );
    assert_eq!(blocked["action"], "blocked");
    assert_eq!(blocked["required_follow_up"], Value::Null);
    assert_eq!(blocked["code"], "out_of_phase_requery_required");
    assert_eq!(
        blocked["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
    assert_eq!(blocked["rederive_via_workflow_operator"], true);
}

#[test]
fn plan_execution_repair_review_state_reroutes_late_stage_surface_only_drift_to_branch_closure() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-repair-review-state-late-stage-reroute");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");
    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should establish a real current branch closure before late-stage reroute coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();
    let summary_path = repo.join("release-readiness-late-stage-reroute.md");
    write_file(
        &summary_path,
        "Release readiness passed before trusted late-stage-only drift.\n",
    );
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
        "advance-late-stage should record release readiness before trusted late-stage drift coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    append_tracked_repo_line(repo, "README.md", "late-stage-only branch drift");
    let prerepair_blocked = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should stay blocked until repair-review-state establishes the confined late-stage reroute",
    );
    assert_eq!(prerepair_blocked["action"], "blocked");
    assert_eq!(
        prerepair_blocked["required_follow_up"],
        "repair_review_state"
    );

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should reroute trusted late-stage-only drift to branch closure re-recording",
    );

    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(
        repair_json["required_follow_up"], "record_branch_closure",
        "json: {repair_json:?}"
    );
    assert_eq!(
        repair_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
    assert!(
        repair_json["stale_unreviewed_closures"]
            .as_array()
            .expect("repair-review-state should expose stale_unreviewed_closures")
            .iter()
            .any(|value| value == &Value::from(branch_closure_id.clone())),
        "repair-review-state should continue surfacing the stale branch closure that fell behind workspace movement"
    );
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
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        operator_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should preserve the same confined late-stage reroute back to branch closure recording",
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
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let rerecord_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should proceed after repair-review-state routes confined late-stage drift back to branch closure recording",
    );
    assert_eq!(rerecord_json["action"], "recorded", "json: {rerecord_json}");
    assert_ne!(
        rerecord_json["branch_closure_id"],
        Value::from(branch_closure_id),
        "late-stage-only branch drift should produce a new current branch closure"
    );
}

#[test]
fn workflow_operator_does_not_preserve_persisted_branch_reroute_after_drift_escapes_late_stage_surface()
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

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should establish a current branch closure before persisted reroute confinement coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");

    let summary_path = repo.join("release-readiness-reroute-reset-summary.md");
    write_file(
        &summary_path,
        "Release readiness passed before persisted reroute confinement reset coverage.\n",
    );
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
    assert_eq!(repair_json["required_follow_up"], "record_branch_closure");

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
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_operator_task_scope_repair_outranks_persisted_branch_reroute() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-task-scope-outranks-branch-reroute");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should establish a current branch closure before persisted reroute precedence coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");

    append_tracked_repo_line(repo, "README.md", "late-stage-only drift before reroute");
    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should persist a branch reroute before task-scope precedence coverage",
    );
    assert_eq!(repair_json["required_follow_up"], "record_branch_closure");

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
fn record_branch_closure_task_scope_repair_outranks_persisted_branch_reroute() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("record-branch-closure-task-scope-outranks-branch-reroute");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should establish a current branch closure before direct reroute precedence coverage",
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
    assert_eq!(repair_json["required_follow_up"], "record_branch_closure");

    let mut payload = authoritative_harness_state(repo, state);
    payload["current_task_closure_records"]["task-1"]["source_plan_revision"] = Value::from(999);
    write_authoritative_harness_state(repo, state, &payload);

    let record_branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed when task-scope repair outranks a persisted branch reroute",
    );
    assert_eq!(record_branch_closure["action"], "blocked");
    assert_eq!(
        record_branch_closure["required_follow_up"],
        "repair_review_state"
    );
}

#[test]
fn workflow_operator_does_not_preserve_persisted_branch_reroute_when_rerecord_baseline_disappears()
{
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-clears-persisted-branch-reroute-when-baseline-disappears");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", "README.md");

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should establish a current branch closure before persisted reroute baseline-loss coverage",
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
    assert_eq!(repair_json["required_follow_up"], "record_branch_closure");

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
    assert_eq!(operator_json["phase"], "executing");
    assert_eq!(operator_json["phase_detail"], "execution_reentry_required");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let record_json = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed once the persisted branch reroute no longer has a rerecord baseline",
    );
    assert_eq!(record_json["action"], "blocked");
    assert_eq!(record_json["required_follow_up"], "repair_review_state");
}

#[test]
fn explain_review_state_preserves_stale_branch_closure_target_when_late_stage_stale() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("explain-review-state-late-stage-stale-closure-target");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("qa-stale-closure-target-summary.md");
    write_file(
        &summary_path,
        "Browser QA passed before stale branch-closure targeting coverage.\n",
    );
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa should succeed before stale-closure targeting coverage",
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

    let explain_json = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should preserve stale branch-closure targeting for late-stage stale state",
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
fn status_and_explain_review_state_share_gate_review_only_final_review_stale_classification() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("status-and-explain-share-gate-review-final-review-stale");
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

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should expose final_review_state_stale without requiring release-doc drift",
    );
    assert_eq!(
        gate_review["allowed"],
        Value::Bool(false),
        "json: {gate_review}"
    );
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "final_review_state_stale")),
        "gate-review should expose final_review_state_stale, got {gate_review}"
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should classify gate-review-only final-review stale state as stale_unreviewed",
    );
    assert_eq!(status_json["review_state_status"], "stale_unreviewed");
    assert_eq!(
        status_json["stale_unreviewed_closures"],
        serde_json::json!(["branch-release-closure"])
    );

    let explain_json = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should classify the same gate-review-only final-review stale state as stale_unreviewed",
    );
    assert_eq!(
        explain_json["stale_unreviewed_closures"],
        serde_json::json!(["branch-release-closure"])
    );
}

#[test]
fn plan_execution_repair_review_state_routes_escaped_drift_to_execution_reentry() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-repair-review-state-escaped-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);
    upsert_plan_header(repo, plan_rel, "Late-Stage Surface", plan_rel);
    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should establish a real current branch closure before escaped-drift coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");
    let summary_path = repo.join("release-readiness-escaped-drift.md");
    write_file(
        &summary_path,
        "Release readiness passed before escaped branch drift.\n",
    );
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
        "advance-late-stage should record release readiness before escaped-drift coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    append_tracked_repo_line(
        repo,
        "README.md",
        "escaped branch drift outside trusted late-stage surface",
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should route escaped late-stage drift back to execution reentry",
    );

    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(repair_json["required_follow_up"], "execution_reentry");
    assert_eq!(
        repair_json["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );
}

#[test]
fn plan_execution_reconcile_review_state_restores_missing_branch_closure_overlay() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-reconcile-review-state-restores-branch-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before reconcile coverage",
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

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should rebuild missing current branch closure overlays",
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
fn plan_execution_reconcile_review_state_restores_branch_overlay_without_branch_closure_markdown() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-reconcile-review-state-restores-authoritative-branch-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before authoritative overlay restore coverage",
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

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should rebuild missing current branch closure overlays from authoritative state",
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
fn plan_execution_reconcile_review_state_preserves_release_readiness_while_restoring_overlay() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-reconcile-review-state-preserves-release-readiness");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before reconcile preservation coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");

    let summary_path = repo.join("release-readiness-summary.md");
    write_file(&summary_path, "Release readiness is still current.\n");
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

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should restore missing overlays without clearing release-readiness",
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
        authoritative_state_after["release_docs_state"],
        Value::from("fresh")
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
                        "contract_identity": task_contract_identity(plan_rel, 1),
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
                        "reviewed_state_id": baseline_tree_id,
                        "contract_identity": task_contract_identity(plan_rel, 2),
                        "effective_reviewed_surface_paths": ["tests/workflow_shell_smoke.rs"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"task 2 current review"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"task 2 current verification"),
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
    assert_eq!(status_json["review_state_status"], "stale_unreviewed");
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
                        "contract_identity": task_contract_identity(plan_rel, 1),
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
                        "contract_identity": task_contract_identity(plan_rel, 2),
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
                        "contract_identity": task_contract_identity(plan_rel, 1),
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
                        "contract_identity": task_contract_identity(plan_rel, 2),
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
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should prefer structural current task-closure failures over stale multi-task drift",
    );
    assert_eq!(repair_json["action"], "blocked");
    assert_eq!(repair_json["required_follow_up"], "execution_reentry");
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
                    && actions.iter().any(|action| {
                        action.as_str() == Some("cleared_current_task_closure_task_2")
                    })
            }),
        "repair-review-state should clear both stale and structurally invalid current task-closure truth before execution reentry, got {repair_json}"
    );
    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["task_closure_record_history"]["task-1-current-closure"]["record_status"],
        Value::from("stale_unreviewed")
    );
    assert_eq!(
        authoritative_state["task_closure_record_history"]["task-2-current-closure"]["record_status"],
        Value::from("historical")
    );
    assert!(
        authoritative_state["strategy_review_dispatch_lineage_history"]
            .as_object()
            .is_some_and(|records| {
                records.values().any(|record| {
                    record["dispatch_id"] == "task-1-current-dispatch"
                        && record["record_status"] == "stale_unreviewed"
                }) && records.values().any(|record| {
                    record["dispatch_id"] == "task-2-current-dispatch"
                        && record["record_status"] == "historical"
                })
            }),
        "repair-review-state should preserve stale-vs-historical task dispatch lineage semantics, got {authoritative_state}"
    );
}

#[test]
fn plan_execution_repair_and_reconcile_do_not_claim_current_when_branch_closure_is_missing() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-missing-current-branch-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let explain = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should describe missing current branch-closure truth instead of claiming the state is already current",
    );
    assert!(
        explain["trace_summary"]
            .as_str()
            .is_some_and(|summary| summary.contains("missing_current_closure")),
        "json: {explain}"
    );

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should fail closed when the active late-stage phase still needs a current branch closure",
    );
    assert_eq!(reconcile["action"], "blocked");
    assert_eq!(
        reconcile["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
    assert_eq!(
        reconcile["actions_performed"],
        Value::from(Vec::<String>::new())
    );

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should fail closed when the active late-stage phase still needs a current branch closure",
    );
    assert_eq!(repair["action"], "blocked");
    assert_eq!(repair["required_follow_up"], "record_branch_closure");
    assert_eq!(
        repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
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
    assert_eq!(repair["required_follow_up"], "execution_reentry");
    assert!(
        repair["actions_performed"]
            .as_array()
            .is_some_and(|actions| actions.iter().any(|action| {
                action.as_str() == Some("cleared_current_task_closure_scope_malformed-scope")
            })),
        "repair-review-state should clear malformed taskless current-task-closure entries by scope key, got {repair}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert!(
        authoritative_state["current_task_closure_records"]
            .as_object()
            .is_some_and(|records| !records.contains_key("malformed-scope")),
        "repair-review-state should remove malformed taskless current-task-closure entries, got {authoritative_state}"
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn workflow_operator_routes_missing_release_readiness_overlay_to_repair_review_state() {
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
        "workflow operator should route missing release-readiness derived state through repair-review-state",
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
fn late_stage_direct_commands_require_repair_review_state_for_clean_structural_release_state() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("late-stage-direct-commands-clean-structural-release-repair");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_with_current_closure_case(repo, state, plan_rel, &base_branch);

    let summary_path = repo.join("release-readiness-clean-repair-summary.md");
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
    let blocked_release = run_plan_execution_json(
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
        "advance-late-stage should preserve repair-review-state as the deterministic blocked follow-up for clean structural release state",
    );
    assert_eq!(blocked_release["action"], "blocked");
    assert_eq!(blocked_release["required_follow_up"], "repair_review_state");
    assert_eq!(blocked_release["code"], Value::Null);

    let qa_summary_path = repo.join("qa-clean-repair-summary.md");
    write_file(
        &qa_summary_path,
        "Browser QA should stay blocked behind review-state repair.\n",
    );
    let blocked_qa = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            qa_summary_path
                .to_str()
                .expect("QA summary path should be utf-8"),
        ],
        "record-qa should preserve repair-review-state as the deterministic blocked follow-up for clean structural late-stage repair states",
    );
    assert_eq!(blocked_qa["action"], "blocked");
    assert_eq!(blocked_qa["required_follow_up"], "repair_review_state");
    assert_eq!(blocked_qa["code"], Value::Null);
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
        "repair-review-state should restore the current release-readiness overlay from authoritative history",
    );
    assert_eq!(repair["action"], "reconciled");
    assert_eq!(repair["required_follow_up"], Value::Null);
    assert_eq!(
        repair["actions_performed"],
        Value::from(vec![String::from(
            "restored_current_release_readiness_overlay"
        )])
    );
    assert_eq!(
        repair["missing_derived_overlays"],
        Value::from(Vec::<String>::new())
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_release_readiness_result"],
        Value::from("ready")
    );
    assert_eq!(
        authoritative_state["release_docs_state"],
        Value::from("fresh")
    );
}

#[test]
fn plan_execution_repair_review_state_reports_reconciled_after_overlay_restore() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-repair-review-state-reconciles-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before repair reconcile coverage",
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

    let repair = run_plan_execution_json(
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
fn plan_execution_repair_review_state_restores_missing_current_task_closure_records() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-task-closure-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch should succeed before current task-closure overlay repair coverage",
    );
    let dispatch_id = dispatch["dispatch_id"]
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
    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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
    let closure_record_id = close_json["closure_record_id"]
        .as_str()
        .expect("task-closure overlay repair fixture should expose closure record id")
        .to_owned();

    update_authoritative_harness_state(
        repo,
        state,
        &[("current_task_closure_records", serde_json::json!({}))],
    );

    let explain = run_plan_execution_json(
        repo,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state should describe missing derivable task-closure overlays instead of claiming the state is already current",
    );
    assert!(
        explain["trace_summary"]
            .as_str()
            .is_some_and(|summary| { summary.contains("derivable overlay fields are missing") }),
        "json: {explain}"
    );

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should restore missing current task-closure overlays from authoritative history",
    );
    assert_eq!(repair["action"], "blocked", "json: {repair}");
    assert_eq!(repair["required_follow_up"], "record_branch_closure");
    assert_eq!(
        repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
    assert!(
        repair["actions_performed"]
            .as_array()
            .is_some_and(|actions| actions
                .iter()
                .any(|action| action == "restored_current_task_closure_records")),
        "repair should restore missing current task-closure overlays, got {repair:?}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_task_closure_records"]["task-1"]["closure_record_id"],
        Value::from(closure_record_id)
    );
}

#[test]
fn plan_execution_repair_review_state_ignores_superseded_task_dispatch_lineage() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-superseded-dispatch-lineage");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let task1_dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "task 1 review dispatch should succeed before superseded repair coverage",
    );
    let task1_dispatch_id = task1_dispatch["dispatch_id"]
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
    run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &task1_dispatch_id,
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

    let status_after_task1 = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose execution fingerprint before task 2 supersession coverage",
    );
    let begin_task2 = run_plan_execution_json(
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
    run_plan_execution_json(
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
    let task2_dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "2",
        ],
        "task 2 review dispatch should succeed before superseded repair coverage",
    );
    let task2_dispatch_id = task2_dispatch["dispatch_id"]
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
    let task2_close = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--dispatch-id",
            &task2_dispatch_id,
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

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should ignore superseded task dispatch lineage when restoring current task overlays",
    );
    assert_eq!(repair["action"], "blocked", "json: {repair}");
    assert_eq!(repair["required_follow_up"], "record_branch_closure");
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
fn plan_execution_repair_review_state_restores_missing_task_closure_negative_result_records() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-task-negative-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch should succeed before failed task-outcome overlay repair coverage",
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
    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should restore missing failed task-outcome overlays from authoritative history",
    );
    assert_eq!(repair["action"], "reconciled", "json: {repair}");
    assert_eq!(repair["required_follow_up"], Value::Null);
    assert!(
        repair["actions_performed"]
            .as_array()
            .is_some_and(|actions| actions
                .iter()
                .any(|action| action == "restored_task_closure_negative_result_records")),
        "repair should restore missing failed task-outcome overlays, got {repair:?}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["task_closure_negative_result_records"]["task-1"]["dispatch_id"],
        Value::from(dispatch_id)
    );
}

#[test]
fn workflow_operator_routes_missing_current_task_closure_overlay_to_repair_review_state() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-missing-task-closure-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch should succeed before task-closure overlay routing coverage",
    );
    let dispatch_id = dispatch["dispatch_id"]
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
    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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

    let status_json = run_plan_execution_json(
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
fn workflow_operator_routes_missing_task_negative_overlay_to_repair_review_state() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("workflow-operator-missing-task-negative-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch should succeed before failed task-outcome overlay routing coverage",
    );
    let dispatch_id = dispatch["dispatch_id"]
        .as_str()
        .expect("failed task-outcome overlay routing fixture should expose dispatch id")
        .to_owned();
    let review_summary_path = repo.join("task-negative-routing-review-summary.md");
    write_file(
        &review_summary_path,
        "Task review found a blocker before negative overlay routing coverage.\n",
    );
    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should route missing task-negative overlays through repair-review-state",
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
        "workflow operator should route missing task-negative overlays through repair-review-state",
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
fn plan_execution_repair_review_state_routes_unrestorable_task_overlay_loss_to_execution_reentry() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-unrestorable-task-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_task_boundary_blocked_case(repo, state, plan_rel, &base_branch);

    let dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "task",
            "--task",
            "1",
        ],
        "record-review-dispatch should succeed before unrestorable task-overlay repair coverage",
    );
    let dispatch_id = dispatch["dispatch_id"]
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
    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
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

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should route unrestorable task overlays back to execution reentry",
    );
    assert_eq!(repair["action"], "blocked", "json: {repair}");
    assert_eq!(
        repair["required_follow_up"], "execution_reentry",
        "json: {repair}"
    );
    assert_eq!(
        repair["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}")),
        "json: {repair}"
    );
}

#[test]
fn plan_execution_repair_review_state_prioritizes_unrestorable_task_overlay_over_late_stage_branch_reroute()
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
    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before mixed repair precedence coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");
    let release_summary_path = repo.join("mixed-repair-precedence-release-summary.md");
    write_file(
        &release_summary_path,
        "Release readiness passed before mixed repair precedence coverage.\n",
    );
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

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should prioritize unrestorable task authority loss over late-stage branch reroute",
    );
    assert_eq!(repair["action"], "blocked", "json: {repair}");
    assert_eq!(
        repair["required_follow_up"], "execution_reentry",
        "json: {repair}"
    );
    assert_eq!(
        repair["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}")),
        "json: {repair}"
    );
}

#[test]
fn workflow_operator_routes_recoverable_missing_current_branch_closure_to_repair_review_state() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("workflow-operator-recoverable-missing-current-branch-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before recoverable current-closure repair coverage",
    );
    let branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();

    let summary_path = repo.join("release-ready-before-current-closure-repair.md");
    write_file(
        &summary_path,
        "Release readiness is current before current-closure repair coverage.\n",
    );
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
        "advance-late-stage should record release readiness before current-closure repair coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    update_authoritative_harness_state(repo, state, &[("current_branch_closure_id", Value::Null)]);

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should route recoverable missing current branch closure through repair-review-state",
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

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should restore a recoverable missing current branch closure binding",
    );
    assert_eq!(repair["action"], "reconciled");
    assert_eq!(repair["required_follow_up"], Value::Null);
    assert!(
        repair["actions_performed"]
            .as_array()
            .is_some_and(|actions| actions
                .iter()
                .any(|action| action == "restored_current_branch_closure_id")),
        "repair should restore the missing current branch closure binding, got {repair:?}"
    );

    let authoritative_state = authoritative_harness_state(repo, state);
    assert_eq!(
        authoritative_state["current_branch_closure_id"],
        Value::from(branch_closure_id)
    );
    assert_eq!(
        authoritative_state["current_release_readiness_result"],
        Value::from("ready")
    );
    assert_eq!(
        authoritative_state["release_docs_state"],
        Value::from("fresh")
    );
}

#[test]
fn malformed_current_branch_closure_reviewed_state_requires_repair_review_state_before_late_stage_progression()
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

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should fail closed when the current branch closure reviewed-state identity is malformed",
    );
    assert_eq!(reconcile["action"], "blocked");
    assert_eq!(
        reconcile["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should fail closed when the current branch closure reviewed-state identity is malformed",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_reviewed_state_id_missing")),
        "gate-finish should reject malformed current branch reviewed-state bindings, got {gate_finish}"
    );
    assert_eq!(
        gate_finish["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should reroute malformed current branch-closure reviewed-state identities back to branch closure recording",
    );
    assert_eq!(repair["action"], "blocked");
    assert_eq!(repair["required_follow_up"], "record_branch_closure");
    assert_eq!(
        repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let post_repair_status = run_plan_execution_json(
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let summary_path = repo.join("release-ready-malformed-branch-closure.md");
    write_file(
        &summary_path,
        "Release readiness should stay blocked until branch closure repair reroutes.\n",
    );
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
        "advance-late-stage should fail closed when the current branch closure reviewed-state identity is malformed",
    );
    assert_eq!(release_json["action"], "blocked");
    assert_eq!(release_json["branch_closure_id"], Value::Null);
    assert_eq!(release_json["required_follow_up"], "record_branch_closure");

    let qa_summary_path = repo.join("qa-malformed-branch-closure.md");
    write_file(
        &qa_summary_path,
        "QA should stay blocked until branch closure repair reroutes.\n",
    );
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            qa_summary_path
                .to_str()
                .expect("qa summary path should be utf-8"),
        ],
        "record-qa should fail closed without exposing an unusable current branch_closure_id when the reviewed-state identity is malformed",
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
fn malformed_current_branch_closure_reconcile_routes_to_repair_when_no_task_baseline_remains() {
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

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should route malformed current branch-closure state through repair-review-state when no still-current task-closure baseline remains",
    );
    assert_eq!(reconcile["action"], "blocked");
    assert_eq!(
        reconcile["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should route malformed current branch-closure state to execution reentry when no still-current task-closure baseline remains",
    );
    assert_eq!(repair["action"], "blocked");
    assert_eq!(repair["required_follow_up"], "execution_reentry");
    assert_eq!(
        repair["recommended_command"],
        Value::from(format!("featureforge workflow operator --plan {plan_rel}"))
    );

    let status_after_repair = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should expose execution reentry after repairing malformed branch-closure state with no baseline",
    );
    assert_eq!(
        status_after_repair["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(status_after_repair["review_state_status"], "clean");
    assert_ne!(
        status_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    assert_ne!(
        status_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let operator_after_repair = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should expose execution reentry after repairing malformed branch-closure state with no baseline",
    );
    assert_eq!(operator_after_repair["phase"], "executing");
    assert_eq!(
        operator_after_repair["phase_detail"],
        "execution_reentry_required"
    );
    assert_eq!(operator_after_repair["review_state_status"], "clean");
    assert_ne!(
        operator_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution repair-review-state --plan {plan_rel}"
        ))
    );
    assert_ne!(
        operator_after_repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn repair_review_state_preserves_branch_reroute_for_structural_branch_damage_with_zero_path_drift()
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

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should expose the stale late-stage artifact even when zero-path branch reroute coverage uses only state mutations",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "final_review_state_stale")),
        "gate-review should expose final_review_state_stale, got {gate_review}"
    );

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should preserve a branch-closure reroute for structural branch damage even when there are zero changed paths",
    );
    assert_eq!(repair["action"], "blocked");
    assert_eq!(repair["required_follow_up"], "record_branch_closure");
    assert_eq!(
        repair["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should keep the persisted branch reroute authoritative even when stale late-stage state has zero changed paths",
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
    assert_eq!(operator_json["next_action"], "record branch closure");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should align with workflow operator for zero-drift structural branch reroutes after repair-review-state",
    );
    assert_eq!(
        status_json["phase_detail"],
        "branch_closure_recording_required_for_release_readiness"
    );
    assert_eq!(
        status_json["review_state_status"],
        "missing_current_closure"
    );
    assert_eq!(status_json["next_action"], "record branch closure");
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
}

#[test]
fn final_review_dispatch_blocks_when_current_branch_closure_overlay_requires_repair() {
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
    let dispatch = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "record-review-dispatch should fail closed when current branch-closure overlay repair is still required",
    );
    assert_eq!(dispatch["allowed"], Value::Bool(false));
    assert_eq!(dispatch["action"], Value::from("blocked"));
    assert!(
        dispatch["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "derived_review_state_missing")),
        "dispatch should surface derived review-state repair as the blocker: {dispatch}"
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
    assert_eq!(
        state_after["final_review_dispatch_lineage"],
        state_before["final_review_dispatch_lineage"]
    );
}

#[test]
fn final_review_dispatch_blocks_when_current_branch_closure_reviewed_state_requires_repair() {
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

    let initial_dispatch = run_plan_execution_json(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "record-review-dispatch should succeed before malformed current branch reviewed-state coverage",
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

    let dispatch = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "record-review-dispatch",
            "--plan",
            plan_rel,
            "--scope",
            "final-review",
        ],
        "record-review-dispatch should fail closed when the current branch closure reviewed state requires repair",
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
fn plan_execution_record_branch_closure_same_id_reassertion_preserves_release_readiness() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-record-branch-closure-reasserts-current-binding");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before same-id reassertion coverage",
    );
    let _branch_closure_id = branch_closure["branch_closure_id"]
        .as_str()
        .expect("branch closure should expose branch_closure_id")
        .to_owned();

    let summary_path = repo.join("release-ready-before-branch-reassertion.md");
    write_file(
        &summary_path,
        "Release readiness is current before same-id branch-closure reassertion coverage.\n",
    );
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
        "advance-late-stage should record release readiness before branch-closure reassertion coverage",
    );
    assert_eq!(release_json["action"], "recorded");

    update_authoritative_harness_state(repo, state, &[("current_branch_closure_id", Value::Null)]);

    let rerecord = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should fail closed before mutating same-id current binding loss",
    );
    assert_eq!(rerecord["action"], "blocked");
    assert_eq!(rerecord["required_follow_up"], "repair_review_state");
    assert_eq!(rerecord["code"], Value::Null);

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
        Value::from("fresh")
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
        Value::from("record branch closure")
    );
    assert_eq!(
        status_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
    assert!(
        status_json["blocking_records"]
            .as_array()
            .is_some_and(|records| records.iter().any(|record| {
                record["code"] == "missing_current_closure"
                    && record["record_type"] == "branch_closure"
                    && record["required_follow_up"] == "record_branch_closure"
            })),
        "status should surface the missing current branch closure blocker when the authoritative record is absent: {status_json}"
    );
}

#[test]
fn incomplete_current_branch_closure_record_fails_closed_across_public_and_finish_surfaces() {
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

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should fail closed when the current branch closure record is incomplete",
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should fail closed when the current branch closure record is incomplete",
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should fail closed when the current branch closure record is incomplete",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert_eq!(
        gate_review["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-review should reject incomplete current branch-closure truth before finish readiness can proceed, got {gate_review}"
    );

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should fail closed when the current branch closure record is incomplete",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert_eq!(
        gate_finish["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-finish should reject incomplete current branch-closure truth, got {gate_finish}"
    );
}

#[test]
fn empty_lineage_late_stage_exemption_record_without_exempt_surface_fails_closed() {
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

    let status_json = run_plan_execution_json(
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

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should reject empty-lineage exemption branch closure truth without a valid late-stage-only surface",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-finish should reject invalid empty-lineage exemption branch-closure truth, got {gate_finish}"
    );
}

#[test]
fn empty_lineage_late_stage_exemption_subset_surface_stays_current_across_public_and_finish_surfaces()
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
    let status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for late-stage exemption subset fixture",
    );
    let preflight = run_plan_execution_json(
        repo,
        state,
        &["preflight", "--plan", plan_rel],
        "plan execution preflight for late-stage exemption subset fixture",
    );
    assert_eq!(preflight["allowed"], Value::Bool(true));
    let begin = run_plan_execution_json(
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
    let _ = run_plan_execution_json(
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
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
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

    let status_json = run_plan_execution_json(
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

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should accept a valid subset late-stage-surface exemption branch closure before gate-finish",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(true));

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should accept a valid subset late-stage-surface exemption branch closure",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(true));
}

#[test]
fn current_branch_closure_record_with_wrong_plan_revision_fails_closed_across_public_and_finish_surfaces()
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

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should fail closed when the current branch closure record is not bound to the active approved plan revision",
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should fail closed when the current branch closure record is not bound to the active approved plan revision",
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
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should reject current branch-closure truth that is not bound to the active approved plan revision",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-review should reject wrong-plan current branch-closure truth before finish readiness can proceed, got {gate_review}"
    );

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should reject current branch-closure truth that is not bound to the active approved plan revision",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-finish should reject wrong-plan current branch-closure truth, got {gate_finish}"
    );
}

#[test]
fn current_branch_closure_record_with_wrong_repository_context_fails_closed_across_public_and_finish_surfaces()
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

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should fail closed when the current branch closure record is not bound to the active repository context",
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
        "workflow operator should fail closed when the current branch closure record is not bound to the active repository context",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should reject current branch-closure truth that is not bound to the active repository context",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-review should reject wrong-context current branch-closure truth before finish readiness can proceed, got {gate_review}"
    );

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should reject current branch-closure truth that is not bound to the active repository context",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-finish should reject wrong-context current branch-closure truth, got {gate_finish}"
    );
}

#[test]
fn current_branch_closure_record_with_wrong_contract_identity_fails_closed_across_public_and_finish_surfaces()
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

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should fail closed when the current branch closure contract identity is corrupted",
    );
    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should fail closed when the current branch closure contract identity is corrupted",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should reject a corrupted current branch closure contract identity",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-finish should reject corrupted current branch-closure identity, got {gate_finish}"
    );

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should reject a corrupted current branch closure contract identity",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-review should reject corrupted current branch-closure identity, got {gate_review}"
    );
}

#[test]
fn current_branch_closure_record_with_wrong_source_task_lineage_fails_closed_across_public_and_finish_surfaces()
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

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should fail closed when the current branch closure lineage does not match still-current task closures",
    );
    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should fail closed when the current branch closure lineage does not match still-current task closures",
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

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should reject a corrupted current branch closure lineage set",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-review should reject corrupted current branch-closure lineage, got {gate_review}"
    );

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should reject a corrupted current branch closure lineage set",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-finish should reject corrupted current branch-closure lineage, got {gate_finish}"
    );
}

#[test]
fn current_branch_closure_record_with_invalid_reviewed_surface_fails_closed_across_public_and_finish_surfaces()
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

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should fail closed when the current branch closure reviewed surface is not runtime-owned",
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
        "workflow operator should fail closed when the current branch closure reviewed surface is not runtime-owned",
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

    let gate_review = run_plan_execution_json(
        repo,
        state,
        &["gate-review", "--plan", plan_rel],
        "gate-review should reject a current branch closure reviewed surface that is not runtime-owned",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    assert!(
        gate_review["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-review should reject invalid current branch-closure reviewed surfaces, got {gate_review}"
    );

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should reject a current branch closure reviewed surface that is not runtime-owned",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-finish should reject invalid current branch-closure reviewed surfaces, got {gate_finish}"
    );
}

#[test]
fn current_branch_closure_record_missing_required_arrays_fails_closed_across_public_and_finish_surfaces()
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

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status should fail closed when the current branch closure record omits required provenance arrays",
    );
    assert!(status_json["current_branch_closure_id"].is_null());
    assert_eq!(
        status_json["review_state_status"],
        Value::from("missing_current_closure")
    );

    let operator_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        &[],
        "workflow operator should fail closed when the current branch closure record omits required provenance arrays",
    );
    assert_eq!(operator_json["phase"], "document_release_pending");
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("branch_closure_recording_required_for_release_readiness")
    );
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge plan execution record-branch-closure --plan {plan_rel}"
        ))
    );

    let gate_finish = run_plan_execution_json(
        repo,
        state,
        &["gate-finish", "--plan", plan_rel],
        "gate-finish should fail closed when the current branch closure record omits required provenance arrays",
    );
    assert_eq!(gate_finish["allowed"], Value::Bool(false));
    assert!(
        gate_finish["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "current_branch_closure_id_missing")),
        "gate-finish should reject current branch-closure truth missing required provenance arrays, got {gate_finish}"
    );
}

#[test]
fn plan_execution_repair_review_state_restores_overlay_from_authoritative_branch_record() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-blocks-unrestorable-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before unrestorable repair coverage",
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

    let repair = run_plan_execution_json(
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
fn plan_execution_repair_review_state_blocks_when_only_branch_closure_markdown_remains() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-repair-review-state-markdown-only-branch-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before markdown-only repair coverage",
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

    let repair = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state should fail closed when only derived branch-closure markdown remains",
    );

    assert_eq!(repair["action"], "blocked");
    assert_eq!(repair["required_follow_up"], "record_branch_closure");
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
fn plan_execution_reconcile_review_state_restores_missing_branch_overlay_while_stale() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) =
        init_repo("plan-execution-reconcile-review-state-restores-stale-branch-overlay");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before stale reconcile coverage",
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

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should restore derivable branch overlays even when the branch state is stale",
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
fn plan_execution_reconcile_review_state_stale_only_does_not_claim_restore() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-reconcile-review-state-stale-only");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let branch_closure = run_plan_execution_json(
        repo,
        state,
        &["record-branch-closure", "--plan", plan_rel],
        "record-branch-closure should succeed before stale-only reconcile coverage",
    );
    assert_eq!(branch_closure["action"], "recorded");
    append_tracked_repo_line(
        repo,
        "README.md",
        "stale reconcile without overlay corruption",
    );

    let reconcile = run_plan_execution_json(
        repo,
        state,
        &["reconcile-review-state", "--plan", plan_rel],
        "reconcile-review-state should not claim overlay restoration when no derived overlays were missing",
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
fn plan_execution_record_qa_blocks_when_test_plan_refresh_is_required() {
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let (repo_dir, state_dir) = init_repo("plan-execution-record-qa-refresh-required");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    setup_qa_pending_case(repo, state, plan_rel, &base_branch);
    remove_branch_test_plan_artifact(repo, state);
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
    let qa_json = run_plan_execution_json(
        repo,
        state,
        &[
            "record-qa",
            "--plan",
            plan_rel,
            "--result",
            "pass",
            "--summary-file",
            summary_path.to_str().expect("summary path should be utf-8"),
        ],
        "record-qa command should fail closed when test-plan refresh is required",
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
fn workflow_operator_routes_pivot_required_to_record_pivot() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-pivot-plan-block");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    write_file(
        &authoritative_state_path,
        r#"{"harness_phase":"pivot_required","latest_authoritative_sequence":23,"reason_codes":["blocked_on_plan_revision"]}"#,
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
    assert_eq!(operator_json["follow_up_override"], "record_pivot");
    assert_eq!(
        operator_json["recommended_command"],
        Value::from(format!(
            "featureforge workflow record-pivot --plan {plan_rel} --reason <reason>"
        ))
    );
}

fn display_json_array(value: &Value) -> String {
    value
        .as_array()
        .map(|items| {
            if items.is_empty() {
                String::from("none")
            } else {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        })
        .unwrap_or_else(|| String::from("none"))
}

fn display_json_optional_str(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| String::from("none"))
}

#[test]
fn workflow_handoff_and_doctor_text_and_json_surfaces_match_harness_evaluator_and_reason_metadata()
{
    let (repo_dir, state_dir) = init_repo("workflow-doctor-handoff-metadata-parity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    setup_document_release_pending_case(repo, state, plan_rel, &base_branch);

    let doctor_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "doctor", "--json"],
        &[],
        "workflow doctor json for shell-smoke metadata parity",
    );
    let handoff_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "handoff", "--json"],
        &[],
        "workflow handoff json for shell-smoke metadata parity",
    );
    let doctor_text_output = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "doctor"],
        &[],
        "workflow doctor text for shell-smoke metadata parity",
    );
    assert!(doctor_text_output.status.success());
    let doctor_text = String::from_utf8_lossy(&doctor_text_output.stdout);
    let handoff_text_output = run_featureforge_with_env(
        repo,
        state,
        &["workflow", "handoff"],
        &[],
        "workflow handoff text for shell-smoke metadata parity",
    );
    assert!(handoff_text_output.status.success());
    let handoff_text = String::from_utf8_lossy(&handoff_text_output.stdout);

    let execution_status = doctor_json["execution_status"]
        .as_object()
        .expect("workflow doctor json should expose execution_status object");
    let write_authority_state = execution_status
        .get("write_authority_state")
        .and_then(Value::as_str)
        .expect("workflow doctor json should expose write_authority_state");
    let write_authority_holder =
        display_json_optional_str(execution_status.get("write_authority_holder"));
    let write_authority_worktree =
        display_json_optional_str(execution_status.get("write_authority_worktree"));
    let reason_codes = display_json_array(
        execution_status
            .get("reason_codes")
            .expect("workflow doctor json should expose reason_codes"),
    );
    let required_evaluators = display_json_array(
        execution_status
            .get("required_evaluator_kinds")
            .expect("workflow doctor json should expose required_evaluator_kinds"),
    );
    let completed_evaluators = display_json_array(
        execution_status
            .get("completed_evaluator_kinds")
            .expect("workflow doctor json should expose completed_evaluator_kinds"),
    );
    let pending_evaluators = display_json_array(
        execution_status
            .get("pending_evaluator_kinds")
            .expect("workflow doctor json should expose pending_evaluator_kinds"),
    );
    let non_passing_evaluators = display_json_array(
        execution_status
            .get("non_passing_evaluator_kinds")
            .expect("workflow doctor json should expose non_passing_evaluator_kinds"),
    );
    let last_evaluator =
        display_json_optional_str(execution_status.get("last_evaluation_evaluator_kind"));
    let finish_reason_codes = display_json_array(
        doctor_json["gate_finish"]
            .get("reason_codes")
            .expect("workflow doctor json should expose gate_finish reason_codes"),
    );

    assert!(doctor_text.contains(&format!(
        "Phase: {}",
        doctor_json["phase"]
            .as_str()
            .expect("workflow doctor json should expose phase"),
    )));
    assert!(doctor_text.contains(&format!(
        "Next action: {}",
        doctor_json["next_action"]
            .as_str()
            .expect("workflow doctor json should expose next_action"),
    )));
    assert!(doctor_text.contains(&format!("Execution reason codes: {reason_codes}")));
    assert!(doctor_text.contains(&format!("Evaluator required kinds: {required_evaluators}")));
    assert!(doctor_text.contains(&format!(
        "Evaluator completed kinds: {completed_evaluators}"
    )));
    assert!(doctor_text.contains(&format!("Evaluator pending kinds: {pending_evaluators}")));
    assert!(doctor_text.contains(&format!(
        "Evaluator non-passing kinds: {non_passing_evaluators}"
    )));
    assert!(doctor_text.contains(&format!("Evaluator last kind: {last_evaluator}")));
    assert!(doctor_text.contains(&format!("Write authority state: {write_authority_state}")));
    assert!(doctor_text.contains(&format!("Write authority holder: {write_authority_holder}")));
    assert!(doctor_text.contains(&format!(
        "Write authority worktree: {write_authority_worktree}"
    )));
    assert!(doctor_text.contains(&format!("Finish gate reason codes: {finish_reason_codes}")));

    assert!(handoff_text.contains(&format!(
        "Phase: {}",
        handoff_json["phase"]
            .as_str()
            .expect("workflow handoff json should expose phase"),
    )));
    assert!(handoff_text.contains(&format!(
        "Next action: {}",
        handoff_json["next_action"]
            .as_str()
            .expect("workflow handoff json should expose next_action"),
    )));
    assert!(handoff_text.contains(&format!("Execution reason codes: {reason_codes}")));
    assert!(handoff_text.contains(&format!("Evaluator required kinds: {required_evaluators}")));
    assert!(handoff_text.contains(&format!("Write authority state: {write_authority_state}")));
    assert!(handoff_text.contains(&format!("Write authority holder: {write_authority_holder}")));
    assert!(handoff_text.contains(&format!(
        "Write authority worktree: {write_authority_worktree}"
    )));
    assert!(handoff_text.contains(&format!(
        "Reason: {}",
        handoff_json["recommendation_reason"]
            .as_str()
            .expect("workflow handoff json should expose recommendation_reason")
    )));
}

#[test]
fn workflow_phase_doctor_handoff_json_parity_for_pivot_required_plan_revision_block() {
    let (repo_dir, state_dir) = init_repo("workflow-shell-smoke-pivot-plan-block");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo, state), &current_branch_name(repo));
    write_file(
        &authoritative_state_path,
        r#"{"harness_phase":"pivot_required","latest_authoritative_sequence":23,"reason_codes":["blocked_on_plan_revision"]}"#,
    );

    let phase_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "phase", "--json"],
        &[],
        "workflow phase json for shell-smoke pivot plan-block parity",
    );
    let doctor_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "doctor", "--json"],
        &[],
        "workflow doctor json for shell-smoke pivot plan-block parity",
    );
    let handoff_json = run_featureforge_with_env_json(
        repo,
        state,
        &["workflow", "handoff", "--json"],
        &[],
        "workflow handoff json for shell-smoke pivot plan-block parity",
    );

    assert_eq!(phase_json["phase"], "pivot_required");
    assert_eq!(doctor_json["phase"], phase_json["phase"]);
    assert_eq!(handoff_json["phase"], phase_json["phase"]);
    assert_eq!(phase_json["next_action"], "pivot / return to planning");
    assert_eq!(doctor_json["next_action"], phase_json["next_action"]);
    assert_eq!(handoff_json["next_action"], phase_json["next_action"]);
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
