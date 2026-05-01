#!/usr/bin/env node

import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MODULE_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(MODULE_DIR, '..');
const DEFAULT_COMMAND = 'node --test tests/codex-runtime/*.test.mjs';
const DEFAULT_TIMEOUT_MS = 120_000;
const KILL_GRACE_MS = 5_000;

function parseTimeoutMs(argv) {
  let timeoutMs = DEFAULT_TIMEOUT_MS;
  for (const arg of argv) {
    const match = arg.match(/^--timeout-ms=(\d+)$/);
    if (!match) {
      throw new Error(`unknown argument: ${arg}`);
    }
    timeoutMs = Number.parseInt(match[1], 10);
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
      throw new Error(`invalid --timeout-ms value: ${match[1]}`);
    }
  }
  return timeoutMs;
}

function commandForEnvironment() {
  if (
    process.env.FEATUREFORGE_CODEX_RUNTIME_WRAPPER_TEST === '1'
    && process.env.FEATUREFORGE_CODEX_RUNTIME_TEST_COMMAND
  ) {
    return process.env.FEATUREFORGE_CODEX_RUNTIME_TEST_COMMAND;
  }
  return DEFAULT_COMMAND;
}

function terminate(child) {
  if (!child.pid) return;
  if (process.platform !== 'win32') {
    try {
      process.kill(-child.pid, 'SIGTERM');
      return;
    } catch {
      // Fall through to the direct child kill below when the process group is
      // already gone or process-group signaling is unavailable.
    }
  }
  child.kill('SIGTERM');
}

function forceTerminate(child) {
  if (!child.pid) return;
  if (process.platform !== 'win32') {
    try {
      process.kill(-child.pid, 'SIGKILL');
      return;
    } catch {
      // Fall through to the direct child kill below.
    }
  }
  child.kill('SIGKILL');
}

async function main() {
  const timeoutMs = parseTimeoutMs(process.argv.slice(2));
  const command = commandForEnvironment();
  console.error(`Running ${command} with ${timeoutMs}ms timeout...`);

  const child = spawn(command, {
    cwd: ROOT,
    detached: process.platform !== 'win32',
    env: process.env,
    shell: true,
    stdio: 'inherit',
  });

  let timedOut = false;
  let killTimer;
  const timeout = setTimeout(() => {
    timedOut = true;
    console.error(`Timed out after ${timeoutMs}ms: ${command}`);
    terminate(child);
    killTimer = setTimeout(() => forceTerminate(child), KILL_GRACE_MS);
  }, timeoutMs);

  const exitCode = await new Promise((resolve) => {
    child.on('error', (error) => {
      console.error(`Failed to start codex-runtime test command: ${error.message}`);
      resolve(1);
    });
    child.on('close', (code, signal) => {
      clearTimeout(timeout);
      if (killTimer) clearTimeout(killTimer);
      if (timedOut) {
        resolve(124);
      } else if (code !== null) {
        resolve(code);
      } else {
        console.error(`Codex-runtime test command terminated by signal ${signal}`);
        resolve(1);
      }
    });
  });

  process.exitCode = exitCode;
}

main().catch((error) => {
  console.error(error.message);
  process.exitCode = 1;
});
