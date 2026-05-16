# Workflow State

Engineering Approved

# Plan Revision

Revision 1 - 2026-05-13

# Execution Mode

featureforge:executing-plans

# Goal

Remediate the twenty-eighth runtime safety audit findings by centralizing remaining runtime decision vocabulary, making public gate diagnostics route agents back to typed public workflow surfaces, and reducing low-signal static/prompt guard churn. The target is a smaller conceptual surface: fewer raw decision strings, fewer prose-pinned tests, and public diagnostics that point to one safe public route.

Source audit: `docs/featureforge/archive/runtime-safety-audit-history/2026-05-13-twenty-eighth-audit-report.md`

# Architecture

- Runtime decision vocabulary belongs to named constants and predicates, not repeated raw string literals.
- Task-boundary reason codes remain owned by execution/closure diagnostics or a clearly named execution owner. Workflow/operator may present them but must not define or spell them locally.
- Gate/requery reason codes should be centralized near gate/follow-up semantics so runtime gate production, operator requery, and follow-up classification cannot drift.
- Public gate diagnostics should remain diagnostic, but remediation text must direct agents through public route JSON and typed argv/template surfaces instead of manual artifact reconstruction.
- Static scanner tests should guard concrete historical public-flow failures. They should not pin prose when a typed category or selected binary list can express the same contract.
- Prompt route-law tests should have one exact-content owner. Broader generated-doc tests should assert presence of the generated mode, not duplicate phrase-level content.
- Boundary tests should protect ownership and import direction. Fine-grained line caps should not become a second architecture spec.

# Change Surface

- `src/execution/closure_diagnostics/reason_codes.rs`
- `src/execution/status_assembly/task_state.rs`
- `src/workflow/operator.rs`
- `src/execution/state/runtime_methods.rs`
- `src/execution/follow_up.rs`
- `src/execution/gates.rs`
- `src/execution/mod.rs` if a new focused reason-code module is added
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/support/public_flow_scan.rs`
- `tests/codex-runtime/gen-skill-docs.unit.test.mjs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `docs/testing.md` only if the public-flow explanation needs one canonical home clarified

# Preconditions

- Do not use FeatureForge runtime/project skills.
- Do not let review subagents spawn additional subagents.
- Use the requested Rust guidance when writing or refactoring Rust.
- Before every full test cycle, verify no `cargo`, `rustc`, `cargo nextest`, `cargo-nextest`, `nextest run`, or active `target/debug/deps/` process is already running.
- Before each new audit-loop iteration, run `cargo clean`.
- After each task implementation, run strict clippy and full no-fail-fast nextest before dispatching review.
- If full nextest takes more than 4-5 minutes, run `cargo clean`, rerun the suite, and remediate repeatable performance regressions. If it exceeds 10 minutes, stop immediately and enter clean/rerun/performance remediation.
- Do not weaken hidden/debug command scanners, typed public argv/template contracts, or prompt budget enforcement to reduce noise.

# Known Footguns / Constraints

- Do not replace raw-string duplication with a new catch-all vocabulary module that accumulates unrelated strings without ownership semantics.
- Do not move user-facing route decisions into workflow/operator. Workflow/operator can render runtime decisions; it must not define them.
- Do not weaken task-boundary or finish-gate behavior while centralizing tokens.
- Do not make gate diagnostics executable by inventing non-route command strings. The authoritative public executable contract remains `recommended_public_command_argv` or a bound `recommended_public_command_template` from workflow/operator JSON.
- Do not convert every gate remediation into the same vague message. Preserve the failing domain detail, but make the action route-owned and public.
- Do not delete public-flow scanner coverage for hidden helpers, display-command execution, token-only blocked follow-ups, or stale-dispatch recovery.
- Do not remove prompt-budget enforcement or mandatory-law retention checks.
- Do not add `#[allow(clippy::...)]` or weaken lint policy.

# Requirement Coverage Matrix

| Requirement | Task Coverage |
| --- | --- |
| `task_closure_baseline_bridge_ready` has one owner | Task 1 |
| `finish_review_gate_already_current` has one owner and predicate | Task 1 |
| Workflow/operator and follow-up logic consume centralized reason predicates | Task 1 |
| Public gate remediation tells agents to use workflow/operator JSON and typed argv/template | Task 2 |
| Gate diagnostics preserve domain failure detail without manual artifact repair instructions | Task 2 |
| Fine-grained module line-cap churn is reduced | Task 3 |
| Public-flow scanner exceptions use typed categories instead of prose-pinned strings | Task 4 |
| Public-flow script explanation is documented once | Task 4 |
| Generated route-law exact content has one test owner | Task 5 |
| Broad generated-doc tests assert generated route mode without phrase duplication | Task 5 |

# Tasks

## Task 1 - Centralize Remaining Runtime Decision Vocabulary

### Spec Coverage

- `task_closure_baseline_bridge_ready` is emitted and consumed as a raw string in separate modules.
- `finish_review_gate_already_current` is produced and consumed as a raw string across gate, requery, and follow-up logic.

### Goal

Give both decision tokens a single execution-owned source of truth and make all production callers use named constants or predicates.

### Context

The audit found two remaining split-vocabulary seams:

- `status_assembly/task_state.rs` emits `task_closure_baseline_bridge_ready`, while `workflow/operator.rs` spells the same raw literal for public wording.
- `state/runtime_methods.rs` produces and consumes `finish_review_gate_already_current`, while `follow_up.rs` independently matches the same raw literal.

These are not broad runtime failures today, but they are high-leverage drift risks because exact spelling controls routing presentation, operator requery, and direct follow-up classification.

### Constraints

- Preserve existing public reason-code values.
- Keep task-boundary vocabulary near closure diagnostics unless a more specific existing owner is clearly better.
- Keep finish-gate vocabulary in an execution-owned module that can be imported by both gate runtime methods and follow-up normalization.
- Add predicates where callers need semantic checks so they do not compare raw strings.
- Do not add a broad miscellaneous constants module.

### Done when

- No production file outside the owner spells `task_closure_baseline_bridge_ready`.
- No production file outside the owner spells `finish_review_gate_already_current`.
- Workflow/operator consumes a named predicate or constant for the baseline-bridge-ready condition.
- Gate requery and follow-up classification consume a named predicate or constant for finish-review already-current.
- Boundary tests fail if either token is reintroduced as an unowned production literal.
- Existing behavior and tests remain unchanged.

### Files

- `src/execution/closure_diagnostics/reason_codes.rs`
- `src/execution/status_assembly/task_state.rs`
- `src/workflow/operator.rs`
- `src/execution/state/runtime_methods.rs`
- `src/execution/follow_up.rs`
- `src/execution/mod.rs` if needed
- `tests/runtime_module_boundaries.rs`

### Detailed Implementation Steps

1. Add `TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_BRIDGE_READY` and a predicate if useful under the closure diagnostics reason-code owner.
2. Replace the raw baseline-bridge-ready push in `status_assembly/task_state.rs`.
3. Replace the raw baseline-bridge-ready presentation check in `workflow/operator.rs`.
4. Add a focused finish-gate reason-code owner. Prefer a small module such as `src/execution/gate_reason_codes.rs` if no existing focused owner fits.
5. Define `FINISH_REVIEW_GATE_ALREADY_CURRENT` and a predicate such as `finish_review_gate_already_current_reason_code`.
6. Replace the producer and consumers in `state/runtime_methods.rs`.
7. Replace the direct follow-up match in `follow_up.rs`.
8. Extend `runtime_module_boundaries.rs` so these reason codes are included in the centralized-vocabulary scan.
9. Add targeted assertions that the owner modules contain the constants and that non-owner production sources do not duplicate the raw literals.

### Validation Expectations

- Targeted: `cargo test --test runtime_module_boundaries -- --nocapture`.
- Targeted: `cargo test --lib finish_review_gate_already_current task_closure_baseline_bridge -- --nocapture` or closest supported filters.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 1 after full validation.

## Task 2 - Make Gate Diagnostic Remediation Public-Route Oriented

### Spec Coverage

- Public gate diagnostic remediation still tells agents to regenerate contracts, reports, and evidence references without pointing to workflow/operator JSON and typed public command surfaces.

### Goal

Make gate failure remediation actionable through public route surfaces while preserving domain-specific failure detail.

### Context

`GateDiagnostic.remediation` is serialized through status/operator/doctor surfaces. Several strings in `src/execution/gates.rs` use imperative “Regenerate the contract/report/evidence_refs...” language. That can make agents manually edit or reconstruct proof artifacts instead of returning to the public route.

### Constraints

- Do not erase detailed validation reasons. The agent still needs to know whether plan provenance, spec provenance, report provenance, criterion mapping, or evidence locator shape failed.
- Do not invent direct command strings in gate diagnostics.
- Public remediation wording should consistently say to query workflow/operator JSON and follow typed argv/template, or rerun the owning public workflow surfaced there.
- If a gate is purely diagnostic and no public mutation is available, say so and direct the agent to workflow/operator JSON for the next public step.

### Done when

- Public-facing gate remediation strings no longer instruct plain manual regeneration of contracts, reports, handoffs, or evidence references.
- Gate remediation strings point to workflow/operator JSON and typed argv/template where a route exists.
- Domain details remain in the `details` text or diagnostic portion.
- Tests cover representative contract provenance, report provenance, and evidence-reference failures.
- Public-output scanners reject future gate remediation text that says to manually regenerate proof artifacts without a public route cue.

### Files

- `src/execution/gates.rs`
- `src/execution/status.rs` if DTO docs need clarifying
- `src/workflow/operator.rs` if gate presentation text needs helper reuse
- `tests/public_cli_flow_contracts.rs`
- `tests/packet_and_schema.rs` if gate diagnostics are asserted there
- `tests/codex-runtime/skill-doc-contracts.test.mjs` only if docs/prompts need matching wording changes

### Detailed Implementation Steps

1. Add a helper in `gates.rs` for public route remediation text, for example:
   - preserve the domain-specific action phrase;
   - append “Query workflow/operator JSON for the plan and follow `recommended_public_command_argv` or bind `recommended_public_command_template`; do not hand-edit or reconstruct proof artifacts.”
2. Replace representative provenance and evidence-reference remediation strings with the helper.
3. Include contract, evaluation report, handoff, and evidence-reference validation families where they are public-facing gate diagnostics.
4. Keep non-public internal error text unchanged only if it is not serialized as `GateDiagnostic.remediation`.
5. Add or update a public-output scanner/test that fails on manual “Regenerate the contract/report/evidence_refs” wording without a public route cue.
6. Run targeted gate/packet/schema tests.

### Validation Expectations

- Targeted: `cargo test --test public_cli_flow_contracts public_text_surfaces_do_not_emit_compound_recording_or_failure_actions -- --exact --nocapture` or closest existing diagnostic text tests.
- Targeted: `cargo test --test packet_and_schema runtime_golden_diagnostic_routes_are_diagnostic_only -- --exact --nocapture`.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 2 after full validation.

## Task 3 - Coarsen Runtime Module Size Guards

### Spec Coverage

- Fine-grained focused module line caps now create mechanical churn and act like a second architecture spec.

### Goal

Keep meaningful modularity enforcement while deleting or coarsening brittle per-file line caps.

### Context

`tests/runtime_module_boundaries.rs` contains a large `FOCUSED_RUNTIME_MODULE_LINE_CAPS` table and a line-count test. That table catches some monolith regressions, but it also fails on harmless helper additions and forces file reshuffling. The more useful guard is the large top-level module exception/follow-up check plus import-boundary and ownership tests.

### Constraints

- Do not remove import-boundary tests.
- Do not remove large top-level module exception/follow-up coverage.
- Keep `state.rs` and `mutate.rs` facade caps or replace them with a similarly focused facade-specific guard.
- Do not lose coverage for the focused semantic modules added by the prior plan; keep ownership/import checks for those modules.

### Done when

- The fine-grained `FOCUSED_RUNTIME_MODULE_LINE_CAPS` table/test is deleted or replaced with a coarse, low-churn guard.
- Large top-level module exception/follow-up coverage remains.
- Reduced facade caps remain or are replaced with a focused facade guard.
- Focused semantic module import/ownership tests remain.
- `focused_explicit_import_module_rels` no longer depends on a line-cap table as its source of truth.

### Files

- `tests/runtime_module_boundaries.rs`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md` if the coarse guard needs doc alignment

### Detailed Implementation Steps

1. Inventory uses of `FOCUSED_RUNTIME_MODULE_LINE_CAPS`.
2. Remove the fine-grained line cap test or replace it with a coarse threshold for top-level execution modules.
3. Keep `large_runtime_modules_have_documented_exception_or_followup` as the canonical module-size signal.
4. Keep or narrow `REDUCED_FACADE_LINE_CAPS`.
5. Replace `focused_explicit_import_module_rels` input with an explicit list of modules that need parent-glob/import discipline, or derive it from source roots without line caps.
6. Verify Task 5 semantic modules still have import-direction coverage.

### Validation Expectations

- Targeted: `cargo test --test runtime_module_boundaries -- --nocapture`.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 3 after full validation.

## Task 4 - Replace Prose-Pinned Public-Flow Scanner Metadata With Categories

### Spec Coverage

- Public-flow scanner exceptions are centralized, but the public tests assert explanation prose and long reason strings.
- Public-flow script comments duplicate `docs/testing.md` as the explanation owner.

### Goal

Keep scanner protection for historical public-flow failures while making exception semantics typed and documentation-owned.

### Context

The scanner must continue to reject hidden helpers, display-command execution, token-only blocked follow-ups, and stale-dispatch repair paths. The low-signal part is not those protections; it is tests that assert prose reasons or script-comment fragments.

### Constraints

- Do not weaken public-flow scanner coverage.
- Do not make the protected public-flow set ambiguous.
- Preserve clear human explanations in docs or comments, but do not make tests depend on exact prose.
- `docs/testing.md` remains the canonical explanation of scanner support versus public runtime proof.

### Done when

- Public-flow scanner exception reasons are represented by typed categories or stable enum-like tokens.
- Tests assert categories, not long explanatory prose.
- `tests/public_cli_flow_contracts.rs` still parses script-selected binaries and rejects internal/scanner suites in public proof.
- Script-comment fragment assertions are removed or narrowed to category/selection behavior.
- `docs/testing.md` remains the canonical prose explanation.

### Files

- `tests/support/public_flow_scan.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/public_cli_flow_contracts.rs`
- `scripts/run-public-runtime-flow-tests.sh` only if comments become misleading
- `docs/testing.md` only if explanation needs one canonical sentence

### Detailed Implementation Steps

1. Add a typed category for non-public semantic/internal exclusions, such as `InternalSemanticComparison`, `ScannerSelfTest`, and `SyntheticFixtureSetup`.
2. Update scanner helper APIs to return categories plus optional display text where needed.
3. Update public-flow scanner contract tests to assert categories.
4. Remove exact prose assertions for liveness exclusion.
5. Remove `scripts/run-public-runtime-flow-tests.sh` comment-fragment assertions from `public_cli_flow_contracts`; keep selected-binary and exclusion checks.
6. Run public-flow scanner and public CLI flow contract tests.

### Validation Expectations

- Targeted: `cargo test --test public_flow_scan_contracts -- --nocapture`.
- Targeted: `cargo test --test public_cli_flow_contracts -- --nocapture`.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 4 after full validation.

## Task 5 - Deduplicate Generated Route-Law Prompt Tests

### Spec Coverage

- Exact generated route-law content is asserted in generator unit tests and again in broader generated skill doc contract tests.

### Goal

Keep route-law prompt enforcement while making one test layer own exact wording and the other own generated-mode placement.

### Context

`gen-skill-docs.unit.test.mjs` is the right place to assert exact route-law snippet content and route-owner classification. `skill-doc-contracts.test.mjs` should verify generated skills include the correct generated section mode and mandatory top-level law posture, without rechecking every phrase.

### Constraints

- Do not weaken prompt budget enforcement.
- Do not move mandatory runtime law entirely into references.
- Do not remove tests that prevent route-owning skills from losing full route law.
- Do not remove tests that prevent non-route-owning skills from bloating with full law.

### Done when

- Generator unit tests remain the exact-content owner for route-law snippets.
- Skill-doc contract tests assert route-owning skills include the generated full-law section and non-route-owning skills include compact reference mode without duplicating all route-law phrases.
- Prompt budget tests still pass.
- Generated docs remain fresh.

### Files

- `tests/codex-runtime/gen-skill-docs.unit.test.mjs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `scripts/gen-skill-docs.mjs` only if stable markers are needed
- generated `skills/**/SKILL.md` only if generator output intentionally changes

### Detailed Implementation Steps

1. Identify the exact-content assertions in `gen-skill-docs.unit.test.mjs` and keep them.
2. In `skill-doc-contracts.test.mjs`, replace duplicate phrase-level route-law checks with stable generated-section or mode checks.
3. If the generated sections lack stable markers, add minimal non-user-facing markers or reusable helper logic in tests rather than adding visible prompt text.
4. Verify high-use route-owning skills still keep mandatory top-level law.
5. Verify non-route-owning skills still link to the canonical operator route reference.
6. Run the relevant Node tests and prompt budget test.

### Validation Expectations

- Targeted: `node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs`.
- Targeted: `node --test tests/codex-runtime/skill-doc-budget.test.mjs`.
- Targeted: `node scripts/gen-skill-docs.mjs --check`.
- Required after task: strict clippy and full nextest no fail fast.
- Clean-context review against Task 5 after full validation.
