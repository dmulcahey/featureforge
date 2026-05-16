# Runtime Signal/Noise Nineteenth Audit Remediation

## Workflow State

Engineering Approved

## Plan Revision

Revision 1 - 2026-05-11

## Execution Mode

Single-agent implementation with clean-context review after each completed task. Do not use FeatureForge runtime skills or project skills. Review subagents must not spawn additional subagents.

## Goal

Remove the remaining actionable audit issues from the nineteenth runtime-safety audit by centralizing route decisioning, keeping diagnostics out of control-plane affordances, reducing duplicate semantic parsing and replay logic, and trimming skill/test contract noise that no longer adds runtime safety.

## Architecture

FeatureForge runtime progression remains:

CLI args -> command module -> transition guard -> event append -> reducer -> read model -> route decision -> workflow operator presentation.

This remediation tightens that architecture in five ways:

1. Route-plan owns public command binding. Mutation paths may persist authoritative state, but they must not patch `ExecutionRoutingState` after route projection.
2. Diagnostic reason codes stay diagnostic unless a shared helper explicitly classifies them as control-plane blocking reasons.
3. Task scope-key parsing has one semantic owner shared by route, query, repair, and stale-target selection code.
4. Read-model projection paths cannot depend on mutation helpers that reload transition state when the authoritative state is already available.
5. Skill and static-test contracts protect durable public behavior and ownership boundaries without repeating every route law in every generated skill or pinning incidental private helper names.

## Change Surface

- Runtime route binding and repair-state paths:
  - `src/execution/review_state.rs`
  - `src/execution/route_plan/**`
  - `src/execution/query.rs`
  - `src/execution/status_assembly.rs`
  - `src/execution/current_truth.rs`
  - `src/execution/public_repair_targets.rs`
  - `src/execution/recording.rs`
  - `src/execution/commands/close_current_task.rs`
- Boundary and public-flow tests:
  - `tests/runtime_module_boundaries.rs`
  - `tests/runtime_behavior_golden.rs`
  - `tests/internal_plan_execution.rs`
  - `tests/liveness_model_checker.rs`
  - `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Generated skills and docs:
  - `scripts/gen-skill-docs.mjs`
  - `skills/**/SKILL.md.tmpl`
  - generated `skills/**/SKILL.md`
  - `references/operator-route-authority.md`
  - `docs/testing.md`

## Preconditions

- Run `cargo clean` before each audit loop iteration.
- Before dispatching any review subagent, run:
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Before starting any full nextest cycle, confirm no `cargo nextest`, `cargo-nextest`, or `nextest run` process is already running.
- Treat 4-5 minutes as the preferred clean-run performance-health target, but use the user-approved 10-minute hard stop as the remediation trigger: if full nextest exceeds 10 minutes, stop it, run `cargo clean`, rerun full nextest, and if it still exceeds 10 minutes stop and fix performance before continuing.
- Regenerate generated skills after template or generator changes.
- Do not hand-edit generated skill docs when a template exists.

## Known Footguns / Constraints

- Do not remove public route fields or schema annotations that agents now rely on: `recommended_public_command_argv` remains exact machine invocation authority, `recommended_public_command_template` remains the bindable fallback surface, and `recommended_command` remains display-only compatibility text.
- Do not weaken strict clippy or add lint suppressions.
- Do not replace public-flow tests with internal helper coverage when the shell boundary is the contract.
- Do not move mandatory route law solely into companion references for route-owning workflow skills.
- Do not preserve static tests that encode incidental helper names when a durable import-direction or public-output assertion can cover the same risk.
- Do not let `repair-review-state` become a normal terminal/finished-state affordance.
- Do not let projection freshness or dispatch-summary diagnostics force reentry when current authoritative closure state is sufficient.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| Route-plan owns public command binding | Task 1 |
| Terminal completion hides repair-state affordances | Task 2 |
| Projection diagnostics stay diagnostic | Task 2 |
| Task scope-key parsing is centralized | Task 3 |
| Read projections avoid mutation helper reloads | Task 4 |
| Close-current-task replay logic is centralized | Task 5 |
| Skill route law remains high signal | Task 6 |
| Static tests enforce durable boundaries only | Task 7 |
| Full verification and independent review loop | Every task |

## Tasks

### Task 1 - Move execution-reentry public binding back into route-plan

**Spec Coverage:** Route authority, split-decisioning, public command typing.

**Goal:** `repair-review-state` may persist an execution-reentry follow-up, but the subsequent executable execution route must be selected and bound through route-plan only.

**Context:**

- `src/execution/review_state.rs::bind_execution_reentry_repair_target_and_refresh_routing` persists an execution-reentry follow-up, calls `route_for_plan`, then patches the route with `bind_execution_reentry_command_to_routing`.
- `src/execution/query.rs::ExecutionRoutingState::bind_public_command` mutates final route DTO fields.
- `src/execution/route_plan/public_commands.rs` already owns typed execution command construction.

**Constraints:**

- Remove or stop using local mutation-side public-command patching.
- Do not regress typed argv/template output.
- Route-plan must choose the same task/step target from authoritative follow-up state and preserve whether the legal public route is `begin` or `reopen`.

**Done when:**

- No mutation module synthesizes a reopen public command after `route_for_plan`.
- The same public route after repair exposes executable `recommended_public_command_argv` for the persisted execution-reentry target when route-plan can legally bind one.
- Boundary tests fail if `review_state.rs` reimports route-plan reopen builders or calls `bind_public_command`.

**Files:**

- Modify: `src/execution/review_state.rs`
- Modify: `src/execution/route_plan/public_commands.rs`
- Modify: `src/execution/route_plan/**`
- Modify: `tests/runtime_module_boundaries.rs`
- Add or update targeted public replay/runtime tests.

**Implementation Steps:**

1. Feed the persisted follow-up target task/step through route-plan authority inputs rather than `ExecutionRoutingState::bind_public_command`.
2. Let route-plan expose the exact legal public execution route (`begin` for resumable legal begin, `reopen` for repair reentry) instead of forcing one command kind from the mutation path.
3. Remove `bind_execution_reentry_command_to_routing` and the route-plan `reopen_public_command` import from `review_state.rs`.
4. Add a boundary assertion that mutation modules cannot call `ExecutionRoutingState::bind_public_command` or import `reopen_public_command`.
5. Add a public runtime regression that runs the repair path and verifies the route-selected `recommended_public_command_argv` is executable through shipped CLI semantics.

**Validation Expectations:**

- Targeted Rust tests for the repair reentry route pass.
- Strict clippy and full nextest pass before review.
- Clean-context review finds no mutation-side route patching.

### Task 2 - Keep terminal and projection diagnostics out of repair control-plane affordances

**Spec Coverage:** Evidence/projection control-plane separation, terminal route UX, doctor diagnostics.

**Goal:** Terminal completion and diagnostic-only projection freshness must not expose `repair-review-state` as a public repair target or change doctor resolution into an actionable runtime repair state.

**Context:**

- `src/execution/route_plan/status_projection.rs::route_decision_exposes_repair_review_state_target` currently returns true for `finish_completion_gate_ready` plus terminal state.
- `prior_task_review_dispatch_stale` is listed as projection diagnostic but participates in stale review-state checks in `current_truth.rs` and `status_assembly.rs`.
- `workflow/operator.rs` merges projection diagnostics into doctor-visible diagnostic reason codes.
- `workflow/doctor_resolution.rs` treats any diagnostic reason as `runtime_diagnostic_required` when no command/input/wait exists.

**Constraints:**

- Preserve diagnostics as readable context.
- Do not hide real structural corruption or stale current-closure conditions.
- Do not turn terminal success into runtime diagnostic required because a projection-only artifact is stale.

**Done when:**

- Terminal finish routes do not include `repair-review-state` public repair targets unless a non-diagnostic structural repair reason exists.
- `prior_task_review_dispatch_stale` remains projection diagnostic and cannot by itself classify a task as stale/unreviewed or route repair.
- Doctor resolution distinguishes projection-only diagnostics from runtime diagnostic blockers.

**Files:**

- Modify: `src/execution/route_plan/status_projection.rs`
- Modify: `src/execution/current_truth.rs`
- Modify: `src/execution/status_assembly.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `src/workflow/doctor_resolution.rs`
- Add or update targeted tests/goldens.

**Implementation Steps:**

1. Remove terminal `finish_completion_gate_ready` as an unconditional repair-review-state target predicate.
2. Add a shared diagnostic classifier for projection-only reason codes and use it before stale/unreviewed classification.
3. Update `task_scope_stale_review_state_reason_present` and status assembly to treat only authoritative stale current-closure reasons as stale control-plane truth.
4. Teach doctor resolution to ignore projection-only diagnostics when the route is terminal and no command/input/wait exists.
5. Add regression coverage for terminal routes and `prior_task_review_dispatch_stale` as diagnostic-only.

**Validation Expectations:**

- Targeted terminal/operator/doctor tests pass.
- Strict clippy and full nextest pass before review.
- Clean-context review confirms no projection diagnostic can force repair/reentry after terminal authority.

### Task 3 - Centralize task scope-key parsing

**Spec Coverage:** Semantic centralization, stale target selection, repair-state cleanup.

**Goal:** All modules parse `task-<n>` scope keys through one helper with documented exact semantics.

**Context:**

- `stale_target_selection.rs` requires the entire suffix to parse.
- `query.rs::task_number_from_task_scope_key` parses leading digits.
- `review_state.rs` reimplements local `strip_prefix("task-")` parsing.

**Constraints:**

- Pick one strict parsing rule and apply it everywhere.
- Add tests for invalid suffixes such as `task-1-extra` and `task-1:closure`.
- Do not break valid persisted `task-<n>` transition-state keys.

**Done when:**

- One public-in-crate helper owns task scope-key parsing.
- Existing parsers delegate to the helper or are removed.
- Boundary tests catch reintroduced local `strip_prefix("task-")` parsing outside the helper.

**Files:**

- Add or modify a focused helper module under `src/execution/`.
- Modify: `src/execution/query.rs`
- Modify: `src/execution/stale_target_selection.rs`
- Modify: `src/execution/review_state.rs`
- Modify: `tests/runtime_module_boundaries.rs`

**Implementation Steps:**

1. Introduce `task_scope_key_task_number(scope_key: &str) -> Option<u32>` with exact `task-<u32>` semantics.
2. Replace all local parsing with the helper.
3. Add unit tests for accepted and rejected keys.
4. Add a source-boundary scanner that allows parsing only in the helper module.

**Validation Expectations:**

- Targeted helper tests pass.
- Strict clippy and full nextest pass before review.
- Clean-context review confirms no duplicate task scope-key parser remains.

### Task 4 - Remove mutation-helper dependency from public repair-target projection

**Spec Coverage:** Read/write boundary, repeated IO reduction, route projection purity.

**Goal:** `public_repair_targets` must compute repair-target candidates from the already-loaded authoritative state and immutable snapshots, without importing mutation helpers that reload transition state.

**Context:**

- `src/execution/public_repair_targets.rs` imports `current_task_closure_worktree_lease_cleanup_would_mutate` from `recording`.
- That helper reloads authoritative transition state even though `public_repair_target_candidates_from_authority` already receives it.

**Constraints:**

- Preserve existing worktree-lease cleanup semantics.
- Do not append events or claim write authority from projection code.
- Avoid repeated transition-state loads in route/read projection hot paths.

**Done when:**

- Projection code depends on read-only helpers only.
- Mutation helpers can reuse read-only decision helpers, but read projections do not import mutation modules.
- Boundary tests assert no `public_repair_targets -> recording` import edge.

**Files:**

- Modify: `src/execution/public_repair_targets.rs`
- Modify: `src/execution/recording.rs`
- Add or modify read-only helper module if needed.
- Modify: `tests/runtime_module_boundaries.rs`

**Implementation Steps:**

1. Extract a read-only worktree-lease cleanup decision helper that accepts authoritative state and active lease snapshots.
2. Let `recording.rs` call the helper for mutation paths.
3. Let `public_repair_targets.rs` call the helper using already-loaded authoritative state.
4. Add boundary coverage that projection modules do not import mutation modules.

**Validation Expectations:**

- Targeted projection and boundary tests pass.
- Strict clippy and full nextest pass before review.
- Clean-context review confirms no repeated transition-state load in the projection path.

### Task 5 - Centralize close-current-task already-current replay decisions

**Spec Coverage:** Runtime churn, idempotent close, summary-drift semantics.

**Goal:** The equivalent replay, positive summary-drift replay, conflict, and negative-result blocker branches in `close-current-task` must be represented by one shared decision helper reused before and after dispatch refresh.

**Context:**

- `src/execution/commands/close_current_task.rs` contains similar already-current/conflict/negative-result logic around lines 33-303, 402-608, and later post-refresh positive/negative branches.

**Constraints:**

- Preserve idempotent replay without summary artifacts.
- Preserve fail-closed behavior for conflicting equivalent-state inputs.
- Preserve postcondition cleanup and worktree lease release when mutation is authorized.

**Done when:**

- The duplicate already-current/conflict logic is collapsed into a shared helper or explicitly documented single owner.
- Both pre-refresh and post-refresh paths call the same helper for the same semantic question.
- Existing public replay tests still pass, with added coverage if behavior changes.

**Files:**

- Modify: `src/execution/commands/close_current_task.rs`
- Add targeted unit/integration tests if needed.

**Implementation Steps:**

1. Define a decision enum for already-current close-current-task replay outcomes.
2. Move equivalent replay, summary drift replay, conflict, and negative-result blocker checks into one helper.
3. Reuse that helper at the initial-current, post-dispatch-refresh, and post-lineage-refresh decision points.
4. Keep output construction explicit enough that public failure payloads do not change unintentionally.

**Validation Expectations:**

- Targeted close-current-task replay tests pass.
- Strict clippy and full nextest pass before review.
- Clean-context review confirms no duplicated semantic branch remains.

### Task 6 - Reduce generated skill control-plane duplication while preserving route law

**Spec Coverage:** Prompt surface, signal-to-noise, generated packaging.

**Goal:** Route-owning workflow skills keep mandatory top-level route law; non-routing skills stop receiving the full installed control-plane block by default.

**Context:**

- `scripts/gen-skill-docs.mjs::generatePreamble` injects `Installed Control Plane` into every generated skill.
- `tests/codex-runtime/skill-doc-contracts.test.mjs` enforces that full block for every generated skill.
- Non-routing skills such as `brainstorming`, `project-memory`, and `writing-skills` now carry route law they do not normally execute.

**Constraints:**

- Do not remove mandatory route law from `using-featureforge`, `executing-plans`, `subagent-driven-development`, `requesting-code-review`, `document-release`, `finishing-a-development-branch`, or review skills that invoke workflow/operator.
- Non-routing skills may keep a compact pointer only when useful.
- Generated docs must stay fresh and within budget.

**Done when:**

- The generator supports route-law modes.
- Contract tests enforce full control-plane route law only for route-owning skills.
- Non-routing generated skills are shorter and more actionable.

**Files:**

- Modify: `scripts/gen-skill-docs.mjs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify generated `skills/**/SKILL.md`
- Modify templates only if they need explicit route-law placeholders.

**Implementation Steps:**

1. Add a route-law mode argument to `generatePreamble`.
2. Define the route-owning generated skill set in one test/generator-visible location or duplicated only as a documented test fixture.
3. Emit full Installed Control Plane law only for route-owning skills.
4. Emit a compact route-reference pointer or no route block for non-routing skills.
5. Regenerate skills and update budgets if line reductions require budget corrections.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check` passes after regeneration.
- `node --test tests/codex-runtime/*.test.mjs` passes.
- Strict clippy and full nextest pass before review.
- Clean-context review confirms the prompt surface got smaller, not just reshuffled.

### Task 7 - Trim brittle static architecture tests to durable boundaries

**Spec Coverage:** Signal-to-noise, modularization enforcement, test realism.

**Goal:** Keep static Rust boundary tests for import direction, module ownership, line caps, and vocabulary centralization, but remove exact private helper-name/prose-shape pins where public behavior or import boundaries are the real contract.

**Context:**

- `tests/runtime_module_boundaries.rs` contains exact helper name and string-shape checks in places that read like a private architecture spec.
- `docs/testing.md` already says helper-name pins should be reserved for named boundary-owner entrypoints.

**Constraints:**

- Do not remove tests that caught real public/private drift or route decision split-brain.
- Replace brittle private-name checks with durable boundary assertions or public behavior tests.
- Keep module line caps and import-direction checks.

**Done when:**

- Static tests no longer pin incidental helper names unless the helper is a named boundary-owner entrypoint.
- New/updated tests cover the risks addressed in Tasks 1-4 through durable contracts.
- `docs/testing.md` remains aligned with the suite.

**Files:**

- Modify: `tests/runtime_module_boundaries.rs`
- Modify or add targeted public behavior tests as needed.
- Modify: `docs/testing.md` only if the policy needs clarification.

**Implementation Steps:**

1. Review each newly added or noisy static check from the latest remediation.
2. Delete exact private helper-name pins that do not represent a public boundary.
3. Replace them with import-direction checks, centralized helper ownership checks, or public output tests.
4. Confirm route-golden and liveness coverage still catch user-facing regressions.

**Validation Expectations:**

- Targeted boundary tests pass.
- Strict clippy and full nextest pass before review.
- Clean-context review confirms test coverage is lower-noise without losing risk coverage.

### Task 8 - Full audit-loop verification and final clean-context review

**Spec Coverage:** End-to-end validation and independent review.

**Goal:** After all remediation tasks pass per-task review, run the whole validation set and dispatch a clean-context reviewer against this full plan.

**Context:**

- This branch’s safety depends on public runtime behavior, generated prompts, schema/docs, and modularization boundaries.

**Constraints:**

- Do not dispatch review before strict clippy and full nextest are clean.
- Do not allow review subagents to spawn subagents.
- If review finds actionable issues, remediate, rerun validation, and rereview.

**Done when:**

- Full validation passes:
  - `node scripts/gen-skill-docs.mjs --check`
  - `node scripts/gen-agent-docs.mjs --check`
  - `node --test tests/codex-runtime/*.test.mjs`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
  - `cargo test --test liveness_model_checker`
  - `git diff --check`
- Clean-context whole-plan review returns no actionable findings.
- If full nextest exceeds 10 minutes after a clean rerun, performance is fixed before completion. A 4-5 minute clean run remains the preferred health target, but the hard remediation gate for this plan is the user-approved 10-minute stop rule.

**Files:**

- All files touched by Tasks 1-7.

**Implementation Steps:**

1. Run full validation.
2. Dispatch a clean-context review agent with this exact plan and explicit no-subagent instruction.
3. Remediate any finding in task order or closest relevant task.
4. Repeat validation and review until no actionable findings remain.

**Validation Expectations:**

- Final validation and review results are recorded in the implementation summary.
