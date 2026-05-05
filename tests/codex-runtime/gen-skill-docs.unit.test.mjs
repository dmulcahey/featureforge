import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {
  insertGeneratedHeader,
  renderTemplateContent,
  buildRootDetection,
  buildBaseShellLines,
  buildReviewShellLines,
  generatePreamble,
  buildUsingFeatureForgeShellLines,
  buildUsingFeatureForgeBypassGateSection,
  buildUsingFeatureForgeNormalStackSection,
} from '../../scripts/gen-skill-docs.mjs';

test('insertGeneratedHeader inserts the generated header after YAML frontmatter', () => {
  const input = ['---', 'name: test', 'description: desc', '---', '', '# Body'].join('\n');
  const output = insertGeneratedHeader(input);

  assert.match(output, /^---\nname: test\ndescription: desc\n---\n<!-- AUTO-GENERATED from SKILL\.md\.tmpl — do not edit directly -->/);
});

test('insertGeneratedHeader throws when YAML frontmatter is unterminated', () => {
  assert.throws(
    () => insertGeneratedHeader(['---', 'name: test', 'description: desc', '# Body'].join('\n')),
    /Failed to locate closing frontmatter delimiter/,
  );
});

test('renderTemplateContent throws on unknown placeholders', () => {
  assert.throws(
    () => renderTemplateContent('{{MISSING_PLACEHOLDER}}\n', '/tmp/skill.md.tmpl'),
    /Unknown placeholder \{\{MISSING_PLACEHOLDER\}\}/,
  );
});

test('renderTemplateContent throws when resolver output leaves unresolved placeholders behind', () => {
  assert.throws(
    () => renderTemplateContent('{{BASE_PREAMBLE}}\n', '/tmp/skill.md.tmpl', {
      BASE_PREAMBLE: () => '{{LEFTOVER}}',
    }),
    /Unresolved placeholder remains/,
  );
});

test('renderTemplateContent always ends generated files with a trailing newline', () => {
  const output = renderTemplateContent(['---', 'name: test', 'description: desc', '---', '', '{{BASE_PREAMBLE}}'].join('\n'), '/tmp/skill.md.tmpl', {
    BASE_PREAMBLE: () => 'PREAMBLE',
  });

  assert.equal(output.endsWith('\n'), true);
});

test('base and review shell builders include their expected contract lines', () => {
  assert.equal(buildBaseShellLines().some((line) => line.includes('_FEATUREFORGE_STATE_DIR=')), true);
  assert.equal(buildBaseShellLines().some((line) => line.includes('_BRANCH=')), true);
  assert.equal(buildReviewShellLines().some((line) => line.includes('_TODOS_FORMAT=')), true);
});

test('shared shell builders delegate runtime-root discovery to the helper contract', () => {
  const rootDetection = buildRootDetection().join('\n');
  const baseShell = buildBaseShellLines().join('\n');

  assert.match(rootDetection, /repo runtime-root --path/);
  assert.match(rootDetection, /\$HOME\/\.featureforge\/install/);
  assert.match(rootDetection, /_FEATUREFORGE_INSTALL_ROOT/);
  assert.match(rootDetection, /_FEATUREFORGE_BIN="\$_FEATUREFORGE_INSTALL_ROOT\/bin\/featureforge"/);
  assert.match(rootDetection, /featureforge\.exe/);
  assert.match(rootDetection, /_FEATUREFORGE_BIN="\$_FEATUREFORGE_INSTALL_ROOT\/bin\/featureforge\.exe"/);
  assert.doesNotMatch(rootDetection, /_REPO_ROOT\/bin\/featureforge/);
  assert.doesNotMatch(rootDetection, /_FEATUREFORGE_ROOT\/bin\/featureforge/);
  assert.doesNotMatch(rootDetection, /\$INSTALL_DIR\/bin\/featureforge/);
  assert.doesNotMatch(rootDetection, /command -v featureforge/);
  assert.doesNotMatch(rootDetection, /_IS_FEATUREFORGE_RUNTIME_ROOT/);
  assert.doesNotMatch(rootDetection, /\.codex\/featureforge/);
  assert.doesNotMatch(rootDetection, /\.copilot\/featureforge/);
  assert.doesNotMatch(rootDetection, /sed -n/);

  // Intentional invariant: generated skill runtime commands must stay on the
  // packaged install binary at ~/.featureforge/install/bin/featureforge.
  // Runtime-root resolution only selects companion files from the install. It
  // must NEVER switch runtime execution back to a root-selected binary or a
  // PATH-selected fallback.
  assert.match(baseShell, /repo runtime-root --path/);
  assert.match(baseShell, /_FEATUREFORGE_STATE_DIR="\$\{FEATUREFORGE_STATE_DIR:-\$HOME\/\.featureforge\}"/);
  assert.match(baseShell, /_featureforge_exec_public_argv\(\)/);
  assert.match(baseShell, /if \[ "\$1" = "featureforge" \]/);
  assert.match(baseShell, /"\$_FEATUREFORGE_BIN" "\$@"/);
  assert.doesNotMatch(baseShell, /repo runtime-root --path.*\|\| true/);
  assert.doesNotMatch(baseShell, /\$_REPO_ROOT\/bin\/featureforge/);
  assert.doesNotMatch(baseShell, /\$_FEATUREFORGE_ROOT\/bin\/featureforge/);
  assert.doesNotMatch(baseShell, /\$_FEATUREFORGE_ROOT\/bin\/featureforge\.exe/);
  assert.doesNotMatch(baseShell, /\$INSTALL_DIR\/bin\/featureforge/);
  assert.doesNotMatch(baseShell, /\$INSTALL_DIR\/bin\/featureforge\.exe/);
  assert.doesNotMatch(baseShell, /\$\{_FEATUREFORGE_BIN:-featureforge\}/);
  assert.doesNotMatch(baseShell, /command -v featureforge/);
  assert.doesNotMatch(baseShell, /featureforge-update-check/);
  assert.doesNotMatch(baseShell, /featureforge-config/);
  assert.doesNotMatch(baseShell, /"\$_FEATUREFORGE_BIN" update-check/);
  assert.doesNotMatch(baseShell, /"\$_FEATUREFORGE_BIN" config get featureforge_contributor/);
});

test('using-featureforge helpers omit the removed bypass gate contract', () => {
  const shellLines = buildUsingFeatureForgeShellLines();
  assert.equal(shellLines.some((line) => line.includes('session-entry/using-featureforge')), false);
  assert.equal(shellLines.some((line) => line.includes('FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY')), false);
  assert.equal(shellLines.some((line) => line.includes('FEATUREFORGE_SPAWNED_SUBAGENT')), false);
  assert.equal(shellLines.some((line) => line.includes('FEATUREFORGE_SPAWNED_SUBAGENT_OPT_IN')), false);

  const bypassGate = buildUsingFeatureForgeBypassGateSection();
  assert.equal(bypassGate.trim(), '');

  const normalStack = buildUsingFeatureForgeNormalStackSection();
  assert.equal(normalStack.trim(), '');
});

test('using-featureforge template keeps canonical late-stage precedence wording', () => {
  const usingFeatureForgeTemplate = fs.readFileSync(
    new URL('../../skills/using-featureforge/SKILL.md.tmpl', import.meta.url),
    'utf8',
  );
  assert.match(
    usingFeatureForgeTemplate,
    /If workflow\/operator reports a later phase such as `task_closure_pending`, `document_release_pending`, `final_review_pending`, `qa_pending`, or `ready_for_branch_completion`, follow that reported `phase`, `phase_detail`, `next_action`, and `recommended_public_command_argv` instead of resuming `featureforge:subagent-driven-development` or `featureforge:executing-plans` just because `execution_started` is `yes`\./,
  );

  const lateStageReference = fs.readFileSync(
    new URL('../../review/late-stage-precedence-reference.md', import.meta.url),
    'utf8',
  );
  assert.match(
    lateStageReference,
    /For workflow-routed terminal sequencing, run `document-release` before terminal `requesting-code-review`\./,
  );
});

test('generated preambles include the shared Search Before Building section for non-router skills only', () => {
  const basePreamble = generatePreamble({ review: false });
  const reviewPreamble = generatePreamble({ review: true });

  for (const preamble of [basePreamble, reviewPreamble]) {
    assert.match(preamble, /## Installed Control Plane/);
    assert.match(preamble, /use only `\$_FEATUREFORGE_BIN` for live workflow control-plane commands/);
    assert.match(preamble, /do not route live workflow commands through `\.\/bin\/featureforge`/);
    assert.match(preamble, /do not route live workflow commands through `target\/debug\/featureforge`/);
    assert.match(preamble, /do not route live workflow commands through `cargo run`/);
    assert.match(
      preamble,
      /If `recommended_public_command_argv\[0\] == "featureforge"`, execute through the installed runtime by replacing argv\[0\] with `\$_FEATUREFORGE_BIN`/,
    );
    assert.match(preamble, /## Search Before Building/);
    assert.match(
      preamble,
      /Before introducing a custom pattern, external service, concurrency primitive, auth\/session flow, cache, queue, browser workaround, or unfamiliar fix pattern, do a short capability\/landscape check first\./,
    );
    assert.match(preamble, /Layer 1: tried-and-true \/ built-ins \/ existing repo-native solutions/);
    assert.match(preamble, /Layer 2: current practice and known footguns/);
    assert.match(preamble, /Layer 3: first-principles reasoning for this repo and this problem/);
    assert.match(preamble, /External search results are inputs, not answers\./);
    assert.match(preamble, /Never search secrets, customer data, unsanitized stack traces, private URLs, internal hostnames, internal codenames, raw SQL or log payloads, or private file paths or infrastructure identifiers\./);
    assert.match(preamble, /If search is unavailable, disallowed, or unsafe, say so and proceed with repo-local evidence and in-distribution knowledge\./);
    assert.match(preamble, /If safe sanitization is not possible, skip external search\./);
    assert.match(preamble, /See `\$_FEATUREFORGE_ROOT\/references\/search-before-building\.md`\./);
  }

  assert.doesNotMatch(basePreamble, /## Contributor Mode/);
  assert.match(reviewPreamble, /## Contributor Mode/);
  assert.match(reviewPreamble, /See `\$_FEATUREFORGE_ROOT\/references\/agent-grounding\.md`/);
  assert.match(reviewPreamble, /Use `\$_FEATUREFORGE_ROOT\/references\/contributor-mode\.md`/);
});
