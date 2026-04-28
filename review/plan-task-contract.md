# Approved Task Contract

This document is the shared source of truth for approved plan tasks, task
packets, plan review, task review, and final code review. Other runtime,
skill, prompt, schema, and reviewer surfaces must point back here when they
enforce task-contract law.

## Canonical Task Shape

Approved implementation plans use this task body, in this order:

```markdown
## Task N: [Title]

**Spec Coverage:** REQ-..., DEC-..., VERIFY-...
**Goal:** [One sentence describing the exact outcome this task produces]

**Context:**
- [Why this task exists in the plan]
- [Repo or architecture fact the implementer/reviewer must know]
- [Spec, decision, or non-goal reference when required by the triggers below]

**Constraints:**
- [Hard rule inherited from the spec, architecture, sequencing, or review]
- [Hard rule about reuse, scope, boundary, migration, or compatibility]

**Done when:**
- [Atomic, binary completion condition]
- [Atomic, binary completion condition]

**Files:**
- Modify: `path`
- Create: `path`
- Test: `path`

- [ ] **Step 1: Optional execution aid**
```

`Spec Coverage` and `Files` remain required traceability fields. `Goal`,
`Context`, `Constraints`, and `Done when` are required task-contract fields.
The legacy fields `Task Outcome`, `Plan Constraints`, and `Open Questions` are
not part of the final approved task body. Draft questions, when useful during
authoring, belong in draft notes or pre-approval authoring state outside the
approved task body.

Legacy task bodies migrate as follows:

- `Task Outcome` is split into the new `Goal` plus concrete `Done when`
  obligations. The goal names the outcome; the obligations define the
  reviewable terminal conditions.
- `Plan Constraints` becomes `Constraints`, preserving only hard rules that
  must bind implementation and review.
- `Open Questions` is removed from approved task bodies. Unresolved questions
  must be resolved before approval or kept outside the approved plan as draft
  authoring notes.

Final approved plans must not contain `Task Outcome`, `Plan Constraints`, or
`Open Questions` inside `## Task N:` bodies.

Project-level plan metadata remains valid and separate from task contract
fields. Headers such as `QA Requirement`, `Execution Mode`, `Source Spec`,
`Source Spec Revision`, and `Plan Revision` are plan-level gates; they cannot
substitute for task-level `Goal`, `Context`, `Constraints`, or `Done when`.
Task-level `**Goal:**` is parsed only inside `## Task N:` blocks.

## Field Law

`Goal` is exactly one sentence and names the exact state change this task exists
to create. It is invalid when it describes a bucket of related work rather than
one outcome.

`Context` is a bullet list. It exists to eliminate interpretation drift for a
fresh implementer and a fresh reviewer. It is invalid when it is empty, filler,
or forces reviewers to invent missing intent.

`Constraints` is a bullet list of hard rules, not advice. It captures spec law,
file ownership, sequencing, migration limits, compatibility limits, and reuse
requirements that must not drift during implementation.

`Done when` is a stable ordered bullet list of atomic obligations. Each bullet
must be concrete, binary, and objectively reviewable from the same diff,
artifacts, and verification evidence. A bullet may be verified by diff
inspection, targeted tests, or concrete artifacts; it does not have to map to a
single mechanical command.

`Files` is the authoritative task file-scope surface. Step checklists may appear
after `Files` as execution aids, but they are not part of the required
task-contract surface.

## Deterministic Done When

A valid `Done when` bullet:

- names a concrete terminal condition
- can be assessed pass/fail without interpreting intent differently
- is specific enough that two reviewers should reach the same verdict from the
  same diff and evidence
- names command or test evidence when that materially improves determinism

An invalid `Done when` bullet:

- is empty or restates only the task title
- relies on hedging language such as "as needed", "appropriately", "if helpful",
  "where possible", "support", or "handle" without naming the exact condition
- says only "tests pass" when the relevant test surface matters
- bundles unrelated outcomes so partial completion requires judgment calls

## Context Spec References

`Context` must include an explicit spec, decision, or non-goal reference when
any of these triggers apply:

- the task is constrained by a decision or non-goal, not only a requirement ID
- the requirement wording is subtle enough that paraphrase could drift
- the task intentionally reuses or avoids an existing abstraction because of a
  spec choice
- the task's `Done when` interpretation depends on exact spec language

When none of these triggers apply, `Spec Coverage` plus concise repo and
architecture context is enough. Do not add boilerplate citations that do not
change interpretation.

## Reuse Hard-Fail Law

Avoidable duplicate implementation of substantive production behavior is a hard
review failure when a shared implementation is practical and architecturally
correct.

This law targets business-rule-bearing behavior such as parsers, normalizers,
validators, routing logic, eligibility logic, policy enforcement, prompt
assembly, shared state transitions, artifact binding, freshness decisions, and
similar runtime semantics. Reviewers must name the duplicated behavior, the
existing or intended shared home, why duplication is harmful, and the smallest
defensible consolidation path.

Allowed exception categories are narrow:

- generated code
- fixtures or test data duplication
- tiny test-only setup repetition
- platform-specific adapters with an explicit boundary and rationale
- compatibility shims during a controlled migration window
- deliberate architectural separation mandated by an explicit layer or boundary
  law

Non-obvious exceptions must be named in task `Constraints`, a review finding
resolution, or a nearby code comment explaining the boundary. Silent divergence
is not an exception.

Reviewer examples:

- Hard fail: a task adds a second parser, normalizer, validator, router,
  eligibility check, policy gate, prompt assembler, state transition, artifact
  binding check, or freshness decision when a shared implementation is practical
  and architecturally correct. The finding names the duplicated behavior, the
  shared home that should own it, why the duplication can drift, and the
  smallest defensible consolidation path.
- Allowed exception: generated code, fixtures, tiny test-only setup,
  platform-specific adapters, controlled migration shims, or deliberate
  architectural separation repeat shape or data for a named boundary reason.
  The reviewer states the exact exception category and verifies the exception is
  narrower than the substantive production behavior law.

## Obligation Indices

Written plans keep stable ordered `Done when` bullets. Downstream packets and
review artifacts assign canonical obligation indices from that order, for
example `DONE_WHEN_1`, `DONE_WHEN_2`, and `CONSTRAINT_1`. Review findings must
refer to these stable obligations when they assess task completion.

## Deterministic Review Finding Shape

Plan-fidelity review, engineering review, task spec review, task code-quality
review, and final whole-diff code review must use this shared finding shape for
every concrete contract failure. Do not replace a concrete failure with general
feedback prose.

```markdown
### Finding <stable-finding-id>

**Finding ID:** <stable-finding-id>
**Severity:** critical | important | minor
**Task:** Task N | Tasks N, M | n/a
**Violated Field or Obligation:** <Goal | Context | Constraints | DONE_WHEN_N | CONSTRAINT_N | PLAN_DEVIATION_FOUND | AMBIGUITY_ESCALATION_REQUIRED | PACKET_REUSE_SCOPE | Files | checklist section | n/a>
**Evidence:** <exact file:line, artifact line, packet excerpt, or diff fact>
**Required Fix:** <smallest acceptable repair delta>
**Hard Fail:** yes | no
```

For task-scoped findings, `Violated Field or Obligation` must use the packet
assigned canonical obligation ID whenever one exists. `Done when` failures use
`DONE_WHEN_N`; constraint failures use `CONSTRAINT_N`. A reviewer must not
invent a new name for an indexed obligation.

When no narrower packet-assigned obligation exists, reviewers may use these
canonical review-scope IDs:

- `PLAN_DEVIATION_FOUND` for implementation, task, file, or behavior drift
  outside the approved packet or plan scope
- `AMBIGUITY_ESCALATION_REQUIRED` when the packet is insufficient to decide
  correctness without inventing intent
- `PACKET_REUSE_SCOPE` for reuse or duplication findings when the packet does
  not assign a narrower `CONSTRAINT_N` or `DONE_WHEN_N` obligation

Prompt-local obligation names are invalid. Add a new canonical ID here before a
review prompt, example, or consumer depends on it.

`Required Fix` is the repair packet. It must be delta-oriented and narrow enough
that the next implementer can act without paraphrasing the reviewer into a new
interpretation. Valid repair deltas include changing the task contract,
splitting a task, adding missing context, rewriting a non-deterministic done
condition, adding required evidence, or consolidating duplicate implementation
through the named shared helper.

If there are no concrete findings, write `## Findings` followed by `none`.

Example failed finding:

```markdown
### Finding TASK_DONE_WHEN_NON_DETERMINISTIC

**Finding ID:** TASK_DONE_WHEN_NON_DETERMINISTIC
**Severity:** critical
**Task:** Task 3
**Violated Field or Obligation:** DONE_WHEN_2
**Evidence:** Task 3 says "implementation is robust" without a binary terminal condition.
**Required Fix:** Rewrite `DONE_WHEN_2` to name the exact observable terminal condition and any evidence that proves it.
**Hard Fail:** yes
```

## Migration Law

A branch-local temporary dual-read/single-write window is allowed only when it
materially reduces migration churn during this contract cutover. New artifacts
must be written in the new task format immediately.

Merge-ready active workflow is new-format-only. Any old-format compatibility
path that survives must be explicitly quarantined outside normal authoring,
packet generation, runtime projection checks, and review flow, and it must be marked
migration-only.
