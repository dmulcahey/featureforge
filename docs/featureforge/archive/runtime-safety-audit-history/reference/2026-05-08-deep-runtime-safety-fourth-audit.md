# FeatureForge Deep Runtime Safety Fourth Audit

## Executive Verdict

**Recommendation:** do not ship yet.

The updated codebase is materially safer on public CLI reachability, public-output wording, prompt surface, plan-review flow, test realism, and stale-closure convergence. The fourth audit still found two actionable structural issues:

1. Execution evidence and legacy plain unit-review receipts can still affect review-gate control-plane outcomes.
2. Public route selection and router ownership still have split decisioning, including a router/public-route import cycle and duplicated branch-closure refresh predicates.

These are not broad regressions, but they are directly in the failure classes this remediation series is meant to eliminate.

## What Is Genuinely Fixed

- Public CLI reachability is clean. Normal flow is reachable through `plan execution begin`, `repair-review-state`, `close-current-task`, `advance-late-stage`, `reopen`, `transfer`, and read-only `workflow status|doctor|operator`.
- Public output no longer recommends hidden repair helpers or compound "repair then reenter" display text.
- `blocked_runtime_bug` is diagnostic-only in public routing and operator output.
- Public-flow tests use compiled CLI helpers and quarantine direct internal helpers.
- Plan-fidelity uses parseable review artifacts, not hidden runtime receipt recording.
- Engineering-review edits stay in engineering review until the explicit final fidelity pass.
- Prompt budget enforcement and generated skill/agent freshness are in place.
- Reviewer recursion prevention is prompt-text scoped.
- Stale closure, cycle-break, targetless stale, and resume routing converge in the inspected paths.

## What Remains Risky

- `gate_review_base_result` still lets missing/stale execution evidence attempts block final review after authoritative completion state exists.
- The legacy plain unit-review receipt fallback still lets current-run receipt artifacts block review gates in contractless/no-active-lease scenarios.
- `router.rs` imports `public_route_selection.rs`, while `public_route_selection.rs` calls back into `router.rs` for shared next-action decisions.
- Branch-closure refresh routing is still expressed in more than one module, including a local predicate in `public_route_selection.rs` duplicating current-truth logic.

## Concrete Dead Ends Still Possible

- A branch with authoritative completed-step or task-closure state can fail final review because `context.evidence` has no completed attempt for a completed step.
- A stale or corrupted legacy plain unit-review receipt can force `repair-review-state` remediation even when no active contract path/fingerprint exists and runtime-owned closure state is otherwise authoritative.

## Concrete Churn Sources Still Possible

- Projection-only execution evidence churn can still change final-review gate outcomes.
- Receipt-only strategy/provenance drift in legacy plain unit-review artifacts can still survive projection rebuilds and keep review gates blocked.
- Router/public-route circular ownership can let future branch-closure routing changes land in one surface but not the other.

## Public/Private Test Mismatch Assessment

No actionable mismatch found. Public-flow tests are guarded by compiled-CLI helpers and static scans. Historical public replay/golden tests cover the major stuck paths through public commands.

## Receipt/Evidence/Projection Control-Plane Assessment

Partially fixed. Task-boundary closure, dispatch refresh, projections, and late-stage docs largely respect runtime-owned state. Execution evidence and legacy plain unit-review receipts are still control-plane inputs for review gates and must be demoted to diagnostic/read-model status when authoritative completion or closure state exists.

## Prompt Surface And Packaging Assessment

Clean. Budgets are enforced, generated docs are fresh, companion references are packaged, mandatory law remains top-level, and reviewer recursion prevention is prompt-only.

## Modularization And Split-Decisioning Assessment

Partially fixed. Module sizes and many boundaries are improved, but route ownership is not one-way yet. The import cycle between `router.rs` and `public_route_selection.rs` and duplicated branch-closure refresh logic are actionable split-decisioning risks.

## Reviewer Recursion Assessment

Clean. Reviewer recursion prevention is scoped to reviewer prompt text and generated reviewer surfaces; no runtime or env recursion guard was introduced.

## Validation Results

Passed in the current tree after the second remediation plan:

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`: 125 passed
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`: 1619 passed
- `cargo nextest run --test runtime_authority_contracts --test workflow_runtime --test workflow_shell_smoke --test workflow_entry_shell_smoke --test plan_execution --test plan_execution_final_review --test workflow_runtime_final_review --test contracts_execution_runtime_boundaries --test execution_query --no-fail-fast`: 329 passed
- `cargo test --test liveness_model_checker`: 28 passed
- `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .`

Subagents also ran targeted checks inside their scopes; none reported validation failures.

## Prioritized Findings

### High: Execution evidence still acts as final-review control-plane truth

**Type:** control-plane leak, user-facing dead-end.

`src/execution/state/review_gate.rs::gate_review_base_result` derives completed steps from authoritative state, then fails final review if `context.evidence` lacks a completed attempt for those steps. Evidence is documented as a projection/read model, so missing/stale evidence must not block final review when authoritative completion or current closure state is sufficient.

References:

- `src/execution/state/review_gate.rs::gate_review_base_result`
- `src/execution/state/review_gate.rs::authoritative_completed_steps_for_gate`
- `tests/internal_plan_execution.rs::internal_only_compatibility_gate_review_rejects_checked_step_without_execution_evidence`

### Medium: Legacy plain unit-review receipts still participate in review-gate truth

**Type:** receipt control-plane leak.

`src/execution/state/worktree_lease_truth.rs::enforce_worktree_lease_binding_truth` falls back to `enforce_plain_unit_review_truth` when no active contract path/fingerprint is present. `src/execution/state/unit_review_truth.rs::enforce_plain_unit_review_truth` scans current-run unit-review receipt artifacts and can fail the gate for unreadable, malformed, or provenance-mismatched receipts.

References:

- `src/execution/state/worktree_lease_truth.rs::enforce_worktree_lease_binding_truth`
- `src/execution/state/worktree_lease_truth.rs::worktree_or_unit_review_binding_artifacts_exist`
- `src/execution/state/unit_review_truth.rs::enforce_plain_unit_review_truth`
- `tests/internal_plan_execution.rs::internal_only_compatibility_rebuild_evidence_noop_preserves_receipt_only_strategy_checkpoint_drift`

### High: Router/public-route decisioning has a circular dependency

**Type:** architecture issue, split decisioning.

`src/execution/router.rs` imports `shared_next_action_seed_from_runtime_state` from `public_route_selection.rs`, while `public_route_selection.rs` calls `router::shared_next_action_decision*`. This keeps route-decision ownership cyclic instead of one-way.

References:

- `src/execution/router.rs`
- `src/execution/public_route_selection.rs::shared_next_action_seed_from_decision`
- `src/execution/public_route_selection.rs::shared_next_action_seed_from_runtime_state`
- `tests/runtime_module_boundaries.rs::public_route_decision_rules_have_focused_module_owners`

### High: Branch-closure refresh predicate is duplicated

**Type:** architecture issue, split decisioning.

`current_truth::branch_closure_refresh_missing_current_closure` exists and is consumed by read-model code, but `public_route_selection.rs::stale_branch_closure_refresh_required` locally re-expresses the same branch-closure refresh decision shape.

References:

- `src/execution/current_truth.rs::branch_closure_refresh_missing_current_closure`
- `src/execution/public_route_selection.rs::stale_branch_closure_refresh_required`
- `src/execution/read_model.rs`

### Medium: Branch-closure route selection remains spread across modules

**Type:** architecture issue.

`next_action.rs`, `public_route_selection.rs`, and `router.rs` still each contain branch-closure recording or late-stage advance route logic. The immediate fix is to centralize the branch-refresh predicate and break the cycle; the remaining branch-closure route cases should be represented by shared helpers and boundary tests so future drift is caught.

## Checklist

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
- Receipt/projection diagnostics do not trigger reentry: partially fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not authoritative unless exact command is begin for same task/step: fixed.
- Repair-review-state cannot loop on same route: fixed.
- Runtime reconcile handles targetless stale states: fixed.

### Evidence/Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: fixed.
- Supersession is append-only and does not rewrite proof: fixed.
- Evidence is audit/projection, not control plane: partially fixed.

### Tests

- Public-flow tests do not call internal helpers: fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: fixed.
- Replay tests cover historical dead ends: fixed.
- Liveness model catches repeated route signatures: fixed.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.

### Prompt Surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: fixed.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: partially fixed.
- New modules have cohesive responsibilities: partially fixed.
- No new catch-all module replaces the old monoliths: fixed.
- Phase/reason strings are centralized: fixed.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: partially fixed.
- Import-boundary tests exist: partially fixed.
