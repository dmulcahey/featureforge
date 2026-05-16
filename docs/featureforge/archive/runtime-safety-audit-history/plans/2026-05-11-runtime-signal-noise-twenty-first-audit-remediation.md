# Runtime Signal/Noise Twenty-First Audit Remediation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` as routing authority after engineering approval, and follow the runtime-selected execution owner skill; do not choose solely from isolated-agent availability. Steps use checkbox (`- [ ]`) syntax for tracking.

## Workflow State

Engineering Approved

## Plan Revision

Revision 1 - 2026-05-11

## Execution Mode

Single-agent serial implementation in task order with strict clippy, full no-fail-fast nextest, and clean-context review after each completed task. Do not use FeatureForge runtime skills or project skills. Review subagents must not spawn additional subagents.

## Source Spec

`docs/featureforge/archive/runtime-safety-audit-history/2026-05-11-twenty-first-audit-report.md`

## Source Spec Revision

Revision 1

## Last Reviewed By

plan-eng-review

## QA Requirement

not-required

**Goal:** Remove the remaining route split-decisioning and prompt/test churn found by the twenty-first runtime safety audit.

**Architecture:** Move route selection toward one route-plan-owned decision pass fed by explicit reducer/status facts, keep mutation commands gated by the same typed route eligibility authority, and collapse prompt/test duplication around durable public contracts. The remediation should delete duplicate decision paths where possible instead of adding more static guards.

**Tech Stack:** Rust runtime modules, Rust integration tests, Node skill-doc generator/tests, generated markdown skills and runtime documentation.

---

## Change Surface

- `src/execution/router.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/**`
- `src/execution/next_action.rs`
- `src/execution/public_route_selection.rs`
- `src/execution/status_assembly.rs`
- `src/execution/read_model.rs`
- `src/execution/commands/advance_late_stage.rs`
- `src/execution/commands/common/mutation_guards.rs`
- `src/execution/command_eligibility.rs`
- `src/execution/state.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/workflow_runtime*.rs`
- `tests/plan_execution*.rs`
- `tests/liveness_model_checker.rs`
- `scripts/gen-skill-docs.mjs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `skills/*/SKILL.md`
- `references/operator-route-authority.md`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `docs/testing.md`

## Preconditions

- The twentieth-audit remediation validation is green:
  - `node scripts/gen-skill-docs.mjs --check`
  - `node scripts/gen-agent-docs.mjs --check`
  - `node --test tests/codex-runtime/*.test.mjs`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- The twenty-first audit validation is green:
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - grouped audit nextest shard for runtime authority/workflow/plan-execution/query tests
  - `cargo test --test liveness_model_checker`
- Before every full nextest cycle, check that no `cargo nextest`, `cargo-nextest`, `nextest run`, or `target/debug/deps/` test-binary process is active.

## Known Footguns / Constraints

- Do not run FeatureForge runtime/project skills.
- Do not allow review subagents to spawn subagents.
- Do not weaken public CLI reachability, typed route authority, prompt-budget enforcement, or reviewer recursion prompt rules.
- Do not convert internal semantic liveness coverage into full shipped-CLI proof.
- Do not remove hidden-helper scanners for public-flow tests.
- Do not preserve duplicate decisioning by renaming it. Route planning should consume shared facts once.
- Do not make `status_assembly` a route decision owner. It can produce reducer/status facts, but public route fields must come from route decisions.
- Do not use private helper-name pins as a substitute for semantic boundary tests.
- Regenerate generated skills after template/generator changes.
- Keep generated top-level route instructions compact; route-owning skills must retain terminal stop law but should delegate detailed field binding to the canonical reference.

## Requirement Coverage Matrix

- REQ-001: Route planning has one route-choice pass and does not revise decisions after status projection -> Tasks 1, 2
- REQ-002: `next_action` no longer owns the main runtime route-ordering tree -> Task 2
- REQ-003: Late-stage mutation readiness uses central typed route/eligibility authority -> Task 3
- REQ-004: Status assembly produces shared facts without public route-law duplication -> Tasks 1, 4
- REQ-005: Tests prove public behavior and import ownership without private helper churn -> Task 5
- REQ-006: Route JSON law is centralized and generated skills stay high signal -> Task 6
- REQ-007: Validation remains performant and comprehensive -> All tasks

## Execution Strategy

Execute all tasks serially. The tasks touch overlapping route/status/test surfaces, and parallel edits would create route-law conflicts. After each task, run strict clippy and the full nextest suite with no fail-fast before dispatching a clean-context review for that exact task.

## Dependency Diagram

```text
Task 1 -> Task 2 -> Task 3 -> Task 4 -> Task 5 -> Task 6 -> final validation/review -> next audit
```

## Task 1: Route Planning Fact Contract

**Spec Coverage:** REQ-001, REQ-004, REQ-007

**Goal:** Introduce a shared route-planning fact contract that carries all targetless stale, baseline bridge, exact resume, blocking-record, and review-state inputs needed before route selection.

**Context:**

- The audit found that `router.rs::project_final_runtime_routing_projection` plans a route first, projects route fields into status, computes blockers, then lets `route_plan.rs::select_route_decision_with_status_projection_authority` revise the route.
- The needed facts already exist, but they are split across status projection, stale projection, status assembly, and route plan helpers.
- This task establishes the data contract only; Task 2 removes the second route-choice pass.

**Constraints:**

- Do not change public route behavior in this task except to make facts explicit.
- Do not make status projection call route constructors or stale target selectors.
- Keep fact computation independent of display strings and `recommended_command`.
- Prefer a small cohesive module under `src/execution/route_plan/` or `src/execution/status_assembly/` rather than expanding `router.rs`.

**Done when:**

- A typed route-planning facts struct exists and is consumed by route planning.
- Facts include at least: earliest stale task target, targetless stale reconcile flag, baseline-bridge close-current-task candidate, exact resume stale task, status blocking records or the inputs needed to compute them, review-state status used for route selection, and persisted repair follow-up.
- Existing behavior remains green for public replay, liveness, workflow runtime, plan execution, and route goldens.
- Boundary tests assert that status projection does not construct/select routes and route-plan owns route-choice facts.

**Files:**

- Modify: `src/execution/route_plan.rs`
- Modify: `src/execution/route_plan/route_facts.rs`
- Modify: `src/execution/route_plan/status_projection.rs`
- Modify: `src/execution/router.rs`
- Modify: `src/execution/status_assembly.rs`
- Modify: `tests/runtime_module_boundaries.rs`
- Test: `tests/public_replay_churn.rs`
- Test: `tests/liveness_model_checker.rs`
- Test: `tests/runtime_behavior_golden.rs`

- [ ] **Step 1:** Add a route-planning facts type with named fields and no presentation strings.
- [ ] **Step 2:** Populate the facts from reducer/status assembly inputs before public route selection.
- [ ] **Step 3:** Replace ad hoc route-plan calls to status-projection-derived selectors with fact reads.
- [ ] **Step 4:** Add boundary tests that reject route constructor/stale selector imports from status projection.
- [ ] **Step 5:** Run targeted route replay/liveness/golden tests before full validation.

## Task 2: Single-Pass Route Choice

**Spec Coverage:** REQ-001, REQ-002, REQ-004, REQ-007

**Goal:** Remove post-status-projection route revision so route planning chooses the final public route exactly once from reducer/status facts.

**Context:**

- Current flow: reducer -> route plan -> route/status projection -> blocker computation -> route-plan revision -> final status/operator projection.
- The target flow is reducer/status facts -> route plan -> read model/status projection -> workflow operator presentation.
- `next_action.rs` can remain as a compatibility facade for constants or text mapping while route ordering moves under route-plan ownership.

**Constraints:**

- Do not drop any existing public behavior for targetless stale reconcile, baseline bridge repair, exact resume stale binding, handoff, planning reentry, late-stage routing, or blocked runtime bug diagnostics.
- Do not leave a replacement function that still revises `RouteDecision` after status projection under a new name.
- If `NextActionDecision` remains, it must be a typed candidate DTO consumed by route-plan only, not a second route owner.

**Done when:**

- `src/execution/router.rs::project_final_runtime_routing_projection` no longer calls a route revision function after status projection.
- `select_route_decision_with_status_projection_authority` is deleted or reduced to a non-routing helper with no route constructor calls.
- `route_plan` owns the final route ordering for runtime routes.
- `next_action.rs` no longer contains the main ordered route decision tree, or it is explicitly a route-plan-internal candidate module with tests enforcing that boundary.
- Public route goldens, liveness, and replay tests remain green.

**Files:**

- Modify: `src/execution/router.rs`
- Modify: `src/execution/route_plan.rs`
- Modify: `src/execution/route_plan/next_action_route.rs`
- Modify: `src/execution/route_plan/next_action_finalization.rs`
- Modify: `src/execution/next_action.rs`
- Modify: `src/execution/public_route_selection.rs`
- Modify: `docs/runtime-architecture.md`
- Modify: `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- Modify: `tests/runtime_module_boundaries.rs`
- Test: `tests/runtime_behavior_golden.rs`
- Test: `tests/public_replay_churn.rs`
- Test: `tests/liveness_model_checker.rs`

- [ ] **Step 1:** Move or wrap route ordering so route-plan is the single owner of public route choice.
- [ ] **Step 2:** Delete post-projection route replacement from router.
- [ ] **Step 3:** Update architecture docs to match the final one-pass flow.
- [ ] **Step 4:** Update boundary tests to assert one route choice owner rather than current split shape.
- [ ] **Step 5:** Regenerate route goldens only if public DTO behavior intentionally changes; otherwise keep goldens unchanged.

## Task 3: Late-Stage Mutation Eligibility Unification

**Spec Coverage:** REQ-003, REQ-007

**Goal:** Make `advance-late-stage` command readiness consume central typed route/eligibility authority instead of locally re-deciding phase/detail/review-state readiness.

**Context:**

- `advance_late_stage.rs` currently checks operator `phase`, `phase_detail`, and `review_state_status` directly in several branches.
- `command_eligibility.rs` and mutation guards already model typed public mutation authorization.
- `begin`, `complete`, and `reopen` are closer to the desired shape; `advance_late_stage` is the outlier.

**Constraints:**

- Do not weaken fail-closed behavior for missing summaries, result inputs, stale branch closure, release blockers, final review, QA, or finish gates.
- Keep public outputs stable unless a route bug requires an explicit golden update.
- Do not reintroduce low-level late-stage recorders as normal public commands.

**Done when:**

- `advance_late_stage.rs` uses central mutation eligibility or route-decision helpers for readiness checks.
- Local checks of operator phase/detail/review-state are reduced to output context or removed.
- Blocked outputs still return one public next step or diagnostic stop.
- Tests cover branch closure, release readiness, final review, QA, finish review, and finish completion through public `advance-late-stage`.

**Files:**

- Modify: `src/execution/commands/advance_late_stage.rs`
- Modify: `src/execution/commands/common/mutation_guards.rs`
- Modify: `src/execution/command_eligibility.rs`
- Modify: `src/execution/route_plan/public_commands.rs`
- Modify: `tests/plan_execution_final_review.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/workflow_runtime_final_review.rs`
- Modify: `tests/public_cli_flow_contracts.rs`

- [ ] **Step 1:** Identify every local phase/detail readiness branch in `advance_late_stage.rs`.
- [ ] **Step 2:** Replace readiness checks with a shared public mutation request/eligibility check where possible.
- [ ] **Step 3:** Keep command-specific input validation after eligibility, not before it changes routing.
- [ ] **Step 4:** Add regression tests for any branch whose guard moved.
- [ ] **Step 5:** Confirm public help still hides internal compatibility flags and normal flow does not expose low-level recorders.

## Task 4: Status Assembly Route-Law Cleanup

**Spec Coverage:** REQ-004, REQ-005, REQ-007

**Goal:** Remove test-only public route derivation and narrow `status_assembly` to status facts rather than public phase/next-action decisions.

**Context:**

- `status_assembly.rs::derive_public_phase_detail` and `derive_public_next_action` duplicate route law behind `#[cfg(test)]`.
- `read_model` tests consume those helpers, so tests can pass against a parallel route derivation.
- `status_assembly` also computes review-state/stale facts used by route planning; those facts should be named as facts, not public route decisions.

**Constraints:**

- Preserve status DTO fields and route projections.
- Do not remove status facts that reducer/runtime truth needs.
- Tests should use route-plan/operator/public DTO behavior for route assertions.

**Done when:**

- `derive_public_phase_detail` and `derive_public_next_action` are deleted or replaced by route-plan/operator assertions.
- `read_model` tests no longer call test-only route derivation.
- Status assembly APIs clearly expose status facts, not public route decisions.
- Boundary tests prevent new test-only public route derivation helpers.

**Files:**

- Modify: `src/execution/status_assembly.rs`
- Modify: `src/execution/read_model.rs`
- Modify: `src/execution/read_model/public_route_projection.rs`
- Modify: `tests/runtime_module_boundaries.rs`
- Modify: `tests/execution_query.rs`
- Modify: `tests/contracts_execution_runtime_boundaries.rs`
- Test: `tests/runtime_behavior_golden.rs`

- [ ] **Step 1:** Replace read-model route assertions with route-plan/operator/public DTO assertions.
- [ ] **Step 2:** Delete test-only public route derivation helpers.
- [ ] **Step 3:** Keep status-review-state derivation only as a status fact with clear naming.
- [ ] **Step 4:** Add a boundary test that rejects new `derive_public_phase_detail` / `derive_public_next_action` style helpers outside route-plan.

## Task 5: Boundary Test Signal Cleanup

**Spec Coverage:** REQ-005, REQ-007

**Goal:** Replace brittle private helper-name pins with durable import direction, ownership, and public behavior checks.

**Context:**

- `tests/runtime_module_boundaries.rs` currently pins private route-plan and late-stage helper names.
- Exact private names create churn during legitimate refactors.
- Boundary tests should enforce owner modules, forbidden dependency edges, public DTO behavior, and line caps where useful.

**Constraints:**

- Do not weaken hidden-helper, public-flow, or route authority scanner tests.
- Do not remove coverage without replacing it with behavior or durable boundary coverage.
- Keep line caps for focused modules where they prevent catch-all modules.

**Done when:**

- Private helper-name requirements are removed unless the helper name is itself a public/boundary contract.
- Tests enforce module ownership through imports, public types, and behavior rather than exact private functions.
- Route/status-projection boundary assertions are consolidated instead of repeated.
- `runtime_module_boundaries` still catches split decisioning regressions.

**Files:**

- Modify: `tests/runtime_module_boundaries.rs`
- Modify: `tests/rust_source_scan_contracts.rs` if scanner fixtures need adjustment
- Modify: `docs/runtime-architecture.md`
- Modify: `docs/featureforge/reference/execution-runtime-module-boundaries.md`

- [ ] **Step 1:** Inventory private helper-name pins and classify each as contract or implementation detail.
- [ ] **Step 2:** Replace implementation-detail pins with import-direction or behavior assertions.
- [ ] **Step 3:** Consolidate duplicate status-projection/route-plan assertions.
- [ ] **Step 4:** Add regression tests that would fail if route choice becomes split again.

## Task 6: Prompt Route-Law Centralization

**Spec Coverage:** REQ-006, REQ-007

**Goal:** Collapse generated route-owning skill prose to compact top-level terminal law and delegate detailed route JSON binding to `references/operator-route-authority.md`.

**Context:**

- The canonical route reference exists and is packaged.
- `scripts/gen-skill-docs.mjs::buildInstalledControlPlaneSection` still emits detailed route binding law into every route-owning generated skill.
- Tests currently require that duplication, preserving prompt churn.

**Constraints:**

- Do not move mandatory terminal law entirely into companion references.
- Top-level route-owning skills must still say: use installed runtime, query operator JSON, execute typed argv/template only, never execute display-only compatibility `recommended_command`, stop when no typed executable surface exists.
- Do not add the same detailed law to each skill manually.
- Regenerate generated skills.

**Done when:**

- Route-owning generated skills contain compact terminal law plus a link to `references/operator-route-authority.md`.
- Detailed field binding rules live in one canonical reference.
- Prompt budget remains in enforce mode and generated skill total stays under budget.
- Tests assert compact top-level law and canonical-reference packaging rather than duplicated full prose.

**Files:**

- Modify: `scripts/gen-skill-docs.mjs`
- Modify: `references/operator-route-authority.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-budget.test.mjs` only if budget values change
- Modify: generated `skills/*/SKILL.md`
- Modify: `docs/testing.md`

- [ ] **Step 1:** Shorten `buildInstalledControlPlaneSection` while preserving terminal law.
- [ ] **Step 2:** Move any missing detailed binding rule to `references/operator-route-authority.md`.
- [ ] **Step 3:** Update Node tests to require compact top-level law and canonical reference coverage.
- [ ] **Step 4:** Regenerate skill docs.
- [ ] **Step 5:** Run prompt budget and generated-doc contract checks.

## Validation Expectations

After each task:

- Run targeted tests for the changed area.
- Run `cargo clippy --all-targets --all-features -- -D warnings`.
- Before full nextest, run an active-process precheck for `cargo nextest`, `cargo-nextest`, `nextest run`, and `target/debug/deps/`.
- Run `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`.
- If full nextest crosses 10 minutes, stop it, run `cargo clean`, rerun, and remediate repeatable performance regression.
- Dispatch a clean-context reviewer for the exact task. The reviewer must not use skills or spawn subagents.

After all tasks:

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo clippy --all-targets --all-features -- -D warnings`
- Full nextest with the same precheck and no-fail-fast command.
- Clean-context whole-plan review.
- New full audit pass using subagents A-H plus the signal-to-noise auditor.
