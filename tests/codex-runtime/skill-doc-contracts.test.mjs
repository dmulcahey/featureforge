import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
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

function getTemplatePath(skill) {
  return path.join(SKILLS_DIR, skill, 'SKILL.md.tmpl');
}

function getSkillPath(skill) {
  return path.join(SKILLS_DIR, skill, 'SKILL.md');
}

function getSkillDescription(skill) {
  const frontmatter = parseFrontmatter(readUtf8(getSkillPath(skill)));
  assert.ok(frontmatter, `${skill} should have frontmatter`);
  return frontmatter.description;
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

function assertSeparatesCandidateArtifactsFromAuthoritativeMutations(content, label) {
  const hasCandidateSurface = /(candidate|task packet|task-packet|packet context|handoff|coverage matrix)/i.test(content);
  const hasAuthoritativeSurface = /(authoritative|helper-owned|coordinator-owned|execution state|execution evidence|review gate|finish-gate|gate-review)/i.test(content);
  const hasBoundaryLanguage = /(must not|do not|never|may not|only|owns?|owned|instead of|fail closed)/i.test(content);
  assert.ok(
    hasCandidateSurface && hasAuthoritativeSurface && hasBoundaryLanguage,
    `${label} should distinguish candidate/planning artifacts from authoritative runtime mutations`,
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

function stripInlineCode(value) {
  return value.replace(/^`|`$/g, '');
}

function parseRuntimeLateStageRows(source) {
  const rowPattern = /LateStageRow\s*\{\s*release:\s*GateState::(Blocked|Ready),\s*review:\s*GateState::(Blocked|Ready),\s*qa:\s*GateState::(Blocked|Ready),\s*phase:\s*"([^"]+)",\s*reason_family:\s*"([^"]+)",\s*\}/gms;
  const rows = [];
  for (const match of source.matchAll(rowPattern)) {
    rows.push({
      release: match[1].toLowerCase(),
      review: match[2].toLowerCase(),
      qa: match[3].toLowerCase(),
      phase: match[4],
      reasonFamily: match[5],
    });
  }
  return rows;
}

function parseLateStageReferenceRows(markdown) {
  return markdown
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.startsWith('| blocked') || line.startsWith('| ready'))
    .map((line) => {
      const columns = line.split('|').slice(1, -1).map((cell) => cell.trim());
      assert.equal(columns.length, 7, `late-stage precedence table row should have 7 columns: ${line}`);
      return {
        release: columns[0],
        review: columns[1],
        qa: columns[2],
        phase: stripInlineCode(columns[3]),
        nextAction: stripInlineCode(columns[4]),
        recommendedSkill: stripInlineCode(columns[5]),
        reasonFamily: stripInlineCode(columns[6]),
      };
    });
}

const LATE_STAGE_PHASE_TO_ACTION = new Map([
  [
    'document_release_pending',
    'derived from phase_detail: advance late stage (branch-closure refresh lane); resolve release blocker',
  ],
  [
    'final_review_pending',
    'derived from phase_detail: request final review; wait for external review result; advance late stage',
  ],
  ['qa_pending', 'derived from phase_detail: run QA; refresh test plan'],
  [
    'ready_for_branch_completion',
    'derived from phase_detail: finish branch',
  ],
]);

const LATE_STAGE_PHASE_TO_SKILL = new Map([
  ['document_release_pending', 'featureforge:document-release'],
  ['final_review_pending', 'featureforge:requesting-code-review'],
  ['qa_pending', 'featureforge:qa-only'],
  ['ready_for_branch_completion', 'featureforge:finishing-a-development-branch'],
]);

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
  }
});

test('generated non-router skill docs include the shared Search Before Building section', () => {
  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));

    const section = extractSection(content, 'Search Before Building');
    assert.ok(section, `${skill} should include the Search Before Building section`);
    const normalized = normalizeWhitespace(section);
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

test('helper BRANCH stays artifact-only in the branch-aware skill consumers', () => {
  for (const skill of ['qa-only', 'finishing-a-development-branch']) {
    const content = readUtf8(getSkillPath(skill));
    const bashBlock = extractBashBlockUnderHeading(content, 'Preamble (run first)');
    assert.match(content, /\$BRANCH/, `${skill} should use helper BRANCH in artifact selection`);
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

test('workflow fixture coverage uses local fixtures instead of historical docs paths', () => {
  const content = readUtf8(path.join(REPO_ROOT, 'tests/runtime_instruction_contracts.rs'));
  assert.match(content, /tests\/codex-runtime\/fixtures\/workflow-artifacts/);
  assert.doesNotMatch(content, /docs\/featureforge\/specs\/2026-/);
  assert.doesNotMatch(content, /docs\/featureforge\/plans\/2026-/);
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
    for (const command of ['record-contract', 'record-evaluation', 'record-handoff', 'begin', 'note', 'complete', 'reopen', 'transfer']) {
      assertForbidsDirectHelperCommandMutation(content, command, label);
    }
  }

  assertSeparatesCandidateArtifactsFromAuthoritativeMutations(executingPlans, 'skills/executing-plans/SKILL.md');
  assertSeparatesCandidateArtifactsFromAuthoritativeMutations(subagentSkill, 'skills/subagent-driven-development/SKILL.md');
  assertSeparatesCandidateArtifactsFromAuthoritativeMutations(implementerPrompt, 'skills/subagent-driven-development/implementer-prompt.md');
  assertDownstreamMaterialStaysGateAndHarnessAware(reviewSkill, 'skills/requesting-code-review/SKILL.md');
  assertDownstreamMaterialStaysGateAndHarnessAware(qaSkill, 'skills/qa-only/SKILL.md');
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

test('execution workflow skills reference the plan-execution helper contract', () => {
  const planEngReview = readUtf8(getSkillPath('plan-eng-review'));
  assert.doesNotMatch(planEngReview, /featureforge plan execution recommend --plan <approved-plan-path>/);
  assert.match(planEngReview, /Present the runtime-selected execution owner skill as the default path with the approved plan path\./);
  assert.match(planEngReview, /If isolated-agent workflows are unavailable, do not present `featureforge:subagent-driven-development` as an available override\./);
  assert.match(
    planEngReview,
    /If workflow\/operator returns a later phase such as `task_closure_pending`, `document_release_pending`, `final_review_pending`, `qa_pending`, or `ready_for_branch_completion`, follow that reported `phase`, `phase_detail`, `next_action`, and `recommended_command` instead of reopening execution preflight\./,
  );
  assert.doesNotMatch(planEngReview, /review_blocked/);

  const writingPlans = readUtf8(getSkillPath('writing-plans'));
  assert.match(writingPlans, /\*\*Plan Revision:\*\* 1/);
  assert.match(writingPlans, /\*\*Execution Mode:\*\* none/);

  for (const skill of ['subagent-driven-development', 'executing-plans']) {
    const content = readUtf8(getSkillPath(skill));
    assert.match(content, /calls `workflow operator --plan \.\.\.` during preflight/);
    assert.match(
      content,
      /uses `status --plan \.\.\.` only for additional diagnostics when operator output alone is insufficient/,
    );
    assert.match(content, /Provides the approved plan and the execution preflight handoff/);
    assert.match(content, /calls `begin` before starting work on a plan step/);
    assert.match(content, /calls `complete` after each completed step/);
    assert.match(content, /calls `note` when work is interrupted or blocked/);
    assert.match(content, /The approved plan checklist is the execution progress record; do not create or maintain a separate authoritative task tracker\./);
  }
  assert.doesNotMatch(readUtf8(getSkillPath('executing-plans')), /track the work in your platform's task checklist/);
  assert.doesNotMatch(readUtf8(getSkillPath('subagent-driven-development')), /task-tracker checklist/);
  assert.doesNotMatch(readUtf8(getSkillPath('subagent-driven-development')), /Mark task complete in task tracker/);

  const reviewSkill = readUtf8(getSkillPath('requesting-code-review'));
  assert.match(reviewSkill, /rejects final review if the plan has invalid execution state or required unfinished work not truthfully represented/);
  assert.match(reviewSkill, /must fail closed when it detects a missed reopen or stale evidence, but must not call `reopen` itself/);
  assert.match(
    reviewSkill,
    /low-level compatibility\/debug dispatch commands are not normal intent-level progression\./,
  );
  assert.match(reviewSkill, /For plan-routed final review, require the exact approved plan path and exact approved spec path from the current execution preflight handoff or session context\./);
  assert.match(reviewSkill, /Run `featureforge plan contract analyze-plan --spec <approved-spec-path> --plan <approved-plan-path> --format json` before dispatching the reviewer\./);
  assert.match(reviewSkill, /Run `featureforge workflow operator --plan <approved-plan-path>` before dispatching the reviewer\./);
  assert.match(reviewSkill, /If workflow\/operator fails, stop and return to the current execution flow; do not guess the public late-stage route from raw execution state\./);
  assert.match(reviewSkill, /Run `featureforge plan execution status --plan <approved-plan-path>` only when you need extra execution-dirty or strategy-checkpoint diagnostics from the current workflow context\./);
  assert.match(reviewSkill, /If diagnostic status fails when those fields are required, stop and return to the current execution flow; do not dispatch review against guessed plan state\./);
  assert.match(reviewSkill, /When diagnostic status is required, parse `active_task`, `blocking_task`, and `resume_task` from that status JSON\./);
  assert.match(reviewSkill, /When diagnostic status is required, if any of `active_task`, `blocking_task`, or `resume_task` is non-null, stop and return to the current execution flow; final review is only valid when all three are `null`\./);
  assert.match(reviewSkill, /treat workflow\/operator as authoritative for the public late-stage route; status is diagnostic only\./);
  assert.match(reviewSkill, /only request a fresh external final review when workflow\/operator reports `phase=final_review_pending` with `phase_detail=final_review_dispatch_required`\./);
  assert.match(reviewSkill, /After the independent reviewer returns a final-review result, rerun `featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready` and require `phase_detail=final_review_recording_ready` before recording the result with `featureforge plan execution advance-late-stage --plan <approved-plan-path> --reviewer-source <source> --reviewer-id <id> --result pass\|fail --summary-file <final-review-summary>`\./);
  assert.match(reviewSkill, /Pass the exact approved plan path into the reviewer context\. When runtime-owned execution evidence or task-packet context is already available from the current workflow handoff, pass it through as supplemental context; do not make the public flow harvest it manually\./);
  assert.match(
    reviewSkill,
    /Do not use PR metadata or repo default-branch APIs as a fallback; keep the review base aligned with runtime-owned workflow context when available and otherwise restrict fallback to repository-local branch metadata\./,
  );
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
  assert.match(reviewSkill, /CONTRACT_STATE=\$\(printf '%s\\n' "\$ANALYZE_JSON" \| node -e 'const fs = require\("fs"\); const parsed = JSON\.parse\(fs\.readFileSync\(0, "utf8"\)\); process\.stdout\.write\(parsed\.contract_state \|\| ""\)'/);
  assert.match(reviewSkill, /if \[ "\$CONTRACT_STATE" != "valid" \] \|\| \[ "\$PACKET_BUILDABLE_TASKS" != "\$TASK_COUNT" \]; then/);
  assert.match(reviewSkill, /When diagnostic status is required, if any of `active_task`, `blocking_task`, or `resume_task` is non-null, stop and return to the current execution flow; final review is only valid when all three are `null`\./);
  assert.match(reviewSkill, /OPERATOR_JSON=\$\("\$_FEATUREFORGE_BIN" workflow operator --plan "\$APPROVED_PLAN_PATH" --json\)/);
  assert.match(reviewSkill, /if \[ "\$PHASE" != "final_review_pending" \] \|\| \[ "\$PHASE_DETAIL" != "final_review_dispatch_required" \]; then/);
  assert.doesNotMatch(reviewSkill, /REVIEW_DISPATCH_JSON=/);
  assert.doesNotMatch(reviewSkill, /REVIEW_DISPATCH_ACTION=/);
  assert.doesNotMatch(reviewSkill, /DISPATCH_ID=/);
  assert.doesNotMatch(reviewSkill, /REVIEW_DISPATCH_ALLOWED=/);
  assert.doesNotMatch(reviewSkill, /REVIEW_GATE_JSON/);
  assert.doesNotMatch(reviewSkill, /review gate rejected the current execution evidence/);
  assert.match(reviewSkill, /RECORDING_READY_JSON=\$\("\$_FEATUREFORGE_BIN" workflow operator --plan "\$APPROVED_PLAN_PATH" --external-review-result-ready --json\)/);
  assert.match(reviewSkill, /if \[ "\$RECORDING_PHASE_DETAIL" != "final_review_recording_ready" \]; then/);
  assert.match(reviewSkill, /"\$_FEATUREFORGE_BIN" plan execution advance-late-stage --plan "\$APPROVED_PLAN_PATH" --reviewer-source fresh-context-subagent --reviewer-id 019d3550-c932-7bb2-9903-33f68d7c30ca --result pass --summary-file review-summary\.md/);
  assert.doesNotMatch(reviewSkill, /STATUS_JSON=/);
  assert.doesNotMatch(reviewSkill, /TASK_PACKET_CONTEXT_TASK_1=/);

  const finishSkill = readUtf8(getSkillPath('finishing-a-development-branch'));
  assert.match(finishSkill, /rejects branch-completion handoff if the approved plan is execution-dirty or malformed/);
  assert.match(finishSkill, /must not allow branch completion while any checked-off plan step still lacks semantic implementation evidence/);
  assert.match(finishSkill, /If the current work was executed from an approved FeatureForge plan, require the exact approved plan path from the current execution workflow context before presenting completion options\./);
  assert.match(finishSkill, /Run `featureforge workflow operator --plan <approved-plan-path>` and require a branch-completion-ready route before presenting completion options\./);
  assert.match(finishSkill, /If the exact approved plan path is unavailable or workflow\/operator fails, stop and return to the current execution flow instead of guessing\./);
  assert.match(finishSkill, /Use `featureforge plan execution status --plan <approved-plan-path>` only when you need additional diagnostics \(`active_task`, `blocking_task`, `resume_task`, `evidence_path`, checkpoint fingerprints\) to explain a blocker\./);
  assert.match(
    finishSkill,
    /keep the order strict: `featureforge:document-release` -> terminal `featureforge:requesting-code-review` -> `featureforge workflow operator --plan <approved-plan-path>` -> any required `featureforge:qa-only` handoff -> `advance-late-stage` only when operator reports `phase_detail=qa_recording_required` -> rerun `featureforge workflow operator --plan <approved-plan-path>` and follow its next finish command\./,
  );
  assert.match(finishSkill, /If the current work is governed by an approved FeatureForge plan, treat the approved plan's normalized `\*\*QA Requirement:\*\* required\|not-required` metadata as authoritative for workflow-routed finish gating\./);
  assert.match(finishSkill, /Treat the current-branch test-plan artifact as a QA scope\/provenance input only when its `Source Plan`, `Source Plan Revision`, and `Head SHA` match the exact approved plan path, revision, and current branch HEAD from the workflow context\./);
  assert.match(finishSkill, /Match current-branch artifacts by their `\*\*Branch:\*\*` header, not by a filename substring glob, so `my-feature` cannot masquerade as `feature`\./);
  assert.doesNotMatch(finishSkill, /\*-"?\$BRANCH"?-test-plan-\*/);
  assert.match(finishSkill, /For plan-routed completion, use the exact `base_branch` from `featureforge workflow operator --plan <approved-plan-path> --json` instead of redetecting the target branch\./);
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
    /If the operator reports `qa_pending` with `phase_detail=test_plan_refresh_required`, hand control back to `featureforge:plan-eng-review` before QA or branch completion\./,
  );
  assert.match(finishSkill, /gh pr create --base "<base-branch>"/);

  const reviewPrompt = readUtf8(path.join(REPO_ROOT, 'skills/requesting-code-review/code-reviewer.md'));
  assert.match(reviewPrompt, /^# Code Review Briefing Template/m);
  assert.match(reviewPrompt, /This file is the skill-local reviewer briefing template, not the generated agent system prompt\./);
  assert.match(reviewPrompt, /\*\*Approved plan path:\*\* \{APPROVED_PLAN_PATH\}/);
  assert.match(reviewPrompt, /\*\*Execution evidence path:\*\* \{EXECUTION_EVIDENCE_PATH\}/);
  assert.match(reviewPrompt, /dedicated independent reviewer for the terminal whole-diff gate/);
  assert.match(reviewPrompt, /Dedicated Reviewer Receipt Contract/);
  assert.match(reviewPrompt, /include structured receipt-ready metadata in your response/);
  assert.match(reviewPrompt, /`Source Plan`, `Source Plan Revision`, `Strategy Checkpoint Fingerprint`, `Branch`, `Repo`, `Base Branch`, `Head SHA`/);
  assert.match(reviewPrompt, /When approved plan and execution evidence paths are provided, read both artifacts and verify that checked-off plan steps are semantically satisfied by the implementation and explicitly evidenced\./);
  assert.match(reviewPrompt, /When execution evidence documents recorded topology downgrades or other execution deviations, explicitly inspect them and state whether those deviations pass final review\./);
  assert.match(reviewPrompt, /runtime-provided base-branch context from `workflow operator` \(`base_branch`\) and release-lineage routing/);
  assert.doesNotMatch(reviewPrompt, /git symbolic-ref --short refs\/remotes\/origin\/HEAD/);
  assert.doesNotMatch(reviewPrompt, /for candidate in main master/);
  assert.doesNotMatch(reviewPrompt, /BASE_BRANCH_EFFECTIVE=/);
  assert.doesNotMatch(reviewPrompt, /gh pr view --json baseRefName/);

  const subagentReviewPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/code-quality-reviewer-prompt.md'));
  assert.match(subagentReviewPrompt, /APPROVED_PLAN_PATH: \[exact approved plan path for plan-routed final review, otherwise blank\]/);
  assert.match(subagentReviewPrompt, /EXECUTION_EVIDENCE_PATH: \[helper-reported evidence path for plan-routed final review, otherwise blank\]/);
});

test('task-fidelity workflow docs and prompts require packet-backed plan contracts', () => {
  const writingPlans = readUtf8(getSkillPath('writing-plans'));
  assert.match(writingPlans, /Requirement Coverage Matrix/);
  assert.match(writingPlans, /## Execution Strategy/);
  assert.match(writingPlans, /## Dependency Diagram/);
  assert.match(writingPlans, /\*\*QA Requirement:\*\* required \| not-required/);
  assert.match(writingPlans, /\*\*Spec Coverage:\*\*/);
  assert.match(writingPlans, /\*\*Task Outcome:\*\*/);
  assert.match(writingPlans, /\*\*Plan Constraints:\*\*/);
  assert.match(writingPlans, /\*\*Open Questions:\*\* none/);
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
  assert.match(planEngReview, /missing, stale, or non-buildable for the approved plan revision/);
  assert.match(planEngReview, /Requirement Index/);
  assert.match(planEngReview, /Requirement Coverage Matrix/);
  assert.match(planEngReview, /Execution Strategy/);
  assert.match(planEngReview, /Dependency Diagram/);
  assert.match(planEngReview, /tasks with `Open Questions` not equal to `none`/);
  assert.match(planEngReview, /invalid `Files:` block structure/);
  assert.match(planEngReview, /fake-parallel hotspot files/i);
  assert.match(planEngReview, /exact isolated workspace truth/i);
  assert.match(planEngReview, /Does the `Requirement Coverage Matrix` cover every approved requirement without orphaned or over-broad tasks\?/);
  assert.match(planEngReview, /Do `Files:` blocks stay within the minimum file scope needed for the covered requirements, or do they signal file-scope drift that should be split or reapproved\?/);

  const executingPlans = readUtf8(getSkillPath('executing-plans'));
  assert.match(executingPlans, /build the canonical task packet/);
  assert.match(executingPlans, /treat it as the exact task contract for that execution segment/);
  assert.match(executingPlans, /mandatory task-boundary closure loop/i);
  assert.match(
    executingPlans,
    /if workflow\/operator reports `task_review_dispatch_required`, treat it as a compatibility\/debug lane and keep routing through workflow\/operator plus intent-level commands; do not expand the normal closure loop into manual low-level command choreography/i,
  );
  assert.match(
    executingPlans,
    /After all tasks complete and verified:[\s\S]*featureforge:document-release[\s\S]*featureforge:requesting-code-review/,
  );
  assert.match(
    executingPlans,
    /rerun `featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready` and follow its route; the normal closure path is `featureforge plan execution close-current-task --plan <approved-plan-path> --task <n> --review-result pass\|fail --review-summary-file <review-summary> --verification-result pass\|fail\|not-run \[--verification-summary-file <path> when verification ran\]`/i,
  );
  assert.match(
    executingPlans,
    /featureforge plan execution close-current-task --plan <approved-plan-path> --task <n> --review-result pass\|fail --review-summary-file <review-summary> --verification-result pass\|fail\|not-run \[--verification-summary-file <path> when verification ran\]/,
  );
  assert.match(executingPlans, /does not require per-dispatch user-consent prompts/);
  assert.match(executingPlans, /Non-execution ad-hoc delegation still follows normal user-consent policy/);

  const subagentSkill = readUtf8(getSkillPath('subagent-driven-development'));
  assert.match(subagentSkill, /pass the packet verbatim to implementer and reviewers/);
  assert.match(subagentSkill, /If the packet does not answer it, the task is ambiguous and execution must stop or route back to review\./);
  assert.match(subagentSkill, /The coordinator owns every `git commit`, `git merge`, and `git push` for this workflow/);
  assert.match(
    subagentSkill,
    /If workflow\/operator reports `task_review_dispatch_required` or `final_review_dispatch_required`, keep routing through workflow\/operator plus the intent-level commands; do not expand the normal closure loop into low-level dispatch-lineage management\./,
  );
  assert.match(
    subagentSkill,
    /"More tasks remain\?" -> "Use featureforge:document-release for release-readiness before terminal review" \[label="no"\];/,
  );
  assert.match(
    subagentSkill,
    /"Use featureforge:document-release for release-readiness before terminal review" -> "Use featureforge:requesting-code-review for final review gate";/,
  );
  assert.match(
    subagentSkill,
    /Rerun `featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready` and follow its route; the normal closure path is `featureforge plan execution close-current-task --plan <approved-plan-path> --task <n> --review-result pass\|fail --review-summary-file <review-summary> --verification-result pass\|fail\|not-run \[--verification-summary-file <path> when verification ran\]`\./,
  );
  assert.match(
    subagentSkill,
    /featureforge plan execution close-current-task --plan <approved-plan-path> --task <n> --review-result pass\|fail --review-summary-file <review-summary> --verification-result pass\|fail\|not-run \[--verification-summary-file <path> when verification ran\]/,
  );
  assert.match(subagentSkill, /run `verification-before-completion` and collect the verification result inputs needed by `close-current-task`/i);
  assertOrderedSubstrings(executingPlans, 'skills/executing-plans/SKILL.md task-boundary loop', [
    'after review is green, run `verification-before-completion` and collect the verification result inputs needed by `close-current-task`',
    'rerun `featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready` and follow its route; the normal closure path is `featureforge plan execution close-current-task --plan <approved-plan-path> --task <n> --review-result pass|fail --review-summary-file <review-summary> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]`',
    'no exceptions: only after close-current-task succeeds may Task `N+1` begin',
  ]);
  assertOrderedSubstrings(subagentSkill, 'skills/subagent-driven-development/SKILL.md task-boundary loop', [
    'After review is green, run `verification-before-completion` and collect the verification result inputs needed by `close-current-task`.',
    'Rerun `featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready` and follow its route; the normal closure path is `featureforge plan execution close-current-task --plan <approved-plan-path> --task <n> --review-result pass|fail --review-summary-file <review-summary> --verification-result pass|fail|not-run [--verification-summary-file <path> when verification ran]`.',
    'No exceptions: only after close-current-task succeeds may you dispatch Task `N+1`.',
  ]);
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
      /Reviewed-Closure Command Matrix/,
      `${label} should include the reviewed-closure command matrix`,
    );
    assert.match(
      normalized,
      /featureforge workflow operator --plan <approved-plan-path>[\s\S]*authoritative for `phase`, `phase_detail`, `review_state_status`, `next_action`, and `recommended_command`/i,
      `${label} should treat workflow operator as the authoritative routing contract`,
    );
    assert.match(
      normalized,
      /featureforge plan execution status --plan <approved-plan-path>[\s\S]*optional diagnostic detail/i,
      `${label} should describe status as optional diagnostic detail`,
    );
    assert.match(
      content,
      /featureforge plan execution close-current-task --plan <approved-plan-path> --task <n>/,
      `${label} should include the aggregate task-closure command`,
    );
    assert.match(
      content,
      /featureforge workflow operator --plan <approved-plan-path> --external-review-result-ready[\s\S]*featureforge plan execution close-current-task --plan <approved-plan-path> --task <n>/,
      `${label} should require workflow operator readiness before close-current-task`,
    );
    assert.match(
      content,
      /featureforge plan execution repair-review-state --plan <approved-plan-path>/,
      `${label} should include the review-state repair command`,
    );
    assert.match(
      content,
      /featureforge plan execution advance-late-stage --plan <approved-plan-path>/,
      `${label} should include the aggregate late-stage command`,
    );
    assert.match(
      content,
      /featureforge plan execution advance-late-stage --plan <approved-plan-path> --result ready\|blocked --summary-file <release-summary>/,
      `${label} should include the exact release-readiness late-stage command`,
    );
    assert.match(
      content,
      /featureforge plan execution advance-late-stage --plan <approved-plan-path> --reviewer-source <source> --reviewer-id <id> --result pass\|fail --summary-file <final-review-summary>/,
      `${label} should include the exact final-review late-stage command`,
    );
    assert.doesNotMatch(
      content,
      /featureforge plan execution advance-late-stage --plan <approved-plan-path> \.\.\./,
      `${label} should not use a generic advance-late-stage placeholder`,
    );
    assert.match(
      content,
      /Compatibility-only escape hatch: use low-level runtime primitives only when explicitly debugging or preserving compatibility/,
      `${label} should keep low-level runtime primitives as compatibility-only escape hatch guidance`,
    );
    assert.match(
      content,
      /featureforge plan execution advance-late-stage --plan <approved-plan-path> --result pass\|fail --summary-file <qa-report>/,
      `${label} should include the QA recording command through advance-late-stage`,
    );
    assert.match(
      normalized,
      /MUST NOT use the internal task-closure recording service boundary directly[\s\S]*MUST use `close-current-task` for task closure/i,
      `${label} should forbid direct task-closure service usage`,
    );
    assert.match(
      normalized,
      /current(?: reviewed)? closure[\s\S]*superseded[\s\S]*stale-unreviewed/i,
      `${label} should distinguish current, superseded, and stale-unreviewed closure state`,
    );
    assert.match(
      normalized,
      /run `featureforge plan execution repair-review-state --plan <approved-plan-path>` directly[\s\S]*`recommended_command` is authoritative for the immediate reroute[\s\S]*Use `featureforge plan execution status --plan <approved-plan-path>` only when additional diagnostics are required/i,
      `${label} should require repair-review-state plus returned recommended_command sequencing`,
    );
    assert.match(
      normalized,
      /MUST NOT manually edit runtime-owned execution records[\s\S]*MUST NOT manually edit derived markdown artifacts or receipts/i,
      `${label} should explicitly forbid manual edits to runtime-owned records and derived markdown receipts`,
    );
    assert.match(
      content,
      /`task_closure_recording_ready`[\s\S]*`recording_context\.task_number`/,
      `${label} should require task recording_context task_number`,
    );
    assert.match(
      content,
      /`release_readiness_recording_ready`[\s\S]*`recording_context\.branch_closure_id`/,
      `${label} should require release recording_context branch_closure_id`,
    );
    assert.match(
      content,
      /`release_blocker_resolution_required`[\s\S]*`recording_context\.branch_closure_id`/,
      `${label} should require release-blocker recording_context branch_closure_id`,
    );
    assert.match(
      content,
      /`final_review_recording_ready`[\s\S]*`recording_context\.branch_closure_id`/,
      `${label} should require final-review recording_context branch_closure_id`,
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
      /`review_remediation`: required after actionable independent-review findings and before remediation starts\. Runtime records it automatically when reviewable dispatch lineage enters remediation and when remediation reopens execution work\./,
      `${label} should bind review_remediation to runtime-managed review-dispatch lineage`,
    );
    assert.doesNotMatch(
      content,
      /`gate-review` dispatch/,
      `${label} should not describe review_remediation as a gate-review dispatch checkpoint`,
    );
    assert.doesNotMatch(
      normalized,
      /\| [^|]+ \| [^|]+ \| [^|]*featureforge plan execution explain-review-state --plan <approved-plan-path>[^|]* \| [^|]+ \|/i,
      `${label} should not promote explain-review-state into the primary command column`,
    );
    assert.doesNotMatch(
      normalized,
      /\| [^|]+ \| [^|]+ \| [^|]*featureforge plan execution reconcile-review-state --plan <approved-plan-path>[^|]* \| [^|]+ \|/i,
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
      /no (?:code|test) edits?[\s\S]*successful preflight[\s\S]*first `begin`/i,
      `${label} should prohibit code/test edits between successful preflight and first begin`,
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
    assert.match(
      normalized,
      /five-step recovery runbook[\s\S]*workflow operator --plan[\s\S]*factual-only[\s\S]*task-boundary review/i,
      `${label} should keep the five-step recovery runbook with workflow-operator anchoring and factual-only backfill before task-boundary review`,
    );
  }

  const implementerPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/implementer-prompt.md'));
  assert.match(implementerPrompt, /## Task Packet/);
  assert.match(implementerPrompt, /the packet is the authoritative task contract for that execution slice/);
  assert.match(implementerPrompt, /do not reinterpret or weaken requirement statements/);
  assert.match(implementerPrompt, /if the packet says `Open Questions: none` and ambiguity remains, stop and escalate/);
  assert.match(implementerPrompt, /Prepare the change for coordinator-owned git actions; do not create commits, merges, or pushes yourself/);
  assert.doesNotMatch(implementerPrompt, /Commit your work/);

  const specReviewerPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/spec-reviewer-prompt.md'));
  assert.match(specReviewerPrompt, /the exact task packet/);
  assert.match(specReviewerPrompt, /PLAN_DEVIATION_FOUND/);
  assert.match(specReviewerPrompt, /AMBIGUITY_ESCALATION_REQUIRED/);

  const codeQualityPrompt = readUtf8(path.join(REPO_ROOT, 'skills/subagent-driven-development/code-quality-reviewer-prompt.md'));
  assert.match(codeQualityPrompt, /TASK_PACKET/);
  assert.match(codeQualityPrompt, /work outside planned file decomposition/);
  assert.match(codeQualityPrompt, /new files or abstractions outside packet scope/);
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
    assert.match(content, /featureforge repo-safety check --intent write/, `${skill} should run the repo-safety check`);
    assert.match(content, /featureforge repo-safety approve --stage/, `${skill} should document the approval rescue flow`);
    assert.match(content, /featureforge:using-git-worktrees/, `${skill} should route blocked writes to using-git-worktrees`);
    assert.match(content, /branch, the stage, and the blocking `failure_class`/, `${skill} should surface blocked-write diagnostics`);
    assert.match(content, targetPattern, `${skill} should use the correct write target family`);
  }

  const planEngReview = readUtf8(getSkillPath('plan-eng-review'));
  assert.match(planEngReview, /plan-artifact-write/, 'plan-eng-review should gate plan-body writes');
  assert.match(planEngReview, /approval-header-write/, 'plan-eng-review should gate approval-header writes separately');
  assert.doesNotMatch(planEngReview, /repo-file-write/, 'plan-eng-review should not regress to repo-file-write');
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
  assert.match(projectMemory, /Read `authority-boundaries\.md` before broad setup or repair work\./);
  assert.match(projectMemory, /Read `examples\.md` before writing new entries\./);
  assert.match(projectMemory, /Reuse the seed layouts in `references\/` when creating missing files\./);
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
  assert.doesNotMatch(usingFeatureForge, /brainstorming first, then implementation skills/);
  assert.match(
    usingFeatureForge,
    /brainstorming first, then follow the artifact-state workflow: plan-ceo-review -> writing-plans -> plan-fidelity-review -> plan-eng-review -> execution\./,
  );
  assert.match(
    usingFeatureForge,
    /Do NOT jump from brainstorming straight to implementation\. For workflow-routed work, every stage owns the handoff into the next one\./,
  );
  assert.match(
    usingFeatureForge,
    /"Fix this bug" → debugging first, then if it changes FeatureForge product or workflow behavior follow the artifact-state workflow; otherwise continue to the appropriate implementation skill\./,
  );
  assert.match(
    usingFeatureForge,
    /For feature requests, bugfixes that materially change FeatureForge product or workflow behavior, product requests, or workflow-change requests inside a FeatureForge project, route by artifact state instead of skipping ahead based on the user's wording alone\./,
  );
  assert.match(
    usingFeatureForge,
    /If `\$_FEATUREFORGE_BIN` is available and an approved plan path is already known, call `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` directly for routing\. Otherwise call `\$_FEATUREFORGE_BIN workflow status --refresh` only to discover the current approved `plan_path`, then immediately route through workflow\/operator\. Do not route directly from `workflow status` fields\./,
  );
  assert.doesNotMatch(usingFeatureForge, /If the JSON result is not `implementation_ready` and contains a non-empty `next_skill`, use that route as compatibility fallback\./);
  assert.match(
    usingFeatureForge,
    /If the JSON result reports `status` `implementation_ready`, immediately call `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json` using that exact approved plan path\./,
  );
  assert.match(
    usingFeatureForge,
    /Treat workflow\/operator `phase`, `phase_detail`, `review_state_status`, `next_action`, and `recommended_command` as the authoritative public routing contract\./,
  );
  assert.match(
    usingFeatureForge,
    /Treat human-readable receipts and companion markdown artifacts as derived output, not routing authority\./,
  );
  assert.match(
    usingFeatureForge,
    /Treat low-level runtime primitives as compatibility\/debug-only surfaces unless workflow\/operator explicitly routes to them\./,
  );
  assert.doesNotMatch(
    usingFeatureForge,
    /featureforge plan execution recommend --plan <approved-plan-path> --isolated-agents <available\|unavailable> --session-intent <stay\|separate\|unknown> --workspace-prepared <yes\|no\|unknown>/,
  );
  assert.match(
    usingFeatureForge,
    /treat `execution_started` as an executor-resume signal only when workflow\/operator reports `phase` `executing`/i,
  );
  assert.match(
    usingFeatureForge,
    /If workflow\/operator reports a later phase such as `task_closure_pending`, `document_release_pending`, `final_review_pending`, `qa_pending`, or `ready_for_branch_completion`, follow that reported `phase`, `phase_detail`, `next_action`, and `recommended_command` instead of resuming `featureforge:subagent-driven-development` or `featureforge:executing-plans` just because `execution_started` is `yes`\./,
  );
  assert.doesNotMatch(usingFeatureForge, /review_blocked/);
  assert.match(
    usingFeatureForge,
    /If helper calls fail:/,
  );
  assert.match(
    usingFeatureForge,
    /Do not re-derive `phase`, `phase_detail`, readiness, or late-stage precedence from markdown headers\./,
  );
  assert.match(
    usingFeatureForge,
    /Do not invent or continue a parallel manual routing graph\./,
  );
  assert.match(
    usingFeatureForge,
    /If helper routing still cannot be recovered, fail closed to the earlier safe stage \(`featureforge:brainstorming`\) or remain in the current execution flow; do not route directly into implementation or late-stage recording from fallback logic\./,
  );

  const ceoReview = readUtf8(getSkillPath('plan-ceo-review'));
  assert.match(ceoReview, /\*\*The terminal state is invoking writing-plans\.\*\*/);
  assert.match(ceoReview, /Do not draft a plan or offer implementation options from `plan-ceo-review`\./);
  assert.match(ceoReview, /runs `sync --artifact spec`/);

  const engReview = readUtf8(getSkillPath('plan-eng-review'));
  assert.match(engReview, /\*\*The terminal state is presenting the execution preflight handoff with the approved plan path\.\*\*/);
  assert.match(engReview, /plan-eng-review also owns the late refresh-test-plan lane when approved-plan `QA Requirement` is `required` and finish readiness reports `test_plan_artifact_missing`, `test_plan_artifact_malformed`, `test_plan_artifact_stale`, `test_plan_artifact_authoritative_provenance_invalid`, or `test_plan_artifact_generator_mismatch` for the current approved plan revision\./);
  assert.match(engReview, /\*\*QA Requirement:\*\* required \| not-required/);
  assert.match(engReview, /\*\*Head SHA:\*\* \{current-head\}/);
  assert.match(engReview, /This field scopes the QA artifact for testers; it is not the authoritative finish-gate policy source\./);
  assert.match(engReview, /Set `\*\*Head SHA:\*\*` to the current `git rev-parse HEAD` for the branch state that this test-plan artifact covers\./);
  assert.match(engReview, /In that late-stage lane, the terminal state is returning to the finish-gate flow with a regenerated current-branch test-plan artifact, not reopening execution preflight\./);
  assert.match(engReview, /Before presenting the final execution preflight handoff, if `\$_FEATUREFORGE_BIN` is available, call `\$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`\./);
  assert.match(engReview, /Treat workflow\/operator `phase`, `phase_detail`, `review_state_status`, `next_action`, and `recommended_command` as authoritative for public routing\./);
  assert.match(engReview, /If workflow\/operator returns `phase` `executing`, present the normal execution preflight handoff below\./);
  assert.match(engReview, /If workflow\/operator returns a later phase such as `task_closure_pending`, `document_release_pending`, `final_review_pending`, `qa_pending`, or `ready_for_branch_completion`, follow that reported `phase`, `phase_detail`, `next_action`, and `recommended_command` instead of reopening execution preflight\./);
  assert.doesNotMatch(engReview, /review_blocked/);
  assert.match(engReview, /Do not start implementation inside `plan-eng-review`\./);

  const brainstorming = readUtf8(getSkillPath('brainstorming'));
  assert.match(brainstorming, /record the intended spec path with `expect`/);
  assert.match(brainstorming, /"\$_FEATUREFORGE_BIN" workflow expect --artifact spec --path/);
  assert.match(brainstorming, /runs `sync --artifact spec`/);

  const writingPlans = readUtf8(getSkillPath('writing-plans'));
  assert.match(writingPlans, /record the intended plan path with `expect`/);
  assert.match(writingPlans, /"\$_FEATUREFORGE_BIN" workflow expect --artifact plan --path/);
  assert.match(writingPlans, /runs `sync --artifact plan`/);
  assert.doesNotMatch(writingPlans, /Use the execution skill recommended by `featureforge plan execution recommend --plan <approved-plan-path>`/);

  const ceoReviewWithSyncPath = readUtf8(getSkillPath('plan-ceo-review'));
  assert.match(ceoReviewWithSyncPath, /"\$_FEATUREFORGE_BIN" workflow sync --artifact spec --path/);

  const sdd = readUtf8(getSkillPath('subagent-driven-development'));
  assert.match(sdd, /"Have engineering-approved implementation plan\?" \[shape=diamond\];/);
  assert.match(sdd, /"Return to using-featureforge artifact-state routing" \[shape=box\];/);
  assert.match(sdd, /"Have engineering-approved implementation plan\?" -> "Return to using-featureforge artifact-state routing" \[label="no"\];/);
  assert.match(sdd, /"Tasks mostly independent\?" -> "executing-plans" \[label="no - tightly coupled or better handled in one coordinator session"\];/);
  assert.match(sdd, /"More tasks remain\?" -> "Use featureforge:document-release for release-readiness before terminal review" \[label="no"\];/);
  assert.match(sdd, /"Use featureforge:document-release for release-readiness before terminal review" -> "Use featureforge:requesting-code-review for final review gate";/);
  assert.match(sdd, /\[Announce: I'm using the requesting-code-review skill for the final review pass\.\]/);
  assert.match(sdd, /\[Invoke featureforge:requesting-code-review\]/);
  assert.match(sdd, /Those per-task review loops satisfy the "review early" rule during execution/);
  assert.doesNotMatch(sdd, /Dispatch final code reviewer subagent for entire implementation/);
  assert.doesNotMatch(sdd, /\[Dispatch final code-reviewer\]/);

  const requestingReview = readUtf8(getSkillPath('requesting-code-review'));
  assert.match(requestingReview, /For the final cross-task review gate in workflow-routed work/);
  assert.doesNotMatch(requestingReview, /After each task in subagent-driven development/);
  assert.match(requestingReview, /plan contract analyze-plan --spec "\$SOURCE_SPEC_PATH" --plan "\$APPROVED_PLAN_PATH" --format json/);

  const finishSkill = readUtf8(getSkillPath('finishing-a-development-branch'));
  assert.match(finishSkill, /If the current work is not governed by an approved FeatureForge plan, skip this helper-owned finish gate and continue with the normal completion flow\./);
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
  assert.match(documentRelease, /Artifact `pass` is the runtime-rendered form of CLI input `--result ready`\./);
  assert.match(documentRelease, /Do not hand-write or edit this artifact\./);
  assert.match(documentRelease, /renders `\*\*Result:\*\* pass\|blocked` in the derived companion artifact/);
  assert.doesNotMatch(documentRelease, /Allowed `\*\*Result:\*\*` values:(?:.|\n)*- `ready`(?:.|\n)*- `blocked`/i);
  assert.match(
    documentRelease,
    /For workflow-routed work, `BASE_BRANCH` is runtime-owned context from `featureforge workflow operator --plan <approved-plan-path> --json` \(`base_branch`\) and the active release-readiness lineage\. Use that exact value and do not redetect\./,
  );
  assert.match(
    documentRelease,
    /For reviewed-closure late-stage routing, run `featureforge workflow operator --plan <approved-plan-path>` first; workflow\/operator remains authoritative for `phase`, `phase_detail`, `next_action`, and `recommended_command`\./,
  );
  assert.match(documentRelease, /Run `featureforge workflow operator --plan <approved-plan-path>` to confirm the current `phase_detail` before recording release-readiness\./);
  assert.match(documentRelease, /If workflow\/operator reports `phase_detail=branch_closure_recording_required_for_release_readiness`, run `featureforge plan execution advance-late-stage --plan <approved-plan-path>` and rerun workflow\/operator\./);
  assert.match(documentRelease, /When workflow\/operator reports `phase_detail=release_readiness_recording_ready`, run `featureforge plan execution advance-late-stage --plan <approved-plan-path> --result ready\|blocked --summary-file <release-summary>` to record the runtime-owned release-readiness milestone\./);
  assert.match(
    documentRelease,
    /When workflow\/operator reports `phase_detail=release_blocker_resolution_required`, resolve the blocker and then run `featureforge plan execution advance-late-stage --plan <approved-plan-path> --result ready\|blocked --summary-file <release-summary>` to record the updated runtime-owned release-readiness milestone\./,
  );
  assert.match(
    documentRelease,
    /if \[ "\$PHASE_DETAIL" != "release_readiness_recording_ready" \] && \[ "\$PHASE_DETAIL" != "release_blocker_resolution_required" \]; then/,
  );
  assert.doesNotMatch(documentRelease, /if \[ "\$PHASE_DETAIL" != "release_readiness_recording_ready" \]; then/);
  assert.match(documentRelease, /If workflow\/operator reports any other phase or phase_detail, stop and return to the current workflow flow instead of forcing release-readiness recording from stale assumptions\./);
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
  assert.doesNotMatch(qaOnly, /\*\*Generated By:\*\* featureforge:qa-only/);

  const generatedReviewerAgent = readUtf8(path.join(REPO_ROOT, 'agents/code-reviewer.md'));
  assert.match(
    generatedReviewerAgent,
    /runtime-owned base-branch contract as the active workflow guidance: use caller-provided `workflow operator --plan <approved-plan-path> --json` `base_branch` \/ release-lineage context when available/,
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
  assert.match(finishSkill, /If `featureforge:document-release` writes repo files or changes release metadata, treat any earlier code review as stale and loop back through `featureforge:requesting-code-review` before presenting completion options\./);
  assert.match(
    finishSkill,
    /For workflow-routed terminal completion, do not run the terminal review gate in this step\. Run it only after `featureforge:document-release` and before any runtime-routed `featureforge:qa-only` handoff\./,
  );
  assert.match(
    finishSkill,
    /Any required `featureforge:qa-only` handoff is downstream of that terminal final-review pass\. Do not move QA ahead of the post-document-release `featureforge:requesting-code-review` gate\./,
  );
  assert.doesNotMatch(finishSkill, /after `featureforge:document-release` and any required `featureforge:qa-only` handoff are current/);
  assert.doesNotMatch(finishSkill, /after `featureforge:document-release` and any required QA handoff/);

  const routingScenarios = readUtf8(path.join(REPO_ROOT, 'tests/evals/using-featureforge-routing.scenarios.md'));
  assert.match(routingScenarios, /branch-completion language still routes to `requesting-code-review` when no fresh final review artifact exists/i);
  assert.match(routingScenarios, /fresh code-review, QA, and release-readiness artifacts exist/i);

  const readme = readUtf8(path.join(REPO_ROOT, 'README.md'));
  assert.match(readme, /Seven layers matter:/);
  assert.match(
    readme,
    /Completion then flows through \(runtime-owned late-stage sequencing keeps `featureforge:document-release` ahead of terminal `featureforge:requesting-code-review`\):/,
  );
  assert.match(
    readme,
    /`featureforge plan execution rebuild-evidence --plan <approved-plan-path>` is a compatibility\/debug projection-regeneration helper\. It does not mutate authoritative execution truth\./,
  );
  assert.match(
    readme,
    /When workflow\/operator reports stale or missing closure context, run `featureforge plan execution repair-review-state --plan <approved-plan-path>` directly\./,
  );
  assert.match(
    readme,
    /After `repair-review-state`, treat that command's own `recommended_command` as the immediate reroute and complete that follow-up before running any extra command\./,
  );
  assert.doesNotMatch(
    readme,
    /`featureforge plan execution rebuild-evidence --plan <approved-plan-path>` replays rebuildable execution-evidence targets from the current approved plan and refreshes helper-owned closure receipts against the current runtime state\./,
    'README should not present rebuild-evidence refresh as normal progression guidance',
  );
  assert.doesNotMatch(
    readme,
    /each task runs a fresh-context independent review loop until `gate-review` is green/,
    'README should stop teaching gate-review as the task-closure green loop',
  );
  const completionSection = readme.slice(
    readme.indexOf('Completion then flows through'),
    readme.indexOf('## Project Memory'),
  );
  assert.ok(
    completionSection.indexOf('featureforge:document-release')
      < completionSection.indexOf('featureforge:requesting-code-review'),
    'README completion flow should list document-release before requesting-code-review',
  );

  const codexReadme = readUtf8(path.join(REPO_ROOT, 'docs/README.codex.md'));
  assert.match(
    codexReadme,
    /for workflow-routed terminal sequencing, run `featureforge:document-release` before terminal `featureforge:requesting-code-review`, then continue to `featureforge:qa-only` \(when required\) and `featureforge:finishing-a-development-branch`/,
  );
  assert.match(
    codexReadme,
    /compatibility\/debug command boundaries .* must not be required in the normal path; normal progression stays on `workflow operator`, `close-current-task`, and `advance-late-stage`/,
  );
  assert.match(
    codexReadme,
    /`featureforge workflow operator --plan <approved-plan-path>` is the normal routing surface after handoff; use `featureforge plan execution status --plan <approved-plan-path>` only for deeper diagnostics/,
  );

  const copilotReadme = readUtf8(path.join(REPO_ROOT, 'docs/README.copilot.md'));
  assert.match(
    copilotReadme,
    /for workflow-routed terminal sequencing, run `featureforge:document-release` before terminal `featureforge:requesting-code-review`, then continue to `featureforge:qa-only` \(when required\) and `featureforge:finishing-a-development-branch`/,
  );
  assert.match(
    copilotReadme,
    /compatibility\/debug command boundaries .* must not be required in the normal path; normal progression stays on `workflow operator`, `close-current-task`, and `advance-late-stage`/,
  );
  assert.match(
    copilotReadme,
    /`featureforge workflow operator --plan <approved-plan-path>` is the normal routing surface after handoff; use `featureforge plan execution status --plan <approved-plan-path>` only for deeper diagnostics/,
  );

  const lateStageReference = readUtf8(path.join(REPO_ROOT, 'review/late-stage-precedence-reference.md'));
  assert.match(lateStageReference, /Legacy finish-gate compatibility commands are compatibility\/debug boundaries, not normal-path commands\./);
  assert.match(lateStageReference, /low-level `record-\*` commands are compatibility\/debug boundaries and must not be required by normal-path guidance\./);
  assert.match(
    lateStageReference,
    /For workflow-routed terminal sequencing, run `document-release` before terminal `requesting-code-review`\./,
  );
});

test('late-stage precedence reference rows stay in row-level parity with runtime precedence rows and mapped operator outputs', () => {
  const lateStageReference = readUtf8(path.join(REPO_ROOT, 'review/late-stage-precedence-reference.md'));
  const runtimePrecedence = readUtf8(path.join(REPO_ROOT, 'src/workflow/late_stage_precedence.rs'));
  const workflowOperator = readUtf8(path.join(REPO_ROOT, 'src/workflow/operator.rs'));

  const runtimeRows = parseRuntimeLateStageRows(runtimePrecedence);
  const referenceRows = parseLateStageReferenceRows(lateStageReference);

  assert.equal(runtimeRows.length, 8, 'runtime PRECEDENCE_ROWS should define exactly eight late-stage rows');
  assert.equal(referenceRows.length, runtimeRows.length, 'late-stage reference table should mirror runtime row count');

  assert.deepEqual(
    referenceRows.map((row) => ({
      release: row.release,
      review: row.review,
      qa: row.qa,
      phase: row.phase,
      reasonFamily: row.reasonFamily,
    })),
    runtimeRows,
    'late-stage precedence reference rows should stay aligned with runtime PRECEDENCE_ROWS',
  );

  for (const row of referenceRows) {
    const expectedAction = LATE_STAGE_PHASE_TO_ACTION.get(row.phase);
    const expectedSkill = LATE_STAGE_PHASE_TO_SKILL.get(row.phase);
    assert.ok(expectedAction, `phase ${row.phase} should have a canonical next action mapping`);
    assert.ok(expectedSkill, `phase ${row.phase} should have a canonical recommended skill mapping`);
    assert.equal(
      row.nextAction,
      expectedAction,
      `late-stage reference next action should match runtime mapping for phase ${row.phase}`,
    );
    for (const internalActionToken of [
      'advance_late_stage',
      'dispatch_final_review',
      'run_qa',
      'run_finish_review_gate',
      'run_finish_completion_gate',
    ]) {
      assert.doesNotMatch(
        row.nextAction,
        new RegExp(escapeRegex(internalActionToken)),
        `late-stage reference next action should use public wording instead of internal token ${internalActionToken} for ${row.phase}`,
      );
    }
    assert.equal(
      row.recommendedSkill,
      expectedSkill,
      `late-stage reference recommended skill should match runtime mapping for phase ${row.phase}`,
    );
  assert.match(
    workflowOperator,
      /fn next_action_for_context\(context: &OperatorContext\) -> &str \{\s*&context\.operator_next_action\s*\}/s,
      'workflow/operator should surface query-derived next_action directly',
    );
    assert.match(
      workflowOperator,
      new RegExp(`"${escapeRegex(row.phase)}"\\s*=>\\s*\\(\\s*String::from\\("${escapeRegex(expectedSkill)}"\\)`, 's'),
      `operator recommended-skill routing should keep ${row.phase} -> ${expectedSkill}`,
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
      /`using-featureforge` is the human-readable entry router that consults `featureforge workflow` directly from repo-visible artifacts\./,
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
  assert.match(
    releaseNotes,
    /workflow phase --json.*session_entry.*needs_user_choice.*bypassed.*session_entry_gate.*continue_outside_featureforge.*schema_version.*2/is,
    'RELEASE-NOTES.md should enumerate the workflow phase output removals and new schema version',
  );
  assert.match(
    releaseNotes,
    /workflow doctor --json.*session_entry.*needs_user_choice.*bypassed.*session_entry_gate.*continue_outside_featureforge.*schema_version.*2/is,
    'RELEASE-NOTES.md should enumerate the workflow doctor output removals and new schema version',
  );
  assert.match(
    releaseNotes,
    /workflow handoff --json.*session_entry.*needs_user_choice.*bypassed.*session_entry_gate.*continue_outside_featureforge.*schema_version.*2/is,
    'RELEASE-NOTES.md should enumerate the workflow handoff output removals and new schema version',
  );
  assert.match(
    releaseNotes,
    /workflow status --refresh.*needs_user_choice.*bypassed.*session_entry_unresolved.*session_entry_bypassed.*schema_version.*3/is,
    'RELEASE-NOTES.md should enumerate the workflow status output removals and retained route schema version',
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
  assert.match(
    releaseNotes,
    /plan execution status --json.*harness_phase.*next_action.*recommended_command.*recording_context.*diagnostic-only/is,
    'RELEASE-NOTES.md should describe the aligned plan execution status JSON route vocabulary and recording context output contract',
  );
});

test('runtime-remediation regression inventory fixture stays complete', () => {
  const inventory = readUtf8(path.join(REPO_ROOT, 'tests/fixtures/runtime-remediation/README.md'));
  for (const scenario of [
    'FS-01', 'FS-02', 'FS-03', 'FS-04', 'FS-05', 'FS-06',
    'FS-07', 'FS-08', 'FS-09', 'FS-10', 'FS-11', 'FS-12',
  ]) {
    assert.match(
      inventory,
      new RegExp(`\\b${scenario}\\b`),
      `runtime-remediation inventory should include ${scenario}`,
    );
  }
  assert.match(
    inventory,
    /Probe Command Target/i,
    'runtime-remediation inventory should keep parity-probe command targets',
  );
});
