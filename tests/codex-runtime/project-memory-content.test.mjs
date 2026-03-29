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
  assert.match(readMemory('bugs.md'), /Source:/, 'bugs.md should include Source markers');
  assert.match(readMemory('decisions.md'), /Source:/, 'decisions.md should include Source markers');
  assert.match(readMemory('issues.md'), /Source:/, 'issues.md should include Source markers');
  assert.match(readMemory('key_facts.md'), /Source:|Last Verified:/, 'key_facts.md should include Source or Last Verified markers');
});

test('project memory avoids tracker drift, authority drift, and obvious secret-like content', () => {
  const combined = REQUIRED_FILES.map(readMemory).join('\n');
  const issues = readMemory('issues.md');

  assert.doesNotMatch(issues, /\bIn Progress\b|\bBlocked\b|\bCompleted\b/, 'issues.md should stay breadcrumb-only');
  assert.doesNotMatch(combined, /ignore the approved plan|this file is authoritative|route through this file instead/i, 'project memory should not contain instruction-authority drift');
  assert.doesNotMatch(combined, /\btoken\b|api key|private key|password/i, 'project memory should not contain obvious secret-like content');
});
