# Execution Runtime Module Boundaries

This reference records the current modularization boundary for the execution
runtime. It is intentionally narrow: it documents the extracted modules that
must stay focused, and it records why remaining large top-level execution
modules are temporarily allowed to stay large.

## Focused Module Caps

These modules were extracted to own one runtime decision or projection family.
They have explicit line caps so they cannot quietly become the next catch-all
module.

| Module | Cap | Boundary |
| --- | ---: | --- |
| `src/execution/closure_dispatch.rs` | 720 | current closure dispatch authority |
| `src/execution/closure_dispatch_mutation.rs` | 160 | review-dispatch mutation and bootstrap |
| `src/execution/closure_diagnostics.rs` | 520 | artifact/projection diagnostic reason classification |
| `src/execution/current_closure_projection.rs` | 450 | current task-closure DTO and reason projection |
| `src/execution/stale_target_projection.rs` | 850 | stale target and stale closure projection |
| `src/execution/repair_target_selection.rs` | 450 | execution reentry and repair target selection |
| `src/execution/late_stage_route_selection.rs` | 350 | late-stage public route selection |
| `src/execution/public_route_selection.rs` | 400 | public next-action route seed projection |
| `src/execution/read_model/late_stage.rs` | 400 | read-model late-stage precedence projection |
| `src/execution/read_model/public_route_projection.rs` | 700 | read-model public route DTO projection |
| `src/execution/read_model/review_state.rs` | 260 | read-model review-state authority projection |
| `src/execution/read_model/task_state.rs` | 500 | read-model task-boundary and exact-command projection |

## Reduced Facade Caps

These facades were already reduced and must stay thin. They have explicit caps
even though they are below the large-module threshold, because they are common
places for unrelated imports and compatibility re-exports to accumulate.

| Module | Cap | Boundary |
| --- | ---: | --- |
| `src/execution/state.rs` | 350 | compatibility facade over execution state/read APIs |
| `src/execution/mutate.rs` | 80 | compatibility facade over public mutation command modules |

## Large Module Threshold

Top-level `src/execution/*.rs` files above 2000 lines must appear below with
either `Status: documented exception` or `Status: scheduled follow-up`.

### `src/execution/transitions.rs`

- Status: documented exception
- Why exception: transition application is the runtime-owned state mutation
  ledger and contains intentionally data-heavy transition reducers.
- Boundary guard: command modules must reach transition writes through the
  recording and command persistence boundaries, not direct transition
  primitives.
- Revisit trigger: extract only when a coherent transition family can move
  without splitting authoritative mutation ordering.

### `src/execution/read_model.rs`

- Status: scheduled follow-up
- Follow-up: task-state, review-state, late-stage, and public-route projection
  families have focused child modules. Continue extracting public blocking
  record projection and status overlay hydration once those families have stable
  ownership boundaries.
- Boundary guard: read-model modules must not import mutation commands, append
  events, or write projection files directly.

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

- Status: scheduled follow-up
- Follow-up: keep moving decision families out as focused modules once a
  single owner exists for each rule family; public-command construction remains
  here until a dedicated command-construction owner is introduced.
- Boundary guard: repair/reopen public commands and exact public next-action
  construction must not be reconstructed in router or command modules.

### `src/execution/authority.rs`

- Status: documented exception
- Why exception: authority parsing and artifact identity validation are a
  security boundary and currently share one failure taxonomy.
- Boundary guard: authority helpers must remain fail-closed for forged or
  non-runtime-owned artifact paths.
- Revisit trigger: split only around stable artifact families with shared
  validation helpers left centralized.

### `src/execution/current_truth.rs`

- Status: scheduled follow-up
- Follow-up: extract late-stage freshness, branch rerecording, and negative
  result follow-up helpers when each can keep one authoritative owner.
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

### `src/execution/router.rs`

- Status: scheduled follow-up
- Follow-up: keep reducing router into DTO assembly and delegation as route
  families gain focused owners.
- Boundary guard: router must delegate public next-action seed selection and
  must not reconstruct public command strings or reopen/repair commands.
