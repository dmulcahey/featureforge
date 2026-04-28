#[path = "support/failure_json.rs"]
mod failure_json_support;
#[path = "support/featureforge.rs"]
mod featureforge_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/git.rs"]
mod git_support;
#[path = "support/process.rs"]
mod process_support;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use featureforge::git::discover_slug_identity;
use featureforge::paths::{harness_authoritative_artifact_path, harness_state_path};
use serde_json::{Value, json};
use tempfile::TempDir;

use failure_json_support::parse_failure_json;
use files_support::write_file;

const REVIEW_SPEC_REL: &str = "docs/featureforge/specs/public-replay-review-design.md";
const REVIEW_PLAN_REL: &str = "docs/featureforge/plans/public-replay-review-plan.md";
const EXEC_SPEC_REL: &str = "docs/featureforge/specs/public-replay-execution-design.md";
const EXEC_PLAN_REL: &str = "docs/featureforge/plans/public-replay-execution-plan.md";

struct PublicCli<'a> {
    repo: &'a Path,
    state: &'a Path,
    counts: BTreeMap<String, usize>,
}

impl<'a> PublicCli<'a> {
    fn new(repo: &'a Path, state: &'a Path) -> Self {
        Self {
            repo,
            state,
            counts: BTreeMap::new(),
        }
    }

    fn json(&mut self, args: &[&str], context: &str) -> Value {
        assert_public_runtime_args(args, context);
        self.record(args);
        featureforge_support::run_public_featureforge_cli_json(self.repo, self.state, args, context)
    }

    fn failure_json_with_env(
        &mut self,
        args: &[&str],
        envs: &[(&str, &str)],
        context: &str,
    ) -> Value {
        assert_public_runtime_args(args, context);
        self.record(args);
        let output = featureforge_support::run_rust_featureforge_with_env_control_real_cli(
            Some(self.repo),
            Some(self.state),
            None,
            &[],
            envs,
            args,
            context,
        );
        parse_failure_json(&output, context)
    }

    fn count(&self, key: &str) -> usize {
        self.counts.get(key).copied().unwrap_or(0)
    }

    fn checkpoint(&self) -> BTreeMap<String, usize> {
        self.counts.clone()
    }

    fn delta_since(&self, checkpoint: &BTreeMap<String, usize>, key: &str) -> usize {
        self.count(key) - checkpoint.get(key).copied().unwrap_or(0)
    }

    fn record(&mut self, args: &[&str]) {
        let key = match args {
            ["workflow", "status", ..] => "workflow status",
            ["workflow", "operator", ..] => "workflow operator",
            ["plan", "execution", command, ..] => command,
            _ => "other",
        };
        *self.counts.entry(key.to_owned()).or_insert(0) += 1;
    }
}

fn assert_public_runtime_args(args: &[&str], context: &str) {
    const HIDDEN_COMMANDS: &[&str] = &[
        "preflight",
        "gate-review",
        "gate-finish",
        "record-review-dispatch",
        "record-branch-closure",
        "record-release-readiness",
        "record-final-review",
        "record-qa",
        "rebuild-evidence",
        "explain-review-state",
        "reconcile-review-state",
    ];
    const HIDDEN_FLAGS: &[&str] = &["--dispatch-id", "--branch-closure-id"];

    for arg in args {
        assert!(
            !HIDDEN_COMMANDS.contains(arg),
            "{context} must not replay through hidden command `{arg}`"
        );
        assert!(
            !HIDDEN_FLAGS.contains(arg),
            "{context} must not replay through hidden flag `{arg}`"
        );
    }
}

fn init_repo(name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();
    git_support::init_repo_with_initial_commit(repo, &format!("# {name}\n"), "init");
    run_git(
        repo,
        &["checkout", "-b", "feature/public-replay"],
        "checkout replay branch",
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
    run_git(repo, &["add", "-A"], "git add public replay fixture");
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
        "git commit public replay fixture",
    );
}

fn write_review_spec_and_plan(repo: &Path, plan_revision: u32, last_reviewed_by: &str) {
    write_file(
        &repo.join(REVIEW_SPEC_REL),
        "# Public Replay Review Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-001][behavior] Public review routing must not dead-end on runtime-owned receipts.\n",
    );
    write_file(
        &repo.join(REVIEW_PLAN_REL),
        &format!(
            "# Public Replay Review Plan\n\n**Workflow State:** Draft\n**Plan Revision:** {plan_revision}\n**Execution Mode:** none\n**Source Spec:** `{REVIEW_SPEC_REL}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** {last_reviewed_by}\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Execution Strategy\n\n- Execute Task 1 last after review routing is accepted.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n\n## Task 1: Review routing replay\n\n**Spec Coverage:** REQ-001\n**Goal:** Review routing remains on public skills without receipt dead ends.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Do not route to hidden receipt commands.\n\n**Done when:**\n- Public workflow operator routes directly to engineering review.\n\n**Files:**\n- Test: `tests/public_replay_churn.rs`\n\n- [ ] **Step 1: Recheck public routing**\n"
        ),
    );
}

fn write_execution_spec_and_plan(repo: &Path) {
    write_file(
        &repo.join(EXEC_SPEC_REL),
        "# Public Replay Execution Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-001][behavior] Public execution commands own runtime preflight.\n- [REQ-002][behavior] Public close-current-task owns closure lineage.\n- [REQ-003][behavior] Current closures are not also stale closures.\n",
    );
    write_file(
        &repo.join(EXEC_PLAN_REL),
        &format!(
            "# Public Replay Execution Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{EXEC_SPEC_REL}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n**QA Requirement:** not-required\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n- REQ-002 -> Task 1\n- REQ-003 -> Task 2\n\n## Execution Strategy\n\n- Execute Task 1 serially to create a task-closure boundary.\n- Execute Task 2 serially after Task 1 proves the boundary can advance.\n\n## Dependency Diagram\n\n```text\nTask 1 -> Task 2\n```\n\n## Task 1: Public closure boundary\n\n**Spec Coverage:** REQ-001, REQ-002\n**Goal:** Public begin, complete, and close-current-task own the first task boundary.\n\n**Context:**\n- Spec Coverage: REQ-001, REQ-002.\n\n**Constraints:**\n- Use only public runtime commands.\n\n**Done when:**\n- Task 1 can close without hidden dispatch or preflight commands.\n\n**Files:**\n- Modify: `docs/public-replay-output.md`\n- Test: `cargo nextest run --test public_replay_churn`\n\n- [ ] **Step 1: Produce public replay output**\n\n## Task 2: Public downstream task\n\n**Spec Coverage:** REQ-003\n**Goal:** Current Task 1 closure lets public execution advance to Task 2.\n\n**Context:**\n- Spec Coverage: REQ-003.\n\n**Constraints:**\n- Do not reopen Task 1 after it is current.\n\n**Done when:**\n- Task 2 begin is allowed after Task 1 closure.\n\n**Files:**\n- Modify: `docs/public-replay-followup.md`\n- Test: `cargo nextest run --test public_replay_churn`\n\n- [ ] **Step 1: Start downstream work**\n"
        ),
    );
    write_file(
        &repo.join("docs/public-replay-output.md"),
        "Public replay output before execution.\n",
    );
    write_file(
        &repo.join("docs/public-replay-followup.md"),
        "Public replay follow-up before execution.\n",
    );
}

fn status(cli: &mut PublicCli<'_>, context: &str) -> Value {
    cli.json(
        &["plan", "execution", "status", "--plan", EXEC_PLAN_REL],
        context,
    )
}

fn workflow_operator(cli: &mut PublicCli<'_>, plan_rel: &str, context: &str) -> Value {
    cli.json(
        &["workflow", "operator", "--plan", plan_rel, "--json"],
        context,
    )
}

fn workflow_status(cli: &mut PublicCli<'_>, context: &str) -> Value {
    cli.json(&["workflow", "status", "--json"], context)
}

fn begin_task(cli: &mut PublicCli<'_>, task: u32, fingerprint: &str, context: &str) -> Value {
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

fn complete_task_1(cli: &mut PublicCli<'_>, fingerprint: &str, context: &str) -> Value {
    cli.json(
        &[
            "plan",
            "execution",
            "complete",
            "--plan",
            EXEC_PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed public replay Task 1.",
            "--manual-verify-summary",
            "Verified public replay Task 1.",
            "--file",
            "docs/public-replay-output.md",
            "--expect-execution-fingerprint",
            fingerprint,
        ],
        context,
    )
}

fn close_task_1(cli: &mut PublicCli<'_>, repo: &Path, context: &str) -> Value {
    let review_summary = repo.join("docs/task-1-review-summary.md");
    let verification_summary = repo.join("docs/task-1-verification-summary.md");
    write_file(&review_summary, "Task 1 public replay review passed.\n");
    write_file(
        &verification_summary,
        "Task 1 public replay verification passed.\n",
    );
    cli.json(
        &[
            "plan",
            "execution",
            "close-current-task",
            "--plan",
            EXEC_PLAN_REL,
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
        context,
    )
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

fn assert_json_text_excludes(value: &Value, needle: &str, context: &str) {
    let text = serde_json::to_string(value).expect("json should serialize");
    assert!(
        !text.contains(needle),
        "{context} should not contain `{needle}`: {text}"
    );
}

fn current_task_1_closure_id(status_json: &Value) -> String {
    status_json["current_task_closures"]
        .as_array()
        .and_then(|closures| {
            closures.iter().find_map(|closure| {
                (closure["task"] == json!(1)).then(|| {
                    closure["closure_record_id"]
                        .as_str()
                        .expect("task 1 closure should expose closure_record_id")
                        .to_owned()
                })
            })
        })
        .expect("status should expose current task 1 closure")
}

fn state_identity(repo: &Path) -> (String, String) {
    let identity = discover_slug_identity(repo);
    (identity.repo_slug, identity.branch_name)
}

fn remove_task_1_projection_artifacts(repo: &Path, state: &Path, status_json: &Value) {
    let execution_run_id = status_json["execution_run_id"]
        .as_str()
        .expect("status should expose execution_run_id");
    let (repo_slug, branch_name) = state_identity(repo);
    for file_name in [
        format!("unit-review-{execution_run_id}-task-1-step-1.md"),
        format!("task-verification-{execution_run_id}-task-1.md"),
    ] {
        let path = harness_authoritative_artifact_path(state, &repo_slug, &branch_name, &file_name);
        if path.is_file() {
            fs::remove_file(&path).unwrap_or_else(|error| {
                panic!(
                    "projection artifact `{}` should be removable: {error}",
                    path.display()
                )
            });
        }
    }
}

fn update_state_fields(repo: &Path, state: &Path, fields: &[(&str, Value)]) {
    let (repo_slug, branch_name) = state_identity(repo);
    let path = harness_state_path(state, &repo_slug, &branch_name);
    let mut value: Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("harness state `{}` should read: {error}", path.display())
        }))
        .expect("harness state should be valid json");
    let object = value
        .as_object_mut()
        .expect("harness state should be a json object");
    for (key, field_value) in fields {
        object.insert((*key).to_owned(), field_value.clone());
    }
    write_file(
        &path,
        &serde_json::to_string_pretty(&value).expect("state should serialize"),
    );
}

fn setup_execution_fixture(name: &str) -> (TempDir, TempDir) {
    let (repo_dir, state_dir) = init_repo(name);
    write_execution_spec_and_plan(repo_dir.path());
    commit_all(repo_dir.path(), "public replay execution fixture");
    (repo_dir, state_dir)
}

fn complete_task_1_without_closure(cli: &mut PublicCli<'_>, repo: &Path) -> Value {
    let initial_status = status(cli, "public replay status before begin");
    let begin = begin_task(
        cli,
        1,
        initial_status["execution_fingerprint"]
            .as_str()
            .expect("initial status should expose execution fingerprint"),
        "public replay begin task 1",
    );
    append_repo_file(
        repo,
        "docs/public-replay-output.md",
        "Public replay Task 1 changed the output.",
    );
    complete_task_1(
        cli,
        begin["execution_fingerprint"]
            .as_str()
            .expect("begin should expose execution fingerprint"),
        "public replay complete task 1",
    )
}

#[test]
fn public_replay_writing_plans_and_mid_review_edits_do_not_require_fidelity_receipts() {
    let (repo_dir, state_dir) = init_repo("public-replay-plan-review-routing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_review_spec_and_plan(repo, 1, "writing-plans");
    commit_all(repo, "public replay review fixture");
    let mut cli = PublicCli::new(repo, state);

    let first = workflow_status(&mut cli, "public replay writing-plans route");
    assert_eq!(
        first["next_skill"], "featureforge:plan-eng-review",
        "writing-plans route should go directly to engineering review: {first}"
    );
    assert_json_text_excludes(&first, "receipt", "writing-plans route");
    assert_json_text_excludes(&first, "plan-fidelity-review", "writing-plans route");

    write_review_spec_and_plan(repo, 2, "writing-plans");
    let edited_plan_path = repo.join(REVIEW_PLAN_REL);
    let edited_plan = fs::read_to_string(&edited_plan_path)
        .expect("review replay plan should be readable")
        .replace(
            "**Goal:** Review routing remains on public skills without receipt dead ends.",
            "**Goal:** Review routing remains on public skills without receipt dead ends after the engineering-review edit.",
        );
    write_file(&edited_plan_path, &edited_plan);
    let checkpoint = cli.checkpoint();
    let after_edit = workflow_status(&mut cli, "public replay engineering-review edit route");
    assert_eq!(
        after_edit["next_skill"], "featureforge:plan-eng-review",
        "engineering-review edit route should stay on engineering review: {after_edit}"
    );
    assert_json_text_excludes(&after_edit, "receipt", "engineering-review edit route");
    assert_json_text_excludes(
        &after_edit,
        "plan-fidelity-review",
        "engineering-review edit route",
    );
    assert_eq!(
        cli.delta_since(&checkpoint, "workflow status"),
        1,
        "engineering-review edit window should need one route check"
    );
}

#[test]
fn public_replay_begin_owns_allowed_preflight_without_hidden_command() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-begin-preflight");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);

    let status_before = status(&mut cli, "public replay begin status");
    assert_json_text_excludes(&status_before, "\"preflight\"", "begin status");
    let checkpoint = cli.checkpoint();
    let begin = begin_task(
        &mut cli,
        1,
        status_before["execution_fingerprint"]
            .as_str()
            .expect("status should expose execution fingerprint"),
        "public replay begin owns preflight",
    );

    assert_eq!(begin["active_task"], json!(1));
    assert!(
        begin["execution_run_id"].as_str().is_some(),
        "begin should persist a run identity: {begin}"
    );
    assert_eq!(
        cli.delta_since(&checkpoint, "begin"),
        1,
        "preflight bridge should need one public begin after route discovery"
    );
}

#[test]
fn public_replay_completed_task_without_closure_routes_to_public_close_once() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-close-route");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    complete_task_1_without_closure(&mut cli, repo);

    let operator = workflow_operator(
        &mut cli,
        EXEC_PLAN_REL,
        "public replay close route operator",
    );
    assert_eq!(operator["phase_detail"], "task_closure_recording_ready");
    assert!(
        operator["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "operator should recommend public close-current-task: {operator}"
    );
    assert_json_text_excludes(
        &operator,
        "task_review_dispatch_required",
        "close route operator",
    );
    let checkpoint = cli.checkpoint();
    let close = close_task_1(&mut cli, repo, "public replay close-current-task");
    assert_eq!(close["action"], "recorded");
    assert_eq!(
        cli.delta_since(&checkpoint, "close-current-task"),
        1,
        "completed task should close with one public close-current-task command"
    );
}

#[test]
fn public_replay_current_closure_is_not_stale_and_projection_loss_does_not_block_next_begin() {
    let (repo_dir, state_dir) =
        setup_execution_fixture("public-replay-current-closure-projections");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    complete_task_1_without_closure(&mut cli, repo);
    let close = close_task_1(&mut cli, repo, "public replay close before projection loss");
    assert_eq!(close["action"], "recorded");

    let status_after_close = status(&mut cli, "public replay status after current closure");
    let closure_id = current_task_1_closure_id(&status_after_close);
    assert!(
        !status_after_close["stale_unreviewed_closures"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|value| value == &json!(closure_id)),
        "current closure must not also be stale: {status_after_close}"
    );
    assert_json_text_excludes(
        &status_after_close,
        "task 1 reopen",
        "current closure status",
    );

    remove_task_1_projection_artifacts(repo, state, &status_after_close);
    let checkpoint = cli.checkpoint();
    let status_after_projection_loss =
        status(&mut cli, "public replay status after projection loss");
    assert_eq!(
        current_task_1_closure_id(&status_after_projection_loss),
        closure_id
    );
    assert_json_text_excludes(
        &status_after_projection_loss,
        "task_review_dispatch_required",
        "projection-loss status",
    );
    let begin_task_2 = begin_task(
        &mut cli,
        2,
        status_after_projection_loss["execution_fingerprint"]
            .as_str()
            .expect("projection-loss status should expose execution fingerprint"),
        "public replay begin task 2 after projection loss",
    );
    assert_eq!(begin_task_2["active_task"], json!(2));
    assert_eq!(
        cli.delta_since(&checkpoint, "status"),
        1,
        "projection loss should need one public status check"
    );
    assert_eq!(
        cli.delta_since(&checkpoint, "begin"),
        1,
        "projection loss should allow one public downstream begin"
    );
}

#[test]
fn public_replay_cycle_break_clears_on_current_closure_refresh_without_loop() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-cycle-break");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    complete_task_1_without_closure(&mut cli, repo);
    let close = close_task_1(&mut cli, repo, "public replay close before cycle break");
    assert_eq!(close["action"], "recorded");
    update_state_fields(
        repo,
        state,
        &[
            ("strategy_state", json!("cycle_breaking")),
            ("strategy_checkpoint_kind", json!("cycle_break")),
            ("strategy_cycle_break_task", json!(1)),
            ("strategy_cycle_break_step", json!(1)),
            (
                "strategy_cycle_break_checkpoint_fingerprint",
                json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            ),
        ],
    );

    let checkpoint = cli.checkpoint();
    let refreshed = close_task_1(
        &mut cli,
        repo,
        "public replay close refresh clears cycle break",
    );
    assert_eq!(refreshed["action"], "already_current");
    let status_after_refresh = status(&mut cli, "public replay status after cycle break refresh");
    assert_json_text_excludes(
        &status_after_refresh,
        "task_cycle_break_active",
        "cycle-break refresh status",
    );
    assert_json_text_excludes(
        &status_after_refresh,
        "reopen",
        "cycle-break refresh status",
    );
    assert_eq!(
        cli.delta_since(&checkpoint, "close-current-task"),
        1,
        "cycle-break recovery should need one closure refresh"
    );
    assert_eq!(
        cli.delta_since(&checkpoint, "reopen"),
        0,
        "cycle-break recovery must not loop through reopen"
    );
}

#[test]
fn public_replay_reviewer_recursion_guard_fails_closed_without_state_mutation() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-reviewer-recursion");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let before = state_dir_snapshot(state);
    let mut cli = PublicCli::new(repo, state);

    for (args, context) in [
        (
            &["workflow", "operator", "--plan", EXEC_PLAN_REL, "--json"][..],
            "public replay reviewer workflow guard",
        ),
        (
            &["plan", "execution", "status", "--plan", EXEC_PLAN_REL][..],
            "public replay reviewer plan execution guard",
        ),
    ] {
        let failure = cli.failure_json_with_env(
            args,
            &[("FEATUREFORGE_REVIEWER_RUNTIME_COMMANDS_ALLOWED", "no")],
            context,
        );
        assert_eq!(failure["error_class"], "ReviewerRuntimeCommandForbidden");
        assert!(
            failure["message"].as_str().is_some_and(|message| message
                .contains("Reviewer subagents may not run FeatureForge runtime commands")
                && message.contains("blocked review")),
            "{context} should return reviewer-context guidance: {failure}"
        );
    }

    assert_eq!(
        state_dir_snapshot(state),
        before,
        "reviewer guard should fail before runtime state mutation"
    );
}

fn state_dir_snapshot(state: &Path) -> Vec<PathBuf> {
    fn visit(root: &Path, base: &Path, out: &mut Vec<PathBuf>) {
        if !root.exists() {
            return;
        }
        for entry in fs::read_dir(root)
            .unwrap_or_else(|error| panic!("state dir `{}` should read: {error}", root.display()))
        {
            let entry = entry.expect("state dir entry should read");
            let path = entry.path();
            if path.is_dir() {
                visit(&path, base, out);
            } else {
                out.push(
                    path.strip_prefix(base)
                        .expect("state file should be under state dir")
                        .to_path_buf(),
                );
            }
        }
    }

    let mut files = Vec::new();
    visit(state, state, &mut files);
    files.sort();
    files
}
