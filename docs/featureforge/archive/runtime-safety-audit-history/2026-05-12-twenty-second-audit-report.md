# FeatureForge Runtime Safety Audit - Twenty-Second Pass

Date: 2026-05-12

## Executive Verdict

**Recommendation: do not ship yet; ship only after targeted runtime split-decisioning and signal-to-noise fixes.**

The updated branch is substantially safer for normal agent use than the earlier failure history. The audit found no currently demonstrated public CLI dead end, no hidden command required for normal flow, no receipt/projection artifact acting as normal workflow truth, and no stale-closure loop reproduced through public routes.

The branch is still not a ship candidate because the remaining issues are exactly the kind that tend to recreate FeatureForge maintenance churn: one public-route validation path can recompute route targets after finalized status projection, the new route-choice module has become a large catch-all, boundary tests pin private/comment shape, and prompt/release validation is adding brittle meta-law instead of reducing conceptual surface area.

## What Is Genuinely Fixed

- Public normal workflow is reachable through shipped commands. `begin`, `close-current-task`, `advance-late-stage`, `complete`, `reopen`, `transfer`, and status/operator surfaces are exposed through `src/cli/plan_execution.rs` and dispatched in `src/lib.rs`.
- Public `begin` owns preflight/run identity setup through `src/execution/commands/begin.rs`.
- Public `close-current-task` owns dispatch refresh and closure recording in `src/execution/commands/close_current_task.rs`.
- Public `advance-late-stage` owns branch closure, final-review dispatch, release readiness, final review, QA, and finish progression in `src/execution/commands/advance_late_stage.rs`.
- Typed public command authority is the executable contract. `recommended_public_command_argv` and `recommended_public_command_template` are constructed from typed command models in `src/execution/command_eligibility.rs` and projected through route/status/operator surfaces.
- Current task closure is task-boundary authority. Missing dispatch, receipt, summary, and projection artifacts stay diagnostic once a current positive closure exists.
- Evidence and projections are explicit read/materialization surfaces, not normal progress gates.
- Plan fidelity is parseable artifact based and does not depend on hidden runtime receipt recording.
- Engineering-review edits stay in engineering review until explicit final-fidelity refresh.
- Public-flow tests are mostly realistic: shipped-runtime claims use compiled CLI helpers, and internal semantic helpers are quarantined by scanner contracts.
- Reviewer recursion prevention remains prompt text scoped to reviewer prompts; no runtime/env recursion guard was found.
- Prompt budget enforcement exists and currently passes.

## What Remains Risky

1. `status_assembly/task_state.rs::require_public_execution_command_route_target` can fall back to `route_plan/public_commands.rs::require_execution_command_route_target`, which recomputes `compute_next_action_decision` when finalized status projection did not carry `execution_command_context` and `recommended_command`.
2. `src/execution/route_plan/next_action_choice.rs` is 2,318 lines, and `compute_next_action_decision_with_authority_inputs` still spans the main route-choice decision tree. This is now the route-choice monolith.
3. `tests/runtime_module_boundaries.rs` pins private helper names, exact imports, and comment markers such as `ordered pass #1`/`ordered pass #5`, creating refactor friction without proving public behavior.
4. `src/execution/state/preflight.rs` emits `authoritative_mutation_recovery_required` with remediation text that sounds like manual authoritative-state repair.
5. `scripts/verify-source-archive.mjs` omits `skills/skill-doc-budgets.json` and `tests/codex-runtime/skill-doc-budget.test.mjs`, even though `docs/testing.md` treats prompt budgets as release-critical.
6. `tests/fixtures/runtime-remediation/README.md` has stale public replay coverage bookkeeping for several `FS-*` scenarios.
7. Reviewer recursion law and route/prompt law are duplicated across generated prompts/tests instead of being centralized around one reference or prelude.
8. `tests/codex-runtime/skill-doc-budget.test.mjs` guards release prose and historical wording, making budget enforcement noisier than necessary.
9. `docs/testing.md` mixes mandatory release gates with manual audit aids, making it read like a second release workflow.

## Concrete Dead Ends Still Possible

No concrete public runtime dead end was reproduced.

The audit did not find a normal path requiring:

- `plan execution preflight`
- `record-review-dispatch`
- `gate-review`
- `gate-finish`
- `rebuild-evidence`
- low-level late-stage recorders
- hidden/debug/compatibility helpers

However, there are still agent-facing traps:

- `authoritative_mutation_recovery_required` tells agents to "Recover interrupted authoritative mutation state" without a typed public route or explicit diagnostic stop.
- exact execution-command validation can recompute a route target after status projection failed to carry the target, hiding a projection/route finalization gap until a later surface diverges.
- source archive verification can pass without the prompt-budget manifest/test assets, which can make packaged release validation drift from repository validation.

## Concrete Churn Sources Still Possible

- `next_action_choice.rs` is large enough to become the new route-law dumping ground.
- Boundary tests in `tests/runtime_module_boundaries.rs` protect private strings and comments rather than durable ownership contracts.
- Prompt budget tests currently include release-note prose checks, so normal release writing can churn prompt-budget test failures.
- Reviewer prompt recursion language is repeated in `skills/subagent-driven-development/spec-reviewer-prompt.md` both outside and inside the embedded dispatch payload.
- The runtime-remediation fixture coverage map can mislead future audits by underreporting public replay coverage that now exists in `tests/public_replay_churn.rs`.
- `docs/testing.md` asks maintainers to perform broad manual scans as if they are release gates, increasing cargo-cult risk.

## Public/Private Test Mismatch Assessment

**Mostly fixed, with low-severity documentation drift.**

No public-flow test was found calling internal runtime helpers for shipped-runtime proof. `tests/public_flow_scan_contracts.rs`, `tests/public_cli_flow_contracts.rs`, and `tests/support/public_flow_scan.rs` guard public-flow files against hidden helper calls, hidden commands, display-command execution, and unregistered synthetic event APIs.

The mismatch that remains is inventory drift, not runtime proof drift: `tests/fixtures/runtime-remediation/README.md` underreports the public replay coverage now present in `tests/public_replay_churn.rs` for `FS-11` through `FS-15` and additional churn scenarios.

## Receipt/Evidence/Projection Control-Plane Assessment

**Fixed for normal workflow.**

The audit found current task closure functioning as the task-boundary authority. Missing/stale dispatch, unit-review, task-verification, summary, receipt, and projection artifacts stay diagnostic after authoritative pass/pass closure exists. Projection exports are explicit materialization, not normal command progress.

No actionable receipt/provenance/evidence control-plane issue was found.

## Prompt-Surface And Packaging Assessment

**Partially fixed.**

Generated skills are fresh, prompt budgets pass, companion references resolve, reviewer recursion prevention is prompt-scoped, and active guidance points agents at typed operator/status JSON rather than display-command parsing.

Remaining issues are signal/noise and packaging:

- source archive verification does not require the budget manifest/test assets
- reviewer recursion rule text is duplicated in spec reviewer prompt surfaces
- budget tests pin release prose instead of only enforcing the budget contract
- `docs/testing.md` blurs mandatory release gates and manual audit aids

## Modularization And Split-Decisioning Assessment

**Close but not done.**

Good progress:

- `next_action.rs` is a facade, and route choice moved under `src/execution/route_plan/`.
- `PublicCommand` and route decision projection are typed.
- `state.rs` and `mutate.rs` are no longer the primary route-decision monoliths.
- Import-boundary tests exist and pass.

Remaining issues:

- `route_plan/next_action_choice.rs` is the new monolith.
- exact execution-command validation still has a recompute fallback through `route_plan/public_commands.rs`.
- module-boundary tests pin implementation shape instead of only durable ownership boundaries.

## Reviewer Recursion Assessment

**Functionally fixed, but duplicated.**

No runtime/env recursion enforcement was found, and reviewer prompts prohibit spawning additional subagents. The issue is duplication: the same reviewer recursion rule appears multiple times in reviewer prompt surfaces, including both surrounding guidance and embedded dispatch payload text.

## Validation Results

The audit iteration started with `cargo clean` after a process check. The clean removed 176,785 files and 21.2 GiB.

- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/*.test.mjs`: passed, 133/133.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed after clean.
- Before the grouped `nextest` shard, process precheck found no active `cargo nextest`, `cargo-nextest`, `nextest run`, or `/target/debug/deps/` process.
- Grouped audit nextest shard passed:
  - Command: `cargo nextest run --test runtime_authority_contracts --test workflow_runtime --test workflow_shell_smoke --test workflow_entry_shell_smoke --test plan_execution --test plan_execution_final_review --test workflow_runtime_final_review --test contracts_execution_runtime_boundaries --test execution_query --no-fail-fast --status-level fail --final-status-level slow`
  - Result: 331/331 passed.
  - Time: real 51.66s.
- `cargo test --test liveness_model_checker`: passed, 32/32, finished in 23.52s.
- `cargo fmt --check`: passed.

No validation command crossed the 4-5 minute clean-rerun threshold or the 10 minute immediate remediation threshold.

## Prioritized Findings

### Blocker

None.

### High

1. **Exact execution-command validation still recomputes route authority after projection.**
   - Classification: architecture issue / split-decisioning issue.
   - References:
     - `src/execution/status_assembly/task_state.rs::require_public_execution_command_route_target`
     - `src/execution/route_plan/public_commands.rs::require_execution_command_route_target`
     - `src/execution/route_plan/public_commands.rs::execution_command_route_target_from_status_context`
     - `src/execution/route_plan/next_action_choice.rs::compute_next_action_decision`
     - `src/execution/read_model/public_route_projection.rs::apply_public_route_projection`
     - `docs/runtime-architecture.md`
   - Impact: a status whose finalized route projection failed to carry the exact command can still pass validation because a second candidate computation proves a target exists.

2. **`route_plan/next_action_choice.rs` is the new route-choice monolith.**
   - Classification: architecture issue / signal-to-noise issue.
   - References:
     - `src/execution/route_plan/next_action_choice.rs`
     - `src/execution/route_plan/next_action_choice.rs::compute_next_action_decision_with_authority_inputs`
     - `docs/featureforge/reference/execution-runtime-module-boundaries.md`
     - `tests/runtime_module_boundaries.rs`
   - Impact: route decisioning is centralized in name but not yet modular in practice; future route changes will be hard to review and easy to churn.

3. **Module-boundary tests pin private/comment implementation shape.**
   - Classification: test realism / maintainability issue.
   - References:
     - `tests/runtime_module_boundaries.rs::public_route_decision_rules_have_focused_module_owners`
     - private helper/import assertions near `repair_reentry_route_semantics_use_shared_decision_object`
   - Impact: tests discourage useful refactors and can cause noise without proving shipped runtime behavior.

### Medium

4. **Preflight authoritative mutation recovery remediation is not actionable through public runtime routes.**
   - Classification: public-output / agent-UX issue.
   - References:
     - `src/execution/state/preflight.rs`
     - `src/execution/status.rs::GateDiagnostic`
     - `src/workflow/operator.rs::WorkflowDoctorOutput`
     - `tests/internal_plan_execution.rs`
   - Impact: wording can push agents toward manual runtime artifact repair rather than diagnostic stop and typed route re-query.

5. **Prompt-budget release-critical assets are omitted from source archive verification.**
   - Classification: packaging / validation issue.
   - References:
     - `scripts/verify-source-archive.mjs::REQUIRED_SOURCE_ARCHIVE_PATHS`
     - `skills/skill-doc-budgets.json`
     - `tests/codex-runtime/skill-doc-budget.test.mjs`
     - `docs/testing.md`
   - Impact: a source/archive validation pass can miss the budget gate even though release docs require it.

6. **Reviewer recursion prompt law is duplicated.**
   - Classification: prompt-surface / signal-to-noise issue.
   - References:
     - `skills/subagent-driven-development/spec-reviewer-prompt.md`
     - reviewer prompt contract checks in `tests/codex-runtime/skill-doc-contracts.test.mjs`
   - Impact: the rule is correct, but duplicated text increases prompt bulk and maintenance cost.

7. **Prompt-budget tests pin release prose instead of only budget contract behavior.**
   - Classification: test signal/noise issue.
   - References:
     - `tests/codex-runtime/skill-doc-budget.test.mjs`
     - `RELEASE-NOTES.md`
     - `docs/testing.md`
   - Impact: useful budget enforcement is coupled to incidental release-note wording.

### Low

8. **Runtime-remediation fixture coverage map is stale.**
   - Classification: documentation/test inventory issue.
   - References:
     - `tests/fixtures/runtime-remediation/README.md`
     - `tests/public_replay_churn.rs`
   - Impact: future audits can undercount public replay coverage and re-open solved public/private mismatch concerns.

9. **`docs/testing.md` reads as a second meta-release workflow.**
   - Classification: documentation / signal-to-noise issue.
   - References:
     - `docs/testing.md`
   - Impact: mandatory gates and manual audit aids are mixed together, making release validation harder to follow.

## Specific Failure-Class Checklist

### Public CLI / Reachability

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

### Plan Review

- Plan-fidelity no longer uses hidden runtime receipt recording: fixed.
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: fixed.
- Engineering-review edits do not bounce back to fidelity early: fixed.
- Final engineering-approved handoff requires current five-surface fidelity: fixed.
- Active docs do not teach plan-fidelity receipt recording: fixed.
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed.

### Execution Runtime

- Current task closure is begin-time authority: fixed.
- Current closure cannot appear in stale closures: fixed.
- Close-current-task can refresh current dispatch internally: fixed.
- Stale dispatch does not block public close: fixed.
- Receipt/projection diagnostics do not trigger reentry: fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not treated as authoritative unless exact command is begin for same task/step: fixed.
- Repair-review-state cannot loop on same route: fixed.
- Runtime reconcile handles targetless stale states: fixed.
- Exact execution-command route validation consumes finalized route projection only: partially fixed.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: fixed.
- Supersession is append-only and does not rewrite proof: fixed.
- Evidence is audit/projection, not control plane: fixed.

### Tests

- Public-flow tests do not call internal helpers: fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: fixed.
- Replay tests cover historical dead ends: fixed, with stale coverage-map documentation.
- Liveness model catches repeated route signatures: fixed.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.
- Boundary tests avoid private/comment shape pins: still broken.

### Prompt Surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: partially fixed because source archive verification omits budget assets.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.
- Reviewer recursion and route law are de-duplicated: partially fixed.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules have cohesive responsibilities: partially fixed.
- No new catch-all module replaces the old monoliths: still broken for `route_plan/next_action_choice.rs`.
- Phase/reason strings are centralized: partially fixed.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: partially fixed.
- Import-boundary tests exist: fixed, but too brittle.

## Recommendation

**Ship only after targeted fixes.**

The remaining fixes should delete or centralize complexity, not add another layer of scanners. The next remediation should:

1. make exact execution-command validation consume finalized route projection only
2. split `next_action_choice.rs` into cohesive route-family modules and cap child-route module growth
3. replace private/comment boundary pins with ownership/behavior tests
4. make preflight recovery remediation diagnostic-only and public-route oriented
5. add prompt-budget assets to source archive verification
6. centralize reviewer recursion prompt prelude and remove duplicated prose
7. remove release-prose checks from the prompt-budget test
8. update stale public replay coverage docs and split mandatory versus manual testing guidance
