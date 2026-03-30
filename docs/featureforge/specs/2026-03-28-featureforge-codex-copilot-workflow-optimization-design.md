# FeatureForge Codex/Copilot Workflow Optimization and gstack-Informed Integration Spec

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review

**Date:** 2026-03-28
**Delivery Lane:** standard

## Source Inputs

This spec merges and supersedes the implementation intent from:

1. the existing **FeatureForge Workflow Streamlining and Contract Alignment Program**
2. the earlier **FeatureForge ← gstack Integration Report**

The goal of the merged program is narrower and more practical than “import gstack.” The goal is to make **FeatureForge the easiest safe way to deliver software features and fixes with Codex and Copilot** while keeping FeatureForge’s strongest properties intact:

- runtime-owned workflow truth
- artifact-backed approvals and receipts
- fail-closed execution and finish gating
- protected-branch discipline
- deterministic recovery and handoff

This program deliberately imports only the gstack ideas that materially improve that experience.

---

## Executive Summary

FeatureForge already has the stronger workflow architecture. The problem is not rigor. The problem is **too much avoidable ceremony in small changes, too much static routing, a few missing review surfaces, and not enough operator-visible state**.

The highest-value work is:

1. make `plan-fidelity-review` a first-class checked-in skill instead of an implied stage
2. create a real lightweight lane for bounded fixes and small feature deltas
3. add a structured **scope-drift check** to final review
4. add **distribution, publishability, and versioning** checks where they belong
5. add machine-readable **Risk & Gate Signals** so the runtime can choose the right downstream gates
6. add `plan-design-review` and `security-review`
7. improve the runtime/operator layer with a dashboard-style `workflow doctor`
8. add safe parallel-execution infrastructure: task-slice fences, reusable worktree management, patch harvesting, and dedup
9. remove prompt/runtime friction: wrong late-stage ordering, interpreter-dependent JSON parsing, stale naming/path drift, and bloated skill docs

The result should feel like this:

- bounded bugfixes stop paying for heavyweight planning behavior they do not need
- larger changes still get FeatureForge’s full rigor
- Codex and Copilot can see exactly what state the workflow is in and exactly what comes next
- downstream review/QA/security/design gates become **explicit and relevant**, not static or guessy
- finishing the branch stops looping because `document-release` happened at the wrong time
- the workflow stays authoritative and fail-closed instead of collapsing into ad hoc shell glue

---

## Product Goal and North Star

FeatureForge should make the common development loop feel simple and trustworthy for both Codex and Copilot:

1. understand the current workflow state immediately
2. create or refine a bounded spec without unnecessary ceremony
3. review scope before the plan grows
4. produce a contract-complete plan with explicit downstream gate signals
5. execute safely, including parallel execution when justified
6. run only the downstream gates that actually apply
7. finish the branch without stale-artifact churn or hidden missing work

### What “best experience” means here

For this program, “best experience” means:

- **low ceremony for small changes**
- **high clarity for larger changes**
- **no hidden workflow stages**
- **no shell/runtime dependency surprises**
- **one-screen visibility into readiness**
- **safe parallelism without manual patch wrangling**
- **skills that are concise enough for assistants to use effectively**
- **quality gates that match the actual risk of the change**

### What this program is not trying to do

This program does **not** try to:

- replace FeatureForge’s runtime-owned workflow with gstack’s looser operator model
- add a monolithic `/ship` equivalent
- add gstack telemetry/state conventions
- split the front door into multiple overlapping “start here” skills
- weaken approval law, plan fidelity, or final-review independence
- import the browser runtime stack yet; that is an important follow-on, but not part of this merged implementation spec

---

## Design Principles

1. **Runtime truth beats prompt convention.**  
   If the workflow depends on a fact, the runtime must own or validate it.

2. **Ceremony must scale with risk.**  
   Bounded changes should not feel like large-platform rewrites.

3. **Dynamic gates beat static default stacks.**  
   Design, security, QA, release, and other gates should activate when the change needs them.

4. **Every required stage must be first-class and visible.**  
   Hidden or implied stages create inconsistent assistant behavior.

5. **The finish path must be freshness-aware and churn-resistant.**  
   Repo-affecting completion work belongs before independent final review.

6. **Machine-readable workflow contracts should come from FeatureForge itself.**  
   Skills should not rely on `node -e`, `jq`, `python`, or other ad hoc parsing helpers.

7. **The worktree substrate should be powerful but mostly invisible.**  
   Users should benefit from safer parallel execution without hand-managing patch flows.

8. **Top-level skill docs are contracts, not essays.**  
   Mandatory behavior stays in `SKILL.md`; examples and long explanation move to companion refs.

9. **The workflow should optimize for Codex and Copilot equally.**  
   No model-specific hacks should become the normative path.

---

## Desired End State

At the end of this program, FeatureForge should provide all of the following:

- a checked-in, documented, test-enforced `plan-fidelity-review` skill
- an explicit `lightweight_change` lane that compresses planning behavior without bypassing approvals
- a required final-review scope check with explicit drift/missing-requirement classifications
- distribution and publishability scrutiny in the right review stages
- explicit versioning decisions in release documentation flows
- machine-readable `Risk & Gate Signals` in approved plans
- runtime-selected downstream gates instead of mostly static routing
- a first-class `plan-design-review` skill for material UI changes
- a first-class `security-review` skill for security-sensitive changes
- a dashboarded `workflow doctor` that explains route state, required gates, freshness, and next action
- safer execution and parallelization via task-slice fences, worktree helpers, and harvested patch evidence
- `document-release` ahead of final code review in the normative finish order
- no active interpreter-dependent JSON parsing in generated skills
- no stale `Superpowers` naming or machine-local absolute paths in active surfaces
- materially smaller and less repetitive top-level skill docs

---

## Success Metrics

This program is successful when the following are true:

### Workflow and UX metrics

- bounded bugfixes and small scoped enhancements have a visibly shorter planning path than standard work
- `workflow doctor` can explain the current state and next required step on one screen
- `document-release` no longer commonly forces a rerun of final review in the default path
- worktree setup no longer nudges fresh repos into `.gitignore` churn by default
- dynamic gate routing lowers false-positive downstream work

### Assistant ergonomics metrics

- top-level `SKILL.md` files for the busiest skills shrink materially
- no active generated skill requires `node`, `python`, `jq`, `perl`, or `ruby` for FeatureForge-owned command parsing
- all required stage transitions are explicit enough that Codex and Copilot follow the same path

### Quality metrics

- final review can distinguish clean implementation, acceptable drift, and missing required work
- material UI changes receive a dedicated design pass before implementation
- security-sensitive changes can trigger a dedicated security pass before finish
- release surfaces have explicit publishability and versioning decisions instead of implied assumptions

---

## Repo Reality Check

This program is grounded in the current repo state.

### Existing strengths already present in FeatureForge

- runtime-owned workflow and authoritative routing already exist
- plan, execution, QA, and release artifacts already have contract structure
- plan-fidelity receipt recording already exists in the runtime
- protected-branch write controls already exist
- workflow doctor, gate-review, gate-finish, and execution preflight already exist

### Current seams and gaps this program addresses

- `src/contracts/plan.rs` already defines the `featureforge:plan-fidelity-review` stage, but the repo does not expose a checked-in `skills/plan-fidelity-review/` skill
- public workflow docs still skip the dedicated plan-fidelity stage
- the workflow still defaults to too much ceremony for bounded changes
- final review currently lacks a structured scope-drift check
- downstream routing is still too static because plans do not emit enough machine-readable risk/gate signal
- there is no dedicated `plan-design-review`
- there is no dedicated `security-review`
- `document-release` is ordered late enough to stale earlier final review artifacts in the common path
- `workflow doctor` is useful but not yet a true readiness dashboard
- execution/subagent flows do not yet have a FeatureForge-native worktree manager with patch harvesting and dedup
- generated skills still contain interpreter-dependent JSON extraction patterns
- active docs still include stale naming/path drift
- generated skill docs are still too repetitive and too large for efficient assistant use

---

## Scope

### In scope

- first-class `plan-fidelity-review`
- lightweight lane for bounded changes
- scope-drift detection in final review
- distribution/publishability checks
- explicit versioning decision logic in release flows
- dashboard expansion of `workflow doctor`
- `Risk & Gate Signals` in plans and runtime consumption of those signals
- `plan-design-review`
- `security-review`
- task-slice write fences
- reusable worktree manager with patch harvesting/dedup
- late-stage finish-order correction
- worktree default-path improvement
- shell-friendly CLI contracts for skills
- active docs/path cleanup
- prompt-surface reduction and skill-doc compaction
- targeted improvements to `systematic-debugging` and `receiving-code-review`
- regression coverage for all new behavior

### Out of scope

- importing gstack’s browser runtime in this program
- importing gstack’s monolithic `/ship` workflow
- importing gstack telemetry, remote logging, or `.gstack`-style state conventions
- adding `office-hours`, `investigate`, `freeze`, `guard`, or `ship` as first-class FeatureForge skills
- rewriting archived/historical docs only for branding cleanup
- weakening final-review independence, plan-fidelity enforcement, or approval law

---

## Explicit gstack Imports and Explicit Non-Imports

### Imported ideas

This spec intentionally pulls in these gstack ideas in FeatureForge-native form:

- scope-drift detection from `/review`
- distribution/publishability checks from `/plan-eng-review` and `/ship`
- explicit version-bump decision logic from `/document-release`
- worktree patch harvesting and dedup concepts from `lib/worktree.ts`
- task-slice safety fences inspired by `/freeze` and `/guard`
- dedicated `plan-design-review`
- dedicated `security-review`
- dashboard-style readiness summary from `/ship`

### Not imported as-is

This spec intentionally does **not** import:

- `/ship` as one giant workflow
- gstack telemetry/state conventions
- `office-hours` as a separate front door
- `investigate` as a duplicate of `systematic-debugging`
- `design-shotgun` / `design-consultation` as-is
- standalone `freeze`, `guard`, or `unfreeze` user-facing skills

---

## Key Decisions

- **DEC-001:** `plan-fidelity-review` becomes a first-class checked-in skill, not an implied stage.
- **DEC-002:** the lightweight lane is a **compressed workflow lane**, not an approval bypass.
- **DEC-003:** final review must include a structured scope check and must not silently pass missing required work.
- **DEC-004:** distribution and publishability are first-class review concerns for new or changed user-facing artifacts.
- **DEC-005:** release/versioning decisions must be explicit whenever the repo has versioned release surfaces.
- **DEC-006:** plans must emit machine-readable `Risk & Gate Signals`, and the runtime must consume them.
- **DEC-007:** `plan-design-review` is a planning-time gate for material UI changes.
- **DEC-008:** `security-review` is a post-implementation specialized gate for security-sensitive changes.
- **DEC-009:** repo-affecting completion work must happen before independent final code review.
- **DEC-010:** the default recommended worktree location becomes `~/.config/featureforge/worktrees/<project-name>/`, while explicit repo-local paths remain supported.
- **DEC-011:** shell-facing consumers must obtain machine-readable values from FeatureForge CLI contracts rather than interpreter snippets.
- **DEC-012:** active docs and prompts must stop teaching stale names and machine-local absolute paths.
- **DEC-013:** top-level `SKILL.md` files remain self-sufficient operational contracts, while long-form guidance moves to companion refs.
- **DEC-014:** legacy plans/specs must remain readable; new contract requirements apply to new or materially revised artifacts via contract versioning.
- **DEC-015:** the browser-runtime program remains a follow-on initiative; this spec only lays the contract groundwork for it.

---

## Assistant Experience Requirements

The workflow must actively optimize for Codex and Copilot.

### Required experience properties

- **single-source routing:** one front door and one runtime-owned explanation of current state
- **contract-complete brevity:** assistants can operate from top-level skills without opening multiple large docs by default
- **shell-safe branching:** assistants should not have to write inline parsers for FeatureForge JSON
- **bounded-change compression:** small fixes should move with less narrative and fewer pause cycles
- **explicit gate visibility:** assistants should know which gates are required and why
- **safe recovery:** reopen, redirect, or finish decisions should come from authoritative state, not memory
- **portable setup:** default worktree flow should avoid repo churn and local-environment surprises

---

## Program Structure and Source Alignment

This merged program preserves the best parts of the attached streamlining spec and adds the highest-value items from phases 1 and 2 of the gstack integration recommendation.

| Merged program phase | Primary source |
| --- | --- |
| Phase 1 — First-class `plan-fidelity-review` | attached spec phase 1 |
| Phase 2 — Lightweight lane for bounded changes | attached spec phase 2 |
| Phase 3 — Review, release, and operator-surface improvements | gstack integration phase 1 |
| Phase 4 — Dynamic gate model and new review surfaces | gstack integration phase 2 |
| Phase 5 — Execution safety and worktree substrate | gstack integration phase 2 + attached spec phase 4 |
| Phase 6 — Finish-path reorder and gate precedence | attached spec phase 3, expanded to fit dynamic gates |
| Phase 7 — Shell-friendly CLI contracts and no interpreter snippets | attached spec phase 5 |
| Phase 8 — Active namespace/path drift cleanup | attached spec phase 6 |
| Phase 9 — Prompt-surface reduction and skill-doc compaction | attached spec phase 7 |

This order is intentional:

1. expose the hidden stage
2. reduce bounded-change friction
3. improve review/release correctness and operator visibility
4. add smarter plan-time routing and required review surfaces
5. improve execution ergonomics and safe parallelization
6. fix late-stage sequencing once the new gates exist
7. remove remaining tooling/documentation friction

---

## Requirement Index

- [REQ-001][behavior] FeatureForge must ship a first-class `featureforge:plan-fidelity-review` skill with explicit instructions for producing the independent review artifact and recording the runtime-owned receipt.
- [REQ-002][behavior] public workflow docs and router guidance must explicitly include `plan-fidelity-review` in the planning path.
- [REQ-003][behavior] FeatureForge must provide a true `lightweight_change` lane for bounded changes that reduces ceremony while preserving explicit artifact state, approvals, and plan-fidelity enforcement.
- [REQ-004][behavior] the lightweight lane must escalate back to `standard` when disqualifying conditions appear.
- [REQ-005][behavior] final review must include a structured scope check comparing intended scope with actual implementation.
- [REQ-006][behavior] the scope check must classify outcomes as `CLEAN`, `DRIFT_DETECTED`, or `REQUIREMENTS_MISSING`.
- [REQ-007][behavior] `REQUIREMENTS_MISSING` must block review completion and branch finish until execution is reopened or the missing work is otherwise resolved.
- [REQ-008][behavior] `DRIFT_DETECTED` must require explicit resolution: accept, TODO/follow-up, or reopen execution.
- [REQ-009][behavior] `plan-ceo-review`, `plan-eng-review`, and `document-release` must include explicit distribution/publishability checks whenever the change introduces or modifies a user-facing artifact, entry point, package, route, flag, workflow, or deployable surface.
- [REQ-010][behavior] `document-release` must require an explicit versioning decision and rationale whenever the repo contains a versioned release surface.
- [REQ-011][behavior] `workflow doctor` must surface a dashboard-style readiness summary that includes route state, approvals, freshness, required gates, and next action, and all read-only operator surfaces must derive those fields from a shared runtime-owned operator snapshot rather than duplicated route/render logic.
- [REQ-012][behavior] approved plans must include machine-readable `Risk & Gate Signals`.
- [REQ-013][behavior] `plan-eng-review` must validate/finalize the `Risk & Gate Signals`.
- [REQ-014][behavior] the runtime must consume approved `Risk & Gate Signals` when deciding downstream required gates.
- [REQ-015][behavior] FeatureForge must ship a first-class `featureforge:plan-design-review` skill for material UI changes.
- [REQ-016][behavior] FeatureForge must ship a first-class `featureforge:security-review` skill for security-sensitive changes.
- [REQ-017][behavior] the finish path must run repo-affecting completion gates before independent final code review.
- [REQ-018][behavior] `document-release` must occur before final code review whenever release documentation is required or stale.
- [REQ-019][behavior] the default recommended worktree location must become the global FeatureForge path while preserving explicit repo-local preferences.
- [REQ-020][behavior] execution and subagent flows must support task-slice write fences derived from plan task `Files:` blocks.
- [REQ-021][behavior] execution and subagent flows must support reusable worktree management with run-scoped directories, patch harvesting, dedup, and stale-prune behavior.
- [REQ-022][behavior] generated skill docs must not rely on external interpreter snippets to extract values from FeatureForge CLI output.
- [REQ-023][behavior] active docs and prompts must stop using stale `Superpowers` naming and machine-local absolute paths in normative guidance.
- [REQ-024][behavior] top-level skill docs must shrink materially while preserving mandatory stop/fail-closed rules in `SKILL.md`.
- [REQ-025][behavior] `systematic-debugging` must produce a lightweight debug report artifact or standard summary output.
- [REQ-026][behavior] `receiving-code-review` must support batching clearly mechanical low-risk comments while keeping judgment-heavy items isolated.
- [REQ-027][behavior] `verification-before-completion` and `finishing-a-development-branch` must reflect dynamic required gates and scope-check state.
- [REQ-028][contract] spec and plan contracts must version any new required metadata so legacy artifacts remain readable.
- [REQ-029][contract] new or materially revised specs must carry `Delivery Lane: standard | lightweight_change`.
- [REQ-030][contract] new or materially revised plans must carry both `Delivery Lane` and `Risk & Gate Signals`.
- [REQ-031][contract] required review artifacts/receipts for plan fidelity, design review, security review, scope check, and release readiness must be bound to the relevant artifact revision or execution fingerprint.
- [REQ-032][contract] CLI read-only operator surfaces must support stable machine-readable output for skill consumption and must expose fields from the same shared runtime-owned operator snapshot used by human-readable workflow status surfaces.
- [VERIFY-001][verification] each phase must add targeted regression coverage before or alongside behavior changes.
- [VERIFY-002][verification] generation/tests must fail if required checked-in skills disappear or stale workflow ordering returns.
- [VERIFY-003][verification] generation/tests must fail if active skills reintroduce forbidden interpreter snippets, stale naming/path drift, or oversized repeated prompt surfaces.
- [VERIFY-004][verification] workflow/runtime tests must cover dynamic gate selection, scope-check behavior, freshness invalidation, and worktree/fence behavior.

## End-State Workflow

### Planning path

```text
brainstorming
  -> plan-ceo-review
  -> writing-plans
  -> plan-fidelity-review
  -> plan-eng-review
     -> plan-design-review (when required)
     -> back to plan-eng-review for final approval
  -> implementation_ready
```

### Execution and finish path

```text
implementation_ready
  -> execution_preflight / recommend
  -> executing-plans or subagent-driven-development
  -> pre-final-review completion gates
       - document-release
       - security-review (when required)
  -> requesting-code-review (includes scope check)
  -> verification gates
       - qa-only (when required)
       - future perf/deploy gates when later added
  -> verification-before-completion
  -> finishing-a-development-branch
```

### Core workflow rule

The runtime, not the prompt, determines:

- current phase
- required next skill
- required downstream gates
- freshness/staleness of review/QA/release/security artifacts
- whether execution must reopen

---

## Detailed Design

## Phase 1 — First-Class `plan-fidelity-review`

### Goal

Turn the existing runtime-owned plan-fidelity contract into a real user-visible workflow stage with checked-in skill material and explicit public routing.

### Why this matters for Codex/Copilot

Today the stage exists in contract law but not as a first-class skill surface. That creates inconsistent assistant behavior because one assistant may infer the stage while another may not. A checked-in skill fixes that.

### Required behavior

The new skill must:

1. require the exact approved spec path and current draft plan path
2. review only fidelity, not business scope expansion or final engineering approval
3. verify, at minimum:
   - requirement-index completeness against the approved spec
   - coverage fidelity between requirements and plan steps
   - execution-topology fidelity versus the plan’s own claims
   - lane fidelity (`Delivery Lane` in spec and plan must agree)
4. produce a dedicated independent review artifact in the runtime-owned artifact area
5. record the runtime-owned receipt through the existing plan-fidelity recording flow
6. return control to:
   - `featureforge:writing-plans` when fidelity fails
   - `featureforge:plan-eng-review` only after a fresh recorded pass receipt

### Required artifact content

The review artifact should include:

- review target metadata (spec, plan, revisions)
- verdict: `pass` or `revise`
- requirement coverage gaps
- topology/fidelity concerns
- lane mismatch, if any
- explicit pass/fail rationale

### File touch points

- new: `skills/plan-fidelity-review/SKILL.md.tmpl`
- new: generated `skills/plan-fidelity-review/SKILL.md`
- new: companion reviewer/reference material under `skills/plan-fidelity-review/`
- update: `README.md`
- update: `docs/README.codex.md`
- update: `docs/README.copilot.md`
- update: `skills/using-featureforge/SKILL.md.tmpl`
- update: `skills/writing-plans/SKILL.md.tmpl`
- update: `skills/plan-eng-review/SKILL.md.tmpl`
- optional clarifications: `src/workflow/status.rs`, `src/cli/workflow.rs`, `src/lib.rs`

### Tests

- skill-generation/doc-contract test asserting the skill exists
- workflow-doc tests asserting the stage appears in public workflow docs
- routing tests asserting no pass receipt routes back to `writing-plans`
- routing tests asserting fresh pass receipt routes to `plan-eng-review`
- skill-contract tests asserting runtime-owned receipt recording is mandatory

### Acceptance criteria

- checked-in skill exists
- public docs show the stage
- plan review routing becomes explicit and deterministic
- no planning surface depends on an implied hidden stage

---

## Phase 2 — Lightweight Lane for Bounded Changes

### Goal

Create a low-ceremony lane for bounded fixes and small, contained feature deltas without weakening approvals, provenance, or plan fidelity.

### Rationale

FeatureForge is strong, but the default path is still too heavy for work like:

- a bounded bugfix
- a narrow refactor
- a small enhancement with one clear user-facing delta
- a contained internal workflow fix

This is one of the highest-leverage changes for assistant usability. Codex and Copilot do better when the workflow makes scope explicit and shortens unnecessary narrative.

### Delivery-lane contract

Introduce a required artifact marker:

```md
**Delivery Lane:** standard | lightweight_change
```

This marker must appear in both new/materially revised specs and plans.

### Qualification rules

`lightweight_change` is allowed only when all qualification checks pass:

- one bounded bugfix, refactor, or tightly scoped enhancement
- no new external dependency or external service requirement
- no new auth/session model
- no schema migration/backfill requirement
- no new public API/protocol surface
- no multi-team rollout coordination
- no high-risk operational change
- no materially ambiguous user-scope expansion

### Initial threshold decisions (v1)

`lightweight_change` also requires all objective caps below at plan approval time:

- max 8 non-generated files in aggregate `Files:` scope across tasks
- estimated net delta <= 400 changed lines (adds + dels), excluding generated skill/doc output
- one primary user-visible behavior delta and at most one additional internal subsystem touch
- `Release Surface` must remain `docs_only` or `code_only_no_deploy`
- `Deploy Impact`, `Distribution Impact`, and `Migration Risk` must each be `none` or `low`
- `Security Review Required: yes` is a hard escalation to `standard` in this rollout

### Disqualifiers and escalation

If any of the following appears at any later review stage, the artifact must be promoted back to `standard`:

- scope expands beyond the original bounded delta
- a migration or rollout dependency appears
- auth/security complexity becomes material
- UI scope becomes materially broader than stated
- public API, package, integration, or deploy implications appear
- the review finds missing exploratory work that the lightweight lane deliberately skipped

### Behavior by stage

#### `brainstorming`

For qualified lightweight changes:

- allow concise delta-oriented spec authoring
- skip optional expansion-heavy behaviors like broad multi-approach ideation when unnecessary
- keep the spec real and reviewable, but shorter and more bounded
- require a short “bounded-change statement” that explains what is changing and what is explicitly out of scope

#### `plan-ceo-review`

For qualified lightweight changes:

- default to hold-scope rigor, not expansion-first behavior
- allow bundled resolution of multiple low-judgment issues
- suppress delight/opportunity expansion unless the current artifact is obviously under-scoped in a way that would make it misleading or broken
- preserve approval authority fully

#### `writing-plans`

For qualified lightweight changes:

- preserve all required contract sections
- allow shorter prose and simpler topology
- default topology recommendation to serial execution unless safe parallel isolation is explicitly demonstrated
- require the plan to justify why lightweight is safe in one concise section

#### `plan-fidelity-review`

Must verify that the plan did not silently expand beyond lightweight qualification.

#### `plan-eng-review`

- map any existing `small_change` behavior to `lightweight_change`
- keep approval law unchanged
- explicitly escalate to `standard` when disqualifiers appear
- validate downstream `Risk & Gate Signals` even for lightweight plans

### Required plan content for lightweight lane

The plan must include:

- `Delivery Lane`
- a short “Why lightweight is safe” note
- normal coverage matrix and task structure
- normal `Files:` declarations per task
- any required risk/gate signals

### File touch points

- update: `skills/brainstorming/SKILL.md.tmpl`
- update: `skills/plan-ceo-review/SKILL.md.tmpl`
- update: `skills/writing-plans/SKILL.md.tmpl`
- update: `skills/plan-eng-review/SKILL.md.tmpl`
- update: `skills/plan-fidelity-review/SKILL.md.tmpl`
- update: `src/contracts/spec.rs`
- update: `src/contracts/plan.rs`
- update: `src/workflow/status.rs` if lane status/help text is surfaced

### Tests

- contract tests for parsing `Delivery Lane` in spec/plan
- doc-contract tests for lightweight-lane behavior across the four planning skills
- escalation tests forcing promotion to `standard`
- routing tests confirming lightweight plans still require approvals and fidelity review

### Acceptance criteria

- one explicit lightweight lane exists across the planning stack
- bounded changes clearly take a lower-ceremony path
- approvals and fidelity remain mandatory
- risk expansion forces escalation back to the standard lane

---

## Phase 3 — Review, Release, and Operator-Surface Improvements

This phase brings in the highest-value items from phase 1 of the gstack integration recommendation.

## 3.1 Scope Drift Detection in Final Review

### Goal

Add a structured, explicit “did we build what we said we would build?” check before or inside final code review.

### Why this matters

FeatureForge already tracks provenance and state very well. That is not the same as validating that the final implementation still matches spec and plan intent. This is a real quality gap.

### Inputs

The scope check must compare:

- approved spec
- approved plan
- requirement coverage matrix
- completed task packets/evidence
- actual diff (working tree and/or branch diff as appropriate)
- optional supporting signals such as TODO updates or commit messages

### Required classifications

The scope check must produce one of:

- `CLEAN`
- `DRIFT_DETECTED`
- `REQUIREMENTS_MISSING`

### Required policy

- `CLEAN` may proceed normally.
- `DRIFT_DETECTED` must not silently pass. The reviewer must explicitly resolve it as one of:
  - accepted as still within intent
  - accepted but spun into a TODO/follow-up
  - reopen execution to remove/contain the drift
- `REQUIREMENTS_MISSING` must block finish and reopen execution or otherwise force explicit remediation.

### Required outputs

The scope-check result must be:

- written into a runtime-owned review artifact or review sub-artifact
- surfaced in `workflow doctor`
- surfaced in `finishing-a-development-branch`
- carried into final-review freshness and finish-gate logic

### File touch points

- update: `skills/requesting-code-review/SKILL.md.tmpl`
- update: generated `skills/requesting-code-review/SKILL.md`
- update: `skills/finishing-a-development-branch/SKILL.md.tmpl`
- update: `skills/verification-before-completion/SKILL.md.tmpl`
- update: `src/execution/state.rs`
- update: `src/execution/harness.rs`
- update: `src/workflow/operator.rs`
- update: `src/cli/plan_execution.rs` or equivalent review-gate surface

### Tests

- fixture-based scope-check tests for all three classifications
- gate-review tests ensuring `REQUIREMENTS_MISSING` blocks
- finish-gate tests ensuring unresolved drift blocks finish
- doctor/render tests surfacing scope-check status and next action

## 3.2 Distribution and Publishability Checks

### Goal

Prevent “technically complete but not actually reachable” work from being approved or finished.

### Where the checks belong

#### `plan-ceo-review`

For any new or changed external artifact, user entry point, route, package, command, workflow, or discoverability surface, require the review to ask:

- how does this reach the user?
- where does the user discover it?
- what changes after merge or deploy?
- if the artifact exists but is not reachable/discoverable, is it actually complete?

#### `plan-eng-review`

Require a more operational version of the same question:

- what makes this reachable after merge?
- are packaging, routes, flags, docs, CI/CD, deploy config, or rollout steps missing?
- is this “buildable” but not actually usable?

#### `document-release`

Require final release-surface verification:

- is the release surface correctly classified?
- are docs, flags, routes, commands, or package/install instructions aligned with the final implementation?
- is the discoverability/distribution path complete and honest?

### Required release-surface classification

`document-release` must explicitly classify the release surface as one of:

- `docs_only`
- `code_only_no_deploy`
- `library_package`
- `app_service_deploy`

This classification is helpful both for human clarity and for future runtime gate expansion.

## 3.3 Explicit Versioning Decisions

### Goal

Make version handling explicit wherever the repo has versioned release surfaces.

### Required behavior

If the repo uses any versioned release surface such as:

- `VERSION`
- package versions
- Cargo/npm/pip/etc. package metadata
- release tags
- structured release notes or changelog version sections

then `document-release` must require:

- `Versioning Decision: none | patch | minor | major`
- `Versioning Rationale: <short explanation>`

`unknown` is allowed during planning but must not survive completion of `document-release`.

### Policy

- if a versioned surface exists and the decision is omitted, `document-release` is incomplete
- if the decision is `none`, rationale is still required
- if the change is `library_package` or other packaged artifact, versioning must be addressed explicitly even for lightweight changes

## 3.4 Dashboard-Style `workflow doctor`

### Goal

Make `workflow doctor` the canonical one-screen readiness summary.

### Required dashboard fields

At minimum, the human-readable dashboard must show:

- route state / phase
- next skill
- next action
- spec status
- plan status
- `Delivery Lane`
- plan-fidelity receipt status
- execution status
- active task / blocked task / resume hint when applicable
- scope-check result
- required gate summary:
  - design review
  - security review
  - document release
  - final code review
  - browser QA
- freshness/staleness summary for each required gate
- per-gate blocking detail for unresolved gates, including artifact fingerprint, stale reason code, owner skill, and reroute target
- release surface classification
- versioning decision/status
- protected-branch or finish constraints
- stale reason codes / why FeatureForge chose this route
- rollout-readiness metrics when a feature is in phased enforcement, including fence false-positive rate, override rate, blocked-lane resolution count, and current enforcement mode

### Shared operator snapshot contract

`workflow doctor`, `workflow handoff`, JSON output, and shell-friendly field output must all read from the same runtime-owned operator snapshot.

That shared snapshot must:

- normalize route state, gate freshness, approvals, and next action once per observed artifact/runtime state
- feed both human-readable and machine-readable output modes without separate route recomputation in each renderer
- surface partial or degraded snapshot state explicitly instead of silently omitting unknown fields
- preserve enough per-gate debug detail that a blocked or stale workflow can be diagnosed from the operator surface without opening separate artifacts first

### Output modes

Prefer extending existing read-only surfaces instead of inventing many new commands. `workflow doctor` should support:

- current human-readable summary
- a richer dashboard-oriented human-readable summary
- JSON output
- shell-friendly field output for skills

## 3.5 Small but useful quality-of-life improvements

### `systematic-debugging`

Add a lightweight debug report artifact or standard output with:

- symptom
- reproduction
- root cause
- evidence
- fix
- regression risk
- follow-up TODOs

### `receiving-code-review`

When processing multiple review comments:

- batch clearly mechanical low-risk fixes together
- keep semantic/judgment-heavy comments isolated and verified one at a time

This change should speed cleanup without weakening the skill’s “verify before agree” rule.

### File touch points for phase 3

- update: `skills/plan-ceo-review/SKILL.md.tmpl`
- update: `skills/plan-eng-review/SKILL.md.tmpl`
- update: `skills/document-release/SKILL.md.tmpl`
- update: `skills/requesting-code-review/SKILL.md.tmpl`
- update: `skills/receiving-code-review/SKILL.md.tmpl`
- update: `skills/systematic-debugging/SKILL.md.tmpl`
- update: `skills/using-featureforge/SKILL.md.tmpl`
- update: `skills/verification-before-completion/SKILL.md.tmpl`
- update: `skills/finishing-a-development-branch/SKILL.md.tmpl`
- update: `src/workflow/operator.rs`
- update: `src/execution/harness.rs`
- update: `src/execution/state.rs`
- update: `src/execution/topology.rs`
- update: `src/cli/plan_execution.rs`
- update: `src/lib.rs`

### Acceptance criteria

- final review surfaces explicit scope-check results
- publishability/distribution questions exist in the right places
- version decisions are explicit in release flows
- `workflow doctor` becomes a high-signal readiness dashboard
- `workflow doctor`, `workflow handoff`, JSON output, and shell fields stay aligned because they render from one runtime-owned operator snapshot
- debug/review-response skills become more efficient without losing rigor

---

## Phase 4 — Dynamic Gate Model and New Review Surfaces

This phase brings in the core of phase 2 from the gstack integration recommendation.

## 4.1 `Risk & Gate Signals` in Plans

### Goal

Teach plans to describe what downstream gates actually apply.

### Required plan section

New or materially revised plans must include:

```md
## Risk & Gate Signals
- Delivery Lane: standard | lightweight_change
- UI Scope: none | minor | material
- Browser QA Required: yes | no
- Design Review Required: yes | no
- Security Review Required: yes | no
- Performance Review Required: yes | no
- Release Surface: docs_only | code_only_no_deploy | library_package | app_service_deploy
- Distribution Impact: none | low | high
- Deploy Impact: none | low | high
- Migration Risk: none | low | high
```

### Required companion section

Plans must also include a concise text section for free-form details when relevant:

```md
## Release & Distribution Notes
- Discoverability / Distribution Path: ...
- Versioning Decision: none | patch | minor | major | unknown
- Versioning Rationale: ...
- Deployment / Rollout Notes: ...
```

### Why both sections exist

- `Risk & Gate Signals` are for runtime parsing and deterministic routing.
- `Release & Distribution Notes` are for concise human explanation that does not fit cleanly into enums.

### Ownership and lifecycle

- `writing-plans` drafts the signals
- `plan-fidelity-review` checks them for fidelity against the spec and plan body
- `plan-eng-review` validates and finalizes them
- the runtime stores or surfaces the approved signal set as the canonical routing input for that plan revision

Signal derivation and finalization during planning are approval-critical contract work. If required gate-signal derivation or validation fails before plan approval, the workflow must fail closed and return to the owning planning gate rather than silently substituting conservative guesses.

### Fail-closed behavior

A plan with missing required signal fields must not be treated as fully valid under the new contract version.

Conservative extra-gate fallback is only acceptable after a valid approved signal set already exists and a downstream runtime read/render path cannot fully consume it. That fallback must not rewrite or implicitly re-derive approved signal truth.

## 4.2 Signal Rules and Gate Semantics

### Core signal rules

- `UI Scope: material` generally implies `Design Review Required: yes`
- `UI Scope: material` usually implies `Browser QA Required: yes`
- changes involving auth/session/tokens/secrets/permissions/webhooks/infra/config/LLM attack surface should usually imply `Security Review Required: yes`
- `Release Surface: library_package` or `app_service_deploy` requires meaningful release/distribution notes
- `Migration Risk: high` is incompatible with lightweight delivery
- `Deploy Impact: high` or `Distribution Impact: high` should be visible in `workflow doctor` even if no deploy skill exists yet
- `Performance Review Required` may initially be informational only until a dedicated performance-review surface exists

### Runtime consumption

The runtime must use approved signals to decide at least:

- whether `plan-design-review` is required
- whether `security-review` is required
- whether browser QA is required
- which release/documentation prompts must be completed
- what `workflow doctor` shows as the required downstream gate set

### Safe defaults

For legacy plans that predate the new contract version:

- the runtime should preserve current safe behavior
- the dashboard should clearly label the plan as legacy/no-signal
- dynamic routing should be conservative rather than optimistic

### Initial security-review trigger thresholds (v1)

`Security Review Required` must be `yes` when any one of the following is true:

- auth/session/token/permission logic changes
- secrets handling, redaction, key use, or sensitive-data classification logic changes
- protected-branch, repo-safety, approval-bypass, or trust-boundary enforcement changes
- subprocess/shell invocation, path normalization, symlink handling, patch-application safety, or filesystem boundary rules change
- new inbound or outbound network integration surfaces (including webhook/callback handlers)
- new dependency introduction or major-version dependency upgrade in runtime or CLI-critical paths

`Security Review Required` may remain `no` only when all are true:

- change type is docs-only, tests-only, or behavior-preserving refactor
- no trust-boundary, secret, authz/authn, or integration-surface changes
- no security control is weakened by diff or configuration

For the initial rollout window, the target is <= 15% false-positive security-review triggers; if higher, tune heuristics without weakening hard trust-boundary triggers.

## 4.3 `plan-design-review`

### Goal

Add a dedicated planning-time gate for material UI work.

### Trigger

This skill activates when:

- `UI Scope` is `material`, or
- `plan-eng-review` or `plan-ceo-review` explicitly escalates the plan into design review

### Scope of review

This is a text-first, FeatureForge-native design gate. It should check:

- information architecture / flow shape
- interaction states
- empty, loading, error, and edge states
- responsive behavior
- accessibility expectations
- design-system alignment if such docs exist
- unresolved design decisions that would otherwise be discovered late in QA

### Required output

Produce a runtime-owned design handoff/review artifact bound to the exact plan revision, with:

- verdict: `pass` or `revise`
- findings / design gaps
- required plan changes
- explicit confirmation of state coverage

This artifact must be treated as a trust-boundary object:

- runtime-issued under the authoritative FeatureForge state root
- wrapped in the shared gate-artifact envelope used by new runtime-owned review artifacts
- schema-versioned and provenance-validated
- fingerprint-bound to the reviewed plan revision
- rejected if hand-authored, spoofed, or provenance-mismatched

### Routing

- fail/revise returns control to `writing-plans`
- fresh pass returns control to `plan-eng-review`
- `plan-eng-review` should not complete final approval while a required design review is missing or stale

## 4.4 `security-review`

### Goal

Add a dedicated specialized gate for security-sensitive changes.

### Trigger

This skill activates when the approved signals or review steps indicate security sensitivity, including but not limited to:

- auth/session/token changes
- permission model changes
- secrets handling
- sensitive data processing
- webhook or integration surfaces
- CI/CD or infra/config exposure
- dependency/supply-chain risk
- LLM/AI attack surfaces when relevant

### Stage placement

For this program, `security-review` is a **post-implementation, pre-final-review specialized gate**. It should happen before independent final code review when required, because its findings may still require code or config changes.

### Required output

Produce a runtime-owned security review artifact bound to the relevant diff fingerprint or execution fingerprint, including:

- threat summary
- attack surface
- findings
- severity
- recommended mitigations
- follow-up actions / accepted residual risk

This artifact must be treated as a trust-boundary object:

- runtime-issued under the authoritative FeatureForge state root
- wrapped in the shared gate-artifact envelope used by new runtime-owned review artifacts
- schema-versioned and provenance-validated
- fingerprint-bound to the reviewed execution/diff state
- rejected if hand-authored, spoofed, or provenance-mismatched

### Freshness

If required, the security review must go stale on repo-changing diffs that affect the relevant review fingerprint.

### File touch points for phase 4

- update: `skills/writing-plans/SKILL.md.tmpl`
- update: `skills/plan-eng-review/SKILL.md.tmpl`
- update: `skills/plan-ceo-review/SKILL.md.tmpl`
- update: `skills/using-featureforge/SKILL.md.tmpl`
- update: `skills/document-release/SKILL.md.tmpl`
- update: `skills/qa-only/SKILL.md.tmpl`
- new: `skills/plan-design-review/SKILL.md.tmpl`
- new: generated `skills/plan-design-review/SKILL.md`
- new: `skills/plan-design-review/*` companion refs
- new: `skills/security-review/SKILL.md.tmpl`
- new: generated `skills/security-review/SKILL.md`
- new: `skills/security-review/*` companion refs
- update: `src/contracts/plan.rs`
- update: `src/execution/topology.rs`
- update: `src/execution/harness.rs`
- update: `src/execution/state.rs`
- update: `src/workflow/status.rs`
- update: `src/workflow/operator.rs`
- update: `src/cli/plan_execution.rs`
- update: `src/lib.rs`

### Tests

- plan contract parse/validation tests for `Risk & Gate Signals`
- doc-contract tests ensuring required signal rules exist in planning skills
- routing tests for required `plan-design-review`
- routing/freshness tests for required `security-review`
- trust-boundary tests rejecting spoofed, hand-authored, stale, or provenance-mismatched design/security review artifacts
- legacy-plan compatibility tests
- dashboard tests surfacing required gates based on signals

### Acceptance criteria

- plans carry machine-readable gate signals
- `plan-eng-review` finalizes those signals
- runtime uses them to choose required gates
- `plan-design-review` and `security-review` exist as first-class skills with fresh/stale semantics

---

## Phase 5 — Execution Safety and Worktree Substrate

This phase imports the best operational ideas from gstack without importing gstack’s looser workflow model.

## 5.1 Task-Slice Write Fences

### Goal

Reduce accidental scope creep and make parallel execution safer.

### Design

When a plan task includes a `Files:` block, the runtime should derive allowed write prefixes from it.

### Required behavior

- writes inside the declared slice proceed normally
- writes outside the declared slice are surfaced as out-of-scope
- the system must support an explicit scope-expansion/override path
- task evidence should record whether out-of-slice writes occurred

### Rollout model

To preserve usability and avoid false-positive pain, implement in three phases:

1. **audit mode first**
   - detect and report out-of-slice writes
   - attach warnings to task evidence and dashboard surfaces
   - default duration: 14 days and at least 40 execution runs
2. **guarded enforcement second**
   - enforce in `subagent-driven-development` lanes
   - keep single-lane `executing-plans` in warn mode unless explicitly enabled
   - require override reason capture on blocked-write bypass
   - default duration: next 14 days
3. **full enforcement third**
   - block out-of-slice writes in all runtime-owned execution lanes unless explicitly overridden
   - keep an emergency repo-level disable switch for incident response

Phase-exit criteria:

- audit -> guarded: false-positive rate < 5%, no open P1/P2 fence defects, and >= 2 representative parallel-run validations
- guarded -> full: false-positive rate < 2%, override rate < 10%, and zero unresolved blocker incidents for 7 consecutive days

These rollout thresholds must come from runtime-owned counters and artifacts rather than ad hoc manual reporting. At minimum, the runtime must record and expose:

- fence false-positive count and computed false-positive rate for the current rollout window
- blocked-write override count and computed override rate for the current rollout window
- blocked-lane and `resolution_required` lane counts, including time-to-resolution summaries
- current enforcement mode and the artifact window used for the rollout calculation

The end-state behavior is full enforcement for runtime-launched execution lanes.

## 5.2 Reusable Worktree Manager

### Goal

Give `executing-plans`, `subagent-driven-development`, and related skills a real operational substrate for safe parallel work.

### Required capabilities

A FeatureForge-native worktree manager should support:

- run-scoped worktree creation
- origin SHA recording
- association with execution run / lane identity
- per-lane changed-file manifest
- diff stat recording
- patch harvesting
- harvested patch dedup
- stale-worktree prune
- optional copying of required ignored artifacts when explicitly configured
- evidence attachment back into authoritative task/lane artifacts

Patch-harvest collision or missing-metadata cases must move the affected lane into an explicit `resolution_required` state. In that state, the runtime must block lane completion, merge readiness, and patch reuse until the collision is resolved or an explicit override is recorded with reason and operator-visible evidence.

### Design constraints

- do not inherit gstack state directories or telemetry conventions
- keep the manager internal/runtime-backed, not as a user-facing giant workflow
- preserve current worktree lease and authoritative execution concepts
- favor stable evidence artifacts over ephemeral shell tricks

## 5.3 Default Worktree Path Flip

### Goal

Change the default recommended worktree location so fresh repos do not incur repo churn.

### Required behavior

- if `.worktrees/` or `worktrees/` already exists, continue honoring it
- if repo instructions explicitly require a local path, continue honoring it
- otherwise recommend `~/.config/featureforge/worktrees/<project-name>/`
- local paths remain supported as an explicit opt-in

This change should also be reflected in `using-git-worktrees`.

## 5.4 Skill integration

### `executing-plans`

- use the worktree manager when creating isolated lanes
- attach changed-file manifests and harvested patches to task evidence
- integrate task-slice fence reporting

### `subagent-driven-development`

Require each lane to report:

- changed files
- diff stat
- patch or merge candidate
- unresolved concerns/conflicts
- lane terminal state: `merge_ready` | `resolution_required` | `abandoned`

### `dispatching-parallel-agents`

De-emphasize this skill for serious execution work. The shared runtime-owned worktree substrate should be owned by `subagent-driven-development` and `executing-plans`, while `dispatching-parallel-agents` remains a lighter coordination surface that should not become a second first-class execution topology.

### `using-git-worktrees`

Become the front door to the preferred worktree substrate rather than only a setup guide.

### File touch points for phase 5

- update: `skills/executing-plans/SKILL.md.tmpl`
- update: `skills/subagent-driven-development/SKILL.md.tmpl`
- update: `skills/dispatching-parallel-agents/SKILL.md.tmpl`
- update: `skills/using-git-worktrees/SKILL.md.tmpl`
- update: generated `skills/using-git-worktrees/SKILL.md`
- update: `src/repo_safety/mod.rs`
- update: `src/execution/harness.rs`
- update: `src/execution/state.rs`
- update: `src/execution/topology.rs`
- update: `src/cli/plan_execution.rs`
- new: reusable worktree-management/runtime helper under `src/`
- update: any worktree lease/status surfaces needed for doctor/handoff output

### Tests

- task-fence detection/enforcement tests
- worktree helper tests: create, harvest, dedup, prune
- lane-state tests for `resolution_required` on patch collisions or missing harvest metadata
- lane evidence tests for changed-file manifests/diff stats
- default-path recommendation tests
- routing/integration tests for execution skills

### Acceptance criteria

- safe parallel execution gets easier
- worktree cleanup and patch harvesting become runtime-backed
- default worktree guidance stops causing avoidable repo churn
- task-slice fences reduce accidental cross-scope editing
- patch-harvest collisions or incomplete harvest metadata cannot silently pass as completed lanes

---

## Phase 6 — Finish-Path Reorder and Gate Precedence

### Goal

Fix the late-stage workflow so repo-affecting completion work happens before independent final code review, while fitting the new dynamic gate model.

### End-state gate grouping

After execution has no open steps, the workflow should route through these groups:

1. **pre-final-review completion gates**
   - `document-release`
   - `security-review` when required
2. **independent final code review**
   - `requesting-code-review` with scope-check artifact
3. **verification gates**
   - `qa-only` when required
   - future performance/deploy verification when later added
4. **branch completion**
   - `verification-before-completion`
   - `finishing-a-development-branch`

### Why this order is correct

- repo-affecting completion work should happen before final code review so final review sees the real near-finished state
- security review should happen before final code review when required because it may reopen code/config work
- QA belongs after final review because it is primarily verification, not a repo-writing preparation pass
- this order removes the common case where `document-release` stales final review immediately after it runs

### Required precedence rule

When multiple late-stage gates are unresolved, route in this precedence order:

1. missing/stale required pre-final-review completion gate
2. missing/stale final code review
3. missing/stale verification gate
4. ready for branch completion

### Freshness rules

Freshness must remain strict:

- repo-changing diffs after final code review stale final review
- repo-changing diffs that affect the security-review fingerprint stale security review
- release-doc changes before final review are normal and should not create the old loop
- QA stales when relevant verified surfaces change

### File touch points

- update: `src/workflow/operator.rs`
- update: `src/execution/state.rs`
- update: `src/execution/harness.rs`
- update: `src/execution/topology.rs`
- update: `skills/document-release/SKILL.md.tmpl`
- update: `skills/security-review/SKILL.md.tmpl`
- update: `skills/requesting-code-review/SKILL.md.tmpl`
- update: `skills/qa-only/SKILL.md.tmpl`
- update: `skills/verification-before-completion/SKILL.md.tmpl`
- update: `skills/finishing-a-development-branch/SKILL.md.tmpl`
- update: `README.md`
- update: `docs/README.codex.md`
- update: `docs/README.copilot.md`
- update: `skills/using-featureforge/SKILL.md.tmpl`

### Tests

- workflow phase tests for all gate-precedence combinations
- freshness tests ensuring repo-affecting pre-review gates no longer cause the default stale-final-review loop
- shell-smoke/operator-output tests matching the new order
- dynamic-gate routing tests with required security review + QA combinations

### Acceptance criteria

- public workflow docs show the new order
- repo-affecting late-stage work occurs before final review
- dynamic gates fit into a coherent finish path
- freshness remains strict without needless churn

---

## Phase 7 — Shell-Friendly CLI Contracts and No Interpreter Snippets

### Goal

Stop making generated skills depend on ad hoc scripting runtimes to consume FeatureForge output.

### Why this matters for Codex/Copilot

Inline parsing hacks add friction, hide failure modes, and make skills less portable across environments. Assistants should not need to improvise parsing logic for commands the workflow already owns.

### Required CLI approach

For commands that skills consume heavily, add stable output contracts such as:

- `--field <field-name>`
- `--format shell`
- continue supporting JSON and human-readable output

### Priority command surfaces

At minimum, extend or verify appropriate support for:

- `featureforge plan contract analyze-plan`
- `featureforge plan execution status`
- `featureforge plan execution gate-review`
- `featureforge workflow doctor`
- any new or updated read-only command surfaces used for scope-check and gate routing

### Example shell output

```text
CONTRACT_STATE=valid
DELIVERY_LANE=lightweight_change
PLAN_FIDELITY_RECEIPT_STATE=pass
DESIGN_REVIEW_REQUIRED=no
SECURITY_REVIEW_REQUIRED=yes
BROWSER_QA_REQUIRED=no
```

and:

```text
SCOPE_CHECK_RESULT=DRIFT_DETECTED
SCOPE_CHECK_RESOLUTION_REQUIRED=yes
NEXT_SKILL=featureforge:document-release
```

### Skill-level requirement

Generated skills must not contain active `node -e`, `python`, `python3`, `jq`, `perl`, or `ruby` snippets for parsing FeatureForge-owned command output.

### File touch points

- update: `src/cli/plan_contract.rs`
- update: `src/cli/plan_execution.rs`
- update: `src/cli/workflow.rs`
- update: `src/lib.rs`
- update: any output/render helpers supporting shell-friendly fields
- update: `skills/requesting-code-review/SKILL.md.tmpl`
- update: `skills/using-featureforge/SKILL.md.tmpl`
- update: any other generated skill currently depending on interpreter snippets
- update: `TODOS.md` if this closes outstanding runtime-dependency cleanup items

### Tests

- CLI output-shape tests for `--field` and `--format shell`
- doc-generation tests ensuring no forbidden interpreter snippets exist
- contract tests covering new dynamic-gate/scope-check fields

### Acceptance criteria

- active skills no longer depend on interpreter-based parsing for FeatureForge-owned output
- assistants can branch on FeatureForge state using stable CLI contracts
- shell portability improves materially

---

## Phase 8 — Active Namespace/Path Drift Cleanup

### Goal

Remove stale branding and machine-local path references from active, normative surfaces.

### Required cleanup targets

Must scrub active/normative material such as:

- `AGENTS.md`
- active `README.md`
- `docs/README.codex.md`
- `docs/README.copilot.md`
- active skill docs/templates
- generated examples intended as current guidance

### Must not rewrite blindly

Do not rewrite:

- `docs/**`
- historical evidence where the old path/name is part of the artifact’s truth
- fixtures intentionally modeling historical content

### Path policy

Replace machine-local absolute paths with repo-relative or otherwise portable references in active surfaces.

### File touch points

- update: `AGENTS.md`
- update: `README.md`
- update: `docs/README.codex.md`
- update: `docs/README.copilot.md`
- update: active non-archived docs under `docs/featureforge/`
- update: any generated templates/examples that still emit stale naming/path drift

### Tests

- active-doc scan failing on stale `Superpowers` naming
- active-doc scan failing on stale absolute `/Users/.../superpowers/...` patterns
- allowlist exclusions for archives/history

### Acceptance criteria

- active surfaces identify the repo as FeatureForge
- active docs use portable paths
- archived history remains historical

---

## Phase 9 — Prompt-Surface Reduction and Skill-Doc Compaction

### Goal

Reduce top-level prompt weight without hiding mandatory workflow law.

### Two-layer skill-doc model

1. **Top-level `SKILL.md` = operational contract**
   - trigger conditions
   - required gates/helpers
   - stop/fail-closed rules
   - required outputs/artifacts
   - concise ordered steps

2. **Companion refs = depth**
   - examples
   - rationale
   - section checklists
   - optional patterns
   - long-form explanation

### Guardrails

- do **not** move approval law, stop rules, or mandatory helper invocations out of top-level `SKILL.md`
- do move narrative explanation, examples, and repeated boilerplate out of top-level docs
- top-level docs must remain self-sufficient enough to execute correctly if companion refs are never opened

### High-priority compaction targets

- `skills/using-featureforge/*`
- `skills/plan-ceo-review/*`
- `skills/writing-plans/*`
- `skills/plan-eng-review/*`
- `skills/requesting-code-review/*`
- `skills/finishing-a-development-branch/*`
- `skills/subagent-driven-development/*`
- new `plan-fidelity-review`, `plan-design-review`, and `security-review`

### Shared repeated sections to compact

- upgrade handling
- search-before-building reminders
- contributor-mode reminders
- long repeated examples that are not load-bearing
- repeated release/review boilerplate that can be referenced compactly

### Measurement targets

- materially reduce total generated `skills/*/SKILL.md` line count
- reduce top-level size of the busiest skills by at least one third
- preserve all mandatory phrases/contracts via tests

Skill-compaction progress must also be measured through repo-owned generation/test artifacts so size-budget enforcement is reproducible rather than eyeballed.

### File touch points

- update: `scripts/gen-skill-docs.mjs`
- update: high-volume skills and their companion refs
- add/expand companion docs where long-form material moves

### Tests

- generation tests still pass
- contract tests still find mandatory stop/fail-closed rules in top-level docs
- size-budget tests fail when top-level docs regress significantly
- size-budget reporting must emit the measured baseline and current totals used for regression decisions

### Acceptance criteria

- top-level skills become materially shorter
- required behavioral law remains visible
- assistants get faster access to the contract that matters

---

## Skill-by-Skill Impact Summary

This section is the implementation-facing answer to “what changes, by skill, and why?”

### `using-featureforge`

**Change:** significant improvement  
Add dashboard-style workflow doctor output, dynamic gate visibility, plan-fidelity visibility, scope-check visibility, and clearer next-action summaries.

**Value:** makes the whole system easier to trust and easier for assistants to route correctly.

### `brainstorming`

**Change:** targeted improvement  
Add lightweight-lane behavior and optional bounded-change framing. Keep real specs, but reduce expansion-heavy behavior when the change is clearly small.

**Value:** small fixes stop paying for unnecessary ideation overhead.

### `plan-ceo-review`

**Change:** targeted improvement  
Add lightweight-lane behavior and explicit distribution/publishability checks when new artifacts or user entry points exist.

**Value:** preserves scope discipline while catching “finished but unreachable” ideas early.

### `writing-plans`

**Change:** major improvement  
Add `Delivery Lane`, `Risk & Gate Signals`, and `Release & Distribution Notes`.

**Value:** this is the contract layer that unlocks smarter downstream routing.

### `plan-fidelity-review`

**Change:** new skill  
Make the existing runtime-owned plan-fidelity stage visible, test-enforced, and assistant-friendly.

**Value:** removes hidden-stage ambiguity.

### `plan-eng-review`

**Change:** major improvement  
Validate/finalize gate signals, escalate lightweight plans when needed, route material UI work into `plan-design-review`, and perform stronger operational/distribution checks.

**Value:** makes engineering approval the place where downstream gate requirements become explicit.

### `plan-design-review`

**Change:** new skill  
Text-first design gate for material UI changes.

**Value:** catches UI/UX/design-state problems before implementation and reduces end-stage QA churn.

### `executing-plans`

**Change:** substantial operational improvement  
Integrate task-slice fences, worktree management, patch evidence, and dynamic gate awareness.

**Value:** safer execution with less glue work.

### `subagent-driven-development`

**Change:** substantial operational improvement  
Use the same worktree substrate, lane manifests, patch harvesting, and conflict reporting.

**Value:** parallel execution becomes practical instead of fragile.

### `dispatching-parallel-agents`

**Change:** targeted de-emphasis  
Keep serious execution on `subagent-driven-development` and `executing-plans`; `dispatching-parallel-agents` may coordinate read-only or lightweight parallel work, but it should not become a competing runtime-owned execution substrate.

**Value:** fewer overlapping execution paths and less substrate duplication.

### `using-git-worktrees`

**Change:** major ergonomic improvement  
Flip default path to the global FeatureForge location and reference the new worktree helper.

**Value:** less repo churn, better recovery paths, safer defaults.

### `requesting-code-review`

**Change:** top-tier improvement  
Add the structured scope check and remove interpreter-dependent parsing.

**Value:** closes a real correctness gap with relatively low architectural risk.

### `receiving-code-review`

**Change:** minor but useful improvement  
Batch mechanical low-risk feedback; keep semantic feedback isolated.

**Value:** faster review-response loops without weakening judgment.

### `qa-only`

**Change:** targeted adjustment now, bigger future upside later  
Make requiredness more explicit via gate signals and reflect the dynamic finish path. Browser runtime replatforming is deferred.

**Value:** less unnecessary QA now; cleaner future browser-platform path later.

### `security-review`

**Change:** new skill  
Specialized post-implementation security gate driven by plan signals.

**Value:** fills a real missing capability in the current workflow.

### `document-release`

**Change:** major improvement  
Move it earlier in the finish path, add release-surface classification, distribution checks, and versioning decisions.

**Value:** cleaner finish flow, better release readiness, fewer stale-review loops.

### `verification-before-completion`

**Change:** targeted improvement  
Reflect dynamic required gates and scope-check state.

**Value:** finish gating becomes more accurate and explainable.

### `finishing-a-development-branch`

**Change:** significant improvement  
Show dashboard-style readiness state and honor the new late-stage ordering.

**Value:** fewer late mistakes and clearer operator choices.

### `systematic-debugging`

**Change:** minor but useful improvement  
Add a standard debug-report output.

**Value:** better handoff and knowledge retention.

### `test-driven-development`

**Change:** no meaningful workflow change  
Keep as-is.

**Value:** none needed beyond normal maintenance.

### `writing-skills`

**Change:** indirect effect only  
Adopt the prompt-compaction conventions and any generator/contract changes needed for the new skill surfaces.

**Value:** keeps the skill authoring system aligned with the smaller prompt model.

---

## Data and Contract Details

## Spec contract changes

New or materially revised specs must include:

```md
**Delivery Lane:** standard | lightweight_change
```

Specs may also include a short bounded-change statement when using the lightweight lane.

## Plan contract changes

New or materially revised plans must include:

1. `Delivery Lane`
2. `Risk & Gate Signals`
3. `Release & Distribution Notes`

### Contract versioning

- bump the plan/spec contract versions appropriately
- preserve parsing support for previous versions
- mark legacy/no-signal plans clearly in diagnostics
- require the new fields for newly created or materially revised artifacts only
- do not retroactively require newly introduced gate fields, artifact trust checks, or downstream gate obligations for already approved artifacts unless those artifacts are materially revised or execution is explicitly reopened under the new contract version

## Artifact identity requirements

The runtime must bind review artifacts to the right identity:

- `plan-fidelity-review` -> plan revision + approved spec
- `plan-design-review` -> plan revision
- `security-review` -> execution/diff fingerprint
- scope-check -> final review fingerprint / diff identity
- document-release -> release-doc artifact freshness as already modeled, extended with versioning/distribution fields

For every artifact that can satisfy or unblock a workflow gate, identity alone is insufficient. The runtime must also validate issuer/provenance, schema version, storage location under the authoritative state root, and fingerprint match before treating the artifact as gate-satisfying truth.

All new runtime-owned gate-satisfying artifacts in this program should share a common envelope contract even when their payloads differ. That envelope should include, at minimum:

- artifact kind
- schema version
- artifact version or revision binding
- issuer/provenance block
- created-at / updated-at timestamps
- fingerprint binding for the reviewed artifact or execution state
- retention/cleanup policy metadata or policy reference

Per-artifact payload fields may differ, but the shared envelope must stay consistent so trust checks, freshness evaluation, retention, and operator rendering do not fragment into one-off formats.

---

## Routing and Runtime Semantics

## Gate recommendation/preflight

`execution recommend` / `execution preflight` should consume approved signals and show:

- delivery lane
- required planning receipts already satisfied or missing
- required downstream gates after execution
- warnings about high deploy/distribution/migration impact

## Doctor/handoff

`workflow doctor` and `workflow handoff` should expose:

- route state
- lane
- approved signals
- required gate set
- freshness of each gate
- scope-check result when available
- explicit next safe step

All read-only operator surfaces for this data must derive from the same shared runtime-owned operator snapshot so route selection, dashboard rendering, handoff text, JSON output, and shell fields cannot drift independently.

## Fail-closed rules

The runtime must fail closed when:

- a required plan-fidelity receipt is missing
- required `Risk & Gate Signals` are missing under the new contract version
- required `Risk & Gate Signals` cannot be derived or validated during plan approval under the new contract version
- a required gate-satisfying artifact is spoofed, hand-authored, provenance-mismatched, schema-invalid, or stored outside the authoritative runtime-owned artifact root
- a required design/security/release/review artifact is missing or stale
- a scope-check reports `REQUIREMENTS_MISSING`
- finish is attempted while required gate freshness is unresolved

---

## Verification Strategy

### Skill/doc generation

```bash
node scripts/gen-skill-docs.mjs
node scripts/gen-agent-docs.mjs
node --test tests/codex-runtime/gen-skill-docs.unit.test.mjs tests/codex-runtime/skill-doc-contracts.test.mjs tests/codex-runtime/skill-doc-generation.test.mjs
```

### Rust workflow/runtime coverage

```bash
cargo nextest run --test contracts_spec_plan --test runtime_instruction_contracts --test runtime_instruction_plan_review_contracts --test using_featureforge_skill --test workflow_runtime --test workflow_runtime_final_review --test workflow_shell_smoke --test plan_execution --test plan_execution_final_review
cargo clippy --all-targets --all-features -- -D warnings
```

When a phase changes topology recommendation, lane/worktree state, lease behavior, blocked-lane handling, or execution/final-review freshness semantics, the verification set must also include:

```bash
cargo nextest run --test plan_execution_topology --test contracts_execution_leases --test execution_harness_state
```

### Additional targeted checks required by this program

- plan-fidelity skill presence tests
- lightweight-lane parse and escalation tests
- scope-check classification and policy tests
- distribution/versioning rule tests
- dashboard field coverage tests
- dynamic gate routing/freshness tests
- plan-design-review and security-review presence + routing tests
- task-fence and worktree-manager tests
- active-doc stale-name/path scans
- generated-skill forbidden-interpreter scan
- size-budget tests for top-level skill compaction

---

## Rollout and Compatibility Strategy

### Recommended rollout mode

1. land contract/parser support
2. land the new/updated skill surfaces
3. land dashboard/operator visibility
4. land routing and freshness logic
5. land audit-mode safety features such as task-slice fences
6. turn on stricter enforcement once evidence confirms signal quality and false-positive rates are acceptable

### Legacy artifact handling

- legacy plans/specs remain readable
- new rules become mandatory for newly created or materially revised artifacts
- the dashboard should clearly say when a plan is legacy and therefore missing dynamic gate metadata
- already approved in-flight plans continue under the rules of their recorded contract version unless they are materially revised or explicitly reopened, so runtime upgrades do not strand valid work midstream

### Why staged rollout matters

This program changes both human-facing skill docs and runtime-owned workflow logic. Rolling everything out in one hard cut would create avoidable confusion.

---

## Risks and Mitigations

- **Risk:** lightweight lane becomes a hidden bypass lane.  
  **Mitigation:** keep approvals, plan-fidelity review, and explicit contract markers mandatory.

- **Risk:** scope-drift logic becomes noisy or subjective.  
  **Mitigation:** keep the classification simple, require explicit reviewer resolution, and test representative fixtures.

- **Risk:** plan signals become busywork.  
  **Mitigation:** keep them compact, enum-based, and routed through `plan-eng-review` rather than scattering them across many stages.

- **Risk:** new gates create more process instead of less.  
  **Mitigation:** dynamic gates should replace irrelevant default work, not just add more boxes. Signal-driven routing must reduce false-positive downstream steps.

- **Risk:** task-slice fences block legitimate implementation work too early.  
  **Mitigation:** start in audit mode and preserve explicit override paths.

- **Risk:** worktree manager becomes a second workflow system.  
  **Mitigation:** keep it as runtime infrastructure under the existing execution model, not a new user-facing paradigm.

- **Risk:** prompt compaction drops a mandatory rule.  
  **Mitigation:** use contract tests for required top-level phrases and stop rules.

- **Risk:** namespace/path cleanup rewrites history.  
  **Mitigation:** scope cleanup to active surfaces and explicitly exclude archives/history.

- **Risk:** contract changes break old plans.  
  **Mitigation:** version contracts and preserve legacy parsing.

---

## Deferred Follow-On Work

This program intentionally stops before the browser-platform work, but it prepares for it.

### Important follow-on after this program

- first-class FeatureForge browser runtime
- browser-backed `qa-only`
- `browse`
- `connect-chrome`
- `setup-browser-cookies`
- fix-capable `qa`
- post-implementation `design-review`
- performance review / benchmark
- deploy readiness and land/deploy lifecycle

The `Risk & Gate Signals` model in this spec should be designed so those later capabilities plug into the same runtime gate architecture cleanly.

---

## Final Acceptance Summary

This merged program is complete when all of the following are true:

- FeatureForge has a visible, checked-in, enforced `plan-fidelity-review`
- bounded bugfixes and small scoped work have a legitimate lightweight lane
- final review includes a structured scope check
- publishability and versioning are explicit instead of implied
- plans carry machine-readable gate signals and the runtime uses them
- material UI work can require `plan-design-review`
- security-sensitive work can require `security-review`
- execution/subagent flows have safer write boundaries and better worktree ergonomics
- the finish path runs repo-affecting work before final review
- active skills no longer depend on interpreter snippets for FeatureForge output
- active docs stop teaching stale names and paths
- top-level skill docs are materially smaller and easier for assistants to use

That combination preserves FeatureForge’s strongest properties while making it much easier to use for the thing it is supposed to optimize: delivering software features and fixes quickly and safely with Codex and Copilot.

---

## Planning Readiness Addendum (2026-03-28)

This addendum tightens the spec for immediate `writing-plans` handoff while keeping scope unchanged.

### Gate A Checklist Mapping

- clear problem statement and desired outcome: covered in `Executive Summary` and `Product Goal and North Star`
- clear scope boundaries: covered in `Scope` and `Explicit gstack Imports and Explicit Non-Imports`
- key interfaces, constraints, and dependencies: covered in `Requirement Index`, `Data and Contract Details`, and `Routing and Runtime Semantics`
- explicit failure-mode thinking: covered in `Risks and Mitigations` plus the registries below
- observability expectations: covered in `3.4 Dashboard-Style workflow doctor`, `Risk & Gate Signals`, and runtime-semantics sections
- rollout and rollback expectations: covered in `Rollout and Compatibility Strategy` and `Fail-closed rules`
- credible risks: covered in `Risks and Mitigations`
- testable acceptance criteria: covered in phase-level acceptance criteria plus `Final Acceptance Summary`

### Resolved Decisions For `writing-plans`

1. `lightweight_change` uses explicit v1 qualification caps: max 8 non-generated files, <= 400 net changed lines, low-only deploy/distribution/migration impact, and hard escalation when security review is required.
2. `security-review` uses explicit trust-boundary trigger rules; hard security surfaces trigger review, while docs/tests/refactors remain out of scope when controls are unchanged.
3. task-slice fences roll out in three phases (`audit` -> `guarded enforcement` -> `full enforcement`) with measured exit criteria and an incident-response kill switch.

## What already exists

- runtime-owned workflow routing, contracts, and fail-closed checks
- plan/spec artifacts and approval headers
- execution preflight and downstream-gate surfaces
- protected-branch write controls
- plan-fidelity receipts in runtime logic
- baseline `workflow doctor`, `gate-review`, and `gate-finish` surfaces

## Dream state delta

- from static-ish downstream defaults to signal-driven dynamic gates
- from implied review steps to fully explicit first-class stages
- from shell/parsing friction to stable CLI field contracts
- from manual parallel-execution plumbing to managed fences/worktrees/patch harvesting
- from long repetitive skill docs to compact contract-first skill surfaces

## NOT in scope

- browser-runtime implementation (`browse`, `connect-chrome`, cookie setup, browser-backed QA)
- post-implementation fix-capable QA workflow
- deploy/land lifecycle orchestration beyond the routing hooks specified here
- telemetry/state-model adoption from gstack outside explicitly listed imports

## Error & Rescue Registry

| Codepath | Failure class | Rescued? | Rescue action | User sees | Logged |
|---|---|---|---|---|---|
| plan contract parse | `ContractParseError` | Y | fail closed and route back to plan stage with explicit fix guidance | explicit blocking message | Y |
| lane derivation | `LaneClassificationError` | Y | default to `standard` lane and require explicit reviewer confirmation | warning + fallback lane | Y |
| required signal missing | `MissingRiskGateSignals` | Y | block preflight and route to `plan-eng-review` | explicit blocking message | Y |
| signal derivation / validation at plan approval | `RiskGateSignalDerivationError` | Y | fail closed, reject plan approval, and return to the owning planning gate with the broken field named | explicit blocking message | Y |
| gate freshness check | `StaleGateArtifact` | Y | invalidate stale receipt and route to owning gate skill | explicit stale-artifact notice | Y |
| scope check classification | `ScopeCheckExecutionError` | Y | mark scope-check unresolved and block finish | explicit unresolved scope-check state | Y |
| downstream security gate consumption | `SecurityTriggerEvaluationError` | Y | preserve approved signal truth, route conservatively to `security-review`, and mark the snapshot degraded | warning + conservative route | Y |
| downstream design gate consumption | `DesignTriggerEvaluationError` | Y | preserve approved signal truth, route conservatively to `plan-design-review`, and mark the snapshot degraded | warning + conservative route | Y |
| worktree manager allocate | `WorktreeAllocationError` | Y | keep execution in current workspace and disable parallel lane | warning + degraded mode | Y |
| worktree patch harvest | `WorktreePatchResolutionRequired` | Y | mark lane `resolution_required`, block completion/merge-reuse, and require explicit resolution evidence or override | explicit lane-blocked notice | Y |
| task-slice fence violation | `TaskSliceFenceViolation` | Y | block write, require override or reassignment | explicit write-denied message | Y |
| shell output contract read | `FieldOutputContractError` | Y | stop automation branch and show exact missing field | explicit contract-field error | Y |

## Failure Modes Registry

```text
CODEPATH                                   | FAILURE MODE                                  | RESCUED? | TEST? | USER SEES?              | LOGGED?
-------------------------------------------|-----------------------------------------------|----------|-------|-------------------------|--------
plan-fidelity-review stage resolution      | checked-in skill missing                      | Y        | Y     | hard block              | Y
lightweight lane routing                   | invalid lane or ambiguous eligibility         | Y        | Y     | fallback to standard    | Y
requesting-code-review scope check         | drift detected but unresolved                 | Y        | Y     | explicit decision gate  | Y
execution recommend/preflight              | missing required plan signals                 | Y        | Y     | hard block              | Y
plan approval signal derivation            | required gate signal cannot be derived        | Y        | Y     | hard block              | Y
downstream gate selection                  | approved signal snapshot read/render error    | Y        | Y     | conservative extra gate | Y
document-release freshness                 | release artifact stale after new commits      | Y        | Y     | stale warning + reroute | Y
security-review artifact freshness         | fingerprint mismatch                          | Y        | Y     | stale warning + reroute | Y
worktree patch harvest                     | patch collision or missing patch metadata     | Y        | Y     | lane blocked pending resolution | Y
task-slice fence enforcement               | unauthorized cross-slice write                | Y        | Y     | write denied            | Y
workflow doctor dashboard render           | partial signal set available                  | Y        | Y     | degraded dashboard note | Y
```

## ASCII Diagrams

### System Architecture

```text
Spec/Plan Artifacts
        |
        v
FeatureForge Runtime -----> Workflow Doctor / Handoff
        |
        +--> Planning Gates (fidelity, eng, design)
        |
        +--> Execution Engine (single or parallel via worktrees/fences)
        |
        +--> Finish Gates (release, security, scope, QA, verify, finish)
```

### Data Flow (with shadow paths)

```text
Approved Spec
   -> Writing Plans
      -> Approved Plan (+ Risk & Gate Signals)
         -> Execution Recommend/Preflight
            -> Required Gate Set
               -> Execute
                  -> Scope Check + Gate Freshness
                     -> Finish

Shadow paths:
1) nil/absent signals     -> fail closed, route to plan fix
2) empty/stale artifact   -> freshness invalidation + reroute
3) upstream command error -> conservative gate routing + explicit warning
```

### State Machine

```text
Draft Spec
  -> CEO Reviewed Draft
  -> Plan Draft
  -> Plan Fidelity Pass
  -> Eng Approved Plan
  -> Implementation Ready
  -> Executing
  -> Pre-Final Gates
  -> Final Review + Scope Check
  -> Verification
  -> Branch Finish
```

### Deployment Sequence

```text
1) land contract parsing/versioning support
2) land checked-in skills + doc generation updates
3) land runtime routing + doctor visibility
4) land safety substrate (fences/worktree manager)
5) enable stricter enforcement after signal-quality burn-in
```

### Rollback Flowchart

```text
Regression detected
   -> identify failing phase (contract | routing | safety)
   -> disable strict enforcement flag for that phase
   -> keep fail-closed checks for data-integrity/security cases
   -> revert to prior stable routing behavior
   -> patch + retest + re-enable phased enforcement
```

## CEO Review Summary

**Review Status:** clear  
**Reviewed At:** 2026-03-28T23:14:44Z  
**Review Mode:** hold_scope  
**Reviewed Spec Revision:** 1  
**Critical Gaps:** 0  
**UI Design Intent Required:** no  
**Outside Voice:** skipped
