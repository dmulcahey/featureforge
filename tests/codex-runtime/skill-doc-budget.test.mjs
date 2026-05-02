import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import {
  REPO_ROOT,
  SKILLS_DIR,
  listGeneratedSkills,
  readUtf8,
} from './helpers/markdown-test-helpers.mjs';

const BASELINE_GENERATED_SKILL_LINES = 7191;
const ENFORCE_MODE = 'enforce';
const HIGH_VOLUME_BUDGETED_SKILLS = [
  'plan-ceo-review',
  'writing-skills',
  'plan-eng-review',
  'subagent-driven-development',
  'finishing-a-development-branch',
  'test-driven-development',
  'systematic-debugging',
  'writing-plans',
  'document-release',
  'qa-only',
  'requesting-code-review',
  'executing-plans',
];

function generatedSkillLineCount(skill) {
  const content = readUtf8(path.join(SKILLS_DIR, skill, 'SKILL.md'));
  return content.endsWith('\n') ? content.split('\n').length - 1 : content.split('\n').length;
}

function readBudgetManifest() {
  const manifestPath = path.join(REPO_ROOT, 'skills/skill-doc-budgets.json');
  return JSON.parse(readUtf8(manifestPath));
}

function collectGeneratedSkillLineCounts() {
  const perSkill = {};
  let total = 0;

  for (const skill of listGeneratedSkills()) {
    const lines = generatedSkillLineCount(skill);
    perSkill[skill] = lines;
    total += lines;
  }

  return { perSkill, total };
}

function formatBudgetReport({ manifest, counts }) {
  const lines = [
    `[skill-doc-budget] mode=${manifest.mode}`,
    `[skill-doc-budget] total=${counts.total} max=${manifest.total_generated_skill_lines_max} baseline=${BASELINE_GENERATED_SKILL_LINES}`,
  ];

  for (const [skill, count] of Object.entries(counts.perSkill)) {
    const budget = manifest.skills[skill]?.max_lines;
    const budgetText = budget === undefined ? 'unbudgeted' : `max=${budget}`;
    lines.push(`[skill-doc-budget] ${skill}=${count} ${budgetText}`);
  }

  return lines.join('\n');
}

function assertBudgetManifestShape(manifest) {
  assert.ok(manifest && typeof manifest === 'object', 'skill-doc-budgets.json should contain a JSON object');
  assert.equal(manifest.mode, ENFORCE_MODE, `manifest mode should be ${ENFORCE_MODE}`);
  assert.equal(
    Number.isInteger(manifest.total_generated_skill_lines_max),
    true,
    'total_generated_skill_lines_max should be an integer',
  );
  assert.ok(
    manifest.total_generated_skill_lines_max > 0,
    'total_generated_skill_lines_max should be positive',
  );
  assert.ok(
    manifest.total_generated_skill_lines_max < BASELINE_GENERATED_SKILL_LINES,
    'total budget should represent a real reduction from the 7,191-line baseline',
  );
  assert.ok(manifest.skills && typeof manifest.skills === 'object', 'manifest should include per-skill budgets');

  for (const skill of HIGH_VOLUME_BUDGETED_SKILLS) {
    assert.ok(manifest.skills[skill], `${skill} should be explicitly budgeted`);
    assert.equal(
      Number.isInteger(manifest.skills[skill].max_lines),
      true,
      `${skill} max_lines should be an integer`,
    );
    assert.ok(manifest.skills[skill].max_lines > 0, `${skill} max_lines should be positive`);
  }
}

test('generated skill budget manifest is machine-readable and enforce mode after compaction', () => {
  const manifest = readBudgetManifest();

  assertBudgetManifestShape(manifest);
});

test('release validation docs keep prompt budget enforcement mandatory and review-owned', () => {
  const testingDoc = readUtf8(path.join(REPO_ROOT, 'docs/testing.md'));

  assert.match(
    testingDoc,
    /node scripts\/run-codex-runtime-tests\.mjs/,
    'docs/testing.md should keep codex-runtime tests in the release validation matrix',
  );
  assert.match(
    testingDoc,
    /node --test tests\/codex-runtime\/skill-doc-budget\.test\.mjs tests\/codex-runtime\/skill-doc-contracts\.test\.mjs/,
    'docs/testing.md should name the prompt-budget and mandatory-law tests together',
  );
  assert.match(
    testingDoc,
    /The budget gate must stay in enforce mode for release work:/,
    'docs/testing.md should mark prompt budgets as a mandatory release gate',
  );
  assert.match(
    testingDoc,
    /Prompt budget enforcement: `tests\/codex-runtime\/skill-doc-budget\.test\.mjs`[\s\S]*Mandatory-law retention: `tests\/codex-runtime\/skill-doc-contracts\.test\.mjs`/,
    'release checklist guidance should distinguish budget failures from missing-law failures',
  );
  assert.match(
    testingDoc,
    /Any change to `skills\/skill-doc-budgets\.json`[\s\S]*requires explicit prompt-budget review/,
    'budget manifest changes should require explicit prompt-budget review',
  );
});

test('generated skill budget report covers top-level generated SKILL.md files only', () => {
  const manifest = readBudgetManifest();
  const counts = collectGeneratedSkillLineCounts();
  const generatedSkills = listGeneratedSkills();

  assert.ok(generatedSkills.length > 0, 'generated skills should be discoverable');
  assert.deepEqual(
    Object.keys(counts.perSkill),
    generatedSkills,
    'budget counts should cover exactly the generated top-level skills',
  );
  assert.equal(
    counts.total,
    Object.values(counts.perSkill).reduce((sum, value) => sum + value, 0),
    'total line count should equal the per-skill sum',
  );

  console.log(formatBudgetReport({ manifest, counts }));

  assert.ok(
    counts.total <= manifest.total_generated_skill_lines_max,
    `generated skill docs total ${counts.total} should be <= ${manifest.total_generated_skill_lines_max}`,
  );

  for (const [skill, budget] of Object.entries(manifest.skills)) {
    assert.ok(counts.perSkill[skill] !== undefined, `${skill} should be a generated skill before enforcing its budget`);
    assert.ok(
      counts.perSkill[skill] <= budget.max_lines,
      `${skill} has ${counts.perSkill[skill]} lines, exceeding budget ${budget.max_lines}`,
    );
  }
});
