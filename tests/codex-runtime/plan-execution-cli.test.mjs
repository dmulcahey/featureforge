import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import os from 'node:os';
import fs from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '../..');
const require = createRequire(path.join(repoRoot, 'runtime/core-helpers/package.json'));
const { build } = require('esbuild');

const PLAN_REL = 'docs/superpowers/plans/2026-03-17-example-execution-plan.md';
const SPEC_REL = 'docs/superpowers/specs/2026-03-17-example-execution-plan-design.md';

async function bundleCli() {
  const tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'superpowers-plan-execution-cli-'));
  const bundledPath = path.join(tmpDir, 'superpowers-plan-execution.cjs');
  await build({
    entryPoints: [path.join(repoRoot, 'runtime/core-helpers/src/cli/superpowers-plan-execution.ts')],
    bundle: true,
    format: 'cjs',
    platform: 'node',
    outfile: bundledPath,
    write: true,
  });

  return { tmpDir, bundledPath };
}

async function withBundledCli(fn) {
  const { tmpDir, bundledPath } = await bundleCli();
  try {
    await fn(bundledPath);
  } finally {
    await fs.rm(tmpDir, { recursive: true, force: true });
  }
}

function runCli(bundledPath, args, options = {}) {
  const { cwd = repoRoot, env = {} } = options;
  return spawnSync(process.execPath, [bundledPath, ...args], {
    cwd,
    env: {
      ...process.env,
      SUPERPOWERS_RUNTIME_ROOT: repoRoot,
      ...env,
    },
    encoding: 'utf8',
  });
}

async function initRepo(repoDir) {
  await fs.mkdir(repoDir, { recursive: true });
  let result = spawnSync('git', ['init'], { cwd: repoDir, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  result = spawnSync('git', ['config', 'user.name', 'Superpowers Test'], { cwd: repoDir, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  result = spawnSync('git', ['config', 'user.email', 'superpowers-tests@example.com'], { cwd: repoDir, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  await fs.writeFile(path.join(repoDir, 'README.md'), '# plan execution cli fixture\n', 'utf8');
  result = spawnSync('git', ['add', 'README.md'], { cwd: repoDir, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  result = spawnSync('git', ['commit', '-m', 'init'], { cwd: repoDir, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
}

async function writeFile(filePath, contents) {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
  await fs.writeFile(filePath, contents, 'utf8');
}

async function writeApprovedSpec(repoDir) {
  await writeFile(
    path.join(repoDir, SPEC_REL),
    [
      '# Example Execution Plan Design',
      '',
      '**Workflow State:** CEO Approved',
      '**Spec Revision:** 1',
      '**Last Reviewed By:** plan-ceo-review',
      '',
    ].join('\n'),
  );
}

async function writePlan(repoDir, planText) {
  await writeFile(path.join(repoDir, PLAN_REL), planText);
}

test('status reports the bounded clean-plan schema for an execution-clean approved plan', async () => {
  await withBundledCli(async (bundledPath) => {
    const repoDir = await fs.mkdtemp(path.join(os.tmpdir(), 'superpowers-plan-execution-status-'));
    try {
      await initRepo(repoDir);
      await writeApprovedSpec(repoDir);
      await writePlan(
        repoDir,
        [
          '# Example Execution Plan',
          '',
          '**Workflow State:** Engineering Approved',
          '**Plan Revision:** 1',
          '**Execution Mode:** none',
          `**Source Spec:** \`${SPEC_REL}\``,
          '**Source Spec Revision:** 1',
          '**Last Reviewed By:** plan-eng-review',
          '',
          '## Task 1: Core flow',
          '',
          '- [ ] **Step 1: Prepare workspace for execution**',
          '- [ ] **Step 2: Validate the generated output**',
          '',
        ].join('\n'),
      );

      const result = runCli(bundledPath, ['status', '--plan', PLAN_REL], { cwd: repoDir });
      assert.equal(result.status, 0, result.stderr);
      assert.match(result.stdout, /"plan_revision":1/);
      assert.match(result.stdout, /"execution_mode":"none"/);
      assert.match(result.stdout, /"execution_started":"no"/);
      assert.match(result.stdout, /"active_task":null/);
    } finally {
      await fs.rm(repoDir, { recursive: true, force: true });
    }
  });
});

test('recommend prefers subagent-driven-development when task scopes are independent', async () => {
  await withBundledCli(async (bundledPath) => {
    const repoDir = await fs.mkdtemp(path.join(os.tmpdir(), 'superpowers-plan-execution-recommend-'));
    try {
      await initRepo(repoDir);
      await writeApprovedSpec(repoDir);
      await writePlan(
        repoDir,
        [
          '# Example Execution Plan',
          '',
          '**Workflow State:** Engineering Approved',
          '**Plan Revision:** 1',
          '**Execution Mode:** none',
          `**Source Spec:** \`${SPEC_REL}\``,
          '**Source Spec Revision:** 1',
          '**Last Reviewed By:** plan-eng-review',
          '',
          '## Task 1: Parser slice',
          '',
          '**Files:**',
          '- Modify: `src/parser-slice.sh:10-40`',
          '- Test: `bash tests/parser-slice.test.sh`',
          '',
          '- [ ] **Step 1: Build parser slice**',
          '',
          '## Task 2: Formatter slice',
          '',
          '**Files:**',
          '- Modify: `src/formatter-slice.sh:12-36`',
          '- Test: `bash tests/formatter-slice.test.sh`',
          '',
          '- [ ] **Step 1: Build formatter slice**',
          '',
        ].join('\n'),
      );

      const result = runCli(
        bundledPath,
        [
          'recommend',
          '--plan',
          PLAN_REL,
          '--isolated-agents',
          'available',
          '--session-intent',
          'stay',
          '--workspace-prepared',
          'yes',
        ],
        { cwd: repoDir },
      );
      assert.equal(result.status, 0, result.stderr);
      assert.match(result.stdout, /"recommended_skill":"superpowers:subagent-driven-development"/);
      assert.match(result.stdout, /"tasks_independent":"yes"/);
    } finally {
      await fs.rm(repoDir, { recursive: true, force: true });
    }
  });
});
