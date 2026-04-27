import test from 'node:test';
import assert from 'node:assert/strict';
import {
  extractBashBlockUnderHeading,
  extractSection,
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

test('readUtf8 resolves repo-relative paths from the helper repo root', () => {
  assert.match(readUtf8('README.md'), /FeatureForge/);
  assert.ok(REPO_ROOT.endsWith('featureforge'));
});
