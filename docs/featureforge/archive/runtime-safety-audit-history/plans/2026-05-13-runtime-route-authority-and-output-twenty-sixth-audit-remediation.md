# Workflow State

Engineering Approved

# Plan Revision

Revision 1 - 2026-05-13

# Execution Mode

featureforge:executing-plans

# Goal

Remediate the twenty-sixth runtime safety audit findings by removing command synthesis outside route-owned executable surfaces, centralizing state-kind and stale-target source semantics, cleaning agent-facing wording that can send agents into old dead ends, and reducing public-flow/test signal noise without weakening runtime safety.

Source audit: `docs/featureforge/archive/runtime-safety-audit-history/2026-05-13-twenty-sixth-audit-report.md`

# Architecture

- `route_plan` owns public executable command choice. Mutator blocked outputs may copy the selected route command/template or expose diagnostic-only stop/requery metadata, but they must not synthesize normal-path mutation commands from `required_follow_up` strings.
- State-kind values and their semantic policy must be derived through a shared execution-owned classifier. Workflow doctor, blockers, invariants, and command eligibility may project that policy, not reimplement it with local string comparisons.
- Stale-target source tokens must be defined once on the stale-target source model. Execution reentry source projection, route follow-up projection, status bridge logic, and tests must reuse that token.
- Agent-facing docs and skills should point agents to operator JSON typed argv/template authority, not fixed command sequences or `next_action` guessing.
- Tests should preserve high-value public/private and boundary contracts while keeping behavioral public runtime proof clearly separated from static scanner self-tests.

# Change Surface

- `src/execution/commands/common/operator_outputs.rs`
- `src/execution/commands/common/unit_tests.rs`
- `src/execution/route_plan/state_kind.rs`
- `src/execution/route_plan/blockers.rs`
- `src/execution/route_plan/follow_up.rs`
- `src/execution/route_plan/unit_tests.rs`
- `src/execution/repair_target_selection.rs`
- `src/execution/stale_target_projection.rs`
- `src/execution/stale_target_selection.rs`
- `src/execution/status_support.rs`
- `src/execution/invariants.rs`
- `src/execution/command_eligibility.rs`
- `src/workflow/doctor_resolution.rs`
- `src/workflow/operator.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/runtime_authority_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/workflow_shell_smoke.rs`
- `scripts/run-public-runtime-flow-tests.sh`
- `skills/executing-plans/SKILL.md.tmpl`
- generated `skills/executing-plans/SKILL.md`
- `docs/featureforge/reference/2026-04-01-review-state-reference.md`
- generated schemas/goldens if public JSON/text contracts intentionally change

# Preconditions

- Do not use FeatureForge runtime/project skills.
- Do not let any review subagent spawn or request additional subagents.
- Use the requested Rust guidance when writing or refactoring Rust.
- Before every full test cycle, verify no `cargo nextest`, `cargo-nextest`, `nextest run`, `target/debug/deps/`, `cargo test`, `cargo clippy`, `cargo check`, or `rustc` process is already running.
- Before each new audit-loop iteration, run `cargo clean`.
- After each task implementation, run strict clippy and a full no-fail-fast nextest suite before dispatching review.
- If full nextest takes more than 4-5 minutes, run `cargo clean`, rerun the suite, and remediate if repeatable. If it exceeds 10 minutes, stop immediately and enter the clean/rerun/performance remediation path.
- Generated skill docs must be regenerated from templates.

# Known Footguns / Constraints

- Do not replace route-output fallback command synthesis with another local command picker under a new name.
- Do not turn diagnostic-only routes into workflow-operator requery loops. JSON requery is an orientation step; it is not mutation authority when no typed argv/template exists.
- Do not classify planning/fidelity/engineering-review reentry as external review waiting unless `external_wait_state` explicitly says the runtime is waiting for an external review result.
- Do not remove public route diagnostics or prompt-law tests solely to reduce test count. Reduce duplication by moving assertions to the owning suite or canonical helper.
- Do not weaken hidden/debug command scanners or public argv/template contracts.
- Do not add `#[allow(clippy::...)]`, weaken lint policy, or hide oversized logic behind test-only exceptions.
- Do not hand-edit generated skill docs without regenerating from `.tmpl`.

# Requirement Coverage Matrix

| Requirement | Task Coverage |
| --- | --- |
| Blocked mutator outputs only copy route-owned executable surfaces | Task 1 |
| Missing route executable surface is diagnostic-only or explicit requery, not synthesized mutation | Task 1 |
| Planning reentry is not mislabeled as external waiting | Task 2 |
| Doctor resolution consumes shared state-kind policy | Task 2 |
| Blocker rendering uses state-kind policy for waiting/planning/diagnostic text | Task 2 |
| State-kind string vocabulary is centralized enough to prevent drift | Task 2 |
| `closure_graph_stale_target` source token is owned by stale-target source model | Task 3 |
| Repair/status/follow-up consumers reuse stale-target source helpers | Task 3 |
| Direct cycle-break repair cleanup has regression coverage | Task 3 |
| Artifact-diagnostic-only route coverage exists on active helper paths | Task 3 |
| Agent-facing docs/skills route through typed operator argv/template and avoid fixed late-stage sequences | Task 4 |
| Text-mode diagnostics do not call JSON requery command authority | Task 4 |
| Public-flow script separates behavioral public CLI proof from scanner proof | Task 5 |
| Boundary tests avoid low-signal private helper pins while preserving ownership checks | Task 5 |

# Tasks

## Task 1 - Route-Owned Blocked Output Recovery

### Spec Coverage

- Blocked mutator outputs only copy route-owned executable surfaces.
- Missing route executable surface is diagnostic-only or explicit requery, not synthesized mutation.

### Goal

Remove the fallback path that constructs `repair-review-state`, `advance-late-stage`, or command templates from follow-up/profile data when workflow/operator does not expose a matching typed public command.

### Context

`public_recovery_contract_for_follow_up` first asks the selected operator route for a matching command, then calls `fallback_public_recovery_contract`. That fallback is a second decision surface. It can create an executable public command even when the route owner did not select one.

### Constraints

- External review waiting may still expose a workflow-operator external-ready requery because that is a query route, not a synthesized mutation command.
- Existing out-of-phase requery helpers may remain for explicit requery-required failures.
- Required input templates may be retained only when they come from the route-owned `recommended_public_command_template`; do not synthesize them from `PublicFollowUpInputProfile`.
- Public blocked outputs must stay explicit about why no argv/template is present.

### Done When

- `fallback_public_recovery_contract` is removed or reduced to diagnostic-only behavior that cannot emit mutation argv/templates.
- `public_recovery_contract_for_follow_up` returns executable surfaces only from `contract_from_matching_operator` or explicit workflow-operator requery for external review/result waiting.
- Unit tests prove a route mismatch does not synthesize `repair-review-state` or `advance-late-stage`.
- Existing blocked-output public command tests are updated to assert diagnostic-only or requery behavior where appropriate.
- Public route goldens are regenerated only when the public contract intentionally changes.

### Files

- `src/execution/commands/common/operator_outputs.rs`
- `src/execution/commands/common/unit_tests.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/runtime_behavior_golden.rs` and `tests/fixtures/runtime-goldens/public-runtime-routes.json` if needed

### Detailed Implementation Steps

1. Refactor `public_recovery_contract_for_follow_up` to:
   - return empty when no follow-up exists.
   - return external-ready workflow-operator requery only for `RequestExternalReview` / `WaitForExternalReviewResult`.
   - return `contract_from_matching_operator` for all other actionable follow-ups.
   - return diagnostic-only when no matching operator command/template exists.
2. Remove `fallback_public_recovery_contract`, `input_template_and_inputs_for_follow_up_profile`, and `command_for_follow_up_profile` if no longer needed.
3. Keep `PublicFollowUpInputProfile` only if callers still need it for external-review input metadata; otherwise remove or narrow it.
4. Update tests that expected fallback-synthesized commands so they assert:
   - no `recommended_public_command_argv`; when present elsewhere it remains the exact authoritative typed public route.
   - no command template unless route-owned.
   - `required_follow_up` remains available as diagnostic context where useful.
   - `rederive_via_workflow_operator` is only set for explicit requery.
5. Add a regression test with mismatched route command and `FOLLOW_UP_REPAIR_REVIEW_STATE` proving no repair command is synthesized.
6. Add a regression test with `FOLLOW_UP_ADVANCE_LATE_STAGE` and no route command proving no late-stage command/template is synthesized.

### Validation Expectations

- Targeted: `cargo test --lib execution::commands::common::unit_tests -- --nocapture`
- Targeted smoke/golden updates if public output changes.
- Full strict clippy.
- Full nextest no fail fast.
- Clean-context task review after full validation.

## Task 2 - Shared State-Kind Semantics And Doctor/Blocker Wording

### Spec Coverage

- Planning reentry is not mislabeled as external waiting.
- Doctor resolution consumes shared state-kind policy.
- Blocker rendering uses state-kind policy for waiting/planning/diagnostic text.
- State-kind string vocabulary is centralized enough to prevent drift.

### Goal

Make state-kind classification parseable and centrally owned, then use that policy in doctor resolution, blockers, invariants, and mutation eligibility so planning/review reentry cannot masquerade as external wait or terminal completion.

### Context

`classify_state_kind` currently returns string literals and treats `planning_reentry_required` without a command as `waiting_external_input`. `derive_doctor_resolution` separately knows only its local constants and can fall through to terminal for waiting-like non-external states.

### Constraints

- Preserve public JSON string values for existing schema compatibility unless adding a new value is necessary.
- External waiting must be driven by `external_wait_state=waiting_for_external_review_result`, not by planning phase detail.
- Runtime diagnostic states must not expose mutation commands.
- If a new state-kind value is added, update schemas, goldens, and tests explicitly.

### Done When

- Shared state-kind constants/helpers live in `src/execution/route_plan/state_kind.rs` and are used by doctor, blockers, invariants, and command eligibility.
- `planning_reentry_required` without a route command is classified as runtime diagnostic or another non-external non-terminal state; it is not `waiting_external_input`.
- Blocker details for planning reentry do not mention external review result waiting.
- Doctor resolution for waiting-like states without `external_wait_state` is not terminal.
- Tests cover external wait, planning reentry without command, blocked runtime bug, runtime reconcile, and terminal finish completion.

### Files

- `src/execution/route_plan/state_kind.rs`
- `src/execution/route_plan/blockers.rs`
- `src/execution/route_plan/unit_tests.rs`
- `src/workflow/doctor_resolution.rs`
- `src/execution/invariants.rs`
- `src/execution/command_eligibility.rs`
- schemas and route goldens if state-kind values or text change

### Detailed Implementation Steps

1. Introduce shared constants or a small `RouteStateKind` enum/newtype with:
   - `ACTIONABLE_PUBLIC_COMMAND`
   - `WAITING_EXTERNAL_INPUT`
   - `TERMINAL`
   - diagnostic detail values for `blocked_runtime_bug` and `runtime_reconcile_required`
   - helper predicates such as `is_terminal`, `is_external_wait`, `is_diagnostic`, and `blocks_local_mutation`.
2. Update `PUBLIC_STATE_KIND_VALUES` to reference shared constants.
3. Change `classify_state_kind` so only explicit external wait state returns `waiting_external_input`.
4. Route `planning_reentry_required` with no command to the appropriate non-terminal diagnostic classification.
5. Update `primary_blocker_for_source` to render external wait text only for actual external waiting. Add planning/reentry blocker text that tells agents to follow typed route output or report the route diagnostic rather than wait for an external result.
6. Update `derive_doctor_resolution` to call shared predicates instead of local string classification.
7. Update mutation eligibility and invariant checks to call shared predicates.
8. Update or add tests for the changed classification and doctor behavior.
9. Regenerate schemas/goldens if the public JSON/text changes.

### Validation Expectations

- Targeted: `cargo test --lib execution::route_plan::unit_tests workflow::doctor_resolution::tests -- --nocapture` or closest supported commands.
- Targeted: route golden test if JSON changes.
- Full strict clippy.
- Full nextest no fail fast.
- Clean-context task review after full validation.

## Task 3 - Central Stale-Target Source Tokens And Residual Runtime Coverage

### Spec Coverage

- `closure_graph_stale_target` source token is owned by stale-target source model.
- Repair/status/follow-up consumers reuse stale-target source helpers.
- Direct cycle-break repair cleanup has regression coverage.
- Artifact-diagnostic-only route coverage exists on active helper paths.

### Goal

Remove duplicated stale-target source strings and add focused regression coverage for the remaining low-risk runtime authority gaps.

### Context

`closure_graph_stale_target` appears as a fallback reason/source string in stale projection, repair target selection, route follow-up projection, status support, and tests. The enum `AuthoritativeStaleTargetSource` should own any source-token translation needed by downstream consumers.

### Constraints

- Do not change public source token output unless intentionally updating goldens/tests.
- Preserve stale-target ordering and task-closure bridge semantics.
- Coverage should exercise active runtime/helper paths, not only source scans.

### Done When

- `closure_graph_stale_target` is represented by one constant/helper.
- `ExecutionReentryTargetSource::ClosureGraphStaleTarget`, `execution_reentry_target_source_for_route`, status bridge logic, and tests reuse that helper.
- Fallback stale reason-code construction uses a named constant rather than a raw string.
- A direct `repair-review-state` regression proves resolved cycle-break cleanup action and strategy fields clear.
- Artifact/projection diagnostic-only coverage includes at least one active repair/status/follow-up helper path beyond the current source-scan allowlist.

### Files

- `src/execution/stale_target_projection.rs`
- `src/execution/stale_target_selection.rs`
- `src/execution/repair_target_selection.rs`
- `src/execution/route_plan/follow_up.rs`
- `src/execution/status_support.rs`
- `tests/public_replay_churn.rs`
- `tests/runtime_authority_contracts.rs`
- related unit tests

### Detailed Implementation Steps

1. Add a public-in-crate stale source token constant or method on `AuthoritativeStaleTargetSource`, for example `execution_reentry_source_token`.
2. Replace raw `closure_graph_stale_target` strings with the shared token.
3. Replace fallback stale-target reason strings with a named constant when the value is semantically a fallback reason rather than an enum source.
4. Add or update tests proving route/status still emit the same token.
5. Add a direct repair-review-state cycle-break cleanup regression:
   - construct or reuse a fixture with resolved cycle-break strategy state.
   - run public `repair-review-state`.
   - assert the cleanup action includes the cycle-break-cleared action and strategy cycle-break fields are clear.
6. Add targeted artifact/projection diagnostic-only coverage for an active routing path not currently covered by the scanner-only contract.

### Validation Expectations

- Targeted stale/repair tests.
- Targeted authority contracts.
- Full strict clippy.
- Full nextest no fail fast.
- Clean-context task review after full validation.

## Task 4 - Agent-Facing Route Authority Wording

### Spec Coverage

- Agent-facing docs/skills route through typed operator argv/template and avoid fixed late-stage sequences.
- Text-mode diagnostics do not call JSON requery command authority.

### Goal

Remove wording that encourages fixed late-stage choreography, `next_action` guessing, or repeated JSON requery loops when no typed executable route exists.

### Context

`executing-plans` opens with a fixed late-stage sequence before the operator route rule. The review-state reference says agents can satisfy `next_action` when no argv/template exists. Operator text-mode diagnostics label JSON requery as command execution authority even on diagnostic-only routes.

### Constraints

- Keep mandatory public route law top-level in route-owning skills.
- Prefer one canonical reference over repeated negative rules.
- Keep skills within budget; added text should replace weaker text.
- Do not hand-edit generated skill docs without template regeneration.

### Done When

- `skills/executing-plans/SKILL.md.tmpl` opens with operator-selected route execution instead of a fixed document-release/final-review sequence.
- Generated `skills/executing-plans/SKILL.md` is fresh.
- Review-state reference says no argv/template means stop and report the route diagnostic unless the route owner exposes a bindable template.
- Operator text distinguishes diagnostic JSON orientation from command execution authority.
- Tests asserting text output and skill/doc contracts are updated without weakening required route law.

### Files

- `skills/executing-plans/SKILL.md.tmpl`
- generated `skills/executing-plans/SKILL.md`
- `docs/featureforge/reference/2026-04-01-review-state-reference.md`
- `src/workflow/operator.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs` if needed
- `skills/skill-doc-budgets.json` if budget changes require it

### Detailed Implementation Steps

1. Replace the fixed late-stage overview sentence in `executing-plans` with a route-owned description: load plan, query operator, execute each returned typed argv/template, and stop on missing executable route diagnostics.
2. Keep late-stage examples in companion references rather than repeating sequence law in the top-level skill.
3. Update review-state reference wording so `next_action` is diagnostic unless a typed argv/template route exists.
4. Change text-mode operator diagnostic guidance:
   - use `Command execution authority` only when argv/template command execution exists.
   - use diagnostic/orientation language for JSON requery in diagnostic routes.
5. Update text-output tests accordingly.
6. Regenerate skill docs and run node doc contract checks.

### Validation Expectations

- `node scripts/gen-skill-docs.mjs`
- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- Targeted workflow shell smoke text tests.
- Full strict clippy.
- Full nextest no fail fast.
- Clean-context task review after full validation.

## Task 5 - Public-Flow And Boundary Test Signal Cleanup

### Spec Coverage

- Public-flow script separates behavioral public CLI proof from scanner proof.
- Boundary tests avoid low-signal private helper pins while preserving ownership checks.

### Goal

Keep the suite protective while reducing self-referential churn. Public runtime proof should be labeled as compiled/public behavior. Static scanner and module-boundary tests should stay focused on durable boundaries rather than private helper shape.

### Context

The public-flow script runs `public_cli_flow_contracts`, whose first tests are scanner self-tests. Boundary tests also include detailed route-plan child ownership and broad literal allowlists that protect real drift but can become noisy.

### Constraints

- Do not remove hidden-command, internal-helper, display-command, or public argv/template guards.
- Do not weaken import-boundary or module-size checks.
- Prefer moving scanner self-tests to `public_flow_scan_contracts` over deleting them.
- Keep route golden coverage for externally visible JSON behavior.

### Done When

- Public-flow script comments and selected suites clearly distinguish behavioral compiled CLI proof from static scanner support.
- Scanner self-tests in `tests/public_cli_flow_contracts.rs` are moved to `tests/public_flow_scan_contracts.rs` or the script/gate is renamed/documented so evidence is not overstated.
- Runtime boundary tests are adjusted only where they pin private helper implementation shape; semantic ownership checks remain.
- Docs/testing explains the separation succinctly.
- All affected scanner/boundary/public-flow tests pass.

### Files

- `scripts/run-public-runtime-flow-tests.sh`
- `tests/public_cli_flow_contracts.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `docs/testing.md`

### Detailed Implementation Steps

1. Move scanner vocabulary self-tests from `public_cli_flow_contracts.rs` to `public_flow_scan_contracts.rs` when the test does not execute compiled public CLI behavior.
2. Remove unused scanner imports from `public_cli_flow_contracts.rs`.
3. Ensure `scripts/run-public-runtime-flow-tests.sh` either:
   - runs only behavioral public CLI suites, or
   - explicitly names scanner suites as static support instead of shipped-runtime proof.
4. Review `runtime_module_boundaries.rs` findings from the audit and remove only low-signal private helper-name pins that do not represent a boundary contract.
5. Update `docs/testing.md` with the behavioral-vs-static gate distinction.
6. Run targeted public-flow and boundary tests.

### Validation Expectations

- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo test --test public_flow_scan_contracts -- --nocapture`
- `cargo test --test runtime_module_boundaries -- --nocapture`
- `scripts/run-public-runtime-flow-tests.sh`
- Full strict clippy.
- Full nextest no fail fast.
- Clean-context task review after full validation.

# Final Whole-Plan Validation

After all tasks are complete:

1. Verify no full test cycle is already running.
2. Run strict clippy.
3. Run full no-fail-fast nextest.
4. If runtime or generated docs changed, run:
   - `node scripts/gen-skill-docs.mjs --check`
   - `node scripts/gen-agent-docs.mjs --check`
   - `node --test tests/codex-runtime/*.test.mjs`
5. Dispatch a clean-context whole-plan reviewer with no permission to spawn subagents.
6. Remediate, revalidate, and rereview until no actionable issues remain.
7. Start the next clean audit-loop iteration with `cargo clean` and the full A-I audit subagent set.
