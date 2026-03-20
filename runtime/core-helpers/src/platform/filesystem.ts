import fs from 'node:fs';

export function pathExists(filePath: string): boolean {
  return fs.existsSync(filePath);
}
