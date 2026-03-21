# Bootstrap And Branch-Safety Hardening

**Workflow State:** Draft
**Spec Revision:** 1
**Last Reviewed By:** brainstorming

## Summary

Harden the two Superpowers guarantees that are currently still soft at the exact boundary where users expect them to be strict:

1. a fresh session must resolve the `using-superpowers` bypass/opt-in gate before any normal Superpowers behavior starts
2. repo-writing workflow stages must fail closed on protected branches unless the write is clearly safe or the user has explicitly approved that protected-branch risk for the current task

The approved direction is runtime-owned rather than skill-prose-only:

- add a narrow `superpowers-session-entry` helper that owns first-turn bootstrap resolution
- add a narrow `superpowers-repo-safety` helper that owns protected-branch repo-write authorization
- keep `superpowers-workflow-status` focused on workflow-state routing
- keep `superpowers-plan-execution` focused on approved-plan execution truth
- update generated docs, workflow skills, and regression gates so the new guarantees are explicit and durable

This is intentionally not a broad workflow rewrite. It is a targeted hardening pass at the two places where the current workflow can still over-promise conservative behavior.

## Problem

Recent workflow changes materially improved the helper-backed workflow state machine after bootstrap:

- `using-superpowers` gained a dedicated session bypass contract
- `superpowers-workflow-status` became stricter about workflow-state routing
- `superpowers-plan-execution` became stricter about stale source-spec linkage

Those changes are good, but they do not fully solve the failures that motivated this item.

Today the real gaps are:

- the first-turn bypass question still depends on the outer harness actually entering `using-superpowers` correctly
- the current deterministic bypass tests validate wording and shell-derived decision paths, but they do not prove that a fresh real session emits the bypass question before normal behavior
- protected-branch safety is still mostly instructional and skill-driven
- there is no single runtime-owned preflight that says "this workflow stage is about to mutate repo state; block it on protected branches unless the write is explicitly authorized"

As a result, Superpowers can still drift at two critical trust boundaries:

- before the workflow runtime is fully in control
- while writing repo state on `main` or another protected branch

## Goals

- Define one runtime-owned bootstrap invariant for first-turn session entry.
- Fail closed when session-entry state is missing or malformed.
- Preserve session-scoped bypass and explicit same-session re-entry.
- Add one shared runtime-owned preflight for protected-branch repo writes.
- Block repo-writing workflow stages on protected branches by default.
- Require an explicit, narrow, auditable escape hatch for protected-branch writes.
- Keep read-only inspection flows and local runtime-state writes out of the protected-branch gate.
- Preserve the current helper authority boundaries for workflow state and execution state.
- Add regression coverage that would have caught both failures seen in this session.

## Not In Scope

- Replacing `superpowers-workflow-status` with a broader workflow engine.
- Making `superpowers-plan-execution` responsible for bootstrap or branch-safety ownership.
- Auto-creating worktrees, auto-switching branches, or auto-approving risky writes.
- A global or session-wide protected-branch bypass.
- A new standalone config file just for protected-branch names.
- A public inspection CLI for historical branch-safety approvals beyond the local state files written by the helper.
- Broad policy for arbitrary branch-pattern parsing in v1.

## Approved Product Decisions

The approved design assumptions from brainstorming are:

- first-turn enforcement must be runtime-owned and harness-facing, not generator-only
- protected branches in v1 default to `main`, `master`, `dev`, and `develop`
- branch protection may be extended through existing repo/user instruction files rather than a new config surface
- the protected-branch escape hatch is task-scoped and persisted as a local runtime record
- all repo-writing stages are guarded, including spec and plan doc writes, approval-header writes, implementation-time repo edits, release-doc updates, and branch-finishing commands
- the design should use two narrow helpers rather than overloading existing helpers

## Bootstrap Invariant

Before any normal Superpowers behavior happens, session entry must resolve to exactly one of these outcomes:

- `enabled`
- `bypassed`
- explicit re-entry for the current turn
- an explicit user-choice prompt asking whether to enable or bypass Superpowers for the session

Any path that enters the normal `using-superpowers` stack without first resolving that invariant is a contract violation.

Missing or malformed decision state must not silently fall through to normal behavior. It must fail closed to the user-choice prompt.

## Architecture Boundary

This design adds two new runtime-owned helpers and keeps the existing helper boundaries intact.

### Helper ownership

`superpowers-session-entry`
- owns first-turn session bootstrap resolution
- owns decision-file interpretation for `enabled`, `bypassed`, missing, malformed, and explicit re-entry cases
- owns the harness-facing contract for when the bypass question must appear

`superpowers-repo-safety`
- owns protected-branch repo-write authorization
- owns task-scoped override records for explicit protected-branch approval
- owns the blocking and explanation contract for repo-writing stages

### Preserved boundaries

`superpowers-workflow-status`
- still owns workflow-state routing only
- must not become the owner of bootstrap-entry guarantees
- must not become the owner of protected-branch authorization

`superpowers-plan-execution`
- still owns approved-plan execution truth only
- must not become the owner of repo-write authorization beyond its existing execution-state contract

### Runtime surface parity

Like the existing runtime helpers, each new helper should ship with Bash and PowerShell entrypoints:

- `bin/superpowers-session-entry`
- `bin/superpowers-session-entry.ps1`
- `bin/superpowers-repo-safety`
- `bin/superpowers-repo-safety.ps1`

## Proposed Runtime Contracts

### 1. `superpowers-session-entry`

This helper becomes the runtime authority for first-turn bootstrap.

#### Commands

```text
superpowers-session-entry resolve --message-file <path> [--session-key <stable-id>]
superpowers-session-entry record --decision enabled|bypassed [--session-key <stable-id>]
```

`resolve`
- reads the current message text
- derives or accepts a stable session key
- evaluates any existing decision file
- detects explicit re-entry requests
- returns the next required bootstrap outcome as JSON

`record`
- persists the explicit user choice after the bypass/opt-in question is answered
- writes only `enabled` or `bypassed`
- returns JSON describing the persisted decision state

#### Session identity

The helper should support this precedence:

1. `--session-key <stable-id>`
2. `SUPERPOWERS_SESSION_KEY`
3. fallback to `$PPID`

Rationale:

- the harness-facing contract needs a stable session identifier when possible
- fallback to `$PPID` preserves compatibility with the current generated-shell behavior
- the helper can be adopted incrementally without making old callers unusable

#### State path

The decision file remains session-scoped under local runtime state:

```text
~/.superpowers/session-flags/using-superpowers/<session-key>
```

Valid persisted values remain:

- `enabled`
- `bypassed`

Any other file content is malformed state, not a third mode.

#### `resolve` output

`resolve` should emit JSON shaped like:

```json
{
  "outcome": "enabled|bypassed|needs_user_choice|runtime_failure",
  "decision_source": "existing_enabled|existing_bypassed|missing|malformed|explicit_reentry|explicit_reentry_unpersisted",
  "session_key": "...",
  "decision_path": "...",
  "persisted": true,
  "reason": "..."
}
```

When `outcome` is `needs_user_choice`, the JSON should also include a structured prompt payload for the first-turn question so the harness-facing entrypoint can emit that exact question before any normal Superpowers behavior.

#### Required semantics

- existing `enabled` decision: return `enabled`
- existing `bypassed` decision with no explicit re-entry request: return `bypassed`
- missing decision: return `needs_user_choice`
- malformed decision content: return `needs_user_choice`
- explicit re-entry request while bypassed:
  - try to persist `enabled`
  - if the write succeeds, return `enabled` with `decision_source=explicit_reentry`
  - if the write fails, return `enabled` with `decision_source=explicit_reentry_unpersisted` and `persisted=false`

This preserves the approved behavior that explicit re-entry should work on the current turn even when persistence fails, while future turns remain undecided.

#### Explicit re-entry matching

Re-entry must stay explicit, not heuristic. The helper should match clear signals such as:

- `use superpowers`
- `superpowers:<skill-name>`
- exact installed Superpowers skill names when directly invoked by the user

Generic phrases or accidental keyword overlap must not silently re-enable the stack.

### 2. `superpowers-repo-safety`

This helper becomes the runtime authority for protected-branch repo-write authorization.

#### Commands

```text
superpowers-repo-safety check --intent write|read --stage <skill-id> --task-id <stable-task-id> [--path <repo-rel-path>]... [--write-target <target>]...
superpowers-repo-safety approve --stage <skill-id> --task-id <stable-task-id> --reason <explicit-user-approved-text> [--path <repo-rel-path>]... [--write-target <target>]...
```

`check`
- decides whether the requested operation is allowed for the current branch and task scope
- returns JSON with the authorization result and any blocking reason

`approve`
- persists an explicit protected-branch approval record for the current task and write scope
- returns JSON with the approval path and normalized scope

#### Default protected branches

The helper must treat these branch names as protected by default:

- `main`
- `master`
- `dev`
- `develop`

#### Optional extension through existing instruction files

V1 may extend the protected list through existing repo/user instruction files, not a new config file.

The directive should be intentionally narrow:

- exact branch-name additions only
- no globbing in v1
- the default protected set always applies even if no directive exists

For example, instruction files may include a line like:

```text
Superpowers protected branches: release, production-hotfix
```

If no directive exists, the default list remains authoritative.

#### Scope model

Protected-branch approvals must be narrow enough to be auditable and non-sticky.

Each approval record must bind to:

- repo root
- branch
- stage
- task id
- approved repo-path scope, when file paths are relevant
- approved symbolic write targets, when file paths alone are insufficient
- approval reason text
- timestamp

Symbolic write targets are required for git mutations that are not captured by file paths alone, for example:

- `git-commit`
- `git-merge`
- `git-push`
- `git-worktree-cleanup`

#### Approval record path

Approval records should live only in local runtime state, for example:

```text
~/.superpowers/projects/<slug>/<user>-<safe-branch>-repo-safety/<task-hash>.json
```

These records are local audit artifacts, not repo truth.

#### `check` output

`check` should emit JSON shaped like:

```json
{
  "outcome": "allowed|blocked|runtime_failure",
  "intent": "write",
  "branch": "...",
  "protected": true,
  "protected_by": "default|instructions",
  "approval_path": "...",
  "reason": "...",
  "suggested_next_skill": "superpowers:using-git-worktrees"
}
```

#### Required semantics

- `--intent read` must return `allowed`, even on protected branches
- non-protected branches must allow repo writes without needing a protected-branch override
- protected branches must block repo writes unless a matching task-scoped approval exists
- a dedicated worktree does not exempt a protected branch by itself
- a feature branch inside a worktree is safe because it is a non-protected branch, not because it is a worktree
- stale or mismatched approvals must not be reused
- the helper must not auto-create a worktree or auto-switch branches

## Decision Flows

### Session-entry flow

```text
incoming user turn
    |
    v
harness-facing Superpowers entrypoint
    |
    v
superpowers-session-entry resolve
    |
    +--> outcome=needs_user_choice
    |       |
    |       +--> emit bypass/opt-in question only
    |
    +--> outcome=bypassed
    |       |
    |       +--> bypass normal Superpowers stack
    |
    +--> outcome=enabled
            |
            +--> enter normal using-superpowers stack
```

### Repo-safety flow

```text
workflow stage wants to mutate repo state
    |
    v
superpowers-repo-safety check --intent write ...
    |
    +--> outcome=allowed
    |       |
    |       +--> perform repo write
    |
    +--> outcome=blocked
            |
            +--> explain protected-branch block
            +--> route to feature branch/worktree or explicit approval
```

## Integration Plan

### Harness-facing session entry

The supported Superpowers entrypoint must call `superpowers-session-entry resolve` before loading the normal `using-superpowers` stack.

Required behavior:

- if `resolve` returns `needs_user_choice`, the first assistant output must be the bypass/opt-in question payload
- if `resolve` returns `bypassed`, the entrypoint must stop before normal Superpowers routing
- if `resolve` returns `enabled`, the entrypoint may continue into `using-superpowers`
- if `resolve` returns `runtime_failure`, the contract must fail closed and surface a visible runtime failure rather than silently proceeding

This is the key change that moves the guarantee from "the skill said this should happen" to "the runtime entry contract required it."

### Generated `using-superpowers` contract

Update the generated `using-superpowers` sections so they no longer imply that generator prose alone enforces the bootstrap boundary.

The generated contract should say:

- session-entry bootstrap ownership is runtime-owned through `superpowers-session-entry`
- the bypass gate must be resolved before the normal stack starts
- missing or malformed decision state fails closed to the opt-in question
- explicit re-entry remains supported on the same turn

The generated docs should remain the human-readable policy, but not pretend to be the sole enforcement layer.

### Repo-writing workflow stages

All repo-writing workflow stages must call `superpowers-repo-safety check --intent write ...` before the repo mutation happens.

This includes:

- `brainstorming` before creating or updating the spec file in the repo and before committing that doc
- `plan-ceo-review` before spec edits and approval-header writes
- `writing-plans` before creating or updating the plan file in the repo and before committing that doc
- `plan-eng-review` before plan edits and approval-header writes
- execution flows before repo file mutation during implementation work
- `document-release` before repo doc updates
- `finishing-a-development-branch` before merge, push, cleanup, or other repo-mutating closeout commands

Local runtime-state operations such as `superpowers-workflow-status expect` and `sync` are not repo writes and must remain outside the protected-branch gate.

### Read-only flows

These flows must remain unblocked by protected-branch enforcement:

- `superpowers-workflow`
- `superpowers-workflow-status`
- review-only inspection
- investigation and debugging that do not mutate repo files
- repo-safety checks run with `--intent read`

## Failure Behavior

### Session entry

- missing decision file: `needs_user_choice`
- malformed decision file: `needs_user_choice`
- explicit re-entry with persistence failure: allow the current turn, mark the outcome unpersisted, leave future turns undecided
- helper runtime failure: surface a visible runtime failure and do not silently fall through to normal behavior

### Repo safety

- protected branch with no matching approval: `blocked`
- approval scope mismatch: `blocked`
- malformed approval record: `blocked`
- helper runtime failure: `runtime_failure`, and the calling stage must fail closed before the repo mutation
- blocked responses must explain that Superpowers will not auto-create a worktree or auto-switch branches

## Observability And Auditability

This change does not introduce a metrics backend. The observability surface is local, deterministic, and testable.

Required visibility:

- stable JSON `reason` and `failure_class` values from both helpers
- persisted decision files for session bypass state
- persisted task-scoped approval records for protected-branch overrides
- explicit blocked-write output naming the branch, stage, and required next action
- explicit approval output naming the approval record path and approved scope

This is sufficient for local inspection, deterministic tests, and future public inspection work if that becomes necessary later.

## Testing And Regression Coverage

The implementation should add three layers of coverage.

### 1. Deterministic helper tests

Add direct tests for `superpowers-session-entry` covering:

- missing decision state
- valid `enabled`
- valid `bypassed`
- malformed decision state
- explicit re-entry while bypassed
- explicit re-entry when persistence fails

Add direct tests for `superpowers-repo-safety` covering:

- protected branch blocked by default
- non-protected branch allowed
- protected branch allowed with a matching approval record
- mismatched task id rejected
- mismatched path scope rejected
- mismatched symbolic write target rejected
- malformed approval record rejected
- read intent allowed

### 2. End-to-end session-entry gate

Add at least one fresh-session integration gate that does not pre-seed `enabled`.

That gate must fail if:

- a fresh real session enters normal behavior before the bypass question
- a malformed decision file enters normal behavior before the bypass question
- the harness-facing entry contract skips `superpowers-session-entry`

This is intentionally different from the existing post-bypass routing eval. The current routing eval should remain focused on post-bootstrap stage routing, while the new gate owns first-turn bootstrap proof.

### 3. Workflow-stage regression tests

Add explicit coverage proving that repo-writing stages:

- fail on `main`, `master`, `dev`, and `develop` by default
- succeed on a feature branch
- succeed on a protected branch only when the explicit approval record matches the task scope
- do not silently broaden a protected-branch override from one stage to another

Add negative tests proving:

- `superpowers-workflow` and `superpowers-workflow-status` are not blocked
- read-only review and investigation flows are not blocked
- local runtime-state writes such as `expect` and `sync` are not treated as repo writes

## Documentation Updates

Update docs and testing guidance so the distinctions are explicit:

- helper-backed workflow-state guarantees
- bootstrap-entry guarantees
- protected-branch repo-write guarantees

Required doc surfaces:

- generated `using-superpowers` contract
- workflow skill docs that now call `superpowers-repo-safety`
- `docs/testing.md`
- any runtime-facing install or workflow docs that describe supported entry behavior

## Rollout

Rollout should be staged:

1. ship the new helpers plus deterministic helper tests
2. update generated `using-superpowers` docs and supported entry instructions to call the new session-entry helper
3. wire `superpowers-repo-safety` into every repo-writing workflow stage
4. land the new end-to-end session-entry gate and stage-level protected-branch regressions
5. update testing and workflow docs

During rollout, the repo should keep the current post-bypass routing eval intact and add the new bootstrap gate alongside it rather than rewriting the routing gate in place.

## Rollback

Rollback is straightforward because the new state is local-only.

If the hardening causes unacceptable friction:

- remove the new helper call sites
- revert generated-doc changes
- revert new tests
- leave local decision files and approval records in `~/.superpowers/` as inert state

No repo migration or artifact rewrite is required for rollback.

## Risks And Mitigations

| Risk | Why it matters | Mitigation |
| --- | --- | --- |
| Harness entry integration remains incomplete on some supported surfaces | The first-turn guarantee is only real where the entry contract is adopted | Make the harness-facing contract explicit in supported runtime docs and add the fresh-session integration gate that fails if the helper is skipped |
| Protected-branch gate over-blocks legitimate work | A noisy gate will get bypassed or distrusted | Limit the gate to repo writes, keep read-only and local-state flows exempt, and provide a narrow task-scoped approval path |
| Protected-branch approval scope becomes too broad | A sticky override would recreate the same trust problem in a different form | Bind approvals to branch, stage, task id, repo-path scope, and symbolic write targets |
| Session identity is unstable | A drifting session key could re-ask or skip incorrectly | Support explicit session keys, use env fallback, and keep `$PPID` only as a compatibility fallback |
| Worktree wording creates a loophole | Users may think any worktree makes a protected branch safe | State clearly that worktree location alone does not exempt a protected branch; only a non-protected branch or explicit approval does |

## Acceptance Criteria

- A fresh session cannot enter normal Superpowers workflow behavior without first resolving the bypass gate.
- Missing or malformed session decision state fails closed and is covered by tests.
- Explicit re-entry still works on the current turn even when persistence fails, and that behavior is covered by tests.
- Repo-writing workflow stages cannot mutate repo state on `main`, `master`, `dev`, or `develop` by accident.
- Protected-branch writes require an explicit, narrow, auditable task-scoped approval record.
- Read-only workflow helpers and local runtime-state writes are not accidentally blocked by the protected-branch gate.
- `superpowers-workflow-status` and `superpowers-plan-execution` remain within their current authority boundaries.
- The new regression coverage would have caught both failures observed in this session.

## Out-Of-Scope Follow-Ups

If later usage shows the need, future work may consider:

- richer protected-branch pattern support
- a public inspection surface for branch-safety approvals and session-entry diagnostics
- longer-lived policy around explicit protected-branch approvals

Those follow-ups are intentionally out of scope for this hardening pass.
