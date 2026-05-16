# Runtime Prompt Boundary And Split-Decisioning Seventh-Audit Remediation Plan

## Workflow State

Engineering Approved

## Plan Revision

1

## Execution Mode

Sequential implementation. Complete each task in order. After each task, run strict clippy and the full nextest suite with no fail fast before clean-context review. Remediate review findings, re-run verification, and re-review until the task has no identified issues.

## Goal

Eliminate the actionable seventh-audit issues:

- Active prompt surfaces must not preserve removed helper command names as authoritative mutation boundaries.
- Tests must reject, not require, removed helper command vocabulary in active prompt/docs.
- Reducer construction must not hide read-model dependencies behind the broad `state.rs` facade.
- Task-closure baseline bridge routing must have one shared decision owner rather than separate local predicates in route presentation and next-action code.
- `repair_route_decision` must be documented and guarded so it does not become an uncapped catch-all.
- The residual active-contract serial unit-review proof behavior must be explicitly classified and guarded so legacy/plain proof artifacts remain diagnostic-only while intentional runtime-owned active-contract proof remains an explicit contract boundary.

## Architecture

Preserve the runtime flow:

CLI args -> command module -> public mutation guard -> transition/write authority -> event append -> reducer -> read model -> route decision -> workflow operator presentation.

This remediation separates vocabulary and routing ownership:

- Prompt docs refer to public runtime/operator routes and coordinator-owned runtime mutation boundaries without naming removed helper commands.
- Static tests deny removed helper command names in active prompt/docs and no longer require those names as proof of ownership boundaries.
- Reducer inputs come from explicit upstream projection modules, not from broad `state.rs` compatibility re-exports.
- Baseline-bridge route readiness and route task selection are owned by `repair_route_decision`.
- `public_route_selection` and `next_action` consume shared baseline-bridge decisions instead of rederiving status/reason-code predicates locally.
- `repair_route_decision` is either split by responsibility or documented with an explicit focused-module cap and sub-boundary expectations.
- Active-contract serial unit-review proof remains intentionally runtime-owned only where an authoritative active contract is present; legacy/plain unit-review proof artifacts remain diagnostic-only and cannot become normal public repair guidance.

## Change Surface

- `skills/executing-plans/SKILL.md.tmpl`
- `skills/executing-plans/SKILL.md`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md`
- `skills/subagent-driven-development/implementer-prompt.md`
- `tests/runtime_instruction_contracts.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `src/execution/reducer.rs`
- `src/execution/state.rs`
- `src/execution/read_model.rs`
- `src/execution/read_model/**`
- `src/execution/public_route_selection.rs`
- `src/execution/next_action.rs`
- `src/execution/repair_route_decision.rs`
- `src/execution/repair_route_decision/**` if split
- `src/execution/state/worktree_lease_truth.rs`
- `src/execution/state/unit_review_truth.rs`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/runtime_module_boundaries.rs`
- targeted runtime tests covering baseline bridge and active-contract unit-review proof

## Preconditions

- Do not use FeatureForge runtime skills, project skills, or repo-local skills.
- Use Rust coding guidance when modifying Rust.
- Preserve event-log authority and guided routing.
- Do not weaken strict clippy or add lint suppressions.
- Do not replace public CLI coverage with internal helper-only tests.
- Generated skill docs must stay template-owned. Edit `.tmpl` files and regenerate checked-in `SKILL.md` artifacts when a generated skill changes.
- Keep `recommended_command` display-only and `recommended_public_command_argv`/templates authoritative.

## Known Footguns / Constraints

- Do not remove current public commands such as `begin`, `complete`, `reopen`, `transfer`, `close-current-task`, or `advance-late-stage`; the prompt issue is removed helper vocabulary, not public command existence.
- Do not tell implementer subagents to run public runtime mutation commands. They should report back; the coordinator/runtime owner executes public routing/mutation commands.
- Do not rename or delete internal compatibility tests unless their behavior is intentionally replaced with an equivalent public/runtime guard.
- Moving functions out of `read_model.rs` must not create a second status derivation path.
- Moving baseline-bridge checks into `repair_route_decision` must preserve existing route outputs for cycle-break, stale-unreviewed, and missing-current-closure cases.
- `repair_route_decision` centralization is useful, but an uncapped broad module recreates the old monolith problem.
- Active-contract serial unit-review proof is not legacy plain proof fallback. Preserve the intended active-contract gate only if it is classified as runtime-owned contract proof and cannot be reached as manual prompt guidance.

## Requirement Coverage Matrix

| Requirement | Task |
| --- | --- |
| Active generated skill docs do not name removed helper commands as authoritative mutation boundaries | Task 1 |
| Implementer prompt does not name removed helper commands | Task 1 |
| Runtime instruction tests reject removed helper command names instead of requiring them | Task 1 |
| Active public doc scanners include removed helper command vocabulary | Task 1 |
| Reducer no longer imports read-model/status builders through `state.rs` compatibility re-exports | Task 2 |
| Boundary tests reject reducer dependence on `state.rs` read-model re-exports | Task 2 |
| Shared upstream projection module owns reducer-consumed truth derivation and blocking-record projection | Task 2 |
| Task-closure baseline bridge route task/readiness is owned by `repair_route_decision` | Task 3 |
| `public_route_selection` and `next_action` consume shared bridge decisions instead of local predicates | Task 3 |
| Boundary tests reject local baseline-bridge route predicates in public-route and next-action modules | Task 3 |
| `repair_route_decision` is documented and line-capped or split into focused child modules | Task 4 |
| Active-contract serial unit-review proof is explicitly classified and guarded as runtime-owned contract proof, while legacy/plain proof artifacts remain diagnostic-only | Task 4 |
| Full validation, prebuilts/provenance if needed, clean-context whole-plan review, and A-H audit loop are complete | Task 5 |

## Ordered Tasks

### Task 1: Remove Removed Helper Command Names From Active Prompt Surfaces And Invert Tests

#### Spec Coverage

- Active generated skill docs do not name removed helper commands as authoritative mutation boundaries.
- Implementer prompt does not name removed helper commands.
- Runtime instruction tests reject removed helper command names instead of requiring them.
- Active public doc scanners include removed helper command vocabulary.

#### Goal

Stop teaching agents stale helper command names while preserving the actual boundary: implementers/subagents produce candidate artifacts and report status; the coordinator/runtime owner executes public workflow/operator-guided mutations.

#### Context

The seventh public-output audit found active prompts still naming the retired contract/evaluation/handoff record-style helpers and the retired note-style helper as authoritative mutation boundaries. The wording prohibits direct invocation, but preserving removed command names in active prompts can still send agents looking for unavailable helpers. Tests currently require those names, which protects the stale wording.

#### Constraints

- Keep the candidate-artifact versus authoritative-runtime-mutation distinction.
- Public commands such as `begin`, `complete`, `reopen`, and `transfer` may be referenced only when they are current public commands and when the text makes coordinator/runtime ownership clear.
- Removed helper names must not remain in active generated skill docs or implementer prompts, even in prohibition wording.
- Do not hand-edit generated `SKILL.md` without editing the matching `.tmpl` and regenerating.

#### Done when

- `skills/executing-plans/SKILL.md.tmpl` and generated `SKILL.md` describe coordinator-owned public runtime mutations without retired contract/evaluation/handoff record-style helper names or the retired note-style helper name.
- `skills/subagent-driven-development/SKILL.md.tmpl`, generated `SKILL.md`, and `implementer-prompt.md` no longer contain those removed helper names.
- `tests/runtime_instruction_contracts.rs` no longer asserts that active docs mention removed helper names.
- `tests/runtime_instruction_contracts.rs`, `tests/public_cli_flow_contracts.rs`, and Node doc contract tests reject those removed helper names in active prompt/doc surfaces.
- Generated docs are fresh.

#### Files

- `skills/executing-plans/SKILL.md.tmpl`
- `skills/executing-plans/SKILL.md`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md`
- `skills/subagent-driven-development/implementer-prompt.md`
- `tests/runtime_instruction_contracts.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`

#### Implementation Steps

1. Replace removed-command lists in execution skills with generic text such as "The coordinator/runtime owns public workflow/operator-guided execution mutations."
2. Keep explicit implementer/subagent prohibitions against direct runtime-state mutation, direct record editing, and ad hoc artifact repair.
3. If current public commands are listed, label them as coordinator/runtime-owned public commands and do not mix them with removed helper names.
4. Update `execution_skill_docs_keep_candidate_artifacts_and_authoritative_mutations_separated` so it proves candidate artifacts are not runtime authority and proves direct runtime mutation is prohibited without requiring removed helper command names.
5. Extend active prompt/doc hidden-helper scans to deny the retired contract/evaluation/handoff record-style helper names and retired note-style helper wording. Use precise patterns so generic prose words like "note" are not banned globally.
6. Add synthetic negative samples in Rust and Node tests that would fail if an active prompt tells agents to run a retired contract record-style helper or invoke the retired note-style helper.
7. Regenerate skill docs and verify generated output is fresh.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo test --test runtime_instruction_contracts -- --nocapture`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 2: Move Reducer-Consumed Truth Derivation Out Of The `state.rs` Read-Model Facade

#### Spec Coverage

- Reducer no longer imports read-model/status builders through `state.rs` compatibility re-exports.
- Boundary tests reject reducer dependence on `state.rs` read-model re-exports.
- Shared upstream projection module owns reducer-consumed truth derivation and blocking-record projection.

#### Goal

Make reducer layering explicit. Reducer may build runtime state from event authority and upstream status-projection primitives, but it must not hide read-model dependencies through the broad compatibility facade.

#### Context

The modularization audit found `reducer.rs` importing `derive_execution_truth_from_authority_with_gates`, `compute_status_blocking_records`, and related types through `crate::execution::state`. That obscures the intended flow and leaves boundary tests aimed only at command modules.

#### Constraints

- Preserve current reducer output and read-model public status output.
- Do not duplicate status derivation logic.
- Do not make command modules import read-model/status builders.
- Prefer a focused module with a narrow name, for example `src/execution/runtime_truth.rs` or `src/execution/reducer_truth.rs`, over another broad facade.

#### Done when

- Reducer imports reducer-consumed truth derivation and blocking-record projection from a focused upstream module, not from `state.rs` read-model re-exports.
- The focused module owns `ExecutionDerivedTruth` and the functions needed by both reducer and read-model loading paths, or otherwise exposes a narrow interface consumed by both.
- `state.rs` no longer re-exports reducer-only/read-model builder functions that command modules could accidentally import.
- Boundary tests reject reducer imports of read-model/status builders through `crate::execution::state`.
- Boundary tests document the allowed reducer -> focused-truth-module edge and reject direct reducer -> read-model projection coupling for route decisions.

#### Files

- `src/execution/reducer.rs`
- `src/execution/state.rs`
- `src/execution/read_model.rs`
- `src/execution/read_model/**`
- `src/execution/mod.rs`
- possible new `src/execution/runtime_truth.rs` or focused child module
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/runtime_module_boundaries.rs`

#### Implementation Steps

1. Identify the minimal reducer-consumed surface: status derivation from authoritative state, blocking-record projection, task-review dispatch id, and final-review dispatch authority.
2. Move that surface into a focused upstream module or child module with a name that reflects event-authority-to-runtime-truth projection.
3. Update read-model loading code to consume the focused module instead of owning the reducer-consumed truth functions directly.
4. Update reducer imports to use the focused module directly.
5. Remove broad `state.rs` re-exports for these reducer/read-model builder functions when no production command module needs them.
6. Add module-boundary tests that fail if `reducer.rs` imports `derive_execution_truth_from_authority*` or `compute_status_blocking_records` through `crate::execution::state`.
7. Update boundary docs with the new module and its line cap.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test execution_query -- --nocapture`
- `cargo test --test workflow_runtime -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 3: Centralize Task-Closure Baseline Bridge Routing

#### Spec Coverage

- Task-closure baseline bridge route task/readiness is owned by `repair_route_decision`.
- `public_route_selection` and `next_action` consume shared bridge decisions instead of local predicates.
- Boundary tests reject local baseline-bridge route predicates in public-route and next-action modules.

#### Goal

Remove split decisioning around when execution reentry should become `close-current-task` through the task-closure baseline bridge.

#### Context

`repair_route_decision.rs` already owns baseline-bridge helpers, but `public_route_selection.rs` still defines `task_closure_baseline_route_task` with local status/reason-code checks, and `next_action.rs` has multiple local baseline-bridge predicate branches. These can drift and reintroduce route loops.

#### Constraints

- Preserve route behavior for missing-current-closure, stale-unreviewed, task-cycle-break, and external-review-ready cases.
- Do not move public command construction out of the shared public command constructors.
- Do not make `repair_route_decision` import router or workflow presentation modules.
- Keep `public_route_selection` focused on seed projection; it should ask shared route decision helpers for baseline bridge facts.

#### Done when

- `public_route_selection.rs` no longer defines `task_closure_baseline_route_task`.
- `next_action.rs` no longer owns local baseline-bridge route readiness predicates such as `stale_unreviewed_bridge_ready_for_task` or `stale_provenance_allows_task_closure_baseline_route`.
- A shared decision object or helper in `repair_route_decision` returns the baseline bridge route task/readiness facts needed by public route selection and next action.
- Boundary tests reject local `fn task_closure_baseline_route_task`, `fn stale_unreviewed_bridge_ready_for_task`, and duplicate baseline-bridge route predicate definitions outside the shared owner.
- Liveness and public replay tests still prove no repeated same-command loops.

#### Files

- `src/execution/repair_route_decision.rs`
- possible `src/execution/repair_route_decision/baseline_bridge.rs`
- `src/execution/public_route_selection.rs`
- `src/execution/next_action.rs`
- `src/execution/router.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/liveness_model_checker.rs`
- `tests/public_replay_churn.rs`

#### Implementation Steps

1. Define a shared baseline-bridge route decision input/output shape in the repair-route decision owner.
2. Move the `public_route_selection` status/reason-code predicate into that shared owner, using existing stale-target and candidate helpers.
3. Replace `public_route_selection::task_closure_baseline_route_task` with a call to the shared owner.
4. Replace duplicated `next_action` baseline-bridge predicates with calls to the shared owner or shared readiness helpers.
5. Preserve existing `NextActionDecision` outputs and `WorkflowRoutingDecision` fields.
6. Strengthen boundary tests to fail on local baseline-bridge selector definitions outside the owner.
7. Add or update liveness/public replay tests if any route behavior changes are necessary.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test liveness_model_checker -- --nocapture`
- `cargo test --test public_replay_churn -- --nocapture`
- `cargo test --test workflow_runtime -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 4: Guard Repair-Route Cohesion And Active-Contract Unit-Review Proof Semantics

#### Spec Coverage

- `repair_route_decision` is documented and line-capped or split into focused child modules.
- Active-contract serial unit-review proof is explicitly classified and guarded as runtime-owned contract proof, while legacy/plain proof artifacts remain diagnostic-only.

#### Goal

Prevent the new shared route owner from becoming a replacement monolith, and close the residual legacy-proof ambiguity without weakening intentional active-contract gates.

#### Context

The modularization audit found `repair_route_decision.rs` at 817 lines with no documented cap. The proof/evidence audit found active-contract serial unit-review proof still fail-closes gate review/finish-gate. Current code and tests treat that as intentional active-contract authority, while legacy/plain proof artifacts have been demoted to diagnostic-only. The implementation needs to make that distinction explicit and test-guarded.

#### Constraints

- Do not weaken active worktree lease binding or active-contract serial unit-review proof where the active contract is runtime-owned and authoritative.
- Do not let plain/legacy unit-review proof artifacts regain control-plane authority.
- Do not leave `repair_route_decision.rs` uncapped after adding baseline-bridge ownership.
- If splitting `repair_route_decision`, preserve import boundaries and avoid cycles with router, next-action, and read-model modules.

#### Done when

- `repair_route_decision` is either split into cohesive child modules or explicitly documented and capped with a defensible focused-module boundary.
- Boundary tests enforce the cap/documentation.
- Active-contract serial unit-review behavior is documented as runtime-owned contract proof, not passive evidence/projection proof authority.
- Tests continue to prove plain/no-active-contract unit-review artifacts are diagnostic-only.
- Tests explicitly prove active-contract serial unit-review proof failures do not expose hidden helper commands or manual proof-repair guidance in public output.

#### Files

- `src/execution/repair_route_decision.rs`
- possible `src/execution/repair_route_decision/**`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `src/execution/state/worktree_lease_truth.rs`
- `src/execution/state/unit_review_truth.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/internal_plan_execution.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_instruction_contracts.rs`

#### Implementation Steps

1. Decide whether the best boundary is a split or an explicit cap. Prefer splitting if Task 3 grows `repair_route_decision.rs` further.
2. If splitting, extract cohesive families such as baseline bridge, repair target planning, follow-up selection, and authority-input construction into child modules with explicit `pub(super)` boundaries.
3. Add focused line caps and boundary documentation for every new child module.
4. Add a boundary test that prevents `repair_route_decision.rs` from exceeding its cap or being omitted from the boundary doc.
5. Add documentation near active-contract serial unit-review enforcement explaining that active contracts are runtime-owned proof boundaries, while plain/no-active-contract proof artifacts are diagnostic-only.
6. Add/adjust tests that assert plain unit-review proof artifacts warn only when no active contract is present.
7. Add/adjust tests that assert active-contract serial unit-review failure messages and public docs do not instruct agents to repair, restore, or record unit-review proof manually.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test internal_plan_execution -- active_contract --nocapture`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo test --test runtime_instruction_contracts -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`

### Task 5: Final Validation, Review, And Audit Loop

#### Spec Coverage

- Full validation, prebuilts/provenance if needed, clean-context whole-plan review, and A-H audit loop are complete.

#### Goal

Prove the seventh-audit remediation is clean end-to-end and rerun the audit loop until no actionable audit issues remain.

#### Context

The user requested strict per-task validation/review and continued audit -> implementation loops until no actionable audit issues remain.

#### Constraints

- Always run strict clippy and full nextest with no fail-fast before dispatching any review.
- Do not allow review/audit subagents to spawn additional subagents.
- Do not use FeatureForge runtime skills or project skills.
- Refresh checked-in prebuilts only if Rust source changes require it. If the default Windows MSVC refresh fails on this macOS host due missing Windows SDK headers, use the documented GNU cross-build path and report the MSVC failure accurately.

#### Done when

- Generated skill/agent docs are fresh.
- Strict clippy passes.
- Full nextest with no fail-fast passes.
- Liveness model checker passes explicitly.
- Prebuilt provenance passes if prebuilts were refreshed.
- Clean-context whole-plan review reports no actionable findings.
- Fresh A-H audit reports no actionable findings. If it finds actionable issues, write the next remediation plan and continue the loop.

#### Files

- All files touched by Tasks 1-4.
- Checked-in prebuilts if Rust source changes alter the runtime binary.
- New audit reference/plan docs if the loop continues.

#### Implementation Steps

1. Run generated doc checks and Node runtime doc tests.
2. Run strict clippy.
3. Run full nextest with no fail-fast.
4. Run `cargo test --test liveness_model_checker`.
5. Refresh prebuilts if Rust source changed and verify provenance.
6. Run `git diff --check`.
7. Create a synthetic review ref over the Plan 6 base and dispatch a clean-context whole-plan reviewer.
8. Remediate/revalidate/rereview until the whole-plan review has no actionable findings.
9. Dispatch fresh A-H audit subagents with clean context and no subagent spawning.
10. Synthesize the A-H audit. If actionable findings remain, write and implement the next plan. If not, report the final verdict.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`
- `cargo test --test liveness_model_checker`
- `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .` if prebuilts were refreshed
- `git diff --check`
