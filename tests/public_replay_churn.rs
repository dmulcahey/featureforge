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

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use featureforge::contracts::plan::{PLAN_FIDELITY_REQUIRED_SURFACES, parse_plan_file};
use featureforge::contracts::spec::parse_spec_file;
use featureforge::execution::follow_up::execution_step_repair_target_id;
use featureforge::git::discover_slug_identity;
use featureforge::git::sha256_hex;
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
        public_featureforge_cli::run_public_featureforge_cli_json(
            self.repo, self.state, args, context,
        )
    }

    fn failure_json(&mut self, args: &[&str], context: &str) -> Value {
        self.failure_json_with_env(args, &[], context)
    }

    fn json_with_env(&mut self, args: &[&str], envs: &[(&str, &str)], context: &str) -> Value {
        assert_public_runtime_args(args, context);
        self.record(args);
        let output = public_featureforge_cli::run_featureforge_with_env_control_real_cli(
            Some(self.repo),
            Some(self.state),
            None,
            &[],
            envs,
            args,
            context,
        );
        assert!(
            output.status.success(),
            "public featureforge CLI command should succeed for {context}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "public featureforge CLI command should emit JSON for {context}: {error}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }

    fn failure_json_with_env(
        &mut self,
        args: &[&str],
        envs: &[(&str, &str)],
        context: &str,
    ) -> Value {
        assert_public_runtime_args(args, context);
        self.record(args);
        let output = public_featureforge_cli::run_featureforge_with_env_control_real_cli(
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
    const HIDDEN_COMMANDS: &[&[&str]] = &[
        &["pre", "flight"],
        &["gate", "-review"],
        &["gate", "-finish"],
        &["record", "-review-dispatch"],
        &["record", "-branch-closure"],
        &["record", "-release-readiness"],
        &["record", "-final-review"],
        &["record", "-qa"],
        &["rebuild", "-evidence"],
        &["explain", "-review-state"],
        &["reconcile", "-review-state"],
    ];
    const HIDDEN_FLAGS: &[&[&str]] = &[&["--dispatch", "-id"], &["--branch", "-closure-id"]];

    for arg in args {
        assert!(
            !HIDDEN_COMMANDS
                .iter()
                .any(|hidden| arg_matches_hidden_parts(arg, hidden)),
            "{context} must not replay through hidden command `{arg}`"
        );
        assert!(
            !HIDDEN_FLAGS
                .iter()
                .any(|hidden| arg_matches_hidden_parts(arg, hidden)),
            "{context} must not replay through hidden flag `{arg}`"
        );
    }
}

fn assert_public_json_excludes_hidden_tokens(value: &Value, context: &str) {
    let Some((hidden, text)) = public_json_hidden_token_violation(value) else {
        return;
    };
    panic!("{context} should not contain `{hidden}`: {text}");
}

fn public_json_hidden_token_violation(value: &Value) -> Option<(String, String)> {
    let text = serde_json::to_string(value).expect("json should serialize");
    for hidden in [
        concat!("record", "-review-dispatch"),
        concat!("gate", "-review"),
        concat!("rebuild", "-evidence"),
        concat!("--dispatch", "-id"),
        concat!("\"pre", "flight\""),
        concat!("featureforge plan execution pre", "flight"),
        concat!("featureforge workflow pre", "flight"),
        concat!("run workflow pre", "flight"),
        concat!("plan execution pre", "flight"),
        concat!("workflow pre", "flight"),
    ] {
        if text.contains(hidden) {
            return Some((hidden.to_owned(), text));
        }
    }
    None
}

#[test]
fn public_json_hidden_token_assertion_rejects_command_shaped_preflight_leaks() {
    let leaked = json!({
        "next_action": concat!("run workflow pre", "flight"),
    });
    let (hidden, _) = public_json_hidden_token_violation(&leaked)
        .expect("hidden-token detector should reject command-shaped preflight leaks");
    assert_eq!(
        hidden,
        concat!("run workflow pre", "flight"),
        "hidden-token detector should identify the command-shaped preflight leak"
    );

    let allowed_route_state = json!({
        "phase_detail": concat!("execution_pre", "flight_required"),
        "repo_state_drift_state": concat!("pre", "flight_pending"),
        "next_action": concat!("execution pre", "flight"),
    });
    assert!(
        public_json_hidden_token_violation(&allowed_route_state).is_none(),
        "hidden-token detector should allow public preflight state vocabulary"
    );
}

#[test]
fn runtime_behavior_goldens_cover_public_replay_regression_labels() {
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/runtime-goldens/public-runtime-routes.json");
    let source = fs::read_to_string(&golden_path).unwrap_or_else(|error| {
        panic!(
            "Task 8 runtime golden fixture `{}` should read before public replay extraction work: {error}",
            golden_path.display()
        )
    });
    let golden: Value =
        serde_json::from_str(&source).expect("Task 8 runtime golden fixture should parse");
    let labels = golden["scenarios"]
        .as_array()
        .expect("runtime golden should expose scenarios")
        .iter()
        .map(|scenario| {
            scenario["label"]
                .as_str()
                .expect("runtime golden scenario should expose label")
                .to_owned()
        })
        .collect::<BTreeSet<_>>();
    for required in [
        "begin_ready",
        "task_closure_recording_ready",
        "repair_review_state_required",
        "stale_targetless_reconcile",
        "blocked_runtime_bug_diagnostic",
        "reviewer_runtime_command_forbidden",
    ] {
        assert!(
            labels.contains(required),
            "runtime behavior goldens must retain public replay regression label `{required}` before modularization: {labels:?}"
        );
    }
}

fn arg_matches_hidden_parts(arg: &str, parts: &[&str]) -> bool {
    let mut remaining = arg;
    for part in parts {
        let Some(next) = remaining.strip_prefix(part) else {
            return false;
        };
        remaining = next;
    }
    remaining.is_empty()
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

fn git_status_short(repo: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("status")
        .arg("--short")
        .current_dir(repo)
        .output()
        .expect("git status --short should run");
    assert!(
        output.status.success(),
        "git status --short should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

fn git_status_entry_path(line: &str) -> String {
    let path = line.get(3..).unwrap_or(line).trim();
    path.rsplit_once(" -> ")
        .map(|(_, destination)| destination)
        .unwrap_or(path)
        .to_owned()
}

fn git_status_new_entries(baseline: &[String], current: &[String]) -> Vec<String> {
    current
        .iter()
        .filter(|line| !baseline.contains(line))
        .cloned()
        .collect()
}

fn assert_no_runtime_projection_status_entries(status: &[String], context: &str) {
    for line in status {
        let path = git_status_entry_path(line);
        assert!(
            !is_runtime_projection_status_path(&path),
            "{context} must not expose runtime projection churn in git status: {status:?}"
        );
    }
}

fn assert_git_status_unchanged_without_projection_churn(
    repo: &Path,
    baseline: &[String],
    context: &str,
) {
    assert_no_runtime_projection_status_entries(baseline, context);
    let current = git_status_short(repo);
    assert_no_runtime_projection_status_entries(&current, context);
    assert_eq!(
        current, baseline,
        "{context} must not add Git-visible projection churn"
    );
}

fn is_runtime_projection_status_path(path: &str) -> bool {
    path.starts_with("docs/featureforge/plans/")
        || path.starts_with("docs/featureforge/execution-evidence/")
        || path.starts_with(".featureforge/reviews/")
        || path.starts_with("docs/featureforge/projections/")
        || path.contains("release-readiness")
        || path.contains("final-review")
        || path.contains("browser-qa")
        || path.contains("/qa/")
}

fn is_projection_export_status_path(path: &str) -> bool {
    path.starts_with("docs/featureforge/projections/")
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
    write_review_spec_and_plan_with_state(repo, plan_revision, "Draft", last_reviewed_by);
}

fn write_review_spec_and_plan_with_state(
    repo: &Path,
    plan_revision: u32,
    workflow_state: &str,
    last_reviewed_by: &str,
) {
    write_file(
        &repo.join(REVIEW_SPEC_REL),
        "# Public Replay Review Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-001][behavior] Public review routing must not dead-end on runtime-owned receipts.\n",
    );
    write_file(
        &repo.join(REVIEW_PLAN_REL),
        &format!(
            "# Public Replay Review Plan\n\n**Workflow State:** {workflow_state}\n**Plan Revision:** {plan_revision}\n**Execution Mode:** none\n**Source Spec:** `{REVIEW_SPEC_REL}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** {last_reviewed_by}\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Execution Strategy\n\n- Execute Task 1 last after review routing is accepted.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n\n## Task 1: Review routing replay\n\n**Spec Coverage:** REQ-001\n**Goal:** Review routing remains on public skills without receipt dead ends.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Do not route to hidden receipt commands.\n\n**Done when:**\n- Public workflow operator routes directly to engineering review.\n\n**Files:**\n- Test: `tests/public_replay_churn.rs`\n\n- [ ] **Step 1: Recheck public routing**\n"
        ),
    );
}

fn write_execution_spec_and_plan(repo: &Path) {
    write_file(
        &repo.join(EXEC_SPEC_REL),
        concat!(
            "# Public Replay Execution Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-001][behavior] Public execution commands own runtime pre",
            "flight.\n- [REQ-002][behavior] Public close-current-task owns closure lineage.\n- [REQ-003][behavior] Current closures are not also stale closures.\n"
        ),
    );
    write_file(
        &repo.join(EXEC_PLAN_REL),
        &format!(
            "# Public Replay Execution Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n**QA Requirement:** not-required\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n- REQ-002 -> Task 1\n- REQ-003 -> Task 2\n\n## Execution Strategy\n\n- Execute Task 1 serially to create a task-closure boundary.\n- Execute Task 2 serially after Task 1 proves the boundary can advance.\n\n## Dependency Diagram\n\n```text\nTask 1 -> Task 2\n```\n\n## Task 1: Public closure boundary\n\n**Spec Coverage:** REQ-001, REQ-002\n**Goal:** Public begin, complete, and close-current-task own the first task boundary.\n\n**Context:**\n- Spec Coverage: REQ-001, REQ-002.\n\n**Constraints:**\n- Use only public runtime commands.\n\n**Done when:**\n- Task 1 can close without hidden dispatch or {} commands.\n\n**Files:**\n- Modify: `docs/public-replay-output.md`\n- Test: `cargo nextest run --test public_replay_churn`\n\n- [ ] **Step 1: Produce public replay output**\n\n## Task 2: Public downstream task\n\n**Spec Coverage:** REQ-003\n**Goal:** Current Task 1 closure lets public execution advance to Task 2.\n\n**Context:**\n- Spec Coverage: REQ-003.\n\n**Constraints:**\n- Do not reopen Task 1 after it is current.\n\n**Done when:**\n- Task 2 begin is allowed after Task 1 closure.\n\n**Files:**\n- Modify: `docs/public-replay-followup.md`\n- Test: `cargo nextest run --test public_replay_churn`\n\n- [ ] **Step 1: Start downstream work**\n",
            EXEC_SPEC_REL,
            concat!("pre", "flight")
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
    write_current_pass_plan_fidelity_review_artifact(
        repo,
        ".featureforge/reviews/public-replay-execution-plan-fidelity.md",
        EXEC_PLAN_REL,
        EXEC_SPEC_REL,
    );
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
    write_file(
        &artifact_path,
        &format!(
            "## Plan Fidelity Review Summary\n\n**Review Stage:** featureforge:plan-fidelity-review\n**Review Verdict:** pass\n**Reviewed Plan:** `{plan_rel}`\n**Reviewed Plan Revision:** {}\n**Reviewed Plan Fingerprint:** {plan_fingerprint}\n**Reviewed Spec:** `{spec_rel}`\n**Reviewed Spec Revision:** {}\n**Reviewed Spec Fingerprint:** {spec_fingerprint}\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** public-replay-fixture-plan-fidelity-reviewer\n**Distinct From Stages:** featureforge:writing-plans, featureforge:plan-eng-review\n**Verified Surfaces:** {}\n**Verified Requirement IDs:** {}\n",
            plan.plan_revision,
            spec.spec_revision,
            PLAN_FIDELITY_REQUIRED_SURFACES.join(", "),
            verified_requirement_ids.join(", "),
        ),
    );
}

fn status(cli: &mut PublicCli<'_>, context: &str) -> Value {
    status_for_plan(cli, EXEC_PLAN_REL, context)
}

fn status_for_plan(cli: &mut PublicCli<'_>, plan_rel: &str, context: &str) -> Value {
    cli.json(
        &["plan", "execution", "status", "--plan", plan_rel],
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

fn reopen_task_1(cli: &mut PublicCli<'_>, fingerprint: &str, context: &str) -> Value {
    cli.json(
        &[
            "plan",
            "execution",
            "reopen",
            "--plan",
            EXEC_PLAN_REL,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "Explicit public replay repair target requires execution reentry.",
            "--expect-execution-fingerprint",
            fingerprint,
        ],
        context,
    )
}

fn write_task_1_summary_files(repo: &Path) -> (PathBuf, PathBuf) {
    let review_summary = repo.join("docs/task-1-review-summary.md");
    let verification_summary = repo.join("docs/task-1-verification-summary.md");
    write_file(&review_summary, "Task 1 public replay review passed.\n");
    write_file(
        &verification_summary,
        "Task 1 public replay verification passed.\n",
    );
    (review_summary, verification_summary)
}

fn close_task_1(cli: &mut PublicCli<'_>, repo: &Path, context: &str) -> Value {
    let (review_summary, verification_summary) = write_task_1_summary_files(repo);
    close_task_1_with_summary_files(cli, &review_summary, &verification_summary, context)
}

fn close_task_1_with_summary_files(
    cli: &mut PublicCli<'_>,
    review_summary: &Path,
    verification_summary: &Path,
    context: &str,
) -> Value {
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

fn bind_explicit_reopen_repair_target(repo: &Path, state: &Path, task: u32, step: u32) {
    update_state_fields(
        repo,
        state,
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

fn invoke_recommended_public_command(cli: &mut PublicCli<'_>, operator_json: &Value) -> Value {
    let context = format!(
        "public replay invoke recommended command phase_detail={} recommended_command={}",
        operator_json["phase_detail"]
            .as_str()
            .unwrap_or("<missing>"),
        operator_json["recommended_command"]
            .as_str()
            .unwrap_or("<missing>")
    );
    invoke_recommended_public_command_for_context(cli, operator_json, &context)
}

fn invoke_recommended_public_command_for_context(
    cli: &mut PublicCli<'_>,
    operator_json: &Value,
    context: &str,
) -> Value {
    assert_public_json_excludes_hidden_tokens(operator_json, context);
    let command_parts = public_recommended_command_argv(operator_json, context);
    let args = concrete_public_command_args(cli.repo, &command_parts, context);
    let args_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = cli.json(&args_refs, context);
    assert_public_json_excludes_hidden_tokens(&output, context);
    output
}

fn public_recommended_command_argv(value: &Value, context: &str) -> Vec<String> {
    let command_parts = value["recommended_public_command_argv"]
        .as_array()
        .unwrap_or_else(|| {
            panic!("{context}: route should expose recommended_public_command_argv: {value}")
        })
        .iter()
        .map(|part| {
            part.as_str()
                .unwrap_or_else(|| {
                    panic!("{context}: recommended_public_command_argv entries must be strings: {value}")
                })
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(
        command_parts.len() >= 3,
        "{context}: recommended argv should include a featureforge command group, got {command_parts:?}"
    );
    assert_eq!(
        command_parts[0], "featureforge",
        "{context}: recommended argv should start with featureforge, got {command_parts:?}"
    );
    assert_public_runtime_args(
        &command_parts.iter().map(String::as_str).collect::<Vec<_>>(),
        context,
    );
    command_parts
}

fn concrete_public_command_args(
    repo: &Path,
    command_parts: &[String],
    context: &str,
) -> Vec<String> {
    let mut args = command_parts[1..].to_vec();
    if command_parts.get(1).is_some_and(|part| part == "plan")
        && command_parts.get(2).is_some_and(|part| part == "execution")
        && command_parts
            .get(3)
            .is_some_and(|part| part == "close-current-task")
        && command_parts
            .iter()
            .any(|part| part.contains('<') || part.contains("pass|fail"))
    {
        let plan = command_parts
            .windows(2)
            .find(|window| window[0] == "--plan")
            .map(|window| window[1].clone())
            .expect("close-current-task recommendation should include --plan");
        let task = command_parts
            .windows(2)
            .find(|window| window[0] == "--task")
            .map(|window| window[1].clone())
            .unwrap_or_else(|| String::from("1"));
        let review_summary = write_recommended_summary_file(
            repo,
            "docs/task-recommended-review-summary.md",
            &format!("Recommended close-current-task review passed for {context}.\n"),
        );
        let verification_summary = write_recommended_summary_file(
            repo,
            "docs/task-recommended-verification-summary.md",
            &format!("Recommended close-current-task verification passed for {context}.\n"),
        );
        args = vec![
            String::from("plan"),
            String::from("execution"),
            String::from("close-current-task"),
            String::from("--plan"),
            plan,
            String::from("--task"),
            task,
            String::from("--review-result"),
            String::from("pass"),
            String::from("--review-summary-file"),
            review_summary,
            String::from("--verification-result"),
            String::from("pass"),
            String::from("--verification-summary-file"),
            verification_summary,
        ];
    }
    assert!(
        args.iter()
            .all(|arg| !arg.contains('<') && !arg.contains("pass|fail")),
        "{context}: recommended command contains unresolved placeholders: {command_parts:?}"
    );
    args
}

fn write_recommended_summary_file(repo: &Path, rel: &str, contents: &str) -> String {
    let path = repo.join(rel);
    write_file(&path, contents);
    path.to_str()
        .expect("recommended summary path should be utf-8")
        .to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PublicRouteTuple {
    phase_detail: String,
    review_state_status: String,
    recommended_command: String,
    execution_fingerprint: String,
}

impl PublicRouteTuple {
    fn from_json(value: &Value) -> Option<Self> {
        let has_route_fields = value.get("phase_detail").is_some()
            || value.get("review_state_status").is_some()
            || value.get("recommended_command").is_some()
            || value.get("execution_fingerprint").is_some();
        has_route_fields.then(|| Self {
            phase_detail: json_field_for_tuple(value, "phase_detail"),
            review_state_status: json_field_for_tuple(value, "review_state_status"),
            recommended_command: json_field_for_tuple(value, "recommended_command"),
            execution_fingerprint: json_field_for_tuple(value, "execution_fingerprint"),
        })
    }
}

#[derive(Default)]
struct PublicRouteLoopDetector {
    seen_after_command: BTreeMap<PublicRouteTuple, usize>,
}

impl PublicRouteLoopDetector {
    fn observe_after_command(&mut self, value: &Value, context: &str) {
        let Some(route_tuple) = PublicRouteTuple::from_json(value) else {
            return;
        };
        let count = self
            .seen_after_command
            .entry(route_tuple.clone())
            .and_modify(|count| *count += 1)
            .or_insert(1);
        assert_eq!(
            *count, 1,
            "non_converging_public_route_loop: {context} returned a repeated public route tuple after executing a recommended command: {route_tuple:?}"
        );
    }
}

fn json_field_for_tuple(value: &Value, field: &str) -> String {
    value
        .get(field)
        .map(|field_value| {
            field_value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| field_value.to_string())
        })
        .unwrap_or_default()
}

fn invoke_recommended_public_command_and_check_progress(
    cli: &mut PublicCli<'_>,
    loop_detector: &mut PublicRouteLoopDetector,
    route_json: &Value,
    context: &str,
) -> Value {
    let before = PublicRouteTuple::from_json(route_json).unwrap_or_else(|| {
        panic!("{context}: public route tuple should be available: {route_json}")
    });
    let output = invoke_recommended_public_command(cli, route_json);
    if let Some(after) = PublicRouteTuple::from_json(&output) {
        assert_ne!(
            before, after,
            "non_converging_public_route_loop: {context} returned to the same route tuple after executing `{}`",
            before.recommended_command
        );
    }
    loop_detector.observe_after_command(&output, context);
    output
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

fn assert_targetless_stale_public_surface(value: &Value, context: &str) {
    assert!(
        value["phase_detail"] == json!("runtime_reconcile_required")
            || value["phase_detail"] == json!("blocked_runtime_bug"),
        "{context} should expose a runtime diagnostic route: {value}"
    );
    assert!(
        value.get("recommended_command").is_none_or(Value::is_null),
        "{context} must not recommend a mutation command: {value}"
    );
    assert!(
        value
            .get("execution_command_context")
            .is_none_or(Value::is_null),
        "{context} must not expose execution command context: {value}"
    );
    assert_json_text_excludes(value, "\"scope_key\":\"current\"", context);
    for forbidden_command in [
        "reopen --plan",
        "begin --plan",
        "close-current-task",
        "advance-late-stage",
    ] {
        assert_json_text_excludes(value, forbidden_command, context);
    }
    assert!(
        value["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "stale_unreviewed_target_missing")),
        "{context} should expose the targetless stale runtime reason code: {value}"
    );
    if let Some(blocking_records) = value["blocking_records"].as_array() {
        assert!(
            blocking_records.iter().any(|record| {
                record["code"] == json!("stale_unreviewed_target_missing")
                    && record["scope_type"] == json!("runtime")
                    && record["scope_key"] == json!("targetless_stale_unreviewed")
                    && record["record_id"].is_null()
                    && record["required_follow_up"].is_null()
            }),
            "{context} should expose the targetless stale runtime blocking record when blocking_records are serialized: {value}"
        );
    }
}

fn assert_hidden_recommended_command_is_blocked(value: &Value, context: &str) {
    assert_eq!(
        value["state_kind"], "blocked_runtime_bug",
        "{context} should fail closed as a blocked runtime bug: {value}"
    );
    assert!(
        value.get("recommended_command").is_none_or(Value::is_null),
        "{context} must not expose the injected hidden recommended command: {value}"
    );
    assert!(
        value.get("next_public_action").is_none_or(Value::is_null),
        "{context} must not expose an executable next public action: {value}"
    );
    assert!(
        value["blocking_reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "recommended_command_hidden_or_debug")),
        "{context} should carry the hidden-command invariant reason code: {value}"
    );
    assert_public_json_excludes_hidden_tokens(value, context);
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

fn remove_task_projection_artifacts(
    repo: &Path,
    state: &Path,
    status_json: &Value,
    task: u32,
) -> Vec<PathBuf> {
    let execution_run_id = status_json["execution_run_id"]
        .as_str()
        .expect("status should expose execution_run_id");
    let (repo_slug, branch_name) = state_identity(repo);
    let marker_path = harness_authoritative_artifact_path(state, &repo_slug, &branch_name, ".keep");
    let artifact_dir = marker_path
        .parent()
        .expect("artifact marker should have a parent directory");
    let unit_prefix = format!("unit-review-{execution_run_id}-task-{task}-step-");
    let verification_name = format!("task-verification-{execution_run_id}-task-{task}.md");
    let mut removed = Vec::new();
    if !artifact_dir.exists() {
        return removed;
    }
    for entry in fs::read_dir(artifact_dir).unwrap_or_else(|error| {
        panic!(
            "artifact dir `{}` should read: {error}",
            artifact_dir.display()
        )
    }) {
        let entry = entry.expect("artifact dir entry should read");
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let task_projection = (file_name.starts_with(&unit_prefix) && file_name.ends_with(".md"))
            || file_name == verification_name;
        if task_projection && path.is_file() {
            fs::remove_file(&path).unwrap_or_else(|error| {
                panic!(
                    "projection artifact `{}` should be removable: {error}",
                    path.display()
                )
            });
            removed.push(path);
        }
    }
    removed.sort();
    removed
}

fn update_state_fields(repo: &Path, state: &Path, fields: &[(&str, Value)]) {
    let (repo_slug, branch_name) = state_identity(repo);
    let path = harness_state_path(state, &repo_slug, &branch_name);
    let mut value =
        featureforge::execution::event_log::load_reduced_authoritative_state_for_tests(&path)
            .unwrap_or_else(|error| {
                panic!(
                    "event-authoritative public replay harness state should reduce for {}: {}",
                    path.display(),
                    error.message
                )
            })
            .unwrap_or_else(|| {
                serde_json::from_str(&fs::read_to_string(&path).unwrap_or_else(|error| {
                    panic!("harness state `{}` should read: {error}", path.display())
                }))
                .expect("harness state should be valid json")
            });
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

fn setup_execution_fixture_with_additional_plan(name: &str, plan_rel: &str) -> (TempDir, TempDir) {
    let (repo_dir, state_dir) = init_repo(name);
    let repo = repo_dir.path();
    write_execution_spec_and_plan(repo);
    let plan_contents =
        fs::read_to_string(repo.join(EXEC_PLAN_REL)).expect("base execution plan should read");
    write_file(&repo.join(plan_rel), &plan_contents);
    write_current_pass_plan_fidelity_review_artifact(
        repo,
        ".featureforge/reviews/public-replay-execution-plan-with-spaces-fidelity.md",
        plan_rel,
        EXEC_SPEC_REL,
    );
    commit_all(
        repo,
        "public replay execution fixture with spaced plan path",
    );
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
fn public_replay_recommended_argv_handles_plan_paths_with_spaces() {
    let spaced_plan_rel = "docs/featureforge/plans/public replay execution plan.md";
    let (repo_dir, state_dir) = setup_execution_fixture_with_additional_plan(
        "public-replay-recommended-argv-spaces",
        spaced_plan_rel,
    );
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);

    let initial_status = status_for_plan(
        &mut cli,
        spaced_plan_rel,
        "recommended argv spaced plan initial status",
    );
    assert_eq!(
        initial_status["recommended_public_command_argv"],
        json!([
            "featureforge",
            "plan",
            "execution",
            "begin",
            "--plan",
            spaced_plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            initial_status["execution_fingerprint"]
                .as_str()
                .expect("initial status should expose execution fingerprint")
        ]),
        "recommended argv should keep the plan path with spaces as one argv element: {initial_status}"
    );
    assert!(
        initial_status["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains(spaced_plan_rel)),
        "rendered command remains present for human display: {initial_status}"
    );

    let begin = invoke_recommended_public_command_for_context(
        &mut cli,
        &initial_status,
        "recommended argv spaced plan begin",
    );
    assert_eq!(
        begin["active_task"],
        json!(1),
        "argv replay should begin Task 1 even though the plan path contains spaces: {begin}"
    );
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
    assert_json_text_excludes(&first, "plan_fidelity_review", "writing-plans route");

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
    assert_json_text_excludes(
        &after_edit,
        "plan_fidelity_review",
        "engineering-review edit route",
    );
    assert_eq!(
        cli.delta_since(&checkpoint, "workflow status"),
        1,
        "engineering-review edit window should need one route check"
    );
}

#[test]
fn public_replay_engineering_approved_plan_without_fidelity_cannot_bypass_to_implementation() {
    let (repo_dir, state_dir) = init_repo("public-replay-approved-plan-fidelity-gate");
    let repo = repo_dir.path();
    let state = state_dir.path();
    write_review_spec_and_plan_with_state(repo, 1, "Engineering Approved", "plan-eng-review");
    commit_all(repo, "public replay approved review fixture");
    let mut cli = PublicCli::new(repo, state);

    let status_json = workflow_status(&mut cli, "public replay approved plan fidelity gate");

    assert_eq!(status_json["status"], "plan_review_required");
    assert_ne!(status_json["status"], "implementation_ready");
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "engineering_approval_missing_plan_fidelity_review"),
        "manual Engineering Approved plan must not bypass missing fidelity: {status_json}"
    );
    assert_eq!(status_json["plan_fidelity_review"]["state"], "missing");
    assert_json_text_excludes(&status_json, "receipt", "approved plan fidelity gate");
}

#[test]
fn public_replay_begin_owns_allowed_preflight_without_hidden_command() {
    let (repo_dir, state_dir) =
        setup_execution_fixture(concat!("public-replay-begin-pre", "flight"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);

    let status_before = status(&mut cli, "public replay begin status");
    assert_json_text_excludes(&status_before, concat!("\"pre", "flight\""), "begin status");
    let checkpoint = cli.checkpoint();
    let begin = begin_task(
        &mut cli,
        1,
        status_before["execution_fingerprint"]
            .as_str()
            .expect("status should expose execution fingerprint"),
        concat!("public replay begin owns pre", "flight"),
    );

    assert_eq!(begin["active_task"], json!(1));
    assert!(
        begin["execution_run_id"].as_str().is_some(),
        "begin should persist a run identity: {begin}"
    );
    assert_eq!(
        cli.delta_since(&checkpoint, "begin"),
        1,
        "{} bridge should need one public begin after route discovery",
        concat!("pre", "flight")
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
fn public_replay_projection_loss_before_current_closure_stays_on_public_close_current_task() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-preclosure-projection-loss");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    complete_task_1_without_closure(&mut cli, repo);

    let status_before_close = status(&mut cli, "public replay preclosure projection status");
    remove_task_projection_artifacts(repo, state, &status_before_close, 1);

    let operator = workflow_operator(
        &mut cli,
        EXEC_PLAN_REL,
        "public replay preclosure projection operator",
    );
    assert_eq!(operator["phase_detail"], "task_closure_recording_ready");
    assert!(
        operator["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "operator should stay on public close-current-task after preclosure projection loss: {operator}"
    );
    assert_json_text_excludes(&operator, "receipt", "preclosure projection operator");
    assert_json_text_excludes(
        &operator,
        concat!("record", "-review-dispatch"),
        "preclosure projection operator",
    );
    assert_json_text_excludes(
        &operator,
        "task_review_dispatch_required",
        "preclosure projection operator",
    );
    assert_json_text_excludes(&operator, "unit-review", "preclosure projection operator");
    assert_json_text_excludes(
        &operator,
        "task-verification",
        "preclosure projection operator",
    );

    let close = close_task_1(
        &mut cli,
        repo,
        "public replay preclosure close-current-task with summaries",
    );
    assert_eq!(close["action"], "recorded");
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

    let materialized = cli.json(
        &[
            "plan",
            "execution",
            "materialize-projections",
            "--plan",
            EXEC_PLAN_REL,
        ],
        "explicit materialization before current closure projection loss",
    );
    assert_eq!(materialized["action"], json!("materialized"));
    assert_eq!(materialized["runtime_truth_changed"], json!(false));
    let removed_projection_artifacts =
        remove_task_projection_artifacts(repo, state, &status_after_close, 1);
    assert!(
        !removed_projection_artifacts.is_empty(),
        "fixture should remove explicitly materialized runtime-owned review projection artifacts after current closure"
    );
    let checkpoint = cli.checkpoint();
    let status_after_projection_loss =
        status(&mut cli, "public replay status after projection loss");
    assert_eq!(
        current_task_1_closure_id(&status_after_projection_loss),
        closure_id
    );
    assert_eq!(
        status_after_projection_loss["phase_detail"], status_after_close["phase_detail"],
        "projection loss should not introduce a new task-boundary route"
    );
    assert_eq!(
        status_after_projection_loss["blocking_task"], status_after_close["blocking_task"],
        "projection loss should not change the public blocking target"
    );
    assert_eq!(
        status_after_projection_loss["recommended_command"],
        status_after_close["recommended_command"],
        "projection loss should not change the next executable public command"
    );
    assert_json_text_excludes(
        &status_after_projection_loss,
        "receipt",
        "projection-loss status",
    );
    assert_json_text_excludes(
        &status_after_projection_loss,
        "task_review_dispatch_required",
        "projection-loss status",
    );
    assert_json_text_excludes(
        &status_after_projection_loss,
        "prior_task_verification_missing",
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
fn public_replay_current_task_closure_never_reappears_as_stale_after_repair() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-current-closure-repair");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    complete_task_1_without_closure(&mut cli, repo);
    let (review_summary, verification_summary) = write_task_1_summary_files(repo);
    let repair = cli.json(
        &[
            "plan",
            "execution",
            "repair-review-state",
            "--plan",
            EXEC_PLAN_REL,
        ],
        "current closure repair repair-review-state",
    );
    assert_eq!(
        repair["phase_detail"], "task_closure_recording_ready",
        "repair-review-state should route the completed task to public task closure recording: {repair}"
    );
    assert!(
        repair["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "repair-review-state should recommend public close-current-task: {repair}"
    );

    let repaired_close = close_task_1_with_summary_files(
        &mut cli,
        &review_summary,
        &verification_summary,
        "current closure repair close-current-task after repair",
    );
    assert!(
        repaired_close["action"] == json!("recorded")
            || repaired_close["action"] == json!("already_current"),
        "close-current-task should record or refresh the repaired current closure: {repaired_close}"
    );

    let status_after_close = status(&mut cli, "current closure repair status after close");
    let operator_after_close = workflow_operator(
        &mut cli,
        EXEC_PLAN_REL,
        "current closure repair operator after close",
    );
    let closure_id = current_task_1_closure_id(&status_after_close);

    for (surface, value) in [
        ("status after repaired close", &status_after_close),
        ("operator after repaired close", &operator_after_close),
    ] {
        if let Some(stale_closures) = value["stale_unreviewed_closures"].as_array() {
            assert!(
                stale_closures
                    .iter()
                    .all(|stale| stale.as_str() != Some(closure_id.as_str())),
                "{surface} must not report the current Task 1 closure as stale: {value}"
            );
        }
        let recommended_command = value["recommended_command"].as_str().unwrap_or_default();
        assert!(
            !recommended_command.contains("reopen") || !recommended_command.contains("--task 1"),
            "{surface} must not recommend reopening the just-closed Task 1 step: {value}"
        );
        let reentry_for_task_1 = value["phase_detail"] == json!("execution_reentry_required")
            && value["execution_command_context"]["task_number"] == json!(1);
        assert!(
            !reentry_for_task_1,
            "{surface} must not route the just-closed Task 1 step back to execution reentry: {value}"
        );
    }
}

#[test]
fn public_replay_recommended_mutations_execute_and_do_not_loop() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-recommended-parity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    let mut loop_detector = PublicRouteLoopDetector::default();

    let initial_status = status(&mut cli, "recommended parity initial status");
    assert_public_json_excludes_hidden_tokens(&initial_status, "recommended parity initial status");
    assert!(
        initial_status["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.starts_with("featureforge plan execution begin --plan ")
                    && command.contains("--task 1 --step 1")
            }),
        "initial public status should recommend Task 1 begin: {initial_status}"
    );
    let begin = invoke_recommended_public_command_and_check_progress(
        &mut cli,
        &mut loop_detector,
        &initial_status,
        "recommended parity begin task 1",
    );
    assert_eq!(begin["active_task"], json!(1));

    append_repo_file(
        repo,
        "docs/public-replay-output.md",
        "Task 1 output changed through the recommended parity replay.",
    );
    let in_progress = status(&mut cli, "recommended parity status before complete");
    assert_public_json_excludes_hidden_tokens(
        &in_progress,
        "recommended parity status before complete",
    );
    assert_eq!(
        in_progress["phase_detail"], "execution_in_progress",
        "status should allow public completion after implementation output changes: {in_progress}"
    );
    let complete = complete_task_1(
        &mut cli,
        begin["execution_fingerprint"]
            .as_str()
            .expect("begin should expose execution fingerprint"),
        "recommended parity complete task 1 with dynamic summaries",
    );
    assert!(
        complete["execution_fingerprint"].as_str().is_some(),
        "complete should return the next execution fingerprint: {complete}"
    );
    assert_public_json_excludes_hidden_tokens(&complete, "recommended parity complete task 1");
    loop_detector.observe_after_command(&complete, "recommended parity complete task 1");

    let operator_after_complete =
        workflow_operator(&mut cli, EXEC_PLAN_REL, "recommended parity close operator");
    assert_public_json_excludes_hidden_tokens(
        &operator_after_complete,
        "recommended parity close operator",
    );
    assert_eq!(
        operator_after_complete["phase_detail"], "task_closure_recording_ready",
        "operator should route completed Task 1 to public close-current-task: {operator_after_complete}"
    );
    assert!(
        operator_after_complete["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "operator should recommend public close-current-task: {operator_after_complete}"
    );
    let close = invoke_recommended_public_command_and_check_progress(
        &mut cli,
        &mut loop_detector,
        &operator_after_complete,
        "recommended parity close task 1",
    );
    assert!(
        close["action"] == json!("recorded") || close["action"] == json!("already_current"),
        "recommended close-current-task should close or refresh Task 1: {close}"
    );

    let status_before_task_2 = status(&mut cli, "recommended parity status before task 2");
    assert_public_json_excludes_hidden_tokens(
        &status_before_task_2,
        "recommended parity status before task 2",
    );
    assert!(
        status_before_task_2["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.starts_with("featureforge plan execution begin --plan ")
                    && command.contains("--task 2 --step 1")
            }),
        "status after Task 1 closure should recommend Task 2 begin: {status_before_task_2}"
    );
    let begin_task_2 = invoke_recommended_public_command_and_check_progress(
        &mut cli,
        &mut loop_detector,
        &status_before_task_2,
        "recommended parity begin task 2",
    );
    assert_eq!(begin_task_2["active_task"], json!(2));
}

#[test]
fn public_replay_stale_current_closure_repair_recommendation_executes_without_loop() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-stale-repair-parity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    let mut loop_detector = PublicRouteLoopDetector::default();
    complete_task_1_without_closure(&mut cli, repo);
    let close = close_task_1(&mut cli, repo, "stale repair parity initial close");
    assert_eq!(close["action"], "recorded");

    append_repo_file(
        repo,
        "docs/public-replay-output.md",
        "Post-closure drift should require public repair-review-state.",
    );
    let stale_operator = workflow_operator(&mut cli, EXEC_PLAN_REL, "stale repair parity operator");
    assert_public_json_excludes_hidden_tokens(&stale_operator, "stale repair parity operator");
    assert!(
        stale_operator["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("repair-review-state")),
        "stale current closure should route through public repair-review-state: {stale_operator}"
    );
    let repair = invoke_recommended_public_command_and_check_progress(
        &mut cli,
        &mut loop_detector,
        &stale_operator,
        "stale repair parity repair-review-state",
    );
    assert_public_json_excludes_hidden_tokens(&repair, "stale repair parity repair output");
    let next_recommended = repair["recommended_command"]
        .as_str()
        .expect("repair-review-state should return the next concrete public command");
    assert!(
        !next_recommended.contains("repair-review-state")
            || stale_operator["recommended_command"] == repair["recommended_command"],
        "repair-review-state should progress to a concrete public command or bounded same-command repair route: {repair}"
    );

    let next = invoke_recommended_public_command_and_check_progress(
        &mut cli,
        &mut loop_detector,
        &repair,
        "stale repair parity next recommended command",
    );
    assert_public_json_excludes_hidden_tokens(&next, "stale repair parity next command output");
}

#[test]
fn public_replay_normal_progress_keeps_projection_materialization_explicit() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-projection-export");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    let mut loop_detector = PublicRouteLoopDetector::default();

    let initial_status = status(&mut cli, "projection explicit initial status");
    let before_begin = git_status_short(repo);
    let begin = begin_task(
        &mut cli,
        1,
        initial_status["execution_fingerprint"]
            .as_str()
            .expect("initial status should expose execution fingerprint"),
        "projection explicit begin task 1",
    );
    assert_git_status_unchanged_without_projection_churn(
        repo,
        &before_begin,
        "normal begin projection explicit replay",
    );

    append_repo_file(
        repo,
        "docs/public-replay-output.md",
        "Task 1 output changed before projection explicit completion.",
    );
    let before_complete = git_status_short(repo);
    let complete = complete_task_1(
        &mut cli,
        begin["execution_fingerprint"]
            .as_str()
            .expect("begin should expose execution fingerprint"),
        "projection explicit complete task 1",
    );
    assert!(
        complete["execution_fingerprint"].as_str().is_some(),
        "complete should return a new execution fingerprint: {complete}"
    );
    assert_git_status_unchanged_without_projection_churn(
        repo,
        &before_complete,
        "normal complete projection explicit replay",
    );

    let (review_summary, verification_summary) = write_task_1_summary_files(repo);
    let before_close = git_status_short(repo);
    let close = close_task_1_with_summary_files(
        &mut cli,
        &review_summary,
        &verification_summary,
        "projection explicit close-current-task",
    );
    assert_eq!(close["action"], "recorded");
    assert_git_status_unchanged_without_projection_churn(
        repo,
        &before_close,
        "normal close-current-task projection explicit replay",
    );

    append_repo_file(
        repo,
        "docs/public-replay-output.md",
        "Post-closure drift should require public repair without projection export.",
    );
    let stale_operator = workflow_operator(
        &mut cli,
        EXEC_PLAN_REL,
        "projection explicit repair operator",
    );
    assert!(
        stale_operator["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("repair-review-state")),
        "post-closure drift should recommend public repair-review-state: {stale_operator}"
    );
    let before_repair = git_status_short(repo);
    let repair = invoke_recommended_public_command_and_check_progress(
        &mut cli,
        &mut loop_detector,
        &stale_operator,
        "projection explicit repair-review-state",
    );
    assert_git_status_unchanged_without_projection_churn(
        repo,
        &before_repair,
        "normal repair-review-state projection explicit replay",
    );
    assert_public_json_excludes_hidden_tokens(&repair, "projection explicit repair output");

    let before_state_dir_materialize = git_status_short(repo);
    let state_dir_materialized = cli.json(
        &[
            "plan",
            "execution",
            "materialize-projections",
            "--plan",
            EXEC_PLAN_REL,
        ],
        "state-dir materialization should not dirty git",
    );
    assert_eq!(state_dir_materialized["action"], "materialized");
    assert_eq!(state_dir_materialized["projection_mode"], "state_dir_only");
    assert_eq!(
        state_dir_materialized["runtime_truth_changed"],
        json!(false)
    );
    assert_git_status_unchanged_without_projection_churn(
        repo,
        &before_state_dir_materialize,
        "state-dir materialization projection explicit replay",
    );

    for (flag, context) in [
        (
            "--repo-export",
            "unconfirmed repo-export materialization should fail closed",
        ),
        (
            "--tracked",
            "deprecated tracked materialization should require confirmation",
        ),
    ] {
        let before_failure = git_status_short(repo);
        let failure = cli.failure_json(
            &[
                "plan",
                "execution",
                "materialize-projections",
                "--plan",
                EXEC_PLAN_REL,
                flag,
            ],
            context,
        );
        assert_eq!(failure["error_class"], "InvalidCommandInput");
        assert!(
            failure["message"]
                .as_str()
                .is_some_and(|message| message.contains("--confirm-repo-export")),
            "{context} should explain the explicit export confirmation requirement: {failure}"
        );
        assert_git_status_unchanged_without_projection_churn(repo, &before_failure, context);
    }

    let before_repo_export = git_status_short(repo);
    let materialized = cli.json(
        &[
            "plan",
            "execution",
            "materialize-projections",
            "--plan",
            EXEC_PLAN_REL,
            "--repo-export",
            "--confirm-repo-export",
        ],
        "confirmed repo projection export",
    );
    assert_eq!(materialized["action"], "materialized");
    assert_eq!(materialized["projection_mode"], "projection_export");
    assert_eq!(materialized["runtime_truth_changed"], json!(false));
    assert!(
        materialized["trace_summary"].as_str().is_some_and(
            |summary| summary.contains("approved plan/evidence files were not modified")
        ),
        "confirmed materialization should describe approved-file preservation: {materialized}"
    );
    let written_paths = materialized["written_paths"]
        .as_array()
        .expect("confirmed materialization should report written paths")
        .iter()
        .map(|path| {
            path.as_str()
                .expect("written path should be a string")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert!(
        !written_paths.is_empty(),
        "confirmed materialization should report Git-visible projection exports: {materialized}"
    );
    assert!(
        written_paths
            .iter()
            .all(|path| path.starts_with("docs/featureforge/projections/")),
        "confirmed materialization should only write projection export paths: {written_paths:?}"
    );
    assert!(
        written_paths.iter().all(|path| repo.join(path).is_file()),
        "confirmed materialization should write every reported path: {written_paths:?}"
    );
    let after_repo_export = git_status_short(repo);
    let export_entries = git_status_new_entries(&before_repo_export, &after_repo_export);
    assert!(
        !export_entries.is_empty(),
        "confirmed materialization should create Git-visible projection export entries: {after_repo_export:?}"
    );
    assert!(
        export_entries
            .iter()
            .map(|line| git_status_entry_path(line))
            .all(|path| is_projection_export_status_path(&path)),
        "only confirmed materialization may add projection export paths, new entries: {export_entries:?}"
    );
}

#[test]
fn public_replay_reopen_does_not_materialize_tracked_projection_files() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-reopen-no-projection-churn");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);

    let initial_status = status(&mut cli, "projection explicit reopen initial status");
    let begin = begin_task(
        &mut cli,
        1,
        initial_status["execution_fingerprint"]
            .as_str()
            .expect("initial status should expose execution fingerprint"),
        "projection explicit reopen begin task 1",
    );
    append_repo_file(
        repo,
        "docs/public-replay-output.md",
        "Task 1 output changed before projection explicit reopen.",
    );
    let complete = complete_task_1(
        &mut cli,
        begin["execution_fingerprint"]
            .as_str()
            .expect("begin should expose execution fingerprint"),
        "projection explicit reopen complete task 1",
    );
    bind_explicit_reopen_repair_target(repo, state, 1, 1);

    let status_after_repair = status(&mut cli, "projection explicit reopen status");
    assert!(
        status_after_repair["public_repair_targets"]
            .as_array()
            .is_some_and(|targets| targets.iter().any(|target| {
                target["command_kind"] == "reopen"
                    && target["task"] == json!(1)
                    && target["step"] == json!(1)
            })),
        "status should expose a public reopen repair target: {status_after_repair}"
    );
    let before_reopen = git_status_short(repo);
    let reopened = reopen_task_1(
        &mut cli,
        status_after_repair["execution_fingerprint"]
            .as_str()
            .or_else(|| complete["execution_fingerprint"].as_str())
            .expect("status should expose execution fingerprint before reopen"),
        "projection explicit reopen command",
    );
    assert_eq!(reopened["resume_task"], json!(1));
    assert_eq!(reopened["resume_step"], json!(1));
    assert_git_status_unchanged_without_projection_churn(
        repo,
        &before_reopen,
        "normal reopen projection explicit replay",
    );
}

#[test]
fn public_replay_hidden_recommended_command_injection_is_blocked_before_execution() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-hidden-recommendation");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let env = [(
        "FEATUREFORGE_PLAN_EXECUTION_READ_INVARIANT_TEST_INJECTION",
        "hidden_recommended_command",
    )];
    let mut cli = PublicCli::new(repo, state);

    let status_json = cli.json_with_env(
        &["plan", "execution", "status", "--plan", EXEC_PLAN_REL],
        &env,
        "public replay hidden recommended status",
    );
    assert_hidden_recommended_command_is_blocked(
        &status_json,
        "public replay hidden recommended status",
    );

    let operator_json = cli.json_with_env(
        &["workflow", "operator", "--plan", EXEC_PLAN_REL, "--json"],
        &env,
        "public replay hidden recommended operator",
    );
    assert_hidden_recommended_command_is_blocked(
        &operator_json,
        "public replay hidden recommended operator",
    );
}

#[test]
fn public_replay_targetless_stale_state_does_not_fabricate_current_scope() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-targetless-stale");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    let env = [(
        "FEATUREFORGE_PLAN_EXECUTION_READ_INVARIANT_TEST_INJECTION",
        "targetless_stale_unreviewed",
    )];

    let status_json = cli.json_with_env(
        &["plan", "execution", "status", "--plan", EXEC_PLAN_REL],
        &env,
        "public replay targetless stale status",
    );
    assert_targetless_stale_public_surface(&status_json, "targetless stale status");

    let operator_json = cli.json_with_env(
        &["workflow", "operator", "--plan", EXEC_PLAN_REL, "--json"],
        &env,
        "public replay targetless stale operator",
    );
    assert_targetless_stale_public_surface(&operator_json, "targetless stale operator");
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
    let (repo_slug, branch_name) = state_identity(repo);
    let state_path = harness_state_path(state, &repo_slug, &branch_name);
    let authoritative_state =
        featureforge::execution::event_log::load_reduced_authoritative_state_for_tests(&state_path)
            .unwrap_or_else(|error| {
                panic!(
                    "event-authoritative cycle-break state `{}` should reduce after refresh: {}",
                    state_path.display(),
                    error.message
                )
            })
            .unwrap_or_else(|| {
                serde_json::from_str(&fs::read_to_string(&state_path).unwrap_or_else(|error| {
                    panic!(
                        "cycle-break authoritative state `{}` should read after refresh: {error}",
                        state_path.display()
                    )
                }))
                .expect("cycle-break authoritative state should remain valid json")
            });
    assert!(
        authoritative_state["strategy_state"].is_null(),
        "resolved current closure should clear cycle-break strategy_state: {authoritative_state}"
    );
    assert!(
        authoritative_state["strategy_checkpoint_kind"].is_null(),
        "resolved current closure should clear cycle-break strategy_checkpoint_kind: {authoritative_state}"
    );
    assert!(
        authoritative_state["strategy_cycle_break_task"].is_null(),
        "resolved current closure should clear cycle-break task binding: {authoritative_state}"
    );
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
    let begin_task_2 = begin_task(
        &mut cli,
        2,
        status_after_refresh["execution_fingerprint"]
            .as_str()
            .expect("cycle-break refresh status should expose execution fingerprint"),
        "public replay begin task 2 after cycle-break cleanup",
    );
    assert_eq!(
        begin_task_2["active_task"],
        json!(2),
        "Task 2 should become begin-able after Task 1 current closure clears cycle-break state"
    );
}

#[test]
fn public_replay_reviewer_recursion_guard_fails_closed_without_state_mutation() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-reviewer-recursion");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let before = state_dir_snapshot(state);
    let git_status_before = git_status_short(repo);
    let mut cli = PublicCli::new(repo, state);

    for (args, context) in [
        (
            &["workflow", "status", "--json"][..],
            "public replay reviewer workflow status guard",
        ),
        (
            &["workflow", "operator", "--plan", EXEC_PLAN_REL, "--json"][..],
            "public replay reviewer workflow operator guard",
        ),
        (
            &["plan", "execution", "status", "--plan", EXEC_PLAN_REL][..],
            "public replay reviewer plan execution status guard",
        ),
        (
            &[
                "plan",
                "execution",
                "begin",
                "--plan",
                EXEC_PLAN_REL,
                "--task",
                "1",
                "--step",
                "1",
                "--expect-execution-fingerprint",
                "reviewer-guard-fingerprint",
            ][..],
            "public replay reviewer plan execution begin guard",
        ),
        (
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
                "missing-review-summary.md",
                "--verification-result",
                "pass",
                "--verification-summary-file",
                "missing-verification-summary.md",
            ][..],
            "public replay reviewer plan execution close-current-task guard",
        ),
        (
            &[
                "plan",
                "execution",
                "advance-late-stage",
                "--plan",
                EXEC_PLAN_REL,
            ][..],
            "public replay reviewer plan execution advance-late-stage guard",
        ),
        (
            &[
                "plan",
                "execution",
                "repair-review-state",
                "--plan",
                EXEC_PLAN_REL,
            ][..],
            "public replay reviewer plan execution repair-review-state guard",
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
        assert_eq!(
            state_dir_snapshot(state),
            before,
            "{context} should fail before runtime state mutation"
        );
        assert_eq!(
            git_status_short(repo),
            git_status_before,
            "{context} should fail before tracked-file mutation"
        );
    }

    assert_eq!(
        state_dir_snapshot(state),
        before,
        "reviewer guard should fail before runtime state mutation"
    );
    assert_eq!(
        git_status_short(repo),
        git_status_before,
        "reviewer guard should fail before tracked-file mutation"
    );
}

fn state_dir_snapshot(state: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, base: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
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
                let relative_path = path
                    .strip_prefix(base)
                    .expect("state file should be under state dir")
                    .to_path_buf();
                let contents = fs::read(&path).unwrap_or_else(|error| {
                    panic!("state file `{}` should read: {error}", path.display())
                });
                out.insert(relative_path, contents);
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(state, state, &mut files);
    files
}
