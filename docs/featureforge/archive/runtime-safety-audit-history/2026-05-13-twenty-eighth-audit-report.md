# Twenty-Eighth Runtime Safety Audit

Date: 2026-05-13

Audited worktree: `/Users/dmulcahey/.codex/worktrees/5d19/featureforge`

## Executive Verdict

Close, but not done. Do not ship until the targeted findings below are remediated.

The latest implementation materially improved runtime safety: typed public command surfaces remain the executable contract, task-boundary closure and stale-target authority no longer showed new control-plane leaks, and the post-review fix closed the reopen-as-begin resume/stale loophole. The remaining issues are not broad runtime dead ends, but they are actionable because they preserve exactly the wrong failure modes for this branch: split decision vocabulary, public diagnostic text that can send agents toward manual artifact reconstruction, and guard-layer bloat that now costs more maintenance signal than it returns.

## What Is Genuinely Fixed

- Public route execution is still typed. Status/operator copy `recommended_public_command_argv` and templates from route decisions instead of deriving executable commands from display text.
- Public `begin`, `close-current-task`, and `advance-late-stage` own the expected normal transitions. No audit agent found a normal path requiring a hidden/debug/compatibility command.
- Runtime-owned current task closure remains task-boundary authority. Projection-only stale facts and stale proof-artifact drift are diagnostic or recovery inputs, not task-boundary truth.
- Resume/stale precedence now has shared route facts, and the rereview remediation removed reopen-target authorization from the legal begin path.
- Plan-fidelity is artifact based and current-surface bound; active schemas expose review artifacts, not old proof-record fields.
- Generated skill docs are compacted and budgeted. Companion references are packaged and discoverable.

## What Remains Risky

- Two decision tokens are still raw strings in multiple modules: `task_closure_baseline_bridge_ready` and `finish_review_gate_already_current`.
- Public gate remediation text still says to regenerate contracts, reports, and evidence references. That can encourage manual artifact repair instead of returning agents to public route JSON and typed command surfaces.
- Some static guard tests have become a parallel architecture language: fine-grained per-file line caps, prose-checked scanner exceptions, script-comment assertions, and duplicated generated route-law assertions.

## Concrete Dead Ends Still Possible

No hard public CLI dead end was found in the runtime route surface. The closest user-facing risk is diagnostic text from `GateDiagnostic.remediation`: an agent can read “Regenerate the contract/report/evidence_refs” and spend time reconstructing proof artifacts manually rather than querying workflow/operator JSON and following the typed route.

## Concrete Churn Sources Still Possible

- `tests/runtime_module_boundaries.rs` line caps force mechanical edits when focused modules grow by a few lines, even when behavior remains correct.
- `tests/support/public_flow_scan.rs` exception metadata is partly prose-shaped; tests assert explanations rather than typed categories.
- `tests/public_cli_flow_contracts.rs` checks script comments that duplicate `docs/testing.md`.
- Generated skill route-law content is checked in both generator unit tests and broader doc-contract tests.

## Public/Private Test Mismatch Assessment

No actionable mismatch found. Public-flow proof is separated from internal helper compatibility. Public replay recovery uses compiled public CLI commands after explicit synthetic setup for impossible historical states. The liveness checker remains internal semantic coverage and is labeled as such.

## Control-Plane Assessment

No actionable control-plane leak found. Current task closure is authoritative. Stale/missing projections and stale proof-artifact signals do not override a current pass/pass closure. Public close refreshes stale dispatch lineage internally under public command ownership.

## Prompt-Surface And Packaging Assessment

No blocker. Skill docs are within budget, generated docs are fresh, companion references resolve, and reviewer recursion prevention is prompt-scoped. Residual prompt-surface concern is signal-to-noise: route-law assertions are repeated in more test layers than necessary.

## Modularization And Split-Decisioning Assessment

Actionable. Most routing boundaries are now coherent, but two token families remain split across production modules without a single owner. These are small but important because route and follow-up behavior depends on exact spelling.

## Reviewer Recursion Assessment

No actionable finding. Recursion prevention is prompt text only and reviewer-prompt scoped; no runtime/env enforcement was found.

## Validation Results

- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/*.test.mjs`: passed, 136/136.
- `git diff --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed from a clean build, real 47.30s.
- Targeted listed nextest shard:
  `cargo nextest run --all-features --no-fail-fast --test runtime_authority_contracts --test workflow_runtime --test workflow_shell_smoke --test workflow_entry_shell_smoke --test plan_execution --test plan_execution_final_review --test workflow_runtime_final_review --test contracts_execution_runtime_boundaries --test execution_query`: passed, 332/332, real 148.34s. The elapsed time includes waiting for in-flight audit-agent Cargo locks.
- `cargo test --test liveness_model_checker -- --nocapture`: passed, 32/32, real 2.01s. The printed panics are expected caught negative assertions.

## Findings

### High

None.

### Medium

1. Split reason-code vocabulary: `task_closure_baseline_bridge_ready`.

   Type: architecture issue / split decisioning.

   Evidence:
   - Producer: `src/execution/status_assembly/task_state.rs`, `add_task_closure_recording_ready_reason_codes`.
   - Consumer: `src/workflow/operator.rs`, task-closure recording text for `DETAIL_TASK_CLOSURE_RECORDING_READY`.
   - Existing central owner does not define the token: `src/execution/closure_diagnostics/reason_codes.rs`.
   - Existing boundary scanner misses it.

   Impact: a spelling drift changes whether operator output identifies the baseline-bridge route as replay-complete enough for close-current-task.

2. Split gate/requery decision vocabulary: `finish_review_gate_already_current`.

   Type: architecture issue / split decisioning.

   Evidence:
   - Producer: `src/execution/state/runtime_methods.rs`, `finish_review_gate_already_current`.
   - Consumers: `src/execution/state/runtime_methods.rs`, `gate_should_rederive_via_workflow_operator` and `apply_out_of_phase_gate_contract`; `src/execution/follow_up.rs`, `direct_gate_follow_up_from_reason_codes`.

   Impact: a spelling drift changes both operator requery behavior and direct follow-up classification for an out-of-phase finish gate.

3. Gate diagnostic remediation text points agents toward manual artifact regeneration.

   Type: public-output / agent UX issue.

   Evidence:
   - `src/execution/gates.rs` has many public gate remediation strings shaped as “Regenerate the contract/report/evidence_refs...”, including contract provenance, evaluation report provenance, and evidence-reference validation paths.

   Impact: agents can spend time reconstructing proof artifacts manually instead of querying public operator JSON and following typed argv/templates.

4. Fine-grained runtime module line caps are now low-signal.

   Type: test maintainability / signal-to-noise issue.

   Evidence:
   - `tests/runtime_module_boundaries.rs`, `FOCUSED_RUNTIME_MODULE_LINE_CAPS` and `focused_runtime_modules_have_line_caps`.

   Impact: harmless extractions or small helper additions can fail tests and force mechanical reshuffling unrelated to shipped behavior.

5. Public-flow scanner exception metadata is prose-heavy.

   Type: test maintainability / signal-to-noise issue.

   Evidence:
   - `tests/support/public_flow_scan.rs` centralizes exception reasons but returns long reason strings.
   - `tests/public_flow_scan_contracts.rs` asserts explanation prose for the liveness exclusion.

   Impact: tests protect scanner prose rather than scanner semantics.

### Low

1. Public-flow script comment text is tested while `docs/testing.md` already owns the explanation.

   Type: documentation/test signal issue.

   Evidence:
   - `tests/public_cli_flow_contracts.rs` asserts explanatory comments in `scripts/run-public-runtime-flow-tests.sh`.

2. Generated route-law snippets are asserted in both generator unit tests and broad doc-contract tests.

   Type: prompt-surface/test signal issue.

   Evidence:
   - `tests/codex-runtime/gen-skill-docs.unit.test.mjs` asserts route-law generation modes.
   - `tests/codex-runtime/skill-doc-contracts.test.mjs` reasserts phrase-level route-law content across generated skills.

## Checklist Classification

### Public CLI / Reachability

- Public `begin` can seed preflight: fixed.
- No normal flow needs hidden/debug preflight command: fixed.
- No normal flow needs hidden dispatch recording: fixed.
- No normal flow needs hidden review/finish gate commands: fixed for normal routing.
- No normal flow needs evidence rebuild: fixed.
- No normal flow needs low-level late-stage recorders: fixed.
- Operator/status do not recommend hidden/debug commands: fixed.
- Public recommended argv is executable by shipped CLI: fixed.

### Plan Review

- Plan-fidelity no longer uses hidden runtime proof recording: fixed.
- Plan-fidelity artifact is parseable: fixed, with strict-format residual risk.
- Engineering-review edits do not bounce back early: fixed.
- Final handoff requires current five-surface fidelity: fixed.
- Active docs do not teach old plan-fidelity proof recording: fixed.

### Execution Runtime

- Current task closure is begin-time authority: fixed.
- Current closure cannot appear as active stale closure: fixed by evidence inspected.
- Close-current-task can refresh dispatch internally: fixed.
- Stale dispatch does not block public close: fixed.
- Projection/proof diagnostics do not trigger reentry after current closure: fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` diagnostic unless exact public begin: fixed after rereview remediation.
- Repair-review-state cannot loop on same route: fixed by evidence inspected.
- Runtime reconcile handles targetless stale states: fixed.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed by tested evidence.
- Projection materialization explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale closures: fixed.
- Supersession append-only: fixed by evidence inspected.
- Evidence is audit/projection, not control plane: fixed.

### Tests

- Public-flow tests do not call internal helpers: fixed.
- Internal helpers quarantined: fixed.
- Static tests catch hidden helper use: fixed.
- Replay tests cover historical dead ends: fixed.
- Liveness catches repeated route signatures: fixed.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.
- Static guard signal-to-noise: partially fixed; further cleanup recommended.

### Prompt Surface

- Skill docs within budget: fixed.
- Mandatory law remains top-level: fixed.
- Companion references exist and are packaged: fixed.
- Generated docs fresh: fixed.
- Reviewer recursion prevention prompt-only: fixed.
- Reviewer prompts prohibit additional subagents: fixed.
- Prompt bloat pressure: partially fixed; route-law assertion duplication remains.

### Modularization

- `state.rs` and `mutate.rs` are not monoliths: fixed.
- New modules cohesive: mostly fixed.
- No new catch-all module replacing old monoliths: fixed.
- Phase/reason strings centralized: partially fixed.
- Public command authority typed: fixed.
- Router/read-model/mutation guards share decision objects: mostly fixed.
- Import-boundary tests exist: fixed, but some line-cap guards are too fine-grained.

## Recommendation

Do not ship yet. Implement targeted fixes for the two split runtime vocabularies and the public gate remediation wording. Then reduce or coarsen the low-signal static/prompt guard tests so the branch continues reducing conceptual surface area rather than adding more meta-infrastructure.
