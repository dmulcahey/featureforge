# Runtime diagnostic and signal remediation

## Workflow State

Draft remediation plan from the fourteenth deep runtime-safety audit loop.

## Plan Revision

2026-05-10.1

## Execution Mode

Sequential implementation with full validation and clean-context review after each task.

## Goal

Close the actionable fourteenth-audit findings without increasing self-referential workflow churn:

- Diagnostic-only `runtime_reconcile_required` routes must fail closed for guessed mutation commands.
- Route selection must have one owner; projection may copy selected route metadata but must not rederive it.
- Bootstrap-only stale target helpers must not drive post-route dispatch targeting.
- Repair-target reason vocabulary must be typed and centralized.
- Active generated skills must not teach removed helper names, even negatively.
- Public output and test text must point agents to typed operator JSON and executable argv/template fields.
- Low-signal duplicate scanners, broad goldens, compatibility re-exports, and repeated prompt prose must be deleted or consolidated.
- Legacy handoff output must not recommend a different review skill than the authoritative route.

## Architecture

The intended runtime chain remains:

1. CLI args parse into typed public commands.
2. Command modules load shared runtime status and route context.
3. Mutation guards compare the exact public request with the selected public route.
4. Commands append authoritative events only after route authorization.
5. Reducer/status assembly derive runtime truth once.
6. `route_plan` selects the public route and carries route metadata.
7. Status/operator/doctor/handoff presentation copy the selected route and add display context without revising route semantics.

This plan must reduce conceptual surface area. Prefer deleting compatibility shims, scanner duplicates, and repeated prose over adding another guard unless a guard protects an actual runtime edge.

## Change Surface

- `src/execution/command_eligibility.rs`
- `src/execution/public_route_selection.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/follow_up.rs`
- `src/execution/route_plan/status_projection.rs`
- `src/execution/closure_dispatch.rs`
- `src/execution/public_repair_targets.rs`
- `src/execution/review_route_tokens.rs` or a new focused repair reason module
- `src/execution/read_model_support.rs`
- `src/execution/mod.rs`
- production imports currently using `crate::execution::read_model_support`
- `src/workflow/operator.rs`
- `src/workflow/doctor_dashboard.rs`
- `schemas/workflow-handoff.schema.json`
- `skills/*/SKILL.md.tmpl`
- generated `skills/*/SKILL.md`
- `scripts/gen-skill-docs.mjs`
- `skills/skill-doc-budgets.json`
- `tests/public_replay_churn.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/fixtures/runtime-goldens/public-runtime-routes.json`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- relevant docs: `docs/runtime-architecture.md`, `docs/testing.md`, `docs/featureforge/reference/execution-runtime-module-boundaries.md`

## Preconditions

- Do not use FeatureForge runtime/project skills.
- Use the Rust coding guidance already loaded from `$rust-skills` when changing Rust code.
- Do not let review or audit subagents spawn additional subagents.
- Do not interrupt in-flight executions.
- Before each new audit-loop iteration, run `cargo clean`.
- After each task, run strict Clippy and the full nextest suite with no fail fast before dispatching review.
- If full nextest takes over 4-5 minutes, run `cargo clean`, rerun full nextest, and stop to fix introduced performance issues if it remains over the threshold.
- Reviewers must be clean-context subagents with exact task scope and no permission to spawn subagents.

## Known Footguns / Constraints

- Do not convert diagnostic-only runtime states into hidden repair workflows.
- Do not make status projection select or revise routes.
- Do not replace deleted scanner duplication with another duplicate scanner.
- Do not move mandatory runtime law only into companion docs; keep short top-level rules in generated skills.
- Do not hand-edit generated `SKILL.md` when a `.tmpl` source exists; edit templates and regenerate.
- Do not let archived audit evidence become active prompt surface again.
- Do not weaken Clippy or add lint suppressions without explicit approval.
- Do not cite semantic liveness tests as full public CLI proof.

## Requirement Coverage Matrix

| Requirement | Tasks |
| --- | --- |
| Diagnostic reconcile guessed repair fails closed | Task 1 |
| Route selection has one owner | Task 2 |
| Projection does not rederive route metadata | Task 2 |
| Bootstrap-only stale target helper removed from non-bootstrap dispatch | Task 3 |
| Repair target reason vocabulary centralized | Task 3 |
| `read_model_support` compatibility layer deleted | Task 3 |
| Active skills avoid removed helper names | Task 4 |
| Doctor/test wording points to typed argv/template | Task 4 |
| Canonical route-execution reference replaces repeated prose | Task 5 |
| Prompt budgets tighten after compaction | Task 5 |
| Duplicate scanners/golden noise reduced | Task 5 |
| Handoff recommended skill follows authoritative route | Task 6 |
| Full validation/review loop repeated after each task | Every task |

## Tasks

### Task 1 - Fail closed for diagnostic-only runtime reconcile

#### Spec Coverage

- High finding: diagnostic `runtime_reconcile_required` still authorizes `repair-review-state`.
- Execution runtime checklist: `repair-review-state` cannot loop on same route; runtime reconcile handles targetless stale states.

#### Goal

Make targetless diagnostic reconcile states reject all public mutation attempts unless the selected public route exposes an exact repair command or repair target.

#### Context

Targetless stale reconcile intentionally emits no argv/template/repair target. `decide_public_mutation` currently authorizes `repair-review-state` whenever `phase_detail == runtime_reconcile_required`, regardless of whether the route is actionable.

#### Constraints

- Do not remove legitimate repairable reconcile paths that expose exact route authority.
- Do not use `recommended_command` display text to authorize mutation.
- The guard must use typed route state, repair targets, `next_public_action`, or equivalent exact public route authority.

#### Done when

- A status with targetless diagnostic reconcile and no public repair target rejects guessed `repair-review-state`.
- A repairable reconcile state with exact public route authority remains accepted.
- Public surfaces still expose no argv/template/repair target for targetless reconcile.
- Replay/liveness tests cover the guessed repair attempt.

#### Files

- `src/execution/command_eligibility.rs`
- `tests/public_replay_churn.rs`
- `tests/liveness_model_checker.rs` if semantic coverage needs adjustment
- focused supporting tests near command eligibility if useful

#### Implementation Steps

1. Add a small helper in `command_eligibility.rs` that identifies whether `runtime_reconcile_required` is repairable through typed public route authority.
2. Treat targetless reconcile as diagnostic-only when all of these are true:
   - `phase_detail == DETAIL_RUNTIME_RECONCILE_REQUIRED`
   - no `recommended_public_command_argv`
   - no `recommended_public_command_template`
   - no compatible `next_public_action`
   - no public repair target binding to `repair-review-state`
3. In `decide_public_mutation`, reject `repair-review-state` for diagnostic-only reconcile with a reason code such as `mutation_runtime_reconcile_diagnostic_only`.
4. Preserve the existing rejection for non-repair commands during repairable reconcile.
5. Add a public replay regression that builds or reuses the targetless stale reconcile fixture, invokes `repair-review-state` through the compiled public CLI, and asserts failure/no mutation.
6. Add a focused unit/semantic test only if public replay setup cannot isolate the guard.

#### Validation Expectations

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Targeted before full run: `cargo nextest run --test public_replay_churn`, `cargo test --test liveness_model_checker -- --nocapture`
- Clean-context review against Task 1 after full validation passes.

### Task 2 - Centralize selected route ownership

#### Spec Coverage

- High finding: public-route seed performs route selection.
- High finding: execution reentry target source has two owners.
- Modularization checklist: router/read-model/mutation guards share decision objects.

#### Goal

Make `route_plan` the single owner of final public route selection and route metadata. Public route seed code may prepare inputs, but must not independently decide the public next action. Status projection must copy route metadata instead of rederiving it.

#### Context

`public_route_selection.rs::shared_next_action_seed_from_precomputed_decision` mutates route fields while `route_plan.rs` also selects/overrides route ordering. `route_plan/status_projection.rs` recomputes `execution_reentry_target_source`.

#### Constraints

- Preserve all public JSON fields and existing behavior unless the current behavior reflects split decisioning.
- Do not move mutation/write imports into workflow/operator or projection code.
- Keep route decision tests focused on route ownership, not incidental formatting.

#### Done when

- `public_route_selection.rs` only prepares seed/input data or delegates to `route_plan` for route decisions.
- `RouteDecision` carries `execution_reentry_target_source` from selection to projection.
- `status_projection.rs` no longer calls stale target or next-action authority helpers to recompute `execution_reentry_target_source`.
- Boundary tests assert projection copies route metadata and does not rederive it.
- Public route goldens or targeted tests confirm no behavior regressions.

#### Files

- `src/execution/public_route_selection.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/constructors.rs`
- `src/execution/route_plan/follow_up.rs`
- `src/execution/route_plan/status_projection.rs`
- `src/execution/read_model/public_route_projection.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/runtime_behavior_golden.rs` if route output changes

#### Implementation Steps

1. Map every branch in `shared_next_action_seed_from_precomputed_decision` that changes `phase_detail`, `next_action`, `recommended_public_command`, `recording_context`, `execution_command_context`, or `blocking_task`.
2. Move those final decisions into existing `route_plan` constructors or a new focused route-decision helper.
3. Replace seed mutations with construction of typed inputs consumed by `route_plan`.
4. Remove `execution_reentry_target_source_for_status_projection`; projection should copy `route_decision.execution_reentry_target_source`.
5. Add boundary tests that fail if `status_projection.rs` imports or calls stale-target/next-action helpers for execution reentry target source.
6. Update docs to state public route seed prepares inputs only.
7. Regenerate goldens only after confirming field changes are intentional route-contract changes.

#### Validation Expectations

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Targeted before full run: `cargo nextest run --test runtime_module_boundaries`, `cargo nextest run --test workflow_runtime`, `cargo nextest run --test workflow_shell_smoke`, `cargo nextest run --test runtime_behavior_golden`
- Clean-context review against Task 2 after full validation passes.

### Task 3 - Remove compatibility and vocabulary drift in route support

#### Spec Coverage

- Medium finding: pre-reducer stale target selection leaks into non-bootstrap dispatch.
- Medium finding: public repair target reason-code taxonomy is duplicated as raw strings.
- Signal-to-noise finding: delete `read_model_support` compatibility layer.

#### Goal

Delete the read-model support compatibility layer, keep bootstrap-only helpers in bootstrap-only use, and centralize public repair target reason vocabulary behind typed constants or an enum.

#### Context

`read_model_support.rs` is a re-export shim, but production modules still import it. `closure_dispatch.rs` calls `pre_reducer_earliest_unresolved_stale_task` during review dispatch targeting. Public repair target reason strings are produced and matched in separate modules.

#### Constraints

- Do not break legitimate bootstrap reconstruction before reducer truth exists.
- Do not duplicate reason strings in tests as another source of truth.
- Prefer existing `review_route_tokens.rs` only if the vocabulary belongs there; otherwise add a focused module.

#### Done when

- `src/execution/read_model_support.rs` is deleted.
- `src/execution/mod.rs` no longer exports `read_model_support`.
- Production imports use `crate::execution::status_support` or narrower authoritative modules directly.
- `closure_dispatch.rs` does not use `pre_reducer_earliest_unresolved_stale_task` outside documented bootstrap context.
- Public repair target reasons are centralized and matched through typed helpers/constants.
- Boundary tests enforce the new import and vocabulary boundaries.

#### Files

- `src/execution/read_model_support.rs`
- `src/execution/mod.rs`
- `src/execution/closure_dispatch.rs`
- `src/execution/public_repair_targets.rs`
- `src/execution/route_plan/status_projection.rs`
- `src/execution/review_route_tokens.rs` or new module
- all production imports found by `rg read_model_support`
- `docs/runtime-architecture.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/runtime_module_boundaries.rs`
- `tests/runtime_authority_contracts.rs`

#### Implementation Steps

1. Replace every production `read_model_support` import with the direct authoritative module import.
2. Delete `read_model_support.rs` and remove it from `mod.rs`.
3. Remove doc/test accommodations that allow the compatibility layer.
4. Refactor `closure_dispatch.rs::review_dispatch_task_boundary_target` so post-route dispatch targeting uses public route target, gate snapshot, status blocking task, or route repair target data; keep pre-reducer stale selection only in clearly documented bootstrap code if still needed.
5. Introduce typed repair target reason constants or an enum with methods for public string output.
6. Update `public_repair_targets.rs` and `route_plan/status_projection.rs` to use that shared vocabulary.
7. Add static tests that reject new raw reason literals outside the owning module and reject new production imports of `read_model_support`.

#### Validation Expectations

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Targeted before full run: `cargo nextest run --test runtime_module_boundaries`, `cargo nextest run --test runtime_authority_contracts`, `cargo nextest run --test workflow_shell_smoke`
- Clean-context review against Task 3 after full validation passes.

### Task 4 - Clean active prompt and public-output command vocabulary

#### Spec Coverage

- Medium finding: active generated skills still mention removed workflow helper names.
- Low finding: doctor dashboard external-review rerun wording is imprecise.
- Low finding: public-flow test labels still say "recommended command" where typed argv is used.

#### Goal

Remove active prompt contamination from removed helper names and make all public-facing rerun guidance point to operator JSON plus typed argv/template fields.

#### Context

The problematic helper names appear in generated skills because they remain in templates. Current Node tests reject executable removed-helper forms but not bare mentions. Doctor dashboard text is conditional but not precise. Some test failure labels reinforce display-command wording.

#### Constraints

- Do not hand-edit generated skills without editing templates.
- Do not move mandatory top-level law solely into companion references.
- Keep wording concise and actionable.

#### Done when

- Active generated skills and templates contain no bare removed workflow helper names.
- Node contract tests reject bare removed helper mentions in active generated prompts.
- Doctor dashboard tells agents to rerun `workflow operator --plan <plan> --external-review-result-ready --json` only after external result exists, then follow `recommended_public_command_argv` or `recommended_public_command_template`.
- Test failure labels distinguish display summaries from executable typed argv.
- Generated docs are fresh.

#### Files

- `skills/brainstorming/SKILL.md.tmpl`
- `skills/writing-plans/SKILL.md.tmpl`
- `skills/plan-ceo-review/SKILL.md.tmpl`
- generated `SKILL.md` files
- `src/workflow/doctor_dashboard.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/runtime_instruction_contracts.rs` if it pins exact wording
- `tests/workflow_shell_smoke.rs`
- `tests/public_replay_churn.rs`

#### Implementation Steps

1. Replace bare helper-name guidance in templates with generic wording such as "do not route through removed compatibility-only workflow helpers."
2. Regenerate generated skill docs with `node scripts/gen-skill-docs.mjs`.
3. Add or strengthen Node prompt-surface tests so active generated prompts reject bare removed helper names.
4. Update runtime instruction tests that intentionally pin the old negative wording.
5. Update doctor dashboard text to name the public operator JSON route and typed fields.
6. Rename test failure labels that say "recommended command should execute" when they execute `recommended_public_command_argv`.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Targeted before full run: `cargo nextest run --test workflow_entry_shell_smoke`, `cargo nextest run --test workflow_shell_smoke`
- Clean-context review against Task 4 after full validation passes.

### Task 5 - Reduce prompt/test signal-to-noise without weakening safety

#### Spec Coverage

- Signal-to-noise findings: collapse duplicate active-doc scanners, reduce scanner self-tests, shrink route golden, centralize route-execution reference, tighten prompt budgets.

#### Goal

Delete low-value duplicate enforcement while preserving the high-signal contracts that prevent runtime dead ends and prompt drift.

#### Context

The Node prompt/docs tests already own active markdown and generated-skill checks. Rust public CLI tests should focus on compiled public runtime behavior and Rust source boundaries. Runtime route goldens capture too much incidental JSON. Route-law prose is centralized in the generator but repeated at length in generated skills.

#### Constraints

- Do not remove coverage for public/private command drift.
- Do not remove budget enforcement.
- Do not reduce top-level skill law below the minimum agents need to act correctly.
- Keep goldens for externally visible route contract fields, not incidental compatibility payloads.

#### Done when

- Rust public CLI contract tests focus on public CLI/runtime behavior; active doc/prompt scans live in Node tests unless there is a Rust-only reason.
- Scanner self-tests are factored into a small support scanner test surface, with broad contract files asserting actual repo violations.
- Runtime golden scenarios remain, but serialized shape is narrowed to external route-contract fields:
  - `phase`
  - `phase_detail`
  - `next_action`
  - `review_state_status`
  - `recommended_public_command_argv`
  - `recommended_public_command_template`
  - `required_inputs`
  - blocking/reason codes
  - explicitly necessary route context fields
- `workflow_status` is omitted from execution-route scenarios unless the scenario is specifically about plan/spec review routing.
- One canonical route-execution reference owns detailed typed-argv/template law.
- High-use generated skills keep a short top-level rule and link to the canonical reference.
- Prompt budgets are tightened after actual compaction.

#### Files

- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- optional `tests/support/rust_source_scan.rs`
- `tests/runtime_behavior_golden.rs`
- `tests/fixtures/runtime-goldens/public-runtime-routes.json`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `scripts/gen-skill-docs.mjs`
- `skills/*/SKILL.md.tmpl`
- generated `skills/*/SKILL.md`
- `skills/skill-doc-budgets.json`
- `docs/testing.md`

#### Implementation Steps

1. Inventory Rust doc/prompt scanners that overlap Node prompt/docs tests.
2. Move any missing active-doc assertions to Node tests, then delete duplicate Rust scanners.
3. Factor reusable Rust source scanner fixtures into one support module/test, or delete excessive synthetic fixture cases when they do not guard a real repo contract.
4. Refactor runtime golden serialization to route-contract DTOs.
5. Regenerate route golden fixtures after reviewing the narrowed shape.
6. Add one canonical generated route-execution reference or reuse an existing packaged reference.
7. Replace repeated long route-law blocks in skill templates with short top-level rules plus a reference link.
8. Update generator tests so they assert the short rule and packaged reference, not duplicated prose everywhere.
9. Tighten total and per-skill budget caps to reflect the new generated line counts with minimal slack.
10. Update docs/testing.md to describe the split between public runtime tests, source-boundary tests, Node prompt/docs tests, and route goldens.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Targeted before full run: `cargo nextest run --test public_cli_flow_contracts`, `cargo nextest run --test runtime_module_boundaries`, `cargo nextest run --test runtime_behavior_golden`
- Clean-context review against Task 5 after full validation passes, with explicit signal-to-noise review.

### Task 6 - Align workflow handoff recommendation with authoritative route

#### Spec Coverage

- Low finding: `workflow-handoff` can recommend the wrong review skill on fidelity-blocked approved plans.

#### Goal

Ensure legacy/internal handoff output does not disagree with the authoritative status/operator route.

#### Context

`next_skill` remains route-derived, but `recommended_skill` maps all `pivot_required` phases to writing-plans. For fidelity-blocked approved plans, the authoritative route is plan engineering review/fidelity refresh.

#### Constraints

- Do not reintroduce handoff as a public routing authority.
- Preserve schema compatibility unless removing a field is already intended and tested.
- Prefer deriving `recommended_skill` from the same route object used for `next_skill`.

#### Done when

- `recommended_skill` matches the authoritative route skill for fidelity-blocked approved plans.
- Handoff schema and tests reflect the intended semantics.
- No handoff consumer is told to use writing-plans when the runtime route is plan-eng-review.

#### Files

- `src/workflow/operator.rs`
- `src/workflow/status.rs` if shared route helper is needed
- `schemas/workflow-handoff.schema.json`
- `tests/workflow_runtime.rs`
- `tests/packet_and_schema.rs`

#### Implementation Steps

1. Inspect handoff builder logic for all `pivot_required` mappings.
2. Replace broad phase-to-skill mapping with route-derived skill selection.
3. Add a regression fixture for an `Engineering Approved` plan missing fidelity where `next_skill` and `recommended_skill` both resolve to plan engineering review.
4. Update schema/golden tests if the field description needs clarification.

#### Validation Expectations

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- Targeted before full run: `cargo nextest run --test workflow_runtime`, `cargo nextest run --test packet_and_schema`
- Clean-context review against Task 6 after full validation passes.

### Task 7 - Final whole-plan review and audit loop decision

#### Spec Coverage

- User-required implementation loop and audit loop closure.

#### Goal

Prove the remediation as a whole and decide whether another audit/implementation loop is necessary.

#### Context

Each task must already have passed full validation and task-scoped clean-context review. This task repeats full validation over the entire branch and dispatches a clean-context whole-plan review.

#### Constraints

- Do not skip full verification because targeted validations passed.
- Do not dispatch review before strict Clippy and full no-fail-fast nextest pass.
- Do not let the reviewer spawn subagents.
- If the reviewer finds actionable issues, remediate and repeat full validation/review.

#### Done when

- Strict Clippy passes.
- Full no-fail-fast nextest passes under the performance threshold.
- Node generated-doc checks pass.
- Node codex-runtime tests pass.
- Prebuilt provenance is either verified if touched or explicitly not applicable if untouched.
- Clean-context whole-plan review finds no actionable issues.
- If no actionable issues remain, start the next audit iteration with `cargo clean` and include the signal-to-noise auditor again.

#### Files

- All files touched by Tasks 1-6.

#### Implementation Steps

1. Run full validation.
2. If full nextest exceeds 4-5 minutes, follow the performance protocol.
3. Dispatch a clean-context reviewer with exact base/head metadata and plan path.
4. Remediate any findings and rerun validation/review until clean.
5. Start the next audit loop with `cargo clean` and the full A-I audit subagent set.

#### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- `cargo test --test liveness_model_checker -- --nocapture`
- `node scripts/prebuilt-runtime-provenance.mjs verify --repo-root .` if prebuilts or source fingerprint inputs changed
