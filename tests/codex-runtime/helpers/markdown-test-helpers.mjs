import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

export const REPO_ROOT = path.resolve(__dirname, '../../..');
export const SKILLS_DIR = path.join(REPO_ROOT, 'skills');

export function listSkillDirs() {
  return fs
    .readdirSync(SKILLS_DIR, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
}

export function listGeneratedSkills() {
  return listSkillDirs().filter((dir) => {
    const tmplPath = path.join(SKILLS_DIR, dir, 'SKILL.md.tmpl');
    return fs.existsSync(tmplPath);
  });
}

export function readUtf8(filePath) {
  const resolvedPath = path.isAbsolute(filePath) ? filePath : path.join(REPO_ROOT, filePath);
  return fs.readFileSync(resolvedPath, 'utf8');
}

export function parseFrontmatter(content) {
  const match = content.match(/^---\n([\s\S]*?)\n---\n/);
  if (!match) {
    return {
      frontmatter: {},
      body: content,
    };
  }

  const frontmatter = {};
  for (const line of match[1].split('\n')) {
    const kv = line.match(/^([A-Za-z0-9_-]+):\s*(.*)$/);
    if (!kv) continue;
    frontmatter[kv[1]] = kv[2];
  }
  return {
    frontmatter,
    body: content.slice(match[0].length),
  };
}

export function getGeneratedHeader(contentOrKind) {
  if (contentOrKind === 'skill') {
    return '<!-- AUTO-GENERATED from SKILL.md.tmpl — do not edit directly -->\n<!-- Regenerate: node scripts/gen-skill-docs.mjs -->';
  }
  if (contentOrKind === 'agent') {
    return '<!-- AUTO-GENERATED from agents/*.instructions.md — do not edit directly -->\n<!-- Regenerate: node scripts/gen-agent-docs.mjs -->';
  }

  const match = contentOrKind.match(/<!-- AUTO-GENERATED from SKILL\.md\.tmpl — do not edit directly -->\n<!-- Regenerate: node scripts\/gen-skill-docs\.mjs -->/);
  return match ? match[0] : null;
}

export function findUnresolvedPlaceholders(content) {
  return content.match(/\{\{[A-Z_]+\}\}/g) ?? [];
}

export function extractSection(content, headingText) {
  const target = String(headingText).trim();
  const targetMatch = target.match(/^(#{1,6})\s+(.+?)\s*#*$/);
  const targetLevel = targetMatch ? targetMatch[1].length : null;
  const targetLabel = (targetMatch ? targetMatch[2] : target).trim();
  const headingPattern = /^(#{1,6})\s+(.+?)\s*#*\s*$/;
  const lines = String(content).split('\n');

  for (let index = 0; index < lines.length; index += 1) {
    const match = lines[index].match(headingPattern);
    if (!match) continue;

    const level = match[1].length;
    const label = match[2].trim();
    if (label !== targetLabel || (targetLevel !== null && level !== targetLevel)) continue;

    let end = lines.length;
    for (let next = index + 1; next < lines.length; next += 1) {
      const nextMatch = lines[next].match(headingPattern);
      if (nextMatch && nextMatch[1].length <= level) {
        end = next;
        break;
      }
    }
    return lines.slice(index, end).join('\n').trimEnd();
  }

  return '';
}

export function extractBashBlockUnderHeading(content, headingText) {
  const section = extractSection(content, headingText);
  if (!section) return '';
  const match = section.match(/```(?:bash|sh)\n([\s\S]*?)\n```/);
  return match ? match[1] : '';
}

export function normalizeWhitespace(value) {
  return value.replace(/\s+/g, ' ').trim();
}

export function countOccurrences(content, literal) {
  if (!literal) return 0;
  return content.split(literal).length - 1;
}
