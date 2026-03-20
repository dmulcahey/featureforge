import os from 'node:os';
import path from 'node:path';

export function resolveFromRuntimeRoot(runtimeRoot: string, relativePath: string): string {
  return path.resolve(runtimeRoot, relativePath);
}

export function resolveRuntimeRoot(entryPath: string, runtimeRootOverride?: string): string {
  if (runtimeRootOverride && runtimeRootOverride.length > 0) {
    return path.resolve(runtimeRootOverride);
  }

  return path.resolve(path.dirname(entryPath), '../../..');
}

export function resolveStateDir(env: NodeJS.ProcessEnv, platform = process.platform): string {
  const pathApi = platform === 'win32' ? path.win32 : path;
  const bashStyleHomeMatch = platform === 'win32' ? env.HOME?.match(/^\/([A-Za-z])(?:\/(.*))?$/) : null;
  const uncStyleHomeMatch = platform === 'win32' ? env.HOME?.match(/^\/\/([^/]+)\/([^/]+)(?:\/(.*))?$/) : null;

  if (env.SUPERPOWERS_STATE_DIR && env.SUPERPOWERS_STATE_DIR.length > 0) {
    return env.SUPERPOWERS_STATE_DIR;
  }

  if (platform === 'win32') {
    if (env.USERPROFILE && env.USERPROFILE.length > 0) {
      return pathApi.join(env.USERPROFILE, '.superpowers');
    }

    if (env.HOMEDRIVE && env.HOMEPATH && env.HOMEDRIVE.length > 0 && env.HOMEPATH.length > 0) {
      return pathApi.join(`${env.HOMEDRIVE}${env.HOMEPATH}`, '.superpowers');
    }

    if (bashStyleHomeMatch) {
      const drive = `${bashStyleHomeMatch[1].toUpperCase()}:\\`;
      const rest = bashStyleHomeMatch[2] ? bashStyleHomeMatch[2].replace(/\//g, '\\') : '';
      return pathApi.join(drive, rest, '.superpowers');
    }

    if (uncStyleHomeMatch) {
      const server = uncStyleHomeMatch[1];
      const share = uncStyleHomeMatch[2];
      const rest = uncStyleHomeMatch[3] ? uncStyleHomeMatch[3].replace(/\//g, '\\') : '';
      return pathApi.join(`\\\\${server}\\${share}`, rest, '.superpowers');
    }

    if (env.HOME && env.HOME.length > 0) {
      return pathApi.join(env.HOME, '.superpowers');
    }

    return pathApi.join(os.homedir(), '.superpowers');
  }

  if (env.HOME && env.HOME.length > 0) {
    return pathApi.join(env.HOME, '.superpowers');
  }

  return pathApi.join(os.homedir(), '.superpowers');
}

export function normalizeRelativePath(input: string): string | null {
  if (input.length === 0 || path.isAbsolute(input)) {
    return null;
  }

  const normalizedParts: string[] = [];
  for (const part of input.replace(/\\/g, '/').split('/')) {
    if (part === '' || part === '.') {
      continue;
    }
    if (part === '..') {
      return null;
    }

    normalizedParts.push(part);
  }

  if (normalizedParts.length === 0) {
    return null;
  }

  return normalizedParts.join('/');
}

export function isPathInsideRoot(rootPath: string, candidatePath: string): boolean {
  const relativePath = path.relative(rootPath, candidatePath);
  return relativePath === '' || (!relativePath.startsWith('..') && !path.isAbsolute(relativePath));
}
