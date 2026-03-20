import fs from 'node:fs';
import path from 'node:path';

export function pathExists(filePath: string): boolean {
  return fs.existsSync(filePath);
}

export function readTextFileIfExists(filePath: string): string {
  if (!pathExists(filePath)) {
    return '';
  }

  return fs.readFileSync(filePath, 'utf8');
}

export function writeTextFileAtomic(filePath: string, contents: string): void {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  const tempPath = `${filePath}.tmp-${process.pid}-${Date.now()}`;
  fs.writeFileSync(tempPath, contents, 'utf8');
  fs.renameSync(tempPath, filePath);
}
