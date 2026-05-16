# Thirty-Second Runtime Safety Audit Report

Audit date: 2026-05-14
Codebase: `/Users/dmulcahey/.codex/worktrees/5d19/featureforge`
Method: clean-context parallel audit subagents A-I, static inspection, validation runs, and local follow-up inspection.

## Executive Verdict

Close, but not done. Do not ship this branch yet.

The latest implementation materially improved runtime safety: typed public argv/template authority is now the practical route contract, stale-target selection is more centralized, late-stage precedence moved out of workflow presentation, and current pass/pass closures no longer appear to be undermined by summary/projection drift. However, the audit still found actionable issues in the exact failure classes this branch is trying to eliminate:

- Derived review-state overlays can still become a progress latch.
- Pre-closure unit-review/task-verification receipt diagnostics still participate in task-boundary readiness.
- Active audit-remediation plan discovery has multiple current candidates.
- Source archive packaging protects Markdown companions better than non-Markdown skill assets.
- Current-closure target selection and execution-template validation still have duplicated decision points.
- Public JSON still has a few executable-looking fields or labels that can mislead agents.

Recommendation: ship only after targeted fixes.

## What Is Genuinely Fixed

- Public runtime transitions are reachable through public commands. Subagent A found normal `begin`, `close-current-task`, and `advance-late-stage` paths available from the shipped CLI surface in `src/cli/plan_execution.rs` and dispatched through `src/lib.rs`.
- `begin` owns preflight setup. Public replay coverage includes `public_replay_begin_owns_allowed_preflight_without_hidden_command` in `tests/public_replay_churn.rs`.
- `close-current-task` owns current dispatch refresh/closure recording. Coverage includes `public_close_current_task_records_positive_closure_after_stale_dispatch_lineage_without_dispatch_id` in `tests/workflow_shell_smoke.rs`.
- `advance-late-stage` owns branch closure, release readiness, final review, QA, and finish progression through the aggregate command in `src/execution/commands/advance_late_stage.rs`.
- Typed command authority is the main executable contract. `recommended_public_command_argv` and `recommended_public_command_template` are produced through `src/execution/command_eligibility.rs`; operator input materialization runs through Rust in `src/workflow/operator.rs::apply_operator_template_inputs`.
- Plan-fidelity review no longer depends on hidden runtime receipts. `src/contracts/plan.rs` aligns active runtime contract language with fresh-context subagent provenance.
- Engineering-review edits do not appear to bounce immediately back into plan-fidelity. Subagent D found current five-surface fidelity is required at implementation handoff, not prematurely during engineering review edits.
- Reviewer recursion prevention remains prompt-text scoped. Subagent F did not find runtime/env recursion enforcement.
- Prompt budget and generated-doc checks passed.
- Performance remediation was required and completed during audit validation. The repeat clean full suite initially exceeded the time gate; the internal cycle test was refactored to preserve semantic coverage without replaying unnecessary begin/complete cycles. Clean full nextest then passed in 3:21.31.

## What Remains Risky

- Derived overlays are still treated as repairable truth in routing. Missing derived review-state fields add `derived_review_state_missing` in `src/execution/status_assembly.rs:508`, become blocking records in `src/execution/status_assembly/blocking_records.rs:328`, route through review-state repair in `src/execution/route_plan/follow_up.rs:106`, and are restored by `src/execution/review_state.rs:2610`.
- Receipt/provenance parsing still affects pre-closure readiness. `src/execution/status_support.rs:398` suppresses diagnostics only after a current positive closure exists; before that, `src/execution/closure_diagnostics.rs:253` and `src/execution/closure_diagnostics.rs:355` parse unit-review and task-verification receipt files into diagnostic reason codes that can affect readiness and next-action classification.
- Current task closure target selection is duplicated. `src/execution/route_plan.rs:453` and `src/execution/status_assembly/blocking_records.rs:352` both use `status.current_task_closures.first()` instead of a shared selector.
- Execution route template validation is split between executable argv validation in `src/execution/command_eligibility/execution_target.rs:56` and template bindability validation in `src/execution/status_assembly/exact_route_template.rs:106`.
- `next_action` is still schema-visible as an enum without an explicit non-executable description. `src/execution/status.rs:517`, `src/workflow/status.rs:1426`, `schemas/workflow-operator.schema.json:719`, and `schemas/plan-execution-status.schema.json:1128` expose it as a bare `$ref`.
- `src/execution/review_state.rs:94` and `src/execution/review_state.rs:102` use `recommended_command` for a prose instruction after reconciliation. That conflicts with the branch-wide rule that `recommended_command` is display-only compatibility text and never executable authority.
- Projection-rebuild failures use `manual_required` labels even though the remediation says not to manually repair artifacts. See `src/execution/commands/common/mutation_guards.rs:499`.
- External-review-ready wording is overbroad. `src/execution/status_support.rs:308` says "external review or verification result" may justify the flag, but the flag should be used only when an external review result exists.

## Concrete Dead Ends Still Possible

1. A runtime with authoritative review-state events but missing derived overlays can route to `repair-review-state` even though the overlay is a projection/cache. This can keep agents repairing FeatureForge surfaces instead of following authoritative runtime state.
2. A task with no current positive closure can still have readiness shaped by unit-review/task-verification receipt files. If those files are missing, malformed, or stale, the agent can be pushed toward reentry semantics before authoritative closure state is considered sufficient.
3. Schema-only consumers can read `next_action` values such as `repair review state` or `run QA` as executable commands because the schemas do not explicitly say they are diagnostic/display context.
4. `repair-review-state` reconciliation can return a `recommended_command` field containing prose. An agent that learned to avoid parsing `recommended_command` elsewhere still sees a conflicting field name on this surface.
5. Projection materialization output can return `manual_required`, which contradicts the message text and can invite manual artifact repair loops.

## Concrete Churn Sources Still Possible

- Multiple active audit-remediation plans remain under `docs/featureforge/plans`: `2026-05-14-runtime-signal-noise-thirtieth-audit-remediation.md` and `2026-05-14-runtime-safety-thirty-first-audit-remediation.md`. Adding a thirty-second plan without archiving completed/superseded plans worsens active discovery churn.
- Broad forbidden vocabulary in `tests/codex-runtime/skill-doc-contracts.test.mjs:622` is protecting real failures but is becoming a wording-governance surface. It should be narrowed to actionable command/help leakage where possible.
- Source archive verification enumerates Markdown companions in `scripts/verify-source-archive.mjs:10`, while skill-local script/assets referenced from companions are protected only by isolated tests or not at all.
- Public route goldens and schema annotations are useful, but their contract scope needs to stay focused on externally visible behavior rather than incidental compatibility shapes.

## Public/Private Test Mismatch Assessment

Mostly improved, with a remaining packaging-route gap.

Public-flow tests are strongly guarded against internal helpers. Subagent B found no current public-flow test directly using private helpers for the claimed public route behavior. Static guard coverage remains strong in `tests/public_cli_flow_contracts.rs` and `tests/support/public_flow_scan.rs`.

The remaining mismatch is that public-flow route tests exercise the cargo-built `CARGO_BIN_EXE_featureforge`, while checked-in/prebuilt runtime binaries are only covered by help/version style smoke coverage. If "shipped runtime" includes `bin/featureforge` or prebuilts, there is no public-route replay proving those artifacts expose the same typed route behavior.

The liveness model checker is valuable internal semantic proof, but not a substitute for public CLI replay of shipped artifacts.

## Receipt/Evidence/Projection Control-Plane Assessment

Partially fixed.

Current positive pass/pass task closure now suppresses the old receipt/projection drift paths. Current closures no longer appear in stale closures, summary hash drift is not treated as reentry once authoritative pass/pass closure exists, and projection materialization is not normal progress.

Still unsafe:

- Derived review-state overlays are not purely diagnostic. Missing overlays can create blocking records and route to repair.
- Pre-current-closure receipt diagnostics still parse unit-review and task-verification artifacts and feed reason codes.
- Review dispatch lineage remains control-plane for `close-current-task`; this appears intentional, but stale lineage must stay refreshable by the public aggregate command.

## Prompt-Surface And Packaging Assessment

Mostly positive, near the signal/noise edge.

Generated skills are within budget, generated docs are fresh, mandatory route law remains top-level, and reviewer recursion prevention is prompt-only. The strongest prompt improvement is that high-use skills now point agents at operator JSON and typed argv/template surfaces rather than memorized command strings.

Remaining issue: source-archive verification protects Markdown companions but not the full transitive set of non-Markdown skill-local helpers. Examples include:

- `skills/brainstorming/visual-companion.md:29` references `scripts/helper.js`.
- `skills/brainstorming/visual-companion.md:41` references `scripts/start-server.sh`.
- `skills/systematic-debugging/root-cause-tracing.md:101` references `find-polluter.sh`.
- `skills/writing-skills/SKILL.md:84` references `graphviz-conventions.dot` and `render-graphs.js`.

## Modularization And Split-Decisioning Assessment

Improved but still not clean.

The execution flow is much closer to the intended shape: CLI args -> command module -> transition guard -> event append -> reducer -> read model -> route decision -> workflow/operator presentation. Late-stage precedence is execution-owned. Stale-target selection is centralized for major read paths.

Remaining split decisioning:

- `src/execution/route_plan.rs:453` has a local fallback selector for current task closures.
- `src/execution/status_assembly/blocking_records.rs:352` repeats current closure positional selection.
- Template bindability and executable argv required-argument logic are separate decision implementations.
- Boundary tests do not yet forbid direct positional reads of `current_task_closures.first()` outside the owning helper.

## Reviewer Recursion Assessment

Pass. Subagent F found reviewer recursion prevention remains prompt-scoped and reviewer-prompt scoped. Reviewer prompts prohibit additional subagent launches. No runtime/env recursion guard was found.

## Validation Results

Passed before performance remediation:

- `node scripts/gen-skill-docs.mjs --check`
- `node scripts/gen-agent-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`: 141/141 passed
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo nextest run --all-targets --all-features --no-fail-fast`: 1768/1768 passed, but wall time 318.89s

Performance remediation triggered:

- Repeat clean full nextest still exceeded the 4-5 minute threshold: 1768/1768 passed, 5:07.81.
- The internal cycle test was refactored to avoid unnecessary replay loops while preserving review-remediation and cycle-break assertions.

Passed after performance remediation:

- `cargo nextest run --all-targets --all-features --no-fail-fast` after `cargo clean`: 1769/1769 passed, 3:21.31.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.

## Prioritized Findings

### Blocker

None found that make every normal public route unusable. The branch is not ship-ready because several High findings are directly in the historical failure classes.

### High

1. Derived review-state overlays still act as progress latches.
   - Type: user-facing dead end, architecture issue.
   - References: `src/execution/status_assembly.rs:508`, `src/execution/status_assembly/blocking_records.rs:328`, `src/execution/route_plan/follow_up.rs:106`, `src/execution/review_state.rs:2610`.
   - Risk: agents can be routed into repair because a derived overlay is missing even when authoritative events exist.

2. Pre-closure receipt diagnostics still influence task-boundary readiness.
   - Type: user-facing dead end, control-plane leakage.
   - References: `src/execution/status_support.rs:398`, `src/execution/closure_diagnostics.rs:253`, `src/execution/closure_diagnostics.rs:355`.
   - Risk: receipt/provenance artifacts still shape progress before current closure exists.

3. Multiple active audit-remediation plans remain discoverable.
   - Type: documentation/workflow churn.
   - References: `docs/featureforge/plans/2026-05-14-runtime-signal-noise-thirtieth-audit-remediation.md:3`, `docs/featureforge/plans/2026-05-14-runtime-safety-thirty-first-audit-remediation.md:3`.
   - Risk: agents can treat completed/superseded remediation plans as active current work.

4. Non-Markdown skill-local companion assets are not protected by the source archive verifier.
   - Type: packaging issue.
   - References: `scripts/verify-source-archive.mjs:10`, `skills/brainstorming/visual-companion.md:29`, `skills/systematic-debugging/root-cause-tracing.md:101`, `skills/writing-skills/SKILL.md:84`.
   - Risk: shipped skills can reference helper scripts/assets that are missing from archive/source verification.

### Medium

5. Current task closure target selection is duplicated.
   - Type: architecture issue.
   - References: `src/execution/route_plan.rs:453`, `src/execution/status_assembly/blocking_records.rs:352`.
   - Risk: route/status can diverge when multiple current closures exist or projection ordering changes.

6. Execution route template validation is split.
   - Type: architecture/test drift issue.
   - References: `src/execution/command_eligibility/execution_target.rs:56`, `src/execution/status_assembly/exact_route_template.rs:106`.
   - Risk: argv and template eligibility can diverge.

7. `next_action` schema fields lack non-executable descriptions.
   - Type: public-output/agent-UX issue.
   - References: `src/execution/status.rs:517`, `src/workflow/status.rs:1426`, `schemas/workflow-operator.schema.json:719`, `schemas/plan-execution-status.schema.json:1128`.
   - Risk: schema-only consumers can execute display/diagnostic action labels.

8. Review-state reconciliation uses `recommended_command` for prose.
   - Type: public-output/agent-UX issue.
   - References: `src/execution/review_state.rs:94`, `src/execution/review_state.rs:102`, `src/execution/review_state.rs:774`.
   - Risk: conflicts with the branch-wide rule that executable authority lives only in typed argv/template.

9. Projection rebuild labels say `manual_required`.
   - Type: public-output issue.
   - Reference: `src/execution/commands/common/mutation_guards.rs:499`.
   - Risk: contradicts diagnostic text and can invite manual artifact repair loops.

10. External-review-ready guidance mentions verification-only work.
    - Type: public-output issue.
    - References: `src/execution/status_support.rs:308`, generated skills that repeat the flag law.
    - Risk: agents may pass `--external-review-result-ready` after local verification without a real external review result.

11. Public-flow tests do not replay typed routes through checked-in/prebuilt runtime artifacts.
    - Type: test realism issue.
    - References: `tests/bootstrap_smoke.rs`, `bin/featureforge`, `bin/prebuilt/manifest.json`.
    - Risk: cargo-built tests can pass while shipped binaries drift.

### Low

12. Hidden compatibility flags/functions still exist but are quarantined.
    - Type: compatibility cleanup.
    - References: hidden flags in `src/cli/plan_execution.rs`, internal late-stage primitives in `src/execution/commands/advance_late_stage.rs`.
    - Risk: low as long as public help/tests keep hiding them.

13. Broad prompt vocabulary scanners are near diminishing returns.
    - Type: signal/noise cleanup.
    - Reference: `tests/codex-runtime/skill-doc-contracts.test.mjs:622`.
    - Risk: future work may add scanners around scanners rather than deleting duplication.

## Required Checklist Status

### Public CLI / Reachability

- Public `begin` can seed preflight: fixed.
- No normal flow needs `plan execution preflight`: fixed.
- No normal flow needs `record-review-dispatch`: fixed.
- No normal flow needs `gate-review`: fixed for public route; hidden compatibility remains quarantined.
- No normal flow needs `gate-finish`: fixed for public route; hidden compatibility remains quarantined.
- No normal flow needs `rebuild-evidence`: fixed for progress, but projection/manual labels need cleanup.
- No normal flow needs low-level late-stage recorders: fixed for public route.
- Operator never recommends hidden/debug commands: fixed by inspected surfaces.
- Status never exposes hidden/debug commands as next actions: fixed by inspected surfaces.
- Public recommended argv is executable by shipped CLI: fixed for cargo-built CLI; not enough evidence for checked-in/prebuilt binaries.

### Plan Review

- Plan-fidelity no longer uses hidden runtime receipt recording: fixed.
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: fixed enough; residual metadata sensitivity remains controlled.
- Engineering-review edits do not bounce back to fidelity early: fixed.
- Final engineering-approved handoff requires current five-surface fidelity: fixed.
- Active docs do not teach plan-fidelity receipt recording: fixed by inspected surfaces.
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed.

### Execution Runtime

- Current task closure is begin-time authority: fixed after current closure exists.
- Current closure cannot appear in stale closures: fixed by inspected tests.
- Close-current-task can refresh current dispatch internally: fixed.
- Stale dispatch does not block public close: fixed by inspected tests.
- Receipt/projection diagnostics do not trigger reentry: partially fixed; still true after current closure, not fully true pre-closure.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` diagnostic only unless exact begin: fixed by inspected routes.
- Repair-review-state cannot loop on same route: not enough evidence for derived overlay case.
- Runtime reconcile handles targetless stale states: fixed by inspected routes.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed by inspected surfaces.
- Projection materialization is explicit and not part of progress: fixed, but labels need cleanup.
- Runtime-owned projection paths do not stale task/branch closures: fixed by inspected tests.
- Supersession is append-only and does not rewrite proof: fixed by inspected event model.
- Evidence is audit/projection, not control plane: partially fixed due pre-closure receipt diagnostics.

### Tests

- Public-flow tests do not call internal helpers: fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: fixed.
- Replay tests cover historical dead ends: mostly fixed; natural-public-creation coverage remains limited.
- Liveness model catches repeated route signatures: fixed by inspected model.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.

### Prompt Surface

- Skill docs are within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: partially fixed; non-Markdown transitive assets need source archive coverage.
- Generated docs are fresh: fixed.
- Reviewer recursion prevention is prompt-only and reviewer-prompt scoped: fixed.
- No runtime/env recursion enforcement is introduced: fixed.
- Reviewer prompts prohibit launching additional subagents: fixed.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: improved.
- New modules have cohesive responsibilities: improved.
- No new catch-all module replaces old monoliths: mostly fixed.
- Phase/reason strings are centralized: improved, but scanner breadth remains noisy.
- Public command authority is typed, not string-parsed: fixed.
- Router/read-model/mutation guards share decision objects: partially fixed; current closure selector and template validation remain split.
- Import-boundary tests exist: fixed, but need selector-specific coverage.

## Recommendation

Do not ship yet. Implement the thirty-second audit remediation plan, then rerun the same verification and clean-context review loop. The next implementation should reduce conceptual surface area and decision duplication, not add another broad static-scanner layer unless it replaces a noisier one.
