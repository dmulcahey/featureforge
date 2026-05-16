# Runtime Signal/Noise Twenty-Second Audit Remediation Plan

> **For Codex and GitHub Copilot workers:** REQUIRED: Use `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` as routing authority after engineering approval, and follow the runtime-selected execution owner skill; do not choose solely from isolated-agent availability. Run only `recommended_public_command_argv` or a bound `recommended_public_command_template`; `recommended_command` is display-only compatibility text. Steps use checkbox (`- [ ]`) syntax for tracking.

## Workflow State

Engineering Approved

## Plan Revision

Revision 1 - 2026-05-12

## Execution Mode

Single-agent serial implementation in task order with strict clippy, full no-fail-fast nextest, and clean-context review after each completed task. Do not use FeatureForge runtime skills or project skills. Review subagents must not spawn additional subagents. Before every full nextest cycle, confirm no `cargo nextest`, `cargo-nextest`, `nextest run`, or active `/target/debug/deps/` process is running. If a full suite exceeds 10 minutes, stop after the run completes, clean, rerun, and remediate repeatable performance regressions. If it exceeds 4-5 minutes, clean and rerun to confirm timing.

## Source Spec

`docs/featureforge/archive/runtime-safety-audit-history/2026-05-12-twenty-second-audit-report.md`

## Source Spec Revision

Revision 1

## Last Reviewed By

plan-eng-review

## QA Requirement

not-required

**Goal:** Remove the twenty-second audit's remaining split-decisioning, prompt/test brittleness, and public-output traps without increasing FeatureForge's conceptual surface area.

**Architecture:** Final public-route validation must consume finalized route projections, route choice must be decomposed into cohesive route-family modules under one route-plan owner, tests must protect durable public/runtime boundaries rather than private implementation shape, and prompt/release validation must centralize mandatory law instead of repeating prose.

**Tech Stack:** Rust runtime modules, Rust integration tests, Node skill-doc/archive tests, generated markdown skills and runtime documentation.

---

## Change Surface

- `src/execution/status_assembly/task_state.rs`
- `src/execution/route_plan/public_commands.rs`
- `src/execution/route_plan/next_action_choice.rs`
- `src/execution/route_plan/**`
- `src/execution/state/preflight.rs`
- `src/execution/status.rs`
- `src/workflow/operator.rs`
- `scripts/verify-source-archive.mjs`
- `tests/runtime_module_boundaries.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/public_replay_churn.rs`
- `tests/fixtures/runtime-remediation/README.md`
- `tests/codex-runtime/skill-doc-budget.test.mjs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `skills/subagent-driven-development/*reviewer-prompt*.md`
- reviewer prompts under `skills/**`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `docs/testing.md`

## Preconditions

- The twenty-first remediation final validation is green:
  - `node scripts/gen-skill-docs.mjs --check`
  - `node scripts/gen-agent-docs.mjs --check`
  - `node --test tests/codex-runtime/*.test.mjs`
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- The twenty-second audit validation is green:
  - Node generated-doc checks and codex-runtime tests
  - strict clippy
  - grouped audit nextest shard
  - `cargo test --test liveness_model_checker`
- The worktree may contain prior audit/remediation edits. Preserve unrelated user or prior-task changes.

## Known Footguns / Constraints

- Do not run FeatureForge runtime/project skills.
- Do not allow review subagents to spawn subagents.
- Do not interrupt productive in-flight commands or subagents.
- Do not weaken public CLI reachability, typed command authority, prompt-budget enforcement, reviewer recursion prevention, or hidden-helper scanners.
- Do not add more static scanners when a duplicate implementation can be deleted.
- Do not preserve split decisioning under a new name.
- Do not let `status_assembly` recompute routes to compensate for missing route projection.
- Do not make `route_plan/next_action_choice.rs` smaller by moving unrelated code into a new catch-all module.
- Do not test private function names, exact comments, or incidental release prose unless they are explicit public contracts.
- Regenerate generated skills after template/generator changes when applicable.
- Keep prompt changes high signal: added mandatory law should replace or collapse weaker duplicated wording.

## Requirement Coverage Matrix

| Requirement | Coverage |
|---|---|
| REQ-001: Exact execution-command validation consumes finalized route projection only | Task 1 |
| REQ-002: Route choice is split into cohesive route-family modules without adding a new monolith | Task 2 |
| REQ-003: Boundary tests protect durable ownership contracts without private/comment pins | Task 3 |
| REQ-004: Public diagnostics point to diagnostic stop or typed public route re-query, not manual artifact repair | Task 4 |
| REQ-005: Prompt-budget/source-archive/release validation stays mandatory but low-noise | Task 4 |
| REQ-006: Reviewer recursion and route/prompt law are centralized enough to remain actionable | Task 5 |
| REQ-007: Historical coverage inventory reflects current public replay proof | Task 4 |
| REQ-008: Full verification remains performant and comprehensive | All tasks |

## Ordered Tasks

Execute tasks serially. After each task, run strict clippy and a full no-fail-fast nextest cycle, then dispatch a clean-context review for that exact task. Remediate review findings and repeat validation/review until no findings remain before moving to the next task.

### Task 1: Remove Exact Route Validation Recompute

**Spec Coverage:** REQ-001, REQ-008

**Goal:** Make exact execution-command route validation fail closed when finalized status projection omits the execution command context, instead of recomputing a target through a second route-choice pass.

**Context:**

- `status_assembly/task_state.rs::require_public_execution_command_route_target` currently accepts `execution_command_context + recommended_command`, but if either is missing it calls `route_plan/public_commands.rs::require_execution_command_route_target`.
- That fallback recomputes `compute_next_action_decision` and can mask a route projection bug.
- Finalized route projection is supposed to be the route authority consumed by status/operator/read-model surfaces.

**Constraints:**

- Do not remove legitimate public execution-route validation.
- Do not use `recommended_command` as executable authority; it may remain a display-field presence check only if needed for backward contract validation.
- Prefer requiring typed fields such as `recommended_public_command_argv`, `recommended_public_command_template`, and `execution_command_context` when the route requires an exact execution command.
- If a status route is invalid, return a clear `JsonFailure` explaining that finalized route projection is missing required public execution command fields.
- Remove or test-quarantine any fallback helper that recomputes route targets from status context.

**Done when:**

- `require_public_execution_command_route_target` no longer calls a helper that recomputes `compute_next_action_decision`.
- `route_plan/public_commands.rs::execution_command_route_target_from_status_context` is removed, made test-only with explicit boundary comments, or no longer used by production validation.
- A regression test constructs a status needing an execution command route with missing finalized route fields and verifies fail-closed behavior instead of recomputation.
- Existing public route goldens and replay tests remain green.

**Files:**

- Modify: `src/execution/status_assembly/task_state.rs`
- Modify: `src/execution/route_plan/public_commands.rs`
- Modify: `tests/runtime_module_boundaries.rs`
- Modify: `tests/runtime_behavior_golden.rs`
- Modify: `tests/public_replay_churn.rs` or relevant status/read-model tests
- Verify: `docs/runtime-architecture.md`

- [ ] **Step 1:** Identify all production callers of `require_execution_command_route_target` and `execution_command_route_target_from_status_context`.
- [ ] **Step 2:** Replace the recompute fallback with validation against finalized public route fields.
- [ ] **Step 3:** Delete or quarantine now-unused recompute helpers.
- [ ] **Step 4:** Add a regression test that proves missing finalized route fields fail closed.
- [ ] **Step 5:** Update boundary tests to forbid production status assembly from invoking route candidate recomputation for exact execution route validation.

### Task 2: Decompose Route Choice Without Reintroducing Split Decisioning

**Spec Coverage:** REQ-002, REQ-008

**Goal:** Split `route_plan/next_action_choice.rs` into cohesive route-family modules while preserving one route-plan-owned public route choice pass.

**Context:**

- `next_action_choice.rs` is 2,318 lines, and `compute_next_action_decision_with_authority_inputs` contains the main route-ordering tree.
- Previous remediation centralized route choice correctly but left the code too large to review safely.
- The goal is not to move code around for file-size optics; it is to isolate cohesive route families and make the route order understandable.

**Constraints:**

- Keep route-plan ownership. Do not move decisioning back to `next_action.rs`, `status_assembly`, router, command modules, or workflow/operator.
- Do not create a new catch-all `helpers.rs` or `common.rs` that simply hides the monolith.
- Preserve current public behavior and route goldens unless the audit exposes an actual route bug.
- Keep public command binding separate from ordered next-action choice.
- Keep `NextActionDecision` a typed DTO, not a second route owner.

**Done when:**

- `next_action_choice.rs` is an orchestrator over cohesive child modules, not the full route tree.
- Route-family modules have clear responsibilities, for example preflight/execution, review/repair, late-stage/finish, and diagnostics/blockers.
- Boundary docs describe the child modules and the single route-choice owner.
- Boundary tests enforce route-plan ownership and reasonable route module size without pinning exact comments or private helper names.
- Full public route/liveness/golden behavior is unchanged or intentionally updated with clear evidence.

**Files:**

- Modify: `src/execution/route_plan/next_action_choice.rs`
- Add/Modify: `src/execution/route_plan/*`
- Modify: `src/execution/next_action.rs`
- Modify: `src/execution/route_plan.rs`
- Modify: `docs/runtime-architecture.md`
- Modify: `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- Modify: `tests/runtime_module_boundaries.rs`
- Test: `tests/runtime_behavior_golden.rs`
- Test: `tests/public_replay_churn.rs`
- Test: `tests/liveness_model_checker.rs`

- [ ] **Step 1:** Map the current ordered decision tree into cohesive route families before editing.
- [ ] **Step 2:** Extract one family at a time behind small functions with typed inputs/outputs.
- [ ] **Step 3:** Keep the top-level function readable as ordered family evaluation.
- [ ] **Step 4:** Add or update module-boundary checks for ownership and size caps that allow useful refactors.
- [ ] **Step 5:** Run targeted route goldens/liveness before full validation.

### Task 3: Replace Brittle Boundary Pins With Durable Contracts

**Spec Coverage:** REQ-003, REQ-008

**Goal:** Remove boundary tests that pin comments, exact private helper names, or incidental import text, and replace them with durable ownership/import/public-behavior checks.

**Context:**

- `tests/runtime_module_boundaries.rs` currently checks strings such as `ordered pass #1` and `ordered pass #5`.
- Some checks assert exact private structs/functions where import direction and public route behavior are the real contracts.
- These tests add churn during modularization and do not directly prove shipped runtime behavior.

**Constraints:**

- Preserve high-value tests that prevent workflow/operator from importing mutators, read-model modules from appending events, command modules from writing projections directly, and public route ownership from drifting out of route-plan.
- Do not weaken public/private helper quarantine tests.
- If a private helper name is intentionally a boundary API, document why in the test message or nearby source.
- Prefer AST/source-scanner checks for import direction and public module boundaries over exact prose/comment assertions.

**Done when:**

- No test requires `ordered pass #*` comments to exist.
- Boundary tests no longer fail solely because private route helper names changed.
- Tests still fail if route choice moves outside `route_plan`, if status assembly recomputes public routes, or if mutation/write helpers are imported across forbidden boundaries.
- The test messages describe semantic boundaries, not current file trivia.

**Files:**

- Modify: `tests/runtime_module_boundaries.rs`
- Modify: `tests/support/rust_source_scan.rs` if helper support is needed
- Verify: `tests/public_flow_scan_contracts.rs`
- Verify: `tests/public_cli_flow_contracts.rs`

- [ ] **Step 1:** Classify boundary assertions as durable ownership checks or private-shape pins.
- [ ] **Step 2:** Delete comment pins and replace private-name pins with import/module responsibility checks.
- [ ] **Step 3:** Add targeted checks for the Task 1 route-validation boundary and Task 2 route-plan module ownership.
- [ ] **Step 4:** Run module-boundary and public-flow scanner tests before full validation.

### Task 4: Public Diagnostic, Archive, Fixture, And Testing-Docs Cleanup

**Spec Coverage:** REQ-004, REQ-005, REQ-007, REQ-008

**Goal:** Fix the concrete public-output and validation inventory issues without adding new workflow law.

**Context:**

- `authoritative_mutation_recovery_required` remediation can sound like manual artifact repair.
- `verify-source-archive.mjs` omits prompt-budget assets that release docs treat as mandatory.
- `tests/fixtures/runtime-remediation/README.md` underreports public replay coverage.
- `skill-doc-budget.test.mjs` currently tests release-note prose.
- `docs/testing.md` mixes mandatory gates and manual audit aids.

**Constraints:**

- Public diagnostics should point to diagnostic stop or typed public route re-query, not manual repair.
- Source archive verification should require release-critical assets without becoming a broad repository inventory.
- Budget tests should enforce budget manifest shape, generated docs, and release validation gate presence, not incidental release prose.
- Keep `docs/testing.md` concise enough to be followed during release work.

**Done when:**

- `authoritative_mutation_recovery_required` remediation says to stop/report runtime diagnostic and re-query workflow/operator JSON only to confirm a typed public route.
- Source archive verification requires `skills/skill-doc-budgets.json` and `tests/codex-runtime/skill-doc-budget.test.mjs`.
- Runtime-remediation coverage map includes current public replay coverage for `FS-11` through `FS-16` and current churn replay coverage.
- Prompt-budget test no longer pins `RELEASE-NOTES.md` wording.
- `docs/testing.md` clearly separates mandatory release gates from manual audit aids.

**Files:**

- Modify: `src/execution/state/preflight.rs`
- Modify: `tests/internal_plan_execution.rs` or public diagnostic tests as needed
- Modify: `scripts/verify-source-archive.mjs`
- Modify: `tests/codex-runtime/skill-doc-budget.test.mjs`
- Modify: `tests/fixtures/runtime-remediation/README.md`
- Modify: `docs/testing.md`
- Verify: `tests/codex-runtime/*.test.mjs`

- [ ] **Step 1:** Update preflight remediation text and add/adjust a regression assertion for the message.
- [ ] **Step 2:** Add budget assets to `REQUIRED_SOURCE_ARCHIVE_PATHS`.
- [ ] **Step 3:** Update prompt-budget tests to focus on mandatory gate presence and manifest enforcement.
- [ ] **Step 4:** Refresh runtime-remediation coverage map from current public replay tests.
- [ ] **Step 5:** Split `docs/testing.md` release gates from manual audit aids.
- [ ] **Step 6:** Run Node codex-runtime tests and targeted Rust diagnostic tests before full validation.

### Task 5: Centralize Reviewer Prompt Recursion Law And Trim Prompt Duplication

**Spec Coverage:** REQ-006, REQ-008

**Goal:** Keep reviewer recursion prevention prompt-only and reviewer-scoped while eliminating duplicated boilerplate and keeping route-owning skills actionable.

**Context:**

- The exact recursion rule appears twice in `skills/subagent-driven-development/spec-reviewer-prompt.md`: once as guidance and once inside the dispatch payload.
- Similar reviewer prompts repeat the same mandatory law.
- The rule is needed in actual reviewer payloads, but surrounding docs should not duplicate the full paragraph when a canonical prelude/reference can be used.

**Constraints:**

- Do not introduce runtime/env recursion enforcement.
- Actual dispatched reviewer prompt payloads must still contain the full prohibition on launching/requesting/delegating to additional subagents.
- Top-level skills must retain mandatory runtime route law and terminal stop law.
- Prefer one canonical reviewer-prompt prelude or generated include used by reviewer prompt templates.
- If generator changes are required, edit templates/source and regenerate checked-in outputs.

**Done when:**

- Reviewer prompt files avoid duplicate full recursion paragraphs outside actual payload blocks.
- A canonical source for reviewer recursion prelude exists, or generation clearly keeps the rule in one source of truth.
- Contract tests still verify reviewer payloads prohibit additional subagents and do not require runtime/env recursion guards.
- Route-owning generated skills remain within budget and point to typed operator JSON plus canonical route reference without repeating low-value prose.

**Files:**

- Modify: `skills/subagent-driven-development/spec-reviewer-prompt.md`
- Modify: other reviewer prompts under `skills/**` as needed
- Modify: `scripts/gen-skill-docs.mjs` if generated prompt/prelude handling is needed
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `skills/*/SKILL.md.tmpl` and generated `skills/*/SKILL.md` if route-law duplication is reduced
- Verify: `references/operator-route-authority.md`

- [ ] **Step 1:** Inventory reviewer prompt recursion text and classify actual payload text versus surrounding instruction text.
- [ ] **Step 2:** Introduce or reuse a canonical prelude/reference for non-payload guidance.
- [ ] **Step 3:** Keep full recursion prohibition inside actual reviewer dispatch payloads.
- [ ] **Step 4:** Simplify tests so they enforce payload behavior and absence of runtime/env guards without requiring duplicated prose.
- [ ] **Step 5:** Regenerate skills if templates or generator output changed.

## Validation Expectations

After each task:

1. Run targeted checks relevant to the changed files.
2. Run `cargo fmt --check`.
3. Run `cargo clippy --all-targets --all-features -- -D warnings`.
4. Confirm no active `cargo nextest`, `cargo-nextest`, `nextest run`, or `/target/debug/deps/` process is running.
5. Run full nextest with no fail-fast:

```bash
cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow
```

6. If the full suite exceeds 10 minutes, stop after completion, run `cargo clean`, rerun, and remediate repeatable performance regressions. If it exceeds 4-5 minutes, run `cargo clean`, rerun, and compare timing.
7. Dispatch a clean-context review subagent for the exact completed task. The reviewer must not use FeatureForge runtime/project skills and must not spawn subagents.
8. Remediate reviewer findings and repeat validation/review until accepted.

Final validation after all tasks:

```bash
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow
cargo test --test liveness_model_checker
git diff --check
```

Then dispatch a clean-context whole-plan review against this plan and run the next full audit iteration with a fresh `cargo clean`.
