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
use featureforge::paths::{
    branch_storage_key, harness_authoritative_artifact_path, harness_state_path,
};
use serde_json::{Value, json};
use tempfile::TempDir;

use failure_json_support::parse_failure_json;
use files_support::write_file;

const REVIEW_SPEC_REL: &str = "docs/featureforge/specs/public-replay-review-design.md";
const REVIEW_PLAN_REL: &str = "docs/featureforge/plans/public-replay-review-plan.md";
const EXEC_SPEC_REL: &str = "docs/featureforge/specs/public-replay-execution-design.md";
const EXEC_PLAN_REL: &str = "docs/featureforge/plans/public-replay-execution-plan.md";
const OLD_SESSION_SPEC_REL: &str = "docs/featureforge/specs/public-replay-old-session-design.md";
const OLD_SESSION_FS11_PLAN_REL: &str =
    "docs/featureforge/plans/public-replay-old-session-fs11-plan.md";
const OLD_SESSION_FS15_PLAN_REL: &str =
    "docs/featureforge/plans/public-replay-old-session-fs15-plan.md";

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
    if json_contains_stale_preflight_next_action(value) {
        return Some((
            concat!("next_action=", "execution pre", "flight").to_owned(),
            text,
        ));
    }
    for hidden in [
        concat!("record", "-review-dispatch"),
        concat!("gate", "-review"),
        concat!("gate", "-finish"),
        concat!("rebuild", "-evidence"),
        concat!("--dispatch", "-id"),
        concat!("--branch", "-closure-id"),
        concat!("FEATUREFORGE", "_ALLOW_INTERNAL_EXECUTION_FLAGS"),
        concat!("unit", "-review receipt"),
        concat!("task", "-verification receipt"),
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

fn json_contains_stale_preflight_next_action(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            (key == "next_action" && value.as_str() == Some(concat!("execution pre", "flight")))
                || json_contains_stale_preflight_next_action(value)
        }),
        Value::Array(values) => values.iter().any(json_contains_stale_preflight_next_action),
        _ => false,
    }
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

    let stale_next_action = json!({
        "next_action": concat!("execution pre", "flight"),
    });
    let (hidden, _) = public_json_hidden_token_violation(&stale_next_action)
        .expect("hidden-token detector should reject stale preflight next_action values");
    assert_eq!(
        hidden,
        concat!("next_action=", "execution pre", "flight"),
        "hidden-token detector should identify the retired preflight next_action field value"
    );

    for leaked in [
        concat!("--branch", "-closure-id"),
        concat!("FEATUREFORGE", "_ALLOW_INTERNAL_EXECUTION_FLAGS"),
    ] {
        let value = json!({
            "recommended_public_command_argv": ["featureforge", "plan", "execution", "advance-late-stage", leaked],
        });
        let (hidden, _) = public_json_hidden_token_violation(&value)
            .expect("hidden-token detector should reject hidden compatibility flag/env leaks");
        assert_eq!(
            hidden, leaked,
            "hidden-token detector should identify hidden compatibility leak `{leaked}`"
        );
    }

    let allowed_route_state = json!({
        "phase_detail": concat!("execution_pre", "flight_required"),
        "repo_state_drift_state": concat!("pre", "flight_pending"),
        "next_action": "continue execution",
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

fn write_old_session_spec(repo: &Path) {
    write_file(
        &repo.join(OLD_SESSION_SPEC_REL),
        "# Public Replay Old Session Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-011][behavior] Public routing must keep stale earlier task boundaries ahead of later resume overlays.\n- [REQ-012][behavior] Authoritative run identity must survive missing or malformed execution control-plane acceptance state.\n- [REQ-013][behavior] Later parked open-step state must not mask earlier stale repair boundaries.\n- [REQ-014][behavior] Missing current task closure baseline routes through public close-current-task.\n- [REQ-015][behavior] Earliest stale boundary selection must not jump to a later stale task after repair.\n- [REQ-016][behavior] Current positive task closures allow downstream begin even if review projection artifacts drift.\n",
    );
}

fn write_old_session_fs11_plan(repo: &Path) {
    write_file(
        &repo.join(OLD_SESSION_FS11_PLAN_REL),
        &format!(
            "# Public Replay Old Session FS11 Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** featureforge:executing-plans\n**Source Spec:** `{OLD_SESSION_SPEC_REL}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n**QA Requirement:** not-required\n\n## Requirement Coverage Matrix\n\n- REQ-011 -> Task 2, Task 3\n- REQ-013 -> Task 2, Task 3\n\n## Execution Strategy\n\n- Seed a forward resume overlay on Task 3 Step 6 while keeping Task 2 as the earliest stale boundary.\n\n## Dependency Diagram\n\n```text\nTask 2 -> Task 3\n```\n\n## Task 2: Earliest stale boundary task\n\n**Spec Coverage:** REQ-011, REQ-013\n**Goal:** Task 2 remains the earliest unresolved stale boundary.\n\n**Context:**\n- Spec Coverage: REQ-011, REQ-013.\n\n**Constraints:**\n- Keep one step for deterministic stale-boundary replay.\n\n**Done when:**\n- Task 2 remains the earliest unresolved stale boundary.\n\n**Files:**\n- Modify: `docs/public-replay-old-session-task-2.md`\n\n- [ ] **Step 1: Execute Task 2 baseline step**\n\n## Task 3: Forward resume overlay task\n\n**Spec Coverage:** REQ-011, REQ-013\n**Goal:** Task 3 Step 6 is the forward overlay target that must never outrank Task 2.\n\n**Context:**\n- Spec Coverage: REQ-011, REQ-013.\n\n**Constraints:**\n- Keep six steps to preserve the exact Task 3 Step 6 contradiction shape.\n\n**Done when:**\n- Task 3 Step 6 is the forward overlay target that must never outrank Task 2.\n\n**Files:**\n- Modify: `docs/public-replay-old-session-task-3.md`\n\n- [ ] **Step 1: Build Task 3 step scaffold**\n- [ ] **Step 2: Build Task 3 step scaffold**\n- [ ] **Step 3: Build Task 3 step scaffold**\n- [ ] **Step 4: Build Task 3 step scaffold**\n- [ ] **Step 5: Build Task 3 step scaffold**\n- [ ] **Step 6: Build Task 3 step scaffold**\n"
        ),
    );
    write_current_pass_plan_fidelity_review_artifact(
        repo,
        ".featureforge/reviews/public-replay-old-session-fs11-plan-fidelity.md",
        OLD_SESSION_FS11_PLAN_REL,
        OLD_SESSION_SPEC_REL,
    );
}

fn write_old_session_fs15_plan(repo: &Path) {
    write_file(
        &repo.join(OLD_SESSION_FS15_PLAN_REL),
        &format!(
            "# Public Replay Old Session FS15 Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** featureforge:executing-plans\n**Source Spec:** `{OLD_SESSION_SPEC_REL}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n**QA Requirement:** not-required\n\n## Requirement Coverage Matrix\n\n- REQ-015 -> Task 1, Task 2, Task 6\n\n## Execution Strategy\n\n- Repair Task 1 through the public task-closure route before resuming forward work.\n- Execute Task 2 before Task 6 to keep stale-boundary ordering deterministic.\n\n## Dependency Diagram\n\n```text\nTask 1 -> Task 2 -> Task 6\n```\n\n## Task 1: Earlier repaired task\n\n**Spec Coverage:** REQ-015\n**Goal:** Recreate the earlier repair transition before stale-boundary targeting continues.\n\n**Context:**\n- Spec Coverage: REQ-015.\n\n**Constraints:**\n- Keep each task to one step for deterministic stale-target routing assertions.\n\n**Done when:**\n- Task 1 has a current positive closure before later stale targets are seeded.\n\n**Files:**\n- Modify: `docs/public-replay-old-session-task-1.md`\n\n- [ ] **Step 1: Repair Task 1 through the public closure route**\n\n## Task 2: Earliest stale boundary\n\n**Spec Coverage:** REQ-015\n**Goal:** Task 2 represents the earliest unresolved stale boundary.\n\n**Context:**\n- Spec Coverage: REQ-015.\n\n**Constraints:**\n- Keep each task to one step for deterministic stale-target routing assertions.\n\n**Done when:**\n- Task 2 represents the earliest unresolved stale boundary.\n\n**Files:**\n- Modify: `docs/public-replay-old-session-task-2.md`\n\n- [ ] **Step 1: Execute Task 2 baseline step**\n\n## Task 6: Later stale overlay target\n\n**Spec Coverage:** REQ-015\n**Goal:** Task 6 represents the later stale overlay that must not outrank Task 2.\n\n**Context:**\n- Spec Coverage: REQ-015.\n\n**Constraints:**\n- Keep each task to one step for deterministic stale-target routing assertions.\n\n**Done when:**\n- Task 6 remains behind Task 2 in stale-boundary targeting.\n\n**Files:**\n- Modify: `docs/public-replay-old-session-task-6.md`\n\n- [ ] **Step 1: Execute Task 6 baseline step**\n"
        ),
    );
    write_current_pass_plan_fidelity_review_artifact(
        repo,
        ".featureforge/reviews/public-replay-old-session-fs15-plan-fidelity.md",
        OLD_SESSION_FS15_PLAN_REL,
        OLD_SESSION_SPEC_REL,
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
    begin_task_for_plan(cli, EXEC_PLAN_REL, task, 1, fingerprint, context)
}

fn begin_task_for_plan(
    cli: &mut PublicCli<'_>,
    plan_rel: &str,
    task: u32,
    step: u32,
    fingerprint: &str,
    context: &str,
) -> Value {
    cli.json(
        &[
            "plan",
            "execution",
            "begin",
            "--plan",
            plan_rel,
            "--task",
            &task.to_string(),
            "--step",
            &step.to_string(),
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            fingerprint,
        ],
        context,
    )
}

fn complete_task_1(cli: &mut PublicCli<'_>, fingerprint: &str, context: &str) -> Value {
    complete_task_for_plan(
        cli,
        EXEC_PLAN_REL,
        1,
        1,
        "docs/public-replay-output.md",
        fingerprint,
        context,
    )
}

fn complete_task_for_plan(
    cli: &mut PublicCli<'_>,
    plan_rel: &str,
    task: u32,
    step: u32,
    file_rel: &str,
    fingerprint: &str,
    context: &str,
) -> Value {
    cli.json(
        &[
            "plan",
            "execution",
            "complete",
            "--plan",
            plan_rel,
            "--task",
            &task.to_string(),
            "--step",
            &step.to_string(),
            "--source",
            "featureforge:executing-plans",
            "--claim",
            &format!("Completed public replay Task {task}."),
            "--manual-verify-summary",
            &format!("Verified public replay Task {task}."),
            "--file",
            file_rel,
            "--expect-execution-fingerprint",
            fingerprint,
        ],
        context,
    )
}

fn reopen_task_1(cli: &mut PublicCli<'_>, fingerprint: &str, context: &str) -> Value {
    reopen_task_for_plan(cli, EXEC_PLAN_REL, 1, 1, fingerprint, context)
}

fn reopen_task_for_plan(
    cli: &mut PublicCli<'_>,
    plan_rel: &str,
    task: u32,
    step: u32,
    fingerprint: &str,
    context: &str,
) -> Value {
    cli.json(
        &[
            "plan",
            "execution",
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            &task.to_string(),
            "--step",
            &step.to_string(),
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
    write_task_summary_files(repo, 1)
}

fn write_task_summary_files(repo: &Path, task: u32) -> (PathBuf, PathBuf) {
    let review_summary = repo.join(format!("docs/task-{task}-review-summary.md"));
    let verification_summary = repo.join(format!("docs/task-{task}-verification-summary.md"));
    write_file(
        &review_summary,
        &format!("Task {task} public replay review passed.\n"),
    );
    write_file(
        &verification_summary,
        &format!("Task {task} public replay verification passed.\n"),
    );
    (review_summary, verification_summary)
}

fn close_task_1(cli: &mut PublicCli<'_>, repo: &Path, context: &str) -> Value {
    close_task_for_plan(cli, repo, EXEC_PLAN_REL, 1, context)
}

fn close_task_for_plan(
    cli: &mut PublicCli<'_>,
    repo: &Path,
    plan_rel: &str,
    task: u32,
    context: &str,
) -> Value {
    let (review_summary, verification_summary) = write_task_summary_files(repo, task);
    close_task_for_plan_with_summary_files(
        cli,
        plan_rel,
        task,
        &review_summary,
        &verification_summary,
        context,
    )
}

fn close_task_1_with_summary_files(
    cli: &mut PublicCli<'_>,
    review_summary: &Path,
    verification_summary: &Path,
    context: &str,
) -> Value {
    close_task_for_plan_with_summary_files(
        cli,
        EXEC_PLAN_REL,
        1,
        review_summary,
        verification_summary,
        context,
    )
}

fn close_task_for_plan_with_summary_files(
    cli: &mut PublicCli<'_>,
    plan_rel: &str,
    task: u32,
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
            plan_rel,
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
    let args = command_parts[1..].to_vec();
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
    assert_public_argv_has_no_display_only_tokens(&command_parts, context);
    command_parts
}

fn assert_public_argv_has_no_display_only_tokens(command_parts: &[String], context: &str) {
    const DISPLAY_ONLY_DENYLIST: &[&str] = &[
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
    for part in command_parts {
        for denied in DISPLAY_ONLY_DENYLIST {
            assert!(
                part != denied,
                "{context}: recommended argv must not include display-only token `{denied}`, got {command_parts:?}"
            );
            assert!(
                part.split_once('=')
                    .is_none_or(|(_, value)| value != *denied),
                "{context}: recommended argv must not include display-only token assignment `{denied}`, got {command_parts:?}"
            );
        }
    }
    assert!(
        !command_parts.windows(3).any(|window| {
            (window[0] == "[when" || window[0] == "when")
                && window[1] == "verification"
                && (window[2] == "ran]" || window[2] == "ran")
        }),
        "{context}: recommended argv must not include display-only optional prose tokens, got {command_parts:?}"
    );
}

fn assert_recommended_public_command_targets_task(
    value: &Value,
    expected_command: &str,
    expected_task: u32,
    context: &str,
) {
    match value.get("recommended_public_command_argv") {
        Some(argv) if argv.is_array() => {
            let command_parts = public_recommended_command_argv(value, context);
            assert_recommended_public_command_parts_include(
                &command_parts,
                expected_command,
                context,
            );
            let expected_task_arg = expected_task.to_string();
            assert!(
                command_parts
                    .windows(2)
                    .any(|window| window[0] == "--task" && window[1] == expected_task_arg),
                "{context}: recommended argv should target Task {expected_task}, got {command_parts:?}"
            );
            return;
        }
        Some(argv) if argv.is_null() => {}
        None => {}
        Some(argv) => {
            panic!(
                "{context}: recommended_public_command_argv must be executable argv, null, or absent; got {argv}: {value}"
            )
        }
    }

    assert_required_inputs_present(value, context);
    assert_public_route_targets_task(value, expected_task, context);
    match expected_command {
        "close-current-task" => {
            assert_close_current_task_required_inputs(value, context);
            let exposes_close_intent = value["next_action"] == "close current task"
                || value["phase_detail"] == "task_closure_recording_ready";
            assert!(
                exposes_close_intent,
                "{context}: argv-absent route should still expose close-current-task intent: {value}"
            );
        }
        command => {
            panic!("{context}: unsupported argv-absent expected command `{command}`: {value}")
        }
    }
}

fn assert_recommended_public_command_parts_include(
    command_parts: &[String],
    expected_command: &str,
    context: &str,
) {
    assert!(
        command_parts.iter().any(|part| part == expected_command),
        "{context}: recommended argv should route through `{expected_command}`, got {command_parts:?}"
    );
}

fn assert_public_route_targets_task(value: &Value, expected_task: u32, context: &str) {
    let targets_expected_task = value["blocking_task"].as_u64() == Some(u64::from(expected_task))
        || value["task_number"].as_u64() == Some(u64::from(expected_task))
        || value["execution_command_context"]["task_number"].as_u64()
            == Some(u64::from(expected_task));
    assert!(
        targets_expected_task,
        "{context}: public route should target Task {expected_task}: {value}"
    );
}

fn assert_public_command_budget(label: &str, observed: usize, max: usize) {
    assert!(
        observed <= max,
        "{label}: public replay exceeded command budget {observed}/{max}"
    );
}

fn assert_required_inputs_present<'a>(value: &'a Value, context: &str) -> &'a [Value] {
    let required_inputs = value["required_inputs"].as_array().unwrap_or_else(|| {
        panic!("{context}: argv-absent route should expose required_inputs: {value}")
    });
    assert!(
        !required_inputs.is_empty(),
        "{context}: argv-absent route should explain missing inputs: {value}"
    );
    required_inputs
}

fn assert_close_current_task_required_inputs(value: &Value, context: &str) {
    let required_inputs = assert_required_inputs_present(value, context);
    let names = required_inputs
        .iter()
        .map(|input| input["name"].as_str().unwrap_or("<missing-name>"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "review_result",
            "review_summary_file",
            "verification_result",
            "verification_summary_file"
        ]),
        "{context}: close-current-task should expose exactly the typed review and verification inputs: {value}"
    );
    assert_required_enum_input(
        required_inputs,
        "review_result",
        &["pass", "fail"],
        context,
        value,
    );
    assert_required_path_input(required_inputs, "review_summary_file", None, context, value);
    assert_required_enum_input(
        required_inputs,
        "verification_result",
        &["pass", "fail", "not-run"],
        context,
        value,
    );
    assert_required_path_input(
        required_inputs,
        "verification_summary_file",
        Some("verification_result!=not-run"),
        context,
        value,
    );
}

fn assert_required_enum_input(
    required_inputs: &[Value],
    name: &str,
    expected_values: &[&str],
    context: &str,
    route: &Value,
) {
    let input = required_input_by_name(required_inputs, name, context, route);
    assert_eq!(
        input["kind"], "enum",
        "{context}: required input `{name}` should be enum typed: {route}"
    );
    let values = input["values"].as_array().unwrap_or_else(|| {
        panic!("{context}: required enum input `{name}` should expose values: {route}")
    });
    let actual_values = values
        .iter()
        .map(|value| {
            value.as_str().unwrap_or_else(|| {
                panic!("{context}: required enum input `{name}` values should be strings: {route}")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual_values, expected_values,
        "{context}: required enum input `{name}` should expose exact allowed values: {route}"
    );
}

fn assert_required_path_input(
    required_inputs: &[Value],
    name: &str,
    required_when: Option<&str>,
    context: &str,
    route: &Value,
) {
    let input = required_input_by_name(required_inputs, name, context, route);
    assert_eq!(
        input["kind"], "path",
        "{context}: required input `{name}` should be path typed: {route}"
    );
    assert_eq!(
        input["must_exist"], true,
        "{context}: required path input `{name}` should require an existing file: {route}"
    );
    match required_when {
        Some(expected) => assert_eq!(
            input["required_when"], expected,
            "{context}: conditional path input `{name}` should expose its condition: {route}"
        ),
        None => assert!(
            input.get("required_when").is_none(),
            "{context}: unconditional path input `{name}` should not expose required_when: {route}"
        ),
    }
}

fn required_input_by_name<'a>(
    required_inputs: &'a [Value],
    name: &str,
    context: &str,
    route: &Value,
) -> &'a Value {
    required_inputs
        .iter()
        .find(|input| input["name"] == name)
        .unwrap_or_else(|| panic!("{context}: required input `{name}` missing: {route}"))
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
    assert_eq!(
        value["phase_detail"],
        json!("runtime_reconcile_required"),
        "{context} should expose a runtime reconcile diagnostic route: {value}"
    );
    assert_eq!(
        value["state_kind"],
        json!("runtime_reconcile_required"),
        "{context} should classify targetless stale reconcile as runtime_reconcile_required: {value}"
    );
    assert_eq!(
        value["next_action"], "runtime diagnostic required",
        "{context} diagnostic-only route must not publish reentry wording: {value}"
    );
    assert!(
        value.get("recommended_command").is_none_or(Value::is_null),
        "{context} must not recommend a mutation command: {value}"
    );
    assert!(
        value
            .get("recommended_public_command_argv")
            .is_none_or(Value::is_null),
        "{context} must not expose executable argv when no authoritative target exists: {value}"
    );
    assert!(
        value["required_inputs"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "{context} must not expose typed inputs for diagnostic-only targetless stale state: {value}"
    );
    assert!(
        value.get("next_public_action").is_none_or(Value::is_null),
        "{context} must not expose a next public action for diagnostic-only targetless stale state: {value}"
    );
    assert!(
        value["public_repair_targets"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "{context} must not expose public repair targets for diagnostic-only targetless stale state: {value}"
    );
    assert!(
        value["blockers"].as_array().is_none_or(Vec::is_empty),
        "{context} must not expose route blockers for diagnostic-only targetless stale state: {value}"
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
    assert_eq!(
        value["next_action"], "runtime diagnostic required",
        "{context} blocked runtime bug must publish diagnostic next_action: {value}"
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

fn execution_acceptance_state_path(repo: &Path, state: &Path) -> PathBuf {
    let (repo_slug, branch_name) = state_identity(repo);
    state
        .join("projects")
        .join(repo_slug)
        .join("branches")
        .join(branch_storage_key(&branch_name))
        .join(concat!("execution-pre", "flight"))
        .join("acceptance-state.json")
}

fn corrupt_execution_acceptance_state(repo: &Path, state: &Path) {
    let path = execution_acceptance_state_path(repo, state);
    assert!(
        path.is_file(),
        "fixture should have persisted public begin acceptance state at {}",
        path.display()
    );
    write_file(
        &path,
        concat!(
            "{ malformed execution pre",
            "flight acceptance for public replay }\n"
        ),
    );
}

fn seed_old_session_fs11_stale_boundary_fixture(repo: &Path, state: &Path) {
    // Fixture-only quarantine: this creates the historical broken runtime shape;
    // the replay assertions below use only the compiled public CLI.
    update_state_fields(
        repo,
        state,
        &[
            (
                "task_closure_record_history",
                json!({
                    "task-2-stale-old-session": {
                        "closure_record_id": "task-2-stale-old-session",
                        "task": 2,
                        "source_plan_path": OLD_SESSION_FS11_PLAN_REL,
                        "source_plan_revision": 1,
                        "record_sequence": 10,
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["docs/public-replay-old-session-task-2.md"]
                    }
                }),
            ),
            (
                "current_open_step_state",
                json!({
                    "task": 3,
                    "step": 6,
                    "note_state": "Interrupted",
                    "note_summary": "FS-11 forward reentry overlay must not outrank stale Task 2 boundary",
                    "source_plan_path": OLD_SESSION_FS11_PLAN_REL,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 30
                }),
            ),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
            ("resume_task", json!(3)),
            ("resume_step", json!(6)),
        ],
    );
}

fn seed_old_session_fs15_stale_boundary_fixture(repo: &Path, state: &Path) {
    // Fixture-only quarantine: the synthetic stale records encode the old
    // later-target failure; replay remains public CLI only.
    update_state_fields(
        repo,
        state,
        &[
            (
                "task_closure_record_history",
                json!({
                    "task-2-stale-old-session": {
                        "closure_record_id": "task-2-stale-old-session",
                        "task": 2,
                        "source_plan_path": OLD_SESSION_FS15_PLAN_REL,
                        "source_plan_revision": 1,
                        "record_sequence": 10,
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["docs/public-replay-old-session-task-2.md"]
                    },
                    "task-6-stale-old-session": {
                        "closure_record_id": "task-6-stale-old-session",
                        "task": 6,
                        "source_plan_path": OLD_SESSION_FS15_PLAN_REL,
                        "source_plan_revision": 1,
                        "record_sequence": 20,
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["docs/public-replay-old-session-task-6.md"]
                    }
                }),
            ),
            (
                "current_open_step_state",
                json!({
                    "task": 6,
                    "step": 1,
                    "note_state": "Interrupted",
                    "note_summary": "FS-15 later resume overlay must not outrank stale Task 2 boundary",
                    "source_plan_path": OLD_SESSION_FS15_PLAN_REL,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 30
                }),
            ),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
            ("resume_task", json!(6)),
            ("resume_step", json!(1)),
        ],
    );
}

fn setup_execution_fixture(name: &str) -> (TempDir, TempDir) {
    let (repo_dir, state_dir) = init_repo(name);
    write_execution_spec_and_plan(repo_dir.path());
    commit_all(repo_dir.path(), "public replay execution fixture");
    (repo_dir, state_dir)
}

fn setup_old_session_fs11_fixture(name: &str) -> (TempDir, TempDir) {
    let (repo_dir, state_dir) = init_repo(name);
    let repo = repo_dir.path();
    write_old_session_spec(repo);
    write_old_session_fs11_plan(repo);
    write_file(
        &repo.join("docs/public-replay-old-session-task-2.md"),
        "Old session Task 2 baseline.\n",
    );
    write_file(
        &repo.join("docs/public-replay-old-session-task-3.md"),
        "Old session Task 3 baseline.\n",
    );
    commit_all(repo, "public replay old session FS11 fixture");

    let mut cli = PublicCli::new(repo, state_dir.path());
    let status_before = status_for_plan(&mut cli, OLD_SESSION_FS11_PLAN_REL, "FS-11 setup status");
    let begin = begin_task_for_plan(
        &mut cli,
        OLD_SESSION_FS11_PLAN_REL,
        2,
        1,
        status_before["execution_fingerprint"]
            .as_str()
            .expect("FS-11 setup status should expose execution fingerprint"),
        "FS-11 setup public begin task 2",
    );
    append_repo_file(
        repo,
        "docs/public-replay-old-session-task-2.md",
        "Old session Task 2 changed before stale-boundary replay.",
    );
    complete_task_for_plan(
        &mut cli,
        OLD_SESSION_FS11_PLAN_REL,
        2,
        1,
        "docs/public-replay-old-session-task-2.md",
        begin["execution_fingerprint"]
            .as_str()
            .expect("FS-11 setup begin should expose execution fingerprint"),
        "FS-11 setup public complete task 2",
    );
    seed_old_session_fs11_stale_boundary_fixture(repo, state_dir.path());
    (repo_dir, state_dir)
}

fn setup_old_session_fs15_fixture(name: &str) -> (TempDir, TempDir) {
    let (repo_dir, state_dir) = init_repo(name);
    let repo = repo_dir.path();
    write_old_session_spec(repo);
    write_old_session_fs15_plan(repo);
    for task in [1, 2, 6] {
        write_file(
            &repo.join(format!("docs/public-replay-old-session-task-{task}.md")),
            &format!("Old session Task {task} baseline.\n"),
        );
    }
    commit_all(repo, "public replay old session FS15 fixture");

    let mut cli = PublicCli::new(repo, state_dir.path());
    let status_before = status_for_plan(&mut cli, OLD_SESSION_FS15_PLAN_REL, "FS-15 setup status");
    let begin_task_1 = begin_task_for_plan(
        &mut cli,
        OLD_SESSION_FS15_PLAN_REL,
        1,
        1,
        status_before["execution_fingerprint"]
            .as_str()
            .expect("FS-15 setup status should expose execution fingerprint"),
        "FS-15 setup public begin task 1",
    );
    append_repo_file(
        repo,
        "docs/public-replay-old-session-task-1.md",
        "Old session Task 1 changed before current closure.",
    );
    complete_task_for_plan(
        &mut cli,
        OLD_SESSION_FS15_PLAN_REL,
        1,
        1,
        "docs/public-replay-old-session-task-1.md",
        begin_task_1["execution_fingerprint"]
            .as_str()
            .expect("FS-15 setup begin task 1 should expose execution fingerprint"),
        "FS-15 setup public complete task 1",
    );
    close_task_for_plan(
        &mut cli,
        repo,
        OLD_SESSION_FS15_PLAN_REL,
        1,
        "FS-15 setup public close task 1",
    );
    let status_after_task_1 = status_for_plan(
        &mut cli,
        OLD_SESSION_FS15_PLAN_REL,
        "FS-15 setup status after task 1",
    );
    let begin_task_2 = begin_task_for_plan(
        &mut cli,
        OLD_SESSION_FS15_PLAN_REL,
        2,
        1,
        status_after_task_1["execution_fingerprint"]
            .as_str()
            .expect("FS-15 setup status after task 1 should expose execution fingerprint"),
        "FS-15 setup public begin task 2",
    );
    append_repo_file(
        repo,
        "docs/public-replay-old-session-task-2.md",
        "Old session Task 2 changed before stale-boundary replay.",
    );
    complete_task_for_plan(
        &mut cli,
        OLD_SESSION_FS15_PLAN_REL,
        2,
        1,
        "docs/public-replay-old-session-task-2.md",
        begin_task_2["execution_fingerprint"]
            .as_str()
            .expect("FS-15 setup begin task 2 should expose execution fingerprint"),
        "FS-15 setup public complete task 2",
    );
    seed_old_session_fs15_stale_boundary_fixture(repo, state_dir.path());
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
fn public_replay_recommended_argv_handles_plan_paths_with_spaces_and_literal_punctuation() {
    let spaced_plan_rel =
        "docs/featureforge/plans/public replay [release]|candidate execution plan.md";
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
        "recommended argv should keep the plan path with spaces and literal template punctuation as one argv element: {initial_status}"
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
        "argv replay should begin Task 1 even though the plan path contains spaces and literal template punctuation: {begin}"
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
fn public_replay_fs11_rebase_resume_targets_earliest_boundary_within_budget() {
    let (repo_dir, state_dir) = setup_old_session_fs11_fixture("public-replay-fs11-rebase-resume");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    let checkpoint = cli.checkpoint();

    let operator = workflow_operator(
        &mut cli,
        OLD_SESSION_FS11_PLAN_REL,
        "FS-11 public replay operator",
    );
    assert_public_json_excludes_hidden_tokens(&operator, "FS-11 public replay operator");
    assert_public_route_targets_task(&operator, 2, "FS-11 public replay operator");
    assert!(
        !operator["recommended_command"]
            .as_str()
            .unwrap_or_default()
            .contains("--task 3"),
        "FS-11 public replay must not recommend the later Task 3 begin dead end: {operator}"
    );
    assert_recommended_public_command_targets_task(
        &operator,
        "reopen",
        2,
        "FS-11 public replay operator",
    );

    let reopened = invoke_recommended_public_command_for_context(
        &mut cli,
        &operator,
        "FS-11 public replay operator-routed reopen",
    );
    assert_eq!(
        reopened["resume_task"],
        json!(2),
        "FS-11 public replay should resume the earliest stale Task 2 boundary: {reopened}"
    );
    assert_public_command_budget(
        "FS11-PUBLIC-REPLAY-BUDGET",
        cli.delta_since(&checkpoint, "workflow operator") + cli.delta_since(&checkpoint, "reopen"),
        3,
    );
}

#[test]
fn public_replay_fs12_authoritative_run_survives_malformed_preflight_within_budget() {
    let (repo_dir, state_dir) =
        setup_execution_fixture(concat!("public-replay-fs12-pre", "flight"));
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);

    let status_before = status(&mut cli, "FS-12 public replay setup status");
    let begin = begin_task(
        &mut cli,
        1,
        status_before["execution_fingerprint"]
            .as_str()
            .expect("FS-12 setup status should expose execution fingerprint"),
        "FS-12 public replay setup begin",
    );
    let execution_run_id = begin["execution_run_id"]
        .as_str()
        .expect("FS-12 setup begin should expose execution_run_id")
        .to_owned();
    corrupt_execution_acceptance_state(repo, state);
    let checkpoint = cli.checkpoint();

    let operator = workflow_operator(
        &mut cli,
        EXEC_PLAN_REL,
        concat!("FS-12 public replay operator after malformed pre", "flight"),
    );
    assert_public_json_excludes_hidden_tokens(&operator, "FS-12 public replay operator");
    assert_ne!(
        operator["next_action"],
        json!("execution preflight"),
        "FS-12 public replay must not route back to execution acceptance when authoritative run identity exists: {operator}"
    );

    append_repo_file(
        repo,
        "docs/public-replay-output.md",
        "FS-12 public replay work after malformed acceptance state.",
    );
    let complete = complete_task_1(
        &mut cli,
        begin["execution_fingerprint"]
            .as_str()
            .expect("FS-12 setup begin should expose execution fingerprint"),
        concat!("FS-12 public replay complete after malformed pre", "flight"),
    );
    assert_public_json_excludes_hidden_tokens(&complete, "FS-12 public replay complete");
    assert_eq!(
        complete["execution_run_id"],
        json!(execution_run_id),
        "FS-12 public replay complete should preserve authoritative run identity: {complete}"
    );
    let close = close_task_1(
        &mut cli,
        repo,
        concat!(
            "FS-12 public replay close-current-task after malformed pre",
            "flight"
        ),
    );
    assert_public_json_excludes_hidden_tokens(&close, "FS-12 public replay close-current-task");
    assert!(
        matches!(
            close["action"].as_str(),
            Some("recorded" | "already_current")
        ),
        "FS-12 public replay close-current-task should work without hidden acceptance replay: {close}"
    );
    assert_public_command_budget(
        "FS12-PUBLIC-REPLAY-BUDGET",
        cli.delta_since(&checkpoint, "workflow operator")
            + cli.delta_since(&checkpoint, "complete")
            + cli.delta_since(&checkpoint, "close-current-task"),
        3,
    );
}

#[test]
fn public_replay_fs13_later_open_step_does_not_mask_earlier_boundary() {
    let (repo_dir, state_dir) =
        setup_old_session_fs11_fixture("public-replay-fs13-later-open-step");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    let checkpoint = cli.checkpoint();

    let operator = workflow_operator(
        &mut cli,
        OLD_SESSION_FS11_PLAN_REL,
        "FS-13 public replay operator before recovery",
    );
    assert_public_json_excludes_hidden_tokens(&operator, "FS-13 public replay operator");
    assert_public_route_targets_task(&operator, 2, "FS-13 public replay operator");
    assert_recommended_public_command_targets_task(
        &operator,
        "reopen",
        2,
        "FS-13 public replay operator",
    );

    let reopened = invoke_recommended_public_command_for_context(
        &mut cli,
        &operator,
        "FS-13 public replay operator-routed reopen",
    );
    assert_eq!(
        reopened["resume_task"],
        json!(2),
        "FS-13 public replay should reopen the earlier stale Task 2 boundary: {reopened}"
    );
    let status_after_repair = status_for_plan(
        &mut cli,
        OLD_SESSION_FS11_PLAN_REL,
        "FS-13 status after reopen",
    );
    assert_ne!(
        status_after_repair["resume_task"],
        json!(3),
        "FS-13 public replay should suppress the later parked Task 3 resume marker after repair: {status_after_repair}"
    );
    assert_public_route_targets_task(&status_after_repair, 2, "FS-13 status after repair");
    assert_public_command_budget(
        "FS13-PUBLIC-REPLAY-BUDGET",
        cli.delta_since(&checkpoint, "workflow operator")
            + cli.delta_since(&checkpoint, "reopen")
            + cli.delta_since(&checkpoint, "status"),
        3,
    );
}

#[test]
fn public_replay_fs14_missing_closure_baseline_routes_to_close_within_budget() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-fs14-missing-closure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    complete_task_1_without_closure(&mut cli, repo);
    let checkpoint = cli.checkpoint();

    let operator = workflow_operator(&mut cli, EXEC_PLAN_REL, "FS-14 public replay operator");
    assert_public_json_excludes_hidden_tokens(&operator, "FS-14 public replay operator");
    assert_eq!(
        operator["phase_detail"], "task_closure_recording_ready",
        "FS-14 public replay should route missing closure baseline to task closure recording: {operator}"
    );
    assert_recommended_public_command_targets_task(
        &operator,
        "close-current-task",
        1,
        "FS-14 public replay operator",
    );
    assert_eq!(
        operator["recommended_public_command_argv"],
        Value::Null,
        "FS-14 close-current-task should omit argv until review and verification inputs are supplied: {operator}"
    );
    let close = close_task_1(&mut cli, repo, "FS-14 public replay close-current-task");
    assert!(
        matches!(
            close["action"].as_str(),
            Some("recorded" | "already_current")
        ),
        "FS-14 public replay close-current-task should rebuild the closure baseline: {close}"
    );
    assert_public_command_budget(
        "FS14-PUBLIC-REPLAY-BUDGET",
        cli.delta_since(&checkpoint, "workflow operator")
            + cli.delta_since(&checkpoint, "close-current-task"),
        2,
    );
}

#[test]
fn public_replay_fs15_repair_keeps_earliest_stale_boundary_within_budget() {
    let (repo_dir, state_dir) = setup_old_session_fs15_fixture("public-replay-fs15-earliest-stale");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    let checkpoint = cli.checkpoint();

    let operator = workflow_operator(
        &mut cli,
        OLD_SESSION_FS15_PLAN_REL,
        "FS-15 public replay operator",
    );
    assert_public_json_excludes_hidden_tokens(&operator, "FS-15 public replay operator");
    assert_public_route_targets_task(&operator, 2, "FS-15 public replay operator");
    assert_recommended_public_command_targets_task(
        &operator,
        "reopen",
        2,
        "FS-15 public replay operator",
    );

    let reopened = invoke_recommended_public_command_for_context(
        &mut cli,
        &operator,
        "FS-15 public replay operator-routed reopen",
    );
    assert_eq!(
        reopened["resume_task"],
        json!(2),
        "FS-15 public replay should reopen the earliest stale Task 2 boundary: {reopened}"
    );
    assert!(
        !operator["recommended_command"]
            .as_str()
            .unwrap_or_default()
            .contains("--task 6"),
        "FS-15 public replay operator must not jump to the later Task 6 stale target: {operator}"
    );
    assert_public_command_budget(
        "FS15-PUBLIC-REPLAY-BUDGET",
        cli.delta_since(&checkpoint, "workflow operator") + cli.delta_since(&checkpoint, "reopen"),
        2,
    );
}

#[test]
fn public_replay_fs16_current_closure_allows_next_begin_after_projection_drift() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-fs16-projection-drift");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);
    complete_task_1_without_closure(&mut cli, repo);
    let close = close_task_1(&mut cli, repo, "FS-16 setup close current task 1");
    assert_eq!(close["action"], "recorded");
    let status_after_close = status(&mut cli, "FS-16 setup status after close");
    let closure_id = current_task_1_closure_id(&status_after_close);
    let materialized = cli.json(
        &[
            "plan",
            "execution",
            "materialize-projections",
            "--plan",
            EXEC_PLAN_REL,
        ],
        "FS-16 setup explicit projection materialization",
    );
    assert_eq!(materialized["action"], json!("materialized"));
    let removed_projection_artifacts =
        remove_task_projection_artifacts(repo, state, &status_after_close, 1);
    assert!(
        !removed_projection_artifacts.is_empty(),
        "FS-16 setup should remove projection artifacts before public replay"
    );
    let checkpoint = cli.checkpoint();

    let status_after_drift = status(
        &mut cli,
        "FS-16 public replay status after projection drift",
    );
    assert_eq!(
        current_task_1_closure_id(&status_after_drift),
        closure_id,
        "FS-16 public replay should keep the current positive closure despite projection drift"
    );
    assert_public_json_excludes_hidden_tokens(&status_after_drift, "FS-16 public replay status");
    assert_recommended_public_command_targets_task(
        &status_after_drift,
        "begin",
        2,
        "FS-16 public replay status",
    );
    let begin = begin_task(
        &mut cli,
        2,
        status_after_drift["execution_fingerprint"]
            .as_str()
            .expect("FS-16 status should expose execution fingerprint"),
        "FS-16 public replay begin task 2",
    );
    assert_public_json_excludes_hidden_tokens(&begin, "FS-16 public replay begin task 2");
    assert_eq!(
        begin["active_task"],
        json!(2),
        "FS-16 public replay should allow downstream begin without regenerating receipt projections"
    );
    assert_public_command_budget(
        "FS16-PUBLIC-REPLAY-BUDGET",
        cli.delta_since(&checkpoint, "status") + cli.delta_since(&checkpoint, "begin"),
        2,
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
    assert_recommended_public_command_targets_task(
        &operator,
        "close-current-task",
        1,
        "public replay close route operator",
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
    assert_recommended_public_command_targets_task(
        &operator,
        "close-current-task",
        1,
        "public replay preclosure projection operator",
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
    assert_recommended_public_command_targets_task(
        &repair,
        "close-current-task",
        1,
        "current closure repair repair-review-state",
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
    assert_recommended_public_command_targets_task(
        &operator_after_complete,
        "close-current-task",
        1,
        "recommended parity close operator",
    );
    let close = close_task_1(&mut cli, repo, "recommended parity close task 1");
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
    assert_recommended_public_command_targets_task(
        &repair,
        "close-current-task",
        1,
        "stale repair parity next recommended command",
    );
    let next = close_task_1(
        &mut cli,
        repo,
        "stale repair parity next close-current-task",
    );
    loop_detector.observe_after_command(&next, "stale repair parity next close-current-task");
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
fn public_replay_real_targetless_stale_reconcile_emits_runtime_reconcile_state_kind() {
    let (repo_dir, state_dir) = setup_execution_fixture("public-replay-real-targetless-stale");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let mut cli = PublicCli::new(repo, state);

    let status_before_task_1 = status(&mut cli, "targetless stale setup status before task 1");
    let begin_task_1 = begin_task(
        &mut cli,
        1,
        status_before_task_1["execution_fingerprint"]
            .as_str()
            .expect("status before task 1 should expose execution fingerprint"),
        "targetless stale setup begin task 1",
    );
    append_repo_file(
        repo,
        "docs/public-replay-output.md",
        "Task 1 changed before targetless stale setup.",
    );
    complete_task_1(
        &mut cli,
        begin_task_1["execution_fingerprint"]
            .as_str()
            .expect("begin task 1 should expose execution fingerprint"),
        "targetless stale setup complete task 1",
    );
    close_task_1(&mut cli, repo, "targetless stale setup close task 1");

    let status_before_task_2 = status(&mut cli, "targetless stale setup status before task 2");
    let begin_task_2 = begin_task(
        &mut cli,
        2,
        status_before_task_2["execution_fingerprint"]
            .as_str()
            .expect("status before task 2 should expose execution fingerprint"),
        "targetless stale setup begin task 2",
    );
    append_repo_file(
        repo,
        "docs/public-replay-followup.md",
        "Task 2 changed before targetless stale setup.",
    );
    complete_task_for_plan(
        &mut cli,
        EXEC_PLAN_REL,
        2,
        1,
        "docs/public-replay-followup.md",
        begin_task_2["execution_fingerprint"]
            .as_str()
            .expect("begin task 2 should expose execution fingerprint"),
        "targetless stale setup complete task 2",
    );
    update_state_fields(
        repo,
        state,
        &[
            (
                "current_open_step_state",
                json!({
                    "task": 2,
                    "step": 1,
                    "note_state": "Interrupted",
                    "note_summary": "Public replay targetless stale reconcile must remain diagnostic-only.",
                    "source_plan_path": EXEC_PLAN_REL,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 1
                }),
            ),
            ("harness_phase", json!("document_release_pending")),
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
        ],
    );

    let status_json = status(
        &mut cli,
        "public replay real targetless stale status without injection",
    );
    assert_targetless_stale_public_surface(
        &status_json,
        "real targetless stale status without injection",
    );

    let operator_json = workflow_operator(
        &mut cli,
        EXEC_PLAN_REL,
        "public replay real targetless stale operator without injection",
    );
    assert_targetless_stale_public_surface(
        &operator_json,
        "real targetless stale operator without injection",
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
