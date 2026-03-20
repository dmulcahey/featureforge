import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import {
  ensureDirectoryExists,
  isFilePath,
  listNewestFiles,
  movePath,
  pathExists,
  readTextFileIfExists,
  writeTextFileAtomic,
} from '../platform/filesystem';
import { isPathInsideRoot, normalizeRelativePath, resolveFromRuntimeRoot, resolveStateDir } from '../platform/paths';

type WorkflowStatusCode =
  | 'needs_brainstorming'
  | 'spec_draft'
  | 'spec_approved_needs_plan'
  | 'plan_draft'
  | 'stale_plan'
  | 'implementation_ready';

type ResolveMode = 'mutating' | 'read_only';

type CommandEnvironment = {
  cwd?: string;
  env?: NodeJS.ProcessEnv;
  runtimeRoot?: string;
};

export type CommandResult = {
  exitCode: number;
  stdout: string;
  stderr: string;
};

export type ParsedSpecHeaders = {
  state: 'Draft' | 'CEO Approved';
  revision: string;
  reviewer: 'brainstorming' | 'plan-ceo-review';
};

export type ParsedPlanHeaders = {
  state: 'Draft' | 'Engineering Approved';
  sourceSpec: string;
  sourceRevision: string;
  reviewer: 'writing-plans' | 'plan-eng-review';
};

export type WorkflowClassificationInput = {
  resolvedSpecPath: string;
  specExists: boolean;
  specHeaders: ParsedSpecHeaders | null;
  resolvedPlanPath: string;
  planExists: boolean;
  planHeaders: ParsedPlanHeaders | null;
  specAmbiguous: boolean;
  specExpectedMissing: boolean;
  planAmbiguous: boolean;
  planExpectedMissing: boolean;
};

export type WorkflowClassification = {
  statusCode: WorkflowStatusCode;
  nextSkill: string;
  reason: string;
};

type ManifestReadResult = 'ok' | 'missing' | 'corrupt';

type ResolveJson = {
  outcome: 'resolved';
  status: WorkflowStatusCode;
  next_skill: string;
  spec_path: string;
  plan_path: string;
  manifest_path: string;
  manifest_source_path: string;
  root: string;
  reason?: string;
  note?: string;
};

type ResolveFailureJson = {
  outcome: 'runtime_failure';
  failure_class: string;
  message: string;
  manifest_path: string;
  root: string;
};

type StatusJson = {
  status: WorkflowStatusCode;
  next_skill: string;
  spec_path: string;
  plan_path: string;
  manifest_path: string;
  root: string;
  reason?: string;
  note?: string;
};

type RuntimeSlug = {
  slug: string;
  safeBranch: string;
};

function normalizeLineEndings(text: string): string {
  return text.replace(/\r\n/g, '\n');
}

function firstMatch(text: string, pattern: RegExp): string {
  for (const line of normalizeLineEndings(text).split('\n')) {
    const match = line.match(pattern);
    if (match) {
      return match[1] ?? '';
    }
  }

  return '';
}

function parseStateString(value: string, allowed: readonly string[]): string | null {
  return allowed.includes(value) ? value : null;
}

export function normalizeRepoRelativePath(input: string): string | null {
  return normalizeRelativePath(input);
}

export function parseSpecHeaders(text: string): ParsedSpecHeaders | null {
  const state = parseStateString(
    firstMatch(text, /^\*\*Workflow State:\*\* (Draft|CEO Approved)$/),
    ['Draft', 'CEO Approved'],
  );
  const revision = firstMatch(text, /^\*\*Spec Revision:\*\* ([0-9]+)$/);
  const reviewer = parseStateString(
    firstMatch(text, /^\*\*Last Reviewed By:\*\* (brainstorming|plan-ceo-review)$/),
    ['brainstorming', 'plan-ceo-review'],
  );

  if (state === null || revision.length === 0 || reviewer === null) {
    return null;
  }

  return {
    state: state as ParsedSpecHeaders['state'],
    revision,
    reviewer: reviewer as ParsedSpecHeaders['reviewer'],
  };
}

export function parsePlanHeaders(text: string): ParsedPlanHeaders | null {
  const state = parseStateString(
    firstMatch(text, /^\*\*Workflow State:\*\* (Draft|Engineering Approved)$/),
    ['Draft', 'Engineering Approved'],
  );
  const rawSourceSpec = firstMatch(text, /^\*\*Source Spec:\*\* (.+)$/);
  const sourceRevision = firstMatch(text, /^\*\*Source Spec Revision:\*\* ([0-9]+)$/);
  const reviewer = parseStateString(
    firstMatch(text, /^\*\*Last Reviewed By:\*\* (writing-plans|plan-eng-review)$/),
    ['writing-plans', 'plan-eng-review'],
  );

  const sourceSpec = rawSourceSpec.replace(/^`/, '').replace(/`$/, '');
  if (state === null || sourceSpec.length === 0 || sourceRevision.length === 0 || reviewer === null) {
    return null;
  }

  return {
    state: state as ParsedPlanHeaders['state'],
    sourceSpec,
    sourceRevision,
    reviewer: reviewer as ParsedPlanHeaders['reviewer'],
  };
}

export function classifyWorkflowState(input: WorkflowClassificationInput): WorkflowClassification {
  const reasons: string[] = [];

  if (input.specAmbiguous) {
    reasons.push('fallback_ambiguity_spec');
    return {
      statusCode: 'spec_draft',
      nextSkill: 'superpowers:plan-ceo-review',
      reason: reasons.join(','),
    };
  }

  if (input.resolvedSpecPath.length === 0) {
    if (input.specExpectedMissing) {
      reasons.push('missing_expected_spec');
    }
    return {
      statusCode: 'needs_brainstorming',
      nextSkill: 'superpowers:brainstorming',
      reason: reasons.join(','),
    };
  }

  if (!input.specExists) {
    if (input.specExpectedMissing) {
      reasons.push('missing_expected_spec');
    }
    return {
      statusCode: 'needs_brainstorming',
      nextSkill: 'superpowers:brainstorming',
      reason: reasons.join(','),
    };
  }

  if (input.specHeaders === null) {
    reasons.push('malformed_spec_headers');
    return {
      statusCode: 'spec_draft',
      nextSkill: 'superpowers:plan-ceo-review',
      reason: reasons.join(','),
    };
  }

  if (input.specHeaders.state === 'Draft') {
    return {
      statusCode: 'spec_draft',
      nextSkill: 'superpowers:plan-ceo-review',
      reason: reasons.join(','),
    };
  }

  if (input.planAmbiguous) {
    reasons.push('fallback_ambiguity_plan');
    return {
      statusCode: 'spec_approved_needs_plan',
      nextSkill: 'superpowers:writing-plans',
      reason: reasons.join(','),
    };
  }

  if (input.resolvedPlanPath.length === 0) {
    if (input.planExpectedMissing) {
      reasons.push('missing_expected_plan');
    }
    return {
      statusCode: 'spec_approved_needs_plan',
      nextSkill: 'superpowers:writing-plans',
      reason: reasons.join(','),
    };
  }

  if (!input.planExists) {
    if (input.planExpectedMissing) {
      reasons.push('missing_expected_plan');
    }
    return {
      statusCode: 'spec_approved_needs_plan',
      nextSkill: 'superpowers:writing-plans',
      reason: reasons.join(','),
    };
  }

  if (input.planHeaders === null) {
    reasons.push('malformed_plan_headers');
    return {
      statusCode: 'plan_draft',
      nextSkill: 'superpowers:plan-eng-review',
      reason: reasons.join(','),
    };
  }

  if (input.planHeaders.state === 'Draft') {
    return {
      statusCode: 'plan_draft',
      nextSkill: 'superpowers:plan-eng-review',
      reason: reasons.join(','),
    };
  }

  const normalizedSource = normalizeRepoRelativePath(input.planHeaders.sourceSpec);
  if (
    normalizedSource === null ||
    normalizedSource !== input.resolvedSpecPath ||
    input.planHeaders.sourceRevision !== input.specHeaders.revision
  ) {
    return {
      statusCode: 'stale_plan',
      nextSkill: 'superpowers:writing-plans',
      reason: '',
    };
  }

  reasons.push('implementation_ready');
  return {
    statusCode: 'implementation_ready',
    nextSkill: '',
    reason: reasons.join(','),
  };
}

class WorkflowStatusRunner {
  private readonly cwd: string;

  private readonly env: NodeJS.ProcessEnv;

  private readonly runtimeRoot: string;

  private readonly stateDir: string;

  private readonly repoRoot: string;

  private readonly repoContextAvailable: boolean;

  private readonly branch: string;

  private readonly safeBranch: string;

  private readonly slug: string;

  private readonly userName: string;

  private readonly manifestPath: string;

  private readonly fallbackLimit: number;

  private readonly recoveryCandidateLimit = 12;

  private readonly stdoutChunks: string[] = [];

  private readonly stderrChunks: string[] = [];

  private manifestSpecPath = '';

  private manifestPlanPath = '';

  private manifestStatus = '';

  private manifestNextSkill = '';

  private manifestReason = '';

  private manifestRepoRoot = '';

  private manifestBranch = '';

  private manifestSourcePath = '';

  private manifestValid = false;

  private manifestCorrupt = false;

  private manifestRecovered = false;

  private manifestRecoveryAmbiguous = false;

  private manifestIdentityMismatch = false;

  private manifestIdentityReason = '';

  private resolvedSpecPath = '';

  private resolvedPlanPath = '';

  private specHeaders: ParsedSpecHeaders | null = null;

  private planHeaders: ParsedPlanHeaders | null = null;

  private statusCode: WorkflowStatusCode = 'needs_brainstorming';

  private nextSkill = 'superpowers:brainstorming';

  private statusNotes: string[] = [];

  private specFromManifest = false;

  private specExpectedMissing = false;

  private specAmbiguous = false;

  private planFromManifest = false;

  private planExpectedMissing = false;

  private planAmbiguous = false;

  constructor(options: CommandEnvironment) {
    this.cwd = path.resolve(options.cwd ?? process.cwd());
    this.env = options.env ?? process.env;
    this.runtimeRoot = options.runtimeRoot ?? this.cwd;
    this.stateDir = resolveStateDir(this.env);

    const repoRootResult = this.runGitCommand(['rev-parse', '--show-toplevel']);
    if (repoRootResult.success) {
      this.repoRoot = repoRootResult.stdout;
      this.repoContextAvailable = true;
    } else {
      this.repoRoot = this.cwd;
      this.repoContextAvailable = false;
    }

    const rawBranch = this.readBranchName();
    this.branch = rawBranch;
    this.userName = this.readUserName();
    this.fallbackLimit = this.readFallbackLimit();

    const runtimeSlug = this.readRuntimeSlug(rawBranch);
    this.slug = runtimeSlug.slug;
    this.safeBranch = runtimeSlug.safeBranch;
    this.manifestPath = path.join(
      this.stateDir,
      'projects',
      this.slug,
      `${this.userName}-${this.safeBranch}-workflow-state.json`,
    );
  }

  run(args: string[]): CommandResult {
    const [command, ...commandArgs] = args;

    switch (command) {
      case 'status':
        return this.commandStatus(commandArgs);
      case 'resolve':
        return this.commandResolve(commandArgs);
      case 'expect':
        return this.commandExpect(commandArgs);
      case 'sync':
        return this.commandSync(commandArgs);
      default:
        this.writeStdout(this.usageText());
        return this.result(1);
    }
  }

  private result(exitCode: number): CommandResult {
    return {
      exitCode,
      stdout: this.stdoutChunks.join(''),
      stderr: this.stderrChunks.join(''),
    };
  }

  private usageText(): string {
    return 'Usage: superpowers-workflow-status {status|resolve|expect|sync} ...\n';
  }

  private writeStdout(text: string): void {
    this.stdoutChunks.push(text);
  }

  private writeStderr(text: string): void {
    this.stderrChunks.push(text);
  }

  private appendStatusNote(note: string): void {
    if (note.length > 0) {
      this.statusNotes.push(note);
    }
  }

  private setStatusNotes(noteText: string): void {
    this.statusNotes = noteText.length > 0 ? [noteText] : [];
  }

  private currentReason(): string {
    return this.statusNotes.join(',');
  }

  private emitStatusJson(statusCode: WorkflowStatusCode, nextSkill: string, specPath: string, planPath: string, reason = ''): void {
    const payload: StatusJson = {
      status: statusCode,
      next_skill: nextSkill,
      spec_path: specPath,
      plan_path: planPath,
      manifest_path: this.manifestPath,
      root: this.repoRoot,
    };
    if (reason.length > 0) {
      payload.reason = reason;
      payload.note = reason;
    }

    this.writeStdout(`${JSON.stringify(payload)}\n`);
  }

  private emitResolveJson(statusCode: WorkflowStatusCode, nextSkill: string, specPath: string, planPath: string, reason = ''): void {
    const payload: ResolveJson = {
      outcome: 'resolved',
      status: statusCode,
      next_skill: nextSkill,
      spec_path: specPath,
      plan_path: planPath,
      manifest_path: this.manifestPath,
      manifest_source_path: this.manifestSourcePath,
      root: this.repoRoot,
    };
    if (reason.length > 0) {
      payload.reason = reason;
      payload.note = reason;
    }

    this.writeStdout(`${JSON.stringify(payload)}\n`);
  }

  private emitResolveFailure(failureClass: string, message: string): void {
    const payload: ResolveFailureJson = {
      outcome: 'runtime_failure',
      failure_class: failureClass,
      message,
      manifest_path: this.manifestPath,
      root: this.repoRoot,
    };
    this.writeStdout(`${JSON.stringify(payload)}\n`);
  }

  private emitSummary(statusCode: WorkflowStatusCode, nextSkill: string, specPath: string, planPath: string, reason = ''): void {
    const nextDisplay =
      statusCode === 'implementation_ready' && nextSkill.length === 0 ? 'execution_handoff' : nextSkill;
    this.writeStdout(
      `status=${statusCode} next=${nextDisplay} spec=${specPath} plan=${planPath} reason=${reason}\n`,
    );
  }

  private ensureManifestDir(): void {
    ensureDirectoryExists(path.dirname(this.manifestPath));
  }

  private readUserName(): string {
    const envUser = this.env.USER ?? this.env.USERNAME;
    if (envUser && envUser.length > 0) {
      return envUser;
    }

    try {
      return os.userInfo().username;
    } catch {
      return 'user';
    }
  }

  private readFallbackLimit(): number {
    const raw = this.env.SUPERPOWERS_WORKFLOW_STATUS_FALLBACK_LIMIT ?? '';
    if (!/^[1-9][0-9]*$/.test(raw)) {
      return 6;
    }

    return Number.parseInt(raw, 10);
  }

  private runGitCommand(args: string[]): { success: boolean; stdout: string } {
    const result = spawnSync('git', args, {
      cwd: this.cwd,
      encoding: 'utf8',
    });

    if (result.status !== 0) {
      return {
        success: false,
        stdout: '',
      };
    }

    return {
      success: true,
      stdout: result.stdout.trim(),
    };
  }

  private readBranchName(): string {
    const branchResult = this.runGitCommand(['rev-parse', '--abbrev-ref', 'HEAD']);
    const rawBranch = branchResult.success && branchResult.stdout.length > 0 ? branchResult.stdout : 'current';
    return rawBranch === 'HEAD' ? 'current' : rawBranch;
  }

  private hashRepoRoot(): string {
    return crypto.createHash('sha256').update(this.repoRoot).digest('hex').slice(0, 12);
  }

  private deriveRepoSlug(remoteUrl: string): string {
    if (remoteUrl.length > 0) {
      const match = remoteUrl.match(/[:/]([^/]+\/[^/]+?)(?:\.git)?$/);
      if (match?.[1]) {
        return match[1].replaceAll('/', '-');
      }
    }

    return `${path.basename(this.repoRoot)}-${this.hashRepoRoot()}`;
  }

  private sanitizeBranch(branch: string): string {
    return branch.replace(/[^A-Za-z0-9._-]/g, '-');
  }

  private readRuntimeSlug(rawBranch: string): RuntimeSlug {
    const helperPath = resolveFromRuntimeRoot(this.runtimeRoot, 'bin/superpowers-slug');
    if (pathExists(helperPath)) {
      const result = spawnSync(helperPath, [], {
        cwd: this.cwd,
        encoding: 'utf8',
      });
      if (result.status === 0) {
        let slug = '';
        let safeBranch = '';

        for (const line of normalizeLineEndings(result.stdout).split('\n')) {
          if (line.startsWith('SLUG=')) {
            slug = line.slice('SLUG='.length);
          }
          if (line.startsWith('BRANCH=')) {
            safeBranch = line.slice('BRANCH='.length);
          }
        }

        if (slug.length > 0 && safeBranch.length > 0) {
          return { slug, safeBranch };
        }
      }
    }

    const remoteUrlResult = this.runGitCommand(['remote', 'get-url', 'origin']);
    return {
      slug: this.deriveRepoSlug(remoteUrlResult.success ? remoteUrlResult.stdout : ''),
      safeBranch: this.sanitizeBranch(rawBranch),
    };
  }

  private resetManifestFields(): void {
    this.manifestSpecPath = '';
    this.manifestPlanPath = '';
    this.manifestStatus = '';
    this.manifestNextSkill = '';
    this.manifestReason = '';
    this.manifestRepoRoot = '';
    this.manifestBranch = '';
    this.manifestSourcePath = '';
    this.manifestValid = false;
    this.manifestCorrupt = false;
  }

  private resetManifestDiagnostics(): void {
    this.manifestRecovered = false;
    this.manifestRecoveryAmbiguous = false;
    this.manifestIdentityMismatch = false;
    this.manifestIdentityReason = '';
  }

  private readManifestFile(filePath: string): ManifestReadResult {
    this.resetManifestFields();

    if (!isFilePath(filePath)) {
      return 'missing';
    }

    const jsonText = readTextFileIfExists(filePath);
    if (jsonText.length === 0) {
      this.manifestCorrupt = true;
      return 'corrupt';
    }

    let parsed: Record<string, unknown>;
    try {
      const value = JSON.parse(jsonText) as unknown;
      if (value === null || Array.isArray(value) || typeof value !== 'object') {
        throw new Error('invalid-manifest');
      }
      parsed = value as Record<string, unknown>;
    } catch {
      this.manifestCorrupt = true;
      return 'corrupt';
    }

    if (parsed.version !== 1) {
      this.manifestCorrupt = true;
      return 'corrupt';
    }

    this.manifestSpecPath = typeof parsed.expected_spec_path === 'string' ? parsed.expected_spec_path : '';
    this.manifestPlanPath = typeof parsed.expected_plan_path === 'string' ? parsed.expected_plan_path : '';
    this.manifestStatus = typeof parsed.status === 'string' ? parsed.status : '';
    this.manifestNextSkill = typeof parsed.next_skill === 'string' ? parsed.next_skill : '';
    this.manifestReason =
      typeof parsed.reason === 'string'
        ? parsed.reason
        : typeof parsed.note === 'string'
          ? parsed.note
          : '';
    this.manifestRepoRoot = typeof parsed.repo_root === 'string' ? parsed.repo_root : '';
    this.manifestBranch = typeof parsed.branch === 'string' ? parsed.branch : '';
    this.manifestSourcePath = filePath;
    this.manifestValid = true;
    return 'ok';
  }

  private readManifest(): ManifestReadResult {
    return this.readManifestFile(this.manifestPath);
  }

  private writeManifest(payload: string): void {
    const tempPath = `${this.manifestPath}.tmp.${process.pid}-${Date.now()}`;
    ensureDirectoryExists(path.dirname(this.manifestPath));
    fs.writeFileSync(tempPath, `${payload}\n`, 'utf8');
    movePath(tempPath, this.manifestPath);
  }

  private writeManifestWithRetry(payload: string): boolean {
    for (let attempt = 1; attempt <= 2; attempt += 1) {
      try {
        this.writeManifest(payload);
        return true;
      } catch {
        if (attempt === 1) {
          this.writeStderr('warning: manifest write conflict, retrying once\n');
        }
      }
    }

    return false;
  }

  private backupCorruptManifest(): void {
    const now = new Date();
    const timestamp = [
      now.getFullYear().toString().padStart(4, '0'),
      (now.getMonth() + 1).toString().padStart(2, '0'),
      now.getDate().toString().padStart(2, '0'),
      '-',
      now.getHours().toString().padStart(2, '0'),
      now.getMinutes().toString().padStart(2, '0'),
      now.getSeconds().toString().padStart(2, '0'),
    ].join('');
    const backupPath = `${this.manifestPath}.corrupt-${timestamp}`;
    movePath(this.manifestPath, backupPath);
    this.writeStderr(`warning: corrupted workflow manifest moved to ${backupPath}\n`);
  }

  private manifestPayload(specPath: string, planPath: string, statusCode: WorkflowStatusCode, nextSkill: string, reason = ''): string {
    const payload: Record<string, unknown> = {
      version: 1,
      repo_root: this.repoRoot,
      branch: this.branch,
      expected_spec_path: specPath,
      expected_plan_path: planPath,
      status: statusCode,
      next_skill: nextSkill,
    };
    if (reason.length > 0) {
      payload.reason = reason;
      payload.note = reason;
    }
    payload.updated_at = new Date().toISOString().replace(/\.\d{3}Z$/, 'Z');

    return JSON.stringify(payload);
  }

  private listNewestCandidates(directoryPath: string): string[] {
    return listNewestFiles(directoryPath, { extension: '.md', limit: this.fallbackLimit });
  }

  private relFromRoot(absolutePath: string): string | null {
    if (!isPathInsideRoot(this.repoRoot, absolutePath)) {
      return null;
    }

    const relativePath = path.relative(this.repoRoot, absolutePath);
    return relativePath.length > 0 ? relativePath.replaceAll(path.sep, '/') : null;
  }

  private repoPathExists(relativePath: string): boolean {
    if (relativePath.length === 0) {
      return false;
    }

    return isFilePath(path.join(this.repoRoot, relativePath));
  }

  private resetResolutionFlags(): void {
    this.specFromManifest = false;
    this.specExpectedMissing = false;
    this.specAmbiguous = false;
    this.planFromManifest = false;
    this.planExpectedMissing = false;
    this.planAmbiguous = false;
    this.resolvedSpecPath = '';
    this.resolvedPlanPath = '';
    this.specHeaders = null;
    this.planHeaders = null;
  }

  private resolvePreferredSpec(): void {
    if (this.manifestSpecPath.length > 0) {
      const normalized = normalizeRepoRelativePath(this.manifestSpecPath);
      if (normalized !== null) {
        this.specFromManifest = true;
        this.manifestSpecPath = normalized;
        const absolutePath = path.join(this.repoRoot, normalized);
        this.resolvedSpecPath = normalized;
        if (isFilePath(absolutePath)) {
          this.specHeaders = parseSpecHeaders(readTextFileIfExists(absolutePath));
          if (this.specHeaders !== null) {
            return;
          }
        } else {
          this.specExpectedMissing = true;
        }
      } else {
        this.appendStatusNote('invalid_manifest_spec_path');
        this.manifestSpecPath = '';
      }
    }

    let firstAny = '';
    let firstValid = '';
    let firstValidHeaders: ParsedSpecHeaders | null = null;
    let validCount = 0;

    for (const absolutePath of this.listNewestCandidates(path.join(this.repoRoot, 'docs/superpowers/specs'))) {
      const localRelative = this.relFromRoot(absolutePath);
      if (localRelative === null) {
        continue;
      }
      if (this.manifestSpecPath.length > 0 && localRelative === this.manifestSpecPath) {
        continue;
      }
      if (firstAny.length === 0) {
        firstAny = localRelative;
      }

      const parsed = parseSpecHeaders(readTextFileIfExists(absolutePath));
      if (parsed !== null) {
        validCount += 1;
        if (firstValid.length === 0) {
          firstValid = localRelative;
          firstValidHeaders = parsed;
        }
      }
    }

    if (validCount > 1) {
      this.specAmbiguous = true;
      return;
    }

    if (validCount === 1 && firstValid.length > 0 && firstValidHeaders !== null) {
      this.resolvedSpecPath = firstValid;
      this.manifestSpecPath = firstValid;
      this.specHeaders = firstValidHeaders;
      return;
    }

    if (firstAny.length > 0) {
      this.resolvedSpecPath = firstAny;
      this.specHeaders = parseSpecHeaders(readTextFileIfExists(path.join(this.repoRoot, firstAny)));
    }
  }

  private resolvePreferredPlan(): void {
    if (this.manifestPlanPath.length > 0) {
      const normalized = normalizeRepoRelativePath(this.manifestPlanPath);
      if (normalized !== null) {
        this.planFromManifest = true;
        this.manifestPlanPath = normalized;
        const absolutePath = path.join(this.repoRoot, normalized);
        this.resolvedPlanPath = normalized;
        if (isFilePath(absolutePath)) {
          this.planHeaders = parsePlanHeaders(readTextFileIfExists(absolutePath));
          if (this.planHeaders !== null) {
            return;
          }
        } else {
          this.planExpectedMissing = true;
        }
      } else {
        this.appendStatusNote('invalid_manifest_plan_path');
        this.manifestPlanPath = '';
      }
    }

    let firstAny = '';
    let firstValid = '';
    let firstValidHeaders: ParsedPlanHeaders | null = null;
    let validCount = 0;

    for (const absolutePath of this.listNewestCandidates(path.join(this.repoRoot, 'docs/superpowers/plans'))) {
      const localRelative = this.relFromRoot(absolutePath);
      if (localRelative === null) {
        continue;
      }
      if (this.manifestPlanPath.length > 0 && localRelative === this.manifestPlanPath) {
        continue;
      }
      if (firstAny.length === 0) {
        firstAny = localRelative;
      }

      const parsed = parsePlanHeaders(readTextFileIfExists(absolutePath));
      if (parsed !== null) {
        validCount += 1;
        if (firstValid.length === 0) {
          firstValid = localRelative;
          firstValidHeaders = parsed;
        }
      }
    }

    if (validCount > 1) {
      this.planAmbiguous = true;
      return;
    }

    if (validCount === 1 && firstValid.length > 0 && firstValidHeaders !== null) {
      this.resolvedPlanPath = firstValid;
      this.manifestPlanPath = firstValid;
      this.planHeaders = firstValidHeaders;
      return;
    }

    if (firstAny.length > 0) {
      this.resolvedPlanPath = firstAny;
      this.planHeaders = parsePlanHeaders(readTextFileIfExists(path.join(this.repoRoot, firstAny)));
    }
  }

  private deriveStatus(): void {
    this.resetResolutionFlags();
    this.statusCode = 'needs_brainstorming';
    this.nextSkill = 'superpowers:brainstorming';
    this.statusNotes = [];

    this.resolvePreferredSpec();
    this.resolvePreferredPlan();

    const classification = classifyWorkflowState({
      resolvedSpecPath: this.resolvedSpecPath,
      specExists: this.resolvedSpecPath.length > 0 && this.repoPathExists(this.resolvedSpecPath),
      specHeaders: this.specHeaders,
      resolvedPlanPath: this.resolvedPlanPath,
      planExists: this.resolvedPlanPath.length > 0 && this.repoPathExists(this.resolvedPlanPath),
      planHeaders: this.planHeaders,
      specAmbiguous: this.specAmbiguous,
      specExpectedMissing: this.specExpectedMissing,
      planAmbiguous: this.planAmbiguous,
      planExpectedMissing: this.planExpectedMissing,
    });

    this.statusCode = classification.statusCode;
    this.nextSkill = classification.nextSkill;
    if (classification.reason.length > 0) {
      this.appendStatusNote(classification.reason);
    }
  }

  private loadManifestOrRecover(): ManifestReadResult {
    this.resetManifestDiagnostics();
    const currentManifest = this.readManifest();
    if (currentManifest === 'ok') {
      if (this.manifestRepoRoot.length > 0 && this.manifestRepoRoot !== this.repoRoot) {
        this.manifestIdentityMismatch = true;
        this.manifestIdentityReason = 'repo_root_mismatch';
        this.writeStderr('warning: workflow manifest repo_root mismatch; rebuilding from current repo context\n');
      } else if (this.manifestBranch.length > 0 && this.manifestBranch !== this.branch) {
        this.manifestIdentityMismatch = true;
        this.manifestIdentityReason = 'branch_mismatch';
        this.writeStderr('warning: workflow manifest branch mismatch; rebuilding from current repo context\n');
      }

      return 'ok';
    }

    if (currentManifest === 'corrupt' || this.manifestCorrupt) {
      this.backupCorruptManifest();
      this.manifestSpecPath = '';
      this.manifestPlanPath = '';
      return 'corrupt';
    }

    const candidateFileName = `${this.userName}-${this.safeBranch}-workflow-state.json`;
    const projectsRoot = path.join(this.stateDir, 'projects');
    let matchedCandidate = '';
    let matchedCount = 0;
    let inspected = 0;

    if (pathExists(projectsRoot)) {
      for (const entry of fs.readdirSync(projectsRoot).sort((left, right) => left.localeCompare(right))) {
        const candidatePath = path.join(projectsRoot, entry, candidateFileName);
        if (candidatePath === this.manifestPath || !isFilePath(candidatePath)) {
          continue;
        }

        inspected += 1;
        if (inspected > this.recoveryCandidateLimit) {
          break;
        }

        if (this.readManifestFile(candidatePath) === 'ok') {
          if (this.manifestRepoRoot === this.repoRoot && this.manifestBranch === this.branch) {
            matchedCandidate = candidatePath;
            matchedCount += 1;
            if (matchedCount > 1) {
              break;
            }
          }
        }
      }
    }

    if (matchedCount > 1) {
      this.manifestRecoveryAmbiguous = true;
      this.resetManifestFields();
      this.writeStderr('warning: multiple prior workflow manifests matched this repo identity; routing conservatively\n');
      return 'missing';
    }

    if (matchedCount === 1 && this.readManifestFile(matchedCandidate) === 'ok') {
      this.manifestRecovered = true;
      this.writeStderr(`warning: recovered workflow manifest from previous repo slug ${matchedCandidate}\n`);
      return 'ok';
    }

    this.resetManifestFields();
    return 'missing';
  }

  private loadManifestReadOnly(): ManifestReadResult {
    let currentManifestCorrupt = false;
    this.resetManifestDiagnostics();
    this.manifestCorrupt = false;

    const currentManifest = this.readManifest();
    if (currentManifest === 'ok') {
      if (this.manifestRepoRoot.length > 0 && this.manifestRepoRoot !== this.repoRoot) {
        this.manifestIdentityMismatch = true;
        this.manifestIdentityReason = 'repo_root_mismatch';
      } else if (this.manifestBranch.length > 0 && this.manifestBranch !== this.branch) {
        this.manifestIdentityMismatch = true;
        this.manifestIdentityReason = 'branch_mismatch';
      }

      return 'ok';
    }

    if (currentManifest === 'corrupt' || this.manifestCorrupt) {
      currentManifestCorrupt = true;
      this.manifestSpecPath = '';
      this.manifestPlanPath = '';
    }

    const candidateFileName = `${this.userName}-${this.safeBranch}-workflow-state.json`;
    const projectsRoot = path.join(this.stateDir, 'projects');
    let matchedCandidate = '';
    let matchedCount = 0;
    let inspected = 0;

    if (pathExists(projectsRoot)) {
      for (const entry of fs.readdirSync(projectsRoot).sort((left, right) => left.localeCompare(right))) {
        const candidatePath = path.join(projectsRoot, entry, candidateFileName);
        if (candidatePath === this.manifestPath || !isFilePath(candidatePath)) {
          continue;
        }

        inspected += 1;
        if (inspected > this.recoveryCandidateLimit) {
          break;
        }

        if (this.readManifestFile(candidatePath) === 'ok') {
          if (this.manifestRepoRoot === this.repoRoot && this.manifestBranch === this.branch) {
            matchedCandidate = candidatePath;
            matchedCount += 1;
            if (matchedCount > 1) {
              break;
            }
          }
        }
      }
    }

    if (matchedCount > 1) {
      this.manifestRecoveryAmbiguous = true;
      this.resetManifestFields();
      this.manifestCorrupt = currentManifestCorrupt;
      return 'missing';
    }

    if (matchedCount === 1 && this.readManifestFile(matchedCandidate) === 'ok') {
      this.manifestRecovered = true;
      this.manifestCorrupt = currentManifestCorrupt;
      return 'ok';
    }

    this.resetManifestFields();
    this.manifestCorrupt = currentManifestCorrupt;
    return 'missing';
  }

  private manifestPathsForWrite(): [string, string] {
    const writeSpec =
      this.manifestSpecPath.length === 0 && !this.specAmbiguous ? this.resolvedSpecPath : this.manifestSpecPath;
    const writePlan =
      this.manifestPlanPath.length === 0 && !this.planAmbiguous ? this.resolvedPlanPath : this.manifestPlanPath;

    return [writeSpec, writePlan];
  }

  private applyResolutionDiagnostics(mode: ResolveMode, corrupted: boolean): void {
    if (mode === 'mutating' && corrupted) {
      this.statusCode = 'needs_brainstorming';
      this.nextSkill = 'superpowers:brainstorming';
      this.setStatusNotes('corrupt_manifest_recovered');
    } else if (mode === 'read_only' && this.manifestCorrupt) {
      this.appendStatusNote('corrupt_manifest_present');
    }

    if (this.manifestIdentityMismatch) {
      this.appendStatusNote(this.manifestIdentityReason);
    }
    if (this.manifestRecovered) {
      this.appendStatusNote('repo_slug_recovered');
    }
    if (this.manifestRecoveryAmbiguous) {
      this.appendStatusNote('repo_slug_recovery_ambiguous');
    }
  }

  private writeAndEmitResult(specPath: string, planPath: string, outputMode: 'json' | 'summary'): CommandResult {
    const payload = this.manifestPayload(specPath, planPath, this.statusCode, this.nextSkill, this.currentReason());
    if (!this.writeManifestWithRetry(payload)) {
      this.statusCode = 'needs_brainstorming';
      this.nextSkill = 'superpowers:brainstorming';
      this.setStatusNotes('manifest_write_conflict');
    }

    if (outputMode === 'summary') {
      this.emitSummary(this.statusCode, this.nextSkill, specPath, planPath, this.currentReason());
    } else {
      this.emitStatusJson(this.statusCode, this.nextSkill, specPath, planPath, this.currentReason());
    }

    return this.result(0);
  }

  private validateRepoRelativePath(input: string): string | null {
    const normalized = normalizeRepoRelativePath(input);
    if (normalized === null) {
      this.writeStderr(`Invalid artifact path: ${input}\n`);
      return null;
    }

    const absolutePath = path.join(this.repoRoot, normalized);
    if (!isPathInsideRoot(this.repoRoot, absolutePath)) {
      this.writeStderr(`Invalid artifact path: ${input}\n`);
      return null;
    }

    return normalized;
  }

  private commandStatus(args: string[]): CommandResult {
    let refresh = false;
    let summary = false;

    for (const arg of args) {
      if (arg === '--refresh') {
        refresh = true;
      } else if (arg === '--summary') {
        summary = true;
      } else {
        this.writeStderr(`Unknown status option: ${arg}\n`);
        this.writeStdout(this.usageText());
        return this.result(1);
      }
    }

    this.ensureManifestDir();
    this.loadManifestOrRecover();
    const corrupted = this.manifestCorrupt;

    if (
      !refresh &&
      this.manifestValid &&
      !corrupted &&
      !this.manifestRecovered &&
      !this.manifestIdentityMismatch &&
      !this.manifestRecoveryAmbiguous
    ) {
      if (summary) {
        this.emitSummary(
          (this.manifestStatus as WorkflowStatusCode) || 'needs_brainstorming',
          this.manifestNextSkill,
          this.manifestSpecPath,
          this.manifestPlanPath,
          this.manifestReason,
        );
      } else {
        this.emitStatusJson(
          (this.manifestStatus as WorkflowStatusCode) || 'needs_brainstorming',
          this.manifestNextSkill,
          this.manifestSpecPath,
          this.manifestPlanPath,
          this.manifestReason,
        );
      }

      return this.result(0);
    }

    this.deriveStatus();
    this.applyResolutionDiagnostics('mutating', corrupted);
    const [persistedSpec, persistedPlan] = this.manifestPathsForWrite();
    return this.writeAndEmitResult(persistedSpec, persistedPlan, summary ? 'summary' : 'json');
  }

  private commandResolve(args: string[]): CommandResult {
    for (const arg of args) {
      if (arg !== '--debug') {
        this.emitResolveFailure('InvalidCommandInput', `Unknown resolve option: ${arg}`);
        return this.result(1);
      }
    }

    switch (this.env.SUPERPOWERS_WORKFLOW_RESOLVE_TEST_FAILPOINT) {
      case 'runtime_failure':
        this.emitResolveFailure('ResolverRuntimeFailure', 'Injected read-only resolver failpoint triggered.');
        return this.result(1);
      case 'invalid_contract':
        this.emitResolveFailure('ResolverContractViolation', 'Injected invalid resolve-contract failpoint triggered.');
        return this.result(1);
      default:
        break;
    }

    if (this.env.SUPERPOWERS_WORKFLOW_STATUS_TEST_FAILPOINT === 'resolve_runtime_failure') {
      this.emitResolveFailure('ResolverRuntimeFailure', 'Injected read-only resolver failpoint triggered.');
      return this.result(1);
    }

    if (!this.repoContextAvailable) {
      this.emitResolveFailure('RepoContextUnavailable', 'Read-only workflow resolution requires a git repo.');
      return this.result(1);
    }

    this.loadManifestReadOnly();
    this.deriveStatus();
    this.applyResolutionDiagnostics('read_only', false);
    this.emitResolveJson(this.statusCode, this.nextSkill, this.resolvedSpecPath, this.resolvedPlanPath, this.currentReason());
    return this.result(0);
  }

  private commandExpect(args: string[]): CommandResult {
    let artifact = '';
    let artifactPath = '';

    for (let index = 0; index < args.length; index += 1) {
      const arg = args[index];
      if (arg === '--artifact') {
        artifact = args[index + 1] ?? '';
        index += 1;
      } else if (arg === '--path') {
        artifactPath = args[index + 1] ?? '';
        index += 1;
      } else {
        this.writeStderr(`Unknown expect option: ${arg}\n`);
        this.writeStdout(this.usageText());
        return this.result(1);
      }
    }

    if (artifact !== 'spec' && artifact !== 'plan') {
      this.writeStderr('expect requires --artifact {spec|plan}\n');
      return this.result(1);
    }
    if (artifactPath.length === 0) {
      this.writeStderr('expect requires --path <repo-relative-path>\n');
      return this.result(1);
    }

    this.ensureManifestDir();
    this.loadManifestOrRecover();

    const normalized = this.validateRepoRelativePath(artifactPath);
    if (normalized === null) {
      return this.result(1);
    }

    if (artifact === 'spec') {
      this.manifestSpecPath = normalized;
    } else {
      this.manifestPlanPath = normalized;
    }

    this.deriveStatus();
    this.appendStatusNote('expect_set');
    const [persistedSpec, persistedPlan] = this.manifestPathsForWrite();
    return this.writeAndEmitResult(persistedSpec, persistedPlan, 'json');
  }

  private commandSync(args: string[]): CommandResult {
    let artifact = '';
    let artifactPath = '';

    for (let index = 0; index < args.length; index += 1) {
      const arg = args[index];
      if (arg === '--artifact') {
        artifact = args[index + 1] ?? '';
        index += 1;
      } else if (arg === '--path') {
        artifactPath = args[index + 1] ?? '';
        index += 1;
      } else {
        this.writeStderr(`Unknown sync option: ${arg}\n`);
        this.writeStdout(this.usageText());
        return this.result(1);
      }
    }

    if (artifact !== 'spec' && artifact !== 'plan') {
      this.writeStderr('sync requires --artifact {spec|plan}\n');
      return this.result(1);
    }

    this.ensureManifestDir();
    this.loadManifestOrRecover();

    let targetPath = '';
    if (artifactPath.length > 0) {
      const normalized = this.validateRepoRelativePath(artifactPath);
      if (normalized === null) {
        return this.result(1);
      }
      targetPath = normalized;
    } else {
      targetPath = artifact === 'spec' ? this.manifestSpecPath : this.manifestPlanPath;
    }

    const missingArtifact = targetPath.length === 0 || !this.repoPathExists(targetPath);
    if (artifact === 'spec') {
      this.manifestSpecPath = targetPath;
    } else {
      this.manifestPlanPath = targetPath;
    }

    this.deriveStatus();
    this.appendStatusNote(`sync_${artifact}`);

    if (missingArtifact) {
      this.appendStatusNote('missing_artifact');
      if (artifact === 'spec') {
        this.statusCode = 'needs_brainstorming';
        this.nextSkill = 'superpowers:brainstorming';
      }
    }

    const [persistedSpec, persistedPlan] = this.manifestPathsForWrite();
    return this.writeAndEmitResult(persistedSpec, persistedPlan, 'json');
  }
}

export function runWorkflowStatusCommand(args: string[], options: CommandEnvironment = {}): CommandResult {
  return new WorkflowStatusRunner(options).run(args);
}
