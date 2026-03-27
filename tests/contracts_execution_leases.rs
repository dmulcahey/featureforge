#[path = "support/files.rs"]
mod files_support;
#[path = "support/json.rs"]
mod json_support;
#[path = "support/process.rs"]
mod process_support;

use assert_cmd::cargo::CommandCargoExt;
use featureforge::paths::branch_storage_key;
use files_support::write_file;
use json_support::parse_json;
use process_support::{run, run_checked};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-17-example-execution-plan.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-03-17-example-execution-plan-design.md";

fn init_repo(name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();

    run_checked(
        {
            let mut command = Command::new("git");
            command.arg("init").current_dir(repo);
            command
        },
        "git init",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["config", "user.name", "FeatureForge Test"])
                .current_dir(repo);
            command
        },
        "git config user.name",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["config", "user.email", "featureforge-tests@example.com"])
                .current_dir(repo);
            command
        },
        "git config user.email",
    );
    write_file(&repo.join("README.md"), &format!("# {name}\n"));
    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["add", "README.md"]).current_dir(repo);
            command
        },
        "git add README",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["commit", "-m", "init"]).current_dir(repo);
            command
        },
        "git commit init",
    );
    run_checked(
        {
            let mut command = Command::new("git");
            command.args(["checkout", "-B", "fixture-work"]).current_dir(repo);
            command
        },
        "git checkout fixture-work",
    );

    (repo_dir, state_dir)
}

fn write_approved_spec(repo: &Path) {
    write_file(
        &repo.join(SPEC_REL),
        r#"# Example Execution Plan Design

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

## Requirement Index

- [REQ-001][behavior] Execution fixtures must support a valid single-task plan path for routing and finish-gate coverage.

## Summary

Fixture spec for focused execution-helper regression coverage.
"#,
    );
}

fn write_single_step_plan(repo: &Path, execution_mode: &str) {
    write_file(
        &repo.join(PLAN_REL),
        &format!(
            r#"# Example Execution Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** {execution_mode}
**Source Spec:** `{SPEC_REL}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1

## Execution Strategy

- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for downstream review routing.

## Dependency Diagram

```text
Task 1
```

## Task 1: Single Step Task

**Spec Coverage:** REQ-001
**Task Outcome:** The workspace is prepared for execution.
**Plan Constraints:**
- Keep the fixture single-step and deterministic.
**Open Questions:** none

**Files:**
- Modify: `docs/example-output.md`

- [ ] **Step 1: Prepare the workspace for execution**
"#
        ),
    );
}

fn branch_name(repo: &Path) -> String {
    let output = run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(repo);
            command
        },
        "git rev-parse branch",
    );
    String::from_utf8(output.stdout)
        .expect("branch should be utf-8")
        .trim()
        .to_owned()
}

fn repo_slug(repo: &Path) -> String {
    let output = run_checked(
        {
            let mut command =
                Command::cargo_bin("featureforge").expect("featureforge binary should exist");
            command.current_dir(repo).args(["repo", "slug"]);
            command
        },
        "featureforge repo slug",
    );
    String::from_utf8(output.stdout)
        .expect("repo slug output should be utf-8")
        .lines()
        .find_map(|line| line.strip_prefix("SLUG="))
        .unwrap_or_else(|| panic!("repo slug output should include SLUG=..."))
        .to_owned()
}

fn harness_branch_dir(repo: &Path, state: &Path) -> PathBuf {
    let safe_branch = branch_storage_key(&branch_name(repo));
    state
        .join("projects")
        .join(repo_slug(repo))
        .join("branches")
        .join(safe_branch)
}

fn preflight_acceptance_state_path(repo: &Path, state: &Path) -> PathBuf {
    harness_branch_dir(repo, state)
        .join("execution-preflight")
        .join("acceptance-state.json")
}

fn run_plan_execution_json(
    repo: &Path,
    state: &Path,
    args: &[&str],
    context: &str,
) -> serde_json::Value {
    let mut command =
        Command::cargo_bin("featureforge").expect("featureforge binary should be available");
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state)
        .args(["plan", "execution"])
        .args(args);
    parse_json(&run(command, context), context)
}

#[test]
fn preflight_reclaims_stale_write_authority_lock_before_acceptance() {
    let (repo_dir, state_dir) = init_repo("contracts-execution-leases-reclaim");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "none");
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["checkout", "-B", "execution-preflight-fixture"])
                .current_dir(repo);
            command
        },
        "git checkout execution-preflight-fixture",
    );

    let lock_path = harness_branch_dir(repo, state)
        .join("execution-harness")
        .join("write-authority.lock");
    let stale_pid = {
        let mut child_cmd = Command::new("sh");
        child_cmd.args(["-c", "exit 0"]);
        let mut child = child_cmd
            .spawn()
            .expect("stale write-authority fixture process should spawn");
        let pid = child.id();
        let exit_status = child
            .wait()
            .expect("stale write-authority fixture process should exit");
        assert!(
            exit_status.success(),
            "stale write-authority fixture process should exit successfully"
        );
        pid
    };
    write_file(&lock_path, &format!("pid={stale_pid}\n"));

    let acceptance_path = preflight_acceptance_state_path(repo, state);
    assert!(
        !acceptance_path.exists(),
        "preflight acceptance state should not exist before stale-lock preflight"
    );

    let gate = run_plan_execution_json(
        repo,
        state,
        &["preflight", "--plan", PLAN_REL],
        "preflight should run",
    );

    assert_eq!(gate["allowed"], true, "stale write-authority locks should be reclaimed");
    assert!(
        !lock_path.exists(),
        "stale write-authority lock should be removed after reclamation"
    );
    assert!(
        acceptance_path.exists(),
        "preflight should persist acceptance state after reclaiming stale write authority"
    );
}

#[test]
fn preflight_blocks_live_write_authority_conflict_without_persisting_acceptance() {
    let (repo_dir, state_dir) = init_repo("contracts-execution-leases-conflict");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "none");
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["checkout", "-B", "execution-preflight-fixture"])
                .current_dir(repo);
            command
        },
        "git checkout execution-preflight-fixture",
    );

    let lock_path = harness_branch_dir(repo, state)
        .join("execution-harness")
        .join("write-authority.lock");
    let mut holder_cmd = Command::new("sh");
    holder_cmd.args(["-c", "sleep 30"]);
    let mut holder = holder_cmd
        .spawn()
        .expect("live write-authority fixture process should spawn");
    write_file(&lock_path, &format!("pid={}\n", holder.id()));

    let acceptance_path = preflight_acceptance_state_path(repo, state);
    assert!(
        !acceptance_path.exists(),
        "preflight acceptance state should not exist before live-lock preflight"
    );

    let gate = run_plan_execution_json(
        repo,
        state,
        &["preflight", "--plan", PLAN_REL],
        "preflight should run",
    );
    let _ = holder.kill();
    let _ = holder.wait();

    assert_eq!(gate["allowed"], false);
    assert!(
        gate["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "write_authority_conflict")),
        "preflight should report the write-authority conflict reason code"
    );
    assert!(
        !acceptance_path.exists(),
        "preflight must not persist acceptance state when write authority is held by a live process"
    );
}
