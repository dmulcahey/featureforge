# Workflow State

Engineering Approved

# Plan Revision

Revision 1 - 2026-05-13

# Execution Mode

featureforge:executing-plans

# Goal

Remediate the twenty-seventh runtime safety audit findings by reducing duplicated decisioning and low-signal meta-infrastructure without weakening the public runtime contract. The runtime should keep typed public routes authoritative, but route repair targets, review-state status, stale/resume precedence, public goldens, scanner tests, and prompt-budget tests should have one clear owner each.

Source audit: `docs/featureforge/archive/runtime-safety-audit-history/2026-05-13-twenty-seventh-audit-report.md`

# Architecture

- Public route decisions remain execution-owned. Workflow/status/operator surfaces may project finalized decisions, but should not independently assemble equivalent repair targets.
- Repair-target assembly should have one owner that accepts route-owned decisions plus authoritative candidates and returns final deduped public repair targets.
- Effective review-state status should be derived once as a route/status fact and reused by route planning, not recomputed under a second policy name.
- Resume/stale precedence should be encoded in one shared route fact used by status suppression, route ordering, stale repair routing, and repair target selection.
- Public route goldens should pin externally visible route behavior, not duplicate every incidental payload field across status/operator surfaces.
- Static scanners and prompt-budget tests should enforce historical public failures and mandatory law, not create parallel architecture specs from synthetic shapes or exact prose.

# Change Surface

- `src/execution/public_repair_targets.rs`
- `src/execution/route_plan/status_projection.rs`
- `src/execution/status_assembly.rs`
- `src/execution/status_assembly/facts.rs`
- `src/execution/route_plan/next_action_choice/types.rs`
- `src/execution/route_plan/next_action_choice/execution_ordering.rs`
- `src/execution/route_plan/stale_repair_target.rs`
- `src/execution/repair_target_selection.rs`
- `src/execution/route_plan/public_commands.rs`
- `src/execution/command_eligibility/mutation_request.rs`
- `src/execution/current_task_closure_cleanup.rs`
- `src/execution/task_scope_key.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/fixtures/runtime-goldens/public-runtime-routes.json`
- `tests/public_flow_scan_contracts.rs`
- `tests/support/public_flow_scan.rs`
- `tests/codex-runtime/skill-doc-budget.test.mjs`
- `docs/testing.md`

# Preconditions

- Do not use FeatureForge runtime/project skills.
- Do not let review subagents spawn additional subagents.
- Use the requested Rust guidance when writing or refactoring Rust.
- Before every full test cycle, verify no `cargo`, `rustc`, `cargo nextest`, `cargo-nextest`, `nextest run`, or active `target/debug/deps/` process is already running.
- Before each new audit-loop iteration, run `cargo clean`.
- After each task implementation, run strict clippy and full no-fail-fast nextest before dispatching review.
- If full nextest takes more than 4-5 minutes, run `cargo clean`, rerun the suite, and remediate repeatable performance regressions. If it exceeds 10 minutes, stop immediately and enter clean/rerun/performance remediation.
- Do not weaken hidden/debug command scanners, typed public argv/template contracts, or prompt budget enforcement to reduce noise.

# Known Footguns / Constraints

- Do not replace split repair-target construction with another local route helper under a different name.
- Do not remove valid authoritative repair candidates; centralize and dedupe them.
- Do not make `resume_task` or `resume_step` authoritative unless the exact legal route remains a matching `begin`.
- Do not make public route goldens so small that route regressions become invisible. Keep semantic parity assertions for phase detail, state kind, typed argv/template, required inputs, blockers, and reason codes.
- Do not delete scanner coverage for hidden helpers or display-command execution. Delete or collapse only scanner self-tests that do not map to a named historical failure.
- Do not move mandatory top-level route law entirely into companion references.
- Do not add `#[allow(clippy::...)]` or weaken lint policy.

# Requirement Coverage Matrix

| Requirement | Task Coverage |
| --- | --- |
| Public repair-target assembly has one owner | Task 1 |
| Route projection consumes centralized repair-target assembly | Task 1 |
| Effective review-state status is derived once | Task 2 |
| Route planning uses shared review-state status fact | Task 2 |
| Resume/stale precedence has one shared decision object | Task 3 |
| Status suppression, route ordering, stale repair, and repair-target selection reuse that object | Task 3 |
| Public route goldens pin behavior, not duplicated incidental JSON | Task 4 |
| Routed `reopen` and `transfer` have public-route evidence | Task 4 |
| Static scanner and prompt-budget tests keep high-value law while deleting low-signal prose/shape pins | Task 5 |
| Focused semantic modules are covered by boundary caps/import tests | Task 5 |

# Tasks

## Task 1 - Centralize Public Repair-Target Assembly

### Spec Coverage

- Public repair-target decisioning is still split across route projection and authority candidates.
- Public route projection should not duplicate public repair target construction.

### Goal

Make `src/execution/public_repair_targets.rs` the single assembler for public repair targets. Route projection should pass route facts and authority candidates into that owner, then consume the final deduped result.

### Context

`public_repair_targets_from_route_decision` in `src/execution/route_plan/status_projection.rs` creates route-local `reopen` and `close-current-task` targets and dedupes them. `public_repair_target_candidates_from_authority` plus `push_task_closure_repair_targets` in `src/execution/public_repair_targets.rs` independently creates similar targets from authoritative state and status. The duplicate constructors currently agree, but they are a drift risk.

### Constraints

- Preserve diagnostic-only route behavior: blocked runtime/reconcile/targetless stale routes must still expose no public repair target.
- Preserve external-wait behavior: external wait routes must not expose local mutation repair targets.
- Preserve all existing reason codes unless a test proves a reason code is purely duplicate/incidental.
- Do not move route planning into `public_repair_targets.rs`; only centralize final repair-target assembly.

### Done when

- Route projection no longer constructs `PublicRepairTarget` literals directly.
- `public_repair_targets.rs` accepts the route decision, status, and authority candidates and returns final public repair targets.
- `close-current-task`, `reopen`, persisted follow-up, and authority cleanup target construction live in one module.
- Tests prove route-local and authority candidates dedupe through the central assembler.
- Boundary tests reject `PublicRepairTarget {` literals in route projection modules.

### Files

- `src/execution/public_repair_targets.rs`
- `src/execution/route_plan/status_projection.rs`
- `src/execution/route_plan/status_application.rs` if needed
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs` if route target assertions need updates

### Detailed implementation steps

1. Add a public(crate) assembly function in `public_repair_targets.rs`, for example `public_repair_targets_for_route_decision(status, route_decision, authority_candidates)`.
2. Move route-local `reopen` and `task_closure_recording_ready` target construction into that function or private helpers in the same module.
3. Move route-local allow/deny predicates from `status_projection.rs` into `public_repair_targets.rs`.
4. Replace `public_repair_targets_from_route_decision` in `status_projection.rs` with a call to the central assembler.
5. Preserve targetless stale and diagnostic-only suppression at the central entrypoint.
6. Add unit tests for:
   - route-owned `reopen` target only.
   - route-owned `close-current-task` target only.
   - duplicate route/authority close-current-task candidate dedupes to one target.
   - diagnostic-only route returns no target even when authority candidates exist.
7. Add or update boundary tests so route projection cannot create `PublicRepairTarget` literals directly.

### Validation expectations

- Targeted: `cargo test --lib public_repair_targets route_plan -- --nocapture` or closest supported module filters.
- Targeted: `cargo test --test runtime_module_boundaries -- --nocapture`.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 1 after full validation.

## Task 2 - Share Effective Review-State Status

### Spec Coverage

- Review-state status still has two semantic classifiers.
- Route planning should reuse status assembly's effective review-state fact instead of re-canonicalizing a second policy.

### Goal

Derive effective review-state status once and make both status projection and route planning consume the same fact.

### Context

`derive_status_review_state_fact` computes `status.review_state_status`. Route planning then calls `canonical_review_state_status`, which can reinterpret a `clean` status into `missing_current_closure` based on branch closure facts. This split means route selection can use a different effective review-state status than the status projection presents.

### Constraints

- Preserve public `review_state_status` values unless tests show a value is already wrong.
- Keep the branch-closure refresh override behavior; move it into the shared fact, not out of the system.
- Do not make workflow/operator compute review-state status independently.

### Done when

- There is one shared function/type for effective review-state status.
- `derive_status_review_state_fact` and route planning use that shared function/type.
- `canonical_review_state_status` is removed or becomes a thin wrapper around the shared owner.
- Tests cover branch closure refresh, stale unreviewed, missing current closure, and clean status consistency between status and route planning.

### Files

- `src/execution/status_assembly.rs`
- `src/execution/status_assembly/facts.rs`
- `src/execution/route_plan/next_action_choice/types.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/unit_tests.rs`
- `tests/runtime_module_boundaries.rs`

### Detailed implementation steps

1. Identify or create an execution-owned home for `EffectiveReviewStateStatus`, preferably `status_assembly/facts.rs` if it already owns status facts.
2. Move branch-closure refresh override logic out of `canonical_review_state_status` and into the shared derivation path.
3. Make `derive_status_review_state_fact` call the shared function.
4. Make route planning consume the already-derived fact or call the same shared function with the same inputs.
5. Delete the second local classifier or keep a deprecated wrapper only if tests require a transition.
6. Add tests proving status projection and route planning see the same effective status for:
   - clean/no branch refresh.
   - clean plus missing current branch closure refresh.
   - stale unreviewed.
   - missing current closure reason code.
7. Add a boundary/static test rejecting a second review-state status classifier in route planning.

### Validation expectations

- Targeted: `cargo test --lib status_assembly route_plan -- --nocapture` or closest supported filters.
- Targeted: `cargo test --test runtime_module_boundaries -- --nocapture`.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 2 after full validation.

## Task 3 - Centralize Resume/Stale Precedence

### Spec Coverage

- Resume-vs-stale precedence is distributed across multiple modules.
- Resume fields remain diagnostic unless the exact legal command is the same `begin`.

### Goal

Create one shared resume/stale precedence decision object and reuse it in status suppression, route ordering, stale repair candidates, and repair target selection.

### Context

Resume/stale precedence currently appears in:

- `suppress_preempted_resume_status_fields`
- `execution_route_facts`
- `resume_step_preempts_later_stale_target`
- `exact_resume_stale_record_task` / `stale_resume_begin_route_candidate`

This is better than earlier targetless stale behavior, but future changes still have to update several local implementations.

### Constraints

- Do not weaken existing liveness guarantees.
- Do not treat resume fields as authoritative without a matching task/step/fingerprint legal begin route.
- Preserve targetless stale reconcile behavior.
- Keep the shared object pure/read-only.

### Done when

- A single shared `ResumeStalePrecedence` or equivalent fact computes earliest stale boundary, exact resume stale binding, resume-preempted-by-stale, stale-preempted-by-resume, and targetless stale reconcile.
- Status suppression consumes this fact.
- Route ordering consumes this fact.
- Stale repair target candidate logic consumes this fact.
- Repair target selection consumes this fact.
- Liveness tests still pass and at least one new regression proves the shared fact is the only allowed source for exact resume/stale binding.

### Files

- `src/execution/route_plan/next_action_choice/execution_ordering.rs`
- `src/execution/route_plan/stale_repair_target.rs`
- `src/execution/repair_target_selection.rs`
- `src/execution/status_assembly.rs`
- new focused helper module if needed, such as `src/execution/resume_stale_precedence.rs`
- `src/execution/mod.rs`
- `tests/liveness_model_checker.rs`
- `tests/runtime_module_boundaries.rs`

### Detailed implementation steps

1. Design a small read-only fact type with explicit fields rather than a broad boolean soup.
2. Build it from `ExecutionContext`, `PlanExecutionStatus`, and `NextActionAuthorityInputs` or equivalent inputs already available to route planning.
3. Replace local stale/resume computations in route ordering with the shared fact.
4. Replace status suppression's local precedence checks with the shared fact or a status-only projection of it.
5. Replace `resume_step_preempts_later_stale_target` with a call into the shared fact.
6. Replace stale resume begin candidate logic with a call into the shared fact.
7. Add boundary tests preventing new local `resume_task`/`stale target` precedence helpers outside the owner, with explicit allowlisted wrappers if necessary.
8. Run liveness model checker targeted before the full gate.

### Validation expectations

- Targeted: `cargo test --test liveness_model_checker -- --nocapture`.
- Targeted: route-plan and repair-target tests.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 3 after full validation.

## Task 4 - Slim Public Route Goldens And Add Missing Route Evidence

### Spec Coverage

- Public route goldens pin duplicated incidental shape.
- Routed `reopen` and `transfer` need public-route evidence.

### Goal

Keep public route golden coverage high-value while reducing duplicated JSON bulk. Add explicit public-route evidence for `reopen` and `transfer`.

### Context

`public-runtime-routes.json` duplicates large `plan_execution_status` and `workflow_operator` payloads for many scenarios. That catches real drift but also churns when unrelated fields move. The audit also found the main golden set does not clearly cover routed `reopen` and `transfer`.

### Constraints

- Do not remove coverage for typed argv/template, required inputs, phase detail, state kind, blocker/reason codes, or display-only summary absence.
- Do not rely on display-command parsing.
- If the fixture format changes, update `tests/fixtures/runtime-goldens/README.md`.

### Done when

- The public route golden fixture stores compact semantic route captures for duplicate status/operator surfaces where full payloads are not needed.
- Tests assert semantic parity between status/operator route fields instead of duplicating entire payloads.
- Public route golden coverage includes routed `reopen`.
- Public route golden coverage includes routed `transfer`.
- README explains what is full payload coverage versus compact semantic route coverage.

### Files

- `tests/runtime_behavior_golden.rs`
- `tests/fixtures/runtime-goldens/public-runtime-routes.json`
- `tests/fixtures/runtime-goldens/README.md`
- Public replay fixtures/tests if easiest source for routed reopen/transfer scenarios

### Detailed implementation steps

1. Inventory current golden scenarios and identify duplicated status/operator fields that can become semantic captures.
2. Update the golden capture/normalization code to compare semantic fields for most status/operator pairs:
   - `phase`
   - `phase_detail`
   - `review_state_status`
   - `operator_state_kind`
   - `next_action`
   - typed argv/template/required inputs
   - blockers and reason codes
3. Keep full payload captures for one or two representative schema compatibility cases.
4. Add routed `reopen` and `transfer` scenarios using existing public replay setup helpers.
5. Regenerate/update the fixture intentionally.
6. Update README to document compact semantic captures and the reason for reduced duplication.
7. Ensure schema-required field checks still run against real outputs before compaction.

### Validation expectations

- Targeted: `cargo test --test runtime_behavior_golden -- --nocapture`.
- Targeted: any public replay tests used for new scenarios.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 4 after full validation.

## Task 5 - Reduce Low-Signal Static/Prompt Tests And Extend Boundary Coverage

### Spec Coverage

- Static scanner infrastructure is becoming a second architecture language.
- Prompt-budget tests pin documentation prose more than behavior.
- Boundary coverage misses focused semantic modules.

### Goal

Keep tests that protect concrete historical failures and mandatory prompt law, while deleting/collapsing synthetic scanner/prose assertions that do not map to shipped behavior. Add boundary coverage for new focused semantic modules.

### Context

Scanner self-tests in `tests/public_flow_scan_contracts.rs` and prose-heavy assertions in `skill-doc-budget.test.mjs` are value-positive in origin but now contribute maintenance cost. At the same time, a few new semantic modules are not under the line/import-boundary coverage used for surrounding modules.

### Constraints

- Do not weaken hidden-command, internal-helper, retired artifact, display-command, or typed-route contract scanners.
- Do not remove prompt budget enforcement or mandatory-law retention checks.
- Every deletion must be justified by either duplicate coverage or lack of mapping to a named historical failure.
- Boundary tests should enforce module ownership/import direction, not private helper names.

### Done when

- Scanner self-tests are reduced to cases tied to named historical failure classes.
- `docs/testing.md` accurately labels scanner tests as static guard support, not public runtime behavior proof.
- `skill-doc-budget.test.mjs` keeps:
  - manifest shape.
  - total and per-skill budget enforcement.
  - enforce mode.
  - one docs pointer to the release validation command.
  - one pointer to explicit prompt-budget review.
- Exact prose regex assertions that do not protect mandatory law are removed or narrowed.
- Boundary tests include `route_plan/public_commands.rs`, `command_eligibility/mutation_request.rs`, `current_task_closure_cleanup.rs`, and `task_scope_key.rs` with reasonable line/import constraints.

### Files

- `tests/public_flow_scan_contracts.rs`
- `tests/support/public_flow_scan.rs`
- `tests/codex-runtime/skill-doc-budget.test.mjs`
- `docs/testing.md`
- `tests/runtime_module_boundaries.rs`
- `scripts/run-public-runtime-flow-tests.sh` if labels need refinement

### Detailed implementation steps

1. Map each scanner self-test to a named failure class. Keep hidden helper, display command, token-only blocked follow-up, and stale-dispatch regressions only where they are not covered elsewhere.
2. Delete or collapse scanner self-tests that only prove parser flexibility for synthetic renamed/distant shapes unless a concrete production failure requires that shape.
3. Update `docs/testing.md` and script comments so static scanner support is explicitly distinct from public CLI behavior proof.
4. Simplify the prompt-budget release-doc test to assert the budget command, manifest review requirement, and companion-reference discoverability at a semantic level.
5. Add module-boundary entries for the focused semantic modules named above.
6. Prefer import/line-cap checks over private helper-name pins.
7. Run targeted Node and Rust boundary tests.

### Validation expectations

- Targeted: `node --test tests/codex-runtime/skill-doc-budget.test.mjs`.
- Targeted: `cargo test --test public_flow_scan_contracts -- --nocapture`.
- Targeted: `cargo test --test runtime_module_boundaries -- --nocapture`.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 5 after full validation.
