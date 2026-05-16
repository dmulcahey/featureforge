# Runtime Signal/Noise Sixteenth Audit Remediation

## Workflow State

- Status: Engineering implementation plan
- Source audit: post-fifteenth-remediation deep audit with public CLI, runtime authority, evidence/projection, plan-review, stale-loop, prompt-surface, modularization, public-output UX, and signal/noise subagents
- Current gate: implement tasks in order; after each task run strict Clippy and the full nextest suite with no fail-fast before clean-context review

## Plan Revision

- Revision: 1
- Date: 2026-05-10

## Execution Mode

- Sequential task execution.
- Do not use FeatureForge runtime skills or project skills.
- Rust changes must follow the user-provided Rust skill guidance: typed APIs, centralized ownership, warning-clean code, and no lint suppression.
- Clean-context review subagents must not spawn additional subagents.

## Goal

Remove the actionable issues found by the latest audit loop without adding another layer of self-referential workflow churn:

- Diagnostic-only runtime states must not present normal mutation guidance in public doctor text.
- Workflow operator presentation must consume one finalized route decision instead of reconstructing or re-preferencing routing/status fields locally.
- Schema vocabularies must be owned by runtime decision modules or constant arrays, not duplicate enum names maintained only for schema generation.
- Prompt/docs/tests must keep high-signal public contracts while deleting brittle exact-wording and private-helper assertions that do not protect user-facing behavior.
- Module-boundary tests must enforce real boundaries without pinning incidental private helper names.

## Architecture

The desired authority flow remains:

CLI args -> command module -> transition guard -> event append -> reducer -> read model -> route decision -> workflow operator presentation.

This plan keeps runtime truth inside reducer/read-model/route decision owners and makes public presentation consume that truth. It does not introduce new routing authorities, hidden helpers, or prompt-law copies. Tests should assert public behavior, schema/runtime vocabulary parity, and import/write boundaries.

## Change Surface

- `src/workflow/doctor_dashboard.rs`
- `src/workflow/operator.rs`
- `src/execution/status.rs`
- `src/execution/phase.rs`
- `src/execution/next_action.rs`
- `src/execution/route_plan/state_kind.rs`
- `src/execution/route_plan/decision_support.rs` and adjacent route-plan modules if needed
- `schemas/plan-execution-status.schema.json`
- `tests/workflow_runtime.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/fixtures/runtime-goldens/README.md`
- generated docs only if template or generator changes require regeneration

## Preconditions

- Start each audit iteration with `cargo clean`; this was done before the current audit.
- Full nextest must stay performant. If a full nextest run exceeds 4-5 minutes, run `cargo clean`, rerun full nextest, and if it still exceeds 4-5 minutes stop implementation work and address introduced performance issues.
- The first clean full nextest run reported one non-reproducible internal test failure; a targeted rerun and full rerun passed. Treat any recurrence as a real test harness issue.
- Do not revert unrelated working-tree changes.

## Known Footguns / Constraints

- Do not solve signal/noise concerns by deleting protection around the actual historic failures: hidden helpers, display command execution, projection control-plane leakage, stale closure reentry, reviewer recursion, or prompt-budget enforcement.
- Do not replace private-helper name pinning with broader source-string scans that are just as brittle.
- Do not create a second schema vocabulary owner. Runtime constants should drive schema enum injection and parity tests.
- Do not let operator text mention nonexistent public fields such as `diagnostic_reason_codes` on the operator payload.
- Do not weaken `blocked_runtime_bug`: it is a stop/report diagnostic state, not a normal route with mutation steps.
- Do not hand-edit generated skill docs when a template or generator owns them.

## Requirement Coverage Matrix

| Requirement | Task |
| --- | --- |
| Diagnostic-only doctor output never sends agents into normal mutation guidance | 1 |
| Operator blocked-runtime text references the actual public payload fields | 1 |
| Operator consumes one authoritative route decision | 2 |
| Missing route decisions fail closed instead of being reconstructed in presentation | 2 |
| Runtime schema vocabularies are centralized | 3 |
| Schema generation remains current and deterministic | 3 |
| Prompt/doc tests keep high-signal public contracts and drop brittle prose pinning | 4 |
| Runtime golden docs describe display-only command exclusion accurately | 4 |
| Boundary tests enforce architecture without private helper overfitting | 5 |
| Full validation and review loop is clean before the next audit | All tasks |

## Task 1: Diagnostic-only public output

### Spec Coverage

- Public-output UX audit findings H1 and H2.
- `blocked_runtime_bug` must remain diagnostic-only.

### Goal

Make doctor dashboard and operator public text stop/report-oriented for runtime diagnostic states, with no normal mutation instruction rows or nonexistent field names.

### Context

Audit found `render_doctor_dashboard` can show blocker action text such as `DOCTOR_TYPED_OPERATOR_ROUTE_ACTION` even when `doctor.resolution.kind == runtime_diagnostic_required`. Audit also found `next_step_text` tells agents to inspect `diagnostic_reason_codes` on workflow operator JSON, but the public operator payload exposes `blocking_reason_codes`.

### Constraints

- Preserve structured JSON fields; this task is about text presentation.
- Keep blocker codes visible for explanation, but suppress or normalize action text when the resolution is diagnostic-only.
- Do not hide the stop reason.

### Done when

- Runtime diagnostic doctor dashboard blocker rows never instruct `advance-late-stage`, `close-current-task`, final-review dispatch, release-doc routes, or typed operator mutation binding.
- Operator blocked-runtime bug text names fields that actually exist on operator JSON.
- Tests cover the diagnostic-only doctor dashboard behavior.

### Files

- `src/workflow/doctor_dashboard.rs`
- `src/workflow/operator.rs`
- `tests/workflow_runtime.rs` or the existing doctor-dashboard contract test location

### Implementation Steps

1. Add a small helper that detects diagnostic-only doctor resolutions, preferably by comparing `doctor.resolution.kind` to the existing runtime diagnostic token.
2. In the blockers section, use a diagnostic action renderer for diagnostic-only resolution. It should render one stop/report action for every blocker code rather than code-specific mutation guidance.
3. Keep warning rendering unchanged unless a warning is also part of the diagnostic-only stop surface.
4. Replace operator blocked-runtime bug text so it says to inspect `blocking_reason_codes` and any other actually serialized diagnostic context; do not mention `diagnostic_reason_codes` as an operator JSON field.
5. Add or update focused tests to assert the text behavior for a `blocked_runtime_bug` doctor fixture.

### Validation Expectations

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Clean-context review against Task 1.

## Task 2: Operator route-decision authority

### Spec Coverage

- Modularization/split-decisioning finding G1.
- Signal/noise finding I1/I2 around route decision reconstruction and local route precedence.

### Goal

Make workflow operator presentation require and consume the finalized runtime route decision produced by routing, rather than reconstructing or making route-precedence decisions locally.

### Context

`build_context_from_routing` currently accepts an optional route-decision override and calls `route_decision_from_routing` when the routing state lacks one. It also chooses phase/detail/action from `execution_status` before route decision, creating a second presentation precedence path.

### Constraints

- Do not remove `route_decision_from_routing` from its real owner. The routing/query path still needs to compute it.
- Presentation should fail closed if a routing state reaches the operator without a route decision.
- Keep intentional display-phase compatibility isolated and documented if it must stay.

### Done when

- `src/workflow/operator.rs` no longer imports or calls `route_decision_from_routing`.
- `build_context_from_routing` requires `routing.route_decision`.
- Operator phase/detail/next-action fields come from the route decision, except for any documented compatibility display-phase rule.
- Existing operator/status goldens and public route tests pass.

### Files

- `src/workflow/operator.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/fixtures/runtime-goldens/public-runtime-routes.json` if public JSON changes intentionally

### Implementation Steps

1. Remove `route_decision_from_routing` from workflow operator imports.
2. Change `build_context_from_runtime` to pass only `ExecutionRoutingState` into `build_context_from_routing`.
3. In `build_context_from_routing`, extract `routing.route_decision` with a fail-closed `JsonFailure` if absent.
4. Drive `operator_phase`, `operator_phase_detail`, `operator_next_action`, state kind, next public action, blockers, repair targets, and command surfaces from that decision.
5. Retain status-derived fields only for non-routing telemetry that does not decide the public route: review-state status, tree IDs, projection diagnostics, and execution command context if the route decision does not own it.
6. Update boundary tests so workflow operator is forbidden from calling route decision constructors.

### Validation Expectations

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Clean-context review against Task 2.

## Task 3: Canonical schema vocabularies

### Spec Coverage

- Signal/noise finding I1: schema route vocabulary duplicated outside runtime owners.

### Goal

Make plan-execution schema enum values derive from runtime constants or route-owner arrays, not hand-maintained schema-only enums.

### Context

`src/execution/status.rs` has schema-only enums for review status, phase detail, state kind, next action, and command kinds. Some runtime arrays already exist, such as `PLAN_EXECUTION_STATUS_PHASE_DETAIL_VALUES` and `PUBLIC_STATUS_PHASE_VALUES`; other vocabularies need explicit owner arrays near their runtime decision modules.

### Constraints

- Preserve generated schema shape and field descriptions unless the current shape is wrong.
- Do not duplicate the same value list in tests.
- Keep generated schema deterministic.

### Done when

- Schema values for phase detail, review status, state kind, next action, QA requirement, execution command kind, public repair target command kind, and required follow-up are injected from constants.
- Schema-only enums are removed where they only existed to duplicate runtime values.
- A static or unit contract asserts generated schema enum sets match the owning runtime constants.

### Files

- `src/execution/status.rs`
- `src/execution/phase.rs`
- `src/execution/next_action.rs`
- `src/execution/route_plan/state_kind.rs`
- `src/execution/route_plan/decision.rs` or another route-plan owner for required follow-up values
- `schemas/plan-execution-status.schema.json`
- Existing schema or boundary tests

### Implementation Steps

1. Add owner constant arrays for review-state statuses, state kinds, next-action strings, QA requirement values, execution command kinds, public repair target command kinds, and required follow-up tokens.
2. Replace schema-only enum `#[schemars(with = ...)]` usage with string schemas tightened by a shared schema-injection helper.
3. Update `write_plan_execution_schema` to insert or replace the relevant `$defs` and property schemas from those arrays.
4. Add a helper to inject string enum definitions and property `$ref`/nullable references, avoiding bespoke per-field code.
5. Regenerate `schemas/plan-execution-status.schema.json`.
6. Add/update tests that verify schema enum arrays come from the owner constants.

### Validation Expectations

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Clean-context review against Task 3.

## Task 4: Prompt/doc/test signal cleanup

### Spec Coverage

- Signal/noise findings I3, I4, and I7.
- Prompt-surface budget must remain enforced while skills stay actionable.

### Goal

Reduce brittle exact-prose and duplicated route-law assertions while preserving tests that prevent old agent failure modes.

### Context

`tests/codex-runtime/skill-doc-contracts.test.mjs` has thousands of lines and pins exact positive prose in several places. The useful protection is budget/freshness/trap coverage, route-authority references, reviewer recursion prohibition, and no hidden-helper/display-command guidance. Runtime golden README still describes preserving "recommended command" even though route goldens exclude the display-only field.

### Constraints

- Do not delete prompt-budget enforcement.
- Do not remove tests that catch hidden helper command mentions, display-command execution, reviewer recursion, missing route-authority reference packaging, generated-doc staleness, or top-level mandatory law.
- Do not add more repeated skill prose.

### Done when

- Exact positive-prose assertions are replaced by compact semantic checks where possible.
- Late-stage route-law duplication in Node tests is either generated from runtime-owned data or reduced to public JSON contract checks.
- Runtime golden README accurately says display-only `recommended_command` is excluded from route goldens.
- Node docs tests remain passing and materially smaller or less brittle.

### Files

- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/fixtures/runtime-goldens/README.md`
- `scripts/gen-skill-docs.mjs` only if a generator-owned simplification is needed
- skill templates/generated docs only if generator output changes

### Implementation Steps

1. Identify exact wording assertions that do not protect a failure mode.
2. Replace them with semantic checks for contract terms, required references, and forbidden traps.
3. Collapse repeated late-stage route-law maps into one runtime-owned fixture or remove them if covered by Rust public route tests.
4. Update runtime-goldens README wording around display-only fields.
5. Run generator checks; regenerate only if the generator or templates changed.

### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Clean-context review against Task 4.

## Task 5: Boundary-test signal cleanup

### Spec Coverage

- Modularization finding G3.
- Signal/noise findings I5 and I6.

### Goal

Keep architecture boundary tests focused on imports, call direction, write surfaces, and public behavior, not incidental private helper names.

### Context

`tests/runtime_module_boundaries.rs` still pins helpers such as `source_route_decision_for_repair_follow_up_binding`, `refresh_route_bound_repair_state`, and `public_repair_target_candidates_from_authority`. `tests/public_cli_flow_contracts.rs` mixes public CLI contract checks with implementation-shape checks.

### Constraints

- Preserve tests that prevent workflow operator from importing mutation/write helpers or command modules from writing projections directly.
- Preserve tests that public-flow tests use real CLI where the CLI boundary is the contract.
- If a test intentionally duplicates scanner behavior, document the boundary and keep it minimal.

### Done when

- Private helper-name pins for repair-route internals are removed or replaced by module-boundary assertions.
- Public CLI flow tests focus on public binary/help/no-hidden/no-display traps and typed public command behavior.
- Scanner self-tests, if still needed, live in a small focused scanner contract rather than mixed into architecture assertions.

### Files

- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/support/rust_source_scan.rs`
- `tests/rust_source_scan_contracts.rs` if scanner-specific tests move there

### Implementation Steps

1. Remove remaining exact private helper-name assertions called out by audit.
2. Replace them with import/call boundary assertions against owner modules and forbidden module crossings.
3. Move scanner-specific parser expectations into `tests/rust_source_scan_contracts.rs` if they are not architecture boundaries.
4. Simplify public CLI flow implementation-shape assertions so public behavior remains the tested contract.
5. Add nearby comments only where an intentional duplicated boundary exists.

### Validation Expectations

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Clean-context review against Task 5.

## Final Whole-Plan Validation and Review

After all tasks pass their individual verification and reviews:

1. Run the full validation set:
   - `node scripts/gen-skill-docs.mjs --check`
   - `node scripts/gen-agent-docs.mjs --check`
   - `node --test tests/codex-runtime/*.test.mjs`
   - `git diff --check`
   - `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .`
   - `cargo fmt --check`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
2. Dispatch one clean-context whole-plan reviewer. Instruct it not to spawn subagents.
3. Remediate any findings and rerun validation/review until clean.
4. Start the next audit iteration with `cargo clean`, including the added signal-to-noise subagent.
