# Shell/Field Output Contracts For Runtime-Owned CLI Commands

**Workflow State:** CEO Approved
**Spec Revision:** 1
**Last Reviewed By:** plan-ceo-review
**Date:** 2026-04-08

## Problem Statement

FeatureForge runtime commands already own authoritative workflow and contract truth, but active skills still parse runtime JSON with external interpreters (`node -e`, `jq`, `python`, `perl`, `ruby`) to extract scalars for shell control flow.

That creates avoidable fragility:

- extra runtime dependencies in skills
- parser boilerplate duplicated across generated docs
- weaker shell ergonomics for command chaining
- drift risk between runtime-owned schemas and skill-side parser snippets

## Desired Outcome

FeatureForge CLI directly exposes shell-consumable values for runtime-owned commands used by active skills, while preserving JSON for structured integrations and backward compatibility.

## Decision

Adopt one additive output contract pattern for selected runtime-owned commands:

- `--field <field-name>` for one scalar
- `--format shell` for compact multi-field shell assignments
- `--format json` (plus legacy `--json` where it already exists)

## Dependency

This spec depends on:

- `2026-04-01-workflow-public-phase-contract.md`
- `2026-04-01-gate-diagnostics-and-runtime-semantics.md`
- `2026-04-01-execution-review-skill-contract-hardening.md`

## Requirement Index

- [REQ-001][behavior] `featureforge plan contract analyze-plan` MUST support `--field` and `--format shell`.
- [REQ-002][behavior] `featureforge plan execution status` MUST support `--field` and `--format shell`.
- [REQ-003][behavior] `featureforge plan execution gate-review` and `record-review-dispatch` MUST support `--field` and `--format shell`.
- [REQ-004][behavior] `featureforge workflow operator` MUST support `--field` and `--format shell`.
- [REQ-005][behavior] `featureforge workflow doctor` SHOULD support `--field` and `--format shell` in v1; if deferred, it MUST ship in the next command-surface follow-up.
- [REQ-006][behavior] `--field` MUST return exactly one scalar value plus trailing newline.
- [REQ-007][behavior] `--format shell` MUST emit valid POSIX shell assignment lines with stable keys.
- [REQ-008][behavior] JSON output MUST remain supported for all commands in scope.
- [REQ-009][behavior] Commands that already expose `--json` MUST preserve compatibility while converging on shared format behavior.
- [REQ-010][behavior] Shell-mode key names and output ordering MUST be treated as versioned contract surface and pinned by tests.
- [VERIFY-001][verification] Generated skill contracts MUST fail if active skills reintroduce external interpreter parsing for FeatureForge-owned command extraction.
- [VERIFY-002][verification] CLI output-shape tests MUST pin field names, scalar semantics, and shell-key mappings.
- [VERIFY-003][verification] Shell output tests MUST prove safe quoting and correct behavior for empty values, apostrophes, and path values containing spaces.

## Scope

In scope:

- additive output contracts for commands listed in this spec
- stable `--field` names and shell-key mappings
- shell-safe scalar rendering rules
- active-skill migration off interpreter parsing for FeatureForge-owned commands
- regression tests that keep generated skills interpreter-free for those parsing paths

Out of scope:

- removing JSON support
- introducing a generic query language
- exposing every nested JSON field in v1
- redesigning workflow command semantics unrelated to output contracts

## What Already Exists

- JSON contracts already exist for these runtime commands.
- `workflow operator`, `workflow doctor`, and plan-execution command families already have extensive shell-smoke test coverage.
- FeatureForge already demonstrates direct shell scalar output (`repo runtime-root --path`), establishing a precedent for runtime-owned shell-friendly extraction.
- `TODOS.md` already calls for avoiding new runtime deps in generated skill/runtime flows and explicitly prefers runtime-owned scalar/field output over interpreter parsing.

## Dream State Delta

```text
CURRENT STATE                         THIS SPEC DELTA                             12-MONTH IDEAL
JSON-first commands + skill parsers   Add runtime-owned --field/--format shell    Skills consume runtime output directly
in node/python/jq/perl/ruby           for high-use runtime-owned commands          with no external parser snippets

Per-command output ergonomics drift    One shared shell/field contract pattern      Unified command surface + pinned contracts

Parser regressions caught late         Contract tests fail on parser reintroduction  Zero parser-regression churn in active skills
```

## Architecture Contract

### System Architecture Diagram

```text
+------------------------+      +------------------------+      +----------------------+
| FeatureForge CLI Args  | ---> | Command Output Adapter | ---> | Renderer             |
| --field / --format     |      | (per command family)   |      | json | field | shell |
+------------------------+      +------------------------+      +----------------------+
                                         |                                  |
                                         v                                  v
                               +---------------------+            +----------------------+
                               | Runtime Query Model |            | Stdout + Exit Codes |
                               +---------------------+            +----------------------+
```

Contract boundary:

- runtime query model remains source of truth
- adapter maps query fields to field/shell contracts
- renderer owns scalar and shell escaping behavior

### Data Flow Diagram (Happy + Shadow Paths)

```text
INPUT FLAGS
  |
  +--> valid field + field exists -----------> scalar render --------> stdout + exit 0
  |
  +--> valid format=shell -------------------> shell render ---------> stdout + exit 0
  |
  +--> unknown field ------------------------> contract error --------> stderr + non-zero
  |
  +--> command execution error --------------> existing error path ---> stderr + non-zero
```

### Output Mode State Machine

```text
[parse args]
    |
    +--> [json mode]   -> emit json contract
    +--> [field mode]  -> emit one scalar
    +--> [shell mode]  -> emit assignments
    +--> [invalid arg combination] -> clap/usage failure
```

### Error Flow Diagram

```text
runtime query failure -> command returns existing error envelope + non-zero
unsupported field    -> error: unsupported field '<field>' for <command> + non-zero
null/empty value     -> empty scalar or KEY='' + exit 0
rendering violation  -> fail closed + non-zero (tested invariant)
```

## Shared Output Contract

For each command in this spec:

```bash
--field <field-name>
--format shell
--format json
```

### Argument behavior

- `--field` and `--format` are mutually exclusive.
- For commands with legacy `--json`, `--json` remains accepted as compatibility input for JSON output.
- `--json` and `--format` are mutually exclusive.
- Unknown fields MUST fail non-zero with a command-specific error.

### Scalar contract (`--field`)

`--field` emits exactly one scalar value followed by `\n`.

Scalar rules:

- null or missing value -> empty line, exit 0
- boolean -> `true` or `false`
- integer -> base-10 digits
- enum/string -> canonical runtime machine string
- reason-code lists -> comma-delimited scalar with no surrounding spaces

### Shell contract (`--format shell`)

`--format shell` emits one assignment per line:

```bash
KEY='value'
```

Rules:

- keys are uppercase snake case
- values are single-quoted and POSIX-shell escaped
- output order is stable and pinned per command
- only the command's declared v1 shell keys are emitted
- null/empty values emit as `KEY=''`

Escaping rules:

- apostrophes MUST be escaped for single-quoted POSIX context
- line breaks MUST be normalized to escaped single-line representation

Trust-boundary rule:

```bash
eval "$(featureforge <command> --format shell)"
```

This is acceptable only when the emitter is trusted FeatureForge runtime.

## Field Naming Conventions

- canonical field names are `snake_case`
- optional `kebab-case` aliases MAY be accepted for compatibility, but docs/help MUST present canonical `snake_case`
- shell keys are uppercase snake case

Mapping rule:

- field `contract_state` -> shell key `CONTRACT_STATE`

## Command Contracts

## 1) `featureforge plan contract analyze-plan`

Required fields:

- `contract_state`
- `task_count`
- `packet_buildable_tasks`
- `workflow_state`
- `plan_revision`
- `source_spec_revision`
- `qa_requirement`
- `plan_fidelity_receipt_state`

Shell keys (stable order):

- `CONTRACT_STATE`
- `TASK_COUNT`
- `PACKET_BUILDABLE_TASKS`
- `WORKFLOW_STATE`
- `PLAN_REVISION`
- `SOURCE_SPEC_REVISION`
- `QA_REQUIREMENT`
- `PLAN_FIDELITY_RECEIPT_STATE`

## 2) `featureforge plan execution status`

Required fields:

- `execution_started`
- `execution_mode`
- `active_task`
- `blocking_task`
- `resume_task`
- `evidence_path`
- `reason_codes`

Shell keys (stable order):

- `EXECUTION_STARTED`
- `EXECUTION_MODE`
- `ACTIVE_TASK`
- `BLOCKING_TASK`
- `RESUME_TASK`
- `EVIDENCE_PATH`
- `REASON_CODES`

`execution_started` scalar values MUST be `yes` or `no` for compatibility with existing skill logic.

## 3) `featureforge plan execution gate-review`

Required fields:

- `allowed`
- `failure_class`
- `reason_codes`

Shell keys:

- `ALLOWED`
- `FAILURE_CLASS`
- `REASON_CODES`

`allowed` MUST be `true` on pass and `false` on blocked/fail outcomes.

## 4) `featureforge plan execution record-review-dispatch`

Required fields:

- `allowed`
- `failure_class`
- `dispatch_id`
- `reason_codes`

Shell keys:

- `ALLOWED`
- `FAILURE_CLASS`
- `DISPATCH_ID`
- `REASON_CODES`

Blocked action behavior:

- `dispatch_id` MUST emit empty scalar / `DISPATCH_ID=''`

## 5) `featureforge workflow operator`

Required fields:

- `phase`
- `phase_detail`
- `next_skill`
- `next_action`
- `reason`
- `contract_state`

Shell keys:

- `PHASE`
- `PHASE_DETAIL`
- `NEXT_SKILL`
- `NEXT_ACTION`
- `REASON`
- `CONTRACT_STATE`

## 6) `featureforge workflow doctor` (recommended v1, required follow-up if deferred)

Target fields:

- `phase`
- `route_status`
- `next_skill`
- `next_action`
- `next_step`
- `spec_path`
- `plan_path`
- `contract_state`

Shell keys:

- `PHASE`
- `ROUTE_STATUS`
- `NEXT_SKILL`
- `NEXT_ACTION`
- `NEXT_STEP`
- `SPEC_PATH`
- `PLAN_PATH`
- `CONTRACT_STATE`

If deferred from first implementation slice, tracking tests MUST preserve these exact field names for the follow-up.

## Error Behavior

### Unsupported field

```text
error: unsupported field '<field>' for <command>
```

Exit non-zero.

### Empty value

Return empty line for `--field` and `KEY=''` for shell output, exit 0.

### Command failure

Preserve existing non-zero semantics and existing error payload behavior.

## Security & Threat Model

Threats and mitigations:

- Threat: shell injection via unescaped values in `--format shell`.
  Likelihood: medium if escaping is wrong.
  Impact: high.
  Mitigation: single-quoted escaping contract + fuzz and fixture tests with apostrophes/newlines.
- Threat: unsafe trust transitivity when users blindly `eval` untrusted binaries.
  Likelihood: low in normal use, non-zero in compromised path scenarios.
  Impact: high.
  Mitigation: explicit trust-boundary rule in docs/skills; use only trusted FeatureForge binary.
- Threat: contract drift between JSON and field/shell mappings.
  Likelihood: medium.
  Impact: medium.
  Mitigation: pinned mapping tests and generated-skill contract tests.
- Threat: accidental exposure of sensitive unexpected fields through shell mode.
  Likelihood: low.
  Impact: medium.
  Mitigation: emit only declared v1 shell keys per command.

## Error & Rescue Registry

```text
METHOD/CODEPATH                         | WHAT CAN GO WRONG                           | EXCEPTION / FAILURE CLASS          | RESCUED? | RESCUE ACTION                                      | USER SEES
----------------------------------------|---------------------------------------------|------------------------------------|----------|----------------------------------------------------|-----------------------------
CLI arg parsing                          | invalid combo (`--json` + `--format`)       | clap usage failure                 | Y        | CLI parse fail with usage                          | clear usage error
field mapper                             | unknown field                               | unsupported_field                  | Y        | command-specific error + non-zero                  | explicit unsupported field
shell renderer                           | apostrophe/newline escaping regression       | render_contract_violation          | N        | fail closed; test catches before release           | non-zero command failure
runtime query                            | underlying runtime command/query failure     | existing runtime error             | Y        | preserve existing error envelope + non-zero        | existing runtime diagnostics
skill contract generation/tests          | parser snippets reintroduced in templates    | contract_test_failure              | Y        | fail CI; block merge                               | CI failure with failing test
```

## Failure Modes Registry

```text
CODEPATH                                 | FAILURE MODE                                 | RESCUED? | TEST? | USER SEES?                 | LOGGED?
-----------------------------------------|----------------------------------------------|----------|-------|----------------------------|--------
plan contract analyze-plan --field        | unsupported field                             | Y        | Y     | explicit field error       | Y
plan execution status --format shell      | value contains apostrophe/newline             | Y        | Y     | correct escaped shell line | Y
workflow operator --field                 | null scalar field                             | Y        | Y     | empty line                 | N
record-review-dispatch --format shell     | blocked action with empty dispatch id         | Y        | Y     | DISPATCH_ID=''             | Y
generated skill contract tests            | forbidden parser snippet in active skills     | Y        | Y     | CI/test failure            | Y
```

No row intentionally allows `RESCUED=N` + `TEST=N` + silent user impact.

## Observability & Debuggability

This spec introduces command-surface contract behavior and requires observability at contract-test boundaries:

- contract tests must log exact command/field pair on failure
- shell-render tests must include fixture names showing escaping case under test
- generated-skill tests must report exact file and snippet class (`node -e`, `python`, `jq`, etc.)
- migration PRs should include a before/after skill snippet in PR description for auditability

## Deployment & Rollout

### Deployment sequence diagram

```text
1) Land additive CLI flags and renderers behind tests
2) Land command-level field/shell tests
3) Migrate requesting-code-review template + regenerate skills
4) Land skill-doc parser-regression enforcement tests
5) Remove no functionality; monitor CI and dogfood in active skills
```

### Rollback flowchart

```text
contract test failures after migration?
  |
  +-- yes --> revert skill template changes only --> keep CLI additive support
  |
  +-- no  --> continue rollout

runtime regressions in CLI rendering?
  |
  +-- yes --> disable/rollback new renderer path in follow-up patch (JSON path unchanged)
  |
  +-- no  --> keep v1 contract active
```

Rollout expectations:

- additive first: no removal of JSON pathways in v1
- migrate one active skill first (`requesting-code-review`) before broader rollout
- keep command behavior fail-closed on unsupported fields

## Migration Plan

### Phase 1: Additive CLI support

- implement `--field` and `--format shell` for in-scope commands
- preserve JSON behavior and compatibility aliases

### Phase 2: Active skill migration

- update `skills/requesting-code-review/SKILL.md.tmpl` first
- regenerate skill docs via `node scripts/gen-skill-docs.mjs`

### Phase 3: Contract enforcement

- extend skill-doc contract tests to fail on banned parser snippets in active generated skills when used for FeatureForge-owned command parsing

## Implementation Touch Points

Primary likely surfaces:

- `src/cli/plan_contract.rs`
- `src/cli/plan_execution.rs`
- `src/cli/workflow.rs`
- shared output rendering helpers under `src/cli/`
- `skills/requesting-code-review/SKILL.md.tmpl`
- generated `skills/requesting-code-review/SKILL.md`
- skill-doc contract tests under `tests/codex-runtime/`

## Verification Plan

### CLI contract tests

- `--field` returns expected scalar for each declared field
- unsupported field fails non-zero with expected error text
- `--format shell` emits expected keys in fixed order
- shell quoting is valid for values containing spaces and apostrophes
- empty/null scalar behavior matches contract

### Test coverage diagram

```text
NEW COMMAND SURFACE:
  - analyze-plan field/shell mode
  - execution status field/shell mode
  - gate-review field/shell mode
  - record-review-dispatch field/shell mode
  - workflow operator field/shell mode
  - workflow doctor field/shell mode (or tracked defer)

NEW ERROR PATHS:
  - unsupported field
  - invalid argument combination
  - shell escaping fixtures

NEW DOC CONTRACT PATHS:
  - requesting-code-review template migration
  - forbidden parser snippet regression checks
```

### Skill contract tests

For active generated skills, fail if FeatureForge command-output parsing reintroduces:

- `node -e`
- `python`
- `python3`
- `jq`
- `perl`
- `ruby`

### Regression tests

- `requesting-code-review` examples consume `--field` or `--format shell`
- `analyze-plan` is no longer JSON-only at CLI surface
- workflow/operator basic routing fields no longer require parser snippets

## Risks & Mitigations

- Risk: field-name churn breaks shell consumers.
  Mitigation: pinned contract tests and explicit stability rule ([SFO-010]).
- Risk: incomplete skill migration leaves mixed parser patterns.
  Mitigation: phase migration + parser-regression tests before completion.
- Risk: mismatch between JSON and field/shell values.
  Mitigation: cross-mode parity tests for each exposed field.

## Acceptance Criteria

- active skills stop using external interpreters to parse FeatureForge-owned output for covered commands
- `plan contract analyze-plan` supports shell and scalar output contracts
- in-scope runtime commands share one consistent field/shell pattern
- output-shape and generated-skill tests fail closed on contract regressions
- rollout preserves JSON compatibility in v1

## NOT In Scope

- replacing all command families with field/shell output in one slice
- deprecating/removing legacy `--json` flags in this slice
- introducing nested query expressions or JSONPath-like selectors
- converting non-runtime-owned third-party command parsing patterns

## CEO Review Summary

**Review Status:** clear
**Reviewed At:** 2026-04-08T18:08:57Z
**Review Mode:** hold_scope
**Reviewed Spec Revision:** 1
**Critical Gaps:** 0
**UI Design Intent Required:** no
**Outside Voice:** skipped
