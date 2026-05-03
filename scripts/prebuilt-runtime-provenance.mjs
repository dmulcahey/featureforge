#!/usr/bin/env node

import childProcess from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MODULE_DIR = path.dirname(fileURLToPath(import.meta.url));
const DEFAULT_ROOT = path.resolve(MODULE_DIR, '..');
const SOURCE_FINGERPRINT_ROOTS = ['Cargo.toml', 'Cargo.lock', 'VERSION', 'src'];
const ROOT_BINARY_PATH = 'bin/featureforge';
const ROOT_BINARY_TARGET = 'darwin-arm64';

function joined(parts, separator) {
  return parts.join(separator);
}

const DENIED_BINARY_TERMS = [
  joined(['record', 'review', 'dispatch'], '-'),
  joined(['gate', 'review'], '-'),
  joined(['gate', 'finish'], '-'),
  joined(['rebuild', 'evidence'], '-'),
  joined(['record', 'branch', 'closure'], '-'),
  joined(['record', 'final', 'review'], '-'),
  joined(['record', 'qa'], '-'),
  joined(['plan', 'fidelity', 'receipt'], '_'),
  joined(['plan', 'fidelity', 'receipt'], '-'),
  `Plan-${joined(['fidelity', 'receipt'], ' ')}`,
  joined(['workflow', joined(['plan', 'fidelity'], '-'), 'record'], ' '),
  joined(['workflow', 'preflight'], ' '),
  joined(['workflow', 'recommend'], ' '),
  joined(['plan', 'execution', 'preflight'], ' '),
  joined(['plan', 'execution', 'recommend'], ' '),
  joined(['execution', 'preflight', 'acceptance'], '-'),
];

function usage() {
  return [
    'usage:',
    '  node scripts/prebuilt-runtime-provenance.mjs source-fingerprint [--repo-root <path>]',
    '  node scripts/prebuilt-runtime-provenance.mjs update --target <key> --binary-path <rel> --checksum-path <rel> --version <version> [--repo-root <path>]',
    '  node scripts/prebuilt-runtime-provenance.mjs verify [--repo-root <path>] [--target <key>] [--skip-help]',
  ].join('\n');
}

function parseArgs(argv) {
  const [command, ...rest] = argv;
  const options = new Map();
  for (let index = 0; index < rest.length; index += 1) {
    const arg = rest[index];
    if (!arg.startsWith('--')) {
      throw new Error(`unexpected argument: ${arg}`);
    }
    const key = arg.slice(2);
    if (key === 'skip-help') {
      options.set(key, true);
      continue;
    }
    const value = rest[index + 1];
    if (value === undefined || value.startsWith('--')) {
      throw new Error(`missing value for --${key}`);
    }
    options.set(key, value);
    index += 1;
  }
  return { command, options };
}

function repoRoot(options) {
  return path.resolve(options.get('repo-root') ?? DEFAULT_ROOT);
}

function relativePath(root, absolutePath) {
  return path.relative(root, absolutePath).split(path.sep).join('/');
}

function collectSourceFiles(root) {
  if (!fs.existsSync(path.join(root, 'Cargo.toml'))) {
    return [];
  }
  const files = [];
  for (const relativeRoot of SOURCE_FINGERPRINT_ROOTS) {
    const absoluteRoot = path.join(root, relativeRoot);
    if (!fs.existsSync(absoluteRoot)) {
      continue;
    }
    const stat = fs.statSync(absoluteRoot);
    if (stat.isFile()) {
      files.push(relativeRoot);
      continue;
    }
    if (!stat.isDirectory()) {
      continue;
    }
    const stack = [absoluteRoot];
    while (stack.length > 0) {
      const current = stack.pop();
      for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
        const absoluteEntry = path.join(current, entry.name);
        if (entry.isDirectory()) {
          stack.push(absoluteEntry);
        } else if (entry.isFile()) {
          files.push(relativePath(root, absoluteEntry));
        }
      }
    }
  }
  return files.sort();
}

function sha256File(absolutePath) {
  const hash = crypto.createHash('sha256');
  hash.update(fs.readFileSync(absolutePath));
  return hash.digest('hex');
}

function sourceFingerprint(root) {
  const files = collectSourceFiles(root);
  const hash = crypto.createHash('sha256');
  hash.update('featureforge-runtime-source-fingerprint-v1\0');
  for (const relative of files) {
    hash.update(relative);
    hash.update('\0');
    hash.update(fs.readFileSync(path.join(root, relative)));
    hash.update('\0');
  }
  return {
    algorithm: 'sha256',
    digest: `sha256:${hash.digest('hex')}`,
    path_count: files.length,
  };
}

function readManifest(root) {
  const manifestPath = path.join(root, 'bin/prebuilt/manifest.json');
  if (!fs.existsSync(manifestPath)) {
    return { runtime_revision: '', targets: {} };
  }
  return JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
}

function writeManifest(root, manifest) {
  const manifestPath = path.join(root, 'bin/prebuilt/manifest.json');
  fs.mkdirSync(path.dirname(manifestPath), { recursive: true });
  fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
}

function updateManifest(root, options) {
  const target = options.get('target');
  const binaryPath = options.get('binary-path');
  const checksumPath = options.get('checksum-path');
  const version = options.get('version');
  if (!target || !binaryPath || !checksumPath || !version) {
    throw new Error('update requires --target, --binary-path, --checksum-path, and --version');
  }

  const absoluteBinary = path.join(root, binaryPath);
  const binarySha256 = sha256File(absoluteBinary);
  const fingerprint = sourceFingerprint(root);
  const manifest = readManifest(root);
  manifest.runtime_revision = version;
  manifest.source_fingerprint = fingerprint.digest;
  manifest.source_fingerprint_algorithm = fingerprint.algorithm;
  manifest.source_fingerprint_path_count = fingerprint.path_count;
  manifest.targets ??= {};
  manifest.targets[target] = {
    binary_path: binaryPath,
    checksum_path: checksumPath,
    binary_sha256: `sha256:${binarySha256}`,
    source_fingerprint: fingerprint.digest,
    source_fingerprint_algorithm: fingerprint.algorithm,
    source_fingerprint_path_count: fingerprint.path_count,
  };
  writeManifest(root, manifest);
}

function fail(failures, message) {
  failures.push(message);
}

function verifyManifest(root, manifest, failures, targetFilter) {
  if (manifest.runtime_revision === undefined || manifest.runtime_revision === '') {
    fail(failures, 'manifest runtime_revision is required');
  }
  if (!manifest.targets || typeof manifest.targets !== 'object') {
    fail(failures, 'manifest targets object is required');
    return;
  }

  const fingerprint = sourceFingerprint(root);
  if (fingerprint.path_count > 0) {
    if (manifest.source_fingerprint !== fingerprint.digest) {
      fail(
        failures,
        `manifest source_fingerprint ${manifest.source_fingerprint ?? '<missing>'} does not match current ${fingerprint.digest}`,
      );
    }
    if (manifest.source_fingerprint_algorithm !== fingerprint.algorithm) {
      fail(failures, 'manifest source_fingerprint_algorithm must be sha256');
    }
    if (manifest.source_fingerprint_path_count !== fingerprint.path_count) {
      fail(
        failures,
        `manifest source_fingerprint_path_count ${manifest.source_fingerprint_path_count ?? '<missing>'} does not match current ${fingerprint.path_count}`,
      );
    }
  }

  const targetEntries = targetFilter === undefined
    ? Object.entries(manifest.targets)
    : [[targetFilter, manifest.targets[targetFilter]]];
  for (const [target, entry] of targetEntries) {
    if (!entry) {
      fail(failures, `manifest target ${target} is required`);
      continue;
    }
    const binaryPath = entry.binary_path;
    const checksumPath = entry.checksum_path;
    if (!binaryPath || !checksumPath) {
      fail(failures, `manifest target ${target} must include binary_path and checksum_path`);
      continue;
    }
    const absoluteBinary = path.join(root, binaryPath);
    const absoluteChecksum = path.join(root, checksumPath);
    if (!fs.existsSync(absoluteBinary)) {
      fail(failures, `${binaryPath}: missing binary`);
      continue;
    }
    if (!fs.existsSync(absoluteChecksum)) {
      fail(failures, `${checksumPath}: missing checksum`);
      continue;
    }
    const actualSha = sha256File(absoluteBinary);
    const expectedManifestSha = `sha256:${actualSha}`;
    if (entry.binary_sha256 !== expectedManifestSha) {
      fail(
        failures,
        `${binaryPath}: manifest binary_sha256 ${entry.binary_sha256 ?? '<missing>'} does not match ${expectedManifestSha}`,
      );
    }
    if (fingerprint.path_count > 0) {
      if (entry.source_fingerprint !== fingerprint.digest) {
        fail(
          failures,
          `${binaryPath}: manifest source_fingerprint ${entry.source_fingerprint ?? '<missing>'} does not match current ${fingerprint.digest}`,
        );
      }
      if (entry.source_fingerprint_algorithm !== fingerprint.algorithm) {
        fail(failures, `${binaryPath}: manifest source_fingerprint_algorithm must be sha256`);
      }
      if (entry.source_fingerprint_path_count !== fingerprint.path_count) {
        fail(
          failures,
          `${binaryPath}: manifest source_fingerprint_path_count ${entry.source_fingerprint_path_count ?? '<missing>'} does not match current ${fingerprint.path_count}`,
        );
      }
    }
    const checksumLine = fs.readFileSync(absoluteChecksum, 'utf8').trim();
    const expectedChecksumLine = `${actualSha}  ${path.basename(binaryPath)}`;
    if (checksumLine !== expectedChecksumLine) {
      fail(
        failures,
        `${checksumPath}: checksum line ${JSON.stringify(checksumLine)} does not match ${JSON.stringify(expectedChecksumLine)}`,
      );
    }
  }
}

function verifyRootBinary(root, manifest, failures, targetFilter) {
  if (targetFilter !== undefined && targetFilter !== ROOT_BINARY_TARGET) {
    return;
  }
  const rootBinary = path.join(root, ROOT_BINARY_PATH);
  if (!fs.existsSync(rootBinary)) {
    return;
  }
  const rootTarget = manifest.targets?.[ROOT_BINARY_TARGET];
  if (!rootTarget?.binary_path) {
    fail(
      failures,
      `${ROOT_BINARY_PATH}: root shipped runtime requires ${ROOT_BINARY_TARGET} manifest target provenance`,
    );
    return;
  }
  const targetBinary = path.join(root, rootTarget.binary_path);
  if (!fs.existsSync(targetBinary)) {
    fail(
      failures,
      `${ROOT_BINARY_PATH}: cannot compare root runtime because ${rootTarget.binary_path} is missing`,
    );
    return;
  }
  const rootSha = sha256File(rootBinary);
  const targetSha = sha256File(targetBinary);
  if (rootSha !== targetSha) {
    fail(
      failures,
      `${ROOT_BINARY_PATH}: root shipped runtime sha256:${rootSha} does not match ${rootTarget.binary_path} sha256:${targetSha}`,
    );
  }
  if (rootTarget.binary_sha256 !== `sha256:${rootSha}`) {
    fail(
      failures,
      `${ROOT_BINARY_PATH}: root shipped runtime hash is not represented by ${ROOT_BINARY_TARGET} manifest binary_sha256`,
    );
  }
}

function auditBinary(root, relative, failures) {
  const absolute = path.join(root, relative);
  if (!fs.existsSync(absolute)) {
    return;
  }
  const contents = fs.readFileSync(absolute).toString('latin1');
  for (const denied of DENIED_BINARY_TERMS) {
    if (contents.includes(denied)) {
      fail(failures, `${relative}: contains denied public/control-plane string ${JSON.stringify(denied)}`);
    }
  }
}

function binaryAuditPaths(manifest) {
  const paths = new Set([ROOT_BINARY_PATH]);
  for (const entry of Object.values(manifest.targets ?? {})) {
    if (entry.binary_path) {
      paths.add(entry.binary_path);
    }
  }
  return [...paths].sort();
}

function runHelp(root, relative, args, failures) {
  const absolute = path.join(root, relative);
  try {
    childProcess.execFileSync(absolute, args, {
      cwd: root,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    return;
  } catch (error) {
    const output = `${error.message ?? ''}\n${error.stdout ?? ''}\n${error.stderr ?? ''}`;
    if (/exec format|bad cpu type|cannot execute binary file|not a valid win32/i.test(output)) {
      try {
        childProcess.execFileSync('file', [absolute], {
          cwd: root,
          encoding: 'utf8',
          stdio: ['ignore', 'pipe', 'pipe'],
        });
        return;
      } catch (fileError) {
        fail(failures, `${relative}: could not run help or inspect with file: ${fileError.message}`);
        return;
      }
    }
    fail(failures, `${relative} ${args.join(' ')} failed: ${output.trim()}`);
  }
}

function verify(root, options) {
  const manifest = readManifest(root);
  const failures = [];
  const targetFilter = options.get('target');
  verifyManifest(root, manifest, failures, targetFilter);
  verifyRootBinary(root, manifest, failures, targetFilter);
  for (const relative of binaryAuditPaths(manifest)) {
    auditBinary(root, relative, failures);
  }
  const shouldCheckRootHelp = targetFilter === undefined || targetFilter === ROOT_BINARY_TARGET;
  if (!options.get('skip-help') && shouldCheckRootHelp && fs.existsSync(path.join(root, 'bin/featureforge'))) {
    runHelp(root, 'bin/featureforge', ['--help'], failures);
    runHelp(root, 'bin/featureforge', ['plan', 'execution', '--help'], failures);
    runHelp(root, 'bin/featureforge', ['workflow', '--help'], failures);
  }
  if (failures.length > 0) {
    console.error('Prebuilt runtime validation failed:');
    for (const failure of failures) {
      console.error(`- ${failure}`);
    }
    process.exit(1);
  }
  console.log('Prebuilt runtime validation passed.');
}

function main() {
  let parsed;
  try {
    parsed = parseArgs(process.argv.slice(2));
    if (!parsed.command) {
      throw new Error('missing command');
    }
    const root = repoRoot(parsed.options);
    switch (parsed.command) {
      case 'source-fingerprint':
        console.log(JSON.stringify(sourceFingerprint(root), null, 2));
        break;
      case 'update':
        updateManifest(root, parsed.options);
        break;
      case 'verify':
        verify(root, parsed.options);
        break;
      default:
        throw new Error(`unknown command: ${parsed.command}`);
    }
  } catch (error) {
    console.error(error.message);
    console.error(usage());
    process.exit(1);
  }
}

main();
