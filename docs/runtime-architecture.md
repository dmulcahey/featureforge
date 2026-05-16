# FeatureForge Runtime Architecture

FeatureForge runtime state is append-only authority plus derived read models. The normal
public path is:

```text
CLI args
  -> command module
  -> transition guard / typed public command oracle
  -> append-only event recording
  -> reducer
  -> route-plan selection plus route-owned status projection
  -> router/read-model/status installation
  -> read-surface invariants
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
current task closure also appearing as stale. Status assembly may return route-neutral
facts such as stale-target projection and review-state diagnostics for reducer reuse,
but public route fields stay empty until the route projection layer applies a
`RouteDecision`.

`src/execution/status_support.rs` owns shared execution-status helpers consumed by
status assembly, read-model presentation, and runtime truth. Call sites must import
that owner directly; the old `read_model_support.rs` compatibility re-export has been
removed. `src/execution/status_assembly.rs` owns route-neutral status construction and
diagnostics; `src/execution/status_assembly/facts.rs` names the intermediate facts
that reducer/route planning may reuse without reading public route fields as authority.
`src/execution/status_assembly/exact_route.rs` validates finalized exact execution
route fields only, with typed surface parsing split between
`src/execution/status_assembly/exact_route_surfaces.rs`,
`src/execution/status_assembly/exact_route_template.rs`, and the executable
argv/template bindability policy in
`src/execution/command_eligibility/execution_target.rs`; none of these modules may
infer whether an execution route is required from raw task/resume/context state.
Raw execution-command route-target selection and target/status matching
live under `src/execution/route_plan/execution_targets.rs`, using
`PublicCommandKind` for command-token ownership.
`src/execution/read_model.rs` and `src/execution/status.rs` own projection from reducer
truth into public status DTOs. They may sanitize and explain invalid derived state,
but they should not invent routing truth that bypasses the reducer.

## Installed Control Plane Diagnostics

Live FeatureForge workflow control-plane execution is the installed runtime at
`~/.featureforge/install/bin/featureforge` using installed skills from
`~/.featureforge/install/skills`. Workspace binaries such as `./bin/featureforge` or
`target/debug/featureforge` are test subjects only and may be used with isolated temp
state in fixture and smoke tests. Workspace skills under `<repo>/skills/*` are
generated product artifacts under test, not active instruction roots.

Agents can inspect the active boundary with:

```bash
featureforge doctor self-hosting --json
```

The command is read-only. Its JSON reports installed, invoked, and workspace runtime
paths and hashes; active skill root classification; state-dir kind; repository context;
whether live mutation is allowed by the workspace-runtime guard; warnings; and the
recommended remediation. Review and evidence tooling should use this diagnostic when
checking installed-vs-workspace separation for FeatureForge-on-FeatureForge work.

Workspace-runtime live mutation is blocked by default. The only override is
`FEATUREFORGE_ALLOW_WORKSPACE_RUNTIME_LIVE_MUTATION=1`; it is intentionally
explicit, must be recorded in evidence/review provenance, and should almost
never be used.

`src/execution/invariants.rs` owns read-surface fail-closed checks. Invariants are
defense in depth; they are not a substitute for reducer correctness, and they must
not reconstruct normal route authority after route-plan finalization. If invariants
mark a public read surface diagnostic, they clear executable surfaces on the status
projection and may install only a route-plan-owned diagnostic decision.

`src/execution/phase.rs` owns public phase and phase-detail vocabulary. New status
phase-detail strings belong there first so status, operator, tests, and docs do not
create duplicate literals.

`src/execution/state.rs` is a compatibility facade for execution-state operations.
Focused state-machine layers live under `src/execution/state/`: command request
normalization, preflight, runtime methods, review gating, finish gating, artifact
readiness, unit-review proof artifacts, worktree leases, projection-rebuild discovery, and repo
safety each have their own module. New code belongs in the focused module that owns the
state-machine decision, not in the facade.

## Routing and Public Commands

`src/execution/command_eligibility.rs` defines typed public command objects and
mutation eligibility checks. Hidden/debug commands are not representable as
`PublicCommand` variants. Public mutation CLI tokens are owned by
`PublicCommandKind`; mutation request kinds derive their command names from that
typed owner instead of maintaining a second string table.

`src/execution/next_action.rs` is a display/type facade for stable next-action
labels and the typed candidate DTO. Ordered next-action candidate computation
lives under `src/execution/route_plan/next_action_choice.rs` and its focused
child modules; it is consumed only by route-plan selection/finalization. Status
assembly, router, and command modules must consume route-plan route or command
projections rather than calling candidate computation as a second route owner.
The retired `public_route_selection` module has been deleted; do not recreate it
as a marker or compatibility staging area.
`src/execution/route_plan.rs` owns final route-plan ordering from reducer truth,
guards, and current review state. Route-decision DTOs, route constructors,
next-action route finalization, blocker materialization, state-kind classification,
follow-up derivation, and route fact helpers live under `src/execution/route_plan/`;
`src/execution/router.rs` installs the selected route and the route-plan-owned status
projection into status/operator DTO surfaces. Route planning returns typed commands
before any display string is rendered.
Public blocking scope/task projection, route phase normalization, and external
wait-state derivation live in `src/execution/route_plan/route_semantics.rs`.
Read/query layers may provide immutable facts, but they must not own those final
route-control fields.
Shared route-to-status field assignment, phase-to-harness mapping, and
projection diagnostics live in `src/execution/route_plan/status_application.rs`
and the final status projection lives under `src/execution/route_plan/`; router and
read-model projection must consume that output rather than recomputing blockers or
route-to-status fields.
Route planning computes `RoutePlanningFacts` before public route selection, so
targetless stale, baseline bridge, persisted follow-up, completed-closure
preemption, and shared next-action decisions are selected in one pass.
Route-plan finalization may derive blocker-dependent `required_follow_up`,
normalize diagnostic-only routes, bind public repair targets, and produce the status
projection before the router sees the decision. Status projection copies
selected-route metadata such as `execution_reentry_target_source`; it must not
rederive stale targets, replace the `RouteDecision`, or mutate route-control fields
after the router has projected the selected route.

`src/workflow/operator.rs` presents the route decision. Its JSON output exposes
`recommended_public_command_argv` for machine invocation and
`recommended_public_command_template` plus `required_inputs` for input-required
routes. It may render `recommended_command` for human compatibility, but all three
representations must come from the typed public command decision, not from reparsing
a hand-written string.
When `recommended_public_command_argv` is present, consumers execute the typed
public route through the installed control-plane runtime. Detailed argv binding,
operator-mediated template materialization, and rebinding rules live in
`references/operator-route-authority.md`.
`recommended_command` is display-only compatibility text and must not be parsed or
split to recover argv.
When a diagnostic-only route has no argv and no typed inputs,
`next_action=runtime diagnostic required`; consumers stop on that diagnostic and
must not invent a repair/reentry command or manually edit runtime artifacts.
`advance-late-stage` is the public late-stage intent. Its serialized output exposes
`intent=advance_late_stage`, `stage_path`, and a semantic `operation`; it must not
publish hidden stage primitive command names as control-plane guidance.

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

- `tests/public_cli_flow_contracts.rs`: public tests use the compiled CLI,
  public route goldens, and JSON schema semantics; they cannot wrap internal
  helpers, hidden commands, or direct runtime/query surfaces unless a named
  internal semantic boundary test has an explicit scanner exception.
- `tests/public_flow_scan_contracts.rs`: focused scanner fixtures for the public-flow
  helper/hidden-command guards consumed by public contract tests. Internal
  semantic and synthetic setup exceptions are marker based (`internal_semantic_`
  and `synthetic_`) so the suite does not grow a per-helper allowlist.
- `tests/runtime_module_boundaries.rs`: import direction, projection writer,
  phase literal, module ownership, and scanner-centralization boundary
  contracts. It avoids line caps, child-module name pins, and exact private
  helper-name pins except where a named boundary-owner entrypoint is itself the
  contract; route-plan coverage should prefer owner modules, DTO/public
  entrypoints, route-fact construction, and projection behavior over private
  helper spellings.
- `tests/rust_source_scan_contracts.rs`: focused scanner fixtures for import,
  macro, and phase-detail parser behavior used by boundary tests.
- `tests/liveness_model_checker.rs`: an internal semantic/liveness matrix checks that
  route states either make progress, expose a true blocker, emit a deterministic
  diagnostic, or resolve an `already_current` state without stale overlays. It
  keeps subprocess cost bounded with a targeted compiled-CLI parity edge rather
  than treating the full matrix as shipped-CLI proof.
- `tests/public_replay_churn.rs`: known historical loops are replayed through the public
  CLI.
- `tests/runtime_behavior_golden.rs` and `tests/packet_and_schema.rs`: public JSON and
  schema contracts stay explicit when output shape changes.
