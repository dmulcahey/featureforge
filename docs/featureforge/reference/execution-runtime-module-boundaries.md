# Execution Runtime Module Boundaries

This reference records the current modularization boundary for the execution
runtime. It is intentionally narrow: it documents the extracted modules that
must stay focused, and it records why remaining large production execution Rust
files are temporarily allowed to stay large.

## Focused Module Health

Focused-module ownership and import-direction guards are enforced in
`tests/runtime_module_boundaries.rs`. This document is not a per-file line-cap
oracle; it records the ownership intent so reviewers can tell whether a module
is growing inside its boundary or absorbing unrelated decisions.

Use boundary tests as high-signal contract checks, not as private-topology
scanners. New scanner assertions should protect a named audited failure class or
a stable public/import-boundary API: import direction, route-plan ownership,
typed public argv/template projection, hidden-command exclusion, or another
externally visible runtime contract. If a proposed assertion only pins private
helper names, child-module layout, or an arbitrary line-count target, prefer
deleting duplicated decisioning, adding public-output coverage, or documenting
the debt here instead.

Keep these focused ownership families narrow:

- closure dispatch, closure diagnostics, current-closure projection, stale-target projection, stale-target selection, repair-target selection, public repair-target reason vocabulary, and reducer-consumed runtime truth
- route planning, route decision DTOs, route facts, route status projection, public action synthesis, next-action route finalization, final-review dispatch repair, and repair follow-up binding
- route-plan next-action choice families for execution ordering, execution/task-boundary routes, late-stage public routes, late-stage repair routes, late-stage orchestration, and public next-action vocabulary
- command eligibility child modules for public command kind and execution argv target parsing
- repair-route decision child modules for baseline-bridge facts and predicates
- status-assembly child modules for authoritative overlay hydration/parsing, branch gate bindings, blocking-record projection, exact-route validation, typed route surfaces, late-stage projection, review-state projection, task-state projection, and route-neutral facts
- execution-owned late-stage precedence resolution in `src/execution/late_stage_precedence.rs`
- read-model public route projection

`src/execution/state.rs` and `src/execution/mutate.rs` are reduced facades.
They must stay thin compatibility surfaces over execution state/read APIs and
public mutation command modules respectively.

## Reduced Status Assembly Boundary

### `src/execution/status_assembly.rs`

- Status: reduced orchestrator
- Boundary: status construction still coordinates context loading, status
  defaults, route-neutral facts, status-boundary route-field reset, and final
  status overlays. Cohesive child modules own authoritative overlay
  hydration/parsing (`overlay.rs`), current branch gate bindings
  (`branch_gate.rs`), blocking-record projection (`blocking_records.rs`),
  exact route validation, late-stage projection, review-state projection,
  task-state projection, and route-neutral facts.
- Boundary guard: runtime truth and reducer may consume status assembly, but
  neither may import read-model presentation. Status assembly and its child
  modules consume lower execution/status helpers and must not import route
  selection, read-model presentation, workflow presentation, or mutation
  command modules.
- Revisit trigger: extract another child module only when a cohesive status
  responsibility becomes independently nameable; do not split by arbitrary line
  count.

### `src/execution/late_stage_precedence.rs`

- Status: focused owner
- Boundary: owns the late-stage release/review/QA precedence rows and resolver
  consumed by execution status assembly and workflow presentation. Workflow code
  may present the selected lane, but it must not carry a second precedence
  matrix.
- Boundary guard: execution status assembly imports this execution-owned resolver
  directly. Workflow presentation must consume execution-derived phase/action
  surfaces instead of re-deriving the precedence order.

## Workflow Presentation Module Debt

`src/workflow/status.rs` and `src/workflow/operator.rs` are large presentation
modules. Their size is tracked debt, but it is not itself execution
split-decisioning: these modules should format and package already-derived
workflow/runtime decisions, not decide mutation eligibility, route ordering,
repair targets, or public command authority.

- Boundary: workflow presentation must consume shared execution DTOs, route
  decisions, typed public command argv/templates, and diagnostic vocabularies.
  It may choose display wording and JSON shape, but it must not recompute the
  semantic question of which public mutation is legal.
- Boundary guard: status/operator code must not import mutation command modules,
  call lower-level write helpers, parse display command strings as authority, or
  fork the public-route decision path. When wording needs the executable route
  contract, keep the detailed law in `references/operator-route-authority.md`
  and link or summarize it from high-use surfaces instead of duplicating route
  law.
- Test guard shape: prefer public-output, public-argv/template, and historical
  dead-end replay coverage. Avoid scanner-only assertions for incidental
  workflow presentation topology unless they protect a concrete audited failure
  class such as presentation reaching into mutation internals or leaking hidden
  commands.
- Revisit trigger: split these modules only when a cohesive presentation family
  becomes nameable, or when review finds duplicated semantic decisioning that
  should move behind an existing shared execution helper.

## Large Module Threshold

Production Rust files under `src/execution/` above 2000 lines must appear below
with either `Status: documented exception` or `Status: scheduled follow-up`.
This threshold is a visibility mechanism, not semantic safety proof. A listed
module is acceptable only when its boundary guard still prevents duplicated
routing/status/mutation authority; the line-count listing alone does not prove
the module is cohesive or safe.

### `src/execution/transitions.rs`

- Status: documented exception
- Why exception: transition application is the runtime-owned state mutation
  ledger and contains intentionally data-heavy transition reducers.
- Boundary guard: command modules must reach transition writes through the
  recording and command persistence boundaries, not direct transition
  primitives.
- Revisit trigger: extract only when a coherent transition family can move
  without splitting authoritative mutation ordering.

### `src/execution/event_log.rs`

- Status: documented exception
- Why exception: event log migration, validation, and append-only replay are
  one authoritative storage boundary.
- Boundary guard: migration parity checks must preserve event-log authority and
  must not publish partial events on failed parity.
- Revisit trigger: split only by stable event-family readers or validators, not
  by arbitrary line count.

### `src/execution/review_state.rs`

- Status: scheduled follow-up
- Follow-up: separate repair-plan construction, reconcile output projection,
  and public follow-up surface assembly behind smaller modules.
- Boundary guard: review-state repair must continue consuming the recording
  boundary for overlay restoration instead of loading transition state or
  writing transition primitives directly.

### `src/execution/context.rs`

- Status: documented exception
- Why exception: execution context loading normalizes plan, evidence, repo, and
  runtime-root inputs at one trust boundary.
- Boundary guard: stale or tampered read-model files must not become mutation
  authority through context loading.
- Revisit trigger: split only around a validated input boundary such as repo
  context resolution or runtime-root discovery.

### `src/execution/next_action.rs`

- Status: reduced facade
- Boundary: exposes stable next-action display labels, `NextActionDecision`,
  `NextActionKind`, and display/projection helpers. Ordered route candidate
  computation lives under `src/execution/route_plan/next_action_choice*`;
  modules outside `src/execution/route_plan/` must consume route-plan command or
  route projection helpers instead of calling route candidate computation.
- Boundary guard: repair/reopen public commands, typed argv/templates, and
  exact public next-action construction must not be reconstructed in
  `next_action.rs`, router, or command modules.

### `src/execution/route_plan/route_semantics.rs`

- Status: route-plan owner
- Boundary: owns public blocking scope/task projection, route phase
  normalization, and external wait-state derivation for finalized public routes.
  Query/read-model layers may expose immutable facts consumed by this module, but
  they must not own final route-control fields.
- Boundary guard: callers outside route planning should consume finalized
  `RouteDecision` or status/operator projections rather than calling these
  helpers to revise selected routes.

### `src/execution/status_assembly/exact_route.rs`

- Status: route-neutral validator
- Boundary: validates finalized exact execution-route fields already projected
  by route-plan. It may coordinate execution context, typed argv/template
  presence, and typed command/context consistency. Typed argv parsing belongs to
  `src/execution/status_assembly/exact_route_surfaces.rs` with executable argv
  shape supplied by `src/execution/command_eligibility/execution_target.rs`;
  template binding validation belongs to
  `src/execution/status_assembly/exact_route_template.rs`, while complete-command
  verification-mode bindability is owned by
  `src/execution/command_eligibility/execution_target.rs`. None of these modules
  may infer route necessity from raw task, resume, evidence, harness, or
  authoritative-sequence state.
- Boundary guard: exact-route validation may consume `PublicCommandKind` token
  helpers but must not import route-plan, router, next-action candidate, repair
  target, stale-target, or transition-state selectors.

### `src/execution/route_plan/next_action_choice.rs`

- Status: route-plan child owner
- Boundary: orchestrates the ordered next-action candidate pass inside route
  planning. Cohesive route families live below
  `src/execution/route_plan/next_action_choice/`: `types.rs` owns display
  vocabulary and DTOs, `execution_ordering.rs` owns execution open-step and
  stale-boundary ordering, `execution_routes.rs` owns execution/task-boundary
  decision constructors, `late_stage_public_routes.rs` owns late-stage public
  milestone selection, `late_stage_repair_routes.rs` owns late-stage repair and
  branch-rerecording decisions, and `late_stage_routes.rs` owns late-stage
  family orchestration plus handoff/planning overrides. These modules may
  consume reducer/status facts, gate-derived authority inputs, stale-target
  selectors, and repair-route decisions, but must not bind public commands or
  project status DTOs. `RoutePlanningFacts` must stay raw immutable input facts
  and must not carry a preselected `NextActionDecision`.
- Boundary guard: route finalization belongs to
  `src/execution/route_plan/next_action_route.rs` and
  `src/execution/route_plan/next_action_finalization.rs`; the route-family
  modules are covered by import-direction and ownership guards so no child
  becomes a replacement monolith by recomputing public-route truth.

### `src/execution/route_plan.rs`

- Status: focused owner
- Boundary: route planning owns the single public runtime route-choice pass.
  It builds `RoutePlanningFacts` before route selection, arbitrates any
  competing route candidates there (including persisted execution-reentry and
  baseline-bridge repair candidates), and returns a selected `RouteDecision`
  plus the route-plan-owned status projection after route-plan-internal
  finalization. Router/status projection installs that projection; it must not
  rebuild blocker records, replace the selected route, or call a
  `RouteDecision -> RouteDecision` finalizer. Read-surface invariants may
  install only a route-plan-owned diagnostic decision after clearing executable
  status surfaces.
- Boundary guard: `src/execution/router.rs` must not call a post-status route
  revision hook, duplicate route-to-status projection, or import projection
  internals. `status_projection.rs` must not import route constructors or
  stale-target selectors.
- Test guard shape: module-boundary tests should pin the `plan_runtime_route`
  entrypoint, route decision DTOs, import direction, selected-route projection
  behavior, and one-pass `RoutePlanningFacts` construction. Avoid asserting line
  caps, child-module names, or private route-plan helper names unless a helper
  name is itself the boundary API consumed by another module.

### `src/execution/route_plan/execution_targets.rs`

- Status: focused owner
- Boundary: owns raw status/context to execution-command route target selection
  (`begin`, `complete`, `reopen`) and the public-status matching checks for
  those targets. `status_assembly` may validate finalized typed route fields,
  but must not define, re-export, or call these route-choice helpers.
- Boundary guard: route-target helpers should construct targets with
  `PublicCommandKind`, not ad hoc command-token strings. Ownership/import guards
  should keep command-token and status-matching logic from becoming a new
  catch-all route module.

### `src/execution/authority.rs`

- Status: documented exception
- Why exception: authority parsing and artifact identity validation are a
  security boundary and currently share one failure taxonomy.
- Boundary guard: authority helpers must remain fail-closed for forged or
  non-runtime-owned artifact paths.
- Revisit trigger: split only around stable artifact families with shared
  validation helpers left centralized.

### `src/execution/gates.rs`

- Status: documented exception
- Why exception: gate remediation text, gate record validation, and public
  operator-route recovery share the same fail-closed diagnostic boundary.
- Boundary guard: gate diagnostics must continue rendering the shared typed
  operator-route remediation instead of reconstructing artifact-repair commands
  or hidden gate helpers locally.
- Revisit trigger: split only when a cohesive gate family can move behind the
  same public-remediation helper without duplicating route wording or
  proof-artifact validation.

### `src/execution/commands/advance_late_stage.rs`

- Status: scheduled follow-up
- Follow-up: extract cohesive late-stage recording families such as
  release-readiness, final-review, QA, and branch-finish support only when each
  extraction can keep the command as the single public intent-level mutation
  owner and avoid duplicating route eligibility, typed input binding, or summary
  validation.
- Boundary guard: late-stage public progression must continue routing through
  `advance-late-stage` and shared command eligibility/public-route surfaces;
  child extraction must not reintroduce low-level recorder command authority or
  parallel branch-closure decisioning.

### `src/execution/current_truth.rs`

- Status: scheduled follow-up
- Follow-up: late-stage freshness reason-code vocabulary is centralized in
  `review_route_tokens.rs`; continue extracting branch rerecording and negative
  result follow-up helpers only when each can keep one authoritative owner.
- Boundary guard: current/stale and reroute truth must converge across status,
  operator, repair, and mutation eligibility surfaces.

### `src/execution/projection_renderer.rs`

- Status: documented exception
- Why exception: projection materialization owns runtime-generated artifact
  rendering and write safety for projection files.
- Boundary guard: normal command modules must not bypass materialize-projection
  behavior to write projection read models directly.
- Revisit trigger: split only after a projection family has an isolated writer
  API and matching path-safety tests.
