# Deep Runtime Audit Remediation Plan

**Workflow State:** Draft
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `docs/archive/audits/2026-05-05-deep-runtime-safety-audit.md`
**Source Spec Revision:** 1
**Last Reviewed By:** unreviewed

## Goal

Close the remaining public-runtime safety gaps found in the 2026-05-05 deep audit so normal agents can progress through FeatureForge without hidden commands, direct-command gate bypasses, token-only follow-up interpretation, or prompt-surface overrun.

## Architecture

- Treat workflow route resolution and execution mutation eligibility as one public contract before execution starts.
- Keep runtime-owned state authoritative; do not reintroduce receipt/provenance artifacts as control-plane truth.
- Make `advance-late-stage` the public owner for late-stage progression, including final-review dispatch lineage, finish-review checkpointing, and finish completion.
- Keep JSON `recommended_public_command_argv` and `required_inputs` as machine authority. Text output may summarize but must not become executable authority.
- Keep tests honest: public-flow tests use the compiled shipped binary unless they are explicitly quarantined as internal helper tests.
- Keep prompt budgets enforced by editing templates and regenerating outputs.

## Change Surface

- Runtime and CLI:
  - `src/execution/commands/begin.rs`
  - `src/execution/commands/advance_late_stage.rs`
  - `src/execution/commands/common/operator_outputs.rs`
  - `src/execution/command_eligibility.rs`
  - `src/execution/late_stage_route_selection.rs`
  - `src/execution/phase.rs`
  - `src/execution/query.rs`
  - `src/execution/read_model.rs`
  - `src/execution/read_model/public_route_projection.rs`
  - `src/workflow/operator.rs`
  - `src/workflow/status.rs`
  - `schemas/plan-execution-status.schema.json`
  - `schemas/workflow-operator.schema.json`
- Tests:
  - `tests/public_replay_churn.rs`
  - `tests/public_cli_flow_contracts.rs`
  - `tests/workflow_shell_smoke.rs`
  - `tests/workflow_runtime.rs`
  - `tests/workflow_runtime_final_review.rs`
  - `tests/contracts_execution_runtime_boundaries.rs`
  - `tests/runtime_module_boundaries.rs`
  - `tests/codex-runtime/*.test.mjs`
- Prompt/doc packaging:
  - `skills/*.md.tmpl`
  - `skills/*/SKILL.md`
  - `skills/skill-doc-budgets.json`
  - `.codex/INSTALL.md`
  - `.copilot/INSTALL.md`
  - `scripts/gen-skill-docs.mjs`

## Preconditions

- Do not use hidden/debug/compatibility commands as implementation dependencies.
- Do not add `#[allow(clippy::...)]`, weaken lint policy, or loosen prompt-budget enforcement.
- Do not move mandatory top-level workflow law solely into companion references.
- Do not hand-edit generated `skills/*/SKILL.md` when a `.tmpl` exists; edit templates and regenerate.
- Do not add public-output wording that instructs agents to record receipts, rebuild evidence, or run gate/record helpers.

## Known Footguns / Constraints

- `workflow status` already blocks Engineering Approved plans with missing/stale plan-fidelity artifacts. Direct `plan execution status` and `begin` must not become a parallel entrypoint that bypasses that block.
- `final_review_dispatch_required` is not just a wait state if the runtime requires current dispatch lineage before recording. The public path must either create/refresh lineage or expose one public action with typed argv/inputs.
- `finish_review_gate_ready` and `finish_completion_gate_ready` cannot remain normal next actions with null argv. If they are diagnostic-only, label them as diagnostic-only; if they are normal flow, make them executable through public `advance-late-stage`.
- Token-only `required_follow_up` values are not enough for agents. Every public blocked response needs executable argv, required inputs, explicit requery, or diagnostic-only fail-closed semantics.
- Existing tests may pass because they encode null command behavior. Update tests to prove public reachability, not current dead ends.

## Requirement Coverage Matrix

- REQ-001: Direct public execution commands enforce the same pre-execution plan-fidelity gate as workflow routing. Covered by Tasks 1 and 4.
- REQ-002: Final-review dispatch lineage is reachable through public late-stage flow. Covered by Tasks 2 and 4.
- REQ-003: Finish-review checkpointing and finish completion are reachable through public late-stage flow. Covered by Tasks 2 and 4.
- REQ-004: Public blocked outputs always provide one executable public recovery contract or an explicit diagnostic-only state. Covered by Task 3.
- REQ-005: Public-flow tests prove shipped CLI behavior and static guards catch direct-helper leaks. Covered by Task 4.
- REQ-006: Generated skill docs pass enforced per-skill budgets while mandatory law remains top-level. Covered by Task 5.
- REQ-007: Route blocking scope/task and late-stage phase mapping have single semantic owners. Covered by Task 6.

## Task 1: Close Direct Begin Plan-Fidelity Bypass

**Spec Coverage:** REQ-001

**Goal:** Ensure direct `plan execution status` and `plan execution begin` cannot start implementation when workflow routing would block an Engineering Approved plan for missing/stale five-surface plan-fidelity review.

**Context:**

- `begin` currently authorizes from `public_status_from_supplied_context_with_shared_routing` in `src/execution/commands/begin.rs`.
- `load_execution_context_with_policies` checks Engineering Approved headers but does not evaluate plan-fidelity artifacts.
- `query_workflow_routing_state` already detects `route_is_engineering_approval_fidelity_blocked` and preserves the non-runtime review route.

**Constraints:**

- Do not reintroduce plan-fidelity receipts.
- Do not require agents to run `workflow status` before `plan execution begin`; the mutation itself must fail closed.
- Preserve already-started execution semantics deliberately. If a plan-fidelity artifact disappears after execution has started, direct mutation behavior must be explicit and covered by tests.

**Done when:**

- Direct `plan execution begin` fails before mutation for an Engineering Approved plan whose current five-surface plan-fidelity artifact is missing, stale, malformed, or incomplete.
- `plan execution status` does not expose a begin route that contradicts workflow route gating before execution starts.
- The failure mentions the public review route/gate reason and does not mention receipts or hidden helpers.

**Files:**

- Modify: `src/execution/commands/begin.rs`
- Modify: `src/execution/context.rs`
- Modify: `src/execution/query.rs`
- Modify: `src/execution/read_model.rs`
- Modify: `src/workflow/status.rs`
- Test: `tests/public_replay_churn.rs`
- Test: `tests/workflow_runtime.rs`
- Test: `tests/plan_execution.rs`

**Implementation steps:**

1. Extract a shared pre-execution implementation-handoff gate helper that evaluates the current plan-fidelity report for an Engineering Approved plan and returns a typed block reason matching workflow status reason codes.
2. Call that helper from direct execution status loading before exposing a first-begin route for a plan whose execution has not started.
3. Call the same helper from `begin` immediately after loading the mutation context and before `claim_step_write_authority`.
4. Make the blocked direct-begin error use `FailureClass::PlanNotExecutionReady` or a more specific existing class, with reason code `engineering_approval_missing_plan_fidelity_review`, `engineering_approval_stale_plan_fidelity_review`, or the current shared reason code.
5. Add a public replay test that writes an Engineering Approved plan without a current fidelity artifact, calls `plan execution status`, then attempts direct `plan execution begin` using any exposed fingerprint. The test must assert no mutation and no active task.
6. Add stale and malformed artifact variants if fixtures already exist; otherwise add the missing case first and leave the variants as explicit follow-up tests in the same module.
7. Update any misleading existing test names that only assert workflow status, or extend them to actually attempt direct begin.

**Validation expectations:**

- `cargo nextest run --test public_replay_churn`
- `cargo nextest run --test workflow_runtime`
- `cargo nextest run --test plan_execution`
- `cargo nextest run --test public_cli_flow_contracts`
- `cargo clippy --all-targets --all-features -- -D warnings`

## Task 2: Make Late-Stage Final Review and Finish Publicly Reachable

**Spec Coverage:** REQ-002, REQ-003

**Goal:** Ensure final-review dispatch lineage, finish-review checkpointing, and finish completion are handled through shipped public commands, preferably `plan execution advance-late-stage`.

**Context:**

- `final_review_dispatch_required`, `finish_review_gate_ready`, and `finish_completion_gate_ready` are currently omitted from public command recommendations.
- `advance-late-stage` records final-review results only after checking `final_review_recording_ready`; missing dispatch lineage can block before `ensure_current_review_dispatch_id(... FinalReview ...)` runs.
- Finish-review checkpointing is currently persisted by `ExecutionRuntime::review_gate`, not a public plan execution command.

**Constraints:**

- Do not expose `record-review-dispatch`, `gate-review`, or `gate-finish` as normal public commands.
- Do not make agents manually edit branch closure, release readiness, final review, QA, or finish checkpoint artifacts.
- Preserve current branch closure and release readiness fingerprint checks.
- Keep negative final-review results and finish blockers routed through existing reentry/handoff semantics.

**Done when:**

- `final_review_dispatch_required` has a public path that either records/refreshes dispatch lineage or asks for external review with parseable required inputs and no hidden command dependency.
- Public final-review recording can proceed after release readiness without a separate hidden dispatch mutation.
- `finish_review_gate_ready` and `finish_completion_gate_ready` expose executable public argv or are reclassified as diagnostic-only with no normal `finish branch` next action.
- Public route tests no longer assert `recommended_command == null` for normal finish progression.

**Files:**

- Modify: `src/execution/commands/advance_late_stage.rs`
- Modify: `src/execution/commands/common/operator_outputs.rs`
- Modify: `src/execution/command_eligibility.rs`
- Modify: `src/execution/late_stage_route_selection.rs`
- Modify: `src/execution/phase.rs`
- Modify: `src/execution/state/runtime_methods.rs`
- Modify: `src/execution/state/review_gate.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `schemas/plan-execution-status.schema.json`
- Modify: `schemas/workflow-operator.schema.json`
- Test: `tests/workflow_shell_smoke.rs`
- Test: `tests/plan_execution_final_review.rs`
- Test: `tests/workflow_runtime_final_review.rs`
- Test: `tests/public_replay_churn.rs`

**Implementation steps:**

1. Add explicit public late-stage modes for final-review dispatch, finish-review checkpoint, and finish completion, or extend existing `advance-late-stage` inference so the phase detail determines the correct intent without hidden flags.
2. For final-review dispatch, move or add `ensure_current_review_dispatch_id(... ReviewDispatchScopeArg::FinalReview ...)` before the `final_review_recording_ready` early-out when final-review recording is requested and the branch/release surfaces are current.
3. Re-derive workflow/operator after dispatch bootstrap so the same public command can proceed to final-review recording when `--external-review-result-ready` and required review inputs are present.
4. For `finish_review_gate_ready`, implement an `advance-late-stage` branch that evaluates the same gate logic, persists `finish_review_gate_pass_branch_closure_id` only when the current branch closure is valid, and records the command as `advance_late_stage` or a new public transition name, not `gate_review`.
5. For `finish_completion_gate_ready`, implement the public finish-completion mutation or route to a single explicit public action that completes the branch handoff. Reuse existing `finish_gate` validation logic without making `finish_gate` a public command dependency.
6. Remove `DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED`, `DETAIL_FINISH_REVIEW_GATE_READY`, and `DETAIL_FINISH_COMPLETION_GATE_READY` from the omitted-public-command list if they become executable normal routes.
7. Update `PublicAdvanceLateStageMode`, public argv construction, required inputs, and schemas.
8. Replace shell-smoke assertions that expect null commands with assertions that the recommended public argv executes and advances the route.
9. Add public replay tests for release-ready to final-review-dispatch to final-review-recording and QA-ready to finish-review to finish-completion.

**Validation expectations:**

- `cargo nextest run --test workflow_shell_smoke`
- `cargo nextest run --test plan_execution_final_review`
- `cargo nextest run --test workflow_runtime_final_review`
- `cargo nextest run --test public_replay_churn`
- `cargo nextest run --test runtime_authority_contracts`
- `cargo clippy --all-targets --all-features -- -D warnings`

## Task 3: Normalize Public Follow-Up and Text Output Contracts

**Spec Coverage:** REQ-004

**Goal:** Remove token-only recovery traps and make public text output subordinate to JSON argv authority.

**Context:**

- Several blocked mutator outputs return only `required_follow_up`.
- Text output prints display command strings that can look executable.
- Active docs say display strings are not authority, but the runtime text surface should not rely on docs alone.

**Constraints:**

- Do not remove `recommended_command` from JSON if compatibility requires it; instead make `recommended_public_command_argv` and `required_inputs` complete.
- Do not turn diagnostic-only states into guessed commands.
- Do not mention hidden helpers in remediation text.

**Done when:**

- Every blocked public JSON output has exactly one of: executable public argv, parseable required inputs, explicit requery via `workflow operator --json`, or diagnostic-only no-follow-up semantics.
- Text output labels display command strings as summaries and points to JSON argv for execution.
- Tests assert no token-only blocked outputs for normal public flows.

**Files:**

- Modify: `src/execution/commands/common/operator_outputs.rs`
- Modify: `src/execution/commands/advance_late_stage.rs`
- Modify: `src/execution/commands/close_current_task.rs`
- Modify: `src/execution/review_state.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `src/workflow/status.rs`
- Modify: `schemas/plan-execution-status.schema.json`
- Modify: `schemas/workflow-operator.schema.json`
- Test: `tests/workflow_shell_smoke.rs`
- Test: `tests/public_cli_flow_contracts.rs`
- Test: `tests/runtime_behavior_golden.rs`

**Implementation steps:**

1. Inventory all public output structs with `required_follow_up`, `recommended_command`, `recommended_public_command_argv`, `required_inputs`, and `rederive_via_workflow_operator`.
2. Create a shared helper that converts route/operator state into one public recovery contract.
3. Replace ad hoc token-only blocked outputs in `advance-late-stage`, `close-current-task`, and `repair-review-state`.
4. For `request_external_review`, expose required reviewer inputs and the exact public route to requery after external review result is ready.
5. For `execution_reentry`, expose the exact public reopen/begin/repair command if a concrete route exists; otherwise emit diagnostic-only blocked state.
6. Change text renderers to say "Display command summary" or "Use JSON recommended_public_command_argv for execution" wherever they render command-shaped text.
7. Add static tests that reject normal-flow blocked outputs with a follow-up token but no argv/inputs/requery/diagnostic marker.

**Validation expectations:**

- `cargo nextest run --test workflow_shell_smoke`
- `cargo nextest run --test public_cli_flow_contracts`
- `cargo nextest run --test runtime_behavior_golden`
- `cargo nextest run --test execution_query`
- `cargo clippy --all-targets --all-features -- -D warnings`

## Task 4: Restore Public-Test Realism

**Spec Coverage:** REQ-001, REQ-002, REQ-003, REQ-005

**Goal:** Ensure tests that claim public runtime behavior exercise the shipped compiled CLI boundary, and quarantine direct helper tests.

**Context:**

- Public-flow protected suites still call `doctor_for_runtime`, `status_refresh`, `phase_for_runtime`, `handoff_for_runtime`, and direct operator helpers.
- Static public-flow guards miss those helper names.
- `run_featureforge_output(..., _real_cli: bool, ...)` ignores `_real_cli`.

**Constraints:**

- Direct helper tests are allowed only when named/scoped as internal boundary tests.
- Public replay setup may synthesize historical broken states, but replay actions must execute through public commands.
- Do not weaken static scans to make current tests pass.

**Done when:**

- Protected public-flow files no longer call direct workflow/runtime helper surfaces, or those tests are moved to internal-only suites with explicit naming.
- Static guards reject direct helper wrappers and direct runtime calls in protected public-flow files.
- Direct-vs-real parity tests either truly compare two paths in an internal boundary suite or are renamed as compiled-CLI public tests.

**Files:**

- Modify: `tests/public_cli_flow_contracts.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_runtime_final_review.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/contracts_execution_runtime_boundaries.rs`
- Modify: `tests/support/runtime_phase_handoff.rs`
- Modify: `tests/support/public_featureforge_cli.rs`

**Implementation steps:**

1. Extend `direct_runtime_surface_marker` and related scanners to catch `doctor_for_runtime`, `doctor_phase_and_next_for_runtime_with_args`, `status_refresh`, `phase_for_runtime`, `handoff_for_runtime`, and wrappers around those helpers.
2. Add scanner fixture tests that prove each new forbidden helper is rejected in protected public-flow files.
3. Convert public assertions in `workflow_shell_smoke.rs`, `workflow_runtime.rs`, and `workflow_runtime_final_review.rs` to compiled CLI calls where stdout/stderr/JSON shape matters.
4. Move remaining direct helper tests to `internal_*` files or rename functions with `internal_only_` and add an explicit boundary comment.
5. Fix `run_featureforge_output` so `_real_cli` is either honored or removed. If direct helper parity is no longer desired, rename affected tests to avoid claiming parity.
6. Add direct public CLI regression tests for the blockers from Tasks 1 and 2.

**Validation expectations:**

- `cargo nextest run --test public_cli_flow_contracts`
- `cargo nextest run --test workflow_shell_smoke`
- `cargo nextest run --test workflow_runtime`
- `cargo nextest run --test workflow_runtime_final_review`
- `cargo nextest run --test contracts_execution_runtime_boundaries`
- `cargo clippy --all-targets --all-features -- -D warnings`

## Task 5: Fix Prompt Budget and Packaging Drift

**Spec Coverage:** REQ-006

**Goal:** Make the generated skill docs pass enforced budgets while preserving mandatory top-level law and correcting install docs.

**Context:**

- `node --test tests/codex-runtime/*.test.mjs` fails because several generated skills exceed per-skill line budgets.
- `.codex/INSTALL.md` and `.copilot/INSTALL.md` still say generated preambles auto-run update checks, while tests forbid that behavior.

**Constraints:**

- Edit `.md.tmpl` files, not generated `SKILL.md` files directly.
- Do not move mandatory law solely into companion references.
- Keep companion references discoverable and packaged.
- Do not raise budgets unless the change includes reviewed rationale for why top-level content must grow.

**Done when:**

- The five over-budget skills are at or below manifest max lines.
- Generated docs are fresh.
- Install docs match the generated preamble contract.
- Reviewer recursion language remains prompt-only and reviewer-prompt scoped.

**Files:**

- Modify: `skills/systematic-debugging/SKILL.md.tmpl`
- Modify: `skills/writing-plans/SKILL.md.tmpl`
- Modify: `skills/document-release/SKILL.md.tmpl`
- Modify: `skills/requesting-code-review/SKILL.md.tmpl`
- Modify: `skills/executing-plans/SKILL.md.tmpl`
- Modify: `.codex/INSTALL.md`
- Modify: `.copilot/INSTALL.md`
- Generated: `skills/*/SKILL.md`
- Test: `tests/codex-runtime/skill-doc-budget.test.mjs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

**Implementation steps:**

1. Run or inspect the budget report and record current per-skill line counts.
2. For each over-budget skill, classify top-level content as mandatory law, quick-start operational instruction, or reference material.
3. Keep mandatory law and the minimum operational path top-level.
4. Move examples, rationale, and lower-frequency detail into existing or new packaged companion references.
5. Update companion-reference links in the top-level skill docs.
6. Correct `.codex/INSTALL.md` and `.copilot/INSTALL.md` to remove the auto update-check claim.
7. Run `node scripts/gen-skill-docs.mjs` to regenerate checked-in `SKILL.md` files.
8. Confirm reviewer recursion prompts remain unchanged in scope and mechanism.

**Validation expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`

## Task 6: Remove Residual Split Decisioning

**Spec Coverage:** REQ-007

**Goal:** Centralize the remaining duplicated routing/status/operator semantic decisions.

**Context:**

- Blocking scope/task is derived or overridden in router/query, read-model public route projection, and workflow operator.
- Late-stage phase-detail to phase mapping is duplicated.
- `state.rs` remains a broad compatibility facade.

**Constraints:**

- Preserve public JSON values and schema compatibility unless an explicit migration is documented.
- Do not move mutation decisions into workflow presentation.
- Do not make tests duplicate semantic logic unless the test documents a boundary reason.

**Done when:**

- Blocking scope/task is computed once and projected to status/operator.
- Late-stage phase mapping delegates to the shared helper.
- Boundary tests cover the new owner and reject duplicate local mapping.
- `state.rs` re-exports are reduced where call sites can import focused modules directly.

**Files:**

- Modify: `src/execution/query.rs`
- Modify: `src/execution/router.rs`
- Modify: `src/execution/read_model/public_route_projection.rs`
- Modify: `src/execution/late_stage_route_selection.rs`
- Modify: `src/workflow/operator.rs`
- Modify: `src/execution/state.rs`
- Test: `tests/runtime_module_boundaries.rs`
- Test: `tests/contracts_execution_runtime_boundaries.rs`

**Implementation steps:**

1. Define a shared blocking-scope/task projection type adjacent to `PublicRouteDecision` or the execution query boundary.
2. Replace read-model and workflow-operator local overrides with that shared projection result.
3. Add tests that construct branch, task, reentry, reconcile, and blocked-runtime-bug routes and assert the same blocking scope/task across status and operator.
4. Replace the late-stage phase-detail mapping in `late_stage_route_selection.rs` with the shared phase canonicalization helper.
5. Add a static boundary test that rejects new local phase-detail to phase match tables outside the approved owner.
6. Audit `state.rs` re-exports and remove any that are no longer needed after the public-flow and late-stage fixes.

**Validation expectations:**

- `cargo nextest run --test runtime_module_boundaries`
- `cargo nextest run --test contracts_execution_runtime_boundaries`
- `cargo nextest run --test execution_query`
- `cargo nextest run --test workflow_runtime`
- `cargo clippy --all-targets --all-features -- -D warnings`

## Final Validation Gate

Run the full targeted matrix after all tasks:

```bash
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs --check
node --test tests/codex-runtime/*.test.mjs
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --test runtime_authority_contracts
cargo nextest run --test workflow_runtime
cargo nextest run --test workflow_shell_smoke
cargo nextest run --test workflow_entry_shell_smoke
cargo nextest run --test plan_execution
cargo nextest run --test plan_execution_final_review
cargo nextest run --test workflow_runtime_final_review
cargo nextest run --test contracts_execution_runtime_boundaries
cargo nextest run --test execution_query
cargo nextest run --test public_cli_flow_contracts
cargo nextest run --test runtime_module_boundaries
cargo nextest run --test public_replay_churn
cargo test --test liveness_model_checker
```

The work is not complete until the public replay tests prove the direct-begin, final-review dispatch, and finish-progression dead ends are closed through shipped public commands.
