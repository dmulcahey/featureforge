# Runtime Safety Audit Remediation Round 2

## Workflow State

Engineering review approved for immediate execution in this session.

## Plan Revision

Revision: 1

## Execution Mode

Serial implementation with full verification and clean-context review after each task.

## Goal

Eliminate the actionable findings from the latest deep audit loop without adding more self-referential workflow churn. Runtime-owned truth must remain authoritative; projection-only, display-only, or documentation-only artifacts must not become public routing authority. Prompt and test surfaces must stay high signal and avoid duplicating route law that is already centralized.

## Architecture

Runtime routing remains:

CLI args -> command module -> transition guard -> event append -> reducer -> read model -> route decision -> workflow operator presentation.

The changes keep that flow intact by:

- Making projection-only stale late-stage identifiers diagnostic-only at stale-target projection boundaries.
- Moving stale reentry source mapping into the focused repair-target selection owner.
- Centralizing late-stage surface reason vocabulary behind an execution-owned constant/predicate.
- Reusing canonical typed operator-route wording in public text instead of local paraphrases.
- Compacting generated skills so top-level law stays actionable while detailed binding rules live in `references/operator-route-authority.md`.
- Reducing static test/doc duplication where tests were enforcing architecture commentary rather than behavior or import boundaries.

## Change Surface

- Runtime: `src/execution/stale_target_projection.rs`, `src/execution/repair_target_selection.rs`, `src/execution/route_plan/follow_up.rs`, late-stage reason-code owner code, and public diagnostic text.
- Workflow output: `src/workflow/doctor_dashboard.rs`.
- Tests: route-plan unit tests, stale-target projection tests, runtime module boundary tests, public-output contract tests, and codex-runtime skill/doc tests.
- Prompt/docs: `scripts/gen-skill-docs.mjs`, generated `skills/**/SKILL.md`, `skills/skill-doc-budgets.json`, `review/late-stage-precedence-reference.md`, and `docs/testing.md`.

## Preconditions

- Do not run FeatureForge skills or project skills.
- Use `rust-skills` guidance for Rust edits.
- Before any full verification cycle, confirm no `cargo`, `rustc`, or `nextest` process is already running.
- If the full nextest run exceeds 4-5 minutes, run `cargo clean`, rerun once, and remediate repeatable performance regressions. If it exceeds 10 minutes, stop immediately and apply the same clean/rerun/remediation rule.
- After each task, run strict clippy and full nextest with `--no-fail-fast` before dispatching review.
- Clean-context reviewers must not spawn subagents.

## Known Footguns / Constraints

- Projection-only record IDs may still be useful diagnostics, but they must not populate executable stale closure surfaces or set `has_authoritative_stale_target`.
- Do not erase real task/branch stale routing. Only projection-only stale IDs lose public repair authority.
- Do not replace typed route authority with display string parsing.
- Do not move mandatory stop rules solely into companion references. Top-level skill docs must still say what to do: query operator JSON, execute typed argv/template, otherwise stop.
- Generated skills must be regenerated from templates/scripts, not hand-edited.
- Static tests should enforce behavior, public contracts, import boundaries, and hidden-command bans. They should not pin incidental helper names, broad markdown tables, or every compatibility field shape.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| Projection-only evidence cannot drive public repair routing | Task 1 |
| Stale target source truth is centralized | Task 1 |
| Late-stage surface reason vocabulary is centralized | Task 2 |
| Public text points to one typed next step | Task 2 |
| Skills remain high signal and actionable | Task 3 |
| Companion route reference remains canonical | Task 3 |
| Static tests enforce boundaries without duplicating architecture docs | Task 4 |
| Full validation and clean-context review after each task | Every task |

## Task 1: Projection-Only Stale Targets Are Diagnostic-Only

### Spec Coverage

- Evidence/projection control-plane assessment.
- Execution runtime checklist: projection materialization is not progress; projection diagnostics do not trigger reentry.
- Modularization checklist: stale reentry source mapping is not split across selection and presentation.

### Goal

Ensure projection-only late-stage stale IDs never become public repair authority, while preserving real stale task, branch, gate, and negative-result routing.

### Context

The audit found that `apply_late_stage_stale_projection` can store unknown IDs as `stale_projection_only_record_ids`, `stale_targets_from_closure_graph` turns those IDs into `AuthoritativeStaleTargetSource::ProjectionOnly`, and downstream code can treat every branch/milestone target as bound. This lets projection-only IDs enter `stale_unreviewed_closures`, blocking records, and repair routing.

The audit also found that `AuthoritativeStaleReentryTarget::into_execution_reentry_target` collapses all stale sources to `ClosureGraphStaleTarget`, while route presentation compensates by rereading `authority_inputs.authoritative_stale_target`.

### Constraints

- Keep projection-only IDs visible only as diagnostics or internal projection facts if needed.
- Do not allow `ProjectionOnly` to satisfy `is_bound_stale_target`, `has_authoritative_stale_target`, `stale_record_ids`, or public repair target creation.
- Keep `ClosureGraph`, `GateReview`, `GateFinish`, `Preflight`, `NegativeResult`, and `BaselineBridge` semantics intact.
- Remove source-token compensation from route presentation after source mapping is fixed at target creation.

### Done When

- `ProjectionOnly` stale targets are not bound targets.
- `ProjectionOnly` targets are excluded from stale closure record projection.
- `has_authoritative_stale_target` ignores projection-only targets.
- `ExecutionReentryTargetSource` carries the original authoritative stale source token through the selected target.
- Tests prove projection-only milestone IDs route to targetless reconcile or diagnostic-only output, not `execution_reentry_required`.

### Files

- `src/execution/stale_target_projection.rs`
- `src/execution/stale_target_projection/unit_tests.rs`
- `src/execution/repair_target_selection.rs`
- `src/execution/route_plan/follow_up.rs`
- `src/execution/route_plan/unit_tests.rs`
- boundary tests as needed

### Implementation Steps

1. Add a focused predicate on `AuthoritativeStaleTarget` or `AuthoritativeStaleTargetSource` that distinguishes public repair authority from projection-only diagnostics.
2. Use that predicate in `is_bound_stale_target`, stale record ID projection, and authoritative stale target presence checks.
3. Update `AuthoritativeStaleReentryTarget::into_execution_reentry_target` so it maps `AuthoritativeStaleTargetSource` into `ExecutionReentryTargetSource` without losing source identity.
4. Simplify `execution_reentry_target_source_for_route` so it reads the selected `ExecutionReentryTarget.source` rather than compensating from `authority_inputs`.
5. Add unit tests for projection-only stale target exclusion and stale source token preservation for at least one non-closure source.
6. Run targeted tests before the full gate.

### Validation Expectations

- Targeted: `cargo test --lib stale_target_projection -- --nocapture`
- Targeted: `cargo test --lib route_plan -- --nocapture`
- Required gate: strict clippy and full `cargo nextest run --no-fail-fast`
- Clean-context review against Task 1 after the full gate passes.

## Task 2: Centralize Late-Stage Reason Vocabulary And Public Text

### Spec Coverage

- Modularization checklist: phase/reason strings are centralized.
- Public-output and agent-UX checklist: failures point to one public next step.

### Goal

Remove raw `late_stage_surface_not_declared` switches from route/status code and align public text with the typed operator-route contract.

### Context

The audit found the late-stage surface reason string matched in several modules. The public-output audit also found doctor text for `final_review_dispatch_required` can read as a direct review-dispatch instruction instead of a typed operator-route instruction, and one preflight remediation omits template binding guidance.

### Constraints

- Preserve existing user-visible reason code where it is part of public diagnostics.
- Centralize matching through a constant/predicate, not by changing the reason code string.
- Public text should not introduce extra commands. It should point to operator JSON and typed argv/template binding.

### Done When

- `late_stage_surface_not_declared` appears as a single owned constant plus tests/docs references, not as multiple raw semantic switches.
- Route/status code calls a shared predicate/constant.
- Doctor dashboard final-review dispatch wording points to the typed operator route.
- Preflight recovery remediation reuses the compact typed public route authority instead of restating field-level binding.

### Files

- late-stage reason owner module, likely `src/execution/current_truth.rs` or a focused diagnostics module
- `src/execution/status_assembly.rs`
- `src/execution/status_support.rs`
- `src/execution/route_plan/next_action_choice/execution_routes.rs`
- `src/execution/route_plan/next_action_choice/late_stage_repair_routes.rs`
- `src/execution/route_plan/stale_repair_target.rs`
- `src/workflow/doctor_dashboard.rs`
- `src/execution/state/preflight.rs`
- `tests/runtime_module_boundaries.rs`
- public output tests as needed

### Implementation Steps

1. Add a shared public/internal constant for the late-stage surface missing reason and a predicate for matching it.
2. Replace raw string comparisons in status assembly, status support, and route planning with the predicate or constant.
3. Add/update boundary tests so this late-stage reason is part of the centralized reason vocabulary contract.
4. Replace doctor final-review dispatch action text with typed operator-route wording.
5. Replace the preflight remediation copy with the canonical typed operator contract wording or a shared helper.
6. Run targeted public-output and boundary tests before the full gate.

### Validation Expectations

- Targeted: `cargo test --test runtime_module_boundaries -- --nocapture`
- Targeted: public-output/status tests that cover doctor and preflight remediation
- Required gate: strict clippy and full `cargo nextest run --no-fail-fast`
- Clean-context review against Task 2 after the full gate passes.

## Task 3: Compact Generated Skills And Canonicalize Route Guidance

### Spec Coverage

- Prompt-surface checklist.
- Signal-to-noise audit: one canonical route reference, fewer repeated negative rules, budget tightened after deletion.

### Goal

Keep mandatory route law top-level and actionable while moving detailed route binding to `references/operator-route-authority.md`.

### Context

The audit found route-owning skills still repeat route law in multiple generated sections, and tests enforce that repetition. The current route guidance is directionally correct, but high-use skills are close to saturation. The next improvement should delete duplicate wording rather than add more static checks.

### Constraints

- Do not remove the top-level rule that agents must use operator JSON typed argv/template and stop when no typed executable route exists.
- Do not remove hidden-helper bans or reviewer-recursion prompt rules.
- Edit `.tmpl` sources or generator code, then regenerate `SKILL.md`.
- Tighten skill budgets after line-count reduction.

### Done When

- Generated route-owning skills contain one compact top-level control-plane section.
- `{{OPERATOR_ROUTE_AUTHORITY}}` no longer repeats Installed Control Plane law or detailed route binding.
- Non-route-owning skills continue to link to the canonical reference without claiming route ownership.
- Tests assert canonical reference linkage and compact top-level stop law, not repeated field-law prose.
- `skills/skill-doc-budgets.json` is tightened below the new generated total with reasonable headroom.

### Files

- `scripts/gen-skill-docs.mjs`
- `skills/**/SKILL.md.tmpl`
- generated `skills/**/SKILL.md`
- `skills/skill-doc-budgets.json`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/codex-runtime/skill-doc-budget.test.mjs`

### Implementation Steps

1. Shrink `buildInstalledControlPlaneSection` to the irreducible top-level runtime law.
2. Shrink `buildOperatorRouteAuthoritySection` to a canonical reference pointer plus no-manual-repair guard.
3. Remove duplicate route-law prose from high-use route-owning templates where the generated sections already cover it.
4. Regenerate skills.
5. Update codex-runtime tests to enforce compact reference use and absence of detailed route-law duplication.
6. Tighten budgets after measuring generated line counts.

### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-budget.test.mjs`
- Required gate: strict clippy and full `cargo nextest run --no-fail-fast`
- Clean-context review against Task 3 after the full gate passes.

## Task 4: Reduce Static Test And Documentation Churn

### Spec Coverage

- Signal-to-noise audit: prefer behavior and import-boundary checks over static tests around static tests.
- Modularization audit: keep boundary enforcement, remove exact helper-name and doc-table-as-oracle coverage where not an API.

### Goal

Keep boundary protection while deleting brittle duplication from runtime module boundary tests, late-stage precedence docs, runtime goldens, and testing docs.

### Context

The audit found `runtime_module_boundaries.rs` has grown into a static architecture spec, `late-stage-precedence-reference.md` duplicates runtime precedence rows, broad public runtime goldens pin incidental JSON shape, and `docs/testing.md` repeats final-gate command lists in several sections.

### Constraints

- Do not weaken hidden command, import direction, public route, or module ownership tests.
- Do not remove externally visible behavior goldens that protect actual public JSON contract fields.
- If a table duplicates runtime data, prefer generation or a pointer to runtime/operator authority over parity tests against manually maintained markdown.
- Keep docs/testing precise enough for maintainers to know required gates.

### Done When

- Runtime module boundary tests focus on import direction, ownership boundaries, and line-health checks, not exact private helper names.
- Late-stage precedence reference no longer maintains a manually duplicated row table as a second source of truth.
- Public runtime goldens or their tests are narrowed where they only pin incidental compatibility shape.
- `docs/testing.md` has one canonical release matrix with change-scoped sections pointing to extras.

### Files

- `tests/runtime_module_boundaries.rs`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `review/late-stage-precedence-reference.md`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/runtime_behavior_golden.rs`
- `tests/fixtures/runtime_behavior_golden.json` if narrowing is needed
- `docs/testing.md`

### Implementation Steps

1. Replace exact stale-target helper-name requirements with owner/import-boundary assertions and behavior tests already covering ordering.
2. Move line-cap source of truth out of the markdown table or narrow the markdown parsing test to a health check that does not make docs the contract oracle.
3. Replace the late-stage precedence markdown table with a short pointer to runtime/operator authority and command-boundary semantics; update tests to assert no hidden/internal command guidance and no stale memorized chains.
4. Review runtime behavior golden coverage and remove incidental fields only if targeted tests already cover the public contract. Otherwise leave it unchanged and document why.
5. Consolidate repeated gate command lists in `docs/testing.md`.
6. Run targeted docs/tests before the full gate.

### Validation Expectations

- `cargo test --test runtime_module_boundaries -- --nocapture`
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- Targeted runtime golden tests if changed
- Required gate: strict clippy and full `cargo nextest run --no-fail-fast`
- Clean-context review against Task 4 after the full gate passes.
