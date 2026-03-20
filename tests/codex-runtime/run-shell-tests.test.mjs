import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL, fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, '../..');
const runnerPath = path.join(repoRoot, 'tests/codex-runtime/run-shell-tests.mjs');

function runNode(args, env = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, args, {
      cwd: repoRoot,
      env: { ...process.env, ...env },
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';

    child.stdout.on('data', (chunk) => {
      stdout += chunk;
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk;
    });
    child.on('error', reject);
    child.on('close', (code) => {
      resolve({ code, stdout, stderr });
    });
  });
}

async function writeShellTest(dir, name, body) {
  const filePath = path.join(dir, name);
  await fs.writeFile(filePath, body, { mode: 0o755 });
  await fs.chmod(filePath, 0o755);
  return filePath;
}

test('runner reports discovered shell tests in lexical order', async (t) => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'superpowers-shell-runner-'));
  t.after(async () => {
    await fs.rm(tempDir, { recursive: true, force: true });
  });

  await writeShellTest(
    tempDir,
    'test-zeta.sh',
    '#!/usr/bin/env bash\nset -euo pipefail\necho "zeta"\n',
  );
  await writeShellTest(
    tempDir,
    'test-~.sh',
    '#!/usr/bin/env bash\nset -euo pipefail\necho "tilde"\n',
  );
  await writeShellTest(
    tempDir,
    'test-alpha.sh',
    '#!/usr/bin/env bash\nset -euo pipefail\necho "alpha"\n',
  );

  const module = await import(pathToFileURL(runnerPath).href);
  const results = await module.runShellTests({ testDirectory: tempDir });
  const report = module.formatShellTestReport(results);

  assert.deepEqual(results.map((result) => path.basename(result.path)), [
    'test-alpha.sh',
    'test-zeta.sh',
    'test-~.sh',
  ]);
  assert.ok(report.indexOf('test-alpha.sh') < report.indexOf('test-zeta.sh'));
  assert.ok(report.indexOf('test-zeta.sh') < report.indexOf('test-~.sh'));
});

test('runner exits nonzero when a discovered shell test fails', async (t) => {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'superpowers-shell-runner-'));
  t.after(async () => {
    await fs.rm(tempDir, { recursive: true, force: true });
  });

  await writeShellTest(
    tempDir,
    'test-ok.sh',
    '#!/usr/bin/env bash\nset -euo pipefail\necho "ok"\n',
  );
  await writeShellTest(
    tempDir,
    'test-fail.sh',
    '#!/usr/bin/env bash\nset -euo pipefail\necho "fail"\nexit 7\n',
  );

  const result = await runNode([runnerPath, '--directory', tempDir]);

  assert.notEqual(result.code, 0);
  assert.match(`${result.stdout}${result.stderr}`, /test-fail\.sh/);
});
