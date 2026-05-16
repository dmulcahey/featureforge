# Runtime Safety Reaudit Remediation Plan

**Workflow State:** Draft

**Plan Revision:** 1

**Execution Mode:** none

**Source Audit:** `docs/featureforge/reference/2026-05-07-deep-runtime-safety-reaudit.md`

## Goal

Eliminate the remaining public-output, input-template, prompt-packaging, test-realism, and runtime-boundary traps found in the 2026-05-07 deep runtime safety reaudit.

The goal is not to make the branch look cleaner. The goal is to make normal FeatureForge use converge through one public, typed, executable route without hidden helper discovery, manual evidence repair, display-string parsing, install-path confusion, or split decisioning.

## Architecture

The intended runtime flow remains:

1. CLI args enter through public CLI command definitions.
2. Command modules normalize and guard public intent.
3. Commands append authoritative events through the recording/event boundary.
4. Reducer computes runtime truth from event authority.
5. Read model projects reducer truth.
6. Router chooses the public route from shared route/reentry decision objects.
7. Workflow operator/status/doctor present typed public command authority.

This plan preserves event-log authority, guided workflow routing, and review gates. It removes remaining display-text authority and stale public wording. It narrows module boundaries without changing workflow semantics.

## Change Surface

- `src/execution/state/rebuild_evidence.rs`
- `src/execution/state/review_gate.rs`
- `src/execution/command_eligibility.rs`
- `src/execution/public_command_types.rs`
- `src/execution/router.rs`
- `src/execution/read_model.rs`
- `src/execution/event_log.rs`
- `src/execution/review_state.rs`
- `src/execution/commands/close_current_task.rs`
- `src/workflow/operator.rs`
- `src/workflow/doctor_dashboard.rs`
- `schemas/plan-execution-status.schema.json`
- `schemas/workflow-operator.schema.json`
- `schemas/workflow-handoff.schema.json`
- `skills/*/SKILL.md.tmpl`
- generated `skills/*/SKILL.md`
- `skills/skill-doc-budgets.json`
- `docs/runtime-architecture.md`
- `docs/testing.md`
- `docs/featureforge/reference/execution-runtime-module-boundaries.md`
- `tests/public_cli_flow_contracts.rs`
- `tests/runtime_instruction_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/workflow_runtime.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/internal_contracts_execution_runtime_boundaries.rs`
- `tests/codex-runtime/*.test.mjs`

## Preconditions

- Do not use FeatureForge runtime commands while implementing this plan unless explicitly requested by the user.
- Do not use FeatureForge/project skills as implementation shortcuts.
- Keep generated skill docs generated from templates.
- Preserve strict Clippy without suppressions.
- Preserve public command compatibility for existing shipped public commands.
- Preserve event-log append-only authority and existing reducer semantics.

## Known Footguns / Constraints

- Do not replace public-output wording with vague "inspect status" guidance. Each route must name exactly one public next step or say it is diagnostic-only.
- Do not reintroduce low-level helper names such as `record-review-dispatch`, `gate-review`, `gate-finish`, or `rebuild-evidence` in active public guidance.
- Do not make `recommended_command` executable authority again.
- Do not solve input-required templates only in prose. The schema/type surface must make binding deterministic enough for agents.
- Do not let `event_log.rs` depend on router semantics.
- Do not move mandatory law only into companion docs while shrinking skill files.
- Do not hide synthetic test setup boundaries. Public replay claims must distinguish synthetic historical setup from public recovery.

## Requirement Coverage Matrix

| Requirement | Coverage |
| --- | --- |
| REQ-001 Public remediation text never teaches retired rebuild/evidence/record-helper workflows | Task 1 |
| REQ-002 Input-required public command templates can be deterministically bound and executed | Task 2 |
| REQ-003 Failure JSON does not make display strings the practical command authority | Task 3 |
| REQ-004 Text-mode operator/doctor output points to one public next step | Task 4 |
| REQ-005 Generated skill references resolve in installed use and high-use skill budgets are capped | Task 5 |
| REQ-006 Runtime module boundaries prevent event-log/router and read-model/router split decisioning | Task 6 |
| REQ-007 `repair-review-state` consumes shared repair decision objects instead of owning local route law | Task 7 |
| REQ-008 `close-current-task` does not hide resolved worktree-lease cleanup failures | Task 8 |
| REQ-009 Tests no longer execute display strings except explicit legacy-display tests | Task 9 |
| REQ-010 Static and dynamic validation prove public output, prompt, and boundary contracts | Task 10 |

## Tasks

### Task 1: Replace Retired Evidence-Rebuild Public Remediation

**Spec Coverage:** REQ-001

**Goal:** Active public diagnostics must never tell agents to rebuild packets or evidence as a manual recovery path.

**Context:**

- The reaudit found stale remediation text in `src/execution/state/rebuild_evidence.rs:400` and `src/execution/state/rebuild_evidence.rs:433`.
- The better public remediation already exists nearby at `src/execution/state/rebuild_evidence.rs:363`.
- Review-gate validation surfaces these diagnostics through `src/execution/state/review_gate.rs:147`.

**Constraints:**

- Preserve reason codes and failure classes unless a more precise public code is needed.
- Do not add hidden helper names.
- Do not weaken stale evidence detection.

**Done when:**

- No active public diagnostic tells agents to "rebuild evidence", "rebuild its evidence", or "rebuild the packet".
- Stale packet/file proof diagnostics route to public operator JSON and typed public command execution.
- Tests fail if active public text regresses to retired evidence repair wording.

**Files:**

- Modify: `src/execution/state/rebuild_evidence.rs`
- Modify: `tests/runtime_instruction_contracts.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_shell_smoke.rs`

**Implementation Steps:**

1. Replace packet-fingerprint and file-proof remediation text with a shared helper that says to rerun `workflow operator --plan <plan> --json` and follow `recommended_public_command_argv` or bind/execute the typed template.
2. Ensure the text explains that stale proof means public replay/reopen/completion is required, not manual proof file reconstruction.
3. Add a unit or integration assertion that scans active `GateResult` diagnostics for forbidden phrases.
4. Add targeted fixture coverage for packet-fingerprint mismatch and file-proof drift that asserts the new public remediation text.
5. Confirm hidden helper terms remain absent from public text.

**Validation Expectations:**

- `cargo nextest run --test workflow_runtime`
- `cargo nextest run --test workflow_shell_smoke`
- `cargo nextest run --test runtime_instruction_contracts`
- `cargo clippy --all-targets --all-features -- -D warnings`

### Task 2: Make Input-Required Templates Deterministically Executable

**Spec Coverage:** REQ-002

**Goal:** When `recommended_public_command_argv` is absent because inputs are missing, the typed template must give agents a deterministic way to bind those inputs into a public command and execute it.

**Context:**

- `src/execution/public_command_types.rs:8` currently exposes only `command_kind`, `base_argv`, and `required_input_names`.
- `src/execution/command_eligibility.rs:785` creates templates, but required input records do not expose CLI flag names or binding locations.
- Docs/skills currently say to satisfy inputs and rerun operator/status, which can loop.

**Constraints:**

- Do not expose fake executable argv with placeholders.
- Do not parse `recommended_command`.
- Keep schema backward-compatible where possible by adding fields rather than removing existing ones.
- Required inputs must remain typed and machine-readable.

**Done when:**

- `PublicCommandInputRequirement` or `PublicCommandTemplate` includes enough structured binding metadata to build the command after values are supplied.
- All public input-required routes expose consistent flag names or binding instructions.
- Active docs/skills say to bind required inputs into the public command and execute it, then rerun operator/status after the command completes.
- Rerun-the-route-owner wording remains only for diagnostic-only or external wait states where no command can be executed yet.

**Files:**

- Modify: `src/execution/public_command_types.rs`
- Modify: `src/execution/command_eligibility.rs`
- Modify: `src/execution/router.rs`
- Modify: `src/execution/commands/common/operator_outputs.rs`
- Modify: `schemas/plan-execution-status.schema.json`
- Modify: `schemas/workflow-operator.schema.json`
- Modify: `schemas/workflow-handoff.schema.json`
- Modify: `docs/runtime-architecture.md`
- Modify: `skills/*/SKILL.md.tmpl`
- Regenerate: `skills/*/SKILL.md`
- Modify: `tests/runtime_instruction_contracts.rs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`

**Implementation Steps:**

1. Add structured binding metadata. Prefer an explicit field such as `cli_flag` on each `PublicCommandInputRequirement`, plus a template-level statement that bound values append as flag/value pairs to `base_argv`.
2. Map every required input to the actual public CLI flag:
   - `expect_execution_fingerprint` -> `--expect-execution-fingerprint`
   - `source` -> `--source`
   - `claim` -> `--claim`
   - `verify_command` -> `--verify-command`
   - `verify_result` -> `--verify-result`
   - `manual_verify_summary` -> `--manual-verify-summary`
   - `reason` -> `--reason`
   - `owner` -> `--to`
   - `scope` -> `--scope`
   - `task` -> `--task`
   - `review_result` -> `--review-result`
   - `review_summary_file` -> `--review-summary-file`
   - `verification_result` -> `--verification-result`
   - `verification_summary_file` -> `--verification-summary-file`
   - `result` -> `--result`
   - `summary_file` -> `--summary-file`
   - `reviewer_source` -> `--reviewer-source`
   - `reviewer_id` -> `--reviewer-id`
3. Add tests that each input-required route's template can be materialized into valid public argv once representative values are supplied.
4. Update docs and generated templates to say: bind inputs, execute the completed public command, then rerun operator/status to observe the next route.
5. Update schema descriptions to stop saying "satisfy inputs and rerun" as the main action.
6. Run skill doc generation and schema/golden updates through the existing generator path.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo nextest run --test runtime_authority_contracts`
- `cargo nextest run --test workflow_shell_smoke`
- `cargo nextest run --test workflow_runtime`
- `cargo nextest run --test contracts_execution_runtime_boundaries`
- `cargo clippy --all-targets --all-features -- -D warnings`

### Task 3: Remove Display-Command Authority From Failure JSON

**Spec Coverage:** REQ-003

**Goal:** `JsonFailure` messages must not present command-shaped display strings as the next public action.

**Context:**

- `src/execution/command_eligibility.rs:1564` derives display command text.
- `src/execution/command_eligibility.rs:1620` embeds it as `Next public action`.
- Tests currently assert this old pattern.

**Constraints:**

- Preserve fail-closed behavior.
- Preserve useful diagnostics for reason code, phase detail, state kind, repair targets, and blocking records.
- Do not introduce a second ad hoc JSON route schema inside failure messages.

**Done when:**

- Failure messages never include a command-shaped display string after "Next public action".
- Failures direct consumers to structured route JSON fields.
- Tests assert that mutation rejection failures do not expose executable-looking display commands.

**Files:**

- Modify: `src/execution/command_eligibility.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/runtime_authority_contracts.rs`

**Implementation Steps:**

1. Replace `next_public_command` display rendering with a non-executable phrase, for example `structured route available` or `none`.
2. If needed, include `recommended_public_command_kind` and `recommended_public_command_has_argv/template` as structured-ish message facts, but not full shell text.
3. Update tests that currently assert display strings in failure messages.
4. Add scanner coverage that fails if `JsonFailure` construction emits `featureforge plan execution` as a "next action" message segment.

**Validation Expectations:**

- `cargo nextest run --test workflow_runtime`
- `cargo nextest run --test workflow_shell_smoke`
- `cargo nextest run --test runtime_authority_contracts`
- `cargo clippy --all-targets --all-features -- -D warnings`

### Task 4: Tighten Text-Mode Operator And Doctor UX

**Spec Coverage:** REQ-004

**Goal:** Text-mode operator and doctor output must point to one public next step and avoid multi-action record/refresh choreography.

**Context:**

- `src/workflow/operator.rs:1785`, `:1792`, `:1803`, and `:1807` still use "record/refresh" and "then record task closure" wording.
- `src/workflow/doctor_dashboard.rs:231` and `:238` still use "Dispatch or record" and "Record or refresh".
- Historical audit context: `src/workflow/doctor_dashboard.rs:234` previously contained stale plan-fidelity missing-artifact compatibility matching.

**Constraints:**

- Text mode should remain concise.
- JSON remains the command authority.
- Do not remove useful context, but remove ambiguous action chains.

**Done when:**

- Task-boundary text says either to rerun operator with `--json` and follow argv/template or to run `close-current-task` when concrete.
- Final-review text says to follow the public `advance-late-stage`/operator route, not "dispatch or record" generically.
- Stale plan-fidelity missing-artifact matching is removed from active dashboard code or moved to historical-only tests.
- Tests assert text-mode output does not contain ambiguous normal-path phrases.

**Files:**

- Modify: `src/workflow/operator.rs`
- Modify: `src/workflow/doctor_dashboard.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/workflow_entry_shell_smoke.rs`
- Modify: `tests/runtime_instruction_contracts.rs`

**Implementation Steps:**

1. Replace task-boundary reason text with public-route wording:
   - Review wait: "Wait for the review result, then rerun workflow operator with external-review-result-ready and follow argv/template."
   - Verification needed: "Run verification, then bind verification/review inputs into the routed close-current-task command."
   - Closure ready: "Run the routed close-current-task command; do not reopen the step."
2. Replace doctor action text for final review and task closure with public aggregate route language.
3. Remove the stale plan-fidelity missing-artifact dashboard matcher or convert it to the current `missing_plan_fidelity_review_artifact` vocabulary.
4. Add text-output scanner tests for forbidden ambiguous phrases in active public surfaces.

**Validation Expectations:**

- `cargo nextest run --test workflow_shell_smoke`
- `cargo nextest run --test workflow_entry_shell_smoke`
- `cargo nextest run --test workflow_runtime`
- `cargo clippy --all-targets --all-features -- -D warnings`

### Task 5: Fix Prompt Reference Packaging And Per-Skill Budgets

**Spec Coverage:** REQ-005

**Goal:** Generated skills must resolve installed FeatureForge companion references reliably and high-use skills must have per-skill budget caps.

**Context:**

- `skills/using-featureforge/SKILL.md:110` references `references/codex-tools.md`, but the file is skill-local under `skills/using-featureforge/references/`.
- Several skills reference installed FeatureForge docs as repo-relative paths.
- `skills/skill-doc-budgets.json:4` leaves high-use generated skills uncapped.

**Constraints:**

- Repo-local project artifact paths must stay repo-relative.
- Installed package references must use `$_FEATUREFORGE_ROOT/...`.
- Do not move mandatory law solely into companion references.

**Done when:**

- Installed references use `$_FEATUREFORGE_ROOT/...` or an explicitly skill-local path that is valid from the skill file.
- Repo artifact references remain repo-relative only where the user repo owns the artifact.
- `using-featureforge` and other high-use generated skills have per-skill caps.
- Prompt budget and companion-reference tests cover the new rule.

**Files:**

- Modify: `skills/skill-doc-budgets.json`
- Modify: `skills/*/SKILL.md.tmpl`
- Regenerate: `skills/*/SKILL.md`
- Modify: `tests/codex-runtime/skill-doc-budget.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/using_featureforge_skill.rs`

**Implementation Steps:**

1. Audit every generated skill reference to `references/`, `review/`, and `docs/featureforge/reference/`.
2. Convert installed FeatureForge companion references to `$_FEATUREFORGE_ROOT/...`.
3. Keep user-project paths such as `docs/featureforge/plans/...`, `docs/project_notes/...`, and `docs/featureforge/specs/...` repo-relative.
4. Add per-skill caps for at least `using-featureforge`, `brainstorming`, and `verification-before-completion`; set caps from current line counts with modest headroom.
5. Extend Node tests to fail on install-fragile companion references.
6. Regenerate skill docs.

**Validation Expectations:**

- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- `cargo nextest run --test using_featureforge_skill`
- `cargo clippy --all-targets --all-features -- -D warnings`

### Task 6: Restore Runtime Import And Reentry Decision Boundaries

**Spec Coverage:** REQ-006

**Goal:** Event-log storage must not call router, and read-model preclassification must not decide reentry with weaker authority than router.

**Context:**

- `src/execution/event_log.rs:18` imports `route_runtime_state`.
- `src/execution/event_log.rs:3570` and `:3594` call router for migration parity projection.
- `src/execution/read_model.rs:1412` calls `execution_reentry_target` with default authority inputs.
- Router uses richer runtime-state authority inputs in `src/execution/router.rs:429` and `:464`.

**Constraints:**

- Preserve event-log migration parity behavior.
- Preserve historical migration tests.
- Do not make event-log migration depend on display strings.
- Do not duplicate route tables in a new helper.

**Done when:**

- `event_log.rs` no longer imports `router`.
- Migration parity route projection is owned by a higher-level query/migration module or a narrow adapter above event-log storage.
- Read model obtains repair/reentry classification from the same reduced runtime-state route decision or shared authority input object as router.
- Boundary tests forbid `event_log -> router`.

**Files:**

- Modify: `src/execution/event_log.rs`
- Modify: `src/execution/query.rs` or add a focused migration parity module
- Modify: `src/execution/read_model.rs`
- Modify: `src/execution/router.rs`
- Modify: `tests/runtime_module_boundaries.rs`
- Modify: `tests/contracts_execution_runtime_boundaries.rs`
- Modify: `docs/runtime-architecture.md`
- Modify: `docs/featureforge/reference/execution-runtime-module-boundaries.md`

**Implementation Steps:**

1. Identify the minimal data event-log migration needs to preserve parity.
2. Move router-dependent migration route projection out of `event_log.rs`.
3. Keep event-log responsible for storage/replay/migration payload management only.
4. Introduce or reuse a shared `RouteAuthorityInputs` object so read model and router decide execution reentry from the same inputs.
5. Replace read-model default-authority `execution_reentry_target` calls with the shared route decision projection.
6. Add import-boundary tests:
   - `event_log.rs` must not import `router`, `next_action`, or workflow modules.
   - read-model helpers must not call stale-target selection with default authority for production route classification.
7. Update architecture docs.

**Validation Expectations:**

- `cargo nextest run --test runtime_module_boundaries`
- `cargo nextest run --test contracts_execution_runtime_boundaries`
- `cargo nextest run --test workflow_runtime`
- `cargo nextest run --test execution_query`
- `cargo clippy --all-targets --all-features -- -D warnings`

### Task 7: Extract Shared Repair-Review Decisioning

**Spec Coverage:** REQ-007

**Goal:** `repair-review-state` should execute a shared repair decision, not reconstruct route/follow-up law locally.

**Context:**

- `src/execution/review_state.rs:2696` through `:3018` builds repair plans and follow-ups locally.
- `src/execution/review_state.rs:3020` rewrites bridge behavior into `task_closure_recording_ready`.
- `docs/featureforge/reference/execution-runtime-module-boundaries.md:76` already marks this as scheduled follow-up.

**Constraints:**

- Do not change public behavior while extracting.
- Preserve all stale boundary ordering and bridge behavior.
- Keep mutation actions separated from read-only route decisions.

**Done when:**

- Repair-plan construction lives in a focused shared module.
- Router/read-model/repair mutation consume the same repair decision object.
- `review_state.rs` performs mutation actions and output assembly, not target/follow-up law.
- Existing repair/reentry/liveness tests pass without broad fixture rewrites.

**Files:**

- Modify: `src/execution/review_state.rs`
- Add or modify: `src/execution/repair_decision.rs`
- Modify: `src/execution/router.rs`
- Modify: `src/execution/read_model.rs`
- Modify: `src/execution/repair_target_selection.rs`
- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/public_replay_churn.rs`
- Modify: `tests/liveness_model_checker.rs`
- Modify: `tests/runtime_module_boundaries.rs`

**Implementation Steps:**

1. Extract pure data inputs currently passed to `analyze_repair_plan`.
2. Define a shared `RepairDecision` with:
   - blocker kind
   - target task/step
   - required follow-up
   - public route action
   - bridge classification
   - mutation actions to perform
3. Move pure decision logic into the shared module.
4. Keep actual state mutation in `repair_review_state`.
5. Route router/read-model through the same decision object where they need repair/follow-up classification.
6. Add parity tests that compare operator/status route decisions before and after `repair-review-state` using the same decision object.
7. Remove or shrink the scheduled-follow-up note once the extraction is complete.

**Validation Expectations:**

- `cargo nextest run --test workflow_runtime`
- `cargo nextest run --test workflow_shell_smoke`
- `cargo nextest run --test public_replay_churn`
- `cargo test --test liveness_model_checker`
- `cargo nextest run --test runtime_module_boundaries`
- `cargo clippy --all-targets --all-features -- -D warnings`

### Task 8: Stop Hiding Worktree-Lease Cleanup Failures

**Spec Coverage:** REQ-008

**Goal:** A successful `close-current-task` must not silently leave a failed resolved lease cleanup behind.

**Context:**

- `src/execution/commands/close_current_task.rs:787` ignores cleanup errors.
- `src/execution/authority.rs:1262` can fail cleanup on malformed authority.
- `src/execution/review_state.rs:1249` propagates equivalent cleanup errors.

**Constraints:**

- Do not roll back authoritative task closure after it has been recorded unless the existing recording transaction can include cleanup safely.
- Preserve append-only event authority.
- Avoid making a successful closure look failed without clear status.

**Done when:**

- Cleanup errors are not silently ignored.
- If cleanup can be validated before closure recording, invalid cleanup state fails closed before mutation.
- If cleanup fails after closure recording, the command returns structured diagnostics that tell the agent the closure was recorded but lease cleanup requires deterministic public repair/reconcile.
- Tests cover cleanup failure and no post-close loop.

**Files:**

- Modify: `src/execution/commands/close_current_task.rs`
- Modify: `src/execution/authority.rs`
- Modify: `src/execution/commands/common/outputs.rs`
- Modify: `tests/workflow_shell_smoke.rs`
- Modify: `tests/plan_execution.rs`
- Modify: `tests/liveness_model_checker.rs`

**Implementation Steps:**

1. Inspect when `release_resolved_worktree_leases_after_current_task_closure` is called relative to closure recording and output assembly.
2. Prefer prevalidating lease index readability and required identity before closure recording when active resolved leases exist.
3. Change cleanup helper to return `Result<Vec<ReleasedLease>, JsonFailure>` or structured warning data.
4. Decide the correct command output on post-commit cleanup failure:
   - If the closure is already committed, avoid claiming nothing happened.
   - Surface a deterministic diagnostic and next public route.
5. Mirror `repair-review-state` error propagation behavior where safe.
6. Add a malformed authority fixture where cleanup fails and assert no silent success.
7. Add a convergence test where the next route does not reopen the just-closed task unless there is real stale/negative state.

**Validation Expectations:**

- `cargo nextest run --test workflow_shell_smoke`
- `cargo nextest run --test plan_execution`
- `cargo test --test liveness_model_checker`
- `cargo clippy --all-targets --all-features -- -D warnings`

### Task 9: Remove Display-String Execution From Tests

**Spec Coverage:** REQ-009

**Goal:** Tests should not preserve the old assumption that `recommended_command` is executable, except in explicit legacy-display boundary tests.

**Context:**

- `tests/workflow_runtime.rs:506` splits `recommended_command`.
- `tests/internal_contracts_execution_runtime_boundaries.rs:468` splits `recommended_command`.
- Public replay tests already use argv correctly.

**Constraints:**

- Preserve CLI-boundary tests where the shell boundary is under test.
- If a legacy display compatibility test remains, its name and comments must say it is legacy display compatibility, not public command authority.

**Done when:**

- Normal tests execute `recommended_public_command_argv` or materialized typed templates.
- No public or semantic test helper splits `recommended_command`.
- Static tests catch new display-string execution helpers.

**Files:**

- Modify: `tests/workflow_runtime.rs`
- Modify: `tests/internal_contracts_execution_runtime_boundaries.rs`
- Modify: `tests/public_cli_flow_contracts.rs`
- Modify: `tests/runtime_authority_contracts.rs`

**Implementation Steps:**

1. Replace helper inputs from `recommended_command: &str` to route JSON containing `recommended_public_command_argv` or typed template.
2. For input-required routes, use the materialization helper from Task 2.
3. Rename any remaining display-command tests to `legacy_display_compatibility_*`.
4. Add static scan for `.split_whitespace()` on `recommended_command` outside explicit legacy test files/functions.
5. Update assertions that compare `authoritative_next_action` to display commands to compare typed route kind/argv/template instead.

**Validation Expectations:**

- `cargo nextest run --test workflow_runtime`
- `cargo nextest run --test contracts_execution_runtime_boundaries`
- `cargo nextest run --test public_cli_flow_contracts`
- `cargo nextest run --test runtime_authority_contracts`
- `cargo clippy --all-targets --all-features -- -D warnings`

### Task 10: Full Validation And Release-Claim Guardrails

**Spec Coverage:** REQ-010

**Goal:** Prove the remediation across Node, Rust, public replay, liveness, and prompt/boundary contracts.

**Context:**

- The reaudit validation passed before these follow-up fixes.
- New fixes touch public UX, schemas, generated skills, and runtime boundaries.

**Constraints:**

- Do not claim public setup coverage for synthetic historical fixtures.
- Do not merge if strict Clippy or any nextest target fails.
- Keep validation evidence precise.

**Done when:**

- All updated tests pass.
- Full Rust verification passes with no fail-fast.
- Release-facing docs distinguish synthetic historical setup from public recovery.
- Audit finding inventory is updated or a new remediation completion note is added.

**Files:**

- Modify: `docs/testing.md`
- Modify: `README.md` if public runtime guidance changes
- Modify: `docs/featureforge/reference/2026-05-07-deep-runtime-safety-reaudit.md` only if the implementation intentionally supersedes a finding with completion notes

**Implementation Steps:**

1. Run generated doc checks:
   - `node scripts/gen-skill-docs.mjs --check`
   - `node scripts/gen-agent-docs.mjs --check`
   - `node --test tests/codex-runtime/*.test.mjs`
2. Run strict Clippy:
   - `cargo clippy --all-targets --all-features -- -D warnings`
3. Run full Rust tests without fail fast:
   - `cargo nextest run --all-targets --all-features --no-fail-fast`
4. Run liveness explicitly if not included in the nextest invocation:
   - `cargo test --test liveness_model_checker`
5. Inspect any test names/docs that mention public replay and ensure synthetic setup caveats are explicit.
6. Record validation results in the implementation handoff.

**Validation Expectations:**

- Node generated-doc checks pass.
- Node codex-runtime tests pass.
- Strict Clippy passes.
- Full nextest with no fail-fast passes.
- Liveness model checker passes.
