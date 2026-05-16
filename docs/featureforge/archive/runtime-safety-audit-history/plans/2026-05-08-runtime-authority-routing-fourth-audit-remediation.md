# Runtime Authority and Routing Fourth-Audit Remediation

## Workflow State

Engineering Approved

## Plan Revision

1

## Execution Mode

featureforge:executing-plans

## Goal

Eliminate the remaining fourth-audit actionable issues so FeatureForge agents can rely on runtime-owned state and one-way public route decisioning without being pulled into evidence/proof-artifact repair or split routing semantics.

## Architecture

Runtime authority remains event-log and transition-state based:

1. CLI args enter public command modules.
2. Command modules enforce transition guards and append events.
3. Reducer builds authoritative runtime state.
4. Read models project status/operator data from shared decisions.
5. Public route selection and workflow operator present typed argv/templates.
6. Evidence, legacy proof artifacts, markdown, and projections are diagnostic/read-model surfaces unless a current runtime-owned record explicitly owns a gate.

This plan keeps that architecture and tightens the remaining weak spots:

- Review gates must derive step completion from authoritative transition state, not execution evidence projection.
- Legacy plain unit-review proof artifacts must not block review gates in contractless/no-active-lease fallback paths.
- Public-route seed selection must not depend cyclically on `router.rs`.
- Branch-closure refresh routing must use shared current-truth predicates and boundary tests.

## Change Surface

- `src/execution/read_model_support.rs`
- `src/execution/state/review_gate.rs`
- `src/execution/state/runtime_methods.rs`
- `src/execution/state/worktree_lease_truth.rs`
- `src/execution/state/unit_review_truth.rs` if still needed after removing the fallback
- `src/execution/public_route_selection.rs`
- `src/execution/router.rs`
- `src/execution/current_truth.rs`
- `tests/internal_plan_execution.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs` if new public-output scanner coverage is needed
- Checked-in prebuilts under `bin/` after Rust behavior changes

## Preconditions

- Work from `/Users/dmulcahey/.codex/worktrees/5d19/featureforge`.
- Do not run FeatureForge runtime/project skills.
- Use Rust skills guidance for Rust changes.
- Do not allow review subagents to spawn subagents.
- Preserve public command authority through typed argv/templates, not display strings.
- Preserve event-log/transition-state authority.
- Preserve existing review gates where they are backed by current runtime-owned records.

## Known Footguns / Constraints

- `context.evidence` may be synthesized from authoritative state, parsed from a state-dir projection, parsed from tracked markdown, or empty. Gate code must distinguish authoritative completion from evidence projection content.
- Demoting evidence projection from gate authority must not allow active/blocked/interrupted or unchecked approved steps through final review.
- Demoting legacy plain unit-review proof artifacts must not weaken active worktree lease binding truth or serial unit-review proof where an active contract is authoritative.
- Moving next-action decision helpers must not change public route JSON shape.
- Breaking the router/public-route cycle must not reintroduce string-parsed public command execution.
- Boundary tests should forbid the regression shape, not only assert the new helper exists.
- Any Rust source change can stale checked-in prebuilts; refresh and verify them before the relevant task review if the task changes shipped runtime behavior.

## Requirement Coverage Matrix

| Requirement | Task |
| --- | --- |
| Execution evidence cannot block final review after authoritative completion exists | Task 1 |
| Review dispatch/final review completion checks use authoritative completion when present | Task 1 |
| Legacy plain unit-review proof artifacts are diagnostic-only in no-contract fallback | Task 1 |
| Active worktree lease and serial unit-review authority remain enforced | Task 1 |
| Router/public-route cycle is removed | Task 2 |
| Branch-closure refresh predicate is centralized | Task 2 |
| Boundary tests catch route ownership regressions | Task 2 |
| Generated docs, tests, clippy, nextest, liveness, and prebuilts are clean | Tasks 1-3 |

## Tasks

### Task 1: Demote execution evidence and legacy plain unit-review proof artifacts from review-gate authority

#### Spec Coverage

- Proof-artifact/evidence/projection control-plane assessment.
- Execution runtime checklist: legacy proof-artifact/projection diagnostics do not trigger reentry.
- Evidence/projection checklist: evidence is audit/projection, not control plane.

#### Goal

Make final-review and task-review gate completion checks use authoritative transition-state completion when available, while treating missing/stale execution evidence projection and legacy plain unit-review proof drift as diagnostic warnings only.

#### Context

`gate_review_base_result` currently derives authoritative completed steps and then fails if `context.evidence` lacks matching completed attempts. `task_review_dispatch_gate_from_context` has the same evidence requirement. `enforce_worktree_lease_binding_truth` calls `enforce_plain_unit_review_truth` in the no-active-contract fallback, allowing legacy proof artifacts to block review gates.

#### Constraints

- Keep active, blocked, interrupted, and unchecked step failures authoritative.
- If authoritative transition state is missing while local execution progress exists, keep failing closed.
- Do not remove serial unit-review enforcement for active-contract paths in this task.
- Do not let worktree lease artifacts become advisory; only demote legacy plain unit-review proof fallback.
- Use warnings or diagnostic-only fields for stale evidence/proof-artifact projection drift.

#### Done When

- `gate_review_base_result` no longer fails with `checked_step_missing_evidence` solely because evidence projection is missing/stale when authoritative completed-step state exists.
- `task_review_dispatch_gate_from_context` follows the same authoritative completion rule.
- The no-active-contract branch in `enforce_worktree_lease_binding_truth` no longer calls `enforce_plain_unit_review_truth`.
- Unit tests that previously asserted plain proof-artifact gate failure are updated to assert diagnostic-only behavior.
- A public-shell regression proves final-review recording can proceed after authoritative completion/release readiness even when execution evidence attempts are stale or empty.
- Active worktree lease and serial unit-review tests still pass.

#### Files

- `src/execution/read_model_support.rs`
- `src/execution/state/review_gate.rs`
- `src/execution/state/runtime_methods.rs`
- `src/execution/state/worktree_lease_truth.rs`
- `tests/internal_plan_execution.rs`
- `tests/workflow_shell_smoke.rs`

#### Implementation Steps

1. Move or add a shared helper that derives completed `(task, step)` pairs from `AuthoritativeTransitionState`.
2. Refactor `gate_review_base_result` to call that helper and classify completion authority separately from evidence projection content.
3. When authoritative completion is present, demote missing/non-completed evidence attempts for completed steps to warning codes.
4. Keep existing fail-closed behavior when authoritative completion state is unavailable but local progress markers or evidence attempts exist.
5. Refactor `task_review_dispatch_gate_from_context` to use the same helper.
6. Replace the no-active-contract `enforce_plain_unit_review_truth` fallback with diagnostic-only warning behavior or no-op behavior.
7. Ensure worktree lease artifact existence checks no longer treat unit-review proof files alone as reason to fail missing authoritative lease state.
8. Update or replace tests that asserted old proof/evidence control-plane behavior.
9. Add a public compiled-CLI regression for final-review progression with stale/empty evidence projection.

#### Validation Expectations

- `cargo test --test internal_plan_execution gate_review -- --nocapture`
- `cargo test --test workflow_shell_smoke final_review -- --nocapture`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- Full task gate before review:
  - `node scripts/gen-skill-docs.mjs --check`
  - `node scripts/gen-agent-docs.mjs --check`
  - `node --test tests/codex-runtime/*.test.mjs`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo nextest run --all-targets --all-features --no-fail-fast`
  - refresh checked-in prebuilts if source fingerprint changes, then `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .`

### Task 2: Break router/public-route cycle and centralize branch-closure refresh decisioning

#### Spec Coverage

- Modularization and split-decisioning assessment.
- Public CLI/reachability: typed public command authority remains executable source of truth.
- Runtime modules have clear state-machine boundaries.

#### Goal

Make public route seed selection one-way and centralize branch-closure refresh predicates so router, public-route projection, and read model do not rederive the same semantic question independently.

#### Context

`router.rs` imports `shared_next_action_seed_from_runtime_state` from `public_route_selection.rs`, while `public_route_selection.rs` calls `router::shared_next_action_decision*`. `public_route_selection.rs::stale_branch_closure_refresh_required` duplicates the current-truth branch-closure refresh predicate.

#### Constraints

- Do not change public JSON field names or typed argv/template behavior.
- Keep `router.rs` as the `PublicRouteDecision` materialization owner.
- Keep `public_route_selection.rs` focused on converting shared next-action decisions into workflow-routing seeds.
- Do not reintroduce display-string execution.

#### Done When

- `public_route_selection.rs` no longer imports or calls into `router.rs`.
- `router.rs` may call `public_route_selection.rs`, but the reverse dependency is gone.
- Shared next-action decision helpers are owned outside `router.rs` or fully inside `public_route_selection.rs`.
- `public_route_selection.rs` uses `current_truth::branch_closure_refresh_missing_current_closure` or a shared helper, not its own local stale branch-closure predicate.
- Boundary tests fail if the router/public-route cycle returns.
- Boundary tests fail if a local `stale_branch_closure_refresh_required` predicate returns.

#### Files

- `src/execution/public_route_selection.rs`
- `src/execution/router.rs`
- `src/execution/current_truth.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/execution_query.rs` or other route parity tests if needed

#### Implementation Steps

1. Move `shared_next_action_decision` and `shared_next_action_decision_from_runtime_state` out of `router.rs` into `public_route_selection.rs`, or into a new focused helper module consumed by both.
2. Update imports so `router.rs` depends one-way on the new owner and the owner does not reference `router.rs`.
3. Replace `stale_branch_closure_refresh_required` in `public_route_selection.rs` with the shared current-truth helper.
4. If the shared helper needs a clearer name or slightly broader semantics, update it once in `current_truth.rs` and keep read-model/public-route call sites aligned.
5. Add `runtime_module_boundaries` tests that scan for forbidden `crate::execution::router::` references in `public_route_selection.rs`.
6. Add boundary coverage that rejects a local `fn stale_branch_closure_refresh_required` or equivalent predicate in public-route selection.
7. Run route parity tests and update goldens only if public output legitimately changes.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test workflow_shell_smoke workflow_operator_routes_document_release_pending_to_record_branch_closure -- --nocapture`
- `cargo test --test execution_query -- --nocapture`
- Full task gate before review:
  - `node scripts/gen-skill-docs.mjs --check`
  - `node scripts/gen-agent-docs.mjs --check`
  - `node --test tests/codex-runtime/*.test.mjs`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo nextest run --all-targets --all-features --no-fail-fast`
  - refresh checked-in prebuilts if source fingerprint changes, then `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .`

### Task 3: Final artifact freshness, docs, and audit-loop readiness

#### Spec Coverage

- Generated docs and prompt surface packaging.
- Runtime binary/prebuilt freshness.
- Full validation and clean-context review loop.

#### Goal

Ensure all generated docs, schemas/goldens, checked-in prebuilts, and validation evidence are clean after Tasks 1 and 2.

#### Context

The previous implementation passed source validation but initially left stale checked-in runtime binaries. This task exists to prevent that failure class from recurring.

#### Constraints

- Do not hand-edit generated `SKILL.md` files if templates own them.
- Do not skip prebuilt provenance verification after Rust behavior changes.
- Do not dispatch final review until strict clippy and full nextest pass.

#### Done When

- Generated skill and agent docs are fresh.
- Node contract tests pass.
- Strict clippy passes.
- Full nextest no-fail-fast passes.
- Standalone liveness test passes if not already covered by full nextest evidence.
- Checked-in prebuilts and checksums are refreshed if needed.
- Prebuilt provenance verification passes.
- Denied public-output strings from prior remediations are absent from checked-in binaries.
- A clean-context reviewer reports no findings against the full plan.

#### Files

- `bin/featureforge`
- `bin/prebuilt/**`
- `skills/**/SKILL.md` only if generators change output
- `.codex/agents/**` and `agents/**` only if agent generator output changes
- Validation docs only if they become stale

#### Implementation Steps

1. Run generation checks and regenerate only if needed.
2. Run Rust formatting/lint/test validation.
3. Refresh prebuilts for supported local targets when the source fingerprint changes.
4. Verify prebuilt provenance and denied strings.
5. Create a synthetic review snapshot including untracked files and refreshed binaries.
6. Dispatch a clean-context full-plan review with no FeatureForge skills and no subagent spawning.
7. Remediate any review findings and repeat validation/review until clean.
8. Run the next deep audit pass using the same A-H process.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`
- `cargo test --test liveness_model_checker`
- `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .`
- `git diff --check`
