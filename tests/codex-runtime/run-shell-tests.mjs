import fs from 'node:fs/promises';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
export const defaultTestDirectory = __dirname;
export const defaultRepoRoot = path.resolve(__dirname, '../..');

function displayPath(testPath, repoRoot) {
  const relativePath = path.relative(repoRoot, testPath);
  if (!relativePath || relativePath.startsWith('..')) {
    return path.basename(testPath);
  }
  return relativePath;
}

export async function discoverShellTests({ testDirectory = defaultTestDirectory } = {}) {
  const entries = await fs.readdir(testDirectory, { withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && /^test-.*\.sh$/.test(entry.name))
    .map((entry) => path.join(testDirectory, entry.name))
    .sort((left, right) => left.localeCompare(right));
}

async function runShellTest(testPath, { repoRoot }) {
  return new Promise((resolve) => {
    const child = spawn('bash', [testPath], {
      cwd: repoRoot,
      env: process.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';
    let settled = false;

    const finish = (result) => {
      if (settled) {
        return;
      }
      settled = true;
      resolve(result);
    };

    child.stdout.on('data', (chunk) => {
      stdout += chunk;
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk;
    });
    child.on('error', (error) => {
      finish({
        path: testPath,
        code: 1,
        stdout,
        stderr: `${stderr}${error.message}\n`,
      });
    });
    child.on('close', (code) => {
      finish({
        path: testPath,
        code: code ?? 1,
        stdout,
        stderr,
      });
    });
  });
}

export async function runShellTests({
  testDirectory = defaultTestDirectory,
  repoRoot = defaultRepoRoot,
} = {}) {
  const tests = await discoverShellTests({ testDirectory });
  const results = await Promise.all(
    tests.map((testPath) => runShellTest(testPath, { repoRoot })),
  );
  return results;
}

export function formatShellTestReport(results, { repoRoot = defaultRepoRoot } = {}) {
  const lines = [];
  let passed = 0;
  let failed = 0;

  for (const result of results) {
    const label = displayPath(result.path, repoRoot);
    if (result.code === 0) {
      passed += 1;
      lines.push(`PASS ${label}`);
      continue;
    }

    failed += 1;
    lines.push(`FAIL ${label} (exit ${result.code})`);
  }

  if (failed > 0) {
    lines.push('');
    for (const result of results) {
      if (result.code === 0) {
        continue;
      }

      const label = displayPath(result.path, repoRoot);
      lines.push(`=== ${label} ===`);
      if (result.stdout) {
        lines.push('--- stdout ---');
        lines.push(result.stdout.trimEnd());
      }
      if (result.stderr) {
        lines.push('--- stderr ---');
        lines.push(result.stderr.trimEnd());
      }
      if (!result.stdout && !result.stderr) {
        lines.push('(no output)');
      }
      lines.push('');
    }
  }

  lines.push(`Summary: ${passed} passed, ${failed} failed`);
  return `${lines.join('\n')}\n`;
}

export function hasShellTestFailures(results) {
  return results.some((result) => result.code !== 0);
}

function parseArgs(argv) {
  let testDirectory = defaultTestDirectory;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === '--directory') {
      testDirectory = path.resolve(argv[index + 1]);
      index += 1;
      continue;
    }

    throw new Error(`Unsupported argument: ${arg}`);
  }

  return { testDirectory };
}

async function main() {
  const { testDirectory } = parseArgs(process.argv.slice(2));
  const repoRoot = testDirectory === defaultTestDirectory ? defaultRepoRoot : testDirectory;
  const results = await runShellTests({ testDirectory, repoRoot });
  process.stdout.write(formatShellTestReport(results, { repoRoot }));
  process.exitCode = hasShellTestFailures(results) ? 1 : 0;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  await main();
}
