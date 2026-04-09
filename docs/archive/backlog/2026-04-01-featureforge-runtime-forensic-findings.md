# FeatureForge Runtime Forensic Findings

**Date:** 2026-04-01  
**Repository:** `featureforge`  
**Scope:** runtime edges with emphasis on execution evidence, task-boundary receipts, release-readiness receipts, final-review receipts, workflow routing, skill/runtime mismatches, and test realism  
**Audit note:** this version supersedes the earlier same-day draft after the branch was updated from `main`
**Normative note:** this document is forensic and non-normative. It preserves historical pain points and may mention obsolete pre-pivot operator surfaces. The April supersession-aware spec corpus under `docs/featureforge/specs` is the current implementation target.

## Executive Summary

The runtime is not weak. It is strict and increasingly explicit.

The problem is still the abstraction boundary. FeatureForge enforces multiple runtime-owned contract surfaces while leaving too much authoring, repair, and reconciliation work to the operator. The merge from `main` improved late-stage sequencing, but it also exposed an additional strict manual surface: release-readiness now sits in front of terminal final review and is validated just as aggressively as other late-stage artifacts.

The strictest surfaces are now:

1. execution evidence freshness
2. task-boundary closure receipts
3. release-readiness artifact validation
4. final-review artifact validation
5. workflow routing and late-stage precedence

The recurring failure mode is unchanged:

- runtime requires an artifact or truth binding
- the public operator surface does not cleanly own that artifact or binding
- skills or docs still imply a supported path
- tests often cover the gap by fabricating runtime-owned artifacts directly

That is the wrong ownership model for safe workflow-guided agent execution.

## What Changed Since The Earlier Audit

Two things materially changed after the branch update:

1. late-stage sequencing is clearer and better than before
2. the unsupported operator surface is larger than before because release-readiness is now a first-class enforced artifact

The newer runtime/docs now explicitly sequence terminal workflow work as:

- `featureforge:document-release`
- terminal `featureforge:requesting-code-review`
- optional `featureforge:qa-only`
- `featureforge:finishing-a-development-branch`

That is an improvement. It is also incomplete, because `document-release` still tells the operator to hand-author a project-scoped release-readiness artifact that `gate-finish` validates strictly.

## What The Independent Audits Added

The two independent clean-context audits materially strengthened the picture in six places:

1. `gate-finish` does not currently enforce the same authoritative late-gate truth as `gate-review`. `gate-review_from_context_internal(context, true)` enforces authoritative late-gate truth, while `gate_finish_from_context` merges `gate_review_from_context_internal(context, false)` and therefore skips that enforcement path. That means a finish gate can look green while authoritative late-stage truth is stale.
2. `gate-review-dispatch` mutates strategy and cycle state before it knows whether review is actually dispatchable. That means blocked dispatch attempts can still churn strategy checkpoints and contribute toward `cycle_break`.
3. `rebuild-evidence` does not just refresh evidence and reconcile truth. It rewrites late-stage branch artifacts in place, including final-review, reviewer, QA, test-plan, and release-readiness headers, then republishes authoritative truth from the rewritten files. That is stronger than simple repair churn. It is effectively proof rewriting.
4. The public routing contract is split across `next_skill`, `next_action`, and `recommended_skill`. For late-stage `implementation_ready` flows, those fields do not present one coherent public contract.
5. The runtime has no first-class public doctor or reconcile surface for authoritative-state corruption, sequence mismatch, or stale write-authority / handoff state even though those are explicit fail-closed states.
6. Base-branch resolution logic is duplicated in multiple skills even though the runtime already has a canonical resolver. That is avoidable doc drift and rebase churn.

Those points do not invalidate the earlier audit. They sharpen it.

## What The Last Two Weeks Of Session History Added

The last two weeks of local FeatureForge session history add one important clarification: the churn is not limited to evidence and receipts.

Across the rename/cutover reviews, test audits, plan review, and runtime audits, the same broader failure modes kept recurring:

1. **Duplicate contract logic keeps drifting across runtime, skills, docs, schema, and scripts.**
   Sessions `019d2656`, `019d2698`, `019d26ae`, and `019d26c3` repeatedly found the same class of bug: one surface had been centralized, but adjacent operator surfaces still carried stale or duplicated logic. Examples included undocumented `_FEATUREFORGE_BIN` / `FEATUREFORGE_COMPAT_BIN` assumptions, upgrade gating that skipped the runtime's own eligibility contract, schema/name drift, and status naming that no longer matched actual freshness semantics.
2. **Coverage theater is not a side issue. It is a major reason churn survives.**
   Sessions `019d227b` and `019d227d` found self-comparison suites, prose-regex checks, meta-tests that only prove files or test names exist, narrow differential harnesses, and wrapper/browser tests that never exercise the claimed behavior. The current runtime audit found the same pattern on the evidence and receipt surfaces. This is a recurring source of false confidence.
3. **Strict invariants are still under-specified away from the final gates.**
   Session `019d2a2b` found that artifact integrity, provenance, and handoff semantics were already being under-translated at spec-to-plan time, before execution even began. The current runtime audit found the same pattern later at operator time: strict validators exist, but public writer and repair surfaces lag behind them.
4. **Public names and control flow still mislead operators about what is really happening.**
   Session `019d2698` found `latest_head_sha` behaving like a stale document-order field instead of real freshness. Session `019d26c3` found session-entry validating message files before bypass decisions that made the message irrelevant. The runtime audit found the same problem in `gate-review-dispatch`, `gate-finish`, and split routing recommendations. This is one pattern: the surface says one thing while the execution path does another.
5. **Supported entrypoint and platform behavior is still fragmented.**
   Sessions `019d2656`, `019d2698`, and `019d26ae` repeatedly found packaged-runtime and Windows-path gaps, hidden environment assumptions, and tests that simulated entrypoints without proving the real packaged path. That is the same abstraction failure in another costume: the supported operator path is not actually owned end to end.

The roadmap was already pointed in the right direction. What the session history adds is stronger evidence that the fix cannot stop at receipt writers and repair commands. It also has to collapse duplicate contract logic, replace coverage theater with behavioral oracles, and make the public surface describe the real control flow.

## Architectural Reset: Supersession-Aware Review Identity

The main design change that follows from this audit is not "keep repairing the old proof model more efficiently." It is to stop treating old per-attempt proof as the main enduring authority surface.

The stronger model is:

- plan checkboxes remain workflow progress, not authority
- runtime-owned closure records become the authoritative proof surface
- each closure record binds one contract identity plus one reviewed state identity
- later reviewed work can supersede earlier reviewed work
- old proof remains historical and auditable, but it stops being current
- post-review unreviewed changes mark the current closure stale instead of forcing in-place proof surgery
- a small intent-level command layer owns the repetitive agent-facing orchestration bundles

That changes the architecture materially:

1. unit-review, task-verification, release-readiness, and final-review markdown artifacts should become derivatives of runtime-owned records, not the primary gate truth
2. per-file content hashes should be demoted from primary long-lived authority to drift diagnostics and repair hints
3. the runtime should reason over an effective current closure set:
   - current
   - superseded
   - stale-unreviewed
   - historical
4. repair should become append-only supersession and reconcile, not proof rewriting
5. the preferred agent-facing surface should converge on:
   - `close-current-task`
   - `repair-review-state`
   - `advance-late-stage`

This does not mean "drop machine-bound identity." It means "stop fingerprinting every attempt forever as if it remains authoritative." The runtime still needs one current machine-checkable answer to "what exact reviewed state are we relying on right now?"

## Component Boundary Implications Of The New Model

The supersession-aware model is not just a data-model change. It demands cleaner ownership than the current runtime has.

The minimum healthy component map is:

1. a pure reviewed-closure domain model
2. reviewed-state / reviewed-surface / contract-input resolvers
3. append-only authoritative stores
4. projection/read-model builders for effective current closure state
5. pure supersession and stale-unreviewed policy
6. recording and reconcile services
7. gate/status query consumers
8. workflow public-contract adapters
9. derived artifact renderers/parsers behind a compatibility boundary

If those are not split, the project will recreate the old failure mode under new names:

- gate logic will become the hidden owner of closure semantics
- artifact rendering will quietly regain authority
- workflow routing will duplicate state transitions
- tests will keep validating oversized mixed surfaces instead of the real policy

The separation-of-concerns rules are straightforward:

- domain and supersession policy must stay pure
- input resolution must not own workflow or gate policy
- stores and projections must not own routing or markdown
- recording/reconcile services must orchestrate mutations without owning CLI copy
- gates and workflow must consume a stable read/query model, not raw store internals
- rendered markdown must be compatibility output, not truth

The testability implications are equally blunt:

- supersession and stale-unreviewed logic must be provable without filesystem or CLI setup
- store/projection behavior must be provable from append-only records
- services must be testable with fake resolvers/stores
- workflow routing must be testable from public review-state fixtures
- end-to-end CLI tests must prove orchestration, not substitute for missing lower-level policy tests

That is the real maintenance/extensibility path. Without those seams, every new milestone type, routing rule, or artifact format will reopen the entire runtime core.

There is one more simplification step the new model should take:

- repeated agent-facing command bundles should be internalized into the runtime through smarter primitives and a small number of intent-level aggregate commands

The most valuable candidates are:

- `close-current-task`
- `repair-review-state`
- `advance-late-stage`

This is not a call for a giant magic command. It is a call to stop making the agent manually stitch together the same runtime-owned sequence in every normal flow.

Implementation rule:

- aggregate commands reduce operator burden
- primitive services remain the authoritative mutation/query seams
- workflow/operator and skills point agents to the aggregate layer first

## Primary Findings

### 1. Task-boundary closure is still enforced across three surfaces, but only one has a public operator path

Task `N+1` begin is still blocked on all of the following:

- review-dispatch lineage
- one dedicated-independent unit-review receipt for each completed step in the prior task
- one task-verification receipt for the prior task

The enforcement still lives in `src/execution/state.rs` via:

- `require_prior_task_closure_for_begin`
- `ensure_prior_task_review_dispatch_closed`
- `ensure_prior_task_review_closed`
- `ensure_prior_task_verification_closed`

The review-dispatch part has a public command:

- `featureforge plan execution gate-review-dispatch --plan ...`

The unit-review and task-verification receipts still do not.

`src/cli/plan_execution.rs` still exposes no public:

- `record-unit-review`
- `record-task-verification`
- `close-task-boundary`

The only discovered writers are still internal helpers in `src/execution/mutate.rs`:

- `refresh_unit_review_receipt_for_step`
- `refresh_task_verification_receipt_for_task`

This remains a real execution dead end.

### 2. The per-task `gate-review` loop is still impossible in the places that matter most

`README.md` still says task closure works like this:

- run `gate-review-dispatch`
- loop fresh-context review until `gate-review` is green
- then close the task

`skills/executing-plans/SKILL.md` and `skills/subagent-driven-development/SKILL.md` still teach the same loop.

The runtime implementation in `src/execution/state.rs::gate_review_from_context_internal` still hard-fails when:

- any active step exists
- any blocked step exists
- any interrupted step exists
- any unchecked step exists

That means `gate-review` is still structurally a whole-plan final-review gate, not a per-task closure signal.

This is not a wording defect. It is a documented procedure that cannot succeed before full plan completion.

### 3. `requesting-code-review` improved, but the improvement is localized

The branch update materially improved `skills/requesting-code-review/SKILL.md`:

- it distinguishes terminal whole-diff review from earlier checkpoint review
- it keeps `gate-review-dispatch` as the dispatch-proof boundary and does not use `gate-review` to mint dispatch lineage
- it now places terminal review after `document-release`

That is real progress.

It does not resolve the broader contract split, because:

- `README.md` still teaches the impossible per-task `gate-review` green loop
- execution skills still inherit that impossible loop
- the runtime still lacks public writers for the receipts required to close a task boundary

The late-stage docs got sharper. The task-boundary docs did not catch up.

### 4. `gate-review-dispatch` is still a mutating checkpoint command with predicate-style naming

`gate-review-dispatch` still does not merely answer a question. It:

1. bootstraps authoritative preflight state
2. records a review-dispatch strategy checkpoint
3. reloads context
4. returns the review gate result

That mutation still lives in `src/execution/state.rs`.

This remains a poor public surface because:

- the name sounds read-only
- the same command is used for both task-boundary dispatch lineage and late-stage review-remediation tracking
- the command mutates runtime truth while being named like a probe

The newer docs now describe the boundary more clearly, but the command shape is still overloaded.

### 5. Late-stage sequencing is better, but it widened the strict unsupported surface

`README.md`, `skills/document-release/SKILL.md`, `skills/requesting-code-review/SKILL.md`, and `skills/finishing-a-development-branch/SKILL.md` now agree on the intended terminal order:

- `document-release`
- terminal final review
- optional QA
- finish gate / branch completion

`src/workflow/operator.rs` and `src/workflow/late_stage_precedence.rs` back this up with explicit precedence:

- `document_release_pending`
- `final_review_pending`
- `qa_pending`
- `ready_for_branch_completion`

This is an improvement over the earlier audit.

The problem is that the runtime now enforces more truth before finish, but it still does not publicly own enough of that truth.

### 6. Release-readiness is now a first-class strict manual artifact surface with no public writer

This is the largest new finding.

`skills/document-release/SKILL.md` still tells the operator to hand-author a project-scoped release-readiness artifact under the runtime state directory.

`src/execution/state.rs::gate_finish_from_context` now validates that artifact strictly:

- exact title `# Release Readiness Result`
- exact approved plan path and revision
- current branch
- repo slug semantics
- current base branch
- current `HEAD`
- `Result: pass`
- `Generated By: featureforge:document-release`

There is still no public runtime writer for this artifact in `src/cli/plan_execution.rs`.

This is the same design error as final review:

- runtime enforces a strict artifact
- the supported path still relies on hand-authored markdown
- tests often fabricate the artifact directly

The only difference is timing. Release-readiness now sits before terminal final review, so the late-stage workflow depends on a manual artifact even earlier.

### 7. Final-review validation is still strict, split, and too manual

`src/execution/final_review.rs` still validates:

- exact title `# Code Review Result`
- exact stage and provenance requirements
- approved reviewer-source vocabulary
- reviewer artifact path and fingerprint binding
- dedicated reviewer artifact filename containing `-independent-review-`
- forbidden self-reference headers in the dedicated reviewer artifact
- exact plan path, plan revision, strategy checkpoint fingerprint, and head SHA

`src/execution/state.rs` still adds more finish-gate checks for:

- branch
- base branch
- repo slug

The skill is better than it was earlier in the day, but the supported authoring path is still manual. There is still no public `record-final-review` command.

The contract is also still layered badly:

- artifact-to-artifact validation lives in `final_review.rs`
- runtime-to-artifact validation lives later in `state.rs`

That guarantees late failures.

### 8. `rebuild-evidence` is still the hidden repair engine, and it now spans release-readiness too

The external investigation context understated one thing: there is a public repair lever.

`featureforge plan execution rebuild-evidence --plan ...` is still public and documented. It is not evidence-only.

In `src/execution/mutate.rs`, it can still:

- replay stale evidence
- refresh task-boundary closure receipts
- refresh downstream truth
- rewrite final-review artifacts
- rewrite QA artifacts
- rewrite release-readiness artifacts

So the runtime already contains repair logic for most of the problem space.

The actual issues are:

- the command name is misleading
- the command shape is overloaded
- the skills do not route operators to it clearly
- the operator has to infer too much about when refresh versus reconcile is the right move

The branch update did not solve that. It merely made release-readiness another surface that can drift.

### 9. Freshness semantics are still internally coherent and publicly confusing

The runtime still intentionally ignores drift for:

- the approved plan file itself
- the execution evidence file itself

when computing `files_proven_drifted` in `src/execution/state.rs::validate_v2_evidence_provenance`.

That is defensible.

The skills still do not explain this model clearly enough, and `latest_head_sha` in status still reads like current Git `HEAD` even though it is derived from the latest completed evidence attempt.

This remains a source of reviewer/runtime disagreement and remediation confusion.

### 10. Workflow public phases are still partially correct and partially stale

The runtime public phases still include:

- `document_release_pending`
- `final_review_pending`
- `qa_pending`
- `ready_for_branch_completion`

The runtime and many tests also now explicitly assert the document-release-before-final-review ordering.

But stale language still exists in active skill/docs/tests:

- `skills/using-featureforge/SKILL.md` still references `review_blocked`
- `skills/plan-eng-review/SKILL.md` still references `review_blocked`
- `tests/runtime_instruction_contracts.rs` still locks `review_blocked`
- `tests/codex-runtime/skill-doc-contracts.test.mjs` still locks `review_blocked`

So the contract is currently split three ways:

- runtime code is mostly current
- some doc-contract tests are current
- other docs and tests still assert dead vocabulary

### 11. Workflow routing now has a clearer late-stage model and a hidden reentry edge

`src/workflow/operator.rs` now computes both `gate_review` and `gate_finish` once execution has started and there are no open steps.

That is good.

It also means the route can say `final_review_pending` while the next action is effectively to return to execution, because `review_requires_execution_reentry` forces reentry when `gate-review` itself is blocked by execution-state truth rather than by a missing terminal review artifact.

That behavior is sensible, but it is subtle. It needs explicit documentation as part of the public contract. Otherwise the operator sees a late-stage phase and still gets bounced back into execution.

### 12. Diagnostics are still too weak for strict receipt repair

Gate diagnostics still expose:

- `code`
- `severity`
- `message`
- `remediation`

That is not enough once the runtime already knows:

- exact allowed reviewer-source values
- exact expected filename shape
- exact expected repo slug
- exact expected base branch
- exact expected fingerprint or path bindings

The reason codes help classify failure. They still do not make one-pass repair realistic.

### 13. Some command surface area is still misleading noise

`rebuild-evidence` still advertises `--max-jobs`, while the implementation in `src/execution/mutate.rs` still rejects anything above `1`.

That is a small issue, but it is representative:

- the surface looks richer than it is
- the operator is asked to reason about machinery that does not really exist

### 14. Core runtime ownership boundaries are still poor

Large files still mix unrelated responsibilities:

- `src/execution/state.rs`
- `src/execution/mutate.rs`
- `src/execution/authority.rs`
- `src/workflow/operator.rs`
- `src/workflow/status.rs`

The main boundary failures remain:

- status assembly mixed with gate logic
- gate logic mixed with receipt validation
- receipt validation mixed with freshness validation
- repair orchestration mixed with downstream artifact rewriting
- workflow routing mixed with cross-worktree borrowing and next-step text

The late-stage precedence helper in `src/workflow/late_stage_precedence.rs` is a move in the right direction, but it does not materially change the broader ownership problem.

### 15. Significant parts of the test suite are still theater, and release-readiness expanded the problem

The earlier audit called out direct unit-review and task-verification receipt fabrication.

That problem still exists. The branch update added or expanded a second late-stage example:

- release-readiness artifacts are often fabricated directly in tests
- final-review artifacts are still frequently fabricated directly in tests
- doc-contract tests still lock stale phrases instead of public runtime semantics

This is valid for narrow parser or validator tests.

It is not valid as primary evidence that the public operator workflow works end to end.

### 16. The external investigation context was directionally right, but it is now incomplete

Verified as materially correct:

- final-review validation is stricter than the old skill contract
- there is no public final-review writer
- `latest_head_sha` is semantically overloaded
- repo slug semantics are easy to miss
- reviewer freshness expectations can diverge from runtime freshness acceptance

Overstated or incomplete:

- there is no repair path at all
- there is no derivative-state rebuild path at all
- release-readiness is not mentioned because that terminal artifact surface was not yet the current branch reality
- the newer `requesting-code-review` docs now do a better job of separating `gate-review` from `gate-review-dispatch`

The real issue is still not total absence. The real issue is opaque, overloaded, non-operator-shaped ownership.

## Runtime Surface Inventory

This section distinguishes the historical runtime surface under audit from the preferred implementation-target surface. The historical inventory is preserved for forensic context only; it is not normative for the April supersession-aware target.

### Historical runtime public operator commands under audit

- `featureforge workflow status`
- `featureforge workflow operator`
- `featureforge plan execution status`
- `featureforge plan execution begin`
- `featureforge plan execution note`
- `featureforge plan execution complete`
- `featureforge plan execution reopen`
- `featureforge plan execution transfer`
- `featureforge plan execution gate-review`
- `featureforge plan execution gate-review-dispatch`
- `featureforge plan execution gate-finish`
- `featureforge plan execution rebuild-evidence`

### Preferred implementation-target operator commands

- `featureforge workflow operator`
- `featureforge plan execution status`
- `featureforge plan execution begin`
- `featureforge plan execution note`
- `featureforge plan execution complete`
- `featureforge plan execution reopen`
- `featureforge plan execution transfer`
- `featureforge plan execution record-review-dispatch`
- `featureforge plan execution close-current-task`
- `featureforge plan execution repair-review-state`
- `featureforge plan execution record-branch-closure`
- `featureforge plan execution advance-late-stage`
- `featureforge plan execution record-qa`
- `featureforge plan execution gate-review`
- `featureforge plan execution gate-finish`
- `featureforge workflow record-pivot`

### Historical Runtime-Owned Surfaces Under Audit

- `~/.featureforge/projects/<repo-slug>/...`
- authoritative harness state
- branch-scoped authoritative artifact bindings
- task-boundary review-dispatch lineage
- unit-review receipts
- task-verification receipts
- release-readiness artifact provenance and freshness truth
- final-review artifact provenance and freshness truth
- downstream QA / test-plan truth overlays

### Human-facing contract surfaces

- `README.md`
- `skills/using-featureforge/SKILL.md`
- `skills/executing-plans/SKILL.md`
- `skills/subagent-driven-development/SKILL.md`
- `skills/document-release/SKILL.md`
- `skills/requesting-code-review/SKILL.md`
- `skills/verification-before-completion/SKILL.md`
- `skills/plan-eng-review/SKILL.md`
- `skills/finishing-a-development-branch/SKILL.md`

### Test surfaces that claim or imply contract coverage

- runtime instruction contract tests
- workflow runtime tests
- plan execution tests
- final-review tests
- workflow shell smoke tests

## Flow Maps

### High-level surface map

```text
repo-visible truth
  docs/featureforge/specs
  docs/featureforge/plans
  docs/featureforge/execution-evidence
        |
        v
workflow/operator
  route -> phase -> next_action
        |
        v
plan execution runtime
  begin / note / complete / reopen / transfer
        |
        +--> ~/.featureforge/... authoritative harness state
        +--> ~/.featureforge/projects/<slug> branch/project artifacts
        +--> gate-review / gate-finish / rebuild-evidence
```

### Task-boundary closure flow as currently enforced

```text
Task N complete
   |
   +--> gate-review-dispatch
   |
   +--> dedicated-independent unit-review receipt for every completed step
   |
   +--> task-verification receipt
   |
   +--> begin Task N+1
         runtime revalidates all prior-task closure surfaces here
```

### Terminal late-stage flow as currently intended

```text
Plan has no open steps
   |
   +--> document-release
   |      writes repo-facing docs
   |      writes release-readiness artifact
   |
   +--> requesting-code-review
   |      writes public final-review artifact
   |      writes dedicated reviewer artifact
   |
   +--> qa-only (when required)
   |
   +--> gate-finish
   |
   +--> finish branch
```

### Terminal precedence and reentry behavior

```text
late-stage signals
  release blocked? review blocked? qa blocked?
        |
        v
late_stage_precedence.rs
  release blocked -> document_release_pending
  else review blocked -> final_review_pending
  else qa blocked -> qa_pending
  else ready_for_branch_completion

special edge:
  final_review_pending + gate-review blocked on execution truth
    -> route back into execution reentry
```

### Recovery flow as currently implemented

```text
review feedback / rebase / drift
   |
   +--> gate-review or gate-finish fails
         files_proven_drifted / stale provenance / late-gate mismatch
   |
   +--> rebuild-evidence
         may replay evidence
         may refresh task-boundary receipts
         may rewrite final-review / QA / release-readiness artifacts
         may restore downstream authoritative truth
```

## Concrete Dead Ends

### Dead end A: next-task begin depends on receipts the operator still cannot publicly write

The runtime blocks progress but does not provide a supported public writer.

### Dead end B: task-boundary docs still tell the agent to wait for `gate-review` to turn green

That cannot happen while unchecked steps remain elsewhere in the plan.

### Dead end C: release-readiness is required before terminal final review, but the supported path is still manual artifact authoring

This is now a first-class late-stage dead end, not a side note.

### Dead end D: final-review artifact repair is strict, but the supported authoring path is still manual

This still creates repeated mismatch loops over path, fingerprint, title, source vocabulary, and repo/base-branch/head bindings.

## Tests That Do Not Really Exercise The Runtime

The following patterns are still high risk:

- helper functions that write authoritative receipts directly under runtime-owned paths
- tests that fabricate release-readiness artifacts instead of using a public writer
- tests that fabricate final-review artifacts instead of using a public writer
- doc-contract tests that assert stale prose strings instead of shared runtime semantics
- workflow tests that validate final state while bypassing public command generation of intermediate artifacts

These tests still have value, but they should not be mistaken for proof that the operator workflow is supported end to end.

## Material Improvements

1. Add a first-class supersession-aware review identity model so the runtime can track one current reviewed closure state instead of preserving every old proof surface as current forever.
2. Make runtime-owned closure records the authoritative gate truth for task closure, release-readiness, and final review; keep markdown receipts as derived human/audit artifacts rather than the primary authority surface.
3. Replace per-task closure gating based on stale packet/file proof permanence with current reviewed task closures that can be superseded automatically by later reviewed work.
4. Replace late-stage release-readiness and final-review markdown synthesis with runtime-owned branch-closure milestone records that bind to the current reviewed branch state.
5. Make blocked validation and dispatch paths fail closed without mutating strategy or cycle state.
6. Replace `rebuild-evidence`-style proof rewriting with append-only supersession and reconcile flows that mark older closures superseded or stale instead of rewriting them in place.
7. Add first-class reconcile and doctor commands that recompute effective current closure state, stale-unreviewed state, and derivable authoritative overlays.
8. Add expected-versus-observed diagnostic payloads for closure mismatch, stale-unreviewed state, and supersession reasons.
9. Make public names and control flow line up with reality, including explicit `workspace_state` versus `current_reviewed_state` semantics.
10. Collapse routing onto one coherent public recommendation contract instead of split `next_skill`, `next_action`, and `recommended_skill` semantics.
11. Replace duplicated operator-side contract logic with runtime-owned helper surfaces or generated references.
12. Align all phase vocabulary to the actual runtime and add an explicit public contract for review-state repair / execution reentry when current reviewed closures are stale or missing.
13. Rewrite the skills so agents are taught current versus superseded versus stale closure semantics and never have to synthesize runtime-owned proof manually.
14. Replace theater-heavy tests with behavioral oracles: CLI-only workflow coverage, supersession scenarios, no self-comparison as a primary oracle, no prose-regex contract tests for behavior, and no fake wrapper/browser/platform coverage.
15. Refactor runtime boundaries so closure recording, supersession, gate evaluation, milestone recording, repair, entrypoint contract logic, and routing logic have clear ownership.

## Bottom Line

FeatureForge is trying to provide safe workflow-guided agent-driven value creation. The trust boundaries are mostly on the right side of strictness. The operator boundary is still not.

After the merge from `main`, the late-stage sequencing is better. The ownership problem is not. The runtime now knows even more, validates even more, and still publicly owns too little of the artifact generation and repair path.

That is why evidence and receipt churn still feels expensive, brittle, and difficult to repair after rebases, review feedback, and late-stage doc updates. The work needed now is not more validation. It is better ownership of the validated surfaces.
