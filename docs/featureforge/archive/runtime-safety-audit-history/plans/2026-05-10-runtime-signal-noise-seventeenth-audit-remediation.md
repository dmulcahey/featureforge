# Workflow State

Engineering Approved

# Plan Revision

1

# Execution Mode

Execute tasks in order. After each task, run strict Clippy and the full no-fail-fast nextest suite before clean-context review. Do not use FeatureForge runtime skills or project skills. Review subagents must not spawn additional subagents.

# Goal

Reduce the remaining FeatureForge conceptual surface area found by the seventeenth audit without weakening the public runtime safety gains already landed.

The runtime already has no known public CLI dead end, legacy proof/projection control-plane leak, stale closure loop, or reviewer recursion issue in this audit. The remaining actionable issue is signal-to-noise: public route truth still flows through too many intermediate shapes, status assembly still computes route-like fields before route projection overwrites them, and several tests/prompts pin implementation shape or repeated prose instead of durable external behavior.

# Architecture

The target architecture is:

1. `next_action.rs` is a candidate/intent helper only. It can classify likely user intent and return `NextActionDecision`, but it must not own final public route fields, display command strings, blocker projection, or status/operator route shape.
2. `route_plan` owns final public route decisions. A `RouteDecision` is the complete public route envelope used by router, status projection, operator output, and route goldens.
3. `router` projects a finalized `RouteDecision`; it does not recompute blocking scope/task, external wait state, or command surfaces from parallel state when the finalized decision already owns them.
4. `status_assembly` builds runtime facts and diagnostics from event-log authority. It does not act as a second route selector. Any route-like facts it must derive are packaged in a small immutable facts object consumed by route planning/status projection.
5. Tests protect public behavior, import boundaries, and ownership boundaries. They should not require exact function names, body order, prose sentences, or scanner internals unless those are the external contract under test.
6. Skills carry compact top-level law: installed runtime, typed operator argv/template authority, stop on absent executable route/diagnostic route, and a link to canonical route law. Detailed binding law lives in `references/operator-route-authority.md`.

# Change Surface

Runtime:

- `src/execution/next_action.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/**`
- `src/execution/query.rs`
- `src/execution/router.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/status_assembly.rs`
- `src/execution/status_assembly/**` if a new fact module is added

Tests:

- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/rust_source_scan_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/fixtures/runtime-goldens/public-runtime-routes.json`
- targeted runtime behavior tests as needed

Prompts/docs/generated artifacts:

- `scripts/gen-skill-docs.mjs`
- `skills/**/SKILL.md.tmpl`
- generated `skills/**/SKILL.md`
- `references/operator-route-authority.md`
- `docs/testing.md`
- `docs/runtime-architecture.md`
- `skills/skill-doc-budgets.json`

# Preconditions

- Plan #2 has passed its whole-plan validation and clean-context review.
- The seventeenth audit completed with no actionable findings from public CLI/reachability, test realism, control-plane, plan-review, reentry-loop, prompt-packaging, modularization, or agent-UX auditors.
- The signal-to-noise auditor found actionable issues in route/status layering and test/prompt over-pinning.
- Validation baseline after the audit:
  - `node scripts/gen-skill-docs.mjs --check`: pass
  - `node scripts/gen-agent-docs.mjs --check`: pass
  - `node --test tests/codex-runtime/*.test.mjs`: pass, 133/133
  - `git diff --check`: pass
  - `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .`: pass
  - `cargo clippy --all-targets --all-features -- -D warnings`: pass
  - `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`: pass, 1629/1629
  - Clean performance confirmation: `cargo clean && cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`: pass, real 193.56s

# Known Footguns / Constraints

- Do not reintroduce hidden/debug commands, legacy proof-token mechanics, display-command execution, or manual artifact repair as normal workflow guidance.
- Do not weaken `recommended_public_command_argv` exact machine-invocation authority or template binding authority.
- Do not remove public route goldens or compiled-CLI public-flow smoke coverage.
- Do not replace source-shape pins with weaker coverage; replace them with public behavior checks or import/ownership boundaries that fail for the historical bugs.
- Do not bury mandatory runtime law solely in companion docs. The top-level skill law must stay short but present.
- Do not add new static scanners to compensate for duplicated runtime decisioning. First reduce duplicated runtime decisioning.
- Do not run FeatureForge runtime skills or project skills.
- Do not allow review subagents to spawn subagents.
- Before every follow-up audit iteration, run full `cargo clean`.
- If a full test suite run exceeds the 4-5 minute threshold, run `cargo clean` and rerun. If it still exceeds the threshold, stop and address performance before continuing normal implementation.

# Requirement Coverage Matrix

| Requirement | Task |
| --- | --- |
| Public route authority is not split across `next_action`, `next_action_seed`, and `route_plan` | Task 1 |
| `RouteDecision` is the complete public route envelope consumed by router/status/operator | Task 1 |
| Status assembly no longer acts as a second route selector | Task 2 |
| Source-shape tests are reduced to durable ownership or behavior coverage | Task 3 |
| Scanner tests stay useful without becoming an architecture language | Task 3 |
| Skill route law remains compact and canonical-reference backed | Task 4 |
| Skill-doc tests assert invariants instead of exact prose | Task 4 |
| Full validation and clean-context review gates remain mandatory | All tasks |

# Tasks

## Task 1: Collapse next-action seed routing into route-plan authority

### Spec Coverage

- High finding: public route authority split across `next_action.rs`, `route_plan/next_action_seed.rs`, and `route_plan.rs`.
- Architecture items 1, 2, and 3.

### Goal

Make `route_plan` the only owner of final public route fields for runtime routes. `next_action.rs` must remain a candidate/intent helper; no intermediate `WorkflowRoutingDecision`/seed DTO should carry final phase, review state, command, blocker, or recording surfaces between candidate selection and route planning.

### Context

Current flow:

- `next_action.rs` returns `NextActionDecision` with phase, phase detail, review state, blockers, and a typed command candidate.
- `route_plan/next_action_seed.rs` rewrites those fields into `WorkflowRoutingDecision`.
- `route_plan.rs` then derives command surfaces, blockers, follow-up, reentry target, state kind, and final route output again.

This is not currently a public dead end, but it is still split decisioning. The fix should remove the seed layer as a public-route shape and have `route_plan` build a finalized `RouteDecision` directly from the candidate decision and route facts.

### Constraints

- Preserve public JSON shape unless the change is strictly needed to remove split authority.
- Do not remove `NextActionDecision` if it is still useful as an intent/candidate object.
- Do not let `next_action.rs` regain display string or final status/operator route authority.
- Keep route decision command surfaces typed and generated through `PublicRouteDecision::command_surfaces`.
- Keep exact-command gating for begin/resume/reopen/close-current-task paths.

### Done when

- `src/execution/query.rs` no longer defines or exports `WorkflowRoutingDecision`.
- `route_plan/next_action_seed.rs` is deleted or replaced by a route-plan-owned module that returns `RouteDecision`, not a seed DTO.
- `route_facts.rs` and `final_review_dispatch.rs` consume `RouteDecision` or narrow route-plan-local facts, not `WorkflowRoutingDecision`.
- `route_plan.rs` owns all final command surface binding, blocking reason derivation, follow-up derivation, reentry target source derivation, recording context, execution command context, and state-kind synthesis for runtime routes.
- Boundary tests assert that `next_action.rs` does not construct `RouteDecision`, display command strings, blockers, or status/operator projection fields.
- Runtime behavior/golden tests still pass without meaningful public route changes unless a justified contract simplification is explicitly reflected in goldens.

### Files

- Modify: `src/execution/query.rs`
- Modify: `src/execution/route_plan.rs`
- Modify/Delete: `src/execution/route_plan/next_action_seed.rs`
- Modify: `src/execution/route_plan/route_facts.rs`
- Modify: `src/execution/route_plan/final_review_dispatch.rs`
- Modify: `src/execution/router.rs`
- Modify: `src/execution/route_plan/decision.rs`
- Modify: `src/execution/route_plan/status_application.rs`
- Modify: `tests/runtime_module_boundaries.rs`
- Modify targeted runtime tests/goldens only if route output changes intentionally.

### Implementation Steps

1. Introduce a route-plan-local helper that accepts `RuntimeState`, `NextActionDecision`, and route inputs, and returns either:
   - a finalized `RouteDecision`, or
   - `None` when exact-command requirements cannot be satisfied.
2. Move all mutation of candidate fields currently in `shared_next_action_seed_from_precomputed_decision` into route-plan finalization. The helper should use route-plan functions to:
   - promote missing-current-closure reentry into `close-current-task` when baseline bridge rules allow it;
   - bind repair-review-state command for missing-current-closure repair routes;
   - bind begin/resume/reopen/close-current-task execution command contexts only after exact-command validation;
   - bind task/final/release recording contexts;
   - promote branch closure refresh when task closure recording would otherwise be stale;
   - derive display, argv, template, and required inputs exactly once through `PublicRouteDecision::command_surfaces`.
3. Delete `WorkflowRoutingDecision` from `query.rs`.
4. Replace `route_facts` helpers that accept seed DTOs with helpers that accept explicit route fields or `RouteDecision`.
5. Replace `final_review_dispatch_route_for_repaired_late_stage_drift` seed input with a route-plan-local candidate/final decision input.
6. Add `blocking_scope`, `blocking_task`, and `external_wait_state` to `PublicRouteDecision` if needed so `RouteDecision` is the complete public route envelope.
7. Update `router` and status projection to consume finalized route decision fields instead of recomputing those fields from parallel `ExecutionRoutingState` values.
8. Update boundary tests to verify ownership by imports and behavior, not exact helper names.
9. Run targeted tests, then full validation.

### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test runtime_authority_contracts -- --nocapture`
- `cargo test --test runtime_behavior_golden -- --nocapture`
- `cargo test --test execution_query -- --nocapture`
- `cargo test --test workflow_runtime -- --nocapture`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Clean-context review of Task 1 before proceeding.

## Task 2: Separate status facts from route decisions

### Spec Coverage

- High finding: `status_assembly.rs` derives route-like truth before route projection reapplies final route truth.
- Architecture item 4.

### Goal

Make status assembly produce authoritative runtime facts and diagnostics, not final route decisions. Route projection should be the only layer that turns those facts into public route phase/detail/action/command/blocker fields.

### Context

`populate_public_status_contract_fields` currently computes repair reroutes, stale projection, harness phase changes, review-state status, blocking records, then calls `clear_route_projection_fields`. Some of that is legitimate status truth, but some is route-selection logic in practice. The route projection then applies a finalized `RouteDecision`.

### Constraints

- Do not remove status JSON fields that are public read-model diagnostics.
- Do not make route planning read markdown/projection artifacts as authority.
- Do not duplicate route decision logic in a new status helper.
- Keep event-log/reducer authority intact.

### Done when

- Route projection fields are only populated by route projection, not by status assembly.
- Status assembly exposes any route-relevant intermediate state as an immutable facts object with a name that makes its non-authoritative status clear.
- `derive_public_review_state_status` is either moved/renamed to a status-facts helper or replaced with route-plan-owned review-state derivation so it is not a hidden route selector.
- `clear_route_projection_fields` is removed, narrowed to a defensive assertion/reset with documentation, or called only at an explicit route-projection boundary.
- Tests cover that status assembly facts and route decisions converge without status assembly recomputing command/action routes.

### Files

- Modify: `src/execution/status_assembly.rs`
- Add/Modify: `src/execution/status_assembly/**` or `src/execution/route_plan/**`
- Modify: `src/execution/router.rs`
- Modify: `src/execution/read_model/public_route_projection.rs`
- Modify: `tests/runtime_module_boundaries.rs`
- Modify/add targeted runtime tests if needed.

### Implementation Steps

1. Identify each value in `populate_public_status_contract_fields` that is true status fact versus route projection.
2. Extract status-only facts into a cohesive struct, for example `StatusRoutingFacts` or `StatusReviewFacts`, with fields such as stale projection, repair follow-up classification, branch reroute validity, and route-neutral review-state diagnostics.
3. Make route planning consume these facts through `RuntimeState` or a route-plan input instead of recomputing from status-shaped side effects.
4. Ensure public route fields remain empty until `apply_shared_routing_projection_to_read_scope_with_routing`.
5. Replace `clear_route_projection_fields` with a narrower boundary helper if a reset is still needed for legacy status loading.
6. Add a boundary test that prevents status assembly from setting display-command text, `next_action`, or execution/recording route contexts outside the route projection boundary.
7. Preserve behavior with public route goldens and workflow runtime tests.

### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test runtime_behavior_golden -- --nocapture`
- `cargo test --test workflow_runtime -- --nocapture`
- `cargo test --test execution_query -- --nocapture`
- `cargo test --test liveness_model_checker -- --nocapture`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Clean-context review of Task 2 before proceeding.

## Task 3: Replace brittle source-shape pins with behavior and boundary coverage

### Spec Coverage

- Medium finding: tests pin exact source shape instead of externally visible behavior.
- Medium finding: scanner tests drift toward tests around tests.
- Architecture item 5.

### Goal

Keep the tests that catch real regressions while deleting or relaxing brittle exact helper-name/body-order/prose pins.

### Context

The current test suite has valuable public-flow and boundary coverage, but some tests assert exact private helper names, struct names, function bodies, or scanner internals. Those tests create churn when code is refactored in the right direction.

### Constraints

- Keep compiled public CLI coverage.
- Keep public/internal test quarantine.
- Keep import-boundary tests that prevent workflow/operator/mutation/read-model boundary violations.
- Keep behavior goldens for externally visible route JSON.
- Do not delete tests merely because they are static; delete or relax only tests that pin incidental implementation shape.

### Done when

- `tests/runtime_module_boundaries.rs` no longer requires exact private helper-name lists unless the exact name is a public boundary owner.
- `tests/public_cli_flow_contracts.rs` asserts public behavior, public JSON schema semantics, and compiled CLI behavior rather than internal struct/body ordering.
- `tests/public_flow_scan_contracts.rs` and `tests/rust_source_scan_contracts.rs` cover scanner fixture behavior only where the scanner protects public/internal boundaries; they are not used as a broad architecture specification language.
- Any removed source-shape assertion is replaced by behavior, import-boundary, or route-golden coverage where the underlying risk is real.
- `docs/testing.md` and `docs/runtime-architecture.md` describe the new boundary coverage accurately.

### Files

- Modify: `tests/runtime_module_boundaries.rs`
- Modify: `tests/public_cli_flow_contracts.rs`
- Modify: `tests/public_flow_scan_contracts.rs`
- Modify: `tests/rust_source_scan_contracts.rs`
- Modify: `tests/support/rust_source_scan.rs` only if needed
- Modify: `docs/testing.md`
- Modify: `docs/runtime-architecture.md`

### Implementation Steps

1. Review source-shape assertions in runtime boundary and public CLI tests.
2. Categorize each assertion:
   - keep: import boundary, public behavior, ownership boundary tied to historical failure;
   - relax: exact helper name/body-order check that can be replaced by dependency/path ownership;
   - delete: duplicate scanner-internal self-test that no longer protects a public failure.
3. Replace exact helper-name pins with module-boundary assertions over imports, owner modules, public behavior, or route goldens.
4. Keep scanner contract tests small and fixture-oriented.
5. Update docs to describe public-flow, runtime-boundary, scanner-contract, and route-golden coverage by purpose.

### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo test --test public_flow_scan_contracts -- --nocapture`
- `cargo test --test rust_source_scan_contracts -- --nocapture`
- `cargo test --test runtime_behavior_golden -- --nocapture`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Clean-context review of Task 3 before proceeding.

## Task 4: Compact skill route law and relax prose pins

### Spec Coverage

- Medium finding: skill docs repeat route law despite a canonical reference.
- Medium finding: skill-doc contract tests over-pin prose.
- Architecture item 6.

### Goal

Keep mandatory route law visible and actionable while making generated skills shorter and less repetitive. Tests should assert route-law invariants, not exact sentences.

### Context

`scripts/gen-skill-docs.mjs` injects installed control-plane and route authority prose into many skills. `using-featureforge` adds another long route-law block. `skill-doc-contracts.test.mjs` exact-matches route bullets and reference prose. This protects real historical failures, but the current wording is near saturation.

### Constraints

- Do not move mandatory law solely into companion references.
- Keep top-level skills explicit that:
  - live workflow routing uses the installed runtime;
  - agents must use typed public argv/template fields;
  - display command text remains display-only compatibility text;
  - agents stop when no executable typed route is present or when route is diagnostic-only;
  - detailed binding law lives in `references/operator-route-authority.md`.
- Keep reviewer recursion prevention prompt-only and reviewer-scoped.
- Keep prompt budgets enforced.

### Done when

- Generated skills carry one compact route-law block, not repeated detailed binding law.
- `using-featureforge` top-level route law is compact and links the canonical reference for detail.
- `references/operator-route-authority.md` remains the detailed route-law source.
- Skill-doc tests assert semantic invariants:
  - canonical reference exists and is linked;
  - no hidden helper fallback;
  - display command text stays non-executable compatibility text;
  - typed argv/template and stop-on-diagnostic law present;
  - reviewer recursion prompt-only law present.
- Tests no longer exact-pin incidental route-law prose.
- Skill budgets remain in enforce mode and generated docs are fresh.

### Files

- Modify: `scripts/gen-skill-docs.mjs`
- Modify: `skills/**/SKILL.md.tmpl`
- Regenerate: `skills/**/SKILL.md`
- Modify: `references/operator-route-authority.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-budget.test.mjs` or `skills/skill-doc-budgets.json` if line counts change
- Modify: `docs/testing.md` if validation docs change

### Implementation Steps

1. Reduce `buildInstalledControlPlaneSection`, `buildOperatorPublicCommandAuthorityBullets`, and `buildOperatorRouteAuthoritySection` to compact top-level law.
2. Move any detailed binding law removed from skills into `references/operator-route-authority.md` if it is not already there.
3. Trim redundant route-law prose from `skills/using-featureforge/SKILL.md.tmpl` and any high-use templates that repeat the same negative rules.
4. Regenerate skill docs with `node scripts/gen-skill-docs.mjs`.
5. Rewrite exact-prose tests into semantic fragment/invariant checks.
6. Update budget manifest if generated line counts change.

### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo test --test runtime_instruction_review_contracts -- --nocapture`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Clean-context review of Task 4 before proceeding.

## Task 5: Final audit and stop condition

### Spec Coverage

- User loop requirement: audit -> implementation until no actionable audit issues remain.

### Goal

Run the full audit process again, including the signal-to-noise auditor, and stop only if no actionable audit issues remain.

### Context

This task is not complete just because tests pass. It is complete only after a clean-context audit finds no actionable issues, or after any new actionable issues are converted into the next implementation plan and the loop continues.

### Constraints

- Run full `cargo clean` before starting the audit iteration.
- Do not use FeatureForge runtime skills or project skills.
- Do not allow subagents to spawn subagents.
- Do not interrupt productive in-flight subagents.

### Done when

- Full validation passes.
- Clean-context final implementation review passes.
- A new audit iteration with public CLI, test realism, control-plane, plan-review, reentry-loop, prompt-surface, modularization, public-output, and signal-to-noise auditors completes.
- If no actionable findings remain, report completion.
- If actionable findings remain, create the next remediation plan and continue the loop.

### Files

- No required code files unless audit findings require more remediation.

### Implementation Steps

1. Run final full validation.
2. Dispatch clean-context review for the full plan.
3. Remediate any review findings and repeat validation/review until clean.
4. Run `cargo clean`.
5. Dispatch the full audit agent set, including signal-to-noise auditor.
6. Synthesize the audit result.

### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `git diff --check`
- `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Clean-context implementation review.
- Clean-context audit.
