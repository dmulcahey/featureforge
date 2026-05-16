import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import {
  REPO_ROOT,
  SKILLS_DIR,
  listGeneratedSkills,
  readUtf8,
  parseFrontmatter,
  extractBashBlockUnderHeading,
  extractSection,
  normalizeWhitespace,
  countOccurrences,
} from './helpers/markdown-test-helpers.mjs';
import {
  renderTemplateContent,
  ROUTE_LAW_MODE,
  ROUTE_OWNING_GENERATED_SKILLS,
  routeLawModeForTemplate,
} from '../../scripts/gen-skill-docs.mjs';
import {
  REQUIRED_PROMPT_COMPANION_SOURCE_ARCHIVE_PATHS,
  REQUIRED_SKILL_COMPANION_ASSET_SOURCE_ARCHIVE_PATHS,
  REQUIRED_SOURCE_ARCHIVE_PATHS,
  isDirectScriptRun,
} from '../../scripts/verify-source-archive.mjs';

function getTemplatePath(skill) {
  return path.join(SKILLS_DIR, skill, 'SKILL.md.tmpl');
}

function getSkillPath(skill) {
  return path.join(SKILLS_DIR, skill, 'SKILL.md');
}

function getSkillDescription(skill) {
  const { frontmatter } = parseFrontmatter(readUtf8(getSkillPath(skill)));
  assert.ok(frontmatter, `${skill} should have frontmatter`);
  return frontmatter.description;
}

function readUtf8WithGeneratedRouteAuthority(filePath) {
  return renderTemplateContent(readUtf8(filePath), filePath);
}

function workflowOperatorStateKindValues() {
  const schema = JSON.parse(
    readUtf8(path.join(REPO_ROOT, 'schemas/workflow-operator.schema.json')),
  );
  const values = schema?.$defs?.WorkflowOperatorStateKindSchema?.enum;
  assert.ok(Array.isArray(values), 'workflow operator schema should expose state_kind enum values');
  return values;
}

function reviewStateReferenceStateKindValues() {
  const reference = readUtf8(
    path.join(REPO_ROOT, 'docs/featureforge/reference/2026-04-01-review-state-reference.md'),
  );
  const match = reference.match(/`state_kind = ([^`]+)` classifies routability/);
  assert.ok(match, 'review-state reference should document the public state_kind taxonomy');
  return match[1].split('|');
}

const GENERATED_ROUTE_FIELD_PATTERN =
  /recommended_public_command_argv|recommended_public_command_template|required_inputs|recommended_command|display-only compatibility text|typed executable surface/;
const RETIRED_RUNTIME_COMMAND_TRAP_PATTERN =
  /\bfeatureforge plan execution (?:preflight|record-review-dispatch|gate-review|gate-finish|rebuild-evidence|record-branch-closure|record-release-readiness|record-final-review|record-qa)\b|\bfeatureforge workflow status\b|(?:"\s*)?(?:\$_FEATUREFORGE_BIN|\$\{_FEATUREFORGE_BIN\})(?:\s*")?\s+workflow\s+status\b|`workflow status\b/;
const ROUTE_SPECIFIC_COMMAND_MAPPING_PATTERN =
  /advance-late-stage --result ready\|blocked|input shape renders `\*\*Result:\*\* pass\|blocked`|Artifact `(?:pass|blocked)` is the runtime-rendered form of CLI input `--result (?:ready|blocked)`/;

function assertContainsOperatorPublicCommandAuthority(content, label) {
  const section = extractSection(content, 'Installed Control Plane');
  assert.ok(section, `${label} should include the generated Installed Control Plane section`);
  assertContainsFragments(section, `${label} generated Installed Control Plane`, [
    '`$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`',
    '`recommended_public_command_argv`',
    '`recommended_public_command_template`',
    '`required_inputs`',
    '`$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --input NAME=VALUE --json`',
    '`recommended_command`',
    '`$_FEATUREFORGE_ROOT/references/operator-route-authority.md`',
  ]);
  assert.doesNotMatch(
    section,
    /rerun that operator query with `--input NAME=VALUE`/,
    `${label} generated Installed Control Plane must include the full plan-bound operator command when binding template inputs`,
  );
  assert.doesNotMatch(
    section,
    /recommended_public_command_template\.input_bindings/,
    `${label} should keep detailed template-binding law in operator-route-authority.md`,
  );
  assert.doesNotMatch(
    content,
    /only rebinds argv\[0\]|recommended_public_command_argv\[0\] == "featureforge"|replacing argv\[0\]|otherwise bind `recommended_public_command_template`/,
    `${label} should keep wrapper and detailed argv/template binding law in operator-route-authority.md`,
  );
}

function assertRouteAuthoritySectionIsCompact(content, label) {
  const section = extractSection(content, 'Reviewed-Closure Route Authority');
  assert.ok(section, `${label} should include compact reviewed-closure route guidance`);
  assertContainsFragments(section, `${label} compact reviewed-closure route guidance`, [
    '`$_FEATUREFORGE_ROOT/references/operator-route-authority.md`',
    '`$_FEATUREFORGE_ROOT/docs/featureforge/reference/2026-04-01-review-state-reference.md`',
    '--external-review-result-ready --json',
  ]);
  assert.doesNotMatch(
    section,
    /recommended_public_command_argv|recommended_public_command_template|required_inputs|recommended_command|Installed Control Plane law above|Detailed argv binding|operator-mediated template materialization/,
    `${label} compact route section must not repeat Installed Control Plane field law`,
  );
}

function assertNoNormalRuntimeHelperVocabulary(content, label) {
  const forbiddenPatterns = [
    /helper-derived workflow routes/i,
    /Helper-first routing/i,
    /helper routing/i,
    /helper calls fail/i,
    /When the helper succeeds/i,
    /Helper-Owned Execution State/i,
    /helper-selected topology/i,
    /compatibility-helper choreography/i,
    /helper-backed/i,
    /helper-owned finish gate/i,
    /helper-owned (?:reopen|remediation|mutation)/i,
    /helper-reported (?:blocking reason|execution state)/i,
    /helper-built packet/i,
    /Implementer helpers\/subagents/i,
    /If the helper returns `(?:allowed|blocked)`/i,
  ];
  for (const pattern of forbiddenPatterns) {
    assert.doesNotMatch(
      content,
      pattern,
      `${label} should use runtime/operator wording instead of hidden-helper routing vocabulary (${pattern})`,
    );
  }
}

function assertNoRepoSafetyHelperReturnVocabulary(content, label) {
  assert.doesNotMatch(
    content,
    /If the helper returns `(?:allowed|blocked)`/i,
    `${label} should describe repo-safety results as repo-safety check output, not hidden-helper return values`,
  );
}

function assertContainsFragments(content, label, fragments) {
  for (const fragment of fragments) {
    assert.ok(
      content.includes(fragment),
      `${label} should include core fragment \`${fragment}\``,
    );
  }
}

function assertLaterPhaseUsesInstalledRouteLaw(content, label) {
  assertContainsFragments(content, label, [
    'workflow/operator JSON returns a later phase',
    'follow that reported route',
    'Installed Control Plane section and canonical route reference',
  ]);
}

function assertRuntimeFirstRoutingPrinciples(content, label) {
  assertContainsFragments(content, label, [
    '`$_FEATUREFORGE_BIN workflow doctor --plan <approved-plan-path> --json`',
    '`$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`',
    'operator-routed public commands',
    '`phase_detail=task_closure_recording_ready`',
    'derived output, not routing authority',
    '`phase` `executing`',
    'canonical route reference',
    'stop and report unresolved route binding',
  ]);
  assert.doesNotMatch(
    content,
    /\$_FEATUREFORGE_BIN plan execution recover|review_blocked/,
    `${label} should keep runtime-first routing on public operator surfaces without hidden helpers or stale route cues`,
  );
}

function retiredProductName() {
  const readme = readUtf8(path.join(REPO_ROOT, 'README.md'));
  const provenanceLine = readme
    .split('\n')
    .find((line) => line.startsWith('FeatureForge began from upstream '));
  assert.ok(provenanceLine, 'README.md should keep the provenance attribution line');
  const match = provenanceLine.match(/upstream ([A-Za-z]+):/);
  assert.ok(match, 'README.md provenance line should expose the retired product name');
  return match[1].toLowerCase();
}

const RETIRED_PRODUCT = retiredProductName();

function repoSafetyCliWriteTargets() {
  const cliSurface = readUtf8(path.join(REPO_ROOT, 'src/cli/repo_safety.rs'));
  return new Set(Array.from(cliSurface.matchAll(/#\[value\(name = "([^"]+)"\)\]/g), ([, target]) => target));
}

const HELPER_COMMAND_PATTERN = /\bfeatureforge-(plan-contract|plan-execution|workflow-status|workflow|repo-safety|session-entry|config|slug|update-check|migrate-install)\b/;

// Intentional invariant: skill installs package the runtime binary on purpose.
// Runtime-root resolution is only for locating companion files from that same
// install. It must NEVER be used to switch runtime command execution to
// $_FEATUREFORGE_ROOT/bin/featureforge, $INSTALL_DIR/bin/featureforge, PATH, or
// any other discovered binary unless product direction changes explicitly.
const FORBIDDEN_RUNTIME_FALLBACK_EXECUTION_PATTERNS = [
  [/\$_REPO_ROOT\/bin\/featureforge/, 'should not probe repo-local binaries from generated runtime docs'],
  [/(?:^|\n)\s*"\$_FEATUREFORGE_ROOT\/bin\/featureforge"/, 'should not execute runtime commands through a root-selected launcher'],
  [/(?:^|\n)\s*"\$INSTALL_DIR\/bin\/featureforge"/, 'should not execute runtime commands through an install-root-selected launcher'],
  [/(?:^|\n)\s*"\$_FEATUREFORGE_ROOT\/bin\/featureforge\.exe"/, 'should not execute runtime commands through a root-selected Windows launcher'],
  [/(?:^|\n)\s*"\$INSTALL_DIR\/bin\/featureforge\.exe"/, 'should not execute runtime commands through an install-root-selected Windows launcher'],
  [/(?:^|\n)\s*FEATUREFORGE_RUNTIME_BIN="\$_FEATUREFORGE_ROOT\/bin\/featureforge"/, 'should not assign the runtime command path from $_FEATUREFORGE_ROOT'],
  [/(?:^|\n)\s*FEATUREFORGE_RUNTIME_BIN="\$INSTALL_DIR\/bin\/featureforge"/, 'should not assign the runtime command path from INSTALL_DIR'],
  [/(?:^|\n)\s*FEATUREFORGE_RUNTIME_BIN="\$_FEATUREFORGE_ROOT\/bin\/featureforge\.exe"/, 'should not assign the runtime command path from a root-selected Windows launcher'],
  [/(?:^|\n)\s*FEATUREFORGE_RUNTIME_BIN="\$INSTALL_DIR\/bin\/featureforge\.exe"/, 'should not assign the runtime command path from an install-root-selected Windows launcher'],
  [/\$\{_FEATUREFORGE_BIN:-featureforge\}/, 'should not fall back to PATH-selected featureforge binaries'],
  [/command -v featureforge/, 'should not rediscover featureforge through PATH lookups'],
];

function assertNoRuntimeFallbackExecution(content, label) {
  for (const [pattern, message] of FORBIDDEN_RUNTIME_FALLBACK_EXECUTION_PATTERNS) {
    assert.doesNotMatch(content, pattern, `${label} ${message}`);
  }
}

function assertForbidsDirectHelperCommandMutation(content, command, label) {
  const quoted = `\`${command}\``;
  const lines = content.split('\n');
  const windows = [];
  for (let i = 0; i < lines.length; i += 1) {
    if (!lines[i].includes(quoted)) continue;
    const start = Math.max(0, i - 3);
    const end = Math.min(lines.length - 1, i + 3);
    windows.push(lines.slice(start, end + 1).join(' '));
  }
  assert.ok(windows.length > 0, `${label} should explicitly mention ${quoted} in helper-boundary guidance`);
  const hasBoundary = windows.some((window) => {
    const hasProhibition = /(must not|do not|never|should not|cannot|can't)/i.test(window);
    const hasDirectAction = /(invoke|call|run|execute|direct(?:ly)?)/i.test(window);
    const hasOwnerActor = /(coordinator|controller|helper|runtime|harness|gate)/i.test(window);
    const hasOwnerVerb = /(owns?|owned|authoritative|handles?|appl(?:y|ies)|executes?|invokes?|calls?|runs?|governs?)/i.test(window);
    return (hasProhibition && hasDirectAction) || (hasOwnerActor && hasOwnerVerb);
  });
  assert.ok(
    hasBoundary,
    `${label} should keep ${quoted} inside coordinator/helper-owned authoritative mutation boundaries`,
  );
}

const REVIEWER_FORBIDDEN_RUNTIME_INVOCATIONS = [
  '\$_FEATUREFORGE_BIN workflow',
  '\$_FEATUREFORGE_BIN plan execution',
  'featureforge:using-featureforge',
  'featureforge:executing-plans',
];

function assertNoPositiveReviewerRuntimeInvocation(content, label) {
  const lines = content.split('\n');
  const negativeInstruction =
    /\b(do not|don't|must not|may not|never|forbid|forbids|forbidden|prohibit|prohibits|prohibited|prohibition|disallow|disallowed|blocked review)\b/i;
  const positiveInstruction = /\b(run|invoke|use|call|execute|dispatch|load)\b/i;

  for (const forbiddenInvocation of REVIEWER_FORBIDDEN_RUNTIME_INVOCATIONS) {
    const forbiddenLower = forbiddenInvocation.toLowerCase();
    for (let i = 0; i < lines.length; i += 1) {
      if (!lines[i].toLowerCase().includes(forbiddenLower)) continue;
      const start = Math.max(0, i - 2);
      const end = Math.min(lines.length - 1, i + 2);
      const window = lines.slice(start, end + 1).join(' ');
      if (negativeInstruction.test(window)) continue;
      assert.doesNotMatch(
        window,
        positiveInstruction,
        `${label} must not positively instruct reviewer agents to use ${forbiddenInvocation}`,
      );
    }
  }
}

function normalizedReviewerRecursionBlock(content) {
  return content
    .split('\n')
    .map((line) => line.trim())
    .join('\n')
    .trim();
}

function canonicalReviewerRecursionRule() {
  return normalizedReviewerRecursionBlock(
    readUtf8(path.join(REPO_ROOT, 'references/reviewer-recursion-rule.md')),
  );
}

function assertCanonicalReviewerRecursionRuleIsStrong() {
  const canonicalRule = canonicalReviewerRecursionRule();
  assertContainsFragments(canonicalRule, 'canonical reviewer recursion rule', [
    '# Review-subagent recursion rule',
    'You are a reviewer.',
    'You may inspect the provided files, packet, summaries, and context and produce review findings.',
    'Do not launch, request, or delegate to additional subagents while performing this review.',
    'Do not delegate this review to another reviewer agent.',
    '`subagent-driven-development`',
    '`requesting-code-review`',
    '`plan-fidelity-review`',
    '`plan-eng-review`',
    '`plan-ceo-review`',
    'return a blocked review finding that names the missing context instead of spawning another agent.',
  ]);
}

function assertReviewerSurfaceCarriesPromptScopedRecursionRule(content, label) {
  assert.ok(
    normalizedReviewerRecursionBlock(content).includes(canonicalReviewerRecursionRule()),
    `${label} should include the canonical prompt-only reviewer recursion rule`,
  );
  assert.doesNotMatch(content, /FEATUREFORGE_REVIEWER_RUNTIME_COMMANDS_ALLOWED/, `${label} should not require reviewer env markers`);
  assert.doesNotMatch(content, /FEATUREFORGE_REVIEWER_CONTEXT/, `${label} should not require reviewer context env markers`);
  assert.doesNotMatch(content, /ReviewerRuntimeCommandForbidden/, `${label} should not teach runtime command rejection`);
  assert.doesNotMatch(content, /REVIEWER_RUNTIME_ENV_CONTRACT/, `${label} should not carry launcher env contracts`);
  assert.doesNotMatch(content, /runtime command guard/i, `${label} should not cite runtime guard enforcement`);
  assert.doesNotMatch(content, /reviewer-mode environment/i, `${label} should not cite reviewer-mode environment enforcement`);
  assertNoPositiveReviewerRuntimeInvocation(content, label);
}

const REVIEWER_RUNTIME_GUARD_MARKERS = [
  'FEATUREFORGE_REVIEWER_RUNTIME_COMMANDS_ALLOWED',
  'FEATUREFORGE_REVIEWER_CONTEXT',
  'ReviewerRuntimeCommandForbidden',
  'REVIEWER_RUNTIME_ENV_CONTRACT',
  'REVIEWER_RUNTIME_COMMANDS_ALLOWED: no',
  'runtime command guard',
  'reviewer-mode environment',
  'reject_runtime_command_in_reviewer_context',
];

function listRuntimeRustSourceFiles() {
  const files = listRepoFiles()
    .filter((relPath) => relPath.startsWith(`src${path.sep}`) && relPath.endsWith('.rs'))
    .sort();
  assert.ok(files.length > 0, 'src should contain Rust source files');
  return files;
}

function assertRuntimeSourcesDoNotEnforceReviewerRecursionGuards() {
  const violations = [];
  for (const relPath of listRuntimeRustSourceFiles()) {
    const content = readUtf8(path.join(REPO_ROOT, relPath));
    for (const marker of REVIEWER_RUNTIME_GUARD_MARKERS) {
      if (content.includes(marker)) {
        violations.push(`${relPath}: ${marker}`);
      }
    }
  }
  assert.deepEqual(
    violations,
    [],
    'reviewer recursion should remain prompt-only, not runtime/env enforced in Rust sources',
  );
}

function assertSpecReviewerPromptKeepsRecursionRulePayloadOnly(content) {
  const prePayload = content.slice(0, content.indexOf('```'));
  assert.match(
    prePayload,
    /\$_FEATUREFORGE_ROOT\/references\/reviewer-recursion-rule\.md/,
    'spec reviewer template should point surrounding guidance at the canonical recursion prelude',
  );
  assert.doesNotMatch(
    prePayload,
    /Do not launch, request, or delegate to additional subagents while performing this review\./,
    'spec reviewer template should not duplicate the full recursion paragraph outside the dispatch payload',
  );
  assertReviewerSurfaceCarriesPromptScopedRecursionRule(
    extractDispatchPromptPayload(content, 'spec reviewer prompt'),
    'spec reviewer dispatch payload',
  );
}

function extractDispatchPromptPayload(content, label) {
  const match = content.match(/(?:^|\n)  prompt: \|\n([\s\S]*?)\n```/);
  assert.ok(match, `${label} should contain a dispatchable prompt: | payload`);
  return match[1];
}

function assertSeparatesCandidateArtifactsFromAuthoritativeMutations(content, label) {
  const violations = candidateAuthorityBoundaryViolations(content);
  assert.deepEqual(
    violations,
    [],
    `${label} must not describe candidate artifacts or implementers/subagents as direct runtime mutation authority`,
  );

  const hasCandidateNonAuthorityBoundary = containsOrderedCasefold(content, [
    'candidate artifacts',
    'not authoritative runtime mutation state',
  ]) || containsOrderedCasefold(content, [
    'candidate artifacts',
    'must not directly mutate runtime state',
  ]) || containsOrderedCasefold(content, [
    'candidate artifacts only',
    'do not authorize direct runtime state mutation',
  ]);
  const hasMutationProhibition = /must not directly mutate runtime (?:execution )?state|do not authorize direct runtime state mutation/i.test(content);
  const hasRuntimeOwner = /coordinator-owned|coordinator\/runtime owns|runtime owns/i.test(content);
  assert.ok(
    hasCandidateNonAuthorityBoundary && hasMutationProhibition && hasRuntimeOwner,
    `${label} should explicitly say candidate artifacts are not runtime mutation authority, prohibit direct implementer/subagent runtime mutation, and identify coordinator/runtime ownership`,
  );
}

function containsOrderedCasefold(content, orderedNeedles) {
  const normalized = content.toLowerCase();
  let searchStart = 0;
  for (const needle of orderedNeedles) {
    const index = normalized.indexOf(needle.toLowerCase(), searchStart);
    if (index < 0) {
      return false;
    }
    searchStart = index + needle.length;
  }
  return true;
}

function candidateAuthorityBoundaryViolations(content) {
  const normalized = content.toLowerCase();
  return [
    'candidate artifacts are authoritative runtime mutation state',
    'candidate edits are authoritative runtime mutation state',
    'task packets are authoritative runtime mutation state',
    'task packets authorize direct runtime state mutation',
    'handoff notes authorize direct runtime state mutation',
    'packet content authorizes direct runtime state mutation',
    'implementer helpers may directly mutate runtime execution state',
    'subagents may directly mutate runtime execution state',
    'implementer helpers directly mutate runtime execution state',
    'subagents directly mutate runtime execution state',
  ].filter((term) => normalized.includes(term));
}

function noteHelperGuidanceViolations(content) {
  return content
    .split('\n')
    .flatMap((line, index) => (lineHasNoteHelperGuidance(line)
      ? [`${index + 1}: retired note-helper guidance \`${line.trim()}\``]
      : []));
}

function lineHasNoteHelperGuidance(line) {
  const normalized = line.toLowerCase();
  if (normalized.includes('featureforge plan execution note')
      || normalized.includes('plan execution note --plan')) {
    return true;
  }
  const noteIndex = normalized.indexOf('`note`');
  if (noteIndex < 0) {
    return false;
  }
  const suffix = normalized.slice(noteIndex + '`note`'.length);
  const nounMatch = suffix.split(/[^a-z]+/).find((word) => word.length > 0);
  if (['command', 'helper'].includes(nounMatch)) {
    return true;
  }
  const prefix = normalized.slice(0, noteIndex);
  return prefix
    .split(/[^a-z]+/)
    .some((word) => ['run', 'invoke', 'call', 'use', 'execute', 'retry', 'follow'].includes(word));
}

function assertNoRemovedHelperCommandNames(content, label) {
  const normalized = content.toLowerCase();
  const violations = removedHelperCommandTerms()
    .filter((term) => normalized.includes(term.toLowerCase()))
    .concat(noteHelperGuidanceViolations(content));
  assert.deepEqual(
    violations,
    [],
    `${label} must not preserve removed helper command names in active prompt guidance`,
  );
}

function assertDownstreamMaterialStaysGateAndHarnessAware(content, label) {
  const hasGateAwareness = /(gate-review|review gate|finish-gate|gate-finish|fail closed)/i.test(content);
  const hasHarnessAwareness = /(execution evidence|task-packet|coverage matrix|source plan|source test plan|workflow-routed|artifact)/i.test(content);
  assert.ok(
    hasGateAwareness && hasHarnessAwareness,
    `${label} should stay downstream-gate-aware and harness-aware for review/QA handoffs`,
  );
}

function assertOrderedSubstrings(content, label, needles) {
  let previousIndex = -1;
  for (const needle of needles) {
    const index = content.indexOf(needle);
    assert.ok(index >= 0, `${label} should contain ${needle}`);
    assert.ok(
      index > previousIndex,
      `${label} should list ${needle} after the previous required boundary text`,
    );
    previousIndex = index;
  }
}

function assertTaskBoundaryClosureLoopSemantics(content, label) {
  const orderedPatterns = [
    /review\s+is\s+green[\s\S]{0,180}verification-before-completion[\s\S]{0,220}close-current-task/i,
    /dedicated-independent review result[\s\S]{0,260}workflow operator --plan <approved-plan-path> --external-review-result-ready --json[\s\S]{0,320}Installed Control Plane section[\s\S]{0,220}canonical route reference/i,
    /verification results[\s\S]{0,180}`?close-current-task`? inputs/i,
    /only after[\s\S]{0,120}close-current-task succeeds[\s\S]{0,180}Task `N\+1`/i,
  ];
  let previousIndex = -1;
  for (const pattern of orderedPatterns) {
    const match = pattern.exec(content);
    assert.ok(match, `${label} should satisfy task-boundary route contract ${pattern}`);
    assert.ok(
      match.index > previousIndex,
      `${label} should order task-boundary review, operator route, close-current-task input, and next-task dispatch semantics`,
    );
    previousIndex = match.index;
  }
}

function assertHighUseExecutionSkillDoesNotInlineDetailedClosureRouteTokens(content, label) {
  assert.doesNotMatch(
    content,
    /task_closure_recording_ready|task_review_dispatch_required|final_review_dispatch_required/,
    `${label} should delegate detailed closure/dispatch route tokens to operator-route-authority.md`,
  );
}

function listRepoFiles(dir = REPO_ROOT) {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    const relPath = path.relative(REPO_ROOT, fullPath);
    if (
      entry.isDirectory()
      && [
        '.git',
        'target',
        'node_modules',
        'docs/archive',
        'docs/featureforge/archive',
        'docs/project_notes',
        'tests/codex-runtime/fixtures/plan-contract/transition-only',
      ].some((prefix) => relPath === prefix || relPath.startsWith(`${prefix}${path.sep}`))
    ) {
      continue;
    }
    if (entry.isDirectory()) {
      files.push(...listRepoFiles(fullPath));
    } else if (entry.isFile()) {
      files.push(relPath);
    }
  }
  return files;
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function listActiveDocSkillAgentFiles() {
  return listRepoFiles()
    .filter((relPath) => {
      const activeRoot =
        relPath === 'README.md'
        || relPath === 'AGENTS.md'
        || relPath.startsWith(path.join('.codex', 'agents') + path.sep)
        || relPath.startsWith(`agents${path.sep}`)
        || relPath.startsWith(`docs${path.sep}`)
        || relPath.startsWith(`qa${path.sep}`)
        || relPath.startsWith(`references${path.sep}`)
        || relPath.startsWith(`review${path.sep}`)
        || relPath.startsWith(`skills${path.sep}`);
      const agentConfig = relPath.startsWith(path.join('.codex', 'agents') + path.sep)
        && relPath.endsWith('.toml');
      const textLike = relPath.endsWith('.md') || relPath.endsWith('.md.tmpl') || agentConfig;
      const explicitTestingDoc = relPath === path.join('docs', 'testing.md');
      return activeRoot
        && textLike
        && !explicitTestingDoc
        && !isDraftFeatureforgePlan(relPath)
        && !isAuditReferenceArtifact(relPath);
    });
}

function isDraftFeatureforgePlan(relPath) {
  if (!relPath.startsWith(`docs${path.sep}featureforge${path.sep}plans${path.sep}`)) {
    return false;
  }
  const content = readUtf8(path.join(REPO_ROOT, relPath));
  return /^## Workflow State\s*\n\s*Draft\b/im.test(content)
    || /^\*\*Workflow State:\*\*\s*Draft\b/im.test(content);
}

function isAuditReferenceArtifact(relPath) {
  return relPath.startsWith(`docs${path.sep}featureforge${path.sep}reference${path.sep}`)
    && /-(?:re)?audit\.md$/i.test(relPath);
}

const RETIRED_RUNTIME_HELPER_TERMS = [
  'record-review-dispatch',
  'gate-review',
  'gate-finish',
  'rebuild-evidence',
  'record-branch-closure',
  'record-release-readiness',
  'record-final-review',
  'record-qa',
  'record-contract',
  'record-evaluation',
  'record-handoff',
  'workflow plan-fidelity record',
  'plan_fidelity_receipt',
  'plan-fidelity-receipt',
  'workflow preflight',
  'workflow recommend',
  'plan execution preflight',
  'plan execution recommend',
  'execution-preflight-acceptance',
];

const RETIRED_NOTE_HELPER_TERMS = [
  'removed `note`',
  'removed note',
  'execution-note command',
];

const RETIRED_WORKFLOW_HELPER_TERMS = [
  'workflow expect',
  'workflow sync',
];

const IMPERATIVE_NOTE_HELPER_TERMS = [
  'invoke `note` for',
  'run `note` for',
  'use `note` for',
  'execute `note` for',
];

const NOTE_HELPER_CONTEXT_TERMS = [
  '`note` for blockers',
  '`note` for interruptions',
  '`note` for handoff',
];

const STALE_ROUTE_CUE_FORBIDDEN_TERMS = [
  'helper `request_code_review`',
  'request_code_review routing',
  'manual artifact inspection',
  'fall back to manual artifact inspection',
];

const ACTIVE_DOC_ONLY_FORBIDDEN_TERMS = [
  'unit-review receipts',
  'task-verification receipt',
  'receipt-ready',
  'Dedicated Reviewer Receipt Contract',
  'runtime-owned receipt',
  ...RETIRED_WORKFLOW_HELPER_TERMS,
  'refresh execution evidence',
  'refresh evidence',
  'repair review state / reenter execution',
  'repair review state or reenter execution',
  ...STALE_ROUTE_CUE_FORBIDDEN_TERMS,
  ...IMPERATIVE_NOTE_HELPER_TERMS,
  ...NOTE_HELPER_CONTEXT_TERMS,
  '--dispatch-id',
  '--branch-closure-id',
  'FEATUREFORGE_ALLOW_INTERNAL_EXECUTION_FLAGS',
  'repair unit-review proof',
  'repair the unit-review proof',
  'restore unit-review proof',
  'restore the unit-review proof',
  'record unit-review proof',
  'record the unit-review proof',
  'manually repair unit-review proof',
  'manual unit-review proof repair',
  'clear the parked downstream note',
  'clear or avoid the downstream parked note',
  'rebuild the stale Task 1 evidence',
  'set `**Last Reviewed By:** plan-eng-review` at the same time',
  'helper-built task packet',
  'helper-reported evidence path',
  'helper-reported execution state',
];

const RELEASE_NOTE_ONLY_RETIRED_HELPER_TERMS = [
  'hidden/debug',
  'hidden command',
  'debug command',
  'hidden compatibility/debug',
  'compatibility/debug',
  '`note` for',
  'featureforge plan execution note',
  'plan execution note --plan',
  'Plan-fidelity receipt',
];

const ACTIVE_DOC_FORBIDDEN_TERMS = [
  ...RETIRED_RUNTIME_HELPER_TERMS,
  ...RETIRED_NOTE_HELPER_TERMS,
  ...ACTIVE_DOC_ONLY_FORBIDDEN_TERMS,
];

const RELEASE_NOTE_RETIRED_HELPER_TERMS = [
  ...RETIRED_RUNTIME_HELPER_TERMS,
  ...RETIRED_NOTE_HELPER_TERMS,
  ...RELEASE_NOTE_ONLY_RETIRED_HELPER_TERMS,
];

function removedHelperCommandTerms() {
  return [
    ...new Set([
      ...RETIRED_RUNTIME_HELPER_TERMS.filter(isRetiredRuntimeCommandTerm),
      ...RETIRED_NOTE_HELPER_TERMS,
      ...RETIRED_WORKFLOW_HELPER_TERMS,
      ...IMPERATIVE_NOTE_HELPER_TERMS,
      ...NOTE_HELPER_CONTEXT_TERMS,
    ]),
  ];
}

function isRetiredRuntimeCommandTerm(term) {
  return term.startsWith('record-')
    || term.startsWith('gate-')
    || term.startsWith('workflow ')
    || term.startsWith('plan execution ')
    || term === 'rebuild-evidence'
    || term === 'execution-preflight-acceptance';
}

function staleRouteCueViolations(content, label) {
  const violations = [];
  for (const phrase of STALE_ROUTE_CUE_FORBIDDEN_TERMS) {
    const pattern = new RegExp(escapeRegExp(phrase), 'i');
    if (pattern.test(content)) {
      violations.push(`${label}: ${phrase}`);
    }
  }
  return violations;
}

test('active docs and agent-facing prompts do not expose forbidden receipt or hidden-helper vocabulary', () => {
  const violations = [];
  for (const relPath of listActiveDocSkillAgentFiles()) {
    const content = readUtf8(path.join(REPO_ROOT, relPath));
    for (const violation of noteHelperGuidanceViolations(content)) {
      violations.push(`${relPath}: ${violation}`);
    }
    for (const phrase of ACTIVE_DOC_FORBIDDEN_TERMS) {
      const pattern = new RegExp(escapeRegExp(phrase), 'i');
      if (pattern.test(content)) {
        violations.push(`${relPath}: ${phrase}`);
      }
    }
  }
  assert.deepEqual(
    violations,
    [],
    'active docs/skills/agent prompts must not teach agents forbidden receipt vocabulary',
  );
});

test('active testing and release-note surfaces do not reintroduce stale route cues', () => {
  const testingDoc = readUtf8(path.join(REPO_ROOT, 'docs/testing.md'));
  const releaseNotes = readUtf8(path.join(REPO_ROOT, 'RELEASE-NOTES.md'));
  const historicalIndex = releaseNotes.indexOf('Historical note:');
  assert.notEqual(
    historicalIndex,
    -1,
    'RELEASE-NOTES.md must keep a historical marker before retired route-cue examples',
  );
  const currentReleaseNotes = releaseNotes.slice(0, historicalIndex);

  assert.deepEqual(
    [
      ...staleRouteCueViolations(testingDoc, 'docs/testing.md'),
      ...staleRouteCueViolations(currentReleaseNotes, 'RELEASE-NOTES.md current section'),
    ],
    [],
    'docs/testing.md and current release notes must not teach stale helper or manual artifact fallback cues',
  );
});

function assertReleaseNotesRetiredHelperMentionsAreHistorical(content, label) {
  const historicalMarker = 'Historical note:';
  const historicalIndex = content.indexOf(historicalMarker);
  assert.notEqual(
    historicalIndex,
    -1,
    `${label} must explicitly mark old retired-helper mentions as historical`,
  );

  const lower = content.toLowerCase();
  const firstRetiredTermIndex = RELEASE_NOTE_RETIRED_HELPER_TERMS
    .map((term) => lower.indexOf(term.toLowerCase()))
    .filter((index) => index >= 0)
    .sort((left, right) => left - right)[0];
  if (firstRetiredTermIndex !== undefined) {
    assert.ok(
      historicalIndex < firstRetiredTermIndex,
      `${label} must place the historical marker before retired helper/provenance vocabulary`,
    );
  }

  const currentSection = content.slice(0, historicalIndex);
  const historicalSection = content.slice(historicalIndex);
  const violations = [];
  for (const [index, line] of currentSection.split('\n').entries()) {
    if (lineHasNoteHelperGuidance(line)) {
      violations.push(`${label}:${index + 1}: current release-note text mentions retired note-helper guidance`);
    }
    for (const term of RELEASE_NOTE_RETIRED_HELPER_TERMS) {
      if (line.toLowerCase().includes(term.toLowerCase())) {
        violations.push(`${label}:${index + 1}: current release-note text mentions retired term ${term}`);
      }
    }
  }

  const commandTerm = RELEASE_NOTE_RETIRED_HELPER_TERMS
    .map(escapeRegExp)
    .join('|');
  const imperativeRetiredCommand = new RegExp(
    String.raw`\b(?:run|retry|execute|invoke|follow|use)\b.{0,96}(?:${commandTerm})`,
    'i',
  );
  for (const [index, line] of historicalSection.split('\n').entries()) {
    if (imperativeRetiredCommand.test(line) || lineHasNoteHelperGuidance(line)) {
      violations.push(
        `${label}:${index + 1}: historical release-note text must not give imperative retired-helper guidance`,
      );
    }
  }

  assert.deepEqual(
    violations,
    [],
    `${label} must keep retired helper/provenance vocabulary historical and non-imperative; imperative retired-helper guidance is forbidden`,
  );
}

test('release notes keep retired helper vocabulary historical and non-imperative', () => {
  const releaseNotes = readUtf8(path.join(REPO_ROOT, 'RELEASE-NOTES.md'));
  assertReleaseNotesRetiredHelperMentionsAreHistorical(releaseNotes, 'RELEASE-NOTES.md');

  assert.throws(
    () => assertReleaseNotesRetiredHelperMentionsAreHistorical(
      [
        '# FeatureForge Release Notes',
        '',
        '## Unreleased',
        '- Run `featureforge plan execution gate-review` before closing the task.',
        '',
        'Historical note: older sections are historical.',
      ].join('\n'),
      'sample-active.md',
    ),
    /historical marker before retired helper|current release-note text mentions retired term/,
  );
  assert.throws(
    () => assertReleaseNotesRetiredHelperMentionsAreHistorical(
      [
        '# FeatureForge Release Notes',
        '',
        '## Unreleased',
        '- Re-run `record-contract` before dispatch.',
        '',
        'Historical note: older sections are historical.',
      ].join('\n'),
      'sample-active-contract-helper.md',
    ),
    /historical marker before retired helper|current release-note text mentions retired term/,
  );
  assert.throws(
    () => assertReleaseNotesRetiredHelperMentionsAreHistorical(
      [
        '# FeatureForge Release Notes',
        '',
        '## Unreleased',
        '- Invoke `note` to report blockers during implementation.',
        '',
        'Historical note: older sections are historical.',
      ].join('\n'),
      'sample-active-note-helper.md',
    ),
    /historical marker before retired helper|current release-note text mentions retired term|retired note-helper guidance|imperative retired-helper guidance is forbidden/,
  );
  assert.throws(
    () => assertReleaseNotesRetiredHelperMentionsAreHistorical(
      [
        '# FeatureForge Release Notes',
        '',
        '## Unreleased',
        '- Call `featureforge plan execution note --plan docs/featureforge/plans/example.md` when blocked.',
        '',
        'Historical note: older sections are historical.',
      ].join('\n'),
      'sample-active-note-command.md',
    ),
    /historical marker before retired helper|current release-note text mentions retired term|retired note-helper guidance|imperative retired-helper guidance is forbidden/,
  );
  assert.throws(
    () => assertReleaseNotesRetiredHelperMentionsAreHistorical(
      [
        '# FeatureForge Release Notes',
        '',
        '## Unreleased',
        '- Remove hidden/debug command recommendations from public routing.',
        '',
        'Historical note: older sections are historical.',
      ].join('\n'),
      'sample-active-hidden-debug.md',
    ),
    /historical marker before retired helper|current release-note text mentions retired term/,
  );
  assert.throws(
    () => assertReleaseNotesRetiredHelperMentionsAreHistorical(
      [
        '# FeatureForge Release Notes',
        '',
        '## Unreleased',
        '',
        'Historical note: older sections are historical.',
        '',
        '## v1.0.0',
        '- Run `featureforge plan execution rebuild-evidence` to repair the branch.',
      ].join('\n'),
      'sample-historical.md',
    ),
    /imperative retired-helper guidance/,
  );
  assert.doesNotThrow(() => {
    assertReleaseNotesRetiredHelperMentionsAreHistorical(
      [
        '# FeatureForge Release Notes',
        '',
        '## Unreleased',
        '- Current routes use typed public argv.',
        '',
        'Historical note: older sections below may mention hidden compatibility/debug commands as part of older contracts.',
        '',
        '## v1.0.0',
        '- split public `featureforge plan execution gate-review` into a read-only gate check and explicit mutation path (historical v1 contract)',
      ].join('\n'),
      'sample-allowed.md',
    );
  });
});

function commandAuthorityDocs() {
  return listActiveDocSkillAgentFiles()
    .filter((relPath) => {
      const content = readUtf8(path.join(REPO_ROOT, relPath));
      return content.includes('recommended_public_command_argv')
        || content.includes('recommended_command');
    });
}

const WORKFLOW_OPERATOR_JSON_FIELDS = [
  'phase',
  'phase_detail',
  'recommended_public_command_argv',
  'recommended_public_command_template',
  'required_inputs',
  'recording_context',
  'base_branch',
];

function workflowOperatorInstructionDocs() {
  const docs = [];
  for (const skill of listGeneratedSkills()) {
    docs.push(path.relative(REPO_ROOT, getSkillPath(skill)));
    const templatePath = getTemplatePath(skill);
    if (fs.existsSync(templatePath)) {
      docs.push(path.relative(REPO_ROOT, templatePath));
    }
  }
  return [...new Set(docs)];
}

function assertNoTextModeWorkflowOperatorJsonFieldConsumption(content, label) {
  const violations = [];
  const lines = content.split('\n');
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    if (!line.includes('workflow operator --plan')) continue;
    if (line.includes('--json')) continue;
    const window = lines.slice(index, Math.min(lines.length, index + 3)).join(' ');
    const consumedFields = WORKFLOW_OPERATOR_JSON_FIELDS.filter((field) => window.includes(field));
    if (consumedFields.length > 0) {
      violations.push(`${label}:${index + 1}: ${consumedFields.join(', ')}`);
    }
  }
  assert.deepEqual(
    violations,
    [],
    'workflow/operator instructions that consume JSON fields must use --json',
  );
}

test('workflow operator JSON-field instructions use JSON mode', () => {
  assert.throws(
    () => assertNoTextModeWorkflowOperatorJsonFieldConsumption(
      'Run `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path>` and follow `phase_detail`.',
      'sample.md',
    ),
    /must use --json/,
  );
  assert.doesNotThrow(() => {
    assertNoTextModeWorkflowOperatorJsonFieldConsumption(
      'Run `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` and follow `phase_detail`.',
      'sample.md',
    );
  });

  for (const relPath of workflowOperatorInstructionDocs()) {
    assertNoTextModeWorkflowOperatorJsonFieldConsumption(
      readUtf8(path.join(REPO_ROOT, relPath)),
      relPath,
    );
  }
});

function assertNoRecommendedCommandParsingGuidance(content, label) {
  const recommendedCommandRef = '\\brecommended_command\\b';
  const authorityVerb = [
    'parse',
    'parsed',
    'shell-parse',
    'shell-parsed',
    'whitespace-split',
    'split',
    'reconstruct',
    'recover',
    'derive',
    'build',
    'execute',
    'invoke',
    'run',
    'use',
    'follow',
    'call',
  ].join('|');
  const positiveBeforeDisplay = new RegExp(
    `\\b(?:${authorityVerb})\\b[^.\\n;]*?${recommendedCommandRef}`,
    'gi',
  );
  const displayBeforePositive = new RegExp(
    `${recommendedCommandRef}[^.\\n;]*?\\b(?:${authorityVerb})\\b|${recommendedCommandRef}[^.\\n;]*?\\bexact next command\\b`,
    'gi',
  );
  const strongProhibition = /\b(?:do not|don't|must not|never|cannot|can't|should not|may not|is not to|are not to|not to)\b/i;
  const prohibitionImmediatelyBeforeMatch =
    /\b(?:do not|don't|must not|never|cannot|can't|should not|may not|not to)(?:\s+be)?\s*$/i;
  const positiveTerm = new RegExp(`\\b(?:${authorityVerb})\\b|\\bexact next command\\b`, 'i');
  const argvInsteadOfDisplay = /\b(?:recommended_public_command_argv|argv(?: vector)?|returned argv)\b[^.\n;]*\b(?:instead of|rather than)\b[^.\n;]*\brecommended_command\b/i;

  const isPositiveBeforeDisplayNegated = (line, matchStart) => {
    const prefix = line.slice(Math.max(0, matchStart - 80), matchStart);
    return prohibitionImmediatelyBeforeMatch.test(prefix);
  };
  const isPositiveBeforeDisplayProhibitedWithinMatch = (matchText) => {
    const displayIndex = matchText.search(new RegExp(recommendedCommandRef, 'i'));
    if (displayIndex <= 0) return false;
    return strongProhibition.test(matchText.slice(0, displayIndex));
  };
  const isDisplayBeforePositiveNegated = (matchText) => {
    const afterDisplay = matchText.slice(matchText.search(new RegExp(recommendedCommandRef, 'i')) + 'recommended_command'.length);
    const termMatch = positiveTerm.exec(afterDisplay);
    if (!termMatch) return false;
    const beforePositiveTerm = afterDisplay.slice(0, termMatch.index);
    return strongProhibition.test(beforePositiveTerm)
      || /\b(?:is not|are not|not)\b[^.\n;]*\bauthoritative\b/i.test(matchText);
  };
  const violations = [];

  for (const [index, line] of content.split('\n').entries()) {
    if (!line.includes('recommended_command')) continue;
    const lineNumber = index + 1;
    for (const match of line.matchAll(positiveBeforeDisplay)) {
      const matchText = match[0];
      if (
        !argvInsteadOfDisplay.test(matchText)
        && !isPositiveBeforeDisplayNegated(line, match.index)
        && !isPositiveBeforeDisplayProhibitedWithinMatch(matchText)
      ) {
        violations.push(`${label}:${lineNumber}: ${matchText.trim()}`);
      }
    }
    for (const match of line.matchAll(displayBeforePositive)) {
      const matchText = match[0];
      if (
        !argvInsteadOfDisplay.test(matchText)
        && !isDisplayBeforePositiveNegated(matchText)
      ) {
        violations.push(`${label}:${lineNumber}: ${matchText.trim()}`);
      }
    }
  }

  assert.deepEqual(
    [...new Set(violations)],
    [],
    'active docs must not tell agents to parse, split, reconstruct, or execute recommended_command display text',
  );
}

test('recommended_command guidance scanner rejects display-string execution samples', () => {
  for (const sample of [
    'If `recommended_command` is not empty, run it as the next command.',
    'Use `recommended_command` when argv is missing.',
    'Follow `recommended_command` as the exact next command.',
    'Parse `recommended_command` to recover argv.',
    'Split `recommended_command` and invoke the tokens.',
    'Do not parse `recommended_command`; run `recommended_command` as the next command.',
    'Do not parse `recommended_command`, but run `recommended_command` as the next command.',
    'Do not parse `recommended_command` and run `recommended_command` as the next command.',
    'Use `recommended_command` instead of `recommended_public_command_argv`.',
    'Run `recommended_command` without checking argv.',
  ]) {
    assert.throws(
      () => assertNoRecommendedCommandParsingGuidance(sample, 'sample.md'),
      /active docs must not tell agents/,
      `scanner should reject: ${sample}`,
    );
  }
  assert.doesNotThrow(() => {
    assertNoRecommendedCommandParsingGuidance(
      'Do not parse `recommended_command`; it is display-only compatibility text.',
      'sample.md',
    );
  });
  assert.doesNotThrow(() => {
    assertNoRecommendedCommandParsingGuidance(
      'Follow `recommended_public_command_argv` instead of `recommended_command`.',
      'sample.md',
    );
  });
  assert.doesNotThrow(() => {
    assertNoRecommendedCommandParsingGuidance(
      'Run `recommended_public_command_argv`; do not parse `recommended_command`.',
      'sample.md',
    );
  });
  assert.doesNotThrow(() => {
    assertNoRecommendedCommandParsingGuidance(
      'Execute typed argv/template only; never execute display-only compatibility text `recommended_command`.',
      'sample.md',
    );
  });
});

test('active command docs never execute display recommended_command text', () => {
  for (const relPath of commandAuthorityDocs()) {
    const content = readUtf8WithGeneratedRouteAuthority(path.join(REPO_ROOT, relPath));
    assertNoRecommendedCommandParsingGuidance(content, relPath);
  }
});

function blockBetween(content, startNeedle, endNeedle, label) {
  const start = content.indexOf(startNeedle);
  assert.notEqual(start, -1, `${label} should contain ${startNeedle}`);
  const end = content.indexOf(endNeedle, start + startNeedle.length);
  return end >= 0 ? content.slice(start, end) : content.slice(start);
}

function assertNoFinalReviewHardcodedCommandTrap(content, label) {
  assert.doesNotMatch(
    content,
    /(?:^|\n)\s*(?:"\$_FEATUREFORGE_BIN"|\$_FEATUREFORGE_BIN|featureforge)\s+plan execution advance-late-stage[\s\S]{0,360}(?:reviewer-source|reviewer-id|summary-file|REVIEW_RESULT|SUMMARY_FILE)/,
    `${label} final-review block must not end operator-guided recording with an unconditional hard-coded advance-late-stage command`,
  );
}

function finalReviewMaterializerBlock(content, label) {
  const heading = '## Final-Review Recording Route Materializer';
  const headingStart = content.indexOf(heading);
  assert.notEqual(headingStart, -1, `${label} should contain ${heading}`);
  const fenceStart = content.indexOf('```bash', headingStart);
  assert.notEqual(fenceStart, -1, `${label} should contain a bash materializer block`);
  const fenceEnd = content.indexOf('\n```', fenceStart + '```bash'.length);
  assert.notEqual(fenceEnd, -1, `${label} should close the bash materializer block`);
  return content.slice(headingStart);
}

function assertFinalReviewRouteMaterializerContract(content, label) {
  const materializerBlock = finalReviewMaterializerBlock(content, label);
  assert.match(
    materializerBlock,
    /REVIEWER_SOURCE:\?Set REVIEWER_SOURCE/,
    `${label} final-review materializer should require the independent reviewer source before recording`,
  );
  assert.match(
    materializerBlock,
    /REVIEWER_ID:\?Set REVIEWER_ID/,
    `${label} final-review materializer should require the independent reviewer id before recording`,
  );
  assert.match(
    materializerBlock,
    /REVIEW_RESULT:\?Set REVIEW_RESULT=pass\|fail/,
    `${label} final-review materializer should require the concrete review result before recording`,
  );
  assert.match(
    materializerBlock,
    /SUMMARY_FILE:\?Set SUMMARY_FILE/,
    `${label} final-review materializer should require the concrete review summary before recording`,
  );
  assert.match(
    materializerBlock,
    /workflow operator[\s\S]{0,240}--external-review-result-ready/,
    `${label} final-review materializer should bind through workflow operator's final-review result-ready route`,
  );
  assert.match(
    materializerBlock,
    /--input "reviewer_source=\$REVIEWER_SOURCE"[\s\S]{0,160}--input "reviewer_id=\$REVIEWER_ID"[\s\S]{0,160}--input "result=\$REVIEW_RESULT"[\s\S]{0,160}--input "summary_file=\$SUMMARY_FILE"/,
    `${label} final-review materializer should pass concrete reviewer/result/summary bindings to workflow operator`,
  );
  assert.match(
    materializerBlock,
    /recommended_public_command_argv[\s\S]{0,160}_featureforge_exec_public_argv/,
    `${label} final-review materializer should execute only the Rust-materialized returned argv`,
  );
  assert.doesNotMatch(
    materializerBlock,
    /node > "\$ROUTE_ARGV_FILE"|ensureFinalReviewTemplate|required_when\.includes|Array\.isArray\(template\.input_bindings|process\.env\.RECORDING_READY_JSON/,
    `${label} final-review materializer should not duplicate Rust template semantics in prompt-side JavaScript`,
  );
  assert.match(
    materializerBlock,
    /still returns `recommended_public_command_template` or does not return executable argv, stop and report `RECORDING_READY_JSON`/,
    `${label} final-review materializer should fail closed if Rust does not return executable argv`,
  );
}

test('requesting-code-review final route materializer delegates template binding to workflow operator', () => {
  const content = readUtf8(path.join(REPO_ROOT, 'references/operator-route-authority.md'));
  assertFinalReviewRouteMaterializerContract(content, 'references/operator-route-authority.md');
  assert.doesNotMatch(content, /node > "\$ROUTE_ARGV_FILE"/);
  assert.doesNotMatch(content, /ensureFinalReviewTemplate/);
  assert.match(content, /--external-review-result-ready[\s\S]{0,260}--input "reviewer_source=\$REVIEWER_SOURCE"/);
  assert.match(content, /--input "reviewer_id=\$REVIEWER_ID"/);
  assert.match(content, /--input "result=\$REVIEW_RESULT"/);
  assert.match(content, /--input "summary_file=\$SUMMARY_FILE"/);
});

function assertNoQaRequirementHardcodedRepairTrap(content, label) {
  const section = blockBetween(
    content,
    '### Step 1.85: Conditional Pre-Landing QA Gate',
    '### Step 1.9: Finish Gate',
    label,
  );
  assert.doesNotMatch(
    section,
    /QA Requirement[\s\S]{0,520}(?:\$_FEATUREFORGE_BIN\s+)?plan execution repair-review-state/,
    `${label} QA Requirement invalid/missing block must not hard-code repair-review-state`,
  );
  assert.match(
    section,
    /QA Requirement[\s\S]{0,260}workflow operator --plan <approved-plan-path> --json[\s\S]{0,260}Installed Control Plane section[\s\S]{0,180}canonical route reference/,
    `${label} QA Requirement invalid/missing block should route through the compact public route law`,
  );
}

function assertNoReviewedClosureFallbackRepairCommand(content, label) {
  assert.doesNotMatch(
    content,
    /recommended_public_command_template\.input_bindings[\s\S]{0,260}(?:otherwise|or)\s+run\s+`?\$_FEATUREFORGE_BIN plan execution repair-review-state --plan <approved-plan-path>`? only when the non-diagnostic route owns that repair lane/i,
    `${label} must not preserve a hard-coded repair-review-state fallback after typed argv/template routing`,
  );
  assert.doesNotMatch(
    content,
    /When workflow\/operator JSON reports `?review_state_status`? as stale or missing closure context[\s\S]{0,760}(?:otherwise|or)\s+run\s+`?\$_FEATUREFORGE_BIN plan execution repair-review-state --plan <approved-plan-path>`?/i,
    `${label} stale or missing closure guidance must stop or use typed operator argv/template instead of hard-coding repair-review-state`,
  );
  assert.doesNotMatch(
    content,
    /\$_FEATUREFORGE_BIN plan execution repair-review-state --plan <approved-plan-path>/,
    `${label} must not hard-code repair-review-state as an active recovery command`,
  );
}

function assertNoLateStageLiteralCommandShapes(content, label) {
  assert.match(
    content,
    /references\/operator-route-authority\.md/,
    `${label} should link the canonical late-stage route law instead of duplicating it`,
  );
  assert.doesNotMatch(
    content,
    /Late-stage aggregate route coverage:|Release-readiness routes bind|Final-review routes bind|QA routes bind/,
    `${label} should not duplicate detailed late-stage route law outside the canonical reference`,
  );
  assert.doesNotMatch(
    content,
    /\$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path>/,
    `${label} must not include literal advance-late-stage shell command shapes in the operator-guided matrix`,
  );
}

function assertNoOperatorGuidedLateStageLiteralCommand(content, label) {
  assert.doesNotMatch(
    content,
    /\$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <(?:approved-plan-)?path>/,
    `${label} must not hard-code literal advance-late-stage shell command shapes for operator-guided late-stage routes`,
  );
  assert.doesNotMatch(
    content,
    /\$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path>/,
    `${label} must not hard-code approved-plan advance-late-stage shell command shapes`,
  );
}

function assertNoLowLevelPrimitiveEscapeHatch(content, label) {
  assert.doesNotMatch(
    content,
    /Compatibility-only escape hatch: use low-level runtime primitives only when explicitly debugging or preserving compatibility/,
    `${label} must not preserve low-level runtime primitive escape-hatch guidance`,
  );
  assert.doesNotMatch(
    content,
    /Treat low-level runtime primitives as compatibility\/debug-only surfaces unless workflow\/operator explicitly routes to them/,
    `${label} must not invite agents to look for low-level primitives when typed public route authority is absent`,
  );
  assert.doesNotMatch(
    content,
    /Low-level compatibility finish commands remain expert\/debug-only surfaces/,
    `${label} must not preserve low-level finish command escape-hatch guidance`,
  );
  assert.match(
    content,
    /stop and report the route diagnostic/,
    `${label} should tell agents to stop on missing typed route authority`,
  );
}

test('late-stage prompt command traps are rejected at block scope', () => {
  assertFinalReviewRouteMaterializerContract(
    readUtf8(path.join(REPO_ROOT, 'references/operator-route-authority.md')),
    'references/operator-route-authority.md',
  );

  for (const [label, content] of [
    ['skills/requesting-code-review/SKILL.md.tmpl', readUtf8(getTemplatePath('requesting-code-review'))],
    ['skills/requesting-code-review/SKILL.md', readUtf8(getSkillPath('requesting-code-review'))],
  ]) {
    assertNoFinalReviewHardcodedCommandTrap(content, label);
  }

  for (const [label, content] of [
    ['skills/finishing-a-development-branch/SKILL.md.tmpl', readUtf8(getTemplatePath('finishing-a-development-branch'))],
    ['skills/finishing-a-development-branch/SKILL.md', readUtf8(getSkillPath('finishing-a-development-branch'))],
  ]) {
    assertNoQaRequirementHardcodedRepairTrap(content, label);
  }

  for (const [label, content] of [
    ['skills/executing-plans/SKILL.md.tmpl', readUtf8WithGeneratedRouteAuthority(getTemplatePath('executing-plans'))],
    ['skills/executing-plans/SKILL.md', readUtf8(getSkillPath('executing-plans'))],
    ['skills/subagent-driven-development/SKILL.md.tmpl', readUtf8WithGeneratedRouteAuthority(getTemplatePath('subagent-driven-development'))],
    ['skills/subagent-driven-development/SKILL.md', readUtf8(getSkillPath('subagent-driven-development'))],
    ['README.md', readUtf8(path.join(REPO_ROOT, 'README.md'))],
    ['docs/README.codex.md', readUtf8(path.join(REPO_ROOT, 'docs/README.codex.md'))],
    ['docs/README.copilot.md', readUtf8(path.join(REPO_ROOT, 'docs/README.copilot.md'))],
    [
      'docs/featureforge/reference/2026-04-01-review-state-reference.md',
      readUtf8(path.join(REPO_ROOT, 'docs/featureforge/reference/2026-04-01-review-state-reference.md')),
    ],
  ]) {
    assertNoReviewedClosureFallbackRepairCommand(content, label);
  }

  for (const [label, content] of [
    ['skills/executing-plans/SKILL.md.tmpl', readUtf8WithGeneratedRouteAuthority(getTemplatePath('executing-plans'))],
    ['skills/executing-plans/SKILL.md', readUtf8(getSkillPath('executing-plans'))],
    ['skills/subagent-driven-development/SKILL.md.tmpl', readUtf8WithGeneratedRouteAuthority(getTemplatePath('subagent-driven-development'))],
    ['skills/subagent-driven-development/SKILL.md', readUtf8(getSkillPath('subagent-driven-development'))],
  ]) {
    assertNoLateStageLiteralCommandShapes(content, label);
  }

  for (const [label, content] of [
    ['skills/finishing-a-development-branch/SKILL.md.tmpl', readUtf8(getTemplatePath('finishing-a-development-branch'))],
    ['skills/finishing-a-development-branch/SKILL.md', readUtf8(getSkillPath('finishing-a-development-branch'))],
    [
      'docs/featureforge/reference/2026-04-01-review-state-reference.md',
      readUtf8(path.join(REPO_ROOT, 'docs/featureforge/reference/2026-04-01-review-state-reference.md')),
    ],
  ]) {
    assertNoOperatorGuidedLateStageLiteralCommand(content, label);
  }

  for (const [label, content] of [
    ['skills/executing-plans/SKILL.md.tmpl', readUtf8WithGeneratedRouteAuthority(getTemplatePath('executing-plans'))],
    ['skills/executing-plans/SKILL.md', readUtf8(getSkillPath('executing-plans'))],
    ['skills/subagent-driven-development/SKILL.md.tmpl', readUtf8WithGeneratedRouteAuthority(getTemplatePath('subagent-driven-development'))],
    ['skills/subagent-driven-development/SKILL.md', readUtf8(getSkillPath('subagent-driven-development'))],
    ['skills/using-featureforge/SKILL.md.tmpl', readUtf8WithGeneratedRouteAuthority(getTemplatePath('using-featureforge'))],
    ['skills/using-featureforge/SKILL.md', readUtf8(getSkillPath('using-featureforge'))],
    ['skills/finishing-a-development-branch/SKILL.md.tmpl', readUtf8WithGeneratedRouteAuthority(getTemplatePath('finishing-a-development-branch'))],
    ['skills/finishing-a-development-branch/SKILL.md', readUtf8(getSkillPath('finishing-a-development-branch'))],
  ]) {
    assertNoLowLevelPrimitiveEscapeHatch(content, label);
  }

  assert.throws(
    () => assertNoFinalReviewHardcodedCommandTrap(
      [
        '## Minimal Terminal Final-Review Command Shape',
        '',
        '```bash',
        '# recommended_public_command_argv is authoritative and recommended_public_command_template is available',
        '"$_FEATUREFORGE_BIN" plan execution advance-late-stage --plan "$APPROVED_PLAN_PATH" --reviewer-source fresh-context-subagent --reviewer-id <id> --result "$REVIEW_RESULT" --summary-file "$SUMMARY_FILE"',
        '```',
      ].join('\n'),
      'sample-review.md',
    ),
    /must not end operator-guided recording/,
  );

  assert.throws(
    () => assertNoQaRequirementHardcodedRepairTrap(
      [
        '### Step 1.85: Conditional Pre-Landing QA Gate',
        '',
        'If approved-plan `QA Requirement` is missing or invalid when deciding whether QA applies, stop.',
        '',
        'Then run:',
        '`$_FEATUREFORGE_BIN plan execution repair-review-state --plan <path>`',
      ].join('\n'),
      'sample-finish.md',
    ),
    /typed argv\/template authority|must not hard-code repair-review-state/,
  );

  assert.throws(
    () => assertNoReviewedClosureFallbackRepairCommand(
      [
        'When workflow/operator JSON reports `review_state_status` as stale or missing closure context, do not invent a repair command.',
        'If a template is present, use `required_inputs` as validation metadata and bind locally through `recommended_public_command_template.input_bindings`; otherwise run `$_FEATUREFORGE_BIN plan execution repair-review-state --plan <approved-plan-path>` only when the non-diagnostic route owns that repair lane.',
      ].join(' '),
      'sample-execution.md',
    ),
    /must not preserve a hard-coded repair-review-state fallback|must stop or use typed operator argv\/template/,
  );

  assert.throws(
    () => assertNoLateStageLiteralCommandShapes(
      [
        'Late-stage aggregate route coverage:',
        '- `$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path> --result ready|blocked --summary-file <release-summary>`',
      ].join('\n'),
      'sample-execution.md',
    ),
    /canonical late-stage route law|must not include literal advance-late-stage shell command shapes/,
  );
});

function buildTimedHookPatterns(timings, targetPattern, gapPattern = '[^.\\n]{0,160}') {
  const obligationPattern = '(?:must|always|required|requires|should|need(?:s)? to|have(?:s)? to|ought to)';
  const imperativeActionPattern = '(?:consult|search|update|use|record)';
  const timingPattern = `(?:${timings.join('|')})`;

  return [
    new RegExp(`${timingPattern}${gapPattern}${obligationPattern}${gapPattern}${targetPattern}`, 'i'),
    new RegExp(`${obligationPattern}${gapPattern}${targetPattern}${gapPattern}${timingPattern}`, 'i'),
    new RegExp(`${targetPattern}${gapPattern}${obligationPattern}${gapPattern}${timingPattern}`, 'i'),
    new RegExp(`${timingPattern}${gapPattern}${targetPattern}${gapPattern}${obligationPattern}`, 'i'),
    new RegExp(`${timingPattern}${gapPattern}${imperativeActionPattern}${gapPattern}${targetPattern}`, 'i'),
    new RegExp(`${imperativeActionPattern}${gapPattern}${targetPattern}${gapPattern}${timingPattern}`, 'i'),
    new RegExp(`${obligationPattern}${gapPattern}featureforge:project-memory${gapPattern}${timingPattern}`, 'i'),
    new RegExp(`featureforge:project-memory${gapPattern}${obligationPattern}${gapPattern}${timingPattern}`, 'i'),
    new RegExp(`featureforge:project-memory${gapPattern}${timingPattern}${gapPattern}${obligationPattern}`, 'i'),
    new RegExp(`${imperativeActionPattern}${gapPattern}featureforge:project-memory${gapPattern}${timingPattern}`, 'i'),
    new RegExp(`featureforge:project-memory${gapPattern}${imperativeActionPattern}${gapPattern}${timingPattern}`, 'i'),
    new RegExp(`featureforge:project-memory${gapPattern}${timingPattern}${gapPattern}${imperativeActionPattern}`, 'i'),
    new RegExp(`featureforge:project-memory${gapPattern}${timingPattern}`, 'i'),
    new RegExp(`${timingPattern}${gapPattern}featureforge:project-memory`, 'i'),
    new RegExp(`featureforge:project-memory${gapPattern}${obligationPattern}${gapPattern}${targetPattern}${gapPattern}${timingPattern}`, 'i'),
    new RegExp(`featureforge:project-memory${gapPattern}${timingPattern}${gapPattern}${obligationPattern}${gapPattern}${targetPattern}`, 'i'),
    new RegExp(`featureforge:project-memory${gapPattern}${timingPattern}${gapPattern}${imperativeActionPattern}${gapPattern}${targetPattern}`, 'i'),
    new RegExp(`${imperativeActionPattern}${gapPattern}featureforge:project-memory${gapPattern}${timingPattern}${gapPattern}${targetPattern}`, 'i'),
    new RegExp(`${imperativeActionPattern}${gapPattern}featureforge:project-memory${gapPattern}${targetPattern}${gapPattern}${timingPattern}`, 'i'),
    new RegExp(`featureforge:project-memory${gapPattern}${timingPattern}${gapPattern}${targetPattern}`, 'i'),
    new RegExp(`featureforge:project-memory${gapPattern}${targetPattern}${gapPattern}${timingPattern}`, 'i'),
  ];
}

function assertForbidsTimedObligationHook(content, label, description, timings, targetPattern) {
  const patterns = buildTimedHookPatterns(timings, targetPattern);
  for (const pattern of patterns) {
    assert.doesNotMatch(content, pattern, `${label} should not turn ${description} into a timed obligation`);
  }
}

function assertDetectsTimedHookSamples(samples, label, description, timings, targetPattern) {
  const patterns = buildTimedHookPatterns(timings, targetPattern, '[^\\n]{0,160}');
  for (const sample of samples) {
    assert.ok(
      patterns.some((pattern) => pattern.test(sample)),
      `${label} should detect timed regressions for ${description}: ${sample}`,
    );
  }
}

function buildGateLikeHookPatterns(targetPattern, gapPattern = '[^.\\n]{0,160}') {
  const subjectPattern = `(?:featureforge:project-memory|${targetPattern})`;
  const gatePattern = '(?:prerequisite|required|required for|gate|gates?|blocks?|blocked|blocking|mandatory|depends on|blocked on)';

  return [
    new RegExp(`${subjectPattern}${gapPattern}(?:is|are|be|being|to be)?${gapPattern}${gatePattern}`, 'i'),
    new RegExp(`${gatePattern}${gapPattern}${subjectPattern}`, 'i'),
  ];
}

function assertForbidsGateLikeHookLanguage(content, label, description, targetPattern) {
  const patterns = buildGateLikeHookPatterns(targetPattern);
  for (const pattern of patterns) {
    assert.doesNotMatch(content, pattern, `${label} should not turn ${description} into gate-like language`);
  }
}

function assertDetectsGateLikeHookSamples(samples, label, description, targetPattern) {
  const patterns = buildGateLikeHookPatterns(targetPattern, '[^\\n]{0,160}');
  for (const sample of samples) {
    assert.ok(
      patterns.some((pattern) => pattern.test(sample)),
      `${label} should detect gate-like regressions for ${description}: ${sample}`,
    );
  }
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function assertContainsTextFragments(content, fragments, label) {
  for (const fragment of fragments) {
    assert.ok(
      content.includes(fragment),
      `${label} should include semantic contract fragment: ${fragment}`,
    );
  }
}

function assertLineContainsTextFragments(content, needle, fragments, label) {
  const line = content
    .split(/\r?\n/)
    .find((candidate) => (
      candidate.includes(needle)
      && fragments.every((fragment) => candidate.includes(fragment))
    ));
  assert.ok(
    line,
    `${label} should include one disclosure line for ${needle} with fragments: ${fragments.join(', ')}`,
  );
  assertContainsTextFragments(line, fragments, `${label} ${needle}`);
}

function assertSkillCarriesProgressProjectionLaw(content, label) {
  assertContainsTextFragments(
    content,
    [
      'approved plan checklist',
      'human-visible execution progress projection',
      'event log remains authoritative',
      'do not create or maintain a separate ad hoc task tracker',
    ],
    label,
  );
}

test('active review-state reference lists the generated public state-kind taxonomy', () => {
  assert.deepEqual(
    reviewStateReferenceStateKindValues(),
    workflowOperatorStateKindValues(),
    'active review-state reference should stay aligned with generated workflow operator state_kind enum values',
  );
});

test('templates declare exactly one base or review preamble placeholder', () => {
  for (const skill of listGeneratedSkills()) {
    const template = readUtf8(getTemplatePath(skill));
    const hasBase = template.includes('{{BASE_PREAMBLE}}');
    const hasReview = template.includes('{{REVIEW_PREAMBLE}}');
    assert.notEqual(hasBase, hasReview, `${skill} should declare exactly one preamble placeholder`);
  }
});

test('generated preamble bash block includes shared runtime-root and state binding without extra session boilerplate', () => {
  for (const skill of listGeneratedSkills()) {
    if (skill === 'using-featureforge') continue;
    const content = readUtf8(getSkillPath(skill));
    const bashBlock = extractBashBlockUnderHeading(content, 'Preamble (run first)');
    assert.ok(bashBlock, `${skill} should include a preamble bash block`);
    assert.match(bashBlock, /repo runtime-root --path/, `${skill} should resolve runtime roots through the helper contract`);
    assert.match(bashBlock, /\$HOME\/\.featureforge\/install/, `${skill} should pin runtime commands to the canonical install root`);
    assert.match(bashBlock, /featureforge\.exe/, `${skill} should keep the Windows packaged launcher path in the install-root contract`);
    assert.match(bashBlock, /_FEATUREFORGE_STATE_DIR="\$\{FEATUREFORGE_STATE_DIR:-\$HOME\/\.featureforge\}"/, `${skill} should bind the shared state dir`);
    assert.doesNotMatch(bashBlock, /_IS_FEATUREFORGE_RUNTIME_ROOT\(\)/, `${skill} should not embed its own runtime-root detector`);
    assertNoRuntimeFallbackExecution(bashBlock, `${skill} preamble`);
    assert.doesNotMatch(bashBlock, /sed -n/, `${skill} should not parse runtime-root JSON in shell`);
    assert.doesNotMatch(bashBlock, /"\$_FEATUREFORGE_BIN" update-check/, `${skill} should not auto-run update checks in every generated preamble`);
    assert.doesNotMatch(bashBlock, /"\$_FEATUREFORGE_BIN" config get featureforge_contributor/, `${skill} should not load contributor mode in every generated preamble shell block`);
    assert.doesNotMatch(bashBlock, /_SESSIONS=/, `${skill} should not track session count in every generated preamble`);
    assert.doesNotMatch(bashBlock, /_CONTRIB=/, `${skill} should not inject contributor config lookup into every generated preamble`);
  }
});

test('install docs describe the path-based runtime-root helper contract', () => {
  for (const relativePath of ['.codex/INSTALL.md', '.copilot/INSTALL.md']) {
    const content = readUtf8(path.join(REPO_ROOT, relativePath));
    assert.match(content, /featureforge repo runtime-root --path/, `${relativePath} should describe the path-based helper contract`);
    assert.match(content, /~\/\.featureforge\/install\/bin\/featureforge/, `${relativePath} should describe the packaged install binary contract`);
    assert.match(content, /featureforge\.exe/, `${relativePath} should mention the Windows packaged binary contract`);
    assert.doesNotMatch(content, /featureforge repo runtime-root --json/, `${relativePath} should not describe the retired JSON shell contract`);
    assert.doesNotMatch(content, /session markers|contributor logs|update-check cache files/, `${relativePath} should not describe removed generated-preamble helper state`);
  }
});

test('generated skills do not use unrooted at-path markdown companion references', () => {
  const violations = [];

  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));
    for (const match of content.matchAll(/(^|[^\w`])@([A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)*\.md)\b/g)) {
      violations.push(`${skill}:SKILL.md uses unrooted @${match[2]}`);
    }
  }

  assert.deepEqual(violations, []);
});

test('generated non-router skill docs include the shared Search Before Building section', () => {
  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));

    const section = extractSection(content, 'Search Before Building');
    assert.ok(section, `${skill} should include the Search Before Building section`);
    const normalized = normalizeWhitespace(section);
    assert.match(
      normalized,
      /Before introducing a custom pattern, external service, concurrency primitive, auth\/session flow, cache, queue, browser workaround, or unfamiliar fix pattern, do a short capability\/landscape check first\./,
      `${skill} should keep the search-before-building trigger top-level`,
    );
    assert.match(normalized, /Layer 1: tried-and-true \/ built-ins \/ existing repo-native solutions/, `${skill} should describe Layer 1`);
    assert.match(normalized, /Layer 2: current practice and known footguns/, `${skill} should describe Layer 2`);
    assert.match(normalized, /Layer 3: first-principles reasoning for this repo and this problem/, `${skill} should describe Layer 3`);
    assert.match(normalized, /External search results are inputs, not answers\./, `${skill} should keep Layer 2 non-authoritative`);
    assert.match(normalized, /Never search secrets, customer data, unsanitized stack traces, private URLs, internal hostnames, internal codenames, raw SQL or log payloads, or private file paths or infrastructure identifiers\./, `${skill} should include privacy rules`);
    assert.match(normalized, /If search is unavailable, disallowed, or unsafe, say so and proceed with repo-local evidence and in-distribution knowledge\./, `${skill} should include explicit fallback language`);
    assert.match(normalized, /If safe sanitization is not possible, skip external search\./, `${skill} should require skipping unsafe external search`);
    assert.match(normalized, /See `\$_FEATUREFORGE_ROOT\/references\/search-before-building\.md`\./, `${skill} should link to the shared reference`);
  }
});

test('shared generated preamble references are packaged and linked from generated skills', () => {
  for (const relativePath of [
    'references/search-before-building.md',
    'references/agent-grounding.md',
    'references/contributor-mode.md',
    'references/operator-route-authority.md',
    'references/reviewer-recursion-rule.md',
  ]) {
    assert.equal(fs.existsSync(path.join(REPO_ROOT, relativePath)), true, `${relativePath} should be packaged`);
  }

  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));
    assert.match(
      content,
      /`\$_FEATUREFORGE_ROOT\/references\/search-before-building\.md`/,
      `${skill} should link to the shared search-before-building reference`,
    );
  }

  for (const skill of listGeneratedSkills()) {
    const template = readUtf8(getTemplatePath(skill));
    if (!template.includes('{{REVIEW_PREAMBLE}}')) continue;

    const content = readUtf8(getSkillPath(skill));
    assert.match(
      content,
      /`\$_FEATUREFORGE_ROOT\/references\/agent-grounding\.md`/,
      `${skill} should link to the shared agent-grounding reference`,
    );
    assert.match(
      content,
      /`\$_FEATUREFORGE_ROOT\/references\/contributor-mode\.md`/,
      `${skill} should link to the shared contributor-mode reference`,
    );
  }

  for (const skill of [
    'using-featureforge',
    'plan-eng-review',
    'requesting-code-review',
    'document-release',
    'finishing-a-development-branch',
    'executing-plans',
    'subagent-driven-development',
  ]) {
    const content = readUtf8(getSkillPath(skill));
    assert.match(
      content,
      /`\$_FEATUREFORGE_ROOT\/references\/operator-route-authority\.md`/,
      `${skill} should link to the shared operator route authority reference`,
    );
  }
});

test('source archive verifier protects active prompt companion references', () => {
  const requiredArchivePaths = new Set(REQUIRED_SOURCE_ARCHIVE_PATHS);
  const requiredPromptCompanionPaths = new Set(REQUIRED_PROMPT_COMPANION_SOURCE_ARCHIVE_PATHS);
  const requiredSkillCompanionAssetPaths = new Set(REQUIRED_SKILL_COMPANION_ASSET_SOURCE_ARCHIVE_PATHS);
  const skillCompanionPathPattern = '[^`]+\\.(?:md|js|sh|ps1|dot|html)';
  const skillLocalCompanionRegex = new RegExp(`skill-local \`(${skillCompanionPathPattern})\``, 'g');
  const rootedSkillCompanionRegex = new RegExp(
    `\`\\$_FEATUREFORGE_ROOT/(skills/${skillCompanionPathPattern})\``,
    'g',
  );
  const skillDirVariableCompanionRegex = new RegExp(
    `\\$[A-Za-z0-9_]*SKILL_DIR/(${skillCompanionPathPattern})`,
    'g',
  );
  const skillDirPowerShellCompanionRegex = new RegExp(
    `\\$[A-Za-z0-9_]*SkillDir\\\\(${skillCompanionPathPattern.replaceAll('/', '\\\\')})`,
    'g',
  );
  const addSkillCompanionReferences = (skill, content, discoveredPaths) => {
    for (const match of content.matchAll(skillLocalCompanionRegex)) {
      discoveredPaths.add(path.posix.join('skills', skill, match[1]));
    }
    for (const match of content.matchAll(rootedSkillCompanionRegex)) {
      discoveredPaths.add(match[1]);
    }
    for (const match of content.matchAll(skillDirVariableCompanionRegex)) {
      discoveredPaths.add(path.posix.join('skills', skill, match[1]));
    }
    for (const match of content.matchAll(skillDirPowerShellCompanionRegex)) {
      discoveredPaths.add(path.posix.join('skills', skill, match[1].replaceAll('\\', '/')));
    }
  };
  const skillNameFromCompanionPath = (relativePath) => {
    const match = relativePath.match(/^skills\/([^/]+)\//);
    return match?.[1] ?? null;
  };
  const sharedCompanionPaths = [
    'docs/featureforge/reference/2026-04-01-review-state-reference.md',
    'qa/references/issue-taxonomy.md',
    'references/agent-grounding.md',
    'references/contributor-mode.md',
    'references/debugging-tdd-examples.md',
    'references/execution-review-qa-examples.md',
    'references/operator-route-authority.md',
    'references/plan-ceo-review-rubric.md',
    'references/plan-eng-review-rubric.md',
    'references/reviewer-recursion-rule.md',
    'references/search-before-building.md',
    'references/writing-plans-examples.md',
    'review/TODOS-format.md',
    'review/checklist.md',
    'review/late-stage-precedence-reference.md',
    'review/plan-task-contract.md',
    'review/review-accelerator-packet-contract.md',
  ];
  const projectMemoryTemplatePaths = [
    'skills/project-memory/references/bugs_template.md',
    'skills/project-memory/references/decisions_template.md',
    'skills/project-memory/references/issues_template.md',
    'skills/project-memory/references/key_facts_template.md',
  ];
  const skillLocalCompanionPaths = new Set(projectMemoryTemplatePaths);
  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));
    addSkillCompanionReferences(skill, content, skillLocalCompanionPaths);
  }

  const skillLocalCompanionQueue = [...skillLocalCompanionPaths];
  const queuedSkillLocalCompanions = new Set(skillLocalCompanionQueue);
  const scannedSkillLocalCompanions = new Set();
  for (let queueIndex = 0; queueIndex < skillLocalCompanionQueue.length; queueIndex += 1) {
    const relativePath = skillLocalCompanionQueue[queueIndex];
    if (scannedSkillLocalCompanions.has(relativePath)) {
      continue;
    }
    scannedSkillLocalCompanions.add(relativePath);
    const skill = skillNameFromCompanionPath(relativePath);
    if (!skill || !fs.existsSync(path.join(REPO_ROOT, relativePath))) {
      continue;
    }
    const beforeScanSize = skillLocalCompanionPaths.size;
    addSkillCompanionReferences(
      skill,
      readUtf8(path.join(REPO_ROOT, relativePath)),
      skillLocalCompanionPaths,
    );
    if (skillLocalCompanionPaths.size > beforeScanSize) {
      for (const discoveredPath of skillLocalCompanionPaths) {
        if (
          !scannedSkillLocalCompanions.has(discoveredPath)
          && !queuedSkillLocalCompanions.has(discoveredPath)
        ) {
          skillLocalCompanionQueue.push(discoveredPath);
          queuedSkillLocalCompanions.add(discoveredPath);
        }
      }
    }
  }

  for (const requiredPath of [
    'skills/subagent-driven-development/implementer-prompt.md',
    'skills/subagent-driven-development/spec-reviewer-prompt.md',
    'skills/subagent-driven-development/code-quality-reviewer-prompt.md',
    'skills/plan-fidelity-review/reviewer-prompt.md',
    'skills/plan-eng-review/accelerated-reviewer-prompt.md',
    'skills/plan-eng-review/outside-voice-prompt.md',
    'skills/plan-ceo-review/accelerated-reviewer-prompt.md',
    'skills/plan-ceo-review/outside-voice-prompt.md',
    'skills/project-memory/authority-boundaries.md',
    'skills/project-memory/examples.md',
    'skills/writing-skills/codex-best-practices.md',
    'skills/writing-skills/copilot-best-practices.md',
    'skills/writing-skills/examples/AGENTS_MD_TESTING.md',
    'skills/writing-skills/persuasion-principles.md',
    'skills/writing-skills/testing-skills-with-subagents.md',
    'skills/brainstorming/scripts/helper.js',
    'skills/brainstorming/scripts/start-server.ps1',
    'skills/brainstorming/scripts/start-server.sh',
    'skills/brainstorming/scripts/stop-server.ps1',
    'skills/brainstorming/scripts/stop-server.sh',
    'skills/brainstorming/scripts/frame-template.html',
    'skills/systematic-debugging/find-polluter.sh',
    'skills/writing-skills/graphviz-conventions.dot',
    'skills/writing-skills/render-graphs.js',
  ]) {
    assert.equal(
      skillLocalCompanionPaths.has(requiredPath),
      true,
      `${requiredPath} should be discovered from active skill companion references`,
    );
  }

  for (const relativePath of [...sharedCompanionPaths, ...skillLocalCompanionPaths].sort()) {
    assert.equal(fs.existsSync(path.join(REPO_ROOT, relativePath)), true, `${relativePath} should exist`);
    assert.equal(
      requiredArchivePaths.has(relativePath),
      true,
      `${relativePath} should be required by scripts/verify-source-archive.mjs`,
    );
    if (relativePath.endsWith('.md')) {
      assert.equal(
        requiredPromptCompanionPaths.has(relativePath),
        true,
        `${relativePath} should be classified as a prompt companion archive path`,
      );
    } else {
      assert.equal(
        requiredSkillCompanionAssetPaths.has(relativePath),
        true,
        `${relativePath} should be classified as a skill companion asset archive path`,
      );
    }
  }
});

test('source archive verifier executes only when run directly', () => {
  const scriptPath = path.join(REPO_ROOT, 'scripts/verify-source-archive.mjs');
  const importOnly = spawnSync(
    process.execPath,
    [
      '--input-type=module',
      '-e',
      `import ${JSON.stringify(pathToFileURL(scriptPath).href)}; console.log('import-only');`,
    ],
    {
      cwd: REPO_ROOT,
      encoding: 'utf8',
    },
  );
  assert.equal(importOnly.status, 0, importOnly.stderr);
  assert.equal(importOnly.stdout.trim(), 'import-only');

  const directRun = spawnSync(process.execPath, [scriptPath], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
  });
  assert.equal(directRun.status, 0, directRun.stderr);
  assert.match(directRun.stdout, /Source archive validation passed\./);

  const pathWithSpaces = path.join('/tmp', 'Feature Forge source archive', 'scripts', 'verify-source-archive.mjs');
  assert.equal(
    isDirectScriptRun(pathToFileURL(pathWithSpaces).href, pathWithSpaces),
    true,
    'direct-run guard should compare decoded filesystem paths',
  );
  assert.equal(
    isDirectScriptRun(pathToFileURL(pathWithSpaces).href, path.join('/tmp', 'different-script.mjs')),
    false,
    'direct-run guard should reject non-matching argv paths',
  );
});

test('canonical operator route authority reference owns detailed typed route law', () => {
  const reference = readUtf8(path.join(REPO_ROOT, 'references/operator-route-authority.md'));
  for (const pattern of [
    /`phase`[\s\S]{0,220}`recording_context`[\s\S]{0,120}public route contract/i,
    /`recommended_command`[\s\S]{0,120}display-only compatibility text[\s\S]{0,120}Do not parse, split, or execute it\./i,
    /Generated shell blocks may provide `_featureforge_exec_public_argv`[\s\S]{0,180}fails closed for any other argv\[0\]/i,
    /bind concrete values by rerunning the same operator query with `workflow operator --plan <approved-plan-path> --input NAME=VALUE --json`[\s\S]{0,120}`recommended_public_command_argv`/,
    /neither executable argv nor a bindable template[\s\S]{0,120}stop and report the route diagnostic/i,
    /`next_action` alone[\s\S]{0,80}executable routing authority/i,
    /`resume_task`[\s\S]{0,40}`resume_step`[\s\S]{0,120}advisory diagnostics/i,
    /After `repair-review-state`, follow that command's returned `recommended_public_command_argv`/,
    /`task_closure_recording_ready`[\s\S]{0,120}`recording_context\.task_number`/,
    /`phase_detail=task_closure_recording_ready`[\s\S]{0,160}run `close-current-task`/,
    /`\*_dispatch_required` lanes[\s\S]{0,160}low-level dispatch-lineage management/,
    /Do not use the internal task-closure recording service boundary directly[\s\S]{0,80}Use `close-current-task`/,
    /Late-stage aggregate route coverage:/,
  ]) {
    assert.match(reference, pattern, 'operator-route-authority.md should own detailed route law');
  }
  assert.doesNotMatch(
    reference,
    /workflow operator --input NAME=VALUE --json/,
    'operator route authority must not teach an incomplete template-binding query without --plan',
  );
});

test('active docs and prompts keep template materialization plan-bound', () => {
  const checkedPaths = [
    'README.md',
    'docs/README.codex.md',
    'docs/README.copilot.md',
    'docs/featureforge/reference/2026-04-01-review-state-reference.md',
    'docs/featureforge/specs/2026-05-04-workflow-doctor-headless-recovery-design.md',
    'references/operator-route-authority.md',
    'review/late-stage-precedence-reference.md',
    ...listGeneratedSkills().flatMap((skill) => [
      `skills/${skill}/SKILL.md`,
      `skills/${skill}/SKILL.md.tmpl`,
    ]),
  ];
  const violations = [];
  const incompleteInputPatterns = [
    /\$_FEATUREFORGE_BIN workflow operator --json/i,
    /workflow\/operator with `--input NAME=VALUE --json`/i,
    /operator `--input NAME=VALUE --json`/i,
    /rerun workflow\/operator with `--input NAME=VALUE --json`/i,
    /workflow operator --input NAME=VALUE --json/i,
  ];
  const directTemplatePatterns = [
    /Use recommended_public_command_argv or recommended_public_command_template instead/i,
    /continue through recommended_public_command_argv or recommended_public_command_template/i,
    /executable authority remains recommended_public_command_argv or recommended_public_command_template/i,
    /follow typed recommended_public_command_argv or recommended_public_command_template/i,
  ];
  for (const relPath of checkedPaths) {
    const content = readUtf8(path.join(REPO_ROOT, relPath));
    for (const pattern of incompleteInputPatterns) {
      if (pattern.test(content)) {
        violations.push(`${relPath}: incomplete non-plan-bound operator --input guidance matched ${pattern}`);
      }
    }
    for (const pattern of directTemplatePatterns) {
      if (pattern.test(content)) {
        violations.push(`${relPath}: template guidance reads like a second executable path via ${pattern}`);
      }
    }
  }
  assert.deepEqual(violations, []);
});

test('planning and plan-review compaction references are packaged and linked from owning skills', () => {
  for (const relativePath of [
    'references/plan-ceo-review-rubric.md',
    'references/plan-eng-review-rubric.md',
    'references/writing-plans-examples.md',
  ]) {
    assert.equal(fs.existsSync(path.join(REPO_ROOT, relativePath)), true, `${relativePath} should be packaged`);
  }

  assert.match(
    readUtf8(getSkillPath('plan-ceo-review')),
    /`\$_FEATUREFORGE_ROOT\/references\/plan-ceo-review-rubric\.md`/,
    'plan-ceo-review should link to its compacted rubric reference',
  );
  assert.match(
    readUtf8(getSkillPath('plan-eng-review')),
    /`\$_FEATUREFORGE_ROOT\/references\/plan-eng-review-rubric\.md`/,
    'plan-eng-review should link to its compacted rubric reference',
  );
  assert.match(
    readUtf8(getSkillPath('writing-plans')),
    /`\$_FEATUREFORGE_ROOT\/references\/writing-plans-examples\.md`/,
    'writing-plans should link to its compacted examples reference',
  );
});

test('execution review QA and debugging compaction references are packaged and linked from owning skills', () => {
  for (const relativePath of [
    'references/execution-review-qa-examples.md',
    'references/debugging-tdd-examples.md',
  ]) {
    assert.equal(fs.existsSync(path.join(REPO_ROOT, relativePath)), true, `${relativePath} should be packaged`);
  }

  for (const skill of [
    'executing-plans',
    'subagent-driven-development',
    'finishing-a-development-branch',
    'requesting-code-review',
    'document-release',
    'qa-only',
  ]) {
    assert.match(
      readUtf8(getSkillPath(skill)),
      /`\$_FEATUREFORGE_ROOT\/references\/execution-review-qa-examples\.md`/,
      `${skill} should link to compacted execution/review/QA examples`,
    );
  }

  for (const skill of ['systematic-debugging', 'test-driven-development']) {
    assert.match(
      readUtf8(getSkillPath(skill)),
      /`\$_FEATUREFORGE_ROOT\/references\/debugging-tdd-examples\.md`/,
      `${skill} should link to compacted debugging/TDD examples`,
    );
  }
});

test('generated skill companion references resolve from installed or skill-local contexts', () => {
  const violations = [];
  const surfaces = [];

  for (const skill of listGeneratedSkills()) {
    const skillDir = path.join(SKILLS_DIR, skill);
    surfaces.push({
      label: `${skill}:SKILL.md`,
      baseDir: skillDir,
      content: readUtf8(getSkillPath(skill)),
    });
  }

  const collectPromptSurfaces = (dir, predicate, baseDirForFile) => {
    if (!fs.existsSync(dir)) return;
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const entryPath = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        collectPromptSurfaces(entryPath, predicate, baseDirForFile);
        continue;
      }
      if (!predicate(entryPath)) continue;
      const rel = path.relative(REPO_ROOT, entryPath).replaceAll(path.sep, '/');
      surfaces.push({
        label: rel,
        baseDir: baseDirForFile(entryPath),
        content: readUtf8(entryPath),
      });
    }
  };

  collectPromptSurfaces(
    SKILLS_DIR,
    (filePath) =>
      ['.md', '.tmpl'].includes(path.extname(filePath)) && path.basename(filePath) !== 'SKILL.md',
    (filePath) => {
      const relative = path.relative(SKILLS_DIR, filePath).split(path.sep);
      return path.join(SKILLS_DIR, relative[0]);
    },
  );
  collectPromptSurfaces(
    path.join(REPO_ROOT, 'agents'),
    (filePath) => ['.md'].includes(path.extname(filePath)),
    (filePath) => path.dirname(filePath),
  );
  collectPromptSurfaces(
    path.join(REPO_ROOT, '.codex', 'agents'),
    (filePath) => ['.toml'].includes(path.extname(filePath)),
    (filePath) => path.dirname(filePath),
  );
  for (const rel of ['review/checklist.md', 'review/review-accelerator-packet-contract.md']) {
    surfaces.push({
      label: rel,
      baseDir: path.dirname(path.join(REPO_ROOT, rel)),
      content: readUtf8(path.join(REPO_ROOT, rel)),
    });
  }

  for (const { label, baseDir, content } of surfaces) {
    for (const match of content.matchAll(/`(\$_FEATUREFORGE_ROOT\/[^`]+)`/g)) {
      const referenced = match[1].replace('$_FEATUREFORGE_ROOT/', '');
      const target = path.join(REPO_ROOT, referenced);
      if (!fs.existsSync(target)) {
        violations.push(`${label}: installed-root reference \`${match[1]}\` does not exist`);
      }
    }

    for (const match of content.matchAll(/`(\$_REPO_ROOT\/[^`]+)`/g)) {
      const referenced = match[1].replace('$_REPO_ROOT/', '');
      const target = path.join(REPO_ROOT, referenced);
      if (!fs.existsSync(target)) {
        violations.push(`${label}: repo-root reference \`${match[1]}\` does not exist`);
      }
    }

    for (const match of content.matchAll(/skill-local `([^`]+)`/g)) {
      const referenced = match[1];
      const target = path.join(baseDir, referenced);
      if (!fs.existsSync(target)) {
        violations.push(`${label}: skill-local reference \`${referenced}\` does not exist relative to ${baseDir}`);
      }
    }

    for (const match of content.matchAll(/`((?:\.\/)?[A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)*\.[A-Za-z0-9]+)`/g)) {
      const referenced = match[1].replace(/^\.\//, '');
      if (referenced === 'SKILL.md' || referenced === 'SKILL.md.tmpl') {
        continue;
      }
      const target = path.join(baseDir, referenced);
      if (!fs.existsSync(target)) {
        continue;
      }
      const prefix = content.slice(Math.max(0, match.index - 24), match.index);
      if (!/\bskill-local\s+$/.test(prefix)) {
        violations.push(
          `${label}: local companion reference \`${match[1]}\` resolves relative to ${baseDir} but is not marked skill-local`,
        );
      }
    }

    for (const fence of content.matchAll(/```(?:bash|sh|zsh|shell|powershell|pwsh)\n([\s\S]*?)```/g)) {
      for (const line of fence[1].split(/\r?\n/)) {
        for (const match of line.matchAll(/(^|[\s;&|])((?:\.\/)?scripts[\/\\][A-Za-z0-9_.\/\\-]+\.(?:sh|ps1|js|mjs|html?))/g)) {
          const referenced = match[2].replace(/^\.\//, '').replaceAll('\\', '/');
          const target = path.join(baseDir, referenced);
          if (!fs.existsSync(target)) {
            continue;
          }
          violations.push(
            `${label}: fenced command example uses skill-local companion path \`${match[2]}\` without an explicit skill directory root`,
          );
        }
      }
    }

    for (const match of content.matchAll(/`((?:review|docs\/featureforge\/reference|references)\/[^`]+)`/g)) {
      const prefix = content.slice(Math.max(0, match.index - 24), match.index);
      if (!/\bskill-local\s+$/.test(prefix)) {
        violations.push(
          `${label}: \`${match[1]}\` should be explicitly skill-local or rooted at $_FEATUREFORGE_ROOT/$_REPO_ROOT`,
        );
      }
    }
  }

  assert.deepEqual(violations, []);
});

test('writing-skills compaction keeps authoring gates and companion references top-level', () => {
  for (const relativePath of [
    'skills/writing-skills/codex-best-practices.md',
    'skills/writing-skills/copilot-best-practices.md',
    'skills/writing-skills/testing-skills-with-subagents.md',
    'skills/writing-skills/persuasion-principles.md',
    'skills/writing-skills/graphviz-conventions.dot',
    'skills/writing-skills/render-graphs.js',
  ]) {
    assert.equal(fs.existsSync(path.join(REPO_ROOT, relativePath)), true, `${relativePath} should be packaged`);
  }

  const writingSkills = readUtf8(getSkillPath('writing-skills'));
  for (const expectedPath of [
    '$_FEATUREFORGE_ROOT/skills/writing-skills/codex-best-practices.md',
    '$_FEATUREFORGE_ROOT/skills/writing-skills/copilot-best-practices.md',
    '$_FEATUREFORGE_ROOT/skills/writing-skills/testing-skills-with-subagents.md',
    '$_FEATUREFORGE_ROOT/skills/writing-skills/persuasion-principles.md',
    '$_FEATUREFORGE_ROOT/skills/writing-skills/graphviz-conventions.dot',
  ]) {
    assert.match(
      writingSkills,
      new RegExp(escapeRegex(expectedPath)),
      `writing-skills should link top-level to ${expectedPath}`,
    );
  }

  assert.match(writingSkills, /`SKILL\.md` must start with YAML frontmatter containing only:/);
  assert.match(writingSkills, /`name`: lowercase letters, numbers, and hyphens\./);
  assert.match(writingSkills, /`description`: third-person trigger text that starts with `Use when\.\.\.`; describe when to load the skill, not the workflow it will follow\./);
  assert.match(writingSkills, /Keep generated top-level skill docs under their manifest budget in `skills\/skill-doc-budgets\.json`\./);
  assert.match(writingSkills, /Do not use absolute local paths in checked-in skill text\./);
  assert.match(writingSkills, /Do not use `@path` links for ordinary references; they force-load files and waste context\./);
  assert.match(writingSkills, /Edit `skills\/<skill>\/SKILL\.md\.tmpl` when it exists\./);
  assert.match(writingSkills, /Run `node scripts\/gen-skill-docs\.mjs --check`\./);
  assert.match(writingSkills, /Never hand-edit generated `SKILL\.md` output while leaving its template stale\./);
  assert.match(writingSkills, /Keep Codex and GitHub Copilot behavior aligned:/);
  assert.match(writingSkills, /The iron law: no new skill or material skill edit without a failing pressure scenario first\./);
  assert.match(writingSkills, /Stop after each skill and complete this checklist before starting another skill\./);
});

test('using-featureforge omits the removed bypass-gate contract', () => {
  const content = readUtf8(getSkillPath('using-featureforge'));
  const bootstrapBlock = extractBashBlockUnderHeading(content, 'Preamble (run first)');
  assert.match(bootstrapBlock, /_FEATUREFORGE_STATE_DIR="\$\{FEATUREFORGE_STATE_DIR:-\$HOME\/\.featureforge\}"/, 'using-featureforge should bind the shared state dir directly');
  assert.doesNotMatch(bootstrapBlock, /touch "\$_FEATUREFORGE_STATE_DIR\/sessions\/\$PPID"/, 'using-featureforge should not carry session-marker boilerplate in the shared preamble');
  assert.doesNotMatch(bootstrapBlock, /_CONTRIB=/, 'using-featureforge should not carry contributor-mode lookup in the shared preamble shell block');
  assertNoRuntimeFallbackExecution(bootstrapBlock, 'using-featureforge preamble');
  assert.doesNotMatch(content, /## Bypass Gate/, 'using-featureforge should not keep the removed bypass-gate section');
  assert.doesNotMatch(content, /## Normal FeatureForge Stack/, 'using-featureforge should not keep the removed post-gate normal-stack section');
  assert.doesNotMatch(content, /session-entry\/using-featureforge/, 'using-featureforge should not derive the removed decision-file path');
  assert.doesNotMatch(content, /featureforge session-entry resolve --message-file <path>/, 'using-featureforge should not reference the removed session-entry helper flow');
  assert.doesNotMatch(content, /ask one interactive question before any normal FeatureForge work happens/, 'using-featureforge should not keep bypass-gate prompt prose');
  assert.doesNotMatch(content, /FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY/, 'using-featureforge should not export the removed strict gate env key');
  assert.doesNotMatch(content, /FEATUREFORGE_SPAWNED_SUBAGENT/, 'using-featureforge should not mention the removed spawned-subagent gate env key');
  assert.doesNotMatch(content, /FEATUREFORGE_SPAWNED_SUBAGENT_OPT_IN/, 'using-featureforge should not mention the removed spawned-subagent opt-in env key');
  assert.doesNotMatch(content, /featureforge-session-entry/, 'using-featureforge should not keep helper-style session-entry commands');
});

test('generated skill docs omit removed session-entry env markers across active surfaces', () => {
  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));
    assert.doesNotMatch(content, /FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY/, `${skill} should not mention the removed strict gate env key`);
    assert.doesNotMatch(content, /FEATUREFORGE_SPAWNED_SUBAGENT/, `${skill} should not mention the removed spawned-subagent env key`);
    assert.doesNotMatch(content, /FEATUREFORGE_SPAWNED_SUBAGENT_OPT_IN/, `${skill} should not mention the removed spawned-subagent opt-in env key`);
  }
});

test('generated skill docs never execute runtime commands through root-selected launchers', () => {
  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));
    assertNoRuntimeFallbackExecution(content, `${skill} generated skill doc`);
  }
});

test('all shipped runtime docs keep execution pinned to the packaged binary contract', () => {
  // This is intentionally redundant with the narrower checks above. We want a
  // broad sweep over shipped docs so fallback resolution cannot quietly return
  // through a different surface later. Do not relax this without an explicit
  // product decision to stop shipping and trusting the packaged install binary.
  const runtimeDocs = [
    ['featureforge-upgrade/SKILL.md', readUtf8(path.join(REPO_ROOT, 'featureforge-upgrade', 'SKILL.md'))],
    ...listGeneratedSkills().map((skill) => [path.join('skills', skill, 'SKILL.md'), readUtf8(getSkillPath(skill))]),
  ];

  for (const [label, content] of runtimeDocs) {
    assertNoRuntimeFallbackExecution(content, label);
  }
});

test('upgrade instructions keep runtime command execution separate from companion-file lookup', () => {
  const upgradeSkill = readUtf8(path.join(REPO_ROOT, 'featureforge-upgrade', 'SKILL.md'));
  const installRuntimeExecPattern = /(?:^|\n)\s*(?:if|while|until)?\s*!?\s*"\$INSTALL_RUNTIME_BIN"\s|\$\("\$INSTALL_RUNTIME_BIN"\s/;

  // Intentional invariant: INSTALL_RUNTIME_BIN is only for locating the
  // packaged binary inside the resolved install root for file-oriented steps.
  // Runtime commands must continue to flow through FEATUREFORGE_RUNTIME_BIN so
  // a future refactor cannot silently reintroduce root-selected execution.
  assert.match(upgradeSkill, /INSTALL_RUNTIME_BIN=/);
  assert.doesNotMatch(upgradeSkill, installRuntimeExecPattern, 'upgrade flow should not execute runtime commands through INSTALL_RUNTIME_BIN');
  assert.doesNotMatch(upgradeSkill, /FEATUREFORGE_RUNTIME_BIN="\$INSTALL_RUNTIME_BIN"/, 'upgrade flow should not rebind FEATUREFORGE_RUNTIME_BIN from INSTALL_RUNTIME_BIN');
});

test('generated preambles capture _BRANCH exactly once and keep helper BRANCH out of grounding', () => {
  const branchAssignmentPattern = /(?:^|\n)_BRANCH=/g;

  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));
    const bashBlock = extractBashBlockUnderHeading(content, 'Preamble (run first)');
    const totalAssignments = content.match(branchAssignmentPattern) ?? [];
    const preambleAssignments = bashBlock.match(branchAssignmentPattern) ?? [];
    assert.equal(totalAssignments.length, 1, `${skill} should include one _BRANCH assignment in the full doc`);
    assert.equal(preambleAssignments.length, 1, `${skill} should capture _BRANCH in the preamble`);
    assert.doesNotMatch(bashBlock, /\bBRANCH=/, `${skill} should not define helper BRANCH in the preamble`);
  }
});

test('generated branch-aware helper loads are guarded through _SLUG_ENV and eval the captured output only', () => {
  for (const skill of ['qa-only', 'plan-eng-review', 'finishing-a-development-branch']) {
    const content = readUtf8(getSkillPath(skill));
    assert.match(content, /_SLUG_ENV=\$\("\$_FEATUREFORGE_BIN" repo slug 2>\/dev\/null \|\| true\)/, `${skill} should capture canonical command output into _SLUG_ENV`);
    assert.match(content, /if \[ -n "\$_SLUG_ENV" \]; then\n\s+eval "\$_SLUG_ENV"\nfi/, `${skill} should only eval guarded helper output`);
    assert.doesNotMatch(content, /eval "\$\("\$_FEATUREFORGE_BIN" repo slug\)/, `${skill} should not unguardedly eval command substitution`);
  }
});

test('branch-aware skill docs consume the slug helper instead of inline sanitization fragments', () => {
  for (const skill of ['qa-only', 'plan-eng-review', 'finishing-a-development-branch']) {
    const content = readUtf8(getSkillPath(skill));
    assert.match(content, /"\$_FEATUREFORGE_BIN" repo slug/, `${skill} should use the canonical repo slug command through the packaged install binary`);
    assert.doesNotMatch(content, /SAFE_BRANCH=\$\(/, `${skill} should not inline branch sanitization`);
    assert.doesNotMatch(content, /(?:^|[^_])BRANCH=\$\(git rev-parse --abbrev-ref HEAD/, `${skill} should not inline raw branch capture`);
    assert.doesNotMatch(content, /SLUG=\$\(printf '%s\\n' "\$REMOTE_URL"/, `${skill} should not inline repo slug derivation`);
  }
});

test('branch-aware artifact selectors compare headers to captured current branch', () => {
  for (const skill of ['qa-only', 'finishing-a-development-branch']) {
    const content = readUtf8(getSkillPath(skill));
    const bashBlock = extractBashBlockUnderHeading(content, 'Preamble (run first)');
    assert.match(
      content,
      /if \[ "\$ARTIFACT_BRANCH" = "\$_BRANCH" \]; then/,
      `${skill} should match artifact Branch headers against captured _BRANCH`,
    );
    assert.doesNotMatch(
      content,
      /if \[ "\$ARTIFACT_BRANCH" = "\$BRANCH" \]; then/,
      `${skill} should not match Branch headers against slug helper BRANCH`,
    );
    assert.doesNotMatch(bashBlock, /\$BRANCH/, `${skill} should not use helper BRANCH in the grounding preamble`);
  }
});

test('review skills include review-only preamble contract', () => {
  for (const skill of listGeneratedSkills()) {
    const template = readUtf8(getTemplatePath(skill));
    if (!template.includes('{{REVIEW_PREAMBLE}}')) continue;

    const content = readUtf8(getSkillPath(skill));
    const bashBlock = extractBashBlockUnderHeading(content, 'Preamble (run first)');
    assert.match(bashBlock, /_TODOS_FORMAT=/, `${skill} should load TODO format state`);
    assert.match(content, /## Agent Grounding/, `${skill} should include Agent Grounding`);
  }
});

test('interactive question contract appears once per generated skill in normalized form', () => {
  const expectedBits = [
    '1. Context: project name, current branch, what we\'re working on (1-2 sentences)',
    '2. The specific question or decision point',
    '3. `RECOMMENDATION: Choose [X] because [one-line reason]`',
    '4. Lettered options: `A) ... B) ... C) ...`',
  ];

  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));
    assert.equal(countOccurrences(content, '## Interactive User Question Format'), 1, `${skill} should define the interactive question format once`);
    const section = extractSection(content, 'Interactive User Question Format');
    assert.ok(section, `${skill} should include the interactive question format section`);
    const normalized = normalizeWhitespace(section);
    for (const bit of expectedBits) {
      assert.match(normalized, new RegExp(bit.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')), `${skill} should include ${bit}`);
    }
  }
});

test('workflow fixture coverage is owned by local Node fixtures instead of historical docs paths', () => {
  const content = readUtf8(path.join(REPO_ROOT, 'tests/codex-runtime/workflow-fixtures.test.mjs'));
  assert.match(content, /tests\/codex-runtime\/fixtures\/workflow-artifacts/);
  assert.doesNotMatch(content, /docs\/featureforge\/specs\/2026-/);
  assert.doesNotMatch(content, /docs\/featureforge\/plans\/2026-/);
});

test('runtime safety audit archive is indexed instead of active per-loop noise', () => {
  const indexRel = 'docs/featureforge/archive/runtime-safety-audit-history/README.md';
  const index = readUtf8(path.join(REPO_ROOT, indexRel));
  assert.match(index, /## Retention Rule/);
  assert.match(index, /## Current Active Runtime-Safety Plan/);
  assert.match(index, /active docs should link here instead of linking individual per-loop reports or remediation plans/i);
  assert.equal(
    REQUIRED_SOURCE_ARCHIVE_PATHS.includes(indexRel),
    true,
    'source archive verifier should package the runtime-safety audit-history index',
  );

  for (const activeDoc of ['README.md', 'docs/testing.md']) {
    assert.match(
      readUtf8(path.join(REPO_ROOT, activeDoc)),
      new RegExp(escapeRegExp(indexRel)),
      `${activeDoc} should point at the audit-history index instead of individual per-loop files`,
    );
  }

  const activePlanMatch = index.match(/- `docs\/featureforge\/plans\/([^`]+)`/);
  assert.ok(activePlanMatch, 'audit-history index should name the current active runtime-safety plan');
  const activeRuntimeSafetyPlans = fs
    .readdirSync(path.join(REPO_ROOT, 'docs/featureforge/plans'))
    .filter((name) => /runtime-safety.*audit.*remediation/.test(name))
    .sort();
  assert.deepEqual(
    activeRuntimeSafetyPlans,
    [activePlanMatch[1]],
    'only the current runtime-safety remediation plan should remain in the active plans directory',
  );

  for (const archivedName of [
    '2026-05-14-runtime-safety-thirty-second-audit-remediation.md',
    '2026-05-14-runtime-safety-thirty-third-audit-remediation.md',
    '2026-05-15-runtime-safety-thirty-fourth-audit-remediation.md',
  ]) {
    assert.equal(
      fs.existsSync(path.join(REPO_ROOT, 'docs/featureforge/archive/runtime-safety-audit-history/plans', archivedName)),
      true,
      `${archivedName} should be retained in the audit-history archive`,
    );
    assert.equal(
      fs.existsSync(path.join(REPO_ROOT, 'docs/featureforge/plans', archivedName)),
      false,
      `${archivedName} should not remain as active per-loop plan noise`,
    );
  }
});

test('broad-safe skill descriptions expand discovery language without taking over workflow authority', () => {
  const expected = {
    'using-featureforge': [/which skill/i, /workflow stage applies/i],
    'brainstorming': [/feature idea/i, /architecture direction/i],
    'systematic-debugging': [/regression/i],
    'document-release': [/release notes/i, /handoff documentation/i],
    'qa-only': [/repro steps/i, /screenshots/i],
  };

  for (const [skill, patterns] of Object.entries(expected)) {
    const description = getSkillDescription(skill);
    for (const pattern of patterns) {
      assert.match(description, pattern, `${skill} description should broaden discovery with ${pattern}`);
    }
  }
});

test('workflow-critical skill descriptions encode approval-stage prerequisites', () => {
  const expected = {
    'plan-ceo-review': [/written FeatureForge design or architecture spec/i, /before implementation planning/i],
    'writing-plans': [/CEO-approved FeatureForge spec/i, /write the implementation plan/i],
    'plan-eng-review': [/written FeatureForge implementation plan/i, /CEO-approved spec/i],
    'subagent-driven-development': [/engineering-approved FeatureForge implementation plan/i, /mostly independent tasks/i],
    'executing-plans': [/engineering-approved FeatureForge implementation plan/i, /separate session/i],
    'requesting-code-review': [/after implementation work/i, /intentional review checkpoint/i],
    'finishing-a-development-branch': [/implementation is complete/i, /verification passes/i],
  };

  for (const [skill, patternOrPatterns] of Object.entries(expected)) {
    const description = getSkillDescription(skill);
    const patterns = Array.isArray(patternOrPatterns) ? patternOrPatterns : [patternOrPatterns];
    for (const pattern of patterns) {
      assert.match(description, pattern, `${skill} description should encode the required workflow gate`);
    }
  }
});

test('execution and review skill docs keep candidate artifacts and downstream gates explicit', () => {
  const executingPlans = readUtf8(getSkillPath('executing-plans'));
  const subagentSkill = readUtf8(getSkillPath('subagent-driven-development'));
  const implementerPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/implementer-prompt.md'));
  const reviewSkill = readUtf8(getSkillPath('requesting-code-review'));
  const qaSkill = readUtf8(getSkillPath('qa-only'));

  for (const [content, label] of [
    [executingPlans, 'skills/executing-plans/SKILL.md'],
    [subagentSkill, 'skills/subagent-driven-development/SKILL.md'],
    [implementerPrompt, 'skills/subagent-driven-development/implementer-prompt.md'],
  ]) {
    assertNoRemovedHelperCommandNames(content, label);
  }

  assertSeparatesCandidateArtifactsFromAuthoritativeMutations(executingPlans, 'skills/executing-plans/SKILL.md');
  assertSeparatesCandidateArtifactsFromAuthoritativeMutations(subagentSkill, 'skills/subagent-driven-development/SKILL.md');
  assertSeparatesCandidateArtifactsFromAuthoritativeMutations(implementerPrompt, 'skills/subagent-driven-development/implementer-prompt.md');
  assertDownstreamMaterialStaysGateAndHarnessAware(reviewSkill, 'skills/requesting-code-review/SKILL.md');
  assertDownstreamMaterialStaysGateAndHarnessAware(qaSkill, 'skills/qa-only/SKILL.md');
});

test('high-use execution templates delegate route law to shared generated surfaces', () => {
  for (const skill of ['executing-plans', 'subagent-driven-development']) {
    const template = readUtf8(getTemplatePath(skill));
    assert.equal(
      countOccurrences(template, '{{OPERATOR_ROUTE_AUTHORITY}}'),
      1,
      `${skill} should include the shared operator route authority resolver exactly once`,
    );
    assert.doesNotMatch(
      template,
      /### Reviewed-Closure (?:Command Matrix|Route Authority)/,
      `${skill} should not duplicate the reviewed-closure route law in template source`,
    );
  }

  for (const skill of listGeneratedSkills()) {
    const template = readUtf8(getTemplatePath(skill));
    assert.equal(
      countOccurrences(template, '{{OPERATOR_PUBLIC_COMMAND_AUTHORITY}}'),
      0,
      `${skill} should use the generated Installed Control Plane route law instead of a second public-command resolver`,
    );
    assert.doesNotMatch(
      template,
      /Workflow\/operator JSON route law:|Treat workflow\/operator JSON `phase`, `phase_detail`, `review_state_status`, `next_action`, `recommended_public_command_argv`/,
      `${skill} should not duplicate the generic operator public-command law in template source`,
    );
  }
});

test('high-use workflow skills use runtime/operator vocabulary for normal routing', () => {
  for (const skill of [
    'using-featureforge',
    'executing-plans',
    'subagent-driven-development',
    'requesting-code-review',
    'finishing-a-development-branch',
  ]) {
    assertNoNormalRuntimeHelperVocabulary(readUtf8(getTemplatePath(skill)), `${skill} template`);
    assertNoNormalRuntimeHelperVocabulary(readUtf8(getSkillPath(skill)), `${skill} generated skill`);
  }
});

test('generated workflow skills use explicit repo-safety result wording', () => {
  for (const skill of listGeneratedSkills()) {
    assertNoRepoSafetyHelperReturnVocabulary(readUtf8(getTemplatePath(skill)), `${skill} template`);
    assertNoRepoSafetyHelperReturnVocabulary(readUtf8(getSkillPath(skill)), `${skill} generated skill`);
  }
});

test('execution templates do not treat next_action as executable fallback authority', () => {
  for (const skill of ['executing-plans', 'subagent-driven-development']) {
    const template = readUtf8WithGeneratedRouteAuthority(getTemplatePath(skill));
    assert.doesNotMatch(
      template,
      /follow the reported `phase`, `phase_detail`, `next_action`, and `recommended_public_command_argv`/,
      `${skill} should not route by next_action when typed command surfaces are absent`,
    );
    assert.match(
      template,
      /If workflow\/operator JSON does not report `phase` `executing`, stop normal execution and follow the Installed Control Plane section plus the canonical route reference for the current operator result\./i,
      `${skill} should delegate non-executing operator handoff to the compact route law`,
    );
    assert.match(
      template,
      /Treat `phase`, `phase_detail`, and `next_action` as diagnostic context/i,
      `${skill} should keep next_action diagnostic-only outside typed argv/template execution`,
    );
    assertContainsOperatorPublicCommandAuthority(template, skill);
  }
});

test('execution prompt hidden-helper scanner rejects removed command vocabulary samples', () => {
  for (const sample of [
    'Run `record-contract` before implementation.',
    'The helper owns `record-evaluation`.',
    'Use `record-handoff` after dispatch.',
    'Invoke removed `note` for blockers.',
    'Invoke `note` for blockers.',
    'Invoke `note` to report blockers.',
    'Call `note` when blocked.',
    'The `note` command records blockers.',
    'The `note` helper owns interruptions.',
    'Run `featureforge plan execution note --plan docs/featureforge/plans/example.md`.',
    'Use the execution-note command for interruptions.',
    'Do not route through compatibility-only `workflow sync`.',
    'Use `workflow expect` to preserve the intended artifact.',
  ]) {
    assert.throws(
      () => assertNoRemovedHelperCommandNames(sample, 'synthetic active prompt sample'),
      /removed helper command names/,
    );
  }
  assert.doesNotThrow(() => {
    assertNoRemovedHelperCommandNames(
      'The migrated **Execution Note:** markdown line is projection input only.',
      'synthetic benign projection sample',
    );
  });
});

test('execution prompt retired workflow-status scanner rejects rooted command samples', () => {
  for (const sample of [
    'Run `featureforge workflow status --json` to get the route.',
    'Run `$_FEATUREFORGE_BIN workflow status --json` to get the route.',
    'Run `"$_FEATUREFORGE_BIN" workflow status --json` to get the route.',
    'Run `${_FEATUREFORGE_BIN} workflow status --json` to get the route.',
    'Run `"${_FEATUREFORGE_BIN}" workflow status --json` to get the route.',
    'Use `workflow status --json` for routing.',
  ]) {
    assert.match(
      sample,
      RETIRED_RUNTIME_COMMAND_TRAP_PATTERN,
      `retired workflow-status trap scanner should reject ${sample}`,
    );
  }
  assert.doesNotMatch(
    'Run `$_FEATUREFORGE_BIN workflow operator --plan "$APPROVED_PLAN_PATH" --json` for routing.',
    RETIRED_RUNTIME_COMMAND_TRAP_PATTERN,
    'retired workflow-status trap scanner should allow the workflow operator route authority',
  );
});

test('execution prompt candidate-authority scanner rejects positive direct-mutation samples', () => {
  for (const sample of [
    'Candidate artifacts are authoritative runtime mutation state; do not forget the appendix.',
    'Candidate edits are authoritative runtime mutation state, while unrelated sections say must not.',
    'Task packets authorize direct runtime state mutation by implementer helpers.',
    'Handoff notes authorize direct runtime state mutation by subagents.',
    'Implementer helpers may directly mutate runtime execution state.',
    'Subagents may directly mutate runtime execution state.',
  ]) {
    assert.notDeepEqual(
      candidateAuthorityBoundaryViolations(sample),
      [],
      `synthetic candidate-authority sample should be rejected: ${sample}`,
    );
  }
});

test('late-stage skill descriptions reject generic skip-ahead trigger phrases', () => {
  const lateStageSkills = [
    'plan-ceo-review',
    'writing-plans',
    'plan-eng-review',
    'executing-plans',
    'subagent-driven-development',
    'requesting-code-review',
    'finishing-a-development-branch',
  ];
  const forbiddenPatterns = [
    /implement this/i,
    /start coding/i,
    /build this/i,
    /plan this feature/i,
    /implementing major features/i,
  ];

  for (const skill of lateStageSkills) {
    const description = getSkillDescription(skill);
    for (const pattern of forbiddenPatterns) {
      assert.doesNotMatch(description, pattern, `${skill} description should not match ${pattern}`);
    }
  }
});

test('execution workflow skills reference the public runtime execution contract', () => {
  const planEngReview = readUtf8(getSkillPath('plan-eng-review'));
  assert.doesNotMatch(planEngReview, /\$_FEATUREFORGE_BIN plan execution recommend --plan <approved-plan-path>/);
  assert.match(planEngReview, /Present the runtime-selected execution owner skill as the default path with the approved plan path\./);
  assert.match(planEngReview, /If isolated-agent workflows are unavailable, do not present `featureforge:subagent-driven-development` as an available override\./);
  assertContainsOperatorPublicCommandAuthority(planEngReview, 'plan-eng-review');
  assertLaterPhaseUsesInstalledRouteLaw(planEngReview, 'plan-eng-review');
  assert.doesNotMatch(planEngReview, /review_blocked/);

  const writingPlans = readUtf8(getSkillPath('writing-plans'));
  assert.match(writingPlans, /\*\*Plan Revision:\*\* 1/);
  assert.match(writingPlans, /\*\*Execution Mode:\*\* none/);

  for (const skill of ['subagent-driven-development', 'executing-plans']) {
    const content = readUtf8(getSkillPath(skill));
    assert.match(content, /calls `\$_FEATUREFORGE_BIN workflow operator --plan \.\.\. --json` during preflight/);
    assert.match(
      content,
      /Run `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` before (?:starting execution|dispatching implementation subagents)\./,
    );
    assert.doesNotMatch(
      content,
      /Run `\$_FEATUREFORGE_BIN workflow preflight --plan <approved-plan-path>` before (?:starting execution|dispatching implementation subagents)\./,
    );
    assert.match(
      content,
      /uses `status --plan \.\.\.` only for additional diagnostics when operator output alone is insufficient/,
    );
    assert.match(content, /Provides the approved plan and the execution preflight handoff/);
    assert.match(content, /calls `begin` before starting work on a plan step/);
    assert.match(content, /calls `complete` after each completed step/);
    assert.match(content, /reports interruptions or blockers in the handoff\/status surface instead of invoking command-shaped note or repair side channels/);
  }
  const executingPlans = readUtf8(getSkillPath('executing-plans'));
  assertSkillCarriesProgressProjectionLaw(executingPlans, 'executing-plans');
  assert.doesNotMatch(
    executingPlans,
    /The approved plan checklist is the execution progress record; do not create or maintain a separate authoritative task tracker\./,
  );
  const subagentDrivenDevelopment = readUtf8(getSkillPath('subagent-driven-development'));
  assertSkillCarriesProgressProjectionLaw(
    subagentDrivenDevelopment,
    'subagent-driven-development',
  );
  assert.doesNotMatch(
    subagentDrivenDevelopment,
    /The approved plan checklist is the execution progress record; do not create or maintain a separate authoritative task tracker\./,
  );
  assert.doesNotMatch(
    executingPlans,
    /use the approved plan checklist as the execution progress record\./i,
  );
  assert.doesNotMatch(
    executingPlans,
    /use the approved plan checklist as the visible progress record for the task's steps\./i,
  );
  assert.doesNotMatch(
    subagentDrivenDevelopment,
    /\[use the approved plan as the execution-progress record\]/i,
  );
  assert.doesNotMatch(executingPlans, /track the work in your platform's task checklist/);
  assert.doesNotMatch(subagentDrivenDevelopment, /task-tracker checklist/);
  assert.doesNotMatch(subagentDrivenDevelopment, /Mark task complete in task tracker/);

  const reviewSkill = readUtf8(getSkillPath('requesting-code-review'));
  assert.match(reviewSkill, /rejects final review if the plan has invalid execution state or required unfinished work not truthfully represented/);
  assert.match(reviewSkill, /must fail closed when it detects a missed reopen or stale evidence, but must not call `reopen` itself/);
  assert.match(
    reviewSkill,
    /low-level compatibility\/debug dispatch commands are not normal intent-level progression\./,
  );
  assertContainsFragments(reviewSkill, 'requesting-code-review plan route context', [
    'plan-routed final review',
    'exact approved plan path',
    'exact approved spec path',
    'current execution preflight handoff or session context',
  ]);
  assert.match(reviewSkill, /Run `\$_FEATUREFORGE_BIN plan contract analyze-plan --spec <approved-spec-path> --plan <approved-plan-path> --format json` before dispatching the reviewer\./);
  assert.match(reviewSkill, /Run `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` before dispatching the reviewer\./);
  assertContainsFragments(reviewSkill, 'requesting-code-review fail-closed route handling', [
    'workflow/operator fails',
    'stop and return to the current execution flow',
    'do not guess the public late-stage route from raw execution state',
  ]);
  assertContainsFragments(reviewSkill, 'requesting-code-review diagnostic status scope', [
    'plan execution status --plan <approved-plan-path>',
    'only when you need extra execution-dirty or strategy-checkpoint diagnostics',
    'diagnostic status fails',
    'do not dispatch review against guessed plan state',
  ]);
  assertContainsFragments(reviewSkill, 'requesting-code-review active runtime blockers', [
    '`active_task`',
    '`blocking_task`',
    '`resume_task`',
    'final review is only valid when all three are `null`',
  ]);
  assert.match(reviewSkill, /treat workflow\/operator JSON as authoritative for the public late-stage route; status is diagnostic only\./);
  assert.match(reviewSkill, /only request a fresh external final review when workflow\/operator JSON reports `phase=final_review_pending` with `phase_detail=final_review_dispatch_required`\./);
  assertContainsOperatorPublicCommandAuthority(reviewSkill, 'requesting-code-review');
  assert.match(
    reviewSkill,
    /After the independent reviewer returns a final-review result[\s\S]{0,180}--external-review-result-ready --json[\s\S]{0,180}Installed Control Plane section[\s\S]{0,120}canonical route reference\./,
    'requesting-code-review should route final-review recording through operator JSON plus the canonical binding reference',
  );
  assertContainsFragments(reviewSkill, 'requesting-code-review reviewer context', [
    'Pass the exact approved plan path into the reviewer context',
    'runtime-owned execution evidence or task-packet context',
    'do not make the public flow harvest it manually',
  ]);
  assertContainsFragments(reviewSkill, 'requesting-code-review base branch authority', [
    'Do not use PR metadata or repo default-branch APIs as a fallback',
    'require `BASE_BRANCH` from `$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`',
    'require an explicitly provided `BASE_BRANCH`',
  ]);
  assert.match(reviewSkill, /Keep review artifacts runtime-owned:/);
  assert.doesNotMatch(reviewSkill, /project-scoped code-review companion artifact/);
  assert.doesNotMatch(reviewSkill, /\{user\}-\{safe-branch\}-code-review-\{datetime\}\.md/);
  assert.match(reviewSkill, /dedicated fresh-context reviewer independent of the implementation context/);
  assert.doesNotMatch(reviewSkill, /\*\*Review Stage:\*\* featureforge:requesting-code-review/);
  assert.doesNotMatch(reviewSkill, /\*\*Reviewer Artifact Path:\*\*/);
  assert.doesNotMatch(reviewSkill, /\*\*Generated By:\*\* featureforge:requesting-code-review/);
  assert.doesNotMatch(reviewSkill, /derived companion for reviewer provenance and audit traceability/);
  assert.doesNotMatch(reviewSkill, /git log --oneline \| grep "Task 1"/);
  assert.doesNotMatch(reviewSkill, /git rev-parse HEAD~1/);
  assert.match(reviewSkill, /Use `ANALYZE_JSON=\$\("\$_FEATUREFORGE_BIN" plan contract analyze-plan --spec "\$SOURCE_SPEC_PATH" --plan "\$APPROVED_PLAN_PATH" --format json\)`/);
  assert.match(reviewSkill, /stop unless `contract_state=valid` and `packet_buildable_tasks=task_count`/);
  assert.match(reviewSkill, /When diagnostic status is required, if any of `active_task`, `blocking_task`, or `resume_task` is non-null, stop and return to the current execution flow; final review is only valid when all three are `null`\./);
  assert.match(reviewSkill, /Use `OPERATOR_JSON=\$\("\$_FEATUREFORGE_BIN" workflow operator --plan "\$APPROVED_PLAN_PATH" --json\)`/);
  assert.match(reviewSkill, /request final review only for `phase=final_review_pending` plus `phase_detail=final_review_dispatch_required`/);
  assert.doesNotMatch(reviewSkill, /REVIEW_DISPATCH_JSON=/);
  assert.doesNotMatch(reviewSkill, /REVIEW_DISPATCH_ACTION=/);
  assert.doesNotMatch(reviewSkill, /DISPATCH_ID=/);
  assert.doesNotMatch(reviewSkill, /REVIEW_DISPATCH_ALLOWED=/);
  assert.doesNotMatch(reviewSkill, /REVIEW_GATE_JSON/);
  assert.doesNotMatch(reviewSkill, /review gate rejected the current execution evidence/);
  assert.match(reviewSkill, /RECORDING_READY_JSON=\$\("\$_FEATUREFORGE_BIN" workflow operator --plan "\$APPROVED_PLAN_PATH" --external-review-result-ready --json\)/);
  assert.doesNotMatch(reviewSkill, /if \[ "\$RECORDING_PHASE_DETAIL" != "final_review_recording_ready" \] && \[ "\$RECORDING_PHASE_DETAIL" != "final_review_dispatch_required" \]; then/);
  assert.match(
    reviewSkill,
    /final-review materializer lives in the `Final-Review Recording Route Materializer` section of `\$_FEATUREFORGE_ROOT\/references\/operator-route-authority\.md`/,
  );
  assert.doesNotMatch(reviewSkill, /node > "\$ROUTE_ARGV_FILE"/);
  const routeReference = readUtf8(path.join(REPO_ROOT, 'references/operator-route-authority.md'));
  assertFinalReviewRouteMaterializerContract(routeReference, 'references/operator-route-authority.md');
  assert.match(
    routeReference,
    /workflow operator[\s\S]{0,240}--external-review-result-ready/,
    'canonical route reference should request Rust-owned final-review result-ready materialization',
  );
  assert.match(
    routeReference,
    /recommended_public_command_argv[\s\S]*_featureforge_exec_public_argv/,
    'canonical route reference should execute the materialized typed route argv',
  );
  assert.match(
    routeReference,
    /--input "reviewer_source=\$REVIEWER_SOURCE"[\s\S]{0,180}--input "reviewer_id=\$REVIEWER_ID"[\s\S]{0,180}--input "result=\$REVIEW_RESULT"[\s\S]{0,180}--input "summary_file=\$SUMMARY_FILE"/,
    'canonical route reference should pass final-review bindings to workflow operator instead of prompt-side template code',
  );
  assert.doesNotMatch(routeReference, /ensureFinalReviewTemplate|node > "\$ROUTE_ARGV_FILE"/);
  assert.doesNotMatch(reviewSkill, /execute_argv/);
  assert.doesNotMatch(reviewSkill, /bind_template/);
  assert.doesNotMatch(reviewSkill, /"\$_FEATUREFORGE_BIN" plan execution advance-late-stage --plan "\$APPROVED_PLAN_PATH" --reviewer-source fresh-context-subagent --reviewer-id <actual-reviewer-id> --result "\$REVIEW_RESULT" --summary-file "\$SUMMARY_FILE"/);
  assert.doesNotMatch(reviewSkill, /--result pass --summary-file review-summary\.md/);
  assert.doesNotMatch(reviewSkill, /STATUS_JSON=/);
  assert.doesNotMatch(reviewSkill, /TASK_PACKET_CONTEXT_TASK_1=/);

  const finishSkill = readUtf8(getSkillPath('finishing-a-development-branch'));
  assert.match(finishSkill, /rejects branch-completion handoff if the approved plan is execution-dirty or malformed/);
  assert.match(finishSkill, /must not allow branch completion while any checked-off plan step still lacks semantic implementation evidence/);
  assert.match(finishSkill, /If the current work was executed from an approved FeatureForge plan, require the exact approved plan path from the current execution workflow context before presenting completion options\./);
  assert.match(finishSkill, /Run `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` and require a branch-completion-ready route before presenting completion options\./);
  assert.match(finishSkill, /If the exact approved plan path is unavailable or workflow\/operator fails, stop and return to the current execution flow instead of guessing\./);
  assert.match(finishSkill, /Use `\$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` only when you need additional diagnostics \(`active_task`, `blocking_task`, `resume_task`, `evidence_path`, checkpoint fingerprints\) to explain a blocker\./);
  assert.match(
    finishSkill,
    /Do not run a fixed terminal sequence from memory\./,
  );
  assert.match(
    finishSkill,
    /workflow\/operator selects that handoff lane or returns the selected public argv\/template route for it\./,
  );
  assert.match(
    finishSkill,
    /release-facing docs or metadata[\s\S]{0,180}context for the operator-selected release-doc lane/i,
  );
  assert.doesNotMatch(
    finishSkill,
    /Route through `featureforge:document-release` first/,
  );
  assert.match(
    finishSkill,
    /this checkpoint does not replace any later operator-selected final-review lane\./,
  );
  assert.doesNotMatch(finishSkill, /keep the order strict:/);
  assert.doesNotMatch(finishSkill, /after `featureforge:document-release` and before any runtime-routed `featureforge:qa-only` handoff/);
  assert.match(
    finishSkill,
    /If approved-plan `QA Requirement` is missing or invalid[\s\S]{0,220}Installed Control Plane section[\s\S]{0,160}canonical route reference[\s\S]{0,160}do not guess/i,
  );
  assert.match(finishSkill, /If the current work is governed by an approved FeatureForge plan, treat the approved plan's normalized `\*\*QA Requirement:\*\* required\|not-required` metadata as authoritative for workflow-routed finish gating\./);
  assert.doesNotMatch(
    finishSkill,
    /QA Requirement[^\n]*\$_FEATUREFORGE_BIN plan execution repair-review-state/,
  );
  assert.match(finishSkill, /Treat the current-branch test-plan artifact as a QA scope\/provenance input only when its `Source Plan`, `Source Plan Revision`, and `Head SHA` match the exact approved plan path, revision, and current branch HEAD from the workflow context\./);
  assert.match(finishSkill, /Match current-branch artifacts by their `\*\*Branch:\*\*` header, not by a filename substring glob, so `my-feature` cannot masquerade as `feature`\./);
  assert.doesNotMatch(finishSkill, /\*-"?\$BRANCH"?-test-plan-\*/);
  assert.match(finishSkill, /For plan-routed completion, use the exact `base_branch` from `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` instead of redetecting the target branch\./);
  assert.match(finishSkill, /The Step 2 `<base-branch>` value stays authoritative for Options A, B, and D\./);
  assert.match(finishSkill, /Use the exact `<base-branch>` resolved in Step 2\. Do not redetect it during PR creation\./);
  assert.doesNotMatch(
    finishSkill,
    /If a fresh release-readiness artifact is already present, its `\*\*Base Branch:\*\*` header must match that runtime-owned `base_branch`; if it is missing or blank, stop and return to `featureforge:document-release`\./,
  );
  assert.match(
    finishSkill,
    /If the current work is governed by an approved FeatureForge plan and workflow\/operator does not route to branch completion, stop and return to the current execution flow; do not present completion options against stale QA or release artifacts\./,
  );
  assert.match(
    finishSkill,
    /If workflow\/operator JSON reports `qa_pending` with `phase_detail=test_plan_refresh_required`, perform only that handoff: return to `featureforge:plan-eng-review` to regenerate the current-branch test-plan artifact before QA or branch completion\./,
  );
  assert.match(
    finishSkill,
    /missing or stale source test-plan projections are diagnostic-only when workflow\/operator routes QA or branch completion from current runtime-owned state\./,
  );
  assert.doesNotMatch(
    finishSkill,
    /no current-branch test-plan artifact exists[\s\S]{0,120}stop and regenerate it before invoking `featureforge:qa-only`, QA outcome recording, or finish-gate commands/,
  );
  assert.match(finishSkill, /gh pr create --base "<base-branch>"/);

  const reviewPrompt = readUtf8(path.join(REPO_ROOT, 'skills/requesting-code-review/code-reviewer.md'));
  assert.match(reviewPrompt, /^# Code Review Briefing Template/m);
  assert.match(reviewPrompt, /This file is the skill-local reviewer briefing template, not the generated agent system prompt\./);
  assert.match(reviewPrompt, /\*\*Approved plan path:\*\* \{APPROVED_PLAN_PATH\}/);
  assert.match(reviewPrompt, /\*\*Execution evidence path:\*\* \{EXECUTION_EVIDENCE_PATH\}/);
  assert.match(reviewPrompt, /dedicated independent reviewer for the terminal whole-diff gate/);
  assert.match(reviewPrompt, /Structured Review Result Metadata/);
  assert.match(reviewPrompt, /review-result metadata for the controller to bind to runtime-owned state/);
  assert.match(reviewPrompt, /Do not create, repair, search for, or reference runtime-owned projection files/);
  assert.doesNotMatch(reviewPrompt, /Dedicated Reviewer Receipt Contract/);
  assert.doesNotMatch(reviewPrompt, /receipt-ready metadata/);
  assert.match(reviewPrompt, /`Source Plan`, `Source Plan Revision`, `Strategy Checkpoint Fingerprint`, `Branch`, `Repo`, `Base Branch`, `Head SHA`/);
  assert.match(reviewPrompt, /When approved plan and execution evidence paths are provided, read both artifacts and verify that checked-off plan steps are semantically satisfied by the implementation and explicitly evidenced\./);
  assert.match(reviewPrompt, /When execution evidence documents recorded topology downgrades or other execution deviations, explicitly inspect them and state whether those deviations pass final review\./);
  assert.match(reviewPrompt, /Use caller-provided base-branch context and release-lineage routing/);
  assert.match(reviewPrompt, /instead of deriving it locally or running workflow commands/);
  assert.doesNotMatch(reviewPrompt, /git symbolic-ref --short refs\/remotes\/origin\/HEAD/);
  assert.doesNotMatch(reviewPrompt, /for candidate in main master/);
  assert.doesNotMatch(reviewPrompt, /BASE_BRANCH_EFFECTIVE=/);
  assert.doesNotMatch(reviewPrompt, /gh pr view --json baseRefName/);

  const subagentReviewPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/code-quality-reviewer-prompt.md'));
  assert.match(subagentReviewPrompt, /TASK_PACKET: \[runtime-provided task packet\]/);
  assert.match(subagentReviewPrompt, /APPROVED_PLAN_PATH: \[exact approved plan path for plan-routed final review, otherwise blank\]/);
  assert.match(subagentReviewPrompt, /EXECUTION_EVIDENCE_PATH: \[runtime-owned execution evidence path for plan-routed final review, otherwise blank\]/);
  assert.match(subagentReviewPrompt, /BASE_BRANCH: \[runtime-provided base branch for plan-routed review, otherwise explicitly provided base branch\]/);
});

test('task-fidelity workflow docs and prompts require packet-backed plan contracts', () => {
  const writingPlans = readUtf8(getSkillPath('writing-plans'));
  assert.match(writingPlans, /Requirement Coverage Matrix/);
  assert.match(writingPlans, /## Execution Strategy/);
  assert.match(writingPlans, /## Dependency Diagram/);
  assert.match(writingPlans, /\*\*QA Requirement:\*\* required \| not-required/);
  assert.match(writingPlans, /\*\*Spec Coverage:\*\*/);
  assert.match(writingPlans, /\*\*Goal:\*\*/);
  assert.match(writingPlans, /\*\*Context:\*\*/);
  assert.match(writingPlans, /\*\*Constraints:\*\*/);
  assert.match(writingPlans, /\*\*Done when:\*\*/);
  assert.doesNotMatch(writingPlans, /\*\*Task Outcome:\*\*/);
  assert.doesNotMatch(writingPlans, /\*\*Plan Constraints:\*\*/);
  assert.doesNotMatch(writingPlans, /\*\*Open Questions:\*\*/);
  assert.match(writingPlans, /`QA Requirement` is a plan-level finish-gating decision/);
  assert.match(writingPlans, /task-level `Done when` bullets must not be used to infer whether QA is required/);
  assert.match(writingPlans, /self-contained enough for a fresh implementer and fresh reviewer/);
    assert.match(
      writingPlans,
      /only when one of the trigger conditions in `\$_FEATUREFORGE_ROOT\/review\/plan-task-contract\.md` applies/,
    );
  assert.match(writingPlans, /Extend the existing task-contract parser; do not add a second parser path\./);
  assert.match(writingPlans, /atomic, binary, objectively reviewable, reviewable without interpretation drift/);
  assert.match(writingPlans, /Do not bundle unrelated outcomes into one task when that would force reviewers to judge partial completion\./);
  assert.match(writingPlans, /Step checklists after `Files` are optional execution aids, not required task-contract surface\./);
  assert.match(writingPlans, /Legacy task fields such as `Task Outcome`, `Plan Constraints`, or task-level `Open Questions`/);
  assert.match(writingPlans, /Vague `Done when` bullets such as "the UX feels right" or "the implementation is robust"/);
  assert.match(writingPlans, /A task that mixes two architectural goals/);
  assert.match(writingPlans, /review\/plan-task-contract\.md/);
  assert.match(writingPlans, /"\$_FEATUREFORGE_BIN" plan contract lint/);
  assert.match(writingPlans, /create .* worktrees? and run Tasks .* in parallel/i);
  assert.match(writingPlans, /Task \d+ owns /);
  assert.match(writingPlans, /Execute Task \d+ serially/i);

  const planEngReview = readUtf8(getSkillPath('plan-eng-review'));
  assert.match(planEngReview, /"\$_FEATUREFORGE_BIN" plan contract analyze-plan/);
  assert.match(planEngReview, /contract_state == valid/);
  assert.match(planEngReview, /packet_buildable_tasks == task_count/);
  assert.match(planEngReview, /execution_strategy_present/);
  assert.match(planEngReview, /dependency_diagram_present/);
  assert.match(planEngReview, /execution_topology_valid/);
  assert.match(planEngReview, /serial_hazards_resolved/);
  assert.match(planEngReview, /parallel_lane_ownership_valid/);
  assert.match(planEngReview, /parallel_workspace_isolation_valid/);
  assert.match(planEngReview, /task_contract_valid/);
  assert.match(planEngReview, /task_goal_valid/);
  assert.match(planEngReview, /task_context_sufficient/);
  assert.match(planEngReview, /task_constraints_valid/);
  assert.match(planEngReview, /task_done_when_deterministic/);
  assert.match(planEngReview, /tasks_self_contained/);
  assert.match(planEngReview, /missing, stale, or non-buildable for the approved plan revision/);
  assert.match(planEngReview, /Requirement Index/);
  assert.match(planEngReview, /Requirement Coverage Matrix/);
  assert.match(planEngReview, /Execution Strategy/);
  assert.match(planEngReview, /Dependency Diagram/);
  assert.match(planEngReview, /missing task `Goal`, `Context`, `Constraints`, `Done when`, `Spec Coverage`, or `Files`/);
  assert.doesNotMatch(planEngReview, /until the runtime analyzer exposes dedicated task-contract booleans/);
  assert.match(planEngReview, /Do not use legacy task-level `Open Questions` review as the primary approval model after cutover/);
  assert.match(planEngReview, /avoidable duplicate implementation of substantive production behavior/);
  assert.match(planEngReview, /fails to name the shared implementation home when reuse is required/);
  assert.match(planEngReview, /invalid `Files:` block structure/);
  assert.match(planEngReview, /fake-parallel hotspot files/i);
  assert.match(planEngReview, /exact isolated workspace truth/i);
  assert.match(planEngReview, /Does the `Requirement Coverage Matrix` cover every approved requirement without orphaned or over-broad tasks\?/);
  assert.match(planEngReview, /Do `Files:` blocks stay within the minimum file scope needed for the covered requirements, or do they signal file-scope drift that should be split or reapproved\?/);

  const acceleratedEngPrompt = readUtf8(path.join(REPO_ROOT, 'skills/plan-eng-review/accelerated-reviewer-prompt.md'));
  assert.match(acceleratedEngPrompt, /preserving the normal engineering hard-fail law/);
  assert.match(acceleratedEngPrompt, /task_done_when_deterministic/);
  assert.match(acceleratedEngPrompt, /rejecting weak task contracts, non-deterministic `Done when`, missing required spec references/);
  assert.match(acceleratedEngPrompt, /naming the existing shared implementation home when reuse is required/);

  const acceleratorPacketContract = readUtf8(path.join(REPO_ROOT, 'review/review-accelerator-packet-contract.md'));
  assert.match(acceleratorPacketContract, /## ENG hard-fail fields/);
  assert.match(acceleratorPacketContract, /analyze-plan boolean snapshot for `task_contract_valid`, `task_goal_valid`, `task_context_sufficient`, `task_constraints_valid`, `task_done_when_deterministic`, and `tasks_self_contained`/);
  assert.match(acceleratorPacketContract, /deterministic `Done when` assessment/);
  assert.match(acceleratorPacketContract, /reuse assessment that names the existing shared implementation home/);

  const planFidelityReview = readUtf8(getSkillPath('plan-fidelity-review'));
  assert.match(planFidelityReview, /task-contract fidelity/);
  assert.match(planFidelityReview, /review\/plan-task-contract\.md/);
  assert.match(planFidelityReview, /plan_fidelity_review\.required_artifact_template/);
  assert.match(planFidelityReview, /template `content` verbatim/);
  assert.match(planFidelityReview, /Do not invent, rename, reorder, omit, or hand-type parseable artifact headers/);
  assert.match(planFidelityReview, /review artifact must record exactly these `Verified Surfaces`/);
  assert.match(planFidelityReview, /task_contract/);
  assert.match(planFidelityReview, /task_determinism/);
  assert.match(planFidelityReview, /spec_reference_fidelity/);

  const planFidelityPrompt = readUtf8(path.join(REPO_ROOT, 'skills/plan-fidelity-review/reviewer-prompt.md'));
  assert.match(planFidelityPrompt, /plan_fidelity_review\.required_artifact_template/);
  assert.match(planFidelityPrompt, /use the supplied `content` verbatim/);
  assert.match(planFidelityPrompt, /Do not\s+invent, rename, reorder, omit, or hand-type parseable headers/);
  assert.match(planFidelityPrompt, /verify every task against the approved task contract in `\$_FEATUREFORGE_ROOT\/review\/plan-task-contract\.md`/);
  assert.match(planFidelityPrompt, /\*\*Review Verdict:\*\* pass \| fail/);
  assert.doesNotMatch(planFidelityPrompt, /pass \| needs-changes/);
  assert.match(planFidelityPrompt, /\*\*Verified Surfaces:\*\* requirement_index, execution_topology, task_contract, task_determinism, spec_reference_fidelity/);
  assert.match(planFidelityPrompt, /TASK_MISSING_GOAL/);
  assert.match(planFidelityPrompt, /TASK_DONE_WHEN_NON_DETERMINISTIC/);
  assert.match(planFidelityPrompt, /TASK_SPEC_REFERENCE_REQUIRED/);

  const executingPlans = readUtf8(getSkillPath('executing-plans'));
  assert.match(executingPlans, /build the canonical task packet/);
  assert.match(executingPlans, /treat it as the exact task contract for that execution segment/);
  assert.match(executingPlans, /mandatory task-boundary closure loop/i);
  assert.match(
    executingPlans,
    /if workflow\/operator still reports a review-dispatch or recording route after an external review result is ready[\s\S]{0,180}canonical route reference[\s\S]{0,180}low-level dispatch-lineage management/i,
    'executing-plans should keep post-review rerouting actionable without duplicating detailed route tokens',
  );
  assert.match(
    executingPlans,
    /Load the approved plan, query workflow\/operator, and execute only the returned typed public argv\/template route for each runtime-selected step\./,
    'executing-plans should open with operator-selected typed route execution instead of a fixed late-stage sequence',
  );
  assert.match(
    executingPlans,
    /After all tasks complete and verified:[\s\S]{0,260}Query `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`[\s\S]{0,260}shared route law[\s\S]{0,160}Installed Control Plane section[\s\S]{0,160}canonical route reference/i,
    'executing-plans should route late-stage progression through operator JSON and the compact route law',
  );
  assert.match(
    executingPlans,
    /After any selected final review route is resolved:[\s\S]{0,220}Rerun `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`[\s\S]{0,220}Invoke `featureforge:finishing-a-development-branch` only when workflow\/operator selects branch completion/i,
    'executing-plans should not hard-code finishing after final review',
  );
  assert.match(
    executingPlans,
    /Use route-specific late-stage skills only when workflow\/operator selects that lane; companion references provide examples and binding detail, not route-selection authority\./,
    'executing-plans should keep workflow/operator as the only late-stage route selector',
  );
  assert.doesNotMatch(
    executingPlans,
    /operator route or the companion reference selects that lane/,
    'executing-plans companion references must not become route-selection authority',
  );
  assert.doesNotMatch(
    executingPlans,
    /After all tasks complete and verified:[\s\S]{0,180}Run `featureforge:document-release` first/,
    'executing-plans should not prescribe a remembered document-release/final-review sequence',
  );
  assert.doesNotMatch(
    executingPlans,
    /After the final review is resolved:[\s\S]{0,180}featureforge:finishing-a-development-branch/,
    'executing-plans should not prescribe finishing after final review without an operator route',
  );
  assert.doesNotMatch(
    executingPlans,
    /REQUIRED SUB-SKILL:\*\* Use featureforge:finishing-a-development-branch/,
    'executing-plans should not make finishing a mandatory un-routed sub-skill',
  );
  assertTaskBoundaryClosureLoopSemantics(executingPlans, 'skills/executing-plans/SKILL.md');
  assert.match(
    executingPlans,
    /dedicated-independent review loops plus verification are required inputs to `close-current-task`; they are not separate begin-time authority once Task `N` has a current positive closure/,
  );
  assert.match(executingPlans, /does not require per-dispatch user-consent prompts/);
  assert.match(executingPlans, /Non-execution ad-hoc delegation still follows normal user-consent policy/);

  const subagentSkill = readUtf8(getSkillPath('subagent-driven-development'));
  assert.match(subagentSkill, /pass the packet verbatim to implementer and reviewers/);
  assert.match(subagentSkill, /If the packet does not answer it, the task is ambiguous and execution must stop or route back to review\./);
  assert.match(subagentSkill, /The coordinator owns every `git commit`, `git merge`, and `git push` for this workflow/);
  assertContainsFragments(subagentSkill, 'subagent-driven-development task-boundary route law', [
    'close-current-task',
    'review-dispatch or recording route',
    'canonical route reference',
    'low-level dispatch-lineage management',
    'selected typed route',
    'selected handoff skill',
  ]);
  assert.match(
    subagentSkill,
    /if workflow\/operator still reports a review-dispatch or recording route after an external review result is ready[\s\S]{0,180}canonical route reference[\s\S]{0,180}low-level dispatch-lineage management/i,
    'subagent-driven-development should keep post-review rerouting actionable without duplicating detailed route tokens',
  );
  assertTaskBoundaryClosureLoopSemantics(
    subagentSkill,
    'skills/subagent-driven-development/SKILL.md',
  );
  assertContainsOperatorPublicCommandAuthority(subagentSkill, 'subagent-driven-development');
  assert.match(subagentSkill, /run `verification-before-completion` and collect the verification result inputs needed by `close-current-task`/i);
  assert.match(subagentSkill, /does not require per-dispatch user-consent prompts/);
  assert.match(subagentSkill, /Non-execution ad-hoc delegation still follows normal user-consent policy/);
  assert.doesNotMatch(subagentSkill, /controller provides full text/);
  assert.doesNotMatch(subagentSkill, /provide full text instead/);
  assert.doesNotMatch(subagentSkill, /Skip scene-setting context/);

  for (const [content, label] of [
    [executingPlans, 'skills/executing-plans/SKILL.md'],
    [subagentSkill, 'skills/subagent-driven-development/SKILL.md'],
  ]) {
    const normalized = normalizeWhitespace(content);
    assert.match(
      content,
      /Reviewed-Closure Route Authority/,
      `${label} should include compact reviewed-closure route guidance`,
    );
    assertRouteAuthoritySectionIsCompact(content, label);
    assertContainsOperatorPublicCommandAuthority(content, label);
    assert.match(
      content,
      /dedicated-independent review loops (?:plus|and) verification are required inputs to `close-current-task`/,
      `${label} should describe the aggregate task-closure input contract`,
    );
    assertHighUseExecutionSkillDoesNotInlineDetailedClosureRouteTokens(content, label);
    assert.match(
      content,
      /\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --external-review-result-ready --json[\s\S]{0,220}Installed Control Plane section[\s\S]{0,120}canonical route reference/i,
      `${label} should require workflow operator readiness before task-closure recording inputs`,
    );
    assert.doesNotMatch(
      content,
      /external review or verification result/i,
      `${label} should reserve --external-review-result-ready for actual external review results`,
    );
    assertNoLateStageLiteralCommandShapes(content, label);
    assert.doesNotMatch(
      content,
      /\$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path> \.\.\./,
      `${label} should not use a generic advance-late-stage placeholder`,
    );
    assert.doesNotMatch(
      content,
      /Compatibility-only escape hatch: use low-level runtime primitives only when explicitly debugging or preserving compatibility/,
      `${label} must not preserve low-level primitive escape-hatch guidance`,
    );
    assert.match(
      normalized,
      /Do not [^.]*manually edit runtime-owned records, derived markdown projections, or `\*\*Execution Note:\*\*` lines to recover routing state\./i,
      `${label} should explicitly forbid manual edits to runtime-owned records and derived markdown projection artifacts`,
    );
    assert.match(
      content,
      /docs\/featureforge\/reference\/2026-04-01-review-state-reference\.md/,
      `${label} should link to the shared review-state reference`,
    );
    assert.doesNotMatch(
      normalized,
      /\| Compatibility-only (?:fallback|diagnostics):/i,
      `${label} should avoid enumerating compatibility command tables in active normal-path guidance`,
    );
    assert.match(
      content,
      /`review_remediation`: required after actionable independent-review findings and before remediation starts\. Runtime records it automatically when reviewable runtime review state enters remediation and when remediation reopens execution work\./,
      `${label} should bind review_remediation to runtime-managed review state`,
    );
    assert.doesNotMatch(
      content,
      /`gate-review` dispatch/,
      `${label} should not describe review_remediation as a gate-review dispatch checkpoint`,
    );
    assert.doesNotMatch(
      normalized,
      /\| [^|]+ \| [^|]+ \| [^|]*\$_FEATUREFORGE_BIN plan execution explain-review-state --plan <approved-plan-path>[^|]* \| [^|]+ \|/i,
      `${label} should not promote explain-review-state into the primary command column`,
    );
    assert.doesNotMatch(
      normalized,
      /\| [^|]+ \| [^|]+ \| [^|]*\$_FEATUREFORGE_BIN plan execution reconcile-review-state --plan <approved-plan-path>[^|]* \| [^|]+ \|/i,
      `${label} should not promote reconcile-review-state into the primary command column`,
    );
    assert.doesNotMatch(
      normalized,
      /\| [^|]+ \| [^|]+ \| [^|]*record-release-readiness[^|]* \| [^|]+ \|/i,
      `${label} should not promote record-release-readiness into the primary command column`,
    );
    assert.doesNotMatch(
      normalized,
      /\| [^|]+ \| [^|]+ \| [^|]*record-final-review[^|]* \| [^|]+ \|/i,
      `${label} should not promote record-final-review into the primary command column`,
    );
    assert.match(
      normalized,
      /no (?:code|test) edits?[\s\S]*(?:successful preflight|execution preflight handoff)[\s\S]*first `begin`/i,
      `${label} should prohibit code/test edits between the execution preflight handoff and first begin`,
    );
    assert.match(
      normalized,
      /workspace[\s\S]*dirty[\s\S]*first `begin`[\s\S]*tracked_worktree_dirty/i,
      `${label} should warn that dirty-before-begin can trigger tracked_worktree_dirty fail-closed checks`,
    );
    assert.match(
      normalized,
      /retroactive (?:execution )?tracking[\s\S]*recovery-only/i,
      `${label} should keep retroactive tracking as recovery-only`,
    );
    assert.doesNotMatch(
      normalized,
      /five-step recovery runbook/i,
      `${label} should delegate the old inline five-step recovery runbook to the canonical route reference`,
    );
    assert.doesNotMatch(
      content,
      /helper-backed route|authoritative helper mutations/i,
      `${label} should not describe dirty-before-begin recovery as helper-backed mutation`,
    );
  }

  for (const [templatePath, label] of [
    ['skills/executing-plans/SKILL.md.tmpl', 'skills/executing-plans/SKILL.md.tmpl'],
    [
      'skills/subagent-driven-development/SKILL.md.tmpl',
      'skills/subagent-driven-development/SKILL.md.tmpl',
    ],
  ]) {
    assertHighUseExecutionSkillDoesNotInlineDetailedClosureRouteTokens(
      readUtf8(path.join(REPO_ROOT, templatePath)),
      label,
    );
  }

  const routeAuthority = readUtf8(path.join(REPO_ROOT, 'references/operator-route-authority.md'));
  assert.match(
    routeAuthority,
    /execution-start tracking must be recovered[\s\S]*follow only the typed public argv\/template from that operator route before any recovery mutation[\s\S]*If no public argv\/template is present, stop and report the route diagnostic/i,
    'canonical route reference should route dirty-before-begin recovery through typed public argv or a stop/report diagnostic',
  );
  assert.match(
    routeAuthority,
    /backfill only factual-only completed steps through public runtime routes returned by workflow\/operator; never infer completion from dirty diffs[\s\S]*task-boundary review and verification gate/i,
    'canonical route reference should keep factual-only backfill on public runtime routes before task-boundary review',
  );

  const implementerPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/implementer-prompt.md'));
  assert.match(implementerPrompt, /## Task Packet/);
  assert.match(implementerPrompt, /the packet is the authoritative task contract for that execution slice/);
  assert.match(implementerPrompt, /do not reinterpret or weaken requirement statements/);
  assert.match(implementerPrompt, /Treat the packet's `DONE_WHEN_N` obligations as the authoritative completion list/);
  assert.match(implementerPrompt, /Objectively reviewable `Done when` bullets remain mandatory/);
  assert.match(implementerPrompt, /If the packet's `Goal`, `Context`, `Constraints`, or indexed `Done when`/);
  assert.match(implementerPrompt, /Prepare the change for coordinator-owned git actions; do not create commits, merges, or pushes yourself/);
  assert.doesNotMatch(implementerPrompt, /Commit your work/);

  const specReviewerPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/spec-reviewer-prompt.md'));
  assert.match(specReviewerPrompt, /the exact task packet/);
  assert.match(specReviewerPrompt, /Grade every packet `DONE_WHEN_N` obligation as `pass` or `fail`/);
  assert.match(specReviewerPrompt, /Grade every packet `CONSTRAINT_N` obligation as `pass` or `fail`/);
  assert.match(specReviewerPrompt, /Every issue must include a stable finding ID and the exact violated obligation ID/);
  assert.match(specReviewerPrompt, /DONE_WHEN_1: pass\/fail/);
  assert.match(specReviewerPrompt, /CONSTRAINT_1: pass\/fail/);
  assert.match(specReviewerPrompt, /PLAN_DEVIATION_FOUND/);
  assert.match(specReviewerPrompt, /AMBIGUITY_ESCALATION_REQUIRED/);

  const codeQualityPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/code-quality-reviewer-prompt.md'));
  assert.match(codeQualityPrompt, /TASK_PACKET/);
  assert.match(codeQualityPrompt, /work outside planned file decomposition/);
  assert.match(codeQualityPrompt, /new files or abstractions outside packet scope/);
  assert.match(codeQualityPrompt, /Did the change reuse the planned shared implementation named by the task packet/);
  assert.match(codeQualityPrompt, /Treat avoidable duplicate implementation as a hard failure/);
  assert.match(codeQualityPrompt, /violated packet obligation ID, such as `CONSTRAINT_2` or `DONE_WHEN_1`/);
  assert.match(codeQualityPrompt, /Return a reuse assessment matrix with pass\/fail rows/);
  assert.match(codeQualityPrompt, /PACKET_REUSE_SCOPE/);
  assert.match(codeQualityPrompt, /Reuse Assessment Matrix/);

  assert.match(executingPlans, /indexed `CONSTRAINT_N` obligations/);
  assert.match(executingPlans, /indexed `DONE_WHEN_N` obligations/);
  assert.match(executingPlans, /Separate-session handoffs must paste the generated task packet verbatim/);
});

test('active task fixtures no longer use legacy approved-task field headers', () => {
  const legacyFields = ['Task Outcome', 'Plan Constraints', 'Open Questions'];
  const legacyMarkers = legacyFields.map((field) => `**${field}:**`);
  const [taskOutcomeMarker, planConstraintsMarker, openQuestionsMarker] = legacyMarkers;
  const searchableExtensions = new Set([
    '.md',
    '.md.tmpl',
    '.mjs',
    '.rs',
    '.toml',
    '.json',
    '.txt',
    '.sh',
  ]);
  const allowedLegacyHeaderLines = new Map([
    [
      'src/contracts/task_contract.rs',
      new Set([
        `    trimmed.starts_with("${taskOutcomeMarker}")`,
        `        || trimmed.starts_with("${planConstraintsMarker}")`,
        `        || trimmed.starts_with("${openQuestionsMarker}")`,
      ]),
    ],
    [
      'tests/contracts_spec_plan.rs',
      new Set([
        `    assert!(!markdown.contains("${openQuestionsMarker}"));`,
        `        "**Goal:** The plan contract is represented as canonical traceability blocks that preserve exact approved wording.\\n${planConstraintsMarker} legacy scalar constraints must be quarantined.",`,
      ]),
    ],
    [
      'tests/codex-runtime/fixtures/plan-contract/final-cutover-regression.json',
      new Set([
        `      "invalid_examples": ["${planConstraintsMarker} legacy scalar constraints must be quarantined."]`,
      ]),
    ],
    [
      'tests/codex-runtime/skill-doc-contracts.test.mjs',
      new Set([
        `        || trimmed.starts_with("${planConstraintsMarker}")`,
        `        \`        "**Goal:** The plan contract is represented as canonical traceability blocks that preserve exact approved wording.\\\\n\${planConstraintsMarker} legacy scalar constraints must be quarantined.",\`,`,
        `        \`      "invalid_examples": ["\${planConstraintsMarker} legacy scalar constraints must be quarantined."]\`,`,
        `    '${planConstraintsMarker} legacy scalar constraints must be quarantined.',`,
      ]),
    ],
    [
      'tests/runtime_instruction_contracts.rs',
      new Set([`        "${openQuestionsMarker}",`]),
    ],
  ]);
  const transitionOnlyReadme = readUtf8(
    path.join(REPO_ROOT, 'tests/codex-runtime/fixtures/plan-contract/transition-only/README.md'),
  );
  const transitionOnlyLegacyFixture = readUtf8(
    path.join(
      REPO_ROOT,
      'tests/codex-runtime/fixtures/plan-contract/transition-only/invalid-open-questions-plan.md',
    ),
  );

  assert.match(transitionOnlyReadme, /transition-only negative fixtures/);
  assert.match(transitionOnlyReadme, /not active approved-plan examples/);
  assert.doesNotMatch(transitionOnlyLegacyFixture, /\*\*Task Outcome:\*\*/);
  assert.doesNotMatch(transitionOnlyLegacyFixture, /\*\*Plan Constraints:\*\*/);
  assert.match(transitionOnlyLegacyFixture, /\*\*Open Questions:\*\*/);

  const offenders = [];
  for (const relPath of listRepoFiles()) {
    const extension = path.extname(relPath);
    if (!searchableExtensions.has(extension) && !relPath.endsWith('.md.tmpl')) {
      continue;
    }
    const content = readUtf8(path.join(REPO_ROOT, relPath));
    const allowedLines = allowedLegacyHeaderLines.get(relPath) ?? new Set();
    const lines = content.split('\n');
    for (const [index, line] of lines.entries()) {
      for (const marker of legacyMarkers) {
        if (line.includes(marker) && !allowedLines.has(line)) {
          offenders.push(`${relPath}:${index + 1}: ${marker}`);
        }
      }
    }
  }

  assert.deepEqual(offenders, []);
});

test('reuse hard-fail law is critical, scoped, and example-backed across reviewer surfaces', () => {
  const checklist = readUtf8(path.join(REPO_ROOT, 'review/checklist.md'));
  const contract = readUtf8(path.join(REPO_ROOT, 'review/plan-task-contract.md'));
  const planEngReview = readUtf8(getSkillPath('plan-eng-review'));
  const finalReviewBriefing = readUtf8(path.join(REPO_ROOT, 'skills/requesting-code-review/code-reviewer.md'));
  const reviewerAgentSource = readUtf8(path.join(REPO_ROOT, 'agents/code-reviewer.instructions.md'));
  const generatedReviewerAgent = readUtf8(path.join(REPO_ROOT, 'agents/code-reviewer.md'));
  const generatedCodexAgent = readUtf8(path.join(REPO_ROOT, '.codex/agents/code-reviewer.toml'));

  const criticalSection = checklist.slice(
    checklist.indexOf('### Pass 1 — Critical'),
    checklist.indexOf('### Pass 2 — Important or Minor'),
  );
  assert.match(criticalSection, /Treat avoidable duplicate implementation of substantive production behavior as a hard fail/);
  assert.match(criticalSection, /same semantic rule, normalization, freshness decision, routing rule, or artifact-binding rule is implemented in multiple places/);
  assert.match(criticalSection, /new local helper partially re-expresses behavior already available from an existing shared helper/);
  assert.match(criticalSection, /test-only, CLI-only, or adapter-only logic drifts from the production helper path/);
  assert.match(criticalSection, /generated code, fixtures or test data, tiny test-only setup repetition/);
  assert.match(criticalSection, /Hard fail: a diff adds a second repo-relative path normalizer/);
  assert.match(criticalSection, /Allowed exception: generated schema output repeats field names/);

  assert.match(contract, /## Reuse Hard-Fail Law/);
  assert.match(contract, /Reviewer examples:/);
  assert.match(contract, /Hard fail: a task adds a second parser, normalizer, validator, router/);
  assert.match(contract, /Allowed exception: generated code, fixtures, tiny test-only setup/);
  assert.match(contract, /The reviewer states the exact exception category/);

  assert.match(planEngReview, /The reuse gate is a hard approval gate, not advisory design feedback/);
  assert.match(planEngReview, /must either extend the named shared implementation home or name one approved exception category/);
  assert.match(planEngReview, /Generated code, fixtures or test data, tiny test-only setup repetition/);
  assert.match(planEngReview, /If an exception is claimed, does it name one approved exception category/);

  for (const content of [finalReviewBriefing, reviewerAgentSource, generatedReviewerAgent, generatedCodexAgent]) {
    assert.match(content, /block landing unless the diff names one approved exception category|Treat avoidable duplicate implementation of substantive production behavior as a hard fail/);
    assert.match(content, /duplicated behavior, the shared implementation home, why duplication is harmful|shared implementation home, why duplication is harmful/);
    assert.match(content, /parsers, normalizers, validators, routing logic, eligibility logic/);
    assert.match(content, /Example hard fail: a diff adds a second repo-relative path normalizer/);
    assert.match(content, /Example allowed exception: generated schema output repeats field names/);
  }
});

test('generated reviewer agent surfaces carry prompt-scoped recursion contract', () => {
  const reviewerSurfaces = [
    ['reviewer agent instructions', path.join(REPO_ROOT, 'agents/code-reviewer.instructions.md')],
    ['generated reviewer markdown', path.join(REPO_ROOT, 'agents/code-reviewer.md')],
    ['generated codex reviewer TOML', path.join(REPO_ROOT, '.codex/agents/code-reviewer.toml')],
  ];

  for (const [label, file] of reviewerSurfaces) {
    const content = readUtf8(file);
    assertReviewerSurfaceCarriesPromptScopedRecursionRule(content, label);
    assert.match(content, /Root Discovery For Standalone Use/, `${label} should define standalone root discovery`);
    assert.match(content, /git rev-parse --show-toplevel/, `${label} should resolve the repo root before using $_REPO_ROOT references`);
    assert.match(content, /repo runtime-root --path/, `${label} should resolve the FeatureForge root before using $_FEATUREFORGE_ROOT references`);
    assert.match(content, /Do not use root discovery to run workflow mutations or reconstruct missing workflow context\./, `${label} should keep root discovery read-only for review context`);
  }

  const requestingCodeReview = readUtf8(getSkillPath('requesting-code-review'));
  assert.match(requestingCodeReview, /The reviewer prompt owns the reviewer-only recursion contract\./);
  assert.doesNotMatch(requestingCodeReview, /FEATUREFORGE_REVIEWER_RUNTIME_COMMANDS_ALLOWED/);
  assert.doesNotMatch(requestingCodeReview, /## Review-subagent recursion rule/);
  assert.doesNotMatch(requestingCodeReview, /Do not launch, request, or delegate to additional subagents/);

  const generatedCodexAgent = readUtf8(path.join(REPO_ROOT, '.codex/agents/code-reviewer.toml'));
  assert.doesNotMatch(generatedCodexAgent, /^# REVIEWER_RUNTIME_ENV_CONTRACT$/m);
  assert.doesNotMatch(generatedCodexAgent, /^# Launcher must set FEATUREFORGE_REVIEWER_RUNTIME_COMMANDS_ALLOWED/m);
});

test('subagent reviewer prompts carry prompt-scoped recursion rule', () => {
  assertCanonicalReviewerRecursionRuleIsStrong();
  assertRuntimeSourcesDoNotEnforceReviewerRecursionGuards();

  const reviewerPrompts = [
    [
      'requesting-code-review final reviewer prompt',
      path.join(REPO_ROOT, 'skills/requesting-code-review/code-reviewer.md'),
    ],
    [
      'plan fidelity reviewer prompt',
      path.join(REPO_ROOT, 'skills/plan-fidelity-review/reviewer-prompt.md'),
    ],
    [
      'accelerated ENG reviewer prompt',
      path.join(REPO_ROOT, 'skills/plan-eng-review/accelerated-reviewer-prompt.md'),
    ],
    [
      'accelerated CEO reviewer prompt',
      path.join(REPO_ROOT, 'skills/plan-ceo-review/accelerated-reviewer-prompt.md'),
    ],
    [
      'outside voice ENG reviewer prompt',
      path.join(REPO_ROOT, 'skills/plan-eng-review/outside-voice-prompt.md'),
    ],
    [
      'outside voice CEO reviewer prompt',
      path.join(REPO_ROOT, 'skills/plan-ceo-review/outside-voice-prompt.md'),
    ],
    [
      'spec reviewer prompt',
      path.join(REPO_ROOT, 'skills/subagent-driven-development/spec-reviewer-prompt.md'),
    ],
    [
      'code quality reviewer prompt',
      path.join(REPO_ROOT, 'skills/subagent-driven-development/code-quality-reviewer-prompt.md'),
    ],
  ];

  for (const [label, file] of reviewerPrompts) {
    assertReviewerSurfaceCarriesPromptScopedRecursionRule(readUtf8(file), label);
  }

  const specReviewerPrompt = readUtf8(
    path.join(REPO_ROOT, 'skills/subagent-driven-development/spec-reviewer-prompt.md'),
  );
  assertSpecReviewerPromptKeepsRecursionRulePayloadOnly(specReviewerPrompt);
});

test('review prompts use deterministic repair-packet findings tied to obligations', () => {
  const contract = readUtf8(path.join(REPO_ROOT, 'review/plan-task-contract.md'));
  const planFidelityPrompt = readUtf8(path.join(REPO_ROOT, 'skills/plan-fidelity-review/reviewer-prompt.md'));
  const planEngReview = readUtf8(getSkillPath('plan-eng-review'));
  const acceleratedEngPrompt = readUtf8(path.join(REPO_ROOT, 'skills/plan-eng-review/accelerated-reviewer-prompt.md'));
  const acceleratorPacketContract = readUtf8(path.join(REPO_ROOT, 'review/review-accelerator-packet-contract.md'));
  const specReviewerPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/spec-reviewer-prompt.md'));
  const codeQualityPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/code-quality-reviewer-prompt.md'));
  const finalReviewBriefing = readUtf8(path.join(REPO_ROOT, 'skills/requesting-code-review/code-reviewer.md'));
  const planEngOutsideVoice = readUtf8(path.join(REPO_ROOT, 'skills/plan-eng-review/outside-voice-prompt.md'));
  const subagentDrivenDevelopment = readUtf8(getSkillPath('subagent-driven-development'));
  const requestingCodeReview = readUtf8(getSkillPath('requesting-code-review'));
  const executionReviewQaExamples = readUtf8(path.join(REPO_ROOT, 'references/execution-review-qa-examples.md'));
  const reviewerAgentSource = readUtf8(path.join(REPO_ROOT, 'agents/code-reviewer.instructions.md'));
  const generatedReviewerAgent = readUtf8(path.join(REPO_ROOT, 'agents/code-reviewer.md'));
  const generatedCodexAgent = readUtf8(path.join(REPO_ROOT, '.codex/agents/code-reviewer.toml'));
  const acceleratorEval = readUtf8(path.join(REPO_ROOT, 'tests/evals/review-accelerator-contract.eval.mjs'));

  assert.match(contract, /## Deterministic Review Finding Shape/);
  assert.match(contract, /\*\*Finding ID:\*\* <stable-finding-id>/);
  assert.match(contract, /\*\*Violated Field or Obligation:\*\*/);
  assert.match(contract, /\*\*Required Fix:\*\*/);
  assert.match(contract, /\*\*Hard Fail:\*\* yes \| no/);
  assert.match(contract, /`Required Fix` is the repair packet/);
  assert.match(contract, /DONE_WHEN_N/);
  assert.match(contract, /CONSTRAINT_N/);
  assert.match(contract, /PLAN_DEVIATION_FOUND/);
  assert.match(contract, /AMBIGUITY_ESCALATION_REQUIRED/);
  assert.match(contract, /PACKET_REUSE_SCOPE/);
  assert.match(contract, /Prompt-local obligation names are invalid/);
  assert.match(contract, /Example failed finding/);

  for (const content of [
    planFidelityPrompt,
    planEngReview,
    acceleratedEngPrompt,
    acceleratorPacketContract,
    specReviewerPrompt,
    codeQualityPrompt,
    finalReviewBriefing,
    planEngOutsideVoice,
    subagentDrivenDevelopment,
    requestingCodeReview,
    reviewerAgentSource,
    generatedReviewerAgent,
    generatedCodexAgent,
  ]) {
    assert.match(content, /Finding ID/);
    assert.match(content, /Severity/);
    assert.match(content, /Task/);
    assert.match(content, /Violated Field or Obligation/);
    assert.match(content, /Evidence/);
    assert.match(content, /Required Fix/);
    assert.match(content, /Hard Fail/);
  }

  assert.match(planFidelityPrompt, /do not replace field-specific findings with broad advice/);
  assert.match(planEngReview, /Do not use general feedback when a failed task field, analyzer boolean, packet-assigned obligation, or checklist law can be named/);
  assert.match(acceleratedEngPrompt, /obligation-tied, delta-oriented repair findings instead of general advice/);
  assert.match(acceleratorPacketContract, /Do not use general advice when the packet can name the violated field/);
  assert.match(specReviewerPrompt, /deterministic repair-packet findings only; no essay-style reinterpretation/);
  assert.match(codeQualityPrompt, /deterministic repair-packet findings/);
  assert.match(finalReviewBriefing, /Keep `Required Fix` as the smallest acceptable repair delta/);
  assert.match(planEngOutsideVoice, /Findings: none/);
  assert.match(planEngOutsideVoice, /Tensions:` only for non-blocking strategic tension notes/);
  assert.match(executionReviewQaExamples, /### Finding TASK2_DONE_WHEN_2_PROGRESS_REPORTING/);
  assert.match(executionReviewQaExamples, /### Finding TASK2_SCOPE_EXTRA_JSON_FLAG/);
  assert.match(executionReviewQaExamples, /\*\*Violated Field or Obligation:\*\* DONE_WHEN_2/);
  assert.match(executionReviewQaExamples, /\*\*Violated Field or Obligation:\*\* PLAN_DEVIATION_FOUND/);
  assert.doesNotMatch(executionReviewQaExamples, /\*\*Violated Field or Obligation:\*\* PLAN_DEVIATION_FOUND\n\*\*Evidence:\*\* The task packet requires progress indicators/);
  assert.doesNotMatch(executionReviewQaExamples, /\*\*Violated Field or Obligation:\*\* PACKET_REUSE_SCOPE\n\*\*Evidence:\*\* The task packet did not approve a new JSON output mode/);
  assert.match(executionReviewQaExamples, /\*\*Required Fix:\*\* Add progress reporting at the packet-required interval\./);
  assert.match(executionReviewQaExamples, /\*\*Required Fix:\*\* Remove the unrequested `--json` flag from the Task 2 diff or route the scope expansion back through plan approval\./);
  assert.doesNotMatch(executionReviewQaExamples, /Add progress reporting at the packet-required interval and remove the unrequested `--json` flag/);
  assert.match(executionReviewQaExamples, /### Finding TASK2_PROGRESS_INTERVAL_CONSTANT/);
  assert.doesNotMatch(executionReviewQaExamples, /Missing: Progress reporting/);
  assert.doesNotMatch(executionReviewQaExamples, /Issues \(Important\): Magic number/);
  assert.match(executionReviewQaExamples, /### Finding FINAL_REVIEW_PROGRESS_INDICATORS/);
  assert.match(executionReviewQaExamples, /\*\*Task:\*\* Task 4/);
  assert.doesNotMatch(executionReviewQaExamples, /\*\*Task:\*\* Whole diff/);
  assert.doesNotMatch(executionReviewQaExamples, /Important: Missing progress indicators/);
  assert.doesNotMatch(executionReviewQaExamples, /Minor: Magic number \(100\)/);
  assert.match(executionReviewQaExamples, /Repeat until no tasks remain -> workflow\/operator JSON selects the next release, final-review, QA, or finish lane/);
  assert.match(executionReviewQaExamples, /If workflow\/operator JSON routes QA, run qa-only, then follow references\/operator-route-authority\.md for the recording route/);
  assert.match(executionReviewQaExamples, /When workflow\/operator JSON reports branch completion ready -> finishing-a-development-branch/);
  assert.match(executionReviewQaExamples, /Local merge example:/);
  assert.match(executionReviewQaExamples, /git merge --no-ff <feature-branch>/);
  assert.match(executionReviewQaExamples, /cargo nextest run --all-targets --all-features --no-fail-fast/);
  assert.match(executionReviewQaExamples, /Discard example after typed confirmation:/);
  assert.match(executionReviewQaExamples, /git branch -D <feature-branch>/);
  assert.doesNotMatch(executionReviewQaExamples, /git push origin --delete <feature-branch>/);
  assert.match(executionReviewQaExamples, /Only delete a remote branch when the typed confirmation explicitly names remote branch deletion\./);
  assert.match(executionReviewQaExamples, /Keep-as-is means leave the branch and worktree untouched/);
  assert.match(executionReviewQaExamples, /PR-created branches also keep their worktree unless the user explicitly chooses cleanup after the PR exists\./);
  assert.match(executionReviewQaExamples, /Worktree cleanup example after the branch is merged, explicitly discarded, or the user explicitly chooses cleanup:/);
  assert.doesNotMatch(executionReviewQaExamples, /Worktree cleanup example after the branch is merged, PR-created, kept, or explicitly discarded:/);
  assert.match(executionReviewQaExamples, /git worktree remove <worktree-path>/);
  assert.match(executionReviewQaExamples, /require typed confirmation before deleting commits or worktrees/);
  assert.match(reviewerAgentSource, /Keep `Required Fix` delta-oriented so the next repair step can be executed without reinterpretation/);
  assert.match(generatedReviewerAgent, /Keep `Required Fix` delta-oriented so the next repair step can be executed without reinterpretation/);
  assert.match(generatedCodexAgent, /Keep `Required Fix` delta-oriented so the next repair step can be executed without reinterpretation/);
  assert.match(acceleratorEval, /deterministic repair-packet fields tied to the violated task field/);
});

test('final cutover fixtures pin active contract, review law, and happy path', () => {
  const regression = JSON.parse(
    readUtf8(path.join(REPO_ROOT, 'tests/codex-runtime/fixtures/plan-contract/final-cutover-regression.json')),
  );
  const happyPath = JSON.parse(
    readUtf8(path.join(REPO_ROOT, 'tests/codex-runtime/fixtures/plan-contract/final-cutover-happy-path.json')),
  );
  const planFidelityPrompt = readUtf8(path.join(REPO_ROOT, 'skills/plan-fidelity-review/reviewer-prompt.md'));
  const planEngReview = readUtf8(getSkillPath('plan-eng-review'));
  const executingPlans = readUtf8(getSkillPath('executing-plans'));
  const subagentDrivenDevelopment = readUtf8(getSkillPath('subagent-driven-development'));
  const codeQualityPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/code-quality-reviewer-prompt.md'));
  const finalReviewBriefing = readUtf8(path.join(REPO_ROOT, 'skills/requesting-code-review/code-reviewer.md'));
  const validPlan = readUtf8(path.join(REPO_ROOT, 'tests/codex-runtime/fixtures/plan-contract/valid-plan.md'));

  assert.equal(regression.fixture_kind, 'final_cutover_regression');
  assert.deepEqual(
    regression.scenarios.map((scenario) => scenario.id),
    [
      'draft_plan_missing_context_fails_plan_fidelity_review',
      'draft_plan_scalar_legacy_plan_constraints_fails_runtime_parser',
      'draft_plan_vague_done_when_fails_engineering_review',
      'draft_plan_multi_sentence_goal_fails_runtime_parser',
      'draft_plan_wrong_task_field_order_fails_runtime_parser',
      'valid_task_packet_reaches_implementation',
      'duplicate_abstraction_fails_code_quality_and_final_review',
    ],
  );

  const missingContext = regression.scenarios[0];
  assert.equal(missingContext.expected_finding_id, 'TASK_MISSING_CONTEXT');
  assert.equal(missingContext.runtime_boolean, 'task_context_sufficient');
  assert.equal(missingContext.runtime_reason_code, 'task_missing_context');
  assert.match(planFidelityPrompt, /TASK_MISSING_CONTEXT/);
  assert.match(planFidelityPrompt, /Verified Surfaces.*task_contract.*task_determinism.*spec_reference_fidelity/s);

  const scalarLegacyPlanConstraints = regression.scenarios[1];
  assert.equal(scalarLegacyPlanConstraints.surface, 'runtime-analyzer');
  assert.equal(scalarLegacyPlanConstraints.runtime_reason_code, 'legacy_task_field');
  assert.deepEqual(scalarLegacyPlanConstraints.invalid_examples, [
    '**Plan Constraints:** legacy scalar constraints must be quarantined.',
  ]);

  const vagueDoneWhen = regression.scenarios[2];
  assert.equal(vagueDoneWhen.expected_analyzer_boolean, 'task_done_when_deterministic');
  assert.equal(vagueDoneWhen.runtime_reason_code, 'task_nondeterministic_done_when');
  assert.deepEqual(vagueDoneWhen.invalid_examples, [
    'The implementation is robust.',
    'The implementation works.',
    'The implementation works as expected.',
    'The changes are ready for review.',
  ]);
  assert.match(planEngReview, /task_done_when_deterministic/);
  assert.match(planEngReview, /keep the plan in `Draft`/);
  assert.match(planEngReview, /non-deterministic, non-atomic, or under-specified `Done when`/);

  const multiSentenceGoal = regression.scenarios[3];
  assert.equal(multiSentenceGoal.surface, 'runtime-analyzer');
  assert.equal(multiSentenceGoal.expected_analyzer_boolean, 'task_goal_valid');
  assert.equal(multiSentenceGoal.runtime_reason_code, 'task_goal_not_atomic');
  assert.deepEqual(multiSentenceGoal.invalid_examples, [
    'The plan contract is represented. It preserves approved wording.',
  ]);

  const wrongFieldOrder = regression.scenarios[4];
  assert.equal(wrongFieldOrder.surface, 'runtime-analyzer');
  assert.deepEqual(wrongFieldOrder.required_field_order, happyPath.required_task_fields);
  assert.equal(wrongFieldOrder.runtime_reason_code, 'task_field_order_invalid');

  const packetScenario = regression.scenarios[5];
  assert.equal(packetScenario.expected_packet_contract_version, 'task-obligation-v2');
  assert.deepEqual(packetScenario.expected_packet_obligations, ['CONSTRAINT_1', 'DONE_WHEN_1']);
  assert.match(executingPlans, /build the canonical task packet/);
  assert.match(subagentDrivenDevelopment, /Task packets must preserve the approved task contract/);
  assert.match(subagentDrivenDevelopment, /indexed `CONSTRAINT_N` obligations/);
  assert.match(subagentDrivenDevelopment, /indexed `DONE_WHEN_N` obligations/);

  const duplicateScenario = regression.scenarios[6];
  assert.equal(duplicateScenario.expected_obligation, 'PACKET_REUSE_SCOPE');
  assert.equal(duplicateScenario.requires_shared_home, true);
  assert.equal(duplicateScenario.negative_review_fixture, 'duplicate-abstraction-hard-fail-review.json');
  const duplicateReviewFixture = JSON.parse(
    readUtf8(path.join(REPO_ROOT, 'tests/codex-runtime/fixtures/plan-contract', duplicateScenario.negative_review_fixture)),
  );
  assert.equal(duplicateReviewFixture.fixture_kind, 'duplicate_abstraction_hard_fail_review');
  assert.equal(duplicateReviewFixture.task_packet.reuse_scope_obligation, 'PACKET_REUSE_SCOPE');
  assert.deepEqual(duplicateReviewFixture.task_packet.file_scope, [
    'src/contracts/plan.rs',
    'src/contracts/runtime.rs',
  ]);
  assert.deepEqual(
    duplicateReviewFixture.implementation_diff_facts.map((fact) => fact.path),
    ['src/contracts/plan.rs', 'src/contracts/runtime.rs'],
  );
  assert.deepEqual(
    duplicateReviewFixture.implementation_diff_facts.map((fact) => `${fact.evidence_path}:${fact.line}`),
    [
      'tests/codex-runtime/fixtures/plan-contract/duplicate-abstraction-bad-diff.patch:5',
      'tests/codex-runtime/fixtures/plan-contract/duplicate-abstraction-bad-diff.patch:22',
    ],
  );
  assert.deepEqual(duplicateReviewFixture.expected_review_finding, {
    finding_id: 'TASK2_DUPLICATE_TASK_INTENT_GROUPING',
    severity: 'critical',
    task: 'Task 2',
    violated_field_or_obligation: 'PACKET_REUSE_SCOPE',
    evidence: 'tests/codex-runtime/fixtures/plan-contract/duplicate-abstraction-bad-diff.patch:5 and tests/codex-runtime/fixtures/plan-contract/duplicate-abstraction-bad-diff.patch:22 duplicate task-intent grouping for typed plan analysis and runtime analysis.',
    required_fix: 'Move task-intent duplicate grouping into the shared task-contract layer and call that shared implementation from both typed plan parsing and runtime analysis.',
    hard_fail: true,
    shared_implementation_home: 'src/contracts/task_contract.rs',
    why_duplication_is_harmful: 'The typed parser and runtime analyzer can drift on which task goals are classified as duplicate or overlapping task intent.',
  });
  assert.match(duplicateReviewFixture.review_output, /### Finding TASK2_DUPLICATE_TASK_INTENT_GROUPING/);
  assert.match(duplicateReviewFixture.review_output, /\*\*Violated Field or Obligation:\*\* PACKET_REUSE_SCOPE/);
  assert.match(duplicateReviewFixture.review_output, /\*\*Severity:\*\* critical/);
  assert.match(duplicateReviewFixture.review_output, /\*\*Hard Fail:\*\* yes/);
  assert.match(duplicateReviewFixture.review_output, /shared task-contract layer/);
  assert.match(codeQualityPrompt, /PACKET_REUSE_SCOPE/);
  assert.match(codeQualityPrompt, /Treat avoidable duplicate implementation as a hard failure/);
  assert.match(finalReviewBriefing, /block landing unless the diff names one approved exception category/);
  assert.match(finalReviewBriefing, /shared implementation home, why duplication is harmful/);

  assert.equal(happyPath.fixture_kind, 'final_cutover_happy_path');
  assert.equal(happyPath.plan_fixture, 'valid-plan.md');
  assert.equal(happyPath.spec_fixture, 'valid-spec.md');
  assert.deepEqual(happyPath.expected_runtime, {
    contract_state: 'valid',
    task_count: 3,
    packet_buildable_tasks: 3,
    task_contract_valid: true,
    task_goal_valid: true,
    task_context_sufficient: true,
    task_constraints_valid: true,
    task_done_when_deterministic: true,
    tasks_self_contained: true,
  });
  assert.deepEqual(happyPath.required_task_fields, [
    'Spec Coverage',
    'Goal',
    'Context',
    'Constraints',
    'Done when',
    'Files',
  ]);
  assert.deepEqual(happyPath.active_surfaces, [
    'authoring',
    'runtime-analyzer',
    'task-packet',
    'plan-fidelity-review',
    'plan-eng-review',
    'task-review',
    'final-review',
  ]);
  for (const field of happyPath.required_task_fields) {
    assert.match(validPlan, new RegExp(`\\*\\*${field}:\\*\\*`));
  }
});

test('repo-writing workflow skills document the protected-branch repo-safety gate consistently', () => {
  const expectedTargets = {
    brainstorming: /spec-artifact-write/,
    'project-memory': /repo-file-write/,
    'plan-ceo-review': /approval-header-write/,
    'writing-plans': /plan-artifact-write/,
    'plan-eng-review': /plan-artifact-write/,
    'executing-plans': /execution-task-slice/,
    'subagent-driven-development': /execution-task-slice/,
    'document-release': /release-doc-write/,
    'finishing-a-development-branch': /branch-finish/,
  };

  for (const [skill, targetPattern] of Object.entries(expectedTargets)) {
    const content = readUtf8(getSkillPath(skill));
    assert.match(content, /Protected-Branch Repo-Write Gate/, `${skill} should document the protected-branch gate`);
    assert.match(content, /\$_FEATUREFORGE_BIN repo-safety check --intent write/, `${skill} should run the repo-safety check`);
    assert.match(content, /\$_FEATUREFORGE_BIN repo-safety approve --stage/, `${skill} should document the approval rescue flow`);
    assert.match(content, /featureforge:using-git-worktrees/, `${skill} should route blocked writes to using-git-worktrees`);
    assert.match(content, /branch, the stage, and the blocking `failure_class`/, `${skill} should surface blocked-write diagnostics`);
    assert.match(content, targetPattern, `${skill} should use the correct write target family`);
  }

  const planEngReview = readUtf8(getSkillPath('plan-eng-review'));
  assert.match(planEngReview, /plan-artifact-write/, 'plan-eng-review should gate plan-body writes');
  assert.match(planEngReview, /approval-header-write/, 'plan-eng-review should gate approval-header writes separately');
  assert.doesNotMatch(planEngReview, /repo-file-write/, 'plan-eng-review should not regress to repo-file-write');

  const executingPlans = readUtf8(getSkillPath('executing-plans'));
  assert.match(executingPlans, /--write-target execution-task-slice \[--write-target git-commit\] \[--write-target git-merge\] \[--write-target git-push\]/);

  const finishingBranch = readUtf8(getSkillPath('finishing-a-development-branch'));
  assert.match(finishingBranch, /--write-target branch-finish \[--write-target git-merge\] \[--write-target git-push\] \[--write-target git-worktree-cleanup\]/);
});

test('project-memory workflow hooks stay consult-only and non-gating', () => {
  const writingPlans = readUtf8(getSkillPath('writing-plans'));
  assert.match(writingPlans, /## Optional Project Memory Consult/);
  assert.match(writingPlans, /consult `docs\/project_notes\/decisions\.md`/);
  assert.match(writingPlans, /consult `docs\/project_notes\/key_facts\.md`/);
  assert.match(
    writingPlans,
    /later `featureforge:project-memory` summary update to `docs\/project_notes\/decisions\.md` may be appropriate after approval\./,
  );
  assert.match(writingPlans, /supportive context only/i);
  assert.match(writingPlans, /Missing or stale notes do not block planning\./);
  assertForbidsGateLikeHookLanguage(
    writingPlans,
    'writing-plans',
    'the project-memory consult into a planning prerequisite or gate',
    'docs\\/project_notes\\/(?:decisions|key_facts)\\.md',
  );
  assertForbidsTimedObligationHook(
    writingPlans,
    'writing-plans',
    'the project-memory consult into a mandatory-before-planning hook',
    [
      'before planning',
      'before defining tasks',
      'before decomposing tasks',
      'during planning',
      'during task breakdown',
      'during decomposition',
      'while planning',
      'while decomposing tasks',
      'to plan',
      'to start planning',
      'to continue planning',
      'task breakdown',
      'planning start',
    ],
    'docs\\/project_notes\\/(?:decisions|key_facts)\\.md',
  );
  assertDetectsTimedHookSamples(
    [
      'Consult `docs/project_notes/decisions.md` before defining tasks.',
      'Consult `docs/project_notes/key_facts.md` during task breakdown.',
      'You should consult `docs/project_notes/decisions.md` before planning.',
      'Consult `docs/project_notes/decisions.md` during planning.',
      'featureforge:project-memory during planning needs to be used.',
      'Consult featureforge:project-memory during planning.',
      'Consult featureforge:project-memory before planning by reviewing `docs/project_notes/decisions.md`.',
    ],
    'writing-plans',
    'timed planning consult regressions',
    [
      'before planning',
      'before defining tasks',
      'before decomposing tasks',
      'during planning',
      'during task breakdown',
      'during decomposition',
      'while planning',
      'while decomposing tasks',
      'to plan',
      'to start planning',
      'to continue planning',
      'task breakdown',
      'planning start',
    ],
    'docs\\/project_notes\\/(?:decisions|key_facts)\\.md',
  );
  assertDetectsGateLikeHookSamples(
    [
      'featureforge:project-memory is a prerequisite for planning.',
      '`docs/project_notes/decisions.md` is required for planning.',
    ],
    'writing-plans',
    'planning gate regressions',
    'docs\\/project_notes\\/(?:decisions|key_facts)\\.md',
  );

  const systematicDebugging = readUtf8(getSkillPath('systematic-debugging'));
  assert.match(systematicDebugging, /Check Recurring Bug Memory When It Exists/);
  assert.match(systematicDebugging, /search `docs\/project_notes\/bugs\.md`/);
  assert.match(systematicDebugging, /update `docs\/project_notes\/bugs\.md`/);
  assert.match(systematicDebugging, /recurring or historically familiar/i);
  assert.match(systematicDebugging, /durable recurring bug pattern/i);
  assertForbidsGateLikeHookLanguage(
    systematicDebugging,
    'systematic-debugging',
    'the bug-memory hook into a debugging prerequisite or gate',
    'docs\\/project_notes\\/bugs\\.md',
  );
  assertForbidsTimedObligationHook(
    systematicDebugging,
    'systematic-debugging',
    'the bugs.md update into an always-after-fix requirement',
    [
      'after (?:every|each) fix',
      'after fixes',
      'after resolving the bug',
      'once the fix lands',
      'after the fix lands',
      'after debugging',
      'during debugging',
      'during the debugging work',
      'while debugging',
      'before fixing',
      'after the repair',
    ],
    'docs\\/project_notes\\/bugs\\.md',
  );
  assertDetectsTimedHookSamples(
    [
      'Update `docs/project_notes/bugs.md` after the fix lands.',
      'Update `docs/project_notes/bugs.md` after resolving the bug.',
      'You should update `docs/project_notes/bugs.md` after debugging.',
      'Update `docs/project_notes/bugs.md` during debugging.',
      'Update `docs/project_notes/bugs.md` while debugging.',
      'Search `docs/project_notes/bugs.md` during debugging.',
      'featureforge:project-memory during debugging should be used.',
      'Update featureforge:project-memory during debugging.',
      'Update featureforge:project-memory after the fix lands with the new `docs/project_notes/bugs.md` entry.',
    ],
    'systematic-debugging',
    'timed bug-memory update regressions',
    [
      'after (?:every|each) fix',
      'after fixes',
      'after resolving the bug',
      'once the fix lands',
      'after the fix lands',
      'after debugging',
      'during debugging',
      'during the debugging work',
      'while debugging',
      'before fixing',
      'after the repair',
    ],
    'docs\\/project_notes\\/bugs\\.md',
  );
  assertDetectsGateLikeHookSamples(
    [
      'featureforge:project-memory is required during debugging.',
      'Updating `docs/project_notes/bugs.md` blocks debugging progress.',
    ],
    'systematic-debugging',
    'debugging gate regressions',
    'docs\\/project_notes\\/bugs\\.md',
  );
  const recurringBugMemoryIndex = systematicDebugging.indexOf('5. **Check Recurring Bug Memory When It Exists**');
  const traceDataFlowIndex = systematicDebugging.indexOf('6. **Trace Data Flow**');
  assert.ok(
    recurringBugMemoryIndex !== -1 && traceDataFlowIndex !== -1 && recurringBugMemoryIndex < traceDataFlowIndex,
    'systematic-debugging should keep the recurring-bug memory step before Trace Data Flow as ordered steps 5 then 6',
  );

  const documentRelease = readUtf8(getSkillPath('document-release'));
  assert.match(documentRelease, /## Optional Project Memory Follow-Up/);
  assert.match(documentRelease, /release pass surfaces durable knowledge worth preserving/i);
  assert.match(documentRelease, /featureforge:project-memory/);
  assert.match(documentRelease, /docs\/project_notes\//);
  assert.match(documentRelease, /docs\/project_notes\/bugs\.md/);
  assert.match(documentRelease, /docs\/project_notes\/decisions\.md/);
  assert.match(documentRelease, /docs\/project_notes\/key_facts\.md/);
  assert.match(documentRelease, /docs\/project_notes\/issues\.md/);
  assert.match(documentRelease, /release pass surfaces durable knowledge worth preserving/i);
  assertForbidsGateLikeHookLanguage(
    documentRelease,
    'document-release',
    'the project-memory follow-up into a release prerequisite or blocker',
    'docs\\/project_notes\\/',
  );
  assertForbidsTimedObligationHook(
    documentRelease,
    'document-release',
    'the project-memory follow-up into a required release-pass gate',
    [
      'before branch completion',
      'before presenting completion options',
      'to complete the branch',
      'required document-release handoff',
      'finish the release pass',
      'complete the release pass',
      'release-readiness pass',
      'during the release-readiness pass',
      'during release-readiness',
    ],
    'docs\\/project_notes\\/',
  );
  assert.match(
    documentRelease,
    /`featureforge:document-release` does not replace checkpoint reviews and does not own review-dispatch minting\. Keep command-boundary semantics explicit: low-level compatibility\/debug commands stay out of the normal-path flow\./,
  );
  assertDetectsTimedHookSamples(
    [
      'Use featureforge:project-memory to update `docs/project_notes/issues.md` before branch completion.',
      'Use featureforge:project-memory to update `docs/project_notes/decisions.md` to finish the release pass.',
      'Use featureforge:project-memory before branch completion to update `docs/project_notes/issues.md`.',
      'Use featureforge:project-memory before branch completion.',
      'featureforge:project-memory before branch completion.',
      'featureforge:project-memory before branch completion should be used.',
      'featureforge:project-memory should update `docs/project_notes/issues.md` before branch completion.',
      'Record durable bugs in `docs/project_notes/bugs.md` before branch completion.',
      'Agents need to update `docs/project_notes/issues.md` to complete the branch.',
      'Update `docs/project_notes/issues.md` during the release-readiness pass.',
    ],
    'document-release',
    'timed release-pass hook regressions',
    [
      'before branch completion',
      'before presenting completion options',
      'to complete the branch',
      'required document-release handoff',
      'finish the release pass',
      'complete the release pass',
      'release-readiness pass',
      'during the release-readiness pass',
      'during release-readiness',
    ],
    'docs\\/project_notes\\/',
  );
  assertDetectsGateLikeHookSamples(
    [
      'featureforge:project-memory is a prerequisite for branch completion.',
      'Updating `docs/project_notes/issues.md` blocks branch completion.',
    ],
    'document-release',
    'release gate regressions',
    'docs\\/project_notes\\/',
  );
});

test('project-memory skill contract stays narrow, deterministic, and repo-safety-bound', () => {
  const projectMemory = readUtf8(getSkillPath('project-memory'));

  assert.match(projectMemory, /Treat `docs\/project_notes\/\*` as supportive context only;/);
  assert.match(projectMemory, /Default write set is limited to `docs\/project_notes\/\*` and the narrow project-memory section this repo owns in `AGENTS\.md`\./);
  assert.match(projectMemory, /If existing memory content is partially valid, preserve the valid content and create or normalize only the missing boundary pieces unless the user explicitly asks for a rewrite\./);
  assert.match(projectMemory, /Read skill-local `authority-boundaries\.md` before broad setup or repair work\./);
  assert.match(projectMemory, /Read skill-local `examples\.md` before writing new entries\./);
  assert.match(projectMemory, /Reuse the seed layouts in skill-local `references\/` when creating missing files\./);
  assert.match(projectMemory, /repo-safety check --intent write --stage featureforge:project-memory --task-id <current-memory-update> --path <repo-relative-path> --write-target repo-file-write/);
  assert.match(projectMemory, /repo-safety approve --stage featureforge:project-memory --task-id <current-memory-update> --reason "<explicit user approval>" --path <repo-relative-path> --write-target repo-file-write/);
  for (const rejectClass of [
    'SecretLikeContent',
    'AuthorityConflict',
    'TrackerDrift',
    'MissingProvenance',
    'OversizedDuplication',
    'InstructionAuthorityDrift',
  ]) {
    assert.match(projectMemory, new RegExp(String.raw`- \`${rejectClass}\``), `project-memory should list ${rejectClass} in the update flow`);
  }
});

test('generated skills use canonical runtime commands instead of helper executables', () => {
  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));
    assert.doesNotMatch(content, HELPER_COMMAND_PATTERN, `${skill} should not use helper-style executable names`);
  }
});

test('workflow handoff skills make terminal ownership explicit', () => {
  const usingFeatureForge = readUtf8(getSkillPath('using-featureforge'));
  const usingFeatureForgeTemplate = readUtf8(getTemplatePath('using-featureforge'));
  for (const [label, content] of [
    ['using-featureforge generated skill', usingFeatureForge],
    ['using-featureforge template', usingFeatureForgeTemplate],
  ]) {
    assert.match(
      content,
      /Check relevant or requested skills before responding or acting unless an explicit user instruction forbids skill use or gives a conflicting process\. User instructions always win\./,
      `${label} should keep skill selection subordinate to explicit user instructions`,
    );
    assert.doesNotMatch(
      content,
      /1% chance|ABSOLUTELY MUST|DO NOT HAVE A CHOICE|This is not negotiable|yes, even 1%/,
      `${label} should not reintroduce high-pressure skill-selection wording`,
    );
  }
  assert.doesNotMatch(usingFeatureForge, /brainstorming first, then implementation skills/);
  assertContainsFragments(usingFeatureForge, 'using-featureforge artifact-state routing', [
    'artifact-state workflow',
    'plan-ceo-review -> writing-plans -> plan-eng-review',
    'plan-fidelity-review runs only after engineering-review edits are complete',
    'Do NOT jump from brainstorming straight to implementation',
    'route by artifact state',
  ]);
  assertRuntimeFirstRoutingPrinciples(usingFeatureForge, 'using-featureforge');
  assert.doesNotMatch(
    usingFeatureForge,
    /\$_FEATUREFORGE_BIN plan execution recover/,
    'using-featureforge should not expose a concrete hidden recovery command literal',
  );
  assert.match(
    usingFeatureForge,
    /recovery remains on operator-routed public commands\./,
  );
  assert.doesNotMatch(usingFeatureForge, /If the JSON result is not `implementation_ready` and contains a non-empty `next_skill`, use that route as compatibility fallback\./);
  assertContainsOperatorPublicCommandAuthority(usingFeatureForge, 'using-featureforge');
  assert.match(
    usingFeatureForge,
    /canonical route reference[\s\S]{0,160}repair[\s\S]{0,160}stop rules; recovery remains on operator-routed public commands[\s\S]{0,1200}do not reconstruct routing from artifacts manually/i,
    'using-featureforge should keep normal routes on public operator-routed commands and reject hidden/manual recovery lanes',
  );
  assert.doesNotMatch(
    usingFeatureForge,
    /\$_FEATUREFORGE_BIN plan execution recommend --plan <approved-plan-path> --isolated-agents <available\|unavailable> --session-intent <stay\|separate\|unknown> --workspace-prepared <yes\|no\|unknown>/,
  );

  const ceoReview = readUtf8(getSkillPath('plan-ceo-review'));
  assert.match(ceoReview, /\*\*The terminal state is invoking writing-plans\.\*\*/);
  assert.match(ceoReview, /Do not draft a plan or offer implementation options from `plan-ceo-review`\./);
  assert.match(ceoReview, /keep using the same repo-relative spec path in later workflow\/operator and writing-plans handoffs/);
  assert.doesNotMatch(ceoReview, /runs `sync --artifact spec`/);
  assert.doesNotMatch(ceoReview, /"\$_FEATUREFORGE_BIN" workflow sync --artifact spec --path/);

  const engReview = readUtf8(getSkillPath('plan-eng-review'));
  assert.match(engReview, /\*\*The terminal state is presenting the execution preflight handoff with the approved plan path\.\*\*/);
  assert.match(engReview, /plan-eng-review also owns the late refresh-test-plan lane only when workflow\/operator explicitly routes to `qa_pending` with `phase_detail=test_plan_refresh_required` for the current approved plan revision\./);
  assert.match(engReview, /Missing or stale source test-plan projections on current QA recording or finish readiness are diagnostic-only and must not be treated as a refresh request by themselves\./);
  assert.doesNotMatch(engReview, /finish readiness reports `test_plan_artifact_missing`/);
  assert.match(engReview, /\*\*QA Requirement:\*\* required \| not-required/);
  assert.match(engReview, /\*\*Head SHA:\*\* \{current-head\}/);
  assert.match(engReview, /This field scopes the QA artifact for testers; it is not the authoritative finish-gate policy source\./);
  assert.match(engReview, /Set `\*\*Head SHA:\*\*` to the current `git rev-parse HEAD` for the branch state that this test-plan artifact covers\./);
  assert.match(engReview, /In that late-stage lane, the terminal state is returning to the finish-gate flow with a regenerated current-branch test-plan artifact, not reopening execution preflight\./);
  assert.match(engReview, /Before presenting the final execution preflight handoff, if `\$_FEATUREFORGE_BIN` is available, call `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`\./);
  assertContainsOperatorPublicCommandAuthority(engReview, 'plan-eng-review');
  assert.match(engReview, /If workflow\/operator JSON returns `phase` `executing`, present the normal execution preflight handoff below\./);
  assertLaterPhaseUsesInstalledRouteLaw(engReview, 'plan-eng-review generated skill');
  assert.doesNotMatch(engReview, /review_blocked/);
  assert.match(engReview, /Do not start implementation inside `plan-eng-review`\./);

  const brainstorming = readUtf8(getSkillPath('brainstorming'));
  assert.match(brainstorming, /Use that repo-relative spec path consistently in later review and workflow\/operator commands/);
  assert.match(brainstorming, /After the spec is written or updated, continue using the same repo-relative spec path in downstream review and workflow\/operator commands\./);
  assert.doesNotMatch(brainstorming, /record the intended spec path with `expect`/);
  assert.doesNotMatch(brainstorming, /"\$_FEATUREFORGE_BIN" workflow expect --artifact spec --path/);
  assert.doesNotMatch(brainstorming, /runs `sync --artifact spec`/);
  assert.doesNotMatch(brainstorming, /"\$_FEATUREFORGE_BIN" workflow sync --artifact spec --path/);

  const writingPlans = readUtf8(getSkillPath('writing-plans'));
  assert.match(writingPlans, /Use that repo-relative plan path consistently in later review and workflow\/operator commands/);
  assert.match(writingPlans, /Keep using the same repo-relative plan path in downstream review and workflow\/operator handoffs\./);
  assert.match(writingPlans, /Invoke `featureforge:plan-eng-review` for the first engineering review pass\./);
  assert.doesNotMatch(writingPlans, /Invoke `featureforge:plan-fidelity-review`\./);
  assert.doesNotMatch(writingPlans, /runtime-owned receipt/i);
  assert.doesNotMatch(writingPlans, /receipt records/i);
  assert.match(writingPlans, /plan-fidelity runs only after engineering-review edits are complete/i);
  assert.doesNotMatch(writingPlans, /record the intended plan path with `expect`/);
  assert.doesNotMatch(writingPlans, /"\$_FEATUREFORGE_BIN" workflow expect --artifact plan --path/);
  assert.doesNotMatch(writingPlans, /runs `sync --artifact plan`/);
  assert.doesNotMatch(writingPlans, /"\$_FEATUREFORGE_BIN" workflow sync --artifact plan --path/);
  assert.doesNotMatch(writingPlans, /Use the execution skill recommended by `\$_FEATUREFORGE_BIN plan execution recommend --plan <approved-plan-path>`/);

  const sdd = readUtf8(getSkillPath('subagent-driven-development'));
  assertContainsFragments(sdd, 'subagent-driven-development routed completion', [
    'workflow operator --plan <approved-plan-path> --json',
    'workflow/operator selects',
    'selected typed route',
    'selected handoff skill',
    'do not run a memorized terminal skill sequence',
  ]);
  assert.match(
    sdd,
    /workflow\/operator[\s\S]{0,240}(?:selected|returned)[\s\S]{0,240}route/i,
    'subagent-driven-development should route terminal work through workflow/operator output',
  );
  assert.doesNotMatch(sdd, /\[Invoke featureforge:requesting-code-review\]/);
  assert.doesNotMatch(sdd, /\[Invoke featureforge:document-release\]/);
  assert.doesNotMatch(sdd, /\[Invoke featureforge:finishing-a-development-branch\]/);
  const terminalOperatorLanes = [
    'featureforge:requesting-code-review',
    'featureforge:finishing-a-development-branch',
    'featureforge:qa-only',
    'featureforge:document-release',
  ];
  for (const lane of terminalOperatorLanes) {
    assert.match(
      sdd,
      new RegExp(`${escapeRegExp(lane)}[\\s\\S]{0,220}workflow/operator selects`, 'i'),
      `${lane} should stay conditioned on workflow/operator selection`,
    );
    assert.doesNotMatch(
      sdd,
      new RegExp(`\\*\\*Required workflow skills:\\*\\*[\\s\\S]{0,500}${escapeRegExp(lane)}`),
    );
    assert.doesNotMatch(
      sdd,
      new RegExp(`${escapeRegExp(lane)}\\*\\* - (?:REQUIRED|Required)(?![^\\n]*workflow/operator selects)`),
    );
  }
  assert.doesNotMatch(sdd, /Dispatch final code reviewer subagent for entire implementation/);
  assert.doesNotMatch(sdd, /\[Dispatch final code-reviewer\]/);

  const requestingReview = readUtf8(getSkillPath('requesting-code-review'));
  assertContainsFragments(requestingReview, 'requesting-code-review routed final gate', [
    'final cross-task review gate',
    'workflow/operator selects terminal final review',
    'current `HEAD`',
  ]);
  assert.doesNotMatch(requestingReview, /after `featureforge:document-release` is current/);
  assert.doesNotMatch(requestingReview, /After each task in subagent-driven development/);
  assert.match(requestingReview, /plan contract analyze-plan --spec "\$SOURCE_SPEC_PATH" --plan "\$APPROVED_PLAN_PATH" --format json/);
  assertContainsFragments(requestingReview, 'requesting-code-review final-review materialization', [
    'fresh-context reviewer',
    'REVIEWER_SOURCE',
    'REVIEWER_ID',
    'REVIEW_RESULT',
    'SUMMARY_FILE',
  ]);
  assert.match(
    requestingReview,
    /Installed Control Plane section[\s\S]{0,80}canonical route reference/i,
    'requesting-code-review should point final-review recording details at the canonical route reference',
  );
  assert.doesNotMatch(requestingReview, /reviewer_values_required: \["reviewer-source", "reviewer-id", "result", "summary-file"\]/);
  assert.doesNotMatch(requestingReview, /--result pass --summary-file review-summary\.md/);

  const finishSkill = readUtf8(getSkillPath('finishing-a-development-branch'));
  assert.match(finishSkill, /If the current work is not governed by an approved FeatureForge plan, skip this workflow-routed finish gate and continue with the normal completion flow\./);
  assert.doesNotMatch(finishSkill, /helper-owned finish gate|helper-backed finish readiness|If the helper returns `allowed`/);
});

test('planning review sync docs describe additive review summaries and richer QA handoff', () => {
  const ceoReview = readUtf8(getSkillPath('plan-ceo-review'));
  assert.match(ceoReview, /SELECTIVE EXPANSION/);
  assert.match(ceoReview, /Section 11: Design & UX Review/);
  assert.match(ceoReview, /## CEO Review Summary/);
  assert.match(ceoReview, /Label the source as `cross-model` only when the outside voice definitely uses a different model\/provider than the main reviewer\./);
  assert.match(ceoReview, /fresh-context-subagent/);
  assert.match(ceoReview, /transport truncates or summarizes/i);
  assert.match(ceoReview, /note `UI_SCOPE` for Section 11/);
  assert.match(ceoReview, /Present each expansion opportunity as its own individual interactive user question\./);
  assert.match(ceoReview, /Do not use PR metadata or repo default-branch APIs as a fallback; keep the system audit locally derivable from repository state\./);
  assert.doesNotMatch(ceoReview, /git symbolic-ref --short refs\/remotes\/origin\/HEAD/);
  assert.doesNotMatch(ceoReview, /for candidate in main master/);
  assert.doesNotMatch(ceoReview, /gh pr view --json baseRefName/);

  const engReview = readUtf8(getSkillPath('plan-eng-review'));
  assert.match(engReview, /coverage graph/i);
  assert.match(engReview, /## Key Interactions/);
  assert.match(engReview, /## Edge Cases/);
  assert.match(engReview, /## Critical Paths/);
  assert.match(engReview, /## E2E Test Decision Matrix/);
  assert.match(engReview, /REGRESSION RULE/i);
  assert.match(engReview, /loading, empty, error, success, partial, navigation, responsive, and accessibility-critical states/i);
  assert.match(engReview, /compatibility, retry\/timeout semantics, replay or backfill behavior, and rollback or migration verification/i);
  assert.match(engReview, /Label the source as `cross-model` only when the outside voice definitely uses a different model\/provider than the main reviewer\./);
  assert.match(engReview, /fresh-context-subagent/);
  assert.match(engReview, /transport truncates or summarizes/i);
  assert.match(engReview, /## Engineering Review Summary/);

  const writingPlans = readUtf8(getSkillPath('writing-plans'));
  assert.match(writingPlans, /## CEO Review Summary/);
  assert.match(writingPlans, /additive context only/);

  const qaOnly = readUtf8(getSkillPath('qa-only'));
  assert.match(qaOnly, /## Engineering Review Summary/);
  assert.match(qaOnly, /additive context only/);
  assert.match(qaOnly, /## E2E Test Decision Matrix/);
  assert.match(qaOnly, /Do not use PR metadata or repo default-branch APIs as a fallback; keep diff-aware scoping locally derivable from repository state\./);
  assert.match(qaOnly, /Match current-branch artifacts by their `\*\*Branch:\*\*` header, not by a filename substring glob, so `my-feature` cannot masquerade as `feature`\./);
  assert.doesNotMatch(qaOnly, /git symbolic-ref --short refs\/remotes\/origin\/HEAD/);
  assert.doesNotMatch(qaOnly, /for candidate in main master/);
  assert.doesNotMatch(qaOnly, /\*-"?\$BRANCH"?-test-plan-\*/);
  assert.doesNotMatch(qaOnly, /gh pr view --json baseRefName/);
});

test('approved workflow-state artifacts document the finalized helper contract', () => {
  const specDoc = readUtf8(path.join(REPO_ROOT, 'docs/archive', RETIRED_PRODUCT, 'specs/2026-03-22-runtime-integration-hardening-design.md'));
  assert.match(
    specDoc,
    new RegExp(String.raw`\`${RETIRED_PRODUCT}-workflow-status\` must emit schema-versioned structured diagnostics including \`contract_state\`, \`reason_codes\`, \`diagnostics\`, \`scan_truncated\`, and candidate counts`),
    'approved spec should describe structured route-time diagnostics',
  );
  assert.match(
    specDoc,
    /`phase` and `doctor` must compose session-entry state/,
    'approved spec should describe session-entry composition in the public CLI',
  );
  assert.match(
    specDoc,
    new RegExp(String.raw`\`${RETIRED_PRODUCT}-plan-execution\` must expose read-only \`preflight\`, \`gate-review\`, and \`gate-finish\` commands`),
    'approved spec should describe helper-owned execution gates',
  );

  const planDoc = readUtf8(path.join(REPO_ROOT, 'docs/archive', RETIRED_PRODUCT, 'plans/2026-03-22-runtime-integration-hardening.md'));
  assert.match(
    planDoc,
    /Route-time readiness and JSON diagnostics are driven by the same canonical approved-plan contract/,
    'approved plan should describe route-time canonical contract hardening',
  );
  assert.match(
    planDoc,
    /The public workflow CLI can report phase, diagnostics, handoff readiness, preflight state, review gate results, and finish gate results/,
    'approved plan should describe the expanded public workflow CLI surface',
  );
  assert.match(
    planDoc,
    /Late-stage gate tasks must leave stale-artifact and stale-evidence proof/,
    'approved plan should require stale-artifact and stale-evidence coverage',
  );
});

test('workflow docs avoid stale ambiguity, commit-ownership, and review-freshness contradictions', () => {
  const usingFeatureForge = readUtf8(getSkillPath('using-featureforge'));
  assert.match(usingFeatureForge, /Do not re-derive `phase`, `phase_detail`, readiness, or late-stage precedence from markdown headers\./);
  assert.doesNotMatch(usingFeatureForge, /newest relevant artifacts/);

  const documentRelease = readUtf8(getSkillPath('document-release'));
  assert.match(documentRelease, /does not own `git commit`, `git merge`, or `git push`/);
  assert.match(documentRelease, /workflow-routed release-readiness must be recorded through runtime-owned commands, not inferred from the companion markdown artifact alone\./);
  assert.match(documentRelease, /featureforge-\{safe-branch\}-release-readiness-\{datetime\}\.md/);
  assert.match(documentRelease, /\*\*Current Reviewed Branch State ID:\*\* git_tree:abc1234/);
  assert.match(documentRelease, /\*\*Branch Closure ID:\*\* branch-release-closure/);
  assert.match(documentRelease, /\*\*Result:\*\* pass/);
  assert.match(documentRelease, /Allowed `\*\*Result:\*\*` values:/);
  assert.match(documentRelease, /- `pass`/);
  assert.match(documentRelease, /- `blocked`/);
  assert.match(documentRelease, /Do not hand-write or edit this artifact\./);
  assert.doesNotMatch(documentRelease, ROUTE_SPECIFIC_COMMAND_MAPPING_PATTERN);
  assert.doesNotMatch(documentRelease, /also write a project-scoped release-readiness companion artifact/i);
  assert.doesNotMatch(documentRelease, /before writing the release-readiness companion artifact/i);
  assert.doesNotMatch(documentRelease, /Allowed `\*\*Result:\*\*` values:(?:.|\n)*- `ready`(?:.|\n)*- `blocked`/i);
  assert.match(
    documentRelease,
    /For workflow-routed work, get `BASE_BRANCH` from `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` \(`base_branch`\) using the concrete approved plan path\./,
  );
  assert.doesNotMatch(documentRelease, /\$_FEATUREFORGE_BIN workflow operator --json/);
  assert.match(documentRelease, /For reviewed-closure late-stage routing, use `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` with the concrete plan\./);
  assertContainsOperatorPublicCommandAuthority(documentRelease, 'document-release');
  assert.match(documentRelease, /Confirm workflow\/operator is routing release-readiness or release-blocker progression before recording release-readiness\./);
  assert.doesNotMatch(documentRelease, /branch_closure_recording_required_for_release_readiness`, execute only the returned typed argv or completed template-derived argv/);
  assert.doesNotMatch(documentRelease, /release_readiness_recording_ready`, bind concrete `result`/);
  assert.match(
    documentRelease,
    /For branch-closure bootstrap, release-readiness recording, or release-blocker resolution routes[\s\S]{0,140}workflow\/operator[\s\S]{0,140}canonical route reference[\s\S]{0,160}do not inline route-specific binding details here\./,
    'document-release should keep route-specific release recording on operator plus the canonical reference',
  );
  assert.doesNotMatch(documentRelease, /\$_FEATUREFORGE_BIN plan execution advance-late-stage --plan <approved-plan-path>/i);
  assert.doesNotMatch(documentRelease, /if \[ "\$PHASE_DETAIL" != "release_readiness_recording_ready" \]; then/);
  assert.match(documentRelease, /If workflow\/operator JSON reports any other phase or phase_detail, stop and return to the current workflow flow instead of forcing release-readiness recording from stale assumptions\./);
  assert.doesNotMatch(documentRelease, /\[--write-target git-commit\]/);
  assert.doesNotMatch(documentRelease, /origin\/HEAD/);
  assert.doesNotMatch(documentRelease, /branch\.<current>\.gh-merge-base/);

  const qaOnly = readUtf8(getSkillPath('qa-only'));
  assert.match(qaOnly, /featureforge-\{safe-branch\}-test-outcome-\{datetime\}\.md/);
  assert.match(qaOnly, /do not hand-write the structured finish-gate artifact/i);
  assert.match(qaOnly, /\*\*Base Branch:\*\* main/);
  assert.match(qaOnly, /\*\*Current Reviewed Branch State ID:\*\* git_tree:abc1234/);
  assert.match(qaOnly, /\*\*Branch Closure ID:\*\* branch-release-closure/);
  assert.match(qaOnly, /\*\*Generated By:\*\* featureforge\/qa/);
  assert.match(qaOnly, /If no URL is provided, run `diff-aware` mode with an explicitly provided `BASE_BRANCH`:/);
  assert.doesNotMatch(qaOnly, /also write a project-scoped outcome artifact/i);
  assert.doesNotMatch(qaOnly, /`diff-aware` inference from the current branch/i);
  assert.doesNotMatch(qaOnly, /automatically enter `diff-aware` mode/i);
  assert.doesNotMatch(qaOnly, /\*\*Generated By:\*\* featureforge:qa-only/);

  const generatedReviewerAgent = readUtf8(path.join(REPO_ROOT, 'agents/code-reviewer.md'));
  assert.match(
    generatedReviewerAgent,
    /Require caller-provided base branch, base SHA, head SHA, plan path if plan-routed, and any runtime context the caller wants considered/,
  );
  assert.match(
    generatedReviewerAgent,
    /Do not derive, repair, or reconstruct missing workflow context locally/,
  );
  assert.match(
    generatedReviewerAgent,
    /When runtime-owned execution evidence, completed task-packet context, or coverage-matrix excerpts are included in the handoff, read them too and use them as supplemental plan-routed review context/,
  );
  assert.match(
    generatedReviewerAgent,
    /Treat provided-but-stale or unreadable execution evidence as a blocking issue for plan-routed final review, but do not require the public flow to harvest supplemental evidence or task-packet context manually when the handoff omitted it/,
  );
  assert.doesNotMatch(generatedReviewerAgent, /origin\/HEAD/);
  assert.doesNotMatch(generatedReviewerAgent, /branch\.<current>\.gh-merge-base/);
  assert.doesNotMatch(generatedReviewerAgent, /needs-user-input/);
  assert.doesNotMatch(
    generatedReviewerAgent,
    /Treat missing or stale execution evidence as a blocking issue for plan-routed final review/,
  );

  const reviewerAgentInstructions = readUtf8(path.join(REPO_ROOT, 'agents/code-reviewer.instructions.md'));
  assert.doesNotMatch(reviewerAgentInstructions, /needs-user-input/);

  const reviewerBriefingTemplate = readUtf8(path.join(REPO_ROOT, 'skills/requesting-code-review/code-reviewer.md'));
  assert.doesNotMatch(reviewerBriefingTemplate, /needs-user-input/);

  const finishSkill = readUtf8(getSkillPath('finishing-a-development-branch'));
  assert.match(finishSkill, /A review stops being fresh as soon as new repo changes land, including release-doc or metadata edits from `featureforge:document-release`/);
  assert.match(finishSkill, /If `featureforge:document-release` writes repo files or changes release metadata, treat any earlier code review as stale\. Requery workflow\/operator and run `featureforge:requesting-code-review` only when the operator selects the terminal final-review lane\./);
  assert.match(
    finishSkill,
    /For workflow-routed terminal completion, do not run a memorized terminal review or QA chain in this step\. Requery workflow\/operator and run only the lane or typed argv\/template it selects\./,
  );
  assert.match(
    finishSkill,
    /Do not run a fixed terminal sequence from memory\. Run `featureforge:document-release`, terminal `featureforge:requesting-code-review`, `featureforge:qa-only`, or `advance-late-stage` only when workflow\/operator selects that handoff lane or returns the selected public argv\/template route for it\./,
  );
  assert.match(finishSkill, /If workflow\/operator routes QA, keep it downstream of a current final-review pass; do not move QA ahead by skill-order memory\./);
  assert.doesNotMatch(finishSkill, /document-release` -> terminal `featureforge:requesting-code-review` ->/);
  assert.doesNotMatch(finishSkill, /after `featureforge:document-release` and the terminal `featureforge:requesting-code-review` gate are current/);
  assert.doesNotMatch(finishSkill, /after `featureforge:document-release` and any required `featureforge:qa-only` handoff are current/);
  assert.doesNotMatch(finishSkill, /after `featureforge:document-release` and any required QA handoff/);

  const routingScenarios = readUtf8(path.join(REPO_ROOT, 'tests/evals/using-featureforge-routing.scenarios.md'));
  assert.match(routingScenarios, /branch-completion language still routes to `requesting-code-review` when no fresh final review artifact exists/i);
  assert.match(routingScenarios, /fresh code-review, QA, and release-readiness artifacts exist/i);

  const readme = readUtf8(path.join(REPO_ROOT, 'README.md'));
  assert.match(readme, /Seven layers matter:/);
  assert.match(
    readme,
    /Late-stage completion is operator-routed, not a memorized skill chain:/,
  );
  assert.match(
    readme,
    /workflow\/operator may route to `featureforge:document-release`, terminal `featureforge:requesting-code-review`, `featureforge:qa-only`, or `featureforge:finishing-a-development-branch`; use those skills only when the current operator route selects them/,
  );
  assert.doesNotMatch(
    readme,
    /Completion then flows through/,
    'README should not teach a fixed late-stage skill sequence',
  );
  assert.match(
    readme,
    /compatibility\/debug command boundaries \(`gate-\*`, low-level `record-\*`\) must not be required in the normal path/,
  );
  assert.match(readme, /Execute only typed argv\/template-derived public argv/);
  assert.match(readme, /references\/operator-route-authority\.md/);
  assert.doesNotMatch(
    readme,
    /`\$_FEATUREFORGE_BIN plan execution rebuild-evidence --plan <approved-plan-path>` replays rebuildable execution-evidence targets from the current approved plan and refreshes helper-owned closure receipts against the current runtime state\./,
    'README should not present rebuild-evidence refresh as normal progression guidance',
  );
  assert.doesNotMatch(
    readme,
    /each task runs a fresh-context independent review loop until `gate-review` is green/,
    'README should stop teaching gate-review as the task-closure green loop',
  );
  assert.match(
    readme,
    /compatibility\/debug command boundaries \(`gate-\*`, low-level `record-\*`\) must not be required in the normal path/,
  );
  assert.doesNotMatch(
    readme,
    /the broader public execution surface also includes commands such as `note`, `complete`, `reopen`, `transfer`, and compatibility\/diagnostic helpers when the route or workflow boundary requires them\./,
    'README should keep compatibility helpers out of the normal public execution surface',
  );
  const completionSection = readme.slice(readme.indexOf('Late-stage completion is operator-routed'), readme.indexOf('## Project Memory'));
  assert.match(
    completionSection,
    /execute only typed `recommended_public_command_argv`; when a template needs input, rerun the same plan-bound workflow\/operator query with `--input NAME=VALUE`/,
    'README completion section should bind execution to typed operator surfaces',
  );

  const codexReadme = readUtf8(path.join(REPO_ROOT, 'docs/README.codex.md'));
  assert.match(
    codexReadme,
    /late-stage and terminal progression are operator-routed through `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`; execute the selected typed argv\/template route and use `references\/operator-route-authority\.md` for route-specific binding or selected handoff lanes/,
  );
  assert.doesNotMatch(codexReadme, /for workflow-routed terminal sequencing, run `featureforge:document-release` before terminal `featureforge:requesting-code-review`/);
  assert.match(
    codexReadme,
    /compatibility\/debug command boundaries .* must not be required in the normal path; normal progression stays on `workflow operator`, `close-current-task`, and `advance-late-stage`/,
  );
  assert.match(
    codexReadme,
    /`\$_FEATUREFORGE_BIN workflow doctor --plan <approved-plan-path> --json` is the first orientation\/diagnosis surface after handoff; `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` remains the authoritative routing surface, and `\$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` is only for deeper diagnostics/,
  );

  const copilotReadme = readUtf8(path.join(REPO_ROOT, 'docs/README.copilot.md'));
  assert.match(
    copilotReadme,
    /late-stage and terminal progression are operator-routed through `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`; execute the selected typed argv\/template route and use `references\/operator-route-authority\.md` for route-specific binding or selected handoff lanes/,
  );
  assert.doesNotMatch(copilotReadme, /for workflow-routed terminal sequencing, run `featureforge:document-release` before terminal `featureforge:requesting-code-review`/);
  assert.match(
    copilotReadme,
    /compatibility\/debug command boundaries .* must not be required in the normal path; normal progression stays on `workflow operator`, `close-current-task`, and `advance-late-stage`/,
  );
  assert.match(
    copilotReadme,
    /`\$_FEATUREFORGE_BIN workflow doctor --plan <approved-plan-path> --json` is the first orientation\/diagnosis surface after handoff; `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` remains the authoritative routing surface, and `\$_FEATUREFORGE_BIN plan execution status --plan <approved-plan-path>` is only for deeper diagnostics/,
  );

  const lateStageReference = readUtf8(path.join(REPO_ROOT, 'review/late-stage-precedence-reference.md'));
  assert.match(lateStageReference, /Legacy finish-gate compatibility commands are compatibility\/debug boundaries,\s+not normal-path commands\./);
  assert.match(lateStageReference, /[Ll]ow-level `record-\*` commands are compatibility\/debug boundaries and must not\s+be required by normal-path guidance\./);
  assert.match(
    lateStageReference,
    /When workflow\/operator selects a terminal late-stage lane,\s+execute that selected\s+typed route or selected handoff lane\. Do not use this reference to run a\s+memorized chain\./,
  );
});

test('late-stage precedence reference delegates row authority to runtime', () => {
  const lateStageReference = readUtf8(path.join(REPO_ROOT, 'review/late-stage-precedence-reference.md'));
  const runtimePrecedence = readUtf8(path.join(REPO_ROOT, 'src/execution/late_stage_precedence.rs'));

  assert.match(
    runtimePrecedence,
    /const PRECEDENCE_ROWS: (?:\&\[LateStageRow\]|\[LateStageRow; \d+\]) = (?:\&)?\[/,
    'runtime should continue owning late-stage precedence rows',
  );
  assert.match(
    lateStageReference,
    /Do not maintain a second phase matrix in\s+this markdown file\./,
    'late-stage reference should explicitly avoid becoming a second source of truth',
  );
  assert.match(lateStageReference, /src\/execution\/late_stage_precedence\.rs/);
  assert.match(lateStageReference, /\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json/);
  assert.match(lateStageReference, /references\/operator-route-authority\.md/);
  assert.doesNotMatch(
    lateStageReference,
    /^\| Release Gate \| Review Gate \| QA Gate \| Phase \|/m,
    'late-stage reference should not keep a manually duplicated precedence table',
  );
  for (const internalActionToken of [
    'advance_late_stage',
    'dispatch_final_review',
    'run_qa',
    'run_finish_review_gate',
    'run_finish_completion_gate',
  ]) {
    assert.doesNotMatch(
      lateStageReference,
      new RegExp(escapeRegex(internalActionToken)),
      `late-stage reference should avoid internal action token ${internalActionToken}`,
    );
  }
});

test('active eval docs use featureforge state roots', () => {
  const evalReadme = readUtf8(path.join(REPO_ROOT, 'tests/evals/README.md'));
  assert.match(evalReadme, /\$FEATUREFORGE_STATE_DIR\/evals\/` or `~\/\.featureforge\/evals\//);
  assert.match(evalReadme, /~\/\.featureforge\/projects\/<slug>\//);
  assert.doesNotMatch(evalReadme, new RegExp(String.raw`~\/\.${RETIRED_PRODUCT}\/(?:evals|projects)\/`));

  const searchBeforeBuildingOrchestrator = readUtf8(path.join(REPO_ROOT, 'tests/evals/search-before-building-contract.orchestrator.md'));
  assert.match(searchBeforeBuildingOrchestrator, /~\/\.featureforge\/projects\/<slug>\/search-before-building-contract-r2\//);
  assert.doesNotMatch(searchBeforeBuildingOrchestrator, new RegExp(String.raw`~\/\.${RETIRED_PRODUCT}\/projects\/`));

  const evalObservability = readUtf8(path.join(REPO_ROOT, 'tests/evals/helpers/eval-observability.mjs'));
  assert.match(evalObservability, /FEATUREFORGE_STATE_DIR/);
  assert.match(evalObservability, /\.featureforge/);
  assert.doesNotMatch(evalObservability, new RegExp(String.raw`\b${RETIRED_PRODUCT.toUpperCase()}_STATE_DIR\b`));
  assert.doesNotMatch(evalObservability, new RegExp(String.raw`\.${RETIRED_PRODUCT}`));
});

test('legacy command shim docs are removed from the active repo', () => {
  for (const relativePath of [
    'commands/brainstorm.md',
    'commands/write-plan.md',
    'commands/execute-plan.md',
  ]) {
    assert.equal(
      fs.existsSync(path.join(REPO_ROOT, relativePath)),
      false,
      `${relativePath} should stay deleted`,
    );
  }
});

test('repo-owned operator docs move to canonical runtime command vocabulary', () => {
  for (const relativePath of [
    'README.md',
    'docs/README.codex.md',
    'docs/README.copilot.md',
    'RELEASE-NOTES.md',
  ]) {
    const content = readUtf8(path.join(REPO_ROOT, relativePath)).replace(
      /tests\/codex-runtime\/test-featureforge-[^\s`]+/g,
      'tests/codex-runtime/test-runtime-contract.sh',
    );
    assert.doesNotMatch(
      content,
      HELPER_COMMAND_PATTERN,
      `${relativePath} should not use helper-style executable names`,
    );
  }
});

test('release-facing docs point at docs/testing.md as the canonical validation entrypoint', () => {
  for (const relativePath of [
    'README.md',
    'docs/README.codex.md',
    'docs/README.copilot.md',
    '.codex/INSTALL.md',
    '.copilot/INSTALL.md',
  ]) {
    assert.match(
      readUtf8(path.join(REPO_ROOT, relativePath)),
      /docs\/testing\.md/,
      `${relativePath} should point readers at docs/testing.md for the canonical validation matrix`,
    );
  }

  for (const relativePath of [
    'docs/testing.md',
    'docs/test-suite-enhancement-plan.md',
  ]) {
    const content = readUtf8(path.join(REPO_ROOT, relativePath));
    assert.match(
      content,
      /cargo nextest run --all-targets --all-features --no-fail-fast/,
      `${relativePath} should document the full no-fail-fast Rust gate`,
    );
    assert.doesNotMatch(
      content,
      /^cargo nextest run --test\b/m,
      `${relativePath} should not present targeted nextest commands as a documented final gate`,
    );
  }

  const readme = readUtf8(path.join(REPO_ROOT, 'README.md'));
  assert.doesNotMatch(
    readme,
    /Core validation:\s*```bash[\s\S]*?```/,
    'README.md should not duplicate the canonical validation matrix',
  );
  for (const forbiddenPartialMatrixCommand of [
    /```bash[\s\S]*cargo clippy --all-targets --all-features[\s\S]*```/,
    /```bash[\s\S]*cargo nextest run --all-targets --all-features[\s\S]*```/,
    /```bash[\s\S]*node scripts\/run-codex-runtime-tests\.mjs[\s\S]*```/,
    /```bash[\s\S]*node --test tests\/evals\/[\s\S]*```/,
    /```bash[\s\S]*npm --prefix tests\/brainstorm-server test[\s\S]*```/,
    /```bash[\s\S]*scripts\/verify-installed-control-plane-isolation\.sh[\s\S]*```/,
    /```bash[\s\S]*scripts\/run-public-runtime-flow-tests\.sh[\s\S]*```/,
    /```bash[\s\S]*scripts\/run-internal-runtime-compatibility-tests\.sh[\s\S]*```/,
    /```bash[\s\S]*scripts\/run-rust-tests-sharded\.sh[\s\S]*```/,
    /```bash[\s\S]*scripts\/refresh-prebuilt-runtime\.sh[\s\S]*```/,
    /```bash[\s\S]*scripts\/prebuilt-runtime-provenance\.mjs verify[\s\S]*```/,
  ]) {
    assert.doesNotMatch(
      readme,
      forbiddenPartialMatrixCommand,
      'README.md should link to docs/testing.md instead of reintroducing partial validation command blocks',
    );
  }
});

test('active docs describe the post-session-entry routing contract', () => {
  for (const relativePath of [
    'README.md',
    'docs/README.codex.md',
    'docs/README.copilot.md',
  ]) {
    const content = readUtf8(path.join(REPO_ROOT, relativePath));
    assert.match(
      content,
      /`using-featureforge` is the human-readable entry router that consults `\$_FEATUREFORGE_BIN workflow` directly from repo-visible artifacts\./,
      `${relativePath} should describe direct workflow routing from repo-visible artifacts`,
    );
    assert.doesNotMatch(content, /featureforge session-entry/, `${relativePath} should not mention the removed session-entry command family`);
    assert.doesNotMatch(content, /FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY/, `${relativePath} should not mention the removed strict gate env key`);
  }

  const testingDoc = readUtf8(path.join(REPO_ROOT, 'docs/testing.md'));
  assert.match(
    testingDoc,
    /direct workflow routing without session-entry prerequisites/i,
    'docs/testing.md should describe the no-session-entry routing contract',
  );

  for (const relativePath of [
    '.codex/INSTALL.md',
    '.copilot/INSTALL.md',
  ]) {
    const content = readUtf8(path.join(REPO_ROOT, relativePath));
    assert.match(
      content,
      /packaged install binary.*featureforge repo runtime-root --path/is,
      `${relativePath} should describe runtime-root-based packaged binary routing`,
    );
    assert.doesNotMatch(
      content,
      /featureforge session-entry resolve/i,
      `${relativePath} should not mention the removed session-entry entry contract`,
    );
    assert.doesNotMatch(
      content,
      /--spawned-subagent(?:-opt-in)?/i,
      `${relativePath} should not advertise removed spawned-subagent session-entry flags`,
    );
  }

  const releaseNotes = readUtf8(path.join(REPO_ROOT, 'RELEASE-NOTES.md'));
  assert.match(
    releaseNotes,
    /breaking contract delta: remove `featureforge session-entry`/i,
    'RELEASE-NOTES.md should call out the removed session-entry command surface',
  );
  assert.match(
    releaseNotes,
    /workflow routing now ignores legacy session-entry decision files and gate env inputs/i,
    'RELEASE-NOTES.md should describe the direct-routing breaking delta',
  );
  assert.match(
    releaseNotes,
    /breaking output contract changes/i,
    'RELEASE-NOTES.md should include a dedicated breaking output contract changes section',
  );
  const removedSessionEntryFragments = [
    'session_entry',
    'needs_user_choice',
    'bypassed',
    'session_entry_gate',
    'continue_outside_featureforge',
    'schema_version',
    '2',
  ];
  assertLineContainsTextFragments(
    releaseNotes,
    'workflow phase --json',
    removedSessionEntryFragments,
    'RELEASE-NOTES.md should enumerate the workflow phase output removals and new schema version',
  );
  assertLineContainsTextFragments(
    releaseNotes,
    'workflow doctor --json',
    removedSessionEntryFragments,
    'RELEASE-NOTES.md should enumerate the workflow doctor output removals and new schema version',
  );
  assertLineContainsTextFragments(
    releaseNotes,
    'workflow handoff --json',
    removedSessionEntryFragments,
    'RELEASE-NOTES.md should enumerate the workflow handoff output removals and new schema version',
  );
  const activeReleaseNotes = releaseNotes.split('Historical note:')[0] ?? releaseNotes;
  assert.doesNotMatch(
    activeReleaseNotes,
    /workflow status --refresh/is,
    'RELEASE-NOTES.md should not document removed workflow status as an active command',
  );
  assert.match(
    releaseNotes,
    /windows prebuilt artifacts/i,
    'RELEASE-NOTES.md should mention refreshed windows prebuilt artifacts when the checked-in windows binary changes',
  );
  assert.match(
    releaseNotes,
    /same runtime-owned routing decision instead of allowing diagnostic\/status drift/i,
    'RELEASE-NOTES.md should call out the shared operator/status routing-parity contract',
  );
  assert.match(
    releaseNotes,
    /projection-only regeneration that fails closed with append-only\/manual-repair blockers instead of rewriting authoritative proof in place/i,
    'RELEASE-NOTES.md should describe the fail-closed projection-only rebuild-evidence contract',
  );
  assertLineContainsTextFragments(
    releaseNotes,
    'plan execution status --json',
    [
      'harness_phase',
      'next_action',
      'recommended_public_command_argv',
      'recommended_command',
      'recording_context',
      'diagnostic-only',
    ],
    'RELEASE-NOTES.md should describe the aligned plan execution status JSON route vocabulary and recording context output contract',
  );
});

test('runtime-remediation regression inventory fixture stays complete', () => {
  const inventory = readUtf8(path.join(REPO_ROOT, 'tests/fixtures/runtime-remediation/README.md'));
  for (const heading of [
    /^# Runtime Remediation Regression Inventory/m,
    /^## Scenario Coverage Matrix/m,
    /^## Coverage Map/m,
    /^### Command-Budget Coverage/m,
  ]) {
    assert.match(
      inventory,
      heading,
      `runtime-remediation inventory should include ${heading.source}`,
    );
  }
  assert.match(
    inventory,
    /scenario\/file granularity/,
    'runtime-remediation inventory should stay at scenario/file granularity',
  );
  assert.doesNotMatch(
    inventory,
    /^## Function-Level Traceability/m,
    'runtime-remediation inventory should not reintroduce function-level traceability',
  );
  for (const scenario of [
    'FS-01', 'FS-02', 'FS-03', 'FS-04', 'FS-05', 'FS-06',
    'FS-07', 'FS-08', 'FS-09', 'FS-10', 'FS-11', 'FS-12', 'FS-13', 'FS-14', 'FS-15', 'FS-16',
    'FS-17', 'FS-18', 'FS-19', 'FS-20', 'FS-21', 'FS-22',
  ]) {
    assert.match(
      inventory,
      new RegExp(`\\b${scenario}\\b`),
      `runtime-remediation inventory should include ${scenario}`,
    );
  }
  assert.match(
    inventory,
    /Command-Budget Coverage[\s\S]*FS-11[\s\S]*FS-17[\s\S]*FS-20/i,
    'runtime-remediation inventory should keep command-budget coverage visible',
  );
});

test('route-owning workflow skills enforce installed control-plane runtime routing rules', () => {
  const generatedSkills = new Set(listGeneratedSkills());
  const routeOwningSkills = [...ROUTE_OWNING_GENERATED_SKILLS];
  for (const skill of routeOwningSkills) {
    assert.equal(generatedSkills.has(skill), true, `${skill} should be a generated skill with explicit route-law ownership`);
    assert.equal(routeLawModeForTemplate(getTemplatePath(skill)), ROUTE_LAW_MODE.FULL, `${skill} should render the full route-law mode`);
    const content = readUtf8(getSkillPath(skill));
    assertContainsOperatorPublicCommandAuthority(content, skill);
    const installedControlPlane = extractSection(content, 'Installed Control Plane');
    let withoutInstalledControlPlane = content.replace(installedControlPlane, '');
    if (skill === 'requesting-code-review') {
      withoutInstalledControlPlane = withoutInstalledControlPlane.replace(
        blockBetween(
          content,
          '## Terminal Final-Review Route',
          'See `$_FEATUREFORGE_ROOT/references/execution-review-qa-examples.md`',
          skill,
        ),
        '',
      );
    }
    assert.doesNotMatch(
      withoutInstalledControlPlane,
      GENERATED_ROUTE_FIELD_PATTERN,
      `${skill} should keep executable field law inside the compact Installed Control Plane section`,
    );
    if (content.includes('Reviewed-Closure Route Authority')) {
      assertRouteAuthoritySectionIsCompact(content, skill);
    }
    assert.doesNotMatch(
      content,
      /When workflow\/operator returns `recommended_public_command_argv`|Detailed argv binding and operator materialization law|recommended_public_command_argv\[0\] == "featureforge"|replacing argv\[0\]/,
      `${skill} should not duplicate detailed argv rebinding law`,
    );
    assert.doesNotMatch(
      withoutInstalledControlPlane,
      RETIRED_RUNTIME_COMMAND_TRAP_PATTERN,
      `${skill} should not contain retired helper or workflow-status command traps outside the Installed Control Plane section`,
    );
  }

  for (const skill of listGeneratedSkills().filter((candidate) => !routeOwningSkills.includes(candidate))) {
    assert.equal(routeLawModeForTemplate(getTemplatePath(skill)), ROUTE_LAW_MODE.REFERENCE, `${skill} should render the compact route-reference mode`);
    const content = readUtf8(getSkillPath(skill));
    assert.doesNotMatch(content, /## Installed Control Plane/, `${skill} should not carry full installed control-plane guidance`);
    assert.match(content, /## Runtime Route Reference/, `${skill} should keep only the compact runtime route reference`);
    assertContainsFragments(
      extractSection(content, 'Runtime Route Reference'),
      `${skill} generated runtime route reference`,
      ['`$_FEATUREFORGE_ROOT/references/operator-route-authority.md`'],
    );
    assert.doesNotMatch(
      extractSection(content, 'Runtime Route Reference') ?? '',
      GENERATED_ROUTE_FIELD_PATTERN,
      `${skill} compact route reference should not duplicate executable route law`,
    );
  }

  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));
    assert.doesNotMatch(
      content,
      RETIRED_RUNTIME_COMMAND_TRAP_PATTERN,
      `${skill} should not contain retired helper or workflow-status command traps`,
    );
    assert.doesNotMatch(
      content,
      ROUTE_SPECIFIC_COMMAND_MAPPING_PATTERN,
      `${skill} should keep route-specific command/result mapping in the canonical route reference, not generated skill docs`,
    );
  }
});

test('workflow execution and review skills require runtime provenance sections for FeatureForge-on-FeatureForge work', () => {
  const executingPlans = readUtf8(getSkillPath('executing-plans'));
  const subagentDriven = readUtf8(getSkillPath('subagent-driven-development'));
  const requestingReview = readUtf8(getSkillPath('requesting-code-review'));
  const reviewerTemplate = readUtf8(path.join(REPO_ROOT, 'skills/requesting-code-review/code-reviewer.md'));

  for (const [name, content] of [
    ['executing-plans', executingPlans],
    ['subagent-driven-development', subagentDriven],
  ]) {
    assert.match(
      content,
      /For FeatureForge-on-FeatureForge execution, every execution-evidence update must include a runtime provenance section/,
      `${name} should require runtime provenance sections in execution evidence`,
    );
    assert.match(
      content,
      /installed runtime path used for live workflow routing/,
      `${name} should require installed runtime path provenance`,
    );
    assert.match(
      content,
      /installed runtime hash used for live workflow routing/,
      `${name} should require installed runtime hash provenance`,
    );
    assert.match(
      content,
      /state dir used for live workflow commands/,
      `${name} should require live state-dir provenance`,
    );
    assert.match(
      content,
      /workspace runtime hash used for tests\/fixtures \(or `none` when no workspace runtime was used\)/,
      `${name} should require workspace runtime hash disclosure`,
    );
    assert.match(
      content,
      /explicit confirmation that workspace runtime did not mutate live workflow state \(or the explicit approved override record when it did\)/,
      `${name} should require explicit live-mutation confirmation`,
    );
  }

  assert.match(
    requestingReview,
    /Required review-dispatch provenance for FeatureForge-on-FeatureForge work:/,
    'requesting-code-review should require review-dispatch provenance',
  );
  assert.match(
    requestingReview,
    /base branch, base SHA, head SHA, working-tree diff hash, installed runtime path\/hash used for live routing, workspace runtime hash used for tests \(if any\), live state dir, active FeatureForge skill source\/roots, installed skill root, and workspace skill root\./,
    'requesting-code-review should enumerate required dispatch provenance fields',
  );
  assert.match(
    requestingReview,
    /Reviewers must fail review when live workflow mutation used workspace runtime without an explicit approved override record\./,
    'requesting-code-review should require reviewer failure on unapproved workspace-runtime live mutations',
  );
  assert.match(
    requestingReview,
    /Reviewers must fail review when active FeatureForge skills resolve from the workspace skill root instead of the installed skill root without an explicit approved self-hosting exception\./,
    'requesting-code-review should require reviewer failure on workspace skill-root discovery',
  );
  assert.match(
    requestingReview,
    /Reviewers must fail review when FeatureForge-on-FeatureForge provenance is missing or incomplete\./,
    'requesting-code-review should require reviewer failure when provenance is missing',
  );

  for (const field of [
    '\\*\\*Working-tree diff hash:\\*\\* \\{WORKING_TREE_DIFF_HASH\\}',
    '\\*\\*Installed runtime path \\(live routing\\):\\*\\* \\{INSTALLED_RUNTIME_PATH\\}',
    '\\*\\*Installed runtime hash \\(live routing\\):\\*\\* \\{INSTALLED_RUNTIME_HASH\\}',
    '\\*\\*Workspace runtime hash \\(tests/fixtures\\):\\*\\* \\{WORKSPACE_RUNTIME_HASH\\}',
    '\\*\\*Live state dir:\\*\\* \\{LIVE_STATE_DIR\\}',
    '\\*\\*Active FeatureForge skill source:\\*\\* \\{ACTIVE_FEATUREFORGE_SKILL_SOURCE\\}',
    '\\*\\*Active FeatureForge skill roots:\\*\\* \\{ACTIVE_FEATUREFORGE_SKILL_ROOTS\\}',
    '\\*\\*Installed skill root:\\*\\* \\{INSTALLED_SKILL_ROOT\\}',
    '\\*\\*Workspace skill root:\\*\\* \\{WORKSPACE_SKILL_ROOT\\}',
    '\\*\\*Workspace-runtime live-mutation confirmation:\\*\\* \\{WORKSPACE_RUNTIME_LIVE_MUTATION_CONFIRMATION\\}',
  ]) {
    assert.match(
      reviewerTemplate,
      new RegExp(field),
      `code-reviewer template should include provenance field ${field}`,
    );
  }
  assert.match(
    reviewerTemplate,
    /Fail review if any live workflow mutation used workspace runtime without an explicit approved override record\./,
    'code-reviewer template should require failing unapproved workspace-runtime live mutations',
  );
  assert.match(
    reviewerTemplate,
    /Fail review if active FeatureForge skills resolve from the workspace skill root instead of the installed skill root without an explicit approved self-hosting exception\./,
    'code-reviewer template should require failing workspace skill-root discovery',
  );
});
