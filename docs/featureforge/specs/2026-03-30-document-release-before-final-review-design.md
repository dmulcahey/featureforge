# Document Release Before Final Review

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review
**Implementation Target:** Historical

> **Implementation-target notice:** This approved March spec remains part of the historical record, but it is superseded as the implementation target by the April supersession-aware architecture corpus rooted at [2026-04-01-supersession-aware-review-identity.md](/Users/dmulcahey/development/skills/featureforge/docs/featureforge/specs/2026-04-01-supersession-aware-review-identity.md) and [future-process-explained.md](/Users/dmulcahey/development/skills/featureforge/docs/featureforge/specs/future-process-explained.md). Its late-stage ordering intent still matters, but command surfaces such as `recommended_skill`, `next_skill`, and `gate-review-dispatch` are no longer the normative public contract for the future implementation target.

## Problem Statement

FeatureForge currently permits a common late-stage churn loop:

1. execution completes
2. final code review runs
3. `document-release` updates release-facing repo content
4. the earlier review becomes stale because repo contents changed
5. final code review must run again

This is not a freshness-policy bug. Freshness should remain strict and fail closed. The issue is ordering: required release-facing repo writes should happen before the final repo-write-sensitive review gate.

## Desired Outcome

FeatureForge late-stage routing should follow this normative sequence for workflow-routed work:

```text
execution complete
  -> document_release_pending
  -> final_review_pending
  -> qa_pending (if required)
  -> ready_for_branch_completion
```

This sequence applies once execution is in a clean late-stage state (no active, blocking, or resume task and no unresolved task-boundary blocker family signaled by runtime `task_boundary_block_reason_code`, including review-not-green, review-independence/receipt-integrity, review-dispatch-lineage, and cycle-break blockers).

The normal path should no longer encourage "review -> required doc update -> review again" churn.

## Scope

In scope:

- reorder workflow late-stage precedence so `document_release_pending` is surfaced before `final_review_pending` when release-facing artifacts are unresolved
- align public routing outputs (`recommended_skill`, `next_action`, reason text, phase summaries) with the reordered sequence
- update runtime and harness-facing phase translation where ordering assumptions are encoded
- update late-stage skill guidance to reflect the new normative order
- preserve QA sequencing after final review unless a separate design changes that policy
- add regression coverage for routing precedence and stale-review-loop prevention
- add observability signals that show whether document-release-first routing is occurring and whether final review is being invalidated by post-review repo writes
- add a routing-consistency matrix that verifies phase, `next_action`, `recommended_skill`, and `recommendation_reason` together across mixed late-stage stale-artifact states
- add a terminal-final-gate-only guard so workflow-routed final review cannot be treated as terminal when release readiness is stale or missing, while preserving early checkpoint/ad-hoc review use
- preserve the current review-dispatch command boundary (`gate-review-dispatch` mints dispatch proof; `gate-review` remains read-only status)
- define one authoritative late-stage precedence decision table used by runtime tests and late-stage guidance text

## Out of Scope

- weakening final-review freshness rules
- weakening finish-gate law
- moving browser QA ahead of final review
- requiring `document-release` for ad-hoc non-workflow work
- blocking non-terminal `requesting-code-review` checkpoints behind `document-release`
- changing task-boundary review-dispatch/cycle-break policy
- collapsing `gate-review-dispatch` and `gate-review` into one command path
- broad workflow redesign beyond late-stage precedence

## Requirement Index

- [REQ-001][behavior] Workflow-routed late-stage order must be `execution complete -> document_release_pending -> final_review_pending -> qa_pending (if required) -> ready_for_branch_completion`.
- [REQ-002][behavior] Public phase selection must prefer `document_release_pending` over `final_review_pending` when release-facing docs or metadata are the earliest unresolved required late-stage artifact.
- [REQ-003][behavior] `featureforge:finishing-a-development-branch` must describe and enforce the reordered sequence.
- [REQ-004][behavior] `featureforge:requesting-code-review` must be the default last repo-write-sensitive late-stage gate in the normative path, and workflow-routed final-review dispatch must continue using `featureforge plan execution gate-review-dispatch` while `gate-review` remains read-only.
- [REQ-005][behavior] Final-review freshness must remain fail closed when repo contents change after review completion.
- [REQ-006][behavior] QA freshness must only surface after release docs and final review are current.
- [REQ-007][behavior] `featureforge:document-release` must explicitly state that workflow-routed work normally runs before final review.
- [REQ-008][behavior] Public docs and workflow summaries must not describe final review as the normative predecessor of required document release.
- [REQ-009][verification] Runtime tests must fail if routing regresses to review-first precedence when release docs are unresolved.
- [REQ-010][verification] Skill/doc contract tests must fail if generated guidance reintroduces the old order.
- [REQ-011][observability] Runtime/operator surfaces must emit deterministic observability signals for late-stage precedence outcomes, including reason-coded counts for `document_release_pending` routing before final review and for post-final-review repo writes that invalidate review freshness.
- [REQ-012][verification] A routing-consistency matrix must assert phase, `next_action`, `recommended_skill`, and `recommendation_reason` parity for mixed stale-artifact combinations across release docs, final review, and QA.
- [REQ-013][behavior] In workflow-routed terminal final-review mode, `featureforge:requesting-code-review` must fail closed as non-terminal when release readiness is stale or missing and must route back to `featureforge:document-release`; ad-hoc or intentionally early checkpoint review use remains allowed.
- [REQ-014][behavior] FeatureForge must define a single authoritative late-stage precedence decision table (`artifact/failure -> phase -> next_action -> recommended_skill -> reason-family`) and align runtime behavior, tests, and workflow-facing guidance to that table.
- [REQ-015][verification] Tests must prove `requesting-code-review` remains valid for intentional non-terminal checkpoint use, is not universally blocked by document-release prerequisites, and does not regress the `gate-review-dispatch` vs `gate-review` command boundary.
- [REQ-016][behavior] The authoritative late-stage precedence decision table must be runtime-owned (source of truth in workflow runtime code), and skills/docs must reference a shared canonical table representation derived from that runtime source rather than maintaining divergent ad-hoc copies.
- [REQ-017][verification] Contract tests must fail when workflow-facing guidance (skill/public doc precedence statements) diverges from the runtime-owned precedence table values.
- [REQ-018][behavior] Authoritative harness-phase emission and operator public-phase routing must both consume the same canonical late-stage precedence contract so `harness_phase` and operator-visible phase/action/skill outputs cannot diverge on stale-artifact precedence.
- [REQ-019][verification] Tests must fail when authoritative `harness_phase` and operator fallback routing produce inconsistent late-stage precedence outcomes for the same execution/gate evidence.
- [REQ-020][behavior] The spec must carry an explicit system architecture/dependency diagram that shows canonical precedence ownership and dataflow across runtime producer surfaces (`execution/state` and harness), operator routing surfaces, and workflow-facing guidance consumers (skills/docs).
- [REQ-021][behavior] Late-stage precedence evaluation failures must fail closed with named failure classes and reason codes, and must route to deterministic recovery actions instead of defaulting to an optimistic review-ready state.
- [REQ-022][verification] Tests must cover fail-closed behavior for precedence-table load/evaluation errors, harness/operator precedence divergence detection, and terminal final-review attempts blocked by stale release readiness.
- [REQ-023][behavior] Late-stage routing and terminal final-review gating must trust only authoritative release-artifact provenance; decoy, stale, malformed, or non-authoritative release artifacts must fail closed and cannot satisfy document-release readiness.

## Selected Approach (Option A)

Reorder late-stage phase precedence while keeping freshness law unchanged.

Why this approach:

- removes an avoidable loop without relaxing quality gates
- keeps final review as the last repo-write-sensitive verification checkpoint
- preserves existing QA and branch-completion semantics

## Current Repo Seams

The repo already has the right surfaces for this change:

- `src/workflow/operator.rs` controls public phase routing, next action, next skill, and reason text
- `src/execution/state.rs` and `src/execution/harness.rs` map runtime state to public/harness-facing phase diagnostics
- `src/execution/state.rs` already enforces task-boundary review-dispatch lineage (`prior_task_review_dispatch_missing|stale`) before next-task advancement and emits explicit remediation guidance through operator repairing flows
- `skills/finishing-a-development-branch/SKILL.md.tmpl` already acknowledges stale-review risk from document changes after review
- `skills/document-release/SKILL.md.tmpl` and `skills/requesting-code-review/SKILL.md.tmpl` define the late-stage skill contract surfaces that must be explicitly ordered, including the workflow-routed `gate-review-dispatch` pre-dispatch gate for final review
- `tests/workflow_runtime.rs`, `tests/workflow_runtime_final_review.rs`, and `tests/execution_harness_state.rs` already cover related phase behavior and can anchor precedence regressions

## Architecture Diagram

```text
                              +----------------------------------+
                              | runtime-owned canonical late-    |
                              | stage precedence table           |
                              | (single source of truth)         |
                              +----------------+-----------------+
                                               |
                       +-----------------------+-----------------------+
                       |                                               |
                       v                                               v
     +----------------------------------+             +----------------------------------+
     | execution status / harness phase |             | workflow operator routing        |
     | producers (state + harness)      |             | (phase, next_action, skill,     |
     | consume canonical precedence     |             | reason) consume same table       |
     +----------------+-----------------+             +----------------+-----------------+
                      |                                                |
                      +----------------------+-------------------------+
                                             |
                                             v
                         +-----------------------------------------+
                         | runtime/public outputs                  |
                         | - phase                                |
                         | - next_action                          |
                         | - recommended_skill                    |
                         | - recommendation_reason                |
                         +----------------+------------------------+
                                          |
                                          v
                        +------------------------------------------+
                        | workflow-facing docs/skills              |
                        | reference canonical precedence artifact  |
                        | (no divergent duplicated logic)          |
                        +------------------------------------------+
```

## Runtime Contract Changes

### 1. Public Phase Precedence in `src/workflow/operator.rs`

After execution has no open steps, public routing behavior must act as if late-stage checks run in this order:

1. release-doc/readiness check -> `document_release_pending` if missing or stale
2. final-review freshness check -> `final_review_pending` if missing or stale
3. QA freshness check -> `qa_pending` if required and missing or stale
4. all late-stage artifacts fresh -> `ready_for_branch_completion`

Implementation can keep internal helper structure flexible, but observed public behavior must match this precedence.

### 2. User-Facing Routing Outputs

When late-stage routing resolves:

- `recommended_skill` must point to `featureforge:document-release` for `document_release_pending`
- `next_action` and `recommendation_reason` must explain document-release-first precedence
- public phase summaries and handoff text must reflect the same ordering
- mixed stale-state behavior must follow one shared precedence decision table so phase/action/skill/reason fields stay mutually consistent

### 3. Runtime/Harness Naming Consistency

If ordering assumptions exist in `src/execution/state.rs` or `src/execution/harness.rs`, update them so diagnostics and phase surfaces align with the new normative sequence.

### 4. Terminal Final-Review Guard Semantics

For workflow-routed terminal final-review mode only:

- if release-readiness artifacts are stale or missing, final review cannot be treated as terminal/fresh and must route to document release
- this guard is not a blanket restriction on `featureforge:requesting-code-review`
- intentional earlier checkpoint reviews remain allowed

### 4.5. Review-Dispatch Command Boundary Compatibility

- preserve the current split where `featureforge plan execution gate-review-dispatch` records review-dispatch provenance and strategy remediation checkpoints
- preserve `featureforge plan execution gate-review` as read-only gate inspection for status/reporting surfaces
- late-stage guidance and routing copy must not regress to treating `gate-review` as the dispatch-minting command for workflow-routed final review

### 5. Observability

Add deterministic observability for:

- count of transitions/routing outcomes that prioritize `document_release_pending` over `final_review_pending`
- count of post-review repo mutations that invalidate final-review freshness
- reason-coded visibility linking stale-artifact class to selected late-stage phase and next action
- provenance diagnostics that distinguish authoritative release-artifact acceptance from decoy/non-authoritative artifact rejection

### 6. Canonical Precedence Table Ownership

- implement a runtime-owned precedence matrix helper as the single source of truth
- expose a canonical table representation for workflow-facing guidance consumption
- keep skills/docs grounded by referencing that canonical table representation instead of duplicating precedence logic in prose
- require both authoritative harness-phase producers and operator routing surfaces to resolve precedence through that same canonical contract

## Error & Rescue Registry

| Method / Codepath | What Can Go Wrong | Failure Class | Reason Code | Rescued? | Required Recovery / Route | User Sees |
|---|---|---|---|---|---|---|
| canonical precedence matrix resolver | canonical table missing/malformed | `MalformedWorkflowState` | `late_stage_precedence_table_invalid` | Y | fail closed to deterministic remediation route, require precedence table repair before terminal gating | explicit workflow error + remediation step |
| authoritative harness-phase emission | emitted phase conflicts with canonical precedence for same gate evidence | `ExecutionStateNotReady` | `late_stage_phase_precedence_conflict` | Y | block terminal routing, recompute from canonical table, require consistency before proceeding | explicit mismatch diagnostic |
| operator late-stage routing | release stale + review stale both unresolved but phase chooses final review first | `ExecutionStateNotReady` | `release_precedence_violation` | Y | force `document_release_pending`, emit precedence-violation diagnostic | document-release-first next action |
| terminal final review guard | workflow-routed terminal final review requested while release readiness stale/missing | `ExecutionStateNotReady` | `terminal_review_blocked_release_not_ready` | Y | route to `featureforge:document-release`, mark review non-terminal | clear “run document-release first” message |
| release-artifact provenance validator | decoy/non-authoritative release artifact presented as fresh release readiness | `ExecutionStateNotReady` | `release_artifact_provenance_invalid` | Y | reject artifact, require authoritative document-release artifact/regeneration | explicit provenance-invalid diagnostic |
| freshness invalidation detector | repo changed after recorded final review artifact | `ReviewArtifactNotFresh` | `post_review_repo_write_detected` | Y | invalidate prior final review artifact and require fresh final review on current `HEAD` | review stale message + rerun instruction |
| task-boundary review-dispatch lineage gate | prior task review-dispatch provenance missing/stale while advancing execution | `ExecutionStateNotReady` | `prior_task_review_dispatch_missing`, `prior_task_review_dispatch_stale` | Y | route to repairing guidance and require `gate-review-dispatch` before next-task begin | explicit task-boundary remediation command |

## Failure Modes Registry

```text
CODEPATH                                  | FAILURE MODE                                           | RESCUED? | TEST? | USER SEES? | LOGGED?
------------------------------------------|--------------------------------------------------------|----------|-------|------------|--------
precedence-table evaluation               | table missing/malformed                                | Y        | Y     | Explicit   | Y
authoritative harness/operator alignment  | phase/action/skill/reason mismatch for same evidence   | Y        | Y     | Explicit   | Y
terminal final review gate                | release readiness stale/missing                        | Y        | Y     | Explicit   | Y
post-review freshness                     | repo mutation after review artifact                    | Y        | Y     | Explicit   | Y
release-artifact provenance               | non-authoritative/decoy artifact accepted             | Y        | Y     | Explicit   | Y
task-boundary dispatch lineage            | review-dispatch proof missing/stale before next task  | Y        | Y     | Explicit   | Y
```

## Skill and Documentation Changes

### `skills/finishing-a-development-branch/SKILL.md.tmpl`

Required updates:

- verify and, where needed, make `document-release` the required late-stage repo-facing pass before final review for workflow-routed work
- reorder guidance so this sequencing is explicit and normative
- keep the warning that any repo write after final review stales that review
- keep browser QA positioned after final review

### `skills/document-release/SKILL.md.tmpl`

Required updates:

- verify and, where needed, explicitly state it normally runs before final review in workflow-routed work
- mark its output as a required late-stage artifact in the normative path
- state that post-review repo changes stale prior review and require a fresh review

### `skills/requesting-code-review/SKILL.md.tmpl`

Required updates:

- verify and, where needed, state it is the default final repo-write-sensitive late-stage gate
- assume release-facing docs and metadata are already current in the normative path
- state that newly discovered required release/doc repo writes make the review non-final until those writes land and review is rerun
- explicitly scope terminal guard behavior to workflow-routed terminal final-review mode so intentional non-terminal checkpoint reviews remain valid
- preserve the `gate-review-dispatch` (dispatch proof) vs `gate-review` (read-only status) command boundary in guidance text and examples

### Public Workflow Docs

Update any workflow descriptions that still imply review-first precedence, including:

- `README.md`
- `docs/README.codex.md`
- `docs/README.copilot.md`
- `skills/using-featureforge/SKILL.md.tmpl` and generated output where late-stage routing is summarized
- shared precedence reference document consumed by late-stage skills so operator guidance stays grounded in runtime truth

## File Touch Points

- `src/workflow/operator.rs`
- `src/execution/state.rs`
- `src/execution/harness.rs`
- `skills/finishing-a-development-branch/SKILL.md.tmpl` and generated `skills/finishing-a-development-branch/SKILL.md`
- `skills/document-release/SKILL.md.tmpl` and generated `skills/document-release/SKILL.md`
- `skills/requesting-code-review/SKILL.md.tmpl` and generated `skills/requesting-code-review/SKILL.md`
- `skills/using-featureforge/SKILL.md.tmpl` and generated `skills/using-featureforge/SKILL.md`
- `README.md`
- `docs/README.codex.md`
- `docs/README.copilot.md`
- canonical late-stage precedence reference artifact used by docs/skills

## Test Plan

### Runtime Phase Tests

Add or update tests to prove:

- execution complete + stale/missing release docs -> `document_release_pending`
- execution complete + fresh release docs + stale/missing review -> `final_review_pending`
- execution complete + fresh release docs + fresh review + stale/missing QA -> `qa_pending`
- all late-stage artifacts fresh -> `ready_for_branch_completion`
- direct terminal final-review invocation with stale release readiness fails closed back to `document_release_pending`
- non-terminal checkpoint review invocations remain allowed
- workflow-routed final-review dispatch path uses `gate-review-dispatch` and does not regress to `gate-review` as the dispatch-minting command

### Precedence Tests

Add or update tests to prove:

- release-doc failures surface before final-review failures when both are unresolved
- QA freshness only surfaces after release docs and final review are current
- authoritative release provenance checks still defeat decoy artifacts
- phase, action, recommended skill, and recommendation reason stay aligned to one precedence table across mixed stale states

### Skill/Doc Contract Tests

Add tests that fail if generated skill/docs describe old ordering such as:

- final review before required document release in normative workflow
- completion guidance that frames document release as optional post-review cleanup for workflow-routed work
- wording that makes checkpoint/ad-hoc `requesting-code-review` universally blocked by document-release
- wording or examples that collapse `gate-review-dispatch` and `gate-review` semantics
- precedence mappings in workflow-facing docs/skills that diverge from runtime-owned canonical table values

### End-to-End Regression

Add one explicit regression that demonstrates the improvement:

1. start from execution-complete state with stale release docs and no final review
2. route to `featureforge:document-release`
3. perform release-facing repo updates
4. route to `featureforge:requesting-code-review`
5. verify normative flow does not require a second review caused by its own required doc pass

## Acceptance Criteria

- public routing surfaces `document_release_pending` before `final_review_pending` when release docs are unresolved
- `featureforge:finishing-a-development-branch` describes and enforces document-release-first sequencing
- `featureforge:requesting-code-review` is the default final repo-write-sensitive late-stage gate
- final-review freshness remains strict and fail closed
- the old normative stale-review loop is removed by ordering, not by policy relaxation
- routing consistency matrix passes for phase/action/skill/reason parity
- terminal final-review guard enforces release-readiness-first without breaking intentional non-terminal review checkpoints

## Risks and Mitigations

- Risk: precedence changes are only partially applied, causing inconsistent phase outputs.
  - Mitigation: assert phase, next skill, next action, and reason text together in runtime tests.
- Risk: docs and generated skills drift from runtime behavior.
  - Mitigation: update `.tmpl` sources and regenerated `SKILL.md` outputs in the same change, with contract tests.
- Risk: users misread this as relaxed freshness.
  - Mitigation: keep explicit stale-review warning text in both `document-release` and `requesting-code-review`.

## Rollout and Rollback

- Rollout as a single contract slice: runtime routing + skill templates + regenerated skill docs + tests.
- Validate targeted runtime and contract suites before merge.
- If regressions appear, rollback by reverting precedence changes while preserving existing freshness enforcement.

## What Already Exists

- workflow/runtime already has explicit late-stage phases (`final_review_pending`, `qa_pending`, `document_release_pending`, `ready_for_branch_completion`)
- skill layer already partially encodes release-readiness-before-completion behavior in `document-release` and `finishing-a-development-branch`
- test suites already cover substantial late-stage routing and can be extended for strict precedence/parity assertions

## NOT in Scope

- introducing new workflow stages beyond existing late-stage phases
- relaxing final-review freshness or finish-gate strictness
- forcing `document-release` for non-workflow ad-hoc reviews
- changing QA-after-final-review policy in this slice
- broad redesign of execution or branch-completion workflow beyond precedence law and guard hardening

## Dream State Delta

```text
CURRENT STATE
- Late-stage precedence is partially encoded across runtime and skill surfaces.
- Document-release-first intent exists in places, but can still drift by surface.
- Review freshness is strict, but avoidable stale-review loops are still possible.

THIS SPEC
- Establishes one runtime-owned canonical precedence contract.
- Forces harness/operator/skill-doc alignment to that same contract.
- Adds fail-closed terminal guard semantics and explicit provenance protections.
- Adds parity and observability tests to detect drift quickly.

12-MONTH IDEAL
- Late-stage precedence is fully contract-driven, generated-consistent, and drift-resistant.
- Operators see deterministic phase/action/skill/reason outputs across all surfaces.
- Stale-review churn is rare and measurable; regressions are caught automatically.
- Workflow guidance stays grounded in runtime truth without manual reconciliation.
```

## CEO Review Summary

**Review Status:** clear
**Reviewed At:** 2026-03-30T11:59:20Z
**Review Mode:** selective_expansion
**Reviewed Spec Revision:** 1
**Critical Gaps:** 0
**UI Design Intent Required:** no
**Outside Voice:** skipped
