# FeatureForge Remediation Program

**Workflow State:** Draft
**Spec Revision:** 1
**Last Reviewed By:** brainstorming

## Problem Statement

FeatureForge already has strong workflow concepts, but several runtime contracts still drift across Rust code, generated skill preambles, checked-in markdown skills, upgrade docs, and contributor documentation. The highest-risk defects are not isolated bugs. They are contract splits:

- install and runtime-root detection are implemented in more than one place
- session-entry and spawned-subagent behavior are partly runtime-owned and partly prose-owned
- canonical path policy and active generated/public surfaces do not fully agree
- contributor validation instructions, generated-doc freshness checks, and example layout are not yet telling one coherent story
- hotspot modules still bundle too many responsibilities, which makes later behavior work harder to review safely

This program turns the remediation findings `FF-01` through `FF-11` into one sequenced delivery plan that keeps the entire scope in view while preventing later cleanup work from obscuring earlier contract fixes.

## Desired Outcome

At the end of this remediation program, FeatureForge should have one runtime-owned story for install discovery, session-entry policy, and canonical artifact routing. Generated skills, upgrade flows, checked-in docs, and tests should consume that story instead of restating or partially re-implementing it. Later refactors should happen only after the behavior they depend on is pinned by regression coverage.

## Scope

In scope:

- all remediation findings `FF-01` through `FF-11`
- one umbrella spec and one full implementation plan that covers the entire remediation set
- phased delivery that preserves all scope while sequencing behavior stabilization before cleanup and refactors

Out of scope:

- reintroducing removed install command surfaces
- preserving legacy-root compatibility as a product feature
- broad structural refactors before behavior regressions are pinned
- mixing new product-policy decisions into later implementation phases

## Requirement Index

- [REQ-001][behavior] Runtime root resolution must become a single runtime-owned contract used by `update-check`, generated skill preambles, and upgrade flows.
- [REQ-002][behavior] Spawned-subagent session-entry behavior must become runtime-owned, deterministic, and testable.
- [REQ-003][behavior] Legacy roots `~/.codex/featureforge` and `~/.copilot/featureforge` must be removed from active discovery and active generated/public surfaces.
- [REQ-004][behavior] Contributor-facing docs, validation commands, generated-doc freshness checks, and starter/example guidance must converge on one canonical FeatureForge story.
- [REQ-005][behavior] Shared helper logic and hotspot modules must be refactored only after earlier contract behavior is pinned by tests.
- [REQ-006][behavior] CLI inputs that encode bounded choices must move to typed parse-boundary validation, and bare `featureforge` invocation must show help instead of silently succeeding.
- [DEC-001][decision] The remediation program remains one umbrella spec and one implementation plan, but delivery is phase-gated rather than executed as one mixed-risk blob.
- [DEC-002][decision] Legacy roots are unsupported and are removed outright from active behavior and active generated/public surfaces; no migration-only runtime path remains in scope.
- [DEC-003][decision] Later phases may not silently repair behavior that belongs to an earlier phase.
- [VERIFY-001][verification] Each behavior-changing phase must add or update regression coverage before the behavior change lands.
- [VERIFY-002][verification] Each phase must define targeted acceptance criteria, explicit exclusions, and release-facing verification commands.
- [NONGOAL-001][non-goal] Do not reintroduce removed install command surfaces or compatibility shims just to soften cutover debt.
- [NONGOAL-002][non-goal] Do not perform broad refactors in Phases 1 through 3 beyond the minimum extraction needed to establish runtime-owned contracts.

## Repo Reality Check

The current repository seams that drive this design are concrete:

- [`src/update_check/mod.rs`](/Users/dmulcahey/development/skills/superpowers/src/update_check/mod.rs) still accepts a repo-local `VERSION` file as a default install signal.
- [`scripts/gen-skill-docs.mjs`](/Users/dmulcahey/development/skills/superpowers/scripts/gen-skill-docs.mjs) still emits root-detection logic and upgrade notes that reference legacy roots.
- [`featureforge-upgrade/SKILL.md`](/Users/dmulcahey/development/skills/superpowers/featureforge-upgrade/SKILL.md) still owns separate install-root search logic, including legacy roots.
- [`src/session_entry/mod.rs`](/Users/dmulcahey/development/skills/superpowers/src/session_entry/mod.rs) owns session-entry state resolution, but spawned-subagent bypass is still described primarily in [`skills/using-featureforge/SKILL.md`](/Users/dmulcahey/development/skills/superpowers/skills/using-featureforge/SKILL.md).
- [`docs/testing.md`](/Users/dmulcahey/development/skills/superpowers/docs/testing.md) still duplicates one `cargo nextest` command and omits one generated-doc freshness check already implied elsewhere.
- [`TODOS.md`](/Users/dmulcahey/development/skills/superpowers/TODOS.md) already points at the unfinished cutover, session-entry, and install-smoke work, which means the remediation program should reshape and extend that known backlog instead of pretending to start from zero.

## Delivery Model

The remediation program keeps the workstreams for traceability, but execution is phase-based:

```text
FF-01..FF-11 findings
        |
        v
Phase 1  -> WS1  runtime root-resolution contract
Phase 2  -> WS3  runtime-owned subagent/session-entry policy
Phase 3  -> WS2  hard canonical cutover and legacy-surface removal
Phase 4  -> WS4  docs, validation, and example convergence
Phase 5  -> WS5 + WS6  helper extraction, module decomposition, typed CLI cleanup
```

The ordering is intentional:

- Phase 1 stabilizes the highest-severity contract bug before anything else changes around it.
- Phase 2 stabilizes the other live runtime/prose contract split before cutover cleanup removes more surface area.
- Phase 3 removes unsupported legacy behavior only after the runtime contracts it used to mask are stable.
- Phase 4 aligns docs and validation after the product story is real.
- Phase 5 performs the lower-risk cleanup and maintainability work only after behavior is pinned.

## Phase Overview

### Phase 1: Runtime Root-Resolution Contract

**Primary workstream:** `WS1`

**Goal**

Make runtime root discovery deterministic and runtime-owned for `update-check`, generated skill preambles, and upgrade flows.

**Key changes**

- Introduce one shared runtime-root resolver module with separate discovery and validation concerns.
- Stop treating arbitrary repo-local `VERSION` files as enough to identify the active install.
- Expose one thin shell-facing helper surface so generators and upgrade docs stop re-implementing the algorithm.
- Update generated-skill preambles and upgrade flow references to consume the runtime contract instead of embedding legacy-root search logic.

**Expected file touch points**

- [`src/update_check/mod.rs`](/Users/dmulcahey/development/skills/superpowers/src/update_check/mod.rs)
- [`src/cli/mod.rs`](/Users/dmulcahey/development/skills/superpowers/src/cli/mod.rs)
- new runtime-root resolver module under `src/`
- [`scripts/gen-skill-docs.mjs`](/Users/dmulcahey/development/skills/superpowers/scripts/gen-skill-docs.mjs)
- [`featureforge-upgrade/SKILL.md`](/Users/dmulcahey/development/skills/superpowers/featureforge-upgrade/SKILL.md)

**Regression tests to add first**

- false-positive install regression for a repo with `VERSION` but no `bin/featureforge`
- positive repo-local runtime case
- binary-adjacent runtime case
- valid explicit `FEATUREFORGE_DIR` override case
- upgrade-specific validation case when runtime-valid roots are not upgrade-eligible

**Acceptance criteria**

- no runtime path infers the install root from `VERSION` alone
- one runtime-owned search order exists and active consumers use it
- generated skills and upgrade docs no longer embed legacy-root search logic

**Not in this phase**

- spawned-subagent session-entry policy
- forbidden-legacy-surface gate
- hotspot module decomposition unrelated to root resolution

### Phase 2: Runtime-Owned Session-Entry Policy

**Primary workstream:** `WS3`

**Goal**

Make spawned-subagent bypass behavior a runtime rule rather than a markdown convention.

**Key changes**

- Introduce one explicit runtime marker for spawned-subagent context.
- Teach session-entry resolution to bypass first-turn bootstrap by default for spawned subagents unless explicitly opted back in.
- Audit launcher and dispatcher surfaces so they set the runtime marker consistently.
- Rewrite skill prose to describe the runtime-owned rule instead of acting as the only source of truth.

**Expected file touch points**

- [`src/session_entry/mod.rs`](/Users/dmulcahey/development/skills/superpowers/src/session_entry/mod.rs)
- [`src/cli/session_entry.rs`](/Users/dmulcahey/development/skills/superpowers/src/cli/session_entry.rs)
- [`skills/using-featureforge/SKILL.md`](/Users/dmulcahey/development/skills/superpowers/skills/using-featureforge/SKILL.md)
- launcher or dispatcher-facing skill templates that start nested work

**Regression tests to add first**

- spawned subagent bypasses bootstrap by default
- explicit subagent opt-in re-enables FeatureForge
- direct human re-entry still works when supported
- nested review or audit flows do not emit bootstrap noise

**Acceptance criteria**

- runtime and skill behavior agree on spawned-subagent policy
- nested review and audit flows no longer rely on prose-only bypass behavior

**Not in this phase**

- legacy-root removal
- docs/testing convergence outside session-entry-specific instructions
- helper extraction that is not required for the runtime policy

### Phase 3: Hard Canonical Cutover

**Primary workstream:** `WS2`

**Goal**

Remove unsupported legacy-root behavior and references from active FeatureForge operation.

**Policy**

Legacy roots are unsupported. This phase does not preserve migration-only runtime compatibility and does not add a dedicated diagnostic path for removed legacy roots. Active behavior resolves only through the supported runtime contract established in Phase 1.

**Key changes**

- remove legacy roots from active root discovery, active generated-skill preambles, and active upgrade/public instructions
- add a forbidden-legacy-surface gate that fails on active files and generated artifacts while ignoring archive/history content where appropriate
- strengthen install-smoke coverage for checked-in prebuilt artifacts on supported layouts

**Expected file touch points**

- [`scripts/gen-skill-docs.mjs`](/Users/dmulcahey/development/skills/superpowers/scripts/gen-skill-docs.mjs)
- [`featureforge-upgrade/SKILL.md`](/Users/dmulcahey/development/skills/superpowers/featureforge-upgrade/SKILL.md)
- generated files under [`skills/`](/Users/dmulcahey/development/skills/superpowers/skills)
- [`tests/upgrade_skill.rs`](/Users/dmulcahey/development/skills/superpowers/tests/upgrade_skill.rs)
- cutover or install smoke coverage under [`tests/`](/Users/dmulcahey/development/skills/superpowers/tests)
- [`TODOS.md`](/Users/dmulcahey/development/skills/superpowers/TODOS.md)

**Regression tests to add first**

- forbidden-legacy-surface gate fails on active content and active paths
- forbidden-legacy-surface gate ignores archive/history fixtures
- upgrade-skill and generated-doc expectations reflect canonical-root-only output
- macOS arm64 and `windows-x64` install-smoke coverage validates expected checked-in artifact layout

**Acceptance criteria**

- active FeatureForge behavior no longer references `~/.codex/featureforge` or `~/.copilot/featureforge`
- active generated/public surfaces point only to canonical supported locations
- automation blocks reintroduction of active legacy-root references

**Not in this phase**

- contributor-doc cleanup that does not affect canonical cutover correctness
- large helper extraction unrelated to legacy-surface removal

### Phase 4: Docs, Validation, and Example Convergence

**Primary workstream:** `WS4`

**Goal**

Make contributor-facing documentation and validation commands match the stabilized product story.

**Key changes**

- replace duplicate or drifted validation instructions with one canonical release-facing entrypoint
- add missing generated-doc freshness references where the repo contract already depends on them
- clarify or add starter example/template guidance so the repository layout matches README claims
- reduce active platform-doc duplication where templating or generator-backed maintenance materially lowers drift risk

**Expected file touch points**

- [`README.md`](/Users/dmulcahey/development/skills/superpowers/README.md)
- [`docs/testing.md`](/Users/dmulcahey/development/skills/superpowers/docs/testing.md)
- platform install docs under [`docs/`](/Users/dmulcahey/development/skills/superpowers/docs)
- template or generator inputs for generated docs
- example or starter artifact paths if they are added

**Regression tests to add first**

- generated-doc freshness checks fail when checked-in generated artifacts drift
- doc contract tests cover the canonical validation entrypoint and starter/example expectations where those are machine-checkable

**Acceptance criteria**

- README, testing docs, generated-doc expectations, and starter/example guidance align on one canonical workflow story
- the documented validation path includes generated-doc freshness checks where the repo contract depends on them

**Not in this phase**

- runtime contract changes that should have landed in earlier phases
- module decomposition unrelated to documentation drift

### Phase 5: Structural Cleanup and CLI Hardening

**Primary workstreams:** `WS5`, `WS6`

**Goal**

Consolidate duplicated helper logic and improve maintainability only after behavior is already pinned.

**Key changes**

- extract duplicated repo slug derivation, markdown scanning, header parsing, hashing, install-root logic, and base-branch resolution into shared runtime-owned helpers where that meaningfully reduces drift
- split hotspot modules along responsibility boundaries once behavior is guarded by tests
- replace raw bounded-string CLI inputs with enums or equivalent typed parsing at the boundary
- make bare `featureforge` invocation print help instead of silently succeeding

**Expected file touch points**

- hotspot modules under [`src/`](/Users/dmulcahey/development/skills/superpowers/src)
- CLI modules under [`src/cli/`](/Users/dmulcahey/development/skills/superpowers/src/cli)
- helper modules under [`src/paths/`](/Users/dmulcahey/development/skills/superpowers/src/paths), [`src/workflow/`](/Users/dmulcahey/development/skills/superpowers/src/workflow), or new focused modules as needed
- contract and regression tests under [`tests/`](/Users/dmulcahey/development/skills/superpowers/tests)

**Regression tests to add first**

- CLI parse-boundary tests for new enums or bounded modes
- bare CLI help behavior tests
- helper-backed regression tests that prove extracted logic preserved earlier behavior

**Acceptance criteria**

- duplicate contract logic is materially reduced without reopening earlier behavior decisions
- hotspot modules are narrower and easier to review
- CLI behavior is stricter and clearer at the parse boundary

**Not in this phase**

- policy changes already settled earlier in this spec
- retroactive expansion of remediation scope beyond `FF-01` through `FF-11`

## Program-Level Rules

1. Every finding `FF-01` through `FF-11` remains in scope for the umbrella remediation program and its implementation plan.
2. Later phases may depend on artifacts from earlier phases, but they may not silently fix earlier-phase behavior under the banner of cleanup.
3. Each phase must land its regression coverage before or alongside the behavior change it protects.
4. Any new abstraction introduced during Phases 1 through 3 must earn its keep by reducing a real contract split, not by serving general cleanup preferences.
5. If a phase uncovers a missing prerequisite from an earlier phase, the work must be resequenced back into that earlier phase instead of hidden inside the current one.

## Failure Modes and Edge Cases

- **False-positive runtime discovery:** a non-FeatureForge repo with a top-level `VERSION` file must not be treated as the active install.
- **False-negative runtime discovery:** valid repo-local runtime checkouts, binary-adjacent installs, and explicit `FEATUREFORGE_DIR` overrides must still resolve correctly.
- **Session-entry noise in nested flows:** dispatched reviewers or auditors must not trigger first-turn bootstrap unexpectedly.
- **Generated-doc drift:** checked-in skills or upgrade docs must not continue carrying removed legacy-root behavior after the runtime cutover.
- **Active-vs-archived gating mistakes:** forbidden-legacy-surface automation must distinguish active files from preserved historical content.
- **Refactor regressions disguised as cleanup:** helper extraction and module splitting must preserve the earlier behavior contracts they build on.

## Observability and Verification Expectations

- Each phase must leave behind machine-checkable regression coverage for the behavior it changes.
- Generated artifacts must remain freshness-checkable in CI and local validation.
- Release-facing verification docs must describe the canonical commands needed to validate active runtime, generated docs, and install-smoke behavior.
- If a phase adds a new machine-readable helper surface, its output contract should be schemaable or otherwise precisely testable.

## Rollout and Rollback

- Roll forward by phase, not by mixing unrelated workstreams into one unbounded branch slice.
- Roll back at phase boundaries when a later phase exposes instability in an earlier contract.
- Do not start Phase 3 until Phases 1 and 2 regression suites are passing, because hard cutover removal is intentionally intolerant of hidden fallback behavior.
- Do not start Phase 5 until the canonical behavior story is stable enough that refactors can be evaluated as pure maintainability changes.

## Risks and Mitigations

- **Risk:** hard removal of legacy roots reveals hidden dependencies in tests or generated docs.
  **Mitigation:** Phase 3 adds explicit forbidden-legacy-surface coverage and updates all generated expectations together.
- **Risk:** helper extraction reopens behavior changes that should be closed.
  **Mitigation:** the spec defers WS5 and WS6 until after contract stabilization and requires helper-preservation regression tests.
- **Risk:** documentation cleanup drifts again after the cutover.
  **Mitigation:** Phase 4 ties docs changes to canonical validation commands and generated-doc freshness checks instead of relying on prose alone.

## Acceptance Criteria

- The repository contains one approved remediation spec and one derived implementation plan that cover the entire remediation set.
- Phase ordering in the plan follows the contract-first sequencing defined here.
- Legacy roots are absent from active behavior and active generated/public surfaces by the end of Phase 3.
- Runtime root resolution and spawned-subagent session-entry behavior are runtime-owned and regression-tested before later cleanup begins.
- Contributor docs, validation commands, and examples align with the stabilized canonical FeatureForge story.
- Structural cleanup work lands only after earlier behavior contracts are pinned and preserved by tests.

## Plan Handoff Notes

The follow-on implementation plan should stay umbrella-shaped and cover every phase in this spec. It should not flatten the work into one unordered checklist. Instead, it should group tasks by phase, preserve the phase gates, and make dependencies explicit so execution can proceed in reviewable slices without reopening product-policy questions.
