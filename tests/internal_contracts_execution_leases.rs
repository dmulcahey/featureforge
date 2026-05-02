// Internal compatibility tests extracted from tests/contracts_execution_leases.rs.
// This file intentionally reuses the source fixture scaffolding from the public-facing integration test.

#[path = "support/files.rs"]
mod files_support;
#[path = "support/git.rs"]
mod git_support;
#[allow(dead_code)]
#[path = "support/plan_execution_direct.rs"]
mod plan_execution_direct_support;
#[path = "support/process.rs"]
mod process_support;

use assert_cmd::cargo::CommandCargoExt;
use featureforge::paths::branch_storage_key;
use files_support::write_file;
use process_support::run_checked;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-17-example-execution-plan.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-03-17-example-execution-plan-design.md";

fn init_repo(name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();

    git_support::init_repo_with_initial_commit(repo, &format!("# {name}\n"), "init");
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["checkout", "-B", "fixture-work"])
                .current_dir(repo);
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
**Goal:** The workspace is prepared for execution.

**Context:**
- Spec Coverage: REQ-001.

**Constraints:**
- Keep the fixture single-step and deterministic.

**Done when:**
- The workspace is prepared for execution.

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
        .join(concat!("execution-pre", "flight"))
        .join("acceptance-state.json")
}

fn internal_only_runtime_preflight_gate_json(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
) -> serde_json::Value {
    plan_execution_direct_support::internal_only_runtime_preflight_gate_json(
        repo,
        state,
        &featureforge::cli::plan_execution::StatusArgs {
            plan: plan_rel.into(),
            external_review_result_ready: false,
        },
    )
    .expect(concat!("internal pre", "flight helper should succeed"))
}

fn internal_only_unit_preflight_failure_json(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
) -> serde_json::Value {
    serde_json::from_str(
        &plan_execution_direct_support::internal_only_runtime_preflight_gate_json(
            repo,
            state,
            &featureforge::cli::plan_execution::StatusArgs {
                plan: plan_rel.into(),
                external_review_result_ready: false,
            },
        )
        .expect_err(concat!("internal pre", "flight helper should fail")),
    )
    .expect(concat!("internal pre", "flight failure should serialize"))
}

#[cfg(unix)]
struct DirectoryModeGuard {
    path: PathBuf,
    original_permissions: fs::Permissions,
}

#[cfg(unix)]
impl DirectoryModeGuard {
    fn new(path: impl Into<PathBuf>, mode: u32) -> Self {
        let path = path.into();
        let original_permissions = fs::metadata(&path)
            .expect("directory should exist")
            .permissions();
        let mut permissions = original_permissions.clone();
        permissions.set_mode(mode);
        fs::set_permissions(&path, permissions).expect("directory permissions should update");
        Self {
            path,
            original_permissions,
        }
    }
}

#[cfg(unix)]
impl Drop for DirectoryModeGuard {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.path, self.original_permissions.clone());
    }
}

#[test]
fn internal_only_compatibility_preflight_reclaims_stale_write_authority_lock_before_acceptance() {
    let (repo_dir, state_dir) = init_repo("contracts-execution-leases-reclaim");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "none");
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["checkout", "-B", concat!("execution-pre", "flight-fixture")])
                .current_dir(repo);
            command
        },
        concat!("git checkout execution-pre", "flight-fixture"),
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
        "{} acceptance state should not exist before stale-lock {}",
        concat!("pre", "flight"),
        concat!("pre", "flight")
    );

    let gate = internal_only_runtime_preflight_gate_json(repo, state, PLAN_REL);

    assert_eq!(
        gate["allowed"], true,
        "stale write-authority locks should be reclaimed"
    );
    assert!(
        !lock_path.exists(),
        "stale write-authority lock should be removed after reclamation"
    );
    assert!(
        acceptance_path.exists(),
        "{} should persist acceptance state after reclaiming stale write authority",
        concat!("pre", "flight")
    );
}

#[test]
fn internal_only_compatibility_preflight_blocks_live_write_authority_conflict_without_persisting_acceptance()
 {
    let (repo_dir, state_dir) = init_repo("contracts-execution-leases-conflict");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "none");
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["checkout", "-B", concat!("execution-pre", "flight-fixture")])
                .current_dir(repo);
            command
        },
        concat!("git checkout execution-pre", "flight-fixture"),
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
        "{} acceptance state should not exist before live-lock {}",
        concat!("pre", "flight"),
        concat!("pre", "flight")
    );

    let gate = internal_only_runtime_preflight_gate_json(repo, state, PLAN_REL);
    let _ = holder.kill();
    let _ = holder.wait();

    assert_eq!(gate["allowed"], false);
    assert!(
        gate["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "write_authority_conflict")),
        "{} should report the write-authority conflict reason code",
        concat!("pre", "flight")
    );
    assert!(
        !acceptance_path.exists(),
        "{} must not persist acceptance state when write authority is held by a live process",
        concat!("pre", "flight")
    );
}

#[cfg(unix)]
#[test]
fn internal_only_compatibility_preflight_fails_closed_when_write_authority_lock_is_unreadable() {
    let (repo_dir, state_dir) = init_repo("contracts-execution-leases-lock-unreadable");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "none");
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["checkout", "-B", concat!("execution-pre", "flight-fixture")])
                .current_dir(repo);
            command
        },
        concat!("git checkout execution-pre", "flight-fixture"),
    );

    let harness_dir = harness_branch_dir(repo, state);
    let lock_path = harness_dir.join("write-authority.lock");
    write_file(&lock_path, "pid=12345\n");
    let _guard = DirectoryModeGuard::new(&harness_dir, 0o000);

    let failure = internal_only_unit_preflight_failure_json(repo, state, PLAN_REL);
    assert_eq!(failure["error_class"], "MalformedExecutionState");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(authoritative_state_inspection_failure_visible),
        "{} should surface authoritative-state inspection failure when hidden-gate migration cannot inspect state: {:?}",
        failure,
        concat!("pre", "flight")
    );
}

fn authoritative_state_inspection_failure_visible(message: &str) -> bool {
    message.contains("Could not inspect authoritative harness state")
        || message.contains("Could not inspect Authoritative event log")
}

#[cfg(unix)]
#[test]
fn internal_only_compatibility_preflight_fails_closed_when_write_authority_lock_is_dangling_symlink()
 {
    let (repo_dir, state_dir) = init_repo("contracts-execution-leases-lock-symlink");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "none");
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["checkout", "-B", concat!("execution-pre", "flight-fixture")])
                .current_dir(repo);
            command
        },
        concat!("git checkout execution-pre", "flight-fixture"),
    );

    let harness_dir = harness_branch_dir(repo, state).join("execution-harness");
    fs::create_dir_all(&harness_dir).expect("harness directory should be creatable");
    let lock_path = harness_dir.join("write-authority.lock");
    symlink("missing-lock-target.pid", &lock_path)
        .expect("dangling write-authority symlink should be creatable");

    let gate = internal_only_runtime_preflight_gate_json(repo, state, PLAN_REL);

    assert_eq!(gate["allowed"], false);
    assert!(
        gate["reason_codes"].as_array().is_some_and(|codes| codes
            .iter()
            .any(|code| code == "write_authority_unavailable")),
        "dangling write-authority symlink should fail closed instead of clearing the {}",
        concat!("pre", "flight")
    );
}

#[cfg(unix)]
#[test]
fn internal_only_compatibility_preflight_fails_closed_when_authoritative_state_is_dangling_symlink()
{
    let (repo_dir, state_dir) =
        init_repo(concat!("contracts-execution-leases-pre", "flight-symlink"));
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_approved_spec(repo);
    write_single_step_plan(repo, "none");
    run_checked(
        {
            let mut command = Command::new("git");
            command
                .args(["checkout", "-B", concat!("execution-pre", "flight-fixture")])
                .current_dir(repo);
            command
        },
        concat!("git checkout execution-pre", "flight-fixture"),
    );

    let harness_dir = harness_branch_dir(repo, state).join("execution-harness");
    fs::create_dir_all(&harness_dir).expect("harness directory should be creatable");
    let state_path = harness_dir.join("state.json");
    symlink("missing-state-target.json", &state_path)
        .expect("dangling authoritative state symlink should be creatable");

    let failure = internal_only_unit_preflight_failure_json(repo, state, PLAN_REL);
    assert_eq!(failure["error_class"], "MalformedExecutionState");
    assert!(
        failure["message"].as_str().is_some_and(|message| {
            message.contains("Authoritative harness state path must not be a symlink")
        }),
        "{} should surface authoritative-state symlink failure when hidden-gate migration cannot load state: {:?}",
        failure,
        concat!("pre", "flight")
    );
}
