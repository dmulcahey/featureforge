import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { REPO_ROOT, readUtf8 } from './helpers/markdown-test-helpers.mjs';

const MEMORY_DIR = path.join(REPO_ROOT, 'docs/project_notes');
const REQUIRED_FILES = [
  'README.md',
  'bugs.md',
  'decisions.md',
  'key_facts.md',
  'issues.md',
];

function memoryPath(name) {
  return path.join(MEMORY_DIR, name);
}

function readMemory(name) {
  return readUtf8(memoryPath(name));
}

function bulletEntries(name) {
  return readMemory(name)
    .replace(/^# .*\n+/, '')
    .split(/\n(?=- )/)
    .map((entry) => entry.trim())
    .filter(Boolean);
}

test('project memory corpus includes the required repo-visible files', () => {
  assert.equal(fs.existsSync(MEMORY_DIR), true, 'docs/project_notes should exist');

  for (const name of REQUIRED_FILES) {
    assert.equal(fs.existsSync(memoryPath(name)), true, `${name} should exist`);
  }
});

test('project memory README teaches the boundary and maintenance rubric', () => {
  const content = readMemory('README.md');

  assert.match(content, /supportive (?:project )?memory/i, 'README should describe project memory as supportive');
  assert.match(content, /not authoritative|supportive context only/i, 'README should reject authority drift');
  assert.match(content, /approved specs?, approved plans?, execution evidence, review artifacts?, and runtime state/i, 'README should name the higher-authority workflow surfaces');
  assert.match(content, /if project memory conflicts/i, 'README should describe the conflict-resolution rule');
  assert.match(content, /bugs\.md/i, 'README should mention bugs.md');
  assert.match(content, /decisions\.md/i, 'README should mention decisions.md');
  assert.match(content, /key_facts\.md/i, 'README should mention key_facts.md');
  assert.match(content, /issues\.md/i, 'README should mention issues.md');
  assert.match(content, /recurring-only/i, 'README should describe recurring-only bug retention');
  assert.match(content, /breadcrumb/i, 'README should describe breadcrumb-only issue retention');
  assert.match(content, /Last Verified/i, 'README should describe Last Verified refresh guidance');
  assert.match(content, /supersede|annotate/i, 'README should describe conservative decision retention');
});

test('seeded project memory entries carry inspectable provenance', () => {
  for (const entry of bulletEntries('bugs.md')) {
    assert.match(entry, /\n\s*Source:/, 'each bugs.md entry should include a Source marker');
  }

  for (const entry of bulletEntries('decisions.md')) {
    assert.match(entry, /\n\s*Context:/, 'each decisions.md entry should include Context');
    assert.match(entry, /\n\s*Decision:/, 'each decisions.md entry should include Decision');
    assert.match(entry, /\n\s*Alternatives considered:/, 'each decisions.md entry should include Alternatives considered');
    assert.match(entry, /\n\s*Consequence:/, 'each decisions.md entry should include Consequence');
    assert.match(entry, /\n\s*Source:/, 'each decisions.md entry should include Source');
  }

  for (const entry of bulletEntries('issues.md')) {
    assert.match(entry, /\n\s*Source:/, 'each issues.md entry should include a Source marker');
  }

  for (const entry of bulletEntries('key_facts.md')) {
    assert.match(entry, /\n\s*Last Verified:/, 'each key_facts.md entry should include Last Verified');
    assert.match(entry, /\n\s*Source:/, 'each key_facts.md entry should include Source');
    assert.doesNotMatch(entry, /Source:\s*`(?:src|scripts)\//, 'key_facts.md should cite stable repo docs or approved artifacts, not implementation paths');
  }
});

test('project memory avoids tracker drift, authority drift, and obvious secret-like content', () => {
  const combined = REQUIRED_FILES.map(readMemory).join('\n');
  const issues = readMemory('issues.md');

  assert.doesNotMatch(issues, /\bIn Progress\b|\bBlocked\b|\bCompleted\b|\bStatus:\b|^\s*-\s*\[[ xX]\]/m, 'issues.md should stay breadcrumb-only');
  assert.doesNotMatch(combined, /ignore the approved plan|this file is authoritative|route through this file instead|follow the notes in this file instead|always do .* first/i, 'project memory should not contain instruction-authority drift');
  assert.doesNotMatch(combined, /\btoken\b|api key|private key|password/i, 'project memory should not contain obvious secret-like content');
});
