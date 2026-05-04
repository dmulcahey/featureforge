import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  extractBashBlockUnderHeading,
  extractSection,
  discoverRepoRoot,
  getGeneratedHeader,
  listGeneratedSkills,
  parseFrontmatter,
  readUtf8,
  REPO_ROOT,
} from './helpers/markdown-test-helpers.mjs';

test('parseFrontmatter returns empty frontmatter and original body when absent', () => {
  const markdown = '# Plain document\n\nNo frontmatter here.\n';
  assert.deepEqual(parseFrontmatter(markdown), {
    frontmatter: {},
    body: markdown,
  });
});

test('parseFrontmatter returns only explicit frontmatter and body fields when present', () => {
  const markdown = '---\nname: example\ndescription: contract\n---\n# Body\n';
  assert.deepEqual(parseFrontmatter(markdown), {
    frontmatter: {
      name: 'example',
      description: 'contract',
    },
    body: '# Body\n',
  });
});

test('extractSection accepts plain or prefixed headings and stops at same or higher level', () => {
  const markdown = [
    '# Top',
    '',
    'Intro',
    '',
    '### Runtime commands',
    'keep this',
    '#### Nested detail',
    'keep nested',
    '### Sibling',
    'drop sibling',
    '## Higher sibling',
    'drop higher',
    '',
  ].join('\n');

  const expected = '### Runtime commands\nkeep this\n#### Nested detail\nkeep nested';
  assert.equal(extractSection(markdown, 'Runtime commands'), expected);
  assert.equal(extractSection(markdown, '### Runtime commands'), expected);
  assert.equal(extractSection(markdown, '## Runtime commands'), '');
});

test('extractBashBlockUnderHeading accepts sh fences through shared section extraction', () => {
  const markdown = [
    '## Runtime commands',
    '',
    '```sh',
    'featureforge --version',
    '```',
    '',
  ].join('\n');

  assert.equal(extractBashBlockUnderHeading(markdown, '## Runtime commands'), 'featureforge --version');
});

test('extractBashBlockUnderHeading accepts empty fences through shared section extraction', () => {
  const markdown = [
    '## Runtime commands',
    '',
    '```',
    'featureforge workflow operator --json',
    '```',
    '',
  ].join('\n');

  assert.equal(
    extractBashBlockUnderHeading(markdown, 'Runtime commands'),
    'featureforge workflow operator --json',
  );
});

test('readUtf8 resolves repo-relative paths from the helper repo root', () => {
  assert.match(readUtf8('README.md'), /FeatureForge/);
  assert.equal(discoverRepoRoot(new URL('./helpers', import.meta.url).pathname), REPO_ROOT);
  assert.equal(discoverRepoRoot(new URL('./helpers/markdown-test-helpers.mjs', import.meta.url).pathname), REPO_ROOT);
  for (const marker of ['Cargo.toml', 'README.md', 'src', 'skills']) {
    assert.ok(fs.existsSync(path.join(REPO_ROOT, marker)), `${marker} marker should exist`);
  }
});

test('generated skill helpers require both template and generated artifact', () => {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'featureforge-skills-'));
  try {
    fs.mkdirSync(path.join(tempDir, 'complete'));
    fs.writeFileSync(path.join(tempDir, 'complete', 'SKILL.md.tmpl'), '---\nname: complete\n---\n');
    fs.writeFileSync(path.join(tempDir, 'complete', 'SKILL.md'), '---\nname: complete\n---\n');

    fs.mkdirSync(path.join(tempDir, 'template-only'));
    fs.writeFileSync(path.join(tempDir, 'template-only', 'SKILL.md.tmpl'), '---\nname: template-only\n---\n');

    fs.mkdirSync(path.join(tempDir, 'generated-only'));
    fs.writeFileSync(path.join(tempDir, 'generated-only', 'SKILL.md'), '---\nname: generated-only\n---\n');

    assert.deepEqual(listGeneratedSkills(tempDir), ['complete']);
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
});

test('getGeneratedHeader returns the skill header with no argument', () => {
  assert.equal(
    getGeneratedHeader(),
    '<!-- AUTO-GENERATED from SKILL.md.tmpl — do not edit directly -->\n<!-- Regenerate: node scripts/gen-skill-docs.mjs -->',
  );
});
