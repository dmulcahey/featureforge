# Autonomous Project Memory Management Integration

**Workflow State:** CEO Approved
**Spec Revision:** 2
**Last Reviewed By:** plan-ceo-review

**Status:** CEO-approved revision aligned to the current FeatureForge runtime and skill-library contracts
**Target Branch:** `dm/spec-backlog`
**Scope:** skill-library, prompt, documentation, and contract-test changes only
**Primary Objective:** Make project memory autonomously useful without allowing memory to become a workflow stage, runtime gate, routing input, release blocker, execution projection, or second source of truth.

## Revision 2 Update Summary

This revision updates the original autonomous project-memory spec for the current codebase after the churn-elimination/runtime-authority cutover.

Material changes incorporated here:

- `featureforge workflow operator --plan <approved-plan-path>` is now the normal post-approval routing surface; the older plan-execution recommendation wording is obsolete.
- Execution authority is now the append-only event log under `execution-harness/events.jsonl`; projections such as `state.json`, checked-list marks, release/readiness artifacts, review artifacts, and QA artifacts are read models.
- Project memory must not feed or alter the reducer/router route decision, semantic workspace identity, review-state repair, late-stage advancement, or mutator legality.
- The `document-release` memory sweep must occur before terminal final review and, when it mutates tracked memory files, before release-readiness is recorded through `advance-late-stage --result ready|blocked`, so final review and runtime milestones see the same semantic workspace.
- Implementation plans now use the current approved task contract: task-level `Goal`, `Context`, `Constraints`, `Done when`, and `Files` fields, plus a parseable execution strategy and dependency diagram.
- Contract-test coverage must include the current public command vocabulary, plan-contract expectations, route fixtures, AGENTS/runtime-instruction assertions, generated skill docs, project-memory content examples, and subagent prompt boundaries.

The earlier intent remains intact: memory is a supportive, low-churn knowledge sidecar. This revision updates the integration points so that intent survives the current codebase shape.

## Current Codebase Baseline

The current codebase already contains:

- a generated skill library with source templates at `skills/*/SKILL.md.tmpl` and checked-in generated docs at `skills/*/SKILL.md`
- repo-visible project memory under `docs/project_notes/`
- `featureforge:project-memory` with repo-safety protection for memory file writes
- `featureforge:writing-plans` with a current plan document header and task contract that require `Goal`, `Context`, `Constraints`, `Done when`, and `Files`
- `review/plan-task-contract.md` as the source of truth for task-body structure
- runtime instructions that route post-approval work through `featureforge workflow operator --plan <approved-plan-path>`
- event-log-owned execution state and projection-only checked artifacts
- contract tests under `tests/codex-runtime/`, Rust runtime-instruction tests, using-featureforge route tests, and plan-contract tests

The codebase also still contains outdated or incomplete project-memory integration points:

- `featureforge:brainstorming` lacks the requested optional memory consult language.
- `featureforge:writing-plans` has memory consult language but does not fully align with document-release ownership or current authority wording.
- `featureforge:systematic-debugging` allows durable recurring bug write-through but needs the stricter threshold and defer-to-document-release rule.
- `featureforge:executing-plans` and `featureforge:subagent-driven-development` do not yet capture durable memory candidates through authoritative artifacts first.
- `featureforge:document-release` has only an optional follow-up, not the structured zero-or-one sweep owner model.
- `featureforge:project-memory` and `authority-boundaries.md` still treat a narrow `AGENTS.md` memory section as part of the default write set; autonomous default scope must narrow to `docs/project_notes/*` only.
- `featureforge:using-featureforge` has explicit memory routing but needs the current workflow-owner clarifier.
- Tests still encode the older optional follow-up and old default-write-set expectations.

## Problem Statement

Project memory exists, but its current integration has five failure modes:

1. read-side use remains inconsistent across planning, brainstorming, debugging, and execution work;
2. durable implementation findings can be lost when no authoritative artifact captures them before the branch closes;
3. memory write permission is too easy to blur across skills, prompts, and instruction surfaces;
4. late memory edits can change the semantic workspace after release/readiness or final-review routing has already been decided;
5. old plan and execution wording no longer matches the current runtime, which now centers the public `workflow operator` route and an event-log reducer.

The system needs one coherent model for who may read memory, who may write memory, when memory writes are allowed, how memory writes interact with release/readiness sequencing, and what surfaces memory must never influence.

## Goals

1. Make project memory autonomously useful during normal workflow-routed work.
2. Preserve authority order: approved specs, approved plans, execution evidence, review artifacts, runtime-owned state, active instructions, stable repo docs, and code stay above project memory.
3. Keep memory low-churn, deterministic, source-backed, and reviewable.
4. Ensure final review sees memory edits on the same semantic workspace and `HEAD` as release/readiness documentation.
5. Protect reviewer independence in approval, fidelity, engineering, and final whole-diff review skills.
6. Avoid adding runtime stages, helper commands, completion gates, route inputs, or workflow projections for memory.
7. Align the implementation plan and validation matrix with the current plan-contract and workflow-operator codebase.

## Non-Goals

This spec does not:

- elevate project memory to workflow truth;
- make project memory a runtime state store, reducer input, route input, or projection;
- require memory updates for planning, execution, review, QA, finish, or branch completion;
- allow `issues.md` to become a live tracker, board, execution checklist, or day-by-day log;
- allow execution or review subagents to directly edit `docs/project_notes/*`;
- make `AGENTS.md` a routine autonomous memory target;
- add workflow stage, helper, gate, hidden command, or completion blocker machinery for memory;
- allow routine post-final-review memory edits from `featureforge:finishing-a-development-branch`;
- rewrite unrelated runtime execution-state logic, plan-contract parsing, or repo-safety behavior.

## Authority And Invariants

### 1) Authority order remains explicit

When artifacts disagree, precedence is:

1. approved specs;
2. approved plans;
3. execution evidence and review artifacts tied to approved work;
4. runtime-owned event-log-derived state, route decisions, semantic workspace identity, and projections;
5. active repo instructions such as `AGENTS.md`;
6. stable repo docs and code;
7. `docs/project_notes/*`.

Project memory may summarize and backlink. It must not override upstream authority.

### 2) `featureforge:project-memory` remains the sole memory writer

`featureforge:project-memory` is the only skill allowed to write repo-visible project memory under `docs/project_notes/*`.

Other skills and prompts may only:

- consult project memory;
- identify durable candidate takeaways;
- pass candidates through authoritative artifacts, packets, handoff notes, or release summaries;
- invoke `featureforge:project-memory` only when an allowlisted owner rule permits it.

### 3) Autonomous write scope is narrower than explicit write scope

Autonomous workflow-owned memory writes default to `docs/project_notes/*` only.

`AGENTS.md` edits require explicit user intent or explicit repo-maintenance scope. `featureforge:project-memory` may still be used for an explicit AGENTS memory-section maintenance request, but no normal workflow skill may include `AGENTS.md` in an autonomous memory sweep.

### 4) Memory remains non-blocking

Memory quality may improve future work. It must never block:

- brainstorming completion;
- plan drafting;
- plan approval;
- execution handoff;
- task completion;
- release/readiness recording;
- final review dispatch;
- QA routing;
- finish gating;
- branch completion.

### 5) Batch over scatter

The default model is one late zero-or-one sweep in `featureforge:document-release`, not many opportunistic writes throughout the workflow.

### 6) Review independence is preserved

Approval, fidelity, engineering, and terminal code-review skills decide from approved artifacts, runtime-owned state, direct repo evidence, and review evidence. They must not gain autonomous project-memory consult steps.

### 7) Runtime routing remains event-log-owned

Project memory must not influence the reducer/router route decision, event sequence, event hash continuity, semantic workspace identity, projection freshness, review-state repair, mutator legality, or public operator recommendation.

## Operating Model

Normal lifecycle:

1. `featureforge:using-featureforge` routes explicit memory requests to `featureforge:project-memory`; otherwise it follows workflow/operator and workflow state.
2. `featureforge:brainstorming` optionally consults memory when prior decisions or key facts could prevent rediscovery.
3. `featureforge:plan-ceo-review` stays memory-independent by default.
4. `featureforge:writing-plans` consults memory where relevant, records plan truth in the plan, and notes that later memory updates are normally owned by `document-release`.
5. `featureforge:plan-fidelity-review` stays memory-independent by default.
6. `featureforge:plan-eng-review` stays memory-independent by default.
7. Execution uses `featureforge workflow operator --plan <approved-plan-path>` and the runtime-selected owner skill.
8. `featureforge:executing-plans` and `featureforge:subagent-driven-development` capture durable lessons first in authoritative artifacts and handoffs.
9. `featureforge:systematic-debugging` consults `bugs.md`; only it may perform immediate recurring-bug write-through under the strict threshold in this spec.
10. `featureforge:document-release` performs the default zero-or-one memory sweep after release-facing documentation review and before terminal final review; when a memory write changes tracked files, the write happens before release/readiness is recorded as ready or blocked.
11. `featureforge:requesting-code-review` reviews the same semantic workspace and remains memory-independent by default.
12. `featureforge:qa-only` runs only when workflow/operator requires QA.
13. `featureforge:finishing-a-development-branch` performs no autonomous memory writes.

Rule of thumb:

- read early when memory reduces rediscovery;
- capture truth in authoritative artifacts during work;
- write once, late, through `featureforge:project-memory`;
- keep routing and gates on workflow/operator and event-log-derived state;
- do not write memory after terminal final review unless the user explicitly reopens the work or explicitly asks for a memory maintenance task.

## Requirement Index

- [REQ-001][authority] `featureforge:project-memory` is the only skill allowed to write `docs/project_notes/*`.
- [REQ-002][authority] Autonomous default write scope is `docs/project_notes/*`; `AGENTS.md` is explicit-only.
- [REQ-003][authority] Skill docs and boundary docs must reject autonomous `AGENTS.md` updates and require explicit user intent or explicit repo-maintenance scope.
- [REQ-004][read] `featureforge:brainstorming` must add optional consult of `decisions.md` and `key_facts.md` for cross-cutting or architecture-shaping work.
- [REQ-005][read] Brainstorming memory consult is supportive, non-blocking, and subordinate to approved artifacts and direct repo evidence.
- [REQ-006][read] `featureforge:writing-plans` must keep consult behavior with explicit non-blocking wording, approved-artifact precedence, and document-release follow-up ownership.
- [REQ-007][read] `featureforge:systematic-debugging` must keep recurring-bug consult behavior with evidence-first conflict handling.
- [REQ-008][review-independence] `plan-ceo-review`, `plan-fidelity-review`, `plan-eng-review`, and `requesting-code-review` must not gain autonomous memory consult steps.
- [REQ-009][execution] `executing-plans` and `subagent-driven-development` must capture durable findings in authoritative artifacts before any memory nomination.
- [REQ-010][execution] execution and reviewer loops must not directly edit `docs/project_notes/*`.
- [REQ-011][exception] only `systematic-debugging` may trigger immediate memory write-through for recurring bugs.
- [REQ-012][exception] recurring-bug write-through requires every threshold condition: recurring or high rediscovery cost, known root cause, validated fix, concrete prevention note, and authoritative backlink.
- [REQ-013][document-release] `document-release` is the default owner of one non-blocking zero-or-one memory sweep before terminal final review.
- [REQ-014][document-release] the sweep may distill only from authoritative or stable branch artifacts; unsourced chat narrative and transient scratch notes are invalid sweep sources.
- [REQ-015][document-release] if no durable delta exists, the sweep is skipped with no workflow penalty.
- [REQ-016][document-release] memory sweep outcomes must not block transition to terminal final review, QA routing, or finish.
- [REQ-017][finish] `finishing-a-development-branch` must not perform autonomous memory writes.
- [REQ-018][router] `using-featureforge` must keep explicit-only project-memory routing and clarify that autonomous follow-up is owned by the active workflow skill.
- [REQ-019][rubric] memory-aware skills must use shared file-intent and reject vocabulary rather than redefining memory semantics.
- [REQ-020][tests] contract tests must allow document-release sweep ownership while rejecting blocker, prerequisite, completion-gate, and workflow-stage wording.
- [REQ-021][tests] contract tests must enforce read/write boundaries and explicit-only `AGENTS.md` memory scope.
- [REQ-022][idempotency] `featureforge:project-memory` must apply deterministic no-duplicate merge behavior when the same durable memory item is nominated by immediate write-through and later document-release sweep on the same branch.
- [REQ-023][observability] when `featureforge:project-memory` rejects sweep candidates, document-release handoff output must include reject class, target memory file, and source-artifact pointer for each rejected candidate.
- [REQ-024][ownership] autonomous project-memory invocation is allowlisted to `document-release` plus the `systematic-debugging` recurring-bug exception.
- [REQ-025][atomicity] when one project-memory invocation spans multiple memory files, writes must be atomic at invocation scope; a structural failure in any target prevents all memory-file mutations for that invocation.
- [REQ-026][verification] implementation completion claims require a fixed validation command matrix to run green on the candidate `HEAD`.
- [REQ-027][observability] every document-release sweep pass must emit structured outcome reporting, including no-op passes where no memory invocation occurs.
- [REQ-028][runtime-authority] project memory must not write, derive, patch, or override execution-harness event logs, state projections, workflow route decisions, semantic workspace identity, or mutator legality.
- [REQ-029][late-stage-sequencing] when document-release mutates memory files, those mutations must occur before `advance-late-stage --result ready|blocked` and before terminal whole-diff final review.
- [REQ-030][public-command-contract] implementation guidance must use current public command vocabulary: `workflow operator`, `plan execution status` for diagnostics, and public mutators such as `begin`, `complete`, `reopen`, `transfer`, `close-current-task`, `repair-review-state`, and `advance-late-stage`; removed or hidden helper recommendations must not appear in normal-path guidance.
- [REQ-031][plan-format] the implementation plan must use the current task contract with `Goal`, `Context`, `Constraints`, `Done when`, and `Files`, and it must not use retired task-level field names.
- [REQ-032][execution-topology] the implementation plan must include a parseable execution strategy and dependency diagram that match current plan-contract expectations.
- [REQ-033][repo-safety] memory writes remain protected by `repo-file-write`; release-facing documentation writes remain protected by `release-doc-write`; no skill may bypass repo-safety through direct edits.
- [REQ-034][generation] template changes must be followed by `node scripts/gen-skill-docs.mjs`, and generated docs must match their templates.
- [REQ-035][route-fixture] using-featureforge route fixtures and Rust route tests must preserve explicit memory override while blocking project-memory insertion into the default mandatory stack.
- [REQ-036][review-readiness] the updated plan must be ready for current plan-fidelity review dimensions: requirement coverage, source-spec fidelity, task contract shape, deterministic completion obligations, execution topology, and validation sufficiency.
- [REQ-037][subagent-boundary] subagent implementer and reviewer prompt surfaces must not instruct subagents to edit project memory directly; durable candidates must flow through packets, execution evidence, review artifacts, or release summaries.

## Detailed Requirements

### 7.1 Project-memory authority and scope

`featureforge:project-memory` remains the single writer for `docs/project_notes/*`.

Required updates:

- Update `skills/project-memory/SKILL.md.tmpl` and generated `SKILL.md` to state that autonomous workflow writes are limited to `docs/project_notes/*`.
- Update `skills/project-memory/authority-boundaries.md` to separate autonomous default scope from explicit AGENTS maintenance scope.
- Update `skills/project-memory/examples.md` so examples do not imply routine AGENTS mutation during normal work.
- Update `docs/project_notes/README.md` if its authority wording no longer matches the current runtime and instruction-authority order.
- Preserve repo-safety preflight requirements for every memory file write.

`AGENTS.md` may still be an explicit maintenance target when the user asks for instruction or memory-section maintenance. It must not be part of document-release sweeps, debug write-through, execution capture, or implicit workflow cleanup.

### 7.2 Read-side consults

#### 7.2.1 Brainstorming

Add optional consult triggers for:

- multi-subsystem changes;
- architecture-shaping proposals;
- likely prior constraints or decisions;
- patterns that may already be settled;
- expensive-to-rediscover repo facts.

Consult files:

- `docs/project_notes/decisions.md`;
- `docs/project_notes/key_facts.md`.

The skill must state that memory is supportive, optional, non-blocking, and subordinate to approved artifacts, direct repo evidence, active instructions, and current code.

#### 7.2.2 Writing plans

Preserve and sharpen current consult behavior:

- consult `key_facts.md` when stable repo facts may affect decomposition;
- consult `decisions.md` when durable prior decisions may constrain architecture or task shape;
- state that plan truth belongs in the plan itself;
- state that later memory updates are normally owned by `document-release`;
- state that approved specs and approved plans win on conflict.

#### 7.2.3 Systematic debugging

Keep recurring-bug consult semantics:

- consult `bugs.md` for recurring, familiar, or high-rediscovery-cost failures;
- use memory as investigation guidance, not evidence replacement;
- current trace evidence and validated reproduction/fix evidence win on conflict.

### 7.3 Review independence

Do not add autonomous memory consults to:

- `featureforge:plan-ceo-review`;
- `featureforge:plan-fidelity-review`;
- `featureforge:plan-eng-review`;
- `featureforge:requesting-code-review`.

If a user explicitly asks a reviewer to examine project memory, the reviewer may mention it as context, but memory remains below approved artifacts, direct repo evidence, runtime-owned state, and active instructions.

### 7.4 Execution capture-first rules

Apply to:

- `featureforge:executing-plans`;
- `featureforge:subagent-driven-development`;
- subagent implementer prompts;
- subagent code-quality reviewer prompts;
- subagent spec reviewer prompts.

Required behavior:

- durable lessons first go into authoritative artifacts for the current step, packet, evidence, review, or handoff;
- implementer and reviewer loops do not directly edit `docs/project_notes/*`;
- durable memory candidates are passed forward for document-release sweep;
- execution skills must not reconstruct closure routing from project memory;
- execution skills follow workflow/operator fields and runtime-selected public commands.

### 7.5 Narrow immediate write-through exception

`featureforge:systematic-debugging` may invoke `featureforge:project-memory` for `docs/project_notes/bugs.md` only when every threshold condition is true:

1. the failure is recurring or has high rediscovery cost;
2. root cause is known;
3. fix is validated;
4. prevention note is concrete;
5. authoritative source backlink exists.

If any threshold condition is absent, debugging must defer the memory candidate to document-release sweep or omit it.

### 7.6 Document-release sweep ownership

`featureforge:document-release` owns the default zero-or-one memory sweep after release-facing documentation review and before terminal final review.

Sweep algorithm:

1. complete normal release/readiness documentation analysis;
2. inspect branch diff, approved artifacts, execution evidence, review artifacts, release summary, and stable repo docs for durable candidates;
3. reject unsourced chat narrative, transient scratch notes, secrets, authority-blurring claims, tracker-like content, instruction-like content, one-off noise, and oversized duplication;
4. group accepted candidates by target memory file intent;
5. skip invocation when no durable candidate survives;
6. invoke `featureforge:project-memory` once when surviving candidates exist;
7. require repo-safety preflight for every target memory path;
8. apply multi-file updates atomically at invocation scope;
9. deduplicate or merge equivalent candidates deterministically;
10. reject materially conflicting claims as `AuthorityConflict`;
11. emit structured sweep outcome reporting for invocation, skip, rejection, failure, and no-op paths;
12. continue workflow routing without treating memory result as a gate.

Sequencing requirement:

- If memory files are mutated, they must be mutated before `advance-late-stage --result ready|blocked` records release/readiness and before terminal final review dispatch.
- If no memory files are mutated, document-release still emits the structured sweep outcome and continues normal release/readiness recording.

### 7.7 File intent mapping

Use the shared file intent rubric:

- recurring bugs -> `docs/project_notes/bugs.md`;
- settled cross-cutting decisions -> `docs/project_notes/decisions.md`;
- stable expensive-to-rediscover non-sensitive facts -> `docs/project_notes/key_facts.md`;
- ticket, PR, plan, review, or evidence breadcrumbs -> `docs/project_notes/issues.md`.

Reject classes remain centralized in project-memory boundary docs:

- `SecretLikeContent`;
- `AuthorityConflict`;
- `TrackerDrift`;
- `MissingProvenance`;
- `OversizedDuplication`;
- `InstructionAuthorityDrift`.

### 7.8 Finish-stage boundary

`featureforge:finishing-a-development-branch` assumes memory housekeeping has already happened, usually in document-release. It must not trigger new autonomous memory writes.

If the user explicitly asks for memory maintenance after finish, route that as explicit project-memory work, not as finish-stage responsibility.

### 7.9 Router boundary

`featureforge:using-featureforge` keeps explicit-only routing to `featureforge:project-memory` for requests that clearly ask to set up, repair, or update project memory.

It must also clarify that autonomous memory follow-up is owned by the active workflow skill, typically document-release or the systematic-debugging exception, not by the entry router.

### 7.10 Runtime and public-command boundary

Skill docs and plan text must align to the current runtime:

- use `featureforge workflow operator --plan <approved-plan-path>` for normal post-approval routing;
- use `featureforge plan execution status --plan <approved-plan-path>` only for diagnostics;
- use public mutators surfaced by operator guidance;
- do not recommend removed normal-path commands or hidden/debug commands in ordinary workflow guidance;
- do not infer routing from markdown artifacts or project memory.

### 7.11 Plan-contract boundary

The updated implementation plan must comply with the current plan-task contract:

- every task includes `Spec Coverage`, `Goal`, `Context`, `Constraints`, `Done when`, and `Files` in the required order;
- every task has deterministic pass/fail completion obligations;
- every task has parseable file-scope declarations;
- execution strategy assigns every task exactly once;
- dependency diagram edges match the execution strategy;
- shared test and generated-doc hotspots are sequenced serially.

### 7.12 Required validation command matrix

Implementation completion claims require successful execution of this matrix on the candidate `HEAD`:

1. `node scripts/gen-skill-docs.mjs`
2. `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
3. `node --test tests/codex-runtime/project-memory-content.test.mjs`
4. `cargo nextest run --test runtime_instruction_contracts --test runtime_instruction_execution_contracts --test using_featureforge_skill`
5. `cargo nextest run --test contracts_spec_plan`
6. `cargo clippy --all-targets --all-features -- -D warnings` when Rust source or Rust tests are changed
7. `node scripts/gen-agent-docs.mjs --check` when generated agent or install docs are touched, or an explicit skip note when no generated agent/install doc is in scope

The implementer records command outputs against the reviewed `HEAD`. Reviewers verify the same `HEAD` and reject completion claims missing required evidence.

## File-Level Change Surface

Where templates exist, edit `.tmpl` files and regenerate checked-in `SKILL.md` outputs.

1. `skills/project-memory/SKILL.md.tmpl`
2. `skills/project-memory/SKILL.md`
3. `skills/project-memory/authority-boundaries.md`
4. `skills/project-memory/examples.md`
5. `docs/project_notes/README.md`
6. `skills/brainstorming/SKILL.md.tmpl`
7. `skills/brainstorming/SKILL.md`
8. `skills/writing-plans/SKILL.md.tmpl`
9. `skills/writing-plans/SKILL.md`
10. `skills/systematic-debugging/SKILL.md.tmpl`
11. `skills/systematic-debugging/SKILL.md`
12. `skills/executing-plans/SKILL.md.tmpl`
13. `skills/executing-plans/SKILL.md`
14. `skills/subagent-driven-development/SKILL.md.tmpl`
15. `skills/subagent-driven-development/SKILL.md`
16. `skills/subagent-driven-development/implementer-prompt.md`
17. `skills/subagent-driven-development/code-quality-reviewer-prompt.md`
18. `skills/subagent-driven-development/spec-reviewer-prompt.md`
19. `skills/document-release/SKILL.md.tmpl`
20. `skills/document-release/SKILL.md`
21. `skills/finishing-a-development-branch/SKILL.md.tmpl`
22. `skills/finishing-a-development-branch/SKILL.md`
23. `skills/using-featureforge/SKILL.md.tmpl`
24. `skills/using-featureforge/SKILL.md`
25. `AGENTS.md`
26. `README.md`
27. `docs/README.codex.md`
28. `docs/README.copilot.md`
29. `tests/codex-runtime/skill-doc-contracts.test.mjs`
30. `tests/codex-runtime/project-memory-content.test.mjs`
31. `tests/runtime_instruction_contracts.rs`
32. `tests/runtime_instruction_execution_contracts.rs`
33. `tests/using_featureforge_skill.rs`
34. `tests/fixtures/using-featureforge-project-memory-route-contract.sh`
35. `tests/contracts_spec_plan.rs`

No runtime production code is expected unless tests expose a current parser/fixture mismatch that blocks validation of the updated docs. Such a mismatch requires explicit reviewer-visible justification.

## Contract Test Requirements

Tests must enforce the model without adding memory gates.

Required assertions:

1. document-release may own a non-blocking zero-or-one memory sweep;
2. blocker, prerequisite, completion-gate, and workflow-stage wording remains forbidden;
3. document-release sweep sequencing is allowed only when non-blocking and before terminal final review;
4. memory mutations occur before release/readiness recording when the sweep writes tracked files;
5. brainstorming has optional read consult for `decisions.md` and `key_facts.md`;
6. writing-plans retains non-blocking consult semantics and document-release ownership language;
7. systematic-debugging enforces the strict recurring-bug write-through threshold;
8. executing-plans and subagent-driven-development enforce capture-first and no direct project-memory writes;
9. finishing-a-development-branch owns no autonomous memory writes;
10. independence-sensitive review skills do not gain autonomous memory consult steps;
11. project-memory default autonomous write scope is `docs/project_notes/*`; `AGENTS.md` is explicit-only;
12. immediate write-through plus document-release sweep collisions do not produce duplicate entries and follow deterministic merge/reject behavior;
13. rejected sweep candidates always emit structured handoff reporting without blocking progression;
14. autonomous project-memory invocation remains restricted to document-release and the debugging recurring-bug exception;
15. multi-file project-memory sweeps are atomic per invocation;
16. every document-release sweep pass emits structured outcome reporting, including no-op passes;
17. using-featureforge preserves explicit memory override and rejects default-stack insertion;
18. plan text and skill docs use current workflow/operator public command vocabulary;
19. subagent prompt surfaces do not instruct implementer or reviewer subagents to edit `docs/project_notes/*`;
20. plan-contract tests reject the previous task-field format for active implementation plans.

## Acceptance Criteria

1. `featureforge:project-memory` is the only skill that writes `docs/project_notes/*`.
2. Autonomous workflow-owned memory writes target `docs/project_notes/*` only.
3. `AGENTS.md` updates require explicit user intent or explicit repo-maintenance scope.
4. `brainstorming`, `writing-plans`, and `systematic-debugging` include supportive read consult hooks.
5. `plan-ceo-review`, `plan-fidelity-review`, `plan-eng-review`, and `requesting-code-review` remain memory-independent by default.
6. execution skills and subagent prompts capture durable findings in authoritative artifacts and avoid direct project-memory writes.
7. recurring-bug write-through is strict and bounded to `systematic-debugging`.
8. `document-release` owns the default zero-or-one non-blocking sweep before terminal review.
9. document-release memory mutations occur before release/readiness recording when tracked memory files change.
10. document-release emits structured sweep outcomes for invocation, rejection, failure, skip, and no-op paths.
11. `finishing-a-development-branch` performs no new autonomous memory writes.
12. `using-featureforge` keeps explicit-only project-memory routing and workflow-owner clarifier.
13. contract tests reject regressions that turn memory into a blocker, route input, runtime state source, or shadow workflow system.
14. generated skill docs match templates.
15. required validation commands run green on the candidate `HEAD` before completion is claimed.
16. the implementation plan uses the current task contract and parseable execution topology.

## Risks And Mitigations

- **Risk:** memory churn from frequent writes.  
  **Mitigation:** one late batched sweep by default, one narrow debugging exception, deterministic no-op handling.
- **Risk:** authority inversion.  
  **Mitigation:** strict authority order, source backlinks, tests that reject workflow-stage and route-input language.
- **Risk:** sensitive-content leakage.  
  **Mitigation:** reject `SecretLikeContent`, forbid unsanitized private data, and require provenance.
- **Risk:** stale or duplicated memory.  
  **Mitigation:** deterministic merge/no-op behavior, backlinks, dated verification where facts can change.
- **Risk:** final-review contamination.  
  **Mitigation:** review-skill independence and document-release sequencing before terminal review.
- **Risk:** runtime churn after the event-log cutover.  
  **Mitigation:** use public workflow/operator commands only and keep memory out of event-log/reducer/projection authority.
- **Risk:** plan rejection due to old task format.  
  **Mitigation:** author the plan directly against `review/plan-task-contract.md` and validate through plan-contract tests.

## Rollout

1. Update contract tests first for the current runtime-aware memory model.
2. Narrow project-memory authority and AGENTS scope.
3. Add consult-only and capture-first language across planning, debugging, execution, and subagent prompts.
4. Implement document-release sweep ownership and structured reporting language.
5. Align router, finish-stage, AGENTS, README, and install-doc wording.
6. Consolidate examples and route fixtures.
7. Regenerate skill docs.
8. Run the fixed validation matrix.
9. Hand off to current plan-fidelity and engineering review flows.

## Explicitly Out Of Scope

- runtime production rewrites;
- new memory helper command family;
- new project-memory workflow stage;
- project-memory route inputs;
- project-memory gate inputs;
- automatic `AGENTS.md` mutation during normal branch work;
- post-final-review autonomous memory edits;
- replacing approved specs/plans/reviews/evidence with project memory;
- using memory to repair runtime state or projections.

## Final Decision

Adopt autonomous project memory as a supportive, batched, workflow-owned sidecar under the current FeatureForge runtime:

- consult memory where it reduces rediscovery;
- record truth in authoritative artifacts during work;
- perform the default non-blocking sweep in `document-release`;
- sequence memory writes before release/readiness recording and terminal review;
- keep final review on the same semantic workspace;
- keep finish-stage clean;
- keep router explicit and simple;
- keep workflow/operator and the event log as runtime authority;
- keep `featureforge:project-memory` as the sole writer for repo-visible memory.
