# TODOS

## Plan Execution

### Enforce Plan Checklist State During Execution

**What:** Require execution workflows to check plan checklist items off as they are completed and treat stale unchecked steps as a workflow defect.

**Why:** Superpowers plans are written as executable checklists, but that value collapses when completed work is left visually indistinguishable from pending work. The plan should accurately communicate current state across sessions, reviews, and handoffs.

**Context:** The current execution flow often completes implementation, tests, and commits without updating the corresponding `- [ ]` steps in the approved plan. That makes it hard for the user, reviewers, and later agents to tell what is actually done versus what is still pending. A follow-up should update execution skills, review gates, and verification habits so checkbox state is part of the execution contract rather than optional hygiene.

**Tracking Spec:** `docs/superpowers/specs/2026-03-17-execution-workflow-clarity-design.md`

**Effort:** M
**Priority:** P1
**Depends on:** None

## Workflow Runtime

### Supported User-Facing Workflow CLI

**What:** Add a supported user-facing CLI for inspecting and navigating Superpowers workflow state on top of the internal workflow-status helper and manifest.

**Why:** The internal helper solves runtime routing first, but users will eventually need a stable, documented way to inspect workflow state directly without reading local manifest files or skill internals.

**Context:** The workflow-state runtime design keeps repo docs authoritative and introduces a branch-scoped local manifest under `~/.superpowers/projects/<repo-slug>/<user>-<safe-branch>-workflow-state.json`. This follow-up should wait until the internal contract is stable, then expose a clear public surface for status, expected next step, and artifact discovery.

**Effort:** M
**Priority:** P3
**Depends on:** Workflow-state runtime v1

## Completed

### Execution Handoff Recommendation Flow

Completed in the execution-workflow helper. `superpowers-plan-execution recommend --plan <approved-plan-path>` now derives `tasks_independent` from task `**Files:**` write scopes, combines that with the session-context inputs, and recommends either `superpowers:subagent-driven-development` or `superpowers:executing-plans` through the approved handoff flow.
