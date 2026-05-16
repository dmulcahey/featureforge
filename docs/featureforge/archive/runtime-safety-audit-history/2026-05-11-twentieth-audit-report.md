# FeatureForge Twentieth Runtime Safety Audit

## Executive Verdict

**Ship only after targeted fixes.**

The current branch is materially safer than the earlier failure modes: public CLI reachability, typed public argv/template authority, plan-fidelity artifact routing, projection materialization, and reviewer recursion controls are now broadly in place and validated. The remaining issues are not mostly old hidden-helper leaks. They are conceptual-surface and split-decisioning risks: route-plan still relies on a legacy `next_action` decision adapter, one route-plan status-projection path can still revise the selected route, liveness/replay loop detection can miss fingerprint-only route churn, and a few prompt/docs surfaces still teach unnecessary workflow bounce or vague manual fallback language.

## What Is Genuinely Fixed

- Public CLI normal workflow is reachable through shipped commands. `begin`, `close-current-task`, and `advance-late-stage` own the expected aggregate transitions, and the removed/hidden helper commands are not exposed as normal-flow CLI commands.
- Operator/status executable authority is typed. Public route execution is through `recommended_public_command_argv` or `recommended_public_command_template`; `recommended_command` is display-only compatibility text.
- Current task closure is the task-boundary authority for the inspected post-closure paths. Missing/stale dispatch lineage, summaries, projection hashes, and task-verification projections do not appear able to force execution reentry after a valid positive current closure.
- Plan-fidelity no longer depends on hidden runtime receipts. It is parseable artifact based, uses the five-surface fidelity contract, and the implementation handoff is gated on current pass state.
- Prompt packaging is much healthier. Generated skills are fresh, the budget is enforced at 5,050 total lines, route-owning skills keep mandatory route law top-level, and companion references are packaged.
- Reviewer recursion prevention is prompt-scoped and reviewer-prompt scoped. No runtime/env recursion enforcement was found.

## What Remains Risky

- Route decisioning is still split between `route_plan` and legacy `next_action`. `RouteDecision` is the presentation authority, but meaningful route choices still arrive through `NextActionDecision` and are adapted in `route_plan/next_action_route.rs`.
- `route_plan/status_projection.rs` still contains route-affecting logic for targetless stale baseline repair. That duplicates `select_runtime_route_decision` and contradicts the architecture statement that status projection must not revise selected routes.
- Public replay and liveness route-signature checks include volatile argv/fingerprint material, so an equivalent route that refreshes only `--expect-execution-fingerprint` can evade repeated-route detection.
- The liveness model has a blind spot for the generic “earlier stale boundary plus later stale/interrupted overlay” combination. Concrete FS-15 integration tests cover one shape, but the model checker does not encode the two-boundary state directly.
- Some docs still cause workflow churn: `writing-plans` tells agents to return to `writing-plans` when engineering review finds issues, while `plan-eng-review` now owns Draft edits; review accelerator docs say “manual review” without naming the owning review flow.

## Concrete Dead Ends Still Possible

- **Fingerprint-churn loop not detected by tests:** A route can stay semantically identical while the rendered argv or execution fingerprint changes, and current repeated-route tests can treat that as progress.
- **Status-projection route rewrite:** Targetless stale baseline repair can be selected in `select_runtime_route_decision` and again in status projection, leaving two owners for the same repair decision.
- **Prompt workflow bounce:** Agents following `writing-plans` can bounce back to planning for engineering-review fixes instead of letting `plan-eng-review` update the Draft plan and continue to fidelity.

## Concrete Churn Sources Still Possible

- Broad prose scanners in `tests/codex-runtime/skill-doc-contracts.test.mjs` can force wording churn across docs/prompts instead of behavior fixes.
- Public-flow scanner coverage is manually listed; important public-looking files such as `plan_execution_final_review.rs` are not covered the same way as core public-flow suites.
- Boundary tests still risk enforcing private helper shape rather than durable module ownership, although several previous brittle pins have been removed.

## Public/Private Test Mismatch Assessment

**Partially fixed.** Public-flow shell smoke and golden tests are much stronger and use compiled CLI where the shell boundary is the contract. Internal helpers are quarantined and marked. However, the liveness matrix is explicitly internal semantic coverage, not shipped-runtime proof, and should not be counted as public runtime behavior beyond its sampled CLI parity edge.

## Receipt/Evidence/Projection Control-Plane Assessment

**Mostly fixed for task closure and projection freshness.** Evidence/projection outputs are mostly derived and explicit materialization is separate from progress. Current positive task closure suppresses stale summary/dispatch/projection reentry paths.

**Residual policy risk:** active-contract serial unit-review proof still validates an authoritative `unit-review-*.md` artifact. The code describes this as runtime-owned active contract truth, not plain receipt fallback, so this audit treats it as policy-dependent rather than a confirmed bug. If the target policy is “no receipt-named markdown artifact ever gates routing,” this remains partially fixed.

## Prompt-Surface And Packaging Assessment

**Fixed with minor polish needed.** Budget enforcement, generated-doc freshness, companion reference packaging, mandatory route law placement, hidden-helper scanners, and reviewer recursion prompts all pass. Remaining issues are wording and signal-to-noise: `writing-plans` has stale engineering-review handoff text, and review accelerator fallback language should name the owning review flow.

## Modularization And Split-Decisioning Assessment

**Partially fixed.** `state.rs`/`mutate.rs` are thinner, route-plan modules are cohesive, workflow operator consumes finalized route state, and import-boundary tests exist. The remaining core risk is route decision dual ownership:

- `src/execution/route_plan.rs` calls `route_decision_from_shared_next_action_candidate`.
- `src/execution/route_plan/next_action_route.rs` adapts `NextActionDecision` into `RouteDecision`.
- `src/execution/next_action.rs` still owns substantial semantic route choices.
- `src/execution/route_plan/status_projection.rs` still revises targetless stale baseline repair.

## Reviewer Recursion Assessment

**Fixed.** Recursion prevention is prompt text only, scoped to reviewer prompts and generated reviewer agent surfaces. Tests assert runtime/env recursion guards are absent and reviewer prompts prohibit launching additional subagents.

## Validation Results

- `node scripts/gen-skill-docs.mjs --check`: passed, real 0.17s.
- `node scripts/gen-agent-docs.mjs --check`: passed, real 0.05s.
- `node --test tests/codex-runtime/*.test.mjs`: passed, 133/133, real 0.84s.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed, real 59.61s after clean rebuild.
- Pre-nextest process check: no active `cargo nextest`, `cargo-nextest`, `nextest run`, or `target/debug/deps/` process found.
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`: passed, 1642/1642, real 269.28s. This covers the listed integration tests including `runtime_authority_contracts`, `workflow_runtime`, `workflow_shell_smoke`, `workflow_entry_shell_smoke`, `plan_execution`, `plan_execution_final_review`, `workflow_runtime_final_review`, `contracts_execution_runtime_boundaries`, and `execution_query`.
- `cargo test --test liveness_model_checker`: passed, 29/29, real 28.04s.
- `git diff --check`: passed.

## Prioritized Findings

### Blocker

None confirmed in the current checkout.

### High

1. **Route decisioning still has two authorities.**
   Type: architecture / split decisioning.
   Refs: `src/execution/route_plan.rs::select_runtime_route_decision`, `src/execution/route_plan/next_action_route.rs::route_decision_from_shared_next_action_candidate`, `src/execution/next_action.rs::compute_next_action_decision_with_authority_inputs`.
   Impact: agents can still encounter route/status divergence if `next_action` and `route_plan` evolve differently.

### Medium

1. **Targetless stale baseline repair can be decided twice.**
   Type: architecture / churn.
   Refs: `src/execution/route_plan.rs::select_runtime_route_decision`, `src/execution/route_plan/status_projection.rs::finalize_route_decision_for_status_projection`, `docs/runtime-architecture.md`.
   Impact: status projection remains route-affecting when it should only project finalized route state.

2. **Repeated-route detection can miss fingerprint-only churn.**
   Type: test realism / liveness.
   Refs: `tests/public_replay_churn.rs`, `tests/liveness_model_checker.rs`.
   Impact: an agent loop with semantically identical route commands but refreshed fingerprints can evade current loop detection.

3. **Public-flow protected-file set is manual and incomplete.**
   Type: test realism.
   Refs: `tests/support/public_flow_scan.rs::is_protected_public_flow_file`, `tests/plan_execution_final_review.rs`, `tests/liveness_model_checker.rs`.
   Impact: future public-looking tests can use direct helpers without the same scanner coverage.

4. **`writing-plans` teaches an outdated engineering-review bounce.**
   Type: documentation / agent UX.
   Refs: `skills/writing-plans/SKILL.md.tmpl`, `skills/writing-plans/SKILL.md`, `skills/plan-eng-review/SKILL.md.tmpl`.
   Impact: unnecessary planning churn after engineering review finds fixable plan issues.

### Low

1. **Liveness model does not encode the generic two-boundary stale overlay.**
   Type: test coverage.
   Refs: `tests/liveness_model_checker.rs`.
   Impact: concrete FS-15 is covered, but the model checker can accept progress to a later stale target without proving the earlier stale target was consumed.

2. **Review accelerator fallback wording is vague.**
   Type: documentation / agent UX.
   Refs: `review/review-accelerator-packet-contract.md`.
   Impact: “manual review” is less actionable than naming the owning public review flow.

3. **Module-boundary reference doc is stale around command construction.**
   Type: documentation.
   Refs: `docs/featureforge/reference/execution-runtime-module-boundaries.md`.
   Impact: docs still say public-command construction remains in `next_action.rs`.

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
- Engineering-review edits do not bounce back to fidelity early: fixed in runtime, partially fixed in docs.
- Final engineering-approved handoff requires current five-surface fidelity: fixed.
- Active docs do not teach plan-fidelity receipt recording: fixed.
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed.

### Execution Runtime

- Current task closure is begin-time authority: fixed.
- Current closure cannot appear in stale closures: fixed for task-boundary closure; branch drift remains intentionally staleable.
- Close-current-task can refresh current dispatch internally: fixed.
- Stale dispatch does not block public close: fixed.
- Receipt/projection diagnostics do not trigger reentry: mostly fixed; active-contract serial unit-review proof remains policy-dependent.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not authoritative unless exact command is begin for same task/step: fixed.
- Repair-review-state cannot loop on same route: fixed by current tests, with liveness blind spot noted.
- Runtime reconcile handles targetless stale states: fixed.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: fixed for inspected normal paths.
- Supersession is append-only and does not rewrite proof: fixed.
- Evidence is audit/projection, not control plane: mostly fixed, active serial review artifact remains policy-dependent.

### Tests

- Public-flow tests do not call internal helpers: mostly fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: partially fixed.
- Replay tests cover historical dead ends: fixed.
- Liveness model catches repeated route signatures: partially fixed.
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

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules have cohesive responsibilities: mostly fixed.
- No new catch-all module replaces old monoliths: fixed.
- Phase/reason strings are centralized: partially fixed.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: partially fixed.
- Import-boundary tests exist: fixed, but some are noisy.

## Recommendation

**Do not ship yet.** Implement the targeted twentieth-remediation plan: remove route-affecting status projection, harden semantic loop detection, make public-flow test boundaries clearer, and clean stale prompt/docs wording. Re-audit after those fixes with the same A-I audit shape.
