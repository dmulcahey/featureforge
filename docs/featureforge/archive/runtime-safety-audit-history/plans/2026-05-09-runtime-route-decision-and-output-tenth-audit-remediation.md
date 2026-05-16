# FeatureForge Runtime Route-Decision And Output Tenth-Audit Remediation

## Workflow State

Engineering Approved

## Plan Revision

1

## Execution Mode

Implementation

## Goal

Remediate the actionable findings from the tenth A-H audit:

- make route decision construction a route-plan-owned boundary instead of a router-owned semantic surface
- centralize route-to-status projection so router and read-model projection cannot drift
- remove helper-shaped recovery wording from active execution prompts
- make handoff follow-up output and schema text point agents to public `transfer`, not a retired handoff recorder command shape
- make low-level `blocked_runtime_bug` diagnostics explicitly stop/report oriented

The end state is a runtime where `router` projects selected route decisions but does not own route constructors, route-to-status mapping is shared, active prompts never suggest helper mutation recovery paths, and diagnostic text cannot lead an agent into retired command names or artifact reconstruction.

## Architecture

The runtime keeps this authoritative flow:

CLI args -> command module -> transition guard -> event append -> reducer -> route-plan selection -> route/status projection -> workflow operator presentation.

This plan tightens the remaining boundaries:

- `route_plan` owns route decision data types, route constructors, and route decision helper semantics.
- `router` may keep orchestration and DTO assembly, but must import route decision objects from `route_plan` instead of exporting them to `route_plan`.
- status projection from a selected route decision must have one shared helper for common fields and phase-to-harness mapping.
- public output and active skills must speak in public-route terms: `recommended_public_command_argv`, `recommended_public_command_template`, `transfer`, and stop/report diagnostics.
- `record_handoff` may remain an internal/follow-up intent token only where required for compatibility, but active user-facing text must not make it look like a command to run.

## Change Surface

- `src/execution/router.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/**`
- possible new `src/execution/route_plan/decision.rs`
- possible new `src/execution/route_plan/status_application.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/command_eligibility.rs`
- `src/execution/review_state.rs`
- `src/execution/event_log.rs`
- `src/execution/migration.rs`
- `src/execution/status.rs`
- `schemas/plan-execution-status.schema.json`
- `schemas/workflow-handoff.schema.json`
- `schemas/workflow-operator.schema.json`
- `skills/executing-plans/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- `skills/using-featureforge/SKILL.md.tmpl` if needed for consistency
- generated `skills/**/SKILL.md`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/runtime_module_boundaries.rs`
- `tests/runtime_instruction_contracts.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- schema/signature tests as needed

## Preconditions

- Do not use FeatureForge runtime skills or project skills.
- Do not weaken public route guards or hidden-helper scanners.
- Do not run FeatureForge runtime/workflow commands as a workflow driver.
- Preserve public CLI compatibility.
- Preserve `record_handoff` as a follow-up token only if removing it would break schema compatibility; make it clearly non-command in active prose/schema descriptions.
- Generated skill docs and schemas must be regenerated from sources when their sources change.
- Historical docs may remain historical, but active docs/prompts must not teach stale helpers as normal flow.

## Known Footguns / Constraints

- `recommended_public_command_argv` is the authoritative executable public argv when present; otherwise bind `recommended_public_command_template` into completed argv. `recommended_command` is display-only compatibility text; do not parse or execute `recommended_command`.
- Moving route decision types must not create a cycle where `route_plan` imports `router`.
- Do not replace `router` with another catch-all. Route decision construction should move into a focused `route_plan` child module; projection helpers should stay focused.
- Route-to-status projection has pre-final and final-status differences. Centralize common projection and make deltas explicit instead of forcing both call sites through an overbroad helper.
- `record_handoff` appears in historical docs and internal compatibility paths. The target is active public output, active schemas, and active prompt/docs, not archive rewrites.
- `BlockedRuntimeBug` diagnostics must not suggest mutation, reconstruction, or repair commands.

## Requirement Coverage Matrix

| Requirement | Task 1 | Task 2 | Task 3 | Task 4 |
| --- | --- | --- | --- | --- |
| Route decision type and constructors are route-plan-owned | x |  |  | x |
| Router is projection/orchestration, not route semantic owner | x | x |  | x |
| Route-to-status mapping has one shared helper |  | x |  | x |
| Active prompts avoid helper-shaped recovery wording |  |  | x | x |
| Handoff follow-up output points to public transfer |  |  | x | x |
| `blocked_runtime_bug` text says stop/report |  |  | x | x |
| Boundary/static tests catch regressions | x | x | x | x |
| Full validation and clean-context review loop | x | x | x | x |

## Tasks

### Task 1: Move Route Decision Construction Under Route-Plan Ownership

#### Spec Coverage

- Modularization G-P2: route ownership must not remain split between `route_plan` and `router`.
- Runtime flow: route-plan selection owns route semantics before router/read-model presentation.

#### Goal

Move `RouteDecision` / `PublicRouteDecision`, route constructor helpers, and route-decision semantic helper functions out of `router.rs` into a focused route-plan-owned module.

#### Context

The tenth audit found that `route_plan.rs` imports route decision types and constructors from `router.rs`. That leaves router deciding route semantics even though route-plan owns ordering. The specific constructor helpers called out were:

- `close_current_task_route_decision`
- `repair_review_state_route_decision`
- `runtime_reconcile_route_decision`
- `branch_closure_recording_route_decision`

Nearby helper functions such as route blocking, state-kind classification, required-follow-up derivation, blocker materialization, and public-command synthesis should move with the route decision type when they are semantic route-decision helpers.

#### Constraints

- Do not change route behavior.
- Do not make `route_plan` import `router`.
- Keep the new module under focused line caps.
- Preserve existing `router` tests by moving or rehoming tests with the helpers they cover.
- Keep `router` importing route decision types/helpers from `route_plan`, not the reverse.

#### Done when

- `src/execution/route_plan.rs` no longer imports from `crate::execution::router`.
- `RouteDecision`, `PublicRouteDecision`, `NextPublicAction`, `Blocker`, and semantic route constructor/helper functions live in `src/execution/route_plan/decision.rs` or another route-plan-owned child module.
- `router.rs` imports route decision objects from `route_plan` and retains only projection/orchestration helpers.
- Boundary tests fail if `route_plan` imports `router` or if route constructor helpers are reintroduced in `router`.
- Existing public/runtime behavior tests still pass.

#### Files

- `src/execution/route_plan.rs`
- new or updated `src/execution/route_plan/decision.rs`
- `src/execution/route_plan/status_projection.rs`
- `src/execution/route_plan/final_review_dispatch.rs`
- `src/execution/route_plan/repair_follow_up_binding.rs`
- `src/execution/router.rs`
- `src/execution/read_model/public_route_projection.rs`
- `tests/runtime_module_boundaries.rs`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`

#### Implementation Steps

1. Create a route-plan child module for route decision data and constructors.
2. Move the route decision structs and their methods out of `router.rs`.
3. Move route constructor helpers and semantic helper functions needed by `route_plan` out of `router.rs`.
4. Update `route_plan`, `router`, read-model projection, workflow/operator, and tests to import route decision objects from the route-plan module.
5. Move helper unit tests from `router.rs` to the new module, or update imports if keeping them in `router` is still projection-only.
6. Update module-boundary docs and line-cap tables for the new child module.
7. Add or strengthen boundary tests that reject `crate::execution::router` imports from `route_plan` and reject route constructor definitions in `router.rs`.

#### Validation Expectations

- `cargo fmt --check`
- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo test --test runtime_authority_contracts -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- `cargo test --test liveness_model_checker -- --nocapture`

### Task 2: Centralize Route-To-Status Projection

#### Spec Coverage

- Modularization G-P2: route/status/read-model projection must consume one decision object without duplicated semantic mapping.
- Public-output H-P1: public status/operator fields must remain aligned.

#### Goal

Extract common route-to-`PlanExecutionStatus` projection into one helper used by both final router projection and read-model public route projection.

#### Context

The tenth audit found duplicated projection code:

- `router.rs::project_route_decision_for_status_blocker_authority`
- `read_model/public_route_projection.rs::project_routing_decision_onto_status`

Both assign route fields and duplicate phase-to-`HarnessPhase` mapping. Their differences are real: final read-model projection includes `state_kind`, `next_public_action`, `blockers`, `execution_reentry_target_source`, and `public_repair_targets`, while pre-final projection should only update the fields needed before status blocker/finalization.

#### Constraints

- Do not collapse pre-final and final projections into an overbroad boolean maze.
- Centralize phase-to-harness mapping and common route field assignment.
- Keep public repair targets and final-only fields final-only.
- Keep task-boundary diagnostic application in one clearly owned place.

#### Done when

- Phase-to-harness mapping exists in one helper.
- Common route/status field assignment exists in one helper.
- Router and read-model projection call the shared helper and specify only their local deltas.
- Boundary tests reject local duplicate phase-to-harness mapping blocks in `router.rs` and `read_model/public_route_projection.rs`.

#### Files

- `src/execution/route_plan/status_application.rs` or equivalent
- `src/execution/router.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/route_plan.rs`
- `tests/runtime_module_boundaries.rs`
- schema/read-model tests as needed

#### Implementation Steps

1. Introduce a focused projection helper with a small input struct that accepts mutable `PlanExecutionStatus`, `ExecutionRoutingState`, and `RouteDecision`.
2. Put the shared phase-to-harness mapping in that helper.
3. Put common field assignment in that helper: phase, phase detail, review status, recording context, execution command context, next action, public command, argv, template, required inputs, display command, blocking task/scope/external wait/blocking reason codes.
4. Let router pre-final projection call the helper, then apply targetless reconcile and task-boundary diagnostics as currently required before blocker finalization.
5. Let read-model final projection call the helper, then apply final-only fields: `state_kind`, `next_public_action`, `blockers`, `execution_reentry_target_source`, `public_repair_targets`, and projection/warning deltas.
6. Add boundary tests that reject duplicated `match route_decision.phase.as_str()` mapping in router/read-model projection modules.

#### Validation Expectations

- `cargo fmt --check`
- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test workflow_entry_shell_smoke -- --nocapture`
- `cargo test --test workflow_runtime -- --nocapture`
- `cargo test --test public_replay_churn -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- `cargo test --test liveness_model_checker -- --nocapture`

### Task 3: Clean Public Output And Prompt Wording

#### Spec Coverage

- Public-output H-P1/H-P2: failures and prompts must route through public argv/template or stop/report diagnostics.
- Prompt surface F: active prompts must not teach hidden helper or helper-shaped mutation recovery.
- Public CLI / reachability: public transfer must be the route for handoff progression.

#### Goal

Remove helper-shaped recovery wording, make handoff follow-up text point to public `transfer`, and make `blocked_runtime_bug` diagnostics explicitly stop/report oriented.

#### Context

The tenth audit found:

- Dirty-before-begin guidance says "helper-backed route" and "authoritative helper mutations" in `executing-plans` and `subagent-driven-development`.
- Repair-review-state follow-up text says "record a handoff" even though public execution goes through `plan execution transfer`.
- `record_handoff` is exposed in schema as a required follow-up token but lacks enough description to tell callers it is not a command.
- Several low-level `blocked_runtime_bug` messages in event-log migration/parity code are diagnostic-only but do not explicitly say to stop/report.

#### Constraints

- Preserve `record_handoff` token compatibility unless schema migration is explicitly required by tests.
- Do not add hidden commands or compatibility instructions.
- Edit `.tmpl` skill sources and regenerate generated docs.
- Regenerate schemas if schema descriptions or enum metadata change.
- Keep historical docs historical; active docs/prompts/tests must be clean.

#### Done when

- Active execution skill templates and generated docs no longer contain "helper-backed route" or "authoritative helper mutations" in current workflow guidance.
- Dirty-before-begin recovery tells agents to query workflow/operator JSON, follow typed argv/template if present, and stop/report if no public route exists.
- Repair-review-state handoff follow-up text says to follow the public `transfer` route.
- Schema/command-kind descriptions make `record_handoff` a follow-up intent token, not an executable command name, or use public transfer terminology where appropriate.
- `BlockedRuntimeBug` event-log/migration messages include stop/report wording and no repair/reconstruct command guidance.
- Static tests reject the stale phrases.

#### Files

- `skills/executing-plans/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- generated `skills/executing-plans/SKILL.md`
- generated `skills/subagent-driven-development/SKILL.md`
- `src/execution/review_state.rs`
- `src/execution/command_eligibility.rs`
- `src/execution/event_log.rs`
- `src/execution/migration.rs`
- `src/execution/status.rs`
- generated schemas
- `tests/runtime_instruction_contracts.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/packet_and_schema.rs`

#### Implementation Steps

1. Replace helper-shaped dirty-before-begin wording in templates with public route authority wording.
2. Regenerate skill docs.
3. Update repair-review-state handoff follow-up text to name public `transfer`.
4. Update public command template/command-kind schema descriptions so `record_handoff` is clearly a follow-up intent token or transfer intent, not a command.
5. Add stop/report language to blocked runtime bug messages in event-log migration/parity code.
6. Add denied-string tests for active docs/prompts covering "helper-backed route" and "authoritative helper mutations".
7. Add public-output tests for handoff follow-up text and blocked-runtime-bug diagnostic wording.
8. Regenerate schemas and update schema/golden tests if needed.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo test --test runtime_instruction_contracts -- --nocapture`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo test --test packet_and_schema -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- `cargo test --test liveness_model_checker -- --nocapture`

### Task 4: Final Cross-Audit Verification And Regression Lockdown

#### Spec Coverage

- Full validation and clean-context review loop.
- Audit loop requirement: no actionable issues remain before stopping.

#### Goal

Run final verification, dispatch clean-context review of the complete remediation plan, remediate any issues, then run the full A-H audit loop again.

#### Context

Prior remediation loops only count as complete when strict clippy, full nextest no-fail-fast, standalone liveness, and clean-context review/audit agree there are no actionable findings.

#### Constraints

- Do not use FeatureForge runtime skills or project skills.
- Do not allow subagents to spawn subagents.
- Always run strict clippy and full nextest before dispatching review.
- If review or audit finds actionable issues, create the next plan and continue the loop.

#### Done when

- Full validation passes after Tasks 1-3.
- Clean-context whole-plan review finds no actionable issues.
- Fresh A-H audit finds no actionable issues, or any remaining issues are captured in the next remediation plan.

#### Files

- All files touched by Tasks 1-3.
- `docs/featureforge/reference/2026-05-09-deep-runtime-safety-tenth-audit.md`
- this plan file.

#### Implementation Steps

1. Run generated-doc checks and Node contract suite.
2. Run `cargo fmt --check`.
3. Run strict clippy.
4. Run full no-fail-fast nextest.
5. Run standalone liveness checker.
6. Dispatch a clean-context whole-plan reviewer with no skills and no subagent delegation.
7. Remediate and revalidate until review is clean.
8. Dispatch fresh A-H auditors with no skills and no subagent delegation.
9. If auditors find actionable issues, write the next plan and continue.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- `cargo test --test liveness_model_checker -- --nocapture`
