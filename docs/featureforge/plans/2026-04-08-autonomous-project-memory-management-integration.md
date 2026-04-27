# Autonomous Project Memory Management Integration Implementation Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 9
**Execution Mode:** none
**Source Spec:** `docs/featureforge/specs/2026-04-08-autonomous-project-memory-management-integration-design.md`
**Source Spec Revision:** 2
**Last Reviewed By:** plan-eng-review
**QA Requirement:** not-required

> **For Codex and GitHub Copilot workers:** REQUIRED: Use `featureforge workflow operator --plan <approved-plan-path>` as routing authority after engineering approval, and follow the runtime-selected execution owner skill; do not choose solely from isolated-agent availability. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement autonomous, non-blocking project-memory behavior across the current FeatureForge skill library while preserving workflow/operator runtime authority, event-log state authority, review independence, and the current plan-task contract.

**Architecture:** Deliver this as a contract-first documentation, prompt, fixture, and test slice. Shared contract tests establish the updated boundaries first, skill templates and generated docs encode the behavior, route and instruction tests protect public routing surfaces, project-memory content tests protect durable examples, and final validation proves the new wording under the current plan-contract and workflow-operator codebase. Runtime production code is out of scope unless a current contract-test fixture cannot validate the documented behavior without a narrow parser or fixture repair.

**Tech Stack:** Markdown skill templates (`skills/*/SKILL.md.tmpl`), generated skill docs (`skills/*/SKILL.md`), project-memory docs (`docs/project_notes/*.md`), subagent prompt markdown, Node contract tests (`tests/codex-runtime/*.mjs`), Rust instruction and plan-contract tests (`tests/*.rs`), FeatureForge runtime public commands (`workflow operator`, `plan execution status`, `advance-late-stage`), and repo-safety write-target contracts (`repo-file-write`, `release-doc-write`).

---

## Plan Contract

This plan implements spec revision 2 of the autonomous project-memory integration. If this plan diverges from the source spec, the source spec wins and this plan must be updated before engineering approval.

The plan is authored in the current task-contract format required by `review/plan-task-contract.md`. Every task uses `Goal`, `Context`, `Constraints`, `Done when`, and `Files` fields, followed by checkbox execution steps.

## Existing Capabilities / Built-ins to Reuse

- `node scripts/gen-skill-docs.mjs` regenerates checked-in `skills/*/SKILL.md` from template sources.
- `tests/codex-runtime/skill-doc-contracts.test.mjs` already enforces cross-skill wording invariants and memory-related non-gating language.
- `tests/codex-runtime/project-memory-content.test.mjs` already validates project-memory examples, references, reject vocabulary, and file-intent semantics.
- `tests/runtime_instruction_contracts.rs` validates repo instruction and runtime-facing documentation wording.
- `tests/runtime_instruction_execution_contracts.rs` validates execution-facing instruction and prompt behavior.
- `tests/using_featureforge_skill.rs` and `tests/fixtures/using-featureforge-project-memory-route-contract.sh` validate the explicit memory route.
- `tests/contracts_spec_plan.rs` validates the current plan-contract task shape, deterministic task obligations, and execution topology.
- `featureforge workflow operator --plan <approved-plan-path>` is the current normal routing authority after plan handoff.
- Repo-safety already distinguishes `repo-file-write` for project-memory files and `release-doc-write` for release-facing documentation.

## Known Footguns / Hard Rules

- Generated skill docs must be regenerated after every template edit.
- The shared test file `tests/codex-runtime/skill-doc-contracts.test.mjs` is a hotspot; this plan sequences every task serially to avoid contract drift.
- `AGENTS.md` must not remain in autonomous project-memory default scope.
- Project memory must not feed workflow/operator routing, event-log state, semantic workspace identity, mutator legality, release/readiness gates, QA gates, or finish gates.
- `document-release` must run the memory sweep before terminal final review and before release/readiness recording when memory files are mutated.
- Review skills must not gain autonomous project-memory consult steps.
- Implementer and reviewer subagents must not directly write `docs/project_notes/*`.
- Normal-path guidance must not recommend removed or hidden helper commands.
- This scope must not introduce a runtime memory stage, command, gate, projection, or router input.

## Cross-Task Invariants

- Add or update failing contract assertions before changing skill text in each task.
- Keep all project-memory writes routed through `featureforge:project-memory`.
- Keep autonomous project-memory invocation allowlisted to `featureforge:document-release` and the strict `featureforge:systematic-debugging` recurring-bug exception.
- Keep `docs/project_notes/*` supportive and below approved specs, approved plans, execution evidence, review artifacts, runtime-owned state, active repo instructions, stable repo docs, and code.
- Run `node scripts/gen-skill-docs.mjs` after template changes before assessing generated-doc contract results.
- Use current public routing wording in every modified skill: `featureforge workflow operator --plan <approved-plan-path>` for normal routing and `plan execution status` for diagnostics.
- Preserve repo-safety write targets for memory and release documentation writes.

## Change Surface

- Project-memory authority and examples:
  - `skills/project-memory/SKILL.md.tmpl`
  - `skills/project-memory/SKILL.md`
  - `skills/project-memory/authority-boundaries.md`
  - `skills/project-memory/examples.md`
  - `docs/project_notes/README.md`
- Read-side consult and execution-capture surfaces:
  - `skills/brainstorming/SKILL.md.tmpl`, `skills/brainstorming/SKILL.md`
  - `skills/writing-plans/SKILL.md.tmpl`, `skills/writing-plans/SKILL.md`
  - `skills/systematic-debugging/SKILL.md.tmpl`, `skills/systematic-debugging/SKILL.md`
  - `skills/executing-plans/SKILL.md.tmpl`, `skills/executing-plans/SKILL.md`
  - `skills/subagent-driven-development/SKILL.md.tmpl`, `skills/subagent-driven-development/SKILL.md`
  - `skills/subagent-driven-development/implementer-prompt.md`
  - `skills/subagent-driven-development/code-quality-reviewer-prompt.md`
  - `skills/subagent-driven-development/spec-reviewer-prompt.md`
- Late-stage ownership and routing surfaces:
  - `skills/document-release/SKILL.md.tmpl`, `skills/document-release/SKILL.md`
  - `skills/finishing-a-development-branch/SKILL.md.tmpl`, `skills/finishing-a-development-branch/SKILL.md`
  - `skills/using-featureforge/SKILL.md.tmpl`, `skills/using-featureforge/SKILL.md`
  - `AGENTS.md`
  - `README.md`
  - `docs/README.codex.md`
  - `docs/README.copilot.md`
- Contract tests and fixtures:
  - `tests/codex-runtime/skill-doc-contracts.test.mjs`
  - `tests/codex-runtime/project-memory-content.test.mjs`
  - `tests/runtime_instruction_contracts.rs`
  - `tests/runtime_instruction_execution_contracts.rs`
  - `tests/using_featureforge_skill.rs`
  - `tests/fixtures/using-featureforge-project-memory-route-contract.sh`
  - `tests/contracts_spec_plan.rs`

## Preconditions

- Source spec revision 2 is approved before this plan is submitted for engineering approval.
- The implementation branch contains the current event-log runtime authority and workflow/operator routing surfaces.
- `node` is available for skill-doc generation and codex-runtime contract tests.
- Rust test tooling and `cargo nextest` are available for targeted Rust contract suites.
- Repo-safety commands are available for any manual verification that edits repo-visible memory or release-facing documentation.

## Evidence Expectations

- Diff shows source template updates and regenerated checked-in skill docs.
- Diff shows project-memory boundaries narrowed to autonomous `docs/project_notes/*` scope.
- Diff shows `AGENTS.md` described only as explicit user or explicit repo-maintenance scope for memory-section updates.
- Diff shows document-release sweep ownership, sequencing, idempotency, atomicity, and structured outcome language.
- Diff shows execution and subagent prompt surfaces capturing durable findings in authoritative artifacts before nomination.
- Diff shows using-featureforge preserving explicit memory routing without adding memory to the default mandatory stack.
- Contract tests encode the allowlist, non-gating language, runtime-authority boundary, plan-format boundary, and review-independence boundary.
- Validation evidence records command outcomes for the candidate `HEAD`.

## Validation Strategy

Run targeted suites at the end of each task and the full matrix in Task 8.

## Path Coverage Matrix

```text
PATH | REQUIRED? | WHY | COVERAGE
consult-only memory reads in brainstorming/writing-plans/debugging | yes | new behavior and precedence wording must be regression-checked | automated
strict recurring-bug write-through threshold | yes | narrow exception boundary is easy to widen accidentally | automated
document-release zero-or-one sweep ownership, outcome reporting, and non-gating behavior | yes | central workflow contract change | automated
late-stage sequencing before advance-late-stage and final review | yes | ordering bug would change semantic workspace truth | automated
explicit project-memory route override in using-featureforge | yes | public routing contract must stay stable | automated
finish-stage no-autonomous-write boundary | yes | prevents late semantic drift | automated
review-independence no-auto-consult boundary | yes | approval and final review must stay memory-independent | automated
browser-visible QA flows | no | spec and plan scope are docs, prompts, fixtures, and contract tests only; no browser surface changes | not required
manual QA | no | change surface is enforced through deterministic Node and Rust contract suites rather than UI interaction | not required
```

Final required matrix:

1. `node scripts/gen-skill-docs.mjs`
2. `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`
3. `node --test tests/codex-runtime/project-memory-content.test.mjs`
4. `cargo nextest run --test runtime_instruction_contracts --test runtime_instruction_execution_contracts --test using_featureforge_skill`
5. `cargo nextest run --test contracts_spec_plan`
6. `cargo clippy --all-targets --all-features -- -D warnings`
7. `node scripts/gen-agent-docs.mjs --check` when generated agent or install docs are touched; otherwise record a skip note naming the untouched generated-doc surface

## Documentation Update Expectations

- Skill docs reflect the current workflow/operator command model.
- Project-memory docs separate autonomous memory sweep scope from explicit AGENTS maintenance scope.
- AGENTS guidance states project memory is supportive and not runtime authority.
- README and install-facing docs avoid old routing vocabulary for plan handoff.
- Project-memory examples show document-release sweep, no-delta skip, recurring-bug threshold, duplicate merge/no-op, and tracker-drift rejection.

## Rollback Plan

- Revert the full policy and contract-test slice if the updated model creates an irreconcilable review or runtime-authority regression.
- Do not keep new memory assertions while reverting the corresponding skill text.
- Do not keep new skill text while reverting the assertions that guard it.
- If a single task fails validation, revert that task and every later dependent task before reattempting the serial chain.

## Risks and Mitigations

- Risk: generated docs drift from templates.
  - Mitigation: regenerate after every template change and test generated docs directly.
- Risk: memory becomes a gate through wording drift.
  - Mitigation: negative assertions reject blocker, prerequisite, completion, and workflow-stage language.
- Risk: AGENTS is mutated autonomously through project-memory wording.
  - Mitigation: split autonomous write scope from explicit maintenance scope and update tests.
- Risk: final review sees a different semantic workspace than release/readiness.
  - Mitigation: document-release sequencing requires memory mutations before release/readiness recording and before terminal review.
- Risk: review skills consume supportive memory as authority.
  - Mitigation: review-independence assertions cover plan review and whole-diff code review skills.
- Risk: current runtime command vocabulary regresses to removed helper language.
  - Mitigation: runtime-instruction tests assert workflow/operator public-command wording.

## Execution Strategy

- Execute Task 1 serially. It updates the shared contract-test expectations before any skill text is changed.
- Execute Task 2 serially after Task 1. It closes the core project-memory authority and AGENTS scope boundary that downstream tasks reference.
- Execute Task 3 serially after Task 2. It updates planning and brainstorming consult wording in generated skill docs.
- Execute Task 4 serially after Task 3. It updates debugging, execution, and subagent capture-first surfaces that depend on the authority boundary.
- Execute Task 5 serially after Task 4. It updates document-release sweep ownership and late-stage sequencing after execution capture rules are stable.
- Execute Task 6 serially after Task 5. It aligns router, finish-stage, AGENTS, README, install docs, and public routing fixtures after sweep ownership is stable.
- Execute Task 7 serially after Task 6. It consolidates examples, route fixture coverage, review-independence assertions, and plan-contract guardrails after all wording surfaces are stable.
- Execute Task 8 last as the candidate `HEAD` validation and review-handoff task.

## Dependency Diagram

```text
Task 1 -> Task 2
Task 2 -> Task 3
Task 3 -> Task 4
Task 4 -> Task 5
Task 5 -> Task 6
Task 6 -> Task 7
Task 7 -> Task 8
```

## Late-Stage Flow Diagram

```text
execution/debugging/planning surfaces
            |
            v
authoritative artifacts capture durable findings first
            |
            v
featureforge:document-release
  |-- no durable delta ----------> emit no-op sweep outcome ----------> continue
  |
  |-- durable candidates exist --> featureforge:project-memory
                                   |-- rejected/partial-invalid --> report reject class + source
                                   |-- accepted ---------------> mutate docs/project_notes/* once
                                                         |
                                                         v
                               if memory files changed, do this before advance-late-stage
                                                         |
                                                         v
                          featureforge plan execution advance-late-stage --result ready|blocked
                                                         |
                                                         v
                                      terminal whole-diff final review on same workspace truth
```

## Requirement Coverage Matrix

- REQ-001 -> Task 2
- REQ-002 -> Task 2, Task 6
- REQ-003 -> Task 2, Task 6
- REQ-004 -> Task 3
- REQ-005 -> Task 3
- REQ-006 -> Task 3
- REQ-007 -> Task 4
- REQ-008 -> Task 7
- REQ-009 -> Task 4
- REQ-010 -> Task 4
- REQ-011 -> Task 1, Task 4
- REQ-012 -> Task 1, Task 4
- REQ-013 -> Task 5
- REQ-014 -> Task 5
- REQ-015 -> Task 1, Task 5, Task 7
- REQ-016 -> Task 5
- REQ-017 -> Task 6
- REQ-018 -> Task 1, Task 6, Task 7
- REQ-019 -> Task 2, Task 5, Task 7
- REQ-020 -> Task 1, Task 5, Task 7
- REQ-021 -> Task 1, Task 2, Task 6, Task 7
- REQ-022 -> Task 1, Task 5, Task 7
- REQ-023 -> Task 5
- REQ-024 -> Task 1, Task 5
- REQ-025 -> Task 5
- REQ-026 -> Task 8
- REQ-027 -> Task 1, Task 5, Task 7
- REQ-028 -> Task 1, Task 5, Task 6
- REQ-029 -> Task 5
- REQ-030 -> Task 1, Task 6
- REQ-031 -> Task 7, Task 8
- REQ-032 -> Task 7, Task 8
- REQ-033 -> Task 2, Task 5
- REQ-034 -> Task 2, Task 3, Task 4, Task 5, Task 6, Task 8
- REQ-035 -> Task 1, Task 6, Task 7
- REQ-036 -> Task 7, Task 8
- REQ-037 -> Task 4

## Task 1: Establish Runtime-Aware Contract Assertions

**Spec Coverage:** REQ-011, REQ-012, REQ-015, REQ-018, REQ-020, REQ-021, REQ-022, REQ-024, REQ-027, REQ-028, REQ-030, REQ-035
**Goal:** The shared contract tests describe the revised autonomous-memory model before skill wording changes land.

**Context:**

- Spec Coverage: REQ-011, REQ-012, REQ-015, REQ-018, REQ-020, REQ-021, REQ-022, REQ-024, REQ-027, REQ-028, REQ-030, REQ-035.
- Current `tests/codex-runtime/skill-doc-contracts.test.mjs` still expects the older optional document-release follow-up and AGENTS-inclusive default write set.
- Runtime authority now flows through workflow/operator and the event-log reducer rather than markdown memory or hidden helper choreography.

**Constraints:**

- Contract assertions must target stable behavior and durable wording, not incidental prose layout.
- Tests must reject memory as a blocker, prerequisite, workflow stage, runtime route input, or projection authority.
- Tests must distinguish autonomous `docs/project_notes/*` scope from explicit AGENTS maintenance scope.
- Tests must use current public command vocabulary and must not bless removed normal-path helper recommendations.

**Done when:**

- `tests/codex-runtime/skill-doc-contracts.test.mjs` contains assertions for document-release zero-or-one sweep ownership, non-gating sweep wording, autonomous-owner allowlist, explicit-only AGENTS scope, workflow/operator routing language, and event-log authority isolation.
- `tests/codex-runtime/project-memory-content.test.mjs` contains assertions for the revised examples that will be added later in the plan.
- Rust instruction and using-featureforge tests contain expected strings for explicit memory routing and public workflow/operator vocabulary.
- Targeted test runs fail only on missing implementation wording, stale expected strings, or absent revised examples.

**Files:**

- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/codex-runtime/project-memory-content.test.mjs`
- Modify: `tests/runtime_instruction_contracts.rs`
- Modify: `tests/runtime_instruction_execution_contracts.rs`
- Modify: `tests/using_featureforge_skill.rs`
- Modify: `tests/fixtures/using-featureforge-project-memory-route-contract.sh`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/project-memory-content.test.mjs`
- Test: `tests/runtime_instruction_contracts.rs`
- Test: `tests/runtime_instruction_execution_contracts.rs`
- Test: `tests/using_featureforge_skill.rs`

- [ ] **Step 1: Add failing Node assertions for document-release sweep ownership, no-gate language, owner allowlist, explicit-only AGENTS scope, structured sweep outcomes, and runtime-authority isolation.**
- [ ] **Step 2: Add failing Node assertions for project-memory examples covering document-release sweep, no-delta skip, recurring-bug threshold, duplicate merge or no-op, atomic multi-file failure, and tracker-drift rejection.**
- [ ] **Step 3: Add failing Rust instruction assertions that current docs use workflow/operator routing and do not describe project memory as route authority, event authority, semantic workspace authority, release gate authority, or finish gate authority.**
- [ ] **Step 4: Add failing using-featureforge route assertions that explicit project-memory intent still overrides helper-derived workflow routes while project memory remains absent from the default mandatory stack.**
- [ ] **Step 5: Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and record the expected red assertions by test name.**
- [ ] **Step 6: Run `node --test tests/codex-runtime/project-memory-content.test.mjs` and record the expected red assertions by test name.**
- [ ] **Step 7: Run `cargo nextest run --test runtime_instruction_contracts --test runtime_instruction_execution_contracts --test using_featureforge_skill` and record the expected red assertions by test name.**
- [ ] **Step 8: Confirm no production runtime code was changed in this task.**

## Task 2: Narrow Project-Memory Authority and AGENTS Scope

**Spec Coverage:** REQ-001, REQ-002, REQ-003, REQ-019, REQ-021, REQ-033, REQ-034
**Goal:** Project-memory boundary docs distinguish autonomous memory writes from explicit AGENTS instruction maintenance.

**Context:**

- Spec Coverage: REQ-001, REQ-002, REQ-003, REQ-019, REQ-021, REQ-033, REQ-034.
- Current project-memory docs allow the narrow AGENTS project-memory section in the default write set.
- The revised spec keeps `featureforge:project-memory` as sole writer for `docs/project_notes/*` while making AGENTS explicit-only.

**Constraints:**

- Autonomous workflow-owned memory writes must target only `docs/project_notes/*`.
- Explicit AGENTS updates must require explicit user intent or explicit repo-maintenance scope.
- Reject vocabulary must remain centralized in project-memory boundary docs.
- Repo-safety `repo-file-write` checks must remain mandatory before memory file edits.
- Template edits must be followed by generated-doc regeneration.

**Done when:**

- `skills/project-memory/SKILL.md.tmpl` and generated `SKILL.md` state that autonomous default write scope is limited to `docs/project_notes/*`.
- `skills/project-memory/authority-boundaries.md` separates autonomous memory file scope from explicit AGENTS maintenance scope.
- `skills/project-memory/examples.md` removes examples that imply routine autonomous AGENTS mutation.
- `docs/project_notes/README.md` matches the revised authority order and no-secrets rule.
- Targeted Node contract tests pass for project-memory authority, reject vocabulary, repo-safety, and explicit-only AGENTS assertions.

**Files:**

- Modify: `skills/project-memory/SKILL.md.tmpl`
- Modify: `skills/project-memory/SKILL.md`
- Modify: `skills/project-memory/authority-boundaries.md`
- Modify: `skills/project-memory/examples.md`
- Modify: `docs/project_notes/README.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/codex-runtime/project-memory-content.test.mjs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/project-memory-content.test.mjs`

- [ ] **Step 1: Update project-memory template hard-boundary text so autonomous workflow writes name only `docs/project_notes/*` and explicit AGENTS maintenance is described as user-directed scope.**
- [ ] **Step 2: Update `authority-boundaries.md` default write set and authority order to match the revised runtime-aware spec.**
- [ ] **Step 3: Update project-memory examples so accepted examples use `docs/project_notes/bugs.md`, `decisions.md`, `key_facts.md`, or `issues.md` and rejected examples cover `InstructionAuthorityDrift`.**
- [ ] **Step 4: Update `docs/project_notes/README.md` so it states project memory is supportive below approved artifacts, runtime-owned state, active instructions, stable docs, and code.**
- [ ] **Step 5: Run `node scripts/gen-skill-docs.mjs` and inspect that `skills/project-memory/SKILL.md` matches the template changes.**
- [ ] **Step 6: Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm project-memory authority assertions pass.**
- [ ] **Step 7: Run `node --test tests/codex-runtime/project-memory-content.test.mjs` and confirm project-memory corpus assertions pass for the Task 2 examples.**
- [ ] **Step 8: Inspect the diff for accidental runtime production changes and remove any out-of-scope edits.**

## Task 3: Add Consult-Only Rules to Brainstorming and Writing Plans

**Spec Coverage:** REQ-004, REQ-005, REQ-006, REQ-034
**Goal:** Brainstorming and planning consult project memory only as optional, subordinate context.

**Context:**

- Spec Coverage: REQ-004, REQ-005, REQ-006, REQ-034.
- `featureforge:writing-plans` already has a memory consult section, but it lacks the revised conflict and document-release ownership language.
- `featureforge:brainstorming` lacks the requested optional consult section.

**Constraints:**

- Memory consults must be optional and non-blocking.
- Approved specs, approved plans, direct repo evidence, active instructions, stable repo docs, and current code must outrank memory.
- Planning truth must be written into the plan rather than left in memory.
- Later memory updates must point to document-release ownership rather than planning-time write-through.
- Template edits must be followed by generated-doc regeneration.

**Done when:**

- `skills/brainstorming/SKILL.md.tmpl` and generated `SKILL.md` include optional consult triggers for `docs/project_notes/decisions.md` and `docs/project_notes/key_facts.md`.
- `skills/writing-plans/SKILL.md.tmpl` and generated `SKILL.md` include non-blocking consult language, approved-artifact precedence, and document-release follow-up ownership.
- Contract tests reject wording that makes memory consult a planning prerequisite or gate.
- Targeted Node contract tests pass for brainstorming and writing-plans memory consult assertions.

**Files:**

- Modify: `skills/brainstorming/SKILL.md.tmpl`
- Modify: `skills/brainstorming/SKILL.md`
- Modify: `skills/writing-plans/SKILL.md.tmpl`
- Modify: `skills/writing-plans/SKILL.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Add brainstorming template text that names cross-cutting changes, architecture-shaping work, prior decisions, existing patterns, and expensive-to-rediscover facts as optional consult triggers.**
- [ ] **Step 2: Add brainstorming template text that names `docs/project_notes/decisions.md` and `docs/project_notes/key_facts.md` as consult files.**
- [ ] **Step 3: Update writing-plans template text so consult output is copied into the plan when it matters and conflicts are resolved in favor of approved artifacts and direct repo evidence.**
- [ ] **Step 4: Update writing-plans template text so later memory updates are normally owned by document-release rather than by plan drafting.**
- [ ] **Step 5: Add or adjust contract assertions for optional consult files, non-blocking wording, precedence wording, and absence of planning-time write-through.**
- [ ] **Step 6: Run `node scripts/gen-skill-docs.mjs` and inspect generated brainstorming and writing-plans docs.**
- [ ] **Step 7: Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm consult assertions pass.**
- [ ] **Step 8: Inspect the generated-doc diff for duplicated consult sections or stale old wording.**

## Task 4: Tighten Debugging Write-Through and Execution Capture-First Boundaries

**Spec Coverage:** REQ-007, REQ-009, REQ-010, REQ-011, REQ-012, REQ-034, REQ-037
**Goal:** Debugging, execution, and subagent surfaces capture durable findings through authoritative artifacts before any memory nomination.

**Context:**

- Spec Coverage: REQ-007, REQ-009, REQ-010, REQ-011, REQ-012, REQ-034, REQ-037.
- `featureforge:systematic-debugging` currently permits durable recurring bug memory updates but does not require every revised threshold condition.
- Execution and subagent docs already follow workflow/operator routing but lack explicit memory candidate capture rules.

**Constraints:**

- Immediate write-through is limited to `featureforge:systematic-debugging` and `docs/project_notes/bugs.md`.
- Debug write-through must require recurring or high rediscovery cost, known root cause, validated fix, concrete prevention note, and authoritative backlink.
- Execution and subagent loops must not directly edit `docs/project_notes/*`.
- Durable findings must be recorded first in packets, execution evidence, review artifacts, release summaries, or other authoritative outputs from the active step.
- Template edits must be followed by generated-doc regeneration.

**Done when:**

- `skills/systematic-debugging/SKILL.md.tmpl` and generated `SKILL.md` include consult guidance, evidence-first conflict handling, strict write-through threshold, and defer-to-document-release wording.
- `skills/executing-plans/SKILL.md.tmpl` and generated `SKILL.md` include capture-first and no-direct-memory-write wording.
- `skills/subagent-driven-development/SKILL.md.tmpl` and generated `SKILL.md` include capture-first and no-direct-memory-write wording.
- Subagent implementer and reviewer prompts instruct durable memory candidates to flow through authoritative artifacts or handoffs rather than direct memory edits.
- Targeted Node and Rust execution-instruction tests pass for debugging, execution, and subagent boundaries.

**Files:**

- Modify: `skills/systematic-debugging/SKILL.md.tmpl`
- Modify: `skills/systematic-debugging/SKILL.md`
- Modify: `skills/executing-plans/SKILL.md.tmpl`
- Modify: `skills/executing-plans/SKILL.md`
- Modify: `skills/subagent-driven-development/SKILL.md.tmpl`
- Modify: `skills/subagent-driven-development/SKILL.md`
- Modify: `skills/subagent-driven-development/implementer-prompt.md`
- Modify: `skills/subagent-driven-development/code-quality-reviewer-prompt.md`
- Modify: `skills/subagent-driven-development/spec-reviewer-prompt.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/runtime_instruction_execution_contracts.rs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/runtime_instruction_execution_contracts.rs`

- [ ] **Step 1: Update systematic-debugging template text so `docs/project_notes/bugs.md` is consult-only unless every write-through threshold condition is satisfied.**
- [ ] **Step 2: Add systematic-debugging defer wording for failures lacking known root cause, validated fix, prevention note, or authoritative backlink.**
- [ ] **Step 3: Update executing-plans template text so durable candidates are captured in execution evidence or handoff outputs before document-release sweep.**
- [ ] **Step 4: Update subagent-driven-development template text so implementer and reviewer loops do not edit project memory directly.**
- [ ] **Step 5: Update implementer, code-quality reviewer, and spec-reviewer prompts to route durable project-memory candidates through packets, evidence, review artifacts, or release summaries.**
- [ ] **Step 6: Add or adjust contract assertions for strict debugging threshold, no direct execution memory writes, and subagent prompt memory boundaries.**
- [ ] **Step 7: Run `node scripts/gen-skill-docs.mjs` and inspect generated debugging, executing-plans, and subagent-driven-development docs.**
- [ ] **Step 8: Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm debugging and execution memory assertions pass.**
- [ ] **Step 9: Run `cargo nextest run --test runtime_instruction_execution_contracts` and confirm prompt and execution-instruction assertions pass.**
- [ ] **Step 10: Inspect the diff for direct-memory-write instructions inside implementer or reviewer prompt bodies and remove them.**

## Task 5: Implement Document-Release Sweep Ownership and Late-Stage Sequencing

**Spec Coverage:** REQ-013, REQ-014, REQ-015, REQ-016, REQ-019, REQ-022, REQ-023, REQ-024, REQ-025, REQ-027, REQ-028, REQ-029, REQ-033, REQ-034
**Goal:** Document-release owns the default non-blocking memory sweep with deterministic reporting before terminal final review.

**Context:**

- Spec Coverage: REQ-013, REQ-014, REQ-015, REQ-016, REQ-019, REQ-022, REQ-023, REQ-024, REQ-025, REQ-027, REQ-028, REQ-029, REQ-033, REQ-034.
- Current document-release docs describe only an optional memory follow-up.
- Current runtime records release/readiness through `advance-late-stage --result ready|blocked`, and memory writes must occur before that recording when the sweep mutates tracked files.

**Constraints:**

- Sweep ownership must remain zero-or-one per document-release pass.
- Sweep candidates must come from authoritative or stable branch artifacts, not unsourced chat narrative or transient scratch notes.
- Multi-file memory writes must be atomic at the invocation scope.
- Duplicate candidates must become deterministic no-op or deterministic merge results.
- Rejected candidates must report reject class, target memory file, and source-artifact pointer.
- Sweep outcome must never block terminal review, QA routing, finish, or branch completion.
- Release-facing doc writes keep `release-doc-write`; memory file writes keep `repo-file-write`.

**Done when:**

- `skills/document-release/SKILL.md.tmpl` and generated `SKILL.md` include the default zero-or-one sweep algorithm and non-blocking continuation language.
- `skills/document-release/SKILL.md.tmpl` and generated `SKILL.md` include source-filter rules and file-intent mapping.
- `skills/document-release/SKILL.md.tmpl` and generated `SKILL.md` include deterministic dedupe or merge behavior and atomic invocation rules.
- `skills/document-release/SKILL.md.tmpl` and generated `SKILL.md` include rejection reporting and no-op reporting.
- Document-release sequencing states memory mutations occur before `advance-late-stage --result ready|blocked` and before terminal final review.
- Project-memory boundary examples include document-release sweep invocation, no-delta skip, rejection reporting, and atomic multi-file failure behavior.
- Contract tests pass for document-release ownership, source filtering, structured outcomes, idempotency, atomicity, allowlist ownership, and runtime-authority isolation.

**Files:**

- Modify: `skills/document-release/SKILL.md.tmpl`
- Modify: `skills/document-release/SKILL.md`
- Modify: `skills/project-memory/authority-boundaries.md`
- Modify: `skills/project-memory/examples.md`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/codex-runtime/project-memory-content.test.mjs`
- Modify: `tests/runtime_instruction_contracts.rs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/project-memory-content.test.mjs`
- Test: `tests/runtime_instruction_contracts.rs`

- [ ] **Step 1: Replace the optional document-release memory follow-up section with a default zero-or-one sweep section that remains explicitly non-blocking.**
- [ ] **Step 2: Add sweep source-filter text naming approved artifacts, branch diff, execution evidence, review artifacts, release summary, and stable repo docs as valid sources.**
- [ ] **Step 3: Add rejection text for unsourced chat narrative, transient scratch notes, secrets, authority-blurring claims, tracker-like content, instruction-like content, one-off noise, and oversized duplication.**
- [ ] **Step 4: Add file-intent mapping text for `bugs.md`, `decisions.md`, `key_facts.md`, and `issues.md`.**
- [ ] **Step 5: Add deterministic collision handling text for equivalent existing entries, additive provenance, verification metadata, and materially conflicting claims.**
- [ ] **Step 6: Add atomic invocation text for multi-file project-memory writes.**
- [ ] **Step 7: Add structured sweep outcome text covering candidates considered, accepted count, rejected count, skip reason, no-op result, invocation result, failure class, reject class, target file, and source-artifact pointer.**
- [ ] **Step 8: Add late-stage sequencing text stating tracked memory mutations occur before `advance-late-stage --result ready|blocked` and before terminal whole-diff final review.**
- [ ] **Step 9: Update project-memory boundary examples for document-release sweep, no-delta skip, duplicate no-op, rejection report, and atomic multi-file structural failure.**
- [ ] **Step 10: Add or adjust contract assertions for every document-release sweep behavior introduced in this task.**
- [ ] **Step 11: Run `node scripts/gen-skill-docs.mjs` and inspect generated document-release docs.**
- [ ] **Step 12: Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm document-release assertions pass.**
- [ ] **Step 13: Run `node --test tests/codex-runtime/project-memory-content.test.mjs` and confirm revised example assertions pass.**
- [ ] **Step 14: Run `cargo nextest run --test runtime_instruction_contracts` and confirm late-stage sequencing and runtime-authority assertions pass.**
- [ ] **Step 15: Inspect the diff for any wording that turns sweep outcomes into a blocker or gate and remove it.**

## Task 6: Align Router, Finish Stage, AGENTS, and Public Docs

**Spec Coverage:** REQ-002, REQ-003, REQ-017, REQ-018, REQ-021, REQ-028, REQ-030, REQ-034, REQ-035
**Goal:** Router, finish, instruction, and public-doc surfaces align with explicit memory routing and workflow/operator authority.

**Context:**

- Spec Coverage: REQ-002, REQ-003, REQ-017, REQ-018, REQ-021, REQ-028, REQ-030, REQ-034, REQ-035.
- `featureforge:using-featureforge` already contains explicit memory routing and a default-stack exclusion.
- `featureforge:finishing-a-development-branch` currently does not own memory writes, but it needs explicit boundary wording.
- AGENTS and public docs must not imply memory route authority or old execution-handoff vocabulary.

**Constraints:**

- Explicit memory requests still route to `featureforge:project-memory`.
- Vague mentions of notes, docs, or memory must not trigger the project-memory route.
- The router must not own autonomous document-release or debugging memory follow-up.
- Finish-stage docs must not trigger new autonomous memory writes.
- Public docs must use workflow/operator for normal post-approval routing.
- Template edits must be followed by generated-doc regeneration.

**Done when:**

- `skills/using-featureforge/SKILL.md.tmpl` and generated `SKILL.md` state that autonomous memory follow-up is owned by the active workflow skill, not by the entry router.
- `skills/finishing-a-development-branch/SKILL.md.tmpl` and generated `SKILL.md` state that finish performs no autonomous project-memory writes.
- `AGENTS.md` project-memory guidance states memory is supportive, explicit updates use `featureforge:project-memory`, and memory is not runtime or route authority.
- README and install-facing docs use current workflow/operator post-approval routing language and avoid old normal-path helper recommendations.
- Using-featureforge route tests and runtime-instruction tests pass for explicit memory routing and public command wording.

**Files:**

- Modify: `skills/using-featureforge/SKILL.md.tmpl`
- Modify: `skills/using-featureforge/SKILL.md`
- Modify: `skills/finishing-a-development-branch/SKILL.md.tmpl`
- Modify: `skills/finishing-a-development-branch/SKILL.md`
- Modify: `AGENTS.md`
- Modify: `README.md`
- Modify: `docs/README.codex.md`
- Modify: `docs/README.copilot.md`
- Modify: `tests/using_featureforge_skill.rs`
- Modify: `tests/fixtures/using-featureforge-project-memory-route-contract.sh`
- Modify: `tests/runtime_instruction_contracts.rs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/using_featureforge_skill.rs`
- Test: `tests/runtime_instruction_contracts.rs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`

- [ ] **Step 1: Update using-featureforge template text so explicit memory intent remains a short-circuit route and autonomous follow-up is delegated to document-release or the debugging exception.**
- [ ] **Step 2: Update using-featureforge template text so project memory remains excluded from the default mandatory workflow stack.**
- [ ] **Step 3: Update finishing-a-development-branch template text so finish assumes memory housekeeping already occurred and performs no autonomous memory writes.**
- [ ] **Step 4: Update AGENTS project-memory guidance for supportive memory, explicit project-memory updates, no secrets, and no runtime route authority.**
- [ ] **Step 5: Update README and install-facing docs to retain workflow/operator routing language and remove stale normal-path handoff wording.**
- [ ] **Step 6: Update using-featureforge route fixture expectations for explicit memory override and default-stack exclusion.**
- [ ] **Step 7: Add or adjust runtime-instruction assertions for AGENTS, README, install docs, finish-stage memory boundary, and public command vocabulary.**
- [ ] **Step 8: Run `node scripts/gen-skill-docs.mjs` and inspect generated using-featureforge and finishing docs.**
- [ ] **Step 9: Run `cargo nextest run --test using_featureforge_skill --test runtime_instruction_contracts` and confirm route and instruction assertions pass.**
- [ ] **Step 10: Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm router and finish-stage assertions pass.**
- [ ] **Step 11: Run `node scripts/gen-agent-docs.mjs --check` when generated install or agent docs are in scope, or record that no generated install or agent doc was touched.**
- [ ] **Step 12: Inspect the diff for old plan handoff language and replace stale normal-path command references.**

## Task 7: Consolidate Examples, Review Independence, and Plan-Contract Guardrails

**Spec Coverage:** REQ-008, REQ-015, REQ-018, REQ-019, REQ-020, REQ-021, REQ-022, REQ-027, REQ-031, REQ-032, REQ-035, REQ-036
**Goal:** Final contract coverage proves memory boundaries, review independence, examples, and plan format are review-ready.

**Context:**

- Spec Coverage: REQ-008, REQ-015, REQ-018, REQ-019, REQ-020, REQ-021, REQ-022, REQ-027, REQ-031, REQ-032, REQ-035, REQ-036.
- Review skills must remain memory-independent by default after all consult and sweep wording has landed.
- The revised implementation plan must satisfy the current task-contract shape and execution topology checks before engineering review.

**Constraints:**

- Review skills must not gain autonomous project-memory consult steps.
- Examples must map each durable memory type to the intended file and reject class.
- Route fixtures must preserve explicit memory override without default-stack insertion.
- Plan-contract guardrails must reject active implementation plans that use retired task-body field names.
- The updated plan must remain serial because shared contract-test and generated-doc hotspots are touched across tasks.

**Done when:**

- Contract tests assert that plan-ceo-review, plan-fidelity-review, plan-eng-review, and requesting-code-review do not include autonomous project-memory consult steps.
- Project-memory content tests pass for accepted examples, rejected examples, shared reject vocabulary, and file-intent mapping.
- Using-featureforge fixture coverage passes for explicit memory intent and non-memory requests.
- Plan-contract tests pass for current task fields, deterministic completion obligations, execution strategy, and dependency diagram topology.
- No task body in the implementation plan contains retired task-field headings.

**Files:**

- Modify: `skills/project-memory/examples.md`
- Modify: `tests/codex-runtime/project-memory-content.test.mjs`
- Modify: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Modify: `tests/using_featureforge_skill.rs`
- Modify: `tests/fixtures/using-featureforge-project-memory-route-contract.sh`
- Modify: `tests/contracts_spec_plan.rs`
- Test: `docs/featureforge/plans/2026-04-08-autonomous-project-memory-management-integration.md`
- Test: `tests/codex-runtime/project-memory-content.test.mjs`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/using_featureforge_skill.rs`
- Test: `tests/contracts_spec_plan.rs`

- [ ] **Step 1: Add or refine review-independence assertions for plan-ceo-review, plan-fidelity-review, plan-eng-review, and requesting-code-review.**
- [ ] **Step 2: Add or refine project-memory example assertions for file-intent mapping, reject vocabulary, source backlinks, duplicate no-op behavior, and no-delta sweep reporting.**
- [ ] **Step 3: Add or refine using-featureforge fixture cases for explicit setup, explicit durable bug logging, explicit decision recording, read-only project-notes question, and vague docs mention.**
- [ ] **Step 4: Add or refine plan-contract tests that enforce current task fields and reject retired task-body field headings in active implementation plans.**
- [ ] **Step 5: Patch examples and fixtures until every revised assertion has concrete source text to validate.**
- [ ] **Step 6: Run `node --test tests/codex-runtime/project-memory-content.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs` and confirm consolidated Node assertions pass.**
- [ ] **Step 7: Run `cargo nextest run --test using_featureforge_skill --test contracts_spec_plan` and confirm route fixture and plan-contract assertions pass.**
- [ ] **Step 8: Inspect all review skill docs and confirm no autonomous project-memory consult language was introduced.**
- [ ] **Step 9: Inspect the active plan text and confirm task bodies contain `Goal`, `Context`, `Constraints`, `Done when`, and `Files` in the current order.**

## Task 8: Run Final Validation Matrix and Prepare Review Handoff

**Spec Coverage:** REQ-026, REQ-031, REQ-032, REQ-034, REQ-036
**Goal:** The candidate `HEAD` has complete post-implementation validation evidence and a deterministic handoff for terminal whole-diff review and downstream finish readiness.

**Context:**

- Spec Coverage: REQ-026, REQ-031, REQ-032, REQ-034, REQ-036.
- The revised spec requires a fixed validation matrix on the candidate `HEAD` for implementation-completion claims and downstream final-review or finish-readiness evidence.
- The current codebase requires plan-contract compliance and workflow/operator vocabulary for review readiness.

**Constraints:**

- Completion must not be claimed without command evidence tied to the same candidate `HEAD`.
- Generated skill docs must be current before final contract tests run.
- Rust clippy must run because this plan edits Rust tests.
- Generated agent or install doc check must be recorded when public docs are touched.
- The handoff must name failures, skipped checks, and candidate `HEAD` identity without paraphrasing them into success claims.

**Done when:**

- `node scripts/gen-skill-docs.mjs` runs with no generated-doc drift after completion.
- `node --test tests/codex-runtime/skill-doc-contracts.test.mjs` passes on candidate `HEAD`.
- `node --test tests/codex-runtime/project-memory-content.test.mjs` passes on candidate `HEAD`.
- `cargo nextest run --test runtime_instruction_contracts --test runtime_instruction_execution_contracts --test using_featureforge_skill` passes on candidate `HEAD`.
- `cargo nextest run --test contracts_spec_plan` passes on candidate `HEAD`.
- `cargo clippy --all-targets --all-features -- -D warnings` passes on candidate `HEAD`.
- `node scripts/gen-agent-docs.mjs --check` passes or the handoff records why no generated agent or install doc check applied.
- Review handoff includes changed files, requirement coverage, validation command outcomes, candidate `HEAD`, and any remaining non-blocking observations for terminal whole-diff review and downstream finish-readiness checks.

**Files:**

- Test: `skills/*/SKILL.md`
- Test: `tests/codex-runtime/skill-doc-contracts.test.mjs`
- Test: `tests/codex-runtime/project-memory-content.test.mjs`
- Test: `tests/runtime_instruction_contracts.rs`
- Test: `tests/runtime_instruction_execution_contracts.rs`
- Test: `tests/using_featureforge_skill.rs`
- Test: `tests/contracts_spec_plan.rs`

- [ ] **Step 1: Run `node scripts/gen-skill-docs.mjs` and confirm generated skill docs are current.**
- [ ] **Step 2: Run `node --test tests/codex-runtime/skill-doc-contracts.test.mjs`.**
- [ ] **Step 3: Run `node --test tests/codex-runtime/project-memory-content.test.mjs`.**
- [ ] **Step 4: Run `cargo nextest run --test runtime_instruction_contracts --test runtime_instruction_execution_contracts --test using_featureforge_skill`.**
- [ ] **Step 5: Run `cargo nextest run --test contracts_spec_plan`.**
- [ ] **Step 6: Run `cargo clippy --all-targets --all-features -- -D warnings`.**
- [ ] **Step 7: Run `node scripts/gen-agent-docs.mjs --check` when generated agent or install docs are touched, or record the explicit skip note.**
- [ ] **Step 8: Capture `git rev-parse HEAD`, `git status --short`, and the validation command outputs for review handoff.**
- [ ] **Step 9: Confirm there are no uncommitted generated-doc drifts after all validation commands.**
- [ ] **Step 10: Prepare a final-review and finish-readiness handoff that lists requirement coverage, execution topology, task-contract compliance, changed files, candidate `HEAD`, per-command validation outcomes, and residual risks.**
