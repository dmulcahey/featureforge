# Runtime Boundary And Documentation Sixth-Audit Remediation Plan

## Workflow State

Engineering Approved

## Plan Revision

1

## Execution Mode

Sequential implementation. Complete each task in order. After each task, run strict clippy and the full nextest suite with no fail fast before clean-context review. Remediate review findings, re-run verification, and re-review until the task has no identified issues.

## Goal

Eliminate the remaining actionable sixth-audit issues:

- Route target selection must not depend on command eligibility.
- Public command constructors must have one owner for normal route/presentation construction.
- Execution-reentry target-source projection must be route-owned, not recomputed by the read model.
- Active public documentation scanners must cover release notes and stale retired-provenance vocabulary in active remediation docs.

## Architecture

Preserve the runtime flow:

CLI args -> command module -> public mutation guard -> transition/write authority -> event append -> reducer -> read model -> route decision -> workflow operator presentation.

This remediation keeps routing facts upstream of mutation eligibility and projection:

- `repair_target_selection` selects target candidates from read-model/status facts only.
- `command_eligibility` consumes typed public mutation requests and public route state after routing.
- Public command construction uses shared helper constructors in `next_action`.
- `RouteDecision` carries execution-reentry target-source metadata when routing has selected an execution-reentry lane.
- The read model copies route-owned metadata and may only project reducer stale-target diagnostics that are not already route-owned.
- Public doc scanners include release notes and active remediation plans so stale hidden-helper/retired-provenance vocabulary cannot drift back into agent-facing surfaces.

## Change Surface

- `src/execution/repair_target_selection.rs`
- `src/execution/command_eligibility.rs`
- `src/execution/next_action.rs`
- `src/execution/router.rs`
- `src/execution/commands/common/operator_outputs.rs`
- `src/execution/commands/transfer.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/query.rs`
- `src/execution/review_state.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `RELEASE-NOTES.md`
- Older active remediation plan/reference docs that still contain stale retired-provenance vocabulary.

## Preconditions

- Do not use FeatureForge runtime skills, project skills, or repo-local skills.
- Use Rust coding guidance when modifying Rust.
- Preserve event-log authority and guided routing.
- Do not weaken strict clippy or add lint suppressions.
- Do not replace public CLI coverage with internal helper-only tests.
- Generated skill docs must stay template-owned; edit templates and regenerate only if skill docs change.

## Known Footguns / Constraints

- Mutation eligibility is a guard, not target selection authority.
- Removing the eligibility call from target selection must not make blocked/runtime-diagnostic states actionable.
- Shared public command constructors must preserve exact `PublicCommand` fields and input-template behavior.
- `recommended_command` stays display-only; this plan must not reintroduce display-string execution.
- Historical release-note text may mention old commands only when clearly marked historical and guarded by scanner tests.
- Stale retired-provenance vocabulary in active remediation docs should be rewritten or moved into explicitly historical context; do not rewrite unrelated history.
- `execution_reentry_target_source` is diagnostic/projection metadata. Moving ownership to `RouteDecision` must not change routing output semantics except to remove duplicated computation.

## Requirement Coverage Matrix

| Requirement | Task |
| --- | --- |
| `repair_target_selection` no longer imports or calls command eligibility | Task 1 |
| Resume/exact route target selection remains behaviorally stable | Task 1 |
| Close-current-task and transfer-handoff public commands use shared constructors | Task 1 |
| Boundary tests reject future direct public command construction drift | Task 1 |
| `RouteDecision` carries execution-reentry target-source metadata | Task 2 |
| Read model copies route-owned target-source metadata instead of recomputing it | Task 2 |
| Boundary tests reject read-model calls back into repair target/route decisioning for route-owned target source | Task 2 |
| Release notes are covered by hidden-helper/retired-provenance public-doc scanners | Task 3 |
| Active remediation docs no longer contain stale retired plan-fidelity missing-artifact guidance | Task 3 |
| Full validation, prebuilts/provenance, and clean-context whole-plan review are complete | Task 4 |

## Ordered Tasks

### Task 1: Remove Route-Selection/Eligibility Feedback And Centralize Public Command Constructors

#### Spec Coverage

- `repair_target_selection` no longer imports or calls command eligibility.
- Resume/exact route target selection remains behaviorally stable.
- Close-current-task and transfer-handoff public commands use shared constructors.
- Boundary tests reject future direct public command construction drift.

#### Goal

Keep target selection as a read-only routing decision and keep normal public command construction owned by shared constructors.

#### Context

The sixth audit found `repair_target_selection.rs` importing `public_execution_mutation_is_authorized` and calling it during resume/exact target selection. It also found direct `PublicCommand::CloseCurrentTask` and `PublicCommand::TransferHandoff` construction in router/output modules even though `next_action.rs` already exposes shared constructors.

#### Constraints

- Do not remove mutation eligibility; move the semantic target-selection check out of the eligibility layer.
- Preserve current route output and mutation guard behavior.
- Do not add a generic string registry or display-command parser.
- Tests may destructure `PublicCommand` variants; the boundary should reject production construction, not legitimate enum pattern matches.

#### Done when

- `src/execution/repair_target_selection.rs` has no import or reference to `command_eligibility`.
- Target selection uses route-target/status predicates from read-model/state-owned helpers only.
- Router, transfer output, and operator output use `close_current_task_public_command` and `transfer_handoff_public_command`.
- Boundary tests reject direct production construction of `PublicCommand::CloseCurrentTask`, `PublicCommand::TransferHandoff`, `PublicCommand::Reopen`, and `PublicCommand::RepairReviewState` outside the shared owner.

#### Files

- `src/execution/repair_target_selection.rs`
- `src/execution/read_model/task_state.rs`
- `src/execution/next_action.rs`
- `src/execution/router.rs`
- `src/execution/commands/common/operator_outputs.rs`
- `src/execution/commands/transfer.rs`
- `tests/runtime_module_boundaries.rs`

#### Implementation Steps

1. Add a neutral route-target predicate near `ExecutionCommandRouteTarget`, for example `execution_command_route_target_matches_public_status(status, command_kind, task, step)`, that checks only read-model/status route facts.
2. Replace `public_execution_mutation_is_authorized` calls in `repair_target_selection.rs` with the neutral predicate or direct `ExecutionCommandRouteTarget` matching.
3. Remove the `command_eligibility` import from `repair_target_selection.rs`.
4. Replace direct `PublicCommand::CloseCurrentTask` construction in router and operator-output code with `close_current_task_public_command`.
5. Replace direct `PublicCommand::TransferHandoff` construction in router and transfer code with `transfer_handoff_public_command`.
6. Extend `reopen_and_repair_public_commands_have_shared_next_action_owner` into an all-normal-public-command construction boundary test.
7. Add a boundary assertion that `repair_target_selection.rs` does not depend on `command_eligibility`.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- Targeted routing tests as needed for resume/exact route selection.
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 2: Make Execution-Reentry Target Source Route-Owned

#### Spec Coverage

- `RouteDecision` carries execution-reentry target-source metadata.
- Read model copies route-owned target-source metadata instead of recomputing it.
- Boundary tests reject read-model calls back into repair target/route decisioning for route-owned target source.

#### Goal

Remove the read-model recomputation of execution-reentry target source and make routing the only owner of that semantic decision.

#### Context

`project_routing_decision_onto_status` currently calls `repair_follow_up_decision` after routing to derive `execution_reentry_target_source`. That duplicates route target semantics in the read model.

#### Constraints

- Preserve the current JSON/status field value for existing execution-reentry cases.
- Do not remove reducer stale-target diagnostics that are not route-owned.
- Do not make `RouteDecision` serialize internal-only source metadata unless the existing public schema intentionally exposes it through status.

#### Done when

- `RouteDecision` has an internal `execution_reentry_target_source` field or equivalent route-owned projection fact.
- Router fills that field for execution-reentry decisions using the same route authority inputs used to select the route.
- Read-model projection copies the field to status and no longer calls `repair_follow_up_decision` for this purpose.
- Boundary tests fail if `read_model/public_route_projection.rs` calls `repair_follow_up_decision` or `execution_reentry_target`.

#### Files

- `src/execution/router.rs`
- `src/execution/query.rs`
- `src/execution/read_model/public_route_projection.rs`
- `src/execution/review_state.rs`
- `tests/runtime_module_boundaries.rs`

#### Implementation Steps

1. Add an internal optional route field for execution-reentry target source.
2. Populate it in runtime route decisions when `phase_detail == execution_reentry_required`.
3. Ensure route decisions built from non-runtime routing or diagnostic routes set it to `None`.
4. Replace the read-model `repair_follow_up_decision` recomputation with a direct copy from `route_decision`.
5. Keep stale-target source projection as a fallback only when route-owned source is absent.
6. Add or update tests to prove the read model does not import/call route-target decision helpers for this source.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test execution_query -- --nocapture`
- `cargo test --test workflow_runtime -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 3: Cover Release Notes And Stale Retired-Provenance Vocabulary In Active Public-Doc Scans

#### Spec Coverage

- Release notes are covered by hidden-helper/retired-provenance public-doc scanners.
- Active remediation docs no longer contain stale retired plan-fidelity missing-artifact guidance.

#### Goal

Prevent public-facing docs from reintroducing old hidden-helper or retired-provenance mechanics while still allowing clearly historical release-note references.

#### Context

The sixth audit found release notes outside the active scanner set and stale retired plan-fidelity missing-artifact wording in older active remediation docs.

#### Constraints

- Do not rewrite unrelated historical release-note content.
- Historical mentions must be scoped as historical, not imperative next-step guidance.
- Active plan docs may keep historical audit context only when clearly marked as already remediated or historical.
- Scanner changes should fail on imperative hidden-helper guidance in `RELEASE-NOTES.md`.

#### Done when

- Active prompt/doc scanner coverage includes `RELEASE-NOTES.md`.
- Release-note hidden-helper/retired-provenance references are allowed only in explicitly historical contexts.
- Stale retired plan-fidelity missing-artifact wording is removed or rewritten as historical/resolved in active remediation docs.
- Tests fail on a synthetic release-note hidden-helper imperative fixture.

#### Files

- `RELEASE-NOTES.md`
- `docs/featureforge/plans/2026-05-07-runtime-safety-reaudit-remediation.md`
- `docs/featureforge/plans/2026-05-07-runtime-safety-reaudit-follow-up-remediation.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_instruction_contracts.rs`

#### Implementation Steps

1. Extend active doc/prompt file enumeration or a targeted scanner to include `RELEASE-NOTES.md`.
2. Add exceptions only for explicit historical release-note wording.
3. Rewrite or annotate stale retired plan-fidelity missing-artifact plan-doc references so they do not read as current pending work.
4. Add test coverage that would fail on imperative hidden-helper guidance in release notes.
5. Run Node doc contracts and targeted Rust public-flow/doc scanners.

#### Validation Expectations

- `node --test tests/codex-runtime/*.test.mjs`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo test --test runtime_instruction_contracts -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 4: Final Validation, Prebuilts, Review, And Audit Loop

#### Spec Coverage

- Full validation, prebuilts/provenance, and clean-context whole-plan review are complete.

#### Goal

Prove the targeted boundary/doc fixes did not regress runtime behavior, doc packaging, checked-in prebuilts, or liveness.

#### Context

This task closes the sixth audit remediation and prepares the next A-H audit pass.

#### Constraints

- Run full validation before review.
- Use a clean-context reviewer.
- Do not allow the reviewer to spawn subagents.
- Do not use FeatureForge/project skills.

#### Done when

- Generated skill and agent docs are fresh.
- Node contracts pass.
- Strict clippy passes.
- Full nextest no-fail-fast passes.
- Standalone liveness test passes.
- Checked-in prebuilts are refreshed if Rust changes changed binaries.
- Prebuilt provenance passes.
- Clean-context reviewer reports no findings against the whole plan.
- A follow-up A-H audit pass finds no actionable issues, or a new plan is produced and implemented.

#### Files

- `bin/featureforge`
- `bin/prebuilt/**`
- `bin/prebuilt/manifest.json`
- Any touched source/test/doc files.

#### Implementation Steps

1. Run `node scripts/gen-skill-docs.mjs --check`.
2. Run `node scripts/gen-agent-docs.mjs --check`.
3. Run `node --test tests/codex-runtime/*.test.mjs`.
4. Run `cargo clippy --all-targets --all-features -- -D warnings`.
5. Run `cargo nextest run --all-targets --all-features --no-fail-fast`.
6. Run `cargo test --test liveness_model_checker`.
7. Refresh checked-in prebuilts if Rust source changes require binary refresh.
8. Run `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .`.
9. Run denied-string scans for hidden helper guidance and stale retired-provenance/reentry phrases.
10. Take a synthetic snapshot and dispatch a clean-context review against this plan.
11. Remediate any findings and repeat validation/review until clean.
12. Re-run the A-H audit process.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`
- `cargo test --test liveness_model_checker`
- `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .`
- `git diff --check`
