# Runtime Signal/Noise Twentieth Audit Remediation

## Workflow State

Engineering Approved

## Plan Revision

Revision 1 - 2026-05-11

## Execution Mode

Single-agent implementation in task order with clean-context review after each completed task. Do not use FeatureForge runtime skills or project skills. Review subagents must not spawn additional subagents.

## Goal

Remove the remaining actionable twentieth-audit findings by reducing split decisioning, hardening semantic loop detection, improving public-test realism, and trimming prompt/doc churn without adding new static-test noise.

## Architecture

FeatureForge runtime progression remains:

CLI args -> command module -> transition guard -> event append -> reducer -> read model -> route decision -> workflow operator presentation.

This plan tightens the boundaries:

1. Route selection owns route choice. Status projection may enrich display/status DTOs but must not revise the selected route.
2. Repeated-route and liveness checks compare stable route semantics, not volatile rendered argv or execution fingerprints.
3. Public-flow tests are explicitly classified: shipped-runtime public tests get public-flow scanners; internal semantic model checkers are labeled as internal and cannot be cited as shipped-runtime proof.
4. Prompt/docs surfaces name the owning workflow path and avoid stale bounce-back language.

## Change Surface

- Runtime route-plan projection:
  - `src/execution/route_plan.rs`
  - `src/execution/route_plan/status_projection.rs`
  - `docs/runtime-architecture.md`
  - `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- Liveness and replay tests:
  - `tests/liveness_model_checker.rs`
  - `tests/public_replay_churn.rs`
  - `tests/runtime_module_boundaries.rs`
  - `tests/support/public_flow_scan.rs`
- Skill and review docs:
  - `skills/writing-plans/SKILL.md.tmpl`
  - generated `skills/writing-plans/SKILL.md`
  - `review/review-accelerator-packet-contract.md`
  - `tests/codex-runtime/skill-doc-contracts.test.mjs`

## Preconditions

- Before each audit-loop iteration, run `cargo clean`.
- Before starting any full test cycle, confirm no `cargo nextest`, `cargo-nextest`, `nextest run`, or `target/debug/deps/` test-binary process is active.
- Before dispatching any task review subagent, run:
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- If full nextest exceeds 10 minutes, stop it, run `cargo clean`, rerun, and remediate repeatable performance regressions before proceeding.
- For skill template changes, regenerate generated skills with `node scripts/gen-skill-docs.mjs`.

## Known Footguns / Constraints

- Do not weaken public typed route surfaces. `recommended_public_command_argv` remains exact executable authority; `recommended_public_command_template` remains the bindable fallback; `recommended_command` remains display-only.
- Do not convert internal semantic liveness coverage into claimed shipped-runtime proof.
- Do not add another guard around duplicate route decisioning when one decision point can be deleted.
- Do not hand-edit generated `SKILL.md` files when a `.tmpl` exists.
- Do not weaken strict clippy, nextest coverage, prompt budget enforcement, or public-flow hidden-helper scanners.
- Do not solve prompt bloat by moving mandatory law solely into companion references for route-owning skills.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| Status projection cannot revise selected route | Task 1 |
| Targetless stale baseline repair has one route owner | Task 1 |
| Route-loop detection ignores volatile fingerprints | Task 2 |
| Liveness model covers earlier stale plus later overlay | Task 2 |
| Public-flow scanners cover public-looking suites without misclassifying internal model checkers | Task 3 |
| Stale prompt/docs bounce language removed | Task 4 |
| Full validation and clean-context review gates | Every task |

## Ordered Tasks

### Task 1 - Make status projection route-neutral

**Spec Coverage:** Modularization, split-decisioning, stale closure/cycle-break routing.

**Goal:** Ensure targetless stale baseline repair is selected only in `select_runtime_route_decision`, not later in status projection.

**Context:** `src/execution/route_plan.rs::select_runtime_route_decision` already handles `targetless_stale_yields_to_task_closure_baseline_repair`. `src/execution/route_plan/status_projection.rs::finalize_route_decision_for_status_projection` repeats that decision and can replace `route_decision`, while `docs/runtime-architecture.md` says status projection must not revise routing.

**Constraints:**

- Keep targetless stale reconcile diagnostics and blocker enrichment.
- Preserve public repair-target projection for a route that was already selected by route-plan.
- Do not remove the route-plan selected baseline repair route.

**Done when:**

- `finalize_route_decision_for_status_projection` never replaces `RouteDecision` with a new route.
- Targetless stale baseline repair remains routed when selected by `select_runtime_route_decision`.
- Boundary tests fail if status projection calls `repair_review_state_route_decision` or imports stale baseline repair route selection helpers.
- Architecture docs and module-boundary reference docs agree that status projection is route-neutral.

**Files:**

- `src/execution/route_plan/status_projection.rs`
- `src/execution/route_plan.rs`
- `tests/runtime_module_boundaries.rs`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`

**Implementation Steps:**

1. Remove the targetless stale baseline repair rewrite from `finalize_route_decision_for_status_projection`.
2. Keep only route-neutral normalization: diagnostic normalization, blocker enrichment, required follow-up derivation, public repair target projection, and status field mirroring.
3. Add or update a boundary test asserting `status_projection.rs` does not call route constructors such as `repair_review_state_route_decision`.
4. Update architecture docs to state that targetless stale baseline repair is owned by route selection and status projection may only project the selected decision.
5. Run targeted tests covering route-plan boundary and targetless stale public route behavior before full validation.

**Validation Expectations:**

- Targeted route-plan/runtime-module tests pass.
- Strict clippy and full nextest pass before review.
- Clean-context review confirms no route-affecting status projection remains.

### Task 2 - Normalize liveness and replay route identity

**Spec Coverage:** Stale closure convergence, cycle-break loops, public/private test realism.

**Goal:** Detect repeated route loops by stable semantic route identity, even when execution fingerprints or rendered argv refresh between iterations.

**Context:** `tests/public_replay_churn.rs` route tuples include `recommended_public_command_argv` and `execution_fingerprint`; `tests/liveness_model_checker.rs` uses exact rendered argv/command strings in route keys. That can miss a loop where only `--expect-execution-fingerprint` changes.

**Constraints:**

- Keep exact argv assertions where the shell boundary is being tested.
- Route-loop detection must compare command kind, task, step, phase/detail, required follow-up, state kind, and stable reason codes.
- Do not hide real progress when command kind or target changes.

**Done when:**

- Public replay repeated-route detection uses stable semantic route keys.
- Liveness model repeated-route detection uses stable semantic route keys.
- A regression covers fingerprint-churned equivalent routes and fails if they are treated as progress.
- The liveness model explicitly covers an earlier stale boundary plus later stale/interrupted overlay and requires convergence on the earlier unresolved boundary.

**Files:**

- `tests/public_replay_churn.rs`
- `tests/liveness_model_checker.rs`
- Optional shared test helper if duplication becomes meaningful.

**Implementation Steps:**

1. Introduce a semantic route key helper in public replay tests that uses typed public command mutation request fields or template command kind/inputs instead of rendered argv.
2. Exclude volatile `execution_fingerprint` and `--expect-execution-fingerprint` values from loop identity while preserving exact argv checks in tests that validate invocation shape.
3. Add a public replay regression that mutates only fingerprint material and asserts the route is treated as repeated.
4. Extend liveness model cases to encode both earlier stale boundary and later stale/interrupted overlay simultaneously.
5. Tighten successor assertions so moving to a later task is not considered convergence unless the earlier stale target is consumed or no longer authoritative.

**Validation Expectations:**

- `cargo test --test public_replay_churn` passes.
- `cargo test --test liveness_model_checker` passes.
- Strict clippy and full nextest pass before review.
- Clean-context review confirms volatile fingerprint churn cannot mask route loops.

### Task 3 - Clarify public-flow test realism boundaries

**Spec Coverage:** Tests vs shipped-runtime realism, public/private helper quarantine.

**Goal:** Keep public-runtime proof public while leaving internal model checkers clearly internal and scanner-protected against accidental misclassification.

**Context:** The public-flow scanner protects a manual file list. `plan_execution_final_review.rs` is public-looking but not in that list. `liveness_model_checker.rs` is intentionally internal semantic coverage, not shipped-runtime proof.

**Constraints:**

- Do not force internal semantic model checkers through shipped CLI if the shell boundary is not the contract.
- Do not weaken hidden-helper or hidden-command scanners for true public-flow tests.
- Keep direct runtime helper use quarantined and named as internal where it remains intentional.

**Done when:**

- `plan_execution_final_review.rs` is either included in the protected public-flow set or explicitly documented/tested as internal with a reason.
- `liveness_model_checker.rs` is explicitly excluded from public-flow proof and cannot be cited by tests/docs as shipped-runtime proof.
- Public-flow scanner tests cover the classification rule.
- Public runtime flow script remains focused on compiled CLI/public shell tests.

**Files:**

- `tests/support/public_flow_scan.rs`
- `tests/public_flow_scan_contracts.rs`
- `scripts/run-public-runtime-flow-tests.sh`
- `tests/liveness_model_checker.rs`
- `docs/testing.md`

**Implementation Steps:**

1. Audit `plan_execution_final_review.rs` for hidden-helper/direct-helper use and decide whether it can be protected as public flow.
2. Update `is_protected_public_flow_file` and scanner contract fixtures to reflect that decision.
3. Add explicit internal semantic disclaimers to liveness model docs/comments and `docs/testing.md`.
4. Ensure public-runtime flow script names only shipped-runtime/public shell proof suites.

**Validation Expectations:**

- `cargo test --test public_flow_scan_contracts` passes.
- Public-flow contract tests pass.
- Strict clippy and full nextest pass before review.
- Clean-context review confirms public/private proof boundaries are explicit.

### Task 4 - Remove stale prompt and review fallback wording

**Spec Coverage:** Plan-review workflow, public-output/agent UX, prompt-surface signal.

**Goal:** Remove prompt/doc wording that can send agents into unnecessary planning churn or vague manual review paths.

**Context:** `writing-plans` says engineering review issues should return to `writing-plans`; current `plan-eng-review` owns Draft edits until issues are resolved. `review/review-accelerator-packet-contract.md` says “normal manual review” without naming the owning review path. Module-boundary docs still say public-command construction remains in `next_action.rs`.

**Constraints:**

- Keep top-level mandatory law in route-owning skills.
- Regenerate generated skills after editing templates.
- Do not add repeated route law to every skill.

**Done when:**

- `writing-plans` says engineering review owns Draft plan edits and final handoff to fidelity.
- Review accelerator fallback text names the owning normal review flows instead of vague manual review.
- Module-boundary reference docs match current route-plan command-construction ownership.
- Generated skill docs are fresh and within budget.

**Files:**

- `skills/writing-plans/SKILL.md.tmpl`
- generated `skills/writing-plans/SKILL.md`
- `review/review-accelerator-packet-contract.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `scripts/gen-skill-docs.mjs` only if generator behavior must change
- `tests/codex-runtime/skill-doc-contracts.test.mjs` only if wording contracts need alignment

**Implementation Steps:**

1. Replace the `writing-plans` fail-closed bullet with wording that hands all engineering-review fixes to `plan-eng-review` while preserving Draft state until issues are resolved.
2. Replace vague “normal manual review” fallback language with explicit normal `plan-ceo-review` / `plan-eng-review` section review flow wording.
3. Update the module-boundary reference to state route-plan owns public-command construction; `next_action` remains legacy semantic candidate/display support until fully retired.
4. Regenerate skill docs and run the Node prompt/doc contract checks.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check` passes after regeneration.
- `node --test tests/codex-runtime/*.test.mjs` passes.
- Strict clippy and full nextest pass before review.
- Clean-context review confirms the docs remove churn without lowering mandatory law.
