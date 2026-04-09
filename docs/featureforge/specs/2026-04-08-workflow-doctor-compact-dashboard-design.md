# Workflow Doctor Compact Dashboard As Primary Orientation Surface

**Workflow State:** Draft  
**Spec Revision:** 1  
**Last Reviewed By:** brainstorming

## Problem Statement

FeatureForge already exposes rich runtime-owned workflow truth, but the default operator-facing orientation path is fragmented:

- `workflow doctor` text output is currently flat and low-signal
- helper-first guidance in `using-featureforge` still anchors on `workflow status --refresh`
- operators and agents often need multiple commands to answer three basic questions:
  - where am I in the workflow?
  - what is blocking progress?
  - what should I do next?

This creates avoidable routing mistakes, repeated helper calls, and stage confusion, especially in Codex/Copilot-heavy execution flows.

## Desired Outcome

`featureforge workflow doctor` becomes the default one-screen orientation surface that makes next action obvious without opening JSON, while preserving runtime truth and machine compatibility.

The default view should immediately communicate:

- current workflow phase and route
- next skill and next action
- active artifact pointers
- current blockers and notable warnings
- late-stage branch-finishing status when applicable

## Decision

Selected approach: keep existing runtime truth sources and `WorkflowDoctor` model semantics, but replace default text rendering with a compact dashboard and update `using-featureforge` to treat `workflow doctor` as first-stop orientation.

This keeps automation stable (`--json`) while improving operator ergonomics.

## Scope

In scope:

- compact text dashboard for `workflow doctor` default output
- high-signal sectioned rendering rules with omission behavior
- compact blocker and warning presentation with runtime-owned reason-code mapping
- late-stage summary rendering for branch-finishing flow
- `using-featureforge` helper-first routing/docs changes to prefer doctor
- verification updates (snapshot coverage + skill-doc contract checks)

Out of scope:

- replacing `workflow operator`, `execution status`, gate commands, or plan-contract analysis commands
- introducing GUI/graphical UI
- removing underlying reason-code detail surfaces from JSON and deeper diagnostics
- schema migrations for persisted runtime state

## Requirement Index

- [WD-001][behavior] `featureforge workflow doctor` must render a compact, one-screen dashboard in default text mode.
- [WD-002][behavior] The dashboard must show workflow phase, next skill, next action, artifact pointers, and current blockers.
- [WD-003][behavior] The dashboard must summarize late-stage status when a plan has reached branch-finishing flow.
- [WD-004][behavior] `using-featureforge` must prefer `workflow doctor` for helper-first routing and orientation.
- [WD-005][behavior] `featureforge workflow doctor --json` must remain available and backward-compatible.
- [WD-006][behavior] The dashboard must surface runtime-owned reason codes in compact, human-meaningful form.
- [WD-007][verification] Snapshot tests must cover major workflow states for doctor text rendering.
- [WD-008][verification] Skill-doc tests must verify `using-featureforge` points to doctor as primary orientation surface.

## Dashboard Contract

### Global rendering rules

- plain text only; no mandatory color/ANSI
- stable section order
- keep common-case output within roughly 25 lines
- avoid raw JSON-like dumps in text mode
- omit empty optional sections unless omission would create ambiguity

### Section order

1. Header
2. Next move
3. Artifacts
4. Execution (conditional)
5. Late stage (conditional)
6. Blockers (conditional but required when blocking exists)
7. Warnings (conditional)

### Header

Required fields:

- repo slug/name
- branch
- phase
- route status

Purpose: immediate orientation to the currently evaluated workflow state.

### Next move

Required lines:

- `Next skill:`
- `Do this now:`

Rules:

- one line each
- plain-language wording for `Do this now`
- `Do this now` remains grounded in runtime-owned action truth (not ad hoc inference)

### Artifacts

Required rows:

- spec path or `none`
- plan path or `none`
- contract state

Future-compatible optional rows are allowed when runtime truth exists (for example delivery-lane or review-stack summaries).

### Execution

Render only when execution-status data exists. Rows:

- mode
- started
- active task
- blocking task
- resume task

Rules:

- omit entire section when execution status is unavailable
- when execution is clean/complete, explicitly render `none` values for task fields

### Late stage

Render only for late-stage branch-finishing flow (for example document release/final review/QA/branch completion path). Rows:

- document release
- final review
- QA
- branch completion

Allowed display statuses:

- `done`
- `next`
- `required`
- `not required`
- `waiting`
- `blocked`

### Blockers

Mandatory when current forward progress is blocked.

Rules:

- show highest-signal blocker reasons
- collapse low-value raw-reason noise when multiple reasons map to one practical action
- target 1-3 bullets when practical

### Warnings

Use for important but non-blocking concerns (for example legacy artifact formats, accepted scope drift follow-up, stale non-gating advisory signals).

## Reason-Code Compaction And Action Mapping

Doctor text mode must preserve runtime truth while improving readability:

- retain canonical reason codes as the underlying source
- provide compact human-facing phrasing for operator action
- avoid lossy inference that changes required next actions

Example compact mappings:

- `final_review_dispatch_required` -> Dispatch the independent final reviewer.
- `plan_fidelity_receipt_missing` -> Run `plan-fidelity-review` for the current draft plan revision.
- `document_release_artifact_stale` -> Run `document-release` for current `HEAD` before final review.

JSON and deeper diagnostic commands remain the source for full reason-code detail.

## Data Model Expectations

The compact dashboard should primarily derive from existing `WorkflowDoctor` fields already present in this branch.

Optional derived convenience fields may be added when useful for renderer simplicity and forward compatibility:

```json
{
  "headline": "final_review_pending",
  "blockers": ["final_review_dispatch_required"],
  "warnings": [],
  "late_stage_summary": {
    "document_release": "done",
    "final_review": "next",
    "qa": "not_required",
    "branch_completion": "waiting"
  }
}
```

Derived fields are additive convenience and do not replace existing nested authoritative state.

## Command Behavior

Default:

- `featureforge workflow doctor` returns compact dashboard text.

Machine-readable:

- `featureforge workflow doctor --json` returns compatible JSON payload.

Status compatibility:

- `featureforge workflow status --refresh` remains supported as compatibility/secondary helper behavior.
- docs and skills should stop presenting `status --refresh` as the primary human-facing orientation command.

## `using-featureforge` Routing Update

Helper-first orientation must prefer doctor:

- primary helper call: `$_FEATUREFORGE_BIN workflow doctor --json`
- optional user-facing summary: `$_FEATUREFORGE_BIN workflow doctor`

Routing expectations:

- if `next_skill` is present, route to it
- if the user asks for diagnosis/orientation, show dashboard text directly
- `workflow status --refresh` remains secondary/fallback, not the primary front-door recommendation

## Verification

### Snapshot coverage

Doctor text snapshots must cover at least:

- brainstorming/pre-approval
- plan-fidelity pending
- engineering review pending
- execution in progress
- document release pending
- final review pending
- QA pending
- ready for branch completion

### Skill-doc contract coverage

Tests must assert `using-featureforge` helper-first orientation points to doctor as the primary orientation surface, not only to `workflow status --refresh`.

### CLI compatibility coverage

- `workflow doctor --json` remains stable or explicitly versioned if shape changes are intentional
- default doctor text snapshots remain stable enough for operator reliance

## Migration And Rollout

- plain-text users receive improved default orientation immediately
- automation continues using `--json`
- no runtime-state schema migration is required when dashboard output is derived from existing fields
- compatibility support for `workflow status --refresh` remains during transition

## Acceptance Criteria

This spec is complete when all are true:

- `workflow doctor` is clearly more useful than `workflow status --refresh` for first-look human orientation
- operators can identify phase, next skill, and blocker in one screen
- `using-featureforge` prefers doctor in helper-first orientation guidance
- JSON automation pathways remain intact and backward-compatible
