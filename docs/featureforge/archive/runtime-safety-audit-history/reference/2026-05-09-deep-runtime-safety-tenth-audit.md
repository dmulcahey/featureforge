# FeatureForge Deep Runtime Safety Tenth Audit

## Executive Verdict

Ship only after targeted fixes.

The updated runtime is materially safer than the prior audit pass. Public CLI reachability, public-flow test realism, receipt/evidence control-plane authority, plan-fidelity workflow, stale/reentry liveness, prompt budget enforcement, and reviewer recursion controls all passed focused clean-context audit slices.

The branch is not done because route ownership is still semantically split between `route_plan` and `router`, route-to-status projection still has duplicated mapping logic, and a few public-output/prompt strings can still point agents toward helper-shaped or retired-command concepts.

## What Is Genuinely Fixed

- Public commands are sufficient for normal runtime flow: `begin`, `close-current-task`, `advance-late-stage`, `repair-review-state`, `transfer`, and typed workflow/operator routes cover the inspected lanes.
- Public-flow tests are guarded against internal helper dependence and replay historical stuck paths through compiled public CLI boundaries.
- Authoritative runtime state, not evidence/projection markdown, drives closure truth and routing.
- Current task closure authority, stale-target selection, targetless reconcile, cycle-break, and reentry liveness converge in the inspected paths.
- Plan-fidelity is parseable artifact based and no longer depends on hidden runtime receipts.
- Prompt-surface budgets are enforced; generated skills/agents are fresh; reviewer recursion prevention is prompt-text scoped.
- Blocked runtime diagnostics no longer expose normal mutation commands in the inspected public route surfaces.

## What Remains Risky

1. `route_plan` owns route ordering but still imports route decision types and route-constructor helpers from `router`. This leaves `router` as more than projection.
2. `PlanExecutionStatus` route projection is duplicated in `router` and `read_model/public_route_projection`, including phase-to-harness mapping and route field assignment.
3. Dirty-before-begin recovery guidance still uses "helper-backed route" and "authoritative helper mutations" wording in active execution skills.
4. Handoff follow-up output still says "record a handoff" while the public executable route is `plan execution transfer`.
5. Some `blocked_runtime_bug` failure messages are diagnostic-only but do not explicitly instruct callers to stop and report the diagnostic.

## Concrete Dead Ends Still Possible

- An agent reading dirty-before-begin recovery wording can go looking for helper mutation/backfill paths instead of returning to workflow/operator typed argv/template authority.
- An agent seeing `record_handoff` as a follow-up token plus "record a handoff" prose can infer a retired command shape instead of the public `transfer` route.
- Future route changes can drift because route constructors remain in `router`, while route ordering lives in `route_plan`.

## Concrete Churn Sources Still Possible

- Duplicated route-to-status projection can drift when a new route field, phase, harness phase, blocker field, or diagnostic field is added.
- Router-held route constructor helpers can accumulate semantic checks that bypass route-plan ownership.
- Diagnostic messages that omit stop/report language can cause agents to retry local artifact repair even when no public route exists.

## Public/Private Test Mismatch Assessment

Clean. Subagent B found no public-flow tests calling private helpers. Static tests and public replay coverage passed:

- `cargo test --test public_cli_flow_contracts -- --nocapture`: 61/61.
- `cargo test --test public_replay_churn -- --nocapture`: 33/33.
- `cargo test --test runtime_authority_contracts -- --nocapture`: 7/7.
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`: 58/58.

## Receipt/Evidence/Projection Control-Plane Assessment

Clean. Subagent C found no active control-plane leakage. Evidence/projection materialization remains projection-only, current closure and reducer-derived runtime truth drive route selection, and stale/missing receipt/projection artifacts are diagnostic-only in inspected paths.

## Prompt-Surface And Packaging Assessment

Mostly clean. Budgets, generated docs, companion refs, and recursion controls passed. The remaining prompt problem is narrow wording in dirty-before-begin recovery instructions, not budget or packaging.

## Modularization And Split-Decisioning Assessment

Partially fixed. Route ordering is improved and route-plan status finalization owns several previously split decisions. Remaining P2 issues:

- `src/execution/route_plan.rs` imports `RouteDecision`, `PublicRouteDecision`, and route constructors from `src/execution/router.rs`.
- `src/execution/router.rs` still owns `close_current_task_route_decision`, `repair_review_state_route_decision`, `runtime_reconcile_route_decision`, and `branch_closure_recording_route_decision`.
- `src/execution/router.rs` and `src/execution/read_model/public_route_projection.rs` duplicate route-to-status projection.

## Reviewer Recursion Assessment

Clean. Recursion prevention is prompt-text only and reviewer-prompt scoped. No runtime/env recursion enforcement was found.

## Validation Results

Pre-audit implementation gate:

- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/*.test.mjs`: 129/129 passed.
- `cargo fmt --check`: passed.
- `cargo test --test runtime_module_boundaries -- --nocapture`: 60/60 passed.
- `cargo test --test plan_execution stale_close_current_task_follow_up_hash_cannot_bridge_execution_reentry_route -- --nocapture`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`: run ID `483b5352-a03b-4e5f-b0ba-d0f8ed297715`, 1652/1652 passed.
- `cargo test --test liveness_model_checker -- --nocapture`: 28/28 passed.

Additional audit-slice validation:

- A: public CLI flow/shell-smoke targeted tests passed.
- B: public flow contracts, public replay, runtime authority, and codex skill-doc contracts passed.
- C: runtime authority and projection-only targeted tests passed.
- D: plan-review contract/schema/workflow targeted tests passed.
- E: liveness/public replay/stale/current-overlap targeted tests passed.
- F: generated skill docs, generated agent docs, codex-runtime tests, and source archive verification passed.
- G: `cargo test --test runtime_module_boundaries -- --nocapture` passed.
- H: public diagnostics and instruction-contract targeted tests passed.

## Prioritized Findings

### Blocker

None.

### High

None.

### Medium

1. **Route ownership remains split between route-plan and router.**
   Category: architecture issue.
   References: `src/execution/route_plan.rs`, `src/execution/router.rs::close_current_task_route_decision`, `repair_review_state_route_decision`, `runtime_reconcile_route_decision`, `branch_closure_recording_route_decision`.
   Required fix: move route decision type/constructors/helpers under `route_plan` or a lower route-decision module consumed by route-plan; make router import them for projection only.

2. **Route-to-status projection is duplicated.**
   Category: architecture/test-drift issue.
   References: `src/execution/router.rs::project_route_decision_for_status_blocker_authority`, `src/execution/read_model/public_route_projection.rs::project_routing_decision_onto_status`.
   Required fix: extract shared route-status projection helpers for phase-to-harness mapping and common route fields; leave only pre/final deltas local.

3. **Dirty-before-begin guidance uses helper-shaped wording.**
   Category: documentation/agent-UX issue.
   References: `skills/executing-plans/SKILL.md.tmpl`, `skills/subagent-driven-development/SKILL.md.tmpl`, generated `SKILL.md` files.
   Required fix: replace "helper-backed route" and "authoritative helper mutations" with workflow/operator typed argv/template and stop-on-diagnostic wording.

4. **Handoff follow-up wording can imply retired command shape.**
   Category: public-output issue.
   References: `src/execution/review_state.rs`, `src/execution/command_eligibility.rs`, generated schemas.
   Required fix: public prose must direct to public `transfer`; schema/template text must identify `record_handoff` as a follow-up intent token, not a command name.

### Low

1. **Some blocked runtime bug messages need explicit stop/report text.**
   Category: public-output issue.
   References: `src/execution/event_log.rs`, `src/execution/migration.rs`.
   Required fix: append diagnostic stop/report wording while preserving no-mutation semantics.

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
- Receipt/projection diagnostics do not trigger reentry: fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not treated as authoritative unless exact command is begin for same task/step: fixed.
- Repair-review-state cannot loop on same route: fixed.
- Runtime reconcile handles targetless stale states: fixed.

### Evidence/Projection

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

### Prompt Surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: fixed.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.
- Prompt wording avoids helper-shaped recovery language: partially fixed.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules have cohesive responsibilities: partially fixed.
- No new catch-all module replaces the old monoliths: fixed.
- Phase/reason strings are centralized: fixed.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: partially fixed.
- Import-boundary tests exist: fixed.

## Recommendation

Do not ship yet. Ship only after the targeted route-decision ownership, route-to-status projection, and public-output wording fixes in `docs/featureforge/plans/2026-05-09-runtime-route-decision-and-output-tenth-audit-remediation.md`.
