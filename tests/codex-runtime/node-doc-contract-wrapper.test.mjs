import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { REPO_ROOT, readUtf8 } from './helpers/markdown-test-helpers.mjs';

const WRAPPER = path.join(REPO_ROOT, 'scripts/run-codex-runtime-tests.mjs');

function shellCommandForNodeEval(source) {
  return `${JSON.stringify(process.execPath)} -e ${JSON.stringify(source)}`;
}

function runWrapper(command, timeoutMs = 1_000) {
  return spawnSync(process.execPath, [WRAPPER, `--timeout-ms=${timeoutMs}`], {
    cwd: REPO_ROOT,
    encoding: 'utf8',
    env: {
      ...process.env,
      FEATUREFORGE_CODEX_RUNTIME_WRAPPER_TEST: '1',
      FEATUREFORGE_CODEX_RUNTIME_TEST_COMMAND: command,
    },
  });
}

test('codex-runtime wrapper preserves the grouped Node contract command', () => {
  const wrapper = readUtf8(WRAPPER);

  assert.match(wrapper, /DEFAULT_COMMAND = 'node --test tests\/codex-runtime\/\*\.test\.mjs'/);
  assert.match(wrapper, /DEFAULT_TIMEOUT_MS = 120_000/);
  assert.match(wrapper, /Timed out after \$\{timeoutMs\}ms/);
});

test('codex-runtime wrapper exits successfully when the grouped command exits', () => {
  const result = runWrapper(shellCommandForNodeEval("console.log('codex-runtime-wrapper-ok')"));

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.match(result.stdout, /codex-runtime-wrapper-ok/);
  assert.match(result.stderr, /Running .* with 1000ms timeout/);
});

test('codex-runtime wrapper fails closed when the grouped command exceeds the timeout', () => {
  const result = runWrapper(shellCommandForNodeEval('setInterval(() => {}, 1000)'), 100);

  assert.equal(result.status, 124, result.stderr || result.stdout);
  assert.match(result.stderr, /Timed out after 100ms/);
});

test('release-facing validation docs use the timeout wrapper for codex-runtime tests', () => {
  const readme = readUtf8(path.join(REPO_ROOT, 'README.md'));
  const testing = readUtf8(path.join(REPO_ROOT, 'docs/testing.md'));

  for (const [label, content] of [
    ['README.md', readme],
    ['docs/testing.md', testing],
  ]) {
    assert.match(
      content,
      /node scripts\/run-codex-runtime-tests\.mjs/,
      `${label} should use the timeout wrapper in release-facing validation commands`,
    );
  }

  assert.match(
    testing,
    /The wrapper runs `node --test tests\/codex-runtime\/\*\.test\.mjs`\s+with a fixed timeout/,
  );
});
