# Runtime Output Route Cohesion Eighth-Audit Remediation Plan

## Workflow State

Engineering Approved

## Plan Revision

1

## Execution Mode

Sequential implementation. Complete each task in order. After each task, run strict clippy and the full nextest suite with no fail fast before clean-context review. Remediate review findings, re-run verification, and re-review until the task has no identified issues. After all tasks, run final full validation and a clean-context whole-plan review, then repeat the deep audit loop if any actionable issues remain.

## Goal

Eliminate the actionable findings from the fresh eighth audit of the updated runtime:

- Public diagnostics and active prompt surfaces must not teach manual authoritative-state edits, retired review-routing lineage concepts, or unconditional command fallbacks that compete with typed public command authority.
- Runtime truth derivation must not depend backward on read-model status builders during reduction.
- Route decision ownership must be one-way: router/shared route authority produces final route facts, while read-model projection copies and exposes those facts without revising them.
- Stale-target and review follow-up selection vocabulary must be centralized or explicitly typed so later modules cannot rederive the same semantic decision with different tie-breakers.
- Focused extracted modules must not retain broad parent glob imports that obscure boundary drift.
- Static tests must guard each of these constraints so future remediation does not reintroduce the same traps under new names.

## Architecture

Preserve the intended flow:

```text
CLI args -> command module -> transition guard -> event append -> reducer -> read model -> route decision -> workflow operator presentation
```

This plan tightens that flow:

- Public output and skills only point to public operator/status routes and typed executable argv/template fields.
- Runtime-derived truth is owned by a lower execution-truth module that reducer and read-model can both consume.
- Router owns final public route selection and blocker projection. Public route projection becomes an adapter from route decisions into public status fields.
- Stale target selectors and review-state/follow-up tokens move behind typed helpers or centralized constants, with the few intentional variants documented by name.
- Extracted modules import the exact dependencies they use.

## Change Surface

- `src/execution/state/preflight.rs`
- `src/execution/state/runtime_methods.rs`
- `src/execution/runtime_truth.rs`
- `src/execution/reducer.rs`
- `src/execution/read_model.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/router.rs`
- `src/execution/repair_target_selection.rs`
- `src/execution/repair_route_decision.rs`
- `src/execution/stale_target_projection.rs`
- `src/execution/follow_up.rs`
- possible new focused modules under `src/execution/**`
- `skills/requesting-code-review/SKILL.md.tmpl`
- `skills/requesting-code-review/SKILL.md`
- `skills/finishing-a-development-branch/SKILL.md.tmpl`
- `skills/finishing-a-development-branch/SKILL.md`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_instruction_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- targeted runtime/query/liveness tests as needed
- generated docs and goldens if route/status output changes

## Preconditions

- Do not use FeatureForge runtime skills, project skills, or repo-local skills.
- Do not let subagents spawn additional subagents.
- Use Rust coding guidance when modifying Rust.
- Preserve event-log authority and guided workflow routing.
- Do not weaken strict clippy, suppress lints, or add compatibility bypasses.
- Do not replace public runtime coverage with internal helper-only tests.
- Generated skill docs must stay template-owned: edit `.tmpl` files, regenerate checked-in `SKILL.md` output, and run generation checks.
- `recommended_public_command_argv` is exact executable authority, and `recommended_public_command_template` is bindable executable authority. Display strings are not executable authority.
- Do not revert unrelated dirty worktree changes.

## Known Footguns / Constraints

- The phrase `repair-review-state` names a current public command. It is allowed when it is returned by typed public argv/template authority or when docs instruct agents to follow the operator route. It must not be hard-coded as a manual artifact fix.
- `advance-late-stage` is a current public command. It is allowed when selected by public runtime authority. It must not appear as an unconditional fallback after text that says to follow operator argv.
- Historical review dispatch records may exist in fixtures or compatibility tests. Active public diagnostics must not tell agents to record or repair review-routing lineage.
- `handoff_required` is a runtime status field. Public diagnostics must not suggest manually clearing it in authoritative state.
- Read-model code can consume router/query DTOs for projection. It must not call router a second time to override the route decision after projection.
- Reducer can consume lower execution-truth helpers. It must not depend on read-model status construction.
- Stale-target helpers can have distinct semantics only when their type names and documentation make the distinction explicit, such as "earliest stale task" versus "actionable reentry target".
- Boundary tests should reject exact old failure shapes and structural recurrence, not just current line numbers.

## Requirement Coverage Matrix

| Requirement | Task |
| --- | --- |
| Public diagnostics do not advise manual authoritative-state edits | Task 1 |
| Public diagnostics do not expose retired review-routing lineage or repair concepts as action guidance | Task 1 |
| Active skills do not hard-code commands after claiming typed operator argv is authoritative | Task 1 |
| Static public-output and prompt tests guard the removed wording | Task 1 |
| Reducer/runtime truth no longer depends on read-model status builders | Task 2 |
| Boundary tests reject reducer -> read-model truth dependency regression | Task 2 |
| Router/shared route authority owns status blocker route integration | Task 3 |
| Public route projection no longer revises route decisions after copying them | Task 3 |
| Stale-target selectors are centralized or explicitly typed by semantic role | Task 4 |
| Review-state and follow-up tokens are centralized as constants or typed values | Task 4 |
| Focused extracted modules avoid broad `use super::*` imports | Task 5 |
| Boundary tests guard focused module imports | Task 5 |
| Full validation, clean-context whole-plan review, and follow-up audit loop are complete | Task 6 |

## Ordered Tasks

### Task 1: Remove Public-Output And Prompt Command Traps

#### Spec Coverage

- Public-output H-P1: handoff remediation must not say to clear authoritative state manually.
- Public-output H-P1: runtime diagnostics must not expose hidden or retired review-routing lineage concepts as normal action guidance.
- Public-output H-P2: final-review and finishing prompts must not hard-code commands that compete with typed public operator argv.
- Tests: static scanners must guard these patterns.

#### Goal

Make every affected public diagnostic and active prompt point to exactly one safe public route: query the workflow operator/status surface and execute only the typed public argv/template it returns, or stop on a diagnostic-only runtime bug.

#### Context

The eighth audit found three wording traps:

- `preflight.rs` tells agents to manually clear the authoritative handoff flag.
- `runtime_methods.rs` references retired review-routing lineage mechanics, phase re-derivation, and manual task-closure repair.
- `requesting-code-review` and `finishing-a-development-branch` generated skills contain hard-coded command fallbacks that can compete with operator `recommended_public_command_argv`.

#### Constraints

- Keep diagnostics actionable without inventing manual repair steps.
- Do not hide the status field name when a JSON field is being reported, but do not tell agents to edit it directly.
- Do not ban legitimate public command names globally.
- If text says typed operator argv/template is authoritative, any adjacent shell snippet must derive the command from those fields or must omit the command entirely.
- Edit skill templates first and regenerate generated docs.

#### Done when

- `src/execution/state/preflight.rs` no longer tells users to clear `handoff_required` manually.
- `src/execution/state/runtime_methods.rs` no longer uses retired review-routing lineage language or non-public repair verbs in active diagnostics.
- `skills/requesting-code-review/SKILL.md.tmpl` and generated `SKILL.md` no longer end an operator-guided final-review block with unconditional `advance-late-stage`.
- `skills/finishing-a-development-branch/SKILL.md.tmpl` and generated `SKILL.md` no longer hard-code `repair-review-state` for missing or invalid `QA Requirement`; they require following operator argv/template.
- Rust and Node static tests reject the removed wording in active production diagnostics and active generated prompt surfaces.
- Generated docs are fresh.

#### Files

- `src/execution/state/preflight.rs`
- `src/execution/state/runtime_methods.rs`
- `skills/requesting-code-review/SKILL.md.tmpl`
- `skills/requesting-code-review/SKILL.md`
- `skills/finishing-a-development-branch/SKILL.md.tmpl`
- `skills/finishing-a-development-branch/SKILL.md`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_instruction_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`

#### Implementation Steps

1. Replace handoff remediation text with public workflow/operator guidance. The acceptable shape is "publish the required public handoff through the workflow route, then retry" or "follow the operator recommended argv/template"; do not mention clearing authoritative state.
2. Replace retired review-routing lineage phrases in runtime diagnostics with neutral descriptions of runtime route inconsistency or missing runtime-owned review state. The action should be "re-query workflow operator/status and follow its typed public command" or "stop and report the diagnostic" depending on whether a public command is available.
3. Replace the old phase re-derivation and task-closure repair wording with diagnostics that describe the problem and point to the public operator route.
4. Update requesting-code-review template text so final-review progression is represented as operator-selected argv/template only. Remove unconditional `advance-late-stage` snippets from that block.
5. Update finishing-a-development-branch template text so missing/invalid `QA Requirement` routes through operator-selected argv/template only. Do not hard-code `repair-review-state`.
6. Extend Rust public-output scanners to reject:
   - manual authoritative handoff flag clearing
   - retired review-dispatch recorder wording
   - old phase re-derivation wording
   - old task-closure repair wording
7. Extend Node skill-doc scanners to reject active prompt blocks that combine `recommended_public_command_argv` authority with an unconditional hard-coded command, and to reject the direct `QA Requirement` -> hard-coded `repair-review-state` instruction.
8. Regenerate skill docs.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo test --test runtime_instruction_contracts -- --nocapture`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 2: Break Runtime Truth Back-Dependency On Read Model

#### Spec Coverage

- Modularization G-P1: reducer/runtime truth must not depend on read-model status helpers.
- Runtime flow: reducer derives state before read-model route/status presentation.
- Tests: boundary tests must make the dependency direction explicit.

#### Goal

Move the minimal status overlay and final-review dispatch authority logic needed by runtime truth into a lower focused module so reducer and read-model can both consume it without a reducer -> read-model dependency.

#### Context

`src/execution/runtime_truth.rs` imports `status_from_context_with_overlay` and final-review dispatch helpers from `read_model`. That creates a reverse dependency: runtime truth used during reduction reaches into read-model status construction.

#### Constraints

- Preserve status output and reducer behavior.
- Do not duplicate status overlay construction.
- Do not make command modules depend on read-model or projection modules.
- Prefer a focused helper module over a broad facade.
- If a helper still needs a full `PlanExecutionStatus`, name it as status assembly support and keep it lower than read-model presentation.

#### Done when

- `runtime_truth.rs` does not import `crate::execution::read_model`.
- `reducer.rs` can derive runtime truth without any read-model dependency.
- Read-model code imports the lower runtime-truth/status-support helper instead of owning the reducer-consumed helper.
- Boundary docs and tests identify the allowed dependency edge.
- Static tests reject `runtime_truth.rs` imports of `read_model` and reject reducer imports of route/status presentation helpers.

#### Files

- `src/execution/runtime_truth.rs`
- `src/execution/reducer.rs`
- `src/execution/read_model.rs`
- possible new `src/execution/runtime_truth/status_support.rs` or `src/execution/status_overlay.rs`
- `src/execution/mod.rs`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/runtime_module_boundaries.rs`

#### Implementation Steps

1. Inspect the current `runtime_truth.rs` use of read-model functions and classify each use as status overlay construction, final-review dispatch lookup, or presentation-only.
2. Move status overlay construction needed by runtime truth to a lower focused module, or move runtime-truth-specific derived inputs so it does not need full read-model status construction.
3. Move final-review dispatch authority helper access to a lower owner if it currently lives only in read-model.
4. Update `read_model.rs` to call the lower helper and remain the presentation assembler.
5. Update `runtime_truth.rs` and `reducer.rs` imports.
6. Add or update module-boundary tests for forbidden `runtime_truth -> read_model` and `reducer -> read_model` imports.
7. Update module boundary docs with the new ownership rule.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test execution_query -- --nocapture`
- `cargo test --test workflow_runtime -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 3: Make Router The Final Status-Blocker Route Owner

#### Spec Coverage

- Modularization G-P1: public route projection must not call router again to revise route decisions.
- Public route authority: status/operator route facts must come from one route decision object.
- Tests: boundary tests must reject read-model route override loops.

#### Goal

Move status-blocker integration into router/shared route authority so `public_route_projection.rs` only projects an already-final route decision onto public status fields.

#### Context

The audit found `public_route_projection.rs` first calls router to get a route decision, then recomputes blocking records and calls `route_decision_with_status_blockers` to revise the route after projection. That creates a read-model/router feedback loop and makes it unclear which decision is final.

#### Constraints

- Preserve public route outputs and blocking records.
- Do not duplicate status blocker construction.
- Do not make workflow/operator presentation recompute route blockers.
- Keep `recommended_public_command_argv` derived from typed public command objects.
- If blocker projection needs data that only read-model currently has, pass it into router as an explicit input rather than calling router a second time after projection.

#### Done when

- `public_route_projection.rs` does not call `route_decision_with_status_blockers`.
- `public_route_projection.rs` does not compute a second route decision after projecting status fields.
- Router or a router-owned child module returns final route decision plus any status-blocking records needed by projection.
- Status/operator outputs remain equivalent except for intentional wording/schema changes.
- Boundary tests reject route revision calls from read-model projection modules.
- Liveness and public replay tests still pass.

#### Files

- `src/execution/router.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/read_model.rs`
- possible new router child module for blocker integration
- `tests/runtime_module_boundaries.rs`
- `tests/public_replay_churn.rs`
- `tests/workflow_runtime.rs`
- `tests/liveness_model_checker.rs`

#### Implementation Steps

1. Trace current construction of `RouteDecision`, status blocking records, and `route_decision_with_status_blockers`.
2. Design a route output type that includes the final `RouteDecision` and the status blocker records needed for public status projection.
3. Move status-blocker integration into router/shared route authority. Avoid passing partially projected status back through router when a smaller input struct can represent blockers.
4. Change public route projection to copy the final route decision and associated blockers without calling router again.
5. Update tests/goldens if public JSON ordering or route diagnostics intentionally changes.
6. Add boundary tests that fail if read-model projection modules call `route_decision_with_status_blockers` or other route-revision functions.
7. Run liveness and public replay checks to prove no repeated route signatures are introduced.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test public_replay_churn -- --nocapture`
- `cargo test --test workflow_runtime -- --nocapture`
- `cargo test --test liveness_model_checker -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 4: Centralize Stale-Target Selection And Review Follow-Up Vocabulary

#### Spec Coverage

- Modularization G-P2: multiple stale-target selectors with different tie-breakers must be centralized or explicitly typed.
- Modularization G-P3: review-state and follow-up vocabulary must be centralized.
- Runtime convergence: stale and repair routes must not drift through duplicated string logic.

#### Goal

Make stale-target selection semantics and repair follow-up tokens explicit, reusable, and statically guarded.

#### Context

The audit found stale-target/task selection in `stale_target_projection.rs`, `repair_target_selection.rs`, and `repair_route_decision.rs`, plus raw follow-up/status strings such as `stale_unreviewed`, `missing_current_closure`, `repair_review_state`, `advance_late_stage`, and `execution_reentry` across multiple modules.

#### Constraints

- Do not collapse distinct selector semantics into one vague helper if they intentionally differ.
- Every distinct selector must have a type/function name that states its semantic role and tie-breaker.
- Prefer constants or enums over raw strings in production decisioning.
- Avoid moving mutation logic into read-model/status modules.
- Fixtures and compatibility tests may contain literal historical JSON values; production decision logic should use constants/types.

#### Done when

- Stale target selection has one shared module or typed helper set that owns:
  - earliest stale task selection
  - actionable execution reentry target selection
  - branch/late-stage stale target selection where applicable
- Any intentionally different tie-breaker is documented beside the helper.
- Follow-up/status tokens used in production decisions are constants or typed values.
- Boundary tests reject new raw production literals for centralized tokens outside allowed constant definitions, fixtures, and tests.
- Existing route behavior is preserved or route-output changes are intentional and tested.

#### Files

- `src/execution/stale_target_projection.rs`
- `src/execution/repair_target_selection.rs`
- `src/execution/repair_route_decision.rs`
- `src/execution/current_truth.rs`
- `src/execution/router.rs`
- `src/execution/follow_up.rs`
- possible new `src/execution/review_route_tokens.rs` or selector child module
- `tests/runtime_module_boundaries.rs`
- targeted runtime/query tests as needed

#### Implementation Steps

1. Inventory each stale-target selector and record its input shape, tie-breaker, and caller expectation.
2. Create a focused shared selector module or child module with typed helper names for each semantic role.
3. Replace local selector implementations with calls to the shared typed helpers.
4. Define centralized constants or a small enum for production decision tokens:
   - `stale_unreviewed`
   - `missing_current_closure`
   - `repair_review_state`
   - `advance_late_stage`
   - `execution_reentry`
5. Replace production raw string comparisons/assignments with the centralized values where doing so does not make fixtures unreadable.
6. Add static tests that reject raw decision-token literals in production modules outside the central owner or documented compatibility boundaries.
7. Add focused unit or integration tests for selector tie-breakers so centralization does not hide behavior.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test execution_query -- --nocapture`
- `cargo test --test public_replay_churn -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 5: Remove Broad Parent Globs From Focused Extracted Modules

#### Spec Coverage

- Modularization G-P4: focused modules should not hide dependency drift behind `use super::*`.
- Boundary enforcement: static tests must guard extracted module import clarity.

#### Goal

Replace broad parent imports in focused extracted runtime modules with explicit imports so future split-decisioning and write/read boundary drift is visible in review and tests.

#### Context

The audit identified `read_model/public_route_projection.rs` and similar extracted modules using `use super::*`. That pattern made sense during extraction but now masks dependencies and can bypass boundary tests that scan explicit import paths.

#### Constraints

- Do not churn every test module. `use super::*` in Rust unit test modules is acceptable.
- Prioritize focused production modules introduced or modified by the runtime split-decisioning work.
- Keep imports grouped by source module and let rustfmt order them.
- Do not change behavior.

#### Done when

- `src/execution/read_model/public_route_projection.rs` has explicit imports.
- Any other focused production modules touched by Tasks 2-4 avoid `use super::*`.
- Runtime module boundary tests reject `use super::*` in the focused extracted production modules covered by this plan.
- Clippy and full nextest pass.

#### Files

- `src/execution/read_model/public_route_projection.rs`
- focused child modules touched by Tasks 2-4
- `tests/runtime_module_boundaries.rs`

#### Implementation Steps

1. Identify production runtime modules touched by this plan that use `use super::*`.
2. Replace broad parent imports with exact parent-module or crate imports.
3. Keep test-only `use super::*` untouched unless it is inside production code.
4. Add boundary tests that scan the focused production module list for `use super::*`.
5. Run formatting, focused module-boundary tests, clippy, and full nextest.

#### Validation Expectations

- `cargo fmt --check`
- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 6: Final Validation, Clean Review, And Re-Audit Loop

#### Spec Coverage

- Whole-plan implementation validation.
- Required review and audit loop until no actionable issues remain.

#### Goal

Prove the full plan is implemented, public artifacts are fresh, and a clean-context reviewer plus a fresh audit find no actionable issues.

#### Context

Earlier implementation rounds passed targeted reviews but new audits found residual public-output and modularization issues. This final task keeps the same loop: validate first, review from clean context, remediate if needed, and re-audit.

#### Constraints

- Do not dispatch review before strict clippy and full nextest pass.
- Clean-context reviewers must be told not to run FeatureForge runtime/project skills and not to spawn subagents.
- If a reviewer finds an issue, fix it in code/tests/docs, rerun strict clippy and full nextest, and re-review.
- If the fresh audit finds actionable issues, write the next remediation plan and continue the loop.
- Refresh generated artifacts and prebuilts only when the implementation changed them or the current branch completion process requires it.

#### Done when

- Generated skill docs and agent docs are fresh.
- Node codex-runtime tests pass.
- Strict clippy passes.
- Full nextest with no fail fast passes.
- A clean-context whole-plan review reports no actionable findings.
- A fresh audit following the original A-H audit process reports no actionable findings, or any actionable findings have been converted into the next remediation plan and implementation has continued.

#### Files

- Entire plan change surface.
- Generated docs/artifacts touched by prior tasks.
- New audit report under `docs/featureforge/reference/` if actionable or useful for handoff.
- New remediation plan under `docs/featureforge/plans/` if the audit finds actionable issues.

#### Implementation Steps

1. Run generation checks:
   - `node scripts/gen-skill-docs.mjs --check`
   - `node scripts/gen-agent-docs.mjs --check`
   - `node --test tests/codex-runtime/*.test.mjs`
2. Run Rust checks:
   - `cargo fmt --check`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo nextest run --all-targets --all-features --no-fail-fast`
   - `cargo test --test liveness_model_checker -- --nocapture`
3. Dispatch a clean-context whole-plan reviewer with this plan file, repo path, and explicit no-skills/no-subagents constraints.
4. Remediate any reviewer finding and repeat validation/review until clean.
5. Perform the original A-H audit process on the updated codebase with clean-context parallel subagents.
6. If no actionable audit issues remain, record the final verdict and validation evidence.
7. If actionable audit issues remain, write the next task-contract remediation plan and continue in task order.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`
- `cargo test --test liveness_model_checker -- --nocapture`
- clean-context whole-plan review with no findings
- fresh A-H audit with no actionable findings before declaring the loop complete
