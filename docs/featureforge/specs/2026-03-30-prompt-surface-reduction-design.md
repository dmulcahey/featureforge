# FeatureForge Prompt-Surface Reduction and Skill-Doc Compaction

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

## Summary

FeatureForge should reduce prompt drag by compacting generated top-level `SKILL.md` documents while preserving the safety-critical operational contract in those top-level surfaces. The implementation should move long-form rationale, examples, and optional deep guidance into companion references, then enforce the new shape with contract tests and size-budget tests.

This is a hold-scope refinement: tighten implementation readiness, keep workflow law intact, and avoid speculative platform expansion.

## Problem Statement

Generated FeatureForge skill docs have grown large enough to degrade execution quality in Codex/Copilot contexts:

- model context is spent on repeated boilerplate before repo-specific work
- mandatory requirements are harder to detect when surrounded by repeated narrative
- large shared sections are emitted into many skills regardless of whether full-length prose is load-bearing
- prompt-surface compaction goals can regress without explicit budgets and tests

## Desired Outcome

After this work:

1. Top-level generated `SKILL.md` files are materially shorter.
2. Required operational law remains in top-level docs and remains fail-closed.
3. Extended rationale/examples/checklists live in predictable companion references.
4. Generator output remains deterministic.
5. Tests enforce both behavioral completeness and size budgets.

## Landscape Snapshot

Layer 1 (repo-native baseline): FeatureForge already has template-driven generation (`scripts/gen-skill-docs.mjs`), contract tests under `tests/codex-runtime/`, and one established deep reference pattern (`references/search-before-building.md`).

Layer 2 (current-practice risk): Many prompt systems fail by optimizing line count alone and accidentally stripping operational law. The key risk is silent safety drift, not missed formatting preferences.

Layer 3 (first-principles fit): The right split is minimal top-level law plus optional depth references, with tests that fail loudly when either behavior or budget regresses.

## Scope

In scope:

- compacting generator-emitted repeated prose where full length is not load-bearing
- preserving required helper commands, stop conditions, stage-order constraints, and approval boundaries in top-level `SKILL.md`
- introducing/using `REFERENCE.md` and optional `CHECKLIST.md` per skill where needed
- reducing top-level size for the highest-impact skills first
- adding/expanding tests that pin required phrase/command contracts and size budgets

## NOT in Scope

- rewriting FeatureForge workflow stages or authority ordering
- removing fail-closed behavior from any skill
- introducing runtime dependencies solely to parse generated output
- requiring companion refs for safe execution correctness
- broad edits to every skill in one pass; this spec starts with prioritized hotspots

## What Already Exists

- `scripts/gen-skill-docs.mjs` centralizes generation and is the highest leverage seam.
- Existing high-volume generated docs already identify hotspots:
  - `skills/plan-ceo-review/SKILL.md` (933 lines)
  - `skills/plan-eng-review/SKILL.md` (652 lines)
  - `skills/finishing-a-development-branch/SKILL.md` (496 lines)
  - `skills/subagent-driven-development/SKILL.md` (516 lines)
  - `skills/requesting-code-review/SKILL.md` (423 lines)
- Current total generated `skills/*/SKILL.md` size is 8,134 lines.
- Contract and generation test surfaces already exist and can be extended instead of replaced.

## Dream State Delta

```text
CURRENT STATE                 THIS SPEC DELTA                       12-MONTH IDEAL
large top-level skill docs -> compact top-level law + refs      -> concise, high-signal skills
repeated prose everywhere   -> shared concise wording in generator-> stable budgets + no drift
size pressure unmanaged     -> explicit budget tests             -> prompt-surface hygiene as a norm
```

## Requirement Index

- [REQ-001][behavior] Top-level generated `SKILL.md` must remain self-sufficient for safe execution and retain mandatory operational law.
- [REQ-002][behavior] Safety-critical content must stay top-level: helper invocations, fail-closed rules, stage order, approval boundaries, and required artifact headers.
- [REQ-003][behavior] Examples, rationale, and optional elaborations should move to per-skill companion refs when they are not load-bearing.
- [REQ-004][behavior] Shared generated sections must be compacted with consistent concise wording across targeted skills.
- [REQ-005][behavior] Companion-reference pointers in `SKILL.md` must be brief and emitted only when the referenced file exists.
- [REQ-006][behavior] Companion-reference absence must not break top-level execution safety.
- [REQ-007][behavior] Generation output must remain deterministic.
- [REQ-008][behavior] No new runtime dependencies may be introduced for generated skill/runtime flows.
- [REQ-009][verification] Tests must enforce presence of mandatory phrase/command contracts in top-level docs.
- [REQ-010][verification] Tests must enforce size budgets for targeted skills and aggregate generated size.
- [REQ-011][verification] Tests must fail on companion-pointer contract violations (pointer without file, or non-concise pointer block).
- [REQ-012][verification] The compaction change must preserve existing workflow routing/contract behavior in relevant suites.

## Design Overview

### Two-Layer Skill-Doc Model

- Layer 1 (`SKILL.md`): operational law used in active prompt surfaces.
- Layer 2 (`REFERENCE.md` and optional `CHECKLIST.md`): explanatory depth, long examples, and optional expanded guidance.

### Canonical Companion Layout

```text
skills/<skill-name>/
  SKILL.md.tmpl
  SKILL.md
  REFERENCE.md      # optional, recommended when long rationale/examples exist
  CHECKLIST.md      # optional, only when checklist content is large
```

### What Must Stay Top-Level

- explicit helper commands required by the workflow
- hard stop/fail-closed conditions
- stage sequencing that controls workflow correctness
- approval semantics and required headers/artifacts
- minimum command/decision outputs required to keep downstream stages safe

### What Should Move Out

- long narrative motivation
- repeated multi-paragraph shared explanations
- non-essential examples
- optional long checklists and style commentary

## Architecture Diagram

```text
skill template (.tmpl)
        |
        v
scripts/gen-skill-docs.mjs
        |
        +--> compact shared wording blocks
        +--> per-skill body rendering
        +--> optional concise pointer to REFERENCE.md/CHECKLIST.md
        v
generated SKILL.md (top-level operational law)
        |
        +--> tests/codex-runtime/* contract checks
        +--> size budget checks
        v
fail closed on contract or budget regressions
```

## Data Flow (Happy + Shadow Paths)

```text
INPUT TEMPLATE -> CLASSIFY CONTENT -> RENDER TOP-LEVEL -> EMIT COMPANION POINTER -> TEST
      |                  |                   |                     |                 |
      |                  |                   |                     |                 +--> fail (budget exceeded)
      |                  |                   |                     +--> skip pointer when file missing
      |                  |                   +--> fail if required law omitted
      |                  +--> fail if classification invalid
      +--> fail if template missing/unreadable
```

## Error & Rescue Registry

| Method/Codepath | What Can Go Wrong | Failure Class | Rescued? | Rescue Action | User Sees |
|---|---|---|---|---|---|
| `scripts/gen-skill-docs.mjs` template load | missing template path | `TemplateNotFound` | Y | fail generation with explicit file path | generation failure with actionable path |
| `scripts/gen-skill-docs.mjs` section compaction | required contract phrase removed | `ContractContentOmitted` | Y | fail contract tests; block merge | CI/test failure |
| companion pointer emission | pointer emitted without file | `CompanionPointerMismatch` | Y | fail contract test and remove bad pointer | CI/test failure |
| budget check suite | line/size budget regression | `SkillBudgetExceeded` | Y | fail budget test with before/after counts | CI/test failure |
| regeneration determinism | non-deterministic render ordering | `NonDeterministicOutput` | Y | fail deterministic snapshot/contract test | CI/test failure |

## Failure Modes Registry

| Codepath | Failure Mode | Rescued? | Test? | User Sees? | Logged? |
|---|---|---|---|---|---|
| generator shared-block compaction | removes fail-closed wording | Y | Y | CI failure | Y |
| pointer logic | emits stale pointer after file removal | Y | Y | CI failure | Y |
| template refactor | moves approval law to ref only | Y | Y | CI failure | Y |
| skill-specific edit | targeted skill exceeds budget | Y | Y | CI failure | Y |
| regen step skipped | stale generated docs committed | Y | Y | CI failure in doc-contract suite | Y |

Critical gap check: no known row has `Rescued=N` and `Test=N` with silent user impact.

## Priority Targets

Phase-1 targets (highest value first):

1. `skills/plan-ceo-review/*`
2. `skills/plan-eng-review/*`
3. `skills/finishing-a-development-branch/*`
4. `skills/subagent-driven-development/*`
5. `skills/requesting-code-review/*`

## File Touch Points

- `scripts/gen-skill-docs.mjs`
- `skills/plan-ceo-review/SKILL.md.tmpl`
- `skills/plan-eng-review/SKILL.md.tmpl`
- `skills/finishing-a-development-branch/SKILL.md.tmpl`
- `skills/subagent-driven-development/SKILL.md.tmpl`
- `skills/requesting-code-review/SKILL.md.tmpl`
- generated outputs for those templates (`skills/*/SKILL.md`)
- new companion refs under targeted skill directories as needed
- `tests/codex-runtime/skill-doc-contracts.test.mjs`
- `tests/codex-runtime/skill-doc-generation.test.mjs`
- any focused budget/contract helper tests added under `tests/codex-runtime/`

## Implementation Phases

### Phase 1: Contract Classification

For each priority skill, classify each block as:

- required top-level law
- compressible shared wording
- companion-reference candidate
- removable redundancy

Deliverable: a per-skill classification table committed with template updates.

### Phase 2: Generator Compaction

Update `scripts/gen-skill-docs.mjs` to:

- emit concise shared wording blocks
- preserve deterministic ordering
- support concise companion pointer insertion only when companion files exist

### Phase 3: Skill-by-Skill Extraction

For each priority skill:

- compact `.tmpl` top-level content
- add `REFERENCE.md` and optional `CHECKLIST.md` where needed
- regenerate `SKILL.md`

### Phase 4: Verification Hardening

Add or update tests for:

- mandatory top-level contract phrases/commands
- companion pointer rules
- deterministic generation behavior
- size budgets for priority skills and aggregate surface

### Phase 5: Rollout

Land as one coherent branch where template changes, generated outputs, and test updates ship together.

## Observability and Reporting

Track and report in CI:

- per-target-skill before/after line count
- aggregate `skills/*/SKILL.md` line delta
- number of companion references introduced
- contract test pass/fail and budget test pass/fail

Required evidence artifact for implementation: include a short table of before/after line counts in execution evidence.

## Verification Plan

Minimum command set for implementation PR validation:

```bash
node scripts/gen-skill-docs.mjs
node --test tests/codex-runtime/skill-doc-contracts.test.mjs
node --test tests/codex-runtime/skill-doc-generation.test.mjs
```

Add any new budget-specific test command to the same verification checklist once created.

## Rollout and Rollback

Rollout:

1. merge compaction changes with updated tests in one PR
2. ensure generated docs are committed alongside template updates
3. verify CI passes contract + generation + budget suites

Rollback:

1. revert compaction commit set
2. regenerate `SKILL.md` from prior templates
3. rerun contract and generation tests to confirm restored baseline

Rollback trigger examples:

- mandatory top-level behavior accidentally removed
- deterministic generation regression
- companion pointer regressions that break contract tests

## Risks and Mitigations

- Risk: safety-critical instructions are moved out of top-level docs.
  - Mitigation: classification-first workflow plus mandatory phrase/command contract tests.
- Risk: companion docs become required for correctness.
  - Mitigation: REQ-001/REQ-006 guard; top-level docs remain self-sufficient.
- Risk: budget checks incentivize harmful line-count gaming.
  - Mitigation: budget tests must always run with behavioral completeness tests.
- Risk: broad churn causes review fatigue and misses subtle regressions.
  - Mitigation: prioritize five hotspots first and keep deterministic generator coverage.

## Acceptance Criteria

- Targeted top-level `SKILL.md` files are materially shorter.
- `skills/plan-ceo-review/SKILL.md` and `skills/plan-eng-review/SKILL.md` each reduce by at least one-third from current baselines.
- Aggregate generated top-level size decreases meaningfully from 8,134 lines.
- Mandatory behavioral law remains in top-level docs and remains test-enforced.
- Companion refs contain depth content where extracted.
- Contract, generation, and budget tests pass with deterministic output.

## UI Scope

No UI scope detected for this spec.

## CEO Review Summary

**Review Status:** clear
**Reviewed At:** 2026-03-30T11:47:39Z
**Review Mode:** hold_scope
**Reviewed Spec Revision:** 1
**Critical Gaps:** 0
**UI Design Intent Required:** no
**Outside Voice:** skipped
