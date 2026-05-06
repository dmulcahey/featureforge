# Workflow Doctor Headless And Compact Dashboard Contract

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

## Problem Statement

FeatureForge already computes rich doctor data in runtime code (`WorkflowDoctor` in `src/workflow/operator.rs`), but `workflow doctor` is currently removed from the public CLI surface. The active public workflow query path is `workflow operator`, and deep diagnostics are split across `workflow operator` plus `plan execution status`.

That split is workable for interactive operators, but it is weak for CI and non-TTY automation that need one deterministic answer to:

- what phase/detail and review-state status are active now
- which public command argv is legal next (if any)
- what typed inputs are missing when argv is not yet actionable
- why no public command is actionable yet

The audit recommendation and prior dashboard-oriented proposal both target this visibility gap, but the current codebase constitution has changed:

- `workflow operator` is the routing authority and must remain authoritative
- removed compatibility workflow commands (`phase`, `handoff`, `doctor`) are parse-boundary guarded today
- intent-level recovery already exists (`repair-review-state`, `close-current-task`, `advance-late-stage`, `materialize-projections`)

## Desired Outcome

Expose a public, machine-first doctor query that reuses existing operator/execution truth, returns deterministic next-step diagnostics for headless callers, and does not introduce a second routing graph or hidden mutation primitive.

## Reconciliation Decisions

1. Keep `workflow operator` as authoritative routing contract.
2. Reintroduce `workflow doctor` as a read-only diagnostic companion surface for headless automation.
3. Do not introduce `plan execution recover` in this slice; existing intent-level recovery commands remain the only mutation lanes.
4. Include compact dashboard text output as the default `workflow doctor` text projection in this slice.

## Scope

In scope:

- new public CLI command: `featureforge workflow doctor --plan <path>` with optional `--json`
- doctor JSON contract for deterministic diagnostics and legal-next-command guidance
- compact dashboard text contract for default non-JSON output
- operator/doctor routing parity requirements
- parse/help boundary updates limited to `workflow doctor`
- tests proving headless parity and stop-reason behavior

Out of scope:

- changing workflow/operator routing authority
- adding new mutation commands
- reviving removed compatibility workflow commands other than doctor

## Architecture Snapshots

### A1: Runtime Boundary Ownership

```text
featureforge CLI
  -> workflow doctor (public read-only)
      -> workflow::operator::build_context_with_plan(...)
          -> shared routing snapshot/context
              -> route fields (phase, phase_detail, next_action, ...)
              -> recommended_public_command_argv / required_inputs
              -> reason codes + wait-state diagnostics
      -> doctor projection (WorkflowDoctor + resolution)

Authority rule:
- workflow operator and workflow doctor consume the same snapshot/context.
- doctor adds diagnostics projection only; no new routing or mutation authority.
```

### A2: Headless Query Data Flow

```text
Input:
  doctor --plan <path> [--external-review-result-ready] [--json]

Flow:
  Parse CLI args
    -> validate plan path
    -> discover execution runtime
    -> build shared operator context
    -> project WorkflowDoctor
    -> derive resolution (shared helper)
    -> emit text or JSON (by --json flag)

Failure flow:
  invalid input / runtime read failure
    -> JsonFailure(error_class, message)
```

### A3: Command Availability State Transition

```text
[snapshot loaded]
      |
      v
has recommended_public_command_argv ?
  yes -> resolution.kind=actionable_public_command
         command_available=true
         stop_reasons=[]
  no  -> has required_inputs ?
           yes -> resolution.kind=actionable_public_command
                  command_available=false
                  stop_reasons=[]
           no  -> external wait state present ?
                    yes -> resolution.kind=waiting_external_input
                           command_available=false
                           stop_reasons=[canonical wait reason code(s)]
                    no  -> runtime diagnostic / terminal classification
                           resolution.kind=runtime_diagnostic_required|terminal
                           command_available=false
                           stop_reasons=[canonical reason code(s)]
```

## Requirement Index

- [DR-001][behavior] FeatureForge must expose public `workflow doctor` with explicit plan binding: `featureforge workflow doctor --plan <path>`, with optional `--json`.
- [DR-002][behavior] `workflow doctor` must accept `--external-review-result-ready` and evaluate the same recording-ready route context as `workflow operator --plan <path> --external-review-result-ready`.
- [DR-003][behavior] `workflow doctor` must reuse the same shared routing context used by workflow/operator and must not derive an independent phase or recovery graph.
- [DR-004][behavior] Doctor top-level routing fields (`phase`, `phase_detail`, `review_state_status`, `next_action`, `recommended_public_command_argv`, `required_inputs`, `blocking_scope`, `blocking_task`, `external_wait_state`, `blocking_reason_codes`) must match workflow/operator for identical inputs.
- [DR-005][behavior] Doctor JSON must expose deterministic diagnostic class and stop metadata for headless callers when no public command argv is actionable.
- [DR-006][behavior] Doctor must expose the exact legal next public command argv when actionable and must not expose shell-parsed command text as invocation authority.
- [DR-007][behavior] Doctor must expose typed `required_inputs` when argv is not actionable because required input binding is incomplete.
- [DR-008][behavior] When neither argv nor required inputs are actionable, doctor must expose explicit stop reasons derived from canonical runtime reason codes (no ad hoc prose-only failure states).
- [DR-009][behavior] The first slice must not add `plan execution recover`; recovery stays on existing intent-level public commands routed by operator/doctor output.
- [DR-010][behavior] Doctor JSON compatibility must preserve existing doctor keys and schema-version behavior for current internal consumers; additive optional fields are allowed.
- [DR-011][behavior] `workflow --help` must list `doctor`, while removed compatibility commands (`phase`, `handoff`) remain hidden and parse-rejected.
- [DR-012][verification] Shell-smoke and runtime parity tests must prove doctor and operator route parity across representative execution and late-stage states.
- [DR-013][verification] CLI parse-boundary tests must prove doctor requires `--plan`, accepts both with and without `--json`, and still rejects handoff/phase as unrecognized subcommands.
- [DR-014][verification] Negative-path tests must prove deterministic stop metadata when command argv is unavailable.
- [DR-015][performance] Doctor rendering/query must execute from one shared routing snapshot/context with no duplicate repo-scan path added solely for doctor.
- [DR-016][behavior] Doctor `resolution` fields must be derived by one focused shared runtime helper module from the same routing snapshot/context used for doctor/operator parity; command-local recomputation across surfaces is prohibited.
- [DR-017][security] Doctor text mode must render runtime-derived strings as inert text by stripping or escaping terminal control sequences from displayed paths, reason text, diagnostics, and command display fields.
- [DR-018][verification] Security rendering tests must cover ANSI/control-sequence and malformed runtime-derived payloads and assert text-mode output does not emit terminal control effects.
- [DR-019][performance] Each doctor invocation must build routing context once and must not perform additional workflow/operator requery loops or duplicate route-context construction within the same invocation.
- [DR-020][verification] Performance-oriented tests must assert single-invocation context-build behavior and no duplicate route requery loops in doctor path execution.
- [DR-021][observability] Doctor JSON must always emit deterministic diagnostic reason-code arrays for non-actionable states and may emit optional non-authoritative debug trace fields without changing routing authority.
- [DR-022][verification] Observability tests must assert stable classification markers in text mode and deterministic diagnostic reason-code projection in JSON mode.
- [DR-023][behavior] Default `workflow doctor --plan <path>` text output must be a compact dashboard view.
- [DR-024][behavior] Compact dashboard section order must be deterministic: Header, Next Move, Artifacts, Execution (conditional), Blockers (conditional), Warnings (conditional).
- [DR-025][behavior] Blocker lines in dashboard mode must include canonical reason code plus plain-language action in one line.
- [DR-026][behavior] Dashboard mode must include stable labeled markers for `Resolution kind` and `Command available`.
- [DR-027][verification] Dashboard text snapshots must cover representative phase/next-action states and assert section ordering and required-field rendering.
- [DR-028][verification] Dashboard text and JSON must remain semantically aligned for classification and command-availability behavior on matched fixtures.

## Public Command Contract

### Command

```bash
featureforge workflow doctor --plan <path>
featureforge workflow doctor --plan <path> --json
featureforge workflow doctor --plan <path> --external-review-result-ready
featureforge workflow doctor --plan <path> --json --external-review-result-ready
```

### Input Rules

- `--plan` is required.
- Missing plan file fails closed with `InvalidCommandInput`.
- `--json` is optional; JSON mode is the primary automation contract.

### Output Rules

Doctor remains a read-only query surface. It must include existing `WorkflowDoctor` contract fields plus additive headless-diagnostic convenience fields.

In default text mode, doctor output is a projection from the same `WorkflowDoctor` snapshot used for JSON mode. Text rendering must not classify routing or recovery state independently.

Required headless diagnostics:

- deterministic diagnostic class (`state_kind` or equivalent)
- canonical reason-code arrays (`blocking_reason_codes`, plus diagnostic reason codes)
- exact `recommended_public_command_argv` when actionable
- typed `required_inputs` when actionable only after binding inputs
- explicit stop reasons when no command is actionable

Additive `resolution` convenience shape:

```json
{
  "resolution": {
    "kind": "actionable_public_command|waiting_external_input|runtime_diagnostic_required|terminal",
    "stop_reasons": ["canonical_reason_code"],
    "command_available": true
  }
}
```

Rules:

- `resolution.command_available=true` only when `recommended_public_command_argv` is present.
- `resolution.stop_reasons` is required when `command_available=false` and `required_inputs` is empty.
- `recommended_command` stays display-only compatibility text.

### Compact Dashboard Text Contract

Default text mode (`workflow doctor --plan <path>`) renders a compact dashboard.

Required section order:

1. Header
2. Next Move
3. Artifacts
4. Execution (only when execution status exists)
5. Blockers (only when blockers exist)
6. Warnings (only when warnings exist)

Required rows:

- Header: `Phase`, `Phase detail`, `Review state`, `Route status`
- Next Move: `Next action`, `Next step`, `Resolution kind`, `Command available`
- Artifacts: `Spec`, `Plan`, `Contract state`

Blockers section contract:

- one blocker per line in format `<reason_code> - <plain-language action>`
- preserve canonical runtime reason order before truncation
- when more than three blockers exist, render first three and append `+N more blockers`

Warnings section contract:

- render at most two warning lines, then append `+N more warnings` when additional warnings exist

Rendering constraints:

- plain text only; no required ANSI color
- text mode remains a projection from the same snapshot as JSON mode
- stable labels are required for `Resolution kind` and `Command available`
- sanitization rules from DR-017 apply to all runtime-derived dashboard strings

Resolution precedence and tie-break rules:

1. If `recommended_public_command_argv` is present, classify as `actionable_public_command` and set `command_available=true`.
2. Else, if `required_inputs` is non-empty, classify as `actionable_public_command` and set `command_available=false`.
3. Else, if `external_wait_state` is present, classify as `waiting_external_input`.
4. Else, classify as `runtime_diagnostic_required` or `terminal` from canonical runtime state/diagnostic signals.

Tie-break constraints:

- Higher-precedence conditions must mask lower-precedence ones for `resolution.kind`.
- When step 4 applies, `runtime_diagnostic_required` takes precedence over `terminal` whenever canonical diagnostic reason codes are present.
- `resolution.stop_reasons` ordering must preserve canonical runtime reason-code order; if a deterministic tie-break is needed, sort tied entries ASCII-lexical ascending.

### Derivation Ownership And Module Boundary

- `workflow doctor` must evaluate one shared routing snapshot/context per invocation and derive both route fields and `resolution` from that same snapshot.
- `resolution` derivation logic must live in a focused helper module (for example `src/workflow/doctor_resolution.rs`) and be invoked by `workflow/operator` surfaces; avoid adding new local classification logic blocks directly inside large workflow files.
- `src/workflow/operator.rs` remains the orchestration call site but must not be the long-term home of duplicated or expanding `resolution` classification logic.
- If any other public surface needs the same classification later, it must reuse that shared helper or an extracted shared type; it must not reimplement `resolution` classification locally.

## Error & Rescue Registry

| Condition | Signal Surface | Classification | Required Rescue |
| --- | --- | --- | --- |
| plan path missing or unreadable | `JsonFailure.error_class=InvalidCommandInput` | command input failure | provide valid `--plan <path>` and rerun doctor |
| runtime discovery/read failure (non-git, missing runtime context, state corruption) | `JsonFailure.error_class` from runtime failure contract | runtime state failure | fix runtime/repo context, then rerun doctor |
| actionable command exists | `recommended_public_command_argv` present | `resolution.kind=actionable_public_command` | run the argv exactly |
| command requires typed inputs first | `recommended_public_command_argv` absent and `required_inputs` non-empty | `resolution.kind=actionable_public_command` with `command_available=false` | supply required inputs, then rerun route owner |
| waiting for external review result | `external_wait_state` present and no actionable argv | `resolution.kind=waiting_external_input` | wait for external result, rerun with `--external-review-result-ready` when available |
| runtime diagnostic stop | no actionable argv, no required inputs, canonical diagnostic reason codes present | `resolution.kind=runtime_diagnostic_required` | follow runtime diagnostic lane; do not invent new command families |
| terminal/no-op state | no actionable argv, no required inputs, terminal classification | `resolution.kind=terminal` | no mutation command required; continue with normal finish workflow if applicable |

Registry rules:

- Doctor text and JSON must expose the same underlying rescue classification from one snapshot.
- When doctor emits no actionable argv and no required inputs, `resolution.stop_reasons` must be non-empty and canonical.
- Rescue actions must stay on existing public command families (`repair-review-state`, `close-current-task`, `advance-late-stage`, execution step commands, workflow/operator requery lanes); do not introduce `recover`.
- Registry behavior must follow the precedence and tie-break rules defined in Output Rules.

## Security Boundaries

Threat surface in this slice:

- runtime-derived string fields projected into text output (paths, reason details, diagnostics, display command text)
- operator/doctor parity data crossing from runtime state into public CLI output

Security rules:

- JSON mode preserves structured payload values without text-rendering sanitization side effects.
- Text mode must sanitize runtime-derived strings so terminal control bytes and escape sequences do not execute in operator terminals.
- Sanitization must be applied during doctor text rendering only; it must not mutate authoritative runtime state or JSON payload truth.

## Observability And Debuggability

Required observability surfaces:

- JSON mode must expose deterministic diagnostic reason-code arrays for non-actionable classifications.
- Text mode must include stable labeled markers for classification and command-availability state (for example `Resolution kind:` and `Command available:` labels).

Optional debug trace:

- Doctor may include optional non-authoritative debug fields (for example `trace_id`, `debug_context`) in JSON output for troubleshooting.
- Optional debug fields must not be treated as route authority and must not replace canonical route/reason-code fields.

## Recovery Decision

### Decision For This Slice

Do not add `featureforge plan execution recover`.

Rationale:

- existing public recovery lane already exists through `repair-review-state`
- operator already returns `recommended_public_command_argv` and `required_inputs`
- adding a second recovery mutator now would duplicate authority and increase routing drift risk

### Future Trigger To Reopen

A separate recover-command spec may be opened only if one or both are proven by tests/evidence:

- a recurring runtime state where doctor/operator can classify the blocker but cannot route to any existing public command or typed input lane
- materially repetitive multi-command recovery choreography that cannot be represented by current routed argv plus typed inputs

## Relationship To Prior Dashboard Proposal

This spec now includes compact dashboard text output in the same delivery slice.

Still deferred from the broader dashboard proposal:

- text truncation/presentation rules
- advanced layout and one-screen budget tuning beyond this compact baseline

Those can be proposed as follow-up refinements after this compact baseline lands.

## `using-featureforge` And Routing Guidance

- `workflow operator` remains the authoritative routing call.
- `workflow doctor` is an additional diagnostic/orientation surface for CI, non-TTY, and explicit diagnosis workflows.
- Skill/doc guidance must not reintroduce status-only or compatibility fallback routing.

## Verification Plan

### CLI Boundary Coverage

- update workflow help tests: `doctor` visible, `phase`/`handoff` still hidden
- update parse-boundary tests:
  - `workflow doctor --plan <path>` accepted
  - `workflow doctor --plan <path> --json` accepted
  - `workflow doctor --json` rejected for missing `--plan`

### Route Parity Coverage

- shell-smoke parity matrix with identical args across:
  - `workflow operator --plan <path> --json`
  - `workflow doctor --plan <path> --json`
- assertions for route parity fields required by DR-004

### Cross-Mode Semantic Parity Coverage

- for matched fixtures, compare `workflow doctor --plan <path>` text mode and `workflow doctor --plan <path> --json`
- assert semantic parity for:
  - command availability state (`command_available` equivalent semantics)
  - actionable-vs-wait-vs-diagnostic-vs-terminal classification
  - presence/absence behavior for required inputs and stop reasons
- use stable token-level assertions for text mode (for example labeled lines/markers), not full-text byte-for-byte snapshots

### Dashboard Snapshot Coverage

- snapshot tests for default text dashboard on representative states:
  - execution in progress
  - required input missing for actionable command
  - external wait state
  - runtime diagnostic required
  - ready/terminal-like state with no blockers
- assertions for deterministic section ordering and required-row presence

### Stop-Reason Coverage

- fixtures where runtime is waiting for external input
- fixtures where runtime requires diagnostic/reconcile and emits no actionable argv
- assertions that stop reasons are canonical reason codes, deterministic, and non-empty

### Security Rendering Coverage

- fixtures with ANSI/control-sequence payloads in runtime-derived diagnostic and path fields
- fixtures with malformed/injection-like runtime-derived strings in displayable fields
- assertions that doctor text output renders those payloads as inert text with no terminal control effects
- assertions that sanitization does not change JSON payload truth for the same fixture

### Observability Coverage

- fixtures asserting stable text-mode classification markers are present
- fixtures asserting deterministic JSON diagnostic reason-code arrays for non-actionable states
- fixtures asserting optional debug trace fields do not alter route authority or parity assertions
- fixtures asserting dashboard labels remain stable (`Resolution kind`, `Command available`) for operator debugging consistency

### Recovery Surface Coverage

- assert doctor never routes to nonexistent `plan execution recover`
- assert doctor preserves routed existing command families (`repair-review-state`, `close-current-task`, `advance-late-stage`, execution step commands, or operator requery lanes)

### Performance Guard

- coverage asserting doctor path reuses shared operator context and does not add duplicate route queries or extra repo scans
- targeted tests asserting one route-context build per doctor invocation
- targeted tests asserting doctor does not call workflow/operator requery loops internally for a single command execution
- optional benchmark checks may be added as non-blocking diagnostics; merge gating remains on deterministic behavioral assertions

Operational note: rollout is standard branch merge. Rollback is standard merge-commit revert.

## Acceptance Criteria

This spec is complete when all are true:

- `workflow doctor --plan <path>` and `workflow doctor --plan <path> --json` are publicly available and parse-boundary tested
- doctor and operator parity holds for required route fields
- default text mode renders the compact dashboard contract with deterministic section ordering
- headless callers receive deterministic command availability, typed input needs, and stop reasons
- no new recover mutator is added and existing recovery lanes remain authoritative
- compatibility-only workflow commands (`phase`, `handoff`) remain removed

## CEO Review Summary

**Review Status:** clear
**Reviewed At:** 2026-05-04T13:45:57Z
**Review Mode:** hold_scope
**Reviewed Spec Revision:** 1
**Critical Gaps:** 0
**UI Design Intent Required:** no
**Outside Voice:** skipped
