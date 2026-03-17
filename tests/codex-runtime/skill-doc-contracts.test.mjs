import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import {
  REPO_ROOT,
  SKILLS_DIR,
  listGeneratedSkills,
  readUtf8,
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

test('templates declare exactly one base or review preamble placeholder', () => {
  for (const skill of listGeneratedSkills()) {
    const template = readUtf8(getTemplatePath(skill));
    const hasBase = template.includes('{{BASE_PREAMBLE}}');
    const hasReview = template.includes('{{REVIEW_PREAMBLE}}');
    assert.notEqual(hasBase, hasReview, `${skill} should declare exactly one preamble placeholder`);
  }
});

test('generated preamble bash block includes shared runtime-root, session, and contributor state', () => {
  for (const skill of listGeneratedSkills()) {
    const content = readUtf8(getSkillPath(skill));
    const bashBlock = extractBashBlockUnderHeading(content, 'Preamble (run first)');
    assert.ok(bashBlock, `${skill} should include a preamble bash block`);
    assert.match(bashBlock, /_IS_SUPERPOWERS_RUNTIME_ROOT\(\)/, `${skill} should define runtime-root detection`);
    assert.match(bashBlock, /_SESSIONS=/, `${skill} should track session count`);
    assert.match(bashBlock, /_CONTRIB=/, `${skill} should load contributor state`);
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

test('workflow sequencing test uses local fixtures instead of historical docs paths', () => {
  const content = readUtf8(path.join(REPO_ROOT, 'tests/codex-runtime/test-workflow-sequencing.sh'));
  assert.match(content, /WORKFLOW_FIXTURE_DIR="tests\/codex-runtime\/fixtures\/workflow-artifacts"/);
  assert.doesNotMatch(content, /docs\/superpowers\/specs\/2026-/);
  assert.doesNotMatch(content, /docs\/superpowers\/plans\/2026-/);
});
