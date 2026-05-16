# Twenty-Seventh Deep Runtime Safety Audit

Audit date: 2026-05-13

Audit mode: read-only parent synthesis plus nine clean-context audit subagents. No FeatureForge runtime/project skills were used. Subagents were instructed not to spawn additional subagents. Parent validation used normal repository validation commands only.

## Executive Verdict

Recommendation: ship only after targeted fixes.

The branch is close and the runtime is no longer structurally unsafe in the original sense. Public routes are reachable, typed argv/template authority is the executable contract, receipts/projections are not acting as normal-path control-plane truth, stale closure/reentry loops have strong model coverage, and prompt compaction is enforced.

The remaining actionable issues are not fresh dead ends, but they are exactly the kind of drift and maintenance pressure that has caused previous FeatureForge loops: duplicated repair-target assembly, duplicated review/resume classification, and tests/goldens that pin too much incidental shape. These should be fixed before shipping because they would make the next change expensive and could reintroduce split decisioning.

## What Is Genuinely Fixed

- Public `begin`, `close-current-task`, `advance-late-stage`, `reopen`, `transfer`, and `repair-review-state` are shipped CLI commands with typed public command models.
- Normal routing uses `recommended_public_command_argv` or a bound `recommended_public_command_template`; `recommended_command` is display-only.
- Public `begin` owns execution preflight setup.
- Public `close-current-task` can record/refresh closure internally without hidden dispatch repair.
- Public `advance-late-stage` owns late-stage intent progression.
- Projection-only stale targets are diagnostic-only and cannot drive public repair.
- Current closure/stale overlap and targetless stale states converge in the liveness model.
- Plan-fidelity uses parseable artifacts, not hidden runtime receipt recording.
- Engineering-review edits do not immediately bounce back to fidelity before final refresh.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped.
- Generated skills and agents are fresh; prompt budgets are enforced.
- `state.rs` and `mutate.rs` are thin facades and import-boundary coverage is materially stronger.

## What Remains Risky

- Public repair-target creation is still split between route projection and authoritative repair-target candidates.
- Review-state effective status is classified once for status projection and again for route planning.
- Resume-vs-stale precedence is distributed across status assembly, route ordering, stale repair target logic, and repair target selection.
- Public route goldens duplicate large status/operator payload shapes, creating churn risk when externally irrelevant fields move.
- Static scanners and scanner self-tests protect real regressions but are becoming a second architecture language.
- Prompt budget tests enforce useful caps but still assert several exact documentation phrases.

## Concrete Dead Ends Still Possible

No confirmed user-facing public-runtime dead end was found in this audit.

The closest evidence gap is public route golden coverage for routed `reopen` and `transfer`: the commands are public and typed, but the main public-route golden set does not pin those route examples. That is a coverage gap, not a discovered break.

## Concrete Churn Sources Still Possible

1. Route-local public repair targets and authoritative public repair targets can drift on close-current-task/reopen/repair target semantics.
2. `review_state_status` can drift between `derive_status_review_state_fact` and `canonical_review_state_status`.
3. Resume/stale ordering can drift because suppression, route facts, stale repair candidates, and repair target selection each encode pieces of precedence.
4. Public route goldens duplicate status/operator JSON bulk instead of focusing on semantic route fields.
5. Scanner self-tests and documentation regex checks can fail on wording or synthetic shape changes unrelated to shipped runtime behavior.

## Public/Private Test Mismatch Assessment

Public/private separation is fixed enough for public proof. Public-flow tests use the compiled CLI where they claim runtime behavior, internal helpers are quarantined in `tests/internal_*.rs` or explicit internal support files, and static guards reject hidden helper use in protected public-flow tests.

Residual issue: some tests named like public-flow proof are static scanner or scanner-self-test coverage. That is not wrong, but it should be labeled and slimmed so the suite does not imply every scanner assertion is shipped public-runtime evidence.

## Receipt/Evidence/Projection Control-Plane Assessment

No confirmed receipt/provenance/projection control-plane defect was found. Projection materialization is explicit; projection-only stale IDs are diagnostic-only; summary drift no longer forces reentry when pass/pass current closure authority is sufficient.

Policy caveat: current task closure can intentionally become stale after covered-path drift and block downstream `begin` via `prior_task_current_closure_stale`. That is authoritative closure-vs-workspace validation, not receipt/projection leakage.

## Prompt-Surface And Packaging Assessment

Prompt-surface compaction is value-positive and enforced. Current generated skill line count is within budget (`4990 / 5015` during validation), generated docs are fresh, and companion references are packaged/discoverable.

Residual risk is signal-to-noise: mandatory law remains top-level, but README, generated preambles, and tests still repeat enough negative law and exact documentation wording that future edits can churn without changing agent behavior.

## Modularization And Split-Decisioning Assessment

Core modularization is improved. The main flow still matches:

CLI args -> command module -> transition guard -> event append -> reducer -> read model -> route decision -> workflow operator presentation.

Remaining split decisioning:

- `src/execution/route_plan/status_projection.rs:122` builds route-local public repair targets while `src/execution/public_repair_targets.rs:44` independently builds authoritative public repair candidates.
- `src/execution/status_assembly.rs:1395` derives status review-state while `src/execution/route_plan/next_action_choice/types.rs:173` re-canonicalizes the same semantic field for routing.
- `src/execution/status_assembly.rs:765`, `src/execution/route_plan/next_action_choice/execution_ordering.rs:48`, `src/execution/repair_target_selection.rs:312`, and `src/execution/route_plan/stale_repair_target.rs:54` each own part of resume/stale precedence.
- New focused semantic modules are not all covered by module-boundary caps/import checks.

## Reviewer Recursion Assessment

Fixed. Reviewer recursion prevention is prompt text only, reviewer-prompt scoped, and tests reject runtime/env recursion enforcement.

## Validation Results

Passed:

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`: 136 passed
- `cargo clippy --all-targets --all-features -- -D warnings`: passed after clean rebuild, `real 47.85`
- `cargo nextest run --all-features --no-fail-fast --test runtime_authority_contracts --test workflow_runtime --test workflow_shell_smoke --test workflow_entry_shell_smoke --test plan_execution --test plan_execution_final_review --test workflow_runtime_final_review --test contracts_execution_runtime_boundaries --test execution_query`: 332 passed, `real 120.68`
- `cargo test --test liveness_model_checker -- --nocapture`: 32 passed, `real 42.05`

Implementation-review validation immediately before this audit also passed:

- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`: 1715 passed, `real 210.74`

## Prioritized Findings

### Blocker

None.

### High

1. Public repair-target decisioning is still split across route projection and authority candidates.
   - Classification: architecture issue / future route drift risk.
   - Files/functions: `src/execution/route_plan/status_projection.rs:122` (`public_repair_targets_from_route_decision`), `src/execution/public_repair_targets.rs:44` (`public_repair_target_candidates_from_authority`), `src/execution/public_repair_targets.rs:145` (`push_task_closure_repair_targets`).
   - Evidence: both modules construct `close-current-task` public repair targets with local dedupe/selection logic.

### Medium

1. Review-state status has two semantic classifiers.
   - Classification: split decisioning.
   - Files/functions: `src/execution/status_assembly.rs:1130` / `src/execution/status_assembly.rs:1395` (`derive_status_review_state_fact`), `src/execution/route_plan/next_action_choice/types.rs:173` (`canonical_review_state_status`).
   - Risk: status projection and route planning can diverge on the effective review-state status.

2. Resume-vs-stale precedence is distributed across multiple modules.
   - Classification: split decisioning / convergence risk.
   - Files/functions: `src/execution/status_assembly.rs:765` (`suppress_preempted_resume_status_fields`), `src/execution/route_plan/next_action_choice/execution_ordering.rs:48` (`execution_route_facts`), `src/execution/repair_target_selection.rs:312` / `src/execution/repair_target_selection.rs:362`, `src/execution/route_plan/stale_repair_target.rs:54`.
   - Risk: future stale/resume changes must update several local precedence encodings.

3. Public route goldens pin duplicated incidental JSON shape.
   - Classification: test signal-to-noise.
   - Files: `tests/fixtures/runtime-goldens/public-runtime-routes.json`, `tests/runtime_behavior_golden.rs`.
   - Risk: route-contract changes churn bulk status/operator fixture JSON even when semantic behavior is unchanged.

4. Static scanner infrastructure is becoming a second architecture language.
   - Classification: test signal-to-noise.
   - Files: `tests/public_flow_scan_contracts.rs:332`, `tests/support/public_flow_scan.rs:296`, `docs/testing.md:351`.
   - Risk: scanner self-tests can grow independently of concrete shipped-runtime failure modes.

5. Prompt budget tests still pin documentation prose more than behavior.
   - Classification: documentation/test signal-to-noise.
   - Files: `tests/codex-runtime/skill-doc-budget.test.mjs:120`.
   - Risk: exact prose edits can fail budget-law tests without weakening the budget contract.

### Low

1. Public route golden coverage for routed `reopen` and `transfer` is thin.
   - Classification: coverage evidence gap.
   - Files: `tests/fixtures/runtime-goldens/public-runtime-routes.json`, `src/execution/commands/reopen.rs:27`, `src/execution/commands/transfer.rs:37`.

2. Boundary coverage misses a few focused semantic modules.
   - Classification: boundary-test gap.
   - Files: `src/execution/route_plan/public_commands.rs`, `src/execution/command_eligibility/mutation_request.rs`, `src/execution/current_task_closure_cleanup.rs`, `src/execution/task_scope_key.rs`, `tests/runtime_module_boundaries.rs`.

## Recommendation

Ship only after targeted fixes in `docs/featureforge/archive/runtime-safety-audit-history/plans/2026-05-13-runtime-signal-noise-twenty-seventh-audit-remediation.md`.
