# Execution And Review Skill Contract Hardening

**Workflow State:** Implementation Target  
**Spec Revision:** 3  
**Last Reviewed By:** clean-context review loop
**Implementation Target:** Current

## Problem Statement

The old skills taught agents to think in terms of:

- evidence freshness
- receipt repair
- manual artifact persistence
- task review loops that preserved old proof

The supersession-aware model requires a different operator mental model:

- plan checkboxes are workflow progress
- current reviewed closures are authoritative
- older reviewed closures may be superseded
- post-review edits can make current closures stale
- the runtime owns recording and reconcile surfaces

If the skills do not teach that clearly, agents will keep doing manual artifact surgery against the wrong model.

## Desired Outcome

An agent following the active skills should be able to:

- record task closure
- understand current versus superseded versus stale closure state
- record release-readiness
- record final review
- reconcile review state after later edits
- follow execution reentry correctly

without manual proof synthesis or guessing.

## Decision

Selected approach: rewrite the relevant skills after the reviewed-closure command and phase model stabilizes.

## Dependency

This spec depends on:

- `2026-04-01-supersession-aware-review-identity.md`
- `2026-04-01-execution-task-closure-command-surface.md`
- `2026-04-02-branch-closure-recording-and-binding.md`
- `2026-04-01-release-readiness-recording-and-binding.md`
- `2026-04-01-final-review-recording-and-binding.md`
- `2026-04-02-qa-recording-and-routing.md`
- `2026-04-01-execution-repair-and-state-reconcile.md`
- `2026-04-01-workflow-public-phase-contract.md`

## Requirement Index

- [REQ-001][behavior] Core execution and review skills must describe current reviewed closure recording, not manual receipt persistence, as the supported operator path.
- [REQ-002][behavior] Skills must clearly explain current versus superseded versus stale-unreviewed closure state.
- [REQ-003][behavior] Skills must route operators to runtime-owned record and reconcile commands instead of manual proof surgery.
- [REQ-004][behavior] Skills must explicitly identify runtime-owned records and derived artifacts that agents must not manually edit.
- [REQ-005][behavior] Skills must use the canonical public phase vocabulary and routing recommendations from the workflow contract.
- [REQ-006][behavior] Skills and supporting references must not duplicate packaged-entrypoint, environment-variable, base-branch, or platform-path logic when the runtime already owns that contract.
- [REQ-007][behavior] The strictest surfaces must have concrete examples for supersession, stale-unreviewed repair, release-readiness, and final review.
- [REQ-008][behavior] Normative skill instructions must use definitive RFC-2119 language such as `MUST`, `SHALL`, and `MUST NOT` for required operator actions, and must not rely on ambiguous words such as `should`, `can`, `optional`, or `maybe` for normative flow steps.
- [REQ-009][behavior] Every workflow skill that covers a reviewed-closure stage must include an explicit command matrix keyed by phase or scenario that tells the agent exactly which runtime commands to run and in what order.
- [REQ-010][behavior] The reviewed-closure workflow skills must treat `close-current-task`, `repair-review-state`, and `advance-late-stage` as the primary operator-facing commands for their respective intents, using lower-level primitives only for explicit fallback, compatibility, or debug paths.
- [REQ-011][behavior] Skills must include exact command strings for the preferred aggregate-command path and for the supported fallback primitive path wherever both exist.
- [REQ-012][verification] Skill contract tests must assert the reviewed-closure model and fail closed on reintroduction of manual artifact workflows, stale old vocabulary, or ambiguous command guidance.

## Scope

In scope:

- `skills/using-featureforge`
- `skills/executing-plans`
- `skills/subagent-driven-development`
- `skills/document-release`
- `skills/requesting-code-review`
- `skills/verification-before-completion`
- `skills/plan-eng-review`
- `skills/finishing-a-development-branch`
- supporting examples and references

Out of scope:

- changing runtime behavior directly
- introducing new workflow stages beyond the agreed public contract

## Selected Approach

Teach the skills to:

- treat task and branch closure records as authoritative
- treat markdown receipts as derived artifacts
- recognize when later reviewed work supersedes earlier review
- recognize when unreviewed changes require review-state repair
- invoke the runtime-owned record and reconcile commands
- avoid duplicating runtime-owned contract logic in shell snippets
- treat `record-review-dispatch` as the canonical mutating review-dispatch path, with any `gate-review-dispatch` mention called out as compatibility-only

Use one shared conceptual reference:

- `docs/featureforge/reference/2026-04-01-review-state-reference.md`

Skills should link to that reference for the reviewed-closure mental model, then link to the narrower command specs for the actionable surfaces.

Skills must also:

- define the exact command sequence for each relevant phase
- say when the agent MUST run `featureforge workflow operator --plan <path>`
- say that `featureforge workflow operator --plan <path>` is authoritative for `phase`, `next_action`, and `recommended_command`, while `featureforge plan execution status --plan <path>` is supporting diagnostic detail
- say when the agent MUST run `featureforge plan execution status`
- say when the agent MUST use `record-review-dispatch` for task review and final-review dispatch checkpoints
- say when the agent MUST use `close-current-task`
- say when the agent MUST use `repair-review-state`
- say when the agent MUST use `record-branch-closure`
- say when the agent MUST use `advance-late-stage`
- say when the agent MUST use `record-qa`
- say that agents MUST NOT use the internal task-closure recording service boundary directly and MUST use `close-current-task` for task closure
- say when the agent MAY fall back to `explain-review-state`, `reconcile-review-state`, `record-release-readiness`, or `record-final-review`

## Normative Language Policy

The skill docs are part of the operator contract.

That means:

1. required actions MUST be written with `MUST`, `SHALL`, or `MUST NOT`
2. prohibited actions MUST be written with `MUST NOT`
3. fallback or compatibility-only paths MUST be labeled explicitly as fallback or compatibility-only
4. words like `should`, `can`, `optional`, `maybe`, or `consider` MUST NOT be used for normative execution flow where the runtime expects one clear action

## Required Command-Matrix Shape

Each relevant skill MUST include a compact table or equivalent structured section with at least:

- triggering phase or scenario
- precondition
- exact runtime command or command sequence
- expected runtime signal after the command
- fallback path if the command fails closed

Example categories that MUST be covered where relevant:

- start-of-session orientation
- task execution
- review-dispatch recording
- task closure
- review-state repair
- release-readiness
- branch-closure recording
- final-review dispatch
- final review
- QA
- finish gating

The preferred command families are:

- task closure: `featureforge plan execution close-current-task --plan <path> --task <n>`
- review-state repair: `featureforge plan execution repair-review-state --plan <path>`
- missing branch closure: `featureforge plan execution record-branch-closure --plan <path>`
- late-stage progression: `featureforge plan execution advance-late-stage --plan <path> ...`
- final-review dispatch: `featureforge plan execution record-review-dispatch --plan <path> --scope final-review`
- QA recording: `featureforge plan execution record-qa --plan <path> --result pass|fail --summary-file <qa-report>`
- finish bundle entry: `featureforge plan execution gate-review --plan <path>`

## Acceptance Criteria

1. Skills no longer tell agents to preserve old proof as if it remains authoritative forever.
2. Skills clearly distinguish superseded closure from stale-unreviewed closure.
3. Skills clearly distinguish task closure, release-readiness, and final review.
4. Skills route agents to runtime-owned reconcile flows after later edits instead of manual artifact surgery.
5. Skills explicitly warn against manual edits to runtime-owned records and derived artifacts.
6. Skills share one runtime-owned helper path for entrypoint/base-branch/runtime contract logic.
7. Skills include concrete supersession and stale-review-state examples.
8. Skills link to one shared review-state reference instead of re-teaching the core model inconsistently.
9. Skills use definitive normative language for required actions.
10. Skills include exact command matrices for each reviewed-closure stage they cover.
11. Skills explicitly teach `record-branch-closure` as the required prerequisite when workflow/operator reports `missing_current_closure` for late-stage work.
12. Skills prefer aggregate runtime commands over manual primitive orchestration when those aggregate commands exist.
13. Skills present primitive commands as explicit fallback or debug surfaces rather than as the normal happy path, except for `record-branch-closure`, `record-review-dispatch`, and `record-qa`, which are explicit first-class public recording commands in the contract.

## Test Strategy

- update skill doc-contract tests to assert the reviewed-closure model
- add targeted doc checks for no-manual-edit guidance and supported record/reconcile command references
- add targeted doc checks for superseded versus stale-unreviewed wording
- add targeted doc checks that relevant skills share one canonical runtime-owned helper path for entrypoint/base-branch/runtime contract logic
- add targeted doc checks that normative execution steps use RFC-2119 language instead of ambiguous advisory wording
- add targeted doc checks that phase/scenario command matrices exist and include exact runtime commands
- add targeted doc checks that aggregate-command guidance is preferred when the runtime surface provides it
- add targeted doc checks that the exact preferred command families are named for task closure, repair, branch closure, final-review dispatch, QA, and late-stage progression

## Risks

- rewriting the skills before command and phase semantics stabilize will create immediate drift
- prose-only updates without concrete supersession examples will still leave agents guessing
