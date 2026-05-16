# Runtime Safety Thirty-Second Audit Remediation

## Workflow State

Engineering remediation plan for the current runtime-safety audit loop. This plan is active until every task below is implemented, fully verified, and independently reviewed.

## Plan Revision

1

## Execution Mode

Sequential implementation with full verification and clean-context review after each task. Do not use FeatureForge runtime/workflow commands as workflow participation. Do not use FeatureForge/project skills.

## Goal

Close the actionable thirty-second audit findings while reducing conceptual surface area. The implementation must remove remaining artifact/projection/derived-overlay control-plane traps, centralize duplicated route decisions, keep public JSON executable semantics unambiguous, preserve skill packaging, and avoid adding low-signal scanner churn.

## Architecture

- Runtime event/closure state is the control plane. Markdown review artifacts, projection overlays, review summaries, generated evidence, and source archive lists are audit/read-model surfaces unless a public mutation command explicitly writes authoritative runtime state.
- Derived review-state overlays are caches/projections. Missing overlays may produce diagnostics and explicit repair capability, but must not select the next public route when authoritative event state is sufficient.
- Task-boundary readiness is owned by current task closure state and public aggregate commands. Unit-review and task-verification artifacts may inform audit diagnostics but must not force execution reentry or artifact repair when the public closure route can establish authority.
- Public executable authority remains typed: `recommended_public_command_argv` is the exact machine invocation when present; otherwise use a same-plan operator-materialized `recommended_public_command_template`. `next_action` and display-only command rendering are not executable authority.
- Route target selection and template bindability are semantic decisions. Each must have one execution-owned helper consumed by status assembly, route planning, operator projection, and tests.
- Prompt and docs should point to one canonical route reference instead of repeating route law. Source archive verification must cover every shipped companion asset that active skills reference.
- Test performance is a product constraint. Semantic tests should avoid subprocess and replay churn when the subprocess boundary is not the contract under test.

## Change Surface

- `docs/featureforge/plans/**`
- `docs/featureforge/archive/runtime-safety-audit-history/**`
- `scripts/verify-source-archive.mjs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `skills/**/SKILL.md.tmpl`
- generated `skills/**/SKILL.md`
- `src/execution/status_assembly.rs`
- `src/execution/status_assembly/blocking_records.rs`
- `src/execution/status_assembly/exact_route_template.rs`
- `src/execution/status_support.rs`
- `src/execution/closure_diagnostics.rs`
- `src/execution/review_state.rs`
- `src/execution/route_plan.rs`
- `src/execution/route_plan/follow_up.rs`
- `src/execution/command_eligibility/execution_target.rs`
- `src/execution/stale_target_selection.rs` or a better execution-owned selector module
- `src/execution/status.rs`
- `src/workflow/status.rs`
- `src/execution/commands/common/mutation_guards.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_authority_contracts.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/bootstrap_smoke.rs`
- focused support modules in `tests/support/**`

## Preconditions

- Do not use FeatureForge skills or project skills.
- Do not run FeatureForge workflow/runtime commands as workflow participation.
- Before each full test cycle, verify no `cargo nextest`, `cargo-nextest`, `nextest run`, `cargo test`, or `cargo clippy` process is active.
- Run strict clippy and a full no-fail-fast nextest suite before dispatching each clean-context review.
- If a full suite exceeds 4-5 minutes, run `cargo clean`, rerun, and remediate if repeatable. If it exceeds 10 minutes, stop immediately and apply the clean/rerun/remediation rule.
- Keep generated skill docs synchronized by editing templates and regenerating.
- Preserve historical audit artifacts by archiving completed/superseded plans, not deleting them.

## Known Footguns / Constraints

- Do not replace a real runtime fix with broader prompt warnings.
- Do not add a new static scanner around duplicated logic when the duplicated logic can be deleted or centralized.
- Do not weaken public/private test quarantine.
- Do not make artifact/projection freshness a hidden precondition for public `begin`, `close-current-task`, or `advance-late-stage`.
- Do not hide mandatory runtime law only in companion docs; keep high-use skill instructions compact and actionable.
- Do not break schema consumers silently. When renaming or retiring a misleading public field, update schemas, goldens, docs, and tests in the same task.
- Do not accept performance regressions in the internal compatibility suite. The audit-triggered performance remediation split the slow cycle test into direct semantic checks; keep the full clean nextest suite under the time gate.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| Superseded active plans leave `docs/featureforge/plans` | Task 1 |
| Non-Markdown skill companion assets are source-archive protected | Task 1 |
| Derived review-state overlays are diagnostic/read-model only | Task 2 |
| `repair-review-state` is not selected solely for missing derived overlays when authoritative state is sufficient | Task 2 |
| Pre-closure artifact diagnostics cannot force reentry or hidden artifact repair | Task 3 |
| Current task closure selection has one helper | Task 4 |
| Direct positional reads of `current_task_closures.first()` are statically guarded | Task 4 |
| Execution argv/template bindability has one policy owner | Task 4 |
| `next_action` is schema-documented as non-executable | Task 5 |
| Review-state reconciliation stops exposing instructions as display-command compatibility text | Task 5 |
| Projection rebuild labels do not say `manual_required` | Task 5 |
| External-review-ready guidance only refers to actual external review results | Task 5 |
| Shipped/prebuilt runtime route smoke exists or the gap is explicitly fail-closed | Task 6 |
| Broad prompt scanners are narrowed or replaced by higher-signal checks | Task 6 |
| Performance remediation remains clean | Task 6 |

## Task 1: Active Plan Hygiene And Source Archive Assets

**Spec Coverage:** Superseded active plans leave `docs/featureforge/plans`; non-Markdown skill companion assets are source-archive protected.

**Goal:** Remove active-plan discovery churn and ensure every active skill-local companion asset referenced by generated skills or companion Markdown is protected by source archive verification.

**Context:**

The signal/noise audit found two active audit-remediation plans under `docs/featureforge/plans`: the thirtieth and thirty-first plans. The thirty-first plan is completed and the thirtieth plan was superseded. Keeping both active makes future agents choose between historical work items.

The prompt-surface audit found that `scripts/verify-source-archive.mjs` enumerates Markdown prompt companions but omits non-Markdown skill helpers referenced by those companions. Existing contract tests explicitly check `writing-skills` graph assets, but source archive verification is the packaging authority and should cover the full referenced companion set.

**Constraints:**

- Move superseded/completed plans into `docs/featureforge/archive/runtime-safety-audit-history/plans/`; do not delete them.
- Do not archive the current thirty-second plan.
- Prefer one focused source-archive asset list or discovery helper over another broad scanner.
- If discovery is implemented, keep it skill-local and deterministic. Do not crawl arbitrary repo paths.

**Done when:**

- Only the current remediation plan remains active among the thirtieth/thirty-first/thirty-second audit plans.
- `scripts/verify-source-archive.mjs --check` fails if a referenced skill-local `.js`, `.sh`, `.ps1`, `.dot`, or similarly explicit helper asset is missing.
- `skills/brainstorming/scripts/helper.js`, `skills/brainstorming/scripts/start-server.sh`, `skills/brainstorming/scripts/start-server.ps1`, `skills/brainstorming/scripts/stop-server.sh`, `skills/systematic-debugging/find-polluter.sh`, `skills/writing-skills/graphviz-conventions.dot`, and `skills/writing-skills/render-graphs.js` are covered by source archive verification or a shared exported companion-asset list consumed by it.
- Existing source archive tests continue to pass.

**Files:**

- `docs/featureforge/plans/2026-05-14-runtime-signal-noise-thirtieth-audit-remediation.md`
- `docs/featureforge/plans/2026-05-14-runtime-safety-thirty-first-audit-remediation.md`
- `docs/featureforge/archive/runtime-safety-audit-history/plans/**`
- `scripts/verify-source-archive.mjs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`

**Implementation Steps:**

1. Move the thirtieth and thirty-first audit-remediation plans from `docs/featureforge/plans` to `docs/featureforge/archive/runtime-safety-audit-history/plans`.
2. Add source archive coverage for skill-local non-Markdown helpers referenced from generated skills and companion Markdown. Prefer a named exported list such as `REQUIRED_SKILL_COMPANION_ASSET_SOURCE_ARCHIVE_PATHS`, or a deterministic helper that extracts rooted/skill-local asset paths from already-packaged Markdown and templates.
3. Ensure `verify-source-archive.mjs --check` validates both Markdown companions and non-Markdown helper assets.
4. Update Node contract tests so the verifier and the skill-doc companion reference checks share the same asset expectations where practical.
5. Keep the implementation narrow: do not introduce a repo-wide file crawler that turns any text mention into a packaging requirement.

**Validation Expectations:**

- `node scripts/verify-source-archive.mjs --check`
- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- Full verification gate before review: strict clippy and full no-fail-fast nextest.

## Task 2: Demote Derived Review-State Overlays From Route Authority

**Spec Coverage:** Derived review-state overlays are diagnostic/read-model only; `repair-review-state` is not selected solely for missing derived overlays when authoritative state is sufficient.

**Goal:** Prevent missing derived overlay/cache fields from forcing agents into `repair-review-state` or late-stage re-recording when authoritative event state already proves the current closure/review state.

**Context:**

Current code still pushes `derived_review_state_missing` when `missing_derived_review_state_fields(...)` returns data in `src/execution/status_assembly.rs`. `src/execution/status_assembly/blocking_records.rs` converts that reason into blocking records. `src/execution/route_plan/follow_up.rs` and `src/execution/review_state.rs` then route/perform overlay repair. This is exactly the old projection-as-control-plane failure mode under a new name.

**Constraints:**

- Do not remove explicit `repair-review-state` as a public command for true stale/unreviewed or structurally inconsistent runtime state.
- Do not stop exposing missing overlay freshness diagnostics. They should remain visible as diagnostic-only/read-model freshness where useful.
- Do not require projection materialization for progress.
- If branch-scope missing authoritative branch closure truly means no current closure exists, keep the proper public aggregate route (`advance-late-stage`) rather than hiding the blocker.

**Done when:**

- Missing derived overlays alone do not create `StatusBlockingRecord`s.
- Missing derived overlays alone do not set a next public route to `repair-review-state`.
- Status/operator JSON can still report missing overlay diagnostics in a diagnostic-only field.
- `repair-review-state` still repairs overlays when explicitly invoked or when true stale/unreviewed/structural review-state defects exist.
- Regression tests cover at least task-scope and branch-scope authoritative-current cases with missing derived overlays.

**Files:**

- `src/execution/status_assembly.rs`
- `src/execution/status_assembly/blocking_records.rs`
- `src/execution/route_plan/follow_up.rs`
- `src/execution/review_state.rs`
- `src/execution/status.rs`
- `src/workflow/status.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/runtime_authority_contracts.rs`
- `tests/execution_query.rs`

**Implementation Steps:**

1. Introduce a clear representation for derived overlay freshness, for example `derived_overlay_diagnostics`, that is separate from blocking route reason codes.
2. Stop adding `REASON_DERIVED_REVIEW_STATE_MISSING` to the same reason-code set consumed by public blocking record derivation unless the underlying authoritative closure is actually missing.
3. Update `derive_public_blocking_records_with_stale_targets` so it cannot choose repair solely because derived overlays are missing.
4. Update route/follow-up decisioning so missing derived overlays do not make `route_requires_review_state_repair(...)` true unless a real stale/unreviewed or structural review-state condition is present.
5. Preserve explicit repair behavior by keeping `review_state::repair_review_state_command` capable of restoring overlays; the change is route authority, not repair capability deletion.
6. Add regression fixtures where authoritative task closure or branch closure exists, derived overlays are absent, and operator/status continue to the real next route instead of repair.
7. Add a negative regression where authoritative closure really is missing and the public aggregate route remains fail-closed/actionable.

**Validation Expectations:**

- Targeted tests for workflow runtime/status/operator overlay-missing fixtures.
- `cargo clippy --all-targets --all-features -- -D warnings`
- Full no-fail-fast nextest before review.

## Task 3: Separate Artifact Diagnostics From Task-Boundary Authority

**Spec Coverage:** Pre-closure artifact diagnostics cannot force reentry or hidden artifact repair.

**Goal:** Ensure unit-review and task-verification artifact parsing cannot decide task-boundary progress. Those artifacts may remain audit diagnostics, but public close/begin/reentry decisions must be driven by runtime-owned closure state and public command eligibility.

**Context:**

`src/execution/status_support.rs` suppresses task-closure diagnostic reason codes only after a current positive task closure exists. Before that, `src/execution/closure_diagnostics.rs` parses unit-review and task-verification artifacts and emits diagnostics such as malformed review artifact or prior verification missing. Those diagnostics can influence readiness/next-action classification and recreate the artifact-as-control-plane failure mode before closure exists.

**Constraints:**

- Do not allow later-task `begin` without authoritative prior-task closure. The authority is the closure record, not review/verification artifact files.
- Do not weaken `close-current-task` input validation. The aggregate close command still requires explicit review/verification result arguments and summary files where public contract requires them.
- Do not delete artifact parsers if they are needed for audit/projection diagnostics. Move their outputs out of the routing authority path.
- Do not revive hidden artifact repair commands.

**Done when:**

- Missing/malformed unit-review or task-verification artifact files cannot by themselves produce `execution_reentry_required`, `task_closure_recording_ready`, or `repair-review-state` routes.
- Pre-closure artifact freshness appears only in diagnostic-only fields/messages.
- Prior-task `begin` still blocks if authoritative current closure is absent.
- Public `close-current-task` remains the route to establish task-boundary authority.
- Tests prove a missing artifact/projection file does not force reentry when public closure recording is the legal route.

**Files:**

- `src/execution/status_support.rs`
- `src/execution/closure_diagnostics.rs`
- `src/execution/route_plan/next_action_choice/**`
- `src/execution/route_plan.rs`
- `src/execution/current_truth.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/public_replay_churn.rs`
- `tests/liveness_model_checker.rs`

**Implementation Steps:**

1. Trace every consumer of `task_closure_recording_diagnostic_reason_codes(...)` and the reason codes emitted from `push_task_closure_pending_verification_reason_codes_for_run(...)`.
2. Split artifact-derived outputs into diagnostic-only freshness data and actual blocking reason codes.
3. Ensure task-boundary begin guards call only current-closure authority checks for prior tasks.
4. Ensure `close-current-task` eligibility is based on active task state, dispatch/current closure authority, and explicit close command inputs, not stale/missing unit-review artifact files.
5. Add tests for missing review artifact, malformed review artifact, and missing task verification artifact before current closure exists. The expected route should be public closure recording or explicit stop diagnostics, not execution reentry or hidden repair.
6. Update liveness/model-checker fixture expectations if they currently treat artifact diagnostics as progress blockers.

**Validation Expectations:**

- Targeted runtime tests for task-boundary and artifact diagnostics.
- `cargo test --test liveness_model_checker`
- Full clippy and nextest before review.

## Task 4: Centralize Current-Closure Selection And Execution Template Policy

**Spec Coverage:** Current task closure selection has one helper; direct positional reads are statically guarded; execution argv/template bindability has one policy owner.

**Goal:** Delete duplicated semantic decisions that can make route/status surfaces diverge.

**Context:**

`src/execution/route_plan.rs` and `src/execution/status_assembly/blocking_records.rs` both use `status.current_task_closures.first()` to choose task repair/record targets. Separately, executable argv validation lives in `src/execution/command_eligibility/execution_target.rs`, while template bindability lives in `src/execution/status_assembly/exact_route_template.rs`.

**Constraints:**

- Preserve existing externally visible route choices unless the current choice is demonstrably wrong.
- The shared current-closure selector must define deterministic ordering and document why.
- Boundary tests should forbid new positional reads outside the helper, but allow tests and the helper itself to inspect vectors.
- Do not move status assembly into command mutation modules.

**Done when:**

- A single execution-owned helper selects the current task closure route target/scope/record id.
- `route_plan.rs` and `blocking_records.rs` consume that helper.
- Boundary tests fail on new production `.current_task_closures.first()` reads outside the helper/allowed tests.
- A single command-eligibility helper owns required execution args and bindable template placeholders for `begin`, `complete`, and `reopen`.
- `status_assembly/exact_route_template.rs` consumes the command-eligibility helper rather than duplicating its policy.

**Files:**

- `src/execution/stale_target_selection.rs` or new `src/execution/current_task_closure_selection.rs`
- `src/execution/route_plan.rs`
- `src/execution/status_assembly/blocking_records.rs`
- `src/execution/command_eligibility/execution_target.rs`
- `src/execution/status_assembly/exact_route_template.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`

**Implementation Steps:**

1. Add a typed selector such as `current_task_closure_route_target(status: &PlanExecutionStatus) -> Option<CurrentTaskClosureRouteTarget>`.
2. Include task number, scope key, and closure record id in the returned type so callers do not recompute pieces.
3. Replace route-plan fallback selection and blocking-record selection with the helper.
4. Add a source scan/boundary test that rejects direct production `.current_task_closures.first()` reads outside the selector module and documented exceptions.
5. Move template bindability policy from status assembly into command eligibility. The helper should answer whether a `PublicCommandTemplate` satisfies the same required-arg/verification-mode policy as executable argv validation.
6. Replace status assembly template checks with the shared helper.
7. Add unit tests that assert argv validation and template validation agree for begin/complete/reopen success and missing-input cases.

**Validation Expectations:**

- `cargo nextest run --test contracts_execution_runtime_boundaries`
- `cargo nextest run --test public_cli_flow_contracts`
- `cargo nextest run --test runtime_module_boundaries`
- Full clippy and nextest before review.

## Task 5: Clean Public Output Executable Semantics

**Spec Coverage:** `next_action` is schema-documented as non-executable; review-state reconciliation stops exposing prose through the display-command compatibility field; projection rebuild labels do not say `manual_required`; external-review-ready guidance only refers to actual external review results.

**Goal:** Remove public-output wording and JSON field traps that can send agents into display-string execution, manual artifact repair, or premature external-review-ready flags.

**Context:**

The public-output audit found that `next_action` schema properties are bare `$ref`s, review-state reconciliation emits prose under the display-command compatibility field, projection rebuild failures label projection-only progress as `manual_required`, and task-boundary remediation text says "external review or verification result" for `--external-review-result-ready`.

**Constraints:**

- Do not make `next_action` executable. Document it as diagnostic/display context.
- Do not leave prose in fields named like command recommendations.
- If a public field is renamed, update schemas, goldens, tests, docs, and generated skill docs in the same task.
- Do not add a second route reference. Use `references/operator-route-authority.md` as the canonical route-law pointer.

**Done when:**

- `next_action` schema properties in plan-execution status, workflow operator, and workflow handoff explicitly say they are not executable authority.
- Review-state reconciliation output uses a field such as `post_repair_instruction` or `operator_requery_instruction` for prose follow-up, not the display-command compatibility field.
- Projection-only rebuild failure labels use `projection_only`, `projection_export_not_progress_route`, or another non-manual label.
- Public remediation text mentions `--external-review-result-ready` only for actual external review result availability.
- Generated skills mirror the tightened external review wording without repeating extra route law.

**Files:**

- `src/execution/status.rs`
- `src/workflow/status.rs`
- `src/execution/review_state.rs`
- `src/execution/commands/common/mutation_guards.rs`
- `src/execution/status_support.rs`
- `schemas/plan-execution-status.schema.json`
- `schemas/workflow-operator.schema.json`
- `schemas/workflow-handoff.schema.json`
- `skills/executing-plans/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- generated skill docs
- `tests/runtime_authority_contracts.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`

**Implementation Steps:**

1. Update schema postprocessors so `next_action` properties gain descriptions that explicitly direct consumers to typed argv/template for executable authority.
2. Rename or replace the reconciliation output's display-command compatibility field with an instruction field that cannot be mistaken for a command. Update serializers, schemas, tests, and callers.
3. Change projection rebuild target labels and failure classes away from `manual_required`; keep the remediation text telling agents to query operator JSON and stop if no typed route exists.
4. Tighten `task_boundary_public_route_remediation(...)` wording so the flag applies only when an external review result is available.
5. Update skill templates only where they repeat the flag wording; regenerate generated skills.
6. Add contract tests preventing display-command prose in review-state reconciliation and preventing `manual_required` projection-only labels.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check` after regeneration.
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- Relevant Rust contract tests.
- Full clippy and nextest before review.

## Task 6: Test Realism, Signal/Noise, And Performance Guard

**Spec Coverage:** Shipped/prebuilt runtime route smoke exists or the gap is explicitly fail-closed; broad prompt scanners are narrowed or replaced by higher-signal checks; performance remediation remains clean.

**Goal:** Keep the safety suite proving real behavior without becoming self-referential churn.

**Context:**

Public-flow tests strongly exercise the cargo-built binary, but checked-in/prebuilt runtime artifacts are not replayed through typed public routes. The signal/noise auditor also flagged broad forbidden-term lists that can force euphemisms instead of deleting low-value wording. During this audit, full nextest initially exceeded the user time gate; the internal cycle test was refactored and the clean full run returned to 3:21.31.

**Constraints:**

- Do not turn every public-flow test into a prebuilt binary test. Add one representative route smoke or a fail-closed packaging assertion.
- If a prebuilt route smoke is impractical on the current platform, make the test explicitly explain why it is skipped/fail-closed rather than silently passing.
- Prefer deleting or narrowing broad prompt bans. Do not add another broad vocabulary list unless it replaces an older noisier one.
- Preserve the performance gain from the internal cycle test refactor.

**Done when:**

- A representative shipped-runtime/prebuilt smoke verifies typed route output, or the test suite explicitly records a platform/package boundary that makes the route smoke unavailable.
- Public-flow cargo-built tests remain the main behavioral coverage.
- Broad forbidden-term scanning is narrowed to actionable hidden-helper/display-command/manual-repair leakage, with reduced euphemism pressure.
- The performance-refactored internal cycle tests remain in place and are covered by full nextest.

**Files:**

- `tests/bootstrap_smoke.rs`
- `tests/internal_bootstrap_smoke.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/internal_plan_execution.rs`
- `tests/internal_workflow_runtime.rs`
- `docs/testing.md`

**Implementation Steps:**

1. Inspect current bootstrap/prebuilt smoke coverage and identify the lowest-cost representative typed-route check.
2. Add a shipped-runtime smoke that queries a tiny public route fixture through `bin/featureforge` or the platform prebuilt when available. Assert typed argv/template semantics, not only help/version.
3. If platform prebuilt execution cannot be made deterministic, add an explicit test assertion documenting the boundary and preserving cargo-built public-route coverage as the behavioral source.
4. Review `ACTIVE_DOC_ONLY_FORBIDDEN_TERMS` and related arrays. Remove or narrow broad phrases that only encourage wording games; keep explicit hidden command, internal flag, artifact repair, manual artifact repair, and display-command execution bans.
5. Keep the already-applied performance refactor in `tests/internal_plan_execution.rs` and verify no other new long-tail tests appear in the full suite.
6. Update `docs/testing.md` if it needs to distinguish public route behavior tests from shipped artifact smoke tests.

**Validation Expectations:**

- Targeted bootstrap/public-flow tests.
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
- `cargo clippy --all-targets --all-features -- -D warnings`
- Full clean no-fail-fast nextest must stay under the time gate.

## Whole-Plan Final Verification

After all tasks pass their individual verification and clean-context review loops:

1. Verify no cargo/nextest process is active.
2. Run `cargo clean`.
3. Run:
   - `node scripts/gen-skill-docs.mjs --check`
   - `node scripts/gen-agent-docs.mjs --check`
   - `node --test tests/codex-runtime/*.test.mjs`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo nextest run --all-targets --all-features --no-fail-fast`
4. If full nextest exceeds the user time gate, apply the clean/rerun/performance remediation rule before review.
5. Dispatch a clean-context reviewer against this entire plan. The reviewer must not use FeatureForge/project skills and must not spawn subagents.
6. Remediate any reviewer findings, rerun full verification, and rereview until no actionable issues remain.
