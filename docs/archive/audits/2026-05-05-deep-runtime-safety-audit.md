# FeatureForge Deep Runtime Safety Audit

Date: 2026-05-05

Scope: runtime and CLI, tests, generated docs, skills, reviewer prompts, schemas, and public output surfaces in this checkout.

Method: static code inspection, clean-context parallel subagent audits, and validation commands. No FeatureForge runtime commands or FeatureForge skills were used for the audit work.

## Executive Verdict

Recommendation: do not ship as-is. Ship only after targeted fixes.

The branch is materially improved in the areas that previously caused task-closure churn: current task closure is treated as authoritative, receipt/projection drift is mostly diagnostic-only, `begin` owns preflight setup, `close-current-task` refreshes current task dispatch internally, and public replay/liveness coverage is strong for stale closure loops.

It is not yet safe enough to call a ship candidate because normal public reachability still has holes and required validation fails:

- Blocker: direct `plan execution begin` appears able to bypass the workflow-level five-surface plan-fidelity handoff because it authorizes from execution context/status rather than the resolved workflow route.
- Blocker: late-stage finish review/finish completion routes expose `finish branch` with no public command, while the only checkpoint writer is still tied to the hidden/internal gate-review path.
- Blocker: final-review dispatch lineage can still block public final-review recording with `required_follow_up=request_external_review` and no executable public command/argv.
- High: required Node contract validation fails because generated top-level skill docs exceed enforced prompt budgets.
- High: public-flow tests still contain direct in-process workflow helper calls, and one direct-vs-real parity helper compares the compiled CLI to itself.

## Subagent Coverage

- Subagent A, public CLI and reachable runtime: found unreachable finish progression and final-review dispatch lineage.
- Subagent B, tests versus shipped-runtime realism: found public-flow suites still using direct workflow helpers and a broken direct-vs-real parity helper.
- Subagent C, receipts/provenance/evidence control plane: found no actionable control-plane defects.
- Subagent D, plan-review workflow: found the direct `begin` plan-fidelity bypass candidate; plan-fidelity artifact design otherwise looks corrected.
- Subagent E, stale closure/cycle/reentry loops: found no actionable convergence defects; marked `resume_task`/`resume_step` as a P3 watch item.
- Subagent F, prompt surface and packaging: found failing prompt budget enforcement and install-doc drift.
- Subagent G, modularization and split decisioning: found residual duplicate blocking-scope and phase-mapping logic.
- Subagent H, public output and agent UX: found display-command text and token-only follow-up traps.

## What Is Genuinely Fixed

- Public `begin` seeds allowed preflight internally through `public_intent_preflight_persistence_required` and `persist_allowed_public_begin_preflight` in `src/execution/commands/begin.rs:38-43` and `src/execution/commands/begin.rs:162`.
- Public `close-current-task` can refresh current task dispatch lineage internally before recording a current task closure. The path calls `ensure_current_review_dispatch_id` from `src/execution/commands/close_current_task.rs:190-199`.
- Typed public command authority exists. `PublicCommand`, `PublicCommandKind`, and `PublicCommandInvocation` live in `src/execution/command_eligibility.rs:13-70`; `PublicRouteDecision` stores typed command data in `src/execution/router.rs`, and status/operator project argv from that route decision.
- Hidden command tokens are centralized and scanned. `record-review-dispatch`, `gate-review`, `gate-finish`, `plan execution preflight`, and similar compatibility lanes are classified in `src/execution/command_eligibility.rs:1240-1256`.
- Receipt/projection artifacts are mostly read-model or diagnostic data, not task-boundary control-plane truth. Current task closure drives boundary progression; projection loss after authoritative closure is covered by `tests/public_replay_churn.rs`.
- `summary_hash_drift_ignored` behavior is intentional for current pass/pass task closures and does not force execution reentry.
- Engineering-review edit flow no longer bounces immediately back to plan fidelity. Draft plans with `Last Reviewed By: plan-eng-review` route to final fidelity refresh only after engineering review handoff.
- Reviewer recursion prevention is prompt text only and scoped to reviewer prompts. It is not implemented as a runtime/env guard.
- Generated docs are fresh according to `node scripts/gen-skill-docs.mjs --check` and `node scripts/gen-agent-docs.mjs --check`.
- Runtime module boundary tests are substantial and passed in the extra validation run.

## What Remains Risky

- Direct execution commands do not always appear to honor the same workflow-level route gates that `workflow status` and `workflow operator` compute.
- Late-stage public routes can still report a normal next action with no public executable command.
- Some public JSON failure shapes return token-only `required_follow_up` values without `recommended_command`, `recommended_public_command_argv`, `required_inputs`, or an explicit requery contract.
- Text output still prints display strings such as `Recommended command: ...` without making clear that JSON argv is the executable authority.
- Tests are improved but still mixed: many public replay tests use the compiled CLI, while protected public-flow suites still call in-process workflow helpers.
- Modularization reduced monolith size but left duplicated semantic decisions in projection/presentation layers.
- Prompt budget enforcement is real, but it is currently failing.

## Prioritized Findings

### Blocker 1: Direct `begin` Can Bypass Workflow Plan-Fidelity Handoff

Category: user-facing control-plane bypass, plan-review workflow issue, test realism issue.

The workflow query path detects an `Engineering Approved` plan whose current five-surface plan-fidelity artifact is missing/stale and routes back to plan review. The direct execution mutation path does not appear to use that workflow route when authorizing `begin`.

Evidence:

- `begin` computes `begin_status` through `public_status_from_supplied_context_with_shared_routing(&context, false)` in `src/execution/commands/begin.rs:38`, then calls `require_public_mutation` with that execution status in `src/execution/commands/begin.rs:110-118`.
- The mutation context loader only checks `Workflow State: Engineering Approved`, accepted execution modes, and `Last Reviewed By: plan-eng-review`; it does not call `evaluate_plan_fidelity_review` or equivalent handoff gating. See `src/execution/context.rs:285-305`.
- Workflow routing does perform the fidelity block through `route_is_engineering_approval_fidelity_blocked` in `src/execution/query.rs:627-678`.
- Existing replay coverage named `public_replay_engineering_approved_plan_without_fidelity_cannot_bypass_to_implementation` only checks `workflow status`; it does not attempt `plan execution status` plus direct `plan execution begin`. See `tests/public_replay_churn.rs:1822-1846`.

Impact:

A user or agent can follow the direct execution surface instead of `workflow operator` and potentially start implementation on an Engineering Approved plan that workflow routing would still send back to plan-fidelity review. That violates the target end state: final implementation handoff requires current five-surface fidelity.

Required fix:

Direct `plan execution status` and `plan execution begin` must share the same pre-execution implementation-handoff gate as workflow routing, or `begin` must fail closed before mutation when the workflow route is fidelity-blocked.

### Blocker 2: Finish Review and Finish Completion Are Not Publicly Reachable

Category: user-facing dead end, public/private command mismatch.

The public CLI exposes `status`, `repair-review-state`, `close-current-task`, `advance-late-stage`, `begin`, `complete`, `reopen`, `transfer`, and `materialize-projections` in `src/cli/plan_execution.rs:13-45`. It does not expose `gate-review` or `gate-finish`.

Normal late-stage routing still reaches `ready_for_branch_completion` with `finish_review_gate_ready` or `finish_completion_gate_ready`, but `late_stage_decision` intentionally returns no public command for both phase details in `src/execution/late_stage_route_selection.rs:179-183`.

The checkpoint mutation for finish-review gate pass remains in `ExecutionRuntime::review_gate`, which calls `persist_finish_review_gate_pass_checkpoint` in `src/execution/state/runtime_methods.rs:246-250`; the persisted command string is still `"gate_review"` in `src/execution/state/review_gate.rs:38`.

Tests currently codify the null command:

- `workflow_operator_requires_persisted_gate_review_checkpoint_before_gate_finish` asserts `phase_detail == "finish_review_gate_ready"`, `next_action == "finish branch"`, and `recommended_command == null` in `tests/workflow_shell_smoke.rs:4840-4875`.
- `workflow_operator_routes_ready_branch_completion_to_gate_finish_after_review_gate_passes` asserts `finish_completion_gate_ready` with `recommended_command == null` in `tests/workflow_shell_smoke.rs:4802-4839`.

Impact:

An agent can reach a normal finish route that says `finish branch` but provides no public argv. The historical hidden `gate-review`/`gate-finish` mechanics are not public, so the route is not executable through shipped normal commands.

Required fix:

`advance-late-stage` should own finish-review checkpointing and finish completion as intent-level progression, and the corresponding route details must expose typed public argv or a diagnostic-only blocked state.

### Blocker 3: Final-Review Dispatch Lineage Can Still Require Hidden/Internal Mutation

Category: user-facing dead end, public/private command mismatch.

After release readiness is current, the read model can derive `final_review_dispatch_required`. That phase detail is omitted from public command recommendation in `src/execution/late_stage_route_selection.rs:179-183` and `src/execution/phase.rs:85-95`.

`advance-late-stage` can record final-review evidence only after the operator is already `final_review_recording_ready`. It checks readiness at `src/execution/commands/advance_late_stage.rs:902-918`; only after that does it call `ensure_current_review_dispatch_id(... FinalReview ...)` at `src/execution/commands/advance_late_stage.rs:923-931`.

The public shell-smoke test confirms the trap: missing dispatch lineage makes public final-review recording block with `required_follow_up=request_external_review` and `recommended_command == null` in `tests/workflow_shell_smoke.rs:5314-5369`.

Impact:

The normal final-review path can require a current dispatch lineage that no public command creates before the recording check. This is the same failure shape as the historical `record-review-dispatch` dead end, only under newer names.

Required fix:

Public late-stage progression must bootstrap or refresh current final-review dispatch lineage before final-review recording readiness is evaluated, or expose a public request/dispatch command with typed argv and required inputs.

### High 1: Prompt Budget Enforcement Fails

Category: validation failure, prompt-surface issue.

Required validation `node --test tests/codex-runtime/*.test.mjs` failed in `tests/codex-runtime/skill-doc-budget.test.mjs`.

Observed budget report:

- `systematic-debugging`: 315 lines, max 300.
- `writing-plans`: 329 lines, max 320.
- `document-release`: 296 lines, max 275.
- `requesting-code-review`: 282 lines, max 240.
- `executing-plans`: 272 lines, max 240.
- Total generated skill lines: 5361, max 5600.

The manifest is enforced in `skills/skill-doc-budgets.json:1-18`. The failing assertion is in `tests/codex-runtime/skill-doc-budget.test.mjs:141-167`.

Impact:

The prompt compaction work is not complete. A required doc/runtime contract test fails, and several high-use skills exceed per-skill budgets.

Required fix:

Trim the corresponding templates, keep mandatory law top-level where required, move non-mandatory details to packaged companion references, regenerate docs, and rerun the Node contract suite.

### High 2: Public-Flow Tests Still Prove Internal Helper Behavior

Category: test realism issue.

The strongest public helper, `tests/support/public_featureforge_cli.rs`, uses `env!("CARGO_BIN_EXE_featureforge")` and does exercise the compiled binary. Many replay and shell tests use it.

However, protected public-flow suites still call direct in-process workflow surfaces:

- `tests/workflow_runtime.rs:184` calls `operator::doctor_for_runtime`.
- `tests/workflow_runtime.rs:736` calls `WorkflowRuntime::discover_for_state_dir(...).status_refresh()`.
- `tests/workflow_runtime.rs:5430` and `tests/workflow_runtime.rs:6352` call direct operator helpers.
- `tests/support/runtime_phase_handoff.rs:6-13` wraps direct `phase_for_runtime` / `handoff_for_runtime`; `tests/workflow_runtime_final_review.rs:796` uses that wrapper.
- `tests/workflow_shell_smoke.rs:4559` calls `operator::doctor_phase_and_next_for_runtime_with_args` directly.

The static guard marks these suites as protected in `tests/public_cli_flow_contracts.rs:1680-1703`, but its forbidden imports list only covers selected helper modules in `tests/public_cli_flow_contracts.rs:1765-1774`, and the direct runtime surface detector only looks for `operator_for_runtime` plus `ExecutionRuntime::{status,review_gate,finish_gate}` in `tests/public_cli_flow_contracts.rs:1941-1950`.

Impact:

Tests can pass by proving the library path rather than the shipped parse/env/stdout/stderr boundary. That is a recurrence of the historical failure class where internal helpers worked but public CLI flow did not.

Required fix:

Convert protected public-flow assertions to compiled CLI calls or move direct helper coverage into clearly quarantined internal suites. Expand static guards to catch `doctor_for_runtime`, `status_refresh`, `phase_for_runtime`, `handoff_for_runtime`, and direct wrappers.

### Medium 1: Direct-Versus-Real Parity Helper Compares the CLI to Itself

Category: test realism issue.

`tests/contracts_execution_runtime_boundaries.rs:189-203` accepts `_real_cli: bool` but ignores it and always calls `public_featureforge_cli::run_featureforge_real_cli`. Tests that appear to compare direct helper and real CLI paths are therefore only public-runtime tests.

Impact:

The tests are useful, but their names overstate coverage. They do not catch direct-helper/real-CLI divergence.

Required fix:

Either restore a real direct-helper branch under an internal-only/quarantined suite or rename and rescope the tests to make clear they are compiled-CLI only.

### Medium 2: Text Output Still Presents Display Strings as Commands

Category: public-output / agent-UX issue.

JSON output exposes `recommended_public_command_argv` and `required_inputs`, which is the correct machine authority. Text output still includes command-shaped display strings:

- `src/workflow/operator.rs:456-468` prints `Recommended command:`.
- `src/workflow/operator.rs:696-705` prints doctor recommended command text.
- `src/workflow/operator.rs:1059-1061` appends `Recommended command: ...`.
- `src/workflow/operator.rs:1781-1785` says `Follow the routed command: ...`.

Active docs warn agents not to parse display strings, but public text output remains command-shaped.

Impact:

An agent reading text mode can still execute display text rather than typed argv. This is not as severe as JSON route mismatch, but it is an agent-UX trap.

Required fix:

Text output should either show a compact JSON-argv hint or explicitly label display strings as non-authoritative summaries and point to JSON `recommended_public_command_argv`.

### Medium 3: Token-Only Follow-Ups Lack Executable Recovery Contracts

Category: public-output / agent-UX issue.

Several blocked mutator outputs return `required_follow_up` tokens without `recommended_command`, `recommended_public_command_argv`, `required_inputs`, or an explicit requery contract:

- `advance_late_stage_follow_up_or_requery_output` in `src/execution/commands/common/operator_outputs.rs:370-419`.
- Branch closure and final-review blocked paths in `src/execution/commands/advance_late_stage.rs`.
- `MissingReviewedStateBinding` and stale reviewed-state paths in `src/execution/commands/close_current_task.rs:41-70`.
- Blocked repair output in `src/execution/review_state.rs:1773-1785`.

Impact:

Tokens like `request_external_review`, `execution_reentry`, or `repair_review_state` are semantic labels. Without a public command/argv or explicit "requery workflow/operator" instruction, agents may translate them into hidden helpers or manual artifact repair.

Required fix:

Every blocked public output should have one of these shapes: executable public argv, parseable required inputs for the current public command, explicit requery via `workflow operator --json`, or a diagnostic-only blocked state with no follow-up token.

### Medium 4: Blocking Scope/Task Is Decided in Multiple Presentation Layers

Category: architecture / split-decisioning issue.

Router/query derives blocking scope and task from route/runtime state; read-model projection then mutates branch/reentry status; workflow operator repeats override logic.

Evidence:

- Router/query: `src/execution/router.rs:1762`, `src/execution/query.rs:872`.
- Read-model projection overrides: `src/execution/read_model/public_route_projection.rs:90` and `src/execution/read_model/public_route_projection.rs:188`.
- Workflow operator overrides: `src/workflow/operator.rs:1301` and `src/workflow/operator.rs:1321`.

Impact:

This is the clearest remaining split-decisioning risk. The duplicated logic has not produced an observed bug in this audit, but it can diverge under future route changes.

Required fix:

Centralize blocking scope/task derivation into a route decision object or shared projection helper consumed by status/operator/presentation.

### Medium 5: Install Docs Still Teach Removed Auto Update-Check Behavior

Category: documentation / packaging issue.

Generated preamble tests explicitly forbid auto-running update checks in every generated preamble:

- `tests/codex-runtime/skill-doc-contracts.test.mjs:635`.
- `tests/codex-runtime/gen-skill-docs.unit.test.mjs:97-99`.

Install docs still say generated skill preambles automatically run the packaged install binary for `update-check`:

- `.codex/INSTALL.md:124`.
- `.copilot/INSTALL.md:109`.

Impact:

This is not a runtime dead end, but it is active install guidance drift.

Required fix:

Update install docs to match the generated preamble contract and tests.

### Low 1: Late-Stage Phase Canonicalization Is Duplicated

Category: architecture cleanup.

The shared helper lives in `src/execution/query.rs:810`, but `src/execution/late_stage_route_selection.rs:199` repeats phase-detail to phase mapping.

Impact:

Low current risk, but it is exactly the kind of duplicated semantic mapping that has caused route/status/operator drift.

Required fix:

Delegate late-stage phase derivation to the shared helper.

### Low 2: `state.rs` Is Line-Thin but Semantically Broad

Category: architecture cleanup.

`src/execution/state.rs` is under line budget and `src/execution/mutate.rs` is thin, but `state.rs` still re-exports context, read-model internals, status DTOs, preflight, rebuild, runtime methods, and command request helpers from one compatibility hub.

Impact:

Boundary tests mitigate misuse, but the facade remains broad enough to hide ownership drift.

Required fix:

Continue shrinking re-exports as call sites migrate to focused modules.

### Low 3: `resume_task` / `resume_step` Remain a Watch Item

Category: reentry-loop watch item.

`resume_task` / `resume_step` can authorize `begin` before the generic exact-route check, but only when status is `execution_in_progress`, execution has started, no active task/step exists, the task/step match exactly, and the fingerprint matches. Current read-model preemption clears stale resume fields first, and replay/liveness tests cover targetless stale with resume.

Impact:

No actionable defect found, but this should remain covered because it is a sensitive bypass lane by design.

## Assessment by Required Area

### Public CLI and Reachability

Partially fixed.

The public CLI inventory is clean and does not expose the old low-level commands. Public `begin` owns preflight setup. Public `close-current-task` owns current task dispatch refresh and closure recording. Typed public argv is the executable route authority for commands that have a public command.

Not fixed: final-review dispatch and branch finish progression can still land on normal states with no public argv. Direct `begin` appears to bypass workflow plan-fidelity gating.

### Tests Versus Shipped Runtime Realism

Partially fixed.

The compiled CLI helper is strong, and replay tests cover many historical stuck paths through public commands. Static guards catch many hidden command and helper leaks.

Not fixed: protected public-flow suites still call in-process workflow helpers; direct-vs-real parity tests compare the CLI to itself; replay coverage for Engineering Approved without fidelity checks workflow status but not direct `plan execution begin`.

### Receipt, Provenance, Evidence, and Projection Control Plane

Fixed based on static audit and tests reviewed.

Current task closure is the task-boundary authority. Missing/stale projection exports and receipt/provenance diagnostics are separated from route blockers after authoritative closure. Runtime-owned projection paths are filtered out of semantic drift. Projection materialization is explicit.

No actionable control-plane findings were identified in this audit.

### Plan Review and Engineering Review Workflow

Partially fixed.

Plan-fidelity now uses parseable review artifacts with plan/spec fingerprints and required surface coverage. Active docs do not teach receipt recording. Engineering-review edits can remain in engineering review until final fidelity refresh.

Not fixed: direct `plan execution begin` does not appear to enforce the same final five-surface fidelity handoff that workflow routing enforces.

### Stale Closure, Cycle-Break, and Reentry Loops

Fixed with one watch item.

No P0-P2 loop/convergence defects were found. `runtime_reconcile_required` and `blocked_runtime_bug` fail closed. Current task closure is not projected as stale. Liveness tests passed, including repeated route signature coverage.

Watch: keep `resume_task` / `resume_step` exact-begin-only safeguards covered.

### Evidence and Projection

Fixed.

Normal public commands do not need tracked approved plan/evidence markdown writes for progress. Supersession is append-only in authoritative histories. Evidence/projection surfaces behave as audit/read-model outputs rather than control-plane truth in the checked paths.

### Prompt Surface and Packaging

Partially fixed.

Generated docs are fresh. Reviewer recursion prevention is prompt-only and reviewer-prompt scoped. Companion references are discoverable and packaged. Mandatory law remains top-level in the reviewed high-use skills.

Not fixed: per-skill line budgets fail, and install docs teach stale auto update-check behavior.

### Modularization and Split Decisioning

Partially fixed.

`mutate.rs` is thin, `state.rs` is line-thin, phase-detail strings are mostly centralized, public command typing is real, and boundary tests passed.

Not fixed: blocking scope/task is still derived or overridden in multiple projection/presentation layers. Late-stage phase canonicalization is duplicated. `state.rs` remains a broad compatibility facade.

### Reviewer Recursion

Fixed.

Reviewer prompts prohibit launching additional subagents and invoking other FeatureForge skills/workflows. This is prompt text only and scoped to reviewer prompts. No runtime/env recursion enforcement was found.

## Concrete Dead Ends Still Possible

- A user reaches `ready_for_branch_completion` with `finish_review_gate_ready`; operator says `finish branch`, but no public command/argv exists. Hidden `gate-review` would be needed to record the checkpoint.
- A user reaches `ready_for_branch_completion` with `finish_completion_gate_ready`; operator still says `finish branch` with no public command/argv.
- A user records release readiness, then tries public final-review recording without current final-review dispatch lineage; `advance-late-stage` returns blocked with `required_follow_up=request_external_review` and no command/argv.
- A user or agent uses direct `plan execution status` and `plan execution begin` on an Engineering Approved plan with missing plan-fidelity artifact; workflow status would block, but direct execution path appears not to consult that workflow gate.

## Concrete Churn Sources Still Possible

- Token-only follow-ups can make agents invent a recovery command or retry the same mutation after manual interpretation.
- Text output display strings can make agents execute `recommended_command` text instead of JSON argv.
- Split blocking-scope derivation can drift between router, status projection, and operator presentation.
- Broad `state.rs` exports make it easier for new code to bypass focused module ownership.
- Prompt budget pressure can lead future edits to move mandatory law into companion docs unless the remediation keeps the top-level-law review criteria explicit.

## Validation Results

Required validation:

- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/*.test.mjs`: failed. One failing test: `tests/codex-runtime/skill-doc-budget.test.mjs`, `systematic-debugging has 315 lines, exceeding budget 300`. The budget report also listed `writing-plans`, `document-release`, `requesting-code-review`, and `executing-plans` over per-skill budget.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo nextest run --test runtime_authority_contracts`: passed, 5 tests.
- `cargo nextest run --test workflow_runtime`: passed, 95 tests.
- `cargo nextest run --test workflow_shell_smoke`: passed, 97 tests.
- `cargo nextest run --test workflow_entry_shell_smoke`: passed, 10 tests.
- `cargo nextest run --test plan_execution`: passed, 44 tests.
- `cargo nextest run --test plan_execution_final_review`: passed, 29 tests.
- `cargo nextest run --test workflow_runtime_final_review`: passed, 2 tests.
- `cargo nextest run --test contracts_execution_runtime_boundaries`: passed, 29 tests.
- `cargo nextest run --test execution_query`: passed, 11 tests.
- `cargo test --test liveness_model_checker`: passed, 28 tests in 118.63s.

Additional validation run:

- `cargo nextest run --test public_cli_flow_contracts --test runtime_module_boundaries --test public_replay_churn`: passed, 91 tests.

Toolchain:

- `cargo 1.94.0`.
- `cargo nextest 0.9.132`.
- `rustc 1.94.0`.

## Checklist

### Public CLI / Reachability

- Public `begin` can seed preflight: fixed.
- No normal flow needs `plan execution preflight`: fixed.
- No normal flow needs `record-review-dispatch`: still broken for final-review dispatch lineage.
- No normal flow needs `gate-review`: still broken for finish-review checkpointing.
- No normal flow needs `gate-finish`: still broken for finish completion.
- No normal flow needs `rebuild-evidence`: fixed based on audited paths.
- No normal flow needs low-level late-stage recorders: partially fixed; main late-stage recorders are public, final-review dispatch and finish are not.
- Operator never recommends hidden/debug commands: fixed for hidden commands, but still emits normal states with null public command.
- Status never exposes hidden/debug commands as next actions: fixed for hidden commands.
- Public recommended argv is executable by shipped CLI: fixed when argv exists; partially fixed overall because normal routes can omit argv.

### Plan Review

- Plan-fidelity no longer uses hidden runtime receipt recording: fixed.
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: fixed.
- Engineering-review edits do not bounce back to fidelity early: fixed.
- Final engineering-approved handoff requires current five-surface fidelity: partially fixed; workflow route enforces it, direct `begin` appears to bypass.
- Active docs do not teach plan-fidelity receipt recording: fixed.
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed based on active surface scan.

### Execution Runtime

- Current task closure is begin-time authority: fixed.
- Current closure cannot appear in stale closures: fixed.
- Close-current-task can refresh current dispatch internally: fixed.
- Stale dispatch does not block public close: fixed for task close.
- Receipt/projection diagnostics do not trigger reentry: fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not treated as authoritative unless exact command is begin for same task/step: fixed with watch item.
- Repair-review-state cannot loop on same route: fixed based on liveness/replay coverage.
- Runtime reconcile handles targetless stale states: fixed.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: fixed.
- Supersession is append-only and does not rewrite proof: fixed.
- Evidence is audit/projection, not control plane: fixed.

### Tests

- Public-flow tests do not call internal helpers: partially fixed; compiled CLI coverage is strong, protected suites still call direct workflow helpers.
- Internal helpers are quarantined in internal-unit-only tests: partially fixed.
- Static tests catch hidden helper use in public-flow tests: partially fixed; guard misses several direct workflow helper names.
- Replay tests cover historical dead ends: partially fixed; strong for churn/reentry, missing direct-begin and late-stage finish/final-dispatch dead ends.
- Liveness model catches repeated route signatures: fixed.
- Node/doc contracts pass: still broken.
- Prompt budget test passes: still broken.

### Prompt Surface

- Skill docs are within budget: still broken.
- Mandatory law remains top-level: fixed based on reviewed skills.
- Companion references exist and are packaged: fixed.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: partially fixed; `mutate.rs` is thin, `state.rs` remains broad.
- New modules have cohesive responsibilities: partially fixed.
- No new catch-all module replaces the old monoliths: partially fixed.
- Phase/reason strings are centralized: partially fixed; phase constants are centralized, but late-stage phase mapping is duplicated.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: partially fixed; blocking scope/task is still overridden in multiple layers.
- Import-boundary tests exist: fixed.

## Final Recommendation

Do not ship as-is. Ship only after targeted fixes for:

1. Direct execution begin/status plan-fidelity gate parity.
2. Public late-stage reachability for final-review dispatch and branch finish progression.
3. Prompt budget validation failures.
4. Public-flow test realism gaps.
5. Public-output follow-up/argv clarity.

The runtime is closer to the target end state than earlier failure histories, especially around stale task closure and receipt/projection control-plane leakage. The remaining blockers are narrower but still structural because they affect public reachability and workflow gate authority.
