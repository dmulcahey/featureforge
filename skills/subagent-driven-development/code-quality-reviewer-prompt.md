# Code Quality Reviewer Prompt Template

Use this template when spawning a code quality reviewer sub-agent or custom agent.

**Purpose:** Verify implementation is well-built (clean, tested, maintainable)

## Review-subagent recursion rule

You are a reviewer. You may inspect the provided files, packet, summaries, and context and produce review findings. Do not launch, request, or delegate to additional subagents while performing this review. Do not delegate this review to another reviewer agent. Do not invoke `subagent-driven-development`, `requesting-code-review`, `plan-fidelity-review`, `plan-eng-review`, `plan-ceo-review`, or any other FeatureForge skill/workflow for the purpose of spawning another reviewer. Use only the files, packet, summaries, and context supplied to this review. If the supplied context is insufficient, return a blocked review finding that names the missing context instead of spawning another agent.

**Only dispatch after spec compliance review passes.**

```
Code-reviewer sub-agent / custom agent:
  Use template at ../requesting-code-review/code-reviewer.md

  TASK_PACKET: [helper-built task packet]
  WHAT_WAS_IMPLEMENTED: [from implementer's report]
  PLAN_OR_REQUIREMENTS: Task N from [plan-file]
  APPROVED_PLAN_PATH: [exact approved plan path for plan-routed final review, otherwise blank]
  EXECUTION_EVIDENCE_PATH: [helper-reported evidence path for plan-routed final review, otherwise blank]
  BASE_BRANCH: [runtime-provided base branch for plan-routed review, otherwise explicitly provided base branch]
  BASE_SHA: [commit before task]
  HEAD_SHA: [current commit]
  DESCRIPTION: [task summary]
```

**In addition to standard code quality concerns, the reviewer should check:**
- Does each file have one clear responsibility with a well-defined interface?
- Are units decomposed so they can be understood and tested independently?
- Is the implementation following the file structure from the task packet?
- Is there work outside planned file decomposition?
- Did the change reuse the planned shared implementation named by the task packet?
- Did the change introduce a second implementation of parser, normalization, validation, routing, eligibility, policy, prompt assembly, state transition, artifact binding, freshness, or other substantive production behavior?
- If separate implementations exist, does the packet name an approved exception and does the diff stay inside that exception?
- Treat avoidable duplicate implementation as a hard failure even when the feature behavior works.
- Every reuse or duplication issue must include a stable finding ID and the violated packet obligation ID, such as `CONSTRAINT_2` or `DONE_WHEN_1`.
- Name the smallest corrective action needed to consolidate or justify the duplicated behavior.
- Use the deterministic review finding shape from `review/plan-task-contract.md` for every concrete issue:
  `Finding ID`, `Severity`, `Task`, `Violated Field or Obligation`, `Evidence`, `Required Fix`, and `Hard Fail: yes|no`.
- Do not use general feedback when a concrete checklist section, task field, or packet-assigned obligation can be named.
- Return a reuse assessment matrix with pass/fail rows for each packet reuse expectation and each duplication check.
- Each reuse assessment row must name the packet obligation ID it grades, or `PACKET_REUSE_SCOPE` when the packet does not assign a narrower obligation.
- Did this implementation create new files that are already large, or significantly grow existing files? (Don't flag pre-existing file sizes — focus on what this change contributed.)
- Did this implementation introduce new files or abstractions outside packet scope?

**Code reviewer returns:** Strengths, Issues as deterministic repair-packet findings (Critical/Important/Minor), Reuse Assessment Matrix, Assessment
