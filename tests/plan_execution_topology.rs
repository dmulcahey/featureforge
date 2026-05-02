#[path = "support/files.rs"]
mod files_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/repo_template.rs"]
mod repo_template_support;

use featureforge::contracts::plan::{AnalyzePlanReport, analyze_plan};
use featureforge::execution::harness::{LearnedTopologyGuidance, TopologySelectionContext};
use featureforge::execution::internal_args::ExecutionTopologyArg;
use featureforge::execution::topology::recommend_topology;
use files_support::write_file;
use process_support::run_checked;
use repo_template_support::populate_repo_from_template;
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const PLAN_REL: &str = "docs/featureforge/plans/2026-03-17-example-execution-plan.md";
const SPEC_REL: &str = "docs/featureforge/specs/2026-03-17-example-execution-plan-design.md";

fn init_repo(name: &str) -> (TempDir, TempDir) {
    let repo_dir = TempDir::new().expect("repo tempdir should exist");
    let state_dir = TempDir::new().expect("state tempdir should exist");
    let repo = repo_dir.path();

    let _ = name;
    populate_repo_from_template(repo);
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
- [REQ-002][behavior] Execution fixtures must support a valid multi-task independent-plan path for topology and preflight coverage.

## Summary

Fixture spec for focused execution-helper regression coverage.
"#,
    );
}

fn write_independent_plan(repo: &Path) {
    write_file(
        &repo.join(PLAN_REL),
        &format!(
            r#"# Example Execution Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `{SPEC_REL}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-002 -> Task 1
- REQ-002 -> Task 2

## Execution Strategy

- After the initial setup, create two worktrees and run Tasks 1 and 2 in parallel:
  - Task 1 owns `docs/task-a.md`.
  - Task 2 owns `docs/task-b.md`.

## Dependency Diagram

```text
Task 1    Task 2
```

## Task 1: Independent Task A

**Spec Coverage:** REQ-001, REQ-002
**Goal:** Task A is isolated.

**Context:**
- Spec Coverage: REQ-001, REQ-002.

**Constraints:**
- Keep Task 1 independent.

**Done when:**
- Task A is isolated.

**Files:**
- Modify: `docs/task-a.md`

- [ ] **Step 1: Complete Task A**

## Task 2: Independent Task B

**Spec Coverage:** REQ-002
**Goal:** Task B is isolated.

**Context:**
- Spec Coverage: REQ-002.

**Constraints:**
- Keep Task 2 independent.

**Done when:**
- Task B is isolated.

**Files:**
- Modify: `docs/task-b.md`

- [ ] **Step 1: Complete Task B**
"#
        ),
    );
}

fn topology_report(repo: &Path) -> AnalyzePlanReport {
    analyze_plan(repo.join(SPEC_REL), repo.join(PLAN_REL)).expect("plan analysis should succeed")
}

fn topology_context(
    execution_context_key: &str,
    tasks_independent: bool,
    isolated_agents_available: &str,
    session_intent: &str,
    workspace_prepared: &str,
    current_parallel_path_ready: bool,
    learned_guidance: Option<LearnedTopologyGuidance>,
) -> TopologySelectionContext {
    TopologySelectionContext {
        execution_context_key: execution_context_key.to_owned(),
        tasks_independent,
        isolated_agents_available: isolated_agents_available.to_owned(),
        session_intent: session_intent.to_owned(),
        workspace_prepared: workspace_prepared.to_owned(),
        current_parallel_path_ready,
        learned_guidance,
    }
}

#[test]
fn runtime_topology_falls_back_conservatively_when_worktrees_or_agents_are_not_ready() {
    let (repo_dir, _state_dir) = init_repo("plan-execution-conservative-fallback");
    let repo = repo_dir.path();
    write_approved_spec(repo);
    write_independent_plan(repo);

    let report = topology_report(repo);
    let recommendation = recommend_topology(
        &report,
        &topology_context(
            "main@base-a",
            true,
            "unavailable",
            "stay",
            "no",
            false,
            None,
        ),
    );

    assert_eq!(
        recommendation.selected_topology,
        ExecutionTopologyArg::ConservativeFallback
    );
    assert_eq!(
        recommendation.recommended_skill,
        "featureforge:executing-plans"
    );
    assert!(
        recommendation
            .reason
            .to_lowercase()
            .contains("conservative"),
        "reason should explain the conservative fallback"
    );
    assert!(
        recommendation
            .reason_codes
            .iter()
            .any(|code: &String| code == "conservative_fallback_policy_safety_block"),
        "fallback topology should expose the actual blocker reason code"
    );
    assert!(
        !recommendation
            .reason_codes
            .iter()
            .any(|code: &String| code == "conservative_fallback_same_session_unavailable"),
        "fallback diagnostics should not blame same-session viability when it is not the actual blocker"
    );
    assert_eq!(
        recommendation.decision_flags.tasks_independent, "yes",
        "topology fallback should not redefine actual task independence"
    );
}

#[test]
fn runtime_topology_separate_session_fallback_uses_actual_blocker_reason_codes() {
    let (repo_dir, _state_dir) = init_repo("plan-execution-separate-session-fallback");
    let repo = repo_dir.path();
    write_approved_spec(repo);
    write_independent_plan(repo);

    let report = topology_report(repo);
    let recommendation = recommend_topology(
        &report,
        &topology_context(
            "main@base-a",
            true,
            "unavailable",
            "separate",
            "yes",
            false,
            None,
        ),
    );

    assert_eq!(
        recommendation.selected_topology,
        ExecutionTopologyArg::ConservativeFallback
    );
    assert_eq!(
        recommendation.recommended_skill,
        "featureforge:executing-plans"
    );
    assert!(
        recommendation
            .reason_codes
            .iter()
            .any(|code| code == "conservative_fallback_policy_safety_block"),
        "separate-session fallback should name the actual blocker"
    );
    assert!(
        !recommendation
            .reason_codes
            .iter()
            .any(|code| code == "conservative_fallback_same_session_unavailable"),
        "separate-session fallback must not claim same-session unavailability"
    );
    assert_eq!(
        recommendation.decision_flags.session_intent, "separate",
        "session intent should still be surfaced verbatim"
    );
}

#[test]
fn runtime_topology_reuses_matching_downgrade_history_for_same_context() {
    let (repo_dir, _state_dir) = init_repo("plan-execution-downgrade-reuse");
    let repo = repo_dir.path();
    write_approved_spec(repo);
    write_independent_plan(repo);

    let report = topology_report(repo);
    let learned_guidance = LearnedTopologyGuidance {
        approved_plan_revision: report.plan_revision,
        execution_context_key: String::from("main@base-a"),
        primary_reason_class: String::from("workspace_unavailable"),
    };
    let recommendation = recommend_topology(
        &report,
        &topology_context(
            "main@base-a",
            true,
            "available",
            "stay",
            "no",
            false,
            Some(learned_guidance),
        ),
    );

    assert_eq!(
        recommendation.selected_topology,
        ExecutionTopologyArg::ConservativeFallback
    );
    assert_eq!(
        recommendation.recommended_skill,
        "featureforge:executing-plans"
    );
    assert!(
        recommendation.learned_downgrade_reused,
        "matching downgrade history should be reused as conservative guidance"
    );
    assert!(
        recommendation
            .reason_codes
            .iter()
            .any(|code| code == "matching_downgrade_history_reused"),
        "matching downgrade history should be visible in the runtime reason codes"
    );
}

#[test]
fn runtime_topology_supersedes_downgrade_history_when_the_blocker_clears() {
    let (repo_dir, _state_dir) = init_repo("plan-execution-downgrade-recovery");
    let repo = repo_dir.path();
    write_approved_spec(repo);
    write_independent_plan(repo);

    let report = topology_report(repo);
    let learned_guidance = LearnedTopologyGuidance {
        approved_plan_revision: report.plan_revision,
        execution_context_key: String::from("main@base-a"),
        primary_reason_class: String::from("workspace_unavailable"),
    };
    let recommendation = recommend_topology(
        &report,
        &topology_context(
            "main@base-a",
            true,
            "available",
            "stay",
            "yes",
            true,
            Some(learned_guidance),
        ),
    );

    assert_eq!(
        recommendation.selected_topology,
        ExecutionTopologyArg::WorktreeBackedParallel
    );
    assert_eq!(
        recommendation.recommended_skill,
        "featureforge:subagent-driven-development"
    );
    assert!(
        !recommendation.learned_downgrade_reused,
        "restored runs should supersede old conservative guidance rather than reuse it"
    );
    assert!(
        recommendation
            .reason_codes
            .iter()
            .any(|code| code == "matching_downgrade_history_superseded"),
        "recovery should explicitly supersede the learned downgrade history"
    );
}

#[test]
fn runtime_topology_serializes_selected_topology_with_contract_values() {
    let serialized = serde_json::to_value(ExecutionTopologyArg::WorktreeBackedParallel)
        .expect("topology enum should serialize");
    assert_eq!(
        serialized,
        Value::String(String::from("worktree-backed-parallel"))
    );

    let round_tripped: ExecutionTopologyArg =
        serde_json::from_value(Value::String(String::from("conservative-fallback")))
            .expect("topology enum should deserialize from contract value");
    assert_eq!(round_tripped, ExecutionTopologyArg::ConservativeFallback);

    let (repo_dir, _state_dir) = init_repo("plan-execution-topology-json-contract");
    let repo = repo_dir.path();
    write_approved_spec(repo);
    write_independent_plan(repo);

    let report = topology_report(repo);
    let recommendation = recommend_topology(
        &report,
        &topology_context("main@base-a", true, "available", "stay", "yes", true, None),
    );
    let json = serde_json::to_value(&recommendation).expect("recommendation should serialize");
    assert_eq!(
        json["selected_topology"],
        Value::String(String::from("worktree-backed-parallel"))
    );
}

#[test]
fn runtime_topology_can_select_worktree_backed_parallel_for_separate_session_coordinators() {
    let (repo_dir, _state_dir) = init_repo("plan-execution-separate-session-parallel");
    let repo = repo_dir.path();
    write_approved_spec(repo);
    write_independent_plan(repo);

    let report = topology_report(repo);
    let recommendation = recommend_topology(
        &report,
        &topology_context(
            "main@base-a",
            true,
            "available",
            "separate",
            "yes",
            true,
            None,
        ),
    );

    assert_eq!(
        recommendation.selected_topology,
        ExecutionTopologyArg::WorktreeBackedParallel
    );
    assert_eq!(
        recommendation.recommended_skill, "featureforge:executing-plans",
        "a separate-session coordinator should still drive the worktree-backed parallel topology"
    );
    assert_eq!(
        recommendation.decision_flags.same_session_viable, "no",
        "the same-session flag should remain about session intent, not topology eligibility"
    );
    assert_eq!(recommendation.decision_flags.tasks_independent, "yes");
}
