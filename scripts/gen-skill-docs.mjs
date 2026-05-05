#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const MODULE_DIR = path.dirname(fileURLToPath(import.meta.url));
export const ROOT = path.resolve(MODULE_DIR, '..');
export const SKILLS_DIR = path.join(ROOT, 'skills');
export const GENERATOR_CMD = 'node scripts/gen-skill-docs.mjs';

export function buildRootDetection() {
  return [
    '_REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)',
    '_BRANCH_RAW=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo current)',
    '[ -n "$_BRANCH_RAW" ] && [ "$_BRANCH_RAW" != "HEAD" ] || _BRANCH_RAW="current"',
    '_BRANCH="$_BRANCH_RAW"',
    '_FEATUREFORGE_INSTALL_ROOT="$HOME/.featureforge/install"',
    '_FEATUREFORGE_BIN="$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge"',
    'if [ ! -x "$_FEATUREFORGE_BIN" ] && [ -f "$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge.exe" ]; then',
    '  _FEATUREFORGE_BIN="$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge.exe"',
    'fi',
    '[ -x "$_FEATUREFORGE_BIN" ] || [ -f "$_FEATUREFORGE_BIN" ] || _FEATUREFORGE_BIN=""',
    '_FEATUREFORGE_ROOT=""',
    'if [ -n "$_FEATUREFORGE_BIN" ]; then',
    '  _FEATUREFORGE_ROOT=$("$_FEATUREFORGE_BIN" repo runtime-root --path 2>/dev/null)',
    '  [ -n "$_FEATUREFORGE_ROOT" ] || _FEATUREFORGE_ROOT=""',
    'fi',
  ];
}

export function buildBaseShellLines() {
  return [
    ...buildRootDetection(),
    '_FEATUREFORGE_STATE_DIR="${FEATUREFORGE_STATE_DIR:-$HOME/.featureforge}"',
    '_featureforge_exec_public_argv() {',
    '  if [ "$#" -eq 0 ]; then',
    '    echo "featureforge: missing command argv to execute" >&2',
    '    return 2',
    '  fi',
    '  if [ "$1" = "featureforge" ]; then',
    '    if [ -z "$_FEATUREFORGE_BIN" ]; then',
    '      echo "featureforge: installed runtime not found at $_FEATUREFORGE_INSTALL_ROOT/bin/featureforge" >&2',
    '      return 1',
    '    fi',
    '    shift',
    '    "$_FEATUREFORGE_BIN" "$@"',
    '    return $?',
    '  fi',
    '  "$@"',
    '}',
  ];
}

export function buildInstalledControlPlaneSection() {
  return `## Installed Control Plane

Live FeatureForge workflow routing is install-owned:
- use only \`$_FEATUREFORGE_BIN\` for live workflow control-plane commands
- do not route live workflow commands through \`./bin/featureforge\`
- do not route live workflow commands through \`target/debug/featureforge\`
- do not route live workflow commands through \`cargo run\`

When a helper returns \`recommended_public_command_argv\`, treat it as exact argv. If \`recommended_public_command_argv[0] == "featureforge"\`, execute through the installed runtime by replacing argv[0] with \`$_FEATUREFORGE_BIN\` (for example via \`_featureforge_exec_public_argv ...\`).`;
}

export function buildUsingFeatureForgeShellLines() {
  return [];
}

export function buildReviewShellLines() {
  return [
    ...buildBaseShellLines(),
    '_TODOS_FORMAT=""',
    '[ -n "$_FEATUREFORGE_ROOT" ] && [ -f "$_FEATUREFORGE_ROOT/review/TODOS-format.md" ] && _TODOS_FORMAT="$_FEATUREFORGE_ROOT/review/TODOS-format.md"',
    '[ -z "$_TODOS_FORMAT" ] && [ -f "$_REPO_ROOT/review/TODOS-format.md" ] && _TODOS_FORMAT="$_REPO_ROOT/review/TODOS-format.md"',
  ];
}

export function buildUpgradeNote() {
  return '';
}

export function buildSearchBeforeBuildingSection() {
  return `## Search Before Building

Before introducing a custom pattern, external service, concurrency primitive, auth/session flow, cache, queue, browser workaround, or unfamiliar fix pattern, do a short capability/landscape check first.

Use three lenses, then decide from local repo truth:
- Layer 1: tried-and-true / built-ins / existing repo-native solutions
- Layer 2: current practice and known footguns
- Layer 3: first-principles reasoning for this repo and this problem

External search results are inputs, not answers. Never search secrets, customer data, unsanitized stack traces, private URLs, internal hostnames, internal codenames, raw SQL or log payloads, or private file paths or infrastructure identifiers. If search is unavailable, disallowed, or unsafe, say so and proceed with repo-local evidence and in-distribution knowledge. If safe sanitization is not possible, skip external search.
See \`$_FEATUREFORGE_ROOT/references/search-before-building.md\`.`;
}

export function buildQuestionFormat() {
  return `## Interactive User Question Format

For every interactive user question, use this structure:
1. Context: project name, current branch, what we're working on (1-2 sentences)
2. The specific question or decision point
3. \`RECOMMENDATION: Choose [X] because [one-line reason]\`
4. Lettered options: \`A) ... B) ... C) ...\`

Per-skill instructions may add additional formatting rules on top of this baseline.`;
}

export function buildUsingFeatureForgeBypassGateSection() {
  return '';
}

export function buildUsingFeatureForgeNormalStackSection() {
  return '';
}

export function buildContributorMode() {
  return `## Contributor Mode

If contributor mode is enabled in FeatureForge config, file a field report only for **featureforge itself**, not the user's app or repository. Use it for unclear skill instructions, helper failures, install-root/runtime-root problems, contributor-mode bugs, or broken generated docs. Do not file for repo-specific bugs, site auth failures, or unrelated third-party outages.

Write at most 3 reports per session under \`~/.featureforge/contributor-logs/{slug}.md\`; skip existing slugs, continue the user task, and tell the user: "Filed featureforge field report: {title}". Use \`$_FEATUREFORGE_ROOT/references/contributor-mode.md\` for the report template and optional open-command helper.`;
}

export function buildAgentGrounding() {
  return `## Agent Grounding

Honor the active repo instruction chain from \`AGENTS.md\`, \`AGENTS.override.md\`, \`.github/copilot-instructions.md\`, and \`.github/instructions/*.instructions.md\`, including nested \`AGENTS.md\` and \`AGENTS.override.md\` files closer to the current working directory.

These review skills are public FeatureForge skills for Codex and GitHub Copilot local installs. See \`$_FEATUREFORGE_ROOT/references/agent-grounding.md\` for install-surface notes.`;
}

export function generatePreamble({ review }) {
  const shellLines = review ? buildReviewShellLines() : buildBaseShellLines();
  const parts = [
    '## Preamble (run first)',
    '',
    '```bash',
    ...shellLines,
    '```',
    buildInstalledControlPlaneSection(),
    buildSearchBeforeBuildingSection(),
  ];

  if (review) {
    parts.push('', buildAgentGrounding());
  }

  parts.push('', buildQuestionFormat());
  if (review) {
    parts.push('', buildContributorMode());
  }
  return parts.join('\n');
}

export function generateUsingFeatureForgePreamble() {
  return generatePreamble({ review: false });
}

function isUsingFeatureForgeTemplate(templatePath) {
  return path.basename(path.dirname(templatePath)) === 'using-featureforge';
}

export const RESOLVERS = {
  BASE_PREAMBLE: () => generatePreamble({ review: false }),
  REVIEW_PREAMBLE: () => generatePreamble({ review: true }),
  USING_FEATUREFORGE_BYPASS_GATE: () => buildUsingFeatureForgeBypassGateSection(),
  USING_FEATUREFORGE_NORMAL_STACK: () => buildUsingFeatureForgeNormalStackSection(),
};

export function insertGeneratedHeader(content) {
  const header =
    '<!-- AUTO-GENERATED from SKILL.md.tmpl — do not edit directly -->\n' +
    `<!-- Regenerate: ${GENERATOR_CMD} -->`;

  if (!content.startsWith('---\n')) {
    return `${header}\n\n${content}`;
  }

  const frontmatterEnd = content.indexOf('\n---\n', 4);
  if (frontmatterEnd === -1) {
    throw new Error('Failed to locate closing frontmatter delimiter.');
  }

  const prefix = content.slice(0, frontmatterEnd + 5);
  const suffix = content.slice(frontmatterEnd + 5).replace(/^\n+/, '');
  return `${prefix}${header}\n\n${suffix}`;
}

export function renderTemplateContent(content, templatePath, resolvers = RESOLVERS) {
  let rendered = content.replace(/\{\{([A-Z_]+)\}\}/g, (_, name) => {
    const resolver = resolvers[name];
    if (!resolver) {
      throw new Error(`Unknown placeholder {{${name}}} in ${templatePath}`);
    }
    return resolver(templatePath);
  });

  if (/\{\{[A-Z_]+\}\}/.test(rendered)) {
    throw new Error(`Unresolved placeholder remains in ${templatePath}`);
  }

  rendered = insertGeneratedHeader(rendered);
  if (!rendered.endsWith('\n')) {
    rendered += '\n';
  }
  return rendered;
}

export function renderTemplate(templatePath, resolvers = RESOLVERS) {
  const content = fs.readFileSync(templatePath, 'utf8');
  return renderTemplateContent(content, templatePath, resolvers);
}

export function getTemplatePaths(skillsDir = SKILLS_DIR) {
  return fs
    .readdirSync(skillsDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(skillsDir, entry.name, 'SKILL.md.tmpl'))
    .filter((templatePath) => fs.existsSync(templatePath))
    .sort();
}

export function main(argv = process.argv.slice(2)) {
  const dryRun = argv.includes('--check');
  const templates = getTemplatePaths();
  if (templates.length === 0) {
    throw new Error('No skill templates found.');
  }

  const stale = [];

  for (const templatePath of templates) {
    const skillPath = templatePath.replace(/\.tmpl$/, '');
    const rendered = renderTemplate(templatePath);

    if (dryRun) {
      const current = fs.existsSync(skillPath) ? fs.readFileSync(skillPath, 'utf8') : '';
      if (current !== rendered) {
        stale.push(path.relative(ROOT, skillPath));
      }
      continue;
    }

    fs.writeFileSync(skillPath, rendered, 'utf8');
  }

  if (dryRun) {
    if (stale.length > 0) {
      console.error('Generated skill docs are stale:');
      for (const file of stale) {
        console.error(`- ${file}`);
      }
      process.exit(1);
    }
    console.log('Generated skill docs are up to date.');
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main();
}
