# Workflow State Runtime
**Workflow State:** Draft
**Spec Revision:** 1
**Last Reviewed By:** brainstorming

## Summary

Add a manifest-backed workflow runtime layer that can determine the next safe Superpowers stage even when no spec or plan artifacts exist yet. Repo documents remain authoritative for approvals and revision gates; the runtime manifest exists to bootstrap missing-artifact states, index expected artifact paths, and reconcile local workflow state against the repo.

## Problem

The current product-workflow router assumes that relevant workflow artifacts already exist under `docs/superpowers/specs/` and `docs/superpowers/plans/`. That works once a workflow is already in motion, but it breaks down in the exact bootstrap case where the runtime should be most helpful:

- a repo has no workflow artifacts yet
- a workflow has started conceptually but the next artifact has not been written yet
- a user or agent needs to know the next safe stage before any design or plan document exists
- a local session has enough context to continue, but artifact discovery alone cannot tell whether the expected next document is missing, pending, or stale

This creates an avoidable gap between Superpowers' product promise and the runtime's current behavior. The system already has durable runtime state under `~/.superpowers/` and already uses cross-session artifacts for QA handoff. Product workflow state should get the same kind of bootstrap support without making local runtime state the approval authority.

## Goals

- Determine the next safe workflow stage for product work even when no spec or plan artifact exists yet.
- Preserve repo documents as the authoritative source for approval state and revision linkage.
- Add a reconstruction-friendly runtime manifest that records expected artifact paths and current derived workflow status.
- Provide a helper binary that relevant skills can call instead of reimplementing routing logic in prose alone.
- Fail closed to the earlier safe stage when repo docs and manifest state disagree.
- Support lazy backfill for existing repositories with zero explicit migration.

## Non-Goals

- Make local runtime state authoritative over repo-tracked workflow docs.
- Replace spec and plan header contracts with manifest-only approval semantics.
- Expand v1 beyond the default product-workflow pipeline (`brainstorming -> plan-ceo-review -> writing-plans -> plan-eng-review -> implementation`).
- Ship a supported user-facing workflow CLI in this change.

## Proposed Architecture

Add two new runtime surfaces:

1. `bin/superpowers-workflow-status`
2. `~/.superpowers/projects/<repo-slug>/workflow-state.json`

The helper binary is the runtime entrypoint. It reads and updates the repo-scoped manifest, inspects any existing workflow docs, and returns the current derived status plus the next safe skill. The manifest is a local index, not the approval record.

Authority split:

- Spec approval comes from the spec document headers.
- Plan approval comes from the plan document headers and source-spec linkage.
- The manifest may record that a spec or plan path is expected before the file exists.
- If a spec or plan file exists, the helper reparses the file and treats its headers as authoritative.
- If the manifest and docs disagree, the helper routes to the earlier safe stage and reports the mismatch.

### High-Level Flow

```text
user request
   |
   v
using-superpowers
   |
   v
superpowers-workflow-status --refresh
   |
   +--> no manifest, no docs
   |       -> bootstrap manifest
   |       -> next skill: brainstorming
   |
   +--> manifest exists, docs missing
   |       -> use manifest intent
   |       -> next skill: earlier safe stage
   |
   +--> docs exist
           -> parse authoritative headers
           -> reconcile manifest
           -> next skill from approved/draft state
```

## Manifest Contract

Manifest location:

- `~/.superpowers/projects/<repo-slug>/workflow-state.json`

Suggested shape:

```json
{
  "version": 1,
  "repo": {
    "slug": "owner-repo",
    "root": "/abs/path/to/repo"
  },
  "workflow": {
    "kind": "product-change",
    "status": "needs_brainstorming",
    "next_skill": "superpowers:brainstorming",
    "reason": "No relevant spec artifact exists yet"
  },
  "artifacts": {
    "spec": {
      "path": "docs/superpowers/specs/2026-03-17-workflow-state-runtime-design.md",
      "exists": true,
      "workflow_state": "Draft",
      "spec_revision": 1,
      "last_reviewed_by": "brainstorming"
    },
    "plan": {
      "path": "docs/superpowers/plans/2026-03-17-workflow-state-runtime.md",
      "exists": false,
      "workflow_state": null,
      "source_spec": null,
      "source_spec_revision": null,
      "last_reviewed_by": null
    }
  },
  "timestamps": {
    "updated_at": "2026-03-17T12:34:56Z"
  }
}
```

Rules:

- `workflow.status` is derived and may be rewritten whenever the helper refreshes from docs.
- `artifacts.spec.path` and `artifacts.plan.path` may be recorded before the corresponding file exists.
- Artifact paths should be repo-relative for portability; `repo.root` is stored only to identify the current checkout context.
- The manifest must remain reconstructable from repo context plus artifact discovery.
- The manifest must never be the sole source of approval truth.

## Helper Interface

The helper is internal-first in v1. It should expose machine-readable output by default and a short human summary when requested.

### Commands

```text
superpowers-workflow-status status [--refresh] [--summary]
superpowers-workflow-status expect --artifact spec|plan --path <repo-relative-path>
superpowers-workflow-status sync --artifact spec|plan [--path <repo-relative-path>]
```

Behavior:

- `status`
  - Resolves current workflow state from manifest plus repo docs.
  - Creates the manifest lazily if none exists.
  - Defaults to JSON output.
- `status --refresh`
  - Forces reconciliation from current repo docs before returning.
- `status --summary`
  - Prints a compact human-readable explanation in addition to, or instead of, JSON. Exact format can be finalized during planning.
- `expect`
  - Records the intended future path for a spec or plan before the file exists.
- `sync`
  - Reads the actual file, parses authoritative headers, and updates manifest discovery fields.

Exit codes:

- Exit `0` for successful state resolution, including safe-stage fallback outcomes.
- Exit nonzero only for true helper/runtime failures, such as invalid invocation or unreadable repo state.

### Expected Status Outcomes

Examples:

- No manifest, no docs -> `superpowers:brainstorming`
- Manifest expects spec path, file still missing -> `superpowers:brainstorming`
- Draft spec exists -> `superpowers:plan-ceo-review`
- Approved spec exists, no plan exists -> `superpowers:writing-plans`
- Draft plan exists -> `superpowers:plan-eng-review`
- Approved plan references stale spec revision -> `superpowers:writing-plans`
- Approved plan matches current approved spec revision -> implementation handoff

## Skill Integration

Relevant skills stop treating artifact inspection as purely ad hoc shell logic and instead call the helper first.

### `using-superpowers`

- Calls `superpowers-workflow-status status --refresh`
- Uses returned `next_skill` and `reason`
- Falls back to manual repo inspection only if the helper itself fails

### `brainstorming`

- Before writing the spec doc, records the intended spec path with `expect`
- After writing the spec doc, runs `sync --artifact spec`
- Leaves status in a draft-spec state that routes to `plan-ceo-review`

### `plan-ceo-review`

- Uses helper state to identify the current spec path when possible
- After spec edits or approval, runs `sync --artifact spec`
- If the spec becomes approved, helper resolves to `superpowers:writing-plans`

### `writing-plans`

- Reads the approved spec path from helper state when available
- Before writing the plan, records intended plan path with `expect`
- After writing the plan, runs `sync --artifact plan`
- Leaves status in a draft-plan state that routes to `plan-eng-review`

### `plan-eng-review`

- Refreshes plan and linked spec state through the helper
- If plan approval succeeds and source-spec revision is current, helper resolves to implementation
- If the linked approved spec revision is stale, helper routes back to `superpowers:writing-plans`

## Reconciliation Rules

The helper must fail closed.

- If the manifest says a spec exists but the file is missing, treat it as a missing artifact and route earlier.
- If the manifest says a plan exists but the file is missing, treat it as a missing artifact and route earlier.
- If a doc exists but required headers are malformed, treat that doc as draft/malformed and route earlier.
- If the manifest claims approval but the doc headers do not, the doc wins.
- If multiple candidate docs exist and the helper cannot determine which one is current, route to the earlier safe stage and explain the ambiguity.
- If the manifest is corrupted or deleted, rebuild it from current repo context and any discoverable workflow docs.

### Reconciliation Flow

```text
manifest present?
  |
  +--> no
  |      -> create bootstrap manifest
  |
  +--> yes
         |
         v
   docs present?
         |
         +--> no
         |      -> keep intent fields
         |      -> route earlier safe stage
         |
         +--> yes
                -> parse headers
                -> compare manifest vs docs
                -> authoritative docs win
                -> update derived workflow status
```

## Error Handling

Specific failure modes the helper and skills must surface explicitly:

- Missing artifact expected by manifest
- Malformed workflow headers in spec
- Malformed workflow headers in plan
- Approved plan linked to missing spec
- Approved plan linked to stale spec revision
- Multiple candidate specs with no unambiguous current winner
- Multiple candidate plans with no unambiguous current winner
- Corrupted manifest JSON
- Repo identity mismatch caused by moved checkout or changed remote slug

User-facing behavior for these cases should remain conservative:

- report the mismatch or malformed state
- route to the earlier safe stage
- avoid silently promoting workflow state

## Testing

Add a dedicated shell regression suite for the helper, following the runtime helper test style already used in this repo.

Required scenarios:

- bootstrap with no manifest and no workflow docs
- manifest bootstrap when `docs/superpowers/` does not exist
- draft spec resolution
- approved spec without plan resolution
- draft plan resolution
- approved plan with current spec resolution
- approved plan with stale spec revision resolution
- malformed spec headers
- malformed plan headers
- manifest/doc mismatch
- corrupted manifest recovery
- helper-created manifest backfilled from existing valid docs

Test strategy:

- use fixture-backed workflow docs for deterministic approval-state scenarios
- add purpose-built temporary repos for missing-artifact and corruption scenarios
- extend sequencing tests to assert that workflow-critical skills call the helper before manual inspection
- add PowerShell wrapper coverage if the helper ships as a public runtime binary on Windows

## Rollout And Migration

Rollout principles:

- Ship this as an internal runtime primitive first.
- Limit v1 to the default product-workflow pipeline.
- Keep repo docs authoritative and document that clearly.

Migration behavior:

- No explicit migration command in v1
- Manifest created lazily on first helper use
- Existing repos with workflow docs are backfilled from those docs
- Existing repos without workflow docs start in bootstrap state and route to `superpowers:brainstorming`
- Deleting the manifest is safe; the helper recreates it

## Alternatives Considered

### Read-only artifact scanner

A read-only helper that only scans spec/plan docs is a simpler first step, but it does not solve the bootstrap problem where the runtime needs to know the next stage before artifacts exist.

### Manifest-authoritative workflow engine

Making the manifest authoritative would create a stronger long-term workflow engine, but it would also introduce hidden local truth and force approval semantics away from the repo docs. That is too large a conceptual shift for this change.

## Deferred Follow-Ups

- Add a supported user-facing CLI built on the same helper and manifest layer once the internal runtime contract is stable.
- Consider expanding the manifest pattern to other workflow surfaces only after the product-workflow pipeline proves reliable.

## Open Questions For Review

- Should the helper's human-readable summary be stable and documented now, or remain intentionally internal in v1?
- Should the helper store only the current artifact paths, or preserve a small bounded history of superseded paths for debugging?
- Should repo identity be keyed only from remote slug, or should the manifest also store a stable repo UUID/fingerprint to handle renamed remotes more gracefully?
