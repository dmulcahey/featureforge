# Workflow Doctor Compact Dashboard As Primary Orientation Surface

**Workflow State:** CEO Approved  
**Spec Revision:** 1  
**Last Reviewed By:** plan-ceo-review

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
- [WD-005][behavior] `featureforge workflow doctor --json` must remain available and backward-compatible: schema version and existing keys remain stable for current consumers; additive optional fields are allowed.
- [WD-006][behavior] The dashboard must surface runtime-owned reason codes in compact, human-meaningful form.
- [WD-007][verification] Snapshot tests must cover major workflow states for doctor text rendering.
- [WD-008][verification] Skill-doc tests must verify `using-featureforge` points to doctor as primary orientation surface.
- [WD-009][verification] Fail-closed negative-path tests must cover helper/runtime failure, malformed required payloads, and required-section structural inconsistencies for doctor text/JSON behavior.
- [WD-010][security] Doctor text rendering must sanitize runtime-derived strings to prevent terminal control-sequence or output-injection artifacts while preserving human-readable meaning.
- [WD-011][verification] Security-oriented rendering tests must cover ANSI/control-sequence payloads and malformed runtime-derived strings to prove text-mode sanitization behavior.
- [WD-012][behavior] Required dashboard fields must follow deterministic null/empty normalization (`none`, `unknown`, or fail-closed) with no silent blank required rows.
- [WD-013][behavior] Dashboard text rendering must use one atomic runtime snapshot; if cross-section consistency cannot be guaranteed, doctor must fail closed.
- [WD-014][behavior] Artifact path rows must use deterministic single-line tail-preserving truncation in text mode when over length budget, with full path values preserved in JSON.
- [WD-015][performance] Dashboard text rendering must run from one pre-fetched doctor context/snapshot without additional repo scans or duplicate status/gate queries during section emission.

## Dashboard Contract

### Global rendering rules

- plain text only; no mandatory color/ANSI
- stable section order
- keep common-case output within roughly 25 lines
- avoid raw JSON-like dumps in text mode
- omit empty optional sections unless omission would create ambiguity
- deterministic overflow behavior is required when content exceeds one-screen budget
- text renderer must strip or escape terminal control sequences from runtime-derived strings (paths, reason text, remediation text, and action text) before display
- null/empty normalization for required fields:
  - artifact path fields render `none` when absent
  - classification/state fields render `unknown` when unavailable but structurally valid
  - required action/routing fields that are structurally missing or empty trigger fail-closed error path
  - renderer must not emit blank required rows
- all sections must render from one consistent runtime snapshot/version; mixed-era section composition is not allowed
- render-cost constraint: section emission must not trigger additional repo scans or duplicate workflow/execution/gate queries beyond the initial doctor context fetch

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
- `Do this now` must remain a single rendered line in text mode
- when `Do this now` exceeds 96 characters, truncate in text mode with trailing `...`
- full untruncated action text must remain available in JSON via `next_step`

### Artifacts

Required rows:

- spec path or `none`
- plan path or `none`
- contract state

Future-compatible optional rows are allowed when runtime truth exists (for example delivery-lane or review-stack summaries).

Path rendering rules:

- artifact paths render on one line in text mode
- when an artifact path exceeds 96 characters, truncate deterministically by preserving filename and nearest parent segments with `...` prefix
- full untruncated artifact path remains available in JSON

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
- each blocker bullet must include canonical reason code plus action text in one line (format: `<reason_code> — <plain-language action>`)
- ordering source is authoritative runtime-provided reason order from doctor context; renderer must preserve that order before truncation
- if a deterministic tiebreak is required, sort tied entries by canonical reason code (ASCII lexical ascending)
- when more than three blocker bullets are available, render the top three by runtime priority and append one summary line: `+N more blockers`

### Warnings

Use for important but non-blocking concerns (for example legacy artifact formats, accepted scope drift follow-up, stale non-gating advisory signals).

Warnings overflow rule:

- ordering source is authoritative runtime-provided warning order from doctor context; renderer must preserve that order before truncation
- if a deterministic tiebreak is required, sort tied entries by canonical warning code (ASCII lexical ascending)
- when more than two warning bullets are available, render the top two by runtime priority and append one summary line: `+N more warnings`

## Reason-Code Compaction And Action Mapping

Doctor text mode must preserve runtime truth while improving readability:

- retain canonical reason codes as the underlying source
- provide compact human-facing phrasing for operator action
- include canonical reason code in displayed blocker bullets so text-mode output is directly traceable
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
- JSON compatibility boundary:
  - preserve existing `schema_version` behavior for the current doctor JSON contract
  - preserve existing top-level and nested keys currently consumed by runtime/fixture/smoke tests
  - allow only additive optional fields in this change slice
  - if a future breaking JSON change is required, it must be explicitly versioned and migration-documented outside this scope

Status compatibility:

- `featureforge workflow status --refresh` remains supported as compatibility/secondary helper behavior.
- docs and skills should stop presenting `status --refresh` as the primary human-facing orientation command.
- helper-first routing policy is fail-closed on doctor: do not auto-fallback to alternate helper chains when doctor fails; surface the failure and repair doctor-path issues directly.
- doctor failure contract in fail-closed mode:
  - command exits non-zero
  - failure output includes named `failure_class`
  - failure output includes canonical `reason_codes`
  - text mode includes exactly one remediation line describing the next repair action
  - remediation line must be runtime-authored from canonical route/gate diagnostics; renderer must not invent ad-hoc remediation text
  - JSON mode preserves structured failure payload for automation consumption
  - partial-surface strictness: if required section data for current phase is structurally invalid or inconsistent, doctor must fail closed instead of rendering partial degraded output
  - debugging transparency: doctor failure output may include full raw diagnostic payloads (including raw paths and state detail) when provided by runtime diagnostics

## `using-featureforge` Routing Update

Helper-first orientation must prefer doctor:

- primary helper call: `$_FEATUREFORGE_BIN workflow doctor --json`
- optional user-facing summary: `$_FEATUREFORGE_BIN workflow doctor`

Routing expectations:

- if `next_skill` is present, route to it
- if the user asks for diagnosis/orientation, show dashboard text directly
- do not auto-fallback to `workflow status --refresh` for helper-first orientation when doctor is unavailable or fails; fail closed and fix doctor-path correctness

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
- fail-closed negative-path coverage must assert:
  - non-zero exit behavior
  - named `failure_class`
  - canonical `reason_codes`
  - exactly one runtime-authored remediation line in text mode
- security rendering coverage must assert:
  - runtime-derived strings containing ANSI/control sequences are rendered without terminal control effects
  - malformed or injection-like runtime-derived strings are rendered as inert text while preserving readable semantics

### Requirement-to-test traceability

| Requirement | Expected coverage |
| --- | --- |
| WD-001 | doctor text snapshot tests in Rust runtime/shell smoke suites for compact one-screen layout |
| WD-002 | doctor text snapshot assertions for phase/next skill/next action/artifacts/blockers presence |
| WD-003 | late-stage fixture snapshots asserting late-stage section rows and status tokens |
| WD-004 | skill-doc contract tests for `using-featureforge` helper-first routing language |
| WD-005 | JSON compatibility tests asserting schema version stability and key preservation |
| WD-006 | text + JSON parity tests asserting blocker lines include canonical reason code + action mapping |
| WD-007 | snapshot matrix covering enumerated major workflow states |
| WD-008 | skill-doc tests asserting doctor-first orientation and no doctor-to-status auto-fallback guidance |
| WD-009 | negative-path tests for fail-closed behavior: non-zero exit, `failure_class`, `reason_codes`, remediation line |
| WD-010 | security rendering tests proving control-sequence stripping/escaping for runtime-derived strings |
| WD-011 | adversarial fixture tests with ANSI/control payloads and malformed/injection-like strings |
| WD-012 | normalization tests for required-field null/empty handling (`none`, `unknown`, fail-closed) |
| WD-013 | atomic snapshot consistency tests rejecting mixed-era section composition |
| WD-014 | deterministic artifact-path truncation tests with JSON full-path preservation checks |
| WD-015 | performance-oriented tests/assertions proving no extra scans or duplicate gate/status queries during text section rendering |

## Migration And Rollout

- plain-text users receive improved default orientation immediately
- automation continues using `--json`
- no runtime-state schema migration is required when dashboard output is derived from existing fields
- compatibility support for `workflow status --refresh` remains during transition

## Review Decisions (Hold Scope)

- Observability scope hold: this spec does not add new telemetry or metrics requirements for doctor rendering/truncation/fail-closed paths; behavior and verification requirements in this spec remain the delivery boundary.
- Rollout gate scope hold: this spec does not add a new pre-merge smoke rollout matrix for the default text-mode switch; existing unit/integration/snapshot verification remains the gate.

## Acceptance Criteria

This spec is complete when all are true:

- `workflow doctor` is clearly more useful than `workflow status --refresh` for first-look human orientation
- operators can identify phase, next skill, and blocker in one screen
- `using-featureforge` prefers doctor in helper-first orientation guidance
- JSON automation pathways remain intact and backward-compatible

## Forward Contract

- deterministic rendering, fail-closed behavior, output sanitization, and atomic snapshot composition defined in this spec are baseline workflow-doctor invariants for future extensions unless superseded by a later CEO-approved spec revision.

## CEO Review Summary

**Review Status:** clear
**Reviewed At:** 2026-04-08T18:57:27Z
**Review Mode:** hold_scope
**Reviewed Spec Revision:** 1
**Critical Gaps:** 0
**UI Design Intent Required:** no
**Outside Voice:** skipped
