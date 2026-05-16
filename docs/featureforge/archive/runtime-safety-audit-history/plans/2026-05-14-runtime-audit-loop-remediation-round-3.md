# Runtime Audit Loop Remediation Round 3

**Workflow State:** Draft
**Plan Revision:** 1
**Execution Mode:** implementation
**Source Spec:** `docs/featureforge/archive/runtime-safety-audit-history/reference/2026-05-13-twenty-ninth-audit-report.md`
**Source Spec Revision:** 1
**Last Reviewed By:** audit-loop

## Goal

Remove the remaining actionable audit findings from the current runtime-safety audit loop without adding another layer of low-signal workflow law. The concrete runtime goal is to keep authoritative lease/transition state as control-plane truth and demote unit-review receipt markdown to diagnostic projection evidence. The architecture goal is to remove a command-common import from `review_state.rs` by moving public recovery output binding into a neutral execution module. The signal-to-noise goal is to keep any prompt or boundary-test cleanup deletion-oriented and narrowly scoped.

## Architecture

Runtime truth remains append-only and structured:

- Worktree lease gating validates `active_worktree_lease_bindings`, lease JSON, active contract fingerprints, approved task packet fingerprints, approved unit contract fingerprints, reviewed checkpoint commits, reconcile result commits, reconcile proof fingerprints, reconcile mode, baseline provenance, ancestry, and cleanup state.
- Unit-review receipt markdown is a derived projection. Its absence, stale content, unsafe path, malformed body, or fingerprint mismatch must not select `repair-review-state`, `execution_reentry_required`, `reopen`, hidden helpers, or public cleanup targets when structured lease proof is current.
- Public recovery command surfaces are execution-level presentation DTOs, not command-common ownership. Command modules and `review_state.rs` should consume a neutral `execution::public_recovery` helper.
- Prompt and test cleanup should reduce repeated law or overbroad scanner assertions. Do not add new scanner layers unless they replace broader, noisier checks.

## Change Surface

- `src/execution/state/worktree_lease_truth.rs`
- `src/execution/state/unit_review_truth.rs`
- `src/execution/harness.rs`
- `src/execution/public_recovery.rs`
- `src/execution/commands/common/operator_outputs.rs`
- `src/execution/commands/common.rs`
- `src/execution/review_state.rs`
- `src/execution/mod.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/internal_plan_execution.rs`
- `tests/runtime_authority_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `scripts/gen-skill-docs.mjs`
- `tests/codex-runtime/*.test.mjs`
- generated `skills/**/SKILL.md` if a template/generator changes

## Preconditions

- Do not use FeatureForge skills or project skills.
- Do not run FeatureForge workflow/runtime commands as a workflow participant.
- Use public CLI only when tests already exercise public CLI behavior.
- Before every full nextest cycle, verify no `cargo nextest`, `cargo-nextest`, `nextest run`, `cargo test`, or `cargo clippy` process is active.
- If a full suite exceeds 4-5 minutes, run `cargo clean`, rerun, and remediate if repeatable. If it exceeds 10 minutes, stop immediately and apply the clean/rerun/remediation rule.

## Known Footguns / Constraints

- Do not weaken worktree lease proof validation. The control-plane replacement for receipt markdown is the existing structured binding plus lease JSON proof, not a pass-by-default path.
- Keep lease receipt fields deserializable for migration/diagnostic compatibility, but do not require their presence or content for normal routing.
- Do not make missing receipt projection warnings actionable blockers.
- Do not let public text suggest manual receipt repair, receipt reconstruction, hidden helpers, or low-level recorders.
- Do not move mandatory route law solely into companion docs. Top-level skill text must retain the minimal rule: query operator JSON, execute typed argv/template only, never execute display strings.
- Boundary tests should protect real import/authority boundaries, not arbitrary helper names.

## Requirement Coverage Matrix

| Requirement | Covered By |
| --- | --- |
| Worktree lease receipt markdown cannot gate routing when structured proof is current | Task 1 |
| Receipt/projection drift is diagnostic-only | Task 1 |
| Public lease cleanup remains task scoped and structured-proof based | Task 1 |
| `review_state.rs` does not import command-common presentation helpers | Task 2 |
| Public recovery output binding has a neutral execution owner | Task 2 |
| Boundary tests catch future command-common import inversions | Task 2 |
| Prompt route law remains compact and canonical-reference based | Task 3 |
| Scanner/boundary checks are reduced or narrowed when they do not prove shipped behavior | Task 4 |

## Task 1: Demote Worktree Lease Review Receipt Markdown To Diagnostics

**Spec Coverage:** Worktree lease receipt markdown cannot gate routing when structured proof is current; receipt/projection drift is diagnostic-only; public lease cleanup remains task scoped and structured-proof based.

**Goal:** Make `active_worktree_lease_bindings` and lease JSON proof the control-plane authority for terminal worktree leases. Receipt markdown may warn, but it must not block, repair-route, reopen, or require cleanup when structured proof is current.

**Context:**
- `enforce_worktree_lease_binding_truth` currently validates `review_receipt_fingerprint`, `review_receipt_artifact_path`, filesystem metadata, receipt filename, receipt body, and receipt fingerprint before allowing a terminal lease to release dependent work.
- The binding already carries structured fields needed for control-plane validation: `execution_context_key`, `approved_task_packet_fingerprint`, `approved_unit_contract_fingerprint`, `reviewed_checkpoint_commit_sha`, `reconcile_result_commit_sha`, `reconcile_result_proof_fingerprint`, and `reconcile_mode`.
- `validate_terminal_worktree_lease_proof` already validates commit-object proof and ancestry from lease JSON.

**Constraints:**
- Keep terminal lease proof strict.
- Rename new control-plane failure codes/messages away from `review_receipt_*` when the failure is about missing structured binding fields.
- Receipt path/fingerprint fields may remain in the schema as diagnostic compatibility fields.
- Existing diagnostic receipt validators can be reused with a scratch `GateState`, but their failures must become warning codes with a `_diagnostic_only` or projection-oriented suffix.

**Done when:**
- Missing, unreadable, malformed, or tampered unit-review receipt markdown does not block when structured lease binding and lease proof are current.
- Missing structured binding proof still blocks with binding/proof reason codes.
- Current task closure cleanup remains executable and task scoped.
- Public outputs do not tell agents to repair receipts manually.

**Files:**
- `src/execution/state/worktree_lease_truth.rs`
- `src/execution/state/unit_review_truth.rs`
- `tests/workflow_shell_smoke.rs`
- `tests/internal_plan_execution.rs`
- `tests/runtime_authority_contracts.rs`

**Implementation Steps:**
1. In the terminal lease branch of `enforce_worktree_lease_binding_truth`, remove hard requirements for `review_receipt_fingerprint` and `review_receipt_artifact_path`.
2. Validate required structured binding fields directly:
   - approved task packet belongs to active contract
   - approved unit contract equals `approved_unit_contract_fingerprint_for_review(...)`
   - execution context key matches the lease
   - reviewed checkpoint matches the lease
   - reconcile result commit/proof match `validate_terminal_worktree_lease_proof`
   - reconcile mode is `identity_preserving`
   - lease baseline head/worktree fingerprint match authoritative baseline
   - reconcile result is descended from checkpoint and current head contains the result
   - cleanup state is `cleaned`
3. Add a helper that optionally checks receipt projection drift only when receipt fields are present. Missing receipt path/fingerprint should warn, not fail. Tamper/mismatch should warn, not fail.
4. Update tests that expected `worktree_lease_review_receipt_missing` to block when structured fields are current.
5. Add or update a public replay/smoke regression proving missing/tampered receipt markdown cannot select `repair-review-state`, `execution_reentry_required`, `reopen`, or hidden helpers when structured proof is current.
6. Preserve internal tests proving missing structured binding fields still fail closed.

**Validation Expectations:**
- Focused tests for worktree lease public shell-smoke and internal plan-execution lease cases pass.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- Full `cargo nextest run --all-targets --all-features --no-fail-fast` passes before review.
- Clean-context review finds no remaining receipt control-plane leakage for this task.

## Task 2: Move Public Recovery Contract Binding Out Of Command-Common

**Spec Coverage:** `review_state.rs` does not import command-common presentation helpers; public recovery output binding has a neutral execution owner; boundary tests catch future command-common import inversions.

**Goal:** Make public recovery output binding a neutral execution helper shared by command modules and `review_state.rs`.

**Context:**
- `review_state.rs` imports `crate::execution::commands::common::public_recovery_contract_for_follow_up`.
- The function lives in `commands/common/operator_outputs.rs`, even though it only binds public route surfaces from `ExecutionRoutingState` and follow-up tokens.

**Constraints:**
- Do not duplicate public recovery binding logic.
- Preserve command module call sites.
- Preserve existing output JSON shape.
- Add a boundary test that allows command modules and the `mutate.rs` facade to import command common, but rejects non-command adapters importing command-common.

**Done when:**
- `review_state.rs` imports public recovery binding from a neutral execution module.
- Command modules continue to compile without local duplicate helpers.
- Boundary tests reject `execution::commands::common` imports from non-command execution/workflow modules.

**Files:**
- `src/execution/public_recovery.rs`
- `src/execution/mod.rs`
- `src/execution/commands/common/operator_outputs.rs`
- `src/execution/commands/common.rs`
- `src/execution/review_state.rs`
- `tests/runtime_module_boundaries.rs`

**Implementation Steps:**
1. Create `src/execution/public_recovery.rs`.
2. Move `PublicRecoveryContract`, `public_recovery_contract_for_follow_up`, and helper functions that do not require command internals into the neutral module.
3. Re-export the neutral helper from command common if that avoids noisy command-module churn.
4. Update `review_state.rs` to import the neutral module directly.
5. Add an import-boundary assertion preventing non-command modules from importing command-common.

**Validation Expectations:**
- Focused runtime module boundary tests pass.
- Focused repair-review-state tests pass.
- Strict clippy and full nextest pass before review.
- Clean-context review finds no boundary inversion.

## Task 3: Compact Repeated Route Law Without Removing Mandatory Top-Level Law

**Spec Coverage:** Prompt route law remains compact and canonical-reference based.

**Goal:** Reduce agent-facing route-law duplication while preserving the top-level executable rule in route-owning skills.

**Context:**
- Generated route-owning skills already delegate detailed binding to `references/operator-route-authority.md`, but the route law still appears in seven skills.
- The acceptable top-level rule is concise: use installed runtime, query operator JSON, execute typed argv/template only, never parse display strings, and stop on no typed surface.

**Constraints:**
- Do not remove mandatory top-level typed-route law from route-owning skills unless the skill no longer owns route execution.
- Do not repeat detailed argv[0] rebinding, template input-binding, or route-specific stop tables outside the canonical reference.
- Prompt budget must stay in enforce mode and under cap.

**Done when:**
- Route-owning generated skills contain one compact route-law paragraph and a canonical-reference pointer.
- Route-specific templates avoid repeating the same field-law prose outside the generated section.
- Node doc contract tests and prompt budget tests pass.

**Files:**
- `scripts/gen-skill-docs.mjs`
- `skills/**/SKILL.md.tmpl`
- generated `skills/**/SKILL.md`
- `tests/codex-runtime/*.test.mjs`

**Implementation Steps:**
1. Inventory repeated route-law prose outside generated `Installed Control Plane`.
2. Delete or collapse duplicate wording when it repeats the generated rule.
3. Keep tests focused on the canonical route-law reference and minimal top-level law.
4. Regenerate skill docs.

**Validation Expectations:**
- `node scripts/gen-skill-docs.mjs --check`
- `node --test tests/codex-runtime/*.test.mjs`
- Strict clippy/full nextest only if Rust tests or generated Rust-adjacent contracts change.

## Task 4: Narrow Scanner And Module-Shape Noise Without Weakening Boundary Protection

**Spec Coverage:** Scanner/boundary checks are reduced or narrowed when they do not prove shipped behavior.

**Goal:** Keep high-signal tests for public behavior and forbidden dependencies, while removing or narrowing brittle private-helper-name and generic phrase-scanner assertions.

**Context:**
- Audit flagged public-flow scanner sprawl and brittle module-shape checks.
- Some scanners protect real historical failures and must stay.

**Constraints:**
- Do not remove hidden command, hidden flag, display-command execution, public/private helper quarantine, or forbidden dependency checks.
- Prefer parser-backed checks and public CLI/golden behavior over line-oriented phrase scans.
- Any removed scanner must be replaced only if it was the sole guard for a concrete historical failure class.

**Done when:**
- At least one noisy scanner or module-shape assertion is deleted or narrowed with no loss of hidden-helper/display-command coverage.
- Boundary tests continue to enforce import direction and central authority ownership.

**Files:**
- `tests/support/public_flow_scan.rs`
- `tests/public_flow_scan_contracts.rs`
- `tests/runtime_module_boundaries.rs`
- `tests/runtime_behavior_golden.rs`

**Implementation Steps:**
1. Identify scanner rules with broad exception tables or private helper-name pinning.
2. Keep rules tied to shipped public behavior and delete/narrow rules that only validate scanner mechanics.
3. Update self-tests to assert the remaining contract, not the old scanner implementation.

**Validation Expectations:**
- Focused scanner/boundary tests pass.
- Node doc contract tests pass if prompt scanners change.
- Strict clippy/full nextest pass before review if Rust tests change.
