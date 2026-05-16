# Runtime Signal/Noise Twenty-Third Audit Remediation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` as routing authority after engineering approval, and follow the runtime-selected execution owner skill; do not choose solely from isolated-agent availability. Run only `recommended_public_command_argv` or a bound `recommended_public_command_template`; `recommended_command` is display-only compatibility text. Steps use checkbox (`- [ ]`) syntax for tracking.

## Workflow State

Engineering Approved

## Plan Revision

Revision 1 - 2026-05-12

## Execution Mode

Single-agent serial implementation in task order with strict clippy, full no-fail-fast nextest, and clean-context review after each completed task. Do not use FeatureForge runtime skills or project skills. Review subagents must not spawn additional subagents. Before every full test cycle, confirm no `cargo nextest`, `cargo-nextest`, `nextest run`, or active `/target/debug/deps/` process is running. Before every full nextest cycle, run that same process check explicitly. If a full suite exceeds 10 minutes, stop after the run completes, run `cargo clean`, rerun the full suite, and remediate repeatable performance regressions. If a full suite exceeds 4-5 minutes, run `cargo clean`, rerun the full suite, and compare timing.

## Source Spec

`docs/featureforge/archive/runtime-safety-audit-history/2026-05-12-twenty-third-audit-report.md`

## Source Spec Revision

Revision 1

## Last Reviewed By

plan-eng-review

## QA Requirement

not-required

**Goal:** Remove the twenty-third audit's remaining split-decisioning and prompt/test signal-noise findings without adding another layer of meta-infrastructure.

**Architecture:** Route-plan owns route decisions and the single route-to-status projection used by runtime presentation. Read-surface invariant enforcement may mark a route diagnostic, but it must not reconstruct an independent route decision after route-plan finalization. Public output must tell agents exactly how to use typed argv/template and when to stop. Tests should protect durable public/runtime contracts, not private helper names or incidental release prose.

**Tech Stack:** Rust runtime modules, Rust integration tests, Node skill/doc tests, generated markdown skills, runtime reference documentation.

---

## Change Surface

- `src/execution/route_plan.rs`
- `src/execution/route_plan/status_projection.rs`
- `src/execution/router.rs`
- `src/execution/query.rs`
- `src/execution/status_assembly/exact_route.rs`
- `src/execution/command_eligibility.rs`
- `src/execution/command_eligibility/mutation_request.rs`
- `src/workflow/operator.rs`
- `src/workflow/doctor_dashboard.rs`
- `references/operator-route-authority.md`
- `references/reviewer-recursion-rule.md`
- `scripts/verify-source-archive.mjs`
- `scripts/gen-skill-docs.mjs`
- `skills/using-featureforge/SKILL.md.tmpl`
- `skills/executing-plans/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- `skills/requesting-code-review/SKILL.md.tmpl`
- generated `skills/**/SKILL.md`
- `tests/packet_and_schema.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/support/public_flow_scan.rs`
- `tests/fixtures/runtime-remediation/README.md`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `docs/testing.md`

## Preconditions

- Twenty-second remediation final validation is green.
- Twenty-third audit validation is green after `cargo clean` and after the reviewer-recursion reference packaging fix:
  - `node scripts/gen-skill-docs.mjs --check`
  - `node scripts/gen-agent-docs.mjs --check`
  - `node --test tests/codex-runtime/*.test.mjs`
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - required audit nextest shards
  - `cargo test --test liveness_model_checker`
  - `git diff --check`
- The worktree contains prior audit/remediation edits. Preserve unrelated user or prior-task changes.

## Known Footguns / Constraints

- Do not run FeatureForge runtime skills or project skills.
- Do not allow review subagents to spawn, request, or delegate to additional subagents.
- Do not interrupt productive in-flight executions or subagents.
- Do not weaken public CLI reachability, typed command authority, current-closure authority, evidence/projection diagnostic-only behavior, prompt budgets, or reviewer recursion prevention.
- Do not solve split decisioning by adding new scanners around duplicated code. Delete or centralize the duplicate decision where possible.
- Do not let read-model/status assembly reconstruct route authority after route-plan finalization.
- Do not treat `recommended_command`, `next_action`, `resume_task`, or `resume_step` as executable authority.
- Do not expand high-use skills with more runtime law unless replacing weaker duplicated wording.
- Regenerate generated skills after template/generator changes.
- Keep tests focused on public behavior, import boundaries, typed DTO ownership, and source-package completeness.
- If a private helper name is tested, document why that helper is a stable boundary. Otherwise remove the private-name pin.

## Requirement Coverage Matrix

| Requirement | Coverage |
|---|---|
| REQ-001: Route-to-status projection has one runtime owner and one error policy | Task 1 |
| REQ-002: Read-surface invariants cannot replace finalized route decisions | Task 1 |
| REQ-003: Exact execution-route validation derives from finalized route/typed status authority | Task 2 |
| REQ-004: Public command token vocabulary has one typed source | Task 2 |
| REQ-005: Diagnostic-only public routes expose no executable argv/template/input/follow-up surfaces | Task 3 |
| REQ-006: Operator/handoff guidance includes the canonical no-executable-surface stop rule | Task 3 |
| REQ-007: Active skills avoid hidden-helper vocabulary and stay actionable | Task 4 |
| REQ-008: High-use skills delegate detailed route law to canonical references | Task 4 |
| REQ-009: Boundary and Node tests protect durable contracts without private/prose overfit | Task 5 |
| REQ-010: Runtime-remediation inventory is compact and coverage-oriented | Task 5 |
| REQ-011: Full verification remains complete and performant | All tasks |

## Ordered Tasks

Execute tasks serially. After each task:

1. Run `cargo fmt --check`.
2. Run `cargo clippy --all-targets --all-features -- -D warnings`.
3. Confirm no active `cargo nextest`, `cargo-nextest`, `nextest run`, or `/target/debug/deps/` process is running.
4. Run full nextest with no fail fast: `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`.
5. Run the relevant Node checks when a task touches skills/docs/tests.
6. Dispatch a clean-context reviewer subagent for the exact task. Explicitly instruct the reviewer not to spawn/request/delegate to subagents.
7. Remediate reviewer findings and repeat validation/review until no actionable findings remain.

### Task 1: Make Route Projection Single-Owner

**Spec Coverage:** REQ-001, REQ-002, REQ-011

**Goal:** Ensure route-plan finalization and runtime presentation use the same route-to-status projection and that read-surface invariants cannot reconstruct route decisions after route-plan finalization.

**Context:**

- `src/execution/route_plan/status_projection.rs::status_for_route_plan_finalization` clones status, applies route projection, projects stale closures, computes blocking records, and applies diagnostics.
- `src/execution/router.rs::project_final_runtime_routing_projection` duplicates most of that work and propagates `compute_status_blocking_records` errors that route-plan finalization currently swallows.
- `src/execution/query.rs::sync_routing_surface_from_status` copies mutated status fields into routing and calls `route_decision_from_routing`, replacing the finalized route decision.
- `src/execution/invariants.rs::convert_status_to_runtime_reconcile_or_bug` already clears executable surfaces when invariant failures require diagnostic routing; this should be expressed as diagnostic status/projection without a second route-choice owner.

**Constraints:**

- Do not preserve two route-to-status projections with comments explaining why they match.
- Pick one error policy. The preferred policy is fail closed with `JsonFailure` when status-blocker computation fails during final runtime projection, rather than silently projecting partial blocker state.
- Keep route-plan as the authority for route decision selection and route-owned field projection.
- Read-surface invariants may alter `execution_status` to a diagnostic status, but must not call `route_decision_from_routing` or otherwise build an independent route decision from status fields.
- Preserve existing diagnostic-only clearing of argv/template/required-input/follow-up surfaces.

**Done when:**

- `router.rs::project_final_runtime_routing_projection` consumes a shared route-plan status projection helper or a `RuntimeRoutePlan` output, rather than duplicating projection logic.
- Route-plan finalization and router projection use the same blocker-computation error behavior.
- `query.rs::sync_routing_surface_from_status` no longer reconstructs `routing.route_decision` from status after invariant mutation.
- If invariants convert status to `runtime_reconcile_required` or `blocked_runtime_bug`, routing surfaces reflect diagnostic-only status without claiming a new route-plan decision.
- Boundary tests fail if router and route-plan each own separate status projection blocks.
- Targeted regression coverage proves invariant-diagnostic routes do not expose executable command surfaces and do not replace finalized route decisions with status-derived decisions.

**Files:**

- Modify: `src/execution/route_plan.rs`
- Modify: `src/execution/route_plan/status_projection.rs`
- Modify: `src/execution/router.rs`
- Modify: `src/execution/query.rs`
- Modify: `src/execution/invariants.rs` only if needed for status/route separation
- Modify: `tests/runtime_module_boundaries.rs`
- Modify: `tests/execution_query.rs` or `tests/workflow_runtime.rs`
- Verify: `docs/runtime-architecture.md`

- [ ] **Step 1:** Trace current route projection call graph from `plan_runtime_route`, `project_final_runtime_routing_projection`, `apply_shared_routing_projection_to_read_scope_with_routing`, and `apply_read_surface_invariants_to_routing_with_targetless_authority`.
- [ ] **Step 2:** Introduce a shared route-plan projection result that includes the finalized `RouteDecision` and `PlanExecutionStatus`, or expose a single route-plan projection helper returning `Result<PlanExecutionStatus, JsonFailure>`.
- [ ] **Step 3:** Update route-plan finalization and router projection to use that shared helper and identical error handling.
- [ ] **Step 4:** Replace post-invariant route-decision rebuild with route-preserving diagnostic surface sync. Keep `routing.route_decision` as the finalized route decision or explicitly clear it only for diagnostic-only invariant failures if that is the safer contract.
- [ ] **Step 5:** Add/update tests for shared projection ownership, error propagation, and invariant diagnostic-only behavior.
- [ ] **Step 6:** Update runtime architecture docs to describe route-plan projection ownership and invariant limits.

### Task 2: Centralize Exact Execution Route And Public Command Tokens

**Spec Coverage:** REQ-003, REQ-004, REQ-011

**Goal:** Remove exact-route and command-token duplication by deriving validation from finalized route authority and one typed public command vocabulary.

**Context:**

- `src/execution/status_assembly/exact_route.rs::public_execution_command_route_required` recomputes whether an exact execution command route should exist from status/context fields.
- `require_public_execution_command_route_target` validates typed route fields, but the requirement predicate remains a second route decision.
- `PublicCommandKind::as_str` in `src/execution/command_eligibility.rs` and `PublicMutationKind::public_command_name` in `src/execution/command_eligibility/mutation_request.rs` repeat the public command token table.

**Constraints:**

- Do not reintroduce a helper that computes next-action candidates from status assembly.
- Do not execute or parse `recommended_command`.
- Require typed executable surfaces for execution routes: non-empty `recommended_public_command_argv` or bindable `recommended_public_command_template`, plus consistent `execution_command_context`.
- If a route is malformed, fail closed with a `JsonFailure` that tells callers to re-query workflow/operator JSON and use typed public route fields.
- Keep transfer, close-current-task, repair-review-state, and advance-late-stage command names public and typed.

**Done when:**

- Exact execution-route validation asks a route-plan-owned/finalized route predicate whether exact validation is required, or validates only when finalized route/status already exposes an execution-command route.
- No production code in `status_assembly` performs route-choice predicates from raw status/context fields beyond field consistency validation.
- Public command names flow from one typed owner. `PublicMutationKind::public_command_name` delegates to `PublicCommandKind` or a shared token table, or the duplication is removed by a clearer typed conversion.
- Tests cover malformed execution-route projection with missing typed fields and inconsistent command/context fields.
- Boundary tests forbid status assembly from importing route-choice modules or recreating route-order predicates.

**Files:**

- Modify: `src/execution/status_assembly/exact_route.rs`
- Modify: `src/execution/route_plan.rs` or a route-plan child module
- Modify: `src/execution/public_command_types.rs` if a shared token type belongs there
- Modify: `src/execution/command_eligibility.rs`
- Modify: `src/execution/command_eligibility/mutation_request.rs`
- Modify: `tests/runtime_module_boundaries.rs`
- Modify: `tests/runtime_authority_contracts.rs`, `tests/execution_query.rs`, or relevant focused tests

- [ ] **Step 1:** Identify every caller and test that depends on exact-route validation.
- [ ] **Step 2:** Define the route-owned predicate or finalized-status marker that means an exact execution command route must be present.
- [ ] **Step 3:** Update `exact_route.rs` to validate only finalized route fields and remove raw route-choice derivation.
- [ ] **Step 4:** Centralize public command token mapping and update `PublicMutationKind`/`PublicCommandKind` conversions.
- [ ] **Step 5:** Add regression tests for missing typed command surfaces, inconsistent context, and token-table drift prevention.

### Task 3: Harden Diagnostic-Only Public Output And Operator Stop Guidance

**Spec Coverage:** REQ-005, REQ-006, REQ-011

**Goal:** Make diagnostic-only routes impossible to expose executable surfaces unnoticed, and make public operator/handoff guidance tell agents to stop when no executable argv/template exists.

**Context:**

- `tests/packet_and_schema.rs::public_runtime_route_golden_diagnostic_routes_are_diagnostic_only` currently skips diagnostic routes if they expose `recommended_public_command_argv` or `required_inputs`.
- The test does not assert absence of `recommended_public_command_template`.
- `src/workflow/operator.rs::operator_json_command_guidance` instructs agents to follow argv/template but omits the no-executable-surface stop rule.
- `src/workflow/doctor_dashboard.rs` and `references/operator-route-authority.md` contain the correct stop instruction.

**Constraints:**

- Diagnostic routes are `runtime_reconcile_required` and `blocked_runtime_bug` whether they appear as `state_kind` or `phase_detail`.
- Diagnostic routes must not expose executable argv, bindable template, required inputs, required follow-up, next public action, public repair targets, blockers with next actions, or mutation blockers.
- Public text should point to one next step: execute typed argv, bind typed template, or stop/report diagnostic.
- Do not add new command surfaces to handle diagnostics.

**Done when:**

- `packet_and_schema.rs` fails on any diagnostic route that exposes argv/template/inputs/follow-up/repair/blocker action surfaces.
- Operator and handoff guidance include the canonical stop rule in plain text.
- The stop rule is sourced from, or string-aligned with, `references/operator-route-authority.md` enough that docs/tests do not drift.
- Public-output tests cover the new operator/handoff guidance and diagnostic-only route surface.

**Files:**

- Modify: `tests/packet_and_schema.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `src/workflow/doctor_dashboard.rs` only if extracting shared wording is appropriate
- Modify: `references/operator-route-authority.md` only if canonical wording needs tightening
- Modify: `tests/workflow_entry_shell_smoke.rs` or `tests/workflow_shell_smoke.rs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs` if public docs/guidance checks need alignment

- [ ] **Step 1:** Replace the diagnostic-only golden skip logic with fail-fast assertions for executable surfaces.
- [ ] **Step 2:** Add `recommended_public_command_template` absence checks.
- [ ] **Step 3:** Add the no-argv/no-template stop sentence to operator/handoff guidance.
- [ ] **Step 4:** Add public-output tests that assert the guidance and diagnostic-only contract.
- [ ] **Step 5:** Confirm diagnostic routes still return `next_action = "runtime diagnostic required"` and no mutation command.

### Task 4: Remove Hidden-Helper Vocabulary And Compact Skill Route Law

**Spec Coverage:** REQ-007, REQ-008, REQ-011

**Goal:** Keep skills actionable by replacing hidden-helper vocabulary with runtime/operator terminology and moving detailed command-binding mechanics into canonical references.

**Context:**

- Active skills and templates still say "helper-selected topology", "Helper-Owned Execution State", "helper routing", and "compatibility-helper choreography".
- The high-use requesting-code-review skill contains detailed terminal route-binding mechanics that duplicate `references/operator-route-authority.md`.
- The useful law is: use workflow/operator JSON, execute `recommended_public_command_argv`, bind `recommended_public_command_template`, and stop/report diagnostic if neither exists.

**Constraints:**

- Edit `.tmpl` files first; regenerate generated `SKILL.md` outputs.
- Do not remove mandatory law from top-level skills. Keep the action-level rule top-level and move only detailed mechanics/examples to canonical references.
- Do not introduce new "runtime helper" or "operator helper" vocabulary that keeps the same hidden-helper mental model.
- Keep prompt budgets passing. Added wording should replace or collapse weaker wording.
- Preserve reviewer recursion rule text in reviewer prompts and keep it prompt-scoped.

**Done when:**

- Active generated skills no longer use "helper" vocabulary for normal runtime routing/execution state, except where the term refers to generic non-runtime support and cannot be confused with hidden workflow helpers.
- `using-featureforge`, `executing-plans`, and `subagent-driven-development` templates use `runtime`, `workflow/operator`, `public route`, `runtime-selected topology`, or equivalent explicit public terminology.
- `requesting-code-review` top-level skill delegates detailed route-binding mechanics to `references/operator-route-authority.md` and remains within budget.
- Generated skills are fresh.
- Node skill-doc contract and budget tests pass.

**Files:**

- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/executing-plans/SKILL.md.tmpl`
- Modify: `skills/subagent-driven-development/SKILL.md.tmpl`
- Modify: `skills/requesting-code-review/SKILL.md.tmpl`
- Regenerate: generated `skills/**/SKILL.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `skills/skill-doc-budgets.json` only if justified by net compaction
- Verify: `references/operator-route-authority.md`

- [ ] **Step 1:** Replace normal-path helper wording in templates with public runtime/operator wording.
- [ ] **Step 2:** Compact `requesting-code-review` route mechanics into a top-level pointer plus the one mandatory execute/bind/stop rule.
- [ ] **Step 3:** Regenerate skill docs.
- [ ] **Step 4:** Update Node tests to enforce absence of hidden-helper normal-path wording without pinning incidental prose.
- [ ] **Step 5:** Verify skill budgets do not grow; prefer net reduction.

### Task 5: Reduce Test And Fixture Noise Without Weakening Contracts

**Spec Coverage:** REQ-009, REQ-010, REQ-011

**Goal:** Replace brittle private/prose pins with durable contract checks and trim runtime-remediation inventory into a compact coverage map.

**Context:**

- `tests/runtime_module_boundaries.rs::route_plan_owns_runtime_route_ordering` has become a long private implementation-shape test.
- `tests/codex-runtime/skill-doc-contracts.test.mjs` pins exact release-note and README wording that is not itself a stable runtime contract.
- `tests/fixtures/runtime-remediation/README.md` duplicates detailed failure narratives and coverage tables.
- `tests/support/public_flow_scan.rs` is useful, but future changes should prefer AST/source parsing through `tests/support/rust_source_scan.rs`.

**Constraints:**

- Keep import-direction and ownership tests that prevent mutators, workflow/operator, read-model, reducer, and route-plan boundaries from crossing.
- Keep tests that enforce typed public argv/template authority, display-only `recommended_command`, hidden-helper rejection, and prompt budget enforcement.
- Do not delete public replay/golden/liveness coverage.
- Do not convert the runtime-remediation inventory into a historical narrative; it should be a compact index pointing to executable coverage.
- Do not remove release-note checks that protect actual breaking output contract disclosure; remove or relax exact prose pins.

**Done when:**

- `route_plan_owns_runtime_route_ordering` checks durable boundaries: route-plan owns route selection/projection, router does not own route choice, status assembly does not recompute route choice, and route modules stay within size/import limits.
- Private helper-name and comment-marker assertions are removed unless explicitly justified as stable boundaries.
- Node tests check release-facing docs for canonical validation pointers and breaking-output disclosure without exact paragraph regexes.
- Runtime-remediation README is a compact scenario table plus coverage map, not a repeated detailed audit report.
- Public-flow scanner docs/comments tell future maintainers to prefer parser-backed checks and avoid expanding line-oriented state machines.

**Files:**

- Modify: `tests/runtime_module_boundaries.rs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/fixtures/runtime-remediation/README.md`
- Modify: `tests/support/public_flow_scan.rs`
- Modify: `docs/testing.md` if release-gate wording needs clearer separation from manual audit aids
- Verify: `tests/runtime_behavior_golden.rs`
- Verify: `tests/public_replay_churn.rs`
- Verify: `tests/liveness_model_checker.rs`

- [ ] **Step 1:** Classify current boundary assertions as durable contract, private implementation pin, or prose/comment pin.
- [ ] **Step 2:** Remove private/prose pins and replace only the necessary ones with import-direction, size, DTO ownership, or public-output tests.
- [ ] **Step 3:** Relax release-note README regexes to stable disclosure checks.
- [ ] **Step 4:** Trim runtime-remediation README to a compact scenario/coverage matrix.
- [ ] **Step 5:** Add comments or helper structure that discourages extending non-AST scanner logic.
- [ ] **Step 6:** Run targeted route/golden/liveness tests before full validation.
