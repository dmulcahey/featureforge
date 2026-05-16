# Runtime signal/noise audit remediation

## Workflow State

Draft

## Plan Revision

2026-05-10.1

## Execution Mode

Sequential implementation with full validation and clean-context review after each task.

## Goal

Resolve the actionable findings from the fifteenth runtime-safety audit without adding more self-referential guardrails. Preserve the runtime safety gains from prior remediation while reducing conceptual surface area, display-command ambiguity, static-test churn, incidental golden coverage, and hidden import coupling.

## Architecture

The runtime route remains authoritative through typed public route data:

CLI args -> command module -> transition guard -> event append -> reducer -> read model -> route decision -> workflow operator presentation.

This plan does not change route authority, event-log truth, task-closure truth, or public command eligibility. The implementation tightens wording and tests so the shipped contract is clearer:

- `recommended_public_command_argv` and bound public templates are executable authority.
- `recommended_command` remains display-only compatibility text.
- diagnostic states fail closed with one route-owner requery path.
- historical regression inventories remain navigational references, not behavior proof.
- public schema assertions protect externally meaningful route DTO fields, not incidental generated JSON shape.
- module-boundary tests protect ownership/import rules without pinning private helper names.
- state submodules expose their own dependencies instead of importing through a broad parent prelude.

## Change Surface

- `tests/workflow_shell_smoke.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/runtime_instruction_contracts.rs`
- `tests/workflow_runtime.rs`
- `tests/plan_execution.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `tests/using_featureforge_skill.rs`
- `tests/packet_and_schema.rs`
- `tests/fixtures/runtime-goldens/README.md`
- `tests/fixtures/runtime-goldens/public-schema-signatures.json`
- `tests/runtime_module_boundaries.rs`
- `src/workflow/operator.rs`
- `src/execution/state.rs`
- `src/execution/state/worktree_lease_truth.rs`
- `src/execution/state/unit_review_truth.rs`
- skill templates and generated skill docs touched by route-failure wording or installed-control-plane boilerplate
- generated docs/tests only where they enforce the revised contract

## Preconditions

- The previous remediation plan `docs/featureforge/archive/runtime-safety-audit-history/plans/2026-05-10-runtime-diagnostic-signal-fourteenth-audit-remediation.md` has passed full validation and independent review.
- `cargo clean` has been run before this audit iteration.
- No FeatureForge runtime/project skills are used.
- Review subagents must not spawn subagents.
- Full validation before each review must include strict Clippy and full no-fail-fast nextest.

## Known Footguns / Constraints

- Do not remove `recommended_command` compatibility fields from public JSON.
- Do not weaken typed public argv/template authority.
- Do not replace runtime behavior tests with static inventory checks.
- Do not add more function-name pinning while trying to remove function-name pinning.
- Do not loosen schema parity tests that prove checked-in schemas match generated schemas.
- Do not hide mandatory route law solely in companion references.
- Do not edit generated `skills/*/SKILL.md` by hand when a `.tmpl` exists; update templates/generator and regenerate.
- Do not use `#[allow(clippy::...)]` or weaken lint policy.
- If full nextest exceeds 4-5 minutes, run `cargo clean`, rerun, and address introduced performance issues if it still exceeds the threshold.

## Requirement Coverage Matrix

| Requirement | Covered By |
|---|---|
| `recommended_command` never reads as executable authority | Task 1 |
| fallback/runtime-diagnostic wording points to one fail-closed action | Task 1 |
| runtime-remediation inventory is useful without being over-tested | Task 2 |
| schema contract coverage protects public route DTO semantics without broad fixture churn | Task 3 |
| module-boundary tests enforce ownership without private helper-name locks | Task 4 |
| `state.rs` stops acting as a broad parent prelude for worktree/unit review truth | Task 4 |
| prompt budget pressure is relieved by deletion/collapse, not cap churn | Task 5 |
| generated docs remain fresh | Task 5 and final validation |

## Tasks

### Task 1 - Clarify display-only and fail-closed wording

#### Spec Coverage

- Public CLI / reachability: typed public command authority over display strings.
- Public-output and agent-UX: failures point to one public next step; `blocked_runtime_bug` wording is diagnostic-only.

#### Goal

Remove remaining test/doc/runtime wording that describes `recommended_command` as executable authority or tells agents to "repair" routing without a concrete safe action.

#### Context

Audit A found public smoke tests still saying `recommended_command` "should return an executable plan execution command". Audit H found route-failure wording such as "repair the doctor/operator route path" and `blocked_runtime_bug` next-step text that weakens the fail-closed stop.

#### Constraints

- Keep `recommended_command` compatibility data present where the schema requires it.
- Do not teach agents to parse, split, or execute display text.
- Use one fail-closed action: stop, report the route diagnostic, fix only obvious binary/state-dir/repo-root binding, then rerun doctor/operator JSON.

#### Done when

- `tests/workflow_shell_smoke.rs` uses typed argv/template surfaces for executable assertions.
- Display summary checks only assert display compatibility, never executable authority.
- Skills/templates no longer use "repair the doctor/operator route path" or "report/repair" wording.
- `blocked_runtime_bug` operator text matches the stronger stop-and-report contract.
- Tests/golden assertions are updated to the revised wording.

#### Files

- `tests/workflow_shell_smoke.rs`
- `src/workflow/operator.rs`
- `tests/workflow_runtime.rs`
- `skills/using-featureforge/SKILL.md.tmpl`
- `skills/plan-eng-review/SKILL.md.tmpl`
- generated `SKILL.md` files
- `tests/using_featureforge_skill.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`

#### Implementation Steps

1. Replace display-command "executable" assertions with `recommended_public_command_argv` assertions.
2. Rename local variables like `routed_commands` when they contain display summaries.
3. Update fallback wording in templates to stop/report/rerun doctor/operator after only obvious binding fixes.
4. Update `blocked_runtime_bug` human next-step text to explicitly stop and avoid mutations/artifact reconstruction.
5. Regenerate skill docs and update tests that assert the old phrasing.

#### Validation Expectations

- Targeted Rust tests covering changed assertions pass.
- Targeted Node skill-doc tests pass.
- Full validation before review passes.

### Task 2 - Prune static runtime-remediation inventory duplication

#### Spec Coverage

- Tests: public-flow tests prove behavior rather than static inventory prose.
- Signal/noise: keep historical failure-shape visibility without static-test sprawl.

#### Goal

Keep one compact inventory contract test and remove inventory-prose assertions from behavior-oriented Rust suites.

#### Context

Audit I found the `runtime-remediation` README is protected by repeated tests across unrelated suites, including exact detail anchors. This proves the README content repeatedly instead of proving runtime behavior.

#### Constraints

- Preserve the README as a scenario/file-level reference.
- Keep one doc-contract test for section shape and `FS-01` through `FS-22` scenario IDs.
- Do not remove runtime behavior tests that actually exercise the historical failures.

#### Done when

- Only one test owns the static inventory shape/scenario-ID check.
- Runtime behavior suites no longer assert detailed README prose anchors or surface maps.
- The README still documents scenario/file granularity and command-budget coverage.

#### Files

- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/runtime_instruction_contracts.rs`
- `tests/workflow_runtime.rs`
- `tests/plan_execution.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `tests/using_featureforge_skill.rs`
- `tests/fixtures/runtime-remediation/README.md` only if wording needs to match the single contract

#### Implementation Steps

1. Keep a single Node doc-contract test that asserts mandatory section names and `FS-01` through `FS-22`.
2. Remove detailed prose-anchor checks from Node and Rust suites.
3. Remove inventory tests entirely from runtime behavior suites unless they protect a unique non-README behavior contract.
4. Leave actual FS behavior tests unchanged.

#### Validation Expectations

- Node doc contracts pass.
- Rust suites with removed inventory tests still compile and pass targeted checks.

### Task 3 - Replace broad public schema signature golden with targeted route schema checks

#### Spec Coverage

- Public route JSON remains contract-covered without incidental schema fixture churn.
- Schema parity remains enforced by generated-vs-checked-in tests.

#### Goal

Delete the large `public-schema-signatures.json` fixture and replace it with targeted assertions for externally meaningful route DTO fields and enums.

#### Context

Audit I found public schema signature coverage duplicates schema parity tests and pins broad generated shape to a 2,460-line fixture. The true safety value is route DTO field/enumeration coverage.

#### Constraints

- Keep checked-in/generated schema parity tests.
- Keep phase/state-kind enum coverage against runtime route goldens.
- Do not remove public route field coverage for `phase`, `phase_detail`, `next_action`, `recommended_public_command_argv`, `recommended_public_command_template`, `required_inputs`, `recording_context`, and related route metadata.

#### Done when

- `tests/fixtures/runtime-goldens/public-schema-signatures.json` is deleted.
- Runtime-golden README no longer references that fixture.
- `tests/packet_and_schema.rs` has targeted public-route schema contract assertions instead of the broad signature snapshot.
- The targeted assertions fail if typed route authority fields disappear or lose their key descriptions.

#### Files

- `tests/packet_and_schema.rs`
- `tests/fixtures/runtime-goldens/README.md`
- `tests/fixtures/runtime-goldens/public-schema-signatures.json`

#### Implementation Steps

1. Remove Task 8 signature snapshot constants/helpers/test that only supported the broad fixture.
2. Add focused schema tests for workflow-operator, plan-execution-status, and workflow-handoff route fields.
3. Assert typed argv/template descriptions are present and `recommended_command` remains compatibility/display-only.
4. Delete the fixture and update README.

#### Validation Expectations

- `cargo test --test packet_and_schema` passes.
- No references to the deleted fixture remain.

### Task 4 - Reduce module-boundary private-name pinning and state parent-prelude coupling

#### Spec Coverage

- Modularization: new modules have cohesive responsibilities; boundary tests enforce imports/ownership without helper-name churn.
- Signal/noise: remove tests that pin private helper names when boundary/import/behavior checks are enough.

#### Goal

Make module-boundary tests less brittle and move worktree lease probe DTOs out of `state.rs` into the owning state submodule.

#### Context

Audit G found no severe split decisioning but flagged `state.rs` as a broad parent prelude. Audit I found module-boundary tests pin exact function names and file topology.

#### Constraints

- Keep import-boundary and forbidden-call scans.
- Preserve ownership checks for public route selection, route-plan projection, blocker projection, and state-kind/follow-up ownership.
- Do not weaken real split-decisioning protections.
- Do not add clippy suppressions.

#### Done when

- `WorktreeLease*Probe` DTOs live in `state/worktree_lease_truth.rs` or a narrow support module, not `state.rs`.
- `unit_review_truth.rs` imports the probe type from the owning module rather than from the parent prelude.
- `state.rs` no longer imports `serde::Deserialize` solely for child-owned DTOs.
- Module-boundary tests no longer require private helper names such as `shared_next_action_seed_from_precomputed_decision`, `harness_phase_for_route_status`, or `public_repair_targets_from_route_decision`.
- Boundary tests still enforce prohibited imports, duplicate route construction, and module-owner responsibilities.

#### Files

- `src/execution/state.rs`
- `src/execution/state/worktree_lease_truth.rs`
- `src/execution/state/unit_review_truth.rs`
- `tests/runtime_module_boundaries.rs`

#### Implementation Steps

1. Move worktree lease authoritative probe structs to `worktree_lease_truth.rs` with the narrowest visibility required by `unit_review_truth.rs`.
2. Replace broad parent imports in touched child modules with direct crate/sibling imports where practical.
3. Remove unused parent imports.
4. Rewrite brittle boundary assertions to check owner modules contain the expected public/semantic type or forbidden dependency edges without exact private helper-name locks.
5. Run targeted module-boundary tests before full validation.

#### Validation Expectations

- `cargo test --test runtime_module_boundaries` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes before review.

### Task 5 - Collapse generated prompt boilerplate and preserve route law actionability

#### Spec Coverage

- Prompt surface: budget stays enforced while mandatory law remains top-level.
- Signal/noise: prompt budget is a forcing function for deletion/collapse.

#### Goal

Reduce generated installed-control-plane boilerplate and any duplicate wording introduced by Task 1, while preserving the actionable top-level law agents need.

#### Context

Audit I found total skill-doc budget is tight enough to encourage line-count games. The best next change is deletion/collapse rather than budget cap churn.

#### Constraints

- Do not move mandatory installed-runtime law solely into companion references.
- Keep explicit prohibitions against repo-local runtime, target/debug runtime, and cargo-run live routing.
- Keep the canonical operator-route reference discoverable.
- Regenerate generated skill docs.

#### Done when

- `buildInstalledControlPlaneSection()` is shorter but still explicit and test-covered.
- Skill budgets gain meaningful headroom without increasing caps.
- Generated docs are fresh.
- Tests assert the compact mandatory law rather than line-by-line boilerplate.

#### Files

- `scripts/gen-skill-docs.mjs`
- generated `skills/*/SKILL.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `skills/skill-doc-budgets.json` only if deletion creates an opportunity to lower caps without churn

#### Implementation Steps

1. Collapse installed-control-plane prose to a compact paragraph/list.
2. Update tests to assert the compact law in one or two patterns.
3. Regenerate skill docs.
4. Run prompt budget and generation checks.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check` passes.
- `node scripts/gen-agent-docs.mjs --check` passes.
- `node --test tests/codex-runtime/*.test.mjs` passes.
- Full validation before review passes.

### Task 6 - Final validation, clean review, and next audit decision

#### Spec Coverage

- Implementation loop requirements.

#### Goal

Validate the full repo, dispatch an independent clean-context review against this plan, remediate any findings, and decide whether the next audit loop has no actionable issues.

#### Context

The user requires strict Clippy and full no-fail-fast nextest before every review dispatch, plus review/remediate/revalidate loops.

#### Constraints

- Do not use FeatureForge runtime/project skills.
- Do not allow subagents to spawn subagents.
- Do not interrupt in-flight executions.
- If full nextest takes more than 4-5 minutes, run `cargo clean`, rerun, and stop to address performance if still over threshold.

#### Done when

- Full validation passes.
- Independent clean-context review passes with no findings.
- If review finds issues, fixes are implemented and the full validation/review loop repeats until clean.
- A new audit iteration starts with `cargo clean`, unless the audit loop concludes with no actionable issues.

#### Files

- No planned source files beyond validation/review remediation.

#### Implementation Steps

1. Run generated-doc checks, Node tests, format check, strict Clippy, full nextest no-fail-fast, liveness model checker, prebuilt provenance verify, and `git diff --check`.
2. Measure full nextest runtime and apply the clean/rerun/performance protocol if required.
3. Create a non-destructive review snapshot commit with `git commit-tree`.
4. Dispatch a clean-context reviewer with exact base/head SHA and this plan.
5. Remediate any review findings with full validation and rereview until clean.
6. Start the next audit iteration with `cargo clean`.

#### Validation Expectations

- Full validation passes and remains performant.
- Clean-context review reports no actionable findings.
