// Internal compatibility tests extracted from tests/cli_parse_boundary.rs.
// This file intentionally reuses the source fixture scaffolding from the public-facing integration test.

#[path = "support/failure_json.rs"]
mod failure_json_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/git.rs"]
mod git_support;
#[path = "support/process.rs"]
mod process_support;

use assert_cmd::cargo::CommandCargoExt;
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

use failure_json_support::parse_failure_json;
use files_support::write_file;
use process_support::run;

const SPEC_REL: &str = "docs/featureforge/specs/2026-03-25-cli-parse-boundary-design.md";
const PLAN_REL: &str = "docs/featureforge/plans/2026-03-25-cli-parse-boundary.md";

fn init_repo(name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();

    git_support::init_repo_with_initial_commit(repo, &format!("# {name}\n"), "init");

    write_file(
        &repo.join(SPEC_REL),
        r#"# CLI Parse Boundary Design

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

## Summary

Fixture spec for CLI parse-boundary coverage.

## Requirement Index

- [REQ-001][behavior] Bounded CLI values must fail at the clap boundary.
"#,
    );
    write_file(
        &repo.join(PLAN_REL),
        &format!(
            r#"# CLI Parse Boundary Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `{SPEC_REL}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1

## Task 1: Parse boundary

**Spec Coverage:** REQ-001
**Goal:** Typed parse-boundary coverage stays explicit.

**Context:**
- Spec Coverage: REQ-001.

**Constraints:**
- Keep parse-boundary failures at the CLI layer.

**Done when:**
- Typed parse-boundary coverage stays explicit.

**Files:**
- Modify: `tests/cli_parse_boundary.rs`
- Test: `cargo nextest run --test cli_parse_boundary`

- [ ] **Step 1: Add red parse-boundary tests**
"#
        ),
    );

    (repo_dir, state_dir)
}

fn run_featureforge(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    run_featureforge_with_env(repo, state_dir, args, context, &[])
}

fn run_featureforge_with_env(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
    extra_env: &[(&str, &str)],
) -> Output {
    let mut command =
        Command::cargo_bin("featureforge").expect("featureforge cargo binary should exist");
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    run(command, context)
}

#[test]
fn internal_only_compatibility_close_current_task_rejects_hidden_dispatch_id_without_internal_flag_env()
 {
    let (repo_dir, state_dir) = init_repo("cli-boundary-close-current-task-hidden-dispatch-id");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let review_summary = repo.join("task-review.md");
    let verification_summary = repo.join("task-verification.md");
    write_file(&review_summary, "Task review passed.\n");
    write_file(&verification_summary, "Verification passed.\n");

    let output = run_featureforge(
        repo,
        state,
        &[
            "plan",
            "execution",
            "close-current-task",
            "--plan",
            PLAN_REL,
            "--task",
            "1",
            concat!("--dispatch", "-id"),
            "dispatch-001",
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
        "close-current-task hidden dispatch-id guard",
    );
    let json = parse_failure_json(&output, "close-current-task hidden dispatch-id guard");

    assert_eq!(json["error_class"], "InvalidCommandInput");
    assert_eq!(
        json["message"],
        Value::from(format!(
            "{} is an internal compatibility flag and is not available in normal public execution. Run close-current-task without it.",
            concat!("--dispatch", "-id")
        ))
    );
}

#[test]
fn internal_only_compatibility_advance_late_stage_rejects_hidden_lineage_flags_without_internal_flag_env()
 {
    let cases = [
        (
            concat!("--dispatch", "-id"),
            "dispatch-001",
            concat!(
                "--dispatch",
                "-id is an internal compatibility flag and is not available in normal public execution. Run advance-late-stage without it."
            ),
        ),
        (
            concat!("--branch", "-closure-id"),
            "branch-closure-001",
            concat!(
                "--branch",
                "-closure-id is an internal compatibility flag and is not available in normal public execution. Run advance-late-stage without it."
            ),
        ),
    ];

    for (flag, value, expected_message) in cases {
        let (repo_dir, state_dir) = init_repo(&format!(
            "cli-boundary-advance-late-stage-hidden-{}",
            flag.trim_start_matches("--")
        ));
        let repo = repo_dir.path();
        let state = state_dir.path();

        let output = run_featureforge(
            repo,
            state,
            &[
                "plan",
                "execution",
                "advance-late-stage",
                "--plan",
                PLAN_REL,
                flag,
                value,
            ],
            "advance-late-stage hidden lineage flag guard",
        );
        let json = parse_failure_json(&output, "advance-late-stage hidden lineage flag guard");

        assert_eq!(json["error_class"], "InvalidCommandInput");
        assert_eq!(json["message"], expected_message);
    }
}

#[test]
fn internal_only_compatibility_internal_execution_flag_env_allows_hidden_lineage_flags_to_reach_normal_validation()
 {
    let (repo_dir, state_dir) = init_repo("cli-boundary-internal-execution-flag-env");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let output = run_featureforge_with_env(
        repo,
        state,
        &[
            "plan",
            "execution",
            "advance-late-stage",
            "--plan",
            PLAN_REL,
            concat!("--dispatch", "-id"),
            "dispatch-001",
        ],
        "advance-late-stage hidden flag with internal env",
        &[(
            concat!("FEATUREFORGE", "_ALLOW_INTERNAL_EXECUTION_FLAGS"),
            "1",
        )],
    );
    let json = parse_failure_json(&output, "advance-late-stage hidden flag with internal env");
    let message = json["message"]
        .as_str()
        .expect("failure message should stay a string");

    assert!(
        !message.contains("internal compatibility flag"),
        "internal debug env should allow hidden flags to reach normal validation, got {json}"
    );
}

#[test]
fn internal_only_compatibility_plan_execution_record_review_dispatch_requires_scope_at_parse_boundary()
 {
    let (repo_dir, state_dir) = init_repo("cli-boundary-review-dispatch-scope");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let output = run_featureforge(
        repo,
        state,
        &[
            "plan",
            "execution",
            concat!("record", "-review-dispatch"),
            "--plan",
            PLAN_REL,
        ],
        concat!("plan execution record", "-review-dispatch missing scope"),
    );
    let json = parse_failure_json(
        &output,
        concat!("plan execution record", "-review-dispatch missing scope"),
    );

    assert_eq!(
        json["error_class"],
        Value::String(String::from("InvalidCommandInput"))
    );
    let message = json["message"]
        .as_str()
        .expect("failure message should stay a string");
    assert!(message.contains(&format!(
        "unrecognized subcommand '{}'",
        concat!("record", "-review-dispatch")
    )));
}

#[test]
fn internal_only_compatibility_plan_execution_record_release_readiness_requires_primitive_arguments_at_parse_boundary()
 {
    let (repo_dir, state_dir) = init_repo(concat!("cli-boundary-record", "-release-readiness"));
    let repo = repo_dir.path();
    let state = state_dir.path();

    let output = run_featureforge(
        repo,
        state,
        &[
            "plan",
            "execution",
            concat!("record", "-release-readiness"),
            "--plan",
            PLAN_REL,
        ],
        concat!(
            "plan execution record",
            "-release-readiness missing primitive arguments"
        ),
    );
    let json = parse_failure_json(
        &output,
        concat!(
            "plan execution record",
            "-release-readiness missing primitive arguments"
        ),
    );

    assert_eq!(
        json["error_class"],
        Value::String(String::from("InvalidCommandInput"))
    );
    let message = json["message"]
        .as_str()
        .expect("failure message should stay a string");
    assert!(message.contains(&format!(
        "unrecognized subcommand '{}'",
        concat!("record", "-release-readiness")
    )));
}

#[test]
fn internal_only_compatibility_plan_execution_record_final_review_requires_primitive_arguments_at_parse_boundary()
 {
    let (repo_dir, state_dir) = init_repo(concat!("cli-boundary-record", "-final-review"));
    let repo = repo_dir.path();
    let state = state_dir.path();

    let output = run_featureforge(
        repo,
        state,
        &[
            "plan",
            "execution",
            concat!("record", "-final-review"),
            "--plan",
            PLAN_REL,
        ],
        concat!(
            "plan execution record",
            "-final-review missing primitive arguments"
        ),
    );
    let json = parse_failure_json(
        &output,
        concat!(
            "plan execution record",
            "-final-review missing primitive arguments"
        ),
    );

    assert_eq!(
        json["error_class"],
        Value::String(String::from("InvalidCommandInput"))
    );
    let message = json["message"]
        .as_str()
        .expect("failure message should stay a string");
    assert!(message.contains(&format!(
        "unrecognized subcommand '{}'",
        concat!("record", "-final-review")
    )));
}

#[test]
fn internal_only_compatibility_workflow_hidden_compatibility_commands_are_removed_from_active_cli_surface()
 {
    let (repo_dir, state_dir) = init_repo("cli-boundary-workflow-hidden-commands");
    let repo = repo_dir.path();
    let state = state_dir.path();

    for command in [
        ["workflow", "resolve"].as_slice(),
        [
            "workflow",
            "expect",
            "--artifact",
            "spec",
            "--path",
            SPEC_REL,
        ]
        .as_slice(),
        ["workflow", "sync", "--artifact", "spec"].as_slice(),
        ["workflow", "next"].as_slice(),
        ["workflow", "artifacts"].as_slice(),
        ["workflow", "explain"].as_slice(),
        ["workflow", "phase", "--json"].as_slice(),
        ["workflow", "doctor", "--json"].as_slice(),
        ["workflow", "handoff", "--json"].as_slice(),
    ] {
        let output = run_featureforge(
            repo,
            state,
            command,
            "workflow hidden compatibility command parse boundary",
        );
        let json = parse_failure_json(&output, "workflow hidden compatibility command");

        assert_eq!(
            json["error_class"],
            Value::String(String::from("InvalidCommandInput"))
        );
        let message = json["message"]
            .as_str()
            .expect("failure message should stay a string");
        assert!(message.contains("unrecognized subcommand"));
    }
}
