# FeatureForge Release Notes

## Unreleased

- internalize normal-path task/final-review dispatch-lineage binding so operator-led `close-current-task` and `advance-late-stage` no longer require public `--dispatch-id`
- remove public normal-path `record-review-dispatch` choreography from active review/execution guidance while keeping the compatibility/debug primitive available off the main path
- align runtime routing, schemas, skill docs, shared review-state reference guidance, and regression coverage on the refactored public command mapping
- keep `plan execution status --json` and `workflow operator --json` on the same runtime-owned routing decision instead of allowing diagnostic/status drift
- harden `rebuild-evidence` as projection-only regeneration that fails closed with append-only/manual-repair blockers instead of rewriting authoritative proof in place
- refresh checked-in repo runtime binaries and darwin/windows prebuilt artifacts so the shipped CLI help matches the refactored command contract

### Breaking Output Contract Changes

- `workflow operator --json`: bump `schema_version` to `2`, add the runtime-owned `base_branch` field for downstream workflow guidance, and change normal task/final-review late-stage guidance so `next_action`/`recommended_command` now point at intent-level `close-current-task` and `advance-late-stage` recording commands instead of public `record-review-dispatch` choreography or required public `--dispatch-id` bindings
- `plan execution status --json`: align the diagnostic surface with the runtime-owned route by exposing the same `harness_phase`, `next_action`/`recommended_command` vocabulary, and late-stage `recording_context` fields while keeping status diagnostic-only

## v1.9.0 - 2026-04-11

Shared-truth convergence release focused on making execution review-state routing, command eligibility, repair/reconcile output, workflow/operator output, and repo/runtime test surfaces derive the same authoritative current state while cutting repeated IO and subprocess overhead out of hot paths.

- centralize reviewed-state selection, routing projection, and command eligibility around shared execution-owned helpers so `workflow operator`, execution status/query surfaces, repair/reconcile, and mutators stop recomputing the same current-truth decisions independently
- fix the April-contract routing regressions where reopened or unfinished task work could still surface as late-stage `document_release_pending`, and make active task execution/task-closure truth outrank branch-closure requirements until late-stage entry is actually valid
- align branch-closure, repair-review-state, final-review, gate, and follow-up override behavior on one authoritative interpretation of current task closures, branch baselines, dispatch currentness, and fail-closed repair reroutes
- harden workflow identity handling so `record-pivot` persists the canonical repo-relative approved-plan path and same-branch worktree adoption refuses detached-worktree state sharing instead of leaking cross-worktree execution truth
- add direct-vs-real CLI parity coverage and broader cross-surface regression coverage for shared routing/follow-up decisions, detached worktree fail-closed behavior, equivalent plan-path spellings, and authoritative review-state projection boundaries
- cut full-suite runtime and test overhead by replacing repeated subprocess/repo-discovery paths with shared in-process helpers, `gix`-backed repo inspection, memoized fixture/runtime helpers, and centralized direct-runtime test support while preserving CLI semantics where the boundary is the contract
- document project-local review/performance expectations in `AGENTS.md` so future reviews treat duplicate truth derivation, repeated immutable IO, avoidable subprocesses, and undocumented direct-vs-real CLI divergence as defects
- refresh checked-in repo runtime binaries and darwin/windows prebuilt artifacts for `1.9.0`

## v1.8.0 - 2026-04-09

Late-stage routing hardening release focused on reviewed-closure authority, exact operator sequencing, and release-ready contract parity across runtime, docs, and tests.

- fail closed when branch-closure truth exists only in overlays, and require authoritative current branch-closure records before release-readiness, final-review, or QA reruns can reuse prior late-stage state
- reroute malformed, plan-mismatched, or authoritative-provenance-invalid final-review artifacts back through fresh final-review dispatch instead of treating those reruns as idempotent success
- preserve negative follow-up overrides for failing final-review and QA reruns while still invalidating stale dispatch lineage for authoritative artifact integrity defects
- align `workflow operator`, `plan execution status`, `document-release`, and branch-finishing guidance on the April late-stage order: document release first, terminal final review second, QA only when operator still requires it, then `gate-review` and `gate-finish`
- harden active runtime/docs contracts for QA `test_plan_refresh_required`, release-blocker recording, supporting `status --plan` diagnostics, and reviewed-closure repair/requery semantics
- expand runtime, shell-smoke, execution-query, and contract coverage for final-review invalidation routing, release/readiness authority, close-current-task follow-up overrides, and stale exact-command fail-closed behavior
- trim remaining shared test binary lookup overhead on the Rust side while keeping the same CLI surface and assertions
- refresh checked-in repo runtime binaries and darwin/windows prebuilt artifacts for `1.8.0`

## v1.7.0 - 2026-04-01

First-class plan-fidelity stage release focused on making `featureforge:plan-fidelity-review` a canonical workflow stage with explicit draft-plan routing ownership, independent-review guidance, and cross-surface contract parity.

- add `featureforge plan execution rebuild-evidence` operator notes covering replayed evidence targets, refreshed helper-owned closure receipts, and the contract-bound versus plain task-boundary unit-review receipt behavior
- split public `featureforge plan execution gate-review` into a read-only gate check and an explicit dispatch-only mutation path for workflow/runtime review-cycle bookkeeping
- let `rebuild-evidence` restore authoritative final-review, test-plan, QA, and release-readiness truth after successful replay, including safe no-op rebinding after rebases when execution evidence is already current
- teach finish readiness to ignore tracked execution-evidence-only writeback so rebuilt evidence does not by itself stale downstream finish gates
- add a first-class `featureforge:plan-fidelity-review` skill surface and reviewer prompt with explicit fresh-context independence and runtime receipt-recording guidance
- route draft-plan fidelity receipt-state blockers (missing, stale, malformed, non-pass, non-independent) to `featureforge:plan-fidelity-review`, while preserving `featureforge:writing-plans` for true authoring defects
- align root/platform docs plus `using-featureforge`, `writing-plans`, and `plan-eng-review` templates/generated docs to the canonical sequence `writing-plans -> plan-fidelity-review -> plan-eng-review`
- expand runtime and instruction contract coverage for first-class stage routing, direct-routing guardrails, and wording parity across active workflow surfaces
- refresh checked-in repo runtime binaries and darwin/windows prebuilt artifacts for `1.7.0`

## v1.6.0 - 2026-03-30

Independent-review dispatch hard-gate release focused on explicit task-boundary review dispatch proof, exact operator guidance, and release ratification for the new execution contract.

- breaking contract delta: remove `featureforge session-entry`, strict gate env exports, and active session-entry schema or CLI surfaces from the supported runtime/docs contract
- workflow routing now ignores legacy session-entry decision files and gate env inputs; `using-featureforge` and `featureforge workflow` route directly from repo-visible artifacts

### Breaking Output Contract Changes

- `workflow phase --json`: remove top-level `session_entry`; remove `phase` values `needs_user_choice` and `bypassed`; remove `next_action` values `session_entry_gate` and `continue_outside_featureforge`; new `schema_version` is `2`
- `workflow doctor --json`: remove top-level `session_entry`; remove `phase` values `needs_user_choice` and `bypassed`; remove `next_action` values `session_entry_gate` and `continue_outside_featureforge`; new `schema_version` is `2`
- `workflow handoff --json`: remove top-level `session_entry`; remove `phase` values `needs_user_choice` and `bypassed`; remove `next_action` values `session_entry_gate` and `continue_outside_featureforge`; new `schema_version` is `2`
- `workflow status --refresh` JSON: remove strict-gate `status` outcomes `needs_user_choice` and `bypassed`; remove strict-gate `reason_codes` `session_entry_unresolved` and `session_entry_bypassed`; retained route `schema_version` is `3`

- enforce explicit `featureforge plan execution record-review-dispatch --plan <approved-plan-path>` dispatch proof at task boundaries before next-task begin can proceed
- keep task-boundary fail-closed behavior for stale or missing dispatch lineage, non-independent review receipts, and missing task verification receipts
- align workflow operator surfaces and execution skill docs on the exact runnable `record-review-dispatch` command text for blocked task-boundary remediation
- harden execution guidance so repo-writing work records runtime begin before mutation and treats backfill as recovery-only workflow repair
- expand runtime, workflow, final-review, and instruction-contract coverage for dispatch hard-gate semantics and preserved final-review behavior
- refresh checked-in repo runtime binaries and darwin/windows prebuilt artifacts for `1.6.0`

## v1.5.0 - 2026-03-29

Project-memory release focused on adding an optional supportive-memory skill and tightening explicit memory routing so workflow authority stays intact.

- add `featureforge:project-memory` with checked-in authority-boundary guidance, examples, and reference templates for `docs/project_notes/*`
- seed `docs/project_notes/` with concise repo-visible memory files and a maintenance README that keeps memory supportive, inspectable, and non-authoritative
- route explicit memory-oriented requests through `using-featureforge` without letting project-memory outrank active workflow owners or approved artifacts
- add narrow project-memory consult hooks to `writing-plans`, `document-release`, and `systematic-debugging` so supportive repo notes can be consulted without turning into a protocol block
- expand Node and Rust contract coverage for project-memory discovery, repo-safety wording, route precedence, provenance, and fail-closed negative cases
- refresh checked-in repo runtime binaries and darwin/windows prebuilt artifacts for `1.5.0`

## v1.4.0 - 2026-03-29

Task-boundary review-gating release focused on mandatory per-task independent review loops, task verification, and execution-phase delegation ergonomics.

- enforce task-boundary `gate-review` checks before each task can close, with fresh-context independent reviewer provenance validation
- block next-task advancement until the current task has a green review result and a recorded task verification receipt
- add runtime-validated review/verification receipt shape checks and status reason-codes for malformed or non-independent task-boundary artifacts
- enforce cycle tracking at task boundaries and fail closed with `task_cycle_break_active` semantics when remediation churn exceeds configured limits
- authorize execution-phase implementation and review subagent dispatch without per-dispatch user-consent prompts once execution has started
- expand workflow/runtime and shell-smoke regressions for task-boundary review gates, stale binding rejection, and final-review coexistence guarantees
- refresh checked-in repo runtime binaries and darwin/windows prebuilt artifacts for `1.4.0`

## v1.3.0 - 2026-03-29

Session-entry gating release focused on strict consent-first routing and thread-scoped entry decisions.

- enforce an optional strict first-entry session gate in workflow status resolution via `FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY=1`
- fail closed before normal workflow routing whenever session entry is unresolved, including explicit `bypassed` handling
- keep session-entry decisions per thread/session key (`FEATUREFORGE_SESSION_KEY`/parent process fallback), not global
- update generated `using-featureforge` helper instructions to export strict gate env and resolve bypass choice before `workflow status --refresh`
- expand workflow/runtime contract tests for strict unresolved, enabled, bypassed, and per-session isolation scenarios
- refresh checked-in repo runtime binaries and darwin/windows prebuilt artifacts for `1.3.0`

## v1.2.0 - 2026-03-28

Execution-runtime hardening release focused on authoritative strategy checkpoints, review-cycle control, and stricter finish-gate provenance contracts.

- route `plan execution recommend` and downstream workflow surfaces through runtime-owned topology/strategy contracts instead of legacy heuristic seams
- add runtime-owned strategy checkpoints (`initial_dispatch`, `review_remediation`, and cycle-break enforcement) with dispatch/reopen tracking and churn guardrails
- require authoritative strategy-checkpoint fingerprint binding in final-review receipts and dedicated reviewer artifacts
- fail closed on authoritative late-gate provenance gaps, including QA `Source Test Plan` symlink-path rejection and stricter canonical artifact checks
- remove legacy pre-harness workflow handoff compatibility paths and tighten fail-closed routing behavior
- expand workflow/runtime/final-review regression coverage for authoritative provenance routing, reviewer binding, and cycle-tracking semantics
- refresh checked-in repo runtime binaries and darwin/windows prebuilt artifacts for `1.2.0`

## v1.1.0 - 2026-03-27

Execution-harness release focused on authoritative workflow truth, durable provenance, and release-ready runtime packaging.

- honor recorded authoritative final-review and downstream finish provenance instead of newer same-branch decoys
- fail `gate-review` closed on stale or missing authoritative late-gate truth
- persist a durable authoritative dependency index on record mutations and fail closed if publishing it breaks
- emit the first production observability sink for authoritative mutations with a persisted counter
- rebuild the repo-root runtime binary and checked-in darwin/windows prebuilt artifacts for `1.1.0`

## v1.0.0 - 2026-03-24

Initial standalone FeatureForge release.

- reset the product version to `1.0.0`
- standardize the supported runtime surface on the canonical `featureforge` binary
- move active skill namespaces to `featureforge:<skill>` and the entry router to `using-featureforge`
- move runtime and install state to `~/.featureforge/`
- move the repo-local default config to `.featureforge/config.yaml`
- preserve historical project documents under `docs/archive/`
- remove wrapper and shim entrypoints from the supported product surface
