#[path = "support/failure_json.rs"]
mod failure_json_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/git.rs"]
mod git_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/public_featureforge_cli.rs"]
mod public_featureforge_cli;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use featureforge::contracts::plan::{PLAN_FIDELITY_REQUIRED_SURFACES, parse_plan_file};
use featureforge::contracts::spec::parse_spec_file;
use featureforge::git::{discover_slug_identity, sha256_hex};
use featureforge::paths::{harness_authoritative_artifact_path, harness_state_path};
use serde_json::{Map, Value, json};
use tempfile::TempDir;

use failure_json_support::parse_failure_json;
use files_support::write_file;

const EXEC_SPEC_REL: &str = "docs/featureforge/specs/runtime-golden-execution-design.md";
const EXEC_PLAN_REL: &str = "docs/featureforge/plans/runtime-golden-execution-plan.md";
const GOLDEN_PATH: &str = "tests/fixtures/runtime-goldens/public-runtime-routes.json";

const REQUIRED_SCENARIOS: &[&str] = &[
    "not_started",
    "begin_ready",
    "active_task",
    "completed_step_waiting_task_closure",
    "task_closure_recording_ready",
    "repair_review_state_required",
    "stale_targetless_reconcile",
    "blocked_runtime_bug_diagnostic",
    "release_readiness_pending",
    "final_review_pending",
    "qa_pending",
    "ready_for_branch_completion",
    "implementation_ready_after_fidelity_pass",
    "engineering_approved_missing_fidelity_gate",
];

struct PublicCli<'a> {
    repo: &'a Path,
    state: &'a Path,
    home_dir: PathBuf,
    codex_home: PathBuf,
}

impl<'a> PublicCli<'a> {
    fn new(repo: &'a Path, state: &'a Path) -> Self {
        let home_dir = repo.join(".runtime-home");
        let codex_home = home_dir.join(".codex");
        fs::create_dir_all(&codex_home).expect("runtime golden home should be creatable");
        Self {
            repo,
            state,
            home_dir,
            codex_home,
        }
    }

    fn capture(&self, args: &[&str], envs: &[(&str, &str)], context: &str) -> Value {
        let codex_home = self
            .codex_home
            .to_str()
            .expect("runtime golden CODEX_HOME should be utf-8");
        let mut merged_envs = Vec::with_capacity(envs.len() + 1);
        merged_envs.push(("CODEX_HOME", codex_home));
        merged_envs.extend_from_slice(envs);
        let output = public_featureforge_cli::run_featureforge_with_env_control_real_cli(
            Some(self.repo),
            Some(self.state),
            Some(&self.home_dir),
            &[],
            &merged_envs,
            args,
            context,
        );
        if output.status.success() {
            let json: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
                panic!(
                    "{context} should emit JSON on stdout: {error}\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )
            });
            json!({
                "ok": true,
                "json": json
            })
        } else {
            json!({
                "ok": false,
                "failure": parse_failure_json(&output, context)
            })
        }
    }

    fn json(&self, args: &[&str], context: &str) -> Value {
        let capture = self.capture(args, &[], context);
        assert!(
            capture["ok"].as_bool() == Some(true),
            "{context} should succeed: {capture}"
        );
        capture["json"].clone()
    }
}

fn init_repo(name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();
    git_support::init_repo_with_initial_commit(repo, &format!("# {name}\n"), "init");
    run_git(
        repo,
        &["checkout", "-b", "feature/runtime-golden"],
        "checkout runtime golden branch",
    );
    (repo_dir, state_dir)
}

fn run_git(repo: &Path, args: &[&str], context: &str) {
    let mut command = Command::new("git");
    command.current_dir(repo).args(args);
    let output = process_support::run(command, context);
    assert!(
        output.status.success(),
        "{context} should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn commit_all(repo: &Path, message: &str) {
    run_git(repo, &["add", "-A"], "git add runtime golden fixture");
    run_git(
        repo,
        &[
            "-c",
            "user.name=FeatureForge Test",
            "-c",
            "user.email=featureforge-tests@example.com",
            "commit",
            "--no-gpg-sign",
            "--no-verify",
            "-m",
            message,
        ],
        "git commit runtime golden fixture",
    );
}

fn write_execution_spec_and_plan(
    repo: &Path,
    task_count: u32,
    qa_requirement: &str,
    include_fidelity: bool,
) {
    write_file(
        &repo.join(EXEC_SPEC_REL),
        concat!(
            "# Runtime Golden Execution Spec\n\n",
            "**Workflow State:** CEO Approved\n",
            "**Spec Revision:** 1\n",
            "**Last Reviewed By:** plan-ceo-review\n\n",
            "## Requirement Index\n\n",
            "- [REQ-001][behavior] Runtime public routing remains stable.\n",
            "- [REQ-002][behavior] Late-stage gates remain stable.\n",
        ),
    );

    let dependency_diagram = if task_count == 1 {
        String::from("Task 1")
    } else {
        (1..=task_count)
            .map(|task| format!("Task {task}"))
            .collect::<Vec<_>>()
            .join(" -> ")
    };
    let task_list = (1..=task_count)
        .map(|task| format!("Task {task}"))
        .collect::<Vec<_>>()
        .join(", ");
    let coverage = format!("- REQ-001 -> {task_list}\n- REQ-002 -> {task_list}\n");
    let strategy = (1..=task_count)
        .map(|task| {
            if task == 1 {
                String::from(
                    "- Execute Task 1 serially. It establishes the runtime boundary before follow-on work begins.",
                )
            } else {
                format!(
                    "- Execute Task {task} serially after Task {}. It validates downstream routing after the prior task closure.",
                    task - 1
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut tasks = String::new();
    for task in 1..=task_count {
        tasks.push_str(&format!(
            "\n## Task {task}: Runtime golden task {task}\n\n\
             **Spec Coverage:** REQ-001, REQ-002\n\
             **Goal:** Exercise public runtime route {task}.\n\n\
             **Context:**\n\
             - Spec Coverage: REQ-001, REQ-002.\n\n\
             **Constraints:**\n\
             - Keep the fixture deterministic and public-command only.\n\n\
             **Done when:**\n\
             - Public status and operator routes remain stable for task {task}.\n\n\
             **Files:**\n\
             - Modify: `docs/runtime-golden-output-{task}.md`\n\
             - Test: `cargo test --test runtime_behavior_golden`\n\n\
             - [ ] **Step 1: Update runtime golden output {task}**\n"
        ));
        write_file(
            &repo.join(format!("docs/runtime-golden-output-{task}.md")),
            &format!("Runtime golden output {task} before execution.\n"),
        );
    }
    write_file(
        &repo.join(EXEC_PLAN_REL),
        &format!(
            "# Runtime Golden Execution Plan\n\n\
             **Workflow State:** Engineering Approved\n\
             **Plan Revision:** 1\n\
             **Execution Mode:** none\n\
             **Source Spec:** `{EXEC_SPEC_REL}`\n\
             **Source Spec Revision:** 1\n\
             **Last Reviewed By:** plan-eng-review\n\
             **QA Requirement:** {qa_requirement}\n\n\
             ## Requirement Coverage Matrix\n\n\
             {coverage}\n\
             ## Execution Strategy\n\n\
             {strategy}\n\n\
             ## Dependency Diagram\n\n\
             ```text\n\
             {dependency_diagram}\n\
             ```\n\
             {tasks}"
        ),
    );
    if include_fidelity {
        write_current_pass_plan_fidelity_review_artifact(
            repo,
            ".featureforge/reviews/runtime-golden-plan-fidelity.md",
            EXEC_PLAN_REL,
            EXEC_SPEC_REL,
        );
    }
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
    write_file(
        &artifact_path,
        &format!(
            "## Plan Fidelity Review Summary\n\n\
             **Review Stage:** featureforge:plan-fidelity-review\n\
             **Review Verdict:** pass\n\
             **Reviewed Plan:** `{plan_rel}`\n\
             **Reviewed Plan Revision:** {}\n\
             **Reviewed Plan Fingerprint:** {plan_fingerprint}\n\
             **Reviewed Spec:** `{spec_rel}`\n\
             **Reviewed Spec Revision:** {}\n\
             **Reviewed Spec Fingerprint:** {spec_fingerprint}\n\
             **Reviewer Source:** fresh-context-subagent\n\
             **Reviewer ID:** runtime-golden-plan-fidelity-reviewer\n\
             **Distinct From Stages:** featureforge:writing-plans, featureforge:plan-eng-review\n\
             **Verified Surfaces:** {}\n\
             **Verified Requirement IDs:** {}\n",
            plan.plan_revision,
            spec.spec_revision,
            PLAN_FIDELITY_REQUIRED_SURFACES.join(", "),
            verified_requirement_ids.join(", "),
        ),
    );
}

fn append_repo_file(repo: &Path, rel: &str, line: &str) {
    let path = repo.join(rel);
    let mut source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture file `{}` should read: {error}", path.display()));
    if !source.ends_with('\n') {
        source.push('\n');
    }
    source.push_str(line);
    source.push('\n');
    write_file(&path, &source);
}

fn plan_status(cli: &PublicCli<'_>, context: &str) -> Value {
    cli.json(
        &["plan", "execution", "status", "--plan", EXEC_PLAN_REL],
        context,
    )
}

fn begin_task(cli: &PublicCli<'_>, task: u32, fingerprint: &str, context: &str) -> Value {
    cli.json(
        &[
            "plan",
            "execution",
            "begin",
            "--plan",
            EXEC_PLAN_REL,
            "--task",
            &task.to_string(),
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            fingerprint,
        ],
        context,
    )
}

fn complete_task(cli: &PublicCli<'_>, task: u32, fingerprint: &str, context: &str) -> Value {
    cli.json(
        &[
            "plan",
            "execution",
            "complete",
            "--plan",
            EXEC_PLAN_REL,
            "--task",
            &task.to_string(),
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            &format!("Completed runtime golden task {task}."),
            "--manual-verify-summary",
            &format!("Verified runtime golden task {task}."),
            "--file",
            &format!("docs/runtime-golden-output-{task}.md"),
            "--expect-execution-fingerprint",
            fingerprint,
        ],
        context,
    )
}

fn close_task(cli: &PublicCli<'_>, _repo: &Path, task: u32, context: &str) -> Value {
    let review_summary = cli
        .state
        .join(format!("runtime-golden-task-{task}-review.md"));
    let verification_summary = cli
        .state
        .join(format!("runtime-golden-task-{task}-verification.md"));
    write_file(
        &review_summary,
        &format!("Runtime golden task {task} review passed.\n"),
    );
    write_file(
        &verification_summary,
        &format!("Runtime golden task {task} verification passed.\n"),
    );
    cli.json(
        &[
            "plan",
            "execution",
            "close-current-task",
            "--plan",
            EXEC_PLAN_REL,
            "--task",
            &task.to_string(),
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
        context,
    )
}

fn advance_branch_closure(cli: &PublicCli<'_>) {
    let output = cli.json(
        &[
            "plan",
            "execution",
            "advance-late-stage",
            "--plan",
            EXEC_PLAN_REL,
        ],
        "runtime golden record branch closure",
    );
    assert_eq!(
        output["stage_path"], "branch_closure",
        "branch closure advance should record branch closure: {output}"
    );
}

fn advance_release_readiness(cli: &PublicCli<'_>) {
    let summary = cli
        .state
        .join("runtime-golden-release-readiness-summary.md");
    write_file(&summary, "Runtime golden release readiness is ready.\n");
    let output = cli.json(
        &[
            "plan",
            "execution",
            "advance-late-stage",
            "--plan",
            EXEC_PLAN_REL,
            "--result",
            "ready",
            "--summary-file",
            summary
                .to_str()
                .expect("release summary path should be utf-8"),
        ],
        "runtime golden record release readiness",
    );
    assert_eq!(
        output["stage_path"], "release_readiness",
        "release-readiness advance should record release readiness: {output}"
    );
}

fn materialize_projections(cli: &PublicCli<'_>, context: &str) {
    let output = cli.json(
        &[
            "plan",
            "execution",
            "materialize-projections",
            "--plan",
            EXEC_PLAN_REL,
        ],
        context,
    );
    assert_eq!(
        output["action"], "materialized",
        "materialize-projections should refresh explicit projections: {output}"
    );
}

fn publish_authoritative_final_review_truth(repo: &Path, state: &Path, qa_required: bool) {
    let identity = discover_slug_identity(repo);
    let state_path = harness_state_path(state, &identity.repo_slug, &identity.branch_name);
    let mut payload =
        featureforge::execution::event_log::load_reduced_authoritative_state_for_tests(&state_path)
            .unwrap_or_else(|error| {
                panic!(
                    "event-authoritative runtime golden state `{}` should reduce: {}",
                    state_path.display(),
                    error.message
                )
            })
            .unwrap_or_else(|| {
                serde_json::from_str(&fs::read_to_string(&state_path).unwrap_or_else(|error| {
                    panic!(
                        "authoritative runtime golden state `{}` should read: {error}",
                        state_path.display()
                    )
                }))
                .expect("authoritative runtime golden state should parse")
            });

    let current_branch_closure_id = json_string(&payload, "current_branch_closure_id")
        .unwrap_or_else(|| {
            panic!("runtime golden state should expose current_branch_closure_id: {payload}")
        });
    let current_release_readiness_record_id =
        json_string(&payload, "current_release_readiness_record_id").unwrap_or_else(|| {
            panic!(
                "runtime golden state should expose current_release_readiness_record_id: {payload}"
            )
        });
    let branch_record = payload
        .get("branch_closure_records")
        .and_then(Value::as_object)
        .and_then(|records| records.get(&current_branch_closure_id))
        .unwrap_or_else(|| {
            panic!(
                "runtime golden state should expose branch closure record `{current_branch_closure_id}`: {payload}"
            )
        });
    let reviewed_state_id = json_string(branch_record, "reviewed_state_id")
        .expect("branch closure record should expose reviewed_state_id");
    let base_branch = json_string(branch_record, "base_branch")
        .expect("branch closure record should expose base_branch");
    let semantic_reviewed_state_id = json_string(branch_record, "semantic_reviewed_state_id");

    let final_review_summary = "Runtime golden final review fixture passed.";
    let final_review_summary_hash = sha256_hex(final_review_summary.as_bytes());
    let final_review_source = format!(
        "# Runtime Golden Final Review\n\n\
         **Source Plan:** `{EXEC_PLAN_REL}`\n\
         **Source Plan Revision:** 1\n\
         **Branch:** {}\n\
         **Repo:** {}\n\
         **Base Branch:** {base_branch}\n\
         **Result:** pass\n\n\
         ## Summary\n\
         - {final_review_summary}\n",
        identity.branch_name, identity.repo_slug,
    );
    let final_review_fingerprint = sha256_hex(final_review_source.as_bytes());
    let final_review_record_id = format!("final-review-record-{final_review_fingerprint}");
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &identity.repo_slug,
            &identity.branch_name,
            &format!("final-review-{final_review_fingerprint}.md"),
        ),
        &final_review_source,
    );

    let object = payload
        .as_object_mut()
        .expect("authoritative runtime golden state should be an object");
    object.insert("dependency_index_state".to_owned(), json!("fresh"));
    object.insert("final_review_state".to_owned(), json!("fresh"));
    object.insert("browser_qa_state".to_owned(), json!("not_required"));
    object.insert(
        "last_final_review_artifact_fingerprint".to_owned(),
        json!(final_review_fingerprint.clone()),
    );
    object.insert(
        "current_final_review_branch_closure_id".to_owned(),
        json!(current_branch_closure_id.clone()),
    );
    object.insert(
        "current_final_review_dispatch_id".to_owned(),
        json!("runtime-golden-final-review-dispatch"),
    );
    object.insert(
        "current_final_review_reviewer_source".to_owned(),
        json!("fresh-context-subagent"),
    );
    object.insert(
        "current_final_review_reviewer_id".to_owned(),
        json!("runtime-golden-final-reviewer"),
    );
    object.insert("current_final_review_result".to_owned(), json!("pass"));
    object.insert(
        "current_final_review_summary_hash".to_owned(),
        json!(final_review_summary_hash.clone()),
    );
    object.insert(
        "current_final_review_record_id".to_owned(),
        json!(final_review_record_id.clone()),
    );
    object.insert(
        "final_review_record_history".to_owned(),
        json!({
            final_review_record_id.clone(): {
                "record_id": final_review_record_id,
                "record_sequence": 1,
                "record_status": "current",
                "branch_closure_id": current_branch_closure_id.clone(),
                "release_readiness_record_id": current_release_readiness_record_id,
                "dispatch_id": "runtime-golden-final-review-dispatch",
                "reviewer_source": "fresh-context-subagent",
                "reviewer_id": "runtime-golden-final-reviewer",
                "result": "pass",
                "final_review_fingerprint": final_review_fingerprint,
                "browser_qa_required": qa_required,
                "source_plan_path": EXEC_PLAN_REL,
                "source_plan_revision": 1,
                "repo_slug": identity.repo_slug,
                "branch_name": identity.branch_name,
                "base_branch": base_branch,
                "reviewed_state_id": reviewed_state_id,
                "semantic_reviewed_state_id": semantic_reviewed_state_id,
                "summary": final_review_summary,
                "summary_hash": final_review_summary_hash
            }
        }),
    );
    object.insert(
        "finish_review_gate_pass_branch_closure_id".to_owned(),
        json!(current_branch_closure_id),
    );
    write_file(
        &state_path,
        &serde_json::to_string(&payload).expect("runtime golden state should serialize"),
    );
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(&state_path, &payload)
        .expect("runtime golden final-review fixture should sync typed event authority");
}

fn json_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn run_to_completed_task(cli: &PublicCli<'_>, repo: &Path, task: u32) -> Value {
    let status = plan_status(cli, "runtime golden status before begin");
    let begin = begin_task(
        cli,
        task,
        status["execution_fingerprint"]
            .as_str()
            .expect("status should expose execution fingerprint"),
        "runtime golden begin task",
    );
    append_repo_file(
        repo,
        &format!("docs/runtime-golden-output-{task}.md"),
        &format!("Runtime golden task {task} changed output."),
    );
    complete_task(
        cli,
        task,
        begin["execution_fingerprint"]
            .as_str()
            .expect("begin should expose execution fingerprint"),
        "runtime golden complete task",
    )
}

fn run_to_closed_task(cli: &PublicCli<'_>, repo: &Path, task: u32) {
    run_to_completed_task(cli, repo, task);
    commit_all(
        repo,
        &format!("runtime golden commit completed task {task} output"),
    );
    let close = close_task(cli, repo, task, "runtime golden close task");
    assert_eq!(
        close["action"], "recorded",
        "close-current-task should record closure: {close}"
    );
}

fn capture_route_state(
    label: &str,
    note: &str,
    cli: &PublicCli<'_>,
    envs: &[(&str, &str)],
) -> Value {
    let status = cli.capture(
        &["plan", "execution", "status", "--plan", EXEC_PLAN_REL],
        envs,
        &format!("runtime golden {label} plan execution status"),
    );
    let operator = cli.capture(
        &["workflow", "operator", "--plan", EXEC_PLAN_REL, "--json"],
        envs,
        &format!("runtime golden {label} workflow operator"),
    );
    let workflow_status = cli.capture(
        &["workflow", "status", "--json"],
        envs,
        &format!("runtime golden {label} workflow status"),
    );
    json!({
        "label": label,
        "note": note,
        "plan_execution_status": normalize_value_for_paths(status, cli.repo, cli.state),
        "workflow_operator": normalize_value_for_paths(operator, cli.repo, cli.state),
        "workflow_status": normalize_value_for_paths(workflow_status, cli.repo, cli.state),
    })
}

fn assert_capture_json_field(capture: &Value, pointer: &str, expected: &str, context: &str) {
    assert_eq!(
        capture.pointer(pointer).and_then(Value::as_str),
        Some(expected),
        "{context} should expose {pointer}={expected}: {capture}"
    );
}

fn assert_capture_json_contains_reason(capture: &Value, reason: &str, context: &str) {
    let contains_reason = capture
        .pointer("/json/blocking_reason_codes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|value| value == reason)
        || capture
            .pointer("/json/reason_codes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|value| value == reason);
    assert!(
        contains_reason,
        "{context} should expose reason code `{reason}`: {capture}"
    );
}

fn assert_capture_recommends(capture: &Value, command_fragment: &str, context: &str) {
    assert!(
        capture
            .pointer("/json/recommended_command")
            .and_then(Value::as_str)
            .is_some_and(|command| command.contains(command_fragment)),
        "{context} should recommend `{command_fragment}`: {capture}"
    );
}

fn assert_capture_requires_task_closure_inputs(capture: &Value, context: &str) {
    assert!(
        capture
            .pointer("/json/recommended_command")
            .is_none_or(Value::is_null),
        "{context} should not expose a placeholder task-closure command: {capture}"
    );
    assert_eq!(
        capture.pointer("/json/required_inputs"),
        Some(&json!([
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
        ])),
        "{context} should expose typed task-closure inputs: {capture}"
    );
}

fn collect_execution_progress_scenarios(scenarios: &mut Vec<Value>) {
    let (repo_dir, state_dir) = init_repo("runtime-golden-progress");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_execution_spec_and_plan(repo, 2, "not-required", true);
    commit_all(repo, "runtime golden progress fixture");
    let cli = PublicCli::new(repo, state);

    let not_started = capture_route_state(
        "not_started",
        "approved plan with current fidelity before any execution command",
        &cli,
        &[],
    );
    assert_capture_json_field(
        &not_started["plan_execution_status"],
        "/json/execution_started",
        "no",
        "not_started status",
    );
    scenarios.push(not_started);

    let initial_status = plan_status(&cli, "runtime golden active flow status");
    let begin = begin_task(
        &cli,
        1,
        initial_status["execution_fingerprint"]
            .as_str()
            .expect("status should expose execution fingerprint"),
        "runtime golden active flow begin",
    );
    let active_task = capture_route_state(
        "active_task",
        "Task 1 has begun and remains active",
        &cli,
        &[],
    );
    assert_eq!(
        active_task["plan_execution_status"]["json"]["active_task"],
        json!(1),
        "active_task status should expose active_task=1: {active_task}"
    );
    scenarios.push(active_task);

    append_repo_file(
        repo,
        "docs/runtime-golden-output-1.md",
        "Runtime golden Task 1 completed output.",
    );
    let complete = complete_task(
        &cli,
        1,
        begin["execution_fingerprint"]
            .as_str()
            .expect("begin should expose execution fingerprint"),
        "runtime golden active flow complete",
    );
    let completed = capture_route_state(
        "completed_step_waiting_task_closure",
        "Task 1 Step 1 completion is recorded and waits for public task closure",
        &cli,
        &[],
    );
    assert_capture_json_field(
        &completed["plan_execution_status"],
        "/json/phase_detail",
        "task_closure_recording_ready",
        "completed step status",
    );
    let completed = with_extra(
        completed,
        "completion_output",
        normalize_value_for_paths(json!({"ok": true, "json": complete}), repo, state),
    );
    scenarios.push(completed);

    let task_closure_ready = capture_route_state(
        "task_closure_recording_ready",
        "Workflow operator projects the same completed task as public close-current-task",
        &cli,
        &[],
    );
    assert_capture_json_field(
        &task_closure_ready["workflow_operator"],
        "/json/phase_detail",
        "task_closure_recording_ready",
        "task closure operator",
    );
    assert_capture_requires_task_closure_inputs(
        &task_closure_ready["workflow_operator"],
        "task closure operator",
    );
    scenarios.push(task_closure_ready);

    let close = close_task(&cli, repo, 1, "runtime golden progress close task 1");
    assert_eq!(
        close["action"], "recorded",
        "Task 1 close should record closure: {close}"
    );
    let begin_ready = capture_route_state(
        "begin_ready",
        "Task 1 closure is current and Task 2 is ready to begin",
        &cli,
        &[],
    );
    assert_capture_recommends(
        &begin_ready["plan_execution_status"],
        "begin --plan",
        "begin_ready status",
    );
    scenarios.push(begin_ready);

    append_repo_file(
        repo,
        "docs/runtime-golden-output-1.md",
        "Post-closure drift requires public repair-review-state.",
    );
    let repair_required = capture_route_state(
        "repair_review_state_required",
        "A stale current task closure routes through public repair-review-state",
        &cli,
        &[],
    );
    assert_capture_recommends(
        &repair_required["workflow_operator"],
        "repair-review-state",
        "repair_review_state_required operator",
    );
    scenarios.push(repair_required);
}

fn with_extra(mut value: Value, key: &str, extra: Value) -> Value {
    value
        .as_object_mut()
        .expect("scenario should be an object")
        .insert(key.to_owned(), extra);
    value
}

fn collect_invariant_diagnostic_scenarios(scenarios: &mut Vec<Value>) {
    let (repo_dir, state_dir) = init_repo("runtime-golden-diagnostics");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_execution_spec_and_plan(repo, 2, "not-required", true);
    commit_all(repo, "runtime golden diagnostics fixture");
    let cli = PublicCli::new(repo, state);

    let targetless = capture_route_state(
        "stale_targetless_reconcile",
        "Invariant-injected stale review state has no target and must not fabricate current scope",
        &cli,
        &[(
            "FEATUREFORGE_PLAN_EXECUTION_READ_INVARIANT_TEST_INJECTION",
            "targetless_stale_unreviewed",
        )],
    );
    assert_capture_json_contains_reason(
        &targetless["plan_execution_status"],
        "stale_unreviewed_target_missing",
        "targetless stale status",
    );
    scenarios.push(targetless);

    let hidden_command = capture_route_state(
        "blocked_runtime_bug_diagnostic",
        "Invariant-injected hidden recommended command fails closed before exposure",
        &cli,
        &[(
            "FEATUREFORGE_PLAN_EXECUTION_READ_INVARIANT_TEST_INJECTION",
            "hidden_recommended_command",
        )],
    );
    assert_capture_json_field(
        &hidden_command["plan_execution_status"],
        "/json/state_kind",
        "blocked_runtime_bug",
        "hidden command status",
    );
    assert_capture_json_contains_reason(
        &hidden_command["workflow_operator"],
        "recommended_command_hidden_or_debug",
        "hidden command operator",
    );
    scenarios.push(hidden_command);
}

fn collect_review_gate_scenarios(scenarios: &mut Vec<Value>) {
    let (repo_dir, state_dir) = init_repo("runtime-golden-missing-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_execution_spec_and_plan(repo, 2, "not-required", false);
    commit_all(repo, "runtime golden missing fidelity fixture");
    let cli = PublicCli::new(repo, state);
    let missing_fidelity = capture_route_state(
        "engineering_approved_missing_fidelity_gate",
        "Engineering Approved plans without a current fidelity artifact route back to engineering review",
        &cli,
        &[],
    );
    assert_capture_json_field(
        &missing_fidelity["workflow_status"],
        "/json/status",
        "plan_review_required",
        "missing fidelity workflow status",
    );
    assert_capture_json_contains_reason(
        &missing_fidelity["workflow_status"],
        "engineering_approval_missing_plan_fidelity_review",
        "missing fidelity workflow status",
    );
    scenarios.push(missing_fidelity);

    let (repo_dir, state_dir) = init_repo("runtime-golden-implementation-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_execution_spec_and_plan(repo, 2, "not-required", true);
    commit_all(repo, "runtime golden implementation ready fixture");
    let cli = PublicCli::new(repo, state);
    let implementation_ready = capture_route_state(
        "implementation_ready_after_fidelity_pass",
        "Engineering Approved plan with current fidelity routes to implementation",
        &cli,
        &[],
    );
    assert_capture_json_field(
        &implementation_ready["workflow_status"],
        "/json/status",
        "implementation_ready",
        "implementation ready workflow status",
    );
    scenarios.push(implementation_ready);
}

fn collect_late_stage_scenarios(scenarios: &mut Vec<Value>) {
    let (repo_dir, state_dir) = init_repo("runtime-golden-late-stage");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_execution_spec_and_plan(repo, 2, "not-required", true);
    commit_all(repo, "runtime golden late-stage fixture");
    let cli = PublicCli::new(repo, state);
    run_to_closed_task(&cli, repo, 1);
    run_to_closed_task(&cli, repo, 2);
    advance_branch_closure(&cli);

    let release_pending = capture_route_state(
        "release_readiness_pending",
        "Current branch closure is recorded and release-readiness evidence is pending",
        &cli,
        &[],
    );
    assert_capture_json_field(
        &release_pending["plan_execution_status"],
        "/json/phase_detail",
        "release_readiness_recording_ready",
        "release readiness status",
    );
    scenarios.push(release_pending);

    advance_release_readiness(&cli);
    materialize_projections(&cli, "runtime golden materialize after release readiness");
    let final_review_pending = capture_route_state(
        "final_review_pending",
        "Release-readiness is current and final review dispatch/result is pending",
        &cli,
        &[],
    );
    assert_capture_json_field(
        &final_review_pending["workflow_operator"],
        "/json/phase",
        "final_review_pending",
        "final review operator",
    );
    scenarios.push(final_review_pending);

    publish_authoritative_final_review_truth(repo, state, false);
    let ready = capture_route_state(
        "ready_for_branch_completion",
        "Release-readiness and final review are current for a plan without required QA",
        &cli,
        &[],
    );
    assert_capture_json_field(
        &ready["workflow_operator"],
        "/json/phase",
        "ready_for_branch_completion",
        "ready for branch completion operator",
    );
    scenarios.push(ready);

    let (repo_dir, state_dir) = init_repo("runtime-golden-qa-pending");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_execution_spec_and_plan(repo, 2, "required", true);
    commit_all(repo, "runtime golden QA fixture");
    let cli = PublicCli::new(repo, state);
    run_to_closed_task(&cli, repo, 1);
    run_to_closed_task(&cli, repo, 2);
    advance_branch_closure(&cli);
    advance_release_readiness(&cli);
    publish_authoritative_final_review_truth(repo, state, true);
    let qa_pending = capture_route_state(
        "qa_pending",
        "Release-readiness and final review are current but required QA is still pending",
        &cli,
        &[],
    );
    assert_capture_json_field(
        &qa_pending["workflow_operator"],
        "/json/phase",
        "qa_pending",
        "QA pending operator",
    );
    scenarios.push(qa_pending);
}

fn collect_runtime_golden() -> Value {
    let mut scenarios = Vec::new();
    collect_execution_progress_scenarios(&mut scenarios);
    collect_invariant_diagnostic_scenarios(&mut scenarios);
    collect_review_gate_scenarios(&mut scenarios);
    collect_late_stage_scenarios(&mut scenarios);
    assert_required_scenarios(&scenarios);
    json!({
        "schema_version": 1,
        "normalization": [
            "absolute temp repo and state paths are replaced",
            "run/chunk ids are replaced",
            "git shas and sha256 fingerprints are replaced",
            "timestamps and generated artifact timestamp slugs are replaced"
        ],
        "scenarios": scenarios
    })
}

fn assert_required_scenarios(scenarios: &[Value]) {
    let actual = scenarios
        .iter()
        .map(|scenario| {
            scenario["label"]
                .as_str()
                .expect("scenario should expose a label")
                .to_owned()
        })
        .collect::<BTreeSet<_>>();
    let expected = REQUIRED_SCENARIOS
        .iter()
        .map(|label| (*label).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual, expected,
        "runtime behavior goldens must cover every Task 8 representative state"
    );
}

fn normalize_value_for_paths(value: Value, repo: &Path, state: &Path) -> Value {
    normalize_value(value, repo, state)
}

fn normalize_value(value: Value, repo: &Path, state: &Path) -> Value {
    match value {
        Value::String(value) => Value::String(normalize_string(&value, repo, state)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| normalize_value(value, repo, state))
                .collect(),
        ),
        Value::Object(object) => {
            let mut entries = object.into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let mut normalized = Map::new();
            for (key, value) in entries {
                let normalized_value = if key == "recommended_public_command_argv" {
                    normalize_public_argv_value(value)
                } else {
                    normalize_value(value, repo, state)
                };
                normalized.insert(key, normalized_value);
            }
            Value::Object(normalized)
        }
        value => value,
    }
}

fn normalize_public_argv_value(value: Value) -> Value {
    match value {
        Value::String(value) => Value::String(normalize_public_argv_string(&value)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(normalize_public_argv_value)
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, normalize_public_argv_value(value)))
                .collect(),
        ),
        value => value,
    }
}

fn normalize_public_argv_string(value: &str) -> String {
    let normalized = replace_hex_runs(
        value,
        64,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    replace_hex_runs(&normalized, 40, "0000000000000000000000000000000000000000")
}

fn normalize_string(value: &str, repo: &Path, state: &Path) -> String {
    let mut normalized = value.to_owned();
    for (path, replacement) in runtime_provenance_path_replacements() {
        normalized = normalized.replace(path, replacement);
    }
    let canonical_repo = repo.canonicalize().ok();
    let canonical_state = state.canonicalize().ok();
    for (path, replacement) in [
        (canonical_repo.as_deref(), "<REPO>"),
        (canonical_state.as_deref(), "<STATE>"),
    ] {
        if let Some(path) = path {
            let path_string = path.to_string_lossy();
            normalized = normalized.replace(path_string.as_ref(), replacement);
        }
    }
    for (path, replacement) in [(repo, "<REPO>"), (state, "<STATE>")] {
        let path_string = path.to_string_lossy();
        normalized = normalized.replace(path_string.as_ref(), replacement);
    }
    normalized = replace_temp_project_slugs(&normalized);
    normalized = replace_runtime_manifest_user_tokens(&normalized);
    normalized = replace_prefixed_hex(&normalized, "run-", 16, "run-<RUN_ID>");
    normalized = replace_prefixed_hex(&normalized, "chunk-", 16, "chunk-<CHUNK_ID>");
    normalized = replace_prefixed_hex(
        &normalized,
        "chunk-pending-",
        12,
        "chunk-pending-<CHUNK_ID>",
    );
    normalized = replace_prefixed_hex(&normalized, "branch-closure-", 16, "branch-closure-<ID>");
    normalized = replace_prefixed_hex(&normalized, "task-closure-", 16, "task-closure-<ID>");
    normalized = replace_prefixed_hex(
        &normalized,
        "release-readiness-",
        16,
        "release-readiness-<ID>",
    );
    normalized = replace_prefixed_digits(&normalized, "feature-runtime-golden-", "<BRANCH_TS>");
    normalized = replace_iso_utc_timestamps(&normalized);
    normalized = replace_hex_runs(&normalized, 64, "<SHA256>");
    replace_hex_runs(&normalized, 40, "<GIT_SHA>")
}

fn runtime_provenance_path_replacements() -> &'static [(String, &'static str)] {
    static REPLACEMENTS: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();
    REPLACEMENTS
        .get_or_init(|| {
            let mut replacements = Vec::new();
            append_runtime_path_replacement(
                &mut replacements,
                Path::new(env!("CARGO_BIN_EXE_featureforge")),
                "<FEATUREFORGE_BIN>",
            );
            append_runtime_path_replacement(
                &mut replacements,
                Path::new(env!("CARGO_MANIFEST_DIR")),
                "<FEATUREFORGE_RUNTIME_ROOT>",
            );
            replacements.sort_by(|(left_path, _), (right_path, _)| {
                right_path
                    .len()
                    .cmp(&left_path.len())
                    .then_with(|| left_path.cmp(right_path))
            });
            replacements.dedup_by(|(left_path, _), (right_path, _)| left_path == right_path);
            replacements
        })
        .as_slice()
}

fn append_runtime_path_replacement(
    replacements: &mut Vec<(String, &'static str)>,
    path: &Path,
    replacement: &'static str,
) {
    replacements.push((path.to_string_lossy().into_owned(), replacement));
    if let Ok(canonical) = path.canonicalize() {
        replacements.push((canonical.to_string_lossy().into_owned(), replacement));
    }
}

fn runtime_manifest_user_tokens() -> BTreeSet<String> {
    ["USER", "LOGNAME"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .chain(std::iter::once(String::from("user")))
        .filter(|value| !value.trim().is_empty())
        .collect()
}

fn replace_runtime_manifest_user_tokens(input: &str) -> String {
    let mut normalized = input.to_owned();
    for user_name in runtime_manifest_user_tokens() {
        normalized = normalized.replace(
            &format!("{user_name}-feature-runtime-golden-"),
            "<USER>-feature-runtime-golden-",
        );
    }
    normalized
}

fn assert_runtime_manifest_user_tokens_normalized(value: &Value) {
    let source = serde_json::to_string(value).expect("golden should serialize for normalization");
    for user_name in runtime_manifest_user_tokens() {
        if user_name == "user" {
            continue;
        }
        assert!(
            !source.contains(&format!("{user_name}-feature-runtime-golden-")),
            "runtime goldens must normalize manifest USER token `{user_name}`"
        );
    }
}

fn replace_temp_project_slugs(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let chars = input.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if starts_with_chars(&chars, index, ".tmp") {
            let mut end = index + ".tmp".len();
            while chars
                .get(end)
                .is_some_and(|value| value.is_ascii_alphanumeric())
            {
                end += 1;
            }
            if chars.get(end) == Some(&'-') {
                end += 1;
                let hex_start = end;
                while chars
                    .get(end)
                    .is_some_and(|value| value.is_ascii_hexdigit())
                {
                    end += 1;
                }
                if end - hex_start >= 8 {
                    output.push_str("<PROJECT_KEY>");
                    index = end;
                    continue;
                }
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn replace_prefixed_hex(input: &str, prefix: &str, len: usize, replacement: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let chars = input.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if starts_with_chars(&chars, index, prefix)
            && chars
                .get(index + prefix.len()..index + prefix.len() + len)
                .is_some_and(|slice| slice.iter().all(|value| value.is_ascii_hexdigit()))
        {
            output.push_str(replacement);
            index += prefix.len() + len;
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    output
}

fn replace_prefixed_digits(input: &str, prefix: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let chars = input.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if starts_with_chars(&chars, index, prefix) {
            let mut end = index + prefix.len();
            while chars.get(end).is_some_and(|value| value.is_ascii_digit()) {
                end += 1;
            }
            if end > index + prefix.len() {
                output.push_str(prefix);
                output.push_str(replacement);
                index = end;
                continue;
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn replace_hex_runs(input: &str, minimum_len: usize, replacement: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let chars = input.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_ascii_hexdigit() {
            let start = index;
            while chars
                .get(index)
                .is_some_and(|value| value.is_ascii_hexdigit())
            {
                index += 1;
            }
            if index - start >= minimum_len {
                output.push_str(replacement);
            } else {
                output.extend(&chars[start..index]);
            }
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    output
}

fn replace_iso_utc_timestamps(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let chars = input.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if timestamp_at(&chars, index) {
            output.push_str("<TIMESTAMP>");
            index += "0000-00-00T00:00:00Z".len();
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    output
}

fn timestamp_at(chars: &[char], index: usize) -> bool {
    let pattern = "dddd-dd-ddTdd:dd:ddZ";
    if index + pattern.len() > chars.len() {
        return false;
    }
    pattern.chars().enumerate().all(|(offset, expected)| {
        let actual = chars[index + offset];
        match expected {
            'd' => actual.is_ascii_digit(),
            other => actual == other,
        }
    })
}

fn starts_with_chars(chars: &[char], index: usize, prefix: &str) -> bool {
    prefix
        .chars()
        .enumerate()
        .all(|(offset, expected)| chars.get(index + offset) == Some(&expected))
}

fn repo_fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn write_pretty_json(path: &Path, value: &Value) {
    let mut source = serde_json::to_string_pretty(value).expect("golden should serialize");
    source.push('\n');
    write_file(path, &source);
}

#[test]
fn public_runtime_status_and_operator_goldens_match_post_semantic_fix_baseline() {
    let actual = collect_runtime_golden();
    assert_runtime_manifest_user_tokens_normalized(&actual);
    let golden_path = repo_fixture_path(GOLDEN_PATH);
    if std::env::var_os("FEATUREFORGE_UPDATE_RUNTIME_GOLDENS").is_some() {
        write_pretty_json(&golden_path, &actual);
        panic!(
            "updated {GOLDEN_PATH}; review the fixture diff, then rerun without FEATUREFORGE_UPDATE_RUNTIME_GOLDENS"
        );
    }
    let expected: Value = serde_json::from_str(
        &fs::read_to_string(&golden_path)
            .unwrap_or_else(|error| panic!("runtime golden `{GOLDEN_PATH}` should read: {error}")),
    )
    .expect("runtime golden should parse");
    assert_eq!(actual, expected);
}
