# FeatureForge deep runtime safety eleventh audit

**Audit date:** 2026-05-09
**Audited checkout:** `/Users/dmulcahey/.codex/worktrees/5d19/featureforge`
**Audited base HEAD:** `9e85a09a65a99bfa5511b66c0b6b54b587346f1d`

## Executive Verdict

**Do not ship yet.** The runtime is substantially safer than earlier audits, but the eleventh audit found actionable issues in three classes that can still make agents chase FeatureForge semantics instead of implementation work:

- receipt/doc freshness reason codes can still promote passive artifact drift into stale control-plane routing;
- public outputs still contain command-shaped display strings under authority-flavored fields;
- one active router instruction contradicts the runtime for non-pass plan-fidelity artifacts.

No new blocker was found in public CLI reachability, public-flow test realism, prompt budget enforcement, reviewer recursion prevention, or liveness convergence. The remaining issues are targeted and implementable.

## Subagent Coverage

- Subagent A, public CLI and reachable runtime: no actionable findings.
- Subagent B, tests versus shipped runtime realism: no actionable findings.
- Subagent C, receipt/provenance/evidence control plane: High, Medium, Low findings.
- Subagent D, plan-review and engineering-review workflow: High finding.
- Subagent E, stale closure, cycle-break, and reentry loops: no actionable findings.
- Subagent F, prompt surface and skill packaging: no actionable findings.
- Subagent G, modularization and split decisioning: High, Medium, Low findings.
- Subagent H, public-output and agent UX: High, High, Medium, Low findings.

## What Is Genuinely Fixed

- Public `begin`, `close-current-task`, `advance-late-stage`, `repair-review-state`, `transfer`, `reopen`, and `materialize-projections` are the normal shipped runtime surface. No inspected normal route required `plan execution preflight`, `record-review-dispatch`, `gate-review`, `gate-finish`, or primitive late-stage recorders.
- Public route authority is typed. Operator/status surfaces expose `recommended_public_command_argv` and `recommended_public_command_template`; active skill docs teach that `recommended_command` is display-only.
- Public-flow tests are largely realistic. Public replay and shell-smoke tests use the compiled CLI, and direct helpers are quarantined under `internal_only_*` tests/support.
- Prompt budget enforcement is active, generated docs are fresh, companion references are packaged, and reviewer recursion prevention is prompt-scoped.
- `blocked_runtime_bug` is diagnostic-only in the inspected runtime: command surfaces are suppressed and mutation eligibility rejects normal mutation attempts.
- Current task closures are not being projected as stale task-boundary targets after repair.

## What Remains Risky

- Late-stage artifact/provenance freshness reason codes are still treated as stale routing truth by `closure_graph`, `current_truth`, and `stale_target_projection`.
- `close-current-task` still reads summary files before it can return an already-current no-op in some public replay paths.
- `repair-review-state`/`reconcile-review-state` output still uses display-command fields as authority-flavored public fields, including `authoritative_next_action`.
- `reconcile-review-state` computes its follow-up from a pre-final route and returns `recommended_command` display text, bypassing final route projection.
- Embedded schema annotations and blocker action DTOs still leave some command-shaped strings insufficiently marked as display-only.
- One active router instruction sends non-pass plan-fidelity artifacts back to fidelity review instead of engineering review for edits.

## Prioritized Findings

### High: Receipt/doc freshness reason codes still drive stale routing truth

**Type:** user-facing churn/control-plane issue

`closure_graph::reason_code_indicates_stale_unreviewed` treats document/projection/receipt freshness diagnostics as stale control-plane truth:

- `release_docs_state_stale`
- `release_docs_state_not_fresh`
- `final_review_state_stale`
- `final_review_state_not_fresh`
- `browser_qa_state_stale`
- `browser_qa_state_not_fresh`
- `plain_unit_review_receipt_fingerprint_mismatch`
- any reason ending `_stale` or `_not_fresh`

References:

- `src/execution/closure_graph.rs::reason_code_indicates_stale_unreviewed`
- `src/execution/current_truth.rs::late_stage_stale_unreviewed`
- `src/execution/current_truth.rs::stale_reason_codes_for_late_stage_projection`
- `src/execution/stale_target_projection.rs::project_authoritative_stale_targets`
- `src/execution/stale_target_projection.rs::append_gate_stale_targets`
- `src/execution/read_model.rs::derive_public_review_state_status_treats_not_fresh_late_gate_reasons_as_stale_unreviewed`

Why it matters: receipt/doc/projection freshness can still produce `stale_unreviewed` route truth and repair/reentry pressure. That violates the current target state where runtime-owned state is authoritative and docs/evidence/projections are diagnostic or derived.

### High: Non-pass plan-fidelity guidance contradicts runtime routing

**Type:** documentation/prompt issue

Active `using-featureforge` guidance says a Draft plan with `Last Reviewed By: plan-eng-review` and a missing/stale/malformed/non-pass/non-independent plan-fidelity artifact should invoke `featureforge:plan-fidelity-review`.

References:

- `skills/using-featureforge/SKILL.md.tmpl`
- `skills/using-featureforge/SKILL.md`
- `tests/runtime_instruction_plan_review_contracts.rs`
- runtime route: `src/workflow/status.rs::route_for_draft_plan_candidate`
- regression: `tests/workflow_runtime.rs::canonical_workflow_status_routes_draft_plan_with_non_pass_fidelity_artifact_to_engineering_review`

The runtime sends non-pass fidelity back to `featureforge:plan-eng-review` so engineering-review edits happen before the next final fidelity pass. The prompt can send agents into an immediate fidelity bounce.

### High: `authoritative_next_action` carries display command text

**Type:** public-output/agent-UX issue

Public JSON output fields named `authoritative_next_action` are populated with display command strings derived from `recommended_command`.

References:

- `src/execution/commands/common/outputs.rs::CloseCurrentTaskOutput`
- `src/execution/commands/common/operator_outputs.rs::with_close_current_task_operator_blocker_metadata`
- `src/execution/review_state.rs::RepairReviewStateOutput`
- tests reinforcing comparison to `operator["recommended_command"]`: `tests/workflow_runtime.rs`, `tests/internal_workflow_runtime.rs`

Why it matters: the name says authoritative, but the value is display-only command text. This undermines typed argv authority and can train agents to parse or execute display strings.

### High: `reconcile-review-state` uses pre-final route/display command as output authority

**Type:** architecture/split-decisioning issue

`src/execution/review_state.rs::reconcile_recommended_command` loads a read scope, calls `project_runtime_routing_state`, and returns `route_decision.recommended_command`. That bypasses final route/blocker projection used by normal read surfaces and returns display-command text.

References:

- `src/execution/review_state.rs::reconcile_recommended_command`
- final projection owner: `src/execution/router.rs::project_runtime_routing_state_with_reduced_state`
- final projection owner: `src/execution/route_plan/status_projection.rs::finalize_route_decision_for_status_projection`
- read-model consumer: `src/execution/read_model/public_route_projection.rs`

Why it matters: reconcile output can diverge from status/operator after diagnostic suppression or blocker finalization.

### High: Blocker `next_public_action` command text lacks sufficient display-only metadata

**Type:** public-output/agent-UX issue

`blockers[].next_public_action` exposes command-shaped strings. The top-level `next_public_action` has display-only metadata and schema annotation, but embedded blocker actions are less explicit and still look executable.

References:

- `src/execution/route_plan/decision.rs::Blocker`
- `src/execution/route_plan/blockers.rs::materialize_blocker_actions`
- schemas: `schemas/workflow-operator.schema.json`, `schemas/plan-execution-status.schema.json`, `schemas/workflow-handoff.schema.json`
- liveness fallback: `tests/liveness_model_checker.rs`

### Medium: Already-current `close-current-task` replay can fail on missing summary files

**Type:** user-facing dead-end/churn issue

`close-current-task` reads `close_current_task_summary_hashes` before returning already-current no-op success. Missing or blank summary files can block idempotent replay even when authoritative runtime state already contains a current pass/pass closure for the same task, closure id, dispatch id, and reviewed state.

References:

- `src/execution/commands/close_current_task.rs::close_current_task`
- `src/execution/commands/common/summary_inputs.rs::close_current_task_summary_hashes`
- `src/execution/commands/common/summary_inputs.rs::read_nonempty_summary_file`

### Medium: Handoff embedded execution-status schema lacks routing-field annotations

**Type:** schema/documentation issue

Top-level workflow handoff fields are annotated, but embedded `execution_status.recommended_command`, `execution_status.recommended_public_command_argv`, and `execution_status.recommended_public_command_template` do not consistently carry the same display-only/typed-authority descriptions.

References:

- `src/workflow/status.rs::annotate_workflow_handoff_routing_field_schemas`
- `schemas/workflow-handoff.schema.json`

### Medium: Guard for display-command authority in `review_state.rs` is brittle

**Type:** test realism/architecture guard issue

Static guards search for raw substrings such as `route_decision.recommended_command`, but the current violation is split across lines. The guard should scan syntax or normalize whitespace enough to catch field access and require final route projection.

References:

- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `src/execution/review_state.rs`

### Low: Receipt-control regression guard excludes current routing files

**Type:** test coverage issue

`production_routing_authority_uses_artifacts_not_receipts` scans a fixed file set that omits `closure_graph.rs`, `stale_target_projection.rs`, and `read_model.rs`, even though those files currently participate in stale routing.

References:

- `tests/runtime_authority_contracts.rs::ROUTING_AUTHORITY_RECEIPT_FREE_FILES`
- `tests/runtime_authority_contracts.rs::production_routing_authority_uses_artifacts_not_receipts`

### Low: External-review-ready failure wording is underqualified

**Type:** public-output/agent-UX issue

One task-boundary failure message tells agents to run `workflow operator --external-review-result-ready --json` and follow the recommended close command, without restating that review/verification results must already be in hand and without naming typed argv/template authority.

Reference:

- `src/execution/status_support.rs`

### Low: Review-state imports command-module output helpers

**Type:** architecture cleanup

`review_state.rs` imports `PublicFollowUpInputProfile` and `public_recovery_contract_for_follow_up` from `execution::commands::common`, muddying command/read-model boundaries. This is lower priority than the pre-final route/display-command issue.

Reference:

- `src/execution/review_state.rs`
- `src/execution/commands/common/operator_outputs.rs`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`

## Concrete Dead Ends Still Possible

- A stale late-stage doc/projection reason can route the agent into stale review repair even when runtime-owned branch/task closure truth is sufficient.
- A rerun of `close-current-task` for an already-current pass/pass closure can fail because the old summary files were moved, deleted, or blanked.
- An agent or schema consumer can treat `authoritative_next_action` or `blockers[].next_public_action` as executable command authority instead of `recommended_public_command_argv`/template.
- A Draft plan with non-pass fidelity can be rerouted by active docs back into fidelity review before engineering edits.

## Checklist Assessment

### Public CLI / Reachability

- Public `begin` can seed preflight: fixed.
- No normal flow needs `plan execution preflight`: fixed.
- No normal flow needs `record-review-dispatch`: fixed.
- No normal flow needs `gate-review`: fixed.
- No normal flow needs `gate-finish`: fixed.
- No normal flow needs `rebuild-evidence`: fixed for normal routing; compatibility docs remain historical.
- No normal flow needs low-level late-stage recorders: fixed.
- Operator never recommends hidden/debug commands: fixed by inspected routes.
- Status never exposes hidden/debug commands as next actions: fixed by inspected routes.
- Public recommended argv is executable by shipped CLI: fixed by inspected routes.

### Plan Review

- Plan-fidelity no longer uses hidden runtime receipt recording: fixed.
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: fixed enough; template-backed strict parser remains.
- Engineering-review edits do not bounce back to fidelity early: runtime fixed; active `using-featureforge` guidance partially broken for non-pass artifacts.
- Final engineering-approved handoff requires current five-surface fidelity: fixed by inspected runtime/tests.
- Active docs do not teach plan-fidelity receipt recording: fixed.
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed.

### Execution Runtime

- Current task closure is begin-time authority: fixed by inspected paths.
- Current closure cannot appear in stale closures: fixed for task-boundary projection.
- Close-current-task can refresh current dispatch internally: fixed.
- Stale dispatch does not block public close: fixed by inspected routes.
- Receipt/projection diagnostics do not trigger reentry: partially fixed; late-stage doc/receipt freshness still feeds stale routing truth.
- Summary hash drift does not trigger reentry when pass/pass closure is current: partially fixed; missing summary files can still block replay before drift-ignore logic.
- Cycle-break clears after current closure: fixed by inspected tests.
- `resume_task` is not treated as authoritative unless exact command is begin for same task/step: fixed by inspected code/tests.
- Repair-review-state cannot loop on same route: fixed by inspected liveness.
- Runtime reconcile handles targetless stale states: fixed.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed by inspected paths.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: partially fixed; late-stage doc/projection reason classification still needs decoupling.
- Supersession is append-only and does not rewrite proof: fixed by inspected paths.
- Evidence is audit/projection, not control plane: partially fixed; stale reason classification remains.

### Tests

- Public-flow tests do not call internal helpers: fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: fixed.
- Replay tests cover historical dead ends: fixed enough, but add missing replay for close-current-task missing summary idempotence.
- Liveness model catches repeated route signatures: fixed.
- Node/doc contracts pass: passed in prior full verification and by subagent F.
- Prompt budget test passes: passed in prior full verification and by subagent F.

### Prompt Surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: fixed.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules have cohesive responsibilities: fixed enough.
- No new catch-all module replaces old monoliths: fixed enough.
- Phase/reason strings are centralized: partially fixed; stale diagnostic/control-plane classification needs a clearer owner.
- Public command authority is typed, not string-parsed: mostly fixed; `authoritative_next_action`/reconcile output still regress display-command authority.
- Router/read-model/mutation guards share decision objects: mostly fixed; reconcile still bypasses final projection.
- Import-boundary tests exist: fixed, but need sharper review-state/reconcile guards.

## Validation Results Available During Audit

Subagents ran focused checks:

- `cargo test --test public_cli_flow_contracts`: passed.
- `cargo test --test runtime_authority_contracts`: passed.
- `cargo test --test runtime_module_boundaries -- --nocapture`: passed.
- `cargo test --test contracts_execution_runtime_boundaries -- --nocapture`: passed.
- `cargo test --test workflow_runtime canonical_workflow_status_routes_draft_plan_with_non_pass_fidelity_artifact_to_engineering_review -- --exact`: passed.
- `cargo test --test liveness_model_checker runtime_liveness_model_checker_requires_public_progress_edge -- --nocapture`: passed.
- `cargo test --test public_replay_churn public_replay_cycle_break_clears_on_current_closure_refresh_without_loop -- --nocapture`: passed.
- `cargo test --test public_replay_churn public_replay_real_targetless_stale_reconcile_emits_runtime_reconcile_state_kind -- --nocapture`: passed.
- `cargo test --test public_replay_churn public_replay_current_task_closure_never_reappears_as_stale_after_repair -- --nocapture`: passed.
- `cargo test --test workflow_runtime read_surface_invariant_blocks_current_stale_overlap_on_public_status_and_operator -- --nocapture`: passed.
- `cargo test --test workflow_runtime runtime_remediation_fs11_operator_begin_repair_share_one_next_action_engine -- --nocapture`: passed.
- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/skill-doc-budget.test.mjs`: passed.
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`: passed.
- `node --test tests/codex-runtime/skill-doc-generation.test.mjs tests/codex-runtime/gen-skill-docs.unit.test.mjs`: passed.
- `node scripts/run-codex-runtime-tests.mjs`: passed.

Full strict clippy and full nextest were last run clean after the tenth-audit remediation. They must be rerun after each remediation task in the new plan.

## Recommendation

**Ship only after targeted fixes.** The remediation should be limited to the actionable eleventh-audit findings and should avoid broad workflow rewrites.
