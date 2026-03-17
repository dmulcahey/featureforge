# TODOS

## Workflow Runtime

### Execution Handoff Recommendation Flow

**What:** Add an analysis step to the execution handoff workflow that recommends `superpowers:subagent-driven-development` or `superpowers:executing-plans` based on the approved plan and current session constraints.

**Why:** The current handoff exposes both execution paths, but the user still has to infer which one fits the plan's task coupling, session context, and platform capabilities. Superpowers should make an opinionated recommendation instead of pushing that routing analysis back onto the user.

**Context:** The workflow-state runtime work sharpened the distinction between same-session isolated-agent execution and separate-session execution. A follow-up should inspect the approved plan, branch/session state, and task independence, then guide the user into the better option with a short explanation instead of leaving the handoff as a passive choice.

**Effort:** S
**Priority:** P2
**Depends on:** None

### Supported User-Facing Workflow CLI

**What:** Add a supported user-facing CLI for inspecting and navigating Superpowers workflow state on top of the internal workflow-status helper and manifest.

**Why:** The internal helper solves runtime routing first, but users will eventually need a stable, documented way to inspect workflow state directly without reading local manifest files or skill internals.

**Context:** The workflow-state runtime design keeps repo docs authoritative and introduces a branch-scoped local manifest under `~/.superpowers/projects/<repo-slug>/<user>-<safe-branch>-workflow-state.json`. This follow-up should wait until the internal contract is stable, then expose a clear public surface for status, expected next step, and artifact discovery.

**Effort:** M
**Priority:** P3
**Depends on:** Workflow-state runtime v1

## Completed
