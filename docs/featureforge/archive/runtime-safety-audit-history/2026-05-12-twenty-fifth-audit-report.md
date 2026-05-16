# Twenty-Fifth Runtime Safety Audit Report

Date: 2026-05-12

Audited worktree: `/Users/dmulcahey/.codex/worktrees/5d19/featureforge`

Baseline diff hash: `0f5d375a0e9453d970fee65afff4631bcea21cd8e365d945f545e42d072a4ec3`

Note: some audit subagents reported `ab8684c424c74eeea7c63b11c50ffa02eb7642b2c36e8ab8ab22f60db568c1bb` after hashing `git diff --binary`. The normal text diff hash remained the baseline value. This was a hash-stream mismatch, not workspace drift.

## Executive Verdict

Ship candidate: no.

Close but not done: yes.

Still structurally unsafe: not broadly. The public CLI, receipt/projection authority boundary, plan-fidelity routing, prompt packaging, and public/private test separation are substantially improved. One P1 stale-route issue can still send a non-task stale boundary through a parked `begin`, and one P2 split-decisioning issue still lets final-review dispatch blocked output synthesize public commands from reason codes instead of preserving router-owned `RouteDecision` surfaces.

Recommendation: ship only after targeted fixes in `docs/featureforge/archive/runtime-safety-audit-history/plans/2026-05-12-runtime-signal-noise-twenty-fifth-audit-remediation.md`.

## What Is Genuinely Fixed

- Public CLI reachability is materially better. `begin`, `close-current-task`, and `advance-late-stage` own the normal public transitions; no blocker/high reachability issue was found.
- Typed public route surfaces are now the executable contract. Operator and status expose `recommended_public_command_argv` / `recommended_public_command_template`, while display `recommended_command` is treated as advisory.
- Public-flow tests are mostly real shipped-runtime tests. The protected public-flow suites use the compiled binary and scanner guards prevent internal helper drift.
- Receipt/provenance/projection artifacts no longer appeared to act as control-plane truth after an authoritative task closure exists.
- Plan-fidelity is artifact/parser based and requires current five-surface fidelity at final handoff without depending on hidden receipt recording.
- Prompt budgets and generated skill freshness are enforced. Reviewer recursion prevention is prompt-text scoped to reviewer prompts.
- Modularization improved: `next_action.rs` is a facade, `router.rs` delegates route choice through `plan_runtime_route`, and public route DTO goldens capture smaller external behavior.

## What Remains Risky

- Non-task stale boundaries can be masked by a parked resume route because `stale_resume_begin_route_candidate` treats `None` stale task as compatible with any `resume_task`.
- Final-review dispatch blocked output still has reason-code-specific direct command synthesis before falling back to route-owned surfaces.
- Test and documentation signal is near saturation. Some boundary tests pin private helper/module shape, and the public-flow proof script includes a scanner self-test that docs already classify as gate coverage rather than production-flow proof.
- Branch-closure identity helpers still reload transition state repeatedly within gate flows.
- Workflow doctor synthetic gate-review classification duplicates reason-code family logic inside presentation code.
- Active skill handoff wording can send agents through final review before document-release, creating a possible document-release/final-review bounce.
- Hidden-command denial vocabulary is split across runtime and tests.

## Concrete Dead Ends Still Possible

1. A branch/milestone stale target with a parked `resume_task` / `resume_step` can route to public `begin` because `stale_resume_begin_route_candidate` accepts `stale_task == None` as a match. This violates the diagnostic-only rule for resume fields.
2. Final-review dispatch blocked output can recommend a directly synthesized `repair-review-state` or `advance-late-stage` command based on reason codes, bypassing the route-plan decision object.
3. `subagent-driven-development` completion handoff can run terminal review before document-release, then `finishing-a-development-branch` can require document-release and stale the completed review.

## Concrete Churn Sources Still Possible

- Public-flow script naming overstates proof by mixing scanner self-tests with compiled public-flow suites.
- Boundary tests lock private module/helper shape in ways that can fail on harmless refactors.
- Phase-detail literals remain allowed in many test files instead of being reserved for public goldens or shared test constants.
- Repeated branch-closure identity loads in gate flows can add avoidable IO.
- Hidden command tokens exist in multiple local lists.

## Public/Private Test Mismatch Assessment

No blocker mismatch found. Public-flow suites use the compiled CLI and static scanners reject internal helper calls. Internal helpers are quarantined under internal-only test files. The remaining issue is signal classification: `public_flow_scan_contracts` is useful static gate coverage, but it is not itself public-flow proof and should not be mixed into the public-flow proof script without clearer naming.

## Receipt/Evidence/Projection Control-Plane Assessment

No actionable control-plane leakage found. Current task closure remains the task-boundary authority. Projection materialization is explicit/read-model oriented. Summary/projection/receipt freshness diagnostics did not appear to force reentry after current pass/pass closure state.

## Prompt Surface And Packaging Assessment

Generated docs are fresh and budgeted. Mandatory route law remains top-level in route-owning skills and detailed binding lives in `references/operator-route-authority.md`. One active prompt sequence issue remains in `subagent-driven-development`: terminal review ordering conflicts with document-release sequencing.

## Modularization And Split-Decisioning Assessment

Main flow is more centralized, but not complete. Route planning owns most route selection, but `record_review_dispatch_blocked_output_from_gate` still synthesizes final-review follow-up commands from reason codes. Workflow doctor presentation still classifies synthetic gate-review reasons locally. Some boundary tests now protect implementation shape more than semantic ownership.

## Reviewer Recursion Assessment

No runtime/env recursion enforcement was found. Reviewer recursion prevention is prompt-text scoped, and reviewer prompts prohibit launching additional subagents.

## Validation Results

- `cargo fmt --check`: passed.
- `node scripts/gen-skill-docs.mjs --check`: passed.
- `node scripts/gen-agent-docs.mjs --check`: passed.
- `node --test tests/codex-runtime/*.test.mjs`: passed, 135/135, 460.76175s.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed, 36.62s after clean rebuild.
- Checked no active `cargo nextest`, `cargo-nextest`, `nextest run`, or `target/debug/deps/` process before full test cycle: clean.
- `cargo nextest run --all-targets --all-features --no-fail-fast --status-level fail --final-status-level slow`: passed, 1681/1681, nextest run time 136.796s, wall time 214.06s after clean rebuild.

The full Rust test cycle was under the 4-5 minute threshold, so no clean/rerun performance remediation was triggered.

## Prioritized Findings

### Blocker

None.

### High

1. Non-task stale targets can be bypassed by parked resume begin.
   - Type: user-facing churn / architecture / test issue.
   - Refs: `src/execution/stale_target_projection.rs:664`, `src/execution/repair_target_selection.rs:351`, `src/execution/route_plan/stale_repair_target.rs:69`, `src/execution/route_plan.rs:441`.
   - Impact: branch/milestone stale states can route to downstream `begin` without proving the stale target is the same task.

### Medium

1. Final-review blocked output synthesizes commands from reason codes.
   - Type: split-decisioning / architecture.
   - Refs: `src/execution/state/runtime_methods.rs:687`.
   - Impact: gate output can bypass route-owned command surfaces.

2. Public-flow script includes scanner self-test under public-flow proof.
   - Type: signal/noise / test-maintenance.
   - Refs: `scripts/run-public-runtime-flow-tests.sh:7`, `docs/testing.md:140`.
   - Impact: release evidence can overstate what the public-flow gate proves.

3. Boundary tests pin private implementation shape.
   - Type: test-maintenance / architecture.
   - Refs: `tests/runtime_module_boundaries.rs:1611`, `tests/runtime_module_boundaries.rs:2480`, `tests/public_cli_flow_contracts.rs:1043`.
   - Impact: harmless internal refactors can fail tests and add workflow churn.

4. `subagent-driven-development` handoff orders terminal review before document-release.
   - Type: public-output / agent UX.
   - Refs: `skills/subagent-driven-development/SKILL.md.tmpl:198`, `skills/subagent-driven-development/SKILL.md:273`, `skills/finishing-a-development-branch/SKILL.md:135`.
   - Impact: agents can stale a just-completed final review by running document-release afterward.

5. Branch-closure identity helpers reload transition state repeatedly.
   - Type: architecture / performance.
   - Refs: `src/execution/status_assembly.rs:1669`, `src/execution/status_assembly.rs:1730`, `src/execution/status_assembly.rs:1788`, `src/execution/state/runtime_methods.rs:248`.
   - Impact: avoidable repeated IO in gate/status hot paths.

6. Workflow doctor synthetic gate-review classification duplicates reason families.
   - Type: vocabulary / split-decisioning.
   - Refs: `src/workflow/operator.rs:572`, `src/workflow/operator.rs:585`.
   - Impact: presentation code can drift from execution-owned reason classification.

### Low

1. Hidden-command deny lists are not fully centralized.
   - Type: test drift / cleanup.
   - Refs: `tests/workflow_shell_smoke.rs:480`, `tests/public_cli_flow_contracts.rs:92`, `tests/support/public_flow_scan.rs:1387`, `src/execution/command_eligibility.rs:1243`.
   - Impact: narrower smoke assertions could miss future hidden late-stage command regressions.

2. Phase-detail literal centralization is only partial.
   - Type: test-maintenance.
   - Refs: `tests/runtime_module_boundaries.rs:6183`, `tests/runtime_module_boundaries.rs:6191`.
   - Impact: route vocabulary can spread across non-golden tests.

## Checklist

### Public CLI / Reachability

- Public `begin` can seed preflight: fixed.
- No normal flow needs `plan execution preflight`: fixed.
- No normal flow needs `record-review-dispatch`: fixed.
- No normal flow needs `gate-review`: fixed.
- No normal flow needs `gate-finish`: fixed.
- No normal flow needs `rebuild-evidence`: fixed.
- No normal flow needs low-level late-stage recorders: fixed, but hidden-token denial lists should be centralized.
- Operator never recommends hidden/debug commands: fixed.
- Status never exposes hidden/debug commands as next actions: fixed.
- Public recommended argv is executable by shipped CLI: fixed.

### Plan Review

- Plan-fidelity no longer uses hidden runtime receipt recording: fixed.
- Plan-fidelity artifact is parseable and not overly hand-format-sensitive: fixed enough; parser is strict by design.
- Engineering-review edits do not bounce back to fidelity early: fixed.
- Final engineering-approved handoff requires current five-surface fidelity: fixed.
- Active docs do not teach plan-fidelity receipt recording: fixed.
- Old `plan_fidelity_receipt` fields are gone or historical only: fixed.

### Execution Runtime

- Current task closure is begin-time authority: fixed.
- Current closure cannot appear in stale closures: fixed.
- Close-current-task can refresh current dispatch internally: fixed.
- Stale dispatch does not block public close: fixed.
- Receipt/projection diagnostics do not trigger reentry: fixed.
- Summary hash drift does not trigger reentry when pass/pass closure is current: fixed.
- Cycle-break clears after current closure: fixed.
- `resume_task` is not treated as authoritative unless exact command is begin for same task/step: still broken for targetless stale boundaries.
- Repair-review-state cannot loop on same route: fixed.
- Runtime reconcile handles targetless stale states: partially fixed; blocked by resume bypass finding.

### Evidence / Projection

- Normal commands do not dirty tracked approved plan/evidence markdown: fixed.
- Projection materialization is explicit and not part of progress: fixed.
- Runtime-owned projection paths do not stale task/branch closures: fixed.
- Supersession is append-only and does not rewrite proof: fixed.
- Evidence is audit/projection, not control plane: fixed.

### Tests

- Public-flow tests do not call internal helpers: fixed.
- Internal helpers are quarantined in internal-unit-only tests: fixed.
- Static tests catch hidden helper use in public-flow tests: fixed, with deny-list centralization cleanup remaining.
- Replay tests cover historical dead ends: fixed.
- Liveness model catches repeated route signatures: fixed.
- Node/doc contracts pass: fixed.
- Prompt budget test passes: fixed.

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
- New modules have cohesive responsibilities: partially fixed.
- No new catch-all module replaces old monoliths: fixed.
- Phase/reason strings are centralized: partially fixed.
- Public command authority is typed, not string-parsed: fixed, with final-review blocked output exception.
- Router/read-model/mutation guards share decision objects: partially fixed.
- Import-boundary tests exist: fixed, but some are over-specific.
