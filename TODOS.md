# TODOS

## Workflow Runtime

### Supported User-Facing Workflow CLI

**What:** Add a supported user-facing CLI for inspecting and navigating Superpowers workflow state on top of the internal workflow-status helper and manifest.

**Why:** The internal helper solves runtime routing first, but users will eventually need a stable, documented way to inspect workflow state directly without reading local manifest files or skill internals.

**Context:** The workflow-state runtime design keeps repo docs authoritative and introduces a local manifest under `~/.superpowers/projects/<repo-slug>/workflow-state.json`. This follow-up should wait until the internal contract is stable, then expose a clear public surface for status, expected next step, and artifact discovery.

**Effort:** M
**Priority:** P3
**Depends on:** Workflow-state runtime v1

## Completed
