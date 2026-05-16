# FeatureForge Runtime Safety Audit - Twenty-First Pass

Date: 2026-05-11

## Executive Verdict

**Recommendation: do not ship yet; ship only after targeted split-decisioning and signal-to-noise fixes.**

The branch is no longer structurally unsafe for normal public runtime use. The prior dead-end classes around hidden commands, receipt/projection control-plane leakage, stale closure loops, and public/private test mismatch are currently covered by public CLI tests, liveness tests, scanner contracts, and prompt budget checks.

The remaining actionable issues are architecture and maintenance risk, not an observed user-facing dead end. They matter because they preserve the same class of future failure mode: more than one surface still participates in route decisioning, and some tests/prompts still encode implementation shape instead of one canonical route authority.

## What Is Genuinely Fixed

- Public normal workflow is reachable through shipped CLI commands. `begin`, `close-current-task`, `advance-late-stage`, `complete`, `reopen`, `transfer`, and status are shipped public subcommands in `src/cli/plan_execution.rs`, and dispatch through `src/lib.rs`.
- Public `begin` owns preflight setup. `src/execution/commands/begin.rs` persists public preflight state before opening a step.
- Public `close-current-task` owns dispatch refresh and closure recording. It refreshes missing lineage under `EventCommandOwner::PublicCloseCurrentTask` and records current closure through event-backed recording.
- Public `advance-late-stage` owns normal late-stage progression. Final review, finish gates, branch closure, release readiness, and QA are reachable without low-level public recorders.
- Typed public command authority is established. Runtime and schemas treat `recommended_public_command_argv` and `recommended_public_command_template` as executable surfaces while `recommended_command` is display-only.
- Current task closure is task-boundary authority. Missing or stale task review/verification artifacts become diagnostics after authoritative closure exists rather than mutation authority.
- Evidence and projections are read models or explicit materializations, not normal progress gates.
- Plan fidelity is artifact based, parseable, and no longer dependent on hidden receipt recording.
- Engineering review owns Draft plan edits before final plan-fidelity refresh.
- Public-flow tests use compiled CLI surfaces when they claim shipped-runtime behavior; internal semantic helpers are quarantined and scanner-documented.
- Reviewer recursion prevention is prompt-text scoped to reviewer prompts, not runtime/env enforcement.
- Prompt budgets are active and enforced: current generated skill total is 5,008/5,050.

## What Remains Risky

1. Route choice still has multiple decision points. `next_action`, `route_plan`, router status projection, and status assembly each still participate in deciding or reshaping the route.
2. `advance_late_stage` still has local readiness checks based on operator phase/detail/review strings instead of relying entirely on centralized mutation eligibility and route decisions.
3. `status_assembly` still derives route-adjacent facts such as repair follow-ups, stale projections, harness phase changes, blocking task, and review-state status.
4. Some tests still pin private helper names and encode current module shape, which will cause churn during the next real refactor.
5. Generated route-owning skills still duplicate detailed route JSON law even though a canonical reference exists.

## Concrete Dead Ends Still Possible

No concrete public dead end was found in this audit.

The public CLI and liveness lanes found no path where:

- a normal flow requires `plan execution preflight`, `record-review-dispatch`, `gate-review`, `gate-finish`, `rebuild-evidence`, or low-level late-stage recorders
- successful `close-current-task` routes back to the same task without real stale/negative state
- targetless stale states mutate through guessed repair commands
- `blocked_runtime_bug` offers a normal mutation command
- `resume_task` or `resume_step` override the exact legal public command

## Concrete Churn Sources Still Possible

- `src/execution/router.rs::project_final_runtime_routing_projection` performs route planning, status projection, blocker computation, then calls `select_route_decision_with_status_projection_authority`, so route choice can still be revised after status shape changes.
- `src/execution/next_action.rs::compute_next_action_decision_with_authority_inputs` and `src/execution/route_plan/next_action_route.rs::route_decision_from_shared_next_action_candidate` both decide parts of "what should happen next."
- `src/execution/status_assembly.rs::derive_public_phase_detail` and `derive_public_next_action` are test-only route-law derivations consumed by read-model tests.
- `tests/runtime_module_boundaries.rs` pins private helper names and repeated shape assertions rather than only durable import direction and ownership contracts.
- `scripts/gen-skill-docs.mjs::buildInstalledControlPlaneSection` repeats detailed route binding law in every route-owning skill instead of delegating most detail to `references/operator-route-authority.md`.

## Public/Private Test Mismatch Assessment

**Fixed for current public-flow claims.**

Public runtime proof uses the compiled binary through `tests/support/public_featureforge_cli.rs`. Public replay and route-golden tests call compiled CLI helpers, not direct runtime helpers. `tests/support/public_flow_scan.rs` protects public-flow files from internal support imports, hidden helper calls, hidden commands/flags, display-command execution, and unregistered synthetic event-log APIs.

`tests/liveness_model_checker.rs` is explicitly internal semantic coverage and is not cited as shipped-runtime proof. `tests/plan_execution_final_review.rs` is scanner-protected but documented as mixed final-review contract coverage, not part of `scripts/run-public-runtime-flow-tests.sh`.

## Receipt/Evidence/Projection Control-Plane Assessment

**Fixed for normal workflow.**

Task closure readiness uses current authoritative closure state. Missing or malformed review/verification artifacts after authoritative closure feed diagnostic reason codes only. Projection materialization is explicit and reports `runtime_truth_changed: false`. Normal commands reject tracked projection export mode unless explicitly materializing projections.

Active-contract serial unit-review proof still gates pre-closure contract truth. That is an intended runtime contract boundary, not legacy receipt leakage.

## Prompt-Surface And Packaging Assessment

**Mostly fixed, with one signal-to-noise issue.**

Generated docs are fresh, budgeted, and packaged references resolve. Mandatory route law remains top-level for route-owning skills. Reviewer recursion prevention remains prompt-text only. Hidden-helper and display-command scanners pass.

The remaining issue is duplication: route-owning generated skills still embed detailed operator JSON execution law instead of keeping only compact terminal law plus a link to the canonical route reference.

## Modularization And Split-Decisioning Assessment

**Close but not done.**

Good progress:

- `stale_target_selection` centralizes stale target ordering.
- `route_plan/public_commands.rs` centralizes most public command construction.
- `workflow/operator` mostly presents finalized route decisions.
- `state.rs` and `mutate.rs` are thinner than the old monoliths.

Remaining issues:

- `next_action` still contains the main route-ordering tree.
- `route_plan/next_action_route` still reinterprets and overrides next-action decisions.
- router still performs a post-status-projection route revision.
- `status_assembly` still owns route-adjacent semantic decisions.
- import boundaries remain porous through `state.rs`.

## Reviewer Recursion Assessment

**Fixed.**

Reviewer recursion prevention is prompt-scoped to reviewer prompts and generated reviewer agent surfaces. There is no runtime/env recursion enforcement path. Tests reject env/runtime guard variants and require reviewer prompts to prohibit additional subagents.

## Validation Results

All commands below were run after `cargo clean` for the audit iteration.

- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/*.test.mjs`: passed, 133/133.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed, real 51.33s.
- Grouped audit nextest shard:
  - Command: `cargo nextest run --all-features --no-fail-fast --status-level fail --final-status-level slow --test runtime_authority_contracts --test workflow_runtime --test workflow_shell_smoke --test workflow_entry_shell_smoke --test plan_execution --test plan_execution_final_review --test workflow_runtime_final_review --test contracts_execution_runtime_boundaries --test execution_query`
  - Result: passed, 331/331, real 119.35s.
- `cargo test --test liveness_model_checker`: passed, 32/32, real 30.74s.

Before the grouped nextest shard, an active-process precheck for `cargo nextest`, `cargo-nextest`, `nextest run`, and `target/debug/deps/` test binaries returned no processes.

## Prioritized Findings

### Blocker

None.

### High

1. **Route ownership remains split between `next_action` and `route_plan`.**
   - Classification: architecture issue.
   - References:
     - `src/execution/next_action.rs::compute_next_action_decision_with_authority_inputs`
     - `src/execution/route_plan/next_action_route.rs::route_decision_from_shared_next_action_candidate`
     - `tests/runtime_module_boundaries.rs::public_route_decision_rules_have_focused_module_owners`
   - Impact: future route changes can diverge because `next_action` selects semantic candidates while `route_plan` reinterprets and overrides them into public `RouteDecision`.

2. **`advance_late_stage` still duplicates public readiness/eligibility logic.**
   - Classification: architecture issue.
   - References:
     - `src/execution/commands/advance_late_stage.rs::record_branch_closure_for_command`
     - `src/execution/command_eligibility.rs::check_public_mutation_allowed`
     - `src/execution/commands/common/mutation_guards.rs`
   - Impact: late-stage mutations can drift from route/eligibility authority because the command locally interprets operator phase/detail/review strings.

### Medium

3. **Router still performs post-status-projection route revision.**
   - Classification: architecture issue.
   - References:
     - `src/execution/router.rs::project_final_runtime_routing_projection`
     - `src/execution/route_plan.rs::select_route_decision_with_status_projection_authority`
     - `src/execution/route_plan/status_projection.rs::finalize_route_decision_for_status_projection`
   - Impact: route choice remains dependent on projected status shape instead of one precomputed route-fact input set.

4. **`status_assembly` still owns route-adjacent semantic decisions.**
   - Classification: architecture issue.
   - References:
     - `src/execution/status_assembly.rs::populate_public_status_contract_fields`
     - `src/execution/status_assembly.rs::derive_status_review_state_status`
     - `src/execution/route_plan/route_facts.rs::effective_route_review_state_status`
   - Impact: review-state and stale/repair semantics can drift between status assembly and route planning.

5. **Generated route law is duplicated across route-owning skills.**
   - Classification: signal-to-noise / documentation issue.
   - References:
     - `references/operator-route-authority.md`
     - `scripts/gen-skill-docs.mjs::buildInstalledControlPlaneSection`
     - `tests/codex-runtime/skill-doc-contracts.test.mjs`
   - Impact: prompt law remains more spread out than necessary, increasing maintenance cost and agent reading burden.

### Low

6. **Test-only route derivation duplicates public route semantics.**
   - Classification: test maintenance issue.
   - References:
     - `src/execution/status_assembly.rs::derive_public_phase_detail`
     - `src/execution/status_assembly.rs::derive_public_next_action`
     - `src/execution/read_model.rs` tests consuming those functions.
   - Impact: tests can pass against a parallel route derivation instead of the route-plan/operator path.

7. **Module-boundary tests pin private helper names and duplicate shape checks.**
   - Classification: test maintenance issue.
   - References:
     - `tests/runtime_module_boundaries.rs::public_route_decision_rules_have_focused_module_owners`
     - route/status-projection boundary assertions near the status-projection ownership tests.
   - Impact: implementation refactors remain noisy even when public behavior and ownership boundaries are preserved.

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
- Replay tests cover historical dead ends: fixed.
- Liveness model catches repeated route signatures: fixed.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.
- Tests avoid duplicating route derivation: partially fixed.
- Module-boundary tests avoid private helper pins: partially fixed.

### Prompt Surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: fixed.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.
- Route law is centralized enough: partially fixed.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: partially fixed.
- New modules have cohesive responsibilities: partially fixed.
- No new catch-all module replaces the old monoliths: partially fixed.
- Phase/reason strings are centralized: fixed.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: partially fixed.
- Import-boundary tests exist: fixed.
- Import-boundary tests enforce semantic ownership rather than private shape: partially fixed.
