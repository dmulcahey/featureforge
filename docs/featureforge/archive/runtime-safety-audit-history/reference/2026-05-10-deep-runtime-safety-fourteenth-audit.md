# Fourteenth deep runtime-safety audit

## Executive verdict

**Ship candidate?** Not yet.

**Verdict:** Close, but not done. The current codebase is materially safer than the earlier failure states: public runtime commands are reachable, typed route output is the executable authority, current task closure dominates stale receipt/provenance drift, generated docs are fresh, and public-flow tests are mostly real shipped-runtime tests.

The remaining issues are not broad public/private mismatch regressions. They are narrower but still actionable:

- A targetless `runtime_reconcile_required` route is diagnostic in public output but still mutation-authorizes a guessed `repair-review-state`.
- Route decisioning is still split across route seeding, route planning, and status projection.
- Active generated skills still mention removed workflow helper names in negative guidance.
- Prompt and test infrastructure has started to duplicate itself enough that the next fix should delete/consolidate, not only add more scanners.

**Recommendation:** Ship only after targeted fixes. The runtime is close enough that a focused remediation pass is the right next step; it is not safe to declare the branch done while diagnostic-only reconcile can still enter a mutation lane and while route ownership remains split.

## What is genuinely fixed

- Public CLI reachability is strong. Normal execution mutations are present as public CLI variants in `src/cli/plan_execution.rs` and dispatched in `src/lib.rs`: `status`, `repair-review-state`, `close-current-task`, `advance-late-stage`, `begin`, `complete`, `reopen`, `transfer`, and `materialize-projections`.
- Public command authority is typed. `PublicRouteDecision` carries command kind, invocation, template, required inputs, and display command separately in `src/execution/route_plan/decision.rs`; status projection copies executable argv/template from the route decision rather than parsing display text.
- `begin` owns preflight setup through `src/execution/state/preflight.rs` and route-guarded `src/execution/commands/begin.rs`.
- `close-current-task` can refresh current dispatch lineage under the public command owner in `src/execution/commands/close_current_task.rs` without requiring hidden dispatch repair.
- `advance-late-stage` owns branch closure, release readiness, final review dispatch/result, QA, and finish progression through public mode-bound mutation checks.
- Current task closure is now the task-boundary authority. Current/stale closure split is explicit in `src/execution/current_closure_projection.rs` and guarded by invariants in `src/execution/invariants.rs`.
- Projection materialization is isolated in `src/execution/commands/materialize_projections.rs` and does not change runtime truth.
- Tests no longer rely on internal helper behavior for public-flow claims. Public-flow helper code executes `CARGO_BIN_EXE_featureforge`, while direct helpers are quarantined as `internal_only_`/internal tests.
- Prompt budgets are enforced and generated docs are fresh. `node scripts/gen-skill-docs.mjs --check`, `node scripts/gen-agent-docs.mjs --check`, and `node --test tests/codex-runtime/*.test.mjs` all passed.
- Reviewer recursion prevention is prompt-text scoped; no runtime/env recursion guard was found.

## What remains risky

- `runtime_reconcile_required` targetless diagnostics expose no public route, but `decide_public_mutation` still allows `repair-review-state` for any status with that phase detail.
- `public_route_selection.rs` still mutates route fields such as `phase_detail`, `next_action`, `recommended_public_command`, and `blocking_task`, while `route_plan.rs` also decides/overrides route order.
- `route_plan/status_projection.rs` recomputes `execution_reentry_target_source` even though route selection already derived it in `route_plan/follow_up.rs`.
- `closure_dispatch.rs` uses `pre_reducer_earliest_unresolved_stale_task` outside a clearly bootstrap-only path.
- Public repair-target reason vocabulary is constructed/matched with raw strings in multiple modules.
- Active generated skills still mention removed helper names in bare negative guidance.
- Rust and Node scanner coverage overlaps. Some tests now verify scanner fixture behavior more than runtime behavior.
- Runtime route goldens capture large incidental JSON shapes instead of a smaller external route contract.
- `workflow-handoff` can expose `recommended_skill = featureforge:writing-plans` for a fidelity-blocked approved plan even when the authoritative route is `featureforge:plan-eng-review`.

## Concrete dead ends still possible

### Diagnostic reconcile guessed repair loop

`route_plan/constructors.rs` and `route_plan/status_projection.rs` intentionally produce targetless `runtime_reconcile_required` with no argv/template/repair target. Public replay coverage asserts that shape in `tests/public_replay_churn.rs`.

However, `src/execution/command_eligibility.rs::decide_public_mutation` still allows `PublicMutationKind::RepairReviewState` for any `DETAIL_RUNTIME_RECONCILE_REQUIRED` status. An agent guessing `featureforge plan execution repair-review-state --plan ...` can enter the mutation path even when public output said no legal command exists. That is a user-facing churn risk: the route is diagnostic-only, but the mutation guard does not fail closed.

### Wrong handoff recommendation after fidelity block

`src/workflow/status.rs` correctly routes an `Engineering Approved` plan with missing/stale/invalid fidelity back to plan engineering review. `src/workflow/operator.rs` maps every `pivot_required` phase to `recommended_skill = featureforge:writing-plans` for handoff output. `next_skill` remains route-derived, so the main operator path is not bypassed, but a handoff consumer following `recommended_skill` can bounce to the wrong review skill.

## Concrete churn sources still possible

- Repeating route decisioning in `public_route_selection.rs`, `route_plan.rs`, and `route_plan/status_projection.rs`.
- Keeping `read_model_support.rs` as a compatibility re-export while production modules still import it.
- Using pre-reducer stale target selection after public route context exists.
- Maintaining duplicated doc/prompt scanners in Rust and Node.
- Requiring many synthetic scanner self-tests in large public contract files.
- Keeping a 204 KB full-object route golden for incidental fields.
- Repeating detailed route-law prose across generated skills instead of using one canonical reference plus short top-level rules.

## Public/private test mismatch assessment

No actionable public/private test mismatch was found.

Evidence:

- `tests/support/public_featureforge_cli.rs` invokes the compiled binary.
- `tests/support/plan_execution_direct.rs` labels direct helpers as unavailable runtime internals.
- `tests/public_cli_flow_contracts.rs` guards public-flow suites against hidden command literals and internal helper imports.
- `tests/public_replay_churn.rs` mutates synthetic event authority only to construct fixtures, then asserts recovery through compiled public CLI wrappers.
- `tests/liveness_model_checker.rs` is semantic/in-process by design and has sampled shipped-CLI parity; it should not be cited as full public CLI proof.

Assessment: fixed, with residual risk limited to keeping semantic liveness tests clearly labeled as model coverage rather than end-to-end CLI proof.

## Receipt/evidence/projection control-plane assessment

No active receipt/projection control-plane regression was found.

Evidence:

- Current closures are derived from authoritative transition state in `current_closure_projection.rs`.
- Stale target projection removes current task closures before routing can use them.
- Prior-task begin guards defer to current closure truth.
- `close-current-task` validates supplied dispatch ids before summary reads and ignores pass/pass summary drift when an equivalent current closure already exists.
- Projection writes are explicit, read-model-only, and return `runtime_truth_changed: false`.
- Normal command paths record projection fingerprints rather than writing read-model projections directly.

Intentional exception: active-contract serial unit-review proof remains authoritative for the worktree lease gate. That is a runtime-owned proof boundary, not accidental markdown/projection authority.

Assessment: fixed for the historical receipt/projection failure class.

## Plan-review and engineering-review assessment

Plan fidelity no longer depends on hidden receipt recording. Active routing uses parseable `PlanFidelityReviewReport` artifacts and the five-surface fidelity state.

Fixed:

- Fidelity artifacts are parseable and template-backed.
- Engineering-review edits stay in engineering review until the final fidelity refresh.
- Old two-surface fidelity artifacts are rejected.
- Active docs do not teach plan-fidelity receipt recording.

Remaining issue:

- Low/P3 handoff mismatch: `workflow-handoff` recommended_skill can point to writing-plans for a fidelity-blocked approved plan even though status/operator route to plan-eng-review.

## Prompt-surface and packaging assessment

Mostly fixed, but not clean.

Fixed:

- Budget enforcement is active.
- Companion references are packaged and tested.
- Generated skills and agents are fresh.
- Reviewer recursion prevention is prompt-only and reviewer-scoped.
- Skills generally point agents to `workflow operator --json`, `recommended_public_command_argv`, and `recommended_public_command_template.input_bindings`.

Remaining issues:

- Bare removed helper names still appear in active generated skills:
  - `skills/brainstorming/SKILL.md` and `.tmpl`
  - `skills/writing-plans/SKILL.md` and `.tmpl`
  - `skills/plan-ceo-review/SKILL.md` and `.tmpl`
- The contract tests reject executable removed-helper forms but not bare active prompt mentions.
- The compact doctor dashboard says to rerun with `external-review-result-ready` without naming the public operator JSON route and typed fields.
- Route-law prose is repeated across generated skills; it should be centralized into one reference with short top-level reminders.

Assessment: partially fixed.

## Modularization and split-decisioning assessment

Partially fixed. There is real progress, but the branch still has duplicated semantic ownership.

Fixed:

- `mutate.rs` is a true facade.
- `state.rs` is within the documented facade cap.
- `workflow/operator.rs` imports query/route-plan/read DTOs rather than mutation/write helpers.
- Normal command modules do not directly write projections.
- `stale_target_selection.rs` is a useful shared truth helper.

Remaining issues:

- `public_route_selection.rs` still performs route selection while `route_plan.rs` owns final route ordering.
- `execution_reentry_target_source` has two owners: route follow-up selection and status projection.
- `closure_dispatch.rs` uses bootstrap-only stale target selection outside a bootstrap-only lane.
- Public repair target reason-code taxonomy is duplicated as raw strings.
- Boundary tests enforce placement but currently bless some split decisions.

Assessment: partially fixed; not structurally final.

## Reviewer recursion assessment

Fixed. Reviewer recursion prevention is prompt-text only and reviewer-prompt scoped. No runtime or env-based recursion guard was found. Reviewer prompts prohibit launching additional subagents.

## Signal-to-noise assessment

The runtime changes remain valuable, but the branch is at the edge where more static enforcement can become self-referential churn.

High-signal work to keep:

- Typed public argv/template authority.
- Diagnostic-only artifact/provenance freshness after closure.
- Idempotent public task closure replay.
- Public-flow tests that use the compiled binary.
- Focused module-boundary tests that prevent write/read/mutation import violations.

Low-signal or overgrown surfaces to reduce:

- Duplicate active-doc scanners in Rust when Node prompt/docs tests own that surface.
- Large synthetic scanner fixture sections embedded in broad contract tests.
- Full-object route goldens that fail on incidental JSON shape changes.
- Repeated long-form route-law prose in every generated skill.
- Loose prompt budget headroom after compaction.

## Validation results

Audit iteration started with `cargo clean`, which removed 109729 files and 15.7 GiB of build output.

Passed:

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`: 132 passed
- `/usr/bin/time -p cargo clippy --all-targets --all-features -- -D warnings`: passed, real 38.37s after clean
- `/usr/bin/time -p cargo nextest run --test runtime_authority_contracts`: 7 passed, real 22.99s
- `/usr/bin/time -p cargo nextest run --test workflow_runtime`: 90 passed, real 13.01s
- `/usr/bin/time -p cargo nextest run --test workflow_shell_smoke`: 106 passed, real 27.31s
- `/usr/bin/time -p cargo nextest run --test workflow_entry_shell_smoke`: 13 passed, real 22.20s
- `/usr/bin/time -p cargo nextest run --test plan_execution`: 45 passed, real 6.34s
- `/usr/bin/time -p cargo nextest run --test plan_execution_final_review`: 29 passed, real 4.16s
- `/usr/bin/time -p cargo nextest run --test workflow_runtime_final_review`: 2 passed, real 3.68s
- `/usr/bin/time -p cargo nextest run --test contracts_execution_runtime_boundaries`: 30 passed, real 4.91s
- `/usr/bin/time -p cargo nextest run --test execution_query`: 12 passed, real 3.55s
- `/usr/bin/time -p cargo test --test liveness_model_checker -- --nocapture`: 29 passed, real 75.94s

The last full no-fail-fast nextest run before this audit rereview passed 1674 tests in 157.266s, under the 4-5 minute performance threshold.

## Prioritized findings

### Blocker

None.

### High

1. **Diagnostic `runtime_reconcile_required` still authorizes `repair-review-state`.**
   - Class: user-facing dead end / churn loop.
   - References: `src/execution/command_eligibility.rs::decide_public_mutation`, `src/execution/route_plan/constructors.rs`, `src/execution/route_plan/status_projection.rs`, `tests/public_replay_churn.rs`.
   - Impact: public surfaces expose no legal command, but guessed repair command can enter mutation path.

2. **Public route seed still selects routes instead of only preparing route inputs.**
   - Class: architecture / split decisioning.
   - References: `src/execution/public_route_selection.rs::shared_next_action_seed_from_precomputed_decision`, `src/execution/route_plan.rs::route_decision_from_runtime_state`, `docs/runtime-architecture.md`.
   - Impact: two modules answer what the next public action should be.

3. **Execution reentry target source has two owners.**
   - Class: architecture / split decisioning.
   - References: `src/execution/route_plan/follow_up.rs::execution_reentry_target_source_for_route`, `src/execution/route_plan/status_projection.rs::execution_reentry_target_source_for_status_projection`.
   - Impact: projection can revise selected-route metadata.

### Medium

4. **Pre-reducer stale target selection leaks into non-bootstrap dispatch.**
   - Class: architecture / split decisioning.
   - References: `src/execution/status_support.rs::pre_reducer_earliest_unresolved_stale_task`, `src/execution/closure_dispatch.rs::review_dispatch_task_boundary_target`, `src/execution/query.rs`.

5. **Public repair target reason-code taxonomy is duplicated as raw strings.**
   - Class: architecture / semantic drift.
   - References: `src/execution/public_repair_targets.rs`, `src/execution/route_plan/status_projection.rs`.

6. **Active generated skills still mention removed workflow helper names.**
   - Class: documentation / prompt contamination.
   - References: `skills/brainstorming/SKILL.md.tmpl`, `skills/writing-plans/SKILL.md.tmpl`, `skills/plan-ceo-review/SKILL.md.tmpl`, generated `SKILL.md` files, `tests/codex-runtime/skill-doc-contracts.test.mjs`.

7. **Signal-to-noise: duplicate active-doc scanners and scanner self-tests.**
   - Class: test maintainability / meta-churn.
   - References: `tests/public_cli_flow_contracts.rs`, `tests/runtime_module_boundaries.rs`, `tests/codex-runtime/skill-doc-contracts.test.mjs`.

8. **Runtime route golden is too broad.**
   - Class: test signal-to-noise.
   - References: `tests/runtime_behavior_golden.rs`, `tests/fixtures/runtime-goldens/public-runtime-routes.json`.

### Low

9. **`workflow-handoff` can recommend the wrong skill for fidelity-blocked approved plans.**
   - Class: agent UX / legacy handoff risk.
   - References: `src/workflow/operator.rs`, `src/workflow/status.rs`, `schemas/workflow-handoff.schema.json`.

10. **Doctor dashboard external-review rerun text is imprecise.**
    - Class: agent UX.
    - Reference: `src/workflow/doctor_dashboard.rs`.

11. **Some test failure labels still say "recommended command" while executing typed argv.**
    - Class: test message cleanup.
    - References: `tests/workflow_shell_smoke.rs`, `tests/public_replay_churn.rs`.

## Checklist

### Public CLI / reachability

- Public `begin` can seed preflight: fixed.
- No normal flow needs `plan execution preflight`: fixed.
- No normal flow needs `record-review-dispatch`: fixed.
- No normal flow needs `gate-review`: fixed.
- No normal flow needs `gate-finish`: fixed.
- No normal flow needs `rebuild-evidence`: fixed.
- No normal flow needs low-level late-stage recorders: fixed.
- Operator never recommends hidden/debug commands: fixed.
- Status never exposes hidden/debug commands as next actions: fixed.
- Public recommended argv is executable by shipped CLI: fixed.

### Plan review

- Plan-fidelity no longer uses hidden runtime receipt recording: fixed.
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: fixed.
- Engineering-review edits do not bounce back to fidelity early: fixed.
- Final engineering-approved handoff requires current five-surface fidelity: fixed with low handoff caveat.
- Active docs do not teach plan-fidelity receipt recording: fixed.
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed.

### Execution runtime

- Current task closure is begin-time authority: fixed.
- Current closure cannot appear in stale closures: fixed.
- Close-current-task can refresh current dispatch internally: fixed.
- Stale dispatch does not block public close: fixed.
- Receipt/projection diagnostics do not trigger reentry: fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not treated as authoritative unless exact command is begin for same task/step: fixed.
- Repair-review-state cannot loop on same route: partially fixed; targetless reconcile guessed repair remains.
- Runtime reconcile handles targetless stale states: partially fixed; public route is diagnostic, mutation guard still too permissive.

### Evidence/projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: fixed.
- Supersession is append-only and does not rewrite proof: fixed.
- Evidence is audit/projection, not control plane: fixed.

### Tests

- Public-flow tests do not call internal helpers: fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: fixed.
- Replay tests cover historical dead ends: fixed for known cases, add targetless reconcile guessed repair replay.
- Liveness model catches repeated route signatures: fixed.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.

### Prompt surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: fixed.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.
- No active prompt hidden-helper names: partially fixed; bare removed helper names remain.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules have cohesive responsibilities: partially fixed.
- No new catch-all module replaces old monoliths: fixed.
- Phase/reason strings are centralized: partially fixed; public repair target reasons remain raw/duplicated.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: partially fixed.
- Import-boundary tests exist: fixed, but they currently bless some split decisions.

## Recommendation

Ship only after targeted fixes. The next implementation should prioritize deleting duplicate decisioning and reducing prompt/test surface, not adding more policy layers around existing drift.
