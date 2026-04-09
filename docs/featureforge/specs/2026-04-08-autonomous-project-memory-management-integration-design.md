# Autonomous Project Memory Management Integration

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

**Status:** Proposed  
**Target Branch:** `dm/spec-backlog`  
**Scope:** skill-library and contract-test changes only  
**Primary Objective:** Make project memory autonomously managed by the agent without turning memory into a workflow stage, runtime gate, or second source of truth.

## Summary

FeatureForge already ships a bounded project-memory layer under `docs/project_notes/`, but usage is still inconsistent and mostly opt-in.

This spec standardizes autonomous behavior:

- consult project memory early only where it reduces rediscovery
- capture durable truth in authoritative artifacts during implementation and review work
- batch most project-memory writes into one non-blocking sweep in `featureforge:document-release`
- keep `featureforge:project-memory` as the only writer for repo-visible project memory
- prohibit routine post-terminal-review memory edits

The intent is supportive memory, not shadow workflow state.

## Problem Statement

Current integration has four failure modes:

1. inconsistent read usage across skills
2. durable findings can be lost when no one remembers to preserve them
3. opportunistic write-through from many skills increases churn and blurs authority
4. late memory edits after terminal review create stale-review loops

The system needs one coherent model for: who reads memory, who writes memory, when writing is allowed, and what must never become memory.

## Goals

1. Make project memory autonomous in normal workflow-routed work.
2. Preserve authority order: approved specs/plans/evidence/reviews/runtime state/instructions remain above project memory.
3. Keep memory low-churn and reviewable.
4. Ensure final review sees memory edits on the same `HEAD`.
5. Protect reviewer independence in approval and final-review skills.
6. Avoid adding runtime stages, helper commands, or completion gates.

## Non-Goals

This spec does not:

- elevate project memory to workflow truth
- require memory updates for planning, execution, review, or finish
- allow `issues.md` to become a live tracker
- allow execution or review subagents to directly edit `docs/project_notes/*`
- make `AGENTS.md` a routine autonomous memory target
- add workflow stage/helper/gate machinery for memory
- permit routine post-final-review memory edits in `featureforge:finishing-a-development-branch`

## Authority And Invariants

### 1) Authority order remains unchanged

When artifacts disagree, precedence is:

1. approved specs
2. approved plans
3. execution evidence and review artifacts tied to approved work
4. stable repo docs and runtime-owned workflow state
5. `docs/project_notes/*`

Project memory may summarize and backlink; it must not override upstream authority.

### 2) Single memory writer

`featureforge:project-memory` remains the only skill allowed to write repo-visible project memory.

Other skills may only:

- consult project memory
- nominate durable candidate takeaways
- invoke `featureforge:project-memory` only when designated owner rules allow it

### 3) Memory remains non-blocking

Memory quality may improve branch quality, but memory updates must never block:

- plan approval
- execution handoff
- review dispatch
- finish gating
- branch completion

### 4) Batch over scatter

Default behavior is one late, non-blocking sweep instead of many distributed writes.

### 5) Review independence

Approval/fidelity/final-review skills remain memory-independent by default.

### 6) AGENTS edits are explicit-only

Autonomous memory management must not mutate `AGENTS.md`.
`AGENTS.md` updates require explicit user intent or explicit repo-maintenance scope.

## Operating Model

Normal lifecycle:

1. `using-featureforge`
2. `brainstorming` (optional consult)
3. `plan-ceo-review`
4. `writing-plans` (consult)
5. `plan-fidelity-review`
6. `plan-eng-review`
7. execution (`executing-plans` / `subagent-driven-development`) captures authoritative sources only
8. `systematic-debugging` consults and may use narrow recurring-bug write-through
9. `document-release` performs default zero-or-one non-blocking memory sweep
10. terminal `requesting-code-review` reviews that same `HEAD`
11. optional `qa-only`
12. `finishing-a-development-branch` with no new autonomous memory writes

Rule of thumb:

- read early when useful
- write once, late, through `featureforge:project-memory`
- do not write after terminal final review unless user explicitly reopens work

## Requirement Index

- [REQ-001][authority] `featureforge:project-memory` is the only writer to `docs/project_notes/*`.
- [REQ-002][authority] Autonomous default write scope narrows to `docs/project_notes/*`; `AGENTS.md` is explicit-only.
- [REQ-003][authority] Skill docs must explicitly reject autonomous `AGENTS.md` updates and require explicit intent/scope.
- [REQ-004][read] `brainstorming` adds optional consult of `decisions.md` and `key_facts.md` for cross-cutting/architecture-shaping work.
- [REQ-005][read] `brainstorming` consult is supportive, non-blocking, and subordinate to approved artifacts.
- [REQ-006][read] `writing-plans` keeps consult behavior with explicit non-blocking wording and conflict rule (approved artifacts win).
- [REQ-007][read] `systematic-debugging` keeps recurring-bug consult and evidence-first conflict handling.
- [REQ-008][review-independence] `plan-ceo-review`, `plan-fidelity-review`, `plan-eng-review`, and `requesting-code-review` must not gain autonomous memory consults.
- [REQ-009][execution] `executing-plans` and `subagent-driven-development` must capture durable findings in authoritative artifacts first.
- [REQ-010][execution] execution/reviewer loops must not directly edit `docs/project_notes/*`.
- [REQ-011][exception] only `systematic-debugging` may trigger immediate memory write-through for recurring bugs under strict threshold.
- [REQ-012][exception] recurring-bug write-through requires all: recurring/high rediscovery cost, known root cause, validated fix, concrete prevention note, authoritative backlink.
- [REQ-013][document-release] `document-release` is default owner of one non-blocking zero-or-one memory sweep before terminal final review.
- [REQ-014][document-release] sweep may distill only from authoritative/stable branch artifacts; no unsourced chat narrative.
- [REQ-015][document-release] if no durable delta exists, skip memory update with no workflow penalty.
- [REQ-016][document-release] memory sweep must not block transition to terminal review.
- [REQ-017][finish] `finishing-a-development-branch` must not perform autonomous memory writes.
- [REQ-018][router] `using-featureforge` keeps explicit-only memory routing and clarifies that autonomous follow-ups are owned by workflow skills.
- [REQ-019][rubric] all memory-aware skills use shared file-intent rubric and reject vocabulary from `project-memory` boundaries.
- [REQ-020][tests] contract tests must allow document-release ownership while still rejecting blocker/gate wording.
- [REQ-021][tests] contract tests must enforce new read/write boundaries and explicit-only `AGENTS.md` memory scope.
- [REQ-022][idempotency] `featureforge:project-memory` must apply deterministic no-duplicate merge behavior when the same durable memory item is nominated by both immediate write-through and later document-release sweep on the same branch.
- [REQ-023][observability] when `featureforge:project-memory` rejects one or more sweep candidates, document-release handoff output must include reject class, target memory file, and source-artifact pointer for each rejected candidate.
- [REQ-024][ownership] autonomous project-memory invocation is allowlisted to `document-release` plus the `systematic-debugging` recurring-bug exception; adding any other autonomous owner requires explicit spec and contract-test updates.
- [REQ-025][atomicity] when one project-memory invocation spans multiple memory files, writes must be atomic at invocation scope: structural failure in any target prevents all memory-file mutations for that invocation.
- [REQ-026][verification] implementation completion claims for this spec require a fixed validation command matrix to run green on the candidate `HEAD`.
- [REQ-027][observability] every document-release sweep pass must emit structured outcome reporting, including no-op passes where no memory invocation occurs.

## Detailed Requirements

### 7.1 `featureforge:project-memory` is sole write authority

- Keep `featureforge:project-memory` as sole writer for `docs/project_notes/*`.
- Other skills may nominate or invoke; they may not directly write project memory.
- Autonomous default write scope is `docs/project_notes/*` only.
- `AGENTS.md` changes are explicit-only for user-directed instruction maintenance tasks.
- Update `skills/project-memory/SKILL.md` and `skills/project-memory/authority-boundaries.md` accordingly.

### 7.2 Read-side consults only where they reduce rediscovery

#### 7.2.1 `featureforge:brainstorming`

Add optional consult triggers for cross-cutting/architecture-shaping work:

- multi-subsystem changes
- new architecture proposals
- likely prior constraints/decisions
- proposing a pattern that may already be settled

Consult files:

- `docs/project_notes/decisions.md`
- `docs/project_notes/key_facts.md`

The section must state: supportive only, optional, non-blocking, subordinate to approved artifacts and direct repo evidence.

#### 7.2.2 `featureforge:writing-plans`

Preserve consults with standardized wording:

- consult `key_facts.md` when present
- consult `decisions.md` when durable decisions may constrain plan shape
- note that later memory update is typically owned by `document-release`

#### 7.2.3 `featureforge:systematic-debugging`

Keep recurring-bug consult semantics:

- consult `bugs.md` for recurring/familiar/high-rediscovery-cost failures
- use as investigation guidance, not as replacement for evidence
- current evidence wins on conflict

### 7.3 Independence-sensitive review skills remain memory-independent

Do not add autonomous memory consults to:

- `featureforge:plan-ceo-review`
- `featureforge:plan-fidelity-review`
- `featureforge:plan-eng-review`
- `featureforge:requesting-code-review`

These skills decide using approved artifacts, execution evidence, review artifacts, runtime-owned workflow truth, and direct repo evidence.

Exception: if the user explicitly asks to review project memory content, reviewers may mention it as context, but memory still cannot override higher authority.

### 7.4 Execution skills capture authoritative sources first

Apply to:

- `featureforge:executing-plans`
- `featureforge:subagent-driven-development`

Required language:

- record durable lessons first in authoritative/stable outputs of the active step
- do not update `docs/project_notes/*` directly from implementer or reviewer loops
- pass durable memory candidates forward for later distillation

### 7.5 Narrow immediate write-through exception for recurring bugs

`featureforge:systematic-debugging` may invoke `featureforge:project-memory` for `bugs.md` only when all threshold conditions in REQ-012 are met.

If threshold is not met, defer memory updates to later `document-release` sweep.

### 7.6 `featureforge:document-release` owns the default sweep

`document-release` performs default memory housekeeping after release-readiness documentation and before terminal final review.

Sweep algorithm:

1. complete normal release-readiness doc pass
2. inspect branch diff and authoritative/stable artifacts for durable candidates
3. reject candidates that are secret-like, authority-blurring, tracker-like, instruction-like, unsourced, or one-off noise
4. group accepted candidates by memory file intent
5. if no surviving candidates: skip memory update
6. if surviving candidates: invoke `featureforge:project-memory` once (may touch multiple files)
7. if `project-memory` declines candidates: continue flow and report each rejection in handoff with reject class, target memory file, and source-artifact pointer
8. for multi-file writes in one invocation, apply updates atomically; if any structural write fails, apply no memory-file mutations and continue non-blocking flow with structured failure reporting
9. always emit a structured sweep outcome in handoff (candidates considered, accepted count, rejected count, skipped/no-delta reason when applicable, and whether `featureforge:project-memory` was invoked)

File intent mapping:

- recurring bugs -> `bugs.md`
- settled cross-cutting decisions -> `decisions.md`
- stable expensive-to-rediscover non-sensitive facts -> `key_facts.md`
- ticket/PR/plan/review breadcrumbs -> `issues.md`

Collision and idempotency handling:

- when document-release nominates an item already written through the recurring-bug exception (or otherwise already present from the same branch work), `featureforge:project-memory` must avoid duplicate entries
- equivalent candidate + existing entry -> no-op
- same durable item with additive provenance or verification metadata -> merge/update existing entry deterministically
- materially conflicting claims (for example conflicting root cause statements) -> reject as `AuthorityConflict` and continue non-blocking flow

### 7.7 `featureforge:finishing-a-development-branch` has no memory ownership

`finishing-a-development-branch` assumes memory housekeeping already occurred (usually in `document-release`) and must not trigger new autonomous memory writes.

### 7.8 `featureforge:using-featureforge` keeps explicit-only routing

Router behavior remains:

- direct route to `featureforge:project-memory` only for explicit project-memory intent
- no project-memory insertion into default mandatory stack

Clarifier to add:

- autonomous follow-up is owned by active workflow skill (typically `document-release`), not the entry router

### 7.9 Shared durability rubric

All memory-aware skills should reference shared rubric and boundaries instead of redefining semantics:

- `decisions.md`: settled cross-cutting decisions + backlinks
- `bugs.md`: recurring failures with root cause/fix/prevention
- `key_facts.md`: stable non-sensitive facts with source or `Last Verified`
- `issues.md`: concise breadcrumbs only

Reject classes remain centralized in project-memory boundary docs.

### 7.10 Autonomous owner allowlist

Autonomous invocation ownership is explicitly allowlisted:

- `featureforge:document-release` default zero-or-one sweep owner
- `featureforge:systematic-debugging` recurring-bug write-through exception

No other skill may autonomously invoke `featureforge:project-memory` during normal workflow-routed execution.
Any future autonomous owner expansion must update this spec and the corresponding contract tests in the same change.

### 7.11 Required validation command matrix

Because this spec is scoped to skill-library and contract-test changes, completion claims must include successful execution of this matrix on the candidate `HEAD`:

1. `node scripts/gen-skill-docs.mjs`
2. `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
3. targeted contract tests covering memory integration changes in this spec (at minimum the updated project-memory/skill-contract suite files touched by the implementation)

Responsibility split:

- implementer runs the matrix and reports command outcomes against the reviewed `HEAD`
- reviewer verifies that required commands were run and passed for the same `HEAD`
- missing or failing required commands means implementation is not complete yet for this spec

## File-Level Change Surface

Where templates exist, edit `.tmpl` files and regenerate checked-in `SKILL.md` outputs.

1. `skills/project-memory/SKILL.md` and `.tmpl`
2. `skills/project-memory/authority-boundaries.md`
3. `skills/project-memory/examples.md`
4. `skills/brainstorming/SKILL.md` and `.tmpl`
5. `skills/writing-plans/SKILL.md` and `.tmpl`
6. `skills/systematic-debugging/SKILL.md` and `.tmpl`
7. `skills/executing-plans/SKILL.md` and `.tmpl`
8. `skills/subagent-driven-development/SKILL.md` and `.tmpl`
9. `skills/document-release/SKILL.md` and `.tmpl`
10. `skills/finishing-a-development-branch/SKILL.md` and `.tmpl`
11. `skills/using-featureforge/SKILL.md` and `.tmpl`
12. `AGENTS.md` (read-side guidance alignment only; no autonomous target implication)
13. contract tests covering skill-doc wording and integration contracts

## Contract Test Requirements

Tests must enforce the model without introducing memory gates.

Required assertions:

1. `document-release` may own a non-blocking memory sweep.
2. blocker/gate language is still forbidden (`required for completion`, `must complete before completion`, `blocks completion`).
3. phrasing that places sweep in release-readiness pass is allowed when non-blocking.
4. `brainstorming` has optional read consult for `decisions.md` and `key_facts.md`.
5. `writing-plans` retains non-blocking consult semantics.
6. `systematic-debugging` enforces strict recurring-bug write-through threshold.
7. `executing-plans` and `subagent-driven-development` enforce capture-first and no direct project-memory writes.
8. `finishing-a-development-branch` does not own memory writes.
9. independence-sensitive review skills do not gain autonomous memory consult steps.
10. project-memory default autonomous write scope is `docs/project_notes/*`; `AGENTS.md` is explicit-only.
11. immediate write-through plus document-release sweep collisions do not produce duplicate entries and follow deterministic merge/reject behavior.
12. rejected sweep candidates always emit structured handoff reporting (reject class, target file, source pointer) without blocking progression.
13. autonomous project-memory invocation remains restricted to the explicit allowlist (`document-release` and the debugging recurring-bug exception).
14. multi-file project-memory sweeps are atomic per invocation (no partial memory-file mutations on structural failure).
15. implementation completion claims require the fixed validation command matrix to pass on the candidate `HEAD`.
16. every document-release sweep pass emits structured outcome reporting, including no-op passes.

## Acceptance Criteria

1. `featureforge:project-memory` is the only skill that writes `docs/project_notes/*`.
2. `brainstorming`, `writing-plans`, and `systematic-debugging` include explicit supportive read consult hooks.
3. `plan-ceo-review`, `plan-fidelity-review`, `plan-eng-review`, and `requesting-code-review` stay memory-independent by default.
4. execution skills capture durable findings in authoritative artifacts and avoid direct project-memory writes.
5. recurring-bug write-through exception is strict and explicitly bounded.
6. `document-release` owns default zero-or-one non-blocking sweep before terminal review.
7. `finishing-a-development-branch` performs no new autonomous memory writes.
8. `using-featureforge` keeps explicit-only routing and workflow-owner clarifier.
9. `AGENTS.md` is not part of autonomous default memory write scope.
10. contract tests reject regressions that turn memory into a blocker or shadow workflow system.
11. required validation commands run green on the candidate `HEAD` before completion is claimed.
12. document-release outputs structured sweep outcomes for invocation and no-op paths.

## Risks And Mitigations

- **Risk:** memory churn from frequent writes  
  **Mitigation:** one late batched sweep by default, one narrow debug exception.
- **Risk:** authority inversion  
  **Mitigation:** strict authority order and backlink requirements.
- **Risk:** sensitive-content leakage  
  **Mitigation:** reject secret-like content and require provenance.
- **Risk:** stale/duplicated memory  
  **Mitigation:** short takeaways plus backlinks; reject oversized duplication.
- **Risk:** final-review contamination  
  **Mitigation:** no autonomous memory writes in independence-sensitive review or finish stage.

## Rollout

1. **Skill and boundary text updates** across listed skills and `AGENTS.md`.
2. **Contract test updates** to enforce non-blocking ownership and consult/write boundaries.
3. **Validation pass** for representative scenarios:
   - cross-cutting feature consult-only brainstorming
   - normal implementation with document-release sweep before final review
   - no-delta pass skipping memory
   - recurring bug write-through only after threshold evidence
   - review independence
   - clean finish with no late memory edits

## Explicitly Out Of Scope

- new runtime-owned memory helper/command family
- new project-memory workflow stage or helper-derived routing state
- automatic `AGENTS.md` mutation during normal branch work
- project-memory gating in review/finish/branch completion
- using project memory as replacement for approved specs/plans/reviews/evidence

## Final Decision

Adopt autonomous project memory as a supportive, batched, workflow-owned sidecar.

- consult memory where it reduces rediscovery
- capture truth in authoritative artifacts during work
- perform default non-blocking sweep in `document-release`
- keep terminal final review on the same `HEAD`
- keep finish-stage clean
- keep router explicit and simple
- keep `featureforge:project-memory` as the sole writer

## CEO Review Summary

**Review Status:** clear
**Reviewed At:** 2026-04-08T18:34:12Z
**Review Mode:** hold_scope
**Reviewed Spec Revision:** 1
**Critical Gaps:** 0
**UI Design Intent Required:** no
**Outside Voice:** skipped
