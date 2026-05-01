# FeatureForge Runtime Architecture

FeatureForge runtime state is append-only authority plus derived read models. The normal
public path is:

```text
CLI args
  -> command module
  -> transition guard / typed public command oracle
  -> append-only event recording
  -> reducer
  -> read model/status projection
  -> read-surface invariants
  -> route decision
  -> workflow operator presentation
```

The runtime must not skip layers in that flow. Command modules validate intent and
record events. Reducers and read-model helpers derive status. Routing consumes the
reduced state and produces typed public commands. Workflow presentation renders those
typed decisions for agents and humans.

## Authority Boundaries

`src/execution/commands/*` owns public mutation entrypoints. A command may validate
arguments, load the runtime context, ask shared guards whether the transition is legal,
and append authoritative events. A command that is not explicitly a projection
materializer must not write projection/read-model artifacts.

`src/execution/commands/common.rs` is only a facade for shared command primitives.
Domain-specific support lives under `src/execution/commands/common/`, where bounded
modules separate public flag checks, mutation guards, dispatch lineage, late-stage rerun
equivalence, operator outputs, branch-closure truth, rebuild support, and persistence
helpers.

`src/execution/event_log.rs`, `src/execution/recording.rs`, and
`src/execution/transitions.rs` own event persistence, sequence/hash continuity, and
transition-state persistence. Event append is the authority boundary; `state.json` and
other read models are projections.

`src/execution/reducer.rs` owns conversion from events and current workspace truth into
`RuntimeState`. Reducer code must eliminate impossible state at the source, such as a
current task closure also appearing as stale.

`src/execution/read_model.rs`, `src/execution/read_model_support.rs`, and
`src/execution/status.rs` own projection from reducer truth into public status DTOs.
They may sanitize and explain invalid derived state, but they should not invent routing
truth that bypasses the reducer.

`src/execution/invariants.rs` owns read-surface fail-closed checks. Invariants are
defense in depth; they are not a substitute for reducer correctness.

`src/execution/phase.rs` owns public phase and phase-detail vocabulary. New status
phase-detail strings belong there first so status, operator, tests, and docs do not
create duplicate literals.

`src/execution/state.rs` is a compatibility facade for execution-state operations.
Focused state-machine layers live under `src/execution/state/`: command request
normalization, preflight, runtime methods, review gating, finish gating, artifact
readiness, unit-review proof artifacts, worktree leases, rebuild-evidence discovery, and repo
safety each have their own module. New code belongs in the focused module that owns the
state-machine decision, not in the facade.

## Routing and Public Commands

`src/execution/command_eligibility.rs` defines typed public command objects and
mutation eligibility checks. Hidden/debug commands are not representable as
`PublicCommand` variants.

`src/execution/next_action.rs` and `src/execution/router.rs` decide the next legal
public action from reducer truth, guards, and current review state. They return typed
commands before any display string is rendered.

`src/workflow/operator.rs` presents the route decision. It exposes
`recommended_public_command_argv` for machine invocation and may render
`recommended_command` for human compatibility, but both representations must come from
the typed public command decision, not from reparsing a hand-written string.

`src/workflow/status.rs` owns non-execution workflow routing such as plan-review gates.
For implementation entry, `Engineering Approved` is not enough by itself: a current
passing plan-fidelity review bound to the current plan/spec fingerprints is required.

## Projections and Materialization

Normal `begin`, `complete`, `reopen`, `transfer`, `repair-review-state`,
`close-current-task`, `advance-late-stage`, `plan execution status`, and
`workflow operator` flows must not update tracked plan/evidence projection files.
Runtime read models live under the state directory.

`src/execution/commands/materialize_projections.rs` is the explicit projection export
path. State-dir materialization is allowed for diagnostics. Repo-local projection export
requires the explicit repo-export confirmation flags and is never required for normal
runtime progress.

## Reviewer and Public-Test Boundaries

Reviewer recursion prevention is reviewer-prompt scoped. Review-subagent prompts define
terminal review workers that inspect supplied context and return findings without
spawning or delegating to nested reviewer agents. Runtime command routing does not own
or enforce this agent-recursion policy.

Public replay tests must exercise the compiled public CLI. Internal direct-runtime
helpers belong in quarantined support files with explicit internal naming. Scanner tests
guard this split so public-flow tests cannot pass by importing helper-only runtime
surfaces that real agents cannot call.

## Where To Add Code

- New command behavior: add it under `src/execution/commands/*`, then route it through
  shared guards and append-only recording.
- New transition rule: add it to the guard module or `src/execution/command_eligibility.rs`
  so mutation guards, status, and operator share the same rule.
- New review, finish, preflight, or repo-safety gate: add it to the matching
  `src/execution/state/*` layer and re-export only the stable facade name needed by
  callers.
- New status field: add it to the status/read-model layer and derive it from reducer
  truth or current workspace truth, not from presentation strings.
- New phase detail: add the literal in `src/execution/phase.rs` and update the schema,
  route projection, and tests that consume public phase-detail vocabulary.
- New route presentation: add it to workflow operator/read-model projection only after
  the route decision already exposes the typed public action.
- New projection writer: keep it behind materialize-projections unless it is state-dir
  diagnostic output that normal progress does not require.

## Guardrails

The following suites protect these boundaries:

- `tests/public_cli_flow_contracts.rs`: public tests use the compiled CLI and cannot
  wrap internal helpers, hidden commands, or direct runtime surfaces.
- `tests/runtime_module_boundaries.rs`: import direction, projection writer, phase
  literal, and scanner-centralization contracts.
- `tests/liveness_model_checker.rs`: public paths must either make progress, expose a
  true blocker, emit a deterministic diagnostic, or resolve an `already_current` state
  without stale overlays.
- `tests/public_replay_churn.rs`: known historical loops are replayed through the public
  CLI.
- `tests/runtime_behavior_golden.rs` and `tests/packet_and_schema.rs`: public JSON and
  schema contracts stay explicit when output shape changes.
