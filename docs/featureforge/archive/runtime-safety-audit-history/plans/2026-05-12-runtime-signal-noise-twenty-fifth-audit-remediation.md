# Workflow State

Engineering Approved

# Plan Revision

Revision 1 - 2026-05-12

# Execution Mode

featureforge:executing-plans

# Goal

Remediate the twenty-fifth runtime safety audit findings without adding more self-referential guard churn. The work must preserve public-runtime authority, remove duplicated decisioning where practical, keep skill guidance actionable, and leave public-flow evidence accurately labeled.

# Architecture

- Route planning remains the owner of public command decisions. Gate/status/operator surfaces may project or copy route-owned decisions, but must not synthesize alternate normal-path commands from reason-code strings.
- Stale task resumption is legal only when a concrete task-scoped stale boundary matches the parked resume target. Targetless branch/milestone stale conditions must route through reconcile or the appropriate late-stage repair path.
- Presentation-specific diagnostics consume execution-owned reason classifiers rather than reimplementing reason families in workflow presentation code.
- Hidden/debug command denial vocabulary should be shared by tests instead of locally retyped in each public-flow assertion.
- Public-flow proof scripts should describe exactly what they prove. Static scanner self-tests are useful gate coverage, not production public-flow execution proof.
- Boundary tests should enforce ownership, import direction, public behavior, and line/module caps without pinning private helper names unless the helper name is the boundary contract.

# Change Surface

- `src/execution/route_plan/stale_repair_target.rs`
- `src/execution/route_plan/planning_facts.rs`
- `src/execution/route_plan/unit_tests.rs`
- `src/execution/state/runtime_methods.rs`
- `src/execution/state/review_gate.rs`
- `src/execution/status_assembly.rs`
- `src/execution/status_assembly/facts.rs`
- `src/execution/review_route_tokens.rs`
- `src/workflow/operator.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/support/public_flow_scan.rs`
- new or updated test-support helpers for hidden command tokens
- `scripts/run-public-runtime-flow-tests.sh`
- `docs/testing.md`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- generated `skills/subagent-driven-development/SKILL.md`
- `skills/skill-doc-budgets.json` if budget changes require regeneration

# Preconditions

- Do not use FeatureForge runtime/project skills.
- Do not let review subagents spawn additional subagents.
- Before each full nextest cycle, ensure no `cargo nextest`, `cargo-nextest`, `nextest run`, or `target/debug/deps/` process is already running.
- Before each audit-loop iteration, run `cargo clean`.
- After each task below, run strict clippy and the full no-fail-fast nextest suite before review.
- If full nextest exceeds 4-5 minutes, run `cargo clean`, rerun, and remediate repeatable regression. If it exceeds 10 minutes, stop immediately after the run and enter clean/rerun/performance remediation.
- Generated skill docs must be regenerated from templates, never hand-edited only.

# Known Footguns / Constraints

- Do not make `resume_task` or `resume_step` authoritative just because no task target is projected. Absence of a task target means there is no exact resume binding.
- Do not reintroduce hidden review-gate helpers, hidden finish-gate helpers, hidden review-dispatch recorders, hidden evidence rebuild helpers, or low-level late-stage recorders as normal-path guidance.
- Do not weaken route goldens or public-flow scanners to make implementation easier. If a scanner is noisy, move it to the right gate and keep its regression value.
- Do not add new prompt prose where a canonical reference can be linked or reused.
- Do not add `#[allow(clippy::...)]` or weaken lint policy.
- Do not treat audit report files as runtime authority.

# Requirement Coverage Matrix

| Requirement | Task Coverage |
| --- | --- |
| Resume fields diagnostic unless exact stale task boundary matches | Task 1 |
| Targetless stale routes converge through reconcile or proper repair, not parked begin | Task 1 |
| Gate blocked output preserves route-owned public command surfaces | Task 2 |
| Final-review gate identity reads avoid repeated transition-state loads | Task 2 |
| Doctor synthetic gate classification consumes shared execution vocabulary | Task 3 |
| Hidden command denial vocabulary is centralized for tests and runtime list is complete | Task 3 |
| Public-flow proof script labels proof accurately and keeps scanner self-tests in static gate coverage | Task 4 |
| Skill handoff orders document-release before terminal final review | Task 4 |
| Boundary tests enforce semantic ownership without private helper shape locks | Task 5 |
| Phase-detail literals stay centralized outside public goldens and shared constants | Task 5 |

# Tasks

## Task 1 - Exact Stale Resume Binding

### Spec Coverage

- Resume fields diagnostic unless exact stale task boundary matches.
- Targetless stale routes converge through reconcile or proper repair.

### Goal

Prevent branch/milestone or targetless stale conditions from routing through parked `resume_task` / `resume_step` unless a concrete task-scoped stale target matches the parked resume task.

### Context

`stale_resume_begin_route_candidate` currently combines projected and authoritative stale task targets, then accepts `None` through `is_none_or`. For non-task stale boundaries this lets an unrelated parked resume become an executable begin route.

### Constraints

- Keep matching task-scoped stale resume behavior intact.
- Do not treat `has_authoritative_stale_target` alone as enough to authorize resume begin.
- Preserve targetless stale reconcile when there is no concrete public target.

### Done When

- `stale_resume_begin_route_candidate` requires a concrete stale task equal to `resume_task`.
- Unit coverage proves matching task stale resume still works.
- Unit coverage proves targetless/branch stale plus parked resume does not create a stale resume begin candidate and still requires reconcile when applicable.
- Public route behavior exposes no begin/reopen command for targetless stale solely because resume fields exist.

### Files

- `src/execution/route_plan/stale_repair_target.rs`
- `src/execution/route_plan/planning_facts.rs`
- `src/execution/route_plan/unit_tests.rs`

### Implementation Steps

1. Change `stale_resume_begin_route_candidate` so it returns false when no concrete stale task exists.
2. Prefer the route-planning exact stale task fact when practical so the predicate and `exact_resume_stale_task` cannot drift.
3. Add regression tests for:
   - exact task stale target matching resume task.
   - stale task mismatch.
   - targetless/branch stale with parked resume and no task target.
   - targetless stale reconcile remains true when reason code is present and no concrete target exists.
4. Run formatting and targeted route-plan tests before full validation.

### Validation Expectations

- `cargo test --lib execution::route_plan::unit_tests -- --nocapture` or the closest supported targeted unit command.
- `cargo clippy --all-targets --all-features -- -D warnings`.
- Full `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`.
- Clean-context task review after full validation.

## Task 2 - Route-Owned Gate Output And Branch Binding Reuse

### Spec Coverage

- Gate blocked output preserves route-owned public command surfaces.
- Final-review gate identity reads avoid repeated transition-state loads.

### Goal

Remove direct reason-code command synthesis from final-review dispatch blocked output and reuse preloaded branch-closure identity data inside gate flows instead of reloading transition state repeatedly.

### Context

`record_review_dispatch_blocked_output_from_gate` has special final-review branches that call `set_gate_public_command` based on reason codes before using `gate_follow_up_routing_state`. `runtime_methods.rs` also calls `current_branch_reviewed_state_id`, `gate_result_current_branch_closure_id`, and `finish_review_gate_pass_branch_closure_id` in the same gate flow; those helpers reload authoritative state.

### Constraints

- Public output must remain actionable.
- If the route decision has no executable public command, fail closed to workflow-operator requery rather than synthesizing one.
- Do not weaken existing route-owned output tests.
- Keep late-stage final-review blocked guidance intact by moving route facts/decisions, not by deleting needed output.

### Done When

- `record_review_dispatch_blocked_output_from_gate` obtains final-review blocked command/template/inputs from route-owned `RouteDecision` or uses requery contract.
- Boundary tests scan the full blocked-output function and reject `set_gate_public_command` or `public_advance_late_stage_command_for_phase_detail` inside it.
- Gate flows compute current branch closure id, reviewed state id, and finish-review pass branch closure id from one preloaded authoritative state read per flow where the data are needed.
- Existing final-review dispatch tests still pass.

### Files

- `src/execution/state/runtime_methods.rs`
- `src/execution/state/review_gate.rs`
- `src/execution/status_assembly.rs`
- `src/execution/status_assembly/facts.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `tests/runtime_module_boundaries.rs`
- related targeted gate tests if existing coverage needs extension

### Implementation Steps

1. Refactor final-review blocked output to call `gate_follow_up_routing_state` first and copy `SpecificGateRecommendation::from_route_decision`.
2. If no route decision provides executable surfaces, apply the existing out-of-phase requery contract.
3. Remove direct final-review reason-code branches that synthesize `repair-review-state` or `advance-late-stage`.
4. Introduce a small branch binding snapshot helper that derives `current_branch_reviewed_state_id`, current branch closure id, and finish-review pass branch closure id from an optional preloaded authoritative transition state.
5. Use the snapshot in review and finish gate projection paths that currently call all three helpers separately.
6. Add or update boundary tests to enforce route-owned gate output and the preloaded-state helper.
7. Run targeted gate/runtime boundary tests before full validation.

### Validation Expectations

- Targeted tests around `contracts_execution_runtime_boundaries`, `runtime_module_boundaries`, and final-review gate/dispatch surfaces.
- `cargo clippy --all-targets --all-features -- -D warnings`.
- Full nextest no-fail-fast.
- Clean-context task review after full validation.

## Task 3 - Shared Diagnostic Vocabulary

### Spec Coverage

- Doctor synthetic gate classification consumes shared execution vocabulary.
- Hidden command denial vocabulary is centralized for tests and runtime list is complete.

### Goal

Move synthetic doctor gate review reason/failure classification out of workflow presentation code, and make hidden-command denial lists use one shared vocabulary for public-flow tests.

### Context

`src/workflow/operator.rs` hardcodes stale/freshness reason families for doctor synthetic gate review output. Hidden command lists are split across `command_eligibility`, `public_flow_scan`, `public_cli_flow_contracts`, and `workflow_shell_smoke`.

### Constraints

- Do not widen public API solely for tests unless the symbol is intentionally a stable public contract.
- Keep active docs able to mention hidden helper terms only in historical/non-imperative contexts already allowed by scanners.
- Do not make the scanner weaker.

### Done When

- Workflow doctor calls an execution-owned helper for synthetic gate review reason classification and failure class.
- Public-flow test hidden-token checks consume one test-support deny-list source.
- Runtime `hidden_command_tokens` includes removed late-stage recorder tokens and branch-closure id flag.
- Static tests prove the scanner, shell smoke checks, and public route argv assertions share the same hidden vocabulary.

### Files

- `src/execution/review_route_tokens.rs` or another cohesive execution-owned vocabulary module
- `src/execution/command_eligibility.rs`
- `src/workflow/operator.rs`
- `tests/support/public_flow_scan.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/workflow_shell_smoke.rs`
- new test-support hidden-token helper if needed
- targeted module-boundary tests

### Implementation Steps

1. Add execution-owned helpers for doctor synthetic gate review reason code and failure class.
2. Replace workflow operator local helpers with calls to the execution-owned helpers.
3. Extend runtime hidden command tokens to cover removed late-stage recorders and the retired branch-closure-id compatibility flag.
4. Add a shared test-support hidden command deny-list and update public CLI flow and workflow smoke assertions to use it.
5. Update public-flow scanner to consume the same list, preserving additional diagnostic phrase scanners separately.
6. Add tests that fail if the shared deny-list drops low-level late-stage recorder tokens.

### Validation Expectations

- Targeted public-flow scanner and workflow shell smoke tests.
- `cargo clippy --all-targets --all-features -- -D warnings`.
- Full nextest no-fail-fast.
- Clean-context task review after full validation.

## Task 4 - Public-Flow Evidence And Skill Handoff Signal

### Spec Coverage

- Public-flow proof script labels proof accurately.
- Scanner self-tests remain valuable static gate coverage.
- Skill handoff orders document-release before terminal final review.

### Goal

Keep public-flow validation evidence high signal and prevent skills from teaching a document-release/final-review loop.

### Context

`scripts/run-public-runtime-flow-tests.sh` includes `public_flow_scan_contracts` while `docs/testing.md` says that suite is gate coverage, not production public-flow proof. `subagent-driven-development` says terminal review happens after document-release, but its handoff block invokes requesting-code-review before finishing, where document-release is then required.

### Constraints

- Do not delete scanner self-tests; move them to the correct static/focused gate or rename the script if needed.
- Keep prompt budget under cap. Collapse wording rather than add repetitive law.
- Regenerate generated skill docs from templates.

### Done When

- Public-flow script contains only compiled public-flow/replay/golden proof suites, or its name/docs accurately identify mixed public+scanner coverage.
- Static scanner self-test remains covered by a documented focused/static validation command.
- `docs/testing.md` distinguishes production public-flow proof from scanner contract coverage.
- `subagent-driven-development` handoff invokes document-release before terminal requesting-code-review, then finishing after both are current.
- Generated skill docs are fresh and budgets pass.

### Files

- `scripts/run-public-runtime-flow-tests.sh`
- `docs/testing.md`
- `tests/public_cli_flow_contracts.rs`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md`
- `skills/skill-doc-budgets.json` if needed
- `tests/codex-runtime/*.test.mjs` only if tests intentionally pin the old sequence

### Implementation Steps

1. Remove `public_flow_scan_contracts` from `run-public-runtime-flow-tests.sh` or rename/reshape the script contract so it is not called pure public-flow proof.
2. Add a documented static/focused command for `public_flow_scan_contracts`.
3. Update tests that assert the public-flow gate contents.
4. Change `subagent-driven-development` template handoff to document-release first, then final requesting-code-review, then finishing.
5. Regenerate skill docs and adjust budget manifest only if generation changes counts.
6. Run Node doc/skill contract checks.

### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`.
- `node --test tests/codex-runtime/*.test.mjs`.
- Targeted public-flow script contract tests.
- `cargo clippy --all-targets --all-features -- -D warnings`.
- Full nextest no-fail-fast.
- Clean-context task review after full validation.

## Task 5 - Boundary Test Signal Cleanup

### Spec Coverage

- Boundary tests enforce semantic ownership without private helper shape locks.
- Phase-detail literals stay centralized outside public goldens/shared constants.

### Goal

Reduce guard-layer noise by trimming implementation-shape locks while preserving meaningful modularity enforcement.

### Context

Audit flagged route-plan child module name/visibility pins, exact helper-name pins in `status_assembly`, and exact import text blocks in public CLI flow contracts. It also found broad phase-detail literal allowlists across public tests.

### Constraints

- Do not remove import-direction, behavior, public-output, or line-cap tests that catch real regressions.
- Replace private-name pins with semantic checks or AST/import-boundary checks.
- Reserve raw phase-detail literals for public JSON/golden expectations and tests where literal output is the contract.

### Done When

- Boundary tests no longer require private helper names where ownership can be enforced through import paths, call graph boundaries, public behavior, or cohesive module caps.
- Public CLI flow contract tests avoid exact import text block pins unless the import block is itself the contract.
- Phase-detail literal allowlist is narrowed or tests use shared constants/helpers for non-golden assertions.
- Signal-to-noise auditor should no longer find static tests around static tests as the main issue.

### Files

- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- shared test helpers/constants as needed

### Implementation Steps

1. Identify each flagged private shape assertion and classify it as semantic boundary, line-cap, import-direction, or removable shape lock.
2. Replace exact child module/helper/import block assertions with broader ownership checks.
3. Narrow phase-detail literal exemptions where safe, moving non-golden tests to constants imported from runtime modules or shared helpers.
4. Preserve tests that protect real public behavior or intentional architecture boundaries.
5. Run targeted boundary tests before full validation.

### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`.
- `cargo test --test public_cli_flow_contracts -- --nocapture`.
- `cargo clippy --all-targets --all-features -- -D warnings`.
- Full nextest no-fail-fast.
- Clean-context task review after full validation.
