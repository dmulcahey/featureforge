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
  buildOperatorRouteAuthoritySection,
  buildRuntimeRouteReferenceSection,
  buildUsingFeatureForgeShellLines,
  buildUsingFeatureForgeBypassGateSection,
  buildUsingFeatureForgeNormalStackSection,
  ROUTE_LAW_MODE,
  ROUTE_OWNING_GENERATED_SKILLS,
  routeLawModeForSkill,
} from '../../scripts/gen-skill-docs.mjs';

function assertContainsFragments(content, label, fragments) {
  for (const fragment of fragments) {
    assert.ok(
      content.includes(fragment),
      `${label} should include core fragment \`${fragment}\``,
    );
  }
}

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

test('operator route authority snippet delegates executable binding to canonical reference', () => {
  const snippet = buildOperatorRouteAuthoritySection();
  assert.match(snippet, /Reviewed-Closure Route Authority/);
  assert.match(snippet, /workflow\/operator JSON as the route authority/);
  assert.match(snippet, /references\/operator-route-authority\.md/);
  assert.match(snippet, /Follow operator JSON and the canonical route reference/);
  assert.doesNotMatch(snippet, /manually edit runtime-owned execution records, derived markdown projections/);
  assert.doesNotMatch(snippet, /recommended_public_command_argv/);
  assert.doesNotMatch(snippet, /recommended_public_command_template/);
  assert.doesNotMatch(snippet, /recommended_command/);
  assert.doesNotMatch(snippet, /recommended_public_command_template\.input_bindings/);
  assert.doesNotMatch(snippet, /Late-stage aggregate route coverage:/);

  const reference = buildRuntimeRouteReferenceSection();
  assert.match(reference, /## Runtime Route Reference/);
  assert.match(reference, /This skill does not own live workflow routing\./);
  assert.match(reference, /references\/operator-route-authority\.md/);
  assert.doesNotMatch(reference, /recommended_public_command_argv/);
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
  assert.match(baseShell, /refusing non-featureforge public argv/);
  assert.doesNotMatch(baseShell, /\n\s*"\$@"\n/);
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
  assertContainsFragments(usingFeatureForgeTemplate, 'using-featureforge later-phase routing', [
    'operator reports task closure, repair, document release, final review, QA, branch completion, or another diagnostic route',
    'follow only the selected route surface',
    'instead of resuming execution or terminal sequencing from memory',
    '`execution_started` as a resume signal only in that phase',
  ]);
  assert.doesNotMatch(
    usingFeatureForgeTemplate,
    /workflow\/operator JSON reports a later phase|resume execution just because `execution_started` is `yes`/i,
  );

  const lateStageReference = fs.readFileSync(
    new URL('../../review/late-stage-precedence-reference.md', import.meta.url),
    'utf8',
  );
  assertContainsFragments(lateStageReference, 'late-stage precedence reference', [
    'When workflow/operator selects a terminal late-stage lane',
    'execute that selected',
    'Do not use this reference to run a',
    'memorized chain',
  ]);
});

test('generated preambles support full route law and compact route reference modes', () => {
  const basePreamble = generatePreamble({ review: false, routeLawMode: ROUTE_LAW_MODE.FULL });
  const reviewPreamble = generatePreamble({ review: true, routeLawMode: ROUTE_LAW_MODE.FULL });

  for (const preamble of [basePreamble, reviewPreamble]) {
    assert.match(preamble, /## Installed Control Plane/);
    assert.doesNotMatch(preamble, /## Runtime Route Reference/);
    assertContainsFragments(preamble, 'generated preamble installed route law', [
      'Live workflow routing uses only',
      '`$_FEATUREFORGE_BIN`',
      '`./bin/featureforge`',
      '`target/debug/featureforge`',
      '`cargo run`',
      '`$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --json`',
      '`recommended_public_command_argv`',
      '`recommended_public_command_template`',
      '`required_inputs`',
      '`$_FEATUREFORGE_BIN workflow operator --plan <approved-plan-path> --input NAME=VALUE --json`',
      'display-only `recommended_command`',
      '`recommended_command`',
      'no typed executable surface exists',
      'stop and report the route diagnostic',
      'Detailed binding and route-specific stop rules',
      'references/operator-route-authority.md',
      'installed runtime/root cannot be resolved',
    ]);
    assert.doesNotMatch(preamble, /recommended_public_command_argv\[0\] == "featureforge"/);
    assert.doesNotMatch(preamble, /otherwise bind `recommended_public_command_template`/);
    assert.doesNotMatch(preamble, /rerun that operator query with `--input NAME=VALUE`/);
    assert.doesNotMatch(preamble, /replacing argv\[0\]/);
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

  const compactPreamble = generatePreamble({ review: false, routeLawMode: ROUTE_LAW_MODE.REFERENCE });
  assert.doesNotMatch(compactPreamble, /## Installed Control Plane/);
  assert.match(compactPreamble, /## Runtime Route Reference/);
  assert.match(compactPreamble, /This skill does not own live workflow routing\./);
  assert.match(compactPreamble, /`\$_FEATUREFORGE_ROOT\/references\/operator-route-authority\.md`/);
  assert.doesNotMatch(compactPreamble, /recommended_public_command_argv/);
  assert.doesNotMatch(compactPreamble, /recommended_public_command_template/);
  assert.doesNotMatch(compactPreamble, /recommended_command/);
  assert.match(compactPreamble, /## Search Before Building/);

  const noRoutePreamble = generatePreamble({ review: false, routeLawMode: ROUTE_LAW_MODE.NONE });
  assert.doesNotMatch(noRoutePreamble, /## Installed Control Plane/);
  assert.doesNotMatch(noRoutePreamble, /## Runtime Route Reference/);
  assert.match(noRoutePreamble, /## Search Before Building/);

  assert.equal(routeLawModeForSkill('brainstorming'), ROUTE_LAW_MODE.REFERENCE);
  assert.ok(ROUTE_OWNING_GENERATED_SKILLS.length > 0, 'route-owning generated skill set should not be empty');
  for (const skill of ROUTE_OWNING_GENERATED_SKILLS) {
    assert.equal(routeLawModeForSkill(skill), ROUTE_LAW_MODE.FULL, `${skill} should be declared as a route-owning skill`);
  }

  assert.doesNotMatch(basePreamble, /## Contributor Mode/);
  assert.match(reviewPreamble, /## Contributor Mode/);
  assert.match(reviewPreamble, /See `\$_FEATUREFORGE_ROOT\/references\/agent-grounding\.md`/);
  assert.match(reviewPreamble, /Use `\$_FEATUREFORGE_ROOT\/references\/contributor-mode\.md`/);
});
