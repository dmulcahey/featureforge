import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { ensureDirectoryExists, pathExists, readTextFileIfExists } from '../platform/filesystem';
import { normalizeRelativePath } from '../platform/paths';

type ExecutionMode = 'none' | 'superpowers:executing-plans' | 'superpowers:subagent-driven-development';
type CommandEnvironment = {
  cwd?: string;
  env?: Record<string, string | undefined>;
};

export type CommandResult = {
  exitCode: number;
  stdout: string;
  stderr: string;
};

type StepNoteState = '' | 'Active' | 'Interrupted' | 'Blocked';
type AttemptStatus = 'Completed' | 'Invalidated';

type StepRecord = {
  task: string;
  step: string;
  checked: boolean;
  title: string;
  noteState: StepNoteState;
  noteSummary: string;
};

type AttemptRecord = {
  task: string;
  step: string;
  number: number;
  status: AttemptStatus;
  recordedAt: string;
  source: Exclude<ExecutionMode, 'none'>;
  claim: string;
  files: string[];
  verification: string;
  invalidationReason: string;
};

type LoadedExecutionState = {
  repoRoot: string;
  env: Record<string, string | undefined>;
  planRelPath: string;
  planAbsPath: string;
  planText: string;
  planWorkflowState: string;
  planRevision: string;
  planExecutionMode: ExecutionMode;
  planSourceSpec: string;
  planSourceSpecRevision: string;
  planLastReviewedBy: string;
  steps: StepRecord[];
  planCheckedCount: number;
  planNoteCount: number;
  planActiveTask: string | null;
  planActiveStep: string | null;
  planBlockingTask: string | null;
  planBlockingStep: string | null;
  planResumeTask: string | null;
  planResumeStep: string | null;
  evidenceRelPath: string;
  evidenceAbsPath: string;
  evidenceExists: boolean;
  evidenceEmptyState: boolean;
  evidenceAttemptCount: number;
  attempts: AttemptRecord[];
  executionStarted: 'yes' | 'no';
  executionFingerprint: string;
};

type PersistedTextValidation = {
  normalized: string;
};

type StatusJson = {
  plan_revision: number;
  execution_mode: string;
  execution_fingerprint: string;
  evidence_path: string;
  execution_started: 'yes' | 'no';
  active_task: number | null;
  active_step: number | null;
  blocking_task: number | null;
  blocking_step: number | null;
  resume_task: number | null;
  resume_step: number | null;
};

type RecommendJson = {
  recommended_skill: string;
  reason: string;
  decision_flags: {
    tasks_independent: 'yes' | 'no' | 'unknown';
    isolated_agents_available: 'yes' | 'no' | 'unknown';
    session_intent: 'stay' | 'separate' | 'unknown';
    workspace_prepared: 'yes' | 'no' | 'unknown';
    same_session_viable: 'yes' | 'no' | 'unknown';
  };
};

class PlanExecutionFailure extends Error {
  readonly failureClass: string;

  constructor(failureClass: string, message: string) {
    super(message);
    this.name = 'PlanExecutionFailure';
    this.failureClass = failureClass;
  }
}

function fail(failureClass: string, message: string): never {
  throw new PlanExecutionFailure(failureClass, message);
}

function splitNormalizedLines(text: string): string[] {
  const lines = text.replace(/\r\n/g, '\n').split('\n');
  if (lines.length > 0 && lines[lines.length - 1] === '') {
    lines.pop();
  }
  return lines;
}

function normalizeWhitespace(value: string): string {
  return value.replace(/[\r\n\t]/g, ' ').replace(/\s+/g, ' ').trim();
}

function truncateWithEllipsis(text: string, maxLength: number): string {
  if (text.length <= maxLength) {
    return text;
  }
  if (maxLength <= 3) {
    return text.slice(0, maxLength);
  }
  return `${text.slice(0, maxLength - 3)}...`;
}

function activeSummaryFromTitle(title: string): string {
  return truncateWithEllipsis(normalizeWhitespace(title), 120);
}

function reopenNoteSummary(reason: string): string {
  return truncateWithEllipsis(`Reopened: ${normalizeWhitespace(reason)}`, 120);
}

function currentTimestamp(): string {
  return new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');
}

function toCommandResult(exitCode: number, stdout = '', stderr = ''): CommandResult {
  return { exitCode, stdout, stderr };
}

function errorResult(failureClass: string, message: string): CommandResult {
  return toCommandResult(1, `${JSON.stringify({ error_class: failureClass, message })}\n`);
}

function usageResult(): CommandResult {
  return toCommandResult(
    1,
    '',
    [
      'Usage:',
      '  superpowers-plan-execution status --plan <approved-plan-path>',
      '  superpowers-plan-execution recommend --plan <approved-plan-path> [--isolated-agents available|unavailable] [--session-intent stay|separate|unknown] [--workspace-prepared yes|no|unknown]',
      '  superpowers-plan-execution begin ...',
      '  superpowers-plan-execution transfer ...',
      '  superpowers-plan-execution complete ...',
      '  superpowers-plan-execution note ...',
      '  superpowers-plan-execution reopen ...',
      '',
    ].join('\n'),
  );
}

function normalizeRepoRelativePath(input: string): string | null {
  return normalizeRelativePath(input);
}

function stripPlanFileReferenceSuffix(value: string): string {
  return value.replace(/:[0-9]+(?:[:-][0-9]+)*$/, '');
}

function normalizePlanScopePath(value: string): string | null {
  return normalizeRepoRelativePath(stripPlanFileReferenceSuffix(value));
}

function planRelToAbs(repoRoot: string, relativePath: string): string {
  return path.join(repoRoot, relativePath);
}

function computeExecutionFingerprint(planText: string, evidenceText: string, evidenceEmptyState: boolean): string {
  const hash = crypto.createHash('sha256');
  hash.update('plan\n');
  hash.update(planText);
  hash.update('\n--evidence--\n');
  hash.update(evidenceEmptyState ? '__EMPTY_EVIDENCE__\n' : evidenceText);
  return hash.digest('hex');
}

function validatePersistedNormalizedText(
  rawValue: string,
  emptyMessage: string,
  overlongMessage?: string,
  maxLength?: number,
): PersistedTextValidation {
  const normalized = normalizeWhitespace(rawValue);
  if (normalized.length === 0) {
    fail('MalformedExecutionState', emptyMessage);
  }
  if (maxLength !== undefined && normalized.length > maxLength) {
    fail('MalformedExecutionState', overlongMessage ?? emptyMessage);
  }
  return { normalized };
}

function validatePersistedRepoRelativePath(rawValue: string, message: string): string {
  const trimmed = rawValue.replace(/^\s+/, '').replace(/\s+$/, '');
  const normalized = normalizeRepoRelativePath(trimmed);
  if (normalized === null) {
    fail('MalformedExecutionState', message);
  }
  return normalized;
}

function validatePersistedExecutionSource(rawValue: string, planExecutionMode: ExecutionMode): Exclude<ExecutionMode, 'none'> {
  const normalized = normalizeWhitespace(rawValue);
  if (
    normalized !== 'superpowers:executing-plans' &&
    normalized !== 'superpowers:subagent-driven-development'
  ) {
    fail(
      'MalformedExecutionState',
      'Execution evidence source must be one of the supported execution modes.',
    );
  }
  if (planExecutionMode !== 'none' && normalized !== planExecutionMode) {
    fail(
      'MalformedExecutionState',
      'Execution evidence source must exactly match the persisted execution mode for this plan revision.',
    );
  }
  return normalized;
}

function validateRequiredNormalizedText(
  rawValue: string,
  emptyMessage: string,
  overlongMessage?: string,
  maxLength?: number,
): string {
  const normalized = normalizeWhitespace(rawValue);
  if (normalized.length === 0) {
    fail('InvalidCommandInput', emptyMessage);
  }
  if (maxLength !== undefined && normalized.length > maxLength) {
    fail('InvalidCommandInput', overlongMessage ?? emptyMessage);
  }
  return normalized;
}

function validateNoteMessage(message: string): string {
  return validateRequiredNormalizedText(
    message,
    'Execution note summaries may not be blank after whitespace normalization.',
    'Normalized execution note summaries may not exceed 120 characters.',
    120,
  );
}

function validateExecutionModeSource(source: string, state: LoadedExecutionState): Exclude<ExecutionMode, 'none'> {
  if (source !== 'superpowers:executing-plans' && source !== 'superpowers:subagent-driven-development') {
    fail('InvalidExecutionMode', 'Execution source must be one of the supported execution modes.');
  }
  if (state.planExecutionMode === 'none' || source !== state.planExecutionMode) {
    fail(
      'InvalidExecutionMode',
      'Execution source must exactly match the persisted execution mode for this plan revision.',
    );
  }
  return source;
}

function resolveRepoRoot(cwd: string): string {
  const result = spawnSync('git', ['rev-parse', '--show-toplevel'], {
    cwd,
    encoding: 'utf8',
  });
  if (result.status === 0) {
    const resolved = result.stdout.trim();
    if (resolved.length > 0) {
      return resolved;
    }
  }
  return cwd;
}

function firstMatchingGroup(text: string, pattern: RegExp): string {
  for (const line of splitNormalizedLines(text)) {
    const match = line.match(pattern);
    if (match) {
      return match[1] ?? '';
    }
  }
  return '';
}

function validateSourceSpec(repoRoot: string, sourceRel: string, expectedRevision: string): void {
  const normalized = normalizeRepoRelativePath(sourceRel);
  if (normalized === null) {
    fail('PlanNotExecutionReady', 'Approved plan source spec path is malformed.');
  }

  const sourceAbs = planRelToAbs(repoRoot, normalized);
  if (!pathExists(sourceAbs)) {
    fail('PlanNotExecutionReady', 'Approved plan source spec does not exist.');
  }

  const text = readTextFileIfExists(sourceAbs);
  const specState = firstMatchingGroup(text, /^\*\*Workflow State:\*\* (.+)$/);
  const specRevision = firstMatchingGroup(text, /^\*\*Spec Revision:\*\* ([0-9]+)$/);
  const specReviewer = normalizeWhitespace(firstMatchingGroup(text, /^\*\*Last Reviewed By:\*\* (.+)$/));

  if (specState !== 'CEO Approved') {
    fail('PlanNotExecutionReady', 'Approved plan source spec is not CEO Approved.');
  }
  if (specRevision !== expectedRevision) {
    fail('PlanNotExecutionReady', 'Approved plan source spec revision is stale.');
  }
  if (specReviewer !== 'brainstorming' && specReviewer !== 'plan-ceo-review') {
    fail(
      'PlanNotExecutionReady',
      'Approved plan source spec Last Reviewed By header is missing or malformed.',
    );
  }
}

function findStepIndex(state: LoadedExecutionState, task: string, step: string): number {
  return state.steps.findIndex((candidate) => candidate.task === task && candidate.step === step);
}

function findStepOrFail(state: LoadedExecutionState, task: string, step: string): number {
  const index = findStepIndex(state, task, step);
  if (index < 0) {
    fail('InvalidStepTransition', 'Requested task/step does not exist in the approved plan.');
  }
  return index;
}

function parsePlanFile(planText: string): Omit<
  LoadedExecutionState,
  | 'repoRoot'
  | 'env'
  | 'planRelPath'
  | 'planAbsPath'
  | 'planText'
  | 'evidenceRelPath'
  | 'evidenceAbsPath'
  | 'evidenceExists'
  | 'evidenceEmptyState'
  | 'evidenceAttemptCount'
  | 'attempts'
  | 'executionStarted'
  | 'executionFingerprint'
> {
  const steps: StepRecord[] = [];
  let planWorkflowState = '';
  let planRevision = '';
  let planExecutionMode = '' as ExecutionMode;
  let planSourceSpec = '';
  let planSourceSpecRevision = '';
  let planLastReviewedBy = '';
  let currentTask = '';
  let pendingIndex: number | null = null;
  let planCheckedCount = 0;
  let planNoteCount = 0;

  for (const line of splitNormalizedLines(planText)) {
    let match = line.match(/^\*\*Workflow State:\*\* (.+)$/);
    if (match) {
      planWorkflowState = match[1] ?? '';
      continue;
    }

    match = line.match(/^\*\*Plan Revision:\*\* ([0-9]+)$/);
    if (match) {
      planRevision = match[1] ?? '';
      continue;
    }

    match = line.match(/^\*\*Execution Mode:\*\* (.+)$/);
    if (match) {
      planExecutionMode = (match[1] ?? '') as ExecutionMode;
      continue;
    }

    match = line.match(/^\*\*Source Spec:\*\* `(.+)`$/);
    if (match) {
      planSourceSpec = match[1] ?? '';
      continue;
    }

    match = line.match(/^\*\*Source Spec Revision:\*\* ([0-9]+)$/);
    if (match) {
      planSourceSpecRevision = match[1] ?? '';
      continue;
    }

    match = line.match(/^\*\*Last Reviewed By:\*\* (.+)$/);
    if (match) {
      planLastReviewedBy = match[1] ?? '';
      continue;
    }

    match = line.match(/^## Task ([0-9]+):/);
    if (match) {
      currentTask = match[1] ?? '';
      pendingIndex = null;
      continue;
    }

    match = line.match(/^- \[([ x])\] \*\*Step ([0-9]+): (.*)\*\*$/);
    if (match) {
      if (currentTask.length === 0) {
        fail('MalformedExecutionState', 'Found a step outside any task heading in approved plan.');
      }
      const step = match[2] ?? '';
      if (steps.some((candidate) => candidate.task === currentTask && candidate.step === step)) {
        fail('MalformedExecutionState', `Duplicate Task ${currentTask} Step ${step} in approved plan.`);
      }
      const checked = (match[1] ?? '') === 'x';
      if (checked) {
        planCheckedCount += 1;
      }
      steps.push({
        task: currentTask,
        step,
        checked,
        title: match[3] ?? '',
        noteState: '',
        noteSummary: '',
      });
      pendingIndex = steps.length - 1;
      continue;
    }

    match = line.match(/^\s+\*\*Execution Note:\*\* (.+)$/);
    if (match) {
      if (pendingIndex === null) {
        fail('MalformedExecutionState', 'Execution note is not adjacent to an unchecked step.');
      }
      const pendingStep = steps[pendingIndex];
      if (pendingStep.noteState.length > 0) {
        fail('MalformedExecutionState', 'Unchecked steps may not carry more than one execution note.');
      }
      if (pendingStep.checked) {
        fail('MalformedExecutionState', 'Checked steps may not retain execution notes.');
      }

      const noteMatch = (match[1] ?? '').match(/^(Active|Interrupted|Blocked) - (.+)$/);
      if (!noteMatch) {
        fail(
          'MalformedExecutionState',
          "Execution notes must use canonical '<State> - <summary>' form.",
        );
      }

      const validated = validatePersistedNormalizedText(
        noteMatch[2] ?? '',
        'Execution note summaries may not be blank after whitespace normalization.',
        'Execution note summaries may not exceed 120 characters after whitespace normalization.',
        120,
      );
      pendingStep.noteState = noteMatch[1] as StepNoteState;
      pendingStep.noteSummary = validated.normalized;
      planNoteCount += 1;
      continue;
    }

    if (pendingIndex !== null && line.trim().length > 0) {
      pendingIndex = null;
    }
  }

  let planActiveTask: string | null = null;
  let planActiveStep: string | null = null;
  let planBlockingTask: string | null = null;
  let planBlockingStep: string | null = null;
  let planResumeTask: string | null = null;
  let planResumeStep: string | null = null;
  let currentWorkCount = 0;
  let interruptedCount = 0;

  for (const step of steps) {
    switch (step.noteState) {
      case 'Active':
        currentWorkCount += 1;
        planActiveTask = step.task;
        planActiveStep = step.step;
        break;
      case 'Blocked':
        currentWorkCount += 1;
        planBlockingTask = step.task;
        planBlockingStep = step.step;
        break;
      case 'Interrupted':
        interruptedCount += 1;
        planResumeTask = step.task;
        planResumeStep = step.step;
        break;
      case '':
        break;
      default:
        fail('MalformedExecutionState', `Unsupported execution note state '${step.noteState}'.`);
    }
  }

  if (currentWorkCount > 1) {
    fail('MalformedExecutionState', 'Plan may have at most one current-work note at a time.');
  }
  if (interruptedCount > 1) {
    fail('MalformedExecutionState', 'Plan may have at most one interrupted resume note at a time.');
  }

  return {
    planWorkflowState,
    planRevision,
    planExecutionMode,
    planSourceSpec,
    planSourceSpecRevision,
    planLastReviewedBy,
    steps,
    planCheckedCount,
    planNoteCount,
    planActiveTask,
    planActiveStep,
    planBlockingTask,
    planBlockingStep,
    planResumeTask,
    planResumeStep,
  };
}

function parseEvidenceFileMetadata(
  evidenceText: string,
  expectedPlanRel: string,
  expectedRevision: string,
): { evidenceEmptyState: boolean; evidenceAttemptCount: number } {
  const lines = splitNormalizedLines(evidenceText);
  const titleCount = lines.filter((line) => line.startsWith('# Execution Evidence:')).length;
  const pathCount = lines.filter((line) => line.startsWith('**Plan Path:** ')).length;
  const revisionCount = lines.filter((line) => line.startsWith('**Plan Revision:** ')).length;
  const sectionCount = lines.filter((line) => line === '## Step Evidence').length;
  const stepSectionCount = lines.filter((line) => /^### Task [0-9]+ Step [0-9]+$/.test(line)).length;
  const attemptCount = lines.filter((line) => /^#### Attempt [0-9]+$/.test(line)).length;

  if (titleCount !== 1 || pathCount !== 1 || revisionCount !== 1 || sectionCount !== 1) {
    fail('MalformedExecutionState', 'Execution evidence artifact header structure is malformed.');
  }

  const evidencePathLine = firstMatchingGroup(evidenceText, /^\*\*Plan Path:\*\* (.+)$/);
  const evidenceRevisionLine = firstMatchingGroup(evidenceText, /^\*\*Plan Revision:\*\* ([0-9]+)$/);
  if (evidencePathLine !== expectedPlanRel || evidenceRevisionLine !== expectedRevision) {
    fail(
      'MalformedExecutionState',
      'Execution evidence artifact does not match the current approved plan revision.',
    );
  }

  if (stepSectionCount === 0 && attemptCount === 0) {
    return { evidenceEmptyState: true, evidenceAttemptCount: 0 };
  }

  if (stepSectionCount === 0 || attemptCount === 0) {
    fail(
      'MalformedExecutionState',
      'Execution evidence artifact is missing required step sections or attempts.',
    );
  }

  return { evidenceEmptyState: false, evidenceAttemptCount: attemptCount };
}

function parseEvidenceAttempts(
  evidenceText: string,
  planExecutionMode: ExecutionMode,
): AttemptRecord[] {
  const attempts: AttemptRecord[] = [];
  const lines = splitNormalizedLines(evidenceText);
  let currentTask = '';
  let currentStep = '';
  let currentAttempt: AttemptRecord | null = null;
  let expectedStage = 0;
  let lastTask = 0;
  let lastStep = 0;
  let lastAttemptNumber = 0;
  const seenSections = new Set<string>();
  let index = 0;
  let persistedEvidenceSource: string | null = null;

  while (index < lines.length) {
    const line = lines[index] ?? '';
    index += 1;

    if (line.length === 0) {
      continue;
    }

    let match = line.match(/^### Task ([0-9]+) Step ([0-9]+)$/);
    if (match) {
      if (currentAttempt !== null && expectedStage !== 8) {
        fail('MalformedExecutionState', 'Execution evidence attempt is incomplete.');
      }
      const taskNumber = Number(match[1] ?? '0');
      const stepNumber = Number(match[2] ?? '0');
      if (taskNumber < lastTask || (taskNumber === lastTask && stepNumber <= lastStep)) {
        fail(
          'MalformedExecutionState',
          'Execution evidence step sections must be ordered canonically and appear once.',
        );
      }
      const sectionKey = `${taskNumber}:${stepNumber}`;
      if (seenSections.has(sectionKey)) {
        fail('MalformedExecutionState', 'Execution evidence step sections may not repeat.');
      }
      seenSections.add(sectionKey);
      currentTask = String(taskNumber);
      currentStep = String(stepNumber);
      currentAttempt = null;
      lastTask = taskNumber;
      lastStep = stepNumber;
      lastAttemptNumber = 0;
      continue;
    }

    match = line.match(/^#### Attempt ([0-9]+)$/);
    if (match) {
      if (currentAttempt !== null && expectedStage !== 8) {
        fail('MalformedExecutionState', 'Execution evidence attempt is incomplete.');
      }
      if (currentTask.length === 0) {
        fail('MalformedExecutionState', 'Execution evidence attempt is missing its step heading.');
      }
      const attemptNumber = Number(match[1] ?? '0');
      if (attemptNumber !== lastAttemptNumber + 1) {
        fail(
          'MalformedExecutionState',
          'Execution evidence attempts must be contiguous and start at 1 within each step section.',
        );
      }
      currentAttempt = {
        task: currentTask,
        step: currentStep,
        number: attemptNumber,
        status: 'Completed',
        recordedAt: '',
        source: 'superpowers:executing-plans',
        claim: '',
        files: [],
        verification: '',
        invalidationReason: '',
      };
      attempts.push(currentAttempt);
      lastAttemptNumber = attemptNumber;
      expectedStage = 1;
      continue;
    }

    if (line.startsWith('# Execution Evidence:') || line === '## Step Evidence' || line.startsWith('**Plan Path:** ') || line.startsWith('**Plan Revision:** ')) {
      continue;
    }

    if (currentAttempt === null) {
      fail('MalformedExecutionState', 'Execution evidence contains unexpected content.');
    }

    match = line.match(/^\*\*Status:\*\* (.+)$/);
    if (match) {
      if (expectedStage !== 1) {
        fail('MalformedExecutionState', 'Execution evidence fields are out of order.');
      }
      currentAttempt.status = (match[1] ?? '') as AttemptStatus;
      expectedStage = 2;
      continue;
    }

    match = line.match(/^\*\*Recorded At:\*\* (.+)$/);
    if (match) {
      if (expectedStage !== 2) {
        fail('MalformedExecutionState', 'Execution evidence fields are out of order.');
      }
      currentAttempt.recordedAt = match[1] ?? '';
      expectedStage = 3;
      continue;
    }

    match = line.match(/^\*\*Execution Source:\*\* (.+)$/);
    if (match) {
      if (expectedStage !== 3) {
        fail('MalformedExecutionState', 'Execution evidence fields are out of order.');
      }
      const source = validatePersistedExecutionSource(match[1] ?? '', planExecutionMode);
      currentAttempt.source = source;
      if (persistedEvidenceSource === null) {
        persistedEvidenceSource = source;
      } else if (persistedEvidenceSource !== source) {
        fail(
          'MalformedExecutionState',
          'Execution evidence contains multiple distinct persisted execution sources for one revision.',
        );
      }
      expectedStage = 4;
      continue;
    }

    match = line.match(/^\*\*Claim:\*\* (.+)$/);
    if (match) {
      if (expectedStage !== 4) {
        fail('MalformedExecutionState', 'Execution evidence fields are out of order.');
      }
      currentAttempt.claim = validatePersistedNormalizedText(
        match[1] ?? '',
        'Execution evidence Claim text may not be blank after whitespace normalization.',
      ).normalized;
      expectedStage = 5;
      continue;
    }

    if (line === '**Files:**') {
      if (expectedStage !== 5) {
        fail('MalformedExecutionState', 'Execution evidence fields are out of order.');
      }
      const files: string[] = [];
      while (index < lines.length) {
        const fileLine = lines[index] ?? '';
        const fileMatch = fileLine.match(/^- (.+)$/);
        if (!fileMatch) {
          break;
        }
        files.push(
          validatePersistedRepoRelativePath(
            fileMatch[1] ?? '',
            'Execution evidence Files bullets must be canonical repo-relative paths.',
          ),
        );
        index += 1;
      }
      if (files.length === 0) {
        fail('MalformedExecutionState', 'Execution evidence Files section must contain at least one bullet.');
      }
      currentAttempt.files = files;
      expectedStage = 6;
      continue;
    }

    if (line === '**Verification:**') {
      if (expectedStage !== 6) {
        fail('MalformedExecutionState', 'Execution evidence fields are out of order.');
      }
      const nextLine = lines[index] ?? '';
      const verificationMatch = nextLine.match(/^- (.+)$/);
      if (!verificationMatch) {
        fail(
          'MalformedExecutionState',
          'Execution evidence Verification section must contain exactly one bullet.',
        );
      }
      currentAttempt.verification = validatePersistedNormalizedText(
        verificationMatch[1] ?? '',
        'Execution evidence Verification text may not be blank after whitespace normalization.',
      ).normalized;
      index += 1;
      const additionalLine = lines[index] ?? '';
      if (/^- (.+)$/.test(additionalLine)) {
        fail(
          'MalformedExecutionState',
          'Execution evidence Verification section may not contain more than one bullet.',
        );
      }
      expectedStage = 7;
      continue;
    }

    match = line.match(/^\*\*Invalidation Reason:\*\* (.+)$/);
    if (match) {
      if (expectedStage !== 7) {
        fail('MalformedExecutionState', 'Execution evidence fields are out of order.');
      }
      currentAttempt.invalidationReason = validatePersistedNormalizedText(
        match[1] ?? '',
        'Execution evidence Invalidation Reason text may not be blank after whitespace normalization.',
      ).normalized;
      expectedStage = 8;
      continue;
    }

    fail('MalformedExecutionState', 'Execution evidence contains unexpected content.');
  }

  if (currentAttempt !== null && expectedStage !== 8) {
    fail('MalformedExecutionState', 'Execution evidence attempt is incomplete.');
  }

  for (const attempt of attempts) {
    switch (attempt.status) {
      case 'Completed':
        if (attempt.invalidationReason !== 'N/A') {
          fail('MalformedExecutionState', "Completed attempts must use 'N/A' as the invalidation reason.");
        }
        break;
      case 'Invalidated':
        if (attempt.invalidationReason.length === 0 || attempt.invalidationReason === 'N/A') {
          fail(
            'MalformedExecutionState',
            'Invalidated attempts must record a non-empty invalidation reason.',
          );
        }
        break;
      default:
        fail(
          'MalformedExecutionState',
          'Execution evidence attempts may only be Completed or Invalidated.',
        );
    }

    if (
      attempt.recordedAt.length === 0 ||
      attempt.source.length === 0 ||
      attempt.claim.length === 0 ||
      attempt.files.length === 0 ||
      attempt.verification.length === 0
    ) {
      fail(
        'MalformedExecutionState',
        'Execution evidence attempts must contain every required field.',
      );
    }
  }

  return attempts;
}

function loadExecutionState(planRel: string, environment: CommandEnvironment): LoadedExecutionState {
  const cwd = environment.cwd ?? process.cwd();
  const env = environment.env ?? process.env;
  const repoRoot = resolveRepoRoot(cwd);
  const planRelPath = normalizeRepoRelativePath(planRel);
  if (planRelPath === null) {
    fail('InvalidCommandInput', 'Plan path must be a normalized repo-relative path.');
  }
  if (!planRelPath.startsWith('docs/superpowers/plans/')) {
    fail('InvalidCommandInput', 'Plan path must live under docs/superpowers/plans/.');
  }

  const planAbsPath = planRelToAbs(repoRoot, planRelPath);
  if (!pathExists(planAbsPath)) {
    fail('InvalidCommandInput', 'Approved plan file does not exist.');
  }

  const planText = readTextFileIfExists(planAbsPath);
  const parsedPlan = parsePlanFile(planText);
  if (parsedPlan.planWorkflowState !== 'Engineering Approved') {
    fail('PlanNotExecutionReady', 'Plan is not Engineering Approved.');
  }
  if (parsedPlan.planRevision.length === 0) {
    fail('PlanNotExecutionReady', 'Plan Revision header is missing or malformed.');
  }
  if (
    parsedPlan.planExecutionMode !== 'none' &&
    parsedPlan.planExecutionMode !== 'superpowers:executing-plans' &&
    parsedPlan.planExecutionMode !== 'superpowers:subagent-driven-development'
  ) {
    fail(
      'PlanNotExecutionReady',
      'Execution Mode header is missing, malformed, or out of range.',
    );
  }
  if (parsedPlan.planSourceSpec.length === 0 || parsedPlan.planSourceSpecRevision.length === 0) {
    fail('PlanNotExecutionReady', 'Approved plan source spec headers are missing or malformed.');
  }

  parsedPlan.planLastReviewedBy = normalizeWhitespace(parsedPlan.planLastReviewedBy);
  if (parsedPlan.planLastReviewedBy !== 'writing-plans' && parsedPlan.planLastReviewedBy !== 'plan-eng-review') {
    fail(
      'PlanNotExecutionReady',
      'Approved plan Last Reviewed By header is missing or malformed.',
    );
  }

  validateSourceSpec(repoRoot, parsedPlan.planSourceSpec, parsedPlan.planSourceSpecRevision);

  const evidenceRelPath = deriveEvidenceRelPath(planRelPath, Number(parsedPlan.planRevision));
  const evidenceAbsPath = planRelToAbs(repoRoot, evidenceRelPath);
  const evidenceExists = pathExists(evidenceAbsPath);
  const evidenceText = evidenceExists ? readTextFileIfExists(evidenceAbsPath) : '';
  const evidenceMetadata = evidenceExists
    ? parseEvidenceFileMetadata(evidenceText, planRelPath, parsedPlan.planRevision)
    : { evidenceEmptyState: true, evidenceAttemptCount: 0 };
  const attempts =
    evidenceExists && !evidenceMetadata.evidenceEmptyState
      ? parseEvidenceAttempts(evidenceText, parsedPlan.planExecutionMode)
      : [];

  if (parsedPlan.planExecutionMode === 'none') {
    if (evidenceMetadata.evidenceAttemptCount > 0) {
      fail(
        'MalformedExecutionState',
        'Execution evidence history cannot exist while Execution Mode is none.',
      );
    }
    if (parsedPlan.planCheckedCount > 0 || parsedPlan.planNoteCount > 0) {
      fail('PlanNotExecutionReady', 'Newly approved plan revisions must start execution-clean.');
    }
  }

  let executionStarted: 'yes' | 'no' =
    parsedPlan.planExecutionMode === 'none' ? 'no' : 'yes';
  if (
    parsedPlan.planCheckedCount > 0 ||
    parsedPlan.planNoteCount > 0 ||
    evidenceMetadata.evidenceAttemptCount > 0
  ) {
    executionStarted = 'yes';
  }

  return {
    repoRoot,
    env,
    planRelPath,
    planAbsPath,
    planText,
    planWorkflowState: parsedPlan.planWorkflowState,
    planRevision: parsedPlan.planRevision,
    planExecutionMode: parsedPlan.planExecutionMode,
    planSourceSpec: parsedPlan.planSourceSpec,
    planSourceSpecRevision: parsedPlan.planSourceSpecRevision,
    planLastReviewedBy: parsedPlan.planLastReviewedBy,
    steps: parsedPlan.steps,
    planCheckedCount: parsedPlan.planCheckedCount,
    planNoteCount: parsedPlan.planNoteCount,
    planActiveTask: parsedPlan.planActiveTask,
    planActiveStep: parsedPlan.planActiveStep,
    planBlockingTask: parsedPlan.planBlockingTask,
    planBlockingStep: parsedPlan.planBlockingStep,
    planResumeTask: parsedPlan.planResumeTask,
    planResumeStep: parsedPlan.planResumeStep,
    evidenceRelPath,
    evidenceAbsPath,
    evidenceExists,
    evidenceEmptyState: evidenceMetadata.evidenceEmptyState,
    evidenceAttemptCount: evidenceMetadata.evidenceAttemptCount,
    attempts,
    executionStarted,
    executionFingerprint: computeExecutionFingerprint(
      planText,
      evidenceText,
      evidenceMetadata.evidenceEmptyState,
    ),
  };
}

function serializeStatusJson(state: LoadedExecutionState): string {
  const payload: StatusJson = {
    plan_revision: Number(state.planRevision),
    execution_mode: state.planExecutionMode,
    execution_fingerprint: state.executionFingerprint,
    evidence_path: state.evidenceRelPath,
    execution_started: state.executionStarted,
    active_task: state.planActiveTask === null ? null : Number(state.planActiveTask),
    active_step: state.planActiveStep === null ? null : Number(state.planActiveStep),
    blocking_task: state.planBlockingTask === null ? null : Number(state.planBlockingTask),
    blocking_step: state.planBlockingStep === null ? null : Number(state.planBlockingStep),
    resume_task: state.planResumeTask === null ? null : Number(state.planResumeTask),
    resume_step: state.planResumeStep === null ? null : Number(state.planResumeStep),
  };
  return `${JSON.stringify(payload)}\n`;
}

function renderStatus(planRel: string, environment: CommandEnvironment): CommandResult {
  const state = loadExecutionState(planRel, environment);
  return toCommandResult(0, serializeStatusJson(state));
}

export function deriveEvidenceRelPath(planRel: string, revision: number): string {
  const base = path.posix.basename(planRel, '.md');
  return `docs/superpowers/execution-evidence/${base}-r${revision}-evidence.md`;
}

export function deriveTasksIndependentFromPlan(planText: string): 'yes' | 'no' | 'unknown' {
  const taskScopes: string[][] = [];
  const taskWriteCounts: number[] = [];
  const taskFileBlocks: boolean[] = [];
  const taskScopeParseable: boolean[] = [];
  let currentTaskIndex = -1;
  let inFilesBlock = false;

  for (const line of splitNormalizedLines(planText)) {
    if (inFilesBlock) {
      let match = line.match(/^- (Create|Modify|Delete): `(.+)`$/);
      if (match) {
        taskFileBlocks[currentTaskIndex] = true;
        const normalizedPath = normalizePlanScopePath(match[2] ?? '');
        if (normalizedPath === null) {
          taskScopeParseable[currentTaskIndex] = false;
        } else {
          taskWriteCounts[currentTaskIndex] += 1;
          taskScopes[currentTaskIndex].push(normalizedPath);
        }
        continue;
      }

      if (/^- Test: `.+`$/.test(line)) {
        taskFileBlocks[currentTaskIndex] = true;
        continue;
      }

      if (/^- \[[ x]\] /.test(line)) {
        inFilesBlock = false;
      } else if (/^- /.test(line)) {
        taskFileBlocks[currentTaskIndex] = true;
        taskScopeParseable[currentTaskIndex] = false;
        continue;
      }

      if (line.trim().length === 0) {
        continue;
      }

      inFilesBlock = false;
    }

    const taskMatch = line.match(/^## Task ([0-9]+):/);
    if (taskMatch) {
      currentTaskIndex += 1;
      taskScopes[currentTaskIndex] = [];
      taskWriteCounts[currentTaskIndex] = 0;
      taskFileBlocks[currentTaskIndex] = false;
      taskScopeParseable[currentTaskIndex] = true;
      inFilesBlock = false;
      continue;
    }

    if (currentTaskIndex >= 0 && line === '**Files:**') {
      taskFileBlocks[currentTaskIndex] = true;
      inFilesBlock = true;
    }
  }

  if (taskScopes.length < 2) {
    return 'unknown';
  }

  for (let index = 0; index < taskScopes.length; index += 1) {
    if (!taskFileBlocks[index] || !taskScopeParseable[index] || taskWriteCounts[index] === 0) {
      return 'unknown';
    }
  }

  for (let leftIndex = 0; leftIndex < taskScopes.length; leftIndex += 1) {
    const leftScopes = new Set(taskScopes[leftIndex]);
    for (let rightIndex = leftIndex + 1; rightIndex < taskScopes.length; rightIndex += 1) {
      for (const candidate of taskScopes[rightIndex]) {
        if (leftScopes.has(candidate)) {
          return 'no';
        }
      }
    }
  }

  return 'yes';
}

function serializeRecommendJson(payload: RecommendJson): string {
  return `${JSON.stringify(payload)}\n`;
}

function assertExpectedFingerprint(state: LoadedExecutionState, expectedFingerprint: string): void {
  if (expectedFingerprint !== state.executionFingerprint) {
    fail('StaleMutation', 'Execution state changed since the last parsed execution fingerprint.');
  }
}

function findLatestAttemptIndex(state: LoadedExecutionState, task: string, step: string): number {
  let latestIndex = -1;
  let latestNumber = 0;
  state.attempts.forEach((attempt, index) => {
    if (attempt.task === task && attempt.step === step && attempt.number >= latestNumber) {
      latestIndex = index;
      latestNumber = attempt.number;
    }
  });
  return latestIndex;
}

function nextAttemptNumber(state: LoadedExecutionState, task: string, step: string): number {
  const latestIndex = findLatestAttemptIndex(state, task, step);
  if (latestIndex < 0) {
    return 1;
  }
  return (state.attempts[latestIndex]?.number ?? 0) + 1;
}

function appendAttempt(
  state: LoadedExecutionState,
  task: string,
  step: string,
  source: Exclude<ExecutionMode, 'none'>,
  claim: string,
  files: string[],
  verification: string,
): void {
  state.attempts.push({
    task,
    step,
    number: nextAttemptNumber(state, task, step),
    status: 'Completed',
    recordedAt: currentTimestamp(),
    source,
    claim,
    files,
    verification,
    invalidationReason: 'N/A',
  });
}

function invalidateAttempt(
  state: LoadedExecutionState,
  attemptIndex: number,
  source: Exclude<ExecutionMode, 'none'>,
  reason: string,
): void {
  const attempt = state.attempts[attemptIndex];
  if (!attempt) {
    return;
  }
  attempt.status = 'Invalidated';
  attempt.recordedAt = currentTimestamp();
  attempt.source = source;
  attempt.invalidationReason = normalizeWhitespace(reason);
}

function renderEvidenceFile(state: LoadedExecutionState): string {
  const lines: string[] = [];
  const topic = path.posix.basename(state.planRelPath, '.md');

  lines.push(`# Execution Evidence: ${topic}`);
  lines.push('');
  lines.push(`**Plan Path:** ${state.planRelPath}`);
  lines.push(`**Plan Revision:** ${state.planRevision}`);
  lines.push('');
  lines.push('## Step Evidence');

  if (state.attempts.length > 0) {
    lines.push('');
  }

  let wroteSection = false;
  for (const step of state.steps) {
    const attempts = state.attempts.filter(
      (attempt) => attempt.task === step.task && attempt.step === step.step,
    );
    if (attempts.length === 0) {
      continue;
    }
    if (wroteSection) {
      lines.push('');
    }
    wroteSection = true;
    lines.push(`### Task ${step.task} Step ${step.step}`);
    attempts.forEach((attempt, index) => {
      if (index > 0) {
        lines.push('');
      }
      lines.push(`#### Attempt ${attempt.number}`);
      lines.push(`**Status:** ${attempt.status}`);
      lines.push(`**Recorded At:** ${attempt.recordedAt}`);
      lines.push(`**Execution Source:** ${attempt.source}`);
      lines.push(`**Claim:** ${attempt.claim}`);
      lines.push('**Files:**');
      attempt.files.forEach((filePath) => {
        lines.push(`- ${filePath}`);
      });
      lines.push('**Verification:**');
      lines.push(`- ${attempt.verification}`);
      lines.push(`**Invalidation Reason:** ${attempt.invalidationReason}`);
    });
  }

  return `${lines.join('\n')}\n`;
}

function renderPlanFile(state: LoadedExecutionState): string {
  const output: string[] = [];
  let currentTask = '';
  let suppressAdjacent = false;

  for (const line of splitNormalizedLines(state.planText)) {
    if (suppressAdjacent) {
      if (line.length === 0 || /^\s+\*\*Execution Note:\*\*/.test(line)) {
        continue;
      }
      suppressAdjacent = false;
    }

    const executionModeMatch = line.match(/^\*\*Execution Mode:\*\* (.+)$/);
    if (executionModeMatch) {
      output.push(`**Execution Mode:** ${state.planExecutionMode}`);
      continue;
    }

    const taskMatch = line.match(/^## Task ([0-9]+):/);
    if (taskMatch) {
      currentTask = taskMatch[1] ?? '';
      output.push(line);
      continue;
    }

    const stepMatch = line.match(/^- \[([ x])\] \*\*Step ([0-9]+): (.*)\*\*$/);
    if (stepMatch) {
      const stepIndex = findStepIndex(state, currentTask, stepMatch[2] ?? '');
      const step = state.steps[stepIndex];
      if (!step) {
        output.push(line);
        continue;
      }
      output.push(`- [${step.checked ? 'x' : ' '}] **Step ${step.step}: ${step.title}**`);
      if (step.noteState.length > 0) {
        output.push('');
        output.push(`  **Execution Note:** ${step.noteState} - ${step.noteSummary}`);
      }
      suppressAdjacent = true;
      continue;
    }

    output.push(line);
  }

  return `${output.join('\n')}\n`;
}

function writeTempFile(targetPath: string, contents: string): string {
  ensureDirectoryExists(path.dirname(targetPath));
  const tempPath = `${targetPath}.tmp-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  fs.writeFileSync(tempPath, contents, 'utf8');
  return tempPath;
}

function cleanupFile(filePath: string): void {
  if (pathExists(filePath)) {
    fs.rmSync(filePath, { force: true });
  }
}

function commitPlanOnly(state: LoadedExecutionState, operation: string): void {
  const nextPlan = renderPlanFile(state);
  const tempPlanPath = writeTempFile(state.planAbsPath, nextPlan);

  if (state.env.SUPERPOWERS_PLAN_EXECUTION_TEST_FAILPOINT === `${operation}_after_plan_write`) {
    cleanupFile(tempPlanPath);
    fail('EvidenceWriteFailed', `Injected write failure during ${operation}.`);
  }

  try {
    fs.renameSync(tempPlanPath, state.planAbsPath);
  } catch {
    cleanupFile(tempPlanPath);
    fail('EvidenceWriteFailed', 'Could not persist the plan mutation.');
  }
}

function commitPlanAndEvidence(state: LoadedExecutionState, operation: string): void {
  const originalPlan = state.planText;
  const originalEvidenceExists = state.evidenceExists;
  const originalEvidence = originalEvidenceExists ? readTextFileIfExists(state.evidenceAbsPath) : '';
  const nextPlan = renderPlanFile(state);
  const nextEvidence = renderEvidenceFile(state);
  const tempPlanPath = writeTempFile(state.planAbsPath, nextPlan);
  const tempEvidencePath = writeTempFile(state.evidenceAbsPath, nextEvidence);

  if (state.env.SUPERPOWERS_PLAN_EXECUTION_TEST_FAILPOINT === `${operation}_after_plan_write`) {
    cleanupFile(tempPlanPath);
    cleanupFile(tempEvidencePath);
    fail('EvidenceWriteFailed', `Injected write failure during ${operation}.`);
  }

  try {
    fs.renameSync(tempEvidencePath, state.evidenceAbsPath);
  } catch {
    cleanupFile(tempPlanPath);
    cleanupFile(tempEvidencePath);
    try {
      fs.writeFileSync(state.planAbsPath, originalPlan, 'utf8');
    } catch {
      // Best-effort rollback to preserve shell helper semantics.
    }
    fail('EvidenceWriteFailed', 'Could not persist the evidence mutation.');
  }

  try {
    fs.renameSync(tempPlanPath, state.planAbsPath);
  } catch {
    try {
      fs.writeFileSync(state.planAbsPath, originalPlan, 'utf8');
      if (originalEvidenceExists) {
        fs.writeFileSync(state.evidenceAbsPath, originalEvidence, 'utf8');
      } else {
        cleanupFile(state.evidenceAbsPath);
      }
    } catch {
      // Best-effort rollback to preserve shell helper semantics.
    }
    cleanupFile(tempPlanPath);
    fail('EvidenceWriteFailed', 'Could not persist the plan mutation.');
  }
}

function parseDiffNameStatus(output: string, query: string): string | null {
  const tokens = output.split('\0').filter((token) => token.length > 0);
  let index = 0;
  while (index < tokens.length) {
    const status = tokens[index] ?? '';
    index += 1;
    if (status.startsWith('R') || status.startsWith('C')) {
      const pathOne = tokens[index] ?? '';
      const pathTwo = tokens[index + 1] ?? '';
      index += 2;
      if (query === pathOne || query === pathTwo) {
        return pathTwo;
      }
      continue;
    }

    const pathOne = tokens[index] ?? '';
    index += 1;
    if (query === pathOne) {
      return pathOne;
    }
  }
  return null;
}

function resolvePathFromCurrentChangeSet(state: LoadedExecutionState, query: string): string | null {
  const cached = spawnSync('git', ['-C', state.repoRoot, 'diff', '--name-status', '-z', '-M', '--cached'], {
    encoding: 'utf8',
  });
  if (cached.status === 0) {
    const resolved = parseDiffNameStatus(cached.stdout, query);
    if (resolved !== null) {
      return resolved;
    }
  }

  const working = spawnSync('git', ['-C', state.repoRoot, 'diff', '--name-status', '-z', '-M'], {
    encoding: 'utf8',
  });
  if (working.status === 0) {
    const resolved = parseDiffNameStatus(working.stdout, query);
    if (resolved !== null) {
      return resolved;
    }
  }

  const untracked = spawnSync(
    'git',
    ['-C', state.repoRoot, 'ls-files', '--others', '--exclude-standard', '--', query],
    { encoding: 'utf8' },
  );
  if (untracked.status === 0) {
    const first = untracked.stdout.split('\n')[0] ?? '';
    if (first === query) {
      return query;
    }
  }

  return null;
}

function buildFilesEntry(state: LoadedExecutionState, inputs: string[]): string[] {
  if (inputs.length === 0) {
    return ['None (no repo file changed)'];
  }

  const collected = new Set<string>();
  for (const input of inputs) {
    const normalized = normalizeRepoRelativePath(input);
    if (normalized === null) {
      fail(
        'InvalidCommandInput',
        'Evidence file paths must be normalized repo-relative paths inside the repo root.',
      );
    }

    const absolutePath = planRelToAbs(state.repoRoot, normalized);
    if (pathExists(absolutePath) || fs.existsSync(absolutePath)) {
      collected.add(normalized);
      continue;
    }

    const resolved = resolvePathFromCurrentChangeSet(state, normalized);
    if (resolved === null) {
      fail(
        'InvalidCommandInput',
        'Evidence file paths must exist or be represented in the current change set.',
      );
    }
    collected.add(resolved);
  }

  return Array.from(collected).sort((left, right) => left.localeCompare(right));
}

function commandStatus(args: string[], environment: CommandEnvironment): CommandResult {
  let planRel = '';
  for (let index = 0; index < args.length; ) {
    const current = args[index] ?? '';
    if (current === '--plan') {
      const value = args[index + 1];
      if (value === undefined) {
        return errorResult('InvalidCommandInput', 'status requires --plan.');
      }
      planRel = value;
      index += 2;
      continue;
    }
    return errorResult('InvalidCommandInput', `Unknown status argument '${current}'.`);
  }

  if (planRel.length === 0) {
    return errorResult('InvalidCommandInput', 'status requires --plan.');
  }

  try {
    return renderStatus(planRel, environment);
  } catch (error) {
    if (error instanceof PlanExecutionFailure) {
      return errorResult(error.failureClass, error.message);
    }
    throw error;
  }
}

function commandRecommend(args: string[], environment: CommandEnvironment): CommandResult {
  let planRel = '';
  let isolatedAgents: 'yes' | 'no' | 'unknown' = 'unknown';
  let sessionIntent: 'stay' | 'separate' | 'unknown' = 'unknown';
  let workspacePrepared: 'yes' | 'no' | 'unknown' = 'unknown';

  for (let index = 0; index < args.length; ) {
    const current = args[index] ?? '';
    switch (current) {
      case '--plan': {
        const value = args[index + 1];
        if (value === undefined) {
          return errorResult('InvalidCommandInput', 'recommend requires --plan.');
        }
        planRel = value;
        index += 2;
        break;
      }
      case '--isolated-agents': {
        const value = args[index + 1];
        if (value === undefined) {
          return errorResult(
            'InvalidCommandInput',
            'recommend requires a value for --isolated-agents.',
          );
        }
        if (value === 'available') {
          isolatedAgents = 'yes';
        } else if (value === 'unavailable') {
          isolatedAgents = 'no';
        } else {
          return errorResult(
            'InvalidCommandInput',
            'isolated-agents must be available or unavailable.',
          );
        }
        index += 2;
        break;
      }
      case '--session-intent': {
        const value = args[index + 1];
        if (value === undefined) {
          return errorResult(
            'InvalidCommandInput',
            'recommend requires a value for --session-intent.',
          );
        }
        if (value === 'stay' || value === 'separate' || value === 'unknown') {
          sessionIntent = value;
        } else {
          return errorResult(
            'InvalidCommandInput',
            'session-intent must be stay, separate, or unknown.',
          );
        }
        index += 2;
        break;
      }
      case '--workspace-prepared': {
        const value = args[index + 1];
        if (value === undefined) {
          return errorResult(
            'InvalidCommandInput',
            'recommend requires a value for --workspace-prepared.',
          );
        }
        if (value === 'yes' || value === 'no' || value === 'unknown') {
          workspacePrepared = value;
        } else {
          return errorResult(
            'InvalidCommandInput',
            'workspace-prepared must be yes, no, or unknown.',
          );
        }
        index += 2;
        break;
      }
      default:
        return errorResult('InvalidCommandInput', `Unknown recommend argument '${current}'.`);
    }
  }

  if (planRel.length === 0) {
    return errorResult('InvalidCommandInput', 'recommend requires --plan.');
  }

  try {
    const state = loadExecutionState(planRel, environment);
    if (state.executionStarted === 'yes') {
      return errorResult(
        'RecommendAfterExecutionStart',
        'recommend is only valid before execution has started for this plan revision.',
      );
    }

    const tasksIndependent = deriveTasksIndependentFromPlan(state.planText);
    let sameSessionViable: 'yes' | 'no' | 'unknown' = 'unknown';
    if (sessionIntent === 'stay' && workspacePrepared === 'yes') {
      sameSessionViable = 'yes';
    } else if (sessionIntent === 'separate' || workspacePrepared === 'no') {
      sameSessionViable = 'no';
    }

    const recommendedSkill =
      tasksIndependent === 'yes' && isolatedAgents === 'yes' && sameSessionViable === 'yes'
        ? 'superpowers:subagent-driven-development'
        : 'superpowers:executing-plans';
    const reason =
      recommendedSkill === 'superpowers:subagent-driven-development'
        ? 'Independent tasks and same-session isolated execution are viable.'
        : 'Defaulting conservatively because the available signals do not positively justify isolated same-session execution.';

    return toCommandResult(
      0,
      serializeRecommendJson({
        recommended_skill: recommendedSkill,
        reason,
        decision_flags: {
          tasks_independent: tasksIndependent,
          isolated_agents_available: isolatedAgents,
          session_intent: sessionIntent,
          workspace_prepared: workspacePrepared,
          same_session_viable: sameSessionViable,
        },
      }),
    );
  } catch (error) {
    if (error instanceof PlanExecutionFailure) {
      return errorResult(error.failureClass, error.message);
    }
    throw error;
  }
}

function commandBegin(args: string[], environment: CommandEnvironment): CommandResult {
  let planRel = '';
  let task = '';
  let step = '';
  let executionModeInput = '';
  let expectedFingerprint = '';

  for (let index = 0; index < args.length; ) {
    const current = args[index] ?? '';
    switch (current) {
      case '--plan':
        planRel = args[index + 1] ?? '';
        index += 2;
        break;
      case '--task':
        task = args[index + 1] ?? '';
        index += 2;
        break;
      case '--step':
        step = args[index + 1] ?? '';
        index += 2;
        break;
      case '--execution-mode':
        executionModeInput = args[index + 1] ?? '';
        index += 2;
        break;
      case '--expect-execution-fingerprint':
        expectedFingerprint = args[index + 1] ?? '';
        index += 2;
        break;
      default:
        return errorResult('InvalidCommandInput', `Unknown begin argument '${current}'.`);
    }
  }

  if (planRel.length === 0 || task.length === 0 || step.length === 0 || expectedFingerprint.length === 0) {
    return errorResult(
      'InvalidCommandInput',
      'begin requires --plan, --task, --step, and --expect-execution-fingerprint.',
    );
  }

  try {
    const state = loadExecutionState(planRel, environment);
    assertExpectedFingerprint(state, expectedFingerprint);
    const stepIndex = findStepOrFail(state, task, step);
    const targetStep = state.steps[stepIndex];

    if (targetStep.checked) {
      return errorResult('InvalidStepTransition', 'begin may not target a completed step.');
    }

    if (state.planExecutionMode === 'none') {
      if (
        executionModeInput !== 'superpowers:executing-plans' &&
        executionModeInput !== 'superpowers:subagent-driven-development'
      ) {
        return errorResult(
          'InvalidExecutionMode',
          'The first begin for a plan revision must supply a valid execution mode.',
        );
      }
      state.planExecutionMode = executionModeInput;
    } else if (executionModeInput.length > 0 && executionModeInput !== state.planExecutionMode) {
      return errorResult('InvalidExecutionMode', 'begin may not change the persisted execution mode.');
    }

    if (state.planActiveTask !== null || state.planBlockingTask !== null || state.planResumeTask !== null) {
      if (state.planActiveTask === task && state.planActiveStep === step) {
        return renderStatus(state.planRelPath, environment);
      }
      if (
        state.planBlockingTask !== null &&
        (state.planBlockingTask !== task || state.planBlockingStep !== step)
      ) {
        return errorResult('InvalidStepTransition', 'Blocked work must resume on the same step.');
      }
      if (
        state.planResumeTask !== null &&
        state.planActiveTask === null &&
        state.planBlockingTask === null &&
        (state.planResumeTask !== task || state.planResumeStep !== step)
      ) {
        return errorResult('InvalidStepTransition', 'Interrupted work must resume on the same step.');
      }
      if (
        state.planActiveTask !== null &&
        (state.planActiveTask !== task || state.planActiveStep !== step)
      ) {
        return errorResult('InvalidStepTransition', 'A different step is already active.');
      }
      if (
        state.planBlockingTask === task &&
        state.planBlockingStep === step
      ) {
        // Resume blocked work on the same step.
      } else if (
        state.planResumeTask === task &&
        state.planResumeStep === step
      ) {
        // Resume interrupted work on the same step.
      } else if (state.planActiveTask !== task || state.planActiveStep !== step) {
        return errorResult(
          'InvalidStepTransition',
          'begin may not bypass existing blocked or interrupted work.',
        );
      }
    }

    targetStep.noteState = 'Active';
    targetStep.noteSummary = activeSummaryFromTitle(targetStep.title);
    commitPlanOnly(state, 'begin');
    return renderStatus(state.planRelPath, environment);
  } catch (error) {
    if (error instanceof PlanExecutionFailure) {
      return errorResult(error.failureClass, error.message);
    }
    throw error;
  }
}

function commandTransfer(args: string[], environment: CommandEnvironment): CommandResult {
  let planRel = '';
  let repairTask = '';
  let repairStep = '';
  let source = '';
  let reason = '';
  let expectedFingerprint = '';

  for (let index = 0; index < args.length; ) {
    const current = args[index] ?? '';
    switch (current) {
      case '--plan':
        planRel = args[index + 1] ?? '';
        index += 2;
        break;
      case '--repair-task':
        repairTask = args[index + 1] ?? '';
        index += 2;
        break;
      case '--repair-step':
        repairStep = args[index + 1] ?? '';
        index += 2;
        break;
      case '--source':
        source = args[index + 1] ?? '';
        index += 2;
        break;
      case '--reason':
        reason = args[index + 1] ?? '';
        index += 2;
        break;
      case '--expect-execution-fingerprint':
        expectedFingerprint = args[index + 1] ?? '';
        index += 2;
        break;
      default:
        return errorResult('InvalidCommandInput', `Unknown transfer argument '${current}'.`);
    }
  }

  if (
    planRel.length === 0 ||
    repairTask.length === 0 ||
    repairStep.length === 0 ||
    source.length === 0 ||
    reason.length === 0 ||
    expectedFingerprint.length === 0
  ) {
    return errorResult(
      'InvalidCommandInput',
      'transfer requires --plan, --repair-task, --repair-step, --source, --reason, and --expect-execution-fingerprint.',
    );
  }

  try {
    const normalizedReason = validateRequiredNormalizedText(
      reason,
      'Transfer reasons may not be blank after whitespace normalization.',
    );
    const state = loadExecutionState(planRel, environment);
    assertExpectedFingerprint(state, expectedFingerprint);
    const normalizedSource = validateExecutionModeSource(source, state);

    if (state.planActiveTask === null || state.planActiveStep === null) {
      return errorResult('InvalidStepTransition', 'transfer requires a current active step.');
    }
    if (state.planResumeTask !== null) {
      return errorResult(
        'InvalidStepTransition',
        'transfer may not create a second parked interrupted step.',
      );
    }

    const activeIndex = findStepOrFail(state, state.planActiveTask, state.planActiveStep);
    const repairIndex = findStepOrFail(state, repairTask, repairStep);
    if (!state.steps[repairIndex]?.checked) {
      return errorResult(
        'InvalidStepTransition',
        'transfer may only reopen a currently completed repair step.',
      );
    }

    const latestAttemptIndex = findLatestAttemptIndex(state, repairTask, repairStep);
    if (latestAttemptIndex < 0) {
      return errorResult(
        'EvidenceWriteFailed',
        'transfer could not find evidence to invalidate for the repair step.',
      );
    }

    invalidateAttempt(state, latestAttemptIndex, normalizedSource, normalizedReason);
    state.steps[activeIndex].noteState = 'Interrupted';
    state.steps[activeIndex].noteSummary = `Parked for repair of Task ${repairTask} Step ${repairStep}`;
    state.steps[repairIndex].checked = false;
    state.steps[repairIndex].noteState = 'Active';
    state.steps[repairIndex].noteSummary = activeSummaryFromTitle(state.steps[repairIndex].title);

    commitPlanAndEvidence(state, 'transfer');
    return renderStatus(state.planRelPath, environment);
  } catch (error) {
    if (error instanceof PlanExecutionFailure) {
      return errorResult(error.failureClass, error.message);
    }
    throw error;
  }
}

function commandComplete(args: string[], environment: CommandEnvironment): CommandResult {
  let planRel = '';
  let task = '';
  let step = '';
  let source = '';
  let claim = '';
  const files: string[] = [];
  let verifyCommand = '';
  let verifyResult = '';
  let manualSummary = '';
  let expectedFingerprint = '';

  for (let index = 0; index < args.length; ) {
    const current = args[index] ?? '';
    switch (current) {
      case '--plan':
        planRel = args[index + 1] ?? '';
        index += 2;
        break;
      case '--task':
        task = args[index + 1] ?? '';
        index += 2;
        break;
      case '--step':
        step = args[index + 1] ?? '';
        index += 2;
        break;
      case '--source':
        source = args[index + 1] ?? '';
        index += 2;
        break;
      case '--claim':
        claim = args[index + 1] ?? '';
        index += 2;
        break;
      case '--file':
        files.push(args[index + 1] ?? '');
        index += 2;
        break;
      case '--verify-command':
        verifyCommand = args[index + 1] ?? '';
        index += 2;
        break;
      case '--verify-result':
        verifyResult = args[index + 1] ?? '';
        index += 2;
        break;
      case '--manual-verify-summary':
        manualSummary = args[index + 1] ?? '';
        index += 2;
        break;
      case '--expect-execution-fingerprint':
        expectedFingerprint = args[index + 1] ?? '';
        index += 2;
        break;
      default:
        return errorResult('InvalidCommandInput', `Unknown complete argument '${current}'.`);
    }
  }

  if (
    planRel.length === 0 ||
    task.length === 0 ||
    step.length === 0 ||
    source.length === 0 ||
    claim.length === 0 ||
    expectedFingerprint.length === 0
  ) {
    return errorResult(
      'InvalidCommandInput',
      'complete requires --plan, --task, --step, --source, --claim, and --expect-execution-fingerprint.',
    );
  }

  try {
    const normalizedClaim = validateRequiredNormalizedText(
      claim,
      'Completion claims may not be blank after whitespace normalization.',
    );

    let verificationEntry = '';
    if (manualSummary.length > 0 && (verifyCommand.length > 0 || verifyResult.length > 0)) {
      return errorResult('InvalidCommandInput', 'complete accepts exactly one verification mode.');
    }
    if (verifyCommand.length > 0 || verifyResult.length > 0) {
      if (verifyCommand.length === 0 || verifyResult.length === 0) {
        return errorResult(
          'InvalidCommandInput',
          'Command verification requires both --verify-command and --verify-result.',
        );
      }
      const normalizedVerifyCommand = validateRequiredNormalizedText(
        verifyCommand,
        'Verification commands may not be blank after whitespace normalization.',
      );
      const normalizedVerifyResult = validateRequiredNormalizedText(
        verifyResult,
        'Verification results may not be blank after whitespace normalization.',
      );
      verificationEntry = `\`${normalizedVerifyCommand}\` -> ${normalizedVerifyResult}`;
    } else if (manualSummary.length > 0) {
      const normalizedManualSummary = validateRequiredNormalizedText(
        manualSummary,
        'Manual verification summaries may not be blank after whitespace normalization.',
      );
      verificationEntry = `Manual inspection only: ${normalizedManualSummary}`;
    } else {
      return errorResult('InvalidCommandInput', 'complete requires exactly one verification mode.');
    }

    const state = loadExecutionState(planRel, environment);
    assertExpectedFingerprint(state, expectedFingerprint);
    const normalizedSource = validateExecutionModeSource(source, state);
    const stepIndex = findStepOrFail(state, task, step);

    if (state.planActiveTask !== task || state.planActiveStep !== step) {
      return errorResult('InvalidStepTransition', 'complete may target only the current active step.');
    }
    if (state.steps[stepIndex]?.checked) {
      return errorResult(
        'InvalidStepTransition',
        'complete may not directly refresh an already checked step.',
      );
    }

    const filesEntry = buildFilesEntry(state, files);
    state.steps[stepIndex].checked = true;
    state.steps[stepIndex].noteState = '';
    state.steps[stepIndex].noteSummary = '';
    appendAttempt(state, task, step, normalizedSource, normalizedClaim, filesEntry, verificationEntry);

    commitPlanAndEvidence(state, 'complete');
    return renderStatus(state.planRelPath, environment);
  } catch (error) {
    if (error instanceof PlanExecutionFailure) {
      return errorResult(error.failureClass, error.message);
    }
    throw error;
  }
}

function commandNote(args: string[], environment: CommandEnvironment): CommandResult {
  let planRel = '';
  let task = '';
  let step = '';
  let noteState = '';
  let message = '';
  let expectedFingerprint = '';

  for (let index = 0; index < args.length; ) {
    const current = args[index] ?? '';
    switch (current) {
      case '--plan':
        planRel = args[index + 1] ?? '';
        index += 2;
        break;
      case '--task':
        task = args[index + 1] ?? '';
        index += 2;
        break;
      case '--step':
        step = args[index + 1] ?? '';
        index += 2;
        break;
      case '--state':
        noteState = args[index + 1] ?? '';
        index += 2;
        break;
      case '--message':
        message = args[index + 1] ?? '';
        index += 2;
        break;
      case '--expect-execution-fingerprint':
        expectedFingerprint = args[index + 1] ?? '';
        index += 2;
        break;
      default:
        return errorResult('InvalidCommandInput', `Unknown note argument '${current}'.`);
    }
  }

  if (
    planRel.length === 0 ||
    task.length === 0 ||
    step.length === 0 ||
    noteState.length === 0 ||
    message.length === 0 ||
    expectedFingerprint.length === 0
  ) {
    return errorResult(
      'InvalidCommandInput',
      'note requires --plan, --task, --step, --state, --message, and --expect-execution-fingerprint.',
    );
  }
  if (noteState !== 'interrupted' && noteState !== 'blocked') {
    return errorResult('InvalidCommandInput', 'note state must be interrupted or blocked.');
  }

  try {
    const normalizedMessage = validateNoteMessage(message);
    const state = loadExecutionState(planRel, environment);
    assertExpectedFingerprint(state, expectedFingerprint);
    const stepIndex = findStepOrFail(state, task, step);

    if (state.planActiveTask === task && state.planActiveStep === step) {
      if (noteState === 'interrupted' && state.planResumeTask !== null) {
        return errorResult(
          'InvalidStepTransition',
          'The current repair step cannot become interrupted while a parked step already exists.',
        );
      }
    } else if (
      noteState === 'blocked' &&
      state.planActiveTask === null &&
      state.planResumeTask === task &&
      state.planResumeStep === step
    ) {
      // Allowed blocked follow-up after reopen.
    } else {
      return errorResult(
        'InvalidStepTransition',
        'note may target only the current active step, except blocked follow-up after reopen.',
      );
    }

    state.steps[stepIndex].noteState = noteState === 'interrupted' ? 'Interrupted' : 'Blocked';
    state.steps[stepIndex].noteSummary = normalizedMessage;
    commitPlanOnly(state, 'note');
    return renderStatus(state.planRelPath, environment);
  } catch (error) {
    if (error instanceof PlanExecutionFailure) {
      return errorResult(error.failureClass, error.message);
    }
    throw error;
  }
}

function commandReopen(args: string[], environment: CommandEnvironment): CommandResult {
  let planRel = '';
  let task = '';
  let step = '';
  let source = '';
  let reason = '';
  let expectedFingerprint = '';

  for (let index = 0; index < args.length; ) {
    const current = args[index] ?? '';
    switch (current) {
      case '--plan':
        planRel = args[index + 1] ?? '';
        index += 2;
        break;
      case '--task':
        task = args[index + 1] ?? '';
        index += 2;
        break;
      case '--step':
        step = args[index + 1] ?? '';
        index += 2;
        break;
      case '--source':
        source = args[index + 1] ?? '';
        index += 2;
        break;
      case '--reason':
        reason = args[index + 1] ?? '';
        index += 2;
        break;
      case '--expect-execution-fingerprint':
        expectedFingerprint = args[index + 1] ?? '';
        index += 2;
        break;
      default:
        return errorResult('InvalidCommandInput', `Unknown reopen argument '${current}'.`);
    }
  }

  if (
    planRel.length === 0 ||
    task.length === 0 ||
    step.length === 0 ||
    source.length === 0 ||
    reason.length === 0 ||
    expectedFingerprint.length === 0
  ) {
    return errorResult(
      'InvalidCommandInput',
      'reopen requires --plan, --task, --step, --source, --reason, and --expect-execution-fingerprint.',
    );
  }

  try {
    const normalizedReason = validateRequiredNormalizedText(
      reason,
      'Reopen reasons may not be blank after whitespace normalization.',
    );
    const state = loadExecutionState(planRel, environment);
    assertExpectedFingerprint(state, expectedFingerprint);
    const normalizedSource = validateExecutionModeSource(source, state);
    const stepIndex = findStepOrFail(state, task, step);

    if (!state.steps[stepIndex]?.checked) {
      return errorResult('InvalidStepTransition', 'reopen may target only a currently completed step.');
    }

    const latestAttemptIndex = findLatestAttemptIndex(state, task, step);
    if (latestAttemptIndex < 0) {
      return errorResult(
        'EvidenceWriteFailed',
        'reopen could not find evidence to invalidate for the target step.',
      );
    }

    invalidateAttempt(state, latestAttemptIndex, normalizedSource, normalizedReason);
    state.steps[stepIndex].checked = false;
    state.steps[stepIndex].noteState = 'Interrupted';
    state.steps[stepIndex].noteSummary = reopenNoteSummary(normalizedReason);

    commitPlanAndEvidence(state, 'reopen');
    return renderStatus(state.planRelPath, environment);
  } catch (error) {
    if (error instanceof PlanExecutionFailure) {
      return errorResult(error.failureClass, error.message);
    }
    throw error;
  }
}

export function runPlanExecutionCommand(
  args: string[],
  environment: CommandEnvironment = {},
): CommandResult {
  const [command, ...rest] = args;
  switch (command) {
    case 'status':
      return commandStatus(rest, environment);
    case 'recommend':
      return commandRecommend(rest, environment);
    case 'begin':
      return commandBegin(rest, environment);
    case 'transfer':
      return commandTransfer(rest, environment);
    case 'complete':
      return commandComplete(rest, environment);
    case 'note':
      return commandNote(rest, environment);
    case 'reopen':
      return commandReopen(rest, environment);
    case undefined:
    case '':
    case '-h':
    case '--help':
    case 'help':
      return usageResult();
    default:
      return errorResult('InvalidCommandInput', `Unknown subcommand '${command}'.`);
  }
}
