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

async function bundleCli() {
  const tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'superpowers-config-cli-'));
  const bundledPath = path.join(tmpDir, 'superpowers-config.cjs');
  await build({
    entryPoints: [path.join(repoRoot, 'runtime/core-helpers/src/cli/superpowers-config.ts')],
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

function runCli(bundledPath, args, env = {}) {
  return spawnSync(process.execPath, [bundledPath, ...args], {
    cwd: repoRoot,
    env: { ...process.env, ...env },
    encoding: 'utf8',
  });
}

test('CLI usage errors exit nonzero with usage text', async () => {
  await withBundledCli(async (bundledPath) => {
    const result = runCli(bundledPath, ['get']);

    assert.equal(result.status, 1);
    assert.match(`${result.stdout}${result.stderr}`, /Usage: superpowers-config/);
  });
});

test('CLI set/get/list roundtrip preserves current config semantics', async () => {
  await withBundledCli(async (bundledPath) => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), 'superpowers-config-state-'));
    try {
      let result = runCli(bundledPath, ['set', 'update_check', 'false'], {
        SUPERPOWERS_STATE_DIR: stateDir,
      });
      assert.equal(result.status, 0);

      result = runCli(bundledPath, ['get', 'update_check'], {
        SUPERPOWERS_STATE_DIR: stateDir,
      });
      assert.equal(result.status, 0);
      assert.equal(result.stdout.trim(), 'false');

      await fs.writeFile(
        path.join(stateDir, 'config.yaml'),
        'update_check: false\nupdate_check: true\nsuperpowers_contributor: true\n',
        'utf8',
      );

      result = runCli(bundledPath, ['get', 'update_check'], {
        SUPERPOWERS_STATE_DIR: stateDir,
      });
      assert.equal(result.status, 0);
      assert.equal(result.stdout.trim(), 'true');

      result = runCli(bundledPath, ['list'], {
        SUPERPOWERS_STATE_DIR: stateDir,
      });
      assert.equal(result.status, 0);
      assert.match(result.stdout, /^update_check: false$/m);
      assert.match(result.stdout, /^update_check: true$/m);
      assert.match(result.stdout, /^superpowers_contributor: true$/m);
    } finally {
      await fs.rm(stateDir, { recursive: true, force: true });
    }
  });
});
