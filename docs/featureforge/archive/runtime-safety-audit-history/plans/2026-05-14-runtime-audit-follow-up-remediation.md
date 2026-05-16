# Workflow State

Engineering Approved

# Plan Revision

Revision 1 - 2026-05-14

# Execution Mode

featureforge:executing-plans

# Goal

Remediate the latest runtime safety audit findings by closing the remaining plan-fidelity approval loop, proving the FS-17 closure-recording replay through the compiled public CLI, folding persisted execution-reentry routing into the canonical route decision, centralizing late-stage freshness reason codes, and keeping the new coverage high-signal rather than adding another layer of brittle scanner churn.

# Architecture

- Runtime and workflow routing must converge on one executable public route. Presentation strings may describe the route, but typed public argv/template surfaces remain the only executable contract.
- Plan-fidelity review evidence is a parseable review artifact. It must prove the final implementation handoff without requiring hidden runtime proof reconstruction or creating a last-header-mutation stale loop.
- Route decisions must be selected once from shared planning facts, then finalized for presentation. A post-selection patch that rewrites phase, next action, command surfaces, and blockers is split decisioning.
- Late-stage freshness reason-code vocabulary belongs behind one shared module surface. Callers should not repeat literal arrays when classifying release, final-review, or QA freshness drift.
- Public replay tests should cover historical dead ends through the compiled public CLI. Internal helper tests can remain, but they cannot be the only proof for a public-flow failure class.
- Signal-to-noise matters: add behavioral tests for user-facing contracts and delete or centralize duplicated logic. Do not add broad static scanners unless the scanner directly protects a previously shipped failure class.

# Change Surface

- `src/contracts/plan.rs`
- `src/workflow/status.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/**`
- `src/execution/current_truth.rs`
- `src/execution/review_route_tokens.rs`
- `src/execution/status_assembly/late_stage.rs`
- `src/execution/route_plan/next_action_choice/late_stage_public_routes.rs`
- `src/execution/closure_graph.rs`
- `skills/plan-eng-review/SKILL.md.tmpl`
- generated `skills/plan-eng-review/SKILL.md`
- `tests/workflow_runtime.rs`
- `tests/public_replay_churn.rs`
- `tests/fixtures/runtime-remediation/README.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`

# Preconditions

- Do not run FeatureForge runtime workflows or project skills.
- Do not allow subagents to spawn additional subagents.
- Use the requested Rust guidance while writing or refactoring Rust.
- Before every full test cycle, verify no `cargo`, `rustc`, `cargo nextest`, `cargo-nextest`, `nextest run`, or active `target/debug/deps/` process is already running.
- After each task implementation, run strict clippy and the full no-fail-fast nextest suite before dispatching a clean-context task review.
- If the full nextest suite takes more than 4-5 minutes, run `cargo clean`, rerun the suite, and remediate any repeatable performance regression. If it exceeds 10 minutes, stop immediately and enter the clean/rerun/performance remediation path.
- Before the next audit-loop iteration, run `cargo clean`.
- Keep all existing dirty worktree changes that are unrelated to this plan.

# Known Footguns / Constraints

- Do not weaken arbitrary plan-fingerprint freshness. Only the final `Draft` to `Engineering Approved` header transition may be treated as approval-stable, and only for plan-fidelity review binding.
- Do not let a plan-fidelity artifact survive real content edits, source spec drift, plan revision drift, required-surface omissions, reviewer provenance defects, or a non-pass verdict.
- Do not fix the plan-fidelity loop solely by telling agents to run another manual review after approval if the public route still bounces.
- Do not add public replay coverage that shells through internal helper commands.
- Do not preserve a route-decision post-patch that rewrites the selected command surface after route selection.
- Do not introduce another duplicate reason-code list while centralizing freshness vocabulary.
- Do not add new static scanner tests for incidental module shape unless they replace lower-signal assertions or guard a concrete historical public failure.

# Requirement Coverage Matrix

| Requirement | Task Coverage |
| --- | --- |
| Final plan-fidelity review remains current across the last approval-header mutation | Task 1 |
| Engineering-approved implementation handoff still requires pass verdict and all five fidelity surfaces | Task 1 |
| Active plan-eng-review guidance no longer teaches a stale final-draft fingerprint loop | Task 1 |
| FS-17 truthful replay is proven through compiled public CLI only | Task 2 |
| Runtime-remediation inventory accurately distinguishes public replay coverage from internal compatibility coverage | Task 2 |
| Persisted execution-reentry follow-up is selected by the route decision path, not patched afterward | Task 3 |
| Route finalization remains presentation-only and does not select a different command | Task 3 |
| Late-stage freshness reason-code vocabulary is centralized and reused | Task 4 |
| Workflow presentation size and module-boundary tracking stay honest without adding brittle scanner churn | Task 5 |
| New tests remain high-signal behavioral coverage | Task 5 |

# Tasks

## Task 1 - Make Plan-Fidelity Approval Handoff Approval-Stable

### Spec Coverage

- Final plan-fidelity review artifact becomes stale when `plan-eng-review` flips only `**Workflow State:**` from `Draft` to `Engineering Approved`.

### Goal

Allow a current pass plan-fidelity artifact produced for the final engineering-reviewed draft to remain current after the exact final approval-header mutation to `Engineering Approved`, without allowing any other plan edit to bypass fidelity freshness.

### Context

`evaluate_plan_fidelity_review` currently binds artifacts to `sha256_hex(plan.source.as_bytes())`. The active `plan-eng-review` guidance requires a pass artifact for the final draft fingerprint and then flips `**Workflow State:** Engineering Approved` as the last step. That single header mutation changes the raw source fingerprint, so `workflow status` can route back to `plan-eng-review` with `engineering_approval_stale_plan_fidelity_review` even though the intended final review just passed.

### Constraints

- Keep raw `plan_fingerprint` in analyze-plan reports unchanged unless there is a deliberate contract reason to change it.
- Introduce a plan-fidelity-specific binding fingerprint or equivalent comparison that normalizes only the `**Workflow State:** Draft` / `**Workflow State:** Engineering Approved` difference for plans already reviewed by `plan-eng-review`.
- Preserve stale classification for changes to task text, files, requirement coverage, source spec fields, plan revision, last reviewer, execution mode, QA requirement, or any other content.
- Preserve stale classification for a draft plan still reviewed by `writing-plans`.
- Preserve pass verdict, reviewer provenance, distinct-stage, required-surface, required-requirement, source spec fingerprint, and source spec revision checks.
- Update guidance so the final approval flow describes the approval-stable plan-fidelity binding accurately.

### Done when

- A test proves a plan-fidelity artifact created against the final draft remains current after only `**Workflow State:** Engineering Approved` is applied.
- Existing tests still reject missing, stale, failed, invalid, and incomplete plan-fidelity artifacts.
- The required artifact template uses the same plan-fidelity binding fingerprint that validation uses.
- `skills/plan-eng-review/SKILL.md.tmpl` and generated `SKILL.md` no longer describe a raw final-draft fingerprint that the final approval mutation invalidates.
- Strict clippy and full nextest pass before clean-context review.

### Files

- `src/contracts/plan.rs`
- `src/workflow/status.rs`
- `tests/workflow_runtime.rs`
- `skills/plan-eng-review/SKILL.md.tmpl`
- `skills/plan-eng-review/SKILL.md`

### Detailed Implementation Steps

1. Add a small plan-fidelity fingerprint helper in `src/contracts/plan.rs`.
2. Make the helper return the raw source hash for ordinary plans, but for plans with `Last Reviewed By: plan-eng-review`, hash a binding source where only the `Workflow State` header is normalized to the engineering-approved handoff value.
3. Use the helper in `evaluate_plan_fidelity_review`, `evaluate_parsed_plan_fidelity_review_artifact`, and `build_plan_fidelity_review_artifact_template`.
4. Keep analyze-plan `plan_fingerprint` raw unless tests prove the public artifact template contract needs an explicit separate field.
5. Add a regression test that writes a final draft with `Last Reviewed By: plan-eng-review`, writes a current pass artifact from that draft, flips only the workflow-state header to `Engineering Approved`, and asserts workflow status is `implementation_ready` with a pass fidelity report.
6. Add or preserve a neighboring stale test proving a non-header content edit after the artifact still routes to the fidelity/engineering approval gate.
7. Update `plan-eng-review` template text and regenerate the generated skill doc.

### Validation Expectations

- Targeted: `cargo test --test workflow_runtime plan_fidelity -- --nocapture`
- Targeted: `node scripts/gen-skill-docs.mjs --check`
- Required after task: `cargo clippy --all-targets --all-features -- -D warnings`
- Required after task: `cargo nextest run --all-targets --all-features --no-fail-fast`
- Clean-context review against Task 1 after full validation.

## Task 2 - Add FS-17 Compiled Public Replay Coverage

### Spec Coverage

- `FS-17` truthful replay convergence is currently proven by internal helper coverage, while the public replay inventory does not contain an FS-17 compiled CLI replay.

### Goal

Prove the FS-17 dead end through the shipped public CLI: replay must converge through `task_closure_recording_ready` and `close-current-task` without hidden dispatch repair, proof reconstruction, or internal helper calls.

### Context

`tests/fixtures/runtime-remediation/README.md` lists FS-17 coverage, but `tests/public_replay_churn.rs` only explicitly covers FS-11 through FS-16 in the public replay set. The direct FS-17 convergence test lives under `internal_only_compatibility_*` in `tests/internal_plan_execution.rs`.

### Constraints

- Use the compiled public binary/CLI path already used by `tests/public_replay_churn.rs`.
- Do not call `internal_only_*` helpers from the public replay test.
- Do not assert incidental JSON ordering or every compatibility field.
- Assert the route signature and executable public argv/template contract that matters for FS-17 convergence.
- Keep the runtime-remediation README accurate after the new coverage lands.

### Done when

- `tests/public_replay_churn.rs` contains an FS-17 public replay that reaches the closure-recording route and completes the public `close-current-task` step.
- The replay asserts no hidden/debug/compatibility commands are exposed.
- The public replay route does not spin on the same route signature after `close-current-task`.
- `tests/fixtures/runtime-remediation/README.md` lists FS-17 under public replay coverage.
- Strict clippy and full nextest pass before clean-context review.

### Files

- `tests/public_replay_churn.rs`
- `tests/fixtures/runtime-remediation/README.md`

### Detailed Implementation Steps

1. Read the internal FS-17 setup in `tests/internal_plan_execution.rs` and identify the minimal runtime state shape needed for the public replay.
2. Reuse existing compiled-CLI public replay fixture builders where possible instead of introducing a parallel fixture system.
3. Add a public replay test named with `fs17` that starts from the stale truthful replay state, queries public route JSON, executes the recommended public `close-current-task` argv, and verifies convergence.
4. Assert the route uses exact `recommended_public_command_argv` authoritative machine invocation or a completed template, never display-only compatibility text `recommended_command` parsing.
5. Add a route-loop detector assertion if an existing helper supports it.
6. Update the remediation coverage README to list FS-17 in the public replay row.

### Validation Expectations

- Targeted: `cargo test --test public_replay_churn fs17 -- --nocapture`
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 2 after full validation.

## Task 3 - Fold Persisted Execution-Reentry Into Route Selection

### Spec Coverage

- Persisted execution-reentry follow-up currently rewrites a selected route after route selection, duplicating command-selection authority.

### Goal

Make persisted execution-reentry fallback a first-class route candidate/finalization input rather than a post-selection mutation that rewrites phase, next action, blockers, and public command surfaces.

### Context

`route_decision_and_status_from_runtime_state_with_inputs` selects a `RouteDecision`, applies planning-fact overrides, then calls `bind_persisted_execution_reentry_fallback`. That helper can rewrite the route into an execution reentry/reopen command after route selection. `persisted_reopen_target` already exists in `RoutePlanningFacts`, so the same decision can be selected in the route-decision path.

### Constraints

- Preserve the current behavior that an already-selected legal `begin` or `reopen` route wins over persisted fallback.
- Preserve blocking reason code `persisted_execution_reentry_follow_up` where it is part of the public diagnostic contract.
- Preserve command surfaces for the resulting public `reopen` route.
- Do not call status projection before selecting the route solely to decide the fallback.
- Keep finalization presentation-only; finalization may bind status-derived display context but must not choose a different mutation command.

### Done when

- No helper mutates a selected `RouteDecision` from an unrelated route into persisted execution reentry after route selection.
- Persisted execution-reentry fallback is represented as a route candidate or named decision constructor used by selection/override logic.
- Tests that cover persisted execution reentry still pass, and at least one test asserts the selected public command remains stable through finalization.
- The execution runtime module-boundary reference remains accurate.
- Strict clippy and full nextest pass before clean-context review.

### Files

- `src/execution/route_plan.rs`
- `src/execution/route_plan/**`
- `tests/runtime_module_boundaries.rs` or focused route-plan tests as needed
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`

### Detailed Implementation Steps

1. Extract the body of `bind_persisted_execution_reentry_fallback` into a pure constructor that returns `Option<RouteDecision>`.
2. Call that constructor from `apply_route_planning_fact_overrides` or the route selection candidate path only when the selected route is not already a legal `begin` or `reopen`.
3. Remove the mutable post-selection helper call from `route_decision_and_status_from_runtime_state_with_inputs`.
4. Keep one status projection before finalization and one after finalization only if both are still required for existing presentation semantics.
5. Add or update a route-plan unit test to prove persisted execution-reentry fallback produces the expected public `reopen` command without a post-selection command rewrite.
6. Update module-boundary docs if they mention the old post-selection patch path.

### Validation Expectations

- Targeted: `cargo test --test execution_query persisted -- --nocapture` or closest existing route-plan filter.
- Targeted: `cargo test --test runtime_module_boundaries route_plan -- --nocapture`
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 3 after full validation.

## Task 4 - Centralize Late-Stage Freshness Reason Codes

### Spec Coverage

- Late-stage freshness reason-code strings are repeated in multiple runtime modules.

### Goal

Move release, final-review, and browser-QA freshness/missing/stale reason-code vocabulary behind shared constants and predicate helpers, then update consumers to use the shared vocabulary.

### Context

`src/execution/review_route_tokens.rs` already defines some freshness constants, while `current_truth.rs`, `status_assembly/late_stage.rs`, `route_plan/next_action_choice/late_stage_public_routes.rs`, and `closure_graph.rs` still repeat literal reason-code arrays.

### Constraints

- Preserve the public reason-code strings exactly.
- Prefer `const` slices or small predicate helpers over allocating vectors.
- Avoid moving unrelated gate reason codes into the late-stage freshness module.
- Do not add a scanner that locks helper names or line placement. Behavioral and compile-time reuse is enough unless a current boundary test already checks this family.

### Done when

- Release docs, final-review, and browser-QA freshness reason codes have one canonical constant/predicate source.
- Runtime callers use the shared constants/predicates instead of local literal arrays for the same semantic classification.
- Public JSON reason codes remain unchanged.
- Strict clippy and full nextest pass before clean-context review.

### Files

- `src/execution/review_route_tokens.rs`
- `src/execution/current_truth.rs`
- `src/execution/status_assembly/late_stage.rs`
- `src/execution/route_plan/next_action_choice/late_stage_public_routes.rs`
- `src/execution/closure_graph.rs`
- tests only if public behavior or boundary assertions need updates

### Detailed Implementation Steps

1. Add missing/stale/not-fresh constants for release docs, final review, and browser QA to `review_route_tokens.rs`.
2. Add predicate helpers such as `is_release_docs_freshness_reason`, `is_final_review_freshness_reason`, `is_browser_qa_freshness_reason`, and `is_late_stage_freshness_reason` if they simplify callers.
3. Replace local `matches!` blocks and literal slices with the shared helpers.
4. Keep tests focused on unchanged public behavior or existing boundary contracts; do not add a broad source scanner for every constant use.

### Validation Expectations

- Targeted: `cargo test --test runtime_authority_contracts freshness -- --nocapture` or closest matching filters.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 4 after full validation.

## Task 5 - Document Presentation-Module Size And Signal-To-Noise Guardrails

### Spec Coverage

- Workflow presentation modules are large but not tracked by the current module-boundary documentation.
- The latest signal-to-noise audit warns that more scanner/test infrastructure would become self-referential churn.

### Goal

Make the remaining presentation-module size debt explicit and keep the new remediation coverage behavioral and high-signal.

### Context

The execution module-boundary docs track oversized execution modules, but `src/workflow/status.rs` and `src/workflow/operator.rs` remain large presentation modules. The audit did not identify a concrete split-decisioning bug inside them, so this task should avoid a broad split. It should document the boundary and ensure this plan did not add low-value scanner churn.

### Constraints

- Do not split `workflow/status.rs` or `workflow/operator.rs` during this task unless implementation work already reveals duplicated semantic decisioning that must be centralized.
- Do not add static source-shape assertions for workflow presentation size.
- Keep the canonical detailed public route law in `references/operator-route-authority.md`; high-use skills should link or summarize, not duplicate large route-law blocks.
- If docs mention module size, distinguish presentation debt from runtime decisioning authority.

### Done when

- Module-boundary docs acknowledge workflow presentation-module size as tracked debt and explain why it is not the same as execution decision split-brain.
- No new low-signal scanner tests were added for incidental topology during Tasks 1-4.
- Any new tests added by this plan are tied directly to public behavior or a concrete historical failure class.
- Strict clippy and full nextest pass before clean-context review.

### Files

- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `references/operator-route-authority.md` only if needed
- `tests/runtime_module_boundaries.rs` only if an existing assertion needs an update

### Detailed Implementation Steps

1. Add a short tracked-debt section to the module-boundary reference for large workflow presentation modules.
2. State that routing/status/operator must continue to consume shared decision objects rather than recomputing mutation eligibility.
3. Confirm this plan's test additions are behavioral; remove or avoid any new scanner-only assertions that do not guard the audited failures.
4. Run doc generation checks if any generated docs or skills changed in earlier tasks.

### Validation Expectations

- Targeted: `node scripts/gen-skill-docs.mjs --check`
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 5.
- After all tasks, run a clean full audit iteration with the added signal-to-noise subagent.
