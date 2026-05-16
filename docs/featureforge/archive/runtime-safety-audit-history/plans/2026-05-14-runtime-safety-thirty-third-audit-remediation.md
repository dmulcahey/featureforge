# Runtime Safety Thirty-Third Audit Remediation

## Workflow State

Engineering remediation plan for the current runtime-safety audit loop. This plan is active until every task below is implemented, fully verified, independently reviewed, and followed by another deep audit loop with the signal-to-noise auditor included.

## Plan Revision

1

## Execution Mode

Sequential implementation with full verification and clean-context review after each task. Do not use FeatureForge runtime/workflow commands as workflow participation. Do not use FeatureForge/project skills. Do not allow reviewers or implementation subagents to spawn additional subagents.

## Goal

Close the actionable thirty-third audit findings while reducing conceptual surface area. The implementation must move route semantics to the route-plan ownership boundary, collapse duplicated public mutation/token authority, expand public-flow proof where tests are already treated as public-flow surfaces, remove hidden-command names from top-level prompts, improve public UX wording, and delete low-signal prose/source-shape assertions that now enforce churn more than behavior.

## Architecture

- Public route authority is typed. `recommended_public_command_argv` is the authoritative executable route when present; otherwise a same-plan operator-materialized `recommended_public_command_template` is the only bindable route. `recommended_command` is display-only compatibility text; `next_action`, prose summaries, and diagnostic context are not executable authority.
- Route semantics are owned by the route-planning layer. Query/read-model code may expose immutable facts, but it must not own final public blocking scope, blocking task, external wait state, or route phase normalization.
- Public mutation authority has one typed source. A mutation request may carry task/step/input context, but the public command token and mutation-kind classification must derive from the typed public command owner instead of a parallel enum.
- Public-flow proof gates should match the test files the scanner protects as public-flow surfaces. Static scanners remain useful, but the named public shipped-runtime gate must not pass while omitting compiled-CLI public smoke suites.
- Late-stage route goldens are useful as external JSON contracts, but transition reachability must also be replayed through public aggregate commands where practical.
- Skills should be short, route-actionable, and high signal. They should refer to canonical route references instead of repeating the same negative law or exposing hidden command names as examples.
- Boundary tests should enforce ownership and import direction. They should not pin incidental private helper names, child-module names, or source substrings unless the symbol is deliberately public architecture.

## Change Surface

- `docs/featureforge/plans/**`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `docs/testing.md`
- `scripts/run-public-runtime-flow-tests.sh`
- `scripts/gen-skill-docs.mjs`
- `skills/using-featureforge/SKILL.md.tmpl`
- generated `skills/using-featureforge/SKILL.md`
- `references/operator-route-authority.md`
- `src/execution/query.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/**`
- `src/execution/command_eligibility.rs`
- `src/execution/command_eligibility/command_kind.rs`
- `src/execution/command_eligibility/mutation_request.rs`
- `src/execution/closure_dispatch.rs`
- `src/execution/review_state.rs`
- `src/execution/status_support.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/public_replay_churn.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/workflow_entry_shell_smoke.rs`
- `tests/plan_execution.rs`
- `tests/plan_execution_final_review.rs`
- `tests/workflow_runtime_final_review.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `tests/execution_query.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`

## Preconditions

- Do not use FeatureForge skills or project skills.
- Do not run FeatureForge runtime/workflow commands as workflow participation. Test commands that exercise the shipped CLI are allowed as validation.
- Before each full test cycle, verify no `cargo nextest`, `cargo-nextest`, `nextest run`, `cargo test`, or `cargo clippy` process is active.
- Before each audit-loop iteration, run `cargo clean`.
- Run strict clippy and a full no-fail-fast nextest suite before dispatching each clean-context review.
- If a full suite exceeds 4-5 minutes, run `cargo clean`, rerun, and remediate introduced performance issues if repeatable. If it exceeds 10 minutes, stop immediately and apply the clean/rerun/remediation rule.
- Edit skill templates, then regenerate generated `SKILL.md` output.
- Prefer deleting duplicated scanner assertions over adding broader scanners around the same duplication.

## Known Footguns / Constraints

- Do not replace route-plan ownership with another catch-all module.
- Do not move public route semantics from `query.rs` into workflow/operator presentation code.
- Do not make `query.rs` append events, mutate state, or construct executable route commands.
- Do not remove public mutation context fields just to collapse enums; close-current-task, transfer, and late-stage modes still need their arguments.
- Do not weaken public/private helper quarantine.
- Do not turn every protected public-flow file into a slow end-to-end replay if the file is a boundary/schema/static proof. Split the public-flow gate into executable proof and static guard phases when needed, but make that split explicit and tested.
- Do not preserve hidden command names in prompts simply to teach agents not to run them.
- Do not delete mandatory route law from top-level route-owning skills. Consolidate duplicate prose into the generated control-plane section and canonical reference.
- Do not make runtime boundary tests blind to ownership. Keep import-direction and single-owner checks; remove only incidental private source-shape checks.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| Route blocking/wait semantics are owned by route planning, not `query.rs` | Task 1 |
| Stale docs no longer reference a nonexistent exact-route module | Task 1 |
| Public mutation token authority derives from one typed public-command owner | Task 2 |
| Production code no longer compares public mutation command tokens as raw strings | Task 2 |
| Public runtime-flow gate covers the protected public-flow test surface or explicitly separates executable/static phases | Task 3 |
| Late-stage final-review/QA route reachability has public replay coverage beyond synthetic goldens | Task 3 |
| Top-level skills do not expose hidden/low-level recovery command strings | Task 4 |
| Public failure text gives one primary next step and external-ready conditions as diagnostics | Task 4 |
| Final-review materializer does not imply an undefined wrapper is required | Task 4 |
| Prompt-law tests stop pinning high-volume exact prose when generated route law and canonical references are enough | Task 5 |
| Runtime boundary tests enforce architecture boundaries without private helper/source-shape churn | Task 5 |
| Generated skills and docs remain fresh | Task 4 and Task 5 |

## Task 1: Move Route Semantics To Route-Plan Ownership

**Spec Coverage:** Route blocking/wait semantics are owned by route planning, not `query.rs`; stale docs no longer reference a nonexistent exact-route module.

**Goal:** Make `route_plan` the semantic owner for public blocking scope, blocking task, external wait state, and canonical route-phase normalization. `query.rs` should expose read/query facts, not route-control decisions.

**Context:**

The audit found route-plan decisions importing query-owned helpers for final public route semantics: `project_execution_blocking`, `blocking_scope_for_phase_detail`, `external_wait_state_for_phase_detail`, and `canonical_phase_for_shared_decision`. The current boundary tests codify that dependency even though architecture docs say route planning owns route decisions. This is partial centralization under the wrong owner.

The same audit found stale documentation references to `src/execution/status_assembly/exact_route_complete_template.rs`, while the actual bindability policy lives in `src/execution/command_eligibility/execution_target.rs`.

**Constraints:**

- Keep read-model/query APIs that are truly facts in read/query modules.
- Route-plan-owned helpers may consume status/read facts, but query/read-model code must not own final route-control projection.
- Do not introduce a new broad catch-all route module. Prefer a focused child module such as `src/execution/route_plan/blocking_projection.rs` or `src/execution/route_plan/route_semantics.rs`.
- Preserve JSON output compatibility unless a field is explicitly diagnostic-only and already covered by schema updates.

**Done when:**

- `src/execution/route_plan/decision.rs` imports route blocking/wait helpers from a route-plan child module, not from `src/execution/query.rs`.
- `query.rs` no longer owns `project_execution_blocking`, `blocking_scope_for_phase_detail`, `external_wait_state_for_phase_detail`, or `canonical_phase_for_shared_decision`.
- Boundary tests assert route-plan ownership and stop requiring route-plan to depend on query-owned route semantics.
- `docs/runtime-architecture.md` and `docs/featureforge/reference/execution-runtime-module-boundaries.md` name the actual exact-route bindability owner.
- Existing route behavior goldens and public-flow tests remain unchanged unless a deliberate external contract correction is required.

**Files:**

- `src/execution/query.rs`
- `src/execution/route_plan/decision.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/**`
- `tests/runtime_module_boundaries.rs`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/runtime_behavior_golden.rs`
- `tests/public_cli_flow_contracts.rs`

**Implementation Steps:**

1. Create a focused route-plan child module for route blocking/wait/phase projection.
2. Move the route-control helper implementations from `query.rs` into that module, preserving names only when they are still the clearest public-in-crate API.
3. Keep any query-only helper inputs as dependencies; do not move unrelated read/query functions.
4. Update `route_plan/decision.rs` and any other route-plan users to import the moved helpers from the new route-plan module.
5. Remove or narrow `query.rs` exports so downstream code cannot accidentally reintroduce route semantic ownership there.
6. Update runtime boundary tests to require the route-plan helper owner and to forbid `route_plan` importing query-owned route-control helpers.
7. Fix stale exact-route module references in architecture docs to point at `command_eligibility/execution_target.rs`.
8. Run targeted route/status/golden tests before the full gate.

**Validation Expectations:**

- `cargo test --test runtime_module_boundaries`
- `cargo test --test public_cli_flow_contracts`
- `cargo test --test runtime_behavior_golden`
- Full verification gate before review: strict clippy and full no-fail-fast nextest.

## Task 2: Collapse Public Mutation Token Authority

**Spec Coverage:** Public mutation token authority derives from one typed public-command owner; production code no longer compares public mutation command tokens as raw strings.

**Goal:** Remove the drift risk between `PublicCommandKind` and `PublicMutationKind`, and ensure command-token comparisons route through typed public command authority instead of raw string literals.

**Context:**

The audit found `PublicCommandKind` and `PublicMutationKind` both deciding what counts as a public mutation. It also found production code comparing tokens such as `begin`, `complete`, `reopen`, `close-current-task`, or `repair-review-state` as raw strings even though `PublicCommandKind::matches_public_mutation_token` exists.

**Constraints:**

- Preserve mutation request context fields for task, step, transfer mode, late-stage mode, and expected execution fingerprint.
- Do not weaken mutation eligibility checks.
- If keeping a small request enum is unavoidable for ergonomic matching, it must wrap or derive from `PublicCommandKind` and must not duplicate the token set.
- Public CLI strings and JSON route output must remain compatible.

**Done when:**

- `PublicMutationRequest` stores the typed public command kind as the command authority.
- The mutation request layer no longer has an independently maintained public mutation enum that repeats the public command set.
- Production code uses typed helpers for public mutation token matching.
- Static boundary tests catch new raw public mutation token comparisons in production code outside the command authority owner.

**Files:**

- `src/execution/command_eligibility.rs`
- `src/execution/command_eligibility/command_kind.rs`
- `src/execution/command_eligibility/mutation_request.rs`
- `src/execution/closure_dispatch.rs`
- `src/execution/review_state.rs`
- `src/execution/route_plan/route_facts.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_authority_contracts.rs`

**Implementation Steps:**

1. Refactor `PublicMutationRequest` to carry `PublicCommandKind` as the typed command authority.
2. Remove or reduce `PublicMutationKind` so it cannot drift from `PublicCommandKind`. If a compatibility alias remains, it must be derived and not independently match command names.
3. Update mutation request constructors (`begin`, `complete`, `reopen`, `transfer_*`, `close_current_task`, `repair_review_state`, `advance_late_stage`) to call a single constructor that validates the public command kind is a mutation.
4. Replace production raw-token comparisons with `PublicCommandKind` matching helpers.
5. Add or adjust static tests to forbid raw public mutation token comparisons outside `command_kind.rs` and CLI parser boundaries.
6. Run targeted public CLI and mutation eligibility tests before the full gate.

**Validation Expectations:**

- `cargo test --test runtime_authority_contracts`
- `cargo test --test public_cli_flow_contracts`
- `cargo test --test plan_execution`
- Full verification gate before review: strict clippy and full no-fail-fast nextest.

## Task 3: Align Public-Flow Proof With Protected Surfaces

**Spec Coverage:** Public runtime-flow gate covers the protected public-flow test surface or explicitly separates executable/static phases; late-stage final-review/QA route reachability has public replay coverage beyond synthetic goldens.

**Goal:** Make the named public-runtime flow gate match what the scanner already treats as public-flow proof, and add public aggregate replay coverage for late-stage routes that currently rely on synthetic golden setup.

**Context:**

The audit found `scripts/run-public-runtime-flow-tests.sh` running only `public_cli_flow_contracts`, `public_replay_churn`, and `runtime_behavior_golden`, while `tests/support/public_flow_scan.rs` protects a much larger public-flow surface including workflow smoke, plan execution, final review, runtime boundaries, and execution query tests. It also found late-stage route goldens where final-review-current states are reached by synthetic authoritative-state setup rather than public command replay.

**Constraints:**

- Do not mislabel static scanner tests as end-to-end shipped-runtime proof.
- Do not make the public-flow script so broad that it becomes slower than the full suite without added signal.
- Prefer a two-phase script shape if needed: executable public-flow proof plus static/contract guard tests.
- Public replay coverage may start from a legitimate long-lived fixture, but the transition under test must use shipped public CLI commands.

**Done when:**

- `scripts/run-public-runtime-flow-tests.sh` either runs every protected public-flow file that is intended as shipped-runtime proof or explicitly separates protected public-flow static guards from executable proof in a way tests enforce.
- The script includes compiled-CLI public smoke suites such as `workflow_shell_smoke`, `workflow_entry_shell_smoke`, and route-relevant workflow/plan execution public-flow suites unless they are documented as static/internal exceptions.
- Tests assert the script and scanner protected-surface definitions stay aligned.
- At least one final-review/QA late-stage route replay proves public `advance-late-stage` progression into or through the state currently covered by synthetic goldens.
- Golden README and script comments accurately describe the remaining synthetic setup, if any.

**Files:**

- `scripts/run-public-runtime-flow-tests.sh`
- `tests/support/public_flow_scan.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/public_replay_churn.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/workflow_entry_shell_smoke.rs`
- `tests/plan_execution.rs`
- `tests/plan_execution_final_review.rs`
- `tests/workflow_runtime_final_review.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `tests/execution_query.rs`
- `tests/fixtures/runtime-goldens/README.md`

**Implementation Steps:**

1. Classify protected public-flow files as executable shipped-runtime proof, public static/contract guard, or documented internal semantic exception.
2. Update `run-public-runtime-flow-tests.sh` to run the executable proof set and, if practical, the static guard set in the same script under explicit comments.
3. Add a test that parses the script and scanner classification so a protected executable public-flow file cannot be omitted accidentally.
4. Add public replay coverage for a late-stage final-review/QA progression using compiled CLI helpers. Reuse existing fixtures, but make the public command transition the asserted behavior.
5. Update runtime golden documentation to distinguish contract capture from transition reachability.
6. Run the script and targeted affected suites before the full gate.

**Validation Expectations:**

- `bash scripts/run-public-runtime-flow-tests.sh`
- `cargo test --test public_flow_scan_contracts`
- `cargo test --test public_replay_churn`
- `cargo test --test workflow_shell_smoke`
- Full verification gate before review: strict clippy and full no-fail-fast nextest.

## Task 4: Remove Hidden Command Names And Tighten Public UX Text

**Spec Coverage:** Top-level skills do not expose hidden/low-level recovery command strings; public failure text gives one primary next step and external-ready conditions as diagnostics; final-review materializer does not imply an undefined wrapper is required.

**Goal:** Keep prompts and public failure text actionable without exposing concrete hidden commands or encouraging agents to run multiple route queries in loops.

**Context:**

The prompt-surface audit found `skills/using-featureforge/SKILL.md` and its template naming `$_FEATUREFORGE_BIN plan execution recover` in a negative instruction. That still puts a concrete hidden/low-level command into the top-level prompt surface, and the Node tests currently preserve the mention.

The agent-UX audit found `references/operator-route-authority.md` telling agents to execute final-review argv through `_featureforge_exec_public_argv` even though the wrapper is optional and not defined in that snippet. It also found `task_boundary_public_route_remediation` giving a primary operator query and then a conditional external-ready query in the same dense failure message.

**Constraints:**

- Do not delete the underlying rule that recovery must stay on operator-routed public commands.
- Do not hide mandatory route law in companion docs only.
- Do not create new helper wrapper requirements.
- Public errors may include a diagnostic hint, but the primary next action must be singular and public.

**Done when:**

- Top-level generated skill docs and templates no longer contain the literal hidden/low-level recovery command string.
- Tests enforce the semantic rule without pinning the hidden command literal.
- The final-review materializer says to execute returned argv exactly, using `$_FEATUREFORGE_BIN` or `_featureforge_exec_public_argv` only when available/needed, and it does not imply the wrapper must exist.
- Task-boundary public remediation text has one primary command and separates the external-review-ready condition into a diagnostic hint.
- Generated skill docs are fresh.

**Files:**

- `skills/using-featureforge/SKILL.md.tmpl`
- `skills/using-featureforge/SKILL.md`
- `references/operator-route-authority.md`
- `src/execution/status_support.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/workflow_entry_shell_smoke.rs`
- `tests/workflow_shell_smoke.rs`

**Implementation Steps:**

1. Replace the hidden command literal in `using-featureforge` template with generic low-level recovery-command wording that preserves the rule.
2. Regenerate skill docs.
3. Update Node contract tests to assert no hidden recovery command literal appears in active prompt surfaces and to assert the generic rule remains.
4. Clarify the final-review materializer execution line in `references/operator-route-authority.md`.
5. Split or simplify `task_boundary_public_route_remediation` so the primary next step is `workflow operator --plan ... --json`, while the external-review-ready query is a separate conditional diagnostic hint.
6. Update any text/golden assertions affected by the wording.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- `cargo test --test workflow_entry_shell_smoke`
- Full verification gate before review: strict clippy and full no-fail-fast nextest.

## Task 5: Reduce Low-Signal Prompt And Boundary Test Churn

**Spec Coverage:** Prompt-law tests stop pinning high-volume exact prose when generated route law and canonical references are enough; runtime boundary tests enforce architecture boundaries without private helper/source-shape churn.

**Goal:** Keep the tests that protect real agent failure modes while deleting or narrowing assertions that force static prose/source-shape churn.

**Context:**

The signal-to-noise auditor found two P2 issues. First, `skill-doc-contracts.test.mjs` still pins many exact skill phrases even though route law is centrally generated and the canonical route reference owns detailed binding rules. Second, `runtime_module_boundaries.rs` pins many private function names, child module names, and source substrings, so harmless refactors pay an architecture-spec tax.

**Constraints:**

- Keep budget enforcement, generated-doc freshness checks, forbidden hidden-helper/fallback vocabulary checks, and canonical route-reference linkage.
- Keep import-direction checks and single-owner checks for route, status, read-model, mutation, and workflow presentation boundaries.
- Remove exact phrase assertions only where the same semantic law is already covered by generated route authority, companion reference linkage, schemas, or forbidden-pattern scanners.
- Remove private helper/source-shape assertions only when import boundaries and public behavior tests still protect the underlying architecture.

**Done when:**

- Prompt tests assert generated route authority inclusion, companion reference linkage, budget, and forbidden fallback vocabulary without preserving incidental sentence wording across many skills.
- Runtime boundary tests prefer module ownership/import direction/public DTO checks over private helper-name and source-substring preservation.
- The signal-to-noise reduction removes redundant assertions; it does not just move them into a new helper.
- Node and Rust boundary tests remain meaningful and pass.

**Files:**

- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/runtime_module_boundaries.rs`
- `docs/testing.md`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`

**Implementation Steps:**

1. Audit the exact phrase assertions around generated route law and using/execution/review skills.
2. Replace redundant exact phrase assertions with checks for generated route-law section presence, canonical reference linkage, budget compliance, and forbidden fallback/hidden-helper vocabulary.
3. Remove or narrow exact phrase checks that duplicate those canonical tests.
4. Audit runtime boundary tests around the reported ranges and classify each assertion as import/ownership boundary, public DTO/golden boundary, or incidental private source shape.
5. Keep boundary assertions for import direction and single public owner; remove private function-name/source-substring checks that do not protect a real boundary.
6. Update `docs/testing.md` to describe the leaner contract philosophy.
7. Run targeted Node and Rust boundary tests before the full gate.

**Validation Expectations:**

- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- `cargo test --test runtime_module_boundaries`
- `cargo test --test public_cli_flow_contracts`
- Full verification gate before review: strict clippy and full no-fail-fast nextest.

## Task 6: Final Whole-Plan Verification And Audit Handoff

**Spec Coverage:** Generated skills and docs remain fresh; all remediation tasks are verified and ready for the next audit loop.

**Goal:** Prove the full branch is clean after Tasks 1-5, then dispatch a clean-context whole-plan review. Remediate any review findings before starting the next audit loop.

**Context:**

The user’s loop requires full verification and clean-context review after each task, then a whole-plan clean-context review before another audit iteration. The audit validation before this plan passed, including strict clippy, full nextest, Node contract suite, and liveness.

**Constraints:**

- Do not skip full verification because targeted checks passed.
- Do not start the next audit loop while a cargo/nextest process is running.
- Do not let reviewers spawn subagents.
- Do not accept broad new scanner churn as remediation for low signal.

**Done when:**

- `node scripts/gen-skill-docs.mjs --check` passes.
- `node scripts/gen-agent-docs.mjs --check` passes.
- `node --test tests/codex-runtime/*.test.mjs` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- `/usr/bin/time -p cargo nextest run --all-targets --all-features --no-fail-fast` passes and stays under the performance threshold or any repeatable regression is remediated.
- `cargo test --test liveness_model_checker` passes.
- A clean-context whole-plan reviewer finds no actionable issues, or all issues are remediated and re-reviewed.

**Files:**

- All files changed by Tasks 1-5.

**Implementation Steps:**

1. Check no cargo/nextest process is active.
2. Run Node generated-doc checks and Node runtime contract suite.
3. Run strict clippy.
4. Run full no-fail-fast nextest under `/usr/bin/time -p`.
5. Run explicit liveness model checker.
6. Dispatch a clean-context reviewer against this exact plan and the resulting diff. Instruct the reviewer not to use FeatureForge skills/runtime commands and not to spawn subagents.
7. Remediate any findings, rerun full verification, and rereview until clean.
8. Start the next audit loop with subagents A-H plus the signal-to-noise auditor after a `cargo clean`.

**Validation Expectations:**

- Full gate listed above.
- Clean-context whole-plan review with no remaining actionable findings.
