# FeatureForge Runtime Units Of Work

**Date:** 2026-04-01  
**Source:** `2026-04-01-featureforge-runtime-forensic-findings.md`

## Purpose

This document repackages the runtime audit into implementation-sized units under the supersession-aware architecture:

- current reviewed closure is authoritative
- older reviewed closures can be superseded
- stale unreviewed changes are surfaced explicitly
- markdown receipts are derived artifacts, not the primary gate truth

## Sequencing

Recommended order:

1. `U1` Supersession-aware review identity core model
2. `U2` Task closure recording on current reviewed closures
3. `U3` Branch-closure recording on current reviewed branch state
4. `U8` Gate diagnostics and runtime semantics
5. `U9` Workflow public phase and routing contract
6. `U7` Supersession-aware reconcile and stale-closure repair
7. `U4` Release-readiness recording on current reviewed branch closures
8. `U5` Final-review recording on current reviewed branch closures
9. `U6` QA recording on current reviewed branch closures
10. `U10` Skill and reference hardening
11. `U11` Runtime-path coverage and doc-contract rehab
12. `U12` Runtime boundary separation

Safe parallelism:

- `U10` should wait until `U1` through `U9` freeze commands and public semantics.
- `U12` should wait until `U11` lands and behavioral coverage exists.

## Unit Breakdown

### U1. Supersession-aware review identity core model

**Why it exists**

The old model treats too much per-attempt proof as if it remains authoritative forever. That is why later reviewed work turns into repair churn instead of clean supersession.

**Primary output**

One runtime-owned model for:

- current reviewed closure
- superseded closure
- stale-unreviewed closure
- historical closure

**Deliverables**

- runtime-owned closure record schema for task and branch scopes
- pure reviewed-closure domain model with no file system or CLI dependencies
- reviewed-state, reviewed-surface, and contract-input resolution boundaries
- dedicated store/projection boundary for authoritative closure persistence
- pure supersession / stale-unreviewed evaluator
- append-only lineage model
- effective-current-closure computation
- explicit dependency rules between domain, resolvers, stores, projections, services, query layer, workflow adapter, and renderers

**Dependency profile**

- first and foundational
- every other unit depends on this model

### U2. Task closure recording on current reviewed closures

**Why it exists**

Task advancement should depend on a current reviewed task closure, not on repairing old packet/file proof and hand-authored receipts.

**Primary output**

A public runtime command surface that records task closure as a current reviewed closure and optionally emits human-readable derivatives.

**Deliverables**

- public CLI for task closure recording
- stable internal task-closure recording service boundary
- preferred aggregate `close-current-task`
- explicit `record-review-dispatch --scope task --task <n>` contract and return shape
- runtime-owned recording of task-review and task-verification milestones
- automatic supersession of older overlapping task closures
- structured blocked/current/superseded/stale results

**Dependency profile**

- depends on `U1`
- should land before repair and skill work

### U3. Branch-closure recording on current reviewed branch state

**Why it exists**

Late-stage work depends on a current reviewed branch closure. That producer path cannot stay implicit.

**Primary output**

A public runtime command surface that records authoritative branch closure and returns deterministic recorded/already-current/blocked results.

**Deliverables**

- public `record-branch-closure` command contract
- dedicated `BranchClosureService`
- runtime-owned branch-closure record schema and return shape
- idempotent re-run behavior
- blocked validation before mutation
- stale and superseded branch-closure semantics

**Dependency profile**

- depends on `U1`
- blocks every late-stage milestone slice

### U4. Release-readiness recording on current reviewed branch closures

**Why it exists**

Release-readiness should bind to current reviewed branch truth, not to hand-authored markdown later validated back into truth.

**Primary output**

A runtime-owned release-readiness milestone record for the current reviewed branch closure, with optional human-readable artifact generation.

**Deliverables**

- `record-release-readiness` primitive
- preferred aggregate `advance-late-stage` contract for `document_release_pending`
- runtime-owned release-readiness record bound to current branch closure
- blocked-result semantics
- stale-after-edit semantics

**Dependency profile**

- depends on `U1`, `U3`, `U7`, `U8`, and `U9`
- must land before `U5`

### U5. Final-review recording on current reviewed branch closures

**Why it exists**

Final review should record an independent pass over the current reviewed branch closure, not keep paired markdown artifacts as the main truth surface.

**Primary output**

A runtime-owned final-review milestone record for the current reviewed branch closure, with optional human-readable artifact generation.

**Deliverables**

- `record-final-review` primitive
- preferred aggregate `advance-late-stage` contract for `final_review_pending`
- explicit final-review dispatch dependency
- runtime-owned final-review record bound to current branch closure
- derived public and dedicated review artifacts when needed
- fail-result execution-reentry semantics

**Dependency profile**

- depends on `U1`, `U2`, `U3`, `U4`, `U7`, `U8`, and `U9`
- must land after release-readiness because final review is only valid once a current release-readiness result `ready` already exists for the same still-current branch closure

### U6. QA recording on current reviewed branch closures

**Why it exists**

`qa_pending` is part of the active public workflow. QA cannot stay a prose placeholder or an implied side effect of some other terminal-stage command.

**Primary output**

A first-class runtime-owned QA milestone record bound to the current reviewed branch closure.

**Deliverables**

- `record-qa` command contract
- pass/fail QA result semantics
- stale-after-edit and historical QA behavior
- explicit execution-reentry follow-up for failed QA
- workflow-facing contract for `qa_pending`

**Dependency profile**

- depends on `U1`, `U3`, `U4`, `U5`, `U7`, `U8`, and `U9`
- should land only after gate and workflow public-contract shapes are frozen and once release-readiness and final-review milestone contracts are already stable

### U7. Supersession-aware reconcile and stale-closure repair

**Why it exists**

Once current reviewed closures become authoritative, repair should stop rewriting old proof and instead reconcile the current closure graph.

**Primary output**

Public flows that explain stale review state, reconcile derived overlays, and return the exact next recording action without minting new closure truth implicitly.

**Deliverables**

- public explain and reconcile primitives
- preferred aggregate `repair-review-state`
- append-only supersession flows instead of in-place proof rewriting
- supported handling for stale-unreviewed task and branch state
- no-manual-edit recovery path

**Dependency profile**

- depends on `U1`, `U2`, `U3`, `U8`, and `U9`
- must land before `U4`, `U5`, and `U6` because stale-late-stage reroute and recovery are already part of those contracts

### U8. Gate diagnostics and runtime semantics

**Why it exists**

The new model only helps if the runtime clearly tells operators what is current, what was superseded, what is stale, and what exact command comes next.

**Primary output**

Clear gate/status semantics for workspace state, current reviewed state, branch closure state, dispatch readiness, milestone state, and deterministic `recommended_command`.

**Deliverables**

- expected-versus-observed gate payloads
- explicit current reviewed state and current branch-closure fields in status
- explicit stale-unreviewed and superseded diagnostics
- deterministic singular `recommended_command`
- parity between `gate-review` and `gate-finish`

**Dependency profile**

- depends on `U1` through `U3`

### U9. Workflow public phase and routing contract

**Why it exists**

Workflow outputs need one coherent public contract for when the operator should keep executing, repair review state, record branch closure, dispatch final review, run QA, or finish the branch.

**Primary output**

One authoritative public phase/routing vocabulary aligned to current/superseded/stale closure state.

**Deliverables**

- documented public phase inventory
- explicit `phase_detail` inventory
- exact `next_action` enum families
- deterministic `recommended_command` contract
- explicit stale late-stage reentry mappings
- canonical QA routing contract

**Dependency profile**

- depends on `U8`

### U10. Skill and reference hardening

**Why it exists**

Agents need to understand current versus superseded versus stale closure state and the runtime-owned commands that manage it.

**Primary output**

Operator-facing guidance that matches the new reviewed-closure model exactly.

**Deliverables**

- updated workflow skills
- explicit no-manual-edit guidance for runtime-owned records and derived artifacts
- examples for task closure, supersession, stale review-state repair, branch closure, release-readiness, final review, and QA
- command matrices that prefer `close-current-task`, `repair-review-state`, `record-branch-closure`, `record-review-dispatch`, `advance-late-stage`, and `record-qa`
- linked shared references instead of re-teaching the core model inconsistently

**Dependency profile**

- depends on `U1` through `U9`

### U11. Runtime-path coverage and doc-contract rehab

**Why it exists**

The new model will fail if the test suite keeps validating phrases and fixtures instead of real reviewed-closure behavior.

**Primary output**

Behavioral coverage for the supersession-aware model.

**Deliverables**

- layered tests for domain policy, store/projection, services, workflow routing, and CLI e2e
- aggregate-command tests for task close, review-state repair, branch closure, late-stage advance, and QA
- CLI-only end-to-end tests for supersession and stale-unreviewed transitions
- CLI-only end-to-end tests for release-readiness, final review, and QA recording
- doc tests that assert semantics instead of frozen prose

**Dependency profile**

- depends on `U1` through `U10`

### U12. Runtime boundary separation

**Why it exists**

The new architecture needs clear ownership or it will devolve back into scattered proof logic and doc drift.

**Primary output**

Clear module ownership for closure records, supersession, milestones, gates, repair, and routing.

**Deliverables**

- extracted reviewed-closure domain modules
- extracted resolver, store, and projection modules
- extracted task, branch, release-readiness, final-review, and QA recording services
- extracted gate/status query modules
- extracted workflow-facing read model and adapter modules
- extracted renderer/parsers behind a compatibility boundary
- documented forbidden dependency directions between layers

**Dependency profile**

- last, after behavioral coverage exists

## Unit Boundaries

Avoid these bundles:

- do not combine `U1` with command-surface implementation work; the core model needs to stabilize first
- do not combine `U7` with `U11`; repair semantics must be specified before coverage is rewritten around them
- do not start `U12` before `U11`; refactor without behavioral oracles will hide regressions

## Packaging Guidance

One architectural umbrella spec can work here.

In fact, `U1` is the right place for it.

The right packaging is:

- one umbrella superspec for the reviewed-closure model and its component boundaries
- downstream slice specs for task closure, branch closure, branch milestones, reconcile, semantics, workflow, skills, tests, and refactor

One undifferentiated super plan is still a worse idea.

The dependency graph is too real:

- the model must stabilize before command surfaces
- command/state semantics must stabilize before skills
- behavioral coverage must stabilize before refactor

So the recommended planning shape is:

- one superspec
- one program-level roadmap or phased plan that references the slice specs
- one implementation plan per delivery slice or tightly coupled phase
