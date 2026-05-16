# Runtime control-plane and public-output eleventh-audit remediation

**Workflow State:** Draft
**Plan Revision:** 1
**Execution Mode:** implementation
**Source Audit:** `docs/featureforge/reference/2026-05-09-deep-runtime-safety-eleventh-audit.md`
**Last Reviewed By:** audit-remediation

## Goal

Eliminate the remaining actionable eleventh-audit findings so FeatureForge agents can follow shipped public runtime surfaces without being pulled into receipt/projection control-plane churn, display-command execution, or stale plan-fidelity guidance.

## Architecture

This plan preserves the existing runtime architecture:

- CLI parses public intent-level commands.
- Command modules validate public mutations.
- Events append to runtime-owned authoritative state.
- Reducer/read-model project runtime truth.
- `route_plan` owns route decisions and final status/blocker projection.
- Workflow operator/status/handoff present typed public argv/templates and display-only compatibility text.

The remediation must tighten those boundaries rather than add new workflow surfaces.

## Change Surface

- Runtime state and route classification:
  - `src/execution/closure_graph.rs`
  - `src/execution/current_truth.rs`
  - `src/execution/stale_target_projection.rs`
  - `src/execution/read_model.rs`
  - related status assembly/route tests
- Task closure command:
  - `src/execution/commands/close_current_task.rs`
  - `src/execution/commands/common/outputs.rs`
  - `src/execution/commands/common/summary_inputs.rs`
  - public replay/runtime tests
- Public output and route projection:
  - `src/execution/review_state.rs`
  - `src/execution/commands/common/operator_outputs.rs`
  - `src/execution/route_plan/decision.rs`
  - `src/execution/route_plan/blockers.rs`
  - `src/execution/status.rs`
  - `src/workflow/status.rs`
  - generated schemas
  - packet/schema/runtime tests
- Skills/docs:
  - `skills/using-featureforge/SKILL.md.tmpl`
  - generated `skills/using-featureforge/SKILL.md`
  - plan-review instruction tests
- Test guards:
  - `tests/runtime_authority_contracts.rs`
  - `tests/public_cli_flow_contracts.rs`
  - `tests/runtime_module_boundaries.rs`
  - `tests/contracts_execution_runtime_boundaries.rs`
  - `tests/workflow_runtime.rs`
  - `tests/public_replay_churn.rs`
  - `tests/packet_and_schema.rs`
  - `tests/codex-runtime/*.test.mjs`

## Preconditions

- Do not use FeatureForge runtime/project skills for this implementation.
- Use Rust best practices for Rust edits.
- Do not let subagents spawn subagents.
- Run strict clippy and full nextest with no fail-fast after each task before review.
- Regenerate generated skill docs and schemas whenever their templates/generators change.
- Do not edit generated `SKILL.md` files without also editing the corresponding `.tmpl`.
- Do not weaken public/runtime boundary tests.

## Known Footguns / Constraints

- Historical docs and archived specs may mention hidden commands; do not treat archived history as active guidance unless it is presented as current.
- Some internal compatibility tests intentionally exercise hidden helpers; keep them quarantined and explicitly internal.
- `recommended_command` remains compatibility/display text. New code must not parse or execute it.
- `blockers[].next_public_action` and legacy follow-up fields must be marked display-only when they remain.
- Existing public JSON compatibility matters. Removing fields may be riskier than deprecating/renaming/annotating while adding typed alternatives.
- Receipt/projection diagnostics may remain visible as diagnostics, but must not create stale control-plane route authority when runtime-owned current closure/milestone truth is sufficient.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| REQ-001: Receipt/doc/projection freshness diagnostics cannot become stale control-plane route truth when runtime-owned state is authoritative. | Task 1 |
| REQ-002: Idempotent `close-current-task` replay succeeds for already-current pass/pass closures without depending on summary files. | Task 2 |
| REQ-003: Public JSON output must not call display command text authoritative or expose unmarked command-shaped strings. | Task 3 |
| REQ-004: `reconcile-review-state` must consume final route projection and avoid display-command authority. | Task 3 |
| REQ-005: Active plan-fidelity routing guidance must match runtime behavior for non-pass artifacts. | Task 4 |
| REQ-006: Static tests must catch recurrence of receipt control-plane routing, review-state display-command authority, and schema annotation gaps. | Tasks 1, 3, 4 |
| REQ-007: All changes pass strict clippy, full no-fail-fast nextest, Node doc checks, and clean-context review loops. | Task 5 |

## Task 1: Decouple artifact freshness diagnostics from stale route authority

**Spec Coverage:** REQ-001, REQ-006

**Goal:** Receipt/doc/projection freshness reason codes remain diagnostic and do not produce stale targets, `stale_unreviewed` review state, repair-review-state routing, or execution reentry when runtime-owned current closure/milestone truth is sufficient.

**Context:**

- Eleventh audit found `release_docs_state_*`, `final_review_state_*`, `browser_qa_state_*`, `plain_unit_review_receipt_fingerprint_mismatch`, and broad `_stale`/`_not_fresh` suffix matching inside stale routing predicates.
- The runtime still needs true implementation drift such as `review_artifact_worktree_dirty`, `post_review_repo_write_detected`, and `files_proven_drifted` to create actionable stale routing.
- `stale_provenance` may still matter when late-stage stale provenance lacks a current branch closure binding; do not silently remove legitimate fail-closed routing.

**Constraints:**

- Centralize the distinction between control-plane stale reasons and diagnostic artifact/projection freshness reasons.
- Do not scatter new string lists across router/read-model/status code.
- Preserve diagnostic visibility of artifact freshness reason codes in status/operator where useful.
- Add tests proving diagnostic-only reason codes do not emit stale route truth.

**Done when:**

- `reason_code_indicates_stale_unreviewed` no longer treats receipt/doc/projection freshness reason codes or broad suffixes as control-plane stale by default.
- Late-stage `release_docs_state_*`, `final_review_state_*`, `browser_qa_state_*`, and `plain_unit_review_receipt_fingerprint_mismatch` remain diagnostics but do not alone produce `stale_unreviewed` or stale targets.
- True implementation drift still routes stale/reentry where intended.
- Regression tests include the current routing files in receipt-control scans.

**Files:**

- `src/execution/closure_graph.rs`
- `src/execution/current_truth.rs`
- `src/execution/stale_target_projection.rs`
- `src/execution/read_model.rs`
- `tests/runtime_authority_contracts.rs`
- `tests/workflow_runtime.rs`
- `tests/public_replay_churn.rs`
- `tests/runtime_module_boundaries.rs`

**Implementation Steps:**

1. Introduce a focused helper naming the authority distinction, for example `reason_code_indicates_control_plane_stale_unreviewed`, and make `reason_code_indicates_stale_unreviewed` delegate only to control-plane reasons if keeping the old function name is lower churn.
2. Move diagnostic artifact/projection freshness tokens into a separate diagnostic-only helper or constant list.
3. Update `late_stage_stale_unreviewed`, `stale_reason_codes_for_late_stage_projection`, `append_gate_stale_targets`, and closure graph signal handling to use only control-plane stale reasons for route authority.
4. Preserve `files_proven_drifted`, `review_artifact_worktree_dirty`, and `post_review_repo_write_detected` as control-plane stale reasons.
5. Treat `stale_provenance` carefully: it should continue to gate missing-current-closure fail-closed conditions only where a current runtime closure binding is absent or explicitly invalid.
6. Update tests that currently expect `*_not_fresh` reason codes to produce `stale_unreviewed`.
7. Add a regression proving late-stage doc/projection freshness reason codes remain visible diagnostics but do not create stale targets or repair-review-state routes when current runtime closure truth is present.
8. Expand `ROUTING_AUTHORITY_RECEIPT_FREE_FILES` to cover `closure_graph.rs`, `stale_target_projection.rs`, `read_model.rs`, and any new classification module.

**Validation Expectations:**

- `cargo fmt --check`
- `cargo test --test runtime_authority_contracts -- --nocapture`
- `cargo test --test workflow_runtime -- --nocapture`
- `cargo test --test public_replay_churn -- --nocapture`
- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- clean-context review for Task 1

## Task 2: Make already-current task closure replay independent of summary files

**Spec Coverage:** REQ-002

**Goal:** If runtime-owned authoritative state already contains a current pass/pass task closure for the same task, dispatch id, closure id, and reviewed state, `close-current-task` can return already-current success even when the caller's summary files are stale, missing, blank, or moved.

**Context:**

- Current code reads `close_current_task_summary_hashes(args)?` before it can reach summary drift ignore/no-op success in some paths.
- Summary files are command inputs for recording a new closure, but they must not be control-plane prerequisites for idempotent replay of existing authoritative pass/pass closure truth.

**Constraints:**

- Do not skip summary validation when recording a new closure or when the existing current closure is negative/conflicting.
- Do not let a conflicting result input no-op against current state.
- Keep mutation authority and worktree lease cleanup checks intact.
- Preserve the existing ability to ignore summary hash drift for pass/pass current closures.

**Done when:**

- Already-current pass/pass replay can return `already_current` before summary file reads when no runtime mutation is required.
- If postcondition/lease cleanup mutation is required, the command still follows the existing authority model.
- Conflicting inputs still fail closed.
- Public replay covers deleted or blank summary files after a current pass/pass closure exists.

**Files:**

- `src/execution/commands/close_current_task.rs`
- `src/execution/commands/common/outputs.rs`
- `tests/public_replay_churn.rs`
- `tests/workflow_shell_smoke.rs` or `tests/workflow_runtime.rs`

**Implementation Steps:**

1. Add a helper that recognizes an already-current positive closure from runtime-owned fields only: task, dispatch id, closure id, reviewed state, `review_result=pass`, `verification_result=pass`, and current closure status.
2. Before reading summary files, check whether the current authoritative closure is a positive match for incoming pass/pass result arguments.
3. If summary files are unavailable but the authoritative closure match is sufficient and no postcondition/lease mutation is needed, return `already_current` with a reason code such as `summary_artifact_unavailable_ignored_for_current_positive_closure`.
4. If mutation is needed for cleanup/postconditions, keep the existing mutation authorization path; avoid introducing a read-only success that hides required cleanup.
5. Keep exact summary-hash replay for cases where files are present.
6. Add a public CLI replay test that records a task closure, deletes or blanks the summary files, reruns the same public `close-current-task`, and asserts success/no hidden command recommendation.

**Validation Expectations:**

- `cargo fmt --check`
- targeted public replay/shell/runtime test for missing summary already-current replay
- `cargo test --test public_replay_churn -- --nocapture`
- `cargo test --test workflow_runtime -- --nocapture`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- clean-context review for Task 2

## Task 3: Remove display-command authority from public outputs and reconcile routing

**Spec Coverage:** REQ-003, REQ-004, REQ-006

**Goal:** Public JSON outputs no longer present display command text as authoritative, `blockers[].next_public_action` is clearly display-only, embedded schemas carry the same typed-authority warnings, and `reconcile-review-state` derives output from final route projection rather than pre-final display-command fields.

**Context:**

- `authoritative_next_action` currently serializes `recommended_command` display text in `CloseCurrentTaskOutput` and `RepairReviewStateOutput`.
- `reconcile_recommended_command` calls a pre-final route projection and returns `route_decision.recommended_command`.
- `blockers[].next_public_action` is command-shaped and insufficiently annotated.

**Constraints:**

- Prefer additive/deprecating compatibility over removing public fields abruptly unless tests show removal is expected.
- If `authoritative_next_action` remains, make it an intent token or next-action phrase, not a command string. Prefer adding `authoritative_next_action_display_only` or schema descriptions if compatibility requires retaining old shape.
- Ensure typed argv/template fields are the executable authority.
- Do not make liveness or mutation guards fall back to blocker display strings as executable commands.
- Strengthen tests to catch split-line field access and pre-final route use.

**Done when:**

- No public output field named `authoritative_next_action` carries a command-shaped display string.
- `RepairReviewStateOutput` and `CloseCurrentTaskOutput` still expose `recommended_public_command_argv` or templates when an executable public route exists.
- `reconcile-review-state` uses the same final projection as status/operator or returns a neutral operator rerun instruction without display command authority.
- `blockers[].next_public_action` includes display-only metadata or is renamed/annotated so agents cannot confuse it for executable authority.
- Schemas annotate top-level and embedded execution-status route fields consistently.
- Static guards fail on `review_state.rs` accessing `recommended_command` from route decisions as authority.

**Files:**

- `src/execution/review_state.rs`
- `src/execution/commands/common/outputs.rs`
- `src/execution/commands/common/operator_outputs.rs`
- `src/execution/route_plan/decision.rs`
- `src/execution/route_plan/blockers.rs`
- `src/execution/status.rs`
- `src/workflow/status.rs`
- `schemas/*.schema.json`
- `tests/workflow_runtime.rs`
- `tests/internal_workflow_runtime.rs`
- `tests/contracts_execution_runtime_boundaries.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/packet_and_schema.rs`
- `tests/liveness_model_checker.rs`

**Implementation Steps:**

1. Update DTOs so `authoritative_next_action` is either omitted/null for command-shaped follow-ups or carries only the canonical `next_action` phrase/intent. Keep `recommended_command` explicitly display-only.
2. Update `with_close_current_task_operator_blocker_metadata` and all `RepairReviewStateOutput` construction sites to stop copying display command strings into `authoritative_next_action`.
3. Replace `reconcile_recommended_command` with a helper that consumes final route projection, or reduce it to a non-command display summary that tells consumers to rerun workflow/operator JSON and use typed argv/template.
4. Update `Blocker`/`RuntimeBlocker` schema/output with `display_only` metadata for `next_public_action`, or rename/add a field that makes display-only status explicit while preserving old compatibility if needed.
5. Extend schema annotation for `workflow-handoff.execution_status` route fields and nested blocker action fields.
6. Add/strengthen static tests so split-line access to `route_decision.recommended_command` in `review_state.rs` fails unless it is in a display-only renderer with clear comments.
7. Update runtime/liveness tests that compare `authoritative_next_action` to `recommended_command`.
8. Regenerate schemas and any affected goldens.

**Validation Expectations:**

- `cargo fmt --check`
- `cargo test --test public_cli_flow_contracts -- --nocapture`
- `cargo test --test runtime_module_boundaries -- --nocapture`
- `cargo test --test contracts_execution_runtime_boundaries -- --nocapture`
- `cargo test --test workflow_runtime -- --nocapture`
- `cargo test --test packet_and_schema -- --nocapture`
- `cargo test --test liveness_model_checker -- --nocapture`
- schema generation/check command used by this repo, if needed
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- clean-context review for Task 3

## Task 4: Align active plan-fidelity guidance with runtime behavior

**Spec Coverage:** REQ-005, REQ-006

**Goal:** Active `using-featureforge` routing guidance sends non-pass plan-fidelity artifacts back to engineering review for edits, while missing/stale/malformed/non-independent fidelity artifacts still route to plan-fidelity review when appropriate.

**Context:**

- Runtime already routes non-pass fidelity artifacts to `featureforge:plan-eng-review`.
- `plan-fidelity-review` skill already says non-pass returns to engineering review.
- `using-featureforge` collapsed non-pass with missing/stale/malformed/non-independent.

**Constraints:**

- Edit the `.tmpl` source and regenerate generated `SKILL.md`.
- Keep the high-level router concise enough to stay within prompt budgets.
- Update tests that pin the old collapsed wording.

**Done when:**

- `using-featureforge` distinguishes:
  - missing/stale/malformed/non-independent fidelity artifact: route to plan-fidelity review;
  - non-pass fidelity artifact: route to plan-eng-review;
  - matching pass artifact: route to plan-eng-review for final approval/handoff.
- Generated docs are fresh and within budget.
- Node and Rust instruction tests match runtime behavior.

**Files:**

- `skills/using-featureforge/SKILL.md.tmpl`
- `skills/using-featureforge/SKILL.md`
- `tests/runtime_instruction_plan_review_contracts.rs`
- `tests/codex-runtime/*.test.mjs`
- possibly `scripts/gen-skill-docs.mjs` only if generator logic needs shared wording changes

**Implementation Steps:**

1. Update the router bullets in `skills/using-featureforge/SKILL.md.tmpl`.
2. Regenerate skills with `node scripts/gen-skill-docs.mjs`.
3. Update tests that assert the old collapsed wording.
4. Confirm runtime tests already pin non-pass route to engineering review; add a doc-contract assertion if one does not exist.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo test --test runtime_instruction_plan_review_contracts -- --nocapture`
- `cargo test --test workflow_runtime canonical_workflow_status_routes_draft_plan_with_non_pass_fidelity_artifact_to_engineering_review -- --exact`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
- clean-context review for Task 4

## Task 5: Final verification, whole-plan review, and audit loop decision

**Spec Coverage:** REQ-007

**Goal:** Prove the whole remediation is clean, reviewed in a fresh context, and ready for another full audit pass.

**Context:**

- The user requires strict clippy and full nextest before every review.
- The audit/implementation loop continues until no actionable audit issues remain.

**Constraints:**

- Do not dispatch review before strict clippy and full no-fail-fast nextest pass.
- Reviewer must be clean-context and must not spawn subagents.
- If review finds issues, remediate, rerun full validation, and rereview until clean.
- After whole-plan review is clean, run the original A-H audit process again plus a ninth signal-to-noise auditor.
- The signal-to-noise auditor must explicitly check whether runtime, test, schema, and skill changes remain beneficial and high-signal: prefer deleting duplicate routing/status logic over adding guards; prefer one canonical typed-public-route reference over repeated prompt law; keep goldens focused on externally visible behavior; and treat prompt budgets as a forcing function to collapse weaker repeated rules.

**Done when:**

- Full validation passes.
- Whole-plan clean-context review reports no actionable issues.
- A fresh A-H audit is launched and synthesized.
- If that audit finds actionable issues, a new remediation plan is written and implemented.
- If that audit finds no actionable issues, final report recommends ship/ship-after-nonfunctional cleanup as appropriate.

**Files:**

- all changed files
- new audit report generated after final audit

**Implementation Steps:**

1. Run:
   - `node scripts/gen-skill-docs.mjs --check`
   - `node scripts/gen-agent-docs.mjs --check`
   - `node --test tests/codex-runtime/*.test.mjs`
   - `cargo fmt --check`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`
   - `cargo test --test liveness_model_checker -- --nocapture`
2. Dispatch clean-context whole-plan reviewer with no subagents allowed.
3. Remediate any review issues and repeat validation/review.
4. Launch the next full A-I audit, using clean-context parallel auditors and no FeatureForge/project skills.
5. Include Subagent I: signal-to-noise and conceptual-surface auditor. Its mission is to find duplicated workflow law, split decisioning, prompt repetition, overbroad static/golden coverage, schema prose churn, or low-value enforcement that should instead be deleted, centralized, or collapsed behind shared runtime/public-command authority.

**Validation Expectations:**

- All listed commands pass.
- Whole-plan review finds no actionable issues.
- Next audit either finds no actionable issues or produces the next remediation plan.
