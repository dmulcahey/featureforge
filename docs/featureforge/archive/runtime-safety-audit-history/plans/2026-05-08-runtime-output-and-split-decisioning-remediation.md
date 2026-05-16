# Runtime output and split-decisioning remediation

## Workflow State

Draft remediation plan for the actionable findings from the third deep runtime safety audit.

## Plan Revision

1

## Execution Mode

Sequential implementation with a full verification and clean-context review gate after each task.

Each task must pass:

- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

If the task touches generated docs, schemas, or prompt surfaces, also run the relevant Node/doc generation checks before the review gate.

Review subagents must start from clean context, must not run FeatureForge runtime/project skills, and must not spawn additional subagents.

## Goal

Eliminate the remaining public-output traps and duplicated runtime semantic predicates found by the third audit so agents receive one public next step, fail-closed diagnostics stay diagnostic-only, stale-target repair decisions share one implementation, and public phase-detail vocabulary remains centrally enforceable.

## Architecture

The remediation keeps the existing runtime architecture:

- CLI args enter command modules.
- Command modules ask typed mutation/route authorities for eligibility.
- Events append to the runtime event log.
- Reducer/read model derive current truth.
- Router/next-action/shared public-route selection select the public route.
- Workflow operator/doctor/status render the route without inventing write semantics.

The changes centralize small semantic predicates and display vocabulary rather than adding a new workflow lane.

## Change Surface

- `src/execution/phase.rs`
- `src/execution/next_action.rs`
- `src/execution/public_route_selection.rs`
- `src/execution/repair_target_selection.rs`
- `src/execution/repair_route_decision.rs`
- `src/execution/stale_target_projection.rs`
- `src/execution/router.rs`
- `src/execution/status.rs`
- `src/workflow/operator.rs`
- `src/workflow/doctor_dashboard.rs`
- `schemas/*.json`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- generated goldens under `tests/fixtures/runtime-goldens/`
- active docs/skills only if schema/doc generation requires synchronized wording

## Preconditions

- Start from the implemented snapshot audited as `0cca79dd59054865a7c6a2d34bd90b21bb660673` or the current working tree that contains equivalent implementation changes.
- Do not revert unrelated dirty worktree changes.
- Do not use FeatureForge runtime/project skills.
- Use Rust guidance when editing Rust.

## Known Footguns / Constraints

- `recommended_public_command_argv` is executable authority. `recommended_command` and `next_action` are display/projection fields only.
- `blocked_runtime_bug` must not expose a normal-flow command, required input contract, or next-step text that implies continuation.
- `repair-review-state` is one public command. Public display text must not imply a second manual reentry action.
- Retired task-review dispatch detail is allowed only as a diagnostic detail if the runtime must describe a historical/broken state; it must still be centrally named.
- Do not reintroduce hidden helpers, low-level recorder commands, or manual artifact repair wording.
- If changing generated schema enums or public route goldens, regenerate artifacts with the repository scripts instead of hand-maintaining stale snapshots.

## Requirement Coverage Matrix

| Requirement | Covered by |
| --- | --- |
| Fail-closed runtime bug output is diagnostic-only | Task 1 |
| Public output gives one next action | Task 1 |
| Vague repair-routing wording removed | Task 1 |
| Stale-target bridge eligibility centralized | Task 2 |
| Authoritative stale-binding predicate centralized | Task 2 |
| Retired phase-detail vocabulary registered centrally | Task 2 |
| Static guards prevent regression | Tasks 1, 2 |
| Generated schemas/goldens/docs remain fresh | Tasks 1, 3 |
| Full validation and clean review loop completed | Tasks 1, 2, 3 |

## Ordered Tasks

### Task 1 - Public diagnostic output must expose one safe next step

#### Spec Coverage

- Public-output and agent-UX findings H-P1, H-P2, H-P3.
- Checklist items: runtime bug diagnostic-only output, single public next action, no vague hidden-helper-adjacent repair wording.

#### Goal

Make `blocked_runtime_bug` public doctor output diagnostic-only and replace the compound `repair review state / reenter execution` public display action with one single-action phrase.

#### Context

`blocked_runtime_bug` already suppresses mutations and public argv, but doctor rendering can still carry a phase-derived `next_step`. `next_action` still encodes two operations even when typed argv selects only `plan execution repair-review-state`.

#### Constraints

- Preserve typed argv behavior.
- Do not change public command eligibility.
- Do not add a new command.
- Keep backwards-incompatible schema changes explicit through generated schema/golden updates.

#### Done when

- `WorkflowDoctor` with `phase_detail=blocked_runtime_bug` renders diagnostic-only next-step text.
- `next_action` for repair-review-state routes is `repair review state`.
- No active public diagnostic text says `repair workflow routing`, `repairing runtime routing`, or `repair review state / reenter execution`.
- Static public-output tests reject the removed phrases.
- Targeted runtime tests cover `blocked_runtime_bug` next-step suppression and repair-review-state single-action display.
- Strict clippy and full nextest pass.
- Clean-context review reports no findings for Task 1.

#### Files

- `src/execution/next_action.rs`
- `src/execution/public_route_selection.rs`
- `src/execution/status.rs`
- `src/workflow/operator.rs`
- `src/workflow/doctor_dashboard.rs`
- `schemas/*.json`
- `tests/public_cli_flow_contracts.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/fixtures/runtime-goldens/*.json`

#### Implementation Steps

1. Add or reuse a single display constant for the repair-review-state public action, preferably close to public phase/action vocabulary.
2. Replace every production `next_action` assignment of `repair review state / reenter execution` with `repair review state`.
3. Update schema enums and Rust schema marker enums to the new display value.
4. Change `WorkflowDoctor` construction or `next_step_text` so `blocked_runtime_bug` returns diagnostic-only text and cannot borrow normal phase next-step text.
5. Replace `execution_reentry_target_missing` doctor blocker text with a stop/follow-operator-JSON diagnostic message.
6. Replace retired task-review dispatch text that says to repair runtime routing with a stop/follow typed operator diagnostic.
7. Add scanner tests for banned public-output phrases:
   - `repair review state / reenter execution`
   - `repair workflow routing`
   - `repairing runtime routing`
8. Update targeted workflow/runtime tests that assert the old compound phrase.
9. Regenerate public runtime route/schema goldens if the repository script owns them.
10. Run targeted tests for public diagnostics and shell smoke paths, then full clippy and full nextest.
11. Dispatch a clean-context reviewer for Task 1 and remediate any findings before continuing.

#### Validation Expectations

- `cargo test --test workflow_runtime blocked_runtime_bug -- --nocapture` or the nearest exact targeted blocked-runtime-bug tests pass.
- `cargo test --test workflow_shell_smoke -- --nocapture` targeted repair-review-state tests pass where practical.
- `cargo test --test public_cli_flow_contracts production_diagnostics_do_not_route_to_hidden_gates_or_receipt_repair -- --nocapture` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- `cargo nextest run --all-targets --all-features --no-fail-fast` passes.

### Task 2 - Centralize remaining split decision predicates and phase-detail vocabulary

#### Spec Coverage

- Modularization findings G-P2 and G-P3.
- Checklist items: phase/reason strings centralized, router/read-model/mutation guards share decision objects.

#### Goal

Remove duplicated stale-target bridge and stale-binding predicates, and make `task_review_dispatch_required` a centrally named phase-detail value with boundary tests that catch future unregistered active literals.

#### Context

The current predicates are equivalent but duplicated across `repair_target_selection`, `repair_route_decision`, and `stale_target_projection`. The retired task-review dispatch detail is active diagnostic vocabulary but is not registered in `phase.rs`.

#### Constraints

- Keep predicate behavior unchanged.
- Prefer borrowing and small helper inputs over cloning full status/projection objects.
- Do not move write-side logic into read-model or workflow modules.
- Boundary tests should detect local literal reintroduction, not just current constants.

#### Done when

- There is one implementation for "stale target allows task-closure bridge for task".
- Both next-action authority and repair-route decision consume that implementation.
- There is one helper for authoritative stale-binding detection shared by snapshot/projection forms.
- `task_review_dispatch_required` is declared in `src/execution/phase.rs`.
- Production code uses `phase::DETAIL_TASK_REVIEW_DISPATCH_REQUIRED`.
- Boundary/static tests fail if production code reintroduces local `task_review_dispatch_required` literals outside the central vocabulary and documented compatibility/test fixtures.
- Strict clippy and full nextest pass.
- Clean-context review reports no findings for Task 2.

#### Files

- `src/execution/phase.rs`
- `src/execution/repair_target_selection.rs`
- `src/execution/repair_route_decision.rs`
- `src/execution/stale_target_projection.rs`
- `src/execution/router.rs`
- `src/workflow/operator.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- targeted runtime/read-model tests as needed

#### Implementation Steps

1. Introduce a shared bridge-eligibility helper that accepts `Option<stale_task>`, `task_closure_bridge_allowed`, and target `task_number`, or a compact typed input struct.
2. Update `NextActionAuthorityInputs::stale_target_allows_task_closure_bridge_for_task` to delegate to the shared helper.
3. Update `repair_route_decision::stale_target_allows_task_closure_bridge` to delegate to the same helper.
4. Add a static boundary test that forbids local reimplementation of the bridge predicate in `repair_target_selection.rs` and `repair_route_decision.rs`.
5. Add a shared helper in `stale_target_projection.rs` for authoritative stale-binding detection over stale-target iterators plus drift flags.
6. Update `RuntimeGateSnapshot::has_authoritative_stale_binding` and `StaleTargetProjection::has_authoritative_stale_binding` to call the shared helper.
7. Add `DETAIL_TASK_REVIEW_DISPATCH_REQUIRED` to `src/execution/phase.rs` and classify it as retired/diagnostic phase-detail vocabulary.
8. Replace production literals with `phase::DETAIL_TASK_REVIEW_DISPATCH_REQUIRED`.
9. Expand boundary tests so active production Rust cannot introduce unregistered `*_required` phase-detail literals.
10. Run targeted boundary/module tests, then full clippy and full nextest.
11. Dispatch a clean-context reviewer for Task 2 and remediate any findings before continuing.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture` passes.
- `cargo test --test public_cli_flow_contracts -- --nocapture` passes or targeted public-output scanner tests pass before the full suite.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- `cargo nextest run --all-targets --all-features --no-fail-fast` passes.

### Task 3 - Regenerate public artifacts and run final clean audit closure

#### Spec Coverage

- Required validation and review loop for the whole plan.
- Ensures schema/golden/generated prompt surfaces match the implementation.

#### Goal

Ensure every generated public artifact reflects Tasks 1 and 2, then run final validation and a clean-context full-plan review/audit closure.

#### Context

Changing public display enum values and phase-detail vocabulary can affect schema signatures, runtime route goldens, docs, and prompt-surface scanners.

#### Constraints

- Do not hand-edit generated skill docs when a template owns the text.
- Do not leave validation generated files stale.
- Do not allow review subagents to spawn.

#### Done when

- Public schema and runtime route goldens are fresh.
- Generated skills/agents are fresh if touched.
- Node prompt/doc tests pass if any prompt/doc surfaces changed.
- Strict clippy and full nextest pass.
- A clean-context full-plan reviewer finds no actionable issues.
- A final deep-audit pass reports no actionable findings or all findings are converted into the next remediation plan.

#### Files

- `schemas/*.json`
- `tests/fixtures/runtime-goldens/*.json`
- generated skill/agent docs if scripts update them
- `docs/featureforge/reference/2026-05-08-deep-runtime-safety-third-audit.md`
- this plan file

#### Implementation Steps

1. Run repository generation/check scripts for schemas/goldens/docs used by the changed surfaces.
2. Inspect diffs for accidental prompt-surface bloat or stale hidden-helper wording.
3. Run the required Node checks if generated docs/prompt surfaces changed.
4. Run strict clippy and full nextest.
5. Dispatch one clean-context reviewer against the complete plan and implementation.
6. If review finds issues, remediate, rerun validation, and rereview.
7. If no findings remain, run or dispatch the final audit process and stop only when no actionable audit issues remain.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check` passes if skill docs are touched or as final confidence.
- `node scripts/gen-agent-docs.mjs --check` passes if agent docs are touched or as final confidence.
- `node --test tests/codex-runtime/*.test.mjs` passes if prompt/docs/schema contracts are touched or as final confidence.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- `cargo nextest run --all-targets --all-features --no-fail-fast` passes.
- Final clean-context review reports no actionable findings.
