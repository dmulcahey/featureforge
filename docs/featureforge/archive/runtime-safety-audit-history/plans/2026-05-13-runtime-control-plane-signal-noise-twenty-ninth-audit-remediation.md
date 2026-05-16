# Workflow State

Engineering Approved

# Plan Revision

Revision 1 - 2026-05-13

# Execution Mode

featureforge:executing-plans

# Goal

Remediate the twenty-ninth runtime safety audit findings by removing the remaining projection-artifact control-plane dependencies, making public diagnostics and high-use skills actionable through typed public route authority, and reducing low-signal modularity/test churn without weakening runtime safety.

Source audit: `docs/featureforge/archive/runtime-safety-audit-history/2026-05-13-twenty-ninth-audit-report.md`

# Architecture

- Runtime-owned state, event records, current closure/branch closure identity, and transition records are control-plane truth.
- Markdown projection artifacts, QA/test-plan artifacts, projection documents, and generated review summaries are audit/projection outputs unless an explicit runtime-owned state record binds them as current control-plane state.
- Gate diagnostics may explain provenance drift, but remediation text must route agents through workflow/operator JSON and typed public argv/template surfaces.
- High-use skills should describe one executable path and reference canonical route law, not duplicate runtime semantics.
- Module-boundary tests should enforce ownership and import direction, not arbitrary source shape.
- `status_assembly.rs` should be a facade/orchestrator over cohesive modules rather than a broad semantic hub.

# Change Surface

- `src/execution/state/unit_review_truth.rs`
- `src/execution/state/worktree_lease_truth.rs`
- `src/execution/commands/advance_late_stage.rs`
- `src/execution/commands/common/late_stage_reruns.rs`
- `src/execution/state/artifact_finish_truth.rs`
- `src/execution/current_truth.rs`
- `src/execution/state/runtime_methods.rs`
- `src/execution/state/review_gate.rs`
- `src/workflow/status.rs`
- `src/execution/route_plan/decision_support.rs`
- `src/execution/query.rs`
- `src/execution/review_state.rs`
- `src/execution/status_assembly.rs`
- new `src/execution/status_assembly/**` modules as needed
- `tests/workflow_shell_smoke.rs`
- `tests/plan_execution.rs`
- `tests/plan_execution_final_review.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `skills/requesting-code-review/SKILL.md.tmpl`
- `skills/test-driven-development/SKILL.md.tmpl`
- `.codex/INSTALL.md`
- `.copilot/INSTALL.md`
- `qa/references/issue-taxonomy.md`
- generated `skills/**/SKILL.md`

# Preconditions

- Do not use FeatureForge runtime/project skills.
- Do not let any subagent spawn additional subagents.
- Use the requested Rust guidance when writing or refactoring Rust.
- Before every full test cycle, verify no `cargo`, `rustc`, `cargo nextest`, `cargo-nextest`, `nextest run`, or active `target/debug/deps/` process is already running.
- Before each new audit-loop iteration, run `cargo clean`.
- After each task implementation, run strict clippy and full no-fail-fast nextest before dispatching a clean-context review.
- If full nextest takes more than 4-5 minutes, run `cargo clean`, rerun the suite, and remediate repeatable performance regressions. If it exceeds 10 minutes, stop immediately and enter clean/rerun/performance remediation.
- Do not weaken hidden/debug command scanners, typed public argv/template contracts, prompt budget enforcement, or reviewer recursion restrictions.

# Known Footguns / Constraints

- Do not turn projection-artifact drift into silent success without preserving diagnostic visibility.
- Do not weaken active contract, worktree lease, completed attempt, task packet, commit proof, branch closure, final-review, QA result, summary hash, or generated-by identity checks.
- Do not make QA source test-plan artifacts authoritative again under another name.
- Do not replace manual diagnostics with new direct command strings. The authoritative executable surfaces remain `recommended_public_command_argv` and a completed `recommended_public_command_template`.
- Do not add broad static scanners when a direct behavior or packaging contract test will catch the same issue.
- Do not split `status_assembly.rs` by moving unrelated imports into a new catch-all child module.
- Do not add `#[allow(clippy::...)]` or weaken lint policy.

# Requirement Coverage Matrix

| Requirement | Task Coverage |
| --- | --- |
| Unit-review markdown projection artifacts cannot block active-contract gate truth | Task 1 |
| Missing/corrupt unit-review projection artifacts remain diagnostic | Task 1 |
| QA recording can proceed without current test-plan markdown materialization | Task 2 |
| Equivalent QA rerun remains idempotent without source test-plan fingerprint | Task 2 |
| Finish readiness does not fail solely because current QA lacks source test-plan binding | Task 2 |
| Retired dispatch/manual repair wording is removed from public diagnostics | Task 3 |
| `requesting-code-review` executes or fails closed on typed final-review route JSON | Task 3 |
| Unrooted `@path` skill references are banned | Task 4 |
| Stale install/QA helper-owned language is removed | Task 4 |
| Runtime callers do not reconstruct route decisions from presentation DTO fields | Task 5 |
| `status_assembly.rs` becomes a smaller facade over cohesive modules | Task 6 |
| Low-signal module-boundary shape assertions are removed or replaced with semantic checks | Task 6 |

# Tasks

## Task 1 - Demote Serial Unit-Review Projection Markdown To Diagnostics

### Spec Coverage

- Active-contract unit-review projection files still control gate truth through `serial_unit_review_*` failures and projection-file validation.

### Goal

Make active-contract serial unit-review gate truth depend on runtime-owned contract state, run identity, completed attempt provenance, approved task packet binding, unit contract fingerprint derivation, and repository commit proof. Treat unit-review markdown projection files as diagnostic projection artifacts.

### Context

The current gate path builds a pseudo lease from runtime state, then requires `unit-review-<run>-task-<task>-step-<step>.md` to exist and pass header/fingerprint validation. If that markdown is pruned or malformed, routing can require public repair/close actions even when authoritative runtime state is sufficient.

### Constraints

- Preserve active-contract and completed-attempt validation.
- Preserve commit proof validation with `reconcile_result_proof_fingerprint_for_review`.
- Keep projection-file validation available for explicit projection/materialization or diagnostic tests.
- Do not let missing/corrupt projection files produce `ExecutionStateNotReady` or `MalformedExecutionState` gate failures in the active-contract serial path.
- Preserve warnings or projection diagnostics so operators can see projection drift without using it as control-plane truth.

### Done when

- `enforce_serial_unit_review_truth` no longer fails gate truth solely because a unit-review markdown projection artifact is missing, unreadable, malformed, stale, or has mismatched headers.
- Projection markdown drift produces warning codes or diagnostic-only reason codes.
- Worktree lease binding logic still fails closed for unsafe active lease state where the lease itself is the runtime-owned authority.
- Public replay/smoke coverage proves a current completed step with missing/corrupt serial unit-review projection artifact can continue through the public route without hidden helpers.
- Existing unsafe lease-state tests still fail closed.

### Files

- `src/execution/state/unit_review_truth.rs`
- `src/execution/state/worktree_lease_truth.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/plan_execution.rs` or `tests/contracts_execution_runtime_boundaries.rs` as needed

### Detailed Implementation Steps

1. Split active-contract serial unit-review proof validation into:
   - authoritative runtime checks that can fail the gate;
   - projection-artifact diagnostics that can only warn.
2. Keep explicit projection validation for paths where a runtime-owned worktree lease binding explicitly names the projection artifact.
3. In `enforce_serial_unit_review_truth`, derive and validate the pseudo lease facts from runtime state, but do not require the projection path to exist.
4. If the projection path exists, validate it best-effort and warn on mismatch; if it is missing/unreadable/malformed, warn with diagnostic-only codes.
5. Add regression coverage for missing serial projection artifact and corrupt serial projection artifact under active contract state.
6. Ensure worktree lease unsafe/open/mismatched proof tests still fail closed.

### Validation Expectations

- Targeted: `cargo test --test workflow_shell_smoke worktree_lease -- --nocapture`
- Targeted: `cargo test --test contracts_execution_runtime_boundaries unit_review -- --nocapture` or closest existing filters
- Required after task: strict clippy and full nextest no fail fast
- Clean-context review against Task 1 after full validation

## Task 2 - Make QA Test-Plan Artifacts Diagnostic-Only

### Spec Coverage

- QA recording and equivalent rerun paths still block on current test-plan markdown artifacts.
- Finish readiness still fails a current QA record missing source-test-plan fingerprint.

### Goal

Make QA readiness and QA recording depend on current runtime-owned QA record truth rather than test-plan markdown materialization.

### Context

The runtime can render QA artifacts without a source test-plan path, but `record_qa` currently blocks when `current_test_plan_artifact_path` is missing/stale. Equivalent current reruns also refuse `already_current` when the QA record lacks source-test-plan fingerprint and no current artifact exists. Finish readiness fails `qa_source_test_plan_mismatch` for the same absence.

### Constraints

- Preserve branch closure, final-review record, result, summary hash, plan path/revision, branch, repo, base branch, reviewed state, and generated-by checks.
- Preserve source test-plan fingerprint when a current test-plan artifact exists.
- Do not require a source test-plan path to render or record QA.
- Do not remove diagnostic evidence that a source test-plan artifact was missing/stale.
- Do not let failed QA or repo-write invalidation become idempotent success.

### Done when

- `record_qa` proceeds when no current test-plan artifact exists, records QA with no source test-plan fingerprint, and emits diagnostic trace/warning where supported.
- Equivalent current QA rerun returns `already_current` based on runtime-owned QA state without requiring source-test-plan fingerprint.
- Finish readiness accepts a current passing QA record without source-test-plan fingerprint.
- Tests cover missing and stale test-plan artifact paths as diagnostic-only for QA progress.
- Tests still cover real QA record mismatch/fail/stale branch conditions as blockers.

### Files

- `src/execution/commands/advance_late_stage.rs`
- `src/execution/commands/common/late_stage_reruns.rs`
- `src/execution/state/artifact_finish_truth.rs`
- `src/execution/current_truth.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/plan_execution_final_review.rs`

### Detailed Implementation Steps

1. Change `record_qa` so `current_test_plan_artifact_path` missing/stale returns `None` for `test_plan_path` rather than a blocked requery output.
2. Keep malformed/unreadable artifact directory errors fail-closed when they indicate runtime state corruption rather than simple absence/staleness.
3. Change equivalent-current QA rerun logic so missing `source_test_plan_fingerprint` does not require locating a current test-plan artifact.
4. Change `require_authoritative_test_plan_binding_for_current_qa` into a diagnostic warning path or remove it from blocking finish readiness.
5. Adjust reason-code helpers that classify `qa_source_test_plan_mismatch` as test-plan refresh if that code is no longer a blocker.
6. Add public or integration regression tests for QA recording and finish readiness without current test-plan artifacts.

### Validation Expectations

- Targeted: `cargo test --test plan_execution_final_review qa -- --nocapture`
- Targeted: `cargo test --test workflow_shell_smoke test_plan -- --nocapture`
- Required after task: strict clippy and full nextest no fail fast
- Clean-context review against Task 2 after full validation

## Task 3 - Make Public Diagnostics And Final-Review Skill Actionable

### Spec Coverage

- Serialized gate diagnostics still mention retired task-review dispatch language.
- Final-review gate diagnostics include multiple local/manual action phrases.
- Workflow status plan/spec diagnostics use free-form “repair” wording.
- `requesting-code-review` prints final-review route metadata instead of executing or failing closed on typed route JSON.

### Goal

Ensure public-facing runtime text and the high-use final-review skill point to one public route: workflow/operator JSON plus typed argv/template execution.

### Context

The branch already made `recommended_command` display-only and added route-law references. The remaining UX problem is stale wording and one skill block that delegates the decisive command execution back to prose.

### Constraints

- Preserve domain-specific failure detail.
- Do not invent direct command strings in diagnostics.
- Use `recommended_public_command_argv` or a bound `recommended_public_command_template`.
- Keep the detailed route-law reference canonical instead of repeating every rule in the skill.

### Done when

- Public gate remediation no longer says “dispatching task review” for normal execution routes.
- Final-review gate remediations point to workflow/operator JSON and typed public surfaces rather than listing local alternatives.
- Workflow status plan/spec diagnostics name the relevant public review/authoring route or `next_skill`.
- `requesting-code-review` materializes and executes the final-review route from `RECORDING_READY_JSON`, or explicitly stops if no executable argv/template exists.
- Skill contract tests verify actionability without pinning incidental prose.

### Files

- `src/execution/state/runtime_methods.rs`
- `src/execution/state/review_gate.rs`
- `src/workflow/status.rs`
- `skills/requesting-code-review/SKILL.md.tmpl`
- `skills/requesting-code-review/SKILL.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/public_cli_flow_contracts.rs`

### Detailed Implementation Steps

1. Replace retired task-review dispatch wording with public-route wording.
2. Normalize final-review gate remediation text to “query workflow/operator JSON for the approved plan and follow typed public argv/template,” while preserving the failing reason in details.
3. Replace free-form workflow “repair the spec/plan” wording with the current public review/authoring route or next skill.
4. Update `requesting-code-review/SKILL.md.tmpl` final shell block:
   - require `REVIEW_RESULT` and `SUMMARY_FILE`;
   - parse `RECORDING_READY_JSON`;
   - execute `recommended_public_command_argv` through `_featureforge_exec_public_argv` when present;
   - bind final-review template inputs from `REVIEWER_SOURCE`, `REVIEWER_ID`, `REVIEW_RESULT`, and `SUMMARY_FILE` when only a template is present;
   - stop with a clear diagnostic when neither surface is executable.
5. Regenerate generated skill docs.
6. Update Node skill contract tests to require actionability and reject the prior metadata-only shell block.

### Validation Expectations

- Targeted: `node scripts/gen-skill-docs.mjs --check`
- Targeted: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- Targeted: `cargo test --test public_cli_flow_contracts -- --nocapture`
- Required after task: strict clippy and full nextest no fail fast
- Clean-context review against Task 3 after full validation

## Task 4 - Clean Prompt/Packaging Stale References

### Spec Coverage

- A generated skill uses an unrooted `@testing-anti-patterns.md` companion reference.
- Install docs preserve stale generated-preamble helper-state wording.
- QA taxonomy says authoritative operations “stay helper-owned.”

### Goal

Keep prompt surfaces compact, resolvable, and aligned with runtime-owned authority.

### Context

These are not runtime blockers, but they are exactly the kind of stale prompt-surface noise that causes agents to rediscover old behavior.

### Constraints

- Do not add broad duplicate route-law prose.
- Edit templates first when generated docs exist.
- Add one focused contract for unrooted `@path` references in generated skills.

### Done when

- `test-driven-development` uses `skill-local \`testing-anti-patterns.md\``.
- Generated skills contain no unrooted `@*.md` ordinary references.
- Install docs no longer describe removed generated-preamble session/contributor/update-cache boilerplate.
- QA taxonomy says `runtime-owned`, not `helper-owned`.
- Generated docs are fresh.

### Files

- `skills/test-driven-development/SKILL.md.tmpl`
- `skills/test-driven-development/SKILL.md`
- `.codex/INSTALL.md`
- `.copilot/INSTALL.md`
- `qa/references/issue-taxonomy.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`

### Detailed Implementation Steps

1. Replace the unrooted `@testing-anti-patterns.md` reference in the template.
2. Add a Node contract that scans generated skill docs for ordinary unrooted `@*.md` references.
3. Update install docs to describe only current runtime helper state.
4. Replace `helper-owned` vocabulary in QA taxonomy with `runtime-owned`.
5. Regenerate skills and run prompt/doc checks.

### Validation Expectations

- Targeted: `node scripts/gen-skill-docs.mjs --check`
- Targeted: `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- Required after task: strict clippy and full nextest no fail fast
- Clean-context review against Task 4 after full validation

## Task 5 - Remove Route DTO Reconstruction From Runtime Callers

### Spec Coverage

- `RouteDecision` can still be reconstructed from `ExecutionRoutingState` DTO fields when `routing.route_decision` is missing.

### Goal

Make the route decision object the single runtime source of truth. Runtime callers should fail closed if a route decision is unexpectedly absent instead of reconstructing it from presentation fields.

### Context

The normal router installs `route_decision`, so no current bug was found. The fallback remains a split-decision escape hatch and is pinned by a boundary test.

### Constraints

- Preserve compatibility only for explicit non-runtime/historical DTO parsing paths if any are required.
- Do not let workflow/operator or query silently infer executable authority from display/status fields.
- Update tests to assert fail-closed or explicit compatibility scoping.

### Done when

- `src/execution/query.rs` and `src/execution/review_state.rs` no longer use DTO reconstruction for normal runtime decisions.
- `route_decision_from_routing` is removed, renamed to an explicit compatibility helper, or restricted to tests/fixtures with clear naming.
- Boundary tests reject normal runtime fallback from presentation fields.
- Public route behavior remains unchanged.

### Files

- `src/execution/route_plan/decision_support.rs`
- `src/execution/query.rs`
- `src/execution/review_state.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `tests/runtime_behavior_golden.rs` if DTO goldens need adjustment

### Detailed Implementation Steps

1. Inventory all callers of `route_decision_from_routing`.
2. For runtime callers, replace fallback reconstruction with fail-closed diagnostics or `None` that preserves no executable route.
3. If compatibility parsing remains necessary, move it behind an explicitly named helper and prevent production runtime callers from importing it.
4. Update boundary tests to encode the new ownership rule.
5. Run route/golden tests to ensure public route JSON still contains typed route decisions in normal flows.

### Validation Expectations

- Targeted: `cargo test --test contracts_execution_runtime_boundaries route_decision -- --nocapture`
- Targeted: `cargo test --test runtime_behavior_golden -- --nocapture`
- Required after task: strict clippy and full nextest no fail fast
- Clean-context review against Task 5 after full validation

## Task 6 - Split `status_assembly.rs` And Delete Low-Signal Shape Guards

### Spec Coverage

- `status_assembly.rs` remains a broad hub.
- Module-boundary tests still include brittle child-module count and line-count shape assertions.

### Goal

Reduce conceptual surface area by extracting cohesive status assembly responsibilities and keeping tests focused on semantic boundaries.

### Context

The prior remediation intentionally deferred the full split. The signal/noise audit found the deferral itself is now the largest remaining architecture noise source.

### Constraints

- Do not change public route semantics.
- Do not create a new catch-all module.
- Keep import-direction and write/read boundary tests.
- Keep facade protection for `state.rs` and `mutate.rs` if still present.
- Remove arbitrary child-module count and facade line-count assertions unless they are replaced by clearer semantic import/owner checks.

### Done when

- `status_assembly.rs` is materially smaller and functions primarily as a facade/orchestrator.
- New child modules have cohesive names and responsibilities, such as overlay hydration, status defaults, branch gate bindings, blocking records, and overlay parsing.
- Boundary docs reflect the new architecture.
- `runtime_module_boundaries.rs` no longer enforces arbitrary child-module count or low-signal line caps.
- Import-direction/owner checks remain green.

### Files

- `src/execution/status_assembly.rs`
- `src/execution/status_assembly/*.rs`
- `tests/runtime_module_boundaries.rs`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`

### Detailed Implementation Steps

1. Extract overlay parsing helpers from `status_assembly.rs` into a focused child module.
2. Extract branch gate binding helpers into a focused child module.
3. Extract blocking-record derivation helpers into a focused child module.
4. Extract route-neutral status defaults or open-step projection helpers if needed to keep the facade small.
5. Replace imports with explicit `pub(crate)` exports only for functions consumed outside status assembly.
6. Remove arbitrary child-module count and low-signal facade line-count assertions from `runtime_module_boundaries.rs`.
7. Keep or strengthen semantic checks for import direction, public command ownership, route decision ownership, and no write helpers in read-model/workflow modules.
8. Update runtime architecture/module-boundary docs.

### Validation Expectations

- Targeted: `cargo test --test runtime_module_boundaries -- --nocapture`
- Targeted: `cargo test --test execution_query -- --nocapture`
- Required after task: strict clippy and full nextest no fail fast
- Clean-context review against Task 6 after full validation
