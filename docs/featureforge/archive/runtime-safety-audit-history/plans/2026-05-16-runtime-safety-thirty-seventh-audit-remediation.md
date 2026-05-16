# Runtime Safety Thirty-Seventh Audit Remediation

## Workflow State

Engineering remediation plan for the thirty-seventh runtime-safety audit loop. This plan is active until every task below is implemented, fully verified, independently reviewed, and followed by another deep audit loop with the signal-to-noise auditor included.

## Plan Revision

1

## Execution Mode

Sequential implementation with full verification and clean-context review after each task. Do not use FeatureForge runtime/workflow/project skills. Do not allow reviewers or implementation subagents to spawn additional subagents. Before each full test cycle, verify no existing `cargo nextest`, `cargo-nextest`, `nextest run`, `cargo test`, `cargo clippy`, or Codex-runtime Node validation process is running.

If a full nextest run exceeds 4-5 minutes, run `cargo clean`, rerun once, and remediate if the regression repeats. If a full test suite run exceeds 10 minutes, stop immediately, run `cargo clean`, rerun, and enter performance remediation if the regression is repeatable.

## Goal

Remove the actionable findings from the thirty-seventh runtime-safety audit loop without adding more low-signal guard layers. The outcome should make the runtime and skills simpler to reason about:

- `workflow status` is unmistakably diagnostic, not route authority.
- Static scanners derive command and follow-up vocabulary from canonical runtime/test helpers.
- Public-flow source scanning tracks the production route/status module graph without hand-maintained blind spots.
- Workflow presentation uses one compatibility projection for recommended skill/reason text.
- Harness phase normalization uses `HarnessPhase`.
- `transfer` does not re-decide public mutation eligibility from workflow presentation fields.
- Runtime-remediation inventory matches current public replay coverage.

## Architecture

Preserve the current runtime architecture:

```text
CLI args
  -> command module
  -> transition guard
  -> event append
  -> reducer
  -> read model
  -> route decision
  -> workflow operator presentation
```

The remediation should reduce conceptual surface area. Prefer shared helpers and deleted duplicates over adding more scanner exceptions or prompt prose. Do not reintroduce hidden compatibility commands, receipt/provenance authority, manual artifact repair, or display-command execution.

## Change Surface

- `src/cli/workflow.rs`
- `src/lib.rs`
- `src/workflow/operator.rs`
- `src/workflow/status.rs`
- `src/execution/harness.rs`
- `src/execution/status_assembly/overlay.rs`
- `src/execution/status_assembly/late_stage.rs`
- `src/execution/route_plan/status_application.rs`
- `src/execution/commands/transfer.rs`
- `src/execution/command_eligibility.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/support/public_flow_scan.rs`
- `tests/public_cli_flow_contracts.rs`
- `tests/fixtures/runtime-remediation/README.md`
- `docs/featureforge/archive/runtime-safety-audit-history/README.md`
- generated schemas or fixtures only if the changed public output requires it

## Preconditions

- The thirty-sixth remediation is complete and archived.
- The thirty-seventh audit report is the source finding set for this plan.
- Do not use FeatureForge runtime/workflow/project skills.
- Use Rust best practices while modifying Rust code.
- Run `cargo clean` before each new audit-loop iteration.
- Before every full verification cycle, check that no previous cargo/nextest/clippy/test process is running.

## Known Footguns / Constraints

- Do not remove `workflow status` unless a task explicitly proves all public callers no longer need it. This plan only demotes its wording and text output to diagnostics.
- Do not weaken hidden-helper, display-command, or prompt-budget scanners.
- Do not replace hardcoded scanner lists with a different hardcoded list in another file.
- If a scanner must keep an exemption, document why the boundary needs it.
- Do not let tests become the new runtime authority. Static tests should consume production enums/helpers where possible.
- Do not hand-edit generated skill docs. This plan should not require skill doc changes unless a test proves otherwise.
- Do not broaden public JSON goldens for incidental shape churn.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| REQ-001 `workflow status` is diagnostic-only UX | Task 1 |
| REQ-002 Public mutation token scanner derives from typed command taxonomy | Task 2 |
| REQ-003 Follow-up token scanner derives from follow-up taxonomy/owners | Task 2 |
| REQ-004 Public-flow production source scanning has no route/status blind spots | Task 3 |
| REQ-005 Workflow skill/reason compatibility projection is centralized | Task 4 |
| REQ-006 Harness phase parsing uses canonical `HarnessPhase` | Task 5 |
| REQ-007 `transfer` uses public mutation authority instead of presentation rechecks | Task 6 |
| REQ-008 Runtime remediation inventory reflects FS-22 public replay proof | Task 7 |

## Task 1: Make `workflow status` Diagnostic-Only In Public UX

**Spec Coverage:** REQ-001

**Goal:** Prevent agents from treating `workflow status` text output or CLI help as executable route authority.

**Context:** The thirty-seventh audit found `src/cli/workflow.rs` still describes `status` as "public workflow routing", and `src/lib.rs::render_workflow_status` prints `next_skill` and reason codes without telling agents to use operator JSON for executable next steps.

**Constraints:**

- Keep the JSON status surface available as a diagnostic/read-model mirror.
- Do not remove typed argv fields from status JSON.
- Do not make `workflow status` another operator.
- Text output must point to one public route authority: `featureforge workflow operator --plan <plan> --json`.

**Done when:**

- `WorkflowCommand::Status` help says read-only diagnostic status, not public routing.
- `render_workflow_status` includes a clear diagnostic-only line and an operator JSON next-step line.
- Existing status text tests/goldens are updated only for the intentional wording change.
- No active prompt/doc scanner is weakened.

**Files:**

- `src/cli/workflow.rs`
- `src/lib.rs`
- `tests/fixtures/differential/workflow-status.json` only if required
- tests that assert workflow status text/help, if any

**Implementation Steps:**

1. Update the Clap `about` text for `WorkflowCommand::Status`.
2. Update `render_workflow_status` to prefix the output with diagnostic-only wording.
3. Include the exact operator JSON command shape using the rendered `plan_path`.
4. Search for tests or fixtures that assert old status wording and update them narrowly.
5. Add or update a regression assertion if no test currently covers the diagnostic-only wording.

**Validation Expectations:**

- Targeted status/help tests discovered during implementation.
- `cargo nextest run --test workflow_runtime --test workflow_shell_smoke --test workflow_entry_shell_smoke --no-fail-fast`
- Full strict clippy and full nextest before task review.

## Task 2: Derive Public Mutation And Follow-Up Scanner Vocabulary

**Spec Coverage:** REQ-002, REQ-003

**Goal:** Remove duplicated command/follow-up taxonomies from `tests/runtime_module_boundaries.rs`.

**Context:** Static scanners currently hardcode public mutation tokens in `public_mutation_tokens()` and follow-up tokens in `raw_tokens`. That protects real boundaries, but it is duplicated policy.

**Constraints:**

- Do not weaken scanner coverage.
- Do not move duplicate lists into a new test-only constant with the same drift risk.
- Prefer production-owned enums/helpers with test-only accessors when needed.
- Keep scanner fixture tests readable.

**Done when:**

- Public mutation token scanning uses values derived from `PublicCommandKind` or a canonical runtime public command taxonomy.
- Follow-up token scanning uses values derived from `FollowUpKind` or canonical follow-up route-token ownership.
- Existing scanner tests still catch raw token comparisons and match arms in non-owner production modules.
- Allowlist entries are minimized and documented as boundary-specific owner exceptions.

**Files:**

- `src/execution/command_eligibility.rs`
- `src/execution/command_eligibility/command_kind.rs`
- `src/execution/follow_up.rs`
- `src/execution/review_route_tokens.rs`
- `tests/runtime_module_boundaries.rs`
- optional test-support helper if it consumes production constants instead of duplicating them

**Implementation Steps:**

1. Inspect existing public command and follow-up enums/helpers.
2. Add narrow `#[cfg(test)]` accessors if production APIs do not expose canonical token lists.
3. Replace `public_mutation_tokens()` with a derived set built from the canonical public command taxonomy.
4. Replace the follow-up `raw_tokens` array with canonical follow-up tokens plus documented route-token owner constants.
5. Keep fixture tests proving that raw `"begin"` and `"complete"` literals are still rejected outside owners.
6. Confirm no new production code depends on test-only helper APIs.

**Validation Expectations:**

- `cargo nextest run --test runtime_module_boundaries --no-fail-fast`
- Full strict clippy and full nextest before task review.

## Task 3: Discover Public-Flow Production Authority Sources

**Spec Coverage:** REQ-004

**Goal:** Replace the hand-maintained public-flow production authority manifest with source discovery that includes current route/status modules.

**Context:** `tests/support/public_flow_scan.rs::production_command_authority_files()` lists a small set of files and misses important current modules under `src/execution/route_plan/**`, `src/execution/status_assembly/**`, and `src/execution/public_recovery.rs`.

**Constraints:**

- Do not scan generated archives or historical docs.
- Do not scan CLI parser/argv construction owner files when the test is about display-command parsing as production route authority.
- Keep exemptions explicit, narrow, and documented.
- Preserve the public-flow scanner's ability to catch production `PublicCommand::parse_display_command` usage.

**Done when:**

- `production_command_authority_files()` discovers production Rust source files from relevant source roots instead of listing only selected files.
- Route/status modules added in the recent modularization are included automatically.
- CLI/parser/test-only owner exemptions are documented close to the filter.
- Public CLI flow contract tests prove the discovery includes `src/execution/route_plan/**`, `src/execution/status_assembly/**`, and `src/execution/public_recovery.rs`.

**Files:**

- `tests/support/public_flow_scan.rs`
- `tests/public_cli_flow_contracts.rs`
- optional `tests/public_flow_scan_contracts.rs`

**Implementation Steps:**

1. Replace the fixed array in `production_command_authority_files()` with recursive discovery under `src/execution` and `src/workflow`.
2. Filter out test-only modules and explicit command parser/renderer owner files.
3. Keep `src/execution/commands/**` included through the same discovery path, not a second extension call.
4. Add contract assertions for representative discovered files.
5. Verify the display-command parsing scanner still catches injected violations.

**Validation Expectations:**

- `cargo nextest run --test public_cli_flow_contracts --test public_flow_scan_contracts --no-fail-fast`
- Full strict clippy and full nextest before task review.

## Task 4: Centralize Workflow Skill And Reason Compatibility Projection

**Spec Coverage:** REQ-005

**Goal:** Ensure workflow handoff/status presentation uses one compatibility projection for recommended skill and reason text.

**Context:** `src/workflow/operator.rs` maps phases to skills/reasons in handoff output and repeats reason text in `reason_text()`. `src/workflow/status.rs` assigns planning route skills directly while documenting `recommended_skill` as a compatibility projection.

**Constraints:**

- Do not change authoritative route status semantics.
- Do not make `recommended_skill` route authority.
- Keep public JSON compatibility fields stable unless tests prove an intentional correction.
- Avoid a broad rewrite of workflow status.

**Done when:**

- A shared helper owns compatibility recommended skill/reason projection for workflow presentation.
- Handoff and status presentation reuse the helper instead of repeating phase-to-skill/prose maps.
- The helper clearly documents that it is presentation compatibility, not route authority.
- Existing workflow/status/operator tests still pass or are updated for intentional wording only.

**Files:**

- `src/workflow/operator.rs`
- `src/workflow/status.rs`
- optional new `src/workflow/recommendation.rs` or equivalent cohesive helper module
- relevant workflow tests/goldens

**Implementation Steps:**

1. Identify the minimum shared input needed for recommended skill/reason projection.
2. Extract a helper or module that projects compatibility recommendation data from route/context.
3. Replace local handoff mapping with the helper.
4. Replace direct status planning route skill/reason projection where it duplicates the same rule.
5. Add focused tests or boundary assertions that both surfaces share the helper path.

**Validation Expectations:**

- `cargo nextest run --test workflow_runtime --test workflow_runtime_final_review --test workflow_shell_smoke --no-fail-fast`
- Full strict clippy and full nextest before task review.

## Task 5: Use Canonical Harness Phase Normalization

**Spec Coverage:** REQ-006

**Goal:** Replace local harness phase string parsing with `HarnessPhase`.

**Context:** Canonical `HarnessPhase::as_str` and `FromStr` exist in `src/execution/harness.rs`, but `status_assembly` and `route_plan/status_application` duplicate phase matching.

**Constraints:**

- Preserve serialized phase strings.
- Do not widen accepted phase spellings unless existing `FromStr` already accepts them.
- Do not move runtime route decisions into harness code.

**Done when:**

- `src/execution/status_assembly/overlay.rs`, `src/execution/status_assembly/late_stage.rs`, and `src/execution/route_plan/status_application.rs` call canonical harness phase parsing/string helpers where they currently duplicate phase matching.
- Tests cover the known overlay/late-stage/status application cases.
- No duplicate literal match remains for the canonical harness phase variants outside owner/tests unless documented.

**Files:**

- `src/execution/harness.rs`
- `src/execution/status_assembly/overlay.rs`
- `src/execution/status_assembly/late_stage.rs`
- `src/execution/route_plan/status_application.rs`
- relevant execution/status tests

**Implementation Steps:**

1. Inspect local phase parsing/mapping logic and compare to `HarnessPhase::from_str`.
2. Add any small ergonomic helper to `HarnessPhase` if needed.
3. Replace duplicate matches with canonical parsing.
4. Add/adjust scanner or unit assertions only if they prevent the exact duplication from returning without broadening noise.

**Validation Expectations:**

- `cargo nextest run --test execution_query --test runtime_authority_contracts --test workflow_runtime --no-fail-fast`
- Full strict clippy and full nextest before task review.

## Task 6: Remove Transfer Presentation Recheck

**Spec Coverage:** REQ-007

**Goal:** Make `transfer` rely on public mutation authority instead of rechecking workflow-operator presentation phase strings.

**Context:** `src/execution/commands/transfer.rs` calls `require_public_mutation(...)` and then recomputes `operator_routes_handoff` from `operator.phase_detail` and `operator.phase`.

**Constraints:**

- Preserve fail-closed behavior when transfer is not publicly authorized.
- Do not remove the public mutation guard.
- Do not use raw operator display strings as a second eligibility decision.
- Keep any remaining presentation check clearly diagnostic, not authoritative.

**Done when:**

- Transfer handoff eligibility is derived from the same public mutation decision or exact route request used by `require_public_mutation`.
- Raw `operator.phase`/`operator.phase_detail` checks no longer decide whether transfer is allowed.
- Regression tests prove authorized handoff transfer still works and non-authorized transfer still fails.

**Files:**

- `src/execution/commands/transfer.rs`
- `src/execution/commands/common/mutation_guards.rs` if the guard needs to return decision metadata
- `src/execution/command_eligibility.rs` if exact-route metadata must be exposed
- relevant transfer/workflow tests

**Implementation Steps:**

1. Inspect `require_public_mutation` return type and exact-route decision data.
2. If needed, extend the guard to return a typed authorization object without broadening callers.
3. Replace the `operator_routes_handoff` presentation check with the typed authorization result.
4. Keep diagnostic messages specific when transfer is blocked.
5. Add a boundary assertion that transfer does not inspect operator presentation phase fields for eligibility.

**Validation Expectations:**

- `cargo nextest run --test workflow_runtime --test plan_execution --test runtime_module_boundaries --no-fail-fast`
- Full strict clippy and full nextest before task review.

## Task 7: Fix Runtime Remediation Inventory

**Spec Coverage:** REQ-008

**Goal:** Keep audit/remediation inventory aligned with current public replay coverage.

**Context:** `tests/fixtures/runtime-remediation/README.md` under-reports the FS-22 public replay that now lives in `tests/public_replay_churn.rs`.

**Constraints:**

- Update inventory only. Do not rewrite historical audit reports.
- Do not claim coverage that is not backed by a test.
- Keep the file useful for future auditors by distinguishing internal semantic coverage from shipped public replay coverage.

**Done when:**

- FS-22 lists `tests/public_replay_churn.rs` public replay coverage.
- The public replay summary includes FS-22.
- No stale statement says FS-22 is covered only by internal tests.

**Files:**

- `tests/fixtures/runtime-remediation/README.md`

**Implementation Steps:**

1. Update the FS-22 entry to name `tests/public_replay_churn.rs`.
2. Update the public replay summary range/list.
3. Search the README for remaining stale FS-22 wording.

**Validation Expectations:**

- `cargo nextest run --test public_replay_churn --no-fail-fast`
- Full strict clippy and full nextest before task review.

## Whole-Plan Validation And Review

After all tasks pass their own verify/review/remediate loops:

1. Ensure no cargo/nextest/clippy/test process is running.
2. Run:

```bash
node scripts/gen-skill-docs.mjs --check
node scripts/gen-agent-docs.mjs --check
node scripts/verify-source-archive.mjs
node --test tests/codex-runtime/*.test.mjs
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-targets --all-features --no-fail-fast
```

3. Dispatch a clean-context reviewer to review the full implementation against this entire plan.
4. Remediate any reviewer findings.
5. Repeat validation and review until no actionable issues remain.
6. Run the next deep audit loop, including the signal-to-noise auditor.
