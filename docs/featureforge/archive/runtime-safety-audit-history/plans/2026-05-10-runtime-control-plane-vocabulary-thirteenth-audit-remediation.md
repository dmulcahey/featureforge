# Runtime control-plane and vocabulary remediation

## Workflow State

Draft remediation plan from the thirteenth deep runtime-safety audit loop.

## Plan Revision

2026-05-10.1

## Execution Mode

Sequential implementation with full validation and clean-context review after each task.

## Goal

Close the remaining actionable audit findings without adding self-referential churn:

- Current pass/pass task closure records remain the task-boundary authority even if dispatch-lineage projection is missing.
- Stale provenance, summary freshness, and evidence fingerprint drift stay diagnostic-only after authoritative task and branch closure state exists.
- `advance-late-stage` mutation eligibility validates the route operation through typed public command authority instead of broad command-kind matching.
- Public follow-up/output vocabulary and requery codes come from one typed source, not repeated raw literals across routing, command output, and status assembly.
- Public remediation text consistently points agents to `recommended_public_command_argv` or `recommended_public_command_template.input_bindings`, never display-command folklore.
- Module-boundary enforcement is driven by the documented focused-module list, including new semantic selector modules.
- The current branch keeps audit evidence useful without expanding active-looking prompt/docs surface.

## Architecture

The runtime keeps this authority chain:

1. CLI args parse into a typed public command or mutation request.
2. Command modules load shared status/route state.
3. Mutation guards compare the exact public request against the typed route decision.
4. Public commands append authoritative events only after the guard passes.
5. Reducer/read model derive runtime truth once.
6. Route planning projects status/operator surfaces from the shared decision object.
7. Public output text instructs agents to execute typed argv/template fields, not display strings.

The remediation must strengthen this chain instead of adding local exceptions:

- Closure records are authoritative control-plane state. Dispatch lineage can be used to populate diagnostics or convenience fields before closure, but a valid current closure suppresses repair routing for missing/stale task dispatch lineage.
- Provenance/evidence staleness can remain in reason/diagnostic fields, but it cannot promote a closed task/branch state to `stale_unreviewed` or expose `repair-review-state` once current closures are sufficient.
- `advance-late-stage` sub-operations are first-class route operations. Mutation requests carry the intended operation mode and the guard compares it against the route's typed `PublicCommand::AdvanceLateStage { mode }`.
- Follow-up tokens and public output codes are centralized in runtime-owned typed helpers.

## Change Surface

- `src/execution/status_assembly.rs`
- `src/execution/route_plan/status_projection.rs`
- `src/execution/command_eligibility.rs`
- `src/execution/command_eligibility/late_stage.rs`
- `src/execution/commands/common/mutation_guards.rs`
- `src/execution/commands/advance_late_stage.rs`
- `src/execution/commands/common/operator_outputs.rs`
- `src/execution/route_plan/follow_up.rs`
- `src/execution/follow_up.rs`
- `src/execution/state.rs`
- `src/execution/state/finish_gate.rs`
- `src/execution/state/runtime_methods.rs`
- `src/execution/status_support.rs`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/runtime_module_boundaries.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/public_replay_churn.rs`
- `tests/runtime_authority_contracts.rs`
- `tests/public_cli_flow_contracts.rs`
- focused unit tests near touched runtime modules

## Preconditions

- Do not use FeatureForge runtime/project skills.
- Do not let review or audit subagents spawn subagents.
- Do not interrupt in-flight executions.
- `cargo clean` was run before the audit iteration that produced this plan.
- Strict Clippy and full nextest passed before the audit.
- If full nextest exceeds 4-5 minutes, run `cargo clean`, rerun full nextest, and stop to fix introduced performance issues if it remains above the threshold.

## Known Footguns / Constraints

- Do not make receipts, summaries, dispatch lineage, evidence markdown, or projection freshness authoritative after valid current closure state exists.
- Do not remove useful diagnostic fields merely to silence stale provenance; demote them from routing authority instead.
- Do not accept broad `advance-late-stage` mutation authorization that ignores route operation mode.
- Do not encode operation-specific routing in display strings.
- Do not add more prompt-law duplication to skills for runtime issues that belong in code.
- Do not hand-edit generated `SKILL.md` files unless their templates change and docs are regenerated.
- Do not modify historical plans/specs just to make working tree status look smaller.

## Requirement Coverage Matrix

| Requirement | Task 1 | Task 2 | Task 3 | Task 4 |
| --- | --- | --- | --- | --- |
| Current closure is task-boundary authority | Yes | No | No | No |
| Dispatch lineage is diagnostic-only after current closure | Yes | No | No | No |
| Stale provenance cannot force reentry after authoritative closure | Yes | No | No | No |
| Public repair targets do not expose provenance-only repair | Yes | No | No | No |
| `advance-late-stage` operation eligibility is typed | No | Yes | No | No |
| Follow-up/output vocabulary is centralized | No | Yes | No | No |
| Public remediation text names typed route fields | No | Yes | Yes | No |
| Module-boundary enforcement uses one source | No | No | Yes | No |
| Signal/noise and evidence docs remain high-signal | No | No | No | Yes |
| Full verification and clean review after each task | Yes | Yes | Yes | Yes |

## Task 1: Demote dispatch lineage and stale provenance after authoritative closure

### Spec Coverage

- Current task closure is begin-time authority.
- Current closure cannot appear stale because dispatch lineage projection is missing.
- Receipt/projection diagnostics do not trigger reentry.
- Summary/provenance hash drift does not trigger reentry when authoritative task and branch closures are current.
- Evidence remains audit/projection, not control plane.

### Goal

Make valid current task/branch closure records sufficient for routing progress even when task dispatch lineage or provenance/evidence freshness signals drift.

### Context

Audit C found two remaining control-plane leaks:

- `tests/workflow_shell_smoke.rs::public_close_current_task_out_of_route_does_not_refresh_dispatch_before_guard` currently expects missing `strategy_review_dispatch_lineage` after a current task closure to route to `execution_reentry_required` and `repair-review-state`.
- `derive_public_review_state_status` still promotes stale provenance and plan/spec fingerprint mismatch to `stale_unreviewed` when current task closures and current branch closure state exist.

### Constraints

- Keep dispatch/provenance diagnostics visible where useful.
- Do not hide real stale implementation work, negative review outcomes, stale current closures, or missing closure baselines.
- Do not weaken begin guards before a valid current closure exists.
- Preserve targetless-stale fail-closed behavior.

### Done when

- Clearing task dispatch lineage after a valid current task closure no longer routes to `execution_reentry_required` or recommends `repair-review-state`.
- Replaying `close-current-task` in that state remains rejected unless it is the exact route, but rejection points to the actual typed route, not a stale dispatch repair route.
- Stale provenance plus current task/branch closures remains diagnostic and does not produce `stale_unreviewed` or public repair targets.
- Regression tests cover the demotion.

### Files

- `src/execution/status_assembly.rs`
- `src/execution/route_plan/status_projection.rs`
- `tests/workflow_shell_smoke.rs`
- `src/execution/read_model.rs`
- `tests/public_replay_churn.rs`
- targeted nearby tests as needed

### Implementation Steps

1. Remove or narrow the `stale_provenance_task_boundary` promotion in `derive_public_review_state_status` so it returns `clean` when authoritative current task closures and current branch closure state are present and no real branch/task stale condition remains.
2. Update `route_decision_exposes_repair_review_state_target` so `REASON_CODE_STALE_PROVENANCE` alone does not expose `repair-review-state` when current closures are present and route state is otherwise clean/diagnostic.
3. Update the public shell-smoke stale-dispatch test to assert the desired behavior after lineage loss: no `execution_reentry_required`, no `repair-review-state` argv, and no stale dispatch hidden-helper terminology.
4. Update unit tests that locked stale provenance to `stale_unreviewed` after closure to assert diagnostic-only status.
5. Add or adjust replay coverage so a stale provenance reason on a closed task/branch state cannot produce public repair targets.

### Validation Expectations

- Targeted tests for changed stale-provenance and dispatch-lineage cases.
- Full strict Clippy.
- Full no-fail-fast nextest under the performance threshold.
- Clean-context task review after the full gate passes.

## Task 2: Centralize public follow-up vocabulary and type `advance-late-stage` operation eligibility

### Spec Coverage

- Public command authority is typed, not string-parsed.
- Runtime modules do not duplicate routing/status/mutation decisions.
- `advance-late-stage` owns late-stage progression but validates sub-operations through the typed route.
- Follow-up/output vocabulary is centralized.

### Goal

Replace broad `PublicMutationKind::AdvanceLateStage` authorization and repeated raw follow-up/output literals with typed helpers that preserve exact public route semantics.

### Context

Audit G found:

- `PublicMutationKind::AdvanceLateStage` is too coarse; mutation matching ignores route operation mode.
- `advance_late_stage.rs` still performs operation-level route checks using operator phase/detail and follow-up strings.
- Follow-up/output tokens such as `request_external_review`, `wait_for_external_review_result`, `run_verification`, `resolve_release_blocker`, and `record_handoff` are repeated across route planning and command output.
- `out_of_phase_requery_required` is repeated as a raw public code.

### Constraints

- The public CLI does not expose a separate mode flag for every intent-only late-stage operation. Intent-only operations must bind their expected mode from the shared route decision/status, not from display strings.
- Do not break existing public argv or templates.
- Keep `advance-late-stage --plan <plan>` usable for branch closure, final-review dispatch, finish-review, and finish-completion when that is the exact typed route.

### Done when

- `PublicMutationRequest` carries an optional `PublicAdvanceLateStageMode`.
- `PublicCommand::AdvanceLateStage { mode }` converts to a mutation request with that mode.
- Mutation matching rejects mismatched `advance-late-stage` modes.
- Command-side `advance-late-stage` guards pass the intended mode derived from supplied args or the shared route phase detail.
- Repeated follow-up public tokens and out-of-phase code literals are centralized behind typed constants/helpers.
- Boundary/static tests cover mode matching and vocabulary centralization.

### Files

- `src/execution/command_eligibility.rs`
- `src/execution/command_eligibility/late_stage.rs`
- `src/execution/commands/common/mutation_guards.rs`
- `src/execution/commands/advance_late_stage.rs`
- `src/execution/follow_up.rs`
- `src/execution/route_plan/follow_up.rs`
- `src/execution/commands/common/operator_outputs.rs`
- `src/execution/status_assembly.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/runtime_authority_contracts.rs`
- targeted command eligibility tests

### Implementation Steps

1. Add `advance_late_stage_mode: Option<PublicAdvanceLateStageMode>` to `PublicMutationRequest`.
2. Update all mutation request construction sites to fill the new field.
3. Update `PublicCommand::to_mutation_request` so typed `AdvanceLateStage` requests carry their mode.
4. Update `public_mutation_requests_match` so `AdvanceLateStage` requires matching mode when either side has one.
5. Add a production helper that derives the supplied late-stage mode from `AdvanceLateStageArgs` and the current shared status phase detail.
6. Change `require_advance_late_stage_public_mutation` to validate the derived mode.
7. Replace raw public follow-up token matches in `route_plan/follow_up.rs` and `commands/common/operator_outputs.rs` with `FollowUpKind` or centralized token helper matches.
8. Add a central `OUT_OF_PHASE_REQUERY_REQUIRED_CODE` constant and use it in command outputs.
9. Update boundary tests to prevent reintroducing broad mode-less `advance-late-stage` mutation guards and raw vocabulary repetition.

### Validation Expectations

- Targeted command eligibility and runtime boundary tests.
- Full strict Clippy.
- Full no-fail-fast nextest under the performance threshold.
- Clean-context task review after the full gate passes.

## Task 3: Normalize public remediation text and module-boundary enforcement

### Spec Coverage

- Public failures are actionable and point to one public next step.
- Status/operator output uses typed argv/template authority.
- Module-boundary tests use one authoritative focused-module list.

### Goal

Remove remaining ambiguous remediation wording and make boundary enforcement derive from the same documented module-cap source of truth.

### Context

Audit H found remediation text that still says to follow a recommended command/next step rather than typed route fields, and one message suggests `--external-review-result-ready` unconditionally. Audit G found focused-module import-boundary tests still use a separate hard-coded list from the documented cap table.

### Constraints

- Do not tell agents to pass `--external-review-result-ready` unless the message explicitly says to do so only after an external review/verification result is actually available.
- Do not add prompt/docs repetition for this; fix runtime output strings and tests.
- Keep module cap documentation as the source of truth for the boundary scanner.

### Done when

- `ensure_prior_task_current_closure_record` remediation text points to `workflow operator --json` and typed route fields, with conditional wording for external review readiness.
- Finish/review gate remediations use the shared typed operator route remediation helper.
- Public-output scanners catch ambiguous "recommended public command sequence" and "recommended public next step" phrasing in active runtime text.
- The focused module import-boundary scan derives focused modules from the documented cap table or otherwise has one authoritative list.
- `src/execution/stale_target_selection.rs` is documented and capped.

### Files

- `src/execution/status_support.rs`
- `src/execution/state.rs`
- `src/execution/state/finish_gate.rs`
- `src/execution/state/runtime_methods.rs`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/runtime_module_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`

### Implementation Steps

1. Add a shared status-support remediation helper for missing prior closure that names `recommended_public_command_argv` and `recommended_public_command_template.input_bindings`.
2. Ensure any `--external-review-result-ready` mention is conditional on already having the external review/verification result.
3. Replace ambiguous gate remediation strings with `public_typed_operator_route_remediation_for_plan`.
4. Extend active runtime diagnostic scanners to reject ambiguous display-command phrasing.
5. Add `src/execution/stale_target_selection.rs` to the module-boundary doc cap table with an explicit focused-module cap.
6. Change the import-boundary scanner to derive focused modules from the documented table, or centralize the list so the doc and scanner cannot drift.

### Validation Expectations

- Targeted public CLI flow and runtime module boundary tests.
- Full strict Clippy.
- Full no-fail-fast nextest under the performance threshold.
- Clean-context task review after the full gate passes.

## Task 4: Signal/noise cleanup and final audit loop

### Spec Coverage

- Prompt and docs surface remains high-signal.
- Generated docs are fresh.
- Audit evidence is useful without becoming active workflow law.
- Another clean audit loop runs with subagents A-I, including signal/noise.

### Goal

Keep the remediation focused on runtime safety and avoid adding more static/prompt infrastructure than the fixes require.

### Context

Audit I judged the runtime/skill changes mixed but acceptable, with the only churn risk being many active-looking untracked audit/remediation artifacts. The next loop should avoid creating more active-looking docs beyond this plan and the final audit synthesis.

### Constraints

- Do not delete or move historical audit evidence unless explicitly needed for the final signal/noise audit.
- Prefer final synthesis over new duplicate prompt law.
- Do not add new skill wording unless a runtime behavior change requires it.

### Done when

- Generated docs remain fresh if any generator-owned surfaces changed.
- The working tree does not add duplicate active-looking audit artifacts beyond the required plan/report artifacts.
- Full validation passes.
- A clean-context whole-plan review finds no actionable issues.
- `cargo clean` runs before the next A-I audit iteration.
- A-I audit subagents run again, including the signal/noise auditor.
- If A-I reports no actionable issues, the loop ends with a ship-after-current-fixes recommendation; otherwise the next remediation loop starts from a new task-contract plan.

### Files

- This plan.
- Any final audit synthesis/report artifact required by the next audit loop.
- Generated docs only if code or templates changed them.

### Implementation Steps

1. Re-run generation checks and regenerate only if necessary.
2. Run strict Clippy and full nextest.
3. Run a clean-context whole-plan review against this plan after validation is clean.
4. Run `cargo clean` before the next audit iteration.
5. Dispatch subagents A-I with the signal/noise auditor included and no subagent spawning allowed.
6. Synthesize the audit into a concise engineering report with exact findings.

### Validation Expectations

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- `cargo test --test liveness_model_checker -- --nocapture`
- Clean-context whole-plan review.
- Clean-context A-I audit.
