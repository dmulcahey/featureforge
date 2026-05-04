# Writing Plans Examples

This reference carries detailed examples for `writing-plans`. The top-level skill remains authoritative for required plan headers, task-contract fields, protected-branch gates, plan-review handoff, and execution handoff.

## Canonical Task Example

````markdown
## Task N: [Component Name]

**Spec Coverage:** REQ-001, DEC-001
**Goal:** [One sentence describing the exact outcome this task produces]

**Context:**
- [Why this task exists in the plan]
- [Repo or architecture fact the implementer/reviewer must know]
- [Spec, decision, or non-goal reference when required by review/plan-task-contract.md]

**Constraints:**
- [Hard rule inherited from the approved spec or review]
- [Hard rule inherited from decomposition, file ownership, sequencing, or reuse law]

**Done when:**
- [Atomic, binary, objectively reviewable completion condition]
- [Atomic, binary, objectively reviewable completion condition]

**Files:**
- Create: `exact/path/to/file.py`
- Modify: `exact/path/to/existing.py`
- Test: `tests/exact/path/to/test.py`

- [ ] **Step 1: Write the failing test**
- [ ] **Step 2: Run test to verify it fails**
- [ ] **Step 3: Write minimal implementation**
- [ ] **Step 4: Run test to verify it passes**
- [ ] **Step 5: Commit**
````

Step checklists are optional execution aids. They are not required task-contract surface.

## Serial Plus Parallel Worktree Example

````markdown
## Execution Strategy

- Execute Task 1 serially. It establishes the approved foundation.
- Execute Task 2 serially. It extracts the shared seam used by the parallel batch.
- After Task 2, create three worktrees and run Tasks 3, 4, and 5 in parallel:
  - Task 3 owns parser integration.
  - Task 4 owns runtime routing.
  - Task 5 owns generated prompt surfaces.
- Execute Task 6 serially after Tasks 3, 4, and 5 merge back. It is the reintegration gate.

## Dependency Diagram

```text
Task 1 -> Task 2
Task 2 -> Task 3
Task 2 -> Task 4
Task 2 -> Task 5
Task 3 -> Task 6
Task 4 -> Task 6
Task 5 -> Task 6
```
````

Use this shape only when write scopes and reintegration hazards support it. If tasks share hotspot files, move the hotspot into an explicit serial seam instead of pretending the work is parallel.
