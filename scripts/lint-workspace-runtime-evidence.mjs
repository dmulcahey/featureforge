#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MODULE_DIR = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_REPO_ROOT = path.resolve(MODULE_DIR, '..');
const DEFAULT_SCAN_DIRS = [
  'docs/featureforge/execution-evidence',
  'docs/featureforge/reviews',
  '.featureforge/reviews',
  'docs/featureforge/handoffs',
  '.featureforge/handoffs',
  'docs/featureforge/projections',
  '.featureforge/projections',
];

const LIVE_STATE_PATTERNS = [
  /~\/\.featureforge(?:[\\/]|$|[^A-Za-z0-9_])/i,
  /\$home\/\.featureforge(?:[\\/]|$|[^A-Za-z0-9_])/i,
  /\$\{home\}\/\.featureforge(?:[\\/]|$|[^A-Za-z0-9_])/i,
  /\$userprofile[\\/]\.featureforge(?:[\\/]|$|[^A-Za-z0-9_])/i,
  /\$\{userprofile\}[\\/]\.featureforge(?:[\\/]|$|[^A-Za-z0-9_])/i,
  /\/Users\/[^/\s]+\/\.featureforge(?:[\\/]|$|[^A-Za-z0-9_])/i,
  /\/home\/[^/\s]+\/\.featureforge(?:[\\/]|$|[^A-Za-z0-9_])/i,
  /[A-Za-z]:\\Users\\[^\\\s]+\\\.featureforge(?:[\\]|$|[^A-Za-z0-9_])/i,
];

const LIVE_WORKFLOW_COMMAND_SUFFIXES = [
  'workflow operator',
  'workflow doctor',
  'workflow status',
  'plan contract build-task-packet',
  'plan execution status',
  'plan execution begin',
  'plan execution complete',
  'plan execution reopen',
  'plan execution transfer',
  'plan execution close-current-task',
  'plan execution repair-review-state',
  'plan execution advance-late-stage',
  'plan execution materialize-projections',
  'repo-safety approve',
];

const LIVE_WORKFLOW_COMMAND_REQUIRED_ARGUMENTS = new Map([
  ['plan contract build-task-packet', String.raw`\s+--persist(?:\s+|=)yes\b`],
]);

const WORKSPACE_ROOT_VARIABLE_NAME = String.raw`(?:[A-Za-z_][A-Za-z0-9_]*(?:REPO|WORKSPACE|WORKTREE|PROJECT|CHECKOUT|PWD|CWD|ROOT)[A-Za-z0-9_]*|(?:REPO|WORKSPACE|WORKTREE|PROJECT|CHECKOUT|PWD|CWD|ROOT)[A-Za-z0-9_]*)`;
const INSTALLED_ROOT_VARIABLE_NAME = String.raw`(?:[A-Za-z_][A-Za-z0-9_]*(?:INSTALL|INSTALLED)[A-Za-z0-9_]*|(?:INSTALL|INSTALLED)[A-Za-z0-9_]*)`;
const WORKSPACE_ROOT_VARIABLE_REFERENCE = String.raw`(?:\$(?!${INSTALLED_ROOT_VARIABLE_NAME}[\\/])${WORKSPACE_ROOT_VARIABLE_NAME}|\$\{(?!${INSTALLED_ROOT_VARIABLE_NAME}\})${WORKSPACE_ROOT_VARIABLE_NAME}\})`;
const SHELL_TOKEN_CHARS_ZERO = '[^\\s"\'`]*';
const SHELL_TOKEN_CHARS = '[^\\s"\'`]+';
const PATH_SEGMENT = "[^\\\\/\\s\"'`$()=;&|]+";
const RELATIVE_ROOT_SEGMENT = String.raw`(?![~$])${PATH_SEGMENT}`;
const RELATIVE_WORKTREE_PREFIX = String.raw`(?:(?:\.{1,2}|${RELATIVE_ROOT_SEGMENT})[\\/](?:${PATH_SEGMENT}[\\/])*)?`;
const ABSOLUTE_WORKTREE_PREFIX = String.raw`(?:[A-Za-z]:[\\/]|/)${SHELL_TOKEN_CHARS_ZERO}[\\/]`;
const TILDE_WORKTREE_PREFIX = String.raw`~[\\/](?!\.featureforge[\\/]install[\\/])(?:${PATH_SEGMENT}[\\/])*`;
const ROOT_VARIABLE_WORKTREE_PREFIX = String.raw`${WORKSPACE_ROOT_VARIABLE_REFERENCE}[\\/]`;
const WORKSPACE_LAUNCH_PREFIX = String.raw`(?:${RELATIVE_WORKTREE_PREFIX}|${ABSOLUTE_WORKTREE_PREFIX}|${TILDE_WORKTREE_PREFIX}|${ROOT_VARIABLE_WORKTREE_PREFIX})`;
const QUOTED_STATE_VALUE = String.raw`(?:"[^"\n]*"|'[^'\n]*')`;
const MKTEMP_STATE_VALUE = String.raw`\$\(\s*mktemp\b[^$()` + "`" + String.raw`;&|]*\)`;
const BARE_STATE_VALUE = "[^\\s\"'`$()=;&|]+";
const STATE_ASSIGNMENT_VALUE = String.raw`(?:${QUOTED_STATE_VALUE}|${MKTEMP_STATE_VALUE}|${BARE_STATE_VALUE})`;
const INLINE_STATE_DIR_ASSIGNMENT = new RegExp(
  String.raw`^\s*FEATUREFORGE_STATE_DIR=(?<value>${STATE_ASSIGNMENT_VALUE})\s*$`,
  'iu',
);
const EXPORTED_STATE_DIR_ASSIGNMENT = new RegExp(
  String.raw`^\s*export\s+FEATUREFORGE_STATE_DIR=(?<value>${STATE_ASSIGNMENT_VALUE})\s*$`,
  'iu',
);

const WORKSPACE_BINARY_SOURCES = [
  {
    display: './bin/featureforge',
    matcher:
      String.raw`(?:(?:${RELATIVE_WORKTREE_PREFIX}|${TILDE_WORKTREE_PREFIX}|${ROOT_VARIABLE_WORKTREE_PREFIX})bin[\\/]featureforge(?:\.exe)?|(?:[A-Za-z]:[\\/]|/)(?!${SHELL_TOKEN_CHARS_ZERO}[\\/]\.featureforge[\\/]install[\\/]bin[\\/]featureforge(?:\.exe)?(?:\b|$))${SHELL_TOKEN_CHARS_ZERO}[\\/]bin[\\/]featureforge(?:\.exe)?)`,
  },
  {
    display: './target/debug/featureforge',
    matcher: String.raw`(?:${WORKSPACE_LAUNCH_PREFIX}target[\\/]debug[\\/]featureforge(?:\.exe)?)`,
  },
  {
    display: './target/release/featureforge',
    matcher: String.raw`(?:${WORKSPACE_LAUNCH_PREFIX}target[\\/]release[\\/]featureforge(?:\.exe)?)`,
  },
  {
    display: './target/<triple>/debug/featureforge',
    matcher: String.raw`(?:${WORKSPACE_LAUNCH_PREFIX}target[\\/]${PATH_SEGMENT}[\\/]debug[\\/]featureforge(?:\.exe)?)`,
  },
  {
    display: './target/<triple>/release/featureforge',
    matcher: String.raw`(?:${WORKSPACE_LAUNCH_PREFIX}target[\\/]${PATH_SEGMENT}[\\/]release[\\/]featureforge(?:\.exe)?)`,
  },
];

const COMMAND_BOUNDARY = "(?:^|[\\s\"'`();=|&])";
const CARGO_RUN_WITH_OR_WITHOUT_SEPARATOR = String.raw`cargo(?:\s+(?!--(?:\s|$))\S+)*\s+(?:run|r)(?:\s+(?!--(?:\s|$))\S+)*(?:\s+--)?\s+`;

function suffixRegex(suffix) {
  return suffix.trim().split(/\s+/u).join(String.raw`\s+`);
}

function escapeRegex(value) {
  return value.replace(/[\\^$.*+?()[\]{}|]/g, '\\$&');
}

function absolutePathMatcher(absolutePath) {
  return path.resolve(absolutePath).split(path.sep).map(escapeRegex).join(String.raw`[\\/]`);
}

function workspaceBinarySources(repoRoot) {
  return [
    ...WORKSPACE_BINARY_SOURCES,
    {
      display: './bin/featureforge',
      matcher: `${absolutePathMatcher(repoRoot)}${String.raw`[\\/]bin[\\/]featureforge(?:\.exe)?`}`,
    },
  ];
}

function buildForbiddenPatterns(repoRoot) {
  const patterns = [];
  for (const suffix of LIVE_WORKFLOW_COMMAND_SUFFIXES) {
    const suffixPattern = suffixRegex(suffix);
    const requiredArgumentPattern = LIVE_WORKFLOW_COMMAND_REQUIRED_ARGUMENTS.get(suffix);
    const commandTailPattern =
      requiredArgumentPattern === undefined
        ? String.raw`\b`
        : String.raw`\b(?=[\s\S]*` + requiredArgumentPattern + ')';
    for (const source of workspaceBinarySources(repoRoot)) {
      patterns.push({
        command: `${source.display} ${suffix}`,
        regex: new RegExp(
          `${COMMAND_BOUNDARY}${source.matcher}${String.raw`\s+`}${suffixPattern}${commandTailPattern}`,
          'i',
        ),
      });
    }
    patterns.push({
      command: `cargo run -- ${suffix}`,
      regex: new RegExp(
        `${COMMAND_BOUNDARY}${CARGO_RUN_WITH_OR_WITHOUT_SEPARATOR}${suffixPattern}${commandTailPattern}`,
        'i',
      ),
    });
  }
  return patterns;
}

function usage() {
  return [
    'usage:',
    '  node scripts/lint-workspace-runtime-evidence.mjs [--repo-root <path>] [--path <path> ...]',
    '',
    'notes:',
    '  - without --path, scans evidence/review/handoff/projection artifact roots',
    '  - with --path, scans only the provided files/directories',
  ].join('\n');
}

function parseArgs(argv) {
  const options = {
    repoRoot: DEFAULT_REPO_ROOT,
    scanPaths: [],
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--help' || arg === '-h') {
      options.help = true;
      continue;
    }
    if (arg === '--repo-root') {
      const value = argv[index + 1];
      if (!value || value.startsWith('--')) {
        throw new Error('missing value for --repo-root');
      }
      options.repoRoot = path.resolve(value);
      index += 1;
      continue;
    }
    if (arg === '--path') {
      const value = argv[index + 1];
      if (!value || value.startsWith('--')) {
        throw new Error('missing value for --path');
      }
      options.scanPaths.push(value);
      index += 1;
      continue;
    }
    throw new Error(`unknown argument: ${arg}`);
  }
  return options;
}

function asRelative(root, absolutePath) {
  return path.relative(root, absolutePath).split(path.sep).join('/');
}

function collectFiles(inputPath, files) {
  let stat;
  try {
    stat = fs.statSync(inputPath);
  } catch {
    return;
  }
  if (stat.isFile()) {
    files.add(path.resolve(inputPath));
    return;
  }
  if (!stat.isDirectory()) {
    return;
  }
  for (const entry of fs.readdirSync(inputPath, { withFileTypes: true })) {
    collectFiles(path.join(inputPath, entry.name), files);
  }
}

function collectScanFiles(repoRoot, rawPaths) {
  const files = new Set();
  const scanRoots =
    rawPaths.length > 0
      ? rawPaths.map((scanPath) =>
          path.isAbsolute(scanPath) ? scanPath : path.join(repoRoot, scanPath),
        )
      : DEFAULT_SCAN_DIRS.map((scanPath) => path.join(repoRoot, scanPath));
  for (const scanRoot of scanRoots) {
    collectFiles(scanRoot, files);
  }
  return [...files].sort();
}

function commandMatchSpan(rule, value) {
  const match = rule.regex.exec(value);
  if (match === null) {
    return null;
  }
  const firstChar = match[0].charAt(0);
  const start = /^[\s"'`();=|&]$/u.test(firstChar) ? match.index + 1 : match.index;
  return {
    start,
    end: match.index + match[0].length,
  };
}

function hasShellBoundary(value) {
  return /(?:;|&&|\|\||\||&|\$\(|`)/u.test(value);
}

function stripMatchingQuotes(value) {
  const trimmed = value.trim();
  if (trimmed.length >= 2) {
    const first = trimmed.charAt(0);
    const last = trimmed.charAt(trimmed.length - 1);
    if ((first === '"' || first === "'") && first === last) {
      return trimmed.slice(1, -1).trim();
    }
  }
  return trimmed;
}

function isSafeStateDirValue(rawValue) {
  const value = stripMatchingQuotes(rawValue);
  if (value.length === 0 || /[`;&|]/u.test(value)) {
    return false;
  }
  if (value.startsWith('$(') || value.includes('$(') || value.includes(')')) {
    if (!value.startsWith('$(') || !value.endsWith(')')) {
      return false;
    }
    const inner = value.slice(2, -1).trim();
    return /^mktemp\b/u.test(inner) && !/[$()`;&|]/u.test(inner);
  }
  return (
    /^(?:\/tmp(?:[\\/]|$)|\/private\/tmp(?:[\\/]|$)|\/var\/folders(?:[\\/]|$))/iu.test(value) ||
    /\b(?:fixture|fixtures|fixture-state|temp-state)\b/iu.test(value)
  );
}

function standaloneExportedSafeStateValue(value) {
  const match = EXPORTED_STATE_DIR_ASSIGNMENT.exec(value);
  if (match?.groups?.value === undefined) {
    return false;
  }
  return isSafeStateDirValue(match.groups.value);
}

function hasUnsafeShellBoundaryAfterCommand(value, commandEnd) {
  return hasShellBoundary(value.slice(commandEnd));
}

function hasInlineSimpleCommandStateAssignment(rule, value) {
  const commandSpan = commandMatchSpan(rule, value);
  if (commandSpan === null || hasUnsafeShellBoundaryAfterCommand(value, commandSpan.end)) {
    return false;
  }
  const prefix = value.slice(0, commandSpan.start);
  const assignment = INLINE_STATE_DIR_ASSIGNMENT.exec(prefix);
  return assignment?.groups?.value !== undefined && isSafeStateDirValue(assignment.groups.value);
}

function hasSafeExportedStateBeforeCommand(source, rule) {
  let sawExportedState = false;
  for (const line of source.split(/\r?\n/u)) {
    const normalizedLine = normalizeMatchCandidate(line);
    const commandSpan = commandMatchSpan(rule, normalizedLine);
    if (commandSpan !== null) {
      const commandPrefix = normalizedLine.slice(0, commandSpan.start);
      return (
        sawExportedState &&
        commandPrefix.trim().length === 0 &&
        !hasUnsafeShellBoundaryAfterCommand(normalizedLine, commandSpan.end)
      );
    }
    if (standaloneExportedSafeStateValue(normalizedLine)) {
      sawExportedState = true;
    }
  }
  return false;
}

function hasSafeContext(candidate, precedingContext, rule) {
  const hasInlineStateOnCommandLine = candidate.source
    .split(/\r?\n/u)
    .some((line) => {
      const normalizedLine = normalizeMatchCandidate(line);
      return hasInlineSimpleCommandStateAssignment(rule, normalizedLine);
    });
  const hasInlineStateOnContinuation =
    candidate.shellContinuation &&
    hasInlineSimpleCommandStateAssignment(rule, candidate.candidate);
  const exportedStateCandidate = [precedingContext, candidate.source].filter(Boolean).join('\n');
  const hasExportedState = hasSafeExportedStateBeforeCommand(exportedStateCandidate, rule);
  return hasInlineStateOnCommandLine || hasInlineStateOnContinuation || hasExportedState;
}

function hasLiveStateMarker(context) {
  return LIVE_STATE_PATTERNS.some((pattern) => pattern.test(context));
}

function normalizeMatchCandidate(value) {
  return value.replace(/\\\s+/g, ' ').replace(/\s+/g, ' ').trim();
}

function buildScanCandidates(lines) {
  const candidates = [];

  for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
    for (let span = 1; span <= 3; span += 1) {
      const end = Math.min(lines.length - 1, lineIndex + span - 1);
      const source = lines.slice(lineIndex, end + 1).join('\n');
      const merged = normalizeMatchCandidate(source);
      if (merged.length === 0) {
        continue;
      }
      candidates.push({
        line: lineIndex + 1,
        start: lineIndex,
        end,
        candidate: merged,
        source,
        shellContinuation: false,
      });
    }
  }

  let continuationStart = 0;
  let continuationHasBackslash = false;
  let continuationBuffer = '';
  for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
    const line = lines[lineIndex];
    if (continuationBuffer.length === 0) {
      continuationStart = lineIndex;
    }
    continuationBuffer += `${continuationBuffer.length > 0 ? ' ' : ''}${line.trimEnd()}`;
    if (/\\\s*$/.test(line)) {
      continuationHasBackslash = true;
      continuationBuffer = continuationBuffer.replace(/\\\s*$/, '');
      continue;
    }
    if (continuationHasBackslash) {
      const merged = normalizeMatchCandidate(continuationBuffer);
      if (merged.length > 0) {
        candidates.push({
          line: continuationStart + 1,
          start: continuationStart,
          end: lineIndex,
          candidate: merged,
          source: lines.slice(continuationStart, lineIndex + 1).join('\n'),
          shellContinuation: true,
        });
      }
    }
    continuationHasBackslash = false;
    continuationBuffer = '';
  }

  return candidates;
}

function scanFile(filePath, forbiddenPatterns) {
  let source;
  try {
    source = fs.readFileSync(filePath, 'utf8');
  } catch (error) {
    return [
      {
        line: 1,
        command: '<read-error>',
        reason: `unable to read file as utf8 (${error.code ?? error.message})`,
      },
    ];
  }

  const lines = source.split(/\r?\n/u);
  const scanCandidates = buildScanCandidates(lines);
  const violations = [];
  const seenViolations = new Set();
  for (const candidate of scanCandidates) {
    for (const rule of forbiddenPatterns) {
      if (!rule.regex.test(candidate.candidate)) {
        continue;
      }
      const key = `${candidate.line}:${rule.command}`;
      if (seenViolations.has(key)) {
        continue;
      }
      seenViolations.add(key);

      const start = Math.max(0, candidate.start - 2);
      const end = Math.min(lines.length - 1, candidate.end + 2);
      const context = lines.slice(start, end + 1).join('\n');
      const precedingContext = lines.slice(start, candidate.start).join('\n');
      const safeContext = hasSafeContext(candidate, precedingContext, rule);
      const liveMarker = hasLiveStateMarker(context);
      if (safeContext && !liveMarker) {
        continue;
      }
      const reason = liveMarker
        ? 'temp/fixture context is mixed with live ~/.featureforge state markers'
        : 'missing nearby fixture/temp-state isolation context';
      violations.push({
        line: candidate.line,
        command: rule.command,
        reason,
      });
    }
  }
  return violations;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    console.log(usage());
    return;
  }

  const files = collectScanFiles(options.repoRoot, options.scanPaths);
  const forbiddenPatterns = buildForbiddenPatterns(options.repoRoot);
  const failures = [];
  for (const absolutePath of files) {
    const violations = scanFile(absolutePath, forbiddenPatterns);
    for (const violation of violations) {
      failures.push({
        file: asRelative(options.repoRoot, absolutePath),
        line: violation.line,
        command: violation.command,
        reason: violation.reason,
      });
    }
  }

  if (failures.length > 0) {
    console.error('workspace-runtime evidence lint failed:');
    for (const failure of failures) {
      console.error(
        `- ${failure.file}:${failure.line}: ${failure.command} (${failure.reason})`,
      );
    }
    process.exit(1);
  }

  if (files.length === 0) {
    console.log('workspace-runtime evidence lint passed (no candidate files found)');
    return;
  }

  console.log(
    `workspace-runtime evidence lint passed (${files.length} file(s) scanned)`,
  );
}

main();
